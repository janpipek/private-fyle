//! The window: two [`Folder`] models, each shown by a [`FolderView`], plus the
//! commands that act on them.

mod appearance;
mod drag_drop;
mod files;
#[cfg(feature = "sftp")]
mod hostkey_view;
mod jobs;
mod keymap_view;
mod keys;
mod location;
mod menus;
mod navigation;
mod open_with;
mod panel_view;
mod remotes;
mod remotes_view;
mod search;
mod tools;
mod view;
mod web;
mod workspace_view;

pub use self::keys::{
    ClearColumnFilter, CopyLocation, EditLocation, FilterColumn, GoTo, Open, Quit, RemoveColumn,
    SelectColumns, SortByExt, SortByModified, SortByName, SortBySize, SortDescending,
    ToggleBookmark, bind_keys, bind_user_keys, validate_user_keys,
};

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use self::appearance::set_font_size;
use self::jobs::RunningJob;
use self::keys::CONTEXT;
use self::tools::load_tools;
use crate::editor_view::EditorPane;
use crate::grid::ViewMode;
use crate::inspector_view::{InspectorEvent, InspectorPane};
use crate::jobs::Yank;
use crate::location::Location;
use crate::menu_bar::MenuBar;
use crate::menus::Cmd;
use crate::model::Folder;
use crate::navigation::PanelNavigation;
use crate::panel::{Activate, FolderView, HeaderEvent, PanelDrop, TreeExpansionChanged};
use crate::places::History;
use crate::preview::Registry as PreviewRegistry;
use crate::preview_view::{PreviewPane, PreviewZoom};
use crate::properties_view::PropertiesPane;
use crate::remote;
use crate::settings::Settings;
use crate::tools::{Programs, Registry};
use crate::workspace::{LayoutMode, PanelSide, Workspace};
use crate::zoom;

struct EditorWindow {
    path: PathBuf,
    window: AnyWindowHandle,
    pane: Entity<EditorPane>,
    _subscription: Subscription,
}

/// Immutable startup registries shared by panels and inspector panes.
pub struct ProviderRegistries {
    /// Built-in and enabled extension columns.
    pub columns: Arc<crate::columns::Catalog>,
    /// Shared local Git snapshot service for columns and Properties.
    pub git: crate::data_providers::git::Service,
    /// Built-in and enabled extension Properties providers.
    pub properties: Arc<crate::properties::Registry>,
    /// Built-in and enabled extension preview providers.
    pub preview: Arc<PreviewRegistry>,
}

pub struct Commander {
    /// Immutable column catalog shared by both panels and the appearance dialog.
    columns: Arc<crate::columns::Catalog>,
    git: crate::data_providers::git::Service,
    folders: [Entity<Folder>; 2],
    views: [Entity<FolderView>; 2],
    /// Content pane used by the inspector.
    preview: Entity<PreviewPane>,
    properties: Entity<PropertiesPane>,
    inspector: Entity<InspectorPane>,
    workspace: Workspace,
    layout_saves: crate::places::history_save::Queue<LayoutMode>,
    _layout_save_task: Task<()>,
    tools: Arc<Registry>,
    programs: Arc<Programs>,
    /// Planning or running a tool in the background.
    _tool_task: Task<()>,
    /// Saves the zoomed font size once zooming pauses.
    _zoom_save: Task<()>,
    /// The current history save and its coalesced latest snapshot.
    history_saves: crate::places::history_save::Queue<(PathBuf, History)>,
    _history_save_task: Task<()>,
    openers: Vec<crate::openers::Opener>,
    /// Applications the desktop associates with file types, read lazily.
    catalog_cache: Option<open_with::CatalogCache>,
    open_with_catalog_loading: bool,
    _open_with_task: Task<()>,
    editor_check: Option<open_with::EditorCheck>,
    _editor_check_task: Task<()>,
    open_with_menu: Option<Entity<PopupMenu>>,
    _open_with_dismiss: Option<Subscription>,
    /// Entries waiting to be pasted.
    yank: Option<Yank>,
    yank_generation: u64,
    menu_bar: Entity<MenuBar>,
    /// What the menu bar was last built for.
    menu_state: Option<crate::menus::State>,
    /// Rejects tool discovery that finishes after another Tools menu opened.
    tools_menu_generation: u64,
    _tools_menu_task: Task<()>,
    /// Newer requests replace and cancel older work for the same panel.
    navigation_tasks: [Task<()>; 2],
    /// Request freshness and pending destination, bound to each panel position.
    navigation: [PanelNavigation; 2],
    _locations_task: Task<()>,
    /// Configured remotes, discovered in the background.
    remotes: Arc<remote::Registry>,
    /// Per-panel bounded remote directory listings, shared by workers.
    listing_caches: [Arc<crate::listing::RemoteListingCache>; 2],
    remotes_generation: u64,
    remotes_pending: bool,
    /// The discovery problems last shown, so each is reported once.
    remote_errors: String,
    _remotes_task: Task<()>,
    /// Remote paths given at startup, opened once the remotes are known;
    /// the panel shows a local folder meanwhile.
    start_remote: [Option<PathBuf>; 2],
    /// Per panel: set to stop its remote listing.
    remote_cancel: [Option<Arc<AtomicBool>>; 2],
    /// Per panel: refreshing a remote listing in place.
    reload_generation: [u64; 2],
    reload_cancel: [Option<Arc<AtomicBool>>; 2],
    reload_tasks: [Task<()>; 2],
    /// Checking a transfer's destination for existing names.
    remote_check: Option<remotes::RemoteCheck>,
    /// A quick change on a remote (new folder, rename).
    _remote_op: Task<()>,
    remote_preview: remotes::RemotePreview,
    /// Last remote copy opened by a native opener; its source stays typed.
    remote_open_copy: Option<crate::remote::jobs::LocalCopy>,
    locations_generation: u64,
    locations_pending: bool,
    bookmarks: Vec<Location>,
    history: History,
    /// Last directory recorded in the history.
    last_visited: Option<crate::location::Location>,
    jobs: Vec<RunningJob>,
    next_job_id: usize,
    jobs_ticking: bool,
    /// Conflict dialog currently owned by `(job id, request id)`.
    active_conflict: Option<(usize, u64)>,
    _jobs_ticker: Task<()>,
    /// Find files: "Containing text" is a regular expression (remembered).
    search_regex: Rc<Cell<bool>>,
    /// Set to stop the running search.
    search_cancel: Option<Arc<AtomicBool>>,
    _search_task: Task<()>,
    /// Per panel: set to stop its running "Calculate Sizes" scan.
    size_cancel: [Option<Arc<AtomicBool>>; 2],
    _size_tasks: [Task<()>; 2],
    _search_inputs: Vec<Subscription>,
    /// External editor command; empty for the built-in editor.
    editor_command: Vec<String>,
    /// Configured terminal command; empty to detect one.
    terminal_command: Vec<String>,
    /// Shortcut overrides currently saved for the running session.
    shortcut_entries: Vec<crate::settings::KeymapEntry>,
    /// Config write for shortcut edits.
    _keymap_save_task: Task<()>,
    /// Config write for the custom title bar preference.
    _custom_titlebar_save_task: Task<()>,
    _columns_save_task: Task<()>,
    /// Whether custom title bars are requested at the next application start.
    custom_titlebar: bool,
    custom_titlebar_saving: bool,
    /// Built-in editor windows owned by this commander.
    editor_windows: Vec<EditorWindow>,
    /// The `theme` setting currently in effect.
    theme: String,
    active: usize,
    show_hidden: bool,
    panel_split: f32,
    dragging_split: bool,
    inspector_split: f32,
    dragging_inspector_split: bool,
    inspector_split_horizontal: bool,
    inspector_target_dirty: bool,
    /// Keep startup diagnostics visible over routine initial listing progress.
    has_startup_diagnostics: bool,
    status: SharedString,
    _subscriptions: Vec<Subscription>,
    _mkdir_subscription: Option<Subscription>,
    _pattern_subscription: Option<Subscription>,
}

impl Commander {
    /// `settings_error` is shown in the status line if loading settings failed.
    pub fn new(
        [left, right]: [PathBuf; 2],
        settings: &Settings,
        settings_error: Option<String>,
        registries: Arc<ProviderRegistries>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let has_startup_diagnostics = settings_error.is_some();
        let mut status: SharedString =
            "Tab: switch panel · Enter: open · Backspace: up · Space/Insert: mark".into();
        set_font_size(cx, |_| zoom::clamp(settings.font_size));
        let panels = &settings.panels;
        let mut ix = 0;
        let mut start_remote: [Option<PathBuf>; 2] = [None, None];
        let fallback = std::env::home_dir()
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| PathBuf::from("/"));
        let folders = [left, right].map(|path| {
            // Remote folders are listed in the background; see `init`.
            let path = if crate::location::is_remote(&path) {
                start_remote[ix] = Some(path);
                fallback.clone()
            } else {
                path
            };
            // Tree mode does not apply to the icon grid: load once, without it.
            let tree = panels.tree && panels.view(ix) == ViewMode::List;
            ix += 1;
            let mut folder = Folder::new(path.clone()).unwrap_or_else(|error| {
                status = format!("{}: {error}", path.display()).into();
                Folder::new(fallback.clone()).expect("fallback home or root is absolute")
            });
            folder = folder.with_options(settings.show_hidden, tree, panels.sort());
            cx.new(|_| folder)
        });
        let columns = crate::columns::normalize(&settings.panels.columns);
        let views = [0, 1].map(|ix| {
            let mode = panels.view(ix);
            cx.new(|cx| {
                FolderView::new(
                    folders[ix].clone(),
                    columns.clone(),
                    crate::panel::Providers {
                        columns: registries.columns.clone(),
                        git: registries.git.clone(),
                    },
                    mode,
                    panels.icon_size,
                    ix,
                    window,
                    cx,
                )
            })
        });

        let preview = cx.new(|cx| PreviewPane::new(registries.preview.clone(), window, cx));
        let mut workspace = Workspace::default();
        workspace.set_mode(settings.workspace.layout);
        let properties = cx.new(|_| PropertiesPane::with_registry(registries.properties.clone()));
        let inspector = cx.new(|_| {
            InspectorPane::new(workspace.perspective(), preview.clone(), properties.clone())
        });

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe(&preview, |this, _, event: &PreviewZoom, cx| {
            this.zoom(cx, |size| zoom::step(size, event.0));
        }));
        subscriptions.push(
            cx.subscribe(&inspector, |this, _, event: &InspectorEvent, cx| {
                this.workspace.set_perspective(event.0);
                this.inspector_target_dirty = true;
                cx.notify();
            }),
        );
        for folder in &folders {
            subscriptions.push(cx.observe_in(folder, window, |this, folder, window, cx| {
                this.update_preview(window, cx);
                this.warm_editor_check(window, cx);
                this.record_visit(&folder, cx);
            }));
        }
        for (ix, view) in views.iter().enumerate() {
            let focus = view.read(cx).focus_handle(cx);
            subscriptions.push(cx.on_focus_in(&focus, window, move |this, window, cx| {
                this.set_active(ix, cx);
                this.update_preview(window, cx);
            }));
            subscriptions.push(cx.subscribe_in(
                view,
                window,
                move |this, _, _: &Activate, window, cx| {
                    this.set_active(ix, cx);
                    this.open(&Open, window, cx);
                },
            ));
            subscriptions.push(cx.subscribe_in(
                view,
                window,
                move |this, _, _: &TreeExpansionChanged, window, cx| {
                    let target = this.folders[ix].read(cx).target();
                    this.load_panel_listing(
                        ix,
                        target,
                        crate::commander::location::ListingCommit::Options,
                        false,
                        window,
                        cx,
                    );
                },
            ));
            subscriptions.push(cx.subscribe_in(
                view,
                window,
                move |this, _, event: &PanelDrop, window, cx| {
                    this.handle_panel_drop(ix, event, window, cx);
                },
            ));
            subscriptions.push(cx.subscribe_in(
                view,
                window,
                move |_, _, event: &HeaderEvent, window, cx| {
                    let event = event.clone();
                    cx.defer_in(window, move |this, window, cx| match event {
                        HeaderEvent::Navigate(target) => {
                            this.navigate_panel(ix, target, window, cx)
                        }
                        HeaderEvent::Locations => this.open_locations(ix, true, window, cx),
                        HeaderEvent::SearchLocations => this.open_locations(ix, false, window, cx),
                        HeaderEvent::ManageRemotes => {
                            this.manage_remotes(&keys::ManageRemotes, window, cx)
                        }
                        HeaderEvent::Edit => {
                            this.set_active(ix, cx);
                            this.views[ix].update(cx, |view, cx| view.open_location(window, cx));
                        }
                        HeaderEvent::ToggleBookmark => {
                            this.set_active(ix, cx);
                            this.toggle_bookmark(&ToggleBookmark, window, cx);
                        }
                        HeaderEvent::CopyPath(path) => {
                            this.set_active(ix, cx);
                            cx.write_to_clipboard(ClipboardItem::new_string(path.clone()));
                            this.status = format!("Copied {path}").into();
                            cx.notify();
                        }
                        HeaderEvent::Zoom(steps) => {
                            this.zoom(cx, |size| zoom::step(size, steps));
                        }
                        HeaderEvent::Error(error) => {
                            this.status = error.into();
                            cx.notify();
                        }
                    });
                },
            ));
        }

        let (tools_registry, tools_error) = load_tools();
        if let Some(e) = tools_error {
            status = e.into();
        }
        let this = Self {
            columns: registries.columns.clone(),
            git: registries.git.clone(),
            folders,
            views,
            preview,
            properties,
            inspector,
            workspace,
            layout_saves: Default::default(),
            _layout_save_task: Task::ready(()),
            tools: Arc::new(tools_registry),
            programs: Arc::new(Programs::from_env()),
            catalog_cache: None,
            open_with_catalog_loading: false,
            _open_with_task: Task::ready(()),
            editor_check: None,
            _editor_check_task: Task::ready(()),
            open_with_menu: None,
            _open_with_dismiss: None,
            _tool_task: Task::ready(()),
            _zoom_save: Task::ready(()),
            history_saves: Default::default(),
            _history_save_task: Task::ready(()),
            yank: None,
            yank_generation: 0,
            menu_bar: MenuBar::new(cx),
            menu_state: None,
            tools_menu_generation: 0,
            _tools_menu_task: Task::ready(()),
            navigation_tasks: [Task::ready(()), Task::ready(())],
            navigation: [PanelNavigation::default(), PanelNavigation::default()],
            _locations_task: Task::ready(()),
            remotes: Arc::default(),
            listing_caches: [Arc::default(), Arc::default()],
            remotes_generation: 0,
            remotes_pending: false,
            remote_errors: String::new(),
            _remotes_task: Task::ready(()),
            start_remote,
            remote_cancel: [None, None],
            reload_generation: [0, 0],
            reload_cancel: [None, None],
            reload_tasks: [Task::ready(()), Task::ready(())],
            remote_check: None,
            _remote_op: Task::ready(()),
            remote_preview: Default::default(),
            remote_open_copy: None,
            locations_generation: 0,
            locations_pending: false,
            openers: settings.openers.clone(),
            bookmarks: settings.bookmarks.clone(),
            history: History::path()
                .map(|p| History::load(&p))
                .unwrap_or_default(),
            last_visited: None,
            jobs: Vec::new(),
            next_job_id: 0,
            jobs_ticking: false,
            active_conflict: None,
            _jobs_ticker: Task::ready(()),
            search_regex: Rc::default(),
            search_cancel: None,
            _search_task: Task::ready(()),
            size_cancel: [None, None],
            _size_tasks: [Task::ready(()), Task::ready(())],
            _search_inputs: Vec::new(),
            editor_windows: Vec::new(),
            editor_command: settings.editor.command.clone(),
            terminal_command: settings.terminal.command.clone(),
            shortcut_entries: settings.keymap.clone(),
            _keymap_save_task: Task::ready(()),
            _custom_titlebar_save_task: Task::ready(()),
            _columns_save_task: Task::ready(()),
            custom_titlebar: settings.window.custom_titlebar,
            custom_titlebar_saving: false,
            theme: settings.theme.clone(),
            active: 0,
            show_hidden: settings.show_hidden,
            panel_split: 0.5,
            dragging_split: false,
            inspector_split: 0.68,
            dragging_inspector_split: false,
            inspector_split_horizontal: true,
            inspector_target_dirty: true,
            has_startup_diagnostics,
            status: settings_error.map_or(status, Into::into),
            _subscriptions: subscriptions,
            _mkdir_subscription: None,
            _pattern_subscription: None,
        };
        this.views[0].update(cx, |view, cx| view.set_active(true, cx));
        this.focus_active(window, cx);
        this
    }

    /// Called once the entity exists (the preview needs `self`'s context).
    pub fn init(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.warm_open_with_catalog(window, cx);
        for (ix, view) in self.views.iter().enumerate() {
            let commander = cx.weak_entity();
            let focus = view.read(cx).focus_handle(cx);
            view.update(cx, |view, cx| {
                view.set_context_menu(
                    move |menu, window, popup_cx| {
                        let Some(commander_entity) = commander.upgrade() else {
                            return menu;
                        };
                        commander_entity.update(popup_cx, |this, cx| this.set_active(ix, cx));
                        let (state, open_with) = {
                            let this = commander_entity.read(popup_cx);
                            (
                                this.menu_state(popup_cx),
                                this.open_with_menu_data(popup_cx),
                            )
                        };
                        Commander::file_context_menu(
                            menu.action_context(focus.clone()),
                            window,
                            popup_cx,
                            state,
                            open_with,
                            commander.clone(),
                        )
                    },
                    cx,
                );
            });
        }
        self.update_preview(window, cx);
        for panel in 0..2 {
            if self.start_remote[panel].is_none() {
                let target = self.folders[panel].read(cx).target();
                self.load_panel_listing(
                    panel,
                    target,
                    location::ListingCommit::Startup,
                    true,
                    window,
                    cx,
                );
            }
        }
        self.discover_remotes(window, cx);
    }

    fn set_active(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.active = ix;
        self.inspector_target_dirty = true;
        self.workspace.set_active_panel(if ix == 0 {
            PanelSide::Left
        } else {
            PanelSide::Right
        });
        for (i, view) in self.views.iter().enumerate() {
            view.update(cx, |view, cx| view.set_active(i == ix, cx));
        }
        cx.notify();
    }

    fn focus_active(&self, window: &mut Window, cx: &mut App) {
        self.views[self.active]
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
    }

    /// Runs `f` on the active folder model and notifies its observers.
    fn update_folder<R>(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut Folder) -> R) -> R {
        self.folders[self.active].update(cx, |folder, cx| {
            let result = f(folder);
            cx.notify();
            result
        })
    }

    fn set_error(&mut self, result: Result<(), String>, cx: &mut Context<Self>) {
        if let Err(e) = result {
            self.status = e.into();
            cx.notify();
        }
    }

    fn reload_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for ix in 0..2 {
            let target = self.folders[ix].read(cx).target();
            self.load_panel_listing(
                ix,
                target,
                location::ListingCommit::Refresh,
                true,
                window,
                cx,
            );
        }
    }

    /// Archives are read-only: refuses (with a status message) any change
    /// to what the active panel shows when it is inside one.
    fn refuse_in_archive(&mut self, cx: &mut Context<Self>) -> bool {
        if self.folders[self.active].read(cx).archive().is_none() {
            return false;
        }
        self.status = "Archives are read-only; F5 extracts the selection".into();
        cx.notify();
        true
    }

    fn quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }
}

impl Render for Commander {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.warm_open_with_catalog(window, cx);
        self.warm_editor_check(window, cx);
        self.update_menu_bar(cx);
        self.sync_bookmark_stars(cx);
        let theme = cx.theme();
        let (bg, fg, muted) = (theme.background, theme.foreground, theme.muted_foreground);
        v_flex()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::go_up))
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::open_config))
            .on_action(cx.listener(Self::about))
            .on_action(cx.listener(Self::go_forward))
            .on_action(cx.listener(Self::go_first))
            .on_action(cx.listener(Self::go_last))
            .on_action(cx.listener(Self::switch_panel))
            .on_action(cx.listener(Self::toggle_mark))
            .on_action(cx.listener(Self::unmark))
            .on_action(cx.listener(Self::clear_selection))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::move_))
            .on_action(cx.listener(Self::extract_archive))
            .on_action(cx.listener(Self::make_dir))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_permanently))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::calculate_sizes))
            .on_action(cx.listener(Self::toggle_hidden))
            .on_action(cx.listener(Self::toggle_tree))
            .on_action(cx.listener(Self::toggle_preview))
            .on_action(cx.listener(Self::show_properties))
            .on_action(cx.listener(|this, _: &keys::LayoutTwoPanels, window, cx| {
                this.set_workspace_layout(LayoutMode::TwoPanels, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &keys::LayoutTwoPanelsInspector, window, cx| {
                    this.set_workspace_layout(LayoutMode::TwoPanelsInspector, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &keys::LayoutOnePanelInspector, window, cx| {
                    this.set_workspace_layout(LayoutMode::OnePanelInspector, window, cx)
                }),
            )
            .on_action(cx.listener(Self::filter))
            .on_action(cx.listener(Self::edit_file))
            .on_action(cx.listener(Self::new_file))
            .on_action(cx.listener(Self::select_theme))
            .on_action(cx.listener(Self::toggle_custom_titlebar))
            .on_action(cx.listener(Self::command_palette))
            .on_action(cx.listener(Self::help))
            .on_action(cx.listener(Self::tools_menu))
            .on_action(cx.listener(Self::find_files))
            .on_action(cx.listener(Self::cancel_jobs))
            .on_action(cx.listener(Self::rename))
            .on_action(cx.listener(Self::open_with))
            .on_action(cx.listener(Self::open_in_browser))
            .on_action(cx.listener(Self::yank_files))
            .on_action(cx.listener(Self::cut_files))
            .on_action(cx.listener(Self::paste_files))
            .on_action(cx.listener(Self::go_to))
            .on_action(cx.listener(Self::edit_location))
            .on_action(cx.listener(Self::copy_location))
            .on_action(cx.listener(Self::copy_full_path))
            .on_action(cx.listener(Self::copy_name))
            .on_action(cx.listener(Self::toggle_bookmark))
            .on_action(cx.listener(Self::go_to_other_folder))
            .on_action(cx.listener(Self::swap_panels))
            .on_action(cx.listener(Self::open_terminal))
            .on_action(cx.listener(Self::select_columns))
            .on_action(cx.listener(Self::remove_column))
            .on_action(cx.listener(Self::filter_column))
            .on_action(cx.listener(Self::clear_column_filter))
            .on_action(cx.listener(Self::clear_column_filters))
            .on_action(cx.listener(Self::edit_shortcuts))
            .on_action(cx.listener(Self::manage_remotes))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::invert_selection))
            .on_action(cx.listener(Self::select_pattern))
            .on_action(cx.listener(|this, _: &SortByName, _, cx| this.sort_by(Cmd::SortByName, cx)))
            .on_action(cx.listener(|this, _: &SortByExt, _, cx| this.sort_by(Cmd::SortByExt, cx)))
            .on_action(cx.listener(|this, _: &SortBySize, _, cx| this.sort_by(Cmd::SortBySize, cx)))
            .on_action(
                cx.listener(|this, _: &SortByModified, _, cx| {
                    this.sort_by(Cmd::SortByModified, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &SortDescending, _, cx| {
                    this.sort_by(Cmd::SortDescending, cx)
                }),
            )
            .on_action(cx.listener(Self::list_view))
            .on_action(cx.listener(Self::icon_view))
            .on_action(cx.listener(Self::larger_icons))
            .on_action(cx.listener(Self::smaller_icons))
            .on_action(cx.listener(Self::expand))
            .on_action(cx.listener(Self::collapse))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::zoom_reset))
            .on_action(cx.listener(Self::quit))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                // Ctrl-wheel zooms like Ctrl-+ / Ctrl--.
                if event.modifiers.control || event.modifiers.platform {
                    let dy = match event.delta {
                        ScrollDelta::Pixels(p) => f32::from(p.y),
                        ScrollDelta::Lines(l) => l.y,
                    };
                    if let Some(steps) = zoom::wheel_steps(dy) {
                        this.zoom(cx, |size| zoom::step(size, steps));
                    }
                }
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                if this.dragging_split {
                    if !event.dragging() {
                        this.dragging_split = false;
                        cx.notify();
                        return;
                    }
                    let dock_ratio = if this.workspace.mode().has_inspector()
                        && this.inspector_split_horizontal
                    {
                        this.inspector_split
                    } else {
                        1.0
                    };
                    let win_width = (f32::from(window.viewport_size().width) - 16.0) * dock_ratio;
                    if win_width > 0.0 {
                        let mouse_x = f32::from(event.position.x) - 8.0;
                        let ratio = (mouse_x / win_width).clamp(0.15, 0.85);
                        if (this.panel_split - ratio).abs() > 0.001 {
                            this.panel_split = ratio;
                            cx.notify();
                        }
                    }
                }
                if this.dragging_inspector_split {
                    if !event.dragging() {
                        this.dragging_inspector_split = false;
                        cx.notify();
                        return;
                    }
                    let viewport = window.viewport_size();
                    let (extent, position) = if this.inspector_split_horizontal {
                        (f32::from(viewport.width), f32::from(event.position.x))
                    } else {
                        (f32::from(viewport.height), f32::from(event.position.y))
                    };
                    if extent > 0.0 {
                        let scale = f32::from(cx.theme().font_size) / 16.0;
                        let mode = this.workspace.mode();
                        let count = if mode == LayoutMode::OnePanelInspector {
                            1.0
                        } else {
                            2.0
                        };
                        let (folder_min, inspector_min, available, origin) =
                            if this.inspector_split_horizontal {
                                (
                                    340.0 * count + 14.0 * (count - 1.0) + 14.0,
                                    300.0,
                                    (extent - 16.0).max(0.0),
                                    8.0,
                                )
                            } else {
                                (220.0 + 14.0, 180.0, (extent - 80.0).max(0.0), 40.0)
                            };
                        let minimum = folder_min * scale / available;
                        let maximum = 1.0 - inspector_min * scale / available;
                        let ratio = if minimum <= maximum {
                            ((position - origin) / available).clamp(minimum, maximum)
                        } else {
                            0.68
                        };
                        if (this.inspector_split - ratio).abs() > 0.001 {
                            this.inspector_split = ratio;
                            cx.notify();
                        }
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.dragging_split {
                        this.dragging_split = false;
                        cx.notify();
                    }
                    if this.dragging_inspector_split {
                        this.dragging_inspector_split = false;
                        cx.notify();
                    }
                }),
            )
            .size_full()
            .bg(bg)
            .text_color(fg)
            .text_sm()
            .when(
                crate::window_chrome_view::uses_custom_titlebar(window, cx),
                |root| {
                    #[cfg(any(windows, target_os = "linux"))]
                    let root = root.child(crate::window_chrome_view::menu_title_bar(
                        self.menu_bar.clone(),
                        cx,
                    ));
                    root
                },
            )
            // Native platforms and Linux window managers with server decorations keep this row.
            .when(
                !crate::window_chrome_view::uses_custom_titlebar(window, cx),
                |root| {
                    root.child(
                        div()
                            .w_full()
                            .h_8()
                            .flex_none()
                            .child(self.menu_bar.clone()),
                    )
                },
            )
            .when_some(self.open_with_menu.clone(), |root, menu| {
                root.child(deferred(
                    anchored()
                        .anchor(Anchor::TopLeft)
                        .snap_to_window_with_margin(px(8.))
                        .child(div().occlude().top_1().child(menu)),
                ))
            })
            .child(self.render_workspace(window, cx))
            .child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(muted)
                    .truncate()
                    .child(self.status.clone()),
            )
            .children(self.render_jobs(cx))
    }
}

impl Drop for Commander {
    fn drop(&mut self) {
        for job in &self.jobs {
            job.progress.cancel();
        }
    }
}
