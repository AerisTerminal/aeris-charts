//! Pure, allocation-contained technical indicators.
//!
//! The indicator layer deliberately knows nothing about charts, panes, WebAssembly, or
//! rendering. It consumes a close/value slice and returns a derived value column that the
//! headless engine can install as an ordinary series. `None` represents the warm-up window.

pub mod volume_profile;

use std::{num::NonZeroUsize, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BollingerPoint {
    pub middle: Option<f64>,
    pub upper: Option<f64>,
    pub lower: Option<f64>,
}

/// Simple moving average. The first `period - 1` values are warm-up `None` entries.
pub fn sma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 {
        return vec![None; values.len()];
    }
    let mut out = vec![None; values.len()];
    let mut sum = 0.0;
    for (i, &value) in values.iter().enumerate() {
        sum += value;
        if i >= period {
            sum -= values[i - period];
        }
        if i + 1 >= period {
            out[i] = Some(sum / period as f64);
        }
    }
    out
}

/// Exponential moving average using the standard SMA seed, followed by the EMA recurrence.
pub fn ema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 {
        return vec![None; values.len()];
    }
    let mut out = vec![None; values.len()];
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut current = None;
    for (i, &value) in values.iter().enumerate() {
        current = match current {
            Some(previous) => Some(alpha * value + (1.0 - alpha) * previous),
            None if i + 1 >= period => {
                Some(values[i + 1 - period..=i].iter().sum::<f64>() / period as f64)
            }
            None => None,
        };
        out[i] = current;
    }
    out
}

/// Bollinger Bands using a simple moving-average center and population standard deviation.
pub fn bollinger(values: &[f64], period: usize, deviation: f64) -> Vec<BollingerPoint> {
    if period == 0 {
        return vec![
            BollingerPoint {
                middle: None,
                upper: None,
                lower: None
            };
            values.len()
        ];
    }
    let mut out = vec![
        BollingerPoint {
            middle: None,
            upper: None,
            lower: None
        };
        values.len()
    ];
    let factor = deviation.max(0.0);
    for i in period.saturating_sub(1)..values.len() {
        let window = &values[i + 1 - period..=i];
        let mean = window.iter().sum::<f64>() / period as f64;
        let variance = window.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / period as f64;
        let spread = variance.sqrt() * factor;
        out[i] = BollingerPoint {
            middle: Some(mean),
            upper: Some(mean + spread),
            lower: Some(mean - spread),
        };
    }
    out
}

/// Weighted moving average: linear weights 1..=period, the most recent bar heaviest.
pub fn wma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 {
        return vec![None; values.len()];
    }
    let mut out = vec![None; values.len()];
    let denominator = (period * (period + 1)) as f64 / 2.0;
    for i in period.saturating_sub(1)..values.len() {
        let window = &values[i + 1 - period..=i];
        let weighted: f64 = window
            .iter()
            .enumerate()
            .map(|(j, v)| (j + 1) as f64 * v)
            .sum();
        out[i] = Some(weighted / denominator);
    }
    out
}

/// Wilder's RSI over close values. The first value lands at index `period` (RSI consumes
/// `period` price changes); the warm-up window is `None`. Flat averages report 50, a
/// zero-loss run 100.
pub fn rsi(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = values.len();
    let mut out = vec![None; n];
    if period == 0 || n <= period {
        return out;
    }
    let mut avg_gain = 0.0;
    let mut avg_loss = 0.0;
    for i in 1..=period {
        let change = values[i] - values[i - 1];
        if change > 0.0 {
            avg_gain += change;
        } else {
            avg_loss -= change;
        }
    }
    avg_gain /= period as f64;
    avg_loss /= period as f64;
    out[period] = Some(rsi_value(avg_gain, avg_loss));
    for i in (period + 1)..n {
        let change = values[i] - values[i - 1];
        avg_gain = (avg_gain * (period as f64 - 1.0) + change.max(0.0)) / period as f64;
        avg_loss = (avg_loss * (period as f64 - 1.0) + (-change).max(0.0)) / period as f64;
        out[i] = Some(rsi_value(avg_gain, avg_loss));
    }
    out
}

fn rsi_value(avg_gain: f64, avg_loss: f64) -> f64 {
    if avg_loss == 0.0 {
        return if avg_gain == 0.0 { 50.0 } else { 100.0 };
    }
    100.0 - 100.0 / (1.0 + avg_gain / avg_loss)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MacdPoint {
    pub macd: Option<f64>,
    pub signal: Option<f64>,
    pub histogram: Option<f64>,
}

/// MACD: `ema(fast) - ema(slow)`; the signal line is an EMA of the macd line over `signal`
/// periods (seeded with the SMA of the first `signal` available macd values, like the plain
/// `ema` seed); the histogram is `macd - signal`.
pub fn macd(values: &[f64], fast: usize, slow: usize, signal: usize) -> Vec<MacdPoint> {
    let n = values.len();
    let mut out = vec![
        MacdPoint {
            macd: None,
            signal: None,
            histogram: None
        };
        n
    ];
    if fast == 0 || slow == 0 || signal == 0 {
        return out;
    }
    let fast_ema = ema(values, fast);
    let slow_ema = ema(values, slow);
    let alpha = 2.0 / (signal as f64 + 1.0);
    let mut sig: Option<f64> = None;
    let mut seen = 0usize;
    let mut seed_sum = 0.0;
    for i in 0..n {
        let line = match (fast_ema[i], slow_ema[i]) {
            (Some(f), Some(s)) => Some(f - s),
            _ => None,
        };
        if let Some(m) = line {
            seen += 1;
            sig = match sig {
                Some(previous) => Some(alpha * m + (1.0 - alpha) * previous),
                None => {
                    seed_sum += m;
                    if seen == signal {
                        Some(seed_sum / signal as f64)
                    } else {
                        None
                    }
                }
            };
        }
        let (sig_out, hist) = match (line, sig) {
            (Some(m), Some(s)) => (Some(s), Some(m - s)),
            _ => (None, None),
        };
        out[i] = MacdPoint {
            macd: line,
            signal: sig_out,
            histogram: hist,
        };
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StochasticPoint {
    pub k: Option<f64>,
    pub d: Option<f64>,
}

/// Stochastic %K = `100 * (C - LL(k)) / (HH(k) - LL(k))`, %D = SMA(%K, d). A zero-range
/// window carries the previous %K (50 for the first), matching the reference's flat-window
/// behavior. Columns are parallel high/low/close slices.
pub fn stochastic(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    k_period: usize,
    d_period: usize,
) -> Vec<StochasticPoint> {
    let n = closes.len().min(highs.len()).min(lows.len());
    let mut out = vec![StochasticPoint { k: None, d: None }; n];
    if k_period == 0 || d_period == 0 {
        return out;
    }
    let mut raw_k = vec![None; n];
    let mut previous_k: Option<f64> = None;
    for i in k_period.saturating_sub(1)..n {
        let hh = highs[i + 1 - k_period..=i]
            .iter()
            .fold(f64::NEG_INFINITY, |a, &v| a.max(v));
        let ll = lows[i + 1 - k_period..=i]
            .iter()
            .fold(f64::INFINITY, |a, &v| a.min(v));
        let range = hh - ll;
        let k = if range > 0.0 {
            100.0 * (closes[i] - ll) / range
        } else {
            previous_k.unwrap_or(50.0)
        };
        previous_k = Some(k);
        raw_k[i] = Some(k);
    }
    for i in 0..n {
        // %D is the simple mean of the trailing `d_period` %K values, valid once that window
        // sits fully inside the computed %K range (i >= k_period + d_period - 2).
        let d = if i + 1 >= k_period + d_period - 1 {
            let window = &raw_k[i + 1 - d_period..=i];
            Some(window.iter().map(|k| k.unwrap_or(0.0)).sum::<f64>() / d_period as f64)
        } else {
            None
        };
        out[i] = StochasticPoint { k: raw_k[i], d };
    }
    out
}

/// Wilder's ATR: TR = `max(H-L, |H-prevC|, |L-prevC|)`, seeded with the SMA of the first
/// `period` TRs (first value at index `period`), then Wilder-smoothed.
pub fn atr(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = closes.len().min(highs.len()).min(lows.len());
    let mut out = vec![None; n];
    if period == 0 || n <= period {
        return out;
    }
    let tr = |i: usize| {
        (highs[i] - lows[i])
            .max((highs[i] - closes[i - 1]).abs())
            .max((lows[i] - closes[i - 1]).abs())
    };
    let mut atr = 0.0;
    for i in 1..=period {
        atr += tr(i);
    }
    atr /= period as f64;
    out[period] = Some(atr);
    for (i, slot) in out.iter_mut().enumerate().skip(period + 1) {
        atr = (atr * (period as f64 - 1.0) + tr(i)) / period as f64;
        *slot = Some(atr);
    }
    out
}

/// Session-anchored VWAP of the typical price `(H+L+C)/3`, resetting cumulative sums at each
/// UTC day boundary (`times` are Unix seconds). `volumes` is a parallel column; an empty
/// slice (or a zero-volume bar) falls back to unit weight.
pub fn vwap(
    times: &[i64],
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    volumes: &[f64],
) -> Vec<Option<f64>> {
    let n = closes
        .len()
        .min(highs.len())
        .min(lows.len())
        .min(times.len());
    let mut out = vec![None; n];
    let mut cum_pv = 0.0;
    let mut cum_v = 0.0;
    let mut session: Option<i64> = None;
    for i in 0..n {
        let day = times[i].div_euclid(86_400);
        if session != Some(day) {
            session = Some(day);
            cum_pv = 0.0;
            cum_v = 0.0;
        }
        let typical = (highs[i] + lows[i] + closes[i]) / 3.0;
        let volume = volumes.get(i).copied().unwrap_or(1.0).max(0.0);
        cum_pv += typical * volume;
        cum_v += volume;
        out[i] = Some(if cum_v > 0.0 { cum_pv / cum_v } else { typical });
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VwapReset {
    Session,
    Weekly,
    Monthly,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VwapBandsPoint {
    pub basis: Option<f64>,
    pub standard_upper: Option<f64>,
    pub standard_lower: Option<f64>,
    pub percent_upper: Option<f64>,
    pub percent_lower: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VwapBandsOptions {
    pub reset: VwapReset,
    pub standard_deviation: f64,
    pub percent: f64,
}

/// Session/weekly/monthly VWAP with population standard-deviation and percentage bands.
pub fn vwap_bands(
    times: &[i64],
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    volumes: &[f64],
    options: VwapBandsOptions,
) -> Vec<VwapBandsPoint> {
    let n = closes
        .len()
        .min(highs.len())
        .min(lows.len())
        .min(times.len());
    let mut out = vec![
        VwapBandsPoint {
            basis: None,
            standard_upper: None,
            standard_lower: None,
            percent_upper: None,
            percent_lower: None,
        };
        n
    ];
    let mut state = VwapBandsState::default();
    for row in 0..n {
        out[row] = vwap_bands_step(
            &mut state,
            VwapBandsSample {
                time_unix_seconds: times[row],
                high: highs[row],
                low: lows[row],
                close: closes[row],
                volume: volumes.get(row).copied(),
            },
            options.reset,
            options.standard_deviation,
            options.percent,
        );
    }
    out
}

/// Borrowed canonical source columns used by the private rolling runtime. The engine owns the
/// source storage; this crate owns only formula state and derived output.
#[derive(Clone, Copy)]
pub struct IndicatorInput<'a> {
    pub times: &'a [i64],
    pub open: &'a [f64],
    pub high: &'a [f64],
    pub low: &'a [f64],
    pub close: &'a [f64],
    pub volume: &'a [f64],
}

/// Recursive formulas keep one checkpoint per 1024 rows plus the two tail states needed by
/// current-bar replacement. This is small enough to be negligible at 1M rows while adding at
/// most 1023 rows of work to a historical repair.
const CHECKPOINT_INTERVAL: usize = 1024;

#[derive(Clone, Copy, Debug)]
struct Checkpoint<T> {
    row: usize,
    state: T,
}

#[derive(Clone, Debug)]
struct RecursiveHistory<T> {
    checkpoints: Arc<Vec<Checkpoint<T>>>,
    tail: Option<T>,
    before_tail: Option<T>,
    len: usize,
}

impl<T: Copy + Default> RecursiveHistory<T> {
    fn new() -> Self {
        Self {
            checkpoints: Arc::new(Vec::new()),
            tail: None,
            before_tail: None,
            len: 0,
        }
    }

    fn begin(&mut self, n: usize, from: usize) -> (usize, T) {
        let from = from.min(n);
        if n == self.len && from + 1 == n {
            if let Some(state) = self.before_tail {
                if self
                    .checkpoints
                    .last()
                    .is_some_and(|checkpoint| checkpoint.row >= from)
                {
                    Arc::make_mut(&mut self.checkpoints).retain(|checkpoint| checkpoint.row < from);
                }
                return (from, state);
            }
        }
        if n >= self.len && from == self.len {
            if let Some(state) = self.tail {
                return (from, state);
            }
        }
        let checkpoint = self
            .checkpoints
            .iter()
            .rposition(|checkpoint| checkpoint.row < from);
        if let Some(position) = checkpoint {
            let checkpoint = self.checkpoints[position];
            if position + 1 < self.checkpoints.len() {
                Arc::make_mut(&mut self.checkpoints).truncate(position + 1);
            }
            (checkpoint.row + 1, checkpoint.state)
        } else {
            if !self.checkpoints.is_empty() {
                Arc::make_mut(&mut self.checkpoints).clear();
            }
            (0, T::default())
        }
    }

    fn finish(&mut self, n: usize, tail: Option<T>, before_tail: Option<T>) {
        self.len = n;
        self.tail = tail;
        self.before_tail = before_tail;
    }

    fn checkpoint(&mut self, row: usize, state: T) {
        if (row + 1).is_multiple_of(CHECKPOINT_INTERVAL) {
            Arc::make_mut(&mut self.checkpoints).push(Checkpoint { row, state });
        }
    }

    fn bytes(&self) -> usize {
        self.checkpoints.capacity() * std::mem::size_of::<Checkpoint<T>>()
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct EmaState {
    seen: usize,
    seed_sum: f64,
    value: f64,
}

fn ema_step(state: &mut EmaState, sample: f64, period: usize) -> Option<f64> {
    state.seen += 1;
    if state.seen <= period {
        state.seed_sum += sample;
        if state.seen == period {
            state.value = state.seed_sum / period as f64;
            Some(state.value)
        } else {
            None
        }
    } else {
        let alpha = 2.0 / (period as f64 + 1.0);
        state.value = alpha * sample + (1.0 - alpha) * state.value;
        Some(state.value)
    }
}

/// Sparse incremental EMA state for host-owned indexed sources that may contain hard gaps.
///
/// `None` samples reset the recursive accumulator and emit `None`. A later non-gap run must
/// accumulate a fresh SMA seed before EMA values resume. Historical repairs replay from the nearest
/// sparse checkpoint before `from`, while the writer is called only for the requested suffix.
#[derive(Clone, Debug)]
pub struct IncrementalEmaState {
    period: NonZeroUsize,
    history: RecursiveHistory<EmaState>,
    last_work_rows: usize,
}

/// One indexed OHLC sample consumed by [`IncrementalAtrState`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtrSample {
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

/// Sparse incremental Wilder ATR state for host-owned indexed sources that may contain hard gaps.
///
/// `None` samples reset the previous-close/seed state and emit `None`. Historical repairs replay
/// from the nearest sparse checkpoint before `from`, while the writer is called only for the
/// requested suffix.
#[derive(Clone, Debug)]
pub struct IncrementalAtrState {
    period: NonZeroUsize,
    history: RecursiveHistory<AtrState>,
    last_work_rows: usize,
}

impl IncrementalAtrState {
    /// Creates one empty incremental ATR runtime with the supplied non-zero period.
    #[must_use]
    pub fn new(period: NonZeroUsize) -> Self {
        Self {
            period,
            history: RecursiveHistory::new(),
            last_work_rows: 0,
        }
    }

    /// Rebuilds or repairs an indexed OHLC source without requiring contiguous temporary columns.
    pub fn rebuild_from_indexed<S, W>(
        &mut self,
        len: usize,
        from: usize,
        mut sample_at: S,
        mut write: W,
    ) where
        S: FnMut(usize) -> Option<AtrSample>,
        W: FnMut(usize, Option<f64>),
    {
        let requested = from.min(len);
        let (start, mut accumulator) = self.history.begin(len, requested);
        self.last_work_rows = len.saturating_sub(start);
        let mut tail = None;
        let mut before_tail = None;
        for row in start..len {
            let previous = accumulator;
            let value = match sample_at(row) {
                Some(sample) => atr_step(&mut accumulator, sample, self.period.get()),
                None => {
                    accumulator = AtrState::default();
                    None
                }
            };
            self.history.checkpoint(row, accumulator);
            if row >= requested {
                write(row, value);
            }
            if row + 1 == len {
                tail = Some(accumulator);
                before_tail = (row > 0).then_some(previous);
            }
        }
        self.history.finish(len, tail, before_tail);
    }

    /// Heap bytes retained by sparse recursive checkpoints.
    #[must_use]
    pub fn runtime_bytes(&self) -> usize {
        self.history.bytes()
    }

    /// Number of source rows replayed by the most recent rebuild or repair.
    #[must_use]
    pub fn last_work_rows(&self) -> usize {
        self.last_work_rows
    }
}

/// One indexed HLCV sample consumed by [`IncrementalVwapState`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VwapSample {
    pub time_unix_seconds: i64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    /// Missing volume follows the full-recomputation API and falls back to unit weight.
    pub volume: Option<f64>,
}

/// Sparse incremental session VWAP state for host-owned indexed sources that may contain hard gaps.
///
/// A hard gap resets cumulative session state. UTC day changes reset the session exactly as the
/// full [`vwap`] calculation does.
#[derive(Clone, Debug)]
pub struct IncrementalVwapState {
    history: RecursiveHistory<VwapState>,
    last_work_rows: usize,
}

impl IncrementalVwapState {
    /// Creates one empty incremental session VWAP runtime.
    #[must_use]
    pub fn new() -> Self {
        Self {
            history: RecursiveHistory::new(),
            last_work_rows: 0,
        }
    }

    /// Rebuilds or repairs an indexed HLCV source without requiring contiguous temporary columns.
    pub fn rebuild_from_indexed<S, W>(
        &mut self,
        len: usize,
        from: usize,
        mut sample_at: S,
        mut write: W,
    ) where
        S: FnMut(usize) -> Option<VwapSample>,
        W: FnMut(usize, Option<f64>),
    {
        let requested = from.min(len);
        let (start, mut accumulator) = self.history.begin(len, requested);
        self.last_work_rows = len.saturating_sub(start);
        let mut tail = None;
        let mut before_tail = None;
        for row in start..len {
            let previous = accumulator;
            let value = match sample_at(row) {
                Some(sample) => Some(vwap_step(&mut accumulator, sample)),
                None => {
                    accumulator = VwapState::default();
                    None
                }
            };
            self.history.checkpoint(row, accumulator);
            if row >= requested {
                write(row, value);
            }
            if row + 1 == len {
                tail = Some(accumulator);
                before_tail = (row > 0).then_some(previous);
            }
        }
        self.history.finish(len, tail, before_tail);
    }

    /// Heap bytes retained by sparse recursive checkpoints.
    #[must_use]
    pub fn runtime_bytes(&self) -> usize {
        self.history.bytes()
    }

    /// Number of source rows replayed by the most recent rebuild or repair.
    #[must_use]
    pub fn last_work_rows(&self) -> usize {
        self.last_work_rows
    }
}

impl Default for IncrementalVwapState {
    fn default() -> Self {
        Self::new()
    }
}

/// Sparse incremental RSI state for host-owned indexed numeric sources that may contain hard gaps.
#[derive(Clone, Debug)]
pub struct IncrementalRsiState {
    period: NonZeroUsize,
    history: RecursiveHistory<IndexedRsiState>,
    last_work_rows: usize,
}

impl IncrementalRsiState {
    #[must_use]
    pub fn new(period: NonZeroUsize) -> Self {
        Self {
            period,
            history: RecursiveHistory::new(),
            last_work_rows: 0,
        }
    }

    pub fn rebuild_from_indexed<S, W>(
        &mut self,
        len: usize,
        from: usize,
        mut sample_at: S,
        mut write: W,
    ) where
        S: FnMut(usize) -> Option<f64>,
        W: FnMut(usize, Option<f64>),
    {
        let requested = from.min(len);
        let (start, mut accumulator) = self.history.begin(len, requested);
        self.last_work_rows = len.saturating_sub(start);
        let mut tail = None;
        let mut before_tail = None;
        for row in start..len {
            let previous = accumulator;
            let value = match sample_at(row) {
                Some(sample) => indexed_rsi_step(&mut accumulator, sample, self.period.get()),
                None => {
                    accumulator = IndexedRsiState::default();
                    None
                }
            };
            self.history.checkpoint(row, accumulator);
            if row >= requested {
                write(row, value);
            }
            if row + 1 == len {
                tail = Some(accumulator);
                before_tail = (row > 0).then_some(previous);
            }
        }
        self.history.finish(len, tail, before_tail);
    }

    #[must_use]
    pub fn runtime_bytes(&self) -> usize {
        self.history.bytes()
    }

    #[must_use]
    pub fn last_work_rows(&self) -> usize {
        self.last_work_rows
    }
}

/// Sparse incremental MACD state for host-owned indexed numeric sources that may contain hard gaps.
#[derive(Clone, Debug)]
pub struct IncrementalMacdState {
    fast_period: NonZeroUsize,
    slow_period: NonZeroUsize,
    signal_period: NonZeroUsize,
    history: RecursiveHistory<MacdState>,
    last_work_rows: usize,
}

/// One indexed HLC sample consumed by [`IncrementalStochasticState`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StochasticSample {
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct IndexedStochasticState {
    previous_k: f64,
    has_k: bool,
    contiguous_samples: usize,
}

/// Sparse incremental Stochastic state with bounded `%D` tail retention.
#[derive(Clone, Debug)]
pub struct IncrementalStochasticState {
    k_period: NonZeroUsize,
    d_period: NonZeroUsize,
    history: RecursiveHistory<IndexedStochasticState>,
    tail_k: Vec<f64>,
    before_tail_k: Vec<f64>,
    source_len: usize,
    last_work_rows: usize,
}

impl IncrementalStochasticState {
    #[must_use]
    pub fn new(k_period: NonZeroUsize, d_period: NonZeroUsize) -> Self {
        Self {
            k_period,
            d_period,
            history: RecursiveHistory::new(),
            tail_k: Vec::new(),
            before_tail_k: Vec::new(),
            source_len: 0,
            last_work_rows: 0,
        }
    }

    pub fn rebuild_from_indexed<S, W>(
        &mut self,
        len: usize,
        from: usize,
        mut sample_at: S,
        mut write: W,
    ) where
        S: FnMut(usize) -> Option<StochasticSample>,
        W: FnMut(usize, StochasticPoint),
    {
        let requested = from.min(len);
        let k_period = self.k_period.get();
        let d_period = self.d_period.get();
        let realtime = requested >= self.source_len.saturating_sub(1) && len >= self.source_len;
        let state_from = if realtime {
            requested
        } else {
            requested.saturating_sub(d_period.saturating_sub(1))
        };
        let (start, mut state) = self.history.begin(len, state_from);
        self.last_work_rows = len.saturating_sub(start);
        let mut recent = std::collections::VecDeque::with_capacity(d_period.min(len));
        if realtime {
            if len == self.source_len && requested + 1 == len {
                recent.extend(self.before_tail_k.iter().copied());
            } else {
                recent.extend(self.tail_k.iter().copied());
            }
        }
        let mut before_tail_recent = Vec::new();
        let mut tail = None;
        let mut before_tail = None;
        for row in start..len {
            if row + 1 == len {
                before_tail_recent.clear();
                before_tail_recent.extend(recent.iter().copied());
            }
            let previous = state;
            let Some(current) = sample_at(row) else {
                state = IndexedStochasticState::default();
                recent.clear();
                self.history.checkpoint(row, state);
                if row >= requested {
                    write(row, StochasticPoint { k: None, d: None });
                }
                if row + 1 == len {
                    tail = Some(state);
                    before_tail = (row > 0).then_some(previous);
                }
                continue;
            };
            state.contiguous_samples = state.contiguous_samples.saturating_add(1);
            let k = if state.contiguous_samples < k_period {
                None
            } else {
                let window_start = row + 1 - k_period;
                let mut high = f64::NEG_INFINITY;
                let mut low = f64::INFINITY;
                let mut valid = true;
                for index in window_start..=row {
                    let sample = if index == row {
                        Some(current)
                    } else {
                        sample_at(index)
                    };
                    let Some(sample) = sample else {
                        valid = false;
                        break;
                    };
                    high = high.max(sample.high);
                    low = low.min(sample.low);
                }
                if valid {
                    let next = if high > low {
                        100.0 * (current.close - low) / (high - low)
                    } else if state.has_k {
                        state.previous_k
                    } else {
                        50.0
                    };
                    state.previous_k = next;
                    state.has_k = true;
                    Some(next)
                } else {
                    None
                }
            };
            let d = if let Some(k) = k {
                recent.push_back(k);
                if recent.len() > d_period {
                    recent.pop_front();
                }
                (recent.len() == d_period).then(|| recent.iter().sum::<f64>() / d_period as f64)
            } else {
                None
            };
            self.history.checkpoint(row, state);
            if row >= requested {
                write(row, StochasticPoint { k, d });
            }
            if row + 1 == len {
                tail = Some(state);
                before_tail = (row > 0).then_some(previous);
            }
        }
        self.history.finish(len, tail, before_tail);
        self.tail_k.clear();
        self.tail_k.extend(recent);
        self.before_tail_k.clear();
        self.before_tail_k.extend(before_tail_recent);
        self.source_len = len;
    }

    #[must_use]
    pub fn runtime_bytes(&self) -> usize {
        self.history
            .bytes()
            .saturating_add(self.tail_k.capacity() * std::mem::size_of::<f64>())
            .saturating_add(self.before_tail_k.capacity() * std::mem::size_of::<f64>())
    }

    #[must_use]
    pub fn last_work_rows(&self) -> usize {
        self.last_work_rows
    }
}

impl IncrementalMacdState {
    #[must_use]
    pub fn new(
        fast_period: NonZeroUsize,
        slow_period: NonZeroUsize,
        signal_period: NonZeroUsize,
    ) -> Self {
        Self {
            fast_period,
            slow_period,
            signal_period,
            history: RecursiveHistory::new(),
            last_work_rows: 0,
        }
    }

    pub fn rebuild_from_indexed<S, W>(
        &mut self,
        len: usize,
        from: usize,
        mut sample_at: S,
        mut write: W,
    ) where
        S: FnMut(usize) -> Option<f64>,
        W: FnMut(usize, MacdPoint),
    {
        let requested = from.min(len);
        let (start, mut accumulator) = self.history.begin(len, requested);
        self.last_work_rows = len.saturating_sub(start);
        let mut tail = None;
        let mut before_tail = None;
        for row in start..len {
            let previous = accumulator;
            let value = match sample_at(row) {
                Some(sample) => macd_step(
                    &mut accumulator,
                    sample,
                    self.fast_period.get(),
                    self.slow_period.get(),
                    self.signal_period.get(),
                ),
                None => {
                    accumulator = MacdState::default();
                    MacdPoint {
                        macd: None,
                        signal: None,
                        histogram: None,
                    }
                }
            };
            self.history.checkpoint(row, accumulator);
            if row >= requested {
                write(row, value);
            }
            if row + 1 == len {
                tail = Some(accumulator);
                before_tail = (row > 0).then_some(previous);
            }
        }
        self.history.finish(len, tail, before_tail);
    }

    #[must_use]
    pub fn runtime_bytes(&self) -> usize {
        self.history.bytes()
    }

    #[must_use]
    pub fn last_work_rows(&self) -> usize {
        self.last_work_rows
    }
}

impl IncrementalEmaState {
    /// Creates one empty incremental EMA runtime with the supplied non-zero period.
    #[must_use]
    pub fn new(period: NonZeroUsize) -> Self {
        Self {
            period,
            history: RecursiveHistory::new(),
            last_work_rows: 0,
        }
    }

    /// Rebuilds or repairs an indexed source without requiring a contiguous temporary value slice.
    ///
    /// `sample_at` is called for every source row that must be replayed to restore recursive state.
    /// `write` is called only for rows in the requested `from..len` suffix. This lets a host convert
    /// fixed-point values lazily for the rows actually visited and patch only the dirty output range.
    pub fn rebuild_from_indexed<S, W>(
        &mut self,
        len: usize,
        from: usize,
        mut sample_at: S,
        mut write: W,
    ) where
        S: FnMut(usize) -> Option<f64>,
        W: FnMut(usize, Option<f64>),
    {
        let requested = from.min(len);
        let (start, mut accumulator) = self.history.begin(len, requested);
        self.last_work_rows = len.saturating_sub(start);
        let mut tail = None;
        let mut before_tail = None;
        for row in start..len {
            let previous = accumulator;
            let value = match sample_at(row) {
                Some(sample) => ema_step(&mut accumulator, sample, self.period.get()),
                None => {
                    accumulator = EmaState::default();
                    None
                }
            };
            self.history.checkpoint(row, accumulator);
            if row >= requested {
                write(row, value);
            }
            if row + 1 == len {
                tail = Some(accumulator);
                before_tail = (row > 0).then_some(previous);
            }
        }
        self.history.finish(len, tail, before_tail);
    }

    /// Heap bytes retained by sparse recursive checkpoints.
    #[must_use]
    pub fn runtime_bytes(&self) -> usize {
        self.history.bytes()
    }

    /// Number of source rows replayed by the most recent rebuild or repair.
    #[must_use]
    pub fn last_work_rows(&self) -> usize {
        self.last_work_rows
    }
}

/// Maximum number of output columns retained by one built-in indicator runtime.
pub const MAX_OUTPUTS: usize = 5;

#[derive(Clone, Copy, Debug, Default)]
struct RsiState {
    gain: f64,
    loss: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct IndexedRsiState {
    previous_close: Option<f64>,
    seen_changes: usize,
    rsi: RsiState,
}

#[derive(Clone, Copy, Debug, Default)]
struct AtrState {
    previous_close: Option<f64>,
    seen: usize,
    seed_sum: f64,
    value: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct MacdState {
    fast: EmaState,
    slow: EmaState,
    signal: EmaState,
}

fn indexed_rsi_step(state: &mut IndexedRsiState, sample: f64, period: usize) -> Option<f64> {
    let previous_close = state.previous_close.replace(sample)?;
    let change = sample - previous_close;
    state.seen_changes = state.seen_changes.saturating_add(1);
    rsi_change_step(&mut state.rsi, change, period, state.seen_changes)
}

fn rsi_change_step(
    state: &mut RsiState,
    change: f64,
    period: usize,
    seen_changes: usize,
) -> Option<f64> {
    if seen_changes <= period {
        state.gain += change.max(0.0);
        state.loss += (-change).max(0.0);
        if seen_changes == period {
            state.gain /= period as f64;
            state.loss /= period as f64;
            Some(rsi_value(state.gain, state.loss))
        } else {
            None
        }
    } else {
        state.gain = (state.gain * (period as f64 - 1.0) + change.max(0.0)) / period as f64;
        state.loss = (state.loss * (period as f64 - 1.0) + (-change).max(0.0)) / period as f64;
        Some(rsi_value(state.gain, state.loss))
    }
}

fn macd_step(
    state: &mut MacdState,
    sample: f64,
    fast_period: usize,
    slow_period: usize,
    signal_period: usize,
) -> MacdPoint {
    let fast = ema_step(&mut state.fast, sample, fast_period);
    let slow = ema_step(&mut state.slow, sample, slow_period);
    let line = fast.zip(slow).map(|(fast, slow)| fast - slow);
    let signal = line.and_then(|line| ema_step(&mut state.signal, line, signal_period));
    MacdPoint {
        macd: line,
        signal,
        histogram: line.zip(signal).map(|(line, signal)| line - signal),
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct VwapState {
    day: i64,
    cumulative_pv: f64,
    cumulative_volume: f64,
    initialized: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct VwapBandsState {
    period: i64,
    cumulative_pv: f64,
    cumulative_pv2: f64,
    cumulative_volume: f64,
    initialized: bool,
}

#[derive(Clone, Copy)]
struct VwapBandsSample {
    time_unix_seconds: i64,
    high: f64,
    low: f64,
    close: f64,
    volume: Option<f64>,
}

fn atr_step(state: &mut AtrState, sample: AtrSample, period: usize) -> Option<f64> {
    let previous_close = state.previous_close.replace(sample.close)?;
    let tr = (sample.high - sample.low)
        .max((sample.high - previous_close).abs())
        .max((sample.low - previous_close).abs());
    state.seen += 1;
    if state.seen <= period {
        state.seed_sum += tr;
        if state.seen == period {
            state.value = state.seed_sum / period as f64;
            Some(state.value)
        } else {
            None
        }
    } else {
        state.value = (state.value * (period as f64 - 1.0) + tr) / period as f64;
        Some(state.value)
    }
}

fn vwap_step(state: &mut VwapState, sample: VwapSample) -> f64 {
    let day = sample.time_unix_seconds.div_euclid(86_400);
    if !state.initialized || state.day != day {
        *state = VwapState {
            day,
            initialized: true,
            ..VwapState::default()
        };
    }
    let typical = (sample.high + sample.low + sample.close) / 3.0;
    let volume = sample.volume.unwrap_or(1.0).max(0.0);
    state.cumulative_pv += typical * volume;
    state.cumulative_volume += volume;
    if state.cumulative_volume > 0.0 {
        state.cumulative_pv / state.cumulative_volume
    } else {
        typical
    }
}

fn vwap_bands_step(
    state: &mut VwapBandsState,
    sample: VwapBandsSample,
    reset: VwapReset,
    standard_deviation: f64,
    percent: f64,
) -> VwapBandsPoint {
    let period = vwap_period_key(sample.time_unix_seconds, reset);
    if !state.initialized || state.period != period {
        *state = VwapBandsState {
            period,
            initialized: true,
            ..VwapBandsState::default()
        };
    }
    let typical = (sample.high + sample.low + sample.close) / 3.0;
    let volume = sample.volume.unwrap_or(1.0).max(0.0);
    state.cumulative_pv += typical * volume;
    state.cumulative_pv2 += typical * typical * volume;
    state.cumulative_volume += volume;
    let basis = if state.cumulative_volume > 0.0 {
        state.cumulative_pv / state.cumulative_volume
    } else {
        typical
    };
    let variance = if state.cumulative_volume > 0.0 {
        (state.cumulative_pv2 / state.cumulative_volume - basis * basis).max(0.0)
    } else {
        0.0
    };
    let spread = variance.sqrt() * standard_deviation.max(0.0);
    let percent = percent.max(0.0) / 100.0;
    VwapBandsPoint {
        basis: Some(basis),
        standard_upper: Some(basis + spread),
        standard_lower: Some(basis - spread),
        percent_upper: Some(basis * (1.0 + percent)),
        percent_lower: Some(basis * (1.0 - percent)),
    }
}

fn vwap_period_key(seconds: i64, reset: VwapReset) -> i64 {
    let days = seconds.div_euclid(86_400);
    match reset {
        VwapReset::Session => days,
        VwapReset::Weekly => days.div_euclid(7),
        VwapReset::Monthly => month_key(days),
    }
}

// Proleptic Gregorian month key from Unix days (Howard Hinnant's civil-from-days reduction).
fn month_key(days: i64) -> i64 {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096).div_euclid(365);
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2).div_euclid(153);
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    year * 12 + month
}

#[derive(Clone, Debug)]
enum IncrementalKind {
    Sma {
        period: usize,
    },
    Ema {
        period: usize,
        state: RecursiveHistory<EmaState>,
    },
    EmaRibbon {
        periods: [usize; MAX_OUTPUTS],
        states: Box<[RecursiveHistory<EmaState>; MAX_OUTPUTS]>,
    },
    Bollinger {
        period: usize,
        deviation: f64,
    },
    Rsi {
        period: usize,
        state: RecursiveHistory<RsiState>,
    },
    Macd {
        fast_period: usize,
        slow_period: usize,
        signal_period: usize,
        state: RecursiveHistory<MacdState>,
    },
    Stochastic {
        k_period: usize,
        d_period: usize,
        state: RecursiveHistory<f64>,
        tail_k: Vec<f64>,
        source_len: usize,
    },
    Atr {
        period: usize,
        state: RecursiveHistory<AtrState>,
    },
    Vwap {
        state: RecursiveHistory<VwapState>,
    },
    VwapBands {
        reset: VwapReset,
        standard_deviation: f64,
        percent: f64,
        state: RecursiveHistory<VwapBandsState>,
    },
    Wma {
        period: usize,
    },
}

/// Incremental formula state. Output columns are short-lived transfer buffers: a full rebuild
/// moves them into the engine's canonical output series, and tail updates reuse only tail-sized
/// capacity. Historical formula state is sparse rather than source-row aligned.
#[derive(Clone, Debug)]
pub struct IncrementalState {
    kind: IncrementalKind,
    outputs: [Vec<f64>; MAX_OUTPUTS],
    output_from: [usize; MAX_OUTPUTS],
    output_count: usize,
    last_work_rows: usize,
}

impl IncrementalState {
    fn new(kind: IncrementalKind, output_count: usize) -> Self {
        Self {
            kind,
            outputs: std::array::from_fn(|_| Vec::new()),
            output_from: [0; MAX_OUTPUTS],
            output_count,
            last_work_rows: 0,
        }
    }

    pub fn sma(period: usize) -> Self {
        Self::new(IncrementalKind::Sma { period }, 1)
    }

    pub fn ema(period: usize) -> Self {
        Self::new(
            IncrementalKind::Ema {
                period,
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn ema_ribbon(periods: [usize; MAX_OUTPUTS]) -> Self {
        Self::new(
            IncrementalKind::EmaRibbon {
                periods,
                states: Box::new(std::array::from_fn(|_| RecursiveHistory::new())),
            },
            MAX_OUTPUTS,
        )
    }

    pub fn bollinger(period: usize, deviation: f64) -> Self {
        Self::new(IncrementalKind::Bollinger { period, deviation }, 3)
    }

    pub fn rsi(period: usize) -> Self {
        Self::new(
            IncrementalKind::Rsi {
                period,
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn macd(fast_period: usize, slow_period: usize, signal_period: usize) -> Self {
        Self::new(
            IncrementalKind::Macd {
                fast_period,
                slow_period,
                signal_period,
                state: RecursiveHistory::new(),
            },
            3,
        )
    }

    pub fn stochastic(k_period: usize, d_period: usize) -> Self {
        Self::new(
            IncrementalKind::Stochastic {
                k_period,
                d_period,
                state: RecursiveHistory::new(),
                tail_k: Vec::new(),
                source_len: 0,
            },
            2,
        )
    }

    pub fn atr(period: usize) -> Self {
        Self::new(
            IncrementalKind::Atr {
                period,
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn vwap() -> Self {
        Self::new(
            IncrementalKind::Vwap {
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn vwap_bands(reset: VwapReset, standard_deviation: f64, percent: f64) -> Self {
        Self::new(
            IncrementalKind::VwapBands {
                reset,
                standard_deviation,
                percent,
                state: RecursiveHistory::new(),
            },
            5,
        )
    }

    pub fn wma(period: usize) -> Self {
        Self::new(IncrementalKind::Wma { period }, 1)
    }

    pub fn output_count(&self) -> usize {
        self.output_count
    }

    pub fn output(&self, index: usize) -> &[f64] {
        self.outputs
            .get(index)
            .filter(|_| index < self.output_count)
            .map(Vec::as_slice)
            .expect("indicator output index")
    }

    pub fn output_from(&self, index: usize) -> usize {
        *self
            .output_from
            .get(index)
            .filter(|_| index < self.output_count)
            .expect("indicator output index")
    }

    pub fn take_output(&mut self, index: usize) -> Vec<f64> {
        assert!(index < self.output_count, "indicator output index");
        std::mem::take(&mut self.outputs[index])
    }

    /// Drop historical transfer capacity after the caller has copied a partial repair. Realtime
    /// batches retain up to 64K rows; larger suffix buffers must not become permanent state.
    pub fn release_transfer_capacity(&mut self) {
        const MAX_RETAINED_ROWS: usize = 65_536;
        for output in &mut self.outputs[..self.output_count] {
            output.clear();
            if output.capacity() > MAX_RETAINED_ROWS {
                output.shrink_to(MAX_RETAINED_ROWS);
            }
        }
    }

    pub fn runtime_bytes(&self) -> usize {
        match &self.kind {
            IncrementalKind::Ema { state, .. } => state.bytes(),
            IncrementalKind::EmaRibbon { states, .. } => {
                states.iter().map(RecursiveHistory::bytes).sum()
            }
            IncrementalKind::Rsi { state, .. } => state.bytes(),
            IncrementalKind::Macd { state, .. } => state.bytes(),
            IncrementalKind::Stochastic { state, tail_k, .. } => {
                state.bytes() + tail_k.capacity() * std::mem::size_of::<f64>()
            }
            IncrementalKind::Atr { state, .. } => state.bytes(),
            IncrementalKind::Vwap { state } => state.bytes(),
            IncrementalKind::VwapBands { state, .. } => state.bytes(),
            IncrementalKind::Sma { .. }
            | IncrementalKind::Bollinger { .. }
            | IncrementalKind::Wma { .. } => 0,
        }
    }

    pub fn transfer_capacity_bytes(&self) -> usize {
        self.outputs[..self.output_count]
            .iter()
            .map(|output| output.capacity() * std::mem::size_of::<f64>())
            .sum()
    }

    pub fn last_work_rows(&self) -> usize {
        self.last_work_rows
    }

    pub fn rebuild_from(&mut self, input: IndicatorInput<'_>, from: usize) {
        let n = input
            .close
            .len()
            .min(input.times.len())
            .min(input.high.len())
            .min(input.low.len());
        let requested = from.min(n);
        self.last_work_rows = 0;
        let starts = output_starts(&self.kind);
        for (index, &start) in starts.iter().enumerate().take(self.output_count) {
            self.output_from[index] = requested.max(start).min(n);
            self.outputs[index].clear();
            let rows = n - self.output_from[index];
            if self.outputs[index].capacity() < rows {
                self.outputs[index].reserve(rows);
            }
        }

        match &mut self.kind {
            IncrementalKind::Sma { period } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                for row in start..n {
                    let sum = input.close[row + 1 - *period..=row].iter().sum::<f64>();
                    self.outputs[0].push(sum / *period as f64);
                }
            }
            IncrementalKind::Ema { period, state } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let value = ema_step(&mut accumulator, input.close[row], *period);
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(value.expect("EMA after warmup"));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::EmaRibbon { periods, states } => {
                for (output_index, (&period, state)) in
                    periods.iter().zip(states.iter_mut()).enumerate()
                {
                    let (start, mut accumulator) = state.begin(n, requested);
                    self.last_work_rows = self.last_work_rows.saturating_add(n - start);
                    let mut tail = None;
                    let mut before_tail = None;
                    for row in start..n {
                        let previous = accumulator;
                        let value = ema_step(&mut accumulator, input.close[row], period);
                        state.checkpoint(row, accumulator);
                        if row >= self.output_from[output_index] {
                            self.outputs[output_index]
                                .push(value.expect("EMA ribbon output after warmup"));
                        }
                        if row + 1 == n {
                            tail = Some(accumulator);
                            before_tail = (row > 0).then_some(previous);
                        }
                    }
                    state.finish(n, tail, before_tail);
                }
            }
            IncrementalKind::Bollinger { period, deviation } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                let factor = deviation.max(0.0);
                for row in start..n {
                    let window = &input.close[row + 1 - *period..=row];
                    let mean = window.iter().sum::<f64>() / *period as f64;
                    let variance = window
                        .iter()
                        .map(|value| (value - mean).powi(2))
                        .sum::<f64>()
                        / *period as f64;
                    let spread = variance.sqrt() * factor;
                    self.outputs[0].push(mean + spread);
                    self.outputs[1].push(mean);
                    self.outputs[2].push(mean - spread);
                }
            }
            IncrementalKind::Rsi { period, state } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let value = if row == 0 {
                        None
                    } else {
                        rsi_change_step(
                            &mut accumulator,
                            input.close[row] - input.close[row - 1],
                            *period,
                            row,
                        )
                    };
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(value.expect("RSI after warmup"));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::Macd {
                fast_period,
                slow_period,
                signal_period,
                state,
            } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let point = macd_step(
                        &mut accumulator,
                        input.close[row],
                        *fast_period,
                        *slow_period,
                        *signal_period,
                    );
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(point.macd.expect("MACD line after warmup"));
                    }
                    if row >= self.output_from[1] {
                        self.outputs[1].push(point.signal.expect("MACD signal after warmup"));
                        self.outputs[2].push(point.histogram.expect("MACD histogram after warmup"));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::Stochastic {
                k_period,
                d_period,
                state,
                tail_k,
                source_len,
            } => {
                let realtime = requested >= source_len.saturating_sub(1) && n >= *source_len;
                let state_from = if realtime {
                    requested
                } else {
                    requested.saturating_sub(d_period.saturating_sub(1))
                };
                let (start, mut previous_k) = state.begin(n, state_from);
                self.last_work_rows = n - start;
                let mut recent = std::collections::VecDeque::with_capacity(*d_period);
                if realtime {
                    recent.extend(tail_k.iter().copied());
                    if n == *source_len && requested + 1 == n {
                        recent.pop_back();
                    }
                }
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = previous_k;
                    let k = if row + 1 < *k_period {
                        50.0
                    } else {
                        let high = input.high[row + 1 - *k_period..=row]
                            .iter()
                            .fold(f64::NEG_INFINITY, |acc, &value| acc.max(value));
                        let low = input.low[row + 1 - *k_period..=row]
                            .iter()
                            .fold(f64::INFINITY, |acc, &value| acc.min(value));
                        if high > low {
                            100.0 * (input.close[row] - low) / (high - low)
                        } else if row + 1 == *k_period {
                            50.0
                        } else {
                            previous_k
                        }
                    };
                    previous_k = k;
                    state.checkpoint(row, k);
                    if row + 1 >= *k_period {
                        recent.push_back(k);
                        if recent.len() > *d_period {
                            recent.pop_front();
                        }
                    }
                    if row >= self.output_from[0] {
                        self.outputs[0].push(k);
                    }
                    if row >= self.output_from[1] {
                        debug_assert_eq!(recent.len(), *d_period);
                        self.outputs[1].push(recent.iter().sum::<f64>() / *d_period as f64);
                    }
                    if row + 1 == n {
                        tail = Some(k);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
                tail_k.clear();
                tail_k.extend(recent);
                *source_len = n;
            }
            IncrementalKind::Atr { period, state } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let value = atr_step(
                        &mut accumulator,
                        AtrSample {
                            high: input.high[row],
                            low: input.low[row],
                            close: input.close[row],
                        },
                        *period,
                    );
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(value.expect("ATR after warmup"));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::Vwap { state } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let value = vwap_step(
                        &mut accumulator,
                        VwapSample {
                            time_unix_seconds: input.times[row],
                            high: input.high[row],
                            low: input.low[row],
                            close: input.close[row],
                            volume: input.volume.get(row).copied(),
                        },
                    );
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(value);
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::VwapBands {
                reset,
                standard_deviation,
                percent,
                state,
            } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let point = vwap_bands_step(
                        &mut accumulator,
                        VwapBandsSample {
                            time_unix_seconds: input.times[row],
                            high: input.high[row],
                            low: input.low[row],
                            close: input.close[row],
                            volume: input.volume.get(row).copied(),
                        },
                        *reset,
                        *standard_deviation,
                        *percent,
                    );
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(point.basis.expect("VWAP basis"));
                        self.outputs[1].push(point.standard_upper.expect("VWAP upper band"));
                        self.outputs[2].push(point.standard_lower.expect("VWAP lower band"));
                        self.outputs[3].push(point.percent_upper.expect("VWAP percent upper band"));
                        self.outputs[4].push(point.percent_lower.expect("VWAP percent lower band"));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::Wma { period } => {
                self.last_work_rows = n - self.output_from[0];
                let denominator = (*period * (*period + 1)) as f64 / 2.0;
                for row in self.output_from[0]..n {
                    let value = input.close[row + 1 - *period..=row]
                        .iter()
                        .enumerate()
                        .map(|(weight, value)| (weight + 1) as f64 * value)
                        .sum::<f64>()
                        / denominator;
                    self.outputs[0].push(value);
                }
            }
        }
    }
}

fn output_starts(kind: &IncrementalKind) -> [usize; MAX_OUTPUTS] {
    match kind {
        IncrementalKind::Sma { period }
        | IncrementalKind::Ema { period, .. }
        | IncrementalKind::Wma { period } => [period.saturating_sub(1), 0, 0, 0, 0],
        IncrementalKind::EmaRibbon { periods, .. } => {
            periods.map(|period| period.saturating_sub(1))
        }
        IncrementalKind::Bollinger { period, .. } => {
            let mut starts = [0; MAX_OUTPUTS];
            starts[..3].fill(period.saturating_sub(1));
            starts
        }
        IncrementalKind::Rsi { period, .. } | IncrementalKind::Atr { period, .. } => {
            [*period, 0, 0, 0, 0]
        }
        IncrementalKind::Macd {
            slow_period,
            signal_period,
            ..
        } => {
            let line = slow_period.saturating_sub(1);
            let signal = line.saturating_add(signal_period.saturating_sub(1));
            [line, signal, signal, 0, 0]
        }
        IncrementalKind::Stochastic {
            k_period, d_period, ..
        } => [
            k_period.saturating_sub(1),
            k_period.saturating_add(*d_period).saturating_sub(2),
            0,
            0,
            0,
        ],
        IncrementalKind::Vwap { .. } | IncrementalKind::VwapBands { .. } => [0; MAX_OUTPUTS],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sma_has_a_warmup_window() {
        assert_eq!(
            sma(&[1.0, 2.0, 3.0, 4.0], 3),
            vec![None, None, Some(2.0), Some(3.0)]
        );
    }

    #[test]
    fn ema_uses_sma_seed() {
        assert_eq!(
            ema(&[1.0, 2.0, 3.0, 5.0], 3),
            vec![None, None, Some(2.0), Some(3.5)]
        );
    }

    #[test]
    fn indexed_ema_resets_on_hard_gaps_and_requires_a_fresh_seed() {
        let samples = [
            Some(1.0),
            Some(2.0),
            Some(3.0),
            Some(4.0),
            None,
            Some(10.0),
            Some(20.0),
            Some(30.0),
            Some(40.0),
        ];
        let mut output = vec![None; samples.len()];
        let mut state = IncrementalEmaState::new(NonZeroUsize::new(3).expect("period"));

        state.rebuild_from_indexed(
            samples.len(),
            0,
            |index| samples[index],
            |index, value| output[index] = value,
        );

        assert_eq!(
            output,
            vec![
                None,
                None,
                Some(2.0),
                Some(3.0),
                None,
                None,
                None,
                Some(20.0),
                Some(30.0),
            ]
        );
    }

    #[test]
    fn indexed_ema_tail_updates_are_constant_work_and_historical_repairs_are_checkpointed() {
        let mut samples = (0..5_000)
            .map(|index| 100.0 + index as f64 * 0.01)
            .collect::<Vec<_>>();
        let mut output = vec![None; samples.len()];
        let mut state = IncrementalEmaState::new(NonZeroUsize::new(20).expect("period"));

        state.rebuild_from_indexed(
            samples.len(),
            0,
            |index| Some(samples[index]),
            |index, value| output[index] = value,
        );
        assert_eq!(output, ema(&samples, 20));
        assert!(state.runtime_bytes() < 4 * 1024);

        let last = samples.len() - 1;
        samples[last] += 3.0;
        state.rebuild_from_indexed(
            samples.len(),
            last,
            |index| Some(samples[index]),
            |index, value| output[index] = value,
        );
        assert_eq!(state.last_work_rows(), 1);
        assert_eq!(output, ema(&samples, 20));

        samples.push(222.0);
        output.push(None);
        let appended = samples.len() - 1;
        state.rebuild_from_indexed(
            samples.len(),
            appended,
            |index| Some(samples[index]),
            |index, value| output[index] = value,
        );
        assert_eq!(state.last_work_rows(), 1);
        assert_eq!(output, ema(&samples, 20));

        let repaired = 2_500;
        samples[repaired] -= 7.5;
        state.rebuild_from_indexed(
            samples.len(),
            repaired,
            |index| Some(samples[index]),
            |index, value| output[index] = value,
        );
        assert!(state.last_work_rows() >= samples.len() - repaired);
        assert!(state.last_work_rows() < samples.len() - repaired + CHECKPOINT_INTERVAL);
        assert_eq!(output, ema(&samples, 20));
    }

    #[test]
    fn indexed_ema_transaction_clone_shares_sparse_checkpoints_for_ordinary_tail_work() {
        let samples = (0..5_000)
            .map(|index| 100.0 + index as f64 * 0.01)
            .collect::<Vec<_>>();
        let mut state = IncrementalEmaState::new(NonZeroUsize::new(20).expect("period"));
        state.rebuild_from_indexed(samples.len(), 0, |index| Some(samples[index]), |_, _| {});

        let mut candidate = state.clone();
        assert!(Arc::ptr_eq(
            &state.history.checkpoints,
            &candidate.history.checkpoints
        ));
        let last = samples.len() - 1;
        candidate.rebuild_from_indexed(
            samples.len(),
            last,
            |index| Some(samples[index]),
            |_, _| {},
        );

        assert_eq!(candidate.last_work_rows(), 1);
        assert!(Arc::ptr_eq(
            &state.history.checkpoints,
            &candidate.history.checkpoints
        ));
    }

    #[test]
    fn indexed_atr_matches_dense_formula_resets_on_gaps_and_repairs_from_checkpoints() {
        let period = NonZeroUsize::new(14).expect("period");
        let mut samples = (0..5_000)
            .map(|index| {
                let close = 100.0 + index as f64 * 0.02;
                Some(AtrSample {
                    high: close + 1.0,
                    low: close - 0.75,
                    close,
                })
            })
            .collect::<Vec<_>>();
        let highs = samples
            .iter()
            .map(|sample| sample.expect("dense").high)
            .collect::<Vec<_>>();
        let lows = samples
            .iter()
            .map(|sample| sample.expect("dense").low)
            .collect::<Vec<_>>();
        let closes = samples
            .iter()
            .map(|sample| sample.expect("dense").close)
            .collect::<Vec<_>>();
        let expected = atr(&highs, &lows, &closes, period.get());
        let mut output = vec![None; samples.len()];
        let mut state = IncrementalAtrState::new(period);
        state.rebuild_from_indexed(
            samples.len(),
            0,
            |index| samples[index],
            |index, value| output[index] = value,
        );
        assert_eq!(output, expected);

        let tail = samples.len() - 1;
        let mut changed = samples[tail].expect("tail");
        changed.high += 2.0;
        samples[tail] = Some(changed);
        state.rebuild_from_indexed(samples.len(), tail, |index| samples[index], |_, _| {});
        assert_eq!(state.last_work_rows(), 1);

        let repaired = 2_500;
        let mut changed = samples[repaired].expect("repair");
        changed.low -= 3.0;
        samples[repaired] = Some(changed);
        state.rebuild_from_indexed(samples.len(), repaired, |index| samples[index], |_, _| {});
        assert!(state.last_work_rows() >= samples.len() - repaired);
        assert!(state.last_work_rows() < samples.len() - repaired + CHECKPOINT_INTERVAL);
        assert!(state.runtime_bytes() < samples.len() * std::mem::size_of::<AtrState>());

        let short = [
            Some(AtrSample {
                high: 2.0,
                low: 1.0,
                close: 1.5,
            }),
            Some(AtrSample {
                high: 3.0,
                low: 2.0,
                close: 2.5,
            }),
            Some(AtrSample {
                high: 4.0,
                low: 3.0,
                close: 3.5,
            }),
            None,
            Some(AtrSample {
                high: 11.0,
                low: 10.0,
                close: 10.5,
            }),
            Some(AtrSample {
                high: 12.0,
                low: 11.0,
                close: 11.5,
            }),
            Some(AtrSample {
                high: 13.0,
                low: 12.0,
                close: 12.5,
            }),
        ];
        let mut output = vec![None; short.len()];
        let mut state = IncrementalAtrState::new(NonZeroUsize::new(2).expect("period"));
        state.rebuild_from_indexed(
            short.len(),
            0,
            |index| short[index],
            |index, value| output[index] = value,
        );
        assert!(output[2].is_some());
        assert_eq!(output[3], None);
        assert_eq!(output[4], None);
        assert_eq!(output[5], None);
        assert!(output[6].is_some());
    }

    #[test]
    fn indexed_vwap_matches_dense_formula_tracks_sessions_and_keeps_tail_work_bounded() {
        let mut samples = (0..5_000)
            .map(|index| {
                let close = 100.0 + index as f64 * 0.01;
                Some(VwapSample {
                    time_unix_seconds: 1_700_000_000 + index as i64 * 60,
                    high: close + 0.5,
                    low: close - 0.5,
                    close,
                    volume: Some(10.0 + (index % 7) as f64),
                })
            })
            .collect::<Vec<_>>();
        let times = samples
            .iter()
            .map(|sample| sample.expect("dense").time_unix_seconds)
            .collect::<Vec<_>>();
        let highs = samples
            .iter()
            .map(|sample| sample.expect("dense").high)
            .collect::<Vec<_>>();
        let lows = samples
            .iter()
            .map(|sample| sample.expect("dense").low)
            .collect::<Vec<_>>();
        let closes = samples
            .iter()
            .map(|sample| sample.expect("dense").close)
            .collect::<Vec<_>>();
        let volumes = samples
            .iter()
            .map(|sample| sample.expect("dense").volume.expect("volume"))
            .collect::<Vec<_>>();
        let expected = vwap(&times, &highs, &lows, &closes, &volumes);
        let mut output = vec![None; samples.len()];
        let mut state = IncrementalVwapState::new();
        state.rebuild_from_indexed(
            samples.len(),
            0,
            |index| samples[index],
            |index, value| output[index] = value,
        );
        assert_eq!(output, expected);

        let tail = samples.len() - 1;
        let mut changed = samples[tail].expect("tail");
        changed.volume = Some(100.0);
        samples[tail] = Some(changed);
        state.rebuild_from_indexed(samples.len(), tail, |index| samples[index], |_, _| {});
        assert_eq!(state.last_work_rows(), 1);

        let repaired = 2_500;
        samples[repaired] = None;
        state.rebuild_from_indexed(samples.len(), repaired, |index| samples[index], |_, _| {});
        assert!(state.last_work_rows() >= samples.len() - repaired);
        assert!(state.last_work_rows() < samples.len() - repaired + CHECKPOINT_INTERVAL);
        assert!(state.runtime_bytes() < samples.len() * std::mem::size_of::<VwapState>());

        let short = [
            Some(VwapSample {
                time_unix_seconds: 86_400,
                high: 11.0,
                low: 9.0,
                close: 10.0,
                volume: Some(2.0),
            }),
            None,
            Some(VwapSample {
                time_unix_seconds: 86_460,
                high: 21.0,
                low: 19.0,
                close: 20.0,
                volume: Some(1.0),
            }),
            Some(VwapSample {
                time_unix_seconds: 172_800,
                high: 31.0,
                low: 29.0,
                close: 30.0,
                volume: None,
            }),
        ];
        let mut output = vec![None; short.len()];
        let mut state = IncrementalVwapState::new();
        state.rebuild_from_indexed(
            short.len(),
            0,
            |index| short[index],
            |index, value| output[index] = value,
        );
        assert_eq!(output, vec![Some(10.0), None, Some(20.0), Some(30.0)]);
    }

    #[test]
    fn indexed_rsi_matches_dense_formula_resets_on_gaps_and_repairs_from_checkpoints() {
        let period = NonZeroUsize::new(14).expect("period");
        let mut samples = (0..5_000)
            .map(|index| Some(100.0 + (index as f64 * 0.03).sin() * 4.0))
            .collect::<Vec<_>>();
        let dense = samples
            .iter()
            .map(|sample| sample.expect("dense"))
            .collect::<Vec<_>>();
        let expected = rsi(&dense, period.get());
        let mut output = vec![None; samples.len()];
        let mut state = IncrementalRsiState::new(period);
        state.rebuild_from_indexed(
            samples.len(),
            0,
            |index| samples[index],
            |index, value| output[index] = value,
        );
        assert_eq!(output, expected);

        let tail = samples.len() - 1;
        samples[tail] = Some(samples[tail].expect("tail") + 5.0);
        state.rebuild_from_indexed(
            samples.len(),
            tail,
            |index| samples[index],
            |index, value| output[index] = value,
        );
        assert_eq!(state.last_work_rows(), 1);
        let dense = samples
            .iter()
            .map(|sample| sample.expect("dense after tail repair"))
            .collect::<Vec<_>>();
        assert_eq!(output, rsi(&dense, period.get()));

        let repaired = 2_500;
        samples[repaired] = Some(samples[repaired].expect("repair") - 7.0);
        state.rebuild_from_indexed(
            samples.len(),
            repaired,
            |index| samples[index],
            |index, value| output[index] = value,
        );
        assert!(state.last_work_rows() >= samples.len() - repaired);
        assert!(state.last_work_rows() < samples.len() - repaired + CHECKPOINT_INTERVAL);
        let dense = samples
            .iter()
            .map(|sample| sample.expect("dense after historical repair"))
            .collect::<Vec<_>>();
        assert_eq!(output, rsi(&dense, period.get()));

        let short = [
            Some(1.0),
            Some(2.0),
            Some(3.0),
            None,
            Some(10.0),
            Some(11.0),
            Some(12.0),
        ];
        let mut output = vec![None; short.len()];
        let mut state = IncrementalRsiState::new(NonZeroUsize::new(2).expect("period"));
        state.rebuild_from_indexed(
            short.len(),
            0,
            |index| short[index],
            |index, value| output[index] = value,
        );
        assert_eq!(output[3], None);
        assert_eq!(output[4], None);
        assert_eq!(output[5], None);
        assert!(output[6].is_some());
    }

    #[test]
    fn indexed_macd_matches_dense_formula_resets_on_gaps_and_keeps_tail_work_constant() {
        let fast = NonZeroUsize::new(12).expect("fast");
        let slow = NonZeroUsize::new(26).expect("slow");
        let signal = NonZeroUsize::new(9).expect("signal");
        let mut samples = (0..5_000)
            .map(|index| Some(100.0 + (index as f64 * 0.02).sin() * 8.0))
            .collect::<Vec<_>>();
        let dense = samples
            .iter()
            .map(|sample| sample.expect("dense"))
            .collect::<Vec<_>>();
        let expected = macd(&dense, fast.get(), slow.get(), signal.get());
        let mut output = vec![
            MacdPoint {
                macd: None,
                signal: None,
                histogram: None,
            };
            samples.len()
        ];
        let mut state = IncrementalMacdState::new(fast, slow, signal);
        state.rebuild_from_indexed(
            samples.len(),
            0,
            |index| samples[index],
            |index, value| output[index] = value,
        );
        assert_eq!(output, expected);

        let tail = samples.len() - 1;
        samples[tail] = Some(samples[tail].expect("tail") + 5.0);
        state.rebuild_from_indexed(
            samples.len(),
            tail,
            |index| samples[index],
            |index, value| output[index] = value,
        );
        assert_eq!(state.last_work_rows(), 1);
        let dense = samples
            .iter()
            .map(|sample| sample.expect("dense after tail repair"))
            .collect::<Vec<_>>();
        assert_eq!(output, macd(&dense, fast.get(), slow.get(), signal.get()));

        let repaired = 2_500;
        samples[repaired] = Some(samples[repaired].expect("repair") - 7.0);
        state.rebuild_from_indexed(
            samples.len(),
            repaired,
            |index| samples[index],
            |index, value| output[index] = value,
        );
        assert!(state.last_work_rows() >= samples.len() - repaired);
        assert!(state.last_work_rows() < samples.len() - repaired + CHECKPOINT_INTERVAL);
        let dense = samples
            .iter()
            .map(|sample| sample.expect("dense after historical repair"))
            .collect::<Vec<_>>();
        assert_eq!(output, macd(&dense, fast.get(), slow.get(), signal.get()));

        let mut gap_output = Vec::new();
        let gap = [
            Some(1.0),
            Some(2.0),
            Some(3.0),
            None,
            Some(10.0),
            Some(11.0),
        ];
        let mut state = IncrementalMacdState::new(
            NonZeroUsize::new(2).expect("fast"),
            NonZeroUsize::new(3).expect("slow"),
            NonZeroUsize::new(2).expect("signal"),
        );
        state.rebuild_from_indexed(
            gap.len(),
            0,
            |index| gap[index],
            |_, point| gap_output.push(point),
        );
        assert_eq!(gap_output[3].macd, None);
        assert_eq!(gap_output[4].macd, None);
        assert_eq!(gap_output[5].macd, None);
    }

    #[test]
    fn indexed_stochastic_matches_dense_formula_resets_gaps_and_bounds_repairs() {
        let k_period = NonZeroUsize::new(14).expect("k");
        let d_period = NonZeroUsize::new(3).expect("d");
        let mut samples = (0..5_000)
            .map(|index| {
                let close = 100.0 + (index as f64 * 0.04).sin() * 5.0;
                Some(StochasticSample {
                    high: close + 1.0,
                    low: close - 1.0,
                    close,
                })
            })
            .collect::<Vec<_>>();
        let highs = samples
            .iter()
            .map(|sample| sample.expect("dense").high)
            .collect::<Vec<_>>();
        let lows = samples
            .iter()
            .map(|sample| sample.expect("dense").low)
            .collect::<Vec<_>>();
        let closes = samples
            .iter()
            .map(|sample| sample.expect("dense").close)
            .collect::<Vec<_>>();
        let expected = stochastic(&highs, &lows, &closes, k_period.get(), d_period.get());
        let mut output = vec![StochasticPoint { k: None, d: None }; samples.len()];
        let mut state = IncrementalStochasticState::new(k_period, d_period);
        state.rebuild_from_indexed(
            samples.len(),
            0,
            |index| samples[index],
            |index, value| output[index] = value,
        );
        assert_eq!(output, expected);

        let tail = samples.len() - 1;
        let mut changed = samples[tail].expect("tail");
        changed.close += 2.0;
        samples[tail] = Some(changed);
        state.rebuild_from_indexed(
            samples.len(),
            tail,
            |index| samples[index],
            |index, value| output[index] = value,
        );
        assert_eq!(state.last_work_rows(), 1);
        let highs = samples
            .iter()
            .map(|sample| sample.expect("dense tail").high)
            .collect::<Vec<_>>();
        let lows = samples
            .iter()
            .map(|sample| sample.expect("dense tail").low)
            .collect::<Vec<_>>();
        let closes = samples
            .iter()
            .map(|sample| sample.expect("dense tail").close)
            .collect::<Vec<_>>();
        assert_eq!(
            output,
            stochastic(&highs, &lows, &closes, k_period.get(), d_period.get())
        );

        let repaired = 2_500;
        samples[repaired] = None;
        state.rebuild_from_indexed(
            samples.len(),
            repaired,
            |index| samples[index],
            |index, value| output[index] = value,
        );
        assert!(state.last_work_rows() >= samples.len() - repaired);
        assert!(
            state.last_work_rows()
                < samples.len() - repaired + CHECKPOINT_INTERVAL + d_period.get()
        );
        let mut expected = vec![StochasticPoint { k: None, d: None }; samples.len()];
        let mut fresh = IncrementalStochasticState::new(k_period, d_period);
        fresh.rebuild_from_indexed(
            samples.len(),
            0,
            |index| samples[index],
            |index, value| expected[index] = value,
        );
        assert_eq!(output, expected);
    }

    #[test]
    fn indexed_stochastic_tail_gap_repair_restores_the_pre_gap_d_window() {
        let k_period = NonZeroUsize::new(2).expect("k");
        let d_period = NonZeroUsize::new(3).expect("d");
        let dense = (0..8)
            .map(|index| {
                let close = 10.0 + index as f64;
                StochasticSample {
                    high: close + 1.0,
                    low: close - 1.0,
                    close,
                }
            })
            .collect::<Vec<_>>();
        let mut samples = dense.iter().copied().map(Some).collect::<Vec<_>>();
        let tail = samples.len() - 1;
        samples[tail] = None;
        let mut output = vec![StochasticPoint { k: None, d: None }; samples.len()];
        let mut state = IncrementalStochasticState::new(k_period, d_period);
        state.rebuild_from_indexed(
            samples.len(),
            0,
            |index| samples[index],
            |index, value| output[index] = value,
        );
        assert_eq!(output[tail], StochasticPoint { k: None, d: None });

        samples[tail] = Some(dense[tail]);
        state.rebuild_from_indexed(
            samples.len(),
            tail,
            |index| samples[index],
            |index, value| output[index] = value,
        );
        assert_eq!(state.last_work_rows(), 1);

        let highs = dense.iter().map(|sample| sample.high).collect::<Vec<_>>();
        let lows = dense.iter().map(|sample| sample.low).collect::<Vec<_>>();
        let closes = dense.iter().map(|sample| sample.close).collect::<Vec<_>>();
        let expected = stochastic(&highs, &lows, &closes, k_period.get(), d_period.get());
        assert_eq!(output[tail], expected[tail]);
    }

    #[test]
    fn indexed_stochastic_d_period_does_not_preallocate_unbounded_memory() {
        let k_period = NonZeroUsize::new(2).expect("k");
        let d_period = NonZeroUsize::new(usize::MAX).expect("d");
        let samples = (0..4)
            .map(|index| {
                let close = 10.0 + index as f64;
                Some(StochasticSample {
                    high: close + 1.0,
                    low: close - 1.0,
                    close,
                })
            })
            .collect::<Vec<_>>();
        let mut state = IncrementalStochasticState::new(k_period, d_period);
        let mut output = vec![StochasticPoint { k: None, d: None }; samples.len()];
        state.rebuild_from_indexed(
            samples.len(),
            0,
            |index| samples[index],
            |index, point| output[index] = point,
        );
        assert!(output.iter().all(|point| point.d.is_none()));
        assert!(state.runtime_bytes() < 1_024);
    }

    #[test]
    fn bollinger_uses_population_deviation() {
        let b = bollinger(&[1.0, 2.0, 3.0], 3, 2.0);
        assert_eq!(b[1].middle, None);
        assert_eq!(b[2].middle, Some(2.0));
        assert!((b[2].upper.unwrap() - 3.632993161855452).abs() < 1e-12);
        assert!((b[2].lower.unwrap() - 0.367006838144548).abs() < 1e-12);
    }

    #[test]
    fn wma_weights_recent_bars_heaviest() {
        // window [1,2,3]: (1*1 + 2*2 + 3*3) / 6 = 14/6; window [2,3,4]: (2 + 6 + 12) / 6 = 20/6
        assert_eq!(
            wma(&[1.0, 2.0, 3.0, 4.0], 3),
            vec![None, None, Some(14.0 / 6.0), Some(20.0 / 6.0)]
        );
    }

    #[test]
    fn rsi_follows_wilder_smoothing() {
        // changes: +2, -1, +2, -1 → seed (period 2): gain 2/2 = 1.0, loss 1/2 = 0.5 → RS 2 → 66.67
        let r = rsi(&[1.0, 3.0, 2.0, 4.0, 3.0], 2);
        assert_eq!(r[0], None);
        assert_eq!(r[1], None);
        assert!((r[2].unwrap() - 200.0 / 3.0).abs() < 1e-9);
        // next change +2: gain (1.0 + 2) / 2 = 1.5, loss (0.5 + 0) / 2 = 0.25 → RS 6 → 85.714
        assert!((r[3].unwrap() - 600.0 / 7.0).abs() < 1e-9);
    }

    #[test]
    fn rsi_is_100_on_a_monotonic_rise_and_50_when_flat() {
        let up = rsi(&[1.0, 2.0, 3.0, 4.0, 5.0], 2);
        assert_eq!(up[2], Some(100.0));
        let flat = rsi(&[7.0, 7.0, 7.0, 7.0], 2);
        assert_eq!(flat[2], Some(50.0));
    }

    #[test]
    fn macd_lines_up_with_the_emas() {
        let values = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let points = macd(&values, 2, 3, 2);
        let fast = ema(&values, 2);
        let slow = ema(&values, 3);
        // First macd value where both emas exist (index 2); signal needs two macd values (index 3).
        assert_eq!(points[1].macd, None);
        assert!((points[2].macd.unwrap() - (fast[2].unwrap() - slow[2].unwrap())).abs() < 1e-12);
        assert_eq!(points[2].signal, None);
        let expected_signal = (points[2].macd.unwrap() + points[3].macd.unwrap()) / 2.0;
        assert!((points[3].signal.unwrap() - expected_signal).abs() < 1e-12);
        assert!(
            (points[3].histogram.unwrap() - (points[3].macd.unwrap() - expected_signal)).abs()
                < 1e-12
        );
    }

    #[test]
    fn stochastic_hits_the_extremes_and_carries_flat_windows() {
        let highs = [2.0, 3.0, 4.0, 4.0, 4.0];
        let lows = [1.0, 2.0, 3.0, 4.0, 4.0];
        let closes = [1.5, 2.0, 4.0, 4.0, 4.0];
        let s = stochastic(&highs, &lows, &closes, 2, 2);
        // i=1: C mid-window → 50; i=2: C at the window high → 100.
        assert_eq!(s[1].k, Some(50.0));
        assert_eq!(s[2].k, Some(100.0));
        // i=3: window high 4 / low 3, C at the high → 100; i=4: flat window carries it.
        assert_eq!(s[3].k, Some(100.0));
        assert_eq!(s[4].k, Some(100.0));
        // %D is the 2-SMA of %K once the trailing window is fully inside valid %K (i >= k+d-2).
        assert_eq!(s[1].d, None);
        assert_eq!(s[2].d, Some(75.0));
        assert_eq!(s[3].d, Some(100.0));
    }

    #[test]
    fn atr_of_a_constant_range_is_that_range() {
        // closes = highs keeps the previous close inside every bar, so TR = H - L = 1.
        let highs = [2.0, 3.0, 4.0, 5.0, 6.0];
        let lows = [1.0, 2.0, 3.0, 4.0, 5.0];
        let closes = [2.0, 3.0, 4.0, 5.0, 6.0];
        let a = atr(&highs, &lows, &closes, 2);
        assert_eq!(a[1], None);
        assert!((a[2].unwrap() - 1.0).abs() < 1e-12);
        assert!((a[4].unwrap() - 1.0).abs() < 1e-12);
        // A gap bar lifts TR through |H - prevC|.
        let gapped = atr(&[2.0, 10.0], &[1.0, 9.0], &[1.5, 9.5], 1);
        assert!((gapped[1].unwrap() - 8.5).abs() < 1e-12);
    }

    #[test]
    fn vwap_weights_by_volume_and_resets_each_utc_day() {
        // Day 0: tp 10 @ vol 1, tp 20 @ vol 3 → (10 + 60) / 4 = 17.5; day 1 restarts at tp 30.
        let times = [0, 3_600, 86_400];
        let highs = [10.0, 20.0, 30.0];
        let lows = [10.0, 20.0, 30.0];
        let closes = [10.0, 20.0, 30.0];
        let volumes = [1.0, 3.0, 5.0];
        let v = vwap(&times, &highs, &lows, &closes, &volumes);
        assert!((v[0].unwrap() - 10.0).abs() < 1e-12);
        assert!((v[1].unwrap() - 17.5).abs() < 1e-12);
        assert!((v[2].unwrap() - 30.0).abs() < 1e-12);
        // Empty volume slice = unit weights (cumulative typical mean).
        let u = vwap(&times[..2], &highs[..2], &lows[..2], &closes[..2], &[]);
        assert!((u[1].unwrap() - 15.0).abs() < 1e-12);
    }

    #[test]
    fn vwap_bands_reset_by_month_and_match_weighted_reference() {
        let points = vwap_bands(
            &[1_704_067_200, 1_704_153_600, 1_706_745_600], // 2024-01-01, Jan-02, Feb-01
            &[10.0, 14.0, 20.0],
            &[10.0, 14.0, 20.0],
            &[10.0, 14.0, 20.0],
            &[1.0, 3.0, 2.0],
            VwapBandsOptions {
                reset: VwapReset::Monthly,
                standard_deviation: 1.0,
                percent: 10.0,
            },
        );
        assert_eq!(points[0].basis, Some(10.0));
        assert_eq!(points[1].basis, Some(13.0));
        assert_eq!(points[2].basis, Some(20.0));
        assert!((points[1].standard_upper.unwrap() - (13.0 + 3.0_f64.sqrt())).abs() < 1e-12);
        assert!((points[1].standard_lower.unwrap() - (13.0 - 3.0_f64.sqrt())).abs() < 1e-12);
        assert_eq!(points[2].percent_upper, Some(22.0));
        assert_eq!(points[2].percent_lower, Some(18.0));
    }

    #[derive(Clone, Copy)]
    enum TestKind {
        Sma,
        Ema,
        EmaRibbon,
        Bollinger,
        Rsi,
        Macd,
        Stochastic,
        Atr,
        Vwap,
        Wma,
    }

    fn expected(kind: TestKind, input: IndicatorInput<'_>) -> Vec<Vec<Option<f64>>> {
        match kind {
            TestKind::Sma => vec![sma(input.close, 5)],
            TestKind::Ema => vec![ema(input.close, 5)],
            TestKind::EmaRibbon => [3, 5, 8, 13, 21]
                .into_iter()
                .map(|period| ema(input.close, period))
                .collect(),
            TestKind::Bollinger => {
                let points = bollinger(input.close, 5, 2.0);
                vec![
                    points.iter().map(|point| point.upper).collect(),
                    points.iter().map(|point| point.middle).collect(),
                    points.iter().map(|point| point.lower).collect(),
                ]
            }
            TestKind::Rsi => vec![rsi(input.close, 5)],
            TestKind::Macd => {
                let points = macd(input.close, 3, 6, 4);
                vec![
                    points.iter().map(|point| point.macd).collect(),
                    points.iter().map(|point| point.signal).collect(),
                    points.iter().map(|point| point.histogram).collect(),
                ]
            }
            TestKind::Stochastic => {
                let points = stochastic(input.high, input.low, input.close, 5, 3);
                vec![
                    points.iter().map(|point| point.k).collect(),
                    points.iter().map(|point| point.d).collect(),
                ]
            }
            TestKind::Atr => vec![atr(input.high, input.low, input.close, 5)],
            TestKind::Vwap => vec![vwap(
                input.times,
                input.high,
                input.low,
                input.close,
                input.volume,
            )],
            TestKind::Wma => vec![wma(input.close, 5)],
        }
    }

    fn assert_incremental_matches_full(
        states: &mut [(TestKind, IncrementalState)],
        input: IndicatorInput<'_>,
        from: usize,
    ) {
        for (kind, state) in states {
            state.rebuild_from(input, from);
            let expected = expected(*kind, input);
            assert_eq!(state.output_count(), expected.len());
            for (output, expected) in expected.iter().enumerate() {
                let output_from = state.output_from(output);
                assert_eq!(state.output(output).len(), expected.len() - output_from);
                for (offset, (&actual, &expected)) in state
                    .output(output)
                    .iter()
                    .zip(&expected[output_from..])
                    .enumerate()
                {
                    let index = output_from + offset;
                    let expected = expected.expect("output begins after warmup");
                    assert!(
                        (actual - expected).abs() < 1e-10,
                        "output {output} row {index}: {actual} != {expected}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_runtime_mutation_matches_fresh_full_recomputation() {
        let mut states = vec![
            (TestKind::Sma, IncrementalState::sma(5)),
            (TestKind::Ema, IncrementalState::ema(5)),
            (
                TestKind::EmaRibbon,
                IncrementalState::ema_ribbon([3, 5, 8, 13, 21]),
            ),
            (TestKind::Bollinger, IncrementalState::bollinger(5, 2.0)),
            (TestKind::Rsi, IncrementalState::rsi(5)),
            (TestKind::Macd, IncrementalState::macd(3, 6, 4)),
            (TestKind::Stochastic, IncrementalState::stochastic(5, 3)),
            (TestKind::Atr, IncrementalState::atr(5)),
            (TestKind::Vwap, IncrementalState::vwap()),
            (TestKind::Wma, IncrementalState::wma(5)),
        ];
        let mut times = (0..40).map(|index| index * 3_600).collect::<Vec<_>>();
        let mut close = (0..40)
            .map(|index| 100.0 + (index as f64 * 0.37).sin() * 8.0 + index as f64 * 0.1)
            .collect::<Vec<_>>();
        let mut high = close.iter().map(|value| value + 1.5).collect::<Vec<_>>();
        let mut low = close.iter().map(|value| value - 1.25).collect::<Vec<_>>();
        let mut volume = (0..40).map(|index| (index % 7) as f64).collect::<Vec<_>>();

        let check = |states: &mut [(TestKind, IncrementalState)],
                     times: &[i64],
                     high: &[f64],
                     low: &[f64],
                     close: &[f64],
                     volume: &[f64],
                     from| {
            assert_incremental_matches_full(
                states,
                IndicatorInput {
                    times,
                    open: close,
                    high,
                    low,
                    close,
                    volume,
                },
                from,
            );
        };

        check(&mut states, &times, &high, &low, &close, &volume, 0);

        times.push(40 * 3_600);
        close.push(108.0);
        high.push(109.0);
        low.push(106.0);
        volume.push(0.0);
        check(&mut states, &times, &high, &low, &close, &volume, 40);

        for index in 41..50 {
            times.push(index * 3_600);
            close.push(100.0 + index as f64 * 0.2);
            high.push(close[index as usize] + 1.0);
            low.push(close[index as usize] - 1.0);
            volume.push((index % 5) as f64);
        }
        check(&mut states, &times, &high, &low, &close, &volume, 41);

        for replacement in 0..1_000 {
            let last = close.len() - 1;
            close[last] = 111.0 + replacement as f64 * 0.001;
            high[last] = close[last] + 2.0;
            low[last] = close[last] - 2.0;
            volume[last] = (replacement % 11) as f64;
            check(&mut states, &times, &high, &low, &close, &volume, last);
        }

        close[17] += 4.0;
        high[17] = close[17] + 1.0;
        low[17] = close[17] - 1.0;
        check(&mut states, &times, &high, &low, &close, &volume, 17);

        let last_ten = close.len() - 10;
        close[last_ten] -= 2.5;
        high[last_ten] = close[last_ten] + 1.0;
        low[last_ten] = close[last_ten] - 1.0;
        check(&mut states, &times, &high, &low, &close, &volume, last_ten);

        close[2] += 1.75;
        high[2] = close[2] + 1.0;
        low[2] = close[2] - 1.0;
        check(&mut states, &times, &high, &low, &close, &volume, 2);

        times.insert(9, times[8] + 1_800);
        close.insert(9, 93.0);
        high.insert(9, 95.0);
        low.insert(9, 91.0);
        volume.insert(9, 3.0);
        check(&mut states, &times, &high, &low, &close, &volume, 9);

        times.truncate(23);
        close.truncate(23);
        high.truncate(23);
        low.truncate(23);
        volume.truncate(23);
        check(&mut states, &times, &high, &low, &close, &volume, 23);

        times = (0..31).map(|index| 86_400 + index * 1_800).collect();
        close = (0..31).map(|index| 80.0 + index as f64 * 0.75).collect();
        high = close.iter().map(|value| value + 3.0).collect();
        low = close.iter().map(|value| value - 2.0).collect();
        volume = (0..31).map(|index| (index % 4 + 1) as f64).collect();
        check(&mut states, &times, &high, &low, &close, &volume, 0);
    }

    #[test]
    fn million_row_rsi_runtime_is_sparse_and_tail_updates_are_constant_work() {
        let rows = 1_000_000;
        let times = (0..rows).map(|row| row as i64 * 60).collect::<Vec<_>>();
        let close = (0..rows)
            .map(|row| 100.0 + (row as f64 * 0.01).sin())
            .collect::<Vec<_>>();
        let high = close.iter().map(|value| value + 1.0).collect::<Vec<_>>();
        let low = close.iter().map(|value| value - 1.0).collect::<Vec<_>>();
        let input = IndicatorInput {
            times: &times,
            open: &close,
            high: &high,
            low: &low,
            close: &close,
            volume: &[],
        };
        let mut state = IncrementalState::rsi(14);

        state.rebuild_from(input, 0);
        let canonical_output = state.take_output(0);
        assert_eq!(canonical_output.len(), rows - 14);
        state.release_transfer_capacity();
        assert!(state.runtime_bytes() < 32 * 1024);
        assert_eq!(state.transfer_capacity_bytes(), 0);

        state.rebuild_from(input, rows - 1);
        assert_eq!(state.last_work_rows(), 1);
        state.release_transfer_capacity();

        state.rebuild_from(input, rows / 2);
        assert!(state.last_work_rows() >= rows / 2);
        assert!(state.last_work_rows() < rows / 2 + CHECKPOINT_INTERVAL);
    }
}
