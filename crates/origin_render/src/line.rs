//! Line / area / baseline geometry builder. Port of `walk-line.ts` + `line-renderer.ts` +
//! `area-renderer-base.ts` (RENDERING_SPEC.md §5).
//!
//! Unlike the integer-rect series, lines are anti-aliased: points stay as floats in bitmap
//! space and are emitted as CPU-tessellated triangles. The stroke is a series of quads (one
//! per segment) plus round-join fans at interior vertices; the area fill is a triangle strip
//! between the polyline and a base level. The backend feathers edges for AA.
//!
//! Line type Simple is implemented; WithSteps/Curved land with the corresponding pane views.

use crate::color::Color;
use crate::draw_list::LineType;

/// A vertex the backend will render: bitmap-space position + straight RGBA color.
/// The stroke pipeline extrudes these with AA; the fill pipeline draws them opaque.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineVertex {
    pub x: f32,
    pub y: f32,
    pub color: [f32; 4],
}

/// One data point in media coordinates (already converted by the views layer).
#[derive(Clone, Copy, Debug)]
pub struct LinePoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct LineParams {
    pub horizontal_pixel_ratio: f64,
    pub vertical_pixel_ratio: f64,
    pub line_width: f64,
    pub line_type: LineType,
}

fn color_to_rgba(c: Color) -> [f32; 4] {
    [
        c.r() as f32 / 255.0,
        c.g() as f32 / 255.0,
        c.b() as f32 / 255.0,
        c.a() as f32 / 255.0,
    ]
}

/// Split a polyline into the solid sub-segments a dash pattern produces (port of the Canvas2D
/// `setLineDash` walk: the pattern starts "on" at the first point and alternates on/off along
/// the path, in the same units as `points`). Each returned run is a maximal "on" sub-polyline of
/// two or more points; gap crossings close the current run. The frame builders emit each run as
/// a solid stroke, so the WebGPU tessellator (which has no dash concept) and the Canvas2D path
/// produce identical dash geometry by construction. `points` are expected already expanded
/// ([`expand_line`]) so dashes follow the rendered path for stepped/curved lines.
pub fn dash_split(points: &[LinePoint], pattern: &[f64]) -> Vec<Vec<LinePoint>> {
    let mut runs: Vec<Vec<LinePoint>> = Vec::new();
    if points.len() < 2 || pattern.is_empty() || pattern.iter().any(|&len| len <= 0.0) {
        return runs;
    }
    let interp = |a: LinePoint, b: LinePoint, t: f64| LinePoint {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
    };
    let near = |a: LinePoint, b: LinePoint| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9;
    let mut element = 0usize;
    let mut element_left = pattern[0];
    let mut on = true;
    let mut run: Vec<LinePoint> = vec![points[0]];
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let seg_len = (b.x - a.x).hypot(b.y - a.y);
        if seg_len < 1e-9 {
            continue;
        }
        let mut t0 = 0.0f64;
        while t0 < seg_len - 1e-9 {
            let step = element_left.min(seg_len - t0);
            let t1 = t0 + step;
            if on {
                let p0 = interp(a, b, t0 / seg_len);
                let p1 = interp(a, b, t1 / seg_len);
                if run.last().is_none_or(|&last| !near(last, p0)) {
                    // A gap ended since the last "on" point: close the run and start a new one.
                    if run.len() >= 2 {
                        runs.push(std::mem::take(&mut run));
                    } else {
                        run.clear();
                    }
                    run.push(p0);
                }
                run.push(p1);
            }
            t0 = t1;
            element_left -= step;
            if element_left <= 1e-9 {
                on = !on;
                element = (element + 1) % pattern.len();
                element_left = pattern[element];
            }
        }
    }
    if run.len() >= 2 {
        runs.push(run);
    }
    runs
}

/// A tessellated stroke: triangle list of extruded segment quads + round joins.
/// The backend applies 1px edge feathering for AA.
#[derive(Default)]
pub struct StrokeMesh {
    pub vertices: Vec<LineVertex>,
}

impl StrokeMesh {
    fn push_tri(&mut self, a: LineVertex, b: LineVertex, c: LineVertex) {
        self.vertices.push(a);
        self.vertices.push(b);
        self.vertices.push(c);
    }

    fn push_quad(
        &mut self,
        p0: [f32; 2],
        p1: [f32; 2],
        p2: [f32; 2],
        p3: [f32; 2],
        color: [f32; 4],
    ) {
        let v = |p: [f32; 2]| LineVertex {
            x: p[0],
            y: p[1],
            color,
        };
        // p0-p1-p2, p0-p2-p3 (winding-agnostic; no culling in the pipeline)
        self.push_tri(v(p0), v(p1), v(p2));
        self.push_tri(v(p0), v(p2), v(p3));
    }

    fn push_round_join(&mut self, center: [f32; 2], radius: f32, color: [f32; 4]) {
        // fan approximation of the joint cap; 8 segments is plenty at typical line widths
        const SEGMENTS: usize = 8;
        let v = |p: [f32; 2]| LineVertex {
            x: p[0],
            y: p[1],
            color,
        };
        let c = v(center);
        for i in 0..SEGMENTS {
            let a0 = (i as f32) / SEGMENTS as f32 * std::f32::consts::TAU;
            let a1 = ((i + 1) as f32) / SEGMENTS as f32 * std::f32::consts::TAU;
            let p0 = [center[0] + radius * a0.cos(), center[1] + radius * a0.sin()];
            let p1 = [center[0] + radius * a1.cos(), center[1] + radius * a1.sin()];
            self.push_tri(c, v(p0), v(p1));
        }
    }
}

/// Number of straight segments a curved interval is tessellated into (RENDERING_SPEC.md §5).
const CURVE_SEGMENTS: usize = 16;

/// Catmull-Rom interpolation of one scalar channel at parameter `t` (0..1).
fn catmull_rom(p0: f64, p1: f64, p2: f64, p3: f64, t: f64) -> f64 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

/// Expand a polyline according to its [`LineType`]: `Simple` is unchanged; `WithSteps` inserts a
/// horizontal-then-vertical corner at each interval (the value holds until the next point, as in
/// the reference charting library); `Curved` tessellates a Catmull-Rom spline through the points.
pub fn expand_line(points: &[LinePoint], line_type: LineType) -> Vec<LinePoint> {
    match line_type {
        LineType::Simple => points.to_vec(),
        LineType::WithSteps => {
            let mut out = Vec::with_capacity(points.len() * 2);
            for (i, p) in points.iter().enumerate() {
                if i > 0 {
                    // step corner: horizontal to this x at the previous y, then drop to this point
                    out.push(LinePoint {
                        x: p.x,
                        y: points[i - 1].y,
                    });
                }
                out.push(*p);
            }
            out
        }
        LineType::Curved => {
            if points.len() < 3 {
                return points.to_vec();
            }
            let n = points.len();
            let mut out = Vec::with_capacity((n - 1) * CURVE_SEGMENTS + 1);
            out.push(points[0]);
            for i in 0..n - 1 {
                let p0 = points[i.saturating_sub(1)];
                let p1 = points[i];
                let p2 = points[i + 1];
                let p3 = points[(i + 2).min(n - 1)];
                for s in 1..=CURVE_SEGMENTS {
                    let t = s as f64 / CURVE_SEGMENTS as f64;
                    out.push(LinePoint {
                        x: catmull_rom(p0.x, p1.x, p2.x, p3.x, t),
                        y: catmull_rom(p0.y, p1.y, p2.y, p3.y, t),
                    });
                }
            }
            out
        }
    }
}

/// Builds a stroke mesh over `points` (single color). `visible_range` is `[from, to)` row
/// offsets. Returns triangles in bitmap space.
pub fn build_line_stroke(
    points: &[LinePoint],
    color: Color,
    params: &LineParams,
    out: &mut StrokeMesh,
) {
    let expanded = expand_line(points, params.line_type);
    let points = &expanded[..];
    if points.len() < 2 {
        // single point: reference draws a short horizontal segment of barWidth; skip until we
        // carry barWidth here (area/line with 1 visible point is a rare edge).
        return;
    }

    let hpr = params.horizontal_pixel_ratio;
    let vpr = params.vertical_pixel_ratio;
    let half = (params.line_width * vpr / 2.0) as f32;
    let rgba = color_to_rgba(color);

    let bp = |p: &LinePoint| [(p.x * hpr) as f32, (p.y * vpr) as f32];

    for i in 0..points.len() - 1 {
        let a = bp(&points[i]);
        let b = bp(&points[i + 1]);

        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-6 {
            continue;
        }
        // normal
        let nx = -dy / len * half;
        let ny = dx / len * half;

        self_push_segment(out, a, b, nx, ny, rgba);

        // round join at the shared interior vertex
        if i > 0 {
            out.push_round_join(a, half, rgba);
        }
    }
}

fn self_push_segment(
    out: &mut StrokeMesh,
    a: [f32; 2],
    b: [f32; 2],
    nx: f32,
    ny: f32,
    rgba: [f32; 4],
) {
    out.push_quad(
        [a[0] + nx, a[1] + ny],
        [b[0] + nx, b[1] + ny],
        [b[0] - nx, b[1] - ny],
        [a[0] - nx, a[1] - ny],
        rgba,
    );
}

/// Area fill mesh: triangle list between the polyline and `base_y` (media px). The backend
/// tints each vertex with a vertical gradient (top color at the line, bottom at base) — here
/// we only emit positions and per-vertex gradient factor via color alpha lerp done by caller.
#[derive(Default)]
pub struct AreaMesh {
    /// Each vertex carries its media-y so the backend can look up the gradient; color is the
    /// resolved top/bottom mix computed here for simplicity (single draw, no gradient uniform).
    pub vertices: Vec<LineVertex>,
}

/// Builds an area fill under `points` down to `base_y` (media px), vertically gradient-shaded
/// from `top_color` (at each point) to `bottom_color` (at `base_y`). Colors are premultiplied
/// per-vertex here so the existing solid triangle path can draw it.
pub fn build_area_fill(
    points: &[LinePoint],
    base_y: f64,
    top_color: Color,
    bottom_color: Color,
    params: &LineParams,
    out: &mut AreaMesh,
) {
    let expanded = expand_line(points, params.line_type);
    let points = &expanded[..];
    if points.len() < 2 {
        return;
    }
    let hpr = params.horizontal_pixel_ratio;
    let vpr = params.vertical_pixel_ratio;
    let base = (base_y * vpr) as f32;

    let top = color_to_rgba(top_color);
    let bottom = color_to_rgba(bottom_color);

    // Gradient factor 0 at the fill's geometric top (top color), 1 at its geometric base
    // (bottom color). A normal fill spans [topmost point, base_y] below the line; an inverted
    // one (reference `invertFilledArea`, or a baseline segment under the base level) spans
    // [base_y, lowest point] above the line. Both directions keep the top stop at the
    // geometrically higher edge so the two backends shade identically.
    let min_y = points.iter().map(|p| p.y).fold(f64::INFINITY, f64::min) * vpr;
    let max_y = points.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max) * vpr;
    let top_coord = min_y.min(base as f64);
    let span = (max_y.max(base as f64) - top_coord).max(1.0);
    let shade = |y: f32| -> [f32; 4] {
        let t = ((y as f64 - top_coord) / span).clamp(0.0, 1.0) as f32;
        [
            top[0] + (bottom[0] - top[0]) * t,
            top[1] + (bottom[1] - top[1]) * t,
            top[2] + (bottom[2] - top[2]) * t,
            top[3] + (bottom[3] - top[3]) * t,
        ]
    };
    let vert = |x: f32, y: f32| LineVertex {
        x,
        y,
        color: shade(y),
    };

    let bp = |p: &LinePoint| [(p.x * hpr) as f32, (p.y * vpr) as f32];

    for i in 0..points.len() - 1 {
        let a = bp(&points[i]);
        let b = bp(&points[i + 1]);
        // quad a -> b -> (b.x, base) -> (a.x, base)
        let a_top = vert(a[0], a[1]);
        let b_top = vert(b[0], b[1]);
        let b_base = vert(b[0], base);
        let a_base = vert(a[0], base);
        out.vertices.push(a_top);
        out.vertices.push(b_top);
        out.vertices.push(b_base);
        out.vertices.push(a_top);
        out.vertices.push(b_base);
        out.vertices.push(a_base);
    }
}

/// Builds a **baseline** series: a line whose portions above `baseline_y` (media px) use
/// `top_line`/`top_fill` and portions below use `bottom_line`/`bottom_fill`, with an area fill to
/// the baseline. Segments crossing the baseline are split at the crossing so the color flips
/// exactly there (port of `baseline-renderer-*.ts`, RENDERING_SPEC.md §5). Smaller y = higher
/// price = "above".
#[allow(clippy::too_many_arguments)]
pub fn build_baseline(
    points: &[LinePoint],
    baseline_y: f64,
    top_line: Color,
    bottom_line: Color,
    top_fill: Color,
    bottom_fill: Color,
    params: &LineParams,
    stroke: &mut StrokeMesh,
    fill: &mut AreaMesh,
) {
    let expanded = expand_line(points, params.line_type);
    let pts = &expanded[..];
    if pts.len() < 2 {
        return;
    }
    let hpr = params.horizontal_pixel_ratio;
    let vpr = params.vertical_pixel_ratio;
    let half = (params.line_width * vpr / 2.0) as f32;
    let base_b = (baseline_y * vpr) as f32;

    // split segment (a,b) at the baseline crossing into 1 or 2 sub-segments in media coords
    let split = |a: LinePoint, b: LinePoint| -> Vec<(LinePoint, LinePoint)> {
        let above_a = a.y < baseline_y;
        let above_b = b.y < baseline_y;
        if above_a == above_b || (b.y - a.y).abs() < 1e-9 {
            vec![(a, b)]
        } else {
            let t = (baseline_y - a.y) / (b.y - a.y);
            let c = LinePoint {
                x: a.x + (b.x - a.x) * t,
                y: baseline_y,
            };
            vec![(a, c), (c, b)]
        }
    };

    for i in 0..pts.len() - 1 {
        for (s0, s1) in split(pts[i], pts[i + 1]) {
            let above = ((s0.y + s1.y) / 2.0) < baseline_y;
            let lc = color_to_rgba(if above { top_line } else { bottom_line });
            let fc = color_to_rgba(if above { top_fill } else { bottom_fill });
            let a = [(s0.x * hpr) as f32, (s0.y * vpr) as f32];
            let b = [(s1.x * hpr) as f32, (s1.y * vpr) as f32];

            // fill between the sub-segment and the baseline
            let av = LineVertex {
                x: a[0],
                y: a[1],
                color: fc,
            };
            let bv = LineVertex {
                x: b[0],
                y: b[1],
                color: fc,
            };
            let ab = LineVertex {
                x: a[0],
                y: base_b,
                color: fc,
            };
            let bb = LineVertex {
                x: b[0],
                y: base_b,
                color: fc,
            };
            fill.vertices.extend([av, bv, bb, av, bb, ab]);

            // stroke the sub-segment
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let len = (dx * dx + dy * dy).sqrt();
            if len >= 1e-6 {
                let nx = -dy / len * half;
                let ny = dx / len * half;
                stroke.push_quad(
                    [a[0] + nx, a[1] + ny],
                    [b[0] + nx, b[1] + ny],
                    [b[0] - nx, b[1] - ny],
                    [a[0] - nx, a[1] - ny],
                    lc,
                );
            }
        }
    }
}

/// Tessellates a filled disc (triangle fan) at `center` with `radius`, all in bitmap px.
/// Used for the crosshair marker on line/area series (RENDERING_SPEC.md §8).
pub fn build_disc(center: [f32; 2], radius: f32, color: Color, out: &mut Vec<LineVertex>) {
    const SEGMENTS: usize = 24;
    let rgba = color_to_rgba(color);
    let v = |x: f32, y: f32| LineVertex { x, y, color: rgba };
    let c = v(center[0], center[1]);
    for i in 0..SEGMENTS {
        let a0 = (i as f32) / SEGMENTS as f32 * std::f32::consts::TAU;
        let a1 = ((i + 1) as f32) / SEGMENTS as f32 * std::f32::consts::TAU;
        out.push(c);
        out.push(v(
            center[0] + radius * a0.cos(),
            center[1] + radius * a0.sin(),
        ));
        out.push(v(
            center[0] + radius * a1.cos(),
            center[1] + radius * a1.sin(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLUE: Color = Color::rgb(0x21, 0x96, 0xf3);

    fn params(dpr: f64, w: f64) -> LineParams {
        LineParams {
            horizontal_pixel_ratio: dpr,
            vertical_pixel_ratio: dpr,
            line_width: w,
            line_type: LineType::Simple,
        }
    }

    #[test]
    fn expand_simple_is_identity() {
        let pts = [LinePoint { x: 0.0, y: 1.0 }, LinePoint { x: 1.0, y: 2.0 }];
        let out = expand_line(&pts, LineType::Simple);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].x, 1.0);
        assert_eq!(out[1].y, 2.0);
    }

    #[test]
    fn expand_steps_inserts_corner_at_previous_y() {
        // two points -> point, step-corner, point = 3 vertices; corner at (x1, y0)
        let pts = [LinePoint { x: 0.0, y: 10.0 }, LinePoint { x: 5.0, y: 20.0 }];
        let out = expand_line(&pts, LineType::WithSteps);
        assert_eq!(out.len(), 3);
        assert_eq!((out[1].x, out[1].y), (5.0, 10.0)); // horizontal then vertical
        assert_eq!((out[2].x, out[2].y), (5.0, 20.0));
    }

    #[test]
    fn expand_curved_densifies_and_passes_through_points() {
        let pts = [
            LinePoint { x: 0.0, y: 0.0 },
            LinePoint { x: 10.0, y: 10.0 },
            LinePoint { x: 20.0, y: 0.0 },
        ];
        let out = expand_line(&pts, LineType::Curved);
        // (n-1)*SEG + 1 vertices
        assert_eq!(out.len(), (3 - 1) * CURVE_SEGMENTS + 1);
        // curve interpolates through the original knots
        assert_eq!((out[0].x, out[0].y), (0.0, 0.0));
        assert_eq!((out[CURVE_SEGMENTS].x, out[CURVE_SEGMENTS].y), (10.0, 10.0));
        assert_eq!(
            (out[2 * CURVE_SEGMENTS].x, out[2 * CURVE_SEGMENTS].y),
            (20.0, 0.0)
        );
    }

    #[test]
    fn baseline_splits_at_crossing() {
        // a below-baseline point to an above-baseline point crosses baseline_y=10 once.
        // Same-side pair => 1 sub-segment (2 tris fill); crossing pair => 2 sub-segments.
        let crossing = [LinePoint { x: 0.0, y: 20.0 }, LinePoint { x: 10.0, y: 0.0 }];
        let same = [LinePoint { x: 0.0, y: 5.0 }, LinePoint { x: 10.0, y: 2.0 }];
        let (tl, bl) = (Color::rgb(0, 200, 0), Color::rgb(200, 0, 0));
        let (tf, bf) = (Color::rgba(0, 200, 0, 40), Color::rgba(200, 0, 0, 40));

        let mut s1 = StrokeMesh::default();
        let mut f1 = AreaMesh::default();
        build_baseline(
            &crossing,
            10.0,
            tl,
            bl,
            tf,
            bf,
            &params(1.0, 2.0),
            &mut s1,
            &mut f1,
        );
        // two sub-segments => 2 stroke quads (12 verts) and 2 fill quads (12 verts)
        assert_eq!(s1.vertices.len(), 12);
        assert_eq!(f1.vertices.len(), 12);

        let mut s2 = StrokeMesh::default();
        let mut f2 = AreaMesh::default();
        build_baseline(
            &same,
            10.0,
            tl,
            bl,
            tf,
            bf,
            &params(1.0, 2.0),
            &mut s2,
            &mut f2,
        );
        // one sub-segment => 1 quad each
        assert_eq!(s2.vertices.len(), 6);
        assert_eq!(f2.vertices.len(), 6);
    }

    #[test]
    fn stroke_emits_two_triangles_per_segment() {
        let pts = [
            LinePoint { x: 0.0, y: 10.0 },
            LinePoint { x: 10.0, y: 10.0 },
        ];
        let mut mesh = StrokeMesh::default();
        build_line_stroke(&pts, BLUE, &params(1.0, 2.0), &mut mesh);
        // one segment, no interior joins -> 6 vertices (2 tris)
        assert_eq!(mesh.vertices.len(), 6);
    }

    #[test]
    fn horizontal_segment_extrudes_vertically() {
        let pts = [
            LinePoint { x: 0.0, y: 10.0 },
            LinePoint { x: 10.0, y: 10.0 },
        ];
        let mut mesh = StrokeMesh::default();
        build_line_stroke(&pts, BLUE, &params(1.0, 4.0), &mut mesh);
        // half width 2; the extruded quad should span y in [8, 12]
        let ys: Vec<f32> = mesh.vertices.iter().map(|v| v.y).collect();
        assert!(ys.iter().cloned().fold(f32::INFINITY, f32::min) == 8.0);
        assert!(ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max) == 12.0);
    }

    #[test]
    fn interior_vertices_get_round_joins() {
        let pts = [
            LinePoint { x: 0.0, y: 0.0 },
            LinePoint { x: 10.0, y: 10.0 },
            LinePoint { x: 20.0, y: 0.0 },
        ];
        let mut mesh = StrokeMesh::default();
        build_line_stroke(&pts, BLUE, &params(1.0, 3.0), &mut mesh);
        // 2 segments (2*6=12) + 1 round join (8 tris = 24 verts) = 36
        assert_eq!(mesh.vertices.len(), 12 + 24);
    }

    #[test]
    fn dpr_scales_positions() {
        let pts = [LinePoint { x: 5.0, y: 5.0 }, LinePoint { x: 15.0, y: 5.0 }];
        let mut mesh = StrokeMesh::default();
        build_line_stroke(&pts, BLUE, &params(2.0, 2.0), &mut mesh);
        // x coords should be scaled by dpr: 10 and 30
        let xs: Vec<f32> = mesh.vertices.iter().map(|v| v.x).collect();
        assert!(xs.contains(&10.0));
        assert!(xs.contains(&30.0));
    }

    #[test]
    fn area_fill_reaches_base() {
        let pts = [
            LinePoint { x: 0.0, y: 10.0 },
            LinePoint { x: 10.0, y: 20.0 },
        ];
        let mut mesh = AreaMesh::default();
        build_area_fill(
            &pts,
            100.0,
            BLUE,
            Color::rgba(0x21, 0x96, 0xf3, 0),
            &params(1.0, 2.0),
            &mut mesh,
        );
        // 6 verts per segment
        assert_eq!(mesh.vertices.len(), 6);
        // some vertex sits at base y = 100
        assert!(mesh.vertices.iter().any(|v| v.y == 100.0));
        // top color opaque, base color transparent (gradient endpoints)
        let at_line = mesh.vertices.iter().find(|v| v.y == 10.0).unwrap();
        assert!(at_line.color[3] > 0.9);
        let at_base = mesh.vertices.iter().find(|v| v.y == 100.0).unwrap();
        assert!(at_base.color[3] < 0.1);
    }

    #[test]
    fn empty_and_single_point_no_geometry() {
        let mut mesh = StrokeMesh::default();
        build_line_stroke(&[], BLUE, &params(1.0, 2.0), &mut mesh);
        build_line_stroke(
            &[LinePoint { x: 0.0, y: 0.0 }],
            BLUE,
            &params(1.0, 2.0),
            &mut mesh,
        );
        assert!(mesh.vertices.is_empty());
    }

    #[test]
    fn disc_is_a_fan_around_center() {
        let mut v = Vec::new();
        build_disc([10.0, 20.0], 4.0, BLUE, &mut v);
        assert_eq!(v.len(), 24 * 3); // 24 fan triangles
                                     // every triangle's first vertex is the center
        for tri in v.chunks(3) {
            assert_eq!([tri[0].x, tri[0].y], [10.0, 20.0]);
        }
        // rim vertices lie ~radius from center
        let rim = &v[1];
        let d = ((rim.x - 10.0).powi(2) + (rim.y - 20.0).powi(2)).sqrt();
        assert!((d - 4.0).abs() < 1e-4);
    }

    #[test]
    fn dash_split_cuts_exact_on_segments() {
        // [2 on, 2 off] over a 10px horizontal line: on-runs [0,2], [4,6], [8,10].
        let pts = [LinePoint { x: 0.0, y: 0.0 }, LinePoint { x: 10.0, y: 0.0 }];
        let runs = dash_split(&pts, &[2.0, 2.0]);
        assert_eq!(runs.len(), 3);
        let spans: Vec<(f64, f64)> = runs
            .iter()
            .map(|run| (run.first().unwrap().x, run.last().unwrap().x))
            .collect();
        assert_eq!(spans, vec![(0.0, 2.0), (4.0, 6.0), (8.0, 10.0)]);
        // every run is a proper sub-polyline
        assert!(runs.iter().all(|run| run.len() >= 2));
    }

    #[test]
    fn dash_split_continues_the_pattern_across_vertices() {
        // L-shaped path: 5px right then 5px down. Pattern [4 on, 4 off]: the first dash
        // covers (0,0)->(4,0); the 4px gap wraps the corner (1px horizontal + 3px vertical),
        // so the second run resumes at (5,3) and continues to (5,5) — the pattern never
        // restarts at a vertex (reference setLineDash semantics).
        let pts = [
            LinePoint { x: 0.0, y: 0.0 },
            LinePoint { x: 5.0, y: 0.0 },
            LinePoint { x: 5.0, y: 5.0 },
        ];
        let runs = dash_split(&pts, &[4.0, 4.0]);
        assert_eq!(runs.len(), 2);
        let r0 = &runs[0];
        assert_eq!(r0.first().unwrap().x, 0.0);
        assert_eq!(r0.first().unwrap().y, 0.0);
        assert!((r0.last().unwrap().x - 4.0).abs() < 1e-9);
        assert_eq!(r0.last().unwrap().y, 0.0);
        let r1 = &runs[1];
        assert!((r1.first().unwrap().x - 5.0).abs() < 1e-9);
        assert!((r1.first().unwrap().y - 3.0).abs() < 1e-9);
        assert!((r1.last().unwrap().y - 5.0).abs() < 1e-9);
    }

    #[test]
    fn dash_split_ends_mid_dash_without_trailing_run() {
        // Path ends inside an "off" element: only the first dash is drawn.
        let pts = [LinePoint { x: 0.0, y: 0.0 }, LinePoint { x: 3.0, y: 0.0 }];
        let runs = dash_split(&pts, &[2.0, 2.0]);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].first().unwrap().x, 0.0);
        assert_eq!(runs[0].last().unwrap().x, 2.0);
    }

    #[test]
    fn dash_split_rejects_degenerate_input() {
        let pts = [LinePoint { x: 0.0, y: 0.0 }, LinePoint { x: 3.0, y: 0.0 }];
        assert!(dash_split(&pts, &[]).is_empty());
        assert!(dash_split(&pts, &[2.0, 0.0]).is_empty());
        assert!(dash_split(&pts[..1], &[2.0, 2.0]).is_empty());
    }

    #[test]
    fn area_fill_shades_inverted_fill_from_base_to_line() {
        // Inverted fill (base above the points): the top color sits at the base edge and the
        // bottom color at the lowest line point — the reverse of the normal direction.
        let pts = [
            LinePoint { x: 0.0, y: 20.0 },
            LinePoint { x: 10.0, y: 30.0 },
        ];
        let mut mesh = AreaMesh::default();
        build_area_fill(
            &pts,
            5.0,
            BLUE,
            Color::rgba(0x21, 0x96, 0xf3, 0),
            &params(1.0, 2.0),
            &mut mesh,
        );
        assert_eq!(mesh.vertices.len(), 6);
        let at_base = mesh.vertices.iter().find(|v| v.y == 5.0).unwrap();
        assert!(at_base.color[3] > 0.9, "base edge keeps the top color");
        let at_lowest = mesh.vertices.iter().find(|v| v.y == 30.0).unwrap();
        assert!(
            at_lowest.color[3] < 0.1,
            "lowest line point fades to the bottom color"
        );
    }
}
