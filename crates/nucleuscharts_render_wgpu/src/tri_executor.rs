//! Converts the anti-aliased geometry subset of the Prim IR (`Polyline` / `AreaFill` / `Circle` /
//! `RoundRect`)
//! into triangle-mesh vertices for the wgpu tri pipeline.
//!
//! The crisp-rect subset goes through [`prims_to_instances`](crate::prims_to_instances); this is
//! its companion for tessellated geometry. Together they let both backends consume one shared prim
//! list — wgpu here, the Canvas2D executor in `nucleuscharts_render` (roadmap Phase D2). Previously the live
//! line/area builders tessellated straight to tris, so the Canvas2D fallback had nothing to render.
//!
//! The shared point pool holds **device-space** points (the builders already baked the DPR in), so
//! tessellation runs with identity pixel ratios — byte-identical to the old direct-to-tri path.

use nucleuscharts_render::draw_list::{LineType, Prim};
use nucleuscharts_render::line::{
    build_area_fill, build_disc, build_line_stroke, AreaMesh, LineParams, LinePoint, LineVertex,
    StrokeMesh,
};

use crate::tri_pipeline::TriVertex;

fn tri(v: &LineVertex) -> TriVertex {
    TriVertex {
        pos: [v.x, v.y],
        color: v.color,
    }
}

/// Slice a `[first, first+count)` window of the shared device-space pool into `LinePoint`s.
fn pool_slice(points: &[[f32; 2]], first: u32, count: u32) -> Vec<LinePoint> {
    let (a, b) = (first as usize, (first + count) as usize);
    points
        .get(a..b)
        .unwrap_or(&[])
        .iter()
        .map(|p| LinePoint {
            x: p[0] as f64,
            y: p[1] as f64,
        })
        .collect()
}

/// Identity `LineParams` — the pool already carries the DPR, so tessellation must not re-scale.
fn identity(line_width: f64, line_type: LineType) -> LineParams {
    LineParams {
        horizontal_pixel_ratio: 1.0,
        vertical_pixel_ratio: 1.0,
        line_width,
        line_type,
    }
}

/// Approximate a rounded rectangle with a small, deterministic polygon. RoundRect is currently
/// used by engine-owned square markers; keeping it in the shared Prim adapter means those markers
/// render in WebGPU exactly as they do through the Canvas2D/native paths instead of disappearing.
fn round_rect_polygon(x: f32, y: f32, w: f32, h: f32, radii: [f32; 4]) -> Vec<[f32; 2]> {
    use std::f32::consts::PI;
    let max_radius = (w.abs().min(h.abs()) / 2.0).max(0.0);
    let [lt, rt, rb, lb] = radii.map(|r| r.max(0.0).min(max_radius));
    let mut out = Vec::with_capacity(24);
    out.push([x + lt, y]);
    out.push([x + w - rt, y]);
    append_arc(&mut out, x + w - rt, y + rt, rt, -PI / 2.0, 0.0);
    out.push([x + w, y + h - rb]);
    append_arc(&mut out, x + w - rb, y + h - rb, rb, 0.0, PI / 2.0);
    out.push([x + lb, y + h]);
    append_arc(&mut out, x + lb, y + h - lb, lb, PI / 2.0, PI);
    out.push([x, y + lt]);
    append_arc(&mut out, x + lt, y + lt, lt, PI, 3.0 * PI / 2.0);
    out
}

fn append_arc(out: &mut Vec<[f32; 2]>, cx: f32, cy: f32, radius: f32, start: f32, end: f32) {
    for step in 1..=4 {
        let t = start + (end - start) * (step as f32 / 4.0);
        out.push([cx + radius * t.cos(), cy + radius * t.sin()]);
    }
}

fn fill_polygon(
    poly: &[[f32; 2]],
    color: nucleuscharts_render::color::Color,
    out: &mut Vec<TriVertex>,
) {
    let (Some(&poly_first), Some(&poly_last)) = (poly.first(), poly.last()) else {
        return;
    };
    if poly.len() < 3 {
        return;
    }
    let center = [
        poly.iter().map(|p| p[0]).sum::<f32>() / poly.len() as f32,
        poly.iter().map(|p| p[1]).sum::<f32>() / poly.len() as f32,
    ];
    let col = [
        color.r() as f32 / 255.0,
        color.g() as f32 / 255.0,
        color.b() as f32 / 255.0,
        color.a() as f32 / 255.0,
    ];
    for pair in poly.windows(2) {
        out.extend([
            TriVertex {
                pos: center,
                color: col,
            },
            TriVertex {
                pos: pair[0],
                color: col,
            },
            TriVertex {
                pos: pair[1],
                color: col,
            },
        ]);
    }
    out.extend([
        TriVertex {
            pos: center,
            color: col,
        },
        TriVertex {
            pos: poly_last,
            color: col,
        },
        TriVertex {
            pos: poly_first,
            color: col,
        },
    ]);
}

#[allow(clippy::too_many_arguments)] // geometry parameters map 1:1 onto the RoundRect prim
fn round_rect_to_tris(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radii: [f32; 4],
    fill: nucleuscharts_render::color::Color,
    border_width: f32,
    border: nucleuscharts_render::color::Color,
    out: &mut Vec<TriVertex>,
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let outer = round_rect_polygon(x, y, w, h, radii);
    if border_width > 0.0 {
        fill_polygon(&outer, border, out);
        let inset = border_width.min(w / 2.0).min(h / 2.0);
        let inner_radii = radii.map(|r| (r - inset).max(0.0));
        let inner = round_rect_polygon(
            x + inset,
            y + inset,
            w - inset * 2.0,
            h - inset * 2.0,
            inner_radii,
        );
        fill_polygon(&inner, fill, out);
    } else {
        fill_polygon(&outer, fill, out);
    }
}

/// Tessellate one geometry prim into `out`, appending nothing for rects, text, and unhandled
/// prims (they render elsewhere). Used by the order-preserving group builder in `frame.rs`.
pub fn geom_prim_to_tris(prim: &Prim, points: &[[f32; 2]], out: &mut Vec<TriVertex>) {
    match prim {
        // reference `layout.background` VerticalGradient: two triangles over the pane rect with
        // the stops as per-vertex colors — the same linear ramp the Canvas2D executor
        // paints with `createLinearGradient` (stop-for-stop identical).
        Prim::Background { rect, gradient } => {
            let [x, y, w, h] = *rect;
            if w <= 0.0 || h <= 0.0 {
                return;
            }
            let to_f32 = |c: nucleuscharts_render::color::Color| {
                [
                    c.r() as f32 / 255.0,
                    c.g() as f32 / 255.0,
                    c.b() as f32 / 255.0,
                    c.a() as f32 / 255.0,
                ]
            };
            let top = to_f32(gradient.top);
            let bottom = to_f32(gradient.bottom);
            let (x0, y0, x1, y1) = (x, y, x + w, y + h);
            let v = |pos: [f32; 2], color: [f32; 4]| TriVertex { pos, color };
            out.extend([
                v([x0, y0], top),
                v([x1, y0], top),
                v([x0, y1], bottom),
                v([x1, y0], top),
                v([x1, y1], bottom),
                v([x0, y1], bottom),
            ]);
        }
        Prim::AreaFill {
            first_point,
            point_count,
            base_y,
            line_type,
            gradient,
        } => {
            let pts = pool_slice(points, *first_point, *point_count);
            let mut mesh = AreaMesh::default();
            build_area_fill(
                &pts,
                *base_y as f64,
                gradient.top,
                gradient.bottom,
                &identity(0.0, *line_type),
                &mut mesh,
            );
            out.extend(mesh.vertices.iter().map(tri));
        }
        Prim::BandFill {
            upper_first,
            lower_first,
            point_count,
            fill,
        } => {
            let upper = pool_slice(points, *upper_first, *point_count);
            let lower = pool_slice(points, *lower_first, *point_count);
            let n = upper.len().min(lower.len());
            if n < 2 {
                return;
            }
            let col = [
                fill.r() as f32 / 255.0,
                fill.g() as f32 / 255.0,
                fill.b() as f32 / 255.0,
                fill.a() as f32 / 255.0,
            ];
            // Two triangles per segment quad (upper_i, lower_i, lower_i+1, upper_i+1).
            let v = |p: &nucleuscharts_render::line::LinePoint| TriVertex {
                pos: [p.x as f32, p.y as f32],
                color: col,
            };
            for i in 0..n - 1 {
                out.extend([
                    v(&upper[i]),
                    v(&lower[i]),
                    v(&lower[i + 1]),
                    v(&upper[i]),
                    v(&lower[i + 1]),
                    v(&upper[i + 1]),
                ]);
            }
        }
        Prim::Polyline {
            first_point,
            point_count,
            width,
            line_type,
            color,
            ..
        } => {
            let pts = pool_slice(points, *first_point, *point_count);
            let mut mesh = StrokeMesh::default();
            build_line_stroke(
                &pts,
                *color,
                &identity(*width as f64, *line_type),
                &mut mesh,
            );
            out.extend(mesh.vertices.iter().map(tri));
        }
        Prim::Circle {
            cx,
            cy,
            radius,
            fill: f,
            ..
        } => {
            let mut disc = Vec::new();
            build_disc([*cx, *cy], *radius, *f, &mut disc);
            out.extend(disc.iter().map(tri));
        }
        Prim::Triangle { a, b, c, color } => {
            let col = [
                color.r() as f32 / 255.0,
                color.g() as f32 / 255.0,
                color.b() as f32 / 255.0,
                color.a() as f32 / 255.0,
            ];
            out.extend([
                TriVertex {
                    pos: *a,
                    color: col,
                },
                TriVertex {
                    pos: *b,
                    color: col,
                },
                TriVertex {
                    pos: *c,
                    color: col,
                },
            ]);
        }
        Prim::RoundRect {
            x,
            y,
            w,
            h,
            radii,
            fill,
            border_width,
            border_color,
        } => {
            round_rect_to_tris(
                *x,
                *y,
                *w,
                *h,
                *radii,
                *fill,
                *border_width,
                *border_color,
                out,
            );
        }
        _ => {}
    }
}

/// Tessellate the geometry prims into `fill` (area fills, drawn first/below) and `stroke` (line
/// strokes + filled discs/markers). Rects, text, and unhandled prims are ignored — they render
/// elsewhere. Kept for bucket-style consumers; the frame's ordered builder uses
/// [`geom_prim_to_tris`] per prim instead so paint order matches the Canvas2D executor.
pub fn geom_prims_to_tris(
    prims: &[Prim],
    points: &[[f32; 2]],
    fill: &mut Vec<TriVertex>,
    stroke: &mut Vec<TriVertex>,
) {
    for prim in prims {
        match prim {
            Prim::Background { .. } | Prim::AreaFill { .. } => {
                geom_prim_to_tris(prim, points, fill);
            }
            _ => geom_prim_to_tris(prim, points, stroke),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nucleuscharts_render::color::Color;
    use nucleuscharts_render::draw_list::Gradient;

    #[test]
    fn polyline_tessellates_to_stroke_only() {
        let points = [[0.0f32, 0.0], [10.0, 10.0], [20.0, 0.0]];
        let prims = [Prim::Polyline {
            first_point: 0,
            point_count: 3,
            width: 2.0,
            style: nucleuscharts_render::draw_list::LineStyle::Solid,
            line_type: LineType::Simple,
            color: Color::rgb(0, 0, 0xFF),
        }];
        let (mut fill, mut stroke) = (Vec::new(), Vec::new());
        geom_prims_to_tris(&prims, &points, &mut fill, &mut stroke);
        assert!(fill.is_empty());
        assert!(
            !stroke.is_empty(),
            "two segments + a join tessellate to tris"
        );
    }

    #[test]
    fn area_fill_tessellates_to_fill_only() {
        let points = [[0.0f32, 10.0], [20.0, 4.0]];
        let prims = [Prim::AreaFill {
            first_point: 0,
            point_count: 2,
            base_y: 40.0,
            line_type: LineType::Simple,
            gradient: Gradient {
                top: Color::rgb(0, 0, 0xFF),
                bottom: Color::rgba(0, 0, 0xFF, 0),
            },
        }];
        let (mut fill, mut stroke) = (Vec::new(), Vec::new());
        geom_prims_to_tris(&prims, &points, &mut fill, &mut stroke);
        assert!(stroke.is_empty());
        assert_eq!(fill.len(), 6, "one quad -> two tris -> six vertices");
    }

    #[test]
    fn circle_tessellates_to_stroke() {
        let prims = [Prim::Circle {
            cx: 5.0,
            cy: 5.0,
            radius: 3.0,
            fill: Color::rgb(0xFF, 0, 0),
            stroke_width: 0.0,
            stroke: Color::rgb(0, 0, 0),
        }];
        let (mut fill, mut stroke) = (Vec::new(), Vec::new());
        geom_prims_to_tris(&prims, &[], &mut fill, &mut stroke);
        assert!(fill.is_empty());
        assert!(!stroke.is_empty());
    }

    #[test]
    fn rects_and_text_ignored() {
        use nucleuscharts_render::draw_list::IRect;
        let prims = [
            Prim::Rect {
                rect: IRect {
                    x: 0,
                    y: 0,
                    w: 5,
                    h: 5,
                },
                color: Color::rgb(0, 0, 0),
            },
            Prim::Text {
                x: 0.0,
                y: 0.0,
                text: "t".into(),
                color: Color::rgb(0, 0, 0),
                size: 12.0,
                family: "Test".into(),
                align: nucleuscharts_render::draw_list::TextAlign::Left,
                weight: 400,
                italic: false,
            },
        ];
        let (mut fill, mut stroke) = (Vec::new(), Vec::new());
        geom_prims_to_tris(&prims, &[], &mut fill, &mut stroke);
        assert!(fill.is_empty() && stroke.is_empty());
    }

    #[test]
    fn rounded_rect_tessellates_to_stroke() {
        let prims = [Prim::RoundRect {
            x: 2.0,
            y: 3.0,
            w: 12.0,
            h: 8.0,
            radii: [2.0; 4],
            fill: Color::rgb(0x10, 0x20, 0x30),
            border_width: 0.0,
            border_color: Color::rgb(0, 0, 0),
        }];
        let (mut fill, mut stroke) = (Vec::new(), Vec::new());
        geom_prims_to_tris(&prims, &[], &mut fill, &mut stroke);
        assert!(fill.is_empty());
        assert!(
            stroke.len() >= 3 * 8,
            "rounded marker must produce filled triangles"
        );
    }
}
