//! Bounded Parquet footer decoding shared by preview and Properties.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};

use parquet::file::metadata::{ParquetMetaData, ParquetMetaDataReader};

/// Maximum serialized Parquet footer read by metadata consumers.
pub const MAX_METADATA_BYTES: u64 = 8 * 1024 * 1024;

/// Validated location and size of the serialized footer metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FooterBounds {
    /// Start offset of the serialized metadata.
    pub offset: u64,
    /// Serialized metadata byte length.
    pub size: u64,
}

/// Reads and decodes a Parquet footer after validating its file bounds.
///
/// Metadata decoding is synchronous and cannot be interrupted. The 8 MiB
/// serialized footer cap does not cap the decoded metadata's memory expansion.
pub fn read_metadata(file: &mut File, cancel: Option<&AtomicBool>) -> io::Result<ParquetMetaData> {
    let bounds = check_footer_bounds(file, cancel)?;
    check_cancel(cancel)?;
    let mut bytes = vec![0; usize::try_from(bounds.size).map_err(invalid)?];
    file.seek(SeekFrom::Start(bounds.offset))?;
    for chunk in bytes.chunks_mut(64 * 1024) {
        check_cancel(cancel)?;
        file.read_exact(chunk)?;
    }
    check_cancel(cancel)?;
    let metadata = ParquetMetaDataReader::decode_metadata(&bytes).map_err(invalid)?;
    check_cancel(cancel)?;
    Ok(metadata)
}

/// Validates the footer magic and the serialized metadata bounds without decoding it.
pub fn check_footer_bounds(
    file: &mut File,
    cancel: Option<&AtomicBool>,
) -> io::Result<FooterBounds> {
    check_cancel(cancel)?;
    let size = file.metadata()?.len();
    if size < 12 {
        return Err(invalid("Invalid Parquet file: missing footer"));
    }
    file.seek(SeekFrom::End(-8))?;
    let mut footer = [0; 8];
    file.read_exact(&mut footer)?;
    if &footer[4..] != b"PAR1" {
        return Err(invalid("Invalid or encrypted Parquet footer"));
    }
    let metadata_size = u64::from(u32::from_le_bytes(
        footer[..4].try_into().expect("four footer bytes"),
    ));
    if metadata_size > MAX_METADATA_BYTES {
        return Err(invalid("Parquet metadata exceeds the 8 MiB footer limit"));
    }
    if metadata_size > size - 12 {
        return Err(invalid("Invalid Parquet metadata length"));
    }
    Ok(FooterBounds {
        offset: size - 8 - metadata_size,
        size: metadata_size,
    })
}

/// Checks a cancellation signal between bounded footer operations.
pub fn check_cancel(cancel: Option<&AtomicBool>) -> io::Result<()> {
    if cancel.is_some_and(|value| value.load(Ordering::Relaxed)) {
        Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
    } else {
        Ok(())
    }
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
