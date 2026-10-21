//! Renders the remote management form and saved-password controls.

use super::*;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::list::List;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;

impl Render for RemoteManager {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let protocol = self.fields[1].read(cx).value().to_string();
        let protocol_label = protocol_title(&protocol);
        let dirty = self.is_dirty(cx);
        let field_view = |label: &str, field: &Entity<InputState>| {
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(div().text_xs().child(label.to_owned()))
                .child(Input::new(field).disabled(self.busy))
        };
        let mut content = v_flex().w_full().min_w_0().gap_3();
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
                                this.forget_password = false;
                                this.credential_status = "No saved password".to_owned();
                                this.credential_status_generation =
                                    this.credential_status_generation.wrapping_add(1);
                                this.password_identity = None;
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
                                let removed_name = this.configs[index].name.clone();
                                this.configs.remove(index);
                                this.selected = None;
                                this.selected_original_name = None;
                                this.persist(window, cx, None, Some(removed_name));
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
                        this.forget_password = false;
                        this.credential_status = "No saved password".to_owned();
                        this.password_identity = None;
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
        let has_saved_password = self
            .selected
            .and_then(|index| self.configs.get(index))
            .is_some_and(|config| config.credential_id.is_some());
        let forget_entity = weak.clone();
        let forget = Button::new("remote-forget-password")
            .label(if self.forget_password {
                "Undo forget"
            } else {
                "Forget saved password"
            })
            .small()
            .disabled(!has_saved_password || self.busy)
            .on_click(move |_, _, cx| {
                let _ = forget_entity.update(cx, |this, cx| {
                    this.forget_password = !this.forget_password;
                    this.message = None;
                    cx.notify();
                });
            });
        let password_value = self.fields[12].read(cx).value().to_string();
        let password_command_set = !self.fields[6].read(cx).value().trim().is_empty();
        let password_help = if !password_value.is_empty() {
            "Save replaces the saved password. Test uses it for this endpoint/account; changing either clears it. Clear password_command before saving it."
        } else if has_saved_password && password_command_set {
            "A saved password and password_command cannot be used together. Forget the saved password before saving this command."
        } else {
            "Leave blank to keep the saved password. A typed password overrides password_command for Test on this endpoint/account; changing either clears it. Clear the command before saving a password."
        };
        let credential_status = if self.forget_password {
            "Saved password will be forgotten when you save".to_owned()
        } else {
            self.credential_status.clone()
        };
        let password_controls = v_flex()
            .min_w_0()
            .gap_1()
            .child(div().text_xs().child("Password (optional)"))
            .child(Input::new(&self.fields[12]).disabled(self.busy))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(
                                if credential_status.contains("unavailable")
                                    || credential_status.contains("missing")
                                {
                                    cx.theme().colors.danger
                                } else {
                                    cx.theme().muted_foreground
                                },
                            )
                            .child(credential_status),
                    )
                    .child(forget),
            )
            .child(
                div()
                    .min_w_0()
                    .whitespace_normal()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(password_help),
            )
            .into_any_element();
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
                            manager.fields[12]
                                .update(cx, |field, cx| field.set_value(String::new(), window, cx));
                            manager.password_identity = None;
                            let has_saved_password = manager
                                .selected
                                .and_then(|index| manager.configs.get(index))
                                .is_some_and(|config| config.credential_id.is_some());
                            manager.forget_password = id == "s3" && has_saved_password;
                            manager.credential_status_generation =
                                manager.credential_status_generation.wrapping_add(1);
                            if manager.forget_password {
                                manager.credential_status =
                                    "S3 does not use saved passwords; Save will forget this one"
                                        .to_owned();
                            } else if has_saved_password {
                                manager.credential_status =
                                    "Saved password is for the saved endpoint/account".to_owned();
                            } else {
                                manager.credential_status = "No saved password".to_owned();
                            }
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
                .min_w_0()
                .gap_2()
                .child(
                    h_flex()
                        .min_w_0()
                        .gap_2()
                        .child(field_view("Host", &self.fields[2]).min_w_0().flex_1())
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
                .child(password_controls)
                .into_any_element(),
            "webdav" => v_flex()
                .min_w_0()
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
                .child(password_controls)
                .into_any_element(),
            "s3" => v_flex()
                .min_w_0()
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
                .when(self.forget_password, |this| {
                    this.child(
                        div()
                            .min_w_0()
                            .whitespace_normal()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("S3 uses the AWS credential chain. Saving this change will forget the saved password."),
                    )
                })
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
            .min_w_0()
            .gap_2()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
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
            .min_w_0()
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
                    || lower.contains("keyring")
                    || lower.contains("missing")
                    || lower.contains("changed on disk")
                    || lower.contains("incomplete")
                    || lower.contains("port must")
                    || lower.contains("clear password_command")
                    || lower.contains("forget the saved password")
                    || lower.contains("could not be deleted");
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
        content = content.child(h_flex().w_full().min_w_0().gap_4().h(px(480.)).child(rows).child(if self.has_draft { form.into_any_element() } else { div().flex_1().min_w_0().child("Select a saved remote or choose Add… to create one.").into_any_element() }))
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
