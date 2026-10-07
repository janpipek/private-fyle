//! Showing search results in place of a directory listing.

use std::path::PathBuf;

use super::{Entry, Folder, Row};
use crate::search::Hit;

impl Folder {
    /// Title of the search whose results are shown, if any.
    pub fn results(&self) -> Option<&str> {
        self.results.as_deref()
    }

    /// Switches to showing search results under `root` (initially none;
    /// see [`Folder::add_results`]). ".." leads back to `root`.
    pub fn show_results(&mut self, root: PathBuf, title: String) {
        self.clear_sizes();
        self.all = vec![Row {
            entry: Entry {
                name: "..".into(),
                path: root.clone(),
                is_dir: true,
                is_parent: true,
                size: 0,
                modified: None,
                attrs: Default::default(),
                link: Default::default(),
            },
            depth: 0,
            hit_line: None,
        }];
        self.path = root;
        self.mount = None;
        self.results = Some(title);
        self.generation += 1;
        self.marked.clear();
        self.filter.clear();
        self.cursor = 0;
        self.apply_filter();
    }

    /// Appends search hits, keeping the cursor on its entry.
    pub fn add_results(&mut self, hits: Vec<Hit>) {
        let selected = self.current().map(|e| e.path.clone());
        self.all.extend(hits.into_iter().map(|hit| Row {
            entry: hit.entry,
            depth: 0,
            hit_line: hit.line,
        }));
        self.apply_filter();
        self.select_path(selected.as_deref());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::model::test_support::{fixture, loaded, names};
    use crate::model::{Activation, Sort};
    use std::fs;

    #[test]
    fn search_results_mode() {
        let dir = fixture();
        fs::write(dir.path().join("sub").join("deep.txt"), "").unwrap();
        let mut folder = loaded(&dir);
        let entry = |rel: &str| {
            let path = dir.path().join(rel);
            Hit {
                entry: Entry {
                    name: rel.into(),
                    is_dir: path.is_dir(),
                    path,
                    is_parent: false,
                    size: 0,
                    modified: None,
                    attrs: Default::default(),
                    link: Default::default(),
                },
                line: (rel == "a.txt").then(|| (3, "found".to_string())),
            }
        };

        folder.show_results(dir.path().to_path_buf(), "search".into());
        assert_eq!(folder.results(), Some("search"));
        assert_eq!(names(&folder), [".."]);
        folder.add_results(vec![entry("sub/deep.txt"), entry("b.txt")]);
        folder.add_results(vec![entry("a.txt")]);
        assert_eq!(names(&folder), ["..", "sub/deep.txt", "b.txt", "a.txt"]);
        assert_eq!(folder.entries()[3].hit_line, Some((3, "found".into())));

        folder.set_sort(Sort::default()); // sorts in place, no re-listing
        assert_eq!(names(&folder), ["..", "a.txt", "b.txt", "sub/deep.txt"]);
        assert_eq!(folder.entries()[1].hit_line, Some((3, "found".into())));

        fs::remove_file(dir.path().join("b.txt")).unwrap();
        folder.load().unwrap(); // drops vanished hits
        assert_eq!(names(&folder), ["..", "a.txt", "sub/deep.txt"]);

        // Enter on a hit shows it in its directory.
        folder.set_cursor(2);
        assert_eq!(folder.activate(), Ok(Activation::Entered));
        assert_eq!(folder.results(), None);
        assert_eq!(folder.path(), dir.path().join("sub"));
        assert_eq!(folder.current().unwrap().name, "deep.txt");

        // ".." / go up from results returns to the search root.
        folder.show_results(dir.path().to_path_buf(), "again".into());
        folder.go_up().unwrap();
        assert_eq!((folder.results(), folder.path()), (None, dir.path()));
        assert_eq!(names(&folder), ["..", "sub", "a.txt"]);
    }
}
