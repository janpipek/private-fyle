//! Versioned, lossless serialization for typed locations stored in text files.

use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::location::{ArchiveMember, Location, RemoteLocation};

/// Marker that makes a serialized location distinguishable from a legacy path.
pub const PREFIX: &str = "fyle-location:v1:";

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum WireLocation {
    Local {
        encoding: String,
        value: Vec<u16>,
    },
    Remote {
        name: String,
        parts: Vec<String>,
    },
    Archive {
        source: Box<WireLocation>,
        member: String,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u16,
    location: WireLocation,
}

/// Encodes a typed location as one text field with an explicit codec version.
/// Local path units are represented losslessly for the current platform.
pub fn encode(location: &Location) -> io::Result<String> {
    let document = Document {
        version: 1,
        location: to_wire(location)?,
    };
    let json = serde_json::to_string(&document).map_err(invalid_data)?;
    Ok(format!("{PREFIX}{json}"))
}

/// Decodes a value produced by [`encode`], rejecting unknown versions and
/// values that are invalid for the current platform or location model.
pub fn decode(value: &str) -> io::Result<Location> {
    let json = value.strip_prefix(PREFIX).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "unsupported location encoding")
    })?;
    let document: Document = serde_json::from_str(json).map_err(invalid_data)?;
    if document.version != 1 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("unsupported location codec version {}", document.version),
        ));
    }
    from_wire(document.location)
}

fn to_wire(location: &Location) -> io::Result<WireLocation> {
    match location {
        Location::Local(path) => {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                Ok(WireLocation::Local {
                    encoding: "unix_bytes".into(),
                    value: path
                        .as_path()
                        .as_os_str()
                        .as_bytes()
                        .iter()
                        .map(|b| u16::from(*b))
                        .collect(),
                })
            }
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStrExt;
                Ok(WireLocation::Local {
                    encoding: "windows_utf16".into(),
                    value: path.as_path().as_os_str().encode_wide().collect(),
                })
            }
            #[cfg(not(any(unix, windows)))]
            {
                let _ = path;
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "native path encoding is unsupported on this platform",
                ))
            }
        }
        Location::Remote(location) => Ok(WireLocation::Remote {
            name: location.remote().to_owned(),
            parts: location.parts().to_vec(),
        }),
        Location::Archive { source, member } => Ok(WireLocation::Archive {
            source: Box::new(to_wire(source)?),
            member: member.as_str().to_owned(),
        }),
    }
}

fn from_wire(wire: WireLocation) -> io::Result<Location> {
    match wire {
        WireLocation::Local { encoding, value } => {
            let path = match encoding.as_str() {
                "unix_bytes" => {
                    #[cfg(unix)]
                    {
                        use std::ffi::OsString;
                        use std::os::unix::ffi::OsStringExt;
                        let bytes = value
                            .into_iter()
                            .map(|unit| {
                                u8::try_from(unit)
                                    .map_err(|_| invalid_data("invalid Unix path byte"))
                            })
                            .collect::<io::Result<Vec<_>>>()?;
                        PathBuf::from(OsString::from_vec(bytes))
                    }
                    #[cfg(not(unix))]
                    {
                        return Err(io::Error::new(
                            io::ErrorKind::Unsupported,
                            "Unix path encoding is unsupported on this platform",
                        ));
                    }
                }
                "windows_utf16" => {
                    #[cfg(windows)]
                    {
                        use std::ffi::OsString;
                        use std::os::windows::ffi::OsStringExt;
                        PathBuf::from(OsString::from_wide(&value))
                    }
                    #[cfg(not(windows))]
                    {
                        return Err(io::Error::new(
                            io::ErrorKind::Unsupported,
                            "Windows path encoding is unsupported on this platform",
                        ));
                    }
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "unsupported native path encoding",
                    ));
                }
            };
            Location::local(path).map_err(invalid_data)
        }
        WireLocation::Remote { name, parts } => {
            let mut location = RemoteLocation::root(&name).map_err(invalid_data)?;
            for part in parts {
                location = location.child(&part).map_err(invalid_data)?;
            }
            Ok(Location::Remote(location))
        }
        WireLocation::Archive { source, member } => {
            let source = from_wire(*source)?;
            let member = ArchiveMember::new(&member).map_err(invalid_data)?;
            Ok(Location::Archive {
                source: Box::new(source),
                member,
            })
        }
    }
}

fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::PREFIX;
    use super::{decode, encode};
    use crate::location::{Location, RemoteLocation};
    #[cfg(unix)]
    use std::path::PathBuf;

    fn roundtrip(location: Location) {
        assert_eq!(decode(&encode(&location).unwrap()).unwrap(), location);
    }

    #[test]
    fn roundtrips_distinct_local_remote_and_recursive_archive_addresses() {
        #[cfg(unix)]
        let colon_path = Location::local(PathBuf::from("/mnt/nas:/x")).unwrap();
        let remote = Location::Remote(
            RemoteLocation::root("nas")
                .unwrap()
                .child("x\tline\nnext")
                .unwrap(),
        );
        #[cfg(unix)]
        assert_ne!(colon_path, remote);
        #[cfg(unix)]
        roundtrip(colon_path);
        roundtrip(remote);
        roundtrip(
            Location::archive(
                Location::archive(
                    Location::local(std::env::temp_dir().join("a.zip")).unwrap(),
                    "inner.zip",
                )
                .unwrap(),
                "folder/file\tname\nline",
            )
            .unwrap(),
        );
    }

    #[test]
    fn remote_and_archive_v1_encodings_remain_stable() {
        let remote = Location::Remote(RemoteLocation::root("nas").unwrap().child("docs").unwrap());
        let encoded_remote = format!(
            "{PREFIX}{{\"version\":1,\"location\":{{\"kind\":\"remote\",\"name\":\"nas\",\"parts\":[\"docs\"]}}}}"
        );
        assert_eq!(encode(&remote).unwrap(), encoded_remote);
        assert_eq!(decode(&encoded_remote).unwrap(), remote);

        let archive = Location::archive(
            Location::Remote(RemoteLocation::root("nas").unwrap()),
            "docs/readme.md",
        )
        .unwrap();
        let encoded_archive = format!(
            "{PREFIX}{{\"version\":1,\"location\":{{\"kind\":\"archive\",\"source\":{{\"kind\":\"remote\",\"name\":\"nas\",\"parts\":[]}},\"member\":\"docs/readme.md\"}}}}"
        );
        assert_eq!(encode(&archive).unwrap(), encoded_archive);
        assert_eq!(decode(&encoded_archive).unwrap(), archive);
    }

    #[cfg(unix)]
    #[test]
    fn roundtrips_non_utf8_and_control_character_native_paths() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from("/tmp").join(OsString::from_vec(vec![b'x', 0xff, b'\t', b'\n']));
        roundtrip(Location::local(path).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_unknown_versions_malformed_documents_and_invalid_addresses() {
        assert_eq!(
            decode("/tmp/legacy").unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert!(decode(&format!("{PREFIX}{{")).is_err());
        let future = format!(
            "{PREFIX}{{\"version\":2,\"location\":{{\"kind\":\"remote\",\"name\":\"nas\",\"parts\":[]}}}}"
        );
        assert_eq!(
            decode(&future).unwrap_err().kind(),
            std::io::ErrorKind::Unsupported
        );
        let invalid_member = format!(
            "{PREFIX}{{\"version\":1,\"location\":{{\"kind\":\"archive\",\"source\":{{\"kind\":\"local\",\"encoding\":\"unix_bytes\",\"value\":[47,116,109,112,47,97,46,122,105,112]}},\"member\":\"../escape\"}}}}"
        );
        assert!(decode(&invalid_member).is_err());
        let invalid_remote = format!(
            "{PREFIX}{{\"version\":1,\"location\":{{\"kind\":\"remote\",\"name\":\"x\",\"parts\":[]}}}}"
        );
        assert!(decode(&invalid_remote).is_err());
    }
}
