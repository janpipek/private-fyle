//! Table view of a [`Folder`] model. Holds no file state of its own: it
//! renders the model and forwards cursor moves, sorting and activation.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::table::{
    Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableState,
};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::columns::ColumnKind;
use crate::fs_ops;
use crate::git_status;
use crate::icons::{self, FileIcon};
use crate::model::{Folder, Sort};
use gpui_kit::assets::IconName as Lucide;

/// Emitted when the user activates the cursor entry with the mouse.
pub struct Activate;

/// Requests from a panel header, carrying enough data for Commander to keep
/// the originating panel when the other panel was active.
#[derive(Clone, Debug)]
pub enum HeaderEvent {
    Navigate(PathBuf),
    Locations,
    SearchLocations,
    Edit,
    CopyPath(String),
    Error(String),
}

type ContextMenuBuilder = Box<dyn Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu>;

actions!(fyle_panel, [CloseFilter, CloseLocation]);

const FILTER_CONTEXT: &str = "FilterBar";
const LOCATION_CONTEXT: &str = "LocationBar";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", CloseFilter, Some(FILTER_CONTEXT)),
        KeyBinding::new("escape", CloseLocation, Some(LOCATION_CONTEXT)),
    ]);
}

pub struct FolderView {
    folder: Entity<Folder>,
    table: Entity<TableState<FileTable>>,
    active: bool,
    /// Filter-as-you-type input, while open.
    filter_input: Option<Entity<InputState>>,
    _filter_subscription: Option<Subscription>,
    /// Editable full path, while Ctrl-L or the edit button is active.
    location_input: Option<Entity<InputState>>,
    _location_subscription: Option<Subscription>,
    breadcrumb_scroll: ScrollHandle,
    /// Locations popup under the map button, while open.
    locations_menu: Option<Entity<PopupMenu>>,
    _locations_menu_dismiss: Option<Subscription>,
    /// Listing generation the Git column was last computed for.
    git_generation: Option<u64>,
    _git_task: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Activate> for FolderView {}
impl EventEmitter<HeaderEvent> for FolderView {}

impl FolderView {
    pub fn new(
        folder: Entity<Folder>,
        columns: Vec<ColumnKind>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let delegate = FileTable {
            folder: folder.clone(),
            columns,
            extra: HashMap::new(),
            exif: HashMap::new(),
            extra_generation: 0,
            git: None,
            context_menu: None,
            focus: None,
            context_row: Rc::new(Cell::new(None)),
        };
        let table = cx.new(|cx| {
            TableState::new(delegate, window, cx)
                .row_header(false)
                .col_selectable(false)
                .col_movable(false)
                .loop_selection(false)
        });
        let subscriptions = vec![
            cx.observe(&folder, |this, _, cx| {
                this.sync_cursor(cx);
                this.refresh_git(cx);
            }),
            cx.subscribe(&table, |this, _, event: &TableEvent, cx| match event {
                TableEvent::SelectRow(row) => this.set_cursor(*row, cx),
                TableEvent::DoubleClickedRow(row) => {
                    this.set_cursor(*row, cx);
                    cx.emit(Activate);
                }
                _ => {}
            }),
        ];
        let mut this = Self {
            folder,
            table,
            active: false,
            filter_input: None,
            _filter_subscription: None,
            location_input: None,
            _location_subscription: None,
            breadcrumb_scroll: ScrollHandle::new(),
            locations_menu: None,
            _locations_menu_dismiss: None,
            git_generation: None,
            _git_task: Task::ready(()),
            _subscriptions: subscriptions,
        };
        this.sync_cursor(cx);
        this.refresh_git(cx);
        this
    }

    /// Opens the locations menu under the map button; choosing a place asks
    /// Commander to navigate this panel.
    pub fn show_locations_menu(
        &mut self,
        groups: Vec<crate::places::Group>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pick = cx.weak_entity();
        let search = pick.clone();
        let menu = crate::location_view::locations_menu(
            groups,
            move |path, _, cx| {
                let _ = pick.update(cx, |_, cx| cx.emit(HeaderEvent::Navigate(path)));
            },
            move |_, cx| {
                let _ = search.update(cx, |_, cx| cx.emit(HeaderEvent::SearchLocations));
            },
            window,
            cx,
        );
        self._locations_menu_dismiss =
            Some(
                cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, _, cx| {
                    this.locations_menu = None;
                    this._locations_menu_dismiss = None;
                    cx.notify();
                }),
            );
        menu.focus_handle(cx).focus(window, cx);
        self.locations_menu = Some(menu);
        cx.notify();
    }

    pub fn columns(&self, cx: &App) -> Vec<ColumnKind> {
        self.table.read(cx).delegate().columns.clone()
    }

    /// Shows `columns` (Name first, see [`crate::columns::normalize`]).
    pub fn set_columns(&mut self, columns: Vec<ColumnKind>, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            table.delegate_mut().columns = columns;
            table.refresh(cx);
            cx.notify();
        });
        self.git_generation = None; // a newly shown Git column needs data
        self.refresh_git(cx);
        cx.notify();
    }

    /// With a Git column: re-runs `git status` in the background whenever
    /// the listing was read again.
    fn refresh_git(&mut self, cx: &mut Context<Self>) {
        if !self
            .table
            .read(cx)
            .delegate()
            .columns
            .contains(&ColumnKind::GitStatus)
        {
            return;
        }
        let folder = self.folder.read(cx);
        let generation = folder.generation();
        if self.git_generation == Some(generation) {
            return;
        }
        self.git_generation = Some(generation);
        let dir = folder.path().to_path_buf();
        let table = self.table.clone();
        self._git_task = cx.spawn(async move |_, cx| {
            let status = cx
                .background_spawn(async move { git_status::status(&dir) })
                .await;
            table.update(cx, |table, cx| {
                table.delegate_mut().git = status.map(Arc::new);
                cx.notify();
            });
        });
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.table.focus_handle(cx)
    }

    pub fn set_context_menu(
        &self,
        builder: impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + 'static,
        cx: &mut Context<Self>,
    ) {
        let focus = self.focus_handle(cx);
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            delegate.context_menu = Some(Box::new(builder));
            delegate.focus = Some(focus);
            cx.notify();
        });
    }

    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active != active {
            self.active = active;
            cx.notify();
        }
    }

    /// Opens the filter bar; typing narrows the listing. Enter keeps the
    /// filter and returns to the list, Esc clears it.
    pub fn open_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.folder.read(cx).filter().to_string();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Filter names…")
                .default_value(current)
        });
        self._filter_subscription = Some(cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let text = input.read(cx).value();
                    this.folder.update(cx, |folder, cx| {
                        folder.set_filter(&text);
                        cx.notify();
                    });
                }
                InputEvent::PressEnter { .. } => this.close_filter_bar(window, cx),
                _ => {}
            },
        ));
        input.update(cx, |input, cx| input.focus(window, cx));
        self.filter_input = Some(input);
        cx.notify();
    }

    /// Replaces breadcrumbs with an editable full path and focuses it.
    pub fn open_location(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = &self.location_input {
            input.update(cx, |input, cx| input.focus(window, cx));
            return;
        }
        let value = {
            let folder = self.folder.read(cx);
            crate::breadcrumbs::editable_path(folder.path())
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
        self._location_subscription =
            Some(
                cx.subscribe_in(&input, window, |this, input, event: &InputEvent, _, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        let text = input.read(cx).value().to_string();
                        let (current, archive) = {
                            let folder = this.folder.read(cx);
                            (
                                folder.path().to_path_buf(),
                                folder.archive().map(Path::to_path_buf),
                            )
                        };
                        match crate::breadcrumbs::parse_location(
                            &text,
                            &current,
                            dirs::home_dir().as_deref(),
                            archive.as_deref(),
                        ) {
                            Ok(path) => {
                                cx.emit(HeaderEvent::Navigate(path));
                            }
                            Err(error) => cx.emit(HeaderEvent::Error(format!("{text}: {error}"))),
                        }
                        cx.notify();
                    }
                }),
            );
        input.update(cx, |input, cx| input.focus(window, cx));
        self.location_input = Some(input);
        cx.notify();
        cx.on_next_frame(window, |_, window, cx| {
            if let Ok(key) = Keystroke::parse("secondary-a") {
                window.dispatch_keystroke(key, cx);
            }
        });
    }

    fn close_location(&mut self, _: &CloseLocation, window: &mut Window, cx: &mut Context<Self>) {
        self.location_input = None;
        self._location_subscription = None;
        self.table.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// Closes the editable location after its background navigation succeeds.
    pub fn finish_location_navigation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.location_input.is_some() {
            self.location_input = None;
            self._location_subscription = None;
            self.table.focus_handle(cx).focus(window, cx);
            cx.notify();
        }
    }

    fn close_filter(&mut self, _: &CloseFilter, window: &mut Window, cx: &mut Context<Self>) {
        self.folder.update(cx, |folder, cx| {
            folder.set_filter("");
            cx.notify();
        });
        self.close_filter_bar(window, cx);
    }

    fn close_filter_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_input = None;
        self._filter_subscription = None;
        self.table.focus_handle(cx).focus(window, cx);
        cx.notify();
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
        let header_foreground = if self.active {
            theme.primary_foreground
        } else {
            theme.foreground
        };
        let folder = self.folder.read(cx);
        let path_text = crate::breadcrumbs::editable_path(folder.path());
        let header_text = folder.header();
        let segments = crate::breadcrumbs::segments(
            folder.path(),
            folder.archive(),
            folder.inner_path().as_deref(),
        );
        let mut footer = format!("{} dirs, {} files", folder.dir_count(), folder.file_count());
        if folder.is_loading() {
            footer = "Reading archive… · Esc cancels".into();
        }
        if let Some(title) = folder.results() {
            footer = format!(
                "{title}: {} found · Enter shows a hit in its folder",
                folder.unfiltered_count()
            );
        }
        if !folder.filter().is_empty() {
            let shown = folder.dir_count() + folder.file_count();
            footer = format!(
                "filter \"{}\": {shown} of {} · Esc clears",
                folder.filter(),
                folder.unfiltered_count()
            );
        }
        if folder.marked_count() > 0 {
            footer += &format!(
                " · {} marked, {}",
                folder.marked_count(),
                folder.marked_total().label()
            );
        }
        let view = cx.weak_entity();
        let mut breadcrumb_items = Vec::new();
        let segment_count = segments.len();
        for (index, segment) in segments.into_iter().enumerate() {
            let path = segment.path;
            let label = match segment.kind {
                crate::breadcrumbs::Kind::Archive => format!("{} [archive]", segment.label),
                crate::breadcrumbs::Kind::ArchiveInner => format!("↳ {}", segment.label),
                crate::breadcrumbs::Kind::Normal => segment.label,
            };
            let view = view.clone();
            breadcrumb_items.push(
                Button::new(("breadcrumb", index))
                    .label(label)
                    .ghost()
                    .compact()
                    .small()
                    .flex_none()
                    .text_color(header_foreground)
                    .accessibility_label(format!("Open {}", path.display()))
                    .on_click(move |_, _, cx| {
                        if let Some(view) = view.upgrade() {
                            view.update(cx, |_, cx| cx.emit(HeaderEvent::Navigate(path.clone())));
                        }
                    })
                    .into_any_element(),
            );
            if index + 1 < segment_count {
                breadcrumb_items.push(
                    Icon::new(IconName::ChevronRight)
                        .size_3()
                        .text_color(header_foreground)
                        .into_any_element(),
                );
            }
        }
        let locations_view = view.clone();
        let edit_view = view.clone();
        let copy_view = view;
        self.breadcrumb_scroll
            .scroll_to_item(breadcrumb_items.len().saturating_sub(1));
        let breadcrumb_scroll = self.breadcrumb_scroll.clone();

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
                            .relative()
                            .flex_none()
                            .child(
                                Button::new("panel-locations")
                                    .icon(Lucide::Map)
                                    .ghost()
                                    .compact()
                                    .small()
                                    .flex_none()
                                    .text_color(header_foreground)
                                    .tooltip_with_action(
                                        "Locations for this panel",
                                        &crate::commander::GoTo,
                                        Some("Commander > DataTable"),
                                    )
                                    .accessibility_label("Open locations for this panel")
                                    .on_click(move |_, _, cx| {
                                        if let Some(view) = locations_view.upgrade() {
                                            view.update(cx, |_, cx| {
                                                cx.emit(HeaderEvent::Locations)
                                            });
                                        }
                                    }),
                            )
                            .when_some(self.locations_menu.clone(), |d, menu| {
                                d.child(deferred(
                                    anchored()
                                        .anchor(Anchor::TopLeft)
                                        .snap_to_window_with_margin(px(8.))
                                        .child(div().occlude().top_1().child(menu)),
                                ))
                            }),
                    )
                    .child(
                        div()
                            .id("breadcrumb-path")
                            .flex_1()
                            .min_w_0()
                            .overflow_x_scroll()
                            .track_scroll(&breadcrumb_scroll)
                            .when_some(self.location_input.clone(), |d, input| {
                                d.key_context(LOCATION_CONTEXT)
                                    .on_action(cx.listener(Self::close_location))
                                    .child(Input::new(&input).small())
                            })
                            .when(self.location_input.is_none(), |d| {
                                d.flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_0p5()
                                    .children(breadcrumb_items)
                            }),
                    )
                    .child(
                        Button::new("panel-edit-location")
                            .icon(Lucide::SquareTerminal)
                            .ghost()
                            .compact()
                            .small()
                            .flex_none()
                            .text_color(header_foreground)
                            .tooltip_with_action(
                                "Type or paste a location",
                                &crate::commander::EditLocation,
                                Some("Commander > DataTable"),
                            )
                            .accessibility_label("Edit full path")
                            .on_click(move |_, _, cx| {
                                if let Some(view) = edit_view.upgrade() {
                                    view.update(cx, |_, cx| cx.emit(HeaderEvent::Edit));
                                }
                            }),
                    )
                    .child(
                        Button::new("panel-copy-location")
                            .icon(Lucide::Copy)
                            .ghost()
                            .compact()
                            .small()
                            .flex_none()
                            .text_color(header_foreground)
                            .tooltip_with_action(
                                format!("Copy full path: {header_text}"),
                                &crate::commander::CopyLocation,
                                Some("Commander > DataTable"),
                            )
                            .accessibility_label("Copy full path")
                            .on_click(move |_, _, cx| {
                                if let Some(view) = copy_view.upgrade() {
                                    let text = path_text.clone();
                                    view.update(cx, |_, cx| cx.emit(HeaderEvent::CopyPath(text)));
                                }
                            }),
                    )
                    .when(folder.results().is_some(), |d| {
                        d.child(div().flex_none().text_xs().child("RESULTS"))
                    })
                    .when(folder.is_tree() && folder.results().is_none(), |d| {
                        d.child(div().flex_none().text_xs().child("TREE"))
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(DataTable::new(&self.table).with_size(theme.font_size * 1.85)),
            )
            .when_some(self.filter_input.clone(), |d, input| {
                d.child(
                    h_flex()
                        .key_context(FILTER_CONTEXT)
                        .on_action(cx.listener(Self::close_filter))
                        .gap_2()
                        .px_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("Filter"),
                        )
                        .child(div().flex_1().child(Input::new(&input).small())),
                )
            })
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
    columns: Vec<ColumnKind>,
    /// Cells that need extra system calls or file reads (xattrs, EXIF),
    /// read lazily for the rows shown and kept until the listing is read
    /// again.
    extra: HashMap<(PathBuf, ColumnKind), SharedString>,
    /// All EXIF columns of a file, read once for all of them.
    exif: HashMap<PathBuf, HashMap<&'static str, String>>,
    extra_generation: u64,
    git: Option<Arc<git_status::Status>>,
    context_menu: Option<ContextMenuBuilder>,
    focus: Option<FocusHandle>,
    context_row: Rc<Cell<Option<usize>>>,
}

impl FileTable {
    fn extra(
        &mut self,
        path: &Path,
        is_dir: bool,
        kind: ColumnKind,
        generation: u64,
    ) -> SharedString {
        if self.extra_generation != generation {
            self.extra.clear();
            self.exif.clear();
            self.extra_generation = generation;
        }
        if let Some(text) = self.extra.get(&(path.to_path_buf(), kind)) {
            return text.clone();
        }
        let text: SharedString = match kind {
            ColumnKind::Xattrs => crate::xattr::names(path).join(", ").into(),
            ColumnKind::Selinux => crate::xattr::selinux_context(path)
                .unwrap_or_default()
                .into(),
            ColumnKind::Exif(field) if !is_dir => self
                .exif
                .entry(path.to_path_buf())
                .or_insert_with(|| crate::preview::exif_fields(path))
                .get(field.key())
                .cloned()
                .unwrap_or_default()
                .into(),
            _ => SharedString::default(),
        };
        self.extra.insert((path.to_path_buf(), kind), text.clone());
        text
    }
}

impl TableDelegate for FileTable {
    fn render_tr(
        &mut self,
        row_ix: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let context_row = self.context_row.clone();
        div()
            .id(("row", row_ix))
            .when_some(self.focus.clone(), |row, focus| {
                // Focus before the table's context menu captures where to return
                // on dismissal, including when the other panel was active.
                row.capture_any_mouse_down(move |event, window, cx| {
                    if event.button == MouseButton::Right {
                        context_row.set(Some(row_ix));
                        focus.focus(window, cx);
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
        self.folder.update(cx, |folder, cx| {
            folder.select_context_row(row_ix);
            cx.notify();
        });
        match &self.context_menu {
            Some(build) => build(menu, window, cx),
            None => menu,
        }
    }

    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, cx: &App) -> usize {
        self.folder.read(cx).entries().len()
    }

    fn column(&self, col_ix: usize, cx: &App) -> Column {
        let kind = self.columns[col_ix];
        let mut column = Column::new(kind.id(), kind.title()).width(px(kind.width()));
        if let Some(key) = kind.sort_key() {
            let sort = self.folder.read(cx).sort();
            column = column.sort(match sort.key == key {
                true if sort.descending => ColumnSort::Descending,
                true => ColumnSort::Ascending,
                false => ColumnSort::Default,
            });
        }
        if kind == ColumnKind::Size {
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
        let Some(key) = self.columns[col_ix].sort_key() else {
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
        let kind = self.columns[col_ix];
        if matches!(
            kind,
            ColumnKind::Xattrs | ColumnKind::Selinux | ColumnKind::Exif(_)
        ) {
            let folder = self.folder.read(cx);
            let Some(row) = folder.entries().get(row_ix).filter(|r| !r.is_parent) else {
                return div().into_any_element();
            };
            let (path, is_dir, generation) = (row.path.clone(), row.is_dir, folder.generation());
            let text = self.extra(&path, is_dir, kind, generation);
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
        let folder = self.folder.read(cx);
        let Some(row) = folder.entries().get(row_ix) else {
            return div().into_any_element();
        };
        let entry = &row.entry;
        let theme = cx.theme();
        let color = if folder.is_marked(&entry.path) {
            theme.danger
        } else if entry.link.is_broken() {
            theme.muted_foreground
        } else if entry.is_dir {
            theme.primary
        } else {
            theme.foreground
        };
        match self.columns[col_ix] {
            ColumnKind::Name => {
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
                let tip = entry.link.tooltip(&entry.path);
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
                            .child(if self.columns.contains(&ColumnKind::Ext) {
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
                    fs_ops::format_size(entry.size)
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
            ColumnKind::Owner => div()
                .text_color(theme.muted_foreground)
                .when(!entry.is_parent, |d| d.child(entry.attrs.owner()))
                .into_any_element(),
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
            ColumnKind::Xattrs | ColumnKind::Selinux | ColumnKind::Exif(_) => {
                unreachable!("handled above")
            }
        }
    }
}

/// The glyph drawing each kind of [`FileIcon`]. Only icons from GPUI Kit's
/// default bundle are used (see the test), so no icon assets are vendored.
fn glyph(icon: FileIcon) -> Lucide {
    match icon {
        FileIcon::ParentDir => Lucide::ArrowUp,
        FileIcon::Folder => Lucide::Folder,
        FileIcon::FolderOpen => Lucide::FolderOpen,
        FileIcon::FolderGit => Lucide::Github,
        FileIcon::Code | FileIcon::Script => Lucide::SquareTerminal,
        FileIcon::Config => Lucide::Settings,
        FileIcon::Data | FileIcon::Text => Lucide::FileText,
        FileIcon::Image => Lucide::Palette,
        FileIcon::Audio | FileIcon::Video => Lucide::Play,
        FileIcon::Archive => Lucide::Inbox,
        FileIcon::Spreadsheet => Lucide::ChartPie,
        FileIcon::Font => Lucide::CaseSensitive,
        FileIcon::Diff => Lucide::Replace,
        FileIcon::File | FileIcon::Lock | FileIcon::Key => Lucide::File,
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: gpui's glob export would shadow `#[test]`.
    use super::{FileIcon, Lucide, glyph};
    use gpui_kit::AssetSource as _;

    #[test]
    fn every_glyph_is_in_the_default_bundle() {
        for icon in FileIcon::ALL {
            let path = glyph(icon).path();
            let svg = gpui_kit::assets::Assets.load(&path).unwrap();
            assert!(svg.is_some(), "{icon:?}: {path} is not bundled");
        }
        for icon in [
            Lucide::Redo2,
            Lucide::Ban,
            Lucide::Map,
            Lucide::SquareTerminal,
            Lucide::Copy,
            Lucide::ChevronRight,
        ] {
            let path = icon.path();
            let svg = gpui_kit::assets::Assets.load(&path).unwrap();
            assert!(svg.is_some(), "header icon {icon:?}: {path} is not bundled");
        }
    }
}
