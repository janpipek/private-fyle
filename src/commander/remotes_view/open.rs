//! Opens and connects configured remotes from the management dialog.

use super::*;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::list::ListState;
use gpui_kit::component::scroll::ScrollableElement as _;

impl Commander {
    /// Opens the remote management dialog after loading configuration off the UI thread.
    pub fn manage_remotes(
        &mut self,
        _: &super::super::keys::ManageRemotes,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = connections::default_path() else {
            self.status = "Cannot locate the user config directory".into();
            return;
        };
        let path_for_read = path.clone();
        let weak = cx.weak_entity();
        cx.spawn_in(window, async move |this, cx| {
            let loaded = cx
                .background_spawn(async move {
                    let baseline = manage::read(&path_for_read)?;
                    let (configs, errors) = connections::parse(&baseline)?;
                    Ok::<_, std::io::Error>((baseline, configs, errors))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match loaded {
                Ok((baseline, configs, errors)) => {
                    let path = path.clone();
                    let commander = weak.clone();
                    let fields = FIELDS
                        .iter()
                        .map(|label| {
                            let field = cx.new(|cx| InputState::new(window, cx).placeholder(*label));
                            if *label == "Password" {
                                field.update(cx, |field, cx| field.set_masked(true, window, cx));
                            }
                            field
                        })
                        .collect();
                    let draft = cx.new(|_| RemoteManager {
                        fields,
                        saved_configs: configs.clone(),
                        configs,
                        selected: None,
                        selected_original_name: None,
                        baseline,
                        path,
                        message: errors.first().map(ToString::to_string),
                        commander,
                        busy: false,
                        save_task: Task::ready(()),
                        test_task: Task::ready(()),
                        test_cancel: None,
                        test_generation: 0,
                        remote_list: None,
                        has_draft: false,
                        forget_password: false,
                        credential_status: "No saved password".to_owned(),
                        credential_status_generation: 0,
                        credential_task: Task::ready(()),
                        password_identity: None,
                        _input_subscriptions: Vec::new(),
                    });
                    let manager_weak = draft.downgrade();
                    let input_fields = draft.read(cx).fields.clone();
                    let subscriptions = input_fields.iter().enumerate().map(|(index, field)| {
                        let manager = manager_weak.clone();
                        cx.subscribe(field, move |_, _, event: &InputEvent, cx| {
                            if matches!(event, InputEvent::Change) {
                                let _ = manager.update(cx, |manager, cx| {
                                    manager.cancel_test();
                                    manager.message = None;
                                    if index == 12 {
                                        manager.password_identity = if manager.fields[12]
                                            .read(cx)
                                            .value()
                                            .is_empty()
                                        {
                                            None
                                        } else {
                                            manager.draft(cx).validate().ok()
                                        };
                                    } else if [2, 3, 4, 7, 8, 9, 10, 11].contains(&index)
                                    {
                                        let current = manager.draft(cx).validate().ok();
                                        let saved = manager
                                            .selected
                                            .and_then(|index| manager.configs.get(index));
                                        if saved.is_some_and(|saved| {
                                            saved.credential_id.is_some()
                                                && !current.as_ref().is_some_and(|current| {
                                                    saved.credential_identity_eq(current)
                                                })
                                        }) {
                                            manager.credential_status_generation = manager
                                                .credential_status_generation
                                                .wrapping_add(1);
                                            manager.credential_status =
                                                "Saved password belongs to the saved endpoint/account"
                                                    .to_owned();
                                        }
                                        if !manager.fields[12].read(cx).value().is_empty() {
                                        let same_identity = manager
                                            .password_identity
                                            .as_ref()
                                            .zip(current.as_ref())
                                            .is_some_and(|(typed_for, current)| {
                                                typed_for.credential_identity_eq(current)
                                            });
                                        if !same_identity {
                                            let password_field = manager.fields[12].clone();
                                            let window_id = cx.entity_id();
                                            let _ = cx.with_window(window_id, |window, cx| {
                                                password_field.update(cx, |field, cx| {
                                                    field.set_value(String::new(), window, cx)
                                                });
                                            });
                                            manager.password_identity = None;
                                            manager.message = Some(
                                                "Typed password cleared because the endpoint or account changed."
                                                    .to_owned(),
                                            );
                                        }
                                        }
                                    }
                                    cx.notify();
                                });
                            }
                        })
                    }).collect();
                    draft.update(cx, |draft, _| draft._input_subscriptions = subscriptions);
                    let remote_list = cx.new(|cx| {
                        ListState::new(
                            RemoteListDelegate {
                                manager: manager_weak,
                            },
                            window,
                            cx,
                        )
                    });
                    draft.update(cx, |draft, _| draft.remote_list = Some(remote_list.clone()));
                    if draft.read(cx).configs.is_empty() {
                        draft.update(cx, |draft, cx| {
                            draft.fields[1]
                                .update(cx, |field, cx| field.set_value("sftp", window, cx));
                            draft.fields[5]
                                .update(cx, |field, cx| field.set_value("/", window, cx));
                        });
                    } else {
                        draft.update(cx, |draft, cx| draft.load_config(0, window, cx));
                    }
                    defer_list_selection(remote_list, draft.read(cx).selected, window, cx);
                    let manager_entity = draft.downgrade();
                    window.open_dialog(cx, move |dialog, _, _| {
                        let manager_entity_cancel = manager_entity.clone();
                        dialog.title("Manage Remotes").w(px(890.)).on_cancel(move |_, window, cx| {
                            let dirty = manager_entity_cancel.upgrade().is_some_and(|manager| {
                                let manager = manager.read(cx);
                                manager.is_dirty(cx)
                            });
                            if dirty {
                                let manager_entity_discard = manager_entity_cancel.clone();
                                window.open_alert_dialog(cx, move |dialog, _, _| {
                                    let discard = manager_entity_discard.clone();
                                    dialog.title("Discard unsaved changes?")
                                        .description("Discard the current remote edits before closing the manager.")
                                        .ok_text("Discard changes")
                                        .show_cancel(true)
                                        .on_ok(move |_, window, cx| {
                                            let _ = discard.update(cx, |manager, cx| {
                                                manager.configs = manager.saved_configs.clone();
                                                if let Some(index) = manager.selected.filter(|index| *index < manager.configs.len()) {
                                                    manager.load_config(index, window, cx);
                                                } else {
                                                    manager.has_draft = false;
                                                    manager.selected = None;
                                                    manager.selected_original_name = None;
                                                    manager.forget_password = false;
                                                    manager.credential_status = "No saved password".to_owned();
                                                    manager.password_identity = None;
                                                    for field in &manager.fields {
                                                        field.update(cx, |field, cx| field.set_value(String::new(), window, cx));
                                                    }
                                                    manager.fields[1].update(cx, |field, cx| field.set_value("sftp", window, cx));
                                                    manager.fields[5].update(cx, |field, cx| field.set_value("/", window, cx));
                                                }
                                                cx.notify();
                                            });
                                            window.defer(cx, |window, cx| window.close_dialog(cx));
                                            true
                                        })
                                });
                                false
                            } else { true }
                        }).child(
                            div()
                                .max_h(px(650.))
                                .overflow_y_scrollbar()
                                .child(draft.clone()),
                        )
                    });
                }
                Err(error) => this.status = format!("remotes.toml: {error}").into(),
            });
        })
        .detach();
    }

    /// Refreshes providers and opens a saved remote at its root.
    pub fn connect_remote_name(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = std::path::PathBuf::from(format!("{name}:/"));
        let Ok(target) = crate::location::PanelTarget::from_panel_path(&path, None) else {
            return;
        };
        let panel = self.active;
        self.remotes_generation = self.remotes_generation.wrapping_add(1);
        let generation = self.remotes_generation;
        self.remotes_pending = true;
        self._remotes_task = cx.spawn_in(window, async move |this, cx| {
            let (registry, errors) = cx
                .background_spawn(async {
                    crate::remote::Registry::discover(&AtomicBool::new(false))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.remotes_generation != generation {
                    return;
                }
                this.remotes_pending = false;
                this.set_remotes(registry, errors, cx);
                this.navigate_panel(panel, target, window, cx);
            });
        });
    }
}
