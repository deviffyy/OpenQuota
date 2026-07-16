use tauri::image::Image;
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Transform};

const ICON_SIZE: u32 = 36;
const ICON_POINTS: f32 = 18.0;
const ICON_SCALE: f32 = ICON_SIZE as f32 / ICON_POINTS;
pub const MAX_BARS: usize = 4;

pub fn bar_icon(fractions: &[f64]) -> Image<'static> {
    Image::new_owned(render_bar_rgba(fractions), ICON_SIZE, ICON_SIZE)
}

fn render_bar_rgba(fractions: &[f64]) -> Vec<u8> {
    let size = ICON_POINTS;
    let mut pixmap = Pixmap::new(ICON_SIZE, ICON_SIZE).expect("menu bar icon dimensions are valid");
    let count = fractions.len().min(MAX_BARS);
    if count == 0 {
        return pixmap.take_demultiplied();
    }

    let padding = (size * 0.08).round().max(1.0);
    let gap = (size * 0.03).round().max(1.0);
    let track_x = padding;
    let track_width = size - 2.0 * padding;
    let layout_count = count.max(2) as f32;
    let track_height = ((size - 2.0 * padding - (layout_count - 1.0) * gap) / layout_count)
        .floor()
        .max(1.0);
    let radius = (track_height / 3.0).floor().max(1.0);
    let total_height = count as f32 * track_height + count.saturating_sub(1) as f32 * gap;
    let y_offset = padding + ((size - 2.0 * padding - total_height) / 2.0).floor();

    for (index, fraction) in fractions.iter().take(MAX_BARS).enumerate() {
        let y = y_offset + index as f32 * (track_height + gap) + 1.0;
        fill_rounded_bar(
            &mut pixmap,
            track_x * ICON_SCALE,
            y * ICON_SCALE,
            track_width * ICON_SCALE,
            track_height * ICON_SCALE,
            radius * ICON_SCALE,
            radius * ICON_SCALE,
            41,
        );

        let fill = bar_fill(track_width, *fraction);
        if fill.fill_width > 0.0 {
            let trailing = if fill.fill_width >= track_width {
                radius
            } else {
                (radius * 0.35).floor().max(0.0)
            };
            fill_rounded_bar(
                &mut pixmap,
                track_x * ICON_SCALE,
                y * ICON_SCALE,
                fill.fill_width * ICON_SCALE,
                track_height * ICON_SCALE,
                radius * ICON_SCALE,
                trailing * ICON_SCALE,
                255,
            );
        }
        if let Some(divider_x) = fill.divider_x {
            fill_rounded_bar(
                &mut pixmap,
                (track_x + divider_x) * ICON_SCALE,
                y * ICON_SCALE,
                fill.remainder_width * ICON_SCALE,
                track_height * ICON_SCALE,
                (radius * 0.2).floor().max(0.0) * ICON_SCALE,
                radius * ICON_SCALE,
                61,
            );
        }
    }
    pixmap.take_demultiplied()
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BarFill {
    fill_width: f32,
    remainder_width: f32,
    divider_x: Option<f32>,
}

fn visual_bar_fraction(fraction: f64) -> f64 {
    if !fraction.is_finite() {
        return 0.0;
    }
    let clamped = fraction.clamp(0.0, 1.0);
    if clamped > 0.7 && clamped < 1.0 {
        let remainder = 1.0 - clamped;
        let quantized = ((remainder / 0.15).ceil() * 0.15).min(1.0);
        1.0 - quantized
    } else {
        clamped
    }
}

fn bar_fill(track_width: f32, fraction: f64) -> BarFill {
    if !fraction.is_finite() || fraction <= 0.0 {
        return BarFill {
            fill_width: 0.0,
            remainder_width: 0.0,
            divider_x: None,
        };
    }
    let visual = visual_bar_fraction(fraction);
    if visual >= 1.0 {
        return BarFill {
            fill_width: track_width,
            remainder_width: 0.0,
            divider_x: None,
        };
    }
    let min_visible = 4.0_f32.max((track_width * 0.2).round());
    let max_fill_width = 1.0_f32.max(track_width - min_visible);
    let fill_width = 1.0_f32.max(max_fill_width.min((track_width * visual as f32).round()));
    let true_remainder = track_width - fill_width;
    let remainder_width = (track_width - 1.0).min(true_remainder.max(min_visible));
    BarFill {
        fill_width,
        remainder_width,
        divider_x: Some(track_width - remainder_width),
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_rounded_bar(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    leading_radius: f32,
    trailing_radius: f32,
    alpha: u8,
) {
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let leading = leading_radius.min(height / 2.0).min(width / 2.0);
    let trailing = trailing_radius.min(height / 2.0).min(width / 2.0);
    let mut builder = PathBuilder::new();
    builder.move_to(x + leading, y);
    builder.line_to(x + width - trailing, y);
    builder.quad_to(x + width, y, x + width, y + trailing);
    builder.line_to(x + width, y + height - trailing);
    builder.quad_to(x + width, y + height, x + width - trailing, y + height);
    builder.line_to(x + leading, y + height);
    builder.quad_to(x, y + height, x, y + height - leading);
    builder.line_to(x, y + leading);
    builder.quad_to(x, y, x + leading, y);
    builder.close();
    let Some(path): Option<Path> = builder.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(0, 0, 0, alpha);
    paint.anti_alias = true;
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::{bar_fill, bar_icon, render_bar_rgba, visual_bar_fraction, ICON_SIZE, MAX_BARS};

    #[test]
    fn renderer_preserves_empty_zero_and_full_states() {
        let alpha_pixels = |rgba: &[u8]| rgba.chunks_exact(4).filter(|pixel| pixel[3] > 0).count();
        let empty = render_bar_rgba(&[]);
        let zero = render_bar_rgba(&[0.0]);
        let half = render_bar_rgba(&[0.5]);
        let full = render_bar_rgba(&[1.0]);

        assert_eq!(empty.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert_eq!(alpha_pixels(&empty), 0);
        assert!(alpha_pixels(&zero) > 0);
        assert!(zero.chunks_exact(4).all(|pixel| pixel[3] < 255));

        let visible = zero
            .chunks_exact(4)
            .enumerate()
            .filter(|(_, pixel)| pixel[3] > 0)
            .map(|(index, _)| (index % ICON_SIZE as usize, index / ICON_SIZE as usize))
            .collect::<Vec<_>>();
        let min_x = visible.iter().map(|point| point.0).min().unwrap();
        let max_x = visible.iter().map(|point| point.0).max().unwrap();
        let min_y = visible.iter().map(|point| point.1).min().unwrap();
        let max_y = visible.iter().map(|point| point.1).max().unwrap();
        assert!(max_x - min_x > max_y - min_y);
        assert!(half.chunks_exact(4).any(|pixel| pixel[3] == 255));
        assert!(full.chunks_exact(4).any(|pixel| pixel[3] == 255));
    }

    #[test]
    fn renderer_sanitizes_values_and_caps_the_visible_metric_count() {
        assert_eq!(
            render_bar_rgba(&[f64::NAN, -1.0, 2.0]),
            render_bar_rgba(&[0.0, 0.0, 1.0])
        );
        let fractions = [0.1, 0.3, 0.6, 1.0, 0.8];
        assert_eq!(
            render_bar_rgba(&fractions),
            render_bar_rgba(&fractions[..MAX_BARS])
        );
    }

    #[test]
    fn near_full_bars_keep_a_visible_remainder() {
        assert_eq!(visual_bar_fraction(0.0), 0.0);
        assert_eq!(visual_bar_fraction(0.5), 0.5);
        assert!((visual_bar_fraction(0.97) - 0.85).abs() < 0.0001);
        assert_eq!(visual_bar_fraction(1.0), 1.0);

        let near_full = bar_fill(16.0, 0.97);
        assert_eq!(near_full.fill_width, 12.0);
        assert_eq!(near_full.remainder_width, 4.0);
        assert_eq!(near_full.divider_x, Some(12.0));

        let full = bar_fill(16.0, 1.0);
        assert_eq!(full.fill_width, 16.0);
        assert_eq!(full.remainder_width, 0.0);
        assert_eq!(full.divider_x, None);
    }

    #[test]
    fn icon_uses_a_retina_density_square() {
        let icon = bar_icon(&[0.5]);
        assert_eq!((icon.width(), icon.height()), (36, 36));
    }
}
