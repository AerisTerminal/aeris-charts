//! Venn diagram of up to eight sets (complete build). Two- and three-set diagrams are
//! area-proportional: two sets solve the lens overlap by bisection, three sets start from the
//! pairwise bisection solution and minimize the squared region error with a bounded
//! Nelder-Mead search, keeping the final fit error on the cache. Larger diagrams place
//! area-sized circles on a ring as a labelled approximation: their overlaps carry no area
//! meaning and only set labels and region tooltips speak for the data. The named algorithm
//! reference is venn.js 0.2 (MIT); no implementation code was copied.
//!
//! Region sizes are exclusive: `{sets: ["A", "B"], size}` is the area of the part of A and B
//! that no other listed set covers, as in venn.js.

use std::collections::HashSet;
use std::f64::consts::PI;

use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{Prim, TextAlign};

use super::{
    GeneralLayoutElement, LayoutSeriesEntry, VennSet, add_text_bytes, invalid_data,
    layout_value_text, mosaic, resource_limit, validate_value,
};
use crate::general_axes::GeneralPlotRect;
use crate::{ChartEngine, ChartError};

/// Distinct sets a diagram can hold; the membership mask of a region fits a `u8`.
pub(super) const MAX_VENN_SETS: usize = 8;
/// Exclusive regions (input rows) a dataset can hold.
pub(super) const MAX_VENN_REGIONS: usize = 255;
/// Fill opacity of the set circles; overlaps deepen by painter's blending.
const FILL_ALPHA: u8 = 0x40;
/// Space reserved between the circle bounds and the plot edges.
const PADDING: f64 = 16.0;
/// Nelder-Mead iteration bound for the three-set fit.
const NELDER_MEAD_ITERATIONS: usize = 300;
/// Side of the candidate grid sampled for region anchors (bounded label placement).
const ANCHOR_GRID: usize = 16;
const LABEL_SIZE: f64 = 12.0;

/// One set's circle in plot coordinates.
#[derive(Clone, Copy, Debug)]
struct VennCircle {
    x: f64,
    y: f64,
    r: f64,
}

/// Circles and region anchors of one venn series for one plot rectangle.
pub(super) struct VennCache {
    plot: GeneralPlotRect,
    /// One circle per distinct set name, in first-appearance order.
    circles: Vec<VennCircle>,
    /// Set names in first-appearance order; `names[i]` labels `circles[i]`.
    names: Vec<String>,
    /// Membership mask of each input row: bit `i` set when circle `i` belongs to the region.
    masks: Vec<u8>,
    /// Best sampled interior point of each input row with its clearance to every boundary,
    /// when a point with exactly that membership exists in the laid-out circles.
    anchors: Vec<Option<((f64, f64), f64)>>,
    /// Root-squared relative region error of the three-set fit; zero otherwise. The
    /// fit-quality contract of the bounded search, observed by the module tests.
    #[allow(dead_code)]
    fit_error: f64,
}

/// Reserved cache bytes for any venn dataset: circles, names and anchors are bounded by the
/// set and region caps.
pub(super) fn cache_reservation() -> usize {
    MAX_VENN_SETS * (std::mem::size_of::<VennCircle>() + std::mem::size_of::<String>() + 24)
        + MAX_VENN_REGIONS
            * (std::mem::size_of::<u8>() + std::mem::size_of::<Option<((f64, f64), f64)>>())
}

/// Distinct set names in first-appearance order.
fn set_names(sets: &[VennSet]) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    for region in sets {
        for name in &region.sets {
            if !names.contains(&name.as_str()) {
                names.push(name);
            }
        }
    }
    names
}

/// Tooltip label of a region: its label override or the set names joined with `∩`.
pub(super) fn region_label(sets: &[VennSet], row: usize) -> Option<String> {
    let region = sets.get(row)?;
    Some(
        region
            .label
            .clone()
            .unwrap_or_else(|| region.sets.join(" ∩ ")),
    )
}

/// Color slot of a region: the first-appearance index of its first set.
pub(super) fn slot(sets: &[VennSet], row: usize) -> usize {
    let names = set_names(sets);
    sets.get(row)
        .and_then(|region| region.sets.first())
        .and_then(|name| names.iter().position(|n| *n == name.as_str()))
        .unwrap_or(0)
}

/// Membership mask of one region against `names`.
fn mask_of(names: &[&str], region: &VennSet) -> u8 {
    region.sets.iter().fold(0u8, |mask, name| {
        mask | 1u8 << names.iter().position(|n| n == name).unwrap_or(0)
    })
}

pub(super) fn validate(sets: &[VennSet]) -> Result<(), ChartError> {
    if sets.is_empty() {
        return Err(invalid_data("a venn dataset needs at least one region"));
    }
    if sets.len() > MAX_VENN_REGIONS {
        return Err(resource_limit("too many venn regions"));
    }
    let mut combos = HashSet::with_capacity(sets.len());
    let mut names: Vec<&str> = Vec::new();
    let mut bytes = 0usize;
    for region in sets {
        if region.sets.is_empty() || region.sets.len() > MAX_VENN_SETS {
            return Err(invalid_data("a venn region names between 1 and 8 sets"));
        }
        validate_value(region.size)?;
        let mut key: Vec<&str> = region.sets.iter().map(String::as_str).collect();
        key.sort_unstable();
        for name in &key {
            if name.is_empty() {
                return Err(invalid_data("a venn set name is empty"));
            }
            add_text_bytes(&mut bytes, name)?;
        }
        if key.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(invalid_data("a venn region names each set once"));
        }
        if !combos.insert(key) {
            return Err(invalid_data("duplicate venn region"));
        }
        if let Some(label) = &region.label {
            add_text_bytes(&mut bytes, label)?;
        }
        for name in &region.sets {
            if !names.contains(&name.as_str()) {
                if names.len() >= MAX_VENN_SETS {
                    return Err(resource_limit("too many venn sets"));
                }
                names.push(name);
            }
        }
    }
    for name in &names {
        if !sets
            .iter()
            .any(|region| region.sets.len() == 1 && region.sets[0] == *name)
        {
            return Err(invalid_data("every venn set needs its own region"));
        }
    }
    // A region cannot outweigh a set it belongs to. Region sizes are exclusive, so the set
    // total includes the region itself; the guard documents the contract and rejects negative
    // or misordered accounting the moment sizes stop summing.
    for region in sets {
        if region.sets.len() < 2 {
            continue;
        }
        for name in &region.sets {
            let total: f64 = sets
                .iter()
                .filter(|other| other.sets.iter().any(|n| n == name))
                .map(|other| other.size)
                .sum();
            if region.size > total {
                return Err(invalid_data("a venn region exceeds its set"));
            }
        }
    }
    Ok(())
}

fn circle_radius(area: f64) -> f64 {
    (area.max(0.0) / PI).sqrt()
}

/// Exact area of the overlap of two circles `d` apart.
fn lens_area(r0: f64, r1: f64, d: f64) -> f64 {
    if r0 <= 0.0 || r1 <= 0.0 || d >= r0 + r1 {
        return 0.0;
    }
    if d <= (r0 - r1).abs() {
        return PI * r0.min(r1).powi(2);
    }
    let a0 = ((d * d + r0 * r0 - r1 * r1) / (2.0 * d * r0)).clamp(-1.0, 1.0);
    let a1 = ((d * d + r1 * r1 - r0 * r0) / (2.0 * d * r1)).clamp(-1.0, 1.0);
    let heron = ((-d + r0 + r1) * (d + r0 - r1) * (d - r0 + r1) * (d + r0 + r1)).max(0.0);
    r0 * r0 * a0.acos() + r1 * r1 * a1.acos() - 0.5 * heron.sqrt()
}

/// Center distance giving two circles the requested overlap area, by bisection on the
/// monotone lens area. Exact to ~2^-80 of the radius scale.
fn distance_for_overlap(r0: f64, r1: f64, overlap: f64) -> f64 {
    let lo = (r0 - r1).abs();
    let hi = r0 + r1;
    if overlap <= 0.0 || r0 <= 0.0 || r1 <= 0.0 {
        return hi;
    }
    if overlap >= PI * r0.min(r1).powi(2) {
        return lo;
    }
    let (mut lo, mut hi) = (lo, hi);
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if lens_area(r0, r1, mid) > overlap {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

type Point = (f64, f64);

/// Both intersection points of two genuinely crossing circles; `None` for disjoint or
/// contained circles.
fn circle_crossings(a: VennCircle, b: VennCircle) -> Option<(Point, Point)> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let d = dx.hypot(dy);
    if d <= 0.0 || d >= a.r + b.r || d <= (a.r - b.r).abs() {
        return None;
    }
    let along = (d * d + a.r * a.r - b.r * b.r) / (2.0 * d);
    let height = (a.r * a.r - along * along).max(0.0).sqrt();
    let (ux, uy) = (dx / d, dy / d);
    let (mx, my) = (a.x + along * ux, a.y + along * uy);
    Some((
        (mx - height * uy, my + height * ux),
        (mx + height * uy, my - height * ux),
    ))
}

/// Area of the circular segment of a circle of radius `r` cut by a chord of width `w`.
fn segment_area(r: f64, w: f64) -> f64 {
    let half = (w / (2.0 * r)).clamp(0.0, 1.0);
    r * r * half.asin() - 0.25 * w * (4.0 * r * r - w * w).max(0.0).sqrt()
}

/// Exact area of the common part of three circles: the triangle of the three inner pairwise
/// crossings plus one circular segment per circle. Degenerate configurations (a contained or
/// disjoint pair) fall back to the contained circle's overlap with the third circle or zero.
fn triple_area(c: [VennCircle; 3]) -> f64 {
    for i in 0..3 {
        for j in 0..3 {
            if i == j {
                continue;
            }
            let k = 3 - i - j;
            let d = (c[j].x - c[i].x).hypot(c[j].y - c[i].y);
            if d + c[j].r <= c[i].r + 1e-12 * c[i].r.max(1.0) {
                return lens_area(c[j].r, c[k].r, (c[k].x - c[j].x).hypot(c[k].y - c[j].y));
            }
        }
    }
    let mut points = [(0.0, 0.0); 3];
    for (slot, (i, j)) in [(0usize, 1usize), (0, 2), (1, 2)].iter().enumerate() {
        let k = 3 - i - j;
        let d = (c[*j].x - c[*i].x).hypot(c[*j].y - c[*i].y);
        if d >= c[*i].r + c[*j].r {
            return 0.0;
        }
        let Some((p, q)) = circle_crossings(c[*i], c[*j]) else {
            return 0.0;
        };
        let inside = |point: Point| {
            (point.0 - c[k].x).hypot(point.1 - c[k].y) <= c[k].r + 1e-9 * c[k].r.max(1.0)
        };
        points[slot] = match (inside(p), inside(q)) {
            (true, _) => p,
            (false, true) => q,
            (false, false) => return 0.0,
        };
    }
    let (a, b, p) = (points[0], points[1], points[2]);
    let triangle = 0.5 * ((b.0 - a.0) * (p.1 - a.1) - (p.0 - a.0) * (b.1 - a.1)).abs();
    let chord = |u: Point, v: Point| (v.0 - u.0).hypot(v.1 - u.1);
    triangle
        + segment_area(c[0].r, chord(points[0], points[1]))
        + segment_area(c[1].r, chord(points[0], points[2]))
        + segment_area(c[2].r, chord(points[1], points[2]))
}

/// Estimated exclusive area of the region `mask` (bit 0 = circle A) of three circles.
fn region_estimate(c: [VennCircle; 3], mask: u8) -> f64 {
    let area = [
        PI * c[0].r * c[0].r,
        PI * c[1].r * c[1].r,
        PI * c[2].r * c[2].r,
    ];
    let d = |i: usize, j: usize| (c[j].x - c[i].x).hypot(c[j].y - c[i].y);
    let (i01, i02, i12) = (
        lens_area(c[0].r, c[1].r, d(0, 1)),
        lens_area(c[0].r, c[2].r, d(0, 2)),
        lens_area(c[1].r, c[2].r, d(1, 2)),
    );
    let t = triple_area(c);
    match mask {
        0b001 => area[0] - i01 - i02 + t,
        0b010 => area[1] - i01 - i12 + t,
        0b100 => area[2] - i02 - i12 + t,
        0b011 => i01 - t,
        0b101 => i02 - t,
        0b110 => i12 - t,
        0b111 => t,
        _ => 0.0,
    }
}

/// Squared region error of the three-circle placement `(x1, y1, x2, y2)` with circle 0 at
/// (0, 0), over the dataset's regions.
fn three_circle_loss(radii: [f64; 3], regions: &[(u8, f64)], [x1, y1, x2, y2]: [f64; 4]) -> f64 {
    let circles = [
        VennCircle {
            x: 0.0,
            y: 0.0,
            r: radii[0],
        },
        VennCircle {
            x: x1,
            y: y1,
            r: radii[1],
        },
        VennCircle {
            x: x2,
            y: y2,
            r: radii[2],
        },
    ];
    regions
        .iter()
        .map(|&(mask, want)| {
            let error = region_estimate(circles, mask) - want;
            error * error
        })
        .sum()
}

/// Bounded downhill simplex (Nelder-Mead) minimization of `loss` from `start`: 4 parameters,
/// 300 iterations, no allocations past the 5-point simplex.
fn nelder_mead(start: [f64; 4], step: f64, loss: impl Fn([f64; 4]) -> f64) -> ([f64; 4], f64) {
    let mut simplex = [start; 5];
    for (index, point) in simplex.iter_mut().enumerate().skip(1) {
        point[index - 1] += step;
    }
    let mut values = simplex.map(&loss);
    let centroid = |simplex: &[[f64; 4]; 5], worst: usize| {
        let mut center = [0.0; 4];
        for (index, point) in simplex.iter().enumerate() {
            if index != worst {
                for dim in 0..4 {
                    center[dim] += point[dim] * 0.25;
                }
            }
        }
        center
    };
    for _ in 0..NELDER_MEAD_ITERATIONS {
        let mut order = [0usize; 5];
        order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
        let (best, worst) = (order[0], order[4]);
        if values[worst] - values[best] <= 1e-18 * values[best].max(1.0) {
            break;
        }
        let center = centroid(&simplex, worst);
        let reflect = |scale: f64| {
            let mut point = [0.0; 4];
            for dim in 0..4 {
                point[dim] = center[dim] + scale * (center[dim] - simplex[worst][dim]);
            }
            point
        };
        let reflected = reflect(1.0);
        let reflected_value = loss(reflected);
        if reflected_value < values[best] {
            let expanded = reflect(2.0);
            let expanded_value = loss(expanded);
            let (point, value) = if expanded_value < reflected_value {
                (expanded, expanded_value)
            } else {
                (reflected, reflected_value)
            };
            simplex[worst] = point;
            values[worst] = value;
        } else if reflected_value < values[order[3]] {
            simplex[worst] = reflected;
            values[worst] = reflected_value;
        } else {
            let contracted = reflect(-0.5);
            let contracted_value = loss(contracted);
            if contracted_value < values[worst] {
                simplex[worst] = contracted;
                values[worst] = contracted_value;
            } else {
                let best_point = simplex[best];
                for &point in &order[1..] {
                    for (coord, &best_coord) in simplex[point].iter_mut().zip(&best_point) {
                        *coord = best_coord + 0.5 * (*coord - best_coord);
                    }
                    values[point] = loss(simplex[point]);
                }
            }
        }
    }
    let best = (0..5)
        .min_by(|&a, &b| values[a].total_cmp(&values[b]))
        .unwrap_or(0);
    (simplex[best], values[best])
}

/// Area-proportional placement of two or three circles, with the fit error of the three-set
/// search. Circles come out in abstract coordinates with circle 0 at (0, 0).
fn fitted_circles(radii: &[f64], regions: &[(u8, f64)]) -> (Vec<VennCircle>, f64) {
    match radii.len() {
        2 => {
            let overlap = regions
                .iter()
                .find(|(mask, _)| *mask == 0b11)
                .map_or(0.0, |&(_, size)| size);
            let d = distance_for_overlap(radii[0], radii[1], overlap);
            (
                vec![
                    VennCircle {
                        x: 0.0,
                        y: 0.0,
                        r: radii[0],
                    },
                    VennCircle {
                        x: d,
                        y: 0.0,
                        r: radii[1],
                    },
                ],
                0.0,
            )
        }
        3 => {
            // The overlap a pair of circles must show is every region both sets cover.
            let pair = |a: u8, b: u8| {
                regions
                    .iter()
                    .filter(|(mask, _)| mask & a != 0 && mask & b != 0)
                    .map(|&(_, size)| size)
                    .sum::<f64>()
            };
            let d01 = distance_for_overlap(radii[0], radii[1], pair(0b001, 0b010));
            let d02 = distance_for_overlap(radii[0], radii[2], pair(0b001, 0b100));
            let d12 = distance_for_overlap(radii[1], radii[2], pair(0b010, 0b100));
            let x2 = if d01 > 0.0 {
                (d02 * d02 - d12 * d12 + d01 * d01) / (2.0 * d01)
            } else {
                0.0
            };
            let y2 = (d02 * d02 - x2 * x2).max(0.0).sqrt();
            let start = [d01, 0.0, x2, y2];
            let step = radii.iter().copied().fold(0.0, f64::max).max(1.0) * 0.25;
            let radii3 = [radii[0], radii[1], radii[2]];
            let (point, loss) = nelder_mead(start, step, |p| three_circle_loss(radii3, regions, p));
            let total: f64 = regions.iter().map(|&(_, size)| size).sum();
            let fit_error = loss.sqrt() / total.max(1e-9);
            (
                vec![
                    VennCircle {
                        x: 0.0,
                        y: 0.0,
                        r: radii[0],
                    },
                    VennCircle {
                        x: point[0],
                        y: point[1],
                        r: radii[1],
                    },
                    VennCircle {
                        x: point[2],
                        y: point[3],
                        r: radii[2],
                    },
                ],
                fit_error,
            )
        }
        _ => (Vec::new(), 0.0),
    }
}

/// The membership mask of `point` against `circles`.
fn membership(circles: &[VennCircle], point: Point) -> u8 {
    circles
        .iter()
        .enumerate()
        .fold(0u8, |mask, (index, circle)| {
            if (point.0 - circle.x).hypot(point.1 - circle.y) <= circle.r {
                mask | (1u8 << index)
            } else {
                mask
            }
        })
}

impl VennCache {
    fn new(sets: &[VennSet], plot: GeneralPlotRect) -> Self {
        let names = set_names(sets);
        let masks: Vec<u8> = sets.iter().map(|region| mask_of(&names, region)).collect();
        let mut totals = vec![0.0_f64; names.len()];
        for (region, &mask) in sets.iter().zip(&masks) {
            for (bit, total) in totals.iter_mut().enumerate() {
                if mask & (1u8 << bit) != 0 {
                    *total += region.size;
                }
            }
        }
        let radii: Vec<f64> = totals.iter().map(|&area| circle_radius(area)).collect();
        let regions: Vec<(u8, f64)> = masks
            .iter()
            .copied()
            .zip(sets.iter().map(|region| region.size))
            .collect();
        let (mut circles, mut fit_error) = if names.len() <= 3 {
            if names.len() == 1 {
                (
                    vec![VennCircle {
                        x: 0.0,
                        y: 0.0,
                        r: radii[0],
                    }],
                    0.0,
                )
            } else {
                fitted_circles(&radii, &regions)
            }
        } else {
            // Above three sets an exact area-proportional circle layout generally does not
            // exist: the circles keep their areas and sit on a ring as a labelled
            // approximation, and the overlaps carry no area meaning.
            let largest = radii.iter().copied().fold(0.0, f64::max);
            let ring = 0.55 * largest;
            let count = names.len() as f64;
            (
                radii
                    .iter()
                    .enumerate()
                    .map(|(index, &r)| {
                        let angle = -0.5 * PI + std::f64::consts::TAU * index as f64 / count;
                        VennCircle {
                            x: ring * angle.cos(),
                            y: ring * angle.sin(),
                            r,
                        }
                    })
                    .collect(),
                0.0,
            )
        };
        // Normalize: scale the circle bounds into the padded plot, centered.
        if let Some(first) = circles.first() {
            let (mut x0, mut y0, mut x1, mut y1) = (
                first.x - first.r,
                first.y - first.r,
                first.x + first.r,
                first.y + first.r,
            );
            for circle in &circles[1..] {
                x0 = x0.min(circle.x - circle.r);
                y0 = y0.min(circle.y - circle.r);
                x1 = x1.max(circle.x + circle.r);
                y1 = y1.max(circle.y + circle.r);
            }
            let (bw, bh) = ((x1 - x0).max(1e-9), (y1 - y0).max(1e-9));
            let inner_w = (plot.width - 2.0 * PADDING).max(0.0);
            let inner_h = (plot.height - 2.0 * PADDING).max(0.0);
            let scale = (inner_w / bw).min(inner_h / bh).max(0.0);
            let ox = plot.x + 0.5 * (plot.width - scale * (x0 + x1));
            let oy = plot.y + 0.5 * (plot.height - scale * (y0 + y1));
            for circle in &mut circles {
                circle.x = ox + scale * circle.x;
                circle.y = oy + scale * circle.y;
                circle.r *= scale;
            }
            if !fit_error.is_finite() {
                fit_error = 0.0;
            }
        }
        let mut cache = Self {
            plot,
            anchors: vec![None; sets.len()],
            circles,
            names: names.iter().map(|name| (*name).to_owned()).collect(),
            masks,
            fit_error,
        };
        cache.place_anchors();
        cache
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.circles.capacity() * std::mem::size_of::<VennCircle>()
            + self.names.capacity() * std::mem::size_of::<String>()
            + self.names.iter().map(String::capacity).sum::<usize>()
            + self.masks.capacity() * std::mem::size_of::<u8>()
            + self.anchors.capacity() * std::mem::size_of::<Option<((f64, f64), f64)>>()
    }

    /// Whether anything can paint or answer hits.
    fn drawable(&self) -> bool {
        self.plot.width > 0.0
            && self.plot.height > 0.0
            && self.circles.iter().any(|circle| circle.r > 0.0)
    }

    /// Sample each region's best interior point: candidates are circle centers, inner pairwise
    /// crossings and a bounded grid over the member circles' bounds, scored by the clearance
    /// to the nearest member boundary and the nearest non-member interior.
    fn place_anchors(&mut self) {
        if !self.drawable() {
            return;
        }
        for (row, &mask) in self.masks.iter().enumerate() {
            let members: Vec<usize> = (0..self.circles.len())
                .filter(|&index| mask & (1u8 << index) != 0)
                .collect();
            let Some(&first) = members.first() else {
                continue;
            };
            let (mut x0, mut y0, mut x1, mut y1) = {
                let circle = self.circles[first];
                (
                    circle.x - circle.r,
                    circle.y - circle.r,
                    circle.x + circle.r,
                    circle.y + circle.r,
                )
            };
            for &index in &members[1..] {
                let circle = self.circles[index];
                x0 = x0.min(circle.x - circle.r);
                y0 = y0.min(circle.y - circle.r);
                x1 = x1.max(circle.x + circle.r);
                y1 = y1.max(circle.y + circle.r);
            }
            let mut candidates: Vec<Point> = members
                .iter()
                .map(|&index| (self.circles[index].x, self.circles[index].y))
                .collect();
            for (position, &a) in members.iter().enumerate() {
                for &b in &members[position + 1..] {
                    if let Some((p, q)) = circle_crossings(self.circles[a], self.circles[b]) {
                        candidates.push(p);
                        candidates.push(q);
                    }
                }
            }
            for gx in 0..ANCHOR_GRID {
                for gy in 0..ANCHOR_GRID {
                    candidates.push((
                        x0 + (x1 - x0) * (gx as f64 + 0.5) / ANCHOR_GRID as f64,
                        y0 + (y1 - y0) * (gy as f64 + 0.5) / ANCHOR_GRID as f64,
                    ));
                }
            }
            let clearance = |point: Point| {
                members.iter().fold(f64::INFINITY, |best, &index| {
                    let circle = self.circles[index];
                    best.min(circle.r - (point.0 - circle.x).hypot(point.1 - circle.y))
                })
            };
            let exclusion = |point: Point| {
                (0..self.circles.len())
                    .filter(|index| mask & (1u8 << index) == 0)
                    .fold(f64::INFINITY, |best, index| {
                        let circle = self.circles[index];
                        best.min((point.0 - circle.x).hypot(point.1 - circle.y) - circle.r)
                    })
            };
            self.anchors[row] = candidates
                .into_iter()
                .filter(|&point| membership(&self.circles, point) == mask)
                .map(|point| (point, clearance(point).min(exclusion(point))))
                .max_by(|a, b| a.1.total_cmp(&b.1));
        }
    }

    /// The input row whose exclusive region contains the point, when one does.
    pub(super) fn hit(&self, x: f64, y: f64) -> Option<(GeneralLayoutElement, usize)> {
        if !self.drawable() {
            return None;
        }
        let mask = membership(&self.circles, (x, y));
        if mask == 0 {
            return None;
        }
        let row = self.masks.iter().position(|&m| m == mask)?;
        Some((GeneralLayoutElement::Region, row))
    }

    /// The sampled interior point of a region, for its focus ring and tooltip anchor.
    pub(super) fn anchor(&self, row: usize) -> Option<(f64, f64)> {
        self.anchors
            .get(row)
            .copied()
            .flatten()
            .map(|(point, _)| point)
    }
}

impl LayoutSeriesEntry {
    pub(super) fn with_venn<R>(
        &self,
        sets: &[VennSet],
        plot: GeneralPlotRect,
        f: impl FnOnce(&VennCache) -> R,
    ) -> R {
        let mut cache = self.venn.borrow_mut();
        if cache.as_ref().is_none_or(|cached| cached.plot != plot) {
            *cache = Some(VennCache::new(sets, plot));
        }
        f(cache.as_ref().expect("venn cache initialized"))
    }
}

fn with_alpha(color: Color, alpha: u8) -> Color {
    Color((color.0 & !0xFF) | u32::from(alpha))
}

impl ChartEngine {
    /// Translucent fills, then outlines, then set labels at the sampled interior points, then
    /// optional region sizes.
    pub(super) fn build_venn_frame(
        &self,
        series: &LayoutSeriesEntry,
        sets: &[VennSet],
        plot: GeneralPlotRect,
        (hpr, vpr): (f64, f64),
        out: &mut Vec<Prim>,
    ) {
        let family = self.options.get().layout.font_family.clone();
        let text_color = self.primary_text_color();
        series.with_venn(sets, plot, |cache| {
            if !cache.drawable() {
                return;
            }
            let ratio = hpr.min(vpr);
            let transparent = Color::rgba(0, 0, 0, 0);
            for (slot, circle) in cache.circles.iter().enumerate() {
                out.push(Prim::Circle {
                    cx: (circle.x * hpr) as f32,
                    cy: (circle.y * vpr) as f32,
                    radius: (circle.r * ratio) as f32,
                    fill: with_alpha(mosaic::segment_color(series.color, slot), FILL_ALPHA),
                    stroke_width: 0.0,
                    stroke: transparent,
                });
            }
            for (slot, circle) in cache.circles.iter().enumerate() {
                out.push(Prim::Circle {
                    cx: (circle.x * hpr) as f32,
                    cy: (circle.y * vpr) as f32,
                    radius: (circle.r * ratio) as f32,
                    fill: transparent,
                    stroke_width: (1.5 * ratio) as f32,
                    stroke: mosaic::segment_color(series.color, slot),
                });
            }
            let label_size = LABEL_SIZE.clamp(9.0, 18.0);
            let push_label = |out: &mut Vec<Prim>, text: &str, point: Point, margin: f64| {
                if text.is_empty() {
                    return;
                }
                let width = self.measure_text_run(text, label_size, &family, 400, false);
                if !width.is_finite() || width > 1.8 * margin || label_size > 1.8 * margin {
                    return;
                }
                out.push(Prim::Text {
                    x: (point.0 * hpr) as f32,
                    y: (point.1 * vpr) as f32,
                    text: text.to_owned(),
                    color: text_color,
                    size: (label_size * vpr) as f32,
                    family: family.clone(),
                    align: TextAlign::Center,
                    weight: 400,
                    italic: false,
                });
            };
            // Set labels sit in the deepest point of each set's exclusive core; when another
            // region fills that core, the label moves to the set's best remaining point.
            for (slot, name) in cache.names.iter().enumerate() {
                let singleton = cache.masks.iter().position(|&mask| mask == (1u8 << slot));
                let point = singleton.and_then(|row| cache.anchors[row]).or_else(|| {
                    let circle = cache.circles[slot];
                    (circle.r > 0.0).then_some(((circle.x, circle.y), circle.r))
                });
                if let Some((point, margin)) = point {
                    let text = singleton
                        .and_then(|row| sets[row].label.as_deref())
                        .unwrap_or(name);
                    push_label(out, text, point, margin.max(0.0));
                }
            }
            if series.options.data_labels {
                for (row, region) in sets.iter().enumerate() {
                    if let Some((point, margin)) = cache.anchors[row] {
                        push_label(
                            out,
                            &layout_value_text(Some(region.size)),
                            point,
                            margin.max(0.0),
                        );
                    }
                }
            }
        });
    }

    /// Label, painted color and detail of a venn region.
    pub(super) fn venn_item_tooltip(
        &self,
        series: &LayoutSeriesEntry,
        sets: &[VennSet],
        row: usize,
    ) -> Option<(String, Color, Option<String>)> {
        let label = region_label(sets, row)?;
        let color = mosaic::segment_color(series.color, slot(sets, row));
        Some((label, color, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLOT: GeneralPlotRect = GeneralPlotRect {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 300.0,
    };

    fn region(sets: &[&str], size: f64) -> VennSet {
        VennSet {
            sets: sets.iter().map(|name| (*name).to_owned()).collect(),
            size,
            label: None,
        }
    }

    #[test]
    fn two_set_overlap_is_exact() {
        for &(a, b, overlap) in &[(10.0, 10.0, 3.0), (8.0, 2.0, 2.0), (5.0, 7.0, 0.0)] {
            let sets = [
                region(&["A"], a),
                region(&["B"], b),
                region(&["A", "B"], overlap),
            ];
            let cache = VennCache::new(&sets, PLOT);
            let [c0, c1] = [cache.circles[0], cache.circles[1]];
            let d = (c1.x - c0.x).hypot(c1.y - c0.y);
            let total_a = a + overlap;
            let scale = c0.r / circle_radius(total_a);
            let want = overlap * scale * scale;
            let got = lens_area(c0.r, c1.r, d);
            let tolerance = 1e-6 * want.max(1e-12);
            assert!(
                (got - want).abs() <= tolerance.max(1e-9),
                "overlap {overlap} of {a}/{b}: got {got}, want {want}"
            );
        }
    }

    #[test]
    fn three_set_fit_error_is_small_on_a_feasible_fixture() {
        // A feasible fixture: the exact exclusive regions of a known circle layout, so a
        // perfect fit exists and the bounded search must find it.
        let circles = [
            VennCircle {
                x: 0.0,
                y: 0.0,
                r: 1.0,
            },
            VennCircle {
                x: 1.2,
                y: 0.0,
                r: 1.0,
            },
            VennCircle {
                x: 0.6,
                y: 1.0,
                r: 0.9,
            },
        ];
        let size = |mask: u8| region_estimate(circles, mask);
        let sets = [
            region(&["A"], size(0b001)),
            region(&["B"], size(0b010)),
            region(&["C"], size(0b100)),
            region(&["A", "B"], size(0b011)),
            region(&["A", "C"], size(0b101)),
            region(&["B", "C"], size(0b110)),
            region(&["A", "B", "C"], size(0b111)),
        ];
        let cache = VennCache::new(&sets, PLOT);
        assert!(cache.fit_error < 1e-3, "fit error {}", cache.fit_error);
    }

    #[test]
    fn triple_area_matches_numeric_integration() {
        let circles = [
            VennCircle {
                x: 0.0,
                y: 0.0,
                r: 1.0,
            },
            VennCircle {
                x: 1.0,
                y: 0.0,
                r: 1.0,
            },
            VennCircle {
                x: 0.5,
                y: 0.9,
                r: 1.1,
            },
        ];
        let exact = triple_area(circles);
        let (mut inside, mut total) = (0usize, 0usize);
        let steps = 400;
        for gx in 0..steps {
            for gy in 0..steps {
                let x = -1.2 + 2.4 * (gx as f64 + 0.5) / steps as f64;
                let y = -1.2 + 2.4 * (gy as f64 + 0.5) / steps as f64;
                total += 1;
                if circles.iter().all(|c| (x - c.x).hypot(y - c.y) <= c.r) {
                    inside += 1;
                }
            }
        }
        let area = inside as f64 / total as f64 * 2.4 * 2.4;
        assert!((exact - area).abs() < 0.01, "exact {exact}, sampled {area}");
    }

    #[test]
    fn every_region_hit_matches_its_membership_and_anchors_hit_back() {
        let sets = [
            region(&["A"], 8.0),
            region(&["B"], 6.0),
            region(&["A", "B"], 4.0),
        ];
        let cache = VennCache::new(&sets, PLOT);
        for row in 0..sets.len() {
            let (x, y) = cache.anchor(row).expect("region anchor");
            assert_eq!(
                cache.hit(x, y),
                Some((GeneralLayoutElement::Region, row)),
                "row {row}"
            );
        }
        assert_eq!(cache.hit(1.0, 1.0), None);
    }

    #[test]
    fn validation_rejects_duplicate_regions_missing_singletons_and_bad_sizes() {
        assert!(validate(&[]).is_err());
        assert!(validate(&[region(&["A"], 1.0), region(&["A"], 2.0)]).is_err());
        assert!(validate(&[region(&["A"], 1.0), region(&["A", "B"], 0.5)]).is_err());
        assert!(validate(&[region(&["A"], -1.0)]).is_err());
        assert!(validate(&[region(&["A"], f64::NAN)]).is_err());
        assert!(validate(&[region(&["A"], 1.0), region(&["B"], 1.0)]).is_ok());
        let nine: Vec<VennSet> = (0..9)
            .map(|index| region(&[&format!("S{index}")], 1.0))
            .collect();
        assert!(validate(&nine).is_err());
    }
}
