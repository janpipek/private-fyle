//! Strict, budgeted conversion of extension Lua results to protocol values.

use std::io;

use mlua::{Table, Value};

use crate::preview_scene::{
    Axis, CellDto, ColumnDto, ColumnKindDto, Gap, NodeDto, PathDto, PathRole, PointDto, SceneDto,
    SceneUnit, StrokeWeight, TextRole,
};

use super::super::protocol::{
    self, Completeness, ContributionOutput, ContributionResult, FieldOutput, FieldOutputState,
    FieldType, OutputState, OutputValue, PropertiesOutput, Request, Unit, ViewerCompleteness,
    ViewerOutput,
};
use super::{Init, lua_error};

const MAX_LUA_VISITS: usize = 65_536;

#[derive(Default)]
struct Budget {
    visits: usize,
    text_bytes: usize,
    nodes: usize,
    rows: usize,
    cells: usize,
    canvases: usize,
    paths: usize,
    points: usize,
}

/// Converts a selected callback's strict return table to its declared output kind.
pub fn convert_result(value: &Value, init: &Init) -> io::Result<ContributionOutput> {
    match &init.request {
        Request::Column { field, field_type } => {
            convert_column(value, field, *field_type).map(ContributionOutput::Column)
        }
        Request::Properties { fields } => {
            convert_properties(value, fields).map(ContributionOutput::Properties)
        }
        Request::Viewer {
            representation,
            version,
        } => convert_viewer(value, representation, *version).map(ContributionOutput::Viewer),
    }
}

fn convert_column(
    value: &Value,
    field: &str,
    field_type: FieldType,
) -> io::Result<ContributionResult> {
    let table = expect_table(value, "read callback must return a result table")?;
    let mut budget = Budget::default();
    check_map(
        &table,
        &["state", "field", "value", "completeness", "reason"],
        &mut budget,
    )?;
    let state = string_field(&table, "state", 32, &mut budget)?;
    let result = match state.as_str() {
        "absent" => {
            check_map(&table, &["state"], &mut budget)?;
            ContributionResult {
                field: None,
                state: OutputState::Absent,
            }
        }
        "unavailable" | "failed" => {
            check_map(&table, &["state", "reason"], &mut budget)?;
            let reason = string_value(
                table.raw_get("reason").map_err(lua_error)?,
                protocol::MAX_RESULT_TEXT_BYTES,
            )?;
            let state = if state == "unavailable" {
                OutputState::Unavailable { reason }
            } else {
                OutputState::Failed { reason }
            };
            ContributionResult { field: None, state }
        }
        "ready" => {
            check_map(
                &table,
                &["state", "field", "value", "completeness"],
                &mut budget,
            )?;
            let actual_field = string_field(&table, "field", 256, &mut budget)?;
            if actual_field != field {
                return Err(invalid("result field does not match selected column"));
            }
            let output_value = value_table(&table, field_type, &mut budget, false)?;
            let completeness = completeness(&table, &mut budget, false)?;
            ContributionResult {
                field: Some(actual_field),
                state: OutputState::Ready {
                    value: output_value,
                    completeness,
                },
            }
        }
        _ => return Err(invalid("unknown contribution result state")),
    };
    Ok(result)
}

fn convert_properties(
    value: &Value,
    declared: &[protocol::FieldSchema],
) -> io::Result<PropertiesOutput> {
    let table = expect_table(value, "Properties callback must return a result table")?;
    let mut budget = Budget::default();
    check_map(&table, &["state", "fields", "reason"], &mut budget)?;
    let state = string_field(&table, "state", 32, &mut budget)?;
    match state.as_str() {
        "absent" => {
            check_map(&table, &["state"], &mut budget)?;
            Ok(PropertiesOutput::Absent)
        }
        "unavailable" | "failed" => {
            check_map(&table, &["state", "reason"], &mut budget)?;
            let reason = display_string(
                table.raw_get("reason").map_err(lua_error)?,
                protocol::MAX_RESULT_TEXT_BYTES,
                &mut budget,
            )?;
            if state == "unavailable" {
                Ok(PropertiesOutput::Unavailable { reason })
            } else {
                Ok(PropertiesOutput::Failed { reason })
            }
        }
        "ready" => {
            check_map(&table, &["state", "fields"], &mut budget)?;
            if declared.len() > crate::extensions::manifest::MAX_FIELDS_PER_PROVIDER {
                return Err(invalid("Properties field declaration exceeds its limit"));
            }
            let fields_table = expect_table(
                &table.raw_get::<Value>("fields").map_err(lua_error)?,
                "Properties fields must be an array",
            )?;
            let values = dense_array(
                &fields_table,
                crate::extensions::manifest::MAX_FIELDS_PER_PROVIDER,
                &mut budget,
            )?;
            if values.len() != declared.len() {
                return Err(invalid(
                    "Properties output must include every declared field exactly once",
                ));
            }
            let mut parsed = Vec::with_capacity(values.len());
            for value in values {
                let output = convert_field(&value, declared, &mut budget)?;
                parsed.push(output);
            }
            for expected in declared {
                if parsed
                    .iter()
                    .filter(|field| field.field == expected.field)
                    .count()
                    != 1
                {
                    return Err(invalid(
                        "Properties output has a missing or duplicate field ID",
                    ));
                }
            }
            parsed.sort_by_key(|field| {
                declared
                    .iter()
                    .position(|expected| expected.field == field.field)
                    .unwrap_or(usize::MAX)
            });
            Ok(PropertiesOutput::Ready { fields: parsed })
        }
        _ => Err(invalid("unknown Properties result state")),
    }
}

fn convert_field(
    value: &Value,
    declared: &[protocol::FieldSchema],
    budget: &mut Budget,
) -> io::Result<FieldOutput> {
    let table = expect_table(value, "Properties field must be a table")?;
    check_map(
        &table,
        &["field", "state", "value", "completeness", "reason"],
        budget,
    )?;
    let field = string_field(&table, "field", 256, budget)?;
    let schema = declared
        .iter()
        .find(|item| item.field == field)
        .ok_or_else(|| invalid("Properties output contains an undeclared field"))?;
    let state_name = string_field(&table, "state", 32, budget)?;
    let state = match state_name.as_str() {
        "absent" => {
            check_map(&table, &["field", "state"], budget)?;
            FieldOutputState::Absent
        }
        "unavailable" | "failed" => {
            check_map(&table, &["field", "state", "reason"], budget)?;
            let reason = display_string(
                table.raw_get("reason").map_err(lua_error)?,
                protocol::MAX_RESULT_TEXT_BYTES,
                budget,
            )?;
            if state_name == "unavailable" {
                FieldOutputState::Unavailable { reason }
            } else {
                FieldOutputState::Failed { reason }
            }
        }
        "ready" => {
            check_map(&table, &["field", "state", "value", "completeness"], budget)?;
            match value_table(&table, schema.field_type, budget, true).and_then(|output| {
                completeness(&table, budget, true).map(|completeness| (output, completeness))
            }) {
                Ok((output, completeness)) => FieldOutputState::Ready {
                    value: output,
                    completeness,
                },
                Err(error) if budget.text_bytes > crate::preview_scene::MAX_TEXT_BYTES => {
                    return Err(error);
                }
                Err(error) => FieldOutputState::Failed {
                    reason: super::bounded_error(&error),
                },
            }
        }
        _ => return Err(invalid("unknown Properties field state")),
    };
    Ok(FieldOutput { field, state })
}

fn convert_viewer(value: &Value, representation: &str, version: u32) -> io::Result<ViewerOutput> {
    if representation != "fyle.scene" || version != 1 {
        return Err(invalid("unsupported viewer representation"));
    }
    let table = expect_table(value, "viewer callback must return a result table")?;
    let mut budget = Budget::default();
    check_map(
        &table,
        &["state", "scene", "completeness", "reason"],
        &mut budget,
    )?;
    let state = string_field(&table, "state", 32, &mut budget)?;
    match state.as_str() {
        "absent" => {
            check_map(&table, &["state"], &mut budget)?;
            Ok(ViewerOutput::Absent)
        }
        "unavailable" | "failed" => {
            check_map(&table, &["state", "reason"], &mut budget)?;
            let reason = display_string(
                table.raw_get("reason").map_err(lua_error)?,
                protocol::MAX_RESULT_TEXT_BYTES,
                &mut budget,
            )?;
            if state == "unavailable" {
                Ok(ViewerOutput::Unavailable { reason })
            } else {
                Ok(ViewerOutput::Failed { reason })
            }
        }
        "ready" => {
            check_map(&table, &["state", "scene", "completeness"], &mut budget)?;
            let scene = scene_dto(
                &table.raw_get::<Value>("scene").map_err(lua_error)?,
                &mut budget,
            )?;
            let completeness_table = expect_table(
                &table.raw_get::<Value>("completeness").map_err(lua_error)?,
                "viewer completeness must be a table",
            )?;
            check_map(&completeness_table, &["kind", "reason"], &mut budget)?;
            let kind = string_field(&completeness_table, "kind", 32, &mut budget)?;
            let completeness = match kind.as_str() {
                "complete" => {
                    check_map(&completeness_table, &["kind"], &mut budget)?;
                    ViewerCompleteness::Complete
                }
                "truncated" => {
                    check_map(&completeness_table, &["kind", "reason"], &mut budget)?;
                    ViewerCompleteness::Truncated {
                        reason: display_string(
                            completeness_table.raw_get("reason").map_err(lua_error)?,
                            protocol::MAX_RESULT_TEXT_BYTES,
                            &mut budget,
                        )?,
                    }
                }
                _ => return Err(invalid("viewer completeness must be complete or truncated")),
            };
            crate::preview_scene::validate(scene.clone())?;
            Ok(ViewerOutput::Ready {
                scene,
                completeness,
            })
        }
        _ => Err(invalid("unknown viewer result state")),
    }
}

fn scene_dto(value: &Value, budget: &mut Budget) -> io::Result<SceneDto> {
    let table = expect_table(value, "scene must be a table")?;
    check_map(&table, &["version", "root"], budget)?;
    let version = integer_field(&table, "version")?;
    let root = node_dto(
        &table.raw_get::<Value>("root").map_err(lua_error)?,
        1,
        budget,
    )?;
    Ok(SceneDto { version, root })
}

fn node_dto(value: &Value, depth: usize, budget: &mut Budget) -> io::Result<NodeDto> {
    if depth > crate::preview_scene::MAX_DEPTH {
        return Err(invalid("scene exceeds its depth limit"));
    }
    budget.nodes = budget.nodes.saturating_add(1);
    if budget.nodes > crate::preview_scene::MAX_NODES {
        return Err(invalid("scene exceeds its node limit"));
    }
    let table = expect_table(value, "scene node must be a table")?;
    let kind = string_field(&table, "kind", 16, budget)?;
    match kind.as_str() {
        "stack" => {
            check_map(&table, &["kind", "id", "axis", "gap", "children"], budget)?;
            let id = string_field(&table, "id", crate::preview_scene::MAX_ID_BYTES, budget)?;
            let axis = match string_field(&table, "axis", 16, budget)?.as_str() {
                "row" => Axis::Row,
                "column" => Axis::Column,
                _ => return Err(invalid("unknown scene stack axis")),
            };
            let gap = match string_field(&table, "gap", 16, budget)?.as_str() {
                "small" => Gap::Small,
                "normal" => Gap::Normal,
                _ => return Err(invalid("unknown scene stack gap")),
            };
            let children =
                array_field(&table, "children", crate::preview_scene::MAX_NODES, budget)?
                    .iter()
                    .map(|child| node_dto(child, depth + 1, budget))
                    .collect::<io::Result<Vec<_>>>()?;
            Ok(NodeDto::Stack {
                id,
                axis,
                gap,
                children,
            })
        }
        "text" => {
            check_map(&table, &["kind", "id", "text", "role"], budget)?;
            let id = string_field(&table, "id", crate::preview_scene::MAX_ID_BYTES, budget)?;
            let text = display_string(
                table.raw_get::<Value>("text").map_err(lua_error)?,
                crate::preview_scene::MAX_STRING_BYTES,
                budget,
            )?;
            let role = match string_field(&table, "role", 16, budget)?.as_str() {
                "body" => TextRole::Body,
                "heading" => TextRole::Heading,
                "muted" => TextRole::Muted,
                _ => return Err(invalid("unknown scene text role")),
            };
            Ok(NodeDto::Text { id, text, role })
        }
        "table" => {
            check_map(&table, &["kind", "id", "columns", "rows"], budget)?;
            let id = string_field(&table, "id", crate::preview_scene::MAX_ID_BYTES, budget)?;
            let columns =
                array_field(&table, "columns", crate::preview_scene::MAX_COLUMNS, budget)?
                    .iter()
                    .map(|column| column_dto(column, budget))
                    .collect::<io::Result<Vec<_>>>()?;
            budget.rows = budget
                .rows
                .saturating_add(table.raw_get::<Table>("rows").map_err(lua_error)?.raw_len());
            if budget.rows > crate::preview_scene::MAX_ROWS {
                return Err(invalid("scene exceeds its table row limit"));
            }
            let row_values = array_field(&table, "rows", crate::preview_scene::MAX_ROWS, budget)?;
            let mut rows = Vec::with_capacity(row_values.len());
            for row in row_values {
                let cells = array_values(&row, crate::preview_scene::MAX_COLUMNS, budget)?;
                if cells.len() != columns.len() {
                    return Err(invalid("scene table row has the wrong number of cells"));
                }
                budget.cells = budget.cells.saturating_add(cells.len());
                if budget.cells > crate::preview_scene::MAX_CELLS {
                    return Err(invalid("scene exceeds its table cell limit"));
                }
                rows.push(
                    cells
                        .iter()
                        .map(|cell| cell_dto(cell, budget))
                        .collect::<io::Result<Vec<_>>>()?,
                );
            }
            Ok(NodeDto::Table { id, columns, rows })
        }
        "canvas" => {
            check_map(&table, &["kind", "id", "width", "height", "paths"], budget)?;
            let id = string_field(&table, "id", crate::preview_scene::MAX_ID_BYTES, budget)?;
            budget.canvases = budget.canvases.saturating_add(1);
            if budget.canvases > crate::preview_scene::MAX_CANVASES {
                return Err(invalid("scene exceeds its canvas limit"));
            }
            let width = number_field(&table, "width")?;
            let height = number_field(&table, "height")?;
            let paths = array_field(&table, "paths", crate::preview_scene::MAX_PATHS, budget)?
                .iter()
                .map(|path| path_dto(path, budget))
                .collect::<io::Result<Vec<_>>>()?;
            Ok(NodeDto::Canvas {
                id,
                width,
                height,
                paths,
            })
        }
        _ => Err(invalid("unknown scene node kind")),
    }
}

fn column_dto(value: &Value, budget: &mut Budget) -> io::Result<ColumnDto> {
    let table = expect_table(value, "scene column must be a table")?;
    check_map(&table, &["id", "label", "kind"], budget)?;
    let id = string_field(&table, "id", crate::preview_scene::MAX_ID_BYTES, budget)?;
    let label = display_string(
        table.raw_get::<Value>("label").map_err(lua_error)?,
        crate::preview_scene::MAX_STRING_BYTES,
        budget,
    )?;
    let kind_table = expect_table(
        &table.raw_get::<Value>("kind").map_err(lua_error)?,
        "column kind must be a table",
    )?;
    let kind = string_field(&kind_table, "kind", 16, budget)?;
    let kind = match kind.as_str() {
        "text" => {
            check_map(&kind_table, &["kind"], budget)?;
            ColumnKindDto::Text
        }
        "quantity" => {
            check_map(&kind_table, &["kind", "unit"], budget)?;
            ColumnKindDto::Quantity {
                unit: scene_unit(&string_field(&kind_table, "unit", 16, budget)?)?,
            }
        }
        _ => return Err(invalid("unknown scene table column kind")),
    };
    Ok(ColumnDto { id, label, kind })
}

fn cell_dto(value: &Value, budget: &mut Budget) -> io::Result<Option<CellDto>> {
    if matches!(value, Value::Boolean(false)) {
        return Ok(None);
    }
    let table = expect_table(value, "scene cell must be a typed table or false")?;
    check_map(&table, &["kind", "value", "unit"], budget)?;
    match string_field(&table, "kind", 16, budget)?.as_str() {
        "text" => {
            check_map(&table, &["kind", "value"], budget)?;
            Ok(Some(CellDto::Text {
                value: display_string(
                    table.raw_get::<Value>("value").map_err(lua_error)?,
                    crate::preview_scene::MAX_STRING_BYTES,
                    budget,
                )?,
            }))
        }
        "quantity" => {
            check_map(&table, &["kind", "value", "unit"], budget)?;
            Ok(Some(CellDto::Quantity {
                value: number_field(&table, "value")?,
                unit: scene_unit(&string_field(&table, "unit", 16, budget)?)?,
            }))
        }
        _ => Err(invalid("unknown scene table cell kind")),
    }
}

fn path_dto(value: &Value, budget: &mut Budget) -> io::Result<PathDto> {
    let table = expect_table(value, "scene path must be a table")?;
    check_map(&table, &["role", "weight", "points"], budget)?;
    budget.paths = budget.paths.saturating_add(1);
    if budget.paths > crate::preview_scene::MAX_PATHS {
        return Err(invalid("scene exceeds its path limit"));
    }
    let role = match string_field(&table, "role", 16, budget)?.as_str() {
        "foreground" => PathRole::Foreground,
        "muted" => PathRole::Muted,
        "accent" => PathRole::Accent,
        "success" => PathRole::Success,
        "warning" => PathRole::Warning,
        _ => return Err(invalid("unknown scene path role")),
    };
    let weight = match string_field(&table, "weight", 16, budget)?.as_str() {
        "thin" => StrokeWeight::Thin,
        "normal" => StrokeWeight::Normal,
        "strong" => StrokeWeight::Strong,
        _ => return Err(invalid("unknown scene path weight")),
    };
    let points_table = table.raw_get::<Table>("points").map_err(lua_error)?;
    let points = dense_array(&points_table, crate::preview_scene::MAX_POINTS, budget)?;
    budget.points = budget.points.saturating_add(points.len());
    if budget.points > crate::preview_scene::MAX_POINTS {
        return Err(invalid("scene exceeds its point limit"));
    }
    let points = points
        .iter()
        .map(|point| {
            let point = expect_table(point, "scene coordinate must be a table")?;
            check_map(&point, &["x", "y"], budget)?;
            Ok(PointDto {
                x: number_field(&point, "x")?,
                y: number_field(&point, "y")?,
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(PathDto {
        role,
        weight,
        points,
    })
}

fn scene_unit(value: &str) -> io::Result<SceneUnit> {
    match value {
        "seconds" => Ok(SceneUnit::Seconds),
        "metres" => Ok(SceneUnit::Metres),
        _ => Err(invalid("unknown scene quantity unit")),
    }
}

fn value_table(
    table: &Table,
    field_type: FieldType,
    budget: &mut Budget,
    aggregate: bool,
) -> io::Result<OutputValue> {
    let value = expect_table(
        &table.raw_get::<Value>("value").map_err(lua_error)?,
        "typed value must be a table",
    )?;
    check_map(&value, &["kind", "value", "unit"], budget)?;
    let kind = string_field(&value, "kind", 16, budget)?;
    match (field_type, kind.as_str()) {
        (FieldType::Text, "text") => {
            check_map(&value, &["kind", "value"], budget)?;
            Ok(OutputValue::Text {
                value: if aggregate {
                    display_string(
                        value.raw_get::<Value>("value").map_err(lua_error)?,
                        protocol::MAX_RESULT_TEXT_BYTES,
                        budget,
                    )?
                } else {
                    string_field(&value, "value", protocol::MAX_RESULT_TEXT_BYTES, budget)?
                },
            })
        }
        (FieldType::Quantity(expected), "quantity") => {
            check_map(&value, &["kind", "value", "unit"], budget)?;
            let number = number_field(&value, "value")?;
            let unit = match string_field(&value, "unit", 16, budget)?.as_str() {
                "seconds" => Unit::Seconds,
                "metres" => Unit::Metres,
                _ => return Err(invalid("unknown quantity unit")),
            };
            if unit != expected || !number.is_finite() || number < 0.0 {
                return Err(invalid("quantity does not match declared field"));
            }
            Ok(OutputValue::Quantity {
                value: number,
                unit,
            })
        }
        _ => Err(invalid("value does not match declared field type")),
    }
}

fn completeness(table: &Table, budget: &mut Budget, aggregate: bool) -> io::Result<Completeness> {
    let table = expect_table(
        &table.raw_get::<Value>("completeness").map_err(lua_error)?,
        "completeness must be a table",
    )?;
    check_map(&table, &["kind", "reason"], budget)?;
    match string_field(&table, "kind", 32, budget)?.as_str() {
        "complete" => {
            check_map(&table, &["kind"], budget)?;
            Ok(Completeness::Complete)
        }
        "lower_bound" => {
            check_map(&table, &["kind"], budget)?;
            Ok(Completeness::LowerBound)
        }
        "truncated" => {
            check_map(&table, &["kind", "reason"], budget)?;
            Ok(Completeness::Truncated {
                reason: if aggregate {
                    display_string(
                        table.raw_get::<Value>("reason").map_err(lua_error)?,
                        protocol::MAX_RESULT_TEXT_BYTES,
                        budget,
                    )?
                } else {
                    string_field(&table, "reason", protocol::MAX_RESULT_TEXT_BYTES, budget)?
                },
            })
        }
        _ => Err(invalid("unknown completeness state")),
    }
}

fn array_field(
    table: &Table,
    key: &str,
    max: usize,
    budget: &mut Budget,
) -> io::Result<Vec<Value>> {
    let value = table.raw_get::<Value>(key).map_err(lua_error)?;
    let table = expect_table(&value, "array value must be a table")?;
    dense_array(&table, max, budget)
}

fn array_values(value: &Value, max: usize, budget: &mut Budget) -> io::Result<Vec<Value>> {
    let table = expect_table(value, "array value must be a table")?;
    dense_array(&table, max, budget)
}

fn dense_array(table: &Table, max: usize, budget: &mut Budget) -> io::Result<Vec<Value>> {
    if table.metatable().is_some() {
        return Err(invalid("Lua arrays cannot have metatables"));
    }
    let length = table.raw_len();
    if length > max {
        return Err(invalid("Lua array exceeds its declared limit"));
    }
    let mut values = Vec::with_capacity(length);
    let mut count = 0usize;
    for pair in table.clone().pairs::<Value, Value>() {
        let (key, value) = pair.map_err(lua_error)?;
        visit(budget)?;
        let Value::Integer(index) = key else {
            return Err(invalid("Lua array keys must be positive integers"));
        };
        if index < 1 || index as usize > length {
            return Err(invalid("Lua arrays cannot be sparse or have extra keys"));
        }
        count = count.saturating_add(1);
        values.push((index as usize, value));
    }
    if count != length {
        return Err(invalid("Lua arrays cannot be sparse"));
    }
    values.sort_by_key(|(index, _)| *index);
    Ok(values.into_iter().map(|(_, value)| value).collect())
}

fn check_map(table: &Table, allowed: &[&str], budget: &mut Budget) -> io::Result<()> {
    if table.metatable().is_some() {
        return Err(invalid("Lua result tables cannot have metatables"));
    }
    for pair in table.clone().pairs::<Value, Value>() {
        let (key, _) = pair.map_err(lua_error)?;
        visit(budget)?;
        let Value::String(key) = key else {
            return Err(invalid("Lua result keys must be strings"));
        };
        let key = key.to_str().map_err(lua_error)?;
        if !allowed.iter().any(|allowed| key.as_ref() == *allowed) {
            return Err(invalid("Lua result contains an unknown field"));
        }
    }
    Ok(())
}

fn visit(budget: &mut Budget) -> io::Result<()> {
    budget.visits = budget.visits.saturating_add(1);
    if budget.visits > MAX_LUA_VISITS {
        return Err(invalid("Lua result exceeds traversal limit"));
    }
    Ok(())
}

fn expect_table(value: &Value, message: &str) -> io::Result<Table> {
    match value {
        Value::Table(table) => Ok(table.clone()),
        _ => Err(invalid(message)),
    }
}

fn string_field(table: &Table, key: &str, max: usize, budget: &mut Budget) -> io::Result<String> {
    let _ = budget;
    string_value(table.raw_get::<Value>(key).map_err(lua_error)?, max)
}

fn string_value(value: Value, max: usize) -> io::Result<String> {
    string_value_unmetered(value, max)
}

fn string_value_unmetered(value: Value, max: usize) -> io::Result<String> {
    let Value::String(value) = value else {
        return Err(invalid("result text must be a string"));
    };
    if value.as_bytes().len() > max {
        return Err(invalid("result text exceeds its size limit"));
    }
    let value = value.to_str().map_err(lua_error)?.to_string();
    Ok(value)
}

fn display_string(value: Value, max: usize, budget: &mut Budget) -> io::Result<String> {
    let value = string_value_unmetered(value, max)?;
    budget.text_bytes = budget.text_bytes.saturating_add(value.len());
    if budget.text_bytes > crate::preview_scene::MAX_TEXT_BYTES {
        return Err(invalid("result exceeds aggregate text limit"));
    }
    Ok(value)
}

fn integer_field(table: &Table, key: &str) -> io::Result<u32> {
    match table.raw_get::<Value>(key).map_err(lua_error)? {
        Value::Integer(value) => {
            u32::try_from(value).map_err(|_| invalid("integer is out of range"))
        }
        _ => Err(invalid("field must be an integer")),
    }
}

fn number_field(table: &Table, key: &str) -> io::Result<f64> {
    match table.raw_get::<Value>(key).map_err(lua_error)? {
        Value::Integer(value) => Ok(value as f64),
        Value::Number(value) => Ok(value),
        _ => Err(invalid("field must be a number")),
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
