//! Multi-chart workspace: the split-grid MODEL behind the platform's chart-splitting feature.
//!
//! A workspace is a binary tree of cells. Every leaf cell is an independent chart (its own
//! engine instance, scales, series, chart type, and drawing primitives on the host side) —
//! splitting never shares or mirrors state between cells. A cell splits horizontally (side by
//! side) or vertically (stacked), recursively and without a built-in cap; removing a cell
//! collapses its split node so the sibling subtree absorbs the freed space.
//!
//! Paywall metering is first-class: the workspace tracks the live chart count, the cumulative
//! split count, and per-cell ages (host-injected clock), exposes a usage snapshot, a hard
//! `max_charts` cap, and a split veto error so the platform enforces its own limits. Layout is
//! engine-owned but rendering-agnostic: hosts read the tree (see [`Workspace::layout_json`])
//! and place each cell's chart themselves.

use std::collections::BTreeMap;

/// Split orientation: `Horizontal` places the two charts side by side, `Vertical` stacks them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// Immutable snapshot of a workspace tree: either a chart cell or a split of two subtrees.
/// The split's `ratio` is the `a` subtree's share of the space (0..1, default 0.5); dragging
/// the divider between two adjacent cells adjusts it via [`Workspace::resize_between`].
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum WorkspaceLayout {
    /// A leaf containing one chart cell.
    Cell {
        /// Stable workspace cell identifier.
        id: u64,
    },
    /// A directional split containing two child layouts.
    Split {
        /// Direction in which the child layouts are arranged.
        direction: SplitDirection,
        /// Fraction of the available space assigned to `a`.
        ratio: f64,
        /// First child subtree.
        a: Box<WorkspaceLayout>,
        /// Second child subtree.
        b: Box<WorkspaceLayout>,
    },
}

impl WorkspaceLayout {
    fn cell_ids(&self, out: &mut Vec<u64>) {
        match self {
            WorkspaceLayout::Cell { id } => out.push(*id),
            WorkspaceLayout::Split { a, b, .. } => {
                a.cell_ids(out);
                b.cell_ids(out);
            }
        }
    }

    fn first_leaf(&self) -> u64 {
        match self {
            WorkspaceLayout::Cell { id } => *id,
            WorkspaceLayout::Split { a, .. } => a.first_leaf(),
        }
    }

    fn last_leaf(&self) -> u64 {
        match self {
            WorkspaceLayout::Cell { id } => *id,
            WorkspaceLayout::Split { b, .. } => b.last_leaf(),
        }
    }
}

/// Why a split/remove was rejected (the host maps these to its own UI affordances).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceError {
    /// The `max_charts` cap rejects the split (paywall tier reached).
    AtCapacity,
    /// Unknown cell id.
    NotFound,
    /// The last remaining cell cannot be removed.
    LastCell,
}

/// Usage/metering snapshot (the paywall signal): live chart count, cumulative splits, and
/// per-cell ages in seconds against the host clock passed to [`Workspace::usage`].
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct WorkspaceUsage {
    pub chart_count: usize,
    pub split_count: u64,
    pub elapsed_seconds: f64,
    pub cells: Vec<CellUsage>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct CellUsage {
    pub id: u64,
    pub age_seconds: f64,
}

/// The split-grid model: an authoritative binary tree plus metering state. Times are
/// host-injected seconds (the engine stays headless/clock-free).
pub struct Workspace {
    root: WorkspaceLayout,
    created_at: BTreeMap<u64, f64>,
    next_id: u64,
    split_count: u64,
    max_charts: Option<usize>,
    started_at: f64,
}

impl Workspace {
    /// A workspace holding a single chart cell.
    pub fn new(now: f64) -> Self {
        Self {
            root: WorkspaceLayout::Cell { id: 1 },
            created_at: BTreeMap::from([(1, now)]),
            next_id: 2,
            split_count: 0,
            max_charts: None,
            started_at: now,
        }
    }

    /// Leaf cell ids in layout order (left-to-right, top-to-bottom).
    pub fn cell_ids(&self) -> Vec<u64> {
        let mut out = Vec::new();
        self.root.cell_ids(&mut out);
        out
    }

    pub fn chart_count(&self) -> usize {
        self.created_at.len()
    }

    /// Hard cap on live charts (`None` = unlimited).
    pub fn set_max_charts(&mut self, max: Option<usize>) {
        self.max_charts = max;
    }

    /// Split a cell in two; the existing chart keeps its state in the first half and the new
    /// cell (returned id) fills the second. `AtCapacity` when the cap rejects the split,
    /// `NotFound` for an unknown cell.
    pub fn split(
        &mut self,
        id: u64,
        direction: SplitDirection,
        now: f64,
    ) -> Result<u64, WorkspaceError> {
        if let Some(max) = self.max_charts {
            if self.chart_count() + 1 > max {
                return Err(WorkspaceError::AtCapacity);
            }
        }
        let new_id = self.next_id;
        if !split_node(&mut self.root, id, direction, new_id) {
            return Err(WorkspaceError::NotFound);
        }
        self.next_id += 1;
        self.created_at.insert(new_id, now);
        self.split_count += 1;
        Ok(new_id)
    }

    /// Remove a cell: its sibling subtree absorbs the freed space. `LastCell` refuses to
    /// remove the final chart.
    pub fn remove(&mut self, id: u64) -> Result<(), WorkspaceError> {
        if !self.created_at.contains_key(&id) {
            return Err(WorkspaceError::NotFound);
        }
        if self.chart_count() <= 1 {
            return Err(WorkspaceError::LastCell);
        }
        if !remove_node(&mut self.root, id) {
            return Err(WorkspaceError::NotFound);
        }
        self.created_at.remove(&id);
        Ok(())
    }

    /// Drag the divider between two adjacent cells: adjusts the ratio of the split node whose
    /// `a` subtree ends at `left_id` and whose `b` subtree starts at `right_id` by
    /// `delta_ratio` (clamped so neither side collapses under ~5%). `NotFound` when no such
    /// divider exists.
    pub fn resize_between(
        &mut self,
        left_id: u64,
        right_id: u64,
        delta_ratio: f64,
    ) -> Result<(), WorkspaceError> {
        if resize_between_node(&mut self.root, left_id, right_id, delta_ratio) {
            Ok(())
        } else {
            Err(WorkspaceError::NotFound)
        }
    }

    /// The metering snapshot at host time `now` (seconds).
    pub fn usage(&self, now: f64) -> WorkspaceUsage {
        WorkspaceUsage {
            chart_count: self.chart_count(),
            split_count: self.split_count,
            elapsed_seconds: (now - self.started_at).max(0.0),
            cells: self
                .cell_ids()
                .into_iter()
                .map(|id| CellUsage {
                    id,
                    age_seconds: (now - self.created_at[&id]).max(0.0),
                })
                .collect(),
        }
    }

    /// A cloned typed snapshot of the current workspace layout.
    pub fn layout(&self) -> WorkspaceLayout {
        self.root.clone()
    }

    /// The layout tree as JSON (cells by id, splits with direction) for host DOM placement.
    /// This preserves the serialized shape of [`Workspace::layout`].
    pub fn layout_json(&self) -> String {
        serde_json::to_string(&self.root).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Replace leaf `id` with a split of `[id, new_id]`; returns whether the leaf existed.
fn split_node(node: &mut WorkspaceLayout, id: u64, direction: SplitDirection, new_id: u64) -> bool {
    match node {
        WorkspaceLayout::Cell { id: cell } if *cell == id => {
            *node = WorkspaceLayout::Split {
                direction,
                ratio: 0.5,
                a: Box::new(WorkspaceLayout::Cell { id }),
                b: Box::new(WorkspaceLayout::Cell { id: new_id }),
            };
            true
        }
        WorkspaceLayout::Cell { .. } => false,
        WorkspaceLayout::Split { a, b, .. } => {
            split_node(a, id, direction, new_id) || split_node(b, id, direction, new_id)
        }
    }
}

/// Adjust the ratio of the split node whose `a` subtree's last leaf is `left_id` and whose
/// `b` subtree's first leaf is `right_id`; clamped to [0.05, 0.95] so neither side collapses.
fn resize_between_node(
    node: &mut WorkspaceLayout,
    left_id: u64,
    right_id: u64,
    delta_ratio: f64,
) -> bool {
    let WorkspaceLayout::Split { a, b, ratio, .. } = node else {
        return false;
    };
    if a.last_leaf() == left_id && b.first_leaf() == right_id {
        *ratio = (*ratio + delta_ratio).clamp(0.05, 0.95);
        return true;
    }
    resize_between_node(a, left_id, right_id, delta_ratio)
        || resize_between_node(b, left_id, right_id, delta_ratio)
}

/// Remove leaf `id`, collapsing its parent split in place (the sibling subtree absorbs the
/// freed slot — at the root this replaces the whole tree with the sibling). Returns whether
/// the leaf existed.
fn remove_node(node: &mut WorkspaceLayout, id: u64) -> bool {
    let WorkspaceLayout::Split { a, b, .. } = node else {
        return false;
    };
    if matches!(**a, WorkspaceLayout::Cell { id: cell } if cell == id) {
        // The split node collapses: the b subtree absorbs the freed slot.
        let absorbed = std::mem::replace(b, Box::new(WorkspaceLayout::Cell { id: u64::MAX }));
        *node = *absorbed;
        return true;
    }
    if matches!(**b, WorkspaceLayout::Cell { id: cell } if cell == id) {
        let absorbed = std::mem::replace(a, Box::new(WorkspaceLayout::Cell { id: u64::MAX }));
        *node = *absorbed;
        return true;
    }
    remove_node(a, id) || remove_node(b, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_grows_the_tree_in_layout_order() {
        let mut ws = Workspace::new(100.0);
        assert_eq!(ws.cell_ids(), [1]);
        let second = ws.split(1, SplitDirection::Horizontal, 110.0).unwrap();
        assert_eq!(second, 2);
        assert_eq!(ws.cell_ids(), [1, 2]);
        let third = ws.split(2, SplitDirection::Vertical, 120.0).unwrap();
        assert_eq!(ws.cell_ids(), [1, 2, 3]);
        let layout = ws.layout_json();
        assert!(layout.contains(r#""direction":"horizontal""#));
        assert!(layout.contains(r#""direction":"vertical""#));
        assert_eq!(ws.chart_count(), 3);
        // The new cell's id is fresh; the source cell keeps its creation time.
        let usage = ws.usage(130.0);
        assert_eq!(usage.chart_count, 3);
        assert_eq!(usage.split_count, 2);
        assert!((usage.elapsed_seconds - 30.0).abs() < 1e-9);
        let age = |id| usage.cells.iter().find(|c| c.id == id).unwrap().age_seconds;
        assert!((age(1) - 30.0).abs() < 1e-9);
        assert!((age(2) - 20.0).abs() < 1e-9);
        assert!((age(third) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn typed_layout_snapshot_tracks_splits_resizes_and_removals() {
        let mut ws = Workspace::new(0.0);
        assert_eq!(ws.layout(), WorkspaceLayout::Cell { id: 1 });

        let second = ws.split(1, SplitDirection::Horizontal, 1.0).unwrap();
        assert_eq!(
            ws.layout(),
            WorkspaceLayout::Split {
                direction: SplitDirection::Horizontal,
                ratio: 0.5,
                a: Box::new(WorkspaceLayout::Cell { id: 1 }),
                b: Box::new(WorkspaceLayout::Cell { id: second }),
            }
        );

        ws.resize_between(1, second, 0.25).unwrap();
        let third = ws.split(second, SplitDirection::Vertical, 2.0).unwrap();
        assert_eq!(
            ws.layout(),
            WorkspaceLayout::Split {
                direction: SplitDirection::Horizontal,
                ratio: 0.75,
                a: Box::new(WorkspaceLayout::Cell { id: 1 }),
                b: Box::new(WorkspaceLayout::Split {
                    direction: SplitDirection::Vertical,
                    ratio: 0.5,
                    a: Box::new(WorkspaceLayout::Cell { id: second }),
                    b: Box::new(WorkspaceLayout::Cell { id: third }),
                }),
            }
        );

        ws.remove(second).unwrap();
        assert_eq!(
            ws.layout(),
            WorkspaceLayout::Split {
                direction: SplitDirection::Horizontal,
                ratio: 0.75,
                a: Box::new(WorkspaceLayout::Cell { id: 1 }),
                b: Box::new(WorkspaceLayout::Cell { id: third }),
            }
        );
    }

    #[test]
    fn remove_collapses_the_split_node_and_the_last_cell_refuses() {
        let mut ws = Workspace::new(0.0);
        let second = ws.split(1, SplitDirection::Horizontal, 1.0).unwrap();
        let third = ws.split(second, SplitDirection::Vertical, 2.0).unwrap();
        // Remove the middle: cell 1 and cell 3 stay, the tree is a horizontal split again.
        ws.remove(second).unwrap();
        assert_eq!(ws.cell_ids(), [1, 3]);
        assert!(!ws.layout_json().contains("vertical"));
        // Root collapse: removing cell 1 leaves cell 3 as the only (root) cell.
        ws.remove(1).unwrap();
        assert_eq!(ws.cell_ids(), [3]);
        assert_eq!(ws.remove(third), Err(WorkspaceError::LastCell));
        assert_eq!(ws.remove(42), Err(WorkspaceError::NotFound));
    }

    #[test]
    fn max_charts_caps_splits() {
        let mut ws = Workspace::new(0.0);
        ws.set_max_charts(Some(2));
        assert!(ws.split(1, SplitDirection::Horizontal, 1.0).is_ok());
        assert_eq!(
            ws.split(1, SplitDirection::Horizontal, 2.0),
            Err(WorkspaceError::AtCapacity)
        );
        assert_eq!(ws.chart_count(), 2);
        ws.set_max_charts(None);
        assert!(ws.split(1, SplitDirection::Vertical, 3.0).is_ok());
        assert_eq!(ws.chart_count(), 3);
    }

    #[test]
    fn remove_placeholder_is_not_a_real_cell() {
        let mut ws = Workspace::new(0.0);
        ws.split(1, SplitDirection::Horizontal, 1.0).unwrap();
        ws.split(2, SplitDirection::Horizontal, 2.0).unwrap();
        ws.remove(2).unwrap();
        // The collapse must not leave the placeholder id anywhere.
        assert!(!ws.layout_json().contains(&u64::MAX.to_string()));
        assert_eq!(ws.cell_ids(), [1, 3]);
    }

    #[test]
    fn resize_between_adjusts_the_shared_divider_and_clamps() {
        let mut ws = Workspace::new(0.0);
        ws.split(1, SplitDirection::Horizontal, 1.0).unwrap();
        let third = ws.split(2, SplitDirection::Vertical, 2.0).unwrap();
        // Nested divider between cells 2 and 3 (the vertical split's own boundary)…
        ws.resize_between(2, third, 0.3).unwrap();
        assert!(ws.layout_json().contains(r#""ratio":0.8"#));
        // …and the outer divider between cell 1 and the (2,3) subtree (its first leaf is 2).
        ws.resize_between(1, 2, -0.2).unwrap();
        assert!(ws.layout_json().contains(r#""ratio":0.3"#));
        // Clamped: a huge drag cannot collapse a side.
        ws.resize_between(1, 2, -1.0).unwrap();
        assert!(ws.layout_json().contains(r#""ratio":0.05"#));
        ws.resize_between(1, 2, 1.0).unwrap();
        assert!(ws.layout_json().contains(r#""ratio":0.95"#));
        // No divider between non-adjacent cells.
        assert_eq!(
            ws.resize_between(1, third, 0.1),
            Err(WorkspaceError::NotFound)
        );
        // Ratios survive an unrelated split.
        ws.split(1, SplitDirection::Horizontal, 3.0).unwrap();
        assert!(ws.layout_json().contains(r#""ratio":0.95"#));
    }
}
