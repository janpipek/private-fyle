//! Finding files by name and searching their contents, UI-independent.
//! Built on ripgrep's libraries: `ignore` walks the tree (honouring
//! `.gitignore`), `grep-searcher` + `grep-regex` search contents.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use globset::{GlobBuilder, GlobMatcher};
use grep_regex::RegexMatcherBuilder;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, sinks};

use crate::fs_ops::Entry;

/// What to look for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub root: PathBuf,
    /// File name pattern: a glob if it contains `*`, `?` or `[`, otherwise a
    /// substring. Case-insensitive; empty matches every name.
    pub name: String,
    /// Text the file must contain (literal, smart case: case-insensitive
    /// unless it has capitals). Empty: no content search.
    pub content: String,
    pub hidden: bool,
    /// Honour `.gitignore` and similar files.
    pub ignore_files: bool,
}

/// Stops collecting after this many hits.
pub const MAX_HITS: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub entry: Entry,
    /// First matching line (1-based) and its text, for content searches.
    pub line: Option<(u64, String)>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub hits: usize,
    pub truncated: bool,
    pub cancelled: bool,
}

enum NameMatcher {
    Any,
    Substring(String),
    Glob(GlobMatcher),
}

impl NameMatcher {
    fn new(pattern: &str) -> Result<Self, String> {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            Ok(Self::Any)
        } else if pattern.contains(['*', '?', '[']) {
            let glob = GlobBuilder::new(pattern)
                .case_insensitive(true)
                .literal_separator(true)
                .build()
                .map_err(|e| e.to_string())?;
            Ok(Self::Glob(glob.compile_matcher()))
        } else {
            Ok(Self::Substring(pattern.to_lowercase()))
        }
    }

    fn matches(&self, name: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Substring(s) => name.to_lowercase().contains(s),
            Self::Glob(glob) => glob.is_match(name),
        }
    }
}

/// Walks `query.root`, calling `on_hit` for each match. Stops early when
/// `cancel` is set or after [`MAX_HITS`]. Blocking; run it off the UI thread.
pub fn run(
    query: &Query,
    cancel: &AtomicBool,
    mut on_hit: impl FnMut(Hit),
) -> Result<Summary, String> {
    let names = NameMatcher::new(&query.name)?;
    let content = query.content.trim();
    let matcher = (!content.is_empty())
        .then(|| {
            RegexMatcherBuilder::new()
                .fixed_strings(true)
                .case_smart(true)
                .build(content)
                .map_err(|e| e.to_string())
        })
        .transpose()?;
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .line_number(true)
        .build();

    let walker = ignore::WalkBuilder::new(&query.root)
        .hidden(!query.hidden)
        .git_ignore(query.ignore_files)
        .git_global(query.ignore_files)
        .git_exclude(query.ignore_files)
        .ignore(query.ignore_files)
        .parents(query.ignore_files)
        .require_git(false)
        .build();

    let mut summary = Summary::default();
    for item in walker {
        if cancel.load(Ordering::Relaxed) {
            summary.cancelled = true;
            break;
        }
        let Ok(item) = item else { continue };
        if item.depth() == 0 {
            continue; // the root itself
        }
        let path = item.path();
        let name = item.file_name().to_string_lossy();
        if !names.matches(&name) {
            continue;
        }
        let is_dir = item.file_type().is_some_and(|t| t.is_dir());
        let line = match &matcher {
            None => None,
            Some(_) if is_dir => continue,
            Some(matcher) => match first_match(&mut searcher, matcher, path) {
                Some(line) => Some(line),
                None => continue,
            },
        };
        let meta = item.metadata().ok();
        on_hit(Hit {
            entry: Entry {
                name: relative_name(&query.root, path),
                path: path.to_path_buf(),
                is_dir,
                is_parent: false,
                size: meta.as_ref().map_or(0, |m| m.len()),
                modified: meta.and_then(|m| m.modified().ok()),
            },
            line,
        });
        summary.hits += 1;
        if summary.hits >= MAX_HITS {
            summary.truncated = true;
            break;
        }
    }
    Ok(summary)
}

fn first_match(
    searcher: &mut Searcher,
    matcher: &grep_regex::RegexMatcher,
    path: &Path,
) -> Option<(u64, String)> {
    let mut found = None;
    let _ = searcher.search_path(
        matcher,
        path,
        sinks::Lossy(|line_number, text| {
            found = Some((line_number, text.trim_end().to_string()));
            Ok(false) // stop at the first match
        }),
    );
    found
}

/// Path relative to the search root, with `/` separators, as shown.
fn relative_name(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("fyle-search-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (path, text) in [
            ("src/main.rs", "fn main() {\n    println!(\"Hello\");\n}\n"),
            ("src/lib.rs", "pub fn hello() {}\n"),
            ("docs/notes.md", "hello world\n"),
            ("target/debug/out.rs", "fn hello() {}\n"),
            (".hidden/secret.rs", "hello\n"),
            ("image.bin", "\0hello\0"),
            (".gitignore", "target/\n"),
        ] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        root
    }

    fn names(query: &Query) -> Vec<String> {
        let mut names = Vec::new();
        run(query, &AtomicBool::new(false), |hit| {
            names.push(hit.entry.name)
        })
        .unwrap();
        names.sort();
        names
    }

    fn query(root: &Path, name: &str, content: &str) -> Query {
        Query {
            root: root.to_path_buf(),
            name: name.into(),
            content: content.into(),
            hidden: false,
            ignore_files: true,
        }
    }

    #[test]
    fn finds_by_name_respecting_gitignore_and_hidden() {
        let root = tree("names");
        assert_eq!(
            names(&query(&root, "*.rs", "")),
            ["src/lib.rs", "src/main.rs"]
        );
        assert_eq!(names(&query(&root, "MAIN", "")), ["src/main.rs"]);
        assert_eq!(names(&query(&root, "src", "")), ["src"]); // directories too

        let mut all = query(&root, "*.rs", "");
        all.hidden = true;
        all.ignore_files = false;
        assert_eq!(
            names(&all),
            [
                ".hidden/secret.rs",
                "src/lib.rs",
                "src/main.rs",
                "target/debug/out.rs"
            ]
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn searches_contents_with_smart_case() {
        let root = tree("content");
        let mut hits = Vec::new();
        run(&query(&root, "", "hello"), &AtomicBool::new(false), |h| {
            hits.push(h)
        })
        .unwrap();
        hits.sort_by(|a, b| a.entry.name.cmp(&b.entry.name));
        let found: Vec<_> = hits
            .iter()
            .map(|h| (h.entry.name.as_str(), h.line.clone().unwrap().0))
            .collect();
        // Binary files are skipped; "Hello" matches case-insensitively.
        assert_eq!(
            found,
            [("docs/notes.md", 1), ("src/lib.rs", 1), ("src/main.rs", 2)]
        );
        assert_eq!(hits[2].line.as_ref().unwrap().1, "    println!(\"Hello\");");

        // Capitals make it case-sensitive; name and content combine.
        assert_eq!(names(&query(&root, "*.rs", "Hello")), ["src/main.rs"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cancels_and_reports_bad_patterns() {
        let root = tree("cancel");
        let summary = run(&query(&root, "", ""), &AtomicBool::new(true), |_| {}).unwrap();
        assert!(summary.cancelled);
        assert_eq!(summary.hits, 0);
        assert!(run(&query(&root, "[", ""), &AtomicBool::new(false), |_| {}).is_err());
        fs::remove_dir_all(&root).unwrap();
    }
}
