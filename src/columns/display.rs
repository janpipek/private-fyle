//! Shared projections for listing-backed column values.

use crate::entry::Entry;

/// Formats a built-in listing-backed value exactly as its table cell displays it.
pub fn listing_text(
    kind: crate::columns::ColumnKind,
    entry: &Entry,
    measured_size: Option<&str>,
) -> Option<String> {
    use crate::columns::ColumnKind;
    Some(match kind {
        ColumnKind::Name => entry.name.clone(),
        ColumnKind::Ext => entry.ext().to_owned(),
        ColumnKind::Size => {
            if entry.is_dir {
                measured_size.unwrap_or("<DIR>").to_owned()
            } else {
                entry
                    .size
                    .map_or_else(|| "—".into(), crate::fs_ops::format_size)
            }
        }
        ColumnKind::Modified => entry
            .modified
            .map(crate::fs_ops::format_time)
            .unwrap_or_default(),
        ColumnKind::Created => entry
            .attrs
            .created
            .filter(|_| !entry.is_parent)
            .map(crate::fs_ops::format_time)
            .unwrap_or_default(),
        ColumnKind::Permissions => {
            if entry.is_parent {
                String::new()
            } else {
                entry.attrs.permissions(entry.is_dir)
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::listing_text;
    use crate::columns::ColumnKind;
    use crate::entry::Entry;

    #[test]
    fn size_projection_matches_directory_and_unknown_file_labels() {
        let mut entry = Entry {
            name: "sub".into(),
            path: "sub".into(),
            is_dir: true,
            is_parent: false,
            size: None,
            modified: None,
            attrs: Default::default(),
            link: Default::default(),
        };
        assert_eq!(
            listing_text(ColumnKind::Size, &entry, None).as_deref(),
            Some("<DIR>")
        );
        assert_eq!(
            listing_text(ColumnKind::Size, &entry, Some("≥ 1 KB")).as_deref(),
            Some("≥ 1 KB")
        );
        entry.is_dir = false;
        assert_eq!(
            listing_text(ColumnKind::Size, &entry, None).as_deref(),
            Some("—")
        );
    }
}
