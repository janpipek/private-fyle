//! Local filesystem and symlink facts for Properties.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use crate::entry::Attrs;

/// Facts about a path obtained from the local filesystem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatFacts {
    /// Whether this filesystem object is a directory.
    pub is_dir: bool,
    /// Whether this filesystem object is a regular file.
    pub is_file: bool,
    /// Byte length for regular files.
    pub size: Option<u64>,
    /// Last modification time.
    pub modified: Option<SystemTime>,
    /// Creation time, where supported.
    pub created: Option<SystemTime>,
    /// Platform attributes.
    pub attrs: Attrs,
    /// Unix owner name, when available in the local account database.
    pub owner_name: Option<String>,
    /// Unix group ID.
    pub gid: Option<u32>,
    /// Unix group name, when available in the local account database.
    pub group_name: Option<String>,
}

/// Local filesystem facts distinguish the link object from its target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilesystemFacts {
    /// Metadata for the directory entry itself, without following a symlink.
    pub link_object: StatFacts,
    /// Stored symlink destination, if the entry is a symlink.
    pub symlink_target: Option<std::path::PathBuf>,
    /// Followed target metadata; absent for a broken or unreadable target.
    pub target: Option<StatFacts>,
    /// Target lookup failure, if following a symlink failed.
    pub target_error: Option<String>,
}

/// Reads local `lstat` details and, for symlinks, stored and followed target facts.
pub(super) fn read(path: &Path, cancel: &AtomicBool) -> io::Result<FilesystemFacts> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
    }
    let link_object = fs::symlink_metadata(path)?;
    let symlink = link_object.file_type().is_symlink();
    let (target_path, target, target_error) = if symlink {
        match fs::read_link(path) {
            Err(error) => (None, None, Some(error.to_string())),
            Ok(target_path) => match fs::metadata(path) {
                Ok(meta) => (Some(target_path), Some(stat_facts(&meta)), None),
                Err(error) => (Some(target_path), None, Some(error.to_string())),
            },
        }
    } else {
        (None, None, None)
    };
    Ok(FilesystemFacts {
        link_object: stat_facts(&link_object),
        symlink_target: target_path,
        target,
        target_error,
    })
}

#[cfg(unix)]
fn stat_facts(meta: &fs::Metadata) -> StatFacts {
    use std::os::unix::fs::MetadataExt;
    let (owner_name, group_name) = crate::fs_ops::account_names(meta.uid(), meta.gid());
    let attrs = Attrs {
        mode: Some(meta.mode()),
        uid: Some(meta.uid()),
        readonly: meta.permissions().readonly(),
        created: meta.created().ok(),
    };
    StatFacts {
        is_dir: meta.is_dir(),
        is_file: meta.is_file(),
        size: meta.is_file().then_some(meta.len()),
        modified: meta.modified().ok(),
        created: meta.created().ok(),
        attrs,
        owner_name,
        gid: Some(meta.gid()),
        group_name,
    }
}

#[cfg(windows)]
fn stat_facts(meta: &fs::Metadata) -> StatFacts {
    let attrs = Attrs {
        mode: None,
        uid: None,
        readonly: meta.permissions().readonly(),
        created: meta.created().ok(),
    };
    StatFacts {
        is_dir: meta.is_dir(),
        is_file: meta.is_file(),
        size: meta.is_file().then_some(meta.len()),
        modified: meta.modified().ok(),
        created: meta.created().ok(),
        attrs,
        owner_name: None,
        gid: None,
        group_name: None,
    }
}
