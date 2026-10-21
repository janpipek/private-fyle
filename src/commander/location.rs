//! Background navigation and the grouped locations picker.

use gpui_kit::component::button::Button;
use gpui_kit::component::list::{List, ListState};
use gpui_kit::component::{IndexPath, Sizable as _, WindowExt as _, v_flex};
use gpui_kit::*;

use super::Commander;
use crate::breadcrumbs::NavigationGuard;
use crate::fs_ops;
use crate::listing::ListingService;
use crate::location::PanelTarget;
use crate::location_view::LocationDelegate;
use crate::places;

#[derive(Clone)]
pub enum ListingCommit {
    /// Commit a user navigation and record the directory left behind.
    Navigate,
    /// Replace the current rows without adding a history visit.
    Refresh,
    /// Refresh after creating a local directory and select that row.
    RefreshSelect(crate::location::Location),
    /// Re-list after changing hidden-file or tree options without clearing cached sizes.
    Options,
    /// Load a panel's first directory without creating a history visit.
    Startup,
    /// Commit Back or Forward and restore its selection and remote origin.
    History {
        back: bool,
        selected: Option<crate::location::Location>,
        remote_origin: Option<crate::location::Location>,
    },
}

fn preserve_startup_diagnostics(has_startup_diagnostics: bool, commit: &ListingCommit) -> bool {
    has_startup_diagnostics && matches!(commit, ListingCommit::Startup)
}

fn successful_listing_status(
    has_startup_diagnostics: bool,
    commit: &ListingCommit,
    shown: &str,
    first_error: Option<String>,
    warnings: String,
) -> Option<String> {
    first_error
        .or_else(|| (!warnings.is_empty()).then_some(warnings))
        .or_else(|| {
            (!preserve_startup_diagnostics(has_startup_diagnostics, commit))
                .then(|| format!("Opened {shown}"))
        })
}

impl Commander {
    /// Loads `path` away from the UI thread and applies it only if it is still
    /// the newest navigation request for this panel.
    pub fn navigate_panel(
        &mut self,
        panel: usize,
        target: PanelTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_active(panel, cx);
        self.focus_active(window, cx);
        self.load_panel_listing(panel, target, ListingCommit::Navigate, false, window, cx);
    }

    /// Loads a typed panel target off the UI thread and commits only the
    /// newest successful result. Existing rows remain visible while loading.
    pub fn load_panel_listing(
        &mut self,
        panel: usize,
        target: PanelTarget,
        commit: ListingCommit,
        fallback: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = target.panel_path();
        if let PanelTarget::Location(crate::location::Location::Local(scope)) = &target {
            self.invalidate_git_scope(scope.as_path(), window, cx);
        }
        self.cancel_remote(panel);
        let request = self.navigation[panel].begin(target.clone());
        let (mut listing, guard) = {
            let folder = self.folders[panel].read(cx);
            let preserve_search = matches!(
                &commit,
                ListingCommit::Refresh | ListingCommit::RefreshSelect(_) | ListingCommit::Options
            ) && folder.target() == target;
            (
                folder.listing_request(target.clone(), fallback, preserve_search),
                NavigationGuard::new(folder.target(), folder.generation()),
            )
        };
        if let ListingCommit::History { remote_origin, .. } = &commit {
            listing.remote_origin = remote_origin.clone();
        }
        listing.force_remote_refresh = matches!(&commit, ListingCommit::Refresh);
        let service =
            ListingService::with_cache(self.remotes.clone(), self.listing_caches[panel].clone());
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.remote_cancel[panel] = Some(cancel.clone());
        let shown = path.display().to_string();
        let preserve_startup_diagnostics =
            preserve_startup_diagnostics(self.has_startup_diagnostics, &commit);
        if !preserve_startup_diagnostics {
            self.status = format!("Opening {shown}").into();
            cx.notify();
        }
        self.navigation_tasks[panel] = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(
                    async move { service.list_with_ancestor_fallback(&listing, &cancel) },
                )
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if !this.navigation[panel].complete(request) {
                    return;
                }
                this.remote_cancel[panel] = None;
                let current = {
                    let live = this.folders[panel].read(cx);
                    guard.matches(&live.target(), live.generation())
                };
                if !current {
                    return;
                }
                match result {
                    Ok(outcome) => {
                        if outcome.snapshot.target != target
                            && let PanelTarget::Location(crate::location::Location::Local(scope)) =
                                &outcome.snapshot.target
                        {
                            this.invalidate_git_scope(scope.as_path(), window, cx);
                        }
                        let warnings = outcome.snapshot.warnings.join("; ");
                        let first_error = outcome.first_error.map(|error| {
                            format!(
                                "{}: {error}",
                                fs_ops::display_path(&outcome.requested.panel_path())
                            )
                        });
                        let status_update = successful_listing_status(
                            this.has_startup_diagnostics,
                            &commit,
                            &shown,
                            first_error,
                            warnings,
                        );
                        this.folders[panel].update(cx, |folder, cx| {
                            let selected = match commit.clone() {
                                ListingCommit::Navigate | ListingCommit::Startup => {
                                    folder.selection_for_navigation(&outcome.snapshot.target)
                                }
                                ListingCommit::Refresh
                                | ListingCommit::RefreshSelect(_)
                                | ListingCommit::Options => {
                                    folder.current().map(|row| row.location.clone())
                                }
                                ListingCommit::History { selected, .. } => selected,
                            };
                            match commit {
                                ListingCommit::Navigate | ListingCommit::Startup => {
                                    folder.commit_navigation_listing(outcome.snapshot, selected);
                                }
                                ListingCommit::Refresh => {
                                    folder.clear_sizes();
                                    folder.apply_listing(outcome.snapshot, selected);
                                }
                                ListingCommit::RefreshSelect(location) => {
                                    folder.clear_sizes();
                                    folder.apply_listing(outcome.snapshot, Some(location));
                                }
                                ListingCommit::Options => {
                                    folder.apply_listing(outcome.snapshot, selected);
                                }
                                ListingCommit::History { back, .. } => {
                                    folder.commit_history_listing(outcome.snapshot, back, selected);
                                }
                            }
                            cx.notify();
                        });
                        this.views[panel]
                            .update(cx, |view, cx| view.finish_location_navigation(window, cx));
                        if let Some(message) = status_update {
                            this.status = message.into();
                        }
                        this.update_preview(window, cx);
                    }
                    Err(error) => {
                        this.status = format!("{}: {error}", fs_ops::display_path(&path)).into();
                        #[cfg(feature = "sftp")]
                        if let Some(key) = crate::remote::hostkey::UnknownKey::from_error(&error) {
                            let entity = cx.entity().downgrade();
                            super::hostkey_view::confirm(
                                key,
                                window,
                                cx,
                                move |result, window, cx| {
                                    let _ = entity.update(cx, |this, cx| {
                                        if !this.navigation[panel].is_current(request) {
                                            return;
                                        }
                                        let live = this.folders[panel].read(cx);
                                        if !guard.matches(&live.target(), live.generation()) {
                                            return;
                                        }
                                        match result {
                                            Ok(()) => this.load_panel_listing(
                                                panel,
                                                target.clone(),
                                                commit.clone(),
                                                fallback,
                                                window,
                                                cx,
                                            ),
                                            Err(error) => {
                                                this.status =
                                                    format!("Cannot save SSH host key: {error}")
                                                        .into();
                                                cx.notify();
                                            }
                                        }
                                    });
                                },
                            );
                        }
                    }
                }
                cx.notify();
            });
        });
    }

    fn invalidate_git_scope(
        &mut self,
        scope: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.git.invalidate(scope);
        // A second panel may already show this scope without a listing change.
        for view in &self.views {
            view.update(cx, |view, cx| view.refresh_git(cx));
        }
        self.update_preview(window, cx);
    }

    /// Discovers locations in the background, then opens them for the panel
    /// that requested it: as a menu under its map button, or as the
    /// searchable list (Ctrl-G).
    pub fn open_locations(
        &mut self,
        panel: usize,
        as_menu: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_active(panel, cx);
        let bookmarks = self.bookmarks.clone();
        let frequent = self
            .history
            .ranked(places::now())
            .into_iter()
            .take(30)
            .collect::<Vec<_>>();
        let remotes = self.remote_places();
        // Show what is known now; a newer discovery serves the next opening.
        self.discover_remotes(window, cx);
        self.status = "Discovering locations…".into();
        self.locations_generation = self.locations_generation.wrapping_add(1);
        let request = self.locations_generation;
        self.locations_pending = true;
        cx.notify();
        self._locations_task = cx.spawn_in(window, async move |this, cx| {
            let groups = cx
                .background_spawn(async move {
                    places::grouped_locations_typed(&bookmarks, &frequent, remotes)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.locations_generation != request {
                    return;
                }
                this.locations_pending = false;
                if as_menu {
                    this.status = String::new().into();
                    this.views[panel]
                        .update(cx, |view, cx| view.show_locations_menu(groups, window, cx));
                } else {
                    this.show_locations(panel, groups, window, cx);
                }
            });
        });
    }

    /// Cancels pending breadcrumb navigation or location discovery. Returns
    /// whether Escape was consumed.
    pub fn cancel_location_work(&mut self, cx: &mut Context<Self>) -> bool {
        let mut cancelled = false;
        for panel in 0..self.navigation.len() {
            if self.navigation[panel].cancel() {
                self.cancel_remote(panel);
                self.navigation_tasks[panel] = Task::ready(());
                cancelled = true;
            }
        }
        if self.cancel_remote_check() {
            cancelled = true;
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
                if let Ok(target) = PanelTarget::from_panel_path(&path, None) {
                    this.navigate_panel(panel, target, window, cx);
                }
            });
        });
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        list.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::new(0)), window, cx)
        });
        let dialog_list = list.clone();
        let commander = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let commander = commander.clone();
            dialog.title("Locations").w(px(640.)).child(
                v_flex()
                    .gap_2()
                    .child(
                        div().h(px(420.)).child(
                            List::new(&dialog_list)
                                .search_placeholder("drives, places, bookmarks, frequent"),
                        ),
                    )
                    .child(
                        Button::new("manage-remotes-from-locations")
                            .label("Manage Remotes…")
                            .small()
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                let _ = commander.update(cx, |this, cx| {
                                    this.manage_remotes(&super::keys::ManageRemotes, window, cx)
                                });
                            }),
                    ),
            )
        });
        list.update(cx, |list, cx| list.focus(window, cx));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{ListingCommit, preserve_startup_diagnostics, successful_listing_status};

    #[test]
    fn startup_diagnostics_survive_only_a_clean_startup_listing() {
        let startup = ListingCommit::Startup;
        let navigate = ListingCommit::Navigate;
        let refresh = ListingCommit::Refresh;

        assert_eq!(
            successful_listing_status(true, &startup, "/home/user", None, String::new(),),
            None
        );
        assert_eq!(
            successful_listing_status(true, &navigate, "/home/user", None, String::new(),),
            Some("Opened /home/user".to_owned())
        );
        assert_eq!(
            successful_listing_status(true, &refresh, "/home/user", None, String::new(),),
            Some("Opened /home/user".to_owned())
        );
        assert_eq!(
            successful_listing_status(
                true,
                &startup,
                "/home/user",
                None,
                "permission warning".to_owned(),
            ),
            Some("permission warning".to_owned())
        );
        assert_eq!(
            successful_listing_status(
                true,
                &startup,
                "/home/user",
                Some("/home/user: permission denied".to_owned()),
                "other warning".to_owned(),
            ),
            Some("/home/user: permission denied".to_owned())
        );
        assert!(preserve_startup_diagnostics(true, &startup));
        assert!(!preserve_startup_diagnostics(false, &startup));
        assert!(!preserve_startup_diagnostics(true, &navigate));
    }
}
