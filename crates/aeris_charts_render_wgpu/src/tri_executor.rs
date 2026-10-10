//! Converts the anti-aliased geometry subset of the Prim IR (`Polyline` / `AreaFill` / `BandFill` / `Circle` /
//! `RoundRect`)
//! into triangle-mesh vertices for the wgpu tri pipeline.
//!
//! The crisp-rect subset goes through [`prims_to_instances`](crate::prims_to_instances); this is
//! its companion for tessellated geometry. Together they let both backends consume one shared prim
//! list — wgpu here, the Canvas2D executor in `aeris_charts_render` (roadmap Phase D2). Previously the live
//! line/area builders tessellated straight to tris, so the Canvas2D fallback had nothing to render.
//!
//! The shared point pool holds **device-space** points (the builders already baked the DPR in), so
//! tessellation runs with identity pixel ratios — byte-identical to the old direct-to-tri path.

use aeris_charts_render::draw_list::{LineStyle, LineType, Prim, positive_finite_extent};
use aeris_charts_render::line::{
    AreaMesh, LineParams, LinePoint, LineVertex, band_segment_triangles, build_area_fill,
    build_disc, circle_segments, dash_split, expand_band, expand_line, normalized_round_rect_radii,
    round_rect_border, round_rect_polygon, stroke_aa,
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

fn fill_polygon(
    poly: &[[f32; 2]],
    color: aeris_charts_render::color::Color,
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

fn stroke_circle(
    center: [f32; 2],
    radius: f32,
    width: f32,
    color: aeris_charts_render::color::Color,
    out: &mut Vec<TriVertex>,
) {
    if radius <= 0.0 || width <= 0.0 {
        return;
    }
    let outer = radius + width * 0.5;
    let inner = (radius - width * 0.5).max(0.0);
    let segments = circle_segments(outer);
    let rgba = [
        color.r() as f32 / 255.0,
        color.g() as f32 / 255.0,
        color.b() as f32 / 255.0,
        color.a() as f32 / 255.0,
    ];
    let vertex = |radius: f32, angle: f32| TriVertex {
        pos: [
            center[0] + radius * angle.cos(),
            center[1] + radius * angle.sin(),
        ],
        color: rgba,
    };
    for index in 0..segments {
        let a0 = index as f32 / segments as f32 * std::f32::consts::TAU;
        let a1 = (index + 1) as f32 / segments as f32 * std::f32::consts::TAU;
        let (outer0, outer1) = (vertex(outer, a0), vertex(outer, a1));
        let (inner0, inner1) = (vertex(inner, a0), vertex(inner, a1));
        out.extend([outer0, inner0, inner1, outer0, inner1, outer1]);
    }
}

#[allow(clippy::too_many_arguments)] // geometry parameters map 1:1 onto the RoundRect prim
fn round_rect_to_tris(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radii: [f32; 4],
    fill: aeris_charts_render::color::Color,
    border_width: f32,
    border: aeris_charts_render::color::Color,
    out: &mut Vec<TriVertex>,
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let radii = normalized_round_rect_radii(w, h, radii);
    if border_width > 0.0 {
        let geometry = round_rect_border(x, y, w, h, radii, border_width);
        fill_polygon(&geometry.inner, fill, out);
        let rgba = [
            border.r() as f32 / 255.0,
            border.g() as f32 / 255.0,
            border.b() as f32 / 255.0,
            border.a() as f32 / 255.0,
        ];
        out.extend(
            geometry
                .ring
                .into_iter()
                .map(|pos| TriVertex { pos, color: rgba }),
        );
    } else {
        let outer = round_rect_polygon(x, y, w, h, radii);
        fill_polygon(&outer, fill, out);
    }
}

fn rgba(color: aeris_charts_render::color::Color) -> [f32; 4] {
    [
        color.r() as f32 / 255.0,
        color.g() as f32 / 255.0,
        color.b() as f32 / 255.0,
        color.a() as f32 / 255.0,
    ]
}

/// Tessellate one geometry prim into `out`, appending nothing for rects, text, and unhandled
/// prims (they render elsewhere). Used by the order-preserving group builder in `frame.rs`.
pub fn geom_prim_to_tris(prim: &Prim, points: &[[f32; 2]], out: &mut Vec<TriVertex>) {
    match prim {
        Prim::GradientRect { rect, gradient } => {
            if rect.w <= 0 || rect.h <= 0 {
                return;
            }
            let (x0, y0) = (rect.x as f32, rect.y as f32);
            let (x1, y1) = ((rect.x + rect.w) as f32, (rect.y + rect.h) as f32);
            let top = rgba(gradient.top);
            let bottom = rgba(gradient.bottom);
            out.extend([
                TriVertex {
                    pos: [x0, y0],
                    color: top,
                },
                TriVertex {
                    pos: [x1, y0],
                    color: top,
                },
                TriVertex {
                    pos: [x0, y1],
                    color: bottom,
                },
                TriVertex {
                    pos: [x1, y0],
                    color: top,
                },
                TriVertex {
                    pos: [x1, y1],
                    color: bottom,
                },
                TriVertex {
                    pos: [x0, y1],
                    color: bottom,
                },
            ]);
        }
        // reference `layout.background` VerticalGradient: two triangles over the pane rect with
        // the stops as per-vertex colors — the same linear ramp the Canvas2D executor
        // paints with `createLinearGradient` (stop-for-stop identical).
        Prim::Background { rect, gradient } => {
            let [x, y, w, h] = *rect;
            if w <= 0.0 || h <= 0.0 {
                return;
            }
            let to_f32 = |c: aeris_charts_render::color::Color| {
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
            line_type,
            fill,
        } => {
            let upper = pool_slice(points, *upper_first, *point_count);
            let lower = pool_slice(points, *lower_first, *point_count);
            let (upper, lower) = expand_band(&upper, &lower, *line_type);
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
            for i in 0..n - 1 {
                let point = |p: &aeris_charts_render::line::LinePoint| [p.x as f32, p.y as f32];
                out.extend(
                    band_segment_triangles(
                        point(&upper[i]),
                        point(&upper[i + 1]),
                        point(&lower[i]),
                        point(&lower[i + 1]),
                    )
                    .map(|pos| TriVertex { pos, color: col }),
                );
            }
        }
        Prim::BandGradientFill {
            upper_first,
            lower_first,
            point_count,
            line_type,
            gradient,
        } => {
            let upper = pool_slice(points, *upper_first, *point_count);
            let lower = pool_slice(points, *lower_first, *point_count);
            let (upper, lower) = expand_band(&upper, &lower, *line_type);
            let n = upper.len().min(lower.len());
            if n < 2 {
                return;
            }
            let (mut y_top, mut y_bottom) = (f32::INFINITY, f32::NEG_INFINITY);
            for point in upper.iter().chain(&lower) {
                y_top = y_top.min(point.y as f32);
                y_bottom = y_bottom.max(point.y as f32);
            }
            let span = (y_bottom - y_top).max(1.0);
            let rgba = |color: aeris_charts_render::color::Color| {
                [
                    color.r() as f32 / 255.0,
                    color.g() as f32 / 255.0,
                    color.b() as f32 / 255.0,
                    color.a() as f32 / 255.0,
                ]
            };
            let top = rgba(gradient.top);
            let bottom = rgba(gradient.bottom);
            for i in 0..n - 1 {
                let point = |p: &aeris_charts_render::line::LinePoint| [p.x as f32, p.y as f32];
                out.extend(
                    band_segment_triangles(
                        point(&upper[i]),
                        point(&upper[i + 1]),
                        point(&lower[i]),
                        point(&lower[i + 1]),
                    )
                    .map(|pos| {
                        let t = ((pos[1] - y_top) / span).clamp(0.0, 1.0);
                        TriVertex {
                            pos,
                            color: std::array::from_fn(|channel| {
                                top[channel] + (bottom[channel] - top[channel]) * t
                            }),
                        }
                    }),
                );
            }
        }
        Prim::Polyline {
            first_point,
            point_count,
            width,
            line_type,
            style,
            color,
        } => {
            if !positive_finite_extent(*width) {
                return;
            }
            // Shared anti-aliased stroker (the one GPUI uses): per-vertex coverage gives edges a
            // continuous ramp on top of MSAA instead of four coverage levels.
            let pts = expand_line(&pool_slice(points, *first_point, *point_count), *line_type);
            let rgba = [
                color.r() as f32 / 255.0,
                color.g() as f32 / 255.0,
                color.b() as f32 / 255.0,
                color.a() as f32 / 255.0,
            ];
            let mut emit_stroke = |run: &[LinePoint]| {
                stroke_aa(run, *width, |triangle| {
                    out.extend(triangle.map(|vertex| TriVertex {
                        pos: vertex.position,
                        color: [rgba[0], rgba[1], rgba[2], rgba[3] * vertex.coverage()],
                    }));
                })
            };
            if *style == LineStyle::Solid {
                emit_stroke(&pts);
            } else {
                let pattern: Vec<f64> = style
                    .dash_pattern(*width)
                    .into_iter()
                    .map(f64::from)
                    .collect();
                for run in dash_split(&pts, &pattern) {
                    emit_stroke(&run);
                }
            }
        }
        Prim::Circle {
            cx,
            cy,
            radius,
            fill: f,
            stroke_width,
            stroke,
        } => {
            if !positive_finite_extent(*radius) {
                return;
            }
            let mut disc = Vec::new();
            build_disc([*cx, *cy], *radius, *f, &mut disc);
            out.extend(disc.iter().map(tri));
            if positive_finite_extent(*stroke_width) {
                stroke_circle([*cx, *cy], *radius, *stroke_width, *stroke, out);
            }
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
        Prim::GradientRoundRect {
            x,
            y,
            w,
            h,
            radii,
            gradient,
        } => {
            let first = out.len();
            round_rect_to_tris(*x, *y, *w, *h, *radii, gradient.top, 0.0, gradient.top, out);
            let top = rgba(gradient.top);
            let bottom = rgba(gradient.bottom);
            for vertex in &mut out[first..] {
                let t = ((vertex.pos[1] - *y) / *h).clamp(0.0, 1.0);
                vertex.color = std::array::from_fn(|channel| {
                    top[channel] + (bottom[channel] - top[channel]) * t
                });
            }
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
            Prim::Background { .. }
            | Prim::AreaFill { .. }
            | Prim::BandFill { .. }
            | Prim::BandGradientFill { .. } => {
                geom_prim_to_tris(prim, points, fill);
            }
            _ => geom_prim_to_tris(prim, points, stroke),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeris_charts_render::color::Color;
    use aeris_charts_render::draw_list::Gradient;

    #[test]
    fn band_gradient_colors_follow_exact_vertical_extent() {
        let points = [[0.0, 0.0], [10.0, 0.0], [0.0, 10.0], [10.0, 10.0]];
        let mut vertices = Vec::new();
        geom_prim_to_tris(
            &Prim::BandGradientFill {
                upper_first: 0,
                lower_first: 2,
                point_count: 2,
                line_type: LineType::Simple,
                gradient: Gradient {
                    top: Color::rgba(255, 0, 0, 128),
                    bottom: Color::rgba(0, 0, 255, 64),
                },
            },
            &points,
            &mut vertices,
        );
        assert_eq!(vertices.len(), 6);
        for vertex in vertices {
            let expected = if vertex.pos[1] == 0.0 {
                [1.0, 0.0, 0.0, 128.0 / 255.0]
            } else {
                [0.0, 0.0, 1.0, 64.0 / 255.0]
            };
            assert_eq!(vertex.color, expected);
        }
    }

    #[test]
    fn crossed_band_has_exact_nonoverlapping_lobes() {
        let points = [[0.0, 0.0], [10.0, 10.0], [0.0, 5.0], [10.0, 5.0]];
        let mut vertices = Vec::new();
        geom_prim_to_tris(
            &Prim::BandFill {
                upper_first: 0,
                lower_first: 2,
                point_count: 2,
                line_type: LineType::Simple,
                fill: Color::rgb(1, 2, 3),
            },
            &points,
            &mut vertices,
        );
        let coverage = |p: [f32; 2]| {
            vertices
                .as_chunks::<3>()
                .0
                .iter()
                .filter(|tri| {
                    let side = |a: [f32; 2], b: [f32; 2]| {
                        (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
                    };
                    let [a, b, c] = tri.map(|v| v.pos);
                    let values = [side(a, b), side(b, c), side(c, a)];
                    values.iter().all(|v| *v > 1e-5) || values.iter().all(|v| *v < -1e-5)
                })
                .count()
        };
        for (point, expected) in [
            ([3.0, 4.0], 1),
            ([3.0, 6.0], 0),
            ([7.0, 6.0], 1),
            ([7.0, 4.0], 0),
        ] {
            assert_eq!(coverage(point), expected, "at {point:?}");
        }
        for ix in 1..40 {
            for iy in 1..40 {
                let point = [ix as f32 * 0.25, iy as f32 * 0.25];
                if (point[1] - point[0]).abs() < 1e-4 || (point[1] - 5.0).abs() < 1e-4 {
                    continue;
                }
                let expected =
                    usize::from(point[1] > point[0].min(5.0) && point[1] < point[0].max(5.0));
                assert_eq!(coverage(point), expected, "at {point:?}");
            }
        }
    }

    #[test]
    fn degenerate_circles_and_polyline_widths_emit_no_triangles() {
        let points = [[0.0, 0.0], [10.0, 10.0]];
        for radius in [-3.0, 0.0, f32::NAN, f32::INFINITY] {
            let mut vertices = Vec::new();
            geom_prim_to_tris(
                &Prim::Circle {
                    cx: 5.0,
                    cy: 6.0,
                    radius,
                    fill: Color::rgb(1, 2, 3),
                    stroke_width: 2.0,
                    stroke: Color::rgb(4, 5, 6),
                },
                &[],
                &mut vertices,
            );
            assert!(vertices.is_empty(), "radius {radius}");
        }
        for width in [-2.0, 0.0, f32::NAN, f32::INFINITY] {
            let mut vertices = Vec::new();
            geom_prim_to_tris(
                &Prim::Polyline {
                    first_point: 0,
                    point_count: 2,
                    width,
                    style: LineStyle::Solid,
                    line_type: LineType::Simple,
                    color: Color::rgb(1, 2, 3),
                },
                &points,
                &mut vertices,
            );
            assert!(vertices.is_empty(), "width {width}");
        }
    }

    #[test]
    fn polyline_tessellates_to_stroke_only() {
        let points = [[0.0f32, 0.0], [10.0, 10.0], [20.0, 0.0]];
        let prims = [Prim::Polyline {
            first_point: 0,
            point_count: 3,
            width: 2.0,
            style: aeris_charts_render::draw_list::LineStyle::Solid,
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
        // Edges are anti-aliased in the mesh itself: a solid core plus vertices that fade to zero
        // coverage, never a hard-edged quad left to MSAA alone.
        assert!(stroke.iter().any(|vertex| vertex.color[3] == 1.0));
        assert!(stroke.iter().any(|vertex| vertex.color[3] == 0.0));
        assert!(stroke.iter().all(|vertex| vertex.color[2] == 1.0));
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
    fn circle_stroke_is_retained_as_an_annulus() {
        let prim = Prim::Circle {
            cx: 5.0,
            cy: 5.0,
            radius: 3.0,
            fill: Color::rgba(0, 0, 0, 0),
            stroke_width: 1.0,
            stroke: Color::rgb(0xff, 0xff, 0xff),
        };
        let mut vertices = Vec::new();
        geom_prim_to_tris(&prim, &[], &mut vertices);
        assert_eq!(vertices.len(), 24 * 3 + 24 * 6);
        assert!(
            vertices[24 * 3..]
                .iter()
                .all(|vertex| vertex.color == [1.0, 1.0, 1.0, 1.0])
        );
    }

    #[test]
    fn bar_gradients_use_bounded_geometry_and_vertex_stop_colors() {
        use aeris_charts_render::draw_list::{Gradient, IRect};
        let gradient = Gradient {
            top: Color::rgb(255, 0, 0),
            bottom: Color::rgb(0, 0, 255),
        };
        let mut vertices = Vec::new();
        geom_prim_to_tris(
            &Prim::GradientRect {
                rect: IRect {
                    x: 2,
                    y: 10,
                    w: 8,
                    h: 20,
                },
                gradient,
            },
            &[],
            &mut vertices,
        );
        assert_eq!(vertices.len(), 6);
        assert!(
            vertices
                .iter()
                .filter(|vertex| vertex.pos[1] == 10.0)
                .all(|vertex| vertex.color == [1.0, 0.0, 0.0, 1.0])
        );
        assert!(
            vertices
                .iter()
                .filter(|vertex| vertex.pos[1] == 30.0)
                .all(|vertex| vertex.color == [0.0, 0.0, 1.0, 1.0])
        );
        vertices.clear();
        geom_prim_to_tris(
            &Prim::GradientRoundRect {
                x: 2.0,
                y: 10.0,
                w: 8.0,
                h: 20.0,
                radii: [2.0; 4],
                gradient,
            },
            &[],
            &mut vertices,
        );
        assert!(!vertices.is_empty());
        assert!(
            vertices
                .iter()
                .all(|vertex| vertex.pos[1] >= 10.0 && vertex.pos[1] <= 30.0)
        );
        assert!(
            vertices
                .iter()
                .any(|vertex| vertex.color[0] > 0.0 && vertex.color[2] > 0.0)
        );
    }

    #[test]
    fn rects_and_text_ignored() {
        use aeris_charts_render::draw_list::IRect;
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
                align: aeris_charts_render::draw_list::TextAlign::Left,
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

    #[test]
    fn pill_ends_stay_round_at_high_dpr() {
        // A 24 CSS px capsule at DPR 3: every chord midpoint must sit within 0.1 device px of the
        // true arc, otherwise the pill ends read as faceted.
        let (h, radius) = (72.0_f32, 36.0_f32);
        let poly = round_rect_polygon(0.0, 0.0, 300.0, h, [radius; 4]);
        let center = [radius, radius];
        let mut worst = 0.0_f32;
        for pair in poly.windows(2) {
            let on_left_arc = pair.iter().all(|p| {
                p[0] <= radius + 1e-3
                    && ((p[0] - center[0]).hypot(p[1] - center[1]) - radius).abs() < 1e-2
            });
            if on_left_arc {
                let mid = [
                    (pair[0][0] + pair[1][0]) / 2.0,
                    (pair[0][1] + pair[1][1]) / 2.0,
                ];
                worst = worst.max(radius - (mid[0] - center[0]).hypot(mid[1] - center[1]));
            }
        }
        assert!(worst > 0.0 && worst <= 0.1, "chord error {worst} device px");
    }
}
