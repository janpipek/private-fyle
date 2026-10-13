//! Shared, bounded readers and models for file formats.

/// Semantic EXIF fields read independently of preview presentation.
pub mod exif;
/// Bounded local Parquet schema and row-group metadata for Properties.
pub mod parquet;
/// Bounded parsing and selection of embedded JPEG previews.
pub mod raw;
