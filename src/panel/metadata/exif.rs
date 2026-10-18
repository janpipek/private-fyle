//! Thin adapter for the existing EXIF columns and metadata queue.

use std::collections::HashMap;
use std::path::Path;

/// Reads values for the existing EXIF columns, returning blanks on optional read failure.
pub fn exif_fields(path: &Path) -> HashMap<&'static str, String> {
    crate::data_providers::exif::columns()
        .read(path)
        .unwrap_or_default()
}
