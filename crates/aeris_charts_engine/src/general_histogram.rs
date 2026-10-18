//! Engine-side histogram binning. Every host and backend paints the same bins because the
//! engine owns bin edges, heights, and their validation.

use crate::{
    ChartError, ErrorCode, GeneralHistogramNormalization, GeneralHistogramOptions,
    MAX_GENERAL_HISTOGRAM_BINS,
};

/// Relative tolerance that keeps a sample or range end lying on a bin edge, up to floating-point
/// rounding, in the bin that starts at that edge.
const EDGE_TOLERANCE: f64 = 1e-9;

/// Bins of one histogram series. Bin `i` spans `[edges[i], edges[i + 1])`; the last bin also
/// includes its right edge, so the sample maximum is always counted.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct GeneralHistogramBins {
    edges: Vec<f64>,
    heights: Vec<f64>,
}

impl GeneralHistogramBins {
    pub(crate) fn len(&self) -> usize {
        self.heights.len()
    }

    pub(crate) fn heights(&self) -> &[f64] {
        &self.heights
    }

    pub(crate) fn height(&self, bin: usize) -> Option<f64> {
        self.heights.get(bin).copied()
    }

    pub(crate) fn bounds(&self, bin: usize) -> Option<(f64, f64)> {
        Some((*self.edges.get(bin)?, *self.edges.get(bin + 1)?))
    }

    /// First and last bin edges, or `None` when no valid sample exists.
    pub(crate) fn extent(&self) -> Option<(f64, f64)> {
        Some((*self.edges.first()?, *self.edges.last()?))
    }

    pub(crate) fn max_height(&self) -> Option<f64> {
        self.heights.iter().copied().reduce(f64::max)
    }

    /// Interval label such as `[10, 20)`; the last bin is closed on the right.
    pub(crate) fn label(&self, bin: usize) -> Option<String> {
        let (from, to) = self.bounds(bin)?;
        let close = if bin + 1 == self.len() { ']' } else { ')' };
        Some(format!("[{from}, {to}{close}"))
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + (self.edges.capacity() + self.heights.capacity()) * std::mem::size_of::<f64>()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BinLayout {
    start: f64,
    width: f64,
    count: usize,
    end: f64,
}

impl BinLayout {
    fn bin(&self, value: f64) -> usize {
        let position = snapped_floor((value - self.start) / self.width);
        if position <= 0.0 {
            0
        } else {
            (position as usize).min(self.count - 1)
        }
    }

    fn edge(&self, index: usize) -> f64 {
        if index == self.count {
            self.end
        } else {
            self.start + self.width * index as f64
        }
    }
}

/// Validate the sample-independent binning policy.
pub(crate) fn validate_histogram_options(
    options: &GeneralHistogramOptions,
) -> Result<(), ChartError> {
    if options
        .bin_count
        .is_some_and(|count| count == 0 || count > MAX_GENERAL_HISTOGRAM_BINS)
    {
        return Err(ChartError::new(
            ErrorCode::InvalidOptions,
            format!("histogram bin_count must be between 1 and {MAX_GENERAL_HISTOGRAM_BINS}"),
        ));
    }
    if options
        .bin_width
        .is_some_and(|width| !width.is_finite() || width <= 0.0)
    {
        return Err(ChartError::new(
            ErrorCode::InvalidOptions,
            "histogram bin_width must be finite and positive",
        ));
    }
    Ok(())
}

/// Validate that the valid samples produce a bounded set of finite bins.
pub(crate) fn validate_histogram_samples(
    options: &GeneralHistogramOptions,
    values: &[f64],
    valid: impl Fn(usize) -> bool,
) -> Result<(), ChartError> {
    match sample_range(values, &valid) {
        Some((min, max, samples)) => bin_layout(options, min, max, samples).map(|_| ()),
        None => Ok(()),
    }
}

/// Bin the valid samples. Samples that fail validation produce no bins.
pub(crate) fn compute_histogram_bins(
    options: &GeneralHistogramOptions,
    values: &[f64],
    valid: impl Fn(usize) -> bool,
) -> GeneralHistogramBins {
    let Some((min, max, samples)) = sample_range(values, &valid) else {
        return GeneralHistogramBins::default();
    };
    let Ok(layout) = bin_layout(options, min, max, samples) else {
        return GeneralHistogramBins::default();
    };
    let mut counts = vec![0_u64; layout.count];
    for (row, value) in values.iter().enumerate() {
        if valid(row) && value.is_finite() {
            counts[layout.bin(*value)] += 1;
        }
    }
    let edges = (0..=layout.count)
        .map(|index| layout.edge(index))
        .collect::<Vec<_>>();
    let total = samples as f64;
    let mut running = 0_u64;
    let heights = counts
        .iter()
        .enumerate()
        .map(|(bin, &count)| {
            let count = if options.cumulative {
                running += count;
                running
            } else {
                count
            } as f64;
            match (options.normalization, options.cumulative) {
                (GeneralHistogramNormalization::Count, _) => count,
                (GeneralHistogramNormalization::Percent, _) => count / total * 100.0,
                (GeneralHistogramNormalization::Density, true) => count / total,
                (GeneralHistogramNormalization::Density, false) => {
                    count / (total * (edges[bin + 1] - edges[bin]))
                }
            }
        })
        .collect();
    GeneralHistogramBins { edges, heights }
}

fn sample_range(values: &[f64], valid: &impl Fn(usize) -> bool) -> Option<(f64, f64, usize)> {
    let mut range: Option<(f64, f64, usize)> = None;
    for (row, &value) in values.iter().enumerate() {
        if !valid(row) || !value.is_finite() {
            continue;
        }
        range = Some(match range {
            Some((min, max, samples)) => (min.min(value), max.max(value), samples + 1),
            None => (value, value, 1),
        });
    }
    range
}

fn bin_layout(
    options: &GeneralHistogramOptions,
    min: f64,
    max: f64,
    samples: usize,
) -> Result<BinLayout, ChartError> {
    let span = max - min;
    if !span.is_finite() {
        return Err(ChartError::new(
            ErrorCode::InvalidData,
            "histogram sample range must be finite",
        ));
    }
    if let Some(width) = options.bin_width {
        return aligned_layout(min, max, width);
    }
    if let Some(count) = options.bin_count {
        if span == 0.0 {
            return Ok(BinLayout {
                start: min - 0.5,
                width: 1.0 / count as f64,
                count,
                end: min + 0.5,
            });
        }
        return Ok(BinLayout {
            start: min,
            width: span / count as f64,
            count,
            end: max,
        });
    }
    if span == 0.0 {
        return Ok(BinLayout {
            start: min - 0.5,
            width: 1.0,
            count: 1,
            end: min + 0.5,
        });
    }
    let sturges = (samples as f64).log2().ceil() as usize + 1;
    aligned_layout(min, max, nice_width(span / sturges as f64))
}

/// Bins aligned to integer multiples of `width` that cover `[min, max]`.
fn aligned_layout(min: f64, max: f64, width: f64) -> Result<BinLayout, ChartError> {
    let first = snapped_floor(min / width);
    let last = snapped_ceil(max / width);
    let count = (last - first).max(1.0);
    if !first.is_finite() || !last.is_finite() || count > MAX_GENERAL_HISTOGRAM_BINS as f64 {
        return Err(ChartError::new(
            ErrorCode::InvalidData,
            format!(
                "histogram bin_width produces more than {MAX_GENERAL_HISTOGRAM_BINS} bins for the sample range"
            ),
        ));
    }
    let count = count as usize;
    let start = first * width;
    Ok(BinLayout {
        start,
        width,
        count,
        end: start + width * count as f64,
    })
}

/// Smallest 1, 2, or 5 times a power of ten that is at least `raw`.
fn nice_width(raw: f64) -> f64 {
    let magnitude = 10_f64.powf(raw.log10().floor());
    let fraction = raw / magnitude;
    let nice = if fraction <= 1.0 + EDGE_TOLERANCE {
        1.0
    } else if fraction <= 2.0 + EDGE_TOLERANCE {
        2.0
    } else if fraction <= 5.0 + EDGE_TOLERANCE {
        5.0
    } else {
        10.0
    };
    nice * magnitude
}

fn snapped_floor(value: f64) -> f64 {
    let nearest = value.round();
    if (value - nearest).abs() <= EDGE_TOLERANCE * nearest.abs().max(1.0) {
        nearest
    } else {
        value.floor()
    }
}

fn snapped_ceil(value: f64) -> f64 {
    let nearest = value.round();
    if (value - nearest).abs() <= EDGE_TOLERANCE * nearest.abs().max(1.0) {
        nearest
    } else {
        value.ceil()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> GeneralHistogramOptions {
        GeneralHistogramOptions::default()
    }

    fn bins(options: &GeneralHistogramOptions, values: &[f64]) -> GeneralHistogramBins {
        compute_histogram_bins(options, values, |_| true)
    }

    #[test]
    fn automatic_bins_use_sturges_count_with_nice_aligned_width() {
        // 20 samples -> ceil(log2(20)) + 1 = 6 bins over a span of 47 -> raw width 7.83 -> 10.
        let values = (0..20)
            .map(|value| 3.0 + value as f64 * 47.0 / 19.0)
            .collect::<Vec<_>>();
        let result = bins(&options(), &values);
        assert_eq!(result.extent(), Some((0.0, 50.0)));
        assert_eq!(result.len(), 5);
        assert_eq!(result.bounds(0), Some((0.0, 10.0)));
        assert_eq!(result.heights().iter().sum::<f64>(), 20.0);
        assert_eq!(result.label(0).as_deref(), Some("[0, 10)"));
        assert_eq!(result.label(4).as_deref(), Some("[40, 50]"));
    }

    #[test]
    fn explicit_count_spans_the_exact_sample_range_and_closes_the_last_bin() {
        let mut options = options();
        options.bin_count = Some(4);
        let result = bins(&options, &[0.0, 1.0, 2.0, 3.0, 4.0, 8.0]);
        assert_eq!(result.extent(), Some((0.0, 8.0)));
        assert_eq!(result.heights(), &[2.0, 2.0, 1.0, 1.0]);
        assert_eq!(result.label(3).as_deref(), Some("[6, 8]"));
    }

    #[test]
    fn explicit_width_overrides_count_and_aligns_to_its_multiples() {
        let mut options = options();
        options.bin_count = Some(50);
        options.bin_width = Some(0.1);
        let result = bins(&options, &[0.3, 0.35, 0.6]);
        assert_eq!(result.len(), 3);
        let (start, end) = result.extent().unwrap();
        assert!((start - 0.3).abs() < 1e-12 && (end - 0.6).abs() < 1e-12);
        // 0.3 lies on the first edge and 0.6 on the closed last edge despite float rounding.
        assert_eq!(result.heights(), &[2.0, 0.0, 1.0]);
    }

    #[test]
    fn identical_samples_produce_one_unit_bin_centered_on_the_value() {
        let result = bins(&options(), &[7.0, 7.0, 7.0]);
        assert_eq!(result.extent(), Some((6.5, 7.5)));
        assert_eq!(result.heights(), &[3.0]);
    }

    #[test]
    fn percent_density_and_cumulative_normalizations() {
        let values = [0.0, 1.0, 1.5, 3.0];
        let mut options = options();
        options.bin_width = Some(2.0);
        options.normalization = GeneralHistogramNormalization::Percent;
        assert_eq!(bins(&options, &values).heights(), &[75.0, 25.0]);
        options.normalization = GeneralHistogramNormalization::Density;
        let density = bins(&options, &values);
        assert_eq!(density.heights(), &[0.375, 0.125]);
        let area = density
            .heights()
            .iter()
            .enumerate()
            .map(|(bin, height)| {
                let (from, to) = density.bounds(bin).unwrap();
                height * (to - from)
            })
            .sum::<f64>();
        assert_eq!(area, 1.0);
        options.cumulative = true;
        assert_eq!(bins(&options, &values).heights(), &[0.75, 1.0]);
        options.normalization = GeneralHistogramNormalization::Percent;
        assert_eq!(bins(&options, &values).heights(), &[75.0, 100.0]);
        options.normalization = GeneralHistogramNormalization::Count;
        assert_eq!(bins(&options, &values).heights(), &[3.0, 4.0]);
    }

    #[test]
    fn invalid_samples_are_excluded_and_empty_input_has_no_bins() {
        let values = [1.0, 100.0, 2.0];
        let result = compute_histogram_bins(&options(), &values, |row| row != 1);
        assert_eq!(result.extent(), Some((1.0, 2.0)));
        assert_eq!(result.heights().iter().sum::<f64>(), 2.0);
        assert_eq!(bins(&options(), &[]).len(), 0);
        assert_eq!(
            compute_histogram_bins(&options(), &values, |_| false).len(),
            0
        );
    }

    #[test]
    fn options_and_sample_ranges_are_bounded() {
        let mut options = options();
        options.bin_count = Some(0);
        assert!(validate_histogram_options(&options).is_err());
        options.bin_count = Some(MAX_GENERAL_HISTOGRAM_BINS + 1);
        assert!(validate_histogram_options(&options).is_err());
        options.bin_count = Some(MAX_GENERAL_HISTOGRAM_BINS);
        assert!(validate_histogram_options(&options).is_ok());
        options.bin_width = Some(0.0);
        assert!(validate_histogram_options(&options).is_err());
        options.bin_width = Some(f64::NAN);
        assert!(validate_histogram_options(&options).is_err());
        options.bin_width = Some(1.0);
        assert!(validate_histogram_options(&options).is_ok());
        assert!(validate_histogram_samples(&options, &[0.0, 1_000.0], |_| true).is_ok());
        let error = validate_histogram_samples(&options, &[0.0, 1_000.5], |_| true).unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidData);
        assert!(validate_histogram_samples(&options, &[-f64::MAX, f64::MAX], |_| true).is_err());
        assert_eq!(bins(&options, &[0.0, 1_000.5]).len(), 0);
    }
}
