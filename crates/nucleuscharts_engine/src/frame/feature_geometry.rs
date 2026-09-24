use super::*;
use crate::feature_series::{FeatureSeriesKind, FeatureSeriesOptions, FeatureValue};
use nucleuscharts_render::bar_width::optimal_candlestick_width;

#[derive(Clone, Copy)]
struct VisibleFeatureBar<'a> {
    x_media: f64,
    value: &'a FeatureValue,
}

#[derive(Clone, Copy)]
struct ColumnPosition {
    left: i32,
    right: i32,
    shifted_left: bool,
}

pub(super) fn positions_line(position: f64, ratio: f64, desired_width: f64) -> (i32, i32) {
    let center = (position * ratio).round() as i32;
    let width = (desired_width * ratio).round().max(1.0) as i32;
    (center - width / 2, width)
}

pub(super) fn positions_box(first: f64, second: f64, ratio: f64) -> (i32, i32) {
    let first = (first * ratio).round() as i32;
    let second = (second * ratio).round() as i32;
    (first.min(second), (second - first).abs() + 1)
}

fn full_bar_width(x: f64, spacing: f64, hpr: f64) -> (i32, i32) {
    let left = ((x - spacing / 2.0) * hpr).round() as i32;
    let right = ((x + spacing / 2.0) * hpr).round() as i32;
    (left, (right - left).max(1))
}

fn column_positions(xs: &[f64], spacing: f64, hpr: f64) -> Vec<ColumnPosition> {
    let gap = if (spacing * hpr).ceil() <= 1.0 {
        0
    } else {
        hpr.floor().max(1.0) as i32
    };
    let width = (spacing * hpr).round() as i32 - gap;
    let shift_even = width % 2 == 0;
    let half = (width - i32::from(!shift_even)) / 2;
    let mut positions: Vec<ColumnPosition> = Vec::with_capacity(xs.len());
    for &x in xs {
        let raw = x * hpr;
        let rounded = raw.round() as i32;
        let mut current = ColumnPosition {
            left: rounded - half,
            right: rounded + half - i32::from(shift_even),
            shifted_left: rounded as f64 > raw,
        };
        if let Some(previous) = positions.last_mut() {
            let expected = gap + 1;
            if current.left - previous.right != expected {
                if previous.shifted_left {
                    previous.right = current.left - expected;
                } else {
                    current.left = previous.right + expected;
                }
            }
        }
        positions.push(current);
    }
    let mut minimum = (spacing * hpr).ceil() as i32;
    for position in &mut positions {
        if position.right < position.left {
            position.right = position.left;
        }
        minimum = minimum.min(position.right - position.left + 1);
    }
    if gap > 0 && minimum < 4 {
        for position in &mut positions {
            if position.right - position.left < minimum {
                continue;
            }
            if position.shifted_left {
                position.right -= 1;
            } else {
                position.left += 1;
            }
        }
    }
    positions
}

fn mix_background_color(low: Color, high: Color, amount: f64) -> Color {
    let channel = |a: u8, b: u8| {
        (a as f64 + (b as f64 - a as f64) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color::rgb(
        channel(low.r(), high.r()),
        channel(low.g(), high.g()),
        channel(low.b(), high.b()),
    )
}

fn push_polyline(
    out: &mut Vec<Prim>,
    points: &mut Vec<[f32; 2]>,
    values: &[[f32; 2]],
    width: f32,
    color: Color,
) {
    if values.len() < 2 {
        return;
    }
    let first = points.len() as u32;
    points.extend_from_slice(values);
    out.push(Prim::Polyline {
        first_point: first,
        point_count: values.len() as u32,
        width,
        style: LineStyle::Solid,
        line_type: LineType::Simple,
        color,
    });
}

impl ChartEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_feature_series_frame(
        &self,
        rs: ResolvedSeries,
        from: i64,
        to: i64,
        hpr: f64,
        vpr: f64,
        pane_top: f64,
        pane_height: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
    ) {
        let Some(feature) = self
            .series_entry(rs.id)
            .and_then(|series| series.feature.as_ref())
        else {
            return;
        };
        let plot = self.data.plot(rs.id);
        // The official pretty-histogram renderer deliberately slices through `visibleRange.to +
        // 1`, painting the first bar beyond the strict right edge so a rounded column does not
        // pop in late while scrolling. Every other example consumes the strict range as-is.
        let render_to = if feature.kind == FeatureSeriesKind::PrettyHistogram {
            to.saturating_add(1)
        } else {
            to
        };
        let visible = plot.visible_rows(from, render_to);
        let bars = visible
            .filter_map(|row| {
                let value = feature.rows.get(row)?.value.as_ref()?;
                let logical = plot.index_at(row)?;
                Some(VisibleFeatureBar {
                    x_media: self.time_scale.index_to_coordinate(logical),
                    value,
                })
            })
            .collect::<Vec<_>>();
        if bars.is_empty() {
            return;
        }
        match feature.kind {
            FeatureSeriesKind::GroupedBars => self.build_grouped_bars_feature(
                &bars,
                &feature.options,
                hpr,
                vpr,
                out,
                scale,
                rs.base_value,
            ),
            FeatureSeriesKind::Heatmap => self.build_heatmap_feature(
                &bars,
                &feature.options,
                hpr,
                vpr,
                out,
                scale,
                rs.base_value,
            ),
            FeatureSeriesKind::HlcArea => self.build_hlc_area_feature(
                &bars,
                &feature.options,
                hpr,
                vpr,
                out,
                points,
                scale,
                rs.base_value,
            ),
            FeatureSeriesKind::PrettyHistogram => self.build_pretty_histogram_feature(
                &bars,
                &feature.options,
                hpr,
                vpr,
                out,
                scale,
                rs.base_value,
            ),
            FeatureSeriesKind::BackgroundShade => self.build_background_shade_feature(
                &bars,
                &feature.options,
                hpr,
                vpr,
                pane_top,
                pane_height,
                out,
            ),
            FeatureSeriesKind::StackedArea => self.build_stacked_area_feature(
                &bars,
                &feature.options,
                hpr,
                vpr,
                out,
                points,
                scale,
                rs.base_value,
            ),
            FeatureSeriesKind::StackedBars => self.build_stacked_bars_feature(
                &bars,
                &feature.options,
                hpr,
                vpr,
                out,
                scale,
                rs.base_value,
            ),
            FeatureSeriesKind::WhiskerBox => self.build_whisker_box_feature(
                &bars,
                &feature.options,
                hpr,
                vpr,
                out,
                scale,
                rs.base_value,
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_grouped_bars_feature(
        &self,
        bars: &[VisibleFeatureBar<'_>],
        options: &FeatureSeriesOptions,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
        base: f64,
    ) {
        let zero = scale.price_to_coordinate(0.0, base);
        let spacing = self.time_scale.bar_spacing();
        for bar in bars {
            let FeatureValue::GroupedBars { values } = bar.value else {
                continue;
            };
            let single = spacing / (values.len() as f64 + 1.0);
            let start = single / 2.0 + bar.x_media - spacing / 2.0 + single / 2.0;
            let mut previous_right = None;
            for (index, value) in values.iter().enumerate() {
                let x = start + index as f64 * single;
                let (left, width) = positions_line(x, hpr, single);
                let right = left + width;
                let draw_left = previous_right.unwrap_or(left);
                let (top, height) =
                    positions_box(zero, scale.price_to_coordinate(*value, base), vpr);
                out.push(Prim::Rect {
                    rect: IRect {
                        x: draw_left,
                        y: top,
                        w: (right - draw_left).max(1),
                        h: height,
                    },
                    color: options.colors[index % options.colors.len()],
                });
                previous_right = Some(right);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_heatmap_feature(
        &self,
        bars: &[VisibleFeatureBar<'_>],
        options: &FeatureSeriesOptions,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
        base: f64,
    ) {
        let spacing = self.time_scale.bar_spacing();
        let draw_border = spacing > options.cell_border_width * 3.0;
        let border_x = if draw_border {
            options.cell_border_width * hpr
        } else {
            0.0
        };
        let border_y = if draw_border {
            options.cell_border_width * vpr
        } else {
            0.0
        };
        for bar in bars {
            let FeatureValue::Heatmap { cells } = bar.value else {
                continue;
            };
            let (left, width) = full_bar_width(bar.x_media, spacing, hpr);
            for cell in cells {
                let low = scale.price_to_coordinate(cell.low, base);
                let high = scale.price_to_coordinate(cell.high, base);
                let (top, height) = positions_box(low, high, vpr);
                let x = left + border_x.round() as i32;
                let y = top + border_y.round() as i32;
                let w = (width - (border_x * 2.0).round() as i32).max(1);
                let h = (height - 1 - (border_y * 2.0).round() as i32).max(1);
                out.push(Prim::Rect {
                    rect: IRect { x, y, w, h },
                    color: cell.rendered_color(),
                });
                if draw_border
                    && options.cell_border_width > 0.0
                    && options.cell_border_color.a() > 0
                {
                    out.push(Prim::RectFrame {
                        rect: IRect {
                            x: left + (border_x / 2.0).round() as i32,
                            y: top + (border_y / 2.0).round() as i32,
                            w: (width - border_x.round() as i32).max(1),
                            h: (height - 1 - border_y.round() as i32).max(1),
                        },
                        border: border_x.round().max(1.0) as i32,
                        color: options.cell_border_color,
                    });
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_hlc_area_feature(
        &self,
        bars: &[VisibleFeatureBar<'_>],
        options: &FeatureSeriesOptions,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
        base: f64,
    ) {
        let mut highs = Vec::with_capacity(bars.len());
        let mut lows = Vec::with_capacity(bars.len());
        let mut closes = Vec::with_capacity(bars.len());
        for bar in bars {
            let FeatureValue::HlcArea { high, low, close } = bar.value else {
                continue;
            };
            let x = (bar.x_media * hpr) as f32;
            highs.push([x, (scale.price_to_coordinate(*high, base) * vpr) as f32]);
            lows.push([x, (scale.price_to_coordinate(*low, base) * vpr) as f32]);
            closes.push([x, (scale.price_to_coordinate(*close, base) * vpr) as f32]);
        }
        if highs.len() >= 2 {
            let high_first = points.len() as u32;
            points.extend_from_slice(&highs);
            let close_first = points.len() as u32;
            points.extend_from_slice(&closes);
            out.push(Prim::BandFill {
                line_type: LineType::Simple,
                upper_first: high_first,
                lower_first: close_first,
                point_count: highs.len() as u32,
                fill: options.area_top_color,
            });
            let close_again = points.len() as u32;
            points.extend_from_slice(&closes);
            let low_first = points.len() as u32;
            points.extend_from_slice(&lows);
            out.push(Prim::BandFill {
                line_type: LineType::Simple,
                upper_first: close_again,
                lower_first: low_first,
                point_count: lows.len() as u32,
                fill: options.area_bottom_color,
            });
        }
        push_polyline(
            out,
            points,
            &lows,
            (options.low_line_width * vpr) as f32,
            options.low_line_color,
        );
        push_polyline(
            out,
            points,
            &highs,
            (options.high_line_width * vpr) as f32,
            options.high_line_color,
        );
        push_polyline(
            out,
            points,
            &closes,
            (options.close_line_width * vpr) as f32,
            options.close_line_color,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn build_pretty_histogram_feature(
        &self,
        bars: &[VisibleFeatureBar<'_>],
        options: &FeatureSeriesOptions,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
        base: f64,
    ) {
        let zero = scale.price_to_coordinate(0.0, base);
        let width = (0.01 * options.width_percent * self.time_scale.bar_spacing() * hpr)
            .round()
            .max(1.0) as f32;
        let requested_radius = (options.radius.unwrap_or(4.0) * hpr) as f32;
        for bar in bars {
            let FeatureValue::PrettyHistogram { value, color } = bar.value else {
                continue;
            };
            let y = scale.price_to_coordinate(*value, base);
            let (top, height) = positions_box(zero, y, vpr);
            let radius = requested_radius.min(width / 2.0).min(height as f32).floor();
            out.push(Prim::RoundRect {
                x: ((bar.x_media * hpr) as f32 - width / 2.0).round(),
                y: top as f32,
                w: width,
                h: height as f32,
                radii: if y < zero {
                    [radius, radius, 0.0, 0.0]
                } else {
                    [0.0, 0.0, radius, radius]
                },
                fill: color.unwrap_or(options.color),
                border_width: 0.0,
                border_color: Color::rgba(0, 0, 0, 0),
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_background_shade_feature(
        &self,
        bars: &[VisibleFeatureBar<'_>],
        options: &FeatureSeriesOptions,
        hpr: f64,
        vpr: f64,
        pane_top: f64,
        pane_height: f64,
        out: &mut Vec<Prim>,
    ) {
        let span = options.high_value - options.low_value;
        let spacing = self.time_scale.bar_spacing();
        let field_top = (pane_top * vpr).round() as i32;
        let field_height = (pane_height * vpr).round().max(1.0) as i32;
        for bar in bars {
            let FeatureValue::BackgroundShade { value } = bar.value else {
                continue;
            };
            let amount = if span == 0.0 {
                0.0
            } else {
                (*value - options.low_value) / span
            };
            // The official example exposes `opacity` but its renderer paints the
            // interpolated RGB value directly; preserve that observable behavior.
            let color = mix_background_color(options.low_color, options.high_color, amount);
            let (x, width) = full_bar_width(bar.x_media, spacing, hpr);
            out.push(Prim::Rect {
                rect: IRect {
                    x,
                    y: field_top,
                    w: width,
                    h: field_height,
                },
                color,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_stacked_area_feature(
        &self,
        bars: &[VisibleFeatureBar<'_>],
        options: &FeatureSeriesOptions,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
        base: f64,
    ) {
        let layers = bars
            .iter()
            .filter_map(|bar| match bar.value {
                FeatureValue::StackedArea { values } => Some(values.len()),
                _ => None,
            })
            .min()
            .unwrap_or(0);
        let zero = (scale.price_to_coordinate(0.0, base) * vpr) as f32;
        let mut previous = bars
            .iter()
            .map(|bar| [(bar.x_media * hpr) as f32, zero])
            .collect::<Vec<_>>();
        for layer in 0..layers {
            let mut cumulative = Vec::with_capacity(bars.len());
            for bar in bars {
                let FeatureValue::StackedArea { values } = bar.value else {
                    continue;
                };
                let total = values[..=layer].iter().sum::<f64>();
                cumulative.push([
                    (bar.x_media * hpr) as f32,
                    (scale.price_to_coordinate(total, base) * vpr) as f32,
                ]);
            }
            if cumulative.len() >= 2 {
                let upper = points.len() as u32;
                points.extend_from_slice(&cumulative);
                let lower = points.len() as u32;
                points.extend_from_slice(&previous);
                out.push(Prim::BandFill {
                    line_type: LineType::Simple,
                    upper_first: upper,
                    lower_first: lower,
                    point_count: cumulative.len() as u32,
                    fill: options.stacked_area_colors[layer % options.stacked_area_colors.len()]
                        .area,
                });
                push_polyline(
                    out,
                    points,
                    &cumulative,
                    (options.line_width * vpr) as f32,
                    options.stacked_area_colors[layer % options.stacked_area_colors.len()].line,
                );
            }
            previous = cumulative;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_stacked_bars_feature(
        &self,
        bars: &[VisibleFeatureBar<'_>],
        options: &FeatureSeriesOptions,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
        base: f64,
    ) {
        let positions = column_positions(
            &bars.iter().map(|bar| bar.x_media).collect::<Vec<_>>(),
            self.time_scale.bar_spacing(),
            hpr,
        );
        for (bar, column) in bars.iter().zip(positions) {
            let FeatureValue::StackedBars { values } = bar.value else {
                continue;
            };
            let mut total = 0.0;
            let width = hpr
                .max((column.right - column.left) as f64)
                .min(self.time_scale.bar_spacing() * hpr) as f32;
            for (index, value) in values.iter().enumerate() {
                let previous = total;
                total += value;
                let (top, height) = positions_box(
                    scale.price_to_coordinate(previous, base),
                    scale.price_to_coordinate(total, base),
                    vpr,
                );
                let color = options.colors[index % options.colors.len()];
                out.push(Prim::RoundRect {
                    x: column.left as f32,
                    y: top as f32,
                    w: width,
                    h: height as f32,
                    radii: [0.0; 4],
                    fill: color,
                    border_width: 0.0,
                    border_color: color,
                });
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_whisker_box_feature(
        &self,
        bars: &[VisibleFeatureBar<'_>],
        options: &FeatureSeriesOptions,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
        base: f64,
    ) {
        let spacing = self.time_scale.bar_spacing();
        let body_media = optimal_candlestick_width(spacing, 1.0).max(1) as f64;
        let median_media = spacing.floor().max(body_media);
        let extreme_media = optimal_candlestick_width(spacing / 2.0, 1.0).max(1) as f64;
        let outlier_radius = body_media.min(4.0);
        let vertical_width = hpr.floor().max(1.0);
        let horizontal_height = vpr.floor().max(1.0);
        for bar in bars {
            let FeatureValue::WhiskerBox {
                quartiles,
                outliers,
            } = bar.value
            else {
                continue;
            };
            let ys = quartiles.map(|price| scale.price_to_coordinate(price, base));
            let (wick_x, wick_w) = positions_line(bar.x_media, hpr, vertical_width / hpr);
            let (top_y, top_h) = positions_box(ys[0], ys[1], vpr);
            let (bottom_y, bottom_h) = positions_box(ys[3], ys[4], vpr);
            for (y, h) in [(top_y, top_h), (bottom_y, bottom_h)] {
                out.push(Prim::Rect {
                    rect: IRect {
                        x: wick_x,
                        y,
                        w: wick_w,
                        h,
                    },
                    color: options.whisker_color,
                });
            }
            let (extreme_x, extreme_w) = positions_line(bar.x_media, hpr, extreme_media);
            for y in [ys[0], ys[4]] {
                let (line_y, line_h) = positions_line(y, vpr, horizontal_height / vpr);
                out.push(Prim::Rect {
                    rect: IRect {
                        x: extreme_x,
                        y: line_y,
                        w: extreme_w,
                        h: line_h,
                    },
                    color: options.whisker_color,
                });
            }
            let (body_x, body_w) = positions_line(bar.x_media, hpr, body_media);
            let (lower_y, lower_h) = positions_box(ys[1], ys[2], vpr);
            let (upper_y, upper_h) = positions_box(ys[2], ys[3], vpr);
            out.push(Prim::Rect {
                rect: IRect {
                    x: body_x,
                    y: lower_y,
                    w: body_w,
                    h: lower_h,
                },
                color: options.lower_quartile_fill,
            });
            out.push(Prim::Rect {
                rect: IRect {
                    x: body_x,
                    y: upper_y,
                    w: body_w,
                    h: upper_h,
                },
                color: options.upper_quartile_fill,
            });
            let (median_x, median_w) = positions_line(bar.x_media, hpr, median_media);
            let (median_y, median_h) = positions_line(ys[2], vpr, horizontal_height / vpr);
            out.push(Prim::Rect {
                rect: IRect {
                    x: median_x,
                    y: median_y,
                    w: median_w,
                    h: median_h,
                },
                color: options.whisker_color,
            });
            if outlier_radius > 2.0 {
                for outlier in outliers {
                    out.push(Prim::Circle {
                        cx: (bar.x_media * hpr).round() as f32,
                        cy: (scale.price_to_coordinate(*outlier, base) * vpr).round() as f32,
                        radius: outlier_radius as f32,
                        fill: options.outlier_color,
                        stroke_width: 0.0,
                        stroke: options.outlier_color,
                    });
                }
            }
        }
    }
}
