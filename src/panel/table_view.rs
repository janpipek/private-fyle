//! File-list table rendering and event forwarding for the shared folder model.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::columns::{Catalog, ColumnId, ColumnKind};
use crate::context_menu::{self, ContextMenuBuilder};
use crate::entry::Sort;
use crate::fs_ops;
use crate::git_status;
use crate::glyphs::glyph;
use crate::icons;
use crate::model::{Folder, PointerSelection};

use super::{PanelDrag, drag_ghost, metadata, native_drag_payload};

/// Adapts a [`Folder`] to gpui-component's table.
pub struct FileTable {
    folder: Entity<Folder>,
    columns: Vec<ColumnId>,
    catalog: Arc<Catalog>,
    metadata: Entity<metadata::MetadataState>,
    git: Option<Arc<git_status::Status>>,
    context_menu: Option<ContextMenuBuilder>,
    focus: Option<FocusHandle>,
    context_row: Rc<Cell<Option<usize>>>,
    drop_row: Rc<Cell<Option<usize>>>,
}

impl FileTable {
    /// Creates a list adapter sharing the panel's folder, metadata and drop target.
    pub fn new(
        folder: Entity<Folder>,
        columns: Vec<ColumnId>,
        catalog: Arc<Catalog>,
        metadata: Entity<metadata::MetadataState>,
        drop_row: Rc<Cell<Option<usize>>>,
    ) -> Self {
        Self {
            folder,
            columns,
            catalog,
            metadata,
            git: None,
            context_menu: None,
            focus: None,
            context_row: Rc::new(Cell::new(None)),
            drop_row,
        }
    }

    /// Visible columns in display order.
    pub fn columns(&self) -> &[ColumnId] {
        &self.columns
    }

    /// Replaces the visible columns; the panel shell refreshes the table.
    pub fn set_columns(&mut self, columns: Vec<ColumnId>) {
        self.columns = columns;
    }

    /// Installs the Git status computed by the panel's background worker.
    pub fn set_git_status(&mut self, status: Option<git_status::Status>) {
        self.git = status.map(Arc::new);
    }

    /// Shares the panel's command builder and focus for row context menus.
    pub fn set_context_menu(&mut self, builder: ContextMenuBuilder, focus: FocusHandle) {
        self.context_menu = Some(builder);
        self.focus = Some(focus);
    }
}

impl TableDelegate for FileTable {
    fn render_tr(
        &mut self,
        row_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let context_row = self.context_row.clone();
        let drop_row = self.drop_row.clone();
        let folder = self.folder.clone();
        let drag = folder
            .read(cx)
            .drag_snapshot(row_ix)
            .map(|snapshot| PanelDrag {
                folder: folder.clone(),
                snapshot,
            });
        div()
            .id(("row", row_ix))
            .on_hover(move |hovered, _, _| {
                if *hovered {
                    drop_row.set(Some(row_ix));
                } else if drop_row.get() == Some(row_ix) {
                    drop_row.set(None);
                }
            })
            .when_some(drag, |row, drag| {
                row.on_drag(drag, |drag, _, _, cx| drag_ghost(drag, cx))
                    .external_drag_payload(|drag: &PanelDrag, _, _| native_drag_payload(drag))
            })
            .when_some(self.focus.clone(), |row, focus| {
                // Focus before the table's context menu captures where to return
                // on dismissal, including when the other panel was active.
                row.capture_any_mouse_down(move |event, window, cx| {
                    if event.button == MouseButton::Right {
                        context_row.set(Some(row_ix));
                        focus.focus(window, cx);
                    } else if event.button == MouseButton::Left {
                        let mode = match (event.modifiers.control, event.modifiers.shift) {
                            (false, false) => PointerSelection::Single,
                            (true, false) => PointerSelection::Toggle,
                            (false, true) => PointerSelection::Range,
                            (true, true) => PointerSelection::AddRange,
                        };
                        folder.update(cx, |folder, cx| {
                            folder.pointer_select(row_ix, mode);
                            cx.notify();
                        });
                    }
                })
            })
    }

    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        // The table retains its last right-clicked row even on a header or
        // blank-space click. Only build actions for a fresh row mouse press.
        if self.context_row.take() != Some(row_ix) {
            return menu;
        }
        context_menu::file_menu(
            &self.folder,
            row_ix,
            self.context_menu.as_ref(),
            menu,
            window,
            cx,
        )
    }

    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, cx: &App) -> usize {
        self.folder.read(cx).entries().len()
    }

    fn column(&self, col_ix: usize, cx: &App) -> Column {
        let descriptor = self.catalog.display_descriptor(&self.columns[col_ix]);
        let mut column = Column::new(descriptor.id.as_str().to_owned(), descriptor.title)
            .width(px(descriptor.width));
        if let Some(key) = descriptor.sort_key {
            let sort = self.folder.read(cx).sort();
            column = column.sort(match sort.key == key {
                true if sort.descending => ColumnSort::Descending,
                true => ColumnSort::Ascending,
                false => ColumnSort::Default,
            });
        }
        if descriptor.builtin == Some(ColumnKind::Size) {
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
        let Some(key) = self
            .catalog
            .descriptor(&self.columns[col_ix])
            .and_then(|item| item.sort_key)
        else {
            return;
        };
        let sort = match sort {
            ColumnSort::Default => Sort::default(),
            sort => Sort {
                key,
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
        let id = self.columns[col_ix].clone();
        let Some(kind) = id.builtin() else {
            let (location, name, is_dir, is_parent, generation, uid, link) = {
                let folder = self.folder.read(cx);
                let Some(row) = folder.entries().get(row_ix) else {
                    return div().into_any_element();
                };
                (
                    row.location.clone(),
                    row.entry.name.clone(),
                    row.is_dir,
                    row.is_parent,
                    folder.generation(),
                    row.entry.attrs.uid,
                    row.entry.link.clone(),
                )
            };
            let Some(path) = location.as_local_path().filter(|_| !is_parent) else {
                return div().into_any_element();
            };
            let path = path.to_path_buf();
            let applies = self.catalog.provider_applies(&id, &location, &name, is_dir);
            let fields = applies
                .then_some(metadata::Field::Provider(id.clone()))
                .into_iter()
                .collect::<Vec<_>>();
            let display = self
                .metadata
                .read(cx)
                .get(&path, generation)
                .and_then(|values| values.provider_cell(&id))
                .map(crate::columns::cell::project)
                .unwrap_or_default();
            self.metadata.update(cx, |metadata, cx| {
                metadata.request_row(
                    metadata::RowRequest {
                        location,
                        generation,
                        fields,
                        uid,
                        link,
                        row: Some(metadata::model::CapturedRow { name, is_dir }),
                    },
                    cx,
                )
            });
            let mut cell = div()
                .id(("provider", row_ix))
                .w_full()
                .h_full()
                .truncate()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(display.text);
            if let Some(tip) = display.tooltip {
                let tip = format!("{}: {tip}", path.display());
                cell = cell.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx));
            }
            return cell.into_any_element();
        };
        if matches!(
            kind,
            ColumnKind::Owner
                | ColumnKind::Xattrs
                | ColumnKind::Selinux
                | ColumnKind::Exif(_)
                | ColumnKind::Duration
        ) {
            let (location, name, is_dir, is_parent, generation, uid, link) = {
                let folder = self.folder.read(cx);
                let Some(row) = folder.entries().get(row_ix) else {
                    return div().into_any_element();
                };
                (
                    row.location.clone(),
                    row.entry.name.clone(),
                    row.is_dir,
                    row.is_parent,
                    folder.generation(),
                    row.entry.attrs.uid,
                    row.entry.link.clone(),
                )
            };
            let Some(path) = location.as_local_path().filter(|_| !is_parent) else {
                return div().into_any_element();
            };
            let path = path.to_path_buf();
            let builtin_columns: Vec<_> =
                self.columns.iter().filter_map(ColumnId::builtin).collect();
            let fields = metadata::fields_for_row(&builtin_columns, &name, is_dir, uid);
            let text = self
                .metadata
                .read(cx)
                .get(&path, generation)
                .and_then(|values| values.column(kind))
                .unwrap_or_default()
                .to_owned();
            self.metadata.update(cx, |metadata, cx| {
                metadata.request_row(
                    metadata::RowRequest {
                        location,
                        generation,
                        fields,
                        uid,
                        link,
                        row: Some(metadata::model::CapturedRow { name, is_dir }),
                    },
                    cx,
                );
            });
            let theme = cx.theme();
            return div()
                .truncate()
                .text_xs()
                .when(!matches!(kind, ColumnKind::Exif(_)), |d| {
                    d.font_family(theme.mono_font_family.clone())
                })
                .text_color(theme.muted_foreground)
                .child(text)
                .into_any_element();
        }
        if kind == ColumnKind::Name {
            let request = {
                let folder = self.folder.read(cx);
                folder.entries().get(row_ix).and_then(|row| {
                    row.location.as_local_path()?;
                    matches!(row.entry.link, crate::links::LinkState::Link { .. }).then(|| {
                        (
                            row.location.clone(),
                            folder.generation(),
                            row.entry.link.clone(),
                        )
                    })
                })
            };
            if let Some((location, generation, link)) = request {
                self.metadata.update(cx, |metadata, cx| {
                    metadata.request(
                        location,
                        generation,
                        &[metadata::Field::ResolvedLink],
                        None,
                        link,
                        cx,
                    );
                });
            }
        }
        let folder = self.folder.read(cx);
        let Some(row) = folder.entries().get(row_ix) else {
            return div().into_any_element();
        };
        let entry = &row.entry;
        let theme = cx.theme();
        let color = if folder.is_marked(&row.location) {
            theme.danger
        } else if entry.link.is_broken() {
            theme.muted_foreground
        } else if entry.is_dir {
            theme.primary
        } else {
            theme.foreground
        };
        match kind {
            ColumnKind::Name => {
                let expanded = folder.is_expanded(&row.location);
                let icon = glyph(icons::icon_for(
                    &entry.name,
                    entry.is_dir,
                    entry.is_parent,
                    expanded,
                ));
                // Tree mode: indentation plus a clickable expand chevron.
                let chevron = folder.is_tree().then(|| {
                    let toggle = (entry.is_dir && !entry.is_parent).then(|| {
                        let (folder, location) = (self.folder.clone(), row.location.clone());
                        div()
                            .child(if expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                folder.update(cx, |folder, cx| {
                                    folder.toggle_expanded(&location);
                                    cx.notify();
                                });
                            })
                    });
                    div().w(px(14.)).flex_none().children(toggle)
                });
                let resolved = row
                    .location
                    .as_local_path()
                    .and_then(|path| self.metadata.read(cx).get(path, folder.generation()))
                    .and_then(metadata::MetadataValues::resolved_link)
                    .flatten();
                let tip = entry.link.tooltip_with_resolved(resolved);
                h_flex()
                    .id(("name", row_ix))
                    .items_center()
                    .gap_1p5()
                    .min_w_0()
                    .pl(px(16. * row.depth as f32))
                    .when_some(tip, |d, tip| {
                        d.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    })
                    .text_color(color)
                    .children(chevron)
                    .child(crate::link_badge::badged(Icon::new(icon), &entry.link, cx))
                    .child(
                        div()
                            .truncate()
                            .when(row.hit_line.is_some(), |d| d.flex_none())
                            // The extension has its own column when Ext is shown.
                            .child(if self.columns.contains(&ColumnId::from(ColumnKind::Ext)) {
                                entry.stem().to_string()
                            } else {
                                entry.name.clone()
                            }),
                    )
                    // Content search hit: the matching line after the name.
                    .when_some(row.hit_line.as_ref(), |d, (number, text)| {
                        d.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.muted_foreground)
                                .child(format!("{number}: {}", text.trim())),
                        )
                    })
                    .into_any_element()
            }
            ColumnKind::Ext => div()
                .text_color(color)
                .child(entry.ext().to_string())
                .into_any_element(),
            ColumnKind::Size => div()
                .w_full()
                .text_right()
                .text_color(color)
                .child(if entry.is_dir {
                    folder
                        .dir_size(&entry.path)
                        .map_or_else(|| "<DIR>".to_string(), |size| size.label())
                } else {
                    entry.size.map_or_else(|| "—".into(), fs_ops::format_size)
                })
                .into_any_element(),
            ColumnKind::Modified => div()
                .text_color(theme.muted_foreground)
                .when_some(entry.modified, |d, t| d.child(fs_ops::format_time(t)))
                .into_any_element(),
            ColumnKind::Permissions => div()
                .font_family(theme.mono_font_family.clone())
                .text_xs()
                .text_color(theme.muted_foreground)
                .when(!entry.is_parent, |d| {
                    d.child(entry.attrs.permissions(entry.is_dir))
                })
                .into_any_element(),
            ColumnKind::Owner => div().into_any_element(),
            ColumnKind::Created => div()
                .text_color(theme.muted_foreground)
                .when_some(entry.attrs.created.filter(|_| !entry.is_parent), |d, t| {
                    d.child(fs_ops::format_time(t))
                })
                .into_any_element(),
            ColumnKind::GitStatus => {
                let mark = self
                    .git
                    .as_ref()
                    .filter(|_| !entry.is_parent)
                    .and_then(|git| git.mark(&entry.path, entry.is_dir));
                let color = match mark {
                    Some(git_status::Mark::Ignored) => theme.muted_foreground,
                    Some(git_status::Mark::Untracked | git_status::Mark::Added) => theme.success,
                    Some(git_status::Mark::Conflicted | git_status::Mark::Deleted) => theme.danger,
                    _ => theme.warning,
                };
                div()
                    .font_family(theme.mono_font_family.clone())
                    .text_color(color)
                    .when_some(mark, |d, mark| d.child(mark.symbol()))
                    .into_any_element()
            }
            ColumnKind::Xattrs
            | ColumnKind::Selinux
            | ColumnKind::Exif(_)
            | ColumnKind::Duration => {
                unreachable!("handled above")
            }
        }
    }
}
