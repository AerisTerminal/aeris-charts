//! Pure price-axis layout policy shared by the browser host and native unit tests.

/// reference grows the axis as soon as labels need more space, but a marks-only repaint does not trigger
/// a full layout merely to shrink it. A resize/full layout may accept the smaller measurement.
pub(crate) fn negotiated_axis_width(current: f64, measured: f64, allow_shrink: bool) -> f64 {
    if allow_shrink || current <= 0.0 {
        measured
    } else {
        current.max(measured)
    }
}

#[cfg(test)]
mod tests {
    use super::negotiated_axis_width;

    #[test]
    fn grows_immediately_but_shrinks_only_on_full_layout() {
        assert_eq!(negotiated_axis_width(58.0, 64.0, false), 64.0);
        assert_eq!(negotiated_axis_width(58.0, 52.0, false), 58.0);
        assert_eq!(negotiated_axis_width(58.0, 52.0, true), 52.0);
        assert_eq!(negotiated_axis_width(0.0, 56.0, false), 56.0);
    }
}
