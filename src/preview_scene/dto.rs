//! Strict, serializable wire DTOs for preview scenes.

use crate::data_providers::Unit;

use serde::{Deserialize, Serialize};

/// Untrusted versioned display-scene data decoded from a worker response.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneDto {
    /// Scene grammar version.
    pub version: u32,
    /// Root of the display tree.
    pub root: NodeDto,
}

/// One untrusted node in a display scene.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NodeDto {
    /// A vertical or horizontal group of child nodes.
    Stack {
        /// Unique scene-wide node identifier.
        id: String,
        /// Child arrangement direction.
        axis: Axis,
        /// Native spacing between children.
        gap: Gap,
        /// Child nodes, in display order.
        children: Vec<NodeDto>,
    },
    /// Selectable text with a native visual role.
    Text {
        /// Unique scene-wide node identifier.
        id: String,
        /// Text contents.
        text: String,
        /// Native text style role.
        role: TextRole,
    },
    /// A typed table with optional values in each cell.
    Table {
        /// Unique scene-wide node identifier.
        id: String,
        /// Table columns and their value schemas.
        columns: Vec<ColumnDto>,
        /// Rows whose cell count must match the column count.
        rows: Vec<Vec<Option<CellDto>>>,
    },
    /// A bounded path canvas for static line graphs.
    Canvas {
        /// Unique scene-wide node identifier.
        id: String,
        /// Canvas width in native logical units.
        width: f64,
        /// Canvas height in native logical units.
        height: f64,
        /// Static paths drawn by native code.
        paths: Vec<PathDto>,
    },
}

/// Untrusted typed column metadata for a scene table.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnDto {
    /// Unique identifier within this table.
    pub id: String,
    /// User-visible heading.
    pub label: String,
    /// Accepted values in this column.
    pub kind: ColumnKindDto,
}

/// A table column's accepted value schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ColumnKindDto {
    /// UTF-8 text cells.
    Text,
    /// Numeric cells in one fixed semantic unit.
    Quantity {
        /// Unit required for every quantity cell.
        unit: SceneUnit,
    },
}

/// Untrusted typed table cell data.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CellDto {
    /// Text cell.
    Text {
        /// Cell text.
        value: String,
    },
    /// Numeric cell.
    Quantity {
        /// Finite nonnegative magnitude.
        value: f64,
        /// Semantic unit.
        unit: SceneUnit,
    },
}

/// Duration or distance units accepted in table schemas and cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneUnit {
    /// Elapsed time in seconds.
    Seconds,
    /// Distance in metres.
    Metres,
}

impl From<SceneUnit> for Unit {
    fn from(unit: SceneUnit) -> Self {
        match unit {
            SceneUnit::Seconds => Self::Seconds,
            SceneUnit::Metres => Self::Metres,
        }
    }
}

/// Untrusted path data in a scene canvas.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PathDto {
    /// Native semantic color role.
    pub role: PathRole,
    /// Native stroke width role.
    pub weight: StrokeWeight,
    /// Canvas-local points, in display order.
    pub points: Vec<PointDto>,
}

/// One untrusted canvas coordinate.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PointDto {
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
}

/// Stack arrangement direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    /// Place children from left to right.
    Row,
    /// Place children from top to bottom.
    Column,
}

/// Native stack spacing role.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Gap {
    /// Compact spacing.
    Small,
    /// Standard spacing.
    Normal,
}

/// Native text style role.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TextRole {
    /// Body text.
    Body,
    /// Heading text.
    Heading,
    /// Secondary text.
    Muted,
}

/// Native semantic color role for a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PathRole {
    /// Main foreground color.
    Foreground,
    /// Secondary color.
    Muted,
    /// Accent color.
    Accent,
    /// Success color.
    Success,
    /// Warning color.
    Warning,
}

/// Native stroke width role.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StrokeWeight {
    /// Thin stroke.
    Thin,
    /// Standard stroke.
    Normal,
    /// Strong stroke.
    Strong,
}
