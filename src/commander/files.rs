//! File operations: clipboard, copy/move, extract, delete, create, rename and edit.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, v_flex};
use gpui_kit::*;

use super::Commander;
use super::jobs::Transfer;
use super::keys::{
    Copy, CutFiles, Delete, DeletePermanently, EditFile, ExtractArchive, MakeDir, Move, NewFile,
    PasteFiles, Rename, YankFiles,
};
use crate::editor::Document;
use crate::editor_view::{EditorEvent, EditorPane};
use crate::jobs::{self, Extraction, JobKind, Yank};
use crate::model::Folder;
use crate::tools::{self};
use crate::{archive, rename};

pub fn describe(paths: &[PathBuf]) -> String {
    match paths {
        [one] => format!(
            "\"{}\"",
            one.file_name().unwrap_or_default().to_string_lossy()
        ),
        many => format!("{} items", many.len()),
    }
}

pub fn summarize(verb: &str, done: usize, errors: &[String]) -> String {
    match errors {
        [] => format!("{verb} {done} item(s)"),
        [first, rest @ ..] => format!(
            "{verb} {done} item(s), {} failed: {first}{}",
            errors.len(),
            if rest.is_empty() { "" } else { " …" }
        ),
    }
}

impl Commander {
    pub fn yank_files(&mut self, _: &YankFiles, _: &mut Window, cx: &mut Context<Self>) {
        self.set_yank(false, cx);
    }

    pub fn cut_files(&mut self, _: &CutFiles, _: &mut Window, cx: &mut Context<Self>) {
        self.set_yank(true, cx);
    }

    /// Remembers the targets for pasting; their paths also go to the
    /// system clipboard as text.
    pub fn set_yank(&mut self, cut: bool, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) {
            return;
        }
        let paths = self.folders[self.active].read(cx).targets();
        if paths.is_empty() {
            return;
        }
        let text = paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        let verb = if cut { "Cut" } else { "Yanked" };
        self.status = format!("{verb} {}; Ctrl-V pastes", describe(&paths)).into();
        self.yank_generation = self.yank_generation.wrapping_add(1);
        self.yank = Some(Yank { paths, cut });
        self.update_folder(cx, Folder::clear_marks);
        cx.notify();
    }

    /// Ctrl-V: copies (or moves, after a cut) the yanked entries here.
    pub fn paste_files(&mut self, _: &PasteFiles, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) {
            return;
        }
        let Some(yank) = self.yank.clone() else {
            self.status = "Nothing to paste (Ctrl-C / Ctrl-X first)".into();
            cx.notify();
            return;
        };
        let dest = self.folders[self.active].read(cx).path().to_path_buf();
        match yank.paste_into(&dest) {
            Ok(kind) => {
                let restore = yank.cut.then(|| {
                    let generation = self.yank_generation;
                    self.yank = None;
                    (generation, yank.clone())
                });
                self.start_job_inner(kind, yank.paths, Some(dest), restore, window, cx);
            }
            Err(e) => {
                self.status = e.into();
                cx.notify();
            }
        }
    }

    /// Shift-F6: renames the cursor entry, or all marked entries at once
    /// by editing one name per line.
    pub fn rename(&mut self, _: &Rename, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) {
            return;
        }
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

    /// F4: edits the cursor file in place of the inactive panel.
    pub fn edit_file(&mut self, _: &EditFile, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) {
            return;
        }
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
        self.open_in_editor(entry.path.clone(), Document::open(&entry.path), window, cx);
    }

    /// Shift-F4: asks for a name, creates an empty file and edits it.
    pub fn new_file(&mut self, _: &NewFile, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) {
            return;
        }
        let dir = self.folders[self.active].read(cx).path().to_path_buf();
        if dir.as_os_str().is_empty() {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("File name"));
        let this = cx.entity().downgrade();
        let dialog_input = input.clone();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (this, input, dir) = (this.clone(), dialog_input.clone(), dir.clone());
            dialog
                .title("New file")
                .description(Input::new(&dialog_input))
                .ok_text("Create")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let name = input.read(cx).value().trim().to_string();
                    if name.is_empty() || name.contains(['/', '\\']) {
                        return false;
                    }
                    let path = dir.join(&name);
                    let _ = this.update(cx, |this, cx| {
                        let doc = crate::editor::create(&path);
                        this.reload_all(cx);
                        this.update_folder(cx, |f| f.select_path(Some(&path)));
                        this.open_in_editor(path, doc, window, cx);
                    });
                    true
                })
        });
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    /// Opens `path` in the external editor if configured, else in the
    /// built-in one (`doc` is the loaded document or why it can't be).
    pub fn open_in_editor(
        &mut self,
        path: PathBuf,
        doc: Result<Document, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let [program, args @ ..] = self.editor_command.as_slice() {
            self.status = match std::process::Command::new(program)
                .args(args)
                .arg(&path)
                .spawn()
            {
                Ok(_) => format!("Opened {} in {program}", path.display()).into(),
                Err(e) => format!("{program}: {e}").into(),
            };
            cx.notify();
            return;
        }
        let doc = match doc {
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
        // A dialog that led here restores focus 250 ms after closing
        // (gpui-component's close animation); take it back after that.
        let focus_target = editor.downgrade();
        cx.spawn_in(window, async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            let _ = focus_target.update_in(cx, |editor, window, cx| editor.focus(window, cx));
        })
        .detach();
        self.editor = Some(editor);
        cx.notify();
    }

    pub fn copy(&mut self, _: &Copy, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_transfer(Transfer::Copy, window, cx);
    }

    pub fn move_(&mut self, _: &Move, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_transfer(Transfer::Move, window, cx);
    }

    pub fn confirm_transfer(&mut self, op: Transfer, window: &mut Window, cx: &mut Context<Self>) {
        if self.folders[1 - self.active].read(cx).archive().is_some() {
            self.status = "The other panel shows an archive, which is read-only".into();
            cx.notify();
            return;
        }
        if self.folders[self.active].read(cx).archive().is_some() {
            if op == Transfer::Move {
                self.refuse_in_archive(cx);
            } else {
                self.confirm_extract(window, cx);
            }
            return;
        }
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

    /// F5 inside an archive: extracts the marked (or cursor) entries to the
    /// other panel's directory.
    pub fn confirm_extract(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((file, wanted)) = self.folders[self.active].read(cx).archive_targets() else {
            return;
        };
        let dest = self.folders[1 - self.active].read(cx).path().to_path_buf();
        if wanted.is_empty() || dest.as_os_str().is_empty() {
            return;
        }
        let extraction = Extraction {
            file,
            wanted,
            into_folder: false,
        };
        self.ask_extract(extraction, dest, window, cx);
    }

    /// Shift-F5: extracts the archive under the cursor to a folder named
    /// after it in the other panel's directory.
    pub fn extract_archive(
        &mut self,
        _: &ExtractArchive,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folder = self.folders[self.active].read(cx);
        let file = folder
            .current()
            .filter(|e| {
                folder.archive().is_none()
                    && !e.is_dir
                    && !e.is_parent
                    && archive::format_of(&e.path).is_some()
            })
            .map(|e| e.path.clone());
        let Some(file) = file else {
            self.status = "Select an archive file to extract".into();
            cx.notify();
            return;
        };
        let dest = self.folders[1 - self.active].read(cx).path().to_path_buf();
        if self.folders[1 - self.active].read(cx).archive().is_some() {
            self.status = "The other panel shows an archive, which is read-only".into();
            cx.notify();
            return;
        }
        if dest.as_os_str().is_empty() {
            return;
        }
        let extraction = Extraction {
            file,
            wanted: vec![String::new()],
            into_folder: true,
        };
        self.ask_extract(extraction, dest, window, cx);
    }

    pub fn ask_extract(
        &mut self,
        extraction: Extraction,
        dest: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let what = match extraction.wanted.as_slice() {
            [one] if one.is_empty() => format!(
                "\"{}\" to a folder",
                extraction
                    .file
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            ),
            [one] => format!("\"{}\"", one.rsplit('/').next().unwrap_or(one)),
            many => format!("{} items", many.len()),
        };
        let description = format!("Extract {what} in {}?", dest.display());
        let this = cx.entity().downgrade();
        let extraction = Rc::new(extraction);
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (this, extraction, dest) = (this.clone(), extraction.clone(), dest.clone());
            dialog
                .title("Extract")
                .description(description.clone())
                .ok_text("Extract")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.update_folder(cx, Folder::clear_marks);
                        let (extraction, dest) = ((*extraction).clone(), dest.clone());
                        this.spawn_job(JobKind::Extract, None, window, cx, move |progress| {
                            jobs::run_extract(&extraction, &dest, progress, &mut |conflict| {
                                progress.resolve(conflict)
                            })
                        });
                        this.focus_active(window, cx);
                    });
                    true
                })
        });
    }

    pub fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_delete(false, window, cx);
    }

    pub fn delete_permanently(
        &mut self,
        _: &DeletePermanently,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_delete(true, window, cx);
    }

    /// Moves the targets to the system trash, or deletes them for good.
    pub fn confirm_delete(&mut self, permanent: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) {
            return;
        }
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

    pub fn make_dir(&mut self, _: &MakeDir, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) {
            return;
        }
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

    pub fn create_dir(&mut self, name: &str, cx: &mut Context<Self>) {
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
}
