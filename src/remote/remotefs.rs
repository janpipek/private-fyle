//! A [`Provider`] for SFTP, FTP/FTPS, WebDAV and S3 servers, built on the
//! `remotefs` crates and configured in `remotes.toml` (see
//! [`super::connections`]).
//!
//! One connection per remote is opened lazily and kept; it is serialised by a
//! mutex, so a long transfer delays other requests to the same remote. A
//! list, folder or remove operation that fails with a connection error
//! reconnects and is tried once more; transfers are not retried, because a
//! half-sent file would then look like an existing one. Downloads go to a
//! `.fyle-part` file that replaces the final name only when complete.
//! Symbolic links on the server are listed as files and skipped when copying
//! folders. WebDAV and S3 buffer whole files in memory, as their crates do.

use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
#[cfg(any(feature = "sftp", feature = "webdav", feature = "s3"))]
use std::sync::OnceLock;
use std::time::Duration;

use remotefs::fs::{ReadOptions, WriteOptions};
use remotefs::{File, RemoteError, RemoteErrorType, RemoteFs};

use super::connections::{Protocol, RemoteConfig};
use super::credentials::{CredentialStore, SystemCredentialStore};
#[cfg(feature = "sftp")]
use super::hostkey;
use super::process::Program;
use super::{Capabilities, Control, Item, Listing, Location, Provider, Removal, entries};

/// Size of the blocks that are copied between disk and server.
pub const CHUNK: usize = 256 * 1024;
#[cfg(feature = "sftp")]
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const PASSWORD_TIMEOUT: Duration = Duration::from_secs(60);

type Client = Box<dyn RemoteFs>;

/// One server from `remotes.toml`, shown as `<name>:/...`.
pub struct RemoteFsProvider {
    config: RemoteConfig,
    password_override: Option<String>,
    label: String,
    client: Mutex<Option<Client>>,
}

impl RemoteFsProvider {
    /// Prepares the remote; nothing is connected until first use.
    pub fn new(config: RemoteConfig) -> Self {
        Self {
            label: format!("{} ({})", config.protocol.label(), config.name),
            config,
            password_override: None,
            client: Mutex::new(None),
        }
    }

    /// Prepares a connection test with an explicit password override.
    pub fn with_password(config: RemoteConfig, password: Option<String>) -> Self {
        Self {
            label: format!("{} ({})", config.protocol.label(), config.name),
            config,
            password_override: password,
            client: Mutex::new(None),
        }
    }

    fn standard_url(&self, location: &Location) -> Option<String> {
        let path = remote_path(&self.config.root, location);
        let path = path.to_str()?;
        let encoded_path = encode_url_path(path);
        match self.config.protocol {
            Protocol::Sftp | Protocol::Ftp | Protocol::Ftps => {
                let scheme = match self.config.protocol {
                    Protocol::Sftp => "sftp",
                    Protocol::Ftp => "ftp",
                    Protocol::Ftps => "ftps",
                    _ => unreachable!(),
                };
                let host = self.config.host.as_deref()?;
                if !safe_url_host(host) {
                    return None;
                }
                let user = self
                    .config
                    .user
                    .as_deref()
                    .map(|u| format!("{}@", encode_url_component(u)))
                    .unwrap_or_default();
                let port = self
                    .config
                    .port
                    .map(|p| format!(":{p}"))
                    .unwrap_or_default();
                Some(format!("{scheme}://{user}{host}{port}{encoded_path}"))
            }
            Protocol::Webdav => {
                let base = self.config.url.as_deref()?;
                let (scheme, rest) = base.split_once("://")?;
                let (authority, base_path) = rest.split_once('/').unwrap_or((rest, ""));
                if rest.contains(['?', '#']) {
                    return None;
                }
                let authority = authority
                    .rsplit_once('@')
                    .map_or(authority, |(_, host)| host);
                if !safe_url_authority(authority) {
                    return None;
                }
                if !safe_uri_path(base_path) {
                    return None;
                }
                let base_path = base_path.trim_matches('/');
                let base = if base_path.is_empty() {
                    String::new()
                } else {
                    format!("/{base_path}")
                };
                Some(format!("{scheme}://{authority}{base}{encoded_path}"))
            }
            Protocol::S3 => {
                let bucket = self.config.bucket.as_deref()?;
                safe_s3_bucket(bucket).then(|| format!("s3://{bucket}{encoded_path}"))
            }
        }
    }

    /// Runs `op` on the connection, connecting first when needed. With
    /// `retry`, a connection error drops the connection and runs `op` once
    /// more on a fresh one.
    fn with_client<T>(
        &self,
        control: &Control,
        retry: bool,
        mut op: impl FnMut(&dyn RemoteFs) -> io::Result<T>,
    ) -> io::Result<T> {
        let mut guard = self
            .client
            .lock()
            .expect("the remote client lock is never held across a panic");
        let mut attempts = if retry { 2 } else { 1 };
        loop {
            if control.cancelled() {
                return Err(Control::cancelled_error());
            }
            if guard.as_ref().is_some_and(|client| !client.is_connected()) {
                *guard = None;
            }
            if guard.is_none() {
                *guard = Some(self.connect(control)?);
            }
            let client = guard.as_deref().expect("the client was just connected");
            match op(client) {
                Err(error) if attempts > 1 && is_connection_error(&error) => {
                    attempts -= 1;
                    *guard = None;
                }
                result => {
                    if result.as_ref().is_err_and(is_connection_error) {
                        *guard = None;
                    }
                    return result;
                }
            }
        }
    }

    fn connect(&self, control: &Control) -> io::Result<Client> {
        let password = password(&self.config, self.password_override.as_deref(), control)?;
        let mut client = build_client(&self.config, password)?;
        // The SFTP host key check reports why it refused through the guard.
        #[cfg(feature = "sftp")]
        let guard = (self.config.protocol == Protocol::Sftp).then(|| {
            let (host, port) = match hostkey::ssh_config_file() {
                Some(file) => hostkey::resolve(
                    self.config.host.as_deref().unwrap_or_default(),
                    self.config.port,
                    &file,
                ),
                None => (
                    self.config.host.clone().unwrap_or_default(),
                    self.config.port.unwrap_or(22),
                ),
            };
            hostkey::connecting(&host, port)
        });
        if let Err(error) = client.connect() {
            #[cfg(feature = "sftp")]
            if let Some(reason) = guard.as_ref().and_then(hostkey::Connecting::rejection) {
                return Err(reason);
            }
            return Err(remote_error(&self.label, error));
        }
        Ok(client)
    }

    fn path(&self, location: &Location) -> PathBuf {
        remote_path(&self.config.root, location)
    }

    fn err(&self, error: RemoteError) -> io::Error {
        remote_error(&self.label, error)
    }
}

fn safe_url_host(host: &str) -> bool {
    !host.is_empty()
        && host.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':' | b'[' | b']')
        })
}

fn safe_url_authority(authority: &str) -> bool {
    !authority.is_empty()
        && authority.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':' | b'[' | b']')
        })
}

fn safe_s3_bucket(bucket: &str) -> bool {
    !bucket.is_empty()
        && bucket.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        })
}

fn safe_uri_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
            continue;
        }
        if !(byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'/' | b':'
                    | b'@'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b'-'
                    | b'.'
                    | b'_'
                    | b'~'
            ))
        {
            return false;
        }
        index += 1;
    }
    true
}

fn encode_url_path(path: &str) -> String {
    path.split('/')
        .map(encode_url_component)
        .collect::<Vec<_>>()
        .join("/")
}

fn encode_url_component(component: &str) -> String {
    let mut encoded = String::new();
    for byte in component.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

/// The server path for a panel location: the configured root plus the
/// location's parts.
pub fn remote_path(root: &str, location: &Location) -> PathBuf {
    let relative = location.relative();
    if relative.is_empty() {
        PathBuf::from(root)
    } else if root == "/" {
        PathBuf::from(format!("/{relative}"))
    } else {
        PathBuf::from(format!("{root}/{relative}"))
    }
}

/// Whether an error from [`remote_error`] says the connection is gone.
fn is_connection_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionAborted | io::ErrorKind::NotConnected
    )
}

/// Converts a `remotefs` error into an `io::Error` with a matching kind and
/// a message starting with the remote's label.
pub fn remote_error(label: &str, error: RemoteError) -> io::Error {
    let kind = match error.kind() {
        RemoteErrorType::NoSuchFileOrDirectory => io::ErrorKind::NotFound,
        RemoteErrorType::AlreadyExists => io::ErrorKind::AlreadyExists,
        RemoteErrorType::PermissionDenied | RemoteErrorType::AuthenticationFailed => {
            io::ErrorKind::PermissionDenied
        }
        RemoteErrorType::ConnectionError | RemoteErrorType::NotConnected => {
            io::ErrorKind::ConnectionAborted
        }
        RemoteErrorType::UnsupportedFeature => io::ErrorKind::Unsupported,
        RemoteErrorType::BadAddress | RemoteErrorType::InvalidPath => io::ErrorKind::InvalidInput,
        RemoteErrorType::DirectoryNotEmpty => io::ErrorKind::DirectoryNotEmpty,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, format!("{label}: {error}"))
}

/// Runs `password_command` (without a shell) and returns its output without
/// the trailing newline.
fn password(
    config: &RemoteConfig,
    override_password: Option<&str>,
    control: &Control,
) -> io::Result<Option<String>> {
    password_with_store(config, override_password, control, &SystemCredentialStore)
}

fn password_with_store(
    config: &RemoteConfig,
    override_password: Option<&str>,
    control: &Control,
    store: &dyn CredentialStore,
) -> io::Result<Option<String>> {
    if let Some(password) = override_password {
        return Ok(Some(password.to_owned()));
    }
    let Some(words) = &config.password_command else {
        let Some(id) = &config.credential_id else {
            return Ok(None);
        };
        if control.cancelled() {
            return Err(Control::cancelled_error());
        }
        let saved = store
            .get_cancellable(id, control.cancel)
            .map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", config.name)))?;
        if control.cancelled() {
            return Err(Control::cancelled_error());
        }
        return saved.map(Some).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "{}: saved password is missing from the keyring; re-enter it or forget the saved password",
                    config.name
                ),
            )
        });
    };
    let program = Program {
        program: words[0].clone().into(),
        service: "password_command",
        install_hint: "use an executable available to fyle, or configure its absolute path",
        failure_hint: "",
    };
    let args: Vec<&OsStr> = words[1..].iter().map(OsStr::new).collect();
    let output = program
        .run(&args, control.cancel, Some(PASSWORD_TIMEOUT))
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!(
                    "{}: password_command could not provide a password",
                    config.name
                ),
            )
        })?;
    let text = String::from_utf8_lossy(&output);
    let text = text.trim_end_matches(['\n', '\r']);
    if text.is_empty() {
        return Err(io::Error::other(format!(
            "{}: password_command printed nothing",
            config.name
        )));
    }
    Ok(Some(text.to_owned()))
}

/// The runtime that drives the async SFTP, WebDAV and S3 clients.
#[cfg(any(feature = "sftp", feature = "webdav", feature = "s3"))]
fn runtime() -> io::Result<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<Result<tokio::runtime::Runtime, String>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .thread_name("fyle-remote")
                .build()
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| io::Error::other(format!("cannot start the network runtime: {e}")))
}

/// The error for a protocol that `remotes.toml` configures but this build
/// was compiled without (see the `sftp`/`ftp`/`webdav`/`s3` Cargo features).
#[cfg(not(all(feature = "sftp", feature = "ftp", feature = "webdav", feature = "s3")))]
fn not_compiled_in(config: &RemoteConfig) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "{}: this build of fyle was compiled without {} support",
            config.name,
            config.protocol.label()
        ),
    )
}

/// Creates the (unconnected) client for the configured protocol. `password`
/// goes unused when sftp, ftp and webdav (the protocols that take one) are
/// all disabled.
#[cfg_attr(
    not(any(feature = "sftp", feature = "ftp", feature = "webdav")),
    allow(unused_variables)
)]
fn build_client(config: &RemoteConfig, password: Option<String>) -> io::Result<Client> {
    match config.protocol {
        #[cfg(feature = "sftp")]
        Protocol::Sftp => {
            let host = config.host.clone().unwrap_or_default();
            let mut opts = remotefs_ssh::SshOpts::new(&host)
                .connection_timeout(CONNECT_TIMEOUT)
                .ssh_agent_identity(Some(remotefs_ssh::SshAgentIdentity::All));
            if let Some(port) = config.port {
                opts = opts.port(port);
            }
            if let Some(user) = &config.user {
                opts = opts.username(user);
            }
            if let Some(password) = &password {
                opts = opts.password(password);
            }
            if let Some(ssh_config) = dirs::home_dir().map(|h| h.join(".ssh").join("config"))
                && ssh_config.is_file()
            {
                let rules = remotefs_ssh::SshConfigParseRule::ALLOW_UNKNOWN_FIELDS
                    | remotefs_ssh::SshConfigParseRule::ALLOW_UNSUPPORTED_FIELDS;
                opts = opts.config_file(ssh_config, rules);
            }
            let client = remotefs_ssh::RusshSftpFs::<hostkey::KnownHostsCheck>::new(opts);
            Ok(Box::new(client.into_blocking(runtime()?.handle().clone())))
        }
        #[cfg(not(feature = "sftp"))]
        Protocol::Sftp => Err(not_compiled_in(config)),

        #[cfg(feature = "ftp")]
        Protocol::Ftp | Protocol::Ftps => {
            let host = config.host.clone().unwrap_or_default();
            let mut client = remotefs_ftp::FtpFs::new(&host, config.port.unwrap_or(21))
                .username(config.user.as_deref().unwrap_or("anonymous"))
                .password(password.as_deref().unwrap_or(""))
                .passive_mode();
            if config.protocol == Protocol::Ftps {
                client = client.secure();
            }
            Ok(Box::new(client))
        }
        #[cfg(not(feature = "ftp"))]
        Protocol::Ftp | Protocol::Ftps => Err(not_compiled_in(config)),

        #[cfg(feature = "webdav")]
        Protocol::Webdav => {
            let bad = |e: RemoteError| remote_error(&config.name, e);
            let auth = match (&config.user, &password) {
                (Some(user), Some(password)) => remotefs_webdav::Auth::basic(user, password),
                (Some(user), None) => remotefs_webdav::Auth::basic(user, ""),
                (None, Some(token)) => remotefs_webdav::Auth::bearer(token),
                (None, None) => remotefs_webdav::Auth::None,
            };
            let url = config.url.as_deref().unwrap_or_default();
            let client = remotefs_webdav::WebDAVFs::new(url, auth).map_err(bad)?;
            Ok(Box::new(client.into_blocking(runtime()?.handle().clone())))
        }
        #[cfg(not(feature = "webdav"))]
        Protocol::Webdav => Err(not_compiled_in(config)),

        #[cfg(feature = "s3")]
        Protocol::S3 => {
            let mut client = remotefs_aws_s3::AwsS3Fs::new(config.bucket.as_deref().unwrap_or(""));
            if let Some(region) = &config.region {
                client = client.region(region);
            }
            if let Some(profile) = &config.profile {
                client = client.profile(profile);
            }
            if let Some(endpoint) = &config.endpoint {
                client = client.endpoint(endpoint).new_path_style(true);
            }
            Ok(Box::new(client.into_blocking(runtime()?.handle().clone())))
        }
        #[cfg(not(feature = "s3"))]
        Protocol::S3 => Err(not_compiled_in(config)),
    }
}

/// Maps a listed server entry to a panel item.
fn item_from(file: &File) -> Item {
    let meta = file.metadata();
    Item {
        name: file.name(),
        is_dir: file.is_dir(),
        size: meta.size,
        modified: meta.modified,
        created: meta.created,
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

fn root_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "cannot change a remote root folder",
    )
}

/// Bytes done so far in one provider call, reported through [`Control`].
struct Progress<'a, 'b> {
    control: &'a Control<'b>,
    done: u64,
}

/// Copies `reader` to `writer` in [`CHUNK`]-sized blocks, stopping with an
/// `Interrupted` error when cancelled and reporting `name` after each block.
fn copy_chunks(
    reader: &mut dyn Read,
    writer: &mut dyn Write,
    name: &str,
    progress: &mut Progress,
) -> io::Result<()> {
    let mut buffer = vec![0; CHUNK];
    loop {
        if progress.control.cancelled() {
            return Err(Control::cancelled_error());
        }
        let read = match reader.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        writer.write_all(&buffer[..read])?;
        progress.done += read as u64;
        (progress.control.progress)(name, progress.done);
    }
}

struct Ctx<'a, 'b, 'c> {
    label: &'a str,
    progress: &'a mut Progress<'b, 'c>,
}

impl Ctx<'_, '_, '_> {
    fn err(&self, error: RemoteError) -> io::Error {
        remote_error(self.label, error)
    }
}

/// Downloads `remote` to `local`, skipping anything that already exists.
fn download_item(
    fs: &dyn RemoteFs,
    remote: &Path,
    local: &Path,
    is_dir: bool,
    ctx: &mut Ctx,
) -> io::Result<()> {
    if local.symlink_metadata().is_ok() {
        return Ok(());
    }
    if is_dir {
        fs::create_dir(local)?;
        let mut children = fs.list_dir(remote).map_err(|e| ctx.err(e))?;
        children.sort_by_key(File::name);
        for child in children {
            let name = child.name();
            if matches!(name.as_str(), "." | "..") || child.is_symlink() {
                continue;
            }
            crate::location::remote::validate_entry_name(&name)?;
            download_item(fs, child.path(), &local.join(&name), child.is_dir(), ctx)?;
        }
        return Ok(());
    }
    let name = local.file_name().and_then(OsStr::to_str).unwrap_or("file");
    let mut part = local.as_os_str().to_owned();
    part.push(".fyle-part");
    let part = PathBuf::from(part);
    let result = (|| {
        let mut reader = fs
            .open(remote, &ReadOptions::default())
            .map_err(|e| ctx.err(e))?;
        let mut file = fs::File::create(&part)?;
        copy_chunks(&mut reader, &mut file, name, ctx.progress)?;
        file.flush()?;
        reader.finish().map_err(|e| ctx.err(e))?;
        fs::rename(&part, local)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

/// Uploads the local `source` to the server path `remote`, skipping
/// anything that already exists.
fn upload_item(fs: &dyn RemoteFs, source: &Path, remote: &Path, ctx: &mut Ctx) -> io::Result<()> {
    if fs.exists(remote).map_err(|e| ctx.err(e))? {
        return Ok(());
    }
    let meta = fs::metadata(source)?;
    if meta.is_dir() {
        fs.create_dir(remote, None).map_err(|e| ctx.err(e))?;
        let mut children = fs::read_dir(source)?.collect::<io::Result<Vec<_>>>()?;
        children.sort_by_key(fs::DirEntry::file_name);
        for child in children {
            if child.file_type()?.is_symlink() {
                continue;
            }
            let Some(name) = child.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            upload_item(fs, &child.path(), &remote.join(name), ctx)?;
        }
        return Ok(());
    }
    let name = source.file_name().and_then(OsStr::to_str).unwrap_or("file");
    let mut reader = fs::File::open(source)?;
    let options = WriteOptions::default().size_hint(meta.len());
    let mut writer = fs.create(remote, &options).map_err(|e| ctx.err(e))?;
    copy_chunks(&mut reader, &mut writer, name, ctx.progress)?;
    writer.flush()?;
    writer.finish().map_err(|e| ctx.err(e))
}

impl Provider for RemoteFsProvider {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            upload: true,
            removal: Some(Removal::Permanent),
            create_folder: true,
            rename: true,
            // WebDAV endpoints are not web views (browsers show raw XML or a
            // login prompt) and the other protocols have none.
            move_within: true,
            web: false,
        }
    }

    fn standard_url(&self, target: &Location) -> Option<String> {
        self.standard_url(target)
    }

    fn list(&self, dir: &Location, control: &Control) -> io::Result<Listing> {
        let path = self.path(dir);
        let files = self.with_client(control, true, |fs| {
            fs.list_dir(&path).map_err(|e| self.err(e))
        })?;
        let items = files
            .iter()
            .filter(|file| !matches!(file.name().as_str(), "." | ".."))
            .map(item_from)
            .collect();
        Ok(entries(dir, items))
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
        let local = absolute(destination)?.join(name);
        fs::create_dir_all(destination)?;
        let remote = self.path(source);
        let mut progress = Progress { control, done: 0 };
        self.with_client(control, false, |fs| {
            let mut ctx = Ctx {
                label: &self.label,
                progress: &mut progress,
            };
            download_item(fs, &remote, &local, is_dir, &mut ctx)
        })
    }

    fn upload(&self, source: &Path, destination: &Location, control: &Control) -> io::Result<()> {
        let source = absolute(source)?;
        let name = source.file_name().and_then(OsStr::to_str).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid local file name")
        })?;
        let remote = self.path(&destination.child(name)?);
        let mut progress = Progress { control, done: 0 };
        self.with_client(control, false, |fs| {
            let mut ctx = Ctx {
                label: &self.label,
                progress: &mut progress,
            };
            upload_item(fs, source, &remote, &mut ctx)
        })
    }

    fn remove(&self, target: &Location, is_dir: bool, control: &Control) -> io::Result<()> {
        if target.name().is_none() {
            return Err(root_error());
        }
        let path = self.path(target);
        self.with_client(control, true, |fs| {
            let result = if is_dir {
                fs.remove_dir_all(&path)
            } else {
                fs.remove_file(&path)
            };
            result.map_err(|e| self.err(e))
        })
    }

    fn create_folder(&self, parent: &Location, name: &str, control: &Control) -> io::Result<()> {
        let path = self.path(&parent.child(name)?);
        self.with_client(control, true, |fs| {
            if fs.exists(&path).map_err(|e| self.err(e))? {
                return Err(exists_error(name));
            }
            fs.create_dir(&path, None).map_err(|e| self.err(e))
        })
    }

    fn rename(&self, target: &Location, new_name: &str, control: &Control) -> io::Result<()> {
        let parent = target.parent().ok_or_else(root_error)?;
        self.move_path(target, &parent.child(new_name)?, new_name, control)
    }

    fn move_to(
        &self,
        target: &Location,
        destination: &Location,
        control: &Control,
    ) -> io::Result<()> {
        let name = target.name().ok_or_else(root_error)?;
        self.move_path(target, &destination.child(name)?, name, control)
    }
}

fn exists_error(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{name} already exists"),
    )
}

impl RemoteFsProvider {
    /// Moves `from` to `to` unless `to` exists.
    fn move_path(
        &self,
        from: &Location,
        to: &Location,
        name: &str,
        control: &Control,
    ) -> io::Result<()> {
        let (from, to) = (self.path(from), self.path(to));
        self.with_client(control, true, |fs| {
            if fs.exists(&to).map_err(|e| self.err(e))? {
                return Err(exists_error(name));
            }
            fs.rename(&from, &to).map_err(|e| self.err(e))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CHUNK, Progress, RemoteFsProvider, copy_chunks, is_connection_error, item_from,
        password_with_store, remote_error, remote_path,
    };
    use crate::remote::connections::parse;
    use crate::remote::credentials::CredentialStore;
    use crate::remote::{Control, Location, Provider, Removal};
    use remotefs::fs::{FileType, Metadata};
    use remotefs::{File, RemoteError, RemoteErrorType};
    use std::io::{self, Cursor};
    use std::path::Path;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn provider() -> RemoteFsProvider {
        let (mut remotes, errors) = parse(
            "[[remote]]\nname = \"nas\"\nprotocol = \"sftp\"\nhost = \"h\"\nroot = \"/srv/data\"\n",
        )
        .unwrap();
        assert!(errors.is_empty());
        RemoteFsProvider::new(remotes.remove(0))
    }

    struct MissingStore;

    impl CredentialStore for MissingStore {
        fn get(&self, _id: &str) -> io::Result<Option<String>> {
            Ok(None)
        }

        fn has(&self, _id: &str) -> io::Result<bool> {
            Ok(false)
        }

        fn set(&self, _id: &str, _password: &str) -> io::Result<()> {
            Ok(())
        }

        fn delete(&self, _id: &str) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn missing_configured_password_is_actionable_and_test_override_wins() {
        let id = uuid::Uuid::new_v4();
        let text = format!(
            "[[remote]]\nname='nas'\nprotocol='sftp'\nhost='server'\ncredential_id='{id}'\n"
        );
        let (configs, errors) = parse(&text).unwrap();
        assert!(errors.is_empty());
        let cancelled = AtomicBool::new(false);
        let control = Control::new(&cancelled);
        let error = password_with_store(&configs[0], None, &control, &MissingStore).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("re-enter it or forget"));
        assert!(!error.to_string().contains(&id.to_string()));

        assert_eq!(
            password_with_store(&configs[0], Some("temporary"), &control, &MissingStore)
                .unwrap()
                .as_deref(),
            Some("temporary")
        );
    }

    #[test]
    fn maps_locations_below_the_configured_root() {
        let nas = Location::root("nas").unwrap();
        assert_eq!(remote_path("/srv/data", &nas), Path::new("/srv/data"));
        let deep = nas.child("a").unwrap().child("b c").unwrap();
        assert_eq!(
            remote_path("/srv/data", &deep),
            Path::new("/srv/data/a/b c")
        );
        assert_eq!(remote_path("/", &deep), Path::new("/a/b c"));
    }

    #[test]
    fn standard_urls_escape_path_components_and_omit_passwords() {
        let (mut remotes, errors) = parse(
            "[[remote]]\nname = \"files\"\nprotocol = \"ftp\"\nhost = \"example.test\"\nuser = \"a@b\"\nport = 2121\nroot = \"/srv/data\"\n",
        )
        .unwrap();
        assert!(errors.is_empty());
        let provider = RemoteFsProvider::new(remotes.remove(0));
        let location = Location::root("files").unwrap().child("a b#c").unwrap();
        assert_eq!(
            provider.standard_url(&location).as_deref(),
            Some("ftp://a%40b@example.test:2121/srv/data/a%20b%23c")
        );
    }

    #[test]
    fn webdav_urls_reject_malformed_base_paths() {
        let parse_provider = |url: &str| {
            let text =
                format!("[[remote]]\nname = \"dav\"\nprotocol = \"webdav\"\nurl = \"{url}\"\n");
            let (mut remotes, errors) = parse(&text).unwrap();
            assert!(errors.is_empty());
            RemoteFsProvider::new(remotes.remove(0))
        };
        let location = Location::root("dav").unwrap().child("file name").unwrap();
        let valid = parse_provider("https://user:secret@example.test/base%20path");
        assert_eq!(
            valid.standard_url(&location).as_deref(),
            Some("https://example.test/base%20path/file%20name")
        );
        let invalid = parse_provider("https://example.test/base path");
        assert_eq!(invalid.standard_url(&location), None);
        let malformed_escape = parse_provider("https://example.test/base%2");
        assert_eq!(malformed_escape.standard_url(&location), None);
    }

    #[test]
    fn s3_urls_require_a_safe_bucket_authority() {
        let make_provider = |bucket: &str| {
            let text =
                format!("[[remote]]\nname = \"store\"\nprotocol = \"s3\"\nbucket = \"{bucket}\"\n");
            let (mut remotes, errors) = parse(&text).unwrap();
            assert!(errors.is_empty());
            RemoteFsProvider::new(remotes.remove(0))
        };
        let location = Location::root("store").unwrap().child("a b").unwrap();
        assert_eq!(
            make_provider("my-bucket.example")
                .standard_url(&location)
                .as_deref(),
            Some("s3://my-bucket.example/a%20b")
        );
        assert_eq!(make_provider("bucket/other").standard_url(&location), None);
        assert_eq!(make_provider("bucket@host").standard_url(&location), None);
        assert_eq!(make_provider("bucket name").standard_url(&location), None);
    }

    #[test]
    fn provider_describes_itself() {
        let provider = provider();
        assert_eq!(provider.name(), "nas");
        assert_eq!(provider.label(), "SFTP (nas)");
        let caps = provider.capabilities();
        assert_eq!(caps.removal, Some(Removal::Permanent));
        assert!(caps.upload && caps.create_folder && caps.rename && caps.move_within);
    }

    #[test]
    fn copies_in_chunks_with_progress() {
        let data = vec![7u8; CHUNK * 2 + 10];
        let mut out = Vec::new();
        let cancel = AtomicBool::new(false);
        let reports = Mutex::new(Vec::new());
        let record = |name: &str, done: u64| reports.lock().unwrap().push((name.to_owned(), done));
        let control = Control {
            cancel: &cancel,
            progress: &record,
        };
        let mut progress = Progress {
            control: &control,
            done: 0,
        };
        copy_chunks(&mut Cursor::new(&data), &mut out, "f", &mut progress).unwrap();
        assert_eq!(out, data);
        let reports = reports.into_inner().unwrap();
        assert_eq!(
            reports.last().unwrap(),
            &("f".to_owned(), data.len() as u64)
        );
        assert!(reports.len() >= 3);
    }

    #[test]
    fn copy_stops_when_cancelled() {
        let cancel = AtomicBool::new(false);
        let stop = |_: &str, _: u64| cancel.store(true, Ordering::Relaxed);
        let control = Control {
            cancel: &cancel,
            progress: &stop,
        };
        let mut progress = Progress {
            control: &control,
            done: 0,
        };
        let data = vec![1u8; CHUNK * 4];
        let mut out = Vec::new();
        let error = copy_chunks(&mut Cursor::new(&data), &mut out, "f", &mut progress).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(out.len(), CHUNK);
    }

    #[test]
    fn errors_keep_their_kind_and_label() {
        let missing = remote_error(
            "SFTP (nas)",
            RemoteError::new(RemoteErrorType::NoSuchFileOrDirectory),
        );
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
        assert!(missing.to_string().starts_with("SFTP (nas): "));
        let gone = remote_error("x", RemoteError::new(RemoteErrorType::ConnectionError));
        assert!(is_connection_error(&gone));
        let denied = remote_error("x", RemoteError::new(RemoteErrorType::AuthenticationFailed));
        assert_eq!(denied.kind(), io::ErrorKind::PermissionDenied);
        assert!(!is_connection_error(&denied));
    }

    #[test]
    fn listed_files_become_items() {
        let when = std::time::SystemTime::UNIX_EPOCH;
        let dir = File::new(
            "/srv/Docs",
            Metadata::default().file_type(FileType::Directory),
        );
        let file = File::new("/srv/a.txt", Metadata::default().size(42).modified(when));
        let (dir, file) = (item_from(&dir), item_from(&file));
        assert!(dir.is_dir);
        let unknown = File::new("/srv/unknown", Metadata::default());
        assert_eq!(item_from(&unknown).size, None);
        assert_eq!(
            (file.name.as_str(), file.size, file.modified),
            ("a.txt", Some(42), Some(when))
        );
    }

    #[test]
    fn rejects_relative_local_paths_and_roots_without_connecting() {
        let provider = provider();
        let cancel = AtomicBool::new(false);
        let control = Control::new(&cancel);
        let root = Location::root("nas").unwrap();
        let file = root.child("a").unwrap();
        assert!(
            provider
                .download(&file, false, Path::new("rel"), &control)
                .is_err()
        );
        assert!(
            provider
                .download(&root, true, Path::new("/tmp"), &control)
                .is_err()
        );
        assert!(provider.upload(Path::new("rel"), &root, &control).is_err());
        assert!(provider.remove(&root, true, &control).is_err());
        assert!(provider.rename(&root, "x", &control).is_err());
        assert!(provider.rename(&file, "../x", &control).is_err());
        assert!(provider.create_folder(&root, "a/b", &control).is_err());
    }
}
