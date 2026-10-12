//! Blocking reads of optional local metadata for panel rows.

use crate::fs_ops;

use super::exif;
use super::model::{Field, Request, Values};

/// Reads only requested fields; missing optional metadata keeps its existing empty value.
pub fn read_metadata(request: &Request) -> Values {
    let path = request.path();
    let mut values = Values::default();
    if request.fields.contains(&Field::Xattrs) {
        values.xattrs = Some(crate::xattr::names(path).join(", "));
    }
    if request.fields.contains(&Field::Selinux) {
        values.selinux = Some(crate::xattr::selinux_context(path).unwrap_or_default());
    }
    if request.fields.contains(&Field::Exif) {
        values.exif = Some(exif::exif_fields(path));
    }
    if request.fields.contains(&Field::Owner) {
        values.owner = Some(fs_ops::owner_name(request.uid));
    }
    if request.fields.contains(&Field::ResolvedLink) {
        values.resolved_link = Some(request.link.resolved(path));
    }
    values
}

#[cfg(test)]
mod tests {
    use super::read_metadata;
    use crate::links::LinkState;
    use crate::location::Location;
    use crate::panel::metadata::model::{Field, MetadataQueue};
    use tempfile::TempDir;

    #[test]
    fn reader_leaves_unrequested_fields_absent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("plain.txt");
        std::fs::write(&path, "plain text").unwrap();
        let mut queue = MetadataQueue::default();
        queue.request(
            &Location::local(path).unwrap(),
            1,
            &[Field::Exif],
            None,
            LinkState::NotLink,
        );
        let values = read_metadata(&queue.take_next().unwrap());
        assert!(values.exif.unwrap().is_empty());
        assert!(values.xattrs.is_none());
        assert!(values.selinux.is_none());
        assert!(values.owner.is_none());
        assert!(values.resolved_link.is_none());
    }

    #[test]
    fn missing_optional_metadata_returns_completed_empty_values() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("missing.jpg");
        let mut queue = MetadataQueue::default();
        queue.request(
            &Location::local(path).unwrap(),
            1,
            &[
                Field::Xattrs,
                Field::Selinux,
                Field::Exif,
                Field::ResolvedLink,
            ],
            None,
            LinkState::Link {
                target: "missing".into(),
            },
        );
        let values = read_metadata(&queue.take_next().unwrap());
        assert_eq!(values.xattrs.as_deref(), Some(""));
        assert_eq!(values.selinux.as_deref(), Some(""));
        assert!(values.exif.unwrap().is_empty());
        assert_eq!(values.resolved_link, Some(None));
        assert!(values.owner.is_none());
    }
}
