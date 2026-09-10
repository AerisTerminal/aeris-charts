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

use super::{POSITION_ENTRY, PRIMARY};
use crate::drawings::{
    resolve_drawing_geometry, Drawing, DrawingBodyGeometry, DrawingHandleMode, DrawingId,
    DrawingKind, DrawingTextHAlign, PositionGeometry, PositionZone, TEXT_CHROME_PAD, TEXT_PAD,
};
use crate::ChartEngine;
use nucleuscharts_core::model::plot_list::PlotValueIndex;

/// TradingView-style drawing anchor handle: a theme-derived disc with the primary-token border
/// (the crosshair-marks disc idiom — the border is a larger filled disc underneath). Slightly
/// larger than the series selection anchors (2.5/1.5, series_geometry.rs) since these are
/// drag targets.
const ANCHOR_RADIUS: f64 = 4.0;
const ANCHOR_BORDER_WIDTH: f64 = 1.5;
const ANCHOR_BORDER: Color = PRIMARY;
/// The hover ring's dimmed variant of the focus border (TradingView shows the same border at
/// roughly half strength until the drawing is actually selected).
const HOVER_BORDER: Color = Color(PRIMARY.0 & 0xFFFF_FF00 | 0x73);

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

    /// Segmented committed build for retained reassembly: stable z-order committed drawings,
    /// recording each emitted drawing's prim/point range in `parts` (stable z-order) and the
    /// trailing preview (brush + pending) start in `preview_start` (prim, point). Previews
    /// always trail committed so assembly can place them topmost among chart content.
    /// Ordering.rs reassembles these parts idle-below / active-above without rebuilding
    /// geometry on hover/selection. Drawings on stale panes draw nowhere.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_drawings_frame_segmented(
        &self,
        pane_index: usize,
        pane_w_px: i32,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
        parts: &mut Vec<super::RetainedDrawingPart>,
        preview_start: &mut (usize, usize),
    ) {
        parts.clear();
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
                let prim_start = out.len();
                let point_start = points.len();
                self.build_drawing_prims(drawing, &px, pane_w_px, vpr, out, points);
                self.build_drawing_text(drawing, &px, pane_w_px, vpr, out);
                parts.push(super::RetainedDrawingPart {
                    id: drawing.id,
                    prim_start,
                    prim_end: out.len(),
                    point_start,
                    point_end: points.len(),
                });
            }
        } else {
            let candidates = self.take_drawing_candidates(pane_index, None);
            let mut runtime = self.drawing_runtime.borrow_mut();
            for &id in &candidates {
                let Some(position) = runtime.position(id) else {
                    continue;
                };
                let Some(drawing) = self.drawings.get(position) else {
                    continue;
                };
                let Some(key) = self.drawing_coordinate_key(drawing) else {
                    continue;
                };
                let Some(px) = self.drawing_px_cached(drawing, &mut runtime, key) else {
                    continue;
                };
                let px = px
                    .iter()
                    .map(|&(x, y)| (x * hpr, y * vpr))
                    .collect::<Vec<_>>();
                let prim_start = out.len();
                let point_start = points.len();
                self.build_drawing_prims(drawing, &px, pane_w_px, vpr, out, points);
                self.build_drawing_text(drawing, &px, pane_w_px, vpr, out);
                parts.push(super::RetainedDrawingPart {
                    id: drawing.id,
                    prim_start,
                    prim_end: out.len(),
                    point_start,
                    point_end: points.len(),
                });
                runtime.record_visible();
            }
            drop(runtime);
            self.recycle_drawing_candidates(candidates);
        }
        *preview_start = (out.len(), points.len());
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
                let is_sequence = pending.drawing.kind.spec().placement.is_sequence();
                if let Some(preview) = pending.preview {
                    if is_sequence || anchors.len() < pending.drawing.kind.anchor_count() {
                        anchors.push(preview);
                    }
                }
                let ready = if is_sequence {
                    anchors.len() >= pending.drawing.kind.anchor_count()
                } else {
                    anchors.len() == pending.drawing.kind.anchor_count()
                };
                if ready {
                    let px: Option<Vec<(f64, f64)>> = anchors
                        .iter()
                        .map(|&point| {
                            self.drawing_to_px_for(pane_index, pending.drawing.price_scale, point)
                        })
                        .collect();
                    if let Some(px) = px {
                        let px: Vec<(f64, f64)> =
                            px.into_iter().map(|(x, y)| (x * hpr, y * vpr)).collect();
                        let mut preview_drawing = pending.drawing.clone();
                        if preview_drawing.kind.spec().handles == DrawingHandleMode::RectangleBounds
                        {
                            if let Some(fill) = preview_drawing.preview_fill_color.clone() {
                                preview_drawing.fill_color = Some(fill);
                            }
                        }
                        self.build_drawing_prims(
                            &preview_drawing,
                            &px,
                            pane_w_px,
                            vpr,
                            out,
                            points,
                        );
                        if pending.drawing.kind.spec().handles == DrawingHandleMode::RectangleBounds
                        {
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
                    if let Some((x, y)) =
                        self.drawing_to_px_for(pane_index, pending.drawing.price_scale, anchors[0])
                    {
                        build_anchor_handles(&[(x * hpr, y * vpr)], vpr, self.anchor_fill(), out);
                    }
                }
            }
        }
    }

    /// The selected/hovered drawing's converted bitmap-px anchor points, or `None` when the
    /// id is stale, on another pane, or off-screen.
    fn overlay_drawing_px(
        &self,
        pane_index: usize,
        id: DrawingId,
        hpr: f64,
        vpr: f64,
    ) -> Option<Vec<(f64, f64)>> {
        let drawing = self.drawing(id)?;
        if drawing.pane_index != pane_index {
            return None;
        }
        let key = self.drawing_coordinate_key(drawing)?;
        let mut runtime = self.drawing_runtime.borrow_mut();
        let px = self.drawing_px_cached(drawing, &mut runtime, key)?;
        Some(
            px.iter()
                .map(|&(x, y)| (x * hpr, y * vpr))
                .collect::<Vec<_>>(),
        )
    }

    /// Selection chrome is retained with the overlay, so selection-only changes do not
    /// invalidate or reconstruct unrelated drawing geometry. The text tool gets no anchor
    /// handles (TradingView: text has no drag points) — its selection affordance is the focus
    /// border alone. That border STAYS painted while the host typing-mode editor is open
    /// (the wrap is borderless; only the caret overlays), so entering/leaving edit cannot
    /// shift the outline.
    pub(super) fn build_selected_drawing_handles_frame(
        &self,
        pane_index: usize,
        pane_w_px: i32,
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
        if drawing.kind.spec().requests_text_editor {
            let Some(px) = self.overlay_drawing_px(pane_index, id, hpr, vpr) else {
                return;
            };
            self.push_text_chrome(drawing, &px, pane_w_px, vpr, ANCHOR_BORDER, out);
            return;
        }
        let Some(px) = self.overlay_drawing_px(pane_index, id, hpr, vpr) else {
            return;
        };
        match drawing.kind.spec().handles {
            DrawingHandleMode::None => {}
            DrawingHandleMode::RectangleBounds if px.len() == 2 => {
                build_rectangle_handles(&px, vpr, self.anchor_fill(), out);
            }
            DrawingHandleMode::Position if px.len() == 3 => {
                build_position_handles(&px, vpr, self.anchor_fill(), out);
            }
            DrawingHandleMode::Endpoints if !px.is_empty() => {
                build_anchor_handles(&[px[0], px[px.len() - 1]], vpr, self.anchor_fill(), out);
            }
            DrawingHandleMode::Anchors | DrawingHandleMode::Endpoints => {
                build_anchor_handles(&px, vpr, self.anchor_fill(), out);
            }
            DrawingHandleMode::RectangleBounds | DrawingHandleMode::Position => {}
        }
    }

    /// The hovered text drawing's focus border at hover opacity (TradingView's hover ring):
    /// the same chrome box as selection, dimmed. Suppressed while the drawing is selected
    /// (the full-strength border already paints, including during typing mode).
    pub(super) fn build_hovered_text_frame(
        &self,
        pane_index: usize,
        pane_w_px: i32,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let Some(id) = self.hovered_text else {
            return;
        };
        if self.selected_drawing == Some(id) {
            return;
        }
        let Some(drawing) = self.drawing(id) else {
            return;
        };
        let Some(px) = self.overlay_drawing_px(pane_index, id, hpr, vpr) else {
            return;
        };
        self.push_text_chrome(drawing, &px, pane_w_px, vpr, HOVER_BORDER, out);
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
        let Some(pane) = self.panes.get(drawing.pane_index) else {
            return;
        };
        let Some(geometry) = resolve_drawing_geometry(
            drawing.kind,
            px,
            f64::from(pane_w_px),
            pane.top * vpr,
            pane.height * vpr,
            drawing.width,
            vpr,
        ) else {
            return;
        };
        match geometry.body {
            DrawingBodyGeometry::Segment { a, b } => {
                let first_point = points.len() as u32;
                points.push([a.0 as f32, a.1 as f32]);
                points.push([b.0 as f32, b.1 as f32]);
                out.push(Prim::Polyline {
                    first_point,
                    point_count: 2,
                    width: (drawing.width * vpr) as f32,
                    style: drawing.style,
                    line_type: LineType::Simple,
                    color,
                });
            }
            DrawingBodyGeometry::Horizontal { y, x0, x1 } => {
                let x0 = (x0.round() as i32).clamp(0, pane_w_px);
                let x1 = (x1.round() as i32).clamp(0, pane_w_px);
                if x0 != x1 {
                    out.push(Prim::HLine {
                        y: y.round() as i32,
                        x0: x0.min(x1),
                        x1: x0.max(x1),
                        width: crisp_width,
                        style: drawing.style,
                        color,
                    });
                }
            }
            DrawingBodyGeometry::Vertical { x, y0, y1 } => {
                out.push(Prim::VLine {
                    x: x.round() as i32,
                    y0: y0.round().max(0.0) as i32,
                    y1: y1.round().max(0.0) as i32,
                    width: crisp_width,
                    style: drawing.style,
                    color,
                });
            }
            DrawingBodyGeometry::Rectangle {
                left,
                right,
                top,
                bottom,
            } => {
                let left = left.round() as i32;
                let right = right.round() as i32;
                let top = top.round() as i32;
                let bottom = bottom.round() as i32;
                // Official `positionsBox`: both endpoint pixels belong to the box, so an
                // equal-point preview still occupies one bitmap pixel.
                let width = (right - left).abs() + 1;
                let height = (bottom - top).abs() + 1;
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
                if !drawing.border_visible {
                    return;
                }
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
            DrawingBodyGeometry::Position(position) => {
                let reward = Color::parse_css(nucleuscharts_core::style::MARKET_UP_CSS)
                    .unwrap_or(Color::rgb(8, 153, 129));
                let risk = Color::parse_css(nucleuscharts_core::style::MARKET_DOWN_CSS)
                    .unwrap_or(Color::rgb(247, 82, 95));
                push_position_zone(out, position.reward_zone(), reward);
                push_position_zone(out, position.risk_zone(), risk);

                let left_px = position.left.round() as i32;
                let right_px = position.right.round() as i32;
                if left_px != right_px {
                    out.push(Prim::HLine {
                        y: position.entry_y.round() as i32,
                        x0: left_px,
                        x1: right_px,
                        width: crisp_width,
                        style: drawing.style,
                        color: POSITION_ENTRY,
                    });
                }
                self.build_position_labels(drawing, position, vpr, reward, risk, out);
            }
            // The text tool's geometry is its label (emitted by `build_drawing_text`).
            DrawingBodyGeometry::Empty => {}
            DrawingBodyGeometry::Polyline {
                points: line_points,
                line_type,
                terminal,
            } => {
                let first_point = points.len() as u32;
                for &(x, y) in line_points {
                    points.push([x as f32, y as f32]);
                }
                out.push(Prim::Polyline {
                    first_point,
                    point_count: line_points.len() as u32,
                    width: (drawing.width * vpr) as f32,
                    style: drawing.style,
                    line_type,
                    color,
                });
                if let Some(terminal) = terminal {
                    let first_point = points.len() as u32;
                    for (x, y) in terminal {
                        points.push([x as f32, y as f32]);
                    }
                    out.push(Prim::Polyline {
                        first_point,
                        point_count: 3,
                        width: (drawing.width * vpr) as f32,
                        style: LineStyle::Solid,
                        line_type: LineType::Simple,
                        color,
                    });
                }
            }
        }
    }

    /// The text run's resolved glyph size (bitmap px, placeholder floor included), aligned
    /// anchor point, and horizontal alignment — shared by the label prim, the container box,
    /// and the focus/hover chrome so every consumer draws the same geometry.
    fn text_run_geometry(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        pane_w_px: i32,
        vpr: f64,
    ) -> (f64, f64, f64, DrawingTextHAlign) {
        let layout = &self.options.get().layout;
        let size = drawing.resolved_text_size(layout.font_size) * vpr;
        let pane = &self.panes[drawing.pane_index];
        let reference = ChartEngine::text_box(
            drawing.kind,
            px,
            f64::from(pane_w_px),
            pane.top * vpr,
            pane.height * vpr,
        );
        let (x, y, align) = ChartEngine::text_placement(drawing, &reference, size, TEXT_PAD * vpr);
        (size, x, y, align)
    }

    /// The text tool's interaction chrome (hover ring, focus border): a crisp integer-snapped
    /// hollow frame on the SAME box the host's editing wrap draws — the label run (advance ×
    /// 1.2·size, the hit test's line-height convention) padded by the editing chrome's
    /// 2 px border + 4 px padding (drawings.rs `TEXT_CHROME_PAD`). Selection, hover, and
    /// typing mode land on one outline, so entering/leaving the editor moves nothing.
    fn push_text_chrome(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        pane_w_px: i32,
        vpr: f64,
        color: Color,
        out: &mut Vec<Prim>,
    ) {
        let (size, x, y, align) = self.text_run_geometry(drawing, px, pane_w_px, vpr);
        let width = self.measure_drawing_text(drawing, size);
        let height = size * 1.2;
        let pad = TEXT_CHROME_PAD * vpr;
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
        out.push(Prim::RectFrame {
            rect,
            border: (2.0 * vpr).round().max(1.0) as i32,
            color,
        });
    }

    /// One drawing's text label (every tool can carry one): the placement resolves the 3×3
    /// alignment against the tool's reference box (drawings.rs `text_box`/`text_placement`),
    /// then emits a `Prim::Text` — x is the aligned edge, y the vertical center (the IR's
    /// middle-baseline convention), so the run rasterizes identically on both backends. Empty
    /// text (including an empty text tool) paints nothing — the host typing-mode editor is the
    /// only empty-state UI, and leaving it without typed text removes the drawing. A text tool
    /// with a `box_color`/`box_border_color` gets its container (crisp integer-snapped
    /// `Rect`/`RectFrame` prims behind the run — TradingView's text-box background/border).
    fn build_drawing_text(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        pane_w_px: i32,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        // Empty text paints nothing. While the host typing-mode editor is open the LABEL and
        // the focus border still paint — the editor wrap is borderless with transparent glyphs,
        // so entering edit cannot lift the text or shift the outline (TradingView's
        // overlay-caret model).
        if drawing.text.is_empty() {
            return;
        }
        let is_text_tool = drawing.kind == DrawingKind::Text;
        let (size, x, y, align) = self.text_run_geometry(drawing, px, pane_w_px, vpr);
        let layout = &self.options.get().layout;
        let color = drawing
            .text_color
            .as_deref()
            .and_then(Color::parse_css)
            .or_else(|| Color::parse_css(&layout.text_color))
            .unwrap_or_else(|| {
                let fallback = nucleuscharts_core::style::DEFAULT_FOREGROUND_RGB;
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
            weight: drawing.text_weight.unwrap_or(400),
            italic: drawing.text_italic,
        });
    }

    fn build_position_labels(
        &self,
        drawing: &Drawing,
        position: PositionGeometry,
        vpr: f64,
        reward: Color,
        risk: Color,
        out: &mut Vec<Prim>,
    ) {
        let (Some(entry), Some(target), Some(stop)) = (
            drawing.points.first(),
            drawing.points.get(1),
            drawing.points.get(2),
        ) else {
            return;
        };
        let reward_distance = (target.price - entry.price).abs();
        let risk_distance = (entry.price - stop.price).abs();
        let base = entry.price.abs();
        let reward_percent = if base > f64::EPSILON {
            reward_distance / base * 100.0
        } else {
            0.0
        };
        let risk_percent = if base > f64::EPSILON {
            risk_distance / base * 100.0
        } else {
            0.0
        };
        let ratio = (risk_distance > f64::EPSILON).then_some(reward_distance / risk_distance);
        let target_text = format!(
            "Target: {} ({reward_percent:.2}%)",
            self.price_formatter.format(reward_distance)
        );
        let ratio_text = ratio
            .map(|ratio| format!("Risk / reward ratio: {ratio:.2}"))
            .unwrap_or_else(|| "Risk / reward ratio: —".to_string());
        let summary_color = match drawing.kind {
            DrawingKind::LongPosition => reward,
            DrawingKind::ShortPosition => risk,
            _ => return,
        };
        let pnl_text = "Open PnL: 0.00, Qty: 0".to_string();
        let stop_text = format!(
            "Stop: {} ({risk_percent:.2}%)",
            self.price_formatter.format(risk_distance)
        );
        let center_x = (position.left + position.right) / 2.0;
        let label_offset = 14.0 * vpr;
        let target_label_y = if position.target_y < position.entry_y {
            position.target_y - label_offset
        } else {
            position.target_y + label_offset
        };
        let stop_label_y = if position.stop_y < position.entry_y {
            position.stop_y - label_offset
        } else {
            position.stop_y + label_offset
        };
        let summary_y = (position.target_y + position.stop_y) / 2.0;
        self.push_position_label_block(out, center_x, target_label_y, &[target_text], reward, vpr);
        self.push_position_label_block(
            out,
            center_x,
            summary_y,
            &[pnl_text, ratio_text],
            summary_color,
            vpr,
        );
        self.push_position_label_block(out, center_x, stop_label_y, &[stop_text], risk, vpr);
    }

    /// Dynamic position progress belongs to pane chrome rather than retained drawing geometry:
    /// series updates already invalidate chrome, so the darker traversed fill and current-price
    /// marker can follow live data without rebuilding every drawing on each tick.
    pub(super) fn build_position_progress_frame(
        &self,
        pane_index: usize,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
        hpr: f64,
        vpr: f64,
    ) {
        for drawing in self.drawings.iter().filter(|drawing| {
            drawing.pane_index == pane_index
                && matches!(
                    drawing.kind,
                    DrawingKind::LongPosition | DrawingKind::ShortPosition
                )
                && drawing.points.len() == 3
        }) {
            let Some(live_point) = self.position_live_point(drawing) else {
                continue;
            };
            let live_price = live_point.price;
            let entry = drawing.points[0].price;
            let target = drawing.points[1].price;
            let stop = drawing.points[2].price;
            let lower = target.min(stop);
            let upper = target.max(stop);
            if !live_price.is_finite() || !entry.is_finite() || lower >= upper {
                continue;
            }

            let progress_price = live_price.clamp(lower, upper);
            if (progress_price - entry).abs() <= f64::EPSILON {
                continue;
            }
            let Some(px) = self.drawing_px(drawing) else {
                continue;
            };
            let px = px
                .into_iter()
                .map(|(x, y)| (x * hpr, y * vpr))
                .collect::<Vec<_>>();
            let Some(geometry) = resolve_drawing_geometry(
                drawing.kind,
                &px,
                self.pane_w * hpr,
                self.panes[pane_index].top * vpr,
                self.panes[pane_index].height * vpr,
                drawing.width,
                vpr,
            ) else {
                continue;
            };
            let DrawingBodyGeometry::Position(position) = geometry.body else {
                continue;
            };
            let progress_point = crate::drawings::DrawingPoint {
                logical: drawing.points[0].logical,
                price: progress_price,
            };
            let Some((_, progress_y)) =
                self.drawing_to_px_for(pane_index, drawing.price_scale, progress_point)
            else {
                continue;
            };
            let progress_y = progress_y * vpr;
            let is_profit = match drawing.kind {
                DrawingKind::LongPosition => progress_price >= entry,
                DrawingKind::ShortPosition => progress_price <= entry,
                _ => false,
            };
            let semantic = if is_profit {
                Color::parse_css(nucleuscharts_core::style::MARKET_UP_CSS)
                    .unwrap_or(Color::rgb(8, 153, 129))
            } else {
                Color::parse_css(nucleuscharts_core::style::MARKET_DOWN_CSS)
                    .unwrap_or(Color::rgb(247, 82, 95))
            };

            // Layer a second translucent fill only over the traversed portion. The untouched
            // remainder keeps the base zone opacity, while progress reads progressively stronger.
            push_position_zone_with_alpha(
                out,
                PositionZone {
                    left: position.left,
                    right: position.right,
                    y0: position.entry_y,
                    y1: progress_y,
                },
                semantic,
                58,
            );

            // Follow the actual price path from the exact entry anchor to the latest plotted bar.
            // Once price leaves the position, clamp only its y coordinate to the completed target
            // or stop boundary; its x coordinate remains the real latest-bar location.
            let progress_point = crate::drawings::DrawingPoint {
                logical: live_point.logical,
                price: progress_price,
            };
            let Some((progress_x, _)) =
                self.drawing_to_px_for(pane_index, drawing.price_scale, progress_point)
            else {
                continue;
            };
            let progress_path = [
                [px[0].0 as f32, position.entry_y as f32],
                [(progress_x * hpr) as f32, progress_y as f32],
            ];
            if progress_path[0] != progress_path[1] {
                super::series_geometry::push_line_stroke(
                    out,
                    points,
                    &progress_path,
                    vpr.max(1.0) as f32,
                    LineStyle::Dotted,
                    LineType::Simple,
                    semantic.solid(),
                );
            }
        }
    }

    fn position_live_point(&self, drawing: &Drawing) -> Option<crate::drawings::DrawingPoint> {
        let target = match drawing.price_scale {
            crate::DrawingPriceScale::Right => crate::PriceScaleTarget::Right,
            crate::DrawingPriceScale::Left => crate::PriceScaleTarget::Left,
            crate::DrawingPriceScale::Overlay => crate::PriceScaleTarget::Overlay,
        };
        let series = self.series.iter().find(|series| {
            series.visible
                && !series.removed
                && series.pane_index == drawing.pane_index
                && super::series_scale_target(series) == target
        })?;
        if series.kind == crate::SeriesKind::Custom {
            let last = series.custom_frame.last?;
            let logical = self.time_to_index(last.time as f64, false)?;
            return last
                .value
                .is_finite()
                .then_some(crate::drawings::DrawingPoint {
                    logical: logical as f64,
                    price: last.value,
                });
        }
        let plot = self.data.plot(series.id);
        let row = plot.last_non_whitespace_row(i64::MAX)?;
        let logical = plot.index_at(row)?;
        let value = plot.value_at(row, PlotValueIndex::Close);
        value.is_finite().then_some(crate::drawings::DrawingPoint {
            logical: logical as f64,
            price: value,
        })
    }

    fn push_position_label_block(
        &self,
        out: &mut Vec<Prim>,
        x: f64,
        y: f64,
        lines: &[String],
        background: Color,
        vpr: f64,
    ) {
        if lines.is_empty() {
            return;
        }
        let layout = &self.options.get().layout;
        let size = (layout.font_size * 0.92).max(10.0) * vpr;
        let line_height = size * 1.25;
        let pad_x = 6.0 * vpr;
        let pad_y = 3.0 * vpr;
        let width = lines
            .iter()
            .map(|line| self.measure_text_run(line, size, &layout.font_family, 400, false))
            .fold(0.0_f64, f64::max)
            + 2.0 * pad_x;
        let height = lines.len() as f64 * line_height + 2.0 * pad_y;
        let rect = IRect {
            x: (x - width / 2.0).round() as i32,
            y: (y - height / 2.0).round() as i32,
            w: width.round().max(1.0) as i32,
            h: height.round().max(1.0) as i32,
        };
        out.push(Prim::Rect {
            rect,
            color: Color::rgba(background.r(), background.g(), background.b(), 224),
        });
        let text_color = if background.luminance() > 175.0 {
            Color::rgb(0, 0, 0)
        } else {
            Color::rgb(255, 255, 255)
        };
        let first_y = y - ((lines.len() as f64 - 1.0) * line_height) / 2.0;
        for (index, text) in lines.iter().enumerate() {
            out.push(Prim::Text {
                x: x as f32,
                y: (first_y + index as f64 * line_height) as f32,
                text: text.clone(),
                color: text_color,
                size: size as f32,
                family: layout.font_family.clone(),
                align: TextAlign::Center,
                weight: 400,
                italic: false,
            });
        }
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

fn push_position_zone(out: &mut Vec<Prim>, zone: PositionZone, color: Color) {
    push_position_zone_with_alpha(out, zone, color, 70);
}

fn push_position_zone_with_alpha(out: &mut Vec<Prim>, zone: PositionZone, color: Color, alpha: u8) {
    let left = zone.left.round() as i32;
    let right = zone.right.round() as i32;
    let top = zone.y0.min(zone.y1).round() as i32;
    let bottom = zone.y0.max(zone.y1).round() as i32;
    let width = (right - left).abs() + 1;
    let height = (bottom - top).abs() + 1;
    let rect = IRect {
        x: left,
        y: top,
        w: width,
        h: height,
    };
    out.push(Prim::Rect {
        rect,
        color: Color::rgba(color.r(), color.g(), color.b(), alpha),
    });
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

/// Long/Short Position selection controls. The controls correspond to target, entry/origin,
/// horizontal extent, and stop; they are intentionally not generic drawing-point anchors.
fn build_position_handles(px: &[(f64, f64)], vpr: f64, fill: Color, out: &mut Vec<Prim>) {
    if px.len() != 3 {
        return;
    }
    let entry = px[0];
    let target = px[1];
    let stop = px[2];
    let controls = [
        (entry.0, target.1, false),
        (entry.0, entry.1, true),
        (target.0, entry.1, false),
        (entry.0, stop.1, false),
    ];
    let outer = ((ANCHOR_RADIUS + ANCHOR_BORDER_WIDTH) * vpr) as f32;
    let inner = (ANCHOR_RADIUS * vpr) as f32;
    for (cx, cy, circular) in controls {
        if circular {
            out.push(Prim::Circle {
                cx: cx as f32,
                cy: cy as f32,
                radius: outer,
                fill: ANCHOR_BORDER,
                stroke_width: 0.0,
                stroke: ANCHOR_BORDER,
            });
            out.push(Prim::Circle {
                cx: cx as f32,
                cy: cy as f32,
                radius: inner,
                fill,
                stroke_width: 0.0,
                stroke: fill,
            });
        } else {
            let side = (2.0 * (ANCHOR_RADIUS + ANCHOR_BORDER_WIDTH) * vpr)
                .round()
                .max(1.0) as i32;
            let inner_side = (2.0 * ANCHOR_RADIUS * vpr).round().max(1.0) as i32;
            out.push(Prim::Rect {
                rect: IRect {
                    x: (cx - f64::from(side) / 2.0).round() as i32,
                    y: (cy - f64::from(side) / 2.0).round() as i32,
                    w: side,
                    h: side,
                },
                color: ANCHOR_BORDER,
            });
            out.push(Prim::Rect {
                rect: IRect {
                    x: (cx - f64::from(inner_side) / 2.0).round() as i32,
                    y: (cy - f64::from(inner_side) / 2.0).round() as i32,
                    w: inner_side,
                    h: inner_side,
                },
                color: fill,
            });
        }
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
