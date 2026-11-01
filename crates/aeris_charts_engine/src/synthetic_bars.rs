//! Deterministic price-action chart transforms.
//!
//! Hosts supply canonical OHLC bars. The engine owns the synthetic sequence and preserves each
//! output bar's source-time bounds; renderers consume ordinary series geometry and never rebuild
//! Renko, Line Break, Kagi, or Point & Figure state.
//!
//! Every transform is one streaming state machine. A full rebuild replays the source rows through
//! the same per-row step that live updates use, and a checkpoint taken before the newest source row
//! lets a replaced tip restore that checkpoint and replay only the tip row, so live work is bounded
//! by the bars the tip row forms rather than by history length.

use crate::BarSequencePoint;

pub const MAX_SYNTHETIC_BARS: usize = 1_000_000;
pub const MAX_SYNTHETIC_SOURCE_BARS: usize = 1_000_000;
pub const DEFAULT_LINE_BREAK_LINES: u32 = 3;
/// Each source row compares against this many prior lines, so the count bounds per-row work.
pub const MAX_LINE_BREAK_LINES: u32 = 100;
pub const DEFAULT_POINT_AND_FIGURE_REVERSAL_BOXES: u32 = 3;

/// Source prices a box transform reacts to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyntheticPriceSource {
    /// Each source row's close.
    #[default]
    Close,
    /// The row's high for advances and its low for declines. The current direction is tested
    /// first; the opposite extreme is used only when the first one forms nothing.
    HighLow,
}

/// How a Kagi `reversal_size` is measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KagiReversalMode {
    /// A price distance.
    #[default]
    Absolute,
    /// A percentage of the current line's extreme.
    Percent,
}

fn default_line_break_lines() -> u32 {
    DEFAULT_LINE_BREAK_LINES
}

fn default_reversal_boxes() -> u32 {
    DEFAULT_POINT_AND_FIGURE_REVERSAL_BOXES
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SyntheticBarOptions {
    RenkoFixed {
        box_size: f64,
        #[serde(default)]
        source: SyntheticPriceSource,
        #[serde(default)]
        wicks: bool,
    },
    /// Box size follows Wilder's ATR of the source rows; bricks start once the ATR is warm.
    RenkoAtr {
        period: u32,
        #[serde(default)]
        source: SyntheticPriceSource,
        #[serde(default)]
        wicks: bool,
    },
    LineBreak {
        #[serde(default = "default_line_break_lines")]
        lines: u32,
    },
    Kagi {
        reversal_size: f64,
        #[serde(default)]
        reversal_mode: KagiReversalMode,
    },
    PointAndFigure {
        box_size: f64,
        #[serde(default = "default_reversal_boxes")]
        reversal_boxes: u32,
        #[serde(default)]
        source: SyntheticPriceSource,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SyntheticSourceBar {
    pub timestamp_micros: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

/// Kagi line thickness state. A line turns yang (thick) when it rises above the prior shoulder
/// and yin (thin) when it falls below the prior waist; the first line is yang when it rises.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct KagiLine {
    pub yang_at_open: bool,
    /// The prior shoulder for a rising line or the prior waist for a falling line. Both are the
    /// previous line's open, so the value is final once this line exists.
    pub reference: Option<f64>,
}

impl KagiLine {
    /// Price where a line spanning `open..close` switches thickness, if it crosses the reference.
    pub fn switch_price(self, open: f64, close: f64) -> Option<f64> {
        let reference = self.reference?;
        let rising = close >= open;
        ((rising && !self.yang_at_open && close > reference)
            || (!rising && self.yang_at_open && close < reference))
            .then_some(reference)
    }

    pub fn yang_at_close(self, open: f64, close: f64) -> bool {
        self.yang_at_open != self.switch_price(open, close).is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct SyntheticBar {
    pub logical_index: u64,
    pub open_timestamp_micros: i64,
    pub close_timestamp_micros: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    /// Thickness state for Kagi lines; `None` for every other transform.
    pub kagi: Option<KagiLine>,
}

impl SyntheticBar {
    pub(crate) fn sequence_point(self) -> BarSequencePoint {
        BarSequencePoint {
            logical_index: self.logical_index,
            open_timestamp_micros: self.open_timestamp_micros,
            close_timestamp_micros: self.close_timestamp_micros,
        }
    }

    fn kagi_yang_at_close(self) -> Option<bool> {
        Some(self.kagi?.yang_at_close(self.open, self.close))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntheticBarError {
    InvalidOptions,
    InvalidSource { index: usize },
    OutOfOrderSource { index: usize },
    UnknownSeries(u32),
    UnsupportedSeries(u32),
    SequenceDomainInUse,
    Capacity,
    SourceCapacity,
}

impl core::fmt::Display for SyntheticBarError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidOptions => write!(f, "synthetic bar options are invalid"),
            Self::InvalidSource { index } => write!(f, "source bar {index} is invalid"),
            Self::OutOfOrderSource { index } => {
                write!(f, "source bar {index} is not ordered by timestamp")
            }
            Self::UnknownSeries(id) => write!(f, "unknown series {id}"),
            Self::UnsupportedSeries(id) => {
                write!(
                    f,
                    "series {id} must use candlestick or OHLC-bar presentation"
                )
            }
            Self::SequenceDomainInUse => write!(
                f,
                "a chart can own only one independent non-time bar sequence"
            ),
            Self::Capacity => write!(
                f,
                "synthetic bar sequence exceeds the {MAX_SYNTHETIC_BARS} bar limit"
            ),
            Self::SourceCapacity => write!(
                f,
                "synthetic source exceeds the {MAX_SYNTHETIC_SOURCE_BARS} row limit"
            ),
        }
    }
}

impl std::error::Error for SyntheticBarError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
}

/// Transform state that is not derivable from the emitted bars.
#[derive(Clone, Copy, Debug)]
struct TransformState {
    true_range_sum: f64,
    atr: Option<f64>,
    /// Renko wick extremes observed since the previous brick; NaN while empty.
    wick_high: f64,
    wick_low: f64,
}

impl Default for TransformState {
    fn default() -> Self {
        Self {
            true_range_sum: 0.0,
            atr: None,
            wick_high: f64::NAN,
            wick_low: f64::NAN,
        }
    }
}

/// Everything a tip replacement must restore. Kagi and Point & Figure extend the previous last
/// bar in place, so that bar is saved alongside the length.
#[derive(Clone, Copy, Debug)]
struct TipCheckpoint {
    bars_len: usize,
    last_bar: Option<SyntheticBar>,
    state: TransformState,
}

#[derive(Clone, Debug)]
pub struct SyntheticBarAggregator {
    options: SyntheticBarOptions,
    source: Vec<SyntheticSourceBar>,
    bars: Vec<SyntheticBar>,
    state: TransformState,
    /// State before the newest source row was applied.
    tip: Option<TipCheckpoint>,
}

impl SyntheticBarAggregator {
    pub fn new(options: SyntheticBarOptions) -> Result<Self, SyntheticBarError> {
        validate_options(options)?;
        Ok(Self {
            options,
            source: Vec::new(),
            bars: Vec::new(),
            state: TransformState::default(),
            tip: None,
        })
    }

    pub fn options(&self) -> SyntheticBarOptions {
        self.options
    }

    pub fn source(&self) -> &[SyntheticSourceBar] {
        &self.source
    }

    pub fn bars(&self) -> &[SyntheticBar] {
        &self.bars
    }

    pub fn set_source(&mut self, source: Vec<SyntheticSourceBar>) -> Result<(), SyntheticBarError> {
        validate_source(&source)?;
        let mut candidate = Self::new(self.options)?;
        candidate.source = source;
        candidate.rebuild()?;
        *self = candidate;
        Ok(())
    }

    /// Append a newer source row or replace the newest one. Returns the first bar index whose
    /// contents may differ, or `None` when the bar sequence is unchanged. Bars before that index
    /// are untouched; bars from it onward are the complete new tail.
    pub fn update_source(
        &mut self,
        bar: SyntheticSourceBar,
    ) -> Result<Option<usize>, SyntheticBarError> {
        validate_source(core::slice::from_ref(&bar))?;
        match self.source.last().map(|source| source.timestamp_micros) {
            Some(last) if bar.timestamp_micros < last => Err(SyntheticBarError::OutOfOrderSource {
                index: self.source.len(),
            }),
            Some(last) if bar.timestamp_micros == last => self.replace_tip(bar),
            _ => self.append(bar),
        }
    }

    fn append(&mut self, bar: SyntheticSourceBar) -> Result<Option<usize>, SyntheticBarError> {
        if self.source.len() == MAX_SYNTHETIC_SOURCE_BARS {
            return Err(SyntheticBarError::SourceCapacity);
        }
        let checkpoint = self.checkpoint();
        self.source.push(bar);
        if let Err(error) = self.step(self.source.len() - 1) {
            self.restore(checkpoint);
            self.source.pop();
            return Err(error);
        }
        self.tip = Some(checkpoint);
        Ok((self.bars.len() != checkpoint.bars_len
            || self.bars.last().copied() != checkpoint.last_bar)
            .then_some(checkpoint.bars_len.saturating_sub(1)))
    }

    fn replace_tip(&mut self, bar: SyntheticSourceBar) -> Result<Option<usize>, SyntheticBarError> {
        let index = self.source.len() - 1;
        let Some(checkpoint) = self.tip else {
            let mut candidate = self.clone();
            candidate.source[index] = bar;
            candidate.rebuild()?;
            *self = candidate;
            return Ok(Some(0));
        };
        let first = checkpoint.bars_len.saturating_sub(1);
        let previous_tail = self.bars[first..].to_vec();
        let previous_state = self.state;
        let previous_source = self.source[index];
        self.restore(checkpoint);
        self.source[index] = bar;
        if let Err(error) = self.step(index) {
            self.bars.truncate(first);
            self.bars.extend_from_slice(&previous_tail);
            self.state = previous_state;
            self.source[index] = previous_source;
            return Err(error);
        }
        Ok((self.bars[first..] != previous_tail[..]).then_some(first))
    }

    fn checkpoint(&self) -> TipCheckpoint {
        TipCheckpoint {
            bars_len: self.bars.len(),
            last_bar: self.bars.last().copied(),
            state: self.state,
        }
    }

    fn restore(&mut self, checkpoint: TipCheckpoint) {
        self.bars.truncate(checkpoint.bars_len);
        if let (Some(saved), Some(last)) = (checkpoint.last_bar, self.bars.last_mut()) {
            *last = saved;
        }
        self.state = checkpoint.state;
    }

    fn rebuild(&mut self) -> Result<(), SyntheticBarError> {
        self.bars.clear();
        self.state = TransformState::default();
        self.tip = None;
        for index in 0..self.source.len() {
            let checkpoint = self.checkpoint();
            self.step(index)?;
            self.tip = Some(checkpoint);
        }
        Ok(())
    }

    fn step(&mut self, index: usize) -> Result<(), SyntheticBarError> {
        match self.options {
            SyntheticBarOptions::RenkoFixed {
                box_size,
                source,
                wicks,
            } => self.renko_step(index, box_size, 0, source, wicks),
            SyntheticBarOptions::RenkoAtr {
                period,
                source,
                wicks,
            } => {
                let period = period as usize;
                let range = true_range(&self.source, index);
                if index < period {
                    self.state.true_range_sum += range;
                    if index + 1 == period {
                        self.state.atr = Some(self.state.true_range_sum / period as f64);
                    }
                } else {
                    self.state.atr = self
                        .state
                        .atr
                        .map(|previous| (previous * (period - 1) as f64 + range) / period as f64);
                }
                match self.state.atr {
                    Some(box_size) => self.renko_step(index, box_size, period - 1, source, wicks),
                    None => Ok(()),
                }
            }
            SyntheticBarOptions::LineBreak { lines } => self.line_break_step(index, lines as usize),
            SyntheticBarOptions::Kagi {
                reversal_size,
                reversal_mode,
            } => self.kagi_step(index, reversal_size, reversal_mode),
            SyntheticBarOptions::PointAndFigure {
                box_size,
                reversal_boxes,
                source,
            } => self.point_and_figure_step(index, box_size, reversal_boxes, source),
        }
    }

    fn last_direction(&self) -> Option<Direction> {
        self.bars.last().map(|bar| {
            if bar.close >= bar.open {
                Direction::Up
            } else {
                Direction::Down
            }
        })
    }

    fn renko_step(
        &mut self,
        index: usize,
        box_size: f64,
        anchor_index: usize,
        source: SyntheticPriceSource,
        wicks: bool,
    ) -> Result<(), SyntheticBarError> {
        if index <= anchor_index {
            return Ok(());
        }
        let row = self.source[index];
        if wicks {
            self.state.wick_high = self.state.wick_high.max(row.high);
            self.state.wick_low = self.state.wick_low.min(row.low);
        }
        // A flat warm-up yields a zero ATR; a zero box would emit bricks without bound.
        if !(box_size.is_finite() && box_size > 0.0) {
            return Ok(());
        }
        let prices = match source {
            SyntheticPriceSource::Close => [row.close, f64::NAN],
            SyntheticPriceSource::HighLow => match self.last_direction() {
                Some(Direction::Up) => [row.high, row.low],
                Some(Direction::Down) => [row.low, row.high],
                None if row.close >= row.open => [row.high, row.low],
                None => [row.low, row.high],
            },
        };
        for price in prices {
            if price.is_nan() || self.renko_bricks(index, price, box_size, anchor_index, wicks)? > 0
            {
                break;
            }
        }
        Ok(())
    }

    /// Emit every brick `price` completes from the current anchor. A reversal needs two boxes.
    fn renko_bricks(
        &mut self,
        index: usize,
        price: f64,
        box_size: f64,
        anchor_index: usize,
        wicks: bool,
    ) -> Result<usize, SyntheticBarError> {
        let time = self.source[index].timestamp_micros;
        let (mut anchor, mut anchor_time) = self.bars.last().map_or(
            (
                self.source[anchor_index].close,
                self.source[anchor_index].timestamp_micros,
            ),
            |bar| (bar.close, bar.close_timestamp_micros),
        );
        let mut direction = self.last_direction();
        let mut emitted = 0;
        loop {
            let up_distance = price - anchor;
            let down_distance = anchor - price;
            let next = match direction {
                None if up_distance >= box_size => Some((Direction::Up, anchor, anchor + box_size)),
                None if down_distance >= box_size => {
                    Some((Direction::Down, anchor, anchor - box_size))
                }
                Some(Direction::Up) if up_distance >= box_size => {
                    Some((Direction::Up, anchor, anchor + box_size))
                }
                Some(Direction::Up) if down_distance >= box_size * 2.0 => {
                    Some((Direction::Down, anchor - box_size, anchor - box_size * 2.0))
                }
                Some(Direction::Down) if down_distance >= box_size => {
                    Some((Direction::Down, anchor, anchor - box_size))
                }
                Some(Direction::Down) if up_distance >= box_size * 2.0 => {
                    Some((Direction::Up, anchor + box_size, anchor + box_size * 2.0))
                }
                _ => None,
            };
            let Some((next_direction, open, close)) = next else {
                break;
            };
            output(&mut self.bars, anchor_time, time, open, close, None)?;
            if wicks {
                if let Some(brick) = self.bars.last_mut() {
                    match next_direction {
                        Direction::Up => brick.low = brick.low.min(self.state.wick_low),
                        Direction::Down => brick.high = brick.high.max(self.state.wick_high),
                    }
                }
                self.state.wick_high = f64::NAN;
                self.state.wick_low = f64::NAN;
            }
            anchor = close;
            anchor_time = time;
            direction = Some(next_direction);
            emitted += 1;
        }
        Ok(emitted)
    }

    fn line_break_step(&mut self, index: usize, lines: usize) -> Result<(), SyntheticBarError> {
        let source = self.source[index];
        let Some(previous) = self.bars.last().copied() else {
            return output(
                &mut self.bars,
                source.timestamp_micros,
                source.timestamp_micros,
                source.open,
                source.close,
                None,
            );
        };
        let from = self.bars.len().saturating_sub(lines);
        let recent = &self.bars[from..];
        let high = recent
            .iter()
            .map(|bar| bar.high)
            .fold(f64::NEG_INFINITY, f64::max);
        let low = recent
            .iter()
            .map(|bar| bar.low)
            .fold(f64::INFINITY, f64::min);
        let rising = source.close > high;
        if rising || source.close < low {
            // A continuation line starts at the previous close; a reversal line starts at the
            // previous line's open, so it never overlaps the line it reverses.
            let reverses = (rising && previous.close < previous.open)
                || (!rising && previous.close > previous.open);
            output(
                &mut self.bars,
                previous.close_timestamp_micros,
                source.timestamp_micros,
                if reverses {
                    previous.open
                } else {
                    previous.close
                },
                source.close,
                None,
            )?;
        }
        Ok(())
    }

    fn kagi_step(
        &mut self,
        index: usize,
        reversal_size: f64,
        mode: KagiReversalMode,
    ) -> Result<(), SyntheticBarError> {
        if index == 0 {
            return Ok(());
        }
        let threshold = |pivot: f64| match mode {
            KagiReversalMode::Absolute => reversal_size,
            KagiReversalMode::Percent => pivot.abs() * reversal_size / 100.0,
        };
        let source = self.source[index];
        let Some(last) = self.bars.last().copied() else {
            let first = self.source[0];
            let distance = (source.close - first.close).abs();
            if distance > 0.0 && distance >= threshold(first.close) {
                output(
                    &mut self.bars,
                    first.timestamp_micros,
                    source.timestamp_micros,
                    first.close,
                    source.close,
                    Some(KagiLine {
                        yang_at_open: source.close > first.close,
                        reference: None,
                    }),
                )?;
            }
            return Ok(());
        };
        let rising = last.close >= last.open;
        let reversal = threshold(last.close);
        if (rising && source.close > last.close) || (!rising && source.close < last.close) {
            if let Some(current) = self.bars.last_mut() {
                current.close = source.close;
                current.high = current.high.max(source.close);
                current.low = current.low.min(source.close);
                current.close_timestamp_micros = source.timestamp_micros;
            }
        } else if (rising && source.close <= last.close - reversal)
            || (!rising && source.close >= last.close + reversal)
        {
            output(
                &mut self.bars,
                last.close_timestamp_micros,
                source.timestamp_micros,
                last.close,
                source.close,
                Some(KagiLine {
                    yang_at_open: last.kagi_yang_at_close().unwrap_or(rising),
                    reference: Some(last.open),
                }),
            )?;
        }
        Ok(())
    }

    fn point_and_figure_step(
        &mut self,
        index: usize,
        box_size: f64,
        reversal_boxes: u32,
        price_source: SyntheticPriceSource,
    ) -> Result<(), SyntheticBarError> {
        if index == 0 {
            return Ok(());
        }
        let source = self.source[index];
        let (high, low) = match price_source {
            SyntheticPriceSource::Close => (source.close, source.close),
            SyntheticPriceSource::HighLow => (source.high, source.low),
        };
        let reversal = box_size * f64::from(reversal_boxes);
        let whole_boxes = |distance: f64| (distance / box_size).floor() * box_size;
        let Some(last) = self.bars.last().copied() else {
            let first = self.source[0];
            let up = high - first.close >= box_size;
            let down = first.close - low >= box_size;
            let close = match (up, down) {
                (true, true) if source.close >= source.open => {
                    first.close + whole_boxes(high - first.close)
                }
                (true, false) => first.close + whole_boxes(high - first.close),
                (_, true) => first.close - whole_boxes(first.close - low),
                (false, false) => return Ok(()),
            };
            return output(
                &mut self.bars,
                first.timestamp_micros,
                source.timestamp_micros,
                first.close,
                close,
                None,
            );
        };
        let rising = last.close >= last.open;
        if rising && high >= last.close + box_size {
            let close = last.close + whole_boxes(high - last.close);
            if let Some(current) = self.bars.last_mut() {
                current.close = close;
                current.high = close;
                current.close_timestamp_micros = source.timestamp_micros;
            }
        } else if !rising && low <= last.close - box_size {
            let close = last.close - whole_boxes(last.close - low);
            if let Some(current) = self.bars.last_mut() {
                current.close = close;
                current.low = close;
                current.close_timestamp_micros = source.timestamp_micros;
            }
        } else if rising && low <= last.close - reversal {
            output(
                &mut self.bars,
                last.close_timestamp_micros,
                source.timestamp_micros,
                last.close - box_size,
                last.close - whole_boxes(last.close - low),
                None,
            )?;
        } else if !rising && high >= last.close + reversal {
            output(
                &mut self.bars,
                last.close_timestamp_micros,
                source.timestamp_micros,
                last.close + box_size,
                last.close + whole_boxes(high - last.close),
                None,
            )?;
        }
        Ok(())
    }

    fn bars_at(&self, clock_micros: Option<i64>) -> Result<Vec<SyntheticBar>, SyntheticBarError> {
        let Some(clock_micros) = clock_micros else {
            return Ok(self.bars.clone());
        };
        let visible = self
            .source
            .partition_point(|bar| bar.timestamp_micros <= clock_micros);
        let mut projection = Self::new(self.options)?;
        projection.source = self.source[..visible].to_vec();
        projection.rebuild()?;
        Ok(projection.bars)
    }
}

impl crate::ChartEngine {
    /// Configure one candlestick/bar series as the chart's price transform. Reconfiguring a
    /// series keeps the canonical source it already holds and rebuilds it atomically: an error
    /// leaves the previous transform in place.
    pub fn configure_synthetic_bar_series(
        &mut self,
        id: crate::SeriesId,
        options: SyntheticBarOptions,
    ) -> Result<(), SyntheticBarError> {
        let series = self
            .series_entry(id)
            .ok_or(SyntheticBarError::UnknownSeries(id))?;
        if !matches!(
            series.kind,
            crate::SeriesKind::Candlestick | crate::SeriesKind::Bar
        ) {
            return Err(SyntheticBarError::UnsupportedSeries(id));
        }
        let other_synthetic = self
            .synthetic_series
            .keys()
            .any(|&series_id| series_id != id);
        let non_time_stream = self.trade_streams.values().any(|stream| {
            !matches!(
                stream.options().bars,
                crate::FootprintBarAggregation::Time { .. }
            )
        });
        if other_synthetic || non_time_stream {
            return Err(SyntheticBarError::SequenceDomainInUse);
        }
        let mut aggregator = SyntheticBarAggregator::new(options)?;
        if let Some(existing) = self.synthetic_series.get(&id) {
            aggregator.set_source(existing.source().to_vec())?;
        }
        self.synthetic_series.insert(id, aggregator);
        self.refresh_synthetic_bar_projection(id)
    }

    pub fn set_synthetic_bar_source(
        &mut self,
        id: crate::SeriesId,
        source: Vec<SyntheticSourceBar>,
    ) -> Result<(), SyntheticBarError> {
        self.synthetic_series
            .get_mut(&id)
            .ok_or(SyntheticBarError::UnknownSeries(id))?
            .set_source(source)?;
        self.refresh_synthetic_bar_projection(id)
    }

    /// Append or replace the newest source row. Outside replay only the bars the tip row formed
    /// are rebuilt and reinstalled; the history before them is not revisited.
    pub fn update_synthetic_bar_source(
        &mut self,
        id: crate::SeriesId,
        source: SyntheticSourceBar,
    ) -> Result<(), SyntheticBarError> {
        let changed = self
            .synthetic_series
            .get_mut(&id)
            .ok_or(SyntheticBarError::UnknownSeries(id))?
            .update_source(source)?;
        let Some(from) = changed else {
            return Ok(());
        };
        if self.replay_clock_micros().is_none() && self.sequence_points().is_some() {
            return self.refresh_synthetic_bar_projection_from(id, from);
        }
        self.refresh_synthetic_bar_projection(id)
    }

    pub fn synthetic_bars(&self, id: crate::SeriesId) -> Option<&[SyntheticBar]> {
        Some(self.synthetic_series.get(&id)?.bars())
    }

    /// The displayed derived bar at a chart logical index. Source-time bounds come from the
    /// sequence sidecar and prices from the installed projection, so replay reports its clipped
    /// tip; the Kagi thickness state is prefix-stable and comes from the aggregator.
    pub fn synthetic_bar_at(&self, id: crate::SeriesId, index: usize) -> Option<SyntheticBar> {
        let aggregator = self.synthetic_series.get(&id)?;
        let point = *self.sequence_points()?.get(index)?;
        let (_, columns) = self.data.series_data(id)?;
        Some(SyntheticBar {
            logical_index: index as u64,
            open_timestamp_micros: point.open_timestamp_micros,
            close_timestamp_micros: point.close_timestamp_micros,
            open: *columns[0].get(index)?,
            high: *columns[1].get(index)?,
            low: *columns[2].get(index)?,
            close: *columns[3].get(index)?,
            kagi: aggregator.bars().get(index).and_then(|bar| bar.kagi),
        })
    }

    pub fn synthetic_bar_options(&self, id: crate::SeriesId) -> Option<SyntheticBarOptions> {
        Some(self.synthetic_series.get(&id)?.options())
    }

    pub(crate) fn synthetic_kagi_line(&self, id: crate::SeriesId, row: usize) -> Option<KagiLine> {
        self.synthetic_series.get(&id)?.bars().get(row)?.kagi
    }

    pub(crate) fn synthetic_series_ids(&self) -> Vec<crate::SeriesId> {
        let mut ids = self.synthetic_series.keys().copied().collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }

    pub(crate) fn refresh_synthetic_replay_projections(&mut self) -> Result<(), SyntheticBarError> {
        for id in self.synthetic_series_ids() {
            self.refresh_synthetic_bar_projection(id)?;
        }
        Ok(())
    }

    fn refresh_synthetic_bar_projection(
        &mut self,
        id: crate::SeriesId,
    ) -> Result<(), SyntheticBarError> {
        let bars = self
            .synthetic_series
            .get(&id)
            .ok_or(SyntheticBarError::UnknownSeries(id))?
            .bars_at(self.replay_clock_micros())?;
        let (points, open, high, low, close) = projection_columns(&bars);
        if self.install_sequence_projection_inner(id, points, open, high, low, close) {
            Ok(())
        } else {
            Err(SyntheticBarError::InvalidSource { index: 0 })
        }
    }

    fn refresh_synthetic_bar_projection_from(
        &mut self,
        id: crate::SeriesId,
        from: usize,
    ) -> Result<(), SyntheticBarError> {
        let projection = {
            let bars = self
                .synthetic_series
                .get(&id)
                .ok_or(SyntheticBarError::UnknownSeries(id))?
                .bars();
            projection_columns(&bars[from.min(bars.len())..])
        };
        let expected = projection.0.len();
        if self.replace_sequence_projection_tail(id, from, projection) == Some(expected) {
            Ok(())
        } else {
            Err(SyntheticBarError::InvalidSource { index: from })
        }
    }
}

fn projection_columns(bars: &[SyntheticBar]) -> crate::SequenceProjectionColumns {
    (
        bars.iter()
            .copied()
            .map(SyntheticBar::sequence_point)
            .collect(),
        bars.iter().map(|bar| bar.open).collect(),
        bars.iter().map(|bar| bar.high).collect(),
        bars.iter().map(|bar| bar.low).collect(),
        bars.iter().map(|bar| bar.close).collect(),
    )
}

pub(crate) fn validate_options(options: SyntheticBarOptions) -> Result<(), SyntheticBarError> {
    let positive = |value: f64| value.is_finite() && value > 0.0;
    let valid = match options {
        SyntheticBarOptions::RenkoFixed { box_size, .. } => positive(box_size),
        SyntheticBarOptions::RenkoAtr { period, .. } => period > 0,
        SyntheticBarOptions::LineBreak { lines } => (1..=MAX_LINE_BREAK_LINES).contains(&lines),
        SyntheticBarOptions::Kagi {
            reversal_size,
            reversal_mode,
        } => {
            positive(reversal_size)
                && (reversal_mode == KagiReversalMode::Absolute || reversal_size < 100.0)
        }
        SyntheticBarOptions::PointAndFigure {
            box_size,
            reversal_boxes,
            ..
        } => positive(box_size) && reversal_boxes > 0,
    };
    if valid {
        Ok(())
    } else {
        Err(SyntheticBarError::InvalidOptions)
    }
}

fn validate_source(source: &[SyntheticSourceBar]) -> Result<(), SyntheticBarError> {
    if source.len() > MAX_SYNTHETIC_SOURCE_BARS {
        return Err(SyntheticBarError::SourceCapacity);
    }
    for (index, bar) in source.iter().enumerate() {
        if ![bar.open, bar.high, bar.low, bar.close]
            .iter()
            .all(|value| value.is_finite())
            || bar.high < bar.open.max(bar.close)
            || bar.low > bar.open.min(bar.close)
            || bar.high < bar.low
        {
            return Err(SyntheticBarError::InvalidSource { index });
        }
        if index > 0 && bar.timestamp_micros <= source[index - 1].timestamp_micros {
            return Err(SyntheticBarError::OutOfOrderSource { index });
        }
    }
    Ok(())
}

fn output(
    bars: &mut Vec<SyntheticBar>,
    open_time: i64,
    close_time: i64,
    open: f64,
    close: f64,
    kagi: Option<KagiLine>,
) -> Result<(), SyntheticBarError> {
    if bars.len() == MAX_SYNTHETIC_BARS {
        return Err(SyntheticBarError::Capacity);
    }
    bars.push(SyntheticBar {
        logical_index: bars.len() as u64,
        open_timestamp_micros: open_time,
        close_timestamp_micros: close_time,
        open,
        high: open.max(close),
        low: open.min(close),
        close,
        kagi,
    });
    Ok(())
}

fn true_range(source: &[SyntheticSourceBar], index: usize) -> f64 {
    let bar = source[index];
    index.checked_sub(1).map_or(bar.high - bar.low, |prior| {
        let previous = source[prior].close;
        (bar.high - bar.low)
            .max((bar.high - previous).abs())
            .max((bar.low - previous).abs())
    })
}

#[cfg(test)]
mod tests;
