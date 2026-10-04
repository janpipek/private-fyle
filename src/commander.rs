use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::table::{DataTable, TableEvent, TableState};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::fs_ops;
use crate::panel::FileTable;

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
        Refresh,
        ToggleHidden,
        Quit,
    ]
);

const CONTEXT: &str = "Commander";
/// Overrides for keys the DataTable would otherwise use for column navigation.
const TABLE_CONTEXT: &str = "Commander > DataTable";

pub fn bind_keys(cx: &mut App) {
    let ctx = Some(CONTEXT);
    let table = Some(TABLE_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("enter", Open, ctx),
        KeyBinding::new("backspace", GoUp, ctx),
        KeyBinding::new("tab", SwitchPanel, table),
        KeyBinding::new("shift-tab", SwitchPanel, table),
        KeyBinding::new("home", GoFirst, table),
        KeyBinding::new("end", GoLast, table),
        KeyBinding::new("left", NoAction, table),
        KeyBinding::new("right", NoAction, table),
        KeyBinding::new("escape", Unmark, table),
        KeyBinding::new("insert", ToggleMark, ctx),
        KeyBinding::new("space", ToggleMark, ctx),
        KeyBinding::new("f5", Copy, ctx),
        KeyBinding::new("f6", Move, ctx),
        KeyBinding::new("f7", MakeDir, ctx),
        KeyBinding::new("f8", Delete, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("secondary-r", Refresh, ctx),
        KeyBinding::new("ctrl-h", ToggleHidden, ctx),
        KeyBinding::new("f10", Quit, ctx),
        KeyBinding::new("secondary-q", Quit, None),
    ]);
    // Finder-style equivalents; Mac keyboards often lack F-keys and Insert.
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-up", GoUp, ctx),
            KeyBinding::new("cmd-down", Open, ctx),
            KeyBinding::new("cmd-backspace", Delete, ctx),
            KeyBinding::new("cmd-shift-.", ToggleHidden, ctx),
            KeyBinding::new("cmd-shift-n", MakeDir, ctx),
        ]);
    }
}

type Table = Entity<TableState<FileTable>>;

#[derive(Clone, Copy, PartialEq)]
enum Transfer {
    Copy,
    Move,
}

pub struct Commander {
    tables: [Table; 2],
    active: usize,
    show_hidden: bool,
    status: SharedString,
    _subscriptions: Vec<Subscription>,
    _mkdir_subscription: Option<Subscription>,
}

impl Commander {
    pub fn new(left: PathBuf, right: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let new_table = |path: PathBuf, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                TableState::new(FileTable::new(path), window, cx)
                    .row_header(false)
                    .col_selectable(false)
                    .col_movable(false)
                    .loop_selection(false)
            })
        };
        let tables = [new_table(left, window, cx), new_table(right, window, cx)];

        let mut subscriptions = Vec::new();
        for (ix, table) in tables.iter().enumerate() {
            subscriptions.push(cx.on_focus_in(
                &table.focus_handle(cx),
                window,
                move |this, _, cx| {
                    this.active = ix;
                    cx.notify();
                },
            ));
            subscriptions.push(cx.subscribe_in(
                table,
                window,
                move |this, _, event, window, cx| {
                    if let TableEvent::DoubleClickedRow(_) = event {
                        this.active = ix;
                        this.open(&Open, window, cx);
                    }
                },
            ));
        }
        tables[0].focus_handle(cx).focus(window, cx);

        let mut this = Self {
            tables,
            active: 0,
            show_hidden: false,
            status: "Tab: switch panel · Enter: open · Backspace: up · Space/Insert: mark".into(),
            _subscriptions: subscriptions,
            _mkdir_subscription: None,
        };
        for ix in 0..2 {
            let path = this.tables[ix].read(cx).delegate().path.clone();
            this.load_or_ancestor(ix, path, cx);
        }
        this
    }

    fn table(&self) -> &Table {
        &self.tables[self.active]
    }

    fn path(&self, ix: usize, cx: &App) -> PathBuf {
        self.tables[ix].read(cx).delegate().path.clone()
    }

    fn cursor(&self, cx: &App) -> Option<fs_ops::Entry> {
        let table = self.table().read(cx);
        let row = table.selected_row()?;
        table.delegate().entries.get(row).cloned()
    }

    /// Marked entries, or the entry under the cursor when nothing is marked.
    fn targets(&self, cx: &App) -> Vec<PathBuf> {
        let marked = &self.table().read(cx).delegate().marked;
        if !marked.is_empty() {
            return marked.iter().cloned().collect();
        }
        self.cursor(cx)
            .filter(|e| !e.is_parent)
            .map(|e| vec![e.path])
            .unwrap_or_default()
    }

    /// Lists `path` into panel `ix`, placing the cursor on `select` if present.
    fn load(
        &mut self,
        ix: usize,
        path: PathBuf,
        select: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let entries = fs_ops::read_dir(&path, self.show_hidden)
            .map_err(|e| format!("{}: {e}", fs_ops::display_path(&path)))?;
        self.tables[ix].update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            delegate.set_entries(path, entries);
            let rows = delegate.entries.len();
            let row = select.and_then(|p| delegate.position(&p)).unwrap_or(0);
            if rows > 0 {
                table.set_selected_row(row.min(rows - 1), cx);
            }
            cx.notify();
        });
        Ok(())
    }

    /// Loads `path`, or its nearest readable ancestor.
    fn load_or_ancestor(&mut self, ix: usize, mut path: PathBuf, cx: &mut Context<Self>) {
        let select = self.tables[ix]
            .read(cx)
            .selected_row()
            .and_then(|row| self.tables[ix].read(cx).delegate().entries.get(row))
            .map(|e| e.path.clone());
        while let Err(e) = self.load(ix, path.clone(), select.clone(), cx) {
            self.status = e.into();
            match fs_ops::parent_of(&path) {
                Some(parent) => path = parent,
                None => break,
            }
        }
    }

    fn reload_all(&mut self, cx: &mut Context<Self>) {
        for ix in 0..2 {
            let path = self.path(ix, cx);
            self.load_or_ancestor(ix, path, cx);
        }
    }

    fn navigate(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let came_from = self.path(self.active, cx);
        if let Err(e) = self.load(self.active, path, Some(came_from), cx) {
            self.status = e.into();
        }
        cx.notify();
    }

    fn open(&mut self, _: &Open, _: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.cursor(cx) else { return };
        if entry.is_dir {
            self.navigate(entry.path, cx);
        } else if let Err(e) = open::that_detached(&entry.path) {
            self.status = format!("Cannot open {}: {e}", entry.name).into();
            cx.notify();
        }
    }

    fn go_up(&mut self, _: &GoUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(parent) = fs_ops::parent_of(&self.path(self.active, cx)) {
            self.navigate(parent, cx);
        }
    }

    fn select_row(&mut self, row: usize, cx: &mut Context<Self>) {
        self.table().update(cx, |table, cx| {
            let rows = table.delegate().entries.len();
            if rows > 0 {
                table.set_selected_row(row.min(rows - 1), cx);
            }
        });
    }

    fn go_first(&mut self, _: &GoFirst, _: &mut Window, cx: &mut Context<Self>) {
        self.select_row(0, cx);
    }

    fn go_last(&mut self, _: &GoLast, _: &mut Window, cx: &mut Context<Self>) {
        self.select_row(usize::MAX, cx);
    }

    fn switch_panel(&mut self, _: &SwitchPanel, window: &mut Window, cx: &mut Context<Self>) {
        self.active = 1 - self.active;
        self.focus_active(window, cx);
        cx.notify();
    }

    fn focus_active(&self, window: &mut Window, cx: &mut App) {
        self.table().focus_handle(cx).focus(window, cx);
    }

    fn toggle_mark(&mut self, _: &ToggleMark, _: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.cursor(cx).filter(|e| !e.is_parent) else {
            return;
        };
        self.table().update(cx, |table, cx| {
            let marked = &mut table.delegate_mut().marked;
            if !marked.remove(&entry.path) {
                marked.insert(entry.path);
            }
            let next = table.selected_row().map_or(0, |row| row + 1);
            if next < table.delegate().entries.len() {
                table.set_selected_row(next, cx);
            }
            cx.notify();
        });
        cx.notify();
    }

    fn unmark(&mut self, _: &Unmark, _: &mut Window, cx: &mut Context<Self>) {
        self.table().update(cx, |table, cx| {
            table.delegate_mut().marked.clear();
            cx.notify();
        });
        cx.notify();
    }

    fn refresh(&mut self, _: &Refresh, _: &mut Window, cx: &mut Context<Self>) {
        self.reload_all(cx);
        cx.notify();
    }

    fn toggle_hidden(&mut self, _: &ToggleHidden, _: &mut Window, cx: &mut Context<Self>) {
        self.show_hidden = !self.show_hidden;
        self.reload_all(cx);
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
        let sources = Rc::new(self.targets(cx));
        let dest = self.path(1 - self.active, cx);
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
        self.table()
            .update(cx, |table, _| table.delegate_mut().marked.clear());
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
        let targets = Rc::new(self.targets(cx));
        if targets.is_empty() {
            return;
        }
        let description = format!("Permanently delete {}?", describe(&targets));
        let this = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (this, targets) = (this.clone(), targets.clone());
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
                        this.reload_all(cx);
                        this.status = summarize("Deleted", done, &errors).into();
                        this.focus_active(window, cx);
                        cx.notify();
                    });
                    true
                })
        });
    }

    fn make_dir(&mut self, _: &MakeDir, window: &mut Window, cx: &mut Context<Self>) {
        if self.path(self.active, cx).as_os_str().is_empty() {
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
        let dir = self.path(self.active, cx);
        let path = dir.join(name);
        match std::fs::create_dir_all(&path) {
            Ok(()) => {
                self.status = format!("Created {}", path.display()).into();
                let _ = self.load(self.active, dir, Some(path), cx);
            }
            Err(e) => self.status = format!("{}: {e}", path.display()).into(),
        }
        cx.notify();
    }

    fn render_panel(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let is_active = ix == self.active;
        let table = self.tables[ix].read(cx).delegate();

        let dirs = table
            .entries
            .iter()
            .filter(|e| e.is_dir && !e.is_parent)
            .count();
        let files = table.entries.iter().filter(|e| !e.is_dir).count();
        let mut footer = format!("{dirs} dirs, {files} files");
        if !table.marked.is_empty() {
            footer += &format!(" · {} marked", table.marked.len());
        }

        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .gap_1()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded(theme.radius)
                    .truncate()
                    .when(is_active, |d| {
                        d.bg(theme.primary).text_color(theme.primary_foreground)
                    })
                    .when(!is_active, |d| d.bg(theme.muted))
                    .child(fs_ops::display_path(&table.path)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(DataTable::new(&self.tables[ix]).small()),
            )
            .child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
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
                    .gap_2()
                    .p_2()
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
