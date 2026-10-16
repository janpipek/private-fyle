//! UI-independent contracts for providers that contribute prepared data.

use std::io;

use crate::location::Location;

mod identity;

/// GPX/FIT provider shared by the Properties and View adapters.
pub mod geo;

/// Validated IDs exposed to provider definitions and prepared observations.
pub use identity::{ContributionId, FieldId, ProviderId};

/// Where a provider definition came from; namespace trust is never inferred from its ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistrationOrigin {
    /// Compiled into fyle.
    Builtin,
    /// Declared by an enabled user package.
    Extension,
}

/// The semantic type declared for a provider field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// Human-readable text.
    Text,
    /// A finite, nonnegative measurement with an explicit unit.
    Quantity(Unit),
}

/// Units needed by the first provider examples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// Elapsed time in seconds.
    Seconds,
    /// Distance in metres.
    Metres,
}

/// A validated provider value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// Textual data.
    Text(String),
    /// A finite, nonnegative measurement.
    Quantity(Quantity),
}

/// A finite, nonnegative numeric quantity with a unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quantity {
    value: f64,
    unit: Unit,
}

impl Quantity {
    /// Constructs a quantity after rejecting negative and nonfinite values.
    pub fn new(value: f64, unit: Unit) -> io::Result<Self> {
        if !value.is_finite() || value < 0.0 {
            return Err(invalid("quantity must be finite and nonnegative"));
        }
        Ok(Self { value, unit })
    }

    /// Returns the numeric magnitude.
    pub fn value(self) -> f64 {
        self.value
    }

    /// Returns the unit associated with this magnitude.
    pub fn unit(self) -> Unit {
        self.unit
    }
}

impl Value {
    /// Constructs a quantity after rejecting negative and nonfinite values.
    pub fn quantity(value: f64, unit: Unit) -> io::Result<Self> {
        Ok(Self::Quantity(Quantity::new(value, unit)?))
    }
}

/// A stable field definition owned by one provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldDefinition {
    /// Provider-qualified identity.
    pub id: FieldId,
    /// The accepted value type and unit.
    pub kind: FieldKind,
}

impl FieldDefinition {
    /// Checks that a value matches this field's declared type and unit.
    pub fn validate(&self, value: &Value) -> io::Result<()> {
        match (self.kind, value) {
            (FieldKind::Text, Value::Text(_)) => Ok(()),
            (FieldKind::Quantity(expected), Value::Quantity(quantity))
                if expected == quantity.unit() =>
            {
                Ok(())
            }
            _ => Err(invalid("value does not match its field definition")),
        }
    }
}

/// A stable producer-qualified contribution header shared by presentations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContributionHeader {
    /// Stable identity.
    pub id: ContributionId,
    /// Display name.
    pub label: String,
    /// Captured facts required for this contribution to apply.
    pub applicability: Applicability,
    /// Maximum acquisition bounds requested by this contribution.
    pub demand: DemandLimits,
}

/// A Properties contribution composed from declared fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertiesContribution {
    /// Shared contribution identity and label.
    pub header: ContributionHeader,
    /// Fields projected into this section.
    pub fields: Vec<FieldId>,
}

/// A column contribution with a stable persisted spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnContribution {
    /// Shared contribution identity and label.
    pub header: ContributionHeader,
    /// Persisted column identifier; built-in spellings remain compatible.
    pub column_id: String,
    /// Field projected into this column.
    pub field: FieldId,
}

/// A viewer contribution declaring the representation it prepares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewerContribution {
    /// Shared contribution identity and label.
    pub header: ContributionHeader,
    /// Stable representation name.
    pub representation: String,
    /// Representation version.
    pub version: u32,
}

/// Contributions registered as one provider-owned bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderDefinition {
    /// Stable provider identity.
    pub id: ProviderId,
    /// Declared semantic fields.
    pub fields: Vec<FieldDefinition>,
    /// Properties contributions.
    pub properties: Vec<PropertiesContribution>,
    /// Column contributions.
    pub columns: Vec<ColumnContribution>,
    /// Viewer contributions.
    pub viewers: Vec<ViewerContribution>,
}

/// Validates ownership and uniqueness across a registration batch.
pub fn validate_definitions(
    definitions: &[(RegistrationOrigin, ProviderDefinition)],
) -> io::Result<()> {
    let mut providers = Vec::new();
    let mut contributions = Vec::new();
    let mut columns = Vec::new();
    let mut fields_seen = Vec::new();
    let mut git_alias = false;
    let mut git_status = false;
    for (origin, definition) in definitions {
        let prefix = match origin {
            RegistrationOrigin::Builtin => "builtin.",
            RegistrationOrigin::Extension => "extension.",
        };
        if !definition.id.as_str().starts_with(prefix) {
            return Err(invalid("provider ID does not match registration origin"));
        }
        if providers.iter().any(|id| id == &definition.id) {
            return Err(invalid("duplicate provider ID"));
        }
        providers.push(definition.id.clone());
        let mut fields = Vec::new();
        for field in &definition.fields {
            if !field
                .id
                .as_str()
                .starts_with(&format!("{}.", definition.id.as_str()))
            {
                return Err(invalid("field ID is not owned by its provider"));
            }
            if fields.contains(&field.id) {
                return Err(invalid("duplicate field ID"));
            }
            if fields_seen.contains(&field.id) {
                return Err(invalid("duplicate field ID across providers"));
            }
            fields.push(field.id.clone());
            fields_seen.push(field.id.clone());
        }
        for header in definition
            .properties
            .iter()
            .map(|value| &value.header)
            .chain(definition.columns.iter().map(|value| &value.header))
            .chain(definition.viewers.iter().map(|value| &value.header))
        {
            if !header
                .id
                .as_str()
                .starts_with(&format!("{}.", definition.id.as_str()))
            {
                return Err(invalid("contribution ID is not owned by its provider"));
            }
            if contributions.contains(&header.id) {
                return Err(invalid("duplicate contribution ID"));
            }
            contributions.push(header.id.clone());
        }
        for field in definition
            .properties
            .iter()
            .flat_map(|value| &value.fields)
            .chain(definition.columns.iter().map(|value| &value.field))
        {
            if !fields.contains(field) {
                return Err(invalid("contribution references an undeclared field"));
            }
        }
        for column in &definition.columns {
            if FieldId::new(column.column_id.clone()).is_err()
                || columns.contains(&column.column_id)
            {
                return Err(invalid("empty or duplicate persisted column ID"));
            }
            if origin == &RegistrationOrigin::Extension
                && !column
                    .column_id
                    .starts_with(&format!("{}.", definition.id.as_str()))
            {
                return Err(invalid("extension column ID must be provider-qualified"));
            }
            if origin == &RegistrationOrigin::Builtin
                && column.column_id == "git"
                && definition.id.as_str() != "builtin.git"
            {
                return Err(invalid("the git compatibility alias is reserved"));
            }
            git_alias |= column.column_id == "git";
            git_status |= column.column_id == "git.status";
            columns.push(column.column_id.clone());
        }
    }
    if git_alias && git_status {
        return Err(invalid(
            "git alias cannot be registered alongside git.status",
        ));
    }
    Ok(())
}

/// The item whose facts or content a provider describes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subject {
    /// Facts about the selected directory entry or file.
    Item(Location),
    /// Facts about the symbolic link object itself.
    LinkObject(Location),
    /// Facts about a link and the distinct target it names.
    LinkTarget { link: Location, target: Location },
}

/// A content or context revision; unknown values are never reusable.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Revision {
    /// The producer cannot establish a stable revision.
    Unknown,
    /// Stable token captured with the request.
    Known(String),
}

/// Identity for surrounding context such as a repository root.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContextRevision {
    /// Context source identity, such as a repository root with native path bytes.
    pub identity: Location,
    /// Context-specific revision token.
    pub revision: Revision,
}

/// Resource limits requested for one acquisition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DemandLimits {
    /// Maximum input bytes requested.
    pub max_bytes: u64,
    /// Maximum items requested.
    pub max_items: u64,
}

/// Data identity separated from the UI request generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestIdentity {
    /// Contribution being prepared.
    pub contribution: ContributionId,
    /// Provider producing the result.
    pub producer: ProviderId,
    /// Exact source subject.
    pub subject: Subject,
    /// Content revision, independent from surrounding context.
    pub source_revision: Revision,
    /// Optional context identity and revision.
    pub context: Option<ContextRevision>,
    /// Requested acquisition bounds.
    pub demand: DemandLimits,
    /// UI-owned request generation.
    pub generation: u64,
}

/// Whether a result covers its requested data completely.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Completeness {
    /// All requested data was acquired.
    Complete,
    /// Acquisition stopped at a known bound.
    Truncated(String),
    /// The value is known to be at least this much.
    LowerBound,
}

/// Lifecycle state of one independent contribution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResultState<T> {
    /// Work has been requested but has no result yet.
    Pending,
    /// A value is ready with its own completeness.
    Ready {
        value: T,
        completeness: Completeness,
    },
    /// The provider determined there is no value.
    Absent,
    /// The provider does not support this request.
    Unavailable(String),
    /// This contribution failed independently of others.
    Failed(String),
}

/// A semantic fact with its source, producer and claim history preserved.
#[derive(Clone, Debug, PartialEq)]
pub struct Observation {
    /// Declared field identity.
    pub field: FieldId,
    /// Provider that produced this value.
    pub producer: ProviderId,
    /// Exact item or link subject.
    pub subject: Subject,
    /// Source revision captured during acquisition.
    pub source_revision: Revision,
    /// Surrounding context revision, when applicable.
    pub context: Option<ContextRevision>,
    /// Typed value.
    pub value: Value,
    /// Whether this value was directly recorded or derived.
    pub provenance: Provenance,
    /// Completeness of the value.
    pub completeness: Completeness,
}

/// Whether an observation was recorded in the source or inferred by a provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    /// Read directly from source data.
    Recorded,
    /// Derived from other source data.
    Inferred,
}

/// Facts captured before applicability is evaluated; constructing it performs no I/O.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicabilityContext {
    /// Original typed source identity.
    pub subject: Subject,
    /// Captured display name.
    pub name: String,
    /// Whether the selected item is a directory.
    pub is_dir: bool,
    /// Captured format identifiers.
    pub formats: Vec<String>,
    /// Known surrounding context, such as a Git repository.
    pub context_identity: Option<ContextIdentity>,
}

/// Small applicability predicates over captured facts only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Applicability {
    /// Applies to every captured item.
    Any,
    /// Applies when captured format evidence contains this ID.
    Format(String),
    /// Applies when this surrounding context was captured.
    Context(String),
}

impl Applicability {
    /// Evaluates applicability without filesystem or repository discovery.
    pub fn matches(&self, context: &ApplicabilityContext) -> bool {
        match self {
            Self::Any => true,
            Self::Format(format) => context.formats.contains(format),
            Self::Context(kind) => context
                .context_identity
                .as_ref()
                .is_some_and(|identity| identity.kind == *kind),
        }
    }
}

/// Typed identity for captured surrounding context.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContextIdentity {
    /// Context kind such as `git`.
    pub kind: String,
    /// Native location identifying the context root.
    pub location: Location,
}

/// Decides whether a completion still belongs to the active request.
pub fn accepts_result(current: &RequestIdentity, completed: &RequestIdentity) -> bool {
    current == completed
}

/// Whether a prepared result can satisfy a later request without new acquisition.
pub fn can_reuse(cached: &RequestIdentity, requested: &RequestIdentity) -> bool {
    cached.contribution == requested.contribution
        && cached.producer == requested.producer
        && cached.subject == requested.subject
        && cached.source_revision == requested.source_revision
        && cached.context == requested.context
        && cached.demand == requested.demand
        && cached.source_revision != Revision::Unknown
        && cached
            .context
            .as_ref()
            .is_none_or(|context| context.revision != Revision::Unknown)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::{
        Applicability, ApplicabilityContext, ColumnContribution, Completeness, ContextIdentity,
        ContextRevision, ContributionHeader, DemandLimits, FieldDefinition, FieldKind, Observation,
        PropertiesContribution, Provenance, ProviderDefinition, RegistrationOrigin,
        RequestIdentity, ResultState, Revision, Subject, Unit, Value, ViewerContribution,
        accepts_result, can_reuse, validate_definitions,
    };
    use crate::location::Location;
    use std::io;
    use std::path::PathBuf;

    fn id_provider(value: &str) -> super::ProviderId {
        super::ProviderId::new(value).expect("valid provider ID")
    }

    fn id_field(value: &str) -> super::FieldId {
        super::FieldId::new(value).expect("valid field ID")
    }

    fn id_contribution(value: &str) -> super::ContributionId {
        super::ContributionId::new(value).expect("valid contribution ID")
    }

    fn absolute(value: &str) -> PathBuf {
        std::env::current_dir()
            .expect("test directory exists")
            .join(value)
    }

    struct SensorData {
        duration_seconds: f64,
        distance_metres: f64,
        samples: Vec<(f64, f64)>,
    }

    fn project_sensor(
        definition: &ProviderDefinition,
        subject: &Subject,
        source_revision: &Revision,
        input: &SensorData,
    ) -> io::Result<Vec<Observation>> {
        let values = [
            Value::quantity(input.duration_seconds, Unit::Seconds)?,
            Value::quantity(input.distance_metres, Unit::Metres)?,
        ];
        definition
            .fields
            .iter()
            .zip(values)
            .map(|(field, value)| {
                field.validate(&value)?;
                Ok(Observation {
                    field: field.id.clone(),
                    producer: definition.id.clone(),
                    subject: subject.clone(),
                    source_revision: source_revision.clone(),
                    context: None,
                    value,
                    provenance: Provenance::Recorded,
                    completeness: Completeness::Complete,
                })
            })
            .collect()
    }

    fn sensor() -> (RegistrationOrigin, ProviderDefinition) {
        let provider = id_provider("extension.sensor.reader");
        let duration = id_field("extension.sensor.reader.duration");
        let distance = id_field("extension.sensor.reader.distance");
        let property = PropertiesContribution {
            header: ContributionHeader {
                id: id_contribution("extension.sensor.reader.properties"),
                label: "Sensor activity".into(),
                applicability: Applicability::Format("sensor.activity".into()),
                demand: DemandLimits {
                    max_bytes: 16 * 1024 * 1024,
                    max_items: 100_000,
                },
            },
            fields: vec![duration.clone(), distance.clone()],
        };
        let column = ColumnContribution {
            header: ContributionHeader {
                id: id_contribution("extension.sensor.reader.duration-column"),
                label: "Duration".into(),
                applicability: Applicability::Format("sensor.activity".into()),
                demand: DemandLimits {
                    max_bytes: 16 * 1024 * 1024,
                    max_items: 100_000,
                },
            },
            column_id: "extension.sensor.reader.duration".into(),
            field: duration.clone(),
        };
        let viewer = ViewerContribution {
            header: ContributionHeader {
                id: id_contribution("extension.sensor.reader.graph"),
                label: "Sensor graph".into(),
                applicability: Applicability::Format("sensor.activity".into()),
                demand: DemandLimits {
                    max_bytes: 16 * 1024 * 1024,
                    max_items: 100_000,
                },
            },
            representation: "sensor.graph".into(),
            version: 1,
        };
        (
            RegistrationOrigin::Extension,
            ProviderDefinition {
                id: provider,
                fields: vec![
                    FieldDefinition {
                        id: duration,
                        kind: FieldKind::Quantity(Unit::Seconds),
                    },
                    FieldDefinition {
                        id: distance,
                        kind: FieldKind::Quantity(Unit::Metres),
                    },
                ],
                properties: vec![property],
                columns: vec![column],
                viewers: vec![viewer],
            },
        )
    }

    fn git() -> (RegistrationOrigin, ProviderDefinition) {
        let status = id_field("builtin.git.status");
        (
            RegistrationOrigin::Builtin,
            ProviderDefinition {
                id: id_provider("builtin.git"),
                fields: vec![FieldDefinition {
                    id: status.clone(),
                    kind: FieldKind::Text,
                }],
                properties: vec![PropertiesContribution {
                    header: ContributionHeader {
                        id: id_contribution("builtin.git.properties"),
                        label: "Git".into(),
                        applicability: Applicability::Context("git".into()),
                        demand: DemandLimits {
                            max_bytes: 0,
                            max_items: 1,
                        },
                    },
                    fields: vec![status.clone()],
                }],
                columns: vec![ColumnContribution {
                    header: ContributionHeader {
                        id: id_contribution("builtin.git.status-column"),
                        label: "Git status".into(),
                        applicability: Applicability::Context("git".into()),
                        demand: DemandLimits {
                            max_bytes: 0,
                            max_items: 1,
                        },
                    },
                    column_id: "git.status".into(),
                    field: status,
                }],
                viewers: Vec::new(),
            },
        )
    }

    fn request() -> RequestIdentity {
        RequestIdentity {
            contribution: id_contribution("builtin.git.properties"),
            producer: id_provider("builtin.git"),
            subject: Subject::Item(Location::local(absolute("repo/a.txt")).expect("absolute path")),
            source_revision: Revision::Known("size=10;mtime=1".into()),
            context: Some(ContextRevision {
                identity: Location::local(absolute("repo")).expect("absolute path"),
                revision: Revision::Known("index=1".into()),
            }),
            demand: DemandLimits {
                max_bytes: 0,
                max_items: 1,
            },
            generation: 1,
        }
    }

    #[test]
    fn sensor_and_git_register_distinct_presentations_and_keep_public_git_column() {
        let mut sensor = sensor();
        let git = git();
        validate_definitions(&[sensor.clone(), git.clone()]).expect("definitions validate");
        assert_eq!(sensor.1.properties.len(), 1);
        assert_eq!(sensor.1.columns.len(), 1);
        assert_eq!(sensor.1.viewers.len(), 1);
        assert_eq!(git.1.columns[0].column_id, "git.status");
        assert!(git.1.viewers.is_empty());

        let sensor_context = ApplicabilityContext {
            subject: Subject::Item(
                Location::local(absolute("data/run.sensor")).expect("absolute path"),
            ),
            name: "run.sensor".into(),
            is_dir: false,
            formats: vec!["sensor.activity".into()],
            context_identity: None,
        };
        assert!(
            sensor.1.properties[0]
                .header
                .applicability
                .matches(&sensor_context)
        );
        let input = SensorData {
            duration_seconds: 61.5,
            distance_metres: 420.0,
            samples: vec![(0.0, 0.0), (1.0, 4.0)],
        };
        let observations = project_sensor(
            &sensor.1,
            &sensor_context.subject,
            &Revision::Known("sensor-v1".into()),
            &input,
        )
        .expect("project sensor values");
        assert!(
            matches!(observations[0].value, Value::Quantity(quantity) if quantity.unit() == Unit::Seconds && quantity.value() == 61.5)
        );
        assert!(
            matches!(observations[1].value, Value::Quantity(quantity) if quantity.unit() == Unit::Metres && quantity.value() == 420.0)
        );
        let property_result = ResultState::Ready {
            value: observations.clone(),
            completeness: Completeness::Complete,
        };
        let column_field = &sensor.1.columns[0].field;
        let column_value = observations
            .iter()
            .find(|observation| &observation.field == column_field)
            .expect("column field is projected")
            .value
            .clone();
        let column_result = ResultState::Ready {
            value: column_value,
            completeness: Completeness::Complete,
        };
        sensor.1.viewers[0].header.demand.max_items = 1;
        let viewer_result: ResultState<Vec<(f64, f64)>> = ResultState::Ready {
            value: input
                .samples
                .iter()
                .take(sensor.1.viewers[0].header.demand.max_items as usize)
                .copied()
                .collect(),
            completeness: Completeness::Truncated("sample bound".into()),
        };
        assert!(matches!(property_result, ResultState::Ready { .. }));
        assert!(matches!(&column_result, ResultState::Ready { .. }));
        assert!(
            matches!(&column_result, ResultState::Ready { value: Value::Quantity(quantity), .. } if quantity.unit() == Unit::Seconds)
        );
        assert!(
            matches!(&viewer_result, ResultState::Ready { value, completeness: Completeness::Truncated(_), .. } if value.len() == 1)
        );
        sensor.1.columns[0].field = observations[1].field.clone();
        let rebound_value = observations
            .iter()
            .find(|observation| observation.field == sensor.1.columns[0].field)
            .expect("rebound column field is projected")
            .value
            .clone();
        assert!(
            matches!(rebound_value, Value::Quantity(quantity) if quantity.unit() == Unit::Metres)
        );
        assert!(matches!(
            viewer_result,
            ResultState::Ready {
                completeness: Completeness::Truncated(_),
                ..
            }
        ));

        let git_context = ApplicabilityContext {
            subject: Subject::Item(
                Location::local(absolute("repo/run.sensor")).expect("absolute path"),
            ),
            name: "run.sensor".into(),
            is_dir: false,
            formats: Vec::new(),
            context_identity: Some(ContextIdentity {
                kind: "git".into(),
                location: Location::local(absolute("repo")).expect("absolute path"),
            }),
        };
        assert!(
            git.1.properties[0]
                .header
                .applicability
                .matches(&git_context)
        );
        let git_status = Value::Text("modified".into());
        git.1.fields[0]
            .validate(&git_status)
            .expect("Git status type");
        let git_property: ResultState<Observation> = ResultState::Ready {
            value: Observation {
                field: git.1.fields[0].id.clone(),
                producer: git.1.id.clone(),
                subject: git_context.subject,
                source_revision: Revision::Known("item-v1".into()),
                context: Some(ContextRevision {
                    identity: Location::local(absolute("repo")).expect("absolute path"),
                    revision: Revision::Known("index-v1".into()),
                }),
                value: git_status.clone(),
                provenance: Provenance::Recorded,
                completeness: Completeness::Complete,
            },
            completeness: Completeness::Complete,
        };
        let git_column: ResultState<Value> = ResultState::Ready {
            value: git_status,
            completeness: Completeness::Complete,
        };
        assert!(matches!(git_property, ResultState::Ready { .. }));
        assert!(matches!(git_column, ResultState::Ready { .. }));
    }

    #[test]
    fn rejects_duplicate_ids_wrong_origin_and_foreign_references() {
        let definition = sensor();
        assert!(validate_definitions(&[definition.clone(), definition]).is_err());
        let mut wrong_origin = sensor();
        wrong_origin.0 = RegistrationOrigin::Builtin;
        assert!(validate_definitions(&[wrong_origin]).is_err());
        let mut foreign = sensor();
        foreign.1.columns[0].field = id_field("extension.other.field");
        assert!(validate_definitions(&[foreign]).is_err());
        let mut duplicate_field = sensor();
        duplicate_field
            .1
            .fields
            .push(duplicate_field.1.fields[0].clone());
        assert!(validate_definitions(&[duplicate_field]).is_err());
        let mut duplicate_contribution = sensor();
        duplicate_contribution.1.viewers[0].header.id =
            duplicate_contribution.1.properties[0].header.id.clone();
        assert!(validate_definitions(&[duplicate_contribution]).is_err());
        let mut invalid_column = sensor();
        invalid_column.1.columns[0].column_id = "extension.sensor.reader.??".into();
        assert!(validate_definitions(&[invalid_column]).is_err());
        let mut alias = git();
        alias.1.columns[0].column_id = "git".into();
        alias.1.columns.push(ColumnContribution {
            header: ContributionHeader {
                id: id_contribution("builtin.git.legacy-status-column"),
                label: "Legacy Git".into(),
                applicability: Applicability::Context("git".into()),
                demand: DemandLimits {
                    max_bytes: 0,
                    max_items: 1,
                },
            },
            column_id: "git.status".into(),
            field: alias.1.fields[0].id.clone(),
        });
        assert!(validate_definitions(&[alias]).is_err());
    }

    #[test]
    fn quantities_are_finite_nonnegative_and_match_declared_unit() {
        let definition = FieldDefinition {
            id: id_field("extension.sensor.reader.duration"),
            kind: FieldKind::Quantity(Unit::Seconds),
        };
        assert!(Value::quantity(f64::NAN, Unit::Seconds).is_err());
        assert!(Value::quantity(f64::INFINITY, Unit::Seconds).is_err());
        assert!(Value::quantity(-1.0, Unit::Seconds).is_err());
        assert!(Value::quantity(f64::NEG_INFINITY, Unit::Seconds).is_err());
        definition
            .validate(&Value::quantity(0.0, Unit::Seconds).expect("zero quantity"))
            .expect("zero is valid");
        definition
            .validate(&Value::quantity(30.0, Unit::Seconds).expect("valid quantity"))
            .expect("unit matches");
        assert!(
            definition
                .validate(&Value::quantity(30.0, Unit::Metres).expect("valid quantity"))
                .is_err()
        );
        assert!(definition.validate(&Value::Text("30".into())).is_err());
        FieldDefinition {
            id: id_field("extension.sensor.reader.note"),
            kind: FieldKind::Text,
        }
        .validate(&Value::Text("recorded".into()))
        .expect("text field accepts text");
    }

    #[test]
    fn contributions_keep_independent_failure_and_completeness() {
        let properties: ResultState<Vec<Observation>> = ResultState::Ready {
            value: Vec::new(),
            completeness: Completeness::Truncated("sample limit".into()),
        };
        let viewer: ResultState<String> = ResultState::Failed("graph decode failed".into());
        assert!(matches!(
            properties,
            ResultState::Ready {
                completeness: Completeness::Truncated(_),
                ..
            }
        ));
        assert!(matches!(viewer, ResultState::Failed(_)));
        assert_ne!(Provenance::Recorded, Provenance::Inferred);
    }

    #[test]
    fn link_object_and_target_are_distinct_subjects() {
        let link = Location::local(absolute("repo/link")).expect("absolute path");
        let target = Location::local(absolute("repo/data")).expect("absolute path");
        assert_ne!(
            Subject::LinkObject(link.clone()),
            Subject::Item(target.clone())
        );
        assert_ne!(
            Subject::LinkTarget {
                link: link.clone(),
                target: target.clone()
            },
            Subject::LinkObject(link)
        );
    }

    #[cfg(unix)]
    #[test]
    fn source_and_context_identity_preserve_non_utf8_path_bytes() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let path = std::env::current_dir()
            .expect("test directory exists")
            .join(OsString::from_vec(vec![b'x', 0xff]));
        let location = Location::local(path).expect("absolute native path");
        let mut captured = request();
        captured.subject = Subject::Item(location.clone());
        captured.context.as_mut().expect("context exists").identity = location.clone();
        assert_eq!(captured.subject, Subject::Item(location.clone()));
        assert_eq!(captured.context.expect("context exists").identity, location);
    }

    #[test]
    fn freshness_checks_generation_and_revisions_but_reuse_ignores_generation() {
        let original = request();
        let mut later = original.clone();
        later.generation += 1;
        assert!(!accepts_result(&later, &original));
        assert!(can_reuse(&original, &later));
        later = original.clone();
        later.source_revision = Revision::Known("changed".into());
        assert!(!accepts_result(&later, &original));
        assert!(!can_reuse(&original, &later));
        later = original.clone();
        later.context.as_mut().expect("context exists").identity =
            Location::local(absolute("other-repository")).expect("absolute path");
        assert!(!accepts_result(&later, &original));
        assert!(!can_reuse(&original, &later));
        later = original.clone();
        later.context.as_mut().expect("context exists").revision =
            Revision::Known("changed".into());
        assert!(!accepts_result(&later, &original));
        assert!(!can_reuse(&original, &later));
        later = original.clone();
        later.subject =
            Subject::Item(Location::local(absolute("repo/other.txt")).expect("absolute path"));
        assert!(!accepts_result(&later, &original));
        later = original.clone();
        later.demand.max_items += 1;
        assert!(!accepts_result(&later, &original));
        assert!(!can_reuse(&original, &later));
        let mut unknown = original.clone();
        unknown.source_revision = Revision::Unknown;
        assert!(accepts_result(&unknown, &unknown));
        assert!(!can_reuse(&unknown, &unknown));
        let mut unknown_context = original.clone();
        unknown_context
            .context
            .as_mut()
            .expect("context exists")
            .revision = Revision::Unknown;
        assert!(!can_reuse(&unknown_context, &unknown_context));
        let mut context_free = original.clone();
        context_free.context = None;
        assert!(can_reuse(&context_free, &context_free));
    }

    #[test]
    fn applicability_uses_captured_format_and_repository_context() {
        let local = Location::local(absolute("repo/activity.sensor")).expect("absolute path");
        let sensor_context = ApplicabilityContext {
            subject: Subject::Item(local.clone()),
            name: "activity.sensor".into(),
            is_dir: false,
            formats: vec!["sensor.activity".into()],
            context_identity: None,
        };
        let git_context = ApplicabilityContext {
            subject: Subject::Item(local),
            name: "activity.sensor".into(),
            is_dir: false,
            formats: Vec::new(),
            context_identity: Some(ContextIdentity {
                kind: "git".into(),
                location: Location::local(absolute("repo")).expect("absolute path"),
            }),
        };
        assert!(Applicability::Format("sensor.activity".into()).matches(&sensor_context));
        assert!(!Applicability::Format("sensor.activity".into()).matches(&git_context));
        assert!(Applicability::Context("git".into()).matches(&git_context));
        assert!(!Applicability::Context("git".into()).matches(&sensor_context));
    }
}
