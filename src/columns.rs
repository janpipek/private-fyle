//! Columns a panel can show (yazi's "linemode"): a catalog grouped by where
//! the data comes from (file system, Git, EXIF, …). Users pick columns in
//! the Columns dialog or as `columns` in settings, by id (`"size"`,
//! `"exif.camera"`). Adding a column means a variant here (and in
//! [`ColumnKind::ALL`]) and a cell renderer in `panel.rs`.

use std::fmt;

use serde::Deserialize;

use crate::entry::SortKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnKind {
    Name,
    Ext,
    Size,
    Modified,
    /// Creation (birth) time, where the file system records it.
    Created,
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
    Unix,
    Git,
    Attributes,
    Exif,
}

impl fmt::Display for Group {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::File => "File",
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
    pub const ALL: [ColumnKind; 19] = [
        Self::Name,
        Self::Ext,
        Self::Size,
        Self::Modified,
        Self::Created,
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

/// The configured columns with Name guaranteed first and no duplicates.
pub fn normalize(columns: &[ColumnKind]) -> Vec<ColumnKind> {
    let mut out = vec![ColumnKind::Name];
    for &c in columns {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// Shows or hides `column`: a newly shown column goes last; Name stays.
pub fn toggle(columns: &[ColumnKind], column: ColumnKind, show: bool) -> Vec<ColumnKind> {
    let mut out: Vec<_> = columns.iter().copied().filter(|&c| c != column).collect();
    if show {
        out.push(column);
    }
    normalize(&out)
}

/// The catalog grouped for the Columns dialog, groups in catalog order.
pub fn catalog() -> Vec<(Group, Vec<ColumnKind>)> {
    let mut groups: Vec<(Group, Vec<ColumnKind>)> = Vec::new();
    for column in ColumnKind::ALL {
        match groups.last_mut() {
            Some((group, columns)) if *group == column.group() => columns.push(column),
            _ => groups.push((column.group(), vec![column])),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use ColumnKind::*;

    #[test]
    fn name_first_without_duplicates() {
        assert_eq!(normalize(&[Size, Name, Owner, Size]), [Name, Size, Owner]);
        assert_eq!(normalize(&[]), [Name]);
    }

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
                Size,
                GitStatus,
                Exif(ExifField::Camera),
                Exif(ExifField::Iso)
            ]
        );
        let err = toml::from_str::<S>(r#"columns = ["exif"]"#).err().unwrap();
        assert!(err.to_string().contains("unknown column \"exif\""), "{err}");
    }

    #[test]
    fn toggles_keep_order_and_name() {
        let cols = [Name, Size, Modified];
        assert_eq!(
            toggle(&cols, Exif(ExifField::Iso), true),
            [Name, Size, Modified, Exif(ExifField::Iso)]
        );
        assert_eq!(toggle(&cols, Size, false), [Name, Modified]);
        assert_eq!(toggle(&cols, Name, false), [Name, Size, Modified]);
        assert_eq!(toggle(&cols, Size, true), [Name, Modified, Size]);
    }

    #[test]
    fn catalog_groups_every_column_once() {
        let groups = catalog();
        let names: Vec<_> = groups.iter().map(|(g, _)| *g).collect();
        assert_eq!(
            names,
            [
                Group::File,
                Group::Unix,
                Group::Git,
                Group::Attributes,
                Group::Exif
            ]
        );
        let total: usize = groups.iter().map(|(_, c)| c.len()).sum();
        assert_eq!(total, ColumnKind::ALL.len());
        assert_eq!(groups[4].1.len(), 9);
    }
}
