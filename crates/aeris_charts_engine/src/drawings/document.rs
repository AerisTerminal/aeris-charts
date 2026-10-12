//! Portable drawing documents. A host persists every committed drawing with the chart position
//! each anchor denotes, then restores the drawings onto a chart showing another history window,
//! interval, or session of the same instrument.
//!
//! Live anchors stay in `{logical, price}` space. The document adds what a bar index cannot carry
//! across data sets: on a time axis, the UTC time of each anchor (bar open times inside the
//! loaded bars, whole projection steps beyond either edge, linear in between); on a sequence
//! axis, the identity of the anchor's bar. Restoring resolves those against the receiving chart,
//! so a shape keeps its time extent instead of stretching with whatever bar gap happens to sit at
//! the edge of the loaded history.

use std::collections::HashMap;

use serde_json::Value;

use super::{
    BarsPatternBar, Drawing, DrawingAnchorTime, DrawingHistory, DrawingId, DrawingKind,
    DrawingPatch, DrawingPoint, MAX_BARS_PATTERN_BARS, MAX_DRAWING_POINTS,
};
use crate::persistence::{
    PERSISTENCE_MAX_DOCUMENT_BYTES, PERSISTENCE_MAX_DRAWINGS, PERSISTENCE_MAX_TOTAL_POINTS,
};
use crate::{ChartEngine, ChartError, ErrorCode};

/// `schema` of a document written by [`ChartEngine::export_drawings_json`].
pub const DRAWINGS_DOCUMENT_SCHEMA: &str = "aeris_charts-drawings";
/// `schema_version` of a document written by [`ChartEngine::export_drawings_json`].
pub const DRAWINGS_DOCUMENT_VERSION: u32 = 1;

/// One drawing restored by [`ChartEngine::import_drawings_json`]. Drawing ids are chart-local
/// handles, so every restored drawing receives a fresh one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportedDrawing {
    /// The `id` the drawing carried in the document, when present.
    pub document_id: Option<DrawingId>,
    /// The live id the drawing has now.
    pub id: DrawingId,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DocumentPoint {
    logical: f64,
    price: f64,
    /// UTC seconds the anchor denotes on a time axis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    time: Option<f64>,
    /// The anchor's bar on a sequence (non-time) axis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sequence_bar: Option<DrawingAnchorTime>,
}

#[derive(serde::Deserialize)]
struct DocumentItem {
    #[serde(default)]
    id: Option<DrawingId>,
    kind: String,
    pane_index: usize,
    points: Vec<DocumentPoint>,
    #[serde(default)]
    bars_pattern: Vec<BarsPatternBar>,
}

fn invalid(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::InvalidData, message)
}

fn resource(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::ResourceLimit, message)
}

fn serialization(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::SerializationError, message)
}

fn pending_at<T: Copy>(pending: Option<&Vec<Option<T>>>, index: usize) -> Option<T> {
    pending?.get(index).copied().flatten()
}

impl ChartEngine {
    /// Every committed drawing as a versioned document, in z-order. Each entry is the
    /// [`Self::drawings_json`] item plus restore data: anchors carry `time` (UTC seconds) on a
    /// time axis or `sequence_bar` on a sequence axis, and bars patterns carry their frozen bars.
    ///
    /// # Errors
    ///
    /// Returns an error when the document cannot be serialized or exceeds the persistence size
    /// limit.
    pub fn export_drawings_json(&self) -> Result<String, ChartError> {
        let time_axis = self.sequence_points().is_none();
        let drawings = self
            .drawings
            .iter()
            .map(|drawing| {
                let sequence_bars = self.drawing_anchor_times_for(drawing);
                let pending_bars = self.drawing_anchor_times.get(&drawing.id);
                let pending_times = self.drawing_time_anchors.get(&drawing.id);
                let points = drawing
                    .points
                    .iter()
                    .enumerate()
                    .map(|(index, point)| DocumentPoint {
                        logical: point.logical,
                        price: point.price,
                        time: pending_at(pending_times, index).or_else(|| {
                            time_axis
                                .then(|| self.drawing_time_at_logical(point.logical))
                                .flatten()
                        }),
                        sequence_bar: pending_at(pending_bars, index)
                            .or_else(|| sequence_bars.get(index).copied().flatten()),
                    })
                    .collect::<Vec<_>>();
                let mut item = serde_json::json!({
                    "id": drawing.id,
                    "kind": drawing.kind.name(),
                    "pane_index": drawing.pane_index,
                    "points": points,
                });
                if let (Value::Object(map), Value::Object(options)) =
                    (&mut item, drawing.options_json())
                {
                    map.extend(options);
                }
                if drawing.kind == DrawingKind::BarsPattern {
                    item["bars_pattern"] = serde_json::json!(drawing.bars_pattern);
                }
                item
            })
            .collect::<Vec<_>>();
        let document = serde_json::to_string(&serde_json::json!({
            "schema": DRAWINGS_DOCUMENT_SCHEMA,
            "schema_version": DRAWINGS_DOCUMENT_VERSION,
            "drawings": drawings,
        }))
        .map_err(|error| serialization(error.to_string()))?;
        if document.len() > PERSISTENCE_MAX_DOCUMENT_BYTES {
            return Err(resource(
                "drawings document exceeds the persistence size limit",
            ));
        }
        Ok(document)
    }

    /// Replace every committed drawing with a document written by [`Self::export_drawings_json`],
    /// as one restore rather than a user edit: undo history starts empty and the drawing revision
    /// advances once. Anchors resolve by time (or sequence bar) against the bars installed now;
    /// on a chart without bars yet they resolve when the first bars arrive. Bars patterns keep
    /// their frozen bars. The whole document is validated before anything changes.
    ///
    /// # Errors
    ///
    /// Returns an error, leaving the drawings untouched, when the document is malformed, has an
    /// unsupported schema or version, exceeds the persistence limits, names an unknown kind or a
    /// pane this chart lacks, or carries invalid anchors or options.
    pub fn import_drawings_json(&mut self, json: &str) -> Result<Vec<ImportedDrawing>, ChartError> {
        if json.len() > PERSISTENCE_MAX_DOCUMENT_BYTES {
            return Err(resource(
                "drawings document exceeds the persistence size limit",
            ));
        }
        let envelope: Value = serde_json::from_str(json)
            .map_err(|error| serialization(format!("malformed drawings document: {error}")))?;
        if envelope.get("schema").and_then(Value::as_str) != Some(DRAWINGS_DOCUMENT_SCHEMA) {
            return Err(serialization(
                "unsupported or missing drawings document schema",
            ));
        }
        let version = envelope
            .get("schema_version")
            .and_then(Value::as_u64)
            .ok_or_else(|| serialization("schema_version must be an integer"))?;
        if version != u64::from(DRAWINGS_DOCUMENT_VERSION) {
            return Err(ChartError::new(
                ErrorCode::PersistenceVersionError,
                format!("unsupported drawings document version {version}"),
            ));
        }
        let items = envelope
            .get("drawings")
            .and_then(Value::as_array)
            .ok_or_else(|| serialization("drawings document has no drawings array"))?;
        if items.len() > PERSISTENCE_MAX_DRAWINGS {
            return Err(resource(format!(
                "drawings document holds more than {PERSISTENCE_MAX_DRAWINGS} drawings"
            )));
        }
        let first_id = self.next_drawing_id;
        let next_drawing_id = u32::try_from(items.len())
            .ok()
            .and_then(|count| first_id.checked_add(count))
            .ok_or_else(|| resource("drawing identity space is exhausted"))?;
        let sequence_index = self.sequence_points().map(|points| {
            points
                .iter()
                .enumerate()
                .map(|(index, point)| {
                    (
                        (point.open_timestamp_micros, point.close_timestamp_micros),
                        index,
                    )
                })
                .collect::<HashMap<_, _>>()
        });
        let time_axis_has_bars = sequence_index.is_none() && !self.data.merged_times().is_empty();

        let mut drawings = Vec::with_capacity(items.len());
        let mut imported = Vec::with_capacity(items.len());
        let mut pending_bars = HashMap::new();
        let mut pending_times = HashMap::new();
        let mut total_points = 0_usize;
        for ((position, value), id) in items.iter().enumerate().zip(first_id..) {
            let item = serde_json::from_value::<DocumentItem>(value.clone())
                .map_err(|error| invalid(format!("drawing {position} is malformed: {error}")))?;
            let patch = serde_json::from_value::<DrawingPatch>(value.clone()).map_err(|error| {
                invalid(format!("drawing {position} has malformed options: {error}"))
            })?;
            let kind = DrawingKind::from_name(&item.kind).ok_or_else(|| {
                invalid(format!(
                    "drawing {position} has unknown kind {:?}",
                    item.kind
                ))
            })?;
            if item.pane_index >= self.panes.len()
                || !self.pane_uses_financial_time(item.pane_index)
            {
                return Err(invalid(format!(
                    "drawing {position} references pane {} which this chart lacks",
                    item.pane_index
                )));
            }
            if item.points.len() > MAX_DRAWING_POINTS || !kind.valid_point_count(item.points.len())
            {
                return Err(invalid(format!(
                    "drawing {position} has an invalid anchor count"
                )));
            }
            total_points = total_points.saturating_add(item.points.len());
            if total_points > PERSISTENCE_MAX_TOTAL_POINTS {
                return Err(resource(format!(
                    "drawings document holds more than {PERSISTENCE_MAX_TOTAL_POINTS} anchors"
                )));
            }
            if item.points.iter().any(|point| {
                !point.logical.is_finite()
                    || !point.price.is_finite()
                    || point.time.is_some_and(|time| !time.is_finite())
            }) {
                return Err(invalid(format!("drawing {position} has an invalid anchor")));
            }

            let mut points = Vec::with_capacity(item.points.len());
            let mut unresolved_times = Vec::with_capacity(item.points.len());
            let mut unresolved_bars = Vec::with_capacity(item.points.len());
            for point in &item.points {
                let mut logical = point.logical;
                let mut unresolved_time = None;
                let mut unresolved_bar = point.sequence_bar;
                if let Some(index) = &sequence_index {
                    if let Some(bar) = point.sequence_bar
                        && let Some(&found) =
                            index.get(&(bar.open_timestamp_micros, bar.close_timestamp_micros))
                    {
                        logical = found as f64 + (point.logical - point.logical.round());
                        unresolved_bar = None;
                    }
                } else if let Some(time) = point.time {
                    if time_axis_has_bars {
                        if let Some(resolved) = self.drawing_logical_at_time(time) {
                            logical = resolved;
                        }
                    } else {
                        unresolved_time = Some(time);
                    }
                }
                points.push(DrawingPoint {
                    logical,
                    price: point.price,
                });
                unresolved_times.push(unresolved_time);
                unresolved_bars.push(unresolved_bar);
            }
            if unresolved_times.iter().any(Option::is_some) {
                pending_times.insert(id, unresolved_times);
            }
            if unresolved_bars.iter().any(Option::is_some) {
                pending_bars.insert(id, unresolved_bars);
            }

            let mut drawing = Drawing::new(id, kind, item.pane_index, points);
            if kind == DrawingKind::AnchoredText
                && let (Some(pane), Some(&point)) =
                    (self.panes.get(item.pane_index), drawing.points.first())
                && let Some((x, y)) =
                    self.drawing_to_px_for(item.pane_index, drawing.price_scale, point)
            {
                drawing.screen_x = (x / self.pane_w.max(1.0)).clamp(0.0, 1.0);
                drawing.screen_y = ((y - pane.top) / pane.height.max(1.0)).clamp(0.0, 1.0);
            }
            if !drawing.apply_patch(patch) {
                return Err(invalid(format!("drawing {position} has invalid options")));
            }
            if kind == DrawingKind::BarsPattern {
                drawing.bars_pattern = if item.bars_pattern.is_empty() {
                    self.capture_bars_pattern(&drawing).ok_or_else(|| {
                        invalid(format!(
                            "drawing {position} has no bars pattern and no source bars"
                        ))
                    })?
                } else if item.bars_pattern.len() > MAX_BARS_PATTERN_BARS
                    || !item.bars_pattern.iter().all(|bar| bar.valid())
                {
                    return Err(invalid(format!(
                        "drawing {position} has an invalid bars pattern"
                    )));
                } else {
                    item.bars_pattern
                };
            }
            imported.push(ImportedDrawing {
                document_id: item.id,
                id,
            });
            drawings.push(drawing);
        }

        self.invalidate_frame_drawings();
        self.drawings = drawings;
        self.drawing_anchor_times = pending_bars;
        self.drawing_time_anchors = pending_times;
        self.next_drawing_id = next_drawing_id;
        // Ids from before the restore no longer name drawings. An armed tool and an in-flight
        // placement survive: they create a new drawing rather than edit a replaced one.
        self.selected_drawing = None;
        self.selected_drawings.clear();
        self.drawing_drag = None;
        self.editing_drawing = None;
        self.drawing_text_edit = None;
        self.hovered_drawing = None;
        self.hovered_text = None;
        self.drawing_history = DrawingHistory::default();
        self.drawing_revision = self.drawing_revision.wrapping_add(1);
        self.bump_drawing_sync_revision();
        self.drawing_runtime
            .borrow_mut()
            .rebuild_all(&self.drawings, self.panes.len());
        Ok(imported)
    }

    /// Drops the persisted identities of a drawing whose anchors were edited or removed.
    pub(crate) fn forget_persisted_anchors(&mut self, id: DrawingId) {
        self.drawing_anchor_times.remove(&id);
        self.drawing_time_anchors.remove(&id);
    }

    /// Resolves imported time anchors once a time axis first has bars.
    pub(crate) fn resolve_pending_drawing_times(&mut self) {
        if self.drawing_time_anchors.is_empty()
            || self.sequence_points().is_some()
            || self.data.merged_times().is_empty()
        {
            return;
        }
        let pending = std::mem::take(&mut self.drawing_time_anchors);
        let resolved = self
            .drawings
            .iter()
            .enumerate()
            .filter_map(|(index, drawing)| {
                let times = pending.get(&drawing.id)?;
                Some((
                    index,
                    times
                        .iter()
                        .map(|time| time.and_then(|time| self.drawing_logical_at_time(time)))
                        .collect::<Vec<_>>(),
                ))
            })
            .collect::<Vec<_>>();
        let mut changed = false;
        for (index, logicals) in resolved {
            for (point, logical) in self.drawings[index].points.iter_mut().zip(logicals) {
                if let Some(logical) = logical
                    && logical != point.logical
                {
                    point.logical = logical;
                    changed = true;
                }
            }
        }
        if changed {
            self.drawing_runtime
                .borrow_mut()
                .rebuild_all(&self.drawings, self.panes.len());
            self.invalidate_frame_drawings();
        }
    }

    /// Seconds between whole axis slots past the newest bar (`future`) or before the oldest: the
    /// host's projection cadence, else the gap between the two outermost bars.
    fn drawing_edge_step(&self, future: bool) -> Option<f64> {
        let projection = if future {
            self.future_time_projection
        } else {
            self.past_time_projection
        };
        if let Some((step, _)) = projection {
            return Some(step as f64);
        }
        let times = self.data.merged_times();
        let gap = if future {
            let before_last = times.len().checked_sub(2)?;
            times[before_last + 1] - times[before_last]
        } else {
            times.get(1)? - times[0]
        };
        (gap > 0).then_some(gap as f64)
    }

    /// UTC seconds a fractional logical index denotes on a time axis.
    pub(crate) fn drawing_time_at_logical(&self, logical: f64) -> Option<f64> {
        if !logical.is_finite() {
            return None;
        }
        let times = self.data.merged_times();
        let (first, last) = (*times.first()? as f64, *times.last()? as f64);
        let last_index = (times.len() - 1) as f64;
        let time = if logical >= last_index {
            let beyond = logical - last_index;
            if beyond == 0.0 {
                last
            } else {
                beyond.mul_add(self.drawing_edge_step(true)?, last)
            }
        } else if logical <= 0.0 {
            if logical == 0.0 {
                first
            } else {
                logical.mul_add(self.drawing_edge_step(false)?, first)
            }
        } else {
            let index = logical.floor() as usize;
            let (start, end) = (times[index] as f64, times[index + 1] as f64);
            (logical - index as f64).mul_add(end - start, start)
        };
        time.is_finite().then_some(time)
    }

    /// Fractional logical index of a UTC time on a time axis; the inverse of
    /// [`Self::drawing_time_at_logical`] for the installed bars.
    pub(crate) fn drawing_logical_at_time(&self, time: f64) -> Option<f64> {
        if !time.is_finite() {
            return None;
        }
        let times = self.data.merged_times();
        let (first, last) = (*times.first()? as f64, *times.last()? as f64);
        let last_index = (times.len() - 1) as f64;
        let logical = if time >= last {
            if time == last {
                last_index
            } else {
                last_index + (time - last) / self.drawing_edge_step(true)?
            }
        } else if time <= first {
            if time == first {
                0.0
            } else {
                (time - first) / self.drawing_edge_step(false)?
            }
        } else {
            let index = times.partition_point(|&bar| (bar as f64) <= time) - 1;
            let (start, end) = (times[index] as f64, times[index + 1] as f64);
            index as f64 + (time - start) / (end - start)
        };
        logical.is_finite().then_some(logical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: i64 = 60;

    fn chart_with_bar_times(times: &[i64], cadence: Option<i64>) -> ChartEngine {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart.set_future_time_projection(cadence, 512);
        chart.set_past_time_projection(cadence, 512);
        let times = times.iter().map(|&time| time as f64).collect::<Vec<_>>();
        let values = vec![100.0; times.len()];
        chart
            .set_series_data(0, &times, &values, &values, &values, &values)
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        chart.build_frame();
        chart
    }

    fn minute_bars(first: i64, count: i64) -> Vec<i64> {
        (0..count).map(|index| first + index * MINUTE).collect()
    }

    fn point(logical: f64, price: f64) -> DrawingPoint {
        DrawingPoint { logical, price }
    }

    fn stair_path(chart: &mut ChartEngine, logicals: [f64; 5]) -> DrawingId {
        let prices = [100.0, 100.0, 101.0, 101.0, 102.0];
        chart
            .add_drawing(
                DrawingKind::Path,
                0,
                logicals
                    .iter()
                    .zip(prices)
                    .map(|(&logical, price)| point(logical, price))
                    .collect(),
                None,
            )
            .unwrap()
    }

    fn logicals(chart: &ChartEngine, index: usize) -> Vec<f64> {
        chart.drawings[index]
            .points
            .iter()
            .map(|point| point.logical)
            .collect()
    }

    fn assert_logicals(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?} vs {expected:?}");
        for (actual_value, expected_value) in actual.iter().zip(expected) {
            assert!(
                (actual_value - expected_value).abs() < 1e-9,
                "{actual:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn a_stair_path_past_the_newest_bar_keeps_its_shape_when_the_edge_gap_changes() {
        // Saved with an even one-minute edge; restored where the last bar follows a gap
        // (sparse second bars, a session break). The steps must keep their time extent.
        let mut source = chart_with_bar_times(&minute_bars(0, 11), Some(MINUTE));
        stair_path(&mut source, [12.0, 14.0, 14.0, 16.0, 16.0]);
        let document = source.export_drawings_json().unwrap();

        let mut sparse_tail = minute_bars(0, 9);
        sparse_tail.push(10 * MINUTE);
        let mut restored = chart_with_bar_times(&sparse_tail, Some(MINUTE));
        restored.import_drawings_json(&document).unwrap();

        // 10 bars end at 10 minutes, so the anchors at 12, 14 and 16 minutes sit 2, 4 and 6
        // projection steps past the newest bar.
        assert_logicals(&logicals(&restored, 0), &[11.0, 13.0, 13.0, 15.0, 15.0]);
        let prices = restored.drawings[0]
            .points
            .iter()
            .map(|point| point.price)
            .collect::<Vec<_>>();
        assert_eq!(prices, vec![100.0, 100.0, 101.0, 101.0, 102.0]);
    }

    #[test]
    fn anchors_keep_their_time_across_intervals_and_history_windows() {
        let mut minute = chart_with_bar_times(&minute_bars(0, 30), Some(MINUTE));
        minute
            .add_drawing(
                DrawingKind::TrendLine,
                0,
                vec![point(2.5, 100.0), point(33.0, 101.0)],
                None,
            )
            .unwrap();
        minute
            .add_drawing(
                DrawingKind::TrendLine,
                0,
                vec![point(-4.0, 99.0), point(10.0, 100.0)],
                None,
            )
            .unwrap();
        let document = minute.export_drawings_json().unwrap();

        // Five-minute bars covering the same half hour.
        let five_minutes = (0..6).map(|index| index * 5 * MINUTE).collect::<Vec<_>>();
        let mut restored = chart_with_bar_times(&five_minutes, Some(5 * MINUTE));
        restored.import_drawings_json(&document).unwrap();
        assert_logicals(&logicals(&restored, 0), &[0.5, 6.6]);
        assert_logicals(&logicals(&restored, 1), &[-0.8, 2.0]);

        // A later one-minute window: the same times sit 20 bars earlier.
        let mut later = chart_with_bar_times(&minute_bars(20 * MINUTE, 30), Some(MINUTE));
        later.import_drawings_json(&document).unwrap();
        assert_logicals(&logicals(&later, 0), &[-17.5, 13.0]);
        assert_logicals(&logicals(&later, 1), &[-24.0, -10.0]);
    }

    #[test]
    fn a_document_restored_before_bars_arrive_resolves_against_the_first_bars() {
        let mut source = chart_with_bar_times(&minute_bars(0, 11), Some(MINUTE));
        stair_path(&mut source, [4.0, 6.0, 6.0, 12.0, 12.0]);
        let document = source.export_drawings_json().unwrap();

        let mut restored = ChartEngine::new(800.0, 500.0, 1.0);
        restored.set_future_time_projection(Some(MINUTE), 512);
        restored.import_drawings_json(&document).unwrap();
        // Still waiting for bars, the document round-trips the saved times unchanged.
        let waiting: Value =
            serde_json::from_str(&restored.export_drawings_json().unwrap()).unwrap();
        assert_eq!(waiting["drawings"][0]["points"][3]["time"], 720.0);

        let window = minute_bars(3 * MINUTE, 6);
        let times = window.iter().map(|&time| time as f64).collect::<Vec<_>>();
        let values = vec![100.0; times.len()];
        restored
            .set_series_data(0, &times, &values, &values, &values, &values)
            .unwrap();
        assert_logicals(&logicals(&restored, 0), &[1.0, 3.0, 3.0, 9.0, 9.0]);
    }

    #[test]
    fn import_replaces_drawings_as_a_restore_not_an_edit() {
        let mut source = chart_with_bar_times(&minute_bars(0, 11), Some(MINUTE));
        let path = stair_path(&mut source, [1.0, 2.0, 2.0, 3.0, 3.0]);
        source.set_drawing_locked(path, true);
        source
            .add_drawing(
                DrawingKind::BarsPattern,
                0,
                vec![point(2.0, 100.0), point(5.0, 100.0), point(7.0, 100.0)],
                None,
            )
            .unwrap();
        let frozen = source.drawings[1].bars_pattern.clone();
        assert!(!frozen.is_empty());
        let document = source.export_drawings_json().unwrap();

        let mut restored = chart_with_bar_times(&minute_bars(0, 11), Some(MINUTE));
        let existing = stair_path(&mut restored, [5.0, 6.0, 6.0, 7.0, 7.0]);
        let revision = restored.drawing_revision();
        let imported = restored.import_drawings_json(&document).unwrap();

        assert_eq!(imported.len(), 2);
        assert_eq!(imported[0].document_id, Some(path));
        assert!(imported.iter().all(|drawing| drawing.id > existing));
        assert_eq!(restored.drawings.len(), 2);
        assert!(restored.drawings[0].locked);
        assert_eq!(restored.drawings[1].bars_pattern, frozen);
        assert_ne!(restored.drawing_revision(), revision);
        assert!(
            !restored.undo_drawing(),
            "a restore is not an undoable edit"
        );
        assert_eq!(restored.drawings.len(), 2);
    }

    #[test]
    fn an_invalid_document_leaves_the_drawings_untouched() {
        let mut chart = chart_with_bar_times(&minute_bars(0, 11), Some(MINUTE));
        stair_path(&mut chart, [1.0, 2.0, 2.0, 3.0, 3.0]);
        let mut document: Value =
            serde_json::from_str(&chart.export_drawings_json().unwrap()).unwrap();
        let revision = chart.drawing_revision();

        document["drawings"][0]["pane_index"] = Value::from(3);
        let error = chart
            .import_drawings_json(&document.to_string())
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidData);

        document["schema_version"] = Value::from(2);
        let error = chart
            .import_drawings_json(&document.to_string())
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::PersistenceVersionError);

        let error = chart.import_drawings_json("[]").unwrap_err();
        assert_eq!(error.code, ErrorCode::SerializationError);

        assert_eq!(chart.drawings.len(), 1);
        assert_eq!(chart.drawing_revision(), revision);
    }
}
