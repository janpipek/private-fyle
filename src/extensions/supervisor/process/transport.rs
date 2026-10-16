//! Bounded nonblocking pipe I/O and frame decoding for the Linux helper process.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;

use super::STDERR_LIMIT;
use crate::extensions::protocol::{
    self, HostFrame, HostMessage, MAX_FRAME_BYTES, MAX_RESULT_TEXT_BYTES, MAX_WORKER_BYTES,
};

const IO_CHUNK: usize = 16 * 1024;

/// Writes queued bytes to a nonblocking helper pipe until it would block.
pub fn write_pending(writer: &mut impl Write, outgoing: &mut VecDeque<u8>) -> io::Result<()> {
    while !outgoing.is_empty() {
        let (first, second) = outgoing.as_slices();
        let bytes = if first.is_empty() { second } else { first };
        match writer.write(bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "worker pipe write returned zero",
                ));
            }
            Ok(written) => {
                outgoing.drain(..written);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Drains and decodes available worker protocol bytes, enforcing cumulative bounds.
pub fn drain_protocol(
    reader: &mut impl Read,
    decoder: &mut FrameDecoder,
    total: &mut usize,
    eof: &mut bool,
) -> io::Result<Vec<protocol::WorkerFrame>> {
    let mut bytes = [0; IO_CHUNK];
    let mut frames = Vec::new();
    loop {
        match reader.read(&mut bytes) {
            Ok(0) => {
                *eof = true;
                decoder.finish()?;
                break;
            }
            Ok(count) => {
                *total = total.saturating_add(count);
                if *total > MAX_WORKER_BYTES {
                    return Err(invalid("worker protocol byte limit exceeded"));
                }
                frames.extend(decoder.feed(&bytes[..count])?);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(frames)
}

/// Drains helper diagnostics without retaining more than the declared limit.
pub fn drain_diagnostics(
    reader: &mut impl Read,
    total: &mut usize,
    retained: &mut Vec<u8>,
    eof: &mut bool,
) -> io::Result<()> {
    let mut bytes = [0; IO_CHUNK];
    loop {
        match reader.read(&mut bytes) {
            Ok(0) => {
                *eof = true;
                break;
            }
            Ok(count) => {
                *total = total.saturating_add(count);
                if *total > STDERR_LIMIT {
                    return Err(invalid("extension helper diagnostics exceeded the limit"));
                }
                retained.extend_from_slice(&bytes[..count]);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Appends one bounded host message to the queued helper input.
pub fn append_host_frame(outgoing: &mut VecDeque<u8>, message: HostMessage) -> io::Result<()> {
    let bytes = protocol::encode_host_frame(&HostFrame {
        version: protocol::VERSION,
        message,
    })?;
    if outgoing.len().saturating_add(bytes.len()) > MAX_FRAME_BYTES * 2 {
        return Err(invalid("host response buffer exceeded the limit"));
    }
    outgoing.extend(bytes);
    Ok(())
}

/// Enables nonblocking mode on an open pipe descriptor.
pub fn set_nonblocking(file: &impl AsRawFd) -> io::Result<()> {
    // SAFETY: fcntl only reads the open pipe's flags and updates that same descriptor.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: flags were obtained from this valid descriptor; O_NONBLOCK changes its status flags.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Incrementally decodes length-prefixed worker frames from arbitrary pipe chunks.
#[derive(Default)]
pub struct FrameDecoder {
    pending: Vec<u8>,
}

impl FrameDecoder {
    /// Accepts an arbitrary frame fragment and returns every complete frame.
    pub fn feed(&mut self, input: &[u8]) -> io::Result<Vec<protocol::WorkerFrame>> {
        if self.pending.len().saturating_add(input.len()) > MAX_FRAME_BYTES + 4 + IO_CHUNK {
            return Err(invalid("worker frame buffer exceeded the limit"));
        }
        self.pending.extend_from_slice(input);
        let mut frames = Vec::new();
        loop {
            if self.pending.len() < 4 {
                break;
            }
            let prefix: [u8; 4] = self.pending[..4]
                .try_into()
                .expect("four-byte frame prefix was checked");
            let length = u32::from_be_bytes(prefix) as usize;
            if length > MAX_FRAME_BYTES {
                return Err(invalid("worker frame exceeds the limit"));
            }
            if self.pending.len() < length + 4 {
                break;
            }
            frames.push(protocol::decode_worker_payload(
                &self.pending[4..4 + length],
            )?);
            self.pending.drain(..4 + length);
        }
        Ok(frames)
    }

    /// Rejects EOF with an incomplete length prefix or payload.
    pub fn finish(&self) -> io::Result<()> {
        if self.pending.is_empty() {
            Ok(())
        } else {
            Err(invalid("worker ended with a partial protocol frame"))
        }
    }
}

/// Truncates an error reason at a UTF-8 boundary for the worker result schema.
pub fn bounded_reason(value: &str) -> String {
    let mut end = value.len().min(MAX_RESULT_TEXT_BYTES);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::FrameDecoder;
    use crate::extensions::protocol::{
        self, ContributionResult, MAX_FRAME_BYTES, OutputState, VERSION, WorkerFrame, WorkerMessage,
    };

    fn absent_frame() -> WorkerFrame {
        WorkerFrame {
            version: VERSION,
            message: WorkerMessage::Result(ContributionResult {
                field: None,
                state: OutputState::Absent,
            }),
        }
    }

    fn framed(frame: &WorkerFrame) -> Vec<u8> {
        let mut bytes = Vec::new();
        protocol::write_worker_frame(&mut bytes, frame).expect("fixture frame encodes");
        bytes
    }

    #[test]
    fn decoder_accepts_split_prefix_payload_and_coalesced_frames() {
        let first = framed(&absent_frame());
        let second = framed(&absent_frame());
        let mut decoder = FrameDecoder::default();
        assert!(
            decoder
                .feed(&first[..2])
                .expect("prefix fragment")
                .is_empty()
        );
        assert!(
            decoder
                .feed(&first[2..7])
                .expect("payload fragment")
                .is_empty()
        );
        let mut rest = first[7..].to_vec();
        rest.extend_from_slice(&second);
        assert_eq!(decoder.feed(&rest).expect("coalesced frames").len(), 2);
        decoder.finish().expect("all frame bytes were consumed");
    }

    #[test]
    fn decoder_accepts_a_maximum_frame_coalesced_with_the_next_prefix() {
        let mut maximum = WorkerFrame {
            version: VERSION,
            message: WorkerMessage::Result(ContributionResult {
                field: None,
                state: OutputState::Failed {
                    reason: String::new(),
                },
            }),
        };
        let base = serde_json::to_vec(&maximum).expect("base worker frame serializes");
        let WorkerMessage::Result(result) = &mut maximum.message else {
            unreachable!("absent frame has a result message")
        };
        result.state = OutputState::Failed {
            reason: "x".repeat(MAX_FRAME_BYTES - base.len()),
        };
        let first = framed(&maximum);
        assert_eq!(
            u32::from_be_bytes(first[..4].try_into().expect("frame prefix")) as usize,
            MAX_FRAME_BYTES
        );
        let second = framed(&absent_frame());
        let mut coalesced = first;
        coalesced.extend_from_slice(&second);
        let mut decoder = FrameDecoder::default();
        assert_eq!(
            decoder
                .feed(&coalesced)
                .expect("maximal coalesced frames")
                .len(),
            2
        );
        decoder.finish().expect("all frame bytes were consumed");
    }
}
