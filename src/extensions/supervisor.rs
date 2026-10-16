//! Shared bounded supervisor for package-column requests.

use std::io;
use std::process::Command;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
};
use std::thread::{self, JoinHandle};

use crate::data_providers::{ApplicabilityContext, FieldKind, ResultState, Subject, Unit, Value};
use crate::location::Location;

use super::Package;
use super::model::QueueModel;
use super::protocol::{Demand, FieldType, Init};

mod process;

/// Work selected by the host for one declared package column.
#[derive(Clone, Debug)]
pub struct ColumnWork {
    /// Validated, unevaluated package retained for the request lifetime.
    pub package: Arc<Package>,
    /// Declared column contribution to prepare.
    pub contribution: crate::data_providers::ContributionId,
    /// Original typed source identity.
    pub source: Location,
    /// Captured display name; never used for path resolution.
    pub name: String,
    /// Captured directory fact.
    pub is_dir: bool,
    /// Navigation generation token from the metadata owner.
    pub generation_cancelled: Arc<AtomicBool>,
}

#[derive(Clone, Debug)]
/// Internal queued request passed to the platform process backend.
pub struct Pending {
    init: Init,
    source: Location,
    response: Sender<ResultState<Value>>,
}

struct State {
    queue: Mutex<QueueModel<Pending>>,
    wake: Condvar,
    closing: AtomicBool,
    finished: Mutex<bool>,
    finished_wake: Condvar,
    launcher: Arc<dyn Fn() -> io::Result<Command> + Send + Sync>,
    fixture_protocol_stderr: bool,
}

struct Controller {
    state: Arc<State>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for Controller {
    fn drop(&mut self) {
        request_shutdown(&self.state);
    }
}

/// Shared handle to the one-request-at-a-time package worker.
#[derive(Clone)]
pub struct Supervisor {
    controller: Arc<Controller>,
}

/// A result ticket whose drop cancels only this request.
pub struct Ticket {
    receiver: Option<Receiver<ResultState<Value>>>,
    cancelled: Arc<AtomicBool>,
    completed: bool,
}

impl Ticket {
    /// Signals this request's cancellation token.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Waits for completion; call from a background task, never the UI thread.
    pub fn wait(mut self) -> io::Result<ResultState<Value>> {
        let result = self
            .receiver
            .take()
            .expect("ticket receiver is present until wait")
            .recv()
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "extension worker stopped"));
        self.completed = true;
        result
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        if !self.completed {
            self.cancelled.store(true, Ordering::Release);
        }
    }
}

impl Supervisor {
    /// Starts the shared background supervisor and its single request thread.
    pub fn new() -> io::Result<Self> {
        Self::with_launcher(Arc::new(process::production_command), false)
    }

    fn with_launcher(
        launcher: Arc<dyn Fn() -> io::Result<Command> + Send + Sync>,
        fixture_protocol_stderr: bool,
    ) -> io::Result<Self> {
        let state = Arc::new(State {
            queue: Mutex::new(QueueModel::default()),
            wake: Condvar::new(),
            closing: AtomicBool::new(false),
            finished: Mutex::new(false),
            finished_wake: Condvar::new(),
            launcher,
            fixture_protocol_stderr,
        });
        let worker_state = state.clone();
        let join = thread::Builder::new()
            .name("fyle-extension-supervisor".into())
            .spawn(move || {
                let _finish = FinishGuard(worker_state.clone());
                supervisor_loop(worker_state)
            })?;
        Ok(Self {
            controller: Arc::new(Controller {
                state,
                join: Mutex::new(Some(join)),
            }),
        })
    }

    /// Admits a request or returns a completed ticket for captured inapplicability.
    pub fn submit(&self, work: ColumnWork) -> io::Result<Ticket> {
        if self.controller.state.closing.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "extension supervisor is shutting down",
            ));
        }
        let prepared = prepare(work)?;
        let (sender, receiver) = mpsc::channel();
        if let Some((init, source, generation_cancelled)) = prepared {
            let pending = Pending {
                init,
                source,
                response: sender,
            };
            let mut queue = self
                .controller
                .state
                .queue
                .lock()
                .expect("extension queue mutex is not poisoned");
            if self.controller.state.closing.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "extension supervisor is shutting down",
                ));
            }
            let (_, ticket_cancelled) = queue.submit(pending, generation_cancelled)?;
            self.controller.state.wake.notify_one();
            Ok(Ticket {
                receiver: Some(receiver),
                cancelled: ticket_cancelled,
                completed: false,
            })
        } else {
            let cancelled = Arc::new(AtomicBool::new(false));
            let _ = sender.send(ResultState::Unavailable(
                "captured source facts do not match this contribution".into(),
            ));
            Ok(Ticket {
                receiver: Some(receiver),
                cancelled,
                completed: false,
            })
        }
    }

    /// Cancels queued and active work, then waits for helper cleanup and thread exit.
    pub fn shutdown(&self) -> io::Result<()> {
        request_shutdown(&self.controller.state);
        let mut finished = self
            .controller
            .state
            .finished
            .lock()
            .expect("extension completion mutex is not poisoned");
        while !*finished {
            finished = self
                .controller
                .state
                .finished_wake
                .wait(finished)
                .expect("extension completion mutex is not poisoned");
        }
        drop(finished);
        if let Some(join) = self
            .controller
            .join
            .lock()
            .expect("extension join mutex is not poisoned")
            .take()
        {
            join.join().map_err(|_| {
                io::Error::other("extension supervisor thread panicked during shutdown")
            })?;
        }
        Ok(())
    }
}

fn prepare(work: ColumnWork) -> io::Result<Option<(Init, Location, Arc<AtomicBool>)>> {
    if work.is_dir || work.source.as_local_path().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "extension columns require a local regular file",
        ));
    }
    let mut selected = None;
    for provider in &work.package.manifest.providers {
        if let Some(column) = provider
            .columns
            .iter()
            .find(|column| column.header.id == work.contribution)
        {
            selected = Some((provider, column));
            break;
        }
    }
    let Some((provider, column)) = selected else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "package does not declare this column contribution",
        ));
    };
    let field = provider
        .fields
        .iter()
        .find(|field| field.id == column.field)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "column field is undeclared"))?;
    let context = ApplicabilityContext {
        subject: Subject::Item(work.source.clone()),
        name: work.name.clone(),
        is_dir: work.is_dir,
        formats: Vec::new(),
        context_identity: None,
    };
    if !column.header.applicability.matches(&context) {
        return Ok(None);
    }
    let field_type = match field.kind {
        FieldKind::Text => FieldType::Text,
        FieldKind::Quantity(Unit::Seconds) => FieldType::Quantity(super::protocol::Unit::Seconds),
        FieldKind::Quantity(Unit::Metres) => FieldType::Quantity(super::protocol::Unit::Metres),
    };
    let init = Init {
        package: work.package.manifest.id.as_str().to_owned(),
        provider: provider.id.as_str().to_owned(),
        contribution: column.header.id.as_str().to_owned(),
        field: field.id.as_str().to_owned(),
        field_type,
        demand: Demand {
            max_bytes: column.header.demand.max_bytes,
            max_items: column.header.demand.max_items,
        },
        name: work.name,
        is_dir: work.is_dir,
        source_handle: 1,
        script: work.package.source.clone(),
    };
    Ok(Some((init, work.source, work.generation_cancelled)))
}

fn request_shutdown(state: &State) {
    state.closing.store(true, Ordering::Release);
    let mut queue = state
        .queue
        .lock()
        .expect("extension queue mutex is not poisoned");
    for job in queue.cancel_all() {
        let _ = job.work.response.send(ResultState::Failed(
            "extension supervisor shut down before the request started".into(),
        ));
    }
    state.wake.notify_all();
}

fn supervisor_loop(state: Arc<State>) {
    loop {
        let job = {
            let mut queue = state
                .queue
                .lock()
                .expect("extension queue mutex is not poisoned");
            loop {
                if state.closing.load(Ordering::Acquire) && !queue.has_active() {
                    break None;
                }
                if let Some(job) = queue.activate_next() {
                    break Some((
                        job.id,
                        job.work.clone(),
                        job.ticket_token(),
                        job.generation_token(),
                    ));
                }
                queue = state
                    .wake
                    .wait(queue)
                    .expect("extension queue mutex is not poisoned");
            }
        };
        let Some((id, pending, ticket_cancelled, generation_cancelled)) = job else {
            break;
        };
        let result = if ticket_cancelled.load(Ordering::Acquire)
            || generation_cancelled.load(Ordering::Acquire)
            || state.closing.load(Ordering::Acquire)
        {
            ResultState::Failed("extension request was cancelled".into())
        } else {
            process::execute(
                &pending,
                || {
                    ticket_cancelled.load(Ordering::Acquire)
                        || generation_cancelled.load(Ordering::Acquire)
                        || state.closing.load(Ordering::Acquire)
                },
                &state.launcher,
                state.fixture_protocol_stderr,
            )
        };
        let mut queue = state
            .queue
            .lock()
            .expect("extension queue mutex is not poisoned");
        queue
            .release(id)
            .expect("supervisor releases the request owning its active slot");
        state.wake.notify_one();
        drop(queue);
        let _ = pending.response.send(result);
    }
}

struct FinishGuard(Arc<State>);

impl Drop for FinishGuard {
    fn drop(&mut self) {
        *self
            .0
            .finished
            .lock()
            .expect("extension completion mutex is not poisoned") = true;
        self.0.finished_wake.notify_all();
    }
}
