//! Persisted column IDs and their shared catalog connect built-in table
//! implementations with validated provider registrations. Built-in behavior
//! remains represented by [`ColumnKind`] and specialized table rendering.

use std::fmt;

use serde::Deserialize;

use crate::entry::SortKey;

pub mod catalog;
/// Display-only projection for prepared provider cells.
pub mod cell;
pub mod provider;

/// Immutable catalog and descriptor types shared by panel consumers.
pub use catalog::Catalog;
/// Native provider reader request and callback types.
pub use provider::{NativeColumnReader, ProviderColumnRequest};

/// Persisted column identity, retaining IDs unknown to this build.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ColumnId(String);

impl ColumnId {
    /// Creates an opaque ID, canonicalizing the historical Git alias.
    pub fn new(value: impl Into<String>) -> std::io::Result<Self> {
        let value = value.into();
        if value.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "column ID cannot be empty",
            ));
        }
        Ok(Self(if value == "git" {
            "git.status".into()
        } else {
            value
        }))
    }

    /// Returns the canonical persisted spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns its compiled implementation, if one is available.
    pub fn builtin(&self) -> Option<ColumnKind> {
        ColumnKind::from_id(&self.0)
    }
}

impl From<ColumnKind> for ColumnId {
    fn from(kind: ColumnKind) -> Self {
        Self(kind.id().into())
    }
}

impl<'de> Deserialize<'de> for ColumnId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnKind {
    Name,
    Ext,
    Size,
    Modified,
    /// Creation (birth) time, where the file system records it.
    Created,
    /// Length of a local audio or video file, when media probing succeeds.
    Duration,
    /// `drwxr-xr-x` on Unix, read-only flag elsewhere.
    Permissions,
    /// File owner (Unix).
    Owner,
    /// Git status (`M`, `A`, `?`, …); a directory shows the most important
    /// state inside it.
    GitStatus,
    /// Names of extended attributes (Linux).
    Xattrs,
    /// SELinux context (Linux).
    Selinux,
    /// A field of a photo's EXIF data.
    Exif(ExifField),
}

/// EXIF fields offered as columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExifField {
    Taken,
    Camera,
    Lens,
    Exposure,
    Aperture,
    Iso,
    FocalLength,
    Dimensions,
    Gps,
}

impl ExifField {
    /// Stable metadata key and column id suffix.
    pub fn key(self) -> &'static str {
        match self {
            Self::Taken => "taken",
            Self::Camera => "camera",
            Self::Lens => "lens",
            Self::Exposure => "exposure",
            Self::Aperture => "aperture",
            Self::Iso => "iso",
            Self::FocalLength => "focal_length",
            Self::Dimensions => "dimensions",
            Self::Gps => "gps",
        }
    }
}

/// Where a column's data comes from; the Columns dialog groups by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    File,
    Media,
    Unix,
    Git,
    Attributes,
    Exif,
}

impl fmt::Display for Group {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::File => "File",
            Self::Media => "Media",
            Self::Unix => "Owner and permissions",
            Self::Git => "Git",
            Self::Attributes => "Extended attributes",
            Self::Exif => "EXIF (photos)",
        })
    }
}

impl ColumnKind {
    pub const DEFAULT: [ColumnKind; 4] = [Self::Name, Self::Ext, Self::Size, Self::Modified];

    /// Every column, in catalog order (grouped).
    pub const ALL: [ColumnKind; 20] = [
        Self::Name,
        Self::Ext,
        Self::Size,
        Self::Modified,
        Self::Created,
        Self::Duration,
        Self::Permissions,
        Self::Owner,
        Self::GitStatus,
        Self::Xattrs,
        Self::Selinux,
        Self::Exif(ExifField::Taken),
        Self::Exif(ExifField::Camera),
        Self::Exif(ExifField::Lens),
        Self::Exif(ExifField::Exposure),
        Self::Exif(ExifField::Aperture),
        Self::Exif(ExifField::Iso),
        Self::Exif(ExifField::FocalLength),
        Self::Exif(ExifField::Dimensions),
        Self::Exif(ExifField::Gps),
    ];

    /// Id in settings, e.g. `"size"`, `"git.status"`, `"exif.camera"`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Ext => "ext",
            Self::Size => "size",
            Self::Modified => "modified",
            Self::Created => "created",
            Self::Duration => "duration",
            Self::Permissions => "permissions",
            Self::Owner => "owner",
            Self::GitStatus => "git.status",
            Self::Xattrs => "xattrs",
            Self::Selinux => "selinux",
            Self::Exif(field) => match field {
                ExifField::Taken => "exif.taken",
                ExifField::Camera => "exif.camera",
                ExifField::Lens => "exif.lens",
                ExifField::Exposure => "exif.exposure",
                ExifField::Aperture => "exif.aperture",
                ExifField::Iso => "exif.iso",
                ExifField::FocalLength => "exif.focal_length",
                ExifField::Dimensions => "exif.dimensions",
                ExifField::Gps => "exif.gps",
            },
        }
    }

    /// Looks a column up by id; `"git"` is accepted for `"git.status"`.
    pub fn from_id(id: &str) -> Option<Self> {
        if id == "git" {
            return Some(Self::GitStatus);
        }
        Self::ALL.into_iter().find(|c| c.id() == id)
    }

    pub fn group(self) -> Group {
        match self {
            Self::Name | Self::Ext | Self::Size | Self::Modified | Self::Created => Group::File,
            Self::Duration => Group::Media,
            Self::Permissions | Self::Owner => Group::Unix,
            Self::GitStatus => Group::Git,
            Self::Xattrs | Self::Selinux => Group::Attributes,
            Self::Exif(_) => Group::Exif,
        }
    }

    /// Column header.
    pub fn title(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Ext => "Ext",
            Self::Size => "Size",
            Self::Modified => "Modified",
            Self::Created => "Created",
            Self::Duration => "Duration",
            Self::Permissions => "Permissions",
            Self::Owner => "Owner",
            Self::GitStatus => "Git",
            Self::Xattrs => "Xattrs",
            Self::Selinux => "SELinux",
            Self::Exif(field) => match field {
                ExifField::Taken => "Taken",
                ExifField::Camera => "Camera",
                ExifField::Lens => "Lens",
                ExifField::Exposure => "Exposure",
                ExifField::Aperture => "Aperture",
                ExifField::Iso => "ISO",
                ExifField::FocalLength => "Focal length",
                ExifField::Dimensions => "Dimensions",
                ExifField::Gps => "GPS",
            },
        }
    }

    /// Label in the Columns dialog, where the header alone may be unclear.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ext => "Extension",
            Self::GitStatus => "Status",
            Self::Xattrs => "Attribute names",
            Self::Selinux => "SELinux context",
            Self::Exif(ExifField::Taken) => "Date taken",
            Self::Exif(ExifField::Dimensions) => "Width × height",
            Self::Exif(ExifField::Gps) => "GPS position",
            other => other.title(),
        }
    }

    pub fn width(self) -> f32 {
        match self {
            Self::Name => 280.,
            Self::Ext => 60.,
            Self::Size => 90.,
            Self::Modified | Self::Created => 140.,
            Self::Duration => 80.,
            Self::Permissions => 100.,
            Self::Owner => 90.,
            Self::GitStatus => 40.,
            Self::Xattrs => 160.,
            Self::Selinux => 220.,
            Self::Exif(field) => match field {
                ExifField::Taken => 140.,
                ExifField::Camera | ExifField::Lens | ExifField::Gps => 160.,
                ExifField::Dimensions => 100.,
                _ => 80.,
            },
        }
    }

    /// The sort order clicking the column selects, if sortable.
    pub fn sort_key(self) -> Option<SortKey> {
        match self {
            Self::Name => Some(SortKey::Name),
            Self::Ext => Some(SortKey::Ext),
            Self::Size => Some(SortKey::Size),
            Self::Modified => Some(SortKey::Modified),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for ColumnKind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let id = String::deserialize(deserializer)?;
        Self::from_id(&id).ok_or_else(|| {
            let known: Vec<_> = Self::ALL.iter().map(|c| c.id()).collect();
            serde::de::Error::custom(format!(
                "unknown column {id:?}; known: {}",
                known.join(", ")
            ))
        })
    }
}

/// Normalizes persisted IDs, retaining unknown columns and Name first.
pub fn normalize(columns: &[ColumnId]) -> Vec<ColumnId> {
    let name = ColumnId::from(ColumnKind::Name);
    let mut result = vec![name.clone()];
    for column in columns {
        if column != &name && !result.contains(column) {
            result.push(column.clone());
        }
    }
    result
}

/// Shows or hides an ID while preserving all other saved IDs.
pub fn toggle(columns: &[ColumnId], column: &ColumnId, show: bool) -> Vec<ColumnId> {
    let mut result: Vec<_> = columns
        .iter()
        .filter(|value| *value != column)
        .cloned()
        .collect();
    if show {
        result.push(column.clone());
    }
    normalize(&result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_round_trip_and_parse_from_settings() {
        for column in ColumnKind::ALL {
            assert_eq!(ColumnKind::from_id(column.id()), Some(column), "{column:?}");
        }
        #[derive(Deserialize)]
        struct S {
            columns: Vec<ColumnKind>,
        }
        let s: S =
            toml::from_str(r#"columns = ["size", "git", "exif.camera", "exif.iso"]"#).unwrap();
        assert_eq!(
            s.columns,
            [
                ColumnKind::Size,
                ColumnKind::GitStatus,
                ColumnKind::Exif(ExifField::Camera),
                ColumnKind::Exif(ExifField::Iso)
            ]
        );
        let err = toml::from_str::<S>(r#"columns = ["exif"]"#).err().unwrap();
        assert!(err.to_string().contains("unknown column \"exif\""), "{err}");
    }

    #[test]
    fn persisted_ids_keep_unknown_values_and_canonicalize_git_alias() {
        #[derive(Deserialize)]
        struct SavedColumns {
            columns: Vec<ColumnId>,
        }
        let parsed = toml::from_str::<SavedColumns>(
            r#"columns = ["git", "extension.sensor.reader.distance", "future.col"]"#,
        )
        .unwrap()
        .columns;
        assert_eq!(parsed[0].as_str(), "git.status");
        assert_eq!(parsed[1].as_str(), "extension.sensor.reader.distance");
        assert_eq!(parsed[2].as_str(), "future.col");
        assert_eq!(parsed[1].builtin(), None);
        let with_duplicate_name = [
            parsed[0].clone(),
            ColumnId::from(ColumnKind::Name),
            parsed[1].clone(),
            parsed[1].clone(),
        ];
        assert_eq!(
            normalize(&with_duplicate_name),
            [
                ColumnId::from(ColumnKind::Name),
                parsed[0].clone(),
                parsed[1].clone()
            ]
        );
        assert_eq!(
            normalize(&parsed),
            [
                ColumnId::from(ColumnKind::Name),
                parsed[0].clone(),
                parsed[1].clone(),
                parsed[2].clone()
            ]
        );
        let toggled = toggle(&normalize(&parsed), &parsed[1], false);
        assert_eq!(
            toggled,
            [
                ColumnId::from(ColumnKind::Name),
                parsed[0].clone(),
                parsed[2].clone()
            ]
        );
        assert!(ColumnId::new("").is_err());
    }
}
