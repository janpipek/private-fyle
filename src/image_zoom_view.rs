//! The zoomable image element of the preview pane. It draws the image at the
//! placement computed by [`crate::view_zoom`] inside a clipped viewport and
//! forwards Ctrl-wheel, drag and click to the pane; it holds no zoom state.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::Theme;
use gpui_kit::*;

use crate::preview_view::{ImageZoomCommand, PreviewPane};
use crate::view_zoom::{Vec2, ViewZoom};

/// Geometry the element reports back to the pane after each paint, so key
/// commands can zoom around the viewport center and the pane can leave
/// Ctrl-wheel over the image to the element.
#[derive(Default)]
pub struct ImageGeometry {
    pub viewport: Cell<Vec2>,
    pub bounds: Cell<Option<Bounds<Pixels>>>,
}

/// One image drawn at a given zoom state.
pub struct ZoomableImage {
    pub path: PathBuf,
    /// Intrinsic pixel size of the image.
    pub content: Vec2,
    pub zoom: ViewZoom,
    pub pane: WeakEntity<PreviewPane>,
    pub focus: FocusHandle,
    pub geometry: Rc<ImageGeometry>,
    /// Shown instead of the image when it cannot be decoded.
    pub failure: &'static str,
}

impl IntoElement for ZoomableImage {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ZoomableImage {
    type RequestLayoutState = ();
    type PrepaintState = AnyElement;

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
        let mut style = Style {
            position: Position::Absolute,
            inset: Edges::all(px(0.).into()),
            ..Style::default()
        };
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let viewport = Vec2::new(f32::from(bounds.size.width), f32::from(bounds.size.height));
        self.geometry.viewport.set(viewport);
        self.geometry.bounds.set(Some(bounds));
        let placement = self.zoom.placement(self.content, viewport);
        let failure = self.failure;
        let mut image = img(self.path.clone())
            .w(px(placement.size.x))
            .h(px(placement.size.y))
            .object_fit(ObjectFit::Fill)
            .with_fallback(move || div().child(failure).into_any_element())
            .into_any_element();
        let origin = point(
            bounds.origin.x + px(placement.origin.x),
            bounds.origin.y + px(placement.origin.y),
        );
        image.prepaint_as_root(
            origin,
            size(AvailableSpace::MinContent, AvailableSpace::MinContent),
            window,
            cx,
        );
        image
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        image: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            image.paint(window, cx);
        });
        let local = move |position: Point<Pixels>| {
            Vec2::new(
                f32::from(position.x - bounds.origin.x),
                f32::from(position.y - bounds.origin.y),
            )
        };

        let pane = self.pane.clone();
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, _, cx| {
            if !phase.capture()
                || !bounds.contains(&event.position)
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
                let anchor = local(event.position);
                let _ = pane.update(cx, |pane, cx| pane.zoom_image_at(steps, anchor, cx));
            }
        });

        let pane = self.pane.clone();
        let focus = self.focus.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase.bubble()
                && event.button == MouseButton::Left
                && bounds.contains(&event.position)
            {
                focus.focus(window, cx);
                let _ = pane.update(cx, |pane, _| pane.begin_drag(event.position));
            }
        });

        let pane = self.pane.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase.bubble() && event.pressed_button == Some(MouseButton::Left) {
                let _ = pane.update(cx, |pane, cx| pane.drag_to(event.position, cx));
            }
        });

        let pane = self.pane.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase.bubble() && event.button == MouseButton::Left {
                let _ = pane.update(cx, |pane, _| pane.end_drag());
            }
        });
    }
}

/// The pane's per-image state: zoom, pan, drag and focus.
pub struct ImageState {
    focus: FocusHandle,
    zoom: ViewZoom,
    size: Option<Vec2>,
    geometry: Rc<ImageGeometry>,
    drag_last: Option<Point<Pixels>>,
}

impl ImageState {
    pub fn new(focus: FocusHandle) -> Self {
        Self {
            focus,
            zoom: ViewZoom::new(),
            size: None,
            geometry: Rc::new(ImageGeometry::default()),
            drag_last: None,
        }
    }

    pub fn focus(&self) -> &FocusHandle {
        &self.focus
    }

    /// Pixel size of the shown image, when it could be read.
    pub fn size(&self) -> Option<Vec2> {
        self.size
    }

    /// Forgets the previous file: back to fit, no size.
    pub fn clear(&mut self) {
        self.zoom.reset();
        self.size = None;
        self.drag_last = None;
        self.geometry.bounds.set(None);
    }

    pub fn set_size(&mut self, size: Option<Vec2>) {
        self.size = size;
    }

    /// Applies a command around the viewport center; true when it changed.
    pub fn command(&mut self, command: ImageZoomCommand) -> bool {
        let Some(content) = self.size else {
            return false;
        };
        let viewport = self.geometry.viewport.get();
        let center = Vec2::new(viewport.x / 2.0, viewport.y / 2.0);
        let before = self.zoom;
        match command {
            ImageZoomCommand::In => self.zoom.zoom_steps(1, center, content, viewport),
            ImageZoomCommand::Out => self.zoom.zoom_steps(-1, center, content, viewport),
            ImageZoomCommand::Fit => self.zoom.reset(),
            ImageZoomCommand::ActualSize => self.zoom.actual_size(center, content, viewport),
        }
        before != self.zoom
    }

    /// Zooms by wheel steps around `anchor`; true when it changed.
    pub fn zoom_at(&mut self, steps: i32, anchor: Vec2) -> bool {
        let Some(content) = self.size else {
            return false;
        };
        let before = self.zoom;
        self.zoom
            .zoom_steps(steps, anchor, content, self.geometry.viewport.get());
        before != self.zoom
    }

    pub fn begin_drag(&mut self, position: Point<Pixels>) {
        self.drag_last = Some(position);
    }

    /// Pans by the pointer movement since the last position.
    pub fn drag_to(&mut self, position: Point<Pixels>) -> bool {
        let (Some(last), Some(content)) = (self.drag_last, self.size) else {
            return false;
        };
        self.drag_last = Some(position);
        let before = self.zoom;
        let delta = Vec2::new(
            f32::from(position.x - last.x),
            f32::from(position.y - last.y),
        );
        self.zoom
            .pan_by(delta, content, self.geometry.viewport.get());
        before != self.zoom
    }

    pub fn end_drag(&mut self) {
        self.drag_last = None;
    }

    /// Geometry shared with the window-zoom wheel capture.
    pub fn geometry(&self) -> Rc<ImageGeometry> {
        self.geometry.clone()
    }

    /// The image with its zoom indicator in the bottom-right corner.
    pub fn element(
        &self,
        path: PathBuf,
        content: Vec2,
        pane: WeakEntity<PreviewPane>,
        theme: &Theme,
    ) -> AnyElement {
        let label = self.zoom.label(content, self.geometry.viewport.get());
        div()
            .size_full()
            .relative()
            .child(ZoomableImage {
                path,
                content,
                zoom: self.zoom,
                pane,
                focus: self.focus.clone(),
                geometry: self.geometry.clone(),
                failure: "Cannot decode image",
            })
            .child(
                div()
                    .absolute()
                    .right_1()
                    .bottom_1()
                    .px_1p5()
                    .rounded(theme.radius)
                    .text_xs()
                    .bg(theme.background.opacity(0.8))
                    .text_color(theme.muted_foreground)
                    .child(label),
            )
            .into_any_element()
    }
}
