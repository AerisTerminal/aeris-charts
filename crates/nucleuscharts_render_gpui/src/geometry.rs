//! Quad and triangle/path conversion for the GPUI executor.
//!
//! Every helper here reuses Nucleus's own pixel math rather than recomputing it:
//! - the integer-rect subset copies the exact expansion the wgpu quad executor and the Canvas2D
//!   executor already agree on (`fillRectInnerBorder`, half-width line centering, dash phase);
//! - the anti-aliased subset calls `nucleuscharts_render::line`'s tessellators
//!   (`build_line_stroke`/`build_area_fill`/`build_disc`), so GPUI draws the same triangles the
//!   WebGPU backend does.
//!
//! Uses Nucleus's coordinate, bar-width, and snapping calculations.

use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::{IRect, LineStyle, LineType};
use nucleuscharts_render::line::{
    build_area_fill, build_disc, build_line_stroke, AreaMesh, LineParams, LinePoint, StrokeMesh,
};

use crate::scene::{DeviceRect, MeshVertex, Paint};

/// The identity `LineParams` the tessellators must run with: the shared point pool already carries
/// the DPR (see `nucleuscharts_render_wgpu::tri_executor`), so re-scaling here would double-apply it.
pub(crate) fn identity_params(line_width: f64, line_type: LineType) -> LineParams {
    LineParams {
        horizontal_pixel_ratio: 1.0,
        vertical_pixel_ratio: 1.0,
        line_width,
        line_type,
    }
}

/// An integer rect as a device rect, or `None` when degenerate.
///
/// Matches `fill_irect` in the Canvas2D executor and `push_rect` in the wgpu quad executor: a
/// non-positive width or height paints nothing (it is *not* clamped to 1 px).
pub(crate) fn irect(rect: IRect) -> Option<DeviceRect> {
    if rect.w <= 0 || rect.h <= 0 {
        return None;
    }
    Some(DeviceRect::new(
        rect.x as f32,
        rect.y as f32,
        rect.w as f32,
        rect.h as f32,
    ))
}

/// The four edge rects of a `RectFrame`, in the same order both existing executors emit them
/// (top, bottom, left, right) — a port of the reference `fillRectInnerBorder`.
pub(crate) fn rect_frame_edges(rect: IRect, border: i32) -> [IRect; 4] {
    let IRect { x, y, w, h } = rect;
    [
        IRect {
            x: x + border,
            y,
            w: w - border * 2,
            h: border,
        },
        IRect {
            x: x + border,
            y: y + h - border,
            w: w - border * 2,
            h: border,
        },
        IRect { x, y, w: border, h },
        IRect {
            x: x + w - border,
            y,
            w: border,
            h,
        },
    ]
}

/// Emit the filled dash spans of `[from, to)` for `style` at `width`.
///
/// Byte-for-byte the same routine as the wgpu and Canvas2D executors, including the `round()` on
/// each span edge and the phase starting at the path start — so dashed grid lines, price lines and
/// crosshairs land on identical pixels in all three backends.
pub(crate) fn dash_spans(
    style: LineStyle,
    width: i32,
    from: i32,
    to: i32,
    mut emit: impl FnMut(i32, i32),
) {
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

/// The top edge of a horizontal line of `width` centered on integer `y` — and, transposed, the
/// left edge of a vertical line. `y - width / 2` with integer division, exactly as both existing
/// executors compute it (the half-pixel translate in the reference's `strokeInPixel` makes odd
/// widths symmetric around `y`).
pub(crate) fn line_span_start(center: i32, width: i32) -> i32 {
    center - width / 2
}

/// Append `verts` as a triangle list, returning `(first_vertex, vertex_count)`.
pub(crate) fn push_vertices(
    pool: &mut Vec<MeshVertex>,
    verts: impl IntoIterator<Item = [f32; 2]>,
) -> (u32, u32) {
    let first = pool.len() as u32;
    pool.extend(verts.into_iter().map(|[x, y]| MeshVertex { x, y }));
    let count = pool.len() as u32 - first;
    // A partial triangle would make GPUI's `push_triangle` loop drop a vertex silently.
    let count = count - count % 3;
    pool.truncate(first as usize + count as usize);
    (first, count)
}

/// Reusable tessellation buffers, owned by the renderer and cleared (never freed) per prim.
///
/// Nucleus's tessellators write into caller-provided `Vec`s. Allocating those fresh per prim made
/// scene construction allocation-bound: a single dense polyline grows a ~1.5 MB `StrokeMesh` from
/// empty every frame, and the doubling reallocations copy roughly twice that. Measured on the
/// standard dense fixture, hoisting these buffers out of the per-frame path is the difference
/// between ~1.0 ms and ~0.6 ms of scene-construction time, with quad emission itself at 0.04 ms.
///
/// Buffers are separate fields rather than one arena so the tessellators can borrow the input and
/// output halves disjointly.
#[derive(Default)]
pub struct Scratch {
    /// The point window sliced out of the frame's shared pool.
    points: Vec<LinePoint>,
    /// Stroke tessellation output.
    stroke: StrokeMesh,
    /// Area tessellation output.
    area: AreaMesh,
    /// `[f32; 2]` staging for polygons, discs, rings and band fills.
    verts: Vec<[f32; 2]>,
    /// Mesh ranges produced by a dashed polyline, read back by the executor.
    pub(crate) ranges: Vec<(u32, u32)>,
}

impl Scratch {
    /// Approximate retained bytes, so a host can assert the scratch stays bounded across a replay.
    pub fn capacity_bytes(&self) -> usize {
        self.points.capacity() * std::mem::size_of::<LinePoint>()
            + self.stroke.vertices.capacity()
                * std::mem::size_of::<nucleuscharts_render::line::LineVertex>()
            + self.area.vertices.capacity()
                * std::mem::size_of::<nucleuscharts_render::line::LineVertex>()
            + self.verts.capacity() * std::mem::size_of::<[f32; 2]>()
            + self.ranges.capacity() * std::mem::size_of::<(u32, u32)>()
    }
}

/// Copy a `[first, first+count)` window of the shared pool into `out`, reusing its allocation.
fn slice_into(out: &mut Vec<LinePoint>, points: &[[f32; 2]], first: u32, count: u32) {
    out.clear();
    let a = first as usize;
    let b = a.saturating_add(count as usize);
    out.extend(points.get(a..b).unwrap_or(&[]).iter().map(|p| LinePoint {
        x: p[0] as f64,
        y: p[1] as f64,
    }));
}

/// Tessellate a solid polyline stroke into `pool`. Returns a zero-length range for a run with
/// fewer than two points, which the caller reports as a dropped prim.
pub(crate) fn polyline_mesh(
    scratch: &mut Scratch,
    pool: &mut Vec<MeshVertex>,
    points: &[[f32; 2]],
    first: u32,
    count: u32,
    width: f32,
    line_type: LineType,
) -> (u32, u32) {
    let Scratch {
        points: window,
        stroke,
        ..
    } = scratch;
    slice_into(window, points, first, count);
    if window.len() < 2 {
        return (pool.len() as u32, 0);
    }
    stroke.vertices.clear();
    // The color is carried by the op's `Paint`; a placeholder keeps the shared tessellator's
    // signature intact without allocating a second vertex format.
    build_line_stroke(
        window,
        Color::rgb(0, 0, 0),
        &identity_params(width as f64, line_type),
        stroke,
    );
    push_vertices(pool, stroke.vertices.iter().map(|v| [v.x, v.y]))
}

/// Tessellate a dashed polyline into one mesh per solid dash run, recording the ranges in
/// [`Scratch::ranges`] for the caller to emit in order.
#[allow(clippy::too_many_arguments)] // the parameters are the prim's fields plus the dash pattern
pub(crate) fn dashed_polyline_meshes(
    scratch: &mut Scratch,
    pool: &mut Vec<MeshVertex>,
    points: &[[f32; 2]],
    first: u32,
    count: u32,
    width: f32,
    line_type: LineType,
    pattern: &[f32],
) {
    scratch.ranges.clear();
    slice_into(&mut scratch.points, points, first, count);
    if scratch.points.len() < 2 {
        return;
    }
    // `expand_line` and `dash_split` both return owned Vecs. Dashed polylines are the rare route
    // (the engine pre-splits the series it owns into solid runs), so they keep the simple form.
    let expanded = nucleuscharts_render::line::expand_line(&scratch.points, line_type);
    let pattern: Vec<f64> = pattern.iter().map(|&len| len as f64).collect();
    for run in nucleuscharts_render::line::dash_split(&expanded, &pattern) {
        scratch.stroke.vertices.clear();
        build_line_stroke(
            &run,
            Color::rgb(0, 0, 0),
            &identity_params(width as f64, LineType::Simple),
            &mut scratch.stroke,
        );
        let range = push_vertices(pool, scratch.stroke.vertices.iter().map(|v| [v.x, v.y]));
        if range.1 >= 3 {
            scratch.ranges.push(range);
        }
    }
}

/// Tessellate an area fill into `pool`, returning the mesh range and the bounds-relative gradient
/// that reproduces `build_area_fill`'s per-vertex shading exactly.
///
/// Nucleus shades each vertex with `t = clamp((y - top) / span, 0, 1)` where
/// `span = max(bottom - top, 1.0)`. When the geometry is at least 1 px tall, `span` equals the
/// mesh's own height and the shading is precisely a bounds-relative 0→1 ramp. When it is shorter,
/// Nucleus's ramp is *wider* than the geometry, so the bottom stop is never reached; we compress it
/// by substituting the color Nucleus would actually have produced at the mesh's bottom edge. Both
/// cases are exact, so no fidelity is traded for the single-`Background` GPUI `Path`.
#[allow(clippy::too_many_arguments)] // the parameters are the prim's own fields
pub(crate) fn area_fill_mesh(
    scratch: &mut Scratch,
    pool: &mut Vec<MeshVertex>,
    points: &[[f32; 2]],
    first: u32,
    count: u32,
    base_y: f32,
    line_type: LineType,
    top: Color,
    bottom: Color,
) -> ((u32, u32), Paint) {
    let Scratch {
        points: window,
        area,
        ..
    } = scratch;
    slice_into(window, points, first, count);
    if window.len() < 2 {
        return ((pool.len() as u32, 0), Paint::VGradient { top, bottom });
    }
    area.vertices.clear();
    build_area_fill(
        window,
        base_y as f64,
        top,
        bottom,
        &identity_params(0.0, line_type),
        area,
    );
    let range = push_vertices(pool, area.vertices.iter().map(|v| [v.x, v.y]));
    let paint = area_gradient(pool, range, top, bottom);
    (range, paint)
}

/// Rescale an area fill's gradient stops onto the mesh's own bounds (see [`area_fill_mesh`]).
fn area_gradient(
    pool: &[MeshVertex],
    (first, count): (u32, u32),
    top: Color,
    bottom: Color,
) -> Paint {
    let verts = pool
        .get(first as usize..(first as usize).saturating_add(count as usize))
        .unwrap_or(&[]);
    if verts.is_empty() {
        return Paint::VGradient { top, bottom };
    }
    let mut y0 = f32::INFINITY;
    let mut y1 = f32::NEG_INFINITY;
    for v in verts {
        y0 = y0.min(v.y);
        y1 = y1.max(v.y);
    }
    let height = (y1 - y0) as f64;
    let span = height.max(1.0);
    // How far along Nucleus's ramp the mesh's bottom edge actually sits.
    let end_t = (height / span).clamp(0.0, 1.0) as f32;
    Paint::VGradient {
        top,
        bottom: lerp_color(top, bottom, end_t),
    }
}

/// Channel-wise linear interpolation, rounding the way `build_area_fill`'s f32 shading does once
/// the GPU quantizes it back to 8 bits.
pub(crate) fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    if t >= 1.0 {
        return b;
    }
    if t <= 0.0 {
        return a;
    }
    let ch = |x: u8, y: u8| {
        (x as f32 + (y as f32 - x as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color::rgba(
        ch(a.r(), b.r()),
        ch(a.g(), b.g()),
        ch(a.b(), b.b()),
        ch(a.a(), b.a()),
    )
}

/// Tessellate a filled disc into `pool` (24 segments, matching `build_disc`).
pub(crate) fn disc_mesh(pool: &mut Vec<MeshVertex>, cx: f32, cy: f32, radius: f32) -> (u32, u32) {
    let mut verts = Vec::new();
    build_disc([cx, cy], radius, Color::rgb(0, 0, 0), &mut verts);
    push_vertices(pool, verts.iter().map(|v| [v.x, v.y]))
}

/// A ring (annulus) between `radius` and `radius - stroke_width`, for `Circle`'s stroke.
///
/// The Canvas2D executor strokes the disc's arc, which covers `[r - w/2, r + w/2]`; the same
/// coverage as an annulus with those radii. 24 segments keeps it aligned with [`disc_mesh`].
pub(crate) fn ring_mesh(
    pool: &mut Vec<MeshVertex>,
    cx: f32,
    cy: f32,
    radius: f32,
    stroke_width: f32,
) -> (u32, u32) {
    const SEGMENTS: usize = 24;
    let outer = radius + stroke_width / 2.0;
    let inner = (radius - stroke_width / 2.0).max(0.0);
    let mut verts = Vec::with_capacity(SEGMENTS * 6);
    for i in 0..SEGMENTS {
        let a0 = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let (c0, s0) = (a0.cos(), a0.sin());
        let (c1, s1) = (a1.cos(), a1.sin());
        let o0 = [cx + outer * c0, cy + outer * s0];
        let o1 = [cx + outer * c1, cy + outer * s1];
        let i0 = [cx + inner * c0, cy + inner * s0];
        let i1 = [cx + inner * c1, cy + inner * s1];
        verts.extend([o0, o1, i1, o0, i1, i0]);
    }
    push_vertices(pool, verts)
}

/// The outline of a rounded rectangle as a closed polygon, matching the wgpu executor's
/// `round_rect_polygon` (4 line segments per corner arc, radii clamped to half the shorter side).
pub(crate) fn round_rect_polygon(x: f32, y: f32, w: f32, h: f32, radii: [f32; 4]) -> Vec<[f32; 2]> {
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

/// Fan-triangulate a closed convex-ish polygon around its centroid, as the wgpu executor's
/// `fill_polygon` does, so rounded corners land on the same triangles.
pub(crate) fn fill_polygon(pool: &mut Vec<MeshVertex>, poly: &[[f32; 2]]) -> (u32, u32) {
    if poly.len() < 3 {
        return (pool.len() as u32, 0);
    }
    let n = poly.len() as f32;
    let center = [
        poly.iter().map(|p| p[0]).sum::<f32>() / n,
        poly.iter().map(|p| p[1]).sum::<f32>() / n,
    ];
    let mut verts = Vec::with_capacity(poly.len() * 3);
    for pair in poly.windows(2) {
        verts.extend([center, pair[0], pair[1]]);
    }
    verts.extend([center, poly[poly.len() - 1], poly[0]]);
    push_vertices(pool, verts)
}

/// A band fill between two polylines over the same x sequence: two triangles per segment, in the
/// same vertex order as the wgpu executor.
///
/// Reads both edges straight out of the frame's shared pool — a band over a dense visible range is
/// as large as a polyline, so it uses the reusable staging buffer rather than allocating.
pub(crate) fn band_fill_mesh(
    scratch: &mut Scratch,
    pool: &mut Vec<MeshVertex>,
    points: &[[f32; 2]],
    upper_first: u32,
    lower_first: u32,
    count: u32,
) -> (u32, u32) {
    let at = |first: u32, i: u32| -> Option<[f32; 2]> {
        points.get(first as usize + i as usize).copied()
    };
    if count < 2 || at(upper_first, count - 1).is_none() || at(lower_first, count - 1).is_none() {
        return (pool.len() as u32, 0);
    }
    scratch.verts.clear();
    for i in 0..count - 1 {
        let (u0, u1) = (at(upper_first, i), at(upper_first, i + 1));
        let (l0, l1) = (at(lower_first, i), at(lower_first, i + 1));
        let (Some(u0), Some(u1), Some(l0), Some(l1)) = (u0, u1, l0, l1) else {
            break;
        };
        scratch.verts.extend([u0, l0, l1, u0, l1, u1]);
    }
    push_vertices(pool, scratch.verts.iter().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degenerate_integer_rects_are_dropped() {
        assert_eq!(
            irect(IRect {
                x: 1,
                y: 2,
                w: 0,
                h: 5
            }),
            None
        );
        assert_eq!(
            irect(IRect {
                x: 1,
                y: 2,
                w: 5,
                h: -1
            }),
            None
        );
        assert_eq!(
            irect(IRect {
                x: 1,
                y: 2,
                w: 3,
                h: 4
            }),
            Some(DeviceRect::new(1.0, 2.0, 3.0, 4.0))
        );
    }

    #[test]
    fn rect_frame_edges_match_the_existing_executors() {
        // Same expectation as `canvas2d::tests::rect_frame_expands_to_four_edges`.
        let edges = rect_frame_edges(
            IRect {
                x: 10,
                y: 20,
                w: 8,
                h: 6,
            },
            1,
        );
        assert_eq!(
            edges[0],
            IRect {
                x: 11,
                y: 20,
                w: 6,
                h: 1
            }
        );
        assert_eq!(
            edges[1],
            IRect {
                x: 11,
                y: 25,
                w: 6,
                h: 1
            }
        );
        assert_eq!(
            edges[2],
            IRect {
                x: 10,
                y: 20,
                w: 1,
                h: 6
            }
        );
        assert_eq!(
            edges[3],
            IRect {
                x: 17,
                y: 20,
                w: 1,
                h: 6
            }
        );
    }

    #[test]
    fn dashed_spans_match_the_existing_executors() {
        // Same expectation as `canvas2d::tests::large_dashed_vline_emits_on_segments_only`.
        let mut spans = Vec::new();
        dash_spans(LineStyle::Dashed, 1, 0, 24, |a, b| spans.push((a, b)));
        assert_eq!(spans, vec![(0, 6), (12, 18)]);

        let mut solid = Vec::new();
        dash_spans(LineStyle::Solid, 1, 3, 9, |a, b| solid.push((a, b)));
        assert_eq!(solid, vec![(3, 9)]);
    }

    #[test]
    fn odd_line_widths_center_on_the_coordinate() {
        assert_eq!(line_span_start(50, 1), 50);
        assert_eq!(line_span_start(50, 2), 49);
        assert_eq!(line_span_start(50, 3), 49);
        assert_eq!(line_span_start(50, 4), 48);
    }

    #[test]
    fn push_vertices_never_leaves_a_partial_triangle() {
        let mut pool = Vec::new();
        let (first, count) = push_vertices(&mut pool, [[0.0, 0.0], [1.0, 1.0]]);
        assert_eq!((first, count), (0, 0));
        assert!(pool.is_empty(), "the partial triangle was rolled back");

        let (first, count) =
            push_vertices(&mut pool, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [2.0, 2.0]]);
        assert_eq!((first, count), (0, 3));
        assert_eq!(pool.len(), 3);
    }

    #[test]
    fn area_gradient_is_a_plain_ramp_when_the_mesh_is_tall_enough() {
        let top = Color::rgba(0x2e, 0xdc, 0x87, 102);
        let bottom = Color::rgba(0x28, 0xdd, 0x64, 0);
        let mut pool = Vec::new();
        let pts = [[0.0f32, 10.0], [20.0, 4.0]];
        let (_, paint) = area_fill_mesh(
            &mut Scratch::default(),
            &mut pool,
            &pts,
            0,
            2,
            40.0,
            LineType::Simple,
            top,
            bottom,
        );
        assert_eq!(paint, Paint::VGradient { top, bottom });
    }

    #[test]
    fn area_gradient_compresses_when_base_floors_its_span() {
        // A fill only 0.5 px tall: Nucleus's `span` floors at 1.0, so its bottom stop is only half
        // reached. The equivalent bounds-relative ramp ends at the midpoint color.
        let top = Color::rgba(0, 0, 0, 0xff);
        let bottom = Color::rgba(0xff, 0xff, 0xff, 0xff);
        let mut pool = Vec::new();
        let pts = [[0.0f32, 10.0], [20.0, 10.0]];
        let (_, paint) = area_fill_mesh(
            &mut Scratch::default(),
            &mut pool,
            &pts,
            0,
            2,
            10.5,
            LineType::Simple,
            top,
            bottom,
        );
        let Paint::VGradient { top: t, bottom: b } = paint else {
            panic!("expected a gradient, got {paint:?}");
        };
        assert_eq!(t, top);
        assert_eq!(b, Color::rgba(0x80, 0x80, 0x80, 0xff));
    }

    #[test]
    fn area_mesh_bounds_match_the_canvas2d_gradient_extent() {
        // `canvas2d::area_extent` spans [min point y, base_y]; the mesh must have the same extent
        // or a bounds-relative gradient would shift.
        let mut pool = Vec::new();
        let pts = [[0.0f32, 10.0], [20.0, 4.0]];
        let ((first, count), _) = area_fill_mesh(
            &mut Scratch::default(),
            &mut pool,
            &pts,
            0,
            2,
            40.0,
            LineType::Simple,
            Color::rgb(0, 0, 0xff),
            Color::rgba(0, 0, 0xff, 0),
        );
        let verts = &pool[first as usize..(first + count) as usize];
        let y0 = verts.iter().map(|v| v.y).fold(f32::INFINITY, f32::min);
        let y1 = verts.iter().map(|v| v.y).fold(f32::NEG_INFINITY, f32::max);
        assert_eq!((y0, y1), (4.0, 40.0));
    }

    #[test]
    fn lerp_color_hits_both_endpoints_exactly() {
        let a = Color::rgba(0, 10, 20, 30);
        let b = Color::rgba(200, 210, 220, 230);
        assert_eq!(lerp_color(a, b, 0.0), a);
        assert_eq!(lerp_color(a, b, 1.0), b);
        assert_eq!(lerp_color(a, b, -5.0), a);
        assert_eq!(lerp_color(a, b, 5.0), b);
    }

    #[test]
    fn band_fill_emits_two_triangles_per_segment() {
        let mut pool = Vec::new();
        // Upper edge at rows 0..4, lower edge at rows 4..8 of one shared pool.
        let mut pts: Vec<[f32; 2]> = (0..4).map(|i| [i as f32, 0.0]).collect();
        pts.extend((0..4).map(|i| [i as f32, 5.0]));
        let (_, count) = band_fill_mesh(&mut Scratch::default(), &mut pool, &pts, 0, 4, 4);
        assert_eq!(count, 3 * 6, "3 segments x 2 triangles x 3 vertices");
    }

    #[test]
    fn band_fill_needs_two_points_per_edge() {
        let mut pool = Vec::new();
        let one = [[0.0f32, 0.0]];
        assert_eq!(
            band_fill_mesh(&mut Scratch::default(), &mut pool, &one, 0, 0, 1).1,
            0
        );
    }

    #[test]
    fn round_rect_radii_clamp_to_half_the_shorter_side() {
        let poly = round_rect_polygon(0.0, 0.0, 10.0, 4.0, [99.0, 99.0, 99.0, 99.0]);
        for p in &poly {
            assert!(p[0] >= -0.01 && p[0] <= 10.01, "x out of bounds: {p:?}");
            assert!(p[1] >= -0.01 && p[1] <= 4.01, "y out of bounds: {p:?}");
        }
    }

    #[test]
    fn ring_mesh_covers_the_stroke_band_only() {
        let mut pool = Vec::new();
        let (first, count) = ring_mesh(&mut pool, 0.0, 0.0, 10.0, 2.0);
        assert_eq!(count, 24 * 6);
        // A Canvas2D `arc` + `stroke` of width 2 at radius 10 covers [9, 11]; nothing inside.
        for v in &pool[first as usize..(first + count) as usize] {
            let r = (v.x * v.x + v.y * v.y).sqrt();
            assert!((8.9..=11.1).contains(&r), "radius {r} outside the band");
        }
        assert!(
            pool[first as usize..(first + count) as usize]
                .iter()
                .all(|v| (v.x * v.x + v.y * v.y).sqrt() > 1.0),
            "the ring must not cover the disc's interior"
        );
    }

    #[test]
    fn out_of_range_windows_are_clamped_not_panics() {
        let pts = [[0.0f32, 1.0], [2.0, 3.0]];
        let mut out = Vec::new();
        slice_into(&mut out, &pts, 0, 2);
        assert_eq!(out.len(), 2);
        slice_into(&mut out, &pts, 5, 2);
        assert!(out.is_empty());
        slice_into(&mut out, &pts, 0, u32::MAX);
        assert!(out.is_empty());
    }

    #[test]
    fn a_polyline_window_out_of_range_yields_an_empty_mesh() {
        let mut pool = Vec::new();
        let pts = [[0.0f32, 0.0], [1.0, 1.0]];
        let (_, count) = polyline_mesh(
            &mut Scratch::default(),
            &mut pool,
            &pts,
            9,
            5,
            2.0,
            LineType::Simple,
        );
        assert_eq!(count, 0);
    }

    #[test]
    fn scratch_buffers_are_reused_across_calls_not_regrown() {
        // The point of `Scratch`: after the first frame the capacity is already there.
        let mut scratch = Scratch::default();
        let mut pool = Vec::new();
        let pts: Vec<[f32; 2]> = (0..512).map(|i| [i as f32, (i % 7) as f32]).collect();
        polyline_mesh(&mut scratch, &mut pool, &pts, 0, 512, 2.0, LineType::Simple);
        let after_first = scratch.capacity_bytes();
        assert!(after_first > 0);
        for _ in 0..20 {
            pool.clear();
            polyline_mesh(&mut scratch, &mut pool, &pts, 0, 512, 2.0, LineType::Simple);
        }
        assert_eq!(
            scratch.capacity_bytes(),
            after_first,
            "repeated identical frames must not grow the scratch"
        );
    }
}
