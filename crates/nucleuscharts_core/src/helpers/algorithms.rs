//! Binary search helpers. Port of `src/helpers/algorithms.ts`.

/// First index in `items` for which `less(item)` is false.
/// `less` must partition the slice: true-prefix, false-suffix.
pub fn lower_bound<T>(items: &[T], mut less: impl FnMut(&T) -> bool) -> usize {
    let mut lo = 0usize;
    let mut hi = items.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if less(&items[mid]) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// First index in `items` for which `greater(item)` is true.
/// `greater` must partition the slice: false-prefix, true-suffix.
pub fn upper_bound<T>(items: &[T], mut greater: impl FnMut(&T) -> bool) -> usize {
    let mut lo = 0usize;
    let mut hi = items.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if greater(&items[mid]) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lower_bound_finds_first_not_less() {
        let v = [1, 2, 4, 4, 7];
        assert_eq!(lower_bound(&v, |x| *x < 4), 2);
        assert_eq!(lower_bound(&v, |x| *x < 0), 0);
        assert_eq!(lower_bound(&v, |x| *x < 100), 5);
    }

    #[test]
    fn upper_bound_finds_first_greater() {
        let v = [1, 2, 4, 4, 7];
        assert_eq!(upper_bound(&v, |x| *x > 4), 4);
        assert_eq!(upper_bound(&v, |x| *x > 0), 0);
        assert_eq!(upper_bound(&v, |x| *x > 100), 5);
    }
}
