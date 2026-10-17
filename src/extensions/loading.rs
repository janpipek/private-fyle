//! Explicit, bounded package discovery and package-local file loading.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::manifest::{self, PackageId};

/// A package whose declarations and unevaluated entry source validated successfully.
#[derive(Debug)]
pub struct Package {
    /// Fully validated declarations.
    pub manifest: manifest::Manifest,
    /// Bounded UTF-8 source; loading never evaluates it.
    pub source: String,
}

/// One package failure, preserving the failing path and I/O error.
#[derive(Debug)]
pub struct PackageDiagnostic {
    /// Requested package identity.
    pub package_id: PackageId,
    /// File or directory operation that failed.
    pub path: PathBuf,
    /// Original filesystem or validation error.
    pub error: io::Error,
}

/// Results of loading the explicitly enabled package IDs.
#[derive(Debug, Default)]
pub struct LoadReport {
    /// Valid packages, sorted by package ID.
    pub packages: Vec<Package>,
    /// Per-package diagnostics in stable requested order.
    pub diagnostics: Vec<PackageDiagnostic>,
}

/// Loads only the listed package IDs from the canonical extensions directory.
pub fn load_enabled(extensions_root: &Path, enabled: &[PackageId]) -> LoadReport {
    if enabled.is_empty() {
        return LoadReport::default();
    }
    let root = fs::canonicalize(extensions_root)
        .and_then(|root| {
            if fs::metadata(&root)?.is_dir() {
                Ok(root)
            } else {
                Err(invalid("extensions root is not a directory"))
            }
        })
        .map_err(Arc::new);
    let mut report = LoadReport::default();
    let mut seen = Vec::<&PackageId>::new();
    for id in enabled {
        if seen.contains(&id) {
            report.diagnostics.push(PackageDiagnostic {
                package_id: id.clone(),
                path: extensions_root.join(id.as_str()),
                error: invalid("duplicate enabled package ID"),
            });
            continue;
        }
        seen.push(id);
        let root = match &root {
            Ok(root) => root,
            Err(error) => {
                report.diagnostics.push(PackageDiagnostic {
                    package_id: id.clone(),
                    path: extensions_root.to_path_buf(),
                    error: io::Error::new(error.kind(), SharedIoError(error.clone())),
                });
                continue;
            }
        };
        match load_one(root, id) {
            Ok(package) => report.packages.push(package),
            Err(error) => report.diagnostics.push(error),
        }
    }
    report
        .packages
        .sort_by(|left, right| left.manifest.id.cmp(&right.manifest.id));
    report
}

fn load_one(root: &Path, id: &PackageId) -> Result<Package, PackageDiagnostic> {
    let package_path = root.join(id.as_str());
    let package_dir =
        fs::canonicalize(&package_path).map_err(|error| diagnostic(id, &package_path, error))?;
    if !package_dir.starts_with(root) || package_dir == root || !package_dir.is_dir() {
        return Err(diagnostic(
            id,
            &package_path,
            invalid("package directory escapes the extensions root"),
        ));
    }
    let manifest_candidate = package_dir.join("manifest.toml");
    let manifest_path = contained_file(id, &package_dir, &manifest_candidate)?;
    let manifest_text = read_bounded_text(&manifest_path, manifest::MANIFEST_BYTES)
        .map_err(|error| diagnostic(id, &manifest_path, error))?;
    let parsed = manifest::parse(id, &manifest_text)
        .map_err(|error| diagnostic(id, &manifest_path, error))?;
    let script_candidate = package_dir.join(Path::new(&parsed.entry));
    let script_path = contained_file(id, &package_dir, &script_candidate)?;
    let source = read_bounded_text(&script_path, manifest::SCRIPT_BYTES)
        .map_err(|error| diagnostic(id, &script_path, error))?;
    Ok(Package {
        manifest: parsed,
        source,
    })
}

fn contained_file(
    id: &PackageId,
    package_dir: &Path,
    candidate: &Path,
) -> Result<PathBuf, PackageDiagnostic> {
    let canonical =
        fs::canonicalize(candidate).map_err(|error| diagnostic(id, candidate, error))?;
    if !canonical.starts_with(package_dir) || canonical == package_dir {
        return Err(diagnostic(
            id,
            candidate,
            invalid("package file escapes its package directory"),
        ));
    }
    Ok(canonical)
}

fn read_bounded_text(path: &Path, limit: usize) -> io::Result<String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    read_text_file(file, limit)
}

fn read_text_file(file: File, limit: usize) -> io::Result<String> {
    if !file.metadata()?.is_file() {
        return Err(invalid("package entry must be a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(invalid("package file exceeds the size limit"));
    }
    String::from_utf8(bytes).map_err(|_| invalid("package file is not valid UTF-8"))
}

fn diagnostic(id: &PackageId, path: &Path, error: io::Error) -> PackageDiagnostic {
    PackageDiagnostic {
        package_id: id.clone(),
        path: path.to_path_buf(),
        error,
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

struct SharedIoError(Arc<io::Error>);

impl std::fmt::Display for SharedIoError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::fmt::Debug for SharedIoError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for SharedIoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::{load_enabled, read_bounded_text};
    use crate::extensions::manifest::{self, PackageId};
    use tempfile::TempDir;

    const MANIFEST: &str = r#"
api_version = 1
id = "sensor"
entry = "main.lua"

[[providers]]
id = "extension.sensor.reader"

[[providers.fields]]
id = "extension.sensor.reader.distance"
kind = "quantity"
unit = "metres"

[[providers.columns]]
id = "extension.sensor.reader.distance-cell"
label = "Distance"
column_id = "extension.sensor.reader.distance"
field = "extension.sensor.reader.distance"
max_bytes = 1024
max_items = 10
"#;

    fn package(root: &std::path::Path, id: &str, manifest: &str, script: &str) {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.toml"), manifest).unwrap();
        std::fs::write(dir.join("main.lua"), script).unwrap();
    }

    fn enabled(id: &str) -> PackageId {
        PackageId::new(id).unwrap()
    }

    #[test]
    fn loads_only_enabled_packages_and_keeps_script_unevaluated() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("extensions");
        package(&root, "sensor", MANIFEST, "error('must not execute')");
        package(&root, "disabled", "this is not toml", "not evaluated");
        let report = load_enabled(&root, &[enabled("sensor")]);
        assert!(report.diagnostics.is_empty());
        assert_eq!(report.packages.len(), 1);
        assert_eq!(report.packages[0].source, "error('must not execute')");
        assert_eq!(report.packages[0].manifest.providers[0].columns.len(), 1);
        assert_eq!(report.packages[0].manifest.id.as_str(), "sensor");
    }

    #[test]
    fn empty_enabled_list_does_not_require_or_scan_root() {
        let temp = TempDir::new().unwrap();
        let report = load_enabled(&temp.path().join("missing"), &[]);
        assert!(report.packages.is_empty());
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn disabled_and_missing_packages_report_without_loading_neighbors() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("extensions");
        package(&root, "disabled", "invalid", "invalid");
        let report = load_enabled(&root, &[enabled("missing")]);
        assert!(report.packages.is_empty());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].package_id.as_str(), "missing");
        assert_eq!(
            report.diagnostics[0].path,
            std::fs::canonicalize(&root).unwrap().join("missing")
        );
        assert_eq!(
            report.diagnostics[0].error.kind(),
            std::io::ErrorKind::NotFound
        );
    }

    #[test]
    fn bad_package_does_not_hide_a_good_package_and_duplicates_load_once() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("extensions");
        package(&root, "sensor", MANIFEST, "ok");
        package(&root, "broken", "api_version = 99", "ok");
        let report = load_enabled(
            &root,
            &[enabled("sensor"), enabled("broken"), enabled("sensor")],
        );
        assert_eq!(report.packages.len(), 1);
        assert_eq!(report.packages[0].manifest.id.as_str(), "sensor");
        assert_eq!(report.diagnostics.len(), 2);
        assert_eq!(report.diagnostics[0].package_id.as_str(), "broken");
        assert_ne!(
            report.diagnostics[0].error.kind(),
            std::io::ErrorKind::NotFound
        );
        assert_eq!(
            report.diagnostics[1].error.to_string(),
            "duplicate enabled package ID"
        );
    }

    #[test]
    fn file_reader_accepts_exact_limit_and_rejects_oversize_and_invalid_utf8() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("text");
        std::fs::write(&path, b"1234").unwrap();
        assert_eq!(read_bounded_text(&path, 4).unwrap(), "1234");
        std::fs::write(&path, b"12345").unwrap();
        assert!(read_bounded_text(&path, 4).is_err());
        std::fs::write(&path, [0xff]).unwrap();
        assert!(read_bounded_text(&path, 4).is_err());
    }

    #[test]
    fn loader_enforces_manifest_and_script_byte_limits_on_files() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("extensions");
        let dir = root.join("sensor");
        std::fs::create_dir_all(&dir).unwrap();
        let exact_manifest = format!(
            "{MANIFEST}{}",
            "#".repeat(manifest::MANIFEST_BYTES - MANIFEST.len())
        );
        std::fs::write(dir.join("manifest.toml"), &exact_manifest).unwrap();
        std::fs::write(dir.join("main.lua"), vec![b'x'; manifest::SCRIPT_BYTES]).unwrap();
        let report = load_enabled(&root, &[enabled("sensor")]);
        assert_eq!(report.packages.len(), 1);
        assert!(report.diagnostics.is_empty());

        std::fs::write(dir.join("main.lua"), vec![b'x'; manifest::SCRIPT_BYTES + 1]).unwrap();
        let report = load_enabled(&root, &[enabled("sensor")]);
        assert!(report.packages.is_empty());
        assert!(
            report.diagnostics[0]
                .error
                .to_string()
                .contains("size limit")
        );

        std::fs::write(dir.join("manifest.toml"), format!("{exact_manifest}#")).unwrap();
        let report = load_enabled(&root, &[enabled("sensor")]);
        assert!(report.packages.is_empty());
        assert!(report.diagnostics[0].path.ends_with("manifest.toml"));
        assert!(
            report.diagnostics[0]
                .error
                .to_string()
                .contains("size limit")
        );
    }

    #[test]
    fn loader_rejects_non_utf8_script_source() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("extensions");
        package(&root, "sensor", MANIFEST, "placeholder");
        std::fs::write(root.join("sensor/main.lua"), [0xff]).unwrap();
        let report = load_enabled(&root, &[enabled("sensor")]);
        assert!(report.packages.is_empty());
        assert!(report.diagnostics[0].error.to_string().contains("UTF-8"));
    }

    #[test]
    fn invalid_manifest_is_reported_before_missing_entry_script() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("extensions");
        let dir = root.join("sensor");
        std::fs::create_dir_all(&dir).unwrap();
        let invalid_manifest = MANIFEST.replace(
            "field = \"extension.sensor.reader.distance\"",
            "field = \"extension.sensor.reader.missing\"",
        );
        std::fs::write(dir.join("manifest.toml"), invalid_manifest).unwrap();
        let report = load_enabled(&root, &[enabled("sensor")]);
        assert_eq!(report.diagnostics.len(), 1);
        assert_ne!(
            report.diagnostics[0].error.kind(),
            std::io::ErrorKind::NotFound
        );
        assert!(report.diagnostics[0].path.ends_with("manifest.toml"));
        assert!(!report.diagnostics[0].error.to_string().contains("main.lua"));
    }

    #[test]
    fn missing_root_preserves_path_and_io_error_kind() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("absent");
        let report = load_enabled(&root, &[enabled("sensor")]);
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].path, root);
        assert_eq!(
            report.diagnostics[0].error.kind(),
            std::io::ErrorKind::NotFound
        );
    }

    #[cfg(unix)]
    #[test]
    fn package_and_entry_symlinks_cannot_escape_root() {
        use std::os::unix::fs::symlink;
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("extensions");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        package(&outside, "sensor", MANIFEST, "ok");
        symlink(outside.join("sensor"), root.join("sensor")).unwrap();
        let report = load_enabled(&root, &[enabled("sensor")]);
        assert!(report.packages.is_empty());
        assert!(report.diagnostics[0].error.to_string().contains("escapes"));

        let package_root = root.join("sensor");
        std::fs::remove_file(&package_root).unwrap();
        package(&root, "sensor", MANIFEST, "ok");
        let external_script = temp.path().join("external.lua");
        std::fs::write(&external_script, "outside").unwrap();
        std::fs::remove_file(package_root.join("main.lua")).unwrap();
        symlink(external_script, package_root.join("main.lua")).unwrap();
        let report = load_enabled(&root, &[enabled("sensor")]);
        assert!(report.packages.is_empty());
        assert!(report.diagnostics[0].error.to_string().contains("escapes"));
    }

    #[cfg(unix)]
    #[test]
    fn manifest_symlink_cannot_escape_package() {
        use std::os::unix::fs::symlink;
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("extensions");
        let dir = root.join("sensor");
        let outside = temp.path().join("external.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&outside, MANIFEST).unwrap();
        symlink(outside, dir.join("manifest.toml")).unwrap();
        let report = load_enabled(&root, &[enabled("sensor")]);
        assert!(report.packages.is_empty());
        assert_eq!(report.diagnostics.len(), 1);
        assert!(report.diagnostics[0].error.to_string().contains("escapes"));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_native_root_path_loads_without_lossy_conversion() {
        use std::os::unix::ffi::OsStringExt;
        let temp = TempDir::new().unwrap();
        let root = temp
            .path()
            .join(std::ffi::OsString::from_vec(b"extensions-\xff".to_vec()));
        package(&root, "sensor", MANIFEST, "native path retained");
        let report = load_enabled(&root, &[enabled("sensor")]);
        assert!(report.diagnostics.is_empty());
        assert_eq!(report.packages[0].source, "native path retained");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn non_regular_fifo_manifest_does_not_block() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt as _;
        use std::os::unix::fs::FileTypeExt as _;
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("extensions");
        let dir = root.join("sensor");
        std::fs::create_dir_all(&dir).unwrap();
        let fifo = dir.join("manifest.toml");
        let fifo_name = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: this NUL-terminated path is contained by the test's TempDir.
        assert_eq!(unsafe { libc::mkfifo(fifo_name.as_ptr(), 0o600) }, 0);
        assert!(
            std::fs::symlink_metadata(&fifo)
                .unwrap()
                .file_type()
                .is_fifo()
        );
        let report = load_enabled(&root, &[enabled("sensor")]);
        assert!(report.packages.is_empty());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].error.to_string(),
            "package entry must be a regular file"
        );
    }
}
