//! Bounded reads from the exact local source selected by the host.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};

use crate::data_providers::DemandLimits;
use crate::location::Location;

use super::protocol::{ReadRequest, ReadResult};

/// Maximum bytes returned by one broker request.
pub const MAX_READ_BYTES: u64 = 64 * 1024;
/// Maximum calls to the source broker per helper.
pub const MAX_READ_CALLS: u32 = 512;
/// Maximum cumulative source bytes authorized by one helper.
pub const MAX_SOURCE_TOTAL_BYTES: u64 = 16 * 1024 * 1024;

/// Owns a selected regular file and enforces byte and call budgets.
#[derive(Debug)]
pub struct SourceBroker {
    file: File,
    handle: u32,
    max_bytes: u64,
    bytes_requested: u64,
    calls: u32,
    next_read_id: u32,
}

impl SourceBroker {
    /// Opens and verifies the captured local source without converting its native path.
    pub fn open(location: &Location, demand: DemandLimits, handle: u32) -> io::Result<Self> {
        let path = location
            .as_local_path()
            .ok_or_else(|| invalid("extension source must be a local file"))?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = options.open(path)?;
        if !file.metadata()?.is_file() {
            return Err(invalid("extension source must be a regular file"));
        }
        Ok(Self {
            file,
            handle,
            max_bytes: demand.max_bytes.min(MAX_SOURCE_TOTAL_BYTES),
            bytes_requested: 0,
            calls: 0,
            next_read_id: 1,
        })
    }

    /// Serves one validated worker read and charges repeated ranges again.
    pub fn read(&mut self, request: ReadRequest) -> io::Result<ReadResult> {
        self.calls = self.calls.saturating_add(1);
        if self.calls > MAX_READ_CALLS {
            return Err(invalid("source read call limit exceeded"));
        }
        if request.read_id != self.next_read_id {
            return Err(invalid("source read IDs must increase by one"));
        }
        self.next_read_id = self
            .next_read_id
            .checked_add(1)
            .ok_or_else(|| invalid("source read ID exhausted"))?;
        if request.source_handle != self.handle {
            return Err(invalid("unknown source handle"));
        }
        if request.length > MAX_READ_BYTES {
            return Err(invalid("source read exceeds the per-read limit"));
        }
        request
            .offset
            .checked_add(request.length)
            .ok_or_else(|| invalid("source read range overflows"))?;
        let total = self
            .bytes_requested
            .checked_add(request.length)
            .ok_or_else(|| invalid("source byte budget overflows"))?;
        if total > self.max_bytes {
            return Err(invalid("source byte budget exceeded"));
        }
        self.bytes_requested = total;
        self.file.seek(SeekFrom::Start(request.offset))?;
        let mut bytes = vec![0; request.length as usize];
        let mut filled = 0;
        while filled < bytes.len() {
            let read = self.file.read(&mut bytes[filled..])?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        bytes.truncate(filled);
        let eof_position = request
            .offset
            .checked_add(filled as u64)
            .ok_or_else(|| invalid("source read position overflows"))?;
        let eof = eof_position >= self.file.metadata()?.len();
        Ok(ReadResult {
            read_id: request.read_id,
            eof,
            bytes,
            error: None,
        })
    }

    /// Returns charged read bytes for diagnostics and tests.
    pub fn bytes_requested(&self) -> u64 {
        self.bytes_requested
    }

    /// Returns broker calls made so far.
    pub fn calls(&self) -> u32 {
        self.calls
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::{MAX_READ_BYTES, MAX_READ_CALLS, SourceBroker};
    use crate::data_providers::DemandLimits;
    use crate::location::Location;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn broker(path: PathBuf, max_bytes: u64) -> SourceBroker {
        SourceBroker::open(
            &Location::local(path).unwrap(),
            DemandLimits {
                max_bytes,
                max_items: 10,
            },
            7,
        )
        .unwrap()
    }

    fn request(read_id: u32, offset: u64, length: u64) -> super::super::protocol::ReadRequest {
        super::super::protocol::ReadRequest {
            read_id,
            source_handle: 7,
            offset,
            length,
        }
    }

    #[test]
    fn broker_serves_bytes_and_charges_overlapping_reads_again() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("source");
        std::fs::write(&path, b"abcdefgh").unwrap();
        let mut broker = broker(path, 8);
        let result = broker.read(request(1, 1, 4)).unwrap();
        assert_eq!(result.bytes, b"bcde");
        assert!(!result.eof);
        assert_eq!(broker.read(request(2, 1, 4)).unwrap().bytes, b"bcde");
        assert_eq!(broker.bytes_requested(), 8);
        assert!(broker.read(request(3, 0, 1)).is_err());
    }

    #[test]
    fn broker_reads_exact_per_call_limit_and_reports_exact_eof_including_zero_length() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("source");
        std::fs::write(&path, vec![b'x'; MAX_READ_BYTES as usize]).unwrap();
        let mut broker = broker(path, MAX_READ_BYTES);
        let end = broker.read(request(1, 0, MAX_READ_BYTES)).unwrap();
        assert_eq!(end.bytes.len(), MAX_READ_BYTES as usize);
        assert!(end.eof);
        let empty = broker.read(request(2, MAX_READ_BYTES, 0)).unwrap();
        assert!(empty.bytes.is_empty());
        assert!(empty.eof);
        assert_eq!(broker.calls(), 2);
    }

    #[test]
    fn broker_rejects_duplicate_or_skipped_read_ids() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("source");
        std::fs::write(&path, b"data").unwrap();
        let mut broker = broker(path, 4);
        assert_eq!(broker.read(request(1, 0, 1)).unwrap().bytes, b"d");
        assert!(broker.read(request(1, 0, 1)).is_err());
        assert!(broker.read(request(3, 0, 1)).is_err());
        assert_eq!(broker.read(request(2, 1, 1)).unwrap().bytes, b"a");
    }

    #[test]
    fn broker_enforces_the_exact_cumulative_source_cap() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("source");
        std::fs::write(&path, vec![b'x'; MAX_READ_BYTES as usize]).unwrap();
        let mut broker = broker(path, u64::MAX);
        for read_id in 1..=256 {
            broker.read(request(read_id, 0, MAX_READ_BYTES)).unwrap();
        }
        assert_eq!(broker.bytes_requested(), super::MAX_SOURCE_TOTAL_BYTES);
        assert!(broker.read(request(257, 0, 1)).is_err());
    }

    #[test]
    fn broker_rejects_directories_and_nonblocking_fifo_sources() {
        let temp = TempDir::new().unwrap();
        let demand = DemandLimits {
            max_bytes: 1,
            max_items: 1,
        };
        let directory = Location::local(temp.path().to_path_buf()).unwrap();
        assert!(SourceBroker::open(&directory, demand, 1).is_err());
        #[cfg(target_os = "linux")]
        {
            use std::ffi::CString;
            use std::os::unix::ffi::OsStrExt as _;
            let fifo = temp.path().join("fifo");
            let fifo_c = CString::new(fifo.as_os_str().as_bytes()).unwrap();
            // SAFETY: the NUL-terminated path is contained by the test's TempDir.
            assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
            let location = Location::local(fifo).unwrap();
            assert!(SourceBroker::open(&location, demand, 1).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn broker_keeps_the_open_source_even_if_its_path_is_replaced() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("source");
        let replacement = temp.path().join("replacement");
        std::fs::write(&path, b"original").unwrap();
        let mut broker = broker(path.clone(), 8);
        std::fs::rename(&path, &replacement).unwrap();
        std::fs::write(&path, b"changed!").unwrap();
        assert_eq!(broker.read(request(1, 0, 8)).unwrap().bytes, b"original");
    }

    #[test]
    fn broker_rejects_unknown_handles_large_reads_overflow_and_call_excess() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("source");
        std::fs::write(&path, vec![0; MAX_READ_BYTES as usize + 1]).unwrap();
        let mut broker = broker(path, u64::MAX);
        assert!(
            broker
                .read(super::super::protocol::ReadRequest {
                    source_handle: 9,
                    ..request(1, 0, 1)
                })
                .is_err()
        );
        assert!(broker.read(request(2, u64::MAX, 1)).is_err());
        assert!(broker.read(request(3, 0, MAX_READ_BYTES + 1)).is_err());
        for index in 4..=MAX_READ_CALLS {
            broker.read(request(index, 0, 0)).unwrap();
        }
        assert_eq!(broker.calls(), MAX_READ_CALLS);
        assert!(broker.read(request(MAX_READ_CALLS + 1, 0, 0)).is_err());
    }

    #[test]
    fn broker_accepts_non_utf8_path_and_rejects_remote_or_archive_sources() {
        let temp = TempDir::new().unwrap();
        #[cfg(unix)]
        let path = {
            use std::ffi::OsString;
            use std::os::unix::ffi::OsStringExt;
            temp.path().join(OsString::from_vec(b"file-\xff".to_vec()))
        };
        #[cfg(not(unix))]
        let path = temp.path().join("file");
        std::fs::write(&path, b"ok").unwrap();
        assert_eq!(broker(path, 2).read(request(1, 0, 2)).unwrap().bytes, b"ok");
        let remote = Location::Remote(crate::location::RemoteLocation::root("nas").unwrap());
        assert!(
            SourceBroker::open(
                &remote,
                DemandLimits {
                    max_bytes: 1,
                    max_items: 1
                },
                1,
            )
            .is_err()
        );
        let archive = Location::archive(
            Location::local(temp.path().join("archive.zip")).unwrap(),
            "member",
        )
        .unwrap();
        assert!(
            SourceBroker::open(
                &archive,
                DemandLimits {
                    max_bytes: 1,
                    max_items: 1
                },
                1,
            )
            .is_err()
        );
    }
}
