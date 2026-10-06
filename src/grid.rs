//! Icon grid geometry, UI-independent: the panel's view mode, the stepped
//! icon sizes, how many cells fit in a row and where arrow keys move the
//! cursor. `icon_view.rs` only draws what this module computes.

use std::ops::Range;

use serde::Deserialize;

/// How a panel shows its folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ViewMode {
    /// The table with columns.
    #[default]
    List,
    /// A grid of icons with names below.
    Icons,
}

impl ViewMode {
    /// The value written to `config.toml`.
    pub fn id(self) -> &'static str {
        match self {
            ViewMode::List => "list",
            ViewMode::Icons => "icons",
        }
    }
}

/// Icon sizes in pixels the Larger / Smaller commands step through.
pub const SIZES: [u32; 5] = [48, 64, 96, 128, 160];

pub const DEFAULT_SIZE: u32 = 96;

/// The step closest to `size` (so a hand-edited `icon_size = 100` works).
pub fn clamp_size(size: u32) -> u32 {
    SIZES
        .into_iter()
        .min_by_key(|step| step.abs_diff(size))
        .unwrap_or(DEFAULT_SIZE)
}

/// `size` moved by `steps` sizes (negative: smaller), stopping at the ends.
pub fn step_size(size: u32, steps: i32) -> u32 {
    let current = SIZES
        .iter()
        .position(|&s| s == clamp_size(size))
        .unwrap_or(0) as i32;
    let next = (current + steps).clamp(0, SIZES.len() as i32 - 1);
    SIZES[next as usize]
}

/// Gap between cells, and padding inside one, in pixels.
pub const GAP: f32 = 4.;
pub const PADDING: f32 = 6.;
/// Narrowest a cell gets, so names under small icons stay readable.
const MIN_CELL_WIDTH: f32 = 84.;

/// Where cells go for a given panel width and icon size. Every cell has the
/// same size, which is what lets the view render rows lazily.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub cols: usize,
    pub cell_width: f32,
    pub cell_height: f32,
}

impl Layout {
    /// The cells fitting in `width` pixels (at least one column); each has
    /// room for `label_height` pixels of name under the icon.
    pub fn new(width: f32, icon_size: u32, label_height: f32) -> Self {
        let cell_width = (icon_size as f32 + 2. * PADDING + 12.).max(MIN_CELL_WIDTH);
        let cell_height = icon_size as f32 + 2. * PADDING + label_height + 4.;
        let cols = ((width + GAP) / (cell_width + GAP)).floor().max(1.) as usize;
        Self {
            cols,
            cell_width,
            cell_height,
        }
    }

    /// Height of a row of cells including the gap below it.
    pub fn row_height(&self) -> f32 {
        self.cell_height + GAP
    }

    /// Rows needed for `count` cells.
    pub fn rows(&self, count: usize) -> usize {
        count.div_ceil(self.cols)
    }

    /// The row holding cell `ix`.
    pub fn row_of(&self, ix: usize) -> usize {
        ix / self.cols
    }

    /// The cells of `row`; the last row may be partial.
    pub fn cells(&self, row: usize, count: usize) -> Range<usize> {
        let start = (row * self.cols).min(count);
        start..(start + self.cols).min(count)
    }

    /// Rows fully visible in a viewport `height` pixels tall (at least one):
    /// how far PageUp and PageDown move.
    pub fn page_rows(&self, height: f32) -> usize {
        ((height / self.row_height()).floor() as usize).max(1)
    }
}

/// A cursor move requested by an arrow or page key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
}

/// Where the cursor goes from `cursor` among `count` cells laid out in
/// `cols` columns. Moves stop at the edges instead of wrapping; Down from a
/// full row into a shorter last row lands on its last cell; paging keeps
/// the column.
pub fn move_cursor(cursor: usize, count: usize, cols: usize, page_rows: usize, mv: Move) -> usize {
    if count == 0 {
        return 0;
    }
    let cols = cols.max(1);
    let cursor = cursor.min(count - 1);
    let (row, col) = (cursor / cols, cursor % cols);
    let last_row = (count - 1) / cols;
    match mv {
        Move::Left => cursor.saturating_sub(1),
        Move::Right => (cursor + 1).min(count - 1),
        Move::Up if row > 0 => cursor - cols,
        Move::Up => cursor,
        Move::Down if row < last_row => (cursor + cols).min(count - 1),
        Move::Down => cursor,
        Move::PageUp => row.saturating_sub(page_rows) * cols + col,
        Move::PageDown => ((row + page_rows).min(last_row) * cols + col).min(count - 1),
    }
}

#[cfg(test)]
mod tests {
    use super::{Layout, Move, ViewMode, clamp_size, move_cursor, step_size};

    #[test]
    fn sizes_snap_and_step_within_the_ends() {
        assert_eq!(clamp_size(100), 96);
        assert_eq!(clamp_size(0), 48);
        assert_eq!(clamp_size(1000), 160);
        assert_eq!(step_size(96, 1), 128);
        assert_eq!(step_size(96, -2), 48);
        assert_eq!(step_size(48, -1), 48);
        assert_eq!(step_size(160, 1), 160);
        assert_eq!(step_size(100, 1), 128);
    }

    #[test]
    fn columns_follow_width_and_size() {
        let small = Layout::new(800., 48, 34.);
        let large = Layout::new(800., 160, 34.);
        assert!(small.cols > large.cols);
        assert!(large.cols >= 1);
        // Never zero columns, even in a sliver of a panel.
        assert_eq!(Layout::new(10., 160, 34.).cols, 1);
        assert_eq!(Layout::new(0., 96, 34.).cols, 1);
        // The cells fit the width.
        let layout = Layout::new(800., 96, 34.);
        assert!(layout.cols as f32 * layout.cell_width <= 800.);
    }

    #[test]
    fn rows_and_cells_cover_a_partial_last_row() {
        let layout = Layout {
            cols: 4,
            cell_width: 100.,
            cell_height: 100.,
        };
        assert_eq!(layout.rows(0), 0);
        assert_eq!(layout.rows(8), 2);
        assert_eq!(layout.rows(9), 3);
        assert_eq!(layout.row_of(7), 1);
        assert_eq!(layout.cells(0, 9), 0..4);
        assert_eq!(layout.cells(2, 9), 8..9);
        assert_eq!(layout.cells(5, 9), 9..9);
        assert_eq!(layout.page_rows(10.), 1);
        assert_eq!(layout.page_rows(1040.), 10);
    }

    #[test]
    fn arrows_move_by_cell_and_row_and_stop_at_edges() {
        // 10 cells in 4 columns: rows 0-3, 4-7, 8-9.
        let go = |cursor, mv| move_cursor(cursor, 10, 4, 2, mv);
        assert_eq!(go(5, Move::Left), 4);
        assert_eq!(go(0, Move::Left), 0);
        assert_eq!(go(5, Move::Right), 6);
        assert_eq!(go(9, Move::Right), 9);
        assert_eq!(go(5, Move::Up), 1);
        assert_eq!(go(2, Move::Up), 2);
        assert_eq!(go(5, Move::Down), 9);
        assert_eq!(go(1, Move::Down), 5);
        // Down into the short last row lands on its last cell.
        assert_eq!(go(7, Move::Down), 9);
        assert_eq!(go(9, Move::Down), 9);
        assert_eq!(go(8, Move::Down), 8);
    }

    #[test]
    fn paging_keeps_the_column_and_clamps() {
        let go = |cursor, mv| move_cursor(cursor, 30, 4, 2, mv);
        assert_eq!(go(13, Move::PageUp), 5);
        assert_eq!(go(5, Move::PageUp), 1);
        assert_eq!(go(13, Move::PageDown), 21);
        // The last row has two cells (28, 29): column 3 clamps to the end.
        assert_eq!(go(23, Move::PageDown), 29);
        assert_eq!(go(26, Move::PageDown), 29);
    }

    #[test]
    fn empty_and_single_column_grids() {
        assert_eq!(move_cursor(0, 0, 4, 2, Move::Down), 0);
        assert_eq!(move_cursor(3, 5, 1, 2, Move::Down), 4);
        assert_eq!(move_cursor(3, 5, 1, 2, Move::Right), 4);
        assert_eq!(move_cursor(3, 5, 0, 2, Move::Up), 2);
        // A stale cursor beyond the end is clamped first.
        assert_eq!(move_cursor(50, 5, 4, 2, Move::Left), 3);
    }

    #[test]
    fn view_modes_parse_and_print_as_in_the_settings() {
        for mode in [ViewMode::List, ViewMode::Icons] {
            let parsed: ViewMode = toml::from_str::<std::collections::HashMap<String, ViewMode>>(
                &format!("v = \"{}\"", mode.id()),
            )
            .expect("valid mode")["v"];
            assert_eq!(parsed, mode);
        }
    }
}
