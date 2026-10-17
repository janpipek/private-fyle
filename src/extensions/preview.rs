//! Adapts declared extension viewer contributions to native preview requests.

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::data_providers::{Applicability, ApplicabilityContext, ResultState, Subject};
use crate::location::Location;
use crate::preview::Probe;
use crate::preview_scene::Scene;

use super::loading::Package;
use super::results::PreparedResult;
use super::supervisor::{ContributionWork, Supervisor};
use crate::data_providers::ContributionId;

/// One supported, validated viewer contribution retained by the preview registry.
#[derive(Clone)]
pub struct Handle {
    package: Arc<Package>,
    id: ContributionId,
    label: String,
    applicability: Applicability,
    supervisor: Supervisor,
}

impl Handle {
    /// Creates a native scene handle for one declared `fyle.scene` contribution.
    pub fn new(
        package: Arc<Package>,
        id: ContributionId,
        supervisor: Supervisor,
    ) -> io::Result<Self> {
        let contribution = package
            .manifest
            .providers
            .iter()
            .flat_map(|provider| &provider.viewers)
            .find(|contribution| contribution.header.id == id)
            .ok_or_else(|| invalid("package does not declare this viewer contribution"))?;
        if contribution.representation != "fyle.scene" || contribution.version != 1 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "unsupported viewer representation or version",
            ));
        }
        let label = contribution.header.label.clone();
        let applicability = contribution.header.applicability.clone();
        Ok(Self {
            package,
            id,
            label,
            applicability,
            supervisor,
        })
    }

    /// Returns this contribution's stable identity for deterministic candidate order.
    pub fn id(&self) -> &ContributionId {
        &self.id
    }

    /// Returns the package's stable ID for deterministic candidate order.
    pub fn package_id(&self) -> &str {
        self.package.manifest.id.as_str()
    }

    /// Returns the manifest label for diagnostics and preview attribution.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Reads one selected source after checking its exact native-path grant and captured facts.
    pub fn read(
        &self,
        readable_path: &Path,
        identity: &Location,
        probe: &Probe,
        cancel: &Arc<AtomicBool>,
    ) -> io::Result<ResultState<Scene>> {
        if cancel.load(Ordering::Acquire) {
            return Err(interrupted());
        }
        if identity.as_local_path() != Some(readable_path) || probe.is_dir || !probe.is_file {
            return Ok(ResultState::Unavailable(
                "viewer requires its original local regular file".into(),
            ));
        }
        let context = ApplicabilityContext {
            subject: Subject::Item(identity.clone()),
            name: probe.name.clone(),
            is_dir: probe.is_dir,
            formats: Vec::new(),
            context_identity: None,
        };
        if !self.applicability.matches(&context) {
            return Ok(ResultState::Unavailable(
                "viewer does not apply to the captured source facts".into(),
            ));
        }
        let result = self
            .supervisor
            .submit_contribution(ContributionWork {
                package: self.package.clone(),
                contribution: self.id.clone(),
                source: identity.clone(),
                name: probe.name.clone(),
                is_dir: probe.is_dir,
                generation_cancelled: cancel.clone(),
            })?
            .wait();
        if cancel.load(Ordering::Acquire) {
            return Err(interrupted());
        }
        match result? {
            PreparedResult::Viewer(result) => Ok(result),
            _ => Err(invalid("worker returned a different presentation result")),
        }
    }
}

fn interrupted() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "cancelled")
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::Handle;
    use crate::extensions::Package;
    use crate::extensions::manifest::{self, PackageId};
    use crate::extensions::supervisor::Supervisor;
    use crate::location::Location;
    use crate::preview::{Content, PreviewStyle, Registry};
    use tempfile::TempDir;

    const MANIFEST: &str = include_str!("../../examples/extensions/sensor/manifest.toml");
    const SCRIPT: &str = include_str!("../../examples/extensions/sensor/main.lua");
    const COMPLETE: &str = include_str!("../../examples/extensions/sensor/fixtures/complete.sens");

    fn package() -> Arc<Package> {
        let id = PackageId::new("sensor").expect("fixture package ID is valid");
        Arc::new(Package {
            manifest: manifest::parse(&id, MANIFEST).expect("sensor manifest is valid"),
            source: SCRIPT.into(),
        })
    }

    #[test]
    fn exact_local_sensor_identity_runs_the_declared_scene_viewer() {
        let root = TempDir::new().expect("temporary sensor files");
        let path = root.path().join("ride.sens");
        std::fs::write(&path, COMPLETE).expect("write sensor fixture");
        let location = Location::local(path.clone()).expect("absolute local source");
        let package = package();
        let viewer = package.manifest.providers[0].viewers[0].header.id.clone();
        let launches = Arc::new(AtomicUsize::new(0));
        let supervisor = Supervisor::fixture_for_tests_with_counter(Some(launches.clone()))
            .expect("test supervisor starts");
        let handle =
            Handle::new(package, viewer, supervisor.clone()).expect("declared viewer handle");
        let mut registry = Registry::with_defaults();
        registry.register_extensions(vec![handle]);
        let preview = registry
            .preview_styled_for(
                &path,
                PreviewStyle::default(),
                Some(&location),
                &Arc::new(AtomicBool::new(false)),
            )
            .expect("sensor viewer returns a scene");
        assert_eq!(preview.kind, "Extension scene");
        assert!(matches!(preview.content, Content::Scene { .. }));
        assert_eq!(launches.load(Ordering::Relaxed), 1);
        supervisor.shutdown().expect("test supervisor shuts down");
    }

    #[test]
    fn materialized_path_without_matching_local_identity_never_runs_extension() {
        let root = TempDir::new().expect("temporary source");
        let path = root.path().join("ordinary.txt");
        std::fs::write(&path, "native text").expect("write native text");
        let foreign = Location::local(root.path().join("elsewhere")).expect("foreign identity");
        let launches = Arc::new(AtomicUsize::new(0));
        let supervisor = Supervisor::fixture_for_tests_with_counter(Some(launches.clone()))
            .expect("test supervisor starts");
        let mut registry = Registry::with_defaults();
        let package = package();
        let viewer = package.manifest.providers[0].viewers[0].header.id.clone();
        registry.register_extensions(vec![
            Handle::new(package, viewer, supervisor.clone()).expect("declared viewer handle"),
        ]);
        let preview = registry
            .preview_styled_for(
                &path,
                PreviewStyle::default(),
                Some(&foreign),
                &Arc::new(AtomicBool::new(false)),
            )
            .expect("native fallback is available");
        assert_eq!(preview.kind, "Text");
        assert_eq!(launches.load(Ordering::Relaxed), 0);
        supervisor.shutdown().expect("test supervisor shuts down");
    }

    #[test]
    fn an_absent_extension_viewer_continues_to_native_text_fallback() {
        let root = TempDir::new().expect("temporary source");
        let path = root.path().join("empty.sens");
        std::fs::write(&path, "FYLE-SENSOR 1\n").expect("write empty sensor fixture");
        let location = Location::local(path.clone()).expect("absolute local source");
        let launches = Arc::new(AtomicUsize::new(0));
        let supervisor = Supervisor::fixture_for_tests_with_counter(Some(launches.clone()))
            .expect("test supervisor starts");
        let package = package();
        let viewer = package.manifest.providers[0].viewers[0].header.id.clone();
        let mut registry = Registry::with_defaults();
        registry.register_extensions(vec![
            Handle::new(package, viewer, supervisor.clone()).expect("declared viewer handle"),
        ]);
        let preview = registry
            .preview_styled_for(
                &path,
                PreviewStyle::default(),
                Some(&location),
                &Arc::new(AtomicBool::new(false)),
            )
            .expect("absent contribution uses native fallback");
        assert_eq!(preview.kind, "Text");
        assert_eq!(launches.load(Ordering::Relaxed), 1);
        supervisor.shutdown().expect("test supervisor shuts down");
    }

    #[test]
    fn native_specialized_preview_takes_priority_without_starting_extension() {
        let root = TempDir::new().expect("temporary source");
        let path = root.path().join("route.gpx");
        std::fs::write(
            &path,
            r#"<gpx version="1.1" creator="test"><trk><name>Loop</name><trkseg><trkpt lat="0" lon="0"><time>2020-01-01T00:00:00Z</time></trkpt><trkpt lat="0" lon="0.001"><time>2020-01-01T00:01:01Z</time></trkpt></trkseg></trk></gpx>"#,
        )
        .expect("write native GPX fixture");
        let location = Location::local(path.clone()).expect("absolute local source");
        let launches = Arc::new(AtomicUsize::new(0));
        let supervisor = Supervisor::fixture_for_tests_with_counter(Some(launches.clone()))
            .expect("test supervisor starts");
        let package = package();
        let viewer = package.manifest.providers[0].viewers[0].header.id.clone();
        let mut registry = Registry::with_defaults();
        registry.register_extensions(vec![
            Handle::new(package, viewer, supervisor.clone()).expect("declared viewer handle"),
        ]);
        let preview = registry
            .preview_styled_for(
                &path,
                PreviewStyle::default(),
                Some(&location),
                &Arc::new(AtomicBool::new(false)),
            )
            .expect("native GPX preview succeeds");
        assert_eq!(preview.kind, "Route / activity");
        assert!(matches!(preview.content, Content::Geo { .. }));
        assert_eq!(launches.load(Ordering::Relaxed), 0);
        supervisor.shutdown().expect("test supervisor shuts down");
    }

    #[test]
    fn remote_archive_and_missing_identity_never_authorize_materialized_paths() {
        let root = TempDir::new().expect("temporary source");
        let path = root.path().join("ride.sens");
        std::fs::write(&path, COMPLETE).expect("write sensor fixture");
        let archive = Location::archive(
            Location::local(root.path().join("outer.zip")).expect("archive path"),
            "ride.sens",
        )
        .expect("archive identity");
        let remote = Location::Remote(
            crate::location::RemoteLocation::root("drive").expect("remote identity"),
        );
        let launches = Arc::new(AtomicUsize::new(0));
        let supervisor = Supervisor::fixture_for_tests_with_counter(Some(launches.clone()))
            .expect("test supervisor starts");
        let package = package();
        let viewer = package.manifest.providers[0].viewers[0].header.id.clone();
        let mut registry = Registry::with_defaults();
        registry.register_extensions(vec![
            Handle::new(package, viewer, supervisor.clone()).expect("declared viewer handle"),
        ]);
        for identity in [Some(remote), Some(archive), None] {
            let preview = registry
                .preview_styled_for(
                    &path,
                    PreviewStyle::default(),
                    identity.as_ref(),
                    &Arc::new(AtomicBool::new(false)),
                )
                .expect("native fallback handles unauthorized identity");
            assert_eq!(preview.kind, "Text");
        }
        assert_eq!(launches.load(Ordering::Relaxed), 0);
        supervisor.shutdown().expect("test supervisor shuts down");
    }

    #[test]
    fn enabled_viewer_handles_sort_by_package_then_contribution_id() {
        let root = TempDir::new().expect("temporary sensor source");
        let path = root.path().join("ride.sens");
        std::fs::write(&path, COMPLETE).expect("write sensor fixture");
        let location = Location::local(path.clone()).expect("absolute local source");
        let launches = Arc::new(AtomicUsize::new(0));
        let supervisor = Supervisor::fixture_for_tests_with_counter(Some(launches.clone()))
            .expect("test supervisor starts");
        let mut handles = Vec::new();
        for (owner, label) in [("zeta", "Zeta"), ("alpha", "Alpha")] {
            let id = PackageId::new(owner).expect("package ID");
            let manifest_text = MANIFEST.replace("sensor", owner);
            let mut manifest = manifest::parse(&id, &manifest_text).expect("valid owner bundle");
            manifest.providers[0].viewers[0].header.label = label.into();
            let package = Arc::new(Package {
                manifest,
                source: SCRIPT.replace("sensor", owner),
            });
            let id = package.manifest.providers[0].viewers[0].header.id.clone();
            handles.push(
                Handle::new(package, id, supervisor.clone()).expect("declared viewer handle"),
            );
        }
        let mut registry = Registry::with_defaults();
        registry.register_extensions(handles);
        let preview = registry
            .preview_styled_for(
                &path,
                PreviewStyle::default(),
                Some(&location),
                &Arc::new(AtomicBool::new(false)),
            )
            .expect("lexically first package reads the source");
        assert!(matches!(preview.content, Content::Scene { label, .. } if label == "Alpha"));
        assert_eq!(launches.load(Ordering::Relaxed), 1);
        supervisor.shutdown().expect("test supervisor shuts down");
    }

    fn competing_package(first_callback: &str) -> Arc<Package> {
        let id = PackageId::new("sensor").expect("fixture package ID");
        let mut manifest = manifest::parse(&id, MANIFEST).expect("sensor manifest is valid");
        let mut first = manifest.providers[0].viewers[0].clone();
        first.header.id =
            crate::data_providers::ContributionId::new("extension.sensor.reader.graph-alpha")
                .expect("first viewer ID");
        first.header.label = "Alpha viewer".into();
        manifest.providers[0].viewers[0].header.id =
            crate::data_providers::ContributionId::new("extension.sensor.reader.graph-zeta")
                .expect("ready viewer ID");
        manifest.providers[0].viewers[0].header.label = "Ready viewer".into();
        manifest.providers[0].viewers.push(first);
        let script = SCRIPT.replace(
            "[owner .. \"graph\"] = contribution(graph),",
            &format!(
                "[\"extension.sensor.reader.graph-alpha\"] = {{ applies = function() return true end, read = {first_callback} }},\n  [owner .. \"graph-zeta\"] = contribution(graph),"
            ),
        );
        Arc::new(Package {
            manifest,
            source: script,
        })
    }

    fn competing_registry(
        supervisor: &Supervisor,
        package: Arc<Package>,
        contribution_suffixes: &[&str],
    ) -> Registry {
        let mut registry = Registry::with_defaults();
        registry.register_extensions(
            contribution_suffixes
                .iter()
                .map(|suffix| {
                    let id = crate::data_providers::ContributionId::new(format!(
                        "extension.sensor.reader.{suffix}"
                    ))
                    .expect("viewer contribution ID");
                    Handle::new(package.clone(), id, supervisor.clone())
                        .expect("declared competing viewer")
                })
                .collect(),
        );
        registry
    }

    #[test]
    fn sorted_absent_candidate_continues_and_failed_candidate_stops() {
        let root = TempDir::new().expect("temporary source");
        let path = root.path().join("ride.sens");
        std::fs::write(&path, COMPLETE).expect("write sensor fixture");
        let location = Location::local(path.clone()).expect("absolute local source");
        let launches = Arc::new(AtomicUsize::new(0));
        let supervisor = Supervisor::fixture_for_tests_with_counter(Some(launches.clone()))
            .expect("test supervisor starts");

        let absent = competing_package("function() return { state = \"absent\" } end");
        let registry = competing_registry(&supervisor, absent, &["graph-zeta", "graph-alpha"]);
        let preview = registry
            .preview_styled_for(
                &path,
                PreviewStyle::default(),
                Some(&location),
                &Arc::new(AtomicBool::new(false)),
            )
            .expect("second candidate produces the scene");
        assert!(matches!(preview.content, Content::Scene { label, .. } if label == "Ready viewer"));
        assert_eq!(launches.load(Ordering::Relaxed), 2);

        launches.store(0, Ordering::Relaxed);
        let failed = competing_package("function() error(\"first candidate failed\") end");
        let registry = competing_registry(&supervisor, failed, &["graph-zeta", "graph-alpha"]);
        let error = registry
            .preview_styled_for(
                &path,
                PreviewStyle::default(),
                Some(&location),
                &Arc::new(AtomicBool::new(false)),
            )
            .expect_err("failed first candidate stops fallback");
        assert!(error.contains("first candidate failed"), "{error}");
        assert_eq!(launches.load(Ordering::Relaxed), 1);
        supervisor.shutdown().expect("test supervisor shuts down");
    }
}
