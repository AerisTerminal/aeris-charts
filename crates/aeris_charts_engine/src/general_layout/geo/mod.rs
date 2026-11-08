//! Map datasets: decoded GeoJSON or TopoJSON features, per-feature values, the projection, and
//! the choropleth color scale. The types exist in both builds so the standard build can reject
//! map data with the typed complete-build error; decoding, projection, triangulation, and frame
//! construction compile only into the complete build.
//!
//! Geometry is held decoded: positions are longitude/latitude degrees (planar x/y for the
//! identity projection), rings are stored open, without the repeated closing position, and a
//! frame never re-parses source text.
//!
//! Bubble and flow maps hold marks instead of geometry. A mark sits at a longitude/latitude
//! position or at the d3 `geoCentroid` of a feature of the pane's base map, the pane's first
//! choropleth series; marks project through that base map's fitted projection so they stay
//! on it, and without a base map they fit their own projection to their positions.

#[cfg(feature = "complete")]
mod choropleth;
#[cfg(feature = "complete")]
mod decode;
#[cfg(feature = "complete")]
mod earcut;
#[cfg(feature = "complete")]
mod marks;
#[cfg(feature = "complete")]
mod projection;
#[cfg(feature = "complete")]
mod view;

#[cfg(feature = "complete")]
pub(super) use choropleth::{GeoCache, cache_reservation};
#[cfg(feature = "complete")]
pub(super) use marks::{MarkCache, MarkInput, bubble_label, flow_id, mark_cache_reservation};
#[cfg(feature = "complete")]
pub use view::MapView;
#[cfg(feature = "complete")]
pub(super) use view::validate_view;

use crate::GeneralPointSymbol;

#[cfg(feature = "complete")]
use std::collections::HashSet;

#[cfg(feature = "complete")]
use aeris_charts_render::color::Color;

#[cfg(feature = "complete")]
use super::{add_text_bytes, invalid_data, resource_limit, validate_rows};
#[cfg(feature = "complete")]
use crate::ChartError;

/// Positions one map dataset can hold: the Natural Earth 1:50m countries (99,539 positions)
/// fit with headroom; the 1:10m countries (544,898) do not.
#[cfg(feature = "complete")]
pub(super) const MAX_GEO_POSITIONS: usize = 262_144;
/// Rings and lines one map dataset can hold.
#[cfg(feature = "complete")]
pub(super) const MAX_GEO_RINGS: usize = 65_536;
/// Largest coordinate magnitude: degrees for spherical projections, plane units for identity.
#[cfg(feature = "complete")]
const MAX_GEO_COORDINATE: f64 = 1e9;
#[cfg(feature = "complete")]
const MAX_QUANTIZE_CLASSES: u8 = 9;
/// Largest bubble radius and flow width, in media px.
#[cfg(feature = "complete")]
const MAX_MARK_SIZE: f64 = 64.0;

/// Geometry family of one map feature.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GeoShape {
    /// A feature without geometry (GeoJSON `null`).
    #[default]
    Empty,
    /// GeoJSON `Polygon` or `MultiPolygon`.
    Polygon,
    /// GeoJSON `LineString` or `MultiLineString`.
    Line,
    /// GeoJSON `Point` or `MultiPoint`.
    Point,
}

/// One decoded map feature (complete build).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeoFeature {
    /// Identity: the host-named id property, else the feature `id`, else `#<index>`.
    pub id: String,
    /// The host-named label property (default `name`), else the id.
    pub label: String,
    pub shape: GeoShape,
    /// Positions in input order.
    pub points: Vec<[f64; 2]>,
    /// Exclusive end offset into `points` of each ring (polygons) or line (lines); empty for
    /// points and empty features.
    pub rings: Vec<u32>,
    /// Exclusive end offset into `rings` of each polygon; empty unless `shape` is `Polygon`.
    /// The first ring of a polygon is its exterior and the rest are holes, as in GeoJSON.
    pub polygons: Vec<u32>,
}

/// The value of one feature, keyed by feature id.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeoValue {
    pub id: String,
    pub value: f64,
}

/// A d3-geo 3 projection family.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeoProjectionKind {
    Mercator,
    #[default]
    EqualEarth,
    NaturalEarth1,
    /// The visible hemisphere, clipped at 90 degrees from the center.
    Orthographic,
    /// Albers conic equal-area with standard `parallels`.
    ConicEqualArea,
    /// Planar coordinates with the y axis pointing up (d3 `geoIdentity().reflectY(true)`).
    Identity,
}

/// Projection of a map dataset. The projected geometry always fits the plot inset by
/// `padding` (d3 `fitExtent`); `center`, when set, then moves that longitude/latitude to the
/// middle of the fitted extent at the fitted scale.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeoProjection {
    #[serde(rename = "type", default)]
    pub kind: GeoProjectionKind,
    /// d3 `projection.rotate` in degrees: `[lambda, phi, gamma]`.
    #[serde(default)]
    pub rotate: [f64; 3],
    #[serde(default)]
    pub center: Option<[f64; 2]>,
    /// Standard parallels of the conic projection in degrees; d3's default is `[0, 60]`.
    #[serde(default = "geo_parallels_default")]
    pub parallels: [f64; 2],
    /// Space between the fitted geometry and the plot edges, in media px.
    #[serde(default = "geo_padding_default")]
    pub padding: f64,
}

fn geo_parallels_default() -> [f64; 2] {
    [0.0, 60.0]
}

fn geo_padding_default() -> f64 {
    8.0
}

impl Default for GeoProjection {
    fn default() -> Self {
        Self {
            kind: GeoProjectionKind::default(),
            rotate: [0.0; 3],
            center: None,
            parallels: geo_parallels_default(),
            padding: geo_padding_default(),
        }
    }
}

/// How a quantized choropleth places its class breaks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantizeMethod {
    /// Classes of equal value width (d3 `scaleQuantize`).
    #[default]
    EqualInterval,
    /// Classes of equal feature count (d3 `scaleQuantile`).
    Quantile,
}

/// Value-to-color mapping of a choropleth. Colors are CSS strings; an omitted low or high
/// color uses the series color at 20% opacity and the series color.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChoroplethScale {
    /// Continuous from the lowest to the highest value.
    Sequential {
        #[serde(default)]
        low_color: Option<String>,
        #[serde(default)]
        high_color: Option<String>,
    },
    /// Continuous through `mid_color` at `midpoint`; defaults blue, light gray, red.
    Diverging {
        #[serde(default)]
        low_color: Option<String>,
        #[serde(default)]
        mid_color: Option<String>,
        #[serde(default)]
        high_color: Option<String>,
        #[serde(default)]
        midpoint: f64,
    },
    /// `classes` (2..=9) discrete colors from low to high.
    Quantize {
        #[serde(default = "quantize_classes_default")]
        classes: u8,
        #[serde(default)]
        method: QuantizeMethod,
        #[serde(default)]
        low_color: Option<String>,
        #[serde(default)]
        high_color: Option<String>,
    },
}

fn quantize_classes_default() -> u8 {
    5
}

impl Default for ChoroplethScale {
    fn default() -> Self {
        Self::Sequential {
            low_color: None,
            high_color: None,
        }
    }
}

/// Where a map mark sits: a `[longitude, latitude]` position in degrees (planar `[x, y]` under
/// the identity projection), or the id of a base-map feature whose centroid places it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum GeoAnchor {
    Position([f64; 2]),
    Feature(String),
}

/// One bubble of a bubble map (complete build).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeoPoint {
    pub id: String,
    /// Name in tooltips and accessibility; defaults to the anchoring feature's label, then
    /// the id.
    #[serde(default)]
    pub label: Option<String>,
    pub at: GeoAnchor,
    /// Non-negative size value; a bubble without one takes the smallest radius.
    #[serde(default)]
    pub value: Option<f64>,
    /// CSS color overriding the series color.
    #[serde(default)]
    pub color: Option<String>,
}

/// A bubble map dataset (complete build): marks with square-root value-scaled radii.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeoPointsInput {
    pub points: Vec<GeoPoint>,
    #[serde(default, with = "symbol_name")]
    pub symbol: GeneralPointSymbol,
    /// Smallest and largest radius in media px; the largest value takes the largest radius.
    #[serde(default = "bubble_radius_default")]
    pub radius: [f64; 2],
    /// Projection used only when the pane has no base map.
    #[serde(default)]
    pub projection: GeoProjection,
}

fn bubble_radius_default() -> [f64; 2] {
    [3.0, 24.0]
}

impl Default for GeoPointsInput {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            symbol: GeneralPointSymbol::Circle,
            radius: bubble_radius_default(),
            projection: GeoProjection::default(),
        }
    }
}

/// One great-circle link of a flow map (complete build).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeoLink {
    /// Identity; defaults to `#<index>`.
    #[serde(default)]
    pub id: Option<String>,
    pub source: GeoAnchor,
    pub target: GeoAnchor,
    /// Endpoint names; default to the anchoring feature's label, then the coordinates.
    #[serde(default)]
    pub source_label: Option<String>,
    #[serde(default)]
    pub target_label: Option<String>,
    /// Non-negative width value; a link without one takes the smallest width.
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub color: Option<String>,
}

/// A flow map dataset (complete build): links drawn as great-circle arcs with value-scaled
/// widths and optional arrowheads at their targets.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeoLinksInput {
    pub links: Vec<GeoLink>,
    /// Smallest and largest width in media px; the largest value takes the largest width.
    #[serde(default = "flow_width_default")]
    pub width: [f64; 2],
    #[serde(default)]
    pub arrows: bool,
    /// Projection used only when the pane has no base map.
    #[serde(default)]
    pub projection: GeoProjection,
}

fn flow_width_default() -> [f64; 2] {
    [1.0, 6.0]
}

impl Default for GeoLinksInput {
    fn default() -> Self {
        Self {
            links: Vec::new(),
            width: flow_width_default(),
            arrows: false,
            projection: GeoProjection::default(),
        }
    }
}

/// Lowercase wire names of the shared point symbols.
mod symbol_name {
    use crate::GeneralPointSymbol;

    pub(super) fn serialize<S: serde::Serializer>(
        symbol: &GeneralPointSymbol,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match symbol {
            GeneralPointSymbol::Circle => "circle",
            GeneralPointSymbol::Square => "square",
            GeneralPointSymbol::Diamond => "diamond",
            GeneralPointSymbol::Triangle => "triangle",
        })
    }

    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<GeneralPointSymbol, D::Error> {
        let name = <std::borrow::Cow<'de, str> as serde::Deserialize>::deserialize(deserializer)?;
        match name.as_ref() {
            "circle" => Ok(GeneralPointSymbol::Circle),
            "square" => Ok(GeneralPointSymbol::Square),
            "diamond" => Ok(GeneralPointSymbol::Diamond),
            "triangle" => Ok(GeneralPointSymbol::Triangle),
            _ => Err(serde::de::Error::unknown_variant(
                &name,
                &["circle", "square", "diamond", "triangle"],
            )),
        }
    }
}

/// A map dataset (complete build): decoded features, values by feature id, the projection,
/// and the choropleth scale. The JSON form accepts a GeoJSON `geojson` object
/// (FeatureCollection, Feature or Geometry) or a TopoJSON `topojson` topology with the name of
/// one `object`, and always serializes as a GeoJSON FeatureCollection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeoInput {
    pub features: Vec<GeoFeature>,
    pub values: Vec<GeoValue>,
    pub projection: GeoProjection,
    pub scale: ChoroplethScale,
    /// Fill of features without a value; defaults to a neutral gray between the surface and
    /// text colors.
    pub missing_color: Option<String>,
}

#[cfg(feature = "complete")]
impl GeoFeature {
    /// The ring (or line) ranges of this feature in `points`.
    pub(super) fn ring_ranges(&self) -> impl Iterator<Item = std::ops::Range<usize>> + '_ {
        self.rings.iter().scan(0usize, |start, &end| {
            let range = *start..end as usize;
            *start = end as usize;
            Some(range)
        })
    }

    fn validate(&self) -> Result<(), ChartError> {
        let monotone = |offsets: &[u32], total: usize| {
            offsets.windows(2).all(|pair| pair[0] <= pair[1])
                && offsets.last().is_none_or(|&end| end as usize == total)
        };
        let consistent = match self.shape {
            GeoShape::Empty => {
                self.points.is_empty() && self.rings.is_empty() && self.polygons.is_empty()
            }
            GeoShape::Point => {
                !self.points.is_empty() && self.rings.is_empty() && self.polygons.is_empty()
            }
            GeoShape::Line => {
                !self.rings.is_empty()
                    && self.polygons.is_empty()
                    && monotone(&self.rings, self.points.len())
            }
            GeoShape::Polygon => {
                !self.polygons.is_empty()
                    && monotone(&self.rings, self.points.len())
                    && monotone(&self.polygons, self.rings.len())
            }
        };
        if !consistent {
            return Err(invalid_data(
                "map feature geometry offsets are inconsistent",
            ));
        }
        if self
            .points
            .iter()
            .flatten()
            .any(|value| !value.is_finite() || value.abs() > MAX_GEO_COORDINATE)
        {
            return Err(invalid_data("map coordinates must be finite and bounded"));
        }
        Ok(())
    }
}

#[cfg(feature = "complete")]
fn validate_color(color: Option<&str>) -> Result<(), ChartError> {
    match color {
        Some(color) if color.len() > 64 || Color::parse_css(color).is_none() => {
            Err(invalid_data("invalid map color"))
        }
        _ => Ok(()),
    }
}

#[cfg(feature = "complete")]
pub(super) fn validate(input: &GeoInput) -> Result<(), ChartError> {
    validate_rows(input.features.len())?;
    validate_rows(input.values.len())?;
    let mut ids = HashSet::with_capacity(input.features.len());
    let mut bytes = 0usize;
    let mut positions = 0usize;
    let mut rings = 0usize;
    for feature in &input.features {
        if feature.id.is_empty() {
            return Err(invalid_data("map feature ID must not be empty"));
        }
        if !ids.insert(feature.id.as_str()) {
            return Err(invalid_data("duplicate map feature ID"));
        }
        add_text_bytes(&mut bytes, &feature.id)?;
        add_text_bytes(&mut bytes, &feature.label)?;
        positions = positions.saturating_add(feature.points.len());
        rings = rings.saturating_add(feature.rings.len());
        if positions > MAX_GEO_POSITIONS {
            return Err(resource_limit("too many map positions"));
        }
        if rings > MAX_GEO_RINGS {
            return Err(resource_limit("too many map rings"));
        }
        feature.validate()?;
    }
    let mut valued = HashSet::with_capacity(input.values.len());
    for value in &input.values {
        if !ids.contains(value.id.as_str()) {
            return Err(invalid_data("a map value names an unknown feature"));
        }
        if !valued.insert(value.id.as_str()) {
            return Err(invalid_data("duplicate map value"));
        }
        if !value.value.is_finite() {
            return Err(invalid_data("map values must be finite"));
        }
        add_text_bytes(&mut bytes, &value.id)?;
    }
    validate_projection(&input.projection)?;
    match &input.scale {
        ChoroplethScale::Sequential {
            low_color,
            high_color,
        } => {
            validate_color(low_color.as_deref())?;
            validate_color(high_color.as_deref())?;
        }
        ChoroplethScale::Diverging {
            low_color,
            mid_color,
            high_color,
            midpoint,
        } => {
            validate_color(low_color.as_deref())?;
            validate_color(mid_color.as_deref())?;
            validate_color(high_color.as_deref())?;
            if !midpoint.is_finite() {
                return Err(invalid_data("choropleth midpoint must be finite"));
            }
        }
        ChoroplethScale::Quantize {
            classes,
            low_color,
            high_color,
            ..
        } => {
            validate_color(low_color.as_deref())?;
            validate_color(high_color.as_deref())?;
            if !(2..=MAX_QUANTIZE_CLASSES).contains(classes) {
                return Err(invalid_data("choropleth classes must be 2 through 9"));
            }
        }
    }
    validate_color(input.missing_color.as_deref())
}

#[cfg(feature = "complete")]
fn validate_projection(projection: &GeoProjection) -> Result<(), ChartError> {
    if !projection.rotate.iter().all(|angle| angle.is_finite())
        || !projection
            .center
            .is_none_or(|[lon, lat]| lon.is_finite() && lat.is_finite())
        || !projection
            .parallels
            .iter()
            .all(|parallel| parallel.is_finite() && parallel.abs() <= 90.0)
        || !(projection.padding.is_finite() && (0.0..=1_000.0).contains(&projection.padding))
    {
        return Err(invalid_data(
            "map projection angles must be finite, parallels within 90 degrees, and padding within 0..=1000",
        ));
    }
    Ok(())
}

#[cfg(feature = "complete")]
fn validate_anchor(anchor: &GeoAnchor, bytes: &mut usize) -> Result<(), ChartError> {
    match anchor {
        GeoAnchor::Position(position) => {
            if position
                .iter()
                .all(|value| value.is_finite() && value.abs() <= MAX_GEO_COORDINATE)
            {
                Ok(())
            } else {
                Err(invalid_data("map coordinates must be finite and bounded"))
            }
        }
        GeoAnchor::Feature(id) if id.is_empty() => {
            Err(invalid_data("map mark feature ID must not be empty"))
        }
        GeoAnchor::Feature(id) => add_text_bytes(bytes, id),
    }
}

#[cfg(feature = "complete")]
fn validate_mark_value(value: Option<f64>) -> Result<(), ChartError> {
    if value.is_none_or(|value| value.is_finite() && value >= 0.0) {
        Ok(())
    } else {
        Err(invalid_data(
            "map mark values must be finite and non-negative",
        ))
    }
}

#[cfg(feature = "complete")]
fn validate_size_range(
    [low, high]: [f64; 2],
    minimum: f64,
    name: &'static str,
) -> Result<(), ChartError> {
    if low.is_finite() && high.is_finite() && minimum <= low && low <= high && high <= MAX_MARK_SIZE
    {
        Ok(())
    } else {
        Err(invalid_data(name))
    }
}

#[cfg(feature = "complete")]
pub(super) fn validate_points(input: &GeoPointsInput) -> Result<(), ChartError> {
    validate_rows(input.points.len())?;
    validate_size_range(
        input.radius,
        0.0,
        "bubble radii must be ordered within 0..=64",
    )?;
    validate_projection(&input.projection)?;
    let mut ids = HashSet::with_capacity(input.points.len());
    let mut bytes = 0usize;
    for point in &input.points {
        if point.id.is_empty() {
            return Err(invalid_data("map mark ID must not be empty"));
        }
        if !ids.insert(point.id.as_str()) {
            return Err(invalid_data("duplicate map mark ID"));
        }
        add_text_bytes(&mut bytes, &point.id)?;
        if let Some(label) = &point.label {
            add_text_bytes(&mut bytes, label)?;
        }
        validate_anchor(&point.at, &mut bytes)?;
        validate_mark_value(point.value)?;
        validate_color(point.color.as_deref())?;
    }
    Ok(())
}

#[cfg(feature = "complete")]
pub(super) fn validate_links(input: &GeoLinksInput) -> Result<(), ChartError> {
    validate_rows(input.links.len())?;
    validate_size_range(
        input.width,
        0.25,
        "flow widths must be ordered within 0.25..=64",
    )?;
    validate_projection(&input.projection)?;
    let mut ids = HashSet::with_capacity(input.links.len());
    let mut bytes = 0usize;
    for link in &input.links {
        if let Some(id) = &link.id {
            if id.is_empty() {
                return Err(invalid_data("map mark ID must not be empty"));
            }
            if !ids.insert(id.as_str()) {
                return Err(invalid_data("duplicate map mark ID"));
            }
            add_text_bytes(&mut bytes, id)?;
        }
        for label in [&link.source_label, &link.target_label]
            .into_iter()
            .flatten()
        {
            add_text_bytes(&mut bytes, label)?;
        }
        validate_anchor(&link.source, &mut bytes)?;
        validate_anchor(&link.target, &mut bytes)?;
        validate_mark_value(link.value)?;
        validate_color(link.color.as_deref())?;
    }
    Ok(())
}

#[cfg(feature = "complete")]
fn anchor_bytes(anchor: &GeoAnchor) -> usize {
    match anchor {
        GeoAnchor::Position(_) => 0,
        GeoAnchor::Feature(id) => id.capacity(),
    }
}

#[cfg(feature = "complete")]
pub(super) fn points_heap_bytes(input: &GeoPointsInput) -> usize {
    input.points.capacity() * std::mem::size_of::<GeoPoint>()
        + input
            .points
            .iter()
            .map(|point| {
                point.id.capacity()
                    + option_bytes(&point.label)
                    + option_bytes(&point.color)
                    + anchor_bytes(&point.at)
            })
            .sum::<usize>()
}

#[cfg(feature = "complete")]
pub(super) fn links_heap_bytes(input: &GeoLinksInput) -> usize {
    input.links.capacity() * std::mem::size_of::<GeoLink>()
        + input
            .links
            .iter()
            .map(|link| {
                option_bytes(&link.id)
                    + option_bytes(&link.source_label)
                    + option_bytes(&link.target_label)
                    + option_bytes(&link.color)
                    + anchor_bytes(&link.source)
                    + anchor_bytes(&link.target)
            })
            .sum::<usize>()
}

#[cfg(feature = "complete")]
fn option_bytes(text: &Option<String>) -> usize {
    text.as_ref().map_or(0, String::capacity)
}

#[cfg(feature = "complete")]
pub(super) fn heap_bytes(input: &GeoInput) -> usize {
    let scale = match &input.scale {
        ChoroplethScale::Sequential {
            low_color,
            high_color,
        }
        | ChoroplethScale::Quantize {
            low_color,
            high_color,
            ..
        } => option_bytes(low_color) + option_bytes(high_color),
        ChoroplethScale::Diverging {
            low_color,
            mid_color,
            high_color,
            ..
        } => option_bytes(low_color) + option_bytes(mid_color) + option_bytes(high_color),
    };
    input.features.capacity() * std::mem::size_of::<GeoFeature>()
        + input
            .features
            .iter()
            .map(|feature| {
                feature.id.capacity()
                    + feature.label.capacity()
                    + feature.points.capacity() * std::mem::size_of::<[f64; 2]>()
                    + (feature.rings.capacity() + feature.polygons.capacity())
                        * std::mem::size_of::<u32>()
            })
            .sum::<usize>()
        + input.values.capacity() * std::mem::size_of::<GeoValue>()
        + input
            .values
            .iter()
            .map(|value| value.id.capacity())
            .sum::<usize>()
        + scale
        + option_bytes(&input.missing_color)
}

/// One value per feature in input order, `NaN` for features without a value.
#[cfg(feature = "complete")]
pub(super) fn semantic_values(input: &GeoInput) -> Vec<f64> {
    let rows: std::collections::HashMap<&str, usize> = input
        .features
        .iter()
        .enumerate()
        .map(|(row, feature)| (feature.id.as_str(), row))
        .collect();
    let mut values = vec![f64::NAN; input.features.len()];
    for value in &input.values {
        if let Some(&row) = rows.get(value.id.as_str()) {
            values[row] = value.value;
        }
    }
    values
}
