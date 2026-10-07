//! `mimeapps.list` files: default applications and added or removed
//! associations, merged in precedence order; UI-independent.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::ini::Ini;

#[derive(Debug, Default, Clone)]
struct List {
    default: HashMap<String, Vec<String>>,
    added: HashMap<String, Vec<String>>,
    removed: HashMap<String, Vec<String>>,
}

fn ids(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(String::from)
        .collect()
}

impl List {
    fn parse(text: &str) -> Self {
        let ini = Ini::parse(text);
        let table = |section: &str| -> HashMap<String, Vec<String>> {
            let mut map: HashMap<String, Vec<String>> = HashMap::new();
            for (mime, value) in ini.section(section) {
                map.entry(mime.clone()).or_default().extend(ids(value));
            }
            map
        };
        Self {
            default: table("Default Applications"),
            added: table("Added Associations"),
            removed: table("Removed Associations"),
        }
    }
}

/// The `mimeapps.list` files, most important first.
#[derive(Debug, Default, Clone)]
pub struct MimeApps {
    files: Vec<List>,
}

impl MimeApps {
    /// Reads the files at `paths` (see [`crate::xdg::Env::mimeapps_paths`]);
    /// missing or unreadable ones are skipped.
    pub fn load(paths: &[PathBuf]) -> Self {
        Self {
            files: paths
                .iter()
                .filter_map(|p| std::fs::read_to_string(p).ok())
                .map(|text| List::parse(&text))
                .collect(),
        }
    }

    /// From file contents, most important first.
    #[cfg(test)]
    pub fn from_texts(texts: &[&str]) -> Self {
        Self {
            files: texts.iter().map(|t| List::parse(t)).collect(),
        }
    }

    /// Desktop file IDs configured as default for `mime`, in preference
    /// order across all files; the caller takes the first installed one.
    pub fn defaults(&self, mime: &str) -> Vec<String> {
        self.files
            .iter()
            .filter_map(|f| f.default.get(mime))
            .flatten()
            .cloned()
            .collect()
    }

    /// IDs added for `mime`, most important file first, without those a
    /// file of equal or higher importance removed.
    pub fn added(&self, mime: &str) -> Vec<String> {
        let mut removed: HashSet<&str> = HashSet::new();
        let mut result: Vec<String> = Vec::new();
        for file in &self.files {
            removed.extend(
                file.removed
                    .get(mime)
                    .into_iter()
                    .flatten()
                    .map(String::as_str),
            );
            for id in file.added.get(mime).into_iter().flatten() {
                if !removed.contains(id.as_str()) && !result.contains(id) {
                    result.push(id.clone());
                }
            }
        }
        result
    }

    /// IDs removed for `mime` in any file.
    pub fn removed(&self, mime: &str) -> HashSet<String> {
        self.files
            .iter()
            .filter_map(|f| f.removed.get(mime))
            .flatten()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_file_then_list_order() {
        let apps = MimeApps::from_texts(&[
            "[Default Applications]\ntext/plain=a.desktop;b.desktop\n",
            "[Default Applications]\ntext/plain=c.desktop\nimage/png=d.desktop\n",
        ]);
        assert_eq!(
            apps.defaults("text/plain"),
            ["a.desktop", "b.desktop", "c.desktop"]
        );
        assert_eq!(apps.defaults("image/png"), ["d.desktop"]);
        assert!(apps.defaults("video/mp4").is_empty());
    }

    #[test]
    fn removal_beats_additions_in_less_important_files() {
        let apps = MimeApps::from_texts(&[
            "[Added Associations]\ntext/plain=new.desktop;x.desktop;\n\
             [Removed Associations]\ntext/plain=old.desktop;\n",
            "[Added Associations]\ntext/plain=old.desktop;x.desktop;kept.desktop\n",
        ]);
        assert_eq!(
            apps.added("text/plain"),
            ["new.desktop", "x.desktop", "kept.desktop"]
        );
        assert!(apps.removed("text/plain").contains("old.desktop"));
    }

    #[test]
    fn load_skips_missing_files_and_honours_order() {
        let tmp = tempfile::TempDir::new().unwrap();
        let first = tmp.path().join("kde-mimeapps.list");
        let second = tmp.path().join("mimeapps.list");
        std::fs::write(&first, "[Default Applications]\ntext/plain=kate.desktop\n").unwrap();
        std::fs::write(
            &second,
            "[Default Applications]\ntext/plain=gedit.desktop\n",
        )
        .unwrap();
        let apps = MimeApps::load(&[tmp.path().join("none"), first, second]);
        assert_eq!(
            apps.defaults("text/plain"),
            ["kate.desktop", "gedit.desktop"]
        );
    }
}
