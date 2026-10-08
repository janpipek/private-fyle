//! Responsive rendering for folder panels and the inspector.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Commander;
use crate::workspace::{self, MinSizes, Viewport};

impl Commander {
    /// Renders the requested workspace layout, docking the inspector where it fits.
    pub(super) fn render_workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let viewport = window.viewport_size();
        let scale = f32::from(theme.font_size) / 16.0;
        let width = (f32::from(viewport.width) - 16.0).max(0.0);
        let height = (f32::from(viewport.height) - 64.0).max(0.0);
        let mode = self.workspace.mode();
        let fit = workspace::fit(
            mode,
            Viewport {
                width,
                height,
                scale,
            },
            MinSizes {
                panel_width: 340.0,
                panel_height: 220.0,
                inspector_width: 300.0,
                inspector_height: 180.0,
                gap: 14.0,
            },
        );

        // F4 temporarily restores the familiar two-column editor arrangement.
        let editor_open = self.editor.is_some();
        let visible = if editor_open {
            [true, true]
        } else {
            self.workspace.visible_panels()
        };
        let active = self.active;
        let panel = |ix: usize| {
            let child = if editor_open && ix != active {
                self.editor
                    .as_ref()
                    .expect("editor_open matches editor presence")
                    .clone()
                    .into_any_element()
            } else {
                self.views[ix].clone().into_any_element()
            };
            div().flex_1().min_w_0().min_h_0().h_full().child(child)
        };

        let divider = div()
            .id("panel-divider")
            .w(px(6.))
            .h_full()
            .flex_none()
            .cursor(CursorStyle::ResizeLeftRight)
            .hover(|style| style.bg(theme.primary))
            .when(self.dragging_split, |style| style.bg(theme.primary))
            .when(!self.dragging_split, |style| style.bg(theme.border))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                    if event.click_count == 2 {
                        this.panel_split = 0.5;
                        this.dragging_split = false;
                    } else {
                        this.dragging_split = true;
                    }
                    cx.notify();
                }),
            );

        let folder_area = if visible[0] && visible[1] {
            h_flex()
                .flex_1()
                .h_full()
                .min_w_0()
                .min_h_0()
                .gap_1()
                .child(
                    div()
                        .w(DefiniteLength::Fraction(self.panel_split))
                        .min_w_0()
                        .h_full()
                        .child(panel(0)),
                )
                .child(divider)
                .child(
                    div()
                        .w(DefiniteLength::Fraction(1.0 - self.panel_split))
                        .min_w_0()
                        .h_full()
                        .child(panel(1)),
                )
                .into_any_element()
        } else {
            let ix = if visible[0] { 0 } else { 1 };
            let destination = self.folders[1 - ix].read(cx).path();
            v_flex()
                .flex_1()
                .h_full()
                .min_w_0()
                .min_h_0()
                .child(
                    div()
                        .px_1()
                        .pb_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .truncate()
                        .child(format!(
                            "Destination: {}",
                            crate::fs_ops::display_path(&destination)
                        )),
                )
                .child(div().flex_1().min_w_0().min_h_0().child(panel(ix)))
                .into_any_element()
        };

        let inspector_requested = mode.has_inspector() && !editor_open;
        let dock = fit.map(|fit| {
            // Prefer the inspector on the right whenever that arrangement fits.
            fit.right
        });
        if inspector_requested && let Some(right) = dock {
            let (extent, folder_min, inspector_min) = if right {
                let count = if visible[0] && visible[1] { 2.0 } else { 1.0 };
                (width, 340.0 * count + 14.0 * count, 300.0)
            } else {
                (height, 234.0, 180.0)
            };
            let low = folder_min * scale / extent;
            let high = 1.0 - inspector_min * scale / extent;
            if low <= high {
                self.inspector_split = self.inspector_split.clamp(low, high);
            }
        }
        if !inspector_requested || dock.is_none() {
            self.inspector_split_horizontal = false;
        }
        let content = if inspector_requested {
            match dock {
                Some(true) => {
                    self.inspector_split_horizontal = true;
                    h_flex()
                        .flex_1()
                        .h_full()
                        .min_w_0()
                        .min_h_0()
                        .gap_1()
                        .child(
                            div()
                                .w(DefiniteLength::Fraction(self.inspector_split))
                                .min_w_0()
                                .h_full()
                                .child(folder_area),
                        )
                        .child(self.inspector_divider(true, theme.primary, theme.border, cx))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .child(self.preview.clone()),
                        )
                        .into_any_element()
                }
                Some(false) => {
                    self.inspector_split_horizontal = false;
                    v_flex()
                        .flex_1()
                        .h_full()
                        .min_w_0()
                        .min_h_0()
                        .gap_1()
                        .child(
                            div()
                                .h(DefiniteLength::Fraction(self.inspector_split))
                                .min_h_0()
                                .w_full()
                                .child(folder_area),
                        )
                        .child(self.inspector_divider(false, theme.primary, theme.border, cx))
                        .child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .w_full()
                                .child(self.preview.clone()),
                        )
                        .into_any_element()
                }
                None => v_flex()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .min_h_0()
                    .child(folder_area)
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .flex_none()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .border_1()
                            .border_color(theme.border)
                            .child("Inspector is hidden because the window is too small"),
                    )
                    .into_any_element(),
            }
        } else {
            folder_area
        };

        div().flex_1().min_h_0().min_w_0().p_2().child(content)
    }

    fn inspector_divider(
        &self,
        right: bool,
        primary: Hsla,
        border: Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (id, cursor) = if right {
            ("inspector-divider-right", CursorStyle::ResizeLeftRight)
        } else {
            ("inspector-divider-bottom", CursorStyle::ResizeUpDown)
        };
        div()
            .id(id)
            .when(right, |d| d.w(px(6.)).h_full())
            .when(!right, |d| d.h(px(6.)).w_full())
            .flex_none()
            .cursor(cursor)
            .hover(|style| style.bg(primary))
            .when(self.dragging_inspector_split, |style| style.bg(primary))
            .when(!self.dragging_inspector_split, |style| style.bg(border))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if event.click_count == 2 {
                        this.inspector_split = 0.68;
                        this.dragging_inspector_split = false;
                    } else {
                        this.dragging_inspector_split = true;
                    }
                    cx.notify();
                }),
            )
    }
}
