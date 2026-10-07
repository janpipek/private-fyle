//! Browsing archives like directories, including the two-step load of an
//! archive that is not cached yet.

use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::Arc;

use super::Folder;
#[cfg(test)]
use super::{Entry, PanelContent, Row};
use crate::archive;
use crate::fs_ops;
#[cfg(test)]
use crate::links::LinkState;
use crate::location::Location;
#[cfg(test)]
use std::io;

/// An archive shown as a directory tree.
#[derive(Clone)]
pub struct Mount {
    /// The local archive file whose index is mounted.
    pub file: PathBuf,
    #[cfg(test)]
    index: Arc<archive::Index>,
}

impl Mount {
    /// Holds an index already read by the listing service.
    pub fn loaded(file: PathBuf) -> Self {
        Self {
            file,
            #[cfg(test)]
            index: Arc::default(),
        }
    }
}

impl Folder {
    /// The archive file being browsed, if any.
    pub fn archive(&self) -> Option<&Path> {
        self.mount.as_ref().map(|m| m.file.as_path())
    }

    /// The location inside the archive being browsed (empty at its root).
    pub fn inner_path(&self) -> Option<String> {
        self.location().and_then(|location| match location {
            Location::Archive { member, .. } => Some(member.as_str().to_owned()),
            _ => None,
        })
    }

    /// The archive and the inner paths of [`Folder::target_locations`], for
    /// extracting them.
    pub fn archive_targets(&self) -> Option<(PathBuf, Vec<String>)> {
        let file = self.archive()?.to_path_buf();
        let inner = self
            .target_locations()
            .iter()
            .filter_map(|location| match location {
                Location::Archive { source, member }
                    if source.as_local_path() == Some(file.as_path()) =>
                {
                    Some(member.as_str().to_owned())
                }
                _ => None,
            })
            .collect();
        Some((file, inner))
    }

    /// The archive file that must be read before `path` can be shown, if
    /// `path` is inside one that is not cached yet (see
    /// [`archive::Index::cached`]). Showing it would block on that read.
    #[cfg(test)]
    pub fn archive_needed(&self, path: &Path) -> Option<PathBuf> {
        if crate::remote::is_remote(path) || path.as_os_str().is_empty() || path.is_dir() {
            return None;
        }
        let (file, _) = archive::locate(path)?;
        (!archive::Index::is_cached(&file)).then_some(file)
    }

    /// Where [`Folder::activate`] would go: the cursor entry if it is a
    /// folder or an archive.
    pub fn activation_target(&self) -> Option<crate::location::PanelTarget> {
        let entry = self.current()?;
        if entry.is_parent {
            return Some(Self::parent_row_target(&entry.path, &entry.location));
        }
        (entry.is_dir
            || (self.mount.is_none()
                && !self.is_remote()
                && entry
                    .location
                    .as_local_path()
                    .is_some_and(|path| archive::format_of(path).is_some())))
        .then(|| crate::location::PanelTarget::Location(entry.location.clone()))
    }

    /// Shows an empty listing of the archive location `target` while its
    /// archive is read elsewhere (see [`Folder::archive_needed`]). The
    /// previous state comes back with [`Folder::end_archive_load`].
    #[cfg(test)]
    pub fn begin_archive_load(&mut self, target: PathBuf) {
        self.end_archive_load(false);
        let Some(file) = self.archive_needed(&target) else {
            return;
        };
        let saved = Box::new(self.clone());
        self.entries.clear();
        self.all.clear();
        self.marked.clear();
        self.filter.clear();
        self.cursor = 0;
        self.mount = Some(Mount {
            file,
            index: Arc::default(),
        });
        self.content = PanelContent::Directory(
            Location::archive(
                Location::local(
                    self.mount
                        .as_ref()
                        .expect("loading has a mount")
                        .file
                        .clone(),
                )
                .expect("archive file has an absolute path"),
                &archive::inner_of(self.archive().expect("loading has an archive"), &target)
                    .expect("target is within the archive"),
            )
            .expect("archive target has a valid member path"),
        );
        self.generation += 1;
        self.loading = Some(saved);
    }

    /// Whether an archive is being read for this folder.
    pub fn is_loading(&self) -> bool {
        #[cfg(test)]
        {
            self.loading.is_some()
        }
        #[cfg(not(test))]
        {
            false
        }
    }

    /// Ends [`Folder::begin_archive_load`], restoring the previous state.
    /// With `leave`, the cursor goes to the archive in its containing
    /// directory (used when the read is cancelled).
    #[cfg(test)]
    pub fn end_archive_load(&mut self, leave: bool) {
        let Some(saved) = self.loading.take() else {
            return;
        };
        let file = self.archive().map(Path::to_path_buf);
        *self = *saved;
        if let (true, Some(file)) = (leave, file) {
            let parent = fs_ops::parent_of(&file).unwrap_or_default();
            if self.path() != parent || self.mount.is_some() {
                // Best effort: the previous state stays if the directory is gone.
                let _ = self.replace(parent, Some(file));
            } else {
                self.select_path(Some(&file));
            }
        }
    }

    /// A compact text form of the panel location, separating an archive file
    /// from its internal path.
    pub fn header(&self) -> String {
        match (self.archive(), self.inner_path()) {
            (Some(file), Some(inner)) => archive::location(file, &inner),
            _ => fs_ops::display_path(&self.path()),
        }
    }

    /// The archive (and the inner path in it) that `path` is in, if it is
    /// not an ordinary directory. The archive is read again only when it
    /// changed on disk ([`archive::Index::cached`]).
    #[cfg(test)]
    pub fn mount_for(&self, path: &Path) -> io::Result<Option<(Mount, String)>> {
        if crate::remote::is_remote(path) || path.as_os_str().is_empty() || path.is_dir() {
            return Ok(None);
        }
        let Some((file, inner)) = archive::locate(path) else {
            return Ok(None);
        };
        let index = archive::Index::cached(&file)?;
        let mount = Mount { file, index };
        Ok(Some((mount, inner)))
    }

    /// Appends the sorted listing of the archive directory `inner`, with
    /// ".." leading to the parent (the containing directory at the root).
    #[cfg(test)]
    pub fn list_archive(&self, mount: &Mount, inner: &str, out: &mut Vec<Row>) -> io::Result<()> {
        if !mount.index.is_dir(inner) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "not a folder in the archive",
            ));
        }
        let parent = match inner.rsplit_once('/') {
            Some((parent, _)) => Some(archive::virtual_path(&mount.file, parent)),
            None if inner.is_empty() => fs_ops::parent_of(&mount.file),
            None => Some(mount.file.clone()),
        };
        let mut entries: Vec<Entry> = parent
            .into_iter()
            .map(|path| Entry {
                name: "..".into(),
                path,
                is_dir: true,
                is_parent: true,
                size: 0,
                modified: None,
                attrs: Default::default(),
                link: Default::default(),
            })
            .collect();
        entries.extend(mount.index.list(inner).map(|node| Entry {
            name: node.name.clone(),
            path: archive::virtual_path(&mount.file, &node.path),
            is_dir: node.is_dir(),
            is_parent: false,
            size: node.size,
            modified: node.modified,
            attrs: fs_ops::Attrs {
                mode: node.mode,
                ..Default::default()
            },
            link: match &node.kind {
                archive::Kind::Symlink(target) => LinkState::Link {
                    target: target.into(),
                },
                _ => LinkState::NotLink,
            },
        }));
        fs_ops::sort_entries(&mut entries, self.sort.key, self.sort.descending);
        out.extend(entries.into_iter().map(|entry| {
            Row {
                location: Location::from_panel_path(&entry.path, Some(&mount.file))
                    .expect("archive listing contains validated paths"),
                entry,
                depth: 0,
                hit_line: None,
            }
        }));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs_ops::SortKey;
    use crate::location::Location;
    use crate::model::test_support::{fixture, loaded, names};
    use crate::model::{Activation, Folder, Sort};
    use std::fs;
    use std::path::Path;

    fn zip_file(path: &Path) {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (name, text) in [
            ("top.txt", "t"),
            ("docs/a.md", "aa"),
            ("docs/deep/b.md", "b"),
        ] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn browses_an_archive_like_a_directory() {
        let dir = fixture();
        let zip = dir.path().join("pack.zip");
        zip_file(&zip);
        let mut folder = loaded(&dir);
        assert_eq!(folder.archive(), None);
        folder.select_path(Some(&zip));
        assert_eq!(folder.activate(), Ok(Activation::Entered));
        assert_eq!(folder.archive(), Some(zip.as_path()));
        assert!(matches!(folder.location(), Some(Location::Archive { .. })));
        assert!(matches!(
            folder.entries()[1].location,
            Location::Archive { .. }
        ));
        assert_eq!(folder.inner_path().as_deref(), Some(""));
        assert_eq!(names(&folder), ["..", "docs", "top.txt"]);
        assert_eq!(folder.header(), format!("{} ›", zip.display()));

        folder.set_cursor(1);
        assert_eq!(folder.activate(), Ok(Activation::Entered));
        assert_eq!(folder.inner_path().as_deref(), Some("docs"));
        assert_eq!(names(&folder), ["..", "deep", "a.md"]);
        assert_eq!(folder.header(), format!("{} › docs", zip.display()));
        assert_eq!(folder.entries()[1].path, zip.join("docs/deep"));

        // Files are not opened; marks and targets work.
        folder.set_cursor(2);
        assert_eq!(
            folder.archive_targets(),
            Some((zip.clone(), vec!["docs/a.md".to_string()]))
        );
        folder.toggle_mark();
        assert_eq!(folder.marked_count(), 1);

        // Up to the archive root, cursor on the folder we came from.
        folder.go_up().unwrap();
        assert_eq!(folder.inner_path().as_deref(), Some(""));
        assert_eq!(folder.current().unwrap().name, "docs");
        // Reloading an unchanged archive does not read it again.
        let before = folder.mount.as_ref().unwrap().index.clone();
        folder.load().unwrap();
        assert!(Arc::ptr_eq(&before, &folder.mount.as_ref().unwrap().index));
        // Up again leaves the archive, cursor on it.
        folder.go_up().unwrap();
        assert_eq!(folder.archive(), None);
        assert!(matches!(folder.location(), Some(Location::Local(_))));
        assert_eq!(folder.path(), dir.path());
        assert_eq!(folder.current().unwrap().path, zip);
    }

    #[test]
    fn uncached_archive_loads_in_two_steps() {
        let dir = fixture();
        let zip = dir.path().join("lazy.zip");
        zip_file(&zip);
        let mut folder = loaded(&dir);
        let before: Vec<String> = names(&folder).into_iter().map(String::from).collect();
        folder.select_path(Some(&zip));
        assert_eq!(folder.activation_target().unwrap().panel_path(), zip);
        assert_eq!(folder.archive_needed(dir.path()), None);
        assert_eq!(folder.archive_needed(&zip), Some(zip.clone()));

        // Loading shows the archive location with nothing in it.
        folder.begin_archive_load(zip.clone());
        assert!(folder.is_loading());
        assert_eq!(folder.header(), format!("{} ›", zip.display()));
        assert!(folder.entries().is_empty());

        // Cancelling returns to the directory, cursor on the archive.
        folder.end_archive_load(true);
        assert!(!folder.is_loading());
        assert_eq!(
            names(&folder),
            before.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert_eq!(folder.current().unwrap().path, zip);

        // Reloading does nothing while loading; going up cancels the read.
        folder.begin_archive_load(zip.clone());
        folder.load().unwrap();
        assert!(folder.is_loading());
        folder.go_up().unwrap();
        assert!(!folder.is_loading());
        assert_eq!(folder.archive(), None);
        assert_eq!(folder.current().unwrap().path, zip);

        // The legacy test-only loading placeholder is independent of a pure
        // hidden-file option change; cancel it explicitly as the view does.
        folder.begin_archive_load(zip.clone());
        assert_eq!(folder.set_show_hidden(true), None);
        folder.end_archive_load(false);
        assert!(!folder.is_loading());
        assert_eq!(folder.archive(), None);

        // Once read (elsewhere), entering is instant and needs no loading.
        archive::Index::cached(&zip).unwrap();
        assert_eq!(folder.archive_needed(&zip), None);
        folder.begin_archive_load(zip.clone());
        assert!(!folder.is_loading());
        assert_eq!(folder.activate(), Ok(Activation::Entered));
        assert_eq!(names(&folder), ["..", "docs", "top.txt"]);
    }

    #[test]
    fn parent_row_leaves_the_archive_and_history_returns() {
        let dir = fixture();
        let zip = dir.path().join("pack.zip");
        zip_file(&zip);
        let mut folder = loaded(&dir);
        folder.select_path(Some(&zip));
        folder.activate().unwrap();
        folder.set_cursor(1);
        folder.activate().unwrap(); // docs
        folder.set_cursor(0);
        folder.activate().unwrap(); // ".." to the archive root
        assert_eq!(folder.inner_path().as_deref(), Some(""));
        folder.set_cursor(0);
        folder.activate().unwrap(); // ".." out of the archive
        assert_eq!(folder.archive(), None);

        // Back re-enters the archive.
        assert_eq!(folder.go_back(), Ok(true));
        assert_eq!(folder.archive(), Some(zip.as_path()));
        assert!(matches!(folder.location(), Some(Location::Archive { .. })));
        assert_eq!(folder.go_back(), Ok(true));
        assert_eq!(folder.inner_path().as_deref(), Some("docs"));
        assert!(matches!(folder.location(), Some(Location::Archive { .. })));
        assert_eq!(folder.go_forward(), Ok(true));
        assert_eq!(folder.inner_path().as_deref(), Some(""));
        assert_eq!(folder.go_forward(), Ok(true));
        assert_eq!(folder.archive(), None);
        assert!(matches!(folder.location(), Some(Location::Local(_))));
    }

    #[test]
    fn archive_listing_honours_hidden_and_sort() {
        let dir = fixture();
        let zip = dir.path().join("pack.zip");
        {
            use std::io::Write;
            let mut z = zip::ZipWriter::new(fs::File::create(&zip).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            for (name, text) in [("a.txt", "1"), ("b.txt", "1234"), (".hid", "")] {
                z.start_file(name, opts).unwrap();
                z.write_all(text.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        let mut folder = Folder::new(zip.clone()).unwrap();
        assert_eq!(folder.load_or_ancestor(), None);
        assert_eq!(folder.archive(), Some(zip.as_path()));
        assert_eq!(names(&folder), ["..", "a.txt", "b.txt"]);
        folder.set_show_hidden(true);
        assert_eq!(names(&folder), ["..", ".hid", "a.txt", "b.txt"]);
        folder.set_sort(Sort {
            key: SortKey::Size,
            descending: true,
        });
        assert_eq!(names(&folder), ["..", "b.txt", "a.txt", ".hid"]);
        assert_eq!(folder.current().unwrap().name, "..");
    }

    #[test]
    fn broken_or_missing_archives_report_errors() {
        let dir = fixture();
        let bad = dir.path().join("bad.zip");
        fs::write(&bad, "not a zip").unwrap();
        let mut folder = loaded(&dir);
        folder.select_path(Some(&bad));
        assert!(folder.activate().is_err());
        assert_eq!(folder.archive(), None);
        assert_eq!(folder.path(), dir.path());

        // An archive that disappears falls back to its directory.
        let zip = dir.path().join("pack.zip");
        zip_file(&zip);
        folder.load().unwrap();
        folder.select_path(Some(&zip));
        folder.activate().unwrap();
        fs::remove_file(&zip).unwrap();
        folder.load_or_ancestor();
        assert_eq!(folder.archive(), None);
        assert_eq!(folder.path(), dir.path());
    }
}
