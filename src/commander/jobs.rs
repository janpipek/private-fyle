//! Background jobs: starting, progress, conflicts and cancelling.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::progress::Progress as ProgressBar;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use super::Commander;
use super::files::summarize;
use super::keys::CancelJobs;
use crate::conflict_view;
use crate::jobs::{self, JobKind, Progress, Yank};
use crate::model::Folder;
use crate::transfer::{ConflictChoice, Decision, EntryInfo};

pub struct RunningJob {
    id: usize,
    kind: JobKind,
    pub progress: Arc<Progress>,
    _task: Task<()>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Transfer {
    Copy,
    Move,
}

pub fn conflict_card(title: &'static str, info: &EntryInfo, cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_2()
        .p_3()
        .border_1()
        .border_color(theme.border)
        .rounded_md()
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
        .children(
            conflict_view::details(info)
                .into_iter()
                .map(|(label, value)| {
                    v_flex()
                        .gap_0p5()
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(label),
                        )
                        .child(div().text_sm().child(value))
                }),
        )
}

/// Runs when a job ends, after its status was set; may replace the status.
pub type AfterJob =
    Box<dyn FnOnce(&mut Commander, &jobs::Outcome, &mut Window, &mut Context<Commander>)>;

/// Waits for `task` or `timer`, whichever comes first; returns the task's
/// result if it finished.
pub async fn first_of<T: Unpin + 'static>(task: &mut Task<T>, timer: Task<()>) -> Option<T> {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::Poll;
    let mut timer = timer;
    std::future::poll_fn(|cx| {
        if let Poll::Ready(value) = Pin::new(&mut *task).poll(cx) {
            return Poll::Ready(Some(value));
        }
        if Pin::new(&mut timer).poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}

impl Commander {
    /// Runs a copy/move/trash/delete in the background; progress shows in
    /// the jobs bar.
    pub fn start_job(
        &mut self,
        kind: JobKind,
        sources: Vec<PathBuf>,
        dest: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_job_inner(kind, sources, dest, None, window, cx);
    }

    pub fn start_job_inner(
        &mut self,
        kind: JobKind,
        sources: Vec<PathBuf>,
        dest: Option<PathBuf>,
        restore_yank: Option<(u64, Yank)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(kind, JobKind::Copy | JobKind::Move) {
            self.update_folder(cx, Folder::clear_marks);
        }
        self.spawn_job(kind, restore_yank, window, cx, move |progress| {
            jobs::run_interactive(kind, &sources, dest.as_deref(), progress)
        });
    }

    /// Runs `work` in the background as a job of `kind` and reports its
    /// outcome when done.
    pub fn spawn_job(
        &mut self,
        kind: JobKind,
        restore_yank: Option<(u64, Yank)>,
        window: &mut Window,
        cx: &mut Context<Self>,
        work: impl FnOnce(&Progress) -> jobs::Outcome + Send + 'static,
    ) {
        self.spawn_job_with(kind, restore_yank, window, cx, work, None);
    }

    /// Like [`Self::spawn_job`], then runs `after` with the outcome.
    pub fn spawn_job_with(
        &mut self,
        kind: JobKind,
        restore_yank: Option<(u64, Yank)>,
        window: &mut Window,
        cx: &mut Context<Self>,
        work: impl FnOnce(&Progress) -> jobs::Outcome + Send + 'static,
        after: Option<AfterJob>,
    ) {
        let progress = Arc::new(Progress::default());
        let id = self.next_job_id;
        self.next_job_id += 1;
        let worker = progress.clone();
        let restore_paths = restore_yank.as_ref().map(|(_, yank)| yank.paths.clone());
        let task = cx.spawn_in(window, async move |this, cx| {
            let (outcome, existing_yank_paths) = cx
                .background_spawn(async move {
                    let outcome = work(&worker);
                    let existing = restore_paths.map(|paths| {
                        paths
                            .into_iter()
                            .filter(|path| std::fs::symlink_metadata(path).is_ok())
                            .collect::<Vec<_>>()
                    });
                    (outcome, existing)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.jobs.retain(|job| job.id != id);
                if this.active_conflict.is_some_and(|active| active.0 == id) {
                    this.active_conflict = None;
                    window.close_dialog(cx);
                }
                if let Some((generation, mut yank)) = restore_yank {
                    yank.paths = existing_yank_paths.unwrap_or_default();
                    if this.yank_generation == generation && this.yank.is_none() {
                        this.yank = (!yank.paths.is_empty()).then_some(yank);
                    }
                }
                this.reload_all(window, cx);
                let mut status =
                    summarize(kind.verb().1, kind.unit(), outcome.done, &outcome.errors);
                if outcome.skipped > 0 {
                    status += &format!(", {} skipped", outcome.skipped);
                }
                if outcome.cancelled {
                    status += " (cancelled)";
                }
                if kind == JobKind::Trash && !outcome.errors.is_empty() {
                    status += " (Shift-F8 deletes permanently)";
                }
                this.status = status.into();
                if let Some(after) = after {
                    after(this, &outcome, window, cx);
                }
                cx.notify();
            });
        });
        self.jobs.push(RunningJob {
            id,
            kind,
            progress,
            _task: task,
        });
        self.ensure_jobs_ticker(window, cx);
        cx.notify();
    }

    /// Repaints the jobs bar while jobs run.
    pub fn ensure_jobs_ticker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.jobs_ticking {
            return;
        }
        self.jobs_ticking = true;
        self._jobs_ticker = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
                let running = this
                    .update_in(cx, |this, window, cx| {
                        this.show_pending_conflict(window, cx);
                        cx.notify();
                        this.jobs_ticking = !this.jobs.is_empty();
                        this.jobs_ticking
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        });
    }

    pub fn show_pending_conflict(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_conflict.is_some() || window.has_active_dialog(cx) {
            return;
        }
        let Some((job_id, progress, request)) = self.jobs.iter().find_map(|job| {
            job.progress
                .pending_conflict()
                .map(|request| (job.id, job.progress.clone(), request))
        }) else {
            return;
        };
        self.active_conflict = Some((job_id, request.id));
        let conflict = request.conflict.clone();
        let request_id = request.id;
        let apply_to_all = Rc::new(Cell::new(false));
        let this = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let choice_button = |label: &'static str, choice: ConflictChoice| {
                let (this, progress, apply_to_all) =
                    (this.clone(), progress.clone(), apply_to_all.clone());
                Button::new(SharedString::from(format!(
                    "conflict-{job_id}-{request_id}-{choice:?}"
                )))
                .label(label)
                .small()
                .outline()
                .on_click(move |_, window, cx| {
                    let decision = Decision {
                        choice,
                        apply_to_all: apply_to_all.get(),
                    };
                    match progress.respond(request_id, decision) {
                        Ok(()) => {
                            let _ = this.update(cx, |this, cx| {
                                if this.active_conflict == Some((job_id, request_id)) {
                                    this.active_conflict = None;
                                    window.close_dialog(cx);
                                }
                            });
                        }
                        Err(error) => {
                            let _ = this.update(cx, |this, cx| {
                                this.status = format!("Conflict: {error}").into();
                                cx.notify();
                            });
                        }
                    }
                })
            };
            let cancel_this = this.clone();
            let cancel_progress = progress.clone();
            let cancel = Button::new(SharedString::from(format!(
                "conflict-{job_id}-{request_id}-cancel"
            )))
            .label("Cancel operation")
            .small()
            .danger()
            .on_click(move |_, window, cx| {
                cancel_progress.cancel();
                let _ = cancel_this.update(cx, |this, cx| {
                    if this.active_conflict == Some((job_id, request_id)) {
                        this.active_conflict = None;
                        window.close_dialog(cx);
                    }
                });
            });
            let checkbox_state = apply_to_all.clone();
            let mut choices = h_flex()
                .flex_wrap()
                .gap_2()
                .child(choice_button("Skip", ConflictChoice::Skip))
                .child(choice_button("Keep both", ConflictChoice::KeepBoth));
            if conflict.allows(ConflictChoice::Replace) {
                choices = choices.child(choice_button("Replace", ConflictChoice::Replace));
            }
            if conflict.allows(ConflictChoice::Merge) {
                choices = choices.child(choice_button("Merge", ConflictChoice::Merge));
            }
            choices = choices.child(cancel);

            let cancel_this = this.clone();
            let cancel_progress = progress.clone();
            let close_this = this.clone();
            let close_progress = progress.clone();
            dialog
                .title("File conflict")
                .w(px(780.))
                .child(
                    v_flex()
                        .gap_4()
                        .child("The destination already contains an entry with this name.")
                        .child(
                            h_flex()
                                .items_start()
                                .gap_3()
                                .child(conflict_card("Source", &conflict.source, cx))
                                .child(conflict_card("Destination", &conflict.destination, cx)),
                        ),
                )
                .footer(
                    v_flex()
                        .w_full()
                        .gap_3()
                        .child(
                            Checkbox::new(SharedString::from(format!(
                                "conflict-{job_id}-{request_id}-all"
                            )))
                            .label("Apply to remaining conflicts of this type")
                            .checked(apply_to_all.get())
                            .on_click(move |checked, window, _| {
                                checkbox_state.set(*checked);
                                window.refresh();
                            }),
                        )
                        .child(choices),
                )
                .on_cancel(move |_, _, cx| {
                    cancel_progress.cancel();
                    let _ = cancel_this.update(cx, |this, _| {
                        if this.active_conflict == Some((job_id, request_id)) {
                            this.active_conflict = None;
                        }
                    });
                    true
                })
                .on_close(move |_, _, cx| {
                    let _ = close_this.update(cx, |this, _| {
                        if this.active_conflict == Some((job_id, request_id)) {
                            close_progress.cancel();
                            this.active_conflict = None;
                        }
                    });
                })
        });
    }

    pub fn cancel_jobs(&mut self, _: &CancelJobs, window: &mut Window, cx: &mut Context<Self>) {
        for ix in 0..2 {
            self.stop_size_scan(ix);
        }
        for job in &self.jobs {
            job.progress.cancel();
        }
        if self.active_conflict.take().is_some() {
            window.close_dialog(cx);
        }
        cx.notify();
    }

    pub fn render_jobs(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.jobs.is_empty() {
            return None;
        }
        let theme = cx.theme();
        Some(
            v_flex()
                .gap_1()
                .px_2()
                .py_1()
                .border_t_1()
                .border_color(theme.border)
                .children(self.jobs.iter().map(|job| {
                    let this = cx.weak_entity();
                    let job_id = job.id;
                    let p = &job.progress;
                    let (done, total) = (
                        p.done.load(Ordering::Relaxed),
                        p.total.load(Ordering::Relaxed),
                    );
                    let current = p
                        .current
                        .lock()
                        .expect("progress lock is never poisoned")
                        .clone();
                    let cancelling = p.cancelled();
                    let progress = job.progress.clone();
                    h_flex()
                        .gap_3()
                        .text_xs()
                        .child(div().w(px(120.)).flex_none().child(job.kind.verb().0))
                        .child(
                            div().w(px(160.)).flex_none().child(
                                ProgressBar::new(("job", job.id)).value(p.fraction() * 100.),
                            ),
                        )
                        .child(div().flex_none().child(format!("{done}/{total}")))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.muted_foreground)
                                .child(if cancelling {
                                    "cancelling…".to_string()
                                } else {
                                    current
                                }),
                        )
                        .child(
                            Button::new(("cancel-job", job.id))
                                .label("Cancel")
                                .xsmall()
                                .ghost()
                                .on_click(move |_, window, cx| {
                                    progress.cancel();
                                    let _ = this.update(cx, |this, cx| {
                                        if this
                                            .active_conflict
                                            .is_some_and(|active| active.0 == job_id)
                                        {
                                            this.active_conflict = None;
                                            window.close_dialog(cx);
                                        }
                                    });
                                }),
                        )
                })),
        )
    }
}
