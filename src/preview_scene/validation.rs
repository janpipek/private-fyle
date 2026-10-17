//! Validation and conversion from untrusted scene DTOs to the display model.

use std::collections::HashSet;
use std::io;

use crate::data_providers::{Quantity, Unit, Value};

use super::{
    CellDto, Column, ColumnKind, ColumnKindDto, MAX_CANVAS_DIMENSION, MAX_CANVASES, MAX_CELLS,
    MAX_COLUMNS, MAX_DEPTH, MAX_ID_BYTES, MAX_NODES, MAX_PATHS, MAX_POINTS, MAX_ROWS,
    MAX_STRING_BYTES, MAX_TEXT_BYTES, Node, NodeDto, NodeKind, Path, Point, Scene, SceneDto,
};

#[derive(Default)]
struct Limits {
    nodes: usize,
    text_bytes: usize,
    rows: usize,
    cells: usize,
    canvases: usize,
    paths: usize,
    points: usize,
    node_ids: HashSet<String>,
}

/// Validates untrusted scene data and converts table cells to provider values.
pub fn validate(dto: SceneDto) -> io::Result<Scene> {
    if dto.version != 1 {
        return Err(invalid("unsupported preview scene version"));
    }
    let mut limits = Limits::default();
    let root = validate_node(dto.root, 1, &mut limits)?;
    Ok(Scene { root })
}

fn validate_node(dto: NodeDto, depth: usize, limits: &mut Limits) -> io::Result<Node> {
    if depth > MAX_DEPTH {
        return Err(invalid("preview scene exceeds the depth limit"));
    }
    add_limit(&mut limits.nodes, 1, MAX_NODES, "node count")?;
    let (id, kind) = match dto {
        NodeDto::Stack {
            id,
            axis,
            gap,
            children,
        } => {
            validate_id(&id)?;
            if !limits.node_ids.insert(id.clone()) {
                return Err(invalid("preview scene node IDs must be unique"));
            }
            let children = children
                .into_iter()
                .map(|child| validate_node(child, depth + 1, limits))
                .collect::<io::Result<Vec<_>>>()?;
            (
                id,
                NodeKind::Stack {
                    axis,
                    gap,
                    children,
                },
            )
        }
        NodeDto::Text { id, text, role } => {
            validate_id(&id)?;
            account_text(&text, limits)?;
            if !limits.node_ids.insert(id.clone()) {
                return Err(invalid("preview scene node IDs must be unique"));
            }
            (id, NodeKind::Text { text, role })
        }
        NodeDto::Table { id, columns, rows } => {
            validate_id(&id)?;
            if !limits.node_ids.insert(id.clone()) {
                return Err(invalid("preview scene node IDs must be unique"));
            }
            if columns.len() > MAX_COLUMNS {
                return Err(invalid("preview scene table exceeds the column limit"));
            }
            add_limit(&mut limits.rows, rows.len(), MAX_ROWS, "table row count")?;
            let cell_count = rows
                .len()
                .checked_mul(columns.len())
                .ok_or_else(|| invalid("preview scene cell count overflows"))?;
            add_limit(&mut limits.cells, cell_count, MAX_CELLS, "table cell count")?;
            let mut column_ids = HashSet::with_capacity(columns.len());
            let columns = columns
                .into_iter()
                .map(|column| {
                    validate_id(&column.id)?;
                    account_text(&column.label, limits)?;
                    if !column_ids.insert(column.id.clone()) {
                        return Err(invalid("preview scene table column IDs must be unique"));
                    }
                    let kind = match column.kind {
                        ColumnKindDto::Text => ColumnKind::Text,
                        ColumnKindDto::Quantity { unit } => ColumnKind::Quantity(unit.into()),
                    };
                    Ok(Column {
                        id: column.id,
                        label: column.label,
                        kind,
                    })
                })
                .collect::<io::Result<Vec<_>>>()?;
            let rows = rows
                .into_iter()
                .map(|row| {
                    if row.len() != columns.len() {
                        return Err(invalid(
                            "preview scene table row width does not match its columns",
                        ));
                    }
                    row.into_iter()
                        .zip(&columns)
                        .map(|(cell, column)| validate_cell(cell, column.kind, limits))
                        .collect::<io::Result<Vec<_>>>()
                })
                .collect::<io::Result<Vec<_>>>()?;
            (id, NodeKind::Table { columns, rows })
        }
        NodeDto::Canvas {
            id,
            width,
            height,
            paths,
        } => {
            validate_id(&id)?;
            if !limits.node_ids.insert(id.clone()) {
                return Err(invalid("preview scene node IDs must be unique"));
            }
            validate_dimension(width)?;
            validate_dimension(height)?;
            add_limit(&mut limits.canvases, 1, MAX_CANVASES, "canvas count")?;
            let paths = paths
                .into_iter()
                .map(|path| {
                    if path.points.len() < 2 {
                        return Err(invalid("preview scene paths need at least two points"));
                    }
                    add_limit(&mut limits.paths, 1, MAX_PATHS, "canvas path count")?;
                    add_limit(
                        &mut limits.points,
                        path.points.len(),
                        MAX_POINTS,
                        "canvas point count",
                    )?;
                    let points = path
                        .points
                        .into_iter()
                        .map(|point| {
                            if !point.x.is_finite()
                                || !point.y.is_finite()
                                || point.x < 0.0
                                || point.y < 0.0
                                || point.x > width
                                || point.y > height
                            {
                                return Err(invalid(
                                    "preview scene point is outside its canvas bounds",
                                ));
                            }
                            Ok(Point {
                                x: point.x,
                                y: point.y,
                            })
                        })
                        .collect::<io::Result<Vec<_>>>()?;
                    Ok(Path {
                        role: path.role,
                        weight: path.weight,
                        points,
                    })
                })
                .collect::<io::Result<Vec<_>>>()?;
            (
                id,
                NodeKind::Canvas {
                    width,
                    height,
                    paths,
                },
            )
        }
    };
    Ok(Node { id, kind })
}

fn validate_cell(
    cell: Option<CellDto>,
    schema: ColumnKind,
    limits: &mut Limits,
) -> io::Result<Option<Value>> {
    let Some(cell) = cell else {
        return Ok(None);
    };
    let value = match (cell, schema) {
        (CellDto::Text { value }, ColumnKind::Text) => {
            account_text(&value, limits)?;
            Value::Text(value)
        }
        (CellDto::Quantity { value, unit }, ColumnKind::Quantity(expected))
            if Unit::from(unit) == expected && value.is_finite() && value >= 0.0 =>
        {
            Value::Quantity(Quantity::new(value, expected)?)
        }
        _ => {
            return Err(invalid(
                "preview scene cell does not match its column schema",
            ));
        }
    };
    Ok(Some(value))
}

fn validate_dimension(value: f64) -> io::Result<()> {
    if !value.is_finite() || !(1.0..=MAX_CANVAS_DIMENSION).contains(&value) {
        return Err(invalid(
            "preview scene canvas dimensions are outside the supported range",
        ));
    }
    Ok(())
}

fn validate_id(value: &str) -> io::Result<()> {
    if value.len() > MAX_ID_BYTES
        || value.is_empty()
        || value.split('.').any(|segment| {
            segment.is_empty()
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
    {
        return Err(invalid("preview scene identifier is invalid"));
    }
    Ok(())
}

fn account_text(value: &str, limits: &mut Limits) -> io::Result<()> {
    if value.len() > MAX_STRING_BYTES {
        return Err(invalid("preview scene string exceeds the size limit"));
    }
    add_limit(
        &mut limits.text_bytes,
        value.len(),
        MAX_TEXT_BYTES,
        "text bytes",
    )
}

fn add_limit(current: &mut usize, amount: usize, max: usize, name: &str) -> io::Result<()> {
    *current = current
        .checked_add(amount)
        .ok_or_else(|| invalid_owned(format!("preview scene {name} overflows")))?;
    if *current > max {
        return Err(invalid_owned(format!(
            "preview scene exceeds the {name} limit"
        )));
    }
    Ok(())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn invalid_owned(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
