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

/// The server a connect is in progress for, and why its key was rejected.
#[derive(Default)]
struct State {
    host: String,
    port: u16,
    rejection: Option<String>,
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
    pub fn rejection(&self) -> Option<String> {
        state().rejection.take()
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

fn known_hosts_file() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".ssh").join("known_hosts"))
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
        state().rejection = Some(reason);
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
        match rejection(&self.host, &verdict(&self.host, self.port, key, &path)) {
            None => Ok(true),
            Some(reason) => self.reject(reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Verdict, rejection, verdict};
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
