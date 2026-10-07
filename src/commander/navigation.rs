//! Navigation commands: opening, history, bookmarks, locations and openers.

use std::path::Path;
use std::rc::Rc;

use gpui_kit::component::list::{List, ListState};
use gpui_kit::component::{IndexPath, WindowExt as _};
use gpui_kit::*;

use super::Commander;
use super::keys::{
    AddBookmark, CopyLocation, EditLocation, GoBack, GoFirst, GoForward, GoLast, GoTo, GoUp, Open,
    OpenTerminal, OpenWith, SwitchPanel,
};
use super::location::ListingCommit;
use crate::editor::Document;
use crate::model::Folder;
use crate::places::{self, History};
use crate::settings::{self};
use crate::tools::{self};
use crate::tools_view::PickDelegate;

impl Commander {
    pub fn open(&mut self, _: &Open, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.folders[self.active].read(cx).activation_target();
        if let Some(target) = target {
            self.navigate_panel(self.active, target, window, cx);
            return;
        }
        if self.folders[self.active].read(cx).archive().is_some() {
            self.status = "Inside an archive: F3 previews, F5 extracts".into();
        } else if let Some(location) = self.folders[self.active]
            .read(cx)
            .current()
            .map(|row| row.location.clone())
        {
            match location {
                crate::location::Location::Remote(location) => {
                    self.open_remote_file(crate::location::Location::Remote(location), window, cx)
                }
                crate::location::Location::Local(path) => {
                    self.open_local_file(path.as_path());
                }
                crate::location::Location::Archive { .. } => {}
            }
        }
        cx.notify();
    }

    /// Opens a local file with the first matching opener, else the system
    /// default. Returns whether something was started (else the status
    /// says why not).
    pub fn open_local_file(&mut self, path: &Path) -> bool {
        let rule = crate::openers::matching(&self.openers, path)
            .first()
            .cloned()
            .cloned();
        match rule {
            Some(rule) => self.run_opener(&rule, path),
            None => match open::that_detached(path) {
                Ok(()) => true,
                Err(e) => {
                    self.status = format!("Cannot open {}: {e}", path.display()).into();
                    false
                }
            },
        }
    }

    pub fn go_up(&mut self, _: &GoUp, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.folders[self.active].read(cx).parent_target();
        if let Some(target) = target {
            self.navigate_panel(self.active, target, window, cx);
        }
    }

    pub fn go_back(&mut self, _: &GoBack, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((target, selected, remote_origin)) =
            self.folders[self.active].read(cx).history_visit(true)
        {
            self.load_panel_listing(
                self.active,
                crate::location::PanelTarget::Location(target),
                ListingCommit::History {
                    back: true,
                    selected,
                    remote_origin,
                },
                false,
                window,
                cx,
            );
        }
    }

    pub fn go_forward(&mut self, _: &GoForward, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((target, selected, remote_origin)) =
            self.folders[self.active].read(cx).history_visit(false)
        {
            self.load_panel_listing(
                self.active,
                crate::location::PanelTarget::Location(target),
                ListingCommit::History {
                    back: false,
                    selected,
                    remote_origin,
                },
                false,
                window,
                cx,
            );
        }
    }

    pub fn go_first(&mut self, _: &GoFirst, _: &mut Window, cx: &mut Context<Self>) {
        self.update_folder(cx, |f| f.set_cursor(0));
    }

    pub fn go_last(&mut self, _: &GoLast, _: &mut Window, cx: &mut Context<Self>) {
        self.update_folder(cx, |f| f.set_cursor(usize::MAX));
    }

    pub fn switch_panel(&mut self, _: &SwitchPanel, window: &mut Window, cx: &mut Context<Self>) {
        self.set_active(1 - self.active, cx);
        self.focus_active(window, cx);
        self.update_preview(window, cx);
    }

    /// Starts `rule` on `path`; returns whether it started (else the
    /// status says why not).
    pub fn run_opener(&mut self, rule: &crate::openers::Opener, path: &Path) -> bool {
        let Some((program, args)) = rule.command_for(path) else {
            self.status = format!("Opener {:?} has no command", rule.label).into();
            return false;
        };
        match std::process::Command::new(&program).args(&args).spawn() {
            Ok(_) => {
                self.status =
                    format!("Opened {} with {}", tools::file_name(path), rule.label).into();
                true
            }
            Err(e) => {
                self.status = format!("{}: {e}", program.display()).into();
                false
            }
        }
    }

    /// Shift-Enter: choose how to open the cursor file.
    pub fn open_with(&mut self, _: &OpenWith, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) || self.refuse_on_remote(cx) {
            return;
        }
        let Some(entry) = self.folders[self.active].read(cx).current().cloned() else {
            return;
        };
        if entry.is_dir {
            return;
        }
        let Some(path) = entry.location.as_local_path().map(Path::to_path_buf) else {
            return;
        };
        let rules: Vec<_> = crate::openers::matching(&self.openers, &path)
            .into_iter()
            .cloned()
            .collect();
        let mut labels: Vec<String> = rules.iter().map(|r| r.label.clone()).collect();
        labels.push("System default".into());
        labels.push("Built-in editor (F4)".into());
        let rules = Rc::new(rules);
        let this = cx.entity().downgrade();
        let delegate = PickDelegate::new(labels, move |index, window, cx| {
            window.close_dialog(cx);
            let path = path.clone();
            let rules = rules.clone();
            let _ = this.update(cx, |this, cx| {
                if let Some(rule) = rules.get(index) {
                    this.run_opener(rule, &path);
                } else if index == rules.len() {
                    if let Err(e) = open::that_detached(&path) {
                        this.status = format!("Cannot open {}: {e}", path.display()).into();
                    }
                } else {
                    this.open_in_editor(path.clone(), Document::open(&path), window, cx);
                }
                cx.notify();
            });
        });
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        list.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::new(0)), window, cx)
        });
        let dialog_list = list.clone();
        let title = format!("Open {} with", entry.name);
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title(title.clone())
                .w(px(420.))
                .child(div().h(px(260.)).child(List::new(&dialog_list)))
        });
        list.update(cx, |list, cx| list.focus(window, cx));
    }

    /// Remembers directories the active panel shows, for frecency.
    pub fn record_visit(&mut self, folder: &Entity<Folder>, cx: &mut Context<Self>) {
        let folder = folder.read(cx);
        if folder.results().is_some()
            || folder.archive().is_some()
            || folder.is_loading()
            || folder.location().is_none()
        {
            return;
        }
        let location = folder
            .location()
            .expect("checked that the folder has a location")
            .clone();
        if self.last_visited.as_ref() == Some(&location) {
            return;
        }
        self.last_visited = Some(location.clone());
        self.history.visit_location(location, places::now());
        if let Some(path) = History::path() {
            let _ = self.history.save(&path);
        }
    }

    /// F9: starts a terminal in the active panel's directory.
    pub fn open_terminal(&mut self, _: &OpenTerminal, _: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) || self.refuse_on_remote(cx) {
            return;
        }
        let Some(dir) = self.folders[self.active]
            .read(cx)
            .local_directory()
            .map(std::path::Path::to_path_buf)
        else {
            return;
        };
        let env = std::env::var("TERMINAL").ok();
        let command =
            crate::terminal::command(&self.terminal_command, env.as_deref(), &dir, |name| {
                self.programs.find(name).is_some()
            });
        self.status = match command.as_deref() {
            Some([program, args @ ..]) => {
                match std::process::Command::new(program)
                    .args(args)
                    .current_dir(&dir)
                    .spawn()
                {
                    Ok(_) => format!("Opened {program} in {}", dir.display()).into(),
                    Err(e) => format!("{program}: {e}").into(),
                }
            }
            _ => "No terminal found: set [terminal] command in config.toml".into(),
        };
        cx.notify();
    }

    /// Ctrl-D: adds the active directory to the bookmarks.
    pub fn add_bookmark(&mut self, _: &AddBookmark, _: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) || self.refuse_on_remote(cx) {
            return;
        }
        let Some(dir) = self.folders[self.active]
            .read(cx)
            .local_directory()
            .map(std::path::Path::to_path_buf)
        else {
            return;
        };
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

    /// Ctrl-G: grouped drives, desktop places, bookmarks and frequent paths.
    pub fn go_to(&mut self, _: &GoTo, window: &mut Window, cx: &mut Context<Self>) {
        self.open_locations(self.active, false, window, cx);
    }

    /// Ctrl-L: edits the active panel's full path.
    pub fn edit_location(&mut self, _: &EditLocation, window: &mut Window, cx: &mut Context<Self>) {
        self.views[self.active].update(cx, |view, cx| view.open_location(window, cx));
    }

    /// Copies the active panel's full path.
    pub fn copy_location(&mut self, _: &CopyLocation, _: &mut Window, cx: &mut Context<Self>) {
        let path = crate::breadcrumbs::editable_path(&self.folders[self.active].read(cx).path());
        cx.write_to_clipboard(ClipboardItem::new_string(path.clone()));
        self.status = format!("Copied {path}").into();
        cx.notify();
    }
}
