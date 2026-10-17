//! Metadata request policy and bounded cache, independent of GUI and disk access.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::columns::{ColumnId, ColumnKind};
use crate::data_providers::{ResultState, Value};
use crate::links::LinkState;
use crate::location::Location;

const CACHE_CAPACITY: usize = 256;
const QUEUE_CAPACITY: usize = 512;
const JOB_CAPACITY: usize = 4;

/// One kind of optional metadata needed by a visible row.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    /// Duration of a candidate local media file.
    Duration,
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
    /// One typed provider-defined column value.
    Provider(ColumnId),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    generation: u64,
    path: PathBuf,
}

/// A local metadata read selected by the queue.
#[derive(Clone, Debug)]
pub struct Request {
    key: Key,
    /// Optional values requested for this row.
    pub fields: HashSet<Field>,
    /// Account identifier captured from the listing.
    pub uid: Option<u32>,
    /// Symlink state captured from the listing.
    pub link: LinkState,
    /// Cancellation shared by all reads from this listing generation.
    pub cancel: Arc<AtomicBool>,
    /// Name captured from the directory listing.
    /// Captured facts used to evaluate provider applicability.
    pub row: Option<CapturedRow>,
}

/// Stable row facts supplied by the directory listing, never inferred by a reader.
#[derive(Clone, Debug, Default)]
pub struct CapturedRow {
    /// Name captured from the directory listing.
    pub name: String,
    /// Whether the captured row is a directory.
    pub is_dir: bool,
}

impl Request {
    /// Native local path to read without resolving or changing its spelling.
    pub fn path(&self) -> &Path {
        &self.key.path
    }
}

/// Read results, including empty values for unavailable optional metadata.
#[derive(Clone, Debug, Default)]
pub struct Values {
    /// Formatted duration or an empty string when unavailable.
    pub duration: Option<String>,
    /// Formatted extended attribute names.
    pub xattrs: Option<String>,
    /// SELinux context or an empty string.
    pub selinux: Option<String>,
    /// EXIF values indexed by stable column keys.
    pub exif: Option<HashMap<&'static str, String>>,
    /// Resolved account name.
    pub owner: Option<String>,
    /// Completed symlink lookup, with no path when resolution failed.
    pub resolved_link: Option<Option<PathBuf>>,
    /// Typed provider-column values, including completed absence and failures.
    pub providers: HashMap<ColumnId, ResultState<Value>>,
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
            ColumnKind::Duration => self.values.duration.as_deref(),
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

    /// Returns a completed typed value for a provider column.
    pub fn provider_cell(&self, id: &ColumnId) -> Option<&ResultState<Value>> {
        self.values.providers.get(id)
    }
}

/// Bounded request queue and least-recently-requested cache, independent of GPUI and disk access.
#[derive(Default)]
pub struct MetadataQueue {
    generation: Option<u64>,
    generation_cancel: Option<Arc<AtomicBool>>,
    cached: HashMap<PathBuf, MetadataValues>,
    order: VecDeque<PathBuf>,
    pending: VecDeque<Request>,
    active: HashMap<Key, HashSet<Field>>,
}

impl MetadataQueue {
    /// Invalidates cached and queued values when a listing generation changes.
    pub fn sync_generation(&mut self, generation: u64) {
        if self.generation.is_some_and(|current| generation <= current) {
            return;
        }
        if let Some(cancel) = &self.generation_cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        self.generation = Some(generation);
        self.generation_cancel = Some(Arc::new(AtomicBool::new(false)));
        self.cached.clear();
        self.order.clear();
        self.pending.clear();
    }

    /// Requests fields for a local row. Duplicate work is merged or skipped.
    pub fn request(
        &mut self,
        location: &Location,
        generation: u64,
        fields: &[Field],
        uid: Option<u32>,
        link: LinkState,
    ) {
        self.request_row(location, generation, fields, uid, link, None);
    }

    /// Requests metadata with name and directory facts captured from the listing.
    pub fn request_row(
        &mut self,
        location: &Location,
        generation: u64,
        fields: &[Field],
        uid: Option<u32>,
        link: LinkState,
        row: Option<CapturedRow>,
    ) {
        self.sync_generation(generation);
        if self.generation != Some(generation) {
            return;
        }
        if fields.is_empty() {
            return;
        }
        let Some(path) = location.as_local_path() else {
            return;
        };
        let key = Key {
            generation,
            path: path.to_path_buf(),
        };
        let mut missing: HashSet<_> = fields.iter().cloned().collect();
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
            if row.is_some() {
                pending.row = row;
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
            cancel: self
                .generation_cancel
                .as_ref()
                .expect("generation token exists")
                .clone(),
            row,
        });
    }

    /// Starts the next request while below the fixed concurrency bound.
    pub fn take_next(&mut self) -> Option<Request> {
        if self.active.len() >= JOB_CAPACITY {
            return None;
        }
        let request = self.pending.pop_front()?;
        self.active
            .insert(request.key.clone(), request.fields.clone());
        Some(request)
    }

    /// Installs a completed request only if its listing generation is current.
    pub fn complete(&mut self, request: Request, values: Values) -> bool {
        self.active.remove(&request.key);
        if self.generation != Some(request.key.generation) {
            return false;
        }
        let cached = self.cached.entry(request.key.path.clone()).or_default();
        if request.fields.contains(&Field::Duration) {
            cached.values.duration = values.duration;
        }
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
        for field in &request.fields {
            if let Field::Provider(id) = field {
                cached.values.providers.insert(
                    id.clone(),
                    values
                        .providers
                        .get(id)
                        .cloned()
                        .unwrap_or(ResultState::Absent),
                );
            }
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
pub fn fields_for_row(
    columns: &[ColumnKind],
    name: &str,
    is_dir: bool,
    uid: Option<u32>,
) -> Vec<Field> {
    let mut fields = Vec::new();
    for column in columns {
        let field = match column {
            ColumnKind::Duration
                if !is_dir
                    && matches!(
                        crate::icons::icon_for(name, false, false, false),
                        crate::icons::FileIcon::Audio | crate::icons::FileIcon::Video
                    ) =>
            {
                Some(Field::Duration)
            }
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

#[cfg(test)]
mod tests {
    use super::{
        CACHE_CAPACITY, Field, JOB_CAPACITY, MetadataQueue, QUEUE_CAPACITY, Values, fields_for_row,
    };
    use crate::columns::{ColumnKind, ExifField};
    use crate::data_providers::ResultState;
    use crate::links::LinkState;
    use crate::location::Location;
    use std::path::PathBuf;

    fn test_path(name: impl AsRef<std::path::Path>) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    #[test]
    fn old_generation_workers_retain_slots_until_they_finish() {
        let mut queue = MetadataQueue::default();
        for index in 0..JOB_CAPACITY {
            queue.request(
                &Location::local(test_path(index.to_string())).unwrap(),
                1,
                &[Field::Xattrs],
                None,
                LinkState::NotLink,
            );
        }
        let old: Vec<_> = (0..JOB_CAPACITY)
            .map(|_| queue.take_next().unwrap())
            .collect();
        let path = Location::local(test_path("new")).unwrap();
        queue.request(&path, 2, &[Field::Exif], None, LinkState::NotLink);
        assert!(
            old.iter()
                .all(|request| request.cancel.load(std::sync::atomic::Ordering::Relaxed))
        );
        assert!(queue.take_next().is_none());
        let mut old = old.into_iter();
        assert!(!queue.complete(old.next().unwrap(), Values::default()));
        let current = queue.take_next().unwrap();
        assert_eq!(current.path(), path.as_local_path().unwrap());
        assert_eq!(queue.active.len(), JOB_CAPACITY);
        assert!(queue.complete(current, Values::default()));
        for request in old {
            assert!(!queue.complete(request, Values::default()));
        }
        assert!(queue.active.is_empty());
        assert_eq!(queue.cached.len(), 1);
    }

    #[test]
    fn stale_duration_completion_cannot_replace_new_listing_values() {
        let mut queue = MetadataQueue::default();
        let path = Location::local(test_path("song.mp3")).unwrap();
        queue.request(&path, 7, &[Field::Duration], None, LinkState::NotLink);
        let old = queue.take_next().expect("duration request");
        queue.sync_generation(8);
        assert!(old.cancel.load(std::sync::atomic::Ordering::Relaxed));
        queue.request(&path, 8, &[Field::Duration], None, LinkState::NotLink);
        queue.request(&path, 7, &[Field::Duration], None, LinkState::NotLink);
        assert_eq!(queue.pending.len(), 1);
        assert!(!queue.complete(
            old,
            Values {
                duration: Some("09:00".into()),
                ..Values::default()
            }
        ));
        assert!(
            queue
                .get(path.as_local_path().expect("local path"), 8)
                .is_none()
        );
        let current = queue.take_next().expect("new duration request");
        assert!(queue.complete(
            current,
            Values {
                duration: Some("01:05".into()),
                ..Values::default()
            }
        ));
        assert_eq!(
            queue
                .get(path.as_local_path().expect("local path"), 8)
                .and_then(|values| values.column(ColumnKind::Duration)),
            Some("01:05")
        );
    }

    #[test]
    fn stale_provider_completion_is_rejected_and_releases_its_active_slot() {
        use crate::columns::ColumnId;
        use crate::data_providers::{Completeness, Unit, Value};
        let mut queue = MetadataQueue::default();
        let path = test_path("provider-item");
        let location = Location::local(path.clone()).unwrap();
        let id = ColumnId::new("extension.sensor.reader.distance").unwrap();
        queue.request(
            &location,
            1,
            &[Field::Provider(id.clone())],
            None,
            LinkState::NotLink,
        );
        let old = queue.take_next().unwrap();
        queue.request(
            &location,
            2,
            &[Field::Provider(id.clone())],
            None,
            LinkState::NotLink,
        );
        let mut values = Values::default();
        values.providers.insert(
            id,
            ResultState::Ready {
                value: Value::quantity(2.0, Unit::Metres).unwrap(),
                completeness: Completeness::Complete,
            },
        );
        assert!(!queue.complete(old, values));
        assert!(queue.active.is_empty());
        assert!(queue.get(&path, 2).is_none());
        assert!(queue.take_next().is_some());
    }

    #[test]
    fn fields_requested_during_a_read_are_requeued_after_completion() {
        let mut queue = MetadataQueue::default();
        let path = Location::local(test_path("partial")).unwrap();
        queue.request(&path, 1, &[Field::Xattrs], None, LinkState::NotLink);
        let active = queue.take_next().unwrap();
        queue.request(
            &path,
            1,
            &[Field::Xattrs, Field::Exif],
            None,
            LinkState::NotLink,
        );
        assert!(queue.pending.is_empty());
        assert!(queue.complete(
            active,
            Values {
                xattrs: Some("user.label".into()),
                ..Values::default()
            }
        ));
        queue.request(
            &path,
            1,
            &[Field::Xattrs, Field::Exif],
            None,
            LinkState::NotLink,
        );
        let next = queue.take_next().unwrap();
        assert_eq!(next.fields, [Field::Exif].into_iter().collect());
        assert!(queue.complete(
            next,
            Values {
                exif: Some([("camera", "Camera".into())].into_iter().collect()),
                ..Values::default()
            }
        ));
        let cached = queue.get(path.as_local_path().unwrap(), 1).unwrap();
        assert_eq!(cached.column(ColumnKind::Xattrs), Some("user.label"));
        assert_eq!(
            cached.column(ColumnKind::Exif(ExifField::Camera)),
            Some("Camera")
        );
        assert_eq!(cached.column(ColumnKind::Exif(ExifField::Iso)), Some(""));
        queue.request(
            &path,
            1,
            &[Field::Xattrs, Field::Exif],
            None,
            LinkState::NotLink,
        );
        assert!(queue.take_next().is_none());
    }

    #[test]
    fn pending_requests_merge_account_and_link_inputs() {
        let mut queue = MetadataQueue::default();
        let path = Location::local(test_path("link")).unwrap();
        queue.request(&path, 1, &[Field::Xattrs], None, LinkState::NotLink);
        let link = LinkState::Link {
            target: "target".into(),
        };
        queue.request(
            &path,
            1,
            &[Field::Owner, Field::ResolvedLink],
            Some(1000),
            link.clone(),
        );
        let request = queue.take_next().unwrap();
        assert_eq!(request.uid, Some(1000));
        assert_eq!(request.link, link);
        assert_eq!(request.fields.len(), 3);
        assert!(queue.complete(
            request,
            Values {
                xattrs: Some(String::new()),
                owner: Some("user".into()),
                resolved_link: Some(None),
                ..Values::default()
            }
        ));
        let cached = queue.get(path.as_local_path().unwrap(), 1).unwrap();
        assert_eq!(cached.column(ColumnKind::Owner), Some("user"));
        assert_eq!(cached.resolved_link(), Some(None));
        queue.request(&path, 1, &[Field::ResolvedLink], None, LinkState::NotLink);
        assert!(queue.take_next().is_none());
    }

    #[test]
    fn empty_and_nonlocal_requests_advance_the_generation_without_queueing_work() {
        let mut queue = MetadataQueue::default();
        let path = Location::local(test_path("local")).unwrap();
        queue.request(&path, 1, &[Field::Xattrs], None, LinkState::NotLink);
        let request = queue.take_next().unwrap();
        assert!(queue.complete(request, Values::default()));
        queue.request(&path, 2, &[], None, LinkState::NotLink);
        let remote = Location::Remote(crate::location::RemoteLocation::root("drive").unwrap());
        queue.request(&remote, 2, &[Field::Xattrs], None, LinkState::NotLink);
        let archive = Location::archive(path.clone(), "member").unwrap();
        queue.request(&archive, 2, &[Field::Exif], None, LinkState::NotLink);
        assert!(queue.get(path.as_local_path().unwrap(), 1).is_none());
        assert!(queue.get(path.as_local_path().unwrap(), 2).is_none());
        assert!(queue.pending.is_empty());
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
            "image.jpg",
            false,
            Some(1000),
        );
        assert_eq!(
            fields,
            [Field::Owner, Field::Xattrs, Field::Selinux, Field::Exif]
        );
        assert!(
            fields_for_row(
                &[ColumnKind::Exif(ExifField::Camera)],
                "image.jpg",
                true,
                None
            )
            .is_empty()
        );
        assert!(fields_for_row(&[ColumnKind::Owner], "image.jpg", false, None).is_empty());
        assert_eq!(
            fields_for_row(&[ColumnKind::Duration], "song.mp3", false, None),
            [Field::Duration]
        );
        assert!(fields_for_row(&[ColumnKind::Duration], "text.txt", false, None).is_empty());
        assert!(fields_for_row(&[ColumnKind::Duration], "song.mp3", true, None).is_empty());
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
