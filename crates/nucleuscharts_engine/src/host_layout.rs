//! Platform-free chart host layout negotiation.
//!
//! Hosts supply glyph widths, while the engine owns label formatting, axis visibility, grow-fast /
//! shrink-on-full policy, even-pixel axis snapping, pane geometry, and time-scale width.

use crate::{ChartEngine, PriceScaleSide, PriceScaleTarget};

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
        let right_visible = self.options.get().right_price_scale.visible;
        let left_visible = self.options.get().left_price_scale.visible;

        let measure_builtins = |engine: &mut ChartEngine| {
            let has_named_right = engine.panes.iter().any(|pane| {
                pane.named_scales
                    .iter()
                    .any(|entry| entry.visible && entry.side == PriceScaleSide::Right)
            });
            let has_named_left = engine.panes.iter().any(|pane| {
                pane.named_scales
                    .iter()
                    .any(|entry| entry.visible && entry.side == PriceScaleSide::Left)
            });
            let measured_right = if right_visible {
                if has_named_right {
                    (0..engine.panes.len())
                        .map(|pane| {
                            engine.optimal_exact_price_axis_width_for(
                                pane,
                                PriceScaleTarget::Right,
                                |text| measure(text),
                            )
                        })
                        .fold(0.0_f64, f64::max)
                } else {
                    engine
                        .optimal_price_axis_width_for(PriceScaleTarget::Right, |text| measure(text))
                }
            } else {
                0.0
            };
            let measured_left = if left_visible {
                if has_named_left {
                    (0..engine.panes.len())
                        .map(|pane| {
                            engine.optimal_exact_price_axis_width_for(
                                pane,
                                PriceScaleTarget::Left,
                                |text| measure(text),
                            )
                        })
                        .fold(0.0_f64, f64::max)
                } else {
                    engine
                        .optimal_price_axis_width_for(PriceScaleTarget::Left, |text| measure(text))
                }
            } else {
                0.0
            };
            engine.right_builtin_axis_w = if right_visible {
                negotiated_axis_width(
                    engine.right_builtin_axis_w,
                    measured_right,
                    allow_axis_shrink,
                )
            } else {
                0.0
            };
            engine.left_builtin_axis_w = if left_visible {
                negotiated_axis_width(engine.left_builtin_axis_w, measured_left, allow_axis_shrink)
            } else {
                0.0
            };
        };
        measure_builtins(self);

        let measure_named = |engine: &mut ChartEngine| {
            let targets: Vec<_> = engine
                .panes
                .iter()
                .enumerate()
                .flat_map(|(pane, state)| {
                    state
                        .named_scales
                        .iter()
                        .filter(|entry| entry.visible)
                        .map(move |entry| (pane, entry.id, entry.width))
                })
                .collect();
            for (pane, id, current) in targets {
                let target = PriceScaleTarget::Named(id);
                let measured =
                    engine.optimal_exact_price_axis_width_for(pane, target, |text| measure(text));
                if let Some(entry) = engine.panes[pane].named_scale_mut(id) {
                    entry.width = negotiated_axis_width(current, measured, allow_axis_shrink);
                }
            }
        };
        measure_named(self);

        let side_total = |engine: &ChartEngine, pane_index: usize, side: PriceScaleSide| {
            engine.panes[pane_index]
                .ordered_side_targets(side)
                .into_iter()
                .filter(|target| engine.price_scale_visible_for(pane_index, *target))
                .filter_map(|target| engine.price_scale_axis_width(pane_index, target))
                .sum::<f64>()
        };
        let mut axis_w = (0..self.panes.len())
            .map(|pane| side_total(self, pane, PriceScaleSide::Right))
            .fold(0.0_f64, f64::max);
        let mut left_axis_w = (0..self.panes.len())
            .map(|pane| side_total(self, pane, PriceScaleSide::Left))
            .fold(0.0_f64, f64::max);

        for _ in 0..2 {
            let pane_w = (self.css_width - left_axis_w - axis_w).max(1.0);
            self.pane_left = left_axis_w;
            self.left_axis_w = left_axis_w;
            self.axis_w = axis_w;
            self.time_scale.set_width(pane_w);
            self.autoscale_visible();
            measure_builtins(self);
            measure_named(self);
            let new_w = (0..self.panes.len())
                .map(|pane| side_total(self, pane, PriceScaleSide::Right))
                .fold(0.0_f64, f64::max);
            let new_left_w = (0..self.panes.len())
                .map(|pane| side_total(self, pane, PriceScaleSide::Left))
                .fold(0.0_f64, f64::max);
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
    use crate::{ChartEngine, PriceScaleSide, SeriesKind};

    #[test]
    fn axes_grow_immediately_but_shrink_only_during_full_layout() {
        assert_eq!(negotiated_axis_width(58.0, 64.0, false), 64.0);
        assert_eq!(negotiated_axis_width(58.0, 52.0, false), 58.0);
        assert_eq!(negotiated_axis_width(58.0, 52.0, true), 52.0);
        assert_eq!(negotiated_axis_width(0.0, 56.0, false), 56.0);
    }

    #[test]
    fn named_scale_labels_do_not_inflate_the_builtin_axis_strip() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart
            .set_series_data(
                0,
                &[1.0, 2.0, 3.0],
                &[10.0, 11.0, 12.0],
                &[10.0, 11.0, 12.0],
                &[10.0, 11.0, 12.0],
                &[10.0, 11.0, 12.0],
            )
            .unwrap();
        let named = chart
            .add_price_scale(0, "large-values", PriceScaleSide::Right, None, true)
            .unwrap();
        chart.fit_content();
        chart.recompute_layout_with_measure(true, |text| text.len() as f64 * 7.0);
        let builtin_width = chart.right_builtin_axis_w;

        let comparison = chart.add_series(SeriesKind::Line);
        chart
            .set_series_data(
                comparison,
                &[1.0, 2.0, 3.0],
                &[1_000_000.0, 1_100_000.0, 1_200_000.0],
                &[1_000_000.0, 1_100_000.0, 1_200_000.0],
                &[1_000_000.0, 1_100_000.0, 1_200_000.0],
                &[1_000_000.0, 1_100_000.0, 1_200_000.0],
            )
            .unwrap();
        assert!(chart.series_apply_price_format_json(
            comparison,
            r#"{"type":"price","precision":4,"min_move":0.0001}"#
        ));
        chart.set_series_price_scale(comparison, named);
        chart.recompute_layout_with_measure(true, |text| text.len() as f64 * 7.0);

        assert_eq!(chart.right_builtin_axis_w, builtin_width);
        assert!(chart.price_scale_axis_width(0, named).unwrap() > builtin_width);
    }
}
