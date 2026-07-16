use std::sync::OnceLock;

use roxmltree::Document;
use svgtypes::{PathParser, PathSegment};
use tauri::image::Image;
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Transform};

use crate::pacing::PaceSeverity;

const SOURCE: &str = include_str!("../../assets/openquota-tray.svg");
const SOURCE_SIZE: f32 = 24.0;
const ICON_SIZE: u32 = 32;
const SEGMENT_COUNT: usize = 6;

const TRACK_COLOR: (u8, u8, u8, u8) = (142, 142, 147, 150);
const HEALTHY_COLOR: (u8, u8, u8, u8) = (22, 137, 239, 255);
const WARNING_COLOR: (u8, u8, u8, u8) = (229, 164, 0, 255);
const CRITICAL_COLOR: (u8, u8, u8, u8) = (227, 72, 63, 255);

struct GaugePaths {
    track: Vec<Path>,
    fill: Vec<Path>,
    needle: Path,
}

pub(crate) fn render_gauge(fraction: f64, severity: PaceSeverity) -> Image<'static> {
    Image::new_owned(render_rgba(fraction, severity), ICON_SIZE, ICON_SIZE)
}

fn render_rgba(fraction: f64, severity: PaceSeverity) -> Vec<u8> {
    let paths = gauge_paths();
    let mut pixmap = Pixmap::new(ICON_SIZE, ICON_SIZE).expect("tray icon dimensions are valid");
    let transform = Transform::from_scale(
        ICON_SIZE as f32 / SOURCE_SIZE,
        ICON_SIZE as f32 / SOURCE_SIZE,
    );

    draw_paths(&mut pixmap, &paths.track, TRACK_COLOR, transform);
    draw_paths(
        &mut pixmap,
        &paths.fill[..visible_segments(fraction)],
        severity_color(severity),
        transform,
    );
    draw_paths(
        &mut pixmap,
        std::slice::from_ref(&paths.needle),
        severity_color(severity),
        transform,
    );

    pixmap.take_demultiplied()
}

fn draw_paths(pixmap: &mut Pixmap, paths: &[Path], color: (u8, u8, u8, u8), transform: Transform) {
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

fn severity_color(severity: PaceSeverity) -> (u8, u8, u8, u8) {
    match severity {
        PaceSeverity::Untracked | PaceSeverity::Healthy => HEALTHY_COLOR,
        PaceSeverity::Close => WARNING_COLOR,
        PaceSeverity::RunningOut | PaceSeverity::Spent => CRITICAL_COLOR,
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
    let needle = parse_named_path(&document, "needle", None)?;
    Ok(GaugePaths {
        track,
        fill,
        needle,
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
        gauge_paths, render_rgba, severity_color, visible_segments, CRITICAL_COLOR, HEALTHY_COLOR,
        ICON_SIZE, SEGMENT_COUNT, WARNING_COLOR,
    };
    use crate::pacing::PaceSeverity;

    #[test]
    fn bundled_svg_exposes_the_expected_geometry() {
        let paths = gauge_paths();
        assert_eq!(paths.track.len(), SEGMENT_COUNT);
        assert_eq!(paths.fill.len(), SEGMENT_COUNT);
        assert!(paths.needle.bounds().width() > 0.0);
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
    fn pacing_severity_uses_the_dashboard_meter_palette() {
        assert_eq!(severity_color(PaceSeverity::Untracked), HEALTHY_COLOR);
        assert_eq!(severity_color(PaceSeverity::Healthy), HEALTHY_COLOR);
        assert_eq!(severity_color(PaceSeverity::Close), WARNING_COLOR);
        assert_eq!(severity_color(PaceSeverity::RunningOut), CRITICAL_COLOR);
        assert_eq!(severity_color(PaceSeverity::Spent), CRITICAL_COLOR);
    }

    #[test]
    fn renderer_produces_a_transparent_owned_rgba_icon() {
        let rgba = render_rgba(0.5, PaceSeverity::Healthy);
        assert_eq!(rgba.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert_eq!(&rgba[..4], &[0, 0, 0, 0]);
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn renderer_changes_both_fill_and_pacing_color() {
        let empty = render_rgba(0.0, PaceSeverity::Healthy);
        let full = render_rgba(1.0, PaceSeverity::Healthy);
        let critical = render_rgba(1.0, PaceSeverity::RunningOut);
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
        assert!(red_pixels(&critical) > 0);
    }
}
