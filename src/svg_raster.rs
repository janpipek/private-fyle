//! Sharp SVG zoom: parses SVGs and plans and renders high-resolution tiles.
//!
//! GPUI rasterizes an SVG once at twice its intrinsic size, so zooming past
//! 200% blurs. Above that scale the preview draws a tile instead: only the
//! visible part of the SVG (plus a margin for small pans), rendered by resvg
//! at the on-screen pixel density. The tile is capped at [`MAX_TILE_PIXELS`]
//! pixels, so memory stays bounded at any zoom; only a very large viewport
//! at very high zoom falls back to a slightly lower density.
//! UI-independent: the view passes placement, viewport and window scale.

use std::io::Read;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use gpui_kit::RenderImage;
use resvg::tiny_skia::{Pixmap, Transform};

use crate::view_zoom::{Placement, Vec2};

/// Largest SVG file that is parsed for the preview.
pub const MAX_SVG_BYTES: u64 = 16 * 1024 * 1024;
/// Zoom scale up to which GPUI's own 2x rasterization is sharp enough.
pub const BASE_SCALE: f32 = 2.0;
/// Most pixels one tile may hold (16 Mpx, 64 MB).
pub const MAX_TILE_PIXELS: f64 = 16.0 * 1024.0 * 1024.0;
/// Longest side of a tile; GPU textures are limited.
const MAX_TILE_SIDE: f64 = 8192.0;
/// Extra area rendered around the visible part, as a fraction of its size.
const MARGIN: f32 = 0.5;
const EPSILON: f32 = 1e-3;

/// An axis-aligned rectangle in SVG user units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    fn width(&self) -> f32 {
        self.x1 - self.x0
    }
    fn height(&self) -> f32 {
        self.y1 - self.y0
    }
    fn contains(&self, other: &Rect) -> bool {
        self.x0 <= other.x0 + EPSILON
            && self.y0 <= other.y0 + EPSILON
            && self.x1 >= other.x1 - EPSILON
            && self.y1 >= other.y1 - EPSILON
    }
}

/// One rendered region: where it is and at which pixel density.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tile {
    /// Device pixels per SVG unit.
    pub scale: f32,
    /// Top-left pixel of the tile on the scaled SVG.
    pub origin_px: (i32, i32),
    /// Tile size in pixels.
    pub size_px: (u32, u32),
}

impl Tile {
    /// The covered region in SVG units.
    pub fn region(&self) -> Rect {
        let s = self.scale;
        Rect {
            x0: self.origin_px.0 as f32 / s,
            y0: self.origin_px.1 as f32 / s,
            x1: (self.origin_px.0 as f64 + self.size_px.0 as f64) as f32 / s,
            y1: (self.origin_px.1 as f64 + self.size_px.1 as f64) as f32 / s,
        }
    }

    fn pixels(&self) -> f64 {
        self.size_px.0 as f64 * self.size_px.1 as f64
    }
}

/// What the view should have for the current zoom state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plan {
    /// The part of the SVG on screen, in SVG units.
    pub visible: Rect,
    /// The tile to render for it.
    pub tile: Tile,
}

impl Plan {
    /// Whether `have` is sharp and large enough for this plan.
    pub fn satisfied_by(&self, have: &Tile) -> bool {
        (have.scale - self.tile.scale).abs() <= self.tile.scale * EPSILON
            && have.region().contains(&self.visible)
    }
}

/// Rounds a region out to whole pixels at `scale`.
fn snap(region: Rect, scale: f32) -> Tile {
    let x0 = (region.x0 as f64 * scale as f64).floor();
    let y0 = (region.y0 as f64 * scale as f64).floor();
    let x1 = (region.x1 as f64 * scale as f64).ceil();
    let y1 = (region.y1 as f64 * scale as f64).ceil();
    Tile {
        scale,
        origin_px: (x0 as i32, y0 as i32),
        size_px: ((x1 - x0) as u32, (y1 - y0) as u32),
    }
}

fn fits(tile: &Tile) -> bool {
    tile.pixels() <= MAX_TILE_PIXELS
        && f64::from(tile.size_px.0) <= MAX_TILE_SIDE
        && f64::from(tile.size_px.1) <= MAX_TILE_SIDE
}

/// Plans the tile for an SVG of `content` units drawn at `placement` in a
/// `viewport`, on a window with `window_scale` device pixels per logical
/// pixel. `None` when GPUI's own rasterization is sharp enough.
pub fn plan(
    placement: &Placement,
    viewport: Vec2,
    content: Vec2,
    window_scale: f32,
) -> Option<Plan> {
    let zoom = placement.scale;
    let valid = zoom > BASE_SCALE && window_scale > 0.0 && content.x > 0.0 && content.y > 0.0;
    if !valid {
        return None;
    }
    let visible = Rect {
        x0: (-placement.origin.x / zoom).max(0.0),
        y0: (-placement.origin.y / zoom).max(0.0),
        x1: ((viewport.x - placement.origin.x) / zoom).min(content.x),
        y1: ((viewport.y - placement.origin.y) / zoom).min(content.y),
    };
    if visible.width() <= 0.0 || visible.height() <= 0.0 {
        return None;
    }
    let mut scale = zoom * window_scale;
    let expand = |margin: f32| Rect {
        x0: (visible.x0 - visible.width() * margin).max(0.0),
        y0: (visible.y0 - visible.height() * margin).max(0.0),
        x1: (visible.x1 + visible.width() * margin).min(content.x),
        y1: (visible.y1 + visible.height() * margin).min(content.y),
    };
    let mut tile = snap(expand(MARGIN), scale);
    if !fits(&tile) {
        tile = snap(expand(0.0), scale);
    }
    // A viewport too big for the cap at this zoom: lower the density a bit.
    while !fits(&tile) {
        let by = (MAX_TILE_PIXELS / tile.pixels())
            .sqrt()
            .min(MAX_TILE_SIDE / f64::from(tile.size_px.0.max(tile.size_px.1)))
            .min(0.98);
        scale *= by as f32;
        tile = snap(expand(0.0), scale);
    }
    Some(Plan { visible, tile })
}

/// Parsed SVG with its own size in user units.
pub struct Svg {
    pub tree: Arc<usvg::Tree>,
    pub size: Vec2,
}

/// Points generic families (sans-serif, ...) that no installed font answers
/// to at an installed one, so text in SVGs still renders.
fn fix_generic_families(db: &mut usvg::fontdb::Database) {
    use usvg::fontdb::{Family, Query};
    const PREFERRED: &[&str] = &[
        "Noto Sans",
        "DejaVu Sans",
        "Liberation Sans",
        "Cantarell",
        "Ubuntu",
        "Arial",
    ];
    let preferred = PREFERRED.iter().find_map(|name| {
        let found = db.query(&Query {
            families: &[Family::Name(name)],
            ..Query::default()
        });
        found.map(|_| (*name).to_owned())
    });
    let Some(fallback) = preferred.or_else(|| {
        db.faces()
            .find_map(|face| face.families.first().map(|(name, _)| name.clone()))
    }) else {
        return;
    };
    let missing = |db: &usvg::fontdb::Database, family| {
        db.query(&Query {
            families: &[family],
            ..Query::default()
        })
        .is_none()
    };
    if missing(db, Family::SansSerif) {
        db.set_sans_serif_family(&fallback);
    }
    if missing(db, Family::Serif) {
        db.set_serif_family(&fallback);
    }
    if missing(db, Family::Monospace) {
        db.set_monospace_family(&fallback);
    }
}

fn options() -> usvg::Options<'static> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        fix_generic_families(&mut db);
        Arc::new(db)
    });
    usvg::Options {
        fontdb: fonts.clone(),
        ..usvg::Options::default()
    }
}

/// Parses SVG bytes; `None` when they are not a usable SVG.
pub fn parse(bytes: &[u8]) -> Option<Svg> {
    let tree = usvg::Tree::from_data(bytes, &options()).ok()?;
    let size = tree.size();
    let size = Vec2::new(size.width(), size.height());
    (size.x > 0.0 && size.y > 0.0 && size.x.is_finite() && size.y.is_finite()).then(|| Svg {
        tree: Arc::new(tree),
        size,
    })
}

/// Reads and parses an `.svg` file of at most [`MAX_SVG_BYTES`].
pub fn load(path: &Path) -> Option<Svg> {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
    {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_SVG_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_SVG_BYTES {
        return None;
    }
    parse(&bytes)
}

/// Renders the tile into a GPUI image (premultiplied BGRA).
pub fn render(tree: &usvg::Tree, tile: &Tile) -> Option<Arc<RenderImage>> {
    let mut pixmap = Pixmap::new(tile.size_px.0, tile.size_px.1)?;
    let transform = Transform::from_scale(tile.scale, tile.scale)
        .post_translate(-(tile.origin_px.0 as f32), -(tile.origin_px.1 as f32));
    resvg::render(tree, transform, &mut pixmap.as_mut());
    let (width, height) = (pixmap.width(), pixmap.height());
    let mut pixels = pixmap.take();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(width, height, pixels)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

#[cfg(test)]
mod tests {
    use super::{BASE_SCALE, MAX_TILE_PIXELS, Tile, parse, plan, render};
    use crate::view_zoom::{Placement, Vec2};

    const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50">
        <rect x="10" y="10" width="20" height="20" fill="#ff0000"/></svg>"##;

    fn placement(scale: f32, x: f32, y: f32, content: Vec2) -> Placement {
        Placement {
            origin: Vec2::new(x, y),
            size: Vec2::new(content.x * scale, content.y * scale),
            scale,
        }
    }

    #[test]
    fn parse_reads_own_size_and_rejects_garbage() {
        let svg = parse(SVG).expect("valid svg");
        assert_eq!(svg.size, Vec2::new(100.0, 50.0));
        assert!(parse(b"not an svg").is_none());
    }

    #[test]
    fn low_zoom_needs_no_tile() {
        let content = Vec2::new(100.0, 50.0);
        let p = placement(BASE_SCALE, 0.0, 0.0, content);
        assert!(plan(&p, Vec2::new(100.0, 100.0), content, 1.0).is_none());
    }

    #[test]
    fn plan_covers_visible_part_with_margin_at_screen_density() {
        let content = Vec2::new(1000.0, 1000.0);
        // 8x zoom, panned so content (100..200, 50..150) is visible.
        let p = placement(8.0, -800.0, -400.0, content);
        let plan = plan(&p, Vec2::new(800.0, 800.0), content, 2.0).expect("plan");
        assert_eq!(plan.visible.x0, 100.0);
        assert_eq!(plan.visible.x1, 200.0);
        assert_eq!(plan.tile.scale, 16.0);
        assert!(plan.tile.region().contains(&plan.visible));
        assert!(plan.tile.region().x0 < 100.0, "has a margin");
        assert!(plan.satisfied_by(&plan.tile));
    }

    #[test]
    fn region_is_clipped_to_the_content() {
        let content = Vec2::new(100.0, 50.0);
        let p = placement(10.0, 0.0, 0.0, content);
        let plan = plan(&p, Vec2::new(5000.0, 5000.0), content, 1.0).expect("plan");
        assert_eq!(plan.visible.x1, 100.0);
        assert_eq!(plan.tile.size_px, (1000, 500));
    }

    #[test]
    fn tile_never_exceeds_the_pixel_cap() {
        let content = Vec2::new(100_000.0, 100_000.0);
        let p = placement(32.0, -1000.0, -1000.0, content);
        let plan = plan(&p, Vec2::new(4000.0, 3000.0), content, 2.0).expect("plan");
        assert!(plan.tile.size_px.0 as f64 * plan.tile.size_px.1 as f64 <= MAX_TILE_PIXELS);
        assert!(plan.tile.size_px.0 <= 8192 && plan.tile.size_px.1 <= 8192);
        assert!(plan.tile.scale < 64.0, "density lowered");
    }

    #[test]
    fn panning_beyond_the_tile_needs_a_new_one() {
        let content = Vec2::new(1000.0, 1000.0);
        let viewport = Vec2::new(400.0, 400.0);
        let first = plan(&placement(8.0, 0.0, 0.0, content), viewport, content, 1.0).unwrap();
        let near = plan(&placement(8.0, -40.0, 0.0, content), viewport, content, 1.0).unwrap();
        let far = plan(
            &placement(8.0, -2000.0, 0.0, content),
            viewport,
            content,
            1.0,
        )
        .unwrap();
        assert!(near.satisfied_by(&first.tile));
        assert!(!far.satisfied_by(&first.tile));
        let other_zoom = plan(&placement(9.0, 0.0, 0.0, content), viewport, content, 1.0).unwrap();
        assert!(!other_zoom.satisfied_by(&first.tile));
    }

    #[test]
    fn render_draws_the_requested_region() {
        let svg = parse(SVG).expect("valid svg");
        // Region starting at (5, 5) units at 4 px per unit: the red square
        // spans pixels 20..100 of a 200x80 tile.
        let tile = Tile {
            scale: 4.0,
            origin_px: (20, 20),
            size_px: (200, 80),
        };
        let image = render(&svg.tree, &tile).expect("rendered");
        let bytes = image.as_bytes(0).expect("frame");
        let pixel = |x: usize, y: usize| &bytes[(y * 200 + x) * 4..][..4];
        // BGRA: red is [0, 0, 255, 255].
        assert_eq!(pixel(40, 20), &[0, 0, 255, 255]);
        assert_eq!(pixel(150, 60)[3], 0);
    }
}
