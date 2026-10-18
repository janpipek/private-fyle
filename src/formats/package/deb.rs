//! Debian ar/control.tar metadata parsing without payload extraction or script execution.
//!
//! Layout: https://manpages.debian.org/dpkg-dev/deb.5.en.html

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom};

use super::{Budget, Field, Format, METADATA_LIMIT, Metadata, invalid};

const CONTROL_LIMIT: usize = 256 * 1024;

/// Reads a Debian control archive without opening its data payload.
pub fn read(file: &mut File, budget: &Budget<'_>) -> io::Result<Metadata> {
    if budget.bytes(file, 8)? != b"!<arch>\n" {
        return Err(invalid("invalid Debian ar magic"));
    }
    let length = file.metadata()?.len();
    for member in 0..32 {
        let header = budget.bytes(file, 60)?;
        if &header[58..] != b"`\n" {
            return Err(invalid("invalid Debian ar member header"));
        }
        let name = std::str::from_utf8(&header[..16])
            .map_err(|_| invalid("invalid ar name"))?
            .trim_end()
            .trim_end_matches('/');
        let size = std::str::from_utf8(&header[48..58])
            .map_err(|_| invalid("invalid ar size"))?
            .trim()
            .parse::<u64>()
            .map_err(|_| invalid("invalid ar size"))?;
        let start = file.stream_position()?;
        let end = start
            .checked_add(size)
            .ok_or_else(|| invalid("ar member size overflow"))?;
        if end > length {
            return Err(invalid("truncated Debian ar member"));
        }
        if member == 0 {
            if name != "debian-binary" || size > 64 {
                return Err(invalid("missing Debian format marker"));
            }
            let version = budget.bytes(file, size as usize)?;
            let version = std::str::from_utf8(&version)
                .map_err(|_| invalid("invalid Debian format version"))?;
            let first = version.lines().next().unwrap_or_default();
            let (major, minor) = first
                .split_once('.')
                .ok_or_else(|| invalid("invalid Debian format version"))?;
            if major != "2" || minor.is_empty() || !minor.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(invalid("unsupported Debian package version"));
            }
        } else if name.starts_with("control.tar") {
            if size > METADATA_LIMIT as u64 {
                return Err(invalid("compressed Debian control archive exceeds 8 MiB"));
            }
            let bytes = budget.bytes(file, size as usize)?;
            let source = Guarded {
                inner: Cursor::new(bytes),
                budget,
            };
            let mut decoder: Box<dyn Read + '_> = match name {
                "control.tar" => Box::new(source),
                "control.tar.gz" => Box::new(flate2::read::MultiGzDecoder::new(source)),
                "control.tar.xz" => {
                    Box::new(lzma_rust2::XzReader::new_mem_limit(source, true, 32 * 1024))
                }
                "control.tar.zst" => {
                    let mut decoder = zstd::stream::read::Decoder::new(source)?;
                    decoder.window_log_max(25)?;
                    Box::new(decoder)
                }
                _ => return Err(invalid("unsupported Debian control compression")),
            };
            let archive = bounded(&mut decoder, METADATA_LIMIT, budget)?;
            return control_archive(&archive, budget);
        } else if !name.starts_with('_') {
            return Err(invalid("expected Debian control archive"));
        }
        file.seek(SeekFrom::Start(end + size % 2))?;
    }
    Err(invalid(
        "Debian control archive not found within 32 members",
    ))
}

struct Guarded<'a, 'b, R> {
    inner: R,
    budget: &'a Budget<'b>,
}

impl<R: Read> Read for Guarded<'_, '_, R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        // Decoders can retry Interrupted forever; the outer package read restores its kind.
        self.budget.check().map_err(io::Error::other)?;
        let limit = bytes.len().min(8192);
        let count = self.inner.read(&mut bytes[..limit])?;
        self.budget.check().map_err(io::Error::other)?;
        Ok(count)
    }
}

fn bounded(reader: &mut dyn Read, limit: usize, budget: &Budget<'_>) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        budget.check()?;
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        if bytes.len() + count > limit {
            return Err(invalid("Debian metadata exceeds decompressed size limit"));
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    budget.check()?;
    Ok(bytes)
}

fn control_archive(bytes: &[u8], budget: &Budget<'_>) -> io::Result<Metadata> {
    let mut archive = tar::Archive::new(Cursor::new(bytes));
    let mut control = None;
    for (index, entry) in archive.entries()?.enumerate() {
        budget.check()?;
        if index >= 128 {
            return Err(invalid("Debian control archive exceeds 128 entries"));
        }
        let mut entry = entry?;
        if matches!(entry.path_bytes().as_ref(), b"control" | b"./control") {
            if control.is_some()
                || !entry.header().entry_type().is_file()
                || entry.size() > CONTROL_LIMIT as u64
            {
                return Err(invalid(
                    "invalid, duplicate or oversized Debian control file",
                ));
            }
            control = Some(bounded(&mut entry, CONTROL_LIMIT, budget)?);
        }
    }
    parse_control(
        &control.ok_or_else(|| invalid("Debian control file not found"))?,
        budget,
    )
}

fn parse_control(bytes: &[u8], budget: &Budget<'_>) -> io::Result<Metadata> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("Debian control is not UTF-8"))?;
    let mut fields: HashMap<String, String> = HashMap::new();
    let mut current = None;
    let mut ended = false;
    for line in text.lines() {
        budget.check()?;
        if line.is_empty() {
            ended |= current.is_some();
            continue;
        }
        if ended {
            return Err(invalid("multiple Debian control paragraphs"));
        }
        if line.starts_with([' ', '\t']) {
            let key = current
                .as_ref()
                .ok_or_else(|| invalid("orphan Debian continuation line"))?;
            let value = fields.get_mut(key).expect("current field was inserted");
            let continuation = &line[1..];
            value.push('\n');
            if continuation != "." {
                value.push_str(continuation);
            }
        } else {
            let (key, value) = line
                .split_once(':')
                .ok_or_else(|| invalid("invalid Debian control field"))?;
            if key.is_empty() || !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
                return Err(invalid("invalid Debian control field name"));
            }
            let key = key.to_ascii_lowercase();
            if fields.len() >= 128 || fields.insert(key.clone(), value.trim().into()).is_some() {
                return Err(invalid("too many or duplicate Debian control fields"));
            }
            current = Some(key);
        }
    }
    let mut result = Metadata::new(Format::Deb);
    for (key, field) in [
        ("package", Field::Name),
        ("version", Field::Version),
        ("architecture", Field::Architecture),
        ("maintainer", Field::Maintainer),
        ("homepage", Field::Homepage),
        ("license", Field::License),
        ("section", Field::Section),
    ] {
        if let Some(value) = fields.get(key) {
            result.text(field, value.as_bytes());
        }
    }
    if let Some(description) = fields.get("description") {
        let (summary, body) = description.split_once('\n').unwrap_or((description, ""));
        result.text(Field::Summary, summary.as_bytes());
        result.text(Field::Description, body.as_bytes());
    }
    if let Some(size) = fields.get("installed-size") {
        result.installed_size = Some(
            size.trim()
                .parse::<u64>()
                .ok()
                .and_then(|size| size.checked_mul(1024))
                .ok_or_else(|| invalid("invalid or overflowing Debian installed size"))?,
        );
    }
    result.validate()
}

#[cfg(test)]
mod tests {
    use super::{Budget, CONTROL_LIMIT, control_archive, parse_control};
    use crate::formats::package::{Field, Format, METADATA_LIMIT, TEXT_LIMIT, read};
    use std::io::Write;
    use std::sync::atomic::AtomicBool;
    use tempfile::TempDir;

    const CONTROL: &[u8] = b"Package: demo\nVersion: 2:1.2-3\nArchitecture: amd64\nMaintainer: Example <demo@example.org>\nInstalled-Size: 42\nHomepage: https://example.org\nDescription: Short summary\n Long description\n .\n Second paragraph\n";

    fn tar(control: &[u8]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(control.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "./control", control)
            .unwrap();
        builder.into_inner().unwrap()
    }

    fn ar(name: &str, data: &[u8]) -> Vec<u8> {
        let mut bytes = b"!<arch>\n".to_vec();
        for (name, data) in [("debian-binary", b"2.0\n".as_slice()), (name, data)] {
            let header = format!(
                "{name:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
                0,
                0,
                0,
                "100644",
                data.len()
            );
            assert_eq!(header.len(), 60);
            bytes.extend(header.as_bytes());
            bytes.extend(data);
            if !data.len().is_multiple_of(2) {
                bytes.push(b'\n');
            }
        }
        bytes
    }

    #[test]
    fn reads_plain_gzip_xz_and_zstd_without_payloads() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("demo.deb");
        let tar = tar(CONTROL);
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&tar).unwrap();
        // A real XZ stream produced offline from the same control tar; no optional encoder/program needed.
        let xz = xz_fixture();
        for (name, bytes) in [
            ("control.tar", tar.clone()),
            ("control.tar.gz", gz.finish().unwrap()),
            ("control.tar.xz", xz),
            (
                "control.tar.zst",
                zstd::stream::encode_all(tar.as_slice(), 1).unwrap(),
            ),
        ] {
            std::fs::write(&path, ar(name, &bytes)).unwrap();
            let result = read(&path, Format::Deb, &AtomicBool::new(false)).unwrap();
            assert!(
                result.fields.contains(&(Field::Name, "demo".into())),
                "{name}"
            );
            assert_eq!(result.installed_size, Some(42 * 1024));
            assert!(result.fields.contains(&(
                Field::Description,
                "Long description\n\nSecond paragraph".into()
            )));
        }
    }

    fn xz_fixture() -> Vec<u8> {
        XZ_FIXTURE.to_vec()
    }
    const XZ_FIXTURE: &[u8] = &[
        253, 55, 122, 88, 90, 0, 0, 4, 230, 214, 180, 70, 2, 0, 33, 1, 22, 0, 0, 0, 116, 47, 229,
        163, 224, 39, 255, 0, 251, 93, 0, 23, 11, 200, 231, 217, 229, 209, 195, 213, 149, 125, 66,
        219, 220, 14, 92, 108, 212, 89, 18, 242, 21, 108, 97, 169, 45, 140, 216, 248, 140, 22, 159,
        48, 208, 74, 139, 134, 0, 60, 244, 89, 170, 27, 73, 73, 180, 188, 22, 230, 184, 197, 145,
        171, 159, 226, 213, 103, 59, 121, 131, 227, 232, 82, 84, 214, 183, 205, 37, 238, 225, 147,
        63, 89, 164, 30, 129, 246, 28, 119, 36, 163, 32, 127, 221, 149, 252, 91, 58, 244, 40, 23,
        171, 10, 29, 90, 68, 151, 113, 60, 116, 95, 212, 35, 207, 118, 78, 69, 196, 116, 0, 210,
        59, 197, 159, 55, 5, 57, 142, 204, 229, 53, 44, 79, 174, 144, 158, 148, 103, 170, 230, 6,
        188, 207, 31, 97, 193, 122, 96, 18, 55, 175, 177, 134, 47, 24, 100, 133, 83, 196, 112, 113,
        172, 101, 181, 104, 97, 130, 125, 58, 225, 139, 82, 48, 135, 67, 253, 160, 38, 82, 92, 82,
        244, 76, 236, 49, 5, 43, 113, 98, 206, 224, 91, 97, 229, 83, 154, 5, 239, 248, 160, 153,
        233, 110, 241, 147, 32, 123, 167, 159, 10, 171, 97, 131, 117, 130, 45, 151, 11, 48, 36,
        225, 57, 243, 93, 183, 169, 175, 116, 190, 15, 40, 44, 164, 220, 46, 168, 62, 46, 169, 24,
        146, 109, 70, 129, 100, 63, 47, 202, 242, 56, 191, 98, 176, 1, 119, 118, 245, 38, 96, 198,
        0, 0, 0, 73, 85, 123, 123, 145, 88, 210, 126, 0, 1, 151, 2, 128, 80, 0, 0, 205, 57, 19, 56,
        177, 196, 103, 251, 2, 0, 0, 0, 0, 4, 89, 90,
    ];

    #[test]
    fn cancellation_during_decoder_input_does_not_trigger_interrupted_retries() {
        use std::io::{self, Cursor, Read};
        use std::sync::atomic::Ordering;
        struct CancelOnRead<'a> {
            input: Cursor<Vec<u8>>,
            cancel: &'a AtomicBool,
        }
        impl Read for CancelOnRead<'_> {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                let count = self.input.read(bytes)?;
                self.cancel.store(true, Ordering::Relaxed);
                Ok(count)
            }
        }
        let cancel = AtomicBool::new(false);
        let budget = Budget::new(&cancel);
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"control content").unwrap();
        let source = CancelOnRead {
            input: Cursor::new(encoder.finish().unwrap()),
            cancel: &cancel,
        };
        let mut decoder = flate2::read::GzDecoder::new(super::Guarded {
            inner: source,
            budget: &budget,
        });
        assert_ne!(
            decoder.read(&mut [0; 32]).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        assert_eq!(
            budget.check().unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
    }

    #[test]
    fn rejects_missing_control_unsupported_compression_and_truncated_members() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("bad.deb");
        for bytes in [
            ar("control.tar.bz2", b"not supported"),
            ar("data.tar", b"payload"),
            ar("control.tar", b"broken tar"),
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(read(&path, Format::Deb, &AtomicBool::new(false)).is_err());
        }
        let mut bytes = ar("control.tar", &tar(CONTROL));
        bytes.truncate(bytes.len() - 4);
        std::fs::write(&path, bytes).unwrap();
        assert!(read(&path, Format::Deb, &AtomicBool::new(false)).is_err());
        std::fs::write(&path, ar("control.tar.xz", LARGE_DICTIONARY_XZ)).unwrap();
        let error = read(&path, Format::Deb, &AtomicBool::new(false)).unwrap_err();
        assert!(error.to_string().contains("mem_limit"), "{error}");
    }

    const LARGE_DICTIONARY_XZ: &[u8] = &[
        253, 55, 122, 88, 90, 0, 0, 4, 230, 214, 180, 70, 2, 0, 33, 1, 28, 0, 0, 0, 16, 207, 88,
        204, 224, 39, 255, 0, 116, 93, 0, 49, 155, 202, 25, 219, 173, 239, 188, 26, 220, 79, 173,
        198, 133, 79, 238, 133, 171, 250, 71, 212, 247, 200, 33, 133, 209, 183, 78, 205, 46, 100,
        164, 49, 56, 81, 52, 57, 89, 190, 110, 155, 189, 119, 35, 169, 22, 23, 187, 159, 196, 124,
        220, 26, 65, 115, 122, 244, 135, 226, 115, 23, 167, 173, 215, 168, 238, 39, 209, 84, 27,
        145, 122, 243, 49, 206, 19, 235, 237, 160, 154, 163, 171, 6, 82, 55, 70, 193, 71, 226, 116,
        97, 60, 244, 138, 246, 172, 172, 144, 124, 237, 99, 21, 239, 131, 82, 35, 197, 116, 243,
        207, 112, 55, 37, 209, 0, 0, 0, 177, 67, 70, 214, 196, 233, 50, 202, 0, 1, 144, 1, 128, 80,
        0, 0, 165, 115, 182, 98, 177, 196, 103, 251, 2, 0, 0, 0, 0, 4, 89, 90,
    ];

    #[test]
    fn rejects_bad_control_and_bounds_text_and_decompression() {
        let cancel = AtomicBool::new(false);
        let budget = Budget::new(&cancel);
        for data in [
            b"Package: demo\npackage: other\nVersion: 1".as_slice(),
            b" orphan",
            b"Package: demo\nVersion: 1\nInstalled-Size: 18446744073709551615",
            b"Package: demo\n",
            b"invalid",
        ] {
            assert!(parse_control(data, &budget).is_err());
        }
        let input = format!(
            "Package: demo\nVersion: 1\nDescription: {}",
            "x".repeat(TEXT_LIMIT + 1)
        );
        assert!(parse_control(input.as_bytes(), &budget).unwrap().truncated);
        assert!(control_archive(&tar(&vec![b'x'; CONTROL_LIMIT + 1]), &budget).is_err());
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&vec![0; METADATA_LIMIT + 1]).unwrap();
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("bomb.deb");
        std::fs::write(&path, ar("control.tar.gz", &gz.finish().unwrap())).unwrap();
        assert!(read(&path, Format::Deb, &cancel).is_err());
        assert_eq!(
            read(&path, Format::Deb, &AtomicBool::new(true))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::Interrupted
        );
    }
}
