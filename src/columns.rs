//! Columns a panel can show (yazi's "linemode"), chosen in settings.
//! Adding an attribute column means a variant here and a cell renderer in
//! `panel.rs`.

use serde::Deserialize;

use crate::fs_ops::SortKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
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
        }
    }

    /// The sort order clicking the column selects, if sortable.
    pub fn sort_key(self) -> Option<SortKey> {
        match self {
            Self::Name => Some(SortKey::Name),
            Self::Ext => Some(SortKey::Ext),
            Self::Size => Some(SortKey::Size),
            Self::Modified => Some(SortKey::Modified),
            Self::Permissions | Self::Owner => None,
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
}
