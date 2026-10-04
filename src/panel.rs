use std::collections::BTreeSet;
use std::path::PathBuf;

use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme as _, IconName, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::fs_ops::{self, Entry, SortKey};

const COLUMNS: [(SortKey, &str, &str, f32); 4] = [
    (SortKey::Name, "name", "Name", 280.),
    (SortKey::Ext, "ext", "Ext", 60.),
    (SortKey::Size, "size", "Size", 90.),
    (SortKey::Modified, "modified", "Modified", 140.),
];

/// Table delegate holding one panel's directory listing.
pub struct FileTable {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    pub marked: BTreeSet<PathBuf>,
    sort_col: usize,
    sort: ColumnSort,
}

impl FileTable {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            entries: Vec::new(),
            marked: BTreeSet::new(),
            sort_col: 0,
            sort: ColumnSort::Ascending,
        }
    }

    /// Replaces the listing, keeping marks that still exist.
    pub fn set_entries(&mut self, path: PathBuf, entries: Vec<Entry>) {
        if path != self.path {
            self.marked.clear();
        }
        self.path = path;
        self.entries = entries;
        self.marked
            .retain(|p| self.entries.iter().any(|e| &e.path == p));
        self.sort_entries();
    }

    fn sort_entries(&mut self) {
        let (key, descending) = match self.sort {
            ColumnSort::Default => (SortKey::Name, false),
            sort => (COLUMNS[self.sort_col].0, sort == ColumnSort::Descending),
        };
        fs_ops::sort_entries(&mut self.entries, key, descending);
    }

    pub fn position(&self, path: &PathBuf) -> Option<usize> {
        self.entries.iter().position(|e| &e.path == path)
    }
}

impl TableDelegate for FileTable {
    fn columns_count(&self, _: &App) -> usize {
        COLUMNS.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.entries.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        let (_, key, name, width) = COLUMNS[col_ix];
        let sort = if col_ix == self.sort_col {
            self.sort
        } else {
            ColumnSort::Default
        };
        let column = Column::new(key, name).width(px(width)).sort(sort);
        match col_ix {
            2 => column.text_right(),
            _ => column,
        }
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        self.sort_col = col_ix;
        self.sort = sort;
        self.sort_entries();
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(entry) = self.entries.get(row_ix) else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let color = if self.marked.contains(&entry.path) {
            theme.danger
        } else if entry.is_dir {
            theme.primary
        } else {
            theme.foreground
        };
        match COLUMNS[col_ix].0 {
            SortKey::Name => h_flex()
                .gap_2()
                .min_w_0()
                .text_color(color)
                .child(if entry.is_dir {
                    IconName::Folder
                } else {
                    IconName::File
                })
                .child(div().truncate().child(entry.name.clone()))
                .into_any_element(),
            SortKey::Ext => div()
                .text_color(color)
                .child(entry.ext().to_string())
                .into_any_element(),
            SortKey::Size => div()
                .w_full()
                .text_right()
                .text_color(color)
                .child(if entry.is_dir {
                    "<DIR>".to_string()
                } else {
                    fs_ops::format_size(entry.size)
                })
                .into_any_element(),
            SortKey::Modified => div()
                .text_color(theme.muted_foreground)
                .when_some(entry.modified, |d, t| d.child(fs_ops::format_time(t)))
                .into_any_element(),
        }
    }
}
