use super::*;
use crate::feature_series::{FeatureSeriesKind, FeatureSeriesOptions, FeatureValue};
use nucleuscharts_render::bar_width::{apply_crosshair_parity, optimal_candlestick_width};

#[derive(Clone, Copy)]
struct VisibleFeatureBar<'a> {
    row: usize,
    logical: i64,
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

fn mix_color(low: Color, high: Color, amount: f64) -> Color {
    let t = amount.clamp(0.0, 1.0);
    let channel = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
    Color::rgba(
        channel(low.r(), high.r()),
        channel(low.g(), high.g()),
        channel(low.b(), high.b()),
        channel(low.a(), high.a()),
    )
}

fn official_heatmap_color(amount: f64) -> Color {
    let amount = amount.clamp(0.0, 100.0);
    Color::rgba(
        0,
        (100.0 + amount * 1.55).round().clamp(0.0, 255.0) as u8,
        amount.round().clamp(0.0, 255.0) as u8,
        ((0.2 + amount * 0.8).clamp(0.0, 1.0) * 255.0).round() as u8,
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
                    row,
                    logical,
                    x_media: self.time_scale.index_to_coordinate(logical),
                    value,
                })
            })
            .collect::<Vec<_>>();
        if bars.is_empty() {
            return;
        }
        match feature.kind {
            FeatureSeriesKind::BrushableArea => self.build_brushable_area_feature(
                &bars,
                &feature.options,
                hpr,
                vpr,
                pane_top + pane_height,
                out,
                points,
                scale,
                rs.base_value,
            ),
            FeatureSeriesKind::DualRangeHistogram => self.build_dual_range_feature(
                &bars,
                &feature.options,
                hpr,
                vpr,
                out,
                scale,
                rs.base_value,
            ),
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
            FeatureSeriesKind::RoundedCandles => self.build_rounded_candles_feature(
                &bars,
                &feature.rows,
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
    fn build_brushable_area_feature(
        &self,
        bars: &[VisibleFeatureBar<'_>],
        options: &FeatureSeriesOptions,
        hpr: f64,
        vpr: f64,
        pane_bottom: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
        base: f64,
    ) {
        let style_for = |logical: i64| {
            options
                .brush_ranges
                .iter()
                .find(|range| {
                    let start = range.from.min(range.to);
                    let end = range.from.max(range.to);
                    logical as f64 >= start && (logical as f64) < end
                })
                .map(|range| range.style)
                .unwrap_or(crate::BrushStyle {
                    line_color: options.line_color,
                    top_color: options.top_color,
                    bottom_color: options.bottom_color,
                    line_width: options.line_width,
                })
        };
        // The reference creates every style gradient over one shared vertical span: the minimum
        // visible line y through the pane bottom. `AreaFill` shades over its own segment bounds,
        // so pre-interpolate the segment's top stop into that shared ramp; linear interpolation
        // from the adjusted stop to the common bottom is then pixel-identical without extending
        // the backend-neutral primitive contract.
        let global_top = bars
            .iter()
            .filter_map(|bar| match bar.value {
                FeatureValue::BrushableArea { value } => {
                    Some(scale.price_to_coordinate(*value, base))
                }
                _ => None,
            })
            .fold(f64::INFINITY, f64::min);
        let global_span = (pane_bottom - global_top).max(1.0);
        for pair in bars.windows(2) {
            let [left, right] = pair else { continue };
            let (
                FeatureValue::BrushableArea { value: left_value },
                FeatureValue::BrushableArea { value: right_value },
            ) = (left.value, right.value)
            else {
                continue;
            };
            let style = style_for(right.logical);
            let left_y = scale.price_to_coordinate(*left_value, base);
            let right_y = scale.price_to_coordinate(*right_value, base);
            let segment_top = left_y.min(right_y);
            let adjusted_top = mix_color(
                style.top_color,
                style.bottom_color,
                (segment_top - global_top) / global_span,
            );
            let segment = [
                [(left.x_media * hpr).round() as f32, (left_y * vpr) as f32],
                [(right.x_media * hpr).round() as f32, (right_y * vpr) as f32],
            ];
            let first = points.len() as u32;
            points.extend_from_slice(&segment);
            out.push(Prim::AreaFill {
                first_point: first,
                point_count: 2,
                base_y: (pane_bottom * vpr) as f32,
                line_type: LineType::Simple,
                gradient: Gradient {
                    top: adjusted_top,
                    bottom: style.bottom_color,
                },
            });
        }

        // The reference strokes one path per contiguous style run. Keeping those runs intact
        // preserves joins and avoids a cap/seam at every bar.
        let mut run_style: Option<crate::BrushStyle> = None;
        let mut run = Vec::<[f32; 2]>::new();
        for pair in bars.windows(2) {
            let [left, right] = pair else { continue };
            let (
                FeatureValue::BrushableArea { value: left_value },
                FeatureValue::BrushableArea { value: right_value },
            ) = (left.value, right.value)
            else {
                continue;
            };
            let style = style_for(right.logical);
            let left_point = [
                (left.x_media * hpr).round() as f32,
                (scale.price_to_coordinate(*left_value, base) * vpr) as f32,
            ];
            let right_point = [
                (right.x_media * hpr).round() as f32,
                (scale.price_to_coordinate(*right_value, base) * vpr) as f32,
            ];
            if run_style.is_some_and(|current| current != style) {
                let current = run_style.unwrap();
                push_polyline(
                    out,
                    points,
                    &run,
                    (current.line_width * vpr) as f32,
                    current.line_color,
                );
                run.clear();
            }
            if run.is_empty() {
                run.push(left_point);
            }
            run.push(right_point);
            run_style = Some(style);
        }
        if let Some(style) = run_style {
            push_polyline(
                out,
                points,
                &run,
                (style.line_width * vpr) as f32,
                style.line_color,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_dual_range_feature(
        &self,
        bars: &[VisibleFeatureBar<'_>],
        options: &FeatureSeriesOptions,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
        base: f64,
    ) {
        let maximum = bars
            .iter()
            .filter_map(|bar| match bar.value {
                FeatureValue::DualRangeHistogram { values } => Some(values.as_slice()),
                _ => None,
            })
            .flatten()
            .map(|value| value.abs())
            .fold(0.0, f64::max);
        if maximum == 0.0 {
            return;
        }
        let positions = column_positions(
            &bars.iter().map(|bar| bar.x_media).collect::<Vec<_>>(),
            self.time_scale.bar_spacing(),
            hpr,
        );
        let zero = scale.price_to_coordinate(0.0, base);
        let border_width = if self.time_scale.bar_spacing() * hpr < 4.0 {
            0.0
        } else {
            (0.5 * hpr).max(1.0)
        } as f32;
        for (bar, column) in bars.iter().zip(positions) {
            let FeatureValue::DualRangeHistogram { values } = bar.value else {
                continue;
            };
            let width = hpr
                .max((column.right - column.left) as f64)
                .min(self.time_scale.bar_spacing() * hpr) as f32;
            for (index, value) in values.iter().enumerate() {
                let y =
                    zero - value.signum() * (value.abs() / maximum) * (options.max_height / 2.0);
                let (top, height) = positions_box(zero, y, vpr);
                let requested = options.border_radius[index % options.border_radius.len()] * vpr;
                let radius = requested.min(width as f64 / 2.0).min(height as f64).floor() as f32;
                let radii = if *value >= 0.0 {
                    [radius, radius, 0.0, 0.0]
                } else {
                    [0.0, 0.0, radius, radius]
                };
                let color = options.colors[index % options.colors.len()];
                out.push(Prim::RoundRect {
                    x: column.left as f32,
                    y: top as f32,
                    w: width,
                    h: height as f32,
                    radii,
                    fill: color,
                    border_width,
                    border_color: Color::rgba(0, 0, 0, 0),
                });
            }
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
                    color: cell
                        .color
                        .unwrap_or_else(|| official_heatmap_color(cell.amount)),
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
    fn build_rounded_candles_feature(
        &self,
        bars: &[VisibleFeatureBar<'_>],
        all_rows: &[crate::feature_series::FeatureRow],
        options: &FeatureSeriesOptions,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
        base: f64,
    ) {
        let mut previous_close = f64::NEG_INFINITY;
        let first_row = bars.first().map_or(0, |bar| bar.row);
        for row in all_rows.iter().take(first_row) {
            if let Some(FeatureValue::RoundedCandles { close, .. }) = &row.value {
                previous_close = *close;
            }
        }
        let spacing = self.time_scale.bar_spacing();
        let body_width =
            apply_crosshair_parity(optimal_candlestick_width(spacing, 1.0), 1.0).max(1) as f64;
        let wick_width = hpr.floor().max(1.0) / hpr;
        let radius = options
            .radius
            .unwrap_or(if spacing < 4.0 { 0.0 } else { spacing / 3.0 }) as f32;
        for bar in bars {
            let FeatureValue::RoundedCandles {
                open,
                high,
                low,
                close,
            } = bar.value
            else {
                continue;
            };
            let rising = *close >= previous_close;
            previous_close = *close;
            let body_color = if rising {
                options.up_color
            } else {
                options.down_color
            };
            let wick_color = if rising {
                options.wick_up_color
            } else {
                options.wick_down_color
            };
            // `RoundedCandleSeriesRenderer._drawWicks` in the official example does not consult
            // the inherited `wickVisible` option. Preserve that observable plugin behavior.
            let high_y = scale.price_to_coordinate(*high, base);
            let low_y = scale.price_to_coordinate(*low, base);
            let (top, height) = positions_box(high_y, low_y, vpr);
            let (x, width) = positions_line(bar.x_media, hpr, wick_width);
            out.push(Prim::Rect {
                rect: IRect {
                    x,
                    y: top,
                    w: width,
                    h: height,
                },
                color: wick_color,
            });
            let open_y = scale.price_to_coordinate(*open, base);
            let close_y = scale.price_to_coordinate(*close, base);
            let (top, height) = positions_box(open_y, close_y, vpr);
            let (x, width) = positions_line(bar.x_media, hpr, body_width);
            // Canvas `roundRect` proportionally normalizes an oversized uniform radius to fit
            // the rectangle. Normalize once in the shared frame so Canvas2D, WebGPU, and GPUI
            // execute the same official geometry.
            let radius = radius.min(width as f32 / 2.0).min(height as f32 / 2.0);
            out.push(Prim::RoundRect {
                x: x as f32,
                y: top as f32,
                w: width as f32,
                h: height as f32,
                radii: [radius; 4],
                fill: body_color,
                border_width: 0.0,
                border_color: body_color,
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
        let samples = bars
            .iter()
            .filter_map(|bar| {
                let FeatureValue::BackgroundShade { value } = bar.value else {
                    return None;
                };
                let amount = if span == 0.0 {
                    0.0
                } else {
                    (*value - options.low_value) / span
                };
                // The official example exposes `opacity` but its renderer paints the
                // interpolated RGB value directly; preserve that observable behavior.
                let color = mix_color(options.low_color, options.high_color, amount);
                Some((bar.x_media, Color::rgb(color.r(), color.g(), color.b())))
            })
            .collect::<Vec<_>>();
        let Some(&(first_x, first_color)) = samples.first() else {
            return;
        };
        let Some(&(last_x, last_color)) = samples.last() else {
            return;
        };

        let spacing = self.time_scale.bar_spacing();
        let (first_left, _) = full_bar_width(first_x, spacing, hpr);
        let (last_left, last_width) = full_bar_width(last_x, spacing, hpr);
        let field_top = (pane_top * vpr).round() as i32;
        let field_height = (pane_height * vpr).round().max(1.0) as i32;
        let mut run_start = first_left;
        let mut run_color = first_color;
        let push_run = |out: &mut Vec<Prim>, start: i32, end: i32, color: Color| {
            if end > start {
                out.push(Prim::Rect {
                    rect: IRect {
                        x: start,
                        y: field_top,
                        w: end - start,
                        h: field_height,
                    },
                    color,
                });
            }
        };

        // Interpolate in bitmap space so the backdrop is a continuous field whose work remains
        // bounded by the physical viewport width, not one flat color pocket per source bar.
        for pair in samples.windows(2) {
            let [(left_x, left_color), (right_x, right_color)] = pair else {
                continue;
            };
            let left = (*left_x * hpr).round() as i32;
            let right = (*right_x * hpr).round() as i32;
            if right <= left {
                continue;
            }
            for x in left.max(run_start)..right {
                let color = mix_color(
                    *left_color,
                    *right_color,
                    (x - left) as f64 / (right - left) as f64,
                );
                if color != run_color {
                    push_run(out, run_start, x, run_color);
                    run_start = x;
                    run_color = color;
                }
            }
        }
        let last_center = (last_x * hpr).round() as i32;
        if run_color != last_color {
            push_run(out, run_start, last_center, run_color);
            run_start = last_center;
            run_color = last_color;
        }
        push_run(out, run_start, last_left + last_width, run_color);
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
