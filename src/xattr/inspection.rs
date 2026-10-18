//! Bounded attribute snapshots with exact byte names, typed previews and per-value failures.

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::{NATIVE_LIMIT, ReadValue};

const ATTRIBUTE_LIMIT: usize = 64;
const READ_BUDGET: usize = 256 * 1024;
const VALUE_PREVIEW_LIMIT: usize = 1024;
const PREVIEW_BUDGET: usize = 8192;
const DEADLINE: Duration = Duration::from_secs(2);

/// Lossless preview of a bounded value prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Preview {
    /// Valid UTF-8 with only printable characters and ordinary text whitespace.
    Text(String),
    /// Space-separated hexadecimal bytes for binary values.
    Hex(String),
}

/// An independent attribute-value result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// Complete native value was read, with a possibly shortened display prefix.
    Ready {
        /// Original value length in bytes.
        bytes: usize,
        /// Number of original bytes represented in the preview.
        shown: usize,
        /// Text or hexadecimal display prefix.
        preview: Preview,
    },
    /// The queried size exceeded the remaining read/display budget; no value was allocated.
    Omitted {
        /// Value length at the native size query.
        bytes: usize,
    },
    /// This value disappeared or could not be read; other values remain available.
    Failed(String),
}

/// One sorted attribute and its independent read result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute {
    /// Printable name, with control characters and non-UTF-8 bytes escaped.
    pub name: String,
    /// Value or a visible reason why it is unavailable.
    pub value: Value,
}

/// Attribute inspection bounded independently of filesystem and format Properties.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inspection {
    /// Up to 64 attributes in native byte-name order.
    pub attributes: Vec<Attribute>,
    /// Additional names excluded by the attribute-count limit.
    pub omitted_names: usize,
}

trait Reader {
    fn list(&self, check: &mut dyn FnMut() -> io::Result<()>) -> io::Result<Vec<u8>>;
    fn get(
        &self,
        name: &[u8],
        limit: usize,
        check: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<ReadValue>;
}

struct Native<'a>(&'a Path);

impl Reader for Native<'_> {
    fn list(&self, check: &mut dyn FnMut() -> io::Result<()>) -> io::Result<Vec<u8>> {
        super::list(self.0, check)
    }
    fn get(
        &self,
        name: &[u8],
        limit: usize,
        check: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<ReadValue> {
        super::get(self.0, name, limit, check)
    }
}

/// Reads attributes of the selected path object, including directories and symlinks.
/// Call on a background worker; unsupported filesystems produce no attributes.
pub fn read(path: &Path, cancel: &AtomicBool) -> io::Result<Inspection> {
    inspect(&Native(path), cancel, Instant::now() + DEADLINE)
}

fn inspect(reader: &impl Reader, cancel: &AtomicBool, deadline: Instant) -> io::Result<Inspection> {
    let mut check = || {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "extended attribute inspection cancelled",
            ));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "extended attribute inspection timed out",
            ));
        }
        Ok(())
    };
    check()?;
    let names = match reader.list(&mut check) {
        Ok(names) => names,
        Err(error) if super::unsupported(&error) => return Ok(Inspection::default()),
        Err(error) => return Err(error),
    };
    check()?;
    if names.len() > NATIVE_LIMIT || (!names.is_empty() && names.last() != Some(&0)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid or oversized extended attribute name list",
        ));
    }
    let mut names: Vec<_> = names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .collect();
    if names.iter().any(|name| name.len() > 255) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "extended attribute name exceeds 255 bytes",
        ));
    }
    names.sort_unstable();
    names.dedup();
    let mut inspection = Inspection {
        omitted_names: names.len().saturating_sub(ATTRIBUTE_LIMIT),
        ..Inspection::default()
    };
    let mut remaining_read = READ_BUDGET;
    let mut remaining_preview = PREVIEW_BUDGET;
    for name in names.into_iter().take(ATTRIBUTE_LIMIT) {
        check()?;
        let limit = if remaining_preview == 0 {
            0
        } else {
            remaining_read.min(NATIVE_LIMIT)
        };
        let read = reader.get(name, limit, &mut check);
        check()?;
        let value = match read {
            Ok(ReadValue::Complete(bytes)) => {
                remaining_read = remaining_read.checked_sub(bytes.len()).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "attribute reader exceeded byte budget",
                    )
                })?;
                let (preview, shown) = preview(&bytes, remaining_preview.min(VALUE_PREVIEW_LIMIT));
                remaining_preview -= shown;
                Value::Ready {
                    bytes: bytes.len(),
                    shown,
                    preview,
                }
            }
            Ok(ReadValue::Omitted(bytes)) => Value::Omitted { bytes },
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(error);
            }
            Err(error) => Value::Failed(error.to_string()),
        };
        inspection.attributes.push(Attribute {
            name: display_name(name),
            value,
        });
    }
    Ok(inspection)
}

fn display_name(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.chars().flat_map(char::escape_debug).collect(),
        Err(_) => bytes
            .iter()
            .flat_map(|byte| std::ascii::escape_default(*byte))
            .map(char::from)
            .collect(),
    }
}

fn preview(bytes: &[u8], limit: usize) -> (Preview, usize) {
    let mut shown = bytes.len().min(limit);
    if let Ok(text) = std::str::from_utf8(bytes)
        && text
            .chars()
            .all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
    {
        while !text.is_char_boundary(shown) {
            shown -= 1;
        }
        (Preview::Text(text[..shown].into()), shown)
    } else {
        let hex = bytes[..shown]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        (Preview::Hex(hex), shown)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ATTRIBUTE_LIMIT, Inspection, PREVIEW_BUDGET, Preview, READ_BUDGET, Reader, Value, inspect,
        preview,
    };
    use crate::xattr::ReadValue;
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::io;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    struct Fake {
        values: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
        bytes_read: Cell<usize>,
    }
    impl Reader for Fake {
        fn list(&self, _: &mut dyn FnMut() -> io::Result<()>) -> io::Result<Vec<u8>> {
            Ok(self
                .values
                .keys()
                .rev()
                .flat_map(|key| key.iter().copied().chain([0]))
                .collect())
        }
        fn get(
            &self,
            name: &[u8],
            limit: usize,
            _: &mut dyn FnMut() -> io::Result<()>,
        ) -> io::Result<ReadValue> {
            let value = self.values[name].as_ref().ok_or_else(|| {
                io::Error::new(io::ErrorKind::PermissionDenied, "unreadable attribute")
            })?;
            if value.len() > limit {
                return Ok(ReadValue::Omitted(value.len()));
            }
            self.bytes_read.set(self.bytes_read.get() + value.len());
            Ok(ReadValue::Complete(value.clone()))
        }
    }
    fn fake(values: impl IntoIterator<Item = (Vec<u8>, Option<Vec<u8>>)>) -> Fake {
        Fake {
            values: values.into_iter().collect(),
            bytes_read: Cell::new(0),
        }
    }
    fn run(reader: &impl Reader) -> Inspection {
        inspect(
            reader,
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap()
    }

    #[test]
    fn distinguishes_empty_text_binary_and_read_failures_in_sorted_order() {
        let reader = fake([
            (b"user.text".to_vec(), Some(b"hello\nworld".to_vec())),
            (b"user.binary".to_vec(), Some(vec![0, 255, 65])),
            (b"user.empty".to_vec(), Some(Vec::new())),
            (b"user.failed".to_vec(), None),
            (b"user.\xff".to_vec(), Some(b"raw name".to_vec())),
        ]);
        let result = run(&reader);
        assert_eq!(result.attributes[0].name, "user.binary");
        assert!(
            matches!(&result.attributes[0].value, Value::Ready { preview: Preview::Hex(hex), bytes: 3, shown: 3 } if hex == "00 ff 41")
        );
        assert!(
            matches!(&result.attributes[1].value, Value::Ready { preview: Preview::Text(text), bytes: 0, .. } if text.is_empty())
        );
        assert!(
            matches!(&result.attributes[2].value, Value::Failed(error) if error.contains("unreadable"))
        );
        assert!(
            matches!(&result.attributes[3].value, Value::Ready { preview: Preview::Text(text), .. } if text == "hello\nworld")
        );
        assert_eq!(result.attributes[4].name, "user.\\xff");
    }

    #[test]
    fn bounds_count_total_read_and_total_display_and_keeps_utf8_boundaries() {
        let reader =
            fake((0..100).map(|i| (format!("user.{i:03}").into_bytes(), Some(vec![b'a'; 65536]))));
        let result = run(&reader);
        assert_eq!(result.attributes.len(), ATTRIBUTE_LIMIT);
        assert_eq!(result.omitted_names, 100 - ATTRIBUTE_LIMIT);
        assert!(reader.bytes_read.get() <= READ_BUDGET);
        let shown: usize = result
            .attributes
            .iter()
            .map(|attr| match attr.value {
                Value::Ready { shown, .. } => shown,
                _ => 0,
            })
            .sum();
        assert!(shown <= PREVIEW_BUDGET);
        assert!(
            result
                .attributes
                .iter()
                .any(|attr| matches!(attr.value, Value::Omitted { .. }))
        );
        assert_eq!(preview("éé".as_bytes(), 3), (Preview::Text("é".into()), 2));
        assert_eq!(preview(b"a\x1bb", 3), (Preview::Hex("61 1b 62".into()), 3));
    }

    #[test]
    fn unsupported_lists_are_empty_but_permission_errors_are_not_hidden() {
        struct Failure(io::ErrorKind);
        impl Reader for Failure {
            fn list(&self, _: &mut dyn FnMut() -> io::Result<()>) -> io::Result<Vec<u8>> {
                Err(io::Error::new(self.0, "list failed"))
            }
            fn get(
                &self,
                _: &[u8],
                _: usize,
                _: &mut dyn FnMut() -> io::Result<()>,
            ) -> io::Result<ReadValue> {
                panic!("no names were listed")
            }
        }
        assert!(
            run(&Failure(io::ErrorKind::Unsupported))
                .attributes
                .is_empty()
        );
        assert_eq!(
            inspect(
                &Failure(io::ErrorKind::PermissionDenied),
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(10)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn cancellation_and_expired_deadlines_stop_before_reading() {
        let reader = fake([(b"user.test".to_vec(), Some(vec![1, 2, 3]))]);
        assert_eq!(
            inspect(
                &reader,
                &AtomicBool::new(true),
                Instant::now() + Duration::from_secs(10)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
        assert_eq!(
            inspect(
                &reader,
                &AtomicBool::new(false),
                Instant::now() - Duration::from_secs(1)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(reader.bytes_read.get(), 0);
    }
}
