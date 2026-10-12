//! Navigation commands: opening, history, bookmarks, locations and openers.

use std::path::Path;

use gpui_kit::*;

use super::Commander;
use super::keys::{
    CopyLocation, EditLocation, GoBack, GoFirst, GoForward, GoLast, GoTo, GoToOtherFolder, GoUp,
    Open, OpenTerminal, SwapPanels, SwitchPanel, ToggleBookmark,
};
use super::location::ListingCommit;
use crate::bookmarks;
use crate::model::Folder;
use crate::places::{self, History};
use crate::tools::{self};

impl Commander {
    /// Tells each panel whether its folder is bookmarked, for the star in
    /// its location bar (hidden where bookmarking is not supported).
    pub fn sync_bookmark_stars(&self, cx: &mut Context<Self>) {
        for (folder, view) in self.folders.iter().zip(&self.views) {
            let state = folder
                .read(cx)
                .bookmark_target()
                .map(|dir| bookmarks::contains(&self.bookmarks, dir));
            view.update(cx, |view, cx| view.set_bookmark(state, cx));
        }
    }

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

    /// Where the other panel is, or is heading (see
    /// [`crate::panels::other_panel_target`]).
    pub fn other_panel_target(
        &self,
        cx: &App,
    ) -> Result<crate::location::PanelTarget, &'static str> {
        let other = 1 - self.active;
        crate::panels::other_panel_target(
            &self.folders[other].read(cx).target(),
            self.navigation[other].pending_target(),
        )
    }

    /// Ctrl-Left / Ctrl-Right: the active panel shows the other panel's
    /// folder; search results lead to their root.
    pub fn go_to_other_folder(
        &mut self,
        _: &GoToOtherFolder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.other_panel_target(cx) {
            Ok(target) => self.navigate_panel(self.active, target, window, cx),
            Err(reason) => {
                self.status = reason.into();
                cx.notify();
            }
        }
    }

    /// Why the panels cannot be swapped now (see [`crate::panels::swap_blocked`]).
    pub fn swap_blocked(&self, _: &App) -> Option<&'static str> {
        crate::panels::swap_blocked(&crate::panels::Activity {
            loading: self
                .navigation
                .iter()
                .any(crate::navigation::PanelNavigation::is_pending)
                || self.reload_cancel.iter().any(Option::is_some)
                || self.remote_check.is_some(),
            scanning: self.search_cancel.is_some() || self.size_cancel.iter().any(Option::is_some),
        })
    }

    /// Ctrl-U: exchanges what the two panels show, with cursor, marks,
    /// history, sort and view mode. The active side stays active.
    pub fn swap_panels(&mut self, _: &SwapPanels, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(reason) = self.swap_blocked(cx) {
            self.status = reason.into();
            cx.notify();
            return;
        }
        let [left, right] = self.folders.clone();
        left.update(cx, |left, cx| {
            right.update(cx, |right, _| left.swap_with(right));
            cx.notify();
        });
        right.update(cx, |_, cx| cx.notify());
        // Per-panel state kept next to the content: the remote listing
        // caches, and a remote path still waiting to open at startup.
        self.listing_caches.swap(0, 1);
        self.start_remote.swap(0, 1);
        let modes = [0, 1].map(|ix| self.views[ix].read(cx).view_mode());
        self.status = "Swapped panels".into();
        for ix in 0..2 {
            self.views[ix].update(cx, |view, cx| {
                view.set_view_mode(modes[1 - ix], cx);
                view.close_inputs(cx);
            });
            if modes[0] != modes[1]
                && let Err(error) = crate::settings::persistence::save_panel_view(ix, modes[1 - ix])
            {
                self.status = error.into();
            }
        }
        self.focus_active(window, cx);
        self.update_preview(window, cx);
        cx.notify();
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

    /// Remembers directories the active panel shows, for frecency.
    pub fn record_visit(&mut self, folder: &Entity<Folder>, cx: &mut Context<Self>) {
        let folder = folder.read(cx);
        if folder.results().is_some() || folder.archive().is_some() || folder.location().is_none() {
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
        if let Some(path) = History::path()
            && let Some(snapshot) = self.history_saves.enqueue((path, self.history.clone()))
        {
            self.start_history_save(snapshot, cx);
        }
    }

    fn start_history_save(
        &mut self,
        snapshot: (std::path::PathBuf, History),
        cx: &mut Context<Self>,
    ) {
        self._history_save_task = cx.spawn(async move |this, cx| {
            let (path, snapshot) = snapshot;
            let _ = cx
                .background_spawn(async move { snapshot.save(&path) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(snapshot) = this.history_saves.complete() {
                    this.start_history_save(snapshot, cx);
                }
            });
        });
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

    /// Ctrl-D: adds the active directory to the bookmarks, or removes it.
    pub fn toggle_bookmark(&mut self, _: &ToggleBookmark, _: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) {
            return;
        }
        let Some(dir) = self.folders[self.active]
            .read(cx)
            .bookmark_target()
            .cloned()
        else {
            self.status = "Only local and remote folders can be bookmarked".into();
            cx.notify();
            return;
        };
        let bookmarked = bookmarks::toggle(&mut self.bookmarks, &dir);
        let shown = dir.panel_path().display().to_string();
        self.status = if bookmarked {
            format!("Bookmarked {shown}")
        } else {
            format!("Removed bookmark {shown}")
        }
        .into();
        // The settings file is written in the background; undo on failure.
        let saved = dir.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { bookmarks::set_bookmark(&saved, bookmarked) })
                .await;
            if let Err(error) = result {
                let _ = this.update(cx, |this, cx| {
                    if bookmarks::contains(&this.bookmarks, &dir) == bookmarked {
                        bookmarks::toggle(&mut this.bookmarks, &dir);
                    }
                    this.status = error.into();
                    cx.notify();
                });
            }
        })
        .detach();
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
