use std::cell::RefCell;
use std::collections::HashMap;
use std::num::NonZeroU32;

use nucleuscharts_core::scale::general_scale::{BandScale, LinearScale};
use nucleuscharts_render::color::Color;

use crate::general_axes::NumericAxisScale;
use crate::{
    AxisDimension, ChartEngine, ChartError, ErrorCode, GeneralAxisDomain, GeneralDatasetId,
    GeneralRowIdentity, GeneralScaleType, GeneralXKind, GeneralXyInput, HorizontalDomain, PaneId,
};

pub const MAX_GENERAL_SERIES: usize = 1_024;
pub const MAX_GENERAL_SERIES_TITLE_BYTES: usize = 4_096;
pub const MAX_GENERAL_SERIES_COLOR_BYTES: usize = 256;
pub const MAX_GENERAL_ACCESSIBILITY_ITEMS: usize = 512;
pub const MIN_GENERAL_POINT_RADIUS: f64 = 1.0;
pub const MAX_GENERAL_POINT_RADIUS: f64 = 64.0;
const SCATTER_GRID_BASE_CELL_CSS: f64 = 32.0;
const MAX_SCATTER_GRID_CELLS: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GeneralSeriesKind {
    Column,
    Scatter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GeneralSeriesId(NonZeroU32);

impl GeneralSeriesId {
    pub fn get(self) -> u32 {
        self.0.get()
    }

    #[doc(hidden)]
    pub fn from_raw(value: u32) -> Option<Self> {
        NonZeroU32::new(value).map(Self)
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
    pub point_radius: f64,
    pub data_labels: bool,
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
            point_radius: 3.0,
            data_labels: false,
        }
    }

    pub fn scatter(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GeneralSeriesKind::Scatter,
            pane,
            dataset,
            x_axis_id: x_axis_id.into(),
            y_axis_id: y_axis_id.into(),
            visible: true,
            title: String::new(),
            color: None,
            point_radius: 3.0,
            data_labels: false,
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
    point_radius: f64,
    data_labels: bool,
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
pub(crate) struct GeneralScatterGeometry {
    pub(crate) row: usize,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) radius: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScatterGeometryKey {
    dataset_generation: u64,
    plot_width: u64,
    plot_y: u64,
    plot_height: u64,
    x_domain: [u64; 2],
    y_domain: [u64; 2],
    x_scale: GeneralScaleType,
    y_scale: GeneralScaleType,
    x_reverse: bool,
    y_reverse: bool,
    radius: u64,
}

#[derive(Clone, Copy)]
struct ScatterGeometryContext {
    key: ScatterGeometryKey,
    x_scale: NumericAxisScale,
    y_scale: NumericAxisScale,
    plot_width: f64,
    plot_y: f64,
    plot_bottom: f64,
    radius: f64,
}

struct ScatterSpatialIndex {
    key: ScatterGeometryKey,
    origin_y: f64,
    cell_size: f64,
    columns: usize,
    rows: usize,
    offsets: Vec<usize>,
    point_indices: Vec<u32>,
    points: Vec<GeneralScatterGeometry>,
}

impl ScatterSpatialIndex {
    fn new(
        key: ScatterGeometryKey,
        origin_y: f64,
        plot_width: f64,
        plot_height: f64,
        points: Vec<GeneralScatterGeometry>,
    ) -> Self {
        let mut cell_size = SCATTER_GRID_BASE_CELL_CSS;
        let (mut columns, mut rows) = grid_dimensions(plot_width, plot_height, cell_size);
        while columns.saturating_mul(rows) > MAX_SCATTER_GRID_CELLS {
            cell_size *= 2.0;
            (columns, rows) = grid_dimensions(plot_width, plot_height, cell_size);
        }
        let cell_count = columns.saturating_mul(rows).max(1);
        let mut counts = vec![0usize; cell_count];
        for point in &points {
            let cell = scatter_cell(point.x, point.y, origin_y, cell_size, columns, rows);
            counts[cell] += 1;
        }
        let mut offsets = vec![0usize; cell_count + 1];
        for (index, count) in counts.into_iter().enumerate() {
            offsets[index + 1] = offsets[index] + count;
        }
        let mut cursors = offsets[..cell_count].to_vec();
        let mut point_indices = vec![0u32; points.len()];
        for (point_index, point) in points.iter().enumerate() {
            let cell = scatter_cell(point.x, point.y, origin_y, cell_size, columns, rows);
            let slot = cursors[cell];
            point_indices[slot] = u32::try_from(point_index)
                .expect("general scatter rows stay below the u32 index ceiling");
            cursors[cell] += 1;
        }
        Self {
            key,
            origin_y,
            cell_size,
            columns,
            rows,
            offsets,
            point_indices,
            points,
        }
    }

    fn estimated_bytes(&self) -> usize {
        self.offsets.capacity() * std::mem::size_of::<usize>()
            + self.point_indices.capacity() * std::mem::size_of::<u32>()
            + self.points.capacity() * std::mem::size_of::<GeneralScatterGeometry>()
    }

    fn visit_candidates<F>(&self, x: f64, y: f64, expansion: f64, mut visit: F)
    where
        F: FnMut(GeneralScatterGeometry),
    {
        let Some((min_col, max_col)) = grid_query_range(
            x - expansion,
            x + expansion,
            0.0,
            self.cell_size,
            self.columns,
        ) else {
            return;
        };
        let Some((min_row, max_row)) = grid_query_range(
            y - expansion,
            y + expansion,
            self.origin_y,
            self.cell_size,
            self.rows,
        ) else {
            return;
        };
        for row in min_row..=max_row {
            for col in min_col..=max_col {
                let cell = row * self.columns + col;
                for &point_index in &self.point_indices[self.offsets[cell]..self.offsets[cell + 1]]
                {
                    if let Some(point) = self.points.get(point_index as usize) {
                        visit(*point);
                    }
                }
            }
        }
    }
}

fn grid_dimensions(width: f64, height: f64, cell_size: f64) -> (usize, usize) {
    let columns = (width.max(1.0) / cell_size).ceil().max(1.0) as usize;
    let rows = (height.max(1.0) / cell_size).ceil().max(1.0) as usize;
    (columns, rows)
}

fn scatter_cell(
    x: f64,
    y: f64,
    origin_y: f64,
    cell_size: f64,
    columns: usize,
    rows: usize,
) -> usize {
    let col = ((x / cell_size).floor() as isize).clamp(0, columns as isize - 1) as usize;
    let row = (((y - origin_y) / cell_size).floor() as isize).clamp(0, rows as isize - 1) as usize;
    row * columns + col
}

fn grid_query_range(
    from: f64,
    to: f64,
    range_start: f64,
    cell_size: f64,
    count: usize,
) -> Option<(usize, usize)> {
    let first = ((from - range_start) / cell_size).floor() as isize;
    let last = ((to - range_start) / cell_size).floor() as isize;
    if last < 0 || first >= count as isize {
        return None;
    }
    Some((
        first.clamp(0, count as isize - 1) as usize,
        last.clamp(0, count as isize - 1) as usize,
    ))
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
struct GeneralInteractionTarget {
    series: GeneralSeriesId,
    row: usize,
    row_id: GeneralRowIdentity,
    distance: f64,
}

impl From<&GeneralSeriesHit> for GeneralInteractionTarget {
    fn from(hit: &GeneralSeriesHit) -> Self {
        Self {
            series: hit.series,
            row: hit.row,
            row_id: hit.row_id.clone(),
            distance: hit.distance,
        }
    }
}

impl GeneralInteractionTarget {
    fn hit(&self) -> GeneralSeriesHit {
        GeneralSeriesHit {
            series: self.series,
            row: self.row,
            row_id: self.row_id.clone(),
            distance: self.distance,
        }
    }

    fn estimated_bytes(&self) -> usize {
        match &self.row_id {
            GeneralRowIdentity::Explicit(crate::GeneralRowId::Text(value)) => value.capacity(),
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralTooltipSnapshot {
    pub series: GeneralSeriesId,
    pub row: usize,
    pub row_id: GeneralRowIdentity,
    pub x_label: String,
    pub label: Option<String>,
    pub value: Option<f64>,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralAccessibilityItem {
    pub row: usize,
    pub row_id: GeneralRowIdentity,
    pub x_label: String,
    pub label: Option<String>,
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

    pub fn point_radius(&self) -> f64 {
        self.point_radius
    }

    pub fn data_labels(&self) -> bool {
        self.data_labels
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
    scatter_spatial: RefCell<HashMap<GeneralSeriesId, ScatterSpatialIndex>>,
    hovered: Option<GeneralInteractionTarget>,
    selected: Option<GeneralInteractionTarget>,
    accessibility_focused: Option<GeneralInteractionTarget>,
}

impl GeneralSeriesRegistry {
    pub(crate) fn new() -> Self {
        Self {
            series: Vec::new(),
            next_id: 1,
            scatter_spatial: RefCell::new(HashMap::new()),
            hovered: None,
            selected: None,
            accessibility_focused: None,
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
            point_radius: options.point_radius,
            data_labels: options.data_labels,
        });
        self.next_id = next_id;
        Ok(id)
    }

    pub(crate) fn remove(&mut self, id: GeneralSeriesId) -> bool {
        let Some(index) = self.series.iter().position(|series| series.id == id) else {
            return false;
        };
        self.series.remove(index);
        self.scatter_spatial.get_mut().remove(&id);
        if self
            .hovered
            .as_ref()
            .is_some_and(|target| target.series == id)
        {
            self.hovered = None;
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|target| target.series == id)
        {
            self.selected = None;
        }
        if self
            .accessibility_focused
            .as_ref()
            .is_some_and(|target| target.series == id)
        {
            self.accessibility_focused = None;
        }
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
        let scatter_bytes = self.scatter_spatial.try_borrow().map_or(0, |cache| {
            cache.capacity()
                * (std::mem::size_of::<GeneralSeriesId>()
                    + std::mem::size_of::<ScatterSpatialIndex>())
                + cache
                    .values()
                    .map(ScatterSpatialIndex::estimated_bytes)
                    .sum::<usize>()
        });
        self.series.capacity() * std::mem::size_of::<GeneralSeries>()
            + self
                .series
                .iter()
                .map(GeneralSeries::estimated_bytes)
                .sum::<usize>()
            + scatter_bytes
            + self
                .hovered
                .as_ref()
                .map_or(0, GeneralInteractionTarget::estimated_bytes)
            + self
                .selected
                .as_ref()
                .map_or(0, GeneralInteractionTarget::estimated_bytes)
            + self
                .accessibility_focused
                .as_ref()
                .map_or(0, GeneralInteractionTarget::estimated_bytes)
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
        let dataset = self.general_dataset(options.dataset).ok_or_else(|| {
            ChartError::new(ErrorCode::InvalidHandle, "general dataset handle is stale")
        })?;
        let dataset_kind = dataset.x_kind();
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
            GeneralSeriesKind::Scatter => {
                if !matches!(pane_domain, HorizontalDomain::Continuous { .. })
                    || dataset_kind != GeneralXKind::Numeric
                    || !matches!(
                        x_axis.scale(),
                        GeneralScaleType::Linear
                            | GeneralScaleType::Logarithmic
                            | GeneralScaleType::SymmetricLog
                    )
                    || !matches!(
                        y_axis.scale(),
                        GeneralScaleType::Linear
                            | GeneralScaleType::Logarithmic
                            | GeneralScaleType::SymmetricLog
                    )
                {
                    return Err(invalid(
                        "scatter requires a continuous pane, numeric X data, and numeric X/Y axes",
                    ));
                }
            }
        }
        validate_dataset_for_series(options.kind, dataset, &x_axis, &y_axis)?;
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
    pub fn general_series_ids_in_pane(&self, pane_index: usize) -> Vec<GeneralSeriesId> {
        let Some(pane_id) = self.pane_stable_id(pane_index) else {
            return Vec::new();
        };
        self.general_series
            .as_ref()
            .into_iter()
            .flat_map(GeneralSeriesRegistry::iter)
            .filter(|series| series.pane_id == pane_id)
            .map(GeneralSeries::id)
            .collect()
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

    pub(crate) fn validate_general_dataset_replacement(
        &self,
        dataset_id: GeneralDatasetId,
        input: &GeneralXyInput,
    ) -> Result<(), ChartError> {
        let Some(registry) = self.general_series.as_ref() else {
            return Ok(());
        };
        for series in registry
            .series
            .iter()
            .filter(|series| series.dataset == dataset_id)
        {
            let x_axis = self
                .general_axis(&series.x_axis_id)
                .ok_or_else(|| invalid("bound general series X axis is stale"))?;
            let y_axis = self
                .general_axis(&series.y_axis_id)
                .ok_or_else(|| invalid("bound general series Y axis is stale"))?;
            validate_input_for_series(series.kind, input, x_axis, y_axis)?;
        }
        Ok(())
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

    fn scatter_geometry_context(&self, series: &GeneralSeries) -> Option<ScatterGeometryContext> {
        if !series.visible || series.kind != GeneralSeriesKind::Scatter {
            return None;
        }
        let pane_index = self.pane_index_for_id(series.pane_id)?;
        let plot = self.general_plot_rect(pane_index)?;
        let dataset = self.general_dataset(series.dataset)?;
        dataset.numeric_x()?;
        let x_axis = self.general_axis(&series.x_axis_id)?;
        let y_axis = self.general_axis(&series.y_axis_id)?;
        let GeneralAxisDomain::Numeric(x_domain) = self.effective_general_axis_domain(x_axis)?
        else {
            return None;
        };
        let GeneralAxisDomain::Numeric(y_domain) = self.effective_general_axis_domain(y_axis)?
        else {
            return None;
        };
        let x_range = if x_axis.reverse() {
            (plot.width, 0.0)
        } else {
            (0.0, plot.width)
        };
        let plot_bottom = plot.y + plot.height;
        let y_range = if y_axis.reverse() {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let x_scale = NumericAxisScale::new(x_axis.scale(), x_domain, x_range.0, x_range.1)?;
        let y_scale = NumericAxisScale::new(y_axis.scale(), y_domain, y_range.0, y_range.1)?;
        Some(ScatterGeometryContext {
            key: ScatterGeometryKey {
                dataset_generation: dataset.generation(),
                plot_width: plot.width.to_bits(),
                plot_y: plot.y.to_bits(),
                plot_height: plot.height.to_bits(),
                x_domain: x_domain.map(f64::to_bits),
                y_domain: y_domain.map(f64::to_bits),
                x_scale: x_axis.scale(),
                y_scale: y_axis.scale(),
                x_reverse: x_axis.reverse(),
                y_reverse: y_axis.reverse(),
                radius: series.point_radius.to_bits(),
            },
            x_scale,
            y_scale,
            plot_width: plot.width,
            plot_y: plot.y,
            plot_bottom,
            radius: series.point_radius,
        })
    }

    fn build_scatter_spatial_index(
        &self,
        series: &GeneralSeries,
        context: ScatterGeometryContext,
    ) -> Option<ScatterSpatialIndex> {
        let dataset = self.general_dataset(series.dataset)?;
        let x_values = dataset.numeric_x()?;
        let mut points = Vec::with_capacity(dataset.len());
        for (row, (&x_value, &y_value)) in x_values.iter().zip(dataset.y()).enumerate() {
            if !dataset.y_is_valid(row) {
                continue;
            }
            let (Some(x), Some(y)) = (
                context.x_scale.coordinate(x_value),
                context.y_scale.coordinate(y_value),
            ) else {
                continue;
            };
            if x < -context.radius
                || x > context.plot_width + context.radius
                || y < context.plot_y - context.radius
                || y > context.plot_bottom + context.radius
            {
                continue;
            }
            points.push(GeneralScatterGeometry {
                row,
                x,
                y,
                radius: context.radius,
            });
        }
        Some(ScatterSpatialIndex::new(
            context.key,
            context.plot_y,
            context.plot_width,
            context.plot_bottom - context.plot_y,
            points,
        ))
    }

    fn with_scatter_spatial_index<R, F>(&self, series: &GeneralSeries, use_index: F) -> Option<R>
    where
        F: FnOnce(&ScatterSpatialIndex) -> R,
    {
        let context = self.scatter_geometry_context(series)?;
        let registry = self.general_series.as_ref()?;
        let current = registry
            .scatter_spatial
            .borrow()
            .get(&series.id)
            .is_some_and(|index| index.key == context.key);
        if !current {
            let index = self.build_scatter_spatial_index(series, context)?;
            registry
                .scatter_spatial
                .borrow_mut()
                .insert(series.id, index);
        }
        let cache = registry.scatter_spatial.borrow();
        let index = cache.get(&series.id)?;
        Some(use_index(index))
    }

    pub(crate) fn visit_general_scatter_points<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralScatterGeometry),
    {
        let _ = self.with_scatter_spatial_index(series, |index| {
            for &point in &index.points {
                visit(point);
            }
        });
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
            let mut consider = |row: usize, distance: f64| {
                if distance > max_distance {
                    return;
                }
                if best
                    .as_ref()
                    .is_some_and(|current| distance >= current.distance)
                {
                    return;
                }
                let Some(row_id) = dataset.row_identity(row).cloned() else {
                    return;
                };
                best = Some(GeneralSeriesHit {
                    series: series.id,
                    row,
                    row_id,
                    distance,
                });
            };
            match series.kind {
                GeneralSeriesKind::Column => self.visit_general_columns(series, |geometry| {
                    consider(geometry.row, distance_to_rect(x_css, y_css, geometry));
                }),
                GeneralSeriesKind::Scatter => {
                    let expansion = series.point_radius + max_distance;
                    let _ = self.with_scatter_spatial_index(series, |index| {
                        index.visit_candidates(x_css, y_css, expansion, |geometry| {
                            consider(geometry.row, distance_to_circle(x_css, y_css, geometry));
                        });
                    });
                }
            }
            if matches!(mode, GeneralHitMode::Exact)
                && best.as_ref().is_some_and(|hit| hit.distance == 0.0)
            {
                break;
            }
        }
        best
    }

    #[doc(hidden)]
    pub fn update_general_hover(
        &mut self,
        pane_index: usize,
        x_css: f64,
        y_css: f64,
    ) -> Option<GeneralSeriesHit> {
        let hit = self.general_hit_test(pane_index, x_css, y_css, GeneralHitMode::Exact);
        let next = hit.as_ref().map(GeneralInteractionTarget::from);
        let changed = self
            .general_series
            .as_ref()
            .is_some_and(|registry| registry.hovered != next);
        if let Some(registry) = self.general_series.as_mut() {
            registry.hovered = next;
        }
        if changed {
            self.invalidate_frame_overlay();
        }
        hit
    }

    #[doc(hidden)]
    pub fn clear_general_hover(&mut self) {
        let changed = self
            .general_series
            .as_mut()
            .is_some_and(|registry| registry.hovered.take().is_some());
        if changed {
            self.invalidate_frame_overlay();
        }
    }

    #[doc(hidden)]
    pub fn general_hovered_hit(&self) -> Option<GeneralSeriesHit> {
        self.general_series
            .as_ref()?
            .hovered
            .as_ref()
            .map(GeneralInteractionTarget::hit)
    }

    #[doc(hidden)]
    pub fn select_general_hovered(&mut self) -> bool {
        let Some(registry) = self.general_series.as_mut() else {
            return false;
        };
        let next = registry.hovered.clone();
        let hit = next.is_some();
        if registry.selected != next {
            registry.selected = next;
            self.invalidate_frame_overlay();
        }
        hit
    }

    #[doc(hidden)]
    pub fn clear_general_selection(&mut self) {
        let changed = self
            .general_series
            .as_mut()
            .is_some_and(|registry| registry.selected.take().is_some());
        if changed {
            self.invalidate_frame_overlay();
        }
    }

    #[doc(hidden)]
    pub fn general_selected_hit(&self) -> Option<GeneralSeriesHit> {
        self.general_series
            .as_ref()?
            .selected
            .as_ref()
            .map(GeneralInteractionTarget::hit)
    }

    #[doc(hidden)]
    pub fn set_general_accessibility_focus(
        &mut self,
        series_id: GeneralSeriesId,
        row: usize,
    ) -> bool {
        let Some(series) = self.general_series(series_id) else {
            return false;
        };
        let dataset_id = series.dataset;
        let Some(row_id) = self
            .general_dataset(dataset_id)
            .and_then(|dataset| dataset.row_identity(row))
            .cloned()
        else {
            return false;
        };
        let next = Some(GeneralInteractionTarget {
            series: series_id,
            row,
            row_id,
            distance: 0.0,
        });
        let changed = self
            .general_series
            .as_ref()
            .is_some_and(|registry| registry.accessibility_focused != next);
        if let Some(registry) = self.general_series.as_mut() {
            registry.accessibility_focused = next;
        }
        if changed {
            self.invalidate_frame_overlay();
        }
        true
    }

    #[doc(hidden)]
    pub fn clear_general_accessibility_focus(&mut self) {
        let changed = self
            .general_series
            .as_mut()
            .is_some_and(|registry| registry.accessibility_focused.take().is_some());
        if changed {
            self.invalidate_frame_overlay();
        }
    }

    #[doc(hidden)]
    pub fn general_accessibility_focused_hit(&self) -> Option<GeneralSeriesHit> {
        self.general_series
            .as_ref()?
            .accessibility_focused
            .as_ref()
            .map(GeneralInteractionTarget::hit)
    }

    pub(crate) fn general_row_interaction(
        &self,
        series: GeneralSeriesId,
        row: usize,
    ) -> (bool, bool) {
        let Some(registry) = self.general_series.as_ref() else {
            return (false, false);
        };
        let matches =
            |target: &GeneralInteractionTarget| target.series == series && target.row == row;
        (
            registry.hovered.as_ref().is_some_and(matches)
                || registry.accessibility_focused.as_ref().is_some_and(matches),
            registry.selected.as_ref().is_some_and(matches),
        )
    }

    pub(crate) fn reconcile_general_interaction_for_dataset(
        &mut self,
        dataset_id: GeneralDatasetId,
        removed_front: usize,
    ) {
        let Some(registry) = self.general_series.as_ref() else {
            return;
        };
        let dataset_series: Vec<GeneralSeriesId> = registry
            .series
            .iter()
            .filter(|series| series.dataset == dataset_id)
            .map(GeneralSeries::id)
            .collect();
        let previous_hovered = registry.hovered.clone();
        let previous_selected = registry.selected.clone();
        let previous_accessibility_focused = registry.accessibility_focused.clone();
        let Some(dataset) = self.general_dataset(dataset_id) else {
            return;
        };
        let reconcile = |target: Option<GeneralInteractionTarget>| {
            let mut current = target?;
            if dataset_series.contains(&current.series) {
                let shifted = current.row.checked_sub(removed_front);
                current.row = shifted
                    .filter(|&row| dataset.row_identity(row) == Some(&current.row_id))
                    .or_else(|| {
                        (0..dataset.len())
                            .find(|&row| dataset.row_identity(row) == Some(&current.row_id))
                    })?;
            }
            Some(current)
        };
        let next_hovered = reconcile(previous_hovered.clone());
        let next_selected = reconcile(previous_selected.clone());
        let next_accessibility_focused = reconcile(previous_accessibility_focused.clone());
        if previous_hovered != next_hovered
            || previous_selected != next_selected
            || previous_accessibility_focused != next_accessibility_focused
        {
            if let Some(registry) = self.general_series.as_mut() {
                registry.hovered = next_hovered;
                registry.selected = next_selected;
                registry.accessibility_focused = next_accessibility_focused;
            }
            self.invalidate_frame_overlay();
        }
    }

    #[cfg(test)]
    pub(crate) fn general_scatter_hit_candidate_count(
        &self,
        series_id: GeneralSeriesId,
        x_css: f64,
        y_css: f64,
        max_distance: f64,
    ) -> Option<usize> {
        let series = self.general_series(series_id)?;
        if series.kind != GeneralSeriesKind::Scatter
            || !x_css.is_finite()
            || !y_css.is_finite()
            || !max_distance.is_finite()
            || max_distance < 0.0
        {
            return None;
        }
        self.with_scatter_spatial_index(series, |index| {
            let mut count = 0usize;
            index.visit_candidates(x_css, y_css, series.point_radius + max_distance, |_| {
                count += 1
            });
            count
        })
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
        let x_label = general_x_label(dataset, row)?;
        Some(GeneralTooltipSnapshot {
            series: series_id,
            row,
            row_id,
            x_label,
            label: dataset.row_label(row).map(str::to_owned),
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
        let total_rows = dataset.len();
        let offset = offset.min(total_rows);
        let end = offset
            .saturating_add(limit.min(MAX_GENERAL_ACCESSIBILITY_ITEMS))
            .min(total_rows);
        let mut items = Vec::with_capacity(end - offset);
        for row in offset..end {
            let row_id = dataset.row_identity(row)?.clone();
            let x_label = general_x_label(dataset, row)?;
            items.push(GeneralAccessibilityItem {
                row,
                row_id,
                x_label,
                label: dataset.row_label(row).map(str::to_owned),
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

fn distance_to_circle(x: f64, y: f64, geometry: GeneralScatterGeometry) -> f64 {
    ((x - geometry.x).hypot(y - geometry.y) - geometry.radius).max(0.0)
}

fn general_x_label(dataset: &crate::GeneralDataset, row: usize) -> Option<String> {
    if let Some(values) = dataset.numeric_x() {
        return values.get(row).map(ToString::to_string);
    }
    if let Some(values) = dataset.temporal_x_epoch_ms() {
        return values.get(row).map(ToString::to_string);
    }
    let category_index = usize::try_from(*dataset.category_indices()?.get(row)?).ok()?;
    dataset.categories()?.get(category_index).cloned()
}

fn validate_dataset_for_series(
    kind: GeneralSeriesKind,
    dataset: &crate::GeneralDataset,
    x_axis: &crate::GeneralAxis,
    y_axis: &crate::GeneralAxis,
) -> Result<(), ChartError> {
    match kind {
        GeneralSeriesKind::Column => {
            if dataset.x_kind() != GeneralXKind::Category {
                return Err(invalid("column series require category X data"));
            }
        }
        GeneralSeriesKind::Scatter => {
            let values = dataset
                .numeric_x()
                .ok_or_else(|| invalid("scatter series require numeric X data"))?;
            if x_axis.scale() == GeneralScaleType::Logarithmic
                && values.iter().any(|value| *value <= 0.0)
            {
                return Err(invalid("logarithmic scatter X values must be positive"));
            }
            if y_axis.scale() == GeneralScaleType::Logarithmic
                && dataset
                    .y()
                    .iter()
                    .enumerate()
                    .any(|(index, value)| dataset.y_is_valid(index) && *value <= 0.0)
            {
                return Err(invalid("logarithmic scatter Y values must be positive"));
            }
        }
    }
    Ok(())
}

fn validate_input_for_series(
    kind: GeneralSeriesKind,
    input: &GeneralXyInput,
    x_axis: &crate::GeneralAxis,
    y_axis: &crate::GeneralAxis,
) -> Result<(), ChartError> {
    match kind {
        GeneralSeriesKind::Column => {
            if input.x_kind() != GeneralXKind::Category {
                return Err(invalid(
                    "a dataset bound to a column series must remain category X data",
                ));
            }
        }
        GeneralSeriesKind::Scatter => {
            let values = input.numeric_x_values().ok_or_else(|| {
                invalid("a dataset bound to a scatter series must remain numeric X data")
            })?;
            if x_axis.scale() == GeneralScaleType::Logarithmic
                && values.iter().any(|value| *value <= 0.0)
            {
                return Err(invalid("logarithmic scatter X values must be positive"));
            }
            if y_axis.scale() == GeneralScaleType::Logarithmic {
                let validity = input.y_valid_values();
                if input.y_values().iter().enumerate().any(|(index, value)| {
                    validity
                        .and_then(|values| values.get(index))
                        .is_none_or(|valid| *valid != 0)
                        && *value <= 0.0
                }) {
                    return Err(invalid("logarithmic scatter Y values must be positive"));
                }
            }
        }
    }
    Ok(())
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
    if options.kind == GeneralSeriesKind::Scatter
        && (!options.point_radius.is_finite()
            || !(MIN_GENERAL_POINT_RADIUS..=MAX_GENERAL_POINT_RADIUS)
                .contains(&options.point_radius))
    {
        return Err(invalid(format!(
            "scatter point radius must be finite and in {MIN_GENERAL_POINT_RADIUS}..={MAX_GENERAL_POINT_RADIUS} CSS px"
        )));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::InvalidOptions, message)
}

fn resource(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::ResourceLimit, message)
}
