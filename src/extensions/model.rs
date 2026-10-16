//! Pure FIFO request admission and supervised active-slot lifecycle.

use std::collections::VecDeque;
use std::io;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Maximum requests waiting behind the single active helper.
pub const PENDING_CAPACITY: usize = 64;

/// Identity for one admitted request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RequestId(u64);

impl RequestId {
    /// Returns the stable request identity.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// A queued request and its independent cancellation sources.
#[derive(Debug)]
pub struct Job<T> {
    /// Request identity.
    pub id: RequestId,
    /// Request payload.
    pub work: T,
    ticket_cancelled: Arc<AtomicBool>,
    generation_cancelled: Arc<AtomicBool>,
}

impl<T> Job<T> {
    /// Returns whether either the ticket or its generation has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.ticket_cancelled.load(Ordering::Acquire)
            || self.generation_cancelled.load(Ordering::Acquire)
    }

    /// Returns a ticket-local cancellation token for the supervisor.
    pub fn ticket_token(&self) -> Arc<AtomicBool> {
        self.ticket_cancelled.clone()
    }

    /// Returns the caller-owned navigation-generation cancellation token.
    pub fn generation_token(&self) -> Arc<AtomicBool> {
        self.generation_cancelled.clone()
    }
}

/// Result of cancelling a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cancellation {
    /// The request was removed before starting a child.
    Queued,
    /// Active work was signalled and retains the active slot until cleanup.
    Active,
    /// No request with this identity remains.
    Unknown,
}

/// FIFO model enforcing one active request and bounded pending capacity.
#[derive(Debug)]
pub struct QueueModel<T> {
    next_id: u64,
    pending: VecDeque<Job<T>>,
    active: Option<Job<T>>,
}

impl<T> Default for QueueModel<T> {
    fn default() -> Self {
        Self {
            next_id: 1,
            pending: VecDeque::new(),
            active: None,
        }
    }
}

impl<T> QueueModel<T> {
    /// Admits work unless all pending slots are occupied.
    pub fn submit(
        &mut self,
        work: T,
        generation_cancelled: Arc<AtomicBool>,
    ) -> io::Result<(RequestId, Arc<AtomicBool>)> {
        self.pending.retain(|job| !job.is_cancelled());
        if self.pending.len() == PENDING_CAPACITY {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "extension request queue is full",
            ));
        }
        let id = RequestId(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| io::Error::other("extension request identity exhausted"))?;
        let ticket_cancelled = Arc::new(AtomicBool::new(false));
        self.pending.push_back(Job {
            id,
            work,
            ticket_cancelled: ticket_cancelled.clone(),
            generation_cancelled,
        });
        Ok((id, ticket_cancelled))
    }

    /// Promotes the first noncancelled pending request when the slot is free.
    pub fn activate_next(&mut self) -> Option<&Job<T>> {
        if self.active.is_none() {
            while let Some(job) = self.pending.pop_front() {
                if !job.is_cancelled() {
                    self.active = Some(job);
                    break;
                }
            }
        }
        self.active.as_ref()
    }

    /// Returns the current active request without releasing its slot.
    pub fn active(&self) -> Option<&Job<T>> {
        self.active.as_ref()
    }

    /// Releases the active slot only for the matching reaped request.
    pub fn release(&mut self, id: RequestId) -> io::Result<Job<T>> {
        if self.active.as_ref().is_none_or(|job| job.id != id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot release a nonactive extension request",
            ));
        }
        Ok(self.active.take().expect("active request was just checked"))
    }

    /// Cancels queued work immediately or signals active work without freeing its slot.
    pub fn cancel(&mut self, id: RequestId) -> Cancellation {
        if let Some(index) = self.pending.iter().position(|job| job.id == id) {
            let job = self
                .pending
                .remove(index)
                .expect("queued index was just found");
            job.ticket_cancelled.store(true, Ordering::Release);
            return Cancellation::Queued;
        }
        if let Some(job) = self.active.as_ref().filter(|job| job.id == id) {
            job.ticket_cancelled.store(true, Ordering::Release);
            return Cancellation::Active;
        }
        Cancellation::Unknown
    }

    /// Signals cancellation for all work during supervisor shutdown.
    pub fn cancel_all(&mut self) -> Vec<Job<T>> {
        let queued: Vec<_> = self.pending.drain(..).collect();
        for job in &queued {
            job.ticket_cancelled.store(true, Ordering::Release);
        }
        if let Some(active) = &self.active {
            active.ticket_cancelled.store(true, Ordering::Release);
        }
        queued
    }

    /// Number of queued requests.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Whether one request currently owns the active slot.
    pub fn has_active(&self) -> bool {
        self.active.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::{Cancellation, PENDING_CAPACITY, QueueModel};
    use std::io;
    use std::sync::{Arc, atomic::AtomicBool};

    fn generation() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    #[test]
    fn queue_enforces_fifo_capacity_and_queued_cancellation() {
        let mut queue = QueueModel::default();
        let (first, _) = queue.submit(1, generation()).unwrap();
        let (cancelled, _) = queue.submit(2, generation()).unwrap();
        let (third, _) = queue.submit(3, generation()).unwrap();
        assert_eq!(queue.cancel(cancelled), Cancellation::Queued);
        assert_eq!(queue.activate_next().unwrap().id, first);
        assert_eq!(queue.release(first).unwrap().work, 1);
        assert_eq!(queue.activate_next().unwrap().id, third);
        queue.release(third).unwrap();
        for index in 0..PENDING_CAPACITY {
            queue.submit(index, generation()).unwrap();
        }
        assert_eq!(
            queue.submit(999, generation()).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn active_cancellation_retains_slot_until_matching_release() {
        let mut queue = QueueModel::default();
        let generation_cancelled = generation();
        let (first, ticket) = queue
            .submit("active", generation_cancelled.clone())
            .unwrap();
        queue.activate_next();
        assert_eq!(queue.cancel(first), Cancellation::Active);
        assert!(ticket.load(std::sync::atomic::Ordering::Acquire));
        assert!(!generation_cancelled.load(std::sync::atomic::Ordering::Acquire));
        assert!(queue.has_active());
        assert!(queue.release(super::RequestId(first.get() + 1)).is_err());
        assert!(queue.has_active());
        queue.release(first).unwrap();
        assert!(!queue.has_active());
    }

    #[test]
    fn shutdown_cancels_queued_and_active_but_keeps_active_until_cleanup() {
        let mut queue = QueueModel::default();
        let (active_id, active_ticket) = queue.submit("active", generation()).unwrap();
        let (queued_id, queued_ticket) = queue.submit("queued", generation()).unwrap();
        queue.activate_next();
        let cancelled = queue.cancel_all();
        assert_eq!(cancelled.len(), 1);
        assert_eq!(cancelled[0].id, queued_id);
        assert!(queued_ticket.load(std::sync::atomic::Ordering::Acquire));
        assert!(active_ticket.load(std::sync::atomic::Ordering::Acquire));
        assert!(queue.has_active());
        assert_eq!(queue.active().unwrap().id, active_id);
        queue.release(active_id).unwrap();
        assert!(!queue.has_active());
    }

    #[test]
    fn externally_cancelled_pending_work_is_removed_before_capacity_check() {
        let mut queue = QueueModel::default();
        let (_, ticket) = queue.submit(usize::MAX, generation()).unwrap();
        ticket.store(true, std::sync::atomic::Ordering::Release);
        for index in 0..PENDING_CAPACITY {
            queue.submit(index, generation()).unwrap();
        }
        assert_eq!(queue.pending_len(), PENDING_CAPACITY);
    }

    #[test]
    fn generation_cancellation_skips_work_before_start() {
        let mut queue = QueueModel::default();
        let stale_generation = generation();
        queue.submit("stale", stale_generation.clone()).unwrap();
        let (fresh, _) = queue.submit("fresh", generation()).unwrap();
        stale_generation.store(true, std::sync::atomic::Ordering::Release);
        assert_eq!(queue.activate_next().unwrap().id, fresh);
    }
}
