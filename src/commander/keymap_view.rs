//! Dialog and draft state for editing the user's command shortcuts.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::*;

use super::Commander;
use crate::keymap::{self, Editor};
use crate::settings::{KeyContext, KeymapEntry};

/// Draft keymap state rendered inside the shortcut dialog.
pub struct KeymapDraft {
    editor: Editor,
    error: Option<String>,
    saving: bool,
    commander: gpui::WeakEntity<Commander>,
}

impl KeymapDraft {
    fn new(entries: Vec<KeymapEntry>, commander: gpui::WeakEntity<Commander>) -> Self {
        Self {
            editor: Editor::new(entries),
            error: None,
            saving: false,
            commander,
        }
    }

    fn entries(&self) -> Vec<KeymapEntry> {
        self.editor.entries().to_vec()
    }
}

impl Render for KeymapDraft {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let draft = cx.entity().downgrade();
        let rows = keymap::commands().into_iter().map(|command| {
            let (current, current_context) = effective_binding(&self.editor, &command);
            let action = command.action;
            let context = command.context;
            let edit_draft = draft.clone();
            let edit = Button::new(SharedString::from(format!("shortcut-edit-{action}")))
                .label("Edit")
                .small()
                .disabled(self.saving)
                .on_click(move |_, window, cx| {
                    edit_shortcut(edit_draft.clone(), action, current_context, window, cx);
                });
            let disable_draft = draft.clone();
            let disable = Button::new(SharedString::from(format!("shortcut-disable-{action}")))
                .label("Disable")
                .small()
                .disabled(self.saving)
                .on_click(move |_, _, cx| {
                    let _ = disable_draft.update(cx, |draft, cx| {
                        draft.editor.disable(action, context);
                        cx.notify();
                    });
                });
            let reset_draft = draft.clone();
            let reset = Button::new(SharedString::from(format!("shortcut-reset-{action}")))
                .label("Reset")
                .small()
                .disabled(self.saving)
                .on_click(move |_, _, cx| {
                    let _ = reset_draft.update(cx, |draft, cx| {
                        draft.editor.reset(action, context);
                        cx.notify();
                    });
                });
            h_flex()
                .gap_2()
                .child(
                    v_flex().flex_1().child(command.label).child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(if current.is_empty() {
                                if !command.keys.is_empty() {
                                    "Disabled".to_owned()
                                } else {
                                    "No shortcut".to_owned()
                                }
                            } else {
                                format!("{current} · {}", context_name(current_context))
                            }),
                    ),
                )
                .child(edit)
                .child(disable)
                .child(reset)
        });
        let save_draft = draft.clone();
        let save = Button::new("shortcut-save")
            .label("Save")
            .small()
            .primary()
            .disabled(self.saving)
            .on_click(move |_, _, cx| {
                let Some(draft_entity) = save_draft.upgrade() else {
                    return;
                };
                let entries = draft_entity.read(cx).entries();
                let commander = draft_entity.read(cx).commander.clone();
                let Some(commander) = commander.upgrade() else {
                    return;
                };
                let draft_for_save = draft_entity.clone();
                commander.update(cx, |this, cx| {
                    this.save_shortcuts(entries, draft_for_save, cx)
                });
            });
        let reset_draft = draft.clone();
        let reset_all = Button::new("shortcut-reset-all")
            .label("Reset all")
            .small()
            .disabled(self.saving)
            .on_click(move |_, _, cx| {
                let _ = reset_draft.update(cx, |draft, cx| {
                    draft.editor.reset_all();
                    cx.notify();
                });
            });
        let mut content = v_flex();
        if let Some(error) = &self.error {
            content = content.child(div().text_color(cx.theme().danger).child(error.clone()));
        }
        content
            .gap_2()
            .child(
                div()
                    .id("shortcut-list")
                    .max_h(px(520.))
                    .overflow_y_scroll()
                    .child(v_flex().gap_2().children(rows)),
            )
            .child(h_flex().justify_between().child(reset_all).child(save))
    }
}

impl Commander {
    /// Opens the shortcut editor for the command catalog.
    pub fn edit_shortcuts(
        &mut self,
        _: &super::keys::EditShortcuts,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let commander = cx.entity().downgrade();
        let draft = cx.new(|_| KeymapDraft::new(self.shortcut_entries.clone(), commander));
        window.open_dialog(cx, move |dialog, _, _| {
            let cancel = Button::new("shortcut-cancel")
                .label("Cancel")
                .small()
                .on_click(|_, window, cx| window.close_dialog(cx));
            dialog.title("Keyboard shortcuts").w(px(760.)).child(
                v_flex()
                    .gap_2()
                    .child(draft.clone())
                    .child(h_flex().justify_end().child(cancel)),
            )
        });
    }

    fn save_shortcuts(
        &mut self,
        entries: Vec<KeymapEntry>,
        draft: Entity<KeymapDraft>,
        cx: &mut Context<Self>,
    ) {
        if draft.read(cx).saving {
            return;
        }
        if let Err(errors) = super::keys::validate_user_keys(&entries, cx) {
            let message = errors.join("; ");
            self.status = message.clone().into();
            draft.update(cx, |draft, cx| {
                draft.error = Some(message);
                cx.notify();
            });
            cx.notify();
            return;
        }
        draft.update(cx, |draft, cx| {
            draft.saving = true;
            draft.error = None;
            cx.notify();
        });
        let saved_entries = entries.clone();
        self._keymap_save_task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { crate::settings::save_keymap(&saved_entries) })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(path) => {
                    let errors = super::keys::replace_user_keys(&entries, cx);
                    this.shortcut_entries = entries;
                    this.status = if errors.is_empty() {
                        format!("Keyboard shortcuts saved to {}", path.display()).into()
                    } else {
                        format!("Keyboard shortcuts saved; {}", errors.join("; ")).into()
                    };
                    draft.update(cx, |draft, cx| {
                        draft.saving = false;
                        draft.error = None;
                        cx.notify();
                    });
                    cx.notify();
                }
                Err(error) => {
                    this.status = error.clone().into();
                    draft.update(cx, |draft, cx| {
                        draft.saving = false;
                        draft.error = Some(error);
                        cx.notify();
                    });
                    cx.notify();
                }
            });
        });
    }
}

fn effective_binding(editor: &Editor, command: &keymap::CommandBinding) -> (String, KeyContext) {
    let entries = editor.entries();
    if let Some(entry) = entries.iter().find(|entry| entry.action == command.action) {
        return (entry.keys.clone(), entry.context);
    }
    if entries.iter().any(|entry| {
        entry.action == "none" && entry.context == command.context && entry.keys == command.keys
    }) {
        (String::new(), command.context)
    } else {
        (command.keys.to_owned(), command.context)
    }
}

fn edit_shortcut(
    draft: gpui::WeakEntity<KeymapDraft>,
    action: &'static str,
    context: KeyContext,
    window: &mut Window,
    cx: &mut App,
) {
    let command = keymap::commands()
        .into_iter()
        .find(|command| command.action == action)
        .expect("action came from command catalog");
    let Some(draft_entity) = draft.upgrade() else {
        return;
    };
    let entries = draft_entity.read(cx).entries();
    let keys = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. ctrl-shift-c"));
    let (current, _) = effective_binding(&Editor::new(entries), &command);
    if !current.is_empty() {
        keys.update(cx, |input, cx| input.set_value(current, window, cx));
    }
    let context_input =
        cx.new(|cx| InputState::new(window, cx).placeholder("panel, global, or editor"));
    context_input.update(cx, |input, cx| {
        input.set_value(context_name(context), window, cx)
    });
    let dialog_keys = keys.clone();
    let dialog_context = context_input.clone();
    window.open_alert_dialog(cx, move |dialog, _, _| {
        let (draft, keys, context_input) =
            (draft.clone(), dialog_keys.clone(), dialog_context.clone());
        dialog
            .title(format!("Edit {}", command.label))
            .description(
                v_flex()
                    .gap_2()
                    .child("Shortcut keys")
                    .child(Input::new(&dialog_keys))
                    .child("Context (panel, global, editor)")
                    .child(Input::new(&dialog_context)),
            )
            .ok_text("Apply")
            .show_cancel(true)
            .on_ok(move |_, _, cx| {
                let keys = keys.read(cx).value().to_string();
                let context = match parse_context(context_input.read(cx).value().as_ref()) {
                    Some(context) => context,
                    None => {
                        let _ = draft.update(cx, |draft, cx| {
                            draft.error =
                                Some("Context must be panel, global, or editor".to_owned());
                            cx.notify();
                        });
                        return false;
                    }
                };
                match draft.update(cx, |draft, cx| {
                    match draft.editor.set(action, &keys, context) {
                        Ok(()) => {
                            draft.error = None;
                            cx.notify();
                            Ok(())
                        }
                        Err(error) => {
                            draft.error = Some(error.to_string());
                            cx.notify();
                            Err(())
                        }
                    }
                }) {
                    Ok(Ok(())) => true,
                    Ok(Err(())) => false,
                    Err(_) => false,
                }
            })
    });
}

fn context_name(context: KeyContext) -> &'static str {
    match context {
        KeyContext::Panel => "panel",
        KeyContext::Global => "global",
        KeyContext::Editor => "editor",
    }
}

fn parse_context(value: &str) -> Option<KeyContext> {
    match value.trim().to_ascii_lowercase().as_str() {
        "panel" => Some(KeyContext::Panel),
        "global" => Some(KeyContext::Global),
        "editor" => Some(KeyContext::Editor),
        _ => None,
    }
}
