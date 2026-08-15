//! Axis frame production: price/time labels, widths, marker/price-line/crosshair labels.

use super::*;

const LIVE_LABEL_TEXT: Color = Color::rgb(
    nucleuscharts_core::style::DARK_FOREGROUND_RGB.0,
    nucleuscharts_core::style::DARK_FOREGROUND_RGB.1,
    nucleuscharts_core::style::DARK_FOREGROUND_RGB.2,
);
const LIVE_LABEL_COUNTDOWN_TEXT: Color = Color::rgba(
    nucleuscharts_core::style::DARK_FOREGROUND_RGB.0,
    nucleuscharts_core::style::DARK_FOREGROUND_RGB.1,
    nucleuscharts_core::style::DARK_FOREGROUND_RGB.2,
    0xb3,
);
const COUNTDOWN_FONT_SCALE: f64 = 11.0 / 12.0;

/// A last-value label candidate before axis overlap resolution (reference IPriceAxisView state:
/// the source `coordinate` plus the render coordinate the overlap pass adjusts). `align`
/// is the owning scale's `alignLabels` — a scale with it off leaves its labels at their raw
/// coordinates (price-axis-widget.ts:633 early-return).
///
/// The candidate is a TradingView-style CLUSTER of up to three independently-toggleable parts:
/// the title chip (a darker shade of the label color, left of the price text), the price text,
/// and a candle-close countdown row stacked below. `y`/`height` describe the whole cluster
/// (center + total height), which is what the overlap pass spaces; `top_height` is the title +
/// price row's share (`0` when both are hidden, e.g. a countdown-only cluster).
struct LastValueLabel {
    /// Price text; `None` when the series' `lastValueVisible` is off (the cluster can still
    /// render its title chip and/or countdown row).
    price_text: Option<String>,
    /// Title chip text (the series' `title`); `None` when unset or `title_visible` is off.
    title: Option<String>,
    /// Countdown row text; `None` when `countdown_visible` is off, the series has no usable
    /// bar interval, or no host clock is installed.
    countdown: Option<String>,
    /// The owning series' id — the attach group of this cluster's chips, so its price and
    /// countdown chips paint with a shared edge WITHOUT chaining into another series' cluster
    /// (a shared constant chained every cluster on the axis into one giant box).
    group_id: u32,
    y: f64,
    height: f64,
    top_height: f64,
    color: Color,
    align: bool,
}

/// Median of the last up-to-10 inter-bar deltas of a series' bar times (fallback: with a single
/// delta the median IS that delta); `None` with fewer than two usable bars, which hides the
/// countdown row. Non-positive deltas (duplicate times) are skipped.
pub(crate) fn median_bar_interval(times: &[i64]) -> Option<f64> {
    let tail = &times[times.len().saturating_sub(11)..];
    let mut deltas: Vec<i64> = tail
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|&d| d > 0)
        .collect();
    if deltas.is_empty() {
        return None;
    }
    deltas.sort_unstable();
    let n = deltas.len();
    Some(if n % 2 == 1 {
        deltas[n / 2] as f64
    } else {
        (deltas[n / 2 - 1] as f64 + deltas[n / 2] as f64) / 2.0
    })
}

/// TradingView countdown format by remaining magnitude: `mm:ss` zero-padded below an hour,
/// `hh:mm:ss` below a day, `"Xd Xh"` at a day and beyond.
pub(crate) fn format_countdown_remaining(remaining: f64) -> String {
    let secs = remaining.max(0.0).floor() as u64;
    if secs < 3600 {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    } else if secs < 86400 {
        format!(
            "{:02}:{:02}:{:02}",
            secs / 3600,
            secs % 3600 / 60,
            secs % 60
        )
    } else {
        format!("{}d {}h", secs / 86400, secs % 86400 / 3600)
    }
}

/// Minimal port of reference price-axis-widget.ts `_fixLabelOverlap` + `recalculateOverlapping`
/// for the last-value labels of one axis side: split the labels around the center label
/// (the first series', the reference's `centerSource`), clamp edge labels into the viewport, then push
/// overlapping labels apart outward from the center, shifting whole groups back when they
/// would fall off the scale. Only labels from `alignLabels` scales participate (reference gates
/// the whole pass on that option); the rest keep their raw coordinates. Fewer than two
/// aligned labels are left untouched.
fn resolve_last_value_label_overlap(labels: &mut [LastValueLabel], scale_height: f64) {
    let aligned: Vec<usize> = (0..labels.len()).filter(|&i| labels[i].align).collect();
    if aligned.len() < 2 {
        return;
    }
    let center = labels[aligned[0]].y;
    // Split around the center and sort each side toward it (reference sorts by the source
    // coordinate, so capture the order before any adjustment).
    let mut top: Vec<usize> = aligned
        .iter()
        .copied()
        .filter(|&i| labels[i].y <= center)
        .collect();
    top.sort_by(|&a, &b| labels[b].y.total_cmp(&labels[a].y)); // center-to-top
    let mut bottom: Vec<usize> = aligned
        .iter()
        .copied()
        .filter(|&i| labels[i].y > center)
        .collect();
    if !top.is_empty() && !bottom.is_empty() {
        bottom.push(top[0]); // share the center label between both passes
    }
    bottom.sort_by(|&a, &b| labels[a].y.total_cmp(&labels[b].y));
    // Edge clamp (price-axis-widget.ts:659-669): a label half-off the scale snaps fully inside.
    for &i in &aligned {
        let label = &mut labels[i];
        let half = (label.height / 2.0).floor();
        if label.y > -half && label.y < half {
            label.y = half;
        }
        if label.y > scale_height - half && label.y < scale_height + half {
            label.y = scale_height - half;
        }
    }
    recalculate_overlapping(labels, &top, 1.0, scale_height);
    recalculate_overlapping(labels, &bottom, -1.0, scale_height);
}

/// reference `recalculateOverlapping` (price-axis-widget.ts:77-121): walk the labels outward from
/// the center (`direction` 1 = toward the top, -1 = toward the bottom) and push each
/// overlapping label past its predecessor; when a pushed group would leave the viewport,
/// shift the whole group back by the space that was free before it.
fn recalculate_overlapping(
    labels: &mut [LastValueLabel],
    order: &[usize],
    direction: f64,
    scale_height: f64,
) {
    if order.is_empty() {
        return;
    }
    let first = order[0];
    let init_height = labels[first].height;
    let mut space_before_group = (if direction > 0.0 {
        scale_height / 2.0 - (labels[first].y - init_height / 2.0)
    } else {
        labels[first].y - init_height / 2.0 - scale_height / 2.0
    })
    .max(0.0);
    let mut group_start = 0usize;
    for i in 1..order.len() {
        let view = order[i];
        let prev = order[i - 1];
        let height = labels[prev].height;
        let overlap = if direction > 0.0 {
            labels[view].y > labels[prev].y - height
        } else {
            labels[view].y < labels[prev].y + height
        };
        if overlap {
            let render_y = labels[prev].y - height * direction;
            labels[view].y = render_y;
            let edge_point = render_y - direction * height / 2.0;
            let out_of_viewport = if direction > 0.0 {
                edge_point < 0.0
            } else {
                edge_point > scale_height
            };
            if out_of_viewport && space_before_group > 0.0 {
                let desired_shift = if direction > 0.0 {
                    -1.0 - edge_point
                } else {
                    edge_point - scale_height
                };
                let shift = desired_shift.min(space_before_group);
                for &k in &order[group_start..] {
                    labels[k].y += direction * shift;
                }
                space_before_group -= shift;
            }
        } else {
            group_start = i;
            space_before_group = if direction > 0.0 {
                labels[prev].y - height - labels[view].y
            } else {
                labels[view].y - (labels[prev].y + height)
            };
        }
    }
}

impl ChartEngine {
    fn primary_text_color(&self) -> Color {
        let fallback = nucleuscharts_core::style::DEFAULT_FOREGROUND_RGB;
        Color::parse_css(&self.options.get().layout.text_color)
            .unwrap_or(Color::rgb(fallback.0, fallback.1, fallback.2))
    }

    pub(super) fn format_scale_value(&self, scale: &PriceScaleCore, value: f64) -> String {
        if scale.mode() == PriceScaleMode::Percentage {
            // Percentage mode has its own formatter; the host price formatter does not apply here
            // (matching reference, where percentage display is independent of `priceFormatter`).
            return PercentageFormatter::default().format(value);
        }
        if let Some(f) = &self.price_formatter_fn {
            if let Some(s) = f(value) {
                return s;
            }
        }
        self.price_formatter.format(value)
    }

    /// Format one value through a series' `priceFormat` (reference series.ts `_recreateFormatter`):
    /// volume = K/M/B suffixes, percent = `%` sign, price = precision/min_move decimals,
    /// custom = the installed host fn. Returns `None` when the format defers to the
    /// scale/chart-level resolution (a factory-default price format, or a custom format with
    /// no/declining fn).
    pub(crate) fn format_with_price_format(
        &self,
        format: &SeriesPriceFormat,
        value: f64,
    ) -> Option<String> {
        match format.kind {
            // reference custom: `formatTickmarks` defaults to mapping the formatter over the values,
            // so one per-value fn serves ticks and labels alike; a `None` return (the callback
            // threw at the boundary) defers to the built-in fallback.
            PriceFormatKind::Custom => format.formatter.as_ref().and_then(|f| f(value)),
            PriceFormatKind::Volume => Some(VolumeFormatter::new(format.precision).format(value)),
            // reference wires `PercentageFormatter(precision)` — passing the precision as the raw
            // price scale, which reads like an upstream quirk (precision 2 -> one decimal).
            // We treat precision as decimal digits (10^precision), matching the percentage
            // scale mode's output at the default precision 2.
            PriceFormatKind::Percent => Some(
                PercentageFormatter::with_price_scale(10i64.pow(format.precision)).format(value),
            ),
            PriceFormatKind::Price => {
                if format.is_reference_default() {
                    None
                } else {
                    Some(
                        PriceFormatter::from_precision(format.precision, format.min_move)
                            .format(value),
                    )
                }
            }
        }
    }

    /// A series' OWN format drives its last-value label, its price-line labels, and the
    /// crosshair price label when the series is the label source. the reference's
    /// `localization.priceFormatter` keeps precedence when installed (price-scale.ts
    /// `_formatValue` consults it before the scale/series formatter).
    pub(super) fn format_series_value(
        &self,
        series: &crate::SeriesEntry,
        scale: &PriceScaleCore,
        value: f64,
    ) -> String {
        if scale.mode() == PriceScaleMode::Percentage {
            return PercentageFormatter::default().format(value);
        }
        if let Some(f) = &self.price_formatter_fn {
            if let Some(s) = f(value) {
                return s;
            }
        }
        self.format_with_price_format(&series.price_format, value)
            .unwrap_or_else(|| self.price_formatter.format(value))
    }

    /// Axis TICK label formatting: the format of the scale's primary source — the first
    /// visible, non-overlay series bound to that scale (reference uses the scale's main source for
    /// ticks: price-scale.ts `updateFormatter` picks the lowest-zorder data source). A primary
    /// source with the factory-default price format defers to the chart-level/built-in
    /// formatter, exactly like before per-series formats existed.
    pub(super) fn format_tick_value(
        &self,
        pane_index: usize,
        target: PriceScaleTarget,
        scale: &PriceScaleCore,
        value: f64,
    ) -> String {
        if scale.mode() == PriceScaleMode::Percentage {
            return PercentageFormatter::default().format(value);
        }
        let primary = self.series.iter().find(|s| {
            s.visible
                && !s.overlay
                && s.pane_index == pane_index
                && series_scale_target(s) == target
        });
        if let Some(series) = primary {
            if let Some(s) = self.format_with_price_format(&series.price_format, value) {
                return s;
            }
        }
        self.format_scale_value(scale, value)
    }

    /// Time-axis tick label, honoring a host `tickMarkFormatter` when installed. Month
    /// labels use the locale month-name table (reference `localization.locale`).
    pub(super) fn format_time_tick(&self, ts: i64, kind: TickMarkType) -> String {
        if let Some(f) = &self.tick_mark_formatter_fn {
            if let Some(s) = f(ts, kind as u8) {
                return s;
            }
        }
        format_tick_label_with(ts, kind, &self.month_names)
    }

    /// Crosshair time label, honoring a host `timeFormatter` when installed. Otherwise the
    /// engine's `localization.dateFormat` pattern with the locale month-name table (reference
    /// chart-options-defaults.ts:34-37).
    pub(super) fn format_crosshair_ts(&self, ts: i64) -> String {
        if let Some(f) = &self.time_formatter_fn {
            if let Some(s) = f(ts) {
                return s;
            }
        }
        format_crosshair_time_with(
            ts,
            self.time_visible,
            self.seconds_visible,
            &self.date_format,
            &self.month_names,
        )
    }

    /// Nucleus extension: bold round-figure price tick labels (TradingView decile rule). Uniform
    /// ticks: the value is a multiple of `step × 10`. Non-uniform (log-style) ticks: the value
    /// is an exact power of ten.
    pub(crate) fn bold_round_decisions(logicals: &[f64], enabled: bool) -> Vec<bool> {
        if !enabled || logicals.is_empty() {
            return vec![false; logicals.len()];
        }
        let step = logicals
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(f64::INFINITY, f64::min);
        let uniform = step.is_finite()
            && step > 0.0
            && logicals
                .windows(2)
                .all(|w| ((w[1] - w[0]).abs() - step).abs() <= step * 1e-6 + 1e-12);
        logicals
            .iter()
            .map(|&v| {
                if uniform {
                    let ratio = v / step;
                    let nearest = ratio.round();
                    (ratio - nearest).abs() < 1e-6 && (nearest as i64) % 10 == 0
                } else {
                    v != 0.0 && {
                        let log = v.abs().log10();
                        (log.round() - log).abs() < 1e-9
                    }
                }
            })
            .collect()
    }

    /// Build backend-neutral axis label decisions. The host supplies only font measurement; all
    /// visible ranges, scale choices, snapping, formatting, and label positions come from the
    /// engine so Canvas2D, WebGPU text, and native glyph backends share one layout result.
    pub fn build_axis_frame<F>(&mut self, max_label_width: f64, measure: F) -> AxisFrame
    where
        F: Fn(&str) -> f64,
    {
        self.sync_frame_input_invalidation();
        let frame = self.build_axis_frame_impl(max_label_width, measure, true);
        self.retained_frame.axis_generation = self.frame_invalidation.axis;
        frame
    }

    /// `include_transient` gates the crosshair labels: they paint per frame, but the axis-width
    /// negotiation must never see them — a wide hovered price would inflate the strip and the
    /// grow-fast/shrink-lazy policy would pin that width forever.
    fn build_axis_frame_impl<F>(
        &mut self,
        max_label_width: f64,
        measure: F,
        include_transient: bool,
    ) -> AxisFrame
    where
        F: Fn(&str) -> f64,
    {
        let uninitialized = !self.retained_frame.initialized;
        if uninitialized || self.retained_frame.layout_generation != self.frame_invalidation.layout
        {
            self.layout_for_frame();
            self.retained_frame.layout_generation = self.frame_invalidation.layout;
            self.frame_build_stats.layout_rebuilds += 1;
        }
        if uninitialized
            || self.retained_frame.autoscale_generation != self.frame_invalidation.autoscale
        {
            self.autoscale_visible();
        }
        self.retained_frame.initialized = true;
        let mut out = AxisFrame {
            separator_hover: self.separator_hover,
            ..AxisFrame::default()
        };
        let visible = self.visible_range_for_frame();
        let layout_text_color = self.primary_text_color();
        // Per-scale label color (reference `textColor`): the scale's own color when set, else the
        // layout text color (price-axis-widget.ts:569).
        let scale_text_color = |scale: &PriceScaleCore| {
            scale
                .options()
                .text_color
                .as_deref()
                .and_then(Color::parse_css)
                .unwrap_or(layout_text_color)
        };
        let right_scale_visible = self.options.get().right_price_scale.visible;
        let left_scale_visible = self.options.get().left_price_scale.visible;
        let font_size = self.options.get().layout.font_size;
        let right_text_x = self.pane_left + self.pane_w + 5.0 + 5.0;
        let left_text_x = (self.pane_left - 5.0 - 5.0).max(0.0);
        for (pi, pane) in self.panes.iter().enumerate() {
            if right_scale_visible {
                // reference `entireTextOnly`: corner marks shift in by half the font height so no
                // label text is clipped (price-tick-mark-builder.ts:71).
                let entire_margin = if pane.price_scale.options().entire_text_only {
                    pane.price_scale.options().font_size / 2.0
                } else {
                    0.0
                };
                let text_color = scale_text_color(&pane.price_scale);
                let ticks_visible = pane.price_scale.options().ticks_visible;
                let marks = pane.price_scale.build_tick_marks(100, entire_margin);
                let bold_round = Self::bold_round_decisions(
                    &marks.iter().map(|m| m.logical).collect::<Vec<_>>(),
                    pane.price_scale.options().bold_round_labels,
                );
                for (mark, bold) in marks.iter().zip(bold_round) {
                    let y = mark.coord;
                    if y >= pane.top - 0.5 && y <= pane.top + pane.height + 0.5 {
                        if ticks_visible {
                            out.price_ticks.push(PriceAxisTick { y, left: false });
                        }
                        out.labels.push(AxisLabel {
                            text: self.format_tick_value(
                                pi,
                                PriceScaleTarget::Right,
                                &pane.price_scale,
                                mark.logical,
                            ),
                            x: right_text_x,
                            y,
                            color: text_color,
                            align: AxisTextAlign::Left,
                            midpoint: AxisTextMidpoint::Label,
                            font_scale: 1.0,
                            bold,
                            background: None,
                            background_corners: AxisLabelCorners::NONE,
                            measure_extra: 0.0,
                            attach_group: None,
                            border: None,
                        });
                    }
                }
            }
            if left_scale_visible {
                let entire_margin = if pane.left_scale.options().entire_text_only {
                    pane.left_scale.options().font_size / 2.0
                } else {
                    0.0
                };
                let text_color = scale_text_color(&pane.left_scale);
                let ticks_visible = pane.left_scale.options().ticks_visible;
                let marks = pane.left_scale.build_tick_marks(100, entire_margin);
                let bold_round = Self::bold_round_decisions(
                    &marks.iter().map(|m| m.logical).collect::<Vec<_>>(),
                    pane.left_scale.options().bold_round_labels,
                );
                for (mark, bold) in marks.iter().zip(bold_round) {
                    let y = mark.coord;
                    if y >= pane.top - 0.5 && y <= pane.top + pane.height + 0.5 {
                        if ticks_visible {
                            out.price_ticks.push(PriceAxisTick { y, left: true });
                        }
                        out.labels.push(AxisLabel {
                            text: self.format_tick_value(
                                pi,
                                PriceScaleTarget::Left,
                                &pane.left_scale,
                                mark.logical,
                            ),
                            x: left_text_x,
                            y,
                            color: text_color,
                            align: AxisTextAlign::Right,
                            midpoint: AxisTextMidpoint::Label,
                            font_scale: 1.0,
                            bold,
                            background: None,
                            background_corners: AxisLabelCorners::NONE,
                            measure_extra: 0.0,
                            attach_group: None,
                            border: None,
                        });
                    }
                }
            }
        }
        if let Some((from, to)) = visible {
            let time_marks = self.time_marks(max_label_width);
            let maximum_weight = time_marks
                .iter()
                .map(|(_, weight)| *weight)
                .max()
                .unwrap_or(0);
            let times = self.data.merged_times();
            for &(index, weight) in &time_marks {
                if index < from || index > to {
                    continue;
                }
                let ts = times[index as usize];
                // A hidden time axis (reference `timeScale.visible` false) drops its whole strip,
                // tick labels and tick stubs included.
                if !self.time_axis_visible {
                    continue;
                }
                let x = self.pane_left + self.time_scale.index_to_coordinate(index);
                if self.time_ticks_visible {
                    out.time_ticks.push(x);
                }
                let kind =
                    weight_to_tick_mark_type(weight, self.time_visible, self.seconds_visible);
                out.labels.push(AxisLabel {
                    text: self.format_time_tick(ts, kind),
                    x,
                    y: self.pane_h + 1.0 + 5.0 + 3.0 + font_size / 2.0,
                    color: layout_text_color,
                    align: AxisTextAlign::Center,
                    midpoint: AxisTextMidpoint::None,
                    font_scale: 1.0,
                    // reference `timeScale.allowBoldLabels` (default true): bold major labels.
                    bold: self.time_scale.options().allow_bold_labels && weight >= maximum_weight,
                    background: None,
                    background_corners: AxisLabelCorners::NONE,
                    measure_extra: 0.0,
                    attach_group: None,
                    border: None,
                });
            }
            self.append_native_vertical_line_labels(&mut out.labels, &measure);
        }
        self.append_rectangle_drawing_axis_views(&mut out, &measure);
        self.append_price_line_labels(&mut out.labels, &measure);
        self.append_drawing_line_labels(&mut out.labels, &measure);
        self.append_last_value_label(&mut out.labels, &measure);
        self.append_trading_axis_labels(&mut out.labels, &measure);
        if include_transient {
            self.append_crosshair_labels(&mut out.labels, &measure);
            self.append_native_user_price_alert_crosshair_labels(&mut out.labels, &measure);
        }
        out.separators = self
            .panes
            .iter()
            .skip(1)
            .map(|p| p.top - PANE_SEPARATOR)
            .collect();
        out
    }

    fn append_rectangle_drawing_axis_views<F>(&self, out: &mut AxisFrame, measure: &F)
    where
        F: Fn(&str) -> f64,
    {
        for drawing in &self.drawings {
            if drawing.kind == DrawingKind::Rectangle && drawing.points.len() == 2 {
                self.append_rectangle_axis_view(drawing, &drawing.points, false, out, measure);
            }
        }
        let Some(pending) = self.pending_drawing() else {
            return;
        };
        if pending.drawing.kind != DrawingKind::Rectangle {
            return;
        }
        let mut points = pending.drawing.points.clone();
        if points.len() < 2 {
            if let Some(preview) = pending.preview {
                points.push(preview);
            }
        }
        if points.len() == 2 {
            self.append_rectangle_axis_view(&pending.drawing, &points, true, out, measure);
        }
    }

    fn append_rectangle_axis_view<F>(
        &self,
        drawing: &crate::Drawing,
        points: &[crate::DrawingPoint],
        preview: bool,
        out: &mut AxisFrame,
        measure: &F,
    ) where
        F: Fn(&str) -> f64,
    {
        let Some(pane) = self.panes.get(drawing.pane_index) else {
            return;
        };
        let (Some(first), Some(second)) = (
            self.drawing_to_px_for(drawing.pane_index, drawing.price_scale, points[0]),
            self.drawing_to_px_for(drawing.pane_index, drawing.price_scale, points[1]),
        ) else {
            return;
        };
        let stroke = Color::parse_css(&drawing.color).unwrap_or(PRIMARY);
        let fill = if preview {
            drawing
                .preview_fill_color
                .as_deref()
                .or(drawing.fill_color.as_deref())
        } else {
            drawing.fill_color.as_deref()
        }
        .and_then(Color::parse_css)
        .unwrap_or(Color::rgba(stroke.r(), stroke.g(), stroke.b(), 51));
        let band_color = Color::rgba(
            fill.r(),
            fill.g(),
            fill.b(),
            u16::from(fill.a()).div_ceil(2) as u8,
        );

        if drawing.axis_bands_visible {
            let top = first.1.min(second.1).max(pane.top);
            let bottom = first.1.max(second.1).min(pane.top + pane.height);
            if bottom >= top {
                let left_visible = self.options.get().left_price_scale.visible
                    && self.left_axis_w > 0.0
                    && matches!(
                        drawing.price_scale,
                        crate::DrawingPriceScale::Left | crate::DrawingPriceScale::Overlay
                    );
                let right_visible = self.options.get().right_price_scale.visible
                    && self.axis_w > 0.0
                    && matches!(
                        drawing.price_scale,
                        crate::DrawingPriceScale::Right | crate::DrawingPriceScale::Overlay
                    );
                if left_visible {
                    out.bands.push(AxisBand {
                        x: self.pane_left - self.left_axis_w.min(15.0),
                        y: top,
                        width: self.left_axis_w.min(15.0),
                        height: (bottom - top).max(1.0 / self.dpr),
                        color: band_color,
                    });
                }
                if right_visible {
                    out.bands.push(AxisBand {
                        x: self.pane_left + self.pane_w,
                        y: top,
                        width: self.axis_w.min(15.0),
                        height: (bottom - top).max(1.0 / self.dpr),
                        color: band_color,
                    });
                }
            }
            if self.time_axis_visible {
                let left = first.0.min(second.0).max(0.0);
                let right = first.0.max(second.0).min(self.pane_w);
                if right >= left {
                    out.bands.push(AxisBand {
                        x: self.pane_left + left,
                        y: self.pane_h,
                        width: (right - left).max(1.0 / self.dpr),
                        height: self.time_axis_height().min(15.0),
                        color: band_color,
                    });
                }
            }
        }

        if !drawing.show_labels {
            return;
        }
        let label_background = drawing
            .label_color
            .as_deref()
            .and_then(Color::parse_css)
            .unwrap_or(stroke);
        let label_text = drawing
            .label_text_color
            .as_deref()
            .and_then(Color::parse_css)
            .unwrap_or_else(|| self.primary_text_color());
        let font_size = self.options.get().layout.font_size;
        for left_side in [true, false] {
            let visible = if left_side {
                self.options.get().left_price_scale.visible
                    && matches!(
                        drawing.price_scale,
                        crate::DrawingPriceScale::Left | crate::DrawingPriceScale::Overlay
                    )
            } else {
                self.options.get().right_price_scale.visible
                    && matches!(
                        drawing.price_scale,
                        crate::DrawingPriceScale::Right | crate::DrawingPriceScale::Overlay
                    )
            };
            if !visible {
                continue;
            }
            for (point, (_, y)) in points.iter().zip([first, second]) {
                if y < pane.top || y > pane.top + pane.height {
                    continue;
                }
                let text = format!("{:.2}", point.price);
                let width = 1.0 + 5.0 + 5.0 + 5.0 + measure(&text);
                let height = font_size + 5.0;
                let (x, align, background_x) = if left_side {
                    (
                        self.pane_left - 10.0,
                        AxisTextAlign::Right,
                        self.pane_left - width,
                    )
                } else {
                    (
                        self.pane_left + self.pane_w + 10.0,
                        AxisTextAlign::Left,
                        self.pane_left + self.pane_w,
                    )
                };
                out.labels.push(AxisLabel {
                    text,
                    x,
                    y,
                    color: label_text,
                    align,
                    midpoint: AxisTextMidpoint::Label,
                    font_scale: 1.0,
                    bold: false,
                    background: Some((
                        background_x,
                        y - height / 2.0,
                        width,
                        height,
                        label_background,
                    )),
                    background_corners: AxisLabelCorners::for_align(align),
                    measure_extra: 0.0,
                    attach_group: None,
                    border: None,
                });
            }
        }
        if self.time_axis_visible {
            const BORDER: f64 = 1.0;
            const TICK: f64 = 5.0;
            const PADDING: f64 = 3.0;
            for (point, (x, _)) in points.iter().zip([first, second]) {
                if x < 0.0 || x > self.pane_w {
                    continue;
                }
                let logical = point.logical.round() as i64;
                let Some(&time) = usize::try_from(logical)
                    .ok()
                    .and_then(|index| self.data.merged_times().get(index))
                else {
                    continue;
                };
                let text = format_date_pattern(time, "M/d/yyyy", &self.month_names);
                let width = measure(&text) + 18.0;
                let height = BORDER + TICK + PADDING + font_size + PADDING;
                let chart_x = self.pane_left + x;
                let box_x = (chart_x - width / 2.0).clamp(
                    self.pane_left,
                    (self.pane_left + self.pane_w - width).max(self.pane_left),
                );
                out.labels.push(AxisLabel {
                    text,
                    x: box_x + width / 2.0,
                    y: self.pane_h + BORDER + TICK + PADDING + font_size / 2.0,
                    color: label_text,
                    align: AxisTextAlign::Center,
                    midpoint: AxisTextMidpoint::None,
                    font_scale: 1.0,
                    bold: false,
                    background: Some((box_x, self.pane_h, width, height, label_background)),
                    background_corners: AxisLabelCorners::BOTTOM,
                    measure_extra: 0.0,
                    attach_group: None,
                    border: None,
                });
            }
        }
    }

    fn append_native_vertical_line_labels<F>(&self, labels: &mut Vec<AxisLabel>, measure: &F)
    where
        F: Fn(&str) -> f64,
    {
        if !self.time_axis_visible {
            return;
        }
        let font_size = self.options.get().layout.font_size;
        const BORDER: f64 = 1.0;
        const TICK: f64 = 5.0;
        const PADDING: f64 = 3.0;
        for series in &self.series {
            if !series.visible || series.removed {
                continue;
            }
            for primitive in &series.native_primitives {
                let crate::native_primitives::NativeSeriesPrimitiveKind::VerticalLine {
                    time,
                    options,
                } = &primitive.kind
                else {
                    continue;
                };
                if !options.show_label {
                    continue;
                }
                let Some(x) = self.time_to_coordinate(*time as f64) else {
                    continue;
                };
                if x < 0.0 || x > self.pane_w {
                    continue;
                }
                let width = measure(&options.label_text) + 9.0 * 2.0;
                let height = BORDER + TICK + PADDING + font_size + PADDING;
                let x = self.pane_left + x;
                let box_x = (x - width / 2.0).clamp(
                    self.pane_left,
                    (self.pane_left + self.pane_w - width).max(self.pane_left),
                );
                labels.push(AxisLabel {
                    text: options.label_text.clone(),
                    x: box_x + width / 2.0,
                    y: self.pane_h + BORDER + TICK + PADDING + font_size / 2.0,
                    color: options.label_text_color,
                    align: AxisTextAlign::Center,
                    midpoint: AxisTextMidpoint::None,
                    font_scale: 1.0,
                    bold: false,
                    background: Some((
                        box_x,
                        self.pane_h,
                        width,
                        height,
                        options.label_background_color,
                    )),
                    background_corners: AxisLabelCorners::BOTTOM,
                    measure_extra: 0.0,
                    attach_group: None,
                    border: None,
                });
            }
        }
    }

    /// reference-compatible right-axis width negotiated from engine-formatted labels and host glyph
    /// measurement. The host contributes font metrics only; label selection and formatting stay
    /// headless. The result is snapped to an even media-pixel width.
    pub fn optimal_price_axis_width<F>(&mut self, measure: F) -> f64
    where
        F: Fn(&str) -> f64,
    {
        self.optimal_price_axis_width_for(PriceScaleTarget::Right, measure)
    }

    /// Measure one visible side independently. Overlay scales deliberately share no axis strip.
    pub fn optimal_price_axis_width_for<F>(&mut self, target: PriceScaleTarget, measure: F) -> f64
    where
        F: Fn(&str) -> f64,
    {
        const AXIS_BORDER_SIZE: f64 = 1.0;
        const AXIS_TICK_LENGTH: f64 = 5.0;
        const PRICE_PADDING_INNER: f64 = 5.0;
        // Exact reference constants (price-axis-widget.ts `optimalWidth`):
        // borderSize + tickLength + paddingInner + paddingOuter + LabelOffset + maxTextWidth.
        const PRICE_PADDING_OUTER: f64 = 5.0;
        const PRICE_LABEL_OFFSET: f64 = 5.0;
        const PRICE_DEFAULT_TEXT_WIDTH: f64 = 34.0;

        let frame = self.build_axis_frame_impl(80.0, &measure, false);
        let wanted_align = match target {
            PriceScaleTarget::Left => AxisTextAlign::Right,
            PriceScaleTarget::Right | PriceScaleTarget::Overlay => AxisTextAlign::Left,
        };
        let mut max_text_width = frame
            .labels
            .iter()
            // Reference optimalWidth measures ticks and ordinary back labels. The smaller
            // Nucleus countdown extension is the exception: it shares the already-negotiated
            // price chip width instead of leaving permanent blank space on the whole scale.
            .filter(|label| label.align == wanted_align && label.font_scale == 1.0)
            .map(|label| measure(&label.text) + label.measure_extra)
            .fold(0.0_f64, f64::max);
        // reference optimalWidth reserves room for the crosshair label via a STATIC worst-case
        // sample (never the live label): the top/bottom prices snapped outward with a
        // 0.11111111111111 fractional tail, formatted by the label source's own formatter.
        if self.crosshair_mode != CrosshairMode::Hidden
            && self.options.get().crosshair.horz_line.label_visible
        {
            for (pi, pane) in self.panes.iter().enumerate() {
                let scale = pane_scale(pane, target);
                if scale.is_empty() {
                    continue;
                }
                let Some((from, _)) = self.visible_range_for_frame() else {
                    continue;
                };
                let series = self
                    .series
                    .iter()
                    .find(|series| series.pane_index == pi && !series.overlay && series.visible);
                let Some(base_value) =
                    series.and_then(|series| self.series_base_value(series.id, from))
                else {
                    continue;
                };
                let top_value = scale.coordinate_to_price(1.0, base_value);
                let bottom_value = scale.coordinate_to_price(pane.height - 2.0, base_value);
                let low = top_value.min(bottom_value).floor() + 0.111_111_111_111_11;
                let high = top_value.max(bottom_value).ceil() - 0.111_111_111_111_11;
                for sample in [low, high] {
                    let text = match series {
                        Some(series) => self.format_series_value(
                            series,
                            scale,
                            scale.price_to_logical_value(sample, base_value),
                        ),
                        None => self.format_scale_value(
                            scale,
                            scale.price_to_logical_value(sample, base_value),
                        ),
                    };
                    max_text_width = max_text_width.max(measure(&text));
                }
            }
        }
        let text_width = if max_text_width > 0.0 {
            max_text_width
        } else {
            PRICE_DEFAULT_TEXT_WIDTH
        };
        // reference `minimumWidth` floors the negotiated strip width (chart-widget.ts
        // `_adjustSizeImpl`: `Math.max(optimalWidth(), minimumWidth)` across the pane's
        // scales on this side).
        let minimum_width = match target {
            PriceScaleTarget::Overlay => 0.0,
            PriceScaleTarget::Right | PriceScaleTarget::Left => self
                .panes
                .iter()
                .map(|pane| pane_scale(pane, target).options().minimum_width)
                .fold(0.0_f64, f64::max),
        };
        let width = (AXIS_BORDER_SIZE
            + AXIS_TICK_LENGTH
            + PRICE_PADDING_INNER
            + PRICE_PADDING_OUTER
            + PRICE_LABEL_OFFSET
            + text_width)
            .ceil()
            .max(minimum_width);
        width + (width as i64 % 2) as f64
    }

    pub(super) fn append_price_line_labels<F>(&self, labels: &mut Vec<AxisLabel>, measure: &F)
    where
        F: Fn(&str) -> f64,
    {
        let font_size = self.options.get().layout.font_size;
        for (pi, pane) in self.panes.iter().enumerate() {
            for s in &self.series {
                if s.pane_index != pi {
                    continue;
                }
                let target = series_scale_target(s);
                let scale = pane_scale(pane, target);
                let Some(base_value) = self.visible_series_base_value(s.id) else {
                    continue;
                };
                for line in &s.price_lines {
                    // reference `axisLabelVisible`: a hidden label leaves the line itself drawn.
                    if !line.axis_label_visible {
                        continue;
                    }
                    if scale.is_empty() {
                        continue;
                    }
                    let y = scale.price_to_coordinate(line.price, base_value);
                    if y < pane.top || y > pane.top + pane.height {
                        continue;
                    }
                    let text = if line.title.is_empty() {
                        // reference custom-price-line-price-axis-view.ts: the label is the line's
                        // price in the OWNING series' format.
                        self.format_series_value(
                            s,
                            scale,
                            scale.price_to_logical_value(line.price, base_value),
                        )
                    } else {
                        line.title.clone()
                    };
                    let width = 1.0 + 5.0 + 5.0 + 5.0 + measure(&text);
                    let height = font_size + 2.5 * 2.0;
                    let (x, align, background_x) = if target == PriceScaleTarget::Left {
                        (
                            self.pane_left - 10.0,
                            AxisTextAlign::Right,
                            self.pane_left - width,
                        )
                    } else {
                        (
                            self.pane_left + self.pane_w + 10.0,
                            AxisTextAlign::Left,
                            self.pane_left + self.pane_w,
                        )
                    };
                    // The label background follows the line color; chart text follows the semantic
                    // foreground token unless the line explicitly supplies a text override.
                    let background = line
                        .axis_label_color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or(line.color);
                    let text_color = line
                        .axis_label_text_color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or_else(|| self.primary_text_color());
                    labels.push(AxisLabel {
                        text,
                        x,
                        y,
                        color: text_color,
                        align,
                        midpoint: AxisTextMidpoint::Label,
                        font_scale: 1.0,
                        bold: false,
                        background: Some((
                            background_x,
                            y - height / 2.0,
                            width,
                            height,
                            background,
                        )),
                        background_corners: AxisLabelCorners::for_align(align),
                        measure_extra: 0.0,
                        attach_group: None,
                        border: None,
                    });
                }
            }
        }
    }

    fn append_trading_axis_labels<F>(&self, labels: &mut Vec<AxisLabel>, measure: &F)
    where
        F: Fn(&str) -> f64,
    {
        let font_size = self.options.get().layout.font_size;
        let fallback = nucleuscharts_core::style::DEFAULT_SURFACE_RGB;
        let chip_fill = Color::parse_css(&self.options.get().layout.background.color)
            .unwrap_or(Color::rgb(fallback.0, fallback.1, fallback.2))
            .solid();
        let mut append = |pane_index: usize,
                          target: crate::TradingPriceScale,
                          price: f64,
                          color: Color,
                          solid: bool| {
            let Some(pane) = self.panes.get(pane_index) else {
                return;
            };
            let Some(y) = self.trading_price_coordinate(pane_index, target, price) else {
                return;
            };
            if y < pane.top || y > pane.top + pane.height {
                return;
            }
            let target = PriceScaleTarget::from(target);
            if target == PriceScaleTarget::Overlay {
                return;
            }
            let text = self.format_trading_price(price);
            let width = 1.0 + 5.0 + 5.0 + 5.0 + measure(&text);
            let height = font_size + 5.0;
            let (x, align, background_x) = if target == PriceScaleTarget::Left {
                (
                    self.pane_left - 10.0,
                    AxisTextAlign::Right,
                    self.pane_left - width,
                )
            } else {
                (
                    self.pane_left + self.pane_w + 10.0,
                    AxisTextAlign::Left,
                    self.pane_left + self.pane_w,
                )
            };
            labels.push(AxisLabel {
                text,
                x,
                y,
                color: if solid { color.contrast_text() } else { color },
                align,
                midpoint: AxisTextMidpoint::Label,
                font_scale: 1.0,
                bold: true,
                background: Some((
                    background_x,
                    y - height / 2.0,
                    width,
                    height,
                    if solid { color.solid() } else { chip_fill },
                )),
                background_corners: AxisLabelCorners::for_align(align),
                measure_extra: 0.0,
                attach_group: None,
                border: (!solid).then_some((1.0, color)),
            });
        };
        for position in &self.trading_state.positions {
            let pending = self
                .trading_state
                .pending_position_action
                .as_ref()
                .is_some_and(|action| action.position_id == position.id);
            append(
                position.pane_index,
                position.price_scale,
                position.average_price,
                if pending {
                    self.trading_state.style.pending
                } else {
                    self.trading_state.style.position
                },
                true,
            );
        }
        for order in &self.trading_state.orders {
            let preview = self.trading_state.preview.as_ref().filter(|preview| {
                matches!(
                    &preview.source,
                    crate::TradingPreviewSource::Order { order_id } if order_id == &order.id
                )
            });
            let color = if preview
                .is_some_and(|preview| preview.phase == crate::TradingPreviewPhase::Pending)
            {
                self.trading_state.style.pending
            } else {
                super::trading_geometry::trading_order_color(
                    &self.trading_state.style,
                    order.role,
                    order.side,
                    order.status,
                )
            };
            append(
                order.pane_index,
                order.price_scale,
                self.trading_effective_order_price(order),
                color,
                order.role == crate::OrderRole::Working,
            );
            if order.kind == crate::OrderKind::StopLimit {
                if let Some(stop_price) = order.stop_price {
                    append(
                        order.pane_index,
                        order.price_scale,
                        stop_price,
                        color,
                        false,
                    );
                }
            }
        }
        if let Some(preview) =
            self.trading_state.preview.as_ref().filter(|preview| {
                !matches!(preview.source, crate::TradingPreviewSource::Order { .. })
            })
        {
            append(
                preview.pane_index,
                preview.price_scale,
                preview.price,
                if preview.phase == crate::TradingPreviewPhase::Pending {
                    self.trading_state.style.pending
                } else {
                    super::trading_geometry::trading_order_color(
                        &self.trading_state.style,
                        preview.role,
                        preview.side,
                        crate::OrderStatus::Working,
                    )
                },
                preview.role == crate::OrderRole::Working,
            );
        }
    }

    /// TradingView's horizontal-line/ray axis label: the drawing's price boxed on the price
    /// axis in the LINE's own color (the label is part of the drawing — recoloring the line
    /// recolors the label on the next frame), with semantic foreground text. Formatted
    /// with the pane's primary series' price format, like the price-line labels.
    pub(super) fn append_drawing_line_labels<F>(&self, labels: &mut Vec<AxisLabel>, measure: &F)
    where
        F: Fn(&str) -> f64,
    {
        let font_size = self.options.get().layout.font_size;
        for (pi, pane) in self.panes.iter().enumerate() {
            let Some(scale) = self.drawing_scale(pi) else {
                continue;
            };
            if scale.is_empty() {
                continue;
            }
            let base = self.drawing_scale_base(pi);
            let series = self
                .series
                .iter()
                .find(|s| s.pane_index == pi && !s.overlay && s.visible && !s.left_scale);
            for drawing in &self.drawings {
                if drawing.pane_index != pi
                    || !matches!(
                        drawing.kind,
                        DrawingKind::HorizontalLine | DrawingKind::HorizontalRay
                    )
                {
                    continue;
                }
                let Some(point) = drawing.points.first() else {
                    continue;
                };
                let y = scale.price_to_coordinate(point.price, base);
                if y < pane.top || y > pane.top + pane.height {
                    continue;
                }
                let logical = scale.price_to_logical_value(point.price, base);
                let text = match series {
                    Some(s) => self.format_series_value(s, scale, logical),
                    None => self.format_scale_value(scale, logical),
                };
                // The label IS the line: background in the drawing's color (parsed per frame,
                // so option changes track), semantic foreground text.
                let background = Color::parse_css(&drawing.color).unwrap_or(PRIMARY).solid();
                let width = 1.0 + 5.0 + 5.0 + 5.0 + measure(&text);
                let height = font_size + 2.5 * 2.0;
                labels.push(AxisLabel {
                    text,
                    x: self.pane_left + self.pane_w + 10.0,
                    y,
                    color: self.primary_text_color(),
                    align: AxisTextAlign::Left,
                    midpoint: AxisTextMidpoint::Label,
                    font_scale: 1.0,
                    bold: false,
                    background: Some((
                        self.pane_left + self.pane_w,
                        y - height / 2.0,
                        width,
                        height,
                        background,
                    )),
                    background_corners: AxisLabelCorners::for_align(AxisTextAlign::Left),
                    measure_extra: 0.0,
                    attach_group: None,
                    border: None,
                });
            }
        }
    }

    /// reference SeriesPriceAxisView: every visible series with `lastValueVisible` (default true)
    /// gets a last-value label on its price scale — the background is the series' bar color,
    /// semantic foreground text, and the last visible bar's close in the scale's format.
    /// Labels sharing an axis side are pushed apart with the reference's overlap resolution
    /// (price-axis-widget.ts `_fixLabelOverlap`).
    ///
    /// TradingView-style cluster extension: the label is one connected box of up to three
    /// independently-toggleable parts — a title chip (the series' `title` in a darker shade of
    /// the label color, left of the price text), the price text itself, and a candle-close
    /// countdown row stacked below, spanning the cluster's full width. The cluster renders while
    /// ANY part is enabled (e.g. `lastValueVisible: false` still leaves title chip + countdown).
    /// The overlap pass runs on the cluster's total height, and the axis-facing corners of the
    /// cluster's outer edges are rounded (internal boundaries stay sharp).
    pub(super) fn append_last_value_label<F>(&self, labels: &mut Vec<AxisLabel>, measure: &F)
    where
        F: Fn(&str) -> f64,
    {
        let Some((from, to)) = self.visible_range_for_frame() else {
            return;
        };
        let row_height = self.options.get().layout.font_size + 2.5 * 2.0;
        // TradingView-style countdown row: 11px secondary text and tighter vertical padding than
        // the 12px price row, keeping the cluster compact without competing with the price.
        let countdown_row_height =
            self.options.get().layout.font_size * COUNTDOWN_FONT_SCALE + 1.5 * 2.0;
        let mut right: Vec<LastValueLabel> = Vec::new();
        let mut left: Vec<LastValueLabel> = Vec::new();
        for (pi, pane) in self.panes.iter().enumerate() {
            for series in &self.series {
                if !series.visible || series.pane_index != pi {
                    continue;
                }
                let show_price = series.last_value_visible;
                let title = if series.title_visible && !series.title.is_empty() {
                    Some(series.title.clone())
                } else {
                    None
                };
                let countdown = if series.countdown_visible {
                    self.series_countdown_text(series.id)
                } else {
                    None
                };
                if !show_price && title.is_none() && countdown.is_none() {
                    continue;
                }
                let target = series_scale_target(series);
                let scale = pane_scale(pane, target);
                let plot = self.data.plot(series.id);
                if plot.is_empty() || scale.is_empty() {
                    continue;
                }
                // The cluster anchors at the last VISIBLE bar's value in the series' bar color,
                // exactly like the plain label (reference series.ts lastValueData(false));
                // a custom series reads its host-recorded frame values instead (Phase C-c).
                let (y, color, text) = if series.kind == SeriesKind::Custom {
                    let Some(last) = series.custom_frame.last_visible else {
                        continue;
                    };
                    let Some(base_value) = self.series_base_value(series.id, from) else {
                        continue;
                    };
                    let y = scale.price_to_coordinate(last.value, base_value);
                    if y < 0.0 || y > self.pane_h {
                        continue;
                    }
                    let text = self.format_series_value(
                        series,
                        scale,
                        scale.price_to_logical_value(last.value, base_value),
                    );
                    (y, last.color, text)
                } else {
                    // whitespace rows are skipped (the reference's plot list omits them).
                    let Some(row) = plot.last_non_whitespace_row(to) else {
                        continue;
                    };
                    let close = plot.value_at(row, PlotValueIndex::Close);
                    if !close.is_finite() {
                        continue;
                    }
                    let Some(base_value) = self.series_base_value(series.id, from) else {
                        continue;
                    };
                    let y = scale.price_to_coordinate(close, base_value);
                    if y < 0.0 || y > self.pane_h {
                        continue;
                    }
                    let baseline = if series.kind == SeriesKind::Baseline {
                        self.resolved_baseline_price(series.id, from, to)
                    } else {
                        None
                    };
                    let color = self.series_bar_color(series, row, baseline);
                    // The series' OWN priceFormat drives its last-value label (reference
                    // series-price-axis-view.ts text, via the scale's series formatter).
                    let text = self.format_series_value(
                        series,
                        scale,
                        scale.price_to_logical_value(close, base_value),
                    );
                    (y, color, text)
                };
                // reference appends overlay (no-scale) series' labels to the pane's default axis
                // (price-axis-widget.ts:601-607); the engine's default axis is the right one.
                // The label's `alignLabels` comes from the axis it lands on.
                let (group, align) = if target == PriceScaleTarget::Left {
                    (&mut left, pane.left_scale.options().align_labels)
                } else {
                    (&mut right, pane.price_scale.options().align_labels)
                };
                // The top row exists only for the price TEXT: when the price chip is off, the
                // title chip attaches to the countdown row instead of leaving a phantom blank
                // row in the strip (and the cluster centers on the value, no shift).
                let top_height = if show_price { row_height } else { 0.0 };
                let countdown_height = if countdown.is_some() {
                    countdown_row_height
                } else {
                    0.0
                };
                // A title-only cluster (price off, countdown off) still gets one row so the
                // outside chip has somewhere to live, centered on the value.
                let height = if top_height + countdown_height > 0.0 {
                    top_height + countdown_height
                } else {
                    row_height
                };
                group.push(LastValueLabel {
                    price_text: show_price.then_some(text),
                    title,
                    countdown,
                    group_id: series.id,
                    // The price row stays centered on the value coordinate; the countdown row
                    // hangs below, so a FULL cluster's center shifts down by half the countdown
                    // row. Without a price row the cluster centers on the value directly.
                    y: y + if top_height > 0.0 {
                        countdown_height / 2.0
                    } else {
                        0.0
                    },
                    height,
                    top_height,
                    // Chip backgrounds follow the series color but always paint solid: a
                    // translucent bar/line color must not bleed through the chips.
                    color: color.solid(),
                    align,
                });
                // TradingView-style bid/ask chips (`bid_ask_visible`, default off): one
                // title+price cluster per side with a live quote, centered on the quote's
                // coordinate. Their attach groups are offset far from any series id so the
                // chips never chain into the main cluster (or each other) when adjacent.
                if series.bid_ask_visible {
                    let sides = [
                        ("Bid", series.bid, series.bid_color.as_str(), PRIMARY, 1u32),
                        ("Ask", series.ask, series.ask_color.as_str(), DOWN, 2u32),
                    ];
                    for (side, value, css, fallback, side_offset) in sides {
                        let Some(quote) = value else {
                            continue;
                        };
                        let Some(base_value) = self.series_base_value(series.id, from) else {
                            continue;
                        };
                        let quote_y = scale.price_to_coordinate(quote, base_value);
                        if quote_y < 0.0 || quote_y > self.pane_h {
                            continue;
                        }
                        let quote_text = self.format_series_value(
                            series,
                            scale,
                            scale.price_to_logical_value(quote, base_value),
                        );
                        let side_color = Color::parse_css(css).unwrap_or(fallback).solid();
                        group.push(LastValueLabel {
                            price_text: Some(quote_text),
                            title: Some(side.to_string()),
                            countdown: None,
                            group_id: (1u32 << 30) + series.id * 4 + side_offset,
                            y: quote_y,
                            height: row_height,
                            top_height: row_height,
                            color: side_color,
                            align,
                        });
                    }
                }
            }
        }
        // reference aligns labels per price-axis widget; the engine has one strip per side. Scales
        // with `alignLabels` off keep raw coordinates; the rest resolve overlap as before.
        resolve_last_value_label_overlap(&mut right, self.pane_h);
        resolve_last_value_label_overlap(&mut left, self.pane_h);
        for (group, target) in [
            (right, PriceScaleTarget::Right),
            (left, PriceScaleTarget::Left),
        ] {
            // Reference geometry (price-axis-view-renderer.ts `_calculateGeometry`): every boxed
            // label is its own content-sized box — borderSize + paddingInner + paddingOuter +
            // tickLength on the text — no cross-label width sharing.
            for label in group {
                // Plain single-box label (no chip, no countdown): the reference-shaped emission,
                // byte-identical to pre-cluster behavior.
                if label.title.is_none() && label.countdown.is_none() {
                    let Some(text) = label.price_text else {
                        continue;
                    };
                    let width = 1.0 + 5.0 + 5.0 + 5.0 + measure(&text);
                    let (x, align, background_x) = if target == PriceScaleTarget::Left {
                        (
                            self.pane_left - 10.0,
                            AxisTextAlign::Right,
                            self.pane_left - width,
                        )
                    } else {
                        (
                            self.pane_left + self.pane_w + 10.0,
                            AxisTextAlign::Left,
                            self.pane_left + self.pane_w,
                        )
                    };
                    labels.push(AxisLabel {
                        text,
                        x,
                        y: label.y,
                        color: LIVE_LABEL_TEXT,
                        align,
                        midpoint: AxisTextMidpoint::Label,
                        font_scale: 1.0,
                        bold: false,
                        background: Some((
                            background_x,
                            label.y - label.height / 2.0,
                            width,
                            label.height,
                            label.color,
                        )),
                        background_corners: AxisLabelCorners::for_align(align),
                        measure_extra: 0.0,
                        attach_group: None,
                        border: None,
                    });
                    continue;
                }
                self.append_last_value_cluster(labels, &label, target, measure);
            }
        }
    }

    /// Emit one TradingView-style last-value cluster (see `append_last_value_label`): one
    /// connected box whose top row holds the title chip (darker shade, left) + price area and
    /// whose optional countdown row spans the full width below. Box width covers the widest row;
    /// each row's text is centered in its area. Axis-facing outer corners are rounded (right
    /// corners on the right strip, left corners on the left strip); internal boundaries and the
    /// chart-facing side stay sharp. On the left strip the chip is the leftmost (axis-facing)
    /// top-row box, so it carries that side's rounded corners instead of the price area.
    fn append_last_value_cluster<F>(
        &self,
        labels: &mut Vec<AxisLabel>,
        label: &LastValueLabel,
        target: PriceScaleTarget,
        measure: &F,
    ) where
        F: Fn(&str) -> f64,
    {
        let right_strip = target != PriceScaleTarget::Left;
        let text_color = LIVE_LABEL_TEXT;
        // The title chip shares the main label color by default (matching the price and
        // countdown chips).
        let chip_color = label.color;
        let title_w = label.title.as_deref().map(measure);
        let chip_w = title_w.map(|w| w + 10.0).unwrap_or(0.0);
        let price_w = label.price_text.as_deref().map(measure).unwrap_or(0.0);
        let countdown_w = label
            .countdown
            .as_deref()
            .map(|text| measure(text) * COUNTDOWN_FONT_SCALE)
            .unwrap_or(0.0);
        // TradingView geometry: the title chip sits outside the strip and the price/countdown
        // box sits inside it. Their logical bounds meet at the border; the primitive encoder
        // excludes the border's exact device pixels from both axis-side boxes.
        // Inside rows share one width, stack flush, and start text at the tick-label inset from
        // the border; the box retains the reference's full outer padding.
        const TEXT_INSET: f64 = 5.0 + 5.0;
        const RIGHT_PAD: f64 = 5.0;
        let inner_text_w = price_w.max(countdown_w);
        // Reference box: borderSize + paddingInner + paddingOuter + tickLength + text.
        let inner_w = 1.0 + TEXT_INSET + inner_text_w + RIGHT_PAD;
        let border_x = if right_strip {
            self.pane_left + self.pane_w
        } else {
            self.pane_left
        };
        let inner_x = if right_strip {
            border_x
        } else {
            border_x - inner_w
        };
        let text_x = if right_strip {
            border_x + TEXT_INSET
        } else {
            border_x - TEXT_INSET
        };
        let text_align = if right_strip {
            AxisTextAlign::Left
        } else {
            AxisTextAlign::Right
        };
        let top_y = label.y - label.height / 2.0;
        let has_countdown = label.countdown.is_some();
        let axis_corners_top = if right_strip {
            AxisLabelCorners {
                top_right: true,
                bottom_right: !has_countdown,
                ..AxisLabelCorners::NONE
            }
        } else {
            AxisLabelCorners {
                top_left: true,
                bottom_left: !has_countdown,
                ..AxisLabelCorners::NONE
            }
        };
        let axis_corners_bottom = if right_strip {
            AxisLabelCorners::RIGHT
        } else {
            AxisLabelCorners::LEFT
        };
        // Title chip: outside the strip, a small standalone rounded box next to the border.
        if let (Some(title), Some(_)) = (&label.title, title_w) {
            let chip_x = if right_strip {
                border_x - chip_w
            } else {
                border_x
            };
            // Without a price row the chip attaches to the cluster's single inside row (the
            // countdown row, or the title-only row) — never a phantom blank top row.
            let chip_row_h = if label.top_height > 0.0 {
                label.top_height
            } else {
                label.height - label.top_height
            };
            labels.push(AxisLabel {
                text: title.clone(),
                x: chip_x + chip_w / 2.0,
                y: top_y + chip_row_h / 2.0,
                color: text_color,
                align: AxisTextAlign::Center,
                midpoint: AxisTextMidpoint::Label,
                font_scale: 1.0,
                bold: false,
                background: Some((chip_x, top_y, chip_w, chip_row_h, chip_color)),
                // The outside chip rounds only its OUTER (chart-facing) side — sharp on the
                // axis-facing side, matching the side rule for axis labels.
                background_corners: if right_strip {
                    AxisLabelCorners::LEFT
                } else {
                    AxisLabelCorners::RIGHT
                },
                // It lives on the pane, not in the strip: it never widens the axis.
                measure_extra: 0.0,
                attach_group: None,
                border: None,
            });
        }
        // The inside price chip renders only when the price text is present (never an empty box).
        if label.top_height > 0.0 && label.price_text.is_some() {
            labels.push(AxisLabel {
                text: label.price_text.clone().unwrap_or_default(),
                x: text_x,
                y: top_y + label.top_height / 2.0,
                color: text_color,
                align: text_align,
                midpoint: AxisTextMidpoint::Label,
                font_scale: 1.0,
                bold: false,
                background: Some((inner_x, top_y, inner_w, label.top_height, label.color)),
                background_corners: axis_corners_top,
                // text + the standard 21px label padding already covers the chip box.
                measure_extra: 0.0,
                // Price chip and countdown chip paint with a shared edge (attached) — the
                // group id is the series id, so the attach never chains into another series'
                // cluster on the same strip.
                attach_group: Some(label.group_id),
                border: None,
            });
        }
        if let Some(countdown) = &label.countdown {
            let countdown_y = top_y + label.top_height;
            let countdown_height = label.height - label.top_height;
            // A countdown-only cluster's top edge is the cluster's top edge, so the countdown
            // box carries the top axis-facing corner as well.
            let standalone = label.top_height == 0.0;
            let corners = if standalone {
                axis_corners_bottom
            } else if right_strip {
                AxisLabelCorners {
                    top_right: false,
                    ..AxisLabelCorners::RIGHT
                }
            } else {
                AxisLabelCorners {
                    top_left: false,
                    ..AxisLabelCorners::LEFT
                }
            };
            labels.push(AxisLabel {
                text: countdown.clone(),
                x: text_x,
                y: countdown_y + countdown_height / 2.0,
                color: LIVE_LABEL_COUNTDOWN_TEXT,
                align: text_align,
                midpoint: AxisTextMidpoint::Label,
                font_scale: COUNTDOWN_FONT_SCALE,
                bold: false,
                background: Some((inner_x, countdown_y, inner_w, countdown_height, label.color)),
                background_corners: corners,
                measure_extra: 0.0,
                // Attached to the price chip above (shared edge, no rounding gap) — the
                // group id is the series id, so the attach never chains into another series'
                // cluster on the same strip.
                attach_group: Some(label.group_id),
                border: None,
            });
        }
    }

    /// The series' candle-close countdown text (TradingView-style `countdown_visible`): the time
    /// until the inferred next bar close — `last_time + interval` minus the host clock — clamped
    /// at zero and formatted by magnitude. The interval is the median of the last up-to-10
    /// inter-bar deltas of the series' own bar times (fallback: the last delta). `None` (the row
    /// hides) with fewer than two bars or no installed host clock (`now_override`).
    pub(crate) fn series_countdown_text(&self, id: SeriesId) -> Option<String> {
        let now = self.now_override?;
        let plot = self.data.plot(id);
        // Only the tail (up to 11 bars → 10 deltas) feeds the inference.
        let times = self.data.merged_times();
        let tail_times: Vec<i64> = (plot.size().saturating_sub(11)..plot.size())
            .filter_map(|row| plot.index_at(row))
            .filter_map(|index| times.get(index as usize).copied())
            .collect();
        let interval = median_bar_interval(&tail_times)?;
        let last_time = *tail_times.last()?;
        let remaining = (last_time as f64 + interval - now).max(0.0);
        Some(format_countdown_remaining(remaining))
    }

    pub(super) fn append_crosshair_labels<F>(&self, labels: &mut Vec<AxisLabel>, measure: &F)
    where
        F: Fn(&str) -> f64,
    {
        let Some((x_css, y_css)) = self.clamped_crosshair() else {
            return;
        };
        let Some((from, to)) = self.visible_range_for_frame() else {
            return;
        };
        if self.crosshair_mode == CrosshairMode::Hidden {
            return;
        }
        // Price-axis label tracks the horizontal line (reference `horzLine`); time-axis label tracks the
        // vertical line (reference `vertLine`). Each carries its own `labelVisible`/`labelBackgroundColor`,
        // and the text color is the semantic foreground token.
        let options = self.options.get();
        let font_size = options.layout.font_size;
        let ch = &options.crosshair;
        if ch.horz_line.label_visible {
            if let Some(pi) = self
                .panes
                .iter()
                .position(|p| y_css >= p.top && y_css <= p.top + p.height)
            {
                let series = self
                    .series
                    .iter()
                    .find(|series| series.pane_index == pi && !series.overlay && series.visible);
                let target = series
                    .map(series_scale_target)
                    .unwrap_or(PriceScaleTarget::Right);
                let scale = pane_scale(&self.panes[pi], target);
                if !scale.is_empty() {
                    let base_value = series
                        .and_then(|series| self.series_base_value(series.id, from))
                        .unwrap_or(0.0);
                    let (price, snap_y) = self.crosshair_snap(pi, x_css, y_css, from, to);
                    // The label source (the pane's first visible, non-overlay series) formats
                    // the crosshair price label with its own priceFormat.
                    let text = match series {
                        Some(series) => self.format_series_value(
                            series,
                            scale,
                            scale.price_to_logical_value(price, base_value),
                        ),
                        None => self.format_scale_value(
                            scale,
                            scale.price_to_logical_value(price, base_value),
                        ),
                    };
                    let width = 1.0 + 5.0 + 5.0 + 5.0 + measure(&text);
                    let height = font_size + 2.5 * 2.0;
                    let (label_x, align, background_x) = if target == PriceScaleTarget::Left {
                        (
                            self.pane_left - 10.0,
                            AxisTextAlign::Right,
                            self.pane_left - width,
                        )
                    } else {
                        (
                            self.pane_left + self.pane_w + 10.0,
                            AxisTextAlign::Left,
                            self.pane_left + self.pane_w,
                        )
                    };
                    let label_bg =
                        css_color(&ch.horz_line.label_background_color, CROSSHAIR_LABEL_BG);
                    labels.push(AxisLabel {
                        text,
                        x: label_x,
                        y: snap_y,
                        color: LIVE_LABEL_TEXT,
                        align,
                        midpoint: AxisTextMidpoint::Label,
                        font_scale: 1.0,
                        bold: false,
                        background: Some((
                            background_x,
                            snap_y - height / 2.0,
                            width,
                            height,
                            label_bg,
                        )),
                        background_corners: AxisLabelCorners::for_align(align),
                        measure_extra: 0.0,
                        attach_group: None,
                        border: None,
                    });
                }
            }
        }
        if ch.vert_line.label_visible && x_css <= self.pane_w && self.time_axis_visible {
            let index = self.snapped_crosshair_index(x_css);
            // reference `indexToTime` returns null in the empty area — the time label is hidden
            // when the snapped index has no bar (past either data edge).
            if index >= 0 && (index as usize) < self.data.merged_times().len() {
                let text = self.format_crosshair_ts(self.data.merged_times()[index as usize]);
                const BORDER: f64 = 1.0;
                const TICK: f64 = 5.0;
                const PADDING: f64 = 3.0;
                let width = measure(&text) + 9.0 * 2.0;
                let height = BORDER + TICK + PADDING + font_size + PADDING;
                let x = self.pane_left + self.time_scale.index_to_coordinate(index);
                let box_x = (x - width / 2.0).clamp(
                    self.pane_left,
                    (self.pane_left + self.pane_w - width).max(self.pane_left),
                );
                let label_bg = css_color(&ch.vert_line.label_background_color, CROSSHAIR_LABEL_BG);
                labels.push(AxisLabel {
                    text,
                    x: box_x + width / 2.0,
                    y: self.pane_h + BORDER + TICK + PADDING + font_size / 2.0,
                    color: LIVE_LABEL_TEXT,
                    align: AxisTextAlign::Center,
                    midpoint: AxisTextMidpoint::StableTime,
                    font_scale: 1.0,
                    bold: false,
                    background: Some((box_x, self.pane_h, width, height, label_bg)),
                    background_corners: AxisLabelCorners::BOTTOM,
                    measure_extra: 0.0,
                    attach_group: None,
                    border: None,
                });
            }
        }
    }

    fn append_native_user_price_alert_crosshair_labels<F>(
        &self,
        labels: &mut Vec<AxisLabel>,
        measure: &F,
    ) where
        F: Fn(&str) -> f64,
    {
        let Some((_, y)) = self.clamped_crosshair() else {
            return;
        };
        let Some(pane_index) = self.pane_at_y(y) else {
            return;
        };
        for series in self
            .series
            .iter()
            .filter(|series| series.visible && series.pane_index == pane_index)
        {
            let Some(state) = series.native_primitives.iter().find_map(|primitive| {
                let crate::native_primitives::NativeSeriesPrimitiveKind::UserPriceAlerts(state) =
                    &primitive.kind
                else {
                    return None;
                };
                Some(state)
            }) else {
                continue;
            };
            let Some(price) = self.series_coordinate_to_price(series.id, y) else {
                continue;
            };
            let Some(text) = self.series_format_price(series.id, price) else {
                continue;
            };
            let width = measure(&text) + 20.0;
            let height = 21.0;
            let left = series_scale_target(series) == PriceScaleTarget::Left;
            let (x, align, background_x) = if left {
                (
                    self.pane_left - 10.0,
                    AxisTextAlign::Right,
                    self.pane_left - width,
                )
            } else {
                (
                    self.pane_left + self.pane_w + 10.0,
                    AxisTextAlign::Left,
                    self.pane_left + self.pane_w,
                )
            };
            labels.push(AxisLabel {
                text,
                x,
                y,
                color: Color::rgb(255, 255, 255),
                align,
                midpoint: AxisTextMidpoint::Label,
                font_scale: 1.0,
                bold: false,
                background: Some((
                    background_x,
                    y - height * 0.5,
                    width,
                    height,
                    state.options.color,
                )),
                background_corners: AxisLabelCorners::for_align(align),
                measure_extra: 0.0,
                attach_group: None,
                border: None,
            });
        }
    }
}
