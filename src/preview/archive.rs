use std::fs::File;
use std::io::{self, BufReader, Read};

use super::{ArchiveItem, Content, Previewer, Probe};

/// Items listed.
const MAX_ITEMS: usize = 2000;

/// Lists the contents of zip (incl. jar, whl, …), tar, tar.gz / tgz archives.
pub struct ArchivePreviewer;

fn kind(probe: &Probe) -> Option<&'static str> {
    let name = probe.name.to_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") || name.ends_with(".crate") {
        Some("tar.gz")
    } else if name.ends_with(".tar") {
        Some("tar")
    } else if probe.head.starts_with(b"PK\x03\x04") || probe.head.starts_with(b"PK\x05\x06") {
        Some("zip") // also .jar, .whl, .docx, .epub, …
    } else {
        None
    }
}

impl Previewer for ArchivePreviewer {
    fn name(&self) -> &'static str {
        "Archive"
    }

    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir && kind(probe).is_some()
    }

    fn load(&self, probe: &Probe) -> io::Result<Content> {
        let file = BufReader::new(File::open(&probe.path)?);
        let (items, total) = match kind(probe) {
            Some("zip") => list_zip(file)?,
            Some("tar") => list_tar(file)?,
            _ => list_tar(flate2::read::GzDecoder::new(file))?,
        };
        Ok(Content::Archive {
            truncated: total > items.len(),
            items,
            total,
        })
    }
}

fn list_zip(file: BufReader<File>) -> io::Result<(Vec<ArchiveItem>, usize)> {
    let mut archive = zip::ZipArchive::new(file).map_err(io::Error::other)?;
    let total = archive.len();
    let mut items = Vec::new();
    for i in 0..total.min(MAX_ITEMS) {
        let entry = archive.by_index_raw(i).map_err(io::Error::other)?;
        items.push(ArchiveItem {
            name: entry.name().to_string(),
            size: entry.size(),
            is_dir: entry.is_dir(),
        });
    }
    Ok((items, total))
}

fn list_tar(reader: impl Read) -> io::Result<(Vec<ArchiveItem>, usize)> {
    let mut archive = tar::Archive::new(reader);
    let mut items = Vec::new();
    let mut total = 0;
    for entry in archive.entries()? {
        let entry = entry?;
        total += 1;
        if items.len() < MAX_ITEMS {
            items.push(ArchiveItem {
                name: entry.path()?.display().to_string(),
                size: entry.header().size().unwrap_or(0),
                is_dir: entry.header().entry_type().is_dir(),
            });
        }
    }
    Ok((items, total))
}
