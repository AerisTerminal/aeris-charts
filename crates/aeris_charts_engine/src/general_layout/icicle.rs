//! Icicle (rectangular partition) of a hierarchy dataset (complete build), following the
//! d3-hierarchy 3.1.2 `partition()` definition with zero padding: every depth is one equal band
//! and each node's breadth divides its parent's in proportion to the summed leaf values, in
//! input order. Horizontal icicles put the root band on the left, vertical icicles on top; the
//! radial partition is the sunburst family. Cells are whole device pixels with a one-pixel gap
//! on their trailing edges, like the treemap.

use std::collections::HashMap;

use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{IRect, Prim};

use super::{
    HierarchyNode, HierarchyOrientation, LayoutSeriesEntry, TreemapShape, hierarchy_layout_tree,
    mosaic,
};
use crate::ChartEngine;
use crate::general_axes::GeneralPlotRect;

pub(super) struct IcicleCache {
    plot: GeneralPlotRect,
    orientation: HierarchyOrientation,
    /// Cells in depth-first pre-order; zero-value nodes have no cell.
    cells: Vec<TreemapShape>,
    /// Color slot of each row: the position of its top-level ancestor among the root's
    /// children, or `None` for the root.
    slots: Vec<Option<usize>>,
}

pub(super) fn cache_reservation(nodes: usize) -> usize {
    2 * nodes
        * (std::mem::size_of::<TreemapShape>()
            + std::mem::size_of::<Option<usize>>()
            + 2 * std::mem::size_of::<usize>())
}

impl IcicleCache {
    fn new(
        nodes: &[HierarchyNode],
        orientation: HierarchyOrientation,
        plot: GeneralPlotRect,
    ) -> Self {
        let mut cache = Self {
            plot,
            orientation,
            cells: Vec::new(),
            slots: slots(nodes),
        };
        if !(plot.width > 0.0 && plot.height > 0.0) {
            return cache;
        }
        let Some((root, children, weights)) = hierarchy_layout_tree(nodes) else {
            return cache;
        };
        let mut depth = vec![0usize; nodes.len()];
        let mut order = Vec::with_capacity(nodes.len());
        let mut stack = vec![root];
        while let Some(row) = stack.pop() {
            order.push(row);
            for &child in children[row].iter().rev() {
                depth[child] = depth[row] + 1;
                stack.push(child);
            }
        }
        let bands = depth.iter().copied().max().unwrap_or(0) + 1;
        let (dx, dy) = match orientation {
            HierarchyOrientation::Vertical => (plot.width, plot.height),
            _ => (plot.height, plot.width),
        };
        // d3 `x0..x1` along the breadth and `y0..y1` along the depth of every node.
        let mut extent = vec![(0.0, 0.0, 0.0, 0.0); nodes.len()];
        extent[root] = (0.0, dx, 0.0, dy / bands as f64);
        cache.cells.reserve(nodes.len());
        for &row in &order {
            let (x0, x1, y0, y1) = extent[row];
            let band = (
                dy * (depth[row] + 1) as f64 / bands as f64,
                dy * (depth[row] + 2) as f64 / bands as f64,
            );
            let k = if weights[row] > 0.0 {
                (x1 - x0) / weights[row]
            } else {
                0.0
            };
            let mut cursor = x0;
            for &child in &children[row] {
                let start = cursor;
                cursor += weights[child] * k;
                extent[child] = (start, cursor, band.0, band.1);
            }
            if x1 > x0 && y1 > y0 {
                let rect = match orientation {
                    HierarchyOrientation::Vertical => GeneralPlotRect {
                        x: plot.x + x0,
                        y: plot.y + y0,
                        width: x1 - x0,
                        height: y1 - y0,
                    },
                    _ => GeneralPlotRect {
                        x: plot.x + y0,
                        y: plot.y + x0,
                        width: y1 - y0,
                        height: x1 - x0,
                    },
                };
                cache.cells.push(TreemapShape { row, rect });
            }
        }
        cache
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.cells.capacity() * std::mem::size_of::<TreemapShape>()
            + self.slots.capacity() * std::mem::size_of::<Option<usize>>()
    }

    pub(super) fn hit(&self, x: f64, y: f64) -> Option<usize> {
        self.cells
            .iter()
            .find(|cell| cell.contains(x, y))
            .map(|cell| cell.row)
    }

    pub(super) fn anchor(&self, row: usize) -> Option<(f64, f64)> {
        self.cells.iter().find(|cell| cell.row == row).map(|cell| {
            (
                cell.rect.x + cell.rect.width * 0.5,
                cell.rect.y + cell.rect.height * 0.5,
            )
        })
    }
}

/// Each row's top-level branch slot (see [`IcicleCache::slots`]).
fn slots(nodes: &[HierarchyNode]) -> Vec<Option<usize>> {
    let ids: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(row, node)| (node.id.as_str(), row))
        .collect();
    let root_id = nodes
        .iter()
        .find(|node| node.parent.is_none())
        .map(|node| node.id.as_str());
    let mut branch = HashMap::new();
    for (row, node) in nodes.iter().enumerate() {
        if node.parent.is_some() && node.parent.as_deref() == root_id {
            let next = branch.len();
            branch.insert(row, next);
        }
    }
    nodes
        .iter()
        .enumerate()
        .map(|(row, _)| {
            let mut current = row;
            // Validated hierarchies are acyclic and at most MAX_LAYOUT_DEPTH deep.
            loop {
                if let Some(&slot) = branch.get(&current) {
                    return Some(slot);
                }
                current = *ids.get(nodes[current].parent.as_deref()?)?;
            }
        })
        .collect()
}

/// The fill of an icicle row: the root in the series color, each top-level branch in its own
/// palette color.
pub(super) fn cell_color(series_color: Color, nodes: &[HierarchyNode], row: usize) -> Color {
    slots(nodes)
        .get(row)
        .copied()
        .flatten()
        .map_or(series_color, |slot| {
            mosaic::segment_color(series_color, slot + 1)
        })
}

impl LayoutSeriesEntry {
    pub(super) fn with_icicle<R>(
        &self,
        nodes: &[HierarchyNode],
        plot: GeneralPlotRect,
        f: impl FnOnce(&IcicleCache) -> R,
    ) -> R {
        let mut cache = self.icicle.borrow_mut();
        if cache.as_ref().is_none_or(|cached| {
            cached.plot != plot || cached.orientation != self.options.hierarchy_orientation
        }) {
            *cache = Some(IcicleCache::new(
                nodes,
                self.options.hierarchy_orientation,
                plot,
            ));
        }
        f(cache.as_ref().expect("icicle cache initialized"))
    }
}

impl ChartEngine {
    pub(super) fn build_icicle_frame(
        &self,
        series: &LayoutSeriesEntry,
        nodes: &[HierarchyNode],
        plot: GeneralPlotRect,
        (hpr, vpr): (f64, f64),
        out: &mut Vec<Prim>,
    ) {
        let gap_x = hpr.round().max(1.0) as i32;
        let gap_y = vpr.round().max(1.0) as i32;
        series.with_icicle(nodes, plot, |cache| {
            for cell in &cache.cells {
                let x0 = (cell.rect.x * hpr).round() as i32;
                let y0 = (cell.rect.y * vpr).round() as i32;
                let x1 = ((cell.rect.x + cell.rect.width) * hpr).round() as i32 - gap_x;
                let y1 = ((cell.rect.y + cell.rect.height) * vpr).round() as i32 - gap_y;
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                let fill = cache.slots[cell.row].map_or(series.color, |slot| {
                    mosaic::segment_color(series.color, slot + 1)
                });
                out.push(Prim::Rect {
                    rect: IRect {
                        x: x0,
                        y: y0,
                        w: x1 - x0,
                        h: y1 - y0,
                    },
                    color: fill,
                });
                self.push_general_layout_label(
                    out,
                    &nodes[cell.row].label,
                    cell.rect,
                    fill,
                    hpr,
                    vpr,
                );
            }
        });
    }
}
