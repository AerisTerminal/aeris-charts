//! Viewport-bounded data conflation: when bar spacing drops below one device pixel, rows that
//! share an x pixel are reduced to endpoint/extrema representatives (per series kind).

use super::*;

/// Pick a bounded set of rows when several source points occupy the same physical x pixel.
///
/// The normal-spacing path remains unchanged. Once the source spacing drops below one physical
/// pixel, each bucket keeps its first/last rows plus the close extrema, preserving the visible
/// envelope and the line's endpoints while avoiding an O(number-of-source-points) draw list.
pub(crate) fn visible_line_rows(
    plot: &PlotList,
    from: i64,
    to: i64,
    bar_spacing: f64,
    hpr: f64,
    x_at: impl Fn(i64) -> f64,
) -> Vec<usize> {
    // Whitespace rows (reference `{time}`-only items) draw nothing: dropping them here leaves the
    // surrounding real bars adjacent in the result, so the line connects across the gap
    // exactly like the reference's whitespace-free plot list.
    let visible = plot
        .visible_rows(from, to)
        .filter(|&row| !plot.is_whitespace_row(row));
    if bar_spacing * hpr >= 1.0 {
        return visible.collect();
    }

    let close = plot.column(PlotValueIndex::Close);
    let indices = plot.indices();
    let mut out = Vec::new();
    let mut bucket_rows = Vec::new();
    let mut bucket: Option<i64> = None;

    let flush = |bucket_rows: &mut Vec<usize>, out: &mut Vec<usize>| {
        let (Some(&first), Some(&last)) = (bucket_rows.first(), bucket_rows.last()) else {
            return;
        };
        let mut low = first;
        let mut high = first;
        for &row in bucket_rows.iter().skip(1) {
            if close[row].is_finite() && (!close[low].is_finite() || close[row] < close[low]) {
                low = row;
            }
            if close[row].is_finite() && (!close[high].is_finite() || close[row] > close[high]) {
                high = row;
            }
        }
        let mut selected = [first, low, high, last];
        selected.sort_unstable();
        for row in selected {
            if out.last().copied() != Some(row) {
                out.push(row);
            }
        }
        bucket_rows.clear();
    };

    for row in visible {
        let current_bucket = x_at(indices[row]).floor() as i64;
        if bucket.is_some_and(|previous| previous != current_bucket) {
            flush(&mut bucket_rows, &mut out);
        }
        bucket = Some(current_bucket);
        bucket_rows.push(row);
    }
    flush(&mut bucket_rows, &mut out);
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VisibleOhlc {
    /// Physical-pixel x coordinate. Aggregated buckets are pinned to their integer pixel so
    /// adjacent buckets cannot round back onto the same column in the geometry builders.
    pub(crate) x_px: f64,
    pub(crate) open: f64,
    pub(crate) high: f64,
    pub(crate) low: f64,
    pub(crate) close: f64,
    /// Source row supplying the bar's identity fields (open/high/low/close + per-point colors):
    /// the row itself at normal spacing, the bucket's last row (which owns the close) when
    /// compressed.
    pub(crate) source_row: usize,
    /// Geometry-local adjacency key (the reference's conflated-item `time` in the range hit test): the
    /// actual time-point index at normal spacing, the physical pixel bucket when compressed.
    pub(crate) geometry_time: i64,
}

/// Aggregate source OHLC rows that share a physical x pixel.
///
/// Each compressed bucket is itself a valid OHLC bar: first open, maximum high, minimum low, and
/// last close. At normal spacing this is an identity transform, apart from copying the visible
/// values into the small frame-local item list required by the render geometry builders.
pub(crate) fn visible_ohlc(
    plot: &PlotList,
    from: i64,
    to: i64,
    bar_spacing: f64,
    hpr: f64,
    x_at: impl Fn(i64) -> f64,
) -> Vec<VisibleOhlc> {
    let indices = plot.indices();
    let open = plot.column(PlotValueIndex::Open);
    let high = plot.column(PlotValueIndex::High);
    let low = plot.column(PlotValueIndex::Low);
    let close = plot.column(PlotValueIndex::Close);
    // Whitespace rows draw nothing (the reference's plot list omits them); a compressed bucket only
    // ever aggregates real bars, so no NaN can leak into an extremum or the bucket close.
    let visible = plot
        .visible_rows(from, to)
        .filter(|&row| !plot.is_whitespace_row(row));

    if bar_spacing * hpr >= 1.0 {
        return visible
            .map(|row| VisibleOhlc {
                x_px: x_at(indices[row]),
                open: open[row],
                high: high[row],
                low: low[row],
                close: close[row],
                source_row: row,
                geometry_time: indices[row],
            })
            .collect();
    }

    let mut out = Vec::new();
    let mut current_bucket: Option<i64> = None;
    let mut current: Option<VisibleOhlc> = None;
    for row in visible {
        let bucket = x_at(indices[row]).floor() as i64;
        if current_bucket.is_some_and(|previous| previous != bucket) {
            // `current` is always populated alongside `current_bucket` below.
            if let Some(item) = current.take() {
                out.push(item);
            }
        }

        match current.as_mut() {
            Some(item) => {
                item.high = item.high.max(high[row]);
                item.low = item.low.min(low[row]);
                item.close = close[row];
                item.source_row = row;
            }
            None => {
                current = Some(VisibleOhlc {
                    x_px: bucket as f64,
                    open: open[row],
                    high: high[row],
                    low: low[row],
                    close: close[row],
                    source_row: row,
                    geometry_time: bucket,
                });
            }
        }
        current_bucket = Some(bucket);
    }
    if let Some(item) = current {
        out.push(item);
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VisibleHistogramRow {
    pub(crate) x_px: f64,
    pub(crate) source_row: usize,
    /// Geometry-local adjacency key. It is the actual time-point index at normal spacing and the
    /// physical pixel bucket when compressed.
    pub(crate) geometry_time: i64,
}

/// Select one conservative histogram sample per physical pixel, retaining the value with the
/// greatest magnitude so a volume/value spike cannot disappear merely because the scale is
/// compressed. The selected source row also carries its original up/down color classification.
pub(crate) fn visible_histogram_rows(
    plot: &PlotList,
    from: i64,
    to: i64,
    bar_spacing: f64,
    hpr: f64,
    x_at: impl Fn(i64) -> f64,
) -> Vec<VisibleHistogramRow> {
    let indices = plot.indices();
    let close = plot.column(PlotValueIndex::Close);
    // Whitespace rows draw nothing (the reference's plot list omits them).
    let visible = plot
        .visible_rows(from, to)
        .filter(|&row| !plot.is_whitespace_row(row));
    if bar_spacing * hpr >= 1.0 {
        return visible
            .map(|source_row| VisibleHistogramRow {
                x_px: x_at(indices[source_row]),
                source_row,
                geometry_time: indices[source_row],
            })
            .collect();
    }

    let mut out: Vec<VisibleHistogramRow> = Vec::new();
    for source_row in visible {
        let bucket = x_at(indices[source_row]).floor() as i64;
        match out.last_mut() {
            Some(item) if item.geometry_time == bucket => {
                if close[source_row].abs() > close[item.source_row].abs() {
                    item.source_row = source_row;
                }
            }
            _ => out.push(VisibleHistogramRow {
                x_px: bucket as f64,
                source_row,
                geometry_time: bucket,
            }),
        }
    }
    out
}
