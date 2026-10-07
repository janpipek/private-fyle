//! Capture Ctrl-wheel over panel content before its table or grid can scroll.

use gpui_kit::*;

use super::{FolderView, HeaderEvent};

/// Captures zoom gestures over a panel's content while leaving normal scrolling alone.
pub struct ControlWheelCapture {
    content: AnyElement,
    view: WeakEntity<FolderView>,
}

impl ControlWheelCapture {
    /// Wraps panel content and routes Ctrl-wheel zoom steps to its view.
    pub fn new(content: impl IntoElement, view: WeakEntity<FolderView>) -> Self {
        Self {
            content: content.into_any_element(),
            view,
        }
    }
}

impl IntoElement for ControlWheelCapture {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ControlWheelCapture {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.content.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.content.prepaint(window, cx);
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let hitbox = hitbox.clone();
        let view = self.view.clone();
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
            if !phase.capture()
                || !hitbox.should_handle_scroll(window)
                || !(event.modifiers.control || event.modifiers.platform)
            {
                return;
            }
            let delta_y = match event.delta {
                ScrollDelta::Pixels(delta) => f32::from(delta.y),
                ScrollDelta::Lines(delta) => delta.y,
            };
            if let Some(steps) = crate::zoom::wheel_steps(delta_y) {
                cx.stop_propagation();
                let _ = view.update(cx, |_, cx| cx.emit(HeaderEvent::Zoom(steps)));
            }
        });
        self.content.paint(window, cx);
    }
}
