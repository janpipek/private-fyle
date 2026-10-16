//! Validated stable identifiers for provider-owned fields and contributions.

use std::io;

/// A provider identity in a reserved built-in or extension namespace.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProviderId(String);

impl ProviderId {
    /// Creates a provider ID with valid lowercase dot-separated segments.
    pub fn new(value: impl Into<String>) -> io::Result<Self> {
        let value = value.into();
        validate_segments(&value)?;
        let segments = value.split('.').count();
        if !((value.starts_with("builtin.") && segments == 2)
            || (value.starts_with("extension.") && segments == 3))
        {
            return Err(invalid("provider ID must use a reserved namespace"));
        }
        Ok(Self(value))
    }

    /// Returns the stable spelling of this provider ID.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A provider-owned contribution identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContributionId(String);

impl ContributionId {
    /// Creates a contribution ID with valid lowercase dot-separated segments.
    pub fn new(value: impl Into<String>) -> io::Result<Self> {
        let value = value.into();
        validate_segments(&value)?;
        Ok(Self(value))
    }

    /// Returns the stable spelling of this contribution ID.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A provider-owned semantic field identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FieldId(String);

impl FieldId {
    /// Creates a field ID with valid lowercase dot-separated segments.
    pub fn new(value: impl Into<String>) -> io::Result<Self> {
        let value = value.into();
        validate_segments(&value)?;
        Ok(Self(value))
    }

    /// Returns the stable spelling of this field ID.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_segments(value: &str) -> io::Result<()> {
    if value.split('.').all(|segment| {
        !segment.is_empty()
            && segment.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte)
            })
    }) {
        Ok(())
    } else {
        Err(invalid(
            "ID must contain lowercase ASCII dot-separated segments",
        ))
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::{ContributionId, FieldId, ProviderId};

    #[test]
    fn identifiers_reject_empty_or_invalid_segments() {
        assert!(ProviderId::new("sensor.reader").is_err());
        assert!(ProviderId::new("builtin..reader").is_err());
        assert!(ProviderId::new("builtin.a.b").is_err());
        assert!(ProviderId::new("extension.sensor").is_err());
        assert!(ProviderId::new("extension.Sensor").is_err());
        assert!(ContributionId::new("extension.sensor.").is_err());
        assert!(FieldId::new("extension.sensor.é").is_err());
    }
}
