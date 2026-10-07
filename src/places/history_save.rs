//! Single-writer scheduling for history snapshots, independent of GPUI and disk I/O.

/// Keeps at most one save active and coalesces later snapshots to the newest one.
#[derive(Debug, Default)]
pub struct Queue<T> {
    active: bool,
    pending: Option<T>,
}

impl<T> Queue<T> {
    /// Enqueues a snapshot, returning it when the caller should start the save.
    pub fn enqueue(&mut self, snapshot: T) -> Option<T> {
        if self.active {
            self.pending = Some(snapshot);
            None
        } else {
            self.active = true;
            Some(snapshot)
        }
    }

    /// Completes the active save and returns the newest waiting snapshot, if any.
    pub fn complete(&mut self) -> Option<T> {
        match self.pending.take() {
            Some(snapshot) => Some(snapshot),
            None => {
                self.active = false;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Queue;

    #[test]
    fn one_save_runs_at_a_time_and_latest_pending_snapshot_wins() {
        let mut queue = Queue::default();
        assert_eq!(queue.enqueue("first"), Some("first"));
        assert_eq!(queue.enqueue("stale"), None);
        assert_eq!(queue.enqueue("latest"), None);
        assert_eq!(queue.complete(), Some("latest"));
        assert_eq!(queue.enqueue("after_pending"), None);
        assert_eq!(queue.complete(), Some("after_pending"));
        assert_eq!(queue.complete(), None);
        assert_eq!(queue.enqueue("new_run"), Some("new_run"));
        assert_eq!(queue.complete(), None);
    }
}
