//! Shared Properties contracts, reader orchestration, and selection lifecycle.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use crate::entry::{Attrs, Entry};
use crate::location::Location;

mod exif_reader;
mod filesystem;
mod geo;
mod media;
mod parquet;
mod sqlite;

pub use exif_reader::{ExifField, ExifFieldId};

/// A selected listing row paired with its source address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Original typed address, retained across materialization boundaries.
    pub location: Location,
    /// Facts provided by the source listing.
    pub entry: Entry,
}

/// Listing facts captured without touching the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListingFacts {
    /// Display name from the source listing.
    pub name: String,
    /// Whether the source reports a directory.
    pub is_dir: bool,
    /// Source-provided byte size, if known.
    pub size: Option<u64>,
    /// Source-provided modification time, if known.
    pub modified: Option<SystemTime>,
    /// Source-provided platform attributes.
    pub attrs: Option<Attrs>,
    /// Link state reported by the source listing.
    pub link: crate::links::LinkState,
}

pub use filesystem::{FilesystemFacts, StatFacts};

/// Status for one independently loaded properties section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Section<T> {
    /// The reader returned this section's data.
    Available(T),
    /// This source or format does not offer this section.
    Unavailable(String),
    /// The section reader failed while other sections may remain usable.
    Failed(String),
    /// Data was read up to a documented resource bound.
    Truncated { value: T, limit: u64 },
}

impl<T> Section<Vec<T>> {
    /// Whether this collection has useful values or an informative read status.
    pub fn is_visible(&self) -> bool {
        match self {
            Self::Available(values) => !values.is_empty(),
            Self::Unavailable(_) => false,
            Self::Failed(_) | Self::Truncated { .. } => true,
        }
    }
}

/// Results of inspecting a selected source item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inspection {
    /// Original source identity.
    pub location: Location,
    /// Facts available from the listing, including for remote and archive rows.
    pub listing: ListingFacts,
    /// Local `lstat` and symlink target details, when applicable.
    pub filesystem: Section<FilesystemFacts>,
    /// EXIF details for supported local image and RAW files.
    pub exif: Section<Vec<ExifField>>,
    /// Additional format groups, such as future database or columnar metadata.
    pub additional_groups: Vec<PropertyGroup>,
}

/// Stable identity for a Properties group.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GroupId(pub String);

/// A semantic group of fields, with room for nested groups and tables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyGroup {
    /// Stable identity independent of presentation labels.
    pub id: GroupId,
    /// Result for this group; other groups may remain usable after failure.
    pub status: GroupStatus,
    /// Semantic fields in this group.
    pub fields: Vec<PropertyField>,
    /// Nested subgroups, such as a database table's columns.
    pub groups: Vec<PropertyGroup>,
    /// Structured tables, such as row-group summaries.
    pub tables: Vec<PropertyTable>,
}

impl PropertyGroup {
    /// Whether this group has data, a visible child, or an informative status.
    pub fn is_visible(&self) -> bool {
        match &self.status {
            GroupStatus::Unavailable(_) => false,
            GroupStatus::Failed(_) | GroupStatus::Truncated { .. } => true,
            GroupStatus::Available => {
                !self.fields.is_empty()
                    || self.groups.iter().any(Self::is_visible)
                    || self
                        .tables
                        .iter()
                        .any(|table| !table.columns.is_empty() || !table.rows.is_empty())
            }
        }
    }
}

/// Load result for one extensible semantic group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupStatus {
    /// The group contains complete values.
    Available,
    /// The source or format has no values for this group.
    Unavailable(String),
    /// Loading this group failed independently.
    Failed(String),
    /// Values were cut off by a resource limit.
    Truncated { reason: String },
}

/// One named semantic field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyField {
    /// Stable field identity independent of its label.
    pub id: String,
    /// Typed value for consumer-specific formatting.
    pub value: PropertyValue,
}

/// Typed property values shared by filesystem and future format readers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropertyValue {
    /// Textual value.
    Text(String),
    /// Unsigned count or size.
    Unsigned(u64),
    /// Signed numeric value.
    Signed(i64),
    /// Boolean value.
    Boolean(bool),
    /// Timestamp.
    Time(SystemTime),
    /// Native path value.
    Path(PathBuf),
}

/// A structured table with stable column identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyTable {
    /// Stable identity independent of its label.
    pub id: String,
    /// Stable column identities.
    pub columns: Vec<String>,
    /// Rows aligned with `columns`.
    pub rows: Vec<Vec<PropertyValue>>,
}

/// Cheap local-file facts supplied to property providers during selection.
#[derive(Debug)]
pub struct Probe {
    /// Local source path inspected by the provider.
    pub path: PathBuf,
    /// File name, as shown in the listing.
    pub name: String,
    /// Lowercase extension without the dot.
    pub ext: String,
    /// Whether the local path is a directory.
    pub is_dir: bool,
    /// Whether the local path is a regular file.
    pub is_file: bool,
    /// Current local file size.
    pub size: u64,
}

impl Probe {
    fn read(path: &std::path::Path) -> io::Result<Self> {
        let metadata = fs::metadata(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            ext: path
                .extension()
                .map(|extension| extension.to_string_lossy().to_lowercase())
                .unwrap_or_default(),
            is_dir: metadata.is_dir(),
            is_file: metadata.is_file(),
            size: metadata.len(),
        })
    }
}

/// A provider that contributes Properties groups when its probe matches.
pub trait PropertyProvider: Send + Sync {
    /// Stable group identity used when a provider fails.
    fn id(&self) -> &'static str;
    /// Whether this provider can contribute properties for the local source.
    fn matches(&self, probe: &Probe) -> bool;
    /// Reads zero or more semantic groups; call from a background worker.
    fn read(&self, probe: &Probe, cancel: &Arc<AtomicBool>) -> io::Result<Vec<PropertyGroup>>;
    /// Converts a reader failure into a visible independent group.
    fn failure(&self, error: &io::Error) -> PropertyGroup {
        PropertyGroup {
            id: GroupId(self.id().into()),
            status: GroupStatus::Failed(error.to_string()),
            fields: Vec::new(),
            groups: Vec::new(),
            tables: Vec::new(),
        }
    }
}

/// Ordered providers; all providers matching a file contribute their groups.
#[derive(Default)]
pub struct Registry {
    providers: Vec<Box<dyn PropertyProvider>>,
}

impl Registry {
    /// Built-in format property providers.
    pub fn with_defaults() -> Self {
        let mut registry = Self::default();
        registry.register(Box::new(parquet::Provider));
        registry.register(Box::new(sqlite::Provider));
        registry.register(Box::new(media::Provider));
        registry.register(Box::new(geo::Provider));
        registry
    }

    /// Adds a provider after existing providers.
    pub fn register(&mut self, provider: Box<dyn PropertyProvider>) {
        self.providers.push(provider);
    }

    /// Reads all matching provider groups, retaining independent failures.
    pub fn read(
        &self,
        path: &std::path::Path,
        cancel: &Arc<AtomicBool>,
    ) -> io::Result<Vec<PropertyGroup>> {
        let probe = match Probe::read(path) {
            Ok(probe) => probe,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => return Err(error),
            // A vanished or unreadable path must not discard independent
            // listing, filesystem, or EXIF results from the inspection.
            Err(_) => return Ok(Vec::new()),
        };
        let mut groups = Vec::new();
        for provider in self
            .providers
            .iter()
            .filter(|provider| provider.matches(&probe))
        {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            match provider.read(&probe, cancel) {
                Ok(mut provided) => groups.append(&mut provided),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => return Err(error),
                Err(error) => groups.push(provider.failure(&error)),
            }
        }
        Ok(groups)
    }
}

/// A cancellable inspection request for the background worker.
#[derive(Clone, Debug)]
pub struct Request {
    /// Monotonically increasing selection generation.
    pub generation: u64,
    /// Selected original source and listing facts.
    pub selection: Selection,
    /// Cancellation signal shared with the worker.
    pub cancel: Arc<AtomicBool>,
}

/// Current Properties loading state.
#[derive(Clone, Debug, Default)]
pub enum State {
    /// No selected source item.
    #[default]
    Empty,
    /// Background inspection is in flight.
    Loading(Request),
    /// The current selection has inspection data.
    Ready {
        selection: Selection,
        inspection: Box<Inspection>,
    },
    /// All requested inspection failed.
    Failed { selection: Selection, error: String },
}

/// Owns request generations, cancellation and stale-completion rejection.
#[derive(Clone, Debug, Default)]
pub struct PropertiesSession {
    generation: u64,
    selection: Option<Selection>,
    state: State,
}

impl PropertiesSession {
    /// Current state, for a controller to render.
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Selects a new source and returns its cancellable background request.
    pub fn set_target(&mut self, target: Option<Selection>) -> Option<Request> {
        if self.selection == target {
            return None;
        }
        self.cancel_current();
        self.generation = self.generation.wrapping_add(1);
        self.selection = target.clone();
        let Some(selection) = target else {
            self.state = State::Empty;
            return None;
        };
        let request = Request {
            generation: self.generation,
            selection,
            cancel: Arc::new(AtomicBool::new(false)),
        };
        self.state = State::Loading(request.clone());
        Some(request)
    }

    /// Invalidates the selected data, usually after refresh or an editor save.
    pub fn invalidate(&mut self) -> Option<Request> {
        let target = self.selection.clone();
        self.selection = None;
        self.set_target(target)
    }

    /// Applies a completion only when it still belongs to the active request.
    pub fn complete(&mut self, request: &Request, result: Result<Inspection, String>) -> bool {
        let State::Loading(active) = &self.state else {
            return false;
        };
        if active.generation != request.generation
            || active.selection != request.selection
            || !Arc::ptr_eq(&active.cancel, &request.cancel)
            || active.cancel.load(Ordering::Relaxed)
            || request.cancel.load(Ordering::Relaxed)
        {
            return false;
        }
        if result
            .as_ref()
            .is_ok_and(|inspection| inspection.location != request.selection.location)
        {
            return false;
        }
        match result {
            Ok(inspection) => {
                self.state = State::Ready {
                    selection: request.selection.clone(),
                    inspection: Box::new(inspection),
                }
            }
            Err(error) => {
                self.state = State::Failed {
                    selection: request.selection.clone(),
                    error,
                }
            }
        }
        true
    }

    fn cancel_current(&mut self) {
        if let State::Loading(request) = &self.state {
            request.cancel.store(true, Ordering::Relaxed);
        }
    }
}

/// Reads bounded properties for a selection; call from a background worker.
pub fn inspect(selection: &Selection, cancel: &Arc<AtomicBool>) -> io::Result<Inspection> {
    inspect_with_registry(selection, cancel, &Registry::with_defaults())
}

/// Reads bounded properties using the providers held by the application.
pub fn inspect_with_registry(
    selection: &Selection,
    cancel: &Arc<AtomicBool>,
    registry: &Registry,
) -> io::Result<Inspection> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
    }
    let listing = ListingFacts {
        name: selection.entry.name.clone(),
        is_dir: selection.entry.is_dir,
        size: selection.entry.size,
        modified: selection.entry.modified,
        attrs: match &selection.location {
            Location::Local(_) => Some(selection.entry.attrs),
            Location::Remote(_) | Location::Archive { .. } => None,
        },
        link: selection.entry.link.clone(),
    };
    let (filesystem, exif, additional_groups) = match &selection.location {
        Location::Local(path) => {
            let current = fs::metadata(path.as_path());
            let regular_file = current.as_ref().is_ok_and(fs::Metadata::is_file);
            let fs = match filesystem::read(path.as_path(), cancel) {
                Ok(facts) => Section::Available(facts),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                    return Err(error);
                }
                Err(error) => Section::Failed(error.to_string()),
            };
            let exif = if !regular_file {
                Section::Unavailable("EXIF is available only for regular files".into())
            } else if selection.entry.is_dir {
                Section::Unavailable("directories have no EXIF metadata".into())
            } else {
                match exif_reader::read(path.as_path(), cancel) {
                    Ok(read) if read.truncated => Section::Truncated {
                        value: read.fields,
                        limit: read.limit,
                    },
                    Ok(read) if read.fields.is_empty() => {
                        Section::Unavailable("no supported EXIF fields".into())
                    }
                    Ok(read) => Section::Available(read.fields),
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                        return Err(error);
                    }
                    Err(error) => Section::Failed(error.to_string()),
                }
            };
            let additional_groups = registry.read(path.as_path(), cancel)?;
            (fs, exif, additional_groups)
        }
        Location::Remote(_) => (
            Section::Unavailable(
                "local filesystem attributes are unavailable for remote items".into(),
            ),
            Section::Unavailable("EXIF is not read from remote items".into()),
            Vec::new(),
        ),
        Location::Archive { .. } => (
            Section::Unavailable(
                "local filesystem attributes are unavailable for archive members".into(),
            ),
            Section::Unavailable("EXIF is not read from archive members".into()),
            Vec::new(),
        ),
    };
    Ok(Inspection {
        location: selection.location.clone(),
        listing,
        filesystem,
        exif,
        additional_groups,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        GroupId, GroupStatus, Probe, PropertiesSession, PropertyField, PropertyGroup,
        PropertyProvider, PropertyTable, Registry, Section, Selection, State, inspect,
    };
    use crate::entry::{Attrs, Entry};
    use crate::location::Location;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use tempfile::TempDir;

    fn property_group(status: GroupStatus) -> PropertyGroup {
        PropertyGroup {
            id: GroupId("format".into()),
            status,
            fields: Vec::<PropertyField>::new(),
            groups: Vec::new(),
            tables: Vec::<PropertyTable>::new(),
        }
    }

    struct TestProvider {
        id: &'static str,
        fails: bool,
    }

    impl PropertyProvider for TestProvider {
        fn id(&self) -> &'static str {
            self.id
        }

        fn matches(&self, _probe: &Probe) -> bool {
            true
        }

        fn read(
            &self,
            _probe: &Probe,
            _cancel: &Arc<AtomicBool>,
        ) -> std::io::Result<Vec<PropertyGroup>> {
            if self.fails {
                return Err(std::io::Error::other("test reader failed"));
            }
            Ok(vec![PropertyGroup {
                id: GroupId(self.id.into()),
                status: GroupStatus::Available,
                fields: Vec::new(),
                groups: Vec::new(),
                tables: Vec::new(),
            }])
        }
    }

    fn test_registry(providers: &[(&'static str, bool)]) -> Registry {
        let mut registry = Registry::default();
        for (id, fails) in providers {
            registry.register(Box::new(TestProvider { id, fails: *fails }));
        }
        registry
    }

    #[test]
    fn registry_reads_all_matching_providers_in_registration_order() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("item.bin");
        std::fs::write(&path, b"data").expect("fixture");
        let registry = test_registry(&[("first", false), ("second", false)]);

        let groups = registry
            .read(&path, &Arc::new(AtomicBool::new(false)))
            .expect("provider groups");

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].id.0, "first");
        assert_eq!(groups[1].id.0, "second");
    }

    #[test]
    fn registry_keeps_provider_failures_independent_of_later_results() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("item.bin");
        std::fs::write(&path, b"data").expect("fixture");
        let registry = test_registry(&[("broken", true), ("working", false)]);

        let groups = registry
            .read(&path, &Arc::new(AtomicBool::new(false)))
            .expect("independent provider results");

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].id.0, "broken");
        assert!(matches!(&groups[0].status, GroupStatus::Failed(_)));
        assert_eq!(groups[1].id.0, "working");
        assert_eq!(&groups[1].status, &GroupStatus::Available);
    }

    #[test]
    fn absent_empty_failed_and_truncated_sections_have_visibility_policy() {
        assert!(!Section::<Vec<u8>>::Unavailable("not available".into()).is_visible());
        assert!(!Section::<Vec<u8>>::Available(Vec::new()).is_visible());
        assert!(Section::<Vec<u8>>::Failed("read failed".into()).is_visible());
        assert!(
            Section::<Vec<u8>>::Truncated {
                value: Vec::<u8>::new(),
                limit: 1024,
            }
            .is_visible()
        );
    }

    #[test]
    fn property_group_visibility_includes_nonempty_children_and_tables() {
        assert!(!property_group(GroupStatus::Unavailable("not applicable".into())).is_visible());
        assert!(!property_group(GroupStatus::Available).is_visible());
        assert!(property_group(GroupStatus::Failed("reader failed".into())).is_visible());

        let mut parent = property_group(GroupStatus::Available);
        parent.groups.push(property_group(GroupStatus::Truncated {
            reason: "limited schema output".into(),
        }));
        assert!(parent.is_visible());

        let mut table_group = property_group(GroupStatus::Available);
        table_group.tables.push(PropertyTable {
            id: "schema".into(),
            columns: vec!["name".into()],
            rows: Vec::new(),
        });
        assert!(table_group.is_visible());
    }

    #[test]
    fn empty_exif_success_is_hidden_but_errors_and_truncation_are_visible() {
        assert!(!Section::<Vec<super::ExifField>>::Available(Vec::new()).is_visible());
        assert!(!Section::<Vec<super::ExifField>>::Unavailable("no EXIF".into()).is_visible());
        assert!(Section::<Vec<super::ExifField>>::Failed("invalid EXIF".into()).is_visible());
        assert!(
            Section::<Vec<super::ExifField>>::Truncated {
                value: Vec::new(),
                limit: 4096,
            }
            .is_visible()
        );
    }

    fn selection(path: PathBuf) -> Selection {
        Selection {
            location: Location::local(path.clone()).expect("absolute temp path"),
            entry: Entry {
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                path,
                is_dir: false,
                is_parent: false,
                size: None,
                modified: None,
                attrs: Attrs::default(),
                link: Default::default(),
            },
        }
    }

    #[test]
    fn local_inspection_keeps_listing_and_filesystem_facts() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("a.txt");
        std::fs::write(&path, "hello").expect("write");
        let result =
            inspect(&selection(path), &Arc::new(AtomicBool::new(false))).expect("inspection");
        assert!(matches!(result.filesystem, super::Section::Available(_)));
        assert!(matches!(result.exif, super::Section::Unavailable(_)));
    }

    #[cfg(unix)]
    #[test]
    fn local_inspection_retains_native_owner_and_group_ids() {
        use std::os::unix::fs::MetadataExt;
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("owned.txt");
        std::fs::write(&path, "hello").expect("write");
        let meta = std::fs::symlink_metadata(&path).expect("metadata");
        let result =
            inspect(&selection(path), &Arc::new(AtomicBool::new(false))).expect("inspection");
        let Section::Available(facts) = result.filesystem else {
            panic!("filesystem facts available");
        };
        assert_eq!(facts.link_object.attrs.uid, Some(meta.uid()));
        assert_eq!(facts.link_object.gid, Some(meta.gid()));
    }

    #[test]
    fn filesystem_failure_does_not_discard_listing_facts() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("missing.jpg");
        let mut selected = selection(path.clone());
        selected.entry.size = Some(17);
        let result = inspect(&selected, &Arc::new(AtomicBool::new(false))).expect("partial result");
        assert_eq!(result.listing.size, Some(17));
        assert!(matches!(result.filesystem, super::Section::Failed(_)));
        assert!(matches!(result.exif, super::Section::Unavailable(_)));
    }

    #[test]
    fn missing_probe_path_keeps_listing_and_filesystem_facts() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("missing.txt");
        let mut selected = selection(path);
        selected.entry.size = Some(17);

        let result = inspect(&selected, &Arc::new(AtomicBool::new(false)))
            .expect("missing source retains partial inspection");

        assert_eq!(result.listing.size, Some(17));
        assert!(matches!(result.filesystem, Section::Failed(_)));
        assert!(matches!(result.exif, Section::Unavailable(_)));
        assert!(result.additional_groups.is_empty());
    }

    #[test]
    fn malformed_parquet_keeps_filesystem_facts_and_reports_only_its_group_failure() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("bad.parquet");
        std::fs::write(&path, b"invalid parquet").expect("write fixture");
        let result =
            inspect(&selection(path), &Arc::new(AtomicBool::new(false))).expect("inspection");
        assert!(matches!(result.filesystem, super::Section::Available(_)));
        assert!(matches!(result.exif, super::Section::Unavailable(_)));
        assert_eq!(result.additional_groups.len(), 1);
        assert!(matches!(
            result.additional_groups[0].status,
            GroupStatus::Failed(_)
        ));
    }

    #[test]
    fn malformed_sqlite_keeps_filesystem_facts_and_reports_its_group_failure() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("bad.sqlite");
        std::fs::write(&path, b"invalid database").expect("write fixture");
        let result = inspect(&selection(path), &Arc::new(AtomicBool::new(false)))
            .expect("partial inspection");
        assert!(matches!(result.filesystem, Section::Available(_)));
        assert!(matches!(
            result.additional_groups[0].status,
            GroupStatus::Failed(_)
        ));
        assert_eq!(result.additional_groups[0].id.0, "sqlite");
    }

    #[test]
    fn inspection_cancellation_is_reported_as_interrupted() {
        let temp = TempDir::new().expect("tempdir");
        let selected = selection(temp.path().join("photo.jpg"));
        let cancel = Arc::new(AtomicBool::new(true));
        assert_eq!(
            inspect(&selected, &cancel).expect_err("cancelled").kind(),
            std::io::ErrorKind::Interrupted
        );
    }

    #[test]
    fn remote_inspection_uses_listing_facts_without_native_attributes() {
        let source =
            Location::from_panel_path(std::path::Path::new("cloud:/photos/image.jpg"), None)
                .expect("remote location");
        let selected = Selection {
            location: source,
            entry: Entry {
                name: "image.jpg".into(),
                path: PathBuf::from("cloud:/photos/image.jpg"),
                is_dir: false,
                is_parent: false,
                size: Some(42),
                modified: None,
                attrs: Attrs::default(),
                link: Default::default(),
            },
        };
        let result =
            inspect(&selected, &Arc::new(AtomicBool::new(false))).expect("remote listing result");
        assert_eq!(result.listing.size, Some(42));
        assert_eq!(result.listing.attrs, None);
        assert!(matches!(result.filesystem, super::Section::Unavailable(_)));
        assert!(result.additional_groups.is_empty());
    }

    #[test]
    fn archive_inspection_uses_listing_facts_without_opening_archive_source() {
        let temp = TempDir::new().expect("tempdir");
        let missing_archive = temp.path().join("missing.zip");
        let location = Location::archive(
            Location::local(missing_archive).expect("absolute source"),
            "folder/item.jpg",
        )
        .expect("archive member");
        let selected = Selection {
            location,
            entry: Entry {
                name: "item.jpg".into(),
                path: temp.path().join("missing.zip/folder/item.jpg"),
                is_dir: false,
                is_parent: false,
                size: Some(23),
                modified: None,
                attrs: Attrs::default(),
                link: Default::default(),
            },
        };
        let result =
            inspect(&selected, &Arc::new(AtomicBool::new(false))).expect("archive listing result");
        assert_eq!(result.listing.size, Some(23));
        assert_eq!(result.listing.attrs, None);
        assert!(matches!(result.filesystem, super::Section::Unavailable(_)));
        assert!(matches!(result.exif, super::Section::Unavailable(_)));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_inspection_distinguishes_link_from_broken_target() {
        use std::os::unix::fs::symlink;
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("broken");
        symlink("missing", &path).expect("symlink");
        let facts = super::filesystem::read(&path, &AtomicBool::new(false)).expect("facts");
        assert!(facts.link_object.attrs.mode.is_some());
        assert_eq!(facts.symlink_target, Some(PathBuf::from("missing")));
        assert!(facts.target.is_none());
        assert!(facts.target_error.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_and_target_keep_their_own_native_ownership() {
        use std::os::unix::fs::{MetadataExt, symlink};
        let temp = TempDir::new().expect("tempdir");
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        std::fs::write(&target, "data").expect("target");
        symlink(&target, &link).expect("link");
        let link_meta = std::fs::symlink_metadata(&link).expect("link metadata");
        let target_meta = std::fs::metadata(&link).expect("target metadata");
        let facts = super::filesystem::read(&link, &AtomicBool::new(false)).expect("facts");
        assert_eq!(facts.link_object.attrs.uid, Some(link_meta.uid()));
        assert_eq!(facts.link_object.gid, Some(link_meta.gid()));
        let followed = facts.target.expect("target facts");
        assert_eq!(followed.attrs.uid, Some(target_meta.uid()));
        assert_eq!(followed.gid, Some(target_meta.gid()));
    }

    #[cfg(unix)]
    #[test]
    fn parquet_symlink_inspects_target_without_losing_link_facts() {
        use std::os::unix::fs::symlink;
        let temp = TempDir::new().expect("tempdir");
        let target = temp.path().join("target.parquet");
        let link = temp.path().join("link.parquet");
        let schema = std::sync::Arc::new(
            parquet::schema::parser::parse_message_type("message sample { REQUIRED INT32 value; }")
                .expect("schema"),
        );
        let writer = parquet::file::writer::SerializedFileWriter::new(
            std::fs::File::create(&target).expect("target"),
            schema,
            Default::default(),
        )
        .expect("writer");
        writer.close().expect("close");
        symlink(&target, &link).expect("symlink");
        let result =
            inspect(&selection(link), &Arc::new(AtomicBool::new(false))).expect("inspection");
        let filesystem = match result.filesystem {
            super::Section::Available(value) => value,
            _ => panic!("filesystem facts available"),
        };
        assert!(filesystem.symlink_target.is_some());
        assert!(
            result
                .additional_groups
                .iter()
                .any(|group| group.id.0 == "parquet")
        );
    }

    #[test]
    fn session_cancels_and_rejects_old_generation() {
        let temp = TempDir::new().expect("tempdir");
        let mut session = PropertiesSession::default();
        let first = session
            .set_target(Some(selection(temp.path().join("one"))))
            .expect("request");
        let second = session
            .set_target(Some(selection(temp.path().join("two"))))
            .expect("request");
        assert!(first.cancel.load(std::sync::atomic::Ordering::Relaxed));
        let dummy = super::Inspection {
            location: second.selection.location.clone(),
            listing: super::ListingFacts {
                name: "two".into(),
                is_dir: false,
                size: None,
                modified: None,
                attrs: Some(Attrs::default()),
                link: Default::default(),
            },
            filesystem: super::Section::Unavailable("x".into()),
            exif: super::Section::Unavailable("x".into()),
            additional_groups: Vec::new(),
        };
        assert!(!session.complete(&first, Ok(dummy.clone())));
        assert!(session.complete(&second, Ok(dummy)));
        assert!(matches!(session.state(), State::Ready { .. }));
        let refresh = session.invalidate().expect("refresh request");
        assert!(refresh.generation > second.generation);
    }
}
