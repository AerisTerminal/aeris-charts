//! Invalidation system. Port of `src/model/invalidate-mask.ts`.
//!
//! Levels: `None < Cursor < Light < Full`. A mask carries a global level, per-pane levels
//! (with an optional autoscale flag), and a queue of time-scale invalidations that the host
//! applies in order on the next animation frame.

use std::collections::HashMap;

use crate::model::range::LogicalRange;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum InvalidationLevel {
    #[default]
    None = 0,
    Cursor = 1,
    Light = 2,
    Full = 3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneInvalidation {
    pub level: InvalidationLevel,
    pub auto_scale: bool,
}

fn merge_pane_invalidation(
    before: Option<PaneInvalidation>,
    new_value: PaneInvalidation,
) -> PaneInvalidation {
    match before {
        None => new_value,
        Some(prev) => PaneInvalidation {
            level: prev.level.max(new_value.level),
            auto_scale: prev.auto_scale || new_value.auto_scale,
        },
    }
}

/// Time-scale change requests queued into a mask.
///
/// The `Animation` variant carries an opaque id: the host owns animation objects (they need a
/// clock) and looks them up by id when applying the mask. This mirrors reference where the mask stores
/// an `ITimeScaleAnimation` reference.
#[derive(Clone, Debug, PartialEq)]
pub enum TimeScaleInvalidation {
    FitContent,
    ApplyRange(LogicalRange),
    ApplyBarSpacing(f64),
    ApplyRightOffset(f64),
    Reset,
    Animation(u64),
    StopAnimation,
}

#[derive(Clone, Debug, Default)]
pub struct InvalidateMask {
    invalidated_panes: HashMap<usize, PaneInvalidation>,
    global_level: InvalidationLevel,
    time_scale_invalidations: Vec<TimeScaleInvalidation>,
}

impl InvalidateMask {
    pub fn new(global_level: InvalidationLevel) -> Self {
        Self {
            invalidated_panes: HashMap::new(),
            global_level,
            time_scale_invalidations: Vec::new(),
        }
    }

    pub fn light() -> Self {
        Self::new(InvalidationLevel::Light)
    }

    pub fn full() -> Self {
        Self::new(InvalidationLevel::Full)
    }

    pub fn cursor() -> Self {
        Self::new(InvalidationLevel::Cursor)
    }

    pub fn invalidate_pane(&mut self, pane_index: usize, invalidation: PaneInvalidation) {
        let prev = self.invalidated_panes.get(&pane_index).copied();
        self.invalidated_panes
            .insert(pane_index, merge_pane_invalidation(prev, invalidation));
    }

    pub fn full_invalidation(&self) -> InvalidationLevel {
        self.global_level
    }

    pub fn invalidate_for_pane(&self, pane_index: usize) -> PaneInvalidation {
        match self.invalidated_panes.get(&pane_index) {
            None => PaneInvalidation {
                level: self.global_level,
                auto_scale: false,
            },
            Some(pane) => PaneInvalidation {
                level: self.global_level.max(pane.level),
                auto_scale: pane.auto_scale,
            },
        }
    }

    pub fn set_fit_content(&mut self) {
        self.stop_time_scale_animation();
        // modifies both bar spacing and right offset -> replaces the whole queue
        self.time_scale_invalidations = vec![TimeScaleInvalidation::FitContent];
    }

    pub fn apply_range(&mut self, range: LogicalRange) {
        self.stop_time_scale_animation();
        self.time_scale_invalidations = vec![TimeScaleInvalidation::ApplyRange(range)];
    }

    pub fn set_time_scale_animation(&mut self, animation_id: u64) {
        self.remove_time_scale_animation();
        self.time_scale_invalidations
            .push(TimeScaleInvalidation::Animation(animation_id));
    }

    pub fn stop_time_scale_animation(&mut self) {
        self.remove_time_scale_animation();
        self.time_scale_invalidations
            .push(TimeScaleInvalidation::StopAnimation);
    }

    pub fn reset_time_scale(&mut self) {
        self.stop_time_scale_animation();
        self.time_scale_invalidations = vec![TimeScaleInvalidation::Reset];
    }

    pub fn set_bar_spacing(&mut self, bar_spacing: f64) {
        self.stop_time_scale_animation();
        self.time_scale_invalidations
            .push(TimeScaleInvalidation::ApplyBarSpacing(bar_spacing));
    }

    pub fn set_right_offset(&mut self, offset: f64) {
        self.stop_time_scale_animation();
        self.time_scale_invalidations
            .push(TimeScaleInvalidation::ApplyRightOffset(offset));
    }

    pub fn time_scale_invalidations(&self) -> &[TimeScaleInvalidation] {
        &self.time_scale_invalidations
    }

    pub fn merge(&mut self, other: &InvalidateMask) {
        for ts_invalidation in &other.time_scale_invalidations {
            self.apply_time_scale_invalidation(ts_invalidation.clone());
        }

        self.global_level = self.global_level.max(other.global_level);
        for (&index, &invalidation) in &other.invalidated_panes {
            self.invalidate_pane(index, invalidation);
        }
    }

    fn apply_time_scale_invalidation(&mut self, invalidation: TimeScaleInvalidation) {
        match invalidation {
            TimeScaleInvalidation::FitContent => self.set_fit_content(),
            TimeScaleInvalidation::ApplyRange(range) => self.apply_range(range),
            TimeScaleInvalidation::ApplyBarSpacing(v) => self.set_bar_spacing(v),
            TimeScaleInvalidation::ApplyRightOffset(v) => self.set_right_offset(v),
            TimeScaleInvalidation::Reset => self.reset_time_scale(),
            TimeScaleInvalidation::Animation(id) => self.set_time_scale_animation(id),
            TimeScaleInvalidation::StopAnimation => self.remove_time_scale_animation(),
        }
    }

    fn remove_time_scale_animation(&mut self) {
        if let Some(index) = self
            .time_scale_invalidations
            .iter()
            .position(|inv| matches!(inv, TimeScaleInvalidation::Animation(_)))
        {
            self.time_scale_invalidations.remove(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_takes_max_level() {
        let mut a = InvalidateMask::cursor();
        let b = InvalidateMask::full();
        a.merge(&b);
        assert_eq!(a.full_invalidation(), InvalidationLevel::Full);
    }

    #[test]
    fn pane_invalidation_merges_autoscale() {
        let mut m = InvalidateMask::light();
        m.invalidate_pane(
            0,
            PaneInvalidation {
                level: InvalidationLevel::None,
                auto_scale: true,
            },
        );
        m.invalidate_pane(
            0,
            PaneInvalidation {
                level: InvalidationLevel::Cursor,
                auto_scale: false,
            },
        );
        let p = m.invalidate_for_pane(0);
        // global light dominates pane cursor level; autoscale flag is sticky
        assert_eq!(p.level, InvalidationLevel::Light);
        assert!(p.auto_scale);
    }

    #[test]
    fn fit_content_replaces_queue() {
        let mut m = InvalidateMask::light();
        m.set_bar_spacing(7.0);
        m.set_right_offset(3.0);
        m.set_fit_content();
        assert_eq!(
            m.time_scale_invalidations(),
            &[TimeScaleInvalidation::FitContent]
        );
    }

    #[test]
    fn animation_is_replaced_not_duplicated() {
        let mut m = InvalidateMask::light();
        m.set_time_scale_animation(1);
        m.set_time_scale_animation(2);
        let animations: Vec<_> = m
            .time_scale_invalidations()
            .iter()
            .filter(|i| matches!(i, TimeScaleInvalidation::Animation(_)))
            .collect();
        assert_eq!(animations.len(), 1);
        assert_eq!(animations[0], &TimeScaleInvalidation::Animation(2));
    }
}
