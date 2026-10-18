//! Native and enabled extension Properties provider orchestration.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{geo, media, package, parquet, sqlite, xattrs};
use crate::extensions::properties::{Handle, PreparedSection};
use crate::properties::{GroupId, GroupStatus, PropertyGroup, Selection};

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
        let metadata = fs::metadata(path).or_else(|_| fs::symlink_metadata(path))?;
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
    extensions: Vec<Handle>,
    git: Option<super::git::Provider>,
}

impl Registry {
    /// Built-in format property providers.
    pub fn with_defaults() -> Self {
        let mut registry = Self::default();
        registry.register(Box::new(parquet::Provider));
        registry.register(Box::new(sqlite::Provider));
        registry.register(Box::new(package::Provider));
        registry.register(Box::new(media::Provider));
        registry.register(Box::new(geo::Provider::new()));
        registry.register(Box::new(xattrs::Provider));
        registry
    }

    /// Adds a provider after existing providers.
    pub fn register(&mut self, provider: Box<dyn PropertyProvider>) {
        self.providers.push(provider);
    }

    /// Registers contextual Git using the same snapshot service as the panels.
    pub fn register_git(&mut self, service: crate::data_providers::git::Service) {
        self.git = Some(super::git::Provider::new(service));
    }

    /// Reads repository context only for the captured original local selection.
    pub fn read_context(
        &self,
        selection: &Selection,
        key: Option<&crate::data_providers::git::Key>,
        cancel: &Arc<AtomicBool>,
    ) -> io::Result<Vec<PropertyGroup>> {
        match (&self.git, key) {
            (Some(provider), Some(key)) => provider.read(selection, key, cancel),
            _ => Ok(Vec::new()),
        }
    }

    /// Registers a section from the immutable enabled-extension snapshot.
    pub fn register_extension(&mut self, handle: Handle) {
        self.extensions.push(handle);
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

    /// Reads enabled sections for the original local regular-file selection.
    pub fn read_extensions(
        &self,
        selection: &Selection,
        cancel: &Arc<AtomicBool>,
    ) -> io::Result<Vec<PreparedSection>> {
        let mut sections = Vec::new();
        for handle in &self.extensions {
            if cancel.load(Ordering::Acquire) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            match handle.read(selection, cancel) {
                Ok(section) => sections.push(section),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => return Err(error),
                Err(error) => sections.push(handle.failure(&error)),
            }
        }
        Ok(sections)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::io;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::Registry;
    use crate::data_providers::{Completeness, ResultState, Unit, Value};
    use crate::entry::{Attrs, Entry};
    use crate::extensions::properties::Handle;
    use crate::location::Location;
    use crate::properties::{
        GroupId, GroupStatus, Probe, PropertyGroup, PropertyProvider, PropertyTable, Selection,
        inspect_with_registry,
    };
    use tempfile::TempDir;

    const COMPLETE_SENSOR: &str =
        include_str!("../../examples/extensions/sensor/fixtures/complete.sens");
    const MALFORMED_SENSOR: &str =
        include_str!("../../examples/extensions/sensor/fixtures/malformed.sens");

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
    fn package_failure_preserves_filesystem_and_nonlocal_sources_are_not_read() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("bad.rpm");
        std::fs::write(&path, b"broken package").unwrap();
        let selected = selection(path.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let registry = Registry::with_defaults();
        let inspection = inspect_with_registry(&selected, &cancel, &registry).unwrap();
        assert!(matches!(
            inspection.filesystem,
            crate::properties::Section::Available(_)
        ));
        assert!(
            inspection
                .additional_groups
                .iter()
                .any(|group| group.id.0 == "package"
                    && matches!(group.status, GroupStatus::Failed(_)))
        );
        let mut archived = selected;
        archived.location = Location::archive(
            Location::local(temp.path().join("outer.zip")).unwrap(),
            "bad.rpm",
        )
        .unwrap();
        assert!(
            inspect_with_registry(&archived, &cancel, &registry)
                .unwrap()
                .additional_groups
                .is_empty()
        );
        let directory = temp.path().join("directory.rpm");
        std::fs::create_dir(&directory).unwrap();
        assert!(
            registry
                .read(&directory, &cancel)
                .unwrap()
                .iter()
                .all(|group| group.id.0 != "package")
        );
    }

    #[test]
    fn dangling_link_probe_preserves_the_object_for_no_follow_metadata() {
        let temp = TempDir::new().unwrap();
        let link = temp.path().join("dangling");
        std::os::unix::fs::symlink(temp.path().join("missing"), &link).unwrap();
        let probe = Probe::read(&link).expect("link objects remain inspectable without a target");
        assert_eq!(probe.path, link);
        assert_eq!(probe.name, "dangling");
        assert!(!probe.is_file);
        assert!(!probe.is_dir);
    }

    struct TestProvider;

    impl PropertyProvider for TestProvider {
        fn id(&self) -> &'static str {
            "native"
        }
        fn matches(&self, _probe: &Probe) -> bool {
            true
        }
        fn read(
            &self,
            _probe: &Probe,
            _cancel: &Arc<AtomicBool>,
        ) -> io::Result<Vec<PropertyGroup>> {
            Ok(vec![PropertyGroup {
                id: GroupId("native".into()),
                status: GroupStatus::Available,
                fields: Vec::new(),
                groups: Vec::new(),
                tables: Vec::<PropertyTable>::new(),
            }])
        }
    }

    struct CancellingProvider;

    impl PropertyProvider for CancellingProvider {
        fn id(&self) -> &'static str {
            "canceller"
        }
        fn matches(&self, _probe: &Probe) -> bool {
            true
        }
        fn read(&self, _probe: &Probe, cancel: &Arc<AtomicBool>) -> io::Result<Vec<PropertyGroup>> {
            cancel.store(true, Ordering::Release);
            Ok(Vec::new())
        }
    }

    fn extension_registry(
        counter: Option<Arc<AtomicUsize>>,
    ) -> (Registry, crate::extensions::Supervisor) {
        use crate::data_providers::ContributionId;
        use crate::extensions::manifest::{self, PackageId};

        let package_id = PackageId::new("sensor").expect("fixture package ID");
        let manifest = manifest::parse(
            &package_id,
            include_str!("../../examples/extensions/sensor/manifest.toml"),
        )
        .expect("sensor manifest");
        let package = Arc::new(crate::extensions::Package {
            manifest,
            source: include_str!("../../examples/extensions/sensor/main.lua").into(),
        });
        let supervisor = crate::extensions::Supervisor::fixture_for_tests_with_counter(counter)
            .expect("test supervisor fixture");
        let handle = Handle::new(
            package,
            ContributionId::new("extension.sensor.reader.summary")
                .expect("fixture contribution ID"),
            supervisor.clone(),
        )
        .expect("sensor Properties handle");
        let mut registry = Registry::default();
        registry.register_extension(handle);
        (registry, supervisor)
    }

    #[test]
    fn extensions_are_gated_by_the_original_local_regular_file_source() {
        let temp = TempDir::new().expect("tempdir");
        let launches = Arc::new(AtomicUsize::new(0));
        let (registry, supervisor) = extension_registry(Some(launches.clone()));
        let remote_location =
            Location::from_panel_path(std::path::Path::new("cloud:/photos/image.sens"), None)
                .expect("remote location");
        let remote = Selection {
            location: remote_location,
            entry: Entry {
                name: "image.sens".into(),
                path: PathBuf::from("cloud:/photos/image.sens"),
                is_dir: false,
                is_parent: false,
                size: Some(5),
                modified: None,
                attrs: Attrs::default(),
                link: Default::default(),
            },
        };
        assert!(
            inspect_with_registry(&remote, &Arc::new(AtomicBool::new(false)), &registry)
                .expect("remote listing")
                .extension_sections
                .is_empty()
        );
        assert_eq!(launches.load(Ordering::Relaxed), 0);

        let archive_source =
            Location::local(temp.path().join("archive.zip")).expect("local archive path");
        let archive = Selection {
            location: Location::archive(archive_source, "image.sens").expect("archive member"),
            entry: Entry {
                name: "image.sens".into(),
                path: temp.path().join("archive.zip/image.sens"),
                is_dir: false,
                is_parent: false,
                size: Some(5),
                modified: None,
                attrs: Attrs::default(),
                link: Default::default(),
            },
        };
        assert!(
            inspect_with_registry(&archive, &Arc::new(AtomicBool::new(false)), &registry)
                .expect("archive listing")
                .extension_sections
                .is_empty()
        );
        assert_eq!(launches.load(Ordering::Relaxed), 0);

        let directory = temp.path().join("directory.sens");
        std::fs::create_dir(&directory).expect("create source directory");
        let mut directory_selection = selection(directory);
        directory_selection.entry.is_dir = true;
        assert!(
            inspect_with_registry(
                &directory_selection,
                &Arc::new(AtomicBool::new(false)),
                &registry,
            )
            .expect("directory listing")
            .extension_sections
            .is_empty()
        );
        assert_eq!(launches.load(Ordering::Relaxed), 0);

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let target = temp.path().join("target.sens");
            let link = temp.path().join("link.sens");
            std::fs::write(&target, COMPLETE_SENSOR).expect("write target");
            symlink(&target, &link).expect("create symlink");
            let inspection = inspect_with_registry(
                &selection(link),
                &Arc::new(AtomicBool::new(false)),
                &registry,
            )
            .expect("symlink listing");
            assert_eq!(inspection.extension_sections.len(), 1);
            assert_summary(&inspection.extension_sections[0].result);
            assert_eq!(launches.load(Ordering::Relaxed), 1);
        }
        supervisor.shutdown().expect("test supervisor shutdown");
    }

    #[test]
    fn extension_cancellation_is_interrupted_before_a_worker_request() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("cancelled.sens");
        std::fs::write(&path, "fixture").expect("write source");
        let mut selected = selection(path);
        selected.entry.name = "cancelled.sens".into();
        let launches = Arc::new(AtomicUsize::new(0));
        let (registry, supervisor) = extension_registry(Some(launches.clone()));
        let cancel = Arc::new(AtomicBool::new(true));
        assert_eq!(
            inspect_with_registry(&selected, &cancel, &registry)
                .expect_err("cancellation stops inspection")
                .kind(),
            std::io::ErrorKind::Interrupted
        );
        assert_eq!(launches.load(Ordering::Relaxed), 0);
        supervisor.shutdown().expect("test supervisor shutdown");
    }

    #[test]
    fn native_and_extension_properties_are_both_preserved() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("complete.sens");
        std::fs::write(&path, COMPLETE_SENSOR).expect("write source");
        let selected = selection(path);
        let (mut registry, supervisor) = extension_registry(None);
        registry.register(Box::new(TestProvider));
        let inspection =
            inspect_with_registry(&selected, &Arc::new(AtomicBool::new(false)), &registry)
                .expect("native inspection remains available");
        assert_eq!(inspection.additional_groups.len(), 1);
        assert_eq!(inspection.additional_groups[0].id.0, "native");
        assert_eq!(inspection.extension_sections.len(), 1);
        assert_summary(&inspection.extension_sections[0].result);
        supervisor.shutdown().expect("test supervisor shutdown");
    }

    #[test]
    fn malformed_extension_result_is_visible_without_removing_native_groups() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("malformed.sens");
        std::fs::write(&path, MALFORMED_SENSOR).expect("write source");
        let selected = selection(path);
        let (mut registry, supervisor) = extension_registry(None);
        registry.register(Box::new(TestProvider));
        let inspection =
            inspect_with_registry(&selected, &Arc::new(AtomicBool::new(false)), &registry)
                .expect("native inspection remains available");
        assert_eq!(inspection.additional_groups.len(), 1);
        assert_eq!(inspection.extension_sections.len(), 1);
        assert!(matches!(
            &inspection.extension_sections[0].result,
            crate::extensions::results::PropertiesResult::Failed(reason)
                if reason.contains("sample times must increase")
        ));
        supervisor.shutdown().expect("test supervisor shutdown");
    }

    fn assert_summary(result: &crate::extensions::results::PropertiesResult) {
        let crate::extensions::results::PropertiesResult::Ready(fields) = result else {
            panic!("expected complete sensor Properties values, got {result:?}");
        };
        let duration = fields
            .iter()
            .find(|field| field.field.as_str() == "extension.sensor.reader.duration")
            .expect("duration field is present");
        let distance = fields
            .iter()
            .find(|field| field.field.as_str() == "extension.sensor.reader.distance")
            .expect("distance field is present");
        for (field, expected, unit) in [
            (duration, 30.0, Unit::Seconds),
            (distance, 95.0, Unit::Metres),
        ] {
            assert!(matches!(
                &field.state,
                ResultState::Ready {
                    value: Value::Quantity(quantity),
                    completeness: Completeness::Complete,
                } if quantity.value() == expected && quantity.unit() == unit
            ));
        }
    }

    #[test]
    fn cancellation_after_native_read_prevents_extension_fields_from_completing() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("cancelled.sens");
        std::fs::write(&path, "fixture").expect("write source");
        let selected = selection(path);
        let launches = Arc::new(AtomicUsize::new(0));
        let (mut registry, supervisor) = extension_registry(Some(launches.clone()));
        registry.register(Box::new(CancellingProvider));
        let cancel = Arc::new(AtomicBool::new(false));
        assert_eq!(
            inspect_with_registry(&selected, &cancel, &registry)
                .expect_err("extension phase observes cancellation")
                .kind(),
            std::io::ErrorKind::Interrupted
        );
        assert_eq!(launches.load(Ordering::Relaxed), 0);
        supervisor.shutdown().expect("test supervisor shutdown");
    }
}
