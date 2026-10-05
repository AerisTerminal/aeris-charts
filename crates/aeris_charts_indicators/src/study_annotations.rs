//! Bounded, chart-independent annotation output for structural studies.

use std::collections::VecDeque;

pub const MAX_STUDY_MARKERS: usize = 4_096;
pub const MAX_STUDY_ZONES: usize = 4_096;
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
}

/// Annotations retained in confirmation order, with independently bounded histories.
///
/// A full history evicts the oldest annotation before append. Active zones have a separate
/// per-side cap; when exceeded, the oldest still-active zone on that side is discarded.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct StudyAnnotations {
    markers: VecDeque<StudyMarker>,
    zones: VecDeque<StudyZone>,
    #[serde(skip)]
    active_zones: [usize; 2],
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

    pub fn push_marker(&mut self, marker: StudyMarker) {
        assert!(
            self.markers
                .back()
                .is_none_or(|last| last.confirm_row <= marker.confirm_row),
            "study markers must be appended in confirmation order"
        );
        if self.markers.len() == MAX_STUDY_MARKERS {
            self.markers.pop_front();
        }
        self.markers.push_back(marker);
    }

    pub fn push_zone(&mut self, zone: StudyZone) {
        assert!(
            self.zones
                .back()
                .is_none_or(|last| last.confirm_row <= zone.confirm_row),
            "study zones must be appended in confirmation order"
        );
        let side = Self::side(zone.bullish);
        if zone.end_row.is_none() && self.active_zones[side] == MAX_ACTIVE_ZONES_PER_SIDE {
            let oldest = self
                .zones
                .iter()
                .position(|prior| prior.bullish == zone.bullish && prior.end_row.is_none())
                .expect("active side has an oldest zone");
            self.zones.remove(oldest);
            self.active_zones[side] -= 1;
        }
        if self.zones.len() == MAX_STUDY_ZONES
            && let Some(evicted) = self.zones.pop_front()
            && evicted.end_row.is_none()
        {
            self.active_zones[Self::side(evicted.bullish)] -= 1;
        }
        if zone.end_row.is_none() {
            self.active_zones[side] += 1;
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
        self.active_zones[Self::side(zone.bullish)] -= 1;
        zone.end_row = Some(end_row);
        true
    }

    /// Discard confirmations in the repaired suffix and undo its mitigations.
    ///
    /// Confirmation order makes the retained prefix searchable without scanning the history.
    pub fn rebuild_from(&mut self, from: usize) {
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
        self.active_zones = [0; 2];
        for zone in &mut self.zones {
            if zone.end_row.is_some_and(|end| end >= from) {
                zone.end_row = None;
            }
            if zone.end_row.is_none() {
                self.active_zones[Self::side(zone.bullish)] += 1;
            }
        }
        // Repair can reactivate previously mitigated zones. Keep the newest 64 per side.
        for bullish in [true, false] {
            let side = Self::side(bullish);
            while self.active_zones[side] > MAX_ACTIVE_ZONES_PER_SIDE {
                let oldest = self
                    .zones
                    .iter()
                    .position(|z| z.bullish == bullish && z.end_row.is_none())
                    .expect("active side has an oldest zone");
                self.zones.remove(oldest);
                self.active_zones[side] -= 1;
            }
        }
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
        }
    }

    #[test]
    fn marker_cap_evicts_oldest_and_allows_equal_confirmation_rows() {
        let mut annotations = StudyAnnotations::default();
        for row in 0..=MAX_STUDY_MARKERS {
            annotations.push_marker(marker(row));
        }
        annotations.push_marker(marker(MAX_STUDY_MARKERS));
        assert_eq!(annotations.markers().len(), MAX_STUDY_MARKERS);
        assert_eq!(annotations.markers()[0].confirm_row, 2);
        assert_eq!(
            annotations.markers().back().unwrap().confirm_row,
            MAX_STUDY_MARKERS
        );
    }

    #[test]
    fn zone_history_and_active_caps_are_independent() {
        let mut annotations = StudyAnnotations::default();
        for row in 0..=MAX_STUDY_ZONES {
            let mut finished = zone(row, row % 2 == 0);
            finished.end_row = Some(row);
            annotations.push_zone(finished);
        }
        assert_eq!(annotations.zones().len(), MAX_STUDY_ZONES);
        assert_eq!(annotations.zones()[0].confirm_row, 1);

        for row in 0..MAX_ACTIVE_ZONES_PER_SIDE {
            annotations.push_zone(zone(MAX_STUDY_ZONES + 1 + row, true));
            annotations.push_zone(zone(MAX_STUDY_ZONES + 1 + row, false));
        }
        let first_bull = MAX_STUDY_ZONES + 1;
        annotations.push_zone(zone(MAX_STUDY_ZONES + 1 + MAX_ACTIVE_ZONES_PER_SIDE, true));
        assert!(
            !annotations
                .zones()
                .iter()
                .any(|z| z.confirm_row == first_bull && z.bullish)
        );
        assert_eq!(
            annotations
                .zones()
                .iter()
                .filter(|z| z.bullish && z.end_row.is_none())
                .count(),
            MAX_ACTIVE_ZONES_PER_SIDE
        );
        assert_eq!(
            annotations
                .zones()
                .iter()
                .filter(|z| !z.bullish && z.end_row.is_none())
                .count(),
            MAX_ACTIVE_ZONES_PER_SIDE
        );
        assert_eq!(annotations.zones().len(), MAX_STUDY_ZONES);
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
        for row in 0..=MAX_ACTIVE_ZONES_PER_SIDE {
            annotations.push_zone(zone(row, true));
            assert!(annotations.end_zone(row, 100));
        }
        annotations.rebuild_from(100);
        assert_eq!(annotations.zones().len(), MAX_ACTIVE_ZONES_PER_SIDE);
        assert_eq!(annotations.zones()[0].confirm_row, 1);
        assert!(annotations.zones().iter().all(|z| z.end_row.is_none()));
        assert!(annotations.end_zone(0, 101));
        annotations.push_zone(zone(101, true));
        assert_eq!(annotations.zones().len(), MAX_ACTIVE_ZONES_PER_SIDE + 1);
    }

    #[test]
    fn history_eviction_releases_an_active_slot() {
        let mut annotations = StudyAnnotations::default();
        annotations.push_zone(zone(0, true));
        for row in 1..MAX_STUDY_ZONES {
            let mut finished = zone(row, false);
            finished.end_row = Some(row);
            annotations.push_zone(finished);
        }
        annotations.push_zone(zone(MAX_STUDY_ZONES, false));
        assert_eq!(annotations.zones().len(), MAX_STUDY_ZONES);
        for row in 1..=MAX_ACTIVE_ZONES_PER_SIDE {
            annotations.push_zone(zone(MAX_STUDY_ZONES + row, true));
        }
        assert_eq!(
            annotations
                .zones()
                .iter()
                .filter(|z| z.bullish && z.end_row.is_none())
                .count(),
            MAX_ACTIVE_ZONES_PER_SIDE
        );
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
