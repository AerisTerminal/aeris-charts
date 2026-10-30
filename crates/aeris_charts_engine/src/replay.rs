// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 Devraj Mahar (Aeris Terminal). Additional terms apply; see NOTICE.

//! Bar replay: trim the chart at a chosen bar, then reveal the hidden history again bar by bar
//! or by a fixed time interval.
//!
//! Replay drives the one chart replay clock ([`ChartEngine::set_replay_clock_micros`]), so every
//! canonical projection (rows, trade streams, depth, synthetic and resampled bars, studies) hides
//! the future through the same boundary and nothing is discarded. This module owns only the mode:
//! the trim selection, the bar timeline steps move along, and the playback cadence on the host
//! input clock ([`ChartEngine::input_wake_deadline_ms`] / [`ChartEngine::input_tick`]). Toolbars
//! stay host-owned and drive the commands below; every state change queues
//! [`ChartInputEvent::ReplayChanged`].

use crate::{ChartEngine, ChartError, ChartInputEvent, ErrorCode};

const MICROS_PER_SECOND: i64 = 1_000_000;
/// Slowest playback: one step every ten seconds.
pub const MIN_REPLAY_SPEED: f64 = 0.1;
/// Fastest playback: ten steps per second.
pub const MAX_REPLAY_SPEED: f64 = 10.0;
/// Opacity of the background wash over the bars a trim would hide.
pub(crate) const REPLAY_DIM_ALPHA: u8 = 0x99;
/// Largest fixed-interval step (one leap year).
pub const MAX_REPLAY_INTERVAL_MICROS: i64 = 366 * 86_400 * MICROS_PER_SECOND;

/// Where the replay mode is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReplayPhase {
    #[default]
    Off,
    /// The trim cursor is armed; the next pane click chooses the last visible bar.
    Selecting,
    Paused,
    Playing,
}

/// How far one replay step moves the clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReplayStep {
    /// Reveal exactly one more bar (or hide one, stepping back).
    #[default]
    Bar,
    /// Move the clock by a fixed interval, revealing intrabar trades on trade-stream charts.
    Interval { micros: i64 },
}

/// The replay state a host toolbar renders.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplaySnapshot {
    pub phase: ReplayPhase,
    /// The chart replay clock; `None` shows all data.
    pub clock_micros: Option<i64>,
    /// Steps per second during playback.
    pub speed: f64,
    pub step: ReplayStep,
    pub can_step_back: bool,
    /// `false` once replay has revealed all retained history.
    pub can_step_forward: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ReplayState {
    phase: ReplayPhase,
    /// The phase a cancelled selection returns to.
    selection_return: ReplayPhase,
    /// The host clock before replay began; [`ChartEngine::replay_exit`] restores it.
    entry_clock_micros: Option<i64>,
    speed: f64,
    step: ReplayStep,
    /// Host-clock time of the next playback step. The engine owns no clock, so playback starts
    /// without one and the first tick anchors it.
    next_step_ms: Option<f64>,
}

impl Default for ReplayState {
    fn default() -> Self {
        Self {
            phase: ReplayPhase::Off,
            selection_return: ReplayPhase::Off,
            entry_clock_micros: None,
            speed: 1.0,
            step: ReplayStep::Bar,
            next_step_ms: None,
        }
    }
}

fn no_bars() -> ChartError {
    ChartError::new(ErrorCode::InvalidData, "no bar to start bar replay from")
}

fn not_active() -> ChartError {
    ChartError::new(
        ErrorCode::UnsupportedOperation,
        "bar replay has no start bar; select one first",
    )
}

impl ChartEngine {
    pub fn replay_state(&self) -> ReplaySnapshot {
        let clock = self.replay_clock_micros();
        let can_step = |forward| {
            self.replay_active()
                && clock.is_some_and(|clock| self.replay_step_target(clock, forward).is_some())
        };
        ReplaySnapshot {
            phase: self.replay.phase,
            clock_micros: clock,
            speed: self.replay.speed,
            step: self.replay.step,
            can_step_back: can_step(false),
            can_step_forward: can_step(true),
        }
    }

    fn ensure_replay_supported(&self) -> Result<(), ChartError> {
        if self.data.merged_times().is_empty() || !self.pane_uses_financial_time(0) {
            return Err(ChartError::new(
                ErrorCode::UnsupportedOperation,
                "bar replay needs bars on a financial time pane",
            ));
        }
        Ok(())
    }

    /// Remember the host clock the first time replay takes it over.
    fn note_replay_entry(&mut self) {
        if self.replay.phase == ReplayPhase::Off {
            self.replay.entry_clock_micros = self.replay_clock_micros();
        }
    }

    pub(crate) fn replay_mode_on(&self) -> bool {
        self.replay.phase != ReplayPhase::Off
    }

    pub(crate) fn replay_selecting(&self) -> bool {
        self.replay.phase == ReplayPhase::Selecting
    }

    pub(crate) fn replay_active(&self) -> bool {
        matches!(
            self.replay.phase,
            ReplayPhase::Paused | ReplayPhase::Playing
        )
    }

    /// Arm the trim cursor. From an active replay this re-selects the start bar; playback pauses
    /// and a cancelled selection returns to the paused replay.
    pub fn replay_select_start(&mut self) -> Result<(), ChartError> {
        self.ensure_replay_supported()?;
        match self.replay.phase {
            ReplayPhase::Selecting => return Ok(()),
            ReplayPhase::Off => {
                self.replay.selection_return = ReplayPhase::Off;
                self.replay.entry_clock_micros = self.replay_clock_micros();
            }
            ReplayPhase::Paused | ReplayPhase::Playing => {
                self.replay.selection_return = ReplayPhase::Paused;
            }
        }
        self.cancel_drawing_tool();
        self.replay.phase = ReplayPhase::Selecting;
        self.replay.next_step_ms = None;
        self.note_replay_changed();
        Ok(())
    }

    /// Leave the trim cursor without choosing a bar. Returns whether a selection was armed.
    pub fn replay_cancel_selection(&mut self) -> bool {
        if !self.replay_selecting() {
            return false;
        }
        self.replay.phase = self.replay.selection_return;
        self.note_replay_changed();
        true
    }

    /// Trim at the bar containing `time_micros` (the first bar when it precedes the data) and
    /// pause there, from any phase. The time may lie behind the current replay clock.
    pub fn replay_select_time(&mut self, time_micros: i64) -> Result<(), ChartError> {
        self.ensure_replay_supported()?;
        let (start, end) = self.replay_bounds().ok_or_else(no_bars)?;
        let probe = time_micros.clamp(start, end);
        self.note_replay_entry();
        let index = if self.sequence_points().is_some() {
            // Sequence bars exist only through their prints, so projecting at the time makes its
            // bar the newest one; the click path then completes it.
            self.apply_replay_clock(Some(probe))?;
            self.data.merged_times().len().checked_sub(1)
        } else {
            // A time bar may open before its first print, so it is found by time among bars
            // the clock does not hide.
            if self
                .replay_clock_micros()
                .is_some_and(|clock| clock < probe)
            {
                self.apply_replay_clock(Some(end))?;
            }
            let seconds = probe.div_euclid(MICROS_PER_SECOND);
            self.data
                .merged_times()
                .partition_point(|&time| time <= seconds)
                .checked_sub(1)
        };
        self.replay_select_index(index.ok_or_else(no_bars)?)
    }

    /// Trim at the bar under pane x. The pointer may sit in the empty area past the newest bar,
    /// which selects that bar.
    pub(crate) fn replay_select_at(&mut self, x: f64) -> bool {
        self.replay_index_at(x)
            .is_some_and(|index| self.replay_select_index(index).is_ok())
    }

    /// The bar the trim cursor addresses at pane x, clamped to the bars on the chart.
    pub(crate) fn replay_index_at(&self, x: f64) -> Option<usize> {
        let len = self.data.merged_times().len();
        if len == 0 || !x.is_finite() {
            return None;
        }
        Some(
            self.time_scale
                .coordinate_to_index(x)
                .clamp(0, len as i64 - 1) as usize,
        )
    }

    fn replay_select_index(&mut self, index: usize) -> Result<(), ChartError> {
        let clock = self.replay_bar_close(index).ok_or_else(no_bars)?;
        self.note_replay_entry();
        self.apply_replay_clock(Some(clock))?;
        self.replay.phase = ReplayPhase::Paused;
        self.replay.next_step_ms = None;
        self.note_replay_changed();
        Ok(())
    }

    pub fn replay_play(&mut self) -> Result<(), ChartError> {
        match self.replay.phase {
            ReplayPhase::Playing => return Ok(()),
            ReplayPhase::Paused => {}
            ReplayPhase::Off | ReplayPhase::Selecting => return Err(not_active()),
        }
        let at_end = self
            .replay_clock_micros()
            .is_none_or(|clock| self.replay_step_target(clock, true).is_none());
        if at_end {
            return Ok(());
        }
        self.replay.phase = ReplayPhase::Playing;
        self.replay.next_step_ms = None;
        self.note_replay_changed();
        Ok(())
    }

    /// Returns whether playback was running.
    pub fn replay_pause(&mut self) -> bool {
        if self.replay.phase != ReplayPhase::Playing {
            return false;
        }
        self.replay.phase = ReplayPhase::Paused;
        self.replay.next_step_ms = None;
        self.note_replay_changed();
        true
    }

    pub fn replay_toggle_play(&mut self) -> Result<(), ChartError> {
        if self.replay_pause() {
            Ok(())
        } else {
            self.replay_play()
        }
    }

    /// One step forward. Returns whether the clock moved; at the end of the data it does not.
    pub fn replay_step_forward(&mut self) -> Result<bool, ChartError> {
        self.replay_step(true)
    }

    /// One step back, always keeping at least one bar.
    pub fn replay_step_back(&mut self) -> Result<bool, ChartError> {
        self.replay_step(false)
    }

    /// Playback speed in steps per second, `MIN_REPLAY_SPEED..=MAX_REPLAY_SPEED`.
    pub fn set_replay_speed(&mut self, speed: f64) -> Result<(), ChartError> {
        if !(MIN_REPLAY_SPEED..=MAX_REPLAY_SPEED).contains(&speed) {
            return Err(ChartError::new(
                ErrorCode::InvalidOptions,
                "replay speed must be between 0.1 and 10 steps per second",
            ));
        }
        if self.replay.speed != speed {
            self.replay.speed = speed;
            self.replay.next_step_ms = None;
            self.note_replay_changed();
        }
        Ok(())
    }

    pub fn set_replay_step(&mut self, step: ReplayStep) -> Result<(), ChartError> {
        if let ReplayStep::Interval { micros } = step
            && !(1..=MAX_REPLAY_INTERVAL_MICROS).contains(&micros)
        {
            return Err(ChartError::new(
                ErrorCode::InvalidOptions,
                "replay interval must be between 1 microsecond and 366 days",
            ));
        }
        if self.replay.step != step {
            self.replay.step = step;
            self.note_replay_changed();
        }
        Ok(())
    }

    /// Leave replay showing all data, live updates included.
    pub fn replay_jump_to_realtime(&mut self) -> Result<(), ChartError> {
        self.replay_finish(None)
    }

    /// Leave replay and restore the clock the host had before replay began.
    pub fn replay_exit(&mut self) -> Result<(), ChartError> {
        self.replay_finish(self.replay.entry_clock_micros)
    }

    fn replay_finish(&mut self, clock: Option<i64>) -> Result<(), ChartError> {
        if self.replay.phase == ReplayPhase::Off {
            return Ok(());
        }
        self.apply_replay_clock(clock)?;
        self.replay.phase = ReplayPhase::Off;
        self.replay.entry_clock_micros = None;
        self.replay.next_step_ms = None;
        self.note_replay_changed();
        Ok(())
    }

    /// The deadline playback needs from the host input clock; `0.0` asks for an immediate wake
    /// that anchors the cadence.
    pub(crate) fn replay_wake_deadline_ms(&self) -> Option<f64> {
        (self.replay.phase == ReplayPhase::Playing).then(|| self.replay.next_step_ms.unwrap_or(0.0))
    }

    /// Advance playback by at most one step per tick: a late or throttled host re-anchors the
    /// cadence instead of bursting through skipped steps. Returns whether state changed.
    pub(crate) fn tick_replay(&mut self, now_ms: f64) -> bool {
        if self.replay.phase != ReplayPhase::Playing || !now_ms.is_finite() {
            return false;
        }
        let interval = 1_000.0 / self.replay.speed;
        let Some(deadline) = self.replay.next_step_ms else {
            self.replay.next_step_ms = Some(now_ms + interval);
            return false;
        };
        if now_ms < deadline {
            return false;
        }
        let target = self
            .replay_clock_micros()
            .and_then(|clock| self.replay_step_target(clock, true));
        let stepped = target.is_some_and(|target| self.apply_replay_clock(Some(target)).is_ok());
        if stepped && target.is_some_and(|target| self.replay_step_target(target, true).is_some()) {
            self.replay.next_step_ms = Some(if now_ms - deadline >= interval {
                now_ms + interval
            } else {
                deadline + interval
            });
        } else {
            self.replay.phase = ReplayPhase::Paused;
            self.replay.next_step_ms = None;
        }
        self.note_replay_changed();
        true
    }

    fn replay_step(&mut self, forward: bool) -> Result<bool, ChartError> {
        if !self.replay_active() {
            return Err(not_active());
        }
        let Some(target) = self
            .replay_clock_micros()
            .and_then(|clock| self.replay_step_target(clock, forward))
        else {
            return Ok(false);
        };
        self.apply_replay_clock(Some(target))?;
        if self.replay.phase == ReplayPhase::Playing {
            self.replay.next_step_ms = None;
        }
        self.note_replay_changed();
        Ok(true)
    }

    fn apply_replay_clock(&mut self, clock: Option<i64>) -> Result<(), ChartError> {
        self.set_replay_clock_micros(clock)
            .map(|_| ())
            .map_err(|error| ChartError::new(ErrorCode::InvalidData, error.to_string()))
    }

    fn note_replay_changed(&mut self) {
        self.push_input_event(ChartInputEvent::ReplayChanged);
        // The trading layer owns the host-clock replay marker, which bar replay hides.
        self.invalidate_frame_trading();
        self.invalidate_frame_overlay();
        self.invalidate_frame_axis();
        self.refresh_input_cursor();
    }

    fn replay_step_target(&self, clock: i64, forward: bool) -> Option<i64> {
        match (self.replay.step, forward) {
            (ReplayStep::Bar, true) => self.replay_next_bar_clock(clock),
            (ReplayStep::Bar, false) => self.replay_previous_bar_clock(clock),
            (ReplayStep::Interval { micros }, true) => {
                let (_, end) = self.replay_bounds()?;
                (clock < end).then(|| clock.saturating_add(micros).min(end))
            }
            (ReplayStep::Interval { micros }, false) => {
                let (start, _) = self.replay_bounds()?;
                (clock > start).then(|| clock.saturating_sub(micros).max(start))
            }
        }
    }

    // --- the bar timeline ---
    //
    // Row-backed bars are atomic: a row is complete once the clock reaches its time, and stays the
    // last one until the clock reaches the next row. Trade-stream bars complete at their newest
    // print. With several sources the earliest completion wins, so one step reveals one bar on
    // every source that shares the timeline.

    /// Canonical rows take part only on the time axis; a sequence axis is projected from its
    /// owning trade stream or synthetic series instead.
    fn replay_rows_on_timeline(&self) -> bool {
        self.sequence_points().is_none()
    }

    /// The clock that completes the row-backed bar at `time` seconds.
    fn replay_row_close(&self, time: i64) -> i64 {
        self.data.canonical_time_after(time).map_or_else(
            || time.saturating_mul(MICROS_PER_SECOND),
            |next| next.saturating_mul(MICROS_PER_SECOND) - 1,
        )
    }

    /// The clock that completes the bar at chart index `index`. Only a following row or a trade
    /// print bounds it; the newest row alone needs just its own time. A partly revealed newest
    /// sequence bar completes at the last print that still joins it.
    fn replay_bar_close(&self, index: usize) -> Option<i64> {
        if let Some(points) = self.sequence_points() {
            let close = points.get(index)?.close_timestamp_micros;
            let forming = (index + 1 == points.len())
                .then(|| self.replay_clock_micros())
                .flatten()
                .and_then(|clock| {
                    self.trade_streams
                        .values()
                        .filter_map(|stream| stream.replay_forming_bar_close(clock))
                        .min()
                });
            return Some(forming.map_or(close, |forming| forming.max(close)));
        }
        let time = *self.data.merged_times().get(index)?;
        let time_micros = time.saturating_mul(MICROS_PER_SECOND);
        let next_row = self
            .data
            .canonical_time_after(time)
            .map(|next| next.saturating_mul(MICROS_PER_SECOND) - 1);
        let bounded = next_row
            .into_iter()
            .chain(
                self.trade_streams
                    .values()
                    .filter_map(|stream| stream.replay_time_bar_close(time_micros)),
            )
            .filter(|&close| close >= time_micros)
            .min();
        Some(bounded.unwrap_or(time_micros))
    }

    fn replay_next_bar_clock(&self, clock: i64) -> Option<i64> {
        let rows = self
            .replay_rows_on_timeline()
            .then(|| {
                self.data
                    .canonical_time_after(clock.div_euclid(MICROS_PER_SECOND))
            })
            .flatten()
            .map(|time| self.replay_row_close(time));
        let streams = self
            .trade_streams
            .values()
            .filter_map(|stream| stream.replay_next_bar_close(clock));
        let synthetic = self.synthetic_series.values().filter_map(|series| {
            let source = series.source();
            source
                .get(source.partition_point(|bar| bar.timestamp_micros <= clock))
                .map(|bar| bar.timestamp_micros)
        });
        rows.into_iter()
            .chain(streams)
            .chain(synthetic)
            .filter(|&target| target > clock)
            .min()
    }

    fn replay_previous_bar_clock(&self, clock: i64) -> Option<i64> {
        let rows = self
            .replay_rows_on_timeline()
            .then(|| {
                let newest = self
                    .data
                    .canonical_time_before(clock.div_euclid(MICROS_PER_SECOND).saturating_add(1))?;
                self.data.canonical_time_before(newest)?;
                Some(newest.saturating_mul(MICROS_PER_SECOND) - 1)
            })
            .flatten();
        let streams = self
            .trade_streams
            .values()
            .filter_map(|stream| stream.replay_previous_bar_clock(clock));
        let synthetic = self.synthetic_series.values().filter_map(|series| {
            let source = series.source();
            let visible = source.partition_point(|bar| bar.timestamp_micros <= clock);
            (visible >= 2).then(|| source[visible - 1].timestamp_micros - 1)
        });
        rows.into_iter()
            .chain(streams)
            .chain(synthetic)
            .filter(|&target| target < clock)
            .max()
    }

    /// The earliest clock that shows one bar and the clock that shows all retained history.
    fn replay_bounds(&self) -> Option<(i64, i64)> {
        let rows = self
            .replay_rows_on_timeline()
            .then(|| self.data.canonical_time_bounds())
            .flatten()
            .map(|(first, last)| {
                (
                    first.saturating_mul(MICROS_PER_SECOND),
                    last.saturating_mul(MICROS_PER_SECOND),
                )
            });
        let streams = self
            .trade_streams
            .values()
            .filter_map(|stream| stream.replay_bounds());
        let synthetic = self.synthetic_series.values().filter_map(|series| {
            let source = series.source();
            Some((
                source.first()?.timestamp_micros,
                source.last()?.timestamp_micros,
            ))
        });
        rows.into_iter()
            .chain(streams)
            .chain(synthetic)
            .reduce(|(first, last), (start, end)| (first.min(start), last.max(end)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AggressorSide, ChartCursor, ChartKey, FootprintAggregationOptions, FootprintBarAggregation,
        FootprintTrade, InputModifiers, PointerInput, ResampleBoundary, ResampleOptions,
        SeriesKind,
    };
    use aeris_charts_render::draw_list::{LineStyle, Prim};

    const M: i64 = MICROS_PER_SECOND;
    const BARS: i64 = 20;

    fn time(index: i64) -> i64 {
        1_000 + index * 60
    }

    fn chart() -> ChartEngine {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let times: Vec<f64> = (0..BARS).map(|i| time(i) as f64).collect();
        let open: Vec<f64> = (0..BARS).map(|i| 100.0 + i as f64).collect();
        let high: Vec<f64> = open.iter().map(|value| value + 2.0).collect();
        let low: Vec<f64> = open.iter().map(|value| value - 2.0).collect();
        let close: Vec<f64> = open.iter().map(|value| value + 1.0).collect();
        chart
            .set_series_data(0, &times, &open, &high, &low, &close)
            .unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 48.0, |_, _| 48.0);
        chart.fit_content();
        chart.autoscale_visible();
        chart
    }

    fn at(x: f64, y: f64) -> PointerInput {
        PointerInput {
            x,
            y,
            ..PointerInput::default()
        }
    }

    fn shift() -> InputModifiers {
        InputModifiers {
            shift: true,
            ..InputModifiers::default()
        }
    }

    fn bars(chart: &ChartEngine) -> usize {
        chart.data.merged_times().len()
    }

    fn count(frame: &crate::ChartFrame, test: impl Fn(&Prim) -> bool) -> usize {
        frame.panes[0].main.iter().filter(|prim| test(prim)).count()
    }

    #[test]
    fn trim_click_cuts_at_the_bar_and_paints_only_the_trim_cursor() {
        let mut chart = chart();
        let (x, y) = (chart.time_scale.index_to_coordinate(7), 200.0);
        chart.input_pointer_move(at(x, y), false);
        let crosshair_frame = chart.build_frame();
        let hlines = |frame: &crate::ChartFrame| count(frame, |p| matches!(p, Prim::HLine { .. }));

        chart.replay_select_start().unwrap();
        assert_eq!(chart.replay_state().phase, ReplayPhase::Selecting);
        assert!(
            chart
                .take_input_events()
                .contains(&ChartInputEvent::ReplayChanged)
        );
        chart.input_pointer_move(at(x, y), false);
        assert_eq!(chart.input_cursor(), ChartCursor::Trim);
        let trim_frame = chart.build_frame();
        assert_eq!(hlines(&trim_frame) + 1, hlines(&crosshair_frame));
        assert_eq!(
            count(&trim_frame, |p| matches!(
                p,
                Prim::VLine {
                    width: 3,
                    style: LineStyle::Solid,
                    ..
                }
            )),
            1
        );
        assert_eq!(count(&trim_frame, |p| matches!(p, Prim::Image { .. })), 1);
        assert_eq!(
            count(
                &trim_frame,
                |p| matches!(p, Prim::Rect { color, .. } if color.a() == REPLAY_DIM_ALPHA)
            ),
            1
        );

        chart.input_pointer_down(at(x, y), 1);
        chart.input_pointer_up(at(x, y));
        let events = chart.take_input_events();
        assert!(events.contains(&ChartInputEvent::ReplayChanged));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ChartInputEvent::Click { .. }))
        );
        let state = chart.replay_state();
        assert_eq!(state.phase, ReplayPhase::Paused);
        assert_eq!(state.clock_micros, Some(time(8) * M - 1));
        assert!(state.can_step_back && state.can_step_forward);
        assert_eq!(bars(&chart), 8);
        assert_eq!(chart.input_cursor(), ChartCursor::Crosshair);
        let replay_marker = |frame: &crate::ChartFrame| {
            count(
                frame,
                |p| matches!(p, Prim::Text { text, .. } if text == "Replay"),
            )
        };
        assert_eq!(replay_marker(&chart.build_frame()), 0);
        chart.replay_exit().unwrap();
        chart.set_replay_clock_micros(Some(time(8) * M)).unwrap();
        assert_eq!(replay_marker(&chart.build_frame()), 1);
        chart.replay_select_start().unwrap();
        assert_eq!(replay_marker(&chart.build_frame()), 0);
        assert!(chart.replay_cancel_selection());
        assert_eq!(replay_marker(&chart.build_frame()), 1);
    }

    #[test]
    fn a_trim_drag_pans_instead_of_selecting() {
        let mut chart = chart();
        chart.replay_select_start().unwrap();
        let before = chart.scroll_position();
        chart.input_pointer_down(at(400.0, 200.0), 1);
        for step in 1..=4 {
            chart.input_pointer_move(at(400.0 + 30.0 * f64::from(step), 200.0), true);
        }
        chart.input_pointer_up(at(520.0, 200.0));
        assert_ne!(chart.scroll_position(), before);
        assert_eq!(chart.replay_state().phase, ReplayPhase::Selecting);
        assert_eq!(bars(&chart), BARS as usize);
    }

    #[test]
    fn bar_steps_reveal_and_hide_one_bar_and_stop_at_both_ends() {
        let mut chart = chart();
        chart.replay_select_time(time(3) * M + 5).unwrap();
        assert_eq!(chart.replay_clock_micros(), Some(time(4) * M - 1));
        assert_eq!(bars(&chart), 4);
        assert!(chart.replay_step_forward().unwrap());
        assert_eq!(bars(&chart), 5);
        assert_eq!(chart.replay_clock_micros(), Some(time(5) * M - 1));
        for expected in [4, 3, 2, 1] {
            assert!(chart.replay_step_back().unwrap());
            assert_eq!(bars(&chart), expected);
        }
        assert!(!chart.replay_step_back().unwrap());
        assert!(!chart.replay_state().can_step_back);

        chart.replay_select_time(time(18) * M).unwrap();
        assert_eq!(bars(&chart), 19);
        assert!(chart.replay_step_forward().unwrap());
        assert_eq!(chart.replay_clock_micros(), Some(time(19) * M));
        assert_eq!(bars(&chart), 20);
        assert!(!chart.replay_step_forward().unwrap());
        assert!(!chart.replay_state().can_step_forward);
    }

    #[test]
    fn playback_runs_on_the_host_input_clock_and_pauses_at_the_end() {
        let mut chart = chart();
        assert!(chart.replay_play().is_err());
        chart.replay_select_time(time(2) * M).unwrap();
        chart
            .set_replay_step(ReplayStep::Interval { micros: 30 * M })
            .unwrap();
        assert!(chart.replay_step_forward().unwrap());
        assert_eq!(chart.replay_clock_micros(), Some(time(3) * M + 30 * M - 1));
        assert_eq!(bars(&chart), 4);
        chart.set_replay_step(ReplayStep::Bar).unwrap();
        assert!(chart.set_replay_speed(0.0).is_err());
        assert!(chart.set_replay_speed(11.0).is_err());
        chart.set_replay_speed(2.0).unwrap();

        chart.replay_play().unwrap();
        assert_eq!(chart.replay_state().phase, ReplayPhase::Playing);
        assert_eq!(chart.input_wake_deadline_ms(), Some(0.0));
        assert!(!chart.input_tick(1_000.0));
        assert_eq!(chart.input_wake_deadline_ms(), Some(1_500.0));
        assert!(!chart.input_tick(1_499.0));
        assert!(chart.input_tick(1_500.0));
        assert_eq!(bars(&chart), 5);
        assert_eq!(chart.input_wake_deadline_ms(), Some(2_000.0));
        // A throttled host gets one step and a fresh cadence, not a burst.
        assert!(chart.input_tick(10_000.0));
        assert_eq!(bars(&chart), 6);
        assert_eq!(chart.input_wake_deadline_ms(), Some(10_500.0));

        assert!(chart.input_key_down(ChartKey::ArrowDown, shift(), false, 0.0));
        assert_eq!(chart.replay_state().phase, ReplayPhase::Paused);
        assert_eq!(chart.input_wake_deadline_ms(), None);
        assert!(chart.input_key_down(ChartKey::ArrowRight, shift(), false, 0.0));
        assert_eq!(bars(&chart), 7);
        assert!(chart.input_key_down(ChartKey::ArrowLeft, shift(), false, 0.0));
        assert_eq!(bars(&chart), 6);

        chart.replay_select_time(time(18) * M).unwrap();
        chart.set_replay_speed(MAX_REPLAY_SPEED).unwrap();
        chart.replay_play().unwrap();
        assert!(!chart.input_tick(0.0));
        assert!(chart.input_tick(100.0));
        let state = chart.replay_state();
        assert_eq!(state.phase, ReplayPhase::Paused);
        assert!(!state.can_step_forward);
        assert_eq!(bars(&chart), 20);
        chart.replay_play().unwrap();
        assert_eq!(chart.replay_state().phase, ReplayPhase::Paused);
    }

    #[test]
    fn escape_restores_the_prior_phase_and_exit_restores_the_host_clock() {
        let mut chart = chart();
        chart.set_replay_clock_micros(Some(time(15) * M)).unwrap();
        chart.replay_select_start().unwrap();
        assert!(chart.input_key_down(ChartKey::Escape, InputModifiers::default(), false, 0.0));
        assert_eq!(chart.replay_state().phase, ReplayPhase::Off);
        assert_eq!(chart.replay_clock_micros(), Some(time(15) * M));

        chart.replay_select_start().unwrap();
        chart.replay_select_time(time(5) * M).unwrap();
        chart.replay_play().unwrap();
        chart.replay_select_start().unwrap();
        assert_eq!(chart.input_wake_deadline_ms(), None);
        assert!(chart.replay_cancel_selection());
        assert_eq!(chart.replay_state().phase, ReplayPhase::Paused);
        assert_eq!(chart.replay_clock_micros(), Some(time(6) * M - 1));

        chart.replay_exit().unwrap();
        assert_eq!(chart.replay_state().phase, ReplayPhase::Off);
        assert_eq!(chart.replay_clock_micros(), Some(time(15) * M));
        chart.replay_select_time(time(5) * M).unwrap();
        chart.replay_jump_to_realtime().unwrap();
        assert_eq!(chart.replay_clock_micros(), None);
        assert_eq!(bars(&chart), BARS as usize);
        assert!(chart.replay_step_forward().is_err());
    }

    fn trade(seconds: i64, price: f64) -> FootprintTrade {
        FootprintTrade {
            timestamp_micros: seconds * M,
            price,
            volume: 1.0,
            aggressor: AggressorSide::Buy,
            bid: None,
            ask: None,
            sequence: Some(seconds as u64),
            trade_id: Some(seconds as u64),
            conditions: 0,
            session_id: Some(1),
        }
    }

    fn trade_chart(
        bars: FootprintBarAggregation,
        trades: Vec<FootprintTrade>,
    ) -> (ChartEngine, crate::SeriesId) {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let stream = chart
            .add_trade_stream(
                "TEST:REPLAY",
                FootprintAggregationOptions {
                    tick_size: 0.25,
                    ticks_per_row: 1,
                    bars,
                    ..FootprintAggregationOptions::default()
                },
            )
            .unwrap();
        let candles = chart.add_series(SeriesKind::Candlestick);
        chart
            .bind_trade_bar_series_to_stream(candles, stream)
            .unwrap();
        chart.set_trade_stream_trades(stream, trades).unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 48.0, |_, _| 48.0);
        chart.fit_content();
        (chart, candles)
    }

    fn rows(chart: &ChartEngine, id: crate::SeriesId) -> (usize, f64) {
        let (times, columns) = chart.data.series_data(id).unwrap();
        (times.len(), columns[3].last().copied().unwrap_or(f64::NAN))
    }

    #[test]
    fn trade_time_bars_step_by_bar_and_by_intrabar_interval() {
        let trades = (0..5)
            .flat_map(|minute| {
                [5, 20, 40].map(|second| trade(minute * 60 + second, 100.0 + second as f64 / 4.0))
            })
            .collect();
        let (mut chart, candles) = trade_chart(
            FootprintBarAggregation::Time {
                interval_micros: 60_000_000,
                anchor_micros: 0,
            },
            trades,
        );
        chart.replay_select_time(60 * M).unwrap();
        assert_eq!(chart.replay_clock_micros(), Some(100 * M));
        assert_eq!(rows(&chart, candles).0, 2);
        assert!(chart.replay_step_forward().unwrap());
        assert_eq!(chart.replay_clock_micros(), Some(160 * M));
        assert_eq!(rows(&chart, candles).0, 3);

        chart
            .set_replay_step(ReplayStep::Interval { micros: 10 * M })
            .unwrap();
        assert!(chart.replay_step_forward().unwrap());
        assert!(chart.replay_step_forward().unwrap());
        assert_eq!(rows(&chart, candles).0, 3);
        assert!(chart.replay_step_forward().unwrap());
        // The fourth bar forms from its first print only.
        assert_eq!(rows(&chart, candles), (4, 101.25));

        chart.set_replay_step(ReplayStep::Bar).unwrap();
        assert!(chart.replay_step_forward().unwrap());
        assert_eq!(chart.replay_clock_micros(), Some(220 * M));
        assert_eq!(rows(&chart, candles), (4, 110.0));
        assert!(chart.replay_step_back().unwrap());
        assert_eq!(chart.replay_clock_micros(), Some(180 * M - 1));
        assert_eq!(rows(&chart, candles).0, 3);
    }

    #[test]
    fn trade_count_bars_step_along_their_sequence_axis() {
        let trades = (1..=8).map(|second| trade(second, 100.0)).collect();
        let (mut chart, candles) = trade_chart(
            FootprintBarAggregation::Trades { trades_per_bar: 2 },
            trades,
        );
        assert!(chart.sequence_points().is_some());
        chart.replay_select_time(3 * M).unwrap();
        assert_eq!(chart.replay_clock_micros(), Some(4 * M));
        assert_eq!(rows(&chart, candles).0, 2);
        assert!(chart.replay_step_forward().unwrap());
        assert_eq!(chart.replay_clock_micros(), Some(6 * M));
        assert_eq!(rows(&chart, candles).0, 3);
        assert!(chart.replay_step_back().unwrap());
        assert_eq!(chart.replay_clock_micros(), Some(5 * M - 1));
        assert_eq!(rows(&chart, candles).0, 2);
        // Re-selecting behind the clock completes the bar the time falls in.
        chart.replay_select_time(7 * M).unwrap();
        assert_eq!(chart.replay_clock_micros(), Some(8 * M));
        assert_eq!(rows(&chart, candles).0, 4);
    }

    #[test]
    fn resampled_bars_rebuild_from_rows_visible_at_the_replay_clock() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let source = chart.add_series(SeriesKind::Candlestick);
        let target = chart.add_series(SeriesKind::Candlestick);
        chart
            .configure_resampled_series(
                source,
                None,
                target,
                None,
                ResampleOptions {
                    interval_seconds: 300,
                    boundaries: vec![ResampleBoundary {
                        start_time: 0,
                        end_time: 3_600,
                        session_id: 1,
                    }],
                },
            )
            .unwrap();
        let times: Vec<f64> = (0..10).map(|i| f64::from(i) * 60.0).collect();
        let open: Vec<f64> = (0..10).map(|i| 10.0 + f64::from(i)).collect();
        chart
            .set_series_data(source, &times, &open, &open, &open, &open)
            .unwrap();
        assert_eq!(chart.resampled_bars(target).unwrap()[0].high, 14.0);

        chart.set_replay_clock_micros(Some(120 * M)).unwrap();
        let bars = chart.resampled_bars(target).unwrap();
        assert_eq!(bars.len(), 1);
        assert_eq!((bars[0].high, bars[0].close), (12.0, 12.0));
        assert_eq!(chart.data.series_data(target).unwrap().1[1], &[12.0]);

        chart.set_replay_clock_micros(None).unwrap();
        assert_eq!(chart.resampled_bars(target).unwrap().len(), 2);
        assert_eq!(chart.resampled_bars(target).unwrap()[0].high, 14.0);
    }
}
