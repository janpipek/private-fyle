//! Tabbed inspector controller joining Preview and Properties perspectives.

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::preview_view::PreviewPane;
use crate::properties_view::PropertiesPane;
use crate::workspace::Perspective;

/// A user-selected inspector perspective change.
pub struct InspectorEvent(pub Perspective);

/// Tab bar and active child view for the workspace inspector slot.
pub struct InspectorPane {
    perspective: Perspective,
    preview: Entity<PreviewPane>,
    properties: Entity<PropertiesPane>,
}

impl EventEmitter<InspectorEvent> for InspectorPane {}

impl InspectorPane {
    /// Creates the tab controller around the two perspective views.
    pub fn new(
        perspective: Perspective,
        preview: Entity<PreviewPane>,
        properties: Entity<PropertiesPane>,
    ) -> Self {
        Self {
            perspective,
            preview,
            properties,
        }
    }

    /// Selects the tab from a command or workspace action.
    pub fn set_perspective(&mut self, perspective: Perspective, cx: &mut Context<Self>) {
        if self.perspective != perspective {
            self.perspective = perspective;
            cx.notify();
        }
    }
}

impl Render for InspectorPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let perspective = self.perspective;
        let tabs = h_flex()
            .w_full()
            .gap_1()
            .px_2()
            .py_1()
            .bg(theme.muted)
            .child(
                div()
                    .id("inspector-view-tab")
                    .px_2()
                    .py_1()
                    .rounded(theme.radius)
                    .when(perspective == Perspective::Preview, |style| {
                        style.bg(theme.background)
                    })
                    .cursor_pointer()
                    .child("View")
                    .on_click(cx.listener(|this, _, _, cx| this.select(Perspective::Preview, cx))),
            )
            .child(
                div()
                    .id("inspector-properties-tab")
                    .px_2()
                    .py_1()
                    .rounded(theme.radius)
                    .when(perspective == Perspective::Properties, |style| {
                        style.bg(theme.background)
                    })
                    .cursor_pointer()
                    .child("Properties")
                    .on_click(
                        cx.listener(|this, _, _, cx| this.select(Perspective::Properties, cx)),
                    ),
            );
        let child = match self.perspective {
            Perspective::Preview => self.preview.clone().into_any_element(),
            Perspective::Properties => self.properties.clone().into_any_element(),
        };
        v_flex()
            .size_full()
            .min_h_0()
            .min_w_0()
            .child(tabs)
            .child(div().flex_1().min_h_0().min_w_0().child(child))
    }
}

impl InspectorPane {
    fn select(&mut self, perspective: Perspective, cx: &mut Context<Self>) {
        if self.perspective != perspective {
            self.perspective = perspective;
            cx.emit(InspectorEvent(perspective));
            cx.notify();
        }
    }
}
