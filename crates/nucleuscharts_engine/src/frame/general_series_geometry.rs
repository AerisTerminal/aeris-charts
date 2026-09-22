use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::{IRect, Prim, TextAlign};

use crate::{ChartEngine, GeneralSeriesKind, DEFAULT_LINE_COLOR};

use super::PRIMARY;

const GENERAL_HOVER: Color = Color(PRIMARY.0 & 0xFFFF_FF00 | 0x73);
const MAX_GENERAL_DATA_LABELS_PER_PANE: usize = 512;
const MAX_GENERAL_DATA_LABEL_ATTEMPTS_PER_PANE: usize = 4_096;

impl ChartEngine {
    pub(super) fn build_general_series_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
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
                GeneralSeriesKind::Scatter => {
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
