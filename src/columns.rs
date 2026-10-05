//! Columns a panel can show (yazi's "linemode"), chosen in settings.
//! Adding an attribute column means a variant here and a cell renderer in
//! `panel.rs`.

use serde::Deserialize;

use crate::fs_ops::SortKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnKind {
    Name,
    Ext,
    Size,
    Modified,
    /// `drwxr-xr-x` on Unix, read-only flag elsewhere.
    Permissions,
    /// File owner (Unix).
    Owner,
    /// Creation (birth) time, where the file system records it.
    Created,
    /// Git status (`M`, `A`, `?`, …); a directory shows the most important
    /// state inside it.
    Git,
    /// Names of extended attributes (Linux).
    Xattrs,
    /// SELinux context (Linux).
    Selinux,
    /// Photos: when taken and the camera, from EXIF data.
    Exif,
}

impl ColumnKind {
    pub const DEFAULT: [ColumnKind; 4] = [Self::Name, Self::Ext, Self::Size, Self::Modified];

    pub fn id(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Ext => "ext",
            Self::Size => "size",
            Self::Modified => "modified",
            Self::Permissions => "permissions",
            Self::Owner => "owner",
            Self::Created => "created",
            Self::Git => "git",
            Self::Xattrs => "xattrs",
            Self::Selinux => "selinux",
            Self::Exif => "exif",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Ext => "Ext",
            Self::Size => "Size",
            Self::Modified => "Modified",
            Self::Permissions => "Permissions",
            Self::Owner => "Owner",
            Self::Created => "Created",
            Self::Git => "Git",
            Self::Xattrs => "Xattrs",
            Self::Selinux => "SELinux",
            Self::Exif => "EXIF",
        }
    }

    pub fn width(self) -> f32 {
        match self {
            Self::Name => 280.,
            Self::Ext => 60.,
            Self::Size => 90.,
            Self::Modified => 140.,
            Self::Permissions => 100.,
            Self::Owner => 90.,
            Self::Created => 140.,
            Self::Git => 40.,
            Self::Xattrs => 160.,
            Self::Selinux => 220.,
            Self::Exif => 220.,
        }
    }

    /// The sort order clicking the column selects, if sortable.
    pub fn sort_key(self) -> Option<SortKey> {
        match self {
            Self::Name => Some(SortKey::Name),
            Self::Ext => Some(SortKey::Ext),
            Self::Size => Some(SortKey::Size),
            Self::Modified => Some(SortKey::Modified),
            Self::Permissions
            | Self::Owner
            | Self::Created
            | Self::Git
            | Self::Xattrs
            | Self::Selinux
            | Self::Exif => None,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_first_without_duplicates() {
        use ColumnKind::*;
        assert_eq!(normalize(&[Size, Name, Owner, Size]), [Name, Size, Owner]);
        assert_eq!(normalize(&[]), [Name]);
    }

    #[test]
    fn parses_setting_names() {
        #[derive(Deserialize)]
        struct S {
            columns: Vec<ColumnKind>,
        }
        let s: S =
            toml::from_str(r#"columns = ["created", "git", "xattrs", "selinux", "exif"]"#).unwrap();
        use ColumnKind::*;
        assert_eq!(s.columns, [Created, Git, Xattrs, Selinux, Exif]);
        for c in s.columns {
            assert_eq!(format!("{c:?}").to_lowercase(), c.id());
        }
    }
}
