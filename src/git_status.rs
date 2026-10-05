//! Git status of a directory's entries, for the Git column. Runs `git
//! status` (off the UI thread) and parses its porcelain output.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One entry's state, most important last (a directory shows the most
/// important state found inside it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark {
    Ignored,
    Untracked,
    Renamed,
    Added,
    Deleted,
    Modified,
    Conflicted,
}

impl Mark {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Ignored => "!",
            Self::Untracked => "?",
            Self::Renamed => "R",
            Self::Added => "A",
            Self::Deleted => "D",
            Self::Modified => "M",
            Self::Conflicted => "U",
        }
    }

    /// From porcelain v1's two status letters.
    fn parse(xy: &[u8]) -> Option<Self> {
        let (x, y) = (*xy.first()?, *xy.get(1)?);
        Some(match (x, y) {
            (b'!', b'!') => Self::Ignored,
            (b'?', b'?') => Self::Untracked,
            (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => Self::Conflicted,
            (_, b'M') | (b'M', _) | (_, b'T') | (b'T', _) => Self::Modified,
            (_, b'D') | (b'D', _) => Self::Deleted,
            (b'A', _) => Self::Added,
            (b'R', _) | (b'C', _) => Self::Renamed,
            _ => return None,
        })
    }
}

/// Status of the paths under one directory.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// Absolute path → state. Untracked or ignored directories appear as
    /// one entry (git does not list their contents).
    marks: HashMap<PathBuf, Mark>,
}

impl Status {
    /// Parses `git status --porcelain=v1 -z` output; paths are relative to
    /// the repository `root`.
    pub fn parse(root: &Path, output: &[u8]) -> Self {
        let mut marks = HashMap::new();
        let mut fields = output.split(|&b| b == 0);
        while let Some(field) = fields.next() {
            if field.len() < 4 {
                continue;
            }
            let (xy, path) = (&field[..2], &field[3..]);
            if matches!(xy[0], b'R' | b'C') {
                fields.next(); // the original path
            }
            let Some(mark) = Mark::parse(xy) else {
                continue;
            };
            let path = String::from_utf8_lossy(path);
            marks.insert(root.join(path.trim_end_matches('/')), mark);
        }
        Self { marks }
    }

    /// The entry's state: its own, or for a directory the most important
    /// state inside it.
    pub fn mark(&self, path: &Path, is_dir: bool) -> Option<Mark> {
        let own = self.marks.get(path).copied();
        if !is_dir || own.is_some() {
            return own;
        }
        self.marks
            .iter()
            .filter(|(p, mark)| **mark != Mark::Ignored && p.starts_with(path))
            .map(|(_, mark)| *mark)
            .max()
    }
}

/// Runs git for `dir`; `None` outside a repository or without git.
pub fn status(dir: &Path) -> Option<Status> {
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| o.stdout)
    };
    let root = git(&["rev-parse", "--show-toplevel"])?;
    let root = PathBuf::from(String::from_utf8_lossy(&root).trim_end());
    let output = git(&[
        "status",
        "--porcelain=v1",
        "-z",
        "--ignored=matching",
        "--",
        ".",
    ])?;
    Some(Status::parse(&root, &output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_and_aggregates_directories() {
        let root = Path::new("/repo");
        let output = b" M src/main.rs\0?? notes/\0R  new.rs\0old.rs\0A  src/sub/a.rs\0\
                       !! target/\0UU both.rs\0 D gone.txt\0";
        let status = Status::parse(root, output);
        let mark = |rel: &str, is_dir| status.mark(&root.join(rel), is_dir);
        assert_eq!(mark("src/main.rs", false), Some(Mark::Modified));
        assert_eq!(mark("notes", true), Some(Mark::Untracked));
        assert_eq!(mark("new.rs", false), Some(Mark::Renamed));
        assert_eq!(mark("old.rs", false), None);
        assert_eq!(mark("target", true), Some(Mark::Ignored));
        assert_eq!(mark("both.rs", false), Some(Mark::Conflicted));
        assert_eq!(mark("gone.txt", false), Some(Mark::Deleted));
        assert_eq!(mark("clean.rs", false), None);
        // Directories: the most important state inside.
        assert_eq!(mark("src", true), Some(Mark::Modified));
        assert_eq!(mark("src/sub", true), Some(Mark::Added));
        assert_eq!(mark("docs", true), None);
    }

    #[test]
    fn runs_git_in_a_repository() {
        let dir = std::env::temp_dir().join(format!("fyle-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if !Command::new("git")
            .args(["init", "-q"])
            .current_dir(&dir)
            .status()
            .is_ok_and(|s| s.success())
        {
            return; // no git here
        }
        std::fs::write(dir.join("new.txt"), "").unwrap();
        let status = status(&dir).unwrap();
        // git may report the canonical root; compare by name.
        let marks: Vec<_> = status
            .marks
            .iter()
            .map(|(p, m)| (p.file_name().unwrap().to_owned(), *m))
            .collect();
        assert_eq!(marks, [("new.txt".into(), Mark::Untracked)]);
        assert!(super::status(&std::env::temp_dir().join("fyle-no-such-dir")).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
