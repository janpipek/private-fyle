//! Dialog for editing user configured remote connections.

use gpui_kit::component::input::InputState;
use gpui_kit::component::list::{ListDelegate, ListItem, ListState};
use gpui_kit::component::{ActiveTheme as _, IndexPath, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use super::Commander;
use crate::remote::credentials::{CredentialStore as _, SystemCredentialStore};
use crate::remote::{connections, manage};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

mod connection_test;

const FIELDS: [&str; 13] = [
    "Name",
    "Protocol",
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
    "Password",
];

fn protocol_title(protocol: &str) -> &'static str {
    match protocol {
        "sftp" => "SFTP",
        "ftp" => "FTP",
        "ftps" => "FTPS",
        "webdav" => "WebDAV",
        "s3" => "S3",
        _ => "Choose type",
    }
}

fn defer_list_selection(
    list: Entity<ListState<RemoteListDelegate>>,
    index: Option<usize>,
    window: &mut Window,
    cx: &mut App,
) {
    window.defer(cx, move |window, cx| {
        list.update(cx, |list, cx| {
            list.set_selected_index(index.map(IndexPath::new), window, cx)
        });
    });
}

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
    remote_list: Option<Entity<ListState<RemoteListDelegate>>>,
    has_draft: bool,
    forget_password: bool,
    credential_status: String,
    credential_status_generation: u64,
    credential_task: Task<()>,
    password_identity: Option<connections::RemoteConfig>,
    _input_subscriptions: Vec<Subscription>,
}

struct RemoteListDelegate {
    manager: WeakEntity<RemoteManager>,
}

enum CredentialChange {
    Keep,
    Replace(String),
    Forget,
}

impl ListDelegate for RemoteListDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        _: &str,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        Task::ready(())
    }

    fn items_count(&self, _: usize, cx: &App) -> usize {
        self.manager
            .upgrade()
            .map_or(0, |manager| manager.read(cx).configs.len())
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let manager = self.manager.upgrade()?;
        let manager = manager.read(cx);
        let config = manager.configs.get(ix.row)?;
        let protocol = match config.protocol {
            connections::Protocol::Sftp => "SFTP",
            connections::Protocol::Ftp => "FTP",
            connections::Protocol::Ftps => "FTPS",
            connections::Protocol::Webdav => "WebDAV",
            connections::Protocol::S3 => "S3",
        };
        let endpoint = config
            .host
            .as_deref()
            .or(config.url.as_deref())
            .or(config.bucket.as_deref())
            .unwrap_or("Remote endpoint not set");
        Some(
            ListItem::new(ix.row)
                .selected(manager.selected == Some(ix.row))
                .child(
                    v_flex()
                        .w_full()
                        .gap_1()
                        .px_2()
                        .py_1()
                        .child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .child(config.name.clone())
                                .child(div().text_xs().child(protocol)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .min_w_0()
                                .truncate()
                                .child(endpoint.to_owned()),
                        ),
                ),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        if let (Some(index), Some(manager)) = (ix.map(|ix| ix.row), self.manager.upgrade()) {
            let (previous, busy, remote_list) = {
                let manager = manager.read(cx);
                (manager.selected, manager.busy, manager.remote_list.clone())
            };
            if previous == Some(index) {
                return;
            }
            if busy {
                if let Some(list) = remote_list {
                    defer_list_selection(list, previous, window, cx);
                }
                return;
            }
            let dirty = {
                let manager = manager.read(cx);
                manager.is_dirty(cx)
            };
            if dirty && manager.read(cx).selected != Some(index) {
                let weak = self.manager.clone();
                let prior_index = manager.read(cx).selected.map(IndexPath::new);
                let remote_list = manager.read(cx).remote_list.clone();
                window.open_alert_dialog(cx, move |dialog, _, _| {
                    let weak_ok = weak.clone();
                    let remote_list_cancel = remote_list.clone();
                    dialog
                        .title("Discard unsaved changes?")
                        .description("Your edits will be lost if you switch remotes.")
                        .ok_text("Discard")
                        .show_cancel(true)
                        .on_ok(move |_, window, cx| {
                            let _ = weak_ok
                                .update(cx, |manager, cx| manager.load_config(index, window, cx));
                            true
                        })
                        .on_cancel(move |_, window, cx| {
                            if let Some(list) = &remote_list_cancel {
                                list.update(cx, |list, cx| {
                                    list.set_selected_index(prior_index, window, cx)
                                });
                            }
                            true
                        })
                });
            } else {
                manager.update(cx, |manager, cx| manager.load_config(index, window, cx));
            }
        }
    }

    fn confirm(&mut self, _: bool, _: &mut Window, _: &mut Context<ListState<Self>>) {}
}

impl RemoteManager {
    fn is_dirty(&self, cx: &App) -> bool {
        self.has_draft
            && (manage::is_dirty(
                &self.draft(cx),
                self.selected.and_then(|index| self.configs.get(index)),
            ) || !self.fields[12].read(cx).value().is_empty()
                || self.forget_password)
    }

    fn draft(&self, cx: &App) -> manage::Draft {
        let values: Vec<String> = self
            .fields
            .iter()
            .take(12)
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
            credential_id: (!self.forget_password)
                .then(|| {
                    self.selected
                        .and_then(|index| self.configs.get(index))
                        .and_then(|config| config.credential_id.clone())
                })
                .flatten(),
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
        self.has_draft = true;
        self.selected_original_name = Some(self.configs[index].name.clone());
        self.forget_password = false;
        self.password_identity = None;
        self.credential_status = "No saved password".to_owned();
        self.credential_status_generation = self.credential_status_generation.wrapping_add(1);
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
            String::new(),
        ];
        for (field, value) in self.fields.iter().zip(values) {
            field.update(cx, |field, cx| field.set_value(value, window, cx));
        }
        self.refresh_credential_status(index, window, cx);
        cx.notify();
    }

    fn refresh_credential_status(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.credential_status_generation = self.credential_status_generation.wrapping_add(1);
        let generation = self.credential_status_generation;
        let Some(id) = self.configs[index].credential_id.clone() else {
            self.credential_status = "No saved password".to_owned();
            return;
        };
        self.credential_status = "Checking saved password…".to_owned();
        let entity = cx.entity().downgrade();
        self.credential_task = cx.spawn_in(window, async move |_, cx| {
            let status = cx
                .background_spawn(async move {
                    SystemCredentialStore.has(&id).map(|found| {
                        if found {
                            "Saved password is available".to_owned()
                        } else {
                            "Saved password is missing; replace or forget it".to_owned()
                        }
                    })
                })
                .await
                .unwrap_or_else(|error| format!("Keyring unavailable: {error}"));
            let _ = entity.update_in(cx, |this, _, cx| {
                if this.credential_status_generation == generation {
                    this.credential_status = status;
                    cx.notify();
                }
            });
        });
    }
}

mod render;

impl Drop for RemoteManager {
    fn drop(&mut self) {
        if let Some(cancel) = self.test_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl RemoteManager {
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let password = self.fields[12].read(cx).value().to_string();
        self.fields[12].update(cx, |field, cx| field.set_value(String::new(), window, cx));
        self.password_identity = None;
        let mut draft = self.draft(cx);
        if self.forget_password {
            draft.credential_id = None;
        }
        if !self.forget_password
            && !draft.password_command.trim().is_empty()
            && self
                .selected
                .and_then(|index| self.configs.get(index))
                .is_some_and(|current| current.credential_id.is_some())
        {
            self.message =
                Some("Forget the saved password before saving password_command.".to_owned());
            cx.notify();
            return;
        }
        let config = match draft.validate() {
            Ok(config) => config,
            Err(error) => {
                self.message = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        if !password.is_empty() && config.password_command.is_some() {
            self.fields[12].update(cx, |field, cx| field.set_value(String::new(), window, cx));
            self.message = Some(
                "Clear password_command before saving a saved password. The typed value was cleared."
                    .to_owned(),
            );
            cx.notify();
            return;
        }
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
        let has_password_command = config.password_command.is_some();
        if let Some(index) = old_index {
            self.configs[index] = config;
        } else {
            self.configs.push(config);
            self.selected = Some(self.configs.len() - 1);
        }
        self.has_draft = true;
        let credential_change = if !password.is_empty() {
            CredentialChange::Replace(password)
        } else if self.forget_password || has_password_command {
            CredentialChange::Forget
        } else {
            CredentialChange::Keep
        };
        self.forget_password = false;
        self.persist(window, cx, Some(credential_change), None);
    }

    fn persist(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        credential_change: Option<CredentialChange>,
        removed_name: Option<String>,
    ) {
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
        let selected_name = self
            .selected
            .and_then(|index| self.configs.get(index))
            .map(|config| config.name.clone());
        let entity = cx.entity().downgrade();
        self.save_task = cx.spawn_in(window, async move |_, cx| {
            let result = cx
                .background_spawn(async move {
                    let mut configs = configs;
                    let persisted = (|| -> std::io::Result<_> {
                        let rename = rename
                            .as_ref()
                            .map(|(old, new)| (old.as_str(), new.as_str()));
                        let (remote_name, action) = if let Some(name) = selected_name.as_deref() {
                            let action = match credential_change.unwrap_or(CredentialChange::Keep) {
                                CredentialChange::Keep => manage::CredentialAction::Keep,
                                CredentialChange::Replace(password) => {
                                    manage::CredentialAction::Replace(password)
                                }
                                CredentialChange::Forget => manage::CredentialAction::Forget,
                            };
                            (name, action)
                        } else if let Some(name) = removed_name.as_deref() {
                            (name, manage::CredentialAction::Forget)
                        } else {
                            return Err(std::io::Error::other(
                                "no remote selected for persistence",
                            ));
                        };
                        let outcome = manage::save_with_credential(
                            &SystemCredentialStore,
                            &path,
                            &baseline,
                            &mut configs,
                            rename,
                            remote_name,
                            action,
                        )?;
                        let baseline = manage::read(&path_for_read)?;
                        Ok((baseline, configs, outcome.cleanup_warning))
                    })();
                    persisted.map_err(|e| format!("{}: {e}", path.display()))
                })
                .await;
            let _ = entity.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(new_baseline) => {
                        this.baseline = new_baseline.0;
                        this.configs = new_baseline.1;
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
                                this.has_draft = false;
                            }
                        } else {
                            this.selected_original_name = this.selected.and_then(|index| {
                                this.configs.get(index).map(|config| config.name.clone())
                            });
                        }
                        this.message = Some(new_baseline.2.map_or_else(
                            || "Remotes saved".to_owned(),
                            |warning| format!("Remotes saved; {warning}"),
                        ));
                        if let Some(index) = this.selected {
                            this.refresh_credential_status(index, window, cx);
                        }
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
                if let Some(list) = &this.remote_list {
                    list.update(cx, |_, cx| cx.notify());
                }
                if let Some(list) = this.remote_list.clone() {
                    defer_list_selection(list, this.selected, window, cx);
                }
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

mod open;
