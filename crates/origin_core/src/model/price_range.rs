//! Price range. Port of `src/model/price-range-impl.ts`.

fn compute_finite_result(method: fn(f64, f64) -> f64, v1: f64, v2: f64, fallback: f64) -> f64 {
    let first_finite = v1.is_finite();
    let second_finite = v2.is_finite();

    if first_finite && second_finite {
        return method(v1, v2);
    }

    if !first_finite && !second_finite {
        fallback
    } else if first_finite {
        v1
    } else {
        v2
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PriceRange {
    min_value: f64,
    max_value: f64,
}

impl PriceRange {
    pub fn new(min_value: f64, max_value: f64) -> Self {
        Self {
            min_value,
            max_value,
        }
    }

    pub fn min_value(&self) -> f64 {
        self.min_value
    }

    pub fn max_value(&self) -> f64 {
        self.max_value
    }

    pub fn length(&self) -> f64 {
        self.max_value - self.min_value
    }

    pub fn is_empty(&self) -> bool {
        self.max_value == self.min_value || self.max_value.is_nan() || self.min_value.is_nan()
    }

    pub fn merge(&self, another: Option<&PriceRange>) -> PriceRange {
        match another {
            None => *self,
            Some(other) => PriceRange::new(
                compute_finite_result(f64::min, self.min_value, other.min_value, f64::NEG_INFINITY),
                compute_finite_result(f64::max, self.max_value, other.max_value, f64::INFINITY),
            ),
        }
    }

    pub fn scale_around_center(&mut self, coeff: f64) {
        if !coeff.is_finite() {
            return;
        }

        let delta = self.max_value - self.min_value;
        if delta == 0.0 {
            return;
        }

        let center = (self.max_value + self.min_value) * 0.5;
        let max_delta = (self.max_value - center) * coeff;
        let min_delta = (self.min_value - center) * coeff;
        self.max_value = center + max_delta;
        self.min_value = center + min_delta;
    }

    pub fn shift(&mut self, delta: f64) {
        if !delta.is_finite() {
            return;
        }
        self.max_value += delta;
        self.min_value += delta;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_around_center() {
        let mut r = PriceRange::new(0.0, 10.0);
        r.scale_around_center(2.0);
        assert_eq!(r.min_value(), -5.0);
        assert_eq!(r.max_value(), 15.0);
    }

    #[test]
    fn merge_with_infinite_values() {
        let a = PriceRange::new(f64::NAN, f64::INFINITY);
        let b = PriceRange::new(1.0, 2.0);
        let m = a.merge(Some(&b));
        assert_eq!(m.min_value(), 1.0);
        assert_eq!(m.max_value(), 2.0);
    }

    #[test]
    fn shift_moves_both_edges() {
        let mut r = PriceRange::new(1.0, 3.0);
        r.shift(2.5);
        assert_eq!(r.min_value(), 3.5);
        assert_eq!(r.max_value(), 5.5);
    }
}
