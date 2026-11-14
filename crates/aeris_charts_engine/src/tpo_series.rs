//! First-class market profile (TPO) series.
//!
//! A TPO series stores ordinary OHLC bars, so scales, crosshair, legend, price chrome and
//! trading geometry treat it like a candle series. What it draws is one profile per
//! host-supplied session: every period that traded at a price prints one letter (or block) on
//! that price's row, packed from the session's left edge. The engine never infers a calendar;
//! the host supplies the sessions as UTC boundaries.
//!
//! Profiles are cached per session against the series' data generation. A data change reports
//! its first changed row, so a live update rebuilds only the sessions at or after that row
//! (normally just the forming session); a change the cache did not observe is detected by its
//! generation and rebuilds every session once.

use std::cell::{Ref, RefCell};

use aeris_charts_render::color::Color;
use serde::{Deserialize, Serialize};

use crate::profiles::{TpoRules, tpo_session, tpo_session_groups, valid_tpo_boundaries};
use crate::{
    ChartEngine, MAX_TPO_TOTAL_CELLS, ProfileError, ResampleBoundary, SeriesId, SeriesKind,
    TpoCellMode, TpoSnapshot,
};

/// Options of a TPO series. Colors are CSS colors; the profile rules match [`crate::TpoRequest`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TpoSeriesOptions {
    /// Length of one TPO period (one letter), in seconds.
    pub period_seconds: u32,
    /// Price height of one profile row. Prices are grouped onto this grid.
    pub tick_size: f64,
    pub value_area_percent: f64,
    /// Periods at the start of each session that form its initial balance.
    pub initial_balance_periods: u16,
    pub mode: TpoCellMode,
    pub color: String,
    pub value_area_color: String,
    pub single_print_color: String,
    pub poc_color: String,
    pub initial_balance_color: String,
}

impl Default for TpoSeriesOptions {
    fn default() -> Self {
        Self {
            period_seconds: 1_800,
            tick_size: 0.01,
            value_area_percent: 70.0,
            initial_balance_periods: 2,
            mode: TpoCellMode::Letters,
            color: "#8493a8".into(),
            value_area_color: "#335cff".into(),
            single_print_color: "#fb4b5f".into(),
            poc_color: "#f5a623".into(),
            initial_balance_color: "#7d52f4".into(),
        }
    }
}

impl TpoSeriesOptions {
    fn rules(&self) -> TpoRules {
        TpoRules {
            period_seconds: self.period_seconds,
            tick_size: self.tick_size,
            value_area_percent: self.value_area_percent,
            initial_balance_periods: self.initial_balance_periods,
        }
    }

    fn colors(&self) -> Option<[Color; 5]> {
        let colors = [
            &self.color,
            &self.value_area_color,
            &self.single_print_color,
            &self.poc_color,
            &self.initial_balance_color,
        ];
        if colors.iter().any(|color| color.len() > 32) {
            return None;
        }
        Some([
            Color::parse_css(colors[0])?,
            Color::parse_css(colors[1])?,
            Color::parse_css(colors[2])?,
            Color::parse_css(colors[3])?,
            Color::parse_css(colors[4])?,
        ])
    }
}

/// One cached session profile and the source rows it was built from.
pub(crate) struct TpoSessionProfile {
    /// `None` when the session exceeded a profile bound; it draws nothing.
    pub profile: Option<TpoSnapshot>,
    pub first_row: usize,
    /// Exclusive.
    pub finish_row: usize,
    cells: usize,
}

#[derive(Default)]
pub(crate) struct TpoCache {
    generation: Option<u64>,
    /// Leading sessions current for `generation`.
    valid: usize,
    sessions: Vec<TpoSessionProfile>,
    #[cfg(test)]
    built_sessions: usize,
}

impl TpoCache {
    /// Record a source mutation whose first changed row is `from`. Sessions ending before it
    /// keep their profiles; a cache that was not current for `previous_generation` keeps its
    /// stale generation and rebuilds on the next read.
    pub(crate) fn note_change(&mut self, previous_generation: u64, generation: u64, from: usize) {
        if self.generation == Some(previous_generation) {
            self.generation = Some(generation);
            self.valid = self.sessions[..self.valid.min(self.sessions.len())]
                .iter()
                .take_while(|session| session.finish_row < from)
                .count();
        }
    }

    fn reset(&mut self) {
        self.generation = None;
        self.valid = 0;
    }

    fn refresh(
        &mut self,
        generation: u64,
        times: &[i64],
        values: [&[f64]; 4],
        boundaries: &[ResampleBoundary],
        rules: &TpoRules,
    ) {
        if self.generation != Some(generation) {
            self.generation = Some(generation);
            self.valid = 0;
        }
        let groups = tpo_session_groups(boundaries).count();
        if self.valid == groups && self.sessions.len() == groups {
            return;
        }
        self.sessions.truncate(self.valid);
        let mut used = self
            .sessions
            .iter()
            .map(|session| session.cells)
            .sum::<usize>();
        for group in tpo_session_groups(boundaries).skip(self.valid) {
            #[cfg(test)]
            {
                self.built_sessions += 1;
            }
            let first_row = times.partition_point(|&time| time < group[0].start_time);
            let finish_row = times.partition_point(|&time| time < group[group.len() - 1].end_time);
            let mut remaining = MAX_TPO_TOTAL_CELLS - used;
            let profile = tpo_session(times, values, group, rules, &mut remaining).ok();
            let cells = if profile.is_some() {
                MAX_TPO_TOTAL_CELLS - used - remaining
            } else {
                0
            };
            used += cells;
            self.sessions.push(TpoSessionProfile {
                profile,
                first_row,
                finish_row,
                cells,
            });
        }
        self.valid = groups;
    }
}

pub(crate) struct TpoSeriesState {
    /// The options as the host supplied them; `rules`, `mode` and `colors` are derived from it.
    pub options: TpoSeriesOptions,
    pub rules: TpoRules,
    pub mode: TpoCellMode,
    /// Regular, value area, single print, POC and initial balance.
    pub colors: [Color; 5],
    pub boundaries: Vec<ResampleBoundary>,
    pub(crate) cache: RefCell<TpoCache>,
}

impl TpoSeriesState {
    fn new(options: &TpoSeriesOptions) -> Result<Self, ProfileError> {
        let rules = options.rules();
        if !rules.valid() {
            return Err(ProfileError::InvalidRequest);
        }
        Ok(Self {
            options: options.clone(),
            rules,
            mode: options.mode,
            colors: options.colors().ok_or(ProfileError::InvalidRequest)?,
            boundaries: Vec::new(),
            cache: RefCell::default(),
        })
    }

    pub(crate) fn default_state() -> Self {
        Self::new(&TpoSeriesOptions::default()).expect("default TPO options must remain valid")
    }

    pub(crate) fn capacity_bytes(&self) -> usize {
        let cache = self.cache.borrow();
        let profiles = cache
            .sessions
            .iter()
            .filter_map(|session| session.profile.as_ref())
            .map(|profile| {
                profile.rows.capacity() * core::mem::size_of::<crate::TpoRowSnapshot>()
                    + profile
                        .rows
                        .iter()
                        .map(|row| row.periods.capacity() * core::mem::size_of::<u16>())
                        .sum::<usize>()
            })
            .sum::<usize>();
        self.boundaries.capacity() * core::mem::size_of::<ResampleBoundary>()
            + cache.sessions.capacity() * core::mem::size_of::<TpoSessionProfile>()
            + profiles
    }
}

impl ChartEngine {
    /// Add a market profile (TPO) series. Feed it OHLC bars with the ordinary series data
    /// calls and its sessions with [`Self::set_tpo_sessions`].
    pub fn add_tpo_series(&mut self, options: TpoSeriesOptions) -> Result<SeriesId, ProfileError> {
        let state = TpoSeriesState::new(&options)?;
        let id = self.add_series(SeriesKind::Tpo);
        if let Some(series) = self.series_entry_mut(id) {
            series.tpo = Some(state);
        }
        Ok(id)
    }

    /// Change a TPO series' rules and colors, keeping its bars and sessions. Turns any OHLC
    /// series into a TPO series.
    pub fn configure_tpo_series(
        &mut self,
        id: SeriesId,
        options: TpoSeriesOptions,
    ) -> Result<(), ProfileError> {
        let mut state = TpoSeriesState::new(&options)?;
        let series = self.series_entry(id).ok_or(ProfileError::UnknownSource)?;
        if !matches!(
            series.kind,
            SeriesKind::Candlestick | SeriesKind::Bar | SeriesKind::Tpo
        ) {
            return Err(ProfileError::UnknownSource);
        }
        if let Some(current) = series.tpo.as_ref() {
            state.boundaries = current.boundaries.clone();
        }
        self.convert_series_kind(id, SeriesKind::Tpo);
        let series = self
            .series_entry_mut(id)
            .ok_or(ProfileError::UnknownSource)?;
        series.tpo = Some(state);
        self.invalidate_frame_series(id);
        Ok(())
    }

    /// Replace the sessions a TPO series draws. Consecutive boundaries that share a session
    /// ID form one profile; boundaries must be ascending and non-overlapping.
    pub fn set_tpo_sessions(
        &mut self,
        id: SeriesId,
        boundaries: Vec<ResampleBoundary>,
    ) -> Result<(), ProfileError> {
        if !boundaries.is_empty() && !valid_tpo_boundaries(&boundaries) {
            return Err(ProfileError::InvalidRequest);
        }
        let state = self
            .series_entry_mut(id)
            .and_then(|series| series.tpo.as_mut())
            .ok_or(ProfileError::UnknownSource)?;
        if state.boundaries != boundaries {
            state.boundaries = boundaries;
            state.cache.get_mut().reset();
            self.invalidate_frame_series(id);
        }
        Ok(())
    }

    /// A TPO series' current options, or `None` for a series that is not a TPO.
    pub fn tpo_series_options(&self, id: SeriesId) -> Option<&TpoSeriesOptions> {
        Some(&self.series_entry(id)?.tpo.as_ref()?.options)
    }

    /// The profiles a TPO series currently draws, oldest session first. Sessions that exceeded
    /// a profile bound are omitted.
    pub fn tpo_series_profiles(&self, id: SeriesId) -> Option<Vec<TpoSnapshot>> {
        let sessions = self.tpo_sessions(id)?;
        Some(
            sessions
                .iter()
                .filter_map(|session| session.profile.clone())
                .collect(),
        )
    }

    /// The series' current session profiles, rebuilt only where its data changed.
    pub(crate) fn tpo_sessions(&self, id: SeriesId) -> Option<Ref<'_, [TpoSessionProfile]>> {
        let state = self.series_entry(id)?.tpo.as_ref()?;
        let generation = self.data.series_generation(id)?;
        let (times, values) = self.data.series_data(id)?;
        state.cache.borrow_mut().refresh(
            generation,
            times,
            values,
            &state.boundaries,
            &state.rules,
        );
        Some(Ref::map(state.cache.borrow(), |cache| &cache.sessions[..]))
    }

    pub(crate) fn tpo_capacity_bytes(&self) -> usize {
        self.series
            .iter()
            .filter_map(|series| series.tpo.as_ref())
            .map(TpoSeriesState::capacity_bytes)
            .sum()
    }

    #[cfg(test)]
    pub(crate) fn tpo_built_sessions(&self, id: SeriesId) -> usize {
        self.series_entry(id)
            .and_then(|series| series.tpo.as_ref())
            .map_or(0, |state| state.cache.borrow().built_sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(day: i64) -> ResampleBoundary {
        ResampleBoundary {
            start_time: day * 86_400,
            end_time: day * 86_400 + 3_600,
            session_id: day as u64 + 1,
        }
    }

    /// Three one-hour sessions of twelve five-minute bars.
    fn tpo_chart() -> (ChartEngine, SeriesId) {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let id = chart
            .add_tpo_series(TpoSeriesOptions {
                period_seconds: 900,
                tick_size: 1.0,
                ..Default::default()
            })
            .unwrap();
        let mut times = Vec::new();
        let (mut open, mut high, mut low, mut close) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for day in 0..3 {
            for bar in 0..12 {
                let price = 100.0 + f64::from(bar % 4);
                times.push((day * 86_400 + bar * 300) as f64);
                open.push(price);
                high.push(price + 1.0);
                low.push(price - 1.0);
                close.push(price);
            }
        }
        chart
            .set_series_data(id, &times, &open, &high, &low, &close)
            .unwrap();
        chart
            .set_tpo_sessions(id, (0..3).map(session).collect())
            .unwrap();
        (chart, id)
    }

    #[test]
    fn each_session_builds_one_profile_from_its_own_bars() {
        let (chart, id) = tpo_chart();
        let profiles = chart.tpo_series_profiles(id).unwrap();
        assert_eq!(profiles.len(), 3);
        let first = &profiles[0];
        assert_eq!(first.session_id, 1);
        // Prices 99..=104 across four 15-minute periods.
        assert_eq!(first.rows.first().unwrap().price, 99.0);
        assert_eq!(first.rows.last().unwrap().price, 104.0);
        assert!(first.rows.iter().all(|row| row.periods.len() <= 4));
        assert!(first.poc.is_some() && first.value_area_low.is_some());
        assert_eq!(chart.series_kind(id), Some(SeriesKind::Tpo));
    }

    #[test]
    fn a_live_update_rebuilds_only_the_forming_session() {
        let (mut chart, id) = tpo_chart();
        chart.tpo_series_profiles(id).unwrap();
        assert_eq!(chart.tpo_built_sessions(id), 3);
        let tip = (2 * 86_400 + 11 * 300) as f64;
        assert!(chart.update_series_bar(id, tip, [100.0, 110.0, 99.0, 108.0]));
        let profiles = chart.tpo_series_profiles(id).unwrap();
        assert_eq!(chart.tpo_built_sessions(id), 4);
        assert_eq!(profiles[2].rows.last().unwrap().price, 110.0);
        assert_eq!(profiles[0].rows.last().unwrap().price, 104.0);

        // A full replacement is a new generation and rebuilds every session once.
        let (times, values) = chart.data.series_data(id).unwrap();
        let times = times.iter().map(|&time| time as f64).collect::<Vec<_>>();
        let values = values.map(<[f64]>::to_vec);
        chart
            .set_series_data(id, &times, &values[0], &values[1], &values[2], &values[3])
            .unwrap();
        chart.tpo_series_profiles(id).unwrap();
        assert_eq!(chart.tpo_built_sessions(id), 7);
    }

    #[test]
    fn invalid_sessions_and_options_are_refused() {
        let (mut chart, id) = tpo_chart();
        let overlapping = vec![
            session(0),
            ResampleBoundary {
                start_time: 100,
                ..session(0)
            },
        ];
        assert_eq!(
            chart.set_tpo_sessions(id, overlapping),
            Err(ProfileError::InvalidRequest)
        );
        assert_eq!(chart.tpo_series_profiles(id).unwrap().len(), 3);
        assert!(
            chart
                .add_tpo_series(TpoSeriesOptions {
                    period_seconds: 0,
                    ..Default::default()
                })
                .is_err()
        );
        assert!(chart.set_tpo_sessions(id, Vec::new()).is_ok());
        assert!(chart.tpo_series_profiles(id).unwrap().is_empty());
    }

    #[test]
    fn an_ohlc_series_converts_to_tpo_and_back_without_losing_bars() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart
            .set_series_data(
                0,
                &[0.0, 300.0],
                &[1.0, 2.0],
                &[2.0, 3.0],
                &[0.5, 1.5],
                &[1.5, 2.5],
            )
            .unwrap();
        chart
            .configure_tpo_series(
                0,
                TpoSeriesOptions {
                    tick_size: 0.5,
                    ..Default::default()
                },
            )
            .unwrap();
        chart.set_tpo_sessions(0, vec![session(0)]).unwrap();
        assert_eq!(chart.tpo_series_profiles(0).unwrap().len(), 1);
        chart.convert_series_kind(0, SeriesKind::Candlestick);
        assert!(chart.tpo_series_profiles(0).is_none());
        assert_eq!(chart.data.series_data(0).unwrap().0.len(), 2);
    }
}
