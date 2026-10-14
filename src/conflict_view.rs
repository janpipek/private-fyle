//! Presentation model for copy/move conflicts.

use crate::fs_ops::{format_size, format_time_with_seconds};
use crate::transfer::{EntryInfo, EntryKind};

pub fn details(info: &EntryInfo) -> Vec<(&'static str, String)> {
    vec![
        ("Path", info.path.display().to_string()),
        ("Type", kind_label(info.kind).to_string()),
        (
            "Size",
            info.size.map(format_size).unwrap_or_else(|| "—".into()),
        ),
        (
            "Modified",
            info.modified
                .map(format_time_with_seconds)
                .unwrap_or_else(|| "—".into()),
        ),
    ]
}

pub fn kind_label(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::File => "File",
        EntryKind::Directory => "Directory",
        EntryKind::Symlink => "Symbolic link",
        EntryKind::Other => "Other",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{details, kind_label};
    use crate::transfer::{EntryInfo, EntryKind};

    #[test]
    fn details_distinguish_type_and_unknown_metadata() {
        let info = EntryInfo {
            path: PathBuf::from("/tmp/example"),
            kind: EntryKind::Symlink,
            size: None,
            modified: None,
        };
        let rows = details(&info);
        assert_eq!(kind_label(info.kind), "Symbolic link");
        assert_eq!(rows[0], ("Path", "/tmp/example".into()));
        assert_eq!(rows[2], ("Size", "—".into()));
        assert_eq!(rows[3], ("Modified", "—".into()));
    }
}
