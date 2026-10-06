//! Search in a panel: the query dialog and the background search.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, v_flex};
use gpui_kit::*;

use super::Commander;
use super::jobs::first_of;
use super::keys::FindFiles;
use crate::fs_ops;
use crate::search::{self, Query};

impl Commander {
    /// Alt-F7: find files by name and/or containing text, below the
    /// active panel's directory.
    pub fn find_files(&mut self, _: &FindFiles, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) {
            return;
        }
        let root = self.folders[self.active].read(cx).path().to_path_buf();
        if root.as_os_str().is_empty() {
            return;
        }
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. *.rs or report"));
        let content = cx.new(|cx| InputState::new(window, cx).placeholder("optional"));
        let this = cx.entity().downgrade();
        // Enter in either field starts the search.
        self._search_inputs = [&name, &content]
            .into_iter()
            .map(|input| {
                let (name, content, root) = (name.clone(), content.clone(), root.clone());
                cx.subscribe_in(
                    input,
                    window,
                    move |this, _, event: &InputEvent, window, cx| {
                        if let InputEvent::PressEnter { .. } = event {
                            window.close_dialog(cx);
                            let query = this.search_query(&root, &name, &content, cx);
                            this.start_search(query, window, cx);
                        }
                    },
                )
            })
            .collect();
        let (dialog_name, dialog_content) = (name.clone(), content.clone());
        let regex = self.search_regex.clone();
        let location = fs_ops::display_path(&root);
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let muted = cx.theme().muted_foreground;
            let (this, name, content, root) = (
                this.clone(),
                dialog_name.clone(),
                dialog_content.clone(),
                root.clone(),
            );
            dialog
                .title("Find files")
                .description(
                    v_flex()
                        .gap_3()
                        .w_full()
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("In {location}")),
                        )
                        .child(v_flex().gap_1().child("File name").child(Input::new(&name)))
                        .child(
                            v_flex()
                                .gap_1()
                                .child("Containing text")
                                .child(Input::new(&content)),
                        )
                        .child({
                            let regex = regex.clone();
                            Checkbox::new("search-regex")
                                .label("Regular expression")
                                .checked(regex.get())
                                .on_click(move |checked, window, _| {
                                    regex.set(*checked);
                                    window.refresh();
                                })
                        }),
                )
                .ok_text("Find")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let (name, content, root) = (name.clone(), content.clone(), root.clone());
                    let _ = this.update(cx, |this, cx| {
                        let query = this.search_query(&root, &name, &content, cx);
                        this.start_search(query, window, cx);
                    });
                    true
                })
        });
        name.update(cx, |input, cx| input.focus(window, cx));
    }

    pub fn search_query(
        &self,
        root: &Path,
        name: &Entity<InputState>,
        content: &Entity<InputState>,
        cx: &App,
    ) -> Query {
        Query {
            root: root.to_path_buf(),
            name: name.read(cx).value().to_string(),
            content: content.read(cx).value().to_string(),
            regex: self.search_regex.get(),
            hidden: self.show_hidden,
            ignore_files: true,
        }
    }

    /// Shows results in the active panel as they arrive.
    pub fn start_search(&mut self, query: Query, window: &mut Window, cx: &mut Context<Self>) {
        self._search_inputs.clear();
        if let Some(previous) = self.search_cancel.take() {
            previous.store(true, Ordering::Relaxed);
        }
        let mut title = match (query.name.trim(), query.content.trim()) {
            ("", "") => "all files".to_string(),
            (name, "") => format!("\"{name}\""),
            ("", text) if query.regex => format!("matching /{text}/"),
            ("", text) => format!("containing \"{text}\""),
            (name, text) if query.regex => format!("\"{name}\" matching /{text}/"),
            (name, text) => format!("\"{name}\" containing \"{text}\""),
        };
        title = format!("Search {title}");
        let ix = self.active;
        self.folders[ix].update(cx, |folder, cx| {
            folder.show_results(query.root.clone(), title.clone());
            cx.notify();
        });
        self.status = "Searching… (Esc stops)".into();
        cx.notify();

        let cancel = Arc::new(AtomicBool::new(false));
        self.search_cancel = Some(cancel.clone());
        let found: Arc<Mutex<Vec<search::Hit>>> = Arc::default();
        let (bg_found, bg_cancel) = (found.clone(), cancel.clone());
        self._search_task = cx.spawn_in(window, async move |this, cx| {
            let mut search = cx.background_spawn(async move {
                search::run(&query, &bg_cancel, |hit| {
                    bg_found
                        .lock()
                        .expect("search hits lock is never poisoned")
                        .push(hit)
                })
            });
            loop {
                let timer = cx.background_executor().timer(Duration::from_millis(120));
                let finished = first_of(&mut search, timer).await;
                let batch: Vec<_> = found
                    .lock()
                    .expect("search hits lock is never poisoned")
                    .drain(..)
                    .collect();
                let keep_going = this
                    .update(cx, |this, cx| {
                        let folder = &this.folders[ix];
                        // Stop if the user left the results.
                        if folder.read(cx).results() != Some(title.as_str()) {
                            cancel.store(true, Ordering::Relaxed);
                            return false;
                        }
                        if !batch.is_empty() {
                            folder.update(cx, |folder, cx| {
                                folder.add_results(batch);
                                cx.notify();
                            });
                        }
                        let count = folder.read(cx).unfiltered_count();
                        match &finished {
                            None => {
                                this.status =
                                    format!("Searching… {count} found (Esc stops)").into();
                            }
                            Some(Ok(summary)) => {
                                let note = if summary.cancelled {
                                    " (stopped)"
                                } else if summary.truncated {
                                    " (limit reached)"
                                } else {
                                    ""
                                };
                                this.status = format!("{title}: {count} found{note}").into();
                                this.search_cancel = None;
                            }
                            Some(Err(e)) => {
                                this.status = format!("Search: {e}").into();
                                this.search_cancel = None;
                            }
                        }
                        cx.notify();
                        finished.is_none()
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        });
    }
}
