//! Candlestick geometry builder. Port of `src/renderers/candlesticks-renderer.ts`
//! Produces integer bitmap-space rects; a backend turns them into
//! instanced quads. Draw order: wicks -> borders -> bodies.

use crate::bar_width::{apply_crosshair_parity, optimal_candlestick_width};
use crate::color::Color;
use crate::draw_list::{IRect, Prim};

/// One candle, already converted to media-space coordinates by the views layer.
#[derive(Clone, Copy, Debug)]
pub struct CandleItem {
    /// Bar center x in media px.
    pub x: f64,
    pub open_y: f64,
    pub high_y: f64,
    pub low_y: f64,
    pub close_y: f64,
    pub body_color: Color,
    pub border_color: Color,
    pub wick_color: Color,
}

#[derive(Clone, Copy, Debug)]
pub struct CandlesParams {
    pub bar_spacing: f64,
    pub horizontal_pixel_ratio: f64,
    pub vertical_pixel_ratio: f64,
    pub wick_visible: bool,
    pub border_visible: bool,
}

/// Port of `_calculateBorderWidth`.
fn calculate_border_width(bar_width: i32, pixel_ratio: f64) -> i32 {
    const BAR_BORDER_WIDTH: f64 = 1.0;

    let mut border_width = (BAR_BORDER_WIDTH * pixel_ratio).floor() as i32;
    if bar_width <= 2 * border_width {
        border_width = ((bar_width as f64 - 1.0) * 0.5).floor() as i32;
    }
    let res = (pixel_ratio.floor() as i32).max(border_width);
    if bar_width <= res * 2 {
        // do not draw bodies, restore source value
        return (pixel_ratio.floor() as i32).max((BAR_BORDER_WIDTH * pixel_ratio).floor() as i32);
    }
    res
}

/// Builds the draw-list prims for the visible candles. `items` must already be sliced to the
/// visible range.
pub fn build_candles(items: &[CandleItem], params: &CandlesParams, out: &mut Vec<Prim>) {
    if items.is_empty() {
        return;
    }

    let hpr = params.horizontal_pixel_ratio;
    let vpr = params.vertical_pixel_ratio;

    let mut bar_width = optimal_candlestick_width(params.bar_spacing, hpr);
    bar_width = apply_crosshair_parity(bar_width, hpr);

    if params.wick_visible {
        draw_wicks(items, params, bar_width, out);
    }

    if params.border_visible {
        draw_borders(items, params, bar_width, out);
    }

    let border_width = calculate_border_width(bar_width, hpr);
    if !params.border_visible || bar_width > border_width * 2 {
        draw_bodies(items, params, bar_width, border_width, out);
    }

    let _ = vpr;
}

fn draw_wicks(items: &[CandleItem], params: &CandlesParams, bar_width: i32, out: &mut Vec<Prim>) {
    let hpr = params.horizontal_pixel_ratio;
    let vpr = params.vertical_pixel_ratio;

    let mut wick_width = (hpr.floor()).min((params.bar_spacing * hpr).floor()) as i32;
    wick_width = (hpr.floor() as i32).max(wick_width.min(bar_width));
    // Sub-1 DPR guard: floor(hpr) is 0 there, which would collapse the wick one column LEFT
    // of the bar center (right = left - 1, the prev-edge clamp then pins it there) while the
    // crosshair paints the center column — keep the wick on the center column like the
    // crosshair's own `max(1, floor)` width rule.
    wick_width = wick_width.max(1);
    let wick_offset = (wick_width as f64 * 0.5).floor() as i32;

    let mut prev_edge: Option<i32> = None;

    for bar in items {
        let top = (bar.open_y.min(bar.close_y) * vpr).round() as i32;
        let bottom = (bar.open_y.max(bar.close_y) * vpr).round() as i32;

        let high = (bar.high_y * vpr).round() as i32;
        let low = (bar.low_y * vpr).round() as i32;

        let scaled_x = (hpr * bar.x).round() as i32;

        let mut left = scaled_x - wick_offset;
        let right = left + wick_width - 1;
        if let Some(prev) = prev_edge {
            left = (prev + 1).max(left).min(right);
        }
        let width = right - left + 1;

        // upper wick: high -> body top; lower wick: body bottom + 1 -> low
        out.push(Prim::Rect {
            rect: IRect {
                x: left,
                y: high,
                w: width,
                h: top - high,
            },
            color: bar.wick_color,
        });
        out.push(Prim::Rect {
            rect: IRect {
                x: left,
                y: bottom + 1,
                w: width,
                h: low - bottom,
            },
            color: bar.wick_color,
        });

        prev_edge = Some(right);
    }
}

fn draw_borders(items: &[CandleItem], params: &CandlesParams, bar_width: i32, out: &mut Vec<Prim>) {
    let hpr = params.horizontal_pixel_ratio;
    let vpr = params.vertical_pixel_ratio;

    let border_width = calculate_border_width(bar_width, hpr);
    let mut prev_edge: Option<i32> = None;

    for bar in items {
        let mut left = (bar.x * hpr).round() as i32 - (bar_width as f64 * 0.5).floor() as i32;
        // important: compute right before patching left
        let right = left + bar_width - 1;

        let top = (bar.open_y.min(bar.close_y) * vpr).round() as i32;
        let bottom = (bar.open_y.max(bar.close_y) * vpr).round() as i32;

        if let Some(prev) = prev_edge {
            left = (prev + 1).max(left).min(right);
        }

        if params.bar_spacing * hpr > (2 * border_width) as f64 {
            out.push(Prim::RectFrame {
                rect: IRect {
                    x: left,
                    y: top,
                    w: right - left + 1,
                    h: bottom - top + 1,
                },
                border: border_width,
                color: bar.border_color,
            });
        } else {
            out.push(Prim::Rect {
                rect: IRect {
                    x: left,
                    y: top,
                    w: right - left + 1,
                    h: bottom - top + 1,
                },
                color: bar.border_color,
            });
        }
        prev_edge = Some(right);
    }
}

fn draw_bodies(
    items: &[CandleItem],
    params: &CandlesParams,
    bar_width: i32,
    border_width: i32,
    out: &mut Vec<Prim>,
) {
    let hpr = params.horizontal_pixel_ratio;
    let vpr = params.vertical_pixel_ratio;

    for bar in items {
        let mut top = (bar.open_y.min(bar.close_y) * vpr).round() as i32;
        let mut bottom = (bar.open_y.max(bar.close_y) * vpr).round() as i32;

        let mut left = (bar.x * hpr).round() as i32 - (bar_width as f64 * 0.5).floor() as i32;
        let mut right = left + bar_width - 1;

        if params.border_visible {
            left += border_width;
            top += border_width;
            right -= border_width;
            bottom -= border_width;
        }

        if top > bottom {
            continue;
        }

        out.push(Prim::Rect {
            rect: IRect {
                x: left,
                y: top,
                w: right - left + 1,
                h: bottom - top + 1,
            },
            color: bar.body_color,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UP: Color = Color::rgb(0x26, 0xa6, 0x9a);
    const BORDER: Color = Color::rgb(0x37, 0x86, 0x58);
    const WICK: Color = Color::rgb(0x73, 0x73, 0x75);

    fn candle(x: f64, open_y: f64, high_y: f64, low_y: f64, close_y: f64) -> CandleItem {
        CandleItem {
            x,
            open_y,
            high_y,
            low_y,
            close_y,
            body_color: UP,
            border_color: BORDER,
            wick_color: WICK,
        }
    }

    fn params(bar_spacing: f64, dpr: f64) -> CandlesParams {
        CandlesParams {
            bar_spacing,
            horizontal_pixel_ratio: dpr,
            vertical_pixel_ratio: dpr,
            wick_visible: true,
            border_visible: true,
        }
    }

    fn rects(prims: &[Prim]) -> Vec<IRect> {
        prims
            .iter()
            .filter_map(|p| match p {
                Prim::Rect { rect, .. } => Some(*rect),
                Prim::RectFrame { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn single_candle_dpr1_spacing6() {
        // barSpacing 6, dpr 1 -> optimal width 5, parity(1) keeps 5
        // candle at x=100, open_y=50, close_y=30, high_y=20, low_y=60
        let items = [candle(100.0, 50.0, 20.0, 60.0, 30.0)];
        let mut out = Vec::new();
        build_candles(&items, &params(6.0, 1.0), &mut out);

        // wick: width 1 centered at 100 -> left=100, right=100
        // upper: y=20, h=top-high=10; lower: y=bottom+1=51, h=low-bottom=10
        assert_eq!(
            out[0],
            Prim::Rect {
                rect: IRect {
                    x: 100,
                    y: 20,
                    w: 1,
                    h: 10
                },
                color: WICK
            }
        );
        assert_eq!(
            out[1],
            Prim::Rect {
                rect: IRect {
                    x: 100,
                    y: 51,
                    w: 1,
                    h: 10
                },
                color: WICK
            }
        );

        // border: left = 100 - floor(2.5) = 98, right = 102; spacing*hpr=6 > 2*1 -> frame
        assert_eq!(
            out[2],
            Prim::RectFrame {
                rect: IRect {
                    x: 98,
                    y: 30,
                    w: 5,
                    h: 21
                },
                border: 1,
                color: BORDER
            }
        );

        // body: inset by border 1 -> 99..101, 31..49
        assert_eq!(
            out[3],
            Prim::Rect {
                rect: IRect {
                    x: 99,
                    y: 31,
                    w: 3,
                    h: 19
                },
                color: UP
            }
        );
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn adjacent_wicks_do_not_overlap() {
        // dpr 2: wick width = 2, offset 1; centers 1.5 media px apart -> 3 bitmap px
        let items = [
            candle(100.0, 50.0, 20.0, 60.0, 30.0),
            candle(101.5, 50.0, 20.0, 60.0, 30.0),
        ];
        let p = CandlesParams {
            wick_visible: true,
            border_visible: false,
            ..params(1.5, 2.0)
        };
        let mut out = Vec::new();
        build_candles(&items, &p, &mut out);

        let rs = rects(&out);
        // first wick: x = 200 - 1 = 199..200; second raw left = 203-1 = 202, prev edge 200 -> stays
        assert_eq!(rs[0].x, 199);
        assert_eq!(rs[0].w, 2);
        assert_eq!(rs[2].x, 202);
        // no overlap between wick columns
        assert!(rs[2].x > rs[0].x + rs[0].w - 1);
    }

    #[test]
    fn tight_candles_clamp_via_prev_edge() {
        // centers only 1 media px apart at dpr 1 with wick width 1:
        // second wick raw left = prev edge -> clamped to prev_edge + 1 = its own right
        let items = [
            candle(100.0, 50.0, 20.0, 60.0, 30.0),
            candle(100.5, 50.0, 20.0, 60.0, 30.0), // rounds to 101? -> raw left 101
        ];
        let p = CandlesParams {
            wick_visible: true,
            border_visible: false,
            ..params(0.5, 1.0)
        };
        let mut out = Vec::new();
        build_candles(&items, &p, &mut out);
        let rs = rects(&out);
        assert!(
            rs[2].x >= rs[0].x + rs[0].w,
            "wicks must not overlap: {:?}",
            rs
        );
    }

    #[test]
    fn bodies_skipped_when_too_thin() {
        // barSpacing 1 -> bar width 1; border visible and barWidth <= 2*borderWidth -> no body
        let items = [candle(100.0, 50.0, 20.0, 60.0, 30.0)];
        let mut out = Vec::new();
        build_candles(&items, &params(1.0, 1.0), &mut out);
        // wick x2 + border rect (solid because spacing*hpr=1 <= 2) and no body
        assert_eq!(out.len(), 3);
        assert!(matches!(out[2], Prim::Rect { color: BORDER, .. }));
    }

    #[test]
    fn doji_body_has_min_height_one() {
        // open == close -> top == bottom -> body height 1 (frame h = 1)
        let items = [candle(100.0, 40.0, 20.0, 60.0, 40.0)];
        let p = CandlesParams {
            border_visible: false,
            ..params(6.0, 1.0)
        };
        let mut out = Vec::new();
        build_candles(&items, &p, &mut out);
        let body = rects(&out)[2];
        assert_eq!(body.h, 1);
        assert_eq!(body.y, 40);
    }

    #[test]
    fn sub_one_dpr_wick_stays_on_the_center_column() {
        // dpr 0.75: floor(hpr) = 0 would collapse the wick one column left of the bar center;
        // the clamp keeps a 1px wick exactly on the center column (crosshair parity).
        let items = [
            candle(100.0, 50.0, 20.0, 60.0, 30.0),
            candle(106.0, 50.0, 20.0, 60.0, 30.0),
        ];
        let p = CandlesParams {
            wick_visible: true,
            border_visible: false,
            ..params(6.0, 0.75)
        };
        let mut out = Vec::new();
        build_candles(&items, &p, &mut out);
        let rs = rects(&out);
        // scaled centers: round(100*0.75) = 75, round(106*0.75) = 80
        assert_eq!((rs[0].x, rs[0].w), (75, 1));
        assert_eq!((rs[2].x, rs[2].w), (80, 1));
    }

    #[test]
    fn dpr2_scales_everything() {
        let items = [candle(100.0, 50.0, 20.0, 60.0, 30.0)];
        let mut out = Vec::new();
        build_candles(&items, &params(6.0, 2.0), &mut out);
        // optimal width dpr2: floor(6*0.85903*2)=10; parity: wick=floor(2)=2 even, 10 even -> 10
        // border: left = 200 - 5 = 195, right = 204
        let border = rects(&out)[2];
        assert_eq!(border.x, 195);
        assert_eq!(border.w, 10);
        assert_eq!(border.y, 60); // 30 * 2
    }
}
