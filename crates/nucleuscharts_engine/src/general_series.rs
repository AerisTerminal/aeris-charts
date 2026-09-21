use std::collections::HashMap;
use std::num::NonZeroU32;

use nucleuscharts_core::scale::general_scale::{BandScale, LinearScale};
use nucleuscharts_render::color::Color;

use crate::{
    AxisDimension, ChartEngine, ChartError, ErrorCode, GeneralAxisDomain, GeneralDatasetId,
    GeneralRowIdentity, GeneralScaleType, GeneralXKind, HorizontalDomain, PaneId,
};

pub const MAX_GENERAL_SERIES: usize = 1_024;
pub const MAX_GENERAL_SERIES_TITLE_BYTES: usize = 4_096;
pub const MAX_GENERAL_SERIES_COLOR_BYTES: usize = 256;
pub const MAX_GENERAL_ACCESSIBILITY_ITEMS: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneralSeriesKind {
    Column,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GeneralSeriesId(NonZeroU32);

impl GeneralSeriesId {
    pub fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralSeriesOptions {
    pub kind: GeneralSeriesKind,
    pub pane: usize,
    pub dataset: GeneralDatasetId,
    pub x_axis_id: String,
    pub y_axis_id: String,
    pub visible: bool,
    pub title: String,
    pub color: Option<String>,
}

impl GeneralSeriesOptions {
    pub fn column(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GeneralSeriesKind::Column,
            pane,
            dataset,
            x_axis_id: x_axis_id.into(),
            y_axis_id: y_axis_id.into(),
            visible: true,
            title: String::new(),
            color: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralSeries {
    id: GeneralSeriesId,
    kind: GeneralSeriesKind,
    pane_id: PaneId,
    dataset: GeneralDatasetId,
    x_axis_id: String,
    y_axis_id: String,
    visible: bool,
    title: String,
    color: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GeneralColumnGeometry {
    pub(crate) row: usize,
    pub(crate) left: f64,
    pub(crate) right: f64,
    pub(crate) top: f64,
    pub(crate) bottom: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GeneralHitMode {
    Exact,
    Nearest { max_distance: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralSeriesHit {
    pub series: GeneralSeriesId,
    pub row: usize,
    pub row_id: GeneralRowIdentity,
    pub distance: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralTooltipSnapshot {
    pub series: GeneralSeriesId,
    pub row: usize,
    pub row_id: GeneralRowIdentity,
    pub x_label: String,
    pub value: Option<f64>,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralAccessibilityItem {
    pub row: usize,
    pub row_id: GeneralRowIdentity,
    pub x_label: String,
    pub value: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralAccessibilitySnapshot {
    pub series: GeneralSeriesId,
    pub title: String,
    pub total_rows: usize,
    pub offset: usize,
    pub items: Vec<GeneralAccessibilityItem>,
}

impl GeneralSeries {
    pub fn id(&self) -> GeneralSeriesId {
        self.id
    }

    pub fn kind(&self) -> GeneralSeriesKind {
        self.kind
    }

    pub fn pane_id(&self) -> PaneId {
        self.pane_id
    }

    pub fn dataset(&self) -> GeneralDatasetId {
        self.dataset
    }

    pub fn x_axis_id(&self) -> &str {
        &self.x_axis_id
    }

    pub fn y_axis_id(&self) -> &str {
        &self.y_axis_id
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn color(&self) -> Option<&str> {
        self.color.as_deref()
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.x_axis_id.capacity()
            + self.y_axis_id.capacity()
            + self.title.capacity()
            + self.color.as_ref().map_or(0, String::capacity)
    }
}

pub(crate) struct GeneralSeriesRegistry {
    series: Vec<GeneralSeries>,
    next_id: u32,
}

impl GeneralSeriesRegistry {
    pub(crate) fn new() -> Self {
        Self {
            series: Vec::new(),
            next_id: 1,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.series.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.series.is_empty()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &GeneralSeries> {
        self.series.iter()
    }

    pub(crate) fn get(&self, id: GeneralSeriesId) -> Option<&GeneralSeries> {
        self.series.iter().find(|series| series.id == id)
    }

    pub(crate) fn get_mut(&mut self, id: GeneralSeriesId) -> Option<&mut GeneralSeries> {
        self.series.iter_mut().find(|series| series.id == id)
    }

    pub(crate) fn insert(
        &mut self,
        pane_id: PaneId,
        options: GeneralSeriesOptions,
    ) -> Result<GeneralSeriesId, ChartError> {
        if self.series.len() >= MAX_GENERAL_SERIES {
            return Err(resource(format!(
                "a chart supports at most {MAX_GENERAL_SERIES} general series"
            )));
        }
        validate_presentation(&options)?;
        let id = NonZeroU32::new(self.next_id)
            .map(GeneralSeriesId)
            .ok_or_else(|| resource("general series identity space is exhausted"))?;
        let next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| resource("general series identity space is exhausted"))?;
        self.series.push(GeneralSeries {
            id,
            kind: options.kind,
            pane_id,
            dataset: options.dataset,
            x_axis_id: options.x_axis_id,
            y_axis_id: options.y_axis_id,
            visible: options.visible,
            title: options.title,
            color: options.color,
        });
        self.next_id = next_id;
        Ok(id)
    }

    pub(crate) fn remove(&mut self, id: GeneralSeriesId) -> bool {
        let Some(index) = self.series.iter().position(|series| series.id == id) else {
            return false;
        };
        self.series.remove(index);
        true
    }

    pub(crate) fn uses_axis(&self, id: &str) -> bool {
        self.series
            .iter()
            .any(|series| series.x_axis_id == id || series.y_axis_id == id)
    }

    pub(crate) fn uses_dataset(&self, id: GeneralDatasetId) -> bool {
        self.series.iter().any(|series| series.dataset == id)
    }

    pub(crate) fn uses_pane(&self, pane_id: PaneId) -> bool {
        self.series.iter().any(|series| series.pane_id == pane_id)
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.series.capacity() * std::mem::size_of::<GeneralSeries>()
            + self
                .series
                .iter()
                .map(GeneralSeries::estimated_bytes)
                .sum::<usize>()
    }
}

impl Default for GeneralSeriesRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartEngine {
    #[doc(hidden)]
    pub fn add_general_series(
        &mut self,
        options: GeneralSeriesOptions,
    ) -> Result<GeneralSeriesId, ChartError> {
        let pane_id = self
            .pane_stable_id(options.pane)
            .ok_or_else(|| invalid("general series references a stale pane"))?;
        let pane_domain = self
            .pane_horizontal_domain(options.pane)
            .ok_or_else(|| invalid("general series references a stale pane"))?;
        let dataset_kind = self
            .general_dataset(options.dataset)
            .map(|dataset| dataset.x_kind())
            .ok_or_else(|| {
                ChartError::new(ErrorCode::InvalidHandle, "general dataset handle is stale")
            })?;
        let x_axis = self
            .general_axis(&options.x_axis_id)
            .cloned()
            .ok_or_else(|| invalid("general series X axis does not exist"))?;
        let y_axis = self
            .general_axis(&options.y_axis_id)
            .cloned()
            .ok_or_else(|| invalid("general series Y axis does not exist"))?;

        if x_axis.pane_id() != pane_id || y_axis.pane_id() != pane_id {
            return Err(invalid("general series axes must belong to its pane"));
        }
        if x_axis.dimension() != AxisDimension::X || y_axis.dimension() != AxisDimension::Y {
            return Err(invalid("general series axis dimensions are incompatible"));
        }
        match options.kind {
            GeneralSeriesKind::Column => {
                if pane_domain
                    != (HorizontalDomain::Category {
                        scale: crate::CategoryScaleType::Band,
                    })
                    || dataset_kind != GeneralXKind::Category
                    || x_axis.scale() != GeneralScaleType::Band
                    || y_axis.scale() != GeneralScaleType::Linear
                {
                    return Err(invalid(
                        "the initial column series requires a category-band pane, category X data, a band X axis, and a linear Y axis",
                    ));
                }
            }
        }
        validate_presentation(&options)?;
        let id = if let Some(registry) = self.general_series.as_mut() {
            registry.insert(pane_id, options)?
        } else {
            let mut registry = GeneralSeriesRegistry::new();
            let id = registry.insert(pane_id, options)?;
            self.general_series = Some(registry);
            id
        };
        self.invalidate_frame_all();
        Ok(id)
    }

    #[doc(hidden)]
    pub fn general_series(&self, id: GeneralSeriesId) -> Option<&GeneralSeries> {
        self.general_series.as_ref()?.get(id)
    }

    #[doc(hidden)]
    pub fn general_series_count(&self) -> usize {
        self.general_series
            .as_ref()
            .map_or(0, GeneralSeriesRegistry::len)
    }

    #[doc(hidden)]
    pub fn remove_general_series(&mut self, id: GeneralSeriesId) -> bool {
        let Some(registry) = self.general_series.as_mut() else {
            return false;
        };
        if !registry.remove(id) {
            return false;
        }
        if registry.is_empty() {
            self.general_series = None;
        }
        self.invalidate_frame_all();
        true
    }

    #[doc(hidden)]
    pub fn set_general_series_visible(&mut self, id: GeneralSeriesId, visible: bool) -> bool {
        let Some(registry) = self.general_series.as_mut() else {
            return false;
        };
        let Some(series) = registry.get_mut(id) else {
            return false;
        };
        if series.visible == visible {
            return true;
        }
        series.visible = visible;
        self.invalidate_frame_all();
        true
    }

    pub(crate) fn general_series_iter(&self) -> impl Iterator<Item = &GeneralSeries> {
        self.general_series
            .as_ref()
            .into_iter()
            .flat_map(GeneralSeriesRegistry::iter)
    }

    pub(crate) fn general_series_uses_axis(&self, id: &str) -> bool {
        self.general_series
            .as_ref()
            .is_some_and(|registry| registry.uses_axis(id))
    }

    pub(crate) fn general_series_uses_dataset(&self, id: GeneralDatasetId) -> bool {
        self.general_series
            .as_ref()
            .is_some_and(|registry| registry.uses_dataset(id))
    }

    pub(crate) fn general_series_uses_pane(&self, pane_id: PaneId) -> bool {
        self.general_series
            .as_ref()
            .is_some_and(|registry| registry.uses_pane(pane_id))
    }

    pub(crate) fn visit_general_columns<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralColumnGeometry),
    {
        if !series.visible || series.kind != GeneralSeriesKind::Column {
            return;
        }
        let Some(pane_index) = self.pane_index_for_id(series.pane_id) else {
            return;
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return;
        };
        let Some(dataset) = self.general_dataset(series.dataset) else {
            return;
        };
        let (Some(categories), Some(category_indices)) =
            (dataset.categories(), dataset.category_indices())
        else {
            return;
        };
        let (Some(x_axis), Some(y_axis)) = (
            self.general_axis(&series.x_axis_id),
            self.general_axis(&series.y_axis_id),
        ) else {
            return;
        };
        let Some(GeneralAxisDomain::Category(axis_categories)) =
            self.effective_general_axis_domain(x_axis)
        else {
            return;
        };
        let Some(GeneralAxisDomain::Numeric([y_from, y_to])) =
            self.effective_general_axis_domain(y_axis)
        else {
            return;
        };
        let x_range = if x_axis.reverse() {
            (plot.width, 0.0)
        } else {
            (0.0, plot.width)
        };
        let Ok(x_scale) = BandScale::new(
            axis_categories.len(),
            x_range.0,
            x_range.1,
            x_axis.band_padding_inner(),
            x_axis.band_padding_outer(),
            0.5,
        ) else {
            return;
        };
        let plot_bottom = plot.y + plot.height;
        let y_range = if y_axis.reverse() {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let Ok(y_scale) = LinearScale::new(y_from, y_to, y_range.0, y_range.1) else {
            return;
        };
        let baseline = y_scale.coordinate_clamped(0.0).unwrap_or(y_range.0);
        let axis_lookup: HashMap<&str, usize> = axis_categories
            .iter()
            .enumerate()
            .map(|(index, category)| (category.as_str(), index))
            .collect();

        for (row, &category_index) in category_indices.iter().enumerate() {
            if !dataset.y_is_valid(row) {
                continue;
            }
            let Some(category) = usize::try_from(category_index)
                .ok()
                .and_then(|index| categories.get(index))
            else {
                continue;
            };
            let Some(&axis_index) = axis_lookup.get(category.as_str()) else {
                continue;
            };
            let Some((left, right)) = x_scale.bounds(axis_index) else {
                continue;
            };
            let Some(value_y) = y_scale.coordinate_clamped(dataset.y()[row]) else {
                continue;
            };
            let left = left.clamp(0.0, plot.width);
            let right = right.clamp(0.0, plot.width);
            let top = baseline.min(value_y).clamp(plot.y, plot_bottom);
            let bottom = baseline.max(value_y).clamp(plot.y, plot_bottom);
            if right <= left || bottom <= top {
                continue;
            }
            visit(GeneralColumnGeometry {
                row,
                left,
                right,
                top,
                bottom,
            });
        }
    }

    #[doc(hidden)]
    pub fn general_hit_test(
        &self,
        pane_index: usize,
        x_css: f64,
        y_css: f64,
        mode: GeneralHitMode,
    ) -> Option<GeneralSeriesHit> {
        if !x_css.is_finite() || !y_css.is_finite() {
            return None;
        }
        let max_distance = match mode {
            GeneralHitMode::Exact => 0.0,
            GeneralHitMode::Nearest { max_distance }
                if max_distance.is_finite() && max_distance >= 0.0 =>
            {
                max_distance
            }
            GeneralHitMode::Nearest { .. } => return None,
        };
        let pane_id = self.pane_stable_id(pane_index)?;
        let registry = self.general_series.as_ref()?;
        let mut best: Option<GeneralSeriesHit> = None;

        for series in registry
            .series
            .iter()
            .rev()
            .filter(|series| series.visible && series.pane_id == pane_id)
        {
            let Some(dataset) = self.general_dataset(series.dataset) else {
                continue;
            };
            self.visit_general_columns(series, |geometry| {
                let distance = distance_to_rect(x_css, y_css, geometry);
                if distance > max_distance {
                    return;
                }
                if best
                    .as_ref()
                    .is_some_and(|current| distance >= current.distance)
                {
                    return;
                }
                let Some(row_id) = dataset.row_identity(geometry.row).cloned() else {
                    return;
                };
                best = Some(GeneralSeriesHit {
                    series: series.id,
                    row: geometry.row,
                    row_id,
                    distance,
                });
            });
            if matches!(mode, GeneralHitMode::Exact)
                && best.as_ref().is_some_and(|hit| hit.distance == 0.0)
            {
                break;
            }
        }
        best
    }

    #[doc(hidden)]
    pub fn general_tooltip_snapshot(
        &self,
        series_id: GeneralSeriesId,
        row: usize,
    ) -> Option<GeneralTooltipSnapshot> {
        let series = self.general_series(series_id)?;
        let dataset = self.general_dataset(series.dataset)?;
        let row_id = dataset.row_identity(row)?.clone();
        let category_index = usize::try_from(*dataset.category_indices()?.get(row)?).ok()?;
        let x_label = dataset.categories()?.get(category_index)?.clone();
        Some(GeneralTooltipSnapshot {
            series: series_id,
            row,
            row_id,
            x_label,
            value: dataset.y_is_valid(row).then(|| dataset.y()[row]),
            title: series.title.clone(),
        })
    }

    #[doc(hidden)]
    pub fn general_accessibility_snapshot(
        &self,
        series_id: GeneralSeriesId,
        offset: usize,
        limit: usize,
    ) -> Option<GeneralAccessibilitySnapshot> {
        let series = self.general_series(series_id)?;
        let dataset = self.general_dataset(series.dataset)?;
        let categories = dataset.categories()?;
        let category_indices = dataset.category_indices()?;
        let total_rows = dataset.len();
        let offset = offset.min(total_rows);
        let end = offset
            .saturating_add(limit.min(MAX_GENERAL_ACCESSIBILITY_ITEMS))
            .min(total_rows);
        let mut items = Vec::with_capacity(end - offset);
        for row in offset..end {
            let row_id = dataset.row_identity(row)?.clone();
            let category_index = usize::try_from(*category_indices.get(row)?).ok()?;
            let x_label = categories.get(category_index)?.clone();
            items.push(GeneralAccessibilityItem {
                row,
                row_id,
                x_label,
                value: dataset.y_is_valid(row).then(|| dataset.y()[row]),
            });
        }
        Some(GeneralAccessibilitySnapshot {
            series: series_id,
            title: series.title.clone(),
            total_rows,
            offset,
            items,
        })
    }
}

fn distance_to_rect(x: f64, y: f64, geometry: GeneralColumnGeometry) -> f64 {
    let dx = if x < geometry.left {
        geometry.left - x
    } else if x > geometry.right {
        x - geometry.right
    } else {
        0.0
    };
    let dy = if y < geometry.top {
        geometry.top - y
    } else if y > geometry.bottom {
        y - geometry.bottom
    } else {
        0.0
    };
    dx.hypot(dy)
}

fn validate_presentation(options: &GeneralSeriesOptions) -> Result<(), ChartError> {
    if options.title.len() > MAX_GENERAL_SERIES_TITLE_BYTES {
        return Err(resource(format!(
            "general series title exceeds {MAX_GENERAL_SERIES_TITLE_BYTES} UTF-8 bytes"
        )));
    }
    if let Some(color) = options.color.as_deref() {
        if color.len() > MAX_GENERAL_SERIES_COLOR_BYTES {
            return Err(resource(format!(
                "general series color exceeds {MAX_GENERAL_SERIES_COLOR_BYTES} UTF-8 bytes"
            )));
        }
        if Color::parse_css(color).is_none() {
            return Err(invalid("general series color must be a valid CSS color"));
        }
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::InvalidOptions, message)
}

fn resource(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::ResourceLimit, message)
}
