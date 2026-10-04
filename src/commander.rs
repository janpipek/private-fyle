//! The window: two [`Folder`] models, each shown by a [`FolderView`], plus the
//! commands that act on them.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::list::{List, ListState};
use gpui_kit::component::progress::Progress as ProgressBar;
use gpui_kit::component::{
    ActiveTheme as _, IndexPath, Sizable as _, ThemeRegistry, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::editor::Document;
use crate::editor_view::{EditorEvent, EditorPane};
use crate::fs_ops;
use crate::jobs::{self, JobKind, Progress};
use crate::model::{Activation, Folder};
use crate::palette::{Command, Palette};
use crate::palette_view::PaletteDelegate;
use crate::panel::{Activate, FolderView};
use crate::places::{self, History};
use crate::preview::Registry as PreviewRegistry;
use crate::preview_view::PreviewPane;
use crate::rename;
use crate::search::{self, Query};
use crate::settings::{self, Settings};
use crate::theme_picker::{self, ThemePickerDelegate};
use crate::themes::ThemePicker;
use crate::tools::{
    self, Action as ToolAction, Offer, ParamKind, Programs, Registry, Target, ToolContext, Values,
};
use crate::tools_view::{MenuDelegate, PickDelegate};

actions!(
    fyle,
    [
        Open,
        GoUp,
        GoFirst,
        GoLast,
        SwitchPanel,
        ToggleMark,
        Unmark,
        Copy,
        Move,
        MakeDir,
        Delete,
        DeletePermanently,
        Refresh,
        ToggleHidden,
        ToggleTree,
        TogglePreview,
        Filter,
        EditFile,
        SelectTheme,
        Rename,
        GoTo,
        AddBookmark,
        CancelJobs,
        FindFiles,
        ToolsMenu,
        CommandPalette,
        Expand,
        Collapse,
        Quit,
    ]
);

const CONTEXT: &str = "Commander";
/// Overrides for keys the DataTable would otherwise use for column navigation.
const TABLE_CONTEXT: &str = "Commander > DataTable";

pub fn bind_keys(cx: &mut App) {
    // Window-wide keys; panel keys are bound in the table context so they do
    // not fire while typing in the editor.
    let ctx = Some(CONTEXT);
    let table = Some(TABLE_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("enter", Open, table),
        KeyBinding::new("backspace", GoUp, table),
        KeyBinding::new("tab", SwitchPanel, table),
        KeyBinding::new("shift-tab", SwitchPanel, table),
        KeyBinding::new("home", GoFirst, table),
        KeyBinding::new("end", GoLast, table),
        KeyBinding::new("left", Collapse, table),
        KeyBinding::new("right", Expand, table),
        KeyBinding::new("escape", Unmark, table),
        KeyBinding::new("insert", ToggleMark, table),
        KeyBinding::new("space", ToggleMark, table),
        KeyBinding::new("f2", ToolsMenu, table),
        KeyBinding::new("shift-f6", Rename, table),
        KeyBinding::new("ctrl-g", GoTo, table),
        KeyBinding::new("alt-f1", GoTo, table),
        KeyBinding::new("ctrl-d", AddBookmark, table),
        KeyBinding::new("alt-f7", FindFiles, table),
        KeyBinding::new("secondary-shift-f", FindFiles, table),
        KeyBinding::new("f3", TogglePreview, ctx),
        KeyBinding::new("/", Filter, table),
        KeyBinding::new("ctrl-f", Filter, table),
        KeyBinding::new("f4", EditFile, table),
        KeyBinding::new("ctrl-k ctrl-t", SelectTheme, ctx),
        KeyBinding::new("secondary-p", CommandPalette, ctx),
        KeyBinding::new("secondary-shift-p", CommandPalette, ctx),
        KeyBinding::new("f5", Copy, table),
        KeyBinding::new("f6", Move, table),
        KeyBinding::new("f7", MakeDir, table),
        KeyBinding::new("f8", Delete, table),
        KeyBinding::new("delete", Delete, table),
        KeyBinding::new("shift-f8", DeletePermanently, table),
        KeyBinding::new("shift-delete", DeletePermanently, table),
        KeyBinding::new("secondary-r", Refresh, table),
        KeyBinding::new("ctrl-h", ToggleHidden, table),
        KeyBinding::new("ctrl-t", ToggleTree, table),
        KeyBinding::new("f10", Quit, ctx),
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("ctrl-shift-c", CancelJobs, ctx),
    ]);
}

struct RunningJob {
    id: usize,
    kind: JobKind,
    progress: Arc<Progress>,
    _task: Task<()>,
}

#[derive(Clone, Copy, PartialEq)]
enum Transfer {
    Copy,
    Move,
}

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
    bookmarks: Vec<PathBuf>,
    history: History,
    /// Last directory recorded in the history.
    last_visited: Option<PathBuf>,
    jobs: Vec<RunningJob>,
    next_job_id: usize,
    jobs_ticking: bool,
    _jobs_ticker: Task<()>,
    /// Set to stop the running search.
    search_cancel: Option<Arc<AtomicBool>>,
    _search_task: Task<()>,
    _search_inputs: Vec<Subscription>,
    /// Open editor, shown in place of the inactive panel (before the preview).
    editor: Option<Entity<EditorPane>>,
    _editor_subscription: Option<Subscription>,
    /// The `theme` setting currently in effect.
    theme: String,
    active: usize,
    show_hidden: bool,
    status: SharedString,
    _subscriptions: Vec<Subscription>,
    _mkdir_subscription: Option<Subscription>,
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
        let panels = &settings.panels;
        let folders = [left, right].map(|path| {
            let mut folder =
                Folder::new(path).with_options(settings.show_hidden, panels.tree, panels.sort());
            if let Some(e) = folder.load_or_ancestor() {
                status = e.into();
            }
            cx.new(|_| folder)
        });
        let views = [0, 1].map(|ix| cx.new(|cx| FolderView::new(folders[ix].clone(), window, cx)));

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
            bookmarks: settings.bookmarks.clone(),
            history: History::path()
                .map(|p| History::load(&p))
                .unwrap_or_default(),
            last_visited: None,
            jobs: Vec::new(),
            next_job_id: 0,
            jobs_ticking: false,
            _jobs_ticker: Task::ready(()),
            search_cancel: None,
            _search_task: Task::ready(()),
            _search_inputs: Vec::new(),
            editor: None,
            _editor_subscription: None,
            theme: settings.theme.clone(),
            active: 0,
            show_hidden: settings.show_hidden,
            status: settings_error.map_or(status, Into::into),
            _subscriptions: subscriptions,
            _mkdir_subscription: None,
        };
        this.views[0].update(cx, |view, cx| view.set_active(true, cx));
        this.focus_active(window, cx);
        this
    }

    /// Called once the entity exists (the preview needs `self`'s context).
    pub fn init(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn open(&mut self, _: &Open, _: &mut Window, cx: &mut Context<Self>) {
        match self.update_folder(cx, Folder::activate) {
            Ok(Activation::OpenFile(path)) => {
                if let Err(e) = open::that_detached(&path) {
                    self.status = format!("Cannot open {}: {e}", path.display()).into();
                }
            }
            Ok(_) => {}
            Err(e) => self.status = e.into(),
        }
        cx.notify();
    }

    fn go_up(&mut self, _: &GoUp, _: &mut Window, cx: &mut Context<Self>) {
        let result = self.update_folder(cx, Folder::go_up);
        self.set_error(result, cx);
    }

    fn go_first(&mut self, _: &GoFirst, _: &mut Window, cx: &mut Context<Self>) {
        self.update_folder(cx, |f| f.set_cursor(0));
    }

    fn go_last(&mut self, _: &GoLast, _: &mut Window, cx: &mut Context<Self>) {
        self.update_folder(cx, |f| f.set_cursor(usize::MAX));
    }

    fn switch_panel(&mut self, _: &SwitchPanel, window: &mut Window, cx: &mut Context<Self>) {
        self.set_active(1 - self.active, cx);
        self.focus_active(window, cx);
        self.update_preview(window, cx);
    }

    /// Shift-F6: renames the cursor entry, or all marked entries at once
    /// by editing one name per line.
    fn rename(&mut self, _: &Rename, window: &mut Window, cx: &mut Context<Self>) {
        let sources = Rc::new(self.folders[self.active].read(cx).targets());
        if sources.is_empty() {
            return;
        }
        let names: Vec<String> = sources.iter().map(|p| tools::file_name(p)).collect();
        enum Field {
            One(Entity<InputState>),
            Many(Entity<TextareaState>),
        }
        let field = Rc::new(if sources.len() == 1 {
            Field::One(cx.new(|cx| InputState::new(window, cx).default_value(names[0].clone())))
        } else {
            Field::Many(cx.new(|cx| {
                let mut state = TextareaState::new(window, cx);
                state.set_value(names.join("\n"), window, cx);
                state
            }))
        });
        let this = cx.entity().downgrade();
        let dialog_field = field.clone();
        let title = if sources.len() == 1 {
            "Rename".to_string()
        } else {
            format!("Rename {} entries", sources.len())
        };
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let muted = cx.theme().muted_foreground;
            let control = match &*dialog_field {
                Field::One(input) => Input::new(input).into_any_element(),
                Field::Many(area) => v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("One name per line, in the same order"),
                    )
                    .child(Textarea::new(area).h(px(260.)))
                    .into_any_element(),
            };
            let (this, field, sources) = (this.clone(), dialog_field.clone(), sources.clone());
            dialog
                .title(title.clone())
                .width(px(520.))
                .description(div().w_full().child(control))
                .ok_text("Rename")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let text = match &*field {
                        Field::One(input) => input.read(cx).value().to_string(),
                        Field::Many(area) => area.read(cx).value().to_string(),
                    };
                    let mut close = true;
                    let _ = this.update(cx, |this, cx| {
                        match rename::plan(&sources, &text) {
                            Ok(renames) => {
                                let (done, errors) = rename::apply(&renames);
                                this.update_folder(cx, Folder::clear_marks);
                                this.reload_all(cx);
                                if let [only] = renames.as_slice() {
                                    let to = only.to.clone();
                                    this.update_folder(cx, |f| f.select_path(Some(&to)));
                                }
                                this.status = summarize("Renamed", done, &errors).into();
                                this.focus_active(window, cx);
                            }
                            Err(e) => {
                                // Keep the dialog open so the text can be fixed.
                                this.status = format!("Rename: {e}").into();
                                close = false;
                            }
                        }
                        cx.notify();
                    });
                    close
                })
        });
        match &*field {
            Field::One(input) => input.update(cx, |input, cx| input.focus(window, cx)),
            Field::Many(area) => area.update(cx, |area, cx| area.focus(window, cx)),
        }
    }

    /// Remembers directories the active panel shows, for frecency.
    fn record_visit(&mut self, folder: &Entity<Folder>, cx: &mut Context<Self>) {
        let folder = folder.read(cx);
        if folder.results().is_some() || folder.path().as_os_str().is_empty() {
            return;
        }
        let dir = folder.path();
        if self.last_visited.as_deref() == Some(dir) {
            return;
        }
        self.last_visited = Some(dir.to_path_buf());
        self.history.visit(dir, places::now());
        if let Some(path) = History::path() {
            let _ = self.history.save(&path);
        }
    }

    /// Ctrl-D: adds the active directory to the bookmarks.
    fn add_bookmark(&mut self, _: &AddBookmark, _: &mut Window, cx: &mut Context<Self>) {
        let dir = self.folders[self.active].read(cx).path().to_path_buf();
        if dir.as_os_str().is_empty() {
            return;
        }
        self.status = match settings::add_bookmark(&dir) {
            Ok(_) => {
                if !self.bookmarks.contains(&dir) {
                    self.bookmarks.push(dir.clone());
                }
                format!("Bookmarked {}", dir.display()).into()
            }
            Err(e) => e.into(),
        };
        cx.notify();
    }

    /// Ctrl-G: bookmarks, desktop places and frequently visited
    /// directories; Enter goes there in the active panel.
    fn go_to(&mut self, _: &GoTo, window: &mut Window, cx: &mut Context<Self>) {
        let mut seen = std::collections::HashSet::new();
        let mut targets: Vec<(String, PathBuf)> = Vec::new();
        let mut add = |group: &str, label: String, path: PathBuf| {
            if path.is_dir() && seen.insert(path.clone()) {
                let shown = places::tilde(&path);
                targets.push((format!("{group}  {label} — {shown}"), path));
            }
        };
        for path in &self.bookmarks {
            add("★", tools::file_name(path), path.clone());
        }
        for place in places::system_places() {
            add("⌂", place.label, place.path);
        }
        for path in self.history.ranked(places::now()).into_iter().take(30) {
            add("↻", tools::file_name(&path), path);
        }
        let labels = targets.iter().map(|(label, _)| label.clone()).collect();
        let paths: Rc<Vec<PathBuf>> = Rc::new(targets.into_iter().map(|(_, p)| p).collect());
        let this = cx.entity().downgrade();
        let delegate = PickDelegate::new(labels, move |index, window, cx| {
            window.close_dialog(cx);
            let path = paths[index].clone();
            let _ = this.update(cx, |this, cx| {
                let result = this.update_folder(cx, |folder| folder.navigate(path));
                this.set_error(result, cx);
                this.focus_active(window, cx);
            });
        });
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        list.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::new(0)), window, cx)
        });
        let dialog_list = list.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Go to")
                .w(px(640.))
                .child(
                    div().h(px(420.)).child(
                        List::new(&dialog_list)
                            .search_placeholder("★ bookmarks · ⌂ places · ↻ frequent"),
                    ),
                )
        });
        list.update(cx, |list, cx| list.focus(window, cx));
    }

    /// Alt-F7: find files by name and/or containing text, below the
    /// active panel's directory.
    fn find_files(&mut self, _: &FindFiles, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.folders[self.active].read(cx).path().to_path_buf();
        if root.as_os_str().is_empty() {
            return;
        }
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. *.rs or report"));
        let content = cx.new(|cx| InputState::new(window, cx).placeholder("optional"));
        let this = cx.entity().downgrade();
        // Enter in either field starts the search.
        self._search_inputs = [&name, &content]
            .into_iter()
            .map(|input| {
                let (name, content, root) = (name.clone(), content.clone(), root.clone());
                cx.subscribe_in(
                    input,
                    window,
                    move |this, _, event: &InputEvent, window, cx| {
                        if let InputEvent::PressEnter { .. } = event {
                            window.close_dialog(cx);
                            let query = this.search_query(&root, &name, &content, cx);
                            this.start_search(query, window, cx);
                        }
                    },
                )
            })
            .collect();
        let (dialog_name, dialog_content) = (name.clone(), content.clone());
        let location = fs_ops::display_path(&root);
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let muted = cx.theme().muted_foreground;
            let (this, name, content, root) = (
                this.clone(),
                dialog_name.clone(),
                dialog_content.clone(),
                root.clone(),
            );
            dialog
                .title("Find files")
                .description(
                    v_flex()
                        .gap_3()
                        .w_full()
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("In {location}")),
                        )
                        .child(v_flex().gap_1().child("File name").child(Input::new(&name)))
                        .child(
                            v_flex()
                                .gap_1()
                                .child("Containing text")
                                .child(Input::new(&content)),
                        ),
                )
                .ok_text("Find")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let (name, content, root) = (name.clone(), content.clone(), root.clone());
                    let _ = this.update(cx, |this, cx| {
                        let query = this.search_query(&root, &name, &content, cx);
                        this.start_search(query, window, cx);
                    });
                    true
                })
        });
        name.update(cx, |input, cx| input.focus(window, cx));
    }

    fn search_query(
        &self,
        root: &Path,
        name: &Entity<InputState>,
        content: &Entity<InputState>,
        cx: &App,
    ) -> Query {
        Query {
            root: root.to_path_buf(),
            name: name.read(cx).value().to_string(),
            content: content.read(cx).value().to_string(),
            hidden: self.show_hidden,
            ignore_files: true,
        }
    }

    /// Shows results in the active panel as they arrive.
    fn start_search(&mut self, query: Query, window: &mut Window, cx: &mut Context<Self>) {
        self._search_inputs.clear();
        if let Some(previous) = self.search_cancel.take() {
            previous.store(true, Ordering::Relaxed);
        }
        let mut title = match (query.name.trim(), query.content.trim()) {
            ("", "") => "all files".to_string(),
            (name, "") => format!("\"{name}\""),
            ("", text) => format!("containing \"{text}\""),
            (name, text) => format!("\"{name}\" containing \"{text}\""),
        };
        title = format!("Search {title}");
        let ix = self.active;
        self.folders[ix].update(cx, |folder, cx| {
            folder.show_results(query.root.clone(), title.clone());
            cx.notify();
        });
        self.status = "Searching… (Esc stops)".into();
        cx.notify();

        let cancel = Arc::new(AtomicBool::new(false));
        self.search_cancel = Some(cancel.clone());
        let found: Arc<Mutex<Vec<fs_ops::Entry>>> = Arc::default();
        let (bg_found, bg_cancel) = (found.clone(), cancel.clone());
        self._search_task = cx.spawn_in(window, async move |this, cx| {
            let mut search = cx.background_spawn(async move {
                search::run(&query, &bg_cancel, |hit| {
                    bg_found.lock().unwrap().push(hit.entry)
                })
            });
            loop {
                let timer = cx.background_executor().timer(Duration::from_millis(120));
                let finished = first_of(&mut search, timer).await;
                let batch: Vec<_> = found.lock().unwrap().drain(..).collect();
                let keep_going = this
                    .update(cx, |this, cx| {
                        let folder = &this.folders[ix];
                        // Stop if the user left the results.
                        if folder.read(cx).results() != Some(title.as_str()) {
                            cancel.store(true, Ordering::Relaxed);
                            return false;
                        }
                        if !batch.is_empty() {
                            folder.update(cx, |folder, cx| {
                                folder.add_results(batch);
                                cx.notify();
                            });
                        }
                        let count = folder.read(cx).unfiltered_count();
                        match &finished {
                            None => {
                                this.status =
                                    format!("Searching… {count} found (Esc stops)").into();
                            }
                            Some(Ok(summary)) => {
                                let note = if summary.cancelled {
                                    " (stopped)"
                                } else if summary.truncated {
                                    " (limit reached)"
                                } else {
                                    ""
                                };
                                this.status = format!("{title}: {count} found{note}").into();
                                this.search_cancel = None;
                            }
                            Some(Err(e)) => {
                                this.status = format!("Search: {e}").into();
                                this.search_cancel = None;
                            }
                        }
                        cx.notify();
                        finished.is_none()
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        });
    }

    /// What the tools see: the active panel and the other panel's directory.
    fn tool_context(&self, cx: &App) -> ToolContext {
        let folder = self.folders[self.active].read(cx);
        let other = self.folders[1 - self.active].read(cx).path();
        ToolContext {
            dir: folder.path().to_path_buf(),
            targets: folder
                .targets()
                .into_iter()
                .map(|path| {
                    let is_dir = path.is_dir();
                    Target::new(path, is_dir)
                })
                .collect(),
            other_dir: (!other.as_os_str().is_empty()).then(|| other.to_path_buf()),
        }
    }

    /// F2: tools applicable to the selection / directory, grouped.
    fn tools_menu(&mut self, _: &ToolsMenu, window: &mut Window, cx: &mut Context<Self>) {
        let ctx = self.tool_context(cx);
        let facts = tools::detect::detect(&ctx);
        let offers = self.tools.offers(&ctx, &facts, &self.programs);
        if offers.is_empty() {
            self.status = "No tools for this selection".into();
            cx.notify();
            return;
        }
        let this = cx.entity().downgrade();
        let delegate = MenuDelegate::new(offers, move |offer: Offer, window, cx| {
            window.close_dialog(cx);
            let (ctx, facts) = (ctx.clone(), facts.clone());
            let _ = this.update(cx, |this, cx| {
                this.ask_params(offer, ctx, facts, window, cx)
            });
        });
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        list.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::new(0)), window, cx)
        });
        let dialog_list = list.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.title("Tools").w(px(480.)).child(
                div()
                    .h(px(400.))
                    .child(List::new(&dialog_list).search_placeholder("Filter tools…")),
            )
        });
        list.update(cx, |list, cx| list.focus(window, cx));
    }

    /// Asks for the tool's parameters (if any), then plans it.
    fn ask_params(
        &mut self,
        offer: Offer,
        ctx: ToolContext,
        facts: tools::Facts,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let params = self
            .tools
            .tool(&offer.id)
            .map(|t| t.params(&ctx))
            .unwrap_or_default();
        if params.is_empty() {
            self.plan_tool(offer, ctx, facts, Values::new(), window, cx);
            return;
        }
        enum Field {
            Text(Entity<InputState>),
            Choice(Vec<String>, Rc<RefCell<String>>),
        }
        let fields: Rc<Vec<(String, String, Field)>> = Rc::new(
            params
                .into_iter()
                .map(|p| {
                    let default = p.default_value();
                    let field =
                        match p.kind {
                            ParamKind::Text { .. } => Field::Text(cx.new(|cx| {
                                InputState::new(window, cx).default_value(default.clone())
                            })),
                            ParamKind::Choice(options) => {
                                Field::Choice(options, Rc::new(RefCell::new(default)))
                            }
                        };
                    (p.key, p.label, field)
                })
                .collect(),
        );
        let this = cx.entity().downgrade();
        let first_input = fields.iter().find_map(|(_, _, f)| match f {
            Field::Text(input) => Some(input.clone()),
            Field::Choice(..) => None,
        });
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let form = v_flex()
                .gap_3()
                .w_full()
                .children(fields.iter().enumerate().map(|(i, (_, label, field))| {
                    let control = match field {
                        Field::Text(input) => Input::new(input).into_any_element(),
                        Field::Choice(options, value) => h_flex()
                            .flex_wrap()
                            .gap_1()
                            .children(options.iter().enumerate().map(|(j, option)| {
                                let (value, option) = (value.clone(), option.clone());
                                let selected = *value.borrow() == option;
                                Button::new(("choice", i * 100 + j))
                                    .label(option.clone())
                                    .small()
                                    .when(selected, |b| b.primary())
                                    .when(!selected, |b| b.outline())
                                    .on_click(move |_, window, _| {
                                        *value.borrow_mut() = option.clone();
                                        window.refresh();
                                    })
                            }))
                            .into_any_element(),
                    };
                    v_flex().gap_1().child(label.clone()).child(control)
                }));
            let (this, fields, offer, ctx, facts) = (
                this.clone(),
                fields.clone(),
                offer.clone(),
                ctx.clone(),
                facts.clone(),
            );
            dialog
                .title(offer.label.clone())
                .description(form)
                .ok_text("Continue")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let values: Values = fields
                        .iter()
                        .map(|(key, _, field)| {
                            let value = match field {
                                Field::Text(input) => input.read(cx).value().to_string(),
                                Field::Choice(_, value) => value.borrow().clone(),
                            };
                            (key.clone(), value)
                        })
                        .collect();
                    let (offer, ctx, facts) = (offer.clone(), ctx.clone(), facts.clone());
                    let _ = this.update(cx, |this, cx| {
                        this.plan_tool(offer, ctx, facts, values, window, cx)
                    });
                    true
                })
        });
        if let Some(input) = first_input {
            input.update(cx, |input, cx| input.focus(window, cx));
        }
    }

    /// Plans in the background (a tool may run quick probes).
    fn plan_tool(
        &mut self,
        offer: Offer,
        ctx: ToolContext,
        facts: tools::Facts,
        values: Values,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.status = format!("{}…", offer.label.trim_end_matches('…')).into();
        cx.notify();
        let (registry, programs) = (self.tools.clone(), self.programs.clone());
        self._tool_task = cx.spawn_in(window, async move |this, cx| {
            let id = offer.id.clone();
            let result = cx
                .background_spawn(
                    async move { registry.plan(&id, &ctx, &facts, &programs, &values) },
                )
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.handle_tool_action(offer.label, result, window, cx)
            });
        });
    }

    /// Resolves picks, confirms anything that changes files, then runs.
    fn handle_tool_action(
        &mut self,
        label: String,
        result: Result<ToolAction, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = match result {
            Ok(action) => action,
            Err(e) => {
                self.status = format!("{label}: {e}").into();
                cx.notify();
                return;
            }
        };
        let this = cx.entity().downgrade();
        match action {
            ToolAction::Pick { title, items } => {
                let labels = items
                    .iter()
                    .map(|i| match &i.detail {
                        Some(detail) => format!("{} — {detail}", i.label),
                        None => i.label.clone(),
                    })
                    .collect();
                let items = Rc::new(RefCell::new(items));
                let delegate = PickDelegate::new(labels, move |index, window, cx| {
                    window.close_dialog(cx);
                    let mut items = items.borrow_mut();
                    if index < items.len() {
                        let item = items.swap_remove(index);
                        let label = item.label.clone();
                        let _ = this.update(cx, |this, cx| {
                            this.handle_tool_action(label, Ok(item.action), window, cx)
                        });
                    }
                });
                let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
                list.update(cx, |list, cx| {
                    list.set_selected_index(Some(IndexPath::new(0)), window, cx)
                });
                let dialog_list = list.clone();
                window.open_dialog(cx, move |dialog, _, _| {
                    dialog
                        .title(title.clone())
                        .w(px(420.))
                        .child(div().h(px(360.)).child(List::new(&dialog_list)))
                });
                list.update(cx, |list, cx| list.focus(window, cx));
            }
            action if action.is_harmless() => self.run_tool_action(label, action, window, cx),
            action => {
                let lines = action.describe();
                let mut shown: Vec<String> = lines.iter().take(12).cloned().collect();
                if lines.len() > shown.len() {
                    shown.push(format!("… and {} more", lines.len() - shown.len()));
                }
                let pending = Rc::new(RefCell::new(Some(action)));
                window.open_alert_dialog(cx, move |dialog, _, cx| {
                    let (this, pending, label) = (this.clone(), pending.clone(), label.clone());
                    let mono = cx.theme().mono_font_family.clone();
                    dialog
                        .title(label.clone())
                        .description(
                            v_flex()
                                .gap_1()
                                .font_family(mono)
                                .text_xs()
                                .children(shown.iter().map(|l| div().child(l.clone()))),
                        )
                        .ok_text("Run")
                        .show_cancel(true)
                        .on_ok(move |_, window, cx| {
                            if let Some(action) = pending.borrow_mut().take() {
                                let label = label.clone();
                                let _ = this.update(cx, |this, cx| {
                                    this.run_tool_action(label, action, window, cx)
                                });
                            }
                            true
                        })
                });
            }
        }
    }

    fn run_tool_action(
        &mut self,
        label: String,
        action: ToolAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.status = format!("Running {label}…").into();
        cx.notify();
        self._tool_task = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { tools::exec::run(action) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(report) => {
                        this.status = format!("{label}: {}", report.summary).into();
                        if report.refresh {
                            this.reload_all(cx);
                        }
                        if !report.log.trim().is_empty() {
                            show_log(&label, report.log, window, cx);
                        }
                    }
                    Err(e) => this.status = format!("{label}: {e}").into(),
                }
                cx.notify();
            });
        });
    }

    /// Every command offered in the palette.
    fn palette_commands() -> Vec<(&'static str, Box<dyn Action>)> {
        vec![
            ("Open / enter directory", Box::new(Open)),
            ("Go to parent directory", Box::new(GoUp)),
            ("Go to first entry", Box::new(GoFirst)),
            ("Go to last entry", Box::new(GoLast)),
            ("Switch panel", Box::new(SwitchPanel)),
            ("Mark / unmark entry", Box::new(ToggleMark)),
            ("Clear filter or marks", Box::new(Unmark)),
            ("Filter names", Box::new(Filter)),
            ("Copy to other panel", Box::new(Copy)),
            ("Move to other panel", Box::new(Move)),
            ("New directory", Box::new(MakeDir)),
            ("Move to trash", Box::new(Delete)),
            ("Delete permanently", Box::new(DeletePermanently)),
            ("Edit file", Box::new(EditFile)),
            ("Toggle preview", Box::new(TogglePreview)),
            ("Toggle tree mode", Box::new(ToggleTree)),
            ("Expand directory (tree)", Box::new(Expand)),
            ("Collapse directory (tree)", Box::new(Collapse)),
            ("Toggle hidden files", Box::new(ToggleHidden)),
            ("Refresh", Box::new(Refresh)),
            ("Select theme", Box::new(SelectTheme)),
            ("Tools…", Box::new(ToolsMenu)),
            ("Find files / search contents…", Box::new(FindFiles)),
            ("Cancel running jobs", Box::new(CancelJobs)),
            ("Rename (bulk when several are marked)", Box::new(Rename)),
            ("Go to… (bookmarks, places, frequent)", Box::new(GoTo)),
            ("Bookmark this directory", Box::new(AddBookmark)),
            ("Quit", Box::new(Quit)),
        ]
    }

    /// Ctrl-P: a filterable list of all commands with their keys.
    fn command_palette(&mut self, _: &CommandPalette, window: &mut Window, cx: &mut Context<Self>) {
        // Keys are looked up from the panel, where all bindings apply.
        let panel = self.views[self.active].read(cx).focus_handle(cx);
        let commands = Self::palette_commands()
            .into_iter()
            .map(|(label, action)| {
                let keys = window
                    .bindings_for_action_in(action.as_ref(), &panel)
                    .iter()
                    .map(|binding| {
                        binding
                            .keystrokes()
                            .iter()
                            .map(|k| k.to_string())
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                Command {
                    label: label.to_string(),
                    keys,
                    payload: action,
                }
            })
            .collect();
        let delegate = PaletteDelegate::new(Palette::new(commands), move |action, window, cx| {
            window.close_dialog(cx);
            // Run once focus is back on the panel, so panel commands apply.
            window.defer(cx, move |window, cx| window.dispatch_action(action, cx));
        });
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        list.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::new(0)), window, cx)
        });
        let dialog_list = list.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.title("Commands").w(px(480.)).child(
                div()
                    .h(px(400.))
                    .child(List::new(&dialog_list).search_placeholder("Type a command…")),
            )
        });
        list.update(cx, |list, cx| list.focus(window, cx));
    }

    /// Opens the theme picker; the highlighted theme is previewed live.
    fn select_theme(&mut self, _: &SelectTheme, window: &mut Window, cx: &mut Context<Self>) {
        let registry = ThemeRegistry::global(cx);
        let defaults = [
            registry.default_light_theme().name.clone(),
            registry.default_dark_theme().name.clone(),
        ];
        let names = registry
            .themes()
            .keys()
            .filter(|name| !defaults.contains(name)) // offered as "Light"/"Dark"
            .map(|name| name.to_string())
            .collect::<Vec<_>>();
        let picker = ThemePicker::new(names, &self.theme);

        let confirmed = Rc::new(Cell::new(false));
        let this = cx.entity().downgrade();
        let delegate =
            ThemePickerDelegate::new(picker, confirmed.clone(), move |name, window, cx| {
                let _ = this.update(cx, |this, cx| this.theme_chosen(name, cx));
                window.close_dialog(cx);
            });
        let original_ix = delegate.original_index();
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        list.update(cx, |list, cx| {
            if let Some(ix) = original_ix {
                list.set_selected_index(Some(IndexPath::new(ix)), window, cx);
                list.scroll_to_selected_item(window, cx);
            }
        });

        let original = self.theme.clone();
        let dialog_list = list.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let (original, confirmed) = (original.clone(), confirmed.clone());
            dialog
                .title("Theme")
                .w(px(420.))
                .child(
                    div()
                        .h(px(380.))
                        .child(List::new(&dialog_list).search_placeholder("Filter themes…")),
                )
                .on_close(move |_, window, cx| {
                    if !confirmed.get() {
                        theme_picker::revert(&original, window, cx);
                    }
                })
        });
        list.update(cx, |list, cx| list.focus(window, cx));
    }

    fn theme_chosen(&mut self, name: String, cx: &mut Context<Self>) {
        self.status = match settings::save_theme(&name) {
            Ok(path) => format!("Theme: {name} (saved to {})", path.display()).into(),
            Err(e) => format!("Theme: {name} (not saved: {e})").into(),
        };
        self.theme = name;
        cx.notify();
    }

    /// F4: edits the cursor file in place of the inactive panel.
    fn edit_file(&mut self, _: &EditFile, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = &self.editor
            && editor.read(cx).is_modified()
        {
            self.status = "Save or close the open file first".into();
            editor
                .clone()
                .update(cx, |editor, cx| editor.focus(window, cx));
            cx.notify();
            return;
        }
        let Some(entry) = self.folders[self.active].read(cx).current().cloned() else {
            return;
        };
        if entry.is_dir {
            return;
        }
        let doc = match Document::open(&entry.path) {
            Ok(doc) => doc,
            Err(e) => {
                self.status = e.into();
                cx.notify();
                return;
            }
        };
        let editor = cx.new(|cx| EditorPane::new(doc, window, cx));
        self._editor_subscription = Some(cx.subscribe_in(
            &editor,
            window,
            |this, _, event: &EditorEvent, window, cx| {
                match event {
                    EditorEvent::Saved(message) => {
                        this.status = message.clone().into();
                        this.reload_all(cx);
                        this.preview.update(cx, |p, cx| p.reload(window, cx));
                    }
                    EditorEvent::Failed(error) => this.status = error.clone().into(),
                    EditorEvent::Closed => {
                        this.editor = None;
                        this._editor_subscription = None;
                        this.focus_active(window, cx);
                    }
                }
                cx.notify();
            },
        ));
        self.editor = Some(editor);
        cx.notify();
    }

    fn filter(&mut self, _: &Filter, window: &mut Window, cx: &mut Context<Self>) {
        self.views[self.active].update(cx, |view, cx| view.open_filter(window, cx));
    }

    fn toggle_preview(&mut self, _: &TogglePreview, window: &mut Window, cx: &mut Context<Self>) {
        self.preview_on = !self.preview_on;
        self.update_preview(window, cx);
        cx.notify();
    }

    /// Points the quick view at the active panel's cursor entry.
    fn update_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self
            .preview_on
            .then(|| self.folders[self.active].read(cx).current())
            .flatten()
            .map(|row| row.path.clone());
        self.preview
            .update(cx, |preview, cx| preview.set_target(target, window, cx));
    }

    fn toggle_tree(&mut self, _: &ToggleTree, _: &mut Window, cx: &mut Context<Self>) {
        let error = self.update_folder(cx, |f| f.set_tree(!f.is_tree()));
        self.set_error(error.map_or(Ok(()), Err), cx);
    }

    fn expand(&mut self, _: &Expand, _: &mut Window, cx: &mut Context<Self>) {
        let error = self.update_folder(cx, Folder::expand);
        self.set_error(error.map_or(Ok(()), Err), cx);
    }

    fn collapse(&mut self, _: &Collapse, _: &mut Window, cx: &mut Context<Self>) {
        let error = self.update_folder(cx, Folder::collapse);
        self.set_error(error.map_or(Ok(()), Err), cx);
    }

    fn toggle_mark(&mut self, _: &ToggleMark, _: &mut Window, cx: &mut Context<Self>) {
        self.update_folder(cx, Folder::toggle_mark);
    }

    /// Esc in a panel: stops a running search, else clears the filter if
    /// one is set, else the marks.
    fn unmark(&mut self, _: &Unmark, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(cancel) = self.search_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
            return;
        }
        self.update_folder(cx, |folder| {
            if folder.filter().is_empty() {
                folder.clear_marks();
            } else {
                folder.set_filter("");
            }
        });
    }

    fn refresh(&mut self, _: &Refresh, _: &mut Window, cx: &mut Context<Self>) {
        self.reload_all(cx);
        cx.notify();
    }

    fn toggle_hidden(&mut self, _: &ToggleHidden, _: &mut Window, cx: &mut Context<Self>) {
        self.show_hidden = !self.show_hidden;
        let show_hidden = self.show_hidden;
        for folder in &self.folders {
            folder.update(cx, |folder, cx| {
                folder.set_show_hidden(show_hidden);
                cx.notify();
            });
        }
        cx.notify();
    }

    fn quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }

    fn copy(&mut self, _: &Copy, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_transfer(Transfer::Copy, window, cx);
    }

    fn move_(&mut self, _: &Move, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_transfer(Transfer::Move, window, cx);
    }

    fn confirm_transfer(&mut self, op: Transfer, window: &mut Window, cx: &mut Context<Self>) {
        let sources = Rc::new(self.folders[self.active].read(cx).targets());
        let dest = self.folders[1 - self.active].read(cx).path().to_path_buf();
        if sources.is_empty() || dest.as_os_str().is_empty() {
            return;
        }
        let verb = if op == Transfer::Copy { "Copy" } else { "Move" };
        let description = format!("{verb} {} to {}?", describe(&sources), dest.display());
        let this = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (this, sources, dest) = (this.clone(), sources.clone(), dest.clone());
            dialog
                .title(verb)
                .description(description.clone())
                .ok_text(verb)
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let _ = this.update(cx, |this, cx| {
                        let kind = if op == Transfer::Copy {
                            JobKind::Copy
                        } else {
                            JobKind::Move
                        };
                        this.start_job(kind, sources.to_vec(), Some(dest.clone()), window, cx);
                        this.focus_active(window, cx);
                    });
                    true
                })
        });
    }

    /// Runs a copy/move/trash/delete in the background; progress shows in
    /// the jobs bar.
    fn start_job(
        &mut self,
        kind: JobKind,
        sources: Vec<PathBuf>,
        dest: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(kind, JobKind::Copy | JobKind::Move) {
            self.update_folder(cx, Folder::clear_marks);
        }
        let progress = Arc::new(Progress::default());
        let id = self.next_job_id;
        self.next_job_id += 1;
        let worker = progress.clone();
        let task = cx.spawn_in(window, async move |this, cx| {
            let outcome = cx
                .background_spawn(
                    async move { jobs::run(kind, &sources, dest.as_deref(), &worker) },
                )
                .await;
            let _ = this.update(cx, |this, cx| {
                this.jobs.retain(|job| job.id != id);
                this.reload_all(cx);
                let mut status = summarize(kind.verb().1, outcome.done, &outcome.errors);
                if outcome.cancelled {
                    status += " (cancelled)";
                }
                if kind == JobKind::Trash && !outcome.errors.is_empty() {
                    status += " (Shift-F8 deletes permanently)";
                }
                this.status = status.into();
                cx.notify();
            });
        });
        self.jobs.push(RunningJob {
            id,
            kind,
            progress,
            _task: task,
        });
        self.ensure_jobs_ticker(window, cx);
        cx.notify();
    }

    /// Repaints the jobs bar while jobs run.
    fn ensure_jobs_ticker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.jobs_ticking {
            return;
        }
        self.jobs_ticking = true;
        self._jobs_ticker = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
                let running = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        this.jobs_ticking = !this.jobs.is_empty();
                        this.jobs_ticking
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        });
    }

    fn cancel_jobs(&mut self, _: &CancelJobs, _: &mut Window, cx: &mut Context<Self>) {
        for job in &self.jobs {
            job.progress.cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }

    fn render_jobs(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.jobs.is_empty() {
            return None;
        }
        let theme = cx.theme();
        Some(
            v_flex()
                .gap_1()
                .px_2()
                .py_1()
                .border_t_1()
                .border_color(theme.border)
                .children(self.jobs.iter().map(|job| {
                    let p = &job.progress;
                    let (done, total) = (
                        p.done.load(Ordering::Relaxed),
                        p.total.load(Ordering::Relaxed),
                    );
                    let current = p.current.lock().unwrap().clone();
                    let cancelling = p.cancelled();
                    let progress = job.progress.clone();
                    h_flex()
                        .gap_3()
                        .text_xs()
                        .child(div().w(px(120.)).flex_none().child(job.kind.verb().0))
                        .child(
                            div().w(px(160.)).flex_none().child(
                                ProgressBar::new(("job", job.id)).value(p.fraction() * 100.),
                            ),
                        )
                        .child(div().flex_none().child(format!("{done}/{total}")))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.muted_foreground)
                                .child(if cancelling {
                                    "cancelling…".to_string()
                                } else {
                                    current
                                }),
                        )
                        .child(
                            Button::new(("cancel-job", job.id))
                                .label("Cancel")
                                .xsmall()
                                .ghost()
                                .on_click(move |_, _, _| {
                                    progress.cancel.store(true, Ordering::Relaxed)
                                }),
                        )
                })),
        )
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_delete(false, window, cx);
    }

    fn delete_permanently(
        &mut self,
        _: &DeletePermanently,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_delete(true, window, cx);
    }

    /// Moves the targets to the system trash, or deletes them for good.
    fn confirm_delete(&mut self, permanent: bool, window: &mut Window, cx: &mut Context<Self>) {
        let targets = Rc::new(self.folders[self.active].read(cx).targets());
        if targets.is_empty() {
            return;
        }
        let (title, description, ok) = if permanent {
            let text = format!(
                "Permanently delete {}? This cannot be undone.",
                describe(&targets)
            );
            ("Delete permanently", text, "Delete")
        } else {
            let text = format!("Move {} to the trash?", describe(&targets));
            ("Move to trash", text, "Move to trash")
        };
        let this = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (this, targets) = (this.clone(), targets.clone());
            dialog
                .title(title)
                .description(description.clone())
                .ok_text(ok)
                .ok_variant(ButtonVariant::Danger)
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let _ = this.update(cx, |this, cx| {
                        let kind = if permanent {
                            JobKind::Delete
                        } else {
                            JobKind::Trash
                        };
                        this.start_job(kind, targets.to_vec(), None, window, cx);
                        this.focus_active(window, cx);
                        cx.notify();
                    });
                    true
                })
        });
    }

    fn make_dir(&mut self, _: &MakeDir, window: &mut Window, cx: &mut Context<Self>) {
        if self.folders[self.active]
            .read(cx)
            .path()
            .as_os_str()
            .is_empty()
        {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Directory name"));
        let this = cx.entity().downgrade();

        // Enter in the input confirms, same as the OK button.
        self._mkdir_subscription = Some(cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let name = input.read(cx).value();
                    this.create_dir(&name, cx);
                    window.close_dialog(cx);
                    this.focus_active(window, cx);
                }
            },
        ));

        let dialog_input = input.clone();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (this, input) = (this.clone(), dialog_input.clone());
            dialog
                .title("New directory")
                .description(Input::new(&dialog_input))
                .ok_text("Create")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let name = input.read(cx).value();
                    let _ = this.update(cx, |this, cx| {
                        this.create_dir(&name, cx);
                        this.focus_active(window, cx);
                    });
                    true
                })
        });
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn create_dir(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let path = self.folders[self.active].read(cx).path().join(name);
        match std::fs::create_dir_all(&path) {
            Ok(()) => {
                self.status = format!("Created {}", path.display()).into();
                self.reload_all(cx);
                self.update_folder(cx, |f| f.select_path(Some(&path)));
            }
            Err(e) => self.status = format!("{}: {e}", path.display()).into(),
        }
        cx.notify();
    }

    fn render_key_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let key = |id: &'static str, label: &'static str| {
            Button::new(id).label(label).ghost().small().flex_1()
        };
        h_flex()
            .gap_1()
            .p_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                key("f3", "F3 Preview").on_click(
                    cx.listener(|this, _, w, cx| this.toggle_preview(&TogglePreview, w, cx)),
                ),
            )
            .child(
                key("f4", "F4 Edit")
                    .on_click(cx.listener(|this, _, w, cx| this.edit_file(&EditFile, w, cx))),
            )
            .child(
                key("f5", "F5 Copy")
                    .on_click(cx.listener(|this, _, w, cx| this.copy(&Copy, w, cx))),
            )
            .child(
                key("f6", "F6 Move")
                    .on_click(cx.listener(|this, _, w, cx| this.move_(&Move, w, cx))),
            )
            .child(
                key("f7", "F7 Mkdir")
                    .on_click(cx.listener(|this, _, w, cx| this.make_dir(&MakeDir, w, cx))),
            )
            .child(
                key("f8", "F8 Trash")
                    .on_click(cx.listener(|this, _, w, cx| this.delete(&Delete, w, cx))),
            )
            .child(
                key("hidden", "^H Hidden").on_click(
                    cx.listener(|this, _, w, cx| this.toggle_hidden(&ToggleHidden, w, cx)),
                ),
            )
            .child(
                key("tree", "^T Tree")
                    .on_click(cx.listener(|this, _, w, cx| this.toggle_tree(&ToggleTree, w, cx))),
            )
            .child(
                key("theme", "^K^T Theme")
                    .on_click(cx.listener(|this, _, w, cx| this.select_theme(&SelectTheme, w, cx))),
            )
            .child(
                key("f10", "F10 Quit")
                    .on_click(cx.listener(|this, _, w, cx| this.quit(&Quit, w, cx))),
            )
    }
}

impl Render for Commander {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, muted) = (theme.background, theme.foreground, theme.muted_foreground);
        v_flex()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::go_up))
            .on_action(cx.listener(Self::go_first))
            .on_action(cx.listener(Self::go_last))
            .on_action(cx.listener(Self::switch_panel))
            .on_action(cx.listener(Self::toggle_mark))
            .on_action(cx.listener(Self::unmark))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::move_))
            .on_action(cx.listener(Self::make_dir))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_permanently))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::toggle_hidden))
            .on_action(cx.listener(Self::toggle_tree))
            .on_action(cx.listener(Self::toggle_preview))
            .on_action(cx.listener(Self::filter))
            .on_action(cx.listener(Self::edit_file))
            .on_action(cx.listener(Self::select_theme))
            .on_action(cx.listener(Self::command_palette))
            .on_action(cx.listener(Self::tools_menu))
            .on_action(cx.listener(Self::find_files))
            .on_action(cx.listener(Self::cancel_jobs))
            .on_action(cx.listener(Self::rename))
            .on_action(cx.listener(Self::go_to))
            .on_action(cx.listener(Self::add_bookmark))
            .on_action(cx.listener(Self::expand))
            .on_action(cx.listener(Self::collapse))
            .on_action(cx.listener(Self::quit))
            .size_full()
            .bg(bg)
            .text_color(fg)
            .text_sm()
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .p_2()
                    .children((0..2).map(|ix| {
                        let pane = div().flex_1().min_w_0().h_full();
                        if let Some(editor) = self.editor.as_ref().filter(|_| ix != self.active) {
                            pane.child(editor.clone())
                        } else if self.preview_on && ix != self.active {
                            pane.child(self.preview.clone())
                        } else {
                            pane.child(self.views[ix].clone())
                        }
                    })),
            )
            .child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(muted)
                    .truncate()
                    .child(self.status.clone()),
            )
            .children(self.render_jobs(cx))
            .child(self.render_key_bar(cx))
    }
}

fn describe(paths: &[PathBuf]) -> String {
    match paths {
        [one] => format!(
            "\"{}\"",
            one.file_name().unwrap_or_default().to_string_lossy()
        ),
        many => format!("{} items", many.len()),
    }
}

fn summarize(verb: &str, done: usize, errors: &[String]) -> String {
    match errors {
        [] => format!("{verb} {done} item(s)"),
        [first, rest @ ..] => format!(
            "{verb} {done} item(s), {} failed: {first}{}",
            errors.len(),
            if rest.is_empty() { "" } else { " …" }
        ),
    }
}

/// Built-in tools plus the user's `<config dir>/fyle/tools.toml`.
fn load_tools() -> (Registry, Option<String>) {
    let mut registry = Registry::with_defaults();
    let Some(path) = dirs::config_dir().map(|d| d.join("fyle").join("tools.toml")) else {
        return (registry, None);
    };
    let error = match std::fs::read_to_string(&path) {
        Ok(text) => registry
            .load_toml(&text)
            .err()
            .map(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(format!("{}: {e}", path.display())),
    };
    (registry, error)
}

/// Shows a command's output in a scrollable dialog.
fn show_log(title: &str, log: String, window: &mut Window, cx: &mut App) {
    let title = format!("{title}: output");
    let lines: Rc<Vec<String>> = Rc::new(log.lines().map(String::from).collect());
    window.open_dialog(cx, move |dialog, _, cx| {
        let mono = cx.theme().mono_font_family.clone();
        dialog.title(title.clone()).w(px(760.)).child(
            div()
                .id("log")
                .h(px(420.))
                .overflow_y_scroll()
                .font_family(mono)
                .text_xs()
                .children(
                    lines
                        .iter()
                        .map(|l| div().whitespace_nowrap().child(l.clone())),
                ),
        )
    });
}

/// Waits for `task` or `timer`, whichever comes first; returns the task's
/// result if it finished.
async fn first_of<T: Unpin + 'static>(task: &mut Task<T>, timer: Task<()>) -> Option<T> {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::Poll;
    let mut timer = timer;
    std::future::poll_fn(|cx| {
        if let Poll::Ready(value) = Pin::new(&mut *task).poll(cx) {
            return Poll::Ready(Some(value));
        }
        if Pin::new(&mut timer).poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}
