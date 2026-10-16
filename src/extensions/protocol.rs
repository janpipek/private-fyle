//! Versioned, bounded JSON frames exchanged with one extension helper.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Protocol version used by the helper and host.
pub const VERSION: u32 = 1;
/// Maximum JSON payload in one frame.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
/// Maximum cumulative worker-to-host frame bytes, including length prefixes.
pub const MAX_WORKER_BYTES: usize = 2 * 1024 * 1024;
/// Maximum result text or diagnostic length in UTF-8 bytes.
pub const MAX_RESULT_TEXT_BYTES: usize = 64 * 1024;

/// Host-to-worker envelope; every frame has an explicit protocol version.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostFrame {
    /// Protocol version.
    pub version: u32,
    /// Message payload.
    pub message: HostMessage,
}

/// Worker-to-host envelope; every frame has an explicit protocol version.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerFrame {
    /// Protocol version.
    pub version: u32,
    /// Message payload.
    pub message: WorkerMessage,
}

/// Host messages accepted by the worker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostMessage {
    /// Initializes the single-request child with declarations and captured facts.
    Init(Init),
    /// Returns bytes requested through the read broker.
    ReadResult(ReadResult),
}

/// Worker messages accepted by the host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerMessage {
    /// Requests a bounded read from the selected source handle.
    Read(ReadRequest),
    /// Completes the selected contribution exactly once.
    Result(ContributionResult),
}

/// Initial package and source context sent to a fresh helper.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Init {
    /// Validated package identity.
    pub package: String,
    /// Provider owning the selected contribution.
    pub provider: String,
    /// Selected column contribution.
    pub contribution: String,
    /// Exact field ID bound by that contribution.
    pub field: String,
    /// Expected field schema.
    pub field_type: FieldType,
    /// Declared acquisition limits.
    pub demand: Demand,
    /// Captured display name, never a lookup instruction.
    pub name: String,
    /// Captured directory fact.
    pub is_dir: bool,
    /// Opaque source handle, not a path or filesystem descriptor.
    pub source_handle: u32,
    /// Bounded package entry source.
    pub script: String,
}

/// Declared value type for the selected provider field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "unit",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FieldType {
    /// UTF-8 text.
    Text,
    /// Finite, nonnegative quantity with a fixed unit.
    Quantity(Unit),
}

/// A supported semantic unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    /// Seconds.
    Seconds,
    /// Metres.
    Metres,
}

/// Declared byte and item bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Demand {
    /// Maximum input bytes requested by the declaration.
    pub max_bytes: u64,
    /// Maximum input items requested by the declaration.
    pub max_items: u64,
}

/// A worker request to read a selected source handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadRequest {
    /// Monotonically increasing request ID within this child.
    pub read_id: u32,
    /// Opaque handle declared in Init.
    pub source_handle: u32,
    /// Byte offset in the original source.
    pub offset: u64,
    /// Requested byte count.
    pub length: u64,
}

/// Reply to one brokered source read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadResult {
    /// Matching request ID.
    pub read_id: u32,
    /// Bytes read from the source.
    pub bytes: Vec<u8>,
    /// Whether the read reached the end of the source.
    pub eof: bool,
    /// Bounded failure detail, absent on success.
    pub error: Option<String>,
}

/// One final result for the contribution named in Init.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContributionResult {
    /// Field ID for a ready result; must equal Init.field.
    pub field: Option<String>,
    /// Terminal contribution state.
    pub state: OutputState,
}

/// Result state returned by one contribution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OutputState {
    /// A typed field value and an explicit completeness claim.
    Ready {
        /// Typed value.
        value: OutputValue,
        /// Whether the value covers its requested data.
        completeness: Completeness,
    },
    /// No value exists for the selected source.
    Absent,
    /// The contribution does not apply to captured facts.
    Unavailable {
        /// Reason shown to the host-side caller.
        reason: String,
    },
    /// The contribution failed while reading or preparing a value.
    Failed {
        /// Bounded diagnostic.
        reason: String,
    },
}

/// Typed value returned by the worker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OutputValue {
    /// UTF-8 text.
    Text {
        /// Text contents.
        value: String,
    },
    /// Numeric measurement.
    Quantity {
        /// Finite, nonnegative magnitude.
        value: f64,
        /// Explicit semantic unit.
        unit: Unit,
    },
}

/// Completeness attached to a ready value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Completeness {
    /// The requested data was covered fully.
    Complete,
    /// Acquisition stopped at a named bound.
    Truncated {
        /// Bounded reason for truncation.
        reason: String,
    },
    /// The value is known to be at least the reported amount.
    LowerBound,
}

/// Writes one checked four-byte-length-prefixed JSON frame.
fn write_frame<T: Serialize, W: Write>(writer: &mut W, value: &T) -> io::Result<()> {
    let mut payload = LimitedPayload(Vec::new());
    serde_json::to_writer(&mut payload, value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let length =
        u32::try_from(payload.len()).map_err(|_| invalid("protocol frame is too large"))?;
    writer.write_all(&length.to_be_bytes())?;
    writer.write_all(&payload.0)
}

/// Reads one checked four-byte-length-prefixed JSON frame.
fn read_frame<T: DeserializeOwned, R: Read>(reader: &mut R) -> io::Result<T> {
    let mut prefix = [0u8; 4];
    reader.read_exact(&mut prefix)?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(invalid("protocol frame exceeds the size limit"));
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload)?;
    serde_json::from_slice(&payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Reads one host frame and rejects unsupported protocol versions.
pub fn read_host_frame<R: Read>(reader: &mut R) -> io::Result<HostFrame> {
    checked_version(read_frame(reader)?)
}

/// Reads one worker frame and rejects unsupported protocol versions.
pub fn read_worker_frame<R: Read>(reader: &mut R) -> io::Result<WorkerFrame> {
    checked_version(read_frame(reader)?)
}

/// Writes a host frame only when its version matches this implementation.
pub fn write_host_frame<W: Write>(writer: &mut W, frame: &HostFrame) -> io::Result<()> {
    if frame.version != VERSION {
        return Err(invalid("unsupported host protocol version"));
    }
    write_frame(writer, frame)
}

/// Writes a worker frame only when its version matches this implementation.
pub fn write_worker_frame<W: Write>(writer: &mut W, frame: &WorkerFrame) -> io::Result<()> {
    if frame.version != VERSION {
        return Err(invalid("unsupported worker protocol version"));
    }
    write_frame(writer, frame)
}

/// Encodes one worker payload with frame-size and version checks for nonblocking transports.
pub fn encode_host_frame(frame: &HostFrame) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    write_host_frame(&mut bytes, frame)?;
    Ok(bytes)
}

/// Decodes one already frame-bounded payload and checks its protocol version.
pub fn decode_worker_payload(payload: &[u8]) -> io::Result<WorkerFrame> {
    if payload.len() > MAX_FRAME_BYTES {
        return Err(invalid("protocol frame exceeds the size limit"));
    }
    checked_version(
        serde_json::from_slice(payload)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
    )
}

fn checked_version<T: HasVersion>(frame: T) -> io::Result<T> {
    if frame.version() == VERSION {
        Ok(frame)
    } else {
        Err(invalid("unsupported protocol version"))
    }
}

trait HasVersion {
    fn version(&self) -> u32;
}

impl HasVersion for HostFrame {
    fn version(&self) -> u32 {
        self.version
    }
}

impl HasVersion for WorkerFrame {
    fn version(&self) -> u32 {
        self.version
    }
}

struct LimitedPayload(Vec<u8>);

impl LimitedPayload {
    fn len(&self) -> usize {
        self.0.len()
    }
}

impl Write for LimitedPayload {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_FRAME_BYTES {
            return Err(invalid("protocol frame exceeds the size limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Validates a decoded worker frame against the selected field schema.
pub fn validate_result(
    init: &Init,
    result: &ContributionResult,
) -> io::Result<crate::data_providers::ResultState<crate::data_providers::Value>> {
    use crate::data_providers::{Quantity, ResultState, Unit as ModelUnit, Value};

    let state = match &result.state {
        OutputState::Ready {
            value,
            completeness,
        } => {
            if result.field.as_deref() != Some(init.field.as_str()) {
                return Err(invalid(
                    "worker result field does not match selected column",
                ));
            }
            let value = match (init.field_type, value) {
                (FieldType::Text, OutputValue::Text { value })
                    if value.len() <= MAX_RESULT_TEXT_BYTES =>
                {
                    Value::Text(value.clone())
                }
                (FieldType::Quantity(expected), OutputValue::Quantity { value, unit })
                    if expected == *unit && value.is_finite() && *value >= 0.0 =>
                {
                    let unit = match unit {
                        Unit::Seconds => ModelUnit::Seconds,
                        Unit::Metres => ModelUnit::Metres,
                    };
                    Value::Quantity(Quantity::new(*value, unit)?)
                }
                _ => return Err(invalid("worker value does not match selected field schema")),
            };
            let completeness = match completeness {
                Completeness::Complete => crate::data_providers::Completeness::Complete,
                Completeness::Truncated { reason } => {
                    validate_reason(reason)?;
                    crate::data_providers::Completeness::Truncated(reason.clone())
                }
                Completeness::LowerBound => crate::data_providers::Completeness::LowerBound,
            };
            ResultState::Ready {
                value,
                completeness,
            }
        }
        OutputState::Absent => ResultState::Absent,
        OutputState::Unavailable { reason } => {
            validate_reason(reason)?;
            ResultState::Unavailable(reason.clone())
        }
        OutputState::Failed { reason } => {
            validate_reason(reason)?;
            ResultState::Failed(reason.clone())
        }
    };
    if !matches!(result.state, OutputState::Ready { .. }) && result.field.is_some() {
        return Err(invalid("non-ready result cannot declare a field"));
    }
    Ok(state)
}

fn validate_reason(reason: &str) -> io::Result<()> {
    if reason.len() > MAX_RESULT_TEXT_BYTES {
        return Err(invalid("worker result text exceeds the size limit"));
    }
    Ok(())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::{
        Completeness, ContributionResult, FieldType, Init, MAX_FRAME_BYTES, OutputState,
        OutputValue, Unit, read_frame, validate_result, write_frame,
    };
    use crate::data_providers::{ResultState, Unit as ModelUnit};
    use std::io::Cursor;

    fn init() -> Init {
        Init {
            package: "sensor".into(),
            provider: "extension.sensor.reader".into(),
            contribution: "extension.sensor.reader.distance".into(),
            field: "extension.sensor.reader.metres".into(),
            field_type: FieldType::Quantity(Unit::Metres),
            demand: super::Demand {
                max_bytes: 100,
                max_items: 10,
            },
            name: "route.gpx".into(),
            is_dir: false,
            source_handle: 1,
            script: "return {}".into(),
        }
    }

    #[test]
    fn frame_codec_roundtrips_and_rejects_invalid_truncated_and_oversize_frames() {
        let frame = super::WorkerFrame {
            version: super::VERSION,
            message: super::WorkerMessage::Result(ContributionResult {
                field: None,
                state: OutputState::Absent,
            }),
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &frame).unwrap();
        assert_eq!(
            read_frame::<super::WorkerFrame, _>(&mut Cursor::new(bytes)).unwrap(),
            frame
        );
        assert!(read_frame::<super::WorkerFrame, _>(&mut Cursor::new([0, 0, 0, 3, b'{'])).is_err());
        assert!(
            read_frame::<super::WorkerFrame, _>(&mut Cursor::new(
                (MAX_FRAME_BYTES as u32 + 1).to_be_bytes()
            ))
            .is_err()
        );
        let extra_key = br#"{"version":1,"message":{"kind":"result","field":null,"state":{"kind":"absent"},"extra":true}}"#;
        let mut bytes = (extra_key.len() as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(extra_key);
        assert!(read_frame::<super::WorkerFrame, _>(&mut Cursor::new(bytes)).is_err());
        let mut too_large = frame.clone();
        if let super::WorkerMessage::Result(result) = &mut too_large.message {
            result.state = OutputState::Failed {
                reason: "x".repeat(MAX_FRAME_BYTES),
            };
        }
        assert!(write_frame(&mut Vec::new(), &too_large).is_err());
    }

    #[test]
    fn checked_frame_entrypoints_reject_other_versions() {
        let frame = super::WorkerFrame {
            version: 2,
            message: super::WorkerMessage::Result(ContributionResult {
                field: None,
                state: OutputState::Absent,
            }),
        };
        let mut encoded = Vec::new();
        write_frame(&mut encoded, &frame).unwrap();
        assert!(super::read_worker_frame(&mut Cursor::new(encoded)).is_err());
        assert!(super::write_worker_frame(&mut Vec::new(), &frame).is_err());
    }

    #[test]
    fn result_validation_checks_field_type_unit_completeness_and_text_bounds() {
        let init = init();
        let ready = ContributionResult {
            field: Some(init.field.clone()),
            state: OutputState::Ready {
                value: OutputValue::Quantity {
                    value: 0.0,
                    unit: Unit::Metres,
                },
                completeness: Completeness::Truncated {
                    reason: "item bound".into(),
                },
            },
        };
        assert!(matches!(
            validate_result(&init, &ready).unwrap(),
            ResultState::Ready {
                value: crate::data_providers::Value::Quantity(quantity),
                completeness: crate::data_providers::Completeness::Truncated(_)
            } if quantity.unit() == ModelUnit::Metres && quantity.value() == 0.0
        ));
        let wrong_field = ContributionResult {
            field: Some("extension.sensor.reader.other".into()),
            ..ready.clone()
        };
        assert!(validate_result(&init, &wrong_field).is_err());
        let wrong_unit = ContributionResult {
            state: OutputState::Ready {
                value: OutputValue::Quantity {
                    value: 1.0,
                    unit: Unit::Seconds,
                },
                completeness: Completeness::Complete,
            },
            ..ready.clone()
        };
        assert!(validate_result(&init, &wrong_unit).is_err());
        let invalid_number = ContributionResult {
            state: OutputState::Ready {
                value: OutputValue::Quantity {
                    value: f64::NAN,
                    unit: Unit::Metres,
                },
                completeness: Completeness::Complete,
            },
            ..ready.clone()
        };
        assert!(validate_result(&init, &invalid_number).is_err());
        let long_reason = ContributionResult {
            state: OutputState::Failed {
                reason: "x".repeat(super::MAX_RESULT_TEXT_BYTES + 1),
            },
            field: None,
        };
        assert!(validate_result(&init, &long_reason).is_err());
    }
}
