//! Typed browser boundary for the engine-owned order book.

use super::*;
use aeris_charts_engine::{
    DepthEventKind, DepthEventLayerOptions, DepthHeatmapOptions, DepthLevel,
    DepthMicrostructureEvent, DepthOptions, DepthSide, DepthSnapshot, DepthUpdate,
    MAX_DEPTH_BATCH_UPDATES,
};

const NO_ORDER_COUNT: u32 = u32::MAX;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

fn integer_micros(value: f64) -> Result<i64, String> {
    if value.is_finite() && value.abs() <= MAX_SAFE_INTEGER && value.fract() == 0.0 {
        Ok(value as i64)
    } else {
        Err("depth timestamps must be exact integer microseconds".to_string())
    }
}

fn sequence(high: u32, low: u32) -> u64 {
    (u64::from(high) << 32) | u64::from(low)
}

fn levels(
    prices: &Float64Array,
    sizes: &Float64Array,
    counts: &js_sys::Uint32Array,
) -> Result<Vec<DepthLevel>, String> {
    let len = prices.length();
    if sizes.length() != len || (counts.length() != 0 && counts.length() != len) {
        return Err("depth level columns have different lengths".to_string());
    }
    let mut output = Vec::with_capacity(len as usize);
    for index in 0..len {
        let count = (counts.length() != 0)
            .then(|| counts.get_index(index))
            .filter(|&count| count != NO_ORDER_COUNT);
        output.push(DepthLevel {
            price: prices.get_index(index),
            size: sizes.get_index(index),
            order_count: count,
        });
    }
    Ok(output)
}

impl ChartInner {
    pub(super) fn add_depth_stream(&mut self, key: &str, options_json: &str) -> u32 {
        let Ok(options) = serde_json::from_str::<DepthOptions>(options_json) else {
            return u32::MAX;
        };
        self.engine
            .add_depth_stream(key, options)
            .ok()
            .and_then(|id| u32::try_from(id).ok())
            .unwrap_or(u32::MAX)
    }

    pub(super) fn depth_stream_id(&self, key: &str) -> u32 {
        self.engine
            .depth_stream_id(key)
            .and_then(|id| u32::try_from(id).ok())
            .unwrap_or(0)
    }

    pub(super) fn remove_depth_stream(&mut self, stream_id: u32) -> bool {
        self.engine.remove_depth_stream(u64::from(stream_id))
    }

    pub(super) fn add_depth_heatmap(&mut self, stream_id: u32, options_json: &str) -> u32 {
        let Ok(options) = serde_json::from_str::<DepthHeatmapOptions>(options_json) else {
            return u32::MAX;
        };
        self.engine
            .add_depth_heatmap(u64::from(stream_id), options)
            .ok()
            .and_then(|id| u32::try_from(id).ok())
            .unwrap_or(u32::MAX)
    }

    pub(super) fn remove_depth_heatmap(&mut self, id: u32) -> bool {
        self.engine.remove_depth_heatmap(u64::from(id))
    }

    pub(super) fn add_depth_event_layer(&mut self, stream_id: u32, options_json: &str) -> u32 {
        let Ok(options) = serde_json::from_str::<DepthEventLayerOptions>(options_json) else {
            return u32::MAX;
        };
        self.engine
            .add_depth_event_layer(u64::from(stream_id), options)
            .ok()
            .and_then(|id| u32::try_from(id).ok())
            .unwrap_or(u32::MAX)
    }

    pub(super) fn remove_depth_event_layer(&mut self, id: u32) -> bool {
        self.engine.remove_depth_event_layer(u64::from(id))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn set_depth_events_typed(
        &mut self,
        stream_id: u32,
        timestamps_micros: &Float64Array,
        prices: &Float64Array,
        sizes: &Float64Array,
        sides: &js_sys::Int8Array,
        kinds: &js_sys::Uint8Array,
        labels_json: &str,
    ) -> String {
        let result = (|| {
            let len = timestamps_micros.length();
            let labels = if labels_json.is_empty() {
                Vec::new()
            } else {
                serde_json::from_str::<Vec<Option<String>>>(labels_json)
                    .map_err(|_| "depth event labels are invalid".to_string())?
            };
            if prices.length() != len
                || sizes.length() != len
                || sides.length() != len
                || kinds.length() != len
                || (!labels.is_empty() && labels.len() != len as usize)
            {
                return Err("depth event columns have different lengths".to_string());
            }
            let mut events = Vec::with_capacity(len as usize);
            for index in 0..len {
                let side = match sides.get_index(index) {
                    -1 => None,
                    0 => Some(DepthSide::Bid),
                    1 => Some(DepthSide::Ask),
                    _ => return Err(format!("depth event side at row {index} is invalid")),
                };
                let kind = match kinds.get_index(index) {
                    0 => DepthEventKind::IcebergRefill,
                    1 => DepthEventKind::PulledLiquidity,
                    2 => DepthEventKind::SizeCluster,
                    3 => DepthEventKind::Sweep,
                    _ => return Err(format!("depth event kind at row {index} is invalid")),
                };
                events.push(DepthMicrostructureEvent {
                    timestamp_micros: integer_micros(timestamps_micros.get_index(index))?,
                    price: prices.get_index(index),
                    size: sizes.get_index(index),
                    side,
                    kind,
                    host_label: labels.get(index as usize).cloned().flatten(),
                });
            }
            self.engine
                .set_depth_microstructure_events(u64::from(stream_id), events)
                .map_err(|error| error.to_string())
        })();
        result.map_or_else(|error| error, |()| String::new())
    }

    pub(super) fn depth_ladder_json(
        &self,
        stream_id: u32,
        levels_per_side: u32,
        minimum_size: f64,
        max_distance_ticks: u32,
    ) -> String {
        let Some(book) = self.engine.depth_book(u64::from(stream_id)) else {
            return "null".to_string();
        };
        serde_json::to_string(&book.ladder(
            levels_per_side as usize,
            minimum_size,
            (max_distance_ticks != u32::MAX).then_some(max_distance_ticks),
        ))
        .unwrap_or_else(|_| "null".to_string())
    }

    pub(super) fn depth_study_json(
        &self,
        stream_id: u32,
        levels_per_side: u32,
        minimum_size: f64,
        max_distance_ticks: u32,
    ) -> String {
        let Some(book) = self.engine.depth_book(u64::from(stream_id)) else {
            return "null".to_string();
        };
        let snapshot = book.study_snapshot(
            levels_per_side as usize,
            minimum_size,
            (max_distance_ticks != u32::MAX).then_some(max_distance_ticks),
        );
        serde_json::to_string(&serde_json::json!({
            "sequence": snapshot.sequence.map(|sequence| sequence.to_string()),
            "best_bid": snapshot.best_bid,
            "best_ask": snapshot.best_ask,
            "bid_cumulative": snapshot.bid_cumulative,
            "ask_cumulative": snapshot.ask_cumulative,
            "imbalance": snapshot.imbalance,
        }))
        .unwrap_or_else(|_| "null".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn set_depth_snapshot_typed(
        &mut self,
        stream_id: u32,
        timestamp_micros: f64,
        sequence_high: u32,
        sequence_low: u32,
        bid_prices: &Float64Array,
        bid_sizes: &Float64Array,
        bid_order_counts: &js_sys::Uint32Array,
        ask_prices: &Float64Array,
        ask_sizes: &Float64Array,
        ask_order_counts: &js_sys::Uint32Array,
    ) -> String {
        let result = (|| {
            let timestamp_micros = integer_micros(timestamp_micros)?;
            let bids = levels(bid_prices, bid_sizes, bid_order_counts)?;
            let asks = levels(ask_prices, ask_sizes, ask_order_counts)?;
            self.engine
                .set_depth_snapshot(
                    u64::from(stream_id),
                    DepthSnapshot {
                        timestamp_micros,
                        sequence: sequence(sequence_high, sequence_low),
                        bids,
                        asks,
                    },
                )
                .map_err(|error| error.to_string())
        })();
        result.map_or_else(|error| error, |()| String::new())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn update_depth_typed(
        &mut self,
        stream_id: u32,
        timestamps_micros: &Float64Array,
        sequence_high: &js_sys::Uint32Array,
        sequence_low: &js_sys::Uint32Array,
        previous_high: &js_sys::Uint32Array,
        previous_low: &js_sys::Uint32Array,
        sides: &js_sys::Uint8Array,
        prices: &Float64Array,
        sizes: &Float64Array,
        order_counts: &js_sys::Uint32Array,
    ) -> String {
        let result = (|| {
            let len = timestamps_micros.length();
            if len as usize > MAX_DEPTH_BATCH_UPDATES
                || sequence_high.length() != len
                || sequence_low.length() != len
                || previous_high.length() != len
                || previous_low.length() != len
                || sides.length() != len
                || prices.length() != len
                || sizes.length() != len
                || (order_counts.length() != 0 && order_counts.length() != len)
            {
                return Err("depth update columns have different or excessive lengths".to_string());
            }
            let mut updates = Vec::with_capacity(len as usize);
            for index in 0..len {
                let side = match sides.get_index(index) {
                    0 => DepthSide::Bid,
                    1 => DepthSide::Ask,
                    _ => return Err(format!("depth side at row {index} must be 0 or 1")),
                };
                let count = (order_counts.length() != 0)
                    .then(|| order_counts.get_index(index))
                    .filter(|&count| count != NO_ORDER_COUNT);
                updates.push(DepthUpdate {
                    timestamp_micros: integer_micros(timestamps_micros.get_index(index))?,
                    sequence: sequence(
                        sequence_high.get_index(index),
                        sequence_low.get_index(index),
                    ),
                    previous_sequence: sequence(
                        previous_high.get_index(index),
                        previous_low.get_index(index),
                    ),
                    side,
                    level: DepthLevel {
                        price: prices.get_index(index),
                        size: sizes.get_index(index),
                        order_count: count,
                    },
                });
            }
            self.engine
                .update_depth_batch(u64::from(stream_id), &updates)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })();
        result.map_or_else(|error| error, |()| String::new())
    }
}
