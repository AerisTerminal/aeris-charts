//! Complete-build statistics geometry: cached model resolution and CSS-space visitors shared
//! by frame emission, hit testing, tooltips, and accessibility.
//!
//! Every visitor derives from the bounded models in [`crate::general_stats`]. Violins and
//! ridgelines draw KDE silhouettes inside category slots, density/ECDF/Q-Q draw value-grid
//! marks on numeric axes, hexbin re-bins in plot pixels per viewport (d3-hexbin semantics),
//! contours paint marching-squares bands plus iso-lines, and the matrix families lay out
//! normalized fields inside the pane. Pixel-space layouts (hexbin, beeswarm, dot plot, strip)
//! re-key their cached model on the viewport, so pan/zoom/resize re-bin deterministically.

use std::collections::HashMap;
use std::sync::Arc;

use aeris_charts_core::scale::general_scale::BandScale;
use aeris_charts_render::color::Color;

use crate::general_axes::NumericAxisScale;
use crate::general_data::GeneralDataset;
use crate::general_series::{
    GeneralAccessibilityItem, GeneralSeries, GeneralSeriesHit, GeneralTooltipSnapshot,
};
use crate::general_stats::{
    self, GeneralStatsCacheEntry, GeneralStatsModel, KdeCurve, StatsViewKey,
};
use crate::{
    AxisDimension, ChartEngine, GeneralAxisDomain, GeneralRowIdentity, GeneralScaleType,
    GeneralSeriesKind,
};

/// Synthetic row identities of statistics marks are tagged so they never collide with the
/// small generated identities of real dataset rows.
const STATS_IDENTITY_TAG: u64 = 0x5A7A_0000_0000_0000;

/// A synthetic statistics row identity (`kind` distinguishes families sharing a dataset).
pub(crate) fn stats_row_identity(kind: GeneralSeriesKind, index: usize) -> GeneralRowIdentity {
    GeneralRowIdentity::Generated(
        STATS_IDENTITY_TAG | (((kind as u64) & 0xFF) << 40) | (index as u64 & 0xFF_FFFF_FFFF),
    )
}

/// Distance from a point to a segment.
fn segment_distance(x: f64, y: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> f64 {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let length_squared = dx * dx + dy * dy;
    if !length_squared.is_finite() || length_squared <= f64::EPSILON {
        return (x - x0).hypot(y - y0);
    }
    let t = (((x - x0) * dx + (y - y0) * dy) / length_squared).clamp(0.0, 1.0);
    (x - (x0 + t * dx)).hypot(y - (y0 + t * dy))
}

/// Density of a KDE curve at `value`, linearly interpolated between grid points (0 outside).
pub(crate) fn kde_density_at(curve: &KdeCurve, value: f64) -> f64 {
    let grid = &curve.grid;
    if grid.len() < 2 || value < grid[0] || value > grid[grid.len() - 1] {
        return 0.0;
    }
    let step = (grid[grid.len() - 1] - grid[0]) / (grid.len() - 1) as f64;
    if step <= 0.0 {
        return curve.density[0];
    }
    let position = ((value - grid[0]) / step).clamp(0.0, (grid.len() - 1) as f64);
    let lower = (position.floor() as usize).min(grid.len() - 2);
    let fraction = position - lower as f64;
    curve.density[lower] + fraction * (curve.density[lower + 1] - curve.density[lower])
}

/// One violin silhouette in CSS space.
pub(crate) struct GeneralViolinGeometry<'a> {
    /// Synthetic row: the category position within the model.
    pub position: usize,
    pub category: &'a general_stats::ViolinCategoryModel,
    pub max_density: f64,
    pub slot_left: f64,
    pub slot_right: f64,
    pub center_x: f64,
    pub y_scale: NumericAxisScale,
}

impl GeneralViolinGeometry<'_> {
    pub(crate) fn y(&self, value: f64) -> Option<f64> {
        self.y_scale.coordinate(value)
    }

    /// Half-width of the silhouette at a density, in CSS px.
    pub(crate) fn half_width(&self, density: f64) -> f64 {
        let half = (self.slot_right - self.slot_left) * 0.5;
        (density / self.max_density).clamp(0.0, 1.0) * half
    }

    /// Half-width of the silhouette at a sample value (interpolated over the KDE grid).
    pub(crate) fn half_width_at(&self, value: f64) -> f64 {
        self.half_width(kde_density_at(&self.category.curve, value))
    }
}

/// A density curve projected to CSS space.
pub(crate) struct GeneralDensityGeometry {
    pub points: Vec<(f64, f64)>,
    pub baseline_y: f64,
}

/// One ridgeline band in CSS space.
pub(crate) struct GeneralRidgelineBand {
    /// Synthetic row: the category position within the model.
    pub position: usize,
    pub base_y: f64,
    pub height: f64,
}

/// Ridgeline layout: per-category bands plus the shared value scale.
pub(crate) struct GeneralRidgelineGeometry {
    pub model: Arc<general_stats::RidgelineModel>,
    pub bands: Vec<GeneralRidgelineBand>,
    pub x_scale: NumericAxisScale,
}

impl GeneralRidgelineGeometry {
    /// Curve point of `band` at grid index `index`.
    pub(crate) fn point(&self, band: &GeneralRidgelineBand, index: usize) -> Option<(f64, f64)> {
        let category = &self.model.categories[band.position];
        let curve = &category.curve;
        let x = self.x_scale.coordinate(*curve.grid.get(index)?)?;
        let share = if self.model.max_density > 0.0 {
            (curve.density[index] / self.model.max_density).clamp(0.0, 1.0)
        } else {
            0.0
        };
        Some((x, band.base_y - share * band.height))
    }
}

/// One offset point of a strip/beeswarm/dot-plot series.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GeneralStatsPointGeometry {
    pub row: usize,
    pub x: f64,
    pub y: f64,
    pub radius: f64,
}

/// One hexbin cell in CSS space.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GeneralHexbinGeometry {
    pub index: usize,
    pub x: f64,
    pub y: f64,
    pub radius: f64,
    pub share: f64,
}

/// Parallel-coordinates layout: axis slots plus one polyline per dataset row (CSS space).
pub(crate) struct GeneralParallelGeometry {
    pub model: Arc<general_stats::MatrixModel>,
    pub slots: Vec<f64>,
    pub top: f64,
    pub height: f64,
    pub rows: Vec<(usize, Vec<(f64, f64)>)>,
}

/// One scatter-matrix pane cell (CSS space).
#[derive(Clone, Copy, Debug)]
pub(crate) struct GeneralMatrixCell {
    pub x_field: usize,
    pub y_field: usize,
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// Scatter-matrix layout: the cell grid plus the row stride that bounds painted circles.
pub(crate) struct GeneralScatterMatrixGeometry {
    pub model: Arc<general_stats::MatrixModel>,
    pub cells: Vec<GeneralMatrixCell>,
    pub rows: usize,
    pub stride: usize,
}

impl GeneralScatterMatrixGeometry {
    /// One row's point inside `cell` (CSS space).
    pub(crate) fn point(
        &self,
        dataset: &GeneralDataset,
        cell: &GeneralMatrixCell,
        row: usize,
    ) -> Option<(f64, f64)> {
        let x = self
            .model
            .normalized(cell.x_field, dataset.matrix_value(cell.x_field, row)?);
        let y = self
            .model
            .normalized(cell.y_field, dataset.matrix_value(cell.y_field, row)?);
        Some((
            cell.left + x * cell.width,
            cell.top + (1.0 - y) * cell.height,
        ))
    }
}

/// A contour model plus the scales that project its data-space geometry to CSS space.
pub(crate) struct GeneralContourGeometry {
    pub model: Arc<general_stats::ContourModel>,
    pub x_scale: NumericAxisScale,
    pub y_scale: NumericAxisScale,
}

impl GeneralContourGeometry {
    pub(crate) fn project(&self, point: [f64; 2]) -> Option<(f64, f64)> {
        Some((
            self.x_scale.coordinate(point[0])?,
            self.y_scale.coordinate(point[1])?,
        ))
    }
}

impl ChartEngine {
    /// The cached statistics model of one series, recomputed when its dataset generation,
    /// options, or viewport key changed.
    pub(crate) fn general_stats_model(&self, series: &GeneralSeries) -> Option<GeneralStatsModel> {
        if !series.kind().requires_complete_build() {
            return None;
        }
        let registry = self.general_series.as_ref()?;
        let dataset = self.general_geometry_dataset(series.dataset())?;
        let view = self.general_stats_view_key(series)?;
        let options = *series.stats();
        let current = |entry: &&GeneralStatsCacheEntry| {
            entry.dataset == series.dataset()
                && entry.generation == dataset.generation()
                && entry.options == options
                && entry.view == view
        };
        if let Some(entry) = registry
            .stats_models
            .borrow()
            .get(&series.id())
            .filter(current)
        {
            return Some(entry.model.clone());
        }
        let model = self
            .compute_general_stats_model(series, dataset, view)
            .ok()??;
        registry.stats_models.borrow_mut().insert(
            series.id(),
            GeneralStatsCacheEntry {
                dataset: series.dataset(),
                generation: dataset.generation(),
                options,
                view,
                model: model.clone(),
            },
        );
        Some(model)
    }

    /// Viewport key of pixel-space layouts; `Some(None)` for value-space models.
    fn general_stats_view_key(&self, series: &GeneralSeries) -> Option<Option<StatsViewKey>> {
        let kind = series.kind();
        let pixel_space = matches!(
            kind,
            GeneralSeriesKind::Hexbin
                | GeneralSeriesKind::Beeswarm
                | GeneralSeriesKind::DotPlot
                | GeneralSeriesKind::StripPlot
        );
        if !pixel_space {
            return Some(None);
        }
        let pane_index = self.pane_index_for_id(series.pane_id())?;
        let plot = self.general_plot_rect(pane_index)?;
        let (axis_id, radius) = if kind == GeneralSeriesKind::Hexbin {
            (
                series.x_axis_id(),
                general_stats::resolved_hexbin_radius(series.stats(), plot.width, plot.height),
            )
        } else {
            (series.y_axis_id(), series.point_radius())
        };
        let axis = self.general_axis(axis_id)?;
        let GeneralAxisDomain::Numeric(domain) = self.effective_general_axis_domain(axis)? else {
            return None;
        };
        Some(Some(StatsViewKey {
            domain,
            extent_px: [plot.width, plot.height],
            radius,
        }))
    }

    fn compute_general_stats_model(
        &self,
        series: &GeneralSeries,
        dataset: &GeneralDataset,
        view: Option<StatsViewKey>,
    ) -> Result<Option<GeneralStatsModel>, crate::ChartError> {
        let stats = series.stats();
        match series.kind() {
            GeneralSeriesKind::Violin => Ok(general_stats::violin_model(dataset, stats)?
                .map(|model| GeneralStatsModel::Violin(Arc::new(model)))),
            GeneralSeriesKind::DensityCurve => Ok(general_stats::density_model(dataset, stats)?
                .map(|model| GeneralStatsModel::Density(Arc::new(model)))),
            GeneralSeriesKind::Ridgeline => Ok(general_stats::ridgeline_model(dataset, stats)?
                .map(|model| GeneralStatsModel::Ridgeline(Arc::new(model)))),
            GeneralSeriesKind::Ecdf => Ok(general_stats::ecdf_model(dataset)
                .map(|model| GeneralStatsModel::Ecdf(Arc::new(model)))),
            GeneralSeriesKind::QqPlot => Ok(general_stats::qq_model(dataset)
                .map(|model| GeneralStatsModel::Qq(Arc::new(model)))),
            GeneralSeriesKind::Contour => Ok(general_stats::contour_model(dataset, stats)?
                .map(|model| GeneralStatsModel::Contour(Arc::new(model)))),
            GeneralSeriesKind::ParallelCoordinates | GeneralSeriesKind::ScatterMatrix => {
                Ok(general_stats::matrix_model(dataset)
                    .map(|model| GeneralStatsModel::Matrix(Arc::new(model))))
            }
            GeneralSeriesKind::Hexbin => {
                let Some(view) = view else { return Ok(None) };
                let Some(points) = self.general_hexbin_pixel_points(series, dataset, view.radius)
                else {
                    return Ok(None);
                };
                Ok(Some(GeneralStatsModel::Hexbin(Arc::new(
                    general_stats::hexbin_model(&points, view.radius),
                ))))
            }
            GeneralSeriesKind::StripPlot => {
                let Some(view) = view else { return Ok(None) };
                let Some(half_slot) = self.general_category_slot_half(series) else {
                    return Ok(None);
                };
                let extent = (half_slot - view.radius).max(0.0) * stats.jitter;
                let mut offsets = Vec::new();
                let Some(categories) = dataset.categories() else {
                    return Ok(None);
                };
                let Some(indices) = dataset.category_indices() else {
                    return Ok(None);
                };
                for category in 0..categories.len() {
                    let rows: Vec<usize> = (0..dataset.len())
                        .filter(|&row| {
                            indices
                                .get(row)
                                .is_some_and(|&index| index as usize == category)
                                && dataset.y_is_valid(row)
                                && dataset.y()[row].is_finite()
                        })
                        .collect();
                    // Deterministic stride caps painted dots per category.
                    let stride = rows.len().div_ceil(general_stats::MAX_STATS_SAMPLES).max(1);
                    let rows: Vec<usize> = rows.into_iter().step_by(stride).collect();
                    offsets.extend(general_stats::strip_offsets(&rows, extent));
                }
                Ok(Some(GeneralStatsModel::Offsets(Arc::new(offsets))))
            }
            GeneralSeriesKind::Beeswarm | GeneralSeriesKind::DotPlot => {
                let Some(view) = view else { return Ok(None) };
                let Some(half_slot) = self.general_category_slot_half(series) else {
                    return Ok(None);
                };
                let Some(px_per_value) = self.general_stats_value_px_per_unit(series) else {
                    return Ok(None);
                };
                let mut offsets = Vec::new();
                let Some(categories) = dataset.categories() else {
                    return Ok(None);
                };
                for category in 0..categories.len() {
                    let sorted = general_stats::sorted_category_samples(dataset, category as u32);
                    if sorted.is_empty() {
                        continue;
                    }
                    let stride = sorted
                        .len()
                        .div_ceil(general_stats::MAX_STATS_SAMPLES)
                        .max(1);
                    let sorted: Vec<(f64, usize)> = sorted.into_iter().step_by(stride).collect();
                    let half = (half_slot - view.radius).max(0.0);
                    if series.kind() == GeneralSeriesKind::Beeswarm {
                        offsets.extend(general_stats::beeswarm_offsets(
                            &sorted,
                            view.radius,
                            px_per_value,
                            half,
                        ));
                    } else {
                        offsets.extend(general_stats::dot_plot_offsets(
                            &sorted,
                            view.radius,
                            px_per_value,
                            half,
                        ));
                    }
                }
                Ok(Some(GeneralStatsModel::Offsets(Arc::new(offsets))))
            }
            _ => Ok(None),
        }
    }

    /// Hexbin sample points in plot pixels (`y` measured from the plot top), clipped to the
    /// plot expanded by two radii.
    fn general_hexbin_pixel_points(
        &self,
        series: &GeneralSeries,
        dataset: &GeneralDataset,
        radius: f64,
    ) -> Option<Vec<(f64, f64, usize)>> {
        let pane_index = self.pane_index_for_id(series.pane_id())?;
        let plot = self.general_plot_rect(pane_index)?;
        let (Some(xs), Some(x_scale), Some(y_scale)) = (
            dataset.numeric_x(),
            self.general_numeric_axis_scale(series, AxisDimension::X),
            self.general_numeric_axis_scale(series, AxisDimension::Y),
        ) else {
            return None;
        };
        let margin = radius * 2.0;
        let mut points = Vec::new();
        for (row, (&x, &y)) in xs.iter().zip(dataset.y()).enumerate() {
            if !dataset.y_is_valid(row) || !x.is_finite() || !y.is_finite() {
                continue;
            }
            let (Some(px), Some(py)) = (x_scale.coordinate(x), y_scale.coordinate(y)) else {
                continue;
            };
            let py = py - plot.y;
            if px >= -margin
                && px <= plot.width + margin
                && py >= -margin
                && py <= plot.height + margin
            {
                points.push((px, py, row));
            }
        }
        Some(points)
    }

    /// Numeric scale of one bound axis (plot-local X, chart-space Y), like the bar families.
    fn general_numeric_axis_scale(
        &self,
        series: &GeneralSeries,
        dimension: AxisDimension,
    ) -> Option<NumericAxisScale> {
        let pane_index = self.pane_index_for_id(series.pane_id())?;
        let plot = self.general_plot_rect(pane_index)?;
        let axis = self.general_axis(if dimension == AxisDimension::X {
            series.x_axis_id()
        } else {
            series.y_axis_id()
        })?;
        let GeneralAxisDomain::Numeric(domain) = self.effective_general_axis_domain(axis)? else {
            return None;
        };
        let range = match dimension {
            AxisDimension::X => {
                if axis.reverse() {
                    (plot.width, 0.0)
                } else {
                    (0.0, plot.width)
                }
            }
            _ => {
                let bottom = plot.y + plot.height;
                if self.general_axis_range_reversed(axis) {
                    (plot.y, bottom)
                } else {
                    (bottom, plot.y)
                }
            }
        };
        NumericAxisScale::new(axis.scale(), domain, range.0, range.1)
    }

    /// Half the uniform category slot width of the bound band axis (CSS px).
    fn general_category_slot_half(&self, series: &GeneralSeries) -> Option<f64> {
        let pane_index = self.pane_index_for_id(series.pane_id())?;
        let plot = self.general_plot_rect(pane_index)?;
        let axis = self.general_axis(series.x_axis_id())?;
        let GeneralAxisDomain::Category(categories) = self.effective_general_axis_domain(axis)?
        else {
            return None;
        };
        let range = if axis.reverse() {
            (plot.width, 0.0)
        } else {
            (0.0, plot.width)
        };
        let scale = BandScale::new(
            categories.len(),
            range.0,
            range.1,
            axis.band_padding_inner(),
            axis.band_padding_outer(),
            0.5,
        )
        .ok()?;
        let (from, to) = scale.bounds(0)?;
        Some((to - from).abs() * 0.5)
    }

    /// Plot px per value unit on the bound numeric Y axis (direction-independent; the local
    /// derivative at the domain midpoint for non-linear scales).
    fn general_stats_value_px_per_unit(&self, series: &GeneralSeries) -> Option<f64> {
        let axis = self.general_axis(series.y_axis_id())?;
        let GeneralAxisDomain::Numeric(domain) = self.effective_general_axis_domain(axis)? else {
            return None;
        };
        let scale = self.general_numeric_axis_scale(series, AxisDimension::Y)?;
        let span = (domain[1] - domain[0]).abs();
        if !span.is_finite() || span <= 0.0 {
            return None;
        }
        let middle = (domain[0] + domain[1]) * 0.5;
        let epsilon = span * 1e-6;
        let (Some(a), Some(b)) = (
            scale.coordinate(middle - epsilon),
            scale.coordinate(middle + epsilon),
        ) else {
            return None;
        };
        let px = (b - a).abs() / (2.0 * epsilon);
        (px.is_finite() && px > 0.0).then_some(px)
    }

    /// Violin silhouettes of one series in model category order.
    pub(crate) fn visit_general_violins<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralViolinGeometry<'_>),
    {
        if !series.visible() || series.kind() != GeneralSeriesKind::Violin {
            return;
        }
        let Some(GeneralStatsModel::Violin(model)) = self.general_stats_model(series) else {
            return;
        };
        let Some(dataset) = self.general_geometry_dataset(series.dataset()) else {
            return;
        };
        let Some(categories) = dataset.categories() else {
            return;
        };
        let Some(pane_index) = self.pane_index_for_id(series.pane_id()) else {
            return;
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return;
        };
        let Some(x_axis) = self.general_axis(series.x_axis_id()) else {
            return;
        };
        let Some(GeneralAxisDomain::Category(axis_categories)) =
            self.effective_general_axis_domain(x_axis)
        else {
            return;
        };
        let Some(y_scale) = self.general_numeric_axis_scale(series, AxisDimension::Y) else {
            return;
        };
        let x_range = if x_axis.reverse() {
            (plot.width, 0.0)
        } else {
            (0.0, plot.width)
        };
        let Ok(band) = BandScale::new(
            axis_categories.len(),
            x_range.0,
            x_range.1,
            x_axis.band_padding_inner(),
            x_axis.band_padding_outer(),
            0.5,
        ) else {
            return;
        };
        let lookup: HashMap<&str, usize> = axis_categories
            .iter()
            .enumerate()
            .map(|(index, category)| (category.as_str(), index))
            .collect();
        for (position, category) in model.categories.iter().enumerate() {
            let Some(name) = categories.get(category.category_index as usize) else {
                continue;
            };
            let Some(&axis_index) = lookup.get(name.as_str()) else {
                continue;
            };
            let Some(bounds) = self
                .category_motion_bounds(x_axis, name, x_range.0, x_range.1)
                .or_else(|| band.bounds(axis_index))
            else {
                continue;
            };
            let (from, to) = self
                .category_row_motion_bounds(x_axis, series.dataset(), category.row, bounds, x_range)
                .unwrap_or(bounds);
            let full_left = from.min(to).clamp(0.0, plot.width);
            let full_right = from.max(to).clamp(0.0, plot.width);
            let inset = (full_right - full_left) * 0.1;
            let slot_left = full_left + inset;
            let slot_right = full_right - inset;
            if slot_right <= slot_left {
                continue;
            }
            visit(GeneralViolinGeometry {
                position,
                category,
                max_density: model.max_density,
                slot_left,
                slot_right,
                center_x: (slot_left + slot_right) * 0.5,
                y_scale,
            });
        }
    }

    /// The density curve of one series projected to CSS space.
    pub(crate) fn general_density_geometry(
        &self,
        series: &GeneralSeries,
    ) -> Option<GeneralDensityGeometry> {
        if !series.visible() || series.kind() != GeneralSeriesKind::DensityCurve {
            return None;
        }
        let GeneralStatsModel::Density(model) = self.general_stats_model(series)? else {
            return None;
        };
        let plot = self
            .pane_index_for_id(series.pane_id())
            .and_then(|pane_index| self.general_plot_rect(pane_index))?;
        let x_scale = self.general_numeric_axis_scale(series, AxisDimension::X)?;
        let y_scale = self.general_numeric_axis_scale(series, AxisDimension::Y)?;
        let curve = &model.curve;
        let mut points = Vec::with_capacity(curve.grid.len());
        for (&value, &density) in curve.grid.iter().zip(curve.density.iter()) {
            points.push((x_scale.coordinate(value)?, y_scale.coordinate(density)?));
        }
        let baseline_y = y_scale
            .coordinate(0.0)
            .unwrap_or(plot.y + plot.height)
            .clamp(plot.y, plot.y + plot.height);
        Some(GeneralDensityGeometry { points, baseline_y })
    }

    /// Ridgeline bands of one series with the shared value scale.
    pub(crate) fn general_ridgeline_geometry(
        &self,
        series: &GeneralSeries,
    ) -> Option<GeneralRidgelineGeometry> {
        if !series.visible() || series.kind() != GeneralSeriesKind::Ridgeline {
            return None;
        }
        let GeneralStatsModel::Ridgeline(model) = self.general_stats_model(series)? else {
            return None;
        };
        let dataset = self.general_geometry_dataset(series.dataset())?;
        let categories = dataset.categories()?;
        let pane_index = self.pane_index_for_id(series.pane_id())?;
        let plot = self.general_plot_rect(pane_index)?;
        let y_axis = self.general_axis(series.y_axis_id())?;
        let GeneralAxisDomain::Category(axis_categories) =
            self.effective_general_axis_domain(y_axis)?
        else {
            return None;
        };
        let x_scale = self.general_numeric_axis_scale(series, AxisDimension::X)?;
        let plot_bottom = plot.y + plot.height;
        let y_range = if self.general_axis_range_reversed(y_axis) {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let band = BandScale::new(
            axis_categories.len(),
            y_range.0,
            y_range.1,
            y_axis.band_padding_inner(),
            y_axis.band_padding_outer(),
            0.5,
        )
        .ok()?;
        let lookup: HashMap<&str, usize> = axis_categories
            .iter()
            .enumerate()
            .map(|(index, category)| (category.as_str(), index))
            .collect();
        let step = band.step().abs();
        let height = (step * series.stats().overlap).max(1.0);
        let mut bands = Vec::with_capacity(model.categories.len());
        for (position, category) in model.categories.iter().enumerate() {
            let Some(name) = categories.get(category.category_index as usize) else {
                continue;
            };
            let Some(&axis_index) = lookup.get(name.as_str()) else {
                continue;
            };
            let Some(bounds) = self
                .category_motion_bounds(y_axis, name, y_range.0, y_range.1)
                .or_else(|| band.bounds(axis_index))
            else {
                continue;
            };
            let (from, to) = self
                .category_row_motion_bounds(y_axis, series.dataset(), category.row, bounds, y_range)
                .unwrap_or(bounds);
            bands.push(GeneralRidgelineBand {
                position,
                base_y: from.max(to),
                height,
            });
        }
        Some(GeneralRidgelineGeometry {
            model,
            bands,
            x_scale,
        })
    }

    /// Offset points of a strip, beeswarm, or dot-plot series (real dataset rows).
    pub(crate) fn visit_general_stats_points<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralStatsPointGeometry),
    {
        let kind = series.kind();
        if !series.visible()
            || !matches!(
                kind,
                GeneralSeriesKind::StripPlot
                    | GeneralSeriesKind::Beeswarm
                    | GeneralSeriesKind::DotPlot
            )
        {
            return;
        }
        let Some(GeneralStatsModel::Offsets(offsets)) = self.general_stats_model(series) else {
            return;
        };
        let Some(dataset) = self.general_geometry_dataset(series.dataset()) else {
            return;
        };
        let (Some(categories), Some(indices)) = (dataset.categories(), dataset.category_indices())
        else {
            return;
        };
        let Some(pane_index) = self.pane_index_for_id(series.pane_id()) else {
            return;
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return;
        };
        let Some(x_axis) = self.general_axis(series.x_axis_id()) else {
            return;
        };
        let Some(GeneralAxisDomain::Category(axis_categories)) =
            self.effective_general_axis_domain(x_axis)
        else {
            return;
        };
        let Some(y_scale) = self.general_numeric_axis_scale(series, AxisDimension::Y) else {
            return;
        };
        let x_range = if x_axis.reverse() {
            (plot.width, 0.0)
        } else {
            (0.0, plot.width)
        };
        let Ok(band) = BandScale::new(
            axis_categories.len(),
            x_range.0,
            x_range.1,
            x_axis.band_padding_inner(),
            x_axis.band_padding_outer(),
            0.5,
        ) else {
            return;
        };
        let lookup: HashMap<&str, usize> = axis_categories
            .iter()
            .enumerate()
            .map(|(index, category)| (category.as_str(), index))
            .collect();
        let offset_lookup: HashMap<usize, f64> = offsets.iter().copied().collect();
        let radius = series.point_radius();
        for (row, &category_index) in indices.iter().enumerate() {
            if !dataset.y_is_valid(row) || !dataset.y()[row].is_finite() {
                continue;
            }
            let Some(category) = usize::try_from(category_index)
                .ok()
                .and_then(|index| categories.get(index))
            else {
                continue;
            };
            let Some(&axis_index) = lookup.get(category.as_str()) else {
                continue;
            };
            let Some(bounds) = self
                .category_motion_bounds(x_axis, category, x_range.0, x_range.1)
                .or_else(|| band.bounds(axis_index))
            else {
                continue;
            };
            let (from, to) = self
                .category_row_motion_bounds(x_axis, series.dataset(), row, bounds, x_range)
                .unwrap_or(bounds);
            let left = from.min(to).clamp(0.0, plot.width);
            let right = from.max(to).clamp(0.0, plot.width);
            let half = (right - left) * 0.5;
            let Some(&offset) = offset_lookup.get(&row) else {
                continue;
            };
            let Some(y) = y_scale.coordinate(dataset.y()[row]) else {
                continue;
            };
            let limit = (half - radius).max(0.0);
            visit(GeneralStatsPointGeometry {
                row,
                x: (left + half + offset.clamp(-limit, limit)).clamp(0.0, plot.width),
                y,
                radius,
            });
        }
    }

    /// The ECDF step path of one series (CSS space), plus the step count.
    pub(crate) fn general_ecdf_path(
        &self,
        series: &GeneralSeries,
    ) -> Option<(Vec<(f64, f64)>, usize)> {
        if !series.visible() || series.kind() != GeneralSeriesKind::Ecdf {
            return None;
        }
        let GeneralStatsModel::Ecdf(model) = self.general_stats_model(series)? else {
            return None;
        };
        let x_scale = self.general_numeric_axis_scale(series, AxisDimension::X)?;
        let y_scale = self.general_numeric_axis_scale(series, AxisDimension::Y)?;
        let mut path = Vec::with_capacity(model.points.len() * 2);
        for (index, &(value, f)) in model.points.iter().enumerate() {
            let (x, y) = (x_scale.coordinate(value)?, y_scale.coordinate(f)?);
            if index == 0 {
                path.push((x, y_scale.coordinate(0.0)?));
            } else {
                let previous = path[path.len() - 1];
                path.push((x, previous.1));
            }
            path.push((x, y));
        }
        Some((path, model.points.len()))
    }

    /// Q-Q points `(row, x, y)` and the quartile reference segment, in CSS space.
    #[allow(clippy::type_complexity)]
    pub(crate) fn general_qq_geometry(
        &self,
        series: &GeneralSeries,
    ) -> Option<(Vec<(usize, f64, f64)>, Option<[(f64, f64); 2]>)> {
        if !series.visible() || series.kind() != GeneralSeriesKind::QqPlot {
            return None;
        }
        let GeneralStatsModel::Qq(model) = self.general_stats_model(series)? else {
            return None;
        };
        let x_scale = self.general_numeric_axis_scale(series, AxisDimension::X)?;
        let y_scale = self.general_numeric_axis_scale(series, AxisDimension::Y)?;
        let mut points = Vec::with_capacity(model.points.len());
        for (row, &(theoretical, sample, _)) in model.points.iter().enumerate() {
            points.push((
                row,
                x_scale.coordinate(theoretical)?,
                y_scale.coordinate(sample)?,
            ));
        }
        let line = model.line.and_then(|(x0, y0, x1, y1)| {
            Some([
                (x_scale.coordinate(x0)?, y_scale.coordinate(y0)?),
                (x_scale.coordinate(x1)?, y_scale.coordinate(y1)?),
            ])
        });
        Some((points, line))
    }

    /// Hexbin cells of one series in CSS space.
    pub(crate) fn visit_general_hexbins<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralHexbinGeometry),
    {
        if !series.visible() || series.kind() != GeneralSeriesKind::Hexbin {
            return;
        }
        let Some(GeneralStatsModel::Hexbin(model)) = self.general_stats_model(series) else {
            return;
        };
        let Some(pane_index) = self.pane_index_for_id(series.pane_id()) else {
            return;
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return;
        };
        let max_count = model.max_count.max(1) as f64;
        for (index, bin) in model.bins.iter().enumerate() {
            visit(GeneralHexbinGeometry {
                index,
                x: bin.cx,
                y: plot.y + bin.cy,
                radius: model.radius,
                share: bin.count as f64 / max_count,
            });
        }
    }

    /// The contour model of one series plus its projection scales.
    pub(crate) fn general_contour_geometry(
        &self,
        series: &GeneralSeries,
    ) -> Option<GeneralContourGeometry> {
        if !series.visible() || series.kind() != GeneralSeriesKind::Contour {
            return None;
        }
        let GeneralStatsModel::Contour(model) = self.general_stats_model(series)? else {
            return None;
        };
        Some(GeneralContourGeometry {
            model,
            x_scale: self.general_numeric_axis_scale(series, AxisDimension::X)?,
            y_scale: self.general_numeric_axis_scale(series, AxisDimension::Y)?,
        })
    }

    /// Parallel-coordinates layout of one series.
    pub(crate) fn general_parallel_geometry(
        &self,
        series: &GeneralSeries,
    ) -> Option<GeneralParallelGeometry> {
        if !series.visible() || series.kind() != GeneralSeriesKind::ParallelCoordinates {
            return None;
        }
        let GeneralStatsModel::Matrix(model) = self.general_stats_model(series)? else {
            return None;
        };
        let dataset = self.general_geometry_dataset(series.dataset())?;
        let pane_index = self.pane_index_for_id(series.pane_id())?;
        let plot = self.general_plot_rect(pane_index)?;
        let fields = model.fields.len();
        if fields < 2 || plot.width <= 0.0 || plot.height <= 0.0 {
            return None;
        }
        let pad = (plot.width * 0.05).max(16.0);
        let span = (plot.width - pad * 2.0).max(1.0);
        let slots: Vec<f64> = (0..fields)
            .map(|field| pad + field as f64 * span / (fields - 1) as f64)
            .collect();
        let top = plot.y + 4.0;
        let height = (plot.height - 8.0).max(1.0);
        let mut rows = Vec::with_capacity(dataset.len());
        for row in 0..dataset.len() {
            let mut points = Vec::with_capacity(fields);
            for (field, &slot_x) in slots.iter().enumerate() {
                let value = dataset.matrix_value(field, row)?;
                let share = model.normalized(field, value);
                points.push((slot_x, top + (1.0 - share) * height));
            }
            rows.push((row, points));
        }
        Some(GeneralParallelGeometry {
            model,
            slots,
            top,
            height,
            rows,
        })
    }

    /// Scatter-matrix layout of one series.
    pub(crate) fn general_scatter_matrix_geometry(
        &self,
        series: &GeneralSeries,
    ) -> Option<GeneralScatterMatrixGeometry> {
        if !series.visible() || series.kind() != GeneralSeriesKind::ScatterMatrix {
            return None;
        }
        let GeneralStatsModel::Matrix(model) = self.general_stats_model(series)? else {
            return None;
        };
        let dataset = self.general_geometry_dataset(series.dataset())?;
        let pane_index = self.pane_index_for_id(series.pane_id())?;
        let plot = self.general_plot_rect(pane_index)?;
        let fields = model.fields.len();
        if fields < 2 || plot.width <= 0.0 || plot.height <= 0.0 {
            return None;
        }
        const GAP: f64 = 6.0;
        let cell_width = (plot.width - GAP * (fields - 1) as f64) / fields as f64;
        let cell_height = (plot.height - GAP * (fields - 1) as f64) / fields as f64;
        if cell_width <= 0.0 || cell_height <= 0.0 {
            return None;
        }
        let mut cells = Vec::with_capacity(fields * fields);
        for y_field in 0..fields {
            for x_field in 0..fields {
                cells.push(GeneralMatrixCell {
                    x_field,
                    y_field,
                    left: x_field as f64 * (cell_width + GAP),
                    // Fields ascend bottom-to-top like d3 SPLOM.
                    top: plot.y + (fields - 1 - y_field) as f64 * (cell_height + GAP),
                    width: cell_width,
                    height: cell_height,
                });
            }
        }
        let rows = dataset.len();
        let stride = (rows * cells.len()).div_ceil(32_768).max(1);
        Some(GeneralScatterMatrixGeometry {
            model,
            cells,
            rows,
            stride,
        })
    }

    /// The number of synthetic rows of a statistics series (tooltip/a11y/reconcile), or
    /// `None` for families whose marks are real dataset rows.
    pub(crate) fn general_stats_row_count(&self, series: &GeneralSeries) -> Option<usize> {
        if !series.kind().requires_complete_build() {
            return None;
        }
        match self.general_stats_model(series)? {
            GeneralStatsModel::Violin(model) => Some(model.categories.len()),
            GeneralStatsModel::Density(model) => Some(model.curve.grid.len()),
            GeneralStatsModel::Ridgeline(model) => Some(model.categories.len()),
            GeneralStatsModel::Ecdf(model) => Some(model.points.len()),
            GeneralStatsModel::Qq(model) => Some(model.points.len()),
            GeneralStatsModel::Hexbin(model) => Some(model.bins.len()),
            GeneralStatsModel::Contour(model) => Some(model.lines.len()),
            GeneralStatsModel::Offsets(_) | GeneralStatsModel::Matrix(_) => None,
        }
    }

    /// Numeric auto-domain bounds contributed by a statistics series on one axis.
    pub(crate) fn general_stats_axis_bounds(
        &self,
        series: &GeneralSeries,
        dimension: AxisDimension,
        scale: GeneralScaleType,
    ) -> Option<(Option<(f64, f64)>, bool)> {
        let kind = series.kind();
        if !kind.requires_complete_build() {
            return None;
        }
        let dataset = self.general_geometry_dataset(series.dataset())?;
        let log = scale == GeneralScaleType::Logarithmic;
        let scan = |values: &[f64], valid: &dyn Fn(usize) -> bool| {
            let mut bounds: Option<(f64, f64)> = None;
            for (row, &value) in values.iter().enumerate() {
                if !valid(row) || !value.is_finite() || (log && value <= 0.0) {
                    continue;
                }
                crate::general_axes::extend_numeric_bounds(&mut bounds, value);
            }
            bounds
        };
        let sample_bounds = scan(dataset.y(), &|row| dataset.y_is_valid(row));
        let result = match (kind, dimension) {
            (
                GeneralSeriesKind::Violin
                | GeneralSeriesKind::StripPlot
                | GeneralSeriesKind::Beeswarm
                | GeneralSeriesKind::DotPlot,
                AxisDimension::Y,
            )
            | (GeneralSeriesKind::Ridgeline | GeneralSeriesKind::Ecdf, AxisDimension::X)
            | (GeneralSeriesKind::QqPlot, AxisDimension::Y)
            | (GeneralSeriesKind::Hexbin | GeneralSeriesKind::Contour, AxisDimension::Y) => {
                (sample_bounds, false)
            }
            (GeneralSeriesKind::Hexbin | GeneralSeriesKind::Contour, AxisDimension::X) => {
                (scan(dataset.numeric_x()?, &|_| true), false)
            }
            (GeneralSeriesKind::DensityCurve, AxisDimension::X) => {
                let mut bounds = sample_bounds;
                if let GeneralStatsModel::Density(model) = self.general_stats_model(series)? {
                    bounds = Some((model.curve.min, model.curve.max));
                }
                if log && bounds.is_some_and(|(low, _)| low <= 0.0) {
                    bounds = None;
                }
                (bounds, false)
            }
            (GeneralSeriesKind::DensityCurve, AxisDimension::Y) => {
                let GeneralStatsModel::Density(model) = self.general_stats_model(series)? else {
                    return Some((None, true));
                };
                (Some((0.0, model.curve.max_density)), true)
            }
            (GeneralSeriesKind::Ecdf, AxisDimension::Y) => (Some((0.0, 1.0)), true),
            (GeneralSeriesKind::QqPlot, AxisDimension::X) => {
                let GeneralStatsModel::Qq(model) = self.general_stats_model(series)? else {
                    return Some((None, false));
                };
                (Some((model.theoretical_min, model.theoretical_max)), false)
            }
            (
                GeneralSeriesKind::ParallelCoordinates | GeneralSeriesKind::ScatterMatrix,
                AxisDimension::X | AxisDimension::Y,
            ) => (Some((0.0, 1.0)), false),
            _ => (None, false),
        };
        Some(result)
    }

    /// Hit test one statistics series; returns the best mark within `max_distance` (CSS px,
    /// plot-local X / chart-space Y like the other general hit paths).
    pub(crate) fn general_stats_hit(
        &self,
        series: &GeneralSeries,
        dataset: &GeneralDataset,
        x_css: f64,
        y_css: f64,
        max_distance: f64,
    ) -> Option<GeneralSeriesHit> {
        let kind = series.kind();
        let mut best: Option<GeneralSeriesHit> = None;
        macro_rules! consider {
            ($row:expr, $row_id:expr, $distance:expr) => {{
                let distance = $distance;
                if distance <= max_distance
                    && best
                        .as_ref()
                        .is_none_or(|current: &GeneralSeriesHit| distance < current.distance)
                {
                    best = Some(GeneralSeriesHit {
                        series: series.id(),
                        row: $row,
                        row_id: $row_id,
                        distance,
                    });
                }
            }};
        }
        match kind {
            GeneralSeriesKind::Violin => {
                let side = series.stats().violin_side;
                self.visit_general_violins(series, |geometry| {
                    if x_css < geometry.slot_left || x_css > geometry.slot_right {
                        return;
                    }
                    // Split violins only answer on their half of the slot.
                    match side {
                        crate::GeneralViolinSide::Full => {}
                        crate::GeneralViolinSide::Left if x_css > geometry.center_x => return,
                        crate::GeneralViolinSide::Right if x_css < geometry.center_x => return,
                        _ => {}
                    }
                    let Some(value) = geometry.y_scale.invert(y_css) else {
                        return;
                    };
                    let half_width = geometry.half_width_at(value);
                    let distance = ((x_css - geometry.center_x).abs() - half_width).max(0.0);
                    consider!(
                        geometry.position,
                        stats_row_identity(kind, geometry.position),
                        distance
                    );
                });
            }
            GeneralSeriesKind::DensityCurve => {
                if let Some(geometry) = self.general_density_geometry(series) {
                    let mut best_index = 0usize;
                    let mut best_distance = f64::INFINITY;
                    for index in 1..geometry.points.len() {
                        let (x0, y0) = geometry.points[index - 1];
                        let (x1, y1) = geometry.points[index];
                        let distance = segment_distance(x_css, y_css, x0, y0, x1, y1);
                        if distance < best_distance {
                            best_distance = distance;
                            best_index = index;
                        }
                    }
                    if best_distance.is_finite() {
                        consider!(
                            best_index,
                            stats_row_identity(kind, best_index),
                            (best_distance - 3.0).max(0.0)
                        );
                    }
                }
            }
            GeneralSeriesKind::Ridgeline => {
                if let Some(geometry) = self.general_ridgeline_geometry(series) {
                    for band in &geometry.bands {
                        if y_css > band.base_y || y_css < band.base_y - band.height {
                            continue;
                        }
                        let Some(value) = geometry.x_scale.invert(x_css) else {
                            continue;
                        };
                        let category = &geometry.model.categories[band.position];
                        let share = if geometry.model.max_density > 0.0 {
                            (kde_density_at(&category.curve, value) / geometry.model.max_density)
                                .clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let curve_y = band.base_y - share * band.height;
                        consider!(
                            band.position,
                            stats_row_identity(kind, band.position),
                            (curve_y - y_css).max(0.0)
                        );
                    }
                }
            }
            GeneralSeriesKind::StripPlot
            | GeneralSeriesKind::Beeswarm
            | GeneralSeriesKind::DotPlot => {
                self.visit_general_stats_points(series, |point| {
                    let distance =
                        ((x_css - point.x).hypot(y_css - point.y) - point.radius).max(0.0);
                    if let Some(row_id) = dataset.row_identity(point.row).cloned() {
                        consider!(point.row, row_id, distance);
                    }
                });
            }
            GeneralSeriesKind::Ecdf => {
                if let Some((path, count)) = self.general_ecdf_path(series) {
                    for index in 1..path.len() {
                        let distance = segment_distance(
                            x_css,
                            y_css,
                            path[index - 1].0,
                            path[index - 1].1,
                            path[index].0,
                            path[index].1,
                        );
                        // Risers (odd index) belong to the step they jump to; treads belong to
                        // the step that set their level.
                        let row = if index % 2 == 1 {
                            (index - 1) / 2
                        } else {
                            (index / 2).saturating_sub(1)
                        }
                        .min(count.saturating_sub(1));
                        consider!(row, stats_row_identity(kind, row), distance);
                    }
                }
            }
            GeneralSeriesKind::QqPlot => {
                if let Some((points, _)) = self.general_qq_geometry(series) {
                    let radius = series.point_radius();
                    for (row, x, y) in points {
                        let distance = ((x_css - x).hypot(y_css - y) - radius).max(0.0);
                        consider!(row, stats_row_identity(kind, row), distance);
                    }
                }
            }
            GeneralSeriesKind::Hexbin => {
                self.visit_general_hexbins(series, |bin| {
                    if general_stats::hexbin_contains(bin.radius, x_css - bin.x, y_css - bin.y) {
                        consider!(bin.index, stats_row_identity(kind, bin.index), 0.0);
                    } else if max_distance > 0.0 {
                        let distance = ((x_css - bin.x).hypot(y_css - bin.y)
                            - bin.radius * 3.0_f64.sqrt() / 2.0)
                            .max(0.0);
                        consider!(bin.index, stats_row_identity(kind, bin.index), distance);
                    }
                });
            }
            GeneralSeriesKind::Contour => {
                if let Some(geometry) = self.general_contour_geometry(series) {
                    for (level_index, line) in geometry.model.lines.iter().enumerate() {
                        for path in &line.paths {
                            for index in 1..path.len() {
                                let (Some((x0, y0)), Some((x1, y1))) = (
                                    geometry.project(path[index - 1]),
                                    geometry.project(path[index]),
                                ) else {
                                    continue;
                                };
                                let distance = segment_distance(x_css, y_css, x0, y0, x1, y1);
                                consider!(
                                    level_index,
                                    stats_row_identity(kind, level_index),
                                    distance
                                );
                            }
                        }
                    }
                }
            }
            GeneralSeriesKind::ParallelCoordinates => {
                if let Some(geometry) = self.general_parallel_geometry(series) {
                    for (row, points) in &geometry.rows {
                        for index in 1..points.len() {
                            let distance = segment_distance(
                                x_css,
                                y_css,
                                points[index - 1].0,
                                points[index - 1].1,
                                points[index].0,
                                points[index].1,
                            );
                            if let Some(row_id) = dataset.row_identity(*row).cloned() {
                                consider!(*row, row_id, distance);
                            }
                        }
                    }
                }
            }
            GeneralSeriesKind::ScatterMatrix => {
                if let Some(geometry) = self.general_scatter_matrix_geometry(series) {
                    let radius = (series.point_radius() * 0.5).max(1.5);
                    for cell in &geometry.cells {
                        if cell.x_field == cell.y_field {
                            continue;
                        }
                        if x_css < cell.left - 4.0
                            || x_css > cell.left + cell.width + 4.0
                            || y_css < cell.top - 4.0
                            || y_css > cell.top + cell.height + 4.0
                        {
                            continue;
                        }
                        for row in (0..geometry.rows).step_by(geometry.stride) {
                            let Some((x, y)) = geometry.point(dataset, cell, row) else {
                                continue;
                            };
                            let distance = ((x_css - x).hypot(y_css - y) - radius).max(0.0);
                            if let Some(row_id) = dataset.row_identity(row).cloned() {
                                consider!(row, row_id, distance);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        best
    }

    /// The painted anchor of one statistics mark (pane-local X, chart-space Y).
    pub(crate) fn general_stats_mark_anchor(
        &self,
        series: &GeneralSeries,
        row: usize,
    ) -> Option<(f64, f64)> {
        let mut found = None;
        match series.kind() {
            GeneralSeriesKind::Violin => self.visit_general_violins(series, |geometry| {
                if geometry.position == row {
                    found = geometry
                        .y(geometry.category.median)
                        .map(|y| (geometry.center_x, y));
                }
            }),
            GeneralSeriesKind::DensityCurve => {
                if let Some(geometry) = self.general_density_geometry(series) {
                    found = geometry.points.get(row).copied();
                }
            }
            GeneralSeriesKind::Ridgeline => {
                if let Some(geometry) = self.general_ridgeline_geometry(series)
                    && let Some(band) = geometry.bands.iter().find(|band| band.position == row)
                {
                    let category = &geometry.model.categories[band.position];
                    let peak = category
                        .curve
                        .density
                        .iter()
                        .enumerate()
                        .max_by(|a, b| f64::total_cmp(a.1, b.1))
                        .map(|(index, _)| index)
                        .unwrap_or(0);
                    found = geometry.point(band, peak);
                }
            }
            GeneralSeriesKind::StripPlot
            | GeneralSeriesKind::Beeswarm
            | GeneralSeriesKind::DotPlot => {
                self.visit_general_stats_points(series, |point| {
                    if point.row == row {
                        found = Some((point.x, point.y));
                    }
                });
            }
            GeneralSeriesKind::Ecdf => {
                if let Some((path, count)) = self.general_ecdf_path(series)
                    && row < count
                {
                    let index = (1 + row * 2).min(path.len().saturating_sub(1));
                    found = path.get(index).copied();
                }
            }
            GeneralSeriesKind::QqPlot => {
                if let Some((points, _)) = self.general_qq_geometry(series) {
                    found = points
                        .iter()
                        .find(|(point_row, _, _)| *point_row == row)
                        .map(|(_, x, y)| (*x, *y));
                }
            }
            GeneralSeriesKind::Hexbin => self.visit_general_hexbins(series, |bin| {
                if bin.index == row {
                    found = Some((bin.x, bin.y));
                }
            }),
            GeneralSeriesKind::ParallelCoordinates => {
                if let Some(geometry) = self.general_parallel_geometry(series) {
                    found = geometry
                        .rows
                        .iter()
                        .find(|(point_row, _)| *point_row == row)
                        .and_then(|(_, points)| points.get(points.len() / 2).copied());
                }
            }
            GeneralSeriesKind::ScatterMatrix => {
                if let Some(geometry) = self.general_scatter_matrix_geometry(series) {
                    let dataset = self.general_geometry_dataset(series.dataset())?;
                    let cell = geometry
                        .cells
                        .iter()
                        .find(|cell| cell.x_field != cell.y_field)?;
                    found = geometry.point(dataset, cell, row);
                }
            }
            _ => {}
        }
        found
    }

    /// The tooltip snapshot of one synthetic statistics row.
    pub(crate) fn general_stats_tooltip_snapshot(
        &self,
        series: &GeneralSeries,
        row: usize,
    ) -> Option<GeneralTooltipSnapshot> {
        let dataset = self.general_geometry_dataset(series.dataset())?;
        let color = series
            .color()
            .and_then(Color::parse_css)
            .unwrap_or(crate::DEFAULT_LINE_COLOR);
        let base = GeneralTooltipSnapshot {
            series: series.id(),
            row,
            row_id: stats_row_identity(series.kind(), row),
            x_label: String::new(),
            y_label: None,
            label: None,
            value: Some(0.0),
            low: None,
            high: None,
            x_low: None,
            x_high: None,
            q1: None,
            q3: None,
            size: None,
            detail: None,
            title: series.title().to_owned(),
            color: Color::rgb(color.r(), color.g(), color.b()),
        };
        let stats_detail = |count: usize| {
            Some(crate::GeneralRowDetail::Stats {
                count: count.min(u32::MAX as usize) as u32,
                min: f64::NAN,
                max: f64::NAN,
            })
        };
        let snapshot = match self.general_stats_model(series)? {
            GeneralStatsModel::Violin(model) => {
                let category = model.categories.get(row)?;
                GeneralTooltipSnapshot {
                    x_label: dataset
                        .categories()?
                        .get(category.category_index as usize)?
                        .clone(),
                    value: Some(category.median),
                    low: Some(category.min),
                    high: Some(category.max),
                    q1: Some(category.q1),
                    q3: Some(category.q3),
                    detail: stats_detail(category.curve.samples),
                    ..base
                }
            }
            GeneralStatsModel::Density(model) => {
                let value = *model.curve.grid.get(row)?;
                GeneralTooltipSnapshot {
                    x_label: value.to_string(),
                    value: Some(model.curve.density[row]),
                    detail: stats_detail(model.curve.samples),
                    ..base
                }
            }
            GeneralStatsModel::Ridgeline(model) => {
                let category = model.categories.get(row)?;
                GeneralTooltipSnapshot {
                    x_label: dataset
                        .categories()?
                        .get(category.category_index as usize)?
                        .clone(),
                    value: Some(category.curve.max_density),
                    low: Some(category.curve.min),
                    high: Some(category.curve.max),
                    detail: stats_detail(category.curve.samples),
                    ..base
                }
            }
            GeneralStatsModel::Ecdf(model) => {
                let &(value, f) = model.points.get(row)?;
                GeneralTooltipSnapshot {
                    x_label: value.to_string(),
                    value: Some(f),
                    detail: stats_detail(model.count),
                    ..base
                }
            }
            GeneralStatsModel::Qq(model) => {
                let &(theoretical, sample, _) = model.points.get(row)?;
                GeneralTooltipSnapshot {
                    x_label: format!("{theoretical:.4}"),
                    value: Some(sample),
                    detail: stats_detail(model.points.len()),
                    ..base
                }
            }
            GeneralStatsModel::Hexbin(model) => {
                let bin = model.bins.get(row)?;
                let center = self
                    .general_numeric_axis_scale(series, AxisDimension::X)
                    .zip(self.general_numeric_axis_scale(series, AxisDimension::Y))
                    .and_then(|(x_scale, y_scale)| {
                        let pane_index = self.pane_index_for_id(series.pane_id())?;
                        let plot = self.general_plot_rect(pane_index)?;
                        let x = x_scale.invert(bin.cx)?;
                        let y = y_scale.invert(plot.y + bin.cy)?;
                        Some(format!("({x:.4}, {y:.4})"))
                    })
                    .unwrap_or_else(|| {
                        crate::text::complete::HEXBIN_CELL
                            .text(self.language)
                            .to_owned()
                    });
                GeneralTooltipSnapshot {
                    x_label: center,
                    value: Some(bin.count as f64),
                    detail: Some(crate::GeneralRowDetail::Stats {
                        count: bin.count.min(u32::MAX as usize) as u32,
                        min: f64::NAN,
                        max: model.max_count as f64,
                    }),
                    ..base
                }
            }
            GeneralStatsModel::Contour(model) => {
                let line = model.lines.get(row)?;
                let next = model.thresholds.get(row + 1).copied().unwrap_or(f64::NAN);
                GeneralTooltipSnapshot {
                    x_label: format!("level {}", line.level),
                    value: Some(line.level),
                    detail: Some(crate::GeneralRowDetail::Stats {
                        count: 0,
                        min: line.level,
                        max: next,
                    }),
                    ..base
                }
            }
            GeneralStatsModel::Offsets(_) | GeneralStatsModel::Matrix(_) => return None,
        };
        Some(snapshot)
    }

    /// The accessibility items of one synthetic-row statistics series.
    pub(crate) fn general_stats_accessibility_snapshot(
        &self,
        series: &GeneralSeries,
        offset: usize,
        limit: usize,
    ) -> Option<crate::GeneralAccessibilitySnapshot> {
        let total_rows = self.general_stats_row_count(series)?;
        let offset = offset.min(total_rows);
        let end = offset
            .saturating_add(limit.min(crate::MAX_GENERAL_ACCESSIBILITY_ITEMS))
            .min(total_rows);
        let items = (offset..end)
            .map(|row| {
                let snapshot = self.general_stats_tooltip_snapshot(series, row)?;
                Some(GeneralAccessibilityItem {
                    row: snapshot.row,
                    row_id: snapshot.row_id,
                    x_label: snapshot.x_label,
                    y_label: snapshot.y_label,
                    label: snapshot.label,
                    value: snapshot.value,
                    low: snapshot.low,
                    high: snapshot.high,
                    x_low: snapshot.x_low,
                    x_high: snapshot.x_high,
                    q1: snapshot.q1,
                    q3: snapshot.q3,
                    size: snapshot.size,
                    detail: snapshot.detail,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(crate::GeneralAccessibilitySnapshot {
            series: series.id(),
            title: series.title().to_owned(),
            total_rows,
            offset,
            items,
        })
    }
}
