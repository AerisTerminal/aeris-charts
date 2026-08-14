//! Drawing-tool frame emission (model in drawings.rs): each pane's committed drawings in
//! z-order, the selected drawing's anchor handles, and the interactive-creation preview.
//!
//! Coordinates follow the frame build's conventions (frame/mod.rs): x is pane-local media px
//! scaled by the exact horizontal ratio (the trailing `translate_prims_x` shifts everything
//! when a left axis reserves space), y is chart-top-relative media px scaled by the vertical
//! ratio — the same space the price-line/series geometry uses, so a drawing's prims land
//! exactly on its converted anchors. Text goes through `Prim::Text`, so labels rasterize
//! identically on both backends (the Canvas2D `fillText` path and the WebGPU atlas share the
//! browser's glyph rasterizer by construction).

use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::{IRect, LineStyle, LineType, Prim, TextAlign};

use super::PRIMARY;
use crate::drawings::{
    Drawing, DrawingKind, DrawingTextHAlign, TEXT_PAD, TEXT_PLACEHOLDER_MIN_SIZE,
};
use crate::ChartEngine;

/// TradingView-style drawing anchor handle: a theme-derived disc with the primary-token border
/// (the crosshair-marks disc idiom — the border is a larger filled disc underneath). Slightly
/// larger than the series selection anchors (2.5/1.5, series_geometry.rs) since these are
/// drag targets.
const ANCHOR_RADIUS: f64 = 4.0;
const ANCHOR_BORDER_WIDTH: f64 = 1.5;
const ANCHOR_BORDER: Color = PRIMARY;

impl ChartEngine {
    #[cfg(test)]
    pub(crate) fn build_drawings_frame_reference(
        &self,
        pane_index: usize,
        pane_w_px: i32,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        for drawing in &self.drawings {
            if drawing.pane_index != pane_index
                || !self.drawing_viewport_candidate_reference(drawing)
            {
                continue;
            }
            let Some(px) = self.drawing_px(drawing) else {
                continue;
            };
            let px = px
                .into_iter()
                .map(|(x, y)| (x * hpr, y * vpr))
                .collect::<Vec<_>>();
            self.build_drawing_prims(drawing, &px, pane_w_px, vpr, out, points);
            self.build_drawing_text(drawing, &px, pane_w_px, vpr, out);
        }
    }

    /// Emit the viewport candidates bound to `pane_index` in canonical z-order plus the
    /// in-progress creation preview. Drawings on stale panes draw nowhere.
    pub(crate) fn build_drawings_frame(
        &self,
        pane_index: usize,
        pane_w_px: i32,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        if self.drawing_runtime.borrow().pane_count(pane_index) <= 20 {
            for drawing in &self.drawings {
                if drawing.pane_index != pane_index {
                    continue;
                }
                let Some(px) = self.drawing_px(drawing) else {
                    continue;
                };
                let px = px
                    .into_iter()
                    .map(|(x, y)| (x * hpr, y * vpr))
                    .collect::<Vec<_>>();
                self.build_drawing_prims(drawing, &px, pane_w_px, vpr, out, points);
                self.build_drawing_text(drawing, &px, pane_w_px, vpr, out);
            }
        } else {
            let (candidates, key) = self.take_drawing_candidates(pane_index, None);
            if let Some(key) = key {
                let mut runtime = self.drawing_runtime.borrow_mut();
                for &id in &candidates {
                    let Some(position) = runtime.position(id) else {
                        continue;
                    };
                    let Some(drawing) = self.drawings.get(position) else {
                        continue;
                    };
                    let Some(px) = self.drawing_px_cached(drawing, &mut runtime, key) else {
                        continue;
                    };
                    let px = px
                        .iter()
                        .map(|&(x, y)| (x * hpr, y * vpr))
                        .collect::<Vec<_>>();
                    self.build_drawing_prims(drawing, &px, pane_w_px, vpr, out, points);
                    self.build_drawing_text(drawing, &px, pane_w_px, vpr, out);
                    runtime.record_visible();
                }
            }
            self.recycle_drawing_candidates(candidates);
        }
        // Live brush stroke: the decimated points so far paint as the same smooth curve the
        // commit will store, so what the user sees while dragging is what they get.
        if let Some(capture) = self.brush_capture() {
            if capture.pane_index == pane_index && capture.points.len() >= 2 {
                let px: Option<Vec<(f64, f64)>> = capture
                    .points
                    .iter()
                    .map(|&point| self.drawing_to_px(pane_index, point))
                    .collect();
                if let Some(px) = px {
                    let px: Vec<(f64, f64)> =
                        px.into_iter().map(|(x, y)| (x * hpr, y * vpr)).collect();
                    self.build_drawing_prims(&capture.options, &px, pane_w_px, vpr, out, points);
                }
            }
        }
        // Interactive creation: committed anchors plus the preview point render as a tentative
        // drawing, with handles on the committed anchors (the reference rectangle-drawing-tool's
        // PreviewRectangle — same geometry, shown while placing).
        if let Some(pending) = self.pending_drawing() {
            if pending.drawing.pane_index == pane_index {
                let mut anchors = pending.drawing.points.clone();
                if let Some(preview) = pending.preview {
                    if anchors.len() < pending.drawing.kind.anchor_count() {
                        anchors.push(preview);
                    }
                }
                if anchors.len() == pending.drawing.kind.anchor_count() {
                    let px: Option<Vec<(f64, f64)>> = anchors
                        .iter()
                        .map(|&point| self.drawing_to_px(pane_index, point))
                        .collect();
                    if let Some(px) = px {
                        let px: Vec<(f64, f64)> =
                            px.into_iter().map(|(x, y)| (x * hpr, y * vpr)).collect();
                        self.build_drawing_prims(
                            &pending.drawing,
                            &px,
                            pane_w_px,
                            vpr,
                            out,
                            points,
                        );
                        if pending.drawing.kind == DrawingKind::Rectangle {
                            // TradingView shows all eight anchors while the rectangle is being
                            // drawn (committed corner + live preview corner), not only after
                            // the commit.
                            build_rectangle_handles(&px, vpr, self.anchor_fill(), out);
                        } else {
                            let committed = pending.drawing.points.len();
                            build_anchor_handles(&px[..committed], vpr, self.anchor_fill(), out);
                        }
                    }
                } else if anchors.len() == 1 {
                    // A one-anchor kind awaiting its click, or a two-anchor kind before the
                    // preview resolves: show the placed anchor as a handle alone.
                    if let Some((x, y)) = self.drawing_to_px(pane_index, anchors[0]) {
                        build_anchor_handles(&[(x * hpr, y * vpr)], vpr, self.anchor_fill(), out);
                    }
                }
            }
        }
    }

    /// Selection handles are retained with the overlay, so selection-only changes do not
    /// invalidate or reconstruct unrelated drawing geometry.
    pub(super) fn build_selected_drawing_handles_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let Some(id) = self.selected_drawing else {
            return;
        };
        let Some(drawing) = self.drawing(id) else {
            return;
        };
        if drawing.pane_index != pane_index {
            return;
        }
        let Some(key) = self.drawing_coordinate_key(pane_index) else {
            return;
        };
        let mut runtime = self.drawing_runtime.borrow_mut();
        let Some(px) = self.drawing_px_cached(drawing, &mut runtime, key) else {
            return;
        };
        let px = px
            .iter()
            .map(|&(x, y)| (x * hpr, y * vpr))
            .collect::<Vec<_>>();
        if drawing.kind == DrawingKind::Rectangle && px.len() == 2 {
            build_rectangle_handles(&px, vpr, self.anchor_fill(), out);
        } else if drawing.kind == DrawingKind::Brush && px.len() > 2 {
            build_anchor_handles(&[px[0], px[px.len() - 1]], vpr, self.anchor_fill(), out);
        } else {
            build_anchor_handles(&px, vpr, self.anchor_fill(), out);
        }
    }

    /// One drawing's geometry prims at bitmap-px anchors `px`.
    fn build_drawing_prims(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        pane_w_px: i32,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let color = Color::parse_css(&drawing.color).unwrap_or(PRIMARY);
        let crisp_width = (drawing.width * vpr).round().max(1.0) as i32;
        match drawing.kind {
            DrawingKind::TrendLine => {
                let first_point = points.len() as u32;
                points.push([px[0].0 as f32, px[0].1 as f32]);
                points.push([px[1].0 as f32, px[1].1 as f32]);
                out.push(Prim::Polyline {
                    first_point,
                    point_count: 2,
                    width: (drawing.width * vpr) as f32,
                    style: drawing.style,
                    line_type: LineType::Simple,
                    color,
                });
            }
            DrawingKind::HorizontalLine => {
                out.push(Prim::HLine {
                    y: px[0].1.round() as i32,
                    x0: 0,
                    x1: pane_w_px,
                    width: crisp_width,
                    style: drawing.style,
                    color,
                });
            }
            DrawingKind::HorizontalRay => {
                let x0 = (px[0].0.round() as i32).clamp(0, pane_w_px);
                if x0 < pane_w_px {
                    out.push(Prim::HLine {
                        y: px[0].1.round() as i32,
                        x0,
                        x1: pane_w_px,
                        width: crisp_width,
                        style: drawing.style,
                        color,
                    });
                }
            }
            DrawingKind::VerticalLine => {
                let pane = &self.panes[drawing.pane_index];
                out.push(Prim::VLine {
                    x: px[0].0.round() as i32,
                    y0: (pane.top * vpr).round().max(0.0) as i32,
                    y1: ((pane.top + pane.height) * vpr).round().max(0.0) as i32,
                    width: crisp_width,
                    style: drawing.style,
                    color,
                });
            }
            DrawingKind::Rectangle => {
                let (a, b) = (px[0], px[1]);
                let left = a.0.min(b.0).round() as i32;
                let top = a.1.min(b.1).round() as i32;
                let width = (a.0 - b.0).abs().round() as i32;
                let height = (a.1 - b.1).abs().round() as i32;
                if width <= 0 || height <= 0 {
                    return;
                }
                // reference rectangle-drawing-tool default: the fill is the border color washed
                // out (its `previewFillColor`/`fillColor` alpha pattern) — 20% here.
                let fill = drawing
                    .fill_color
                    .as_deref()
                    .and_then(Color::parse_css)
                    .unwrap_or(Color::rgba(color.r(), color.g(), color.b(), 51));
                out.push(Prim::Rect {
                    rect: IRect {
                        x: left,
                        y: top,
                        w: width,
                        h: height,
                    },
                    color: fill,
                });
                if drawing.style == LineStyle::Solid {
                    out.push(Prim::RectFrame {
                        rect: IRect {
                            x: left,
                            y: top,
                            w: width,
                            h: height,
                        },
                        border: crisp_width,
                        color,
                    });
                } else {
                    // Dotted/dashed border: four crisp line prims sharing the dash pattern,
                    // centered on the frame's inner edge (where RectFrame paints).
                    let half = (crisp_width as f64 / 2.0) as i32;
                    for y in [top + half, top + height - half] {
                        out.push(Prim::HLine {
                            y,
                            x0: left,
                            x1: left + width,
                            width: crisp_width,
                            style: drawing.style,
                            color,
                        });
                    }
                    for x in [left + half, left + width - half] {
                        out.push(Prim::VLine {
                            x,
                            y0: top,
                            y1: top + height,
                            width: crisp_width,
                            style: drawing.style,
                            color,
                        });
                    }
                }
            }
            // The text tool's geometry IS its label (emitted by `build_drawing_text`).
            DrawingKind::Text => {}
            DrawingKind::Brush => {
                // TradingView's brush stroke: ONE smooth curved polyline through the
                // simplified path (the same `LineType::Curved` interpolation the series line
                // family uses), so the stroke is ultra smooth and identical on both backends.
                let first_point = points.len() as u32;
                for &(x, y) in px {
                    points.push([x as f32, y as f32]);
                }
                out.push(Prim::Polyline {
                    first_point,
                    point_count: px.len() as u32,
                    width: (drawing.width * vpr) as f32,
                    style: drawing.style,
                    line_type: LineType::Curved,
                    color,
                });
            }
        }
    }

    /// One drawing's text label (every tool can carry one): the placement resolves the 3×3
    /// alignment against the tool's reference box (drawings.rs `text_box`/`text_placement`),
    /// then emits a `Prim::Text` — x is the aligned edge, y the vertical center (the IR's
    /// middle-baseline convention), so the run rasterizes identically on both backends. An
    /// empty text tool renders the muted "+ Add Text" prompt (drawings.rs `TEXT_PLACEHOLDER`),
    /// and a text tool with a `box_color`/`box_border_color` gets its container (crisp
    /// integer-snapped `Rect`/`RectFrame` prims behind the run — TradingView's text-box
    /// background/border).
    fn build_drawing_text(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        pane_w_px: i32,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let is_text_tool = drawing.kind == DrawingKind::Text;
        if drawing.text.is_empty() && !is_text_tool {
            return;
        }
        // While the host's typing-mode editor owns a text drawing, its label/placeholder is
        // suppressed — the editor's preview is the only visual for it (TradingView's editing
        // state).
        if is_text_tool && self.editing_drawing == Some(drawing.id) {
            return;
        }
        let placeholder = is_text_tool && drawing.text.is_empty();
        let layout = &self.options.get().layout;
        let size = if placeholder {
            // The preview reads bold + bigger (≥ 12 CSS px, TradingView's prompt).
            drawing
                .text_size
                .unwrap_or(layout.font_size)
                .max(TEXT_PLACEHOLDER_MIN_SIZE)
                * vpr
        } else {
            drawing.text_size.unwrap_or(layout.font_size) * vpr
        };
        let pane = &self.panes[drawing.pane_index];
        let reference = ChartEngine::text_box(
            drawing.kind,
            px,
            f64::from(pane_w_px),
            pane.top * vpr,
            pane.height * vpr,
        );
        let (x, y, align) = ChartEngine::text_placement(drawing, &reference, size, TEXT_PAD * vpr);
        let token = if placeholder {
            &layout.muted_text_color
        } else {
            &layout.text_color
        };
        let color = drawing
            .text_color
            .as_deref()
            .filter(|_| !placeholder)
            .and_then(Color::parse_css)
            .or_else(|| Color::parse_css(token))
            .unwrap_or_else(|| {
                let fallback = if placeholder {
                    nucleuscharts_core::style::DEFAULT_MUTED_FOREGROUND_RGB
                } else {
                    nucleuscharts_core::style::DEFAULT_FOREGROUND_RGB
                };
                Color::rgb(fallback.0, fallback.1, fallback.2)
            });

        // The container (text tool with a background/border): a box wrapping the run, emitted
        // as the rectangle tool's crisp integer-snapped prims (`Rect` fill + `RectFrame`
        // border) — strong-color thin geometry at fractional positions AA-phases differently
        // between the backends, so the box snaps to whole device px (TradingView's boxes are
        // crisp the same way).
        let box_fill = drawing.box_color.as_deref().and_then(Color::parse_css);
        let box_border = drawing
            .box_border_color
            .as_deref()
            .and_then(Color::parse_css);
        if is_text_tool && (box_fill.is_some() || box_border.is_some()) {
            let width = self.measure_drawing_text(drawing, size);
            let height = size * 1.2;
            let pad = 4.0 * vpr;
            let left = match align {
                DrawingTextHAlign::Left => x,
                DrawingTextHAlign::Center => x - width / 2.0,
                DrawingTextHAlign::Right => x - width,
            };
            let rect = IRect {
                x: (left - pad).round() as i32,
                y: (y - height / 2.0 - pad).round() as i32,
                w: (width + 2.0 * pad).round().max(1.0) as i32,
                h: (height + 2.0 * pad).round().max(1.0) as i32,
            };
            if let Some(fill) = box_fill {
                out.push(Prim::Rect { rect, color: fill });
            }
            if let Some(border) = box_border {
                out.push(Prim::RectFrame {
                    rect,
                    border: (drawing.box_border_width * vpr).round().max(1.0) as i32,
                    color: border,
                });
            }
        }

        out.push(Prim::Text {
            x: x as f32,
            y: y as f32,
            text: drawing.display_text().to_string(),
            color,
            size: size as f32,
            family: layout.font_family.clone(),
            align: match align {
                DrawingTextHAlign::Left => TextAlign::Left,
                DrawingTextHAlign::Center => TextAlign::Center,
                DrawingTextHAlign::Right => TextAlign::Right,
            },
            // The preview reads bold (TradingView's prompt); the committed label uses the
            // drawing's own weight (normal 400 when unset).
            weight: if placeholder {
                700
            } else {
                drawing.text_weight.unwrap_or(400)
            },
            italic: drawing.text_italic,
        });
    }

    /// The anchor-handle fill for the current theme (white on light backgrounds, black on dark —
    /// the series selection anchors' luminance rule, series_geometry.rs).
    fn anchor_fill(&self) -> Color {
        let fallback = nucleuscharts_core::style::DEFAULT_SURFACE_RGB;
        let background = Color::parse_css(&self.options.get().layout.background.color)
            .unwrap_or(Color::rgb(fallback.0, fallback.1, fallback.2));
        if background.luminance() > 160.0 {
            Color::rgb(0xff, 0xff, 0xff)
        } else {
            Color::rgb(0, 0, 0)
        }
    }
}

/// One handle per anchor: the border disc underneath, the fill disc on top.
fn build_anchor_handles(px: &[(f64, f64)], vpr: f64, fill: Color, out: &mut Vec<Prim>) {
    for &(cx, cy) in px {
        out.push(Prim::Circle {
            cx: cx as f32,
            cy: cy as f32,
            radius: ((ANCHOR_RADIUS + ANCHOR_BORDER_WIDTH) * vpr) as f32,
            fill: ANCHOR_BORDER,
            stroke_width: 0.0,
            stroke: ANCHOR_BORDER,
        });
        out.push(Prim::Circle {
            cx: cx as f32,
            cy: cy as f32,
            radius: (ANCHOR_RADIUS * vpr) as f32,
            fill,
            stroke_width: 0.0,
            stroke: fill,
        });
    }
}

/// The rectangle's eight TradingView handles: fully-rounded discs on the four corners and
/// slightly-rounded square handles on the four edge midpoints (the midpoint drags resize one
/// edge independently).
fn build_rectangle_handles(px: &[(f64, f64)], vpr: f64, fill: Color, out: &mut Vec<Prim>) {
    let anchors = ChartEngine::rectangle_anchors(px);
    for (index, &(cx, cy)) in anchors.iter().enumerate() {
        if index % 2 == 0 {
            // Corners: the standard disc handles.
            out.push(Prim::Circle {
                cx: cx as f32,
                cy: cy as f32,
                radius: ((ANCHOR_RADIUS + ANCHOR_BORDER_WIDTH) * vpr) as f32,
                fill: ANCHOR_BORDER,
                stroke_width: 0.0,
                stroke: ANCHOR_BORDER,
            });
            out.push(Prim::Circle {
                cx: cx as f32,
                cy: cy as f32,
                radius: (ANCHOR_RADIUS * vpr) as f32,
                fill,
                stroke_width: 0.0,
                stroke: fill,
            });
        } else {
            // Edge midpoints: slightly-rounded squares (2px corner radius), border square
            // underneath, fill square on top.
            let outer = ((ANCHOR_RADIUS + ANCHOR_BORDER_WIDTH) * vpr) as f32;
            let inner = (ANCHOR_RADIUS * vpr) as f32;
            let radii = [2.0 * vpr as f32; 4];
            out.push(Prim::RoundRect {
                x: cx as f32 - outer,
                y: cy as f32 - outer,
                w: outer * 2.0,
                h: outer * 2.0,
                radii,
                fill: ANCHOR_BORDER,
                border_width: 0.0,
                border_color: ANCHOR_BORDER,
            });
            out.push(Prim::RoundRect {
                x: cx as f32 - inner,
                y: cy as f32 - inner,
                w: inner * 2.0,
                h: inner * 2.0,
                radii,
                fill,
                border_width: 0.0,
                border_color: fill,
            });
        }
    }
}
