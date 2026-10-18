//! Bounded, cancellable package metadata readers; payloads are not read and scripts are not executed.

mod deb;
mod rpm;

use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Maximum characters retained for any package text value.
pub const TEXT_LIMIT: usize = 4096;
const METADATA_LIMIT: usize = 8 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(2);

/// Package container being inspected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// RPM header metadata.
    Rpm,
    /// Debian binary package control metadata.
    Deb,
}

/// Closed set of text facts selected from package metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// Package identifier.
    Name,
    /// Upstream/package version.
    Version,
    /// RPM distribution release.
    Release,
    /// Target architecture.
    Architecture,
    /// Short package description.
    Summary,
    /// Long package description.
    Description,
    /// Debian maintainer contact.
    Maintainer,
    /// RPM vendor.
    Vendor,
    /// RPM packager contact.
    Packager,
    /// Project homepage.
    Homepage,
    /// Declared license, when included in the metadata.
    License,
    /// Package section/group.
    Section,
}

impl Field {
    /// Stable semantic field identifier used by Properties.
    pub fn id(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Version => "version",
            Self::Release => "release",
            Self::Architecture => "architecture",
            Self::Summary => "summary",
            Self::Description => "description",
            Self::Maintainer => "maintainer",
            Self::Vendor => "vendor",
            Self::Packager => "packager",
            Self::Homepage => "homepage",
            Self::License => "license",
            Self::Section => "section",
        }
    }
}

/// Selected metadata, independent of the UI and of installed-package databases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    /// Source package format.
    pub format: Format,
    /// Bounded selected text fields.
    pub fields: Vec<(Field, String)>,
    /// Declared installed size in bytes, not compressed package size.
    pub installed_size: Option<u64>,
    /// RPM version epoch, when explicitly present.
    pub epoch: Option<u64>,
    /// Whether text was truncated or invalid UTF-8 was replaced.
    pub truncated: bool,
}

impl Metadata {
    fn new(format: Format) -> Self {
        Self {
            format,
            fields: Vec::new(),
            installed_size: None,
            epoch: None,
            truncated: false,
        }
    }

    fn text(&mut self, field: Field, value: &[u8]) {
        let decoded = String::from_utf8_lossy(value);
        self.truncated |= matches!(decoded, std::borrow::Cow::Owned(_));
        let value = decoded.trim();
        if value.is_empty() {
            return;
        }
        let mut chars = value.chars();
        let selected = chars.by_ref().take(TEXT_LIMIT).collect();
        self.truncated |= chars.next().is_some();
        self.fields.push((field, selected));
    }

    fn validate(self) -> io::Result<Self> {
        if ![Field::Name, Field::Version]
            .iter()
            .all(|required| self.fields.iter().any(|(field, _)| field == required))
        {
            return Err(invalid("package name or version is missing"));
        }
        Ok(self)
    }
}

struct Budget<'a> {
    cancel: &'a AtomicBool,
    deadline: Instant,
}

impl<'a> Budget<'a> {
    fn new(cancel: &'a AtomicBool) -> Self {
        Self {
            cancel,
            deadline: Instant::now() + DEADLINE,
        }
    }
    fn check(&self) -> io::Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "package inspection cancelled",
            ));
        }
        if Instant::now() >= self.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "package inspection timed out",
            ));
        }
        Ok(())
    }
    fn bytes(&self, reader: &mut impl Read, count: usize) -> io::Result<Vec<u8>> {
        if count > METADATA_LIMIT {
            return Err(invalid("package metadata exceeds 8 MiB limit"));
        }
        let mut result = vec![0; count];
        for chunk in result.chunks_mut(8192) {
            self.check()?;
            reader.read_exact(chunk)?;
        }
        self.check()?;
        Ok(result)
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Reports the format from a filename hint or distinctive package prefix.
pub fn candidate(path: &Path) -> Option<Format> {
    let name = path.file_name()?.to_string_lossy();
    let recognize = |head| {
        let candidates = crate::recognition::detect(
            &name,
            false,
            head,
            None,
            &crate::recognition::NativeDetector,
        );
        match candidates[0].mime.as_str() {
            "application/x-rpm" => Some(Format::Rpm),
            "application/vnd.debian.binary-package" => Some(Format::Deb),
            _ => None,
        }
    };
    if let Some(format) = recognize(None) {
        return Some(format);
    }
    let mut file = open_regular(path).ok()?;
    let mut head = [0; 68];
    let count = file.read(&mut head).ok()?;
    recognize(Some(&head[..count]))
}

fn open_regular(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid("not a regular package file"));
    }
    Ok(file)
}

/// Reads headers/control metadata of a local regular package with independent bounds.
pub fn read(path: &Path, format: Format, cancel: &AtomicBool) -> io::Result<Metadata> {
    let budget = Budget::new(cancel);
    budget.check()?;
    let mut file = open_regular(path)?;
    let result = match format {
        Format::Rpm => rpm::read(&mut file, &budget),
        Format::Deb => deb::read(&mut file, &budget),
    };
    budget.check()?;
    result
}

#[cfg(test)]
mod tests {
    use super::{Budget, Field, Format, Metadata, TEXT_LIMIT, candidate};
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    #[test]
    fn bounds_text_and_reports_cancellation_and_deadline() {
        let mut metadata = Metadata::new(Format::Rpm);
        metadata.text(Field::Description, "é".repeat(TEXT_LIMIT + 1).as_bytes());
        assert_eq!(metadata.fields[0].1.chars().count(), TEXT_LIMIT);
        assert!(metadata.truncated);
        let cancel = AtomicBool::new(true);
        assert_eq!(
            Budget::new(&cancel).check().unwrap_err().kind(),
            std::io::ErrorKind::Interrupted
        );
        let cancel = AtomicBool::new(false);
        let budget = Budget {
            cancel: &cancel,
            deadline: Instant::now() - Duration::from_secs(1),
        };
        assert_eq!(
            budget.check().unwrap_err().kind(),
            std::io::ErrorKind::TimedOut
        );
    }

    #[test]
    fn extensionless_packages_and_ordinary_ar_archives_are_distinguished() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("unknown");
        std::fs::write(&path, b"\xed\xab\xee\xdb").unwrap();
        assert_eq!(candidate(&path), Some(Format::Rpm));
        let mut bytes = b"!<arch>\n".to_vec();
        bytes.extend_from_slice(b"debian-binary   ");
        bytes.extend([b' '; 44]);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(candidate(&path), Some(Format::Deb));
        bytes[8..24].copy_from_slice(b"library.o/      ");
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(candidate(&path), None);
    }
}
