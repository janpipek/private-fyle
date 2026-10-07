//! Bookmarks, UI-independent: which locations can be bookmarked, and adding,
//! removing and toggling them in memory and in the settings file.

use std::path::{Path, PathBuf};

use crate::location::Location;

/// Whether `dir` is in the bookmark list.
pub fn contains(bookmarks: &[PathBuf], dir: &Path) -> bool {
    bookmarks.iter().any(|stored| stored == dir)
}

/// Adds `dir` unless present, otherwise removes it. Returns whether it is
/// bookmarked afterwards.
pub fn toggle(bookmarks: &mut Vec<PathBuf>, dir: &Path) -> bool {
    if contains(bookmarks, dir) {
        bookmarks.retain(|stored| stored != dir);
        false
    } else {
        bookmarks.push(dir.to_path_buf());
        true
    }
}

/// Adds or removes `dir` in the settings file, per `bookmarked`.
pub fn set_bookmark(dir: &Path, bookmarked: bool) -> Result<PathBuf, String> {
    edit_settings(dir, if bookmarked { Edit::Add } else { Edit::Remove })
}

#[derive(Clone, Copy)]
enum Edit {
    Add,
    Remove,
}

fn edit_settings(dir: &Path, edit: Edit) -> Result<PathBuf, String> {
    let path = crate::settings::Settings::path().ok_or("No config directory on this platform")?;
    edit_file(&path, dir, edit)?;
    Ok(path)
}

#[cfg(test)]
pub fn add_bookmark_to(path: &Path, dir: &Path) -> Result<(), String> {
    edit_file(path, dir, Edit::Add)
}

#[cfg(test)]
pub fn remove_bookmark_from(path: &Path, dir: &Path) -> Result<(), String> {
    edit_file(path, dir, Edit::Remove)
}

fn edit_file(path: &Path, dir: &Path, edit: Edit) -> Result<(), String> {
    let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(error(&e)),
    };
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| error(&e))?;
    let location = Location::local(dir.to_path_buf())
        .map_err(|error| format!("{}: {error}", dir.display()))?;
    let value = crate::location_codec::encode(&location)
        .map_err(|error| format!("{}: {error}", dir.display()))?;
    match edit {
        Edit::Add => {
            let list = doc
                .entry("bookmarks")
                .or_insert_with(|| toml_edit::value(toml_edit::Array::new()))
                .as_array_mut()
                .ok_or_else(|| error(&"`bookmarks` is not a list"))?;
            if !list.iter().any(|item| matches(item, &location, dir)) {
                list.push(value);
            }
        }
        Edit::Remove => match doc.get_mut("bookmarks") {
            None => return Ok(()),
            Some(item) => {
                let list = item
                    .as_array_mut()
                    .ok_or_else(|| error(&"`bookmarks` is not a list"))?;
                list.retain(|item| !matches(item, &location, dir));
            }
        },
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| error(&e))?;
    }
    std::fs::write(path, doc.to_string()).map_err(|e| error(&e))
}

/// Whether a stored list entry (versioned location or legacy path string)
/// names `dir`.
fn matches(item: &toml_edit::Value, location: &Location, dir: &Path) -> bool {
    let Some(stored) = item.as_str() else {
        return false;
    };
    if stored.starts_with(crate::location_codec::PREFIX) {
        crate::location_codec::decode(stored).is_ok_and(|stored| stored == *location)
    } else {
        Path::new(stored) == dir
    }
}

#[cfg(test)]
mod tests {
    use super::{add_bookmark_to, contains, remove_bookmark_from, toggle};
    use crate::settings::Settings;
    use std::path::{Path, PathBuf};

    #[test]
    fn toggle_adds_then_removes_in_memory() {
        let mut list = vec![PathBuf::from("/a")];
        assert!(toggle(&mut list, Path::new("/b")));
        assert!(contains(&list, Path::new("/b")));
        assert!(!toggle(&mut list, Path::new("/b")));
        assert_eq!(list, [PathBuf::from("/a")]);
        assert!(!toggle(&mut list, Path::new("/a")));
        assert!(list.is_empty());
    }

    #[test]
    fn removing_from_the_file_keeps_comments_and_other_entries() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# keep\ntheme = \"x\"\nbookmarks = [\"/legacy\"]\n").unwrap();
        add_bookmark_to(&path, Path::new("/srv/a")).unwrap();
        add_bookmark_to(&path, Path::new("/srv/b")).unwrap();
        remove_bookmark_from(&path, Path::new("/srv/a")).unwrap();
        remove_bookmark_from(&path, Path::new("/legacy")).unwrap();
        remove_bookmark_from(&path, Path::new("/never")).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# keep") && text.contains("theme"));
        assert_eq!(
            Settings::parse(&text).unwrap().bookmarks,
            [PathBuf::from("/srv/b")]
        );
    }

    #[test]
    fn removing_without_a_file_creates_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        remove_bookmark_from(&path, Path::new("/srv/a")).unwrap();
        assert!(!path.exists());
    }
}
