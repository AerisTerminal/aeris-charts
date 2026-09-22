use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::{Gradient, IRect, LineStyle, LineType, Prim, TextAlign};

use crate::{ChartEngine, GeneralSeriesKind, DEFAULT_LINE_COLOR};

use super::PRIMARY;

const GENERAL_HOVER: Color = Color(PRIMARY.0 & 0xFFFF_FF00 | 0x73);
const GENERAL_LINE_WIDTH_CSS: f64 = 2.0;
const MAX_GENERAL_DATA_LABELS_PER_PANE: usize = 512;
const MAX_GENERAL_DATA_LABEL_ATTEMPTS_PER_PANE: usize = 4_096;

impl ChartEngine {
    pub(super) fn build_general_series_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) -> [Option<Prim>; 2] {
        let mut interaction = [None, None];
        let Some(pane_id) = self.pane_stable_id(pane_index) else {
            return interaction;
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return interaction;
        };
        let layout = &self.options.get().layout;
        let label_size = layout.font_size.max(1.0);
        let label_color = self.primary_text_color();
        let mut occupied_labels: Vec<[f64; 4]> = Vec::new();
        let mut label_primitives = Vec::new();
        let mut label_attempts = 0;
        for series in self
            .general_series_iter()
            .filter(|series| series.visible() && series.pane_id() == pane_id)
        {
            let Some(dataset) = self.general_dataset(series.dataset()) else {
                continue;
            };
            let mut push_label = |row: usize, x: f64, above: f64, below: f64, width_limit: f64| {
                if !series.data_labels()
                    || occupied_labels.len() >= MAX_GENERAL_DATA_LABELS_PER_PANE
                    || label_attempts >= MAX_GENERAL_DATA_LABEL_ATTEMPTS_PER_PANE
                {
                    return;
                }
                label_attempts += 1;
                let text = dataset
                    .row_label(row)
                    .map(str::to_owned)
                    .unwrap_or_else(|| dataset.y()[row].to_string());
                let width =
                    self.measure_text_run(&text, label_size, &layout.font_family, 400, false);
                if !width.is_finite() || width <= 0.0 || width > width_limit {
                    return;
                }
                let left = x - width * 0.5;
                let right = x + width * 0.5;
                if left < 2.0 || right > plot.width - 2.0 {
                    return;
                }
                for y in [above, below] {
                    let top = y - label_size * 0.55;
                    let bottom = y + label_size * 0.55;
                    if top < plot.y + 2.0 || bottom > plot.y + plot.height - 2.0 {
                        continue;
                    }
                    let rect = [left, top, right, bottom];
                    if occupied_labels.iter().any(|other| {
                        rect[0] < other[2] + 2.0
                            && rect[2] > other[0] - 2.0
                            && rect[1] < other[3] + 2.0
                            && rect[3] > other[1] - 2.0
                    }) {
                        continue;
                    }
                    occupied_labels.push(rect);
                    label_primitives.push(Prim::Text {
                        x: (x * hpr) as f32,
                        y: (y * vpr) as f32,
                        text,
                        color: label_color,
                        size: (label_size * vpr) as f32,
                        family: layout.font_family.clone(),
                        align: TextAlign::Center,
                        weight: 400,
                        italic: false,
                    });
                    break;
                }
            };
            match series.kind() {
                GeneralSeriesKind::XyLine | GeneralSeriesKind::XyArea => {
                    let color = series
                        .color()
                        .and_then(Color::parse_css)
                        .unwrap_or(DEFAULT_LINE_COLOR);
                    let is_area = series.kind() == GeneralSeriesKind::XyArea;
                    let baseline_y = is_area
                        .then(|| self.general_path_baseline_y(series))
                        .flatten();
                    let mut run = Vec::<[f32; 2]>::new();
                    let mut flush_run = |run: &mut Vec<[f32; 2]>| {
                        if run.len() >= 2 {
                            let first_point = points.len() as u32;
                            let point_count = run.len() as u32;
                            points.append(run);
                            if let Some(base_y) = baseline_y {
                                out.push(Prim::AreaFill {
                                    first_point,
                                    point_count,
                                    base_y: (base_y * vpr) as f32,
                                    line_type: LineType::Simple,
                                    gradient: Gradient {
                                        top: Color::rgba(color.r(), color.g(), color.b(), 72),
                                        bottom: Color::rgba(color.r(), color.g(), color.b(), 24),
                                    },
                                });
                            }
                            out.push(Prim::Polyline {
                                first_point,
                                point_count,
                                width: (GENERAL_LINE_WIDTH_CSS * vpr) as f32,
                                style: LineStyle::Solid,
                                line_type: LineType::Simple,
                                color,
                            });
                        } else {
                            run.clear();
                        }
                    };
                    self.visit_general_path_points(series, |geometry| {
                        if geometry.starts_new_run {
                            flush_run(&mut run);
                        }
                        run.push([(geometry.x * hpr) as f32, (geometry.y * vpr) as f32]);
                        push_label(
                            geometry.row,
                            geometry.x,
                            geometry.y - label_size * 0.65 - 4.0,
                            geometry.y + label_size * 0.65 + 4.0,
                            plot.width - 4.0,
                        );
                        let (hovered, selected) =
                            self.general_row_interaction(series.id(), geometry.row);
                        if hovered || selected {
                            let stroke = if selected { PRIMARY } else { GENERAL_HOVER };
                            interaction[usize::from(selected)] = Some(Prim::Circle {
                                cx: (geometry.x * hpr) as f32,
                                cy: (geometry.y * vpr) as f32,
                                radius: ((if selected { 5.0 } else { 4.0 }) * vpr) as f32,
                                fill: Color::rgba(0, 0, 0, 0),
                                stroke_width: (if selected { 2.0 } else { 1.0 }) * vpr as f32,
                                stroke,
                            });
                        }
                    });
                    flush_run(&mut run);
                }
                GeneralSeriesKind::RangeArea => {
                    let color = series
                        .color()
                        .and_then(Color::parse_css)
                        .unwrap_or(DEFAULT_LINE_COLOR);
                    let fill = Color::rgba(color.r(), color.g(), color.b(), 56);
                    let mut upper = Vec::<[f32; 2]>::new();
                    let mut lower = Vec::<[f32; 2]>::new();
                    let mut flush_run = |upper: &mut Vec<[f32; 2]>, lower: &mut Vec<[f32; 2]>| {
                        if upper.len() >= 2 && upper.len() == lower.len() {
                            let upper_first = points.len() as u32;
                            let point_count = upper.len() as u32;
                            points.append(upper);
                            let lower_first = points.len() as u32;
                            points.append(lower);
                            out.push(Prim::BandFill {
                                upper_first,
                                lower_first,
                                point_count,
                                fill,
                            });
                            out.push(Prim::Polyline {
                                first_point: upper_first,
                                point_count,
                                width: (GENERAL_LINE_WIDTH_CSS * vpr) as f32,
                                style: LineStyle::Solid,
                                line_type: LineType::Simple,
                                color,
                            });
                            out.push(Prim::Polyline {
                                first_point: lower_first,
                                point_count,
                                width: (GENERAL_LINE_WIDTH_CSS * vpr) as f32,
                                style: LineStyle::Solid,
                                line_type: LineType::Simple,
                                color,
                            });
                        } else {
                            upper.clear();
                            lower.clear();
                        }
                    };
                    self.visit_general_range_points(series, |geometry| {
                        if geometry.starts_new_run {
                            flush_run(&mut upper, &mut lower);
                        }
                        upper.push([(geometry.x * hpr) as f32, (geometry.high_y * vpr) as f32]);
                        lower.push([(geometry.x * hpr) as f32, (geometry.low_y * vpr) as f32]);
                        push_label(
                            geometry.row,
                            geometry.x,
                            geometry.high_y - label_size * 0.65 - 4.0,
                            geometry.low_y + label_size * 0.65 + 4.0,
                            plot.width - 4.0,
                        );
                        let (hovered, selected) =
                            self.general_row_interaction(series.id(), geometry.row);
                        if hovered || selected {
                            let stroke = if selected { PRIMARY } else { GENERAL_HOVER };
                            interaction[usize::from(selected)] = Some(Prim::Circle {
                                cx: (geometry.x * hpr) as f32,
                                cy: (((geometry.low_y + geometry.high_y) * 0.5) * vpr) as f32,
                                radius: ((if selected { 5.0 } else { 4.0 }) * vpr) as f32,
                                fill: Color::rgba(0, 0, 0, 0),
                                stroke_width: (if selected { 2.0 } else { 1.0 }) * vpr as f32,
                                stroke,
                            });
                        }
                    });
                    flush_run(&mut upper, &mut lower);
                }
                GeneralSeriesKind::ErrorBar => {
                    let color = series
                        .color()
                        .and_then(Color::parse_css)
                        .unwrap_or(DEFAULT_LINE_COLOR);
                    let line_width = hpr.min(vpr).round().max(1.0) as i32;
                    self.visit_general_error_bars(series, |geometry| {
                        let x = (geometry.x * hpr).round() as i32;
                        let y = (geometry.y * vpr).round() as i32;
                        let cap_x = (geometry.cap_half_size * hpr).round().max(1.0) as i32;
                        let cap_y = (geometry.cap_half_size * vpr).round().max(1.0) as i32;
                        let x_low = geometry.x_low.map(|value| (value * hpr).round() as i32);
                        let x_high = geometry.x_high.map(|value| (value * hpr).round() as i32);
                        let y_low = geometry.y_low.map(|value| (value * vpr).round() as i32);
                        let y_high = geometry.y_high.map(|value| (value * vpr).round() as i32);
                        if x_low.is_some() || x_high.is_some() {
                            let from = x_low.unwrap_or(x);
                            let to = x_high.unwrap_or(x);
                            out.push(Prim::HLine {
                                y,
                                x0: from.min(to),
                                x1: from.max(to),
                                width: line_width,
                                style: LineStyle::Solid,
                                color,
                            });
                            for bound in [x_low, x_high].into_iter().flatten() {
                                out.push(Prim::VLine {
                                    x: bound,
                                    y0: y - cap_y,
                                    y1: y + cap_y,
                                    width: line_width,
                                    style: LineStyle::Solid,
                                    color,
                                });
                            }
                        }
                        if y_low.is_some() || y_high.is_some() {
                            let from = y_low.unwrap_or(y);
                            let to = y_high.unwrap_or(y);
                            out.push(Prim::VLine {
                                x,
                                y0: from.min(to),
                                y1: from.max(to),
                                width: line_width,
                                style: LineStyle::Solid,
                                color,
                            });
                            for bound in [y_low, y_high].into_iter().flatten() {
                                out.push(Prim::HLine {
                                    y: bound,
                                    x0: x - cap_x,
                                    x1: x + cap_x,
                                    width: line_width,
                                    style: LineStyle::Solid,
                                    color,
                                });
                            }
                        }
                        out.push(Prim::Circle {
                            cx: (geometry.x * hpr) as f32,
                            cy: (geometry.y * vpr) as f32,
                            radius: (2.0 * vpr) as f32,
                            fill: color,
                            stroke_width: 0.0,
                            stroke: color,
                        });
                        let top = [Some(geometry.y), geometry.y_low, geometry.y_high]
                            .into_iter()
                            .flatten()
                            .fold(geometry.y, f64::min);
                        let bottom = [Some(geometry.y), geometry.y_low, geometry.y_high]
                            .into_iter()
                            .flatten()
                            .fold(geometry.y, f64::max);
                        push_label(
                            geometry.row,
                            geometry.x,
                            top - label_size * 0.65 - 4.0,
                            bottom + label_size * 0.65 + 4.0,
                            plot.width - 4.0,
                        );
                        let (hovered, selected) =
                            self.general_row_interaction(series.id(), geometry.row);
                        if hovered || selected {
                            interaction[usize::from(selected)] = Some(Prim::Circle {
                                cx: (geometry.x * hpr) as f32,
                                cy: (geometry.y * vpr) as f32,
                                radius: ((geometry.cap_half_size
                                    + if selected { 3.0 } else { 2.0 })
                                    * vpr) as f32,
                                fill: Color::rgba(0, 0, 0, 0),
                                stroke_width: (if selected { 2.0 } else { 1.0 }) * vpr as f32,
                                stroke: if selected { PRIMARY } else { GENERAL_HOVER },
                            });
                        }
                    });
                }
                GeneralSeriesKind::Column => {
                    let color = series
                        .color()
                        .and_then(Color::parse_css)
                        .unwrap_or(DEFAULT_LINE_COLOR);
                    self.visit_general_columns(series, |geometry| {
                        let left = (geometry.left * hpr).round() as i32;
                        let right = (geometry.right * hpr).round() as i32;
                        let top = (geometry.top * vpr).round() as i32;
                        let bottom = (geometry.bottom * vpr).round() as i32;
                        let width = (right - left).max(1);
                        let height = bottom - top;
                        if height <= 0 {
                            return;
                        }
                        out.push(Prim::Rect {
                            rect: IRect {
                                x: left,
                                y: top,
                                w: width,
                                h: height,
                            },
                            color,
                        });
                        let positive_above = (dataset.y()[geometry.row] >= 0.0)
                            != self
                                .general_axis(series.y_axis_id())
                                .is_some_and(|axis| axis.reverse());
                        let (above, below) = if positive_above {
                            (
                                geometry.top - label_size * 0.65 - 2.0,
                                geometry.bottom + label_size * 0.65 + 2.0,
                            )
                        } else {
                            (
                                geometry.bottom + label_size * 0.65 + 2.0,
                                geometry.top - label_size * 0.65 - 2.0,
                            )
                        };
                        push_label(
                            geometry.row,
                            (geometry.left + geometry.right) * 0.5,
                            above,
                            below,
                            geometry.right - geometry.left - 4.0,
                        );
                        let (hovered, selected) =
                            self.general_row_interaction(series.id(), geometry.row);
                        if hovered || selected {
                            interaction[usize::from(selected)] = Some(Prim::RectFrame {
                                rect: IRect {
                                    x: left,
                                    y: top,
                                    w: width,
                                    h: height,
                                },
                                border: ((if selected { 2.0 } else { 1.0 }) * hpr.min(vpr))
                                    .round()
                                    .max(1.0) as i32,
                                color: if selected { PRIMARY } else { GENERAL_HOVER },
                            });
                        }
                    });
                }
                GeneralSeriesKind::Scatter | GeneralSeriesKind::Bubble => {
                    let color = series
                        .color()
                        .and_then(Color::parse_css)
                        .unwrap_or(DEFAULT_LINE_COLOR);
                    self.visit_general_scatter_points(series, |geometry| {
                        out.push(Prim::Circle {
                            cx: (geometry.x * hpr) as f32,
                            cy: (geometry.y * vpr) as f32,
                            radius: (geometry.radius * vpr) as f32,
                            fill: color,
                            stroke_width: 0.0,
                            stroke: color,
                        });
                        push_label(
                            geometry.row,
                            geometry.x,
                            geometry.y - geometry.radius - label_size * 0.65 - 2.0,
                            geometry.y + geometry.radius + label_size * 0.65 + 2.0,
                            plot.width - 4.0,
                        );
                        let (hovered, selected) =
                            self.general_row_interaction(series.id(), geometry.row);
                        if hovered || selected {
                            let stroke = if selected { PRIMARY } else { GENERAL_HOVER };
                            interaction[usize::from(selected)] = Some(Prim::Circle {
                                cx: (geometry.x * hpr) as f32,
                                cy: (geometry.y * vpr) as f32,
                                radius: ((geometry.radius + if selected { 3.0 } else { 2.0 }) * vpr)
                                    as f32,
                                fill: Color::rgba(0, 0, 0, 0),
                                stroke_width: (if selected { 2.0 } else { 1.0 }) * vpr as f32,
                                stroke,
                            });
                        }
                    });
                }
            }
        }
        out.extend(label_primitives);
        interaction
    }
}
