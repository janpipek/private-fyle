//! Editing and persistence rules for user-managed `remotes.toml` entries.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

use super::connections::{self, Protocol, RemoteConfig};
pub mod credentials;
pub use credentials::{CredentialAction, save_with_credential};

/// Editable inputs supported by the remote manager.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftField {
    /// Network host.
    Host,
    /// Network port.
    Port,
    /// Account name.
    User,
    /// Remote root folder.
    Root,
    /// Password printing command.
    PasswordCommand,
    /// WebDAV base URL.
    Url,
    /// S3 bucket.
    Bucket,
    /// S3 region.
    Region,
    /// AWS credentials profile.
    Profile,
    /// Custom S3 endpoint.
    Endpoint,
}

/// Returns the fields applicable to a protocol identifier.
pub fn fields_for_protocol(protocol: &str) -> &'static [DraftField] {
    use DraftField as F;
    match protocol {
        "sftp" | "ftp" | "ftps" => &[F::Host, F::Port, F::User, F::Root, F::PasswordCommand],
        "webdav" => &[F::Url, F::User, F::Root, F::PasswordCommand],
        "s3" => &[F::Bucket, F::Region, F::Profile, F::Endpoint, F::Root],
        _ => &[],
    }
}

/// Reports whether a draft differs from its saved remote; a new draft is dirty.
pub fn is_dirty(draft: &Draft, saved: Option<&RemoteConfig>) -> bool {
    saved.is_none_or(|saved| {
        draft.validate().map_or_else(
            |_| Draft::from_config(saved) != *draft,
            |current| current != *saved,
        )
    })
}

/// Editable fields for one remote. The model deliberately stores strings so
/// incomplete drafts can be edited before validation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Draft {
    /// Panel prefix for the remote.
    pub name: String,
    /// Provider protocol identifier.
    pub protocol: String,
    /// Host name for SFTP, FTP or FTPS.
    pub host: String,
    /// Optional network port.
    pub port: String,
    /// Optional login name.
    pub user: String,
    /// Absolute folder exposed as the remote root.
    pub root: String,
    /// Command that prints the password when connecting.
    pub password_command: String,
    /// Opaque keyring identifier retained from the selected remote.
    pub credential_id: Option<String>,
    /// Base URL for WebDAV.
    pub url: String,
    /// Bucket name for S3.
    pub bucket: String,
    /// S3 region.
    pub region: String,
    /// AWS profile name for S3.
    pub profile: String,
    /// Custom S3 endpoint.
    pub endpoint: String,
}

impl Draft {
    /// Creates a form draft from a validated configuration.
    pub fn from_config(config: &RemoteConfig) -> Self {
        Self {
            name: config.name.clone(),
            protocol: match config.protocol {
                Protocol::Sftp => "sftp",
                Protocol::Ftp => "ftp",
                Protocol::Ftps => "ftps",
                Protocol::Webdav => "webdav",
                Protocol::S3 => "s3",
            }
            .to_owned(),
            host: config.host.clone().unwrap_or_default(),
            port: config.port.map(|port| port.to_string()).unwrap_or_default(),
            user: config.user.clone().unwrap_or_default(),
            root: config.root.clone(),
            password_command: config
                .password_command
                .as_ref()
                .map(|parts| quote_words(parts))
                .unwrap_or_default(),
            credential_id: config.credential_id.clone(),
            url: config.url.clone().unwrap_or_default(),
            bucket: config.bucket.clone().unwrap_or_default(),
            region: config.region.clone().unwrap_or_default(),
            profile: config.profile.clone().unwrap_or_default(),
            endpoint: config.endpoint.clone().unwrap_or_default(),
        }
    }

    /// Validates and converts this draft through the production parser.
    pub fn validate(&self) -> io::Result<RemoteConfig> {
        let active = self.active_fields();
        if fields_for_protocol(&self.protocol).contains(&DraftField::Port)
            && !active.port.trim().is_empty()
            && !active.port.trim().parse::<u16>().is_ok_and(|port| port > 0)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "port must be a number from 1 to 65535",
            ));
        }
        let text = active.to_toml();
        let (mut configs, mut errors) = connections::parse(&text)?;
        if let Some(error) = errors.pop() {
            return Err(error);
        }
        configs
            .pop()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "remote is incomplete"))
    }

    fn active_fields(&self) -> Self {
        let allowed = fields_for_protocol(&self.protocol);
        let mut active = self.clone();
        for (field, value) in [
            (DraftField::Host, &mut active.host),
            (DraftField::Port, &mut active.port),
            (DraftField::User, &mut active.user),
            (DraftField::Root, &mut active.root),
            (DraftField::PasswordCommand, &mut active.password_command),
            (DraftField::Url, &mut active.url),
            (DraftField::Bucket, &mut active.bucket),
            (DraftField::Region, &mut active.region),
            (DraftField::Profile, &mut active.profile),
            (DraftField::Endpoint, &mut active.endpoint),
        ] {
            if !allowed.contains(&field) {
                value.clear();
            }
        }
        active
    }

    fn to_toml(&self) -> String {
        let mut table = toml_edit::Table::new();
        table["name"] = value(&self.name);
        table["protocol"] = value(&self.protocol);
        for (key, field) in [
            ("host", &self.host),
            ("user", &self.user),
            ("root", &self.root),
            ("password_command", &self.password_command),
            ("url", &self.url),
            ("bucket", &self.bucket),
            ("region", &self.region),
            ("profile", &self.profile),
            ("endpoint", &self.endpoint),
        ] {
            if !field.trim().is_empty() {
                table[key] = value(field);
            }
        }
        if let Some(id) = &self.credential_id {
            table["credential_id"] = value(id);
        }
        if !self.port.trim().is_empty()
            && let Ok(port) = self.port.trim().parse::<i64>()
        {
            table["port"] = toml_edit::value(port);
        }
        format!("[[remote]]\n{table}")
    }
}

/// Formats password command words using the quoting rules accepted by its parser.
pub fn quote_words(words: &[String]) -> String {
    words
        .iter()
        .map(|word| {
            word.split('\'')
                .map(|part| format!("'{part}'"))
                .collect::<Vec<_>>()
                .join("\"'\"")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn value(text: &str) -> toml_edit::Item {
    toml_edit::value(text)
}

/// Reads the exact current file text, treating a missing file as empty.
pub fn read(path: &Path) -> io::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error),
    }
}

/// Saves configurations while retaining the table at `old_name` when a
/// selected remote is renamed.
/// The exact baseline text prevents overwriting edits made by another process.
pub fn save_with_rename(
    path: &Path,
    baseline: &str,
    configs: &[RemoteConfig],
    rename: Option<(&str, &str)>,
) -> io::Result<()> {
    let current = read(path)?;
    if current != baseline {
        return Err(io::Error::other(
            "remotes.toml changed on disk; reload before saving",
        ));
    }
    let mut doc = if current.trim().is_empty() {
        toml_edit::DocumentMut::new()
    } else {
        current
            .parse::<toml_edit::DocumentMut>()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
    };
    let desired: HashMap<String, RemoteConfig> = configs
        .iter()
        .cloned()
        .map(|config| (config.name.clone(), config))
        .collect();
    let mut written = HashSet::new();
    let mut tables = toml_edit::ArrayOfTables::new();
    if let Some(old_tables) = doc
        .get("remote")
        .and_then(toml_edit::Item::as_array_of_tables)
    {
        for old in old_tables.iter() {
            let parsed = connections::parse(&format!("[[remote]]\n{old}"));
            let old_config = parsed.ok().and_then(|(mut parsed, errors)| {
                if errors.is_empty() {
                    parsed.pop()
                } else {
                    None
                }
            });
            if let Some(old_config) = old_config {
                let old_name = old_config.name.as_str();
                let name = rename
                    .filter(|(from, _)| *from == old_name)
                    .map_or(old_name, |(_, to)| to);
                if written.contains(name) {
                    tables.push(old.clone());
                } else if let Some(config) = desired.get(name) {
                    let mut table = old.clone();
                    update_table(&mut table, config);
                    tables.push(table);
                    written.insert(name.to_owned());
                }
            } else {
                // Keep malformed entries intact so valid remote edits do not
                // silently erase configuration the manager cannot understand.
                tables.push(old.clone());
            }
        }
    }
    for config in configs
        .iter()
        .filter(|config| !written.contains(&config.name))
    {
        let mut table = toml_edit::Table::new();
        update_table(&mut table, config);
        tables.push(table);
    }
    doc["remote"] = toml_edit::Item::ArrayOfTables(tables);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = temp_path(path);
    std::fs::write(&temporary, doc.to_string())?;
    if read(path)? != baseline {
        let _ = std::fs::remove_file(&temporary);
        return Err(io::Error::other(
            "remotes.toml changed on disk while saving; reload before saving",
        ));
    }
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn update_table(table: &mut toml_edit::Table, config: &RemoteConfig) {
    set_value(
        table,
        "name",
        Some(toml_edit::Value::from(config.name.as_str())),
    );
    let protocol = match config.protocol {
        Protocol::Sftp => "sftp",
        Protocol::Ftp => "ftp",
        Protocol::Ftps => "ftps",
        Protocol::Webdav => "webdav",
        Protocol::S3 => "s3",
    };
    set_value(table, "protocol", Some(toml_edit::Value::from(protocol)));
    set_value(
        table,
        "host",
        config.host.as_deref().map(toml_edit::Value::from),
    );
    set_value(
        table,
        "port",
        config
            .port
            .map(|port| toml_edit::Value::from(i64::from(port))),
    );
    set_value(
        table,
        "user",
        config.user.as_deref().map(toml_edit::Value::from),
    );
    set_value(
        table,
        "root",
        Some(toml_edit::Value::from(config.root.as_str())),
    );
    let password_command = config
        .password_command
        .as_ref()
        .map(|parts| quote_words(parts));
    set_value(
        table,
        "password_command",
        password_command.as_deref().map(toml_edit::Value::from),
    );
    set_value(
        table,
        "credential_id",
        config.credential_id.as_deref().map(toml_edit::Value::from),
    );
    set_value(
        table,
        "url",
        config.url.as_deref().map(toml_edit::Value::from),
    );
    set_value(
        table,
        "bucket",
        config.bucket.as_deref().map(toml_edit::Value::from),
    );
    set_value(
        table,
        "region",
        config.region.as_deref().map(toml_edit::Value::from),
    );
    set_value(
        table,
        "profile",
        config.profile.as_deref().map(toml_edit::Value::from),
    );
    set_value(
        table,
        "endpoint",
        config.endpoint.as_deref().map(toml_edit::Value::from),
    );
}

fn set_value(table: &mut toml_edit::Table, key: &str, value: Option<toml_edit::Value>) {
    match value {
        Some(mut replacement) => {
            if let Some(previous) = table.get(key).and_then(toml_edit::Item::as_value) {
                *replacement.decor_mut() = previous.decor().clone();
            }
            table[key] = toml_edit::Item::Value(replacement);
        }
        None => {
            table.remove(key);
        }
    }
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.tmp", std::process::id()));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::{Draft, DraftField as F, fields_for_protocol, read, save_with_rename};
    use crate::remote::connections;

    #[test]
    fn draft_round_trips_through_connection_validation() {
        let (configs, errors) =
            connections::parse("[[remote]]\nname='nas'\nprotocol='sftp'\nhost='host'\n").unwrap();
        assert!(errors.is_empty());
        let draft = Draft::from_config(&configs[0]);
        assert_eq!(draft.validate().unwrap(), configs[0]);
    }

    #[test]
    fn draft_rejects_invalid_ports() {
        let valid = "[[remote]]\nname='nas'\nprotocol='sftp'\nhost='server'\n";
        let (configs, _) = connections::parse(valid).unwrap();
        for port in ["abc", "0", "65536"] {
            let mut draft = Draft::from_config(&configs[0]);
            draft.port = port.to_owned();
            assert!(draft.validate().is_err(), "accepted port {port}");
        }
    }

    #[test]
    fn protocol_field_policy_hides_unrelated_inputs_and_ignores_hidden_values() {
        assert_eq!(
            fields_for_protocol("sftp"),
            &[F::Host, F::Port, F::User, F::Root, F::PasswordCommand]
        );
        assert_eq!(
            fields_for_protocol("webdav"),
            &[F::Url, F::User, F::Root, F::PasswordCommand]
        );
        assert_eq!(
            fields_for_protocol("s3"),
            &[F::Bucket, F::Region, F::Profile, F::Endpoint, F::Root]
        );

        let mut draft = Draft {
            name: "cloud".into(),
            protocol: "webdav".into(),
            url: "https://example.test/dav".into(),
            port: "not a port".into(),
            host: "hidden host".into(),
            ..Draft::default()
        };
        let config = draft.validate().unwrap();
        assert_eq!(config.url.as_deref(), Some("https://example.test/dav"));
        assert_eq!(config.host, None);
        assert_eq!(config.port, None);

        draft.protocol = "s3".into();
        draft.bucket = "bucket".into();
        assert_eq!(draft.validate().unwrap().bucket.as_deref(), Some("bucket"));
    }

    #[test]
    fn dirty_policy_compares_the_editable_draft_to_saved_settings() {
        let (configs, errors) =
            connections::parse("[[remote]]\nname='nas'\nprotocol='sftp'\nhost='server'\n").unwrap();
        assert!(errors.is_empty());
        let draft = Draft::from_config(&configs[0]);
        assert!(!super::is_dirty(&draft, Some(&configs[0])));
        let mut edited = draft.clone();
        edited.host = "elsewhere".into();
        assert!(super::is_dirty(&edited, Some(&configs[0])));
        assert!(super::is_dirty(&draft, None));

        let mut switched = draft;
        switched.protocol = "s3".into();
        switched.bucket = "bucket".into();
        switched.port = "not a port".into();
        switched.host = "retained for switching back".into();
        let saved_s3 = switched.validate().unwrap();
        assert!(!super::is_dirty(&switched, Some(&saved_s3)));
    }

    #[test]
    fn password_command_round_trips_empty_and_mixed_quote_arguments() {
        let words = vec![String::new(), "a b".into(), "it's \"fine\"".into()];
        let encoded = super::quote_words(&words);
        assert_eq!(connections::split_command(&encoded).unwrap(), words);
    }

    #[test]
    fn save_preserves_unrelated_comments_and_rejects_external_edits() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remotes.toml");
        let source = "# my remotes\n[[remote]]\nname='nas' # keep name note\nprotocol='sftp'\nhost='host' # keep host note\n\n[[remote]]\nname='broken'\nprotocol='sftp'\n";
        std::fs::write(&path, source).unwrap();
        let (configs, errors) = connections::parse(source).unwrap();
        assert_eq!(errors.len(), 1);
        save_with_rename(&path, source, &configs, None).unwrap();
        assert!(read(&path).unwrap().contains("# my remotes"));
        let saved = read(&path).unwrap();
        assert!(saved.contains("# keep name note"));
        assert!(saved.contains("# keep host note"));
        assert!(saved.contains("name='broken'"));
        let changed = read(&path).unwrap();
        std::fs::write(&path, format!("{changed}\n# external\n")).unwrap();
        assert!(save_with_rename(&path, &changed, &configs, None).is_err());
    }

    #[test]
    fn add_edit_rename_and_remove_keep_other_remotes_and_comments() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remotes.toml");
        let first_text =
            "[[remote]]\nname='nas' # keep this comment\nprotocol='sftp'\nhost='old-host'\n";
        let (first, errors) = connections::parse(first_text).unwrap();
        assert!(errors.is_empty());
        save_with_rename(&path, "", &first, None).unwrap();
        assert_eq!(connections::parse(&read(&path).unwrap()).unwrap().0, first);
        std::fs::write(&path, first_text).unwrap();

        let baseline = read(&path).unwrap();
        let mut renamed = first[0].clone();
        renamed.name = "archive".into();
        renamed.host = Some("new-host".into());
        let (second, errors) = connections::parse(
            "[[remote]]\nname='cloud'\nprotocol='webdav'\nurl='https://example.test'\n",
        )
        .unwrap();
        assert!(errors.is_empty());
        save_with_rename(
            &path,
            &baseline,
            &[renamed, second[0].clone()],
            Some(("nas", "archive")),
        )
        .unwrap();
        let changed = read(&path).unwrap();
        assert!(changed.contains("# keep this comment"));
        let (configs, errors) = connections::parse(&changed).unwrap();
        assert!(errors.is_empty());
        assert_eq!(
            configs
                .iter()
                .map(|config| config.name.as_str())
                .collect::<Vec<_>>(),
            ["archive", "cloud"]
        );
        assert_eq!(configs[0].host.as_deref(), Some("new-host"));

        save_with_rename(&path, &changed, &[second[0].clone()], None).unwrap();
        let (configs, errors) = connections::parse(&read(&path).unwrap()).unwrap();
        assert!(errors.is_empty());
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "cloud");
    }

    #[test]
    fn invalid_duplicate_name_before_valid_entry_is_preserved() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remotes.toml");
        let source = "[[remote]]\nname='nas'\nprotocol='sftp'\n\n[[remote]]\nname='nas'\nprotocol='sftp'\nhost='server'\n";
        std::fs::write(&path, source).unwrap();
        let (configs, errors) = connections::parse(source).unwrap();
        assert_eq!(errors.len(), 1);
        save_with_rename(&path, source, &configs, None).unwrap();
        let saved = read(&path).unwrap();
        assert_eq!(saved.matches("[[remote]]").count(), 2);
        let (preserved, errors) = connections::parse(&saved).unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(preserved.len(), 1);
        assert_eq!(preserved[0].host.as_deref(), Some("server"));
    }
}
