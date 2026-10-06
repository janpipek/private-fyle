//! Proton Drive through the official `proton-drive` CLI and its JSON output.

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::Deserialize;

use super::process::Program;
use super::{Capabilities, Entry, Item, Location, Provider, Removal, entries, timestamp};

const LIST_TIMEOUT: Duration = Duration::from_secs(120);

/// Proton Drive, using the authenticated `proton-drive` program from PATH.
/// Panel root `proton:/` is the CLI's `/my-files`.
#[derive(Clone, Debug)]
pub struct Proton {
    program: Program,
}

impl Default for Proton {
    fn default() -> Self {
        Self {
            program: Program {
                program: "proton-drive".into(),
                service: "Proton Drive",
                install_hint: "install the official Proton Drive CLI (proton-drive), then run proton-drive auth login",
                failure_hint: "If signed out, run proton-drive auth login",
            },
        }
    }
}

impl Proton {
    /// Whether `proton-drive` is on PATH.
    pub fn is_installed(&self) -> bool {
        self.program.is_installed()
    }

    fn run(
        &self,
        args: &[&OsStr],
        cancel: &AtomicBool,
        timeout: Option<Duration>,
    ) -> io::Result<Vec<u8>> {
        let mut all = args.to_vec();
        all.push(OsStr::new("--json"));
        self.program.run(&all, cancel, timeout)
    }
}

/// The CLI path for a panel location.
fn cli_path(location: &Location) -> String {
    match location.relative().as_str() {
        "" => "/my-files".into(),
        relative => format!("/my-files/{relative}"),
    }
}

/// Rejects relative local paths, which the CLI could read as options.
fn absolute(path: &Path) -> io::Result<&OsStr> {
    if path.is_absolute() {
        Ok(path.as_os_str())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{}: local path must be absolute", path.display()),
        ))
    }
}

impl Provider for Proton {
    fn name(&self) -> &str {
        "proton"
    }

    fn label(&self) -> &str {
        "Proton Drive"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            upload: true,
            removal: Some(Removal::Trash),
        }
    }

    fn list(&self, dir: &Location, cancel: &AtomicBool) -> io::Result<Vec<Entry>> {
        let cli = cli_path(dir);
        let args = ["filesystem", "list", cli.as_str()].map(OsStr::new);
        let bytes = self.run(&args, cancel, Some(LIST_TIMEOUT))?;
        parse_listing(dir, &bytes)
    }

    fn download(
        &self,
        source: &Location,
        _is_dir: bool,
        destination: &Path,
        cancel: &AtomicBool,
    ) -> io::Result<()> {
        let cli = cli_path(source);
        let args = [
            OsStr::new("filesystem"),
            OsStr::new("download"),
            OsStr::new(&cli),
            absolute(destination)?,
            OsStr::new("--file-conflict-strategy"),
            OsStr::new("skip"),
            OsStr::new("--folder-conflict-strategy"),
            OsStr::new("skip"),
        ];
        transfer_result(&self.run(&args, cancel, None)?)
    }

    fn upload(&self, source: &Path, destination: &Location, cancel: &AtomicBool) -> io::Result<()> {
        let cli = cli_path(destination);
        let args = [
            OsStr::new("filesystem"),
            OsStr::new("upload"),
            absolute(source)?,
            OsStr::new(&cli),
            OsStr::new("--file-conflict-strategy"),
            OsStr::new("skip"),
            OsStr::new("--folder-conflict-strategy"),
            OsStr::new("skip"),
        ];
        transfer_result(&self.run(&args, cancel, None)?)
    }

    fn remove(&self, target: &Location, _is_dir: bool, cancel: &AtomicBool) -> io::Result<()> {
        let cli = cli_path(target);
        let args = ["filesystem", "trash", cli.as_str()].map(OsStr::new);
        trash_result(&self.run(&args, cancel, None)?)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    name: Name,
    #[serde(rename = "type")]
    kind: String,
    modification_time: Option<String>,
    creation_time: Option<String>,
    active_revision: Option<Revision>,
}

#[derive(Deserialize)]
struct Name {
    ok: bool,
    value: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Revision {
    claimed_size: Option<u64>,
    claimed_modification_time: Option<String>,
}

/// Decodes the CLI's JSON array of nodes; rejects undecryptable, unsafe and
/// duplicate names.
fn parse_listing(dir: &Location, bytes: &[u8]) -> io::Result<Vec<Entry>> {
    let nodes: Vec<Node> =
        serde_json::from_slice(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let items = nodes
        .into_iter()
        .map(|node| {
            let name =
                node.name.value.filter(|_| node.name.ok).ok_or_else(|| {
                    io::Error::other("Proton Drive could not decrypt an entry name")
                })?;
            let is_dir = match node.kind.as_str() {
                "folder" => true,
                "file" => false,
                _ => return Err(io::Error::other("unsupported Proton Drive entry type")),
            };
            let revision = node.active_revision.as_ref();
            Ok(Item {
                name,
                is_dir,
                size: revision.and_then(|r| r.claimed_size).unwrap_or(0),
                modified: timestamp(
                    revision
                        .and_then(|r| r.claimed_modification_time.as_deref())
                        .or(node.modification_time.as_deref()),
                ),
                created: timestamp(node.creation_time.as_deref()),
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    entries(dir, "Proton Drive", items)
}

fn transfer_result(bytes: &[u8]) -> io::Result<()> {
    let summary: serde_json::Value = serde_json::from_slice(bytes).map_err(io::Error::other)?;
    if summary
        .get("failed")
        .and_then(|v| v.as_u64())
        .is_some_and(|v| v > 0)
        || summary
            .get("failures")
            .and_then(|v| v.as_array())
            .is_some_and(|v| !v.is_empty())
    {
        return Err(io::Error::other(format!(
            "Proton Drive transfer failed: {summary}"
        )));
    }
    Ok(())
}

fn trash_result(bytes: &[u8]) -> io::Result<()> {
    let results: Vec<serde_json::Value> =
        serde_json::from_slice(bytes).map_err(io::Error::other)?;
    if results.is_empty()
        || results
            .iter()
            .any(|r| r.get("ok").and_then(|v| v.as_bool()) != Some(true))
    {
        return Err(io::Error::other(format!(
            "Proton Drive could not trash the item: {}",
            String::from_utf8_lossy(bytes)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Proton, absolute, cli_path, parse_listing, transfer_result, trash_result};
    use crate::remote::{Location, Provider};
    use std::path::Path;
    use std::sync::atomic::AtomicBool;

    fn root() -> Location {
        Location::root("proton").unwrap()
    }

    #[test]
    fn maps_panel_paths_to_cli_paths() {
        assert_eq!(cli_path(&root()), "/my-files");
        let deep = root().child("a").unwrap().child("b c").unwrap();
        assert_eq!(cli_path(&deep), "/my-files/a/b c");
    }

    #[test]
    fn decodes_official_cli_metadata() {
        let entries = parse_listing(
            &root(),
            br#"[
            {"name":{"ok":true,"value":"Reports"},"type":"folder"},
            {"name":{"ok":true,"value":"a.txt"},"type":"file","activeRevision":{"claimedSize":42,"claimedModificationTime":"2026-10-06T12:00:00Z"}}
        ]"#,
        )
        .unwrap();
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].size, 42);
        assert!(entries[1].modified.is_some());
        assert_eq!(entries[1].path, Path::new("proton:/a.txt"));
    }

    #[test]
    fn rejects_degraded_and_unsafe_names_and_partial_json() {
        for input in [
            r#"[{"name":{"ok":false},"type":"file"}]"#,
            r#"[{"name":{"ok":true,"value":"../escape"},"type":"file"}]"#,
            r#"[{"name":{"ok":true,"value":"a"},"type":"file"},{"name":{"ok":true,"value":"a"},"type":"file"}]"#,
            r#"[{"name":{"ok":true,"value":"a"},"type":"file"},"#,
        ] {
            assert!(parse_listing(&root(), input.as_bytes()).is_err(), "{input}");
        }
    }

    #[test]
    fn trash_checks_item_results_even_when_process_succeeds() {
        assert!(trash_result(br#"[{"uid":"a","ok":true}]"#).is_ok());
        assert!(trash_result(br#"[{"uid":"a","ok":false,"error":{}}]"#).is_err());
        assert!(trash_result(b"[]").is_err());
    }

    #[test]
    fn transfer_reports_failures() {
        assert!(transfer_result(br#"{"failed":0}"#).is_ok());
        assert!(transfer_result(br#"{"failed":2}"#).is_err());
        assert!(transfer_result(br#"{"failures":["x"]}"#).is_err());
    }

    #[test]
    fn relative_local_paths_are_rejected_before_running() {
        assert!(absolute(Path::new("/tmp/a")).is_ok());
        assert!(absolute(Path::new("--evil")).is_err());
        let cancel = AtomicBool::new(false);
        let error = Proton::default()
            .download(&root(), false, Path::new("rel"), &cancel)
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        let error = Proton::default()
            .upload(Path::new("rel"), &root(), &cancel)
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
}
