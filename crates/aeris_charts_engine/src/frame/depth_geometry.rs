use super::*;

/// A printed level size needs this many font sizes of row height.
const PROFILE_TEXT_HEIGHT: f64 = 1.05;
/// Gap (CSS px) between a printed level size and the pane's right edge.
const PROFILE_TEXT_PAD: f64 = 3.0;

fn profile_color(rgba: Option<[u8; 4]>, theme: Color, opacity: f32) -> Color {
    let [r, g, b, a] = rgba.unwrap_or([theme.r(), theme.g(), theme.b(), theme.a()]);
    Color::rgba(r, g, b, (f32::from(a) * opacity).round() as u8)
}

impl ChartEngine {
    pub(crate) fn build_depth_heatmap_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        output: &mut Vec<Prim>,
    ) {
        let Some(pane) = self.panes.get(pane_index) else {
            return;
        };
        let scale = pane_scale(pane, PriceScaleTarget::Right);
        for heatmap in self
            .depth_heatmaps
            .values()
            .filter(|heatmap| heatmap.options.pane_index == pane_index)
        {
            let y_min = scale.price_to_coordinate(heatmap.options.price_min, 0.0) * vpr;
            let y_max = scale.price_to_coordinate(heatmap.options.price_max, 0.0) * vpr;
            let top = y_min.min(y_max) as f32;
            let height = (y_max - y_min).abs() as f32;
            if !top.is_finite() || !height.is_finite() || height <= 0.0 {
                continue;
            }
            let interval = self
                .depth_streams
                .get(&heatmap.stream_id)
                .map(|book| book.options().history_bucket_micros)
                .unwrap_or(1);
            for chunk in &heatmap.chunks {
                let Some(x0) = self.depth_time_coordinate(chunk.start_micros) else {
                    continue;
                };
                let end_micros = chunk.start_micros.saturating_add(
                    interval.saturating_mul(crate::depth::DEPTH_HEATMAP_CHUNK_COLUMNS as i64),
                );
                let Some(x1) = self.depth_time_coordinate(end_micros) else {
                    continue;
                };
                let left = (x0.min(x1) * hpr) as f32;
                let width = ((x1 - x0).abs() * hpr).max(1.0) as f32;
                output.push(Prim::Image {
                    image: chunk.image.clone(),
                    rect: [left, top, width, height],
                    opacity: heatmap.options.opacity,
                });
            }
            if let Some(column) = &heatmap.active {
                let Some(x0) = self.depth_time_coordinate(column.start_micros) else {
                    continue;
                };
                let Some(x1) = self.depth_time_coordinate(column.start_micros + interval) else {
                    continue;
                };
                let left = (x0.min(x1) * hpr) as f32;
                let width = ((x1 - x0).abs() * hpr).max(1.0) as f32;
                output.push(Prim::Image {
                    image: column.image.clone(),
                    rect: [left, top, width, height],
                    opacity: heatmap.options.opacity,
                });
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
            let from_micros = (from_seconds * 1_000_000.0).round() as i64;
            let mut to_micros = (to_seconds * 1_000_000.0).round() as i64;
            to_micros = to_micros.saturating_add(999_999);
            if let Some(clock) = book.replay_clock_micros() {
                to_micros = to_micros.min(clock);
            }
            for event in
                book.microstructure_events_lod(from_micros, to_micros, layer.options.max_markers)
            {
                let Some(x) = self.depth_time_coordinate(event.timestamp_micros) else {
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
                        Some(crate::DepthSide::Bid) => DOWN,
                        Some(crate::DepthSide::Ask) => UP,
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

    /// Current-book bars anchored at the pane's right edge, the largest drawn level spanning
    /// `max_width`. Each level spans its tick on the right price scale, the same mapping the
    /// heatmap uses.
    pub(crate) fn build_depth_profile_frame(
        &self,
        pane_index: usize,
        pane_w_px: i32,
        hpr: f64,
        vpr: f64,
        output: &mut Vec<Prim>,
    ) {
        let Some(pane) = self.panes.get(pane_index) else {
            return;
        };
        let scale = pane_scale(pane, PriceScaleTarget::Right);
        let layout = &self.options.get().layout;
        let (bullish, bearish) = self.themed_candle_colors();
        let text_color = Color::parse_css(&layout.text_color).unwrap_or(Color::rgb(215, 219, 228));
        let text_size = layout.font_size * vpr;
        let text_pad = (PROFILE_TEXT_PAD * hpr).round();
        for profile in self
            .depth_profiles
            .values()
            .filter(|profile| profile.options.pane_index == pane_index)
        {
            let Some(book) = self.depth_streams.get(&profile.stream_id) else {
                continue;
            };
            let options = profile.options;
            let rows = book.ladder(options.levels_per_side, 0.0, None);
            let largest = rows
                .iter()
                .flat_map(|row| [row.bid_size, row.ask_size])
                .flatten()
                .fold(0.0, f64::max);
            if largest <= 0.0 {
                continue;
            }
            let half_tick = book.options().tick_size / 2.0;
            let bid = profile_color(options.bid_rgba, bullish, options.opacity);
            let ask = profile_color(options.ask_rgba, bearish, options.opacity);
            for row in &rows {
                let upper = scale.price_to_coordinate(row.price + half_tick, 0.0) * vpr;
                let lower = scale.price_to_coordinate(row.price - half_tick, 0.0) * vpr;
                if !upper.is_finite() || !lower.is_finite() {
                    continue;
                }
                let top = upper.min(lower).round();
                let span = upper.max(lower).round() - top;
                // Adjacent levels keep a one-pixel gap once rows are tall enough to show it.
                let height = if span >= 3.0 {
                    span - 1.0
                } else {
                    span.max(1.0)
                };
                for (size, color) in [(row.bid_size, bid), (row.ask_size, ask)] {
                    let Some(size) = size else {
                        continue;
                    };
                    let width = (size / largest * options.max_width * hpr).round().max(1.0);
                    output.push(Prim::Rect {
                        rect: IRect {
                            x: pane_w_px - width as i32,
                            y: top as i32,
                            w: width as i32,
                            h: height as i32,
                        },
                        color,
                    });
                    if options.show_sizes && height >= text_size * PROFILE_TEXT_HEIGHT {
                        output.push(Prim::Text {
                            x: (f64::from(pane_w_px) - text_pad) as f32,
                            y: (top + height / 2.0) as f32,
                            text: super::footprint_geometry::compact_volume(size),
                            color: text_color,
                            size: text_size as f32,
                            family: layout.font_family.clone(),
                            align: TextAlign::Right,
                            weight: 400,
                            italic: false,
                        });
                    }
                }
            }
        }
    }

    pub(crate) fn depth_time_coordinate(&self, timestamp_micros: i64) -> Option<f64> {
        let times = self.data_layer().merged_times();
        if times.is_empty() {
            return None;
        }
        let seconds = timestamp_micros as f64 / 1_000_000.0;
        let position = times.partition_point(|&time| time as f64 <= seconds);
        if position == 0 {
            return Some(self.time_scale.index_to_coordinate(0));
        }
        if position >= times.len() {
            return Some(
                self.time_scale
                    .index_to_coordinate(times.len().saturating_sub(1) as i64),
            );
        }
        let left = times[position - 1] as f64;
        let right = times[position] as f64;
        let fraction = if right > left {
            ((seconds - left) / (right - left)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let x0 = self.time_scale.index_to_coordinate((position - 1) as i64);
        let x1 = self.time_scale.index_to_coordinate(position as i64);
        Some(x0 + fraction * (x1 - x0))
    }
}
