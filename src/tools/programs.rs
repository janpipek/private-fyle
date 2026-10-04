//! Finding external programs on `PATH`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// An external program a tool needs, e.g. ImageMagick as `magick` or the
/// older `convert`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    /// Name used in command templates (`{magick}`).
    pub alias: String,
    /// Executable names to try, in order.
    pub candidates: Vec<String>,
    /// Shown when missing, e.g. "ImageMagick".
    pub hint: String,
}

impl Requirement {
    /// Parses `"magick|convert"` (alias = first candidate) with an optional
    /// hint: `"magick|convert (ImageMagick)"`.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let (names, hint) = match spec.split_once('(') {
            Some((names, hint)) => (names, hint.trim_end_matches(')').trim()),
            None => (spec, ""),
        };
        let candidates: Vec<String> = names
            .split('|')
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(String::from)
            .collect();
        let alias = candidates
            .first()
            .cloned()
            .ok_or_else(|| format!("empty program requirement {spec:?}"))?;
        Ok(Self {
            hint: if hint.is_empty() {
                alias.clone()
            } else {
                hint.to_string()
            },
            alias,
            candidates,
        })
    }
}

/// Resolved requirements: alias → executable path.
pub type Resolved = HashMap<String, PathBuf>;

/// Looks programs up in a search path, caching results.
pub struct Programs {
    search: Vec<PathBuf>,
    cache: Mutex<HashMap<String, Option<PathBuf>>>,
}

impl Programs {
    /// Uses the `PATH` environment variable.
    pub fn from_env() -> Self {
        let search = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        Self::new(search)
    }

    pub fn new(search: Vec<PathBuf>) -> Self {
        Self {
            search,
            cache: Mutex::default(),
        }
    }

    pub fn find(&self, name: &str) -> Option<PathBuf> {
        let mut cache = self.cache.lock().unwrap();
        cache
            .entry(name.to_string())
            .or_insert_with(|| find_in(&self.search, name))
            .clone()
    }

    /// Resolves every requirement, or returns the hints of the missing ones.
    pub fn resolve(&self, requirements: &[Requirement]) -> Result<Resolved, Vec<String>> {
        let mut resolved = Resolved::new();
        let mut missing = Vec::new();
        for req in requirements {
            match req.candidates.iter().find_map(|c| self.find(c)) {
                Some(path) => {
                    resolved.insert(req.alias.clone(), path);
                }
                None => missing.push(req.hint.clone()),
            }
        }
        if missing.is_empty() {
            Ok(resolved)
        } else {
            Err(missing)
        }
    }
}

fn find_in(search: &[PathBuf], name: &str) -> Option<PathBuf> {
    search
        .iter()
        .find_map(|dir| candidates(dir, name).into_iter().find(|p| is_executable(p)))
}

#[cfg(windows)]
fn candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    std::iter::once(dir.join(name))
        .chain(
            exts.split(';')
                .filter(|e| !e.is_empty())
                .map(|e| dir.join(format!("{name}{e}"))),
        )
        .collect()
}

#[cfg(not(windows))]
fn candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    vec![dir.join(name)]
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = path.metadata() else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}
