//! Linux extended-attribute snapshots mapped to wrapping, selectable Properties fields.

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::properties::{
    GroupId, GroupStatus, Probe, PropertyField, PropertyGroup, PropertyProvider, PropertyValue,
};
use crate::xattr::inspection::{Inspection, Preview, Value};

/// Native attribute provider for local objects, preserving no-follow symlink semantics.
pub struct Provider;

impl PropertyProvider for Provider {
    fn id(&self) -> &'static str {
        "extended_attributes"
    }
    fn matches(&self, _: &Probe) -> bool {
        cfg!(target_os = "linux")
    }
    fn read(&self, probe: &Probe, cancel: &Arc<AtomicBool>) -> io::Result<Vec<PropertyGroup>> {
        crate::xattr::inspection::read(&probe.path, cancel)
            .map(|inspection| groups(inspection, &probe.path))
            .map_err(|error| {
                io::Error::new(error.kind(), format!("{}: {error}", probe.path.display()))
            })
    }
}

fn groups(inspection: Inspection, path: &Path) -> Vec<PropertyGroup> {
    if inspection.attributes.is_empty() && inspection.omitted_names == 0 {
        return Vec::new();
    }
    let groups = inspection
        .attributes
        .into_iter()
        .enumerate()
        .map(|(index, attribute)| {
            let mut fields = vec![text("name", attribute.name.clone())];
            let status = match attribute.value {
                Value::Ready {
                    bytes,
                    shown,
                    preview,
                } => {
                    fields.push(size(bytes));
                    let (encoding, value) = match preview {
                        Preview::Text(text) => (
                            "UTF-8 text",
                            if text.is_empty() && bytes == 0 {
                                "(empty)".into()
                            } else {
                                text
                            },
                        ),
                        Preview::Hex(hex) => ("Binary (hex)", hex),
                    };
                    fields.push(text("encoding", encoding.into()));
                    fields.push(text("value", value));
                    if shown < bytes {
                        GroupStatus::Truncated {
                            reason: format!("Showing {shown} of {bytes} bytes"),
                        }
                    } else {
                        GroupStatus::Available
                    }
                }
                Value::Omitted { bytes } => {
                    fields.push(size(bytes));
                    GroupStatus::Truncated {
                        reason: "Value not read: exceeds the remaining read or display budget"
                            .into(),
                    }
                }
                Value::Failed(error) => {
                    GroupStatus::Failed(format!("{}: {}: {error}", path.display(), attribute.name))
                }
            };
            PropertyGroup {
                id: GroupId(format!("attribute_{}", index + 1)),
                status,
                fields,
                groups: Vec::new(),
                tables: Vec::new(),
            }
        })
        .collect();
    vec![PropertyGroup {
        id: GroupId("extended_attributes".into()),
        status: if inspection.omitted_names > 0 {
            GroupStatus::Truncated {
                reason: format!(
                    "Limited to 64 attributes; {} additional names omitted",
                    inspection.omitted_names
                ),
            }
        } else {
            GroupStatus::Available
        },
        fields: Vec::new(),
        groups,
        tables: Vec::new(),
    }]
}

fn text(id: &str, value: String) -> PropertyField {
    PropertyField {
        id: id.into(),
        value: PropertyValue::Text(value),
    }
}
fn size(bytes: usize) -> PropertyField {
    PropertyField {
        id: "size_bytes".into(),
        value: PropertyValue::Unsigned(bytes as u64),
    }
}

#[cfg(test)]
mod tests {
    use super::groups;
    use crate::properties::{GroupStatus, PropertyValue};
    use crate::xattr::inspection::{Attribute, Inspection, Preview, Value};
    use std::path::Path;

    #[test]
    fn projects_binary_truncation_empty_and_failure_without_losing_other_attributes() {
        let result = groups(
            Inspection {
                omitted_names: 2,
                attributes: vec![
                    Attribute {
                        name: "user.binary".into(),
                        value: Value::Ready {
                            bytes: 4,
                            shown: 2,
                            preview: Preview::Hex("00 ff".into()),
                        },
                    },
                    Attribute {
                        name: "user.empty".into(),
                        value: Value::Ready {
                            bytes: 0,
                            shown: 0,
                            preview: Preview::Text(String::new()),
                        },
                    },
                    Attribute {
                        name: "user.denied".into(),
                        value: Value::Failed("permission denied".into()),
                    },
                ],
            },
            Path::new("/test/file"),
        );
        assert!(matches!(result[0].status, GroupStatus::Truncated { .. }));
        let children = &result[0].groups;
        assert_eq!(children.len(), 3);
        assert!(
            matches!(&children[0].status, GroupStatus::Truncated { reason } if reason == "Showing 2 of 4 bytes")
        );
        assert!(
            children[0]
                .fields
                .iter()
                .any(|field| field.value == PropertyValue::Text("Binary (hex)".into()))
        );
        assert!(
            children[1]
                .fields
                .iter()
                .any(|field| field.value == PropertyValue::Text("(empty)".into()))
        );
        assert!(
            matches!(&children[2].status, GroupStatus::Failed(error) if error == "/test/file: user.denied: permission denied")
        );
        assert!(groups(Inspection::default(), Path::new("/test/file")).is_empty());
    }
}
