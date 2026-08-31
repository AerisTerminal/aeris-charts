//! Browser translation for engine-owned tick-driven footprint series.

use super::*;
use nucleuscharts_engine::{
    AggressorSide, FootprintBarAggregation, FootprintCellMode, FootprintSeriesOptions,
    FootprintTrade, FootprintUpdateKind,
};

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

fn number(value: &serde_json::Value, key: &str) -> Result<Option<f64>, String> {
    value
        .get(key)
        .map(|value| {
            value
                .as_f64()
                .filter(|number| number.is_finite())
                .ok_or_else(|| format!("{key} must be a finite number"))
        })
        .transpose()
}

fn boolean(value: &serde_json::Value, key: &str) -> Result<Option<bool>, String> {
    value
        .get(key)
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| format!("{key} must be a boolean"))
        })
        .transpose()
}

fn color(value: &serde_json::Value, key: &str) -> Result<Option<Color>, String> {
    value
        .get(key)
        .map(|value| {
            value
                .as_str()
                .and_then(Color::parse_css)
                .ok_or_else(|| format!("{key} must be a supported CSS color"))
        })
        .transpose()
}

fn parse_options(json: &str) -> Result<FootprintSeriesOptions, String> {
    let value = serde_json::from_str::<serde_json::Value>(json)
        .map_err(|error| format!("invalid footprint options JSON: {error}"))?;
    if !value.is_object() {
        return Err("footprint options must be an object".to_string());
    }
    let mut options = FootprintSeriesOptions::default();
    if let Some(tick_size) = number(&value, "tick_size")? {
        options.aggregation.tick_size = tick_size;
    }
    let interval_seconds = number(&value, "interval_seconds")?.unwrap_or(60.0);
    let anchor_seconds = number(&value, "anchor_seconds")?.unwrap_or(0.0);
    let interval_micros = interval_seconds * 1_000_000.0;
    let anchor_micros = anchor_seconds * 1_000_000.0;
    if interval_micros < 0.0
        || interval_micros > u64::MAX as f64
        || anchor_micros < i64::MIN as f64
        || anchor_micros > i64::MAX as f64
    {
        return Err("footprint time aggregation is outside the supported range".to_string());
    }
    options.aggregation.bars = FootprintBarAggregation::Time {
        interval_micros: interval_micros.round() as u64,
        anchor_micros: anchor_micros.round() as i64,
    };
    if let Some(ratio) = number(&value, "imbalance_ratio")? {
        options.aggregation.imbalance.ratio = ratio;
    }
    if let Some(minimum) = number(&value, "imbalance_minimum_volume")? {
        options.aggregation.imbalance.minimum_volume = minimum;
    }
    if let Some(levels_value) = value.get("stacked_imbalance_levels") {
        let levels = levels_value
            .as_u64()
            .ok_or_else(|| "stacked_imbalance_levels must be a non-negative integer".to_string())?;
        options.aggregation.imbalance.consecutive_levels = levels.min(u32::MAX as u64) as u32;
    }
    if let Some(mode_value) = value.get("cell_mode") {
        let mode = mode_value
            .as_str()
            .ok_or_else(|| "cell_mode must be a string".to_string())?;
        options.visual.cell_mode = match mode {
            "bid_ask" => FootprintCellMode::BidAsk,
            "total" => FootprintCellMode::Total,
            "delta" => FootprintCellMode::Delta,
            _ => return Err(format!("unsupported footprint cell_mode '{mode}'")),
        };
    }
    if let Some(font_size) = number(&value, "font_size")? {
        options.visual.font_size = font_size;
    }
    macro_rules! set_color {
        ($field:ident, $key:literal) => {
            if let Some(color) = color(&value, $key)? {
                options.visual.$field = color;
            }
        };
    }
    set_color!(bid_color, "bid_color");
    set_color!(ask_color, "ask_color");
    set_color!(positive_delta_color, "positive_delta_color");
    set_color!(negative_delta_color, "negative_delta_color");
    if let Some(text_color) = value.get("text_color") {
        options.visual.text_color = if text_color.is_null() {
            None
        } else {
            Some(
                text_color
                    .as_str()
                    .and_then(Color::parse_css)
                    .ok_or_else(|| {
                        "text_color must be null or a supported CSS color".to_string()
                    })?,
            )
        };
    }
    set_color!(poc_color, "poc_color");
    set_color!(stacked_bid_color, "stacked_bid_color");
    set_color!(stacked_ask_color, "stacked_ask_color");
    if let Some(show) = boolean(&value, "show_bar_summary")? {
        options.visual.show_bar_summary = show;
    }
    Ok(options)
}

pub(super) fn options_json(options: &FootprintSeriesOptions) -> String {
    let FootprintBarAggregation::Time {
        interval_micros,
        anchor_micros,
    } = options.aggregation.bars
    else {
        return "{}".to_string();
    };
    serde_json::json!({
        "tick_size": options.aggregation.tick_size,
        "interval_seconds": interval_micros as f64 / 1_000_000.0,
        "anchor_seconds": anchor_micros as f64 / 1_000_000.0,
        "imbalance_ratio": options.aggregation.imbalance.ratio,
        "imbalance_minimum_volume": options.aggregation.imbalance.minimum_volume,
        "stacked_imbalance_levels": options.aggregation.imbalance.consecutive_levels,
        "cell_mode": match options.visual.cell_mode {
            FootprintCellMode::BidAsk => "bid_ask",
            FootprintCellMode::Total => "total",
            FootprintCellMode::Delta => "delta",
        },
        "font_size": options.visual.font_size,
        "bid_color": options.visual.bid_color.to_css(),
        "ask_color": options.visual.ask_color.to_css(),
        "positive_delta_color": options.visual.positive_delta_color.to_css(),
        "negative_delta_color": options.visual.negative_delta_color.to_css(),
        "text_color": options.visual.text_color.map(|color| color.to_css()),
        "poc_color": options.visual.poc_color.to_css(),
        "stacked_bid_color": options.visual.stacked_bid_color.to_css(),
        "stacked_ask_color": options.visual.stacked_ask_color.to_css(),
        "show_bar_summary": options.visual.show_bar_summary,
    })
    .to_string()
}

fn error_json(error: impl core::fmt::Display) -> String {
    serde_json::json!({ "ok": false, "error": error.to_string() }).to_string()
}

fn optional_number(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

fn optional_safe_u64(value: f64) -> Option<u64> {
    (value.is_finite() && (0.0..=MAX_SAFE_INTEGER).contains(&value) && value.fract() == 0.0)
        .then_some(value as u64)
}

#[allow(clippy::too_many_arguments)]
fn trade(
    timestamp_micros: f64,
    price: f64,
    volume: f64,
    side: u8,
    bid: f64,
    ask: f64,
    sequence: f64,
    trade_id: f64,
    conditions: u32,
    session_id: f64,
) -> Result<FootprintTrade, &'static str> {
    if !timestamp_micros.is_finite()
        || timestamp_micros.abs() > MAX_SAFE_INTEGER
        || timestamp_micros.fract() != 0.0
    {
        return Err("timestamp_micros must be a safe integer");
    }
    if (sequence.is_finite() && optional_safe_u64(sequence).is_none())
        || (trade_id.is_finite() && optional_safe_u64(trade_id).is_none())
        || (session_id.is_finite() && optional_safe_u64(session_id).is_none())
    {
        return Err("sequence, trade_id, and session_id must be safe non-negative integers");
    }
    Ok(FootprintTrade {
        timestamp_micros: timestamp_micros as i64,
        price,
        volume,
        aggressor: match side {
            1 => AggressorSide::Buy,
            2 => AggressorSide::Sell,
            _ => AggressorSide::Unknown,
        },
        bid: optional_number(bid),
        ask: optional_number(ask),
        sequence: optional_safe_u64(sequence),
        trade_id: optional_safe_u64(trade_id),
        conditions,
        session_id: optional_safe_u64(session_id),
    })
}

#[allow(clippy::too_many_arguments)]
fn trades_from_columns(
    timestamps: &[f64],
    prices: &[f64],
    volumes: &[f64],
    sides: &[u8],
    bids: &[f64],
    asks: &[f64],
    sequences: &[f64],
    trade_ids: &[f64],
    conditions: &[u32],
    session_ids: &[f64],
) -> Result<Vec<FootprintTrade>, String> {
    let len = timestamps.len();
    if [
        prices.len(),
        volumes.len(),
        sides.len(),
        bids.len(),
        asks.len(),
        sequences.len(),
        trade_ids.len(),
        conditions.len(),
        session_ids.len(),
    ]
    .into_iter()
    .any(|other| other != len)
    {
        return Err("footprint typed columns have different lengths".to_string());
    }
    let mut trades = Vec::with_capacity(len);
    for index in 0..len {
        let event = trade(
            timestamps[index],
            prices[index],
            volumes[index],
            sides[index],
            bids[index],
            asks[index],
            sequences[index],
            trade_ids[index],
            conditions[index],
            session_ids[index],
        )
        .map_err(|error| format!("invalid footprint trade at index {index}: {error}"))?;
        trades.push(event);
    }
    Ok(trades)
}

impl ChartInner {
    pub(super) fn add_footprint_series(&mut self, adopt_primary: bool, options_json: &str) -> u32 {
        let Ok(options) = parse_options(options_json) else {
            return u32::MAX;
        };
        if adopt_primary {
            self.engine
                .configure_footprint_series(0, options)
                .map(|()| 0)
                .unwrap_or(u32::MAX)
        } else {
            self.engine
                .add_footprint_series(options)
                .unwrap_or(u32::MAX)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn set_footprint_trades_typed(
        &mut self,
        id: u32,
        timestamps: &[f64],
        prices: &[f64],
        volumes: &[f64],
        sides: &[u8],
        bids: &[f64],
        asks: &[f64],
        sequences: &[f64],
        trade_ids: &[f64],
        conditions: &[u32],
        session_ids: &[f64],
    ) -> String {
        let trades = match trades_from_columns(
            timestamps,
            prices,
            volumes,
            sides,
            bids,
            asks,
            sequences,
            trade_ids,
            conditions,
            session_ids,
        ) {
            Ok(trades) => trades,
            Err(error) => return error_json(error),
        };
        self.engine
            .set_footprint_trades(id, trades)
            .map_or_else(error_json, |()| String::new())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn update_footprint_trades_typed(
        &mut self,
        id: u32,
        timestamps: &[f64],
        prices: &[f64],
        volumes: &[f64],
        sides: &[u8],
        bids: &[f64],
        asks: &[f64],
        sequences: &[f64],
        trade_ids: &[f64],
        conditions: &[u32],
        session_ids: &[f64],
    ) -> String {
        let trades = match trades_from_columns(
            timestamps,
            prices,
            volumes,
            sides,
            bids,
            asks,
            sequences,
            trade_ids,
            conditions,
            session_ids,
        ) {
            Ok(trades) => trades,
            Err(error) => return error_json(error),
        };
        self.engine
            .update_footprint_trades(id, trades)
            .map_or_else(error_json, |kind| {
                match kind {
                    FootprintUpdateKind::Tip => "tip",
                    FootprintUpdateKind::Historical => "historical",
                }
                .to_string()
            })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn update_footprint_trade_typed(
        &mut self,
        id: u32,
        timestamp: f64,
        price: f64,
        volume: f64,
        side: u8,
        bid: f64,
        ask: f64,
        sequence: f64,
        trade_id: f64,
        conditions: u32,
        session_id: f64,
    ) -> String {
        let event = match trade(
            timestamp, price, volume, side, bid, ask, sequence, trade_id, conditions, session_id,
        ) {
            Ok(event) => event,
            Err(error) => return error_json(error),
        };
        self.engine
            .update_footprint_trade(id, event)
            .map_or_else(error_json, |kind| {
                match kind {
                    FootprintUpdateKind::Tip => "tip",
                    FootprintUpdateKind::Historical => "historical",
                }
                .to_string()
            })
    }

    pub(super) fn apply_footprint_options(&mut self, id: u32, json: &str) -> String {
        let options = match parse_options(json) {
            Ok(options) => options,
            Err(error) => return error_json(error),
        };
        self.engine
            .apply_footprint_series_options(id, options)
            .map_or_else(error_json, |()| String::new())
    }
}
