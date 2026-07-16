use std::sync::OnceLock;

use roxmltree::Document;
use svgtypes::{PathParser, PathSegment};
#[cfg(not(target_os = "macos"))]
use tauri::image::Image;
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Transform};

const SOURCE: &str = include_str!("../../assets/openquota-tray.svg");
const SOURCE_SIZE: f32 = 24.0;
const ICON_SIZE: u32 = 32;
const SEGMENT_COUNT: usize = 6;

type Rgba = (u8, u8, u8, u8);

#[derive(Clone, Copy)]
struct Hsv {
    hue: f64,
    saturation: f64,
    value: f64,
}

const TRACK_COLOR: Rgba = (142, 142, 147, 150);
const HEALTHY_COLOR: Rgba = (22, 137, 239, 255);
const CAUTION_COLOR: Rgba = (240, 195, 60, 255);
const WARNING_COLOR: Rgba = (237, 129, 43, 255);
const CRITICAL_COLOR: Rgba = (227, 72, 63, 255);

const QUOTA_COLOR_STOPS: &[(f64, Rgba)] = &[
    (0.0, CRITICAL_COLOR),
    (0.2, WARNING_COLOR),
    (0.45, CAUTION_COLOR),
    (0.7, HEALTHY_COLOR),
    (1.0, HEALTHY_COLOR),
];

struct GaugePaths {
    track: Vec<Path>,
    fill: Vec<Path>,
    q_tail: Path,
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn render_gauge(display_fraction: f64, remaining_fraction: f64) -> Image<'static> {
    Image::new_owned(
        render_rgba(display_fraction, remaining_fraction),
        ICON_SIZE,
        ICON_SIZE,
    )
}

fn render_rgba(display_fraction: f64, remaining_fraction: f64) -> Vec<u8> {
    let paths = gauge_paths();
    let mut pixmap = Pixmap::new(ICON_SIZE, ICON_SIZE).expect("tray icon dimensions are valid");
    let transform = Transform::from_scale(
        ICON_SIZE as f32 / SOURCE_SIZE,
        ICON_SIZE as f32 / SOURCE_SIZE,
    );

    let quota_color = quota_color(remaining_fraction);
    draw_paths(&mut pixmap, &paths.track, TRACK_COLOR, transform);
    draw_paths(
        &mut pixmap,
        &paths.fill[..visible_segments(display_fraction)],
        quota_color,
        transform,
    );
    draw_paths(
        &mut pixmap,
        std::slice::from_ref(&paths.q_tail),
        quota_color,
        transform,
    );

    pixmap.take_demultiplied()
}

fn draw_paths(pixmap: &mut Pixmap, paths: &[Path], color: Rgba, transform: Transform) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.0, color.1, color.2, color.3);
    paint.anti_alias = true;
    for path in paths {
        pixmap.fill_path(path, &paint, FillRule::Winding, transform, None);
    }
}

fn visible_segments(fraction: f64) -> usize {
    if !fraction.is_finite() {
        return 0;
    }
    (fraction.clamp(0.0, 1.0) * SEGMENT_COUNT as f64)
        .round()
        .clamp(0.0, SEGMENT_COUNT as f64) as usize
}

fn quota_color(remaining_fraction: f64) -> Rgba {
    let remaining = if remaining_fraction.is_finite() {
        remaining_fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    for stops in QUOTA_COLOR_STOPS.windows(2) {
        let (start_fraction, start_color) = stops[0];
        let (end_fraction, end_color) = stops[1];
        if remaining <= end_fraction {
            let progress = (remaining - start_fraction) / (end_fraction - start_fraction);
            return interpolate_color(start_color, end_color, progress);
        }
    }
    HEALTHY_COLOR
}

fn interpolate_color(start: Rgba, end: Rgba, progress: f64) -> Rgba {
    let progress = progress.clamp(0.0, 1.0);
    let start_hsv = rgb_to_hsv(start);
    let end_hsv = rgb_to_hsv(end);
    let mut hue_delta = (end_hsv.hue - start_hsv.hue).rem_euclid(360.0);
    if hue_delta > 180.0 {
        hue_delta -= 360.0;
    }
    let lerp = |start: f64, end: f64| start + (end - start) * progress;
    let alpha = lerp(start.3 as f64, end.3 as f64).round() as u8;
    hsv_to_rgba(
        Hsv {
            hue: (start_hsv.hue + hue_delta * progress).rem_euclid(360.0),
            saturation: lerp(start_hsv.saturation, end_hsv.saturation),
            value: lerp(start_hsv.value, end_hsv.value),
        },
        alpha,
    )
}

fn rgb_to_hsv(color: Rgba) -> Hsv {
    let red = color.0 as f64 / 255.0;
    let green = color.1 as f64 / 255.0;
    let blue = color.2 as f64 / 255.0;
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let chroma = max - min;
    let hue = if chroma == 0.0 {
        0.0
    } else if max == red {
        60.0 * ((green - blue) / chroma).rem_euclid(6.0)
    } else if max == green {
        60.0 * ((blue - red) / chroma + 2.0)
    } else {
        60.0 * ((red - green) / chroma + 4.0)
    };
    Hsv {
        hue,
        saturation: if max == 0.0 { 0.0 } else { chroma / max },
        value: max,
    }
}

fn hsv_to_rgba(color: Hsv, alpha: u8) -> Rgba {
    let chroma = color.value * color.saturation;
    let sector = color.hue / 60.0;
    let secondary = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (red, green, blue) = match sector.floor() as u8 {
        0 => (chroma, secondary, 0.0),
        1 => (secondary, chroma, 0.0),
        2 => (0.0, chroma, secondary),
        3 => (0.0, secondary, chroma),
        4 => (secondary, 0.0, chroma),
        _ => (chroma, 0.0, secondary),
    };
    let match_value = color.value - chroma;
    let channel = |value: f64| ((value + match_value) * 255.0).round() as u8;
    (channel(red), channel(green), channel(blue), alpha)
}

fn gauge_paths() -> &'static GaugePaths {
    static PATHS: OnceLock<GaugePaths> = OnceLock::new();
    PATHS.get_or_init(|| {
        parse_gauge_paths().unwrap_or_else(|error| panic!("invalid bundled tray SVG: {error}"))
    })
}

fn parse_gauge_paths() -> Result<GaugePaths, String> {
    let document = Document::parse(SOURCE).map_err(|error| error.to_string())?;
    require_element(&document, "openquota-tray")?;

    let mut track = Vec::with_capacity(SEGMENT_COUNT);
    let mut fill = Vec::with_capacity(SEGMENT_COUNT);
    for index in 1..=SEGMENT_COUNT {
        track.push(parse_named_path(
            &document,
            &format!("track-segment-{index}"),
            Some("track"),
        )?);
        fill.push(parse_named_path(
            &document,
            &format!("fill-segment-{index}"),
            Some("fill"),
        )?);
    }
    let q_tail = parse_named_path(&document, "q-tail", None)?;
    Ok(GaugePaths {
        track,
        fill,
        q_tail,
    })
}

fn require_element<'a>(
    document: &'a Document<'a>,
    id: &str,
) -> Result<roxmltree::Node<'a, 'a>, String> {
    document
        .descendants()
        .find(|node| node.is_element() && node.attribute("id") == Some(id))
        .ok_or_else(|| format!("missing #{id}"))
}

fn parse_named_path(
    document: &Document<'_>,
    id: &str,
    expected_group: Option<&str>,
) -> Result<Path, String> {
    let node = require_element(document, id)?;
    if node.tag_name().name() != "path" {
        return Err(format!("#{id} must be a path"));
    }
    if let Some(group) = expected_group {
        let is_grouped = node
            .ancestors()
            .any(|ancestor| ancestor.attribute("id") == Some(group));
        if !is_grouped {
            return Err(format!("#{id} must be inside #{group}"));
        }
    }
    let data = node
        .attribute("d")
        .ok_or_else(|| format!("#{id} has no path data"))?;
    parse_path_data(data).map_err(|error| format!("#{id}: {error}"))
}

fn parse_path_data(data: &str) -> Result<Path, String> {
    let mut builder = PathBuilder::new();
    for segment in PathParser::from(data) {
        match segment.map_err(|error| error.to_string())? {
            PathSegment::MoveTo { abs: true, x, y } => builder.move_to(x as f32, y as f32),
            PathSegment::LineTo { abs: true, x, y } => builder.line_to(x as f32, y as f32),
            PathSegment::CurveTo {
                abs: true,
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => builder.cubic_to(
                x1 as f32, y1 as f32, x2 as f32, y2 as f32, x as f32, y as f32,
            ),
            PathSegment::ClosePath { .. } => builder.close(),
            _ => return Err("only absolute M, L, C and Z commands are supported".into()),
        }
    }
    builder.finish().ok_or_else(|| "path is empty".into())
}

#[cfg(test)]
mod tests {
    use super::{
        gauge_paths, quota_color, render_rgba, visible_segments, CAUTION_COLOR, CRITICAL_COLOR,
        HEALTHY_COLOR, ICON_SIZE, SEGMENT_COUNT, WARNING_COLOR,
    };

    #[test]
    fn bundled_svg_exposes_the_expected_geometry() {
        let paths = gauge_paths();
        assert_eq!(paths.track.len(), SEGMENT_COUNT);
        assert_eq!(paths.fill.len(), SEGMENT_COUNT);
        assert!(paths.q_tail.bounds().width() > 0.0);
    }

    #[test]
    fn fraction_maps_to_seven_stable_segment_states() {
        assert_eq!(visible_segments(f64::NAN), 0);
        assert_eq!(visible_segments(-1.0), 0);
        assert_eq!(visible_segments(0.0), 0);
        assert_eq!(visible_segments(0.17), 1);
        assert_eq!(visible_segments(0.5), 3);
        assert_eq!(visible_segments(0.99), 6);
        assert_eq!(visible_segments(2.0), 6);
    }

    #[test]
    fn remaining_quota_moves_smoothly_through_the_status_palette() {
        let channel_spread = |color: (u8, u8, u8, u8)| {
            color.0.max(color.1).max(color.2) - color.0.min(color.1).min(color.2)
        };
        assert_eq!(quota_color(f64::NAN), CRITICAL_COLOR);
        assert_eq!(quota_color(0.0), CRITICAL_COLOR);
        assert_eq!(quota_color(0.2), WARNING_COLOR);
        assert_eq!(quota_color(0.45), CAUTION_COLOR);
        assert_eq!(quota_color(0.7), HEALTHY_COLOR);
        assert_eq!(quota_color(1.0), HEALTHY_COLOR);
        assert_ne!(quota_color(0.1), CRITICAL_COLOR);
        assert_ne!(quota_color(0.325), WARNING_COLOR);
        assert_ne!(quota_color(0.575), CAUTION_COLOR);
        assert!(channel_spread(quota_color(0.1)) > 150);
        assert!(channel_spread(quota_color(0.325)) > 150);
        assert!(channel_spread(quota_color(0.575)) > 150);
    }

    #[test]
    fn renderer_produces_a_transparent_owned_rgba_icon() {
        let rgba = render_rgba(0.5, 0.5);
        assert_eq!(rgba.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert_eq!(&rgba[..4], &[0, 0, 0, 0]);
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn renderer_changes_both_fill_and_remaining_quota_color() {
        let empty = render_rgba(0.0, 1.0);
        let full = render_rgba(1.0, 1.0);
        let depleted = render_rgba(1.0, 0.0);
        let blue_pixels = |rgba: &[u8]| {
            rgba.chunks_exact(4)
                .filter(|pixel| pixel[2] > pixel[0] && pixel[2] > pixel[1] && pixel[3] > 200)
                .count()
        };
        let red_pixels = |rgba: &[u8]| {
            rgba.chunks_exact(4)
                .filter(|pixel| pixel[0] > pixel[1] && pixel[0] > pixel[2] && pixel[3] > 200)
                .count()
        };
        assert!(blue_pixels(&full) > blue_pixels(&empty));
        assert!(red_pixels(&depleted) > 0);
    }
}
