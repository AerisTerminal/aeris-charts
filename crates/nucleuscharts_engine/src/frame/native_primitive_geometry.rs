use super::feature_geometry::{positions_box, positions_line};
use super::*;
use crate::native_primitives::{
    AnchoredTextHorizontalAlign, AnchoredTextVerticalAlign, NativePanePrimitiveKind,
    NativeSeriesPrimitiveKind, OverlayPriceScaleSide,
};
use nucleuscharts_core::format::time_formatter::{
    format_date_pattern, format_tick_label_with, TickMarkType,
};
use nucleuscharts_core::TimePointIndex;
use nucleuscharts_render::draw_list::TextAlign;

fn utc_hour(time: i64) -> u8 {
    (time.rem_euclid(86_400) / 3_600) as u8
}

fn is_weekend(time: i64) -> bool {
    let weekday_from_sunday = (time.div_euclid(86_400) + 4).rem_euclid(7);
    weekday_from_sunday == 0 || weekday_from_sunday == 6
}

fn session_color(time: i64, options: crate::SessionHighlightingOptions) -> Option<Color> {
    if let (Some(start), Some(end)) = (options.start_hour_utc, options.end_hour_utc) {
        let hour = utc_hour(time);
        let inside = if start <= end {
            hour >= start && hour < end
        } else {
            hour >= start || hour < end
        };
        if !inside {
            return None;
        }
    }
    Some(if is_weekend(time) {
        options.weekend_color
    } else {
        options.weekday_color
    })
}

fn faded(color: Color, fade: bool) -> Color {
    if fade {
        Color::rgba(
            color.r(),
            color.g(),
            color.b(),
            u16::from(color.a()).div_ceil(2) as u8,
        )
    } else {
        color
    }
}

fn push_native_polyline(
    out: &mut Vec<Prim>,
    points: &mut Vec<[f32; 2]>,
    coordinates: &[[f32; 2]],
    width: f32,
    color: Color,
) {
    let first_point = points.len() as u32;
    points.extend_from_slice(coordinates);
    out.push(Prim::Polyline {
        first_point,
        point_count: coordinates.len() as u32,
        width,
        style: LineStyle::Solid,
        line_type: LineType::Simple,
        color,
    });
}

fn push_native_dashed_hline(
    out: &mut Vec<Prim>,
    x0: f64,
    x1: f64,
    y: f64,
    hpr: f64,
    vpr: f64,
    color: Color,
) {
    let mut x = x0;
    while x < x1 {
        out.push(Prim::HLine {
            y: (y * vpr).round() as i32,
            x0: (x * hpr).round() as i32,
            x1: ((x + 4.0).min(x1) * hpr).round() as i32,
            width: vpr.round().max(1.0) as i32,
            style: LineStyle::Solid,
            color,
        });
        x += 8.0;
    }
}

#[allow(clippy::too_many_arguments)]
fn push_native_clock_icon(
    out: &mut Vec<Prim>,
    points: &mut Vec<[f32; 2]>,
    cx: f64,
    cy: f64,
    hpr: f64,
    vpr: f64,
    color: Color,
    plus: bool,
) {
    let ratio = hpr.min(vpr);
    out.push(Prim::Circle {
        cx: (cx * hpr) as f32,
        cy: (cy * vpr) as f32,
        radius: (4.7 * ratio) as f32,
        fill: Color::rgba(0, 0, 0, 0),
        stroke_width: ratio.max(1.0) as f32,
        stroke: color,
    });
    let hands = [
        [(cx * hpr) as f32, ((cy - 3.0) * vpr) as f32],
        [(cx * hpr) as f32, (cy * vpr) as f32],
        [((cx - 2.2) * hpr) as f32, (cy * vpr) as f32],
    ];
    push_native_polyline(out, points, &hands, ratio.max(1.0) as f32, color);
    if plus {
        let plus_x = cx + 4.7;
        let plus_y = cy + 4.7;
        let horizontal = [
            [((plus_x - 2.0) * hpr) as f32, (plus_y * vpr) as f32],
            [((plus_x + 2.0) * hpr) as f32, (plus_y * vpr) as f32],
        ];
        let vertical = [
            [(plus_x * hpr) as f32, ((plus_y - 2.0) * vpr) as f32],
            [(plus_x * hpr) as f32, ((plus_y + 2.0) * vpr) as f32],
        ];
        push_native_polyline(out, points, &horizontal, ratio.max(1.0) as f32, color);
        push_native_polyline(out, points, &vertical, ratio.max(1.0) as f32, color);
    }
}

impl ChartEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_native_series_background_primitives_frame(
        &self,
        rs: ResolvedSeries,
        from: i64,
        to: i64,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
        scale: &PriceScaleCore,
    ) {
        let Some(series) = self.series_entry(rs.id) else {
            return;
        };
        if series.native_primitives.is_empty() || scale.is_empty() {
            return;
        }
        let plot = self.data.plot(rs.id);
        let rows = plot
            .visible_rows(from, to)
            .filter(|&row| !plot.is_whitespace_row(row))
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return;
        }
        for primitive in &series.native_primitives {
            let NativeSeriesPrimitiveKind::BandsIndicator(options) = &primitive.kind else {
                continue;
            };
            let upper_first = points.len() as u32;
            points.extend(rows.iter().filter_map(|&row| {
                let index = plot.index_at(row)?;
                let price = plot.value_at(row, PlotValueIndex::Close);
                price.is_finite().then(|| {
                    [
                        (self.time_scale.index_to_coordinate(index) * hpr) as f32,
                        (scale.price_to_coordinate(price * 1.1, rs.base_value) * vpr) as f32,
                    ]
                })
            }));
            let count = points.len() as u32 - upper_first;
            if count == 0 {
                continue;
            }
            let lower_first = points.len() as u32;
            points.extend(rows.iter().filter_map(|&row| {
                let index = plot.index_at(row)?;
                let price = plot.value_at(row, PlotValueIndex::Close);
                price.is_finite().then(|| {
                    [
                        (self.time_scale.index_to_coordinate(index) * hpr) as f32,
                        (scale.price_to_coordinate(price * 0.9, rs.base_value) * vpr) as f32,
                    ]
                })
            }));
            if points.len() as u32 - lower_first != count {
                points.truncate(upper_first as usize);
                continue;
            }
            let width = (options.line_width * vpr) as f32;
            out.push(Prim::Polyline {
                first_point: upper_first,
                point_count: count,
                width,
                style: LineStyle::Solid,
                line_type: LineType::Simple,
                color: options.line_color,
            });
            out.push(Prim::Polyline {
                first_point: lower_first,
                point_count: count,
                width,
                style: LineStyle::Solid,
                line_type: LineType::Simple,
                color: options.line_color,
            });
            // The official renderer strokes `lines` before filling `region`.
            out.push(Prim::BandFill {
                upper_first,
                lower_first,
                point_count: count,
                fill: options.fill_color,
            });
        }
    }

    pub(super) fn build_native_delta_tooltip_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        const FAMILY: &str =
            "-apple-system, BlinkMacSystemFont, 'Trebuchet MS', Roboto, Ubuntu, sans-serif";
        const INLINE_PADDING: f64 = 10.0;
        const BLOCK_PADDING: f64 = 5.0;
        let pane = &self.panes[pane_index];
        for series in self
            .series
            .iter()
            .filter(|series| series.visible && series.pane_index == pane_index)
        {
            for primitive in &series.native_primitives {
                let NativeSeriesPrimitiveKind::DeltaTooltip(state) = &primitive.kind else {
                    continue;
                };
                if state.points.is_empty() {
                    continue;
                }
                let plot = self.data.plot(series.id);
                let Some((times, _)) = self.data.series_data(series.id) else {
                    continue;
                };
                let mut items = Vec::with_capacity(2);
                for point in &state.points {
                    let Some(row) = plot.search(
                        point.index,
                        nucleuscharts_core::model::plot_list::MismatchDirection::None,
                    ) else {
                        continue;
                    };
                    let price = plot.value_at(
                        row,
                        nucleuscharts_core::model::plot_list::PlotValueIndex::Close,
                    );
                    let Some(time) = times.get(row).copied() else {
                        continue;
                    };
                    items.push((point.x, point.index, price, time));
                }
                if items.is_empty() {
                    continue;
                }
                let marker_color = series
                    .line_color
                    .as_deref()
                    .and_then(Color::parse_css)
                    .unwrap_or(Color::rgb(0x88, 0x88, 0x88));
                let background = {
                    let layout = &self.options.get().layout.background;
                    Color::parse_css(&layout.color)
                        .or_else(|| Color::parse_css(&layout.top_color))
                        .unwrap_or(Color::rgb(255, 255, 255))
                };
                let top = pane.top + state.options.top_offset;
                for (x, _, price, _) in &items {
                    let y = self
                        .series_price_to_coordinate(series.id, *price)
                        .unwrap_or(-1_000.0);
                    out.push(Prim::VLine {
                        x: (*x * hpr).round() as i32,
                        y0: (top * vpr).round() as i32,
                        y1: ((pane.top + pane.height) * vpr).round() as i32,
                        width: hpr.round().max(1.0) as i32,
                        style: LineStyle::Solid,
                        color: state.options.line_color,
                    });
                    if y.is_finite() {
                        out.push(Prim::Circle {
                            cx: (*x * hpr) as f32,
                            cy: (y * vpr) as f32,
                            radius: (6.0 * hpr.min(vpr)) as f32,
                            fill: background,
                            stroke_width: 0.0,
                            stroke: background,
                        });
                        out.push(Prim::Circle {
                            cx: (*x * hpr) as f32,
                            cy: (y * vpr) as f32,
                            radius: (4.0 * hpr.min(vpr)) as f32,
                            fill: marker_color,
                            stroke_width: 0.0,
                            stroke: marker_color,
                        });
                    }
                }

                items.sort_by_key(|item| item.1);
                let tooltip_lines = |item: &(f64, i64, f64, i64)| {
                    let mut lines = vec![
                        format!("{:.2}", item.2),
                        format_date_pattern(item.3, "dd MMM yyyy", &self.month_names),
                    ];
                    if state.options.show_time {
                        lines.push(format_tick_label_with(
                            item.3,
                            TickMarkType::Time,
                            &self.month_names,
                        ));
                    }
                    lines
                };
                let lines: Vec<Vec<String>> = items.iter().map(tooltip_lines).collect();
                let section_width = |content: &[String], sizes: &[f64], weights: &[u16]| {
                    content
                        .iter()
                        .enumerate()
                        .map(|(index, text)| {
                            self.measure_text_run(text, sizes[index], FAMILY, weights[index], false)
                        })
                        .fold(0.0_f64, f64::max)
                        + INLINE_PADDING * 2.0
                };
                let section_height = |count: usize, line_heights: &[f64]| {
                    BLOCK_PADDING * 1.5 + line_heights.iter().take(count).sum::<f64>()
                };
                let tooltip_sizes = [14.0, 12.0, 12.0];
                let tooltip_weights = [590, 400, 400];
                let tooltip_heights = [18.0, 16.0, 16.0];
                let mut positions: Vec<(f64, f64)> = items
                    .iter()
                    .zip(&lines)
                    .map(|(item, lines)| {
                        let width = section_width(lines, &tooltip_sizes, &tooltip_weights);
                        (
                            (item.0 - width * 0.5).clamp(0.0, self.pane_w - width),
                            width,
                        )
                    })
                    .collect();
                let mut delta_top = String::new();
                let mut delta_bottom = String::new();
                let mut delta_bg = Color::rgb(255, 255, 255);
                let mut delta_text = Color::rgb(0x13, 0x17, 0x22);
                let mut delta_width = 0.0;
                if items.len() == 2 {
                    let change = items[1].2 - items[0].2;
                    let percent = 100.0 * change / items[0].2;
                    let positive = change >= 0.0;
                    delta_top = format!("{}{change:.2}", if positive { "+" } else { "" });
                    delta_bottom = format!("{}{percent:.2}%", if positive { "+" } else { "" });
                    delta_bg = if positive {
                        Color::rgba(4, 153, 129, 51)
                    } else {
                        Color::rgba(239, 83, 80, 51)
                    };
                    delta_text = if positive {
                        Color::rgb(4, 153, 129)
                    } else {
                        Color::rgb(239, 83, 80)
                    };
                    let min_delta = section_width(
                        &[delta_top.clone(), delta_bottom.clone()],
                        &[14.0, 12.0],
                        &[590, 400],
                    );
                    let overlap = min_delta + positions[0].0 + positions[0].1 - positions[1].0;
                    if overlap > 0.0 {
                        let half = overlap * 0.5;
                        let left_space = positions[0].0;
                        let right_space = self.pane_w - positions[1].0 - positions[1].1;
                        if left_space >= half && right_space >= half {
                            positions[0].0 -= half;
                            positions[1].0 += half;
                        } else if left_space < right_space {
                            positions[0].0 = 0.0;
                            positions[1].0 += overlap - left_space;
                        } else {
                            positions[0].0 = (positions[0].0 - (overlap - right_space)).max(0.0);
                            positions[1].0 += right_space;
                        }
                    }
                    delta_width = (positions[1].0 - positions[0].0 - positions[0].1).round();
                }
                let tooltip_height = lines
                    .iter()
                    .map(|lines| section_height(lines.len(), &tooltip_heights))
                    .fold(0.0_f64, f64::max);
                let delta_height = if items.len() == 2 {
                    section_height(2, &[18.0, 16.0])
                } else {
                    0.0
                };
                let main_height = tooltip_height.max(delta_height);
                let main_x = positions[0].0.round();
                let main_width = if items.len() == 2 {
                    (positions[1].0 + positions[1].1 - positions[0].0).round()
                } else {
                    positions[0].1.round()
                };
                out.push(Prim::RoundRect {
                    x: ((main_x + 1.0) * hpr) as f32,
                    y: ((top + 2.0) * vpr) as f32,
                    w: (main_width * hpr) as f32,
                    h: (main_height * vpr) as f32,
                    radii: [(5.0 * hpr) as f32; 4],
                    fill: Color::rgba(0, 0, 0, 51),
                    border_width: 0.0,
                    border_color: Color::rgba(0, 0, 0, 0),
                });
                out.push(Prim::RoundRect {
                    x: (main_x * hpr) as f32,
                    y: (top * vpr) as f32,
                    w: (main_width * hpr) as f32,
                    h: (main_height * vpr) as f32,
                    radii: [(5.0 * hpr) as f32; 4],
                    fill: Color::rgb(255, 255, 255),
                    border_width: 0.0,
                    border_color: Color::rgb(255, 255, 255),
                });
                if items.len() == 2 && delta_width > 0.0 {
                    out.push(Prim::Rect {
                        rect: IRect {
                            x: ((positions[0].0 + positions[0].1) * hpr).round() as i32,
                            y: (top * vpr).round() as i32,
                            w: (delta_width * hpr).round() as i32,
                            h: (main_height * vpr).round() as i32,
                        },
                        color: delta_bg,
                    });
                }
                for (index, content) in lines.iter().enumerate() {
                    let center_x = positions[index].0 + positions[index].1 * 0.5;
                    let content_height = section_height(content.len(), &tooltip_heights);
                    let mut y = top + (main_height - content_height) * 0.5 + BLOCK_PADDING;
                    for (line_index, text) in content.iter().enumerate() {
                        out.push(Prim::Text {
                            x: (center_x * hpr) as f32,
                            y: ((y + tooltip_sizes[line_index] * 0.5) * vpr) as f32,
                            text: text.clone(),
                            color: if line_index == 0 {
                                Color::rgb(0x13, 0x17, 0x22)
                            } else {
                                Color::rgb(0x78, 0x7b, 0x86)
                            },
                            size: (tooltip_sizes[line_index] * vpr) as f32,
                            family: FAMILY.into(),
                            align: TextAlign::Center,
                            weight: tooltip_weights[line_index],
                            italic: false,
                        });
                        y += tooltip_heights[line_index];
                    }
                }
                if items.len() == 2 {
                    let delta_center = positions[1].0 - delta_width * 0.5;
                    let content_height = section_height(2, &[18.0, 16.0]);
                    let mut y = top + (main_height - content_height) * 0.5 + BLOCK_PADDING;
                    for (index, text) in [delta_top, delta_bottom].into_iter().enumerate() {
                        let size = [14.0, 12.0][index];
                        out.push(Prim::Text {
                            x: (delta_center * hpr) as f32,
                            y: ((y + size * 0.5) * vpr) as f32,
                            text,
                            color: delta_text,
                            size: (size * vpr) as f32,
                            family: FAMILY.into(),
                            align: TextAlign::Center,
                            weight: [590, 400][index],
                            italic: false,
                        });
                        y += [18.0, 16.0][index];
                    }
                }
            }
        }
    }

    pub(super) fn build_native_user_price_alerts_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        const BUTTON_WIDTH: f64 = 21.0;
        const BUTTON_HEIGHT: f64 = 21.0;
        const ALERT_LABEL_HEIGHT: f64 = 17.0;
        const CENTRE_LABEL_HEIGHT: f64 = 20.0;
        const REMOVE_WIDTH: f64 = 26.0;
        let pointer = self
            .clamped_crosshair()
            .filter(|(_, y)| self.pane_at_y(*y) == Some(pane_index));
        for series in self
            .series
            .iter()
            .filter(|series| series.visible && series.pane_index == pane_index)
        {
            for primitive in &series.native_primitives {
                let NativeSeriesPrimitiveKind::UserPriceAlerts(state) = &primitive.kind else {
                    continue;
                };
                let alert_positions: Vec<_> = state
                    .alerts
                    .iter()
                    .filter_map(|alert| {
                        self.series_price_to_coordinate(series.id, alert.price)
                            .filter(|y| y.is_finite())
                            .map(|y| (alert, y))
                    })
                    .collect();
                for (_, y) in &alert_positions {
                    push_native_dashed_hline(
                        out,
                        0.0,
                        (self.pane_w - BUTTON_WIDTH).max(0.0),
                        *y,
                        hpr,
                        vpr,
                        state.options.color,
                    );
                    let (label_y, label_h) = positions_line(*y, vpr, ALERT_LABEL_HEIGHT);
                    let label_x = (self.pane_w - BUTTON_WIDTH - 1.0) * hpr;
                    out.push(Prim::RoundRect {
                        x: label_x as f32,
                        y: label_y as f32,
                        w: (BUTTON_WIDTH * hpr) as f32,
                        h: label_h as f32,
                        radii: [(2.0 * hpr) as f32; 4],
                        fill: state.options.color,
                        border_width: 0.0,
                        border_color: state.options.color,
                    });
                    push_native_clock_icon(
                        out,
                        points,
                        self.pane_w - 11.5,
                        *y,
                        hpr,
                        vpr,
                        Color::rgb(255, 255, 255),
                        false,
                    );
                }

                let active = pointer.and_then(|(_, pointer_y)| {
                    alert_positions
                        .iter()
                        .min_by(|a, b| (pointer_y - a.1).abs().total_cmp(&(pointer_y - b.1).abs()))
                        .filter(|(_, y)| (pointer_y - *y).abs() < 50.0)
                        .copied()
                });
                let remove_hover = pointer.is_some_and(|(x, y)| {
                    matches!(
                        self.user_price_alerts_hit_test(primitive.id, x, y),
                        Some(crate::UserPriceAlertsHit::Remove(_))
                    )
                });
                if let Some((x, y)) = pointer.filter(|_| !remove_hover) {
                    push_native_dashed_hline(
                        out,
                        0.0,
                        (self.pane_w - BUTTON_WIDTH).max(0.0),
                        y,
                        hpr,
                        vpr,
                        state.options.color,
                    );
                    let hovered = (1.0..BUTTON_WIDTH).contains(&(self.pane_w - x));
                    let (button_y, button_h) = positions_line(y, vpr, BUTTON_HEIGHT);
                    let button_x = (self.pane_w - BUTTON_WIDTH - 1.0) * hpr;
                    out.push(Prim::RoundRect {
                        x: button_x as f32,
                        y: button_y as f32,
                        w: (BUTTON_WIDTH * hpr) as f32,
                        h: button_h as f32,
                        radii: [(2.0 * hpr) as f32, 0.0, 0.0, (2.0 * hpr) as f32],
                        fill: if hovered {
                            state.options.hover_color
                        } else {
                            state.options.color
                        },
                        border_width: 0.0,
                        border_color: state.options.color,
                    });
                    push_native_clock_icon(
                        out,
                        points,
                        self.pane_w - 11.5,
                        y,
                        hpr,
                        vpr,
                        Color::rgb(255, 255, 255),
                        true,
                    );
                }

                let Some((alert, y)) = active else {
                    continue;
                };
                let Some(price) = self.series_format_price(series.id, alert.price) else {
                    continue;
                };
                let text = format!("{} crossing {price}", state.options.symbol_name);
                let label_width = 18.0 + REMOVE_WIDTH + text.chars().count() as f64 * 5.81;
                let label_x = (self.pane_w - label_width) * 0.5;
                let label_y = y - CENTRE_LABEL_HEIGHT * 0.5;
                let remove_x = label_x + label_width - REMOVE_WIDTH;
                let white = Color::rgb(255, 255, 255);
                let border = Color::rgb(0x13, 0x17, 0x22);
                out.push(Prim::RoundRect {
                    x: (label_x * hpr) as f32,
                    y: (label_y * vpr) as f32,
                    w: (label_width * hpr) as f32,
                    h: (CENTRE_LABEL_HEIGHT * vpr) as f32,
                    radii: [(4.0 * hpr) as f32; 4],
                    fill: white,
                    border_width: hpr.max(1.0) as f32,
                    border_color: border,
                });
                if remove_hover {
                    out.push(Prim::RoundRect {
                        x: (remove_x * hpr) as f32,
                        y: (label_y * vpr) as f32,
                        w: (REMOVE_WIDTH * hpr) as f32,
                        h: (CENTRE_LABEL_HEIGHT * vpr) as f32,
                        radii: [0.0, (4.0 * hpr) as f32, (4.0 * hpr) as f32, 0.0],
                        fill: Color::rgb(0xf0, 0xf3, 0xfa),
                        border_width: 0.0,
                        border_color: white,
                    });
                }
                out.push(Prim::VLine {
                    x: (remove_x * hpr).round() as i32,
                    y0: (label_y * vpr).round() as i32,
                    y1: ((label_y + CENTRE_LABEL_HEIGHT) * vpr).round() as i32,
                    width: hpr.round().max(1.0) as i32,
                    style: LineStyle::Solid,
                    color: Color::rgb(0xf1, 0xf3, 0xfb),
                });
                out.push(Prim::Text {
                    x: ((label_x + 9.0) * hpr) as f32,
                    y: (y * vpr) as f32,
                    text,
                    color: border,
                    size: (12.0 * vpr) as f32,
                    family: "sans-serif".into(),
                    align: TextAlign::Left,
                    weight: 400,
                    italic: false,
                });
                let cross_cx = remove_x + REMOVE_WIDTH * 0.5;
                let first = [
                    [((cross_cx - 4.5) * hpr) as f32, ((y - 4.5) * vpr) as f32],
                    [((cross_cx + 4.5) * hpr) as f32, ((y + 4.5) * vpr) as f32],
                ];
                let second = [
                    [((cross_cx + 4.5) * hpr) as f32, ((y - 4.5) * vpr) as f32],
                    [((cross_cx - 4.5) * hpr) as f32, ((y + 4.5) * vpr) as f32],
                ];
                push_native_polyline(out, points, &first, hpr.max(1.0) as f32, border);
                push_native_polyline(out, points, &second, hpr.max(1.0) as f32, border);
            }
        }
    }

    pub(super) fn build_native_user_price_lines_button_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        const LABEL_HEIGHT: f64 = 21.0;
        let Some((x, y)) = self.clamped_crosshair() else {
            return;
        };
        if self.pane_at_y(y) != Some(pane_index) || self.pane_w - x > LABEL_HEIGHT * 2.0 {
            return;
        }
        let button_color = Color::parse_css(
            &self
                .options
                .get()
                .crosshair
                .horz_line
                .label_background_color,
        )
        .unwrap_or(Color::rgb(
            nucleuscharts_core::style::DEFAULT_CROSSHAIR_LABEL_RGB.0,
            nucleuscharts_core::style::DEFAULT_CROSSHAIR_LABEL_RGB.1,
            nucleuscharts_core::style::DEFAULT_CROSSHAIR_LABEL_RGB.2,
        ));
        let text_color = Color::rgb(255, 255, 255);
        let hovered = self.pane_w - x < LABEL_HEIGHT;
        for series in self
            .series
            .iter()
            .filter(|series| series.visible && series.pane_index == pane_index)
        {
            for primitive in &series.native_primitives {
                let NativeSeriesPrimitiveKind::UserPriceLinesButton(options) = primitive.kind
                else {
                    continue;
                };
                let (button_x, button_width) =
                    positions_box(self.pane_w - LABEL_HEIGHT - 1.0, self.pane_w - 1.0, hpr);
                let (button_y, button_height) = positions_line(y, vpr, LABEL_HEIGHT);
                out.push(Prim::RoundRect {
                    x: button_x as f32,
                    y: button_y as f32,
                    w: button_width as f32,
                    h: button_height as f32,
                    radii: [(5.0 * hpr) as f32, 0.0, 0.0, (5.0 * hpr) as f32],
                    fill: if hovered {
                        options.hover_color
                    } else {
                        button_color
                    },
                    border_width: 0.0,
                    border_color: button_color,
                });
                let cx = (button_x as f64 + 10.5 * hpr) as f32;
                let cy = (button_y as f64 + 10.5 * vpr) as f32;
                out.push(Prim::Circle {
                    cx,
                    cy,
                    radius: (7.0 * hpr.min(vpr)) as f32,
                    fill: Color::rgba(0, 0, 0, 0),
                    stroke_width: hpr.min(vpr).max(1.0) as f32,
                    stroke: text_color,
                });
                out.push(Prim::HLine {
                    y: cy.round() as i32,
                    x0: (cx - 3.5 * hpr as f32).round() as i32,
                    x1: (cx + 3.5 * hpr as f32).round() as i32,
                    width: hpr.round().max(1.0) as i32,
                    style: LineStyle::Solid,
                    color: text_color,
                });
                out.push(Prim::VLine {
                    x: cx.round() as i32,
                    y0: (cy - 3.5 * vpr as f32).round() as i32,
                    y1: (cy + 3.5 * vpr as f32).round() as i32,
                    width: vpr.round().max(1.0) as i32,
                    style: LineStyle::Solid,
                    color: text_color,
                });
            }
        }
    }

    /// Official anchored-text placement: viewport-relative alignment with fixed 20px/10px
    /// horizontal/vertical margins. Text stays in the shared pane frame and is independent of
    /// time/price coordinates or source data visibility.
    pub(super) fn build_native_anchored_text_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let Some(pane) = self.panes.get(pane_index) else {
            return;
        };
        for primitive in self
            .series
            .iter()
            .filter(|series| series.visible && series.pane_index == pane_index)
            .flat_map(|series| &series.native_primitives)
        {
            let NativeSeriesPrimitiveKind::AnchoredText(options) = &primitive.kind else {
                continue;
            };
            let (x, align) = match options.horizontal_align {
                AnchoredTextHorizontalAlign::Left => (20.0, TextAlign::Left),
                AnchoredTextHorizontalAlign::Middle => (self.pane_w * 0.5, TextAlign::Center),
                AnchoredTextHorizontalAlign::Right => {
                    ((self.pane_w - 20.0).max(0.0), TextAlign::Right)
                }
            };
            // Prim::Text uses a middle baseline. Convert the reference renderer's alphabetic
            // baseline placement using its declared line-height box.
            let y = match options.vertical_align {
                AnchoredTextVerticalAlign::Top => 10.0 + options.line_height * 0.5,
                AnchoredTextVerticalAlign::Middle => pane.height * 0.5,
                AnchoredTextVerticalAlign::Bottom => {
                    (pane.height - 10.0 - options.line_height * 0.5).max(0.0)
                }
            };
            out.push(Prim::Text {
                x: (x * hpr) as f32,
                y: ((pane.top + y) * vpr) as f32,
                text: options.text.clone(),
                color: options.color,
                size: (options.font_size * vpr) as f32,
                family: options.font_family.clone(),
                align,
                weight: options.font_weight,
                italic: options.italic,
            });
        }
    }

    pub(super) fn build_native_text_watermark_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let Some(pane) = self.panes.get(pane_index) else {
            return;
        };
        let Some(pane_id) = pane.stable_id() else {
            return;
        };
        for primitive in self
            .native_pane_primitives
            .iter()
            .filter(|primitive| primitive.pane_id == pane_id)
        {
            let NativePanePrimitiveKind::TextWatermark(options) = &primitive.kind;
            if !options.visible {
                continue;
            }
            let laid = options
                .lines
                .iter()
                .filter(|line| !line.text.is_empty())
                .map(|line| {
                    let width = self.measure_text_run(
                        &line.text,
                        line.font_size,
                        &line.font_family,
                        line.font_weight,
                        line.italic,
                    );
                    let zoom = if width > self.pane_w && width > 0.0 {
                        self.pane_w / width
                    } else {
                        1.0
                    };
                    (line, zoom)
                })
                .collect::<Vec<_>>();
            let text_height = laid
                .iter()
                .map(|(line, zoom)| line.line_height * zoom)
                .sum::<f64>();
            let mut y = match options.vertical_align {
                AnchoredTextVerticalAlign::Top => 0.0,
                AnchoredTextVerticalAlign::Middle => ((pane.height - text_height) * 0.5).max(0.0),
                AnchoredTextVerticalAlign::Bottom => (pane.height - text_height).max(0.0),
            };
            for (line, zoom) in laid {
                let (x, align) = match options.horizontal_align {
                    AnchoredTextHorizontalAlign::Left => (line.line_height * 0.5, TextAlign::Left),
                    AnchoredTextHorizontalAlign::Middle => (self.pane_w * 0.5, TextAlign::Center),
                    AnchoredTextHorizontalAlign::Right => (
                        (self.pane_w - 1.0 - line.line_height * 0.5).max(0.0),
                        TextAlign::Right,
                    ),
                };
                let size = line.font_size * zoom;
                out.push(Prim::Text {
                    x: (x * hpr) as f32,
                    y: ((pane.top + y + size * 0.5) * vpr) as f32,
                    text: line.text.clone(),
                    color: line.color,
                    size: (size * vpr) as f32,
                    family: line.font_family.clone(),
                    align,
                    weight: line.font_weight,
                    italic: line.italic,
                });
                y += line.line_height * zoom;
            }
        }
    }

    /// Official image-watermark placement: center the source aspect ratio in the plot area after
    /// padding and optional maximum dimensions. The image is the attached series' bottom view.
    pub(super) fn build_native_image_watermark_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let Some(pane) = self.panes.get(pane_index) else {
            return;
        };
        for primitive in self
            .series
            .iter()
            .filter(|series| series.visible && series.pane_index == pane_index)
            .flat_map(|series| &series.native_primitives)
        {
            let NativeSeriesPrimitiveKind::ImageWatermark { image, options } = &primitive.kind
            else {
                continue;
            };
            let mut available_width = self.pane_w - options.padding * 2.0;
            let mut available_height = pane.height - options.padding * 2.0;
            if let Some(max_width) = options.max_width {
                available_width = available_width.min(max_width);
            }
            if let Some(max_height) = options.max_height {
                available_height = available_height.min(max_height);
            }
            if available_width <= 0.0 || available_height <= 0.0 || options.alpha <= 0.0 {
                continue;
            }
            let scale = (available_width / f64::from(image.width))
                .min(available_height / f64::from(image.height));
            if !scale.is_finite() || scale <= 0.0 {
                continue;
            }
            let width = f64::from(image.width) * scale;
            let height = f64::from(image.height) * scale;
            out.push(Prim::Image {
                image: image.clone(),
                rect: [
                    ((self.pane_w - width) * 0.5 * hpr) as f32,
                    ((pane.top + (pane.height - height) * 0.5) * vpr) as f32,
                    (width * hpr) as f32,
                    (height * vpr) as f32,
                ],
                opacity: options.alpha as f32,
            });
        }
    }

    /// Official session-highlighting semantics: color every source bar, derive the slot width from
    /// the first two source coordinates, clip in bitmap space, and paint below all series.
    pub(super) fn build_native_session_highlighting_frame(
        &self,
        pane_index: usize,
        from: i64,
        to: i64,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let pane = &self.panes[pane_index];
        let pane_width = (self.pane_w * hpr).round() as i32;
        let y = (pane.top * vpr).round() as i32;
        let height = (pane.height * vpr).round().max(1.0) as i32;
        for series in self
            .series
            .iter()
            .filter(|series| series.visible && series.pane_index == pane_index)
        {
            let Some((times, _)) = self.data.series_data(series.id) else {
                continue;
            };
            let bar_width = if times.len() > 1 {
                let Some(first) = self.time_to_coordinate(times[0] as f64) else {
                    continue;
                };
                let Some(second) = self.time_to_coordinate(times[1] as f64) else {
                    continue;
                };
                second - first
            } else {
                6.0
            };
            let half_width = hpr * bar_width / 2.0;
            for primitive in &series.native_primitives {
                let NativeSeriesPrimitiveKind::SessionHighlighting(state) = &primitive.kind else {
                    continue;
                };
                let mut pending: Option<(IRect, Color)> = None;
                {
                    let mut append = |time: i64, color: Color| {
                        let Some(logical) = self.time_to_index(time as f64, false) else {
                            return;
                        };
                        if logical < from || logical > to {
                            return;
                        }
                        let x = self.time_scale.index_to_coordinate(logical) * hpr;
                        let left = (x - half_width).round().max(0.0) as i32;
                        let right = (x + half_width).round().min(pane_width as f64) as i32;
                        if right <= left {
                            return;
                        }
                        let rect = IRect {
                            x: left,
                            y,
                            w: right - left,
                            h: height,
                        };
                        match pending.as_mut() {
                            Some((previous, previous_color))
                                if *previous_color == color
                                    && rect.x <= previous.x.saturating_add(previous.w) =>
                            {
                                previous.w = previous.w.max(rect.x + rect.w - previous.x);
                            }
                            Some(_) => {
                                let (rect, color) = pending.replace((rect, color)).unwrap();
                                out.push(Prim::Rect { rect, color });
                            }
                            None => pending = Some((rect, color)),
                        }
                    };
                    if let Some(highlights) = &state.highlights {
                        for highlight in highlights {
                            append(highlight.time, highlight.color);
                        }
                    } else {
                        for &time in times {
                            if let Some(color) = session_color(time, state.options) {
                                append(time, color);
                            }
                        }
                    }
                }
                if let Some((rect, color)) = pending {
                    out.push(Prim::Rect { rect, color });
                }
            }
        }
    }

    /// Cursor highlight is retained separately from the static underlay so movement rebuilds one
    /// rectangle instead of the grid and every series.
    pub(super) fn build_native_crosshair_highlight_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let Some((x, _)) = self.clamped_crosshair() else {
            return;
        };
        let x = self
            .time_scale
            .index_to_coordinate(self.snapped_crosshair_index(x));
        let pane = &self.panes[pane_index];
        let (left, width) = positions_line(x, hpr, self.time_scale.bar_spacing());
        for series in self
            .series
            .iter()
            .filter(|series| series.visible && series.pane_index == pane_index)
        {
            for primitive in &series.native_primitives {
                let NativeSeriesPrimitiveKind::HighlightBarCrosshair { color } = primitive.kind
                else {
                    continue;
                };
                out.push(Prim::Rect {
                    rect: IRect {
                        x: left,
                        y: (pane.top * vpr).round() as i32,
                        w: width,
                        h: (pane.height * vpr).round().max(1.0) as i32,
                    },
                    color,
                });
            }
        }
    }

    /// Visible accessibility focus ring. Keyboard and ARIA semantics live at the host boundary;
    /// the selected source time, exact value lookup, device-pixel geometry, and backend parity are
    /// owned by the shared engine.
    pub(super) fn build_native_accessibility_focus_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let transparent = Color::rgba(0, 0, 0, 0);
        for series in self
            .series
            .iter()
            .filter(|series| series.visible && series.pane_index == pane_index)
        {
            for primitive in &series.native_primitives {
                let NativeSeriesPrimitiveKind::AccessibilityFocus(state) = primitive.kind else {
                    continue;
                };
                let Some(time) = state.time else {
                    continue;
                };
                let Some(logical) = self.time_to_index(time as f64, false) else {
                    continue;
                };
                let plot = self.data.plot(series.id);
                let Some(row) = plot.search(
                    logical,
                    nucleuscharts_core::model::plot_list::MismatchDirection::None,
                ) else {
                    continue;
                };
                if plot.is_whitespace_row(row) {
                    continue;
                }
                let price = plot.value_at(
                    row,
                    nucleuscharts_core::model::plot_list::PlotValueIndex::Close,
                );
                let Some(y) = self.series_price_to_coordinate(series.id, price) else {
                    continue;
                };
                let x = self.time_scale.index_to_coordinate(logical);
                if x < 0.0 || x > self.pane_w {
                    continue;
                }
                let cx = (x * hpr) as f32;
                let cy = (y * vpr) as f32;
                let radius = (state.options.size * 0.5 * hpr.min(vpr)) as f32;
                if state.options.high_contrast {
                    out.push(Prim::Circle {
                        cx,
                        cy,
                        radius: radius + (3.0 * hpr.min(vpr)) as f32,
                        fill: transparent,
                        stroke_width: (4.0 * hpr.min(vpr)) as f32,
                        stroke: Color::rgb(0, 0, 0),
                    });
                    out.push(Prim::Circle {
                        cx,
                        cy,
                        radius: radius + hpr.min(vpr) as f32,
                        fill: transparent,
                        stroke_width: (4.0 * hpr.min(vpr)) as f32,
                        stroke: Color::rgb(255, 255, 255),
                    });
                } else {
                    out.push(Prim::Circle {
                        cx,
                        cy,
                        radius: radius + hpr.min(vpr) as f32,
                        fill: transparent,
                        stroke_width: (4.0 * hpr.min(vpr)) as f32,
                        stroke: Color::rgba(255, 255, 255, 230),
                    });
                }
                out.push(Prim::Circle {
                    cx,
                    cy,
                    radius,
                    fill: transparent,
                    stroke_width: ((if state.options.high_contrast {
                        3.0
                    } else {
                        2.0
                    }) * hpr.min(vpr)) as f32,
                    stroke: state.options.color,
                });
            }
        }
    }

    /// Official tooltip primitive's bottom-layer vertical guide. Pointer normalization and exact
    /// source-row lookup are shared with [`ChartEngine::tooltip_snapshot`]; the DOM tooltip is
    /// only presentation chrome at the browser boundary.
    pub(super) fn build_native_tooltip_crosshair_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let pane = &self.panes[pane_index];
        for series in self
            .series
            .iter()
            .filter(|series| series.visible && series.pane_index == pane_index)
        {
            for primitive in &series.native_primitives {
                let NativeSeriesPrimitiveKind::Tooltip(options) = primitive.kind else {
                    continue;
                };
                let Some(snapshot) = self.tooltip_snapshot(primitive.id) else {
                    continue;
                };
                let (x, width) = positions_line(snapshot.x, hpr, 1.0);
                let top = pane.top + options.top_margin;
                let bottom = pane.top + pane.height;
                if top >= bottom {
                    continue;
                }
                out.push(Prim::Rect {
                    rect: IRect {
                        x,
                        y: (top * vpr).round() as i32,
                        w: width,
                        h: ((bottom - top) * vpr).round().max(1.0) as i32,
                    },
                    color: options.line_color,
                });
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_native_series_primitives_frame(
        &self,
        rs: ResolvedSeries,
        from: i64,
        to: i64,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
        scale: &PriceScaleCore,
    ) {
        let Some(series) = self.series_entry(rs.id) else {
            return;
        };
        if series.native_primitives.is_empty() || scale.is_empty() {
            return;
        }
        let Some(base_value) = self.series_base_value(rs.id, from) else {
            return;
        };
        for primitive in &series.native_primitives {
            match &primitive.kind {
                NativeSeriesPrimitiveKind::AnchoredText(_)
                | NativeSeriesPrimitiveKind::ImageWatermark { .. } => {}
                NativeSeriesPrimitiveKind::BandsIndicator(_) => {}
                NativeSeriesPrimitiveKind::OverlayPriceScale(options) => {
                    const TICK_SPACING: f64 = 40.0;
                    const HALF_TICK: f64 = 10.0;
                    const HORIZONTAL_PADDING: f64 = 3.0;
                    const VERTICAL_PADDING: f64 = 2.0;
                    const SIDE_MARGIN: f64 = 10.0;
                    const FONT_SIZE: f64 = 12.0;
                    const RADIUS: f32 = 4.0;
                    const FAMILY: &str = "-apple-system, BlinkMacSystemFont, 'Trebuchet MS', Roboto, Ubuntu, sans-serif";
                    let pane = &self.panes[series.pane_index];
                    let mut labels = Vec::new();
                    let mut position = HALF_TICK;
                    while position <= pane.height - HALF_TICK {
                        let coordinate = pane.top + position;
                        let price = scale.coordinate_to_price(coordinate, base_value);
                        let logical = scale.price_to_logical_value(price, base_value);
                        labels.push((position, self.format_series_value(series, scale, logical)));
                        position += TICK_SPACING;
                    }
                    let max_label_length = labels
                        .iter()
                        .map(|(_, label)| label.chars().count())
                        .max()
                        .unwrap_or(0);
                    if max_label_length == 0 {
                        continue;
                    }
                    let test_label = "0".repeat(max_label_length);
                    let width = self.measure_text_run(&test_label, FONT_SIZE, FAMILY, 400, false);
                    let x = match options.side {
                        OverlayPriceScaleSide::Left => SIDE_MARGIN,
                        OverlayPriceScaleSide::Right => self.pane_w - SIDE_MARGIN - width,
                    };
                    let text_x = x + HORIZONTAL_PADDING + (width / 2.0).round();
                    for (position, label) in labels {
                        let top_y = pane.top + position - FONT_SIZE / 2.0;
                        out.push(Prim::RoundRect {
                            x: (x * hpr) as f32,
                            y: (top_y * vpr) as f32,
                            w: ((width + HORIZONTAL_PADDING * 2.0) * hpr) as f32,
                            h: ((FONT_SIZE + VERTICAL_PADDING * 2.0) * vpr) as f32,
                            radii: [RADIUS * hpr as f32; 4],
                            fill: options.background_color,
                            border_width: 0.0,
                            border_color: options.background_color,
                        });
                        out.push(Prim::Text {
                            x: (text_x * hpr) as f32,
                            y: ((top_y + VERTICAL_PADDING + FONT_SIZE / 2.0) * vpr) as f32,
                            text: label,
                            color: options.text_color,
                            size: (FONT_SIZE * vpr) as f32,
                            family: FAMILY.into(),
                            align: TextAlign::Center,
                            weight: 400,
                            italic: false,
                        });
                    }
                }
                NativeSeriesPrimitiveKind::PartialPriceLine => {
                    let plot = self.data.plot(rs.id);
                    let Some(row) = plot.last_non_whitespace_row(TimePointIndex::MAX) else {
                        continue;
                    };
                    let Some(logical) = plot.index_at(row) else {
                        continue;
                    };
                    let price = plot.value_at(row, PlotValueIndex::Close);
                    if !price.is_finite() {
                        continue;
                    }
                    let start = (self.time_scale.index_to_coordinate(logical) * hpr).round() as i32;
                    let end = (self.pane_w * hpr).round() as i32;
                    if start >= end {
                        continue;
                    }
                    let (line_y, line_height) =
                        positions_line(scale.price_to_coordinate(price, base_value), vpr, 1.0);
                    let y = line_y + line_height / 2;
                    let baseline = (series.kind == SeriesKind::Baseline)
                        .then(|| self.resolved_baseline_price(series.id, from, to))
                        .flatten();
                    let color = series
                        .price_line_color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or_else(|| self.series_bar_color(series, row, baseline));
                    let dash = (4.0 * vpr).round().max(1.0) as i32;
                    let gap = (2.0 * vpr).round().max(1.0) as i32;
                    let mut x = start;
                    while x < end {
                        out.push(Prim::HLine {
                            y,
                            x0: x,
                            x1: (x + dash).min(end),
                            width: vpr.round().max(1.0) as i32,
                            style: LineStyle::Solid,
                            color,
                        });
                        x = x.saturating_add(dash + gap);
                    }
                }
                NativeSeriesPrimitiveKind::VolumeProfile { data, options } => {
                    let Some(logical) = self.time_to_index(data.time as f64, false) else {
                        continue;
                    };
                    if to < logical || from as f64 > logical as f64 + data.width {
                        continue;
                    }
                    let x = self.time_scale.index_to_coordinate(logical);
                    let width = self.time_scale.bar_spacing() * data.width;
                    let y1 = scale.price_to_coordinate(data.profile[0].price, base_value);
                    let y2 = scale.price_to_coordinate(data.profile[1].price, base_value);
                    let column_height = (y1 - y2).max(1.0);
                    let max_volume = data
                        .profile
                        .iter()
                        .map(|point| point.volume)
                        .fold(0.0_f64, f64::max);
                    let (background_x, background_width) = positions_box(x, x + width, hpr);
                    let (background_y, background_height) =
                        positions_box(y1, y1 - column_height * data.profile.len() as f64, vpr);
                    out.push(Prim::Rect {
                        rect: IRect {
                            x: background_x,
                            y: background_y,
                            w: background_width,
                            h: background_height,
                        },
                        color: options.background_color,
                    });
                    for point in &data.profile {
                        if point.volume <= 0.0 {
                            continue;
                        }
                        let row_y = scale.price_to_coordinate(point.price, base_value);
                        let row_width = width * point.volume / max_volume;
                        let (row_x, row_width) = positions_box(x, x + row_width, hpr);
                        let (row_y, row_height) = positions_box(row_y, row_y - column_height, vpr);
                        out.push(Prim::Rect {
                            rect: IRect {
                                x: row_x,
                                y: row_y,
                                w: row_width,
                                h: (row_height - 2).max(1),
                            },
                            color: options.row_color,
                        });
                    }
                }
                NativeSeriesPrimitiveKind::VerticalLine { time, options } => {
                    let Some(x) = self.time_to_coordinate(*time as f64) else {
                        continue;
                    };
                    let pane = &self.panes[series.pane_index];
                    let (x, width) = positions_line(x, hpr, options.width);
                    out.push(Prim::Rect {
                        rect: IRect {
                            x,
                            y: (pane.top * vpr).round() as i32,
                            w: width,
                            h: (pane.height * vpr).round().max(1.0) as i32,
                        },
                        color: options.color,
                    });
                }
                NativeSeriesPrimitiveKind::TrendLine {
                    first_time,
                    first_price,
                    second_time,
                    second_price,
                    options,
                } => {
                    let (Some(first_x), Some(second_x)) = (
                        self.time_to_coordinate(*first_time as f64),
                        self.time_to_coordinate(*second_time as f64),
                    ) else {
                        continue;
                    };
                    let first_y = scale.price_to_coordinate(*first_price, base_value);
                    let second_y = scale.price_to_coordinate(*second_price, base_value);
                    let first = [
                        (first_x * hpr).round() as f32,
                        (first_y * vpr).round() as f32,
                    ];
                    let second = [
                        (second_x * hpr).round() as f32,
                        (second_y * vpr).round() as f32,
                    ];
                    let first_point = points.len() as u32;
                    points.extend([first, second]);
                    out.push(Prim::Polyline {
                        first_point,
                        point_count: 2,
                        width: options.width as f32,
                        style: LineStyle::Solid,
                        line_type: LineType::Simple,
                        color: options.line_color,
                    });
                    if options.show_labels {
                        let first_text = format!("{first_price:.1}");
                        let second_text = format!("{second_price:.1}");
                        self.build_native_trend_label(first, &first_text, true, hpr, options, out);
                        self.build_native_trend_label(
                            second,
                            &second_text,
                            false,
                            hpr,
                            options,
                            out,
                        );
                    }
                }
                NativeSeriesPrimitiveKind::ExpiringPriceAlerts(state) => {
                    let pane_width = self.pane_w;
                    for alert in &state.alerts {
                        let (start_x, end_x) = (
                            self.time_to_coordinate(alert.start as f64),
                            self.time_to_coordinate(alert.end as f64),
                        );
                        if start_x.is_none() && end_x.is_none() {
                            continue;
                        }
                        let start_x = start_x.unwrap_or(0.0);
                        let end_x = end_x.unwrap_or(pane_width);
                        let base_color = match alert.crossing_direction {
                            crate::AlertCrossingDirection::Up => {
                                if alert.crossed {
                                    Color::rgb(0x38, 0x6d, 0x2e)
                                } else if alert.expired {
                                    Color::rgb(0x30, 0x47, 0x2c)
                                } else {
                                    Color::rgb(0x64, 0xc7, 0x50)
                                }
                            }
                            crate::AlertCrossingDirection::Down => {
                                if alert.crossed {
                                    Color::rgb(0x7c, 0x1f, 0x3e)
                                } else if alert.expired {
                                    Color::rgb(0x4a, 0x2d, 0x37)
                                } else {
                                    Color::rgb(0xc8, 0x32, 0x64)
                                }
                            }
                        };
                        let color = faded(base_color, alert.expired);
                        let white = faded(Color::rgb(255, 255, 255), alert.expired);
                        let (line_y, line_height) = positions_line(
                            scale.price_to_coordinate(alert.price, base_value),
                            vpr,
                            2.0,
                        );
                        let y = f64::from(line_y) + f64::from(line_height) * 0.5;
                        let start_bitmap = start_x * hpr;
                        let end_bitmap = end_x * hpr;
                        let y_bitmap = y as f32;

                        if end_x - 4.0 > start_x + 4.0 {
                            out.push(Prim::HLine {
                                y: y.round() as i32,
                                x0: ((start_x + 4.0) * hpr).round() as i32,
                                x1: ((end_x - 4.0) * hpr).round() as i32,
                                width: (2.0 * vpr).round().max(1.0) as i32,
                                style: LineStyle::Solid,
                                color,
                            });
                        }
                        let mut dash_x = start_x - 30.0;
                        while dash_x < pane_width {
                            let dash_end = (dash_x + 3.0).min(pane_width);
                            out.push(Prim::HLine {
                                y: y.round() as i32,
                                x0: (dash_x * hpr).round() as i32,
                                x1: (dash_end * hpr).round() as i32,
                                width: (2.0 * vpr).round().max(1.0) as i32,
                                style: LineStyle::Solid,
                                color,
                            });
                            dash_x += 9.0;
                        }
                        for cx in [start_bitmap, end_bitmap] {
                            out.push(Prim::Circle {
                                cx: cx as f32,
                                cy: y_bitmap,
                                radius: (4.0 * hpr) as f32,
                                fill: color,
                                stroke_width: 0.0,
                                stroke: color,
                            });
                        }

                        let text_width =
                            self.measure_text_run(&alert.title, 10.0, "sans-serif", 400, false);
                        let label_left = start_x - 50.0 - text_width;
                        out.push(Prim::RoundRect {
                            x: (label_left * hpr) as f32,
                            y: ((y / vpr - 7.0) * vpr) as f32,
                            w: ((text_width + 20.0) * hpr) as f32,
                            h: (14.0 * vpr) as f32,
                            radii: [(4.0 * hpr) as f32; 4],
                            fill: color,
                            border_width: 0.0,
                            border_color: color,
                        });
                        out.push(Prim::Text {
                            x: ((start_x - 45.0 - text_width) * hpr) as f32,
                            y: y_bitmap,
                            text: alert.title.clone(),
                            color: white,
                            size: (10.0 * vpr) as f32,
                            family: "sans-serif".into(),
                            align: TextAlign::Left,
                            weight: 400,
                            italic: false,
                        });

                        let icon_x = (start_x - 44.0) * hpr;
                        let icon_y = y;
                        let icon_scale = 0.6 * hpr;
                        let icon_width = (1.5 * hpr).max(1.0) as f32;
                        if alert.crossed || alert.expired {
                            out.push(Prim::Circle {
                                cx: icon_x as f32,
                                cy: icon_y as f32,
                                radius: (4.8 * hpr) as f32,
                                fill: white,
                                stroke_width: 0.0,
                                stroke: white,
                            });
                            if alert.crossed {
                                let check = [
                                    [(icon_x - 2.3 * hpr) as f32, (icon_y + 0.2 * vpr) as f32],
                                    [(icon_x - 0.5 * hpr) as f32, (icon_y + 2.0 * vpr) as f32],
                                    [(icon_x + 2.8 * hpr) as f32, (icon_y - 2.5 * vpr) as f32],
                                ];
                                push_native_polyline(out, points, &check, icon_width, color);
                            } else {
                                let first = [
                                    [(icon_x - 2.0 * hpr) as f32, (icon_y - 2.0 * vpr) as f32],
                                    [(icon_x + 2.0 * hpr) as f32, (icon_y + 2.0 * vpr) as f32],
                                ];
                                let second = [
                                    [(icon_x + 2.0 * hpr) as f32, (icon_y - 2.0 * vpr) as f32],
                                    [(icon_x - 2.0 * hpr) as f32, (icon_y + 2.0 * vpr) as f32],
                                ];
                                push_native_polyline(out, points, &first, icon_width, color);
                                push_native_polyline(out, points, &second, icon_width, color);
                            }
                        } else {
                            let sign = if matches!(
                                alert.crossing_direction,
                                crate::AlertCrossingDirection::Up
                            ) {
                                -1.0
                            } else {
                                1.0
                            };
                            let arrow = [
                                [
                                    (icon_x - 3.0 * icon_scale) as f32,
                                    (icon_y - sign * 3.0 * icon_scale) as f32,
                                ],
                                [
                                    (icon_x + 3.0 * icon_scale) as f32,
                                    (icon_y + sign * 3.0 * icon_scale) as f32,
                                ],
                            ];
                            push_native_polyline(out, points, &arrow, icon_width, white);
                            let head_h = [
                                [
                                    (icon_x + 0.5 * hpr) as f32,
                                    (icon_y + sign * 3.0 * icon_scale) as f32,
                                ],
                                [
                                    (icon_x + 3.0 * icon_scale) as f32,
                                    (icon_y + sign * 3.0 * icon_scale) as f32,
                                ],
                            ];
                            let head_v = [
                                [
                                    (icon_x + 3.0 * icon_scale) as f32,
                                    (icon_y + sign * 3.0 * icon_scale) as f32,
                                ],
                                [
                                    (icon_x + 3.0 * icon_scale) as f32,
                                    (icon_y + sign * 0.5 * hpr) as f32,
                                ],
                            ];
                            push_native_polyline(out, points, &head_h, icon_width, white);
                            push_native_polyline(out, points, &head_v, icon_width, white);
                        }
                    }
                }
                NativeSeriesPrimitiveKind::AccessibilityFocus(_)
                | NativeSeriesPrimitiveKind::SessionHighlighting(_)
                | NativeSeriesPrimitiveKind::HighlightBarCrosshair { .. }
                | NativeSeriesPrimitiveKind::UserPriceLinesButton(_)
                | NativeSeriesPrimitiveKind::UserPriceAlerts(_)
                | NativeSeriesPrimitiveKind::Tooltip(_)
                | NativeSeriesPrimitiveKind::DeltaTooltip(_) => {}
            }
        }
    }

    fn build_native_trend_label(
        &self,
        point: [f32; 2],
        text: &str,
        left: bool,
        hpr: f64,
        options: &crate::TrendLineOptions,
        out: &mut Vec<Prim>,
    ) {
        const SIZE: f64 = 24.0;
        let offset = 5.0 * hpr;
        let text_width = self.measure_text_run(text, SIZE, "Arial", 400, false);
        let left_adjustment = if left { text_width + offset * 4.0 } else { 0.0 };
        let x = f64::from(point[0]);
        let y = f64::from(point[1]);
        out.push(Prim::RoundRect {
            x: (x + offset - left_adjustment) as f32,
            y: (y - SIZE) as f32,
            w: (text_width + offset * 2.0) as f32,
            h: (SIZE + offset) as f32,
            radii: [5.0; 4],
            fill: options.label_background_color,
            border_width: 0.0,
            border_color: options.label_background_color,
        });
        out.push(Prim::Text {
            x: (x + offset * 2.0 - left_adjustment) as f32,
            y: (y - SIZE * 0.5) as f32,
            text: text.to_string(),
            color: options.label_text_color,
            size: SIZE as f32,
            family: "Arial".into(),
            align: TextAlign::Left,
            weight: 400,
            italic: false,
        });
    }
}
