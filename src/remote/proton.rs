//! Proton Drive through the official `proton-drive` CLI and its JSON output.
//!
//! Every command is run as `proton-drive filesystem <command> <options>
//! --json -- <positionals>`. The `--` makes node's `parseArgs` treat all
//! later words as positionals, so no name can be read as an option; the CLI
//! drops it before counting arguments (it keeps the positionals after the
//! first four), so this is safe. Proton Docs and Sheets are listed like
//! files, but the CLI skips them on download.

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;

use super::process::Program;
use super::{
    Capabilities, Control, Item, Listing, Location, Provider, Removal, entries, percent_encode,
    timestamp,
};

const LIST_TIMEOUT: Duration = Duration::from_secs(120);
const QUICK_TIMEOUT: Duration = Duration::from_secs(120);
const SIGNED_OUT: &str = "You need to login first";

/// Proton Drive, using the authenticated `proton-drive` program from PATH.
/// Panel root `proton:/` is the CLI's `/my-files`.
#[derive(Clone, Debug)]
pub struct Proton {
    program: Program,
    /// The `deprecatedShareId` of `/my-files`, learnt by the first web
    /// address that needed it; clones share it.
    root_share: Arc<Mutex<Option<String>>>,
}

impl Default for Proton {
    fn default() -> Self {
        Self {
            program: Program {
                program: "proton-drive".into(),
                service: "Proton Drive",
                install_hint: "install the official Proton Drive CLI (proton-drive), then run proton-drive auth login",
                failure_hint: "",
            },
            root_share: Arc::default(),
        }
    }
}

impl Proton {
    /// Whether `proton-drive` is on PATH.
    pub fn is_installed(&self) -> bool {
        self.program.is_installed()
    }

    /// Runs `filesystem <command> <options> --json -- <positionals>`.
    fn run(
        &self,
        command: &str,
        options: &[&OsStr],
        positionals: &[&OsStr],
        cancel: &AtomicBool,
        timeout: Option<Duration>,
    ) -> io::Result<Vec<u8>> {
        let mut args = vec![OsStr::new("filesystem"), OsStr::new(command)];
        args.extend_from_slice(options);
        args.extend([OsStr::new("--json"), OsStr::new("--")]);
        args.extend_from_slice(positionals);
        self.program
            .run(&args, cancel, timeout)
            .map_err(signed_out_hint)
    }
}

impl Proton {
    /// `filesystem info <cliPath>`: one node.
    fn node_info(&self, cli: &str, control: &Control) -> io::Result<Info> {
        let bytes = self.run(
            "info",
            &[],
            &[OsStr::new(cli)],
            control.cancel,
            Some(QUICK_TIMEOUT),
        )?;
        serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// The share ID of `/my-files`, cached after the first success; `None`
    /// when it cannot be learnt (the caller then falls back).
    fn root_share(&self, control: &Control) -> Option<String> {
        let cached = self.root_share.lock().expect("share cache lock").clone();
        if cached.is_some() {
            return cached;
        }
        let share = self
            .node_info("/my-files", control)
            .ok()?
            .deprecated_share_id?;
        *self.root_share.lock().expect("share cache lock") = Some(share.clone());
        Some(share)
    }
}

/// Replaces the CLI's "signed out" message with what to do about it.
fn signed_out_hint(error: io::Error) -> io::Error {
    if error.to_string().contains(SIGNED_OUT) {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Proton Drive: signed out; run proton-drive auth login",
        )
    } else {
        error
    }
}

/// The CLI path for a panel location.
fn cli_path(location: &Location) -> String {
    match location.relative().as_str() {
        "" => "/my-files".into(),
        relative => format!("/my-files/{relative}"),
    }
}

/// Rejects relative local paths and the root, which the CLI refuses.
fn absolute(path: &Path) -> io::Result<&OsStr> {
    if path.is_absolute() && path.parent().is_some() {
        Ok(path.as_os_str())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{}: local path must be absolute", path.display()),
        ))
    }
}

/// Options so that nothing is ever overwritten: conflicting items are skipped.
const SKIP: [&str; 4] = [
    "--file-conflict-strategy",
    "skip",
    "--folder-conflict-strategy",
    "skip",
];

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
            create_folder: true,
            rename: true,
            move_within: true,
            web: true,
        }
    }

    fn list(&self, dir: &Location, control: &Control) -> io::Result<Listing> {
        let cli = cli_path(dir);
        let bytes = self.run(
            "list",
            &[],
            &[OsStr::new(&cli)],
            control.cancel,
            Some(LIST_TIMEOUT),
        )?;
        parse_listing(dir, &bytes)
    }

    fn download(
        &self,
        source: &Location,
        _is_dir: bool,
        destination: &Path,
        control: &Control,
    ) -> io::Result<()> {
        let cli = cli_path(source);
        let destination = absolute(destination)?;
        // The CLI creates a missing destination folder and refuses `/`.
        (control.progress)(source.name().unwrap_or("Proton Drive"), 0);
        let options = SKIP.map(OsStr::new);
        let bytes = self.run(
            "download",
            &options,
            &[OsStr::new(&cli), destination],
            control.cancel,
            None,
        )?;
        transfer_result(&bytes)
    }

    fn upload(&self, source: &Path, destination: &Location, control: &Control) -> io::Result<()> {
        let cli = cli_path(destination);
        let source = absolute(source)?;
        let name = source.to_str().and_then(|s| s.rsplit('/').next());
        (control.progress)(name.unwrap_or("upload"), 0);
        let options = SKIP.map(OsStr::new);
        let bytes = self.run(
            "upload",
            &options,
            &[source, OsStr::new(&cli)],
            control.cancel,
            None,
        )?;
        transfer_result(&bytes)
    }

    fn remove(&self, target: &Location, _is_dir: bool, control: &Control) -> io::Result<()> {
        let cli = cli_path(target);
        non_root(target)?;
        (control.progress)(target.name().unwrap_or_default(), 0);
        let bytes = self.run(
            "trash",
            &[],
            &[OsStr::new(&cli)],
            control.cancel,
            Some(QUICK_TIMEOUT),
        )?;
        item_results(&bytes, "trash")
    }

    fn create_folder(&self, parent: &Location, name: &str, control: &Control) -> io::Result<()> {
        parent.child(name)?;
        let cli = cli_path(parent);
        let bytes = self.run(
            "create-folder",
            &[],
            &[OsStr::new(&cli), OsStr::new(name)],
            control.cancel,
            Some(QUICK_TIMEOUT),
        )?;
        node_result(&bytes, "create the folder")
    }

    fn rename(&self, target: &Location, new_name: &str, control: &Control) -> io::Result<()> {
        non_root(target)?;
        target.parent().map(|p| p.child(new_name)).transpose()?;
        let cli = cli_path(target);
        let bytes = self.run(
            "rename",
            &[],
            &[OsStr::new(&cli), OsStr::new(new_name)],
            control.cancel,
            Some(QUICK_TIMEOUT),
        )?;
        node_result(&bytes, "rename the item")
    }

    fn move_to(
        &self,
        target: &Location,
        destination: &Location,
        control: &Control,
    ) -> io::Result<()> {
        non_root(target)?;
        let (source, folder) = (cli_path(target), cli_path(destination));
        (control.progress)(target.name().unwrap_or_default(), 0);
        let bytes = self.run(
            "move",
            &[],
            &[OsStr::new(&source), OsStr::new(&folder)],
            control.cancel,
            None,
        )?;
        item_results(&bytes, "move")
    }

    fn web_url(&self, target: &Location, control: &Control) -> io::Result<String> {
        if target.name().is_none() {
            return Ok(WEB_DRIVE.into());
        }
        let info = self.node_info(&cli_path(target), control)?;
        let mut share = info.deprecated_share_id.clone();
        if share.is_none() && needs_share(&info) {
            share = self.root_share(control);
        }
        web_url_for(&info, share.as_deref())
    }
}

fn non_root(location: &Location) -> io::Result<()> {
    if location.name().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot change the Proton Drive root",
        ));
    }
    Ok(())
}

/// A node as `filesystem list` prints it (the SDK's `NodeEntity` through
/// `JSON.stringify`); only the fields used here.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    name: Name,
    #[serde(rename = "type")]
    kind: String,
    modification_time: Option<String>,
    creation_time: Option<String>,
    active_revision: Option<Revision>,
    folder: Option<FolderInfo>,
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
    storage_size: Option<u64>,
    claimed_modification_time: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FolderInfo {
    claimed_modification_time: Option<String>,
}

const WEB_DRIVE: &str = "https://drive.proton.me/";
const WEB_DOCS: &str = "https://docs.proton.me";
const DOC_TYPE: &str = "application/vnd.proton.doc";
const SHEET_TYPE: &str = "application/vnd.proton.sheet";

/// A node as `filesystem info` prints it (one SDK `NodeEntity`); only the
/// fields needed for a web address.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Info {
    /// `<volumeId>~<linkId>`.
    uid: String,
    #[serde(rename = "type")]
    kind: String,
    media_type: Option<String>,
    deprecated_share_id: Option<String>,
}

/// Docs and Sheets open in their own app and need no share ID.
fn is_proton_document(info: &Info) -> bool {
    matches!(info.media_type.as_deref(), Some(DOC_TYPE | SHEET_TYPE))
}

fn needs_share(info: &Info) -> bool {
    !is_proton_document(info)
}

/// The private web address of a node, which needs the user's own login.
///
/// Docs and Sheets use `docs.proton.me/{doc,sheet}?mode=open&volumeId=..&linkId=..`,
/// verified in Proton's web client source. Other items use
/// `drive.proton.me/<shareId>/{file,folder}/<linkId>`: the URL shape was
/// verified there too, but that the CLI's `deprecatedShareId` (the node's
/// own, else the one of `/my-files`) opens exactly this item is UNVERIFIED.
/// Without any share ID it falls back to the Drive start page.
fn web_url_for(info: &Info, share: Option<&str>) -> io::Result<String> {
    let (volume, link) = info
        .uid
        .split_once('~')
        .filter(|(volume, link)| !volume.is_empty() && !link.is_empty())
        .ok_or_else(|| io::Error::other("Proton Drive returned an unexpected item ID"))?;
    let (volume, link) = (percent_encode(volume), percent_encode(link));
    let app = match info.media_type.as_deref() {
        Some(DOC_TYPE) => Some("doc"),
        Some(SHEET_TYPE) => Some("sheet"),
        _ => None,
    };
    if let Some(app) = app {
        return Ok(format!(
            "{WEB_DOCS}/{app}?mode=open&volumeId={volume}&linkId={link}"
        ));
    }
    let Some(share) = share else {
        return Ok(WEB_DRIVE.into());
    };
    let kind = if info.kind == "folder" {
        "folder"
    } else {
        "file"
    };
    Ok(format!(
        "{WEB_DRIVE}{}/{kind}/{link}",
        percent_encode(share)
    ))
}

/// Decodes the CLI's JSON array of nodes; rejects undecryptable names (the
/// rest of the folder could not be trusted); duplicate and unsafe names
/// become warnings.
fn parse_listing(dir: &Location, bytes: &[u8]) -> io::Result<Listing> {
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
            let claimed = if is_dir {
                node.folder
                    .as_ref()
                    .and_then(|f| f.claimed_modification_time.as_deref())
            } else {
                revision.and_then(|r| r.claimed_modification_time.as_deref())
            };
            Ok(Item {
                name,
                is_dir,
                size: revision.and_then(|r| r.claimed_size.or(r.storage_size)),
                modified: timestamp(claimed.or(node.modification_time.as_deref())),
                created: timestamp(node.creation_time.as_deref()),
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(entries(dir, items))
}

/// Checks the summary printed by `download` and `upload`:
/// `{transferredItems, transferredBytes, skippedItems, failedItems,
/// failures: [{name, nodeUid, error}]}`.
fn transfer_result(bytes: &[u8]) -> io::Result<()> {
    let summary: serde_json::Value = serde_json::from_slice(bytes).map_err(io::Error::other)?;
    let failed = summary
        .get("failedItems")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| io::Error::other("Proton Drive printed no transfer summary"))?;
    let failures = summary
        .get("failures")
        .and_then(|v| v.as_array())
        .map(Vec::as_slice)
        .unwrap_or_default();
    if failed > 0 || !failures.is_empty() {
        let text = |f: &serde_json::Value, key: &str| {
            f.get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_owned()
        };
        let list: Vec<String> = failures
            .iter()
            .take(5)
            .map(|f| format!("{}: {}", text(f, "name"), text(f, "error")))
            .collect();
        return Err(io::Error::other(format!(
            "Proton Drive could not transfer {failed} item(s): {}",
            list.join("; ")
        )));
    }
    Ok(())
}

/// Checks the `[{uid, ok, error?}]` array printed by `trash` and `move`.
fn item_results(bytes: &[u8], action: &str) -> io::Result<()> {
    let results: Vec<serde_json::Value> =
        serde_json::from_slice(bytes).map_err(io::Error::other)?;
    let failed = results
        .iter()
        .find(|r| r.get("ok").and_then(|v| v.as_bool()) != Some(true));
    if results.is_empty() || failed.is_some() {
        let detail = failed.and_then(|r| r.get("error")).map_or_else(
            || String::from_utf8_lossy(bytes).into_owned(),
            |e| e.to_string(),
        );
        return Err(io::Error::other(format!(
            "Proton Drive could not {action} the item: {detail}"
        )));
    }
    Ok(())
}

/// Checks the node object printed by `create-folder` and `rename`.
fn node_result(bytes: &[u8], action: &str) -> io::Result<()> {
    let node: serde_json::Value = serde_json::from_slice(bytes).map_err(io::Error::other)?;
    if node.get("uid").and_then(|v| v.as_str()).is_none() {
        return Err(io::Error::other(format!(
            "Proton Drive did not {action}: {}",
            String::from_utf8_lossy(bytes)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Proton, absolute, cli_path, item_results, node_result, parse_listing, signed_out_hint,
        transfer_result,
    };
    use crate::remote::{Control, Location, Provider};
    use std::io;
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
            {"uid":"v~1","name":{"ok":true,"value":"Reports"},"type":"folder","creationTime":"2026-01-01T00:00:00.000Z","modificationTime":"2026-03-01T00:00:00.000Z","folder":{"claimedModificationTime":"2026-02-01T00:00:00.000Z","isImported":false}},
            {"uid":"v~2","name":{"ok":true,"value":"a.txt"},"type":"file","mediaType":"text/plain","creationTime":"2026-10-06T12:00:01.000Z","modificationTime":"2026-10-06T12:00:02.000Z","activeRevision":{"uid":"v~3","claimedSize":42,"storageSize":64,"claimedModificationTime":"2026-10-06T12:00:00.000Z"}},
            {"uid":"v~4","name":{"ok":true,"value":"Notes"},"type":"file","mediaType":"application/vnd.proton.doc","creationTime":"2026-10-06T12:00:01.000Z","modificationTime":"2026-10-06T12:00:02.000Z","activeRevision":{"storageSize":10}},
            {"uid":"v~5","name":{"ok":true,"value":"Unknown"},"type":"file"}
        ]"#,
        )
        .unwrap()
        .entries;
        assert!(entries[0].is_dir);
        let folder_time = crate::remote::timestamp(Some("2026-02-01T00:00:00Z"));
        assert_eq!(entries[0].modified, folder_time);
        assert_eq!(entries[1].size, Some(42));
        let file_time = crate::remote::timestamp(Some("2026-10-06T12:00:00Z"));
        assert_eq!(entries[1].modified, file_time);
        assert_eq!(entries[1].path, Path::new("proton:/a.txt"));
        // Docs are listed like files; storage size stands in for a missing claim.
        assert_eq!(entries[2].size, Some(10));
        assert!(!entries[2].is_dir);
        assert_eq!(entries[3].size, None);
    }

    #[test]
    fn rejects_degraded_names_and_partial_json() {
        for input in [
            r#"[{"name":{"ok":false,"error":{}},"type":"file"}]"#,
            r#"[{"name":{"ok":true,"value":"p"},"type":"photo"}]"#,
            r#"[{"name":{"ok":true,"value":"a"},"type":"file"},"#,
        ] {
            assert!(parse_listing(&root(), input.as_bytes()).is_err(), "{input}");
        }
    }

    #[test]
    fn duplicate_and_unsafe_names_become_warnings() {
        let listing = parse_listing(
            &root(),
            br#"[{"name":{"ok":true,"value":"a"},"type":"file"},{"name":{"ok":true,"value":"a"},"type":"file"},{"name":{"ok":true,"value":"../x"},"type":"file"}]"#,
        )
        .unwrap();
        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.warnings.len(), 2);
    }

    #[test]
    fn item_results_are_checked_even_when_process_succeeds() {
        assert!(item_results(br#"[{"uid":"a","ok":true}]"#, "trash").is_ok());
        let error = item_results(br#"[{"uid":"a","ok":false,"error":"nope"}]"#, "move");
        assert!(error.unwrap_err().to_string().contains("could not move"));
        assert!(item_results(b"[]", "trash").is_err());
    }

    #[test]
    fn node_results_need_a_uid() {
        assert!(node_result(br#"{"uid":"v~1","name":{"ok":true,"value":"x"}}"#, "x").is_ok());
        assert!(node_result(b"{}", "x").is_err());
    }

    #[test]
    fn transfer_summary_reports_failures() {
        let ok = br#"{"transferredItems":2,"transferredBytes":10,"skippedItems":1,"failedItems":0,"failures":[]}"#;
        assert!(transfer_result(ok).is_ok());
        let bad = br#"{"transferredItems":1,"transferredBytes":5,"skippedItems":0,"failedItems":1,"failures":[{"name":"a.txt","nodeUid":"v~1","error":"disk full"}]}"#;
        let message = transfer_result(bad).unwrap_err().to_string();
        assert!(message.contains("a.txt: disk full"), "{message}");
        assert!(transfer_result(b"{}").is_err());
        assert!(transfer_result(b"not json").is_err());
    }

    #[test]
    fn signed_out_gets_a_login_hint() {
        let error = signed_out_hint(io::Error::other("Proton Drive: You need to login first. "));
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("proton-drive auth login"));
        let other = signed_out_hint(io::Error::other("boom"));
        assert_eq!(other.to_string(), "boom");
    }

    #[test]
    fn bad_local_paths_and_names_are_rejected_before_running() {
        assert!(absolute(Path::new("/tmp/a")).is_ok());
        assert!(absolute(Path::new("--evil")).is_err());
        assert!(absolute(Path::new("/")).is_err());
        let cancel = AtomicBool::new(false);
        let control = Control::new(&cancel);
        let proton = Proton::default();
        let invalid = io::ErrorKind::InvalidInput;
        let error = proton.download(&root(), false, Path::new("rel"), &control);
        assert_eq!(error.unwrap_err().kind(), invalid);
        let error = proton.upload(Path::new("rel"), &root(), &control);
        assert_eq!(error.unwrap_err().kind(), invalid);
        assert_eq!(
            proton.remove(&root(), true, &control).unwrap_err().kind(),
            invalid
        );
        assert!(proton.create_folder(&root(), "a/b", &control).is_err());
        let file = root().child("a").unwrap();
        assert!(proton.rename(&file, "../x", &control).is_err());
    }
}
