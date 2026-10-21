//! Lossless escaped SFTP names kept separate from literal new file names.

use std::ffi::OsStr;
use std::io;

/// Escapes a literal UTF-8 name or configured root for the SFTP wire boundary.
/// Listing-derived paths are already escaped and must not pass through this again.
pub fn escape_literal(name: &str) -> String {
    russh_sftp::wire_text::encode(name.as_bytes())
}

/// Represents a native upload name without discarding non-UTF-8 Unix bytes.
pub fn local_name(name: &OsStr) -> io::Result<String> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(russh_sftp::wire_text::encode(name.as_bytes()))
    }
    #[cfg(not(unix))]
    {
        name.to_str().map(escape_literal).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "local filename is not valid UTF-8",
            )
        })
    }
}

/// Restores exact server bytes from a listing-derived escaped SFTP path.
pub fn wire_path(path: &str) -> Vec<u8> {
    russh_sftp::wire_text::decode(path)
}

/// Formats an SFTP URL path from the server bytes rather than its escaped label.
pub fn url_path(path: &str) -> String {
    use std::fmt::Write as _;
    let mut encoded = String::new();
    for byte in wire_path(path) {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").expect("formatting into a String cannot fail");
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{escape_literal, local_name, url_path, wire_path};

    #[test]
    fn literal_marker_names_cannot_alias_non_utf8_names() {
        let raw_name = russh_sftp::wire_text::encode(b"caf\xe9.txt");
        let marker_name = escape_literal("caf~xE9.txt");
        let replacement_name = escape_literal("caf�.txt");
        assert_eq!(raw_name, "caf~xE9.txt");
        assert_eq!(marker_name, "caf~x7ExE9.txt");
        assert_ne!(raw_name, marker_name);
        assert_ne!(raw_name, replacement_name);
        assert_eq!(wire_path(&raw_name), b"caf\xe9.txt");
        assert_eq!(wire_path(&marker_name), b"caf~xE9.txt");
        assert_eq!(wire_path(&replacement_name), "caf�.txt".as_bytes());
    }

    #[test]
    fn unicode_and_structural_bytes_keep_their_meaning() {
        let path = "/příliš/../žluťoučký.txt";
        assert_eq!(escape_literal(path), path);
        assert_eq!(wire_path(path), path.as_bytes());
        for text in ["~x2F", "~x2E", "~x00", "~x5C", "~x7F"] {
            assert_eq!(wire_path(text), text.as_bytes());
            crate::location::remote::validate_entry_name(text).unwrap();
        }
        let malicious = russh_sftp::wire_text::encode(b"../\xff");
        assert!(crate::location::remote::validate_entry_name(&malicious).is_err());
    }

    #[test]
    fn standard_urls_use_original_bytes_and_escape_url_metacharacters() {
        assert_eq!(url_path("/caf~xE9 a#.txt"), "/caf%E9%20a%23.txt");
        assert_eq!(url_path("/~x7ExE9.txt"), "/~xE9.txt");
        assert_eq!(url_path("/příliš.txt"), "/p%C5%99%C3%ADli%C5%A1.txt");
    }

    #[test]
    fn wire_codec_preserves_all_byte_values_and_multibyte_error_boundaries() {
        for first in 0..=u8::MAX {
            for second in 0..=u8::MAX {
                let raw = [b'a', first, second, b'~', b'x', b'F', b'F'];
                let encoded = russh_sftp::wire_text::encode(&raw);
                assert_eq!(wire_path(&encoded), raw);
            }
        }
    }

    #[test]
    fn actual_sftp_filename_and_data_packets_retain_exact_bytes() {
        use russh_sftp::protocol::{Data, File, FileAttributes, Name, OpenDir};
        let raw = b"caf\xe9~xFF.txt";
        let name = russh_sftp::wire_text::encode(raw);
        let packet = Name {
            id: 7,
            files: vec![File {
                filename: name.clone(),
                longname: String::new(),
                attrs: FileAttributes::empty(),
            }],
        };
        let mut wire = russh_sftp::ser::to_bytes(&packet).unwrap();
        assert_eq!(&wire[8..12], &(raw.len() as u32).to_be_bytes());
        assert_eq!(&wire[12..12 + raw.len()], raw);
        let received: Name = russh_sftp::de::from_bytes(&mut wire).unwrap();
        assert_eq!(received.files[0].filename, name);
        let request = OpenDir {
            id: 8,
            path: format!("/parent/{}", received.files[0].filename),
        };
        let wire = russh_sftp::ser::to_bytes(&request).unwrap();
        let mut expected = b"/parent/".to_vec();
        expected.extend_from_slice(raw);
        assert_eq!(&wire[8..], expected);

        let payload = vec![0, 0xff, b'~', b'x', b'8', b'0'];
        let mut wire = russh_sftp::ser::to_bytes(&Data {
            id: 9,
            data: payload.clone(),
        })
        .unwrap();
        assert_eq!(&wire[8..], payload);
        let received: Data = russh_sftp::de::from_bytes(&mut wire).unwrap();
        assert_eq!(received.data, payload);
    }

    #[cfg(unix)]
    #[test]
    fn local_upload_names_preserve_native_bytes() {
        use std::os::unix::ffi::OsStrExt;
        for raw in [
            b"caf\xe9.txt".as_slice(),
            b"literal~xE9",
            "příliš".as_bytes(),
        ] {
            let encoded = local_name(std::ffi::OsStr::from_bytes(raw)).unwrap();
            assert_eq!(wire_path(&encoded), raw);
        }
    }
}
