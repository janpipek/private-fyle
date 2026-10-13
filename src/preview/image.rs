//! Raster image previews.

use std::io;

use super::{Content, Previewer, Probe};

/// Formats GPUI can decode.
const EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico", "tif", "tiff",
];

pub struct ImagePreviewer;

impl Previewer for ImagePreviewer {
    fn name(&self) -> &'static str {
        "Image"
    }

    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir && EXTENSIONS.contains(&probe.ext.as_str())
    }

    fn load(&self, probe: &Probe) -> io::Result<Content> {
        // Decoding happens in the view, which caches images.
        Ok(Content::Image {
            path: probe.path.clone(),
        })
    }
}
