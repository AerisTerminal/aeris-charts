//! Axis frame production: price/time labels, widths, marker/price-line/crosshair labels.

use super::*;

/// A last-value label candidate before axis overlap resolution (reference IPriceAxisView state:
/// the original `coordinate` plus the render coordinate the overlap pass adjusts). `align`
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
/// `h:mm:ss` below a day, `"Xd Xh"` at a day and beyond.
pub(crate) fn format_countdown_remaining(remaining: f64) -> String {
    let secs = remaining.max(0.0).floor() as u64;
    if secs < 3600 {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    } else if secs < 86400 {
        format!("{}:{:02}:{:02}", secs / 3600, secs % 3600 / 60, secs % 60)
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
    // Split around the center and sort each side toward it (reference sorts by the original
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

    /// Aion extension: bold round-figure price tick labels (TradingView decile rule). Uniform
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
        self.layout_for_frame();
        self.autoscale_visible();
        let mut out = AxisFrame {
            separator_hover: self.separator_hover,
            ..AxisFrame::default()
        };
        let visible = self.visible_range_for_frame();
        let layout_text_color = Color::parse_css(&self.options.get().layout.text_color)
            .unwrap_or(Color::rgb(0x19, 0x19, 0x19));
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
        let options = self.options.get();
        let right_text_x = self.pane_left + self.pane_w + 5.0 + 5.0;
        let left_text_x = (self.pane_left - 5.0 - 5.0).max(0.0);
        for (pi, pane) in self.panes.iter().enumerate() {
            if options.right_price_scale.visible {
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
                            bold,
                            background: None,
                            background_corners: AxisLabelCorners::NONE,
                            measure_extra: 0.0,
                            attach_group: None,
                        });
                    }
                }
            }
            if options.left_price_scale.visible {
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
                            bold,
                            background: None,
                            background_corners: AxisLabelCorners::NONE,
                            measure_extra: 0.0,
                            attach_group: None,
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
                    y: self.pane_h + 1.0 + 5.0 + 3.0 + options.layout.font_size / 2.0,
                    color: layout_text_color,
                    align: AxisTextAlign::Center,
                    midpoint: AxisTextMidpoint::None,
                    // reference `timeScale.allowBoldLabels` (default true): bold major labels.
                    bold: self.time_scale.options().allow_bold_labels && weight >= maximum_weight,
                    background: None,
                    background_corners: AxisLabelCorners::NONE,
                    measure_extra: 0.0,
                    attach_group: None,
                });
            }
            self.append_marker_labels(&mut out.labels, from, to);
        }
        self.append_price_line_labels(&mut out.labels, &measure);
        self.append_drawing_line_labels(&mut out.labels, &measure);
        self.append_last_value_label(&mut out.labels, &measure);
        self.append_crosshair_labels(&mut out.labels, &measure);
        out.separators = self
            .panes
            .iter()
            .skip(1)
            .map(|p| p.top - PANE_SEPARATOR)
            .collect();
        out
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
        const PRICE_PADDING_OUTER: f64 = 5.0;
        const PRICE_LABEL_OFFSET: f64 = 5.0;
        const PRICE_DEFAULT_TEXT_WIDTH: f64 = 34.0;

        let frame = self.build_axis_frame(80.0, &measure);
        let wanted_align = match target {
            PriceScaleTarget::Left => AxisTextAlign::Right,
            PriceScaleTarget::Right | PriceScaleTarget::Overlay => AxisTextAlign::Left,
        };
        let max_text_width = frame
            .labels
            .iter()
            .filter(|label| label.align == wanted_align)
            // `measure_extra` carries the cluster's title-chip width on its price-area label,
            // so the negotiated strip covers the chip + price row as one unit.
            .map(|label| measure(&label.text) + label.measure_extra)
            .fold(0.0_f64, f64::max);
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

    pub(super) fn append_marker_labels(&self, labels: &mut Vec<AxisLabel>, from: i64, to: i64) {
        let times = self.data.merged_times();
        for (pi, pane) in self.panes.iter().enumerate() {
            for s in &self.series {
                if !s.visible || s.pane_index != pi {
                    continue;
                }
                let scale = pane_scale(pane, series_scale_target(s));
                let Some(base_value) = self.series_base_value(s.id, from) else {
                    continue;
                };
                if scale.is_empty() {
                    continue;
                }
                let plot = self.data.plot(s.id);
                for marker in &s.markers {
                    if marker.text.is_empty() {
                        continue;
                    }
                    let Ok(pos) = times.binary_search(&marker.time) else {
                        continue;
                    };
                    let index = pos as i64;
                    if index < from || index > to {
                        continue;
                    }
                    let Some(row) = plot.search(index, MismatchDirection::None) else {
                        continue;
                    };
                    if plot.is_whitespace_row(row) {
                        continue;
                    }
                    let high = plot.value_at(row, PlotValueIndex::High);
                    let low = plot.value_at(row, PlotValueIndex::Low);
                    let close = plot.value_at(row, PlotValueIndex::Close);
                    let x = self.pane_left + self.time_scale.index_to_coordinate(index);
                    let envelope = marker_envelope_size(self.time_scale.bar_spacing());
                    let half_envelope = envelope / 2.0;
                    let margin = marker_margin(self.time_scale.bar_spacing());
                    let text_height = self.options.get().layout.font_size;
                    let y = match marker.position {
                        crate::marker_pos::BELOW => {
                            scale.price_to_coordinate(low, base_value)
                                + envelope
                                + margin * 2.0
                                + text_height * 0.6
                        }
                        crate::marker_pos::ABOVE => {
                            scale.price_to_coordinate(high, base_value)
                                - envelope
                                - margin
                                - text_height * 0.6
                        }
                        _ => {
                            scale.price_to_coordinate(close, base_value)
                                + half_envelope
                                + margin
                                + text_height * 0.6
                        }
                    };
                    if y >= pane.top && y <= pane.top + pane.height && x >= 0.0 && x <= self.pane_w
                    {
                        labels.push(AxisLabel {
                            text: marker.text.clone(),
                            x,
                            y,
                            color: marker.color,
                            align: AxisTextAlign::Center,
                            midpoint: AxisTextMidpoint::None,
                            bold: false,
                            background: None,
                            background_corners: AxisLabelCorners::NONE,
                            measure_extra: 0.0,
                            attach_group: None,
                        });
                    }
                }
            }
        }
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
                    // reference defaults: the label background follows the line color and the text is
                    // its contrast pick (as the crosshair labels do); both are overridable.
                    let background = line
                        .axis_label_color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or(line.color);
                    let text_color = line
                        .axis_label_text_color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or_else(|| background.contrast_text());
                    labels.push(AxisLabel {
                        text,
                        x,
                        y,
                        color: text_color,
                        align,
                        midpoint: AxisTextMidpoint::Label,
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
                    });
                }
            }
        }
    }

    /// TradingView's horizontal-line/ray axis label: the drawing's price boxed on the price
    /// axis in the LINE's own color (the label is part of the drawing — recoloring the line
    /// recolors the label on the next frame), with the contrast-pick text color. Formatted
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
                // so option changes track), contrast-picked text.
                let background = Color::parse_css(&drawing.color)
                    .unwrap_or(Color::rgb(0x29, 0x62, 0xff))
                    .solid();
                let width = 1.0 + 5.0 + 5.0 + 5.0 + measure(&text);
                let height = font_size + 2.5 * 2.0;
                labels.push(AxisLabel {
                    text,
                    x: self.pane_left + self.pane_w + 10.0,
                    y,
                    color: background.contrast_text(),
                    align: AxisTextAlign::Left,
                    midpoint: AxisTextMidpoint::Label,
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
                });
            }
        }
    }

    /// reference SeriesPriceAxisView: every visible series with `lastValueVisible` (default true)
    /// gets a last-value label on its price scale — the background is the series' bar color,
    /// the text its contrast, the value the last visible bar's close in the scale's format.
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
                let top_height = if show_price || title.is_some() {
                    row_height
                } else {
                    0.0
                };
                let countdown_height = if countdown.is_some() { row_height } else { 0.0 };
                group.push(LastValueLabel {
                    price_text: show_price.then_some(text),
                    title,
                    countdown,
                    // The price row stays centered on the value coordinate; the countdown row
                    // hangs below, so the cluster center shifts down by half the countdown row.
                    y: y + countdown_height / 2.0,
                    height: top_height + countdown_height,
                    top_height,
                    // Chip backgrounds follow the series color but always paint solid: a
                    // translucent bar/line color must not bleed through the chips.
                    color: color.solid(),
                    align,
                });
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
            // One chip width per scale: every price chip on the strip — a plain price-only
            // box (indicator outputs) or a cluster's inner price/countdown box (the main
            // series) — spans the widest label's box, so all chips line up exactly.
            let shared_box_w = group
                .iter()
                .map(|label| {
                    let text_w = label
                        .price_text
                        .as_deref()
                        .map(&measure)
                        .unwrap_or(0.0)
                        .max(label.countdown.as_deref().map(&measure).unwrap_or(0.0));
                    if label.title.is_none() && label.countdown.is_none() {
                        // Plain single-box need: 1px edge + 5+5+5 padding.
                        1.0 + 5.0 + 5.0 + 5.0 + text_w
                    } else {
                        // Cluster inner need: 1px edge + 5 + text + 5.
                        1.0 + 5.0 + text_w + 5.0
                    }
                })
                .fold(0.0_f64, f64::max);
            for label in group {
                // Plain single-box label (no chip, no countdown): the reference-shaped emission,
                // byte-identical to pre-cluster behavior.
                if label.title.is_none() && label.countdown.is_none() {
                    let Some(text) = label.price_text else {
                        continue;
                    };
                    let width = shared_box_w.max(1.0 + 5.0 + 5.0 + 5.0 + measure(&text));
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
                        color: label.color.contrast_text(),
                        align,
                        midpoint: AxisTextMidpoint::Label,
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
                    });
                    continue;
                }
                self.append_last_value_cluster(labels, &label, target, measure, shared_box_w);
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
        shared_inner_w: f64,
    ) where
        F: Fn(&str) -> f64,
    {
        let right_strip = target != PriceScaleTarget::Left;
        let text_color = label.color.contrast_text();
        // The countdown row's text is slightly MUTED against the chip (TradingView-style)
        // instead of the full-contrast color the title and price texts use.
        let muted_text_color = {
            let base = text_color;
            Color::rgba(
                base.r(),
                base.g(),
                base.b(),
                (base.a() as f64 * 0.65).round() as u8,
            )
        };
        // The title chip shares the main label color by default (matching the price and
        // countdown chips).
        let chip_color = label.color;
        let title_w = label.title.as_deref().map(measure);
        let chip_w = title_w.map(|w| w + 10.0).unwrap_or(0.0);
        let price_w = label.price_text.as_deref().map(measure).unwrap_or(0.0);
        let countdown_w = label.countdown.as_deref().map(measure).unwrap_or(0.0);
        // TradingView geometry: the title chip sits OUTSIDE the axis strip (on the pane, a small
        // gap before the border), while the price chip and the countdown chip live inside the
        // strip, share ONE width (the wider of the two texts), and stack flush with left-aligned
        // text so they end at the exact same place — "held together".
        const GAP: f64 = 1.0;
        const PAD: f64 = 5.0;
        let inner_text_w = price_w.max(countdown_w);
        // The scale-shared chip width (the widest box on the strip) so every cluster's inner
        // chips match the other series' price chips exactly; never narrower than its own text.
        let inner_w = shared_inner_w.max(1.0 + PAD + inner_text_w + PAD);
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
            inner_x + 1.0 + PAD
        } else {
            inner_x + inner_w - 1.0 - PAD
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
                border_x - GAP - chip_w
            } else {
                border_x + GAP
            };
            let chip_row_h = if label.top_height > 0.0 {
                label.top_height
            } else {
                label.height
            };
            labels.push(AxisLabel {
                text: title.clone(),
                x: chip_x + chip_w / 2.0,
                y: top_y + chip_row_h / 2.0,
                color: text_color,
                align: AxisTextAlign::Center,
                midpoint: AxisTextMidpoint::Label,
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
                bold: false,
                background: Some((inner_x, top_y, inner_w, label.top_height, label.color)),
                background_corners: axis_corners_top,
                // text + the standard 21px label padding already covers the chip box.
                measure_extra: 0.0,
                // Price chip and countdown chip paint with a shared edge (attached).
                attach_group: Some(1),
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
                color: muted_text_color,
                align: text_align,
                midpoint: AxisTextMidpoint::Label,
                bold: false,
                background: Some((inner_x, countdown_y, inner_w, countdown_height, label.color)),
                background_corners: corners,
                measure_extra: 0.0,
                // Attached to the price chip above (shared edge, no rounding gap).
                attach_group: Some(1),
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
        let indices = plot.indices();
        // Only the tail (up to 11 bars → 10 deltas) feeds the inference.
        let tail = &indices[indices.len().saturating_sub(11)..];
        let times = self.data.merged_times();
        let tail_times: Vec<i64> = tail
            .iter()
            .filter_map(|i| times.get(*i as usize).copied())
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
        // and the text color is the reference contrast pick against that background.
        let options = self.options.get();
        let font_size = options.layout.font_size;
        let ch = options.crosshair;
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
                        color: label_bg.contrast_text(),
                        align,
                        midpoint: AxisTextMidpoint::Label,
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
                let width = measure(&text) + 9.0 * 2.0;
                let height = font_size + 3.0 + 3.0;
                let x = self.pane_left + self.time_scale.index_to_coordinate(index);
                let box_x = (x - width / 2.0).clamp(
                    self.pane_left,
                    (self.pane_left + self.pane_w - width).max(self.pane_left),
                );
                let label_bg = css_color(&ch.vert_line.label_background_color, CROSSHAIR_LABEL_BG);
                labels.push(AxisLabel {
                    text,
                    x: box_x + width / 2.0,
                    y: self.pane_h + 1.0 + height / 2.0,
                    color: label_bg.contrast_text(),
                    align: AxisTextAlign::Center,
                    midpoint: AxisTextMidpoint::StableTime,
                    bold: false,
                    background: Some((box_x, self.pane_h + 1.0, width, height, label_bg)),
                    background_corners: AxisLabelCorners::BOTTOM,
                    measure_extra: 0.0,
                    attach_group: None,
                });
            }
        }
    }
}
