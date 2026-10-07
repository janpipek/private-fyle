//! MIME types from shared-mime-info's database (`mime/globs2`,
//! `subclasses`, `aliases`); UI-independent. Magic sniffing is not done:
//! the type comes from the file name only.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// The type of directories.
pub const DIRECTORY: &str = "inode/directory";
/// What an unknown file is.
pub const UNKNOWN: &str = "application/octet-stream";

#[derive(Debug, Clone)]
struct Glob {
    weight: u32,
    mime: String,
    /// Lowercased unless `case_sensitive`.
    pattern: Vec<char>,
    case_sensitive: bool,
}

/// Globs, subclass and alias tables merged from the data directories.
#[derive(Debug, Default, Clone)]
pub struct MimeDb {
    globs: Vec<Glob>,
    /// Type to its direct parents.
    parents: HashMap<String, Vec<String>>,
    aliases: HashMap<String, String>,
}

impl MimeDb {
    /// Reads `<dir>/mime/{globs2,subclasses,aliases}` of every data
    /// directory; missing files are skipped.
    pub fn load(data_dirs: &[PathBuf]) -> Self {
        let mut db = Self::default();
        for dir in data_dirs {
            let read = |name: &str| std::fs::read_to_string(dir.join("mime").join(name)).ok();
            if let Some(text) = read("globs2") {
                db.add_globs(&text);
            }
            if let Some(text) = read("subclasses") {
                db.add_subclasses(&text);
            }
            if let Some(text) = read("aliases") {
                db.add_aliases(&text);
            }
        }
        db
    }

    /// Adds `weight:type:glob[:flags]` lines.
    pub fn add_globs(&mut self, text: &str) {
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let mut parts = line.splitn(4, ':');
            let (Some(weight), Some(mime), Some(pattern)) =
                (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let Ok(weight) = weight.parse() else { continue };
            if pattern.is_empty() || pattern == "__NOGLOBS__" {
                continue;
            }
            let case_sensitive = parts
                .next()
                .is_some_and(|f| f.split(',').any(|f| f == "cs"));
            let pattern = if case_sensitive {
                pattern.chars().collect()
            } else {
                pattern.to_lowercase().chars().collect()
            };
            self.globs.push(Glob {
                weight,
                mime: mime.to_string(),
                pattern,
                case_sensitive,
            });
        }
    }

    /// Adds `child parent` lines.
    pub fn add_subclasses(&mut self, text: &str) {
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            if let Some((child, parent)) = line.split_once(char::is_whitespace) {
                self.parents
                    .entry(child.trim().to_string())
                    .or_default()
                    .push(parent.trim().to_string());
            }
        }
    }

    /// Adds `alias canonical` lines.
    pub fn add_aliases(&mut self, text: &str) {
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            if let Some((alias, mime)) = line.split_once(char::is_whitespace) {
                self.aliases
                    .insert(alias.trim().to_string(), mime.trim().to_string());
            }
        }
    }

    /// The canonical name of `mime` (aliases resolved).
    pub fn canonical<'a>(&'a self, mime: &'a str) -> &'a str {
        self.aliases.get(mime).map_or(mime, String::as_str)
    }

    /// The type of a file by name: the matching glob with the highest
    /// weight, then the longest pattern. Directories are `inode/directory`;
    /// no match gives `application/octet-stream`.
    pub fn detect(&self, file_name: &str, is_dir: bool) -> String {
        if is_dir {
            return DIRECTORY.into();
        }
        let lower: Vec<char> = file_name.to_lowercase().chars().collect();
        let exact: Vec<char> = file_name.chars().collect();
        self.globs
            .iter()
            .filter(|g| glob_match(&g.pattern, if g.case_sensitive { &exact } else { &lower }))
            .max_by_key(|g| (g.weight, g.pattern.len(), g.case_sensitive))
            .map_or_else(|| UNKNOWN.into(), |g| self.canonical(&g.mime).to_string())
    }

    /// `mime` followed by its ancestors, nearest first (breadth first); every
    /// `text/*` type has `text/plain` as an ancestor.
    pub fn with_ancestors(&self, mime: &str) -> Vec<String> {
        let first = self.canonical(mime).to_string();
        let mut seen: HashSet<String> = HashSet::from([first.clone()]);
        let mut order = vec![first];
        let mut next = 0;
        while next < order.len() {
            let parents = self.parents.get(&order[next]).cloned().unwrap_or_default();
            for parent in parents {
                let parent = self.canonical(&parent).to_string();
                if seen.insert(parent.clone()) {
                    order.push(parent);
                }
            }
            next += 1;
        }
        if order[0].starts_with("text/") && seen.insert("text/plain".into()) {
            order.push("text/plain".into());
        }
        order
    }
}

/// Shell-style match supporting `*`, `?`, `[set]`, `[!set]` and `\` escapes.
fn glob_match(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => (0..=text.len()).any(|skip| glob_match(rest, &text[skip..])),
        Some(('?', rest)) => !text.is_empty() && glob_match(rest, &text[1..]),
        Some(('[', rest)) => {
            let Some((&c, text_rest)) = text.split_first() else {
                return false;
            };
            let Some(end) = rest.iter().skip(1).position(|&p| p == ']').map(|e| e + 1) else {
                // No closing bracket: a literal '['.
                return c == '[' && glob_match(rest, text_rest);
            };
            let (negate, set) = match rest[..end].split_first() {
                Some((&('!' | '^'), set)) => (true, set),
                _ => (false, &rest[..end]),
            };
            let mut hit = false;
            let mut i = 0;
            while i < set.len() {
                if i + 2 < set.len() && set[i + 1] == '-' {
                    hit |= (set[i]..=set[i + 2]).contains(&c);
                    i += 3;
                } else {
                    hit |= set[i] == c;
                    i += 1;
                }
            }
            hit != negate && glob_match(&rest[end + 1..], text_rest)
        }
        Some(('\\', rest)) if !rest.is_empty() => {
            text.first() == Some(&rest[0]) && glob_match(&rest[1..], &text[1..])
        }
        Some((&p, rest)) => text.first() == Some(&p) && glob_match(rest, &text[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> MimeDb {
        let mut db = MimeDb::default();
        db.add_globs(
            "# comment\n\
             50:application/gzip:*.gz\n\
             50:application/x-compressed-tar:*.tar.gz\n\
             50:text/x-rust:*.rs\n\
             50:text/plain:*.txt\n\
             50:image/jpeg:*.jpg\n\
             10:image/x-lowweight:*.jpg\n\
             50:text/x-makefile:Makefile\n\
             50:text/x-csrc:*.c\n\
             50:text/x-c++src:*.C:cs\n\
             50:text/x-readme:README*\n\
             50:application/x-x:file[0-9].?\n\
             50:application/x-n:*.[!a]q\n\
             0:application/none:__NOGLOBS__\n",
        );
        db.add_subclasses(
            "text/x-rust text/plain\ntext/x-makefile text/plain\ntext/x-csrc text/x-chdr\n",
        );
        db.add_aliases("text/x-rust-alias text/x-rust\nimage/jpg image/jpeg\n");
        db
    }

    #[test]
    fn detects_by_longest_glob_and_weight() {
        let db = db();
        assert_eq!(db.detect("a.tar.gz", false), "application/x-compressed-tar");
        assert_eq!(db.detect("a.gz", false), "application/gzip");
        assert_eq!(db.detect("PHOTO.JPG", false), "image/jpeg");
        assert_eq!(db.detect("Makefile", false), "text/x-makefile");
        assert_eq!(db.detect("README.md", false), "text/x-readme");
        assert_eq!(db.detect("x", true), DIRECTORY);
        assert_eq!(db.detect("unknown.zzz", false), UNKNOWN);
    }

    #[test]
    fn case_sensitive_globs_and_classes() {
        let db = db();
        assert_eq!(db.detect("a.C", false), "text/x-c++src");
        assert_eq!(db.detect("a.c", false), "text/x-csrc");
        assert_eq!(db.detect("file3.x", false), "application/x-x");
        assert_eq!(db.detect("file33.x", false), UNKNOWN);
        assert_eq!(db.detect("x.bq", false), "application/x-n");
        assert_eq!(db.detect("x.aq", false), UNKNOWN);
    }

    #[test]
    fn ancestors_follow_subclasses_aliases_and_text() {
        let db = db();
        assert_eq!(
            db.with_ancestors("text/x-rust"),
            ["text/x-rust", "text/plain"]
        );
        assert_eq!(db.with_ancestors("text/x-rust-alias")[0], "text/x-rust");
        assert_eq!(
            db.with_ancestors("text/x-csrc"),
            ["text/x-csrc", "text/x-chdr", "text/plain"]
        );
        assert_eq!(db.with_ancestors("image/png"), ["image/png"]);
    }

    #[test]
    fn loads_from_directories_in_order() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mime = tmp.path().join("mime");
        std::fs::create_dir_all(&mime).unwrap();
        std::fs::write(mime.join("globs2"), "50:text/x-rust:*.rs\n").unwrap();
        std::fs::write(mime.join("subclasses"), "text/x-rust text/plain\n").unwrap();
        let db = MimeDb::load(&[tmp.path().join("missing"), tmp.path().to_path_buf()]);
        assert_eq!(db.detect("main.rs", false), "text/x-rust");
        assert_eq!(db.with_ancestors("text/x-rust").len(), 2);
    }
}
