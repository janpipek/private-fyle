use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::fs_ops::{self, Entry};

actions!(
    fyle,
    [
        SelectPrev,
        SelectNext,
        SelectFirst,
        SelectLast,
        PageUp,
        PageDown,
        Open,
        GoUp,
        SwitchPanel,
        ToggleMark,
        Copy,
        Move,
        MakeDir,
        Delete,
        Refresh,
        ToggleHidden,
        Quit,
    ]
);

const CONTEXT: &str = "Commander";
const ROW_HEIGHT: f32 = 22.;
const PAGE: usize = 20;

pub fn bind_keys(cx: &mut App) {
    let ctx = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, ctx),
        KeyBinding::new("down", SelectNext, ctx),
        KeyBinding::new("home", SelectFirst, ctx),
        KeyBinding::new("end", SelectLast, ctx),
        KeyBinding::new("pageup", PageUp, ctx),
        KeyBinding::new("pagedown", PageDown, ctx),
        KeyBinding::new("enter", Open, ctx),
        KeyBinding::new("backspace", GoUp, ctx),
        KeyBinding::new("tab", SwitchPanel, ctx),
        KeyBinding::new("insert", ToggleMark, ctx),
        KeyBinding::new("space", ToggleMark, ctx),
        KeyBinding::new("f5", Copy, ctx),
        KeyBinding::new("f6", Move, ctx),
        KeyBinding::new("f7", MakeDir, ctx),
        KeyBinding::new("f8", Delete, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("ctrl-r", Refresh, ctx),
        KeyBinding::new("ctrl-h", ToggleHidden, ctx),
        KeyBinding::new("f10", Quit, ctx),
        KeyBinding::new("ctrl-q", Quit, None),
    ]);
}

struct Panel {
    path: PathBuf,
    entries: Rc<Vec<Entry>>,
    selected: usize,
    marked: BTreeSet<PathBuf>,
    scroll: UniformListScrollHandle,
}

impl Panel {
    fn new(path: PathBuf, show_hidden: bool) -> Self {
        let mut panel = Self {
            path,
            entries: Rc::default(),
            selected: 0,
            marked: BTreeSet::new(),
            scroll: UniformListScrollHandle::new(),
        };
        // Fall back to the nearest readable ancestor.
        while panel.reload(show_hidden).is_err() {
            match panel.path.parent() {
                Some(parent) => panel.path = parent.to_path_buf(),
                None => break,
            }
        }
        panel
    }

    /// Re-reads the directory, keeping the selection on the same name if possible.
    fn reload(&mut self, show_hidden: bool) -> Result<(), String> {
        let entries = fs_ops::read_dir(&self.path, show_hidden).map_err(|e| e.to_string())?;
        let current = self.entries.get(self.selected).map(|e| e.name.clone());
        self.selected = current
            .and_then(|name| entries.iter().position(|e| e.name == name))
            .unwrap_or(self.selected)
            .min(entries.len().saturating_sub(1));
        self.marked.retain(|p| entries.iter().any(|e| &e.path == p));
        self.entries = Rc::new(entries);
        Ok(())
    }

    fn navigate(&mut self, path: PathBuf, show_hidden: bool) -> Result<(), String> {
        let entries = fs_ops::read_dir(&path, show_hidden).map_err(|e| e.to_string())?;
        // When going up, put the cursor on the directory we came from.
        let came_from = self.path.clone();
        self.selected = entries
            .iter()
            .position(|e| !e.is_parent && e.path == came_from)
            .unwrap_or(0);
        self.path = path;
        self.entries = Rc::new(entries);
        self.marked.clear();
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Center);
        Ok(())
    }

    fn select(&mut self, ix: usize) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = ix.min(self.entries.len() - 1);
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Top);
    }

    fn current(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }

    /// Marked entries, or the entry under the cursor when nothing is marked.
    fn targets(&self) -> Vec<PathBuf> {
        if !self.marked.is_empty() {
            return self.marked.iter().cloned().collect();
        }
        self.current()
            .filter(|e| !e.is_parent)
            .map(|e| vec![e.path.clone()])
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Transfer {
    Copy,
    Move,
}

pub struct Commander {
    panels: [Panel; 2],
    active: usize,
    show_hidden: bool,
    status: SharedString,
    focus_handle: FocusHandle,
    _mkdir_subscription: Option<Subscription>,
}

impl Commander {
    pub fn new(left: PathBuf, right: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        Self {
            panels: [Panel::new(left, false), Panel::new(right, false)],
            active: 0,
            show_hidden: false,
            status: "Tab: switch panel · Enter: open · Backspace: up · Space/Insert: mark".into(),
            focus_handle,
            _mkdir_subscription: None,
        }
    }

    fn panel(&mut self) -> &mut Panel {
        &mut self.panels[self.active]
    }

    fn reload_all(&mut self) {
        let show_hidden = self.show_hidden;
        for panel in &mut self.panels {
            if let Err(e) = panel.reload(show_hidden) {
                self.status = format!("{}: {e}", panel.path.display()).into();
            }
        }
    }

    fn move_cursor(&mut self, delta: isize, cx: &mut Context<Self>) {
        let panel = self.panel();
        let ix = panel.selected.saturating_add_signed(delta);
        panel.select(ix);
        cx.notify();
    }

    fn select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(-1, cx);
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(1, cx);
    }

    fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(-(PAGE as isize), cx);
    }

    fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(PAGE as isize, cx);
    }

    fn select_first(&mut self, _: &SelectFirst, _: &mut Window, cx: &mut Context<Self>) {
        self.panel().select(0);
        cx.notify();
    }

    fn select_last(&mut self, _: &SelectLast, _: &mut Window, cx: &mut Context<Self>) {
        self.panel().select(usize::MAX);
        cx.notify();
    }

    fn open(&mut self, _: &Open, _: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.panel().current().cloned() else {
            return;
        };
        if entry.is_dir {
            let show_hidden = self.show_hidden;
            if let Err(e) = self.panel().navigate(entry.path.clone(), show_hidden) {
                self.status = format!("{}: {e}", entry.path.display()).into();
            }
        } else if let Err(e) = open::that_detached(&entry.path) {
            self.status = format!("Cannot open {}: {e}", entry.name).into();
        }
        cx.notify();
    }

    fn go_up(&mut self, _: &GoUp, _: &mut Window, cx: &mut Context<Self>) {
        let show_hidden = self.show_hidden;
        let panel = self.panel();
        if let Some(parent) = panel.path.parent().map(Path::to_path_buf) {
            if let Err(e) = panel.navigate(parent, show_hidden) {
                self.status = e.into();
            }
            cx.notify();
        }
    }

    fn switch_panel(&mut self, _: &SwitchPanel, _: &mut Window, cx: &mut Context<Self>) {
        self.active = 1 - self.active;
        cx.notify();
    }

    fn toggle_mark(&mut self, _: &ToggleMark, _: &mut Window, cx: &mut Context<Self>) {
        let panel = self.panel();
        if let Some(entry) = panel.current().filter(|e| !e.is_parent) {
            let path = entry.path.clone();
            if !panel.marked.remove(&path) {
                panel.marked.insert(path);
            }
        }
        let next = panel.selected + 1;
        panel.select(next);
        cx.notify();
    }

    fn refresh(&mut self, _: &Refresh, _: &mut Window, cx: &mut Context<Self>) {
        self.reload_all();
        cx.notify();
    }

    fn toggle_hidden(&mut self, _: &ToggleHidden, _: &mut Window, cx: &mut Context<Self>) {
        self.show_hidden = !self.show_hidden;
        self.reload_all();
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
        let sources = Rc::new(self.panel().targets());
        if sources.is_empty() {
            return;
        }
        let dest = self.panels[1 - self.active].path.clone();
        let verb = if op == Transfer::Copy { "Copy" } else { "Move" };
        let description = format!("{verb} {} to {}?", describe(&sources), dest.display());
        let this = cx.entity().downgrade();
        let focus = self.focus_handle.clone();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (this, sources, dest, focus) =
                (this.clone(), sources.clone(), dest.clone(), focus.clone());
            dialog
                .title(verb)
                .description(description.clone())
                .ok_text(verb)
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.run_transfer(op, &sources, &dest);
                        cx.notify();
                    });
                    window.focus(&focus, cx);
                    true
                })
        });
    }

    fn run_transfer(&mut self, op: Transfer, sources: &[PathBuf], dest: &Path) {
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
        self.panels[self.active].marked.clear();
        self.reload_all();
        let verb = if op == Transfer::Copy {
            "Copied"
        } else {
            "Moved"
        };
        self.status = summarize(verb, done, &errors).into();
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        let targets = Rc::new(self.panel().targets());
        if targets.is_empty() {
            return;
        }
        let description = format!("Permanently delete {}?", describe(&targets));
        let this = cx.entity().downgrade();
        let focus = self.focus_handle.clone();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (this, targets, focus) = (this.clone(), targets.clone(), focus.clone());
            dialog
                .title("Delete")
                .description(description.clone())
                .ok_text("Delete")
                .ok_variant(ButtonVariant::Danger)
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let _ = this.update(cx, |this, cx| {
                        let mut done = 0;
                        let mut errors = Vec::new();
                        for path in targets.iter() {
                            match fs_ops::delete(path) {
                                Ok(()) => done += 1,
                                Err(e) => errors.push(format!("{}: {e}", path.display())),
                            }
                        }
                        this.reload_all();
                        this.status = summarize("Deleted", done, &errors).into();
                        cx.notify();
                    });
                    window.focus(&focus, cx);
                    true
                })
        });
    }

    fn make_dir(&mut self, _: &MakeDir, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Directory name"));
        let this = cx.entity().downgrade();
        let focus = self.focus_handle.clone();

        // Enter in the input confirms, same as the OK button.
        self._mkdir_subscription = Some(cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let name = input.read(cx).value();
                    this.create_dir(&name, cx);
                    window.close_dialog(cx);
                    window.focus(&this.focus_handle, cx);
                }
            },
        ));

        let dialog_input = input.clone();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (this, input, focus) = (this.clone(), dialog_input.clone(), focus.clone());
            dialog
                .title("New directory")
                .description(Input::new(&dialog_input))
                .ok_text("Create")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let name = input.read(cx).value();
                    let _ = this.update(cx, |this, cx| this.create_dir(&name, cx));
                    window.focus(&focus, cx);
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
        let path = self.panel().path.join(name);
        self.status = match std::fs::create_dir_all(&path) {
            Ok(()) => format!("Created {}", path.display()).into(),
            Err(e) => format!("{}: {e}", path.display()).into(),
        };
        self.reload_all();
        if let Some(ix) = self.panel().entries.iter().position(|e| e.path == path) {
            self.panel().select(ix);
        }
        cx.notify();
    }

    fn click_row(
        &mut self,
        panel: usize,
        ix: usize,
        clicks: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active = panel;
        self.panels[panel].select(ix);
        window.focus(&self.focus_handle, cx);
        if clicks >= 2 {
            self.open(&Open, window, cx);
        }
        cx.notify();
    }

    fn render_panel(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let panel = &self.panels[ix];
        let is_active = ix == self.active;

        let entries = panel.entries.clone();
        let marked = panel.marked.clone();
        let selected = panel.selected;
        let cursor_bg = if is_active {
            theme.list_active
        } else {
            theme.list_hover
        };
        let dir_color = theme.primary;
        let mark_color = theme.danger;
        let muted = theme.muted_foreground;
        let this = cx.entity().downgrade();

        let dirs = entries.iter().filter(|e| e.is_dir && !e.is_parent).count();
        let files = entries.iter().filter(|e| !e.is_dir).count();
        let mut footer = format!("{dirs} dirs, {files} files");
        if !marked.is_empty() {
            footer += &format!(" · {} marked", marked.len());
        }

        let list = uniform_list(("panel", ix), entries.len(), move |range, _, _| {
            range
                .map(|i| {
                    let entry = &entries[i];
                    let is_marked = marked.contains(&entry.path);
                    let this = this.clone();
                    let size = if entry.is_dir {
                        "<DIR>".to_string()
                    } else {
                        fs_ops::format_size(entry.size)
                    };
                    let modified = entry.modified.map(fs_ops::format_time).unwrap_or_default();
                    h_flex()
                        .id(i)
                        .w_full()
                        .h(px(ROW_HEIGHT))
                        .px_2()
                        .gap_2()
                        .when(i == selected, |row| row.bg(cursor_bg))
                        .when(entry.is_dir, |row| row.text_color(dir_color))
                        .when(is_marked, |row| row.text_color(mark_color))
                        .child(if entry.is_dir {
                            IconName::Folder
                        } else {
                            IconName::File
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(entry.name.clone()),
                        )
                        .child(div().w(px(80.)).flex_none().text_right().child(size))
                        .child(
                            div()
                                .w(px(130.))
                                .flex_none()
                                .text_color(muted)
                                .child(modified),
                        )
                        .on_click(move |event, window, cx| {
                            let clicks = event.click_count();
                            let _ = this
                                .update(cx, |this, cx| this.click_row(ix, i, clicks, window, cx));
                        })
                })
                .collect()
        })
        .track_scroll(&panel.scroll)
        .flex_1()
        .min_h_0();

        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .border_1()
            .border_color(if is_active {
                theme.primary
            } else {
                theme.border
            })
            .child(
                div()
                    .px_2()
                    .py_1()
                    .truncate()
                    .when(is_active, |d| {
                        d.bg(theme.primary).text_color(theme.primary_foreground)
                    })
                    .when(!is_active, |d| d.bg(theme.muted))
                    .child(panel.path.display().to_string()),
            )
            .child(
                h_flex()
                    .px_2()
                    .gap_2()
                    .text_xs()
                    .text_color(muted)
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().w(px(16.)))
                    .child(div().flex_1().child("Name"))
                    .child(div().w(px(80.)).text_right().child("Size"))
                    .child(div().w(px(130.)).child("Modified")),
            )
            .child(list)
            .child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(muted)
                    .border_t_1()
                    .border_color(theme.border)
                    .child(footer),
            )
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
                key("f8", "F8 Delete")
                    .on_click(cx.listener(|this, _, w, cx| this.delete(&Delete, w, cx))),
            )
            .child(
                key("hidden", "^H Hidden").on_click(
                    cx.listener(|this, _, w, cx| this.toggle_hidden(&ToggleHidden, w, cx)),
                ),
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
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_first))
            .on_action(cx.listener(Self::select_last))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::go_up))
            .on_action(cx.listener(Self::switch_panel))
            .on_action(cx.listener(Self::toggle_mark))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::move_))
            .on_action(cx.listener(Self::make_dir))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::toggle_hidden))
            .on_action(cx.listener(Self::quit))
            .size_full()
            .bg(bg)
            .text_color(fg)
            .text_sm()
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_1()
                    .p_1()
                    .child(self.render_panel(0, cx))
                    .child(self.render_panel(1, cx)),
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
