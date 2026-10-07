//! Shift-Enter "Open with…": configured openers, the desktop's default and
//! associated applications, the system opener and the built-in editor.

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::list::{List, ListState};
use gpui_kit::component::{IndexPath, WindowExt as _};
use gpui_kit::*;

use super::Commander;
use super::keys::OpenWith;
use crate::apps::{self, Application, Catalog, Choice};
use crate::editor::Document;
use crate::open_with_view::OpenWithDelegate;
use crate::tools::{self, Programs};
use crate::xdg::Env;

/// How long the discovered application database is reused; F5 drops it
/// earlier. Short enough to notice a newly installed application.
const CATALOG_MAX_AGE: Duration = Duration::from_secs(60);

/// The application database and when it was read.
pub struct CatalogCache {
    catalog: Arc<Catalog>,
    loaded: Instant,
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
    /// the user choose how to open the cursor file.
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
        let openers: Vec<_> = crate::openers::matching(&self.openers, &path)
            .into_iter()
            .cloned()
            .collect();
        let cached = CatalogCache::fresh(&self.catalog_cache);
        let name = entry.name.clone();
        let file_name = name.clone();
        self.status = "Looking up applications…".into();
        cx.notify();
        self._open_with_task = cx.spawn_in(window, async move |this, cx| {
            let (catalog, apps) = cx
                .background_spawn(async move {
                    let reloaded = cached.is_none();
                    let catalog =
                        cached.unwrap_or_else(|| Arc::new(Catalog::load(&Env::from_env())));
                    // A new lookup per call, so installing a program shows up.
                    let programs = Programs::from_env();
                    let apps =
                        catalog.applications_for(&name, false, &|p| programs.find(p).is_some());
                    (reloaded.then_some(catalog), apps)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Some(catalog) = catalog {
                    this.catalog_cache = Some(CatalogCache {
                        catalog,
                        loaded: Instant::now(),
                    });
                }
                this.status = "".into();
                this.show_open_with(path, file_name, apps::merge(openers, apps), window, cx);
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
        let run = choices.clone();
        let delegate = OpenWithDelegate::new(choices, move |index, window, cx| {
            window.close_dialog(cx);
            let path = path.clone();
            let choice = run[index].clone();
            let _ = this.update(cx, |this, cx| {
                match choice {
                    Choice::Opener { opener, .. } => {
                        this.run_opener(&opener, &path);
                    }
                    Choice::App(app) => {
                        this.run_application(&app, &path);
                    }
                    Choice::SystemDefault => {
                        if let Err(e) = open::that_detached(&path) {
                            this.status = format!("Cannot open {}: {e}", path.display()).into();
                        }
                    }
                    Choice::BuiltInEditor => {
                        this.open_in_editor(path.clone(), Document::open(&path), window, cx)
                    }
                }
                cx.notify();
            });
        });
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        list.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::new(0)), window, cx)
        });
        let dialog_list = list.clone();
        let title = format!("Open {file_name} with");
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title(title.clone())
                .w(px(460.))
                .child(div().h(px(320.)).child(List::new(&dialog_list)))
        });
        list.update(cx, |list, cx| list.focus(window, cx));
        cx.notify();
    }
}
