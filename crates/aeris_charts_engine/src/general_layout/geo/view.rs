//! Map pan and zoom (complete build): the persisted view of a map pane, the post-projection
//! transform every map series of the pane shares, screen clipping of transformed geometry, and
//! the engine operations the input controller drives.
//!
//! Every map series of a pane stores the same view and frames read it from the first one, so
//! the base map and its bubble and flow maps always move together and removing a series never
//! strands the view. The view is resize-stable: `x` and `y` place the pre-view point shown at
//! the map center as a fraction of the map rectangle away from its center, and `zoom` scales
//! about that point. Pans stay inside the fitted map, so the visible window never leaves the
//! geometry the caches projected and clipped.
//!
//! Caches hold geometry in pre-view coordinates. A view change maps cached triangles, rings
//! and marks through one affine transform and clips them to the map rectangle; it never
//! reprojects or retriangulates.

use super::super::{GeneralLayoutSeriesKind, GeneralLayoutStore, LayoutSeriesEntry};
use super::GeoInput;
use super::choropleth::GeoCache;
use super::projection::clip_segment;
use crate::general_axes::GeneralPlotRect;
use crate::{ChartEngine, ChartError, ErrorCode, GeneralLayoutSeriesId, PaneId};

pub(crate) const MIN_MAP_ZOOM: f64 = 1.0;
/// Deepest zoom: a 1:50m world fitted to a 400 px plot shows about 12 km per px at zoom 32,
/// the 1:50m data resolution, so deeper zoom would only magnify vertices.
pub(crate) const MAX_MAP_ZOOM: f64 = 32.0;

/// Pan offset and zoom of a map pane (complete build).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MapView {
    /// Offset of the pre-view point at the map center from the map center, as a fraction of
    /// the map width; at most `0.5 - 0.5 / zoom` either way.
    pub x: f64,
    /// The same offset as a fraction of the map height.
    pub y: f64,
    /// Scale about the map center, `1..=32`.
    pub zoom: f64,
}

impl Default for MapView {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        }
    }
}

impl MapView {
    pub(crate) fn is_default(&self) -> bool {
        *self == Self::default()
    }

    fn pan_limit(zoom: f64) -> f64 {
        0.5 - 0.5 / zoom
    }

    /// This view with its zoom in range and its pan inside the fitted map.
    fn clamped(self) -> Self {
        let zoom = self.zoom.clamp(MIN_MAP_ZOOM, MAX_MAP_ZOOM);
        let limit = Self::pan_limit(zoom);
        Self {
            x: self.x.clamp(-limit, limit),
            y: self.y.clamp(-limit, limit),
            zoom,
        }
    }
}

pub(in crate::general_layout) fn validate_view(view: MapView) -> Result<(), ChartError> {
    let limit = MapView::pan_limit(view.zoom);
    if (MIN_MAP_ZOOM..=MAX_MAP_ZOOM).contains(&view.zoom)
        && view.x.abs() <= limit
        && view.y.abs() <= limit
    {
        Ok(())
    } else {
        Err(ChartError::new(
            ErrorCode::InvalidOptions,
            "map view needs a zoom between 1 and 32 and pan offsets within 0.5 - 0.5 / zoom",
        ))
    }
}

/// The screen mapping of one map pane: `screen = mid + zoom * (p - center)` for a pre-view
/// point `p`, where `mid` is the map rectangle center and `center` the pre-view point shown
/// there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::general_layout) struct MapTransform {
    rect: [f64; 4],
    mid: [f64; 2],
    center: [f64; 2],
    zoom: f64,
}

impl MapTransform {
    pub(in crate::general_layout) fn new(rect: [f64; 4], view: MapView) -> Self {
        let mid = [(rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0];
        Self {
            rect,
            mid,
            center: [
                mid[0] + view.x * (rect[2] - rect[0]),
                mid[1] + view.y * (rect[3] - rect[1]),
            ],
            zoom: view.zoom,
        }
    }

    #[cfg(test)]
    pub(in crate::general_layout) fn identity(rect: [f64; 4]) -> Self {
        Self::new(rect, MapView::default())
    }

    fn is_identity(&self) -> bool {
        self.zoom == 1.0 && self.center == self.mid
    }

    pub(in crate::general_layout) fn rect(&self) -> [f64; 4] {
        self.rect
    }

    pub(in crate::general_layout) fn zoom(&self) -> f64 {
        self.zoom
    }

    pub(in crate::general_layout) fn forward(&self, p: [f64; 2]) -> [f64; 2] {
        if self.is_identity() {
            return p;
        }
        [
            self.mid[0] + self.zoom * (p[0] - self.center[0]),
            self.mid[1] + self.zoom * (p[1] - self.center[1]),
        ]
    }

    pub(in crate::general_layout) fn inverse(&self, q: [f64; 2]) -> [f64; 2] {
        if self.is_identity() {
            return q;
        }
        [
            self.center[0] + (q[0] - self.mid[0]) / self.zoom,
            self.center[1] + (q[1] - self.mid[1]) / self.zoom,
        ]
    }

    pub(in crate::general_layout) fn contains(&self, [x, y]: [f64; 2]) -> bool {
        x >= self.rect[0] && x <= self.rect[2] && y >= self.rect[1] && y <= self.rect[3]
    }
}

fn plot_rect(plot: GeneralPlotRect) -> [f64; 4] {
    [plot.x, plot.y, plot.x + plot.width, plot.y + plot.height]
}

/// Appends screen triangle `corners` clipped to `rect` as a fan of bitmap triangles
/// (Sutherland-Hodgman; a triangle clips to at most seven vertices).
pub(in crate::general_layout) fn push_clipped_triangle(
    corners: [[f64; 2]; 3],
    rect: [f64; 4],
    bitmap: &impl Fn([f64; 2]) -> [f32; 2],
    points: &mut Vec<[f32; 2]>,
) {
    let inside =
        |p: &[f64; 2]| p[0] >= rect[0] && p[0] <= rect[2] && p[1] >= rect[1] && p[1] <= rect[3];
    if corners.iter().all(inside) {
        points.extend(corners.map(bitmap));
        return;
    }
    for axis in 0..2 {
        if corners.iter().all(|p| p[axis] < rect[axis])
            || corners.iter().all(|p| p[axis] > rect[axis + 2])
        {
            return;
        }
    }
    let mut polygon = [[0.0; 2]; 8];
    polygon[..3].copy_from_slice(&corners);
    let mut count = 3;
    for (axis, bound, lower) in [
        (0, rect[0], true),
        (0, rect[2], false),
        (1, rect[1], true),
        (1, rect[3], false),
    ] {
        let keep = |p: [f64; 2]| {
            if lower {
                p[axis] >= bound
            } else {
                p[axis] <= bound
            }
        };
        let mut next = [[0.0; 2]; 8];
        let mut kept = 0;
        for index in 0..count {
            let a = polygon[index];
            let b = polygon[(index + 1) % count];
            if keep(a) {
                next[kept] = a;
                kept += 1;
            }
            if keep(a) != keep(b) {
                let t = (bound - a[axis]) / (b[axis] - a[axis]);
                let mut crossing = [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
                crossing[axis] = bound;
                next[kept] = crossing;
                kept += 1;
            }
        }
        polygon = next;
        count = kept;
        if count < 3 {
            return;
        }
    }
    for index in 1..count - 1 {
        points.extend([polygon[0], polygon[index], polygon[index + 1]].map(bitmap));
    }
}

/// Appends the parts of a screen polyline inside `rect` as bitmap runs, calling `emit` with
/// the first point and point count of each run of at least two points.
pub(in crate::general_layout) fn push_clipped_polyline(
    line: impl Iterator<Item = [f64; 2]>,
    rect: [f64; 4],
    bitmap: &impl Fn([f64; 2]) -> [f32; 2],
    points: &mut Vec<[f32; 2]>,
    mut emit: impl FnMut(u32, u32),
) {
    let mut start = points.len() as u32;
    let mut open = false;
    let mut previous: Option<[f64; 2]> = None;
    let mut close = |points: &mut Vec<[f32; 2]>, start: &mut u32| {
        let count = points.len() as u32 - *start;
        if count >= 2 {
            emit(*start, count);
        } else {
            points.truncate(*start as usize);
        }
        *start = points.len() as u32;
    };
    for point in line {
        if let Some(from) = previous {
            let (mut a, mut b) = ((from[0], from[1]), (point[0], point[1]));
            if clip_segment(&mut a, &mut b, rect) {
                if !open {
                    close(points, &mut start);
                    points.push(bitmap([a.0, a.1]));
                }
                points.push(bitmap([b.0, b.1]));
                open = b == (point[0], point[1]);
            } else {
                open = false;
            }
        }
        previous = Some(point);
    }
    close(points, &mut start);
}

impl GeneralLayoutSeriesKind {
    pub(in crate::general_layout) fn is_map(self) -> bool {
        matches!(self, Self::Choropleth | Self::BubbleMap | Self::FlowMap)
    }
}

impl GeneralLayoutStore {
    /// The base map of a pane: its first choropleth series over a map dataset, visible or not.
    pub(in crate::general_layout) fn map_base(
        &self,
        pane_id: PaneId,
    ) -> Option<(&LayoutSeriesEntry, &GeoInput)> {
        self.series
            .iter()
            .filter(|series| {
                series.pane_id == pane_id
                    && series.options.kind == GeneralLayoutSeriesKind::Choropleth
            })
            .find_map(|series| match &self.entry(series.options.dataset)?.input {
                super::super::GeneralLayoutInput::Geo(input) => Some((series, input)),
                _ => None,
            })
    }

    /// The first map series of a pane, which holds the pane's view.
    fn map_view_owner(&self, pane_id: PaneId) -> Option<&LayoutSeriesEntry> {
        self.series
            .iter()
            .find(|series| series.pane_id == pane_id && series.options.kind.is_map())
    }

    /// The transform every map series of the pane draws and hits through.
    pub(in crate::general_layout) fn map_transform(
        &self,
        pane_id: PaneId,
        plot: GeneralPlotRect,
    ) -> MapTransform {
        let rect = self.map_base(pane_id).map_or_else(
            || plot_rect(plot),
            |(base, input)| base.with_geo(input, plot, GeoCache::clip),
        );
        let view = self
            .map_view_owner(pane_id)
            .map_or_else(MapView::default, |owner| owner.options.map_view);
        MapTransform::new(rect, view)
    }
}

impl ChartEngine {
    /// The view owner of the map pane whose map rectangle holds `(x, y)`, when the pane shows
    /// at least one map series.
    pub(crate) fn map_press_target(&self, x: f64, y: f64) -> Option<GeneralLayoutSeriesId> {
        let pane = self.pane_at_y(y)?;
        let pane_id = self.pane_stable_id(pane)?;
        let plot = self.general_plot_rect(pane)?;
        let store = self.general_layout_data.as_ref()?;
        if !store.series.iter().any(|series| {
            series.pane_id == pane_id && series.options.visible && series.options.kind.is_map()
        }) {
            return None;
        }
        if !store.map_transform(pane_id, plot).contains([x, y]) {
            return None;
        }
        store.map_view_owner(pane_id).map(|owner| owner.id)
    }

    pub(crate) fn map_view(&self, owner: GeneralLayoutSeriesId) -> Option<MapView> {
        self.general_layout_series_options(owner)
            .filter(|options| options.kind.is_map())
            .map(|options| options.map_view)
    }

    /// The pane and current transform of the map pane `owner` belongs to.
    fn map_owner_transform(&self, owner: GeneralLayoutSeriesId) -> Option<(PaneId, MapTransform)> {
        let store = self.general_layout_data.as_ref()?;
        let series = store
            .series
            .iter()
            .find(|series| series.id == owner && series.options.kind.is_map())?;
        let plot = self
            .pane_index_for_id(series.pane_id)
            .and_then(|pane| self.general_plot_rect(pane))?;
        Some((series.pane_id, store.map_transform(series.pane_id, plot)))
    }

    /// Set the view of the map pane `owner` belongs to, on every map series of that pane.
    /// Returns whether it changed.
    pub(crate) fn set_map_view(&mut self, owner: GeneralLayoutSeriesId, view: MapView) -> bool {
        if validate_view(view).is_err() {
            return false;
        }
        let Some((pane_id, _)) = self.map_owner_transform(owner) else {
            return false;
        };
        let Some(store) = self.general_layout_data.as_mut() else {
            return false;
        };
        let mut changed = false;
        for series in store
            .series
            .iter_mut()
            .filter(|series| series.pane_id == pane_id && series.options.kind.is_map())
        {
            changed |= series.options.map_view != view;
            series.options.map_view = view;
        }
        if changed {
            self.invalidate_frame_scene();
        }
        changed
    }

    /// Pan a map view by a screen delta from the view `before` the gesture began.
    pub(crate) fn map_pan(
        &mut self,
        owner: GeneralLayoutSeriesId,
        before: MapView,
        dx: f64,
        dy: f64,
    ) -> bool {
        let Some((_, transform)) = self.map_owner_transform(owner) else {
            return false;
        };
        let [x0, y0, x1, y1] = transform.rect();
        let (width, height) = (x1 - x0, y1 - y0);
        if !(width > 0.0 && height > 0.0) {
            return false;
        }
        let view = MapView {
            x: before.x - dx / (before.zoom * width),
            y: before.y - dy / (before.zoom * height),
            zoom: before.zoom,
        };
        self.set_map_view(owner, view.clamped())
    }

    /// Zoom the map under `(x, y)` by `factor` around that point. Returns whether a map was
    /// under the point; a zoom already at its limit still consumes the gesture.
    pub(crate) fn map_zoom_at(&mut self, x: f64, y: f64, factor: f64) -> bool {
        let Some(owner) = self.map_press_target(x, y) else {
            return false;
        };
        self.map_zoom_about(owner, [x, y], factor);
        true
    }

    fn map_zoom_about(&mut self, owner: GeneralLayoutSeriesId, q: [f64; 2], factor: f64) -> bool {
        if !factor.is_finite() || factor <= 0.0 {
            return false;
        }
        let (Some((_, transform)), Some(view)) =
            (self.map_owner_transform(owner), self.map_view(owner))
        else {
            return false;
        };
        let [x0, y0, x1, y1] = transform.rect();
        let (width, height) = (x1 - x0, y1 - y0);
        if !(width > 0.0 && height > 0.0) {
            return false;
        }
        let anchor = transform.inverse(q);
        let zoom = (view.zoom * factor).clamp(MIN_MAP_ZOOM, MAX_MAP_ZOOM);
        let mid = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
        let center = [
            anchor[0] - (q[0] - mid[0]) / zoom,
            anchor[1] - (q[1] - mid[1]) / zoom,
        ];
        let next = MapView {
            x: (center[0] - mid[0]) / width,
            y: (center[1] - mid[1]) / height,
            zoom,
        };
        self.set_map_view(owner, next.clamped())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECT: [f64; 4] = [10.0, 20.0, 410.0, 320.0];

    fn bitmap(p: [f64; 2]) -> [f32; 2] {
        [p[0] as f32, p[1] as f32]
    }

    fn area(points: &[[f32; 2]]) -> f64 {
        points
            .as_chunks::<3>()
            .0
            .iter()
            .map(|[a, b, c]| {
                let (a, b, c) = (a.map(f64::from), b.map(f64::from), c.map(f64::from));
                ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0
            })
            .sum()
    }

    #[test]
    fn transforms_invert_and_identity_is_exact() {
        let identity = MapTransform::identity(RECT);
        let p = [123.456_789, 98.765_432_1];
        assert_eq!(identity.forward(p), p);
        assert_eq!(identity.inverse(p), p);
        let view = MapView {
            x: 0.2,
            y: -0.1,
            zoom: 4.0,
        };
        assert!(validate_view(view).is_ok());
        let zoomed = MapTransform::new(RECT, view);
        let back = zoomed.inverse(zoomed.forward(p));
        assert!((back[0] - p[0]).abs() < 1e-12 && (back[1] - p[1]).abs() < 1e-12);
        let center = [210.0 + 0.2 * 400.0, 170.0 - 0.1 * 300.0];
        assert_eq!(zoomed.forward(center), [210.0, 170.0]);
    }

    #[test]
    fn views_validate_and_clamp_inside_the_fitted_map() {
        for invalid in [
            MapView {
                zoom: 0.5,
                ..MapView::default()
            },
            MapView {
                zoom: 33.0,
                ..MapView::default()
            },
            MapView {
                x: 0.1,
                ..MapView::default()
            },
            MapView {
                x: f64::NAN,
                y: 0.0,
                zoom: 2.0,
            },
        ] {
            assert!(validate_view(invalid).is_err(), "{invalid:?}");
        }
        let clamped = MapView {
            x: 9.0,
            y: -9.0,
            zoom: 100.0,
        }
        .clamped();
        assert_eq!(clamped.zoom, MAX_MAP_ZOOM);
        assert_eq!(clamped.x, 0.5 - 0.5 / 32.0);
        assert!(validate_view(clamped).is_ok());
    }

    #[test]
    fn clipped_triangles_cover_exactly_their_part_inside_the_rect() {
        let mut points = Vec::new();
        push_clipped_triangle(
            [[0.0, 0.0], [400.0, 0.0], [0.0, 400.0]],
            [0.0, 0.0, 100.0, 100.0],
            &bitmap,
            &mut points,
        );
        assert!((area(&points) - 10_000.0).abs() < 1e-6);
        points.clear();
        push_clipped_triangle(
            [[200.0, 0.0], [300.0, 0.0], [250.0, 50.0]],
            [0.0, 0.0, 100.0, 100.0],
            &bitmap,
            &mut points,
        );
        assert!(points.is_empty());
        push_clipped_triangle(
            [[10.0, 10.0], [20.0, 10.0], [10.0, 20.0]],
            [0.0, 0.0, 100.0, 100.0],
            &bitmap,
            &mut points,
        );
        assert_eq!(points, [[10.0, 10.0], [20.0, 10.0], [10.0, 20.0]]);
    }

    #[test]
    fn clipped_polylines_split_into_inside_runs() {
        let mut points = Vec::new();
        let mut runs = Vec::new();
        push_clipped_polyline(
            [
                [-10.0, 50.0],
                [50.0, 50.0],
                [150.0, 50.0],
                [150.0, 60.0],
                [50.0, 60.0],
            ]
            .into_iter(),
            [0.0, 0.0, 100.0, 100.0],
            &bitmap,
            &mut points,
            |first, count| runs.push((first, count)),
        );
        assert_eq!(runs, [(0, 3), (3, 2)]);
        assert_eq!(
            points,
            [
                [0.0, 50.0],
                [50.0, 50.0],
                [100.0, 50.0],
                [100.0, 60.0],
                [50.0, 60.0]
            ]
        );
    }
}
