use super::*;

/// A printed level size needs this many font sizes of row height.
const PROFILE_TEXT_HEIGHT: f64 = 1.05;
/// Gap (CSS px) between a printed level size and the pane's right edge.
const PROFILE_TEXT_PAD: f64 = 3.0;
/// Extra row height (CSS px) beyond the printed size, so a merged row keeps its gap.
const PROFILE_ROW_PAD: f64 = 2.0;

/// Whole ticks per drawn row: one while a tick is tall enough to read, otherwise the smallest
/// 1-2-5 multiple that makes a row `min_row` tall, so zoomed-out books stay legible and rows
/// start on round prices.
fn profile_ticks_per_row(tick_height: f64, min_row: f64) -> i64 {
    if !tick_height.is_finite() || tick_height <= 0.0 || tick_height >= min_row {
        return 1;
    }
    let needed = min_row / tick_height;
    let mut magnitude: i64 = 1;
    loop {
        for step in [1, 2, 5] {
            let ticks = step * magnitude;
            if ticks as f64 >= needed {
                return ticks;
            }
        }
        match magnitude.checked_mul(10) {
            Some(next) => magnitude = next,
            None => return magnitude,
        }
    }
}

/// One drawn row: whole ticks `[first_tick, first_tick + ticks)` and each side's summed size.
#[derive(Clone, Copy)]
struct ProfileRow {
    first_tick: i64,
    bid: Option<f64>,
    ask: Option<f64>,
}

fn merged_profile_rows(
    levels: &[crate::depth::DepthLadderRow],
    tick_size: f64,
    ticks: i64,
) -> Vec<ProfileRow> {
    let mut rows: Vec<ProfileRow> = Vec::with_capacity(levels.len());
    let add = |total: Option<f64>, size: Option<f64>| match (total, size) {
        (Some(total), Some(size)) => Some(total + size),
        (total, size) => total.or(size),
    };
    // Ladder levels arrive best-first by descending price, so equal rows are adjacent.
    for level in levels {
        let tick = (level.price / tick_size).round() as i64;
        let first_tick = tick.div_euclid(ticks) * ticks;
        match rows.last_mut() {
            Some(row) if row.first_tick == first_tick => {
                row.bid = add(row.bid, level.bid_size);
                row.ask = add(row.ask, level.ask_size);
            }
            _ => rows.push(ProfileRow {
                first_tick,
                bid: level.bid_size,
                ask: level.ask_size,
            }),
        }
    }
    rows
}

fn profile_color(rgba: Option<[u8; 4]>, theme: Color, opacity: f32) -> Color {
    let [r, g, b, a] = rgba.unwrap_or([theme.r(), theme.g(), theme.b(), theme.a()]);
    Color::rgba(r, g, b, (f32::from(a) * opacity).round() as u8)
}

/// One painted depth-profile bar in device px.
#[derive(Clone, Copy)]
pub(crate) struct DepthProfileBar {
    pub rect: IRect,
    pub size: f64,
    pub ask: bool,
}

/// The bar time axis as consecutive time slots, the horizontal frame of every depth layer.
/// Bar `i` owns `[open_i, open_i + duration_i)` and spans its whole bar width, half a bar
/// spacing either side of its center, so a time maps proportionally into its bar. A bar lasts
/// until the next bar opens unless that gap exceeds 1.5 typical bar intervals (a session
/// break, weekend, or missing data); it then lasts one typical interval and the rest of the
/// gap has no horizontal position. Times before the first bar or after the last bar's slot
/// have none either.
#[derive(Clone, Copy)]
pub(crate) struct DepthTimeSlots<'a> {
    times: &'a [i64],
    nominal_micros: i64,
}

impl DepthTimeSlots<'_> {
    fn open_micros(&self, index: usize) -> i64 {
        self.times[index].saturating_mul(1_000_000)
    }

    fn duration_micros(&self, index: usize) -> i64 {
        let Some(&next) = self.times.get(index + 1) else {
            return self.nominal_micros;
        };
        let gap = next
            .saturating_sub(self.times[index])
            .saturating_mul(1_000_000);
        if gap > 0 && gap.saturating_mul(2) <= self.nominal_micros.saturating_mul(3) {
            gap
        } else {
            self.nominal_micros
        }
    }

    /// The last bar opening at or before `micros`.
    fn bar_at_or_before(&self, micros: i64) -> Option<usize> {
        self.times
            .partition_point(|&time| time.saturating_mul(1_000_000) <= micros)
            .checked_sub(1)
    }
}

/// One linear piece of a depth time span on the bar axis: `[start, end)` microseconds drawn
/// from `left` to `right` CSS px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DepthTimePiece {
    pub start_micros: i64,
    pub end_micros: i64,
    pub left: f64,
    pub right: f64,
}

impl ChartEngine {
    /// `fallback_micros` is the typical bar interval while fewer than two bars exist.
    pub(crate) fn depth_time_slots(&self, fallback_micros: i64) -> Option<DepthTimeSlots<'_>> {
        let times = self.data_layer().merged_times();
        if times.is_empty() {
            return None;
        }
        let nominal_micros = super::axis::median_bar_interval(times)
            .map(|seconds| (seconds * 1_000_000.0).round() as i64)
            .filter(|&micros| micros > 0)
            .unwrap_or(fallback_micros.max(1));
        Some(DepthTimeSlots {
            times,
            nominal_micros,
        })
    }

    fn depth_slot_x(&self, slots: &DepthTimeSlots<'_>, index: usize, micros: i64) -> f64 {
        let spacing = self.time_scale.bar_spacing();
        let left = self.time_scale.index_to_coordinate(index as i64) - spacing / 2.0;
        let elapsed = micros.saturating_sub(slots.open_micros(index)) as f64;
        left + elapsed / slots.duration_micros(index) as f64 * spacing
    }

    /// The x (CSS px) of a depth timestamp inside its bar slot.
    pub(crate) fn depth_slot_coordinate(
        &self,
        slots: &DepthTimeSlots<'_>,
        micros: i64,
    ) -> Option<f64> {
        let index = slots.bar_at_or_before(micros)?;
        let end = slots
            .open_micros(index)
            .saturating_add(slots.duration_micros(index));
        (micros < end).then(|| self.depth_slot_x(slots, index, micros))
    }

    #[cfg(test)]
    pub(crate) fn depth_time_coordinate(&self, timestamp_micros: i64) -> Option<f64> {
        self.depth_slot_coordinate(&self.depth_time_slots(1_000_000)?, timestamp_micros)
    }

    /// Visit the maximal linear pieces of `[start, end)` inside visible bar slots, left to
    /// right. Consecutive bars of equal duration share a piece; a session gap or a change of
    /// bar duration starts the next. Work is bounded by the visible bars the span covers.
    pub(crate) fn for_each_depth_time_piece(
        &self,
        slots: &DepthTimeSlots<'_>,
        start_micros: i64,
        end_micros: i64,
        mut visit: impl FnMut(DepthTimePiece),
    ) {
        let Some(range) = self.time_scale.visible_logical_range() else {
            return;
        };
        let last = slots.times.len() as i64 - 1;
        let first_visible = (range.left().floor() as i64 - 1).clamp(0, last) as usize;
        let last_visible = (range.right().ceil() as i64 + 1).clamp(0, last) as usize;
        let mut index = slots
            .bar_at_or_before(start_micros)
            .unwrap_or(0)
            .max(first_visible);
        let mut current: Option<(DepthTimePiece, i64)> = None;
        while index <= last_visible {
            let open = slots.open_micros(index);
            if open >= end_micros {
                break;
            }
            let duration = slots.duration_micros(index);
            let from = start_micros.max(open);
            let to = end_micros.min(open.saturating_add(duration));
            if from < to {
                let right = self.depth_slot_x(slots, index, to);
                match &mut current {
                    Some((piece, piece_duration))
                        if piece.end_micros == from && *piece_duration == duration =>
                    {
                        piece.end_micros = to;
                        piece.right = right;
                    }
                    _ => {
                        if let Some((piece, _)) = current.take() {
                            visit(piece);
                        }
                        let piece = DepthTimePiece {
                            start_micros: from,
                            end_micros: to,
                            left: self.depth_slot_x(slots, index, from),
                            right,
                        };
                        current = Some((piece, duration));
                    }
                }
            }
            index += 1;
        }
        if let Some((piece, _)) = current {
            visit(piece);
        }
    }

    /// Each bucket column occupies exactly its own time slot on the bar axis: a bucket as
    /// long as a bar fills that bar, shorter buckets subdivide their bar in time, and longer
    /// ones span several bars. Columns before the first bar, after the last bar's slot, or in
    /// a session gap are clipped, never squeezed. Each cached chunk is drawn as one
    /// nearest-sampled sub-image per linear piece of its occupied columns, so cells stay hard
    /// edged and the image cache is untouched.
    pub(crate) fn build_depth_heatmap_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        output: &mut Vec<Prim>,
    ) {
        if self.depth_heatmaps.is_empty() {
            return;
        }
        let Some(pane) = self.panes.get(pane_index) else {
            return;
        };
        let scale = pane_scale(pane, PriceScaleTarget::Right);
        for id in self.depth_heatmap_ids(pane_index) {
            let heatmap = &self.depth_heatmaps[&id];
            let Some(book) = self.depth_streams.get(&heatmap.stream_id) else {
                continue;
            };
            let interval = book.options().history_bucket_micros.max(1);
            let Some((low, high)) = heatmap.price_span(book.options().tick_size) else {
                continue;
            };
            let y_low = scale.price_to_coordinate(low, 0.0) * vpr;
            let y_high = scale.price_to_coordinate(high, 0.0) * vpr;
            let top = y_low.min(y_high) as f32;
            let height = (y_low - y_high).abs() as f32;
            if !top.is_finite() || !height.is_finite() || height <= 0.0 {
                continue;
            }
            let Some(slots) = self.depth_time_slots(interval) else {
                continue;
            };
            let mut push = |image: &RasterImage, first_column_micros: i64, start, end| {
                self.for_each_depth_time_piece(&slots, start, end, |piece| {
                    let column = |micros: i64| {
                        (micros.saturating_sub(first_column_micros) as f64 / interval as f64) as f32
                    };
                    let x = column(piece.start_micros);
                    let width = column(piece.end_micros).min(image.width as f32) - x;
                    let left = piece.left * hpr;
                    output.push(Prim::Image {
                        image: image.clone(),
                        source: [x, 0.0, width, image.height as f32],
                        rect: [left as f32, top, (piece.right * hpr - left) as f32, height],
                        opacity: heatmap.options.opacity,
                        sampling: ImageSampling::Nearest,
                    });
                });
            };
            for chunk in &heatmap.chunks {
                if let (Some(&first), Some(&last)) =
                    (chunk.bucket_starts.first(), chunk.bucket_starts.last())
                {
                    push(
                        &chunk.image,
                        chunk.start_micros,
                        first,
                        last.saturating_add(interval),
                    );
                }
            }
            if let Some(column) = &heatmap.active {
                let start = column.start_micros;
                push(&column.image, start, start, start.saturating_add(interval));
            }
        }
    }

    pub(crate) fn build_depth_event_frame(
        &self,
        pane_index: usize,
        from: i64,
        to: i64,
        hpr: f64,
        vpr: f64,
        output: &mut Vec<Prim>,
    ) {
        let Some(pane) = self.panes.get(pane_index) else {
            return;
        };
        let Some(from_seconds) = self.axis_time_seconds_at(from.max(0) as usize) else {
            return;
        };
        let Some(to_seconds) = self.axis_time_seconds_at(to.max(0) as usize) else {
            return;
        };
        let scale = pane_scale(pane, PriceScaleTarget::Right);
        for layer in self
            .depth_event_layers
            .values()
            .filter(|layer| layer.options.pane_index == pane_index)
        {
            let Some(book) = self.depth_streams.get(&layer.stream_id) else {
                continue;
            };
            let Some(slots) = self.depth_time_slots(book.options().history_bucket_micros) else {
                continue;
            };
            let from_micros = (from_seconds * 1_000_000.0).round() as i64;
            let mut to_micros = (to_seconds * 1_000_000.0).round() as i64;
            to_micros = to_micros.saturating_add(999_999);
            if let Some(clock) = book.replay_clock_micros() {
                to_micros = to_micros.min(clock);
            }
            for event in
                book.microstructure_events_lod(from_micros, to_micros, layer.options.max_markers)
            {
                let Some(x) = self.depth_slot_coordinate(&slots, event.timestamp_micros) else {
                    continue;
                };
                let y = scale.price_to_coordinate(event.price, 0.0);
                if !y.is_finite() {
                    continue;
                }
                let fill = match event.kind {
                    crate::DepthEventKind::IcebergRefill => Color::rgb(0x7E, 0x57, 0xC2),
                    crate::DepthEventKind::PulledLiquidity => Color::rgb(0xFF, 0xA7, 0x26),
                    crate::DepthEventKind::SizeCluster => Color::rgb(0x42, 0xA5, 0xF5),
                    crate::DepthEventKind::Sweep => match event.side {
                        Some(crate::DepthSide::Bid) => self.market_colors().down,
                        Some(crate::DepthSide::Ask) => self.market_colors().up,
                        None => PRIMARY,
                    },
                    crate::DepthEventKind::Mixed => PRIMARY,
                };
                output.push(Prim::Circle {
                    cx: (x * hpr) as f32,
                    cy: (y * vpr) as f32,
                    radius: (3.0 + f64::from(event.event_count).ln_1p().min(4.0)) as f32
                        * hpr.min(vpr) as f32,
                    fill,
                    stroke_width: hpr.min(vpr).max(1.0) as f32,
                    stroke: Color::rgba(0xFF, 0xFF, 0xFF, 0xCC),
                });
            }
        }
    }

    /// Current-book bars anchored at the pane's right edge, the largest drawn row spanning
    /// `max_width`. Each row spans its whole ticks on the right price scale, the same mapping
    /// the heatmap uses; ticks too short to read merge into taller rows with summed sizes.
    /// Visit each painted bar of one depth profile in device px, top row first. Frame
    /// construction, hit testing, and selection anchors share it, so hits match pixels.
    pub(crate) fn for_each_depth_profile_bar(
        &self,
        profile: &crate::depth::DepthProfile,
        pane_w_px: i32,
        hpr: f64,
        vpr: f64,
        mut visit: impl FnMut(DepthProfileBar),
    ) {
        let Some(pane) = self.panes.get(profile.options.pane_index) else {
            return;
        };
        let Some(book) = self.depth_streams.get(&profile.stream_id) else {
            return;
        };
        let scale = pane_scale(pane, PriceScaleTarget::Right);
        let font_size = self.options.get().layout.font_size;
        let options = profile.options;
        let levels = book.ladder(options.levels_per_side, 0.0, None);
        let Some(reference) = levels.first().map(|level| level.price) else {
            return;
        };
        let tick_size = book.options().tick_size;
        let tick_height = (scale.price_to_coordinate(reference + tick_size, 0.0)
            - scale.price_to_coordinate(reference, 0.0))
        .abs();
        let ticks = profile_ticks_per_row(
            tick_height,
            font_size * PROFILE_TEXT_HEIGHT + PROFILE_ROW_PAD,
        );
        let rows = merged_profile_rows(&levels, tick_size, ticks);
        let largest = rows
            .iter()
            .flat_map(|row| [row.bid, row.ask])
            .flatten()
            .fold(0.0, f64::max);
        if largest <= 0.0 {
            return;
        }
        let half_tick = tick_size / 2.0;
        for row in &rows {
            let low = row.first_tick as f64 * tick_size - half_tick;
            let high = (row.first_tick + ticks) as f64 * tick_size - half_tick;
            let upper = scale.price_to_coordinate(high, 0.0) * vpr;
            let lower = scale.price_to_coordinate(low, 0.0) * vpr;
            if !upper.is_finite() || !lower.is_finite() {
                continue;
            }
            let top = upper.min(lower).round();
            let span = upper.max(lower).round() - top;
            // A merged row at the touch can hold both sides: asks take its upper half.
            let halves = if row.bid.is_some() && row.ask.is_some() {
                let upper_half = (span / 2.0).round();
                [
                    (row.ask, true, top, upper_half),
                    (row.bid, false, top + upper_half, span - upper_half),
                ]
            } else {
                [(row.ask, true, top, span), (row.bid, false, top, span)]
            };
            for (size, ask, top, span) in halves {
                let Some(size) = size else {
                    continue;
                };
                // Adjacent rows keep a one-pixel gap once rows are tall enough to show it.
                let height = if span >= 3.0 {
                    span - 1.0
                } else {
                    span.max(1.0)
                };
                let width = (size / largest * options.max_width * hpr).round().max(1.0);
                visit(DepthProfileBar {
                    rect: IRect {
                        x: pane_w_px - width as i32,
                        y: top as i32,
                        w: width as i32,
                        h: height as i32,
                    },
                    size,
                    ask,
                });
            }
        }
    }

    /// The painted surface's `--book-bid-fill`, `--book-ask-fill`, and the `--book-fill-text`
    /// printed over them.
    fn order_book_colors(&self) -> (Color, Color, Color) {
        use aeris_charts_core::style::{
            DARK_BOOK_ASK_RGB, DARK_BOOK_BID_RGB, DARK_BOOK_FILL_ALPHA, DARK_BOOK_FILL_TEXT_RGB,
            LIGHT_BOOK_ASK_RGB, LIGHT_BOOK_BID_RGB, LIGHT_BOOK_FILL_ALPHA,
            LIGHT_BOOK_FILL_TEXT_RGB,
        };
        let (bid, ask, alpha, text) = match self.surface_theme() {
            crate::ChartTheme::Light => (
                LIGHT_BOOK_BID_RGB,
                LIGHT_BOOK_ASK_RGB,
                LIGHT_BOOK_FILL_ALPHA,
                LIGHT_BOOK_FILL_TEXT_RGB,
            ),
            crate::ChartTheme::Dark => (
                DARK_BOOK_BID_RGB,
                DARK_BOOK_ASK_RGB,
                DARK_BOOK_FILL_ALPHA,
                DARK_BOOK_FILL_TEXT_RGB,
            ),
        };
        (
            Color::rgba(bid.0, bid.1, bid.2, alpha),
            Color::rgba(ask.0, ask.1, ask.2, alpha),
            Color::rgb(text.0, text.1, text.2),
        )
    }

    pub(crate) fn build_depth_profile_frame(
        &self,
        pane_index: usize,
        pane_w_px: i32,
        hpr: f64,
        vpr: f64,
        output: &mut Vec<Prim>,
    ) {
        if self.depth_profiles.is_empty() {
            return;
        }
        let layout = &self.options.get().layout;
        let (book_bid, book_ask, text_color) = self.order_book_colors();
        let text_size = layout.font_size * vpr;
        let text_pad = (PROFILE_TEXT_PAD * hpr).round();
        for id in self.depth_profile_ids(pane_index) {
            let profile = &self.depth_profiles[&id];
            let options = profile.options;
            let bid = profile_color(options.bid_rgba, book_bid, options.opacity);
            let ask = profile_color(options.ask_rgba, book_ask, options.opacity);
            self.for_each_depth_profile_bar(profile, pane_w_px, hpr, vpr, |bar| {
                output.push(Prim::Rect {
                    rect: bar.rect,
                    color: if bar.ask { ask } else { bid },
                });
                let (top, height) = (f64::from(bar.rect.y), f64::from(bar.rect.h));
                if options.show_sizes && height >= text_size * PROFILE_TEXT_HEIGHT {
                    output.push(Prim::Text {
                        x: (f64::from(pane_w_px) - text_pad) as f32,
                        y: (top + height / 2.0) as f32,
                        text: super::footprint_geometry::compact_volume(bar.size),
                        color: text_color,
                        size: text_size as f32,
                        family: layout.font_family.clone(),
                        align: TextAlign::Right,
                        weight: 400,
                        italic: false,
                    });
                }
            });
        }
    }
}
