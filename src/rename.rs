//! Renaming one or many entries from edited text (one name per line),
//! UI-independent. Validates everything before touching the disk and
//! handles swaps (a→b, b→a) through temporary names.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// One rename; both paths are in the same directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    pub from: PathBuf,
    pub to: PathBuf,
}

/// Pairs `sources` with the lines of `text`. Unchanged names are skipped.
/// Fails without renaming anything if a name is empty, contains a path
/// separator, is duplicated, or would replace an entry not being renamed.
pub fn plan(sources: &[PathBuf], text: &str) -> Result<Vec<Rename>, String> {
    let names: Vec<&str> = text.lines().map(str::trim_end).collect();
    let names: Vec<&str> = match names.as_slice() {
        // A trailing empty line from the editor is not a name.
        [rest @ .., ""] if rest.len() == sources.len() => rest.to_vec(),
        _ => names,
    };
    if names.len() != sources.len() {
        return Err(format!(
            "{} names for {} entries: keep one line per entry",
            names.len(),
            sources.len()
        ));
    }
    let mut targets = HashSet::new();
    let mut renames = Vec::new();
    for (from, name) in sources.iter().zip(names) {
        if name.trim().is_empty() {
            return Err("empty name".into());
        }
        if name.contains('/')
            || (cfg!(windows) && name.contains('\\'))
            || name == "."
            || name == ".."
        {
            return Err(format!("invalid name {name:?}"));
        }
        let to = from.with_file_name(name);
        if !targets.insert(to.clone()) {
            return Err(format!("{name:?} is used twice"));
        }
        if to != *from {
            renames.push(Rename {
                from: from.clone(),
                to,
            });
        }
    }
    let moving: HashSet<&PathBuf> = renames.iter().map(|r| &r.from).collect();
    for rename in &renames {
        // Taking over a name that is itself being renamed away is fine.
        if rename.to.symlink_metadata().is_ok()
            && !moving.contains(&rename.to)
            && !same_file(&rename.from, &rename.to)
        {
            return Err(format!("{} already exists", rename.to.display()));
        }
    }
    Ok(renames)
}

/// Case-only renames on case-insensitive filesystems point at the same file.
fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Applies renames in two phases (to temporary names, then to the final
/// ones) so swaps and chains work. Returns how many succeeded and errors.
pub fn apply(renames: &[Rename]) -> (usize, Vec<String>) {
    let mut staged = Vec::new();
    let mut errors = Vec::new();
    for (i, rename) in renames.iter().enumerate() {
        let tmp = rename
            .from
            .with_file_name(format!(".fyle-rename-{}-{i}", std::process::id()));
        match fs::rename(&rename.from, &tmp) {
            Ok(()) => staged.push((tmp, rename)),
            Err(e) => errors.push(format!("{}: {e}", rename.from.display())),
        }
    }
    let mut done = 0;
    for (tmp, rename) in staged {
        match fs::rename(&tmp, &rename.to) {
            Ok(()) => done += 1,
            Err(e) => {
                // Put it back rather than leave a temporary name.
                let _ = fs::rename(&tmp, &rename.from);
                errors.push(format!("{}: {e}", rename.to.display()));
            }
        }
    }
    (done, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            fs::write(dir.path().join(f), f).unwrap();
        }
        dir
    }

    fn listing(dir: &Path) -> Vec<(String, String)> {
        let mut items: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| {
                let path = e.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                (name, fs::read_to_string(&path).unwrap())
            })
            .collect();
        items.sort();
        items
    }

    #[test]
    fn renames_and_swaps() {
        let tmp = dir(&["a", "b", "c"]);
        let d = tmp.path();
        let sources = [d.join("a"), d.join("b"), d.join("c")];
        let renames = plan(&sources, "b\na\nc\n").unwrap();
        assert_eq!(renames.len(), 2); // "c" unchanged
        assert_eq!(apply(&renames), (2, vec![]));
        let pairs = |v: &[(&str, &str)]| {
            v.iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect::<Vec<_>>()
        };
        assert_eq!(listing(d), pairs(&[("a", "b"), ("b", "a"), ("c", "c")]));
    }

    #[test]
    fn rejects_bad_plans_without_touching_files() {
        let tmp = dir(&["a", "b", "other"]);
        let d = tmp.path();
        let sources = [d.join("a"), d.join("b")];
        let err = |text| plan(&sources, text).unwrap_err();
        assert!(err("x").contains("1 names for 2"));
        assert!(err("x\n\n").contains("empty name"));
        assert!(err("x\nsub/y").contains("invalid name"));
        assert!(err("x\nx").contains("used twice"));
        assert!(err("other\ny").contains("already exists"));
        assert_eq!(plan(&sources, "a\nb").unwrap(), vec![]);
        assert_eq!(listing(d).len(), 3);
    }
}
