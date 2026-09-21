use std::collections::HashSet;
use std::num::NonZeroU32;

use crate::{
    CategoryScaleType, ChartEngine, ChartError, ContinuousScaleType, ErrorCode, HorizontalDomain,
    PaneId,
};

pub const MAX_GENERAL_AXES: usize = 128;
pub const MAX_GENERAL_AXIS_ID_BYTES: usize = 128;
pub const MAX_GENERAL_AXIS_TITLE_BYTES: usize = 4_096;
pub const MAX_GENERAL_AXIS_TICKS: u16 = 512;
pub const MAX_GENERAL_AXIS_CATEGORIES: usize = 65_536;
pub const MAX_GENERAL_AXIS_CATEGORY_BYTES: usize = 1_048_576;
pub const MAX_GENERAL_TEMPORAL_MILLISECONDS: i64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisDimension {
    X,
    Y,
    Angle,
    Radius,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisPosition {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneralScaleType {
    Linear,
    Logarithmic,
    SymmetricLog,
    Temporal,
    Band,
    Point,
    RadialLinear,
    AngularCategory,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum GeneralAxisDomain {
    #[default]
    Auto,
    Numeric([f64; 2]),
    Temporal([i64; 2]),
    Category(Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralAxisOptions {
    pub id: String,
    pub pane: usize,
    pub dimension: AxisDimension,
    pub position: Option<AxisPosition>,
    pub scale: GeneralScaleType,
    pub domain: GeneralAxisDomain,
    pub reverse: bool,
    pub visible: bool,
    pub title: Option<String>,
    pub tick_count: Option<u16>,
    pub min_tick_gap: f64,
    pub band_padding_inner: f64,
    pub band_padding_outer: f64,
    pub zero_line: bool,
    pub grid_visible: bool,
}

impl GeneralAxisOptions {
    pub fn new(
        id: impl Into<String>,
        pane: usize,
        dimension: AxisDimension,
        scale: GeneralScaleType,
    ) -> Self {
        Self {
            id: id.into(),
            pane,
            dimension,
            position: None,
            scale,
            domain: GeneralAxisDomain::Auto,
            reverse: false,
            visible: true,
            title: None,
            tick_count: None,
            min_tick_gap: 4.0,
            band_padding_inner: 0.1,
            band_padding_outer: 0.1,
            zero_line: true,
            grid_visible: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GeneralAxisHandle(NonZeroU32);

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralAxis {
    handle: GeneralAxisHandle,
    id: String,
    pane_id: PaneId,
    dimension: AxisDimension,
    position: Option<AxisPosition>,
    scale: GeneralScaleType,
    domain: GeneralAxisDomain,
    reverse: bool,
    visible: bool,
    title: Option<String>,
    tick_count: Option<u16>,
    min_tick_gap: f64,
    band_padding_inner: f64,
    band_padding_outer: f64,
    zero_line: bool,
    grid_visible: bool,
}

impl GeneralAxis {
    #[cfg(test)]
    pub(crate) fn handle(&self) -> GeneralAxisHandle {
        self.handle
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn pane_id(&self) -> PaneId {
        self.pane_id
    }

    pub fn dimension(&self) -> AxisDimension {
        self.dimension
    }

    pub fn position(&self) -> Option<AxisPosition> {
        self.position
    }

    pub fn scale(&self) -> GeneralScaleType {
        self.scale
    }

    pub fn domain(&self) -> &GeneralAxisDomain {
        &self.domain
    }

    pub fn reverse(&self) -> bool {
        self.reverse
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn tick_count(&self) -> Option<u16> {
        self.tick_count
    }

    pub fn min_tick_gap(&self) -> f64 {
        self.min_tick_gap
    }

    pub fn band_padding_inner(&self) -> f64 {
        self.band_padding_inner
    }

    pub fn band_padding_outer(&self) -> f64 {
        self.band_padding_outer
    }

    pub fn zero_line(&self) -> bool {
        self.zero_line
    }

    pub fn grid_visible(&self) -> bool {
        self.grid_visible
    }

    fn estimated_bytes(&self) -> usize {
        self.id.capacity()
            + self.title.as_ref().map_or(0, String::capacity)
            + match &self.domain {
                GeneralAxisDomain::Category(values) => {
                    values.capacity() * std::mem::size_of::<String>()
                        + values.iter().map(String::capacity).sum::<usize>()
                }
                _ => 0,
            }
    }
}

pub(crate) struct GeneralAxisRegistry {
    axes: Vec<GeneralAxis>,
    next_handle: u32,
}

impl GeneralAxisRegistry {
    pub(crate) fn new() -> Self {
        Self {
            axes: Vec::new(),
            next_handle: 1,
        }
    }

    fn insert(
        &mut self,
        pane_id: PaneId,
        options: GeneralAxisOptions,
    ) -> Result<GeneralAxisHandle, ChartError> {
        if self.axes.len() >= MAX_GENERAL_AXES {
            return Err(resource(format!(
                "a chart supports at most {MAX_GENERAL_AXES} general axes"
            )));
        }
        if self.axes.iter().any(|axis| axis.id == options.id) {
            return Err(invalid(format!(
                "general axis id {:?} already exists",
                options.id
            )));
        }
        validate_options(&options)?;
        let handle = NonZeroU32::new(self.next_handle)
            .map(GeneralAxisHandle)
            .ok_or_else(|| resource("general axis identity space is exhausted"))?;
        self.next_handle = self
            .next_handle
            .checked_add(1)
            .ok_or_else(|| resource("general axis identity space is exhausted"))?;
        let position = resolved_position(options.dimension, options.position);
        self.axes.push(GeneralAxis {
            handle,
            id: options.id,
            pane_id,
            dimension: options.dimension,
            position,
            scale: options.scale,
            domain: options.domain,
            reverse: options.reverse,
            visible: options.visible,
            title: options.title,
            tick_count: options.tick_count,
            min_tick_gap: options.min_tick_gap,
            band_padding_inner: options.band_padding_inner,
            band_padding_outer: options.band_padding_outer,
            zero_line: options.zero_line,
            grid_visible: options.grid_visible,
        });
        Ok(handle)
    }

    fn get(&self, id: &str) -> Option<&GeneralAxis> {
        self.axes.iter().find(|axis| axis.id == id)
    }

    fn remove(&mut self, id: &str) -> bool {
        let Some(index) = self.axes.iter().position(|axis| axis.id == id) else {
            return false;
        };
        self.axes.remove(index);
        true
    }

    pub(crate) fn remove_pane(&mut self, pane_id: Option<PaneId>) {
        let Some(pane_id) = pane_id else {
            return;
        };
        self.axes.retain(|axis| axis.pane_id != pane_id);
    }

    fn iter(&self) -> impl Iterator<Item = &GeneralAxis> {
        self.axes.iter()
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.axes.capacity() * std::mem::size_of::<GeneralAxis>()
            + self
                .axes
                .iter()
                .map(GeneralAxis::estimated_bytes)
                .sum::<usize>()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.axes.len()
    }
}

impl Default for GeneralAxisRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartEngine {
    pub fn add_general_axis(&mut self, options: GeneralAxisOptions) -> Result<(), ChartError> {
        let pane_id = self
            .pane_stable_id(options.pane)
            .ok_or_else(|| invalid("general axis references a stale pane"))?;
        let horizontal_domain = self
            .pane_horizontal_domain(options.pane)
            .ok_or_else(|| invalid("general axis pane has no horizontal domain"))?;
        validate_compatibility(horizontal_domain, &options)?;
        self.general_axes.insert(pane_id, options)?;
        self.invalidate_frame_all();
        Ok(())
    }

    pub fn general_axis(&self, id: &str) -> Option<&GeneralAxis> {
        self.general_axes.get(id)
    }

    pub fn general_axis_pane_index(&self, id: &str) -> Option<usize> {
        self.pane_index_for_id(self.general_axes.get(id)?.pane_id)
    }

    /// General axes in insertion order, optionally filtered to the pane currently at `pane`.
    pub fn general_axes(&self, pane: Option<usize>) -> Vec<&GeneralAxis> {
        let pane_id = match pane {
            Some(index) => match self.pane_stable_id(index) {
                Some(id) => Some(id),
                None => return Vec::new(),
            },
            None => None,
        };
        self.general_axes
            .iter()
            .filter(|axis| pane_id.is_none_or(|id| axis.pane_id == id))
            .collect()
    }

    /// Remove an unpopulated general axis. General-series ownership will add the populated-axis
    /// guard at the same registry boundary when those series are introduced.
    pub fn remove_general_axis(&mut self, id: &str) -> bool {
        let removed = self.general_axes.remove(id);
        if removed {
            self.invalidate_frame_all();
        }
        removed
    }
}

fn validate_options(options: &GeneralAxisOptions) -> Result<(), ChartError> {
    if options.id.is_empty() || options.id.len() > MAX_GENERAL_AXIS_ID_BYTES {
        return Err(invalid(format!(
            "general axis id must contain 1..={MAX_GENERAL_AXIS_ID_BYTES} UTF-8 bytes"
        )));
    }
    if options
        .title
        .as_ref()
        .is_some_and(|title| title.len() > MAX_GENERAL_AXIS_TITLE_BYTES)
    {
        return Err(resource(format!(
            "general axis title exceeds {MAX_GENERAL_AXIS_TITLE_BYTES} UTF-8 bytes"
        )));
    }
    if options
        .tick_count
        .is_some_and(|count| count == 0 || count > MAX_GENERAL_AXIS_TICKS)
    {
        return Err(invalid(format!(
            "general axis tick_count must be in 1..={MAX_GENERAL_AXIS_TICKS}"
        )));
    }
    if !options.min_tick_gap.is_finite() || !(0.0..=10_000.0).contains(&options.min_tick_gap) {
        return Err(invalid(
            "general axis min_tick_gap must be finite and in 0..=10000",
        ));
    }
    for (name, value) in [
        ("band_padding_inner", options.band_padding_inner),
        ("band_padding_outer", options.band_padding_outer),
    ] {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(invalid(format!(
                "general axis {name} must be finite and in 0..=1"
            )));
        }
    }
    validate_position(options.dimension, options.position)?;
    validate_domain(options.scale, &options.domain)
}

fn validate_position(
    dimension: AxisDimension,
    position: Option<AxisPosition>,
) -> Result<(), ChartError> {
    let valid = match (dimension, position) {
        (_, None) => true,
        (AxisDimension::X, Some(AxisPosition::Top | AxisPosition::Bottom)) => true,
        (AxisDimension::Y, Some(AxisPosition::Left | AxisPosition::Right)) => true,
        (AxisDimension::Angle | AxisDimension::Radius, Some(_)) => false,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid(
            "general axis position is incompatible with its dimension",
        ))
    }
}

fn resolved_position(
    dimension: AxisDimension,
    position: Option<AxisPosition>,
) -> Option<AxisPosition> {
    position.or(match dimension {
        AxisDimension::X => Some(AxisPosition::Bottom),
        AxisDimension::Y => Some(AxisPosition::Left),
        AxisDimension::Angle | AxisDimension::Radius => None,
    })
}

fn validate_domain(scale: GeneralScaleType, domain: &GeneralAxisDomain) -> Result<(), ChartError> {
    match domain {
        GeneralAxisDomain::Auto => Ok(()),
        GeneralAxisDomain::Numeric([from, to]) => {
            if !matches!(
                scale,
                GeneralScaleType::Linear
                    | GeneralScaleType::Logarithmic
                    | GeneralScaleType::SymmetricLog
                    | GeneralScaleType::RadialLinear
            ) {
                return Err(invalid("numeric domain requires a numeric scale"));
            }
            if !from.is_finite() || !to.is_finite() || from >= to || !(*to - *from).is_finite() {
                return Err(invalid(
                    "numeric domain bounds must be finite and strictly ascending",
                ));
            }
            if scale == GeneralScaleType::Logarithmic && *from <= 0.0 {
                return Err(invalid("logarithmic domain bounds must be positive"));
            }
            Ok(())
        }
        GeneralAxisDomain::Temporal([from, to]) => {
            if scale != GeneralScaleType::Temporal {
                return Err(invalid("temporal domain requires a temporal scale"));
            }
            if from >= to
                || from.unsigned_abs() > MAX_GENERAL_TEMPORAL_MILLISECONDS as u64
                || to.unsigned_abs() > MAX_GENERAL_TEMPORAL_MILLISECONDS as u64
            {
                return Err(invalid(
                    "temporal domain bounds must be strictly ascending safe epoch milliseconds",
                ));
            }
            Ok(())
        }
        GeneralAxisDomain::Category(values) => {
            if !matches!(
                scale,
                GeneralScaleType::Band
                    | GeneralScaleType::Point
                    | GeneralScaleType::AngularCategory
            ) {
                return Err(invalid("category domain requires a category scale"));
            }
            if values.len() > MAX_GENERAL_AXIS_CATEGORIES {
                return Err(resource(format!(
                    "general axis category domain exceeds {MAX_GENERAL_AXIS_CATEGORIES} labels"
                )));
            }
            let bytes = values.iter().try_fold(0usize, |total, value| {
                total
                    .checked_add(value.len())
                    .ok_or_else(|| resource("general axis category domain byte count overflow"))
            })?;
            if bytes > MAX_GENERAL_AXIS_CATEGORY_BYTES {
                return Err(resource(format!(
                    "general axis category domain exceeds {MAX_GENERAL_AXIS_CATEGORY_BYTES} UTF-8 bytes"
                )));
            }
            let mut unique = HashSet::with_capacity(values.len());
            if values.iter().any(|value| !unique.insert(value.as_str())) {
                return Err(invalid("general axis category labels must be unique"));
            }
            Ok(())
        }
    }
}

fn validate_compatibility(
    horizontal_domain: HorizontalDomain,
    options: &GeneralAxisOptions,
) -> Result<(), ChartError> {
    let compatible = match (horizontal_domain, options.dimension, options.scale) {
        (HorizontalDomain::FinancialTime, _, _) => false,
        (HorizontalDomain::Continuous { scale: domain }, AxisDimension::X, axis_scale) => {
            numeric_scale_for_domain(domain) == axis_scale
        }
        (HorizontalDomain::Temporal, AxisDimension::X, GeneralScaleType::Temporal) => true,
        (HorizontalDomain::Category { scale: domain }, AxisDimension::X, axis_scale) => {
            category_scale_for_domain(domain) == axis_scale
        }
        (
            HorizontalDomain::Continuous { .. }
            | HorizontalDomain::Temporal
            | HorizontalDomain::Category { .. },
            AxisDimension::Y,
            GeneralScaleType::Linear
            | GeneralScaleType::Logarithmic
            | GeneralScaleType::SymmetricLog,
        ) => true,
        (HorizontalDomain::Polar, AxisDimension::Angle, GeneralScaleType::AngularCategory) => true,
        (HorizontalDomain::Polar, AxisDimension::Radius, GeneralScaleType::RadialLinear) => true,
        _ => false,
    };
    if compatible {
        Ok(())
    } else {
        Err(invalid(
            "general axis scale or dimension is incompatible with the pane domain",
        ))
    }
}

fn numeric_scale_for_domain(scale: ContinuousScaleType) -> GeneralScaleType {
    match scale {
        ContinuousScaleType::Linear => GeneralScaleType::Linear,
        ContinuousScaleType::Logarithmic => GeneralScaleType::Logarithmic,
        ContinuousScaleType::SymmetricLog => GeneralScaleType::SymmetricLog,
    }
}

fn category_scale_for_domain(scale: CategoryScaleType) -> GeneralScaleType {
    match scale {
        CategoryScaleType::Band => GeneralScaleType::Band,
        CategoryScaleType::Point => GeneralScaleType::Point,
    }
}

fn invalid(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::InvalidOptions, message)
}

fn resource(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::ResourceLimit, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn category_axis(pane: usize, id: &str) -> GeneralAxisOptions {
        GeneralAxisOptions::new(id, pane, AxisDimension::X, GeneralScaleType::Band)
    }

    #[test]
    fn compatibility_matrix_accepts_only_matching_axes() {
        let cases = [
            (
                HorizontalDomain::Continuous {
                    scale: ContinuousScaleType::Linear,
                },
                AxisDimension::X,
                GeneralScaleType::Linear,
            ),
            (
                HorizontalDomain::Temporal,
                AxisDimension::X,
                GeneralScaleType::Temporal,
            ),
            (
                HorizontalDomain::Category {
                    scale: CategoryScaleType::Point,
                },
                AxisDimension::X,
                GeneralScaleType::Point,
            ),
            (
                HorizontalDomain::Category {
                    scale: CategoryScaleType::Band,
                },
                AxisDimension::Y,
                GeneralScaleType::Logarithmic,
            ),
            (
                HorizontalDomain::Polar,
                AxisDimension::Angle,
                GeneralScaleType::AngularCategory,
            ),
            (
                HorizontalDomain::Polar,
                AxisDimension::Radius,
                GeneralScaleType::RadialLinear,
            ),
        ];
        for (domain, dimension, scale) in cases {
            let options = GeneralAxisOptions::new("axis", 0, dimension, scale);
            assert!(validate_compatibility(domain, &options).is_ok());
        }

        for (domain, dimension, scale) in [
            (
                HorizontalDomain::FinancialTime,
                AxisDimension::Y,
                GeneralScaleType::Linear,
            ),
            (
                HorizontalDomain::Temporal,
                AxisDimension::X,
                GeneralScaleType::Linear,
            ),
            (
                HorizontalDomain::Category {
                    scale: CategoryScaleType::Band,
                },
                AxisDimension::X,
                GeneralScaleType::Point,
            ),
            (
                HorizontalDomain::Polar,
                AxisDimension::X,
                GeneralScaleType::Linear,
            ),
        ] {
            let options = GeneralAxisOptions::new("axis", 0, dimension, scale);
            assert_eq!(
                validate_compatibility(domain, &options).unwrap_err().code(),
                ErrorCode::InvalidOptions
            );
        }
    }

    #[test]
    fn explicit_domains_validate_scale_bounds_and_category_identity() {
        let mut numeric =
            GeneralAxisOptions::new("value", 0, AxisDimension::Y, GeneralScaleType::Logarithmic);
        numeric.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        assert!(validate_options(&numeric).is_err());
        numeric.domain = GeneralAxisDomain::Numeric([0.1, 10.0]);
        assert!(validate_options(&numeric).is_ok());

        let mut temporal =
            GeneralAxisOptions::new("time", 0, AxisDimension::X, GeneralScaleType::Temporal);
        temporal.domain = GeneralAxisDomain::Temporal([
            -MAX_GENERAL_TEMPORAL_MILLISECONDS,
            MAX_GENERAL_TEMPORAL_MILLISECONDS,
        ]);
        assert!(validate_options(&temporal).is_ok());
        temporal.domain = GeneralAxisDomain::Temporal([0, i64::MAX]);
        assert!(validate_options(&temporal).is_err());

        let mut category = category_axis(0, "category");
        category.domain = GeneralAxisDomain::Category(vec!["".into(), "A".into(), "A".into()]);
        assert!(validate_options(&category).is_err());
        category.domain = GeneralAxisDomain::Category(vec!["".into(), "A".into()]);
        assert!(validate_options(&category).is_ok());
    }

    #[test]
    fn position_defaults_and_validation_follow_dimension() {
        assert_eq!(
            resolved_position(AxisDimension::X, None),
            Some(AxisPosition::Bottom)
        );
        assert_eq!(
            resolved_position(AxisDimension::Y, None),
            Some(AxisPosition::Left)
        );
        assert_eq!(resolved_position(AxisDimension::Angle, None), None);
        assert!(validate_position(AxisDimension::X, Some(AxisPosition::Top)).is_ok());
        assert!(validate_position(AxisDimension::X, Some(AxisPosition::Left)).is_err());
        assert!(validate_position(AxisDimension::Radius, Some(AxisPosition::Right)).is_err());
    }
}
