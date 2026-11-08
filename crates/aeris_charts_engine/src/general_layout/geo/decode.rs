//! GeoJSON and TopoJSON decoding into [`GeoInput`], and its canonical JSON form.
//!
//! Coordinates stream straight from the JSON tokens into flat position and offset buffers
//! (no intermediate JSON tree), so decoding allocates about what the decoded dataset keeps.
//! The geometry `type` may follow `coordinates` in key order, so nesting is recorded without
//! knowing the type and checked against it afterwards. Every buffer stops at the dataset
//! position, ring, and arc-reference limits while it is filled.
//!
//! TopoJSON follows topojson-client 3 `feature()`: quantized arcs are delta-decoded through the
//! topology transform, a line or ring concatenates its arcs dropping each joint's repeated
//! position, a negative index `~i` reverses arc `i`, and rings shorter than four positions
//! repeat their first position.
//!
//! The serialized form is a GeoJSON FeatureCollection with explicit `id`s and the label in the
//! `name` property; decoding it reproduces the same features bit for bit.

use std::fmt;
use std::ops::Range;

use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

use super::{
    ChoroplethScale, GeoFeature, GeoInput, GeoProjection, GeoShape, GeoValue, MAX_GEO_POSITIONS,
    MAX_GEO_RINGS,
};

/// Arc references one topology can hold across all of its objects.
const MAX_ARC_REFERENCES: usize = 1 << 20;
const LABEL_PROPERTY: &str = "name";
const MIXED: &str = "map coordinates mix positions and arrays at one level";

/// Flat nested-number arrays. In position mode a list of numbers is one position; in index
/// mode it is a list of arc references. `ends[level]` holds the exclusive end offsets of the
/// lists one level above the leaves: `ends[0]` into `points` (or `indices`), `ends[1]` into
/// `ends[0]`, and `ends[2]` into `ends[1]`.
#[derive(Default)]
struct Nested {
    points: Vec<[f64; 2]>,
    indices: Vec<i64>,
    ends: [Vec<u32>; 3],
    /// Nesting depth of the root array: 1 for a position or a reference list, 0 when empty.
    depth: u8,
}

enum Node {
    Number(f64),
    List(u8),
}

struct NestedSeed<'a> {
    out: &'a mut Nested,
    positions: bool,
}

fn push_end<E: de::Error>(ends: &mut Vec<u32>, end: usize) -> Result<(), E> {
    if ends.len() >= MAX_GEO_RINGS {
        return Err(E::custom("too many map rings"));
    }
    ends.push(end as u32);
    Ok(())
}

impl<'de> DeserializeSeed<'de> for NestedSeed<'_> {
    type Value = Node;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Node, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for NestedSeed<'_> {
    type Value = Node;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("nested coordinate arrays")
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Node, E> {
        Ok(Node::Number(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Node, E> {
        Ok(Node::Number(value as f64))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Node, E> {
        Ok(Node::Number(value as f64))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
        let positions = self.positions;
        let out = self.out;
        let mut numbers = 0usize;
        let mut first = [0.0; 2];
        let mut child: Option<u8> = None;
        while let Some(node) = seq.next_element_seed(NestedSeed {
            out: &mut *out,
            positions,
        })? {
            match node {
                Node::Number(value) => {
                    if child.is_some() {
                        return Err(de::Error::custom(MIXED));
                    }
                    if positions {
                        if numbers < 2 {
                            first[numbers] = value;
                        }
                    } else {
                        if value.fract() != 0.0 || value.abs() > f64::from(i32::MAX) {
                            return Err(de::Error::custom("arc references must be integers"));
                        }
                        if out.indices.len() >= MAX_ARC_REFERENCES {
                            return Err(de::Error::custom("too many map arc references"));
                        }
                        out.indices.push(value as i64);
                    }
                    numbers += 1;
                }
                Node::List(0) => {
                    if numbers > 0 {
                        return Err(de::Error::custom(MIXED));
                    }
                }
                Node::List(depth) => {
                    if numbers > 0 || child.is_some_and(|known| known != depth) {
                        return Err(de::Error::custom(MIXED));
                    }
                    child = Some(depth);
                }
            }
        }
        if numbers > 0 {
            if positions {
                if numbers < 2 {
                    return Err(de::Error::custom("a map position needs two coordinates"));
                }
                if out.points.len() >= MAX_GEO_POSITIONS {
                    return Err(de::Error::custom("too many map positions"));
                }
                out.points.push(first);
            } else {
                push_end(&mut out.ends[0], out.indices.len())?;
            }
            return Ok(Node::List(1));
        }
        let Some(child) = child else {
            return Ok(Node::List(0));
        };
        let depth = child + 1;
        let level = usize::from(depth) - if positions { 2 } else { 1 };
        if level > 2 {
            return Err(de::Error::custom("map coordinates are nested too deeply"));
        }
        let below = if level == 0 {
            if positions {
                out.points.len()
            } else {
                out.indices.len()
            }
        } else {
            out.ends[level - 1].len()
        };
        push_end(&mut out.ends[level], below)?;
        Ok(Node::List(depth))
    }
}

/// Seed of one root coordinate (or arc reference) array.
struct NestedRoot {
    positions: bool,
}

impl<'de> DeserializeSeed<'de> for NestedRoot {
    type Value = Nested;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Nested, D::Error> {
        let mut nested = Nested::default();
        match (NestedSeed {
            out: &mut nested,
            positions: self.positions,
        })
        .deserialize(deserializer)?
        {
            Node::Number(_) => Err(de::Error::custom("map coordinates must be arrays")),
            Node::List(depth) => {
                nested.depth = depth;
                Ok(nested)
            }
        }
    }
}

/// A scalar property or id as text; arrays, objects, and nulls read as `None`.
struct Scalar(Option<String>);

/// JavaScript `String(number)` for the integers map ids use, Rust's shortest form otherwise.
fn number_text(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ScalarVisitor;

        impl<'de> Visitor<'de> for ScalarVisitor {
            type Value = Scalar;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map property value")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Scalar, E> {
                Ok(Scalar(Some(value.to_owned())))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<Scalar, E> {
                Ok(Scalar(Some(value)))
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Scalar, E> {
                Ok(Scalar(Some(value.to_string())))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Scalar, E> {
                Ok(Scalar(Some(value.to_string())))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Scalar, E> {
                Ok(Scalar(Some(value.to_string())))
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Scalar, E> {
                Ok(Scalar(Some(number_text(value))))
            }

            fn visit_unit<E: de::Error>(self) -> Result<Scalar, E> {
                Ok(Scalar(None))
            }

            fn visit_none<E: de::Error>(self) -> Result<Scalar, E> {
                Ok(Scalar(None))
            }

            fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Scalar, D::Error> {
                Scalar::deserialize(deserializer)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Scalar, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Scalar(None))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Scalar, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(Scalar(None))
            }
        }

        deserializer.deserialize_any(ScalarVisitor)
    }
}

/// The scalar members of a GeoJSON `properties` object.
#[derive(Default)]
struct Properties(Vec<(String, String)>);

impl<'de> Deserialize<'de> for Properties {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PropertiesVisitor;

        impl<'de> Visitor<'de> for PropertiesVisitor {
            type Value = Properties;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a properties object or null")
            }

            fn visit_unit<E: de::Error>(self) -> Result<Properties, E> {
                Ok(Properties::default())
            }

            fn visit_none<E: de::Error>(self) -> Result<Properties, E> {
                Ok(Properties::default())
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Properties, A::Error> {
                let mut properties = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    if let Scalar(Some(value)) = map.next_value()? {
                        properties.push((key, value));
                    }
                }
                Ok(Properties(properties))
            }
        }

        deserializer.deserialize_any(PropertiesVisitor)
    }
}

/// One GeoJSON or TopoJSON object: a feature collection, feature, geometry, or geometry
/// collection, keeping only the members map decoding reads.
#[derive(Default)]
struct RawObject {
    kind: Option<String>,
    id: Option<String>,
    properties: Vec<(String, String)>,
    geometry: Option<Box<RawObject>>,
    coordinates: Option<Nested>,
    arcs: Option<Nested>,
    /// `features` of a FeatureCollection or `geometries` of a GeometryCollection.
    members: Option<Vec<RawObject>>,
}

impl<'de> Deserialize<'de> for RawObject {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;

        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = RawObject;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a GeoJSON or TopoJSON object")
            }

            fn visit_unit<E: de::Error>(self) -> Result<RawObject, E> {
                Ok(RawObject::default())
            }

            fn visit_none<E: de::Error>(self) -> Result<RawObject, E> {
                Ok(RawObject::default())
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RawObject, A::Error> {
                let mut object = RawObject::default();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "type" => object.kind = map.next_value()?,
                        "id" => object.id = map.next_value::<Scalar>()?.0,
                        "properties" => object.properties = map.next_value::<Properties>()?.0,
                        "geometry" => object.geometry = Some(Box::new(map.next_value()?)),
                        "coordinates" => {
                            object.coordinates =
                                Some(map.next_value_seed(NestedRoot { positions: true })?);
                        }
                        "arcs" => {
                            object.arcs =
                                Some(map.next_value_seed(NestedRoot { positions: false })?);
                        }
                        "features" | "geometries" => object.members = Some(map.next_value()?),
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(object)
            }
        }

        deserializer.deserialize_any(ObjectVisitor)
    }
}

#[derive(Deserialize)]
struct Transform {
    scale: [f64; 2],
    translate: [f64; 2],
}

/// The named objects of a topology, in input order.
struct Objects(Vec<(String, RawObject)>);

impl<'de> Deserialize<'de> for Objects {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectsVisitor;

        impl<'de> Visitor<'de> for ObjectsVisitor {
            type Value = Objects;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map of topology objects")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Objects, A::Error> {
                let mut objects = Vec::new();
                while let Some(entry) = map.next_entry::<String, RawObject>()? {
                    objects.push(entry);
                }
                Ok(Objects(objects))
            }
        }

        deserializer.deserialize_map(ObjectsVisitor)
    }
}

#[derive(Default)]
struct RawTopology {
    kind: Option<String>,
    transform: Option<Transform>,
    arcs: Option<Nested>,
    objects: Vec<(String, RawObject)>,
}

impl<'de> Deserialize<'de> for RawTopology {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TopologyVisitor;

        impl<'de> Visitor<'de> for TopologyVisitor {
            type Value = RawTopology;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a TopoJSON topology")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RawTopology, A::Error> {
                let mut topology = RawTopology::default();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "type" => topology.kind = map.next_value()?,
                        "transform" => topology.transform = map.next_value()?,
                        "arcs" => {
                            topology.arcs =
                                Some(map.next_value_seed(NestedRoot { positions: true })?);
                        }
                        "objects" => topology.objects = map.next_value::<Objects>()?.0,
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(topology)
            }
        }

        deserializer.deserialize_map(TopologyVisitor)
    }
}

/// Which properties name a feature: the host-named id property (else the feature `id`, else
/// `#<index>`) and the label property (else the id).
struct Naming<'a> {
    id: Option<&'a str>,
    label: &'a str,
}

impl Naming<'_> {
    fn identity(&self, object: &RawObject, index: usize) -> (String, String) {
        let property = |name: &str| {
            object
                .properties
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        let id = self
            .id
            .and_then(property)
            .or_else(|| object.id.clone())
            .unwrap_or_else(|| format!("#{index}"));
        let label = property(self.label).unwrap_or_else(|| id.clone());
        (id, label)
    }
}

struct Geometry {
    shape: GeoShape,
    points: Vec<[f64; 2]>,
    rings: Vec<u32>,
    polygons: Vec<u32>,
}

impl Geometry {
    fn empty() -> Self {
        Self {
            shape: GeoShape::Empty,
            points: Vec::new(),
            rings: Vec::new(),
            polygons: Vec::new(),
        }
    }

    fn into_feature(self, (id, label): (String, String)) -> GeoFeature {
        let mut feature = GeoFeature {
            id,
            label,
            shape: self.shape,
            points: self.points,
            rings: self.rings,
            polygons: self.polygons,
        };
        feature.points.shrink_to_fit();
        feature.rings.shrink_to_fit();
        feature.polygons.shrink_to_fit();
        feature
    }
}

/// Drops each ring's repeated closing position, compacting `points` in place.
fn open_rings(points: &mut Vec<[f64; 2]>, rings: &mut [u32]) {
    let mut write = 0usize;
    let mut start = 0usize;
    for end in rings.iter_mut() {
        let stop = *end as usize;
        let keep = if stop - start > 1 && points[start] == points[stop - 1] {
            stop - 1
        } else {
            stop
        };
        points.copy_within(start..keep, write);
        write += keep - start;
        start = stop;
        *end = write as u32;
    }
    points.truncate(write);
}

/// The coordinate nesting depth of a geometry type.
fn geometry_depth(kind: &str) -> Result<u8, String> {
    match kind {
        "Point" => Ok(1),
        "MultiPoint" | "LineString" => Ok(2),
        "MultiLineString" | "Polygon" => Ok(3),
        "MultiPolygon" => Ok(4),
        "GeometryCollection" => Err("map GeometryCollection features are not supported".into()),
        other => Err(format!("unsupported map geometry type {other:?}")),
    }
}

fn shape_of(kind: &str) -> GeoShape {
    match kind {
        "Point" | "MultiPoint" => GeoShape::Point,
        "LineString" | "MultiLineString" => GeoShape::Line,
        _ => GeoShape::Polygon,
    }
}

fn geojson_geometry(object: RawObject, positions: &mut usize) -> Result<Geometry, String> {
    let Some(kind) = object.kind else {
        return Ok(Geometry::empty());
    };
    let depth = geometry_depth(&kind)?;
    let Some(nested) = object.coordinates.filter(|nested| nested.depth > 0) else {
        return Ok(Geometry::empty());
    };
    if nested.depth != depth {
        return Err(format!(
            "map {kind} coordinates are nested {}",
            nested.depth
        ));
    }
    *positions += nested.points.len();
    if *positions > MAX_GEO_POSITIONS {
        return Err("too many map positions".into());
    }
    let Nested {
        mut points,
        ends: [mut rings, polygons, _],
        ..
    } = nested;
    Ok(match shape_of(&kind) {
        GeoShape::Point => Geometry {
            shape: GeoShape::Point,
            points,
            rings: Vec::new(),
            polygons: Vec::new(),
        },
        GeoShape::Line => Geometry {
            shape: GeoShape::Line,
            points,
            rings,
            polygons: Vec::new(),
        },
        _ => {
            open_rings(&mut points, &mut rings);
            Geometry {
                shape: GeoShape::Polygon,
                points,
                rings,
                polygons,
            }
        }
    })
}

fn from_geojson(root: RawObject, naming: &Naming) -> Result<Vec<GeoFeature>, String> {
    let mut positions = 0usize;
    let feature = |object: RawObject, index: usize, positions: &mut usize| {
        if object.kind.as_deref() != Some("Feature") {
            return Err("a map FeatureCollection holds only Feature objects".to_owned());
        }
        let identity = naming.identity(&object, index);
        let geometry = match object.geometry {
            Some(geometry) => geojson_geometry(*geometry, positions)?,
            None => Geometry::empty(),
        };
        Ok(geometry.into_feature(identity))
    };
    match root.kind.as_deref() {
        Some("FeatureCollection") => root
            .members
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, object)| feature(object, index, &mut positions))
            .collect(),
        Some("Feature") => Ok(vec![feature(root, 0, &mut positions)?]),
        Some(_) => {
            let identity = naming.identity(&root, 0);
            Ok(vec![
                geojson_geometry(root, &mut positions)?.into_feature(identity),
            ])
        }
        None => Err("geojson needs a type".into()),
    }
}

fn arc_range(arcs: &Nested, index: usize) -> Option<Range<usize>> {
    let end = *arcs.ends[0].get(index)? as usize;
    let start = index
        .checked_sub(1)
        .map_or(0, |previous| arcs.ends[0][previous] as usize);
    Some(start..end)
}

/// topojson-client `line`/`ring`: concatenates arcs, dropping the repeated joint positions.
fn stitch(
    arcs: &Nested,
    references: &[i64],
    ring: bool,
    points: &mut Vec<[f64; 2]>,
) -> Result<(), String> {
    let start = points.len();
    for &reference in references {
        let (index, reverse) = if reference < 0 {
            ((!reference) as usize, true)
        } else {
            (reference as usize, false)
        };
        let range = arc_range(arcs, index).ok_or("a map arc reference is out of range")?;
        if points.len() > start {
            points.pop();
        }
        let first = points.len();
        if first + range.len() > MAX_GEO_POSITIONS {
            return Err("too many map positions".into());
        }
        points.extend_from_slice(&arcs.points[range]);
        if reverse {
            points[first..].reverse();
        }
    }
    let minimum = if ring { 4 } else { 2 };
    while points.len() > start && points.len() - start < minimum {
        points.push(points[start]);
    }
    Ok(())
}

fn topology_geometry(
    arcs: &Nested,
    transform: Option<&Transform>,
    object: RawObject,
    positions: &mut usize,
) -> Result<Geometry, String> {
    let Some(kind) = object.kind else {
        return Ok(Geometry::empty());
    };
    let depth = geometry_depth(&kind)?;
    let mut geometry = match shape_of(&kind) {
        GeoShape::Point => {
            let Some(mut nested) = object.coordinates.filter(|nested| nested.depth > 0) else {
                return Ok(Geometry::empty());
            };
            if nested.depth != depth {
                return Err(format!(
                    "map {kind} coordinates are nested {}",
                    nested.depth
                ));
            }
            if let Some(transform) = transform {
                for point in &mut nested.points {
                    *point = [
                        point[0] * transform.scale[0] + transform.translate[0],
                        point[1] * transform.scale[1] + transform.translate[1],
                    ];
                }
            }
            Geometry {
                shape: GeoShape::Point,
                points: nested.points,
                rings: Vec::new(),
                polygons: Vec::new(),
            }
        }
        shape => {
            let Some(references) = object.arcs.filter(|nested| nested.depth > 0) else {
                return Ok(Geometry::empty());
            };
            if references.depth != depth - 1 {
                return Err(format!("map {kind} arcs are nested {}", references.depth));
            }
            let ring = shape == GeoShape::Polygon;
            let mut points = Vec::new();
            let mut rings = Vec::with_capacity(references.ends[0].len());
            let mut start = 0usize;
            for &end in &references.ends[0] {
                stitch(
                    arcs,
                    &references.indices[start..end as usize],
                    ring,
                    &mut points,
                )?;
                start = end as usize;
                rings.push(points.len() as u32);
            }
            let polygons = match (ring, references.depth) {
                (false, _) => Vec::new(),
                (true, 2) => vec![rings.len() as u32],
                (true, _) => references.ends[1].clone(),
            };
            if ring {
                open_rings(&mut points, &mut rings);
            }
            Geometry {
                shape,
                points,
                rings,
                polygons,
            }
        }
    };
    *positions += geometry.points.len();
    if *positions > MAX_GEO_POSITIONS {
        return Err("too many map positions".into());
    }
    if geometry.points.is_empty() {
        geometry = Geometry::empty();
    }
    Ok(geometry)
}

fn from_topology(
    topology: RawTopology,
    object: Option<&str>,
    naming: &Naming,
) -> Result<Vec<GeoFeature>, String> {
    if topology.kind.as_deref() != Some("Topology") {
        return Err("topojson needs type Topology".into());
    }
    let mut arcs = topology.arcs.unwrap_or_default();
    if arcs.depth != 0 && arcs.depth != 3 {
        return Err("topology arcs must be arrays of positions".into());
    }
    if let Some(transform) = &topology.transform {
        let mut start = 0usize;
        for &end in &arcs.ends[0] {
            let (mut x, mut y) = (0.0, 0.0);
            for point in &mut arcs.points[start..end as usize] {
                x += point[0];
                y += point[1];
                *point = [
                    x * transform.scale[0] + transform.translate[0],
                    y * transform.scale[1] + transform.translate[1],
                ];
            }
            start = end as usize;
        }
    }
    let mut objects = topology.objects;
    let index = match object {
        Some(name) => objects.iter().position(|(key, _)| key == name),
        None if objects.len() == 1 => Some(0),
        None => None,
    }
    .ok_or("topojson needs the name of one of its objects")?;
    let (_, root) = objects.swap_remove(index);
    let members = if root.kind.as_deref() == Some("GeometryCollection") {
        root.members.unwrap_or_default()
    } else {
        vec![root]
    };
    let mut positions = 0usize;
    members
        .into_iter()
        .enumerate()
        .map(|(index, member)| {
            let identity = naming.identity(&member, index);
            Ok(
                topology_geometry(&arcs, topology.transform.as_ref(), member, &mut positions)?
                    .into_feature(identity),
            )
        })
        .collect()
}

/// Feature values as `[{ id, value }]` rows or an `{ id: value }` object.
struct Values(Vec<GeoValue>);

impl<'de> Deserialize<'de> for Values {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ValuesVisitor;

        impl<'de> Visitor<'de> for ValuesVisitor {
            type Value = Values;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("map values as rows or an object keyed by feature ID")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Values, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<GeoValue>()? {
                    if values.len() >= super::super::MAX_LAYOUT_ROWS {
                        return Err(de::Error::custom("too many map values"));
                    }
                    values.push(value);
                }
                Ok(Values(values))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Values, A::Error> {
                let mut values = Vec::new();
                while let Some((id, value)) = map.next_entry::<String, f64>()? {
                    if values.len() >= super::super::MAX_LAYOUT_ROWS {
                        return Err(de::Error::custom("too many map values"));
                    }
                    values.push(GeoValue { id, value });
                }
                Ok(Values(values))
            }
        }

        deserializer.deserialize_any(ValuesVisitor)
    }
}

const FIELDS: &[&str] = &[
    "type",
    "geojson",
    "topojson",
    "object",
    "id_property",
    "label_property",
    "values",
    "projection",
    "scale",
    "missing_color",
];

impl<'de> Deserialize<'de> for GeoInput {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct InputVisitor;

        impl<'de> Visitor<'de> for InputVisitor {
            type Value = GeoInput;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map dataset")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<GeoInput, A::Error> {
                let mut geojson: Option<RawObject> = None;
                let mut topojson: Option<RawTopology> = None;
                let mut object: Option<String> = None;
                let mut id_property: Option<String> = None;
                let mut label_property: Option<String> = None;
                let mut values: Vec<GeoValue> = Vec::new();
                let mut projection = GeoProjection::default();
                let mut scale = ChoroplethScale::default();
                let mut missing_color: Option<String> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "geojson" => geojson = Some(map.next_value()?),
                        "topojson" => topojson = Some(map.next_value()?),
                        "object" => object = map.next_value()?,
                        "id_property" => id_property = map.next_value()?,
                        "label_property" => label_property = map.next_value()?,
                        "values" => values = map.next_value::<Values>()?.0,
                        "projection" => projection = map.next_value()?,
                        "scale" => scale = map.next_value()?,
                        "missing_color" => missing_color = map.next_value()?,
                        // The browser wire tags its layout data; map data decodes as this type
                        // directly so the tagged enum never buffers the whole topology.
                        "type" => {
                            let tag = map.next_value::<String>()?;
                            if tag != "geo" {
                                return Err(de::Error::invalid_value(
                                    de::Unexpected::Str(&tag),
                                    &"geo",
                                ));
                            }
                        }
                        other => return Err(de::Error::unknown_field(other, FIELDS)),
                    }
                }
                let naming = Naming {
                    id: id_property.as_deref(),
                    label: label_property.as_deref().unwrap_or(LABEL_PROPERTY),
                };
                let features = match (geojson, topojson) {
                    (Some(geojson), None) => from_geojson(geojson, &naming),
                    (None, Some(topojson)) => from_topology(topojson, object.as_deref(), &naming),
                    _ => Err("a map dataset needs exactly one of geojson or topojson".into()),
                }
                .map_err(de::Error::custom)?;
                Ok(GeoInput {
                    features,
                    values,
                    projection,
                    scale,
                    missing_color,
                })
            }
        }

        deserializer.deserialize_map(InputVisitor)
    }
}

/// Ring ranges of `ends[range]` in the position array, where `ends` are exclusive offsets.
fn ranges(ends: &[u32], range: Range<usize>) -> impl Iterator<Item = Range<usize>> + '_ {
    let start = range
        .start
        .checked_sub(1)
        .map_or(0, |previous| ends[previous] as usize);
    ends[range].iter().scan(start, |start, &end| {
        let ring = *start..end as usize;
        *start = end as usize;
        Some(ring)
    })
}

struct ClosedRing<'a>(&'a [[f64; 2]]);

impl Serialize for ClosedRing<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().chain(self.0.first()))
    }
}

/// The rings of one polygon: `rings[range]` of `feature`.
struct PolygonRings<'a> {
    feature: &'a GeoFeature,
    range: Range<usize>,
}

impl Serialize for PolygonRings<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let points = &self.feature.points;
        serializer.collect_seq(
            ranges(&self.feature.rings, self.range.clone()).map(|ring| ClosedRing(&points[ring])),
        )
    }
}

struct Coordinates<'a>(&'a GeoFeature);

impl Serialize for Coordinates<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let feature = self.0;
        let points = &feature.points;
        match feature.shape {
            GeoShape::Point if points.len() == 1 => points[0].serialize(serializer),
            GeoShape::Point | GeoShape::Empty => points.serialize(serializer),
            GeoShape::Line if feature.rings.len() == 1 => points.serialize(serializer),
            GeoShape::Line => serializer.collect_seq(
                ranges(&feature.rings, 0..feature.rings.len()).map(|line| &points[line]),
            ),
            GeoShape::Polygon => {
                if feature.polygons.len() == 1 {
                    PolygonRings {
                        feature,
                        range: 0..feature.rings.len(),
                    }
                    .serialize(serializer)
                } else {
                    serializer.collect_seq(
                        ranges(&feature.polygons, 0..feature.polygons.len())
                            .map(|range| PolygonRings { feature, range }),
                    )
                }
            }
        }
    }
}

struct GeometryOut<'a>(&'a GeoFeature);

impl Serialize for GeometryOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let feature = self.0;
        let kind = match (feature.shape, feature.points.len(), feature.rings.len()) {
            (GeoShape::Empty, ..) => return serializer.serialize_unit(),
            (GeoShape::Point, 1, _) => "Point",
            (GeoShape::Point, ..) => "MultiPoint",
            (GeoShape::Line, _, 1) => "LineString",
            (GeoShape::Line, ..) => "MultiLineString",
            (GeoShape::Polygon, ..) if feature.polygons.len() == 1 => "Polygon",
            (GeoShape::Polygon, ..) => "MultiPolygon",
        };
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("type", kind)?;
        map.serialize_entry("coordinates", &Coordinates(feature))?;
        map.end()
    }
}

struct Label<'a>(&'a str);

impl Serialize for Label<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry(LABEL_PROPERTY, self.0)?;
        map.end()
    }
}

struct FeatureOut<'a>(&'a GeoFeature);

impl Serialize for FeatureOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(4))?;
        map.serialize_entry("type", "Feature")?;
        map.serialize_entry("id", &self.0.id)?;
        map.serialize_entry("properties", &Label(&self.0.label))?;
        map.serialize_entry("geometry", &GeometryOut(self.0))?;
        map.end()
    }
}

struct Collection<'a>(&'a [GeoFeature]);

impl Serialize for Collection<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("type", "FeatureCollection")?;
        map.serialize_entry(
            "features",
            &self.0.iter().map(FeatureOut).collect::<Vec<_>>(),
        )?;
        map.end()
    }
}

impl Serialize for GeoInput {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(5))?;
        map.serialize_entry("geojson", &Collection(&self.features))?;
        map.serialize_entry("values", &self.values)?;
        map.serialize_entry("projection", &self.projection)?;
        map.serialize_entry("scale", &self.scale)?;
        map.serialize_entry("missing_color", &self.missing_color)?;
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(value: serde_json::Value) -> Result<GeoInput, String> {
        serde_json::from_value(value).map_err(|error| error.to_string())
    }

    #[test]
    fn topojson_decodes_to_the_equivalent_geojson_features() {
        // Two squares sharing the arc x = 11, quantized by scale [0.5, 0.25] and translate
        // [10, 20]; topojson-client 3 `feature()` yields the GeoJSON below.
        let topology = serde_json::json!({
            "topojson": {
                "type": "Topology",
                "transform": { "scale": [0.5, 0.25], "translate": [10, 20] },
                "objects": {
                    "other": { "type": "GeometryCollection", "geometries": [] },
                    "shapes": {
                        "type": "GeometryCollection",
                        "geometries": [
                            { "type": "Polygon", "id": "L", "properties": { "name": "Left" }, "arcs": [[1, 0]] },
                            { "type": "Polygon", "id": 7, "properties": { "name": "Right" }, "arcs": [[2, -1]] },
                            { "type": "LineString", "properties": { "name": "Edge" }, "arcs": [0] },
                            { "type": "Point", "id": "P", "properties": { "label": "Pin" }, "coordinates": [2, 2] },
                            { "type": null, "id": "N" }
                        ]
                    }
                },
                "arcs": [
                    [[2, 0], [0, 4]],
                    [[2, 4], [-2, 0], [0, -4], [2, 0]],
                    [[2, 0], [2, 0], [0, 4], [-2, 0]]
                ]
            },
            "object": "shapes",
            "values": { "L": 1.5 }
        });
        let geojson = serde_json::json!({
            "geojson": {
                "type": "FeatureCollection",
                "features": [
                    { "type": "Feature", "id": "L", "properties": { "name": "Left" }, "geometry": {
                        "type": "Polygon", "coordinates": [[[11, 21], [10, 21], [10, 20], [11, 20], [11, 21]]] } },
                    { "type": "Feature", "id": 7, "properties": { "name": "Right" }, "geometry": {
                        "coordinates": [[[11, 20], [12, 20], [12, 21], [11, 21], [11, 20]]], "type": "Polygon" } },
                    { "type": "Feature", "properties": { "name": "Edge" }, "geometry": {
                        "type": "LineString", "coordinates": [[11, 20], [11, 21]] } },
                    { "type": "Feature", "id": "P", "properties": { "label": "Pin" }, "geometry": {
                        "type": "Point", "coordinates": [11, 20.5] } },
                    { "type": "Feature", "id": "N", "properties": null, "geometry": null }
                ]
            },
            "values": [{ "id": "L", "value": 1.5 }]
        });
        let topo = decode(topology).unwrap();
        let geo = decode(geojson).unwrap();
        assert_eq!(topo, geo);
        let ids: Vec<&str> = geo
            .features
            .iter()
            .map(|feature| feature.id.as_str())
            .collect();
        assert_eq!(ids, ["L", "7", "#2", "P", "N"]);
        let labels: Vec<&str> = geo.features.iter().map(|f| f.label.as_str()).collect();
        assert_eq!(labels, ["Left", "Right", "Edge", "P", "N"]);
        assert_eq!(geo.features[0].points.len(), 4, "rings are stored open");
        assert_eq!(geo.features[1].shape, GeoShape::Polygon);
        assert_eq!(geo.features[2].shape, GeoShape::Line);
        assert_eq!(geo.features[3].points, [[11.0, 20.5]]);
        assert_eq!(geo.features[4].shape, GeoShape::Empty);
    }

    #[test]
    fn host_named_properties_choose_ids_and_labels() {
        let geo = decode(serde_json::json!({
            "geojson": { "type": "Feature", "id": "x", "properties": { "iso": "FR", "title": "France" },
                "geometry": { "type": "MultiPoint", "coordinates": [[1, 2], [3, 4]] } },
            "id_property": "iso",
            "label_property": "title"
        }))
        .unwrap();
        assert_eq!(
            (geo.features[0].id.as_str(), geo.features[0].label.as_str()),
            ("FR", "France")
        );
        assert_eq!(geo.features[0].shape, GeoShape::Point);
    }

    #[test]
    fn multipolygons_keep_polygon_and_hole_offsets_and_round_trip() {
        let source = serde_json::json!({
            "geojson": { "type": "MultiPolygon", "coordinates": [
                [[[0, 0], [4, 0], [4, 4], [0, 4], [0, 0]], [[1, 1], [1, 2], [2, 2], [1, 1]]],
                [[[5, 5], [6, 5], [6, 6], [5, 5]]]
            ] },
            "projection": { "type": "orthographic", "rotate": [10, 20, 0] },
            "scale": { "type": "diverging", "midpoint": 0.5 }
        });
        let mut tagged = source.clone();
        tagged["type"] = "geo".into();
        let geo = decode(source).unwrap();
        assert_eq!(decode(tagged).unwrap(), geo);
        let feature = &geo.features[0];
        assert_eq!(feature.rings, [4, 7, 10]);
        assert_eq!(feature.polygons, [2, 3]);
        let text = serde_json::to_string(&geo).unwrap();
        let again: GeoInput = serde_json::from_str(&text).unwrap();
        assert_eq!(again, geo);
        assert_eq!(serde_json::to_string(&again).unwrap(), text);
    }

    #[test]
    fn malformed_map_json_is_rejected() {
        let point = serde_json::json!({ "type": "Point", "coordinates": [0, 0] });
        let cases = [
            serde_json::json!({}),
            serde_json::json!({ "geojson": point, "topojson": { "type": "Topology", "objects": {}, "arcs": [] } }),
            serde_json::json!({ "geojson": point, "colour": "red" }),
            serde_json::json!({ "geojson": { "type": "GeometryCollection", "geometries": [point] } }),
            serde_json::json!({ "geojson": { "type": "Polygon", "coordinates": [[0, 0], [1, 1]] } }),
            serde_json::json!({ "geojson": { "type": "Point", "coordinates": [[0, 0]] } }),
            serde_json::json!({ "geojson": { "type": "Point", "coordinates": [0, "a"] } }),
            serde_json::json!({ "geojson": { "type": "Circle", "coordinates": [0, 0] } }),
            serde_json::json!({ "geojson": { "type": "FeatureCollection", "features": [point] } }),
            serde_json::json!({ "topojson": { "type": "Topology", "objects": { "a": {}, "b": {} }, "arcs": [] } }),
            serde_json::json!({ "topojson": { "type": "Topology", "objects": { "a": {
                "type": "LineString", "arcs": [5] } }, "arcs": [] } }),
            serde_json::json!({ "geojson": point, "values": { "a": "high" } }),
            serde_json::json!({ "type": "words", "geojson": point }),
        ];
        for case in cases {
            assert!(decode(case.clone()).is_err(), "{case}");
        }
    }

    #[test]
    fn decoding_stops_at_the_position_limit() {
        let positions = vec![[0.0, 0.0]; MAX_GEO_POSITIONS + 1];
        let source =
            serde_json::json!({ "geojson": { "type": "MultiPoint", "coordinates": positions } });
        let error = decode(source).unwrap_err();
        assert!(error.contains("too many map positions"), "{error}");
    }
}
