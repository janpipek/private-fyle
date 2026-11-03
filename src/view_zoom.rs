//! Zoom and pan of one piece of content (an image today) inside a viewport.
//!
//! UI-independent and content-agnostic: callers pass the content's intrinsic
//! size and the viewport size, so PDF pages or video frames can reuse it and
//! rotation can later swap the content size. Coordinates are logical pixels
//! with the origin at the viewport's top-left corner.

/// The smallest scale that can be set.
pub const MIN_SCALE: f32 = 0.05;
/// The largest scale that can be set.
pub const MAX_SCALE: f32 = 32.0;
/// Scale factor of one zoom step.
const STEP: f32 = 1.25;

/// A point or size in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Where the content is drawn inside the viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Top-left corner of the scaled content.
    pub origin: Vec2,
    /// Size of the scaled content.
    pub size: Vec2,
    pub scale: f32,
}

/// How the content is scaled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    /// Whole content visible, never enlarged beyond 100%.
    Fit,
    /// A fixed scale (1.0 is actual size).
    Scale(f32),
}

/// Zoom mode plus pan offset; `Fit` always shows the content centered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewZoom {
    mode: Mode,
    /// Top-left of the content while in `Scale` mode (clamped when used).
    origin: Vec2,
}

impl Default for ViewZoom {
    fn default() -> Self {
        Self {
            mode: Mode::Fit,
            origin: Vec2::default(),
        }
    }
}

fn valid(size: Vec2) -> bool {
    size.x > 0.0 && size.y > 0.0 && size.x.is_finite() && size.y.is_finite()
}

/// The scale at which the whole content fits; never above 1.0.
pub fn fit_scale(content: Vec2, viewport: Vec2) -> f32 {
    if !valid(content) || !valid(viewport) {
        return 1.0;
    }
    (viewport.x / content.x)
        .min(viewport.y / content.y)
        .min(1.0)
}

/// Clamps one axis: centered when the content is smaller than the viewport,
/// otherwise kept so that the content still covers the viewport.
fn clamp_axis(origin: f32, content: f32, viewport: f32) -> f32 {
    if content <= viewport {
        (viewport - content) / 2.0
    } else {
        origin.clamp(viewport - content, 0.0)
    }
}

impl ViewZoom {
    /// Starts in fit mode.
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn is_fit(&self) -> bool {
        self.mode == Mode::Fit
    }

    /// The scale in effect for this content and viewport.
    pub fn scale(&self, content: Vec2, viewport: Vec2) -> f32 {
        match self.mode {
            Mode::Fit => fit_scale(content, viewport),
            Mode::Scale(scale) => scale,
        }
    }

    /// Where to draw the content; the pan offset is clamped for the viewport.
    pub fn placement(&self, content: Vec2, viewport: Vec2) -> Placement {
        let scale = self.scale(content, viewport);
        let size = Vec2::new(content.x * scale, content.y * scale);
        let origin = match self.mode {
            Mode::Fit => Vec2::new(
                clamp_axis(0.0, size.x, viewport.x),
                clamp_axis(0.0, size.y, viewport.y),
            ),
            Mode::Scale(_) => Vec2::new(
                clamp_axis(self.origin.x, size.x, viewport.x),
                clamp_axis(self.origin.y, size.y, viewport.y),
            ),
        };
        Placement {
            origin,
            size,
            scale,
        }
    }

    /// Text for the corner indicator: "Fit" or a percentage.
    pub fn label(&self, content: Vec2, viewport: Vec2) -> String {
        match self.mode {
            Mode::Fit => "Fit".to_owned(),
            Mode::Scale(_) => format!("{}%", (self.scale(content, viewport) * 100.0).round()),
        }
    }

    /// Back to fit mode.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Sets `scale`, keeping the content point under `anchor` fixed.
    pub fn set_scale(&mut self, scale: f32, anchor: Vec2, content: Vec2, viewport: Vec2) {
        if !valid(content) || !valid(viewport) || !scale.is_finite() {
            return;
        }
        let old = self.placement(content, viewport);
        let scale = scale.clamp(MIN_SCALE, MAX_SCALE);
        let fit = fit_scale(content, viewport);
        // Zooming out to the fitted size is the same as fit mode.
        if scale <= fit && scale <= old.scale {
            self.reset();
            return;
        }
        let point = Vec2::new(
            (anchor.x - old.origin.x) / old.scale,
            (anchor.y - old.origin.y) / old.scale,
        );
        self.mode = Mode::Scale(scale);
        self.origin = Vec2::new(anchor.x - point.x * scale, anchor.y - point.y * scale);
        self.origin = self.placement(content, viewport).origin;
    }

    /// Zooms by `steps` (negative zooms out) around `anchor`.
    pub fn zoom_steps(&mut self, steps: i32, anchor: Vec2, content: Vec2, viewport: Vec2) {
        let current = self.scale(content, viewport);
        self.set_scale(current * STEP.powi(steps), anchor, content, viewport);
    }

    /// Shows the content at 100% around `anchor`.
    pub fn actual_size(&mut self, anchor: Vec2, content: Vec2, viewport: Vec2) {
        self.set_scale(1.0, anchor, content, viewport);
        // At 100% a small image is exactly what fit mode shows.
        if fit_scale(content, viewport) >= 1.0 {
            self.reset();
        }
    }

    /// Drags the content by `delta`; does nothing in fit mode.
    pub fn pan_by(&mut self, delta: Vec2, content: Vec2, viewport: Vec2) {
        if self.is_fit() || !valid(content) || !valid(viewport) {
            return;
        }
        let at = self.placement(content, viewport).origin;
        self.origin = Vec2::new(at.x + delta.x, at.y + delta.y);
        self.origin = self.placement(content, viewport).origin;
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_SCALE, MIN_SCALE, Mode, Vec2, ViewZoom, fit_scale};

    const VIEW: Vec2 = Vec2::new(400.0, 300.0);

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn fit_shrinks_large_content_and_never_enlarges() {
        assert!(near(fit_scale(Vec2::new(800.0, 300.0), VIEW), 0.5));
        assert!(near(fit_scale(Vec2::new(400.0, 600.0), VIEW), 0.5));
        assert_eq!(fit_scale(Vec2::new(50.0, 50.0), VIEW), 1.0);
        assert_eq!(fit_scale(Vec2::new(0.0, 50.0), VIEW), 1.0);
    }

    #[test]
    fn fit_centers_content() {
        let placement = ViewZoom::new().placement(Vec2::new(50.0, 50.0), VIEW);
        assert_eq!(placement.size, Vec2::new(50.0, 50.0));
        assert_eq!(placement.origin, Vec2::new(175.0, 125.0));
    }

    #[test]
    fn zoom_in_keeps_the_anchor_fixed() {
        let content = Vec2::new(800.0, 600.0);
        let mut zoom = ViewZoom::new();
        let anchor = Vec2::new(100.0, 80.0);
        let before = zoom.placement(content, VIEW);
        let point = (
            (anchor.x - before.origin.x) / before.scale,
            (anchor.y - before.origin.y) / before.scale,
        );
        zoom.zoom_steps(4, anchor, content, VIEW);
        let after = zoom.placement(content, VIEW);
        assert!(after.scale > 1.0);
        assert!(near(after.origin.x + point.0 * after.scale, anchor.x));
        assert!(near(after.origin.y + point.1 * after.scale, anchor.y));
    }

    #[test]
    fn zooming_out_to_the_fitted_size_returns_to_fit() {
        let content = Vec2::new(800.0, 600.0);
        let center = Vec2::new(200.0, 150.0);
        let mut zoom = ViewZoom::new();
        zoom.zoom_steps(2, center, content, VIEW);
        assert!(!zoom.is_fit());
        zoom.zoom_steps(-2, center, content, VIEW);
        assert!(zoom.is_fit());
        zoom.zoom_steps(-3, center, content, VIEW);
        assert!(zoom.is_fit());
    }

    #[test]
    fn scale_is_clamped() {
        let content = Vec2::new(800.0, 600.0);
        let mut zoom = ViewZoom::new();
        zoom.zoom_steps(200, Vec2::default(), content, VIEW);
        assert_eq!(zoom.mode(), Mode::Scale(MAX_SCALE));
        zoom.set_scale(0.001, Vec2::default(), content, VIEW);
        assert!(zoom.is_fit());
        // Huge content stops at the minimum scale instead of shrinking forever.
        let big = Vec2::new(100_000.0, 100_000.0);
        let mut zoom = ViewZoom::new();
        zoom.set_scale(0.0001, Vec2::default(), big, VIEW);
        assert_eq!(zoom.mode(), Mode::Scale(MIN_SCALE));
    }

    #[test]
    fn actual_size_shows_one_to_one() {
        let content = Vec2::new(800.0, 600.0);
        let mut zoom = ViewZoom::new();
        zoom.actual_size(Vec2::new(200.0, 150.0), content, VIEW);
        let placement = zoom.placement(content, VIEW);
        assert_eq!(placement.scale, 1.0);
        assert_eq!(placement.size, content);
        assert_eq!(zoom.label(content, VIEW), "100%");
        // The viewport center stays on the content center.
        assert!(near(placement.origin.x, -200.0));
        assert!(near(placement.origin.y, -150.0));
    }

    #[test]
    fn actual_size_of_small_content_is_fit() {
        let mut zoom = ViewZoom::new();
        zoom.actual_size(Vec2::default(), Vec2::new(50.0, 50.0), VIEW);
        assert!(zoom.is_fit());
    }

    #[test]
    fn pan_is_clamped_and_ignored_in_fit_mode() {
        let content = Vec2::new(800.0, 600.0);
        let mut zoom = ViewZoom::new();
        zoom.pan_by(Vec2::new(50.0, 50.0), content, VIEW);
        assert!(zoom.is_fit());
        zoom.actual_size(Vec2::new(200.0, 150.0), content, VIEW);
        zoom.pan_by(Vec2::new(10_000.0, 10_000.0), content, VIEW);
        let placement = zoom.placement(content, VIEW);
        assert_eq!(placement.origin, Vec2::new(0.0, 0.0));
        zoom.pan_by(Vec2::new(-10_000.0, -10_000.0), content, VIEW);
        let placement = zoom.placement(content, VIEW);
        assert_eq!(placement.origin, Vec2::new(-400.0, -300.0));
    }

    #[test]
    fn content_smaller_than_the_viewport_stays_centered_on_one_axis() {
        // Narrow, tall content at 100%: centered horizontally, pannable vertically.
        let content = Vec2::new(200.0, 2000.0);
        let mut zoom = ViewZoom::new();
        zoom.actual_size(Vec2::new(200.0, 150.0), content, VIEW);
        zoom.pan_by(Vec2::new(300.0, 500.0), content, VIEW);
        let placement = zoom.placement(content, VIEW);
        assert_eq!(placement.origin.x, 100.0);
        assert!(near(placement.origin.y, -350.0));
    }

    #[test]
    fn pan_survives_a_viewport_resize_by_clamping() {
        let content = Vec2::new(800.0, 600.0);
        let mut zoom = ViewZoom::new();
        zoom.actual_size(Vec2::new(200.0, 150.0), content, VIEW);
        zoom.pan_by(Vec2::new(-10_000.0, 0.0), content, VIEW);
        let wide = Vec2::new(700.0, 300.0);
        assert_eq!(zoom.placement(content, wide).origin.x, -100.0);
    }

    #[test]
    fn label_names_the_mode() {
        let content = Vec2::new(800.0, 600.0);
        let mut zoom = ViewZoom::new();
        assert_eq!(zoom.label(content, VIEW), "Fit");
        zoom.set_scale(2.0, Vec2::default(), content, VIEW);
        assert_eq!(zoom.label(content, VIEW), "200%");
        zoom.reset();
        assert!(zoom.is_fit());
    }
}
