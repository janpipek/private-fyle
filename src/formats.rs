//! Shared, bounded readers and models for file formats.

/// Semantic EXIF fields read independently of preview presentation.
pub mod exif;
/// Bounded GPX and FIT route and activity data shared by Preview and Properties.
pub mod geo;
/// Bounded local media probe shared by Properties, Preview, and columns.
pub mod media;
/// Bounded native RPM/Debian package metadata for Properties.
pub mod package;
/// Bounded local Parquet schema and row-group metadata for Properties.
pub mod parquet;
/// Bounded parsing and selection of embedded JPEG previews.
pub mod raw;
