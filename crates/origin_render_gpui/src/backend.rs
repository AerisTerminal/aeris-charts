//! The GPUI binding: walks a [`ScenePlan`] and issues the corresponding `gpui::Window` calls.
//!
//! Compiled only with the `gpui-backend` feature. Everything interesting already happened in
//! [`crate::executor`]; this module is deliberately a thin, order-preserving translation so there
//! is exactly one place where GPUI types appear.
//!
//! # Coordinate conversion
//!
//! `Window::paint_quad` and `Window::paint_path` multiply their inputs by the window's scale factor
//! before inserting into the scene. Origin's IR is already in device px, so every coordinate is
//! divided by the scale factor here and offset by the element's origin. The round trip
//! (`device / sf * sf`) is accurate to ~1e-5 px at chart scale — orders of magnitude below one
//! pixel of coverage — and [`crate::GpuiChartRenderer::plan_frame`] refuses to paint a frame whose
//! `pixel_ratio` disagrees with the window, so the two never drift apart.
//!
//! # Why no `paint_layer`
//!
//! GPUI derives a primitive's draw order from a bounds tree: a primitive that overlaps existing
//! ones gets a strictly higher order, and `Scene::finish` sorts by it. That preserves insertion
//! order wherever it is observable. `Window::paint_layer`, by contrast, forces every primitive
//! inside it to share one order — which would flatten the chart's z-order. Clipping therefore goes
//! through `with_content_mask`, which affects masking only.

use std::collections::HashMap;

use gpui::{
    fill, linear_color_stop, linear_gradient, point, px, size, App, Background, Bounds,
    ContentMask, Font, FontStyle, FontWeight, Hsla, Path, Pixels, Rgba, ShapedLine, SharedString,
    Window,
};

use origin_render::color::Color;

use crate::metrics::GpuiFrameMetrics;
use crate::scene::{DeviceRect, Paint, SceneOp, ScenePlan, TextRun};
use crate::text::{self, TextKey, TextMetrics};
use crate::{GpuiChartRenderer, GpuiRenderError, OriginViewport, PreparedOriginFrame};

/// The gradient angle for a top-to-bottom ramp.
///
/// GPUI documents `linear_gradient`'s angle as CSS `linear-gradient`'s: 0 points to the top and
/// values increase clockwise, so 180° is "to bottom" — the direction every vertical gradient in the
/// `Prim` IR runs (`Gradient { top, bottom }`).
const TO_BOTTOM_DEGREES: f32 = 180.0;

/// Origin's 8-bit sRGB color as a GPUI `Hsla`.
///
/// GPUI stores colors as `Hsla`, so this round-trip is unavoidable. It is lossless in practice:
/// `Rgba -> Hsla -> Rgba` is exact in real arithmetic and carries ~1e-7 relative error in f32,
/// which cannot move an 8-bit channel.
pub fn to_hsla(color: Color) -> Hsla {
    Hsla::from(Rgba {
        r: color.r() as f32 / 255.0,
        g: color.g() as f32 / 255.0,
        b: color.b() as f32 / 255.0,
        a: color.a() as f32 / 255.0,
    })
}

/// A [`Paint`] as a GPUI `Background`.
pub fn to_background(paint: Paint) -> Background {
    match paint {
        Paint::Solid(c) => to_hsla(c).into(),
        Paint::VGradient { top, bottom } => linear_gradient(
            TO_BOTTOM_DEGREES,
            linear_color_stop(to_hsla(top), 0.0),
            linear_color_stop(to_hsla(bottom), 1.0),
        ),
    }
}

/// Maps device px to the window's logical px.
#[derive(Clone, Copy, Debug)]
struct Transform {
    origin_x: f32,
    origin_y: f32,
    inv_scale: f32,
}

impl Transform {
    fn new(viewport: OriginViewport, scale_factor: f32) -> Self {
        Self {
            origin_x: viewport.origin_x,
            origin_y: viewport.origin_y,
            inv_scale: 1.0 / scale_factor,
        }
    }

    fn x(&self, device_x: f32) -> Pixels {
        px(self.origin_x + device_x * self.inv_scale)
    }

    fn y(&self, device_y: f32) -> Pixels {
        px(self.origin_y + device_y * self.inv_scale)
    }

    fn len(&self, device_len: f32) -> Pixels {
        px(device_len * self.inv_scale)
    }

    fn point(&self, device_x: f32, device_y: f32) -> gpui::Point<Pixels> {
        point(self.x(device_x), self.y(device_y))
    }

    fn bounds(&self, rect: DeviceRect) -> Bounds<Pixels> {
        Bounds {
            origin: self.point(rect.x, rect.y),
            size: size(self.len(rect.w), self.len(rect.h)),
        }
    }
}

/// A GPUI `Font` for one text run.
///
/// The `Prim` IR carries a CSS family *list*; GPUI's `Font::family` takes a single name with
/// separate `fallbacks`, so the list is split on commas — the first entry becomes the family and
/// the rest become fallbacks, which is what the CSS list means.
pub fn to_font(run: &TextRun) -> Font {
    let mut parts = run
        .family
        .split(',')
        .map(|p| p.trim().trim_matches(['"', '\''].as_ref()))
        .filter(|p| !p.is_empty());
    let family = parts.next().unwrap_or(".SystemUIFont").to_string();
    let fallbacks: Vec<String> = parts.map(|p| p.to_string()).collect();
    Font {
        family: family.into(),
        features: Default::default(),
        fallbacks: (!fallbacks.is_empty()).then(|| gpui::FontFallbacks::from_fonts(fallbacks)),
        weight: FontWeight(run.weight as f32),
        style: if run.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
    }
}

/// Logical-pixel metrics from GPUI's native text shaper.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuiTextMetrics {
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
}

/// Measure one text run with the same GPUI font resolution and shaping used by chart painting.
///
/// Hosts use this during prepaint for price-axis width negotiation and tick density. The returned
/// values are logical pixels; frame construction applies DPR exactly once afterward.
pub fn measure_text(
    window: &Window,
    text: &str,
    family: &str,
    size: f32,
    weight: u16,
    italic: bool,
) -> GpuiTextMetrics {
    let run = TextRun {
        x: 0.0,
        y: 0.0,
        text: text.to_owned(),
        color: Color::rgb(0, 0, 0),
        size,
        family: family.to_owned(),
        align: origin_render::draw_list::TextAlign::Left,
        weight,
        italic,
    };
    let font = to_font(&run);
    let text_system = window.text_system().clone();
    let font_size = px(size);
    let font_id = text_system.resolve_font(&font);
    let ascent: f32 = text_system.ascent(font_id, font_size).into();
    let descent: f32 = text_system.descent(font_id, font_size).into();
    let gpui_run = gpui::TextRun {
        len: text.len(),
        font,
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = text_system.shape_line(
        SharedString::from(text.to_owned()),
        font_size,
        std::slice::from_ref(&gpui_run),
        None,
    );
    GpuiTextMetrics {
        width: line.width.into(),
        ascent,
        descent,
    }
}

/// Everything GPUI's shaping result depends on.
///
/// Position and alignment are intentionally absent: they are applied when the cached line is
/// painted, and GPUI chooses the glyph raster's subpixel variant from that paint origin. Keeping
/// them out lets moving crosshair labels reuse shaping without reusing wrongly positioned pixels.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ShapedTextKey {
    text: String,
    family: String,
    logical_size_bits: u32,
    color: Color,
    weight: u16,
    italic: bool,
}

impl ShapedTextKey {
    fn for_run(run: &TextRun, logical_size: Pixels) -> Self {
        let logical_size: f32 = logical_size.into();
        Self {
            text: run.text.clone(),
            family: run.family.clone(),
            logical_size_bits: logical_size.to_bits(),
            color: run.color,
            weight: run.weight,
            italic: run.italic,
        }
    }
}

#[derive(Clone, Debug)]
struct CachedShapedText {
    line: ShapedLine,
    /// Logical-pixel font metrics used to place the shaped line.
    ascent: f32,
    descent: f32,
}

/// Bounded LRU of GPUI-native shaped lines.
///
/// This stays in the feature-gated backend so the default crate remains GPUI-free. `ShapedLine`
/// shares its immutable layout through an `Arc`, making a cache hit a cheap clone and, crucially,
/// avoiding `WindowTextSystem::shape_line` entirely.
pub(crate) struct ShapedTextCache {
    entries: HashMap<ShapedTextKey, (CachedShapedText, u64)>,
    capacity: usize,
    tick: u64,
    hits: u32,
    misses: u32,
}

impl Default for ShapedTextCache {
    fn default() -> Self {
        Self::with_capacity(text::TEXT_CACHE_CAPACITY)
    }
}

impl ShapedTextCache {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity: capacity.max(1),
            tick: 0,
            hits: 0,
            misses: 0,
        }
    }

    fn get_or_shape(
        &mut self,
        key: ShapedTextKey,
        shape: impl FnOnce() -> CachedShapedText,
    ) -> CachedShapedText {
        self.tick += 1;
        if let Some((line, stamp)) = self.entries.get_mut(&key) {
            *stamp = self.tick;
            self.hits += 1;
            return line.clone();
        }

        self.misses += 1;
        if self.entries.len() >= self.capacity {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, (_, stamp))| *stamp)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                self.entries.remove(&oldest);
            }
        }

        let line = shape();
        self.entries.insert(key, (line.clone(), self.tick));
        line
    }

    pub(crate) fn invalidate(&mut self) {
        self.entries.clear();
    }

    fn reset_counters(&mut self) {
        self.hits = 0;
        self.misses = 0;
    }

    fn counters(&self) -> (u32, u32) {
        (self.hits, self.misses)
    }
}

impl GpuiChartRenderer {
    /// Lower a prepared frame and paint it into `window`.
    ///
    /// Call from a GPUI element's `paint` phase. The frame must already be built (see
    /// [`PreparedOriginFrame`]): this method only reads it, so no model, layout, or indicator
    /// recomputation happens during paint, and no engine lock is held while the scene is emitted.
    ///
    /// Returns [`GpuiRenderError::ScaleFactorMismatch`] if the frame was built for a different DPR
    /// than the window reports, rather than silently painting a mis-scaled chart.
    pub fn paint_frame(
        &mut self,
        prepared: &PreparedOriginFrame<'_>,
        viewport: OriginViewport,
        scale_factor: f32,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<GpuiFrameMetrics, GpuiRenderError> {
        let mut metrics = self.plan_frame(prepared, scale_factor)?;
        let started = std::time::Instant::now();
        self.shaped_text.reset_counters();
        // Move the plan out so the borrow checker allows both caches alongside it; the allocation
        // returns to `self` before the method ends, so nothing is reallocated.
        let plan = std::mem::take(&mut self.plan);
        paint_plan(
            &plan,
            &mut self.text,
            &mut self.shaped_text,
            viewport,
            scale_factor,
            window,
            cx,
            &mut metrics,
        );
        self.plan = plan;
        let (hits, misses) = self.shaped_text.counters();
        metrics.text_cache_hits = hits;
        metrics.text_cache_misses = misses;
        metrics.paint_nanos = started.elapsed().as_nanos() as u64;
        Ok(metrics)
    }
}

impl GpuiChartRenderer {
    /// Paint a bare `Prim` layer into `window`, with no frame or clip around it.
    ///
    /// The counterpart of [`GpuiChartRenderer::plan_prims`]: used by the pixel-parity harness, which
    /// must put the *identical* prim list through GPUI and through `origin_native` to attribute any
    /// residual. Hosts should use [`GpuiChartRenderer::paint_frame`].
    pub fn paint_prims(
        &mut self,
        prims: &[origin_render::draw_list::Prim],
        points: &[[f32; 2]],
        viewport: OriginViewport,
        scale_factor: f32,
        window: &mut Window,
        cx: &mut App,
    ) -> GpuiFrameMetrics {
        let mut metrics = self.plan_prims(prims, points);
        let started = std::time::Instant::now();
        self.shaped_text.reset_counters();
        let plan = std::mem::take(&mut self.plan);
        paint_plan(
            &plan,
            &mut self.text,
            &mut self.shaped_text,
            viewport,
            scale_factor,
            window,
            cx,
            &mut metrics,
        );
        self.plan = plan;
        let (hits, misses) = self.shaped_text.counters();
        metrics.text_cache_hits = hits;
        metrics.text_cache_misses = misses;
        metrics.paint_nanos = started.elapsed().as_nanos() as u64;
        metrics
    }
}

/// Walk a plan and issue its GPUI calls, in order.
///
/// Clip ops are handled by recursing into the masked range rather than by mutating a stack:
/// `Window::with_content_mask` is a scoped call, so the nesting has to be expressed as nesting.
#[allow(clippy::too_many_arguments)] // one context bundle per GPUI paint call; a wrapper struct
                                     // would only move these borrows behind another name
fn paint_plan(
    plan: &ScenePlan,
    text_cache: &mut crate::text::TextCache,
    shaped_text: &mut ShapedTextCache,
    viewport: OriginViewport,
    scale_factor: f32,
    window: &mut Window,
    cx: &mut App,
    metrics: &mut GpuiFrameMetrics,
) {
    let transform = Transform::new(viewport, scale_factor);
    paint_range(
        plan,
        0,
        plan.ops.len(),
        transform,
        text_cache,
        shaped_text,
        window,
        cx,
        metrics,
    );
}

#[allow(clippy::too_many_arguments)] // one context bundle per GPUI paint call; splitting it would
                                     // only move the arguments behind a struct with no gain
fn paint_range(
    plan: &ScenePlan,
    start: usize,
    end: usize,
    transform: Transform,
    text_cache: &mut crate::text::TextCache,
    shaped_text: &mut ShapedTextCache,
    window: &mut Window,
    cx: &mut App,
    metrics: &mut GpuiFrameMetrics,
) {
    let mut i = start;
    while i < end {
        match &plan.ops[i] {
            SceneOp::PushClip(rect) => {
                let inner_start = i + 1;
                let inner_end = matching_pop(plan, inner_start, end);
                let mask = ContentMask {
                    bounds: transform.bounds(*rect),
                };
                window.with_content_mask(Some(mask), |window| {
                    paint_range(
                        plan,
                        inner_start,
                        inner_end,
                        transform,
                        text_cache,
                        shaped_text,
                        window,
                        cx,
                        metrics,
                    );
                });
                // Skip the block and its PopClip.
                i = if inner_end < end { inner_end + 1 } else { end };
            }
            SceneOp::PopClip => {
                // Only reachable for an unbalanced plan, which the executor never produces.
                i += 1;
            }
            SceneOp::Quad {
                rect,
                fill: paint,
                corner_radii,
                border_width,
                border_color,
            } => {
                // Batch a run of identically-filled plain opaque quads into one GPUI Path.
                // GPUI charges ~1.3-1.7 us per `paint_quad` (a bounds-tree insert plus storing the
                // primitive twice), so a chart's thousands of grid lines, wicks and bodies dominate
                // submission cost. See `batchable_run` for why this cannot change the output.
                let run = batchable_run(plan, i, end);
                if run >= QUAD_BATCH_THRESHOLD {
                    if let Some(path) = build_quad_run_path(plan, i, i + run, transform) {
                        window.paint_path(path, to_background(*paint));
                        metrics.batched_quads += run as u32;
                        metrics.quad_batches += 1;
                        i += run;
                        continue;
                    }
                }
                let bounds = transform.bounds(*rect);
                let mut quad = fill(bounds, to_background(*paint));
                let [lt, rt, rb, lb] = *corner_radii;
                if lt != 0.0 || rt != 0.0 || rb != 0.0 || lb != 0.0 {
                    quad.corner_radii = gpui::Corners {
                        top_left: transform.len(lt),
                        top_right: transform.len(rt),
                        bottom_right: transform.len(rb),
                        bottom_left: transform.len(lb),
                    };
                }
                if *border_width > 0.0 {
                    quad.border_widths = gpui::Edges::all(transform.len(*border_width));
                    quad.border_color = to_hsla(*border_color);
                }
                window.paint_quad(quad);
                i += 1;
            }
            SceneOp::Mesh {
                first_vertex,
                vertex_count,
                fill: paint,
            } => {
                if let Some(path) = build_path(plan, *first_vertex, *vertex_count, transform) {
                    window.paint_path(path, to_background(*paint));
                }
                i += 1;
            }
            SceneOp::Text(run) => {
                paint_text(run, transform, text_cache, shaped_text, window, cx, metrics);
                i += 1;
            }
        }
    }
}

/// Minimum run length worth collapsing into one path. Below this the per-path overhead (an atlas
/// tile plus a second compositing pass) outweighs the saved `paint_quad` calls.
const QUAD_BATCH_THRESHOLD: usize = 8;

/// Length of the run of quads starting at `start` that may be collapsed into a single path.
///
/// A quad joins the run only if it is a *plain* quad — no corner radii, no border — with a **solid,
/// fully opaque** fill identical to the run's. Those conditions are what make the collapse
/// output-preserving:
///
/// - **Order is untouched.** Only a contiguous run is merged, so nothing is reordered relative to
///   anything else. The merged path takes the run's position in the stream.
/// - **Overlap is irrelevant.** Every quad in the run paints the same fully opaque colour, so a
///   pixel covered once and a pixel covered twice both end up exactly that colour — whether the
///   overlap composites inside GPUI's path atlas or between two separate quads.
/// - **No blending is skipped.** Opaque-only means the run cannot be one where alpha accumulation
///   would be observable; translucent quads are never batched.
///
/// Gradients are excluded because a GPUI gradient is resolved against the primitive's own bounds:
/// merging two gradient quads would restretch the ramp over the union.
fn batchable_run(plan: &ScenePlan, start: usize, end: usize) -> usize {
    let Some(SceneOp::Quad {
        fill: first_fill,
        corner_radii,
        border_width,
        ..
    }) = plan.ops.get(start)
    else {
        return 0;
    };
    if !is_plain_opaque(first_fill, corner_radii, border_width) {
        return 0;
    }
    let mut n = 1;
    while start + n < end {
        match &plan.ops[start + n] {
            SceneOp::Quad {
                fill,
                corner_radii,
                border_width,
                ..
            } if fill == first_fill && is_plain_opaque(fill, corner_radii, border_width) => n += 1,
            _ => break,
        }
    }
    n
}

/// Whether a quad is a plain, fully opaque, solid-filled rectangle.
fn is_plain_opaque(fill: &Paint, corner_radii: &[f32; 4], border_width: &f32) -> bool {
    let Paint::Solid(color) = fill else {
        return false;
    };
    color.a() == 0xff && *border_width <= 0.0 && corner_radii.iter().all(|r| *r == 0.0)
}

/// Two triangles per quad, in one path.
fn build_quad_run_path(
    plan: &ScenePlan,
    start: usize,
    end: usize,
    transform: Transform,
) -> Option<Path<Pixels>> {
    let solid = point(0.0, 1.0);
    let mut path: Option<Path<Pixels>> = None;
    for op in &plan.ops[start..end] {
        let SceneOp::Quad { rect, .. } = op else {
            continue;
        };
        let (x0, y0) = (rect.x, rect.y);
        let (x1, y1) = (rect.x + rect.w, rect.y + rect.h);
        let tl = transform.point(x0, y0);
        let tr = transform.point(x1, y0);
        let br = transform.point(x1, y1);
        let bl = transform.point(x0, y1);
        let p = path.get_or_insert_with(|| Path::new(tl));
        p.push_triangle((tl, tr, br), (solid, solid, solid));
        p.push_triangle((tl, br, bl), (solid, solid, solid));
    }
    path
}

/// Index of the `PopClip` matching the `PushClip` that opened at `start - 1`, or `end`.
fn matching_pop(plan: &ScenePlan, start: usize, end: usize) -> usize {
    let mut depth = 0usize;
    for (offset, op) in plan.ops[start..end].iter().enumerate() {
        match op {
            SceneOp::PushClip(_) => depth += 1,
            SceneOp::PopClip => {
                if depth == 0 {
                    return start + offset;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    end
}

/// Build a GPUI `Path` from a triangle-list mesh.
///
/// `st = (0, 1)` on all three vertices is GPUI's "solid interior" convention (the same one
/// `Path::line_to` uses): the path shader's curve term degenerates to full coverage, so the
/// triangle rasterizes hard-edged — matching the WebGPU backend's triangles rather than adding an
/// antialiasing pass Origin's other backends do not have.
fn build_path(
    plan: &ScenePlan,
    first_vertex: u32,
    vertex_count: u32,
    transform: Transform,
) -> Option<Path<Pixels>> {
    let verts = plan.mesh_vertices(first_vertex, vertex_count);
    if verts.len() < 3 {
        return None;
    }
    let solid = point(0.0, 1.0);
    let first = transform.point(verts[0].x, verts[0].y);
    let mut path = Path::new(first);
    for tri in verts.chunks_exact(3) {
        path.push_triangle(
            (
                transform.point(tri[0].x, tri[0].y),
                transform.point(tri[1].x, tri[1].y),
                transform.point(tri[2].x, tri[2].y),
            ),
            (solid, solid, solid),
        );
    }
    Some(path)
}

/// Shape and paint one text run (§8 Stage 2: GPUI's own text system).
///
/// Placement reproduces the Canvas2D contract the other backends implement: `x` is the aligned
/// edge, `y` is the run's vertical center, and the baseline is
/// `y + (ascent + descent) / 2` in `ab_glyph` sign convention. GPUI reports descent as positive
/// below the baseline, so it is negated on the way into [`crate::text::middle_baseline`].
///
/// `ShapedLine::paint` positions glyphs at `origin.y + (line_height - ascent - descent) / 2 +
/// ascent`. Passing `line_height = ascent + descent` zeroes that padding term, so the baseline is
/// exactly `origin.y + ascent` and the origin follows directly from the target baseline.
fn paint_text(
    run: &TextRun,
    transform: Transform,
    text_cache: &mut crate::text::TextCache,
    shaped_text: &mut ShapedTextCache,
    window: &mut Window,
    cx: &mut App,
    metrics: &mut GpuiFrameMetrics,
) {
    // GPUI's text system works in logical px, so the run's device-px size converts too.
    let font_size = transform.len(run.size);
    let shape_key = ShapedTextKey::for_run(run, font_size);
    let cached = shaped_text.get_or_shape(shape_key, || {
        let font = to_font(run);
        let text_system = window.text_system().clone();
        let font_id = text_system.resolve_font(&font);
        let ascent: f32 = text_system.ascent(font_id, font_size).into();
        let descent: f32 = text_system.descent(font_id, font_size).into();
        let gpui_run = gpui::TextRun {
            len: run.text.len(),
            font,
            color: to_hsla(run.color),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = text_system.shape_line(
            SharedString::from(run.text.clone()),
            font_size,
            std::slice::from_ref(&gpui_run),
            None,
        );
        CachedShapedText {
            line,
            ascent,
            descent,
        }
    });

    // Keep GPUI-free device-pixel metrics available for Stage 3 and host diagnostics. This cache
    // is separate from the shaped-line cache above; an actual shape-cache hit never calls
    // `shape_line`, even if a moving label's subpixel phase causes a metrics-cache miss.
    let logical_to_device = 1.0 / transform.inv_scale;
    let measured = TextMetrics {
        width: f32::from(cached.line.width) * logical_to_device,
        ascent: cached.ascent * logical_to_device,
        descent: -cached.descent * logical_to_device,
    };
    text_cache.measure_with(TextKey::for_run(run), || measured);

    // Placement in logical px, from the cached GPUI metrics.
    let width: f32 = cached.line.width.into();
    let anchor_x: f32 = transform.x(run.x).into();
    let anchor_y: f32 = transform.y(run.y).into();
    let left = text::aligned_left(anchor_x, width, run.align);
    let baseline = text::middle_baseline(anchor_y, cached.ascent, -cached.descent);
    let line_height = px(cached.ascent + cached.descent);
    let origin = point(px(left), px(baseline - cached.ascent));

    if cached.line.paint(origin, line_height, window, cx).is_ok() {
        metrics.glyph_runs_painted += 1;
    } else {
        metrics.dropped_prims += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use origin_render::draw_list::TextAlign;

    fn run(family: &str) -> TextRun {
        TextRun {
            x: 0.0,
            y: 0.0,
            text: "x".into(),
            color: Color::rgb(0, 0, 0),
            size: 12.0,
            family: family.into(),
            align: TextAlign::Left,
            weight: 400,
            italic: false,
        }
    }

    fn plain_quad(color: Color) -> SceneOp {
        SceneOp::Quad {
            rect: DeviceRect::new(0.0, 0.0, 4.0, 4.0),
            fill: Paint::Solid(color),
            corner_radii: [0.0; 4],
            border_width: 0.0,
            border_color: Color::rgba(0, 0, 0, 0),
        }
    }

    fn plan_of(ops: Vec<SceneOp>) -> ScenePlan {
        ScenePlan {
            ops,
            ..Default::default()
        }
    }

    const OPAQUE: Color = Color::rgb(0x26, 0xa6, 0x9a);
    const OPAQUE2: Color = Color::rgb(0xef, 0x53, 0x50);

    #[test]
    fn a_run_of_identical_opaque_quads_is_batchable() {
        let plan = plan_of(vec![plain_quad(OPAQUE); 10]);
        assert_eq!(batchable_run(&plan, 0, plan.ops.len()), 10);
    }

    #[test]
    fn a_run_stops_at_a_different_colour() {
        let mut ops = vec![plain_quad(OPAQUE); 5];
        ops.push(plain_quad(OPAQUE2));
        ops.extend(vec![plain_quad(OPAQUE); 3]);
        let plan = plan_of(ops);
        assert_eq!(batchable_run(&plan, 0, plan.ops.len()), 5);
    }

    #[test]
    fn translucent_quads_are_never_batched() {
        // Overlapping translucent quads accumulate alpha; merging them into one path would composite
        // the overlap once instead of twice, which is observable.
        let plan = plan_of(vec![plain_quad(Color::rgba(0x26, 0xa6, 0x9a, 0x80)); 10]);
        assert_eq!(batchable_run(&plan, 0, plan.ops.len()), 0);
    }

    #[test]
    fn gradient_quads_are_never_batched() {
        // A GPUI gradient resolves against the primitive's own bounds, so merging would restretch
        // the ramp over the union of the run.
        let plan = plan_of(vec![
            SceneOp::Quad {
                rect: DeviceRect::new(0.0, 0.0, 4.0, 4.0),
                fill: Paint::VGradient {
                    top: OPAQUE,
                    bottom: OPAQUE2,
                },
                corner_radii: [0.0; 4],
                border_width: 0.0,
                border_color: Color::rgba(0, 0, 0, 0),
            };
            10
        ]);
        assert_eq!(batchable_run(&plan, 0, plan.ops.len()), 0);
    }

    #[test]
    fn rounded_or_bordered_quads_are_never_batched() {
        let rounded = SceneOp::Quad {
            rect: DeviceRect::new(0.0, 0.0, 4.0, 4.0),
            fill: Paint::Solid(OPAQUE),
            corner_radii: [2.0; 4],
            border_width: 0.0,
            border_color: Color::rgba(0, 0, 0, 0),
        };
        assert_eq!(batchable_run(&plan_of(vec![rounded; 10]), 0, 10), 0);

        let bordered = SceneOp::Quad {
            rect: DeviceRect::new(0.0, 0.0, 4.0, 4.0),
            fill: Paint::Solid(OPAQUE),
            corner_radii: [0.0; 4],
            border_width: 1.0,
            border_color: OPAQUE2,
        };
        assert_eq!(batchable_run(&plan_of(vec![bordered; 10]), 0, 10), 0);
    }

    #[test]
    fn a_run_never_crosses_a_clip_boundary_or_another_op_kind() {
        let mut ops = vec![plain_quad(OPAQUE); 4];
        ops.push(SceneOp::PopClip);
        ops.extend(vec![plain_quad(OPAQUE); 4]);
        let plan = plan_of(ops);
        assert_eq!(batchable_run(&plan, 0, plan.ops.len()), 4);

        let mut ops = vec![plain_quad(OPAQUE); 3];
        ops.push(SceneOp::Mesh {
            first_vertex: 0,
            vertex_count: 3,
            fill: Paint::Solid(OPAQUE),
        });
        let plan = plan_of(ops);
        assert_eq!(batchable_run(&plan, 0, plan.ops.len()), 3);
    }

    #[test]
    fn a_run_is_clamped_to_the_range_end() {
        // `end` is the enclosing clip block's end; a run must never read past it.
        let plan = plan_of(vec![plain_quad(OPAQUE); 10]);
        assert_eq!(batchable_run(&plan, 0, 4), 4);
        assert_eq!(batchable_run(&plan, 8, 10), 2);
    }

    #[test]
    fn the_batch_path_has_two_triangles_per_quad() {
        let plan = plan_of(vec![plain_quad(OPAQUE); 5]);
        let t = Transform::new(OriginViewport::new(0.0, 0.0, 100.0, 100.0), 1.0);
        let path = build_quad_run_path(&plan, 0, 5, t).expect("a path");
        // 5 quads x 2 triangles x 3 vertices
        assert_eq!(format!("{path:?}").matches("PathVertex").count(), 30);
    }

    #[test]
    fn a_run_exactly_at_the_threshold_is_reported_in_full() {
        // The threshold gates whether batching is *worth* it; the predicate still reports the true
        // run length, so an off-by-one there would silently drop a quad from the frame.
        let plan = plan_of(vec![plain_quad(OPAQUE); QUAD_BATCH_THRESHOLD]);
        assert_eq!(
            batchable_run(&plan, 0, plan.ops.len()),
            QUAD_BATCH_THRESHOLD
        );
    }

    #[test]
    fn font_family_list_splits_into_family_plus_fallbacks() {
        let f = to_font(&run("Inter, \"Helvetica Neue\", sans-serif"));
        assert_eq!(f.family.as_ref(), "Inter");
        assert!(f.fallbacks.is_some());
    }

    #[test]
    fn a_single_family_has_no_fallbacks() {
        let f = to_font(&run("Inter"));
        assert_eq!(f.family.as_ref(), "Inter");
        assert!(f.fallbacks.is_none());
    }

    #[test]
    fn an_empty_family_falls_back_to_the_system_ui_font() {
        let f = to_font(&run("  "));
        assert_eq!(f.family.as_ref(), ".SystemUIFont");
    }

    #[test]
    fn weight_and_style_map_onto_gpui() {
        let mut r = run("Inter");
        r.weight = 700;
        r.italic = true;
        let f = to_font(&r);
        assert_eq!(f.weight, FontWeight(700.0));
        assert_eq!(f.style, FontStyle::Italic);
    }

    #[test]
    fn rgba_survives_the_hsla_round_trip_for_every_chart_palette_color() {
        // The engine's palette plus the greys and alphas the chart actually paints.
        let palette = [
            Color::rgb(0x26, 0xa6, 0x9a),
            Color::rgb(0xef, 0x53, 0x50),
            Color::rgb(0xd6, 0xdc, 0xde),
            Color::rgb(0x21, 0x96, 0xf3),
            Color::rgb(0x33, 0xd7, 0x78),
            Color::rgba(0x2e, 0xdc, 0x87, 102),
            Color::rgba(0x28, 0xdd, 0x64, 0),
            Color::rgba(0x26, 0xa6, 0x9a, 0x80),
            Color::rgb(0x13, 0x17, 0x22),
            Color::rgb(0x95, 0x98, 0xa1),
            Color::rgb(0x00, 0x00, 0x00),
            Color::rgb(0xff, 0xff, 0xff),
        ];
        for c in palette {
            let back = Rgba::from(to_hsla(c));
            let q = |v: f32| (v * 255.0).round() as u8;
            assert_eq!(
                (q(back.r), q(back.g), q(back.b), q(back.a)),
                (c.r(), c.g(), c.b(), c.a()),
                "{c:?} did not survive the Hsla round trip"
            );
        }
    }

    #[test]
    fn rgba_hsla_round_trip_is_exact_across_the_grey_ramp_and_hue_wheel() {
        for v in 0..=255u8 {
            let grey = Color::rgb(v, v, v);
            let back = Rgba::from(to_hsla(grey));
            assert_eq!((back.r * 255.0).round() as u8, v, "grey {v}");
        }
        for step in 0..64u8 {
            let c = Color::rgb(step.wrapping_mul(4), 255 - step * 4, 128);
            let back = Rgba::from(to_hsla(c));
            let q = |x: f32| (x * 255.0).round() as u8;
            assert_eq!((q(back.r), q(back.g), q(back.b)), (c.r(), c.g(), c.b()));
        }
    }

    #[test]
    fn solid_and_gradient_paints_map_to_the_right_background_kinds() {
        let solid = to_background(Paint::Solid(Color::rgb(1, 2, 3)));
        let grad = to_background(Paint::VGradient {
            top: Color::rgb(1, 2, 3),
            bottom: Color::rgb(4, 5, 6),
        });
        assert_ne!(
            format!("{solid:?}"),
            format!("{grad:?}"),
            "a gradient must not collapse to a solid"
        );
        assert!(format!("{solid:?}").starts_with("Solid"));
        assert!(format!("{grad:?}").starts_with("LinearGradient"));
        assert!(
            format!("{grad:?}").contains("180"),
            "the ramp must run top-to-bottom: {grad:?}"
        );
    }

    #[test]
    fn transform_maps_device_px_into_logical_px() {
        let t = Transform::new(OriginViewport::new(10.0, 20.0, 800.0, 600.0), 2.0);
        assert_eq!(f32::from(t.x(100.0)), 60.0);
        assert_eq!(f32::from(t.y(100.0)), 70.0);
        assert_eq!(f32::from(t.len(100.0)), 50.0);

        let b = t.bounds(DeviceRect::new(0.0, 0.0, 200.0, 100.0));
        assert_eq!(f32::from(b.origin.x), 10.0);
        assert_eq!(f32::from(b.size.width), 100.0);
    }

    #[test]
    fn matching_pop_finds_the_balanced_close_across_nesting() {
        let mut plan = ScenePlan::default();
        plan.ops
            .push(SceneOp::PushClip(DeviceRect::new(0.0, 0.0, 1.0, 1.0)));
        plan.ops
            .push(SceneOp::PushClip(DeviceRect::new(0.0, 0.0, 1.0, 1.0)));
        plan.ops.push(SceneOp::PopClip);
        plan.ops.push(SceneOp::PopClip);
        // The outer PushClip opened at 0, so its block starts at 1 and closes at index 3.
        assert_eq!(matching_pop(&plan, 1, plan.ops.len()), 3);
    }

    #[test]
    fn matching_pop_returns_end_for_an_unterminated_clip() {
        let mut plan = ScenePlan::default();
        plan.ops
            .push(SceneOp::PushClip(DeviceRect::new(0.0, 0.0, 1.0, 1.0)));
        assert_eq!(matching_pop(&plan, 1, plan.ops.len()), 1);
    }

    #[test]
    fn build_path_emits_one_triangle_per_three_vertices() {
        let mut plan = ScenePlan::default();
        for i in 0..6 {
            plan.vertices.push(crate::scene::MeshVertex {
                x: i as f32,
                y: i as f32,
            });
        }
        let t = Transform::new(OriginViewport::new(0.0, 0.0, 100.0, 100.0), 1.0);
        let path = build_path(&plan, 0, 6, t).expect("two triangles");
        // GPUI stores 3 vertices per pushed triangle.
        assert_eq!(format!("{path:?}").matches("PathVertex").count(), 6);
    }

    #[test]
    fn build_path_rejects_a_partial_triangle() {
        let mut plan = ScenePlan::default();
        plan.vertices
            .push(crate::scene::MeshVertex { x: 0.0, y: 0.0 });
        plan.vertices
            .push(crate::scene::MeshVertex { x: 1.0, y: 0.0 });
        let t = Transform::new(OriginViewport::new(0.0, 0.0, 100.0, 100.0), 1.0);
        assert!(build_path(&plan, 0, 2, t).is_none());
    }

    fn cached_line(ascent: f32) -> CachedShapedText {
        CachedShapedText {
            line: ShapedLine::default(),
            ascent,
            descent: 3.0,
        }
    }

    #[test]
    fn shaped_text_cache_avoids_reinvoking_the_shaper_on_a_hit() {
        let key = ShapedTextKey::for_run(&run("Inter"), px(12.0));
        let mut cache = ShapedTextCache::with_capacity(4);
        let first = cache.get_or_shape(key.clone(), || cached_line(9.0));
        let hit = cache.get_or_shape(key, || panic!("a cache hit must not shape again"));
        assert_eq!((first.ascent, hit.ascent), (9.0, 9.0));
        assert_eq!(cache.counters(), (1, 1));
    }

    #[test]
    fn shaped_text_cache_is_bounded_and_evicts_the_lru_entry() {
        let mut cache = ShapedTextCache::with_capacity(2);
        let mut a = run("Inter");
        a.text = "a".into();
        let mut b = run("Inter");
        b.text = "b".into();
        let mut c = run("Inter");
        c.text = "c".into();
        let (ka, kb, kc) = (
            ShapedTextKey::for_run(&a, px(12.0)),
            ShapedTextKey::for_run(&b, px(12.0)),
            ShapedTextKey::for_run(&c, px(12.0)),
        );
        cache.get_or_shape(ka.clone(), || cached_line(1.0));
        cache.get_or_shape(kb.clone(), || cached_line(2.0));
        cache.get_or_shape(ka.clone(), || panic!("a should hit"));
        cache.get_or_shape(kc.clone(), || cached_line(3.0));

        assert_eq!(cache.entries.len(), 2);
        assert!(cache.entries.contains_key(&ka));
        assert!(cache.entries.contains_key(&kc));
        assert!(
            !cache.entries.contains_key(&kb),
            "b was least recently used"
        );
    }

    #[test]
    fn shaped_text_key_reuses_layout_across_position_and_alignment_changes() {
        let a = run("Inter");
        let mut b = a.clone();
        b.x = 41.25;
        b.y = 99.75;
        b.align = TextAlign::Right;
        assert_eq!(
            ShapedTextKey::for_run(&a, px(12.0)),
            ShapedTextKey::for_run(&b, px(12.0))
        );

        b.color = Color::rgb(1, 2, 3);
        assert_ne!(
            ShapedTextKey::for_run(&a, px(12.0)),
            ShapedTextKey::for_run(&b, px(12.0)),
            "the shaped line owns its decoration colour"
        );
    }
}
