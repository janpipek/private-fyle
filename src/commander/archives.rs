//! Reading archive listings off the UI thread.

use gpui_kit::*;
use std::path::PathBuf;

use super::Commander;
use crate::archive;
use crate::fs_ops;

impl Commander {
    /// When `target` is inside an archive that is not cached yet, shows the
    /// panel as loading, reads the archive in the background and then runs
    /// `then` (which finds the archive cached and so does not block). Returns
    /// whether the work was deferred. Esc, or any other navigation of the
    /// panel, drops the result (see `cancel_location_work`).
    pub fn defer_archive_read(
        &mut self,
        panel: usize,
        target: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> bool {
        let Some(target) = target else {
            return false;
        };
        let Some(file) = self.folders[panel].read(cx).archive_needed(&target) else {
            return false;
        };
        self.navigation_generation[panel] = self.navigation_generation[panel].wrapping_add(1);
        let request = self.navigation_generation[panel];
        self.folders[panel].update(cx, |folder, cx| {
            folder.begin_archive_load(target);
            cx.notify();
        });
        self.navigation_pending[panel] = true;
        self.pending_target[panel] = None;
        let shown = fs_ops::display_path(&file);
        self.status = format!("Reading {shown}…").into();
        cx.notify();
        self.navigation_tasks[panel] = cx.spawn_in(window, async move |this, cx| {
            let path = file.clone();
            let result = cx
                .background_spawn(async move { archive::Index::cached(&path).map(drop) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.navigation_generation[panel] != request {
                    return;
                }
                this.navigation_pending[panel] = false;
                let loading = this.folders[panel].update(cx, |folder, cx| {
                    let loading = folder.is_loading();
                    folder.end_archive_load(false);
                    cx.notify();
                    loading
                });
                if !loading {
                    return;
                }
                match result {
                    Ok(()) => {
                        this.status = String::new().into();
                        then(this, window, cx);
                    }
                    Err(e) => this.status = format!("{shown}: {e}").into(),
                }
                cx.notify();
            });
        });
        true
    }
}
