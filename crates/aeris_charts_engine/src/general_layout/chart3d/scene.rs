//! 3D scene construction (complete build): the shared axis box of a 3D pane, the projection of
//! every visible 3D series through the pane camera, painter's-algorithm depth ordering, shading,
//! picking, and the camera operations the input controller drives.
//!
//! One cache per pane, held by the pane's first 3D series, stores the projected and depth-sorted
//! triangles of every visible 3D series as one per-triangle-colored mesh, the owner of each
//! triangle for picking, and the projected box: back-face grid lines, axis lines, ticks and
//! labels. A camera, plot, series set or series color change rebuilds it; a dataset replacement
//! clears the caches of its pane.
//!
//! Depth ordering sorts whole pieces, never single triangles: one bar box, one scatter symbol,
//! one half segment of a line, or one surface cell with its wireframe. Pieces paint far to near
//! by view depth with series order then input order as the tie-break, so equal input gives an
//! identical frame on every executor. Bars sort by the horizontal distance of their footprint
//! from the eye, which orders disjoint vertical prisms correctly at any height. The painter's
//! algorithm cannot resolve pieces that intersect: two surfaces crossing each other, or a bar
//! piercing a surface, show the nearer piece's whole cell on top. Every color is painted opaque.
//!
//! Level of detail: a surface draws at most 64 by 64 cells, striding evenly through longer grids
//! (every sample stays addressable and hittable at the nearest drawn corner), and one pane holds
//! at most `MAX_SCENE_TRIANGLES` triangles: a piece that would cross the bound is dropped whole,
//! as is every piece after it, first in build order and then, after plot clipping splits
//! triangles, in paint order.

use std::sync::Arc;

use aeris_charts_core::scale::general_scale::{LinearScale, nice_linear_domain};
use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{LineStyle, LineType, Prim, TextAlign};

use super::super::geo::{push_clipped_polyline, push_clipped_triangle};
use super::super::{
    GeneralLayoutElement, GeneralLayoutHit, GeneralLayoutInput, GeneralLayoutSeriesKind,
    GeneralLayoutStore, LayoutSeriesEntry, layout_value_text, point_in_triangle,
};
use super::camera::{Camera3d, NEAR, View3d, validate_camera};
use super::{Axis3d, Points3dInput, SurfaceInput};
use crate::frame::interpolate_color;
use crate::general_axes::GeneralPlotRect;
use crate::{ChartEngine, ChartKey, GeneralLayoutDatasetId, GeneralLayoutSeriesId, PaneId};

/// Half the side of the axis box in box units (ECharts GL `boxWidth` 100).
const HALF: f64 = 50.0;
/// Triangles one pane's scene may hold.
pub(super) const MAX_SCENE_TRIANGLES: usize = 131_072;
/// Cells drawn along one side of a surface.
const MAX_DRAWN_CELLS: usize = 64;
/// Value axis intervals (ECharts `splitNumber`).
const AXIS_SPLITS: usize = 5;
/// Category labels drawn along one axis; longer category lists label every n-th category.
const MAX_CATEGORY_LABELS: usize = 12;
const SCATTER_SIZE: f64 = 10.0;
const SCATTER_SEGMENTS: usize = 16;
const LINE_WIDTH: f64 = 2.0;
const WIREFRAME_WIDTH: f64 = 1.0;
const GRID_WIDTH: f64 = 1.0;
const AXIS_WIDTH: f64 = 1.0;
const TEXT_SIZE: f64 = 12.0;
const TICK_LENGTH: f64 = 5.0;
const LABEL_MARGIN: f64 = 8.0;
const NAME_MARGIN: f64 = 32.0;
/// Bar footprint as a share of a category band, and of the default value-axis spacing.
const BAR_BAND_SHARE: f64 = 0.7;
const BAR_VALUE_SHARE: f64 = 0.6;
/// Lambert shading: `ambient + diffuse * max(n · l, 0)` under a world-fixed light placed like
/// ECharts GL's main light (`alpha` 30, `beta` 40).
const AMBIENT: f64 = 0.35;
const DIFFUSE: f64 = 0.65;
const LIGHT_ALPHA: f64 = 30.0;
const LIGHT_BETA: f64 = 40.0;
/// Zoom of one zoom key, one wheel notch (`2^(1/5)`).
const KEY_ZOOM: f64 = 1.148_698_354_997_035;
/// Retained bytes of one mesh triangle: three corners, a color, and an owner.
const TRIANGLE_BYTES: usize =
    3 * std::mem::size_of::<[f32; 2]>() + std::mem::size_of::<Color>() + std::mem::size_of::<u32>();

impl GeneralLayoutSeriesKind {
    pub(in crate::general_layout) fn is_3d(self) -> bool {
        matches!(
            self,
            Self::Bar3d | Self::Scatter3d | Self::Line3d | Self::Surface3d
        )
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalized(v: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(v, v).sqrt();
    (length > 1e-12 && length.is_finite()).then(|| v.map(|axis| axis / length))
}

fn lerp3(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn light() -> [f64; 3] {
    let (alpha, beta) = (LIGHT_ALPHA.to_radians(), LIGHT_BETA.to_radians());
    [
        alpha.cos() * beta.sin(),
        alpha.sin(),
        alpha.cos() * beta.cos(),
    ]
}

fn opaque(color: Color) -> Color {
    Color::rgb(color.r(), color.g(), color.b())
}

fn shade(color: Color, normal: [f64; 3]) -> Color {
    let k = AMBIENT + DIFFUSE * dot(normal, light()).max(0.0);
    let channel = |value: u8| (f64::from(value) * k).round().clamp(0.0, 255.0) as u8;
    Color::rgb(channel(color.r()), channel(color.g()), channel(color.b()))
}

/// The data of one 3D series.
#[derive(Clone, Copy)]
pub(in crate::general_layout) enum SceneData<'a> {
    Points(&'a Points3dInput),
    Surface(&'a SurfaceInput),
}

impl<'a> SceneData<'a> {
    fn of(input: &'a GeneralLayoutInput) -> Option<Self> {
        match input {
            GeneralLayoutInput::Points3d(input) => Some(Self::Points(input)),
            GeneralLayoutInput::Surface(input) => Some(Self::Surface(input)),
            _ => None,
        }
    }

    fn axes(self) -> [&'a Axis3d; 3] {
        match self {
            Self::Points(input) => [&input.x_axis, &input.y_axis, &input.z_axis],
            Self::Surface(input) => [&input.x_axis, &input.y_axis, &input.z_axis],
        }
    }

    /// Every value along data axis `axis`.
    fn values(self, axis: usize) -> Box<dyn Iterator<Item = f64> + 'a> {
        match self {
            Self::Points(input) => Box::new(input.points.iter().map(move |point| match axis {
                0 => point.x,
                1 => point.y,
                _ => point.z,
            })),
            Self::Surface(input) => match axis {
                0 => Box::new(input.x.iter().copied()),
                1 => Box::new(input.y.iter().copied()),
                _ => Box::new(input.z.iter().flatten().copied()),
            },
        }
    }

    /// Data coordinates of item `row`.
    fn position(self, row: usize) -> Option<[f64; 3]> {
        match self {
            Self::Points(input) => {
                let point = input.points.get(row)?;
                Some([point.x, point.y, point.z])
            }
            Self::Surface(input) => {
                let columns = input.x.len().max(1);
                let z = (*input.z.get(row)?)?;
                Some([
                    *input.x.get(row % columns)?,
                    *input.y.get(row / columns)?,
                    z,
                ])
            }
        }
    }
}

/// One visible 3D series of a pane.
pub(in crate::general_layout) struct SceneMember<'a> {
    id: GeneralLayoutSeriesId,
    kind: GeneralLayoutSeriesKind,
    dataset: GeneralLayoutDatasetId,
    color: Color,
    data: SceneData<'a>,
}

impl SceneMember<'_> {
    fn element(&self) -> GeneralLayoutElement {
        match self.data {
            SceneData::Points(_) => GeneralLayoutElement::Point,
            SceneData::Surface(_) => GeneralLayoutElement::Sample,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct SceneKey {
    plot: GeneralPlotRect,
    camera: Camera3d,
    /// Every visible 3D series of the pane: a change of any of these rebuilds the scene.
    members: Vec<(
        GeneralLayoutSeriesId,
        GeneralLayoutSeriesKind,
        GeneralLayoutDatasetId,
        Color,
    )>,
}

/// One axis of the box: its domain, ticks, and title.
#[derive(Clone, Debug)]
struct AxisScale {
    from: f64,
    to: f64,
    category: bool,
    ticks: Vec<(f64, String)>,
    name: String,
}

impl AxisScale {
    fn new(
        axis: &Axis3d,
        fallback: &str,
        values: impl Iterator<Item = f64>,
        categories: bool,
    ) -> Self {
        let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
        for value in values {
            low = low.min(value);
            high = high.max(value);
        }
        let name = if axis.name.is_empty() {
            fallback.to_owned()
        } else {
            axis.name.clone()
        };
        if categories && !axis.categories.is_empty() {
            let indexed = if high.is_finite() && high >= 0.0 {
                high.min(4_095.0) as usize + 1
            } else {
                0
            };
            let count = axis.categories.len().max(indexed);
            let stride = count.div_ceil(MAX_CATEGORY_LABELS).max(1);
            let ticks = (0..count)
                .step_by(stride)
                .map(|index| {
                    let label = axis
                        .categories
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| index.to_string());
                    (index as f64, label)
                })
                .collect();
            return Self {
                from: -0.5,
                to: count as f64 - 0.5,
                category: true,
                ticks,
                name,
            };
        }
        if low.is_nan() || high.is_nan() || low > high {
            (low, high) = (0.0, 1.0);
        }
        // ECharts value axes default to `scale: false`, which keeps zero in the extent.
        let (low, high) = (low.min(0.0), high.max(0.0));
        let [from, to] = if low < high {
            nice_linear_domain(low, high, AXIS_SPLITS)
        } else {
            [0.0, 1.0]
        };
        let ticks = LinearScale::new(from, to, 0.0, 1.0)
            .map(|scale| scale.ticks(AXIS_SPLITS))
            .unwrap_or_default()
            .into_iter()
            .map(|value| (value, layout_value_text(Some(value))))
            .collect();
        Self {
            from,
            to,
            category: false,
            ticks,
            name,
        }
    }

    fn unit(&self, value: f64) -> f64 {
        (value - self.from) / (self.to - self.from)
    }
}

/// World coordinate along the world axis that data axis `axis` maps to: data `x` runs left to
/// right, data `y` from near to far, and data `z` up.
fn world_coordinate(axis: usize, unit: f64) -> f64 {
    match axis {
        1 => HALF * (1.0 - 2.0 * unit),
        _ => HALF * (2.0 * unit - 1.0),
    }
}

/// The data axis a world axis shows.
const DATA_AXIS_OF_WORLD: [usize; 3] = [0, 2, 1];

#[derive(Clone, Copy, Debug)]
struct SceneItem {
    member: u16,
    count: u8,
    /// Item rows: one for a point or line half, the four corners of a surface cell.
    rows: [u32; 4],
}

struct SceneLabel {
    text: String,
    at: [f64; 2],
    align: TextAlign,
}

/// The projected, depth-sorted scene of one 3D pane.
pub(in crate::general_layout) struct SceneCache {
    key: SceneKey,
    view: View3d,
    axes: [AxisScale; 3],
    /// Triangle corners in paint order, media px.
    points: Vec<[f32; 2]>,
    colors: Arc<[Color]>,
    /// Item of each triangle, an index into `items`.
    owners: Vec<u32>,
    items: Vec<SceneItem>,
    /// Back-face split lines, screen segments.
    grid: Vec<[[f64; 2]; 2]>,
    /// Axis lines and ticks, screen segments.
    lines: Vec<[[f64; 2]; 2]>,
    labels: Vec<SceneLabel>,
}

/// Reserved cache bytes of one 3D series: its own triangles and items within the pane bound.
/// The pane's one cache never holds more than `MAX_SCENE_TRIANGLES` triangles, however many
/// series share it or clipping splits.
pub(in crate::general_layout) fn cache_reservation(
    kind: GeneralLayoutSeriesKind,
    input: &GeneralLayoutInput,
) -> usize {
    let (triangles, items) = match (kind, input) {
        (GeneralLayoutSeriesKind::Bar3d, GeneralLayoutInput::Points3d(input)) => {
            (12 * input.points.len(), input.points.len())
        }
        (GeneralLayoutSeriesKind::Scatter3d, GeneralLayoutInput::Points3d(input)) => {
            (SCATTER_SEGMENTS * input.points.len(), input.points.len())
        }
        (GeneralLayoutSeriesKind::Line3d, GeneralLayoutInput::Points3d(input)) => {
            (4 * input.points.len(), 2 * input.points.len())
        }
        (GeneralLayoutSeriesKind::Surface3d, GeneralLayoutInput::Surface(input)) => {
            let cells = lod_indices(input.x.len()).len().saturating_sub(1)
                * lod_indices(input.y.len()).len().saturating_sub(1);
            (10 * cells, cells)
        }
        _ => return 0,
    };
    triangles.min(MAX_SCENE_TRIANGLES) * TRIANGLE_BYTES + items * std::mem::size_of::<SceneItem>()
}

/// Grid indices a surface side draws: evenly strided to at most `MAX_DRAWN_CELLS` cells, always
/// ending on the last sample.
fn lod_indices(len: usize) -> Vec<usize> {
    if len < 2 {
        return Vec::new();
    }
    let stride = (len - 1).div_ceil(MAX_DRAWN_CELLS).max(1);
    let mut indices: Vec<usize> = (0..len).step_by(stride).collect();
    if indices.last() != Some(&(len - 1)) {
        indices.push(len - 1);
    }
    indices
}

/// A run of triangles that sorts as one unit.
struct Piece {
    depth: f64,
    order: u32,
    start: u32,
    end: u32,
}

struct Builder<'v> {
    view: &'v View3d,
    triangles: Vec<[[f64; 2]; 3]>,
    colors: Vec<Color>,
    owners: Vec<u32>,
    pieces: Vec<Piece>,
    items: Vec<SceneItem>,
    start: usize,
    /// Set once a triangle was refused at the bound; no later piece is kept.
    full: bool,
}

impl Builder<'_> {
    fn begin(&mut self) {
        self.start = self.triangles.len();
    }

    fn end(&mut self, depth: f64) {
        if self.triangles.len() > self.start && depth.is_finite() && !self.full {
            self.pieces.push(Piece {
                depth,
                order: self.pieces.len() as u32,
                start: self.start as u32,
                end: self.triangles.len() as u32,
            });
        } else {
            self.discard();
        }
    }

    fn discard(&mut self) {
        self.triangles.truncate(self.start);
        self.colors.truncate(self.start);
        self.owners.truncate(self.start);
    }

    fn item(&mut self, member: usize, rows: &[usize]) -> u32 {
        let mut item = SceneItem {
            member: member as u16,
            count: rows.len() as u8,
            rows: [0; 4],
        };
        for (slot, &row) in item.rows.iter_mut().zip(rows) {
            *slot = row as u32;
        }
        self.items.push(item);
        (self.items.len() - 1) as u32
    }

    fn screen_triangle(&mut self, corners: [[f64; 2]; 3], color: Color, owner: u32) {
        if !corners.iter().flatten().all(|value| value.is_finite()) {
            return;
        }
        if self.triangles.len() >= MAX_SCENE_TRIANGLES {
            self.full = true;
            return;
        }
        self.triangles.push(corners);
        self.colors.push(color);
        self.owners.push(owner);
    }

    /// A view-space triangle clipped to the near plane and projected.
    fn view_triangle(&mut self, corners: [[f64; 3]; 3], color: Color, owner: u32) {
        let view = self.view;
        if corners.iter().all(|&v| view.visible(v)) {
            self.screen_triangle(corners.map(|v| view.screen(v)), color, owner);
            return;
        }
        let mut polygon = [[0.0; 3]; 4];
        let mut count = 0;
        for index in 0..3 {
            let (a, b) = (corners[index], corners[(index + 1) % 3]);
            if view.visible(a) {
                polygon[count] = a;
                count += 1;
            }
            if view.visible(a) != view.visible(b) {
                polygon[count] = lerp3(a, b, (NEAR - a[2]) / (b[2] - a[2]));
                count += 1;
            }
        }
        for index in 1..count.saturating_sub(1) {
            self.screen_triangle(
                [polygon[0], polygon[index], polygon[index + 1]].map(|v| view.screen(v)),
                color,
                owner,
            );
        }
    }

    /// A screen segment `width` px wide as a quad.
    fn screen_segment(&mut self, a: [f64; 2], b: [f64; 2], width: f64, color: Color, owner: u32) {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length = dx.hypot(dy);
        if length.is_nan() || length <= 1e-9 {
            return;
        }
        let (nx, ny) = (-dy / length * width / 2.0, dx / length * width / 2.0);
        let corners = [
            [a[0] + nx, a[1] + ny],
            [b[0] + nx, b[1] + ny],
            [b[0] - nx, b[1] - ny],
            [a[0] - nx, a[1] - ny],
        ];
        self.screen_triangle([corners[0], corners[1], corners[2]], color, owner);
        self.screen_triangle([corners[0], corners[2], corners[3]], color, owner);
    }
}

/// The part of view segment `a`-`b` in front of the near plane.
fn clip_near(view: &View3d, a: [f64; 3], b: [f64; 3]) -> Option<([f64; 3], [f64; 3])> {
    match (view.visible(a), view.visible(b)) {
        (true, true) => Some((a, b)),
        (false, false) => None,
        (true, false) => Some((a, lerp3(a, b, (NEAR - a[2]) / (b[2] - a[2])))),
        (false, true) => Some((lerp3(a, b, (NEAR - a[2]) / (b[2] - a[2])), b)),
    }
}

fn parse_or(color: Option<&str>, fallback: Color) -> Color {
    opaque(color.and_then(Color::parse_css).unwrap_or(fallback))
}

/// The color a surface paints at height `z`: its color map stops spread evenly over the
/// surface's own height extent, else the series color.
fn surface_color(input: &SurfaceInput, series: Color, extent: (f64, f64), z: f64) -> Color {
    let stops: Vec<Color> = input
        .colors
        .iter()
        .filter_map(|color| Color::parse_css(color))
        .collect();
    match stops.len() {
        0 => opaque(series),
        1 => opaque(stops[0]),
        count => {
            let (low, high) = extent;
            let t = if high > low {
                ((z - low) / (high - low)).clamp(0.0, 1.0)
            } else {
                0.5
            };
            let position = t * (count - 1) as f64;
            let index = (position.floor() as usize).min(count - 2);
            opaque(interpolate_color(
                stops[index],
                stops[index + 1],
                position - index as f64,
            ))
        }
    }
}

fn height_extent(input: &SurfaceInput) -> (f64, f64) {
    input
        .z
        .iter()
        .flatten()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), &z| {
            (low.min(z), high.max(z))
        })
}

impl SceneCache {
    fn new(key: SceneKey, axes: [&Axis3d; 3], members: &[SceneMember<'_>]) -> Self {
        let rect = [
            key.plot.x,
            key.plot.y,
            key.plot.x + key.plot.width,
            key.plot.y + key.plot.height,
        ];
        let view = View3d::new(&key.camera, rect);
        let scales = [0, 1, 2].map(|axis| {
            AxisScale::new(
                axes[axis],
                ["x", "y", "z"][axis],
                members.iter().flat_map(|member| member.data.values(axis)),
                axis < 2,
            )
        });
        let mut builder = Builder {
            view: &view,
            triangles: Vec::new(),
            colors: Vec::new(),
            owners: Vec::new(),
            pieces: Vec::new(),
            items: Vec::new(),
            start: 0,
            full: false,
        };
        let world = |[x, y, z]: [f64; 3]| {
            [
                world_coordinate(0, scales[0].unit(x)),
                world_coordinate(2, scales[2].unit(z)),
                world_coordinate(1, scales[1].unit(y)),
            ]
        };
        for (index, member) in members.iter().enumerate() {
            match (member.kind, member.data) {
                (GeneralLayoutSeriesKind::Bar3d, SceneData::Points(input)) => {
                    push_bars(&mut builder, index, member, input, &scales, &world);
                }
                (GeneralLayoutSeriesKind::Scatter3d, SceneData::Points(input)) => {
                    push_scatter(&mut builder, index, member, input, &world);
                }
                (GeneralLayoutSeriesKind::Line3d, SceneData::Points(input)) => {
                    push_line(&mut builder, index, member, input, &world);
                }
                (GeneralLayoutSeriesKind::Surface3d, SceneData::Surface(input)) => {
                    push_surface(&mut builder, index, member, input, &world);
                }
                _ => {}
            }
        }
        let Builder {
            triangles,
            colors: piece_colors,
            owners: piece_owners,
            mut pieces,
            items,
            ..
        } = builder;
        pieces.sort_by(|a, b| b.depth.total_cmp(&a.depth).then(a.order.cmp(&b.order)));
        let identity = |[x, y]: [f64; 2]| [x as f32, y as f32];
        let mut points = Vec::with_capacity(triangles.len() * 3);
        let mut colors = Vec::with_capacity(triangles.len());
        let mut owners = Vec::with_capacity(triangles.len());
        for piece in &pieces {
            let kept = colors.len();
            for index in piece.start as usize..piece.end as usize {
                push_clipped_triangle(triangles[index], rect, &identity, &mut points);
                let added = points.len() / 3 - colors.len();
                colors.extend(std::iter::repeat_n(piece_colors[index], added));
                owners.extend(std::iter::repeat_n(piece_owners[index], added));
            }
            if colors.len() > MAX_SCENE_TRIANGLES {
                colors.truncate(kept);
                owners.truncate(kept);
                points.truncate(kept * 3);
                break;
            }
        }
        let mut cache = Self {
            key,
            view,
            axes: scales,
            points,
            colors: colors.into(),
            owners,
            items,
            grid: Vec::new(),
            lines: Vec::new(),
            labels: Vec::new(),
        };
        cache.build_box();
        cache
    }

    /// Projects one world segment, clipped to the near plane.
    fn segment(&self, a: [f64; 3], b: [f64; 3]) -> Option<[[f64; 2]; 2]> {
        let (a, b) = clip_near(&self.view, self.view.view_point(a), self.view.view_point(b))?;
        Some([self.view.screen(a), self.view.screen(b)])
    }

    /// Back-face split lines, then the three axis lines with their ticks, labels and names.
    fn build_box(&mut self) {
        for world_axis in 0..3 {
            for sign in [-1.0, 1.0] {
                let mut normal = [0.0; 3];
                normal[world_axis] = sign;
                if self.view.faces_eye(normal.map(|axis| axis * HALF), normal) {
                    continue;
                }
                for across in (0..3).filter(|&axis| axis != world_axis) {
                    let along = 3 - world_axis - across;
                    let data_axis = DATA_AXIS_OF_WORLD[across];
                    for index in 0..self.axes[data_axis].ticks.len() {
                        let value = self.axes[data_axis].ticks[index].0;
                        let coordinate =
                            world_coordinate(data_axis, self.axes[data_axis].unit(value));
                        let mut a = [0.0; 3];
                        a[world_axis] = sign * HALF;
                        a[across] = coordinate;
                        let mut b = a;
                        a[along] = -HALF;
                        b[along] = HALF;
                        if let Some(segment) = self.segment(a, b) {
                            self.grid.push(segment);
                        }
                    }
                }
            }
        }
        let faces = |axis: usize| {
            let mut normal = [0.0; 3];
            normal[axis] = 1.0;
            self.view
                .faces_eye(normal.map(|value| value * HALF), normal)
        };
        // The floor is the horizontal face turned away from the eye; the x and y axes run along
        // its edges nearest the eye and the z axis rises at the far end of the x axis.
        let floor = if self.view.faces_eye([0.0, -HALF, 0.0], [0.0, -1.0, 0.0]) {
            HALF
        } else {
            -HALF
        };
        let near_x = if faces(0) { HALF } else { -HALF };
        let near_z = if faces(2) { HALF } else { -HALF };
        let axis_point = |axis: usize, unit: f64| match axis {
            0 => [world_coordinate(0, unit), floor, near_z],
            1 => [near_x, floor, world_coordinate(1, unit)],
            _ => [-near_x, world_coordinate(2, unit), near_z],
        };
        let Some(center) = self.view.project([0.0; 3]) else {
            return;
        };
        for axis in 0..3 {
            let Some([s0, s1]) = self.segment(axis_point(axis, 0.0), axis_point(axis, 1.0)) else {
                continue;
            };
            self.lines.push([s0, s1]);
            let mid = [(s0[0] + s1[0]) / 2.0, (s0[1] + s1[1]) / 2.0];
            let outward = [mid[0] - center[0], mid[1] - center[1]];
            let (dx, dy) = (s1[0] - s0[0], s1[1] - s0[1]);
            let length = dx.hypot(dy);
            let mut normal = if length > 1e-6 {
                [-dy / length, dx / length]
            } else {
                let size = outward[0].hypot(outward[1]);
                if size <= 1e-6 {
                    continue;
                }
                [outward[0] / size, outward[1] / size]
            };
            if normal[0] * outward[0] + normal[1] * outward[1] < 0.0 {
                normal = [-normal[0], -normal[1]];
            }
            let scale = &self.axes[axis];
            let mut labels = Vec::with_capacity(scale.ticks.len() + 1);
            for (value, text) in &scale.ticks {
                let Some(p) = self.view.project(axis_point(axis, scale.unit(*value))) else {
                    continue;
                };
                self.lines.push([
                    p,
                    [
                        p[0] + normal[0] * TICK_LENGTH,
                        p[1] + normal[1] * TICK_LENGTH,
                    ],
                ]);
                labels.push(label(text.clone(), p, normal, LABEL_MARGIN));
            }
            labels.push(label(scale.name.clone(), mid, normal, NAME_MARGIN));
            self.labels.extend(labels);
        }
    }

    pub(in crate::general_layout) fn heap_bytes(&self) -> usize {
        self.key.members.capacity()
            * std::mem::size_of::<(
                GeneralLayoutSeriesId,
                GeneralLayoutSeriesKind,
                GeneralLayoutDatasetId,
                Color,
            )>()
            + self
                .axes
                .iter()
                .map(|axis| {
                    axis.name.capacity()
                        + axis.ticks.capacity() * std::mem::size_of::<(f64, String)>()
                        + axis
                            .ticks
                            .iter()
                            .map(|(_, text)| text.capacity())
                            .sum::<usize>()
                })
                .sum::<usize>()
            + self.points.capacity() * std::mem::size_of::<[f32; 2]>()
            + self.colors.len() * std::mem::size_of::<Color>()
            + self.owners.capacity() * std::mem::size_of::<u32>()
            + self.items.capacity() * std::mem::size_of::<SceneItem>()
            + (self.grid.capacity() + self.lines.capacity()) * std::mem::size_of::<[[f64; 2]; 2]>()
            + self.labels.capacity() * std::mem::size_of::<SceneLabel>()
            + self
                .labels
                .iter()
                .map(|label| label.text.capacity())
                .sum::<usize>()
    }

    fn world(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        [
            world_coordinate(0, self.axes[0].unit(x)),
            world_coordinate(2, self.axes[2].unit(z)),
            world_coordinate(1, self.axes[1].unit(y)),
        ]
    }

    fn contains(&self, [x, y]: [f64; 2]) -> bool {
        let plot = self.key.plot;
        x >= plot.x && x <= plot.x + plot.width && y >= plot.y && y <= plot.y + plot.height
    }

    /// Screen position of item `row` of `data` (a bar's top center, a symbol or line vertex, a
    /// surface sample), or `None` outside the plot or behind the eye.
    fn anchor(&self, data: SceneData<'_>, row: usize) -> Option<[f64; 2]> {
        let point = self.view.project(self.world(data.position(row)?))?;
        self.contains(point).then_some(point)
    }

    /// The front-most item under `(x, y)`: the last painted triangle that contains it.
    fn hit(&self, members: &[SceneMember<'_>], x: f64, y: f64) -> Option<GeneralLayoutHit> {
        if !self.contains([x, y]) {
            return None;
        }
        let (px, py) = (x as f32, y as f32);
        let triangles = self.points.as_chunks::<3>().0;
        let owner = triangles
            .iter()
            .zip(&self.owners)
            .rev()
            .find(|([a, b, c], _)| {
                px >= a[0].min(b[0]).min(c[0])
                    && px <= a[0].max(b[0]).max(c[0])
                    && py >= a[1].min(b[1]).min(c[1])
                    && py <= a[1].max(b[1]).max(c[1])
                    && point_in_triangle(
                        (x, y),
                        (f64::from(a[0]), f64::from(a[1])),
                        (f64::from(b[0]), f64::from(b[1])),
                        (f64::from(c[0]), f64::from(c[1])),
                    )
            })
            .map(|(_, &owner)| owner)?;
        let item = self.items.get(owner as usize)?;
        let member = members.get(usize::from(item.member))?;
        let rows = &item.rows[..usize::from(item.count)];
        let row = rows.iter().map(|&row| row as usize).min_by(|&a, &b| {
            let distance = |row: usize| {
                self.view
                    .project(self.world(member.data.position(row).unwrap_or([f64::NAN; 3])))
                    .map_or(f64::INFINITY, |[sx, sy]| (sx - x).hypot(sy - y))
            };
            distance(a).total_cmp(&distance(b)).then(a.cmp(&b))
        })?;
        Some(GeneralLayoutHit {
            series: member.id,
            element: member.element(),
            row,
        })
    }
}

fn label(text: String, at: [f64; 2], normal: [f64; 2], margin: f64) -> SceneLabel {
    let mut at = [at[0] + normal[0] * margin, at[1] + normal[1] * margin];
    let align = if normal[0].abs() < 0.5 {
        at[1] += normal[1].signum() * TEXT_SIZE / 2.0;
        TextAlign::Center
    } else if normal[0] > 0.0 {
        TextAlign::Left
    } else {
        TextAlign::Right
    };
    SceneLabel { text, at, align }
}

/// Bars as boxes from zero to each value, a footprint of `size` box units (default 70% of a
/// category band, else 60% of the even spacing of the series' point count).
fn push_bars(
    builder: &mut Builder<'_>,
    member_index: usize,
    member: &SceneMember<'_>,
    input: &Points3dInput,
    scales: &[AxisScale; 3],
    world: &impl Fn([f64; 3]) -> [f64; 3],
) {
    let spacing = (2.0 * HALF / (input.points.len().max(1) as f64).sqrt()).round();
    let half_size = |scale: &AxisScale| {
        input.size.unwrap_or(if scale.category {
            2.0 * HALF / (scale.to - scale.from) * BAR_BAND_SHARE
        } else {
            spacing * BAR_VALUE_SHARE
        }) / 2.0
    };
    let (half_x, half_z) = (half_size(&scales[0]), half_size(&scales[1]));
    let view = builder.view;
    let eye = view.eye();
    for (row, point) in input.points.iter().enumerate() {
        let owner = builder.item(member_index, &[row]);
        let top = world([point.x, point.y, point.z]);
        let base = world([point.x, point.y, 0.0])[1];
        let (y0, y1) = (base.min(top[1]), base.max(top[1]));
        if y1 - y0 <= 0.0 {
            continue;
        }
        let (x0, x1) = (top[0] - half_x, top[0] + half_x);
        let (z0, z1) = (top[2] - half_z, top[2] + half_z);
        let color = parse_or(point.color.as_deref(), member.color);
        builder.begin();
        let faces: [([f64; 3], [[f64; 3]; 4]); 6] = [
            (
                [1.0, 0.0, 0.0],
                [[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]],
            ),
            (
                [-1.0, 0.0, 0.0],
                [[x0, y0, z1], [x0, y1, z1], [x0, y1, z0], [x0, y0, z0]],
            ),
            (
                [0.0, 1.0, 0.0],
                [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
            ),
            (
                [0.0, -1.0, 0.0],
                [[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]],
            ),
            (
                [0.0, 0.0, 1.0],
                [[x1, y0, z1], [x1, y1, z1], [x0, y1, z1], [x0, y0, z1]],
            ),
            (
                [0.0, 0.0, -1.0],
                [[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]],
            ),
        ];
        for (normal, corners) in faces {
            let middle = lerp3(corners[0], corners[2], 0.5);
            if !view.faces_eye(middle, normal) {
                continue;
            }
            let shaded = shade(color, normal);
            let v = corners.map(|corner| view.view_point(corner));
            builder.view_triangle([v[0], v[1], v[2]], shaded, owner);
            builder.view_triangle([v[0], v[2], v[3]], shaded, owner);
        }
        let depth = if eye[0].abs() < 900.0 {
            (top[0] - eye[0]).hypot(top[2] - eye[2])
        } else {
            view.view_point([top[0], 0.0, top[2]])[2]
        };
        builder.end(depth);
    }
}

/// Screen-sized flat symbols of `size` px diameter (default 10).
fn push_scatter(
    builder: &mut Builder<'_>,
    member_index: usize,
    member: &SceneMember<'_>,
    input: &Points3dInput,
    world: &impl Fn([f64; 3]) -> [f64; 3],
) {
    let radius = input.size.unwrap_or(SCATTER_SIZE) / 2.0;
    let view = builder.view;
    let ring: Vec<[f64; 2]> = (0..SCATTER_SEGMENTS)
        .map(|index| {
            let angle = std::f64::consts::TAU * index as f64 / SCATTER_SEGMENTS as f64;
            [radius * angle.cos(), radius * angle.sin()]
        })
        .collect();
    for (row, point) in input.points.iter().enumerate() {
        let owner = builder.item(member_index, &[row]);
        let v = view.view_point(world([point.x, point.y, point.z]));
        if !view.visible(v) {
            continue;
        }
        let [cx, cy] = view.screen(v);
        let color = parse_or(point.color.as_deref(), member.color);
        builder.begin();
        for index in 0..SCATTER_SEGMENTS {
            let (a, b) = (ring[index], ring[(index + 1) % SCATTER_SEGMENTS]);
            builder.screen_triangle(
                [[cx, cy], [cx + a[0], cy + a[1]], [cx + b[0], cy + b[1]]],
                color,
                owner,
            );
        }
        builder.end(v[2]);
    }
}

/// A polyline through the rows in input order, `size` px wide (default 2). Each segment paints
/// as two halves owned by their nearer vertex, so picking a line names its nearest point.
fn push_line(
    builder: &mut Builder<'_>,
    member_index: usize,
    member: &SceneMember<'_>,
    input: &Points3dInput,
    world: &impl Fn([f64; 3]) -> [f64; 3],
) {
    let width = input.size.unwrap_or(LINE_WIDTH);
    let view = builder.view;
    let owners: Vec<u32> = (0..input.points.len())
        .map(|row| builder.item(member_index, &[row]))
        .collect();
    for (row, pair) in input.points.windows(2).enumerate() {
        let [a, b] =
            [&pair[0], &pair[1]].map(|point| view.view_point(world([point.x, point.y, point.z])));
        let Some((a, b)) = clip_near(view, a, b) else {
            continue;
        };
        let middle = lerp3(a, b, 0.5);
        let color = parse_or(pair[0].color.as_deref(), member.color);
        let (sa, sm, sb) = (view.screen(a), view.screen(middle), view.screen(b));
        builder.begin();
        builder.screen_segment(sa, sm, width, color, owners[row]);
        builder.end(lerp3(a, middle, 0.5)[2]);
        builder.begin();
        builder.screen_segment(sm, sb, width, color, owners[row + 1]);
        builder.end(lerp3(middle, b, 0.5)[2]);
    }
}

/// Shaded, two-sided cells colored by height, each followed by its wireframe outline.
fn push_surface(
    builder: &mut Builder<'_>,
    member_index: usize,
    member: &SceneMember<'_>,
    input: &SurfaceInput,
    world: &impl Fn([f64; 3]) -> [f64; 3],
) {
    let columns = input.x.len();
    let extent = height_extent(input);
    let view = builder.view;
    let (xs, ys) = (lod_indices(columns), lod_indices(input.y.len()));
    for rows in ys.windows(2) {
        for cols in xs.windows(2) {
            let corners = [
                rows[0] * columns + cols[0],
                rows[0] * columns + cols[1],
                rows[1] * columns + cols[1],
                rows[1] * columns + cols[0],
            ];
            let Some(heights) = corners
                .iter()
                .map(|&sample| input.z.get(sample).copied().flatten())
                .collect::<Option<Vec<f64>>>()
            else {
                continue;
            };
            let positions: Vec<[f64; 3]> = corners
                .iter()
                .zip(&heights)
                .map(|(&sample, &z)| {
                    world([input.x[sample % columns], input.y[sample / columns], z])
                })
                .collect();
            let owner = builder.item(member_index, &corners);
            let v: Vec<[f64; 3]> = positions.iter().map(|&p| view.view_point(p)).collect();
            builder.begin();
            let mut outline = None;
            for [i, j, k] in [[0, 1, 2], [0, 2, 3]] {
                let base = surface_color(
                    input,
                    member.color,
                    extent,
                    (heights[i] + heights[j] + heights[k]) / 3.0,
                );
                let centroid = lerp3(
                    lerp3(positions[i], positions[j], 0.5),
                    positions[k],
                    1.0 / 3.0,
                );
                let color = match normalized(cross(
                    sub(positions[j], positions[i]),
                    sub(positions[k], positions[i]),
                )) {
                    Some(normal) if view.faces_eye(centroid, normal) => shade(base, normal),
                    Some(normal) => shade(base, normal.map(|axis| -axis)),
                    None => base,
                };
                outline.get_or_insert(interpolate_color(color, Color::rgb(0, 0, 0), 0.5));
                builder.view_triangle([v[i], v[j], v[k]], color, owner);
            }
            if input.wireframe
                && let Some(edge) = outline
            {
                for index in 0..4 {
                    if let Some((a, b)) = clip_near(view, v[index], v[(index + 1) % 4]) {
                        builder.screen_segment(
                            view.screen(a),
                            view.screen(b),
                            WIREFRAME_WIDTH,
                            opaque(edge),
                            owner,
                        );
                    }
                }
            }
            let depth = v.iter().map(|point| point[2]).sum::<f64>() / 4.0;
            builder.end(depth);
        }
    }
}

impl LayoutSeriesEntry {
    fn scene_data<'a>(&self, store: &'a GeneralLayoutStore) -> Option<SceneData<'a>> {
        SceneData::of(&store.entry(self.options.dataset)?.input)
    }
}

impl GeneralLayoutStore {
    /// The first 3D series of a pane, visible or not: it holds the pane's camera, axis names and
    /// categories, and scene cache.
    fn scene_owner(&self, pane_id: PaneId) -> Option<&LayoutSeriesEntry> {
        self.series
            .iter()
            .find(|series| series.pane_id == pane_id && series.options.kind.is_3d())
    }

    /// The first visible 3D series of a pane, where the frame paints the whole scene.
    pub(in crate::general_layout) fn scene_drawer(
        &self,
        pane_id: PaneId,
    ) -> Option<GeneralLayoutSeriesId> {
        self.series
            .iter()
            .find(|series| {
                series.pane_id == pane_id && series.options.visible && series.options.kind.is_3d()
            })
            .map(|series| series.id)
    }

    fn scene_members(&self, pane_id: PaneId) -> Vec<SceneMember<'_>> {
        self.series
            .iter()
            .filter(|series| {
                series.pane_id == pane_id && series.options.visible && series.options.kind.is_3d()
            })
            .filter_map(|series| {
                Some(SceneMember {
                    id: series.id,
                    kind: series.options.kind,
                    dataset: series.options.dataset,
                    color: series.color,
                    data: series.scene_data(self)?,
                })
            })
            .collect()
    }

    /// Reads the current scene of a pane, rebuilding it when its key changed.
    pub(in crate::general_layout) fn with_scene<R>(
        &self,
        pane_id: PaneId,
        plot: GeneralPlotRect,
        f: impl FnOnce(&SceneCache, &[SceneMember<'_>]) -> R,
    ) -> Option<R> {
        let owner = self.scene_owner(pane_id)?;
        let members = self.scene_members(pane_id);
        let key = SceneKey {
            plot,
            camera: owner.options.camera_3d,
            members: members
                .iter()
                .map(|member| (member.id, member.kind, member.dataset, member.color))
                .collect(),
        };
        let mut cache = owner.scene.borrow_mut();
        if cache.as_ref().is_none_or(|cached| cached.key != key) {
            let fallback = Axis3d::default();
            let axes = owner
                .scene_data(self)
                .map_or([&fallback; 3], SceneData::axes);
            *cache = Some(SceneCache::new(key, axes, &members));
        }
        Some(f(
            cache.as_ref().expect("scene cache initialized"),
            &members,
        ))
    }

    /// Drops the scene caches of every pane that shows a series of `dataset`.
    pub(in crate::general_layout) fn clear_scenes_showing(&self, dataset: GeneralLayoutDatasetId) {
        for series in &self.series {
            if series.options.kind.is_3d()
                && self.series.iter().any(|other| {
                    other.pane_id == series.pane_id && other.options.dataset == dataset
                })
            {
                *series.scene.borrow_mut() = None;
            }
        }
    }

    pub(in crate::general_layout) fn scene_hit(
        &self,
        pane_id: PaneId,
        plot: GeneralPlotRect,
        x: f64,
        y: f64,
    ) -> Option<GeneralLayoutHit> {
        self.with_scene(pane_id, plot, |cache, members| cache.hit(members, x, y))
            .flatten()
    }

    pub(in crate::general_layout) fn scene_anchor(
        &self,
        pane_id: PaneId,
        plot: GeneralPlotRect,
        hit: GeneralLayoutHit,
    ) -> Option<(f64, f64)> {
        self.with_scene(pane_id, plot, |cache, members| {
            let member = members.iter().find(|member| member.id == hit.series)?;
            if member.element() != hit.element {
                return None;
            }
            cache.anchor(member.data, hit.row).map(|[x, y]| (x, y))
        })
        .flatten()
    }
}

/// Painted color of a 3D item, before shading.
pub(in crate::general_layout) fn item_color(
    series: Color,
    input: &GeneralLayoutInput,
    row: usize,
) -> Option<Color> {
    match input {
        GeneralLayoutInput::Points3d(input) => {
            Some(parse_or(input.points.get(row)?.color.as_deref(), series))
        }
        GeneralLayoutInput::Surface(input) => Some(surface_color(
            input,
            series,
            height_extent(input),
            (*input.z.get(row)?)?,
        )),
        _ => None,
    }
}

impl ChartEngine {
    /// Grid lines, the depth-sorted mesh, then axis lines, ticks, labels and axis names.
    pub(in crate::general_layout) fn build_scene_frame(
        &self,
        store: &GeneralLayoutStore,
        pane_id: PaneId,
        plot: GeneralPlotRect,
        (hpr, vpr): (f64, f64),
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let options = self.options.get();
        let fallback = Color::rgb(0xd1, 0xd4, 0xdc);
        let grid_color = Color::parse_css(&options.grid.vert_lines.color).unwrap_or(fallback);
        let axis_color =
            Color::parse_css(&options.right_price_scale.border_color).unwrap_or(fallback);
        let text_color = self.primary_text_color();
        let family = options.layout.font_family.clone();
        let scale = hpr.min(vpr);
        let rect = [plot.x, plot.y, plot.x + plot.width, plot.y + plot.height];
        let bitmap = |[x, y]: [f64; 2]| [(x * hpr) as f32, (y * vpr) as f32];
        store.with_scene(pane_id, plot, |cache, _| {
            let lines = |segments: &[[[f64; 2]; 2]],
                         width: f64,
                         color: Color,
                         out: &mut Vec<Prim>,
                         points: &mut Vec<[f32; 2]>| {
                for segment in segments {
                    push_clipped_polyline(
                        segment.iter().copied(),
                        rect,
                        &bitmap,
                        points,
                        |first_point, point_count| {
                            out.push(Prim::Polyline {
                                first_point,
                                point_count,
                                width: (width * scale) as f32,
                                style: LineStyle::Solid,
                                line_type: LineType::Simple,
                                color,
                            });
                        },
                    );
                }
            };
            lines(&cache.grid, GRID_WIDTH, grid_color, out, points);
            if !cache.colors.is_empty() {
                let first_point = points.len() as u32;
                points.extend(
                    cache
                        .points
                        .iter()
                        .map(|&[x, y]| [(f64::from(x) * hpr) as f32, (f64::from(y) * vpr) as f32]),
                );
                out.push(Prim::ColorMesh {
                    first_point,
                    point_count: points.len() as u32 - first_point,
                    colors: cache.colors.clone(),
                });
            }
            lines(&cache.lines, AXIS_WIDTH, axis_color, out, points);
            for label in &cache.labels {
                if label.text.is_empty() || !cache.contains(label.at) {
                    continue;
                }
                out.push(Prim::Text {
                    x: (label.at[0] * hpr) as f32,
                    y: (label.at[1] * vpr) as f32,
                    text: label.text.clone(),
                    color: text_color,
                    size: (TEXT_SIZE * vpr) as f32,
                    family: family.clone(),
                    align: label.align,
                    weight: 400,
                    italic: false,
                });
            }
        });
    }

    /// The camera owner of the 3D pane `pane` when it shows a visible 3D series and, when a
    /// point is given, its plot holds the point.
    fn orbit_pane_owner(
        &self,
        pane: usize,
        point: Option<(f64, f64)>,
    ) -> Option<GeneralLayoutSeriesId> {
        let pane_id = self.pane_stable_id(pane)?;
        let plot = self.general_plot_rect(pane)?;
        let store = self.general_layout_data.as_ref()?;
        store.scene_drawer(pane_id)?;
        if let Some((x, y)) = point
            && !(x >= plot.x && x < plot.x + plot.width && y >= plot.y && y < plot.y + plot.height)
        {
            return None;
        }
        store.scene_owner(pane_id).map(|owner| owner.id)
    }

    /// The camera owner of the 3D pane whose plot holds `(x, y)`.
    pub(crate) fn orbit_press_target(&self, x: f64, y: f64) -> Option<GeneralLayoutSeriesId> {
        self.orbit_pane_owner(self.pane_at_y(y)?, Some((x, y)))
    }

    /// The 3D pane chart keys turn: the one under the pointer, or without a pointer the first
    /// pane showing a 3D series. A pointer over any other pane leaves keys to their usual work.
    pub(crate) fn orbit_key_target(
        &self,
        pointer: Option<(f64, f64)>,
    ) -> Option<GeneralLayoutSeriesId> {
        match pointer {
            Some((x, y)) => self.orbit_press_target(x, y),
            None => (0..self.panes.len()).find_map(|pane| self.orbit_pane_owner(pane, None)),
        }
    }

    pub(crate) fn camera_3d(&self, owner: GeneralLayoutSeriesId) -> Option<Camera3d> {
        self.general_layout_series_options(owner)
            .filter(|options| options.kind.is_3d())
            .map(|options| options.camera_3d)
    }

    /// Set the camera of the 3D pane `owner` belongs to, on every 3D series of that pane.
    /// Returns whether it changed.
    pub(crate) fn set_camera_3d(&mut self, owner: GeneralLayoutSeriesId, camera: Camera3d) -> bool {
        if validate_camera(camera).is_err() {
            return false;
        }
        let Some(store) = self.general_layout_data.as_mut() else {
            return false;
        };
        let Some(pane_id) = store
            .series
            .iter()
            .find(|series| series.id == owner && series.options.kind.is_3d())
            .map(|series| series.pane_id)
        else {
            return false;
        };
        let mut changed = false;
        for series in store
            .series
            .iter_mut()
            .filter(|series| series.pane_id == pane_id && series.options.kind.is_3d())
        {
            changed |= series.options.camera_3d != camera;
            series.options.camera_3d = camera;
        }
        if changed {
            self.invalidate_frame_scene();
        }
        changed
    }

    fn orbit_plot(&self, owner: GeneralLayoutSeriesId) -> Option<GeneralPlotRect> {
        let store = self.general_layout_data.as_ref()?;
        let series = store.series.iter().find(|series| series.id == owner)?;
        self.general_plot_rect(self.pane_index_for_id(series.pane_id)?)
    }

    /// Orbit from the camera `before` a drag began by a screen delta, as ECharts GL's
    /// `OrbitControl` does: a drag across the plot height turns the box two radians, and a drag
    /// across the plot width tilts it two radians. Dragging right turns the box right; dragging
    /// down tilts its top toward the viewer.
    pub(crate) fn orbit_rotate(
        &mut self,
        owner: GeneralLayoutSeriesId,
        before: Camera3d,
        dx: f64,
        dy: f64,
    ) -> bool {
        let Some(plot) = self.orbit_plot(owner) else {
            return false;
        };
        if !(plot.width > 0.0 && plot.height > 0.0) {
            return false;
        }
        let camera = before.rotated(
            (dy / plot.width * 2.0).to_degrees(),
            -(dx / plot.height * 2.0).to_degrees(),
        );
        self.set_camera_3d(owner, camera)
    }

    /// Turn the camera of `owner`'s pane by degrees.
    pub(crate) fn orbit_turn(
        &mut self,
        owner: GeneralLayoutSeriesId,
        alpha: f64,
        beta: f64,
    ) -> bool {
        let Some(camera) = self.camera_3d(owner) else {
            return false;
        };
        self.set_camera_3d(owner, camera.rotated(alpha, beta))
    }

    /// Move the camera of `owner`'s pane `factor` times closer.
    pub(crate) fn orbit_zoom(&mut self, owner: GeneralLayoutSeriesId, factor: f64) -> bool {
        if !factor.is_finite() || factor <= 0.0 {
            return false;
        }
        let Some(camera) = self.camera_3d(owner) else {
            return false;
        };
        self.set_camera_3d(owner, camera.zoomed(factor))
    }

    /// One orbit key on the camera of `owner`'s pane: arrows turn it as a drag in their
    /// direction would, by 5 degrees or 15 when `coarse`; zoom keys move it one wheel notch;
    /// Home restores the default angles and distance, keeping the projection.
    pub(crate) fn orbit_key(
        &mut self,
        owner: GeneralLayoutSeriesId,
        key: ChartKey,
        coarse: bool,
    ) -> bool {
        let step = if coarse { 15.0 } else { 5.0 };
        match key {
            ChartKey::ArrowLeft => self.orbit_turn(owner, 0.0, step),
            ChartKey::ArrowRight => self.orbit_turn(owner, 0.0, -step),
            ChartKey::ArrowUp => self.orbit_turn(owner, -step, 0.0),
            ChartKey::ArrowDown => self.orbit_turn(owner, step, 0.0),
            ChartKey::ZoomIn => self.orbit_zoom(owner, KEY_ZOOM),
            ChartKey::ZoomOut => self.orbit_zoom(owner, 1.0 / KEY_ZOOM),
            ChartKey::Home => {
                let Some(camera) = self.camera_3d(owner) else {
                    return false;
                };
                self.set_camera_3d(
                    owner,
                    Camera3d {
                        projection: camera.projection,
                        ..Camera3d::default()
                    },
                )
            }
            _ => false,
        }
    }

    /// Zoom the 3D pane under `(x, y)`. Returns whether a 3D pane was under the point; a zoom
    /// already at its limit still consumes the gesture.
    pub(crate) fn orbit_zoom_at(&mut self, x: f64, y: f64, factor: f64) -> bool {
        let Some(owner) = self.orbit_press_target(x, y) else {
            return false;
        };
        self.orbit_zoom(owner, factor);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lod_strides_long_sides_and_keeps_the_last_sample() {
        assert_eq!(lod_indices(1), Vec::<usize>::new());
        assert_eq!(lod_indices(3), [0, 1, 2]);
        assert_eq!(lod_indices(65).len(), 65);
        let long = lod_indices(128);
        assert_eq!(long.len() - 1, 64);
        assert_eq!(*long.last().unwrap(), 127);
        assert!(long.windows(2).all(|pair| pair[1] - pair[0] <= 2));
    }

    #[test]
    fn value_axes_keep_zero_and_round_to_nice_ticks() {
        let axis = AxisScale::new(&Axis3d::default(), "z", [3.2, 9.7].into_iter(), false);
        assert_eq!((axis.from, axis.to), (0.0, 10.0));
        assert_eq!(
            axis.ticks.iter().map(|tick| tick.0).collect::<Vec<_>>(),
            [0.0, 2.0, 4.0, 6.0, 8.0, 10.0]
        );
        let categories = Axis3d {
            name: String::new(),
            categories: ["a", "b", "c"].map(String::from).to_vec(),
        };
        let band = AxisScale::new(&categories, "x", [0.0, 2.0].into_iter(), true);
        assert_eq!((band.from, band.to, band.category), (-0.5, 2.5, true));
        assert_eq!(band.ticks[1], (1.0, "b".to_owned()));
        assert_eq!(band.name, "x");
    }

    #[test]
    fn shading_is_lambert_under_the_fixed_light() {
        let white = Color::rgb(255, 255, 255);
        let lit = shade(white, light());
        assert_eq!((lit.r(), lit.g(), lit.b()), (255, 255, 255));
        let away = shade(white, light().map(|axis| -axis));
        assert_eq!(away.r(), (255.0 * AMBIENT).round() as u8);
        assert_eq!(shade(Color::rgba(10, 20, 30, 40), light()).a(), 255);
    }
}
