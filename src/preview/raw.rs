//! F3 previewer for embedded JPEG previews in camera RAW files.

use std::io;

use super::{Content, Previewer, Probe};

/// Shows a camera file's embedded JPEG preview when extraction succeeds.
pub struct RawPreviewer;

impl Previewer for RawPreviewer {
    fn name(&self) -> &'static str {
        "RAW image"
    }

    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir && crate::formats::raw::supported(&probe.path)
    }

    fn load(&self, probe: &Probe) -> io::Result<Content> {
        let path = crate::raw_preview::extract(&probe.path)?;
        Ok(Content::Image { path })
    }
}
