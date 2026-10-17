//! Typed, validated results delivered from one supervised contribution request.

use std::io;

use crate::data_providers::{
    Completeness, FieldDefinition, FieldId, FieldKind, ResultState, Value,
};
use crate::preview_scene::Scene;

use super::protocol::{
    ContributionOutput, FieldOutput, FieldOutputState, MAX_RESULT_TEXT_BYTES, OutputState,
    OutputValue, Request, ViewerOutput,
};

/// Prepared output for the requested presentation contribution.
#[derive(Clone, Debug, PartialEq)]
pub enum PreparedResult {
    /// One validated column value.
    Column(ResultState<Value>),
    /// Independently available declared Properties fields.
    Properties(PropertiesResult),
    /// One validated, bounded display scene.
    Viewer(ResultState<Scene>),
}

impl PreparedResult {
    /// Creates a request-kind-specific failure without losing the presentation type.
    pub fn failed(request: &super::protocol::Request, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        match request {
            super::protocol::Request::Column { .. } => Self::Column(ResultState::Failed(reason)),
            super::protocol::Request::Properties { .. } => {
                Self::Properties(PropertiesResult::Failed(reason))
            }
            super::protocol::Request::Viewer { .. } => Self::Viewer(ResultState::Failed(reason)),
        }
    }

    /// Creates a request-kind-specific unavailable result.
    pub fn unavailable(request: &super::protocol::Request, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        match request {
            super::protocol::Request::Column { .. } => {
                Self::Column(ResultState::Unavailable(reason))
            }
            super::protocol::Request::Properties { .. } => {
                Self::Properties(PropertiesResult::Unavailable(reason))
            }
            super::protocol::Request::Viewer { .. } => {
                Self::Viewer(ResultState::Unavailable(reason))
            }
        }
    }
}

/// Complete result for one Properties contribution.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertiesResult {
    /// The exact declared set of fields, normalized to declaration order.
    Ready(Vec<FieldResult>),
    /// The contribution has no values for this source.
    Absent,
    /// Captured facts do not support this contribution.
    Unavailable(String),
    /// The contribution failed as a whole.
    Failed(String),
}

/// One independently prepared Properties field.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldResult {
    /// Stable provider-owned field identity.
    pub field: FieldId,
    /// Typed state and completeness for this field.
    pub state: ResultState<Value>,
}

/// Validates a worker output against the exact request schema.
pub fn prepare(output: ContributionOutput, request: &Request) -> io::Result<PreparedResult> {
    match (request, output) {
        (
            super::protocol::Request::Column { field, field_type },
            ContributionOutput::Column(out),
        ) => {
            let definition = FieldDefinition {
                id: FieldId::new(field.clone())?,
                kind: (*field_type).into(),
            };
            let returned_field = out.field;
            if !matches!(&out.state, OutputState::Ready { .. }) && returned_field.is_some() {
                return Err(invalid("non-ready column result cannot name a field"));
            }
            let result = output_state(out.state, &definition)?;
            if let Some(returned_field) = returned_field.as_deref()
                && returned_field != field.as_str()
            {
                return Err(invalid("column result field does not match its request"));
            }
            if matches!(result, ResultState::Ready { .. }) && returned_field.is_none() {
                return Err(invalid("ready column result omitted its field ID"));
            }
            Ok(PreparedResult::Column(result))
        }
        (
            super::protocol::Request::Properties { fields },
            ContributionOutput::Properties(output),
        ) => {
            let result = match output {
                super::protocol::PropertiesOutput::Ready {
                    fields: output_fields,
                } => properties_fields(fields, output_fields)?,
                super::protocol::PropertiesOutput::Absent => PropertiesResult::Absent,
                super::protocol::PropertiesOutput::Unavailable { reason } => {
                    validate_reason(&reason)?;
                    PropertiesResult::Unavailable(reason)
                }
                super::protocol::PropertiesOutput::Failed { reason } => {
                    validate_reason(&reason)?;
                    PropertiesResult::Failed(reason)
                }
            };
            Ok(PreparedResult::Properties(result))
        }
        (
            Request::Viewer {
                representation,
                version,
            },
            ContributionOutput::Viewer(output),
        ) => {
            if representation != "fyle.scene" || *version != 1 {
                return Err(invalid(
                    "viewer result does not match a supported representation",
                ));
            }
            let result = match output {
                ViewerOutput::Absent => ResultState::Absent,
                ViewerOutput::Unavailable { reason } => {
                    validate_reason(&reason)?;
                    ResultState::Unavailable(reason)
                }
                ViewerOutput::Failed { reason } => {
                    validate_reason(&reason)?;
                    ResultState::Failed(reason)
                }
                ViewerOutput::Ready {
                    scene,
                    completeness,
                } => {
                    if let super::protocol::ViewerCompleteness::Truncated { reason } = &completeness
                    {
                        validate_reason(reason)?;
                    }
                    let scene = crate::preview_scene::validate(scene)?;
                    let completeness = match completeness {
                        super::protocol::ViewerCompleteness::Complete => Completeness::Complete,
                        super::protocol::ViewerCompleteness::Truncated { reason } => {
                            Completeness::Truncated(reason)
                        }
                    };
                    ResultState::Ready {
                        value: scene,
                        completeness,
                    }
                }
            };
            Ok(PreparedResult::Viewer(result))
        }
        _ => Err(invalid("worker returned a different presentation kind")),
    }
}

fn properties_fields(
    declared: &[super::protocol::FieldSchema],
    outputs: Vec<FieldOutput>,
) -> io::Result<PropertiesResult> {
    if declared.len() > super::manifest::MAX_FIELDS_PER_PROVIDER || outputs.len() != declared.len()
    {
        return Err(invalid(
            "Properties output omitted or added declared fields",
        ));
    }
    for (index, field) in declared.iter().enumerate() {
        if declared[..index]
            .iter()
            .any(|earlier| earlier.field == field.field)
        {
            return Err(invalid("Properties request declared a duplicate field ID"));
        }
    }
    let mut text_bytes = 0usize;
    let mut fields = Vec::with_capacity(outputs.len());
    for expected in declared {
        let mut matches = outputs
            .iter()
            .filter(|output| output.field == expected.field);
        let output = matches
            .next()
            .ok_or_else(|| invalid("Properties output omitted a declared field"))?;
        if matches.next().is_some() {
            return Err(invalid("Properties output duplicated a field ID"));
        }
        if output.field.len() > 256 || FieldId::new(output.field.clone()).is_err() {
            return Err(invalid("Properties output has an invalid field ID"));
        }
        account_field_text(&output.state, &mut text_bytes)?;
        let definition = FieldDefinition {
            id: FieldId::new(expected.field.clone())?,
            kind: expected.field_type.into(),
        };
        fields.push(FieldResult {
            field: definition.id.clone(),
            state: field_output_state(output.state.clone(), &definition)?,
        });
    }
    Ok(PropertiesResult::Ready(fields))
}

fn field_output_state(
    output: FieldOutputState,
    definition: &FieldDefinition,
) -> io::Result<ResultState<Value>> {
    match output {
        FieldOutputState::Ready {
            value,
            completeness,
        } => {
            validate_completeness(&completeness)?;
            let value = match value {
                OutputValue::Text { value } if value.len() <= MAX_RESULT_TEXT_BYTES => {
                    Value::Text(value)
                }
                OutputValue::Text { .. } => {
                    return Ok(ResultState::Failed(
                        "field text exceeds its size limit".into(),
                    ));
                }
                OutputValue::Quantity { value, unit } => {
                    match Value::quantity(value, unit.into()) {
                        Ok(value) => value,
                        Err(error) => return Ok(ResultState::Failed(error.to_string())),
                    }
                }
            };
            if let Err(error) = definition.validate(&value) {
                return Ok(ResultState::Failed(error.to_string()));
            }
            Ok(ResultState::Ready {
                value,
                completeness: completeness.into(),
            })
        }
        FieldOutputState::Absent => Ok(ResultState::Absent),
        FieldOutputState::Unavailable { reason } => {
            validate_reason(&reason)?;
            Ok(ResultState::Unavailable(reason))
        }
        FieldOutputState::Failed { reason } => {
            validate_reason(&reason)?;
            Ok(ResultState::Failed(reason))
        }
    }
}

fn output_state(
    output: OutputState,
    definition: &FieldDefinition,
) -> io::Result<ResultState<Value>> {
    match output {
        OutputState::Ready {
            value,
            completeness,
        } => {
            validate_completeness(&completeness)?;
            let value = output_value(value)?;
            if let Err(error) = definition.validate(&value) {
                return Ok(ResultState::Failed(error.to_string()));
            }
            Ok(ResultState::Ready {
                value,
                completeness: completeness.into(),
            })
        }
        OutputState::Absent => Ok(ResultState::Absent),
        OutputState::Unavailable { reason } => {
            validate_reason(&reason)?;
            Ok(ResultState::Unavailable(reason))
        }
        OutputState::Failed { reason } => {
            validate_reason(&reason)?;
            Ok(ResultState::Failed(reason))
        }
    }
}

fn output_value(output: OutputValue) -> io::Result<Value> {
    match output {
        OutputValue::Text { value } if value.len() <= MAX_RESULT_TEXT_BYTES => {
            Ok(Value::Text(value))
        }
        OutputValue::Text { .. } => Err(invalid("worker result text exceeds its size limit")),
        OutputValue::Quantity { value, unit } => Value::quantity(value, unit.into()),
    }
}

impl From<super::protocol::FieldType> for FieldKind {
    fn from(value: super::protocol::FieldType) -> Self {
        match value {
            super::protocol::FieldType::Text => Self::Text,
            super::protocol::FieldType::Quantity(unit) => Self::Quantity(unit.into()),
        }
    }
}

impl From<super::protocol::Unit> for crate::data_providers::Unit {
    fn from(value: super::protocol::Unit) -> Self {
        match value {
            super::protocol::Unit::Seconds => Self::Seconds,
            super::protocol::Unit::Metres => Self::Metres,
        }
    }
}

impl From<super::protocol::Completeness> for Completeness {
    fn from(value: super::protocol::Completeness) -> Self {
        match value {
            super::protocol::Completeness::Complete => Self::Complete,
            super::protocol::Completeness::Truncated { reason } => Self::Truncated(reason),
            super::protocol::Completeness::LowerBound => Self::LowerBound,
        }
    }
}

fn validate_completeness(completeness: &super::protocol::Completeness) -> io::Result<()> {
    if let super::protocol::Completeness::Truncated { reason } = completeness {
        validate_reason(reason)?;
    }
    Ok(())
}

fn validate_reason(reason: &str) -> io::Result<()> {
    if reason.len() > MAX_RESULT_TEXT_BYTES {
        return Err(invalid("worker diagnostic exceeds its size limit"));
    }
    Ok(())
}

fn account_field_text(state: &FieldOutputState, total: &mut usize) -> io::Result<()> {
    let mut add = |text: &str| -> io::Result<()> {
        validate_reason(text)?;
        *total = total.saturating_add(text.len());
        if *total > MAX_RESULT_TEXT_BYTES {
            return Err(invalid(
                "Properties result exceeds its aggregate text limit",
            ));
        }
        Ok(())
    };
    match state {
        FieldOutputState::Ready {
            value,
            completeness,
        } => {
            if let OutputValue::Text { value } = value {
                add(value)?;
            }
            if let super::protocol::Completeness::Truncated { reason } = completeness {
                add(reason)?;
            }
        }
        FieldOutputState::Unavailable { reason } | FieldOutputState::Failed { reason } => {
            add(reason)?;
        }
        FieldOutputState::Absent => {}
    }
    Ok(())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::{PreparedResult, PropertiesResult, prepare};
    use crate::data_providers::{Completeness, ResultState, Unit, Value};
    use crate::extensions::protocol::{
        self, ContributionOutput, FieldOutput, FieldOutputState, FieldSchema, FieldType,
        OutputState, OutputValue, PropertiesOutput, Request, Unit as ProtocolUnit,
        ViewerCompleteness, ViewerOutput,
    };
    use crate::preview_scene::{NodeDto, TextRole};

    fn fields() -> Vec<FieldSchema> {
        vec![
            FieldSchema {
                field: "extension.sensor.reader.label".into(),
                field_type: FieldType::Text,
            },
            FieldSchema {
                field: "extension.sensor.reader.duration".into(),
                field_type: FieldType::Quantity(ProtocolUnit::Seconds),
            },
        ]
    }

    fn ready_fields() -> Vec<FieldOutput> {
        vec![
            FieldOutput {
                field: "extension.sensor.reader.duration".into(),
                state: FieldOutputState::Ready {
                    value: OutputValue::Quantity {
                        value: 2.5,
                        unit: ProtocolUnit::Seconds,
                    },
                    completeness: protocol::Completeness::LowerBound,
                },
            },
            FieldOutput {
                field: "extension.sensor.reader.label".into(),
                state: FieldOutputState::Ready {
                    value: OutputValue::Text {
                        value: "sensor".into(),
                    },
                    completeness: protocol::Completeness::Truncated {
                        reason: "sample limit".into(),
                    },
                },
            },
        ]
    }

    #[test]
    fn properties_fields_normalize_order_and_preserve_independent_completeness() {
        let output = ContributionOutput::Properties(PropertiesOutput::Ready {
            fields: ready_fields(),
        });
        let PreparedResult::Properties(PropertiesResult::Ready(fields)) =
            prepare(output, &Request::Properties { fields: fields() })
                .expect("valid Properties result is prepared")
        else {
            panic!("expected ready Properties fields");
        };
        assert_eq!(fields[0].field.as_str(), "extension.sensor.reader.label");
        assert!(matches!(
            fields[0].state,
            ResultState::Ready {
                value: Value::Text(ref value),
                completeness: Completeness::Truncated(ref reason),
            } if value == "sensor" && reason == "sample limit"
        ));
        assert_eq!(fields[1].field.as_str(), "extension.sensor.reader.duration");
        assert!(matches!(
            fields[1].state,
            ResultState::Ready {
                value: Value::Quantity(ref value),
                completeness: Completeness::LowerBound,
            } if value.value() == 2.5 && value.unit() == Unit::Seconds
        ));
    }

    #[test]
    fn properties_reject_missing_duplicate_and_foreign_fields() {
        let request = Request::Properties { fields: fields() };
        let cases = [
            vec![ready_fields()[0].clone()],
            vec![ready_fields()[0].clone(), ready_fields()[0].clone()],
            {
                let mut output = ready_fields();
                output[0].field = "extension.sensor.other.foreign".into();
                output
            },
        ];
        for outputs in cases {
            assert!(
                prepare(
                    ContributionOutput::Properties(PropertiesOutput::Ready { fields: outputs }),
                    &request,
                )
                .is_err()
            );
        }
    }

    #[test]
    fn bad_property_value_is_failed_independently_and_global_reason_bound_is_enforced() {
        let expected_fields = fields();
        let bad_values = [
            OutputValue::Quantity {
                value: -1.0,
                unit: ProtocolUnit::Seconds,
            },
            OutputValue::Quantity {
                value: 1.0,
                unit: ProtocolUnit::Metres,
            },
            OutputValue::Text {
                value: "wrong kind".into(),
            },
        ];
        for bad_value in bad_values {
            let mut outputs = ready_fields();
            outputs[0].state = FieldOutputState::Ready {
                value: bad_value,
                completeness: protocol::Completeness::Complete,
            };
            let PreparedResult::Properties(PropertiesResult::Ready(fields)) = prepare(
                ContributionOutput::Properties(PropertiesOutput::Ready { fields: outputs }),
                &Request::Properties {
                    fields: expected_fields.clone(),
                },
            )
            .expect("one bad field does not discard its sibling") else {
                panic!("expected ready Properties section");
            };
            assert!(matches!(fields[1].state, ResultState::Failed(_)));
            assert!(matches!(fields[0].state, ResultState::Ready { .. }));
        }

        assert!(
            prepare(
                ContributionOutput::Properties(PropertiesOutput::Failed {
                    reason: "x".repeat(protocol::MAX_RESULT_TEXT_BYTES + 1),
                }),
                &Request::Properties {
                    fields: expected_fields,
                },
            )
            .is_err()
        );

        let text_fields = vec![
            FieldSchema {
                field: "extension.sensor.reader.first".into(),
                field_type: FieldType::Text,
            },
            FieldSchema {
                field: "extension.sensor.reader.second".into(),
                field_type: FieldType::Text,
            },
        ];
        let oversized_fields = [40 * 1024, 30 * 1024]
            .into_iter()
            .enumerate()
            .map(|(index, length)| FieldOutput {
                field: text_fields[index].field.clone(),
                state: FieldOutputState::Ready {
                    value: OutputValue::Text {
                        value: "x".repeat(length),
                    },
                    completeness: protocol::Completeness::Complete,
                },
            })
            .collect();
        assert!(
            prepare(
                ContributionOutput::Properties(PropertiesOutput::Ready {
                    fields: oversized_fields,
                }),
                &Request::Properties {
                    fields: text_fields
                },
            )
            .is_err()
        );
    }

    #[test]
    fn scalar_ready_requires_its_exact_field_and_presentation_kind() {
        let request = Request::Column {
            field: "extension.sensor.reader.duration".into(),
            field_type: FieldType::Quantity(ProtocolUnit::Seconds),
        };
        let output = ContributionOutput::Column(protocol::ContributionResult {
            field: Some("extension.sensor.reader.other".into()),
            state: OutputState::Ready {
                value: OutputValue::Quantity {
                    value: 4.0,
                    unit: ProtocolUnit::Seconds,
                },
                completeness: protocol::Completeness::Complete,
            },
        });
        assert!(prepare(output, &request).is_err());
        assert!(
            prepare(
                ContributionOutput::Properties(PropertiesOutput::Absent),
                &request,
            )
            .is_err()
        );
    }

    #[test]
    fn viewer_host_revalidates_untrusted_scene_dto() {
        let scene = crate::preview_scene::SceneDto {
            version: 2,
            root: NodeDto::Text {
                id: "summary".into(),
                text: "ready".into(),
                role: TextRole::Body,
            },
        };
        assert!(
            prepare(
                ContributionOutput::Viewer(ViewerOutput::Ready {
                    scene,
                    completeness: ViewerCompleteness::Complete,
                }),
                &Request::Viewer {
                    representation: "fyle.scene".into(),
                    version: 1,
                },
            )
            .is_err()
        );
    }
}
