//! Adapts declared extension Properties contributions to the application Properties model.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::data_providers::{ContributionId, ResultState};
use crate::properties::Selection;

use super::loading::Package;
use super::manifest::MAX_FIELDS_PER_PROVIDER;
use super::results::{PreparedResult, PropertiesResult};
use super::supervisor::{ContributionWork, Supervisor};

/// One declared extension Properties contribution retained by the application.
#[derive(Clone)]
pub struct Handle {
    package: Arc<Package>,
    id: ContributionId,
    label: String,
    supervisor: Supervisor,
}

impl Handle {
    /// Creates a handle after verifying its declared Properties fields.
    pub fn new(
        package: Arc<Package>,
        id: ContributionId,
        supervisor: Supervisor,
    ) -> io::Result<Self> {
        let contribution = package
            .manifest
            .providers
            .iter()
            .flat_map(|provider| &provider.properties)
            .find(|contribution| contribution.header.id == id)
            .ok_or_else(|| invalid("package does not declare this Properties contribution"))?;
        if contribution.fields.is_empty()
            || contribution.fields.len() > MAX_FIELDS_PER_PROVIDER
            || contribution
                .fields
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != contribution.fields.len()
        {
            return Err(invalid(
                "Properties contribution fields must be nonempty and unique",
            ));
        }
        let label = contribution.header.label.clone();
        Ok(Self {
            package,
            id,
            label,
            supervisor,
        })
    }

    /// Returns the stable contribution identity.
    pub fn id(&self) -> &ContributionId {
        &self.id
    }

    /// Returns the manifest-provided section label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Reads this contribution for the original selection in a background task.
    pub fn read(
        &self,
        selection: &Selection,
        cancel: &Arc<AtomicBool>,
    ) -> io::Result<PreparedSection> {
        if cancel.load(Ordering::Acquire) {
            return Err(interrupted());
        }
        if selection.location.as_local_path().is_none() {
            return Ok(self.unavailable("extensions require a local regular file"));
        }
        if selection.entry.is_dir {
            return Ok(self.unavailable("extensions require a local regular file"));
        }
        let ticket = self.supervisor.submit_contribution(ContributionWork {
            package: self.package.clone(),
            contribution: self.id.clone(),
            source: selection.location.clone(),
            name: selection.entry.name.clone(),
            is_dir: selection.entry.is_dir,
            generation_cancelled: cancel.clone(),
        })?;
        let result = ticket.wait();
        if cancel.load(Ordering::Acquire) {
            return Err(interrupted());
        }
        match result? {
            PreparedResult::Properties(result) => Ok(PreparedSection {
                id: self.id.clone(),
                label: self.label.clone(),
                result,
            }),
            _ => Err(invalid("worker returned a different presentation result")),
        }
    }

    /// Creates a visible independent failure section for an orchestration error.
    pub fn failure(&self, error: &io::Error) -> PreparedSection {
        PreparedSection {
            id: self.id.clone(),
            label: self.label.clone(),
            result: PropertiesResult::Failed(error.to_string()),
        }
    }

    fn unavailable(&self, message: &str) -> PreparedSection {
        PreparedSection {
            id: self.id.clone(),
            label: self.label.clone(),
            result: PropertiesResult::Unavailable(message.into()),
        }
    }
}

/// One independently prepared extension Properties section.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedSection {
    /// Stable contribution identity.
    pub id: ContributionId,
    /// Manifest-provided display label.
    pub label: String,
    /// Result for the declared fields.
    pub result: PropertiesResult,
}

impl PreparedSection {
    /// Whether this section contains useful values or a visible failure.
    pub fn is_visible(&self) -> bool {
        match &self.result {
            PropertiesResult::Failed(_) => true,
            PropertiesResult::Ready(fields) => fields.iter().any(|field| {
                matches!(
                    field.state,
                    ResultState::Ready { .. } | ResultState::Failed(_)
                )
            }),
            PropertiesResult::Absent | PropertiesResult::Unavailable(_) => false,
        }
    }
}

fn interrupted() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "cancelled")
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::PreparedSection;
    use crate::data_providers::{ContributionId, FieldId, ResultState};
    use crate::extensions::results::{FieldResult, PropertiesResult};

    fn field(state: ResultState<crate::data_providers::Value>) -> FieldResult {
        FieldResult {
            field: FieldId::new("extension.demo.provider.width").expect("valid test field ID"),
            state,
        }
    }

    fn section(result: PropertiesResult) -> PreparedSection {
        PreparedSection {
            id: ContributionId::new("extension.demo.provider.details")
                .expect("valid test contribution ID"),
            label: "Details".into(),
            result,
        }
    }

    #[test]
    fn visibility_retains_useful_or_failed_fields_but_hides_empty_results() {
        assert!(
            section(PropertiesResult::Ready(vec![field(ResultState::Ready {
                value: crate::data_providers::Value::Text("value".into()),
                completeness: crate::data_providers::Completeness::Complete,
            })]))
            .is_visible()
        );
        assert!(
            section(PropertiesResult::Ready(vec![field(ResultState::Failed(
                "field failed".into(),
            ))]))
            .is_visible()
        );
        assert!(
            !section(PropertiesResult::Ready(vec![
                field(ResultState::Absent),
                field(ResultState::Unavailable("not supported".into())),
                field(ResultState::Pending),
            ]))
            .is_visible()
        );
        assert!(!section(PropertiesResult::Absent).is_visible());
        assert!(!section(PropertiesResult::Unavailable("no source".into())).is_visible());
        assert!(section(PropertiesResult::Failed("worker stopped".into())).is_visible());
    }
}
