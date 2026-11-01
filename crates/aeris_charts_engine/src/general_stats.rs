//! G11 statistics core (complete build): shared deterministic math for the statistics series
//! families and the cached per-series models consumed by frame emission, hit tests, tooltips,
//! and accessibility.
//!
//! Numeric policy (stable across hosts and backends):
//! - Only finite `f64` samples participate; invalid/missing rows are excluded before sorting.
//! - Quantiles use the R-7 method (d3 `quantile`): `h = (n - 1) * p`; the result interpolates
//!   linearly between `s[floor(h)]` and `s[floor(h) + 1]`.
//! - The ECDF is a right-continuous step function with ties grouped: a value shared by `k`
//!   sorted positions steps once by `k / n`.
//! - The inverse normal CDF uses Acklam's rational approximation (published constants, maximum
//!   absolute error about 1.15e-9 over the open interval); `p <= 0` maps to `-inf` and
//!   `p >= 1` maps to `+inf`.
//! - KDE grids are evenly spaced over the exact sample range; a degenerate range expands by
//!   +-0.5. Evaluation uses the full sorted sample when it has at most [`MAX_STATS_SAMPLES`]
//!   points and otherwise a deterministic stride over the sorted order.
//! - Bandwidths follow Scott (`sigma * n^-1/5`) and Silverman
//!   (`0.9 * min(sigma, IQR / 1.34) * n^-1/5`) with the sample standard deviation; a zero or
//!   non-finite estimate falls back to `1.0`.
//! - 2D density rasterizes truncated kernels onto the grid nodes within a 3-bandwidth window
//!   (Gaussian) or their exact 1-bandwidth support (Epanechnikov), as in the public d3-contour
//!   algorithm description; `smooth: false` uses raw 2D bin counts on the same grid.
//! - Contour lines use marching squares with linear edge interpolation and the cell-mean
//!   saddle decider; filled bands are the `value >= level` regions painted in ascending level
//!   order so higher bands paint over lower ones.
//! - Hexbin assignment follows the public d3-hexbin axial-offset rounding (`dx = r * sqrt(3)`,
//!   `dy = 1.5 * r`) with halves rounded toward positive infinity like `Math.round`; bin
//!   centers live in plot pixels so a pan or zoom re-bins deterministically.
//! - Beeswarm offsets are placed greedily in sorted value order at hexagonal lane pitch
//!   `r * sqrt(3)`; strip offsets follow a golden-ratio sequence per category; Wilkinson dot
//!   bins use a width of one dot diameter aligned to multiples of the width.

use std::sync::Arc;

use crate::general_data::GeneralDataset;
use crate::general_series::{
    GeneralSeriesKind, GeneralSeriesOptions, GeneralStatsBandwidth, GeneralStatsKernel,
    GeneralStatsOptions,
};
use crate::{ChartError, ErrorCode};

/// Maximum number of samples one KDE evaluation sums over (deterministic sorted stride).
pub(crate) const MAX_STATS_SAMPLES: usize = 4_096;
/// Maximum rows of one parallel-coordinates or scatter-matrix dataset.
pub(crate) const MAX_MATRIX_ROWS: usize = 512;
/// Field-count bounds of a matrix dataset (parallel coordinates, scatter matrix).
pub(crate) const MIN_MATRIX_FIELDS: usize = 2;
pub(crate) const MAX_MATRIX_FIELDS: usize = 8;
/// Default KDE grid resolution (curve points, contour nodes per axis).
pub(crate) const DEFAULT_STATS_GRID: usize = 64;
pub(crate) const MIN_STATS_GRID: usize = 8;
pub(crate) const MAX_STATS_GRID: usize = 256;
/// Contour-specific grid bound so band triangulation stays bounded.
pub(crate) const MAX_CONTOUR_GRID: usize = 96;
pub(crate) const MAX_CONTOUR_THRESHOLDS: usize = 16;
/// Hard caps on emitted contour geometry (deterministic emission order: bands ascending,
/// cells row-major); geometry beyond the cap is dropped.
pub(crate) const MAX_CONTOUR_TRIANGLES: usize = 200_000;
pub(crate) const MAX_CONTOUR_LINE_POINTS: usize = 200_000;
/// Violin and ridgeline run one KDE per category; bound the per-series category count.
pub(crate) const MAX_STATS_CATEGORIES: usize = 64;

/// Row bound for every statistics-family dataset (category, values, numeric, and matrix
/// kinds); KDE and dot placement stride-sample further down to `MAX_STATS_SAMPLES`.
pub(crate) const MAX_STATS_ROWS: usize = 65_536;
/// Hexbin bin count bound; the effective radius is raised until the grid fits 64 columns.
pub(crate) const MAX_HEXBIN_BINS: usize = 4_096;
pub(crate) const MIN_HEXBIN_RADIUS: f64 = 4.0;
pub(crate) const MAX_HEXBIN_RADIUS: f64 = 128.0;
/// Golden-ratio fractional part driving deterministic strip-plot jitter.
const GOLDEN_FRACT: f64 = 0.618_033_988_749_894_9;

fn invalid_data(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::InvalidData, message)
}

fn invalid_options(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::InvalidOptions, message)
}

fn resource(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::ResourceLimit, message)
}

/// `Math.round` semantics: halves round toward positive infinity so hexbin assignment matches
/// the public d3-hexbin algorithm bit for bit on every platform.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// Sorted valid samples of a distribution dataset (`Values` or category `y` column).
pub(crate) fn sorted_samples(dataset: &GeneralDataset) -> Vec<f64> {
    let mut samples: Vec<f64> = dataset
        .y()
        .iter()
        .enumerate()
        .filter(|(row, value)| dataset.y_is_valid(*row) && value.is_finite())
        .map(|(_, value)| *value)
        .collect();
    samples.sort_by(f64::total_cmp);
    samples
}

/// Sorted valid samples of one category, with their dataset rows, in value order.
pub(crate) fn sorted_category_samples(
    dataset: &GeneralDataset,
    category_index: u32,
) -> Vec<(f64, usize)> {
    let Some(indices) = dataset.category_indices() else {
        return Vec::new();
    };
    let mut samples: Vec<(f64, usize)> = indices
        .iter()
        .enumerate()
        .filter(|(row, index)| {
            **index == category_index && dataset.y_is_valid(*row) && dataset.y()[*row].is_finite()
        })
        .map(|(row, _)| (dataset.y()[row], row))
        .collect();
    samples.sort_by(|a, b| f64::total_cmp(&a.0, &b.0));
    samples
}

/// R-7 quantile of a sorted sample (d3 `quantile`).
pub(crate) fn quantile_type7(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() || !p.is_finite() {
        return None;
    }
    let p = p.clamp(0.0, 1.0);
    let h = (sorted.len() - 1) as f64 * p;
    let lower = h.floor() as usize;
    let fraction = h - lower as f64;
    let value = sorted[lower];
    Some(if lower + 1 < sorted.len() {
        value + fraction * (sorted[lower + 1] - value)
    } else {
        value
    })
}

/// Mean and sample (n - 1) standard deviation.
pub(crate) fn mean_stddev(values: &[f64]) -> (f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0);
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    if values.len() < 2 {
        return (mean, 0.0);
    }
    let variance = values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / (values.len() - 1) as f64;
    (mean, variance.sqrt())
}

/// Right-continuous ECDF step points with ties grouped: each entry is `(x, F)` where `F` is
/// the cumulative share after the last sample equal to `x`.
pub(crate) fn ecdf_points(sorted: &[f64]) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    let n = sorted.len();
    let mut index = 0;
    while index < n {
        let mut end = index + 1;
        while end < n && sorted[end] == sorted[index] {
            end += 1;
        }
        points.push((sorted[index], end as f64 / n as f64));
        index = end;
    }
    points
}

/// Inverse standard normal CDF (Acklam's rational approximation, published constants;
/// maximum absolute error about 1.15e-9). Edges map to the infinities.
pub(crate) fn inverse_normal_cdf(p: f64) -> f64 {
    const A: [f64; 6] = [
        -3.969_683_028_665_376e+01,
        2.209_460_984_245_205e+02,
        -2.759_285_104_469_687e+02,
        1.383_577_518_672_69e+02,
        -3.066_479_806_614_716e+01,
        2.506_628_277_459_239e+00,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e+01,
        1.615_858_368_580_409e+02,
        -1.556_989_798_598_866e+02,
        6.680_131_188_771_972e+01,
        -1.328_068_155_288_572e+01,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-03,
        -3.223_964_580_411_365e-01,
        -2.400_758_277_161_838e+00,
        -2.549_732_539_343_734e+00,
        4.374_664_141_464_968e+00,
        2.938_163_982_698_783e+00,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-03,
        3.224_671_290_700_398e-01,
        2.445_134_137_142_996e+00,
        3.754_408_661_907_416e+00,
    ];
    const P_LOW: f64 = 0.02425;
    const P_HIGH: f64 = 1.0 - P_LOW;
    if p.is_nan() {
        return f64::NAN;
    }
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }
    if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= P_HIGH {
        let q = p - 0.5;
        let r = q * q;
        ((((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q)
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    }
}

/// Evenly spaced grid over the exact sample range; a degenerate range expands by +-0.5.
pub(crate) fn grid_points(min: f64, max: f64, grid: usize) -> (Vec<f64>, f64, f64) {
    let grid = grid.max(2);
    let (min, max) = if min == max {
        (min - 0.5, max + 0.5)
    } else {
        (min, max)
    };
    let step = (max - min) / (grid - 1) as f64;
    (
        (0..grid).map(|index| min + index as f64 * step).collect(),
        min,
        max,
    )
}

/// Resolved bandwidth in sample units; a zero or non-finite estimate falls back to 1.0.
pub(crate) fn resolve_bandwidth(
    sorted: &[f64],
    bandwidth: GeneralStatsBandwidth,
) -> Result<f64, ChartError> {
    let n = sorted.len();
    if n == 0 {
        return Ok(1.0);
    }
    let estimate = match bandwidth {
        GeneralStatsBandwidth::Scott => {
            let (_, stddev) = mean_stddev(sorted);
            stddev * (n as f64).powf(-0.2)
        }
        GeneralStatsBandwidth::Silverman => {
            let (_, stddev) = mean_stddev(sorted);
            let iqr = quantile_type7(sorted, 0.75).unwrap_or(0.0)
                - quantile_type7(sorted, 0.25).unwrap_or(0.0);
            0.9 * stddev.min(iqr / 1.34) * (n as f64).powf(-0.2)
        }
        GeneralStatsBandwidth::Explicit(value) => value,
    };
    Ok(if estimate.is_finite() && estimate > 0.0 {
        estimate
    } else {
        1.0
    })
}

fn kernel_weight(kernel: GeneralStatsKernel, u: f64) -> f64 {
    match kernel {
        GeneralStatsKernel::Gaussian => {
            (-0.5 * u * u).exp() * (1.0 / (2.0 * std::f64::consts::PI).sqrt())
        }
        GeneralStatsKernel::Epanechnikov => {
            if u.abs() <= 1.0 {
                0.75 * (1.0 - u * u)
            } else {
                0.0
            }
        }
    }
}

/// Kernel support half-width in bandwidth units (Gaussian truncated at 3).
fn kernel_support(kernel: GeneralStatsKernel) -> f64 {
    match kernel {
        GeneralStatsKernel::Gaussian => 3.0,
        GeneralStatsKernel::Epanechnikov => 1.0,
    }
}

/// One 1D KDE curve over an evenly spaced grid spanning the exact sample range.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct KdeCurve {
    pub grid: Vec<f64>,
    pub density: Vec<f64>,
    pub min: f64,
    pub max: f64,
    pub max_density: f64,
    pub bandwidth: f64,
    pub samples: usize,
}

pub(crate) fn kde_curve(
    sorted: &[f64],
    kernel: GeneralStatsKernel,
    bandwidth: GeneralStatsBandwidth,
    grid: usize,
) -> Result<Option<KdeCurve>, ChartError> {
    if sorted.is_empty() {
        return Ok(None);
    }
    let h = resolve_bandwidth(sorted, bandwidth)?;
    let (grid_points, min, max) = grid_points(sorted[0], sorted[sorted.len() - 1], grid);
    // Deterministic stride downsampling keeps the KDE bounded on large datasets.
    let stride = sorted.len().div_ceil(MAX_STATS_SAMPLES);
    let used: Vec<f64> = sorted.iter().step_by(stride).copied().collect();
    let n = used.len() as f64;
    let support = kernel_support(kernel) * h;
    let density: Vec<f64> = grid_points
        .iter()
        .map(|&point| {
            used.iter()
                .filter(|sample| (**sample - point).abs() <= support)
                .map(|sample| kernel_weight(kernel, (point - sample) / h))
                .sum::<f64>()
                / (n * h)
        })
        .collect();
    let max_density = density.iter().copied().fold(0.0, f64::max);
    Ok(Some(KdeCurve {
        grid: grid_points,
        density,
        min,
        max,
        max_density,
        bandwidth: h,
        samples: sorted.len(),
    }))
}

/// One violin category: its KDE silhouette and quartile overlay anchors.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ViolinCategoryModel {
    pub category_index: u32,
    /// First dataset row of the category (tooltip/identity anchor).
    pub row: usize,
    pub curve: KdeCurve,
    pub q1: f64,
    pub median: f64,
    pub q3: f64,
    pub min: f64,
    pub max: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ViolinModel {
    pub categories: Vec<ViolinCategoryModel>,
    pub max_density: f64,
}

pub(crate) fn violin_model(
    dataset: &GeneralDataset,
    options: &GeneralStatsOptions,
) -> Result<Option<ViolinModel>, ChartError> {
    let Some(categories) = dataset.categories() else {
        return Ok(None);
    };
    if categories.len() > MAX_STATS_CATEGORIES {
        return Err(resource(format!(
            "violin supports at most {MAX_STATS_CATEGORIES} categories"
        )));
    }
    let grid = resolved_grid(options.grid);
    let mut models = Vec::new();
    for (category_index, _) in categories.iter().enumerate() {
        let samples = sorted_category_samples(dataset, category_index as u32);
        if samples.is_empty() {
            continue;
        }
        let sorted: Vec<f64> = samples.iter().map(|(value, _)| *value).collect();
        let Some(curve) = kde_curve(&sorted, options.kernel, options.bandwidth, grid)? else {
            continue;
        };
        models.push(ViolinCategoryModel {
            category_index: category_index as u32,
            row: samples[0].1,
            q1: quantile_type7(&sorted, 0.25).unwrap_or(0.0),
            median: quantile_type7(&sorted, 0.5).unwrap_or(0.0),
            q3: quantile_type7(&sorted, 0.75).unwrap_or(0.0),
            min: sorted[0],
            max: sorted[sorted.len() - 1],
            curve,
        });
    }
    if models.is_empty() {
        return Ok(None);
    }
    let max_density = models
        .iter()
        .map(|model| model.curve.max_density)
        .fold(0.0, f64::max);
    Ok(Some(ViolinModel {
        categories: models,
        max_density,
    }))
}

/// Density-curve model: one KDE curve over the sample range.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DensityModel {
    pub curve: KdeCurve,
}

pub(crate) fn density_model(
    dataset: &GeneralDataset,
    options: &GeneralStatsOptions,
) -> Result<Option<DensityModel>, ChartError> {
    let samples = sorted_samples(dataset);
    let Some(curve) = kde_curve(
        &samples,
        options.kernel,
        options.bandwidth,
        resolved_grid(options.grid),
    )?
    else {
        return Ok(None);
    };
    Ok(Some(DensityModel { curve }))
}

/// Ridgeline model: one KDE per category over a shared grid spanning the global range, so
/// ridges align across categories.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RidgelineCategoryModel {
    pub category_index: u32,
    pub row: usize,
    pub curve: KdeCurve,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RidgelineModel {
    pub categories: Vec<RidgelineCategoryModel>,
    pub min: f64,
    pub max: f64,
    pub max_density: f64,
}

pub(crate) fn ridgeline_model(
    dataset: &GeneralDataset,
    options: &GeneralStatsOptions,
) -> Result<Option<RidgelineModel>, ChartError> {
    let Some(categories) = dataset.categories() else {
        return Ok(None);
    };
    if categories.len() > MAX_STATS_CATEGORIES {
        return Err(resource(format!(
            "ridgeline supports at most {MAX_STATS_CATEGORIES} categories"
        )));
    }
    let all = sorted_samples(dataset);
    if all.is_empty() {
        return Ok(None);
    }
    let bandwidth = resolve_bandwidth(&all, options.bandwidth)?;
    let grid = resolved_grid(options.grid);
    let (grid_points, min, max) = grid_points(all[0], all[all.len() - 1], grid);
    let mut models = Vec::new();
    let mut max_density: f64 = 0.0;
    for (category_index, _) in categories.iter().enumerate() {
        let samples = sorted_category_samples(dataset, category_index as u32);
        if samples.is_empty() {
            continue;
        }
        let sorted: Vec<f64> = samples.iter().map(|(value, _)| *value).collect();
        let stride = sorted.len().div_ceil(MAX_STATS_SAMPLES);
        let used: Vec<f64> = sorted.iter().step_by(stride).copied().collect();
        let n = used.len() as f64;
        let support = kernel_support(options.kernel) * bandwidth;
        let density: Vec<f64> = grid_points
            .iter()
            .map(|&point| {
                used.iter()
                    .filter(|sample| (**sample - point).abs() <= support)
                    .map(|sample| kernel_weight(options.kernel, (point - sample) / bandwidth))
                    .sum::<f64>()
                    / (n * bandwidth)
            })
            .collect();
        let category_max = density.iter().copied().fold(0.0, f64::max);
        max_density = max_density.max(category_max);
        models.push(RidgelineCategoryModel {
            category_index: category_index as u32,
            row: samples[0].1,
            curve: KdeCurve {
                grid: grid_points.clone(),
                density,
                min,
                max,
                max_density: category_max,
                bandwidth,
                samples: sorted.len(),
            },
        });
    }
    if models.is_empty() {
        return Ok(None);
    }
    Ok(Some(RidgelineModel {
        categories: models,
        min,
        max,
        max_density,
    }))
}

/// ECDF model: grouped step points plus the sample count.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EcdfModel {
    pub points: Vec<(f64, f64)>,
    pub count: usize,
}

pub(crate) fn ecdf_model(dataset: &GeneralDataset) -> Option<EcdfModel> {
    let samples = sorted_samples(dataset);
    if samples.is_empty() {
        return None;
    }
    Some(EcdfModel {
        points: ecdf_points(&samples),
        count: samples.len(),
    })
}

/// Normal Q-Q model: `(theoretical, sample)` pairs and the robust quartile reference line
/// through `(Phi^-1(0.25), q1)` and `(Phi^-1(0.75), q3)`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QqModel {
    /// `(theoretical, sample)` pairs with their dataset rows.
    pub points: Vec<(f64, f64, usize)>,
    /// Reference line as `(x0, y0, x1, y1)` in theoretical/sample units.
    pub line: Option<(f64, f64, f64, f64)>,
    pub theoretical_min: f64,
    pub theoretical_max: f64,
    pub sample_min: f64,
    pub sample_max: f64,
}

pub(crate) fn qq_model(dataset: &GeneralDataset) -> Option<QqModel> {
    let mut samples: Vec<(f64, usize)> = dataset
        .y()
        .iter()
        .enumerate()
        .filter(|(row, value)| dataset.y_is_valid(*row) && value.is_finite())
        .map(|(row, value)| (*value, row))
        .collect();
    samples.sort_by(|a, b| f64::total_cmp(&a.0, &b.0));
    let n = samples.len();
    if n == 0 {
        return None;
    }
    let points: Vec<(f64, f64, usize)> = samples
        .iter()
        .enumerate()
        .map(|(index, (value, row))| {
            (
                inverse_normal_cdf((index + 1) as f64 / (n + 1) as f64),
                *value,
                *row,
            )
        })
        .collect();
    let sorted: Vec<f64> = samples.iter().map(|(value, _)| *value).collect();
    let line = if n >= 2 {
        let t1 = inverse_normal_cdf(0.25);
        let t3 = inverse_normal_cdf(0.75);
        let q1 = quantile_type7(&sorted, 0.25).unwrap_or(0.0);
        let q3 = quantile_type7(&sorted, 0.75).unwrap_or(0.0);
        let slope = if (t3 - t1).abs() > f64::EPSILON {
            (q3 - q1) / (t3 - t1)
        } else {
            0.0
        };
        let intercept = q1 - slope * t1;
        let x0 = points[0].0;
        let x1 = points[n - 1].0;
        Some((x0, slope * x0 + intercept, x1, slope * x1 + intercept))
    } else {
        None
    };
    let theoretical_min = points[0].0;
    let theoretical_max = points[n - 1].0;
    Some(QqModel {
        points,
        line,
        theoretical_min,
        theoretical_max,
        sample_min: sorted[0],
        sample_max: sorted[n - 1],
    })
}

/// One hexagon bin in plot pixels.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HexbinBin {
    pub cx: f64,
    pub cy: f64,
    pub count: usize,
    /// First row of the bin (tooltip/identity anchor).
    pub row: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HexbinModel {
    pub bins: Vec<HexbinBin>,
    pub max_count: usize,
    pub radius: f64,
}

/// Resolved hexbin radius: the configured value (default 16 CSS px), raised when the plot
/// would otherwise exceed [`MAX_HEXBIN_BINS`] cells so the frame stays bounded.
pub(crate) fn resolved_hexbin_radius(
    options: &GeneralStatsOptions,
    width: f64,
    height: f64,
) -> f64 {
    let configured = if options.hexbin_radius == 0.0 {
        16.0
    } else {
        options
            .hexbin_radius
            .clamp(MIN_HEXBIN_RADIUS, MAX_HEXBIN_RADIUS)
    };
    if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
        // A pointy-top hexagon covers about 2.6 * r^2 square px.
        let budget_radius = (width * height / (2.6 * MAX_HEXBIN_BINS as f64)).sqrt();
        configured.max(budget_radius)
    } else {
        configured
    }
}

/// d3-hexbin axial-offset assignment over plot-pixel points `(x, y, row)`.
pub(crate) fn hexbin_model(points: &[(f64, f64, usize)], radius: f64) -> HexbinModel {
    let dx = radius * 3.0_f64.sqrt();
    let dy = radius * 1.5;
    let mut bins: Vec<HexbinBin> = Vec::new();
    let mut lookup: std::collections::HashMap<(i64, i64), usize> = std::collections::HashMap::new();
    for &(x, y, row) in points {
        let py = y / dy;
        let mut pj = js_round(py) as i64;
        let px = x / dx - (pj.rem_euclid(2) as f64) / 2.0;
        let mut pi = js_round(px) as i64;
        let py1 = py - pj as f64;
        if py1.abs() * 3.0 > 1.0 {
            let px1 = px - pi as f64;
            let pi2 = pi as f64 + if px < pi as f64 { -1.0 } else { 1.0 } / 2.0;
            let pj2 = pj + if py < pj as f64 { -1 } else { 1 };
            let px2 = px - pi2;
            let py2 = py - pj2 as f64;
            if px1 * px1 + py1 * py1 > px2 * px2 + py2 * py2 {
                pi = (pi2 + if pj.rem_euclid(2) == 1 { 1.0 } else { -1.0 } / 2.0) as i64;
                pj = pj2;
            }
        }
        let key = (pi, pj);
        if let Some(&index) = lookup.get(&key) {
            bins[index].count += 1;
        } else {
            lookup.insert(key, bins.len());
            bins.push(HexbinBin {
                cx: (pi as f64 + (pj.rem_euclid(2) as f64) / 2.0) * dx,
                cy: pj as f64 * dy,
                count: 1,
                row,
            });
        }
    }
    let max_count = bins.iter().map(|bin| bin.count).max().unwrap_or(0);
    HexbinModel {
        bins,
        max_count,
        radius,
    }
}

/// Point-in-hexagon test for a pointy-top hexagon with circumradius `radius`.
pub(crate) fn hexbin_contains(radius: f64, dx: f64, dy: f64) -> bool {
    let half_width = radius * 3.0_f64.sqrt() / 2.0;
    dx.abs() <= half_width && dx.abs() + 3.0_f64.sqrt() * dy.abs() <= 3.0_f64.sqrt() * radius
}

/// Hexagon corners (pointy-top, circumradius `radius`) around `(cx, cy)`.
pub(crate) fn hexbin_polygon(cx: f64, cy: f64, radius: f64) -> [[f64; 2]; 6] {
    let half = radius * 3.0_f64.sqrt() / 2.0;
    [
        [cx + half, cy - radius / 2.0],
        [cx, cy - radius],
        [cx - half, cy - radius / 2.0],
        [cx - half, cy + radius / 2.0],
        [cx, cy + radius],
        [cx + half, cy + radius / 2.0],
    ]
}

/// Raw 2D bin counts on an `nx * ny` node grid; edges align like histogram bins (half-open
/// with the last cell closed on the right).
pub(crate) fn bin2d(
    points: &[(f64, f64)],
    nx: usize,
    ny: usize,
    x_range: (f64, f64),
    y_range: (f64, f64),
) -> Vec<f64> {
    let mut cells = vec![0.0; nx * ny];
    let x_span = x_range.1 - x_range.0;
    let y_span = y_range.1 - y_range.0;
    for &(x, y) in points {
        let xi = if x_span > 0.0 {
            let index = ((x - x_range.0) / x_span * nx as f64).floor() as i64;
            index.clamp(0, nx as i64 - 1) as usize
        } else {
            0
        };
        let yi = if y_span > 0.0 {
            let index = ((y - y_range.0) / y_span * ny as f64).floor() as i64;
            index.clamp(0, ny as i64 - 1) as usize
        } else {
            0
        };
        cells[yi * nx + xi] += 1.0;
    }
    cells
}

/// 2D KDE rasterized onto grid nodes (row-major `ny * nx`), as in the public d3-contour
/// density description: each sample adds its truncated kernel weight to nearby nodes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn kde2d(
    points: &[(f64, f64)],
    kernel: GeneralStatsKernel,
    hx: f64,
    hy: f64,
    nx: usize,
    ny: usize,
    x_range: (f64, f64),
    y_range: (f64, f64),
) -> Vec<f64> {
    let mut values = vec![0.0; nx * ny];
    let n = points.len() as f64;
    if n == 0.0 || hx <= 0.0 || hy <= 0.0 {
        return values;
    }
    let support = kernel_support(kernel);
    let dx = if nx > 1 {
        (x_range.1 - x_range.0) / (nx - 1) as f64
    } else {
        1.0
    };
    let dy = if ny > 1 {
        (y_range.1 - y_range.0) / (ny - 1) as f64
    } else {
        1.0
    };
    for &(x, y) in points {
        let wx = support * hx;
        let wy = support * hy;
        let i0 = (((x - wx) - x_range.0) / dx).ceil().max(0.0) as usize;
        let i1 = ((((x + wx) - x_range.0) / dx).floor().max(-1.0) as i64)
            .min(nx as i64 - 1)
            .max(-1);
        let j0 = (((y - wy) - y_range.0) / dy).ceil().max(0.0) as usize;
        let j1 = ((((y + wy) - y_range.0) / dy).floor().max(-1.0) as i64)
            .min(ny as i64 - 1)
            .max(-1);
        if i1 < 0 || j1 < 0 {
            continue;
        }
        let (i1, j1) = (i1 as usize, j1 as usize);
        let kx: Vec<f64> = (i0..=i1)
            .map(|i| kernel_weight(kernel, (x_range.0 + i as f64 * dx - x) / hx))
            .collect();
        for j in j0..=j1 {
            let ky = kernel_weight(kernel, (y_range.0 + j as f64 * dy - y) / hy);
            if ky == 0.0 {
                continue;
            }
            for (offset, &kx_value) in kx.iter().enumerate() {
                if kx_value != 0.0 {
                    values[j * nx + i0 + offset] += kx_value * ky / (n * hx * hy);
                }
            }
        }
    }
    values
}

/// Automatic contour thresholds: `count` evenly spaced levels of the grid maximum.
pub(crate) fn contour_thresholds(max_value: f64, count: usize) -> Vec<f64> {
    let count = count.clamp(1, MAX_CONTOUR_THRESHOLDS);
    if !max_value.is_finite() || max_value <= 0.0 {
        return Vec::new();
    }
    (1..=count)
        .map(|k| max_value * k as f64 / (count + 1) as f64)
        .collect()
}

/// One filled contour band: the `value >= level` region as data-space triangles.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ContourBand {
    pub level: f64,
    pub next: Option<f64>,
    pub triangles: Vec<[f64; 2]>,
}

/// One iso-line level: stitched polylines in data space (closed loops repeat their first
/// point).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ContourLine {
    pub level: f64,
    pub paths: Vec<Vec<[f64; 2]>>,
}

/// Contour model: the grid plus precomputed bands and iso-lines in data space.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ContourModel {
    pub nx: usize,
    pub ny: usize,
    pub x0: f64,
    pub y0: f64,
    pub dx: f64,
    pub dy: f64,
    pub thresholds: Vec<f64>,
    pub max_value: f64,
    pub bands: Vec<ContourBand>,
    pub lines: Vec<ContourLine>,
}

type EdgeKey = (u32, u32, u8);

fn edge_point(key: EdgeKey, t: f64, x0: f64, y0: f64, dx: f64, dy: f64) -> [f64; 2] {
    let (i, j, orientation) = key;
    if orientation == 0 {
        [x0 + (i as f64 + t) * dx, y0 + j as f64 * dy]
    } else {
        [x0 + i as f64 * dx, y0 + (j as f64 + t) * dy]
    }
}

/// Marching squares over a node grid: filled `value >= level` triangles per threshold and
/// stitched iso-lines. Deterministic: cells are visited row-major and line paths start from
/// the lowest unvisited segment.
#[allow(clippy::too_many_arguments)]
pub(crate) fn marching_squares(
    values: &[f64],
    nx: usize,
    ny: usize,
    x0: f64,
    y0: f64,
    dx: f64,
    dy: f64,
    thresholds: &[f64],
) -> (Vec<ContourBand>, Vec<ContourLine>) {
    let mut bands: Vec<ContourBand> = thresholds
        .iter()
        .enumerate()
        .map(|(index, &level)| ContourBand {
            level,
            next: thresholds.get(index + 1).copied(),
            triangles: Vec::new(),
        })
        .collect();
    let mut lines: Vec<ContourLine> = thresholds
        .iter()
        .map(|&level| ContourLine {
            level,
            paths: Vec::new(),
        })
        .collect();
    let mut triangle_budget = MAX_CONTOUR_TRIANGLES;
    for (band_index, &level) in thresholds.iter().enumerate() {
        if triangle_budget == 0 {
            break;
        }
        let mut segments: Vec<(EdgeKey, EdgeKey, f64, f64)> = Vec::new();
        for j in 0..ny.saturating_sub(1) {
            for i in 0..nx.saturating_sub(1) {
                let tl = values[j * nx + i];
                let tr = values[j * nx + i + 1];
                let br = values[(j + 1) * nx + i + 1];
                let bl = values[(j + 1) * nx + i];
                let case = (u8::from(tl > level) << 3)
                    | (u8::from(tr > level) << 2)
                    | (u8::from(br > level) << 1)
                    | u8::from(bl > level);
                if case == 0 {
                    continue;
                }
                // Canonical edges: 0 = top (H at (i, j)), 1 = right (V at (i + 1, j)),
                // 2 = bottom (H at (i, j + 1)), 3 = left (V at (i, j)). Crossing fractions
                // are measured along the canonical orientation (left-to-right, top-to-bottom).
                let crossing = |from: f64, to: f64| -> f64 {
                    let span = to - from;
                    if span.abs() < f64::EPSILON {
                        0.5
                    } else {
                        ((level - from) / span).clamp(0.0, 1.0)
                    }
                };
                let crossings = [
                    ((tl > level) != (tr > level)).then(|| crossing(tl, tr)),
                    ((tr > level) != (br > level)).then(|| crossing(tr, br)),
                    ((bl > level) != (br > level)).then(|| crossing(bl, br)),
                    ((tl > level) != (bl > level)).then(|| crossing(tl, bl)),
                ];
                let keys: [EdgeKey; 4] = [
                    (i as u32, j as u32, 0),
                    ((i + 1) as u32, j as u32, 1),
                    (i as u32, (j + 1) as u32, 0),
                    (i as u32, j as u32, 1),
                ];
                // Filled region `value >= level`: corners above the level plus edge crossings
                // in boundary order (tl -> tr -> br -> bl). Saddle cases split into two
                // triangles when the cell mean is below the level.
                let corners = [
                    (tl > level, [0.0, 0.0], tl),
                    (tr > level, [1.0, 0.0], tr),
                    (br > level, [1.0, 1.0], br),
                    (bl > level, [0.0, 1.0], bl),
                ];
                let saddle = case == 0b0101 || case == 0b1010;
                let mean = (tl + tr + br + bl) / 4.0;
                let connected = !saddle || mean > level;
                let mut polygon: Vec<[f64; 2]> = Vec::new();
                if connected || !saddle {
                    for edge in 0..4 {
                        let corner = corners[edge];
                        if corner.0 {
                            polygon.push([
                                x0 + (i as f64 + corner.1[0]) * dx,
                                y0 + (j as f64 + corner.1[1]) * dy,
                            ]);
                        }
                        if let Some(t) = crossings[edge] {
                            polygon.push(edge_point(keys[edge], t, x0, y0, dx, dy));
                        }
                    }
                }
                if saddle && !connected {
                    // Two disconnected above-level corners, each with its two crossings.
                    let (first, second) = if case == 0b0101 {
                        // tr and bl are above: tr uses crossings 0/1, bl uses 2/3.
                        ((1usize, [0usize, 1usize]), (3usize, [2usize, 3usize]))
                    } else {
                        // tl and br are above: tl uses crossings 3/0, br uses 1/2.
                        ((0usize, [3usize, 0usize]), (2usize, [1usize, 2usize]))
                    };
                    for (corner_index, edge_pair) in [first, second] {
                        let corner = corners[corner_index];
                        let point = [
                            x0 + (i as f64 + corner.1[0]) * dx,
                            y0 + (j as f64 + corner.1[1]) * dy,
                        ];
                        let mut piece = vec![point];
                        for edge in edge_pair {
                            if let Some(t) = crossings[edge] {
                                piece.push(edge_point(keys[edge], t, x0, y0, dx, dy));
                            }
                        }
                        if piece.len() >= 3 && triangle_budget > 0 {
                            bands[band_index].triangles.extend_from_slice(&piece);
                            triangle_budget -= 1;
                        }
                    }
                } else if polygon.len() >= 3 {
                    let anchor = polygon[0];
                    for index in 1..polygon.len() - 1 {
                        if triangle_budget == 0 {
                            break;
                        }
                        bands[band_index].triangles.extend_from_slice(&[
                            anchor,
                            polygon[index],
                            polygon[index + 1],
                        ]);
                        triangle_budget -= 1;
                    }
                }
                // Iso-line segments for this cell.
                let mut push_segment = |first: usize, second: usize| {
                    if let (Some(ta), Some(tb)) = (crossings[first], crossings[second]) {
                        segments.push((keys[first], keys[second], ta, tb));
                    }
                };
                match case {
                    0b0001 | 0b1110 => push_segment(2, 3),
                    0b0010 | 0b1101 => push_segment(1, 2),
                    0b0011 | 0b1100 => push_segment(1, 3),
                    0b0100 | 0b1011 => push_segment(0, 1),
                    0b0110 | 0b1001 => push_segment(0, 2),
                    0b0111 | 0b1000 => push_segment(0, 3),
                    0b0101 => {
                        // Saddle: tr/bl above. Connected -> cut off tl (0-3) and br (1-2);
                        // disconnected keeps the tr (0-1) and bl (2-3) corners.
                        if mean > level {
                            push_segment(0, 3);
                            push_segment(1, 2);
                        } else {
                            push_segment(0, 1);
                            push_segment(2, 3);
                        }
                    }
                    0b1010 => {
                        // Saddle: tl/br above. Connected -> cut off tr (0-1) and bl (2-3);
                        // disconnected keeps the tl (0-3) and br (1-2) corners.
                        if mean > level {
                            push_segment(0, 1);
                            push_segment(2, 3);
                        } else {
                            push_segment(0, 3);
                            push_segment(1, 2);
                        }
                    }
                    _ => {}
                }
            }
        }
        // Stitch this level's segments into paths.
        let mut by_edge: std::collections::HashMap<EdgeKey, Vec<usize>> =
            std::collections::HashMap::new();
        for (index, (ka, kb, _, _)) in segments.iter().enumerate() {
            by_edge.entry(*ka).or_default().push(index);
            by_edge.entry(*kb).or_default().push(index);
        }
        let mut visited = vec![false; segments.len()];
        let mut point_budget = MAX_CONTOUR_LINE_POINTS;
        for start in 0..segments.len() {
            if visited[start] {
                continue;
            }
            visited[start] = true;
            let (ka, kb, ta, tb) = segments[start];
            let mut path: Vec<([f64; 2], EdgeKey)> = vec![
                (edge_point(ka, ta, x0, y0, dx, dy), ka),
                (edge_point(kb, tb, x0, y0, dx, dy), kb),
            ];
            // Extend the tail, then the head, through shared edges.
            loop {
                let (_, key) = *path.last().expect("nonempty path");
                let next = by_edge.get(&key).and_then(|candidates| {
                    candidates.iter().copied().find(|index| !visited[*index])
                });
                let Some(next) = next else { break };
                visited[next] = true;
                let (na, nb, nta, ntb) = segments[next];
                let (point, far) = if na == key {
                    (edge_point(nb, ntb, x0, y0, dx, dy), nb)
                } else {
                    (edge_point(na, nta, x0, y0, dx, dy), na)
                };
                path.push((point, far));
                if point_budget == 0 {
                    break;
                }
                point_budget -= 1;
            }
            loop {
                let (_, key) = *path.first().expect("nonempty path");
                let next = by_edge.get(&key).and_then(|candidates| {
                    candidates.iter().copied().find(|index| !visited[*index])
                });
                let Some(next) = next else { break };
                visited[next] = true;
                let (na, nb, nta, ntb) = segments[next];
                let (point, far) = if na == key {
                    (edge_point(nb, ntb, x0, y0, dx, dy), nb)
                } else {
                    (edge_point(na, nta, x0, y0, dx, dy), na)
                };
                path.insert(0, (point, far));
                if point_budget == 0 {
                    break;
                }
                point_budget -= 1;
            }
            // A closed loop's tail extension already ends on the first edge, so the first and
            // last points coincide; open chains simply stop at their boundary edges.
            let polyline: Vec<[f64; 2]> = path.into_iter().map(|(point, _)| point).collect();
            if polyline.len() >= 2 {
                lines[band_index].paths.push(polyline);
            }
        }
    }
    (bands, lines)
}

/// Contour model over valid numeric sample pairs; the grid spans the exact data extent.
pub(crate) fn contour_model(
    dataset: &GeneralDataset,
    options: &GeneralStatsOptions,
) -> Result<Option<ContourModel>, ChartError> {
    let Some(xs) = dataset.numeric_x() else {
        return Ok(None);
    };
    let points: Vec<(f64, f64)> = xs
        .iter()
        .zip(dataset.y())
        .enumerate()
        .filter(|(row, (x, y))| dataset.y_is_valid(*row) && x.is_finite() && y.is_finite())
        .map(|(_, (x, y))| (*x, *y))
        .collect();
    if points.is_empty() {
        return Ok(None);
    }
    let x_min = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let x_max = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let y_min = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let y_max = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let grid = resolved_grid(options.grid).min(MAX_CONTOUR_GRID);
    let (_, x0, x1) = grid_points(x_min, x_max, grid);
    let (_, y0, y1) = grid_points(y_min, y_max, grid);
    let x_range = (x0, x1);
    let y_range = (y0, y1);
    let values = if options.smooth {
        // Deterministic stride downsampling for very large datasets.
        let stride = points.len().div_ceil(MAX_STATS_SAMPLES);
        let used: Vec<(f64, f64)> = points.iter().step_by(stride).copied().collect();
        let sorted_x: Vec<f64> = {
            let mut values: Vec<f64> = used.iter().map(|p| p.0).collect();
            values.sort_by(f64::total_cmp);
            values
        };
        let sorted_y: Vec<f64> = {
            let mut values: Vec<f64> = used.iter().map(|p| p.1).collect();
            values.sort_by(f64::total_cmp);
            values
        };
        let hx = resolve_bandwidth(&sorted_x, options.bandwidth)?;
        let hy = resolve_bandwidth(&sorted_y, options.bandwidth)?;
        kde2d(&used, options.kernel, hx, hy, grid, grid, x_range, y_range)
    } else {
        bin2d(&points, grid, grid, x_range, y_range)
    };
    let max_value = values.iter().copied().fold(0.0, f64::max);
    if max_value <= 0.0 {
        return Ok(None);
    }
    let thresholds = if options.thresholds > 0 {
        contour_thresholds(max_value, options.thresholds)
    } else {
        contour_thresholds(max_value, 8)
    };
    let dx = (x1 - x0) / (grid - 1) as f64;
    let dy = (y1 - y0) / (grid - 1) as f64;
    let (bands, lines) = marching_squares(&values, grid, grid, x0, y0, dx, dy, &thresholds);
    Ok(Some(ContourModel {
        nx: grid,
        ny: grid,
        x0,
        y0,
        dx,
        dy,
        thresholds,
        max_value,
        bands,
        lines,
    }))
}

/// Deterministic strip-plot offsets per category: the golden-ratio fractional sequence in
/// dataset row order, scaled to `+- extent`.
pub(crate) fn strip_offsets(rows: &[usize], extent: f64) -> Vec<(usize, f64)> {
    rows.iter()
        .enumerate()
        .map(|(ordinal, &row)| {
            let fraction = ((ordinal as f64 + 1.0) * GOLDEN_FRACT).fract();
            (row, (fraction * 2.0 - 1.0) * extent)
        })
        .collect()
}

/// Deterministic beeswarm offsets (plot px) per row. Points are placed greedily in sorted
/// value order; candidate lanes are `0, +p, -p, +2p, -2p, ...` with pitch `r * sqrt(3)`.
pub(crate) fn beeswarm_offsets(
    sorted: &[(f64, usize)],
    radius: f64,
    px_per_value: f64,
    half_slot: f64,
) -> Vec<(usize, f64)> {
    let pitch = radius * 3.0_f64.sqrt();
    let diameter = radius * 2.0;
    let max_lane = if pitch > 0.0 {
        ((half_slot - radius).max(0.0) / pitch).floor() as i64
    } else {
        0
    };
    let mut placed: Vec<(f64, f64)> = Vec::new(); // (value px, offset)
    let mut offsets: Vec<(usize, f64)> = Vec::with_capacity(sorted.len());
    for &(value, row) in sorted {
        let vx = value * px_per_value;
        let mut chosen: Option<f64> = None;
        let mut fallback = (usize::MAX, 0.0);
        let mut lane = 0_i64;
        loop {
            if lane > max_lane {
                break;
            }
            for candidate in if lane == 0 {
                vec![0.0]
            } else {
                vec![lane as f64 * pitch, -(lane as f64) * pitch]
            } {
                let mut overlaps = 0_usize;
                let mut collides = false;
                for &(pv, po) in placed.iter().rev() {
                    if vx - pv > diameter {
                        break; // sorted: earlier points cannot overlap any more
                    }
                    let dv = vx - pv;
                    let d2 = dv * dv + (candidate - po) * (candidate - po);
                    if d2 < (diameter - 1e-9) * (diameter - 1e-9) {
                        overlaps += 1;
                        collides = true;
                    }
                }
                if !collides {
                    chosen = Some(candidate);
                    break;
                }
                if overlaps < fallback.0 {
                    fallback = (overlaps, candidate);
                }
            }
            if chosen.is_some() {
                break;
            }
            lane += 1;
        }
        let offset = chosen.unwrap_or(fallback.1);
        placed.push((vx, offset));
        offsets.push((row, offset));
    }
    offsets
}

/// Wilkinson dot-plot stacks: bin width of one dot diameter aligned to multiples of the
/// width; each bin's dots stack centered on the slot with the spacing squeezed to fit.
pub(crate) fn dot_plot_offsets(
    sorted: &[(f64, usize)],
    radius: f64,
    px_per_value: f64,
    half_slot: f64,
) -> Vec<(usize, f64)> {
    if sorted.is_empty() || px_per_value <= 0.0 {
        return Vec::new();
    }
    let width = radius * 2.0 / px_per_value;
    if !width.is_finite() || width <= 0.0 {
        return sorted.iter().map(|&(_, row)| (row, 0.0)).collect();
    }
    let mut bins: std::collections::BTreeMap<i64, Vec<usize>> = std::collections::BTreeMap::new();
    for &(value, row) in sorted {
        bins.entry((value / width).floor() as i64)
            .or_default()
            .push(row);
    }
    let mut offsets = Vec::with_capacity(sorted.len());
    for rows in bins.values() {
        let count = rows.len();
        let spacing = if count > 1 {
            ((half_slot * 2.0 - radius * 2.0).max(radius * 0.5) / (count - 1) as f64)
                .min(radius * 2.0)
        } else {
            0.0
        };
        for (index, &row) in rows.iter().enumerate() {
            offsets.push((row, (index as f64 - (count - 1) as f64 / 2.0) * spacing));
        }
    }
    offsets
}

/// Matrix (parallel coordinates / scatter matrix) model: field names and extents.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MatrixModel {
    pub fields: Vec<String>,
    /// `(min, max)` per field; degenerate extents expand by +-0.5 for normalization.
    pub extents: Vec<(f64, f64)>,
    pub rows: usize,
}

impl MatrixModel {
    /// Normalized field value in 0..=1 (0.5 for degenerate extents).
    pub(crate) fn normalized(&self, field: usize, value: f64) -> f64 {
        let (min, max) = self.extents[field];
        let span = max - min;
        if span > 0.0 {
            ((value - min) / span).clamp(0.0, 1.0)
        } else {
            0.5
        }
    }
}

pub(crate) fn matrix_model(dataset: &GeneralDataset) -> Option<MatrixModel> {
    let fields: Vec<String> = dataset.matrix_fields()?.to_vec();
    let columns = dataset.matrix_columns()?;
    let rows = dataset.len();
    let extents = columns
        .iter()
        .map(|column| {
            let min = column
                .iter()
                .filter(|value| value.is_finite())
                .fold(f64::INFINITY, |a, b| a.min(*b));
            let max = column
                .iter()
                .filter(|value| value.is_finite())
                .fold(f64::NEG_INFINITY, |a, b| a.max(*b));
            if min.is_finite() && max.is_finite() {
                (min, max)
            } else {
                (0.0, 1.0)
            }
        })
        .collect();
    Some(MatrixModel {
        fields,
        extents,
        rows,
    })
}

/// Viewport-dependent cache key for pixel-space models (beeswarm, dot plot, hexbin): the
/// sample-axis domain and plot extent that the offsets were placed against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct StatsViewKey {
    pub domain: [f64; 2],
    pub extent_px: [f64; 2],
    pub radius: f64,
}

/// Cached statistics model of one series, keyed by dataset generation, options, and (for
/// pixel-space layouts) the viewport key.
#[derive(Clone)]
pub(crate) struct GeneralStatsCacheEntry {
    pub dataset: crate::GeneralDatasetId,
    pub generation: u64,
    pub options: GeneralStatsOptions,
    pub view: Option<StatsViewKey>,
    pub model: GeneralStatsModel,
}

/// Per-kind statistics models.
#[derive(Clone)]
pub(crate) enum GeneralStatsModel {
    Violin(Arc<ViolinModel>),
    Density(Arc<DensityModel>),
    Ridgeline(Arc<RidgelineModel>),
    Ecdf(Arc<EcdfModel>),
    Qq(Arc<QqModel>),
    Hexbin(Arc<HexbinModel>),
    Contour(Arc<ContourModel>),
    /// Deterministic per-row offsets in plot pixels (beeswarm or dot plot).
    Offsets(Arc<Vec<(usize, f64)>>),
    Matrix(Arc<MatrixModel>),
}

fn resolved_grid(grid: usize) -> usize {
    if grid == 0 {
        DEFAULT_STATS_GRID
    } else {
        grid.clamp(MIN_STATS_GRID, MAX_STATS_GRID)
    }
}

/// Validate the stats options of one series kind, rejecting out-of-domain values.
pub(crate) fn validate_stats_options(options: &GeneralSeriesOptions) -> Result<(), ChartError> {
    let stats = &options.stats;
    if stats.grid != 0 && !(MIN_STATS_GRID..=MAX_STATS_GRID).contains(&stats.grid) {
        return Err(invalid_options(format!(
            "stats grid must be 0 (default) or {MIN_STATS_GRID}..={MAX_STATS_GRID}"
        )));
    }
    if !stats.jitter.is_finite() || !(0.0..=1.0).contains(&stats.jitter) {
        return Err(invalid_options(
            "strip jitter must be a finite fraction 0..=1",
        ));
    }
    if !stats.overlap.is_finite() || !(0.25..=8.0).contains(&stats.overlap) {
        return Err(invalid_options(
            "ridgeline overlap must be finite within 0.25..=8",
        ));
    }
    if let GeneralStatsBandwidth::Explicit(value) = stats.bandwidth
        && (!value.is_finite() || value <= 0.0)
    {
        return Err(invalid_options(
            "an explicit stats bandwidth must be finite and positive",
        ));
    }
    if stats.hexbin_radius != 0.0
        && (!stats.hexbin_radius.is_finite()
            || !(MIN_HEXBIN_RADIUS..=MAX_HEXBIN_RADIUS).contains(&stats.hexbin_radius))
    {
        return Err(invalid_options(format!(
            "hexbin radius must be 0 (auto) or {MIN_HEXBIN_RADIUS}..={MAX_HEXBIN_RADIUS} px"
        )));
    }
    if stats.thresholds > MAX_CONTOUR_THRESHOLDS {
        return Err(invalid_options(format!(
            "contour thresholds must be at most {MAX_CONTOUR_THRESHOLDS}"
        )));
    }
    Ok(())
}

/// Validate a stats dataset against its kind's data contract.
pub(crate) fn validate_stats_dataset(
    options: &GeneralSeriesOptions,
    dataset: &GeneralDataset,
) -> Result<(), ChartError> {
    match options.kind {
        GeneralSeriesKind::Violin
        | GeneralSeriesKind::Ridgeline
        | GeneralSeriesKind::StripPlot
        | GeneralSeriesKind::Beeswarm
        | GeneralSeriesKind::DotPlot => {
            let Some(categories) = dataset.categories() else {
                return Err(invalid_data(
                    "statistics category series require category data",
                ));
            };
            if matches!(
                options.kind,
                GeneralSeriesKind::Violin | GeneralSeriesKind::Ridgeline
            ) && categories.len() > MAX_STATS_CATEGORIES
            {
                return Err(resource(format!(
                    "violin/ridgeline support at most {MAX_STATS_CATEGORIES} categories"
                )));
            }
        }
        GeneralSeriesKind::DensityCurve | GeneralSeriesKind::Ecdf | GeneralSeriesKind::QqPlot => {
            if dataset.x_kind() != crate::GeneralXKind::Values {
                return Err(invalid_data(
                    "density/ecdf/qq series require sample values data",
                ));
            }
        }
        GeneralSeriesKind::Hexbin | GeneralSeriesKind::Contour => {
            if dataset.x_kind() != crate::GeneralXKind::Numeric {
                return Err(invalid_data("hexbin/contour require numeric X/Y data"));
            }
        }
        GeneralSeriesKind::ParallelCoordinates | GeneralSeriesKind::ScatterMatrix => {
            let Some(fields) = dataset.matrix_fields() else {
                return Err(invalid_data(
                    "parallel_coordinates/scatter_matrix require matrix data",
                ));
            };
            if !(MIN_MATRIX_FIELDS..=MAX_MATRIX_FIELDS).contains(&fields.len()) {
                return Err(resource(format!(
                    "matrix datasets need {MIN_MATRIX_FIELDS}..={MAX_MATRIX_FIELDS} fields"
                )));
            }
            if dataset.len() > MAX_MATRIX_ROWS {
                return Err(resource(format!(
                    "matrix datasets support at most {MAX_MATRIX_ROWS} rows"
                )));
            }
        }
        _ => {}
    }
    if matches!(
        options.kind,
        GeneralSeriesKind::Violin
            | GeneralSeriesKind::DensityCurve
            | GeneralSeriesKind::Ridgeline
            | GeneralSeriesKind::StripPlot
            | GeneralSeriesKind::Beeswarm
            | GeneralSeriesKind::DotPlot
            | GeneralSeriesKind::Ecdf
            | GeneralSeriesKind::QqPlot
            | GeneralSeriesKind::Hexbin
            | GeneralSeriesKind::Contour
    ) && dataset.len() > MAX_STATS_ROWS
    {
        return Err(resource(format!(
            "statistics series support at most {MAX_STATS_ROWS} rows"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantile_type7_interpolates_like_d3() {
        let sorted = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(quantile_type7(&sorted, 0.0), Some(1.0));
        assert_eq!(quantile_type7(&sorted, 0.25), Some(1.75));
        assert_eq!(quantile_type7(&sorted, 0.5), Some(2.5));
        assert_eq!(quantile_type7(&sorted, 0.75), Some(3.25));
        assert_eq!(quantile_type7(&sorted, 1.0), Some(4.0));
        assert_eq!(quantile_type7(&[], 0.5), None);
        assert_eq!(quantile_type7(&sorted, 2.0), Some(4.0));
    }

    #[test]
    fn ecdf_groups_ties() {
        let points = ecdf_points(&[1.0, 2.0, 2.0, 3.0]);
        assert_eq!(points, vec![(1.0, 0.25), (2.0, 0.75), (3.0, 1.0)]);
        assert!(ecdf_points(&[]).is_empty());
    }

    #[test]
    fn inverse_normal_matches_published_values() {
        assert_eq!(inverse_normal_cdf(0.5), 0.0);
        assert!((inverse_normal_cdf(0.975) - 1.959_963_986_120_195).abs() < 1e-9);
        // Acklam's tail-region error peaks just above 1e-9 near p = 0.001 / 0.999.
        assert!((inverse_normal_cdf(0.001) + 3.090_232_306_167_813).abs() < 1e-8);
        assert!((inverse_normal_cdf(0.999) - 3.090_232_306_167_813).abs() < 1e-8);
        assert_eq!(inverse_normal_cdf(0.0), f64::NEG_INFINITY);
        assert_eq!(inverse_normal_cdf(1.0), f64::INFINITY);
        assert!(inverse_normal_cdf(f64::NAN).is_nan());
    }

    #[test]
    fn bandwidth_rules_match_reference_formulas() {
        let sorted = [1.0, 2.0, 3.0, 4.0];
        let (_, stddev) = mean_stddev(&sorted);
        let scott = resolve_bandwidth(&sorted, GeneralStatsBandwidth::Scott).unwrap();
        assert!((scott - stddev * 4.0_f64.powf(-0.2)).abs() < 1e-12);
        let iqr = quantile_type7(&sorted, 0.75).unwrap() - quantile_type7(&sorted, 0.25).unwrap();
        let silverman = resolve_bandwidth(&sorted, GeneralStatsBandwidth::Silverman).unwrap();
        assert!((silverman - 0.9 * stddev.min(iqr / 1.34) * 4.0_f64.powf(-0.2)).abs() < 1e-12);
        // Zero variance falls back to 1.0.
        assert_eq!(
            resolve_bandwidth(&[2.0, 2.0, 2.0], GeneralStatsBandwidth::Scott).unwrap(),
            1.0
        );
        assert_eq!(
            resolve_bandwidth(&[], GeneralStatsBandwidth::Silverman).unwrap(),
            1.0
        );
        assert_eq!(
            resolve_bandwidth(&sorted, GeneralStatsBandwidth::Explicit(2.5)).unwrap(),
            2.5
        );
    }

    #[test]
    fn kde_curve_matches_hand_computed_gaussian() {
        let sorted = [-1.0, 1.0];
        let curve = kde_curve(
            &sorted,
            GeneralStatsKernel::Gaussian,
            GeneralStatsBandwidth::Explicit(1.0),
            5,
        )
        .unwrap()
        .unwrap();
        assert_eq!(curve.grid, vec![-1.0, -0.5, 0.0, 0.5, 1.0]);
        let k = |u: f64| (-0.5 * u * u).exp() / (2.0 * std::f64::consts::PI).sqrt();
        // density(x) = (K(x + 1) + K(x - 1)) / (2 * 1)
        assert!((curve.density[2] - k(1.0)).abs() < 1e-12);
        assert!((curve.density[0] - (k(0.0) + k(2.0)) / 2.0).abs() < 1e-12);
        assert!((curve.density[4] - (k(2.0) + k(0.0)) / 2.0).abs() < 1e-12);
    }

    #[test]
    fn kde_curve_epanechnikov_support() {
        let sorted = [0.0];
        let curve = kde_curve(
            &sorted,
            GeneralStatsKernel::Epanechnikov,
            GeneralStatsBandwidth::Explicit(1.0),
            5,
        )
        .unwrap()
        .unwrap();
        // Degenerate range expands to [-0.5, 0.5]; K(u) = 0.75 (1 - u^2) for |u| <= 1.
        assert_eq!(curve.grid, vec![-0.5, -0.25, 0.0, 0.25, 0.5]);
        assert!((curve.density[2] - 0.75).abs() < 1e-12);
        assert!((curve.density[1] - 0.75 * (1.0 - 0.0625)).abs() < 1e-12);
    }

    #[test]
    fn kde_downsamples_deterministically() {
        let sorted: Vec<f64> = (0..10_000).map(|i| i as f64).collect();
        let curve = kde_curve(
            &sorted,
            GeneralStatsKernel::Gaussian,
            GeneralStatsBandwidth::Explicit(10.0),
            16,
        )
        .unwrap()
        .unwrap();
        assert_eq!(curve.samples, 10_000);
        // Stride 3 over the sorted order: 0, 3, 6, ... deterministic and bounded.
        let repeat = kde_curve(
            &sorted,
            GeneralStatsKernel::Gaussian,
            GeneralStatsBandwidth::Explicit(10.0),
            16,
        )
        .unwrap()
        .unwrap();
        assert_eq!(curve.density, repeat.density);
    }

    #[test]
    fn hexbin_assignment_matches_d3_hexbin() {
        let radius = 10.0;
        let dx = radius * 3.0_f64.sqrt();
        let dy = radius * 1.5;
        // A point at (0, 0) bins at cell (0, 0).
        let model = hexbin_model(&[(0.0, 0.0, 7)], radius);
        assert_eq!(model.bins.len(), 1);
        assert_eq!(
            (model.bins[0].cx, model.bins[0].cy, model.bins[0].row),
            (0.0, 0.0, 7)
        );
        // Points within the same hexagon merge; a point past the rounding boundary moves.
        let model = hexbin_model(
            &[
                (0.0, 0.0, 0),
                (dx * 0.4, 0.0, 1),
                (dx * 0.6, 0.0, 2),
                (0.0, dy, 3),
            ],
            radius,
        );
        assert_eq!(model.bins.len(), 3);
        assert_eq!(model.bins[0].count, 2);
        assert_eq!(model.max_count, 2);
        assert!((model.bins[1].cx - dx).abs() < 1e-9 && model.bins[1].cy == 0.0);
        // Odd rows offset by dx / 2.
        assert!((model.bins[2].cx - dx / 2.0).abs() < 1e-9);
        assert!((model.bins[2].cy - dy).abs() < 1e-9);
    }

    #[test]
    fn hexbin_contains_and_polygon() {
        let radius = 10.0;
        let half = radius * 3.0_f64.sqrt() / 2.0;
        assert!(hexbin_contains(radius, 0.0, 0.0));
        assert!(hexbin_contains(radius, half, 0.0));
        assert!(!hexbin_contains(radius, half + 0.1, 0.0));
        assert!(hexbin_contains(radius, 0.0, radius));
        assert!(!hexbin_contains(radius, 0.0, radius + 0.1));
        assert!(!hexbin_contains(radius, half * 0.9, radius * 0.9));
        let polygon = hexbin_polygon(5.0, 7.0, radius);
        assert_eq!(polygon.len(), 6);
        assert!((polygon[1][1] - (7.0 - radius)).abs() < 1e-9);
    }

    #[test]
    fn bin2d_counts_half_open_cells() {
        let points = [(0.0, 0.0), (0.5, 0.5), (1.0, 1.0), (1.0, 0.0)];
        let cells = bin2d(&points, 2, 2, (0.0, 1.0), (0.0, 1.0));
        // Half-open cells with the last column/row closed on the right: (0.5, 0.5) and
        // (1, 1) land in the bottom-right cell, (1, 0) in the bottom-left.
        assert_eq!(cells, vec![1.0, 1.0, 0.0, 2.0]);
    }

    #[test]
    fn marching_squares_single_cell_line_and_band() {
        // 2x2 nodes: bottom row above the level; one horizontal iso segment at y = 0.5.
        let values = [0.0, 0.0, 1.0, 1.0];
        let (bands, lines) = marching_squares(&values, 2, 2, 0.0, 0.0, 1.0, 1.0, &[0.5]);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].paths.len(), 1);
        let path = &lines[0].paths[0];
        assert_eq!(path.len(), 2);
        let mut endpoints = path.clone();
        endpoints.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());
        assert_eq!(endpoints, vec![[0.0, 0.5], [1.0, 0.5]]);
        // Band 0 is the bottom half quad: two triangles, six points.
        assert_eq!(bands[0].triangles.len(), 6);
    }

    #[test]
    fn marching_squares_closed_loop() {
        // A single hot center node produces one closed diamond loop.
        let mut values = vec![0.0; 9];
        values[4] = 1.0;
        let (bands, lines) = marching_squares(&values, 3, 3, 0.0, 0.0, 1.0, 1.0, &[0.5]);
        assert_eq!(lines[0].paths.len(), 1);
        let path = &lines[0].paths[0];
        assert_eq!(path.first(), path.last());
        assert_eq!(path.len(), 5);
        assert_eq!(bands[0].triangles.len(), 4 * 3);
    }

    #[test]
    fn marching_squares_saddle_disconnects() {
        // Diagonal saddle with mean below the level: two separate corner triangles.
        let values = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let (bands, lines) = marching_squares(&values, 3, 3, 0.0, 0.0, 1.0, 1.0, &[0.5]);
        assert_eq!(lines[0].paths.len(), 2);
        assert_eq!(bands[0].triangles.len(), 2 * 3);
    }

    #[test]
    fn strip_offsets_are_golden_ratio_steps() {
        let offsets = strip_offsets(&[10, 11, 12], 4.0);
        assert_eq!(offsets.len(), 3);
        let first = (GOLDEN_FRACT * 2.0 - 1.0) * 4.0;
        assert!((offsets[0].1 - first).abs() < 1e-12);
        assert_eq!(offsets[0].0, 10);
        // Distinct deterministic offsets.
        assert!(offsets.windows(2).all(|pair| pair[0].1 != pair[1].1));
    }

    #[test]
    fn beeswarm_places_hexagonal_lanes() {
        let radius = 1.0;
        let pitch = 3.0_f64.sqrt();
        let sorted = vec![(0.0, 0usize), (0.0, 1), (0.0, 2), (0.0, 3)];
        let offsets = beeswarm_offsets(&sorted, radius, 1.0, 10.0);
        let mut values: Vec<f64> = offsets.iter().map(|(_, offset)| *offset).collect();
        values.sort_by(f64::total_cmp);
        // Points at the same value need 2r of perpendicular clearance, so the pitch (r * sqrt(3))
        // lane collides and placement lands on every second lane.
        assert_eq!(values, vec![-2.0 * pitch, 0.0, 2.0 * pitch, 4.0 * pitch]);
        // Rows keep their identity.
        assert_eq!(offsets[0], (0, 0.0));
        assert_eq!(offsets[1], (1, 2.0 * pitch));
        assert_eq!(offsets[2], (2, -2.0 * pitch));
    }

    #[test]
    fn dot_plot_stacks_within_bins() {
        let sorted = vec![(0.0, 0usize), (0.1, 1), (5.0, 2)];
        let offsets = dot_plot_offsets(&sorted, 1.0, 1.0, 10.0);
        // Width 2: rows 0/1 share bin 0 (offsets -1/+1), row 2 alone in bin 2.
        let mut by_row: Vec<(usize, f64)> = offsets.clone();
        by_row.sort_by(|a, b| a.0.cmp(&b.0).then(f64::total_cmp(&a.1, &b.1)));
        assert_eq!(by_row, vec![(0, -1.0), (1, 1.0), (2, 0.0)]);
    }

    #[test]
    fn contour_model_rejects_empty() {
        // Handled at the model level through datasets; here verify thresholds helper bounds.
        assert!(contour_thresholds(0.0, 8).is_empty());
        assert_eq!(contour_thresholds(2.0, 2), vec![2.0 / 3.0, 4.0 / 3.0]);
        assert_eq!(contour_thresholds(1.0, 100).len(), MAX_CONTOUR_THRESHOLDS);
    }

    #[test]
    fn kde2d_rasterizes_kernel_windows() {
        let points = [(0.0, 0.0)];
        let values = kde2d(
            &points,
            GeneralStatsKernel::Gaussian,
            1.0,
            1.0,
            3,
            3,
            (-1.0, 1.0),
            (-1.0, 1.0),
        );
        // Center node carries K(0)^2 / (1 * 1 * 1).
        let k0 = 1.0 / (2.0 * std::f64::consts::PI).sqrt();
        assert!((values[4] - k0 * k0).abs() < 1e-12);
        // Corners are within the 3-bandwidth window and equal by symmetry.
        assert!((values[0] - values[2]).abs() < 1e-12);
        assert!(values.iter().all(|value| value.is_finite()));
    }
}
