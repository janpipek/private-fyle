//! The window: two [`Folder`] models, each shown by a [`FolderView`], plus the
//! commands that act on them.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::list::{List, ListState};
use gpui_kit::component::{
    ActiveTheme as _, IndexPath, Sizable as _, ThemeRegistry, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::*;

use crate::editor::Document;
use crate::editor_view::{EditorEvent, EditorPane};
use crate::fs_ops;
use crate::model::{Activation, Folder};
use crate::panel::{Activate, FolderView};
use crate::preview::Registry;
use crate::preview_view::PreviewPane;
use crate::settings::{self, Settings};
use crate::theme_picker::{self, ThemePickerDelegate};
use crate::themes::ThemePicker;

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
        EditFile,
        SelectTheme,
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
        KeyBinding::new("f3", TogglePreview, ctx),
        KeyBinding::new("f4", EditFile, table),
        KeyBinding::new("ctrl-k ctrl-t", SelectTheme, ctx),
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
    ]);
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

        let registry = Arc::new(Registry::with_defaults());
        let preview = cx.new(|cx| PreviewPane::new(registry, window, cx));

        let mut subscriptions = Vec::new();
        for folder in &folders {
            subscriptions.push(cx.observe_in(folder, window, |this, _, window, cx| {
                this.update_preview(window, cx);
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

        let this = Self {
            folders,
            views,
            preview,
            preview_on: settings.preview.open_on_start,
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

    fn unmark(&mut self, _: &Unmark, _: &mut Window, cx: &mut Context<Self>) {
        self.update_folder(cx, Folder::clear_marks);
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
                        this.run_transfer(op, &sources, &dest, cx);
                        this.focus_active(window, cx);
                    });
                    true
                })
        });
    }

    fn run_transfer(
        &mut self,
        op: Transfer,
        sources: &[PathBuf],
        dest: &Path,
        cx: &mut Context<Self>,
    ) {
        let mut done = 0;
        let mut errors = Vec::new();
        for src in sources {
            let Some(name) = src.file_name() else {
                continue;
            };
            let target = dest.join(name);
            let result = match op {
                Transfer::Copy => fs_ops::copy(src, &target),
                Transfer::Move => fs_ops::move_to(src, &target),
            };
            match result {
                Ok(()) => done += 1,
                Err(e) => errors.push(format!("{}: {e}", name.to_string_lossy())),
            }
        }
        self.update_folder(cx, Folder::clear_marks);
        self.reload_all(cx);
        let verb = if op == Transfer::Copy {
            "Copied"
        } else {
            "Moved"
        };
        self.status = summarize(verb, done, &errors).into();
        cx.notify();
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
                        let mut done = 0;
                        let mut errors = Vec::new();
                        for path in targets.iter() {
                            let result = if permanent {
                                fs_ops::delete(path).map_err(|e| e.to_string())
                            } else {
                                fs_ops::trash(path)
                            };
                            match result {
                                Ok(()) => done += 1,
                                Err(e) => errors.push(format!("{}: {e}", path.display())),
                            }
                        }
                        this.reload_all(cx);
                        let verb = if permanent {
                            "Deleted"
                        } else {
                            "Moved to trash"
                        };
                        let mut status = summarize(verb, done, &errors);
                        if !permanent && !errors.is_empty() {
                            status += " (Shift-F8 deletes permanently)";
                        }
                        this.status = status.into();
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
            .on_action(cx.listener(Self::edit_file))
            .on_action(cx.listener(Self::select_theme))
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
