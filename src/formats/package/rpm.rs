//! RPM metadata header decoding, independent of payload compression and package databases.
//!
//! Layout: https://rpm-software-management.github.io/rpm/manual/format_header.html

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

use super::{Budget, Field, Format, METADATA_LIMIT, Metadata, invalid};

const INDEX_LIMIT: usize = 4096;
const MAGIC: &[u8; 8] = b"\x8e\xad\xe8\x01\0\0\0\0";

/// Reads selected RPM header facts without opening the payload.
pub fn read(file: &mut File, budget: &Budget<'_>) -> io::Result<Metadata> {
    let lead = budget.bytes(file, 96)?;
    if !lead.starts_with(b"\xed\xab\xee\xdb") || !matches!(lead[4], 3 | 4) {
        return Err(invalid("unsupported RPM lead"));
    }
    let (count, size) = intro(file, budget)?;
    let signature_size = 16 + count * 16 + size;
    let next = 96 + (signature_size as u64).div_ceil(8) * 8;
    if next > file.metadata()?.len() {
        return Err(invalid("truncated RPM signature header"));
    }
    file.seek(SeekFrom::Start(next))?;
    let (count, size) = intro(file, budget)?;
    let index = budget.bytes(file, count * 16)?;
    let data = budget.bytes(file, size)?;
    let mut result = Metadata::new(Format::Rpm);
    let mut seen = HashSet::new();
    for entry in index.as_chunks::<16>().0 {
        budget.check()?;
        let tag = integer(&entry[..4]);
        let kind = integer(&entry[4..8]);
        let offset = integer(&entry[8..12]) as usize;
        let count = integer(&entry[12..16]) as usize;
        let field = match tag {
            1000 => Some(Field::Name),
            1001 => Some(Field::Version),
            1002 => Some(Field::Release),
            1004 => Some(Field::Summary),
            1005 => Some(Field::Description),
            1011 => Some(Field::Vendor),
            1014 => Some(Field::License),
            1015 => Some(Field::Packager),
            1016 => Some(Field::Section),
            1020 => Some(Field::Homepage),
            1022 => Some(Field::Architecture),
            _ => None,
        };
        if field.is_none() && !matches!(tag, 1003 | 1009 | 5009) {
            continue;
        }
        if !seen.insert(tag) {
            return Err(invalid("duplicate selected RPM tag"));
        }
        let tail = data
            .get(offset..)
            .ok_or_else(|| invalid("RPM tag offset outside header"))?;
        if let Some(field) = field {
            if !matches!(kind, 6 | 9) || count == 0 || count > 128 || (kind == 6 && count != 1) {
                return Err(invalid("invalid RPM text tag type or count"));
            }
            // i18n strings use the first stored translation (normally the C locale).
            let end = tail
                .iter()
                .position(|byte| *byte == 0)
                .ok_or_else(|| invalid("unterminated RPM text tag"))?;
            let mut remaining = tail;
            for _ in 0..count {
                budget.check()?;
                let nul = remaining
                    .iter()
                    .position(|byte| *byte == 0)
                    .ok_or_else(|| invalid("truncated RPM translations"))?;
                remaining = &remaining[nul + 1..];
            }
            result.text(field, &tail[..end]);
        } else {
            let width = if tag == 5009 { 8 } else { 4 };
            if count != 1 || kind != if width == 8 { 5 } else { 4 } || !offset.is_multiple_of(width)
            {
                return Err(invalid("invalid RPM numeric tag type, count or alignment"));
            }
            let value = tail
                .get(..width)
                .ok_or_else(|| invalid("truncated RPM numeric tag"))?;
            let value = if width == 8 {
                u64::from_be_bytes(value.try_into().expect("eight-byte numeric tag"))
            } else {
                u64::from(integer(value))
            };
            match tag {
                1003 => result.epoch = Some(value),
                5009 => result.installed_size = Some(value),
                _ if !seen.contains(&5009) => result.installed_size = Some(value),
                _ => {}
            }
        }
    }
    result.validate()
}

fn intro(file: &mut impl Read, budget: &Budget<'_>) -> io::Result<(usize, usize)> {
    let bytes = budget.bytes(file, 16)?;
    if &bytes[..8] != MAGIC {
        return Err(invalid("invalid RPM header magic"));
    }
    let count = integer(&bytes[8..12]) as usize;
    let size = integer(&bytes[12..16]) as usize;
    if count > INDEX_LIMIT || size > METADATA_LIMIT {
        return Err(invalid("RPM header exceeds 4096 tags or 8 MiB"));
    }
    Ok((count, size))
}

fn integer(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().expect("four-byte header integer"))
}

#[cfg(test)]
mod tests {
    use super::{INDEX_LIMIT, MAGIC};
    use crate::formats::package::{Field, Format, METADATA_LIMIT, read};
    use std::sync::atomic::AtomicBool;
    use tempfile::TempDir;

    fn fixture() -> Vec<u8> {
        let fields: &[(u32, u32, &[u8])] = &[
            (1000, 6, b"demo\0"),
            (1001, 6, b"1.2\0"),
            (1002, 6, b"3.fc43\0"),
            (1004, 9, b"Example package\0"),
            (1005, 9, b"Long description\0"),
            (1014, 6, b"MIT\0"),
            (1022, 6, b"x86_64\0"),
            (1003, 4, &[0, 0, 0, 2]),
            (1009, 4, &[0, 0, 4, 0]),
            (5009, 5, &8589934592u64.to_be_bytes()),
        ];
        let mut data = Vec::new();
        let mut index = Vec::new();
        for (tag, kind, value) in fields {
            let align = match kind {
                4 => 4,
                5 => 8,
                _ => 1,
            };
            while !data.len().is_multiple_of(align) {
                data.push(0);
            }
            index.extend(tag.to_be_bytes());
            index.extend(kind.to_be_bytes());
            index.extend((data.len() as u32).to_be_bytes());
            index.extend(1u32.to_be_bytes());
            data.extend_from_slice(value);
        }
        let mut bytes = vec![0; 96];
        bytes[..5].copy_from_slice(b"\xed\xab\xee\xdb\x03");
        bytes.extend(MAGIC);
        bytes.extend([0; 8]);
        bytes.extend(MAGIC);
        bytes.extend((fields.len() as u32).to_be_bytes());
        bytes.extend((data.len() as u32).to_be_bytes());
        bytes.extend(index);
        bytes.extend(data);
        bytes
    }

    #[test]
    fn reads_header_only_including_epoch_and_64_bit_size() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("demo.rpm");
        std::fs::write(&path, fixture()).unwrap();
        let metadata = read(&path, Format::Rpm, &AtomicBool::new(false)).unwrap();
        assert_eq!(metadata.epoch, Some(2));
        assert_eq!(metadata.installed_size, Some(8589934592));
        assert!(metadata.fields.contains(&(Field::Name, "demo".into())));
        assert!(metadata.fields.contains(&(Field::License, "MIT".into())));
        assert!(!metadata.truncated);
    }

    #[test]
    fn rejects_corruption_and_allocation_bombs() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("bad.rpm");
        let original = fixture();
        for (offset, value) in [
            (120, (INDEX_LIMIT as u32 + 1)),
            (124, METADATA_LIMIT as u32 + 1),
            (136, u32::MAX),
            (140, 0),
            (132, 4),
        ] {
            let mut bytes = original.clone();
            bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
            std::fs::write(&path, bytes).unwrap();
            assert!(
                read(&path, Format::Rpm, &AtomicBool::new(false)).is_err(),
                "offset {offset}"
            );
        }
        for end in [0, 95, 111, original.len() - 1] {
            std::fs::write(&path, &original[..end]).unwrap();
            assert!(read(&path, Format::Rpm, &AtomicBool::new(false)).is_err());
        }
    }
}
