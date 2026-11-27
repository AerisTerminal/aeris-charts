//! Frame emission for study-owned semantic scene objects (lines, boxes, polylines,
//! labels, and viewport-anchored dashboard tables).
//!
//! Study-owned scene objects are stored in `ChartEngine::external_study_scenes`, kept
//! strictly separate from user drawings, and emitted into the main pane's `DrawList`
//! in deterministic study/object order.

use crate::ChartEngine;
use crate::external_studies::{
    ExternalStudySceneBox, ExternalStudySceneLabel, ExternalStudySceneLine,
    ExternalStudySceneObject, ExternalStudyScenePolyline, ExternalStudySceneTable,
    ExternalStudyTablePosition,
};
use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{IRect, LineStyle, LineType, Prim, TextAlign};

const NANOS_PER_SECOND: i64 = 1_000_000_000;

fn unpack_rgba(packed: u32) -> Color {
    Color::rgba(
        ((packed >> 24) & 0xFF) as u8,
        ((packed >> 16) & 0xFF) as u8,
        ((packed >> 8) & 0xFF) as u8,
        (packed & 0xFF) as u8,
    )
}

impl ChartEngine {
    pub(super) fn emit_external_study_scenes(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        prims: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        if pane_index != 0 || self.external_study_scenes.is_empty() {
            return;
        }
        let Some(pane) = self.panes.get(pane_index) else {
            return;
        };
        let pane_width = self.time_scale.width().max(1.0);
        let pane_height = pane.height.max(1.0);
        let font_family = self.options.get().layout.font_family.clone();
        let font_size = self.options.get().layout.font_size.max(10.0);

        for (&study_id, scene) in &self.external_study_scenes {
            if self.external_study_visible(study_id) == Some(false) {
                continue;
            }
            for object in scene.values() {
                match object {
                    ExternalStudySceneObject::Line(line) => {
                        self.emit_study_scene_line(line, pane_width, hpr, vpr, prims, points);
                    }
                    ExternalStudySceneObject::Box(rect) => {
                        self.emit_study_scene_box(rect, hpr, vpr, prims);
                    }
                    ExternalStudySceneObject::Polyline(poly) => {
                        self.emit_study_scene_polyline(poly, hpr, vpr, prims, points);
                    }
                    ExternalStudySceneObject::Label(label) => {
                        self.emit_study_scene_label(
                            label,
                            &font_family,
                            font_size,
                            hpr,
                            vpr,
                            prims,
                        );
                    }
                    ExternalStudySceneObject::Table(table) => {
                        emit_study_scene_table(
                            table,
                            pane_width,
                            pane_height,
                            &font_family,
                            font_size,
                            hpr,
                            vpr,
                            prims,
                        );
                    }
                }
            }
        }
    }

    fn study_anchor_to_px(
        &self,
        unix_nanos: i64,
        price: f64,
        hpr: f64,
        vpr: f64,
    ) -> Option<[f32; 2]> {
        let time_sec = (unix_nanos.div_euclid(NANOS_PER_SECOND)) as f64;
        let x = self.time_to_coordinate(time_sec)?;
        let y = self.series_price_to_coordinate(0, price)?;
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        Some([(x * hpr) as f32, (y * vpr) as f32])
    }

    fn emit_study_scene_line(
        &self,
        line: &ExternalStudySceneLine,
        pane_width: f64,
        hpr: f64,
        vpr: f64,
        prims: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let (Some(mut p1), Some(mut p2)) = (
            self.study_anchor_to_px(line.x1_unix_nanos, line.y1, hpr, vpr),
            self.study_anchor_to_px(line.x2_unix_nanos, line.y2, hpr, vpr),
        ) else {
            return;
        };
        let dx = p2[0] - p1[0];
        let dy = p2[1] - p1[1];
        let max_x = (pane_width * hpr) as f32;
        if dx.abs() > 1e-4 {
            let slope = dy / dx;
            if line.extend_left {
                let target_x = 0.0_f32;
                let target_y = p1[1] + (target_x - p1[0]) * slope;
                if dx > 0.0 {
                    p1 = [target_x, target_y];
                } else {
                    p2 = [target_x, target_y];
                }
            }
            if line.extend_right {
                let target_x = max_x;
                let target_y = p1[1] + (target_x - p1[0]) * slope;
                if dx > 0.0 {
                    p2 = [target_x, target_y];
                } else {
                    p1 = [target_x, target_y];
                }
            }
        }
        let first_point = points.len() as u32;
        points.push(p1);
        points.push(p2);
        prims.push(Prim::Polyline {
            first_point,
            point_count: 2,
            width: (f64::from(line.width.max(1)) * hpr) as f32,
            style: LineStyle::Solid,
            line_type: LineType::Simple,
            color: unpack_rgba(line.color_rgba),
        });
    }

    fn emit_study_scene_box(
        &self,
        rect: &ExternalStudySceneBox,
        hpr: f64,
        vpr: f64,
        prims: &mut Vec<Prim>,
    ) {
        let (Some(p1), Some(p2)) = (
            self.study_anchor_to_px(rect.left_unix_nanos, rect.top, hpr, vpr),
            self.study_anchor_to_px(rect.right_unix_nanos, rect.bottom, hpr, vpr),
        ) else {
            return;
        };
        let x0 = p1[0].min(p2[0]).round() as i32;
        let y0 = p1[1].min(p2[1]).round() as i32;
        let x1 = p1[0].max(p2[0]).round() as i32;
        let y1 = p1[1].max(p2[1]).round() as i32;
        let irect = IRect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(1),
            h: (y1 - y0).max(1),
        };
        if let Some(fill) = rect.fill_color_rgba {
            prims.push(Prim::Rect {
                rect: irect,
                color: unpack_rgba(fill),
            });
        }
        if let Some(border) = rect.border_color_rgba {
            prims.push(Prim::RectFrame {
                rect: irect,
                border: (f64::from(rect.border_width.max(1)) * hpr).round().max(1.0) as i32,
                color: unpack_rgba(border),
            });
        }
    }

    fn emit_study_scene_polyline(
        &self,
        poly: &ExternalStudyScenePolyline,
        hpr: f64,
        vpr: f64,
        prims: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        if poly.points.len() < 2 {
            return;
        }
        let first_point = points.len() as u32;
        let mut count = 0_u32;
        let mut first_px = None;
        for &(ts, price) in &poly.points {
            if let Some(pt) = self.study_anchor_to_px(ts, price, hpr, vpr) {
                if first_px.is_none() {
                    first_px = Some(pt);
                }
                points.push(pt);
                count += 1;
            }
        }
        if count < 2 {
            points.truncate(first_point as usize);
            return;
        }
        if poly.closed
            && let Some(first) = first_px
        {
            points.push(first);
            count += 1;
        }
        prims.push(Prim::Polyline {
            first_point,
            point_count: count,
            width: (f64::from(poly.width.max(1)) * hpr) as f32,
            style: LineStyle::Solid,
            line_type: LineType::Simple,
            color: unpack_rgba(poly.line_color_rgba),
        });
    }

    fn emit_study_scene_label(
        &self,
        label: &ExternalStudySceneLabel,
        font_family: &str,
        font_size: f64,
        hpr: f64,
        vpr: f64,
        prims: &mut Vec<Prim>,
    ) {
        let Some(anchor) = self.study_anchor_to_px(label.x_unix_nanos, label.y, hpr, vpr) else {
            return;
        };
        let text_size = (font_size * vpr) as f32;
        let pad_x = (6.0 * hpr) as f32;
        let pad_y = (3.0 * vpr) as f32;
        let est_w = (label.text.chars().count().max(1) as f32) * text_size * 0.6 + pad_x * 2.0;
        let est_h = text_size + pad_y * 2.0;
        if let Some(bg) = label.background_color_rgba {
            let radius = (4.0 * hpr) as f32;
            prims.push(Prim::RoundRect {
                x: anchor[0] - est_w * 0.5,
                y: anchor[1] - est_h * 0.5,
                w: est_w,
                h: est_h,
                radii: [radius; 4],
                fill: unpack_rgba(bg),
                border_width: 0.0,
                border_color: Color::rgba(0, 0, 0, 0),
            });
        }
        if !label.text.is_empty() {
            prims.push(Prim::Text {
                x: anchor[0],
                y: anchor[1],
                text: label.text.clone(),
                color: unpack_rgba(label.text_color_rgba),
                size: text_size,
                family: font_family.to_owned(),
                align: TextAlign::Center,
                weight: 500,
                italic: false,
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_study_scene_table(
    table: &ExternalStudySceneTable,
    pane_width: f64,
    pane_height: f64,
    font_family: &str,
    font_size: f64,
    hpr: f64,
    vpr: f64,
    prims: &mut Vec<Prim>,
) {
    let rows = usize::from(table.rows);
    let cols = usize::from(table.cols);
    if rows == 0 || cols == 0 || table.cells.is_empty() {
        return;
    }
    let text_size = ((font_size * 0.92).max(10.0) * vpr) as f32;
    let cell_w = (88.0 * hpr) as f32;
    let cell_h = (22.0 * vpr) as f32;
    let margin_x = (12.0 * hpr) as f32;
    let margin_y = (12.0 * vpr) as f32;
    let total_w = cell_w * cols as f32;
    let total_h = cell_h * rows as f32;
    let pw = (pane_width * hpr) as f32;
    let ph = (pane_height * vpr) as f32;

    let origin_x = match table.position {
        ExternalStudyTablePosition::TopLeft
        | ExternalStudyTablePosition::MiddleLeft
        | ExternalStudyTablePosition::BottomLeft => margin_x,
        ExternalStudyTablePosition::TopCenter
        | ExternalStudyTablePosition::MiddleCenter
        | ExternalStudyTablePosition::BottomCenter => ((pw - total_w) * 0.5).max(margin_x),
        ExternalStudyTablePosition::TopRight
        | ExternalStudyTablePosition::MiddleRight
        | ExternalStudyTablePosition::BottomRight => (pw - total_w - margin_x).max(margin_x),
    };
    let origin_y = match table.position {
        ExternalStudyTablePosition::TopLeft
        | ExternalStudyTablePosition::TopCenter
        | ExternalStudyTablePosition::TopRight => margin_y,
        ExternalStudyTablePosition::MiddleLeft
        | ExternalStudyTablePosition::MiddleCenter
        | ExternalStudyTablePosition::MiddleRight => ((ph - total_h) * 0.5).max(margin_y),
        ExternalStudyTablePosition::BottomLeft
        | ExternalStudyTablePosition::BottomCenter
        | ExternalStudyTablePosition::BottomRight => (ph - total_h - margin_y).max(margin_y),
    };

    if let Some(bg) = table.background_color_rgba {
        prims.push(Prim::Rect {
            rect: IRect {
                x: origin_x.round() as i32,
                y: origin_y.round() as i32,
                w: total_w.round().max(1.0) as i32,
                h: total_h.round().max(1.0) as i32,
            },
            color: unpack_rgba(bg),
        });
    }

    for cell in &table.cells {
        if usize::from(cell.row) >= rows || usize::from(cell.col) >= cols {
            continue;
        }
        let cx = origin_x + f32::from(cell.col) * cell_w;
        let cy = origin_y + f32::from(cell.row) * cell_h;
        let cell_rect = IRect {
            x: cx.round() as i32,
            y: cy.round() as i32,
            w: cell_w.round().max(1.0) as i32,
            h: cell_h.round().max(1.0) as i32,
        };
        if let Some(bg) = cell.background_color_rgba {
            prims.push(Prim::Rect {
                rect: cell_rect,
                color: unpack_rgba(bg),
            });
        }
        if let Some(border) = table.border_color_rgba {
            prims.push(Prim::RectFrame {
                rect: cell_rect,
                border: (1.0 * hpr).round().max(1.0) as i32,
                color: unpack_rgba(border),
            });
        }
        if !cell.text.is_empty() {
            prims.push(Prim::Text {
                x: cx + cell_w * 0.5,
                y: cy + cell_h * 0.5,
                text: cell.text.clone(),
                color: unpack_rgba(cell.text_color_rgba.unwrap_or(0xE2E8_F0FF)),
                size: text_size,
                family: font_family.to_owned(),
                align: TextAlign::Center,
                weight: 500,
                italic: false,
            });
        }
    }
}
