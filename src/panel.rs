//! Shared folder-panel shell: headers, inputs, focus and list/icon view coordination.

use std::cell::Cell;
use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::table::{DataTable, TableEvent, TableState};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::columns::ColumnKind;
use crate::context_menu::ContextMenuBuilder;
use crate::git_status;
use crate::grid::ViewMode;
use crate::icon_view::{GridEvent, IconGrid};
use crate::location::PanelTarget;
use crate::model::{DragSnapshot, Folder};

/// Breadcrumb, location-bar and header-button rendering.
pub mod header_view;
pub mod metadata;
/// List-table rendering and forwarding to the folder model.
pub mod table_view;
pub mod wheel_view;

use table_view::FileTable;
use wheel_view::ControlWheelCapture;

/// Emitted when the user activates the cursor entry with the mouse.
pub struct Activate;

/// Emitted when a tree chevron changes which folders should be listed.
pub struct TreeExpansionChanged;

/// A drag started on a row, with its source identity held across focus changes.
#[derive(Clone)]
pub struct PanelDrag {
    pub folder: Entity<Folder>,
    pub snapshot: DragSnapshot,
}

/// A drop received by a panel; the Commander chooses and runs the operation.
pub enum PanelDrop {
    Internal(PanelDrag, Option<usize>),
    External(Vec<std::path::PathBuf>, Option<usize>),
}

/// A small visual label following a panel drag.
pub struct DragGhost {
    label: String,
}

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .p_2()
            .rounded(cx.theme().radius)
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .child(self.label.clone())
    }
}

/// Builds the drag label without reading file metadata on the UI thread.
pub fn drag_ghost(drag: &PanelDrag, cx: &mut App) -> Entity<DragGhost> {
    let label = match drag.snapshot.items.as_slice() {
        [one] => one
            .location
            .panel_path()
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        many => format!("{} items", many.len()),
    };
    cx.new(|_| DragGhost { label })
}

/// Offers local entries to the desktop as native file paths on drag out.
pub fn native_drag_payload(drag: &PanelDrag) -> Option<ExternalDragPayload> {
    Some(ExternalDragPayload::Files(FileDragPaths::new(
        drag.snapshot.native_paths()?,
    )))
}

/// Requests from a panel header, carrying enough data for Commander to keep
/// the originating panel when the other panel was active.
#[derive(Clone, Debug)]
pub enum HeaderEvent {
    Navigate(PanelTarget),
    Locations,
    SearchLocations,
    Edit,
    CopyPath(String),
    /// The bookmark star was clicked.
    ToggleBookmark,
    /// Request a global font zoom step from the Commander.
    Zoom(i32),
    Error(String),
}

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
    /// The icon grid, shown instead of the table in icon view. It shares the
    /// table's focus handle, so the panel's key context and focus stay put
    /// when switching.
    grid: Entity<IconGrid>,
    mode: ViewMode,
    active: bool,
    /// Whether this panel's folder is bookmarked; `None` when it cannot be.
    bookmark: Option<bool>,
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
    expanded_snapshot: HashSet<crate::location::Location>,
    drop_row: Rc<Cell<Option<usize>>>,
    _git_task: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Activate> for FolderView {}
impl EventEmitter<HeaderEvent> for FolderView {}
impl EventEmitter<TreeExpansionChanged> for FolderView {}
impl EventEmitter<PanelDrop> for FolderView {}

impl FolderView {
    pub fn new(
        folder: Entity<Folder>,
        columns: Vec<ColumnKind>,
        mode: ViewMode,
        icon_size: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let metadata = cx.new(|_| metadata::MetadataState::new());
        let drop_row = Rc::new(Cell::new(None));
        let delegate = FileTable::new(folder.clone(), columns, metadata.clone(), drop_row.clone());
        let table = cx.new(|cx| {
            TableState::new(delegate, window, cx)
                .row_header(false)
                .col_selectable(false)
                .col_movable(false)
                .loop_selection(false)
        });
        let focus = table.focus_handle(cx);
        let grid = cx.new(|cx| {
            IconGrid::new(
                folder.clone(),
                focus,
                metadata.clone(),
                drop_row.clone(),
                icon_size,
                cx,
            )
        });
        let subscriptions = vec![
            cx.subscribe(&grid, |this, _, event: &GridEvent, cx| match event {
                GridEvent::Select(ix) => this.set_cursor(*ix, cx),
                GridEvent::Activate(ix) => {
                    this.set_cursor(*ix, cx);
                    cx.emit(Activate);
                }
            }),
            cx.observe(&folder, |this, folder, cx| {
                let expanded = folder.read(cx).expanded_locations();
                if expanded != this.expanded_snapshot {
                    this.expanded_snapshot = expanded;
                    cx.emit(TreeExpansionChanged);
                }
                this.sync_cursor(cx);
                this.refresh_git(cx);
            }),
            cx.observe(&metadata, |this, _, cx| {
                this.table.update(cx, |table, cx| table.refresh(cx));
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
        let expanded_snapshot = folder.read(cx).expanded_locations();
        let mut this = Self {
            folder,
            table,
            grid,
            mode,
            active: false,
            bookmark: None,
            filter_input: None,
            _filter_subscription: None,
            location_input: None,
            _location_subscription: None,
            breadcrumb_scroll: ScrollHandle::new(),
            locations_menu: None,
            _locations_menu_dismiss: None,
            git_generation: None,
            expanded_snapshot,
            drop_row,
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
                if let Ok(target) = PanelTarget::from_panel_path(&path, None) {
                    let _ = pick.update(cx, |_, cx| cx.emit(HeaderEvent::Navigate(target)));
                }
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
        self.table.read(cx).delegate().columns().to_vec()
    }

    /// Shows `columns` (Name first, see [`crate::columns::normalize`]).
    pub fn set_columns(&mut self, columns: Vec<ColumnKind>, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            table.delegate_mut().set_columns(columns);
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
            .columns()
            .contains(&ColumnKind::GitStatus)
        {
            return;
        }
        let folder = self.folder.read(cx);
        let generation = folder.generation();
        // `git status` would run in whatever local directory `proton:/`
        // happens to resolve to.
        if folder.is_remote() || self.git_generation == Some(generation) {
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
                table.delegate_mut().set_git_status(status);
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
        let builder: ContextMenuBuilder = Rc::new(builder);
        self.table.update(cx, |table, cx| {
            table
                .delegate_mut()
                .set_context_menu(builder.clone(), focus);
            cx.notify();
        });
        self.grid
            .update(cx, |grid, _| grid.set_context_menu(builder));
    }

    pub fn view_mode(&self) -> ViewMode {
        self.mode
    }

    pub fn set_view_mode(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        if self.mode != mode {
            self.mode = mode;
            cx.notify();
        }
    }

    pub fn icon_size(&self, cx: &App) -> u32 {
        self.grid.read(cx).icon_size()
    }

    pub fn set_icon_size(&mut self, size: u32, cx: &mut Context<Self>) {
        self.grid
            .update(cx, |grid, cx| grid.set_icon_size(size, cx));
    }

    /// Sets the bookmark star: `None` hides it, `Some(on)` shows its state.
    pub fn set_bookmark(&mut self, bookmark: Option<bool>, cx: &mut Context<Self>) {
        if self.bookmark != bookmark {
            self.bookmark = bookmark;
            cx.notify();
        }
    }

    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active != active {
            self.active = active;
            self.grid.update(cx, |grid, cx| grid.set_active(active, cx));
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
            crate::breadcrumbs::editable_path(&folder.path())
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
        self._location_subscription = Some(cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, _, cx| {
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
                        Ok(path) => match PanelTarget::from_panel_path(&path, archive.as_deref()) {
                            Ok(target) => cx.emit(HeaderEvent::Navigate(target)),
                            Err(error) => cx.emit(HeaderEvent::Error(format!("{text}: {error}"))),
                        },
                        Err(error) => cx.emit(HeaderEvent::Error(format!("{text}: {error}"))),
                    }
                    cx.notify();
                }
            },
        ));
        input.update(cx, |input, cx| input.focus(window, cx));
        let focus = input.focus_handle(cx);
        self.location_input = Some(input);
        cx.notify();
        // Dispatch may redraw the window, so do not hold FolderView's entity
        // lease via Context::on_next_frame while selecting the input text.
        window.on_next_frame(move |window, cx| {
            if focus.is_focused(window)
                && let Ok(key) = Keystroke::parse("secondary-a")
            {
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

    /// Closes the filter bar and the location editor, which belong to the
    /// content this panel no longer shows after a swap.
    pub fn close_inputs(&mut self, cx: &mut Context<Self>) {
        self.filter_input = None;
        self._filter_subscription = None;
        self.location_input = None;
        self._location_subscription = None;
        cx.notify();
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.render_header(window, cx);
        let theme = cx.theme();
        let folder = self.folder.read(cx);
        let mut footer = format!("{} dirs, {} files", folder.dir_count(), folder.file_count());
        if folder.is_loading() {
            footer = if folder.is_remote() {
                "Loading… · Esc cancels".into()
            } else {
                "Reading archive… · Esc cancels".into()
            };
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
        let internal_drop_view = cx.weak_entity();
        let external_drop_view = cx.weak_entity();
        let internal_drop_row = self.drop_row.clone();
        let external_drop_row = self.drop_row.clone();
        v_flex()
            .size_full()
            .gap_1()
            .child(header)
            .child(
                div().flex_1().min_h_0().child(ControlWheelCapture::new(
                    div().size_full().child(match self.mode {
                        ViewMode::List => DataTable::new(&self.table)
                            .with_size(theme.font_size * 1.85)
                            .into_any_element(),
                        ViewMode::Icons => self.grid.clone().into_any_element(),
                    }),
                    cx.entity().downgrade(),
                )),
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
            .drag_over::<PanelDrag>(|panel, _, _, cx| {
                panel.border_2().border_color(cx.theme().drag_border)
            })
            .drag_over::<ExternalPaths>(|panel, _, _, cx| {
                panel.border_2().border_color(cx.theme().drag_border)
            })
            .on_drop(move |drag: &PanelDrag, _, cx| {
                let row = internal_drop_row.get();
                let _ = internal_drop_view
                    .update(cx, |_, cx| cx.emit(PanelDrop::Internal(drag.clone(), row)));
            })
            .on_drop(move |paths: &ExternalPaths, _, cx| {
                let paths = paths.paths().to_vec();
                let row = external_drop_row.get();
                let _ =
                    external_drop_view.update(cx, |_, cx| cx.emit(PanelDrop::External(paths, row)));
            })
    }
}
