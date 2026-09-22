use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;

use nucleuscharts_core::scale::general_scale::{BandScale, LinearScale, PointScale};
use nucleuscharts_render::color::Color;

use crate::general_axes::NumericAxisScale;
use crate::{
    AxisDimension, ChartEngine, ChartError, ErrorCode, GeneralAxisDomain, GeneralDatasetId,
    GeneralRowIdentity, GeneralScaleType, GeneralXKind, GeneralXyInput, HorizontalDomain, PaneId,
};

pub const MAX_GENERAL_SERIES: usize = 1_024;
pub const MAX_GENERAL_SERIES_TITLE_BYTES: usize = 4_096;
pub const MAX_GENERAL_SERIES_COLOR_BYTES: usize = 256;
pub const MAX_GENERAL_SERIES_GROUP_ID_BYTES: usize = 128;
pub const MAX_GENERAL_SERIES_STACK_ID_BYTES: usize = 128;
pub const MAX_GENERAL_ACCESSIBILITY_ITEMS: usize = 512;
pub const MIN_GENERAL_POINT_RADIUS: f64 = 1.0;
pub const MAX_GENERAL_POINT_RADIUS: f64 = 64.0;
const SCATTER_GRID_BASE_CELL_CSS: f64 = 32.0;
const MAX_SCATTER_GRID_CELLS: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GeneralSeriesKind {
    XyLine,
    XyArea,
    RangeArea,
    Column,
    Scatter,
    Bubble,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum GeneralStackMode {
    #[default]
    Normal,
    Percent,
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
    pub group_id: Option<String>,
    pub stack_id: Option<String>,
    pub stack_mode: GeneralStackMode,
}

impl GeneralSeriesOptions {
    pub fn xy_line(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GeneralSeriesKind::XyLine,
            pane,
            dataset,
            x_axis_id: x_axis_id.into(),
            y_axis_id: y_axis_id.into(),
            visible: true,
            title: String::new(),
            color: None,
            point_radius: 3.0,
            data_labels: false,
            group_id: None,
            stack_id: None,
            stack_mode: GeneralStackMode::Normal,
        }
    }

    pub fn xy_area(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GeneralSeriesKind::XyArea,
            pane,
            dataset,
            x_axis_id: x_axis_id.into(),
            y_axis_id: y_axis_id.into(),
            visible: true,
            title: String::new(),
            color: None,
            point_radius: 3.0,
            data_labels: false,
            group_id: None,
            stack_id: None,
            stack_mode: GeneralStackMode::Normal,
        }
    }

    pub fn range_area(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GeneralSeriesKind::RangeArea,
            pane,
            dataset,
            x_axis_id: x_axis_id.into(),
            y_axis_id: y_axis_id.into(),
            visible: true,
            title: String::new(),
            color: None,
            point_radius: 3.0,
            data_labels: false,
            group_id: None,
            stack_id: None,
            stack_mode: GeneralStackMode::Normal,
        }
    }

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
            group_id: None,
            stack_id: None,
            stack_mode: GeneralStackMode::Normal,
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
            group_id: None,
            stack_id: None,
            stack_mode: GeneralStackMode::Normal,
        }
    }

    pub fn bubble(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GeneralSeriesKind::Bubble,
            pane,
            dataset,
            x_axis_id: x_axis_id.into(),
            y_axis_id: y_axis_id.into(),
            visible: true,
            title: String::new(),
            color: None,
            point_radius: 3.0,
            data_labels: false,
            group_id: None,
            stack_id: None,
            stack_mode: GeneralStackMode::Normal,
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
    group_id: Option<String>,
    stack_id: Option<String>,
    stack_mode: GeneralStackMode,
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GeneralLinePointGeometry {
    pub(crate) row: usize,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) starts_new_run: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GeneralRangePointGeometry {
    pub(crate) row: usize,
    pub(crate) x: f64,
    pub(crate) low_y: f64,
    pub(crate) high_y: f64,
    pub(crate) starts_new_run: bool,
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
    pub low: Option<f64>,
    pub high: Option<f64>,
    pub size: Option<f64>,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralAccessibilityItem {
    pub row: usize,
    pub row_id: GeneralRowIdentity,
    pub x_label: String,
    pub label: Option<String>,
    pub value: Option<f64>,
    pub low: Option<f64>,
    pub high: Option<f64>,
    pub size: Option<f64>,
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

    pub fn group_id(&self) -> Option<&str> {
        self.group_id.as_deref()
    }

    pub fn stack_id(&self) -> Option<&str> {
        self.stack_id.as_deref()
    }

    pub fn stack_mode(&self) -> GeneralStackMode {
        self.stack_mode
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.x_axis_id.capacity()
            + self.y_axis_id.capacity()
            + self.title.capacity()
            + self.color.as_ref().map_or(0, String::capacity)
            + self.group_id.as_ref().map_or(0, String::capacity)
            + self.stack_id.as_ref().map_or(0, String::capacity)
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
            group_id: options.group_id,
            stack_id: options.stack_id,
            stack_mode: options.stack_mode,
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
            GeneralSeriesKind::XyLine
            | GeneralSeriesKind::XyArea
            | GeneralSeriesKind::RangeArea => {
                let compatible_x = matches!(
                    (pane_domain, dataset_kind, x_axis.scale()),
                    (
                        HorizontalDomain::Continuous { .. },
                        GeneralXKind::Numeric,
                        GeneralScaleType::Linear
                            | GeneralScaleType::Logarithmic
                            | GeneralScaleType::SymmetricLog
                    ) | (
                        HorizontalDomain::Temporal,
                        GeneralXKind::Temporal,
                        GeneralScaleType::Temporal
                    ) | (
                        HorizontalDomain::Category {
                            scale: crate::CategoryScaleType::Band
                        },
                        GeneralXKind::Category,
                        GeneralScaleType::Band
                    ) | (
                        HorizontalDomain::Category {
                            scale: crate::CategoryScaleType::Point
                        },
                        GeneralXKind::Category,
                        GeneralScaleType::Point
                    )
                );
                if !compatible_x
                    || !matches!(
                        y_axis.scale(),
                        GeneralScaleType::Linear
                            | GeneralScaleType::Logarithmic
                            | GeneralScaleType::SymmetricLog
                    )
                {
                    return Err(invalid(
                        "xy_line/xy_area/range_area requires X data/axis semantics matching its continuous, temporal, or category pane and a numeric Y axis",
                    ));
                }
            }
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
            GeneralSeriesKind::Scatter | GeneralSeriesKind::Bubble => {
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
                        "scatter/bubble requires a continuous pane, numeric X data, and numeric X/Y axes",
                    ));
                }
            }
        }
        validate_dataset_for_series(options.kind, dataset, &x_axis, &y_axis)?;
        validate_presentation(&options)?;
        self.validate_column_layout_compatibility(pane_id, &options)?;
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

    fn validate_column_layout_compatibility(
        &self,
        pane_id: PaneId,
        options: &GeneralSeriesOptions,
    ) -> Result<(), ChartError> {
        if options.kind != GeneralSeriesKind::Column {
            return Ok(());
        }

        if let Some(group_id) = options.group_id.as_deref() {
            for sibling in self.general_series_iter().filter(|series| {
                series.kind == GeneralSeriesKind::Column
                    && series.pane_id == pane_id
                    && series.group_id() == Some(group_id)
            }) {
                if sibling.x_axis_id != options.x_axis_id {
                    return Err(invalid(
                        "grouped column series must share the same category X axis",
                    ));
                }
            }
        }

        if let Some(stack_id) = options.stack_id.as_deref() {
            for sibling in self.general_series_iter().filter(|series| {
                series.kind == GeneralSeriesKind::Column
                    && series.pane_id == pane_id
                    && series.group_id() == options.group_id.as_deref()
                    && series.stack_id() == Some(stack_id)
            }) {
                if sibling.x_axis_id != options.x_axis_id
                    || sibling.y_axis_id != options.y_axis_id
                    || sibling.stack_mode != options.stack_mode
                {
                    return Err(invalid(
                        "stacked column series must share X/Y axes, group ID, and stack mode",
                    ));
                }
            }
        }
        Ok(())
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
        let axis_lookup: HashMap<&str, usize> = axis_categories
            .iter()
            .enumerate()
            .map(|(index, category)| (category.as_str(), index))
            .collect();

        let (slot_index, slot_count) = self.general_column_group_slot(series);
        let mut stack_base = series
            .stack_id
            .is_some()
            .then(|| vec![(0.0_f64, 0.0_f64); axis_categories.len()]);
        let mut stack_totals = (series.stack_mode == GeneralStackMode::Percent)
            .then(|| vec![(0.0_f64, 0.0_f64); axis_categories.len()]);
        if series.stack_id.is_some() {
            if let Some(registry) = self.general_series.as_ref() {
                if let Some(totals) = stack_totals.as_mut() {
                    for sibling in registry.series.iter().filter(|candidate| {
                        candidate.visible && column_stack_matches(series, candidate)
                    }) {
                        accumulate_column_values(
                            self,
                            sibling,
                            &axis_lookup,
                            |axis_index, value| {
                                if value >= 0.0 {
                                    totals[axis_index].0 += value;
                                } else {
                                    totals[axis_index].1 += -value;
                                }
                            },
                        );
                    }
                }
                for sibling in registry.series.iter().filter(|candidate| {
                    candidate.visible && column_stack_matches(series, candidate)
                }) {
                    if sibling.id == series.id {
                        break;
                    }
                    let Some(base) = stack_base.as_mut() else {
                        break;
                    };
                    accumulate_column_values(self, sibling, &axis_lookup, |axis_index, value| {
                        if value >= 0.0 {
                            base[axis_index].0 += value;
                        } else {
                            base[axis_index].1 += value;
                        }
                    });
                }
                if let (Some(base), Some(totals)) = (stack_base.as_mut(), stack_totals.as_ref()) {
                    for (base, total) in base.iter_mut().zip(totals) {
                        if total.0 > 0.0 {
                            base.0 /= total.0;
                        }
                        if total.1 > 0.0 {
                            base.1 /= total.1;
                        }
                    }
                }
            }
        }

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
            let full_left = left.min(right).clamp(0.0, plot.width);
            let full_right = left.max(right).clamp(0.0, plot.width);
            let slot_width = (full_right - full_left) / slot_count.max(1) as f64;
            let left = full_left + slot_width * slot_index as f64;
            let right = if slot_index + 1 == slot_count {
                full_right
            } else {
                left + slot_width
            };
            let raw_value = dataset.y()[row];
            let (from_value, to_value) = if series.stack_id.is_some() {
                let normalized_value = match stack_totals.as_ref() {
                    Some(totals) if raw_value >= 0.0 => {
                        let total = totals[axis_index].0;
                        if total > 0.0 {
                            raw_value / total
                        } else {
                            0.0
                        }
                    }
                    Some(totals) => {
                        let total = totals[axis_index].1;
                        if total > 0.0 {
                            raw_value / total
                        } else {
                            0.0
                        }
                    }
                    None => raw_value,
                };
                let Some(stack_base) = stack_base.as_mut() else {
                    continue;
                };
                let base = if raw_value >= 0.0 {
                    &mut stack_base[axis_index].0
                } else {
                    &mut stack_base[axis_index].1
                };
                let from = *base;
                let to = from + normalized_value;
                *base = to;
                (from, to)
            } else {
                (0.0, raw_value)
            };
            let Some(from_y) = y_scale.coordinate_clamped(from_value) else {
                continue;
            };
            let Some(to_y) = y_scale.coordinate_clamped(to_value) else {
                continue;
            };
            let top = from_y.min(to_y).clamp(plot.y, plot_bottom);
            let bottom = from_y.max(to_y).clamp(plot.y, plot_bottom);
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

    fn general_column_group_slot(&self, series: &GeneralSeries) -> (usize, usize) {
        let Some(group_id) = series.group_id() else {
            return (0, 1);
        };
        let Some(registry) = self.general_series.as_ref() else {
            return (0, 1);
        };
        let mut target_slot = 0usize;
        let mut slot_count = 0usize;
        for (index, sibling) in registry.series.iter().enumerate() {
            if !sibling.visible
                || sibling.kind != GeneralSeriesKind::Column
                || sibling.pane_id != series.pane_id
                || sibling.x_axis_id != series.x_axis_id
                || sibling.group_id() != Some(group_id)
            {
                continue;
            }
            let representative = sibling.stack_id().is_none()
                || !registry.series[..index].iter().any(|previous| {
                    previous.visible
                        && previous.kind == GeneralSeriesKind::Column
                        && previous.pane_id == series.pane_id
                        && previous.x_axis_id == series.x_axis_id
                        && previous.group_id() == Some(group_id)
                        && previous.stack_id() == sibling.stack_id()
                });
            if !representative {
                continue;
            }
            if sibling.id == series.id
                || (series.stack_id().is_some() && sibling.stack_id() == series.stack_id())
            {
                target_slot = slot_count;
            }
            slot_count += 1;
        }
        (target_slot, slot_count.max(1))
    }

    pub(crate) fn general_column_axis_bounds(
        &self,
        pane_id: PaneId,
        y_axis_id: &str,
    ) -> Option<(f64, f64)> {
        let registry = self.general_series.as_ref()?;
        let mut bounds: Option<(f64, f64)> = None;
        let mut processed_stacks = HashSet::new();

        for series in registry.series.iter().filter(|series| {
            series.visible
                && series.kind == GeneralSeriesKind::Column
                && series.pane_id == pane_id
                && series.y_axis_id == y_axis_id
        }) {
            if let Some(stack_id) = series.stack_id() {
                let key = (
                    series.x_axis_id.clone(),
                    series.group_id.clone(),
                    stack_id.to_owned(),
                    series.stack_mode,
                );
                if !processed_stacks.insert(key) {
                    continue;
                }
                let mut category_totals: HashMap<String, (f64, f64)> = HashMap::new();
                for sibling in registry.series.iter().filter(|candidate| {
                    candidate.visible
                        && candidate.y_axis_id == y_axis_id
                        && column_stack_matches(series, candidate)
                }) {
                    let Some(dataset) = self.general_dataset(sibling.dataset) else {
                        continue;
                    };
                    let (Some(categories), Some(indices)) =
                        (dataset.categories(), dataset.category_indices())
                    else {
                        continue;
                    };
                    for (row, &category_index) in indices.iter().enumerate() {
                        if !dataset.y_is_valid(row) {
                            continue;
                        }
                        let Some(category) = usize::try_from(category_index)
                            .ok()
                            .and_then(|index| categories.get(index))
                        else {
                            continue;
                        };
                        let value = dataset.y()[row];
                        let totals = category_totals.entry(category.clone()).or_default();
                        if value >= 0.0 {
                            totals.0 += value;
                        } else {
                            totals.1 += value;
                        }
                    }
                }
                for (positive, negative) in category_totals.into_values() {
                    match series.stack_mode {
                        GeneralStackMode::Normal => {
                            extend_numeric_pair(&mut bounds, positive);
                            extend_numeric_pair(&mut bounds, negative);
                        }
                        GeneralStackMode::Percent => {
                            if positive > 0.0 {
                                extend_numeric_pair(&mut bounds, 1.0);
                            }
                            if negative < 0.0 {
                                extend_numeric_pair(&mut bounds, -1.0);
                            }
                        }
                    }
                }
            } else if let Some(dataset) = self.general_dataset(series.dataset) {
                for (row, &value) in dataset.y().iter().enumerate() {
                    if dataset.y_is_valid(row) {
                        extend_numeric_pair(&mut bounds, value);
                    }
                }
            }
        }
        if bounds.is_some() {
            extend_numeric_pair(&mut bounds, 0.0);
        }
        bounds
    }

    pub(crate) fn visit_general_path_points<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralLinePointGeometry),
    {
        if !series.visible
            || !matches!(
                series.kind,
                GeneralSeriesKind::XyLine | GeneralSeriesKind::XyArea
            )
        {
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
        let (Some(x_axis), Some(y_axis)) = (
            self.general_axis(&series.x_axis_id),
            self.general_axis(&series.y_axis_id),
        ) else {
            return;
        };
        let Some(x_domain) = self.effective_general_axis_domain(x_axis) else {
            return;
        };
        let Some(GeneralAxisDomain::Numeric(y_domain)) = self.effective_general_axis_domain(y_axis)
        else {
            return;
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
        let Some(y_scale) = NumericAxisScale::new(y_axis.scale(), y_domain, y_range.0, y_range.1)
        else {
            return;
        };
        let numeric_x_scale = match &x_domain {
            GeneralAxisDomain::Numeric(domain) => {
                NumericAxisScale::new(x_axis.scale(), *domain, x_range.0, x_range.1)
            }
            _ => None,
        };
        let temporal_x_scale = match &x_domain {
            GeneralAxisDomain::Temporal([from, to]) => {
                LinearScale::new(*from as f64, *to as f64, x_range.0, x_range.1).ok()
            }
            _ => None,
        };
        let category_lookup: Option<HashMap<&str, usize>> = match &x_domain {
            GeneralAxisDomain::Category(values) => Some(
                values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| (value.as_str(), index))
                    .collect(),
            ),
            _ => None,
        };
        let category_band_scale = match &x_domain {
            GeneralAxisDomain::Category(values) if x_axis.scale() == GeneralScaleType::Band => {
                BandScale::new(
                    values.len(),
                    x_range.0,
                    x_range.1,
                    x_axis.band_padding_inner(),
                    x_axis.band_padding_outer(),
                    0.5,
                )
                .ok()
            }
            _ => None,
        };
        let category_point_scale = match &x_domain {
            GeneralAxisDomain::Category(values) if x_axis.scale() == GeneralScaleType::Point => {
                PointScale::new(
                    values.len(),
                    x_range.0,
                    x_range.1,
                    x_axis.band_padding_outer(),
                    0.5,
                )
                .ok()
            }
            _ => None,
        };

        let mut starts_new_run = true;
        for row in 0..dataset.len() {
            if !dataset.y_is_valid(row) {
                starts_new_run = true;
                continue;
            }
            let Some(y) = y_scale.coordinate(dataset.y()[row]) else {
                starts_new_run = true;
                continue;
            };
            let x = match dataset.x_kind() {
                GeneralXKind::Numeric => dataset
                    .numeric_x()
                    .and_then(|values| values.get(row))
                    .and_then(|value| numeric_x_scale.and_then(|scale| scale.coordinate(*value))),
                GeneralXKind::Temporal => dataset
                    .temporal_x_epoch_ms()
                    .and_then(|values| values.get(row))
                    .and_then(|value| {
                        temporal_x_scale
                            .as_ref()
                            .and_then(|scale| scale.coordinate(*value as f64))
                    }),
                GeneralXKind::Category => {
                    let axis_index = dataset
                        .category_indices()
                        .and_then(|values| values.get(row))
                        .and_then(|value| usize::try_from(*value).ok())
                        .and_then(|index| dataset.categories().and_then(|values| values.get(index)))
                        .and_then(|category| {
                            category_lookup
                                .as_ref()
                                .and_then(|lookup| lookup.get(category.as_str()))
                                .copied()
                        });
                    axis_index.and_then(|index| match x_axis.scale() {
                        GeneralScaleType::Band => category_band_scale
                            .as_ref()
                            .and_then(|scale| scale.center(index)),
                        GeneralScaleType::Point => category_point_scale
                            .as_ref()
                            .and_then(|scale| scale.coordinate(index)),
                        _ => None,
                    })
                }
            };
            let Some(x) = x else {
                starts_new_run = true;
                continue;
            };
            if !x.is_finite() || !y.is_finite() {
                starts_new_run = true;
                continue;
            }
            visit(GeneralLinePointGeometry {
                row,
                x,
                y,
                starts_new_run,
            });
            starts_new_run = false;
        }
    }

    pub(crate) fn visit_general_range_points<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralRangePointGeometry),
    {
        if !series.visible || series.kind != GeneralSeriesKind::RangeArea {
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
        let Some(low_values) = dataset.low() else {
            return;
        };
        let (Some(x_axis), Some(y_axis)) = (
            self.general_axis(&series.x_axis_id),
            self.general_axis(&series.y_axis_id),
        ) else {
            return;
        };
        let Some(x_domain) = self.effective_general_axis_domain(x_axis) else {
            return;
        };
        let Some(GeneralAxisDomain::Numeric(y_domain)) = self.effective_general_axis_domain(y_axis)
        else {
            return;
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
        let Some(y_scale) = NumericAxisScale::new(y_axis.scale(), y_domain, y_range.0, y_range.1)
        else {
            return;
        };
        let numeric_x_scale = match &x_domain {
            GeneralAxisDomain::Numeric(domain) => {
                NumericAxisScale::new(x_axis.scale(), *domain, x_range.0, x_range.1)
            }
            _ => None,
        };
        let temporal_x_scale = match &x_domain {
            GeneralAxisDomain::Temporal([from, to]) => {
                LinearScale::new(*from as f64, *to as f64, x_range.0, x_range.1).ok()
            }
            _ => None,
        };
        let category_lookup: Option<HashMap<&str, usize>> = match &x_domain {
            GeneralAxisDomain::Category(values) => Some(
                values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| (value.as_str(), index))
                    .collect(),
            ),
            _ => None,
        };
        let category_band_scale = match &x_domain {
            GeneralAxisDomain::Category(values) if x_axis.scale() == GeneralScaleType::Band => {
                BandScale::new(
                    values.len(),
                    x_range.0,
                    x_range.1,
                    x_axis.band_padding_inner(),
                    x_axis.band_padding_outer(),
                    0.5,
                )
                .ok()
            }
            _ => None,
        };
        let category_point_scale = match &x_domain {
            GeneralAxisDomain::Category(values) if x_axis.scale() == GeneralScaleType::Point => {
                PointScale::new(
                    values.len(),
                    x_range.0,
                    x_range.1,
                    x_axis.band_padding_outer(),
                    0.5,
                )
                .ok()
            }
            _ => None,
        };

        let mut starts_new_run = true;
        for (row, &low) in low_values.iter().enumerate().take(dataset.len()) {
            if !dataset.y_is_valid(row) || !dataset.low_is_valid(row) {
                starts_new_run = true;
                continue;
            }
            let Some(high_y) = y_scale.coordinate(dataset.y()[row]) else {
                starts_new_run = true;
                continue;
            };
            let Some(low_y) = y_scale.coordinate(low) else {
                starts_new_run = true;
                continue;
            };
            let x = match dataset.x_kind() {
                GeneralXKind::Numeric => dataset
                    .numeric_x()
                    .and_then(|values| values.get(row))
                    .and_then(|value| numeric_x_scale.and_then(|scale| scale.coordinate(*value))),
                GeneralXKind::Temporal => dataset
                    .temporal_x_epoch_ms()
                    .and_then(|values| values.get(row))
                    .and_then(|value| {
                        temporal_x_scale
                            .as_ref()
                            .and_then(|scale| scale.coordinate(*value as f64))
                    }),
                GeneralXKind::Category => {
                    let axis_index = dataset
                        .category_indices()
                        .and_then(|values| values.get(row))
                        .and_then(|value| usize::try_from(*value).ok())
                        .and_then(|index| dataset.categories().and_then(|values| values.get(index)))
                        .and_then(|category| {
                            category_lookup
                                .as_ref()
                                .and_then(|lookup| lookup.get(category.as_str()))
                                .copied()
                        });
                    axis_index.and_then(|index| match x_axis.scale() {
                        GeneralScaleType::Band => category_band_scale
                            .as_ref()
                            .and_then(|scale| scale.center(index)),
                        GeneralScaleType::Point => category_point_scale
                            .as_ref()
                            .and_then(|scale| scale.coordinate(index)),
                        _ => None,
                    })
                }
            };
            let Some(x) = x else {
                starts_new_run = true;
                continue;
            };
            if !x.is_finite() || !low_y.is_finite() || !high_y.is_finite() {
                starts_new_run = true;
                continue;
            }
            visit(GeneralRangePointGeometry {
                row,
                x,
                low_y,
                high_y,
                starts_new_run,
            });
            starts_new_run = false;
        }
    }

    pub(crate) fn general_path_baseline_y(&self, series: &GeneralSeries) -> Option<f64> {
        if !series.visible
            || !matches!(
                series.kind,
                GeneralSeriesKind::XyLine | GeneralSeriesKind::XyArea
            )
        {
            return None;
        }
        let pane_index = self.pane_index_for_id(series.pane_id)?;
        let plot = self.general_plot_rect(pane_index)?;
        let y_axis = self.general_axis(&series.y_axis_id)?;
        let GeneralAxisDomain::Numeric(y_domain) = self.effective_general_axis_domain(y_axis)?
        else {
            return None;
        };
        let plot_bottom = plot.y + plot.height;
        let y_range = if y_axis.reverse() {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let scale = NumericAxisScale::new(y_axis.scale(), y_domain, y_range.0, y_range.1)?;
        Some(
            scale
                .coordinate(0.0)
                .unwrap_or(y_range.0)
                .clamp(plot.y, plot_bottom),
        )
    }

    fn scatter_geometry_context(&self, series: &GeneralSeries) -> Option<ScatterGeometryContext> {
        if !series.visible
            || !matches!(
                series.kind,
                GeneralSeriesKind::Scatter | GeneralSeriesKind::Bubble
            )
        {
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
        let radius = if series.kind == GeneralSeriesKind::Bubble {
            MAX_GENERAL_POINT_RADIUS
        } else {
            series.point_radius
        };
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
                radius: radius.to_bits(),
            },
            x_scale,
            y_scale,
            plot_width: plot.width,
            plot_y: plot.y,
            plot_bottom,
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
            let radius = match series.kind {
                GeneralSeriesKind::Scatter => series.point_radius,
                GeneralSeriesKind::Bubble => {
                    let size = dataset.size()?.get(row).copied()?;
                    if !dataset.size_is_valid(row) || size <= 0.0 {
                        continue;
                    }
                    size.sqrt()
                        .clamp(MIN_GENERAL_POINT_RADIUS, MAX_GENERAL_POINT_RADIUS)
                }
                _ => return None,
            };
            let (Some(x), Some(y)) = (
                context.x_scale.coordinate(x_value),
                context.y_scale.coordinate(y_value),
            ) else {
                continue;
            };
            if x < -radius
                || x > context.plot_width + radius
                || y < context.plot_y - radius
                || y > context.plot_bottom + radius
            {
                continue;
            }
            points.push(GeneralScatterGeometry { row, x, y, radius });
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
                GeneralSeriesKind::XyLine => {
                    let mut previous: Option<GeneralLinePointGeometry> = None;
                    self.visit_general_path_points(series, |geometry| {
                        if geometry.starts_new_run {
                            previous = Some(geometry);
                            return;
                        }
                        let Some(from) = previous else {
                            previous = Some(geometry);
                            return;
                        };
                        let (distance, position) = distance_to_segment(
                            x_css, y_css, from.x, from.y, geometry.x, geometry.y,
                        );
                        let row = if position <= 0.5 {
                            from.row
                        } else {
                            geometry.row
                        };
                        consider(row, (distance - 3.0).max(0.0));
                        previous = Some(geometry);
                    });
                }
                GeneralSeriesKind::XyArea => {
                    let Some(baseline_y) = self.general_path_baseline_y(series) else {
                        continue;
                    };
                    let mut previous: Option<GeneralLinePointGeometry> = None;
                    self.visit_general_path_points(series, |geometry| {
                        if geometry.starts_new_run {
                            previous = Some(geometry);
                            return;
                        }
                        let Some(from) = previous else {
                            previous = Some(geometry);
                            return;
                        };
                        let (distance, position) = distance_to_area_segment(
                            x_css, y_css, from.x, from.y, geometry.x, geometry.y, baseline_y,
                        );
                        let row = if position <= 0.5 {
                            from.row
                        } else {
                            geometry.row
                        };
                        consider(row, distance);
                        previous = Some(geometry);
                    });
                }
                GeneralSeriesKind::RangeArea => {
                    let mut previous: Option<GeneralRangePointGeometry> = None;
                    self.visit_general_range_points(series, |geometry| {
                        if geometry.starts_new_run {
                            previous = Some(geometry);
                            return;
                        }
                        let Some(from) = previous else {
                            previous = Some(geometry);
                            return;
                        };
                        let (distance, position) = distance_to_band_segment(
                            x_css,
                            y_css,
                            from.x,
                            from.low_y,
                            from.high_y,
                            geometry.x,
                            geometry.low_y,
                            geometry.high_y,
                        );
                        let row = if position <= 0.5 {
                            from.row
                        } else {
                            geometry.row
                        };
                        consider(row, distance);
                        previous = Some(geometry);
                    });
                }
                GeneralSeriesKind::Column => self.visit_general_columns(series, |geometry| {
                    consider(geometry.row, distance_to_rect(x_css, y_css, geometry));
                }),
                GeneralSeriesKind::Scatter | GeneralSeriesKind::Bubble => {
                    let expansion = if series.kind == GeneralSeriesKind::Bubble {
                        MAX_GENERAL_POINT_RADIUS
                    } else {
                        series.point_radius
                    } + max_distance;
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
        if !matches!(
            series.kind,
            GeneralSeriesKind::Scatter | GeneralSeriesKind::Bubble
        ) || !x_css.is_finite()
            || !y_css.is_finite()
            || !max_distance.is_finite()
            || max_distance < 0.0
        {
            return None;
        }
        self.with_scatter_spatial_index(series, |index| {
            let mut count = 0usize;
            let expansion = if series.kind == GeneralSeriesKind::Bubble {
                MAX_GENERAL_POINT_RADIUS
            } else {
                series.point_radius
            } + max_distance;
            index.visit_candidates(x_css, y_css, expansion, |_| count += 1);
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
            low: dataset
                .low()
                .and_then(|values| dataset.low_is_valid(row).then(|| values[row])),
            high: dataset
                .low()
                .and_then(|_| dataset.y_is_valid(row).then(|| dataset.y()[row])),
            size: dataset
                .size()
                .and_then(|values| dataset.size_is_valid(row).then(|| values[row])),
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
                low: dataset
                    .low()
                    .and_then(|values| dataset.low_is_valid(row).then(|| values[row])),
                high: dataset
                    .low()
                    .and_then(|_| dataset.y_is_valid(row).then(|| dataset.y()[row])),
                size: dataset
                    .size()
                    .and_then(|values| dataset.size_is_valid(row).then(|| values[row])),
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

fn column_stack_matches(reference: &GeneralSeries, candidate: &GeneralSeries) -> bool {
    reference.kind == GeneralSeriesKind::Column
        && candidate.kind == GeneralSeriesKind::Column
        && reference.pane_id == candidate.pane_id
        && reference.x_axis_id == candidate.x_axis_id
        && reference.y_axis_id == candidate.y_axis_id
        && reference.group_id == candidate.group_id
        && reference.stack_id.is_some()
        && reference.stack_id == candidate.stack_id
        && reference.stack_mode == candidate.stack_mode
}

fn accumulate_column_values<F>(
    engine: &ChartEngine,
    series: &GeneralSeries,
    axis_lookup: &HashMap<&str, usize>,
    mut visit: F,
) where
    F: FnMut(usize, f64),
{
    let Some(dataset) = engine.general_dataset(series.dataset) else {
        return;
    };
    let (Some(categories), Some(indices)) = (dataset.categories(), dataset.category_indices())
    else {
        return;
    };
    for (row, &category_index) in indices.iter().enumerate() {
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
        visit(axis_index, dataset.y()[row]);
    }
}

fn extend_numeric_pair(bounds: &mut Option<(f64, f64)>, value: f64) {
    *bounds = Some(match *bounds {
        Some((low, high)) => (low.min(value), high.max(value)),
        None => (value, value),
    });
}

fn distance_to_circle(x: f64, y: f64, geometry: GeneralScatterGeometry) -> f64 {
    ((x - geometry.x).hypot(y - geometry.y) - geometry.radius).max(0.0)
}

fn distance_to_segment(x: f64, y: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let length_squared = dx * dx + dy * dy;
    if !length_squared.is_finite() || length_squared <= f64::EPSILON {
        return ((x - x0).hypot(y - y0), 0.0);
    }
    let position = (((x - x0) * dx + (y - y0) * dy) / length_squared).clamp(0.0, 1.0);
    let nearest_x = x0 + dx * position;
    let nearest_y = y0 + dy * position;
    ((x - nearest_x).hypot(y - nearest_y), position)
}

fn distance_to_area_segment(
    x: f64,
    y: f64,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    baseline_y: f64,
) -> (f64, f64) {
    let (_, projected) = distance_to_segment(x, y, x0, y0, x1, y1);
    let position = if (x1 - x0).abs() > f64::EPSILON {
        ((x - x0) / (x1 - x0)).clamp(0.0, 1.0)
    } else {
        projected
    };
    let top_y = y0 + (y1 - y0) * position;
    if x >= x0.min(x1)
        && x <= x0.max(x1)
        && y >= top_y.min(baseline_y)
        && y <= top_y.max(baseline_y)
    {
        return (0.0, position);
    }
    let distance = [
        distance_to_segment(x, y, x0, y0, x1, y1).0,
        distance_to_segment(x, y, x0, baseline_y, x1, baseline_y).0,
        distance_to_segment(x, y, x0, y0, x0, baseline_y).0,
        distance_to_segment(x, y, x1, y1, x1, baseline_y).0,
    ]
    .into_iter()
    .fold(f64::INFINITY, f64::min);
    (distance, position)
}

#[allow(clippy::too_many_arguments)]
fn distance_to_band_segment(
    x: f64,
    y: f64,
    x0: f64,
    low0: f64,
    high0: f64,
    x1: f64,
    low1: f64,
    high1: f64,
) -> (f64, f64) {
    let (_, projected) = distance_to_segment(x, y, x0, high0, x1, high1);
    let position = if (x1 - x0).abs() > f64::EPSILON {
        ((x - x0) / (x1 - x0)).clamp(0.0, 1.0)
    } else {
        projected
    };
    let low_y = low0 + (low1 - low0) * position;
    let high_y = high0 + (high1 - high0) * position;
    if x >= x0.min(x1) && x <= x0.max(x1) && y >= low_y.min(high_y) && y <= low_y.max(high_y) {
        return (0.0, position);
    }
    let distance = [
        distance_to_segment(x, y, x0, low0, x1, low1).0,
        distance_to_segment(x, y, x0, high0, x1, high1).0,
        distance_to_segment(x, y, x0, low0, x0, high0).0,
        distance_to_segment(x, y, x1, low1, x1, high1).0,
    ]
    .into_iter()
    .fold(f64::INFINITY, f64::min);
    (distance, position)
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
        GeneralSeriesKind::XyLine | GeneralSeriesKind::XyArea => {
            let expected_x = match x_axis.scale() {
                GeneralScaleType::Linear
                | GeneralScaleType::Logarithmic
                | GeneralScaleType::SymmetricLog => GeneralXKind::Numeric,
                GeneralScaleType::Temporal => GeneralXKind::Temporal,
                GeneralScaleType::Band | GeneralScaleType::Point => GeneralXKind::Category,
                GeneralScaleType::RadialLinear | GeneralScaleType::AngularCategory => {
                    return Err(invalid("general path X axis scale is incompatible"));
                }
            };
            if dataset.x_kind() != expected_x {
                return Err(invalid(
                    "general path dataset X kind must match its bound X axis",
                ));
            }
        }
        GeneralSeriesKind::RangeArea => {
            let expected_x = match x_axis.scale() {
                GeneralScaleType::Linear
                | GeneralScaleType::Logarithmic
                | GeneralScaleType::SymmetricLog => GeneralXKind::Numeric,
                GeneralScaleType::Temporal => GeneralXKind::Temporal,
                GeneralScaleType::Band | GeneralScaleType::Point => GeneralXKind::Category,
                GeneralScaleType::RadialLinear | GeneralScaleType::AngularCategory => {
                    return Err(invalid("range-area X axis scale is incompatible"));
                }
            };
            if dataset.x_kind() != expected_x {
                return Err(invalid(
                    "range-area dataset X kind must match its bound X axis",
                ));
            }
            dataset
                .low()
                .ok_or_else(|| invalid("range-area series require a low-value channel"))?;
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
            validate_logarithmic_general_y(dataset, y_axis)?;
        }
        GeneralSeriesKind::Bubble => {
            dataset
                .numeric_x()
                .ok_or_else(|| invalid("bubble series require numeric X data"))?;
            dataset
                .size()
                .ok_or_else(|| invalid("bubble series require a size channel"))?;
        }
    }
    Ok(())
}

fn validate_logarithmic_general_y(
    dataset: &crate::GeneralDataset,
    y_axis: &crate::GeneralAxis,
) -> Result<(), ChartError> {
    if y_axis.scale() == GeneralScaleType::Logarithmic
        && dataset
            .y()
            .iter()
            .enumerate()
            .any(|(index, value)| dataset.y_is_valid(index) && *value <= 0.0)
    {
        return Err(invalid(
            "logarithmic general-series Y values must be positive",
        ));
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
        GeneralSeriesKind::XyLine | GeneralSeriesKind::XyArea => {
            let expected_x = match x_axis.scale() {
                GeneralScaleType::Linear
                | GeneralScaleType::Logarithmic
                | GeneralScaleType::SymmetricLog => GeneralXKind::Numeric,
                GeneralScaleType::Temporal => GeneralXKind::Temporal,
                GeneralScaleType::Band | GeneralScaleType::Point => GeneralXKind::Category,
                GeneralScaleType::RadialLinear | GeneralScaleType::AngularCategory => {
                    return Err(invalid("general path X axis scale is incompatible"));
                }
            };
            if input.x_kind() != expected_x {
                return Err(invalid(
                    "a dataset bound to a general path series must keep the X kind required by its X axis",
                ));
            }
        }
        GeneralSeriesKind::RangeArea => {
            let expected_x = match x_axis.scale() {
                GeneralScaleType::Linear
                | GeneralScaleType::Logarithmic
                | GeneralScaleType::SymmetricLog => GeneralXKind::Numeric,
                GeneralScaleType::Temporal => GeneralXKind::Temporal,
                GeneralScaleType::Band | GeneralScaleType::Point => GeneralXKind::Category,
                GeneralScaleType::RadialLinear | GeneralScaleType::AngularCategory => {
                    return Err(invalid("range-area X axis scale is incompatible"));
                }
            };
            if input.x_kind() != expected_x {
                return Err(invalid(
                    "a dataset bound to a range-area series must keep the X kind required by its X axis",
                ));
            }
            input.low_values().ok_or_else(|| {
                invalid("a dataset bound to a range-area series must retain its low-value channel")
            })?;
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
            validate_logarithmic_input_y(input, y_axis)?;
        }
        GeneralSeriesKind::Bubble => {
            input.numeric_x_values().ok_or_else(|| {
                invalid("a dataset bound to a bubble series must remain numeric X data")
            })?;
            input.size_values().ok_or_else(|| {
                invalid("a dataset bound to a bubble series must retain its size channel")
            })?;
        }
    }
    Ok(())
}

fn validate_logarithmic_input_y(
    input: &GeneralXyInput,
    y_axis: &crate::GeneralAxis,
) -> Result<(), ChartError> {
    if y_axis.scale() == GeneralScaleType::Logarithmic {
        let validity = input.y_valid_values();
        if input.y_values().iter().enumerate().any(|(index, value)| {
            validity
                .and_then(|values| values.get(index))
                .is_none_or(|valid| *valid != 0)
                && *value <= 0.0
        }) {
            return Err(invalid(
                "logarithmic general-series Y values must be positive",
            ));
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
    if options
        .group_id
        .as_ref()
        .is_some_and(|value| value.len() > MAX_GENERAL_SERIES_GROUP_ID_BYTES)
    {
        return Err(resource(format!(
            "general series group ID exceeds {MAX_GENERAL_SERIES_GROUP_ID_BYTES} UTF-8 bytes"
        )));
    }
    if options
        .stack_id
        .as_ref()
        .is_some_and(|value| value.len() > MAX_GENERAL_SERIES_STACK_ID_BYTES)
    {
        return Err(resource(format!(
            "general series stack ID exceeds {MAX_GENERAL_SERIES_STACK_ID_BYTES} UTF-8 bytes"
        )));
    }
    if options.kind != GeneralSeriesKind::Column
        && (options.group_id.is_some()
            || options.stack_id.is_some()
            || options.stack_mode != GeneralStackMode::Normal)
    {
        return Err(invalid(
            "grouping and stacking options are currently supported only by column series",
        ));
    }
    if options.stack_id.is_none() && options.stack_mode != GeneralStackMode::Normal {
        return Err(invalid("percent stack mode requires a stack ID"));
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
