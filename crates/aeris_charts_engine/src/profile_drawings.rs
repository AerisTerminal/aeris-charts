//! Engine-owned data binding, bounded calculation cache, and shared geometry for the
//! volume-profile drawings (fixed range and anchored) and anchored VWAP.
//!
//! A drawing never needs host wiring to show data. Its source resolves at frame time: an
//! explicit `configure_profile_drawing` binding wins, then the canonical tape of an order-flow
//! presentation on the drawing's pane, then the volume series the host registered for the
//! pane's primary price series. Frame construction, hit testing, and axis labels read one
//! cached snapshot through [`ChartEngine::profile_drawing_data`] and one geometry resolver, so
//! every consumer sees identical rows and levels.
//!
//! A fixed range profile is a box: its two anchors are opposite corners and its time span
//! selects the bars or trades. While `profile_fit_prices` holds, frame refresh snaps the box's
//! top and bottom to the traded high and low; once the user resizes either edge, the box's
//! price span limits the profiled prices instead. A zero-height box profiles every price.

use std::borrow::Cow;

use aeris_charts_render::color::Color;

use crate::profiles::{ProfileDrawingSnapshot, ProfileRequest, ProfileSnapshot, ProfileSource};
use crate::{
    ChartEngine, Drawing, DrawingId, DrawingKind, DrawingPoint, DrawingPriceScale,
    MAX_PROFILE_ROWS, SeriesId, SeriesKind,
};

pub(crate) const DEFAULT_PROFILE_DRAWING_ROWS: usize = 100;
pub(crate) const DEFAULT_PROFILE_DRAWING_VALUE_AREA: f64 = 70.0;
pub(crate) const DEFAULT_PROFILE_DRAWING_WIDTH: f64 = 30.0;
/// Fallback row grid when neither the instrument nor the bound scale defines a tick.
const FALLBACK_PROFILE_TICK: f64 = 0.01;

pub(crate) const PROFILE_VALUE_AREA_COLOR: Color = Color::rgba(255, 152, 0, 235);
pub(crate) const PROFILE_VOLUME_COLOR: Color = Color::rgba(255, 213, 79, 220);
pub(crate) const PROFILE_POSITIVE_COLOR: Color = Color::rgba(76, 175, 80, 220);
pub(crate) const PROFILE_NEGATIVE_COLOR: Color = Color::rgba(171, 71, 188, 220);
pub(crate) const PROFILE_POC_COLOR: Color = Color::rgb(255, 82, 82);

/// Everything a profile drawing's calculation depends on. Equal keys produce equal snapshots,
/// so a cache entry is reused exactly while its key is unchanged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ProfileDrawingKey {
    kind: DrawingKind,
    source: ProfileSource,
    revisions: [u64; 2],
    start_micros: i64,
    end_micros: i64,
    tick: f64,
    rows: usize,
    value_area_percent: f64,
    band_multiplier: f64,
    price_window: Option<(f64, f64)>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProfileDrawingCacheEntry {
    id: DrawingId,
    key: ProfileDrawingKey,
    snapshot: Option<ProfileDrawingSnapshot>,
}

/// Resolved paint colors of one volume-profile drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ProfileDrawingColors {
    pub value_area: Color,
    pub volume: Color,
    pub positive: Color,
    pub negative: Color,
    pub poc: Color,
    pub line: Color,
}

/// Media-space layout of one volume profile drawing. `spine` is the range start; total volume
/// grows right of it and delta grows left of it, each up to `side` px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ProfileDrawingGeometry {
    pub spine: f64,
    pub right: f64,
    pub left: f64,
    pub side: f64,
    pub top: f64,
    pub bottom: f64,
    pub poc: Option<(f64, f64)>,
    pub value_area_high: Option<(f64, f64)>,
    pub value_area_low: Option<(f64, f64)>,
}

/// One histogram row in media space. `volume_right` is the end of the total-volume bar;
/// `delta_left` is the start of the delta bar (equal to the spine for zero delta).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ProfileDrawingRow {
    pub top: f64,
    pub bottom: f64,
    pub volume_right: f64,
    pub delta_left: f64,
    pub value_area: bool,
    pub positive: bool,
}

fn css_or(slot: Option<&str>, fallback: Color) -> Color {
    slot.and_then(Color::parse_css).unwrap_or(fallback)
}

impl DrawingKind {
    /// The volume-profile drawings (fixed range and anchored).
    pub(crate) const fn is_volume_profile(self) -> bool {
        matches!(
            self,
            Self::FixedRangeVolumeProfile | Self::AnchoredVolumeProfile
        )
    }

    /// Drawings whose content is calculated from engine-owned market data.
    pub(crate) const fn is_profile_bound(self) -> bool {
        self.is_volume_profile() || matches!(self, Self::AnchoredVwap)
    }
}

impl Drawing {
    /// The fixed range box's inclusive price span, or `None` for an unbounded profile.
    pub(crate) fn profile_price_window(&self) -> Option<(f64, f64)> {
        if self.kind != DrawingKind::FixedRangeVolumeProfile || self.profile_fit_prices {
            return None;
        }
        let (a, b) = (self.points.first()?.price, self.points.get(1)?.price);
        let (low, high) = (a.min(b), a.max(b));
        let representable = low.is_finite()
            && high.is_finite()
            && high - low > f64::EPSILON * high.abs().max(low.abs()).max(1.0);
        representable.then_some((low, high))
    }

    pub(crate) fn profile_colors(&self) -> ProfileDrawingColors {
        ProfileDrawingColors {
            value_area: css_or(
                self.profile_value_area_color.as_deref(),
                PROFILE_VALUE_AREA_COLOR,
            ),
            volume: css_or(self.profile_volume_color.as_deref(), PROFILE_VOLUME_COLOR),
            positive: css_or(
                self.profile_positive_color.as_deref(),
                PROFILE_POSITIVE_COLOR,
            ),
            negative: css_or(
                self.profile_negative_color.as_deref(),
                PROFILE_NEGATIVE_COLOR,
            ),
            poc: css_or(self.profile_poc_color.as_deref(), PROFILE_POC_COLOR),
            line: Color::parse_css(&self.color).unwrap_or(crate::frame::PRIMARY),
        }
    }
}

/// The inclusive low and high of a profile's rows, or `None` without rows.
fn profile_price_extent(profile: &ProfileSnapshot) -> Option<(f64, f64)> {
    let low = profile
        .rows
        .iter()
        .map(|row| row.low)
        .fold(f64::INFINITY, f64::min);
    let high = profile
        .rows
        .iter()
        .map(|row| row.high)
        .fold(f64::NEG_INFINITY, f64::max);
    (low.is_finite() && high.is_finite()).then_some((low, high))
}

pub(crate) fn valid_profile_drawing_rows(rows: usize) -> bool {
    (1..=MAX_PROFILE_ROWS).contains(&rows)
}

pub(crate) fn valid_profile_drawing_percent(value: f64) -> bool {
    value.is_finite() && 0.0 < value && value <= 100.0
}

impl ChartEngine {
    /// Register the scalar volume series that profile drawings and other volume consumers read
    /// for one OHLC price series. `None` clears the registration. The price series must be a
    /// candlestick or bar series and the volume series a different histogram series.
    pub fn set_series_volume_source(
        &mut self,
        price_series: SeriesId,
        volume_series: Option<SeriesId>,
    ) -> bool {
        if !matches!(
            self.series_entry(price_series).map(|entry| entry.kind),
            Some(SeriesKind::Candlestick | SeriesKind::Bar)
        ) {
            return false;
        }
        if let Some(volume) = volume_series
            && (volume == price_series
                || !matches!(
                    self.series_entry(volume).map(|entry| entry.kind),
                    Some(SeriesKind::Histogram)
                ))
        {
            return false;
        }
        self.series_volume_sources
            .retain(|(price, _)| *price != price_series);
        if let Some(volume) = volume_series {
            self.series_volume_sources.push((price_series, volume));
        }
        self.invalidate_frame_drawings();
        true
    }

    /// The volume series registered for a price series.
    #[must_use]
    pub fn series_volume_source(&self, price_series: SeriesId) -> Option<SeriesId> {
        self.series_volume_sources
            .iter()
            .find(|(price, _)| *price == price_series)
            .map(|&(_, volume)| volume)
    }

    pub(crate) fn forget_series_volume_sources(&mut self, removed: &[SeriesId]) {
        self.series_volume_sources
            .retain(|(price, volume)| !removed.contains(price) && !removed.contains(volume));
        self.order_flow_tapes
            .retain(|(series, _)| !removed.contains(series));
    }

    pub(crate) fn remember_order_flow_tape(&mut self, primary_series: SeriesId, stream: u64) {
        self.order_flow_tapes
            .retain(|(series, _)| *series != primary_series);
        self.order_flow_tapes.push((primary_series, stream));
        self.invalidate_frame_drawings();
    }

    pub(crate) fn forget_order_flow_tape(&mut self, stream: u64) {
        let before = self.order_flow_tapes.len();
        self.order_flow_tapes.retain(|&(_, id)| id != stream);
        if self.order_flow_tapes.len() != before {
            self.invalidate_frame_drawings();
        }
    }

    /// The data source a profile-bound drawing reads right now, or `None` when its pane has
    /// no usable source.
    pub(crate) fn profile_drawing_source(&self, drawing: &Drawing) -> Option<ProfileSource> {
        if let Some(options) = drawing.profile.as_ref() {
            return self
                .validate_profile_source(options.source)
                .is_ok()
                .then_some(options.source);
        }
        let live_in_pane = |id: SeriesId| {
            self.series.iter().any(|series| {
                series.id == id && !series.removed && series.pane_index == drawing.pane_index
            })
        };
        if let Some(&(_, stream_id)) = self
            .order_flow_tapes
            .iter()
            .find(|&&(series, stream)| live_in_pane(series) && self.trade_stream(stream).is_some())
        {
            return Some(ProfileSource::Tape { stream_id });
        }
        if let Some(stream_id) = self
            .series
            .iter()
            .filter(|series| !series.removed && series.pane_index == drawing.pane_index)
            .filter_map(|series| series.footprint.as_ref().map(|state| state.trade_stream_id))
            .find(|&stream| self.trade_stream(stream).is_some())
        {
            return Some(ProfileSource::Tape { stream_id });
        }
        let target = match drawing.price_scale {
            DrawingPriceScale::Right => crate::PriceScaleTarget::Right,
            DrawingPriceScale::Left => crate::PriceScaleTarget::Left,
            DrawingPriceScale::Overlay => crate::PriceScaleTarget::Overlay,
        };
        let primary = self
            .primary_series_on_price_scale(drawing.pane_index, target)
            .map(|primary| primary.series_id);
        let (price_series, volume_series) = primary
            .and_then(|price| {
                self.series_volume_source(price)
                    .map(|volume| (price, volume))
            })
            .or_else(|| {
                self.series_volume_sources
                    .iter()
                    .copied()
                    .find(|&(price, _)| live_in_pane(price))
            })?;
        let source = ProfileSource::Candles {
            price_series,
            volume_series,
        };
        self.validate_profile_source(source)
            .is_ok()
            .then_some(source)
    }

    pub(crate) fn profile_drawing_key(&self, drawing: &Drawing) -> Option<ProfileDrawingKey> {
        if !drawing.kind.is_profile_bound() {
            return None;
        }
        let source = self.profile_drawing_source(drawing)?;
        let first = drawing.points.first()?.logical;
        let last = match drawing.kind {
            DrawingKind::FixedRangeVolumeProfile => drawing.points.get(1)?.logical,
            _ => self.data.merged_times().len().checked_sub(1)? as f64,
        };
        let start_micros = self.logical_timestamp_micros(first.min(last))?;
        let end_micros = self
            .logical_timestamp_micros(first.max(last))?
            .saturating_add(1);
        let (revisions, stream_tick) = match source {
            ProfileSource::Tape { stream_id } => {
                let stream = self.trade_stream(stream_id)?;
                ([stream.revision(), 0], Some(stream.options().tick_size))
            }
            ProfileSource::Candles {
                price_series,
                volume_series,
            } => (
                [
                    self.data.series_generation(price_series).unwrap_or(0),
                    self.data.series_generation(volume_series).unwrap_or(0),
                ],
                None,
            ),
        };
        let tick = drawing
            .profile
            .as_ref()
            .map(|options| options.tick_size)
            .or(stream_tick)
            .or_else(|| self.position_price_tick(drawing.pane_index, drawing.price_scale))
            .filter(|tick| tick.is_finite() && *tick > 0.0)
            .unwrap_or(FALLBACK_PROFILE_TICK);
        Some(ProfileDrawingKey {
            kind: drawing.kind,
            source,
            revisions,
            start_micros,
            end_micros,
            tick,
            rows: drawing.profile_rows,
            value_area_percent: drawing.profile_value_area_percent,
            band_multiplier: drawing
                .profile
                .as_ref()
                .map_or(1.0, |options| options.band_multiplier),
            price_window: drawing.profile_price_window(),
        })
    }

    pub(crate) fn compute_profile_drawing(
        &self,
        key: &ProfileDrawingKey,
        include_developing: bool,
    ) -> Option<ProfileDrawingSnapshot> {
        if key.kind == DrawingKind::AnchoredVwap {
            return self
                .anchored_vwap(
                    key.source,
                    key.start_micros,
                    key.end_micros,
                    key.band_multiplier,
                )
                .ok()
                .map(ProfileDrawingSnapshot::Vwap);
        }
        let tick_size = match key.source {
            ProfileSource::Tape { stream_id } => self.tape_profile_row_tick(stream_id, key)?,
            ProfileSource::Candles { .. } => key.tick,
        };
        let request = ProfileRequest {
            source: key.source,
            start_timestamp_micros: key.start_micros,
            end_timestamp_micros: key.end_micros,
            tick_size,
            row_count: key.rows,
            value_area_percent: key.value_area_percent,
        };
        match key.price_window {
            Some(window) => {
                self.volume_profile_window_snapshot(&request, window, include_developing)
            }
            None => self.volume_profile_snapshot_impl(&request, include_developing, None),
        }
        .ok()
        .map(ProfileDrawingSnapshot::Volume)
    }

    /// Tape profiles bin by price level, so merge whole instrument ticks until the traded range
    /// fits the requested row count.
    fn tape_profile_row_tick(&self, stream_id: u64, key: &ProfileDrawingKey) -> Option<f64> {
        let stream = self.trade_stream(stream_id)?;
        let (low, high) = stream
            .classified_trades()
            .map(|(trade, _)| trade)
            .filter(|trade| {
                trade.timestamp_micros >= key.start_micros
                    && trade.timestamp_micros < key.end_micros
                    && trade.price.is_finite()
                    && key
                        .price_window
                        .is_none_or(|(low, high)| low <= trade.price && trade.price <= high)
            })
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), trade| {
                (low.min(trade.price), high.max(trade.price))
            });
        if low > high {
            return Some(key.tick);
        }
        let levels = ((high - low) / key.tick).round() + 1.0;
        // Rounding to the merged grid can add one row at each end of the traded range.
        let budget = key.rows.saturating_sub(2).max(1) as f64;
        Some(key.tick * (levels / budget).ceil().max(1.0))
    }

    /// The snapshot a profile-bound drawing paints: the cached calculation while its key is
    /// current, otherwise a fresh one (creation previews and drags before the next refresh).
    pub(crate) fn profile_drawing_data(
        &self,
        drawing: &Drawing,
    ) -> Option<Cow<'_, ProfileDrawingSnapshot>> {
        let key = self.profile_drawing_key(drawing)?;
        if let Some(entry) = self
            .profile_drawing_cache
            .iter()
            .find(|entry| entry.id == drawing.id && entry.key == key)
        {
            return entry.snapshot.as_ref().map(Cow::Borrowed);
        }
        self.compute_profile_drawing(&key, false).map(Cow::Owned)
    }

    /// Recalculate only profile drawings whose inputs changed, then snap fitted fixed range
    /// boxes to their traded range. Entries are bounded by live profile drawings; a change
    /// invalidates the retained drawing layer once.
    pub(crate) fn refresh_profile_drawings(&mut self) {
        let before = self.profile_drawing_cache.len();
        let drawings = &self.drawings;
        self.profile_drawing_cache.retain(|entry| {
            drawings
                .iter()
                .any(|drawing| drawing.id == entry.id && drawing.kind.is_profile_bound())
        });
        let mut changed = self.profile_drawing_cache.len() != before;
        let mut updates = Vec::new();
        for drawing in self.drawings.iter().filter(|d| d.kind.is_profile_bound()) {
            let key = self.profile_drawing_key(drawing);
            let cached = self
                .profile_drawing_cache
                .iter()
                .position(|entry| entry.id == drawing.id);
            match (key, cached) {
                (None, None) => {}
                (Some(key), Some(index)) if self.profile_drawing_cache[index].key == key => {}
                (key, _) => updates.push((
                    drawing.id,
                    key.map(|key| (key, self.compute_profile_drawing(&key, false))),
                )),
            }
        }
        for (id, update) in updates {
            changed = true;
            self.profile_drawing_cache.retain(|entry| entry.id != id);
            if let Some((key, snapshot)) = update {
                self.profile_drawing_cache
                    .push(ProfileDrawingCacheEntry { id, key, snapshot });
                self.profile_drawing_recalculations =
                    self.profile_drawing_recalculations.wrapping_add(1);
            }
        }
        let mut fitted = Vec::new();
        for drawing in self.drawings.iter().filter(|drawing| {
            drawing.kind == DrawingKind::FixedRangeVolumeProfile
                && drawing.profile_fit_prices
                && drawing.points.len() == 2
        }) {
            let Some(ProfileDrawingSnapshot::Volume(profile)) = self
                .profile_drawing_cache
                .iter()
                .find(|entry| entry.id == drawing.id)
                .and_then(|entry| entry.snapshot.as_ref())
            else {
                continue;
            };
            let Some((low, high)) = profile_price_extent(profile) else {
                continue;
            };
            // The anchor that was higher stays the top corner.
            let (top, bottom) = if drawing.points[0].price >= drawing.points[1].price {
                (0, 1)
            } else {
                (1, 0)
            };
            if drawing.points[top].price != high || drawing.points[bottom].price != low {
                fitted.push((drawing.id, top, bottom, low, high));
            }
        }
        for (id, top, bottom, low, high) in fitted {
            if let Some(drawing) = self.drawings.iter_mut().find(|drawing| drawing.id == id) {
                drawing.points[top].price = high;
                drawing.points[bottom].price = low;
            }
            self.update_drawing_runtime(id);
            changed = true;
        }
        if changed {
            self.invalidate_frame_drawings();
        }
    }

    /// Number of profile drawing calculations performed by frame refreshes.
    #[must_use]
    pub fn profile_drawing_recalculations(&self) -> u64 {
        self.profile_drawing_recalculations
    }

    /// Media-space layout of a volume profile drawing. `anchors_x` are the drawing's rendered
    /// anchor x positions (media px); the price axis resolves row edges and levels. A fixed
    /// range box has a layout before any data arrives; an anchored profile needs its rows.
    pub(crate) fn profile_drawing_geometry(
        &self,
        drawing: &Drawing,
        profile: Option<&ProfileSnapshot>,
        anchors_x: &[f64],
    ) -> Option<ProfileDrawingGeometry> {
        let first = *anchors_x.first()?;
        let (spine, right) = match drawing.kind {
            DrawingKind::FixedRangeVolumeProfile => {
                let second = *anchors_x.get(1)?;
                (first.min(second), first.max(second))
            }
            DrawingKind::AnchoredVolumeProfile => {
                let last = self.data.merged_times().len().checked_sub(1)? as f64;
                let anchor = drawing.points.first()?;
                let end = self
                    .drawing_to_px_for(
                        drawing.pane_index,
                        drawing.price_scale,
                        DrawingPoint {
                            logical: last,
                            price: anchor.price,
                        },
                    )?
                    .0;
                (first, end.max(first))
            }
            _ => return None,
        };
        let (mut low, mut high) = profile
            .and_then(profile_price_extent)
            .unwrap_or((f64::INFINITY, f64::NEG_INFINITY));
        if let Some(window) = drawing.profile_price_window() {
            (low, high) = window;
        } else if !(high.is_finite() && low.is_finite())
            && drawing.kind == DrawingKind::FixedRangeVolumeProfile
        {
            let (a, b) = (drawing.points.first()?.price, drawing.points.get(1)?.price);
            (low, high) = (a.min(b), a.max(b));
        }
        if !(high.is_finite() && low.is_finite()) {
            return None;
        }
        let y = |price: f64| self.profile_price_y(drawing, price);
        let (top, bottom) = (y(high)?, y(low)?);
        let side = (right - spine).max(0.0) * drawing.profile_width_percent / 100.0;
        let left = if drawing.profile_delta_visible {
            spine - side
        } else {
            spine
        };
        let level = |price: Option<f64>| price.and_then(|price| Some((price, y(price)?)));
        Some(ProfileDrawingGeometry {
            spine,
            right,
            left,
            side,
            top: top.min(bottom),
            bottom: top.max(bottom),
            poc: level(profile.and_then(|profile| profile.poc)),
            value_area_high: level(profile.and_then(|profile| profile.value_area_high)),
            value_area_low: level(profile.and_then(|profile| profile.value_area_low)),
        })
    }

    fn profile_price_y(&self, drawing: &Drawing, price: f64) -> Option<f64> {
        let logical = drawing.points.first()?.logical;
        self.drawing_to_px_for(
            drawing.pane_index,
            drawing.price_scale,
            DrawingPoint { logical, price },
        )
        .map(|(_, y)| y)
    }

    /// Visit each non-empty row of a volume profile drawing in media space.
    pub(crate) fn for_each_profile_drawing_row(
        &self,
        drawing: &Drawing,
        profile: &ProfileSnapshot,
        geometry: &ProfileDrawingGeometry,
        mut visit: impl FnMut(ProfileDrawingRow),
    ) {
        let max_volume = profile
            .rows
            .iter()
            .map(|row| row.total_volume)
            .fold(0.0_f64, f64::max);
        let max_delta = profile
            .rows
            .iter()
            .map(|row| row.delta.abs())
            .fold(0.0_f64, f64::max);
        if max_volume <= 0.0 {
            return;
        }
        let epsilon = profile.tick_size.abs() * 1e-6;
        let (area_low, area_high) = (
            profile.value_area_low.unwrap_or(f64::INFINITY),
            profile.value_area_high.unwrap_or(f64::NEG_INFINITY),
        );
        for row in &profile.rows {
            if row.total_volume <= 0.0 {
                continue;
            }
            let (Some(y0), Some(y1)) = (
                self.profile_price_y(drawing, row.high),
                self.profile_price_y(drawing, row.low),
            ) else {
                continue;
            };
            let center = (row.low + row.high) * 0.5;
            let delta = if drawing.profile_delta_visible && max_delta > 0.0 {
                geometry.side * row.delta.abs() / max_delta
            } else {
                0.0
            };
            visit(ProfileDrawingRow {
                top: y0.min(y1),
                bottom: y0.max(y1),
                volume_right: geometry.spine + geometry.side * row.total_volume / max_volume,
                delta_left: geometry.spine - delta,
                value_area: center >= area_low - epsilon && center <= area_high + epsilon,
                positive: row.delta >= 0.0,
            });
        }
    }

    /// Whether a media-space point lies on a volume profile drawing's painted extent: the
    /// histogram on either side of the spine, or a fixed range box edge.
    pub(crate) fn profile_drawing_body_hit(
        &self,
        drawing: &Drawing,
        anchors_x: &[f64],
        x: f64,
        y: f64,
        tolerance: f64,
    ) -> Option<bool> {
        let data = self.profile_drawing_data(drawing);
        let profile = match data.as_deref() {
            Some(ProfileDrawingSnapshot::Volume(profile)) => Some(profile),
            Some(ProfileDrawingSnapshot::Vwap(_)) => return None,
            None if drawing.kind == DrawingKind::FixedRangeVolumeProfile => None,
            None => return None,
        };
        let geometry = self.profile_drawing_geometry(drawing, profile, anchors_x)?;
        let within_rows = y >= geometry.top - tolerance && y <= geometry.bottom + tolerance;
        let boxed = drawing.kind == DrawingKind::FixedRangeVolumeProfile;
        let on_spine = within_rows && (x - geometry.spine).abs() <= tolerance;
        let within_span = x >= geometry.spine - tolerance && x <= geometry.right + tolerance;
        let on_box = boxed
            && ((within_span
                && ((y - geometry.top).abs() <= tolerance
                    || (y - geometry.bottom).abs() <= tolerance))
                || (within_rows && (x - geometry.right).abs() <= tolerance));
        // Like a rectangle, the histogram and box interior grab the drawing only once it is
        // selected; unselected, they pan the chart like empty space.
        let grabs_interior = self.is_drawing_selected(drawing.id)
            && within_rows
            && x >= geometry.left - tolerance
            && x <= geometry.right.max(geometry.spine + geometry.side) + tolerance;
        Some(on_spine || on_box || grabs_interior)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeris_charts_render::draw_list::Prim;

    fn chart(dpr: f64) -> (ChartEngine, SeriesId) {
        let mut chart = ChartEngine::new(800.0, 500.0, dpr);
        let times = [0.0, 60.0, 120.0, 180.0, 240.0, 300.0];
        let opens = [10.0, 11.0, 12.0, 11.5, 12.5, 12.0];
        let highs = [10.5, 11.5, 12.5, 12.0, 13.0, 12.6];
        let lows = [9.5, 10.5, 11.5, 11.0, 12.0, 11.4];
        let closes = [11.0, 12.0, 11.5, 12.5, 12.8, 11.6];
        chart
            .set_series_data(0, &times, &opens, &highs, &lows, &closes)
            .unwrap();
        let volume = chart.add_series(SeriesKind::Histogram);
        let volumes = [5.0, 8.0, 20.0, 9.0, 6.0, 4.0];
        chart
            .set_series_data(volume, &times, &volumes, &volumes, &volumes, &volumes)
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        (chart, volume)
    }

    fn fixed_range(chart: &mut ChartEngine) -> DrawingId {
        chart
            .add_drawing(
                DrawingKind::FixedRangeVolumeProfile,
                0,
                vec![
                    DrawingPoint {
                        logical: 1.0,
                        price: 11.0,
                    },
                    DrawingPoint {
                        logical: 4.0,
                        price: 11.0,
                    },
                ],
                None,
            )
            .unwrap()
    }

    fn rects(prims: &[Prim], color: Color) -> Vec<aeris_charts_render::draw_list::IRect> {
        prims
            .iter()
            .filter_map(|prim| match prim {
                Prim::Rect { rect, color: c } if *c == color => Some(*rect),
                _ => None,
            })
            .collect()
    }

    fn spine_x(prims: &[Prim]) -> Option<i32> {
        prims.iter().find_map(|prim| match prim {
            Prim::VLine { x, .. } => Some(*x),
            _ => None,
        })
    }

    #[test]
    fn drawn_profile_binds_the_registered_volume_without_host_configuration() {
        let (mut chart, volume) = chart(1.0);
        let id = fixed_range(&mut chart);
        let unbound = chart.build_frame();
        assert!(rects(&unbound.panes[0].main, PROFILE_VALUE_AREA_COLOR).is_empty());

        assert!(chart.set_series_volume_source(0, Some(volume)));
        assert_eq!(chart.series_volume_source(0), Some(volume));
        let frame = chart.build_frame();
        let main = &frame.panes[0].main;
        let spine = spine_x(main).expect("profile spine");
        let value_area = rects(main, PROFILE_VALUE_AREA_COLOR);
        assert!(
            !value_area.is_empty(),
            "value-area rows paint right of the spine"
        );
        assert!(value_area.iter().all(|rect| rect.x == spine && rect.w > 0));
        let delta = [PROFILE_POSITIVE_COLOR, PROFILE_NEGATIVE_COLOR]
            .into_iter()
            .flat_map(|color| rects(main, color))
            .collect::<Vec<_>>();
        assert!(!delta.is_empty(), "delta rows paint left of the spine");
        assert!(delta.iter().all(|rect| rect.x + rect.w == spine));
        assert!(main.iter().any(|prim| matches!(prim,
            Prim::HLine { color, .. } if *color == PROFILE_POC_COLOR)));
        let Ok(ProfileDrawingSnapshot::Volume(snapshot)) = chart.profile_drawing_snapshot(id)
        else {
            panic!("volume snapshot");
        };
        assert!(snapshot.candle_approximation);
        assert!(snapshot.poc.is_some());

        // Removing the volume series releases the binding and the profile.
        chart.remove_series(volume);
        assert_eq!(chart.series_volume_source(0), None);
        let released = chart.build_frame();
        assert!(rects(&released.panes[0].main, PROFILE_VALUE_AREA_COLOR).is_empty());
    }

    #[test]
    fn registration_rejects_mismatched_series_kinds() {
        let (mut chart, volume) = chart(1.0);
        let line = chart.add_series(SeriesKind::Line);
        assert!(!chart.set_series_volume_source(volume, Some(volume)));
        assert!(!chart.set_series_volume_source(0, Some(line)));
        assert!(!chart.set_series_volume_source(0, Some(0)));
        assert!(chart.set_series_volume_source(0, Some(volume)));
        assert!(chart.set_series_volume_source(0, None));
        assert_eq!(chart.series_volume_source(0), None);
    }

    #[test]
    fn order_flow_tape_takes_precedence_over_candle_volume() {
        let (mut chart, volume) = chart(1.0);
        chart.set_series_volume_source(0, Some(volume));
        let presentation = chart
            .add_order_flow_presentation(
                "ES",
                0,
                crate::OrderFlowPresentationOptions {
                    aggregation: crate::FootprintAggregationOptions {
                        tick_size: 0.5,
                        ticks_per_row: 1,
                        ..crate::FootprintAggregationOptions::default()
                    },
                    visual: crate::FootprintVisualOptions::default(),
                    show_footprint: false,
                    show_cumulative_delta: false,
                    show_delta_histogram: false,
                    cumulative_delta: crate::TradeStudyOptions::default(),
                    delta_histogram: crate::TradeStudyOptions::default(),
                    big_trades: None,
                },
            )
            .unwrap();
        let trade = |timestamp_micros: i64, price: f64, aggressor| crate::FootprintTrade {
            timestamp_micros,
            price,
            volume: 2.0,
            aggressor,
            bid: None,
            ask: None,
            sequence: None,
            trade_id: None,
            conditions: 0,
            session_id: Some(1),
        };
        chart
            .update_order_flow_presentation(
                presentation,
                vec![
                    trade(60_000_000, 11.0, crate::AggressorSide::Buy),
                    trade(120_000_000, 12.0, crate::AggressorSide::Sell),
                    trade(180_000_000, 12.0, crate::AggressorSide::Sell),
                ],
                false,
            )
            .unwrap();
        let id = fixed_range(&mut chart);
        let Ok(ProfileDrawingSnapshot::Volume(snapshot)) = chart.profile_drawing_snapshot(id)
        else {
            panic!("volume snapshot");
        };
        assert!(!snapshot.candle_approximation, "the tape is preferred");
        assert_eq!(snapshot.poc, Some(12.0));
        let frame = chart.build_frame();
        assert!(!rects(&frame.panes[0].main, PROFILE_NEGATIVE_COLOR).is_empty());

        assert!(chart.remove_order_flow_presentation(presentation));
        let Ok(ProfileDrawingSnapshot::Volume(snapshot)) = chart.profile_drawing_snapshot(id)
        else {
            panic!("candle fallback");
        };
        assert!(snapshot.candle_approximation);
    }

    #[test]
    fn unchanged_inputs_reuse_the_calculation_and_data_changes_refresh_it() {
        let (mut chart, volume) = chart(1.0);
        chart.set_series_volume_source(0, Some(volume));
        let id = fixed_range(&mut chart);
        chart.build_frame();
        let first = chart.profile_drawing_recalculations();
        assert_eq!(first, 1);
        chart.build_frame();
        chart.time_scale.set_right_offset(3.0);
        chart.build_frame();
        assert_eq!(
            chart.profile_drawing_recalculations(),
            first,
            "panning reuses the cache"
        );
        assert!(chart.update_series_bar(volume, 180.0, [90.0; 4]));
        chart.build_frame();
        assert_eq!(chart.profile_drawing_recalculations(), first + 1);
        assert!(chart.drawing_apply_options(id, r#"{"profile_rows":12}"#));
        chart.build_frame();
        assert_eq!(chart.profile_drawing_recalculations(), first + 2);
        chart.remove_drawing(id);
        chart.build_frame();
        assert!(chart.profile_drawing_cache.is_empty());
    }

    #[test]
    fn options_validate_and_drive_the_painted_levels() {
        let (mut chart, volume) = chart(1.0);
        chart.set_series_volume_source(0, Some(volume));
        let id = fixed_range(&mut chart);
        assert!(!chart.drawing_apply_options(id, r#"{"profile_rows":0}"#));
        assert!(!chart.drawing_apply_options(id, r#"{"profile_width_percent":120}"#));
        assert!(!chart.drawing_apply_options(id, r#"{"profile_value_area_percent":0}"#));
        let trend = chart
            .add_drawing(
                DrawingKind::TrendLine,
                0,
                vec![
                    DrawingPoint {
                        logical: 0.0,
                        price: 10.0,
                    },
                    DrawingPoint {
                        logical: 2.0,
                        price: 11.0,
                    },
                ],
                None,
            )
            .unwrap();
        assert!(!chart.drawing_apply_options(trend, r#"{"profile_rows":10}"#));

        assert!(chart.drawing_apply_options(
            id,
            r##"{"profile_delta_visible":false,"profile_poc_color":"#00ff00","extend_right":true}"##
        ));
        let frame = chart.build_frame();
        let main = &frame.panes[0].main;
        assert!(rects(main, PROFILE_POSITIVE_COLOR).is_empty());
        assert!(rects(main, PROFILE_NEGATIVE_COLOR).is_empty());
        let pane_w = chart.pane_w.round() as i32;
        assert!(main.iter().any(|prim| matches!(prim,
            Prim::HLine { color, x1, .. }
                if *color == Color::rgb(0, 255, 0) && (*x1 - pane_w).abs() <= 1)));

        assert!(chart.drawing_apply_options(
            id,
            r#"{"profile_poc_visible":false,"profile_value_area_lines_visible":false}"#
        ));
        let frame = chart.build_frame();
        assert!(!frame.panes[0].main.iter().any(|prim| matches!(prim,
            Prim::HLine { color, .. } if *color == Color::rgb(0, 255, 0))));
    }

    #[test]
    fn the_histogram_pans_until_selected_while_the_spine_always_grabs() {
        let (mut chart, volume) = chart(1.0);
        chart.set_series_volume_source(0, Some(volume));
        let id = fixed_range(&mut chart);
        let frame = chart.build_frame();
        let main = &frame.panes[0].main;
        let spine = f64::from(spine_x(main).unwrap());
        let delta = [PROFILE_POSITIVE_COLOR, PROFILE_NEGATIVE_COLOR]
            .into_iter()
            .flat_map(|color| rects(main, color))
            .max_by_key(|rect| rect.w)
            .unwrap();
        let x = f64::from(delta.x) + 1.0;
        let y = f64::from(delta.y) + f64::from(delta.h) / 2.0;
        assert_eq!(
            chart.hit_test_drawing(x, y),
            None,
            "no barrier over the chart"
        );
        assert_eq!(chart.hit_test_drawing(spine, y).map(|hit| hit.id), Some(id));
        chart.set_selected_drawing(Some(id));
        assert_eq!(chart.hit_test_drawing(x, y).map(|hit| hit.id), Some(id));
    }

    #[test]
    fn frame_geometry_scales_with_the_device_pixel_ratio() {
        let spine = |dpr: f64| {
            let (mut chart, volume) = chart(dpr);
            chart.set_series_volume_source(0, Some(volume));
            fixed_range(&mut chart);
            spine_x(&chart.build_frame().panes[0].main).unwrap()
        };
        let (one, two) = (spine(1.0), spine(2.0));
        assert!(
            (two - one * 2).abs() <= 1,
            "spine at {one} css px, {two} device px"
        );
    }

    #[test]
    fn axis_tags_name_the_point_of_control_and_value_area() {
        let (mut chart, volume) = chart(1.0);
        chart.set_series_volume_source(0, Some(volume));
        let id = fixed_range(&mut chart);
        let poc_tags = |chart: &mut ChartEngine| {
            chart.build_frame();
            let axis = chart.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
            axis.labels
                .iter()
                .filter(|label| {
                    label
                        .background
                        .is_some_and(|background| background.4 == PROFILE_POC_COLOR)
                })
                .count()
        };
        assert_eq!(poc_tags(&mut chart), 0, "axis tags are opt-in");
        assert!(chart.drawing_apply_options(id, r#"{"price_labels_visible":true}"#));
        assert_eq!(poc_tags(&mut chart), 1);
    }

    #[test]
    fn settings_survive_persistence() {
        let (mut chart, volume) = chart(1.0);
        chart.set_series_volume_source(0, Some(volume));
        let id = fixed_range(&mut chart);
        assert!(chart.drawing_apply_options(
            id,
            r##"{"profile_rows":37,"profile_width_percent":55,"profile_delta_visible":false,"profile_volume_color":"#123456","price_labels_visible":true,"profile_fit_prices":false}"##
        ));
        let json = chart.export_state_json().unwrap();
        let (mut restored, _) = self::chart(1.0);
        restored.import_state_json(&json).unwrap();
        let drawing = restored.drawings.first().unwrap();
        assert_eq!(drawing.profile_rows, 37);
        assert_eq!(drawing.profile_width_percent, 55.0);
        assert!(!drawing.profile_delta_visible);
        assert!(drawing.price_labels_visible);
        assert!(!drawing.profile_fit_prices);
        assert_eq!(drawing.profile_volume_color.as_deref(), Some("#123456"));
    }

    #[test]
    fn schema_exposes_profile_settings() {
        let schema =
            crate::drawing_contract::drawing_property_schema(DrawingKind::FixedRangeVolumeProfile);
        for name in [
            "profile_rows",
            "profile_value_area_percent",
            "profile_width_percent",
            "profile_delta_visible",
            "profile_poc_color",
            "extend_right",
            "price_labels_visible",
            "color",
        ] {
            assert!(
                schema
                    .properties
                    .iter()
                    .any(|property| property.name == name),
                "{name}"
            );
        }
    }
}
