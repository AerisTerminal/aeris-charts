//! Frame emission for study-owned semantic scene objects (lines, boxes, polylines,
//! labels, and viewport-anchored dashboard tables).
//!
//! Study-owned scene objects are stored in `ChartEngine::external_study_scenes`, kept
//! strictly separate from user drawings, and emitted into the main pane's `DrawList`
//! in deterministic study/object order.

use crate::ChartEngine;
use crate::external_studies::{
    ExternalStudyFill, ExternalStudyLevel, ExternalStudyLineStyle, ExternalStudySceneBox,
    ExternalStudySceneLabel, ExternalStudySceneLine, ExternalStudySceneObject,
    ExternalStudyScenePolyline, ExternalStudySceneTable, ExternalStudyTablePosition,
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
        if pane_index != 0
            || (self.external_study_scenes.is_empty() && self.external_study_overlays.is_empty())
        {
            return;
        }
        let Some(pane) = self.panes.get(pane_index) else {
            return;
        };
        let pane_width = self.time_scale.width().max(1.0);
        let pane_height = pane.height.max(1.0);
        let font_family = self.options.get().layout.font_family.clone();
        let font_size = self.options.get().layout.font_size.max(10.0);

        for (&study_id, overlay) in &self.external_study_overlays {
            if self.external_study_visible(study_id) == Some(false) {
                continue;
            }
            self.emit_study_background_colors(
                &overlay.background_colors,
                pane_width,
                pane_height,
                hpr,
                vpr,
                prims,
            );
            for fill in &overlay.fills {
                self.emit_study_fill(study_id, *fill, pane_width, hpr, vpr, prims, points);
            }
            for level in &overlay.levels {
                self.emit_study_level(level, pane_width, &font_family, font_size, hpr, vpr, prims);
            }
        }

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
                        self.emit_study_scene_box(rect, pane_width, hpr, vpr, prims);
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

    fn study_price_to_y(&self, price: f64) -> Option<f64> {
        if !price.is_finite() {
            return None;
        }
        if let Some(y) = self.series_price_to_coordinate(0, price) {
            return Some(y);
        }
        let scale = self.price_scale_for(0, crate::PriceScaleTarget::Right)?;
        if scale.is_empty() {
            return None;
        }
        Some(scale.price_to_coordinate(price, 0.0))
    }

    fn study_time_to_x(&self, unix_nanos: i64) -> Option<f64> {
        let time_sec = (unix_nanos as f64) / (NANOS_PER_SECOND as f64);
        if !time_sec.is_finite() {
            return None;
        }
        // Exact integer-second fast path for historical bar anchors.
        if unix_nanos.rem_euclid(NANOS_PER_SECOND) == 0
            && let Some(x) = self.time_to_coordinate(time_sec)
            && x.is_finite()
        {
            return Some(x);
        }
        let times = self.data.merged_times();
        if times.is_empty() {
            return None;
        }
        let first_time = times[0] as f64;
        let last_idx = times.len() - 1;
        let last_time = times[last_idx] as f64;
        let spacing = self.time_scale.bar_spacing();
        if !spacing.is_finite() || spacing <= 0.0 {
            return None;
        }

        // Compute robust bar step (median of up to last 9 positive intervals) for out-of-range projection.
        let mut step_sec = 60.0_f64;
        if times.len() >= 2 {
            let mut deltas = [0.0_f64; 9];
            let mut count = 0usize;
            let start = times.len().saturating_sub(10);
            for i in (start + 1)..times.len() {
                let d = (times[i] - times[i - 1]) as f64;
                if d > 0.0 && d.is_finite() {
                    deltas[count] = d;
                    count += 1;
                }
            }
            if count > 0 {
                let slice = &mut deltas[..count];
                slice.sort_by(f64::total_cmp);
                step_sec = slice[count / 2].max(1.0);
            }
        }

        if time_sec >= last_time {
            let last_x = self.time_scale.index_to_coordinate(last_idx as i64);
            let offset_bars = (time_sec - last_time) / step_sec;
            let x = last_x + offset_bars * spacing;
            return x.is_finite().then_some(x);
        }
        if time_sec <= first_time {
            let first_x = self.time_scale.index_to_coordinate(0);
            let offset_bars = (first_time - time_sec) / step_sec;
            let x = first_x - offset_bars * spacing;
            return x.is_finite().then_some(x);
        }

        // Between two existing bars: linearly interpolate between neighboring bar coordinates.
        let target_floor = time_sec.floor() as i64;
        let pos = times.partition_point(|&t| t < target_floor);
        if pos == 0 {
            let x = self.time_scale.index_to_coordinate(0);
            return x.is_finite().then_some(x);
        }
        let right_idx = pos.min(last_idx);
        let left_idx = right_idx.saturating_sub(1);
        let t0 = times[left_idx] as f64;
        let t1 = times[right_idx] as f64;
        let x0 = self.time_scale.index_to_coordinate(left_idx as i64);
        let x1 = self.time_scale.index_to_coordinate(right_idx as i64);
        let frac = if t1 > t0 {
            ((time_sec - t0) / (t1 - t0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let x = x0 + (x1 - x0) * frac;
        x.is_finite().then_some(x)
    }

    fn study_anchor_to_px(
        &self,
        unix_nanos: i64,
        price: f64,
        hpr: f64,
        vpr: f64,
    ) -> Option<[f32; 2]> {
        let x = self.study_time_to_x(unix_nanos)?;
        let y = self.study_price_to_y(price)?;
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        Some([(x * hpr) as f32, (y * vpr) as f32])
    }

    fn emit_study_background_colors(
        &self,
        background_colors: &[(i64, u32)],
        pane_width: f64,
        pane_height: f64,
        hpr: f64,
        vpr: f64,
        prims: &mut Vec<Prim>,
    ) {
        let half_bar = (self.time_scale.bar_spacing() * 0.5 * hpr).max(1.0);
        let max_x = pane_width * hpr + half_bar * 2.0;
        let min_x = -half_bar * 2.0;
        let h = (pane_height * vpr).round().max(1.0) as i32;
        for &(ts_ns, color_rgba) in background_colors {
            if color_rgba == 0 {
                continue;
            }
            let time_sec = (ts_ns.div_euclid(NANOS_PER_SECOND)) as f64;
            let Some(x) = self.time_to_coordinate(time_sec) else {
                continue;
            };
            if !x.is_finite() {
                continue;
            }
            let cx = x * hpr;
            if cx < min_x || cx > max_x {
                continue;
            }
            prims.push(Prim::Rect {
                rect: IRect {
                    x: (cx - half_bar).round() as i32,
                    y: 0,
                    w: (half_bar * 2.0).round().max(1.0) as i32,
                    h,
                },
                color: unpack_rgba(color_rgba),
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_study_level(
        &self,
        level: &ExternalStudyLevel,
        pane_width: f64,
        font_family: &str,
        font_size: f64,
        hpr: f64,
        vpr: f64,
        prims: &mut Vec<Prim>,
    ) {
        let Some(y) = self.study_price_to_y(level.value) else {
            return;
        };
        if !y.is_finite() {
            return;
        }
        let y_px = (y * vpr).round() as i32;
        let x1 = (pane_width * hpr).round().max(1.0) as i32;
        let style = match level.style {
            ExternalStudyLineStyle::Solid => LineStyle::Solid,
            ExternalStudyLineStyle::Dashed => LineStyle::Dashed,
            ExternalStudyLineStyle::Dotted => LineStyle::Dotted,
        };
        let color = unpack_rgba(level.color_rgba);
        prims.push(Prim::HLine {
            y: y_px,
            x0: 0,
            x1,
            width: (1.0 * vpr).round().max(1.0) as i32,
            style,
            color,
        });
        if let Some(title) = level.title.as_deref()
            && !title.is_empty()
        {
            prims.push(Prim::Text {
                x: (x1 as f32 - (8.0 * hpr) as f32).max(0.0),
                y: y_px as f32 - (8.0 * vpr) as f32,
                text: title.to_owned(),
                color,
                size: ((font_size * 0.88).max(9.0) * vpr) as f32,
                family: font_family.to_owned(),
                align: TextAlign::Right,
                weight: 500,
                italic: false,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_study_fill(
        &self,
        study_id: u64,
        fill: ExternalStudyFill,
        pane_width: f64,
        hpr: f64,
        vpr: f64,
        prims: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let (Some(first), Some(second)) = (
            self.external_study_output_info(study_id, fill.first_output_index),
            self.external_study_output_info(study_id, fill.second_output_index),
        ) else {
            return;
        };
        let d1 = self.series_data(first.series_id);
        let d2 = self.series_data(second.series_id);
        if d1.len() < 2 || d2.len() < 2 {
            return;
        }

        let margin_px = (self.time_scale.bar_spacing() * 3.0 * hpr).max(32.0);
        let min_x_px = (-margin_px) as f32;
        let max_x_px = (pane_width * hpr + margin_px) as f32;
        let fill_color = unpack_rgba(fill.color_rgba);

        let flush_segment = |upper_pts: &mut Vec<[f32; 2]>,
                             lower_pts: &mut Vec<[f32; 2]>,
                             prims: &mut Vec<Prim>,
                             points: &mut Vec<[f32; 2]>| {
            if upper_pts.len() >= 2 {
                let upper_first = points.len() as u32;
                points.extend_from_slice(upper_pts);
                let lower_first = points.len() as u32;
                points.extend_from_slice(lower_pts);
                prims.push(Prim::BandFill {
                    upper_first,
                    lower_first,
                    point_count: upper_pts.len() as u32,
                    line_type: LineType::Simple,
                    fill: fill_color,
                });
            }
            upper_pts.clear();
            lower_pts.clear();
        };

        let mut upper_pts: Vec<[f32; 2]> = Vec::new();
        let mut lower_pts: Vec<[f32; 2]> = Vec::new();
        let mut i1 = 0usize;
        let mut i2 = 0usize;

        // Strict timestamp merge: any missing timestamp or non-finite value on either side
        // splits into separate contiguous BandFill polygons so fills never bridge across gaps.
        while i1 < d1.len() && i2 < d2.len() {
            let p1 = &d1[i1];
            let p2 = &d2[i2];
            match p1.time.cmp(&p2.time) {
                std::cmp::Ordering::Less => {
                    flush_segment(&mut upper_pts, &mut lower_pts, prims, points);
                    i1 += 1;
                }
                std::cmp::Ordering::Greater => {
                    flush_segment(&mut upper_pts, &mut lower_pts, prims, points);
                    i2 += 1;
                }
                std::cmp::Ordering::Equal => {
                    i1 += 1;
                    i2 += 1;
                    if !p1.close.is_finite() || !p2.close.is_finite() {
                        flush_segment(&mut upper_pts, &mut lower_pts, prims, points);
                        continue;
                    }
                    let Some(x) = self.time_to_coordinate(p1.time as f64) else {
                        flush_segment(&mut upper_pts, &mut lower_pts, prims, points);
                        continue;
                    };
                    if !x.is_finite() {
                        flush_segment(&mut upper_pts, &mut lower_pts, prims, points);
                        continue;
                    }
                    let x_px = (x * hpr) as f32;
                    if x_px < min_x_px {
                        continue;
                    }
                    if x_px > max_x_px {
                        if let (Some(y1), Some(y2)) = (
                            self.series_price_to_coordinate(first.series_id, p1.close),
                            self.series_price_to_coordinate(second.series_id, p2.close),
                        ) && y1.is_finite()
                            && y2.is_finite()
                            && !upper_pts.is_empty()
                        {
                            upper_pts.push([x_px, (y1 * vpr) as f32]);
                            lower_pts.push([x_px, (y2 * vpr) as f32]);
                        }
                        break;
                    }
                    let (Some(y1), Some(y2)) = (
                        self.series_price_to_coordinate(first.series_id, p1.close),
                        self.series_price_to_coordinate(second.series_id, p2.close),
                    ) else {
                        flush_segment(&mut upper_pts, &mut lower_pts, prims, points);
                        continue;
                    };
                    if y1.is_finite() && y2.is_finite() {
                        upper_pts.push([x_px, (y1 * vpr) as f32]);
                        lower_pts.push([x_px, (y2 * vpr) as f32]);
                    } else {
                        flush_segment(&mut upper_pts, &mut lower_pts, prims, points);
                    }
                }
            }
        }
        flush_segment(&mut upper_pts, &mut lower_pts, prims, points);
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
        if !line.extend_left && !line.extend_right {
            let min_lx = p1[0].min(p2[0]);
            let max_lx = p1[0].max(p2[0]);
            if max_lx < -4.0 || min_lx > max_x + 4.0 {
                return;
            }
        }
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
        pane_width: f64,
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
        let min_x = p1[0].min(p2[0]);
        let max_x = p1[0].max(p2[0]);
        let pane_max_x = (pane_width * hpr) as f32;
        if max_x < -4.0 || min_x > pane_max_x + 4.0 {
            return;
        }
        let x0 = min_x.round() as i32;
        let y0 = p1[1].min(p2[1]).round() as i32;
        let x1 = max_x.round() as i32;
        let y1 = p1[1].max(p2[1]).round() as i32;
        let irect = IRect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(1),
            h: (y1 - y0).max(1),
        };
        if let Some(fill) = rect.fill_color_rgba
            && (fill & 0xFF) != 0
        {
            prims.push(Prim::Rect {
                rect: irect,
                color: unpack_rgba(fill),
            });
        }
        if let Some(border) = rect.border_color_rgba
            && (border & 0xFF) != 0
            && rect.border_width > 0
        {
            prims.push(Prim::RectFrame {
                rect: irect,
                border: (f64::from(rect.border_width) * hpr).round().max(1.0) as i32,
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
    let text_size = ((font_size * 0.88).max(9.5) * vpr) as f32;
    let pad_x = (8.0 * hpr) as f32;
    let min_col_w = (58.0 * hpr) as f32;
    let max_col_w = (190.0 * hpr) as f32;
    let mut col_widths = vec![min_col_w; cols];
    for cell in &table.cells {
        let c = usize::from(cell.col);
        if c < cols {
            let est = (cell.text.chars().count() as f32) * text_size * 0.58 + pad_x * 2.0;
            col_widths[c] = col_widths[c].max(est.clamp(min_col_w, max_col_w));
        }
    }
    let mut col_offsets = Vec::with_capacity(cols);
    let mut total_w = 0.0_f32;
    for &w in &col_widths {
        col_offsets.push(total_w);
        total_w += w;
    }
    let cell_h = (20.0 * vpr) as f32;
    let margin_x = (12.0 * hpr) as f32;
    let margin_y = (12.0 * vpr) as f32;
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
        let r = usize::from(cell.row);
        let c = usize::from(cell.col);
        if r >= rows || c >= cols {
            continue;
        }
        let cw = col_widths[c];
        let cx = origin_x + col_offsets[c];
        let cy = origin_y + (r as f32) * cell_h;
        let cell_rect = IRect {
            x: cx.round() as i32,
            y: cy.round() as i32,
            w: cw.round().max(1.0) as i32,
            h: cell_h.round().max(1.0) as i32,
        };
        if let Some(bg) = cell.background_color_rgba
            && (bg & 0xFF) != 0
        {
            prims.push(Prim::Rect {
                rect: cell_rect,
                color: unpack_rgba(bg),
            });
        }
        if let Some(border) = table.border_color_rgba
            && (border & 0xFF) != 0
        {
            prims.push(Prim::RectFrame {
                rect: cell_rect,
                border: (1.0 * hpr).round().max(1.0) as i32,
                color: unpack_rgba(border),
            });
        }
        if !cell.text.is_empty() {
            let (tx, align) = if cols == 2 && c == 0 {
                (cx + pad_x, TextAlign::Left)
            } else if cols == 2 && c == 1 {
                (cx + cw - pad_x, TextAlign::Right)
            } else {
                (cx + cw * 0.5, TextAlign::Center)
            };
            prims.push(Prim::Text {
                x: tx,
                y: cy + cell_h * 0.5,
                text: cell.text.clone(),
                color: unpack_rgba(cell.text_color_rgba.unwrap_or(0xE2E8_F0FF)),
                size: text_size,
                family: font_family.to_owned(),
                align,
                weight: 500,
                italic: false,
            });
        }
    }
}
