//! Credential storage for configured remotes, separated from connection data.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
use std::time::Duration;

/// Stores remote passwords under opaque identifiers kept in `remotes.toml`.
pub trait CredentialStore: Send + Sync {
    /// Retrieves a password, returning `None` when the identifier has no item.
    fn get(&self, id: &str) -> io::Result<Option<String>>;
    /// Retrieves a password while observing an in-flight provider cancellation.
    fn get_cancellable(&self, id: &str, cancel: &AtomicBool) -> io::Result<Option<String>> {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let value = self.get(id)?;
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        Ok(value)
    }
    /// Reports whether an identifier has a stored password.
    fn has(&self, id: &str) -> io::Result<bool>;
    /// Stores or replaces a password under an identifier.
    fn set(&self, id: &str, password: &str) -> io::Result<()>;
    /// Removes the password stored under an identifier.
    fn delete(&self, id: &str) -> io::Result<()>;
}

/// Native Linux Secret Service and portal-backed credential store.
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxCredentialStore;

/// Platform-neutral credential store used by remote providers and the UI.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemCredentialStore;

#[cfg(target_os = "linux")]
const OP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
#[cfg(target_os = "linux")]
const SERVICE: &str = "org.fyle.remote-password";

#[cfg(target_os = "linux")]
fn run<T>(future: impl std::future::Future<Output = io::Result<T>>) -> io::Result<T> {
    use futures_lite::future::{block_on, race};

    block_on(race(future, async {
        async_io::Timer::after(OP_TIMEOUT).await;
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Linux credential service did not respond within 60 seconds",
        ))
    }))
}

#[cfg(target_os = "linux")]
fn run_cancellable<T>(
    future: impl std::future::Future<Output = io::Result<T>>,
    cancel: &AtomicBool,
) -> io::Result<T> {
    use futures_lite::future::{block_on, race};

    block_on(race(
        future,
        race(
            async {
                async_io::Timer::after(OP_TIMEOUT).await;
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Linux credential service did not respond within 60 seconds",
                ))
            },
            async {
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
                    }
                    async_io::Timer::after(Duration::from_millis(100)).await;
                }
            },
        ),
    ))
}

#[cfg(target_os = "linux")]
fn attributes(id: &str) -> [(String, String); 2] {
    [("service".into(), SERVICE.into()), ("id".into(), id.into())]
}

#[cfg(target_os = "linux")]
fn service_error(operation: &str) -> io::Error {
    io::Error::other(format!(
        "Linux credential service could not {operation}; check that the desktop keyring is available and unlocked"
    ))
}

#[cfg(target_os = "linux")]
async fn keyring() -> io::Result<oo7::Keyring> {
    let keyring = oo7::Keyring::new()
        .await
        .map_err(|_| service_error("connect to the credential service"))?;
    let locked = keyring
        .is_locked()
        .await
        .map_err(|_| service_error("check whether the keyring is unlocked"))?;
    if locked {
        keyring
            .unlock()
            .await
            .map_err(|_| service_error("unlock the keyring"))?;
    }
    Ok(keyring)
}

#[cfg(target_os = "linux")]
async fn read_secret(id: String) -> io::Result<Option<String>> {
    let keyring = keyring().await?;
    let items = keyring
        .search_items(&attributes(&id))
        .await
        .map_err(|_| service_error("read the saved password"))?;
    let Some(item) = items.into_iter().next() else {
        return Ok(None);
    };
    let secret = item
        .secret()
        .await
        .map_err(|_| service_error("read the saved password"))?;
    String::from_utf8(secret.as_bytes().to_vec())
        .map(Some)
        .map_err(|_| service_error("read the saved password"))
}

#[cfg(target_os = "linux")]
impl CredentialStore for LinuxCredentialStore {
    fn get(&self, id: &str) -> io::Result<Option<String>> {
        run(read_secret(id.to_owned()))
    }

    fn get_cancellable(&self, id: &str, cancel: &AtomicBool) -> io::Result<Option<String>> {
        run_cancellable(read_secret(id.to_owned()), cancel)
    }

    fn has(&self, id: &str) -> io::Result<bool> {
        run(async move {
            let keyring = keyring().await?;
            keyring
                .search_items(&attributes(id))
                .await
                .map(|items| !items.is_empty())
                .map_err(|_| service_error("check for a saved password"))
        })
    }

    fn set(&self, id: &str, password: &str) -> io::Result<()> {
        let password = password.to_owned();
        run(async move {
            let keyring = keyring().await?;
            keyring
                .create_item(
                    "Fyle remote password",
                    &attributes(id),
                    oo7::Secret::text(password),
                    true,
                )
                .await
                .map_err(|_| service_error("save the password"))
        })
    }

    fn delete(&self, id: &str) -> io::Result<()> {
        run(async move {
            let keyring = keyring().await?;
            keyring
                .delete(&attributes(id))
                .await
                .map_err(|_| service_error("remove the saved password"))
        })
    }
}

impl CredentialStore for SystemCredentialStore {
    fn get(&self, id: &str) -> io::Result<Option<String>> {
        LinuxCredentialStore.get(id)
    }

    fn has(&self, id: &str) -> io::Result<bool> {
        LinuxCredentialStore.has(id)
    }

    fn get_cancellable(&self, id: &str, cancel: &AtomicBool) -> io::Result<Option<String>> {
        LinuxCredentialStore.get_cancellable(id, cancel)
    }

    fn set(&self, id: &str, password: &str) -> io::Result<()> {
        LinuxCredentialStore.set(id, password)
    }

    fn delete(&self, id: &str) -> io::Result<()> {
        LinuxCredentialStore.delete(id)
    }
}

#[cfg(not(target_os = "linux"))]
impl CredentialStore for LinuxCredentialStore {
    fn get(&self, _id: &str) -> io::Result<Option<String>> {
        Err(unsupported())
    }

    fn has(&self, _id: &str) -> io::Result<bool> {
        Err(unsupported())
    }

    fn set(&self, _id: &str, _password: &str) -> io::Result<()> {
        Err(unsupported())
    }

    fn delete(&self, _id: &str) -> io::Result<()> {
        Err(unsupported())
    }
}

#[cfg(not(target_os = "linux"))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "saved remote passwords are currently supported on Linux only",
    )
}

/// Creates a fresh opaque keyring identifier for a password replacement.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::new_id;

    #[test]
    fn generated_ids_are_opaque_unique_uuids() {
        let first = new_id();
        let second = new_id();
        assert_ne!(first, second);
        assert!(uuid::Uuid::parse_str(&first).is_ok());
        assert!(uuid::Uuid::parse_str(&second).is_ok());
    }
}
