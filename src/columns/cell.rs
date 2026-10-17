//! Converts typed provider cell states into compact table text and hover diagnostics.

use crate::data_providers::{Completeness, ResultState, Unit, Value};

/// Prepared display text and optional explanation for one provider cell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CellDisplay {
    /// Text shown in the table; empty for states without a value.
    pub text: String,
    /// Hover explanation for partial, unavailable or failed data.
    pub tooltip: Option<String>,
}

/// Projects a typed provider state for display without discarding its cached state.
pub fn project(state: &ResultState<Value>) -> CellDisplay {
    match state {
        ResultState::Ready {
            value,
            completeness: Completeness::Complete,
        } => CellDisplay {
            text: value_text(value),
            tooltip: None,
        },
        ResultState::Ready {
            value,
            completeness: Completeness::Truncated(reason),
        } => CellDisplay {
            text: format!("… {}", value_text(value)),
            tooltip: Some(format!("Partial: {reason}")),
        },
        ResultState::Ready {
            value,
            completeness: Completeness::LowerBound,
        } => CellDisplay {
            text: format!("≥ {}", lower_bound_text(value)),
            tooltip: Some("Lower bound".into()),
        },
        ResultState::Unavailable(reason) | ResultState::Failed(reason) => CellDisplay {
            text: String::new(),
            tooltip: Some(reason.clone()),
        },
        ResultState::Pending | ResultState::Absent => CellDisplay::default(),
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::Text(text) => text.clone(),
        Value::Quantity(quantity) => match quantity.unit() {
            Unit::Seconds => crate::formats::media::format_duration(quantity.value())
                .unwrap_or_else(|| format!("{} s", quantity.value())),
            Unit::Metres => format!("{:.1} m", quantity.value()),
        },
    }
}

fn lower_bound_text(value: &Value) -> String {
    match value {
        Value::Text(text) => text.clone(),
        Value::Quantity(quantity) => match quantity.unit() {
            Unit::Seconds => format!("{} s", quantity.value()),
            Unit::Metres => format!("{} m", quantity.value()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{CellDisplay, project};
    use crate::data_providers::{Completeness, ResultState, Unit, Value};

    #[test]
    fn preserves_complete_format_and_marks_partial_values() {
        let value = Value::quantity(12.34, Unit::Metres).expect("valid fixture quantity");
        assert_eq!(
            project(&ResultState::Ready {
                value: value.clone(),
                completeness: Completeness::Complete,
            }),
            CellDisplay {
                text: "12.3 m".into(),
                tooltip: None,
            }
        );
        assert_eq!(
            project(&ResultState::Ready {
                value: value.clone(),
                completeness: Completeness::Truncated("sample cap".into()),
            }),
            CellDisplay {
                text: "… 12.3 m".into(),
                tooltip: Some("Partial: sample cap".into()),
            }
        );
        assert_eq!(
            project(&ResultState::Ready {
                value,
                completeness: Completeness::LowerBound,
            }),
            CellDisplay {
                text: "≥ 12.34 m".into(),
                tooltip: Some("Lower bound".into()),
            }
        );
    }

    #[test]
    fn missing_values_stay_blank_and_errors_are_explainable() {
        assert_eq!(project(&ResultState::Absent), CellDisplay::default());
        assert_eq!(project(&ResultState::Pending), CellDisplay::default());
        assert_eq!(
            project(&ResultState::Failed("reader failed".into())),
            CellDisplay {
                text: String::new(),
                tooltip: Some("reader failed".into()),
            }
        );
    }

    #[test]
    fn lower_bounds_do_not_round_up_and_large_durations_remain_visible() {
        let distance = Value::quantity(12.36, Unit::Metres).expect("valid distance");
        assert_eq!(
            project(&ResultState::Ready {
                value: distance,
                completeness: Completeness::LowerBound,
            })
            .text,
            "≥ 12.36 m"
        );
        let duration = Value::quantity(f64::MAX, Unit::Seconds).expect("valid duration");
        assert!(
            project(&ResultState::Ready {
                value: duration,
                completeness: Completeness::Complete,
            })
            .text
            .ends_with(" s")
        );
    }
}
