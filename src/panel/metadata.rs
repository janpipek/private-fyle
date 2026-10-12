//! Bounded metadata cache and background reader for visible local panel rows.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use gpui_kit::*;

use crate::columns::ColumnKind;
use crate::fs_ops;
use crate::links::LinkState;
use crate::location::Location;

const CACHE_CAPACITY: usize = 256;
const QUEUE_CAPACITY: usize = 512;
const JOB_CAPACITY: usize = 4;

/// One kind of optional metadata needed by a visible row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    /// Extended attribute names.
    Xattrs,
    /// SELinux security context.
    Selinux,
    /// The EXIF fields shown in photo columns.
    Exif,
    /// The owner's user name resolved from the system account database.
    Owner,
    /// Canonical target path for a working symlink tooltip.
    ResolvedLink,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    generation: u64,
    path: PathBuf,
}

#[derive(Clone, Debug)]
struct Request {
    key: Key,
    fields: HashSet<Field>,
    uid: Option<u32>,
    link: LinkState,
}

#[derive(Clone, Debug, Default)]
struct Values {
    xattrs: Option<String>,
    selinux: Option<String>,
    exif: Option<HashMap<&'static str, String>>,
    owner: Option<String>,
    resolved_link: Option<Option<PathBuf>>,
}

/// Loaded display values for one local path.
#[derive(Clone, Debug, Default)]
pub struct MetadataValues {
    loaded: HashSet<Field>,
    values: Values,
}

impl MetadataValues {
    /// Returns the formatted value for an optional metadata column when loaded.
    pub fn column(&self, kind: ColumnKind) -> Option<&str> {
        match kind {
            ColumnKind::Owner => self.values.owner.as_deref(),
            ColumnKind::Xattrs => self.values.xattrs.as_deref(),
            ColumnKind::Selinux => self.values.selinux.as_deref(),
            ColumnKind::Exif(field) => Some(
                self.values
                    .exif
                    .as_ref()?
                    .get(field.key())
                    .map(String::as_str)
                    .unwrap_or(""),
            ),
            _ => None,
        }
    }

    /// Returns the canonical target of a completed symlink lookup.
    pub fn resolved_link(&self) -> Option<Option<&Path>> {
        self.values
            .resolved_link
            .as_ref()
            .map(|path| path.as_deref())
    }
}

/// Bounded request queue and least-recently-requested cache, independent of GPUI and disk access.
#[derive(Default)]
struct MetadataQueue {
    generation: Option<u64>,
    cached: HashMap<PathBuf, MetadataValues>,
    order: VecDeque<PathBuf>,
    pending: VecDeque<Request>,
    active: HashMap<Key, HashSet<Field>>,
}

impl MetadataQueue {
    /// Requests fields for a local row. Duplicate work is merged or skipped.
    fn request(
        &mut self,
        location: &Location,
        generation: u64,
        fields: &[Field],
        uid: Option<u32>,
        link: LinkState,
    ) {
        if fields.is_empty() {
            return;
        }
        let Some(path) = location.as_local_path() else {
            return;
        };
        if self.generation != Some(generation) {
            self.generation = Some(generation);
            self.cached.clear();
            self.order.clear();
            self.pending.clear();
        }
        let key = Key {
            generation,
            path: path.to_path_buf(),
        };
        let mut missing: HashSet<_> = fields.iter().copied().collect();
        if let Some(cached) = self.cached.get(&key.path) {
            missing.retain(|field| !cached.loaded.contains(field));
        }
        if missing.is_empty() {
            self.touch(&key.path);
            return;
        }
        if let Some(pending) = self.pending.iter_mut().find(|pending| pending.key == key) {
            pending.fields.extend(missing);
            if uid.is_some() {
                pending.uid = uid;
            }
            if !matches!(link, LinkState::NotLink) {
                pending.link = link;
            }
            return;
        }
        if let Some(active) = self.active.get(&key) {
            missing.retain(|field| !active.contains(field));
            if missing.is_empty() {
                return;
            }
            // Completion triggers a redraw, which will enqueue fields not
            // covered by this in-flight request.
            return;
        }
        if self.pending.len() >= QUEUE_CAPACITY {
            return;
        }
        self.pending.push_back(Request {
            key,
            fields: missing,
            uid,
            link,
        });
    }

    /// Starts the next request while below the fixed concurrency bound.
    fn take_next(&mut self) -> Option<Request> {
        if self.active.len() >= JOB_CAPACITY {
            return None;
        }
        let request = self.pending.pop_front()?;
        self.active
            .insert(request.key.clone(), request.fields.clone());
        Some(request)
    }

    /// Installs a completed request only if its listing generation is current.
    fn complete(&mut self, request: Request, values: Values) -> bool {
        self.active.remove(&request.key);
        if self.generation != Some(request.key.generation) {
            return false;
        }
        let cached = self.cached.entry(request.key.path.clone()).or_default();
        if request.fields.contains(&Field::Xattrs) {
            cached.values.xattrs = values.xattrs;
        }
        if request.fields.contains(&Field::Selinux) {
            cached.values.selinux = values.selinux;
        }
        if request.fields.contains(&Field::Exif) {
            cached.values.exif = values.exif;
        }
        if request.fields.contains(&Field::Owner) {
            cached.values.owner = values.owner;
        }
        if request.fields.contains(&Field::ResolvedLink) {
            cached.values.resolved_link = values.resolved_link;
        }
        cached.loaded.extend(request.fields);
        self.touch(&request.key.path);
        while self.cached.len() > CACHE_CAPACITY {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.cached.remove(&oldest);
        }
        true
    }

    /// Returns loaded fields for `path` only when they belong to `generation`.
    pub fn get(&self, path: &Path, generation: u64) -> Option<&MetadataValues> {
        if self.generation != Some(generation) {
            return None;
        }
        self.cached.get(path)
    }

    fn touch(&mut self, path: &Path) {
        self.order.retain(|old| old != path);
        if self.cached.contains_key(path) {
            self.order.push_back(path.to_path_buf());
        }
    }
}

/// Requests only the optional fields enabled for this row and panel.
pub fn fields_for_row(columns: &[ColumnKind], is_dir: bool, uid: Option<u32>) -> Vec<Field> {
    let mut fields = Vec::new();
    for column in columns {
        let field = match column {
            ColumnKind::Owner if uid.is_some() => Some(Field::Owner),
            ColumnKind::Xattrs => Some(Field::Xattrs),
            ColumnKind::Selinux => Some(Field::Selinux),
            ColumnKind::Exif(_) if !is_dir => Some(Field::Exif),
            _ => None,
        };
        if let Some(field) = field.filter(|field| !fields.contains(field)) {
            fields.push(field);
        }
    }
    fields
}

/// Cache and background workers shared by the list and icon views of a panel.
pub struct MetadataState {
    queue: MetadataQueue,
}

impl MetadataState {
    /// Creates an empty metadata cache for one panel.
    pub fn new() -> Self {
        Self {
            queue: MetadataQueue::default(),
        }
    }

    /// Returns completed values for a path in the current listing generation.
    pub fn get(&self, path: &Path, generation: u64) -> Option<&MetadataValues> {
        self.queue.get(path, generation)
    }

    /// Queues missing local metadata fields and starts bounded background work.
    pub fn request(
        &mut self,
        location: Location,
        generation: u64,
        fields: &[Field],
        uid: Option<u32>,
        link: LinkState,
        cx: &mut Context<Self>,
    ) {
        self.queue.request(&location, generation, fields, uid, link);
        self.start(cx);
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        while let Some(request) = self.queue.take_next() {
            let worker_request = request.clone();
            let state = cx.weak_entity();
            cx.spawn(async move |_, cx| {
                let values = cx
                    .background_spawn(async move { read_metadata(&worker_request) })
                    .await;
                let _ = state.update(cx, |state, cx| {
                    if state.queue.complete(request, values) {
                        cx.notify();
                    }
                    state.start(cx);
                });
            })
            .detach();
        }
    }
}

fn read_metadata(request: &Request) -> Values {
    let path = &request.key.path;
    let mut values = Values::default();
    if request.fields.contains(&Field::Xattrs) {
        values.xattrs = Some(crate::xattr::names(path).join(", "));
    }
    if request.fields.contains(&Field::Selinux) {
        values.selinux = Some(crate::xattr::selinux_context(path).unwrap_or_default());
    }
    if request.fields.contains(&Field::Exif) {
        values.exif = Some(crate::preview::exif_fields(path));
    }
    if request.fields.contains(&Field::Owner) {
        values.owner = Some(fs_ops::owner_name(request.uid));
    }
    if request.fields.contains(&Field::ResolvedLink) {
        values.resolved_link = Some(request.link.resolved(path));
    }
    values
}

#[cfg(test)]
mod tests {
    use super::{
        CACHE_CAPACITY, Field, JOB_CAPACITY, MetadataQueue, QUEUE_CAPACITY, Values, fields_for_row,
    };
    use crate::columns::{ColumnKind, ExifField};
    use crate::links::LinkState;
    use crate::location::Location;
    use std::path::PathBuf;

    fn test_path(name: impl AsRef<std::path::Path>) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    #[test]
    fn fields_follow_visible_columns_and_file_kind() {
        let fields = fields_for_row(
            &[
                ColumnKind::Name,
                ColumnKind::Owner,
                ColumnKind::Xattrs,
                ColumnKind::Selinux,
                ColumnKind::Exif(ExifField::Camera),
                ColumnKind::Exif(ExifField::Iso),
            ],
            false,
            Some(1000),
        );
        assert_eq!(
            fields,
            [Field::Owner, Field::Xattrs, Field::Selinux, Field::Exif]
        );
        assert!(fields_for_row(&[ColumnKind::Exif(ExifField::Camera)], true, None).is_empty());
        assert!(fields_for_row(&[ColumnKind::Owner], false, None).is_empty());
    }

    #[test]
    fn queue_coalesces_duplicate_requests_and_limits_active_jobs() {
        let mut queue = MetadataQueue::default();
        let path = Location::local(test_path("a.jpg")).unwrap();
        queue.request(&path, 7, &[Field::Xattrs], None, LinkState::NotLink);
        queue.request(
            &path,
            7,
            &[Field::Exif, Field::Xattrs],
            None,
            LinkState::NotLink,
        );
        assert_eq!(queue.pending.len(), 1);
        assert_eq!(queue.pending[0].fields.len(), 2);

        for index in 0..QUEUE_CAPACITY + 3 {
            queue.request(
                &Location::local(test_path(index.to_string())).unwrap(),
                7,
                &[Field::Xattrs],
                None,
                LinkState::NotLink,
            );
        }
        assert_eq!(queue.pending.len(), QUEUE_CAPACITY);
        while queue.take_next().is_some() {}
        assert_eq!(queue.active.len(), JOB_CAPACITY);
        assert_eq!(queue.pending.len(), QUEUE_CAPACITY - JOB_CAPACITY);
    }

    #[test]
    fn listing_generation_rejects_stale_completions_and_clears_old_cache() {
        let mut queue = MetadataQueue::default();
        queue.request(
            &Location::local(test_path("a")).unwrap(),
            1,
            &[Field::Xattrs],
            None,
            LinkState::NotLink,
        );
        let old = queue.take_next().unwrap();
        queue.request(
            &Location::local(test_path("b")).unwrap(),
            2,
            &[Field::Selinux],
            None,
            LinkState::NotLink,
        );
        assert!(!queue.complete(old, Values::default()));
        assert!(queue.get(&test_path("a"), 2).is_none());
        let current = queue.take_next().unwrap();
        assert!(queue.complete(current, Values::default()));
        assert!(queue.get(&test_path("b"), 2).is_some());
    }

    #[test]
    fn completed_cache_is_bounded_and_lru_evicted() {
        let mut queue = MetadataQueue::default();
        for index in 0..CACHE_CAPACITY {
            queue.request(
                &Location::local(test_path(index.to_string())).unwrap(),
                1,
                &[Field::Xattrs],
                None,
                LinkState::NotLink,
            );
            let request = queue.take_next().unwrap();
            assert!(queue.complete(request, Values::default()));
        }
        queue.request(
            &Location::local(test_path("0")).unwrap(),
            1,
            &[Field::Xattrs],
            None,
            LinkState::NotLink,
        );
        queue.request(
            &Location::local(test_path(CACHE_CAPACITY.to_string())).unwrap(),
            1,
            &[Field::Xattrs],
            None,
            LinkState::NotLink,
        );
        let request = queue.take_next().unwrap();
        assert!(queue.complete(request, Values::default()));
        assert_eq!(queue.cached.len(), CACHE_CAPACITY);
        assert!(queue.get(&test_path("0"), 1).is_some());
        assert!(queue.get(&test_path("1"), 1).is_none());
        assert!(
            queue
                .get(&test_path(CACHE_CAPACITY.to_string()), 1)
                .is_some()
        );
    }
}
