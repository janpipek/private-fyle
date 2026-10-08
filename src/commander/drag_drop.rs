//! Mouse drops between panels and from desktop applications.

use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Commander;
use crate::jobs::{self, Extraction, JobKind};
use crate::location::Location;
use crate::model::DragSnapshot;
use crate::panel::PanelDrop;
use crate::panels;
use crate::remote::jobs::{Side, TransferPlan, plan_transfer};

#[derive(Clone)]
enum DropJob {
    Local {
        kind: JobKind,
        sources: Vec<PathBuf>,
        dest: PathBuf,
        source_ix: Option<usize>,
    },
    Remote {
        plan: TransferPlan,
        sources: Vec<Location>,
        dest: Location,
        source_ix: usize,
    },
    Extract {
        extraction: Extraction,
        dest: PathBuf,
        source_ix: usize,
    },
}

struct DropOffer {
    sources: Vec<Location>,
    source_ix: Option<usize>,
    source_snapshot: Option<DragSnapshot>,
    source_archive: Option<PathBuf>,
    source_side: Side,
    dest: Location,
}

impl Commander {
    /// Validates the captured source and offers the supported transfer actions.
    pub fn handle_panel_drop(
        &mut self,
        dest_ix: usize,
        event: &PanelDrop,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.navigation_pending[dest_ix] || self.reload_cancel[dest_ix].is_some() {
            self.drop_error("Destination folder is still loading", cx);
            return;
        }
        let row = match event {
            PanelDrop::Internal(_, row) | PanelDrop::External(_, row) => *row,
        };
        let dest = match panels::drop_directory(self.folders[dest_ix].read(cx), row) {
            Ok(dest) => dest,
            Err(reason) => {
                self.drop_error(reason, cx);
                return;
            }
        };
        let (sources, source_ix, source_snapshot, source_archive, source_side) = match event {
            PanelDrop::Internal(drag, _) => {
                let Some(source_ix) = self
                    .folders
                    .iter()
                    .position(|folder| folder.entity_id() == drag.folder.entity_id())
                else {
                    return;
                };
                if self.navigation_pending[source_ix]
                    || self.reload_cancel[source_ix].is_some()
                    || !drag.snapshot.is_current(self.folders[source_ix].read(cx))
                {
                    self.drop_error("Source folder changed during the drag", cx);
                    return;
                }
                let folder = self.folders[source_ix].read(cx);
                if folder.location() == Some(&dest) {
                    self.drop_error("Choose a different destination folder", cx);
                    return;
                }
                (
                    drag.snapshot.locations(),
                    Some(source_ix),
                    Some(drag.snapshot.clone()),
                    folder.archive().map(PathBuf::from),
                    Side::of(folder.location()),
                )
            }
            PanelDrop::External(paths, _) => {
                let sources = paths
                    .iter()
                    .map(|path| Location::local(path.clone()))
                    .collect::<Result<Vec<_>, _>>();
                let Ok(sources) = sources else {
                    self.drop_error("Only absolute local paths can be dropped here", cx);
                    return;
                };
                (sources, None, None, None, Side::Local)
            }
        };
        if sources.is_empty() {
            return;
        }
        let offer = DropOffer {
            sources,
            source_ix,
            source_snapshot,
            source_archive,
            source_side,
            dest,
        };
        let copy = self.plan_drop(&offer, true);
        let move_ = self.plan_drop(&offer, false);
        if let (Err(copy_error), Err(move_error)) = (&copy, &move_) {
            self.drop_error(&format!("Copy: {copy_error}; Move: {move_error}"), cx);
            return;
        }
        self.ask_drop_operation(offer, copy, move_, window, cx);
    }

    fn drop_error(&mut self, message: &str, cx: &mut Context<Self>) {
        self.status = message.to_owned().into();
        cx.notify();
    }

    fn plan_drop(&self, offer: &DropOffer, copy: bool) -> Result<DropJob, String> {
        let dest_path = offer.dest.panel_path();
        if let Some(file) = &offer.source_archive {
            if !copy {
                return Err("Archive contents cannot be moved".into());
            }
            let dest = offer
                .dest
                .as_local_path()
                .ok_or("Archive extraction needs a local folder")?;
            let wanted = offer
                .sources
                .iter()
                .map(|location| match location {
                    Location::Archive { source, member }
                        if source.as_local_path() == Some(file.as_path()) =>
                    {
                        Ok(member.as_str().to_owned())
                    }
                    _ => Err("Archive selection changed"),
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(DropJob::Extract {
                extraction: Extraction {
                    file: file.clone(),
                    wanted,
                    into_folder: false,
                },
                dest: dest.to_path_buf(),
                source_ix: offer.source_ix.expect("archive drag has a panel"),
            });
        }
        let dest_side = Side::of(Some(&offer.dest));
        let plan = plan_transfer(copy, &offer.source_side, &dest_side, |name| {
            self.remote_info(name)
        })
        .map_err(str::to_owned)?;
        if let Some(plan) = plan {
            return Ok(DropJob::Remote {
                plan,
                sources: offer.sources.clone(),
                dest: offer.dest.clone(),
                source_ix: offer.source_ix.unwrap_or(0),
            });
        }
        let dest = offer
            .dest
            .as_local_path()
            .ok_or("Destination is not a local folder")?;
        let sources = offer
            .sources
            .iter()
            .map(|location| {
                location
                    .as_local_path()
                    .map(PathBuf::from)
                    .ok_or_else(|| format!("{}: not a local file", location.panel_path().display()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if sources.iter().any(|source| source == dest) {
            return Err("Source and destination are the same folder".into());
        }
        if sources.iter().any(|source| dest.starts_with(source)) {
            return Err("Cannot copy or move a folder into itself".into());
        }
        Ok(DropJob::Local {
            kind: if copy { JobKind::Copy } else { JobKind::Move },
            sources,
            dest: dest_path,
            source_ix: offer.source_ix,
        })
    }

    fn ask_drop_operation(
        &mut self,
        offer: DropOffer,
        copy: Result<DropJob, String>,
        move_: Result<DropJob, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source_paths: Vec<_> = offer.sources.iter().map(Location::panel_path).collect();
        let description = format!(
            "{} → {}",
            super::files::describe(&source_paths),
            offer.dest.panel_path().display()
        );
        let explanations = [copy.as_ref().err(), move_.as_ref().err()]
            .into_iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>()
            .join(" · ");
        let this = cx.weak_entity();
        let source_ix = offer.source_ix;
        let source_snapshot = offer.source_snapshot;
        window.open_dialog(cx, move |dialog, _, cx| {
            let button = |label: &'static str, id: &'static str, job: &Result<DropJob, String>| {
                let this = this.clone();
                let chosen = job.as_ref().ok().cloned();
                let source_snapshot = source_snapshot.clone();
                Button::new(id)
                    .label(label)
                    .disabled(chosen.is_none())
                    .on_click(move |_, window, cx| {
                        let Some(job) = chosen.clone() else { return };
                        window.close_dialog(cx);
                        let _ = this.update(cx, |this, cx| {
                            if let (Some(ix), Some(snapshot)) = (source_ix, &source_snapshot)
                                && !snapshot.is_current(this.folders[ix].read(cx))
                            {
                                this.drop_error("Source folder changed during the drag", cx);
                                return;
                            }
                            this.start_drop_job(job, window, cx);
                        });
                    })
            };
            dialog.title("Drop files").child(
                v_flex()
                    .gap_3()
                    .child(description.clone())
                    .when(!explanations.is_empty(), |view| {
                        view.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(explanations.clone()),
                        )
                    })
                    .child(
                        h_flex()
                            .gap_2()
                            .child(button("Copy", "drop-copy", &copy).primary())
                            .child(button("Move", "drop-move", &move_).outline())
                            .child(
                                Button::new("drop-cancel")
                                    .label("Cancel")
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            ),
                    ),
            )
        });
    }

    fn start_drop_job(&mut self, job: DropJob, window: &mut Window, cx: &mut Context<Self>) {
        match job {
            DropJob::Local {
                kind,
                sources,
                dest,
                source_ix,
            } => {
                if let Some(ix) = source_ix {
                    self.folders[ix].update(cx, |folder, cx| {
                        folder.clear_marks();
                        cx.notify();
                    });
                }
                self.spawn_job(kind, None, window, cx, move |progress| {
                    jobs::run_interactive(kind, &sources, Some(&dest), progress)
                });
            }
            DropJob::Remote {
                plan,
                sources,
                dest,
                source_ix,
            } => self.start_remote_transfer(plan, sources, dest, source_ix, window, cx),
            DropJob::Extract {
                extraction,
                dest,
                source_ix,
            } => {
                self.folders[source_ix].update(cx, |folder, cx| {
                    folder.clear_marks();
                    cx.notify();
                });
                self.spawn_job(JobKind::Extract, None, window, cx, move |progress| {
                    jobs::run_extract(&extraction, &dest, progress, &mut |conflict| {
                        progress.resolve(conflict)
                    })
                });
            }
        }
        self.focus_active(window, cx);
    }
}
