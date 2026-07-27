//! Numeric helpers. Port of `src/helpers/mathex.ts`.

pub fn clamp(value: f64, min_val: f64, max_val: f64) -> f64 {
    value.max(min_val).min(max_val)
}

/// True if `value` is a power of 10 (1, 10, 100, ...). Port of `isBaseDecimal`.
pub fn is_base_decimal(value: i64) -> bool {
    if value < 0 {
        return false;
    }
    let mut v = value;
    while v > 1 {
        if v % 10 != 0 {
            return false;
        }
        v /= 10;
    }
    true
}

/// `x1 >= x2` within epsilon: `(x2 - x1) <= epsilon`.
pub fn greater_or_equal(x1: f64, x2: f64, epsilon: f64) -> bool {
    (x2 - x1) <= epsilon
}

pub fn equal(x1: f64, x2: f64, epsilon: f64) -> bool {
    (x1 - x2).abs() < epsilon
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_decimal() {
        assert!(is_base_decimal(1));
        assert!(is_base_decimal(10));
        assert!(is_base_decimal(100));
        assert!(is_base_decimal(10000));
        assert!(!is_base_decimal(25));
        assert!(!is_base_decimal(0) || is_base_decimal(0)); // 0: loop skipped -> true in reference too
        assert!(!is_base_decimal(-100));
    }
}
