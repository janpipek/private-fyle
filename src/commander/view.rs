//! View commands: filter, preview, tree, marks, sorting, sizes and hidden files.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::*;

use super::Commander;
use super::jobs::first_of;
use super::keys::{
    CalculateSizes, Collapse, Expand, Filter, InvertSelection, Refresh, SelectAll, SelectPattern,
    ToggleHidden, ToggleMark, TogglePreview, ToggleTree, Unmark,
};
use super::location::ListingCommit;
use crate::dirsize::{self, DirSize};
use crate::fs_ops;
use crate::grid::ViewMode;
use crate::menus::Cmd;
use crate::model::Folder;

impl Commander {
    pub fn filter(&mut self, _: &Filter, window: &mut Window, cx: &mut Context<Self>) {
        self.views[self.active].update(cx, |view, cx| view.open_filter(window, cx));
    }

    pub fn toggle_preview(
        &mut self,
        _: &TogglePreview,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preview_on = !self.preview_on;
        self.update_preview(window, cx);
        cx.notify();
    }

    /// Points the quick view at the active panel's cursor entry, at the
    /// matching line for a content search hit.
    pub fn update_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let row = self
            .preview_on
            .then(|| self.folders[self.active].read(cx).current())
            .flatten();
        let mut target = row.and_then(|row| match &row.location {
            crate::location::Location::Local(local) => Some(local.as_path().to_path_buf()),
            crate::location::Location::Remote(_) => None,
            crate::location::Location::Archive { .. } => Some(row.path.clone()),
        });
        let line = row.and_then(|row| row.hit_line.as_ref()).map(|(n, _)| *n);
        let remote = row
            .filter(|row| matches!(&row.location, crate::location::Location::Remote(_)))
            .map(|row| (row.location.clone(), row.size, row.is_dir || row.is_parent));
        if let Some((location, size, folder)) = remote {
            // Never read a remote path from the disk: show its downloaded
            // copy once there is one (small files only).
            target = if folder {
                self.remote_preview = Default::default();
                None
            } else {
                self.remote_preview_target(location, size, window, cx)
            };
        }
        self.preview.update(cx, |preview, cx| {
            preview.set_target(target, line, window, cx)
        });
    }

    pub fn toggle_tree(&mut self, _: &ToggleTree, window: &mut Window, cx: &mut Context<Self>) {
        if self.tree_unavailable(cx) {
            return;
        }
        let target = self.folders[self.active].read(cx).target();
        self.update_folder(cx, |folder| {
            folder.set_tree(!folder.is_tree());
        });
        self.load_panel_listing(
            self.active,
            target,
            ListingCommit::Options,
            false,
            window,
            cx,
        );
    }

    /// Tree mode belongs to the list: in icon view, says so in the status.
    fn tree_unavailable(&mut self, cx: &mut Context<Self>) -> bool {
        if self.folders[self.active].read(cx).is_remote() {
            self.status = "Not available on remote folders".into();
            cx.notify();
            return true;
        }
        if self.views[self.active].read(cx).view_mode() != ViewMode::Icons {
            return false;
        }
        self.status = Cmd::ToggleTree
            .unavailable(&self.menu_state(cx))
            .unwrap_or_default()
            .into();
        cx.notify();
        true
    }

    pub fn expand(&mut self, _: &Expand, window: &mut Window, cx: &mut Context<Self>) {
        if self.tree_unavailable(cx) {
            return;
        }
        let panel = self.active;
        let (target, needs_load) = {
            let folder = self.folders[panel].read(cx);
            let target = folder.target();
            let needs_load = folder
                .current()
                .filter(|row| row.is_dir && !row.is_parent)
                .is_some_and(|row| !folder.is_expanded(&row.location));
            (target, needs_load)
        };
        let error = self.update_folder(cx, Folder::expand);
        self.set_error(error.map_or(Ok(()), Err), cx);
        if needs_load {
            self.load_panel_listing(panel, target, ListingCommit::Options, false, window, cx);
        }
    }

    pub fn collapse(&mut self, _: &Collapse, window: &mut Window, cx: &mut Context<Self>) {
        if self.tree_unavailable(cx) {
            return;
        }
        let panel = self.active;
        let (target, needs_load) = {
            let folder = self.folders[panel].read(cx);
            let target = folder.target();
            let needs_load = folder
                .current()
                .is_some_and(|row| folder.is_expanded(&row.location));
            (target, needs_load)
        };
        let error = self.update_folder(cx, Folder::collapse);
        self.set_error(error.map_or(Ok(()), Err), cx);
        if needs_load {
            self.load_panel_listing(panel, target, ListingCommit::Options, false, window, cx);
        }
    }

    pub fn toggle_mark(&mut self, _: &ToggleMark, _: &mut Window, cx: &mut Context<Self>) {
        self.update_folder(cx, Folder::toggle_mark);
    }

    /// View › Sort: a sort command orders the active panel by its key
    /// (keeping the direction); Descending flips the direction.
    pub fn sort_by(&mut self, cmd: Cmd, cx: &mut Context<Self>) {
        self.update_folder(cx, |folder| {
            let mut sort = folder.sort();
            match cmd.sort_key() {
                Some(key) => sort.key = key,
                None => sort.descending = !sort.descending,
            }
            folder.set_sort(sort);
        });
    }

    pub fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.update_folder(cx, Folder::mark_all);
    }

    pub fn invert_selection(
        &mut self,
        _: &InvertSelection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_folder(cx, Folder::invert_marks);
    }

    /// Select by pattern: asks for a glob (or substring) and marks the
    /// matching entries, adding to the current marks.
    pub fn select_pattern(
        &mut self,
        _: &SelectPattern,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("e.g. *.jpg or report")
                .default_value("*")
        });
        let this = cx.entity().downgrade();
        // Enter in the input confirms, same as the OK button.
        self._pattern_subscription = Some(cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let pattern = input.read(cx).value();
                    this.mark_matching(&pattern, cx);
                    window.close_dialog(cx);
                    this.focus_active(window, cx);
                }
            },
        ));
        let dialog_input = input.clone();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (this, input) = (this.clone(), dialog_input.clone());
            dialog
                .title("Select by pattern")
                .description(Input::new(&dialog_input))
                .ok_text("Select")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let pattern = input.read(cx).value();
                    let _ = this.update(cx, |this, cx| {
                        this.mark_matching(&pattern, cx);
                        this.focus_active(window, cx);
                    });
                    true
                })
        });
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }

    pub fn mark_matching(&mut self, pattern: &str, cx: &mut Context<Self>) {
        self.status = match self.update_folder(cx, |f| f.mark_matching(pattern)) {
            Ok(n) => format!("{n} matching \"{pattern}\" marked").into(),
            Err(e) => e.into(),
        };
        cx.notify();
    }

    /// Esc in a panel: stops a running size scan or search, else clears the
    /// filter if one is set, else the marks.
    pub fn unmark(&mut self, _: &Unmark, _: &mut Window, cx: &mut Context<Self>) {
        if self.cancel_location_work(cx) {
            return;
        }
        if self.stop_size_scan(self.active) {
            return;
        }
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

    /// Ctrl-R: reloads both panels and forgets computed folder sizes
    /// (stopping the scans that would recompute them).
    pub fn refresh(&mut self, _: &Refresh, window: &mut Window, cx: &mut Context<Self>) {
        for ix in 0..2 {
            self.stop_size_scan(ix);
        }
        for ix in 0..2 {
            let target = self.folders[ix].read(cx).target();
            self.load_panel_listing(ix, target, ListingCommit::Refresh, true, window, cx);
        }
        cx.notify();
    }

    /// Asks the size scan of panel `ix` to stop; returns whether one was
    /// running. Its task finishes the rows (as incomplete) and reports.
    pub fn stop_size_scan(&mut self, ix: usize) -> bool {
        self.size_cancel[ix]
            .take()
            .is_some_and(|cancel| !cancel.swap(true, Ordering::Relaxed))
    }

    /// Calculate Sizes: measures the folders chosen by
    /// [`Folder::size_targets`] in the background, updating their rows as
    /// the scan progresses. Stops when the panel leaves the folder.
    pub fn calculate_sizes(
        &mut self,
        _: &CalculateSizes,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ix = self.active;
        let folder = self.folders[ix].clone();
        if self.refuse_on_remote(cx) {
            return;
        }
        if folder.read(cx).archive().is_some() {
            self.status = "Folder sizes are not available inside an archive".into();
            cx.notify();
            return;
        }
        let roots = folder.read(cx).size_targets();
        if roots.is_empty() {
            self.status = "No folders to measure".into();
            cx.notify();
            return;
        }
        self.stop_size_scan(ix);
        let location = |folder: &Folder| {
            (
                folder
                    .local_directory()
                    .map(PathBuf::from)
                    .unwrap_or_default(),
                folder.results().map(str::to_string),
            )
        };
        let started = location(folder.read(cx));
        let epoch = folder.read(cx).size_epoch();
        folder.update(cx, |folder, cx| {
            folder.cancel_running_sizes();
            for root in &roots {
                folder.set_dir_size(root, DirSize::default());
            }
            cx.notify();
        });
        self.status = format!("Calculating sizes of {} folders… (Esc stops)", roots.len()).into();
        cx.notify();

        let cancel = Arc::new(AtomicBool::new(false));
        self.size_cancel[ix] = Some(cancel.clone());
        let latest: Arc<Mutex<HashMap<PathBuf, DirSize>>> = Arc::default();
        let (bg_roots, bg_latest, bg_cancel) = (roots.clone(), latest.clone(), cancel.clone());
        self._size_tasks[ix] = cx.spawn_in(window, async move |this, cx| {
            let mut scan = cx.background_spawn(async move {
                dirsize::scan_dirs(&bg_roots, &bg_cancel, |path, size| {
                    bg_latest
                        .lock()
                        .expect("size updates lock is never poisoned")
                        .insert(path.to_path_buf(), *size);
                })
            });
            loop {
                let timer = cx.background_executor().timer(Duration::from_millis(150));
                let finished = first_of(&mut scan, timer).await.is_some();
                let updates: Vec<_> = latest
                    .lock()
                    .expect("size updates lock is never poisoned")
                    .drain()
                    .collect();
                let keep_going = this
                    .update(cx, |this, cx| {
                        let panel = this.folders[ix].clone();
                        if location(panel.read(cx)) != started
                            || panel.read(cx).size_epoch() != epoch
                        {
                            // Left the folder or refreshed: results are stale.
                            cancel.store(true, Ordering::Relaxed);
                            if this.size_cancel[ix]
                                .as_ref()
                                .is_some_and(|c| Arc::ptr_eq(c, &cancel))
                            {
                                this.size_cancel[ix] = None;
                                this.status = "Size calculation stopped".into();
                            }
                            cx.notify();
                            return false;
                        }
                        panel.update(cx, |folder, cx| {
                            for (path, size) in updates {
                                folder.set_dir_size_at(epoch, &path, size);
                            }
                            if finished {
                                folder.cancel_running_sizes();
                                folder.resort_by_size();
                            }
                            cx.notify();
                        });
                        let sizes: Vec<_> = roots
                            .iter()
                            .filter_map(|r| panel.read(cx).dir_size(r).copied())
                            .collect();
                        let bytes: u64 = sizes.iter().map(|s| s.bytes).sum();
                        let errors: u64 = sizes.iter().map(|s| s.errors).sum();
                        let done = sizes.iter().filter(|s| !s.is_running()).count();
                        this.status = if !finished {
                            format!(
                                "Calculating sizes: {done} of {} folders, ≥ {} (Esc stops)",
                                roots.len(),
                                fs_ops::format_size(bytes)
                            )
                        } else {
                            this.size_cancel[ix] = None;
                            let mut text = format!(
                                "Sizes of {} folders: {}",
                                roots.len(),
                                fs_ops::format_size(bytes)
                            );
                            if cancel.load(Ordering::Relaxed) {
                                text += " (stopped, lower bound)";
                            }
                            if errors > 0 {
                                text += &format!(" · {errors} unreadable entries skipped (≥)");
                            }
                            text
                        }
                        .into();
                        cx.notify();
                        !finished
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        });
    }

    pub fn toggle_hidden(&mut self, _: &ToggleHidden, window: &mut Window, cx: &mut Context<Self>) {
        self.show_hidden = !self.show_hidden;
        let show_hidden = self.show_hidden;
        for ix in 0..2 {
            let target = self.folders[ix].read(cx).target();
            self.folders[ix].update(cx, |folder, cx| {
                folder.set_show_hidden(show_hidden);
                cx.notify();
            });
            self.load_panel_listing(ix, target, ListingCommit::Options, false, window, cx);
        }
        cx.notify();
    }
}
