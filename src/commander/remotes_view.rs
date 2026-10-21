//! Dialog for editing user configured remote connections.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::list::{List, ListDelegate, ListItem, ListState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IndexPath, Sizable as _, WindowExt as _, h_flex,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Commander;
use crate::remote::{connections, manage};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

mod connection_test;

const FIELDS: [&str; 12] = [
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
    _input_subscriptions: Vec<Subscription>,
}

struct RemoteListDelegate {
    manager: WeakEntity<RemoteManager>,
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
                manager.has_draft
                    && manage::is_dirty(
                        &manager.draft(cx),
                        manager
                            .selected
                            .and_then(|index| manager.configs.get(index)),
                    )
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
        self.has_draft = true;
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
        let protocol = self.fields[1].read(cx).value().to_string();
        let protocol_label = protocol_title(&protocol);
        let dirty = self.has_draft
            && manage::is_dirty(
                &self.draft(cx),
                self.selected.and_then(|index| self.configs.get(index)),
            );
        let field_view = |label: &str, field: &Entity<InputState>| {
            v_flex()
                .flex_1()
                .gap_1()
                .child(div().text_xs().child(label.to_owned()))
                .child(Input::new(field).disabled(self.busy))
        };
        let mut content = v_flex().gap_3();
        let refresh = weak.clone();
        let add = Button::new("remote-add")
            .label("Add…")
            .icon(Icon::new(Lucide::ChevronDown))
            .small()
            .disabled(self.busy || dirty)
            .dropdown_menu(move |mut menu, _, _| {
                let parent = refresh.clone();
                for (id, label) in [
                    ("sftp", "SFTP"),
                    ("ftp", "FTP"),
                    ("ftps", "FTPS"),
                    ("webdav", "WebDAV"),
                    ("s3", "S3"),
                ] {
                    let parent = parent.clone();
                    menu = menu.item(PopupMenuItem::new(format!("New {label} remote")).on_click(
                        move |_, window, cx| {
                            let _ = parent.update(cx, |this, cx| {
                                this.cancel_test();
                                this.selected = None;
                                this.selected_original_name = None;
                                this.has_draft = true;
                                for field in &this.fields {
                                    field
                                        .update(cx, |f, cx| f.set_value(String::new(), window, cx));
                                }
                                this.fields[1]
                                    .update(cx, |f, cx| f.set_value(id.to_owned(), window, cx));
                                this.fields[5].update(cx, |f, cx| f.set_value("/", window, cx));
                                if let Some(list) = this.remote_list.clone() {
                                    defer_list_selection(list, None, window, cx);
                                }
                                this.message = None;
                                cx.notify();
                            });
                        },
                    ));
                }
                menu
            });
        let del = weak.clone();
        let remove = Button::new("remote-remove")
            .label("Remove…")
            .small()
            .disabled(self.selected.is_none() || self.busy || dirty)
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
            .on_click(move |_, window, cx| {
                let _ = test_entity.update(cx, |this, cx| this.test(window, cx));
            });
        let connect_entity = weak.clone();
        let connect = Button::new("remote-connect")
            .label("Connect")
            .small()
            .disabled(self.selected.is_none() || self.busy || dirty)
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
        let cancel_entity = weak.clone();
        let cancel_changes = Button::new("remote-cancel-changes")
            .label("Cancel changes")
            .small()
            .disabled(!dirty || self.busy)
            .on_click(move |_, window, cx| {
                let _ = cancel_entity.update(cx, |this, cx| {
                    this.cancel_test();
                    if let Some(index) = this
                        .selected
                        .filter(|index| *index < this.saved_configs.len())
                    {
                        this.configs = this.saved_configs.clone();
                        this.load_config(index, window, cx);
                    } else {
                        this.configs = this.saved_configs.clone();
                        this.selected = None;
                        this.selected_original_name = None;
                        this.has_draft = false;
                        for field in &this.fields {
                            field.update(cx, |f, cx| f.set_value(String::new(), window, cx));
                        }
                        if let Some(list) = this.remote_list.clone() {
                            defer_list_selection(list, None, window, cx);
                        }
                    }
                    this.message = None;
                    cx.notify();
                });
            });
        let selector = Button::new("remote-protocol")
            .label(protocol_label)
            .icon(Icon::new(Lucide::ChevronDown))
            .small()
            .disabled(self.busy)
            .dropdown_menu(move |mut menu, _, _| {
                let entity = weak.clone();
                for (id, label) in [
                    ("sftp", "SFTP"),
                    ("ftp", "FTP"),
                    ("ftps", "FTPS"),
                    ("webdav", "WebDAV"),
                    ("s3", "S3"),
                ] {
                    let entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        let _ = entity.update(cx, |manager, cx| {
                            manager.cancel_test();
                            manager.fields[1]
                                .update(cx, |field, cx| field.set_value(id.to_owned(), window, cx));
                            manager.message = None;
                            cx.notify();
                        });
                    }));
                }
                menu
            });
        let port_label = match protocol.as_str() {
            "ftp" | "ftps" => "Port (default 21)",
            _ => "Port (default 22)",
        };
        let protocol_fields = match protocol.as_str() {
            "sftp" | "ftp" | "ftps" => v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .gap_2()
                        .child(field_view("Host", &self.fields[2]).flex_1())
                        .child(
                            field_view(port_label, &self.fields[3])
                                .w(px(120.))
                                .flex_none(),
                        ),
                )
                .child(field_view("Username", &self.fields[4]))
                .child(field_view("Root folder (optional)", &self.fields[5]))
                .child(
                    v_flex()
                        .gap_1()
                        .child(field_view("Password command", &self.fields[6]))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Prints the password; it is not run through a shell."),
                        ),
                )
                .into_any_element(),
            "webdav" => v_flex()
                .gap_2()
                .child(field_view("WebDAV URL", &self.fields[7]))
                .child(field_view("Username", &self.fields[4]))
                .child(field_view("Root folder (optional)", &self.fields[5]))
                .child(
                    v_flex()
                        .gap_1()
                        .child(field_view("Password command", &self.fields[6]))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Prints the password; it is not run through a shell."),
                        ),
                )
                .into_any_element(),
            "s3" => v_flex()
                .gap_2()
                .child(field_view("Bucket", &self.fields[8]))
                .child(
                    h_flex()
                        .gap_2()
                        .child(field_view("Region", &self.fields[9]))
                        .child(field_view("AWS profile", &self.fields[10])),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Uses the standard AWS credential chain."),
                )
                .child(field_view("Custom endpoint (optional)", &self.fields[11]))
                .child(field_view("Root folder (optional)", &self.fields[5]))
                .into_any_element(),
            _ => div()
                .child("Choose a supported protocol.")
                .into_any_element(),
        };
        let rows = v_flex()
            .w(px(265.))
            .h_full()
            .gap_2()
            .child(h_flex().justify_between().child("Saved remotes").child(add));
        let mut rows = rows;
        if self.configs.is_empty() {
            rows = rows.child(
                div()
                    .flex_1()
                    .p_3()
                    .child("No saved remotes yet. Choose Add… to create one."),
            );
        } else if let Some(list) = &self.remote_list {
            rows = rows.child(
                div()
                    .flex_1()
                    .border_1()
                    .rounded_md()
                    .overflow_y_scrollbar()
                    .child(List::new(list)),
            );
        }
        let rows = rows.child(remove);
        let name_and_type = h_flex()
            .gap_2()
            .child(
                v_flex()
                    .flex_1()
                    .gap_1()
                    .child(div().text_xs().child("Name"))
                    .child(Input::new(&self.fields[0]).disabled(self.busy)),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_xs().child("Type"))
                    .child(selector),
            );
        let heading = self
            .selected
            .and_then(|index| self.configs.get(index))
            .map_or_else(
                || format!("New {} remote", protocol_title(&protocol)),
                |config| format!("Edit {}", config.name),
            );
        let form = v_flex()
            .flex_1()
            .h_full()
            .min_h_0()
            .gap_2()
            .child(div().text_sm().child(heading))
            .child(name_and_type)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(protocol_fields),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(save)
                    .child(cancel_changes)
                    .child(test.label("Test connection"))
                    .child(connect),
            )
            .when_some(self.message.clone(), |this, message| {
                let lower = message.to_lowercase();
                let error = lower.contains("failed")
                    || lower.contains("error")
                    || lower.contains("changed on disk")
                    || lower.contains("incomplete")
                    || lower.contains("port must");
                this.child(
                    div()
                        .text_xs()
                        .text_color(if error {
                            cx.theme().colors.danger
                        } else {
                            cx.theme().muted_foreground
                        })
                        .child(message),
                )
            })
            .when(dirty && self.selected.is_some(), |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Save or cancel changes before connecting."),
                )
            });
        content = content.child(h_flex().gap_4().h(px(455.)).child(rows).child(if self.has_draft { form.into_any_element() } else { div().flex_1().child("Select a saved remote or choose Add… to create one.").into_any_element() }))
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child("rclone remotes use rclone config. Proton Drive requires proton-drive auth login."))
            .when(dirty, |this| this.child(div().text_xs().text_color(cx.theme().muted_foreground).child("Save or cancel changes before adding or removing remotes.")));
        if !self.has_draft
            && let Some(message) = &self.message
        {
            content = content.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.danger)
                    .child(message.clone()),
            );
        }
        content
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
        self.has_draft = true;
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
                                this.has_draft = false;
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
                        remote_list: None,
                        has_draft: false,
                        _input_subscriptions: Vec::new(),
                    });
                    let manager_weak = draft.downgrade();
                    let input_fields = draft.read(cx).fields.clone();
                    let subscriptions = input_fields.iter().map(|field| {
                        let manager = manager_weak.clone();
                        cx.subscribe(field, move |_, _, event: &InputEvent, cx| {
                            if matches!(event, InputEvent::Change) {
                                let _ = manager.update(cx, |manager, cx| {
                                    manager.cancel_test();
                                    manager.message = None;
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
                                manager.has_draft && manage::is_dirty(&manager.draft(cx), manager.selected.and_then(|i| manager.configs.get(i)))
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
