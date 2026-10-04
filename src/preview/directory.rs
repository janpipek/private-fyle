use std::fs;
use std::io;

use super::{Content, Previewer, Probe};

pub struct DirectoryPreviewer;

impl Previewer for DirectoryPreviewer {
    fn name(&self) -> &'static str {
        "Directory"
    }

    fn matches(&self, probe: &Probe) -> bool {
        probe.is_dir
    }

    /// Counts direct children only, so huge trees stay cheap.
    fn load(&self, probe: &Probe) -> io::Result<Content> {
        let (mut dirs, mut files, mut files_size) = (0, 0, 0);
        for item in fs::read_dir(&probe.path)? {
            let Ok(meta) = item.and_then(|i| fs::metadata(i.path())) else {
                continue;
            };
            if meta.is_dir() {
                dirs += 1;
            } else {
                files += 1;
                files_size += meta.len();
            }
        }
        Ok(Content::Directory {
            dirs,
            files,
            files_size,
        })
    }
}
