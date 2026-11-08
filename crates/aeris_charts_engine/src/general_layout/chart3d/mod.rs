//! 3D datasets: point rows for bar, scatter and line series and height grids for surfaces. The
//! types exist in both builds so the standard build can reject 3D data with the typed
//! complete-build error; the camera, projection, depth ordering and frame construction compile
//! only into the complete build.
//!
//! Every 3D series of a pane shares one axis box and one camera. The box is ECharts GL's
//! `grid3D`: 100 units on each side with data `x` along the width, data `y` along the depth from
//! near to far, and data `z` up. The pane's first 3D series owns the axis names and categories
//! and holds the camera; frames read the camera from it, so removing a later series never strands
//! the view.

#[cfg(feature = "complete")]
mod camera;
#[cfg(feature = "complete")]
mod scene;

#[cfg(feature = "complete")]
pub(super) use camera::validate_camera;
#[cfg(feature = "complete")]
pub use camera::{Camera3d, Projection3d};
#[cfg(feature = "complete")]
pub(super) use scene::{SceneCache, cache_reservation, item_color};

#[cfg(feature = "complete")]
use std::collections::HashSet;

#[cfg(feature = "complete")]
use aeris_charts_render::color::Color;

#[cfg(feature = "complete")]
use super::{add_text_bytes, invalid_data, resource_limit, validate_rows};
#[cfg(feature = "complete")]
use crate::ChartError;

/// Samples along one side of a surface grid.
#[cfg(feature = "complete")]
pub(super) const MAX_SURFACE_SIDE: usize = 128;
/// Largest coordinate magnitude of 3D data.
#[cfg(feature = "complete")]
const MAX_3D_COORDINATE: f64 = 1e15;
/// Largest bar footprint in box units, scatter diameter or line width in media px.
#[cfg(feature = "complete")]
const MAX_3D_SIZE: f64 = 100.0;
/// Color stops of a surface color map.
#[cfg(feature = "complete")]
const MAX_SURFACE_COLORS: usize = 9;

/// One axis of the 3D box (complete build).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Axis3d {
    /// Axis title drawn beside the axis line.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Category names. When set, values on this axis are category indices `0..len` placed at
    /// band centers; `x` and `y` accept categories, `z` does not.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
}

/// One 3D datum (complete build).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Point3d {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// Name in tooltips and accessibility; defaults to the `x` and `y` values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// CSS color overriding the series color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// Rows of a 3D bar, scatter or line series (complete build). A line joins the rows in input
/// order.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Points3dInput {
    pub points: Vec<Point3d>,
    #[serde(default)]
    pub x_axis: Axis3d,
    #[serde(default)]
    pub y_axis: Axis3d,
    #[serde(default)]
    pub z_axis: Axis3d,
    /// Bar footprint in box units, scatter symbol diameter or line width in media px; `None`
    /// uses the ECharts GL default of the family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
}

/// A surface over a rectangular grid (complete build). Build one from a function with
/// [`SurfaceInput::sample`].
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SurfaceInput {
    /// Strictly increasing grid positions along `x`, 2 through 128 of them.
    pub x: Vec<f64>,
    /// Strictly increasing grid positions along `y`, 2 through 128 of them.
    pub y: Vec<f64>,
    /// Heights row by row: `z[row * x.len() + column]` is the height at `(x[column], y[row])`.
    /// `None` leaves a hole: cells touching it are not drawn.
    pub z: Vec<Option<f64>>,
    #[serde(default)]
    pub x_axis: Axis3d,
    #[serde(default)]
    pub y_axis: Axis3d,
    #[serde(default)]
    pub z_axis: Axis3d,
    /// CSS color stops spread evenly from the lowest to the highest height; empty shades the
    /// series color.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub colors: Vec<String>,
    /// Outline every drawn cell (ECharts GL `wireframe.show`, on by default).
    #[serde(default = "surface_wireframe_default")]
    pub wireframe: bool,
}

fn surface_wireframe_default() -> bool {
    true
}

impl Default for SurfaceInput {
    fn default() -> Self {
        Self {
            x: Vec::new(),
            y: Vec::new(),
            z: Vec::new(),
            x_axis: Axis3d::default(),
            y_axis: Axis3d::default(),
            z_axis: Axis3d::default(),
            colors: Vec::new(),
            wireframe: true,
        }
    }
}

impl SurfaceInput {
    /// Samples `z(x, y)` on the grid `x = [min, max, step]` by `y = [min, max, step]`, as ECharts
    /// GL's `equation` does: positions run from `min` in `step` increments up to `max`, and a
    /// non-finite height leaves a hole. A range that is not finite, ascending and positive
    /// stepped yields an empty grid, which validation rejects.
    pub fn sample(x: [f64; 3], y: [f64; 3], mut z: impl FnMut(f64, f64) -> f64) -> Self {
        fn positions([min, max, step]: [f64; 3]) -> Vec<f64> {
            if !(min.is_finite() && max.is_finite() && step.is_finite() && step > 0.0 && max > min)
            {
                return Vec::new();
            }
            let count = ((max - min) / step + 1e-9).floor() as usize + 1;
            (0..count.min(4_096))
                .map(|index| min + step * index as f64)
                .collect()
        }
        let (x, y) = (positions(x), positions(y));
        let z = y
            .iter()
            .flat_map(|&row| x.iter().map(move |&column| (column, row)))
            .map(|(column, row)| Some(z(column, row)).filter(|value| value.is_finite()))
            .collect();
        Self {
            x,
            y,
            z,
            ..Self::default()
        }
    }
}

#[cfg(feature = "complete")]
fn validate_color(color: Option<&str>) -> Result<(), ChartError> {
    match color {
        Some(color) if color.len() > 64 || Color::parse_css(color).is_none() => {
            Err(invalid_data("invalid 3D color"))
        }
        _ => Ok(()),
    }
}

#[cfg(feature = "complete")]
fn validate_axis(axis: &Axis3d, categories: bool, bytes: &mut usize) -> Result<(), ChartError> {
    add_text_bytes(bytes, &axis.name)?;
    if !categories && !axis.categories.is_empty() {
        return Err(invalid_data(
            "only the x and y axes of 3D points take categories",
        ));
    }
    validate_rows(axis.categories.len())?;
    let mut names = HashSet::with_capacity(axis.categories.len());
    for category in &axis.categories {
        if !names.insert(category.as_str()) {
            return Err(invalid_data("duplicate 3D axis category"));
        }
        add_text_bytes(bytes, category)?;
    }
    Ok(())
}

#[cfg(feature = "complete")]
fn finite_coordinate(value: f64) -> bool {
    value.is_finite() && value.abs() <= MAX_3D_COORDINATE
}

#[cfg(feature = "complete")]
pub(super) fn validate_points(input: &Points3dInput) -> Result<(), ChartError> {
    validate_rows(input.points.len())?;
    let mut bytes = 0usize;
    validate_axis(&input.x_axis, true, &mut bytes)?;
    validate_axis(&input.y_axis, true, &mut bytes)?;
    validate_axis(&input.z_axis, false, &mut bytes)?;
    if input
        .size
        .is_some_and(|size| !(size.is_finite() && size > 0.0 && size <= MAX_3D_SIZE))
    {
        return Err(invalid_data(
            "3D size must be within 0 exclusive through 100",
        ));
    }
    let category = |value: f64, axis: &Axis3d| {
        axis.categories.is_empty()
            || (value.fract() == 0.0 && value >= 0.0 && value < axis.categories.len() as f64)
    };
    for point in &input.points {
        if ![point.x, point.y, point.z]
            .into_iter()
            .all(finite_coordinate)
        {
            return Err(invalid_data("3D coordinates must be finite and bounded"));
        }
        if !category(point.x, &input.x_axis) || !category(point.y, &input.y_axis) {
            return Err(invalid_data(
                "3D values on a category axis must be category indices",
            ));
        }
        if let Some(label) = &point.label {
            add_text_bytes(&mut bytes, label)?;
        }
        validate_color(point.color.as_deref())?;
    }
    Ok(())
}

#[cfg(feature = "complete")]
pub(super) fn validate_surface(input: &SurfaceInput) -> Result<(), ChartError> {
    let side = |positions: &[f64]| {
        (2..=MAX_SURFACE_SIDE).contains(&positions.len())
            && positions.iter().copied().all(finite_coordinate)
            && positions.windows(2).all(|pair| pair[0] < pair[1])
    };
    if input.x.len() > MAX_SURFACE_SIDE || input.y.len() > MAX_SURFACE_SIDE {
        return Err(resource_limit(
            "surface grids hold at most 128 by 128 samples",
        ));
    }
    if !side(&input.x) || !side(&input.y) {
        return Err(invalid_data(
            "surface grid positions must be 2 through 128 finite, strictly increasing values",
        ));
    }
    if input.z.len() != input.x.len() * input.y.len() {
        return Err(invalid_data(
            "surface heights must hold one entry per grid sample",
        ));
    }
    if !input.z.iter().flatten().copied().all(finite_coordinate) {
        return Err(invalid_data("surface heights must be finite and bounded"));
    }
    let mut bytes = 0usize;
    validate_axis(&input.x_axis, false, &mut bytes)?;
    validate_axis(&input.y_axis, false, &mut bytes)?;
    validate_axis(&input.z_axis, false, &mut bytes)?;
    if input.colors.len() > MAX_SURFACE_COLORS {
        return Err(invalid_data("surface color maps hold at most 9 stops"));
    }
    for color in &input.colors {
        validate_color(Some(color))?;
    }
    Ok(())
}

#[cfg(feature = "complete")]
fn axis_bytes(axis: &Axis3d) -> usize {
    axis.name.capacity()
        + axis.categories.capacity() * std::mem::size_of::<String>()
        + axis.categories.iter().map(String::capacity).sum::<usize>()
}

#[cfg(feature = "complete")]
pub(super) fn points_heap_bytes(input: &Points3dInput) -> usize {
    input.points.capacity() * std::mem::size_of::<Point3d>()
        + input
            .points
            .iter()
            .map(|point| {
                point.label.as_ref().map_or(0, String::capacity)
                    + point.color.as_ref().map_or(0, String::capacity)
            })
            .sum::<usize>()
        + axis_bytes(&input.x_axis)
        + axis_bytes(&input.y_axis)
        + axis_bytes(&input.z_axis)
}

#[cfg(feature = "complete")]
pub(super) fn surface_heap_bytes(input: &SurfaceInput) -> usize {
    (input.x.capacity() + input.y.capacity()) * std::mem::size_of::<f64>()
        + input.z.capacity() * std::mem::size_of::<Option<f64>>()
        + input.colors.capacity() * std::mem::size_of::<String>()
        + input.colors.iter().map(String::capacity).sum::<usize>()
        + axis_bytes(&input.x_axis)
        + axis_bytes(&input.y_axis)
        + axis_bytes(&input.z_axis)
}

/// The shortest text of one axis value: its category name, else the number.
#[cfg(feature = "complete")]
fn axis_value_text(axis: &Axis3d, value: f64) -> String {
    axis.categories
        .get(value as usize)
        .filter(|_| !axis.categories.is_empty())
        .cloned()
        .unwrap_or_else(|| format!("{value}"))
}

/// Tooltip and accessibility name of a point row.
#[cfg(feature = "complete")]
pub(super) fn point_label(input: &Points3dInput, row: usize) -> Option<String> {
    let point = input.points.get(row)?;
    Some(point.label.clone().unwrap_or_else(|| {
        format!(
            "{}, {}",
            axis_value_text(&input.x_axis, point.x),
            axis_value_text(&input.y_axis, point.y)
        )
    }))
}

/// Tooltip and accessibility name of a surface sample.
#[cfg(feature = "complete")]
pub(super) fn sample_label(input: &SurfaceInput, row: usize) -> Option<String> {
    let columns = input.x.len().max(1);
    let (x, y) = (input.x.get(row % columns)?, input.y.get(row / columns)?);
    Some(format!("{x}, {y}"))
}

/// Host identity of a surface sample: its grid column and row.
#[cfg(feature = "complete")]
pub(super) fn sample_id(input: &SurfaceInput, row: usize) -> Option<String> {
    let columns = input.x.len().max(1);
    (row < input.z.len()).then(|| format!("{},{}", row % columns, row / columns))
}
