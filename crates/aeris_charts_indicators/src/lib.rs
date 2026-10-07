//! Pure, allocation-contained technical indicators.
//!
//! The indicator layer deliberately knows nothing about charts, panes, WebAssembly, or
//! rendering. It consumes a close/value slice and returns a derived value column that the
//! headless engine can install as an ordinary series. `None` represents the warm-up window.

pub mod session_studies;
pub mod structure_studies;
pub mod study_annotations;
pub mod volume_profile;

pub use session_studies::{
    PreviousPeriod, SessionSource, SessionStudy, SessionStudyPoint, SessionStudyState,
    session_study,
};
pub use structure_studies::{
    BreakOn, MAX_ORDER_BLOCK_SEARCH_ROWS, Mitigation, MitigationPrice, OrderBlockZone,
    StructureStudy, StructureStudyKind, StructureStudyResult, structure_study,
};
pub use study_annotations::{
    SessionSpan, StudyAnnotations, StudyMarker, StudyMarkerKind, StudyZone,
};

use std::{collections::VecDeque, num::NonZeroUsize, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BollingerPoint {
    pub middle: Option<f64>,
    pub upper: Option<f64>,
    pub lower: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DonchianPoint {
    pub upper: Option<f64>,
    pub middle: Option<f64>,
    pub lower: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeltnerPoint {
    pub upper: Option<f64>,
    pub middle: Option<f64>,
    pub lower: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdxDmiPoint {
    pub plus_di: Option<f64>,
    pub minus_di: Option<f64>,
    pub adx: Option<f64>,
}

/// Pivot-point formula families supported by the built-in daily level study.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PivotKind {
    #[default]
    Standard,
    Fibonacci,
    Camarilla,
    Woodie,
    DeMark,
}

/// Previous-session pivot levels aligned to the first row of the next UTC day.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PivotPoint {
    pub pivot: Option<f64>,
    pub resistance_1: Option<f64>,
    pub support_1: Option<f64>,
    pub resistance_2: Option<f64>,
    pub support_2: Option<f64>,
}

/// Compute daily pivot levels without looking ahead into the current UTC session.
///
/// The first session has no prior completed range and therefore emits `None`. Every later
/// session receives the immediately preceding session's OHLC-derived levels at its first row;
/// values remain constant until the next UTC day. The five outputs are pivot, R1, S1, R2, S2.
pub fn pivot_points(
    times: &[i64],
    opens: &[f64],
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    kind: PivotKind,
) -> Vec<PivotPoint> {
    let n = times
        .len()
        .min(opens.len())
        .min(highs.len())
        .min(lows.len())
        .min(closes.len());
    let mut out = vec![PivotPoint::default(); n];
    if n == 0 {
        return out;
    }

    let mut current_day = None;
    let mut current: Option<Session> = None;
    let mut previous = None;
    for row in 0..n {
        let day = times[row].div_euclid(86_400);
        if current_day != Some(day) {
            if let Some(session) = current.take() {
                previous = Some(session);
            }
            current_day = Some(day);
        }
        if valid_bar(highs[row], lows[row], closes[row]) && opens[row].is_finite() {
            if let Some(session) = current.as_mut() {
                session.high = session.high.max(highs[row]);
                session.low = session.low.min(lows[row]);
                session.close = closes[row];
            } else {
                current = Some(Session {
                    open: opens[row],
                    high: highs[row],
                    low: lows[row],
                    close: closes[row],
                });
            }
            if let Some(session) = previous {
                out[row] = pivot_levels(session, kind);
            }
        }
    }
    out
}

/// Compute confirmed ZigZag turning points from high/low bars.
///
/// `deviation_percent` is the minimum percentage move required to confirm a reversal. The
/// current extreme is emitted as a provisional endpoint, while earlier extrema are only emitted
/// once the opposing move has crossed the threshold.
pub fn zigzag(highs: &[f64], lows: &[f64], deviation_percent: f64) -> Vec<Option<f64>> {
    let n = highs.len().min(lows.len());
    let mut out = vec![None; n];
    if n == 0 || !deviation_percent.is_finite() || deviation_percent <= 0.0 {
        return out;
    }
    let threshold = deviation_percent / 100.0;
    let mut direction = 0_i8;
    let mut extreme_index = 0_usize;
    let mut extreme = (highs[0] + lows[0]) / 2.0;
    for index in 1..n {
        if direction == 0 {
            if high_at_least(highs[index], lows[0], threshold) {
                direction = 1;
                extreme_index = index;
                extreme = highs[index];
                out[0] = Some(lows[0]);
            } else if low_at_most(lows[index], highs[0], threshold) {
                direction = -1;
                extreme_index = index;
                extreme = lows[index];
                out[0] = Some(highs[0]);
            } else if highs[index] > extreme {
                extreme = highs[index];
                extreme_index = index;
            } else if lows[index] < extreme {
                extreme = lows[index];
                extreme_index = index;
            }
            continue;
        }
        if direction > 0 {
            if highs[index] >= extreme {
                extreme = highs[index];
                extreme_index = index;
            } else if low_at_most(lows[index], extreme, threshold) {
                out[extreme_index] = Some(extreme);
                direction = -1;
                extreme_index = index;
                extreme = lows[index];
            }
        } else if lows[index] <= extreme {
            extreme = lows[index];
            extreme_index = index;
        } else if high_at_least(highs[index], extreme, threshold) {
            out[extreme_index] = Some(extreme);
            direction = 1;
            extreme_index = index;
            extreme = highs[index];
        }
    }
    if direction != 0 {
        out[extreme_index] = Some(extreme);
    }
    out
}

fn high_at_least(high: f64, reference: f64, threshold: f64) -> bool {
    high >= reference * (1.0 + threshold)
}

fn low_at_most(low: f64, reference: f64, threshold: f64) -> bool {
    low <= reference * (1.0 - threshold)
}

#[derive(Clone, Copy)]
struct Session {
    open: f64,
    high: f64,
    low: f64,
    close: f64,
}

fn pivot_levels(session: Session, kind: PivotKind) -> PivotPoint {
    let range = session.high - session.low;
    let (pivot, r1, s1, r2, s2) = match kind {
        PivotKind::Standard => {
            let pivot = (session.high + session.low + session.close) / 3.0;
            (
                pivot,
                2.0 * pivot - session.low,
                2.0 * pivot - session.high,
                pivot + range,
                pivot - range,
            )
        }
        PivotKind::Fibonacci => {
            let pivot = (session.high + session.low + session.close) / 3.0;
            (
                pivot,
                pivot + range * 0.382,
                pivot - range * 0.382,
                pivot + range * 0.618,
                pivot - range * 0.618,
            )
        }
        PivotKind::Camarilla => (
            session.close,
            session.close + range * 1.1 / 12.0,
            session.close - range * 1.1 / 12.0,
            session.close + range * 1.1 / 6.0,
            session.close - range * 1.1 / 6.0,
        ),
        PivotKind::Woodie => {
            let pivot = (session.high + session.low + 2.0 * session.close) / 4.0;
            (
                pivot,
                2.0 * pivot - session.low,
                2.0 * pivot - session.high,
                pivot + range,
                pivot - range,
            )
        }
        PivotKind::DeMark => {
            let weighted = if session.close < session.open {
                session.high + 2.0 * session.low + session.close
            } else if session.close > session.open {
                2.0 * session.high + session.low + session.close
            } else {
                session.high + session.low + 2.0 * session.close
            };
            let pivot = weighted / 4.0;
            (
                pivot,
                weighted / 2.0 - session.low,
                weighted / 2.0 - session.high,
                pivot + range,
                pivot - range,
            )
        }
    };
    PivotPoint {
        pivot: Some(pivot),
        resistance_1: Some(r1),
        support_1: Some(s1),
        resistance_2: Some(r2),
        support_2: Some(s2),
    }
}

/// Parabolic SAR with the conventional 0.02 acceleration step and 0.20 cap.
pub fn parabolic_sar(highs: &[f64], lows: &[f64]) -> Vec<Option<f64>> {
    let n = highs.len().min(lows.len());
    let mut out = vec![None; n];
    let mut state = ParabolicSarState::default();
    for row in 0..n {
        if valid_range(highs[row], lows[row]) {
            out[row] = Some(parabolic_sar_step(
                &mut state, highs[row], lows[row], 0.02, 0.20,
            ));
        }
    }
    out
}

/// SuperTrend line using Wilder ATR and a midpoint-based volatility multiplier.
pub fn supertrend(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    period: usize,
    multiplier: f64,
) -> Vec<Option<f64>> {
    let n = highs.len().min(lows.len()).min(closes.len());
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    let mut state = SuperTrendState::default();
    for row in 0..n {
        if !valid_bar(highs[row], lows[row], closes[row]) {
            continue;
        }
        out[row] = supertrend_step(
            &mut state,
            DirectionalSample {
                high: highs[row],
                low: lows[row],
                close: closes[row],
            },
            period,
            multiplier,
        );
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IchimokuPoint {
    pub conversion: Option<f64>,
    pub base: Option<f64>,
    pub leading_a: Option<f64>,
    pub leading_b: Option<f64>,
    pub lagging: Option<f64>,
}

/// Ichimoku cloud with the conventional 9/26/52 periods. Values are aligned to the source row;
/// hosts that need visual displacement can apply it without changing canonical study data.
pub fn ichimoku(highs: &[f64], lows: &[f64], closes: &[f64]) -> Vec<IchimokuPoint> {
    let n = highs.len().min(lows.len()).min(closes.len());
    let mut out = vec![
        IchimokuPoint {
            conversion: None,
            base: None,
            leading_a: None,
            leading_b: None,
            lagging: None,
        };
        n
    ];
    for (row, output) in out.iter_mut().enumerate() {
        let conversion = rolling_midpoint(highs, lows, row, 9);
        let base = rolling_midpoint(highs, lows, row, 26);
        let leading_b = rolling_midpoint(highs, lows, row, 52);
        let leading_a = conversion
            .zip(base)
            .map(|(conversion, base)| (conversion + base) * 0.5);
        *output = IchimokuPoint {
            conversion,
            base,
            leading_a,
            leading_b,
            lagging: closes.get(row).copied(),
        };
    }
    out
}

/// Simple moving average. The first `period - 1` values are warm-up `None` entries.
/// Aroon uses the most recent extremum in the current bar plus `period` preceding bars.
/// Equal extrema prefer the newest occurrence, so a repeated high or low resets to 100.
pub fn aroon(high: &[f64], low: &[f64], period: usize) -> Vec<(Option<f64>, Option<f64>)> {
    let n = high.len().min(low.len());
    let mut out = vec![(None, None); n];
    if period == 0 {
        return out;
    }
    for (row, output) in out.iter_mut().enumerate().skip(period) {
        let start = row - period;
        if !(start..=row).all(|index| high[index].is_finite() && low[index].is_finite()) {
            continue;
        }
        let mut high_index = start;
        let mut low_index = start;
        for index in start + 1..=row {
            if high[index] >= high[high_index] {
                high_index = index;
            }
            if low[index] <= low[low_index] {
                low_index = index;
            }
        }
        *output = (
            Some((period - (row - high_index)) as f64 * 100.0 / period as f64),
            Some((period - (row - low_index)) as f64 * 100.0 / period as f64),
        );
    }
    out
}

/// Bill Williams' Awesome Oscillator: SMA(5, HL2) minus SMA(34, HL2).
pub fn awesome_oscillator(high: &[f64], low: &[f64]) -> Vec<Option<f64>> {
    let n = high.len().min(low.len());
    let mut out = vec![None; n];
    for (row, output) in out.iter_mut().enumerate().skip(33) {
        if !(row - 33..=row).all(|index| high[index].is_finite() && low[index].is_finite()) {
            continue;
        }
        let short = (row - 4..=row)
            .map(|index| (high[index] + low[index]) / 2.0)
            .sum::<f64>()
            / 5.0;
        let long = (row - 33..=row)
            .map(|index| (high[index] + low[index]) / 2.0)
            .sum::<f64>()
            / 34.0;
        *output = Some(short - long);
    }
    out
}

/// Uncentered DPO: price from `period / 2 + 1` bars ago minus today's period SMA.
pub fn dpo(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 {
        return out;
    }
    let lag = period / 2 + 1;
    for (row, output) in out
        .iter_mut()
        .enumerate()
        .skip(period.saturating_sub(1).max(lag))
    {
        let window = &values[row + 1 - period..=row];
        if window.iter().all(|value| value.is_finite()) {
            *output = Some(values[row - lag] - window.iter().sum::<f64>() / period as f64);
        }
    }
    out
}

/// Chande Momentum Oscillator: signed close movement divided by total movement.
pub fn chande_momentum(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 {
        return out;
    }
    for row in period..values.len() {
        let mut signed = 0.0;
        let mut absolute = 0.0;
        let mut valid = true;
        for pair in values[row - period..=row].windows(2) {
            if !pair[0].is_finite() || !pair[1].is_finite() {
                valid = false;
                break;
            }
            let delta = pair[1] - pair[0];
            signed += delta;
            absolute += delta.abs();
        }
        if valid {
            out[row] = Some(if absolute == 0.0 {
                0.0
            } else {
                100.0 * signed / absolute
            });
        }
    }
    out
}

#[cfg(test)]
mod breadth_reference_tests {
    use super::*;

    #[test]
    fn aroon_uses_period_plus_current_and_prefers_latest_equal_extreme() {
        let high = [1.0, 4.0, 3.0, 4.0, 2.0];
        let low = [5.0, 4.0, 3.0, 3.0, 6.0];
        let out = aroon(&high, &low, 3);
        assert_eq!(out[..3], [(None, None); 3]);
        assert_eq!(out[3], (Some(100.0), Some(100.0)));
        assert_eq!(out[4], (Some(200.0 / 3.0), Some(200.0 / 3.0)));
    }

    #[test]
    fn awesome_oscillator_uses_median_price_and_34_bar_warmup() {
        let high = (0..34).map(|row| row as f64 + 1.0).collect::<Vec<_>>();
        let low = (0..34).map(|row| row as f64 - 1.0).collect::<Vec<_>>();
        let out = awesome_oscillator(&high, &low);
        assert!(out[..33].iter().all(Option::is_none));
        assert_eq!(out[33], Some(14.5));
    }

    #[test]
    fn dpo_uses_lagged_price_against_current_sma() {
        let values = (0..10).map(f64::from).collect::<Vec<_>>();
        let out = dpo(&values, 5);
        assert_eq!(out[..4], [None; 4]);
        assert_eq!(out[4], Some(-1.0));
        assert_eq!(out[9], Some(-1.0));
    }

    #[test]
    fn chande_momentum_uses_signed_over_absolute_movement() {
        let out = chande_momentum(&[10.0, 12.0, 11.0, 13.0, 13.0], 3);
        assert_eq!(out[..3], [None; 3]);
        assert_eq!(out[3], Some(60.0));
        assert_eq!(out[4], Some(100.0 / 3.0));
        assert_eq!(chande_momentum(&[3.0, 3.0, 3.0], 2)[2], Some(0.0));
    }

    #[test]
    fn bollinger_metrics_measure_band_position_and_relative_width() {
        let out = bollinger_metrics(&[10.0, 11.0, 12.0], 3, 1.0);
        assert_eq!(out[..2], [(None, None); 2]);
        let spread = (2.0_f64 / 3.0).sqrt();
        assert!((out[2].0.unwrap() - (1.0 + spread) / (2.0 * spread)).abs() < 1e-12);
        assert!((out[2].1.unwrap() - 2.0 * spread / 11.0 * 100.0).abs() < 1e-12);
    }

    #[test]
    fn envelopes_place_percent_bands_around_selected_average() {
        let simple = envelopes(&[10.0, 12.0, 14.0], 2, 10.0, false);
        assert_eq!(simple[0], (None, None, None));
        assert!((simple[1].0.unwrap() - 12.1).abs() < 1e-12);
        assert_eq!(simple[1].1, Some(11.0));
        assert!((simple[1].2.unwrap() - 9.9).abs() < 1e-12);
        assert!((simple[2].1.unwrap() - 13.0).abs() < 1e-12);

        let exponential = envelopes(&[10.0, 12.0, 14.0], 2, 10.0, true);
        assert_eq!(exponential[0], (None, None, None));
        assert!((exponential[2].1.unwrap() - 13.0).abs() < 1e-12);
    }

    #[test]
    fn alma_uses_normalized_shifted_gaussian_weights() {
        let symmetric = alma(&[1.0, 2.0, 3.0], 3, 0.5, 6.0);
        assert_eq!(symmetric[..2], [None; 2]);
        assert!((symmetric[2].unwrap() - 2.0).abs() < 1e-12);
        let recent = alma(&[1.0, 2.0, 3.0], 3, 1.0, 6.0);
        let first = (-8.0_f64).exp();
        let second = (-2.0_f64).exp();
        let expected = (first + 2.0 * second + 3.0) / (first + second + 1.0);
        assert!((recent[2].unwrap() - expected).abs() < 1e-12);
    }

    #[test]
    fn cumulative_volume_studies_use_their_distinct_price_weights() {
        let highs = [12.0, 14.0, 15.0, 18.0];
        let lows = [8.0, 10.0, 15.0, 14.0];
        let closes = [11.0, 11.0, 15.0, 14.0];
        let volumes = [10.0, 20.0, 30.0, 40.0];
        assert_eq!(
            accumulation_distribution(&highs, &lows, &closes, &volumes),
            vec![Some(5.0), Some(-5.0), Some(-5.0), Some(-45.0)]
        );
        let pvt = price_volume_trend(&[10.0, 12.0, 9.0], &[5.0, 10.0, 8.0]);
        assert_eq!(pvt, vec![Some(0.0), Some(2.0), Some(0.0)]);
        assert_eq!(
            price_volume_trend(&[0.0, 1.0], &[1.0, 5.0]),
            vec![Some(0.0), Some(0.0)]
        );
    }

    #[test]
    fn chaikin_oscillator_subtracts_emas_of_cumulative_money_flow() {
        let out = chaikin_oscillator(&[2.0; 4], &[0.0; 4], &[2.0, 2.0, 0.0, 0.0], &[1.0; 4], 2, 3);
        assert_eq!(out[..2], [None; 2]);
        assert!((out[2].unwrap() + 1.0 / 6.0).abs() < 1e-12);
        assert!((out[3].unwrap() + 5.0 / 18.0).abs() < 1e-12);
    }

    #[test]
    fn relative_volume_excludes_current_bar_from_baseline() {
        assert_eq!(
            relative_volume(&[10.0, 20.0, 30.0, 40.0], 2),
            vec![None, None, Some(2.0), Some(1.6)]
        );
        let zero_baseline = relative_volume(&[0.0, 0.0, 5.0], 2);
        assert_eq!(zero_baseline[..2], [None; 2]);
        assert!(zero_baseline[2].unwrap().is_nan());
    }

    #[test]
    fn volume_oscillator_normalizes_fast_slow_ema_gap_and_signals_it() {
        let out = volume_oscillator(&[1.0, 2.0, 3.0, 4.0], 2, 3, 2);
        assert!(out[..2].iter().all(|point| point.line.is_none()));
        assert_eq!(out[2].line, Some(25.0));
        assert_eq!(out[2].signal, None);
        assert!((out[3].line.unwrap() - 100.0 / 6.0).abs() < 1e-12);
        assert!((out[3].signal.unwrap() - 125.0 / 6.0).abs() < 1e-12);
        assert!((out[3].histogram.unwrap() + 25.0 / 6.0).abs() < 1e-12);
    }

    #[test]
    fn elder_force_smooths_close_change_weighted_by_volume() {
        let out = elder_force(&[10.0, 12.0, 11.0, 14.0], &[5.0, 10.0, 20.0, 10.0], 2);
        assert_eq!(out, vec![None, None, Some(0.0), Some(20.0)]);
    }

    #[test]
    fn ease_of_movement_smooths_midpoint_distance_over_box_ratio() {
        let out = ease_of_movement(
            &[12.0, 14.0, 16.0, 17.0],
            &[8.0, 10.0, 12.0, 13.0],
            &[10.0, 20.0, 10.0, 0.0],
            2,
            100.0,
        );
        assert_eq!(out[0], None);
        assert_eq!(out[1], None);
        assert_eq!(out[2], Some(60.0));
        assert!(out[3].unwrap().is_nan());
    }

    #[test]
    fn historical_volatility_uses_sample_log_return_deviation() {
        let out = historical_volatility(&[1.0, 2.0, 4.0, 16.0], 2, 4.0);
        assert_eq!(out[..2], [None, None]);
        assert!(out[2].unwrap().abs() < 1e-10);
        assert!((out[3].unwrap() - 200.0 * 2.0_f64.ln() / 2.0_f64.sqrt()).abs() < 1e-10);
        let gaps = historical_volatility(&[1.0, 0.0, 4.0, 16.0], 2, 4.0);
        assert!(gaps[2].unwrap().is_nan());
        assert!(gaps[3].unwrap().is_nan());
    }

    #[test]
    fn trix_is_percent_change_of_three_successive_emas_with_signal() {
        let out = trix(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 2, 2);
        assert!(out[..4].iter().all(|point| point.line.is_none()));
        assert_eq!(out[4].line, Some(40.0));
        assert_eq!(out[4].signal, None);
        assert!((out[5].line.unwrap() - 200.0 / 7.0).abs() < 1e-10);
        assert!((out[5].signal.unwrap() - 240.0 / 7.0).abs() < 1e-10);
    }

    #[test]
    fn coppock_curve_weights_the_sum_of_two_percent_changes() {
        let out = coppock_curve(&[1.0, 2.0, 4.0, 8.0, 16.0], 2, 1, 2);
        assert_eq!(out, vec![None, None, None, Some(400.0), Some(400.0)]);
    }

    #[test]
    fn fisher_transform_smooths_clamped_rolling_price_position() {
        let out = fisher_transform(&[2.0, 4.0, 6.0], &[0.0, 2.0, 4.0], 2);
        assert_eq!(
            out[0],
            FisherPoint {
                line: None,
                trigger: None
            }
        );
        let first_value = 0.165_f64;
        let first = 0.5 * ((1.0 + first_value) / (1.0 - first_value)).ln();
        let second_value = 0.165 + 0.67 * first_value;
        let second = 0.5 * ((1.0 + second_value) / (1.0 - second_value)).ln() + 0.5 * first;
        assert!((out[1].line.unwrap() - first).abs() < 1e-12);
        assert_eq!(out[1].trigger, None);
        assert!((out[2].line.unwrap() - second).abs() < 1e-12);
        assert_eq!(out[2].trigger, Some(first));
        let flat = fisher_transform(&[1.0, 1.0, 1.0], &[1.0, 1.0, 1.0], 2);
        assert_eq!(flat[2].line, Some(0.0));
        assert_eq!(flat[2].trigger, Some(0.0));
        let gapped = fisher_transform(&[1.0, f64::NAN, 2.0, 3.0], &[0.0, 1.0, 1.0, 2.0], 2);
        let compact = fisher_transform(&[1.0, 2.0, 3.0], &[0.0, 1.0, 2.0], 2);
        assert!(gapped[1].line.is_none());
        assert_eq!(gapped[2].line, compact[1].line);
        assert_eq!(gapped[3].trigger, compact[2].trigger);
    }

    #[test]
    fn ultimate_oscillator_weights_three_true_range_windows() {
        let out = ultimate_oscillator(
            &[10.0, 12.0, 14.0],
            &[0.0, 2.0, 4.0],
            &[5.0, 9.0, 13.0],
            1,
            2,
            3,
        );
        assert_eq!(out[..2], [None, None]);
        assert!((out[2].unwrap() - 590.0 / 7.0).abs() < 1e-12);
        let flat = ultimate_oscillator(&[1.0; 3], &[1.0; 3], &[1.0; 3], 1, 2, 3);
        assert!(flat[2].unwrap().is_nan());
    }

    #[test]
    fn vortex_sums_cross_bar_movement_against_true_range() {
        let points = vortex(&[2.0, 4.0, 6.0], &[0.0, 2.0, 4.0], &[1.0, 3.0, 5.0], 2);
        assert_eq!(points[0].plus, None);
        assert_eq!(points[1].minus, None);
        assert!((points[2].plus.unwrap() - 4.0 / 3.0).abs() < 1e-12);
        assert_eq!(points[2].minus, Some(0.0));
    }

    #[test]
    fn kst_weights_four_roc_averages_and_sma_signal() {
        let out = kst(&[1.0, 2.0, 4.0], [1; 4], [1; 4], 2);
        assert_eq!(
            out[0],
            KstPoint {
                line: None,
                signal: None
            }
        );
        assert_eq!(
            out[1],
            KstPoint {
                line: Some(1000.0),
                signal: None
            }
        );
        assert_eq!(
            out[2],
            KstPoint {
                line: Some(1000.0),
                signal: Some(1000.0)
            }
        );
        let gap = kst(&[1.0, 2.0, f64::NAN, 4.0, 8.0, 16.0], [1; 4], [1; 4], 2);
        assert!(gap[4].line.unwrap().is_finite());
        assert!(gap[4].signal.unwrap().is_nan());
        assert_eq!(gap[5].signal, Some(1000.0));
        // Different ROC and smoothing windows exercise all four independent weights.
        let closes: Vec<_> = (1..=12).map(f64::from).collect();
        let roc = [1, 2, 3, 4];
        let smoothing = [1, 2, 3, 4];
        let weighted = |row: usize| {
            roc.iter()
                .zip(smoothing)
                .enumerate()
                .map(|(component, (&lag, period))| {
                    let average = (row + 1 - period..=row)
                        .map(|i| (closes[i] / closes[i - lag] - 1.0) * 100.0)
                        .sum::<f64>()
                        / period as f64;
                    (component + 1) as f64 * average
                })
                .sum::<f64>()
        };
        let varied = kst(&closes, roc, smoothing, 2);
        assert!(varied[..7].iter().all(|point| point.line.is_none()));
        assert!((varied[7].line.unwrap() - weighted(7)).abs() < 1e-9);
        assert!((varied[8].signal.unwrap() - (weighted(7) + weighted(8)) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn tsi_double_ema_and_signal_match_hand_derived_momentum() {
        let out = tsi(&[1.0, 2.0, 4.0, 3.0, 5.0], 2, 2, 2);
        assert!(out[..3].iter().all(|point| point.line.is_none()));
        assert!((out[3].line.unwrap() - 50.0).abs() < 1e-12);
        assert_eq!(out[3].signal, None);
        assert!((out[4].line.unwrap() - 2900.0 / 43.0).abs() < 1e-12);
        assert!((out[4].signal.unwrap() - 2525.0 / 43.0).abs() < 1e-12);
        assert!(
            out.iter()
                .filter_map(|point| point.line)
                .all(|value| value.abs() <= 100.0)
        );
        let flat = tsi(&[5.0; 8], 2, 2, 2);
        assert_eq!(flat[4].line, Some(0.0));
        assert_eq!(flat[4].signal, Some(0.0));
        let gap = tsi(
            &[1.0, 2.0, 3.0, 4.0, f64::NAN, 5.0, 6.0, 7.0, 8.0, 9.0],
            2,
            2,
            2,
        );
        let compact = tsi(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0], 2, 2, 2);
        assert!(gap[4].line.unwrap().is_nan());
        assert_eq!(gap[7].line, compact[6].line);
        assert_eq!(gap[8].signal, compact[7].signal);
    }

    #[test]
    fn mass_index_sums_two_ema_ratios_and_continues_after_gap() {
        let lows = [0.0; 5];
        let out = mass_index(&[1.0, 2.0, 4.0, 3.0, 5.0], &lows, 2, 2);
        assert_eq!(out[..3], [None; 3]);
        assert!((out[3].unwrap() - (19.0 / 14.0 + 165.0 / 152.0)).abs() < 1e-12);
        let flat = mass_index(&[0.0; 5], &lows, 2, 2);
        assert!(flat[3].unwrap().is_nan());
        let gap = mass_index(
            &[1.0, 2.0, 4.0, f64::NAN, 1.0, 2.0, 4.0, 3.0],
            &[0.0; 8],
            2,
            2,
        );
        let compact = mass_index(&[1.0, 2.0, 4.0, 1.0, 2.0, 4.0, 3.0], &[0.0; 7], 2, 2);
        assert!(gap[3].unwrap().is_nan());
        assert_eq!(gap[6], compact[5]);
        assert_eq!(gap[7], compact[6]);
    }

    #[test]
    fn klinger_uses_signed_cumulative_range_force_and_ema_signal() {
        // Ranges are all two; trends +,+,-,-. Force = 100, 0, 0, -100/3.
        let highs = [3.0, 4.0, 3.0, 2.0];
        let lows = [1.0, 2.0, 1.0, 0.0];
        let closes = [2.0, 3.0, 2.0, 1.0];
        let out = klinger(&highs, &lows, &closes, &[1.0; 4], 1, 2, 2);
        assert_eq!(out[0].line, None);
        assert_eq!(out[1].line, Some(-50.0));
        assert_eq!(out[1].signal, None);
        assert!((out[2].line.unwrap() + 50.0 / 3.0).abs() < 1e-12);
        assert!((out[2].signal.unwrap() + 100.0 / 3.0).abs() < 1e-12);
        let gap = klinger(
            &[3.0, f64::NAN, 3.0, 4.0],
            &[1.0; 4],
            &[2.0; 4],
            &[1.0; 4],
            1,
            2,
            2,
        );
        assert!(gap[1].line.unwrap().is_nan());
        let compact = klinger(&[3.0, 3.0, 4.0], &[1.0; 3], &[2.0; 3], &[1.0; 3], 1, 2, 2);
        assert_eq!(gap[2].line, compact[1].line);
        assert_eq!(gap[3].signal, compact[2].signal);
    }

    #[test]
    fn kama_squares_efficiency_adjusted_smoothing_and_continues() {
        let out = kama(&[1.0, 2.0, 3.0, 4.0, 3.0, 4.0], 3, 2, 5);
        assert_eq!(out[..2], [None, None]);
        assert_eq!(out[2], Some(2.0));
        assert!((out[3].unwrap() - (2.0 + 2.0 * (2.0_f64 / 3.0).powi(2))).abs() < 1e-12);
        // Net 1, path 3: ER=1/3, SC=(1/3*(2/3-1/3)+1/3)^2 = 16/81.
        let expected = out[3].unwrap() + (3.0 - out[3].unwrap()) * 16.0 / 81.0;
        assert!((out[4].unwrap() - expected).abs() < 1e-12);
        let gap = kama(&[1.0, 2.0, 3.0, f64::NAN, 4.0, 5.0, 6.0], 3, 2, 5);
        let compact = kama(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 3, 2, 5);
        assert!(gap[3].unwrap().is_nan());
        assert_eq!(gap[4], compact[3]);
        assert_eq!(gap[6], compact[5]);
        assert_eq!(kama(&[1.0; 5], 2, 2, 5)[4], Some(1.0));
    }

    #[test]
    fn mcginley_adapts_with_fourth_power_ratio() {
        let out = mcginley(&[2.0, 4.0, 4.0, f64::NAN, 8.0], 2);
        assert_eq!(out[0], Some(2.0));
        assert_eq!(out[1], Some(2.0625)); // 2 + (4-2)/(2 * (4/2)^4)
        assert!(out[3].unwrap().is_nan());
        // The carried state uses the previous valid 4.0 close, not a new 8.0 seed.
        let preceding = out[2].unwrap();
        assert_eq!(
            out[4],
            Some(preceding + (8.0 - preceding) / (2.0 * (8.0 / preceding).powi(4)))
        );
        assert!(mcginley(&[0.0, 2.0], 2)[0].unwrap().is_nan());
    }

    #[test]
    fn regression_channel_uses_residual_not_price_deviation() {
        let out = linear_regression(&[1.0, 3.0, 2.0, 5.0], 3, 2.0);
        assert_eq!(out[0].curve, None);
        assert_eq!(out[1].upper, None);
        // Window 1,3,2: slope 1/2, intercept 3/2, fitted 3/2,2,5/2.
        // Residuals -1/2,1,-1/2: population variance 1/2.
        assert!((out[2].curve.unwrap() - 2.5).abs() < 1e-12);
        assert!((out[2].upper.unwrap() - (2.5 + 2.0 / 2.0_f64.sqrt())).abs() < 1e-12);
        assert!((out[2].lower.unwrap() - (2.5 - 2.0 / 2.0_f64.sqrt())).abs() < 1e-12);
        let linear = linear_regression(&[1.0, 2.0, 3.0], 3, 3.0);
        assert_eq!(linear[2].upper, Some(3.0));
        assert_eq!(linear[2].lower, Some(3.0));
        assert!(
            linear_regression(&[1.0, f64::NAN, 3.0], 2, 2.0)[2]
                .curve
                .unwrap()
                .is_nan()
        );
        assert_eq!(linear_regression(&[5.0], 1, 2.0)[0].curve, Some(5.0));
    }
}

pub fn sma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 {
        return vec![None; values.len()];
    }
    let mut out = vec![None; values.len()];
    for i in period - 1..values.len() {
        out[i] = Some(values[i + 1 - period..=i].iter().sum::<f64>() / period as f64);
    }
    out
}

/// Kaufman's adaptive moving average, SMA-seeded after `period` prices.
/// Efficiency compares the net `period`-bar change with the sum of absolute
/// changes; a flat window has zero efficiency.
pub fn kama(values: &[f64], period: usize, fast: usize, slow: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 || fast == 0 || slow == 0 || fast >= slow {
        return out;
    }
    let mut state = KamaState::default();
    for (row, slot) in out.iter_mut().enumerate() {
        let value = kama_step(&mut state, values, row, period, fast, slow);
        if row + 1 >= period {
            *slot = Some(value.unwrap_or(f64::NAN));
        }
    }
    out
}

/// McGinley Dynamic, initialized at the first close. Missing prices skip
/// the recurrence; a nonpositive price or invalid ratio requires a new seed.
pub fn mcginley(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 {
        return out;
    }
    let mut state = McGinleyState::default();
    for (slot, &value) in out.iter_mut().zip(values) {
        *slot = Some(mcginley_step(&mut state, value, period).unwrap_or(f64::NAN));
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearRegressionPoint {
    pub curve: Option<f64>,
    pub upper: Option<f64>,
    pub lower: Option<f64>,
}

/// Rolling least-squares line evaluated at the current bar. Channel width is
/// `deviation` times the population standard deviation of residuals inside
/// that window (not the standard deviation of the price itself).
pub fn linear_regression(
    values: &[f64],
    period: usize,
    deviation: f64,
) -> Vec<LinearRegressionPoint> {
    let mut out = vec![
        LinearRegressionPoint {
            curve: None,
            upper: None,
            lower: None,
        };
        values.len()
    ];
    if period == 0 || !deviation.is_finite() || deviation < 0.0 {
        return out;
    }
    for (row, slot) in out.iter_mut().enumerate().skip(period - 1) {
        let (curve, upper, lower) = linear_regression_at(values, row, period, deviation);
        *slot = LinearRegressionPoint {
            curve: Some(curve),
            upper: Some(upper),
            lower: Some(lower),
        };
    }
    out
}

/// Choppiness Index on the last `period` true ranges, requiring a prior close
/// for the first range. A zero high/low span or a broken OHLC window is whitespace.
pub fn choppiness(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = highs.len().min(lows.len()).min(closes.len());
    let mut out = vec![None; n];
    if period < 2 {
        return out;
    }
    for (row, slot) in out.iter_mut().enumerate().skip(period) {
        *slot = Some(choppiness_at(highs, lows, closes, row, period));
    }
    out
}

fn choppiness_at(highs: &[f64], lows: &[f64], closes: &[f64], row: usize, period: usize) -> f64 {
    let start = row + 1 - period;
    let mut highest = f64::NEG_INFINITY;
    let mut lowest = f64::INFINITY;
    let mut sum_tr = 0.0;
    for index in start..=row {
        let (high, low, close, previous) =
            (highs[index], lows[index], closes[index], closes[index - 1]);
        if !high.is_finite()
            || !low.is_finite()
            || !close.is_finite()
            || !previous.is_finite()
            || high < low
        {
            return f64::NAN;
        }
        highest = highest.max(high);
        lowest = lowest.min(low);
        sum_tr += (high - low)
            .max((high - previous).abs())
            .max((low - previous).abs());
    }
    let span = highest - lowest;
    if span <= 0.0 || !span.is_finite() || !sum_tr.is_finite() || sum_tr <= 0.0 {
        f64::NAN
    } else {
        (100.0 * (sum_tr / span).log10() / (period as f64).log10()).clamp(0.0, 100.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtrBandsPoint {
    pub basis: Option<f64>,
    pub upper: Option<f64>,
    pub lower: Option<f64>,
}

/// Close-centered bands at `close ± multiplier × Wilder ATR`. Whitespace
/// leaves the ATR seed unchanged and has no output.
pub fn atr_bands(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    period: usize,
    multiplier: f64,
) -> Vec<AtrBandsPoint> {
    let n = highs.len().min(lows.len()).min(closes.len());
    let mut out = vec![
        AtrBandsPoint {
            basis: None,
            upper: None,
            lower: None
        };
        n
    ];
    if period == 0 || !multiplier.is_finite() || multiplier < 0.0 {
        return out;
    }
    let mut state = AtrState::default();
    for (row, slot) in out.iter_mut().enumerate() {
        let atr = atr_bands_step(&mut state, highs[row], lows[row], closes[row], period);
        if atr.is_some() {
            *slot = atr_bands_point(closes[row], atr, multiplier);
        }
    }
    out
}

#[cfg(test)]
mod choppiness_atr_bands_tests {
    use super::*;

    #[test]
    fn empty_single_warmup_flat_and_gapped_ohlc() {
        assert!(choppiness(&[], &[], &[], 3).is_empty());
        assert!(atr_bands(&[], &[], &[], 3, 2.0).is_empty());
        assert_eq!(choppiness(&[2.0], &[1.0], &[1.5], 2), [None]);
        assert_eq!(atr_bands(&[2.0], &[1.0], &[1.5], 2, 2.0)[0].basis, None);

        let highs = [2.0, 2.0, 2.0, 2.0];
        let lows = [0.0; 4];
        let closes = [1.0; 4];
        // Each TR = 2, range = 2: 100 * log10(2) / log10(2).
        assert_eq!(
            choppiness(&highs, &lows, &closes, 2),
            [None, None, Some(100.0), Some(100.0)]
        );
        let bands = atr_bands(&highs, &lows, &closes, 2, 1.5);
        assert_eq!(bands[2].basis, Some(1.0));
        assert_eq!(bands[2].upper, Some(4.0));
        assert_eq!(bands[2].lower, Some(-2.0));
        assert!(
            choppiness(&[1.0; 4], &[1.0; 4], &[1.0; 4], 2)[2]
                .unwrap()
                .is_nan()
        );

        let highs = [2.0, 2.0, f64::NAN, 2.0, 2.0, 2.0, 2.0];
        let chop = choppiness(&highs, &[0.0; 7], &[1.0; 7], 2);
        assert!(chop[2..4].iter().all(|value| value.unwrap().is_nan()));
        assert_eq!(chop[4], Some(100.0));
        assert_eq!(chop[5], Some(100.0));
        let bands = atr_bands(&highs, &[0.0; 7], &[1.0; 7], 2, 1.5);
        assert_eq!(bands[2].upper, None);
        assert_eq!(bands[3].upper, Some(4.0));
        assert_eq!(bands[4].upper, Some(4.0));
        assert_eq!(bands[5].upper, Some(4.0));
    }
}

fn atr_bands_step(
    state: &mut AtrState,
    high: f64,
    low: f64,
    close: f64,
    period: usize,
) -> Option<f64> {
    if !valid_bar(high, low, close) {
        return None;
    }
    atr_step(state, AtrSample { high, low, close }, period)
}

fn atr_bands_point(close: f64, atr: Option<f64>, multiplier: f64) -> AtrBandsPoint {
    let (basis, upper, lower) = match atr {
        Some(atr) if atr.is_finite() => {
            let spread = atr * multiplier;
            (Some(close), Some(close + spread), Some(close - spread))
        }
        _ => (Some(f64::NAN), Some(f64::NAN), Some(f64::NAN)),
    };
    AtrBandsPoint {
        basis,
        upper,
        lower,
    }
}

fn linear_regression_at(
    values: &[f64],
    row: usize,
    period: usize,
    deviation: f64,
) -> (f64, f64, f64) {
    let window = &values[row + 1 - period..=row];
    if window.iter().any(|value| !value.is_finite()) {
        return (f64::NAN, f64::NAN, f64::NAN);
    }
    let n = period as f64;
    let mean_x = (n - 1.0) * 0.5;
    let mean_y = window.iter().sum::<f64>() / n;
    let denominator = n * (n * n - 1.0) / 12.0;
    let slope = if period == 1 {
        0.0
    } else {
        window
            .iter()
            .enumerate()
            .map(|(x, &y)| (x as f64 - mean_x) * (y - mean_y))
            .sum::<f64>()
            / denominator
    };
    let intercept = mean_y - slope * mean_x;
    let curve = intercept + slope * (n - 1.0);
    let variance = window
        .iter()
        .enumerate()
        .map(|(x, &y)| {
            let residual = y - (intercept + slope * x as f64);
            residual * residual
        })
        .sum::<f64>()
        / n;
    let spread = variance.sqrt() * deviation;
    (curve, curve + spread, curve - spread)
}

/// Exponential moving average using the standard SMA seed, followed by the EMA recurrence.
pub fn ema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 {
        return vec![None; values.len()];
    }
    let mut out = vec![None; values.len()];
    let mut state = EmaState::default();
    for (i, &value) in values.iter().enumerate() {
        out[i] = ema_step(&mut state, value, period);
    }
    out
}

/// Double exponential moving average: `2 * EMA(source) - EMA(EMA(source))`.
pub fn dema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 {
        return vec![None; values.len()];
    }
    let mut out = vec![None; values.len()];
    let first = ema(values, period);
    let compact = first.iter().flatten().copied().collect::<Vec<_>>();
    let second = ema(&compact, period);
    let mut second_index = 0;
    for (index, first) in first.into_iter().enumerate() {
        if let Some(first) = first {
            if let Some(second) = second.get(second_index).copied().flatten() {
                out[index] = Some(2.0 * first - second);
            }
            second_index += 1;
        }
    }
    out
}

/// Triple exponential moving average: `3 * EMA(source) - 3 * EMA(EMA(source)) + EMA(EMA(EMA(source)))`.
pub fn tema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 {
        return vec![None; values.len()];
    }
    let mut out = vec![None; values.len()];
    let first = ema(values, period);
    let first_values = first.iter().flatten().copied().collect::<Vec<_>>();
    let second = ema(&first_values, period);
    let second_values = second.iter().flatten().copied().collect::<Vec<_>>();
    let third = ema(&second_values, period);
    let mut second_index = 0;
    let mut third_index = 0;
    for (index, first) in first.into_iter().enumerate() {
        if let Some(first) = first {
            if let Some(second) = second.get(second_index).copied().flatten() {
                if let Some(third) = third.get(third_index).copied().flatten() {
                    out[index] = Some(3.0 * first - 3.0 * second + third);
                }
                third_index += 1;
            }
            second_index += 1;
        }
    }
    out
}

/// Smoothed moving average (also called Wilder's moving average or RMA).
/// The first value is an SMA seed; later values use Wilder's `1 / period` smoothing.
pub fn smma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 {
        return vec![None; values.len()];
    }
    let mut out = vec![None; values.len()];
    let mut state = SmmaState::default();
    for (index, &value) in values.iter().enumerate() {
        out[index] = smma_step(&mut state, value, period);
    }
    out
}

/// Wilder's moving average (RMA), an alias of [`smma`].
pub fn rma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    smma(values, period)
}

/// Hull moving average: `WMA(2 * WMA(source, period / 2) - WMA(source, period), sqrt(period))`.
/// Periods use the conventional floored half and square-root lengths, each clamped to one.
pub fn hma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 {
        return vec![None; values.len()];
    }
    let mut out = vec![None; values.len()];
    let output_start = period
        .saturating_sub(1)
        .saturating_add((period as f64).sqrt() as usize)
        .saturating_sub(1);
    for (row, output) in out.iter_mut().enumerate().skip(output_start) {
        *output = hma_at(values, row, period);
    }
    out
}

/// Volume-weighted moving average. Missing volume rows use unit weight; nonpositive volume
/// contributes zero, and an all-zero window falls back to its simple average.
pub fn vwma(values: &[f64], volumes: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 {
        return vec![None; values.len()];
    }
    let mut out = vec![None; values.len()];
    for (row, output) in out.iter_mut().enumerate().skip(period.saturating_sub(1)) {
        *output = vwma_at(values, volumes, row, period);
    }
    out
}

/// Population standard deviation over a rolling window.
pub fn standard_deviation(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 {
        return vec![None; values.len()];
    }
    let mut out = vec![None; values.len()];
    for (row, output) in out.iter_mut().enumerate().skip(period.saturating_sub(1)) {
        let start = row + 1 - period;
        let window = &values[start..=row];
        let mean = window.iter().sum::<f64>() / period as f64;
        let variance = window
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum::<f64>()
            / period as f64;
        *output = Some(variance.sqrt());
    }
    out
}

/// Donchian channel: rolling high, midpoint, and rolling low over the high/low columns.
pub fn donchian(high: &[f64], low: &[f64], period: usize) -> Vec<DonchianPoint> {
    let length = high.len().min(low.len());
    let mut out = vec![
        DonchianPoint {
            upper: None,
            middle: None,
            lower: None,
        };
        length
    ];
    if period == 0 {
        return out;
    }
    for (row, output) in out.iter_mut().enumerate().skip(period.saturating_sub(1)) {
        let start = row + 1 - period;
        if !(start..=row).all(|index| valid_range(high[index], low[index])) {
            *output = DonchianPoint {
                upper: Some(f64::NAN),
                middle: Some(f64::NAN),
                lower: Some(f64::NAN),
            };
            continue;
        }
        let upper = high[start..=row]
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let lower = low[start..=row]
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        *output = DonchianPoint {
            upper: Some(upper),
            middle: Some((upper + lower) * 0.5),
            lower: Some(lower),
        };
    }
    out
}

/// Keltner channel using an EMA center and Wilder ATR envelope.
pub fn keltner(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    period: usize,
    multiplier: f64,
) -> Vec<KeltnerPoint> {
    let n = highs.len().min(lows.len()).min(closes.len());
    let mut out = vec![
        KeltnerPoint {
            upper: None,
            middle: None,
            lower: None,
        };
        n
    ];
    if period == 0 {
        return out;
    }
    let mut state = KeltnerState::default();
    for row in 0..n {
        out[row] = keltner_step(
            &mut state,
            AtrSample {
                high: highs[row],
                low: lows[row],
                close: closes[row],
            },
            period,
            multiplier,
        );
    }
    out
}

/// Wilder's directional movement index and ADX. The first directional values are available after
/// `period` price changes; ADX is seeded after a further `period - 1` DX values.
pub fn adx_dmi(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Vec<AdxDmiPoint> {
    let n = highs.len().min(lows.len()).min(closes.len());
    let mut out = vec![
        AdxDmiPoint {
            plus_di: None,
            minus_di: None,
            adx: None,
        };
        n
    ];
    if period == 0 {
        return out;
    }
    let mut state = AdxDmiState::default();
    for row in 0..n {
        if valid_bar(highs[row], lows[row], closes[row]) {
            out[row] = adx_dmi_step(
                &mut state,
                DirectionalSample {
                    high: highs[row],
                    low: lows[row],
                    close: closes[row],
                },
                period,
            );
        }
    }
    out
}

/// Commodity Channel Index using the typical price and a rolling mean deviation.
/// The conventional constant is 0.015; a zero-deviation window emits zero rather than NaN.
pub fn cci(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = highs.len().min(lows.len()).min(closes.len());
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    for (row, output) in out.iter_mut().enumerate().skip(period.saturating_sub(1)) {
        let start = row + 1 - period;
        let typical = |index: usize| (highs[index] + lows[index] + closes[index]) / 3.0;
        if !(start..=row).all(|index| valid_bar(highs[index], lows[index], closes[index])) {
            *output = Some(f64::NAN);
            continue;
        }
        let mean = (start..=row).map(typical).sum::<f64>() / period as f64;
        let mean_deviation = (start..=row)
            .map(|index| (typical(index) - mean).abs())
            .sum::<f64>()
            / period as f64;
        *output = Some(if mean_deviation > 0.0 {
            (typical(row) - mean) / (0.015 * mean_deviation)
        } else {
            0.0
        });
    }
    out
}

/// Williams %R over a rolling high/low window. Flat windows emit zero instead of NaN.
pub fn williams_r(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = highs.len().min(lows.len()).min(closes.len());
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    for (row, output) in out.iter_mut().enumerate().skip(period.saturating_sub(1)) {
        let start = row + 1 - period;
        if !(start..=row).all(|index| valid_bar(highs[index], lows[index], closes[index])) {
            *output = Some(f64::NAN);
            continue;
        }
        let high = highs[start..=row]
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let low = lows[start..=row]
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        *output = Some(if high > low {
            -100.0 * (high - closes[row]) / (high - low)
        } else {
            0.0
        });
    }
    out
}

/// Stochastic RSI: normalize Wilder RSI within a rolling RSI range.
/// The first value is available after both the RSI and stochastic windows warm up.
pub fn stochastic_rsi(
    values: &[f64],
    rsi_period: usize,
    stochastic_period: usize,
) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if rsi_period == 0 || stochastic_period == 0 {
        return out;
    }
    let rsi_values = rsi(values, rsi_period);
    let mut window = VecDeque::with_capacity(stochastic_period.min(values.len()));
    for (row, output) in out.iter_mut().enumerate() {
        let Some(current) = rsi_values[row] else {
            continue;
        };
        window.push_back(current);
        if window.len() > stochastic_period {
            window.pop_front();
        }
        if window.len() < stochastic_period {
            continue;
        }
        let low = window.iter().copied().fold(f64::INFINITY, f64::min);
        let high = window.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        *output = Some(if high > low {
            100.0 * (current - low) / (high - low)
        } else {
            0.0
        });
    }
    out
}

/// Momentum as the current value minus the value `period` rows earlier.
/// A missing row anywhere in the lag window leaves the result blank.
pub fn momentum(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 {
        return out;
    }
    let mut valid_run = 0;
    for (row, output) in out.iter_mut().enumerate() {
        valid_run = if values[row].is_finite() {
            valid_run + 1
        } else {
            0
        };
        if row >= period && valid_run > period {
            *output = Some(values[row] - values[row - period]);
        }
    }
    out
}

/// Rate of change as a percentage difference from the value `period` rows earlier.
/// A zero denominator emits zero instead of a non-finite value.
pub fn rate_of_change(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 {
        return out;
    }
    let mut valid_run = 0;
    for (row, output) in out.iter_mut().enumerate() {
        valid_run = if values[row].is_finite() {
            valid_run + 1
        } else {
            0
        };
        if row < period || valid_run <= period {
            continue;
        }
        let previous = values[row - period];
        *output = Some(if previous != 0.0 {
            (values[row] / previous - 1.0) * 100.0
        } else {
            0.0
        });
    }
    out
}

fn wma_at(values: &[f64], row: usize, period: usize) -> Option<f64> {
    if period == 0 || row.saturating_add(1) < period {
        return None;
    }
    let denominator = (period * (period + 1)) as f64 / 2.0;
    let start = row + 1 - period;
    Some(
        values[start..=row]
            .iter()
            .enumerate()
            .map(|(weight, value)| (weight + 1) as f64 * value)
            .sum::<f64>()
            / denominator,
    )
}

fn hma_at(values: &[f64], row: usize, period: usize) -> Option<f64> {
    let half_period = (period / 2).max(1);
    let smoothing_period = ((period as f64).sqrt() as usize).max(1);
    let raw_start = row + 1 - smoothing_period;
    let denominator = (smoothing_period * (smoothing_period + 1)) as f64 / 2.0;
    let weighted = (raw_start..=row)
        .enumerate()
        .map(|(weight, raw_row)| {
            let half = wma_at(values, raw_row, half_period)?;
            let full = wma_at(values, raw_row, period)?;
            Some((weight + 1) as f64 * (2.0 * half - full))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(weighted.into_iter().sum::<f64>() / denominator)
}

fn vwma_at(values: &[f64], volumes: &[f64], row: usize, period: usize) -> Option<f64> {
    if period == 0 || row.saturating_add(1) < period {
        return None;
    }
    let start = row + 1 - period;
    let mut weighted_sum = 0.0;
    let mut volume_sum = 0.0;
    let mut simple_sum = 0.0;
    for (index, &value) in values.iter().enumerate().take(row + 1).skip(start) {
        let volume = volumes.get(index).copied().unwrap_or(1.0).max(0.0);
        weighted_sum += value * volume;
        volume_sum += volume;
        simple_sum += value;
    }
    Some(if volume_sum > 0.0 {
        weighted_sum / volume_sum
    } else {
        simple_sum / period as f64
    })
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

/// Bollinger %B (unit interval at the bands) and BandWidth (percent of basis).
pub fn bollinger_metrics(
    values: &[f64],
    period: usize,
    deviation: f64,
) -> Vec<(Option<f64>, Option<f64>)> {
    bollinger(values, period, deviation)
        .into_iter()
        .enumerate()
        .map(|(row, point)| {
            let (Some(upper), Some(middle), Some(lower)) = (point.upper, point.middle, point.lower)
            else {
                return (None, None);
            };
            let width = upper - lower;
            let percent_b = (width != 0.0).then_some((values[row] - lower) / width);
            let bandwidth = (middle != 0.0).then_some(width / middle * 100.0);
            (percent_b, bandwidth)
        })
        .collect()
}

/// Moving-average envelopes, with a percentage distance around SMA or EMA basis.
pub fn envelopes(
    values: &[f64],
    period: usize,
    percent: f64,
    exponential: bool,
) -> Vec<(Option<f64>, Option<f64>, Option<f64>)> {
    let basis = if exponential {
        ema(values, period)
    } else {
        sma(values, period)
    };
    let fraction = percent / 100.0;
    basis
        .into_iter()
        .map(|value| match value {
            Some(middle) => (
                Some(middle * (1.0 + fraction)),
                Some(middle),
                Some(middle * (1.0 - fraction)),
            ),
            None => (None, None, None),
        })
        .collect()
}

fn alma_weights(period: usize, offset: f64, sigma: f64) -> (Vec<f64>, f64) {
    if period == 0 || !offset.is_finite() || !sigma.is_finite() || sigma <= 0.0 {
        return (Vec::new(), 0.0);
    }
    let center = offset * (period - 1) as f64;
    let width = period as f64 / sigma;
    let exponents = (0..period)
        .map(|index| {
            let z = (index as f64 - center) / width;
            -0.5 * z * z
        })
        .collect::<Vec<_>>();
    let maximum = exponents.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let weights = exponents
        .into_iter()
        .map(|exponent| (exponent - maximum).exp())
        .collect::<Vec<_>>();
    let sum = weights.iter().sum();
    (weights, sum)
}

/// Gaussian-weighted moving average with the weight peak shifted by `offset`.
pub fn alma(values: &[f64], period: usize, offset: f64, sigma: f64) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    let (weights, sum) = alma_weights(period, offset, sigma);
    if sum == 0.0 {
        return out;
    }
    for row in period.saturating_sub(1)..values.len() {
        let window = &values[row + 1 - period..=row];
        out[row] = Some(
            window
                .iter()
                .zip(&weights)
                .map(|(value, weight)| value * weight)
                .sum::<f64>()
                / sum,
        );
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
    let mut state = IndexedRsiState::default();
    for (row, &value) in values.iter().enumerate() {
        out[row] = indexed_rsi_step(&mut state, value, period);
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
    let mut state = MacdState::default();
    for i in 0..n {
        out[i] = macd_step(&mut state, values[i], fast, slow, signal);
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StochasticPoint {
    pub k: Option<f64>,
    pub d: Option<f64>,
}

// Keep the last k valid bars' extrema, not the last k physical rows' extrema. In a
// flat window after whitespace, the compacted series can have already computed a
// different %K from windows that spanned the gap. Monotone queues make the carry
// update amortized O(1) per valid bar, including across arbitrarily long gaps.
#[derive(Clone, Debug, Default)]
struct StochasticCarry {
    previous_k: f64,
    valid_count: usize,
    highs: std::collections::VecDeque<(usize, f64)>,
    lows: std::collections::VecDeque<(usize, f64)>,
}

impl StochasticCarry {
    fn advance(&mut self, high: f64, low: f64, close: f64, period: usize) -> f64 {
        self.valid_count += 1;
        let index = self.valid_count;
        while self.highs.back().is_some_and(|&(_, value)| value <= high) {
            self.highs.pop_back();
        }
        self.highs.push_back((index, high));
        while self.lows.back().is_some_and(|&(_, value)| value >= low) {
            self.lows.pop_back();
        }
        self.lows.push_back((index, low));
        let oldest = index.saturating_sub(period);
        while self.highs.front().is_some_and(|&(row, _)| row <= oldest) {
            self.highs.pop_front();
        }
        while self.lows.front().is_some_and(|&(row, _)| row <= oldest) {
            self.lows.pop_front();
        }
        if index >= period {
            let hh = self.highs.front().unwrap().1;
            let ll = self.lows.front().unwrap().1;
            self.previous_k = if hh > ll {
                100.0 * (close - ll) / (hh - ll)
            } else if index == period {
                50.0
            } else {
                self.previous_k
            };
        }
        self.previous_k
    }

    fn bytes(&self) -> usize {
        (self.highs.capacity() + self.lows.capacity()) * std::mem::size_of::<(usize, f64)>()
    }
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
    let mut carry = StochasticCarry::default();
    for i in 0..n {
        if valid_bar(highs[i], lows[i], closes[i]) {
            carry.advance(highs[i], lows[i], closes[i], k_period);
        }
        if i + 1 < k_period {
            continue;
        }
        if !(i + 1 - k_period..=i).all(|row| valid_bar(highs[row], lows[row], closes[row])) {
            raw_k[i] = Some(f64::NAN);
            continue;
        }
        raw_k[i] = Some(carry.previous_k);
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
    let mut state = AtrState::default();
    for (i, slot) in out.iter_mut().enumerate() {
        if valid_bar(highs[i], lows[i], closes[i]) {
            *slot = atr_step(
                &mut state,
                AtrSample {
                    high: highs[i],
                    low: lows[i],
                    close: closes[i],
                },
                period,
            );
        }
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
    let mut state = VwapState::default();
    for i in 0..n {
        out[i] = Some(vwap_step(
            &mut state,
            VwapSample {
                time_unix_seconds: times[i],
                high: highs[i],
                low: lows[i],
                close: closes[i],
                volume: volumes.get(i).copied(),
            },
        ));
    }
    out
}

/// On-balance volume, seeded at zero and accumulated using each bar's volume according to the
/// close-to-close direction. Non-positive volumes contribute zero so malformed provider values
/// cannot invert the direction signal.
pub fn obv(closes: &[f64], volumes: &[f64]) -> Vec<Option<f64>> {
    let n = closes.len().min(volumes.len());
    let mut out = vec![None; n];
    if n == 0 {
        return out;
    }
    let mut state = CumulativeCloseVolumeState::default();
    for (row, slot) in out.iter_mut().enumerate() {
        *slot = Some(obv_step(&mut state, closes[row], volumes[row]));
    }
    out
}

/// Cumulative money-flow volume. Flat bars and non-positive volume contribute zero.
pub fn accumulation_distribution(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    volumes: &[f64],
) -> Vec<Option<f64>> {
    let n = highs
        .len()
        .min(lows.len())
        .min(closes.len())
        .min(volumes.len());
    let mut out = vec![None; n];
    let mut cumulative = 0.0;
    for row in 0..n {
        out[row] = Some(accumulation_distribution_step(
            &mut cumulative,
            highs[row],
            lows[row],
            closes[row],
            volumes[row],
        ));
    }
    out
}

/// Price Volume Trend, seeded at zero. A zero previous close contributes no change.
pub fn price_volume_trend(closes: &[f64], volumes: &[f64]) -> Vec<Option<f64>> {
    let n = closes.len().min(volumes.len());
    let mut out = vec![None; n];
    if n == 0 {
        return out;
    }
    let mut state = CumulativeCloseVolumeState::default();
    for row in 0..n {
        out[row] = Some(price_volume_trend_step(
            &mut state,
            closes[row],
            volumes[row],
        ));
    }
    out
}

/// Chaikin Oscillator: fast EMA of A/D minus slow EMA of A/D.
pub fn chaikin_oscillator(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    volumes: &[f64],
    fast: usize,
    slow: usize,
) -> Vec<Option<f64>> {
    let n = highs
        .len()
        .min(lows.len())
        .min(closes.len())
        .min(volumes.len());
    let mut out = vec![None; n];
    if fast == 0 || slow == 0 || fast >= slow {
        return out;
    }
    let mut state = ChaikinState::default();
    for row in 0..n {
        let adl = accumulation_distribution_step(
            &mut state.cumulative,
            highs[row],
            lows[row],
            closes[row],
            volumes[row],
        );
        let fast_value = ema_step(&mut state.fast, adl, fast);
        let slow_value = ema_step(&mut state.slow, adl, slow);
        if row >= slow - 1 {
            out[row] = Some(fast_value.zip(slow_value).map_or(f64::NAN, |(a, b)| a - b));
        }
    }
    out
}

/// Current non-negative volume divided by the mean of the previous `period` bars.
pub fn relative_volume(volumes: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; volumes.len()];
    if period == 0 {
        return out;
    }
    for row in period..volumes.len() {
        if !volumes[row - period..=row].iter().all(|v| v.is_finite()) {
            out[row] = Some(f64::NAN);
            continue;
        }
        let sum = volumes[row - period..row]
            .iter()
            .map(|volume| volume.max(0.0))
            .sum::<f64>();
        out[row] = Some(if sum > 0.0 {
            volumes[row].max(0.0) * period as f64 / sum
        } else {
            f64::NAN
        });
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeOscillatorPoint {
    pub line: Option<f64>,
    pub signal: Option<f64>,
    pub histogram: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KlingerPoint {
    pub line: Option<f64>,
    pub signal: Option<f64>,
}

/// Klinger volume oscillator: EMA(fast) - EMA(slow) of signed volume force,
/// and an EMA signal. The trend follows the change in high + low + close;
/// cumulative measurement restarts when the trend reverses. Flat ranges
/// contribute zero force. Whitespace OHLC rows leave the EMA seeds unchanged.
pub fn klinger(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    volumes: &[f64],
    fast: usize,
    slow: usize,
    signal: usize,
) -> Vec<KlingerPoint> {
    let n = highs
        .len()
        .min(lows.len())
        .min(closes.len())
        .min(volumes.len());
    let mut out = vec![
        KlingerPoint {
            line: None,
            signal: None
        };
        n
    ];
    if fast == 0 || slow == 0 || signal == 0 || fast >= slow {
        return out;
    }
    let mut state = KlingerState::default();
    let signal_start = slow.saturating_add(signal).saturating_sub(2);
    for row in 0..n {
        let (line, signal_value) = klinger_step(
            &mut state,
            highs[row],
            lows[row],
            closes[row],
            volumes[row],
            [fast, slow, signal],
        );
        if row + 1 >= slow {
            out[row].line = Some(line.unwrap_or(f64::NAN));
        }
        if row >= signal_start {
            out[row].signal = Some(signal_value.unwrap_or(f64::NAN));
        }
    }
    out
}

/// Percentage Volume Oscillator with an EMA signal and line-minus-signal histogram.
pub fn volume_oscillator(
    volumes: &[f64],
    fast: usize,
    slow: usize,
    signal: usize,
) -> Vec<VolumeOscillatorPoint> {
    let mut out = vec![
        VolumeOscillatorPoint {
            line: None,
            signal: None,
            histogram: None,
        };
        volumes.len()
    ];
    if fast == 0 || slow == 0 || signal == 0 || fast >= slow {
        return out;
    }
    let mut state = MacdState::default();
    for (row, &volume) in volumes.iter().enumerate() {
        let point = volume_oscillator_step(&mut state, volume, fast, slow, signal);
        if row >= slow - 1 {
            out[row].line = Some(point.line.unwrap_or(f64::NAN));
        }
        if row >= slow + signal - 2 {
            out[row].signal = Some(point.signal.unwrap_or(f64::NAN));
            out[row].histogram = Some(point.histogram.unwrap_or(f64::NAN));
        }
    }
    out
}

/// Elder Force Index: EMA of close-to-close change times non-negative current volume.
/// The first bar has no preceding close and does not seed the EMA.
pub fn elder_force(closes: &[f64], volumes: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = closes.len().min(volumes.len());
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    let mut ema_state = EmaState::default();
    let mut previous = None;
    for row in 0..n {
        let close = closes[row];
        if !close.is_finite() {
            continue;
        }
        if let Some(last) = previous {
            let force = (close - last) * volumes[row].max(0.0);
            out[row] = ema_step(&mut ema_state, force, period);
        }
        previous = Some(close);
    }
    out
}

/// Ease of Movement: SMA of midpoint distance times high-low range divided by normalized
/// current volume. A zero-volume bar makes each covering window undefined, preserving the gap.
pub fn ease_of_movement(
    highs: &[f64],
    lows: &[f64],
    volumes: &[f64],
    period: usize,
    divisor: f64,
) -> Vec<Option<f64>> {
    let n = highs.len().min(lows.len()).min(volumes.len());
    let mut out = vec![None; n];
    if period == 0 || !divisor.is_finite() || divisor <= 0.0 {
        return out;
    }
    let mut sum = 0.0;
    let mut missing = 0;
    for (row, slot) in out.iter_mut().enumerate().skip(1) {
        let raw = ease_of_movement_raw(highs, lows, volumes, row, divisor);
        if raw.is_finite() {
            sum += raw;
        } else {
            missing += 1;
        }
        if row > period {
            let outgoing = ease_of_movement_raw(highs, lows, volumes, row - period, divisor);
            if outgoing.is_finite() {
                sum -= outgoing;
            } else {
                missing -= 1;
            }
        }
        if row >= period {
            *slot = Some(if missing == 0 {
                sum / period as f64
            } else {
                f64::NAN
            });
        }
    }
    out
}

/// Annualized sample standard deviation of close-to-close log returns, in percent.
/// `annualization` is the number of chart bars per year, typically 252 for daily equities.
pub fn historical_volatility(
    closes: &[f64],
    period: usize,
    annualization: f64,
) -> Vec<Option<f64>> {
    let mut out = vec![None; closes.len()];
    if period < 2 || !annualization.is_finite() || annualization <= 0.0 {
        return out;
    }
    let mut window = HistoricalVolatilityState::default();
    for (row, slot) in out.iter_mut().enumerate() {
        *slot = historical_volatility_step(&mut window, closes, row, period, annualization);
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrixPoint {
    pub line: Option<f64>,
    pub signal: Option<f64>,
}

/// Percent change of a triple-smoothed EMA and an EMA of that percent-change line.
pub fn trix(values: &[f64], period: usize, signal_period: usize) -> Vec<TrixPoint> {
    let mut out = vec![
        TrixPoint {
            line: None,
            signal: None
        };
        values.len()
    ];
    if period == 0 || signal_period == 0 {
        return out;
    }
    let mut state = TrixState::default();
    for (&value, point) in values.iter().zip(out.iter_mut()) {
        let (line, signal) = trix_step(&mut state, value, period, signal_period);
        point.line = line;
        point.signal = signal;
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KstPoint {
    pub line: Option<f64>,
    pub signal: Option<f64>,
}

/// Know Sure Thing: 1:2:3:4 weighted SMAs of four percentage ROC series,
/// with an SMA signal. Each unavailable ROC invalidates its whole smoothing window.
pub fn kst(
    closes: &[f64],
    roc_periods: [usize; 4],
    sma_periods: [usize; 4],
    signal_period: usize,
) -> Vec<KstPoint> {
    let mut out = vec![
        KstPoint {
            line: None,
            signal: None
        };
        closes.len()
    ];
    if roc_periods.contains(&0) || sma_periods.contains(&0) || signal_period == 0 {
        return out;
    }
    let start = kst_line_start(roc_periods, sma_periods);
    for (row, point) in out.iter_mut().enumerate().skip(start) {
        point.line = Some(kst_at(closes, row, roc_periods, sma_periods));
        if row >= start.saturating_add(signal_period - 1) {
            let signal = (row + 1 - signal_period..=row)
                .map(|index| kst_at(closes, index, roc_periods, sma_periods))
                .fold(0.0, |sum, value| sum + value);
            point.signal = Some(signal / signal_period as f64);
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TsiPoint {
    pub line: Option<f64>,
    pub signal: Option<f64>,
}

/// True Strength Index: double EMA of close momentum divided by double EMA of its
/// absolute value, multiplied by 100, with an EMA signal. Flat momentum gives zero.
pub fn tsi(closes: &[f64], long: usize, short: usize, signal_period: usize) -> Vec<TsiPoint> {
    let mut out = vec![
        TsiPoint {
            line: None,
            signal: None
        };
        closes.len()
    ];
    if long == 0 || short == 0 || signal_period == 0 {
        return out;
    }
    let line_start = long.saturating_add(short).saturating_sub(1);
    let mut state = TsiState::default();
    for (row, point) in out.iter_mut().enumerate() {
        let (line, signal) = tsi_step(&mut state, closes[row], long, short, signal_period);
        if row >= line_start {
            point.line = Some(line.unwrap_or(f64::NAN));
        }
        if row >= line_start.saturating_add(signal_period - 1) {
            point.signal = Some(signal.unwrap_or(f64::NAN));
        }
    }
    out
}

/// Sum of `sum_period` ratios of EMA(high-low) to its second EMA.
pub fn mass_index(
    highs: &[f64],
    lows: &[f64],
    ema_period: usize,
    sum_period: usize,
) -> Vec<Option<f64>> {
    let n = highs.len().min(lows.len());
    let mut out = vec![None; n];
    if ema_period == 0 || sum_period == 0 {
        return out;
    }
    let mut state = MassState::default();
    let start = mass_start(ema_period, sum_period);
    for row in 0..n {
        let value = mass_index_step(&mut state, highs[row], lows[row], ema_period, sum_period);
        if row >= start {
            out[row] = Some(value.unwrap_or(f64::NAN));
        }
    }
    out
}

/// Weighted moving average of the sum of long and short percentage rates of change.
pub fn coppock_curve(
    closes: &[f64],
    long_period: usize,
    short_period: usize,
    smoothing: usize,
) -> Vec<Option<f64>> {
    let mut out = vec![None; closes.len()];
    if long_period == 0 || short_period == 0 || smoothing == 0 {
        return out;
    }
    let first = coppock_start(long_period, short_period, smoothing);
    for (row, slot) in out.iter_mut().enumerate().skip(first) {
        *slot = Some(coppock_at(
            closes,
            row,
            long_period,
            short_period,
            smoothing,
        ));
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FisherPoint {
    pub line: Option<f64>,
    pub trigger: Option<f64>,
}

/// Ehlers Fisher Transform of median price over rolling high/low extrema. The trigger is the
/// previous Fisher line. Flat ranges normalize to the midpoint; whitespace skips the recurrence.
pub fn fisher_transform(highs: &[f64], lows: &[f64], period: usize) -> Vec<FisherPoint> {
    let n = highs.len().min(lows.len());
    let mut out = vec![
        FisherPoint {
            line: None,
            trigger: None
        };
        n
    ];
    if period == 0 {
        return out;
    }
    let mut window = FisherWindow::default();
    let mut state = FisherState::default();
    for (row, point) in out.iter_mut().enumerate() {
        window.advance(highs, lows, row, period);
        if window.valid_rows.len() == period && valid_range(highs[row], lows[row]) {
            let (line, trigger) = fisher_step(&mut state, &window, highs, lows, row);
            point.line = Some(line);
            if row >= period {
                point.trigger = Some(trigger);
            }
        }
    }
    out
}

/// Weighted buying pressure relative to true range across three windows (4:2:1).
pub fn ultimate_oscillator(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    short: usize,
    medium: usize,
    long: usize,
) -> Vec<Option<f64>> {
    let n = highs.len().min(lows.len()).min(closes.len());
    let mut out = vec![None; n];
    if short == 0 || medium == 0 || long == 0 {
        return out;
    }
    for (row, slot) in out
        .iter_mut()
        .enumerate()
        .skip(short.max(medium).max(long) - 1)
    {
        *slot = Some(ultimate_at(highs, lows, closes, row, [short, medium, long]));
    }
    out
}

fn ultimate_at(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    row: usize,
    periods: [usize; 3],
) -> f64 {
    let mut averages = [0.0; 3];
    for (average, period) in averages.iter_mut().zip(periods) {
        let mut pressure = 0.0;
        let mut range = 0.0;
        for index in row + 1 - period..=row {
            let previous = if index == 0 {
                closes[0]
            } else {
                closes[index - 1]
            };
            let bottom = lows[index].min(previous);
            let top = highs[index].max(previous);
            let current_pressure = closes[index] - bottom;
            let current_range = top - bottom;
            if !previous.is_finite()
                || !highs[index].is_finite()
                || !lows[index].is_finite()
                || !closes[index].is_finite()
                || current_range < 0.0
            {
                return f64::NAN;
            }
            pressure += current_pressure;
            range += current_range;
        }
        if range <= 0.0 {
            return f64::NAN;
        }
        *average = pressure / range;
    }
    100.0 * (4.0 * averages[0] + 2.0 * averages[1] + averages[2]) / 7.0
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VortexPoint {
    pub plus: Option<f64>,
    pub minus: Option<f64>,
}

/// Positive and negative vortex movement divided by true range over a common window.
pub fn vortex(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Vec<VortexPoint> {
    let n = highs.len().min(lows.len()).min(closes.len());
    let mut out = vec![
        VortexPoint {
            plus: None,
            minus: None
        };
        n
    ];
    if period == 0 {
        return out;
    }
    for (row, point) in out.iter_mut().enumerate().skip(period) {
        let (plus, minus) = vortex_at(highs, lows, closes, row, period);
        point.plus = Some(plus);
        point.minus = Some(minus);
    }
    out
}

fn vortex_at(highs: &[f64], lows: &[f64], closes: &[f64], row: usize, period: usize) -> (f64, f64) {
    let mut plus = 0.0;
    let mut minus = 0.0;
    let mut range = 0.0;
    for index in row + 1 - period..=row {
        let previous = index - 1;
        if !highs[index].is_finite()
            || !lows[index].is_finite()
            || !highs[previous].is_finite()
            || !lows[previous].is_finite()
            || !closes[previous].is_finite()
        {
            return (f64::NAN, f64::NAN);
        }
        plus += (highs[index] - lows[previous]).abs();
        minus += (lows[index] - highs[previous]).abs();
        range += (highs[index] - lows[index])
            .max((highs[index] - closes[previous]).abs())
            .max((lows[index] - closes[previous]).abs());
    }
    if range <= 0.0 {
        return (f64::NAN, f64::NAN);
    }
    (plus / range, minus / range)
}

#[derive(Clone, Copy, Debug, Default)]
struct FisherState {
    normalized: f64,
    line: f64,
    valid: bool,
}

#[derive(Clone, Debug, Default)]
struct FisherWindow {
    high_deque: VecDeque<usize>,
    low_deque: VecDeque<usize>,
    valid_rows: VecDeque<usize>,
}

impl FisherWindow {
    fn clear(&mut self) {
        self.high_deque.clear();
        self.low_deque.clear();
        self.valid_rows.clear();
    }

    fn bytes(&self) -> usize {
        (self.high_deque.capacity() + self.low_deque.capacity() + self.valid_rows.capacity())
            * std::mem::size_of::<usize>()
    }

    fn advance(&mut self, highs: &[f64], lows: &[f64], row: usize, period: usize) {
        let high = highs[row];
        let low = lows[row];
        if !high.is_finite() || !low.is_finite() {
            return;
        }
        self.valid_rows.push_back(row);
        if self.valid_rows.len() > period {
            self.valid_rows.pop_front();
        }
        while self
            .high_deque
            .back()
            .is_some_and(|&index| high >= highs[index])
        {
            self.high_deque.pop_back();
        }
        while self
            .low_deque
            .back()
            .is_some_and(|&index| low <= lows[index])
        {
            self.low_deque.pop_back();
        }
        self.high_deque.push_back(row);
        self.low_deque.push_back(row);
        let first = *self.valid_rows.front().expect("current valid Fisher row");
        while self.high_deque.front().is_some_and(|&index| index < first) {
            self.high_deque.pop_front();
        }
        while self.low_deque.front().is_some_and(|&index| index < first) {
            self.low_deque.pop_front();
        }
    }
}

fn fisher_step(
    state: &mut FisherState,
    window: &FisherWindow,
    highs: &[f64],
    lows: &[f64],
    row: usize,
) -> (f64, f64) {
    let highest = highs[*window.high_deque.front().expect("valid Fisher high")];
    let lowest = lows[*window.low_deque.front().expect("valid Fisher low")];
    let midpoint = (highs[row] + lows[row]) * 0.5;
    let position = if highest > lowest {
        ((midpoint - lowest) / (highest - lowest)).clamp(0.0, 1.0)
    } else {
        0.5
    };
    let normalized = (0.66 * (position - 0.5) + 0.67 * state.normalized).clamp(-0.999, 0.999);
    let trigger = if state.valid { state.line } else { f64::NAN };
    let line = 0.5 * ((1.0 + normalized) / (1.0 - normalized)).ln() + 0.5 * state.line;
    state.normalized = normalized;
    state.line = line;
    state.valid = true;
    (line, trigger)
}

fn coppock_start(long_period: usize, short_period: usize, smoothing: usize) -> usize {
    long_period
        .max(short_period)
        .saturating_add(smoothing.saturating_sub(1))
}

fn coppock_at(
    closes: &[f64],
    row: usize,
    long_period: usize,
    short_period: usize,
    smoothing: usize,
) -> f64 {
    let first = row + 1 - smoothing;
    if !closes[first - long_period.max(short_period)..=row]
        .iter()
        .all(|close| close.is_finite())
    {
        return f64::NAN;
    }
    let weighted = (first..=row)
        .enumerate()
        .map(|(index, current)| {
            let close = closes[current];
            let long_base = closes[current - long_period];
            let short_base = closes[current - short_period];
            let long = if long_base != 0.0 {
                (close / long_base - 1.0) * 100.0
            } else {
                0.0
            };
            let short = if short_base != 0.0 {
                (close / short_base - 1.0) * 100.0
            } else {
                0.0
            };
            (index + 1) as f64 * (long + short)
        })
        .sum::<f64>();
    weighted / (smoothing as f64 * (smoothing as f64 + 1.0) * 0.5)
}

fn trix_line_start(period: usize) -> usize {
    period.saturating_sub(1).saturating_mul(3).saturating_add(1)
}

#[derive(Clone, Copy, Debug, Default)]
struct TrixState {
    first: EmaState,
    second: EmaState,
    third: EmaState,
    signal: EmaState,
    previous_triple: Option<f64>,
}

fn trix_step(
    state: &mut TrixState,
    sample: f64,
    period: usize,
    signal_period: usize,
) -> (Option<f64>, Option<f64>) {
    if !sample.is_finite() {
        return (None, None);
    }
    let triple = ema_step(&mut state.first, sample, period)
        .and_then(|value| ema_step(&mut state.second, value, period))
        .and_then(|value| ema_step(&mut state.third, value, period));
    let Some(triple) = triple else {
        return (None, None);
    };
    let previous = state.previous_triple.replace(triple);
    let line = previous.map(|previous| {
        if previous == 0.0 {
            f64::NAN
        } else {
            (triple - previous) / previous * 100.0
        }
    });
    let signal = line.and_then(|line| {
        if line.is_finite() {
            ema_step(&mut state.signal, line, signal_period)
        } else {
            state.signal = EmaState::default();
            None
        }
    });
    (line, signal)
}

fn kst_line_start(roc: [usize; 4], smooth: [usize; 4]) -> usize {
    roc.into_iter()
        .zip(smooth)
        .map(|(roc, smooth)| roc.saturating_add(smooth.saturating_sub(1)))
        .max()
        .unwrap_or(0)
}

fn kst_at(closes: &[f64], row: usize, roc: [usize; 4], smooth: [usize; 4]) -> f64 {
    let mut line = 0.0;
    for (component, (lag, period)) in roc.into_iter().zip(smooth).enumerate() {
        // Every ROC sample depends on the entire lag interval, even when
        // smoothing is shorter than the lag and no endpoint lands on a gap.
        if !closes[row + 1 - period - lag..=row]
            .iter()
            .all(|close| close.is_finite())
        {
            return f64::NAN;
        }
        let mut sum = 0.0;
        for index in row + 1 - period..=row {
            let current = closes[index];
            let previous = closes[index - lag];
            if previous == 0.0 {
                return f64::NAN;
            }
            sum += 100.0 * (current / previous - 1.0);
        }
        line += (component + 1) as f64 * sum / period as f64;
    }
    line
}

#[derive(Clone, Copy, Debug, Default)]
struct TsiState {
    previous: Option<f64>,
    momentum_long: EmaState,
    momentum_short: EmaState,
    absolute_long: EmaState,
    absolute_short: EmaState,
    signal: EmaState,
}

#[derive(Clone, Debug, Default)]
struct KamaState {
    value: f64,
    recent: VecDeque<f64>,
}

fn kama_step(
    state: &mut KamaState,
    values: &[f64],
    row: usize,
    period: usize,
    fast: usize,
    slow: usize,
) -> Option<f64> {
    let close = values[row];
    if !close.is_finite() {
        return None;
    }
    state.recent.push_back(close);
    if state.recent.len() < period {
        return None;
    }
    if state.recent.len() == period {
        state.value = state.recent.iter().sum::<f64>() / period as f64;
    } else {
        let change = (close - state.recent[0]).abs();
        let volatility = state
            .recent
            .iter()
            .zip(state.recent.iter().skip(1))
            .map(|(a, b)| (b - a).abs())
            .sum::<f64>();
        let efficiency = if volatility == 0.0 {
            0.0
        } else {
            change / volatility
        };
        let smoothing = (efficiency * (2.0 / (fast as f64 + 1.0) - 2.0 / (slow as f64 + 1.0))
            + 2.0 / (slow as f64 + 1.0))
            .powi(2);
        state.value += smoothing * (close - state.value);
        state.recent.pop_front();
    }
    Some(state.value)
}

#[derive(Clone, Copy, Debug, Default)]
struct McGinleyState {
    value: Option<f64>,
}

fn mcginley_step(state: &mut McGinleyState, close: f64, period: usize) -> Option<f64> {
    if !close.is_finite() {
        return None;
    }
    if close <= 0.0 {
        state.value = None;
        return None;
    }
    let value = match state.value {
        Some(previous) => {
            let denominator = period as f64 * (close / previous).powi(4);
            previous + (close - previous) / denominator
        }
        None => close,
    };
    state.value = value.is_finite().then_some(value);
    state.value
}

#[derive(Clone, Copy, Debug, Default)]
struct KlingerState {
    previous_sum: Option<f64>,
    previous_range: f64,
    trend: f64,
    cumulative: f64,
    fast: EmaState,
    slow: EmaState,
    signal: EmaState,
}

fn klinger_step(
    state: &mut KlingerState,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
    periods: [usize; 3],
) -> (Option<f64>, Option<f64>) {
    let range = high - low;
    let sum = high + low + close;
    if !sum.is_finite() || !range.is_finite() || range < 0.0 {
        return (None, None);
    }
    if !volume.is_finite() || volume < 0.0 {
        *state = KlingerState::default();
        return (None, None);
    }
    let trend = state
        .previous_sum
        .map_or(1.0, |previous| if sum > previous { 1.0 } else { -1.0 });
    state.cumulative = if state.previous_sum.is_some() && trend == state.trend {
        state.cumulative + range
    } else {
        state.previous_range + range
    };
    state.previous_range = range;
    state.previous_sum = Some(sum);
    state.trend = trend;
    let force = if state.cumulative == 0.0 {
        0.0
    } else {
        volume * trend * 100.0 * (2.0 * range / state.cumulative - 1.0).abs()
    };
    let first = ema_step(&mut state.fast, force, periods[0]);
    let second = ema_step(&mut state.slow, force, periods[1]);
    let line = first.zip(second).map(|(first, second)| first - second);
    let signal_value = line.and_then(|line| ema_step(&mut state.signal, line, periods[2]));
    (line, signal_value)
}

fn tsi_step(
    state: &mut TsiState,
    close: f64,
    long: usize,
    short: usize,
    signal_period: usize,
) -> (Option<f64>, Option<f64>) {
    if !close.is_finite() {
        return (None, None);
    }
    let previous = state.previous.replace(close);
    let Some(previous) = previous else {
        return (None, None);
    };
    let momentum = close - previous;
    if !momentum.is_finite() {
        *state = TsiState::default();
        return (None, None);
    }
    let numerator = ema_step(&mut state.momentum_long, momentum, long)
        .and_then(|sample| ema_step(&mut state.momentum_short, sample, short));
    let denominator = ema_step(&mut state.absolute_long, momentum.abs(), long)
        .and_then(|sample| ema_step(&mut state.absolute_short, sample, short));
    let line = numerator.zip(denominator).map(|(numerator, denominator)| {
        if denominator == 0.0 {
            0.0
        } else {
            (100.0 * (numerator / denominator)).clamp(-100.0, 100.0)
        }
    });
    let signal = line.and_then(|line| ema_step(&mut state.signal, line, signal_period));
    (line, signal)
}

fn mass_start(ema_period: usize, sum_period: usize) -> usize {
    ema_period
        .saturating_sub(1)
        .saturating_mul(2)
        .saturating_add(sum_period - 1)
}

#[derive(Clone, Debug, Default)]
struct MassState {
    first: EmaState,
    second: EmaState,
    ratios: VecDeque<f64>,
}

fn mass_step(state: &mut MassState, high: f64, low: f64, period: usize) -> Option<f64> {
    let range = high - low;
    if !range.is_finite() || range < 0.0 {
        return None;
    }
    let first = ema_step(&mut state.first, range, period)?;
    let second = ema_step(&mut state.second, first, period)?;
    Some(if second == 0.0 {
        f64::NAN
    } else {
        first / second
    })
}

fn mass_index_step(
    state: &mut MassState,
    high: f64,
    low: f64,
    ema_period: usize,
    sum_period: usize,
) -> Option<f64> {
    if !valid_range(high, low) {
        return None;
    }
    if let Some(ratio) = mass_step(state, high, low, ema_period) {
        state.ratios.push_back(ratio);
        if state.ratios.len() > sum_period {
            state.ratios.pop_front();
        }
    }
    (state.ratios.len() == sum_period).then(|| state.ratios.iter().sum())
}
#[derive(Clone, Copy, Debug, Default)]
struct HistoricalVolatilityState {
    sum: f64,
    sum_squares: f64,
    invalid: usize,
}

fn log_return(closes: &[f64], row: usize) -> f64 {
    let previous = closes[row - 1];
    let current = closes[row];
    if previous <= 0.0 || current <= 0.0 {
        f64::NAN
    } else {
        current.ln() - previous.ln()
    }
}

fn historical_volatility_step(
    state: &mut HistoricalVolatilityState,
    closes: &[f64],
    row: usize,
    period: usize,
    annualization: f64,
) -> Option<f64> {
    if row == 0 {
        return None;
    }
    let incoming = log_return(closes, row);
    if incoming.is_finite() {
        state.sum += incoming;
        state.sum_squares += incoming * incoming;
    } else {
        state.invalid += 1;
    }
    if row > period {
        let outgoing = log_return(closes, row - period);
        if outgoing.is_finite() {
            state.sum -= outgoing;
            state.sum_squares -= outgoing * outgoing;
        } else {
            state.invalid -= 1;
        }
    }
    if row < period {
        return None;
    }
    if state.invalid > 0 {
        return Some(f64::NAN);
    }
    let count = period as f64;
    let numerator = state.sum_squares - state.sum * state.sum / count;
    // Sliding sums lose all significant digits on a constant-close plateau.
    // Recenter only those nearly constant windows using the bounded period;
    // this also prevents old rounding noise from leaking past a gap.
    let variance = if numerator.abs() <= (1.0 + state.sum_squares) * 1e-12 {
        let first = log_return(closes, row + 1 - period);
        let (sum, squares) = (row + 1 - period..=row).fold((0.0, 0.0), |(sum, squares), index| {
            let diff = log_return(closes, index) - first;
            (sum + diff, squares + diff * diff)
        });
        (squares - sum * sum / count) / (count - 1.0)
    } else {
        numerator / (count - 1.0)
    }
    .max(0.0);
    Some(variance.sqrt() * annualization.sqrt() * 100.0)
}

fn ease_of_movement_raw(
    highs: &[f64],
    lows: &[f64],
    volumes: &[f64],
    row: usize,
    divisor: f64,
) -> f64 {
    let volume = volumes.get(row).copied().unwrap_or(0.0).max(0.0);
    if volume == 0.0 {
        return f64::NAN;
    }
    let prior_midpoint = (highs[row - 1] + lows[row - 1]) * 0.5;
    let midpoint = (highs[row] + lows[row]) * 0.5;
    (midpoint - prior_midpoint) * (highs[row] - lows[row]) * divisor / volume
}

/// Chaikin money flow over a rolling window. Each bar contributes its close location value times
/// non-negative volume; zero-volume windows return zero rather than a fictitious flow signal.
pub fn cmf(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    volumes: &[f64],
    period: usize,
) -> Vec<Option<f64>> {
    let n = highs
        .len()
        .min(lows.len())
        .min(closes.len())
        .min(volumes.len());
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    for (row, slot) in out.iter_mut().enumerate().skip(period.saturating_sub(1)) {
        let start = row + 1 - period;
        if !(start..=row).all(|index| valid_bar(highs[index], lows[index], closes[index])) {
            *slot = Some(f64::NAN);
            continue;
        }
        let mut flow = 0.0;
        let mut volume = 0.0;
        for index in start..=row {
            let bar_volume = volumes[index].max(0.0);
            let range = highs[index] - lows[index];
            let location = if range != 0.0 {
                ((closes[index] - lows[index]) - (highs[index] - closes[index])) / range
            } else {
                0.0
            };
            flow += location * bar_volume;
            volume += bar_volume;
        }
        *slot = Some(if volume > 0.0 { flow / volume } else { 0.0 });
    }
    out
}

/// Money flow index over a rolling window, using typical price and non-negative volume. The first
/// comparable window begins after `period` direction observations; a flat or zero-flow window is
/// neutral at 50.
pub fn mfi(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    volumes: &[f64],
    period: usize,
) -> Vec<Option<f64>> {
    let n = highs
        .len()
        .min(lows.len())
        .min(closes.len())
        .min(volumes.len());
    let mut out = vec![None; n];
    if period == 0 || n <= period {
        return out;
    }
    let typical = |row: usize| (highs[row] + lows[row] + closes[row]) / 3.0;
    let mut previous_valid = vec![None; n];
    let mut last_valid = None;
    for (row, previous) in previous_valid.iter_mut().enumerate() {
        *previous = last_valid;
        if valid_bar(highs[row], lows[row], closes[row]) {
            last_valid = Some(typical(row));
        }
    }
    for (row, slot) in out.iter_mut().enumerate().skip(period) {
        let start = row + 1 - period;
        if !(start..=row).all(|index| valid_bar(highs[index], lows[index], closes[index]))
            || previous_valid[start].is_none()
        {
            *slot = Some(f64::NAN);
            continue;
        }
        let mut positive = 0.0;
        let mut negative = 0.0;
        for (index, &bar_volume) in volumes.iter().enumerate().take(row + 1).skip(start) {
            let previous = previous_valid[index].expect("valid MFI window reference");
            let flow = typical(index) * bar_volume.max(0.0);
            if typical(index) > previous {
                positive += flow;
            } else if typical(index) < previous {
                negative += flow;
            }
        }
        *slot = Some(if negative == 0.0 {
            if positive == 0.0 { 50.0 } else { 100.0 }
        } else {
            100.0 - 100.0 / (1.0 + positive / negative)
        });
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

#[derive(Clone, Debug)]
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

impl<T: Clone + Default> RecursiveHistory<T> {
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
        if n == self.len
            && from + 1 == n
            && let Some(state) = self.before_tail.as_ref()
        {
            if self
                .checkpoints
                .last()
                .is_some_and(|checkpoint| checkpoint.row >= from)
            {
                Arc::make_mut(&mut self.checkpoints).retain(|checkpoint| checkpoint.row < from);
            }
            return (from, state.clone());
        }
        if n >= self.len
            && from == self.len
            && let Some(state) = self.tail.as_ref()
        {
            return (from, state.clone());
        }
        let checkpoint = self
            .checkpoints
            .iter()
            .rposition(|checkpoint| checkpoint.row < from);
        if let Some(position) = checkpoint {
            let checkpoint = &self.checkpoints[position];
            let row = checkpoint.row;
            let saved = checkpoint.state.clone();
            if position + 1 < self.checkpoints.len() {
                Arc::make_mut(&mut self.checkpoints).truncate(position + 1);
            }
            (row + 1, saved)
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

    fn bytes_with(&self, extra: impl Fn(&T) -> usize) -> usize {
        self.bytes()
            + self
                .checkpoints
                .iter()
                .map(|checkpoint| extra(&checkpoint.state))
                .sum::<usize>()
            + self.tail.as_ref().map_or(0, &extra)
            + self.before_tail.as_ref().map_or(0, extra)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct EmaState {
    seen: usize,
    seed_sum: f64,
    value: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct DemaState {
    first: EmaState,
    second: EmaState,
}

#[derive(Clone, Copy, Debug, Default)]
struct TemaState {
    first: EmaState,
    second: EmaState,
    third: EmaState,
}

#[derive(Clone, Copy, Debug, Default)]
struct SmmaState {
    seen: usize,
    seed_sum: f64,
    value: f64,
}

fn dema_step(state: &mut DemaState, sample: f64, period: usize) -> Option<f64> {
    let first = ema_step(&mut state.first, sample, period)?;
    let second = ema_step(&mut state.second, first, period)?;
    Some(2.0 * first - second)
}

fn tema_step(state: &mut TemaState, sample: f64, period: usize) -> Option<f64> {
    let first = ema_step(&mut state.first, sample, period)?;
    let second = ema_step(&mut state.second, first, period)?;
    let third = ema_step(&mut state.third, second, period)?;
    Some(3.0 * first - 3.0 * second + third)
}

fn smma_step(state: &mut SmmaState, sample: f64, period: usize) -> Option<f64> {
    if !sample.is_finite() {
        return None;
    }
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
        state.value = (state.value * (period as f64 - 1.0) + sample) / period as f64;
        Some(state.value)
    }
}

fn ema_step(state: &mut EmaState, sample: f64, period: usize) -> Option<f64> {
    if !sample.is_finite() {
        return None;
    }
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
struct KeltnerState {
    middle: EmaState,
    atr: AtrState,
}

#[derive(Clone, Copy, Debug, Default)]
struct AdxDmiState {
    previous_high: Option<f64>,
    previous_low: Option<f64>,
    previous_close: Option<f64>,
    seen: usize,
    tr_sum: f64,
    plus_sum: f64,
    minus_sum: f64,
    tr_value: f64,
    plus_value: f64,
    minus_value: f64,
    dx_seen: usize,
    dx_sum: f64,
    adx: f64,
}

#[derive(Clone, Copy, Debug)]
struct ParabolicSarState {
    initialized: bool,
    rising: bool,
    sar: f64,
    extreme: f64,
    acceleration: f64,
    previous_high: f64,
    previous_low: f64,
    before_previous_high: Option<f64>,
    before_previous_low: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default)]
struct SuperTrendState {
    atr: AtrState,
    previous_close: Option<f64>,
    final_upper: f64,
    final_lower: f64,
    trend_up: bool,
    initialized: bool,
}

impl Default for ParabolicSarState {
    fn default() -> Self {
        Self {
            initialized: false,
            rising: true,
            sar: 0.0,
            extreme: 0.0,
            acceleration: 0.02,
            previous_high: 0.0,
            previous_low: 0.0,
            before_previous_high: None,
            before_previous_low: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct MacdState {
    fast: EmaState,
    slow: EmaState,
    signal: EmaState,
}

#[derive(Clone, Copy)]
struct DirectionalSample {
    high: f64,
    low: f64,
    close: f64,
}

fn indexed_rsi_step(state: &mut IndexedRsiState, sample: f64, period: usize) -> Option<f64> {
    if !sample.is_finite() {
        return None;
    }
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

fn volume_oscillator_step(
    state: &mut MacdState,
    volume: f64,
    fast_period: usize,
    slow_period: usize,
    signal_period: usize,
) -> VolumeOscillatorPoint {
    if !volume.is_finite() {
        return VolumeOscillatorPoint {
            line: None,
            signal: None,
            histogram: None,
        };
    }
    let sample = volume.max(0.0);
    let fast = ema_step(&mut state.fast, sample, fast_period);
    let slow = ema_step(&mut state.slow, sample, slow_period);
    let line = fast.zip(slow).map(|(fast, slow)| {
        if slow == 0.0 {
            0.0
        } else {
            (fast - slow) / slow * 100.0
        }
    });
    let signal = line.and_then(|line| ema_step(&mut state.signal, line, signal_period));
    VolumeOscillatorPoint {
        line,
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
struct CumulativeCloseVolumeState {
    cumulative: f64,
    previous_close: f64,
    initialized: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct ChaikinState {
    cumulative: f64,
    fast: EmaState,
    slow: EmaState,
}

#[derive(Clone, Copy, Debug, Default)]
struct ElderForceState {
    previous_close: Option<f64>,
    ema: EmaState,
}

// A missing source price is not an observation. Volume whitespace has a separate policy.
fn valid_range(high: f64, low: f64) -> bool {
    high.is_finite() && low.is_finite()
}

fn valid_bar(high: f64, low: f64, close: f64) -> bool {
    valid_range(high, low) && close.is_finite()
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
    if !valid_bar(sample.high, sample.low, sample.close) {
        return None;
    }
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

fn keltner_step(
    state: &mut KeltnerState,
    sample: AtrSample,
    period: usize,
    multiplier: f64,
) -> KeltnerPoint {
    if !valid_bar(sample.high, sample.low, sample.close) {
        return KeltnerPoint {
            upper: None,
            middle: None,
            lower: None,
        };
    }
    let middle = ema_step(&mut state.middle, sample.close, period);
    let range = atr_step(&mut state.atr, sample, period);
    match (middle, range) {
        (Some(middle), Some(range)) => {
            let spread = range * multiplier.max(0.0);
            KeltnerPoint {
                upper: Some(middle + spread),
                middle: Some(middle),
                lower: Some(middle - spread),
            }
        }
        _ => KeltnerPoint {
            upper: None,
            middle: None,
            lower: None,
        },
    }
}

fn adx_dmi_step(state: &mut AdxDmiState, sample: DirectionalSample, period: usize) -> AdxDmiPoint {
    if !valid_bar(sample.high, sample.low, sample.close) {
        return AdxDmiPoint {
            plus_di: None,
            minus_di: None,
            adx: None,
        };
    }
    let Some(previous_high) = state.previous_high.replace(sample.high) else {
        state.previous_low = Some(sample.low);
        state.previous_close = Some(sample.close);
        return AdxDmiPoint {
            plus_di: None,
            minus_di: None,
            adx: None,
        };
    };
    let previous_low = state.previous_low.replace(sample.low).unwrap_or(sample.low);
    let previous_close = state
        .previous_close
        .replace(sample.close)
        .unwrap_or(sample.close);
    let up_move = sample.high - previous_high;
    let down_move = previous_low - sample.low;
    let plus = if up_move > down_move && up_move > 0.0 {
        up_move
    } else {
        0.0
    };
    let minus = if down_move > up_move && down_move > 0.0 {
        down_move
    } else {
        0.0
    };
    let true_range = (sample.high - sample.low)
        .max((sample.high - previous_close).abs())
        .max((sample.low - previous_close).abs());
    state.seen += 1;
    if state.seen <= period {
        state.tr_sum += true_range;
        state.plus_sum += plus;
        state.minus_sum += minus;
        if state.seen < period {
            return AdxDmiPoint {
                plus_di: None,
                minus_di: None,
                adx: None,
            };
        }
        state.tr_value = state.tr_sum / period as f64;
        state.plus_value = state.plus_sum / period as f64;
        state.minus_value = state.minus_sum / period as f64;
    } else {
        state.tr_value = (state.tr_value * (period as f64 - 1.0) + true_range) / period as f64;
        state.plus_value = (state.plus_value * (period as f64 - 1.0) + plus) / period as f64;
        state.minus_value = (state.minus_value * (period as f64 - 1.0) + minus) / period as f64;
    }
    let (plus_di, minus_di, dx) = if state.tr_value > 0.0 {
        let plus_di = 100.0 * state.plus_value / state.tr_value;
        let minus_di = 100.0 * state.minus_value / state.tr_value;
        let denominator = plus_di + minus_di;
        let dx = if denominator > 0.0 {
            100.0 * (plus_di - minus_di).abs() / denominator
        } else {
            0.0
        };
        (plus_di, minus_di, dx)
    } else {
        (0.0, 0.0, 0.0)
    };
    state.dx_seen += 1;
    let adx = if state.dx_seen <= period {
        state.dx_sum += dx;
        (state.dx_seen == period).then(|| {
            state.adx = state.dx_sum / period as f64;
            state.adx
        })
    } else {
        state.adx = (state.adx * (period as f64 - 1.0) + dx) / period as f64;
        Some(state.adx)
    };
    AdxDmiPoint {
        plus_di: Some(plus_di),
        minus_di: Some(minus_di),
        adx,
    }
}

fn parabolic_sar_step(
    state: &mut ParabolicSarState,
    high: f64,
    low: f64,
    step: f64,
    max_step: f64,
) -> f64 {
    if !state.initialized {
        state.initialized = true;
        state.previous_high = high;
        state.previous_low = low;
        state.sar = low;
        state.extreme = high;
        state.acceleration = step;
        return state.sar;
    }
    let mut candidate = state.sar + state.acceleration * (state.extreme - state.sar);
    if state.rising {
        candidate = candidate.min(state.previous_low);
        if let Some(before) = state.before_previous_low {
            candidate = candidate.min(before);
        }
        if low < candidate {
            state.rising = false;
            candidate = state.extreme;
            state.extreme = low;
            state.acceleration = step;
        } else if high > state.extreme {
            state.extreme = high;
            state.acceleration = (state.acceleration + step).min(max_step);
        }
    } else {
        candidate = candidate.max(state.previous_high);
        if let Some(before) = state.before_previous_high {
            candidate = candidate.max(before);
        }
        if high > candidate {
            state.rising = true;
            candidate = state.extreme;
            state.extreme = high;
            state.acceleration = step;
        } else if low < state.extreme {
            state.extreme = low;
            state.acceleration = (state.acceleration + step).min(max_step);
        }
    }
    state.before_previous_high = Some(state.previous_high);
    state.before_previous_low = Some(state.previous_low);
    state.previous_high = high;
    state.previous_low = low;
    state.sar = candidate;
    candidate
}

fn supertrend_step(
    state: &mut SuperTrendState,
    sample: DirectionalSample,
    period: usize,
    multiplier: f64,
) -> Option<f64> {
    if !valid_bar(sample.high, sample.low, sample.close) {
        return None;
    }
    let atr = atr_step(
        &mut state.atr,
        AtrSample {
            high: sample.high,
            low: sample.low,
            close: sample.close,
        },
        period,
    );
    let previous_close = state.previous_close.replace(sample.close);
    let atr = atr?;
    let midpoint = (sample.high + sample.low) * 0.5;
    let spread = atr * multiplier.max(0.0);
    let basic_upper = midpoint + spread;
    let basic_lower = midpoint - spread;
    if !state.initialized {
        state.initialized = true;
        state.trend_up = true;
        state.final_upper = basic_upper;
        state.final_lower = basic_lower;
        return Some(basic_lower);
    }
    let previous_close = previous_close.unwrap_or(sample.close);
    if basic_upper < state.final_upper || previous_close > state.final_upper {
        state.final_upper = basic_upper;
    }
    if basic_lower > state.final_lower || previous_close < state.final_lower {
        state.final_lower = basic_lower;
    }
    if state.trend_up {
        if sample.close < state.final_lower {
            state.trend_up = false;
            Some(state.final_upper)
        } else {
            Some(state.final_lower)
        }
    } else if sample.close > state.final_upper {
        state.trend_up = true;
        Some(state.final_lower)
    } else {
        Some(state.final_upper)
    }
}

fn rolling_midpoint(highs: &[f64], lows: &[f64], row: usize, period: usize) -> Option<f64> {
    if row + 1 < period {
        return None;
    }
    let start = row + 1 - period;
    if !(start..=row).all(|index| valid_range(highs[index], lows[index])) {
        return Some(f64::NAN);
    }
    let high = highs[start..=row]
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let low = lows[start..=row]
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    Some((high + low) * 0.5)
}

fn vwap_step(state: &mut VwapState, sample: VwapSample) -> f64 {
    if !valid_bar(sample.high, sample.low, sample.close) {
        return f64::NAN;
    }
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

fn obv_step(state: &mut CumulativeCloseVolumeState, close: f64, volume: f64) -> f64 {
    if !close.is_finite() {
        return f64::NAN;
    }
    if !state.initialized {
        state.previous_close = close;
        state.initialized = true;
        return state.cumulative;
    }
    let volume = volume.max(0.0);
    if close > state.previous_close {
        state.cumulative += volume;
    } else if close < state.previous_close {
        state.cumulative -= volume;
    }
    state.previous_close = close;
    state.cumulative
}

fn accumulation_distribution_step(
    cumulative: &mut f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
) -> f64 {
    if !valid_bar(high, low, close) {
        return f64::NAN;
    }
    let range = high - low;
    if range > 0.0 {
        *cumulative += ((close - low) - (high - close)) / range * volume.max(0.0);
    }
    *cumulative
}

fn price_volume_trend_step(state: &mut CumulativeCloseVolumeState, close: f64, volume: f64) -> f64 {
    if !close.is_finite() {
        return f64::NAN;
    }
    if state.initialized && state.previous_close != 0.0 {
        state.cumulative += (close - state.previous_close) / state.previous_close * volume.max(0.0);
    }
    state.previous_close = close;
    state.initialized = true;
    state.cumulative
}

fn vwap_bands_step(
    state: &mut VwapBandsState,
    sample: VwapBandsSample,
    reset: VwapReset,
    standard_deviation: f64,
    percent: f64,
) -> VwapBandsPoint {
    if !valid_bar(sample.high, sample.low, sample.close) {
        return VwapBandsPoint {
            basis: None,
            standard_upper: None,
            standard_lower: None,
            percent_upper: None,
            percent_lower: None,
        };
    }
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
    Aroon {
        period: usize,
        high_deque: VecDeque<usize>,
        low_deque: VecDeque<usize>,
    },
    AwesomeOscillator,
    Dpo {
        period: usize,
    },
    ChandeMomentum {
        period: usize,
    },
    Sma {
        period: usize,
    },
    Ema {
        period: usize,
        state: RecursiveHistory<EmaState>,
    },
    Dema {
        period: usize,
        state: RecursiveHistory<DemaState>,
    },
    Tema {
        period: usize,
        state: RecursiveHistory<TemaState>,
    },
    Smma {
        period: usize,
        state: RecursiveHistory<SmmaState>,
    },
    Hma {
        period: usize,
    },
    Vwma {
        period: usize,
    },
    StandardDeviation {
        period: usize,
    },
    Cci {
        period: usize,
    },
    WilliamsR {
        period: usize,
    },
    StochasticRsi {
        rsi_period: usize,
        stochastic_period: usize,
    },
    Momentum {
        period: usize,
    },
    RateOfChange {
        period: usize,
    },
    Donchian {
        period: usize,
    },
    PivotPoints {
        kind: PivotKind,
    },
    ZigZag {
        deviation_percent: f64,
    },
    Keltner {
        period: usize,
        multiplier: f64,
        state: RecursiveHistory<KeltnerState>,
    },
    AdxDmi {
        period: usize,
        state: RecursiveHistory<AdxDmiState>,
    },
    ParabolicSar {
        state: RecursiveHistory<ParabolicSarState>,
    },
    SuperTrend {
        period: usize,
        multiplier: f64,
        state: RecursiveHistory<SuperTrendState>,
    },
    Ichimoku,
    EmaRibbon {
        periods: [usize; MAX_OUTPUTS],
        states: Box<[RecursiveHistory<EmaState>; MAX_OUTPUTS]>,
    },
    Bollinger {
        period: usize,
        deviation: f64,
    },
    BollingerMetrics {
        period: usize,
        deviation: f64,
    },
    Envelopes {
        period: usize,
        percent: f64,
        ema_state: Option<RecursiveHistory<EmaState>>,
    },
    Alma {
        period: usize,
        weights: Vec<f64>,
        weight_sum: f64,
    },
    Rsi {
        period: usize,
        state: RecursiveHistory<IndexedRsiState>,
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
        state: RecursiveHistory<StochasticCarry>,
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
    Obv {
        state: RecursiveHistory<CumulativeCloseVolumeState>,
    },
    AccumulationDistribution {
        state: RecursiveHistory<f64>,
    },
    PriceVolumeTrend {
        state: RecursiveHistory<CumulativeCloseVolumeState>,
    },
    ChaikinOscillator {
        fast: usize,
        slow: usize,
        state: RecursiveHistory<ChaikinState>,
    },
    RelativeVolume {
        period: usize,
    },
    VolumeOscillator {
        fast_period: usize,
        slow_period: usize,
        signal_period: usize,
        state: RecursiveHistory<MacdState>,
    },
    ElderForce {
        period: usize,
        state: RecursiveHistory<ElderForceState>,
    },
    EaseOfMovement {
        period: usize,
        divisor: f64,
    },
    HistoricalVolatility {
        period: usize,
        annualization: f64,
        state: RecursiveHistory<HistoricalVolatilityState>,
    },
    Trix {
        period: usize,
        signal_period: usize,
        state: RecursiveHistory<TrixState>,
    },
    Kst {
        roc_periods: [usize; 4],
        sma_periods: [usize; 4],
        signal_period: usize,
    },
    Tsi {
        long: usize,
        short: usize,
        signal_period: usize,
        state: RecursiveHistory<TsiState>,
    },
    MassIndex {
        ema_period: usize,
        sum_period: usize,
        state: RecursiveHistory<MassState>,
    },
    Klinger {
        fast: usize,
        slow: usize,
        signal: usize,
        state: RecursiveHistory<KlingerState>,
    },
    Kama {
        period: usize,
        fast: usize,
        slow: usize,
        state: RecursiveHistory<KamaState>,
    },
    McGinley {
        period: usize,
        state: RecursiveHistory<McGinleyState>,
    },
    LinearRegression {
        period: usize,
        deviation: f64,
    },
    Choppiness {
        period: usize,
    },
    AtrBands {
        period: usize,
        multiplier: f64,
        state: RecursiveHistory<AtrState>,
    },
    CoppockCurve {
        long_period: usize,
        short_period: usize,
        smoothing: usize,
    },
    FisherTransform {
        period: usize,
        state: RecursiveHistory<FisherState>,
        window: FisherWindow,
    },
    UltimateOscillator {
        short: usize,
        medium: usize,
        long: usize,
    },
    Vortex {
        period: usize,
    },
    Cmf {
        period: usize,
    },
    Mfi {
        period: usize,
    },
    Volume {
        period: usize,
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

    pub fn aroon(period: usize) -> Self {
        Self::new(
            IncrementalKind::Aroon {
                period,
                high_deque: VecDeque::new(),
                low_deque: VecDeque::new(),
            },
            2,
        )
    }

    pub fn awesome_oscillator() -> Self {
        Self::new(IncrementalKind::AwesomeOscillator, 1)
    }

    pub fn dpo(period: usize) -> Self {
        Self::new(IncrementalKind::Dpo { period }, 1)
    }

    pub fn chande_momentum(period: usize) -> Self {
        Self::new(IncrementalKind::ChandeMomentum { period }, 1)
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

    pub fn dema(period: usize) -> Self {
        Self::new(
            IncrementalKind::Dema {
                period,
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn tema(period: usize) -> Self {
        Self::new(
            IncrementalKind::Tema {
                period,
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn smma(period: usize) -> Self {
        Self::new(
            IncrementalKind::Smma {
                period,
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn hma(period: usize) -> Self {
        Self::new(IncrementalKind::Hma { period }, 1)
    }

    pub fn vwma(period: usize) -> Self {
        Self::new(IncrementalKind::Vwma { period }, 1)
    }

    pub fn standard_deviation(period: usize) -> Self {
        Self::new(IncrementalKind::StandardDeviation { period }, 1)
    }

    pub fn cci(period: usize) -> Self {
        Self::new(IncrementalKind::Cci { period }, 1)
    }

    pub fn williams_r(period: usize) -> Self {
        Self::new(IncrementalKind::WilliamsR { period }, 1)
    }

    pub fn stochastic_rsi(rsi_period: usize, stochastic_period: usize) -> Self {
        Self::new(
            IncrementalKind::StochasticRsi {
                rsi_period,
                stochastic_period,
            },
            1,
        )
    }

    pub fn momentum(period: usize) -> Self {
        Self::new(IncrementalKind::Momentum { period }, 1)
    }

    pub fn rate_of_change(period: usize) -> Self {
        Self::new(IncrementalKind::RateOfChange { period }, 1)
    }

    pub fn donchian(period: usize) -> Self {
        Self::new(IncrementalKind::Donchian { period }, 3)
    }

    pub fn pivot_points(kind: PivotKind) -> Self {
        Self::new(IncrementalKind::PivotPoints { kind }, 5)
    }

    pub fn zigzag(deviation_percent: f64) -> Self {
        Self::new(IncrementalKind::ZigZag { deviation_percent }, 1)
    }

    pub fn keltner(period: usize, multiplier: f64) -> Self {
        Self::new(
            IncrementalKind::Keltner {
                period,
                multiplier,
                state: RecursiveHistory::new(),
            },
            3,
        )
    }

    pub fn adx_dmi(period: usize) -> Self {
        Self::new(
            IncrementalKind::AdxDmi {
                period,
                state: RecursiveHistory::new(),
            },
            3,
        )
    }

    pub fn parabolic_sar() -> Self {
        Self::new(
            IncrementalKind::ParabolicSar {
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn supertrend(period: usize, multiplier: f64) -> Self {
        Self::new(
            IncrementalKind::SuperTrend {
                period,
                multiplier,
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn ichimoku() -> Self {
        Self::new(IncrementalKind::Ichimoku, 5)
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

    pub fn bollinger_metrics(period: usize, deviation: f64) -> Self {
        Self::new(IncrementalKind::BollingerMetrics { period, deviation }, 2)
    }

    pub fn envelopes(period: usize, percent: f64, exponential: bool) -> Self {
        Self::new(
            IncrementalKind::Envelopes {
                period,
                percent,
                ema_state: exponential.then(RecursiveHistory::new),
            },
            3,
        )
    }

    pub fn alma(period: usize, offset: f64, sigma: f64) -> Self {
        let (weights, weight_sum) = alma_weights(period, offset, sigma);
        Self::new(
            IncrementalKind::Alma {
                period,
                weights,
                weight_sum,
            },
            1,
        )
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

    pub fn obv() -> Self {
        Self::new(
            IncrementalKind::Obv {
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn accumulation_distribution() -> Self {
        Self::new(
            IncrementalKind::AccumulationDistribution {
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn price_volume_trend() -> Self {
        Self::new(
            IncrementalKind::PriceVolumeTrend {
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn chaikin_oscillator(fast: usize, slow: usize) -> Self {
        Self::new(
            IncrementalKind::ChaikinOscillator {
                fast,
                slow,
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn relative_volume(period: usize) -> Self {
        Self::new(IncrementalKind::RelativeVolume { period }, 1)
    }

    pub fn volume_oscillator(fast_period: usize, slow_period: usize, signal_period: usize) -> Self {
        Self::new(
            IncrementalKind::VolumeOscillator {
                fast_period,
                slow_period,
                signal_period,
                state: RecursiveHistory::new(),
            },
            3,
        )
    }

    pub fn elder_force(period: usize) -> Self {
        Self::new(
            IncrementalKind::ElderForce {
                period,
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn ease_of_movement(period: usize, divisor: f64) -> Self {
        Self::new(IncrementalKind::EaseOfMovement { period, divisor }, 1)
    }

    pub fn historical_volatility(period: usize, annualization: f64) -> Self {
        Self::new(
            IncrementalKind::HistoricalVolatility {
                period,
                annualization,
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn trix(period: usize, signal_period: usize) -> Self {
        Self::new(
            IncrementalKind::Trix {
                period,
                signal_period,
                state: RecursiveHistory::new(),
            },
            2,
        )
    }

    pub fn kst(roc_periods: [usize; 4], sma_periods: [usize; 4], signal_period: usize) -> Self {
        Self::new(
            IncrementalKind::Kst {
                roc_periods,
                sma_periods,
                signal_period,
            },
            2,
        )
    }

    pub fn tsi(long: usize, short: usize, signal_period: usize) -> Self {
        Self::new(
            IncrementalKind::Tsi {
                long,
                short,
                signal_period,
                state: RecursiveHistory::new(),
            },
            2,
        )
    }

    pub fn mass_index(ema_period: usize, sum_period: usize) -> Self {
        Self::new(
            IncrementalKind::MassIndex {
                ema_period,
                sum_period,
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn klinger(fast: usize, slow: usize, signal: usize) -> Self {
        Self::new(
            IncrementalKind::Klinger {
                fast,
                slow,
                signal,
                state: RecursiveHistory::new(),
            },
            2,
        )
    }

    pub fn kama(period: usize, fast: usize, slow: usize) -> Self {
        Self::new(
            IncrementalKind::Kama {
                period,
                fast,
                slow,
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn mcginley(period: usize) -> Self {
        Self::new(
            IncrementalKind::McGinley {
                period,
                state: RecursiveHistory::new(),
            },
            1,
        )
    }

    pub fn linear_regression(period: usize, deviation: f64) -> Self {
        Self::new(IncrementalKind::LinearRegression { period, deviation }, 3)
    }

    pub fn choppiness(period: usize) -> Self {
        Self::new(IncrementalKind::Choppiness { period }, 1)
    }

    pub fn atr_bands(period: usize, multiplier: f64) -> Self {
        Self::new(
            IncrementalKind::AtrBands {
                period,
                multiplier,
                state: RecursiveHistory::new(),
            },
            3,
        )
    }

    pub fn coppock_curve(long_period: usize, short_period: usize, smoothing: usize) -> Self {
        Self::new(
            IncrementalKind::CoppockCurve {
                long_period,
                short_period,
                smoothing,
            },
            1,
        )
    }

    pub fn fisher_transform(period: usize) -> Self {
        Self::new(
            IncrementalKind::FisherTransform {
                period,
                state: RecursiveHistory::new(),
                window: FisherWindow::default(),
            },
            2,
        )
    }

    pub fn ultimate_oscillator(short: usize, medium: usize, long: usize) -> Self {
        Self::new(
            IncrementalKind::UltimateOscillator {
                short,
                medium,
                long,
            },
            1,
        )
    }

    pub fn vortex(period: usize) -> Self {
        Self::new(IncrementalKind::Vortex { period }, 2)
    }

    pub fn cmf(period: usize) -> Self {
        Self::new(IncrementalKind::Cmf { period }, 1)
    }

    pub fn mfi(period: usize) -> Self {
        Self::new(IncrementalKind::Mfi { period }, 1)
    }

    pub fn volume(period: usize) -> Self {
        Self::new(IncrementalKind::Volume { period }, 2)
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
            IncrementalKind::Aroon {
                high_deque,
                low_deque,
                ..
            } => (high_deque.capacity() + low_deque.capacity()) * std::mem::size_of::<usize>(),
            IncrementalKind::AwesomeOscillator => 0,
            IncrementalKind::Dpo { .. } => 0,
            IncrementalKind::ChandeMomentum { .. } => 0,
            IncrementalKind::Envelopes { ema_state, .. } => {
                ema_state.as_ref().map_or(0, RecursiveHistory::bytes)
            }
            IncrementalKind::Alma { weights, .. } => {
                weights.capacity() * std::mem::size_of::<f64>()
            }
            IncrementalKind::Ema { state, .. } => state.bytes(),
            IncrementalKind::Dema { state, .. } => state.bytes(),
            IncrementalKind::Tema { state, .. } => state.bytes(),
            IncrementalKind::Smma { state, .. } => state.bytes(),
            IncrementalKind::EmaRibbon { states, .. } => {
                states.iter().map(RecursiveHistory::bytes).sum()
            }
            IncrementalKind::Rsi { state, .. } => state.bytes(),
            IncrementalKind::Macd { state, .. } => state.bytes(),
            IncrementalKind::Stochastic { state, tail_k, .. } => {
                state.bytes_with(StochasticCarry::bytes)
                    + tail_k.capacity() * std::mem::size_of::<f64>()
            }
            IncrementalKind::Atr { state, .. } => state.bytes(),
            IncrementalKind::Keltner { state, .. } => state.bytes(),
            IncrementalKind::AdxDmi { state, .. } => state.bytes(),
            IncrementalKind::ParabolicSar { state } => state.bytes(),
            IncrementalKind::SuperTrend { state, .. } => state.bytes(),
            IncrementalKind::Ichimoku => 0,
            IncrementalKind::Vwap { state } => state.bytes(),
            IncrementalKind::Obv { state } => state.bytes(),
            IncrementalKind::AccumulationDistribution { state } => state.bytes(),
            IncrementalKind::PriceVolumeTrend { state } => state.bytes(),
            IncrementalKind::ChaikinOscillator { state, .. } => state.bytes(),
            IncrementalKind::RelativeVolume { .. } => 0,
            IncrementalKind::VolumeOscillator { state, .. } => state.bytes(),
            IncrementalKind::ElderForce { state, .. } => state.bytes(),
            IncrementalKind::EaseOfMovement { .. } => 0,
            IncrementalKind::HistoricalVolatility { state, .. } => state.bytes(),
            IncrementalKind::Trix { state, .. } => state.bytes(),
            IncrementalKind::Kst { .. } => 0,
            IncrementalKind::Tsi { state, .. } => state.bytes(),
            IncrementalKind::MassIndex { state, .. } => {
                state.bytes_with(|saved| saved.ratios.capacity() * std::mem::size_of::<f64>())
            }
            IncrementalKind::Klinger { state, .. } => state.bytes(),
            IncrementalKind::Kama { state, .. } => {
                state.bytes_with(|saved| saved.recent.capacity() * std::mem::size_of::<f64>())
            }
            IncrementalKind::McGinley { state, .. } => state.bytes(),
            IncrementalKind::LinearRegression { .. } => 0,
            IncrementalKind::Choppiness { .. } => 0,
            IncrementalKind::AtrBands { state, .. } => state.bytes(),
            IncrementalKind::CoppockCurve { .. } => 0,
            IncrementalKind::FisherTransform { state, window, .. } => {
                state.bytes() + window.bytes()
            }
            IncrementalKind::UltimateOscillator { .. } => 0,
            IncrementalKind::Vortex { .. } => 0,
            IncrementalKind::Cmf { .. } => 0,
            IncrementalKind::Mfi { .. } => 0,
            IncrementalKind::Volume { .. } => 0,
            IncrementalKind::VwapBands { state, .. } => state.bytes(),
            IncrementalKind::Sma { .. }
            | IncrementalKind::Bollinger { .. }
            | IncrementalKind::BollingerMetrics { .. }
            | IncrementalKind::Wma { .. }
            | IncrementalKind::Hma { .. } => 0,
            IncrementalKind::Vwma { .. } => 0,
            IncrementalKind::StandardDeviation { .. } => 0,
            IncrementalKind::Cci { .. } => 0,
            IncrementalKind::WilliamsR { .. } => 0,
            IncrementalKind::StochasticRsi { .. } => 0,
            IncrementalKind::Momentum { .. } | IncrementalKind::RateOfChange { .. } => 0,
            IncrementalKind::Donchian { .. } => 0,
            IncrementalKind::PivotPoints { .. } => 0,
            IncrementalKind::ZigZag { .. } => 0,
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
            IncrementalKind::Aroon {
                period,
                high_deque,
                low_deque,
            } => {
                let start = self.output_from[0];
                let repair = start.saturating_sub(*period);
                high_deque.clear();
                low_deque.clear();
                let mut invalid_count = 0_usize;
                self.last_work_rows = n - repair;
                for row in repair..n {
                    if row > repair + *period {
                        let expired = row - *period - 1;
                        invalid_count -= usize::from(
                            !input.high[expired].is_finite() || !input.low[expired].is_finite(),
                        );
                    }
                    let high = input.high[row];
                    let low = input.low[row];
                    if !high.is_finite() || !low.is_finite() {
                        invalid_count += 1;
                    } else {
                        while high_deque
                            .back()
                            .is_some_and(|&index| high >= input.high[index])
                        {
                            high_deque.pop_back();
                        }
                        while low_deque
                            .back()
                            .is_some_and(|&index| low <= input.low[index])
                        {
                            low_deque.pop_back();
                        }
                        high_deque.push_back(row);
                        low_deque.push_back(row);
                    }
                    while high_deque
                        .front()
                        .is_some_and(|&index| index + *period < row)
                    {
                        high_deque.pop_front();
                    }
                    while low_deque
                        .front()
                        .is_some_and(|&index| index + *period < row)
                    {
                        low_deque.pop_front();
                    }
                    if row >= start {
                        let value = |index: Option<usize>| {
                            if invalid_count != 0 {
                                f64::NAN
                            } else {
                                index.map_or(f64::NAN, |index| {
                                    (*period - (row - index)) as f64 * 100.0 / *period as f64
                                })
                            }
                        };
                        self.outputs[0].push(value(high_deque.front().copied()));
                        self.outputs[1].push(value(low_deque.front().copied()));
                    }
                }
            }
            IncrementalKind::Alma {
                period,
                weights,
                weight_sum,
            } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                for row in start..n {
                    let window = &input.close[row + 1 - *period..=row];
                    self.outputs[0].push(if *weight_sum == 0.0 {
                        f64::NAN
                    } else {
                        window
                            .iter()
                            .zip(weights.iter())
                            .map(|(value, weight)| value * weight)
                            .sum::<f64>()
                            / *weight_sum
                    });
                }
            }
            IncrementalKind::AwesomeOscillator => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                for row in start..n {
                    let median = |index| (input.high[index] + input.low[index]) / 2.0;
                    let short = (row - 4..=row).map(median).sum::<f64>() / 5.0;
                    let long = (row - 33..=row).map(median).sum::<f64>() / 34.0;
                    self.outputs[0].push(short - long);
                }
            }
            IncrementalKind::Dpo { period } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                let lag = *period / 2 + 1;
                for row in start..n {
                    let window = &input.close[row + 1 - *period..=row];
                    self.outputs[0].push(
                        if window.iter().all(|value| value.is_finite())
                            && input.close[row - lag].is_finite()
                        {
                            input.close[row - lag] - window.iter().sum::<f64>() / *period as f64
                        } else {
                            f64::NAN
                        },
                    );
                }
            }
            IncrementalKind::ChandeMomentum { period } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                for row in start..n {
                    let mut signed = 0.0;
                    let mut absolute = 0.0;
                    let mut valid = true;
                    // A fresh bounded window cannot retain roundoff from a
                    // movement that has already left it (especially when
                    // the current window is flat).
                    for pair in input.close[row - *period..=row].windows(2) {
                        if !pair[0].is_finite() || !pair[1].is_finite() {
                            valid = false;
                            break;
                        }
                        let change = pair[1] - pair[0];
                        signed += change;
                        absolute += change.abs();
                    }
                    self.outputs[0].push(if !valid {
                        f64::NAN
                    } else if absolute == 0.0 {
                        0.0
                    } else {
                        100.0 * signed / absolute
                    });
                }
            }
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
                        self.outputs[0].push(value.unwrap_or(f64::NAN));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::Dema { period, state } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let value = dema_step(&mut accumulator, input.close[row], *period);
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(value.unwrap_or(f64::NAN));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::Tema { period, state } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let value = tema_step(&mut accumulator, input.close[row], *period);
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(value.unwrap_or(f64::NAN));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::Smma { period, state } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let value = smma_step(&mut accumulator, input.close[row], *period);
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(value.unwrap_or(f64::NAN));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::Hma { period } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                for row in start..n {
                    self.outputs[0]
                        .push(hma_at(&input.close[..n], row, *period).expect("HMA after warmup"));
                }
            }
            IncrementalKind::Vwma { period } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                for row in start..n {
                    self.outputs[0].push(
                        vwma_at(input.close, input.volume, row, *period)
                            .expect("VWMA after warmup"),
                    );
                }
            }
            IncrementalKind::StandardDeviation { period } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                let values = standard_deviation(input.close, *period);
                self.outputs[0].extend(values.into_iter().skip(start).flatten());
            }
            IncrementalKind::Cci { period } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                let values = cci(input.high, input.low, input.close, *period);
                self.outputs[0].extend(values.into_iter().skip(start).flatten());
            }
            IncrementalKind::WilliamsR { period } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                let values = williams_r(input.high, input.low, input.close, *period);
                self.outputs[0].extend(values.into_iter().skip(start).flatten());
            }
            IncrementalKind::StochasticRsi {
                rsi_period,
                stochastic_period,
            } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                let values = stochastic_rsi(input.close, *rsi_period, *stochastic_period);
                self.outputs[0].extend(
                    values
                        .into_iter()
                        .skip(start)
                        .map(|value| value.unwrap_or(f64::NAN)),
                );
            }
            IncrementalKind::Momentum { period } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                let values = momentum(input.close, *period);
                self.outputs[0].extend(
                    values
                        .into_iter()
                        .skip(start)
                        .map(|value| value.unwrap_or(f64::NAN)),
                );
            }
            IncrementalKind::RateOfChange { period } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                let values = rate_of_change(input.close, *period);
                self.outputs[0].extend(
                    values
                        .into_iter()
                        .skip(start)
                        .map(|value| value.unwrap_or(f64::NAN)),
                );
            }
            IncrementalKind::Donchian { period } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                let points = donchian(input.high, input.low, *period);
                for point in points.into_iter().skip(start) {
                    self.outputs[0].push(point.upper.expect("Donchian upper after warmup"));
                    self.outputs[1].push(point.middle.expect("Donchian middle after warmup"));
                    self.outputs[2].push(point.lower.expect("Donchian lower after warmup"));
                }
            }
            IncrementalKind::PivotPoints { kind } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                let points = pivot_points(
                    input.times,
                    input.open,
                    input.high,
                    input.low,
                    input.close,
                    *kind,
                );
                for point in points.into_iter().skip(start) {
                    self.outputs[0].push(point.pivot.unwrap_or(f64::NAN));
                    self.outputs[1].push(point.resistance_1.unwrap_or(f64::NAN));
                    self.outputs[2].push(point.support_1.unwrap_or(f64::NAN));
                    self.outputs[3].push(point.resistance_2.unwrap_or(f64::NAN));
                    self.outputs[4].push(point.support_2.unwrap_or(f64::NAN));
                }
            }
            IncrementalKind::ZigZag { deviation_percent } => {
                // A new bar can move the provisional endpoint, and a historical correction can
                // alter every later confirmation. Recompute the bounded source window so the
                // sparse turning-point stream never leaves a stale endpoint behind.
                self.output_from[0] = 0;
                self.last_work_rows = n;
                self.outputs[0].extend(
                    zigzag(input.high, input.low, *deviation_percent)
                        .into_iter()
                        .map(|value| value.unwrap_or(f64::NAN)),
                );
            }
            IncrementalKind::Ichimoku => {
                let points = ichimoku(input.high, input.low, input.close);
                self.last_work_rows = n;
                for (output_index, start) in
                    self.output_from[..self.output_count].iter().enumerate()
                {
                    for &point in points.iter().skip(*start).take(n - *start) {
                        let value = match output_index {
                            0 => point.conversion,
                            1 => point.base,
                            2 => point.leading_a,
                            3 => point.leading_b,
                            4 => point.lagging,
                            _ => unreachable!("Ichimoku output index"),
                        };
                        self.outputs[output_index]
                            .push(value.expect("Ichimoku output after warmup"));
                    }
                }
            }
            IncrementalKind::Keltner {
                period,
                multiplier,
                state,
            } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let point = keltner_step(
                        &mut accumulator,
                        AtrSample {
                            high: input.high[row],
                            low: input.low[row],
                            close: input.close[row],
                        },
                        *period,
                        *multiplier,
                    );
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(point.upper.unwrap_or(f64::NAN));
                        self.outputs[1].push(point.middle.unwrap_or(f64::NAN));
                        self.outputs[2].push(point.lower.unwrap_or(f64::NAN));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::AdxDmi { period, state } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let point = adx_dmi_step(
                        &mut accumulator,
                        DirectionalSample {
                            high: input.high[row],
                            low: input.low[row],
                            close: input.close[row],
                        },
                        *period,
                    );
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(point.plus_di.unwrap_or(f64::NAN));
                        self.outputs[1].push(point.minus_di.unwrap_or(f64::NAN));
                    }
                    if row >= self.output_from[2] {
                        self.outputs[2].push(point.adx.unwrap_or(f64::NAN));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::ParabolicSar { state } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let value = valid_range(input.high[row], input.low[row]).then(|| {
                        parabolic_sar_step(
                            &mut accumulator,
                            input.high[row],
                            input.low[row],
                            0.02,
                            0.20,
                        )
                    });
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(value.unwrap_or(f64::NAN));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::SuperTrend {
                period,
                multiplier,
                state,
            } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let value = supertrend_step(
                        &mut accumulator,
                        DirectionalSample {
                            high: input.high[row],
                            low: input.low[row],
                            close: input.close[row],
                        },
                        *period,
                        *multiplier,
                    );
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(value.unwrap_or(f64::NAN));
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
                            self.outputs[output_index].push(value.unwrap_or(f64::NAN));
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
            IncrementalKind::BollingerMetrics { period, deviation } => {
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
                    // The dense metrics use the rounded band endpoints, not
                    // 2*spread. Near-flat bands can be only a few ULPs wide.
                    let upper = mean + spread;
                    let lower = mean - spread;
                    let width = upper - lower;
                    self.outputs[0].push(if width == 0.0 {
                        f64::NAN
                    } else {
                        (input.close[row] - lower) / width
                    });
                    self.outputs[1].push(if mean == 0.0 {
                        f64::NAN
                    } else {
                        width / mean * 100.0
                    });
                }
            }
            IncrementalKind::Envelopes {
                period,
                percent,
                ema_state,
            } => {
                let fraction = *percent / 100.0;
                if let Some(state) = ema_state {
                    let (start, mut accumulator) = state.begin(n, requested);
                    self.last_work_rows = n - start;
                    let mut tail = None;
                    let mut before_tail = None;
                    for row in start..n {
                        let previous = accumulator;
                        let value = ema_step(&mut accumulator, input.close[row], *period);
                        state.checkpoint(row, accumulator);
                        if row >= self.output_from[0] {
                            let basis = value.unwrap_or(f64::NAN);
                            self.outputs[0].push(basis * (1.0 + fraction));
                            self.outputs[1].push(basis);
                            self.outputs[2].push(basis * (1.0 - fraction));
                        }
                        if row + 1 == n {
                            tail = Some(accumulator);
                            before_tail = (row > 0).then_some(previous);
                        }
                    }
                    state.finish(n, tail, before_tail);
                } else {
                    let start = self.output_from[0];
                    self.last_work_rows = n - start;
                    for row in start..n {
                        let basis = input.close[row + 1 - *period..=row].iter().sum::<f64>()
                            / *period as f64;
                        self.outputs[0].push(basis * (1.0 + fraction));
                        self.outputs[1].push(basis);
                        self.outputs[2].push(basis * (1.0 - fraction));
                    }
                }
            }
            IncrementalKind::Rsi { period, state } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let value = indexed_rsi_step(&mut accumulator, input.close[row], *period);
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(value.unwrap_or(f64::NAN));
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
                        self.outputs[0].push(point.macd.unwrap_or(f64::NAN));
                    }
                    if row >= self.output_from[1] {
                        self.outputs[1].push(point.signal.unwrap_or(f64::NAN));
                        self.outputs[2].push(point.histogram.unwrap_or(f64::NAN));
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
                let (start, mut carry) = state.begin(n, state_from);
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
                    let previous = if row + 1 == n {
                        Some(carry.clone())
                    } else {
                        None
                    };
                    let valid = valid_bar(input.high[row], input.low[row], input.close[row]);
                    if valid {
                        carry.advance(input.high[row], input.low[row], input.close[row], *k_period);
                    }
                    let k = if row + 1 < *k_period {
                        50.0
                    } else if !valid
                        || !(row + 1 - *k_period..=row).all(|index| {
                            valid_bar(input.high[index], input.low[index], input.close[index])
                        })
                    {
                        f64::NAN
                    } else {
                        carry.previous_k
                    };
                    if (row + 1).is_multiple_of(CHECKPOINT_INTERVAL) {
                        state.checkpoint(row, carry.clone());
                    }
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
                        tail = Some(carry.clone());
                        before_tail = if row > 0 { previous } else { None };
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
                        self.outputs[0].push(value.unwrap_or(f64::NAN));
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
            IncrementalKind::Obv { state } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let value = obv_step(
                        &mut accumulator,
                        input.close[row],
                        input.volume.get(row).copied().unwrap_or(0.0),
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
            IncrementalKind::AccumulationDistribution { state } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let value = accumulation_distribution_step(
                        &mut accumulator,
                        input.high[row],
                        input.low[row],
                        input.close[row],
                        input.volume.get(row).copied().unwrap_or(0.0),
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
            IncrementalKind::PriceVolumeTrend { state } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let value = price_volume_trend_step(
                        &mut accumulator,
                        input.close[row],
                        input.volume.get(row).copied().unwrap_or(0.0),
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
            IncrementalKind::ChaikinOscillator { fast, slow, state } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let adl = accumulation_distribution_step(
                        &mut accumulator.cumulative,
                        input.high[row],
                        input.low[row],
                        input.close[row],
                        input.volume.get(row).copied().unwrap_or(0.0),
                    );
                    let fast_value = ema_step(&mut accumulator.fast, adl, *fast);
                    let slow_value = ema_step(&mut accumulator.slow, adl, *slow);
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0]
                            .push(fast_value.zip(slow_value).map_or(f64::NAN, |(a, b)| a - b));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::RelativeVolume { period } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                let volume_at =
                    |index: usize| input.volume.get(index).copied().unwrap_or(0.0).max(0.0);
                for row in start..n {
                    let sum = (row - *period..row).map(volume_at).sum::<f64>();
                    self.outputs[0].push(
                        if !input.close[row - *period..=row]
                            .iter()
                            .all(|v| v.is_finite())
                        {
                            f64::NAN
                        } else if sum > 0.0 {
                            volume_at(row) * *period as f64 / sum
                        } else {
                            f64::NAN
                        },
                    );
                }
            }
            IncrementalKind::VolumeOscillator {
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
                    let point = volume_oscillator_step(
                        &mut accumulator,
                        if input.close[row].is_finite() {
                            input.volume.get(row).copied().unwrap_or(0.0).max(0.0)
                        } else {
                            f64::NAN
                        },
                        *fast_period,
                        *slow_period,
                        *signal_period,
                    );
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(point.line.unwrap_or(f64::NAN));
                    }
                    if row >= self.output_from[1] {
                        self.outputs[1].push(point.signal.unwrap_or(f64::NAN));
                        self.outputs[2].push(point.histogram.unwrap_or(f64::NAN));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::ElderForce { period, state } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let close = input.close[row];
                    if close.is_finite() {
                        if let Some(previous_close) = accumulator.previous_close {
                            let force = (close - previous_close)
                                * input.volume.get(row).copied().unwrap_or(0.0).max(0.0);
                            let value = ema_step(&mut accumulator.ema, force, *period);
                            if row >= self.output_from[0] {
                                self.outputs[0].push(value.unwrap_or(f64::NAN));
                            }
                        } else if row >= self.output_from[0] {
                            self.outputs[0].push(f64::NAN);
                        }
                        accumulator.previous_close = Some(close);
                    } else if row >= self.output_from[0] {
                        self.outputs[0].push(f64::NAN);
                    }
                    state.checkpoint(row, accumulator);
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::EaseOfMovement { period, divisor } => {
                let start = self.output_from[0]
                    .saturating_sub(period.saturating_sub(1))
                    .max(1);
                self.last_work_rows = n.saturating_sub(start);
                let mut sum = 0.0;
                let mut missing = 0;
                for row in start..n {
                    let raw =
                        ease_of_movement_raw(input.high, input.low, input.volume, row, *divisor);
                    if raw.is_finite() {
                        sum += raw;
                    } else {
                        missing += 1;
                    }
                    if row >= start.saturating_add(*period) {
                        let outgoing = ease_of_movement_raw(
                            input.high,
                            input.low,
                            input.volume,
                            row - *period,
                            *divisor,
                        );
                        if outgoing.is_finite() {
                            sum -= outgoing;
                        } else {
                            missing -= 1;
                        }
                    }
                    if row >= self.output_from[0] {
                        self.outputs[0].push(if missing == 0 {
                            sum / *period as f64
                        } else {
                            f64::NAN
                        });
                    }
                }
            }
            IncrementalKind::HistoricalVolatility {
                period,
                annualization,
                state,
            } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let value = historical_volatility_step(
                        &mut accumulator,
                        input.close,
                        row,
                        *period,
                        *annualization,
                    );
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(value.expect("historical volatility after warmup"));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::Trix {
                period,
                signal_period,
                state,
            } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let (line, signal) =
                        trix_step(&mut accumulator, input.close[row], *period, *signal_period);
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(line.unwrap_or(f64::NAN));
                    }
                    if row >= self.output_from[1] {
                        self.outputs[1].push(signal.unwrap_or(f64::NAN));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::Kst {
                roc_periods,
                sma_periods,
                signal_period,
            } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                for row in start..n {
                    self.outputs[0].push(kst_at(input.close, row, *roc_periods, *sma_periods));
                    if row >= self.output_from[1] {
                        let signal = (row + 1 - *signal_period..=row)
                            .map(|index| kst_at(input.close, index, *roc_periods, *sma_periods))
                            .fold(0.0, |sum, value| sum + value);
                        self.outputs[1].push(signal / *signal_period as f64);
                    }
                }
            }
            IncrementalKind::Tsi {
                long,
                short,
                signal_period,
                state,
            } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let (line, signal) = tsi_step(
                        &mut accumulator,
                        input.close[row],
                        *long,
                        *short,
                        *signal_period,
                    );
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(line.unwrap_or(f64::NAN));
                    }
                    if row >= self.output_from[1] {
                        self.outputs[1].push(signal.unwrap_or(f64::NAN));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::MassIndex {
                ema_period,
                sum_period,
                state,
            } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = (row + 1 == n && row > 0).then(|| accumulator.clone());
                    let value = mass_index_step(
                        &mut accumulator,
                        input.high[row],
                        input.low[row],
                        *ema_period,
                        *sum_period,
                    );
                    if (row + 1).is_multiple_of(CHECKPOINT_INTERVAL) {
                        state.checkpoint(row, accumulator.clone());
                    }
                    if row >= self.output_from[0] {
                        self.outputs[0].push(value.unwrap_or(f64::NAN));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator.clone());
                        before_tail = previous;
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::Klinger {
                fast,
                slow,
                signal,
                state,
            } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let (line, signal_value) = klinger_step(
                        &mut accumulator,
                        input.high[row],
                        input.low[row],
                        input.close[row],
                        input.volume.get(row).copied().unwrap_or(f64::NAN),
                        [*fast, *slow, *signal],
                    );
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(line.unwrap_or(f64::NAN));
                    }
                    if row >= self.output_from[1] {
                        self.outputs[1].push(signal_value.unwrap_or(f64::NAN));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::Kama {
                period,
                fast,
                slow,
                state,
            } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = (row + 1 == n && row > 0).then(|| accumulator.clone());
                    let value =
                        kama_step(&mut accumulator, input.close, row, *period, *fast, *slow);
                    if (row + 1).is_multiple_of(CHECKPOINT_INTERVAL) {
                        state.checkpoint(row, accumulator.clone());
                    }
                    if row >= self.output_from[0] {
                        self.outputs[0].push(value.unwrap_or(f64::NAN));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator.clone());
                        before_tail = previous;
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::McGinley { period, state } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let value = mcginley_step(&mut accumulator, input.close[row], *period);
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(value.unwrap_or(f64::NAN));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::LinearRegression { period, deviation } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                for row in start..n {
                    let (curve, upper, lower) =
                        linear_regression_at(input.close, row, *period, *deviation);
                    self.outputs[0].push(curve);
                    self.outputs[1].push(upper);
                    self.outputs[2].push(lower);
                }
            }
            IncrementalKind::Choppiness { period } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                for row in start..n {
                    self.outputs[0].push(choppiness_at(
                        input.high,
                        input.low,
                        input.close,
                        row,
                        *period,
                    ));
                }
            }
            IncrementalKind::AtrBands {
                period,
                multiplier,
                state,
            } => {
                let (start, mut accumulator) = state.begin(n, requested);
                self.last_work_rows = n - start;
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    let atr = atr_bands_step(
                        &mut accumulator,
                        input.high[row],
                        input.low[row],
                        input.close[row],
                        *period,
                    );
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        let point = if atr.is_some() {
                            atr_bands_point(input.close[row], atr, *multiplier)
                        } else {
                            AtrBandsPoint {
                                basis: None,
                                upper: None,
                                lower: None,
                            }
                        };
                        self.outputs[0].push(point.upper.unwrap_or(f64::NAN));
                        self.outputs[1].push(point.basis.unwrap_or(f64::NAN));
                        self.outputs[2].push(point.lower.unwrap_or(f64::NAN));
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::CoppockCurve {
                long_period,
                short_period,
                smoothing,
            } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                for row in start..n {
                    self.outputs[0].push(coppock_at(
                        input.close,
                        row,
                        *long_period,
                        *short_period,
                        *smoothing,
                    ));
                }
            }
            IncrementalKind::FisherTransform {
                period,
                state,
                window,
            } => {
                let (start, mut accumulator) = state.begin(n, requested);
                // Recover the valid-price window preceding the EMA checkpoint, not merely
                // the previous physical rows: whitespace does not consume a sample.
                let mut repair = start;
                let mut needed = period.saturating_sub(1);
                while repair > 0 && needed > 0 {
                    repair -= 1;
                    if valid_range(input.high[repair], input.low[repair]) {
                        needed -= 1;
                    }
                }
                self.last_work_rows = n - repair;
                window.clear();
                for row in repair..start {
                    window.advance(input.high, input.low, row, *period);
                }
                let mut tail = None;
                let mut before_tail = None;
                for row in start..n {
                    let previous = accumulator;
                    window.advance(input.high, input.low, row, *period);
                    let (line, trigger) = if window.valid_rows.len() == *period
                        && valid_range(input.high[row], input.low[row])
                    {
                        fisher_step(&mut accumulator, window, input.high, input.low, row)
                    } else {
                        (f64::NAN, f64::NAN)
                    };
                    state.checkpoint(row, accumulator);
                    if row >= self.output_from[0] {
                        self.outputs[0].push(line);
                    }
                    if row >= self.output_from[1] {
                        self.outputs[1].push(trigger);
                    }
                    if row + 1 == n {
                        tail = Some(accumulator);
                        before_tail = (row > 0).then_some(previous);
                    }
                }
                state.finish(n, tail, before_tail);
            }
            IncrementalKind::UltimateOscillator {
                short,
                medium,
                long,
            } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                for row in start..n {
                    self.outputs[0].push(ultimate_at(
                        input.high,
                        input.low,
                        input.close,
                        row,
                        [*short, *medium, *long],
                    ));
                }
            }
            IncrementalKind::Vortex { period } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                for row in start..n {
                    let (plus, minus) = vortex_at(input.high, input.low, input.close, row, *period);
                    self.outputs[0].push(plus);
                    self.outputs[1].push(minus);
                }
            }
            IncrementalKind::Cmf { period } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                let values = cmf(input.high, input.low, input.close, input.volume, *period);
                self.outputs[0].extend(values.into_iter().skip(start).flatten());
            }
            IncrementalKind::Mfi { period } => {
                let start = self.output_from[0];
                self.last_work_rows = n - start;
                let values = mfi(input.high, input.low, input.close, input.volume, *period);
                self.outputs[0].extend(values.into_iter().skip(start).flatten());
            }
            IncrementalKind::Volume { period } => {
                let volume = input.volume;
                let volume_start = self.output_from[0];
                let average_start = self.output_from[1];
                self.last_work_rows = n.saturating_sub(volume_start.min(average_start));
                for row in volume_start..n {
                    self.outputs[0].push(if input.close[row].is_finite() {
                        volume.get(row).copied().unwrap_or(0.0).max(0.0)
                    } else {
                        f64::NAN
                    });
                }
                for row in average_start..n {
                    let window = &volume[row + 1 - *period..=row];
                    self.outputs[1].push(
                        if input.close[row + 1 - *period..=row]
                            .iter()
                            .all(|v| v.is_finite())
                        {
                            window.iter().map(|value| value.max(0.0)).sum::<f64>() / *period as f64
                        } else {
                            f64::NAN
                        },
                    );
                }
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
                        self.outputs[0].push(point.basis.unwrap_or(f64::NAN));
                        self.outputs[1].push(point.standard_upper.unwrap_or(f64::NAN));
                        self.outputs[2].push(point.standard_lower.unwrap_or(f64::NAN));
                        self.outputs[3].push(point.percent_upper.unwrap_or(f64::NAN));
                        self.outputs[4].push(point.percent_lower.unwrap_or(f64::NAN));
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
        IncrementalKind::Aroon { period, .. } => [*period, *period, 0, 0, 0],
        IncrementalKind::AwesomeOscillator => [33, 0, 0, 0, 0],
        IncrementalKind::Dpo { period } => {
            [period.saturating_sub(1).max(period / 2 + 1), 0, 0, 0, 0]
        }
        IncrementalKind::ChandeMomentum { period } => [*period, 0, 0, 0, 0],
        IncrementalKind::Sma { period }
        | IncrementalKind::Ema { period, .. }
        | IncrementalKind::Wma { period } => [period.saturating_sub(1), 0, 0, 0, 0],
        IncrementalKind::Dema { period, .. } => {
            [period.saturating_mul(2).saturating_sub(2), 0, 0, 0, 0]
        }
        IncrementalKind::Tema { period, .. } => {
            [period.saturating_mul(3).saturating_sub(3), 0, 0, 0, 0]
        }
        IncrementalKind::Smma { period, .. } => [period.saturating_sub(1), 0, 0, 0, 0],
        IncrementalKind::Hma { period } => {
            let smoothing = ((*period as f64).sqrt() as usize).max(1);
            [
                period
                    .saturating_sub(1)
                    .saturating_add(smoothing)
                    .saturating_sub(1),
                0,
                0,
                0,
                0,
            ]
        }
        IncrementalKind::Vwma { period } => [period.saturating_sub(1), 0, 0, 0, 0],
        IncrementalKind::StandardDeviation { period } => [period.saturating_sub(1), 0, 0, 0, 0],
        IncrementalKind::Cci { period } => [period.saturating_sub(1), 0, 0, 0, 0],
        IncrementalKind::WilliamsR { period } => [period.saturating_sub(1), 0, 0, 0, 0],
        IncrementalKind::StochasticRsi {
            rsi_period,
            stochastic_period,
        } => [
            rsi_period
                .saturating_add(*stochastic_period)
                .saturating_sub(1),
            0,
            0,
            0,
            0,
        ],
        IncrementalKind::Momentum { period } | IncrementalKind::RateOfChange { period } => {
            [*period, 0, 0, 0, 0]
        }
        IncrementalKind::Donchian { period } => [
            period.saturating_sub(1),
            period.saturating_sub(1),
            period.saturating_sub(1),
            0,
            0,
        ],
        IncrementalKind::PivotPoints { .. } => [0; MAX_OUTPUTS],
        IncrementalKind::ZigZag { .. } => [0; MAX_OUTPUTS],
        IncrementalKind::Keltner { period, .. } => [*period, *period, *period, 0, 0],
        IncrementalKind::AdxDmi { period, .. } => {
            let start = period.saturating_add(period.saturating_sub(1));
            [*period, *period, start, 0, 0]
        }
        IncrementalKind::ParabolicSar { .. } => [0, 0, 0, 0, 0],
        IncrementalKind::SuperTrend { period, .. } => [*period, 0, 0, 0, 0],
        IncrementalKind::Ichimoku => [8, 25, 25, 51, 0],
        IncrementalKind::EmaRibbon { periods, .. } => {
            periods.map(|period| period.saturating_sub(1))
        }
        IncrementalKind::Bollinger { period, .. } => {
            let mut starts = [0; MAX_OUTPUTS];
            starts[..3].fill(period.saturating_sub(1));
            starts
        }
        IncrementalKind::BollingerMetrics { period, .. } => {
            let mut starts = [0; MAX_OUTPUTS];
            starts[..2].fill(period.saturating_sub(1));
            starts
        }
        IncrementalKind::Envelopes { period, .. } => {
            let mut starts = [0; MAX_OUTPUTS];
            starts[..3].fill(period.saturating_sub(1));
            starts
        }
        IncrementalKind::Alma { period, .. } => [period.saturating_sub(1), 0, 0, 0, 0],
        IncrementalKind::ChaikinOscillator { slow, .. } => [slow.saturating_sub(1), 0, 0, 0, 0],
        IncrementalKind::RelativeVolume { period } => [*period, 0, 0, 0, 0],
        IncrementalKind::VolumeOscillator {
            slow_period,
            signal_period,
            ..
        } => {
            let line = slow_period.saturating_sub(1);
            let signal = line.saturating_add(signal_period.saturating_sub(1));
            [line, signal, signal, 0, 0]
        }
        IncrementalKind::ElderForce { period, .. } => [*period, 0, 0, 0, 0],
        IncrementalKind::EaseOfMovement { period, .. } => [*period, 0, 0, 0, 0],
        IncrementalKind::HistoricalVolatility { period, .. } => [*period, 0, 0, 0, 0],
        IncrementalKind::Trix {
            period,
            signal_period,
            ..
        } => {
            let line = trix_line_start(*period);
            [
                line,
                line.saturating_add(signal_period.saturating_sub(1)),
                0,
                0,
                0,
            ]
        }
        IncrementalKind::Kst {
            roc_periods,
            sma_periods,
            signal_period,
        } => {
            let line = kst_line_start(*roc_periods, *sma_periods);
            [
                line,
                line.saturating_add(signal_period.saturating_sub(1)),
                0,
                0,
                0,
            ]
        }
        IncrementalKind::Tsi {
            long,
            short,
            signal_period,
            ..
        } => {
            let line = long.saturating_add(*short).saturating_sub(1);
            [
                line,
                line.saturating_add(signal_period.saturating_sub(1)),
                0,
                0,
                0,
            ]
        }
        IncrementalKind::MassIndex {
            ema_period,
            sum_period,
            ..
        } => [mass_start(*ema_period, *sum_period), 0, 0, 0, 0],
        IncrementalKind::Klinger { slow, signal, .. } => {
            let line = slow.saturating_sub(1);
            [line, line.saturating_add(signal.saturating_sub(1)), 0, 0, 0]
        }
        IncrementalKind::Kama { period, .. } => [period.saturating_sub(1), 0, 0, 0, 0],
        IncrementalKind::McGinley { .. } => [0; MAX_OUTPUTS],
        IncrementalKind::LinearRegression { period, .. } => {
            let start = period.saturating_sub(1);
            [start, start, start, 0, 0]
        }
        IncrementalKind::Choppiness { period } => [*period, 0, 0, 0, 0],
        IncrementalKind::AtrBands { period, .. } => [*period, *period, *period, 0, 0],
        IncrementalKind::CoppockCurve {
            long_period,
            short_period,
            smoothing,
        } => [
            coppock_start(*long_period, *short_period, *smoothing),
            0,
            0,
            0,
            0,
        ],
        IncrementalKind::FisherTransform { period, .. } => {
            [period.saturating_sub(1), *period, 0, 0, 0]
        }
        IncrementalKind::UltimateOscillator {
            short,
            medium,
            long,
        } => [short.max(medium).max(long).saturating_sub(1), 0, 0, 0, 0],
        IncrementalKind::Vortex { period } => [*period, *period, 0, 0, 0],
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
        IncrementalKind::Cmf { period } => [period.saturating_sub(1), 0, 0, 0, 0],
        IncrementalKind::Mfi { period } => [*period, 0, 0, 0, 0],
        IncrementalKind::Volume { period } => [0, period.saturating_sub(1), 0, 0, 0],
        IncrementalKind::Vwap { .. }
        | IncrementalKind::Obv { .. }
        | IncrementalKind::AccumulationDistribution { .. }
        | IncrementalKind::PriceVolumeTrend { .. }
        | IncrementalKind::VwapBands { .. } => [0; MAX_OUTPUTS],
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
    fn dema_uses_two_sma_seeded_ema_stages() {
        assert_eq!(
            dema(&[1.0, 2.0, 3.0, 5.0, 8.0], 3),
            vec![None, None, None, None, Some(7.75)]
        );
    }

    #[test]
    fn tema_uses_three_sma_seeded_ema_stages() {
        assert_eq!(
            tema(&[1.0, 2.0, 3.0, 5.0, 8.0, 13.0, 21.0], 3),
            vec![None, None, None, None, None, None, Some(20.0)]
        );
    }

    #[test]
    fn smma_uses_wilder_smoothing_and_rma_aliases_it() {
        let values = [1.0, 2.0, 4.0, 8.0, 16.0];
        let actual = smma(&values, 3);
        let expected = [
            None,
            None,
            Some(7.0 / 3.0),
            Some(38.0 / 9.0),
            Some(220.0 / 27.0),
        ];
        for (actual, expected) in actual.into_iter().zip(expected) {
            match (actual, expected) {
                (None, None) => {}
                (Some(actual), Some(expected)) => assert!((actual - expected).abs() < 1e-12),
                other => panic!("unexpected SMMA result: {other:?}"),
            }
        }
        assert_eq!(rma(&values, 3), smma(&values, 3));
    }

    #[test]
    fn hma_combines_half_full_and_smoothing_wmas() {
        assert_eq!(
            hma(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0], 4),
            vec![None, None, None, None, Some(5.0), Some(6.0), Some(7.0)]
        );
    }

    #[test]
    fn vwma_weights_the_window_and_falls_back_for_missing_or_zero_volume() {
        assert_eq!(
            vwma(&[10.0, 20.0, 30.0], &[1.0, 2.0, 3.0], 2),
            vec![None, Some(50.0 / 3.0), Some(130.0 / 5.0)]
        );
        assert_eq!(
            vwma(&[10.0, 20.0, 30.0], &[], 2),
            vec![None, Some(15.0), Some(25.0)]
        );
        assert_eq!(
            vwma(&[10.0, 20.0, 30.0], &[0.0, 0.0, 0.0], 2),
            vec![None, Some(15.0), Some(25.0)]
        );
    }

    #[test]
    fn standard_deviation_uses_population_window_variance() {
        assert_eq!(
            standard_deviation(&[1.0, 2.0, 3.0, 5.0], 3),
            vec![
                None,
                None,
                Some((2.0_f64 / 3.0).sqrt()),
                Some((14.0_f64 / 9.0).sqrt())
            ]
        );
    }

    #[test]
    fn donchian_reports_rolling_high_low_and_midpoint() {
        let points = donchian(&[3.0, 5.0, 4.0, 8.0], &[1.0, 2.0, 2.5, 6.0], 3);
        assert_eq!(points[0].upper, None);
        assert_eq!(
            points[2],
            DonchianPoint {
                upper: Some(5.0),
                middle: Some(3.0),
                lower: Some(1.0),
            }
        );
        assert_eq!(
            points[3],
            DonchianPoint {
                upper: Some(8.0),
                middle: Some(5.0),
                lower: Some(2.0),
            }
        );
    }

    #[test]
    fn pivot_points_use_the_previous_utc_session_without_lookahead() {
        let points = pivot_points(
            &[0, 3_600, 86_400, 90_000],
            &[10.0, 11.0, 12.0, 13.0],
            &[12.0, 14.0, 15.0, 16.0],
            &[8.0, 9.0, 10.0, 11.0],
            &[11.0, 13.0, 14.0, 15.0],
            PivotKind::Standard,
        );
        assert_eq!(points[0], PivotPoint::default());
        assert_eq!(points[1], PivotPoint::default());
        let point = points[2];
        let expected = [35.0 / 3.0, 46.0 / 3.0, 28.0 / 3.0, 53.0 / 3.0, 17.0 / 3.0];
        for (actual, expected) in [
            point.pivot,
            point.resistance_1,
            point.support_1,
            point.resistance_2,
            point.support_2,
        ]
        .into_iter()
        .zip(expected)
        {
            assert!((actual.unwrap() - expected).abs() < 1e-12);
        }
        assert_eq!(points[3], point);

        for (kind, expected_pivot) in [
            (PivotKind::Fibonacci, 35.0 / 3.0),
            (PivotKind::Camarilla, 13.0),
            (PivotKind::Woodie, 12.0),
            (PivotKind::DeMark, 49.0 / 4.0),
        ] {
            let points = pivot_points(
                &[0, 3_600, 86_400],
                &[10.0, 11.0, 12.0],
                &[12.0, 14.0, 15.0],
                &[8.0, 9.0, 10.0],
                &[11.0, 13.0, 14.0],
                kind,
            );
            assert!((points[2].pivot.unwrap() - expected_pivot).abs() < 1e-12);
            assert!(
                points[2].resistance_1.is_some()
                    && points[2].support_1.is_some()
                    && points[2].resistance_2.is_some()
                    && points[2].support_2.is_some()
            );
        }
    }

    #[test]
    fn zigzag_confirms_reversals_at_the_requested_percentage() {
        let values = zigzag(
            &[100.0, 101.0, 106.0, 105.0, 99.0, 100.0, 108.0],
            &[99.0, 100.0, 105.0, 104.0, 98.0, 99.0, 107.0],
            5.0,
        );
        assert_eq!(
            values,
            vec![
                Some(99.0),
                None,
                Some(106.0),
                None,
                Some(98.0),
                None,
                Some(108.0)
            ]
        );
        assert!(zigzag(&[1.0], &[1.0], 0.0).iter().all(Option::is_none));
    }

    #[test]
    fn ichimoku_reports_conventional_warmups_and_aligned_lagging_close() {
        let highs = (0..60).map(|value| value as f64 + 10.0).collect::<Vec<_>>();
        let lows = (0..60).map(|value| value as f64).collect::<Vec<_>>();
        let closes = (0..60).map(|value| value as f64 + 0.5).collect::<Vec<_>>();
        let points = ichimoku(&highs, &lows, &closes);

        assert!(points[..8].iter().all(|point| point.conversion.is_none()));
        assert!(points[..25].iter().all(|point| point.base.is_none()));
        assert!(points[..25].iter().all(|point| point.leading_a.is_none()));
        assert!(points[..51].iter().all(|point| point.leading_b.is_none()));
        assert!(points.iter().all(|point| point.lagging.is_some()));
        assert_eq!(points[8].conversion, Some(9.0));
        assert_eq!(points[25].base, Some(17.5));
        assert_eq!(points[25].leading_a, Some(21.75));
        assert_eq!(points[51].leading_b, Some(30.5));
        assert_eq!(points[59].lagging, Some(59.5));
    }

    #[test]
    fn cci_uses_typical_price_and_zero_for_flat_deviation() {
        let values = [1.0, 2.0, 3.0, 4.0];
        let points = cci(&values, &values, &values, 3);
        assert_eq!(points[..2], [None, None]);
        assert!((points[2].unwrap() - 100.0).abs() < 1e-12);
        assert!((points[3].unwrap() - 100.0).abs() < 1e-12);
        assert_eq!(
            cci(&[5.0, 5.0, 5.0], &[5.0, 5.0, 5.0], &[5.0, 5.0, 5.0], 3),
            vec![None, None, Some(0.0)]
        );
    }

    #[test]
    fn williams_r_uses_rolling_extremes_and_zero_for_flat_window() {
        let highs = [10.0, 11.0, 12.0, 13.0];
        let lows = [0.0, 0.0, 0.0, 0.0];
        let closes = [5.0, 6.0, 9.0, 12.0];
        let points = williams_r(&highs, &lows, &closes, 3);
        assert_eq!(points[..2], [None, None]);
        assert!((points[2].unwrap() + 25.0).abs() < 1e-12);
        assert!((points[3].unwrap() + 7.692307692307692).abs() < 1e-12);
        assert_eq!(
            williams_r(&[5.0, 5.0], &[5.0, 5.0], &[5.0, 5.0], 2),
            vec![None, Some(0.0)]
        );
    }

    #[test]
    fn stochastic_rsi_warms_up_both_windows_and_normalizes_the_range() {
        let values = (0..12).map(|index| index as f64).collect::<Vec<_>>();
        let points = stochastic_rsi(&values, 3, 3);
        assert!(points[..5].iter().all(Option::is_none));
        assert_eq!(points[5], Some(0.0));
        assert!(points[6..].iter().all(|value| *value == Some(0.0)));
        assert!(stochastic_rsi(&values, 0, 3).iter().all(Option::is_none));
    }

    #[test]
    fn momentum_and_rate_of_change_use_lagged_values() {
        let values = [10.0, 12.0, 15.0, 20.0];
        assert_eq!(momentum(&values, 2), vec![None, None, Some(5.0), Some(8.0)]);
        assert_eq!(
            rate_of_change(&values, 2),
            vec![None, None, Some(50.0), Some(66.66666666666667)]
        );
        assert_eq!(rate_of_change(&[0.0, 1.0], 1), vec![None, Some(0.0)]);
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
    fn keltner_uses_ema_center_and_atr_envelope() {
        let highs = [11.0, 12.0, 14.0, 15.0];
        let lows = [9.0, 10.0, 11.0, 12.0];
        let closes = [10.0, 11.0, 13.0, 14.0];
        let points = keltner(&highs, &lows, &closes, 2, 2.0);
        assert!(points[1].middle.is_none());
        let center = ema(&closes, 2)[2].expect("EMA warmup");
        let range = atr(&highs, &lows, &closes, 2)[2].expect("ATR warmup");
        assert_eq!(points[2].middle, Some(center));
        assert_eq!(points[2].upper, Some(center + range * 2.0));
        assert_eq!(points[2].lower, Some(center - range * 2.0));
    }

    #[test]
    fn adx_dmi_seeds_directional_values_then_adx() {
        let highs = [10.0, 12.0, 14.0, 13.0, 15.0, 16.0];
        let lows = [8.0, 9.0, 11.0, 10.0, 12.0, 13.0];
        let closes = [9.0, 11.0, 13.0, 11.0, 14.0, 15.0];
        let points = adx_dmi(&highs, &lows, &closes, 2);
        assert!(points[1].plus_di.is_none());
        assert!(points[2].plus_di.is_some());
        assert!(points[2].minus_di.is_some());
        assert!(points[2].adx.is_none());
        assert!(points[3].adx.is_some());
        assert!(points.iter().skip(2).all(|point| {
            point
                .plus_di
                .is_some_and(|value| (0.0..=100.0).contains(&value))
                && point
                    .minus_di
                    .is_some_and(|value| (0.0..=100.0).contains(&value))
        }));
    }

    #[test]
    fn parabolic_sar_reverses_and_stays_inside_prior_extremes() {
        let highs = [10.0, 11.0, 12.0, 9.0, 8.0, 10.0];
        let lows = [8.0, 9.0, 10.0, 7.0, 6.0, 8.0];
        let values = parabolic_sar(&highs, &lows);
        assert_eq!(values[0], Some(8.0));
        assert!(values.iter().all(|value| value.is_some_and(f64::is_finite)));
        assert!(values[3].unwrap() >= highs[2]);
    }

    #[test]
    fn supertrend_warms_up_with_atr_and_stays_finite() {
        let closes = [10.0, 11.0, 12.0, 11.0, 13.0, 14.0];
        let highs = closes.iter().map(|value| value + 1.0).collect::<Vec<_>>();
        let lows = closes.iter().map(|value| value - 1.0).collect::<Vec<_>>();
        let values = supertrend(&highs, &lows, &closes, 2, 3.0);
        assert!(values[1].is_none());
        assert!(
            values[2..]
                .iter()
                .all(|value| value.is_some_and(f64::is_finite))
        );
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

    #[test]
    fn obv_uses_current_volume_and_direction() {
        assert_eq!(
            obv(&[10.0, 12.0, 11.0, 11.0, 13.0], &[4.0, 5.0, 3.0, -2.0, 7.0]),
            vec![Some(0.0), Some(5.0), Some(2.0), Some(2.0), Some(9.0)]
        );
    }

    #[test]
    fn cmf_weights_close_location_by_volume() {
        let values = cmf(
            &[12.0, 14.0, 13.0],
            &[8.0, 10.0, 9.0],
            &[10.0, 13.0, 10.0],
            &[2.0, 4.0, 2.0],
            2,
        );
        assert_eq!(values[0], None);
        assert!((values[1].unwrap() - (1.0 / 3.0)).abs() < 1e-12);
        assert!((values[2].unwrap() - (1.0 / 6.0)).abs() < 1e-12);
    }

    #[test]
    fn mfi_uses_directional_money_flow_windows() {
        let values = mfi(
            &[10.0, 11.0, 12.0, 11.0],
            &[10.0, 11.0, 12.0, 11.0],
            &[10.0, 11.0, 12.0, 11.0],
            &[1.0, 2.0, 3.0, 4.0],
            2,
        );
        assert_eq!(values[0], None);
        assert_eq!(values[1], None);
        assert_eq!(values[2], Some(100.0));
        assert!((values[3].unwrap() - (100.0 - 100.0 / (1.0 + 36.0 / 44.0))).abs() < 1e-12);
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum TestKind {
        Aroon,
        AwesomeOscillator,
        Dpo,
        ChandeMomentum,
        Sma,
        Ema,
        Dema,
        Tema,
        Smma,
        Hma,
        Vwma,
        StandardDeviation,
        Cci,
        WilliamsR,
        StochasticRsi,
        Momentum,
        RateOfChange,
        Donchian,
        PivotPoints,
        ZigZag,
        Keltner,
        AdxDmi,
        ParabolicSar,
        SuperTrend,
        Ichimoku,
        EmaRibbon,
        Bollinger,
        BollingerMetrics,
        EnvelopesSma,
        EnvelopesEma,
        Alma,
        Rsi,
        Macd,
        Stochastic,
        Atr,
        Vwap,
        VwapBands,
        Obv,
        AccumulationDistribution,
        PriceVolumeTrend,
        ChaikinOscillator,
        RelativeVolume,
        VolumeOscillator,
        ElderForce,
        EaseOfMovement,
        HistoricalVolatility,
        Trix,
        Kst,
        Tsi,
        MassIndex,
        Klinger,
        Kama,
        McGinley,
        LinearRegression,
        Choppiness,
        AtrBands,
        CoppockCurve,
        FisherTransform,
        UltimateOscillator,
        Vortex,
        Cmf,
        Mfi,
        Volume,
        Wma,
    }

    fn expected(kind: TestKind, input: IndicatorInput<'_>) -> Vec<Vec<Option<f64>>> {
        match kind {
            TestKind::Aroon => {
                let points = aroon(input.high, input.low, 5);
                vec![
                    points.iter().map(|point| point.0).collect(),
                    points.iter().map(|point| point.1).collect(),
                ]
            }
            TestKind::AwesomeOscillator => vec![awesome_oscillator(input.high, input.low)],
            TestKind::Dpo => vec![dpo(input.close, 5)],
            TestKind::ChandeMomentum => vec![chande_momentum(input.close, 5)],
            TestKind::Sma => vec![sma(input.close, 5)],
            TestKind::Ema => vec![ema(input.close, 5)],
            TestKind::Dema => vec![dema(input.close, 5)],
            TestKind::Tema => vec![tema(input.close, 5)],
            TestKind::Smma => vec![smma(input.close, 5)],
            TestKind::Hma => vec![hma(input.close, 5)],
            TestKind::Vwma => vec![vwma(input.close, input.volume, 5)],
            TestKind::StandardDeviation => vec![standard_deviation(input.close, 5)],
            TestKind::Cci => vec![cci(input.high, input.low, input.close, 5)],
            TestKind::WilliamsR => vec![williams_r(input.high, input.low, input.close, 5)],
            TestKind::StochasticRsi => vec![stochastic_rsi(input.close, 5, 5)],
            TestKind::Momentum => vec![momentum(input.close, 5)],
            TestKind::RateOfChange => vec![rate_of_change(input.close, 5)],
            TestKind::Donchian => {
                let points = donchian(input.high, input.low, 5);
                vec![
                    points.iter().map(|point| point.upper).collect(),
                    points.iter().map(|point| point.middle).collect(),
                    points.iter().map(|point| point.lower).collect(),
                ]
            }
            TestKind::PivotPoints => {
                let points = pivot_points(
                    input.times,
                    input.open,
                    input.high,
                    input.low,
                    input.close,
                    PivotKind::Standard,
                );
                vec![
                    points.iter().map(|point| point.pivot).collect(),
                    points.iter().map(|point| point.resistance_1).collect(),
                    points.iter().map(|point| point.support_1).collect(),
                    points.iter().map(|point| point.resistance_2).collect(),
                    points.iter().map(|point| point.support_2).collect(),
                ]
            }
            TestKind::ZigZag => vec![zigzag(input.high, input.low, 5.0)],
            TestKind::Keltner => {
                let points = keltner(input.high, input.low, input.close, 5, 2.0);
                vec![
                    points.iter().map(|point| point.upper).collect(),
                    points.iter().map(|point| point.middle).collect(),
                    points.iter().map(|point| point.lower).collect(),
                ]
            }
            TestKind::AdxDmi => {
                let points = adx_dmi(input.high, input.low, input.close, 5);
                vec![
                    points.iter().map(|point| point.plus_di).collect(),
                    points.iter().map(|point| point.minus_di).collect(),
                    points.iter().map(|point| point.adx).collect(),
                ]
            }
            TestKind::ParabolicSar => vec![parabolic_sar(input.high, input.low)],
            TestKind::SuperTrend => vec![supertrend(input.high, input.low, input.close, 5, 3.0)],
            TestKind::Ichimoku => {
                let points = ichimoku(input.high, input.low, input.close);
                vec![
                    points.iter().map(|point| point.conversion).collect(),
                    points.iter().map(|point| point.base).collect(),
                    points.iter().map(|point| point.leading_a).collect(),
                    points.iter().map(|point| point.leading_b).collect(),
                    points.iter().map(|point| point.lagging).collect(),
                ]
            }
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
            TestKind::BollingerMetrics => {
                let points = bollinger_metrics(input.close, 5, 2.0);
                vec![
                    points.iter().map(|point| point.0).collect(),
                    points.iter().map(|point| point.1).collect(),
                ]
            }
            TestKind::EnvelopesSma | TestKind::EnvelopesEma => {
                let points =
                    envelopes(input.close, 5, 10.0, matches!(kind, TestKind::EnvelopesEma));
                vec![
                    points.iter().map(|point| point.0).collect(),
                    points.iter().map(|point| point.1).collect(),
                    points.iter().map(|point| point.2).collect(),
                ]
            }
            TestKind::Alma => vec![alma(input.close, 5, 0.85, 6.0)],
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
            TestKind::VwapBands => {
                let points = vwap_bands(
                    input.times,
                    input.high,
                    input.low,
                    input.close,
                    input.volume,
                    VwapBandsOptions {
                        reset: VwapReset::Monthly,
                        standard_deviation: 1.0,
                        percent: 5.0,
                    },
                );
                vec![
                    points.iter().map(|point| point.basis).collect(),
                    points.iter().map(|point| point.standard_upper).collect(),
                    points.iter().map(|point| point.standard_lower).collect(),
                    points.iter().map(|point| point.percent_upper).collect(),
                    points.iter().map(|point| point.percent_lower).collect(),
                ]
            }
            TestKind::Obv => vec![obv(input.close, input.volume)],
            TestKind::AccumulationDistribution => vec![accumulation_distribution(
                input.high,
                input.low,
                input.close,
                input.volume,
            )],
            TestKind::PriceVolumeTrend => vec![price_volume_trend(input.close, input.volume)],
            TestKind::ChaikinOscillator => vec![chaikin_oscillator(
                input.high,
                input.low,
                input.close,
                input.volume,
                3,
                7,
            )],
            // Volume-only formulas receive source-aligned volume with whitespace masked.
            TestKind::RelativeVolume => vec![relative_volume(
                &input
                    .volume
                    .iter()
                    .zip(input.close)
                    .map(|(&volume, &close)| if close.is_finite() { volume } else { f64::NAN })
                    .collect::<Vec<_>>(),
                5,
            )],
            TestKind::VolumeOscillator => {
                let volume = input
                    .volume
                    .iter()
                    .zip(input.close)
                    .map(|(&volume, &close)| if close.is_finite() { volume } else { f64::NAN })
                    .collect::<Vec<_>>();
                let points = volume_oscillator(&volume, 3, 7, 4);
                vec![
                    points.iter().map(|point| point.line).collect(),
                    points.iter().map(|point| point.signal).collect(),
                    points.iter().map(|point| point.histogram).collect(),
                ]
            }
            TestKind::ElderForce => vec![elder_force(input.close, input.volume, 5)],
            TestKind::EaseOfMovement => vec![ease_of_movement(
                input.high,
                input.low,
                input.volume,
                5,
                100.0,
            )],
            TestKind::HistoricalVolatility => {
                vec![historical_volatility(input.close, 5, 252.0)]
            }
            TestKind::Trix => {
                let points = trix(input.close, 3, 4);
                vec![
                    points.iter().map(|point| point.line).collect(),
                    points.iter().map(|point| point.signal).collect(),
                ]
            }
            TestKind::Kst => {
                let points = kst(input.close, [2, 3, 4, 5], [2, 2, 2, 3], 3);
                vec![
                    points.iter().map(|point| point.line).collect(),
                    points.iter().map(|point| point.signal).collect(),
                ]
            }
            TestKind::Tsi => {
                let points = tsi(input.close, 5, 3, 3);
                vec![
                    points.iter().map(|point| point.line).collect(),
                    points.iter().map(|point| point.signal).collect(),
                ]
            }
            TestKind::MassIndex => vec![mass_index(input.high, input.low, 3, 5)],
            TestKind::Klinger => {
                let points = klinger(input.high, input.low, input.close, input.volume, 3, 7, 4);
                vec![
                    points.iter().map(|point| point.line).collect(),
                    points.iter().map(|point| point.signal).collect(),
                ]
            }
            TestKind::Kama => vec![kama(input.close, 5, 2, 10)],
            TestKind::McGinley => vec![mcginley(input.close, 5)],
            TestKind::LinearRegression => {
                let points = linear_regression(input.close, 5, 2.0);
                vec![
                    points.iter().map(|point| point.curve).collect(),
                    points.iter().map(|point| point.upper).collect(),
                    points.iter().map(|point| point.lower).collect(),
                ]
            }
            TestKind::Choppiness => vec![choppiness(input.high, input.low, input.close, 5)],
            TestKind::AtrBands => {
                let points = atr_bands(input.high, input.low, input.close, 5, 2.0);
                vec![
                    points.iter().map(|point| point.upper).collect(),
                    points.iter().map(|point| point.basis).collect(),
                    points.iter().map(|point| point.lower).collect(),
                ]
            }
            TestKind::CoppockCurve => vec![coppock_curve(input.close, 7, 5, 3)],
            TestKind::FisherTransform => {
                let points = fisher_transform(input.high, input.low, 5);
                vec![
                    points.iter().map(|point| point.line).collect(),
                    points.iter().map(|point| point.trigger).collect(),
                ]
            }
            TestKind::UltimateOscillator => vec![ultimate_oscillator(
                input.high,
                input.low,
                input.close,
                3,
                5,
                7,
            )],
            TestKind::Vortex => {
                let points = vortex(input.high, input.low, input.close, 5);
                vec![
                    points.iter().map(|point| point.plus).collect(),
                    points.iter().map(|point| point.minus).collect(),
                ]
            }
            TestKind::Cmf => vec![cmf(input.high, input.low, input.close, input.volume, 5)],
            TestKind::Mfi => vec![mfi(input.high, input.low, input.close, input.volume, 5)],
            TestKind::Volume => {
                let values = (0..input.close.len())
                    .map(|row| {
                        input.close[row]
                            .is_finite()
                            .then(|| input.volume[row].max(0.0))
                    })
                    .collect::<Vec<_>>();
                let average = (0..values.len())
                    .map(|row| {
                        (row >= 4).then(|| {
                            let window = &values[row - 4..=row];
                            if window.iter().all(Option::is_some) {
                                window.iter().map(|v| v.unwrap()).sum::<f64>() / 5.0
                            } else {
                                f64::NAN
                            }
                        })
                    })
                    .collect();
                vec![values, average]
            }
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
                    match expected {
                        Some(expected) if expected.is_nan() => assert!(
                            actual.is_nan(),
                            "{kind:?} output {output} row {index}: expected NaN, got {actual}"
                        ),
                        Some(expected) => assert!(
                            (actual - expected).abs() < 1e-10,
                            "{kind:?} output {output} row {index}: {actual} != {expected}"
                        ),
                        None => assert!(
                            actual.is_nan(),
                            "{kind:?} output {output} row {index}: expected warmup NaN, got {actual}"
                        ),
                    }
                }
            }
        }
    }

    // Deleting source whitespace is an independent metamorphic oracle: the compact run never
    // sees a NaN, so it cannot inherit the gapped formula's accidentally reset/poisoned state.
    fn assert_continues_across_gaps(
        kind: TestKind,
        state: &mut IncrementalState,
        input: IndicatorInput<'_>,
        from: usize,
    ) {
        let kept = (0..input.close.len())
            .filter(|&row| {
                let range = input.high[row].is_finite() && input.low[row].is_finite();
                match kind {
                    TestKind::MassIndex | TestKind::FisherTransform | TestKind::ParabolicSar => {
                        range
                    }
                    TestKind::Atr
                    | TestKind::AdxDmi
                    | TestKind::SuperTrend
                    | TestKind::AtrBands
                    | TestKind::Keltner
                    | TestKind::Klinger
                    | TestKind::Vwap
                    | TestKind::VwapBands
                    | TestKind::AccumulationDistribution
                    | TestKind::ChaikinOscillator => range && input.close[row].is_finite(),
                    _ => input.close[row].is_finite(),
                }
            })
            .collect::<Vec<_>>();
        let times = kept.iter().map(|&row| input.times[row]).collect::<Vec<_>>();
        let open = kept.iter().map(|&row| input.open[row]).collect::<Vec<_>>();
        let high = kept.iter().map(|&row| input.high[row]).collect::<Vec<_>>();
        let low = kept.iter().map(|&row| input.low[row]).collect::<Vec<_>>();
        let close = kept.iter().map(|&row| input.close[row]).collect::<Vec<_>>();
        let volume = kept
            .iter()
            .map(|&row| input.volume[row])
            .collect::<Vec<_>>();
        let compact = expected(
            kind,
            IndicatorInput {
                times: &times,
                open: &open,
                high: &high,
                low: &low,
                close: &close,
                volume: &volume,
            },
        );
        let actual = expected(kind, input);
        state.rebuild_from(input, from);
        assert_eq!(actual.len(), compact.len(), "{kind:?} output count");
        for output in 0..actual.len() {
            let mut compact_row = 0;
            for (row, &observed) in actual[output].iter().enumerate() {
                let want = if kept.binary_search(&row).is_ok() {
                    let value = compact[output][compact_row];
                    compact_row += 1;
                    value
                } else {
                    None
                };
                let compare = |found: Option<f64>, path: &str| match (
                    want.filter(|v| v.is_finite()),
                    found.filter(|v| v.is_finite()),
                ) {
                    (None, None) => {}
                    (Some(a), Some(b)) if (a - b).abs() <= 1e-9 * a.abs().max(1.0) => {}
                    _ => panic!("{kind:?} {path} output {output} row {row}: {found:?} != {want:?}"),
                };
                compare(observed, "batch");
                let incremental = if row >= state.output_from(output) {
                    Some(state.output(output)[row - state.output_from(output)])
                } else {
                    None
                };
                if row >= from {
                    compare(incremental, "incremental");
                }
            }
        }
    }

    #[test]
    fn ema_family_deleted_rows_oracle() {
        gap_family_oracle(&[
            (TestKind::Ema, IncrementalState::ema(5)),
            (TestKind::Smma, IncrementalState::smma(5)),
            (TestKind::Dema, IncrementalState::dema(5)),
            (TestKind::Tema, IncrementalState::tema(5)),
            (
                TestKind::EmaRibbon,
                IncrementalState::ema_ribbon([3, 5, 8, 13, 21]),
            ),
            (
                TestKind::EnvelopesEma,
                IncrementalState::envelopes(5, 10.0, true),
            ),
            (TestKind::Macd, IncrementalState::macd(3, 6, 4)),
            (TestKind::Trix, IncrementalState::trix(3, 4)),
            (TestKind::Keltner, IncrementalState::keltner(5, 2.0)),
            (TestKind::ElderForce, IncrementalState::elder_force(5)),
        ]);
    }

    #[test]
    fn rsi_family_deleted_rows_oracle() {
        gap_family_oracle(&[
            (TestKind::Rsi, IncrementalState::rsi(5)),
            (
                TestKind::StochasticRsi,
                IncrementalState::stochastic_rsi(5, 5),
            ),
        ]);
    }

    #[test]
    fn atr_family_deleted_rows_oracle() {
        gap_family_oracle(&[
            (TestKind::Atr, IncrementalState::atr(5)),
            (TestKind::Keltner, IncrementalState::keltner(5, 2.0)),
            (TestKind::AdxDmi, IncrementalState::adx_dmi(5)),
            (TestKind::SuperTrend, IncrementalState::supertrend(5, 3.0)),
            (TestKind::ParabolicSar, IncrementalState::parabolic_sar()),
            (TestKind::AtrBands, IncrementalState::atr_bands(5, 2.0)),
        ]);
    }

    #[test]
    fn cumulative_family_deleted_rows_oracle() {
        gap_family_oracle(&[
            (TestKind::Obv, IncrementalState::obv()),
            (
                TestKind::AccumulationDistribution,
                IncrementalState::accumulation_distribution(),
            ),
            (
                TestKind::ChaikinOscillator,
                IncrementalState::chaikin_oscillator(3, 7),
            ),
            (
                TestKind::PriceVolumeTrend,
                IncrementalState::price_volume_trend(),
            ),
            (TestKind::Vwap, IncrementalState::vwap()),
            (
                TestKind::VwapBands,
                IncrementalState::vwap_bands(VwapReset::Monthly, 1.0, 5.0),
            ),
        ]);
    }

    #[test]
    fn formerly_reset_family_deleted_rows_oracle() {
        gap_family_oracle(&[
            (TestKind::Tsi, IncrementalState::tsi(5, 3, 3)),
            (TestKind::Kama, IncrementalState::kama(5, 2, 10)),
            (TestKind::Klinger, IncrementalState::klinger(3, 7, 4)),
            (TestKind::MassIndex, IncrementalState::mass_index(3, 5)),
            (
                TestKind::FisherTransform,
                IncrementalState::fisher_transform(5),
            ),
            (TestKind::McGinley, IncrementalState::mcginley(5)),
        ]);
    }

    #[test]
    fn nan_correct_windows_recover_and_mfi_uses_last_valid_typical_price() {
        let high = [11.0, 12.0, 13.0, f64::NAN, 18.0, 16.0, 15.0, 14.0];
        let low = [9.0, 10.0, 11.0, f64::NAN, 16.0, 14.0, 13.0, 12.0];
        let close = [10.0, 11.0, 12.0, f64::NAN, 17.0, 15.0, 14.0, 13.0];
        let volume = [1.0; 8];
        let cci_values = cci(&high, &low, &close, 3);
        let donchian_values = donchian(&high, &low, 3);
        let mfi_values = mfi(&high, &low, &close, &volume, 3);
        for row in 3..6 {
            assert!(cci_values[row].unwrap().is_nan(), "CCI row {row}");
            assert!(
                donchian_values[row].upper.unwrap().is_nan(),
                "Donchian row {row}"
            );
            assert!(mfi_values[row].unwrap().is_nan(), "MFI row {row}");
        }
        let compact_high = [11.0, 12.0, 13.0, 18.0, 16.0, 15.0, 14.0];
        let compact_low = [9.0, 10.0, 11.0, 16.0, 14.0, 13.0, 12.0];
        let compact_close = [10.0, 11.0, 12.0, 17.0, 15.0, 14.0, 13.0];
        assert_eq!(
            cci_values[6],
            cci(&compact_high, &compact_low, &compact_close, 3)[5]
        );
        assert_eq!(
            donchian_values[6],
            donchian(&compact_high, &compact_low, 3)[5]
        );
        // The first flow window starting after the gap includes row 4. Its upward
        // direction compares typical 17 against the last valid 12, not NaN at row 3.
        assert_eq!(
            mfi_values[6],
            mfi(&compact_high, &compact_low, &compact_close, &volume[..7], 3)[5]
        );
    }

    #[test]
    fn window_kinds_blank_until_their_input_windows_clear() {
        // All 26 window-rule entries from VAL-KI-008. ZigZag has no fixed window;
        // its historical confirmations retain their existing sparse-point rule.
        let kinds = [
            TestKind::Sma,
            TestKind::Wma,
            TestKind::ChandeMomentum,
            TestKind::Momentum,
            TestKind::RateOfChange,
            TestKind::Kst,
            TestKind::CoppockCurve,
            TestKind::HistoricalVolatility,
            TestKind::Dpo,
            TestKind::StandardDeviation,
            TestKind::Bollinger,
            TestKind::BollingerMetrics,
            TestKind::LinearRegression,
            TestKind::Hma,
            TestKind::Alma,
            TestKind::EnvelopesSma,
            TestKind::WilliamsR,
            TestKind::Stochastic,
            TestKind::UltimateOscillator,
            TestKind::Vortex,
            TestKind::Aroon,
            TestKind::Choppiness,
            TestKind::Cmf,
            TestKind::EaseOfMovement,
            TestKind::Vwma,
            TestKind::ZigZag,
        ];
        // Lookback is the greatest number of preceding physical rows a formula reads,
        // including nested ROC/smoothing and preceding-close references. In particular
        // Coppock uses a 7-row ROC followed by a 3-row WMA (7 + 3 - 1).
        let lookback = |kind, output| match kind {
            TestKind::Momentum | TestKind::RateOfChange | TestKind::ChandeMomentum => 5,
            TestKind::CoppockCurve => 9,
            TestKind::Kst if output == 0 => 7, // ROC 5 + smoothing 3 - 1
            TestKind::Kst => 9,                // plus signal 3 - 1
            TestKind::Stochastic if output == 0 => 4,
            TestKind::Stochastic => 6,
            TestKind::UltimateOscillator => 7,
            TestKind::HistoricalVolatility
            | TestKind::Hma
            | TestKind::EaseOfMovement
            | TestKind::Vortex
            | TestKind::Choppiness
            | TestKind::Aroon => 5,
            _ => 4,
        };
        for (n, gap) in [
            (220, 75..76),      // single interior row
            (220, 75..77),      // consecutive rows
            (220, 75..80),      // exactly the five-row lag
            (220, 75..83),      // longer than the lag
            (220, 75..109),     // exactly the largest 34-row moving window
            (220, 75..111),     // longer than the largest moving window
            (220, 3..5),        // warm-up
            (1100, 1023..1026), // across a checkpoint
        ] {
            let times = (0..n as i64).collect::<Vec<_>>();
            let mut close = (0..n)
                .map(|row| 100.0 + (row as f64 * 0.33).sin() * 2.0 + row as f64 * 0.13)
                .collect::<Vec<_>>();
            let mut high = close.iter().map(|v| v + 1.0).collect::<Vec<_>>();
            let mut low = close.iter().map(|v| v - 1.0).collect::<Vec<_>>();
            let volume = (0..n).map(|row| (row % 11 + 2) as f64).collect::<Vec<_>>();
            for row in gap.clone() {
                close[row] = f64::NAN;
                high[row] = f64::NAN;
                low[row] = f64::NAN;
            }
            let input = IndicatorInput {
                times: &times,
                open: &close,
                high: &high,
                low: &low,
                close: &close,
                volume: &volume,
            };
            let kept = (0..n)
                .filter(|&row| close[row].is_finite())
                .collect::<Vec<_>>();
            let compact_times = kept.iter().map(|&row| times[row]).collect::<Vec<_>>();
            let compact_close = kept.iter().map(|&row| close[row]).collect::<Vec<_>>();
            let compact_high = kept.iter().map(|&row| high[row]).collect::<Vec<_>>();
            let compact_low = kept.iter().map(|&row| low[row]).collect::<Vec<_>>();
            let compact_volume = kept.iter().map(|&row| volume[row]).collect::<Vec<_>>();
            let compact = IndicatorInput {
                times: &compact_times,
                open: &compact_close,
                high: &compact_high,
                low: &compact_low,
                close: &compact_close,
                volume: &compact_volume,
            };
            for kind in kinds {
                let full = expected(kind, input);
                let deleted = expected(kind, compact);
                let mut states = all_test_states();
                let state = &mut states
                    .iter_mut()
                    .find(|(candidate, _)| *candidate == kind)
                    .expect("window kind has an incremental state")
                    .1;
                state.rebuild_from(input, 0);
                for (output, values) in full.iter().enumerate() {
                    assert!(
                        gap.clone()
                            .all(|row| !values[row].is_some_and(f64::is_finite)),
                        "{kind:?} output {output} emits in {gap:?}"
                    );
                    for row in gap.clone() {
                        if row >= state.output_from(output) {
                            assert!(
                                state.output(output)[row - state.output_from(output)].is_nan(),
                                "{kind:?} incremental output {output} emits in {gap:?} at {row}"
                            );
                        }
                    }
                    if matches!(kind, TestKind::ZigZag) {
                        continue; // historical confirmations have no fixed window
                    }
                    let lag = lookback(kind, output);
                    for (row, value) in values
                        .iter()
                        .enumerate()
                        .take((gap.end + lag).min(n))
                        .skip(gap.end)
                    {
                        assert!(
                            !value.is_some_and(f64::is_finite),
                            "{kind:?} output {output} reads {gap:?} at row {row}"
                        );
                        if row >= state.output_from(output) {
                            assert!(
                                state.output(output)[row - state.output_from(output)].is_nan(),
                                "{kind:?} incremental output {output} reads {gap:?} at row {row}"
                            );
                        }
                    }
                    // Check the FIRST cleared row and EVERY subsequent row, rather
                    // than sampling a late row that hides positional lag errors.
                    for (row, value) in values.iter().enumerate().skip(gap.end + lag) {
                        let observed = value.filter(|v| v.is_finite());
                        let compact_row = row - (gap.end - gap.start);
                        let want = deleted[output][compact_row].filter(|v| v.is_finite());
                        match (observed, want) {
                            (None, None) => {}
                            (Some(a), Some(b)) if (a - b).abs() <= 1e-9 * b.abs().max(1.0) => {}
                            _ => panic!(
                                "{kind:?} output {output} gap {gap:?} row {row}: {observed:?} != {want:?}"
                            ),
                        }
                        let actual = state.output(output)[row - state.output_from(output)];
                        assert!(
                            (actual.is_nan() && want.is_none())
                                || want
                                    .is_some_and(|v| (actual - v).abs() <= 1e-9 * v.abs().max(1.0)),
                            "{kind:?} incremental gap {gap:?} row {row}: {actual:?} != {want:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn kst_noncontiguous_lag_windows_follow_deleted_rows_oracle() {
        let roc = [10, 15, 20, 30];
        let smooth = [2; 4];
        let signal = 3;
        for gap in [3..5, 75..77] {
            let mut close = (0..130)
                .map(|row| 100.0 + row as f64 * 0.13 + (row as f64 * 0.33).sin() * 2.0)
                .collect::<Vec<_>>();
            for row in gap.clone() {
                close[row] = f64::NAN;
            }
            let compact = close
                .iter()
                .copied()
                .filter(|v| v.is_finite())
                .collect::<Vec<_>>();
            let oracle = kst(&compact, roc, smooth, signal);
            let dense = kst(&close, roc, smooth, signal);
            let times = (0..close.len() as i64).collect::<Vec<_>>();
            let volume = vec![1.0; close.len()];
            let mut incremental = IncrementalState::kst(roc, smooth, signal);
            incremental.rebuild_from(
                IndicatorInput {
                    times: &times,
                    open: &close,
                    high: &close,
                    low: &close,
                    close: &close,
                    volume: &volume,
                },
                0,
            );
            for (row, point) in dense.iter().enumerate() {
                let compact_row = row - (gap.end - gap.start).min(row.saturating_sub(gap.start));
                for (output, actual) in [point.line, point.signal].into_iter().enumerate() {
                    let want = if gap.contains(&row) {
                        None
                    } else {
                        let lookback = roc
                            .into_iter()
                            .zip(smooth)
                            .map(|(lag, width)| lag + width - 1)
                            .max()
                            .unwrap()
                            + if output == 1 { signal - 1 } else { 0 };
                        if row >= gap.end && row - gap.end < lookback {
                            None
                        } else {
                            let point = oracle[compact_row];
                            if output == 0 {
                                point.line
                            } else {
                                point.signal
                            }
                        }
                    };
                    let check = |value: Option<f64>, path: &str| {
                        let observed = value.filter(|v| v.is_finite());
                        let expected = want.filter(|v| v.is_finite());
                        assert!(
                            matches!((observed, expected), (None, None) | (Some(_), Some(_)))
                                && observed
                                    .zip(expected)
                                    .is_none_or(|(a, b)| (a - b).abs() <= 1e-9 * b.abs().max(1.0)),
                            "KST {path} gap {gap:?} row {row} output {output}: {observed:?} != {expected:?}"
                        );
                    };
                    check(actual, "dense");
                    let from = incremental.output_from(output);
                    check(
                        (row >= from).then(|| incremental.output(output)[row - from]),
                        "incremental",
                    );
                }
            }
        }
    }

    #[test]
    fn stochastic_flat_run_after_gap_carries_compacted_k() {
        for (period, previous, flat, carried) in [(5, 75.0, 8.0, 0.0), (14, 48.633, 12.0, 100.0)] {
            let len = period * 3 + 6;
            let mut high = vec![flat; len];
            let mut low = vec![flat; len];
            let mut close = vec![flat; len];
            for row in 0..period + 2 {
                high[row] = 12.0;
                low[row] = 8.0;
                close[row] = if row == period + 1 {
                    8.0 + previous * 0.04
                } else {
                    10.0
                };
            }
            let gap = period + 2;
            high[gap] = f64::NAN;
            low[gap] = f64::NAN;
            close[gap] = f64::NAN;
            let kept = (0..len).filter(|&row| row != gap).collect::<Vec<_>>();
            let compact_high = kept.iter().map(|&row| high[row]).collect::<Vec<_>>();
            let compact_low = kept.iter().map(|&row| low[row]).collect::<Vec<_>>();
            let compact_close = kept.iter().map(|&row| close[row]).collect::<Vec<_>>();
            let oracle = stochastic(&compact_high, &compact_low, &compact_close, period, 3);
            let dense = stochastic(&high, &low, &close, period, 3);
            let times = (0..len as i64).collect::<Vec<_>>();
            let volume = vec![1.0; len];
            let input = IndicatorInput {
                times: &times,
                open: &close,
                high: &high,
                low: &low,
                close: &close,
                volume: &volume,
            };
            let mut state = IncrementalState::stochastic(period, 3);
            state.rebuild_from(input, 0);
            let mut repaired = IncrementalState::stochastic(period, 3);
            let mut uninterrupted_high = high.clone();
            let mut uninterrupted_low = low.clone();
            let mut uninterrupted_close = close.clone();
            uninterrupted_high[gap] = 10.0;
            uninterrupted_low[gap] = 10.0;
            uninterrupted_close[gap] = 10.0;
            repaired.rebuild_from(
                IndicatorInput {
                    times: &times,
                    open: &uninterrupted_close,
                    high: &uninterrupted_high,
                    low: &uninterrupted_low,
                    close: &uninterrupted_close,
                    volume: &volume,
                },
                0,
            );
            repaired.rebuild_from(input, gap);
            for row in gap + period..len {
                let expected = oracle[row - 1];
                assert_eq!(expected.k, Some(carried));
                for (output, want, got) in [
                    (0, expected.k, dense[row].k),
                    (
                        1,
                        (row >= gap + period + 2).then_some(expected.d).flatten(),
                        dense[row].d,
                    ),
                ] {
                    assert!(
                        matches!((got.filter(|v| v.is_finite()), want), (None, None))
                            || got
                                .filter(|v| v.is_finite())
                                .zip(want)
                                .is_some_and(|(a, b)| (a - b).abs() < 1e-9),
                        "dense period {period} row {row} output {output}: {got:?} != {want:?}"
                    );
                    let actual = state.output(output)[row - state.output_from(output)];
                    assert!(
                        want.is_some_and(|expected| (actual - expected).abs() < 1e-9)
                            || (want.is_none() && actual.is_nan()),
                        "incremental period {period} row {row} output {output}: {actual} != {want:?}"
                    );
                    let corrected = repaired.output(output)[row - repaired.output_from(output)];
                    assert!(
                        want.is_some_and(|expected| (corrected - expected).abs() < 1e-9)
                            || (want.is_none() && corrected.is_nan()),
                        "correction period {period} row {row} output {output}: {corrected} != {want:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn zero_volume_cmf_gap_and_flat_historical_volatility_recover() {
        let high = [12.0, 12.0, f64::NAN, 12.0, 12.0, 12.0, 12.0];
        let low = [8.0, 8.0, f64::NAN, 8.0, 8.0, 8.0, 8.0];
        let close = [10.0, 10.0, f64::NAN, 10.0, 10.0, 10.0, 10.0];
        let volume = [0.0; 7];
        let cmf_values = cmf(&high, &low, &close, &volume, 3);
        assert!(cmf_values[3].unwrap().is_nan());
        assert!(cmf_values[4].unwrap().is_nan());
        assert_eq!(cmf_values[5], Some(0.0));
        let hv = historical_volatility(&close, 3, 252.0);
        assert!(hv[4].unwrap().is_nan());
        assert_eq!(hv[6], Some(0.0));
        // A constant *nonzero* log return has zero variance too. Without the
        // centered-window calculation, the subtraction of two large sums
        // leaves a spurious positive historical volatility after the gap.
        let mut growing = (0..32)
            .map(|row| (0.001 * row as f64).exp())
            .collect::<Vec<_>>();
        growing[8] = f64::NAN;
        let recovered = historical_volatility(&growing, 5, 252.0);
        for (row, value) in recovered.iter().enumerate().skip(14) {
            assert!(
                value.is_some_and(|value| value < 1e-10),
                "row {row}: {:?}",
                value
            );
        }
    }

    #[test]
    fn sliding_window_states_match_dense_on_seeded_degenerate_series() {
        // Fixed seed, including long flat runs straddling the 1,024-row
        // checkpoint, near-flat and large prices, alternating moves, and
        // gaps. Check every physical row, not just the final recovered row.
        let window_kinds = [
            TestKind::Sma,
            TestKind::Wma,
            TestKind::AwesomeOscillator,
            TestKind::Dpo,
            TestKind::ChandeMomentum,
            TestKind::Hma,
            TestKind::Vwma,
            TestKind::StandardDeviation,
            TestKind::Cci,
            TestKind::WilliamsR,
            TestKind::StochasticRsi,
            TestKind::Donchian,
            TestKind::Ichimoku,
            TestKind::Bollinger,
            TestKind::BollingerMetrics,
            TestKind::EnvelopesSma,
            TestKind::Alma,
            TestKind::Stochastic,
            TestKind::RelativeVolume,
            TestKind::EaseOfMovement,
            TestKind::HistoricalVolatility,
            TestKind::Kst,
            TestKind::MassIndex,
            TestKind::Kama,
            TestKind::LinearRegression,
            TestKind::Choppiness,
            TestKind::CoppockCurve,
            TestKind::FisherTransform,
            TestKind::UltimateOscillator,
            TestKind::Vortex,
            TestKind::Aroon,
            TestKind::Cmf,
            TestKind::Mfi,
            TestKind::Volume,
        ];
        let n = 1300;
        let times = (0..n as i64).map(|row| row * 3_600).collect::<Vec<_>>();
        for shape in 0..4 {
            for gaps in [false, true] {
                let mut seed = 0xc015_5206_u64;
                let mut close = Vec::with_capacity(n);
                let mut high = Vec::with_capacity(n);
                let mut low = Vec::with_capacity(n);
                let mut volume = Vec::with_capacity(n);
                for row in 0..n {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    let small = (seed % 11) as f64 - 5.0;
                    let price = match shape {
                        0 => 100.0 + ((row / 109) % 3) as f64 * 0.01,
                        1 => 100.0 + small * 1e-8,
                        2 => 1e6 + small * 1e-7,
                        _ => 1e6 + if row % 2 == 0 { 0.001 } else { -0.001 },
                    };
                    let gap = gaps && [7, 171, 333, 899, 1021, 1022, 1090].contains(&row);
                    close.push(if gap { f64::NAN } else { price });
                    high.push(if gap { f64::NAN } else { price + 0.5 });
                    low.push(if gap { f64::NAN } else { price - 0.5 });
                    volume.push(if row % 17 == 0 {
                        0.0
                    } else {
                        (seed % 47 + 1) as f64
                    });
                }
                let input = IndicatorInput {
                    times: &times,
                    open: &close,
                    high: &high,
                    low: &low,
                    close: &close,
                    volume: &volume,
                };
                for (kind, mut state) in all_test_states()
                    .into_iter()
                    .filter(|(kind, _)| window_kinds.contains(kind))
                {
                    state.rebuild_from(input, 0);
                    let dense = expected(kind, input);
                    for (output, expected) in dense.iter().enumerate() {
                        for (row, &value) in
                            expected.iter().enumerate().skip(state.output_from(output))
                        {
                            let actual = state.output(output)[row - state.output_from(output)];
                            match value {
                                Some(value) if value.is_finite() => assert!(
                                    actual.is_finite()
                                        && (actual - value).abs()
                                            <= 1e-12_f64.max(1e-9 * value.abs()),
                                    "{kind:?} shape={shape} gaps={gaps} out={output} row={row}: {actual} != {value}"
                                ),
                                _ => assert!(
                                    actual.is_nan(),
                                    "{kind:?} shape={shape} gaps={gaps} out={output} row={row}: expected whitespace, got {actual}"
                                ),
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn live_bollinger_metrics_and_cmo_do_not_retain_flat_window_residue() {
        let n = 1_280;
        let times = (0..n as i64).collect::<Vec<_>>();
        let mut close = (0..n)
            .map(|row| {
                if (350..700).contains(&row) || (900..n).contains(&row) {
                    100.0
                } else if (700..900).contains(&row) {
                    100.0 + (row % 3) as f64 * 1e-8
                } else {
                    100.0 + ((row * 13 % 37) as f64 - 18.0) * 0.01
                }
            })
            .collect::<Vec<_>>();
        let volume = vec![1.0; n];
        for gaps in [false, true] {
            if gaps {
                close[750] = f64::NAN;
                close[1022] = f64::NAN;
            }
            for (mut state, dense) in [
                (IncrementalState::bollinger_metrics(3, 1.35), {
                    let points = bollinger_metrics(&close, 3, 1.35);
                    [
                        points.iter().map(|point| point.0).collect::<Vec<_>>(),
                        points.iter().map(|point| point.1).collect(),
                    ]
                }),
                (
                    IncrementalState::chande_momentum(7),
                    [chande_momentum(&close, 7), vec![]],
                ),
            ] {
                for row in 0..n {
                    let input = IndicatorInput {
                        times: &times[..=row],
                        open: &close[..=row],
                        high: &close[..=row],
                        low: &close[..=row],
                        close: &close[..=row],
                        volume: &volume[..=row],
                    };
                    state.rebuild_from(input, row);
                    for (column, expected) in dense.iter().enumerate().take(state.output_count()) {
                        if row < state.output_from(column) {
                            continue;
                        }
                        let actual = state.output(column)[row - state.output_from(column)];
                        if let Some(expected) = expected[row].filter(|value| value.is_finite()) {
                            assert!(
                                actual.is_finite()
                                    && (actual - expected).abs()
                                        <= 1e-12_f64.max(1e-9 * expected.abs()),
                                "{gaps} row={row} col={column}: {actual} != {expected}"
                            );
                        } else {
                            assert!(actual.is_nan(), "{gaps} row={row} col={column}: {actual}");
                        }
                    }
                }
                // A historical edit overlapping a sparse checkpoint has the
                // same value as a fresh dense rebuild, not a stale tail sum.
                state.rebuild_from(
                    IndicatorInput {
                        times: &times,
                        open: &close,
                        high: &close,
                        low: &close,
                        close: &close,
                        volume: &volume,
                    },
                    1021,
                );
                for (column, expected) in dense.iter().enumerate().take(state.output_count()) {
                    for row in 1021..n {
                        let actual = state.output(column)[row - state.output_from(column)];
                        if let Some(expected) = expected[row].filter(|value| value.is_finite()) {
                            assert!(
                                (actual - expected).abs() <= 1e-12_f64.max(1e-9 * expected.abs()),
                                "repair {gaps} row={row} col={column}: {actual} != {expected}"
                            );
                        } else {
                            assert!(actual.is_nan());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn stochastic_flat_carry_survives_sparse_checkpoint_repair() {
        let n = 2200;
        let period = 14;
        let gap = 1090;
        let mut high = vec![12.0; n];
        let mut low = vec![8.0; n];
        let mut close = vec![11.0; n];
        for row in gap + 1..n {
            high[row] = 10.0;
            low[row] = 10.0;
            close[row] = 10.0;
        }
        high[gap] = f64::NAN;
        low[gap] = f64::NAN;
        close[gap] = f64::NAN;
        let times = (0..n as i64).collect::<Vec<_>>();
        let volume = vec![1.0; n];
        let input = IndicatorInput {
            times: &times,
            open: &close,
            high: &high,
            low: &low,
            close: &close,
            volume: &volume,
        };
        let mut state = IncrementalState::stochastic(period, 3);
        state.rebuild_from(input, 0);
        state.rebuild_from(input, gap);
        assert!(state.last_work_rows() <= n - gap + CHECKPOINT_INTERVAL);
        let without_gap = |values: &[f64]| {
            values[..gap]
                .iter()
                .chain(&values[gap + 1..])
                .copied()
                .collect::<Vec<_>>()
        };
        let oracle = stochastic(
            &without_gap(&high),
            &without_gap(&low),
            &without_gap(&close),
            period,
            3,
        );
        for row in gap + period + 2..n {
            let k = state.output(0)[row - state.output_from(0)];
            let d = state.output(1)[row - state.output_from(1)];
            assert_eq!(
                (k, d),
                (oracle[row - 1].k.unwrap(), oracle[row - 1].d.unwrap())
            );
        }
    }

    #[test]
    fn seeded_parameterized_gap_oracle_covers_window_and_continue_kinds() {
        // A compact (gap rows deleted) run is independent of whitespace handling.
        // Fixed seeds and small bounded series keep the complete catalog inexpensive.
        let kinds = all_test_states();
        assert_eq!(kinds.len(), 64, "update the oracle when a kind is added");
        for seed in [0x4b53_5421_u64, 0x8acd_2026] {
            let mut random = seed;
            let mut next = || {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                random
            };
            for (kind, _) in &kinds {
                // Valid short and long periods, plus independent multi-term settings.
                let period = if next() & 1 == 0 {
                    3 + next() as usize % 8
                } else {
                    18 + next() as usize % 15
                };
                let second = 2 + next() as usize % 6;
                let third = 2 + next() as usize % 5;
                for shape in 0..10 {
                    let n = 155;
                    let mut close = (0..n)
                        .map(|row| {
                            100.0
                                + row as f64 * 0.09
                                + (row as f64 * 0.29 + seed as f64 * 1e-9).sin() * 2.0
                        })
                        .collect::<Vec<_>>();
                    let mut high = close.iter().map(|v| v + 1.4).collect::<Vec<_>>();
                    let mut low = close.iter().map(|v| v - 1.1).collect::<Vec<_>>();
                    let mut volume = (0..n)
                        .map(|row| 3.0 + (row * 7 % 19) as f64)
                        .collect::<Vec<_>>();
                    let times = (0..n as i64).map(|row| row * 3_600).collect::<Vec<_>>();
                    let middle = 64 + next() as usize % 16;
                    // Retain real candles for the baseline repair, including degenerate
                    // candles; the only difference in that path is the introduced gaps.
                    match shape {
                        5 => {
                            for row in middle - 40..middle + 42 {
                                high[row] = 100.0;
                                low[row] = 100.0;
                                close[row] = 100.0;
                            }
                        }
                        6 => {
                            for row in middle - 40..middle {
                                high[row] = 102.0;
                                low[row] = 98.0;
                                close[row] = 101.0;
                            }
                            for row in middle + 1..middle + 42 {
                                high[row] = 100.0;
                                low[row] = 100.0;
                                close[row] = 100.0;
                            }
                        }
                        7 => {
                            close.fill(100.0);
                            for row in middle - 38..middle + 40 {
                                high[row] = 100.0;
                                low[row] = 100.0;
                            }
                        }
                        8 => {
                            volume[middle - 42..middle + 42].fill(0.0);
                        }
                        9 => {
                            for row in middle - 38..middle + 40 {
                                close[row] = 100.0;
                                high[row] = 100.0;
                                low[row] = 100.0;
                                volume[row] = 0.0;
                            }
                        }
                        _ => {}
                    }
                    let initial_close = close.clone();
                    let initial_high = high.clone();
                    let initial_low = low.clone();
                    let gaps: Vec<usize> = match shape {
                        0 => vec![middle],
                        1 => vec![middle, middle + 1],
                        2 => vec![1, 2, 3],
                        3 => vec![middle, middle + 1, middle + 4, middle + 5],
                        4 => vec![n - 4, n - 3],
                        _ => vec![middle],
                    };
                    for &row in &gaps {
                        close[row] = f64::NAN;
                        high[row] = f64::NAN;
                        low[row] = f64::NAN;
                    }
                    let input = IndicatorInput {
                        times: &times,
                        open: &close,
                        high: &high,
                        low: &low,
                        close: &close,
                        volume: &volume,
                    };
                    let kept = (0..n).filter(|row| !gaps.contains(row)).collect::<Vec<_>>();
                    let compact_times = kept.iter().map(|&row| times[row]).collect::<Vec<_>>();
                    let compact_close = kept.iter().map(|&row| close[row]).collect::<Vec<_>>();
                    let compact_high = kept.iter().map(|&row| high[row]).collect::<Vec<_>>();
                    let compact_low = kept.iter().map(|&row| low[row]).collect::<Vec<_>>();
                    let compact_volume = kept.iter().map(|&row| volume[row]).collect::<Vec<_>>();
                    let compact = IndicatorInput {
                        times: &compact_times,
                        open: &compact_close,
                        high: &compact_high,
                        low: &compact_low,
                        close: &compact_close,
                        volume: &compact_volume,
                    };
                    let (full, mut state) =
                        parameterized_gap_case(*kind, period, second, third, input);
                    let (deleted, _) =
                        parameterized_gap_case(*kind, period, second, third, compact);
                    let mut repaired = state.clone();
                    state.rebuild_from(input, 0);
                    repaired.rebuild_from(
                        IndicatorInput {
                            times: &times,
                            open: &initial_close,
                            high: &initial_high,
                            low: &initial_low,
                            close: &initial_close,
                            volume: &volume,
                        },
                        0,
                    );
                    repaired.rebuild_from(input, gaps[0]);
                    for (output, values) in full.iter().enumerate() {
                        let width = gap_window_width(*kind, output, period, second, third);
                        let mut compact_row = 0;
                        for (row, &value) in values.iter().enumerate() {
                            let is_gap = gaps.contains(&row);
                            let window_has_gap = width.is_some_and(|width| {
                                row + 1 >= width
                                    && gaps.iter().any(|&gap| gap <= row && row - gap < width)
                            });
                            let want = if is_gap || window_has_gap {
                                None
                            } else {
                                deleted[output][compact_row].filter(|v| v.is_finite())
                            };
                            if !is_gap {
                                compact_row += 1;
                            }
                            let check = |found: Option<f64>, path: &str| {
                                let found = found.filter(|v| v.is_finite());
                                assert!(
                                    matches!((found, want), (None, None) | (Some(_), Some(_)))
                                        && found.zip(want).is_none_or(|(a, b)| {
                                            (a - b).abs() <= 1e-9 * b.abs().max(1.0)
                                        }),
                                    "{kind:?} {path} seed {seed:#x} params ({period},{second},{third}) shape {shape} output {output} row {row}: {found:?} != {want:?}, width {width:?}"
                                );
                            };
                            check(value, "dense");
                            let from = state.output_from(output);
                            check(
                                (row >= from).then(|| state.output(output)[row - from]),
                                "incremental",
                            );
                            if row >= gaps[0] {
                                let from = repaired.output_from(output);
                                check(
                                    (row >= from).then(|| repaired.output(output)[row - from]),
                                    "incremental repair",
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn gap_window_width(
        kind: TestKind,
        output: usize,
        period: usize,
        second: usize,
        third: usize,
    ) -> Option<usize> {
        Some(match kind {
            TestKind::AwesomeOscillator => 34,
            TestKind::Ichimoku => [9, 26, 26, 52, 1][output],
            TestKind::Volume => {
                if output == 0 {
                    1
                } else {
                    period
                }
            }
            TestKind::RelativeVolume
            | TestKind::Aroon
            | TestKind::ChandeMomentum
            | TestKind::Momentum
            | TestKind::RateOfChange
            | TestKind::HistoricalVolatility
            | TestKind::Vortex
            | TestKind::Choppiness
            | TestKind::EaseOfMovement => period + 1,
            TestKind::UltimateOscillator => period + second + third + 1,
            TestKind::CoppockCurve => period + second + third,
            TestKind::Kst => period + 12 + (2 + second % 3) + if output == 1 { third } else { 0 },
            TestKind::Dpo => period,
            TestKind::Hma => period + (period as f64).sqrt() as usize - 1,
            TestKind::Stochastic if output == 1 => period + second - 1,
            TestKind::PivotPoints | TestKind::ZigZag => return None,
            TestKind::Sma
            | TestKind::Wma
            | TestKind::Vwma
            | TestKind::StandardDeviation
            | TestKind::Cci
            | TestKind::WilliamsR
            | TestKind::Donchian
            | TestKind::Bollinger
            | TestKind::BollingerMetrics
            | TestKind::EnvelopesSma
            | TestKind::Alma
            | TestKind::Stochastic
            | TestKind::LinearRegression
            | TestKind::Cmf
            | TestKind::Mfi => period,
            _ => return None, // Recursive/cumulative kinds use every compact row.
        })
    }

    fn parameterized_gap_case(
        kind: TestKind,
        period: usize,
        second: usize,
        third: usize,
        input: IndicatorInput<'_>,
    ) -> (Vec<Vec<Option<f64>>>, IncrementalState) {
        let c = input.close;
        let h = input.high;
        let l = input.low;
        let v = input.volume;
        let p = period;
        let q = second;
        let s = third;
        macro_rules! single {
            ($dense:expr, $incremental:expr) => {
                (vec![$dense], $incremental)
            };
        }
        match kind {
            TestKind::Sma => single!(sma(c, p), IncrementalState::sma(p)),
            TestKind::Wma => single!(wma(c, p), IncrementalState::wma(p)),
            TestKind::Vwma => single!(vwma(c, v, p), IncrementalState::vwma(p)),
            TestKind::Ema => single!(ema(c, p), IncrementalState::ema(p)),
            TestKind::Smma => single!(smma(c, p), IncrementalState::smma(p)),
            TestKind::Dema => single!(dema(c, p), IncrementalState::dema(p)),
            TestKind::Tema => single!(tema(c, p), IncrementalState::tema(p)),
            TestKind::Hma => single!(hma(c, p), IncrementalState::hma(p)),
            TestKind::Dpo => single!(dpo(c, p), IncrementalState::dpo(p)),
            TestKind::ChandeMomentum => {
                single!(chande_momentum(c, p), IncrementalState::chande_momentum(p))
            }
            TestKind::Momentum => single!(momentum(c, p), IncrementalState::momentum(p)),
            TestKind::RateOfChange => {
                single!(rate_of_change(c, p), IncrementalState::rate_of_change(p))
            }
            TestKind::StandardDeviation => single!(
                standard_deviation(c, p),
                IncrementalState::standard_deviation(p)
            ),
            TestKind::Cci => single!(cci(h, l, c, p), IncrementalState::cci(p)),
            TestKind::WilliamsR => single!(williams_r(h, l, c, p), IncrementalState::williams_r(p)),
            TestKind::Rsi => single!(rsi(c, p), IncrementalState::rsi(p)),
            TestKind::StochasticRsi => single!(
                stochastic_rsi(c, p, q),
                IncrementalState::stochastic_rsi(p, q)
            ),
            TestKind::Atr => single!(atr(h, l, c, p), IncrementalState::atr(p)),
            TestKind::AdxDmi => {
                let points = adx_dmi(h, l, c, p);
                (
                    vec![
                        points.iter().map(|point| point.plus_di).collect(),
                        points.iter().map(|point| point.minus_di).collect(),
                        points.iter().map(|point| point.adx).collect(),
                    ],
                    IncrementalState::adx_dmi(p),
                )
            }
            TestKind::SuperTrend => single!(
                supertrend(h, l, c, p, 3.0),
                IncrementalState::supertrend(p, 3.0)
            ),
            TestKind::AtrBands => {
                let points = atr_bands(h, l, c, p, 2.0);
                (
                    vec![
                        points.iter().map(|point| point.upper).collect(),
                        points.iter().map(|point| point.basis).collect(),
                        points.iter().map(|point| point.lower).collect(),
                    ],
                    IncrementalState::atr_bands(p, 2.0),
                )
            }
            TestKind::Keltner => {
                let points = keltner(h, l, c, p, 2.0);
                (
                    vec![
                        points.iter().map(|point| point.upper).collect(),
                        points.iter().map(|point| point.middle).collect(),
                        points.iter().map(|point| point.lower).collect(),
                    ],
                    IncrementalState::keltner(p, 2.0),
                )
            }
            TestKind::Donchian => {
                let points = donchian(h, l, p);
                (
                    vec![
                        points.iter().map(|point| point.upper).collect(),
                        points.iter().map(|point| point.middle).collect(),
                        points.iter().map(|point| point.lower).collect(),
                    ],
                    IncrementalState::donchian(p),
                )
            }
            TestKind::Bollinger | TestKind::BollingerMetrics => {
                let points = if kind == TestKind::Bollinger {
                    bollinger(c, p, 2.0)
                        .iter()
                        .map(|point| [point.upper, point.middle, point.lower])
                        .collect::<Vec<_>>()
                } else {
                    bollinger_metrics(c, p, 2.0)
                        .iter()
                        .map(|point| [point.0, point.1, None])
                        .collect::<Vec<_>>()
                };
                let count = if kind == TestKind::Bollinger { 3 } else { 2 };
                (
                    (0..count)
                        .map(|output| points.iter().map(|point| point[output]).collect())
                        .collect(),
                    if count == 3 {
                        IncrementalState::bollinger(p, 2.0)
                    } else {
                        IncrementalState::bollinger_metrics(p, 2.0)
                    },
                )
            }
            TestKind::EnvelopesSma | TestKind::EnvelopesEma => {
                let exponential = kind == TestKind::EnvelopesEma;
                let points = envelopes(c, p, 10.0, exponential);
                (
                    vec![
                        points.iter().map(|point| point.0).collect(),
                        points.iter().map(|point| point.1).collect(),
                        points.iter().map(|point| point.2).collect(),
                    ],
                    IncrementalState::envelopes(p, 10.0, exponential),
                )
            }
            TestKind::EmaRibbon => {
                let periods = [q, q + 2, q + 4, q + 7, p + q + 8];
                (
                    periods.iter().map(|&width| ema(c, width)).collect(),
                    IncrementalState::ema_ribbon(periods),
                )
            }
            TestKind::Alma => single!(alma(c, p, 0.85, 6.0), IncrementalState::alma(p, 0.85, 6.0)),
            TestKind::Macd => {
                let points = macd(c, q, p + q, s);
                (
                    vec![
                        points.iter().map(|point| point.macd).collect(),
                        points.iter().map(|point| point.signal).collect(),
                        points.iter().map(|point| point.histogram).collect(),
                    ],
                    IncrementalState::macd(q, p + q, s),
                )
            }
            TestKind::Stochastic => {
                let points = stochastic(h, l, c, p, q);
                (
                    vec![
                        points.iter().map(|point| point.k).collect(),
                        points.iter().map(|point| point.d).collect(),
                    ],
                    IncrementalState::stochastic(p, q),
                )
            }
            TestKind::Aroon => {
                let points = aroon(h, l, p);
                (
                    vec![
                        points.iter().map(|point| point.0).collect(),
                        points.iter().map(|point| point.1).collect(),
                    ],
                    IncrementalState::aroon(p),
                )
            }
            TestKind::RelativeVolume => {
                let masked = v
                    .iter()
                    .zip(c)
                    .map(|(&volume, &close)| if close.is_finite() { volume } else { f64::NAN })
                    .collect::<Vec<_>>();
                single!(
                    relative_volume(&masked, p),
                    IncrementalState::relative_volume(p)
                )
            }
            TestKind::Volume => {
                let values = v
                    .iter()
                    .zip(c)
                    .map(|(&volume, &close)| close.is_finite().then_some(volume.max(0.0)))
                    .collect::<Vec<_>>();
                let average = (0..c.len())
                    .map(|row| {
                        if row + 1 < p {
                            None
                        } else {
                            let window = &values[row + 1 - p..=row];
                            Some(if window.iter().all(Option::is_some) {
                                window.iter().map(|item| item.unwrap()).sum::<f64>() / p as f64
                            } else {
                                f64::NAN
                            })
                        }
                    })
                    .collect();
                (vec![values, average], IncrementalState::volume(p))
            }
            TestKind::ElderForce => single!(elder_force(c, v, p), IncrementalState::elder_force(p)),
            TestKind::ChaikinOscillator => single!(
                chaikin_oscillator(h, l, c, v, q, p + q),
                IncrementalState::chaikin_oscillator(q, p + q)
            ),
            TestKind::VolumeOscillator => {
                let masked = v
                    .iter()
                    .zip(c)
                    .map(|(&volume, &close)| if close.is_finite() { volume } else { f64::NAN })
                    .collect::<Vec<_>>();
                let points = volume_oscillator(&masked, q, p + q, s);
                (
                    vec![
                        points.iter().map(|point| point.line).collect(),
                        points.iter().map(|point| point.signal).collect(),
                        points.iter().map(|point| point.histogram).collect(),
                    ],
                    IncrementalState::volume_oscillator(q, p + q, s),
                )
            }
            TestKind::EaseOfMovement => single!(
                ease_of_movement(h, l, v, p, 100.0),
                IncrementalState::ease_of_movement(p, 100.0)
            ),
            TestKind::HistoricalVolatility => single!(
                historical_volatility(c, p, 252.0),
                IncrementalState::historical_volatility(p, 252.0)
            ),
            TestKind::MassIndex => {
                single!(mass_index(h, l, q, p), IncrementalState::mass_index(q, p))
            }
            TestKind::Trix => {
                let points = trix(c, q, s);
                (
                    vec![
                        points.iter().map(|point| point.line).collect(),
                        points.iter().map(|point| point.signal).collect(),
                    ],
                    IncrementalState::trix(q, s),
                )
            }
            TestKind::Tsi => {
                let points = tsi(c, p, q, s);
                (
                    vec![
                        points.iter().map(|point| point.line).collect(),
                        points.iter().map(|point| point.signal).collect(),
                    ],
                    IncrementalState::tsi(p, q, s),
                )
            }
            TestKind::Klinger => {
                let points = klinger(h, l, c, v, q, p + q, s);
                (
                    vec![
                        points.iter().map(|point| point.line).collect(),
                        points.iter().map(|point| point.signal).collect(),
                    ],
                    IncrementalState::klinger(q, p + q, s),
                )
            }
            TestKind::FisherTransform => {
                let points = fisher_transform(h, l, p);
                (
                    vec![
                        points.iter().map(|point| point.line).collect(),
                        points.iter().map(|point| point.trigger).collect(),
                    ],
                    IncrementalState::fisher_transform(p),
                )
            }
            TestKind::Kama => single!(kama(c, p, 2, p + q), IncrementalState::kama(p, 2, p + q)),
            TestKind::McGinley => single!(mcginley(c, p), IncrementalState::mcginley(p)),
            TestKind::LinearRegression => {
                let points = linear_regression(c, p, 2.0);
                (
                    vec![
                        points.iter().map(|point| point.curve).collect(),
                        points.iter().map(|point| point.upper).collect(),
                        points.iter().map(|point| point.lower).collect(),
                    ],
                    IncrementalState::linear_regression(p, 2.0),
                )
            }
            TestKind::Choppiness => {
                single!(choppiness(h, l, c, p), IncrementalState::choppiness(p))
            }
            TestKind::Cmf => single!(cmf(h, l, c, v, p), IncrementalState::cmf(p)),
            TestKind::Mfi => single!(mfi(h, l, c, v, p), IncrementalState::mfi(p)),
            TestKind::Vortex => {
                let points = vortex(h, l, c, p);
                (
                    vec![
                        points.iter().map(|point| point.plus).collect(),
                        points.iter().map(|point| point.minus).collect(),
                    ],
                    IncrementalState::vortex(p),
                )
            }
            TestKind::UltimateOscillator => single!(
                ultimate_oscillator(h, l, c, q, q + s, p + q + s),
                IncrementalState::ultimate_oscillator(q, q + s, p + q + s)
            ),
            TestKind::CoppockCurve => single!(
                coppock_curve(c, p + q, p, s),
                IncrementalState::coppock_curve(p + q, p, s)
            ),
            TestKind::Kst => {
                let roc = [p, p + 4, p + 8, p + 12];
                let smooth = [2 + q % 3; 4];
                let points = kst(c, roc, smooth, s + 1);
                (
                    vec![
                        points.iter().map(|point| point.line).collect(),
                        points.iter().map(|point| point.signal).collect(),
                    ],
                    IncrementalState::kst(roc, smooth, s + 1),
                )
            }
            _ => {
                let state = all_test_states()
                    .into_iter()
                    .find(|(candidate, _)| *candidate == kind)
                    .unwrap()
                    .1;
                (expected(kind, input), state)
            }
        }
    }

    #[test]
    fn previously_unlisted_kinds_follow_declared_gap_rules() {
        // Awesome Oscillator: two median-price SMA windows (maximum 34 rows).
        // Ichimoku: 9/26/52-row extrema windows and instantaneous lagging close.
        // Pivot Points: session state continues over missing rows, never seeds
        // from a missing row; current gap output is blank.
        // Relative Volume: current volume plus the previous 5-row window.
        // Volume Oscillator: recursive EMAs continue using valid volume observations.
        // Volume: instantaneous volume, and a 5-row volume-average window.
        #[derive(Clone, Copy)]
        enum GapRule {
            Continue,
            Window { width: usize },
            Session,
        }
        let kinds = [
            (TestKind::AwesomeOscillator, GapRule::Window { width: 34 }),
            (TestKind::Ichimoku, GapRule::Window { width: 52 }),
            (TestKind::PivotPoints, GapRule::Session),
            (TestKind::RelativeVolume, GapRule::Window { width: 6 }),
            (TestKind::VolumeOscillator, GapRule::Continue),
            (TestKind::Volume, GapRule::Window { width: 5 }),
        ];
        let n = 225;
        let times = (0..n as i64).map(|row| row * 3_600).collect::<Vec<_>>();
        let mut close = (0..n)
            .map(|row| 100.0 + row as f64 * 0.15 + (row as f64 * 0.23).sin())
            .collect::<Vec<_>>();
        let mut high = close.iter().map(|v| v + 1.0).collect::<Vec<_>>();
        let mut low = close.iter().map(|v| v - 1.0).collect::<Vec<_>>();
        let volume = (0..n).map(|row| (row % 11 + 2) as f64).collect::<Vec<_>>();
        for row in 75..77 {
            close[row] = f64::NAN;
            high[row] = f64::NAN;
            low[row] = f64::NAN;
        }
        let input = IndicatorInput {
            times: &times,
            open: &close,
            high: &high,
            low: &low,
            close: &close,
            volume: &volume,
        };
        let kept = (0..n)
            .filter(|&row| close[row].is_finite())
            .collect::<Vec<_>>();
        let compact_times = kept.iter().map(|&row| times[row]).collect::<Vec<_>>();
        let compact_close = kept.iter().map(|&row| close[row]).collect::<Vec<_>>();
        let compact_high = kept.iter().map(|&row| high[row]).collect::<Vec<_>>();
        let compact_low = kept.iter().map(|&row| low[row]).collect::<Vec<_>>();
        let compact_volume = kept.iter().map(|&row| volume[row]).collect::<Vec<_>>();
        let compact = IndicatorInput {
            times: &compact_times,
            open: &compact_close,
            high: &compact_high,
            low: &compact_low,
            close: &compact_close,
            volume: &compact_volume,
        };
        for (kind, rule) in kinds {
            let actual = expected(kind, input);
            let deleted = expected(kind, compact);
            for (output, values) in actual.iter().enumerate() {
                for (row, &value) in values.iter().enumerate().take(77).skip(75) {
                    assert!(
                        !value.is_some_and(f64::is_finite),
                        "{kind:?} output {output} at gap {row}"
                    );
                }
                let first = match rule {
                    GapRule::Continue | GapRule::Session => 77,
                    GapRule::Window { width } => 76 + width,
                };
                for row in first..n {
                    let observed = values[row].filter(|v| v.is_finite());
                    let want = deleted[output][row - 2].filter(|v| v.is_finite());
                    match (observed, want) {
                        (None, None) => {}
                        (Some(a), Some(b)) if (a - b).abs() <= 1e-9 * b.abs().max(1.0) => {}
                        _ => panic!("{kind:?} output {output} row {row}: {observed:?} != {want:?}"),
                    }
                }
            }
        }
    }

    fn gap_family_oracle(kinds: &[(TestKind, IncrementalState)]) {
        let n = 1100;
        let times = (0..n as i64).collect::<Vec<_>>();
        let mut close = (0..n)
            .map(|row| 100.0 + (row as f64 * 0.43).sin() * 3.0 + row as f64 * 0.05)
            .collect::<Vec<_>>();
        let mut high = close.iter().map(|v| v + 1.4).collect::<Vec<_>>();
        let mut low = close.iter().map(|v| v - 1.2).collect::<Vec<_>>();
        let volume = (0..n).map(|row| (row % 13 + 1) as f64).collect::<Vec<_>>();
        for &gap in &[3, 29, 70, 71, 72, 1023, 1024, 1025] {
            close[gap] = f64::NAN;
            high[gap] = f64::NAN;
            low[gap] = f64::NAN;
        }
        // Inverted but finite ranges are observations; a missing required field is not.
        high[8] = low[8] - 0.5;
        high[31] = f64::NAN;
        low[37] = f64::NAN;
        close[43] = f64::NAN;
        for &(kind, ref fresh_state) in kinds {
            let mut state = fresh_state.clone();
            let check =
                |state: &mut IncrementalState, close: &[f64], high: &[f64], low: &[f64], from| {
                    assert_continues_across_gaps(
                        kind,
                        state,
                        IndicatorInput {
                            times: &times,
                            open: close,
                            high,
                            low,
                            close,
                            volume: &volume,
                        },
                        from,
                    );
                };
            // Each prefix is an append, including the whitespace tip and the first post-gap row.
            for len in 1..=36 {
                assert_continues_across_gaps(
                    kind,
                    &mut state,
                    IndicatorInput {
                        times: &times[..len],
                        open: &close[..len],
                        high: &high[..len],
                        low: &low[..len],
                        close: &close[..len],
                        volume: &volume[..len],
                    },
                    len - 1,
                );
            }
            check(&mut state, &close, &high, &low, 36);
            close[1099] = f64::NAN;
            high[1099] = f64::NAN;
            low[1099] = f64::NAN;
            check(&mut state, &close, &high, &low, 1099);
            close[1099] = 151.0;
            high[1099] = 153.0;
            low[1099] = 149.0;
            check(&mut state, &close, &high, &low, 1099);
            close[1024] = 124.0;
            high[1024] = 126.0;
            low[1024] = 122.0;
            check(&mut state, &close, &high, &low, 1024);
            close[1010] = f64::NAN;
            high[1010] = f64::NAN;
            low[1010] = f64::NAN;
            check(&mut state, &close, &high, &low, 1010);
            close[1010] = 138.0;
            high[1010] = 140.0;
            low[1010] = 136.0;
            check(&mut state, &close, &high, &low, 1010);
            // Restore the shared fixture for the next kind.
            close[1099] = 100.0 + (1099.0_f64 * 0.43).sin() * 3.0 + 1099.0 * 0.05;
            high[1099] = close[1099] + 1.4;
            low[1099] = close[1099] - 1.2;
            close[1024] = f64::NAN;
            high[1024] = f64::NAN;
            low[1024] = f64::NAN;
            close[1010] = 100.0 + (1010.0_f64 * 0.43).sin() * 3.0 + 1010.0 * 0.05;
            high[1010] = close[1010] + 1.4;
            low[1010] = close[1010] - 1.2;
        }
    }

    #[test]
    fn ohlc_batch_matches_incremental_with_inverted_and_partial_whitespace_bars() {
        let mut states = all_test_states();
        let n = 120;
        let times = (0..n as i64).map(|row| row * 3_600).collect::<Vec<_>>();
        let mut close = (0..n)
            .map(|row| 100.0 + row as f64 * 0.3)
            .collect::<Vec<_>>();
        let mut high = close.iter().map(|v| v + 1.4).collect::<Vec<_>>();
        let mut low = close.iter().map(|v| v - 1.2).collect::<Vec<_>>();
        let volume = vec![10.0; n];
        high[8] = low[8] - 0.5;
        high[31] = f64::NAN;
        low[37] = f64::NAN;
        close[43] = f64::NAN;
        let kinds = [
            TestKind::Atr,
            TestKind::Keltner,
            TestKind::AdxDmi,
            TestKind::SuperTrend,
            TestKind::ParabolicSar,
            TestKind::AtrBands,
            TestKind::Cci,
            TestKind::WilliamsR,
            TestKind::Stochastic,
            TestKind::Donchian,
            TestKind::Vwap,
            TestKind::VwapBands,
            TestKind::AccumulationDistribution,
            TestKind::ChaikinOscillator,
            TestKind::Klinger,
            TestKind::MassIndex,
            TestKind::FisherTransform,
            TestKind::UltimateOscillator,
            TestKind::Vortex,
            TestKind::Mfi,
            TestKind::Cmf,
            TestKind::Choppiness,
            TestKind::EaseOfMovement,
            TestKind::Aroon,
            TestKind::ZigZag,
            TestKind::PivotPoints,
            TestKind::Ichimoku,
            TestKind::AwesomeOscillator,
        ];
        states.retain(|(kind, _)| {
            kinds
                .iter()
                .any(|candidate| std::mem::discriminant(candidate) == std::mem::discriminant(kind))
        });
        let input = IndicatorInput {
            times: &times,
            open: &close,
            high: &high,
            low: &low,
            close: &close,
            volume: &volume,
        };
        assert_incremental_matches_full(&mut states, input, 0);
        for kind in [
            TestKind::Atr,
            TestKind::Keltner,
            TestKind::AdxDmi,
            TestKind::SuperTrend,
            TestKind::ParabolicSar,
            TestKind::AtrBands,
        ] {
            assert!(
                expected(kind, input)[0][8].is_some_and(f64::is_finite),
                "{kind:?} must process the finite inverted range"
            );
        }
    }

    #[test]
    fn ohlc_windows_match_deleted_required_fields_after_recovery() {
        let kinds = [
            TestKind::Aroon,
            TestKind::AwesomeOscillator,
            TestKind::Cci,
            TestKind::WilliamsR,
            TestKind::Stochastic,
            TestKind::Donchian,
            TestKind::Choppiness,
            TestKind::UltimateOscillator,
            TestKind::Vortex,
            TestKind::Cmf,
            TestKind::Mfi,
            TestKind::EaseOfMovement,
            TestKind::PivotPoints,
            TestKind::Ichimoku,
            TestKind::ZigZag,
        ];
        let n = 160;
        let times = (0..n as i64).map(|row| row * 3_600).collect::<Vec<_>>();
        let mut close = (0..n)
            .map(|row| 100.0 + (row as f64 * 0.43).sin() * 3.0 + row as f64 * 0.05)
            .collect::<Vec<_>>();
        let mut high = close.iter().map(|v| v + 1.4).collect::<Vec<_>>();
        let mut low = close.iter().map(|v| v - 1.2).collect::<Vec<_>>();
        let volume = (0..n).map(|row| (row % 13 + 1) as f64).collect::<Vec<_>>();
        high[8] = low[8] - 0.5;
        high[31] = f64::NAN;
        low[37] = f64::NAN;
        close[43] = f64::NAN;
        let input = IndicatorInput {
            times: &times,
            open: &close,
            high: &high,
            low: &low,
            close: &close,
            volume: &volume,
        };
        for kind in kinds {
            let needs_close = !matches!(
                kind,
                TestKind::Aroon
                    | TestKind::AwesomeOscillator
                    | TestKind::Donchian
                    | TestKind::EaseOfMovement
                    | TestKind::ZigZag
            );
            let kept = (0..n)
                .filter(|&row| {
                    high[row].is_finite()
                        && low[row].is_finite()
                        && (!needs_close || close[row].is_finite())
                })
                .collect::<Vec<_>>();
            let compact_times = kept.iter().map(|&row| times[row]).collect::<Vec<_>>();
            let compact_close = kept.iter().map(|&row| close[row]).collect::<Vec<_>>();
            let compact_high = kept.iter().map(|&row| high[row]).collect::<Vec<_>>();
            let compact_low = kept.iter().map(|&row| low[row]).collect::<Vec<_>>();
            let compact_volume = kept.iter().map(|&row| volume[row]).collect::<Vec<_>>();
            let compact = IndicatorInput {
                times: &compact_times,
                open: &compact_close,
                high: &compact_high,
                low: &compact_low,
                close: &compact_close,
                volume: &compact_volume,
            };
            let actual = expected(kind, input);
            let deleted = expected(kind, compact);
            for (output, values) in actual.iter().enumerate() {
                for (row, &value) in values.iter().enumerate().skip(100) {
                    let compact_row = kept.binary_search(&row).expect("recovered row");
                    let observed = value.filter(|v| v.is_finite());
                    let want = deleted[output][compact_row].filter(|v| v.is_finite());
                    match (observed, want) {
                        (None, None) => {}
                        (Some(a), Some(b)) if (a - b).abs() <= 1e-9 * b.abs().max(1.0) => {}
                        _ => panic!("{kind:?} output {output} row {row}: {observed:?} != {want:?}"),
                    }
                }
            }
        }
    }

    #[test]
    fn every_scalar_kind_has_whitespace_gap_rows_and_matches_incremental_after_repairs() {
        let mut states = all_test_states();
        let n = 1100;
        let mut times = (0..n as i64).map(|row| row * 3_600).collect::<Vec<_>>();
        let mut close = (0..n)
            .map(|row| 100.0 + (row as f64 * 0.43).sin() * 3.0 + row as f64 * 0.05)
            .collect::<Vec<_>>();
        let mut high = close.iter().map(|v| v + 1.4).collect::<Vec<_>>();
        let mut low = close.iter().map(|v| v - 1.2).collect::<Vec<_>>();
        let mut volume = (0..n).map(|row| (row % 13 + 1) as f64).collect::<Vec<_>>();
        // Two rows, the 52-row Ichimoku window, longer than the window, and a
        // run straddling the 1024-row checkpoint (including warm-up and tip gaps).
        for row in (3..5).chain(70..122).chain(140..204).chain(1023..1026) {
            close[row] = f64::NAN;
            high[row] = f64::NAN;
            low[row] = f64::NAN;
        }
        let input = IndicatorInput {
            times: &times,
            open: &close,
            high: &high,
            low: &low,
            close: &close,
            volume: &volume,
        };
        assert_incremental_matches_full(&mut states, input, 0);
        for (kind, state) in &states {
            let batch = expected(*kind, input);
            for (output, batch_output) in batch.iter().enumerate().take(state.output_count()) {
                for row in (3..5).chain(70..122).chain(140..204).chain(1023..1026) {
                    assert!(
                        !batch_output[row].is_some_and(f64::is_finite),
                        "{kind:?} batch output {output} gap row {row}"
                    );
                    if row >= state.output_from(output) {
                        assert!(
                            state.output(output)[row - state.output_from(output)].is_nan(),
                            "{kind:?} incremental output {output} gap row {row}"
                        );
                    }
                }
            }
        }
        // All kinds are checked on both sides of a sparse checkpoint, on
        // a newly created gap, on a filled gap, and after a whitespace tip
        // is followed by another append.
        let check = |states: &mut [(TestKind, IncrementalState)],
                     times: &[i64],
                     close: &[f64],
                     high: &[f64],
                     low: &[f64],
                     volume: &[f64],
                     from: usize| {
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
        close[1024] = 149.0;
        high[1024] = 150.4;
        low[1024] = 147.8;
        check(&mut states, &times, &close, &high, &low, &volume, 1024);
        close[1010] = f64::NAN;
        high[1010] = f64::NAN;
        low[1010] = f64::NAN;
        check(&mut states, &times, &close, &high, &low, &volume, 1010);
        close[1010] = 148.0;
        high[1010] = 149.4;
        low[1010] = 146.8;
        check(&mut states, &times, &close, &high, &low, &volume, 1010);
        close[1099] = f64::NAN;
        high[1099] = f64::NAN;
        low[1099] = f64::NAN;
        check(&mut states, &times, &close, &high, &low, &volume, 1099);
        times.push(1100 * 3_600);
        close.push(155.0);
        high.push(156.4);
        low.push(153.8);
        volume.push(3.0);
        check(&mut states, &times, &close, &high, &low, &volume, 1100);
    }

    fn all_test_states() -> Vec<(TestKind, IncrementalState)> {
        vec![
            (TestKind::Aroon, IncrementalState::aroon(5)),
            (
                TestKind::AwesomeOscillator,
                IncrementalState::awesome_oscillator(),
            ),
            (TestKind::Dpo, IncrementalState::dpo(5)),
            (
                TestKind::ChandeMomentum,
                IncrementalState::chande_momentum(5),
            ),
            (TestKind::Sma, IncrementalState::sma(5)),
            (TestKind::Ema, IncrementalState::ema(5)),
            (TestKind::Dema, IncrementalState::dema(5)),
            (TestKind::Tema, IncrementalState::tema(5)),
            (TestKind::Smma, IncrementalState::smma(5)),
            (TestKind::Hma, IncrementalState::hma(5)),
            (TestKind::Vwma, IncrementalState::vwma(5)),
            (
                TestKind::StandardDeviation,
                IncrementalState::standard_deviation(5),
            ),
            (TestKind::Cci, IncrementalState::cci(5)),
            (TestKind::WilliamsR, IncrementalState::williams_r(5)),
            (
                TestKind::StochasticRsi,
                IncrementalState::stochastic_rsi(5, 5),
            ),
            (TestKind::Momentum, IncrementalState::momentum(5)),
            (TestKind::RateOfChange, IncrementalState::rate_of_change(5)),
            (TestKind::Donchian, IncrementalState::donchian(5)),
            (
                TestKind::PivotPoints,
                IncrementalState::pivot_points(PivotKind::Standard),
            ),
            (TestKind::ZigZag, IncrementalState::zigzag(5.0)),
            (TestKind::Keltner, IncrementalState::keltner(5, 2.0)),
            (TestKind::AdxDmi, IncrementalState::adx_dmi(5)),
            (TestKind::ParabolicSar, IncrementalState::parabolic_sar()),
            (TestKind::SuperTrend, IncrementalState::supertrend(5, 3.0)),
            (TestKind::Ichimoku, IncrementalState::ichimoku()),
            (
                TestKind::EmaRibbon,
                IncrementalState::ema_ribbon([3, 5, 8, 13, 21]),
            ),
            (TestKind::Bollinger, IncrementalState::bollinger(5, 2.0)),
            (
                TestKind::BollingerMetrics,
                IncrementalState::bollinger_metrics(5, 2.0),
            ),
            (
                TestKind::EnvelopesSma,
                IncrementalState::envelopes(5, 10.0, false),
            ),
            (
                TestKind::EnvelopesEma,
                IncrementalState::envelopes(5, 10.0, true),
            ),
            (TestKind::Alma, IncrementalState::alma(5, 0.85, 6.0)),
            (TestKind::Rsi, IncrementalState::rsi(5)),
            (TestKind::Macd, IncrementalState::macd(3, 6, 4)),
            (TestKind::Stochastic, IncrementalState::stochastic(5, 3)),
            (TestKind::Atr, IncrementalState::atr(5)),
            (TestKind::Vwap, IncrementalState::vwap()),
            (
                TestKind::VwapBands,
                IncrementalState::vwap_bands(VwapReset::Monthly, 1.0, 5.0),
            ),
            (TestKind::Obv, IncrementalState::obv()),
            (
                TestKind::AccumulationDistribution,
                IncrementalState::accumulation_distribution(),
            ),
            (
                TestKind::PriceVolumeTrend,
                IncrementalState::price_volume_trend(),
            ),
            (
                TestKind::ChaikinOscillator,
                IncrementalState::chaikin_oscillator(3, 7),
            ),
            (
                TestKind::RelativeVolume,
                IncrementalState::relative_volume(5),
            ),
            (
                TestKind::VolumeOscillator,
                IncrementalState::volume_oscillator(3, 7, 4),
            ),
            (TestKind::ElderForce, IncrementalState::elder_force(5)),
            (
                TestKind::EaseOfMovement,
                IncrementalState::ease_of_movement(5, 100.0),
            ),
            (
                TestKind::HistoricalVolatility,
                IncrementalState::historical_volatility(5, 252.0),
            ),
            (TestKind::Trix, IncrementalState::trix(3, 4)),
            (
                TestKind::Kst,
                IncrementalState::kst([2, 3, 4, 5], [2, 2, 2, 3], 3),
            ),
            (TestKind::Tsi, IncrementalState::tsi(5, 3, 3)),
            (TestKind::MassIndex, IncrementalState::mass_index(3, 5)),
            (TestKind::Klinger, IncrementalState::klinger(3, 7, 4)),
            (TestKind::Kama, IncrementalState::kama(5, 2, 10)),
            (TestKind::McGinley, IncrementalState::mcginley(5)),
            (
                TestKind::LinearRegression,
                IncrementalState::linear_regression(5, 2.0),
            ),
            (TestKind::Choppiness, IncrementalState::choppiness(5)),
            (TestKind::AtrBands, IncrementalState::atr_bands(5, 2.0)),
            (
                TestKind::CoppockCurve,
                IncrementalState::coppock_curve(7, 5, 3),
            ),
            (
                TestKind::FisherTransform,
                IncrementalState::fisher_transform(5),
            ),
            (
                TestKind::UltimateOscillator,
                IncrementalState::ultimate_oscillator(3, 5, 7),
            ),
            (TestKind::Vortex, IncrementalState::vortex(5)),
            (TestKind::Cmf, IncrementalState::cmf(5)),
            (TestKind::Mfi, IncrementalState::mfi(5)),
            (TestKind::Volume, IncrementalState::volume(5)),
            (TestKind::Wma, IncrementalState::wma(5)),
        ]
    }

    #[test]
    fn every_runtime_mutation_matches_fresh_full_recomputation() {
        let mut states = all_test_states();
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
    fn new_studies_repair_gaps_across_sparse_checkpoints() {
        let mut states = all_test_states();
        let times = (0..1100).map(i64::from).collect::<Vec<_>>();
        let mut close = (0..1100)
            .map(|row| 100.0 + (row as f64 / 7.0).sin() * 4.0)
            .collect::<Vec<_>>();
        let mut high = close.iter().map(|value| value + 2.0).collect::<Vec<_>>();
        let mut low = close.iter().map(|value| value - 2.0).collect::<Vec<_>>();
        let volume = vec![1.0; 1100];
        let check = |states: &mut [(TestKind, IncrementalState)],
                     close: &[f64],
                     high: &[f64],
                     low: &[f64],
                     from| {
            assert_incremental_matches_full(
                states,
                IndicatorInput {
                    times: &times,
                    open: close,
                    high,
                    low,
                    close,
                    volume: &volume,
                },
                from,
            );
        };
        check(&mut states, &close, &high, &low, 0);
        close[1030] = f64::NAN;
        high[1030] = f64::NAN;
        low[1030] = f64::NAN;
        check(&mut states, &close, &high, &low, 1030);
        close[1030] = 99.0;
        high[1030] = 101.0;
        low[1030] = 97.0;
        check(&mut states, &close, &high, &low, 1030);
        close[1099] += 3.0;
        high[1099] += 3.0;
        low[1099] += 3.0;
        check(&mut states, &close, &high, &low, 1099);
    }

    #[test]
    fn new_studies_handle_empty_short_gap_and_historical_correction() {
        let mut states = [
            (
                TestKind::Kst,
                IncrementalState::kst([2, 3, 4, 5], [2, 2, 2, 3], 3),
            ),
            (TestKind::Klinger, IncrementalState::klinger(3, 7, 4)),
            (
                TestKind::LinearRegression,
                IncrementalState::linear_regression(5, 2.0),
            ),
            (TestKind::MassIndex, IncrementalState::mass_index(3, 5)),
            (TestKind::Tsi, IncrementalState::tsi(5, 3, 3)),
            (TestKind::Vortex, IncrementalState::vortex(5)),
            (TestKind::Kama, IncrementalState::kama(5, 2, 10)),
            (TestKind::McGinley, IncrementalState::mcginley(5)),
            (TestKind::Choppiness, IncrementalState::choppiness(5)),
            (TestKind::AtrBands, IncrementalState::atr_bands(5, 2.0)),
        ];
        let times = (0..48).map(i64::from).collect::<Vec<_>>();
        let mut close = (0..48)
            .map(|row| 100.0 + (row as f64 * 0.41).sin() * 3.0 + row as f64 * 0.1)
            .collect::<Vec<_>>();
        let mut high = close.iter().map(|value| value + 2.0).collect::<Vec<_>>();
        let mut low = close.iter().map(|value| value - 1.5).collect::<Vec<_>>();
        let mut volume = (0..48).map(|row| (row % 7 + 1) as f64).collect::<Vec<_>>();
        let check = |states: &mut [(TestKind, IncrementalState)],
                     len: usize,
                     close: &[f64],
                     high: &[f64],
                     low: &[f64],
                     volume: &[f64],
                     from: usize| {
            assert_incremental_matches_full(
                states,
                IndicatorInput {
                    times: &times[..len],
                    open: &close[..len],
                    high: &high[..len],
                    low: &low[..len],
                    close: &close[..len],
                    volume: &volume[..len],
                },
                from,
            );
        };

        check(&mut states, 0, &close, &high, &low, &volume, 0);
        for (_, state) in &states {
            for output in 0..state.output_count() {
                assert!(state.output(output).is_empty());
            }
        }

        check(&mut states, 1, &close, &high, &low, &volume, 0);
        for (kind, state) in &states {
            for output in 0..state.output_count() {
                if matches!(kind, TestKind::McGinley) {
                    assert!(
                        state.output(output)[0].is_finite(),
                        "McGinley seeds at the first close"
                    );
                } else {
                    assert!(
                        state.output(output).iter().all(|value| value.is_nan()),
                        "one-bar warmup must be blank"
                    );
                }
            }
        }

        check(&mut states, 4, &close, &high, &low, &volume, 1);
        for (kind, state) in &states {
            for output in 0..state.output_count() {
                if matches!(kind, TestKind::McGinley) {
                    assert!(state.output(output).iter().all(|value| value.is_finite()));
                } else {
                    assert!(state.output(output).iter().all(|value| value.is_nan()));
                }
            }
        }

        check(&mut states, 48, &close, &high, &low, &volume, 4);
        let gap = 24;
        close[gap] = f64::NAN;
        high[gap] = f64::NAN;
        low[gap] = f64::NAN;
        volume[gap] = f64::NAN;
        check(&mut states, 48, &close, &high, &low, &volume, gap);
        for (_, state) in &states {
            for output in 0..state.output_count() {
                let from = state.output_from(output);
                assert!(
                    state.output(output)[gap - from].is_nan(),
                    "gap row must be blank"
                );
            }
        }

        close[7] += 5.0;
        high[7] = close[7] + 2.0;
        low[7] = close[7] - 1.5;
        volume[7] += 4.0;
        check(&mut states, 48, &close, &high, &low, &volume, 7);

        close[gap] = 102.0;
        high[gap] = 104.0;
        low[gap] = 100.5;
        volume[gap] = 6.0;
        check(&mut states, 48, &close, &high, &low, &volume, gap);
    }

    #[test]
    fn every_study_handles_short_inputs_and_gap_repair() {
        let mut states = all_test_states();
        let times = (0..80).map(i64::from).collect::<Vec<_>>();
        let mut close = (0..80)
            .map(|row| 100.0 + row as f64 * 0.1 + (row as f64 * 0.37).sin())
            .collect::<Vec<_>>();
        let mut high = close.iter().map(|value| value + 1.0).collect::<Vec<_>>();
        let mut low = close.iter().map(|value| value - 1.0).collect::<Vec<_>>();
        let volume = vec![5.0; 80];
        let check = |states: &mut [(TestKind, IncrementalState)],
                     n: usize,
                     close: &[f64],
                     high: &[f64],
                     low: &[f64],
                     from: usize| {
            assert_incremental_matches_full(
                states,
                IndicatorInput {
                    times: &times[..n],
                    open: &close[..n],
                    high: &high[..n],
                    low: &low[..n],
                    close: &close[..n],
                    volume: &volume[..n],
                },
                from,
            );
        };
        for len in [0, 1, 4, 80] {
            check(&mut states, len, &close, &high, &low, 0);
        }
        for (row, value) in [
            (24, f64::NAN), // create a historical gap
            (32, f64::NAN), // create another gap inside the short windows
            (79, f64::NAN), // replace the tip with whitespace
            (79, 119.0),    // fill the tip
            (24, 112.0),    // fill the historical gap
            (32, 114.0),
        ] {
            close[row] = value;
            high[row] = if value.is_finite() {
                value + 1.0
            } else {
                f64::NAN
            };
            low[row] = if value.is_finite() {
                value - 1.0
            } else {
                f64::NAN
            };
            check(&mut states, 80, &close, &high, &low, row);
            if !value.is_finite() {
                for (kind, state) in &states {
                    for output in 0..state.output_count() {
                        if row >= state.output_from(output) {
                            assert!(
                                state.output(output)[row - state.output_from(output)].is_nan(),
                                "{kind:?} output {output} emits at gap row {row}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn i1_and_i2_reference_fixture_covers_catalog_outputs() {
        // These values are a fixed external reference fixture. The assertions intentionally do
        // not call the dense formula functions, so a shared implementation defect cannot make
        // the incremental and reference paths agree by construction.
        let rows = 60;
        let times = (0..rows)
            .map(|index| index as i64 * 86_400)
            .collect::<Vec<_>>();
        let close = (0..rows)
            .map(|index| 100.0 + (index as f64 * 0.63).sin() * 5.0 + index as f64 * 0.08)
            .collect::<Vec<_>>();
        let open = close
            .iter()
            .enumerate()
            .map(|(index, value)| value + (index as f64 * 0.41).cos() * 0.7)
            .collect::<Vec<_>>();
        let high = close.iter().map(|value| value + 1.4).collect::<Vec<_>>();
        let low = close.iter().map(|value| value - 1.1).collect::<Vec<_>>();
        let volume = (0..rows)
            .map(|index| (index % 9 + 1) as f64 * 10.0)
            .collect::<Vec<_>>();
        let input = IndicatorInput {
            times: &times,
            open: &open,
            high: &high,
            low: &low,
            close: &close,
            volume: &volume,
        };
        let assert_final =
            |label: &str, mut state: IncrementalState, output: usize, expected: f64| {
                state.rebuild_from(input, 0);
                let actual = *state.output(output).last().expect("reference output row");
                assert!(
                    (actual - expected).abs() < 1e-9,
                    "{label}: {actual} != {expected}"
                );
            };

        // Independently calculated from the fixture OHLCV rows (not from dense indicator
        // implementations). Short KST windows keep both its line and signal within 60 rows.
        assert_final("Aroon up", IncrementalState::aroon(5), 0, 0.0);
        assert_final("Aroon down", IncrementalState::aroon(5), 1, 60.0);
        assert_final(
            "Awesome Oscillator",
            IncrementalState::awesome_oscillator(),
            0,
            -1.536797362138088,
        );
        assert_final(
            "Chande Momentum",
            IncrementalState::chande_momentum(5),
            0,
            -48.19370754205317,
        );
        assert_final(
            "Chaikin Oscillator",
            IncrementalState::chaikin_oscillator(3, 6),
            0,
            -8.099183009038654,
        );
        assert_final(
            "Coppock Curve",
            IncrementalState::coppock_curve(6, 3, 4),
            0,
            -9.41909313330223,
        );
        assert_final("DPO", IncrementalState::dpo(5), 0, -0.2351633003471107);
        assert_final(
            "Elder Force",
            IncrementalState::elder_force(5),
            0,
            31.96442399619355,
        );
        assert_final(
            "Ease of Movement",
            IncrementalState::ease_of_movement(5, 1.0),
            0,
            -0.1166970104225895,
        );
        assert_final(
            "Fisher line",
            IncrementalState::fisher_transform(5),
            0,
            -0.6891408918939269,
        );
        assert_final(
            "Fisher trigger",
            IncrementalState::fisher_transform(5),
            1,
            -0.7700306869497627,
        );
        assert_final(
            "Historical Volatility",
            IncrementalState::historical_volatility(5, 252.0),
            0,
            33.55558767101923,
        );
        assert_final(
            "KST line",
            IncrementalState::kst([2, 3, 4, 5], [2, 3, 4, 5], 3),
            0,
            -45.15601171260293,
        );
        assert_final(
            "KST signal",
            IncrementalState::kst([2, 3, 4, 5], [2, 3, 4, 5], 3),
            1,
            -40.42890476968924,
        );
        assert_final(
            "Klinger line",
            IncrementalState::klinger(3, 6, 4),
            0,
            376.5561190129093,
        );
        assert_final(
            "Klinger signal",
            IncrementalState::klinger(3, 6, 4),
            1,
            -185.2666012083956,
        );
        assert_final(
            "Regression curve",
            IncrementalState::linear_regression(5, 2.0),
            0,
            100.4888887275999,
        );
        assert_final(
            "Regression upper",
            IncrementalState::linear_regression(5, 2.0),
            1,
            103.2854271400926,
        );
        assert_final(
            "Regression lower",
            IncrementalState::linear_regression(5, 2.0),
            2,
            97.69235031510719,
        );
        assert_final("Mass Index", IncrementalState::mass_index(5, 5), 0, 5.0);
        assert_final(
            "Ultimate Oscillator",
            IncrementalState::ultimate_oscillator(3, 5, 7),
            0,
            45.65847840650458,
        );
        assert_final(
            "TRIX line",
            IncrementalState::trix(5, 3),
            0,
            -0.409962754833737,
        );
        assert_final(
            "TRIX signal",
            IncrementalState::trix(5, 3),
            1,
            -0.280666792016231,
        );
        assert_final(
            "TSI line",
            IncrementalState::tsi(5, 3, 3),
            0,
            -26.580720443143,
        );
        assert_final(
            "TSI signal",
            IncrementalState::tsi(5, 3, 3),
            1,
            -36.46784062557349,
        );
        assert_final(
            "Vortex +VI",
            IncrementalState::vortex(5),
            0,
            0.5579692505882866,
        );
        assert_final(
            "Vortex -VI",
            IncrementalState::vortex(5),
            1,
            1.038943112552389,
        );
        assert_final(
            "Envelope SMA upper",
            IncrementalState::envelopes(5, 2.0, false),
            0,
            103.4371031507486,
        );
        assert_final(
            "Envelope SMA basis",
            IncrementalState::envelopes(5, 2.0, false),
            1,
            101.4089246575966,
        );
        assert_final(
            "Envelope SMA lower",
            IncrementalState::envelopes(5, 2.0, false),
            2,
            99.38074616444469,
        );
        assert_final(
            "Envelope EMA upper",
            IncrementalState::envelopes(5, 2.0, true),
            0,
            104.0025555497179,
        );
        assert_final(
            "Envelope EMA basis",
            IncrementalState::envelopes(5, 2.0, true),
            1,
            101.9632897546254,
        );
        assert_final(
            "Envelope EMA lower",
            IncrementalState::envelopes(5, 2.0, true),
            2,
            99.92402395953289,
        );
        assert_final(
            "ALMA",
            IncrementalState::alma(5, 0.85, 6.0),
            0,
            100.8775648482768,
        );
        assert_final(
            "KAMA",
            IncrementalState::kama(5, 2, 30),
            0,
            101.9160311331937,
        );
        assert_final(
            "McGinley",
            IncrementalState::mcginley(5),
            0,
            102.2960315743182,
        );
        assert_final(
            "Choppiness",
            IncrementalState::choppiness(5),
            0,
            55.5975026457025,
        );
        assert_final(
            "Bollinger %B",
            IncrementalState::bollinger_metrics(5, 2.0),
            0,
            0.6276163411758227,
        );
        assert_final(
            "Bollinger BandWidth",
            IncrementalState::bollinger_metrics(5, 2.0),
            1,
            6.083105287510145,
        );
        assert_final(
            "ATR bands upper",
            IncrementalState::atr_bands(5, 2.0),
            0,
            108.6787277996608,
        );
        assert_final(
            "ATR bands basis",
            IncrementalState::atr_bands(5, 2.0),
            1,
            102.19616583077465,
        );
        assert_final(
            "ATR bands lower",
            IncrementalState::atr_bands(5, 2.0),
            2,
            95.71360386188844,
        );
        assert_final(
            "Accumulation/Distribution",
            IncrementalState::accumulation_distribution(),
            0,
            -349.2000000000132,
        );
        assert_final(
            "Price Volume Trend",
            IncrementalState::price_volume_trend(),
            0,
            -4.062813009843773,
        );
        assert_final(
            "Volume Oscillator line",
            IncrementalState::volume_oscillator(3, 6, 4),
            0,
            8.09094542282349,
        );
        assert_final(
            "Volume Oscillator signal",
            IncrementalState::volume_oscillator(3, 6, 4),
            1,
            -1.079470318694398,
        );
        assert_final(
            "Volume Oscillator histogram",
            IncrementalState::volume_oscillator(3, 6, 4),
            2,
            9.170415741517887,
        );
        assert_final(
            "Relative Volume",
            IncrementalState::relative_volume(5),
            0,
            2.0,
        );

        assert_final("SMA", IncrementalState::sma(5), 0, 101.40892465759661);
        assert_final("EMA", IncrementalState::ema(5), 0, 101.96328975462541);
        assert_final("RSI", IncrementalState::rsi(5), 0, 46.373060486779785);
        assert_final("ATR", IncrementalState::atr(5), 0, 3.2412809844431);
        assert_final("VWAP", IncrementalState::vwap(), 0, 102.29616583077465);
        assert_final(
            "Bollinger upper",
            IncrementalState::bollinger(5, 2.0),
            0,
            104.49333048652335,
        );
        assert_final(
            "Bollinger middle",
            IncrementalState::bollinger(5, 2.0),
            1,
            101.40892465759663,
        );
        assert_final(
            "Bollinger lower",
            IncrementalState::bollinger(5, 2.0),
            2,
            98.32451882866991,
        );
        assert_final(
            "MACD",
            IncrementalState::macd(3, 6, 4),
            0,
            -0.7050224639613702,
        );
        assert_final(
            "MACD signal",
            IncrementalState::macd(3, 6, 4),
            1,
            -0.8431021200221607,
        );
        assert_final(
            "MACD histogram",
            IncrementalState::macd(3, 6, 4),
            2,
            0.13807965606079042,
        );
        assert_final(
            "Stochastic K",
            IncrementalState::stochastic(5, 3),
            0,
            53.516458777874405,
        );
        assert_final(
            "Stochastic D",
            IncrementalState::stochastic(5, 3),
            1,
            26.065449201630944,
        );
        assert_final(
            "EMA ribbon 5",
            IncrementalState::ema_ribbon([3, 5, 8, 13, 21]),
            0,
            101.49995512853438,
        );
        assert_final(
            "EMA ribbon 10",
            IncrementalState::ema_ribbon([3, 5, 8, 13, 21]),
            1,
            101.96328975462541,
        );
        assert_final(
            "EMA ribbon 20",
            IncrementalState::ema_ribbon([3, 5, 8, 13, 21]),
            2,
            102.57828153839074,
        );
        assert_final(
            "EMA ribbon 50",
            IncrementalState::ema_ribbon([3, 5, 8, 13, 21]),
            3,
            103.03218446733823,
        );
        assert_final(
            "EMA ribbon 200",
            IncrementalState::ema_ribbon([3, 5, 8, 13, 21]),
            4,
            103.17775917896441,
        );
        assert_final("HMA", IncrementalState::hma(5), 0, 100.81322452527017);
        assert_final("VWMA", IncrementalState::vwma(5), 0, 101.17891567509746);
        assert_final("DEMA", IncrementalState::dema(5), 0, 100.93821390320514);
        assert_final("TEMA", IncrementalState::tema(5), 0, 100.76808077358208);
        assert_final("SMMA/RMA", IncrementalState::smma(5), 0, 102.71390487376016);
        assert_final(
            "standard deviation",
            IncrementalState::standard_deviation(5),
            0,
            1.54220291446336,
        );
        assert_final("WMA", IncrementalState::wma(5), 0, 101.1022460142644);
        assert_final("CCI", IncrementalState::cci(5), 0, 39.56099529637163);
        assert_final(
            "Williams %R",
            IncrementalState::williams_r(5),
            0,
            -46.483541222125595,
        );
        assert_final(
            "Stochastic RSI",
            IncrementalState::stochastic_rsi(5, 5),
            0,
            100.0,
        );
        assert_final(
            "Momentum",
            IncrementalState::momentum(5),
            0,
            -4.6838671643187695,
        );
        assert_final(
            "ROC",
            IncrementalState::rate_of_change(5),
            0,
            -4.382359392173651,
        );
        assert_final(
            "Donchian upper",
            IncrementalState::donchian(5),
            0,
            105.33825479950816,
        );
        assert_final(
            "Donchian middle",
            IncrementalState::donchian(5),
            1,
            101.958468227613,
        );
        assert_final(
            "Donchian lower",
            IncrementalState::donchian(5),
            2,
            98.57868165571782,
        );
        assert_final(
            "Keltner upper",
            IncrementalState::keltner(5, 2.0),
            0,
            108.44585172351161,
        );
        assert_final(
            "Keltner middle",
            IncrementalState::keltner(5, 2.0),
            1,
            101.96328975462541,
        );
        assert_final(
            "Keltner lower",
            IncrementalState::keltner(5, 2.0),
            2,
            95.4807277857392,
        );
        assert_final(
            "ADX +DI",
            IncrementalState::adx_dmi(5),
            0,
            25.528012909972404,
        );
        assert_final(
            "ADX -DI",
            IncrementalState::adx_dmi(5),
            1,
            29.52121748802997,
        );
        assert_final("ADX", IncrementalState::adx_dmi(5), 2, 31.00060976884448);
        assert_final(
            "Parabolic SAR",
            IncrementalState::parabolic_sar(),
            0,
            109.96264834890401,
        );
        assert_final(
            "SuperTrend",
            IncrementalState::supertrend(5, 3.0),
            0,
            98.98519436972305,
        );
        assert_final(
            "Ichimoku conversion",
            IncrementalState::ichimoku(),
            0,
            104.50536152514003,
        );
        assert_final(
            "Ichimoku base",
            IncrementalState::ichimoku(),
            1,
            103.72494716464809,
        );
        assert_final(
            "Ichimoku leading A",
            IncrementalState::ichimoku(),
            2,
            104.11515434489405,
        );
        assert_final(
            "Ichimoku leading B",
            IncrementalState::ichimoku(),
            3,
            102.6189862582996,
        );
        assert_final(
            "Ichimoku lagging",
            IncrementalState::ichimoku(),
            4,
            102.19616583077465,
        );
        assert_final("OBV", IncrementalState::obv(), 0, -220.0);
        assert_final("CMF", IncrementalState::cmf(5), 0, -0.12000000000000455);
        assert_final("MFI", IncrementalState::mfi(5), 0, 55.02457178668054);
        assert_final("Volume", IncrementalState::volume(5), 0, 60.0);
        assert_final("Volume MA", IncrementalState::volume(5), 1, 40.0);
        assert_final(
            "VWAP bands basis",
            IncrementalState::vwap_bands(VwapReset::Monthly, 1.0, 5.0),
            0,
            102.29616583077465,
        );
        assert_final(
            "VWAP bands standard upper",
            IncrementalState::vwap_bands(VwapReset::Monthly, 1.0, 5.0),
            1,
            102.29616583077465,
        );
        assert_final(
            "VWAP bands standard lower",
            IncrementalState::vwap_bands(VwapReset::Monthly, 1.0, 5.0),
            2,
            102.29616583077465,
        );
        assert_final(
            "VWAP bands percent upper",
            IncrementalState::vwap_bands(VwapReset::Monthly, 1.0, 5.0),
            3,
            107.41097412231339,
        );
        assert_final(
            "VWAP bands percent lower",
            IncrementalState::vwap_bands(VwapReset::Monthly, 1.0, 5.0),
            4,
            97.18135753923592,
        );
        assert_final(
            "ZigZag",
            IncrementalState::zigzag(5.0),
            0,
            103.59616583077465,
        );

        for (kind, expected) in [
            (
                PivotKind::Standard,
                [
                    100.157759644733,
                    101.357759644733,
                    98.857759644733,
                    102.657759644733,
                    97.657759644733,
                ],
            ),
            (
                PivotKind::Fibonacci,
                [
                    100.157759644733,
                    101.112759644733,
                    99.202759644733,
                    101.702759644733,
                    98.612759644733,
                ],
            ),
            (
                PivotKind::Camarilla,
                [
                    100.05775964473301,
                    100.28692631139968,
                    99.82859297806634,
                    100.51609297806634,
                    99.59942631139968,
                ],
            ),
            (
                PivotKind::Woodie,
                [
                    100.13275964473301,
                    101.30775964473301,
                    98.80775964473301,
                    102.63275964473301,
                    97.63275964473301,
                ],
            ),
            (
                PivotKind::DeMark,
                [
                    99.85775964473302,
                    100.75775964473303,
                    98.25775964473303,
                    102.35775964473302,
                    97.35775964473302,
                ],
            ),
        ] {
            let mut state = IncrementalState::pivot_points(kind);
            state.rebuild_from(input, 0);
            for (output, expected) in expected.into_iter().enumerate() {
                let actual = *state
                    .output(output)
                    .last()
                    .expect("pivot reference output row");
                assert!(
                    (actual - expected).abs() < 1e-9,
                    "pivot {kind:?} output {output}: {actual} != {expected}"
                );
            }
        }
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
        // Each 1,024-row checkpoint now also stores the previous valid close and the valid
        // change count. This is necessary to repair across arbitrarily long gaps without a
        // backwards history scan; retained bytes still scale with checkpoints, not rows.
        assert!(state.runtime_bytes() < 64 * 1024);
        assert!(
            state.runtime_bytes()
                <= 2 * rows.div_ceil(CHECKPOINT_INTERVAL)
                    * std::mem::size_of::<Checkpoint<IndexedRsiState>>()
        );
        assert_eq!(state.transfer_capacity_bytes(), 0);

        state.rebuild_from(input, rows - 1);
        assert_eq!(state.last_work_rows(), 1);
        state.release_transfer_capacity();

        state.rebuild_from(input, rows / 2);
        assert!(state.last_work_rows() >= rows / 2);
        assert!(state.last_work_rows() < rows / 2 + CHECKPOINT_INTERVAL);
    }
}
