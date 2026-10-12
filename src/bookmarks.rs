//! Bookmarks, UI-independent: adding, removing and toggling bookmarked local
//! and remote folders in memory and in the settings file. Entries are stored
//! as versioned typed locations ([`crate::location_codec`]); legacy path
//! strings are still read. A bookmark of a remote that is no longer
//! configured stays in the list and fails when opened, like a typed address.

use std::path::{Path, PathBuf};

use crate::location::Location;

/// Whether `dir` is in the bookmark list.
pub fn contains(bookmarks: &[Location], dir: &Location) -> bool {
    bookmarks.iter().any(|stored| stored == dir)
}

/// Adds `dir` unless present, otherwise removes it. Returns whether it is
/// bookmarked afterwards.
pub fn toggle(bookmarks: &mut Vec<Location>, dir: &Location) -> bool {
    if contains(bookmarks, dir) {
        bookmarks.retain(|stored| stored != dir);
        false
    } else {
        bookmarks.push(dir.clone());
        true
    }
}

/// Adds or removes `dir` in the settings file, per `bookmarked`.
pub fn set_bookmark(dir: &Location, bookmarked: bool) -> Result<PathBuf, String> {
    edit_settings(dir, if bookmarked { Edit::Add } else { Edit::Remove })
}

#[derive(Clone, Copy)]
enum Edit {
    Add,
    Remove,
}

fn edit_settings(dir: &Location, edit: Edit) -> Result<PathBuf, String> {
    let path =
        crate::settings::persistence::path().ok_or("No config directory on this platform")?;
    edit_file(&path, dir, edit)?;
    Ok(path)
}

#[cfg(test)]
pub fn add_bookmark_to(path: &Path, dir: &Location) -> Result<(), String> {
    edit_file(path, dir, Edit::Add)
}

#[cfg(test)]
pub fn remove_bookmark_from(path: &Path, dir: &Location) -> Result<(), String> {
    edit_file(path, dir, Edit::Remove)
}

fn edit_file(path: &Path, location: &Location, edit: Edit) -> Result<(), String> {
    let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(error(&e)),
    };
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| error(&e))?;
    let name = location.panel_path();
    let value = crate::location_codec::encode(location)
        .map_err(|error| format!("{}: {error}", name.display()))?;
    match edit {
        Edit::Add => {
            let list = doc
                .entry("bookmarks")
                .or_insert_with(|| toml_edit::value(toml_edit::Array::new()))
                .as_array_mut()
                .ok_or_else(|| error(&"`bookmarks` is not a list"))?;
            if !list.iter().any(|item| matches(item, location)) {
                list.push(value);
            }
        }
        Edit::Remove => match doc.get_mut("bookmarks") {
            None => return Ok(()),
            Some(item) => {
                let list = item
                    .as_array_mut()
                    .ok_or_else(|| error(&"`bookmarks` is not a list"))?;
                list.retain(|item| !matches(item, location));
            }
        },
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| error(&e))?;
    }
    std::fs::write(path, doc.to_string()).map_err(|e| error(&e))
}

/// Whether a stored list entry (versioned location or legacy path string)
/// names `location`.
fn matches(item: &toml_edit::Value, location: &Location) -> bool {
    let Some(stored) = item.as_str() else {
        return false;
    };
    if stored.starts_with(crate::location_codec::PREFIX) {
        crate::location_codec::decode(stored).is_ok_and(|stored| stored == *location)
    } else {
        location.as_local_path() == Some(Path::new(stored))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{add_bookmark_to, contains, remove_bookmark_from, toggle};
    use crate::location::Location;
    use crate::settings::Settings;

    fn local(path: &str) -> Location {
        #[cfg(unix)]
        let path = PathBuf::from(path);
        #[cfg(windows)]
        let path = PathBuf::from(format!("C:{}", path.replace('/', "\\")));
        Location::local(path).unwrap()
    }

    fn remote(path: &str) -> Location {
        Location::from_panel_path(std::path::Path::new(path), None).unwrap()
    }

    #[test]
    fn toggle_adds_then_removes_in_memory() {
        let mut list = vec![local("/a")];
        assert!(toggle(&mut list, &local("/b")));
        assert!(contains(&list, &local("/b")));
        assert!(!toggle(&mut list, &local("/b")));
        assert_eq!(list, [local("/a")]);
        assert!(!toggle(&mut list, &local("/a")));
        assert!(list.is_empty());
    }

    #[test]
    fn remote_locations_toggle_beside_local_ones() {
        let mut list = vec![local("/a")];
        assert!(toggle(&mut list, &remote("proton:/a/b")));
        assert!(contains(&list, &remote("proton:/a/b")));
        assert!(!contains(&list, &remote("proton:/a")));
        assert!(!contains(&list, &remote("other:/a/b")));
        assert!(toggle(&mut list, &remote("proton:/")));
        assert!(!toggle(&mut list, &remote("proton:/a/b")));
        assert_eq!(list, [local("/a"), remote("proton:/")]);
    }

    #[cfg(unix)]
    #[test]
    fn removing_from_the_file_keeps_comments_and_other_entries() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# keep\ntheme = \"x\"\nbookmarks = [\"/legacy\"]\n").unwrap();
        add_bookmark_to(&path, &local("/srv/a")).unwrap();
        add_bookmark_to(&path, &local("/srv/b")).unwrap();
        remove_bookmark_from(&path, &local("/srv/a")).unwrap();
        remove_bookmark_from(&path, &local("/legacy")).unwrap();
        remove_bookmark_from(&path, &local("/never")).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# keep") && text.contains("theme"));
        assert_eq!(Settings::parse(&text).unwrap().bookmarks, [local("/srv/b")]);
    }

    #[test]
    fn mixed_local_and_remote_bookmarks_round_trip_in_the_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# keep\nfont_size = 15.0\n").unwrap();
        add_bookmark_to(&path, &local("/srv/a")).unwrap();
        add_bookmark_to(&path, &remote("proton:/a/b")).unwrap();
        add_bookmark_to(&path, &remote("proton:/a/b")).unwrap();
        add_bookmark_to(&path, &remote("gdrive:/Docs & Fotos")).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# keep") && text.contains("font_size"));
        assert_eq!(
            Settings::parse(&text).unwrap().bookmarks,
            [
                local("/srv/a"),
                remote("proton:/a/b"),
                remote("gdrive:/Docs & Fotos")
            ]
        );
        remove_bookmark_from(&path, &remote("proton:/a/b")).unwrap();
        remove_bookmark_from(&path, &remote("proton:/a/b")).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# keep"));
        assert_eq!(
            Settings::parse(&text).unwrap().bookmarks,
            [local("/srv/a"), remote("gdrive:/Docs & Fotos")]
        );
    }

    #[test]
    fn removing_without_a_file_creates_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        remove_bookmark_from(&path, &local("/srv/a")).unwrap();
        assert!(!path.exists());
    }
}
