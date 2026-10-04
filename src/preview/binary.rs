use std::fmt::Write as _;
use std::io;

use super::{Content, Previewer, Probe};

/// Bytes shown in the hex dump.
const MAX_DUMP: usize = 4 * 1024;

/// Fallback for anything not handled by a more specific previewer.
pub struct BinaryPreviewer;

impl Previewer for BinaryPreviewer {
    fn name(&self) -> &'static str {
        "Binary"
    }

    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir
    }

    fn load(&self, probe: &Probe) -> io::Result<Content> {
        let bytes = &probe.head[..probe.head.len().min(MAX_DUMP)];
        Ok(Content::Binary {
            dump: hex_dump(bytes),
            truncated: probe.size > bytes.len() as u64,
        })
    }
}

/// `offset  hex bytes  |ascii|`, 16 bytes per line.
fn hex_dump(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (line, chunk) in bytes.chunks(16).enumerate() {
        let _ = write!(out, "{:08x} ", line * 16);
        for (i, byte) in chunk.iter().enumerate() {
            let gap = if i == 8 { "  " } else { " " };
            let _ = write!(out, "{gap}{byte:02x}");
        }
        let missing = 16 - chunk.len();
        out.push_str(&" ".repeat(missing * 3 + usize::from(chunk.len() <= 8)));
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        let _ = write!(out, "  |{ascii}|");
        out.push('\n');
    }
    out.pop();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dump_lines_align() {
        let dump = hex_dump(&(0u8..20).collect::<Vec<_>>());
        let lines: Vec<_> = dump.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].find('|'), lines[1].find('|'));
        assert!(lines[1].starts_with("00000010  10 11 12 13"));
    }
}
