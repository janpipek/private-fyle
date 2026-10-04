//! Table view of a [`Folder`] model. Holds no file state of its own: it
//! renders the model and forwards cursor moves, sorting and activation.

use gpui_kit::component::table::{
    Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableState,
};
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::fs_ops::{self, SortKey};
use crate::icons::{self, FileIcon};
use crate::model::{Folder, Sort};
use gpui_kit::assets::IconName as Lucide;

const COLUMNS: [(SortKey, &str, &str, f32); 4] = [
    (SortKey::Name, "name", "Name", 280.),
    (SortKey::Ext, "ext", "Ext", 60.),
    (SortKey::Size, "size", "Size", 90.),
    (SortKey::Modified, "modified", "Modified", 140.),
];

/// Emitted when the user activates the cursor entry with the mouse.
pub struct Activate;

pub struct FolderView {
    folder: Entity<Folder>,
    table: Entity<TableState<FileTable>>,
    active: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Activate> for FolderView {}

impl FolderView {
    pub fn new(folder: Entity<Folder>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let delegate = FileTable {
            folder: folder.clone(),
        };
        let table = cx.new(|cx| {
            TableState::new(delegate, window, cx)
                .row_header(false)
                .col_selectable(false)
                .col_movable(false)
                .loop_selection(false)
        });
        let subscriptions = vec![
            cx.observe(&folder, |this, _, cx| this.sync_cursor(cx)),
            cx.subscribe(&table, |this, _, event: &TableEvent, cx| match event {
                TableEvent::SelectRow(row) => this.set_cursor(*row, cx),
                TableEvent::DoubleClickedRow(row) => {
                    this.set_cursor(*row, cx);
                    cx.emit(Activate);
                }
                _ => {}
            }),
        ];
        let this = Self {
            folder,
            table,
            active: false,
            _subscriptions: subscriptions,
        };
        this.sync_cursor(cx);
        this
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.table.focus_handle(cx)
    }

    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active != active {
            self.active = active;
            cx.notify();
        }
    }

    fn set_cursor(&self, row: usize, cx: &mut Context<Self>) {
        self.folder.update(cx, |folder, cx| {
            if folder.set_cursor(row) {
                cx.notify();
            }
        });
    }

    /// Pushes the model's cursor into the table and repaints it.
    fn sync_cursor(&self, cx: &mut Context<Self>) {
        let folder = self.folder.read(cx);
        let (cursor, rows) = (folder.cursor(), folder.entries().len());
        self.table.update(cx, |table, cx| {
            if rows > 0 && table.selected_row() != Some(cursor) {
                table.set_selected_row(cursor, cx);
            }
            cx.notify();
        });
        cx.notify();
    }
}

impl Render for FolderView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let folder = self.folder.read(cx);
        let mut footer = format!("{} dirs, {} files", folder.dir_count(), folder.file_count());
        if folder.marked_count() > 0 {
            footer += &format!(" · {} marked", folder.marked_count());
        }

        v_flex()
            .size_full()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(theme.radius)
                    .when(self.active, |d| {
                        d.bg(theme.primary).text_color(theme.primary_foreground)
                    })
                    .when(!self.active, |d| d.bg(theme.muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(fs_ops::display_path(folder.path())),
                    )
                    .when(folder.is_tree(), |d| {
                        d.child(div().flex_none().text_xs().child("TREE"))
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(DataTable::new(&self.table).small()),
            )
            .child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(footer),
            )
    }
}

/// Adapts a [`Folder`] to gpui-component's table.
pub struct FileTable {
    folder: Entity<Folder>,
}

impl TableDelegate for FileTable {
    fn columns_count(&self, _: &App) -> usize {
        COLUMNS.len()
    }

    fn rows_count(&self, cx: &App) -> usize {
        self.folder.read(cx).entries().len()
    }

    fn column(&self, col_ix: usize, cx: &App) -> Column {
        let (key, id, name, width) = COLUMNS[col_ix];
        let sort = self.folder.read(cx).sort();
        let column_sort = match sort.key == key {
            true if sort.descending => ColumnSort::Descending,
            true => ColumnSort::Ascending,
            false => ColumnSort::Default,
        };
        let column = Column::new(id, name).width(px(width)).sort(column_sort);
        if key == SortKey::Size {
            column.text_right()
        } else {
            column
        }
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        // The table's "unsorted" state means the default order: name, ascending.
        let sort = match sort {
            ColumnSort::Default => Sort::default(),
            sort => Sort {
                key: COLUMNS[col_ix].0,
                descending: sort == ColumnSort::Descending,
            },
        };
        self.folder.update(cx, |folder, cx| {
            folder.set_sort(sort);
            cx.notify();
        });
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let folder = self.folder.read(cx);
        let Some(row) = folder.entries().get(row_ix) else {
            return div().into_any_element();
        };
        let entry = &row.entry;
        let theme = cx.theme();
        let color = if folder.is_marked(&entry.path) {
            theme.danger
        } else if entry.is_dir {
            theme.primary
        } else {
            theme.foreground
        };
        match COLUMNS[col_ix].0 {
            SortKey::Name => {
                let expanded = folder.is_expanded(&entry.path);
                let icon = glyph(icons::icon_for(
                    &entry.name,
                    entry.is_dir,
                    entry.is_parent,
                    expanded,
                ));
                // Tree mode: indentation plus a clickable expand chevron.
                let chevron = folder.is_tree().then(|| {
                    let toggle = (entry.is_dir && !entry.is_parent).then(|| {
                        let (folder, path) = (self.folder.clone(), entry.path.clone());
                        div()
                            .child(if expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                folder.update(cx, |folder, cx| {
                                    folder.toggle_expanded(&path);
                                    cx.notify();
                                });
                            })
                    });
                    div().w(px(14.)).flex_none().children(toggle)
                });
                h_flex()
                    .gap_1p5()
                    .min_w_0()
                    .pl(px(16. * row.depth as f32))
                    .text_color(color)
                    .children(chevron)
                    .child(icon)
                    .child(div().truncate().child(entry.name.clone()))
                    .into_any_element()
            }
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

/// The Lucide glyph drawing each kind of [`FileIcon`].
fn glyph(icon: FileIcon) -> Lucide {
    match icon {
        FileIcon::ParentDir => Lucide::FolderUp,
        FileIcon::Folder => Lucide::Folder,
        FileIcon::FolderOpen => Lucide::FolderOpen,
        FileIcon::FolderGit => Lucide::FolderGit2,
        FileIcon::File => Lucide::File,
        FileIcon::Code => Lucide::FileCode,
        FileIcon::Data => Lucide::FileBraces,
        FileIcon::Text => Lucide::FileText,
        FileIcon::Image => Lucide::FileImage,
        FileIcon::Audio => Lucide::FileMusic,
        FileIcon::Video => Lucide::FileVideoCamera,
        FileIcon::Archive => Lucide::FileArchive,
        FileIcon::Spreadsheet => Lucide::FileSpreadsheet,
        FileIcon::Config => Lucide::FileCog,
        FileIcon::Lock => Lucide::FileLock,
        FileIcon::Key => Lucide::FileKey,
        FileIcon::Script => Lucide::FileTerminal,
        FileIcon::Font => Lucide::FileType,
        FileIcon::Diff => Lucide::FileDiff,
    }
}
