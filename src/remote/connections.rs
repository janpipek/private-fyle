//! The user's `remotes.toml`: connections to SFTP, FTP/FTPS, WebDAV and S3
//! servers, parsed into validated [`RemoteConfig`]s without touching the
//! network. Passwords are never stored here; `password_command` or
//! `credential_id` selects an external source for them.
//!
//! ```toml
//! [[remote]]
//! name = "nas"                       # panel prefix: nas:/
//! protocol = "sftp"                  # sftp | ftp | ftps | webdav | s3
//! host = "nas.local"
//! port = 22                          # optional
//! user = "jan"                       # optional
//! root = "/srv/data"                 # optional folder that nas:/ maps to
//! password_command = "pass show nas" # optional
//! credential_id = "opaque UUID"    # optional, keyring lookup only
//! ```
//!
//! WebDAV uses `url` instead of `host`; S3 uses `bucket` with optional
//! `region`, `profile` and `endpoint`.

use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::Location;

/// Which protocol a remote speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Sftp,
    Ftp,
    Ftps,
    Webdav,
    S3,
}

impl Protocol {
    /// Human-readable name for labels.
    pub fn label(self) -> &'static str {
        match self {
            Self::Sftp => "SFTP",
            Self::Ftp => "FTP",
            Self::Ftps => "FTPS",
            Self::Webdav => "WebDAV",
            Self::S3 => "Amazon S3",
        }
    }
}

/// One validated `[[remote]]` entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteConfig {
    /// Panel prefix; a valid [`Location::root`] name.
    pub name: String,
    pub protocol: Protocol,
    /// Server name (sftp, ftp, ftps).
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    /// Absolute folder on the server that `<name>:/` maps to ("/" default).
    pub root: String,
    /// Program and arguments that print the password on stdout.
    pub password_command: Option<Vec<String>>,
    /// Opaque identifier for a password stored in the system keyring.
    pub credential_id: Option<String>,
    /// WebDAV base URL.
    pub url: Option<String>,
    pub bucket: Option<String>,
    pub region: Option<String>,
    /// AWS profile for S3 credentials.
    pub profile: Option<String>,
    /// Custom S3 endpoint (e.g. MinIO).
    pub endpoint: Option<String>,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    remote: Vec<toml::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    name: String,
    protocol: Protocol,
    host: Option<String>,
    port: Option<u16>,
    user: Option<String>,
    root: Option<String>,
    password_command: Option<String>,
    credential_id: Option<String>,
    url: Option<String>,
    bucket: Option<String>,
    region: Option<String>,
    profile: Option<String>,
    endpoint: Option<String>,
}

/// Where `remotes.toml` lives: next to `config.toml`.
pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("fyle").join("remotes.toml"))
}

/// Loads the default `remotes.toml`; a missing file is no error.
pub fn load_default() -> (Vec<RemoteConfig>, Vec<io::Error>) {
    match default_path() {
        Some(path) => load(&path),
        None => (Vec::new(), Vec::new()),
    }
}

/// Loads `path`. A missing file gives no remotes and no errors; bad entries
/// are reported without dropping the good ones.
pub fn load(path: &Path) -> (Vec<RemoteConfig>, Vec<io::Error>) {
    let prefixed = |e: io::Error| io::Error::new(e.kind(), format!("{}: {e}", path.display()));
    match std::fs::read_to_string(path) {
        Ok(text) => match parse(&text) {
            Ok((remotes, errors)) => (remotes, errors.into_iter().map(prefixed).collect()),
            Err(error) => (Vec::new(), vec![prefixed(error)]),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => (Vec::new(), Vec::new()),
        Err(error) => (Vec::new(), vec![prefixed(error)]),
    }
}

/// Parses the text of a `remotes.toml`. The outer error means the TOML
/// itself is broken; the returned list holds per-entry errors.
pub fn parse(text: &str) -> io::Result<(Vec<RemoteConfig>, Vec<io::Error>)> {
    let file: File =
        toml::from_str(text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let mut remotes: Vec<RemoteConfig> = Vec::new();
    let mut errors = Vec::new();
    for value in file.remote {
        let result = value
            .try_into::<Raw>()
            .map_err(|e| invalid(&e.to_string()))
            .and_then(validate);
        match result {
            Ok(config) if remotes.iter().any(|r| r.name == config.name) => errors.push(invalid(
                &format!("remote {}: name is used twice", config.name),
            )),
            Ok(config)
                if config.credential_id.is_some()
                    && remotes
                        .iter()
                        .any(|r| r.credential_id == config.credential_id) =>
            {
                errors.push(invalid(&format!(
                    "remote {}: credential_id is used more than once",
                    config.name
                )))
            }
            Ok(config) => remotes.push(config),
            Err(error) => errors.push(error),
        }
    }
    Ok((remotes, errors))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

fn validate(raw: Raw) -> io::Result<RemoteConfig> {
    let name = raw.name.clone();
    let fail = |message: &str| invalid(&format!("remote {name}: {message}"));
    Location::root(&raw.name)
        .map_err(|_| fail("invalid name (use 2+ letters, digits, - _ . + @)"))?;
    let need = |value: &Option<String>, key: &str| {
        value
            .as_deref()
            .filter(|v| !v.trim().is_empty())
            .map(|_| ())
            .ok_or_else(|| fail(&format!("{} needs {key}", raw.protocol.label())))
    };
    match raw.protocol {
        Protocol::Sftp | Protocol::Ftp | Protocol::Ftps => need(&raw.host, "host")?,
        Protocol::Webdav => {
            need(&raw.url, "url")?;
            let url = raw.url.as_deref().unwrap_or_default();
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(fail("url must start with http:// or https://"));
            }
        }
        Protocol::S3 => need(&raw.bucket, "bucket")?,
    }
    let root = normalize_root(raw.root.as_deref().unwrap_or("/")).map_err(fail)?;
    let password_command = raw
        .password_command
        .as_deref()
        .map(|text| split_command(text).map_err(fail))
        .transpose()?;
    let credential_id = raw
        .credential_id
        .map(|id| {
            uuid::Uuid::parse_str(&id)
                .map(|uuid| uuid.to_string())
                .map_err(|_| fail("credential_id must be an opaque UUID"))
        })
        .transpose()?;
    if password_command.is_some() && credential_id.is_some() {
        return Err(fail(
            "use either password_command or credential_id, not both",
        ));
    }
    Ok(RemoteConfig {
        name: raw.name,
        protocol: raw.protocol,
        host: raw.host,
        port: raw.port,
        user: raw.user,
        root,
        password_command,
        credential_id,
        url: raw.url,
        bucket: raw.bucket,
        region: raw.region,
        profile: raw.profile,
        endpoint: raw.endpoint,
    })
}

impl RemoteConfig {
    /// Reports whether two configurations address the same endpoint and
    /// account, so saved credentials are not silently reused after edits.
    pub fn credential_identity_eq(&self, other: &Self) -> bool {
        if self.protocol != other.protocol || self.user != other.user {
            return false;
        }
        match self.protocol {
            Protocol::Sftp | Protocol::Ftp | Protocol::Ftps => {
                self.host == other.host && self.port == other.port
            }
            Protocol::Webdav => self.url == other.url,
            Protocol::S3 => {
                self.bucket == other.bucket
                    && self.region == other.region
                    && self.profile == other.profile
                    && self.endpoint == other.endpoint
            }
        }
    }
}

/// Makes `root` an absolute path without `.`, `..` or empty parts.
fn normalize_root(root: &str) -> Result<String, &'static str> {
    if !root.starts_with('/') {
        return Err("root must be an absolute path starting with /");
    }
    let mut parts = Vec::new();
    for part in root.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err("root must not contain .."),
            part if part.contains('\0') => return Err("root contains a NUL character"),
            part => parts.push(part),
        }
    }
    Ok(format!("/{}", parts.join("/")))
}

/// Splits a command line into words without a shell: words are separated by
/// whitespace, and single or double quotes group a word (no escapes, no
/// expansion, no pipes). Fails on an unterminated quote or an empty command.
pub fn split_command(text: &str) -> Result<Vec<String>, &'static str> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, c) => {
                word.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err("password_command has an unterminated quote");
    }
    if started {
        words.push(word);
    }
    if words.is_empty() {
        return Err("password_command is empty");
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::{Protocol, load, normalize_root, parse, split_command};

    const SAMPLE: &str = r#"
[[remote]]
name = "nas"
protocol = "sftp"
host = "nas.local"
port = 2222
user = "jan"
root = "/srv//data/"
password_command = "pass show 'my nas'"

[[remote]]
name = "cloud"
protocol = "webdav"
url = "https://dav.example.com/remote.php/webdav"
user = "jan"

[[remote]]
name = "bucket"
protocol = "s3"
bucket = "photos"
region = "eu-west-1"
profile = "work"
"#;

    #[test]
    fn parses_all_protocols() {
        let (remotes, errors) = parse(SAMPLE).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(remotes.len(), 3);
        assert_eq!(remotes[0].protocol, Protocol::Sftp);
        assert_eq!(remotes[0].port, Some(2222));
        assert_eq!(remotes[0].root, "/srv/data");
        assert_eq!(
            remotes[0].password_command,
            Some(vec!["pass".into(), "show".into(), "my nas".into()])
        );
        assert_eq!(remotes[1].root, "/");
        assert_eq!(remotes[2].bucket.as_deref(), Some("photos"));
    }

    #[test]
    fn credential_ids_are_exclusive_and_identity_ignores_name_and_root() {
        let id = uuid::Uuid::new_v4();
        let source = format!(
            "[[remote]]\nname='nas'\nprotocol='sftp'\nhost='server'\ncredential_id='{id}'\n"
        );
        let (first, errors) = parse(&source).unwrap();
        assert!(errors.is_empty());
        let renamed = source
            .replace("name='nas'", "name='archive'")
            .replace("credential_id", "root")
            .replace(&format!("'{id}'"), "'/different'");
        let (second, errors) = parse(&renamed).unwrap();
        assert!(errors.is_empty());
        assert!(first[0].credential_identity_eq(&second[0]));

        let duplicate = format!(
            "{source}\n{}",
            source.replace("name='nas'", "name='archive'")
        );
        let (configs, errors) = parse(&duplicate).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(errors.len(), 1);

        let conflict = source.replace(
            "credential_id=",
            "password_command='pass show nas'\ncredential_id=",
        );
        let (configs, errors) = parse(&conflict).unwrap();
        assert!(configs.is_empty());
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn bad_entries_are_reported_without_dropping_good_ones() {
        let text = r#"
[[remote]]
name = "ok"
protocol = "ftp"
host = "h"

[[remote]]
name = "x"
protocol = "ftp"
host = "h"

[[remote]]
name = "nohost"
protocol = "sftp"

[[remote]]
name = "badurl"
protocol = "webdav"
url = "ftp://x"

[[remote]]
name = "ok"
protocol = "ftp"
host = "again"

[[remote]]
name = "typo"
protocol = "ftp"
host = "h"
pasword = "x"

[[remote]]
name = "relroot"
protocol = "ftps"
host = "h"
root = "data"

[[remote]]
name = "other"
protocol = "gopher"
host = "h"

[[remote]]
name = "nobucket"
protocol = "s3"
"#;
        let (remotes, errors) = parse(text).unwrap();
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].name, "ok");
        assert_eq!(errors.len(), 8, "{errors:?}");
        assert!(errors.iter().any(|e| e.to_string().contains("nohost")));
    }

    #[test]
    fn the_shipped_example_is_valid_once_uncommented() {
        let text = include_str!("../../remotes.example.toml")
            .replace("#| ", "")
            .replace("#|", "");
        let (remotes, errors) = parse(&text).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        let protocols: Vec<_> = remotes.iter().map(|r| r.protocol).collect();
        assert_eq!(
            protocols,
            [
                Protocol::Sftp,
                Protocol::Ftps,
                Protocol::Webdav,
                Protocol::S3
            ]
        );
        assert!(
            parse(include_str!("../../remotes.example.toml"))
                .unwrap()
                .0
                .is_empty()
        );
    }

    #[test]
    fn broken_toml_and_missing_files() {
        assert!(parse("[[remote").is_err());
        assert!(parse("").unwrap().0.is_empty());
        let dir = tempfile::TempDir::new().unwrap();
        let missing = load(&dir.path().join("remotes.toml"));
        assert!(missing.0.is_empty() && missing.1.is_empty());
        let path = dir.path().join("remotes.toml");
        std::fs::write(&path, "[[remote").unwrap();
        let (remotes, errors) = load(&path);
        assert!(remotes.is_empty());
        assert!(errors[0].to_string().contains("remotes.toml"));
        std::fs::write(&path, SAMPLE).unwrap();
        assert_eq!(load(&path).0.len(), 3);
    }

    #[test]
    fn splits_commands_like_simple_shell_words() {
        assert_eq!(
            split_command("pass show nas").unwrap(),
            ["pass", "show", "nas"]
        );
        assert_eq!(
            split_command(r#"  secret-tool lookup "host name" 'a b' "" "#).unwrap(),
            ["secret-tool", "lookup", "host name", "a b", ""]
        );
        assert_eq!(split_command("a\"b c\"d").unwrap(), ["ab cd"]);
        assert!(split_command("pass 'oops").is_err());
        assert!(split_command("   ").is_err());
    }

    #[test]
    fn normalizes_roots() {
        assert_eq!(normalize_root("/"), Ok("/".to_owned()));
        assert_eq!(normalize_root("/a/./b//"), Ok("/a/b".to_owned()));
        assert!(normalize_root("a").is_err());
        assert!(normalize_root("/a/../b").is_err());
    }
}
