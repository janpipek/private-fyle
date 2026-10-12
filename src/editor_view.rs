//! Editor pane for a [`Document`]: gpui-component's code editor with save,
//! close and a modified marker.

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Editor, EditorState, InputEvent};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::editor::Document;

actions!(fyle_editor, [SaveFile, CloseEditor]);

const CONTEXT: &str = "EditorPane";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-s", SaveFile, Some(CONTEXT)),
        KeyBinding::new("secondary-w", CloseEditor, Some(CONTEXT)),
        KeyBinding::new("escape", CloseEditor, Some(CONTEXT)),
    ]);
}

pub enum EditorEvent {
    Saved(String),
    Failed(String),
    Closed,
}

/// Opens an editor in its own window and returns its window handle and pane.
pub fn open_window(doc: Document, cx: &mut App) -> Result<(AnyWindowHandle, Entity<EditorPane>)> {
    let title = format!("{} — Fyle", doc.name());
    let bounds = Bounds::centered(None, size(px(900.), px(700.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    gpui_kit::open_window(options, cx, move |window, cx| {
        let pane = cx.new(|cx| EditorPane::new(doc, window, cx));
        let weak_pane = pane.downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            let _ = weak_pane.update(cx, |pane, cx| pane.close(&CloseEditor, window, cx));
            false
        });
        pane
    })
}

pub struct EditorPane {
    doc: Document,
    state: Entity<EditorState>,
    modified: bool,
    /// Close once the pending save succeeds (Save in the unsaved-changes
    /// dialog, possibly via the overwrite prompt).
    close_after_save: bool,
    _subscription: Subscription,
}

impl EventEmitter<EditorEvent> for EditorPane {}

impl EditorPane {
    pub fn new(doc: Document, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(doc.language())
                .line_number(true)
                .soft_wrap(false)
        });
        state.update(cx, |state, cx| {
            state.set_value(doc.text().to_string(), window, cx);
            state.focus(window, cx);
        });
        let subscription = cx.subscribe(&state, |this, state, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                let modified = this.doc.is_modified(&state.read(cx).value());
                if modified != this.modified {
                    this.modified = modified;
                    cx.notify();
                }
            }
        });
        Self {
            doc,
            state,
            modified: false,
            close_after_save: false,
            _subscription: subscription,
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.state.update(cx, |state, cx| state.focus(window, cx));
    }

    fn save(&mut self, _: &SaveFile, window: &mut Window, cx: &mut Context<Self>) {
        if !self.doc.changed_on_disk() {
            self.write(window, cx);
            return;
        }
        // Someone else changed the file since it was opened.
        let this = cx.entity().downgrade();
        let description = format!(
            "\"{}\" was changed on disk since it was opened. Overwrite it with your version?",
            self.doc.name()
        );
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let this = this.clone();
            dialog
                .title("File changed on disk")
                .description(description.clone())
                .ok_text("Overwrite")
                .ok_variant(ButtonVariant::Danger)
                .show_cancel(true)
                .on_ok({
                    let this = this.clone();
                    move |_, window, cx| {
                        let _ = this.update(cx, |this, cx| this.write(window, cx));
                        true
                    }
                })
                .on_cancel(move |_, _, cx| {
                    let _ = this.update(cx, |this, _| this.close_after_save = false);
                    true
                })
        });
    }

    fn write(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.state.read(cx).value();
        match self.doc.save(&text) {
            Ok(()) => {
                self.modified = false;
                cx.emit(EditorEvent::Saved(format!(
                    "Saved {}",
                    self.doc.path().display()
                )));
                if std::mem::take(&mut self.close_after_save) {
                    self.finish_close(window, cx);
                }
            }
            Err(e) => {
                self.close_after_save = false;
                cx.emit(EditorEvent::Failed(e));
            }
        }
        cx.notify();
    }

    fn finish_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(EditorEvent::Closed);
        window.remove_window();
    }

    /// Closes, asking first if there are unsaved changes.
    fn close(&mut self, _: &CloseEditor, window: &mut Window, cx: &mut Context<Self>) {
        if !self.modified {
            self.finish_close(window, cx);
            return;
        }
        let this = cx.entity().downgrade();
        let description = format!("Save changes to \"{}\" before closing?", self.doc.name());
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let button =
                |id: &'static str,
                 label: &'static str,
                 action: fn(&mut Self, &mut Window, &mut Context<Self>)| {
                    let this = this.clone();
                    Button::new(id).label(label).on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        let _ = this.update(cx, |this, cx| action(this, window, cx));
                    })
                };
            dialog
                .title("Unsaved changes")
                .description(description.clone())
                .footer(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            button("discard", "Discard", |this, window, cx| {
                                this.finish_close(window, cx)
                            })
                            .with_variant(ButtonVariant::Danger),
                        )
                        .child(div().flex_1())
                        .child(button("cancel", "Cancel", |_, _, _| {}))
                        .child(
                            button("save", "Save", |this, window, cx| {
                                this.close_after_save = true;
                                this.save(&SaveFile, window, cx);
                            })
                            .primary(),
                        ),
                )
        });
    }
}

impl Render for EditorPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::close))
            .size_full()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .px_2()
                    .py_0p5()
                    .rounded(theme.radius)
                    .bg(theme.muted)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(self.doc.path().display().to_string()),
                    )
                    .when(self.modified, |d| {
                        d.child(div().flex_none().child("● modified"))
                    })
                    .child(
                        Button::new("save")
                            .label("Save")
                            .ghost()
                            .xsmall()
                            .disabled(!self.modified)
                            .on_click(cx.listener(|this, _, w, cx| this.save(&SaveFile, w, cx))),
                    )
                    .child(
                        Button::new("close")
                            .label("Close")
                            .ghost()
                            .xsmall()
                            .on_click(
                                cx.listener(|this, _, w, cx| this.close(&CloseEditor, w, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.border)
                    .overflow_hidden()
                    .child(Editor::new(&self.state).bordered(false).h_full()),
            )
            .child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Ctrl-S save · Esc / Ctrl-W close"),
            )
    }
}
