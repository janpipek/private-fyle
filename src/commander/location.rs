//! Background navigation and the grouped locations picker.

use gpui_kit::component::list::{List, ListState};
use gpui_kit::component::{IndexPath, WindowExt as _};
use gpui_kit::*;
use std::path::PathBuf;

use super::Commander;
use crate::breadcrumbs::NavigationGuard;
use crate::location_view::LocationDelegate;
use crate::places;

impl Commander {
    /// Loads `path` away from the UI thread and applies it only if it is still
    /// the newest navigation request for this panel.
    pub fn navigate_panel(
        &mut self,
        panel: usize,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_active(panel, cx);
        self.focus_active(window, cx);
        self.navigation_generation[panel] = self.navigation_generation[panel].wrapping_add(1);
        let request = self.navigation_generation[panel];
        let mut folder = self.folders[panel].read(cx).clone();
        let guard = NavigationGuard::new(folder.path().to_path_buf(), folder.generation());
        self.navigation_pending[panel] = true;
        let shown = path.display().to_string();
        self.status = format!("Opening {shown}").into();
        cx.notify();
        self.navigation_tasks[panel] = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { folder.navigate(path).map(|()| folder) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.navigation_generation[panel] != request {
                    return;
                }
                this.navigation_pending[panel] = false;
                let current = {
                    let live = this.folders[panel].read(cx);
                    guard.matches(live.path(), live.generation())
                };
                if !current {
                    return;
                }
                match result {
                    Ok(loaded) => {
                        this.folders[panel].update(cx, |folder, cx| {
                            *folder = loaded;
                            cx.notify();
                        });
                        this.views[panel]
                            .update(cx, |view, cx| view.finish_location_navigation(window, cx));
                        this.status = format!("Opened {shown}").into();
                        this.update_preview(window, cx);
                    }
                    Err(error) => this.status = error.into(),
                }
                cx.notify();
            });
        });
    }

    /// Discovers locations in the background, then opens the picker for the
    /// panel that requested it.
    pub fn open_locations(&mut self, panel: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.set_active(panel, cx);
        let bookmarks = self.bookmarks.clone();
        let frequent = self
            .history
            .ranked(places::now())
            .into_iter()
            .take(30)
            .collect::<Vec<_>>();
        self.status = "Discovering locations…".into();
        self.locations_generation = self.locations_generation.wrapping_add(1);
        let request = self.locations_generation;
        self.locations_pending = true;
        cx.notify();
        self._locations_task = cx.spawn_in(window, async move |this, cx| {
            let groups = cx
                .background_spawn(async move { places::grouped_locations(&bookmarks, &frequent) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.locations_generation != request {
                    return;
                }
                this.locations_pending = false;
                this.show_locations(panel, groups, window, cx);
            });
        });
    }

    /// Cancels pending breadcrumb navigation or location discovery. Returns
    /// whether Escape was consumed.
    pub fn cancel_location_work(&mut self, cx: &mut Context<Self>) -> bool {
        let mut cancelled = false;
        for panel in 0..self.navigation_pending.len() {
            if self.navigation_pending[panel] {
                self.navigation_generation[panel] =
                    self.navigation_generation[panel].wrapping_add(1);
                self.navigation_pending[panel] = false;
                self.navigation_tasks[panel] = Task::ready(());
                cancelled = true;
            }
        }
        if self.locations_pending {
            self.locations_generation = self.locations_generation.wrapping_add(1);
            self.locations_pending = false;
            self._locations_task = Task::ready(());
            cancelled = true;
        }
        if cancelled {
            self.status = "Cancelled location request".into();
            cx.notify();
        }
        cancelled
    }

    fn show_locations(
        &mut self,
        panel: usize,
        groups: Vec<places::Group>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.status = String::new().into();
        let this = cx.weak_entity();
        let delegate = LocationDelegate::new(groups, move |path, window, cx| {
            window.close_dialog(cx);
            let _ = this.update(cx, |this, cx| {
                this.navigate_panel(panel, path, window, cx);
            });
        });
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        list.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::new(0)), window, cx)
        });
        let dialog_list = list.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Locations")
                .w(px(640.))
                .child(
                    div().h(px(420.)).child(
                        List::new(&dialog_list)
                            .search_placeholder("drives, places, bookmarks, frequent"),
                    ),
                )
        });
        list.update(cx, |list, cx| list.focus(window, cx));
        cx.notify();
    }
}
