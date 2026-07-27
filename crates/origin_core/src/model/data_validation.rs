//! Boundary data validation & sanitization (roadmap Phase A3).
//!
//! Real market feeds are messy: out-of-order rows, duplicate timestamps, NaN/Infinity, absurd
//! magnitudes, mismatched array lengths. The rest of the engine ([`super::data_layer::DataLayer`]
//! and [`super::plot_list::PlotList`]) assumes **ascending, unique, finite** input — its ordering
//! guard is a `debug_assert!` that is compiled out of the release wasm build, so bad data there
//! would silently corrupt indices or panic in `reindex_all`.
//!
//! This module is the single choke point that makes that assumption safe to hold. Unlike
//! the reference charting library's `data-validators.ts` (which only `assert`s in dev builds and throws in
//! prod), we *repair* what we can and *report* what we changed, so a production embedder gets a
//! rendered chart plus a diagnostic instead of a thrown error or a dead canvas.
//!
//! Repair policy, in order:
//! 1. **Length mismatch** between the time and value columns is unrecoverable → [`Err`].
//! 2. **Non-finite / out-of-safe-range** rows (NaN, ±Inf, |v| beyond [`MAX_SAFE_VALUE`], or a
//!    non-finite time) are dropped and counted — **except** a row whose four values are all
//!    NaN, which is kept as an explicit **whitespace** row (the reference's `{time}`-only item,
//!    data-consumer.ts `isWhitespaceData`): a real bar never has all four NaN, and for
//!    single-value series a NaN value is whitespace. Whitespace rows occupy their time point
//!    but draw nothing; genuinely malformed rows (a partial NaN set, ±Inf, out-of-range) are
//!    still dropped.
//! 3. **Unordered** rows are stably sorted by time (`reordered` flagged).
//! 4. **Duplicate** timestamps collapse **last-wins** (the last occurrence in the *original*
//!    input for that timestamp survives — matching a streaming `update()` overwriting a bar).

/// the reference's safe magnitude bound (`data-validators.ts`): `Number.MAX_SAFE_INTEGER / 100`.
pub const MAX_SAFE_VALUE: f64 = 9_007_199_254_740_991.0 / 100.0;
/// Symmetric lower bound.
pub const MIN_SAFE_VALUE: f64 = -MAX_SAFE_VALUE;

/// What the sanitizer had to change to make the data ingestible.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ValidationReport {
    /// Rows dropped for a non-finite / out-of-range time or value.
    pub dropped_invalid: usize,
    /// Rows discarded because a later row shared their timestamp (last-wins).
    pub dropped_duplicate: usize,
    /// The input was not already ascending and had to be sorted.
    pub reordered: bool,
    /// Rows that made it into the sanitized output.
    pub accepted: usize,
}

impl ValidationReport {
    /// True when the input was already clean (nothing dropped or reordered).
    pub fn is_clean(&self) -> bool {
        self.dropped_invalid == 0 && self.dropped_duplicate == 0 && !self.reordered
    }
}

/// Structural problems the sanitizer cannot repair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// The time column and the value columns have differing lengths.
    LengthMismatch {
        times: usize,
        open: usize,
        high: usize,
        low: usize,
        close: usize,
    },
    /// A per-point color channel's length differs from the time column.
    ColorLengthMismatch {
        times: usize,
        channel: &'static str,
        colors: usize,
    },
}

impl core::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ValidationError::LengthMismatch { times, open, high, low, close } => write!(
                f,
                "time/OHLC arrays must have equal length (times={times}, open={open}, high={high}, low={low}, close={close})"
            ),
            ValidationError::ColorLengthMismatch { times, channel, colors } => write!(
                f,
                "point-color channel must match the row count (channel={channel}, times={times}, colors={colors})"
            ),
        }
    }
}

/// Ascending, unique, finite OHLC rows ready for [`super::data_layer::DataLayer::set_data`].
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SanitizedOhlc {
    pub times: Vec<i64>,
    pub open: Vec<f64>,
    pub high: Vec<f64>,
    pub low: Vec<f64>,
    pub close: Vec<f64>,
    pub report: ValidationReport,
}

fn safe(v: f64) -> bool {
    v.is_finite() && (MIN_SAFE_VALUE..=MAX_SAFE_VALUE).contains(&v)
}

/// Whether the four values form an explicit whitespace row (reference `{time}`-only item): all
/// four are NaN. A real bar never has all four NaN; single-value series alias one value
/// into all four slots, so a NaN value is whitespace there as well.
pub fn is_whitespace_values(values: [f64; 4]) -> bool {
    values.iter().all(|v| v.is_nan())
}

/// Sanitize parallel time/OHLC columns into ascending, unique, finite rows.
///
/// `times` are wall-clock seconds as `f64` at the JS boundary; each is truncated toward zero to an
/// `i64` time key (a non-finite time drops the row). Single-value series pass the same value in all
/// four columns, so this covers line/area/histogram too.
pub fn sanitize_ohlc(
    times: &[f64],
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
) -> Result<SanitizedOhlc, ValidationError> {
    sanitize_rows(times, open, high, low, close, |_| ()).map(|(out, _)| out)
}

/// [`sanitize_ohlc`] output plus the per-row color channels carried through the same repair.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SanitizedOhlcStyled {
    pub data: SanitizedOhlc,
    /// The three reference data-item color channels (body/wick/border) after the repair pipeline;
    /// each is empty (channel absent) or aligned with `data`'s rows.
    pub colors: [Vec<u32>; 3],
}

/// [`sanitize_ohlc`] carrying per-row data-item color channels (reference series-bar-colorer.ts).
/// Every present channel must match the time column's length (a mismatch is unrecoverable,
/// like the OHLC columns); within a channel, `0` means "no override at this row". The repair
/// policy treats the channels as part of their row: invalid rows drop them, the stable sort
/// moves them, and the last-wins dedupe keeps the winning row's channels.
pub fn sanitize_ohlc_styled(
    times: &[f64],
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    colors: [Option<Vec<u32>>; 3],
) -> Result<SanitizedOhlcStyled, ValidationError> {
    const CHANNEL_NAMES: [&str; 3] = ["body", "wick", "border"];
    for (name, channel) in CHANNEL_NAMES.into_iter().zip(&colors) {
        if let Some(channel) = channel {
            if channel.len() != times.len() {
                return Err(ValidationError::ColorLengthMismatch {
                    times: times.len(),
                    channel: name,
                    colors: channel.len(),
                });
            }
        }
    }
    let present = [
        colors[0].is_some(),
        colors[1].is_some(),
        colors[2].is_some(),
    ];
    let (data, payloads) = sanitize_rows(times, open, high, low, close, |row| {
        [
            colors[0].as_ref().map_or(0, |c| c[row]),
            colors[1].as_ref().map_or(0, |c| c[row]),
            colors[2].as_ref().map_or(0, |c| c[row]),
        ]
    })?;
    let mut channels: [Vec<u32>; 3] = [vec![], vec![], vec![]];
    for i in 0..3 {
        if present[i] {
            channels[i] = payloads.iter().map(|p| p[i]).collect();
        }
    }
    Ok(SanitizedOhlcStyled {
        data,
        colors: channels,
    })
}

/// Shared repair pipeline for [`sanitize_ohlc`] and [`sanitize_ohlc_styled`]: applies the
/// drop-invalid → stable-sort → last-wins-dedupe policy, carrying a per-row payload through
/// the same fate (the payload follows the winning row).
fn sanitize_rows<P: Clone>(
    times: &[f64],
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    payload_of: impl Fn(usize) -> P,
) -> Result<(SanitizedOhlc, Vec<P>), ValidationError> {
    let n = times.len();
    if open.len() != n || high.len() != n || low.len() != n || close.len() != n {
        return Err(ValidationError::LengthMismatch {
            times: n,
            open: open.len(),
            high: high.len(),
            low: low.len(),
            close: close.len(),
        });
    }

    let mut report = ValidationReport::default();

    // 1. Keep only finite, in-range rows; remember original order for stable sort + last-wins.
    //    All-NaN rows survive as explicit whitespace (reference `{time}`-only items).
    let mut rows: Vec<(i64, [f64; 4], usize, P)> = Vec::with_capacity(n);
    for i in 0..n {
        let t = times[i];
        let v = [open[i], high[i], low[i], close[i]];
        if !t.is_finite() || !(is_whitespace_values(v) || v.iter().copied().all(safe)) {
            report.dropped_invalid += 1;
            continue;
        }
        rows.push((t as i64, v, i, payload_of(i)));
    }

    // 2. Detect out-of-order before sorting (so `reordered` reflects the caller's input).
    report.reordered = rows.windows(2).any(|w| w[0].0 > w[1].0);
    if report.reordered {
        // Stable by time so that, within a duplicate group, original order is preserved and the
        // last original occurrence is the one we keep below.
        rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)));
    }

    // 3. Collapse duplicate timestamps, last-wins. `rows` is time-ascending; equal times are in
    //    ascending original-index order, so the last of each run is the latest-provided bar.
    let mut out = SanitizedOhlc::default();
    out.times.reserve(rows.len());
    let mut payloads: Vec<P> = Vec::with_capacity(rows.len());
    for (t, v, _, payload) in rows {
        if out.times.last() == Some(&t) {
            report.dropped_duplicate += 1;
            let last = out.times.len() - 1;
            out.open[last] = v[0];
            out.high[last] = v[1];
            out.low[last] = v[2];
            out.close[last] = v[3];
            payloads[last] = payload;
        } else {
            out.times.push(t);
            out.open.push(v[0]);
            out.high.push(v[1]);
            out.low.push(v[2]);
            out.close.push(v[3]);
            payloads.push(payload);
        }
    }

    report.accepted = out.times.len();
    out.report = report;
    Ok((out, payloads))
}

/// Owned-input variant used by typed-array hosts. Clean integer-timestamp feeds take ownership of
/// their columns without the intermediate row matrix; malformed or fractional feeds fall back to
/// the fully repairing sanitizer. This keeps the common ingestion path to one JS→WASM copy.
pub fn sanitize_ohlc_owned(
    times: Vec<f64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
) -> Result<SanitizedOhlc, ValidationError> {
    let n = times.len();
    if open.len() != n || high.len() != n || low.len() != n || close.len() != n {
        return Err(ValidationError::LengthMismatch {
            times: n,
            open: open.len(),
            high: high.len(),
            low: low.len(),
            close: close.len(),
        });
    }
    let clean = times
        .windows(2)
        .all(|w| w[0].is_finite() && w[0].fract() == 0.0 && w[0] < w[1])
        && times
            .last()
            .map(|t| t.is_finite() && t.fract() == 0.0)
            .unwrap_or(true)
        && open
            .iter()
            .chain(&high)
            .chain(&low)
            .chain(&close)
            .copied()
            .all(safe);
    if clean {
        let accepted = times.len();
        return Ok(SanitizedOhlc {
            times: times.into_iter().map(|t| t as i64).collect(),
            open,
            high,
            low,
            close,
            report: ValidationReport {
                accepted,
                ..ValidationReport::default()
            },
        });
    }
    sanitize_ohlc(&times, &open, &high, &low, &close)
}

/// Sanitize a single streaming point. Returns `None` (with no effect on the chart) when the point
/// is non-finite or out of range, so a bad tick is dropped instead of corrupting the series.
/// An all-NaN value set is a valid whitespace update (reference `series.update` with a `{time}`-only
/// item replaces the bar with whitespace); a partial NaN set or ±Inf is a bad tick.
pub fn sanitize_point(time: f64, values: [f64; 4]) -> Option<(i64, [f64; 4])> {
    if !time.is_finite() || !(is_whitespace_values(values) || values.iter().copied().all(safe)) {
        return None;
    }
    Some((time as i64, values))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ohlc(times: &[f64], v: &[f64]) -> Result<SanitizedOhlc, ValidationError> {
        // single-value convenience: all four columns equal
        sanitize_ohlc(times, v, v, v, v)
    }

    #[test]
    fn clean_data_passes_through_untouched() {
        let s = ohlc(&[1.0, 2.0, 3.0], &[10.0, 20.0, 30.0]).unwrap();
        assert_eq!(s.times, [1, 2, 3]);
        assert_eq!(s.close, [10.0, 20.0, 30.0]);
        assert!(s.report.is_clean());
        assert_eq!(s.report.accepted, 3);
    }

    #[test]
    fn length_mismatch_is_an_error() {
        let err = sanitize_ohlc(&[1.0, 2.0], &[1.0], &[1.0], &[1.0], &[1.0]).unwrap_err();
        assert!(matches!(
            err,
            ValidationError::LengthMismatch {
                times: 2,
                open: 1,
                ..
            }
        ));
    }

    #[test]
    fn drops_non_finite_and_out_of_range() {
        let times = [1.0, 2.0, 3.0, 4.0, 5.0];
        let vals = [10.0, f64::NAN, f64::INFINITY, MAX_SAFE_VALUE * 2.0, 50.0];
        let s = ohlc(&times, &vals).unwrap();
        // single-value columns: row 2 is all-NaN → explicit whitespace (kept); rows 3,4 drop
        assert_eq!(s.times, [1, 2, 5]);
        assert_eq!(s.close[0], 10.0);
        assert!(s.close[1].is_nan());
        assert_eq!(s.close[2], 50.0);
        assert_eq!(s.report.dropped_invalid, 2);
    }

    #[test]
    fn all_nan_rows_are_kept_as_whitespace() {
        // reference `{time}`-only items: an all-NaN row is explicit whitespace, not invalid data.
        let nan = f64::NAN;
        let s = sanitize_ohlc(
            &[1.0, 2.0, 3.0],
            &[10.0, nan, 30.0],
            &[10.0, nan, 30.0],
            &[10.0, nan, 30.0],
            &[10.0, nan, 30.0],
        )
        .unwrap();
        assert_eq!(s.times, [1, 2, 3]);
        assert!(s.close[1].is_nan());
        assert!(s.open[1].is_nan() && s.high[1].is_nan() && s.low[1].is_nan());
        assert!(s.report.is_clean());
        assert_eq!(s.report.accepted, 3);

        // A partial NaN set is genuinely malformed and still drops, as do ±Inf rows.
        let s = sanitize_ohlc(
            &[1.0, 2.0, 3.0, 4.0],
            &[10.0, nan, 30.0, 40.0],
            &[10.0, 1.0, 30.0, 40.0],
            &[10.0, 1.0, 30.0, 40.0],
            &[10.0, 1.0, 30.0, 40.0],
        )
        .unwrap();
        assert_eq!(s.times, [1, 3, 4]);
        assert_eq!(s.report.dropped_invalid, 1);
        let s = sanitize_ohlc(
            &[1.0, 2.0],
            &[10.0, f64::INFINITY],
            &[10.0, f64::INFINITY],
            &[10.0, f64::INFINITY],
            &[10.0, f64::INFINITY],
        )
        .unwrap();
        assert_eq!(s.times, [1]);
        assert_eq!(s.report.dropped_invalid, 1);
    }

    #[test]
    fn whitespace_rows_sort_dedupe_and_carry_colors_like_real_rows() {
        let nan = f64::NAN;
        let s = sanitize_ohlc_styled(
            &[2.0, 1.0, 3.0],
            &[20.0, 10.0, nan],
            &[20.0, 10.0, nan],
            &[20.0, 10.0, nan],
            &[20.0, 10.0, nan],
            [Some(vec![22, 11, 33]), None, None],
        )
        .unwrap();
        assert_eq!(s.data.times, [1, 2, 3]);
        assert!(s.data.close[2].is_nan());
        assert_eq!(s.colors[0], [11, 22, 33]); // the whitespace row keeps its channel slot
    }

    #[test]
    fn sanitize_point_accepts_whitespace_ticks() {
        let nan = f64::NAN;
        let ws = sanitize_point(2.0, [nan, nan, nan, nan]).unwrap();
        assert_eq!(ws.0, 2);
        assert!(ws.1.iter().all(|v| v.is_nan()));
        // partial NaN / Inf ticks are still dropped
        assert!(sanitize_point(2.0, [1.0, nan, 1.0, 1.0]).is_none());
        assert!(sanitize_point(2.0, [f64::INFINITY; 4]).is_none());
    }

    #[test]
    fn drops_row_with_non_finite_time() {
        let s = ohlc(&[1.0, f64::NAN, 3.0], &[10.0, 20.0, 30.0]).unwrap();
        assert_eq!(s.times, [1, 3]);
        assert_eq!(s.report.dropped_invalid, 1);
    }

    #[test]
    fn sorts_unordered_input() {
        let s = ohlc(&[3.0, 1.0, 2.0], &[30.0, 10.0, 20.0]).unwrap();
        assert_eq!(s.times, [1, 2, 3]);
        assert_eq!(s.close, [10.0, 20.0, 30.0]);
        assert!(s.report.reordered);
        assert_eq!(s.report.dropped_duplicate, 0);
    }

    #[test]
    fn dedupes_last_wins_in_order() {
        // duplicate time 2 appears twice; the later value (25) must win
        let s = ohlc(&[1.0, 2.0, 2.0, 3.0], &[10.0, 20.0, 25.0, 30.0]).unwrap();
        assert_eq!(s.times, [1, 2, 3]);
        assert_eq!(s.close, [10.0, 25.0, 30.0]);
        assert_eq!(s.report.dropped_duplicate, 1);
        assert!(!s.report.reordered);
    }

    #[test]
    fn dedupes_last_wins_after_sort() {
        // out of order AND duplicated: original indices break the tie so the later input wins
        // times: 2(a=20) , 1(=10) , 2(b=99) -> sorted stable: 1, 2a, 2b -> keep 2b
        let s = ohlc(&[2.0, 1.0, 2.0], &[20.0, 10.0, 99.0]).unwrap();
        assert_eq!(s.times, [1, 2]);
        assert_eq!(s.close, [10.0, 99.0]);
        assert!(s.report.reordered);
        assert_eq!(s.report.dropped_duplicate, 1);
    }

    #[test]
    fn empty_input_is_clean_empty_output() {
        let s = ohlc(&[], &[]).unwrap();
        assert!(s.times.is_empty());
        assert!(s.report.is_clean());
        assert_eq!(s.report.accepted, 0);
    }

    #[test]
    fn full_ohlc_columns_are_kept_independent() {
        let s = sanitize_ohlc(
            &[1.0, 2.0],
            &[1.0, 2.0],
            &[5.0, 6.0],
            &[0.5, 1.5],
            &[3.0, 4.0],
        )
        .unwrap();
        assert_eq!(s.open, [1.0, 2.0]);
        assert_eq!(s.high, [5.0, 6.0]);
        assert_eq!(s.low, [0.5, 1.5]);
        assert_eq!(s.close, [3.0, 4.0]);
    }

    #[test]
    fn truncates_fractional_seconds_to_i64() {
        let s = ohlc(&[1.9, 2.4], &[10.0, 20.0]).unwrap();
        assert_eq!(s.times, [1, 2]);
    }

    #[test]
    fn sanitize_point_rejects_bad_ticks() {
        assert!(sanitize_point(f64::NAN, [1.0, 1.0, 1.0, 1.0]).is_none());
        assert!(sanitize_point(1.0, [1.0, f64::INFINITY, 1.0, 1.0]).is_none());
        assert_eq!(
            sanitize_point(1.5, [1.0, 2.0, 0.5, 1.5]),
            Some((1, [1.0, 2.0, 0.5, 1.5]))
        );
    }

    #[test]
    fn styled_dedupe_last_wins_keeps_the_winning_color() {
        // duplicate time 2 appears twice; the later row (value 25, color 99) must win.
        let s = sanitize_ohlc_styled(
            &[1.0, 2.0, 2.0, 3.0],
            &[10.0, 20.0, 25.0, 30.0],
            &[10.0, 20.0, 25.0, 30.0],
            &[10.0, 20.0, 25.0, 30.0],
            &[10.0, 20.0, 25.0, 30.0],
            [Some(vec![10, 20, 99, 30]), None, None],
        )
        .unwrap();
        assert_eq!(s.data.times, [1, 2, 3]);
        assert_eq!(s.data.close, [10.0, 25.0, 30.0]);
        assert_eq!(s.colors[0], [10, 99, 30]);
        assert!(s.colors[1].is_empty() && s.colors[2].is_empty());
        assert_eq!(s.data.report.dropped_duplicate, 1);
    }

    #[test]
    fn styled_sort_carries_colors_with_their_rows() {
        // out of order AND duplicated: stable sort keeps the last original occurrence of
        // time 2 (value 99, color 77).
        let s = sanitize_ohlc_styled(
            &[2.0, 1.0, 2.0],
            &[20.0, 10.0, 99.0],
            &[20.0, 10.0, 99.0],
            &[20.0, 10.0, 99.0],
            &[20.0, 10.0, 99.0],
            [Some(vec![55, 11, 77]), Some(vec![5, 1, 7]), None],
        )
        .unwrap();
        assert_eq!(s.data.times, [1, 2]);
        assert_eq!(s.data.close, [10.0, 99.0]);
        assert_eq!(s.colors[0], [11, 77]);
        assert_eq!(s.colors[1], [1, 7]);
        assert!(s.data.report.reordered);
    }

    #[test]
    fn styled_drops_colors_of_invalid_rows_and_checks_lengths() {
        let s = sanitize_ohlc_styled(
            &[1.0, 2.0, 3.0],
            &[10.0, f64::NAN, 30.0],
            &[10.0, 1.0, 30.0],
            &[10.0, 1.0, 30.0],
            &[10.0, 1.0, 30.0],
            [Some(vec![10, 20, 30]), None, None],
        )
        .unwrap();
        assert_eq!(s.data.times, [1, 3]);
        assert_eq!(s.colors[0], [10, 30]);

        let err = sanitize_ohlc_styled(
            &[1.0, 2.0],
            &[1.0, 2.0],
            &[1.0, 2.0],
            &[1.0, 2.0],
            &[1.0, 2.0],
            [Some(vec![1]), None, None],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            ValidationError::ColorLengthMismatch {
                times: 2,
                channel: "body",
                colors: 1
            }
        ));
    }

    #[test]
    fn owned_clean_input_avoids_repair_path() {
        let s = sanitize_ohlc_owned(
            vec![1.0, 2.0],
            vec![1.0, 2.0],
            vec![2.0, 3.0],
            vec![0.0, 1.0],
            vec![1.5, 2.5],
        )
        .unwrap();
        assert!(s.report.is_clean());
        assert_eq!(s.times, [1, 2]);
    }
}
