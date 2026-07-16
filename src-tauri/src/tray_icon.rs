use std::sync::OnceLock;

use roxmltree::Document;
use svgtypes::{PathParser, PathSegment};
#[cfg(not(target_os = "macos"))]
use tauri::image::Image;
use tiny_skia::{FillRule, Mask, Paint, Path, PathBuilder, Pixmap, Transform};

const SOURCE: &str = include_str!("../../assets/openquota-tray.svg");
const SOURCE_SIZE: f32 = 24.0;
const ICON_SIZE: u32 = 32;
const SEGMENT_COUNT: usize = 6;
const SWEEP_CENTER: f32 = ICON_SIZE as f32 / 2.0;
const SWEEP_RADIUS: f32 = ICON_SIZE as f32 * 1.5;
const SWEEP_START_RADIANS: f32 = std::f32::consts::FRAC_PI_4;
const SWEEP_STEPS: usize = 192;

type Rgba = (u8, u8, u8, u8);

const TRACK_COLOR: Rgba = (142, 142, 147, 150);
const HEALTHY_COLOR: Rgba = (22, 137, 239, 255);
const CAUTION_COLOR: Rgba = (240, 195, 60, 255);
const CRITICAL_COLOR: Rgba = (227, 72, 63, 255);
const CRITICAL_THRESHOLD: f64 = 0.2;
const HEALTHY_THRESHOLD: f64 = 0.6;

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

    let display_fraction = sanitized_fraction(display_fraction);
    let quota_color = quota_color(remaining_fraction);
    draw_paths(&mut pixmap, &paths.track, TRACK_COLOR, transform);
    if display_fraction >= 1.0 {
        draw_paths(&mut pixmap, &paths.fill, quota_color, transform);
    } else if display_fraction > 0.0 {
        let mask = sweep_mask(display_fraction);
        draw_paths_masked(
            &mut pixmap,
            &paths.fill,
            quota_color,
            transform,
            Some(&mask),
        );
    }
    draw_paths(
        &mut pixmap,
        std::slice::from_ref(&paths.q_tail),
        quota_color,
        transform,
    );

    pixmap.take_demultiplied()
}

fn draw_paths(pixmap: &mut Pixmap, paths: &[Path], color: Rgba, transform: Transform) {
    draw_paths_masked(pixmap, paths, color, transform, None);
}

fn draw_paths_masked(
    pixmap: &mut Pixmap,
    paths: &[Path],
    color: Rgba,
    transform: Transform,
    mask: Option<&Mask>,
) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.0, color.1, color.2, color.3);
    paint.anti_alias = true;
    for path in paths {
        pixmap.fill_path(path, &paint, FillRule::Winding, transform, mask);
    }
}

fn sweep_mask(fraction: f64) -> Mask {
    let fraction = sanitized_fraction(fraction) as f32;
    let mut mask = Mask::new(ICON_SIZE, ICON_SIZE).expect("tray mask dimensions are valid");
    if fraction <= 0.0 {
        return mask;
    }
    let sweep = std::f32::consts::TAU * fraction;
    let steps = ((SWEEP_STEPS as f32 * fraction).ceil() as usize).max(1);
    let mut builder = PathBuilder::new();
    builder.move_to(SWEEP_CENTER, SWEEP_CENTER);
    builder.line_to(
        SWEEP_CENTER + SWEEP_RADIUS * SWEEP_START_RADIANS.cos(),
        SWEEP_CENTER + SWEEP_RADIUS * SWEEP_START_RADIANS.sin(),
    );
    for step in 1..=steps {
        let progress = step as f32 / steps as f32;
        let angle = SWEEP_START_RADIANS + sweep * progress;
        builder.line_to(
            SWEEP_CENTER + SWEEP_RADIUS * angle.cos(),
            SWEEP_CENTER + SWEEP_RADIUS * angle.sin(),
        );
    }
    builder.close();
    if let Some(path) = builder.finish() {
        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    }
    mask
}

fn sanitized_fraction(fraction: f64) -> f64 {
    if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn quota_color(remaining_fraction: f64) -> Rgba {
    let remaining = sanitized_fraction(remaining_fraction);
    if remaining >= HEALTHY_THRESHOLD {
        HEALTHY_COLOR
    } else if remaining > CRITICAL_THRESHOLD {
        CAUTION_COLOR
    } else {
        CRITICAL_COLOR
    }
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
        gauge_paths, quota_color, render_rgba, sanitized_fraction, sweep_mask, CAUTION_COLOR,
        CRITICAL_COLOR, HEALTHY_COLOR, ICON_SIZE, SEGMENT_COUNT,
    };

    #[test]
    fn bundled_svg_exposes_the_expected_geometry() {
        let paths = gauge_paths();
        assert_eq!(paths.track.len(), SEGMENT_COUNT);
        assert_eq!(paths.fill.len(), SEGMENT_COUNT);
        assert!(paths.q_tail.bounds().width() > 0.0);
    }

    #[test]
    fn fractions_are_sanitized_without_quantizing_them() {
        assert_eq!(sanitized_fraction(f64::NAN), 0.0);
        assert_eq!(sanitized_fraction(-1.0), 0.0);
        assert_eq!(sanitized_fraction(0.24), 0.24);
        assert_eq!(sanitized_fraction(0.25), 0.25);
        assert_eq!(sanitized_fraction(2.0), 1.0);
    }

    #[test]
    fn angular_mask_advances_continuously_between_nearby_percentages() {
        let mask_weight = |fraction| {
            sweep_mask(fraction)
                .data()
                .iter()
                .map(|alpha| u64::from(*alpha))
                .sum::<u64>()
        };
        let quarter_minus = mask_weight(0.24);
        let quarter = mask_weight(0.25);
        let quarter_plus = mask_weight(0.26);
        assert!(quarter_minus < quarter);
        assert!(quarter < quarter_plus);
    }

    #[test]
    fn remaining_quota_uses_three_status_colors() {
        assert_eq!(quota_color(f64::NAN), CRITICAL_COLOR);
        assert_eq!(quota_color(0.0), CRITICAL_COLOR);
        assert_eq!(quota_color(0.2), CRITICAL_COLOR);
        assert_eq!(quota_color(0.21), CAUTION_COLOR);
        assert_eq!(quota_color(0.59), CAUTION_COLOR);
        assert_eq!(quota_color(0.6), HEALTHY_COLOR);
        assert_eq!(quota_color(1.0), HEALTHY_COLOR);
    }

    #[test]
    fn renderer_produces_a_transparent_owned_rgba_icon() {
        let rgba = render_rgba(0.5, 0.5);
        assert_eq!(rgba.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert_eq!(&rgba[..4], &[0, 0, 0, 0]);
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn renderer_uses_display_fraction_for_fill_and_remaining_fraction_for_color() {
        let empty = render_rgba(0.0, 1.0);
        let quarter = render_rgba(0.25, 1.0);
        let half = render_rgba(0.5, 1.0);
        let full = render_rgba(1.0, 1.0);
        let depleted = render_rgba(1.0, 0.0);
        let alpha_weight = |rgba: &[u8]| {
            rgba.chunks_exact(4)
                .map(|pixel| u64::from(pixel[3]))
                .sum::<u64>()
        };
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
        assert!(alpha_weight(&empty) < alpha_weight(&quarter));
        assert!(alpha_weight(&quarter) < alpha_weight(&half));
        assert!(alpha_weight(&half) < alpha_weight(&full));
        assert!(blue_pixels(&full) > 0);
        assert!(red_pixels(&depleted) > 0);
    }
}
