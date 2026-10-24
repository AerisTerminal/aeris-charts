//! Histogram (volume-style column) geometry builder. Port of
//! `src/renderers/histogram-renderer.ts`, including the
//! gap-alignment and min-width equalization passes.

use crate::color::Color;
use crate::draw_list::{IRect, Prim};

const SHOW_SPACING_MINIMAL_BAR_WIDTH: i32 = 1;
const ALIGN_TO_MINIMAL_WIDTH_LIMIT: i32 = 4;

#[derive(Clone, Copy, Debug)]
pub struct HistogramItem {
    pub x: f64,
    pub y: f64,
    /// Time-point index; used to detect adjacency for the gap-alignment pass.
    pub time: i64,
    pub color: Color,
}

#[derive(Clone, Copy, Debug)]
pub struct HistogramParams {
    pub bar_spacing: f64,
    pub horizontal_pixel_ratio: f64,
    pub vertical_pixel_ratio: f64,
    /// Base line in media px (price scale coordinate of `base`).
    pub histogram_base: f64,
    /// Column width as a fraction of the bar spacing. `None` keeps the reference volume layout
    /// (full spacing minus a one-pixel gap); `Some` draws equal odd-width columns centered on
    /// each bar's device pixel, at least one device pixel wide, leaving the rest as gap.
    pub column_width_ratio: Option<f64>,
}

struct ColumnCoords {
    left: i32,
    right: i32,
    rounded_center: i32,
    center: f64,
    time: i64,
}

fn precalculate_columns(items: &[HistogramItem], params: &HistogramParams) -> Vec<ColumnCoords> {
    let pixel_ratio = params.horizontal_pixel_ratio;
    if let Some(ratio) = params.column_width_ratio {
        return ratio_columns(items, params.bar_spacing * pixel_ratio * ratio, pixel_ratio);
    }
    let spacing =
        if (params.bar_spacing * pixel_ratio).ceil() as i32 <= SHOW_SPACING_MINIMAL_BAR_WIDTH {
            0
        } else {
            1i32.max(pixel_ratio.floor() as i32)
        };
    let column_width = (params.bar_spacing * pixel_ratio).round() as i32 - spacing;

    let mut columns: Vec<ColumnCoords> = items
        .iter()
        .map(|item| {
            let x = (item.x * pixel_ratio).round() as i32;
            let (left, right) = if column_width % 2 != 0 {
                let half_width = (column_width - 1) / 2;
                (x - half_width, x + half_width)
            } else {
                // shift one pixel to the left
                let half_width = column_width / 2;
                (x - half_width, x + half_width - 1)
            };
            ColumnCoords {
                left,
                right,
                rounded_center: x,
                center: item.x * pixel_ratio,
                time: item.time,
            }
        })
        .collect();

    // gap alignment: adjacent columns must be exactly `spacing + 1` apart
    for i in 1..columns.len() {
        let (prev_slice, current_slice) = columns.split_at_mut(i);
        let prev = &mut prev_slice[i - 1];
        let current = &mut current_slice[0];
        if current.time != prev.time + 1 {
            continue;
        }
        if current.left - prev.right != spacing + 1 {
            if (prev.rounded_center as f64) > prev.center {
                // prev was shifted left, add a pixel to its right
                prev.right = current.left - spacing - 1;
            } else {
                // extend current to the left
                current.left = prev.right + spacing + 1;
            }
        }
    }

    // min-width equalization
    let mut min_width = (params.bar_spacing * pixel_ratio).ceil() as i32;
    for column in &mut columns {
        // can happen if bar spacing < 1
        if column.right < column.left {
            column.right = column.left;
        }
        min_width = min_width.min(column.right - column.left + 1);
    }

    if spacing > 0 && min_width < ALIGN_TO_MINIMAL_WIDTH_LIMIT {
        for column in &mut columns {
            let width = column.right - column.left + 1;
            if width > min_width {
                if (column.rounded_center as f64) > column.center {
                    column.right -= 1;
                } else {
                    column.left += 1;
                }
            }
        }
    }

    columns
}

/// Equal columns need no gap alignment or width equalization: an odd width centers exactly on
/// the rounded bar pixel, so every gap is identical wherever the bar spacing allows one.
fn ratio_columns(items: &[HistogramItem], width_px: f64, pixel_ratio: f64) -> Vec<ColumnCoords> {
    let mut width = (width_px.round() as i32).max(1);
    if width % 2 == 0 {
        width -= 1;
    }
    let half_width = (width - 1) / 2;
    items
        .iter()
        .map(|item| {
            let x = (item.x * pixel_ratio).round() as i32;
            ColumnCoords {
                left: x - half_width,
                right: x + half_width,
                rounded_center: x,
                center: item.x * pixel_ratio,
                time: item.time,
            }
        })
        .collect()
}

/// The device-pixel row band `(top, height)` the columns share as their base. A base line drawn
/// over this band sits exactly under every column's base edge.
#[must_use]
pub fn histogram_base_band(histogram_base: f64, vertical_pixel_ratio: f64) -> (i32, i32) {
    let tick_width = 1i32.max(vertical_pixel_ratio.floor() as i32);
    let histogram_base = (histogram_base * vertical_pixel_ratio).round() as i32;
    (histogram_base - tick_width / 2, tick_width)
}

pub fn build_histogram(items: &[HistogramItem], params: &HistogramParams, out: &mut Vec<Prim>) {
    if items.is_empty() {
        return;
    }

    let vpr = params.vertical_pixel_ratio;
    let columns = precalculate_columns(items, params);

    let (top_histogram_base, tick_width) = histogram_base_band(params.histogram_base, vpr);
    let bottom_histogram_base = top_histogram_base + tick_width;

    for (item, column) in items.iter().zip(&columns) {
        let y = (item.y * vpr).round() as i32;

        let (top, bottom) = if y <= top_histogram_base {
            (y, bottom_histogram_base)
        } else {
            (top_histogram_base, y - tick_width / 2 + tick_width)
        };

        out.push(Prim::Rect {
            rect: IRect {
                x: column.left,
                y: top,
                w: column.right - column.left + 1,
                h: bottom - top,
            },
            color: item.color,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: Color = Color::rgb(0x26, 0xa6, 0x9a);

    fn item(x: f64, y: f64, time: i64) -> HistogramItem {
        HistogramItem {
            x,
            y,
            time,
            color: C,
        }
    }

    fn params(bar_spacing: f64, dpr: f64, base: f64) -> HistogramParams {
        HistogramParams {
            bar_spacing,
            horizontal_pixel_ratio: dpr,
            vertical_pixel_ratio: dpr,
            histogram_base: base,
            column_width_ratio: None,
        }
    }

    fn rects(prims: &[Prim]) -> Vec<IRect> {
        prims
            .iter()
            .filter_map(|p| match p {
                Prim::Rect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn column_above_base() {
        // spacing 6, dpr 1: spacing px = 1, column width = 5 (odd -> symmetric +-2)
        let mut out = Vec::new();
        build_histogram(&[item(100.0, 30.0, 0)], &params(6.0, 1.0, 90.0), &mut out);
        let r = rects(&out)[0];
        assert_eq!(r.x, 98);
        assert_eq!(r.w, 5);
        // y=30 above base 90: top=30, bottom = base bottom (90 - 0 + 1 = 91)
        assert_eq!(r.y, 30);
        assert_eq!(r.h, 61);
    }

    #[test]
    fn column_below_base_inverted() {
        let mut out = Vec::new();
        build_histogram(&[item(100.0, 120.0, 0)], &params(6.0, 1.0, 90.0), &mut out);
        let r = rects(&out)[0];
        // y=120 below base: top = base top (90), bottom = 120 - 0 + 1 = 121
        assert_eq!(r.y, 90);
        assert_eq!(r.h, 31);
    }

    #[test]
    fn adjacent_columns_have_uniform_gaps() {
        // fractional spacing at dpr 1 forces rounding jitter that the alignment pass fixes
        let spacing = 6.3;
        let items: Vec<HistogramItem> = (0..20)
            .map(|i| item(50.0 + i as f64 * spacing, 30.0, i as i64))
            .collect();
        let mut out = Vec::new();
        build_histogram(&items, &params(spacing, 1.0, 90.0), &mut out);
        let rs = rects(&out);
        for w in rs.windows(2) {
            let gap = w[1].x - (w[0].x + w[0].w);
            assert_eq!(gap, 1, "gap must be exactly spacing px: {:?}", w);
        }
    }

    #[test]
    fn tiny_spacing_no_gaps() {
        // bar spacing 1 at dpr 1 -> ceil(1) <= 1 -> spacing 0, columns touch
        let items: Vec<HistogramItem> = (0..10)
            .map(|i| item(50.0 + i as f64, 30.0, i as i64))
            .collect();
        let mut out = Vec::new();
        build_histogram(&items, &params(1.0, 1.0, 90.0), &mut out);
        let rs = rects(&out);
        for w in rs.windows(2) {
            assert_eq!(w[1].x - (w[0].x + w[0].w), 0, "{:?}", w);
        }
    }

    #[test]
    fn ratio_columns_are_equal_odd_and_centered_with_a_gap() {
        for (spacing, dpr) in [(6.3, 1.0), (8.0, 1.0), (7.0, 2.0), (6.3, 1.5), (13.7, 1.25)] {
            let items: Vec<HistogramItem> = (0..20)
                .map(|i| item(50.0 + i as f64 * spacing, 30.0, i as i64))
                .collect();
            let mut out = Vec::new();
            let params = HistogramParams {
                column_width_ratio: Some(0.5),
                ..params(spacing, dpr, 90.0)
            };
            build_histogram(&items, &params, &mut out);
            let rs = rects(&out);
            let width = rs[0].w;
            assert!(width >= 1 && width % 2 == 1, "{spacing}@{dpr}: {width}");
            assert!(f64::from(width) < spacing * dpr, "{spacing}@{dpr}: {width}");
            for (rect, item) in rs.iter().zip(&items) {
                assert_eq!(rect.w, width);
                let center = (item.x * dpr).round() as i32;
                assert_eq!(rect.x + rect.w / 2, center, "centered on the bar pixel");
            }
            for w in rs.windows(2) {
                assert!(w[1].x - (w[0].x + w[0].w) >= 1, "{spacing}@{dpr}: {:?}", w);
            }
        }
    }

    #[test]
    fn ratio_columns_keep_one_device_pixel_when_dense() {
        let mut out = Vec::new();
        let params = HistogramParams {
            column_width_ratio: Some(0.5),
            ..params(0.5, 1.0, 90.0)
        };
        build_histogram(&[item(10.0, 30.0, 0)], &params, &mut out);
        assert_eq!(rects(&out)[0].w, 1);
    }

    #[test]
    fn base_band_matches_the_column_base_edge() {
        let mut out = Vec::new();
        build_histogram(&[item(100.0, 30.0, 0)], &params(6.0, 2.0, 45.3), &mut out);
        let column = rects(&out)[0];
        let (top, height) = histogram_base_band(45.3, 2.0);
        assert_eq!(height, 2);
        assert_eq!(column.y + column.h, top + height);
    }
}
