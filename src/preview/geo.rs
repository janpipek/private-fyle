//! GPX and FIT preview adapter with a bounded route outline image.

use std::io;

use image::{Rgba, RgbaImage};

use super::{Content, PreviewStyle, Previewer, Probe, cache_file};
use crate::formats::geo::{self, GeoData, Point};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;
const PADDING: f64 = 24.0;
const MAX_DRAW_STEPS: i32 = 400_000;

/// Previewer for GPX and FIT routes and activities.
pub struct GeoPreviewer;

impl Previewer for GeoPreviewer {
    fn name(&self) -> &'static str {
        "Route / activity"
    }

    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir && matches!(probe.ext.as_str(), "gpx" | "fit")
    }

    fn load(&self, probe: &Probe) -> io::Result<Content> {
        self.load_styled(probe, PreviewStyle::default())
    }

    fn load_styled(&self, probe: &Probe, style: PreviewStyle) -> io::Result<Content> {
        let data = geo::read(&probe.path, || false)?;
        let outline = if data.points > 0 {
            Some(render_outline(&probe.path, &data, style)?)
        } else {
            None
        };
        Ok(Content::Geo {
            info: summary(&data),
            outline,
        })
    }
}

fn summary(data: &GeoData) -> Vec<(String, String)> {
    let mut fields = vec![("Format".into(), data.format.into())];
    for (label, value) in [
        ("Type", data.kind.as_ref()),
        ("Name", data.name.as_ref()),
        ("Sport", data.sport.as_ref()),
        ("Start", data.start.as_ref()),
    ] {
        if let Some(value) = value {
            fields.push((label.into(), value.clone()));
        }
    }
    if let Some(seconds) = data.duration_seconds {
        fields.push(("Duration".into(), format_duration(seconds)));
    }
    if let Some(meters) = data.distance_meters {
        fields.push(("Distance".into(), format!("{:.2} km", meters / 1000.0)));
    }
    for (label, meters) in [
        ("Ascent", data.ascent_meters),
        ("Descent", data.descent_meters),
    ] {
        if let Some(meters) = meters {
            fields.push((label.into(), format!("{meters:.0} m")));
        }
    }
    for (label, count) in [
        ("Tracks", data.tracks),
        ("Routes", data.routes),
        ("Waypoints", data.waypoints),
        ("Laps", data.laps),
        ("GPS points", data.points),
    ] {
        if count > 0 {
            fields.push((label.into(), count.to_string()));
        }
    }
    if data.truncated {
        fields.push((
            "Read limit".into(),
            format!("first {} GPS points", geo::MAX_POINTS),
        ));
    }
    fields
}

fn format_duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0).round() as u64;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn render_outline(
    path: &std::path::Path,
    data: &GeoData,
    style: PreviewStyle,
) -> io::Result<std::path::PathBuf> {
    let colors = [style.foreground, style.background];
    let colors_with_markers = [style.success, style.danger];
    let key: String = colors
        .into_iter()
        .flatten()
        .chain(colors_with_markers.into_iter().flatten())
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let cached = cache_file(path, &format!("route-lines-{key}.png"))?;
    if cached.is_file() {
        return Ok(cached);
    }
    let points: Vec<_> = data.segments.iter().flatten().copied().collect();
    let reference = points.first().map_or(0.0, |point| point.longitude);
    let longitude = |point: Point| {
        let mut delta = point.longitude - reference;
        if delta > 180.0 {
            delta -= 360.0;
        }
        if delta < -180.0 {
            delta += 360.0;
        }
        delta
    };
    let min_x = points
        .iter()
        .map(|p| longitude(*p))
        .fold(f64::INFINITY, f64::min);
    let max_x = points
        .iter()
        .map(|p| longitude(*p))
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = points
        .iter()
        .map(|p| p.latitude)
        .fold(f64::INFINITY, f64::min);
    let max_y = points
        .iter()
        .map(|p| p.latitude)
        .fold(f64::NEG_INFINITY, f64::max);
    let cos_lat = ((min_y + max_y) / 2.0).to_radians().cos().abs().max(0.01);
    let span_x = ((max_x - min_x) * cos_lat).max(0.000001);
    let span_y = (max_y - min_y).max(0.000001);
    let scale = ((f64::from(WIDTH) - 2.0 * PADDING) / span_x)
        .min((f64::from(HEIGHT) - 2.0 * PADDING) / span_y);
    let x_offset = (f64::from(WIDTH) - span_x * scale) / 2.0;
    let y_offset = (f64::from(HEIGHT) - span_y * scale) / 2.0;
    let project = |point: Point| {
        let x = x_offset + (longitude(point) - min_x) * cos_lat * scale;
        let y = y_offset + (max_y - point.latitude) * scale;
        (x.round() as i32, y.round() as i32)
    };
    let mut image = RgbaImage::new(WIDTH, HEIGHT);
    let total_lines: usize = data
        .segments
        .iter()
        .map(|segment| segment.len().saturating_sub(1))
        .sum();
    let stride = total_lines.div_ceil(2048).max(1);
    let mut drawn_steps = 0;
    let mut lines = Vec::new();
    for segment in &data.segments {
        let mut previous = None;
        for (index, location) in segment.iter().copied().enumerate() {
            if index % stride != 0 && index + 1 != segment.len() {
                continue;
            }
            let current = project(location);
            let Some(a) = previous.replace(current) else {
                continue;
            };
            let b = current;
            let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs());
            if drawn_steps + steps > MAX_DRAW_STEPS {
                break;
            }
            drawn_steps += steps;
            lines.push((a, b));
        }
    }
    // Finish the halo before drawing tracks so adjacent edges cannot erase them.
    for &(a, b) in &lines {
        line(&mut image, a, b, 6, opaque(style.background));
    }
    for &(a, b) in &lines {
        line(&mut image, a, b, 4, opaque(style.foreground));
    }
    if let Some(first) = points.first() {
        dot(&mut image, project(*first), 6, opaque(style.success));
    }
    if let Some(last) = points.last() {
        dot(&mut image, project(*last), 5, opaque(style.danger));
    }
    image.save(&cached).map_err(io::Error::other)?;
    Ok(cached)
}

fn opaque(rgb: [u8; 3]) -> Rgba<u8> {
    Rgba([rgb[0], rgb[1], rgb[2], 255])
}

fn line(image: &mut RgbaImage, a: (i32, i32), b: (i32, i32), radius: i32, color: Rgba<u8>) {
    let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).max(1);
    for step in 0..=steps {
        let fraction = f64::from(step) / f64::from(steps);
        let x = (f64::from(a.0) + f64::from(b.0 - a.0) * fraction).round() as i32;
        let y = (f64::from(a.1) + f64::from(b.1 - a.1) * fraction).round() as i32;
        dot(image, (x, y), radius, color);
    }
}

fn dot(image: &mut RgbaImage, center: (i32, i32), radius: i32, color: Rgba<u8>) {
    for y in -radius..=radius {
        for x in -radius..=radius {
            if x * x + y * y <= radius * radius {
                let px = center.0 + x;
                let py = center.1 + y;
                if px >= 0 && py >= 0 && px < WIDTH as i32 && py < HEIGHT as i32 {
                    image.put_pixel(px as u32, py as u32, color);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GeoData, Point, PreviewStyle, render_outline};

    #[test]
    fn dense_track_remains_continuous() {
        let dir = tempfile::TempDir::new().expect("temporary directory");
        let source = dir.path().join("dense.gpx");
        std::fs::write(&source, b"dense route").expect("source");
        let segment: Vec<_> = (0..=1000)
            .map(|index| Point {
                latitude: 0.0,
                longitude: f64::from(index) / 1000.0,
            })
            .collect();
        let data = GeoData {
            points: segment.len(),
            segments: vec![segment],
            ..GeoData::default()
        };
        let style = PreviewStyle::default();
        let path = render_outline(&source, &data, style).expect("outline");
        let image = image::open(path).expect("decode").into_rgba8();
        for x in 40..920 {
            assert_eq!(image.get_pixel(x, 270).0, [12, 20, 31, 255], "x={x}");
        }
    }

    #[test]
    fn route_line_uses_readable_theme_foreground() {
        let dir = tempfile::TempDir::new().expect("temporary directory");
        let source = dir.path().join("route.gpx");
        std::fs::write(&source, b"route").expect("source");
        let data = GeoData {
            points: 2,
            segments: vec![vec![
                Point {
                    latitude: 0.0,
                    longitude: 0.0,
                },
                Point {
                    latitude: 1.0,
                    longitude: 1.0,
                },
            ]],
            ..GeoData::default()
        };
        let style = PreviewStyle::default();
        let path = render_outline(&source, &data, style).expect("outline");
        let image = image::open(path).expect("decode").into_rgba8();
        assert_eq!(image.get_pixel(480, 270).0, [12, 20, 31, 255]);

        let light = PreviewStyle {
            foreground: [240, 235, 225],
            background: [16, 20, 28],
            ..style
        };
        let path_dark = render_outline(&source, &data, light).expect("dark outline");
        assert_ne!(
            path_dark,
            render_outline(&source, &data, style).expect("light outline")
        );
        let dark_image = image::open(path_dark).expect("decode dark").into_rgba8();
        assert_eq!(dark_image.get_pixel(480, 270).0, [240, 235, 225, 255]);
    }
}
