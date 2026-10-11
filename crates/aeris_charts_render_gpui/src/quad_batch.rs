//! Submission grouping for runs of plain quads.
//!
//! Every `paint_quad` costs GPUI a bounds-tree insertion, and every quad whose bounds overlap an
//! earlier path pushes the next path into a new draw order. A zoomed-out candlestick pane is
//! thousands of quads alternating between the up and down colors; submitted in order, short
//! same-color runs become paths interleaved with single quads, and the DirectX/Metal/Vulkan
//! renderers pay a full-window path rasterization pass for every resulting path batch.
//!
//! [`QuadBatcher`] instead assigns each quad of a run the smallest *level* that keeps it after
//! every earlier quad it must stay above, then submits the run as one path per `(level, paint)`.
//! Two quads must keep their order only when they overlap and the result depends on the order:
//! their paints differ, or the paint is translucent (blending twice is visible). Overlapping
//! quads of one opaque paint and disjoint quads commute, so the regrouped submission produces the
//! same pixels as the original order while a candle pane collapses to a handful of paths.
//!
//! The work is bounded: quads are bucketed by x, and a run whose overlap checks or group count
//! exceed fixed budgets falls back to merging only adjacent quads of one opaque paint, which never
//! reorders anything.

use crate::scene::{DeviceRect, Paint, SceneOp};

/// Minimum number of quads worth one path. Below this the per-path overhead (an intermediate
/// rasterization plus a compositing sprite) outweighs the saved `paint_quad` calls.
pub const QUAD_BATCH_THRESHOLD: usize = 8;

/// X buckets for the overlap search over one run.
const BUCKETS: usize = 256;
/// Overlap checks and bucket insertions allowed per quad before a run falls back.
const WORK_PER_QUAD: usize = 64;
/// Distinct `(level, paint)` groups allowed before a run falls back.
const MAX_GROUPS: usize = 64;

/// One group of a run: `count` op offsets starting at `first` in [`QuadBatcher::members`], all
/// with `paint`. Translucent members never overlap one another.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuadGroup {
    pub paint: Paint,
    pub first: usize,
    pub count: usize,
}

/// Reusable buffers for grouping quad runs; held by the renderer so steady-state painting does not
/// allocate.
#[derive(Debug, Default)]
pub struct QuadBatcher {
    levels: Vec<u32>,
    buckets: Vec<Vec<u32>>,
    keys: Vec<(u32, Paint)>,
    group_of: Vec<u32>,
    rank: Vec<u32>,
    members: Vec<u32>,
    groups: Vec<QuadGroup>,
}

/// The quad's rect and paint when it may join a regrouped run: a solid fill with no corner radii
/// and no border. Gradients are excluded because GPUI resolves a gradient against the primitive's
/// own bounds, so merging would restretch the ramp over the union.
pub fn plain_quad(op: &SceneOp) -> Option<(DeviceRect, Paint)> {
    match op {
        SceneOp::Quad {
            rect,
            fill: paint @ Paint::Solid(_),
            corner_radii,
            border_width,
            ..
        } if *border_width <= 0.0 && corner_radii.iter().all(|radius| *radius == 0.0) => {
            Some((*rect, *paint))
        }
        _ => None,
    }
}

/// Length of the run of plain quads starting at `start`, clamped to `end`.
pub fn plain_quad_run(ops: &[SceneOp], start: usize, end: usize) -> usize {
    ops[start..end.min(ops.len())]
        .iter()
        .take_while(|op| plain_quad(op).is_some())
        .count()
}

fn opaque(paint: Paint) -> bool {
    matches!(paint, Paint::Solid(color) if color.a() == 0xff)
}

impl QuadBatcher {
    /// Group `run` (every op a plain quad, see [`plain_quad_run`]) for submission; read the result
    /// through [`QuadBatcher::groups`].
    pub fn group(&mut self, run: &[SceneOp]) {
        self.groups.clear();
        self.members.clear();
        if !self.assign_levels(run) || !self.collect_groups(run) {
            self.adjacent_groups(run);
        }
    }

    /// Groups of the last [`QuadBatcher::group`] call, in submission order. Members are offsets
    /// into that run, in original order within each group.
    pub fn groups(&self) -> &[QuadGroup] {
        &self.groups
    }

    /// Member offsets referenced by the groups of the last [`QuadBatcher::group`] call.
    pub fn members(&self, group: &QuadGroup) -> &[u32] {
        &self.members[group.first..group.first + group.count]
    }

    /// Smallest level per quad that stays above every earlier conflicting quad, or `false` when
    /// the overlap search exceeds its budget.
    fn assign_levels(&mut self, run: &[SceneOp]) -> bool {
        let (mut left, mut right) = (f32::INFINITY, f32::NEG_INFINITY);
        for op in run {
            let Some((rect, _)) = plain_quad(op) else {
                return false;
            };
            left = left.min(rect.x);
            right = right.max(rect.right());
        }
        if !(left.is_finite() && right.is_finite()) {
            return false;
        }
        let width = ((right - left) / BUCKETS as f32).max(1.0);
        let bucket = |x: f32| (((x - left) / width).max(0.0) as usize).min(BUCKETS - 1);
        self.buckets.resize_with(BUCKETS, Vec::new);
        self.buckets.iter_mut().for_each(Vec::clear);
        self.levels.clear();
        let mut budget = WORK_PER_QUAD * run.len();
        for (index, (rect, paint)) in run.iter().filter_map(plain_quad).enumerate() {
            let span = bucket(rect.x)..=bucket(rect.right());
            let mut level = 0;
            for slot in span.clone() {
                let earlier = &self.buckets[slot];
                let Some(rest) = budget.checked_sub(earlier.len() + 1) else {
                    return false;
                };
                budget = rest;
                for &other in earlier {
                    let Some((other_rect, other_paint)) = plain_quad(&run[other as usize]) else {
                        continue;
                    };
                    if (other_paint != paint || !opaque(paint))
                        && rect.intersect(&other_rect).is_some()
                    {
                        level = level.max(self.levels[other as usize] + 1);
                    }
                }
            }
            self.levels.push(level);
            for slot in span {
                self.buckets[slot].push(index as u32);
            }
        }
        true
    }

    /// One group per `(level, paint)`, ordered by level and then first appearance, or `false`
    /// when the run has too many groups to be worth regrouping.
    fn collect_groups(&mut self, run: &[SceneOp]) -> bool {
        self.keys.clear();
        self.group_of.clear();
        for (op, &level) in run.iter().zip(&self.levels) {
            let Some((_, paint)) = plain_quad(op) else {
                return false;
            };
            let key = (level, paint);
            let group = match self.keys.iter().position(|candidate| *candidate == key) {
                Some(group) => group,
                None if self.keys.len() < MAX_GROUPS => {
                    self.keys.push(key);
                    self.keys.len() - 1
                }
                None => return false,
            };
            self.group_of.push(group as u32);
        }
        // Rank groups by level; the stable sort keeps first-appearance order inside a level.
        let mut by_level: Vec<u32> = (0..self.keys.len() as u32).collect();
        by_level.sort_by_key(|&group| self.keys[group as usize].0);
        self.rank.clear();
        self.rank.resize(self.keys.len(), 0);
        for (rank, &group) in by_level.iter().enumerate() {
            self.rank[group as usize] = rank as u32;
        }
        self.members.extend(0..run.len() as u32);
        let (rank, group_of) = (&self.rank, &self.group_of);
        self.members
            .sort_by_key(|&member| rank[group_of[member as usize] as usize]);
        let mut first = 0;
        while first < self.members.len() {
            let group = self.group_of[self.members[first] as usize];
            let count = self.members[first..]
                .iter()
                .take_while(|&&member| self.group_of[member as usize] == group)
                .count();
            self.groups.push(QuadGroup {
                paint: self.keys[group as usize].1,
                first,
                count,
            });
            first += count;
        }
        true
    }

    /// Original order, merging only adjacent quads of one opaque paint.
    fn adjacent_groups(&mut self, run: &[SceneOp]) {
        self.groups.clear();
        self.members.clear();
        self.members.extend(0..run.len() as u32);
        let mut first = 0;
        while first < run.len() {
            let Some((_, paint)) = plain_quad(&run[first]) else {
                first += 1;
                continue;
            };
            let count = if opaque(paint) {
                run[first..]
                    .iter()
                    .take_while(|op| plain_quad(op).is_some_and(|(_, other)| other == paint))
                    .count()
            } else {
                1
            };
            self.groups.push(QuadGroup {
                paint,
                first,
                count,
            });
            first += count;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeris_charts_render::color::Color;

    const UP: Color = Color::rgb(0x26, 0xa6, 0x9a);
    const DOWN: Color = Color::rgb(0xef, 0x53, 0x50);
    const VOLUME: Color = Color::rgba(0x26, 0xa6, 0x9a, 0x80);

    fn quad(x: f32, y: f32, w: f32, h: f32, color: Color) -> SceneOp {
        SceneOp::Quad {
            rect: DeviceRect::new(x, y, w, h),
            fill: Paint::Solid(color),
            corner_radii: [0.0; 4],
            border_width: 0.0,
            border_color: Color::rgba(0, 0, 0, 0),
        }
    }

    /// Composite the ops onto a white 1 px grid in the given order.
    fn render(ops: &[&SceneOp], width: usize, height: usize) -> Vec<[f32; 3]> {
        let mut pixels = vec![[1.0f32; 3]; width * height];
        for op in ops {
            let (rect, Paint::Solid(color)) = plain_quad(op).expect("plain quad") else {
                unreachable!("plain quads are solid")
            };
            let alpha = f32::from(color.a()) / 255.0;
            let rgb = [color.r(), color.g(), color.b()].map(|c| f32::from(c) / 255.0);
            for y in rect.y as usize..rect.bottom() as usize {
                for x in rect.x as usize..rect.right() as usize {
                    let pixel = &mut pixels[y * width + x];
                    for channel in 0..3 {
                        pixel[channel] = rgb[channel] * alpha + pixel[channel] * (1.0 - alpha);
                    }
                }
            }
        }
        pixels
    }

    fn grouped(batcher: &mut QuadBatcher, run: &[SceneOp]) -> Vec<QuadGroup> {
        batcher.group(run);
        batcher.groups().to_vec()
    }

    fn regrouped<'a>(batcher: &mut QuadBatcher, run: &'a [SceneOp]) -> Vec<&'a SceneOp> {
        let groups = grouped(batcher, run);
        groups
            .iter()
            .flat_map(|group| batcher.members(group).to_vec())
            .map(|member| &run[member as usize])
            .collect()
    }

    fn candles(columns: usize) -> Vec<SceneOp> {
        let mut state = 0x9e37_79b9_u32;
        let mut ops = Vec::new();
        for column in 0..columns {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let color = if state.is_multiple_of(3) { DOWN } else { UP };
            let top = (state % 20) as f32;
            ops.push(quad(column as f32, top, 1.0, 12.0, color));
            ops.push(quad(column as f32, top + 3.0, 1.0, 5.0, color));
        }
        ops
    }

    #[test]
    fn alternating_candles_collapse_to_one_group_per_color() {
        let run = candles(300);
        let mut batcher = QuadBatcher::default();
        let groups = grouped(&mut batcher, &run);
        assert_eq!(groups.len(), 2, "{groups:?}");
        assert_eq!(
            groups.iter().map(|group| group.count).sum::<usize>(),
            run.len()
        );
        assert_eq!(
            render(&regrouped(&mut batcher, &run), 300, 40),
            render(&run.iter().collect::<Vec<_>>(), 300, 40)
        );
    }

    #[test]
    fn overlapping_different_paints_keep_their_order() {
        // Per column: a bordered candle (border rect, body on top) and a translucent mark over
        // the border.
        let mut run = Vec::new();
        for column in 0..20 {
            let x = column as f32 * 3.0;
            run.push(quad(x, 0.0, 3.0, 10.0, DOWN));
            run.push(quad(x + 1.0, 1.0, 1.0, 8.0, UP));
            run.push(quad(x + 2.0, 4.0, 1.0, 2.0, VOLUME));
        }
        let mut batcher = QuadBatcher::default();
        let order = regrouped(&mut batcher, &run);
        assert_eq!(
            render(&order, 60, 10),
            render(&run.iter().collect::<Vec<_>>(), 60, 10)
        );
        assert_eq!(
            grouped(&mut batcher, &run).len(),
            3,
            "levels group across columns"
        );

        // The translucent mark now also overlaps the next column's border, chaining the levels.
        for op in run.iter_mut().skip(2).step_by(3) {
            if let SceneOp::Quad { rect, .. } = op {
                rect.w = 2.0;
            }
        }
        let order = regrouped(&mut batcher, &run);
        assert_eq!(order.len(), run.len());
        assert_eq!(
            render(&order, 61, 10),
            render(&run.iter().collect::<Vec<_>>(), 61, 10)
        );
    }

    #[test]
    fn overlapping_translucent_quads_of_one_paint_never_share_a_group() {
        let run = vec![quad(0.0, 0.0, 4.0, 4.0, VOLUME); 10];
        let mut batcher = QuadBatcher::default();
        let groups = grouped(&mut batcher, &run);
        assert!(groups.iter().all(|group| group.count == 1), "{groups:?}");
        assert_eq!(
            render(&regrouped(&mut batcher, &run), 4, 4),
            render(&run.iter().collect::<Vec<_>>(), 4, 4)
        );
    }

    #[test]
    fn disjoint_translucent_quads_share_a_group() {
        let run: Vec<_> = (0..30)
            .map(|column| quad(column as f32, 0.0, 1.0, 5.0, VOLUME))
            .collect();
        let mut batcher = QuadBatcher::default();
        assert_eq!(grouped(&mut batcher, &run).len(), 1);
    }

    #[test]
    fn a_run_over_budget_falls_back_to_adjacent_opaque_merges_in_order() {
        // Every quad covers the same pixel with a new paint: the level chain exceeds the group
        // budget, so nothing is reordered.
        let run: Vec<_> = (0..200u8)
            .map(|shade| quad(0.0, 0.0, 4.0, 4.0, Color::rgb(shade, 0, 0)))
            .collect();
        let mut batcher = QuadBatcher::default();
        let groups = grouped(&mut batcher, &run);
        assert_eq!(groups.len(), run.len());
        let members: Vec<u32> = groups
            .iter()
            .flat_map(|group| batcher.members(group).to_vec())
            .collect();
        assert_eq!(members, (0..200).collect::<Vec<u32>>());

        let mut same = vec![quad(0.0, 0.0, 4.0, 4.0, UP); 9];
        same.extend(vec![quad(0.0, 0.0, 4.0, 4.0, VOLUME); 2]);
        batcher.adjacent_groups(&same);
        let counts: Vec<usize> = batcher.groups.iter().map(|group| group.count).collect();
        assert_eq!(counts, vec![9, 1, 1]);
    }

    #[test]
    fn a_run_stops_at_anything_but_a_plain_solid_quad() {
        let mut ops = vec![quad(0.0, 0.0, 1.0, 1.0, UP); 4];
        ops.push(SceneOp::PopClip);
        ops.push(quad(0.0, 0.0, 1.0, 1.0, UP));
        assert_eq!(plain_quad_run(&ops, 0, ops.len()), 4);
        assert_eq!(plain_quad_run(&ops, 0, 2), 2);

        let rounded = SceneOp::Quad {
            rect: DeviceRect::new(0.0, 0.0, 4.0, 4.0),
            fill: Paint::Solid(UP),
            corner_radii: [2.0; 4],
            border_width: 0.0,
            border_color: DOWN,
        };
        let bordered = SceneOp::Quad {
            rect: DeviceRect::new(0.0, 0.0, 4.0, 4.0),
            fill: Paint::Solid(UP),
            corner_radii: [0.0; 4],
            border_width: 1.0,
            border_color: DOWN,
        };
        let gradient = SceneOp::Quad {
            rect: DeviceRect::new(0.0, 0.0, 4.0, 4.0),
            fill: Paint::VGradient {
                top: UP,
                bottom: DOWN,
            },
            corner_radii: [0.0; 4],
            border_width: 0.0,
            border_color: DOWN,
        };
        for op in [rounded, bordered, gradient] {
            assert!(plain_quad(&op).is_none(), "{op:?}");
        }
    }
}
