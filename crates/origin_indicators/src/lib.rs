//! Pure, allocation-contained technical indicators.
//!
//! The indicator layer deliberately knows nothing about charts, panes, WebAssembly, or
//! rendering. It consumes a close/value slice and returns a derived value column that the
//! headless engine can install as an ordinary series. `None` represents the warm-up window.

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
}
