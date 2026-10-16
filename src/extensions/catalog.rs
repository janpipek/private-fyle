//! Builds the immutable application column catalog from explicitly enabled packages.

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

/// Prepared catalog, shared supervisor and startup diagnostics for the application.
pub struct PreparedCatalog {
    /// Built-in columns and columns from successfully registered enabled packages.
    pub catalog: Catalog,
    /// Shared single-worker supervisor, retained by the application until shutdown.
    pub supervisor: Option<Supervisor>,
    /// Package-specific startup errors for presentation at the UI boundary.
    pub diagnostics: Vec<StartupDiagnostic>,
}

/// Loads only explicitly enabled packages and installs their declared column readers.
pub fn prepare(config_directory: Option<&Path>, enabled: &[PackageId]) -> PreparedCatalog {
    if enabled.is_empty() {
        return PreparedCatalog {
            catalog: Catalog::with_defaults(),
            supervisor: None,
            diagnostics: Vec::new(),
        };
    }

    let Some(config_directory) = config_directory else {
        return PreparedCatalog {
            catalog: Catalog::with_defaults(),
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
        package
            .manifest
            .providers
            .iter()
            .any(|provider| !provider.columns.is_empty())
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
    let mut diagnostics = Vec::new();
    for package in packages {
        if package
            .manifest
            .providers
            .iter()
            .any(|provider| !provider.columns.is_empty())
            && supervisor.is_none()
        {
            diagnostics.push(StartupDiagnostic::Operation {
                path: package_root.join(package.manifest.id.as_str()),
                error: io::Error::new(
                    io::ErrorKind::NotConnected,
                    "extension columns are unavailable because the worker could not start",
                ),
            });
            continue;
        }
        let package = Arc::new(package);
        for provider in &package.manifest.providers {
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
    PreparedCatalog {
        catalog,
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
    use std::path::Path;

    use super::{StartupDiagnostic, prepare};
    use crate::extensions::PackageId;
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
}
