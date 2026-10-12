//! Shared, bounded readers and models for file formats.

/// Semantic EXIF fields read independently of preview presentation.
pub mod exif;
/// Bounded parsing and selection of embedded JPEG previews.
pub mod raw;
