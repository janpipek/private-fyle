//! Dialog for editing user configured remote connections.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use super::Commander;
use crate::remote::{Control, Location, Provider, connections, manage};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const FIELDS: [&str; 12] = [
    "Name",
    "Protocol (sftp/ftp/ftps/webdav/s3)",
    "Host",
    "Port",
    "User",
    "Root",
    "Password command",
    "WebDAV URL",
    "S3 bucket",
    "S3 region",
    "S3 profile",
    "S3 endpoint",
];

/// Remote list and editable form shown in the manager dialog.
pub struct RemoteManager {
    fields: Vec<Entity<InputState>>,
    configs: Vec<connections::RemoteConfig>,
    saved_configs: Vec<connections::RemoteConfig>,
    selected: Option<usize>,
    selected_original_name: Option<String>,
    baseline: String,
    path: std::path::PathBuf,
    message: Option<String>,
    commander: WeakEntity<Commander>,
    busy: bool,
    save_task: Task<()>,
    test_task: Task<()>,
    test_cancel: Option<Arc<AtomicBool>>,
    test_generation: u64,
}

impl RemoteManager {
    fn draft(&self, cx: &App) -> manage::Draft {
        let values: Vec<String> = self
            .fields
            .iter()
            .map(|field| field.read(cx).value().to_string())
            .collect();
        manage::Draft {
            name: values[0].clone(),
            protocol: values[1].clone(),
            host: values[2].clone(),
            port: values[3].clone(),
            user: values[4].clone(),
            root: values[5].clone(),
            password_command: values[6].clone(),
            url: values[7].clone(),
            bucket: values[8].clone(),
            region: values[9].clone(),
            profile: values[10].clone(),
            endpoint: values[11].clone(),
        }
    }

    fn load_config(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_test();
        self.selected = Some(index);
        self.selected_original_name = Some(self.configs[index].name.clone());
        let draft = manage::Draft::from_config(&self.configs[index]);
        let values = [
            draft.name,
            draft.protocol,
            draft.host,
            draft.port,
            draft.user,
            draft.root,
            draft.password_command,
            draft.url,
            draft.bucket,
            draft.region,
            draft.profile,
            draft.endpoint,
        ];
        for (field, value) in self.fields.iter().zip(values) {
            field.update(cx, |field, cx| field.set_value(value, window, cx));
        }
        cx.notify();
    }
}

impl Render for RemoteManager {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let remotes = self.configs.iter().enumerate().map(|(index, config)| {
            let entity = weak.clone();
            let name = config.name.clone();
            let button = Button::new(SharedString::from(format!("remote-{name}")))
                .label(name)
                .small()
                .disabled(self.busy)
                .on_click(move |_, window, cx| {
                    let _ = entity.update(cx, |this, cx| this.load_config(index, window, cx));
                });
            if self.selected == Some(index) {
                button.primary()
            } else {
                button
            }
        });
        let field_view = |label: &str, field: &Entity<InputState>| {
            v_flex()
                .gap_1()
                .child(div().text_xs().child(label.to_owned()))
                .child(Input::new(field).disabled(self.busy))
        };
        let left_fields = FIELDS
            .iter()
            .step_by(2)
            .zip(self.fields.iter().step_by(2))
            .map(|(label, field)| field_view(label, field));
        let right_fields = FIELDS
            .iter()
            .skip(1)
            .step_by(2)
            .zip(self.fields.iter().skip(1).step_by(2))
            .map(|(label, field)| field_view(label, field));
        let mut content = v_flex().gap_3().child(div().child("Manage remotes.toml. rclone remotes use rclone config; Proton Drive needs proton-drive auth login."));
        if let Some(message) = &self.message {
            content = content.child(div().child(message.clone()));
        }
        let refresh = weak.clone();
        let add = Button::new("remote-add")
            .label("New")
            .small()
            .disabled(self.busy)
            .on_click(move |_, window, cx| {
                let _ = refresh.update(cx, |this, cx| {
                    this.cancel_test();
                    this.selected = None;
                    this.selected_original_name = None;
                    for field in &this.fields {
                        field.update(cx, |f, cx| f.set_value("".to_owned(), window, cx));
                    }
                    this.fields[1].update(cx, |f, cx| f.set_value("sftp".to_owned(), window, cx));
                    this.fields[5].update(cx, |f, cx| f.set_value("/".to_owned(), window, cx));
                    cx.notify();
                });
            });
        let del = weak.clone();
        let remove = Button::new("remote-remove")
            .label("Remove…")
            .small()
            .disabled(self.selected.is_none() || self.busy)
            .on_click(move |_, window, cx| {
                let Some(entity) = del.upgrade() else {
                    return;
                };
                let index = entity.read(cx).selected;
                let Some(index) = index else {
                    return;
                };
                let name = entity.read(cx).configs[index].name.clone();
                let entity2 = del.clone();
                window.open_alert_dialog(cx, move |dialog, _, _| {
                    let entity2 = entity2.clone();
                    dialog
                        .title("Remove remote?")
                        .description(format!(
                            "Remove {name} from remotes.toml? This does not delete server files."
                        ))
                        .ok_text("Remove")
                        .show_cancel(true)
                        .on_ok(move |_, window, cx| {
                            let _ = entity2.update(cx, |this, cx| {
                                this.cancel_test();
                                this.configs.remove(index);
                                this.selected = None;
                                this.selected_original_name = None;
                                this.persist(window, cx);
                            });
                            true
                        })
                });
            });
        let save_entity = weak.clone();
        let save = Button::new("remote-save")
            .label("Save")
            .small()
            .primary()
            .disabled(self.busy)
            .on_click(move |_, window, cx| {
                let _ = save_entity.update(cx, |this, cx| this.save(window, cx));
            });
        let test_entity = weak.clone();
        let test = Button::new("remote-test")
            .label("Test")
            .small()
            .disabled(self.busy)
            .on_click(move |_, _, cx| {
                let _ = test_entity.update(cx, |this, cx| this.test(cx));
            });
        let connect_entity = weak.clone();
        let connect = Button::new("remote-connect")
            .label("Connect saved")
            .small()
            .disabled(self.selected.is_none() || self.busy)
            .on_click(move |_, window, cx| {
                let Some(entity) = connect_entity.upgrade() else {
                    return;
                };
                let (index, name, commander) = {
                    let manager = entity.read(cx);
                    let index = manager.selected;
                    let name = index.map(|index| manager.configs[index].name.clone());
                    (index, name, manager.commander.clone())
                };
                let (Some(_), Some(name)) = (index, name) else {
                    return;
                };
                window.close_dialog(cx);
                if let Some(commander) = commander.upgrade() {
                    commander.update(cx, |this, cx| this.connect_remote_name(name, window, cx));
                }
            });
        content.child(
            h_flex()
                .gap_2()
                .child(
                    v_flex()
                        .w(px(190.))
                        .gap_2()
                        .children(remotes)
                        .child(add)
                        .child(remove),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap_2()
                        .child(
                            div().max_h(px(400.)).overflow_y_scrollbar().child(
                                h_flex()
                                    .gap_3()
                                    .child(v_flex().flex_1().gap_2().children(left_fields))
                                    .child(v_flex().flex_1().gap_2().children(right_fields)),
                            ),
                        )
                        .child(h_flex().gap_2().child(save).child(test).child(connect)),
                ),
        )
    }
}

impl Drop for RemoteManager {
    fn drop(&mut self) {
        if let Some(cancel) = self.test_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl RemoteManager {
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.draft(cx);
        let config = match draft.validate() {
            Ok(config) => config,
            Err(error) => {
                self.message = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        let old_index = self.selected;
        if self
            .configs
            .iter()
            .enumerate()
            .any(|(i, current)| current.name == config.name && Some(i) != old_index)
        {
            self.message = Some(format!("remote {}: name is used twice", config.name));
            cx.notify();
            return;
        }
        if let Some(index) = old_index {
            self.configs[index] = config;
        } else {
            self.configs.push(config);
            self.selected = Some(self.configs.len() - 1);
        }
        self.persist(window, cx);
    }

    fn persist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.busy = true;
        let path = self.path.clone();
        let baseline = self.baseline.clone();
        let configs = self.configs.clone();
        let rename = self.selected_original_name.as_ref().and_then(|old_name| {
            self.selected
                .and_then(|index| {
                    self.configs
                        .get(index)
                        .map(|config| (old_name.clone(), config.name.clone()))
                })
                .filter(|(old, new)| old != new)
        });
        let path_for_read = path.clone();
        let entity = cx.entity().downgrade();
        self.save_task = cx.spawn_in(window, async move |_, cx| {
            let result = cx
                .background_spawn(async move {
                    manage::save_with_rename(
                        &path,
                        &baseline,
                        &configs,
                        rename
                            .as_ref()
                            .map(|(old, new)| (old.as_str(), new.as_str())),
                    )
                    .and_then(|()| manage::read(&path_for_read))
                    .map_err(|e| format!("{}: {e}", path.display()))
                })
                .await;
            let _ = entity.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(new_baseline) => {
                        this.baseline = new_baseline;
                        this.saved_configs = this.configs.clone();
                        if this.selected.is_none() {
                            if !this.configs.is_empty() {
                                this.load_config(0, window, cx);
                            } else {
                                for field in &this.fields {
                                    field.update(cx, |field, cx| {
                                        field.set_value(String::new(), window, cx)
                                    });
                                }
                                this.fields[1]
                                    .update(cx, |field, cx| field.set_value("sftp", window, cx));
                                this.fields[5]
                                    .update(cx, |field, cx| field.set_value("/", window, cx));
                                this.selected_original_name = None;
                            }
                        } else {
                            this.selected_original_name = this.selected.and_then(|index| {
                                this.configs.get(index).map(|config| config.name.clone())
                            });
                        }
                        this.message = Some("Remotes saved".into());
                        if let Some(commander) = this.commander.upgrade() {
                            commander
                                .update(cx, |commander, cx| commander.discover_remotes(window, cx));
                        }
                    }
                    Err(error) => {
                        this.configs = this.saved_configs.clone();
                        if this
                            .selected
                            .is_some_and(|index| index >= this.configs.len())
                        {
                            this.selected = None;
                        }
                        this.selected_original_name = this.selected.and_then(|index| {
                            this.configs.get(index).map(|config| config.name.clone())
                        });
                        this.message = Some(error);
                    }
                };
                cx.notify();
            });
        });
    }

    fn test(&mut self, cx: &mut Context<Self>) {
        self.cancel_test();
        self.busy = true;
        self.message = Some("Testing connection…".into());
        let config = match self.draft(cx).validate() {
            Ok(config) => config,
            Err(error) => {
                self.busy = false;
                self.message = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        let entity = cx.entity().downgrade();
        self.test_generation = self.test_generation.wrapping_add(1);
        let generation = self.test_generation;
        let cancel = Arc::new(AtomicBool::new(false));
        self.test_cancel = Some(cancel.clone());
        let tested_config = config.clone();
        self.test_task = cx.spawn(async move |_, cx| {
            let result = cx
                .background_spawn(async move {
                    let provider = crate::remote::remotefs::RemoteFsProvider::new(config.clone());
                    let root = Location::root(&config.name)?;
                    provider
                        .list(&root, &Control::new(&cancel))
                        .map(|listing| listing.entries.len())
                })
                .await;
            let _ = entity.update(cx, |this, cx| {
                if this.test_generation != generation {
                    return;
                }
                this.busy = false;
                this.test_cancel = None;
                if !this
                    .draft(cx)
                    .validate()
                    .is_ok_and(|current| current == tested_config)
                {
                    cx.notify();
                    return;
                }
                this.message = Some(match result {
                    Ok(count) => format!("Connection works; {count} items at the root"),
                    Err(error) => format!("{}: connection failed: {error}", tested_config.name),
                });
                cx.notify();
            });
        });
    }

    fn cancel_test(&mut self) {
        self.test_generation = self.test_generation.wrapping_add(1);
        if let Some(cancel) = self.test_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
            self.busy = false;
        }
    }
}

impl Commander {
    /// Opens the remote management dialog after loading configuration off the UI thread.
    pub fn manage_remotes(
        &mut self,
        _: &super::keys::ManageRemotes,
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
                        .map(|label| cx.new(|cx| InputState::new(window, cx).placeholder(*label)))
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
                    });
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
                    window.open_dialog(cx, move |dialog, _, _| {
                        dialog.title("Manage Remotes").w(px(850.)).child(
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
