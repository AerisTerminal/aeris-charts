//! Converts draw-list prims into quad instances.
//!
//! Handles the integer-rect subset (Rect, RectFrame, HLine, VLine). Line prims replicate
//! Canvas2D stroke coverage: a line of width `w` centered on integer coordinate `c` covers
//! pixels `c - w/2 ..= c - w/2 + w - 1` (the half-pixel translate in
//! `strokeInPixel`/`drawVerticalLine` makes odd widths symmetric around `c`).
//! Dashed styles are expanded into segment rects with the reference dash patterns
//! (RENDERING_SPEC.md §6), phase starting at the path start like Canvas2D.

use origin_render::color::Color;
use origin_render::draw_list::{IRect, LineStyle, Prim};

use crate::quad_pipeline::QuadInstance;

fn color_to_rgba(color: Color) -> [f32; 4] {
    [
        color.r() as f32 / 255.0,
        color.g() as f32 / 255.0,
        color.b() as f32 / 255.0,
        color.a() as f32 / 255.0,
    ]
}

fn push_rect(out: &mut Vec<QuadInstance>, rect: IRect, color: Color) {
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    out.push(QuadInstance {
        rect: [rect.x as f32, rect.y as f32, rect.w as f32, rect.h as f32],
        color: color_to_rgba(color),
    });
}

/// Port of `fillRectInnerBorder` (`src/helpers/canvas-helpers.ts`).
fn push_rect_frame(out: &mut Vec<QuadInstance>, rect: IRect, border: i32, color: Color) {
    let IRect { x, y, w, h } = rect;
    // horizontal (top and bottom) edges
    push_rect(
        out,
        IRect {
            x: x + border,
            y,
            w: w - border * 2,
            h: border,
        },
        color,
    );
    push_rect(
        out,
        IRect {
            x: x + border,
            y: y + h - border,
            w: w - border * 2,
            h: border,
        },
        color,
    );
    // vertical (left and right) edges
    push_rect(out, IRect { x, y, w: border, h }, color);
    push_rect(
        out,
        IRect {
            x: x + w - border,
            y,
            w: border,
            h,
        },
        color,
    );
}

/// Emits filled dash segments over `[from, to)` using the style's pattern.
fn dash_segments(style: LineStyle, width: i32, from: i32, to: i32, mut emit: impl FnMut(i32, i32)) {
    let pattern = style.dash_pattern(width as f32);
    if pattern.is_empty() {
        emit(from, to);
        return;
    }

    let mut pos = from as f32;
    let mut i = 0usize;
    let mut on = true;
    while pos < to as f32 {
        let seg = pattern[i % pattern.len()];
        if on {
            let a = pos.round() as i32;
            let b = ((pos + seg).min(to as f32)).round() as i32;
            if b > a {
                emit(a, b);
            }
        }
        pos += seg;
        i += 1;
        on = !on;
    }
}

/// Convert one rect-family prim into quad instances. Non-rect geometry is handled by the
/// triangle adapter; text remains on the host overlay rather than in the pane frame.
pub fn prim_to_instances(prim: &Prim, out: &mut Vec<QuadInstance>) {
    match prim {
        Prim::Rect { rect, color } => push_rect(out, *rect, *color),
        Prim::RectFrame {
            rect,
            border,
            color,
        } => push_rect_frame(out, *rect, *border, *color),
        Prim::HLine {
            y,
            x0,
            x1,
            width,
            style,
            color,
        } => {
            let top = y - width / 2;
            dash_segments(*style, *width, *x0, *x1, |a, b| {
                push_rect(
                    out,
                    IRect {
                        x: a,
                        y: top,
                        w: b - a,
                        h: *width,
                    },
                    *color,
                );
            });
        }
        Prim::VLine {
            x,
            y0,
            y1,
            width,
            style,
            color,
        } => {
            let left = x - width / 2;
            dash_segments(*style, *width, *y0, *y1, |a, b| {
                push_rect(
                    out,
                    IRect {
                        x: left,
                        y: a,
                        w: *width,
                        h: b - a,
                    },
                    *color,
                );
            });
        }
        _ => {}
    }
}

pub fn prims_to_instances(prims: &[Prim], out: &mut Vec<QuadInstance>) {
    for prim in prims {
        prim_to_instances(prim, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: Color = Color::rgb(0x10, 0x20, 0x30);

    #[test]
    fn rect_frame_expands_to_four() {
        let mut out = Vec::new();
        push_rect_frame(
            &mut out,
            IRect {
                x: 10,
                y: 20,
                w: 8,
                h: 6,
            },
            1,
            C,
        );
        assert_eq!(out.len(), 4);
        // top edge: (11, 20, 6, 1)
        assert_eq!(out[0].rect, [11.0, 20.0, 6.0, 1.0]);
        // bottom edge: (11, 25, 6, 1)
        assert_eq!(out[1].rect, [11.0, 25.0, 6.0, 1.0]);
        // left/right full-height columns
        assert_eq!(out[2].rect, [10.0, 20.0, 1.0, 6.0]);
        assert_eq!(out[3].rect, [17.0, 20.0, 1.0, 6.0]);
    }

    #[test]
    fn solid_vline_covers_pixel_column() {
        let mut out = Vec::new();
        prims_to_instances(
            &[Prim::VLine {
                x: 100,
                y0: 0,
                y1: 50,
                width: 1,
                style: LineStyle::Solid,
                color: C,
            }],
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].rect, [100.0, 0.0, 1.0, 50.0]);
    }

    #[test]
    fn large_dashed_pattern_6_on_6_off() {
        let mut out = Vec::new();
        prims_to_instances(
            &[Prim::VLine {
                x: 0,
                y0: 0,
                y1: 24,
                width: 1,
                style: LineStyle::Dashed,
                color: C,
            }],
            &mut out,
        );
        // segments [0,6) and [12,18)
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].rect, [0.0, 0.0, 1.0, 6.0]);
        assert_eq!(out[1].rect, [0.0, 12.0, 1.0, 6.0]);
    }

    #[test]
    fn degenerate_rects_dropped() {
        let mut out = Vec::new();
        push_rect(
            &mut out,
            IRect {
                x: 0,
                y: 0,
                w: 0,
                h: 5,
            },
            C,
        );
        push_rect(
            &mut out,
            IRect {
                x: 0,
                y: 0,
                w: 5,
                h: -1,
            },
            C,
        );
        assert!(out.is_empty());
    }
}
