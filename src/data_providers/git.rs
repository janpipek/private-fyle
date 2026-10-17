//! Shared, UI-independent contextual Git status requests and service.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread;
use std::time::Duration;

use super::{
    Applicability, ColumnContribution, ContributionHeader, ContributionId, DemandLimits,
    FieldDefinition, FieldId, FieldKind, PropertiesContribution, ProviderDefinition, ProviderId,
};
use crate::location::Location;

/// The bounded Git result for one listing scope.
pub type Snapshot = crate::git_status::Snapshot;

/// Declares the built-in Git column and contextual Properties fields.
pub fn definition() -> ProviderDefinition {
    let provider = ProviderId::new("builtin.git").expect("built-in Git provider ID is valid");
    let fields = ["root", "scope", "relative", "status"]
        .into_iter()
        .map(|name| FieldDefinition {
            id: FieldId::new(format!("builtin.git.{name}")).expect("Git field ID is valid"),
            kind: FieldKind::Text,
        })
        .collect::<Vec<_>>();
    let header = |id: &str, label: &str| ContributionHeader {
        id: ContributionId::new(format!("builtin.git.{id}")).expect("Git contribution ID is valid"),
        label: label.into(),
        applicability: Applicability::Context("git".into()),
        demand: DemandLimits {
            max_bytes: crate::git_status::STATUS_BYTES as u64,
            max_items: crate::git_status::STATUS_ITEMS as u64,
        },
    };
    ProviderDefinition {
        id: provider,
        fields: fields.clone(),
        properties: vec![PropertiesContribution {
            header: header("properties", "Git"),
            fields: fields.iter().map(|field| field.id.clone()).collect(),
        }],
        columns: vec![ColumnContribution {
            header: header("column", "Git status"),
            column_id: "git.status".into(),
            field: fields[3].id.clone(),
        }],
        viewers: Vec::new(),
    }
}

/// Contextual facts projected from one acquired repository snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    /// Repository root used for this status result.
    pub root: PathBuf,
    /// Directory queried by Git.
    pub scope: PathBuf,
    /// Selected path relative to the repository root.
    pub relative: PathBuf,
    /// Human readable status label, or a conservative no-record label.
    pub status: String,
}

/// Projects a snapshot to the selected path without further filesystem access.
pub fn project(snapshot: &Snapshot, selected: &Path, is_dir: bool) -> io::Result<Facts> {
    let relative = snapshot.relative_path(selected).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "selected path is outside Git scope",
        )
    })?;
    let status = snapshot.mark(selected, is_dir).map_or_else(
        || "No changes reported".to_owned(),
        |mark| match mark {
            crate::git_status::Mark::Ignored => "Ignored".to_owned(),
            crate::git_status::Mark::Untracked => "Untracked".to_owned(),
            crate::git_status::Mark::Renamed => "Renamed".to_owned(),
            crate::git_status::Mark::Added => "Added".to_owned(),
            crate::git_status::Mark::Deleted => "Deleted".to_owned(),
            crate::git_status::Mark::Modified => "Modified".to_owned(),
            crate::git_status::Mark::Conflicted => "Conflicted".to_owned(),
        },
    );
    Ok(Facts {
        root: snapshot.root.clone(),
        scope: snapshot.scope.clone(),
        relative,
        status,
    })
}

const CACHE_ENTRIES: usize = 16;
const CACHE_BYTES: usize = 16 * 1024 * 1024;
const PENDING_LIMIT: usize = 8;
const EPOCH_LIMIT: usize = 256;
const SUBSCRIBER_LIMIT: usize = 64;
const WAIT_POLL: Duration = Duration::from_millis(20);

/// Identifies a scope at one service invalidation epoch.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Key {
    /// Native path used as the listing scope.
    pub scope: PathBuf,
    /// Incremented when the scope is explicitly invalidated.
    pub epoch: u64,
}

/// A cloneable handle to shared pending work and a bounded snapshot cache.
#[derive(Clone)]
pub struct Service {
    inner: Arc<Inner>,
}

type Runner = dyn Fn(&Path, &AtomicBool) -> io::Result<Option<Snapshot>> + Send + Sync;
type SharedResult = Result<Option<Arc<Snapshot>>, SharedError>;

#[derive(Clone)]
struct SharedError {
    kind: io::ErrorKind,
    message: String,
}

impl From<io::Error> for SharedError {
    fn from(error: io::Error) -> Self {
        Self {
            kind: error.kind(),
            message: error.to_string(),
        }
    }
}

struct Inner {
    runner: Arc<Runner>,
    definition: ProviderDefinition,
    state: Mutex<State>,
    next_subscriber: AtomicU64,
    next_job: AtomicU64,
    idle: Condvar,
}

#[derive(Default)]
struct State {
    epochs: HashMap<PathBuf, u64>,
    global_epoch: u64,
    epoch_clock: u64,
    cache: VecDeque<Cached>,
    cache_bytes: usize,
    pending: HashMap<Key, Arc<Pending>>,
    active_jobs: HashMap<u64, Arc<Pending>>,
    shutting_down: bool,
}

struct Cached {
    key: Key,
    value: Arc<Snapshot>,
    bytes: usize,
}

struct Pending {
    job_id: u64,
    cancel: Arc<AtomicBool>,
    subscribers: Mutex<HashMap<u64, Sender<SharedResult>>>,
}

/// One independently cancellable attachment to shared work.
pub struct Lease {
    inner: Arc<Inner>,
    key: Key,
    pending: Weak<Pending>,
    subscriber: u64,
    receiver: Receiver<SharedResult>,
    attached: bool,
}

impl Service {
    /// Creates a service backed by the bounded native Git runner.
    pub fn new() -> Self {
        Self::with_runner(|scope, cancelled| {
            crate::git_status::discover(scope, || cancelled.load(Ordering::Acquire))
        })
    }

    /// Creates a service with an injected runner, primarily for deterministic model tests.
    pub fn with_runner(
        runner: impl Fn(&Path, &AtomicBool) -> io::Result<Option<Snapshot>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                runner: Arc::new(runner),
                definition: definition(),
                state: Mutex::new(State::default()),
                next_subscriber: AtomicU64::new(1),
                next_job: AtomicU64::new(1),
                idle: Condvar::new(),
            }),
        }
    }

    /// Returns the stable provider contributions used by column and Properties adapters.
    pub fn definition(&self) -> &ProviderDefinition {
        &self.inner.definition
    }

    /// Captures a key for the supplied scope without probing the filesystem.
    pub fn key_for_scope(&self, scope: &Path) -> Key {
        let state = self
            .inner
            .state
            .lock()
            .expect("Git service lock is available");
        Key {
            scope: scope.to_path_buf(),
            epoch: state
                .epochs
                .get(scope)
                .copied()
                .unwrap_or(state.global_epoch),
        }
    }

    /// Advances a scope epoch and cancels/removes its old work and cached snapshots.
    pub fn invalidate(&self, scope: &Path) {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("Git service lock is available");
        state.epoch_clock = state.epoch_clock.saturating_add(1);
        let epoch = state.epoch_clock;
        state.epochs.insert(scope.to_path_buf(), epoch);
        if state.epochs.len() > EPOCH_LIMIT {
            state.epochs.clear();
            state.global_epoch = epoch;
            state.cache.clear();
            state.cache_bytes = 0;
            state.pending.clear();
            for pending in state.active_jobs.values() {
                pending.cancel.store(true, Ordering::Release);
            }
        }
        let mut retained = VecDeque::new();
        while let Some(entry) = state.cache.pop_front() {
            if entry.key.scope == scope {
                state.cache_bytes = state.cache_bytes.saturating_sub(entry.bytes);
            } else {
                retained.push_back(entry);
            }
        }
        state.cache = retained;
        let old_keys: Vec<Key> = state
            .pending
            .keys()
            .filter(|key| key.scope == scope)
            .cloned()
            .collect();
        for key in old_keys {
            if let Some(pending) = state.pending.remove(&key) {
                pending.cancel.store(true, Ordering::Release);
            }
        }
    }

    /// Cancels every active request and waits until runner cleanup has completed.
    pub fn shutdown(&self) {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("Git service lock is available");
        state.shutting_down = true;
        for pending in state.active_jobs.values() {
            pending.cancel.store(true, Ordering::Release);
        }
        state.pending.clear();
        while !state.active_jobs.is_empty() {
            state = self
                .inner
                .idle
                .wait(state)
                .expect("Git service lock is available after shutdown wait");
        }
    }

    /// Attaches to the matching shared request or returns a cached result.
    pub fn subscribe(&self, key: Key) -> io::Result<Lease> {
        let (sender, receiver) = mpsc::channel();
        let subscriber = self.inner.next_subscriber.fetch_add(1, Ordering::Relaxed);
        let mut state = self
            .inner
            .state
            .lock()
            .expect("Git service lock is available");
        if state.shutting_down {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Git service is shutting down",
            ));
        }
        if state
            .epochs
            .get(&key.scope)
            .copied()
            .unwrap_or(state.global_epoch)
            != key.epoch
        {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "stale Git scope key",
            ));
        }
        if let Some(index) = state.cache.iter().position(|entry| entry.key == key) {
            let entry = state
                .cache
                .remove(index)
                .expect("located cache entry exists");
            let value = entry.value.clone();
            state.cache.push_front(entry);
            let _ = sender.send(Ok(Some(value)));
            return Ok(Lease {
                inner: self.inner.clone(),
                key,
                pending: Weak::new(),
                subscriber,
                receiver,
                attached: false,
            });
        }
        let (pending, start_worker) = if let Some(pending) = state.pending.get(&key) {
            (pending.clone(), false)
        } else {
            if state.active_jobs.len() >= PENDING_LIMIT {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Git request limit reached",
                ));
            }
            let pending = Arc::new(Pending {
                job_id: self.inner.next_job.fetch_add(1, Ordering::Relaxed),
                cancel: Arc::new(AtomicBool::new(false)),
                subscribers: Mutex::new(HashMap::new()),
            });
            state.pending.insert(key.clone(), pending.clone());
            state.active_jobs.insert(pending.job_id, pending.clone());
            (pending, true)
        };
        let mut subscribers = pending
            .subscribers
            .lock()
            .expect("Git subscriber lock is available");
        if subscribers.len() >= SUBSCRIBER_LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Git subscriber limit reached",
            ));
        }
        subscribers.insert(subscriber, sender);
        drop(subscribers);
        if start_worker {
            let worker_inner = Arc::downgrade(&self.inner);
            let worker_pending = pending.clone();
            let worker_key = key.clone();
            if let Err(error) = thread::Builder::new()
                .name("fyle-git-status".into())
                .spawn(move || run_request(worker_inner, worker_key, worker_pending))
            {
                state.pending.remove(&key);
                state.active_jobs.remove(&pending.job_id);
                return Err(error);
            }
        }
        Ok(Lease {
            inner: self.inner.clone(),
            key,
            pending: Arc::downgrade(&pending),
            subscriber,
            receiver,
            attached: true,
        })
    }

    #[cfg(test)]
    fn cache_len(&self) -> usize {
        self.inner
            .state
            .lock()
            .expect("Git service lock is available")
            .cache
            .len()
    }
}

impl Default for Service {
    fn default() -> Self {
        Self::new()
    }
}

impl Lease {
    /// Waits for this request, detaching and returning no result when cancelled.
    pub fn wait(mut self, cancel: &AtomicBool) -> io::Result<Option<Arc<Snapshot>>> {
        loop {
            if cancel.load(Ordering::Acquire) {
                self.detach();
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Git request wait was cancelled",
                ));
            }
            match self.receiver.recv_timeout(WAIT_POLL) {
                Ok(result) => {
                    self.attached = false;
                    return result.map_err(|error| io::Error::new(error.kind, error.message));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.attached = false;
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "Git request ended without a result",
                    ));
                }
            }
        }
    }

    fn detach(&mut self) {
        if !self.attached {
            return;
        }
        self.attached = false;
        if let Some(pending) = self.pending.upgrade() {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("Git service lock is available");
            let mut subscribers = pending
                .subscribers
                .lock()
                .expect("Git subscriber lock is available");
            subscribers.remove(&self.subscriber);
            if subscribers.is_empty()
                && state
                    .pending
                    .get(&self.key)
                    .is_some_and(|current| Arc::ptr_eq(current, &pending))
            {
                pending.cancel.store(true, Ordering::Release);
                state.pending.remove(&self.key);
            }
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.detach();
    }
}

fn run_request(inner: Weak<Inner>, key: Key, pending: Arc<Pending>) {
    let Some(service) = inner.upgrade() else {
        return;
    };
    let result = (service.runner)(&key.scope, &pending.cancel)
        .map(|snapshot| snapshot.map(Arc::new))
        .map_err(SharedError::from);
    let mut state = service.state.lock().expect("Git service lock is available");
    let current_epoch = state
        .epochs
        .get(&key.scope)
        .copied()
        .unwrap_or(state.global_epoch);
    let is_current = current_epoch == key.epoch
        && state
            .pending
            .get(&key)
            .is_some_and(|current| Arc::ptr_eq(current, &pending));
    if is_current {
        state.pending.remove(&key);
        if !pending.cancel.load(Ordering::Acquire)
            && let Ok(Some(value)) = &result
        {
            let bytes = value.status.estimated_bytes().saturating_add(
                value
                    .root
                    .as_os_str()
                    .len()
                    .saturating_add(value.scope.as_os_str().len()),
            );
            if bytes <= CACHE_BYTES {
                state.cache.push_front(Cached {
                    key,
                    value: value.clone(),
                    bytes,
                });
                state.cache_bytes = state.cache_bytes.saturating_add(bytes);
                while state.cache.len() > CACHE_ENTRIES || state.cache_bytes > CACHE_BYTES {
                    if let Some(old) = state.cache.pop_back() {
                        state.cache_bytes = state.cache_bytes.saturating_sub(old.bytes);
                    }
                }
            }
        }
        let response = if pending.cancel.load(Ordering::Acquire) {
            Err(SharedError {
                kind: io::ErrorKind::Interrupted,
                message: "Git request was cancelled".into(),
            })
        } else {
            result.clone()
        };
        for (_, sender) in pending
            .subscribers
            .lock()
            .expect("Git subscriber lock is available")
            .drain()
        {
            let _ = sender.send(response.clone());
        }
    } else {
        for (_, sender) in pending
            .subscribers
            .lock()
            .expect("Git subscriber lock is available")
            .drain()
        {
            let _ = sender.send(Err(SharedError {
                kind: io::ErrorKind::Interrupted,
                message: "Git request is stale".into(),
            }));
        }
    }
    state.active_jobs.remove(&pending.job_id);
    service.idle.notify_all();
}

/// A captured request owned by a panel-facing controller.
#[derive(Clone)]
pub struct Request {
    /// Scope key captured when the request was made.
    pub key: Key,
    /// UI-owned folder generation captured with the key.
    pub generation: u64,
    /// Original typed local folder location.
    pub location: Location,
    /// Cancellation shared with the worker wait and this session.
    pub cancel: Arc<AtomicBool>,
}

/// Tracks one panel's active Git request independently from GPUI.
#[derive(Default)]
pub struct ColumnSession {
    current: Option<Request>,
}

impl ColumnSession {
    /// Updates the captured target, cancelling the prior request on supersession.
    pub fn sync(&mut self, target: Option<(Location, u64, Key)>) -> Option<Request> {
        if let Some((location, generation, key)) = target {
            if location.as_local_path().is_none() {
                self.cancel();
                return None;
            }
            if let Some(current) = self.current.as_ref()
                && current.location == location
                && current.generation == generation
                && current.key == key
            {
                return None;
            }
            self.cancel();
            let request = Request {
                key,
                generation,
                location,
                cancel: Arc::new(AtomicBool::new(false)),
            };
            self.current = Some(request.clone());
            Some(request)
        } else {
            self.cancel();
            None
        }
    }

    /// Reports whether the completion still belongs to the active uncancelled request.
    pub fn accepts(&self, request: &Request) -> bool {
        !request.cancel.load(Ordering::Acquire)
            && self.current.as_ref().is_some_and(|current| {
                current.location == request.location
                    && current.generation == request.generation
                    && current.key == request.key
            })
    }

    /// Cancels and forgets the active request.
    pub fn cancel(&mut self) {
        if let Some(current) = self.current.take() {
            current.cancel.store(true, Ordering::Release);
        }
    }
}

impl Drop for ColumnSession {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::{ColumnSession, Key, Service};
    use crate::git_status::{Snapshot, Status};
    use crate::location::Location;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    fn snapshot(scope: &std::path::Path) -> Snapshot {
        Snapshot {
            root: scope.to_path_buf(),
            scope: scope.to_path_buf(),
            canonical_scope: scope.to_path_buf(),
            status: Status::default(),
        }
    }

    fn wait_count(count: &AtomicUsize, target: usize) {
        for _ in 0..100 {
            if count.load(Ordering::SeqCst) == target {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(count.load(Ordering::SeqCst), target);
    }

    #[test]
    fn identical_keys_share_work_and_one_cancel_does_not_cancel_the_other() {
        let calls = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Barrier::new(2));
        let cancellation_seen = Arc::new(AtomicBool::new(true));
        let runner_calls = calls.clone();
        let runner_gate = gate.clone();
        let runner_cancellation_seen = cancellation_seen.clone();
        let service = Service::with_runner(move |scope, cancelled| {
            runner_calls.fetch_add(1, Ordering::SeqCst);
            runner_gate.wait();
            runner_cancellation_seen.store(cancelled.load(Ordering::Acquire), Ordering::SeqCst);
            Ok(Some(snapshot(scope)))
        });
        let key = service.key_for_scope(std::path::Path::new("/repo"));
        let first = service.subscribe(key.clone()).expect("first lease");
        let second = service.subscribe(key).expect("second lease");
        wait_count(&calls, 1);
        drop(first);
        gate.wait();
        let result = second
            .wait(&AtomicBool::new(false))
            .expect("runner succeeds");
        assert!(result.is_some());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!cancellation_seen.load(Ordering::SeqCst));
    }

    #[test]
    fn last_detach_cancels_and_stale_completion_is_not_cached() {
        let calls = Arc::new(AtomicUsize::new(0));
        let exited = Arc::new(AtomicBool::new(false));
        let runner_calls = calls.clone();
        let runner_exited = exited.clone();
        let service = Service::with_runner(move |scope, cancelled| {
            runner_calls.fetch_add(1, Ordering::SeqCst);
            while !cancelled.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(2));
            }
            runner_exited.store(true, Ordering::SeqCst);
            Ok(Some(snapshot(scope)))
        });
        let scope = PathBuf::from("/repo");
        let key = service.key_for_scope(&scope);
        let lease = service.subscribe(key.clone()).expect("lease");
        wait_count(&calls, 1);
        drop(lease);
        for _ in 0..100 {
            if exited.load(Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert!(exited.load(Ordering::SeqCst));
        service.invalidate(&scope);
        assert!(service.subscribe(key).is_err());
        assert_eq!(service.cache_len(), 0);
    }

    #[test]
    fn epochs_and_scopes_are_independent_and_cache_is_bounded() {
        let service = Service::with_runner(|scope, _| Ok(Some(snapshot(scope))));
        let first = PathBuf::from("/repo");
        let other = PathBuf::from("/repo/sub");
        let old = service.key_for_scope(&first);
        let other_key = service.key_for_scope(&other);
        assert_ne!(old, other_key);
        let result = service
            .subscribe(old.clone())
            .expect("lease")
            .wait(&AtomicBool::new(false));
        assert!(result.expect("runner succeeds").is_some());
        service.invalidate(&first);
        assert_ne!(service.key_for_scope(&first), old);
        assert!(service.subscribe(old).is_err());
        assert!(
            service
                .subscribe(other_key)
                .expect("scope key")
                .wait(&AtomicBool::new(false))
                .expect("runner")
                .is_some()
        );

        for index in 0..20 {
            let scope = PathBuf::from(format!("/cache/{index}"));
            let key = service.key_for_scope(&scope);
            let _ = service
                .subscribe(key)
                .expect("lease")
                .wait(&AtomicBool::new(false));
        }
        assert!(service.cache_len() <= 16);
    }

    #[test]
    fn session_cancels_superseded_targets_and_accepts_only_current_request() {
        let mut session = ColumnSession::default();
        let location = Location::local(PathBuf::from("/repo")).expect("location");
        let first = session
            .sync(Some((
                location.clone(),
                1,
                Key {
                    scope: PathBuf::from("/repo"),
                    epoch: 0,
                },
            )))
            .expect("request");
        assert!(session.accepts(&first));
        assert!(
            session
                .sync(Some((location.clone(), 1, first.key.clone())))
                .is_none()
        );
        let second = session
            .sync(Some((location, 2, first.key.clone())))
            .expect("next request");
        assert!(first.cancel.load(Ordering::Acquire));
        assert!(!session.accepts(&first));
        assert!(session.accepts(&second));
        session.sync(None);
        assert!(second.cancel.load(Ordering::Acquire));
    }

    #[test]
    fn definition_owns_the_legacy_git_column_and_properties_fields() {
        let definition = super::definition();
        crate::data_providers::validate_definitions(&[(
            crate::data_providers::RegistrationOrigin::Builtin,
            definition.clone(),
        )])
        .expect("Git provider definition is valid");
        assert_eq!(definition.columns[0].column_id, "git.status");
        assert_eq!(definition.properties[0].fields.len(), 4);
    }

    #[test]
    fn project_uses_original_scope_and_repository_relative_path() {
        let snapshot = Snapshot {
            root: PathBuf::from("/real/repo"),
            scope: PathBuf::from("/repo/sub"),
            canonical_scope: PathBuf::from("/real/repo/sub"),
            status: Status::parse(PathBuf::from("/real/repo").as_path(), b"?? sub/file\0"),
        };
        let facts = super::project(&snapshot, std::path::Path::new("/repo/sub/file"), false)
            .expect("selected path is in scope");
        assert_eq!(facts.relative, PathBuf::from("sub/file"));
        assert_eq!(facts.status, "Untracked");
        assert!(super::project(&snapshot, std::path::Path::new("/repo/file"), false).is_err());

        let grouped = Snapshot {
            root: PathBuf::from("/repo"),
            scope: PathBuf::from("/repo"),
            canonical_scope: PathBuf::from("/repo"),
            status: Status::parse(
                PathBuf::from("/repo").as_path(),
                b"!! target/\0?? newdir/\0",
            ),
        };
        for selected in ["/repo/target/child.txt", "/repo/newdir/child.txt"] {
            assert_eq!(
                super::project(&grouped, std::path::Path::new(selected), false)
                    .expect("selected child is in scope")
                    .status,
                "No changes reported"
            );
        }
    }

    #[test]
    fn shutdown_cancels_and_waits_for_runner_cleanup() {
        let exited = Arc::new(AtomicBool::new(false));
        let runner_exited = exited.clone();
        let service = Service::with_runner(move |_, cancelled| {
            while !cancelled.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(2));
            }
            runner_exited.store(true, Ordering::SeqCst);
            Ok(None)
        });
        let lease = service
            .subscribe(service.key_for_scope(std::path::Path::new("/repo")))
            .expect("lease");
        service.shutdown();
        assert!(exited.load(Ordering::SeqCst));
        assert_eq!(
            lease
                .wait(&AtomicBool::new(false))
                .expect_err("shutdown is reported")
                .kind(),
            std::io::ErrorKind::Interrupted
        );
        assert!(
            service
                .subscribe(service.key_for_scope(std::path::Path::new("/later")))
                .is_err()
        );
    }

    #[test]
    fn active_runner_limit_includes_detached_work_until_it_exits() {
        let started = Arc::new(AtomicUsize::new(0));
        let ended = Arc::new(AtomicUsize::new(0));
        let runner_started = started.clone();
        let runner_ended = ended.clone();
        let service = Service::with_runner(move |scope, cancelled| {
            runner_started.fetch_add(1, Ordering::SeqCst);
            while !cancelled.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(2));
            }
            runner_ended.fetch_add(1, Ordering::SeqCst);
            Ok(Some(snapshot(scope)))
        });
        let mut leases = Vec::new();
        for index in 0..8 {
            let scope = PathBuf::from(format!("/pending/{index}"));
            leases.push(
                service
                    .subscribe(service.key_for_scope(&scope))
                    .expect("request under active limit"),
            );
        }
        wait_count(&started, 8);
        assert!(
            service
                .subscribe(service.key_for_scope(std::path::Path::new("/overflow")))
                .is_err()
        );
        drop(leases.remove(0));
        wait_count(&ended, 1);
        let replacement_scope = PathBuf::from("/replacement");
        leases.push(
            service
                .subscribe(service.key_for_scope(&replacement_scope))
                .expect("slot is released after runner cleanup"),
        );
        service.shutdown();
        drop(leases);
        assert_eq!(ended.load(Ordering::SeqCst), 9);
    }

    #[test]
    fn bounded_epoch_fallback_rejects_old_keys_and_accepts_new_keys() {
        let service = Service::with_runner(|scope, _| Ok(Some(snapshot(scope))));
        let retained_scope = PathBuf::from("/retained");
        let old_key = service.key_for_scope(&retained_scope);
        for index in 0..257 {
            service.invalidate(&PathBuf::from(format!("/invalidated/{index}")));
        }
        assert!(service.subscribe(old_key).is_err());
        let fresh = service.key_for_scope(&retained_scope);
        assert!(
            service
                .subscribe(fresh)
                .expect("new fallback key")
                .wait(&AtomicBool::new(false))
                .expect("new key is accepted")
                .is_some()
        );
    }
}
