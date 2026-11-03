//! Ctrl-wheel capture over the whole preview pane: forwards it to the window
//! zoom, except over a zoomable image, which handles it itself.

use std::rc::Rc;

use gpui_kit::*;

use crate::image_zoom_view::ImageGeometry;
use crate::preview_view::{PreviewPane, PreviewZoom};

/// Captures Ctrl-wheel before nested preview scroll areas consume it.
pub struct PreviewWheelCapture {
    pub content: AnyElement,
    pub view: WeakEntity<PreviewPane>,
    /// Where the zoomable image is; Ctrl-wheel there zooms the image instead.
    pub image: Rc<ImageGeometry>,
}

impl IntoElement for PreviewWheelCapture {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for PreviewWheelCapture {
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
    ) -> (LayoutId, ()) {
        (self.content.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        self.content.prepaint(window, cx);
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        let hitbox = hitbox.clone();
        let view = self.view.clone();
        let image = self.image.clone();
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
            let over_image = image
                .bounds
                .get()
                .is_some_and(|bounds| bounds.contains(&event.position));
            if !phase.capture()
                || over_image
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
                let _ = view.update(cx, |_, cx| cx.emit(PreviewZoom(steps)));
            }
        });
        self.content.paint(window, cx);
    }
}
