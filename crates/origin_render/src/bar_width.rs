//! Optimal bar/candle widths. Port of `src/renderers/optimal-bar-width.ts`.
//! See RENDERING_SPEC.md §2.1, §3.

use std::f64::consts::PI;

pub fn optimal_bar_width(bar_spacing: f64, pixel_ratio: f64) -> f64 {
    (bar_spacing * 0.3 * pixel_ratio).floor()
}

pub fn optimal_candlestick_width(bar_spacing: f64, pixel_ratio: f64) -> i32 {
    const BAR_SPACING_SPECIAL_CASE_FROM: f64 = 2.5;
    const BAR_SPACING_SPECIAL_CASE_TO: f64 = 4.0;
    const BAR_SPACING_SPECIAL_CASE_COEFF: f64 = 3.0;

    if (BAR_SPACING_SPECIAL_CASE_FROM..=BAR_SPACING_SPECIAL_CASE_TO).contains(&bar_spacing) {
        return (BAR_SPACING_SPECIAL_CASE_COEFF * pixel_ratio).floor() as i32;
    }

    // coeff should be 1 on small bar spacing and go to 0.8 as spacing grows
    const BAR_SPACING_REDUCING_COEFF: f64 = 0.2;
    let coeff = 1.0
        - BAR_SPACING_REDUCING_COEFF
            * (bar_spacing.max(BAR_SPACING_SPECIAL_CASE_TO) - BAR_SPACING_SPECIAL_CASE_TO).atan()
            / (PI * 0.5);
    let res = (bar_spacing * coeff * pixel_ratio).floor();
    let scaled_bar_spacing = (bar_spacing * pixel_ratio).floor();
    let optimal = res.min(scaled_bar_spacing);
    (pixel_ratio.floor().max(optimal)) as i32
}

/// Crosshair-symmetry parity correction (RENDERING_SPEC.md §2.1): grid/crosshair line width is
/// `floor(pixel_ratio)` clamped to 1 device px (the engines' `max(1, floor)` line rule); candle
/// width parity must match it so the crosshair centers on candles at any ratio, sub-1 included.
pub fn apply_crosshair_parity(mut bar_width: i32, pixel_ratio: f64) -> i32 {
    if bar_width >= 2 {
        let wick_width = (pixel_ratio.floor() as i32).max(1);
        if (wick_width % 2) != (bar_width % 2) {
            bar_width -= 1;
        }
    }
    bar_width
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_case_band() {
        // barSpacing in [2.5, 4] -> floor(3 * dpr)
        assert_eq!(optimal_candlestick_width(2.5, 1.0), 3);
        assert_eq!(optimal_candlestick_width(3.0, 1.0), 3);
        assert_eq!(optimal_candlestick_width(4.0, 1.0), 3);
        assert_eq!(optimal_candlestick_width(3.0, 2.0), 6);
    }

    #[test]
    fn default_spacing_dpr1() {
        // barSpacing 6, dpr 1: coeff = 1 - 0.2*atan(2)/(pi/2) ~= 0.85903; floor(6*coeff) = 5
        assert_eq!(optimal_candlestick_width(6.0, 1.0), 5);
    }

    #[test]
    fn small_spacing_keeps_min_one_pixel() {
        assert_eq!(optimal_candlestick_width(0.5, 1.0), 1);
        assert_eq!(optimal_candlestick_width(1.0, 1.0), 1);
        assert_eq!(optimal_candlestick_width(0.5, 2.0), 2);
    }

    #[test]
    fn coeff_approaches_08_for_large_spacing() {
        // large spacing: coeff -> 0.8
        let w = optimal_candlestick_width(100.0, 1.0);
        assert!((80..=82).contains(&w), "got {w}");
    }

    #[test]
    fn parity_correction() {
        // dpr 1 -> wick width 1 (odd); even body widths shrink by 1
        assert_eq!(apply_crosshair_parity(6, 1.0), 5);
        assert_eq!(apply_crosshair_parity(5, 1.0), 5);
        // dpr 2 -> wick width 2 (even); odd body widths shrink by 1
        assert_eq!(apply_crosshair_parity(5, 2.0), 4);
        assert_eq!(apply_crosshair_parity(6, 2.0), 6);
        // width 1 never corrected
        assert_eq!(apply_crosshair_parity(1, 2.0), 1);
        // sub-1 dpr -> wick width clamps to 1 (odd), so bodies stay odd (centered on the wick)
        assert_eq!(apply_crosshair_parity(4, 0.75), 3);
        assert_eq!(apply_crosshair_parity(3, 0.75), 3);
        assert_eq!(apply_crosshair_parity(6, 0.5), 5);
    }

    #[test]
    fn bar_width_30_percent() {
        assert_eq!(optimal_bar_width(6.0, 1.0), 1.0);
        assert_eq!(optimal_bar_width(10.0, 1.0), 3.0);
        assert_eq!(optimal_bar_width(10.0, 2.0), 6.0);
    }
}
