//! Chart-independent annotation output bounded by retained source rows.

use std::collections::VecDeque;

pub const MAX_ACTIVE_ZONES_PER_SIDE: usize = 64;

/// Host-supplied UTC interval: inclusive start, exclusive end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionSpan {
    pub start: i64,
    pub end: i64,
    pub session_id: u64,
}

impl SessionSpan {
    /// UTC civil-day ordinal of the last included second, never a host-local date.
    pub fn trading_day(self) -> i64 {
        (self.end - 1).div_euclid(86_400)
    }
}

#[cfg(test)]
mod session_tests {
    use super::SessionSpan;

    #[test]
    fn host_trading_date_uses_the_last_included_utc_second() {
        let span = SessionSpan {
            start: 86_399,
            end: 86_401,
            session_id: 7,
        };
        assert_eq!(span.trading_day(), 1);
        let ending_at_midnight = SessionSpan {
            end: 86_400,
            ..span
        };
        assert_eq!(ending_at_midnight.trading_day(), 0);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StudyMarkerKind {
    SwingHigh,
    SwingLow,
    Bos { up: bool },
    Choch { up: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct StudyMarker {
    pub row: usize,
    pub confirm_row: usize,
    pub price: f64,
    pub kind: StudyMarkerKind,
    /// Pivot row for a BOS/CHoCH segment.
    pub from_row: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct StudyZone {
    pub start_row: usize,
    pub confirm_row: usize,
    pub top: f64,
    pub bottom: f64,
    pub bullish: bool,
    /// First row that mitigates the zone, if any.
    pub end_row: Option<usize>,
    /// Retired by the active-zone limit, rather than mitigated by price.
    pub retired: bool,
}

/// Confirmation-ordered history. Swing and structure emit at most two markers per
/// confirmation row; FVG and order blocks emit at most two zones per row.
/// History is removed only when its source rows are removed.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct StudyAnnotations {
    markers: VecDeque<StudyMarker>,
    zones: VecDeque<StudyZone>,
    #[serde(skip)]
    active_zones: [VecDeque<usize>; 2],
    #[serde(skip)]
    ends: Vec<(usize, usize)>,
}

impl StudyAnnotations {
    fn side(bullish: bool) -> usize {
        usize::from(bullish)
    }

    pub fn markers(&self) -> &VecDeque<StudyMarker> {
        &self.markers
    }

    pub fn zones(&self) -> &VecDeque<StudyZone> {
        &self.zones
    }

    pub fn capacity_bytes(&self) -> usize {
        self.markers.capacity() * std::mem::size_of::<StudyMarker>()
            + self.zones.capacity() * std::mem::size_of::<StudyZone>()
            + self.ends.capacity() * std::mem::size_of::<(usize, usize)>()
            + self
                .active_zones
                .iter()
                .map(|side| side.capacity() * std::mem::size_of::<usize>())
                .sum::<usize>()
    }

    pub(crate) fn active_snapshot(&self) -> [VecDeque<usize>; 2] {
        self.active_zones.clone()
    }

    pub fn push_marker(&mut self, marker: StudyMarker) {
        assert!(
            self.markers
                .back()
                .is_none_or(|last| last.confirm_row <= marker.confirm_row),
            "study markers must be appended in confirmation order"
        );
        self.markers.push_back(marker);
    }

    pub fn push_zone(&mut self, zone: StudyZone) {
        self.push_zone_with_cap(zone, MAX_ACTIVE_ZONES_PER_SIDE);
    }

    pub fn push_zone_with_cap(&mut self, zone: StudyZone, cap: usize) {
        assert!((1..=MAX_ACTIVE_ZONES_PER_SIDE).contains(&cap));
        assert!(
            self.zones
                .back()
                .is_none_or(|last| last.confirm_row <= zone.confirm_row),
            "study zones must be appended in confirmation order"
        );
        let side = Self::side(zone.bullish);
        if zone.end_row.is_none() {
            if self.active_zones[side].len() == cap {
                let oldest = self.active_zones[side].pop_front().expect("active zone");
                self.zones[oldest].end_row = Some(zone.confirm_row);
                self.zones[oldest].retired = true;
                self.ends.push((zone.confirm_row, oldest));
            }
            self.active_zones[side].push_back(self.zones.len());
        }
        self.zones.push_back(zone);
    }

    /// Record mitigation for a retained zone by its current deque index.
    /// Returns false for an unknown index or a mitigation preceding confirmation.
    pub fn end_zone(&mut self, index: usize, end_row: usize) -> bool {
        let Some(zone) = self.zones.get_mut(index) else {
            return false;
        };
        if end_row < zone.confirm_row || zone.end_row.is_some() {
            return false;
        }
        self.active_zones[Self::side(zone.bullish)].retain(|&active| active != index);
        zone.end_row = Some(end_row);
        self.ends.push((end_row, index));
        true
    }

    /// Discard annotations anchored on evicted source rows, preserving all other history.
    pub fn drop_before(&mut self, first_row: usize) {
        self.markers.retain(|m| m.row >= first_row);
        let mut mapping = vec![None; self.zones.len()];
        let mut next = 0;
        for (i, zone) in self.zones.iter().enumerate() {
            if zone.start_row >= first_row {
                mapping[i] = Some(next);
                next += 1;
            }
        }
        self.zones.retain(|z| z.start_row >= first_row);
        for side in &mut self.active_zones {
            *side = side.iter().filter_map(|&i| mapping[i]).collect();
        }
        self.ends = self
            .ends
            .iter()
            .filter_map(|&(end, i)| mapping[i].map(|i| (end, i)))
            .collect();
        self.markers.shrink_to_fit();
        self.zones.shrink_to_fit();
    }

    /// Discard confirmations in the repaired suffix and undo its mitigations.
    ///
    /// Confirmation order makes the retained prefix searchable without scanning the history.
    pub fn rebuild_from(&mut self, from: usize) {
        self.rebuild_from_snapshot(from, None);
    }

    /// Restore a small checkpoint's active indices without inspecting old history.
    pub(crate) fn rebuild_from_snapshot(
        &mut self,
        from: usize,
        active: Option<[VecDeque<usize>; 2]>,
    ) {
        fn first_at_or_after<T>(
            items: &VecDeque<T>,
            from: usize,
            row: impl Fn(&T) -> usize,
        ) -> usize {
            let (mut lo, mut hi) = (0, items.len());
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if row(&items[mid]) < from {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            lo
        }
        let marker_end = first_at_or_after(&self.markers, from, |m| m.confirm_row);
        self.markers.truncate(marker_end);
        let zone_end = first_at_or_after(&self.zones, from, |z| z.confirm_row);
        self.zones.truncate(zone_end);
        while self.ends.last().is_some_and(|&(end, _)| end >= from) {
            let (_, index) = self.ends.pop().expect("last end");
            if index < self.zones.len() {
                self.zones[index].end_row = None;
                self.zones[index].retired = false;
            }
        }
        self.active_zones = active.unwrap_or_else(|| {
            let mut sides = [VecDeque::new(), VecDeque::new()];
            for (i, z) in self.zones.iter().enumerate() {
                if z.end_row.is_none() {
                    sides[Self::side(z.bullish)].push_back(i);
                }
            }
            sides
        });
    }

    pub(crate) fn active_indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.active_zones
            .iter()
            .flat_map(|side| side.iter().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(confirm_row: usize) -> StudyMarker {
        StudyMarker {
            row: confirm_row.saturating_sub(2),
            confirm_row,
            price: 100.0,
            kind: StudyMarkerKind::SwingHigh,
            from_row: None,
        }
    }

    fn zone(confirm_row: usize, bullish: bool) -> StudyZone {
        StudyZone {
            start_row: confirm_row.saturating_sub(1),
            confirm_row,
            top: 101.0,
            bottom: 100.0,
            bullish,
            end_row: None,
            retired: false,
        }
    }

    #[test]
    fn long_history_retains_every_annotation_and_drops_only_evicted_source_rows() {
        let mut annotations = StudyAnnotations::default();
        for row in 0..10_000 {
            annotations.push_marker(marker(row));
            let mut finished = zone(row, row % 2 == 0);
            finished.end_row = Some(row);
            annotations.push_zone(finished);
        }
        assert_eq!(annotations.markers().len(), 10_000);
        assert_eq!(annotations.zones().len(), 10_000);
        let bytes = annotations.capacity_bytes();
        assert!(
            bytes
                >= 10_000 * (std::mem::size_of::<StudyMarker>() + std::mem::size_of::<StudyZone>())
        );
        annotations.drop_before(5_000);
        assert!(annotations.capacity_bytes() < bytes);
        assert_eq!(annotations.markers().front().unwrap().row, 5_000);
        assert_eq!(annotations.zones().front().unwrap().start_row, 5_000);
        assert_eq!(annotations.markers().len(), 4_998);
        assert_eq!(annotations.zones().len(), 4_999);
    }

    #[test]
    fn over_cap_retires_oldest_without_deleting_history() {
        let mut annotations = StudyAnnotations::default();
        for row in 0..10_000 {
            annotations.push_zone_with_cap(zone(row, true), 1);
        }
        assert_eq!(annotations.zones().len(), 10_000);
        assert_eq!(annotations.zones()[0].end_row, Some(1));
        assert!(annotations.zones()[0].retired);
        assert_eq!(annotations.active_indices().count(), 1);
        annotations.rebuild_from(9_999);
        assert_eq!(annotations.zones().len(), 9_999);
        assert!(!annotations.zones()[9_998].retired);
        assert_eq!(annotations.active_indices().count(), 1);
    }

    #[test]
    fn repair_truncates_confirmations_and_clears_suffix_mitigation() {
        let mut annotations = StudyAnnotations::default();
        for row in [2, 4, 4, 6] {
            annotations.push_marker(marker(row));
            annotations.push_zone(zone(row, true));
        }
        assert!(!annotations.end_zone(0, 1));
        assert!(annotations.end_zone(0, 3));
        assert!(!annotations.end_zone(0, 5));
        assert!(annotations.end_zone(1, 5));
        assert!(!annotations.end_zone(4, 7));
        annotations.rebuild_from(5);
        assert_eq!(
            annotations
                .markers()
                .iter()
                .map(|m| m.confirm_row)
                .collect::<Vec<_>>(),
            [2, 4, 4]
        );
        assert_eq!(annotations.zones().len(), 3);
        assert_eq!(annotations.zones()[0].end_row, Some(3));
        assert_eq!(annotations.zones()[1].end_row, None);
        annotations.rebuild_from(4);
        assert_eq!(annotations.markers().len(), 1);
        assert_eq!(annotations.zones().len(), 1);
    }

    #[test]
    fn repair_reactivation_preserves_per_side_cap() {
        let mut annotations = StudyAnnotations::default();
        for row in 0..MAX_ACTIVE_ZONES_PER_SIDE {
            annotations.push_zone(zone(row, true));
            assert!(annotations.end_zone(row, 100));
        }
        annotations.rebuild_from(100);
        assert_eq!(annotations.zones().len(), MAX_ACTIVE_ZONES_PER_SIDE);
        assert_eq!(annotations.zones()[0].confirm_row, 0);
        assert!(annotations.zones().iter().all(|z| z.end_row.is_none()));
        annotations.push_zone(zone(101, true));
        assert_eq!(annotations.zones().len(), MAX_ACTIVE_ZONES_PER_SIDE + 1);
        assert!(annotations.zones()[0].retired);
    }

    #[test]
    #[should_panic(expected = "study markers must be appended in confirmation order")]
    fn marker_out_of_order_is_rejected() {
        let mut annotations = StudyAnnotations::default();
        annotations.push_marker(marker(2));
        annotations.push_marker(marker(1));
    }

    #[test]
    #[should_panic(expected = "study zones must be appended in confirmation order")]
    fn zone_out_of_order_is_rejected() {
        let mut annotations = StudyAnnotations::default();
        annotations.push_zone(zone(2, true));
        annotations.push_zone(zone(1, false));
    }
}
