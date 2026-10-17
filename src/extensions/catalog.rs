//! Prepares shared columns, Properties, and preview registries from enabled packages.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::Arc;

use crate::columns::{Catalog, NativeColumnReader, ProviderColumnRequest};
use crate::data_providers::{RegistrationOrigin, ResultState, Value};

use super::{ColumnWork, Package, PackageDiagnostic, PackageId, Supervisor, load_enabled};

/// A startup issue retained with its native path until the UI boundary.
pub enum StartupDiagnostic {
    /// A package loader rejected or could not read one package file.
    Package(PackageDiagnostic),
    /// The worker could not be started or a validated bundle could not register.
    Operation {
        path: std::path::PathBuf,
        error: io::Error,
    },
    /// Explicitly enabled package IDs cannot be loaded without a config directory.
    ConfigDirectoryUnavailable(PackageId),
}

/// Prepared provider registries, shared supervisor, and startup diagnostics.
pub struct PreparedCatalog {
    /// Built-in columns and columns from successfully registered enabled packages.
    pub catalog: Catalog,
    /// Native and extension Properties providers from the same startup snapshot.
    pub properties: crate::properties::Registry,
    /// Supported scene viewers in deterministic package/contribution order.
    pub viewers: Vec<super::preview::Handle>,
    /// Shared single-worker supervisor, retained by the application until shutdown.
    pub supervisor: Option<Supervisor>,
    /// Package-specific startup errors for presentation at the UI boundary.
    pub diagnostics: Vec<StartupDiagnostic>,
}

/// Loads only explicitly enabled packages and installs their declared feature handles.
pub fn prepare(config_directory: Option<&Path>, enabled: &[PackageId]) -> PreparedCatalog {
    if enabled.is_empty() {
        return PreparedCatalog {
            catalog: Catalog::with_defaults(),
            properties: crate::properties::Registry::with_defaults(),
            viewers: Vec::new(),
            supervisor: None,
            diagnostics: Vec::new(),
        };
    }

    let Some(config_directory) = config_directory else {
        return PreparedCatalog {
            catalog: Catalog::with_defaults(),
            properties: crate::properties::Registry::with_defaults(),
            viewers: Vec::new(),
            supervisor: None,
            diagnostics: enabled
                .iter()
                .cloned()
                .map(StartupDiagnostic::ConfigDirectoryUnavailable)
                .collect(),
        };
    };
    let root = config_directory.join("extensions");
    let report = load_enabled(&root, enabled);
    let mut diagnostics = report
        .diagnostics
        .into_iter()
        .map(StartupDiagnostic::Package)
        .collect::<Vec<_>>();
    let needs_worker = report.packages.iter().any(|package| {
        package.manifest.providers.iter().any(|provider| {
            !provider.columns.is_empty()
                || !provider.properties.is_empty()
                || provider
                    .viewers
                    .iter()
                    .any(|viewer| viewer.representation == "fyle.scene" && viewer.version == 1)
        })
    });
    let supervisor = if needs_worker {
        match Supervisor::new() {
            Ok(supervisor) => Some(supervisor),
            Err(error) => {
                diagnostics.push(StartupDiagnostic::Operation {
                    path: root.clone(),
                    error,
                });
                None
            }
        }
    } else {
        None
    };
    let mut prepared = install_packages(report.packages, supervisor, &root);
    diagnostics.append(&mut prepared.diagnostics);
    prepared.diagnostics = diagnostics;
    prepared
}

/// Installs already validated package bundles using one shared supervisor.
pub fn install_packages(
    packages: Vec<Package>,
    supervisor: Option<Supervisor>,
    package_root: &Path,
) -> PreparedCatalog {
    let mut catalog = Catalog::with_defaults();
    let mut properties = crate::properties::Registry::with_defaults();
    let mut viewers = Vec::new();
    let mut diagnostics = Vec::new();
    for package in packages {
        if package.manifest.providers.iter().any(|provider| {
            !provider.columns.is_empty()
                || !provider.properties.is_empty()
                || provider
                    .viewers
                    .iter()
                    .any(|viewer| viewer.representation == "fyle.scene" && viewer.version == 1)
        }) && supervisor.is_none()
        {
            diagnostics.push(StartupDiagnostic::Operation {
                path: package_root.join(package.manifest.id.as_str()),
                error: io::Error::new(
                    io::ErrorKind::NotConnected,
                    "extension contributions are unavailable because the worker could not start",
                ),
            });
            continue;
        }
        let package = Arc::new(package);
        for provider in &package.manifest.providers {
            for declared in &provider.properties {
                if let Some(supervisor) = supervisor.as_ref() {
                    match super::properties::Handle::new(
                        package.clone(),
                        declared.header.id.clone(),
                        supervisor.clone(),
                    ) {
                        Ok(handle) => properties.register_extension(handle),
                        Err(error) => diagnostics.push(StartupDiagnostic::Operation {
                            path: package_root
                                .join(package.manifest.id.as_str())
                                .join("manifest.toml"),
                            error,
                        }),
                    }
                }
            }
            for declared in &provider.viewers {
                let path = package_root
                    .join(package.manifest.id.as_str())
                    .join("manifest.toml");
                if declared.representation != "fyle.scene" || declared.version != 1 {
                    diagnostics.push(StartupDiagnostic::Operation {
                        path,
                        error: io::Error::new(
                            io::ErrorKind::Unsupported,
                            "unsupported viewer representation or version",
                        ),
                    });
                } else if let Some(supervisor) = supervisor.as_ref() {
                    match super::preview::Handle::new(
                        package.clone(),
                        declared.header.id.clone(),
                        supervisor.clone(),
                    ) {
                        Ok(handle) => viewers.push(handle),
                        Err(error) => {
                            diagnostics.push(StartupDiagnostic::Operation { path, error })
                        }
                    }
                }
            }
            let mut readers = HashMap::new();
            for column in &provider.columns {
                let supervisor = supervisor
                    .as_ref()
                    .expect("providers with columns require the shared supervisor")
                    .clone();
                let package = package.clone();
                let contribution = column.header.id.clone();
                let reader: NativeColumnReader =
                    Arc::new(move |request: &ProviderColumnRequest| {
                        extension_reader(
                            &supervisor,
                            package.clone(),
                            contribution.clone(),
                            request,
                        )
                    });
                readers.insert(column.header.id.clone(), reader);
            }
            if let Err(error) =
                catalog.register_provider(RegistrationOrigin::Extension, provider, &readers)
            {
                diagnostics.push(StartupDiagnostic::Operation {
                    path: package_root
                        .join(package.manifest.id.as_str())
                        .join("manifest.toml"),
                    error,
                });
            }
        }
    }
    viewers.sort_by(|left, right| {
        left.package_id()
            .cmp(right.package_id())
            .then_with(|| left.id().as_str().cmp(right.id().as_str()))
    });
    PreparedCatalog {
        catalog,
        properties,
        viewers,
        supervisor,
        diagnostics,
    }
}

fn extension_reader(
    supervisor: &Supervisor,
    package: Arc<Package>,
    contribution: crate::data_providers::ContributionId,
    request: &ProviderColumnRequest,
) -> io::Result<ResultState<Value>> {
    if request.is_dir || request.location.as_local_path().is_none() {
        return Ok(ResultState::Unavailable(
            "extension columns require a local regular file".into(),
        ));
    }
    let work = ColumnWork {
        package,
        contribution,
        source: request.location.clone(),
        name: request.name.clone(),
        is_dir: request.is_dir,
        generation_cancelled: request.cancel.clone(),
    };
    supervisor.submit(work)?.wait()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use std::path::Path;

    use super::{StartupDiagnostic, install_packages, prepare};
    use crate::extensions::{Package, PackageId};
    use tempfile::TempDir;

    #[test]
    fn disabled_extensions_use_builtins_without_scanning_or_starting_worker() {
        let prepared = prepare(None, &[]);
        assert!(prepared.supervisor.is_none());
        assert!(prepared.diagnostics.is_empty());
        assert!(
            prepared
                .catalog
                .descriptor(&crate::columns::ColumnKind::Name.into())
                .is_some()
        );
    }

    #[test]
    fn absent_config_directory_reports_explicitly_enabled_packages() {
        let id = PackageId::new("future-reader").expect("fixture package ID");
        let prepared = prepare(None, std::slice::from_ref(&id));
        assert!(prepared.supervisor.is_none());
        assert!(matches!(
            prepared.diagnostics.as_slice(),
            [StartupDiagnostic::ConfigDirectoryUnavailable(package)] if package == &id
        ));
    }

    #[test]
    fn enabled_missing_package_is_diagnostic_and_keeps_builtin_catalog() {
        let temp = TempDir::new().expect("temporary config directory");
        std::fs::create_dir(temp.path().join("extensions")).expect("create extensions root");
        let id = PackageId::new("missing").expect("fixture package ID");
        let prepared = prepare(Some(Path::new(temp.path())), std::slice::from_ref(&id));
        assert!(prepared.supervisor.is_none());
        assert!(matches!(
            prepared.diagnostics.as_slice(),
            [StartupDiagnostic::Package(_)]
        ));
        assert!(
            prepared
                .catalog
                .descriptor(&crate::columns::ColumnKind::Name.into())
                .is_some()
        );
        assert!(
            prepared
                .catalog
                .descriptor(
                    &crate::columns::ColumnId::new("extension.missing.reader.distance")
                        .expect("column ID")
                )
                .is_none()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn properties_only_and_viewer_only_packages_install_executable_handles() {
        use crate::extensions::Supervisor;
        use crate::extensions::manifest;

        let manifest_text = include_str!("../../examples/extensions/sensor/manifest.toml");
        let source = include_str!("../../examples/extensions/sensor/main.lua");
        let id = PackageId::new("sensor").expect("fixture package ID");
        let make_package = || Package {
            manifest: manifest::parse(&id, manifest_text).expect("valid fixture manifest"),
            source: source.into(),
        };
        let root = TempDir::new().expect("temporary package root");
        let supervisor = Supervisor::fixture_for_tests().expect("fixture supervisor");

        let mut properties_package = make_package();
        properties_package.manifest.providers[0].columns.clear();
        properties_package.manifest.providers[0].viewers.clear();
        let source_path = root.path().join("summary.sens");
        std::fs::write(
            &source_path,
            include_str!("../../examples/extensions/sensor/fixtures/complete.sens"),
        )
        .expect("write source fixture");
        let prepared = install_packages(
            vec![properties_package],
            Some(supervisor.clone()),
            root.path(),
        );
        assert!(prepared.supervisor.is_some());
        assert!(prepared.viewers.is_empty());
        assert!(prepared.diagnostics.is_empty());
        let selection = crate::properties::Selection {
            location: crate::location::Location::local(source_path.clone())
                .expect("absolute local source"),
            entry: crate::entry::Entry {
                name: "summary.sens".into(),
                path: source_path,
                is_dir: false,
                is_parent: false,
                size: None,
                modified: None,
                attrs: Default::default(),
                link: Default::default(),
            },
        };
        let sections = prepared
            .properties
            .read_extensions(&selection, &Arc::new(AtomicBool::new(false)))
            .expect("Properties-only package runs through shared worker");
        assert_eq!(sections.len(), 1);
        assert!(sections[0].is_visible());
        supervisor.shutdown().expect("fixture supervisor shutdown");

        let supervisor = Supervisor::fixture_for_tests().expect("fixture supervisor");
        let mut viewer_package = make_package();
        viewer_package.manifest.providers[0].columns.clear();
        viewer_package.manifest.providers[0].properties.clear();
        let prepared =
            install_packages(vec![viewer_package], Some(supervisor.clone()), root.path());
        assert_eq!(prepared.viewers.len(), 1);
        assert!(
            prepared
                .properties
                .read_extensions(
                    &crate::properties::Selection {
                        location: crate::location::Location::local(
                            root.path().join("missing.sens")
                        )
                        .expect("absolute source"),
                        entry: crate::entry::Entry {
                            name: "missing.sens".into(),
                            path: root.path().join("missing.sens"),
                            is_dir: false,
                            is_parent: false,
                            size: None,
                            modified: None,
                            attrs: Default::default(),
                            link: Default::default(),
                        },
                    },
                    &Arc::new(AtomicBool::new(false)),
                )
                .is_ok()
        );
        supervisor.shutdown().expect("fixture supervisor shutdown");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unsupported_viewer_keeps_valid_sibling_columns_and_properties() {
        use crate::extensions::Supervisor;
        use crate::extensions::manifest;

        let id = PackageId::new("sensor").expect("fixture package ID");
        let mut package = Package {
            manifest: manifest::parse(
                &id,
                include_str!("../../examples/extensions/sensor/manifest.toml"),
            )
            .expect("valid fixture manifest"),
            source: include_str!("../../examples/extensions/sensor/main.lua").into(),
        };
        package.manifest.providers[0].viewers[0].representation = "unsupported.scene".into();
        let supervisor = Supervisor::fixture_for_tests().expect("fixture supervisor");
        let root = TempDir::new().expect("temporary package root");
        let prepared = install_packages(vec![package], Some(supervisor.clone()), root.path());
        assert!(
            prepared
                .catalog
                .descriptor(
                    &crate::columns::ColumnId::new("extension.sensor.reader.distance")
                        .expect("column ID")
                )
                .is_some()
        );
        assert!(prepared.viewers.is_empty());
        assert!(
            matches!(prepared.diagnostics.as_slice(), [StartupDiagnostic::Operation { error, .. }] if error.kind() == std::io::ErrorKind::Unsupported)
        );
        supervisor.shutdown().expect("fixture supervisor shutdown");
    }
}
