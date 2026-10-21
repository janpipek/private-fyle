//! SSH host key verification for SFTP against `~/.ssh/known_hosts`.
//!
//! The russh backend of `remotefs-ssh` builds its [`Handler`] with
//! `Default`, so it cannot be told which server it talks to. [`connecting`]
//! therefore records the target in shared state (and serialises SFTP
//! connects) while a connection is made; [`KnownHostsCheck`] reads it, and
//! leaves the rejection reason there for the caller to report.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use russh::client::Handler;
use russh::keys::{PublicKey, PublicKeyOrCertificate, check_known_hosts_path};
use ssh2_config::{ParseRule, SshConfig};

/// The server a connect is in progress for, and why its key was rejected.
#[derive(Default)]
struct State {
    host: String,
    port: u16,
    rejection: Option<std::io::Error>,
}

static STATE: Mutex<State> = Mutex::new(State {
    host: String::new(),
    port: 0,
    rejection: None,
});
static CONNECT_LOCK: Mutex<()> = Mutex::new(());

fn state() -> MutexGuard<'static, State> {
    STATE
        .lock()
        .expect("the host key state mutex is never poisoned")
}

/// Marks a connect to `host:port` in progress; only one runs at a time.
pub struct Connecting {
    _lock: MutexGuard<'static, ()>,
}

/// Starts a connect to `host:port`; keep the guard until it has finished.
pub fn connecting(host: &str, port: u16) -> Connecting {
    let lock = CONNECT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    *state() = State {
        host: host.to_owned(),
        port,
        rejection: None,
    };
    Connecting { _lock: lock }
}

impl Connecting {
    /// The reason the server key was rejected, if it was.
    pub fn rejection(&self) -> Option<std::io::Error> {
        state().rejection.take()
    }
}

/// An untrusted key returned as an I/O error for explicit user confirmation.
#[derive(Clone, Debug)]
pub struct UnknownKey {
    host: String,
    port: u16,
    key: PublicKey,
    path: PathBuf,
}

impl std::fmt::Display for UnknownKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: unknown SSH host key", self.host, self.port)
    }
}

impl std::error::Error for UnknownKey {}

impl UnknownKey {
    /// Describes the exact key and destination that accepting will trust.
    pub fn description(&self) -> String {
        format!(
            "{}:{}\n{}\n{}\n\nVerify this fingerprint with the server administrator. Accepting saves this key to {} and retries the connection.",
            self.host,
            self.port,
            self.key.algorithm(),
            self.key.fingerprint(russh::keys::HashAlg::Sha256),
            self.path.display()
        )
    }

    /// Saves only this key, refusing a conflicting key added since the prompt.
    pub fn accept(&self) -> std::io::Result<()> {
        self.save().map_err(|error| {
            std::io::Error::new(error.kind(), format!("{}: {error}", self.path.display()))
        })
    }

    fn save(&self) -> std::io::Result<()> {
        use std::io::Write;
        if self.host.is_empty()
            || self.host.chars().any(|c| {
                c.is_whitespace() || c.is_control() || matches!(c, ',' | '*' | '?' | '|' | '#')
            })
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid SSH host name",
            ));
        }
        let _lock = CONNECT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        match verdict(&self.host, self.port, &self.key, &self.path) {
            Verdict::Known => return Ok(()),
            Verdict::Unknown => {}
            other => {
                return Err(std::io::Error::other(
                    rejection(&self.host, &other).expect("nonmatching verdict has a reason"),
                ));
            }
        }
        if let Some(parent) = self.path.parent() {
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(parent)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&self.path)?;
        let host = if self.port == 22 {
            self.host.clone()
        } else {
            format!("[{}]:{}", self.host, self.port)
        };
        let key = self.key.to_openssh().map_err(std::io::Error::other)?;
        // A leading newline also handles an existing file without a final newline.
        writeln!(file, "\n{host} {key}")?;
        file.flush()
    }

    /// Extracts a confirmation request without interpreting an error message.
    pub fn from_error(error: &std::io::Error) -> Option<Self> {
        error.get_ref()?.downcast_ref::<Self>().cloned()
    }
}

/// How a server key compares with `known_hosts`.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The recorded key matches.
    Known,
    /// Nothing is recorded for the host.
    Unknown,
    /// A different key is recorded for the host.
    Changed,
    /// The file could not be used.
    Unreadable(String),
}

/// Compares `key` with the entries for `host:port` in the file at `path`.
/// Entries for ports other than 22 are `[host]:port`; hashed ones work.
pub fn verdict(host: &str, port: u16, key: &PublicKey, path: &Path) -> Verdict {
    // The upstream checker treats every open failure as a missing file.
    // Only NotFound should offer the user a first-connection confirmation.
    match std::fs::File::open(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Verdict::Unknown,
        Err(error) => return Verdict::Unreadable(error.to_string()),
    }
    match check_known_hosts_path(host, port, key, path) {
        Ok(true) => Verdict::Known,
        Ok(false) => Verdict::Unknown,
        Err(russh::keys::Error::KeyChanged { .. }) => Verdict::Changed,
        Err(error) => Verdict::Unreadable(error.to_string()),
    }
}

/// The message for a rejected key, or `None` when it is accepted.
pub fn rejection(host: &str, verdict: &Verdict) -> Option<String> {
    match verdict {
        Verdict::Known => None,
        Verdict::Unknown => Some(format!(
            "{host}: host key is not in ~/.ssh/known_hosts; connect once with ssh {host} to verify and accept it"
        )),
        Verdict::Changed => Some(format!(
            "{host}: HOST KEY CHANGED (possible man-in-the-middle); check ~/.ssh/known_hosts"
        )),
        Verdict::Unreadable(error) => {
            Some(format!("{host}: cannot check ~/.ssh/known_hosts: {error}"))
        }
    }
}

/// The server `host` really is when `~/.ssh/config` (the file at `config`)
/// maps it to another `HostName` and `Port`, as ssh does; `port` from
/// `remotes.toml` wins, like in the SFTP client. Unreadable or missing
/// configuration changes nothing. `remotefs-ssh` resolves the same way when
/// it connects, so known_hosts must be checked for the resolved server.
pub fn resolve(host: &str, port: Option<u16>, config: &Path) -> (String, u16) {
    let params = std::fs::File::open(config)
        .ok()
        .and_then(|file| {
            let rules = ParseRule::ALLOW_UNKNOWN_FIELDS | ParseRule::ALLOW_UNSUPPORTED_FIELDS;
            SshConfig::default()
                .parse(&mut std::io::BufReader::new(file), rules)
                .ok()
        })
        .map(|config| config.query(host));
    let resolved = params
        .as_ref()
        .and_then(|p| p.host_name.clone())
        .unwrap_or_else(|| host.to_owned());
    let port = port.or_else(|| params.and_then(|p| p.port)).unwrap_or(22);
    (resolved, port)
}

/// `~/.ssh/config`.
pub fn ssh_config_file() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".ssh").join("config"))
}

fn known_hosts_file() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".ssh").join("known_hosts"))
}

fn check_key(host: &str, port: u16, key: &PublicKey, path: &Path) -> std::io::Result<()> {
    let verdict = verdict(host, port, key, path);
    if verdict == Verdict::Unknown {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            UnknownKey {
                host: host.to_owned(),
                port,
                key: key.clone(),
                path: path.to_path_buf(),
            },
        ));
    }
    match rejection(host, &verdict) {
        None => Ok(()),
        Some(reason) => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            reason,
        )),
    }
}

/// A russh handler that accepts only keys listed in `~/.ssh/known_hosts`.
pub struct KnownHostsCheck {
    host: String,
    port: u16,
}

impl Default for KnownHostsCheck {
    fn default() -> Self {
        let state = state();
        Self {
            host: state.host.clone(),
            port: state.port,
        }
    }
}

impl KnownHostsCheck {
    fn reject(&self, reason: String) -> Result<bool, russh::Error> {
        state().rejection = Some(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            reason,
        ));
        Ok(false)
    }
}

impl Handler for KnownHostsCheck {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = server_public_key else {
            return self.reject(format!(
                "{}: host certificates are not supported; use a plain host key in ~/.ssh/known_hosts",
                self.host
            ));
        };
        let Some(path) = known_hosts_file() else {
            return self.reject(format!("{}: cannot find the home directory", self.host));
        };
        match check_key(&self.host, self.port, key, &path) {
            Ok(()) => Ok(true),
            Err(error) => {
                state().rejection = Some(error);
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Verdict, rejection, resolve, verdict};
    use russh::keys::PublicKey;

    const KEY_A: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";
    const KEY_B: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIJd2kyP4V3kY2nZyS0rY0ZyWlZrJq3o8mV6yGxZbq9yC";

    fn key(base64: &str) -> PublicKey {
        PublicKey::from_openssh(&format!("ssh-ed25519 {base64}")).expect("a valid test key")
    }

    fn file(lines: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("known_hosts");
        std::fs::write(&path, lines).expect("write known_hosts");
        (dir, path)
    }

    #[test]
    fn only_unknown_keys_produce_a_confirmation_request() {
        let (_dir, path) = file(&format!("example.org ssh-ed25519 {KEY_A}\n"));
        assert!(super::check_key("example.org", 22, &key(KEY_A), &path).is_ok());
        let changed = super::check_key("example.org", 22, &key(KEY_B), &path).unwrap_err();
        assert!(super::UnknownKey::from_error(&changed).is_none());
        let error = super::check_key("new.org", 22, &key(KEY_A), &path).unwrap_err();
        let request = super::UnknownKey::from_error(&error).expect("confirmation required");
        assert_eq!(verdict("new.org", 22, &key(KEY_A), &path), Verdict::Unknown);
        request.accept().unwrap();
        assert!(super::check_key("new.org", 22, &key(KEY_A), &path).is_ok());
    }

    #[test]
    fn accepted_key_is_persisted_for_only_the_confirmed_host_and_port() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("ssh/known_hosts");
        let request = super::UnknownKey {
            host: "example.org".into(),
            port: 2222,
            key: key(KEY_A),
            path: path.clone(),
        };
        let description = request.description();
        assert!(description.contains("example.org:2222"));
        assert!(description.contains("ssh-ed25519"));
        assert!(description.contains("SHA256:"));
        request.accept().expect("accept key");
        assert_eq!(
            verdict("example.org", 2222, &key(KEY_A), &path),
            Verdict::Known
        );
        assert_eq!(
            verdict("example.org", 22, &key(KEY_A), &path),
            Verdict::Unknown
        );
        let before = std::fs::read(&path).unwrap();
        request.accept().expect("already accepted");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, request);
        assert!(super::UnknownKey::from_error(&error).is_some());
        assert!(super::UnknownKey::from_error(&std::io::Error::other("unknown key")).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn new_known_hosts_and_parent_have_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("ssh/known_hosts");
        super::UnknownKey {
            host: "example.org".into(),
            port: 22,
            key: key(KEY_A),
            path: path.clone(),
        }
        .accept()
        .unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    #[test]
    fn acceptance_cannot_add_extra_host_patterns_or_lines() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("known_hosts");
        for host in ["a,b", "*", "a\nb", "a b", "|1|salt|hash"] {
            let request = super::UnknownKey {
                host: host.into(),
                port: 22,
                key: key(KEY_A),
                path: path.clone(),
            };
            assert!(request.accept().is_err());
            assert!(!path.exists());
        }
    }

    #[test]
    fn acceptance_preserves_existing_entries_without_a_final_newline() {
        let (_dir, path) = file(&format!("other.org ssh-ed25519 {KEY_B}"));
        super::UnknownKey {
            host: "example.org".into(),
            port: 22,
            key: key(KEY_A),
            path: path.clone(),
        }
        .accept()
        .expect("accept key");
        assert_eq!(verdict("other.org", 22, &key(KEY_B), &path), Verdict::Known);
        assert_eq!(
            verdict("example.org", 22, &key(KEY_A), &path),
            Verdict::Known
        );
    }

    #[test]
    fn acceptance_refuses_a_key_changed_while_the_prompt_was_open() {
        let (_dir, path) = file(&format!("example.org ssh-ed25519 {KEY_B}\n"));
        let before = std::fs::read(&path).unwrap();
        let request = super::UnknownKey {
            host: "example.org".into(),
            port: 22,
            key: key(KEY_A),
            path: path.clone(),
        };
        assert!(request.accept().is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn acceptance_reports_an_unusable_known_hosts_path() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let request = super::UnknownKey {
            host: "example.org".into(),
            port: 22,
            key: key(KEY_A),
            path: dir.path().to_path_buf(),
        };
        assert!(request.accept().is_err());
    }

    #[test]
    fn matches_known_unknown_and_changed_keys() {
        let (_dir, path) = file(&format!("example.org ssh-ed25519 {KEY_A}\n"));
        assert_eq!(
            verdict("example.org", 22, &key(KEY_A), &path),
            Verdict::Known
        );
        assert_eq!(
            verdict("other.org", 22, &key(KEY_A), &path),
            Verdict::Unknown
        );
        assert_eq!(
            verdict("example.org", 22, &key(KEY_B), &path),
            Verdict::Changed
        );
    }

    #[test]
    fn non_default_ports_use_the_bracket_form() {
        let (_dir, path) = file(&format!("[example.org]:2222 ssh-ed25519 {KEY_A}\n"));
        assert_eq!(
            verdict("example.org", 2222, &key(KEY_A), &path),
            Verdict::Known
        );
        assert_eq!(
            verdict("example.org", 22, &key(KEY_A), &path),
            Verdict::Unknown
        );
    }

    #[test]
    fn ssh_config_aliases_resolve_to_the_real_host_and_port() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let config = dir.path().join("config");
        std::fs::write(
            &config,
            "Host nas\n  HostName nas.example.org\n  Port 2222\n  Unknown yes\nHost *\n  User jan\n",
        )
        .expect("write config");
        let nas = ("nas.example.org".to_owned(), 2222);
        assert_eq!(resolve("nas", None, &config), nas);
        // The port in remotes.toml wins over the config's.
        assert_eq!(resolve("nas", Some(22), &config).1, 22);
        // Other hosts and a missing file stay as configured.
        assert_eq!(resolve("other", None, &config), ("other".to_owned(), 22));
        let none = dir.path().join("none");
        assert_eq!(resolve("nas", Some(2200), &none), ("nas".to_owned(), 2200));
    }

    #[test]
    fn missing_file_is_unknown() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("none");
        assert_eq!(
            verdict("example.org", 22, &key(KEY_A), &path),
            Verdict::Unknown
        );
    }

    #[test]
    fn messages_name_the_host() {
        assert_eq!(rejection("h", &Verdict::Known), None);
        assert_eq!(
            rejection("h", &Verdict::Unknown).as_deref(),
            Some(
                "h: host key is not in ~/.ssh/known_hosts; connect once with ssh h to verify and accept it"
            )
        );
        assert_eq!(
            rejection("h", &Verdict::Changed).as_deref(),
            Some("h: HOST KEY CHANGED (possible man-in-the-middle); check ~/.ssh/known_hosts")
        );
        assert!(rejection("h", &Verdict::Unreadable("x".into())).is_some());
    }
}
