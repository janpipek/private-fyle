//! Saved-password changes coordinated with remote configuration persistence.

use std::io;
use std::path::Path;

use super::save_with_rename;
use crate::remote::connections::{self, RemoteConfig};
use crate::remote::credentials::{self, CredentialStore};

/// The saved-password change requested with a remote configuration save.
pub enum CredentialAction {
    /// Keep the current password, requiring unchanged endpoint/account identity.
    Keep,
    /// Store a new password and assign it a fresh opaque identifier.
    Replace(String),
    /// Remove the saved-password reference from the remote.
    Forget,
}

/// Cleanup information after the configuration has been saved successfully.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CredentialSaveOutcome {
    /// The config is saved, but an obsolete keyring item could not be deleted.
    pub cleanup_warning: Option<String>,
}

/// Saves one remote and its password without putting secret text in config.
/// The password is stored first; a failed config save deletes the new item.
/// The old item is removed only after the new config has been persisted.
pub fn save_with_credential(
    store: &dyn CredentialStore,
    path: &Path,
    baseline: &str,
    configs: &mut [RemoteConfig],
    rename: Option<(&str, &str)>,
    remote_name: &str,
    action: CredentialAction,
) -> io::Result<CredentialSaveOutcome> {
    let (saved, _entry_errors) = connections::parse(baseline)?;
    let Some(index) = configs.iter().position(|config| config.name == remote_name) else {
        if !matches!(&action, CredentialAction::Forget) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "remote to save was not found",
            ));
        }
        let old_id = saved
            .iter()
            .find(|saved| saved.name == remote_name)
            .and_then(|saved| saved.credential_id.clone());
        save_with_rename(path, baseline, configs, rename)?;
        return Ok(CredentialSaveOutcome {
            cleanup_warning: delete_old_if_unused(store, old_id.as_deref(), configs),
        });
    };
    let old_name = match rename {
        Some((from, to)) if to == remote_name => from,
        _ => remote_name,
    };
    let old_config = saved.iter().find(|saved| saved.name == old_name);
    let old_id = old_config.and_then(|saved| saved.credential_id.clone());
    let new_has_password_command = configs[index].password_command.is_some();
    let cleanup_warning = match action {
        CredentialAction::Keep => {
            if old_id.is_some() && new_has_password_command {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "clear password_command before keeping a saved password",
                ));
            }
            if old_id.is_some()
                && let Some(saved) = old_config
            {
                if !saved.credential_identity_eq(&configs[index]) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "remote endpoint or account changed; enter a new password or forget the saved one",
                    ));
                }
                configs[index].credential_id = old_id.clone();
            } else {
                configs[index].credential_id = None;
            }
            save_with_rename(path, baseline, configs, rename)?;
            None
        }
        CredentialAction::Replace(password) => {
            if new_has_password_command {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "clear password_command before saving a password",
                ));
            }
            let new_id = credentials::new_id();
            store.set(&new_id, &password)?;
            configs[index].credential_id = Some(new_id.clone());
            if let Err(error) = save_with_rename(path, baseline, configs, rename) {
                let rollback = store.delete(&new_id);
                return match rollback {
                    Ok(()) => Err(error),
                    Err(_) => Err(io::Error::other(format!(
                        "{}; the new keyring item could not be removed",
                        error
                    ))),
                };
            }
            delete_old_if_unused(store, old_id.as_deref(), configs)
        }
        CredentialAction::Forget => {
            configs[index].credential_id = None;
            save_with_rename(path, baseline, configs, rename)?;
            delete_old_if_unused(store, old_id.as_deref(), configs)
        }
    };
    Ok(CredentialSaveOutcome { cleanup_warning })
}

fn delete_old_if_unused(
    store: &dyn CredentialStore,
    old_id: Option<&str>,
    configs: &[RemoteConfig],
) -> Option<String> {
    let old_id = old_id?;
    if configs
        .iter()
        .any(|config| config.credential_id.as_deref() == Some(old_id))
    {
        return None;
    }
    store
        .delete(old_id)
        .err()
        .map(|_| "configuration saved, but the old keyring password could not be deleted".into())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io;
    use std::sync::Mutex;

    use super::{CredentialAction, save_with_credential};
    use crate::remote::connections;
    use crate::remote::credentials::CredentialStore;
    use crate::remote::manage::read;

    #[derive(Default)]
    struct MemoryStore {
        values: Mutex<HashMap<String, String>>,
        fail_set: bool,
        fail_delete: bool,
    }

    impl CredentialStore for MemoryStore {
        fn get(&self, id: &str) -> io::Result<Option<String>> {
            Ok(self.values.lock().unwrap().get(id).cloned())
        }

        fn has(&self, id: &str) -> io::Result<bool> {
            Ok(self.values.lock().unwrap().contains_key(id))
        }

        fn set(&self, id: &str, password: &str) -> io::Result<()> {
            if self.fail_set {
                return Err(io::Error::other("injected store failure"));
            }
            self.values
                .lock()
                .unwrap()
                .insert(id.to_owned(), password.to_owned());
            Ok(())
        }

        fn delete(&self, id: &str) -> io::Result<()> {
            if self.fail_delete {
                return Err(io::Error::other("injected store failure"));
            }
            self.values.lock().unwrap().remove(id);
            Ok(())
        }
    }

    #[test]
    fn replacement_saves_only_an_id_and_forget_removes_the_keyring_item() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remotes.toml");
        let baseline = "[[remote]]\nname='nas'\nprotocol='sftp'\nhost='server'\n";
        std::fs::write(&path, baseline).unwrap();
        let (mut configs, errors) = connections::parse(baseline).unwrap();
        assert!(errors.is_empty());
        let store = MemoryStore::default();
        let outcome = save_with_credential(
            &store,
            &path,
            baseline,
            &mut configs,
            None,
            "nas",
            CredentialAction::Replace("private password".into()),
        )
        .unwrap();
        assert_eq!(outcome.cleanup_warning, None);
        let id = configs[0].credential_id.clone().unwrap();
        assert_eq!(store.get(&id).unwrap().as_deref(), Some("private password"));
        let saved = read(&path).unwrap();
        assert!(saved.contains(&id));
        assert!(!saved.contains("private password"));

        let baseline = saved;
        let (mut configs, errors) = connections::parse(&baseline).unwrap();
        assert!(errors.is_empty());
        let outcome = save_with_credential(
            &store,
            &path,
            &baseline,
            &mut configs,
            None,
            "nas",
            CredentialAction::Forget,
        )
        .unwrap();
        assert_eq!(outcome.cleanup_warning, None);
        assert!(!store.has(&id).unwrap());
        assert!(!read(&path).unwrap().contains("credential_id"));
    }

    #[test]
    fn failed_config_save_rolls_back_new_keyring_item() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remotes.toml");
        let baseline = "[[remote]]\nname='nas'\nprotocol='sftp'\nhost='server'\n";
        std::fs::write(&path, format!("{baseline}\n# changed externally\n")).unwrap();
        let (mut configs, errors) = connections::parse(baseline).unwrap();
        assert!(errors.is_empty());
        let store = MemoryStore::default();
        assert!(
            save_with_credential(
                &store,
                &path,
                baseline,
                &mut configs,
                None,
                "nas",
                CredentialAction::Replace("private password".into()),
            )
            .is_err()
        );
        assert!(store.values.lock().unwrap().is_empty());
    }

    #[test]
    fn keep_rejects_changed_endpoint_identity() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remotes.toml");
        let baseline = format!(
            "[[remote]]\nname='nas'\nprotocol='sftp'\nhost='server'\ncredential_id='{}'\n",
            uuid::Uuid::new_v4()
        );
        std::fs::write(&path, &baseline).unwrap();
        let (mut configs, errors) = connections::parse(&baseline).unwrap();
        assert!(errors.is_empty());
        configs[0].host = Some("other-server".into());
        assert!(
            save_with_credential(
                &MemoryStore::default(),
                &path,
                &baseline,
                &mut configs,
                None,
                "nas",
                CredentialAction::Keep,
            )
            .is_err()
        );
    }

    #[test]
    fn keep_preserves_legacy_password_commands_when_endpoint_changes_or_renames() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remotes.toml");
        let baseline = "[[remote]]\nname='nas'\nprotocol='sftp'\nhost='server'\npassword_command='pass show nas'\n";
        std::fs::write(&path, baseline).unwrap();
        let (mut configs, errors) = connections::parse(baseline).unwrap();
        assert!(errors.is_empty());
        configs[0].name = "archive".into();
        configs[0].host = Some("other-server".into());
        let outcome = save_with_credential(
            &MemoryStore::default(),
            &path,
            baseline,
            &mut configs,
            Some(("nas", "archive")),
            "archive",
            CredentialAction::Keep,
        )
        .unwrap();
        assert_eq!(outcome.cleanup_warning, None);
        let (saved, errors) = connections::parse(&read(&path).unwrap()).unwrap();
        assert!(errors.is_empty());
        assert_eq!(saved[0].host.as_deref(), Some("other-server"));
        assert!(saved[0].credential_id.is_none());
        assert_eq!(
            saved[0].password_command,
            Some(vec!["pass".into(), "show".into(), "nas".into()])
        );
    }

    #[test]
    fn failed_replacement_preserves_old_id_and_password() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remotes.toml");
        let old_id = uuid::Uuid::new_v4().to_string();
        let baseline = format!(
            "[[remote]]\nname='nas'\nprotocol='sftp'\nhost='server'\ncredential_id='{old_id}'\n"
        );
        std::fs::write(&path, format!("{baseline}\n# external edit\n")).unwrap();
        let (mut configs, errors) = connections::parse(&baseline).unwrap();
        assert!(errors.is_empty());
        let store = MemoryStore::default();
        store.set(&old_id, "old password").unwrap();
        assert!(
            save_with_credential(
                &store,
                &path,
                &baseline,
                &mut configs,
                None,
                "nas",
                CredentialAction::Replace("new password".into()),
            )
            .is_err()
        );
        assert_eq!(store.get(&old_id).unwrap().as_deref(), Some("old password"));
        assert_eq!(store.values.lock().unwrap().len(), 1);
        assert!(read(&path).unwrap().contains(&old_id));
    }

    #[test]
    fn keyring_write_failure_leaves_config_untouched() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remotes.toml");
        let baseline = "[[remote]]\nname='nas'\nprotocol='sftp'\nhost='server'\n";
        std::fs::write(&path, baseline).unwrap();
        let (mut configs, errors) = connections::parse(baseline).unwrap();
        assert!(errors.is_empty());
        let store = MemoryStore {
            fail_set: true,
            ..MemoryStore::default()
        };
        assert!(
            save_with_credential(
                &store,
                &path,
                baseline,
                &mut configs,
                None,
                "nas",
                CredentialAction::Replace("new password".into()),
            )
            .is_err()
        );
        assert_eq!(read(&path).unwrap(), baseline);
        assert!(configs[0].credential_id.is_none());
    }

    #[test]
    fn cleanup_failure_reports_saved_config_and_warning() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remotes.toml");
        let old_id = uuid::Uuid::new_v4().to_string();
        let baseline = format!(
            "[[remote]]\nname='nas'\nprotocol='sftp'\nhost='server'\ncredential_id='{old_id}'\n"
        );
        std::fs::write(&path, &baseline).unwrap();
        let (mut configs, errors) = connections::parse(&baseline).unwrap();
        assert!(errors.is_empty());
        let store = MemoryStore {
            fail_delete: true,
            ..MemoryStore::default()
        };
        let outcome = save_with_credential(
            &store,
            &path,
            &baseline,
            &mut configs,
            None,
            "nas",
            CredentialAction::Forget,
        )
        .unwrap();
        assert!(outcome.cleanup_warning.is_some());
        assert!(!read(&path).unwrap().contains("credential_id"));
    }

    #[test]
    fn keep_preserves_credential_id_through_rename() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remotes.toml");
        let id = uuid::Uuid::new_v4().to_string();
        let baseline = format!(
            "[[remote]]\nname='nas'\nprotocol='sftp'\nhost='server'\ncredential_id='{id}'\n"
        );
        std::fs::write(&path, &baseline).unwrap();
        let (mut configs, errors) = connections::parse(&baseline).unwrap();
        assert!(errors.is_empty());
        configs[0].name = "archive".into();
        save_with_credential(
            &MemoryStore::default(),
            &path,
            &baseline,
            &mut configs,
            Some(("nas", "archive")),
            "archive",
            CredentialAction::Keep,
        )
        .unwrap();
        assert_eq!(configs[0].credential_id.as_deref(), Some(id.as_str()));
        assert!(read(&path).unwrap().contains(&id));
    }

    #[test]
    fn removed_remote_keeps_secret_until_config_removal_succeeds() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remotes.toml");
        let id = uuid::Uuid::new_v4().to_string();
        let baseline = format!(
            "[[remote]]\nname='nas'\nprotocol='sftp'\nhost='server'\ncredential_id='{id}'\n"
        );
        std::fs::write(&path, format!("{baseline}\n# external edit\n")).unwrap();
        let store = MemoryStore::default();
        store.set(&id, "old password").unwrap();
        assert!(
            save_with_credential(
                &store,
                &path,
                &baseline,
                &mut [],
                None,
                "nas",
                CredentialAction::Forget,
            )
            .is_err()
        );
        assert_eq!(store.get(&id).unwrap().as_deref(), Some("old password"));

        std::fs::write(&path, &baseline).unwrap();
        let outcome = save_with_credential(
            &store,
            &path,
            &baseline,
            &mut [],
            None,
            "nas",
            CredentialAction::Forget,
        )
        .unwrap();
        assert_eq!(outcome.cleanup_warning, None);
        assert!(!store.has(&id).unwrap());
    }

    #[test]
    fn save_keeps_malformed_sibling_entries() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remotes.toml");
        let baseline = "[[remote]]\nname='nas'\nprotocol='sftp'\nhost='server'\n\n[[remote]]\nname='broken'\nprotocol='sftp'\n";
        std::fs::write(&path, baseline).unwrap();
        let (mut configs, errors) = connections::parse(baseline).unwrap();
        assert_eq!(errors.len(), 1);
        configs[0].root = "/data".into();
        save_with_credential(
            &MemoryStore::default(),
            &path,
            baseline,
            &mut configs,
            None,
            "nas",
            CredentialAction::Keep,
        )
        .unwrap();
        let saved = read(&path).unwrap();
        assert!(saved.contains("name='broken'"));
        assert!(saved.contains("root = \"/data\""));
    }
}
