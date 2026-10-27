//! Chart-independent annotation output bounded by retained source rows.

use std::collections::VecDeque;

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
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct StudyMarker {
    pub row: usize,
    pub confirm_row: usize,
    pub price: f64,
    pub kind: StudyMarkerKind,
}

/// Confirmation-ordered history. Swing points emit at most two markers per confirmation row.
/// History is removed only when its source rows are removed.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct StudyAnnotations {
    markers: VecDeque<StudyMarker>,
    #[serde(skip)]
    marker_index: RowIntervalIndex,
}

/// Confirmation-order segment tree. Each node bounds the rows in its subtree;
/// a viewport query visits only branches whose intervals can intersect it.
#[derive(Clone, Debug, Default)]
struct RowIntervalIndex {
    tree: Vec<(usize, usize)>,
    base: usize,
    len: usize,
    #[cfg(test)]
    repair_nodes: usize,
}

impl RowIntervalIndex {
    const EMPTY: (usize, usize) = (usize::MAX, 0);

    fn merge(a: (usize, usize), b: (usize, usize)) -> (usize, usize) {
        (a.0.min(b.0), a.1.max(b.1))
    }

    fn set(&mut self, index: usize, value: (usize, usize)) {
        if index >= self.base {
            let old = std::mem::take(&mut self.tree);
            let old_base = self.base;
            self.base = (index + 1).next_power_of_two();
            self.tree = vec![Self::EMPTY; self.base * 2];
            if old_base != 0 {
                self.tree[self.base..self.base + self.len]
                    .copy_from_slice(&old[old_base..old_base + self.len]);
            }
            for node in (1..self.base).rev() {
                self.tree[node] = Self::merge(self.tree[2 * node], self.tree[2 * node + 1]);
            }
        }
        self.len = self.len.max(index + 1);
        let mut node = self.base + index;
        self.tree[node] = value;
        #[cfg(test)]
        {
            self.repair_nodes += 1;
        }
        while node > 1 {
            node /= 2;
            self.tree[node] = Self::merge(self.tree[2 * node], self.tree[2 * node + 1]);
            #[cfg(test)]
            {
                self.repair_nodes += 1;
            }
        }
    }

    /// Clear discarded leaves and recompute only their ancestors. The tree's
    /// base and allocation survive suffix repair; each level covers at most
    /// half as many nodes as the one below it.
    fn truncate(&mut self, len: usize) {
        assert!(len <= self.len);
        if len == self.len {
            return;
        }
        let (mut first, mut end) = (self.base + len, self.base + self.len);
        self.tree[first..end].fill(Self::EMPTY);
        #[cfg(test)]
        {
            self.repair_nodes += end - first;
        }
        self.len = len;
        while first > 1 {
            first /= 2;
            end = end.div_ceil(2);
            for node in first..end {
                self.tree[node] = Self::merge(self.tree[2 * node], self.tree[2 * node + 1]);
                #[cfg(test)]
                {
                    self.repair_nodes += 1;
                }
            }
        }
    }

    fn visit(&self, from: usize, to: usize, mut visit: impl FnMut(usize)) {
        if self.len == 0 || from >= to {
            return;
        }
        self.visit_node(1, 0, self.base, from, to, &mut visit);
    }

    fn visit_node(
        &self,
        node: usize,
        start: usize,
        end: usize,
        from: usize,
        to: usize,
        visit: &mut impl FnMut(usize),
    ) {
        let (min_start, max_end) = self.tree[node];
        if min_start >= to || max_end < from || min_start == usize::MAX {
            return;
        }
        if end - start == 1 {
            if start < self.len {
                visit(start);
            }
            return;
        }
        let mid = start + (end - start) / 2;
        self.visit_node(node * 2, start, mid, from, to, visit);
        self.visit_node(node * 2 + 1, mid, end, from, to, visit);
    }

    fn capacity_bytes(&self) -> usize {
        self.tree.capacity() * std::mem::size_of::<(usize, usize)>()
    }
}

// Query accelerators are derived storage, not observable annotation state. In
// particular, append-then-repair may leave a larger tree than a batch build.
impl PartialEq for StudyAnnotations {
    fn eq(&self, other: &Self) -> bool {
        self.markers == other.markers
    }
}

impl StudyAnnotations {
    pub fn markers(&self) -> &VecDeque<StudyMarker> {
        &self.markers
    }

    pub fn capacity_bytes(&self) -> usize {
        self.markers.capacity() * std::mem::size_of::<StudyMarker>()
            + self.marker_index.capacity_bytes()
    }

    pub fn push_marker(&mut self, marker: StudyMarker) {
        assert!(
            self.markers
                .back()
                .is_none_or(|last| last.confirm_row <= marker.confirm_row),
            "study markers must be appended in confirmation order"
        );
        self.markers.push_back(marker);
        let index = self.markers.len() - 1;
        self.marker_index.set(index, (marker.row, marker.row));
    }

    /// Discard confirmations in the repaired suffix.
    ///
    /// Confirmation order makes the retained prefix searchable without scanning the history.
    pub fn rebuild_from(&mut self, from: usize) {
        let (mut lo, mut hi) = (0, self.markers.len());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.markers[mid].confirm_row < from {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        self.marker_index.truncate(lo);
        self.markers.truncate(lo);
    }

    /// Source-row viewport `[from, to)`, in confirmation paint order.
    pub fn visit_visible_markers(
        &self,
        from: usize,
        to: usize,
        mut visit: impl FnMut(&StudyMarker),
    ) {
        self.marker_index
            .visit(from, to, |i| visit(&self.markers[i]));
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
        }
    }

    #[test]
    fn long_history_retains_every_annotation_until_source_rebuild() {
        let mut annotations = StudyAnnotations::default();
        for row in 0..10_000 {
            annotations.push_marker(marker(row));
        }
        assert_eq!(annotations.markers().len(), 10_000);
        assert!(annotations.capacity_bytes() >= 10_000 * std::mem::size_of::<StudyMarker>());
        annotations.rebuild_from(5_000);
        assert_eq!(annotations.markers().len(), 5_000);
        assert_eq!(annotations.marker_index.len, 5_000);
    }

    #[test]
    fn repair_truncates_confirmations() {
        let mut annotations = StudyAnnotations::default();
        for row in [2, 4, 4, 6] {
            annotations.push_marker(marker(row));
        }
        annotations.rebuild_from(5);
        assert_eq!(
            annotations
                .markers()
                .iter()
                .map(|m| m.confirm_row)
                .collect::<Vec<_>>(),
            [2, 4, 4]
        );
        annotations.rebuild_from(4);
        assert_eq!(annotations.markers().len(), 1);
    }

    #[test]
    fn long_history_frame_query_visits_only_intersecting_rows() {
        let mut annotations = StudyAnnotations::default();
        for row in 0..100_000 {
            annotations.push_marker(marker(row));
        }
        let mut marker_rows = Vec::new();
        annotations.visit_visible_markers(80_000, 80_010, |m| marker_rows.push(m.confirm_row));
        assert_eq!(marker_rows, (80_002..80_012).collect::<Vec<_>>());
        assert!(annotations.capacity_bytes() > 100_000 * std::mem::size_of::<StudyMarker>());

        annotations.rebuild_from(80_005);
        marker_rows.clear();
        annotations.visit_visible_markers(80_000, 80_010, |m| marker_rows.push(m.confirm_row));
        assert_eq!(marker_rows, (80_002..80_005).collect::<Vec<_>>());
    }

    #[test]
    fn repair_and_batch_history_compare_equal_despite_reused_index_capacity() {
        let mut batch = StudyAnnotations::default();
        let mut repaired = StudyAnnotations::default();
        for row in 0..40 {
            if row < 20 {
                batch.push_marker(marker(row));
            }
            repaired.push_marker(marker(row));
        }
        repaired.rebuild_from(20);
        assert_eq!(batch, repaired);
        for from in [0, 5, 19, 20, 30] {
            let visible = |annotations: &StudyAnnotations| {
                let mut markers = Vec::new();
                annotations.visit_visible_markers(from, from + 2, |m| markers.push(m.confirm_row));
                markers
            };
            assert_eq!(visible(&batch), visible(&repaired));
        }
    }

    #[test]
    fn long_history_tip_repair_reuses_the_tree_with_bounded_index_work() {
        let mut annotations = StudyAnnotations::default();
        for row in 0..100_000 {
            annotations.push_marker(marker(row));
        }
        let marker_tree = annotations.marker_index.tree.as_ptr();
        let marker_capacity = annotations.marker_index.tree.capacity();
        for _ in 0..8 {
            annotations.marker_index.repair_nodes = 0;
            annotations.rebuild_from(99_999);
            annotations.push_marker(marker(99_999));
            assert_eq!(annotations.marker_index.tree.as_ptr(), marker_tree);
            assert_eq!(annotations.marker_index.tree.capacity(), marker_capacity);
            assert!(annotations.marker_index.repair_nodes < 128);
        }
    }

    #[test]
    #[should_panic(expected = "study markers must be appended in confirmation order")]
    fn marker_out_of_order_is_rejected() {
        let mut annotations = StudyAnnotations::default();
        annotations.push_marker(marker(2));
        annotations.push_marker(marker(1));
    }
}
