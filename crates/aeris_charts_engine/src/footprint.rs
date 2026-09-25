//! Tick-truth aggregation for footprint / numbers-bar series.
//!
//! The retained trade tape is authoritative. Bars, price levels, delta extrema, POC, and
//! imbalances are derived state and can be rebuilt deterministically after a late event. Rendering
//! consumes [`FootprintBar`] values; it never attempts to infer order flow from OHLC rows.

use std::collections::HashMap;

use aeris_charts_core::model::data_layer::{SeriesId, SeriesIdError};
use aeris_charts_core::model::data_validation::{MAX_SAFE_VALUE, MIN_SAFE_VALUE};
use aeris_charts_core::style::MARKET_UP_RGB;
use aeris_charts_render::color::Color;

use crate::{ChartEngine, PriceFormatKind, SeriesKind, SeriesPriceFormat};

const MICROS_PER_SECOND: i64 = 1_000_000;
const MIN_TIMESTAMP_MICROS: i64 = -62_167_219_200 * MICROS_PER_SECOND;
const MAX_TIMESTAMP_MICROS: i64 = 253_402_300_799 * MICROS_PER_SECOND + 999_999;

/// Which side initiated a trade. Unknown trades remain in total volume but never manufacture bid
/// or ask volume.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AggressorSide {
    Buy,
    Sell,
    #[default]
    Unknown,
}

/// One raw trade event. `timestamp_micros` is signed Unix time at microsecond resolution. The
/// optional quote is the market state at the trade and is used only when `aggressor` is unknown.
#[derive(Clone, Debug, PartialEq)]
pub struct FootprintTrade {
    pub timestamp_micros: i64,
    pub price: f64,
    pub volume: f64,
    pub aggressor: AggressorSide,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    /// Feed ordering within an equal timestamp. Missing sequences retain input order.
    pub sequence: Option<u64>,
    /// Stable provider identity used for idempotent correction/replay when available.
    pub trade_id: Option<u64>,
    /// Opaque feed condition bits retained for later microstructure extensions.
    pub conditions: u32,
    /// Host-defined session identity. A change starts a new bar and resets session delta.
    pub session_id: Option<u64>,
}

/// The bar-building policy. Time bars are aligned to `anchor_micros`; trade and volume bars start
/// at their first event. A trade is never split between volume bars.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FootprintBarAggregation {
    Time {
        interval_micros: u64,
        anchor_micros: i64,
    },
    Trades {
        trades_per_bar: u32,
    },
    Volume {
        volume_per_bar: f64,
    },
}

impl Default for FootprintBarAggregation {
    fn default() -> Self {
        Self::Time {
            interval_micros: 60_000_000,
            anchor_micros: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FootprintImbalanceOptions {
    /// Dominant volume divided by the diagonally adjacent opposite-side volume.
    pub ratio: f64,
    /// Minimum dominant-side volume required before a level can be imbalanced.
    pub minimum_volume: f64,
    /// Adjacent imbalanced levels required to mark the complete run as stacked.
    pub consecutive_levels: u32,
}

impl Default for FootprintImbalanceOptions {
    fn default() -> Self {
        Self {
            ratio: 3.0,
            minimum_volume: 10.0,
            consecutive_levels: 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FootprintAggregationOptions {
    pub tick_size: f64,
    pub bars: FootprintBarAggregation,
    pub imbalance: FootprintImbalanceOptions,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FootprintCellMode {
    #[default]
    BidAsk,
    Total,
    Delta,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FootprintVisualOptions {
    pub cell_mode: FootprintCellMode,
    pub font_size: f64,
    pub bid_color: Color,
    pub ask_color: Color,
    pub positive_delta_color: Color,
    pub negative_delta_color: Color,
    /// `None` follows the chart layout foreground and retokenizes on theme changes.
    pub text_color: Option<Color>,
    pub poc_color: Color,
    pub stacked_bid_color: Color,
    pub stacked_ask_color: Color,
    pub show_bar_summary: bool,
}

impl Default for FootprintVisualOptions {
    fn default() -> Self {
        Self {
            cell_mode: FootprintCellMode::BidAsk,
            font_size: 11.0,
            bid_color: Color::rgba(239, 83, 80, 70),
            ask_color: Color::rgba(MARKET_UP_RGB.0, MARKET_UP_RGB.1, MARKET_UP_RGB.2, 70),
            positive_delta_color: Color::rgba(
                MARKET_UP_RGB.0,
                MARKET_UP_RGB.1,
                MARKET_UP_RGB.2,
                110,
            ),
            negative_delta_color: Color::rgba(239, 83, 80, 110),
            text_color: None,
            poc_color: Color::rgb(255, 193, 7),
            stacked_bid_color: Color::rgb(255, 82, 82),
            stacked_ask_color: Color::rgb(MARKET_UP_RGB.0, MARKET_UP_RGB.1, MARKET_UP_RGB.2),
            show_bar_summary: true,
        }
    }
}

impl FootprintVisualOptions {
    pub(crate) fn reset_style_to_defaults(&mut self) {
        let defaults = Self::default();
        self.font_size = defaults.font_size;
        self.bid_color = defaults.bid_color;
        self.ask_color = defaults.ask_color;
        self.positive_delta_color = defaults.positive_delta_color;
        self.negative_delta_color = defaults.negative_delta_color;
        self.text_color = defaults.text_color;
        self.poc_color = defaults.poc_color;
        self.stacked_bid_color = defaults.stacked_bid_color;
        self.stacked_ask_color = defaults.stacked_ask_color;
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct FootprintSeriesOptions {
    pub aggregation: FootprintAggregationOptions,
    pub visual: FootprintVisualOptions,
}

impl Default for FootprintAggregationOptions {
    fn default() -> Self {
        Self {
            tick_size: 0.25,
            bars: FootprintBarAggregation::default(),
            imbalance: FootprintImbalanceOptions::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
pub struct FootprintLevel {
    /// Exact integer grid identity. `price == level * tick_size`.
    pub level: i64,
    pub price: f64,
    pub bid_volume: f64,
    pub ask_volume: f64,
    pub unknown_volume: f64,
    pub total_volume: f64,
    pub delta: f64,
    pub bid_imbalance: bool,
    pub ask_imbalance: bool,
    pub stacked_bid_imbalance: bool,
    pub stacked_ask_imbalance: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct FootprintBar {
    pub start_timestamp_micros: i64,
    pub end_timestamp_micros: i64,
    pub session_id: Option<u64>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub bid_volume: f64,
    pub ask_volume: f64,
    pub unknown_volume: f64,
    pub total_volume: f64,
    pub delta: f64,
    /// Highest running bar delta observed after applying each trade, with zero as the initial
    /// state. This deliberately cannot be reconstructed from final `delta`.
    pub max_delta: f64,
    /// Lowest running bar delta observed after applying each trade, with zero as the initial state.
    pub min_delta: f64,
    /// Session cumulative delta at the end of this bar.
    pub session_delta: f64,
    pub trade_count: u32,
    pub poc_level: i64,
    pub poc_price: f64,
    /// Sorted ascending by integer price level.
    pub levels: Vec<FootprintLevel>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FootprintWorkStats {
    pub incremental_ticks: usize,
    pub historical_rebuilds: usize,
    pub rebuilt_ticks: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FootprintError {
    InvalidTickSize,
    InvalidAggregation,
    InvalidImbalance,
    InvalidVisualOptions,
    InvalidTimestamp { index: usize },
    InvalidPrice { index: usize },
    OffGridPrice { index: usize },
    InvalidVolume { index: usize },
    InvalidQuote { index: usize },
    DuplicateTradeId { trade_id: u64 },
    UnsupportedChartAggregation,
    ProjectionTimeCollision,
    UnknownSeries(SeriesId),
    StaleSeries(SeriesId),
}

impl core::fmt::Display for FootprintError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidTickSize => write!(f, "tick_size must be finite and greater than zero"),
            Self::InvalidAggregation => write!(f, "footprint bar aggregation is invalid"),
            Self::InvalidImbalance => write!(f, "footprint imbalance options are invalid"),
            Self::InvalidVisualOptions => write!(f, "footprint visual options are invalid"),
            Self::InvalidTimestamp { index } => write!(f, "trade {index} has an invalid timestamp"),
            Self::InvalidPrice { index } => write!(f, "trade {index} has an invalid price"),
            Self::OffGridPrice { index } => {
                write!(f, "trade {index} price is not aligned to tick_size")
            }
            Self::InvalidVolume { index } => write!(f, "trade {index} has an invalid volume"),
            Self::InvalidQuote { index } => write!(f, "trade {index} has an invalid bid/ask quote"),
            Self::DuplicateTradeId { trade_id } => {
                write!(f, "trade_id {trade_id} appears more than once")
            }
            Self::UnsupportedChartAggregation => write!(
                f,
                "chart footprint series currently require whole-second aligned time bars"
            ),
            Self::ProjectionTimeCollision => write!(
                f,
                "two footprint bars resolve to the same canonical chart second"
            ),
            Self::UnknownSeries(id) => write!(f, "unknown series id {id}"),
            Self::StaleSeries(id) => write!(f, "stale series id {id}"),
        }
    }
}

impl std::error::Error for FootprintError {}

#[derive(Clone, Debug)]
struct StoredTrade {
    event: FootprintTrade,
    input_order: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct RebuildSeed {
    last_trade_price: Option<f64>,
    last_classified_side: AggressorSide,
    active_session: Option<Option<u64>>,
    session_delta: f64,
}

/// Authoritative tick tape plus its reusable derived footprint bars.
#[derive(Clone, Debug)]
pub struct FootprintAggregator {
    options: FootprintAggregationOptions,
    trades: Vec<StoredTrade>,
    trade_ids: HashMap<u64, usize>,
    bars: Vec<FootprintBar>,
    next_input_order: u64,
    rebuild_seed: RebuildSeed,
    last_trade_price: Option<f64>,
    last_classified_side: AggressorSide,
    active_session: Option<Option<u64>>,
    session_delta: f64,
    work: FootprintWorkStats,
}

#[derive(Clone, Debug)]
pub(crate) struct FootprintSeriesState {
    pub aggregator: FootprintAggregator,
    pub visual: FootprintVisualOptions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FootprintUpdateKind {
    Tip,
    Historical,
}

impl FootprintAggregator {
    pub fn new(options: FootprintAggregationOptions) -> Result<Self, FootprintError> {
        validate_options(options)?;
        Ok(Self {
            options,
            trades: Vec::new(),
            trade_ids: HashMap::new(),
            bars: Vec::new(),
            next_input_order: 0,
            rebuild_seed: RebuildSeed::default(),
            last_trade_price: None,
            last_classified_side: AggressorSide::Unknown,
            active_session: None,
            session_delta: 0.0,
            work: FootprintWorkStats::default(),
        })
    }

    pub fn options(&self) -> FootprintAggregationOptions {
        self.options
    }

    pub fn bars(&self) -> &[FootprintBar] {
        &self.bars
    }

    pub fn trades(&self) -> impl ExactSizeIterator<Item = &FootprintTrade> {
        self.trades.iter().map(|trade| &trade.event)
    }

    pub fn work_stats(&self) -> FootprintWorkStats {
        self.work
    }

    pub fn reset_work_stats(&mut self) {
        self.work = FootprintWorkStats::default();
    }

    pub fn capacity_bytes(&self) -> usize {
        self.trades.capacity() * core::mem::size_of::<StoredTrade>()
            + self.trade_ids.capacity() * core::mem::size_of::<(u64, usize)>()
            + self.bars.capacity() * core::mem::size_of::<FootprintBar>()
            + self
                .bars
                .iter()
                .map(|bar| bar.levels.capacity() * core::mem::size_of::<FootprintLevel>())
                .sum::<usize>()
    }

    pub(crate) fn retain_last_bars(&mut self, keep: usize) {
        if self.bars.len() <= keep {
            return;
        }
        if keep == 0 {
            self.trades.clear();
            self.trade_ids.clear();
            self.bars.clear();
            self.last_trade_price = None;
            self.last_classified_side = AggressorSide::Unknown;
            self.active_session = None;
            self.session_delta = 0.0;
            self.rebuild_seed = RebuildSeed::default();
            return;
        }
        let first = self.bars.len() - keep;
        let cutoff = self.bars[first].start_timestamp_micros;
        let trade_start = self
            .trades
            .partition_point(|trade| trade.event.timestamp_micros < cutoff);
        let mut seed = self.rebuild_seed;
        for stored in &self.trades[..trade_start] {
            let trade = &stored.event;
            if seed.active_session != Some(trade.session_id) {
                seed.active_session = Some(trade.session_id);
                seed.session_delta = 0.0;
            }
            let side = classify_aggressor(trade, seed.last_trade_price, seed.last_classified_side);
            seed.last_trade_price = Some(trade.price);
            if side != AggressorSide::Unknown {
                seed.last_classified_side = side;
            }
            seed.session_delta += match side {
                AggressorSide::Buy => trade.volume,
                AggressorSide::Sell => -trade.volume,
                AggressorSide::Unknown => 0.0,
            };
        }
        self.rebuild_seed = seed;
        self.trades.drain(..trade_start);
        self.bars.drain(..first);
        self.reindex_trade_ids();
    }

    /// Atomically replace the tape, sort it by feed order, and reconstruct every derived bar.
    pub fn set_trades(&mut self, input: Vec<FootprintTrade>) -> Result<(), FootprintError> {
        validate_trade_batch(self.options, &input)?;
        let base = self.next_input_order;
        let mut trades = input
            .into_iter()
            .enumerate()
            .map(|(index, event)| StoredTrade {
                event,
                input_order: base.saturating_add(index as u64),
            })
            .collect::<Vec<_>>();
        trades.sort_by_key(trade_order_key);
        self.next_input_order = base.saturating_add(trades.len() as u64);
        self.trades = trades;
        self.reindex_trade_ids();
        self.rebuild_seed = RebuildSeed::default();
        self.rebuild();
        Ok(())
    }

    /// Append a live event in O(current-bar levels) when it is at the tape tip. A late event or a
    /// provider correction is inserted in canonical order and triggers deterministic historical
    /// reconstruction instead of corrupting Max/Min Delta paths.
    pub fn update_trade(
        &mut self,
        event: FootprintTrade,
    ) -> Result<FootprintUpdateKind, FootprintError> {
        self.update_trades(vec![event])
    }

    /// Atomically apply a provider batch. Monotonic new events retain the incremental path;
    /// corrections or late events merge into the final canonical tape and rebuild exactly once.
    pub fn update_trades(
        &mut self,
        input: Vec<FootprintTrade>,
    ) -> Result<FootprintUpdateKind, FootprintError> {
        validate_trade_batch(self.options, &input)?;
        if input.is_empty() {
            return Ok(FootprintUpdateKind::Tip);
        }
        if self.batch_is_tip(&input) {
            for event in input {
                let stored = StoredTrade {
                    event,
                    input_order: self.next_input_order,
                };
                self.next_input_order = self.next_input_order.saturating_add(1);
                self.apply_trade(&stored.event);
                if let Some(trade_id) = stored.event.trade_id {
                    self.trade_ids.insert(trade_id, self.trades.len());
                }
                self.trades.push(stored);
                self.work.incremental_ticks += 1;
            }
            return Ok(FootprintUpdateKind::Tip);
        }

        let mut next_input_order = self.next_input_order;
        for event in input {
            if let Some(position) = event
                .trade_id
                .and_then(|trade_id| self.trade_ids.get(&trade_id).copied())
            {
                let input_order = self.trades[position].input_order;
                self.trades[position] = StoredTrade { event, input_order };
            } else {
                self.trades.push(StoredTrade {
                    event,
                    input_order: next_input_order,
                });
                next_input_order = next_input_order.saturating_add(1);
            }
        }
        self.trades.sort_by_key(trade_order_key);
        self.next_input_order = next_input_order;
        self.reindex_trade_ids();
        self.rebuild();
        Ok(FootprintUpdateKind::Historical)
    }

    pub(crate) fn batch_is_tip(&self, input: &[FootprintTrade]) -> bool {
        let mut previous = self.trades.last().map(trade_order_key);
        for (index, event) in input.iter().enumerate() {
            if event
                .trade_id
                .is_some_and(|trade_id| self.trade_ids.contains_key(&trade_id))
            {
                return false;
            }
            let key = (
                event.timestamp_micros,
                event.sequence.unwrap_or(u64::MAX),
                self.next_input_order.saturating_add(index as u64),
            );
            if previous.is_some_and(|previous| previous > key) {
                return false;
            }
            previous = Some(key);
        }
        true
    }

    fn historical_update_candidate(&self) -> Self {
        Self {
            options: self.options,
            trades: self.trades.clone(),
            trade_ids: self.trade_ids.clone(),
            bars: Vec::with_capacity(self.bars.len()),
            next_input_order: self.next_input_order,
            rebuild_seed: self.rebuild_seed,
            last_trade_price: self.last_trade_price,
            last_classified_side: self.last_classified_side,
            active_session: self.active_session,
            session_delta: self.session_delta,
            work: self.work,
        }
    }

    pub fn bar(&self, index: usize) -> Option<&FootprintBar> {
        self.bars.get(index)
    }

    fn rebuild(&mut self) {
        self.bars.clear();
        self.last_trade_price = self.rebuild_seed.last_trade_price;
        self.last_classified_side = self.rebuild_seed.last_classified_side;
        self.active_session = self.rebuild_seed.active_session;
        self.session_delta = self.rebuild_seed.session_delta;
        let trade_count = self.trades.len();
        for index in 0..trade_count {
            // The event has no heap-owned fields. Copying one value avoids retaining a second tape
            // while mutable derived state is rebuilt.
            let trade = self.trades[index].event.clone();
            self.apply_trade(&trade);
        }
        self.work.historical_rebuilds += 1;
        self.work.rebuilt_ticks += trade_count;
    }

    fn reindex_trade_ids(&mut self) {
        self.trade_ids.clear();
        self.trade_ids.reserve(
            self.trades
                .iter()
                .filter(|trade| trade.event.trade_id.is_some())
                .count(),
        );
        for (index, trade) in self.trades.iter().enumerate() {
            if let Some(trade_id) = trade.event.trade_id {
                self.trade_ids.insert(trade_id, index);
            }
        }
    }

    fn apply_trade(&mut self, trade: &FootprintTrade) {
        if self.active_session != Some(trade.session_id) {
            self.active_session = Some(trade.session_id);
            self.session_delta = 0.0;
        }
        let side = classify_aggressor(trade, self.last_trade_price, self.last_classified_side);
        self.last_trade_price = Some(trade.price);
        if side != AggressorSide::Unknown {
            self.last_classified_side = side;
        }
        let level = price_level(trade.price, self.options.tick_size);
        let start_new = self
            .bars
            .last()
            .is_none_or(|bar| self.must_start_bar(bar, trade));
        if start_new {
            let start = match self.options.bars {
                FootprintBarAggregation::Time {
                    interval_micros,
                    anchor_micros,
                } => aligned_bucket_start(
                    trade.timestamp_micros,
                    interval_micros as i64,
                    anchor_micros,
                ),
                FootprintBarAggregation::Trades { .. } | FootprintBarAggregation::Volume { .. } => {
                    trade.timestamp_micros
                }
            };
            self.bars.push(FootprintBar {
                start_timestamp_micros: start,
                end_timestamp_micros: trade.timestamp_micros,
                session_id: trade.session_id,
                open: trade.price,
                high: trade.price,
                low: trade.price,
                close: trade.price,
                bid_volume: 0.0,
                ask_volume: 0.0,
                unknown_volume: 0.0,
                total_volume: 0.0,
                delta: 0.0,
                max_delta: 0.0,
                min_delta: 0.0,
                session_delta: self.session_delta,
                trade_count: 0,
                poc_level: level,
                poc_price: trade.price,
                levels: Vec::new(),
            });
        }
        let bar = self.bars.last_mut().expect("a trade always owns a bar");
        bar.end_timestamp_micros = trade.timestamp_micros;
        bar.high = bar.high.max(trade.price);
        bar.low = bar.low.min(trade.price);
        bar.close = trade.price;
        bar.trade_count = bar.trade_count.saturating_add(1);
        bar.total_volume += trade.volume;
        let delta = match side {
            AggressorSide::Buy => {
                bar.ask_volume += trade.volume;
                trade.volume
            }
            AggressorSide::Sell => {
                bar.bid_volume += trade.volume;
                -trade.volume
            }
            AggressorSide::Unknown => {
                bar.unknown_volume += trade.volume;
                0.0
            }
        };
        bar.delta += delta;
        bar.max_delta = bar.max_delta.max(bar.delta);
        bar.min_delta = bar.min_delta.min(bar.delta);
        self.session_delta += delta;
        bar.session_delta = self.session_delta;

        let level_position = bar
            .levels
            .binary_search_by_key(&level, |entry| entry.level)
            .unwrap_or_else(|position| {
                bar.levels.insert(
                    position,
                    FootprintLevel {
                        level,
                        price: level as f64 * self.options.tick_size,
                        ..FootprintLevel::default()
                    },
                );
                position
            });
        let cell = &mut bar.levels[level_position];
        cell.total_volume += trade.volume;
        match side {
            AggressorSide::Buy => cell.ask_volume += trade.volume,
            AggressorSide::Sell => cell.bid_volume += trade.volume,
            AggressorSide::Unknown => cell.unknown_volume += trade.volume,
        }
        cell.delta = cell.ask_volume - cell.bid_volume;
        recompute_bar_derived(bar, self.options.imbalance);
    }

    fn must_start_bar(&self, bar: &FootprintBar, trade: &FootprintTrade) -> bool {
        if bar.session_id != trade.session_id {
            return true;
        }
        match self.options.bars {
            FootprintBarAggregation::Time {
                interval_micros,
                anchor_micros,
            } => {
                aligned_bucket_start(
                    trade.timestamp_micros,
                    interval_micros as i64,
                    anchor_micros,
                ) != bar.start_timestamp_micros
            }
            FootprintBarAggregation::Trades { trades_per_bar } => bar.trade_count >= trades_per_bar,
            FootprintBarAggregation::Volume { volume_per_bar } => {
                bar.total_volume >= volume_per_bar
            }
        }
    }
}

impl ChartEngine {
    /// Add a first-class tick-driven footprint series. The shared chart time axis currently
    /// projects whole-second aligned time bars; analytical aggregation also supports trade-count
    /// and volume bars without pretending that several bars share one canonical second.
    pub fn add_footprint_series(
        &mut self,
        options: FootprintSeriesOptions,
    ) -> Result<SeriesId, FootprintError> {
        let id = self.add_series(SeriesKind::Footprint);
        if let Err(error) = self.configure_footprint_series(id, options) {
            self.remove_series(id);
            return Err(error);
        }
        Ok(id)
    }

    pub fn configure_footprint_series(
        &mut self,
        id: SeriesId,
        options: FootprintSeriesOptions,
    ) -> Result<(), FootprintError> {
        validate_chart_projection(options.aggregation)?;
        validate_visual_options(&options.visual)?;
        self.validate_series_id(id).map_err(series_error)?;
        let aggregator = FootprintAggregator::new(options.aggregation)?;
        let had_data = !self.data_layer().plot(id).is_empty();
        let series = self
            .series_entry_mut(id)
            .ok_or(FootprintError::UnknownSeries(id))?;
        series.kind = SeriesKind::Footprint;
        series.feature = None;
        series.footprint = Some(FootprintSeriesState {
            aggregator,
            visual: options.visual,
        });
        series.price_format = footprint_price_format(options.aggregation.tick_size);
        series.custom_frame = Default::default();
        self.data.set_rows_count_as_data(id, true);
        if had_data {
            let cleared = self.install_footprint_projection(
                id,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            );
            debug_assert!(cleared);
        }
        self.invalidate_frame_series(id);
        Ok(())
    }

    pub fn footprint_series_options(&self, id: SeriesId) -> Option<FootprintSeriesOptions> {
        let state = self.series_entry(id)?.footprint.as_ref()?;
        Some(FootprintSeriesOptions {
            aggregation: state.aggregator.options(),
            visual: state.visual.clone(),
        })
    }

    pub fn apply_footprint_series_options(
        &mut self,
        id: SeriesId,
        options: FootprintSeriesOptions,
    ) -> Result<(), FootprintError> {
        validate_chart_projection(options.aggregation)?;
        validate_visual_options(&options.visual)?;
        self.validate_series_id(id).map_err(series_error)?;
        let state = self
            .series_entry(id)
            .and_then(|series| series.footprint.as_ref())
            .ok_or(FootprintError::UnknownSeries(id))?;
        if state.aggregator.options() == options.aggregation {
            self.series_entry_mut(id)
                .and_then(|series| series.footprint.as_mut())
                .expect("validated footprint series")
                .visual = options.visual;
            self.invalidate_frame_series(id);
            return Ok(());
        }
        let trades = state.aggregator.trades().cloned().collect::<Vec<_>>();
        let mut aggregator = FootprintAggregator::new(options.aggregation)?;
        aggregator.set_trades(trades)?;
        let (times, open, high, low, close) = projection_columns(aggregator.bars())?;
        let series = self
            .series_entry_mut(id)
            .expect("validated footprint series");
        series
            .footprint
            .as_mut()
            .expect("validated footprint series")
            .clone_from(&FootprintSeriesState {
                aggregator,
                visual: options.visual,
            });
        series.price_format = footprint_price_format(options.aggregation.tick_size);
        if !self.install_footprint_projection(id, times, open, high, low, close) {
            return Err(FootprintError::UnknownSeries(id));
        }
        self.invalidate_frame_series(id);
        Ok(())
    }

    pub fn footprint_bars(&self, id: SeriesId) -> Option<&[FootprintBar]> {
        Some(self.series_entry(id)?.footprint.as_ref()?.aggregator.bars())
    }

    pub fn footprint_bar(&self, id: SeriesId, bar_index: usize) -> Option<&FootprintBar> {
        self.series_entry(id)?
            .footprint
            .as_ref()?
            .aggregator
            .bar(bar_index)
    }

    pub fn footprint_work_stats(&self, id: SeriesId) -> Option<FootprintWorkStats> {
        Some(
            self.series_entry(id)?
                .footprint
                .as_ref()?
                .aggregator
                .work_stats(),
        )
    }

    pub fn set_footprint_trades(
        &mut self,
        id: SeriesId,
        trades: Vec<FootprintTrade>,
    ) -> Result<(), FootprintError> {
        self.validate_series_id(id).map_err(series_error)?;
        let mut next = self
            .series_entry(id)
            .and_then(|series| series.footprint.as_ref())
            .ok_or(FootprintError::UnknownSeries(id))?
            .aggregator
            .clone();
        next.set_trades(trades)?;
        let (times, open, high, low, close) = projection_columns(next.bars())?;
        self.series_entry_mut(id)
            .and_then(|series| series.footprint.as_mut())
            .expect("validated footprint series")
            .aggregator = next;
        if !self.install_footprint_projection(id, times, open, high, low, close) {
            return Err(FootprintError::UnknownSeries(id));
        }
        self.invalidate_frame_series(id);
        Ok(())
    }

    pub fn update_footprint_trade(
        &mut self,
        id: SeriesId,
        trade: FootprintTrade,
    ) -> Result<FootprintUpdateKind, FootprintError> {
        self.update_footprint_trades(id, vec![trade])
    }

    /// Apply a live trade batch while synchronizing the shared time/scale projection once. A batch
    /// containing only tip events merges the active/recent bars in one data-layer operation;
    /// corrections or late events reconstruct once after the final canonical tape is known.
    pub fn update_footprint_trades(
        &mut self,
        id: SeriesId,
        trades: Vec<FootprintTrade>,
    ) -> Result<FootprintUpdateKind, FootprintError> {
        self.validate_series_id(id).map_err(series_error)?;
        if trades.is_empty() {
            return Ok(FootprintUpdateKind::Tip);
        }
        let state = self
            .series_entry(id)
            .and_then(|series| series.footprint.as_ref())
            .ok_or(FootprintError::UnknownSeries(id))?;
        let options = state.aggregator.options();
        validate_trade_batch(options, &trades)?;
        let historical = !state.aggregator.batch_is_tip(&trades);
        let previous_bar_count = state.aggregator.bars().len();
        if historical {
            let mut next = state.aggregator.historical_update_candidate();
            let result = next.update_trades(trades)?;
            debug_assert_eq!(result, FootprintUpdateKind::Historical);
            let (times, open, high, low, close) = projection_columns(next.bars())?;
            self.series_entry_mut(id)
                .and_then(|series| series.footprint.as_mut())
                .expect("validated footprint series")
                .aggregator = next;
            if !self.install_footprint_projection(id, times, open, high, low, close) {
                return Err(FootprintError::UnknownSeries(id));
            }
            self.invalidate_frame_series(id);
            return Ok(FootprintUpdateKind::Historical);
        }

        validate_projection_sessions(state.aggregator.bars(), options, &trades)?;
        let result = {
            let aggregator = &mut self
                .series_entry_mut(id)
                .and_then(|series| series.footprint.as_mut())
                .expect("validated footprint series")
                .aggregator;
            aggregator.update_trades(trades)?
        };
        debug_assert_eq!(result, FootprintUpdateKind::Tip);
        let from = previous_bar_count.saturating_sub(1);
        let (times, open, high, low, close) = projection_columns(
            &self.footprint_bars(id).expect("validated footprint series")[from..],
        )?;
        if self.update_footprint_projection_bars(id, times, open, high, low, close) == 0 {
            return Err(FootprintError::UnknownSeries(id));
        }
        self.invalidate_frame_series(id);
        Ok(result)
    }

    pub(crate) fn footprint_capacity_bytes(&self) -> usize {
        self.series
            .iter()
            .filter_map(|series| series.footprint.as_ref())
            .map(|state| state.aggregator.capacity_bytes())
            .sum()
    }

    pub(crate) fn trim_footprint_rows_front(&mut self, id: SeriesId, keep: usize) {
        if let Some(state) = self
            .series_entry_mut(id)
            .and_then(|series| series.footprint.as_mut())
        {
            state.aggregator.retain_last_bars(keep);
        }
    }
}

fn validate_chart_projection(options: FootprintAggregationOptions) -> Result<(), FootprintError> {
    match options.bars {
        FootprintBarAggregation::Time {
            interval_micros,
            anchor_micros,
        } if interval_micros >= MICROS_PER_SECOND as u64
            && interval_micros.is_multiple_of(MICROS_PER_SECOND as u64)
            && anchor_micros % MICROS_PER_SECOND == 0 =>
        {
            Ok(())
        }
        _ => Err(FootprintError::UnsupportedChartAggregation),
    }
}

fn validate_projection_sessions(
    bars: &[FootprintBar],
    options: FootprintAggregationOptions,
    trades: &[FootprintTrade],
) -> Result<(), FootprintError> {
    let FootprintBarAggregation::Time {
        interval_micros,
        anchor_micros,
    } = options.bars
    else {
        return Err(FootprintError::UnsupportedChartAggregation);
    };
    let mut sessions = HashMap::<i64, Option<u64>>::with_capacity(trades.len());
    for trade in trades {
        let bucket = aligned_bucket_start(
            trade.timestamp_micros,
            interval_micros as i64,
            anchor_micros,
        );
        if let Ok(index) = bars.binary_search_by_key(&bucket, |bar| bar.start_timestamp_micros) {
            if bars[index].session_id != trade.session_id {
                return Err(FootprintError::ProjectionTimeCollision);
            }
        }
        if let Some(previous) = sessions.insert(bucket, trade.session_id) {
            if previous != trade.session_id {
                return Err(FootprintError::ProjectionTimeCollision);
            }
        }
    }
    Ok(())
}

fn validate_visual_options(options: &FootprintVisualOptions) -> Result<(), FootprintError> {
    if options.font_size.is_finite() && (6.0..=48.0).contains(&options.font_size) {
        Ok(())
    } else {
        Err(FootprintError::InvalidVisualOptions)
    }
}

fn footprint_price_format(tick_size: f64) -> SeriesPriceFormat {
    let precision = (0..=15)
        .find(|precision| {
            let scaled = tick_size * 10_f64.powi(*precision as i32);
            (scaled - scaled.round()).abs() <= scaled.abs().max(1.0) * 1e-10
        })
        .unwrap_or(15);
    SeriesPriceFormat {
        kind: PriceFormatKind::Price,
        precision,
        min_move: tick_size,
        formatter: None,
    }
}

pub(crate) fn footprint_cell_price_bounds(low: f64, high: f64, tick_size: f64) -> (f64, f64) {
    (low - tick_size / 2.0, high + tick_size / 2.0)
}

fn series_error(error: SeriesIdError) -> FootprintError {
    match error {
        SeriesIdError::Unknown(id) => FootprintError::UnknownSeries(id),
        SeriesIdError::Stale(id) => FootprintError::StaleSeries(id),
    }
}

type ProjectionColumns = (Vec<i64>, Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>);

fn projection_columns(bars: &[FootprintBar]) -> Result<ProjectionColumns, FootprintError> {
    let mut times = Vec::with_capacity(bars.len());
    let mut open = Vec::with_capacity(bars.len());
    let mut high = Vec::with_capacity(bars.len());
    let mut low = Vec::with_capacity(bars.len());
    let mut close = Vec::with_capacity(bars.len());
    for bar in bars {
        let time = bar.start_timestamp_micros.div_euclid(MICROS_PER_SECOND);
        if times.last() == Some(&time) {
            return Err(FootprintError::ProjectionTimeCollision);
        }
        times.push(time);
        open.push(bar.open);
        high.push(bar.high);
        low.push(bar.low);
        close.push(bar.close);
    }
    Ok((times, open, high, low, close))
}

fn validate_options(options: FootprintAggregationOptions) -> Result<(), FootprintError> {
    if !options.tick_size.is_finite() || options.tick_size <= 0.0 {
        return Err(FootprintError::InvalidTickSize);
    }
    let valid_bars = match options.bars {
        FootprintBarAggregation::Time {
            interval_micros,
            anchor_micros,
        } => {
            let timestamp_span = (MAX_TIMESTAMP_MICROS - MIN_TIMESTAMP_MICROS) as u64;
            interval_micros > 0
                && interval_micros <= timestamp_span
                && (MIN_TIMESTAMP_MICROS..=MAX_TIMESTAMP_MICROS).contains(&anchor_micros)
        }
        FootprintBarAggregation::Trades { trades_per_bar } => trades_per_bar > 0,
        FootprintBarAggregation::Volume { volume_per_bar } => {
            volume_per_bar.is_finite() && volume_per_bar > 0.0
        }
    };
    if !valid_bars {
        return Err(FootprintError::InvalidAggregation);
    }
    let imbalance = options.imbalance;
    if !imbalance.ratio.is_finite()
        || imbalance.ratio < 1.0
        || !imbalance.minimum_volume.is_finite()
        || imbalance.minimum_volume < 0.0
        || imbalance.consecutive_levels == 0
    {
        return Err(FootprintError::InvalidImbalance);
    }
    Ok(())
}

fn validate_trade_batch(
    options: FootprintAggregationOptions,
    input: &[FootprintTrade],
) -> Result<(), FootprintError> {
    validate_options(options)?;
    let mut ids = HashMap::with_capacity(input.len());
    for (index, trade) in input.iter().enumerate() {
        validate_trade(options, trade, index)?;
        if let Some(trade_id) = trade.trade_id {
            if ids.insert(trade_id, index).is_some() {
                return Err(FootprintError::DuplicateTradeId { trade_id });
            }
        }
    }
    Ok(())
}

fn validate_trade(
    options: FootprintAggregationOptions,
    trade: &FootprintTrade,
    index: usize,
) -> Result<(), FootprintError> {
    if !(MIN_TIMESTAMP_MICROS..=MAX_TIMESTAMP_MICROS).contains(&trade.timestamp_micros) {
        return Err(FootprintError::InvalidTimestamp { index });
    }
    if !trade.price.is_finite() || !(MIN_SAFE_VALUE..=MAX_SAFE_VALUE).contains(&trade.price) {
        return Err(FootprintError::InvalidPrice { index });
    }
    let scaled_price = trade.price / options.tick_size;
    if !scaled_price.is_finite() || scaled_price < i64::MIN as f64 || scaled_price > i64::MAX as f64
    {
        return Err(FootprintError::InvalidPrice { index });
    }
    let level = price_level(trade.price, options.tick_size);
    let snapped = level as f64 * options.tick_size;
    let tolerance =
        options.tick_size.abs() * 1e-9 + f64::EPSILON * trade.price.abs().max(1.0) * 4.0;
    if (snapped - trade.price).abs() > tolerance {
        return Err(FootprintError::OffGridPrice { index });
    }
    if !trade.volume.is_finite() || trade.volume <= 0.0 || trade.volume > MAX_SAFE_VALUE {
        return Err(FootprintError::InvalidVolume { index });
    }
    if trade.bid.is_some_and(|price| {
        !price.is_finite() || !(MIN_SAFE_VALUE..=MAX_SAFE_VALUE).contains(&price)
    }) || trade.ask.is_some_and(|price| {
        !price.is_finite() || !(MIN_SAFE_VALUE..=MAX_SAFE_VALUE).contains(&price)
    }) || matches!((trade.bid, trade.ask), (Some(bid), Some(ask)) if bid > ask)
    {
        return Err(FootprintError::InvalidQuote { index });
    }
    Ok(())
}

fn trade_order_key(trade: &StoredTrade) -> (i64, u64, u64) {
    (
        trade.event.timestamp_micros,
        trade.event.sequence.unwrap_or(u64::MAX),
        trade.input_order,
    )
}

fn classify_aggressor(
    trade: &FootprintTrade,
    previous_price: Option<f64>,
    previous_side: AggressorSide,
) -> AggressorSide {
    if trade.aggressor != AggressorSide::Unknown {
        return trade.aggressor;
    }
    if trade.ask.is_some_and(|ask| trade.price >= ask) {
        return AggressorSide::Buy;
    }
    if trade.bid.is_some_and(|bid| trade.price <= bid) {
        return AggressorSide::Sell;
    }
    match previous_price.and_then(|previous| trade.price.partial_cmp(&previous)) {
        Some(core::cmp::Ordering::Greater) => AggressorSide::Buy,
        Some(core::cmp::Ordering::Less) => AggressorSide::Sell,
        Some(core::cmp::Ordering::Equal) => previous_side,
        None => AggressorSide::Unknown,
    }
}

fn price_level(price: f64, tick_size: f64) -> i64 {
    (price / tick_size).round() as i64
}

fn aligned_bucket_start(timestamp: i64, interval: i64, anchor: i64) -> i64 {
    anchor + (timestamp - anchor).div_euclid(interval) * interval
}

fn recompute_bar_derived(bar: &mut FootprintBar, options: FootprintImbalanceOptions) {
    for level in &mut bar.levels {
        level.bid_imbalance = false;
        level.ask_imbalance = false;
        level.stacked_bid_imbalance = false;
        level.stacked_ask_imbalance = false;
    }
    for index in 0..bar.levels.len() {
        let level = bar.levels[index].level;
        let lower_bid = index
            .checked_sub(1)
            .and_then(|lower| {
                (bar.levels[lower].level == level - 1).then_some(bar.levels[lower].bid_volume)
            })
            .unwrap_or(0.0);
        let upper_ask = bar
            .levels
            .get(index + 1)
            .filter(|upper| upper.level == level + 1)
            .map_or(0.0, |upper| upper.ask_volume);
        let ask = bar.levels[index].ask_volume;
        let bid = bar.levels[index].bid_volume;
        bar.levels[index].ask_imbalance = dominant(ask, lower_bid, options);
        bar.levels[index].bid_imbalance = dominant(bid, upper_ask, options);
    }
    mark_stacks(&mut bar.levels, options.consecutive_levels as usize, true);
    mark_stacks(&mut bar.levels, options.consecutive_levels as usize, false);

    if let Some(poc) = bar.levels.iter().max_by(|left, right| {
        left.total_volume
            .total_cmp(&right.total_volume)
            .then_with(|| {
                let left_distance = (left.price - bar.close).abs();
                let right_distance = (right.price - bar.close).abs();
                right_distance.total_cmp(&left_distance)
            })
            .then_with(|| right.level.cmp(&left.level))
    }) {
        bar.poc_level = poc.level;
        bar.poc_price = poc.price;
    }
}

fn dominant(value: f64, opposite: f64, options: FootprintImbalanceOptions) -> bool {
    value >= options.minimum_volume && (opposite == 0.0 || value / opposite >= options.ratio)
}

fn mark_stacks(levels: &mut [FootprintLevel], minimum: usize, ask: bool) {
    let mut start = 0;
    while start < levels.len() {
        let imbalanced = if ask {
            levels[start].ask_imbalance
        } else {
            levels[start].bid_imbalance
        };
        if !imbalanced {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < levels.len()
            && levels[end].level == levels[end - 1].level + 1
            && if ask {
                levels[end].ask_imbalance
            } else {
                levels[end].bid_imbalance
            }
        {
            end += 1;
        }
        if end - start >= minimum {
            for level in &mut levels[start..end] {
                if ask {
                    level.stacked_ask_imbalance = true;
                } else {
                    level.stacked_bid_imbalance = true;
                }
            }
        }
        start = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeris_charts_render::draw_list::Prim;

    fn trade(
        timestamp_micros: i64,
        price: f64,
        volume: f64,
        side: AggressorSide,
    ) -> FootprintTrade {
        FootprintTrade {
            timestamp_micros,
            price,
            volume,
            aggressor: side,
            bid: None,
            ask: None,
            sequence: None,
            trade_id: None,
            conditions: 0,
            session_id: Some(1),
        }
    }

    #[test]
    fn default_positive_footprint_roles_share_the_market_up_hue() {
        let visual = FootprintVisualOptions::default();
        let market_up = (MARKET_UP_RGB.0, MARKET_UP_RGB.1, MARKET_UP_RGB.2);
        for color in [
            visual.ask_color,
            visual.positive_delta_color,
            visual.stacked_ask_color,
        ] {
            assert_eq!((color.r(), color.g(), color.b()), market_up);
        }
        assert_eq!(visual.ask_color.a(), 70);
        assert_eq!(visual.positive_delta_color.a(), 110);
        assert_eq!(visual.stacked_ask_color.a(), 255);
    }

    #[test]
    fn tick_truth_drives_levels_poc_and_non_final_delta_extrema() {
        let mut aggregator = FootprintAggregator::new(FootprintAggregationOptions {
            tick_size: 1.0,
            bars: FootprintBarAggregation::Time {
                interval_micros: 60_000_000,
                anchor_micros: 0,
            },
            imbalance: FootprintImbalanceOptions {
                ratio: 10.0,
                minimum_volume: 1_000.0,
                consecutive_levels: 3,
            },
        })
        .unwrap();
        aggregator
            .set_trades(vec![
                trade(1_000_000, 100.0, 10.0, AggressorSide::Buy),
                trade(2_000_000, 101.0, 7.0, AggressorSide::Sell),
                trade(3_000_000, 101.0, 8.0, AggressorSide::Sell),
                trade(4_000_000, 100.0, 6.0, AggressorSide::Buy),
            ])
            .unwrap();

        let bar = &aggregator.bars()[0];
        assert_eq!(
            (bar.bid_volume, bar.ask_volume, bar.total_volume),
            (15.0, 16.0, 31.0)
        );
        assert_eq!((bar.delta, bar.max_delta, bar.min_delta), (1.0, 10.0, -5.0));
        assert_eq!(
            (bar.open, bar.high, bar.low, bar.close),
            (100.0, 101.0, 100.0, 100.0)
        );
        assert_eq!((bar.poc_price, bar.session_delta), (100.0, 1.0));
        assert_eq!(bar.levels.len(), 2);
        assert_eq!(
            (bar.levels[0].bid_volume, bar.levels[0].ask_volume),
            (0.0, 16.0)
        );
        assert_eq!(
            (bar.levels[1].bid_volume, bar.levels[1].ask_volume),
            (15.0, 0.0)
        );
    }

    #[test]
    fn stacked_imbalances_mark_complete_bid_and_ask_runs() {
        let options = FootprintAggregationOptions {
            tick_size: 1.0,
            bars: FootprintBarAggregation::Time {
                interval_micros: 60_000_000,
                anchor_micros: 0,
            },
            imbalance: FootprintImbalanceOptions {
                ratio: 3.0,
                minimum_volume: 20.0,
                consecutive_levels: 2,
            },
        };
        let mut ask = FootprintAggregator::new(options).unwrap();
        ask.set_trades(vec![
            trade(1, 100.0, 10.0, AggressorSide::Sell),
            trade(2, 101.0, 40.0, AggressorSide::Buy),
            trade(3, 101.0, 10.0, AggressorSide::Sell),
            trade(4, 102.0, 50.0, AggressorSide::Buy),
        ])
        .unwrap();
        let levels = &ask.bars()[0].levels;
        assert!(!levels[0].stacked_ask_imbalance);
        assert!(levels[1].ask_imbalance && levels[1].stacked_ask_imbalance);
        assert!(levels[2].ask_imbalance && levels[2].stacked_ask_imbalance);

        let mut bid = FootprintAggregator::new(options).unwrap();
        bid.set_trades(vec![
            trade(1, 100.0, 50.0, AggressorSide::Sell),
            trade(2, 101.0, 10.0, AggressorSide::Buy),
            trade(3, 101.0, 40.0, AggressorSide::Sell),
            trade(4, 102.0, 10.0, AggressorSide::Buy),
        ])
        .unwrap();
        let levels = &bid.bars()[0].levels;
        assert!(levels[0].bid_imbalance && levels[0].stacked_bid_imbalance);
        assert!(levels[1].bid_imbalance && levels[1].stacked_bid_imbalance);
        assert!(!levels[2].stacked_bid_imbalance);
    }

    #[test]
    fn host_side_quote_rule_tick_rule_and_ambiguity_are_deterministic() {
        let mut aggregator = FootprintAggregator::new(FootprintAggregationOptions {
            tick_size: 1.0,
            ..FootprintAggregationOptions::default()
        })
        .unwrap();
        let mut first = trade(1, 100.0, 2.0, AggressorSide::Unknown);
        first.bid = Some(99.0);
        first.ask = Some(100.0);
        let equal = trade(2, 100.0, 3.0, AggressorSide::Unknown);
        let lower = trade(3, 99.0, 5.0, AggressorSide::Unknown);
        aggregator.set_trades(vec![first, equal, lower]).unwrap();
        let bar = &aggregator.bars()[0];
        assert_eq!(
            (bar.ask_volume, bar.bid_volume, bar.unknown_volume),
            (5.0, 5.0, 0.0)
        );

        let mut ambiguous = FootprintAggregator::new(FootprintAggregationOptions {
            tick_size: 1.0,
            ..FootprintAggregationOptions::default()
        })
        .unwrap();
        ambiguous
            .set_trades(vec![trade(1, 100.0, 4.0, AggressorSide::Unknown)])
            .unwrap();
        assert_eq!(ambiguous.bars()[0].unknown_volume, 4.0);
    }

    #[test]
    fn late_event_rebuild_matches_sorted_history_and_live_tip_is_incremental() {
        let options = FootprintAggregationOptions {
            tick_size: 1.0,
            ..FootprintAggregationOptions::default()
        };
        let first = trade(1, 100.0, 8.0, AggressorSide::Buy);
        let late = trade(2, 100.0, 20.0, AggressorSide::Sell);
        let last = trade(3, 100.0, 15.0, AggressorSide::Buy);
        let mut streamed = FootprintAggregator::new(options).unwrap();
        streamed.update_trade(first.clone()).unwrap();
        streamed.update_trade(last.clone()).unwrap();
        assert_eq!(streamed.work_stats().incremental_ticks, 2);
        streamed.update_trade(late.clone()).unwrap();
        assert_eq!(streamed.work_stats().historical_rebuilds, 1);

        let mut historical = FootprintAggregator::new(options).unwrap();
        historical.set_trades(vec![first, late, last]).unwrap();
        assert_eq!(streamed.bars(), historical.bars());
        assert_eq!(
            (streamed.bars()[0].max_delta, streamed.bars()[0].min_delta),
            (8.0, -12.0)
        );
    }

    #[test]
    fn historical_batch_merges_final_tape_with_one_rebuild() {
        let options = FootprintAggregationOptions {
            tick_size: 1.0,
            ..FootprintAggregationOptions::default()
        };
        let mut aggregator = FootprintAggregator::new(options).unwrap();
        let mut history = (0..100)
            .map(|index| {
                let mut event = trade(
                    index * 1_000,
                    100.0 + (index % 3) as f64,
                    1.0,
                    AggressorSide::Buy,
                );
                event.trade_id = Some(index as u64);
                event
            })
            .collect::<Vec<_>>();
        aggregator.set_trades(history.clone()).unwrap();
        aggregator.reset_work_stats();

        let corrections = [10, 30, 70]
            .into_iter()
            .map(|index| {
                let mut event = history[index].clone();
                event.volume = 5.0;
                event.aggressor = AggressorSide::Sell;
                history[index] = event.clone();
                event
            })
            .collect();
        assert_eq!(
            aggregator.update_trades(corrections).unwrap(),
            FootprintUpdateKind::Historical
        );
        assert_eq!(
            aggregator.work_stats(),
            FootprintWorkStats {
                incremental_ticks: 0,
                historical_rebuilds: 1,
                rebuilt_ticks: 100,
            }
        );

        let mut expected = FootprintAggregator::new(options).unwrap();
        expected.set_trades(history).unwrap();
        assert_eq!(aggregator.bars(), expected.bars());
    }

    #[test]
    fn session_change_resets_cumulative_delta_and_forces_a_bar_boundary() {
        let mut aggregator = FootprintAggregator::new(FootprintAggregationOptions {
            tick_size: 1.0,
            ..FootprintAggregationOptions::default()
        })
        .unwrap();
        let first = trade(1, 100.0, 7.0, AggressorSide::Buy);
        let mut second = trade(2, 100.0, 3.0, AggressorSide::Sell);
        second.session_id = Some(2);
        aggregator.set_trades(vec![first, second]).unwrap();
        assert_eq!(aggregator.bars().len(), 2);
        assert_eq!(aggregator.bars()[0].session_delta, 7.0);
        assert_eq!(aggregator.bars()[1].session_delta, -3.0);
    }

    #[test]
    fn time_trade_and_volume_bar_modes_have_stable_boundaries() {
        let tape = vec![
            trade(1, 100.0, 4.0, AggressorSide::Buy),
            trade(2, 100.0, 4.0, AggressorSide::Buy),
            trade(3, 100.0, 4.0, AggressorSide::Buy),
            trade(4, 100.0, 4.0, AggressorSide::Buy),
            trade(5, 100.0, 4.0, AggressorSide::Buy),
        ];
        let mut trades = FootprintAggregator::new(FootprintAggregationOptions {
            tick_size: 1.0,
            bars: FootprintBarAggregation::Trades { trades_per_bar: 2 },
            ..FootprintAggregationOptions::default()
        })
        .unwrap();
        trades.set_trades(tape.clone()).unwrap();
        assert_eq!(
            trades
                .bars()
                .iter()
                .map(|bar| bar.trade_count)
                .collect::<Vec<_>>(),
            vec![2, 2, 1]
        );

        let mut volume = FootprintAggregator::new(FootprintAggregationOptions {
            tick_size: 1.0,
            bars: FootprintBarAggregation::Volume {
                volume_per_bar: 10.0,
            },
            ..FootprintAggregationOptions::default()
        })
        .unwrap();
        volume.set_trades(tape).unwrap();
        assert_eq!(
            volume
                .bars()
                .iter()
                .map(|bar| bar.total_volume)
                .collect::<Vec<_>>(),
            vec![12.0, 8.0]
        );
    }

    #[test]
    fn invalid_or_off_grid_batches_are_atomic() {
        let mut aggregator = FootprintAggregator::new(FootprintAggregationOptions {
            tick_size: 0.25,
            ..FootprintAggregationOptions::default()
        })
        .unwrap();
        aggregator
            .set_trades(vec![trade(1, 100.25, 1.0, AggressorSide::Buy)])
            .unwrap();
        let before = aggregator.bars().to_vec();
        let error = aggregator
            .set_trades(vec![trade(2, 100.30, 1.0, AggressorSide::Buy)])
            .unwrap_err();
        assert_eq!(error, FootprintError::OffGridPrice { index: 0 });
        assert_eq!(aggregator.bars(), before);
    }

    #[test]
    fn trade_id_correction_replaces_source_truth_and_rebuilds_delta_path() {
        let mut aggregator = FootprintAggregator::new(FootprintAggregationOptions {
            tick_size: 1.0,
            ..FootprintAggregationOptions::default()
        })
        .unwrap();
        let mut original = trade(1, 100.0, 12.0, AggressorSide::Buy);
        original.trade_id = Some(77);
        aggregator.set_trades(vec![original]).unwrap();

        let mut correction = trade(1, 100.0, 5.0, AggressorSide::Sell);
        correction.trade_id = Some(77);
        assert_eq!(
            aggregator.update_trade(correction).unwrap(),
            FootprintUpdateKind::Historical
        );
        let bar = &aggregator.bars()[0];
        assert_eq!(
            (bar.bid_volume, bar.ask_volume, bar.delta),
            (5.0, 0.0, -5.0)
        );
        assert_eq!((bar.max_delta, bar.min_delta), (0.0, -5.0));
        assert_eq!(aggregator.trades().len(), 1);
    }

    #[test]
    fn session_correction_validates_final_tape_and_collision_failure_is_atomic() {
        let mut chart = ChartEngine::new(800.0, 420.0, 1.0);
        chart
            .configure_footprint_series(
                0,
                FootprintSeriesOptions {
                    aggregation: FootprintAggregationOptions {
                        tick_size: 1.0,
                        ..FootprintAggregationOptions::default()
                    },
                    visual: FootprintVisualOptions::default(),
                },
            )
            .unwrap();
        let mut original = trade(1_000_000, 100.0, 4.0, AggressorSide::Buy);
        original.trade_id = Some(7);
        chart
            .set_footprint_trades(0, vec![original.clone()])
            .unwrap();

        let mut correction = original;
        correction.session_id = Some(2);
        assert_eq!(
            chart.update_footprint_trade(0, correction).unwrap(),
            FootprintUpdateKind::Historical
        );
        assert_eq!(chart.footprint_bars(0).unwrap()[0].session_id, Some(2));

        let before_bars = chart.footprint_bars(0).unwrap().to_vec();
        let before_stats = chart.footprint_work_stats(0).unwrap();
        let mut collision = trade(2_000_000, 101.0, 2.0, AggressorSide::Sell);
        collision.session_id = Some(3);
        assert_eq!(
            chart.update_footprint_trade(0, collision).unwrap_err(),
            FootprintError::ProjectionTimeCollision
        );
        assert_eq!(chart.footprint_bars(0).unwrap(), before_bars);
        assert_eq!(chart.footprint_work_stats(0).unwrap(), before_stats);
    }

    #[test]
    fn tick_size_owns_named_scale_format_autoscale_and_outer_cell_hit_bounds() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.5);
        let target = chart
            .add_price_scale(
                0,
                "footprint-dedicated",
                crate::PriceScaleSide::Right,
                None,
                true,
            )
            .unwrap();
        assert!(chart.try_set_series_pane_and_scale(0, 0, 1.0, "footprint-dedicated"));
        chart
            .configure_footprint_series(
                0,
                FootprintSeriesOptions {
                    aggregation: FootprintAggregationOptions {
                        tick_size: 1.0,
                        ..FootprintAggregationOptions::default()
                    },
                    visual: FootprintVisualOptions::default(),
                },
            )
            .unwrap();
        chart
            .set_footprint_trades(0, vec![trade(1_000_000, 100.0, 4.0, AggressorSide::Buy)])
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        chart.build_frame();

        assert_eq!(
            chart.price_scale_id_for_target(0, target),
            Some("footprint-dedicated")
        );
        let options: serde_json::Value =
            serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
        assert_eq!(options["price_format"]["precision"], 0);
        assert_eq!(options["price_format"]["min_move"], 1.0);
        let range = chart.price_scale_visible_range_for(0, target).unwrap();
        assert!(range.0 <= 99.5 && range.1 >= 100.5, "range was {range:?}");

        let x = chart.logical_to_coordinate(0.0).unwrap();
        let outer_cell_y = chart.series_price_to_coordinate(0, 100.45).unwrap();
        assert!(chart.hit_test_one_series(0, x, outer_cell_y).is_some());
    }

    #[test]
    fn chart_series_projects_scale_rows_but_keeps_tick_derived_queries() {
        let mut chart = ChartEngine::new(800.0, 420.0, 1.0);
        chart
            .configure_footprint_series(
                0,
                FootprintSeriesOptions {
                    aggregation: FootprintAggregationOptions {
                        tick_size: 1.0,
                        bars: FootprintBarAggregation::Time {
                            interval_micros: 60_000_000,
                            anchor_micros: 0,
                        },
                        imbalance: FootprintImbalanceOptions::default(),
                    },
                    visual: FootprintVisualOptions::default(),
                },
            )
            .unwrap();
        chart
            .set_footprint_trades(
                0,
                vec![
                    trade(1_000_000, 100.0, 10.0, AggressorSide::Buy),
                    trade(2_000_000, 100.0, 18.0, AggressorSide::Sell),
                    trade(61_000_000, 101.0, 7.0, AggressorSide::Buy),
                ],
            )
            .unwrap();

        assert_eq!(chart.series_kind(0), Some(SeriesKind::Footprint));
        assert_eq!(chart.data_layer().series_data(0).unwrap().0.len(), 2);
        assert_eq!(chart.footprint_bars(0).unwrap().len(), 2);
        assert_eq!(chart.footprint_bar(0, 0).unwrap().delta, -8.0);
        assert_eq!(chart.footprint_bar(0, 0).unwrap().max_delta, 10.0);
        assert_eq!(chart.footprint_bar(0, 0).unwrap().min_delta, -8.0);

        let update = chart
            .update_footprint_trade(0, trade(62_000_000, 102.0, 4.0, AggressorSide::Sell))
            .unwrap();
        assert_eq!(update, FootprintUpdateKind::Tip);
        assert_eq!(chart.data_layer().series_data(0).unwrap().0.len(), 2);
        assert_eq!(chart.footprint_bar(0, 1).unwrap().close, 102.0);
        assert!(chart.footprint_work_stats(0).unwrap().incremental_ticks > 0);

        let historical = chart
            .update_footprint_trade(0, trade(1_500_000, 100.0, 20.0, AggressorSide::Buy))
            .unwrap();
        assert_eq!(historical, FootprintUpdateKind::Historical);
        assert_eq!(chart.data_layer().series_data(0).unwrap().0.len(), 2);
        assert_eq!(chart.footprint_bar(0, 0).unwrap().delta, 12.0);
    }

    #[test]
    fn generic_ohlc_mutations_cannot_desynchronize_footprint_source_truth() {
        let mut chart = ChartEngine::new(800.0, 420.0, 1.0);
        let id = chart.add_series(SeriesKind::Footprint);
        assert!(chart.footprint_bars(id).is_some());
        chart
            .set_footprint_trades(id, vec![trade(1, 100.0, 3.0, AggressorSide::Buy)])
            .unwrap();
        let source_bar = chart.footprint_bar(id, 0).unwrap().clone();

        assert!(!chart.update_series_bar(id, 60.0, [1.0, 2.0, 0.0, 1.0]));
        assert_eq!(chart.series_pop(id, 1), None);
        assert_eq!(
            chart
                .set_series_data(id, &[60.0], &[1.0], &[2.0], &[0.0], &[1.0])
                .unwrap_err(),
            aeris_charts_core::model::data_validation::ValidationError::UnsupportedSeriesData(id)
        );
        assert_eq!(chart.footprint_bar(id, 0), Some(&source_bar));
        assert_eq!(chart.data_layer().series_data(id).unwrap().0.len(), 1);

        chart.convert_series_kind(id, SeriesKind::Candlestick);
        assert_eq!(chart.series_kind(id), Some(SeriesKind::Candlestick));
        chart.convert_series_kind(id, SeriesKind::Footprint);
        assert_eq!(chart.series_kind(id), Some(SeriesKind::Candlestick));

        let removable = chart
            .add_footprint_series(FootprintSeriesOptions::default())
            .unwrap();
        chart
            .set_footprint_trades(removable, vec![trade(2, 100.0, 3.0, AggressorSide::Buy)])
            .unwrap();
        assert!(chart.memory_usage().footprint_capacity_bytes > 0);
        assert!(chart.remove_series(removable));
        assert_eq!(chart.memory_usage().footprint_capacity_bytes, 0);
    }

    #[test]
    fn footprint_frame_owns_detail_summary_poc_and_stacked_highlights() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let visual = FootprintVisualOptions {
            font_size: 9.0,
            ..FootprintVisualOptions::default()
        };
        let stacked_ask = visual.stacked_ask_color;
        let poc = visual.poc_color;
        chart
            .configure_footprint_series(
                0,
                FootprintSeriesOptions {
                    aggregation: FootprintAggregationOptions {
                        tick_size: 1.0,
                        bars: FootprintBarAggregation::Time {
                            interval_micros: 60_000_000,
                            anchor_micros: 0,
                        },
                        imbalance: FootprintImbalanceOptions {
                            ratio: 3.0,
                            minimum_volume: 20.0,
                            consecutive_levels: 2,
                        },
                    },
                    visual,
                },
            )
            .unwrap();
        chart
            .set_footprint_trades(
                0,
                vec![
                    trade(1, 100.0, 10.0, AggressorSide::Sell),
                    trade(2, 101.0, 40.0, AggressorSide::Buy),
                    trade(3, 101.0, 10.0, AggressorSide::Sell),
                    trade(4, 102.0, 50.0, AggressorSide::Buy),
                ],
            )
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        chart.set_bar_spacing(100.0);
        let detailed = chart.build_frame();
        let segment = chart
            .frame_series_segments(0)
            .iter()
            .find(|segment| segment.series_id == Some(0))
            .unwrap();
        let prims = &detailed.panes[0].main[segment.start..segment.end];
        assert!(prims.iter().any(|primitive| {
            matches!(primitive, Prim::Text { text, .. } if text.contains("Δ 70") && text.contains("H 70") && text.contains("L -10"))
        }));
        assert!(prims.iter().any(|primitive| {
            matches!(primitive, Prim::Text { text, .. } if text.contains("V 110") && text.contains("B 20") && text.contains("A 90"))
        }));
        assert!(prims.iter().any(|primitive| {
            matches!(primitive, Prim::Rect { color, .. } if *color == stacked_ask)
        }));
        // POC reads as a side stripe on its row, not a full outline.
        assert!(prims.iter().any(|primitive| {
            matches!(primitive, Prim::Rect { rect, color }
                if *color == poc.solid() && rect.w == 2 && rect.h > 1)
        }));
        // Imbalance glyphs are bold so the signal scans at a glance.
        assert!(prims
            .iter()
            .any(|primitive| { matches!(primitive, Prim::Text { weight, .. } if *weight == 700) }));

        chart.set_theme(crate::ChartTheme::Light);
        let expected_text = Color::parse_css(&chart.options.get().layout.text_color).unwrap();
        let themed = chart.build_frame();
        let segment = chart
            .frame_series_segments(0)
            .iter()
            .find(|segment| segment.series_id == Some(0))
            .unwrap();
        let cell_text_colors = themed.panes[0].main[segment.start..segment.end]
            .iter()
            .filter_map(|primitive| match primitive {
                Prim::Text { text, color, .. }
                    if text
                        .chars()
                        .all(|character| character.is_ascii_digit() || character == '-') =>
                {
                    Some(*color)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(!cell_text_colors.is_empty());
        assert!(cell_text_colors.iter().all(|color| *color == expected_text));

        chart.set_bar_spacing(20.0);
        let cells = chart.build_frame();
        let segment = chart
            .frame_series_segments(0)
            .iter()
            .find(|segment| segment.series_id == Some(0))
            .unwrap();
        let prims = &cells.panes[0].main[segment.start..segment.end];
        assert!(!prims
            .iter()
            .any(|primitive| matches!(primitive, Prim::Text { .. })));
        assert!(prims.iter().any(|primitive| {
            matches!(primitive, Prim::Rect { rect, color }
                if *color == poc.solid() && rect.w == 2 && rect.h > 1)
        }));
        assert!(
            prims
                .iter()
                .filter(|primitive| matches!(primitive, Prim::Rect { .. }))
                .count()
                > 2
        );

        chart.set_bar_spacing(3.0);
        let summary = chart.build_frame();
        let segment = chart
            .frame_series_segments(0)
            .iter()
            .find(|segment| segment.series_id == Some(0))
            .unwrap();
        let prims = &summary.panes[0].main[segment.start..segment.end];
        assert!(!prims
            .iter()
            .any(|primitive| matches!(primitive, Prim::Text { .. })));
        assert!(prims
            .iter()
            .any(|primitive| matches!(primitive, Prim::Rect { color, .. } if *color == poc)));
    }

    #[test]
    fn footprint_volume_profile_scales_cell_intensity() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let visual = FootprintVisualOptions {
            font_size: 9.0,
            ..FootprintVisualOptions::default()
        };
        let bid = visual.bid_color;
        chart
            .configure_footprint_series(
                0,
                FootprintSeriesOptions {
                    aggregation: FootprintAggregationOptions {
                        tick_size: 1.0,
                        bars: FootprintBarAggregation::Time {
                            interval_micros: 60_000_000,
                            anchor_micros: 0,
                        },
                        // Disable imbalance so raw volume heat is directly comparable.
                        imbalance: FootprintImbalanceOptions {
                            ratio: 3.0,
                            minimum_volume: 1_000_000.0,
                            consecutive_levels: 3,
                        },
                    },
                    visual,
                },
            )
            .unwrap();
        chart
            .set_footprint_trades(
                0,
                vec![
                    trade(1, 100.0, 5.0, AggressorSide::Sell),
                    trade(2, 101.0, 50.0, AggressorSide::Sell),
                    // POC settles here so the two compared rows keep raw profile bars.
                    trade(3, 102.0, 200.0, AggressorSide::Sell),
                ],
            )
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        chart.set_bar_spacing(100.0);
        let frame = chart.build_frame();
        let segment = chart
            .frame_series_segments(0)
            .iter()
            .find(|segment| segment.series_id == Some(0))
            .unwrap();
        let prims = &frame.panes[0].main[segment.start..segment.end];
        let mut alphas = prims
            .iter()
            .filter_map(|primitive| match primitive {
                Prim::Rect { rect: _, color }
                    if color.r() == bid.r() && color.g() == bid.g() && color.b() == bid.b() =>
                {
                    Some(color.a())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        alphas.sort_unstable();
        alphas.dedup();
        assert!(
            alphas.len() >= 2,
            "quiet and heavy prints must differ in intensity, got {alphas:?}"
        );
        assert!(
            alphas.iter().all(|alpha| *alpha >= 26),
            "faint cells must keep their shape, got {alphas:?}"
        );
        // Profile silhouette: the heavy print's bar must extend further than the
        // quiet print's bar within the same half-width.
        let mut bar_widths = prims
            .iter()
            .filter_map(|primitive| match primitive {
                Prim::Rect { rect, color }
                    if color.r() == bid.r()
                        && color.g() == bid.g()
                        && color.b() == bid.b()
                        && color.a() > 26 =>
                {
                    Some(rect.w)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        bar_widths.sort_unstable();
        bar_widths.dedup();
        assert!(
            bar_widths.len() >= 2,
            "profile bars must grow with volume, got {bar_widths:?}"
        );
    }

    #[test]
    fn footprint_numbers_grow_into_tall_rows() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart
            .configure_footprint_series(
                0,
                FootprintSeriesOptions {
                    aggregation: FootprintAggregationOptions {
                        tick_size: 1.0,
                        bars: FootprintBarAggregation::Time {
                            interval_micros: 60_000_000,
                            anchor_micros: 0,
                        },
                        imbalance: FootprintImbalanceOptions {
                            ratio: 3.0,
                            minimum_volume: 1_000_000.0,
                            consecutive_levels: 3,
                        },
                    },
                    visual: FootprintVisualOptions {
                        font_size: 9.0,
                        ..FootprintVisualOptions::default()
                    },
                },
            )
            .unwrap();
        // Three levels across a tall pane: rows are far taller than the
        // configured 9px, so numbers must grow instead of floating tiny.
        chart
            .set_footprint_trades(
                0,
                vec![
                    trade(1, 100.0, 5.0, AggressorSide::Sell),
                    trade(2, 101.0, 50.0, AggressorSide::Sell),
                    trade(3, 102.0, 200.0, AggressorSide::Sell),
                ],
            )
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        chart.set_bar_spacing(100.0);
        let frame = chart.build_frame();
        let segment = chart
            .frame_series_segments(0)
            .iter()
            .find(|segment| segment.series_id == Some(0))
            .unwrap();
        let largest = frame.panes[0].main[segment.start..segment.end]
            .iter()
            .filter_map(|primitive| match primitive {
                Prim::Text { size, .. } => Some(*size),
                _ => None,
            })
            .fold(0.0f32, f32::max);
        assert!(
            largest > 9.0,
            "tall rows must grow numbers past the configured 9px, got {largest}"
        );
    }

    #[test]
    fn footprint_summary_drops_out_when_the_bar_cannot_fit_it() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart
            .configure_footprint_series(
                0,
                FootprintSeriesOptions {
                    aggregation: FootprintAggregationOptions {
                        tick_size: 1.0,
                        bars: FootprintBarAggregation::Time {
                            interval_micros: 60_000_000,
                            anchor_micros: 0,
                        },
                        ..FootprintAggregationOptions::default()
                    },
                    visual: FootprintVisualOptions {
                        font_size: 9.0,
                        ..FootprintVisualOptions::default()
                    },
                },
            )
            .unwrap();
        chart
            .set_footprint_trades(
                0,
                vec![
                    trade(1, 100.0, 5.0, AggressorSide::Sell),
                    trade(2, 101.0, 50.0, AggressorSide::Sell),
                    trade(3, 102.0, 200.0, AggressorSide::Sell),
                ],
            )
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        let summaries = |chart: &mut ChartEngine| {
            chart.build_frame().panes[0]
                .main
                .iter()
                .filter(
                    |primitive| matches!(primitive, Prim::Text { text, .. } if text.contains("Δ")),
                )
                .count()
        };
        // 9px type needs spacing >= 81; at 72 the summary would overprint neighbors.
        chart.set_bar_spacing(100.0);
        assert!(summaries(&mut chart) > 0);
        chart.set_bar_spacing(72.0);
        assert_eq!(summaries(&mut chart), 0);
    }

    #[test]
    fn footprint_single_imbalance_highlights_without_stack() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let visual = FootprintVisualOptions {
            font_size: 9.0,
            ..FootprintVisualOptions::default()
        };
        let stacked_ask = visual.stacked_ask_color;
        let single = Color::rgba(stacked_ask.r(), stacked_ask.g(), stacked_ask.b(), 215);
        chart
            .configure_footprint_series(
                0,
                FootprintSeriesOptions {
                    aggregation: FootprintAggregationOptions {
                        tick_size: 1.0,
                        bars: FootprintBarAggregation::Time {
                            interval_micros: 60_000_000,
                            anchor_micros: 0,
                        },
                        imbalance: FootprintImbalanceOptions {
                            ratio: 3.0,
                            minimum_volume: 20.0,
                            consecutive_levels: 3,
                        },
                    },
                    visual,
                },
            )
            .unwrap();
        chart
            .set_footprint_trades(
                0,
                vec![
                    trade(1, 100.0, 30.0, AggressorSide::Sell),
                    trade(2, 101.0, 100.0, AggressorSide::Buy),
                    trade(3, 102.0, 5.0, AggressorSide::Buy),
                ],
            )
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        chart.set_bar_spacing(100.0);
        let frame = chart.build_frame();
        let segment = chart
            .frame_series_segments(0)
            .iter()
            .find(|segment| segment.series_id == Some(0))
            .unwrap();
        let prims = &frame.panes[0].main[segment.start..segment.end];
        assert!(
            prims
                .iter()
                .any(|primitive| matches!(primitive, Prim::Rect { color, .. } if *color == single)),
            "a lone diagonal imbalance must still highlight its cell"
        );
        assert!(
            !prims.iter().any(
                |primitive| matches!(primitive, Prim::Rect { color, .. } if *color == stacked_ask)
            ),
            "a run shorter than the stacked threshold must not use the stacked treatment"
        );
    }

    #[test]
    fn live_batch_updates_many_ticks_with_one_projection_generation() {
        let mut chart = ChartEngine::new(600.0, 400.0, 1.0);
        chart
            .configure_footprint_series(
                0,
                FootprintSeriesOptions {
                    aggregation: FootprintAggregationOptions {
                        tick_size: 1.0,
                        ..FootprintAggregationOptions::default()
                    },
                    visual: FootprintVisualOptions::default(),
                },
            )
            .unwrap();
        chart
            .set_footprint_trades(0, vec![trade(1, 100.0, 1.0, AggressorSide::Buy)])
            .unwrap();
        let update = chart
            .update_footprint_trades(
                0,
                vec![
                    trade(2, 101.0, 2.0, AggressorSide::Buy),
                    trade(60_000_001, 102.0, 3.0, AggressorSide::Sell),
                    trade(60_000_002, 101.0, 4.0, AggressorSide::Buy),
                ],
            )
            .unwrap();
        assert_eq!(update, FootprintUpdateKind::Tip);
        assert_eq!(chart.data_layer().series_data(0).unwrap().0.len(), 2);
        assert_eq!(chart.footprint_bars(0).unwrap().len(), 2);
        assert_eq!(chart.footprint_work_stats(0).unwrap().incremental_ticks, 3);

        let update = chart
            .update_footprint_trades(
                0,
                vec![
                    trade(3, 100.0, 5.0, AggressorSide::Sell),
                    trade(60_000_003, 102.0, 1.0, AggressorSide::Buy),
                ],
            )
            .unwrap();
        assert_eq!(update, FootprintUpdateKind::Historical);
        assert_eq!(chart.footprint_bars(0).unwrap()[0].delta, -2.0);
    }

    #[test]
    fn retention_evicts_tape_and_bars_together_without_losing_session_delta_seed() {
        let mut chart = ChartEngine::new(600.0, 400.0, 1.0);
        chart
            .configure_footprint_series(
                0,
                FootprintSeriesOptions {
                    aggregation: FootprintAggregationOptions {
                        tick_size: 1.0,
                        ..FootprintAggregationOptions::default()
                    },
                    visual: FootprintVisualOptions::default(),
                },
            )
            .unwrap();
        chart
            .set_footprint_trades(
                0,
                vec![
                    trade(1, 100.0, 10.0, AggressorSide::Buy),
                    trade(60_000_001, 101.0, 5.0, AggressorSide::Sell),
                    trade(120_000_001, 102.0, 2.0, AggressorSide::Buy),
                ],
            )
            .unwrap();
        assert!(chart.set_series_max_points(0, Some(2)));
        let bars = chart.footprint_bars(0).unwrap();
        assert_eq!(bars.len(), 2);
        assert_eq!((bars[0].session_delta, bars[1].session_delta), (5.0, 7.0));
        assert_eq!(
            chart
                .series_entry(0)
                .unwrap()
                .footprint
                .as_ref()
                .unwrap()
                .aggregator
                .trades()
                .len(),
            2
        );

        assert_eq!(
            chart
                .update_footprint_trade(0, trade(60_000_002, 101.0, 1.0, AggressorSide::Buy),)
                .unwrap(),
            FootprintUpdateKind::Historical
        );
        let bars = chart.footprint_bars(0).unwrap();
        assert_eq!((bars[0].session_delta, bars[1].session_delta), (6.0, 8.0));
        assert_eq!(chart.data_layer().series_data(0).unwrap().0.len(), 2);
    }
}
