//! Multi-chart workspace host: the wasm surface over `nucleuscharts_engine::Workspace` (the split-grid
//! MODEL). The browser side keeps only DOM placement and per-cell chart instances keyed by the
//! cell ids this returns; every topology decision, the usage metering, and the paywall cap are
//! engine-owned and headless-tested.

use nucleuscharts_engine::{SplitDirection, Workspace, WorkspaceError};
use wasm_bindgen::prelude::*;

/// Host-facing workspace handle. Clock is host-injected seconds (Date.now()/1000).
#[wasm_bindgen]
pub struct NucleusWorkspace {
    workspace: Workspace,
}

#[wasm_bindgen]
impl NucleusWorkspace {
    /// A workspace holding a single chart cell (id 1).
    #[wasm_bindgen(constructor)]
    pub fn new(now: f64) -> Self {
        Self {
            workspace: Workspace::new(now),
        }
    }

    /// Split a cell: `"horizontal"` (side by side) / `"vertical"` (stacked). Returns the new
    /// cell's id, or -1 when the cap rejects it / the cell is unknown.
    pub fn split(&mut self, id: u32, direction: &str, now: f64) -> i64 {
        let direction = match direction {
            "horizontal" => SplitDirection::Horizontal,
            "vertical" => SplitDirection::Vertical,
            _ => return -1,
        };
        match self.workspace.split(id as u64, direction, now) {
            Ok(new_id) => new_id as i64,
            Err(_) => -1,
        }
    }

    /// Remove a cell; false when the id is unknown or it is the last chart.
    pub fn remove(&mut self, id: u32) -> bool {
        matches!(self.workspace.remove(id as u64), Ok(()))
    }

    /// Drag the divider between two adjacent cells: adjusts their shared split's ratio by
    /// `delta_ratio` (the `a` side's share change, clamped to [0.05, 0.95]). False when no
    /// such divider exists.
    pub fn resize_between(&mut self, left_id: u32, right_id: u32, delta_ratio: f64) -> bool {
        self.workspace
            .resize_between(left_id as u64, right_id as u64, delta_ratio)
            .is_ok()
    }

    /// Hard cap on live charts (`undefined` clears it).
    pub fn set_max_charts(&mut self, max: Option<u32>) {
        self.workspace.set_max_charts(max.map(|m| m as usize));
    }

    pub fn chart_count(&self) -> usize {
        self.workspace.chart_count()
    }

    /// Leaf cell ids in layout order as a JSON array.
    pub fn cell_ids_json(&self) -> String {
        serde_json::to_string(&self.workspace.cell_ids()).unwrap_or_else(|_| "[]".to_string())
    }

    /// The layout tree as JSON (cells by id, splits with direction) for host DOM placement.
    pub fn layout_json(&self) -> String {
        self.workspace.layout_json()
    }

    /// The usage/metering snapshot at host time `now` as JSON (`chart_count`, `split_count`,
    /// `elapsed_seconds`, per-cell `age_seconds`).
    pub fn usage_json(&self, now: f64) -> String {
        serde_json::to_string(&self.workspace.usage(now)).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Maps engine rejections to the host convention (-1 / false); kept for future richer errors.
#[allow(dead_code)]
fn _err_code(err: WorkspaceError) -> i64 {
    match err {
        WorkspaceError::AtCapacity => -1,
        WorkspaceError::NotFound => -2,
        WorkspaceError::LastCell => -3,
    }
}
