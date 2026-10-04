//! Facet detection: cheap filesystem probes, no external programs.

use std::path::{Path, PathBuf};

use super::{Facet, FacetKind, Facts, ToolContext};

pub fn detect(ctx: &ToolContext) -> Facts {
    Facts {
        dir: facets(&ctx.dir, true),
        targets: ctx
            .targets
            .iter()
            .map(|t| {
                if t.is_dir {
                    facets(&t.path, false)
                } else {
                    Vec::new()
                }
            })
            .collect(),
    }
}

/// Facets of `dir`; with `walk_up`, also of its ancestors (the nearest of
/// each kind wins), so a file deep inside a repository sees the repository.
pub fn facets(dir: &Path, walk_up: bool) -> Vec<Facet> {
    let mut found: Vec<Facet> = Vec::new();
    let has = |found: &[Facet], kind| found.iter().any(|f: &Facet| f.kind() == kind);
    for dir in dir.ancestors().take(if walk_up { usize::MAX } else { 1 }) {
        if dir.as_os_str().is_empty() {
            break;
        }
        if !has(&found, FacetKind::GitRepo) && dir.join(".git").exists() {
            found.push(Facet::GitRepo {
                root: dir.to_path_buf(),
                web_url: git_origin(dir).as_deref().and_then(web_url),
            });
        }
        let pyproject = dir.join("pyproject.toml");
        if !has(&found, FacetKind::PythonProject) && pyproject.is_file() {
            found.push(Facet::PythonProject {
                root: dir.to_path_buf(),
            });
            let uses_uv = dir.join("uv.lock").is_file()
                || std::fs::read_to_string(&pyproject).is_ok_and(|t| t.contains("[tool.uv"));
            if uses_uv && !has(&found, FacetKind::UvProject) {
                found.push(Facet::UvProject {
                    root: dir.to_path_buf(),
                });
            }
        }
        if !has(&found, FacetKind::CargoProject) && dir.join("Cargo.toml").is_file() {
            found.push(Facet::CargoProject {
                root: dir.to_path_buf(),
            });
        }
        if !has(&found, FacetKind::NodeProject) && dir.join("package.json").is_file() {
            found.push(Facet::NodeProject {
                root: dir.to_path_buf(),
            });
        }
        if !has(&found, FacetKind::Justfile)
            && let Some(path) = ["justfile", "Justfile", ".justfile"]
                .iter()
                .map(|n| dir.join(n))
                .find(|p| p.is_file())
        {
            found.push(Facet::Justfile { path });
        }
    }
    found
}

/// URL of the `origin` remote (or the first remote) from the repository's
/// config file. Handles `.git` files of worktrees and submodules.
fn git_origin(root: &Path) -> Option<String> {
    let dot_git = root.join(".git");
    let git_dir = if dot_git.is_file() {
        let text = std::fs::read_to_string(&dot_git).ok()?;
        let dir = PathBuf::from(text.strip_prefix("gitdir:")?.trim());
        if dir.is_absolute() {
            dir
        } else {
            root.join(dir)
        }
    } else {
        dot_git
    };
    let config = std::fs::read_to_string(git_dir.join("config"))
        .or_else(|_| std::fs::read_to_string(git_dir.join("../../config")))
        .ok()?;
    remote_url(&config)
}

/// The url of `[remote "origin"]`, else of the first remote.
pub fn remote_url(config: &str) -> Option<String> {
    let mut section = String::new();
    let mut first = None;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line.to_string();
            continue;
        }
        let Some(url) = line
            .strip_prefix("url")
            .map(str::trim_start)
            .and_then(|l| l.strip_prefix('='))
            .map(str::trim)
        else {
            continue;
        };
        if section == r#"[remote "origin"]"# {
            return Some(url.to_string());
        }
        if section.starts_with("[remote ") && first.is_none() {
            first = Some(url.to_string());
        }
    }
    first
}

/// Browser URL for a remote: `git@github.com:o/r.git`,
/// `ssh://git@host:22/o/r` and `https://user@host/o/r.git` all become
/// `https://host/o/r`. Local paths give `None`.
pub fn web_url(remote: &str) -> Option<String> {
    let (host, path) = if let Some(rest) = remote
        .strip_prefix("https://")
        .or_else(|| remote.strip_prefix("http://"))
        .or_else(|| remote.strip_prefix("ssh://"))
        .or_else(|| remote.strip_prefix("git://"))
    {
        let (authority, path) = rest.split_once('/')?;
        let host = authority.rsplit('@').next()?;
        let host = host.split(':').next()?; // drop port
        (host, path)
    } else {
        // scp-like: [user@]host:path
        let (authority, path) = remote.split_once(':')?;
        if authority.contains('/') || authority.len() == 1 {
            return None; // a local path or a Windows drive letter
        }
        (authority.rsplit('@').next()?, path)
    };
    let path = path.trim_matches('/').trim_end_matches(".git");
    (!host.is_empty() && !path.is_empty()).then(|| format!("https://{host}/{path}"))
}
