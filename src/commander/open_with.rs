//! Shift-Enter "Open with…": configured openers, the desktop's default and
//! associated applications, the system opener and the built-in editor.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;

use super::Commander;
use super::keys::OpenWith;
use crate::apps::{self, Application, Catalog, Choice};
use crate::editor::Document;
use crate::tools::{self, Programs};
use crate::xdg::Env;

/// How long the discovered application database is reused; F5 drops it
/// earlier. Short enough to notice a newly installed application.
const CATALOG_MAX_AGE: Duration = Duration::from_secs(60);

/// The application database and when it was read.
pub struct CatalogCache {
    catalog: Arc<Catalog>,
    availability: HashMap<String, bool>,
    loaded: Instant,
}

impl CatalogCache {
    fn load() -> Self {
        let catalog = Arc::new(Catalog::load(&Env::from_env()));
        let programs = Programs::from_env();
        let availability = catalog
            .program_names()
            .into_iter()
            .map(|name| {
                let exists = programs.find(&name).is_some();
                (name, exists)
            })
            .collect();
        Self {
            catalog,
            availability,
            loaded: Instant::now(),
        }
    }

    fn application_list(&self, file_name: &str, is_dir: bool) -> Vec<Application> {
        self.catalog
            .applications_for(file_name, is_dir, &|program| {
                self.availability.get(program).copied().unwrap_or(false)
            })
    }
}

/// Adds available applications under group labels in a popup menu.
pub fn choice_menu(
    mut menu: PopupMenu,
    choices: Vec<Choice>,
    path: PathBuf,
    commander: WeakEntity<Commander>,
) -> PopupMenu {
    for group in [
        crate::apps::Group::Configured,
        crate::apps::Group::System,
        crate::apps::Group::Other,
    ] {
        let group_choices = choices.iter().filter(|choice| choice.group() == group);
        let mut added = false;
        for choice in group_choices {
            if !added {
                menu = menu.label(group.title());
                added = true;
            }
            let selected = choice.clone();
            let path = path.clone();
            let commander = commander.clone();
            menu = menu.item(
                PopupMenuItem::new(choice.label())
                    .disabled(choice.unavailable().is_some())
                    .on_click(move |_, window, cx| {
                        let _ = commander.update(cx, |this, cx| {
                            this.run_open_with_choice(selected.clone(), &path, window, cx)
                        });
                    }),
            );
        }
    }
    menu
}

/// Adds an Open With submenu to a parent entry menu.
pub fn add_choice_submenu(
    menu: PopupMenu,
    choices: Vec<Choice>,
    path: PathBuf,
    commander: WeakEntity<Commander>,
    window: &mut Window,
    cx: &mut App,
) -> PopupMenu {
    let submenu = PopupMenu::build(window, cx, move |submenu, _, _| {
        choice_menu(submenu, choices, path, commander)
    });
    menu.item(PopupMenuItem::submenu("Open With", submenu))
}

impl CatalogCache {
    fn fresh(cache: &Option<Self>) -> Option<Arc<Catalog>> {
        cache
            .as_ref()
            .filter(|c| c.loaded.elapsed() < CATALOG_MAX_AGE)
            .map(|c| c.catalog.clone())
    }
}

impl Commander {
    /// Current local entry and the Open With choices available in cached catalog data.
    pub fn open_with_menu_data(&self, cx: &App) -> Option<(std::path::PathBuf, Vec<Choice>)> {
        let folder = self.folders[self.active].read(cx);
        if folder.archive().is_some() || folder.is_remote() {
            return None;
        }
        let entry = folder.current()?.clone();
        if entry.is_parent {
            return None;
        }
        let path = entry.location.as_local_path()?.to_path_buf();
        let openers = crate::openers::matching_entry(&self.openers, &path, entry.is_dir)
            .into_iter()
            .cloned()
            .collect();
        let apps = self.catalog_cache.as_ref().map_or_else(Vec::new, |cache| {
            cache.application_list(&entry.name, entry.is_dir)
        });
        Some((path, apps::merge(openers, apps, entry.is_dir)))
    }

    /// Loads the desktop application catalog before menus need its choices.
    pub fn warm_open_with_catalog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_with_catalog_loading || CatalogCache::fresh(&self.catalog_cache).is_some() {
            return;
        }
        self.open_with_catalog_loading = true;
        self._open_with_task = cx.spawn_in(window, async move |this, cx| {
            let cache = cx
                .background_spawn(async move { CatalogCache::load() })
                .await;
            let _ = this.update_in(cx, |this, _, cx| {
                this.catalog_cache = Some(cache);
                this.open_with_catalog_loading = false;
                this.menu_state = None;
                cx.notify();
            });
        });
    }

    /// Runs one selected Open With choice for a file or directory.
    pub fn run_open_with_choice(
        &mut self,
        choice: Choice,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match choice {
            Choice::Opener { opener, .. } => {
                self.run_opener(&opener, path);
            }
            Choice::App(app) => {
                self.run_application(&app, path);
            }
            Choice::SystemDefault => {
                if let Err(e) = open::that_detached(path) {
                    self.status = format!("Cannot open {}: {e}", path.display()).into();
                }
            }
            Choice::BuiltInEditor => {
                self.open_in_editor(path.to_path_buf(), Document::open(path), window, cx)
            }
        }
        cx.notify();
    }

    /// Starts `app` on `path`; returns whether it started (else the status
    /// says why not). Terminal applications run in the configured terminal.
    pub fn run_application(&mut self, app: &Application, path: &Path) -> bool {
        let argv = match app.command_line(path) {
            Ok(argv) => argv,
            Err(e) => {
                self.status = format!("{}: {e}", path.display()).into();
                return false;
            }
        };
        let dir = path.parent().unwrap_or(Path::new("/"));
        let argv = if app.terminal {
            let env = std::env::var("TERMINAL").ok();
            let wrapped = crate::terminal::run_command(
                &self.terminal_command,
                env.as_deref(),
                dir,
                |name| self.programs.find(name).is_some(),
                &argv,
            );
            match wrapped {
                Some(argv) => argv,
                None => {
                    self.status = "No terminal found: set [terminal] command in config.toml".into();
                    return false;
                }
            }
        } else {
            argv
        };
        let Some((program, args)) = argv.split_first() else {
            return false;
        };
        let started = std::process::Command::new(program)
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match started {
            Ok(_) => {
                self.status = format!("Opened {} with {}", tools::file_name(path), app.name).into();
                true
            }
            Err(e) => {
                self.status = format!("{program}: {e}").into();
                false
            }
        }
    }

    /// Shift-Enter: looks the applications up in the background, then lets
    /// the user choose how to open the cursor entry.
    pub fn open_with(&mut self, _: &OpenWith, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) || self.refuse_on_remote(cx) {
            return;
        }
        let Some(entry) = self.folders[self.active].read(cx).current().cloned() else {
            return;
        };
        if entry.is_parent {
            return;
        }
        let Some(path) = entry.location.as_local_path().map(Path::to_path_buf) else {
            return;
        };
        let openers: Vec<_> = crate::openers::matching_entry(&self.openers, &path, entry.is_dir)
            .into_iter()
            .cloned()
            .collect();
        let cached = self
            .catalog_cache
            .as_ref()
            .filter(|cache| cache.loaded.elapsed() < CATALOG_MAX_AGE)
            .map(|cache| {
                (
                    cache.catalog.clone(),
                    cache.availability.clone(),
                    cache.loaded,
                )
            });
        let name = entry.name.clone();
        let file_name = name.clone();
        let is_dir = entry.is_dir;
        self.status = "Looking up applications…".into();
        cx.notify();
        self._open_with_task = cx.spawn_in(window, async move |this, cx| {
            let (cache, apps) = cx
                .background_spawn(async move {
                    let reloaded = cached.is_none();
                    let cache = cached.map_or_else(
                        CatalogCache::load,
                        |(catalog, availability, loaded)| CatalogCache {
                            catalog,
                            availability,
                            loaded,
                        },
                    );
                    let apps = cache.application_list(&name, is_dir);
                    (reloaded.then_some(cache), apps)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Some(cache) = cache {
                    this.catalog_cache = Some(cache);
                    this.open_with_catalog_loading = false;
                    this.menu_state = None;
                }
                this.status = "".into();
                this.show_open_with(
                    path,
                    file_name,
                    apps::merge(openers, apps, is_dir),
                    window,
                    cx,
                );
            });
        });
    }

    fn show_open_with(
        &mut self,
        path: std::path::PathBuf,
        file_name: String,
        choices: Vec<Choice>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        let file_name = format!("Open {file_name} with");
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            choice_menu(menu.label(file_name), choices, path, this)
        });
        self._open_with_dismiss =
            Some(
                cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, _, cx| {
                    this.open_with_menu = None;
                    this._open_with_dismiss = None;
                    cx.notify();
                }),
            );
        menu.focus_handle(cx).focus(window, cx);
        self.open_with_menu = Some(menu);
        cx.notify();
    }
}
