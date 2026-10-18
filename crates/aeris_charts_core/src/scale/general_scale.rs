//! Host-neutral Cartesian and polar scales for non-financial series.
//!
//! These scales deliberately operate on scalar or category-index domains. Category labels remain
//! caller-owned, so constructing a scale does not duplicate host strings or allocate category
//! state. They are not wired into the financial pane path.

/// Hard limit for one linear-axis tick calculation.
pub const MAX_GENERAL_TICKS: usize = 512;
pub const DEFAULT_SYMLOG_CONSTANT: f64 = 1.0;

const MAX_EXACT_INTEGER: u128 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleError {
    NonFinite,
    DegenerateDomain,
    NonPositiveDomain,
    InvalidConstant,
    InvalidPadding,
    InvalidAlign,
    CategoryCountTooLarge,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogScale {
    domain_from: f64,
    domain_to: f64,
    range_from: f64,
    range_to: f64,
}

impl LogScale {
    pub fn new(
        domain_from: f64,
        domain_to: f64,
        range_from: f64,
        range_to: f64,
    ) -> Result<Self, ScaleError> {
        validate_domain(domain_from, domain_to)?;
        validate_range(range_from, range_to)?;
        if domain_from <= 0.0 || domain_to <= 0.0 {
            return Err(ScaleError::NonPositiveDomain);
        }
        Ok(Self {
            domain_from,
            domain_to,
            range_from,
            range_to,
        })
    }

    pub fn coordinate(&self, value: f64) -> Option<f64> {
        if !value.is_finite() || value <= 0.0 {
            return None;
        }
        let from = self.domain_from.ln();
        let to = self.domain_to.ln();
        let unit = (value.ln() - from) / (to - from);
        let coordinate = self.range_from + unit * (self.range_to - self.range_from);
        coordinate.is_finite().then_some(normalize_zero(coordinate))
    }

    pub fn invert(&self, coordinate: f64) -> Option<f64> {
        if !coordinate.is_finite() || self.range_from == self.range_to {
            return None;
        }
        let unit = (coordinate - self.range_from) / (self.range_to - self.range_from);
        let from = self.domain_from.ln();
        let to = self.domain_to.ln();
        let value = (from + unit * (to - from)).exp();
        (value.is_finite() && value > 0.0).then_some(value)
    }

    pub fn ticks(&self, target_count: usize) -> Vec<f64> {
        if target_count == 0 {
            return Vec::new();
        }
        let low = self.domain_from.min(self.domain_to);
        let high = self.domain_from.max(self.domain_to);
        let first_exp = low.log10().floor() as i32;
        let last_exp = high.log10().ceil() as i32;
        let candidates = (last_exp - first_exp + 1).max(1) as usize * 3;
        let stride = candidates
            .div_ceil(target_count.min(MAX_GENERAL_TICKS))
            .max(1);
        let mut ticks = Vec::with_capacity(target_count.min(MAX_GENERAL_TICKS));
        let mut candidate_index = 0usize;
        for exp in first_exp..=last_exp {
            let power = 10.0_f64.powi(exp);
            if !power.is_finite() || power <= 0.0 {
                continue;
            }
            for factor in [1.0, 2.0, 5.0] {
                let value = factor * power;
                if value >= low
                    && value <= high
                    && candidate_index.is_multiple_of(stride)
                    && ticks.len() < MAX_GENERAL_TICKS
                {
                    ticks.push(value);
                }
                candidate_index = candidate_index.saturating_add(1);
            }
        }
        if ticks.is_empty() {
            ticks.push(low);
            if high != low && ticks.len() < target_count.min(MAX_GENERAL_TICKS) {
                ticks.push(high);
            }
        }
        if self.domain_from > self.domain_to {
            ticks.reverse();
        }
        ticks
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SymLogScale {
    domain_from: f64,
    domain_to: f64,
    range_from: f64,
    range_to: f64,
    constant: f64,
}

impl SymLogScale {
    pub fn new(
        domain_from: f64,
        domain_to: f64,
        range_from: f64,
        range_to: f64,
        constant: f64,
    ) -> Result<Self, ScaleError> {
        validate_domain(domain_from, domain_to)?;
        validate_range(range_from, range_to)?;
        if !constant.is_finite() || constant <= 0.0 {
            return Err(ScaleError::InvalidConstant);
        }
        Ok(Self {
            domain_from,
            domain_to,
            range_from,
            range_to,
            constant,
        })
    }

    fn transform(&self, value: f64) -> Option<f64> {
        if !value.is_finite() {
            return None;
        }
        Some(value.signum() * (1.0 + value.abs() / self.constant).ln())
    }

    fn untransform(&self, value: f64) -> Option<f64> {
        if !value.is_finite() {
            return None;
        }
        let result = value.signum() * self.constant * (value.abs().exp() - 1.0);
        result.is_finite().then_some(normalize_zero(result))
    }

    pub fn coordinate(&self, value: f64) -> Option<f64> {
        let from = self.transform(self.domain_from)?;
        let to = self.transform(self.domain_to)?;
        let transformed = self.transform(value)?;
        let unit = (transformed - from) / (to - from);
        let coordinate = self.range_from + unit * (self.range_to - self.range_from);
        coordinate.is_finite().then_some(normalize_zero(coordinate))
    }

    pub fn invert(&self, coordinate: f64) -> Option<f64> {
        if !coordinate.is_finite() || self.range_from == self.range_to {
            return None;
        }
        let from = self.transform(self.domain_from)?;
        let to = self.transform(self.domain_to)?;
        let unit = (coordinate - self.range_from) / (self.range_to - self.range_from);
        self.untransform(from + unit * (to - from))
    }

    /// Zero plus signed 1/2/5 decade ticks from the linear threshold outward, falling back to
    /// decades only and then every n-th decade (kept from the outermost) to honor the density
    /// hint. Decades-only ticks skip a decade closer to zero than half the decade spacing,
    /// where the log compression would crowd its label into zero's. A domain inside the linear
    /// threshold uses plain linear ticks.
    pub fn ticks(&self, target_count: usize) -> Vec<f64> {
        if target_count == 0 {
            return Vec::new();
        }
        let budget = target_count.min(MAX_GENERAL_TICKS);
        let low = self.domain_from.min(self.domain_to);
        let high = self.domain_from.max(self.domain_to);
        let magnitude = low.abs().max(high.abs());
        let first_exp = self.constant.log10().floor() as i32;
        let last_exp = magnitude.log10().floor() as i32;
        let decades = || (first_exp..=last_exp).rev();
        let spans_zero = low <= 0.0 && high >= 0.0;
        let collect = |factors: &[f64], stride: usize| {
            let min_from_zero = if spans_zero && factors.len() == 1 {
                0.5 * stride as f64 * std::f64::consts::LN_10
            } else {
                0.0
            };
            let mut ticks = Vec::new();
            if spans_zero {
                ticks.push(0.0);
            }
            for (index, exp) in decades().enumerate() {
                if !index.is_multiple_of(stride) {
                    continue;
                }
                let power = 10.0_f64.powi(exp);
                for factor in factors {
                    let value = factor * power;
                    if !value.is_finite()
                        || value < self.constant
                        || self.transform(value).is_none_or(|t| t < min_from_zero)
                    {
                        continue;
                    }
                    for signed in [value, -value] {
                        if signed >= low && signed <= high {
                            ticks.push(signed);
                        }
                    }
                }
            }
            ticks.sort_by(f64::total_cmp);
            ticks
        };
        let mut ticks = collect(&[1.0, 2.0, 5.0], 1);
        if ticks.len() > budget {
            ticks = collect(&[1.0], 1);
        }
        let mut stride = 2;
        while ticks.len() > budget && stride <= decades().count() {
            ticks = collect(&[1.0], stride);
            stride += 1;
        }
        if ticks.len() < 2 {
            return LinearScale::new(self.domain_from, self.domain_to, 0.0, 1.0)
                .map(|linear| linear.ticks(target_count))
                .unwrap_or_default();
        }
        ticks.truncate(MAX_GENERAL_TICKS);
        if self.domain_from > self.domain_to {
            ticks.reverse();
        }
        ticks
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearScale {
    domain_from: f64,
    domain_to: f64,
    range_from: f64,
    range_to: f64,
}

impl LinearScale {
    pub fn new(
        domain_from: f64,
        domain_to: f64,
        range_from: f64,
        range_to: f64,
    ) -> Result<Self, ScaleError> {
        validate_domain(domain_from, domain_to)?;
        validate_range(range_from, range_to)?;
        Ok(Self {
            domain_from,
            domain_to,
            range_from,
            range_to,
        })
    }

    pub fn domain(&self) -> (f64, f64) {
        (self.domain_from, self.domain_to)
    }

    pub fn range(&self) -> (f64, f64) {
        (self.range_from, self.range_to)
    }

    /// Replace both domain endpoints atomically.
    pub fn set_domain(&mut self, from: f64, to: f64) -> Result<(), ScaleError> {
        validate_domain(from, to)?;
        self.domain_from = from;
        self.domain_to = to;
        Ok(())
    }

    /// Replace both output-range endpoints atomically. A zero-length range is valid during layout.
    pub fn set_range(&mut self, from: f64, to: f64) -> Result<(), ScaleError> {
        validate_range(from, to)?;
        self.range_from = from;
        self.range_to = to;
        Ok(())
    }

    pub fn coordinate(&self, value: f64) -> Option<f64> {
        let unit = normalized_unit(value, self.domain_from, self.domain_to)?;
        let coordinate = self.range_from + unit * (self.range_to - self.range_from);
        coordinate.is_finite().then_some(normalize_zero(coordinate))
    }

    pub fn coordinate_clamped(&self, value: f64) -> Option<f64> {
        let low = self.domain_from.min(self.domain_to);
        let high = self.domain_from.max(self.domain_to);
        self.coordinate(value.clamp(low, high))
    }

    /// Convert a coordinate back into the domain. A collapsed layout range has no inverse.
    pub fn invert(&self, coordinate: f64) -> Option<f64> {
        if !coordinate.is_finite() || self.range_from == self.range_to {
            return None;
        }
        let unit = normalized_unit(coordinate, self.range_from, self.range_to)?;
        interpolate_unit(self.domain_from, self.domain_to, unit)
    }

    /// Generate deterministic 1/2/5 ticks. `target_count` is a density hint, not a promise.
    pub fn ticks(&self, target_count: usize) -> Vec<f64> {
        if target_count == 0 {
            return Vec::new();
        }

        let low = self.domain_from.min(self.domain_to);
        let high = self.domain_from.max(self.domain_to);
        // Leave room for both ends when the chosen step divides the domain exactly.
        let count = target_count.min(MAX_GENERAL_TICKS - 1);
        let span = high - low;
        let raw_step = if span.is_finite() {
            span / count as f64
        } else {
            let scaled_step = high / count as f64 - low / count as f64;
            if scaled_step.is_finite() {
                scaled_step
            } else {
                f64::MAX
            }
        };
        let step = nice_step(raw_step);
        if !step.is_finite() || step <= 0.0 {
            return Vec::new();
        }

        let quotient = low / step;
        let mut tick = quotient.ceil() * step;
        if !tick.is_finite() {
            return Vec::new();
        }

        let tolerance = step.abs() * 1e-12;
        let mut ticks = Vec::with_capacity(count.saturating_add(1).min(MAX_GENERAL_TICKS));
        while ticks.len() < MAX_GENERAL_TICKS {
            if tick > high && tick - high > tolerance {
                break;
            }
            let value = normalize_zero(tick);
            if ticks.last().is_none_or(|previous| *previous != value) {
                ticks.push(value);
            }
            let next = tick + step;
            if !next.is_finite() || next <= tick {
                break;
            }
            tick = next;
        }

        if self.domain_from > self.domain_to {
            ticks.reverse();
        }
        ticks
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandScale {
    count: usize,
    range_from: f64,
    range_to: f64,
    padding_inner: f64,
    padding_outer: f64,
    align: f64,
}

impl BandScale {
    pub fn new(
        count: usize,
        range_from: f64,
        range_to: f64,
        padding_inner: f64,
        padding_outer: f64,
        align: f64,
    ) -> Result<Self, ScaleError> {
        validate_category_inputs(
            count,
            range_from,
            range_to,
            padding_inner,
            padding_outer,
            align,
        )?;
        Ok(Self {
            count,
            range_from,
            range_to,
            padding_inner,
            padding_outer,
            align,
        })
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn step(&self) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        let span = (self.range_to - self.range_from).abs();
        let denominator =
            (self.count as f64 - self.padding_inner + 2.0 * self.padding_outer).max(1.0);
        span / denominator
    }

    pub fn bandwidth(&self) -> f64 {
        self.step() * (1.0 - self.padding_inner)
    }

    pub fn center(&self, index: usize) -> Option<f64> {
        if index >= self.count {
            return None;
        }
        let step = self.step();
        let bandwidth = self.bandwidth();
        let span = (self.range_to - self.range_from).abs();
        let occupied = step * (self.count as f64 - self.padding_inner);
        let offset = (span - occupied) * self.align + bandwidth * 0.5 + index as f64 * step;
        Some(normalize_zero(project_from_start(
            self.range_from,
            self.range_to,
            offset,
        )))
    }

    /// Return ascending coordinate bounds regardless of range direction.
    pub fn bounds(&self, index: usize) -> Option<(f64, f64)> {
        let center = self.center(index)?;
        let half = self.bandwidth() * 0.5;
        Some((normalize_zero(center - half), normalize_zero(center + half)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointScale {
    count: usize,
    range_from: f64,
    range_to: f64,
    padding: f64,
    align: f64,
}

impl PointScale {
    pub fn new(
        count: usize,
        range_from: f64,
        range_to: f64,
        padding: f64,
        align: f64,
    ) -> Result<Self, ScaleError> {
        validate_category_inputs(count, range_from, range_to, 1.0, padding, align)?;
        Ok(Self {
            count,
            range_from,
            range_to,
            padding,
            align,
        })
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn step(&self) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        let span = (self.range_to - self.range_from).abs();
        let denominator = (self.count.saturating_sub(1) as f64 + 2.0 * self.padding).max(1.0);
        span / denominator
    }

    pub fn coordinate(&self, index: usize) -> Option<f64> {
        if index >= self.count {
            return None;
        }
        let step = self.step();
        let span = (self.range_to - self.range_from).abs();
        let occupied = step * self.count.saturating_sub(1) as f64;
        let offset = (span - occupied) * self.align + index as f64 * step;
        Some(normalize_zero(project_from_start(
            self.range_from,
            self.range_to,
            offset,
        )))
    }
}

/// Equal angular category slots around a complete circle. Category labels remain engine-owned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AngularCategoryScale {
    count: usize,
    start_angle: f64,
    direction: f64,
}

impl AngularCategoryScale {
    pub fn new(count: usize, start_angle: f64, clockwise: bool) -> Result<Self, ScaleError> {
        if count == 0 {
            return Err(ScaleError::DegenerateDomain);
        }
        if count > 65_536 {
            return Err(ScaleError::CategoryCountTooLarge);
        }
        if !start_angle.is_finite() {
            return Err(ScaleError::NonFinite);
        }
        Ok(Self {
            count,
            start_angle: start_angle.rem_euclid(std::f64::consts::TAU),
            direction: if clockwise { 1.0 } else { -1.0 },
        })
    }

    pub fn count(self) -> usize {
        self.count
    }

    /// Unwrapped angular bounds keep the final slot contiguous at the circle seam.
    pub fn sector(self, index: usize) -> Option<(f64, f64)> {
        if index >= self.count {
            return None;
        }
        let step = self.direction * std::f64::consts::TAU / self.count as f64;
        Some((
            self.start_angle + step * index as f64,
            self.start_angle + step * (index + 1) as f64,
        ))
    }

    pub fn center(self, index: usize) -> Option<f64> {
        let (start, end) = self.sector(index)?;
        Some((start + end) / 2.0)
    }

    /// The start boundary belongs to the following slot in drawing order.
    pub fn index_at(self, angle: f64) -> Option<usize> {
        if !angle.is_finite() {
            return None;
        }
        let advance =
            ((angle - self.start_angle) * self.direction).rem_euclid(std::f64::consts::TAU);
        Some(
            ((advance / std::f64::consts::TAU * self.count as f64).floor() as usize)
                .min(self.count - 1),
        )
    }
}

/// Media-space polar plot. Angle zero points up and positive angles advance clockwise.
/// Geometry stays in `f64`; executors encode device pixels only after frame construction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolarPlot {
    center_x: f64,
    center_y: f64,
    outer_radius: f64,
}

/// A bounded annular sector in media space. The same sweep owns fill geometry and hit testing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolarSector {
    plot: PolarPlot,
    start: f64,
    sweep: f64,
    inner: f64,
    outer: f64,
    segments: usize,
}

impl PolarSector {
    /// `sweep` may be clockwise or counterclockwise and may cover at most one turn.
    pub fn new(
        plot: PolarPlot,
        start: f64,
        sweep: f64,
        inner: f64,
        outer: f64,
    ) -> Result<Self, ScaleError> {
        if ![start, sweep, inner, outer]
            .iter()
            .all(|value| value.is_finite())
        {
            return Err(ScaleError::NonFinite);
        }
        if sweep == 0.0
            || sweep.abs() > std::f64::consts::TAU
            || inner < 0.0
            || outer > 1.0
            || inner >= outer
        {
            return Err(ScaleError::DegenerateDomain);
        }
        let radius = plot.outer_radius * outer;
        // A quarter media pixel of sagitta bounds visible arc error until the per-sector
        // work cap takes over. The cap also bounds a full-circle mesh to 1,024 triangles.
        let step = if radius <= 0.25 {
            sweep.abs()
        } else {
            2.0 * (1.0 - 0.25 / radius).acos()
        };
        let chord_segments = (sweep.abs() / step).ceil() as usize;
        let turn_segments = (sweep.abs() / (std::f64::consts::TAU / 8.0)).ceil() as usize;
        let segments = chord_segments.max(turn_segments).clamp(1, 512);
        Ok(Self {
            plot,
            start: start.rem_euclid(std::f64::consts::TAU),
            sweep,
            inner,
            outer,
            segments,
        })
    }

    pub fn triangle_count(self) -> usize {
        self.segments * if self.inner == 0.0 { 1 } else { 2 }
    }

    pub fn mid_angle(self) -> f64 {
        self.start + self.sweep / 2.0
    }

    pub fn mid_radius(self) -> f64 {
        (self.inner + self.outer) / 2.0
    }

    /// Validated inner and outer fractions of the plot radius, shared by leaders and fills.
    pub fn radial_bounds(self) -> (f64, f64) {
        (self.inner, self.outer)
    }

    pub fn plot(self) -> PolarPlot {
        self.plot
    }

    /// Visits a nonoverlapping triangle mesh without retaining a path or allocating points.
    pub fn visit_triangles(self, mut visit: impl FnMut([(f64, f64); 3])) {
        let center = self.plot.center();
        for index in 0..self.segments {
            let angle0 = self.start + self.sweep * index as f64 / self.segments as f64;
            let angle1 = self.start + self.sweep * (index + 1) as f64 / self.segments as f64;
            let outer0 = self
                .plot
                .point(angle0, self.outer)
                .expect("validated sector");
            let outer1 = self
                .plot
                .point(angle1, self.outer)
                .expect("validated sector");
            if self.inner == 0.0 {
                visit([center, outer0, outer1]);
            } else {
                let inner0 = self
                    .plot
                    .point(angle0, self.inner)
                    .expect("validated sector");
                let inner1 = self
                    .plot
                    .point(angle1, self.inner)
                    .expect("validated sector");
                visit([inner0, outer0, outer1]);
                visit([inner0, outer1, inner1]);
            }
        }
    }

    /// Start edge is included; the end edge belongs to the next sector.
    pub fn contains(self, x: f64, y: f64) -> bool {
        let Some((angle, radius)) = self.plot.invert(x, y) else {
            return false;
        };
        if radius < self.inner || radius > self.outer {
            return false;
        }
        if self.sweep.abs() == std::f64::consts::TAU {
            return true;
        }
        let advance =
            ((angle - self.start) * self.sweep.signum()).rem_euclid(std::f64::consts::TAU);
        advance < self.sweep.abs()
    }
}

/// A closed radial polygon with one radius for each angular category.
/// Static equal-angle polygons keep constant-work hits; moving vertices use the emitted fan.
#[derive(Clone, Copy, Debug)]
pub struct PolarPolygon<'a> {
    plot: PolarPlot,
    angles: AngularCategoryScale,
    moving_sectors: Option<&'a [(f64, f64)]>,
    ordered_sectors: bool,
    radii: &'a [f64],
    has_area: bool,
}

impl<'a> PolarPolygon<'a> {
    pub fn vertex_count(self) -> usize {
        self.radii.len()
    }

    pub fn plot(self) -> PolarPlot {
        self.plot
    }

    pub fn vertex(self, index: usize) -> Option<(f64, f64)> {
        self.plot
            .point(self.vertex_angle(index)?, *self.radii.get(index)?)
    }

    pub fn angle(self, index: usize) -> Option<f64> {
        self.vertex_angle(index)
    }

    pub fn category_at(self, x: f64, y: f64) -> Option<usize> {
        let (angle, _) = self.plot.invert(x, y)?;
        if self.moving_sectors.is_none() {
            return self.angles.index_at(angle);
        }
        if self.ordered_sectors {
            let mut low = 0;
            let mut high = self.radii.len();
            while low < high {
                let middle = (low + high) / 2;
                if self.vertex_angle(middle).unwrap() <= angle {
                    low = middle + 1;
                } else {
                    high = middle;
                }
            }
            let before = low.checked_sub(1).unwrap_or(self.radii.len() - 1);
            let after = low % self.radii.len();
            let distance = |index| {
                let delta =
                    (angle - self.vertex_angle(index).unwrap()).rem_euclid(std::f64::consts::TAU);
                delta.min(std::f64::consts::TAU - delta)
            };
            return Some(if distance(before) <= distance(after) {
                before
            } else {
                after
            });
        }
        (0..self.radii.len()).min_by(|&a, &b| {
            let distance = |index| {
                let delta =
                    (angle - self.vertex_angle(index).unwrap()).rem_euclid(std::f64::consts::TAU);
                delta.min(std::f64::consts::TAU - delta)
            };
            distance(a).total_cmp(&distance(b))
        })
    }

    pub fn new(
        plot: PolarPlot,
        angles: AngularCategoryScale,
        radii: &'a [f64],
    ) -> Result<Self, ScaleError> {
        if radii.len() != angles.count() || radii.len() < 3 {
            return Err(ScaleError::DegenerateDomain);
        }
        if radii.iter().any(|radius| !radius.is_finite()) {
            return Err(ScaleError::NonFinite);
        }
        if radii.iter().any(|radius| !(0.0..=1.0).contains(radius)) {
            return Err(ScaleError::DegenerateDomain);
        }
        let has_area = radii
            .iter()
            .enumerate()
            .any(|(index, value)| *value > 0.0 && radii[(index + 1) % radii.len()] > 0.0);
        Ok(Self {
            plot,
            angles,
            moving_sectors: None,
            ordered_sectors: false,
            radii,
            has_area,
        })
    }

    /// Uses sampled sector bounds while category spokes are moving.
    pub fn new_with_sectors(
        plot: PolarPlot,
        angles: AngularCategoryScale,
        sectors: &'a [(f64, f64)],
        radii: &'a [f64],
    ) -> Result<Self, ScaleError> {
        if sectors.len() != angles.count()
            || sectors
                .iter()
                .any(|(first, second)| !first.is_finite() || !second.is_finite())
        {
            return Err(ScaleError::DegenerateDomain);
        }
        let mut polygon = Self::new(plot, angles, radii)?;
        polygon.moving_sectors = Some(sectors);
        polygon.ordered_sectors = sectors.windows(2).all(|pair| {
            let a = (pair[0].0 + pair[0].1) / 2.0;
            let b = (pair[1].0 + pair[1].1) / 2.0;
            a >= 0.0 && b < std::f64::consts::TAU && a < b
        });
        Ok(polygon)
    }

    fn vertex_angle(self, index: usize) -> Option<f64> {
        self.moving_sectors
            .map(|sectors| {
                sectors
                    .get(index)
                    .map(|(first, second)| (first + second) / 2.0)
            })
            .unwrap_or_else(|| self.angles.center(index))
    }

    /// Radius of the filled fan boundary on a ray, in plot-radius units.
    pub fn radius_at(self, angle: f64) -> Option<f64> {
        if !angle.is_finite() {
            return None;
        }
        let angle = angle.rem_euclid(std::f64::consts::TAU);
        let direction = (angle.sin(), -angle.cos());
        let count = self.radii.len();
        let edge = |index: usize| {
            let next = (index + 1) % count;
            let first = self.vertex_angle(index)?;
            let second = self.vertex_angle(next)?;
            let a = (
                self.radii[index] * first.sin(),
                -self.radii[index] * first.cos(),
            );
            let b = (
                self.radii[next] * second.sin(),
                -self.radii[next] * second.cos(),
            );
            ray_edge_radius(direction, a, b)
        };
        if self.moving_sectors.is_none() {
            let first = self.angles.center(0)?;
            let step = self.angles.sector(0)?.1 - self.angles.sector(0)?.0;
            let advance = ((angle - first) * step.signum()).rem_euclid(std::f64::consts::TAU);
            let index = ((advance / step.abs()).floor() as usize).min(count - 1);
            for candidate in [index, (index + 1) % count] {
                let vertex = self
                    .vertex_angle(candidate)?
                    .rem_euclid(std::f64::consts::TAU);
                let gap = (angle - vertex).abs();
                if gap.min(std::f64::consts::TAU - gap) < 1e-12 {
                    return Some(self.radii[candidate]);
                }
            }
            return Some(edge(index).unwrap_or(0.0));
        }
        if self.ordered_sectors {
            let mut low = 0;
            let mut high = count;
            while low < high {
                let middle = (low + high) / 2;
                if self.vertex_angle(middle).unwrap() <= angle {
                    low = middle + 1;
                } else {
                    high = middle;
                }
            }
            let before = low.checked_sub(1).unwrap_or(count - 1);
            for candidate in [before, low % count] {
                let vertex = self
                    .vertex_angle(candidate)?
                    .rem_euclid(std::f64::consts::TAU);
                let gap = (angle - vertex).abs();
                if gap.min(std::f64::consts::TAU - gap) < 1e-12 {
                    return Some(self.radii[candidate]);
                }
            }
            return Some(edge(before).unwrap_or(0.0));
        }
        Some((0..count).filter_map(edge).fold(0.0, f64::max))
    }

    pub fn visit_triangles(self, mut visit: impl FnMut([(f64, f64); 3])) {
        let center = self.plot.center();
        for index in 0..self.radii.len() {
            let next = (index + 1) % self.radii.len();
            let a = self
                .plot
                .point(
                    self.vertex_angle(index).expect("validated category"),
                    self.radii[index],
                )
                .expect("validated radius");
            let b = self
                .plot
                .point(
                    self.vertex_angle(next).expect("validated category"),
                    self.radii[next],
                )
                .expect("validated radius");
            visit([center, a, b]);
        }
    }

    /// Uses the same center fan as the fill, evaluated in unit coordinates to avoid overflow.
    pub fn contains(self, x: f64, y: f64) -> bool {
        let Some((angle, radius)) = self.plot.invert(x, y) else {
            return false;
        };
        if radius > 1.0 {
            return false;
        }
        if radius == 0.0 {
            if self.ordered_sectors {
                return self.has_area;
            }
            if self.moving_sectors.is_some() {
                return (0..self.radii.len()).any(|index| {
                    let next = (index + 1) % self.radii.len();
                    let a = self.radii[index];
                    let b = self.radii[next];
                    a > 0.0
                        && b > 0.0
                        && (self.vertex_angle(next).unwrap() - self.vertex_angle(index).unwrap())
                            .sin()
                            .abs()
                            > 1e-12
                });
            }
            return self.has_area;
        }
        if self.ordered_sectors {
            return radius <= self.radius_at(angle).unwrap_or(0.0) + 1e-12;
        }
        let point = (radius * angle.sin(), -radius * angle.cos());
        if self.moving_sectors.is_some() {
            return (0..self.radii.len()).any(|index| {
                let next = (index + 1) % self.radii.len();
                let a_angle = self.vertex_angle(index).unwrap();
                let b_angle = self.vertex_angle(next).unwrap();
                let a = (
                    self.radii[index] * a_angle.sin(),
                    -self.radii[index] * a_angle.cos(),
                );
                let b = (
                    self.radii[next] * b_angle.sin(),
                    -self.radii[next] * b_angle.cos(),
                );
                fan_triangle_contains(point, a, b)
            });
        }
        // Equal-angle vertices partition the disk into disjoint center-fan wedges, so only
        // one triangle can contain a noncentral pointer. Dense radar hits remain O(1).
        let first_angle = self.angles.center(0).expect("validated category");
        let advance =
            ((angle - first_angle) * self.angles.direction).rem_euclid(std::f64::consts::TAU);
        let index = ((advance / std::f64::consts::TAU * self.radii.len() as f64).floor() as usize)
            .min(self.radii.len() - 1);
        let next = (index + 1) % self.radii.len();
        let a_angle = self.angles.center(index).expect("validated category");
        let b_angle = self.angles.center(next).expect("validated category");
        let a = (
            self.radii[index] * a_angle.sin(),
            -self.radii[index] * a_angle.cos(),
        );
        let b = (
            self.radii[next] * b_angle.sin(),
            -self.radii[next] * b_angle.cos(),
        );
        fan_triangle_contains(point, a, b)
    }
}

fn ray_edge_radius(direction: (f64, f64), a: (f64, f64), b: (f64, f64)) -> Option<f64> {
    let edge = (b.0 - a.0, b.1 - a.1);
    let denominator = direction.0 * edge.1 - direction.1 * edge.0;
    if denominator.abs() <= 1e-15 {
        return None;
    }
    let radius = (a.0 * edge.1 - a.1 * edge.0) / denominator;
    let along_edge = (a.0 * direction.1 - a.1 * direction.0) / denominator;
    (radius >= -1e-12 && (-1e-12..=1.0 + 1e-12).contains(&along_edge))
        .then_some(radius.clamp(0.0, 1.0))
}

fn fan_triangle_contains(point: (f64, f64), a: (f64, f64), b: (f64, f64)) -> bool {
    let cross_a = a.0 * point.1 - a.1 * point.0;
    let cross_b = point.0 * b.1 - point.1 * b.0;
    let cross_edge = (b.0 - a.0) * (point.1 - a.1) - (b.1 - a.1) * (point.0 - a.0);
    let orientation = a.0 * b.1 - a.1 * b.0;
    // Trig and inverse-trig roundoff can put a point on a shared ray a few ulps outside
    // either candidate wedge. This tolerance is in unit-radius coordinates.
    const EDGE_EPSILON: f64 = 1.0e-12;
    orientation > 0.0
        && cross_a >= -EDGE_EPSILON
        && cross_b >= -EDGE_EPSILON
        && cross_edge >= -EDGE_EPSILON
        || orientation < 0.0
            && cross_a <= EDGE_EPSILON
            && cross_b <= EDGE_EPSILON
            && cross_edge <= EDGE_EPSILON
}

impl PolarPlot {
    pub fn from_rect(x: f64, y: f64, width: f64, height: f64) -> Result<Self, ScaleError> {
        if ![x, y, width, height].iter().all(|value| value.is_finite()) {
            return Err(ScaleError::NonFinite);
        }
        if width <= 0.0 || height <= 0.0 {
            return Err(ScaleError::DegenerateDomain);
        }
        let center_x = x + width / 2.0;
        let center_y = y + height / 2.0;
        let outer_radius = width.min(height) / 2.0;
        if !center_x.is_finite()
            || !center_y.is_finite()
            || ![
                center_x - outer_radius,
                center_x + outer_radius,
                center_y - outer_radius,
                center_y + outer_radius,
            ]
            .iter()
            .all(|value| value.is_finite())
            || outer_radius <= 0.0
        {
            return Err(ScaleError::NonFinite);
        }
        Ok(Self {
            center_x,
            center_y,
            outer_radius,
        })
    }

    pub fn center(self) -> (f64, f64) {
        (self.center_x, self.center_y)
    }

    pub fn outer_radius(self) -> f64 {
        self.outer_radius
    }

    /// `radius_fraction` may exceed one for labels outside the plot.
    pub fn point(self, angle_radians: f64, radius_fraction: f64) -> Option<(f64, f64)> {
        if !angle_radians.is_finite() || !radius_fraction.is_finite() || radius_fraction < 0.0 {
            return None;
        }
        let radius = self.outer_radius * radius_fraction;
        let point = (
            self.center_x + radius * angle_radians.sin(),
            self.center_y - radius * angle_radians.cos(),
        );
        (point.0.is_finite() && point.1.is_finite()).then_some(point)
    }

    /// Returns a clockwise angle in `[0, 2π)` and a radius relative to the plot's outer edge.
    pub fn invert(self, x: f64, y: f64) -> Option<(f64, f64)> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let dx = x - self.center_x;
        let dy = y - self.center_y;
        let radius_fraction = dx.hypot(dy) / self.outer_radius;
        if !radius_fraction.is_finite() {
            return None;
        }
        if radius_fraction == 0.0 {
            return Some((0.0, 0.0));
        }
        let angle = dx.atan2(-dy).rem_euclid(std::f64::consts::TAU);
        Some((angle, radius_fraction))
    }
}

fn validate_domain(from: f64, to: f64) -> Result<(), ScaleError> {
    if !from.is_finite() || !to.is_finite() {
        return Err(ScaleError::NonFinite);
    }
    if from == to {
        return Err(ScaleError::DegenerateDomain);
    }
    Ok(())
}

fn validate_range(from: f64, to: f64) -> Result<(), ScaleError> {
    if !from.is_finite() || !to.is_finite() || !(to - from).is_finite() {
        return Err(ScaleError::NonFinite);
    }
    Ok(())
}

fn validate_category_inputs(
    count: usize,
    range_from: f64,
    range_to: f64,
    padding_inner: f64,
    padding_outer: f64,
    align: f64,
) -> Result<(), ScaleError> {
    validate_range(range_from, range_to)?;
    if count as u128 > MAX_EXACT_INTEGER {
        return Err(ScaleError::CategoryCountTooLarge);
    }
    if !padding_inner.is_finite()
        || !padding_outer.is_finite()
        || !(0.0..=1.0).contains(&padding_inner)
        || !(0.0..=1.0).contains(&padding_outer)
    {
        return Err(ScaleError::InvalidPadding);
    }
    if !align.is_finite() || !(0.0..=1.0).contains(&align) {
        return Err(ScaleError::InvalidAlign);
    }
    Ok(())
}

/// Extend a linear domain outward to the tightest multiples of a 1/2/5 step that cover it in at
/// most `target_intervals` intervals, so both ends land on clean tick values. Non-finite or
/// degenerate input, or an extension that would overflow, returns the ascending input unchanged.
pub fn nice_linear_domain(from: f64, to: f64, target_intervals: usize) -> [f64; 2] {
    let low = from.min(to);
    let high = from.max(to);
    if target_intervals == 0 || !low.is_finite() || !high.is_finite() || low >= high {
        return [low, high];
    }
    let count = target_intervals.min(MAX_GENERAL_TICKS - 1) as f64;
    let mut step = nice_step((high - low) / count);
    // The first candidate is at least span / count, so a few larger 1/2/5 steps always suffice
    // once the outward rounding of both ends is accounted for.
    for _ in 0..4 {
        if !step.is_finite() || step <= 0.0 {
            break;
        }
        let first = (low / step).floor();
        let last = (high / step).ceil();
        let nice_low = first * step;
        let nice_high = last * step;
        if !nice_low.is_finite() || !nice_high.is_finite() || nice_low >= nice_high {
            break;
        }
        if last - first <= count {
            return [normalize_zero(nice_low), normalize_zero(nice_high)];
        }
        step = next_nice_step(step);
    }
    [low, high]
}

/// The next larger step in the 1, 2, 5, 10 sequence.
fn next_nice_step(step: f64) -> f64 {
    let power = 10.0_f64.powf(step.log10().floor());
    let mantissa = (step / power).round();
    let factor = if mantissa < 2.0 {
        2.0
    } else if mantissa < 5.0 {
        5.0
    } else {
        10.0
    };
    factor * power
}

fn nice_step(raw_step: f64) -> f64 {
    if !raw_step.is_finite() || raw_step <= 0.0 {
        return f64::NAN;
    }
    let power = 10.0_f64.powf(raw_step.log10().floor());
    if !power.is_finite() || power <= 0.0 {
        return raw_step;
    }
    let error = raw_step / power;
    // Round upward so the density hint can never expand beyond the hard output bound.
    let factor = if error <= 1.0 {
        1.0
    } else if error <= 2.0 {
        2.0
    } else if error <= 5.0 {
        5.0
    } else {
        10.0
    };
    let step = factor * power;
    if step.is_finite() { step } else { raw_step }
}

fn normalized_unit(value: f64, from: f64, to: f64) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    let span = to - from;
    let offset = value - from;
    let unit = if span.is_finite() && offset.is_finite() {
        offset / span
    } else {
        (value * 0.5 - from * 0.5) / (to * 0.5 - from * 0.5)
    };
    unit.is_finite().then_some(unit)
}

fn interpolate_unit(from: f64, to: f64, unit: f64) -> Option<f64> {
    let value = if unit == 0.0 {
        from
    } else if unit == 1.0 {
        to
    } else {
        from * (1.0 - unit) + to * unit
    };
    value.is_finite().then_some(normalize_zero(value))
}

fn project_from_start(range_from: f64, range_to: f64, offset: f64) -> f64 {
    if range_to >= range_from {
        range_from + offset
    } else {
        range_from - offset
    }
}

fn normalize_zero(value: f64) -> f64 {
    if value == 0.0 { 0.0 } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polar_plot_maps_clockwise_angles_and_inverts_media_coordinates() {
        let plot = PolarPlot::from_rect(10.0, 20.0, 200.0, 100.0).unwrap();
        assert_eq!(plot.center(), (110.0, 70.0));
        assert_eq!(plot.outer_radius(), 50.0);
        assert_eq!(plot.point(0.0, 1.0), Some((110.0, 20.0)));
        let right = plot.point(std::f64::consts::FRAC_PI_2, 1.0).unwrap();
        assert!((right.0 - 160.0).abs() < 1e-12);
        assert!((right.1 - 70.0).abs() < 1e-12);
        for angle in [0.0, 0.2, 1.5, 3.1, 5.8] {
            let (x, y) = plot.point(angle, 0.75).unwrap();
            let (back_angle, back_radius) = plot.invert(x, y).unwrap();
            assert!((back_angle - angle).abs() < 1e-12);
            assert!((back_radius - 0.75).abs() < 1e-12);
        }
        assert_eq!(plot.invert(110.0, 70.0), Some((0.0, 0.0)));
        assert!(PolarPlot::from_rect(0.0, 0.0, 0.0, 100.0).is_err());
        assert!(PolarPlot::from_rect(f64::MAX, 0.0, f64::MAX, 100.0).is_err());
        assert!(PolarPlot::from_rect(1.0e308, 0.0, 1.0e308, 1.0e308).is_err());
        assert_eq!(plot.point(f64::NAN, 1.0), None);
        assert_eq!(plot.point(0.0, -1.0), None);
    }

    #[test]
    fn angular_categories_share_sector_boundaries_with_pointer_inversion() {
        let clockwise = AngularCategoryScale::new(4, 0.0, true).unwrap();
        let counterclockwise = AngularCategoryScale::new(4, 0.0, false).unwrap();
        assert_eq!(
            clockwise.sector(0),
            Some((0.0, std::f64::consts::FRAC_PI_2))
        );
        assert_eq!(clockwise.center(0), Some(std::f64::consts::FRAC_PI_4));
        assert_eq!(clockwise.sector(4), None);
        for index in 0..4 {
            assert_eq!(
                clockwise.index_at(clockwise.center(index).unwrap()),
                Some(index)
            );
            assert_eq!(
                counterclockwise.index_at(counterclockwise.center(index).unwrap()),
                Some(index)
            );
        }
        assert_eq!(clockwise.index_at(std::f64::consts::TAU), Some(0));
        assert_eq!(clockwise.index_at(f64::NAN), None);
        assert!(AngularCategoryScale::new(0, 0.0, true).is_err());
        assert!(AngularCategoryScale::new(65_537, 0.0, true).is_err());
    }

    #[test]
    fn polar_sector_mesh_and_hits_share_clockwise_and_reverse_boundaries() {
        let plot = PolarPlot::from_rect(0.0, 0.0, 200.0, 200.0).unwrap();
        let first = PolarSector::new(plot, 0.0, std::f64::consts::FRAC_PI_2, 0.25, 1.0).unwrap();
        let next = PolarSector::new(
            plot,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
            0.25,
            1.0,
        )
        .unwrap();
        let boundary = plot.point(std::f64::consts::FRAC_PI_2, 0.5).unwrap();
        assert!(!first.contains(boundary.0, boundary.1));
        assert!(next.contains(boundary.0, boundary.1));
        assert!(!first.contains(100.0, 100.0));
        let mut triangles = 0;
        first.visit_triangles(|triangle| {
            triangles += 1;
            assert!(
                triangle
                    .iter()
                    .all(|point| point.0.is_finite() && point.1.is_finite())
            );
        });
        assert_eq!(triangles, first.triangle_count());
        assert!(triangles <= 1_024);
        let reverse = PolarSector::new(plot, 0.0, -std::f64::consts::FRAC_PI_2, 0.0, 1.0).unwrap();
        let inside = plot.point(7.0 * std::f64::consts::FRAC_PI_4, 0.5).unwrap();
        assert!(reverse.contains(inside.0, inside.1));
        assert!(!reverse.contains(boundary.0, boundary.1));
        assert!(PolarSector::new(plot, 0.0, 0.0, 0.0, 1.0).is_err());
        assert!(PolarSector::new(plot, 0.0, std::f64::consts::TAU, 1.0, 0.0).is_err());
        let tiny_plot = PolarPlot::from_rect(0.0, 0.0, 0.1, 0.1).unwrap();
        let full_circle =
            PolarSector::new(tiny_plot, 0.0, std::f64::consts::TAU, 0.0, 1.0).unwrap();
        assert!(full_circle.triangle_count() >= 8);
    }

    #[test]
    fn radial_polygon_fill_and_hits_share_a_concave_center_fan() {
        let plot = PolarPlot::from_rect(0.0, 0.0, 200.0, 200.0).unwrap();
        let angles = AngularCategoryScale::new(5, 0.0, true).unwrap();
        let radii = [1.0, 0.1, 1.0, 1.0, 1.0];
        let polygon = PolarPolygon::new(plot, angles, &radii).unwrap();
        for (index, radius) in radii.iter().enumerate() {
            close(
                polygon.radius_at(angles.center(index).unwrap()).unwrap(),
                *radius,
            );
        }
        let mut triangles = 0;
        polygon.visit_triangles(|triangle| {
            triangles += 1;
            assert!(
                triangle
                    .iter()
                    .all(|point| point.0.is_finite() && point.1.is_finite())
            );
        });
        assert_eq!(triangles, radii.len());
        assert!(polygon.contains(100.0, 100.0));
        let outer_tip = plot.point(angles.center(0).unwrap(), 0.9).unwrap();
        assert!(polygon.contains(outer_tip.0, outer_tip.1));
        let concavity = plot.point(angles.center(1).unwrap(), 0.5).unwrap();
        assert!(!polygon.contains(concavity.0, concavity.1));
        assert!(!polygon.contains(f64::NAN, 0.0));
        assert!(PolarPolygon::new(plot, angles, &[1.0, 1.0]).is_err());
        assert!(PolarPolygon::new(plot, angles, &[1.0, 1.0, f64::NAN, 1.0, 1.0]).is_err());
        let reverse_angles = AngularCategoryScale::new(65_536, 1.2, false).unwrap();
        let reverse_radii = vec![1.0; reverse_angles.count()];
        let reverse = PolarPolygon::new(plot, reverse_angles, &reverse_radii).unwrap();
        close(
            reverse
                .radius_at(reverse_angles.center(60_000).unwrap())
                .unwrap(),
            1.0,
        );
        let interior = plot
            .point(reverse_angles.center(60_000).unwrap(), 0.5)
            .unwrap();
        assert!(reverse.contains(interior.0, interior.1));
        let center_hole = [1.0, 0.0, 0.0, 0.0, 1.0];
        assert!(
            PolarPolygon::new(plot, angles, &center_hole)
                .unwrap()
                .contains(100.0, 100.0)
        );
        let empty = [0.0; 5];
        assert!(
            !PolarPolygon::new(plot, angles, &empty)
                .unwrap()
                .contains(100.0, 100.0)
        );
    }

    #[test]
    fn moving_radial_polygon_hits_the_same_fan_that_it_emits() {
        let plot = PolarPlot::from_rect(0.0, 0.0, 200.0, 200.0).unwrap();
        let angles = AngularCategoryScale::new(5, 0.0, true).unwrap();
        let sectors = [(0.0, 0.4), (1.5, 1.9), (3.5, 3.9), (2.7, 3.1), (5.1, 5.5)];
        let radii = [0.8; 5];
        let polygon = PolarPolygon::new_with_sectors(plot, angles, &sectors, &radii).unwrap();
        for sector in sectors {
            let angle = (sector.0 + sector.1) / 2.0;
            close(polygon.radius_at(angle).unwrap(), 0.8);
        }
        let mut count = 0;
        polygon.visit_triangles(|triangle| {
            count += 1;
            let point = (
                (triangle[0].0 + triangle[1].0 + triangle[2].0) / 3.0,
                (triangle[0].1 + triangle[1].1 + triangle[2].1) / 3.0,
            );
            assert!(polygon.contains(point.0, point.1));
        });
        assert_eq!(count, 5);
        let vertex = polygon.vertex(1).unwrap();
        assert_eq!(polygon.category_at(vertex.0, vertex.1), Some(1));
    }

    #[test]
    fn ordered_radial_samples_match_the_emitted_fan_for_hits() {
        let plot = PolarPlot::from_rect(0.0, 0.0, 200.0, 200.0).unwrap();
        let centers = [
            0.0,
            std::f64::consts::FRAC_PI_2,
            2.0 * std::f64::consts::PI / 3.0,
            std::f64::consts::PI,
            4.0 * std::f64::consts::PI / 3.0,
            3.0 * std::f64::consts::FRAC_PI_2,
        ];
        let sectors = centers.map(|angle| (angle, angle));
        let radii = [0.2, 0.5, 0.4, 0.7, 0.8, 0.6];
        let equal = AngularCategoryScale::new(centers.len(), 0.0, true).unwrap();
        let polygon = PolarPolygon::new_with_sectors(plot, equal, &sectors, &radii).unwrap();
        for (index, &angle) in centers.iter().enumerate() {
            close(polygon.radius_at(angle).unwrap(), radii[index]);
        }
        for angle_index in 0..360 {
            let angle = std::f64::consts::TAU * (angle_index as f64 + 0.37) / 360.0;
            for radius_index in 1..40 {
                let radius = (radius_index as f64 + 0.13) / 40.0;
                let point = plot.point(angle, radius).unwrap();
                let unit = (radius * angle.sin(), -radius * angle.cos());
                let expected = (0..centers.len()).any(|index| {
                    let next = (index + 1) % centers.len();
                    let a = (
                        radii[index] * centers[index].sin(),
                        -radii[index] * centers[index].cos(),
                    );
                    let b = (
                        radii[next] * centers[next].sin(),
                        -radii[next] * centers[next].cos(),
                    );
                    fan_triangle_contains(unit, a, b)
                });
                assert_eq!(polygon.contains(point.0, point.1), expected);
            }
        }
    }

    fn close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-10,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn linear_maps_inverts_and_supports_reversed_domains() {
        let scale = LinearScale::new(10.0, 20.0, 0.0, 100.0).unwrap();
        close(scale.coordinate(12.5).unwrap(), 25.0);
        close(scale.invert(75.0).unwrap(), 17.5);

        let reversed = LinearScale::new(20.0, 10.0, -50.0, 50.0).unwrap();
        close(reversed.coordinate(15.0).unwrap(), 0.0);
        close(reversed.coordinate(10.0).unwrap(), 50.0);
        close(reversed.invert(25.0).unwrap(), 12.5);
    }

    #[test]
    fn linear_maps_the_complete_finite_domain_without_overflow() {
        let scale = LinearScale::new(-f64::MAX, f64::MAX, -1.0, 1.0).unwrap();
        assert_eq!(scale.coordinate(-f64::MAX), Some(-1.0));
        assert_eq!(scale.coordinate(0.0), Some(0.0));
        assert_eq!(scale.coordinate(f64::MAX), Some(1.0));
        assert_eq!(scale.invert(-1.0), Some(-f64::MAX));
        assert_eq!(scale.invert(0.0), Some(0.0));
        assert_eq!(scale.invert(1.0), Some(f64::MAX));
        let ticks = scale.ticks(10);
        assert!(!ticks.is_empty());
        assert!(ticks.len() <= MAX_GENERAL_TICKS);
        assert!(ticks.iter().all(|value| value.is_finite()));
    }

    #[test]
    fn linear_clamps_and_collapsed_ranges_have_no_inverse() {
        let mut scale = LinearScale::new(-1.0, 1.0, 10.0, 20.0).unwrap();
        assert_eq!(scale.coordinate_clamped(-4.0), Some(10.0));
        assert_eq!(scale.coordinate_clamped(4.0), Some(20.0));
        scale.set_range(7.0, 7.0).unwrap();
        assert_eq!(scale.coordinate(0.0), Some(7.0));
        assert_eq!(scale.invert(7.0), None);
        assert_eq!(scale.coordinate(f64::NAN), None);
    }

    #[test]
    fn linear_updates_reject_invalid_input_atomically() {
        let mut scale = LinearScale::new(0.0, 10.0, 0.0, 100.0).unwrap();
        assert_eq!(
            scale.set_domain(4.0, 4.0),
            Err(ScaleError::DegenerateDomain)
        );
        assert_eq!(scale.domain(), (0.0, 10.0));
        assert_eq!(
            scale.set_range(0.0, f64::INFINITY),
            Err(ScaleError::NonFinite)
        );
        assert_eq!(scale.range(), (0.0, 100.0));
    }

    #[test]
    fn linear_ticks_are_nice_bounded_and_follow_domain_direction() {
        let scale = LinearScale::new(-0.7, 9.3, 0.0, 100.0).unwrap();
        assert_eq!(scale.ticks(5), vec![0.0, 2.0, 4.0, 6.0, 8.0]);
        assert!(scale.ticks(usize::MAX).len() <= MAX_GENERAL_TICKS);

        let reversed = LinearScale::new(9.3, -0.7, 0.0, 100.0).unwrap();
        assert_eq!(reversed.ticks(5), vec![8.0, 6.0, 4.0, 2.0, 0.0]);
        assert!(reversed.ticks(0).is_empty());
    }

    #[test]
    fn nice_linear_domains_take_the_tightest_clean_step() {
        assert_eq!(nice_linear_domain(0.0, 72.0, 5), [0.0, 80.0]);
        assert_eq!(nice_linear_domain(0.0, 81.0, 5), [0.0, 100.0]);
        assert_eq!(nice_linear_domain(-5.0, 5.0, 5), [-5.0, 5.0]);
        assert_eq!(nice_linear_domain(-3.5, 11.5, 5), [-5.0, 15.0]);
        assert_eq!(nice_linear_domain(0.0, 7.0, 5), [0.0, 8.0]);
        assert_eq!(nice_linear_domain(72.0, 0.0, 5), [0.0, 80.0]);
        assert_eq!(nice_linear_domain(0.0, 0.3, 5), [0.0, 0.30000000000000004]);
        assert_eq!(nice_linear_domain(1.0, 1.0, 5), [1.0, 1.0]);
        assert_eq!(
            nice_linear_domain(-f64::MAX, f64::MAX, 5),
            [-f64::MAX, f64::MAX]
        );
        assert_eq!(nice_linear_domain(2.0, 3.0, 0), [2.0, 3.0]);
    }

    #[test]
    fn log_scale_maps_inverts_and_emits_bounded_decade_ticks() {
        let scale = LogScale::new(1.0, 1_000.0, 0.0, 300.0).unwrap();
        close(scale.coordinate(10.0).unwrap(), 100.0);
        close(scale.coordinate(100.0).unwrap(), 200.0);
        close(scale.invert(150.0).unwrap(), 10.0_f64.powf(1.5));
        let ticks = scale.ticks(8);
        assert!(ticks.contains(&1.0));
        assert!(ticks.iter().all(|value| *value > 0.0));
        assert!(ticks.len() <= MAX_GENERAL_TICKS);
        assert_eq!(
            LogScale::new(3.0, 4.0, 0.0, 1.0).unwrap().ticks(6),
            vec![3.0, 4.0]
        );
        assert_eq!(
            LogScale::new(0.0, 10.0, 0.0, 1.0),
            Err(ScaleError::NonPositiveDomain)
        );
    }

    #[test]
    fn symlog_scale_round_trips_through_zero_and_validates_constant() {
        let scale = SymLogScale::new(-100.0, 100.0, 0.0, 200.0, 1.0).unwrap();
        close(scale.coordinate(0.0).unwrap(), 100.0);
        for value in [-100.0, -4.0, 0.0, 3.5, 100.0] {
            close(
                scale.invert(scale.coordinate(value).unwrap()).unwrap(),
                value,
            );
        }
        assert!(scale.ticks(7).len() <= MAX_GENERAL_TICKS);
        // Ticks are round signed decades around zero, never transformed-space fractions.
        let wide = SymLogScale::new(-5_200.0, 7_300.0, 0.0, 300.0, 1.0).unwrap();
        assert_eq!(
            wide.ticks(10),
            vec![-1_000.0, -100.0, -10.0, 0.0, 10.0, 100.0, 1_000.0]
        );
        assert_eq!(wide.ticks(5), vec![-1_000.0, -10.0, 0.0, 10.0, 1_000.0]);
        // The thinned innermost decade would crowd zero's label, so it is skipped.
        assert_eq!(
            SymLogScale::new(-12_000.0, 12_000.0, 0.0, 300.0, 1.0)
                .unwrap()
                .ticks(5),
            vec![-10_000.0, -100.0, 0.0, 100.0, 10_000.0]
        );
        assert_eq!(
            SymLogScale::new(10.0, -10.0, 0.0, 1.0, 1.0)
                .unwrap()
                .ticks(12),
            vec![10.0, 5.0, 2.0, 1.0, 0.0, -1.0, -2.0, -5.0, -10.0]
        );
        assert_eq!(
            SymLogScale::new(-0.5, 0.5, 0.0, 1.0, 1.0).unwrap().ticks(5),
            LinearScale::new(-0.5, 0.5, 0.0, 1.0).unwrap().ticks(5)
        );
        assert_eq!(
            SymLogScale::new(-1.0, 1.0, 0.0, 1.0, 0.0),
            Err(ScaleError::InvalidConstant)
        );
    }

    #[test]
    fn band_scale_respects_padding_alignment_and_reverse_direction() {
        let scale = BandScale::new(3, 0.0, 120.0, 0.2, 0.1, 0.5).unwrap();
        close(scale.step(), 40.0);
        close(scale.bandwidth(), 32.0);
        assert_eq!(scale.bounds(0), Some((4.0, 36.0)));
        assert_eq!(scale.center(1), Some(60.0));
        assert_eq!(scale.bounds(2), Some((84.0, 116.0)));
        assert_eq!(scale.center(3), None);

        let reversed = BandScale::new(3, 120.0, 0.0, 0.2, 0.1, 0.5).unwrap();
        assert_eq!(reversed.bounds(0), Some((84.0, 116.0)));
        assert_eq!(reversed.center(2), Some(20.0));
    }

    #[test]
    fn point_scale_handles_endpoints_singletons_and_reverse_direction() {
        let scale = PointScale::new(3, 0.0, 100.0, 0.0, 0.5).unwrap();
        assert_eq!(scale.step(), 50.0);
        assert_eq!(scale.coordinate(0), Some(0.0));
        assert_eq!(scale.coordinate(1), Some(50.0));
        assert_eq!(scale.coordinate(2), Some(100.0));

        let singleton = PointScale::new(1, 0.0, 100.0, 0.0, 0.25).unwrap();
        assert_eq!(singleton.coordinate(0), Some(25.0));

        let reversed = PointScale::new(3, 100.0, 0.0, 0.0, 0.5).unwrap();
        assert_eq!(reversed.coordinate(0), Some(100.0));
        assert_eq!(reversed.coordinate(2), Some(0.0));
        assert_eq!(reversed.coordinate(3), None);
    }

    #[test]
    fn category_scales_validate_without_allocating_category_state() {
        assert_eq!(
            BandScale::new(2, 0.0, 10.0, 1.1, 0.0, 0.5),
            Err(ScaleError::InvalidPadding)
        );
        assert_eq!(
            PointScale::new(2, 0.0, 10.0, 0.0, -0.1),
            Err(ScaleError::InvalidAlign)
        );
        assert_eq!(
            BandScale::new(0, 0.0, 10.0, 0.0, 0.0, 0.5)
                .unwrap()
                .center(0),
            None
        );
        assert_eq!(
            PointScale::new(0, 0.0, 10.0, 0.0, 0.5)
                .unwrap()
                .coordinate(0),
            None
        );
    }
}
