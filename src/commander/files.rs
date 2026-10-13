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
    Copy, CopyFullPath, CopyName, CutFiles, Delete, DeletePermanently, EditFile, ExtractArchive,
    MakeDir, Move, NewFile, PasteFiles, Rename, YankFiles,
};
use crate::editor::Document;
use crate::editor_view::EditorEvent;
use crate::jobs::{self, Extraction, JobKind, Yank};
use crate::menus::Cmd;
use crate::model::Folder;
use crate::remote::jobs::{Side, plan_transfer};
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

pub fn summarize(verb: &str, unit: &str, done: usize, errors: &[String]) -> String {
    match errors {
        [] => format!("{verb} {done} {unit}"),
        [first, rest @ ..] => format!(
            "{verb} {done} {unit}, {} failed: {first}{}",
            errors.len(),
            if rest.is_empty() { "" } else { " …" }
        ),
    }
}

impl Commander {
    /// Copies the selected entry's native path or standard remote URL.
    pub fn copy_full_path(&mut self, _: &CopyFullPath, _: &mut Window, cx: &mut Context<Self>) {
        let folder = self.folders[self.active].read(cx);
        let Some(row) = folder.current().filter(|row| !row.is_parent) else {
            return;
        };
        let address = crate::copy_path::selected_path(&row.location, |remote| {
            self.remotes
                .get(remote.remote())
                .and_then(|provider| provider.standard_url(remote))
        });
        let Some(address) = address else { return };
        cx.write_to_clipboard(ClipboardItem::new_string(address.clone()));
        self.status = format!("Copied path: {address}").into();
        cx.notify();
    }

    /// Copies the name of the entry under the cursor to the system clipboard.
    pub fn copy_name(&mut self, _: &CopyName, _: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.folders[self.active]
            .read(cx)
            .current_copy_name()
            .map(str::to_owned)
        else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(name.clone()));
        self.status = format!("Copied name: {name}").into();
        cx.notify();
    }

    fn local_targets(&mut self, cx: &mut Context<Self>) -> Option<Vec<PathBuf>> {
        match self.folders[self.active].read(cx).local_targets() {
            Ok(targets) => Some(targets),
            Err(error) => {
                self.status = error.to_string().into();
                cx.notify();
                None
            }
        }
    }

    pub fn yank_files(&mut self, _: &YankFiles, window: &mut Window, cx: &mut Context<Self>) {
        if gpui_kit::base::TextSelection::has_selection(window, cx) {
            let text = gpui_kit::base::TextSelection::selected_text(window, cx);
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            return;
        }
        self.set_yank(false, cx);
    }

    pub fn cut_files(&mut self, _: &CutFiles, _: &mut Window, cx: &mut Context<Self>) {
        self.set_yank(true, cx);
    }

    /// Remembers the targets for pasting; their paths also go to the
    /// system clipboard as text.
    pub fn set_yank(&mut self, cut: bool, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) || self.refuse_on_remote(cx) {
            return;
        }
        let Some(paths) = self.local_targets(cx) else {
            return;
        };
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
        if self.refuse_in_archive(cx) || self.refuse_on_remote(cx) {
            return;
        }
        let Some(yank) = self.yank.clone() else {
            self.status = "Nothing to paste (Ctrl-C / Ctrl-X first)".into();
            cx.notify();
            return;
        };
        let Some(dest) = self.folders[self.active]
            .read(cx)
            .local_directory()
            .map(PathBuf::from)
        else {
            return;
        };
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
        if self.folders[self.active].read(cx).is_remote() {
            self.rename_remote(window, cx);
            return;
        }
        if self.refuse_in_archive(cx) {
            return;
        }
        let Some(sources) = self.local_targets(cx) else {
            return;
        };
        let sources = Rc::new(sources);
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
                                this.reload_all(window, cx);
                                if let [only] = renames.as_slice() {
                                    let to = only.to.clone();
                                    this.update_folder(cx, |f| f.select_path(Some(&to)));
                                }
                                this.status = summarize("Renamed", "item(s)", done, &errors).into();
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

    /// F4: opens the cursor file in its own editor window.
    pub fn edit_file(&mut self, _: &EditFile, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) || self.refuse_on_remote(cx) {
            return;
        }
        let Some(entry) = self.folders[self.active].read(cx).current().cloned() else {
            return;
        };
        if entry.is_dir {
            return;
        }
        let Some(path) = entry.location.as_local_path().map(PathBuf::from) else {
            return;
        };
        self.open_in_editor(path.clone(), Document::open(&path), window, cx);
    }

    /// Shift-F4: asks for a name, creates an empty file and edits it.
    pub fn new_file(&mut self, _: &NewFile, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) || self.refuse_on_remote(cx) {
            return;
        }
        let Some(dir) = self.folders[self.active]
            .read(cx)
            .local_directory()
            .map(PathBuf::from)
        else {
            return;
        };
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
                        this.reload_all(window, cx);
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
        if let Some(existing) = self
            .editor_windows
            .iter()
            .find(|editor| editor.path == path)
        {
            let _ = existing.window.update(cx, |_, window, cx| {
                window.activate_window();
                existing.pane.update(cx, |pane, cx| pane.focus(window, cx));
                cx.activate(true);
            });
            return;
        }
        let main_window = window.window_handle();
        let (editor_window, editor) = match crate::editor_view::open_window(doc, cx) {
            Ok(opened) => opened,
            Err(error) => {
                self.status = format!("{}: {error}", path.display()).into();
                cx.notify();
                return;
            }
        };
        let subscription = cx.subscribe(&editor, move |this, _, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Saved(message) => {
                    this.status = message.clone().into();
                    let commander = cx.entity().downgrade();
                    cx.defer(move |cx| {
                        let _ = main_window.update(cx, |_, window, cx| {
                            let _ = commander.update(cx, |this, cx| {
                                this.reload_all(window, cx);
                                if this.workspace.perspective()
                                    == crate::workspace::Perspective::Preview
                                {
                                    this.preview
                                        .update(cx, |preview, cx| preview.reload(window, cx));
                                    this.properties.update(cx, |properties, cx| {
                                        properties.set_target(None, window, cx)
                                    });
                                } else {
                                    this.preview.update(cx, |preview, cx| {
                                        preview.set_location_target(None, None, None, window, cx)
                                    });
                                    this.properties.update(cx, |properties, cx| {
                                        properties.invalidate(window, cx)
                                    });
                                }
                            });
                        });
                    });
                }
                EditorEvent::Failed(error) => this.status = error.clone().into(),
                EditorEvent::Closed => {
                    this.editor_windows
                        .retain(|editor| editor.window != editor_window);
                }
            }
            cx.notify();
        });
        self.editor_windows.push(super::EditorWindow {
            path,
            window: editor_window,
            pane: editor,
            _subscription: subscription,
        });
        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            let _ = editor_window.update(cx, |_, window, cx| {
                window.activate_window();
                cx.activate(true);
            });
        })
        .detach();
        cx.notify();
    }

    pub fn copy(&mut self, _: &Copy, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_transfer(Transfer::Copy, window, cx);
    }

    pub fn move_(&mut self, _: &Move, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_transfer(Transfer::Move, window, cx);
    }

    pub fn confirm_transfer(&mut self, op: Transfer, window: &mut Window, cx: &mut Context<Self>) {
        // A remote on either side is handled by its provider.
        let (from, to) = self.panel_sides(cx);
        let in_archive = self.folders[self.active].read(cx).archive().is_some();
        if in_archive && to != Side::Local {
            self.status = "Extracting into a remote folder is not supported yet".into();
            cx.notify();
            return;
        }
        if !in_archive {
            match plan_transfer(op == Transfer::Copy, &from, &to, |name| {
                self.remote_info(name)
            }) {
                Ok(None) => {}
                Ok(Some(plan)) => {
                    self.confirm_remote_transfer(plan, op, window, cx);
                    return;
                }
                Err(reason) => {
                    self.status = reason.into();
                    cx.notify();
                    return;
                }
            }
        }
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
        let Some(sources) = self.local_targets(cx) else {
            return;
        };
        let sources = Rc::new(sources);
        let Some(dest) = self.folders[1 - self.active]
            .read(cx)
            .local_directory()
            .map(PathBuf::from)
        else {
            return;
        };
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
        let Some(dest) = self.folders[1 - self.active]
            .read(cx)
            .local_directory()
            .map(PathBuf::from)
        else {
            return;
        };
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
        if self.refuse_on_remote(cx) {
            return;
        }
        let folder = self.folders[self.active].read(cx);
        let file = folder
            .current()
            .filter(|e| {
                folder.archive().is_none()
                    && !e.is_dir
                    && !e.is_parent
                    && e.location
                        .as_local_path()
                        .is_some_and(|path| archive::format_of(path).is_some())
            })
            .and_then(|e| e.location.as_local_path().map(PathBuf::from));
        let Some(file) = file else {
            self.status = "Select an archive file to extract".into();
            cx.notify();
            return;
        };
        let Some(dest) = self.folders[1 - self.active]
            .read(cx)
            .local_directory()
            .map(PathBuf::from)
        else {
            return;
        };
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
        if self.folders[self.active].read(cx).is_remote() {
            self.confirm_remote_delete(permanent, window, cx);
            return;
        }
        if self.refuse_in_archive(cx) {
            return;
        }
        let Some(targets) = self.local_targets(cx) else {
            return;
        };
        let targets = Rc::new(targets);
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
        if self.folders[self.active].read(cx).is_remote()
            && let Some(reason) = Cmd::MakeDir.unavailable(&self.menu_state(cx))
        {
            self.status = reason.into();
            cx.notify();
            return;
        }
        if self.folders[self.active]
            .read(cx)
            .local_directory()
            .is_none()
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
                    this.create_dir(&name, window, cx);
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
                        this.create_dir(&name, window, cx);
                        this.focus_active(window, cx);
                    });
                    true
                })
        });
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    pub fn create_dir(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if self.folders[self.active].read(cx).is_remote() {
            self.create_remote_dir(name, cx);
            return;
        }
        let (Some(dir), target) = ({
            let folder = self.folders[self.active].read(cx);
            (folder.local_directory().map(PathBuf::from), folder.target())
        }) else {
            return;
        };
        let path = dir.join(name);
        let display = path.display().to_string();
        let selected = crate::location::Location::local(path.clone());
        let panel = self.active;
        self.status = format!("Creating {display}…").into();
        cx.notify();
        cx.spawn_in(window, async move |this_entity, cx| {
            let result = cx
                .background_spawn(async move { std::fs::create_dir_all(&path) })
                .await;
            let _ = this_entity.update_in(cx, |this, window, cx| {
                match result {
                    Ok(()) => match selected {
                        Ok(selected) => {
                            let unchanged = this.folders[panel].read(cx).target() == target;
                            if unchanged {
                                this.load_panel_listing(
                                    panel,
                                    target,
                                    super::location::ListingCommit::RefreshSelect(selected),
                                    false,
                                    window,
                                    cx,
                                );
                            }
                            this.status = format!("Created {display}").into();
                        }
                        Err(error) => this.status = format!("{display}: {error}").into(),
                    },
                    Err(error) => this.status = format!("{display}: {error}").into(),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
