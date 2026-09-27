use super::*;

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
