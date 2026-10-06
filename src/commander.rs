//! The window: two [`Folder`] models, each shown by a [`FolderView`], plus the
//! commands that act on them.

mod appearance;
mod archives;
mod files;
mod jobs;
mod keys;
mod location;
mod menus;
mod navigation;
mod panel_view;
mod search;
mod tools;
mod view;

pub use self::keys::{
    CopyLocation, EditLocation, GoTo, Open, Quit, SortByExt, SortByModified, SortByName,
    SortBySize, SortDescending, bind_keys, bind_user_keys,
};

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use self::appearance::set_font_size;
use self::jobs::RunningJob;
use self::keys::CONTEXT;
use self::tools::load_tools;
use crate::editor_view::EditorPane;
use crate::grid::ViewMode;
use crate::jobs::Yank;
use crate::menu_bar::MenuBar;
use crate::menus::Cmd;
use crate::model::Folder;
use crate::panel::{Activate, FolderView, HeaderEvent};
use crate::places::History;
use crate::preview::Registry as PreviewRegistry;
use crate::preview_view::PreviewPane;
use crate::settings::Settings;
use crate::tools::{Programs, Registry};
use crate::zoom;

pub struct Commander {
    folders: [Entity<Folder>; 2],
    views: [Entity<FolderView>; 2],
    /// Quick view shown in place of the inactive panel while enabled.
    preview: Entity<PreviewPane>,
    preview_on: bool,
    tools: Arc<Registry>,
    programs: Arc<Programs>,
    /// Planning or running a tool in the background.
    _tool_task: Task<()>,
    /// Saves the zoomed font size once zooming pauses.
    _zoom_save: Task<()>,
    openers: Vec<crate::openers::Opener>,
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
    navigation_generation: [u64; 2],
    navigation_pending: [bool; 2],
    _locations_task: Task<()>,
    locations_generation: u64,
    locations_pending: bool,
    bookmarks: Vec<PathBuf>,
    history: History,
    /// Last directory recorded in the history.
    last_visited: Option<PathBuf>,
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
    /// Open editor, shown in place of the inactive panel (before the preview).
    editor: Option<Entity<EditorPane>>,
    _editor_subscription: Option<Subscription>,
    /// The `theme` setting currently in effect.
    theme: String,
    active: usize,
    show_hidden: bool,
    panel_split: f32,
    dragging_split: bool,
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut status: SharedString =
            "Tab: switch panel · Enter: open · Backspace: up · Space/Insert: mark".into();
        set_font_size(cx, |_| zoom::clamp(settings.font_size));
        let panels = &settings.panels;
        let mut ix = 0;
        let folders = [left, right].map(|path| {
            // Tree mode does not apply to the icon grid: load once, without it.
            let tree = panels.tree && panels.view(ix) == ViewMode::List;
            ix += 1;
            let mut folder =
                Folder::new(path).with_options(settings.show_hidden, tree, panels.sort());
            if let Some(e) = folder.load_or_ancestor() {
                status = e.into();
            }
            cx.new(|_| folder)
        });
        let columns = crate::columns::normalize(&settings.panels.columns);
        let views = [0, 1].map(|ix| {
            let mode = panels.view(ix);
            cx.new(|cx| {
                FolderView::new(
                    folders[ix].clone(),
                    columns.clone(),
                    mode,
                    panels.icon_size,
                    window,
                    cx,
                )
            })
        });

        let registry = Arc::new(PreviewRegistry::with_defaults());
        let preview = cx.new(|cx| PreviewPane::new(registry, window, cx));

        let mut subscriptions = Vec::new();
        for folder in &folders {
            subscriptions.push(cx.observe_in(folder, window, |this, folder, window, cx| {
                this.update_preview(window, cx);
                this.record_visit(&folder, cx);
            }));
        }
        for (ix, view) in views.iter().enumerate() {
            let focus = view.read(cx).focus_handle(cx);
            subscriptions.push(cx.on_focus_in(&focus, window, move |this, _, cx| {
                this.set_active(ix, cx);
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
                move |_, _, event: &HeaderEvent, window, cx| {
                    let event = event.clone();
                    cx.defer_in(window, move |this, window, cx| match event {
                        HeaderEvent::Navigate(path) => this.navigate_panel(ix, path, window, cx),
                        HeaderEvent::Locations => this.open_locations(ix, true, window, cx),
                        HeaderEvent::SearchLocations => this.open_locations(ix, false, window, cx),
                        HeaderEvent::Edit => {
                            this.set_active(ix, cx);
                            this.views[ix].update(cx, |view, cx| view.open_location(window, cx));
                        }
                        HeaderEvent::CopyPath(path) => {
                            this.set_active(ix, cx);
                            cx.write_to_clipboard(ClipboardItem::new_string(path.clone()));
                            this.status = format!("Copied {path}").into();
                            cx.notify();
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
            folders,
            views,
            preview,
            preview_on: settings.preview.open_on_start,
            tools: Arc::new(tools_registry),
            programs: Arc::new(Programs::from_env()),
            _tool_task: Task::ready(()),
            _zoom_save: Task::ready(()),
            yank: None,
            yank_generation: 0,
            menu_bar: MenuBar::new(cx),
            menu_state: None,
            tools_menu_generation: 0,
            _tools_menu_task: Task::ready(()),
            navigation_tasks: [Task::ready(()), Task::ready(())],
            navigation_generation: [0, 0],
            navigation_pending: [false, false],
            _locations_task: Task::ready(()),
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
            editor: None,
            editor_command: settings.editor.command.clone(),
            terminal_command: settings.terminal.command.clone(),
            _editor_subscription: None,
            theme: settings.theme.clone(),
            active: 0,
            show_hidden: settings.show_hidden,
            panel_split: 0.5,
            dragging_split: false,
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
        for (ix, view) in self.views.iter().enumerate() {
            let commander = cx.weak_entity();
            let focus = view.read(cx).focus_handle(cx);
            view.update(cx, |view, cx| {
                view.set_context_menu(
                    move |menu, _, cx| {
                        let Some(commander) = commander.upgrade() else {
                            return menu;
                        };
                        commander.update(cx, |this, cx| {
                            this.set_active(ix, cx);
                            this.file_context_menu(menu.action_context(focus.clone()), cx)
                        })
                    },
                    cx,
                );
            });
        }
        self.update_preview(window, cx);
    }

    fn set_active(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.active = ix;
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

    fn reload_all(&mut self, cx: &mut Context<Self>) {
        for folder in &self.folders {
            if let Some(e) = folder.update(cx, |folder, cx| {
                cx.notify();
                folder.load_or_ancestor()
            }) {
                self.status = e.into();
            }
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.update_menu_bar(cx);
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
            .on_action(cx.listener(Self::filter))
            .on_action(cx.listener(Self::edit_file))
            .on_action(cx.listener(Self::new_file))
            .on_action(cx.listener(Self::select_theme))
            .on_action(cx.listener(Self::command_palette))
            .on_action(cx.listener(Self::help))
            .on_action(cx.listener(Self::tools_menu))
            .on_action(cx.listener(Self::find_files))
            .on_action(cx.listener(Self::cancel_jobs))
            .on_action(cx.listener(Self::rename))
            .on_action(cx.listener(Self::open_with))
            .on_action(cx.listener(Self::yank_files))
            .on_action(cx.listener(Self::cut_files))
            .on_action(cx.listener(Self::paste_files))
            .on_action(cx.listener(Self::go_to))
            .on_action(cx.listener(Self::edit_location))
            .on_action(cx.listener(Self::copy_location))
            .on_action(cx.listener(Self::add_bookmark))
            .on_action(cx.listener(Self::open_terminal))
            .on_action(cx.listener(Self::select_columns))
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
                    if dy != 0.0 {
                        let steps = if dy > 0.0 { 1 } else { -1 };
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
                    let win_width = f32::from(window.viewport_size().width);
                    if win_width > 0.0 {
                        let mouse_x = f32::from(event.position.x);
                        let ratio = (mouse_x / win_width).clamp(0.15, 0.85);
                        if (this.panel_split - ratio).abs() > 0.001 {
                            this.panel_split = ratio;
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
                }),
            )
            .size_full()
            .bg(bg)
            .text_color(fg)
            .text_sm()
            // AppMenuBar fills its parent, so give it a single row rather
            // than letting it claim the full window height.
            .child(
                div()
                    .w_full()
                    .h_8()
                    .flex_none()
                    .child(self.menu_bar.clone()),
            )
            .child({
                let left_w = DefiniteLength::Fraction(self.panel_split);
                let right_w = DefiniteLength::Fraction(1.0 - self.panel_split);

                let left_child =
                    if let Some(editor) = self.editor.as_ref().filter(|_| 0 != self.active) {
                        editor.clone().into_any_element()
                    } else if self.preview_on && 0 != self.active {
                        self.preview.clone().into_any_element()
                    } else {
                        self.views[0].clone().into_any_element()
                    };

                let right_child =
                    if let Some(editor) = self.editor.as_ref().filter(|_| 1 != self.active) {
                        editor.clone().into_any_element()
                    } else if self.preview_on && 1 != self.active {
                        self.preview.clone().into_any_element()
                    } else {
                        self.views[1].clone().into_any_element()
                    };

                let left_pane = div().w(left_w).min_w_0().h_full().child(left_child);
                let right_pane = div().w(right_w).min_w_0().h_full().child(right_child);

                let divider = div()
                    .id("panel-divider")
                    .w(px(6.))
                    .h_full()
                    .flex_none()
                    .cursor(CursorStyle::ResizeLeftRight)
                    .hover(|style| style.bg(theme.primary))
                    .when(self.dragging_split, |style| style.bg(theme.primary))
                    .when(!self.dragging_split, |style| style.bg(theme.border))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                            if event.click_count == 2 {
                                this.panel_split = 0.5;
                                this.dragging_split = false;
                            } else {
                                this.dragging_split = true;
                            }
                            cx.notify();
                        }),
                    );

                h_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_1()
                    .p_2()
                    .child(left_pane)
                    .child(divider)
                    .child(right_pane)
            })
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
