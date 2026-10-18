//! Package metadata mapped to one independent Properties group.

use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::formats::package::{self, Format, Metadata};
use crate::properties::{
    GroupId, GroupStatus, Probe, PropertyField, PropertyGroup, PropertyProvider, PropertyValue,
};

/// Native RPM and Debian metadata provider.
pub struct Provider;

impl PropertyProvider for Provider {
    fn id(&self) -> &'static str {
        "package"
    }
    fn matches(&self, probe: &Probe) -> bool {
        probe.is_file && package::candidate(&probe.path).is_some()
    }
    fn read(&self, probe: &Probe, cancel: &Arc<AtomicBool>) -> io::Result<Vec<PropertyGroup>> {
        let format = package::candidate(&probe.path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{}: package format no longer recognized",
                    probe.path.display()
                ),
            )
        })?;
        package::read(&probe.path, format, cancel)
            .map(|metadata| vec![group(metadata)])
            .map_err(|error| {
                io::Error::new(error.kind(), format!("{}: {error}", probe.path.display()))
            })
    }
}

fn group(metadata: Metadata) -> PropertyGroup {
    let mut fields = vec![PropertyField {
        id: "format".into(),
        value: PropertyValue::Text(
            match metadata.format {
                Format::Rpm => "RPM",
                Format::Deb => "DEB",
            }
            .into(),
        ),
    }];
    fields.extend(
        metadata
            .fields
            .into_iter()
            .map(|(field, text)| PropertyField {
                id: field.id().into(),
                value: PropertyValue::Text(text),
            }),
    );
    if let Some(size) = metadata.installed_size {
        fields.push(PropertyField {
            id: "installed_size_bytes".into(),
            value: PropertyValue::Unsigned(size),
        });
    }
    if let Some(epoch) = metadata.epoch {
        fields.push(PropertyField {
            id: "epoch".into(),
            value: PropertyValue::Unsigned(epoch),
        });
    }
    PropertyGroup {
        id: GroupId("package".into()),
        status: if metadata.truncated {
            GroupStatus::Truncated {
                reason:
                    "Package text limited to 4,096 characters per value or invalid UTF-8 replaced"
                        .into(),
            }
        } else {
            GroupStatus::Available
        },
        fields,
        groups: Vec::new(),
        tables: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::group;
    use crate::formats::package::{Field, Format, Metadata};
    use crate::properties::{GroupStatus, PropertyValue};

    #[test]
    fn projects_semantic_fields_and_explicit_truncation() {
        let result = group(Metadata {
            format: Format::Rpm,
            fields: vec![(Field::Name, "demo".into())],
            installed_size: Some(2048),
            epoch: Some(2),
            truncated: true,
        });
        assert_eq!(result.id.0, "package");
        assert!(matches!(result.status, GroupStatus::Truncated { .. }));
        assert!(
            result
                .fields
                .iter()
                .any(|field| field.id == "installed_size_bytes"
                    && field.value == PropertyValue::Unsigned(2048))
        );
        assert!(
            result
                .fields
                .iter()
                .any(|field| field.id == "epoch" && field.value == PropertyValue::Unsigned(2))
        );
    }
}
