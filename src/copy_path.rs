//! Addresses copied for the selected entry, independent of clipboard UI.

use crate::location::Location;

/// Returns a useful copied path, asking the caller for standard URLs for remotes.
pub fn selected_path(
    location: &Location,
    remote_url: impl FnOnce(&crate::location::RemoteLocation) -> Option<String>,
) -> Option<String> {
    match location {
        Location::Local(path) => Some(path.as_path().to_str()?.to_owned()),
        Location::Remote(remote) => remote_url(remote),
        Location::Archive { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::selected_path;
    use crate::location::Location;

    #[test]
    fn local_path_is_native_and_archive_has_no_address() {
        let local_path = std::env::current_dir().unwrap().join("a b");
        let expected = local_path.to_str().unwrap().to_owned();
        let local = Location::local(local_path).unwrap();
        assert_eq!(
            selected_path(&local, |_| None).as_deref(),
            Some(expected.as_str())
        );
        let archive = Location::archive(local, "inside.txt").unwrap();
        assert_eq!(selected_path(&archive, |_| None), None);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_local_path_has_no_clipboard_representation() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let path = Location::local(std::path::PathBuf::from(OsString::from_vec(vec![
            b'/', b't', b'm', b'p', b'/', 0xff,
        ])))
        .unwrap();
        assert_eq!(selected_path(&path, |_| None), None);
    }
}
