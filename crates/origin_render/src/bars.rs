//! OHLC bar geometry builder. Port of `src/renderers/bars-renderer.ts`
//! (RENDERING_SPEC.md §3).

use crate::bar_width::optimal_bar_width;
use crate::color::Color;
use crate::draw_list::{IRect, Prim};

#[derive(Clone, Copy, Debug)]
pub struct BarItem {
    pub x: f64,
    pub open_y: f64,
    pub high_y: f64,
    pub low_y: f64,
    pub close_y: f64,
    pub color: Color,
}

#[derive(Clone, Copy, Debug)]
pub struct BarsParams {
    pub bar_spacing: f64,
    pub horizontal_pixel_ratio: f64,
    pub vertical_pixel_ratio: f64,
    pub open_visible: bool,
    pub thin_bars: bool,
}

pub fn build_bars(items: &[BarItem], params: &BarsParams, out: &mut Vec<Prim>) {
    if items.is_empty() {
        return;
    }

    let hpr = params.horizontal_pixel_ratio;
    let vpr = params.vertical_pixel_ratio;

    let mut bar_width = (hpr.floor() as i32).max(optimal_bar_width(params.bar_spacing, hpr) as i32);

    // crosshair parity (same rule as candles, but vs max(1, floor(hpr)))
    if bar_width >= 2 {
        let line_width = 1i32.max(hpr.floor() as i32);
        if (line_width % 2) != (bar_width % 2) {
            bar_width -= 1;
        }
    }

    // if the scale is compressed, the bar could become less than 1 CSS pixel
    let bar_line_width = if params.thin_bars {
        bar_width.min(hpr.floor() as i32)
    } else {
        bar_width
    };

    let draw_open_close = bar_line_width <= bar_width && params.bar_spacing >= (1.5 * hpr).floor();

    for bar in items {
        let body_width_half = (bar_line_width as f64 * 0.5).floor() as i32;

        let body_center = (bar.x * hpr).round() as i32;
        let body_left = body_center - body_width_half;
        let body_width = bar_line_width;
        let body_right = body_left + body_width - 1;

        let high = bar.high_y.min(bar.low_y);
        let low = bar.high_y.max(bar.low_y);

        let body_top = (high * vpr).round() as i32 - body_width_half;
        let body_bottom = (low * vpr).round() as i32 + body_width_half;
        let body_height = (body_bottom - body_top).max(bar_line_width);

        out.push(Prim::Rect {
            rect: IRect {
                x: body_left,
                y: body_top,
                w: body_width,
                h: body_height,
            },
            color: bar.color,
        });

        let side_width = (bar_width as f64 * 1.5).ceil() as i32;

        if draw_open_close {
            if params.open_visible {
                let open_left = body_center - side_width;
                let mut open_top =
                    body_top.max((bar.open_y * vpr).round() as i32 - body_width_half);
                let mut open_bottom = open_top + body_width - 1;
                if open_bottom > body_top + body_height - 1 {
                    open_bottom = body_top + body_height - 1;
                    open_top = open_bottom - body_width + 1;
                }
                out.push(Prim::Rect {
                    rect: IRect {
                        x: open_left,
                        y: open_top,
                        w: body_left - open_left,
                        h: open_bottom - open_top + 1,
                    },
                    color: bar.color,
                });
            }

            let close_right = body_center + side_width;
            let mut close_top = body_top.max((bar.close_y * vpr).round() as i32 - body_width_half);
            let mut close_bottom = close_top + body_width - 1;
            if close_bottom > body_top + body_height - 1 {
                close_bottom = body_top + body_height - 1;
                close_top = close_bottom - body_width + 1;
            }
            out.push(Prim::Rect {
                rect: IRect {
                    x: body_right + 1,
                    y: close_top,
                    w: close_right - body_right,
                    h: close_bottom - close_top + 1,
                },
                color: bar.color,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: Color = Color::rgb(0x26, 0xa6, 0x9a);

    fn bar(x: f64) -> BarItem {
        BarItem {
            x,
            open_y: 50.0,
            high_y: 20.0,
            low_y: 60.0,
            close_y: 30.0,
            color: C,
        }
    }

    fn params(bar_spacing: f64, dpr: f64) -> BarsParams {
        BarsParams {
            bar_spacing,
            horizontal_pixel_ratio: dpr,
            vertical_pixel_ratio: dpr,
            open_visible: true,
            thin_bars: true,
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
    fn bar_with_open_close_ticks() {
        // spacing 10, dpr 1: optimal_bar_width = floor(3) = 3; parity vs 1 -> 3 stays
        // thin: line width = min(3, 1) = 1
        let mut out = Vec::new();
        build_bars(&[bar(100.0)], &params(10.0, 1.0), &mut out);
        let rs = rects(&out);
        assert_eq!(rs.len(), 3); // body + open tick + close tick

        // body: center 100, half 0 -> x=100 w=1; top = 20, bottom = 60, h = 40
        assert_eq!(
            rs[0],
            IRect {
                x: 100,
                y: 20,
                w: 1,
                h: 40
            }
        );

        // side width = ceil(3*1.5) = 5
        // open tick: from 95 to body_left(100) exclusive -> w=5, at open_y=50
        assert_eq!(
            rs[1],
            IRect {
                x: 95,
                y: 50,
                w: 5,
                h: 1
            }
        );
        // close tick: from body_right+1 (101) to 105 -> w=5, at close_y=30
        assert_eq!(
            rs[2],
            IRect {
                x: 101,
                y: 30,
                w: 5,
                h: 1
            }
        );
    }

    #[test]
    fn no_ticks_when_too_compressed() {
        // spacing 1 < floor(1.5*1)=1? 1 >= 1 -> draws; use spacing 0.9 -> 0.9 >= 1 false
        let mut out = Vec::new();
        build_bars(&[bar(100.0)], &params(0.9, 1.0), &mut out);
        assert_eq!(rects(&out).len(), 1); // body only
    }

    #[test]
    fn thick_bars_when_thin_disabled() {
        let mut p = params(10.0, 1.0);
        p.thin_bars = false;
        let mut out = Vec::new();
        build_bars(&[bar(100.0)], &p, &mut out);
        let body = rects(&out)[0];
        // line width = bar width = 3; half = 1
        assert_eq!(body.x, 99);
        assert_eq!(body.w, 3);
        // body extends by half beyond high/low: top 19, bottom 61 -> h 42
        assert_eq!(body.y, 19);
        assert_eq!(body.h, 42);
    }

    #[test]
    fn ticks_clamped_inside_body() {
        // open beyond low: clamped to body bottom
        let item = BarItem {
            x: 100.0,
            open_y: 60.0,
            high_y: 20.0,
            low_y: 60.0,
            close_y: 20.0,
            color: C,
        };
        let mut out = Vec::new();
        build_bars(&[item], &params(10.0, 1.0), &mut out);
        let rs = rects(&out);
        // body covers rows 20..59 (h = bottom - top = 40); open tick clamps to the last row
        assert_eq!(rs[1].y, 59);
        assert_eq!(rs[1].h, 1);
        // close tick at body top
        assert_eq!(rs[2].y, 20);
    }
}
