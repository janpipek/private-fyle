//! Strict, bounded display scenes prepared by extension preview workers.
//!
//! The deserializable DTO is deliberately separate from [`Scene`]: renderers
//! receive only values that passed schema, identity, and resource-limit checks.

use crate::data_providers::{Unit, Value};

mod dto;
mod validation;

pub use dto::{
    Axis, CellDto, ColumnDto, ColumnKindDto, Gap, NodeDto, PathDto, PathRole, PointDto, SceneDto,
    SceneUnit, StrokeWeight, TextRole,
};
pub use validation::validate;

/// Maximum number of nodes in one scene tree.
pub const MAX_NODES: usize = 128;
/// Maximum node depth, counting the root as depth one.
pub const MAX_DEPTH: usize = 8;
/// Maximum byte length of any scene string.
pub const MAX_STRING_BYTES: usize = 4 * 1024;
/// Maximum combined byte length of displayed text and column labels.
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
/// Maximum number of table rows across all tables.
pub const MAX_ROWS: usize = 256;
/// Maximum number of columns in one table.
pub const MAX_COLUMNS: usize = 16;
/// Maximum number of cells across all tables.
pub const MAX_CELLS: usize = 2_048;
/// Maximum number of canvas nodes in one scene.
pub const MAX_CANVASES: usize = 8;
/// Maximum number of paths across all canvases.
pub const MAX_PATHS: usize = 64;
/// Maximum number of points across all paths.
pub const MAX_POINTS: usize = 8_192;
/// Maximum width or height of a canvas.
pub const MAX_CANVAS_DIMENSION: f64 = 4_096.0;
/// Maximum byte length of one scene or column identifier.
pub const MAX_ID_BYTES: usize = 128;

/// Validated extension scene with private construction.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    root: Node,
}

impl Scene {
    /// Returns the validated scene root for read-only rendering.
    pub fn root(&self) -> &Node {
        &self.root
    }
}

/// Validated scene node with private construction.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    id: String,
    kind: NodeKind,
}

impl Node {
    /// Returns the scene-wide unique node identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the validated node content for read-only rendering.
    pub fn kind(&self) -> &NodeKind {
        &self.kind
    }
}

/// Validated display content for one scene node.
#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    /// Child nodes arranged in one direction.
    Stack {
        /// Arrangement direction.
        axis: Axis,
        /// Native spacing role.
        gap: Gap,
        /// Child nodes in display order.
        children: Vec<Node>,
    },
    /// Selectable text.
    Text {
        /// Text contents.
        text: String,
        /// Native style role.
        role: TextRole,
    },
    /// Typed table values.
    Table {
        /// Validated column schemas.
        columns: Vec<Column>,
        /// Rows with validated typed values.
        rows: Vec<Vec<Option<Value>>>,
    },
    /// Static path canvas.
    Canvas {
        /// Validated canvas width.
        width: f64,
        /// Validated canvas height.
        height: f64,
        /// Validated paths in draw order.
        paths: Vec<Path>,
    },
}

/// Validated table column with private construction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    id: String,
    label: String,
    kind: ColumnKind,
}

impl Column {
    /// Returns the unique identifier within the containing table.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the displayed table heading.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the validated value schema.
    pub fn kind(&self) -> ColumnKind {
        self.kind
    }
}

/// Validated table column value schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnKind {
    /// UTF-8 text cells.
    Text,
    /// Numeric cells in one fixed unit.
    Quantity(Unit),
}

/// Validated table path with private construction.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    role: PathRole,
    weight: StrokeWeight,
    points: Vec<Point>,
}

impl Path {
    /// Returns the native semantic color role.
    pub fn role(&self) -> PathRole {
        self.role
    }

    /// Returns the native stroke width role.
    pub fn weight(&self) -> StrokeWeight {
        self.weight
    }

    /// Returns validated canvas-local points.
    pub fn points(&self) -> &[Point] {
        &self.points
    }
}

/// Validated canvas coordinate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    x: f64,
    y: f64,
}

impl Point {
    /// Returns the horizontal coordinate.
    pub fn x(&self) -> f64 {
        self.x
    }

    /// Returns the vertical coordinate.
    pub fn y(&self) -> f64 {
        self.y
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Axis, CellDto, ColumnDto, ColumnKind, ColumnKindDto, Gap, NodeDto, NodeKind, PathDto,
        PathRole, PointDto, SceneDto, SceneUnit, StrokeWeight, TextRole, validate,
    };
    use crate::data_providers::{Unit, Value};

    fn text_node(id: impl Into<String>, text: impl Into<String>) -> NodeDto {
        NodeDto::Text {
            id: id.into(),
            text: text.into(),
            role: TextRole::Body,
        }
    }

    fn scene(root: NodeDto) -> SceneDto {
        SceneDto { version: 1, root }
    }

    fn stack(id: impl Into<String>, children: Vec<NodeDto>) -> NodeDto {
        NodeDto::Stack {
            id: id.into(),
            axis: Axis::Column,
            gap: Gap::Small,
            children,
        }
    }

    fn table(id: &str, columns: usize, rows: Vec<Vec<Option<CellDto>>>) -> NodeDto {
        NodeDto::Table {
            id: id.into(),
            columns: (0..columns)
                .map(|index| ColumnDto {
                    id: format!("col{index}"),
                    label: format!("Column {index}"),
                    kind: ColumnKindDto::Text,
                })
                .collect(),
            rows,
        }
    }

    fn path(points: Vec<PointDto>) -> PathDto {
        PathDto {
            role: PathRole::Accent,
            weight: StrokeWeight::Normal,
            points,
        }
    }

    fn point(x: f64, y: f64) -> PointDto {
        PointDto { x, y }
    }

    #[test]
    fn validates_typed_tree_and_exposes_only_read_only_model_accessors() {
        let root = stack(
            "root",
            vec![
                NodeDto::Text {
                    id: "heading".into(),
                    text: "Track".into(),
                    role: TextRole::Heading,
                },
                NodeDto::Table {
                    id: "samples".into(),
                    columns: vec![ColumnDto {
                        id: "distance".into(),
                        label: "Distance".into(),
                        kind: ColumnKindDto::Quantity {
                            unit: SceneUnit::Metres,
                        },
                    }],
                    rows: vec![vec![Some(CellDto::Quantity {
                        value: 12.5,
                        unit: SceneUnit::Metres,
                    })]],
                },
                NodeDto::Canvas {
                    id: "profile".into(),
                    width: 320.0,
                    height: 120.0,
                    paths: vec![path(vec![point(0.0, 120.0), point(320.0, 0.0)])],
                },
            ],
        );
        let scene = validate(scene(root)).unwrap();
        let NodeKind::Stack { children, .. } = scene.root().kind() else {
            panic!("expected stack root");
        };
        assert_eq!(scene.root().id(), "root");
        assert!(matches!(
            children[0].kind(),
            NodeKind::Text {
                role: TextRole::Heading,
                ..
            }
        ));
        let NodeKind::Table { columns, rows } = children[1].kind() else {
            panic!("expected table node");
        };
        assert_eq!(columns[0].kind(), ColumnKind::Quantity(Unit::Metres));
        assert_eq!(
            rows[0][0],
            Some(Value::quantity(12.5, Unit::Metres).unwrap())
        );
        assert!(matches!(children[2].kind(), NodeKind::Canvas { .. }));
    }

    #[test]
    fn serde_dto_is_versioned_and_rejects_unknown_fields() {
        let parsed: SceneDto = serde_json::from_str(
            r#"{"version":1,"root":{"kind":"text","id":"root","text":"ok","role":"body"}}"#,
        )
        .unwrap();
        assert!(validate(parsed).is_ok());
        assert!(serde_json::from_str::<SceneDto>(
            r#"{"version":1,"unknown":true,"root":{"kind":"text","id":"root","text":"ok","role":"body"}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<SceneDto>(
            r#"{"version":1,"root":{"kind":"text","id":"root","text":"ok","role":"body","extra":1}}"#
        )
        .is_err());
        let wire = serde_json::to_string(&scene(text_node("root", "ok"))).unwrap();
        assert!(serde_json::from_str::<SceneDto>(&wire).is_ok());
        assert!(
            validate(SceneDto {
                version: 2,
                root: text_node("root", "ok"),
            })
            .is_err()
        );
    }

    #[test]
    fn node_and_depth_limits_accept_boundary_and_reject_one_beyond() {
        let accepted = stack(
            "root",
            (0..super::MAX_NODES - 1)
                .map(|index| text_node(format!("n{index}"), ""))
                .collect(),
        );
        assert!(validate(scene(accepted)).is_ok());
        let rejected = stack(
            "root",
            (0..super::MAX_NODES)
                .map(|index| text_node(format!("n{index}"), ""))
                .collect(),
        );
        assert!(validate(scene(rejected)).is_err());

        let mut at_depth = text_node("leaf", "");
        for index in (0..super::MAX_DEPTH - 1).rev() {
            at_depth = stack(format!("s{index}"), vec![at_depth]);
        }
        assert!(validate(scene(at_depth.clone())).is_ok());
        assert!(validate(scene(stack("extra", vec![at_depth]))).is_err());
    }

    #[test]
    fn text_and_identifier_limits_are_enforced() {
        assert!(validate(scene(text_node("a", "x".repeat(super::MAX_STRING_BYTES)))).is_ok());
        assert!(
            validate(scene(text_node(
                "a",
                "x".repeat(super::MAX_STRING_BYTES + 1)
            )))
            .is_err()
        );
        assert!(validate(scene(text_node("a..b", "x"))).is_err());
        assert!(validate(scene(text_node("", "x"))).is_err());
        assert!(validate(scene(text_node("a".repeat(super::MAX_ID_BYTES + 1), "x"))).is_err());

        let exactly_total = stack(
            "root",
            (0..16)
                .map(|index| text_node(format!("n{index}"), "x".repeat(4 * 1024)))
                .collect(),
        );
        assert!(validate(scene(exactly_total)).is_ok());
        let over_total = stack(
            "root",
            (0..17)
                .map(|index| text_node(format!("n{index}"), "x".repeat(4 * 1024)))
                .collect(),
        );
        assert!(validate(scene(over_total)).is_err());
    }

    #[test]
    fn tables_enforce_rows_columns_cells_width_ids_and_typed_units() {
        let exact_rows = table(
            "rows",
            1,
            (0..super::MAX_ROWS).map(|_| vec![None]).collect(),
        );
        assert!(validate(scene(exact_rows)).is_ok());
        let over_rows = table(
            "rows",
            1,
            (0..super::MAX_ROWS + 1).map(|_| vec![None]).collect(),
        );
        assert!(validate(scene(over_rows)).is_err());
        assert!(validate(scene(table("wide", super::MAX_COLUMNS + 1, vec![]))).is_err());
        assert!(validate(scene(table("ragged", 2, vec![vec![None]]))).is_err());

        let over_cells = stack(
            "root",
            vec![
                table("first", 16, (0..128).map(|_| vec![None; 16]).collect()),
                table("second", 1, vec![vec![None]]),
            ],
        );
        assert!(validate(scene(over_cells)).is_err());
        let under_cells = table("under", 16, (0..128).map(|_| vec![None; 16]).collect());
        assert!(validate(scene(under_cells)).is_ok());

        let wrong_type = NodeDto::Table {
            id: "typed".into(),
            columns: vec![ColumnDto {
                id: "distance".into(),
                label: "Distance".into(),
                kind: ColumnKindDto::Quantity {
                    unit: SceneUnit::Metres,
                },
            }],
            rows: vec![vec![Some(CellDto::Text {
                value: "wrong".into(),
            })]],
        };
        assert!(validate(scene(wrong_type)).is_err());
        let wrong_unit = NodeDto::Table {
            id: "typed".into(),
            columns: vec![ColumnDto {
                id: "distance".into(),
                label: "Distance".into(),
                kind: ColumnKindDto::Quantity {
                    unit: SceneUnit::Metres,
                },
            }],
            rows: vec![vec![Some(CellDto::Quantity {
                value: 1.0,
                unit: SceneUnit::Seconds,
            })]],
        };
        assert!(validate(scene(wrong_unit)).is_err());
    }

    #[test]
    fn duplicate_node_and_per_table_column_ids_are_rejected() {
        assert!(validate(scene(stack("root", vec![text_node("root", "x")]))).is_err());
        let duplicate_columns = NodeDto::Table {
            id: "table".into(),
            columns: vec![
                ColumnDto {
                    id: "same".into(),
                    label: "A".into(),
                    kind: ColumnKindDto::Text,
                },
                ColumnDto {
                    id: "same".into(),
                    label: "B".into(),
                    kind: ColumnKindDto::Text,
                },
            ],
            rows: vec![],
        };
        assert!(validate(scene(duplicate_columns)).is_err());
    }

    #[test]
    fn canvases_enforce_dimensions_path_points_and_aggregate_bounds() {
        let boundary = NodeDto::Canvas {
            id: "canvas".into(),
            width: super::MAX_CANVAS_DIMENSION,
            height: 1.0,
            paths: vec![path(vec![
                point(0.0, 0.0),
                point(super::MAX_CANVAS_DIMENSION, 1.0),
            ])],
        };
        assert!(validate(scene(boundary)).is_ok());
        let out_of_bounds = NodeDto::Canvas {
            id: "canvas".into(),
            width: 10.0,
            height: 10.0,
            paths: vec![path(vec![point(0.0, 0.0), point(10.1, 2.0)])],
        };
        assert!(validate(scene(out_of_bounds)).is_err());
        let too_small = NodeDto::Canvas {
            id: "canvas".into(),
            width: 0.0,
            height: 1.0,
            paths: vec![],
        };
        assert!(validate(scene(too_small)).is_err());
        let one_point = NodeDto::Canvas {
            id: "canvas".into(),
            width: 1.0,
            height: 1.0,
            paths: vec![path(vec![point(0.0, 0.0)])],
        };
        assert!(validate(scene(one_point)).is_err());

        let exact_paths = NodeDto::Canvas {
            id: "paths".into(),
            width: 10.0,
            height: 10.0,
            paths: (0..super::MAX_PATHS)
                .map(|_| path(vec![point(0.0, 0.0), point(1.0, 1.0)]))
                .collect(),
        };
        assert!(validate(scene(exact_paths)).is_ok());
        let too_many_paths = NodeDto::Canvas {
            id: "paths".into(),
            width: 10.0,
            height: 10.0,
            paths: (0..super::MAX_PATHS + 1)
                .map(|_| path(vec![point(0.0, 0.0), point(1.0, 1.0)]))
                .collect(),
        };
        assert!(validate(scene(too_many_paths)).is_err());

        let exact_canvases = stack(
            "root",
            (0..super::MAX_CANVASES)
                .map(|index| NodeDto::Canvas {
                    id: format!("canvas{index}"),
                    width: 1.0,
                    height: 1.0,
                    paths: vec![],
                })
                .collect(),
        );
        assert!(validate(scene(exact_canvases)).is_ok());
        let too_many_canvases = stack(
            "root",
            (0..super::MAX_CANVASES + 1)
                .map(|index| NodeDto::Canvas {
                    id: format!("canvas{index}"),
                    width: 1.0,
                    height: 1.0,
                    paths: vec![],
                })
                .collect(),
        );
        assert!(validate(scene(too_many_canvases)).is_err());

        let exact_points = NodeDto::Canvas {
            id: "exact-points".into(),
            width: 10.0,
            height: 10.0,
            paths: (0..64)
                .map(|_| {
                    path(
                        (0..128)
                            .map(|i| point((i % 10) as f64, (i % 10) as f64))
                            .collect(),
                    )
                })
                .collect(),
        };
        assert!(validate(scene(exact_points)).is_ok());

        let point_limit = NodeDto::Canvas {
            id: "points".into(),
            width: 10.0,
            height: 10.0,
            paths: (0..64)
                .map(|index| {
                    let count = if index == 0 { 129 } else { 128 };
                    path(
                        (0..count)
                            .map(|i| point((i % 10) as f64, (i % 10) as f64))
                            .collect(),
                    )
                })
                .collect(),
        };
        assert!(validate(scene(point_limit)).is_err());
        let nonfinite = NodeDto::Canvas {
            id: "nonfinite".into(),
            width: 10.0,
            height: 10.0,
            paths: vec![path(vec![point(0.0, 0.0), point(f64::NAN, 1.0)])],
        };
        assert!(validate(scene(nonfinite)).is_err());
    }

    #[test]
    fn typed_quantities_reject_nonfinite_or_negative_values() {
        for value in [f64::NAN, f64::INFINITY, -1.0] {
            let dto = NodeDto::Table {
                id: "typed".into(),
                columns: vec![ColumnDto {
                    id: "distance".into(),
                    label: "Distance".into(),
                    kind: ColumnKindDto::Quantity {
                        unit: SceneUnit::Metres,
                    },
                }],
                rows: vec![vec![Some(CellDto::Quantity {
                    value,
                    unit: SceneUnit::Metres,
                })]],
            };
            assert!(validate(scene(dto)).is_err());
        }
    }
}
