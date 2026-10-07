//! Remote folders in the commander: discovery of remotes, background
//! listing with cancellation, and copying, removing, renaming, creating and
//! opening on them. Decisions live in `remote::jobs`; this only wires them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::*;

use super::Commander;
use super::files::describe;
use super::jobs::Transfer;
use crate::jobs::JobKind;
use crate::listing::ListingService;
use crate::location::Location as TargetLocation;
use crate::menus::Cmd;
use crate::model::Folder;
use crate::remote::jobs::{self as rjobs, RemoteInfo, Side, TransferPlan};
use crate::remote::{Control, Location as RemoteLocation, Provider, Registry};
use crate::remote_labels::RemoteLabels;

/// Remote files up to this size are downloaded for the quick view
/// automatically; bigger ones only when opened.
const PREVIEW_LIMIT: u64 = 8 * 1024 * 1024;

/// The pre-check of a transfer's destination, running in the background.
pub struct RemoteCheck {
    cancel: Arc<AtomicBool>,
    _task: Task<()>,
}

/// The remote file the quick view shows from its downloaded copy.
#[derive(Default)]
pub struct RemotePreview {
    /// The remote path being fetched or shown, and the local copy once
    /// it arrived.
    shown: Option<(TargetLocation, Option<PathBuf>)>,
    cancel: Option<Arc<AtomicBool>>,
    task: Option<Task<()>>,
}

type Resolved = (Arc<dyn Provider>, RemoteLocation);

impl Commander {
    /// Looks for configured remotes in the background; the Locations list
    /// shows what was known meanwhile. Problems go to the status line once.
    pub fn discover_remotes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.remotes_generation = self.remotes_generation.wrapping_add(1);
        let generation = self.remotes_generation;
        self.remotes_pending = true;
        self._remotes_task = cx.spawn_in(window, async move |this, cx| {
            let (registry, errors) = cx
                .background_spawn(async { Registry::discover(&AtomicBool::new(false)) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.remotes_generation == generation {
                    this.remotes_pending = false;
                    this.set_remotes(registry, errors, cx);
                    this.open_start_remotes(window, cx);
                }
            });
        });
    }

    /// Opens the remote folders given as start paths (command line or
    /// settings) now that the remotes are known. An unknown remote leaves
    /// the panel on its fallback folder and says why on the status line.
    fn open_start_remotes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.active;
        for panel in 0..2 {
            if let Some(path) = self.start_remote[panel].take()
                && let Ok(target) = crate::location::PanelTarget::from_panel_path(&path, None)
            {
                self.navigate_panel(panel, target, window, cx);
            }
        }
        self.set_active(active, cx);
    }

    fn set_remotes(&mut self, registry: Registry, errors: Vec<io::Error>, cx: &mut Context<Self>) {
        for cache in &self.listing_caches {
            cache.clear();
        }
        let labels: HashMap<String, String> = registry
            .providers()
            .iter()
            .map(|p| (p.name().to_owned(), p.label().to_owned()))
            .collect();
        cx.set_global(RemoteLabels(labels));
        self.remotes = Arc::new(registry);
        let text = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        if !text.is_empty() && self.remote_errors != text {
            self.status = text.clone().into();
        }
        self.remote_errors = text;
        for view in &self.views {
            view.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }

    /// The abilities of the configured remote `name`.
    pub fn remote_info(&self, name: &str) -> Option<RemoteInfo> {
        let provider = self.remotes.get(name)?;
        Some(RemoteInfo {
            capabilities: provider.capabilities(),
        })
    }

    /// One Locations entry per configured remote.
    pub fn remote_places(&self) -> Vec<crate::places::Place> {
        self.remotes
            .providers()
            .iter()
            .map(|p| crate::places::Place::remote(p.name(), p.label()))
            .collect()
    }

    /// Resolves a typed remote address to its configured provider.
    pub fn resolve_remote(&self, address: &TargetLocation) -> Result<Resolved, String> {
        let path = address.panel_path();
        let error = |e: &dyn std::fmt::Display| {
            let pending = if self.remotes_pending {
                " (still looking for remotes)"
            } else {
                ""
            };
            format!("{}: {e}{pending}", path.display())
        };
        let TargetLocation::Remote(location) = address else {
            return Err(error(&"not a remote folder"));
        };
        let provider = self.remotes.get(location.remote()).ok_or_else(|| {
            error(&format!(
                "no remote named {} is configured",
                location.remote()
            ))
        })?;
        Ok((provider.clone(), location.clone()))
    }

    /// Stops the listing under way for `panel`, if any.
    pub fn cancel_remote(&mut self, panel: usize) {
        if let Some(cancel) = self.remote_cancel[panel].take() {
            cancel.store(true, Ordering::Relaxed);
        }
        if let Some(cancel) = self.reload_cancel[panel].take() {
            cancel.store(true, Ordering::Relaxed);
            self.reload_generation[panel] = self.reload_generation[panel].wrapping_add(1);
        }
    }

    /// Lists the remote folder of `panel` again in the background and
    /// refreshes it in place (cursor and marks stay). Does nothing for
    /// local folders.
    pub fn reload_remote(
        &mut self,
        panel: usize,
        selected: Option<TargetLocation>,
        cx: &mut Context<Self>,
    ) {
        // A mutation may finish while navigation is in flight. Invalidate
        // cached rows even if that navigation means this refresh cannot apply.
        self.listing_caches[panel].clear();
        // A navigation owns the panel until its listing completes. Applying a
        // remote reload in that interval would advance the folder generation
        // and make the newer navigation's guard reject its result.
        if self.navigation_pending[panel] {
            return;
        }
        let (path, listing, guard) = {
            let folder = self.folders[panel].read(cx);
            (
                folder.path(),
                folder.listing_request(folder.target(), false, false),
                crate::breadcrumbs::NavigationGuard::new(folder.target(), folder.generation()),
            )
        };
        if !self.folders[panel].read(cx).is_remote() {
            return;
        }
        self.cancel_remote(panel);
        self.reload_generation[panel] = self.reload_generation[panel].wrapping_add(1);
        let request = self.reload_generation[panel];
        let cancel = Arc::new(AtomicBool::new(false));
        self.reload_cancel[panel] = Some(cancel.clone());
        let mut listing = listing;
        listing.force_remote_refresh = true;
        let service =
            ListingService::with_cache(self.remotes.clone(), self.listing_caches[panel].clone());
        self.reload_tasks[panel] = cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { service.list(&listing, &cancel) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.reload_generation[panel] != request {
                    return;
                }
                this.reload_cancel[panel] = None;
                match result {
                    Ok(snapshot) => {
                        let warnings = snapshot.warnings.join("; ");
                        this.folders[panel].update(cx, |folder, cx| {
                            if !guard.matches(&folder.target(), folder.generation()) {
                                return;
                            }
                            folder.clear_sizes();
                            folder.apply_listing(snapshot, selected);
                            cx.notify();
                        });
                        if !warnings.is_empty() {
                            this.status = warnings.into();
                        }
                    }
                    Err(e) => this.status = format!("{}: {e}", path.display()).into(),
                }
                cx.notify();
            });
        });
    }

    /// Whether the active panel shows a remote folder; says so and returns
    /// true for commands that need local files.
    pub fn refuse_on_remote(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.folders[self.active].read(cx).is_remote() {
            return false;
        }
        self.status = "Not available on remote folders".into();
        cx.notify();
        true
    }

    /// The active remote folder with its provider.
    fn active_remote(&self, cx: &App) -> Result<Resolved, String> {
        let folder = self.folders[self.active].read(cx);
        folder
            .location()
            .map(|location| self.resolve_remote(location))
            .unwrap_or_else(|| Err("Drives: not a remote folder".into()))
    }

    /// Why `cmd` cannot run in the current state, as a status message.
    pub fn refuse_cmd(&mut self, cmd: Cmd, cx: &mut Context<Self>) -> bool {
        match cmd.unavailable(&self.menu_state(cx)) {
            Some(reason) => {
                self.status = reason.into();
                cx.notify();
                true
            }
            None => false,
        }
    }

    // --- copy and move -------------------------------------------------

    /// F5/F6 where a remote is involved: confirms, checks the destination
    /// for names that exist, and runs the transfer as a job.
    pub fn confirm_remote_transfer(
        &mut self,
        plan: TransferPlan,
        op: Transfer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sources = Rc::new(self.folders[self.active].read(cx).target_locations());
        let Some(dest) = self.folders[1 - self.active].read(cx).location().cloned() else {
            return;
        };
        if sources.is_empty() {
            return;
        }
        let dest_path = dest.panel_path();
        let verb = if op == Transfer::Copy { "Copy" } else { "Move" };
        let source_paths: Vec<_> = sources.iter().map(TargetLocation::panel_path).collect();
        let description = format!(
            "{verb} {} to {}?",
            describe(&source_paths),
            dest_path.display()
        );
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
                        this.start_remote_transfer(
                            plan,
                            sources.to_vec(),
                            dest.clone(),
                            window,
                            cx,
                        );
                        this.focus_active(window, cx);
                    });
                    true
                })
        });
    }

    fn prepare_transfer(
        &self,
        plan: TransferPlan,
        sources: Vec<TargetLocation>,
        dest: &TargetLocation,
        cx: &App,
    ) -> Result<rjobs::Transfer, String> {
        let source_folder = self.folders[self.active].read(cx);
        match plan {
            TransferPlan::Upload => {
                let (provider, dest) = self.resolve_remote(dest)?;
                let sources = sources
                    .iter()
                    .map(|location| {
                        location
                            .as_local_path()
                            .map(|path| path.to_path_buf())
                            .ok_or_else(|| {
                                format!(
                                    "{}: expected a local source",
                                    location.panel_path().display()
                                )
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rjobs::Transfer::Upload {
                    provider,
                    sources,
                    dest,
                })
            }
            TransferPlan::Download => {
                let items = source_folder.remote_items(&sources);
                let source = source_folder
                    .location()
                    .ok_or("not a remote source folder")?;
                let (provider, _) = self.resolve_remote(source)?;
                let dest = dest.as_local_path().ok_or_else(|| {
                    format!(
                        "{}: download destination is not local",
                        dest.panel_path().display()
                    )
                })?;
                Ok(rjobs::Transfer::Download {
                    provider,
                    items,
                    dest: dest.to_path_buf(),
                })
            }
            TransferPlan::MoveWithin => {
                let items = source_folder.remote_items(&sources);
                let source = source_folder
                    .location()
                    .ok_or("not a remote source folder")?;
                let (provider, _) = self.resolve_remote(source)?;
                let (_, dest) = self.resolve_remote(dest)?;
                Ok(rjobs::Transfer::Move {
                    provider,
                    items,
                    dest,
                })
            }
        }
    }

    fn start_remote_transfer(
        &mut self,
        plan: TransferPlan,
        sources: Vec<TargetLocation>,
        dest: TargetLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display_dest = dest.panel_path();
        let transfer = match self.prepare_transfer(plan, sources, &dest, cx) {
            Ok(transfer) if !transfer.is_empty() => transfer,
            Ok(_) => return,
            Err(message) => {
                self.status = message.into();
                cx.notify();
                return;
            }
        };
        self.status = format!("Checking {}…", display_dest.display()).into();
        cx.notify();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let existing = transfer.existing(&Control::new(&worker_cancel))?;
                    Ok::<_, io::Error>(transfer.skip_existing(&existing))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.remote_check = None;
                match result {
                    Ok((free, taken)) => {
                        this.ask_skip_existing(free, taken, &display_dest, window, cx)
                    }
                    Err(e) => this.status = format!("{}: {e}", display_dest.display()).into(),
                }
                cx.notify();
            });
        });
        self.remote_check = Some(RemoteCheck {
            cancel,
            _task: task,
        });
    }

    /// Providers never overwrite, so names that exist at the destination
    /// are skipped, after asking.
    fn ask_skip_existing(
        &mut self,
        free: rjobs::Transfer,
        taken: Vec<String>,
        dest: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if taken.is_empty() {
            self.run_remote_transfer(free, window, cx);
            return;
        }
        let shown = match taken.as_slice() {
            [one] => format!("\"{one}\" already exists"),
            [a, b] => format!("\"{a}\" and \"{b}\" already exist"),
            [a, b, rest @ ..] => format!("\"{a}\", \"{b}\" and {} more already exist", rest.len()),
            [] => String::new(),
        };
        if free.is_empty() {
            self.status = format!("{shown} in {}; nothing to do", dest.display()).into();
            return;
        }
        let description = format!(
            "{shown} in {}. Skip and continue with the rest?",
            dest.display()
        );
        let free = Rc::new(RefCell::new(Some(free)));
        let this = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (this, free) = (this.clone(), free.clone());
            dialog
                .title("Already exists")
                .description(description.clone())
                .ok_text("Skip existing")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    if let Some(free) = free.borrow_mut().take() {
                        let _ = this.update(cx, |this, cx| {
                            this.run_remote_transfer(free, window, cx);
                            this.focus_active(window, cx);
                        });
                    }
                    true
                })
        });
    }

    fn run_remote_transfer(
        &mut self,
        transfer: rjobs::Transfer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_folder(cx, Folder::clear_marks);
        let kind = transfer.job_kind();
        self.spawn_job(kind, None, window, cx, move |progress| {
            transfer.run(progress)
        });
    }

    /// Stops the destination check of a transfer, if one is running.
    pub fn cancel_remote_check(&mut self) -> bool {
        match self.remote_check.take() {
            Some(check) => {
                check.cancel.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    // --- delete, folders and rename -------------------------------------

    /// F8 / Shift-F8 on a remote folder: removes as the remote does.
    pub fn confirm_remote_delete(
        &mut self,
        permanent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cmd = if permanent {
            Cmd::DeletePermanently
        } else {
            Cmd::Delete
        };
        if self.refuse_cmd(cmd, cx) {
            return;
        }
        let Ok((provider, _)) = self.active_remote(cx) else {
            return;
        };
        let Some(removal) = provider.capabilities().removal else {
            return;
        };
        let targets = self.folders[self.active].read(cx).target_locations();
        if targets.is_empty() {
            return;
        }
        let items = Rc::new(self.folders[self.active].read(cx).remote_items(&targets));
        let target_paths: Vec<_> = targets.iter().map(TargetLocation::panel_path).collect();
        let (title, description, ok) =
            rjobs::removal_prompt(removal, &describe(&target_paths), provider.label());
        let this = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (this, items, provider) = (this.clone(), items.clone(), provider.clone());
            dialog
                .title(title)
                .description(description.clone())
                .ok_text(ok)
                .ok_variant(ButtonVariant::Danger)
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let (items, provider) = (items.to_vec(), provider.clone());
                    let _ = this.update(cx, |this, cx| {
                        this.update_folder(cx, Folder::clear_marks);
                        this.spawn_job(JobKind::Remove, None, window, cx, move |progress| {
                            rjobs::remove(provider.as_ref(), &items, progress)
                        });
                        this.focus_active(window, cx);
                    });
                    true
                })
        });
    }

    /// Creates the folder `name` in the active remote folder.
    pub fn create_remote_dir(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.refuse_cmd(Cmd::MakeDir, cx) {
            return;
        }
        let parent = match self.active_remote(cx) {
            Ok((_, location)) => location,
            Err(message) => {
                self.status = message.into();
                cx.notify();
                return;
            }
        };
        let child = parent.child(name).ok();
        let Some(child) = child else {
            self.status = format!(
                "{}: invalid folder name {name:?}",
                parent.panel_path().display()
            )
            .into();
            cx.notify();
            return;
        };
        let name = name.to_owned();
        self.remote_mutate(
            self.active,
            format!("Created {}", child.panel_path().display()),
            child.panel_path(),
            Some(TargetLocation::Remote(child.clone())),
            cx,
            move |provider, control| provider.create_folder(&parent, &name, control),
        );
    }

    /// Shift-F6 on a remote folder: renames the one target.
    pub fn rename_remote(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_cmd(Cmd::Rename, cx) {
            return;
        }
        let targets = self.folders[self.active].read(cx).target_locations();
        let [target] = targets.as_slice() else {
            self.status = "Rename on a remote folder works on one item at a time".into();
            cx.notify();
            return;
        };
        let Some(location) = (match target {
            TargetLocation::Remote(location) => Some(location.clone()),
            _ => None,
        }) else {
            return;
        };
        let current = location.name().unwrap_or_default().to_owned();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(current));
        let this = cx.entity().downgrade();
        let panel = self.active;
        let dialog_input = input.clone();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (this, input, location) = (this.clone(), dialog_input.clone(), location.clone());
            dialog
                .title("Rename")
                .width(px(520.))
                .description(v_flex().w_full().child(Input::new(&dialog_input)))
                .ok_text("Rename")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let name = input.read(cx).value().trim().to_string();
                    let _ = this.update(cx, |this, cx| {
                        this.apply_remote_rename(panel, &location, name, cx);
                        this.focus_active(window, cx);
                    });
                    true
                })
        });
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn apply_remote_rename(
        &mut self,
        panel: usize,
        location: &RemoteLocation,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let shown = location.panel_path();
        let renamed = location.parent().and_then(|p| p.child(&name).ok());
        let Some(renamed) = renamed else {
            self.status = format!("{}: invalid name {name:?}", shown.display()).into();
            cx.notify();
            return;
        };
        if renamed == *location {
            return;
        }
        let target = location.clone();
        self.remote_mutate(
            panel,
            format!("Renamed to {}", renamed.panel_path().display()),
            shown,
            Some(TargetLocation::Remote(renamed.clone())),
            cx,
            move |provider, control| provider.rename(&target, &name, control),
        );
    }

    /// Runs one change on the remote of `panel` in the background, then
    /// lists the folder again and selects `select`.
    fn remote_mutate(
        &mut self,
        panel: usize,
        done: String,
        target: PathBuf,
        select: Option<TargetLocation>,
        cx: &mut Context<Self>,
        work: impl FnOnce(&dyn Provider, &Control) -> io::Result<()> + Send + 'static,
    ) {
        let Some(folder_location) = self.folders[panel].read(cx).location().cloned() else {
            return;
        };
        let (provider, _) = match self.resolve_remote(&folder_location) {
            Ok(found) => found,
            Err(message) => {
                self.status = message.into();
                cx.notify();
                return;
            }
        };
        self.status = format!("{}…", target.display()).into();
        cx.notify();
        self._remote_op = cx.spawn(async move |this, cx| {
            let result: io::Result<()> = cx
                .background_spawn(async move {
                    let cancel = AtomicBool::new(false);
                    let control = Control::new(&cancel);
                    work(provider.as_ref(), &control)?;
                    Ok(())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.folders[panel].update(cx, |folder, cx| {
                            if folder.location() == Some(&folder_location) {
                                folder.clear_marks();
                            }
                            cx.notify();
                        });
                        this.reload_remote(panel, select, cx);
                        this.reload_remote(1 - panel, None, cx);
                        this.status = done.into();
                    }
                    Err(e) => this.status = format!("{}: {e}", target.display()).into(),
                }
                cx.notify();
            });
        });
    }

    // --- opening and previewing files -----------------------------------

    /// Enter on a remote file: downloads it into the cache in the
    /// background, then opens the copy as usual.
    pub fn open_remote_file(
        &mut self,
        address: TargetLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = address.panel_path();
        let (provider, location) = match self.resolve_remote(&address) {
            Ok(found) => found,
            Err(message) => {
                self.status = message.into();
                cx.notify();
                return;
            }
        };
        let Some(root) = rjobs::cache_root() else {
            self.status = format!("{}: no cache folder", path.display()).into();
            cx.notify();
            return;
        };
        let local = rjobs::cached_file(&root, &location);
        let items = [location];
        let name = crate::tools::file_name(&local);
        let after: super::jobs::AfterJob = Box::new(move |this, outcome, _, cx| {
            if outcome.done == 1 && this.open_local_file(&local) {
                this.status =
                    format!("Opened {name}: a downloaded copy, edits are not uploaded back").into();
                cx.notify();
            }
        });
        self.spawn_job_with(
            JobKind::Download,
            None,
            window,
            cx,
            move |progress| {
                rjobs::run_items(
                    &items,
                    |location| location.panel_path().display().to_string(),
                    progress,
                    |location, control| {
                        rjobs::fetch(provider.as_ref(), location, &root, control).map(drop)
                    },
                )
            },
            Some(after),
        );
    }

    /// For the quick view: the local copy of the remote file under the
    /// cursor, once downloaded. Starts the download (small files only).
    pub fn remote_preview_target(
        &mut self,
        address: TargetLocation,
        size: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<PathBuf> {
        let path = address.panel_path();
        if let Some((shown, local)) = &self.remote_preview.shown
            && *shown == address
        {
            return local.clone();
        }
        if let Some(cancel) = self.remote_preview.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.remote_preview.task = None;
        self.remote_preview.shown = Some((address.clone(), None));
        if !remote_preview_eligible(size) {
            return None;
        }
        let (provider, location) = self.resolve_remote(&address).ok()?;
        let root = rjobs::cache_root()?;
        let cancel = Arc::new(AtomicBool::new(false));
        self.remote_preview.cancel = Some(cancel.clone());
        self.remote_preview.task = Some(cx.spawn_in(window, async move |this, cx| {
            // Holding an arrow key must not start a download per row.
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            let fetch_path = path.clone();
            let fetch_address = address.clone();
            let result = cx
                .background_spawn(async move {
                    rjobs::fetch(provider.as_ref(), &location, &root, &Control::new(&cancel))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                let current = this.remote_preview.shown.as_ref().map(|(p, _)| p);
                if current != Some(&fetch_address) {
                    return;
                }
                match result {
                    Ok(local) => {
                        this.remote_preview.shown = Some((fetch_address, Some(local)));
                        this.update_preview(window, cx);
                    }
                    Err(e) => this.status = format!("{}: {e}", fetch_path.display()).into(),
                }
                cx.notify();
            });
        }));
        None
    }

    /// The remote sides of the two panels, for [`crate::menus::State`].
    pub fn panel_sides(&self, cx: &App) -> (Side, Side) {
        (
            Side::of(self.folders[self.active].read(cx).location()),
            Side::of(self.folders[1 - self.active].read(cx).location()),
        )
    }
}

/// Whether a remote quick view can safely download a file without measuring it first.
pub fn remote_preview_eligible(size: Option<u64>) -> bool {
    size.is_some_and(|size| size <= PREVIEW_LIMIT)
}

#[cfg(test)]
mod size_tests {
    use super::remote_preview_eligible;

    #[test]
    fn unknown_remote_size_is_not_an_unbounded_preview_download() {
        assert!(!remote_preview_eligible(None));
        assert!(remote_preview_eligible(Some(0)));
        assert!(remote_preview_eligible(Some(8 * 1024 * 1024)));
        assert!(!remote_preview_eligible(Some(8 * 1024 * 1024 + 1)));
    }
}
