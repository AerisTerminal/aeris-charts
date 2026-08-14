//! Platform-free chart host layout negotiation.
//!
//! Hosts supply glyph widths, while the engine owns label formatting, axis visibility, grow-fast /
//! shrink-on-full policy, even-pixel axis snapping, pane geometry, and time-scale width.

use crate::{ChartEngine, PriceScaleTarget};

fn negotiated_axis_width(current: f64, measured: f64, allow_shrink: bool) -> f64 {
    if allow_shrink || current <= 0.0 {
        measured
    } else {
        current.max(measured)
    }
}

impl ChartEngine {
    /// Recompute pane and axis geometry from the current CSS size using host-native text widths.
    ///
    /// The operation is idempotent and performs the same two-pass refinement used by the browser
    /// host. `allow_axis_shrink` should be true for a full resize/layout and false for ordinary
    /// repaints, where axes grow immediately but do not visually breathe smaller.
    pub fn recompute_layout_with_measure<F>(&mut self, allow_axis_shrink: bool, measure: F)
    where
        F: Fn(&str) -> f64,
    {
        self.frame_build_stats.layout_rebuilds += 1;
        let content_h = (self.css_height - self.time_axis_height()).max(1.0);
        self.layout_panes(content_h);
        let options = self.options.get();
        let right_visible = options.right_price_scale.visible;
        let left_visible = options.left_price_scale.visible;

        let measure_side = |engine: &mut ChartEngine, target| {
            engine.optimal_price_axis_width_for(target, |text| measure(text))
        };
        let measured_axis_w = if right_visible {
            measure_side(self, PriceScaleTarget::Right)
        } else {
            0.0
        };
        let measured_left_axis_w = if left_visible {
            measure_side(self, PriceScaleTarget::Left)
        } else {
            0.0
        };
        let mut axis_w = if right_visible {
            negotiated_axis_width(self.axis_w, measured_axis_w, allow_axis_shrink)
        } else {
            0.0
        };
        let mut left_axis_w = if left_visible {
            negotiated_axis_width(self.left_axis_w, measured_left_axis_w, allow_axis_shrink)
        } else {
            0.0
        };

        for _ in 0..2 {
            let pane_w = (self.css_width - left_axis_w - axis_w).max(1.0);
            self.pane_left = left_axis_w;
            self.left_axis_w = left_axis_w;
            self.axis_w = axis_w;
            self.time_scale.set_width(pane_w);
            self.autoscale_visible();
            let measured_new_w = if right_visible {
                measure_side(self, PriceScaleTarget::Right)
            } else {
                0.0
            };
            let measured_new_left_w = if left_visible {
                measure_side(self, PriceScaleTarget::Left)
            } else {
                0.0
            };
            let new_w = if right_visible {
                negotiated_axis_width(axis_w, measured_new_w, allow_axis_shrink)
            } else {
                0.0
            };
            let new_left_w = if left_visible {
                negotiated_axis_width(left_axis_w, measured_new_left_w, allow_axis_shrink)
            } else {
                0.0
            };
            if new_w == axis_w && new_left_w == left_axis_w {
                break;
            }
            axis_w = new_w;
            left_axis_w = new_left_w;
        }

        self.pane_left = left_axis_w;
        self.left_axis_w = left_axis_w;
        self.pane_w = (self.css_width - left_axis_w - axis_w).max(1.0);
        self.pane_h = content_h;
        self.axis_w = axis_w;
        self.frame_layout_prepared();
    }
}

#[cfg(test)]
mod tests {
    use super::negotiated_axis_width;

    #[test]
    fn axes_grow_immediately_but_shrink_only_during_full_layout() {
        assert_eq!(negotiated_axis_width(58.0, 64.0, false), 64.0);
        assert_eq!(negotiated_axis_width(58.0, 52.0, false), 58.0);
        assert_eq!(negotiated_axis_width(58.0, 52.0, true), 52.0);
        assert_eq!(negotiated_axis_width(0.0, 56.0, false), 56.0);
    }
}
