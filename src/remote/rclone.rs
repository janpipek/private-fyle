//! Remotes configured in rclone (Google Drive, SFTP, S3, ...). Credentials
//! stay in rclone's own configuration.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::Deserialize;

use super::process::Program;
use super::{Capabilities, Control, Entry, Item, Location, Provider, Removal, entries, timestamp};

const LIST_TIMEOUT: Duration = Duration::from_secs(120);
const REMOTES_TIMEOUT: Duration = Duration::from_secs(30);

fn program() -> Program {
    Program {
        program: "rclone".into(),
        service: "rclone",
        install_hint: "install rclone and set up a remote with rclone config",
        failure_hint: "Check the remote with rclone config",
    }
}

/// One rclone remote, shown as `<remote>:/...`.
#[derive(Clone, Debug)]
pub struct Rclone {
    program: Program,
    remote: String,
    backend: String,
    label: String,
}

impl Rclone {
    /// Whether `rclone` is on PATH.
    pub fn is_installed() -> bool {
        program().is_installed()
    }

    /// The remotes from `rclone listremotes --long`, except those that
    /// cannot be panel paths and Proton Drive (the official CLI serves it).
    pub fn remotes(cancel: &AtomicBool) -> io::Result<Vec<Rclone>> {
        let program = program();
        let args = ["listremotes", "--long"].map(OsStr::new);
        let bytes = program.run(&args, cancel, Some(REMOTES_TIMEOUT))?;
        Ok(parse_remotes(&String::from_utf8_lossy(&bytes))
            .into_iter()
            .filter(|(name, backend)| backend != "protondrive" && Location::root(name).is_ok())
            .map(|(remote, backend)| Rclone {
                program: program.clone(),
                label: format!("{} ({remote})", backend_label(&backend)),
                remote,
                backend,
            })
            .collect())
    }

    fn path(&self, location: &Location) -> String {
        format!("{}:{}", self.remote, location.relative())
    }

    /// `rclone moveto` (it overwrites, so the target must not exist).
    fn move_to_path(&self, from: &Location, to: &Location, control: &Control) -> io::Result<()> {
        let parent = to.parent().ok_or_else(root_error)?;
        let name = to.name().ok_or_else(root_error)?;
        if self
            .list(&parent, control)?
            .iter()
            .any(|entry| entry.name == name)
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{name} already exists"),
            ));
        }
        let args = vec![
            "moveto".into(),
            self.path(from).into(),
            self.path(to).into(),
        ];
        self.run(&args, control.cancel, None).map(drop)
    }

    fn run(
        &self,
        args: &[OsString],
        cancel: &AtomicBool,
        timeout: Option<Duration>,
    ) -> io::Result<Vec<u8>> {
        let args: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
        self.program.run(&args, cancel, timeout)
    }
}

/// Parses `rclone listremotes --long` lines such as `gdrive:   drive` into
/// (name, backend) pairs.
fn parse_remotes(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let (name, backend) = line.split_once(':')?;
            let name = name.trim();
            (!name.is_empty()).then(|| (name.to_owned(), backend.trim().to_owned()))
        })
        .collect()
}

fn root_error() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "cannot change a remote root")
}

fn backend_label(backend: &str) -> &str {
    match backend {
        "drive" => "Google Drive",
        "s3" => "Amazon S3",
        "sftp" => "SFTP",
        "ftp" => "FTP",
        "onedrive" => "OneDrive",
        "dropbox" => "Dropbox",
        other => other,
    }
}

fn absolute(path: &Path) -> io::Result<&Path> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{}: local path must be absolute", path.display()),
        ))
    }
}

/// Arguments copying `source` to `destination` without overwriting. A file
/// uses `copyto` (destination is the file), a folder `copy` (destination is
/// the folder).
fn transfer_args(source: OsString, destination: OsString, is_dir: bool) -> Vec<OsString> {
    vec![
        if is_dir { "copy" } else { "copyto" }.into(),
        source,
        destination,
        "--ignore-existing".into(),
    ]
}

fn remove_args(path: String, is_dir: bool) -> Vec<OsString> {
    vec![
        if is_dir { "purge" } else { "deletefile" }.into(),
        path.into(),
    ]
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Listed {
    name: String,
    #[serde(default)]
    size: i64,
    mod_time: Option<String>,
    #[serde(default)]
    is_dir: bool,
}

/// Decodes `rclone lsjson` output; rejects unsafe and duplicate names.
fn parse_listing(dir: &Location, label: &str, bytes: &[u8]) -> io::Result<Vec<Entry>> {
    let listed: Vec<Listed> =
        serde_json::from_slice(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let items = listed
        .into_iter()
        .map(|item| Item {
            name: item.name,
            is_dir: item.is_dir,
            size: u64::try_from(item.size).unwrap_or(0),
            modified: timestamp(item.mod_time.as_deref()),
            created: None,
        })
        .collect();
    entries(dir, label, items)
}

impl Provider for Rclone {
    fn name(&self) -> &str {
        &self.remote
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            upload: true,
            removal: Some(if self.backend == "drive" {
                Removal::Trash
            } else {
                Removal::Permanent
            }),
            create_folder: true,
            rename: true,
            move_within: true,
        }
    }

    fn list(&self, dir: &Location, control: &Control) -> io::Result<Vec<Entry>> {
        let args = vec!["lsjson".into(), self.path(dir).into()];
        let bytes = self.run(&args, control.cancel, Some(LIST_TIMEOUT))?;
        parse_listing(dir, &self.label, &bytes)
    }

    fn download(
        &self,
        source: &Location,
        is_dir: bool,
        destination: &Path,
        control: &Control,
    ) -> io::Result<()> {
        let name = source.name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "cannot download a remote root")
        })?;
        let target = absolute(destination)?.join(name);
        let args = transfer_args(self.path(source).into(), target.into(), is_dir);
        (control.progress)(name, 0);
        self.run(&args, control.cancel, None).map(drop)
    }

    fn upload(&self, source: &Path, destination: &Location, control: &Control) -> io::Result<()> {
        let source = absolute(source)?;
        let name = source.file_name().and_then(OsStr::to_str).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid local file name")
        })?;
        let target = self.path(&destination.child(name)?);
        let args = transfer_args(source.into(), target.into(), source.is_dir());
        (control.progress)(name, 0);
        self.run(&args, control.cancel, None).map(drop)
    }

    fn remove(&self, target: &Location, is_dir: bool, control: &Control) -> io::Result<()> {
        if target.name().is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot remove a remote root",
            ));
        }
        self.run(
            &remove_args(self.path(target), is_dir),
            control.cancel,
            None,
        )
        .map(drop)
    }

    fn create_folder(&self, parent: &Location, name: &str, control: &Control) -> io::Result<()> {
        let args = vec!["mkdir".into(), self.path(&parent.child(name)?).into()];
        self.run(&args, control.cancel, None).map(drop)
    }

    fn rename(&self, target: &Location, new_name: &str, control: &Control) -> io::Result<()> {
        let parent = target.parent().ok_or_else(root_error)?;
        self.move_to_path(target, &parent.child(new_name)?, control)
    }

    fn move_to(
        &self,
        target: &Location,
        destination: &Location,
        control: &Control,
    ) -> io::Result<()> {
        let name = target.name().ok_or_else(root_error)?;
        self.move_to_path(target, &destination.child(name)?, control)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Rclone, backend_label, parse_listing, parse_remotes, program, remove_args, transfer_args,
    };
    use crate::remote::{Control, Location, Provider, Removal};
    use std::ffi::OsString;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;

    fn remote(name: &str, backend: &str) -> Rclone {
        Rclone {
            program: program(),
            remote: name.into(),
            backend: backend.into(),
            label: format!("{} ({name})", backend_label(backend)),
        }
    }

    #[test]
    fn parses_listremotes_long() {
        let text = "gdrive:   drive\nbox: s3\n\nbroken\nsrv:sftp\n";
        assert_eq!(
            parse_remotes(text),
            vec![
                ("gdrive".to_owned(), "drive".to_owned()),
                ("box".to_owned(), "s3".to_owned()),
                ("srv".to_owned(), "sftp".to_owned()),
            ]
        );
    }

    #[test]
    fn labels_and_capabilities_follow_the_backend() {
        let drive = remote("gdrive", "drive");
        assert_eq!(drive.label(), "Google Drive (gdrive)");
        assert_eq!(drive.name(), "gdrive");
        assert_eq!(drive.capabilities().removal, Some(Removal::Trash));
        let other = remote("nas", "webdav");
        assert_eq!(other.label(), "webdav (nas)");
        assert!(other.capabilities().upload);
        assert_eq!(other.capabilities().removal, Some(Removal::Permanent));
    }

    #[test]
    fn decodes_lsjson() {
        let dir = Location::root("gdrive").unwrap();
        let entries = parse_listing(
            &dir,
            "Google Drive (gdrive)",
            br#"[
            {"Path":"Docs","Name":"Docs","Size":-1,"MimeType":"inode/directory","ModTime":"2026-10-06T12:00:00.123456789Z","IsDir":true},
            {"Path":"a b.txt","Name":"a b.txt","Size":42,"ModTime":"2026-10-06T12:00:00+02:00","IsDir":false}
        ]"#,
        )
        .unwrap();
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].size, 0);
        assert!(entries[0].modified.is_some());
        assert_eq!(entries[1].size, 42);
        assert_eq!(entries[1].path, Path::new("gdrive:/a b.txt"));
    }

    #[test]
    fn rejects_unsafe_names_and_partial_json() {
        let dir = Location::root("gdrive").unwrap();
        for input in [
            r#"[{"Name":"../x","Size":1,"IsDir":false}]"#,
            r#"[{"Name":"a","Size":1,"IsDir":false},{"Name":"a","Size":1,"IsDir":false}]"#,
            r#"[{"Name":"a","#,
        ] {
            assert!(
                parse_listing(&dir, "x", input.as_bytes()).is_err(),
                "{input}"
            );
        }
    }

    #[test]
    fn builds_transfer_and_remove_args() {
        let s = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(
            transfer_args("g:a.txt".into(), "/tmp/a.txt".into(), false),
            s(&["copyto", "g:a.txt", "/tmp/a.txt", "--ignore-existing"])
        );
        assert_eq!(
            transfer_args("/tmp/d".into(), "g:x/d".into(), true),
            s(&["copy", "/tmp/d", "g:x/d", "--ignore-existing"])
        );
        assert_eq!(remove_args("g:a".into(), false), s(&["deletefile", "g:a"]));
        assert_eq!(remove_args("g:a".into(), true), s(&["purge", "g:a"]));
    }

    #[test]
    fn rejects_relative_local_paths_and_roots() {
        let cancel = AtomicBool::new(false);
        let control = Control::new(&cancel);
        let drive = remote("gdrive", "drive");
        let root = Location::root("gdrive").unwrap();
        let file = root.child("a").unwrap();
        assert!(
            drive
                .download(&file, false, Path::new("rel"), &control)
                .is_err()
        );
        assert!(drive.upload(Path::new("rel"), &root, &control).is_err());
        assert!(drive.remove(&root, true, &control).is_err());
        assert_eq!(drive.path(&root), "gdrive:");
        assert_eq!(drive.path(&file), "gdrive:a");
    }
}
