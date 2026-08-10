//! Interaction models the engine owns outright: hosts forward
//! normalized pointer/wheel samples and schedule frames; every formula lives here so the
//! native headless harness exercises the exact code the browser runs.
//!
//! - kinetic (momentum) scroll — reference `model/kinetic-animation.ts`, sampled in pointer px
//! - axis drag-to-scale and vertical price pan routing onto the pane scales — reference
//!   `PriceAxisWidget`/`TimeAxisWidget` pressedMouseMove and `startScrollPrice`/`scrollPriceTo`
//! - wheel/pinch zoom increments — reference chart-widget.ts `_onMousewheel` and pane-widget.ts
//!   `pinchEvent`
//! - animated scroll-to-position — cubic ease-out progress (the host only schedules frames)

use origin_core::model::kinetic_animation::KineticAnimation;

use super::*;

/// reference `KineticScrollConstants` (pane-widget.ts:38-43) in the px domain: the reference
/// divides them by the bar spacing to work in rightOffset units; sampling pointer px directly
/// with the raw constants is the equivalent formulation the Origin hosts have always used.
pub const KINETIC_MIN_SPEED: f64 = 0.2;
pub const KINETIC_MAX_SPEED: f64 = 7.0;
pub const KINETIC_DUMPING: f64 = 0.997;
pub const KINETIC_MIN_MOVE: f64 = 15.0;

/// reference pane-widget.ts `pinchEvent`: the incremental scale multiplier per pinch step.
pub const PINCH_ZOOM_INTENSITY: f64 = 5.0;

/// reference chart-widget.ts `_onMousewheel`: `scrollChart(deltaX * -80)` — "80 is a made up
/// coefficient, and minus is for the 'natural' scroll".
pub const WHEEL_SCROLL_PX_PER_DELTA: f64 = -80.0;

/// reference chart-widget.ts `_onMousewheel`: `sign(deltaY) * min(1, |deltaY|)` — the wheel
/// delta normalized to a zoom increment in 1/10-spacing units (the input to `zoom`).
pub fn wheel_zoom_scale(delta_y: f64) -> f64 {
    delta_y.signum() * delta_y.abs().min(1.0)
}

/// reference pane-widget.ts `pinchEvent`: the scale ratio delta since the previous step, times
/// the intensity (the engine clamps the resulting spacing).
pub fn pinch_zoom_scale(scale_delta: f64) -> f64 {
    scale_delta * PINCH_ZOOM_INTENSITY
}

/// An in-flight animated scroll (reference `scrollToPosition(position, animated)` semantics):
/// cubic ease-out from the position at `start` to `target` over `duration_ms`. The host's only
/// jobs are scheduling a frame per tick and cancelling on a newer scroll or user gesture.
#[derive(Clone, Copy, Debug)]
pub struct ScrollAnimation {
    pub start_position: f64,
    pub target_position: f64,
    pub start_time_ms: f64,
    pub duration_ms: f64,
}

impl ScrollAnimation {
    /// Cubic ease-out progress in [0, 1] (1 completes the animation).
    fn progress(&self, now_ms: f64) -> f64 {
        if self.duration_ms <= 0.0 {
            return 1.0;
        }
        ((now_ms - self.start_time_ms) / self.duration_ms).clamp(0.0, 1.0)
    }

    /// The eased position at `now_ms`.
    fn position(&self, now_ms: f64) -> f64 {
        let t = self.progress(now_ms);
        let eased = 1.0 - (1.0 - t).powi(3);
        self.start_position + (self.target_position - self.start_position) * eased
    }
}

impl ChartEngine {
    // --- kinetic (momentum) scroll ---

    /// Open a kinetic sampling session alongside a drag-scroll (reference creates a fresh
    /// `KineticAnimation` on the first scrolling move when the device option allows it;
    /// `enabled = false` mirrors its `_scrollXAnimation = null`). Seeds the first sample.
    pub fn kinetic_begin_sampling(&mut self, enabled: bool, x: f64, now_ms: f64) {
        self.kinetic = enabled.then(|| {
            let mut animation = KineticAnimation::new(
                KINETIC_MIN_SPEED,
                KINETIC_MAX_SPEED,
                KINETIC_DUMPING,
                KINETIC_MIN_MOVE,
            );
            animation.add_position(x, now_ms);
            animation
        });
    }

    /// Feed a drag-move sample (reference `addPosition` on every scrolling move).
    pub fn kinetic_add_sample(&mut self, x: f64, now_ms: f64) {
        if let Some(animation) = self.kinetic.as_mut() {
            animation.add_position(x, now_ms);
        }
    }

    /// The drag was released: freeze the coast. Returns whether a coast engaged (the host then
    /// drives `kinetic_position` from its frame scheduler instead of ending the scroll session).
    pub fn kinetic_release(&mut self, x: f64, now_ms: f64) -> bool {
        let Some(animation) = self.kinetic.as_mut() else {
            return false;
        };
        animation.start(x, now_ms);
        !animation.finished(now_ms)
    }

    /// The coast's position at `now_ms`; `None` when no coast is engaged.
    pub fn kinetic_position(&self, now_ms: f64) -> Option<f64> {
        self.kinetic.as_ref().map(|a| a.position(now_ms))
    }

    /// Whether the coast has run its course (true when none is engaged).
    pub fn kinetic_finished(&self, now_ms: f64) -> bool {
        self.kinetic.as_ref().is_none_or(|a| a.finished(now_ms))
    }

    /// Drop the sampler/coast entirely (a fresh gesture supersedes any in-flight coast).
    pub fn kinetic_stop(&mut self) {
        self.kinetic = None;
    }

    // --- axis drag-to-scale ---

    /// reference `TimeAxisWidget` pressedMouseMove arm (`TimeScale.startScale`).
    pub fn time_axis_start_scale(&mut self, x: f64) {
        self.time_scale.start_scale(x);
    }

    /// reference `TimeScale.scaleTo` (bar spacing by the ratio of distances-from-right).
    pub fn time_axis_scale_to(&mut self, x: f64) {
        self.time_scale.scale_to(x);
    }

    pub fn time_axis_end_scale(&mut self) {
        self.time_scale.end_scale();
    }

    /// Whether a drag on this price axis can scale it (reference `PriceScale.scaleTo` no-ops in
    /// percentage and indexed-to-100 modes; an empty scale has nothing to scale).
    pub fn price_axis_scalable(&self, pane: usize, target: PriceScaleTarget) -> bool {
        let Some(scale) = self.price_scale_for(pane, target) else {
            return false;
        };
        !scale.is_percentage() && !scale.is_indexed_to_100() && scale.price_range().is_some()
    }

    /// reference `PriceAxisWidget` pressedMouseMove arm (`PriceScale.startScale`); `y` is the
    /// chart-content coordinate (the scale crops itself to the pane via internal margins).
    pub fn price_axis_start_scale(&mut self, pane: usize, target: PriceScaleTarget, y: f64) {
        if let Some(scale) = self.price_scale_for_mut(pane, target) {
            scale.start_scale(y);
        }
    }

    /// reference `PriceScale.scaleTo` (the start range scaled around its center).
    pub fn price_axis_scale_to(&mut self, pane: usize, target: PriceScaleTarget, y: f64) {
        if let Some(scale) = self.price_scale_for_mut(pane, target) {
            scale.scale_to(y);
        }
    }

    pub fn price_axis_end_scale(&mut self, pane: usize, target: PriceScaleTarget) {
        if let Some(scale) = self.price_scale_for_mut(pane, target) {
            scale.end_scale();
        }
    }

    /// TradingView-style wheel zoom on a price axis (the reference has no price-axis wheel;
    /// the time axis wheel is `_onMousewheel` → `zoomTime`). `scale` is the same normalized
    /// increment the time-axis wheel consumes (`wheel_zoom_scale`), converted to a per-notch
    /// range factor: 10% per full notch, anchored at the cursor's price.
    pub fn price_axis_wheel_zoom(
        &mut self,
        pane: usize,
        target: PriceScaleTarget,
        y: f64,
        scale: f64,
    ) {
        let factor = (1.0 - scale * 0.1).clamp(0.05, 20.0);
        if let Some(price_scale) = self.price_scale_for_mut(pane, target) {
            price_scale.zoom(y, factor);
        }
    }

    // --- vertical price pan ---

    /// reference chart-model.ts `startScrollPrice`: no-ops while the scale is in autoscale.
    pub fn price_axis_start_scroll(&mut self, pane: usize, target: PriceScaleTarget, y: f64) {
        if let Some(scale) = self.price_scale_for_mut(pane, target) {
            scale.start_scroll(y);
        }
    }

    /// reference chart-model.ts `scrollPriceTo`: the start range shifted by `dy * span/(h-1)`.
    pub fn price_axis_scroll_to(&mut self, pane: usize, target: PriceScaleTarget, y: f64) {
        if let Some(scale) = self.price_scale_for_mut(pane, target) {
            scale.scroll_to(y);
        }
    }

    pub fn price_axis_end_scroll(&mut self, pane: usize, target: PriceScaleTarget) {
        if let Some(scale) = self.price_scale_for_mut(pane, target) {
            scale.end_scroll();
        }
    }

    // --- animated scroll-to-position ---

    /// Start an eased scroll to `target_position` (logical bars from the right edge), replacing
    /// any in-flight animation. The engine applies each tick itself; the host schedules frames
    /// and repaints.
    pub fn start_scroll_animation(&mut self, target_position: f64, duration_ms: f64, now_ms: f64) {
        if !target_position.is_finite() {
            return;
        }
        self.scroll_animation = Some(ScrollAnimation {
            start_position: self.scroll_position(),
            target_position,
            start_time_ms: now_ms,
            duration_ms: duration_ms.max(0.0),
        });
    }

    /// Apply the animation's eased position for `now_ms`. Returns the applied position, or
    /// `None` when no animation is running (finished animations self-clear after applying the
    /// final position).
    pub fn scroll_animation_tick(&mut self, now_ms: f64) -> Option<f64> {
        let animation = self.scroll_animation?;
        let position = animation.position(now_ms);
        self.scroll_to_position(position);
        if animation.progress(now_ms) >= 1.0 {
            self.scroll_animation = None;
            return None;
        }
        Some(position)
    }

    /// Invalidate any in-flight animated scroll (a new scroll call or user gesture supersedes).
    pub fn cancel_scroll_animation(&mut self) {
        self.scroll_animation = None;
    }

    pub fn scroll_animation_active(&self) -> bool {
        self.scroll_animation.is_some()
    }

    // --- pane hit-testing ---

    /// Index of the stacked pane containing content-y `y` (panes own their bounds; separators
    /// between them count as the pane above). Clamped to the last pane so coordinates below the
    /// content area still resolve.
    pub fn pane_index_at_y(&self, y: f64) -> usize {
        let mut index = 0;
        for (i, pane) in self.panes.iter().enumerate() {
            if y >= pane.top && (y < pane.top + pane.height || i + 1 == self.panes.len()) {
                return i;
            }
            index = i;
        }
        index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chart_with_data(width: f64, height: f64) -> ChartEngine {
        let mut chart = ChartEngine::new(width, height, 1.0);
        chart
            .set_series_data(
                0,
                &[1.0, 2.0, 3.0, 4.0, 5.0],
                &[10.0, 11.0, 12.0, 13.0, 14.0],
                &[11.0, 12.0, 13.0, 14.0, 15.0],
                &[9.0, 10.0, 11.0, 12.0, 13.0],
                &[10.5, 11.5, 12.5, 13.5, 14.5],
            )
            .unwrap();
        chart.time_scale.set_width(width);
        chart.layout_panes(height);
        chart.fit_content();
        chart.autoscale_visible();
        chart
    }

    #[test]
    fn kinetic_coast_engages_and_drives_the_scroll_session() {
        let mut chart = chart_with_data(400.0, 300.0);
        chart.time_scale.start_scroll(200.0);
        chart.kinetic_begin_sampling(true, 200.0, 1000.0);
        chart.time_scale.scroll_to(160.0);
        chart.kinetic_add_sample(160.0, 1020.0);
        chart.time_scale.scroll_to(120.0);
        chart.kinetic_add_sample(120.0, 1040.0);
        let offset_before = chart.right_offset();
        assert!(
            chart.kinetic_release(120.0, 1040.0),
            "a fast flick engages the coast"
        );
        assert!(!chart.kinetic_finished(1040.0));
        // The host drives the coast: positions feed the ongoing scroll session.
        let x = chart.kinetic_position(1060.0).unwrap();
        chart.time_scale.scroll_to(x);
        assert_ne!(chart.right_offset(), offset_before);
        assert!(chart.kinetic_finished(100_000.0));
        chart.kinetic_stop();
        assert!(chart.kinetic_position(1060.0).is_none());
        chart.time_scale.end_scroll();
    }

    #[test]
    fn kinetic_disabled_sampler_never_engages() {
        let mut chart = chart_with_data(400.0, 300.0);
        chart.kinetic_begin_sampling(false, 200.0, 1000.0);
        chart.kinetic_add_sample(120.0, 1020.0);
        assert!(!chart.kinetic_release(120.0, 1020.0));
        assert!(chart.kinetic_finished(1020.0));
    }

    #[test]
    fn time_axis_drag_scales_bar_spacing_by_the_right_ratio() {
        let mut chart = chart_with_data(400.0, 300.0);
        let start_spacing = chart.bar_spacing();
        chart.time_axis_start_scale(300.0);
        chart.time_axis_scale_to(200.0);
        // reference TimeScale.scaleTo: start * (width - x) / (width - startX)
        let expected = start_spacing * (400.0 - 200.0) / (400.0 - 300.0);
        assert!((chart.bar_spacing() - expected).abs() < 1e-9);
        chart.time_axis_end_scale();
    }

    #[test]
    fn price_axis_drag_scales_the_range_around_its_center() {
        let mut chart = chart_with_data(400.0, 300.0);
        chart.set_price_scale_auto_scale(0, false, false);
        let (from, to) = chart.price_scale_visible_range(0, false).unwrap();
        assert!(chart.price_axis_scalable(0, PriceScaleTarget::Right));
        chart.price_axis_start_scale(0, PriceScaleTarget::Right, 250.0);
        chart.price_axis_scale_to(0, PriceScaleTarget::Right, 200.0);
        let (new_from, new_to) = chart.price_scale_visible_range(0, false).unwrap();
        // reference PriceScale.scaleTo with height-flipped coordinates (height = 300):
        // coeff = (50 + 299*0.2) / (100 + 299*0.2)
        let coeff: f64 = (50.0 + 299.0 * 0.2) / (100.0 + 299.0 * 0.2);
        let mid = (from + to) / 2.0;
        let half = (to - from) / 2.0 * coeff.max(0.1);
        assert!((new_from - (mid - half)).abs() < 1e-9);
        assert!((new_to - (mid + half)).abs() < 1e-9);
        chart.price_axis_end_scale(0, PriceScaleTarget::Right);
    }

    #[test]
    fn price_axis_drag_noops_in_percentage_mode() {
        let mut chart = chart_with_data(400.0, 300.0);
        chart.set_price_scale_mode(0, false, PriceScaleMode::Percentage);
        assert!(!chart.price_axis_scalable(0, PriceScaleTarget::Right));
    }

    #[test]
    fn price_pan_shifts_the_range_by_pixels() {
        let mut chart = chart_with_data(400.0, 300.0);
        chart.set_price_scale_auto_scale(0, false, false);
        let (from, to) = chart.price_scale_visible_range(0, false).unwrap();
        chart.price_axis_start_scroll(0, PriceScaleTarget::Right, 100.0);
        chart.price_axis_scroll_to(0, PriceScaleTarget::Right, 110.0);
        let (new_from, new_to) = chart.price_scale_visible_range(0, false).unwrap();
        // +10 px down shifts the range up by 10 * span/(internalHeight-1); the internal height
        // is the scale height minus the fractional scale margins.
        let (margin_top, margin_bottom) = chart.price_scale_margins(0, false).unwrap();
        let internal_h = 300.0 * (1.0 - margin_top - margin_bottom);
        let shift = 10.0 * (to - from) / (internal_h - 1.0);
        assert!((new_from - (from + shift)).abs() < 1e-9);
        assert!((new_to - (to + shift)).abs() < 1e-9);
        chart.price_axis_end_scroll(0, PriceScaleTarget::Right);
    }

    #[test]
    fn price_pan_noops_under_autoscale() {
        let mut chart = chart_with_data(400.0, 300.0);
        let (from, to) = chart.price_scale_visible_range(0, false).unwrap();
        chart.price_axis_start_scroll(0, PriceScaleTarget::Right, 100.0);
        chart.price_axis_scroll_to(0, PriceScaleTarget::Right, 120.0);
        let (new_from, new_to) = chart.price_scale_visible_range(0, false).unwrap();
        assert_eq!((new_from, new_to), (from, to));
    }

    #[test]
    fn wheel_and_pinch_increments_match_the_reference_coefficients() {
        assert_eq!(wheel_zoom_scale(0.42), 0.42);
        assert_eq!(wheel_zoom_scale(-3.7), -1.0);
        assert_eq!(wheel_zoom_scale(0.0), 0.0);
        assert_eq!(pinch_zoom_scale(0.1), 0.5);
        assert_eq!(WHEEL_SCROLL_PX_PER_DELTA, -80.0);
        assert_eq!(PINCH_ZOOM_INTENSITY, 5.0);
    }

    #[test]
    fn scroll_animation_eases_to_the_target_and_clears() {
        let mut chart = chart_with_data(400.0, 300.0);
        chart.scroll_to_position(0.0);
        chart.start_scroll_animation(2.0, 200.0, 1000.0);
        assert!(chart.scroll_animation_active());
        // halfway: cubic ease-out(0.5) = 1 - 0.5^3 = 0.875 -> 0 + 2*0.875
        let mid = chart.scroll_animation_tick(1100.0).unwrap();
        assert!((mid - 1.75).abs() < 1e-9);
        assert!((chart.scroll_position() - 1.75).abs() < 1e-9);
        // completion applies the target and self-clears
        assert!(chart.scroll_animation_tick(1200.0).is_none());
        assert_eq!(chart.scroll_position(), 2.0);
        assert!(!chart.scroll_animation_active());
        assert!(chart.scroll_animation_tick(1300.0).is_none());
    }

    #[test]
    fn scroll_animation_cancel_and_zero_duration() {
        let mut chart = chart_with_data(400.0, 300.0);
        chart.start_scroll_animation(2.0, 200.0, 1000.0);
        chart.cancel_scroll_animation();
        assert!(!chart.scroll_animation_active());
        assert!(chart.scroll_animation_tick(1100.0).is_none());
        // zero duration jumps straight to the target on the first tick
        chart.start_scroll_animation(1.0, 0.0, 1000.0);
        assert!(chart.scroll_animation_tick(1000.0).is_none());
        assert_eq!(chart.scroll_position(), 1.0);
    }

    #[test]
    fn pane_index_at_y_uses_engine_pane_bounds() {
        let mut chart = chart_with_data(400.0, 300.0);
        chart.add_pane(true);
        chart.panes[1].stretch_factor = 0.5; // 2:1 split of 299 usable px (1px separator)
        chart.layout_panes(300.0);
        let first_h = chart.panes[0].height;
        assert_eq!(chart.pane_index_at_y(0.0), 0);
        assert_eq!(chart.pane_index_at_y(first_h - 1.0), 0);
        assert_eq!(chart.pane_index_at_y(first_h + 10.0), 1);
        assert_eq!(chart.pane_index_at_y(299.0), 1);
    }
}
