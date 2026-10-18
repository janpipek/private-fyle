//! Thin adapter for the existing EXIF columns and metadata queue.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// Reads values for the existing EXIF columns, returning blanks on optional read failure.
pub fn exif_fields(path: &Path) -> HashMap<&'static str, String> {
    crate::data_providers::exif::columns()
        .read(path)
        .unwrap_or_default()
}

/// Reads bounded EXIF column values while preserving acquisition and cancellation errors.
pub fn exif_fields_checked(
    path: &Path,
    cancel: &AtomicBool,
) -> std::io::Result<crate::data_providers::exif::CheckedExifColumns> {
    crate::data_providers::exif::columns().read_checked(path, cancel)
}
