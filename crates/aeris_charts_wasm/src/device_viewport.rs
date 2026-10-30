//! Engine viewport for a canvas whose backing store is an exact device-pixel box.

/// Media width, media height, and pixel ratio for a `css_width` x `css_height` container with a
/// `bitmap_width` x `bitmap_height` device-pixel backing store.
///
/// The engine sizes every surface as `round(media * ratio)` per axis with one ratio. The ratio
/// comes from the width, and the container's own CSS height is kept whenever it rounds onto the
/// device box. At fractional scaling the browser can round the height independently, landing one
/// device pixel away; the WebGPU surface would then resize the backing store away from its CSS box
/// and the compositor would stretch every horizontal edge. Only then does the media height come
/// from the bitmap, which is exactly the height of the device-grid-aligned canvas box.
pub fn engine_viewport(
    css_width: f64,
    css_height: f64,
    bitmap_width: f64,
    bitmap_height: f64,
) -> (f64, f64, f64) {
    let width = css_width.max(1.0);
    let ratio = bitmap_width.max(1.0) / width;
    let bitmap_height = bitmap_height.max(1.0);
    let height = if (css_height * ratio).round() == bitmap_height {
        css_height
    } else {
        bitmap_height / ratio
    };
    (width, height, ratio)
}

#[cfg(test)]
mod tests {
    use super::engine_viewport;

    #[test]
    fn engine_bitmap_matches_the_exact_device_box_on_both_axes() {
        // Container sizes at common fractional scalings, with device boxes the browser rounds
        // independently per axis (several where round(css_h * bw / css_w) != bh).
        let cases = [
            (801.6, 451.2, 1002.0, 564.0),
            (801.6, 450.4, 1002.0, 563.0),
            (1203.33, 678.0, 1805.0, 1017.0),
            (640.0, 359.3, 800.0, 449.0),
            (1000.0, 600.0, 1750.0, 1050.0),
            (333.3, 222.4, 333.0, 222.0),
        ];
        for (css_w, css_h, bw, bh) in cases {
            let (width, height, ratio) = engine_viewport(css_w, css_h, bw, bh);
            assert_eq!((width * ratio).round(), bw, "width for {css_w}");
            assert_eq!((height * ratio).round(), bh, "height for {css_w}x{css_h}");
        }
    }

    #[test]
    fn a_height_that_already_rounds_onto_the_device_box_is_kept() {
        // 655 CSS px at 1.25 is 818.75 device px, which the browser reports as 819.
        assert_eq!(
            engine_viewport(844.0, 655.0, 1055.0, 819.0),
            (844.0, 655.0, 1.25)
        );
    }

    #[test]
    fn degenerate_sizes_stay_positive() {
        let (width, height, ratio) = engine_viewport(0.0, 0.0, 0.0, 0.0);
        assert!(width > 0.0 && height > 0.0 && ratio > 0.0);
    }
}
