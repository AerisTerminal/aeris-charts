//! Calendar heatmap layout: one block of week columns and weekday rows per UTC calendar year,
//! month outlines, locale month and weekday labels, and a continuous color legend that shares
//! the heatmap color policy.
//!
//! Dates follow an explicit UTC policy: a day is `floor(epoch_ms / 86_400_000)` regardless of the
//! chart's display time zone, so the same input always lands in the same cell on every host.

use std::collections::HashSet;

use aeris_charts_core::scale::time_tick_marks::{civil_from_timestamp, days_from_civil};
use aeris_charts_core::style::DEFAULT_BORDER_RGB;
use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{LineStyle, LineType, Prim, TextAlign};

use super::{
    GeneralLayoutAccessibilityItem, GeneralLayoutElement, GeneralLayoutHit, LayoutSeriesEntry,
    add_text_bytes, invalid_data, layout_irect, layout_value_text, resource_limit, validate_rows,
};
use crate::frame::interpolate_color;
use crate::general_axes::GeneralPlotRect;
use crate::{ChartEngine, ChartError, GeneralLayoutSeriesId, MAX_GENERAL_TEMPORAL_MILLISECONDS};

const DAY_MS: f64 = 86_400_000.0;
const MAX_CALENDAR_YEARS: usize = 10;
const MONTH_LABEL_HEIGHT: f64 = 20.0;
const YEAR_GAP: f64 = 16.0;
const LEGEND_HEIGHT: f64 = 36.0;
const LEGEND_WIDTH: f64 = 160.0;
const LEGEND_STRIP: f64 = 10.0;
const LEGEND_STEPS: usize = 32;
const TEXT_SIZE: f64 = 12.0;
const LABEL_GAP: f64 = 6.0;
const RIGHT_PADDING: f64 = 8.0;
const MIN_CELL: f64 = 2.0;

/// One calendar value. `date` is any instant inside its UTC day, in epoch milliseconds.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CalendarDay {
    pub date: f64,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CalendarInput {
    pub days: Vec<CalendarDay>,
    /// Weekday in the first row of every week column: 0 = Sunday through 6 = Saturday.
    #[serde(default)]
    pub week_start: u8,
    /// Seven weekday labels beginning with Sunday; empty uses English initials.
    #[serde(default)]
    pub day_labels: Vec<String>,
    /// Color of the lowest value; defaults to the series color at 20% opacity.
    #[serde(default)]
    pub low_color: Option<String>,
    /// Color of the highest value; defaults to the series color.
    #[serde(default)]
    pub high_color: Option<String>,
}

/// Whole UTC years covering every input day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CalendarRange {
    first_year: i64,
    years: usize,
    start_day: i64,
    day_count: usize,
}

fn day_number(date: f64) -> i64 {
    (date / DAY_MS).floor() as i64
}

/// 1970-01-01 was a Thursday; 0 is Sunday.
fn weekday(day: i64) -> i64 {
    (day + 4).rem_euclid(7)
}

fn year_of(day: i64) -> i64 {
    civil_from_timestamp(day.saturating_mul(86_400)).0
}

pub(super) fn range(days: &[CalendarDay]) -> Option<CalendarRange> {
    let (first, last) = days
        .iter()
        .fold((i64::MAX, i64::MIN), |(first, last), day| {
            let number = day_number(day.date);
            (first.min(number), last.max(number))
        });
    if days.is_empty() {
        return None;
    }
    let first_year = year_of(first);
    let last_year = year_of(last);
    let start_day = days_from_civil(first_year, 1, 1)?;
    let end_day = days_from_civil(last_year.checked_add(1)?, 1, 1)?;
    Some(CalendarRange {
        first_year,
        years: usize::try_from(last_year - first_year + 1).ok()?,
        start_day,
        day_count: usize::try_from(end_day - start_day).ok()?,
    })
}

pub(super) fn validate(input: &CalendarInput) -> Result<(), ChartError> {
    validate_rows(input.days.len())?;
    if input.week_start > 6 {
        return Err(invalid_data(
            "calendar week_start must be 0 (Sunday) through 6 (Saturday)",
        ));
    }
    if !(input.day_labels.is_empty() || input.day_labels.len() == 7) {
        return Err(invalid_data(
            "calendar day_labels needs seven names beginning with Sunday",
        ));
    }
    let mut bytes = 0usize;
    for label in &input.day_labels {
        add_text_bytes(&mut bytes, label)?;
    }
    for color in [&input.low_color, &input.high_color].into_iter().flatten() {
        if color.len() > 64 || Color::parse_css(color).is_none() {
            return Err(invalid_data("invalid calendar color"));
        }
    }
    let limit = MAX_GENERAL_TEMPORAL_MILLISECONDS as f64;
    let mut seen = HashSet::with_capacity(input.days.len());
    for day in &input.days {
        if !day.date.is_finite() || day.date.abs() > limit {
            return Err(invalid_data(
                "calendar dates must be bounded epoch milliseconds",
            ));
        }
        if !day.value.is_finite() {
            return Err(invalid_data("calendar values must be finite"));
        }
        if !seen.insert(day_number(day.date)) {
            return Err(invalid_data("duplicate calendar day"));
        }
    }
    if range(&input.days).is_some_and(|range| range.years > MAX_CALENDAR_YEARS) {
        return Err(resource_limit("calendar spans too many years"));
    }
    Ok(())
}

pub(super) fn heap_bytes(input: &CalendarInput) -> usize {
    input.days.capacity() * std::mem::size_of::<CalendarDay>()
        + input.day_labels.capacity() * std::mem::size_of::<String>()
        + input.day_labels.iter().map(String::capacity).sum::<usize>()
        + input.low_color.as_ref().map_or(0, String::capacity)
        + input.high_color.as_ref().map_or(0, String::capacity)
}

/// One value per day of the covered years, `NaN` where the input has no day. Calendar items are
/// addressed by day offset from the first covered January 1, so keyboard navigation can move by
/// exactly one day or one week across empty days.
pub(super) fn semantic_values(days: &[CalendarDay]) -> Vec<f64> {
    let Some(range) = range(days) else {
        return Vec::new();
    };
    let mut values = vec![f64::NAN; range.day_count];
    for day in days {
        values[(day_number(day.date) - range.start_day) as usize] = day.value;
    }
    values
}

pub(super) fn semantic_value_count(days: &[CalendarDay]) -> usize {
    range(days).map_or(0, |range| range.day_count)
}

/// `YYYY-MM-DD` of a calendar item.
pub(super) fn item_date(days: &[CalendarDay], row: usize) -> Option<String> {
    let range = range(days)?;
    let (year, month, day) = civil_from_timestamp(
        range
            .start_day
            .checked_add(i64::try_from(row).ok()?)?
            .checked_mul(86_400)?,
    );
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

struct CalendarGeometry {
    range: CalendarRange,
    week_start: i64,
    plot: GeneralPlotRect,
    origin_x: f64,
    cell: f64,
    columns: i64,
}

impl CalendarGeometry {
    fn new(range: CalendarRange, week_start: u8, left: f64, plot: GeneralPlotRect) -> Option<Self> {
        let week_start = i64::from(week_start);
        let columns = (0..range.years)
            .filter_map(|index| {
                let year = range.first_year + index as i64;
                let jan1 = days_from_civil(year, 1, 1)?;
                let next = days_from_civil(year + 1, 1, 1)?;
                Some(((weekday(jan1) - week_start).rem_euclid(7) + next - jan1 + 6) / 7)
            })
            .max()?;
        let years = range.years as f64;
        let width = plot.width - left - RIGHT_PADDING;
        let height =
            plot.height - LEGEND_HEIGHT - years * MONTH_LABEL_HEIGHT - (years - 1.0) * YEAR_GAP;
        let cell = (width / columns as f64).min(height / (7.0 * years));
        (cell >= MIN_CELL).then_some(Self {
            range,
            week_start,
            plot,
            origin_x: plot.x + left,
            cell,
            columns,
        })
    }

    fn year_top(&self, index: usize) -> f64 {
        self.plot.y
            + MONTH_LABEL_HEIGHT
            + index as f64 * (MONTH_LABEL_HEIGHT + 7.0 * self.cell + YEAR_GAP)
    }

    fn lead(&self, jan1: i64) -> i64 {
        (weekday(jan1) - self.week_start).rem_euclid(7)
    }

    fn grid(&self, day: i64, jan1: i64) -> (i64, i64) {
        (
            (day - jan1 + self.lead(jan1)) / 7,
            (weekday(day) - self.week_start).rem_euclid(7),
        )
    }

    /// Top-left corner of a day's cell.
    fn position(&self, day: i64) -> Option<(f64, f64)> {
        let year = year_of(day);
        let index = usize::try_from(year - self.range.first_year)
            .ok()
            .filter(|index| *index < self.range.years)?;
        let (column, row) = self.grid(day, days_from_civil(year, 1, 1)?);
        Some((
            self.origin_x + column as f64 * self.cell,
            self.year_top(index) + row as f64 * self.cell,
        ))
    }

    fn day_at(&self, x: f64, y: f64) -> Option<i64> {
        if x < self.origin_x || x >= self.origin_x + self.columns as f64 * self.cell {
            return None;
        }
        let column = ((x - self.origin_x) / self.cell).floor() as i64;
        let index = (0..self.range.years).find(|index| {
            let top = self.year_top(*index);
            y >= top && y < top + 7.0 * self.cell
        })?;
        let row = ((y - self.year_top(index)) / self.cell).floor() as i64;
        let year = self.range.first_year + index as i64;
        let jan1 = days_from_civil(year, 1, 1)?;
        let next = days_from_civil(year + 1, 1, 1)?;
        let day = jan1 - self.lead(jan1) + column * 7 + row;
        (day >= jan1 && day < next).then_some(day)
    }

    fn legend_top(&self) -> f64 {
        self.year_top(self.range.years - 1) + 7.0 * self.cell + LABEL_GAP * 2.0
    }
}

fn value_extent(values: &[f64]) -> Option<(f64, f64)> {
    values
        .iter()
        .filter(|value| !value.is_nan())
        .fold(None, |extent, &value| match extent {
            None => Some((value, value)),
            Some((low, high)) => Some((f64::min(low, value), f64::max(high, value))),
        })
}

fn intensity(value: f64, (low, high): (f64, f64)) -> f64 {
    if high > low {
        (value - low) / (high - low)
    } else {
        1.0
    }
}

/// The general heatmap ramp: an explicit low/high pair, or the series color from 20% to full
/// opacity.
fn color_scale(base: Color, input: &CalendarInput) -> (Color, Color) {
    let low = input
        .low_color
        .as_deref()
        .and_then(Color::parse_css)
        .unwrap_or_else(|| {
            Color::rgba(
                base.r(),
                base.g(),
                base.b(),
                (f64::from(base.a()) * 0.2).round() as u8,
            )
        });
    let high = input
        .high_color
        .as_deref()
        .and_then(Color::parse_css)
        .unwrap_or(base);
    (low, high)
}

fn day_labels(language: crate::ChartLanguage, input: &CalendarInput) -> [&str; 7] {
    let defaults = aeris_charts_core::format::time_formatter::weekday_initials(language);
    std::array::from_fn(|index| {
        input
            .day_labels
            .get(index)
            .map_or(defaults[index], String::as_str)
    })
}

impl ChartEngine {
    /// Left gutter: wide enough for the weekday labels and for the year label above them.
    fn calendar_left(&self, input: &CalendarInput, range: CalendarRange) -> f64 {
        let family = &self.options.get().layout.font_family;
        let days = day_labels(self.language, input)
            .iter()
            .map(|label| self.measure_text_run(label, TEXT_SIZE, family, 400, false))
            .fold(0.0_f64, f64::max);
        let last_year = (range.first_year + range.years as i64 - 1).to_string();
        let year = self.measure_text_run(&last_year, TEXT_SIZE, family, 600, false);
        (days + LABEL_GAP * 2.0).max(year + LABEL_GAP)
    }

    fn calendar_geometry(
        &self,
        input: &CalendarInput,
        plot: GeneralPlotRect,
    ) -> Option<CalendarGeometry> {
        let range = range(&input.days)?;
        CalendarGeometry::new(
            range,
            input.week_start,
            self.calendar_left(input, range),
            plot,
        )
    }

    /// The day offset under `(x, y)` when that day has a value.
    pub(super) fn calendar_hit(
        &self,
        input: &CalendarInput,
        values: &[f64],
        plot: GeneralPlotRect,
        x: f64,
        y: f64,
    ) -> Option<usize> {
        let geometry = self.calendar_geometry(input, plot)?;
        let row = usize::try_from(geometry.day_at(x, y)? - geometry.range.start_day).ok()?;
        values
            .get(row)
            .is_some_and(|value| !value.is_nan())
            .then_some(row)
    }

    pub(super) fn calendar_anchor(
        &self,
        input: &CalendarInput,
        plot: GeneralPlotRect,
        row: usize,
    ) -> Option<(f64, f64)> {
        let geometry = self.calendar_geometry(input, plot)?;
        if row >= geometry.range.day_count {
            return None;
        }
        let (x, y) = geometry.position(geometry.range.start_day + row as i64)?;
        Some((x + geometry.cell * 0.5, y + geometry.cell * 0.5))
    }

    pub(super) fn calendar_day_color(
        &self,
        series: &LayoutSeriesEntry,
        input: &CalendarInput,
        values: &[f64],
        row: usize,
    ) -> Color {
        let (low, high) = color_scale(series.color, input);
        match (values.get(row), value_extent(values)) {
            (Some(value), Some(extent)) if !value.is_nan() => {
                interpolate_color(low, high, intensity(*value, extent))
            }
            _ => self.chart_surface_color(),
        }
    }

    pub(super) fn calendar_accessibility_items(
        &self,
        series: GeneralLayoutSeriesId,
        days: &[CalendarDay],
        values: &[f64],
        selected: Option<GeneralLayoutHit>,
        offset: usize,
        limit: usize,
    ) -> Vec<GeneralLayoutAccessibilityItem> {
        let Some(range) = range(days) else {
            return Vec::new();
        };
        (offset..offset.saturating_add(limit).min(range.day_count))
            .map(|row| {
                let (year, month, day) =
                    civil_from_timestamp((range.start_day + row as i64) * 86_400);
                GeneralLayoutAccessibilityItem {
                    element: GeneralLayoutElement::Day,
                    row,
                    id: format!("{year:04}-{month:02}-{day:02}"),
                    label: format!(
                        "{day} {} {year}",
                        self.month_names.long[(month - 1) as usize]
                    ),
                    value: values.get(row).copied().filter(|value| !value.is_nan()),
                    selected: selected
                        == Some(GeneralLayoutHit {
                            series,
                            element: GeneralLayoutElement::Day,
                            row,
                        }),
                    #[cfg(feature = "complete")]
                    expanded: None,
                    #[cfg(feature = "complete")]
                    placed: None,
                }
            })
            .collect()
    }

    fn calendar_text(
        &self,
        text: String,
        (x, y): (f64, f64),
        color: Color,
        align: TextAlign,
        weight: u16,
        (hpr, vpr): (f64, f64),
    ) -> Prim {
        Prim::Text {
            x: (x * hpr) as f32,
            y: (y * vpr) as f32,
            text,
            color,
            size: (TEXT_SIZE * vpr) as f32,
            family: self.options.get().layout.font_family.clone(),
            align,
            weight,
            italic: false,
        }
    }

    pub(super) fn build_calendar_frame(
        &self,
        series: &LayoutSeriesEntry,
        (input, values): (&CalendarInput, &[f64]),
        plot: GeneralPlotRect,
        ratio: (f64, f64),
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let Some(geometry) = self.calendar_geometry(input, plot) else {
            return;
        };
        let (hpr, vpr) = ratio;
        let options = self.options.get();
        let family = &options.layout.font_family;
        let fallback = Color::rgb(
            DEFAULT_BORDER_RGB.0,
            DEFAULT_BORDER_RGB.1,
            DEFAULT_BORDER_RGB.2,
        );
        let border = Color::parse_css(&options.grid.horz_lines.color).unwrap_or(fallback);
        let surface = self.chart_surface_color();
        let text_color = self.primary_text_color();
        let (low, high) = color_scale(series.color, input);
        let extent = value_extent(values);
        let frame_width = hpr.min(vpr).round().max(1.0) as i32;
        let line_width = hpr.min(vpr) as f32;
        let labels = day_labels(self.language, input);
        let cell = geometry.cell;
        for index in 0..geometry.range.years {
            let year = geometry.range.first_year + index as i64;
            let (Some(jan1), Some(next)) =
                (days_from_civil(year, 1, 1), days_from_civil(year + 1, 1, 1))
            else {
                continue;
            };
            let top = geometry.year_top(index);
            for day in jan1..next {
                let Some((x, y)) = geometry.position(day) else {
                    continue;
                };
                let Some(rect) = layout_irect(
                    GeneralPlotRect {
                        x,
                        y,
                        width: cell,
                        height: cell,
                    },
                    hpr,
                    vpr,
                ) else {
                    continue;
                };
                let value = values
                    .get((day - geometry.range.start_day) as usize)
                    .copied()
                    .unwrap_or(f64::NAN);
                let fill = match extent {
                    Some(extent) if !value.is_nan() => {
                        interpolate_color(low, high, intensity(value, extent))
                    }
                    _ => surface,
                };
                out.push(Prim::Rect { rect, color: fill });
                out.push(Prim::RectFrame {
                    rect,
                    border: frame_width,
                    color: border,
                });
            }
            for month in 1..=12 {
                let Some(first) = days_from_civil(year, month, 1) else {
                    continue;
                };
                let last = if month == 12 {
                    next - 1
                } else {
                    let Some(following) = days_from_civil(year, month + 1, 1) else {
                        continue;
                    };
                    following - 1
                };
                let (first_column, first_row) = geometry.grid(first, jan1);
                let (last_column, last_row) = geometry.grid(last, jan1);
                let point = |column: i64, row: i64| {
                    [
                        ((geometry.origin_x + column as f64 * cell) * hpr) as f32,
                        ((top + row as f64 * cell) * vpr) as f32,
                    ]
                };
                let first_point = points.len() as u32;
                points.extend([
                    point(first_column, first_row),
                    point(first_column + 1, first_row),
                    point(first_column + 1, 0),
                    point(last_column + 1, 0),
                    point(last_column + 1, last_row + 1),
                    point(last_column, last_row + 1),
                    point(last_column, 7),
                    point(first_column, 7),
                    point(first_column, first_row),
                ]);
                out.push(Prim::Polyline {
                    first_point,
                    point_count: 9,
                    width: line_width,
                    style: LineStyle::Solid,
                    line_type: LineType::Simple,
                    color: text_color,
                });
                let name = &self.month_names.short[(month - 1) as usize];
                if self.measure_text_run(name, TEXT_SIZE, family, 400, false) <= 4.0 * cell {
                    out.push(self.calendar_text(
                        name.clone(),
                        (
                            geometry.origin_x + first_column as f64 * cell,
                            top - MONTH_LABEL_HEIGHT * 0.5,
                        ),
                        text_color,
                        TextAlign::Left,
                        400,
                        ratio,
                    ));
                }
            }
            if cell >= 9.0 {
                for row in 0..7 {
                    let label = labels[((geometry.week_start + row) % 7) as usize];
                    out.push(self.calendar_text(
                        label.to_owned(),
                        (
                            geometry.origin_x - LABEL_GAP,
                            top + (row as f64 + 0.5) * cell,
                        ),
                        text_color,
                        TextAlign::Right,
                        400,
                        ratio,
                    ));
                }
            }
            out.push(self.calendar_text(
                year.to_string(),
                (plot.x, top - MONTH_LABEL_HEIGHT * 0.5),
                text_color,
                TextAlign::Left,
                600,
                ratio,
            ));
        }
        let Some((minimum, maximum)) = extent else {
            return;
        };
        let strip = GeneralPlotRect {
            x: geometry.origin_x,
            y: geometry.legend_top(),
            width: LEGEND_WIDTH.min(geometry.columns as f64 * cell),
            height: LEGEND_STRIP,
        };
        if strip.y + LEGEND_STRIP + TEXT_SIZE > plot.y + plot.height {
            return;
        }
        let step = strip.width / LEGEND_STEPS as f64;
        for index in 0..LEGEND_STEPS {
            if let Some(rect) = layout_irect(
                GeneralPlotRect {
                    x: strip.x + index as f64 * step,
                    width: step,
                    ..strip
                },
                hpr,
                vpr,
            ) {
                out.push(Prim::Rect {
                    rect,
                    color: interpolate_color(low, high, (index as f64 + 0.5) / LEGEND_STEPS as f64),
                });
            }
        }
        let label_y = strip.y + LEGEND_STRIP + LABEL_GAP + TEXT_SIZE * 0.5;
        out.push(self.calendar_text(
            layout_value_text(Some(minimum)),
            (strip.x, label_y),
            text_color,
            TextAlign::Left,
            400,
            ratio,
        ));
        out.push(self.calendar_text(
            layout_value_text(Some(maximum)),
            (strip.x + strip.width, label_y),
            text_color,
            TextAlign::Right,
            400,
            ratio,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(year: i64, month: u32, day: u32) -> f64 {
        days_from_civil(year, month, day).unwrap() as f64 * DAY_MS
    }

    fn input(days: &[(f64, f64)], week_start: u8) -> CalendarInput {
        CalendarInput {
            days: days
                .iter()
                .map(|&(date, value)| CalendarDay { date, value })
                .collect(),
            week_start,
            day_labels: Vec::new(),
            low_color: None,
            high_color: None,
        }
    }

    fn geometry(input: &CalendarInput) -> CalendarGeometry {
        CalendarGeometry::new(
            range(&input.days).unwrap(),
            input.week_start,
            30.0,
            GeneralPlotRect {
                x: 0.0,
                y: 0.0,
                width: 1_200.0,
                height: 400.0,
            },
        )
        .unwrap()
    }

    #[test]
    fn leap_years_cover_366_days_and_week_start_moves_rows_and_columns() {
        let leap = input(&[(utc(2024, 2, 29), 1.0)], 0);
        assert_eq!(range(&leap.days).unwrap().day_count, 366);
        assert_eq!(
            semantic_value_count(&input(&[(utc(2023, 6, 1), 1.0)], 0).days),
            365
        );
        assert_eq!(
            semantic_value_count(&input(&[(utc(1900, 6, 1), 1.0)], 0).days),
            365
        );
        assert_eq!(
            semantic_value_count(&input(&[(utc(2000, 6, 1), 1.0)], 0).days),
            366
        );
        // 2024-01-01 is a Monday: Sunday-first puts it in row 1, Monday-first in row 0.
        let jan1 = days_from_civil(2024, 1, 1).unwrap();
        assert_eq!(weekday(jan1), 1);
        let sunday = geometry(&input(&[(utc(2024, 1, 1), 1.0)], 0));
        let monday = geometry(&input(&[(utc(2024, 1, 1), 1.0)], 1));
        assert_eq!(sunday.grid(jan1, jan1), (0, 1));
        assert_eq!(monday.grid(jan1, jan1), (0, 0));
        let dec31 = days_from_civil(2024, 12, 31).unwrap();
        assert_eq!(sunday.grid(dec31, jan1), (52, 2));
        assert_eq!(monday.grid(dec31, jan1), (52, 1));
        let feb29 = days_from_civil(2024, 2, 29).unwrap();
        let (x, y) = sunday.position(feb29).unwrap();
        assert_eq!(
            sunday.day_at(x + sunday.cell * 0.5, y + sunday.cell * 0.5),
            Some(feb29)
        );
        // The cell before January 1 in its first column is outside the year.
        let (x, y) = sunday.position(jan1).unwrap();
        assert_eq!(sunday.day_at(x + 1.0, y - sunday.cell + 1.0), None);
    }

    #[test]
    fn utc_day_boundaries_ignore_the_time_of_day() {
        let midnight = utc(2024, 3, 10);
        let late = midnight + DAY_MS - 1.0;
        assert_eq!(day_number(midnight), day_number(late));
        assert_eq!(day_number(midnight - 1.0), day_number(midnight) - 1);
        assert!(validate(&input(&[(midnight, 1.0), (late, 2.0)], 0)).is_err());
        assert!(validate(&input(&[(midnight - 1.0, 1.0), (midnight, 2.0)], 0)).is_ok());
        let values = semantic_values(&input(&[(late, 7.0)], 0).days);
        let row = (day_number(midnight) - range(&input(&[(late, 7.0)], 0).days).unwrap().start_day)
            as usize;
        assert_eq!(values[row], 7.0);
        assert_eq!(values.iter().filter(|value| !value.is_nan()).count(), 1);
        assert_eq!(
            item_date(&input(&[(late, 7.0)], 0).days, row).as_deref(),
            Some("2024-03-10")
        );
    }

    #[test]
    fn calendar_validation_is_bounded() {
        assert!(validate(&input(&[(utc(2024, 1, 1), 1.0)], 7)).is_err());
        assert!(validate(&input(&[(utc(2024, 1, 1), f64::NAN)], 0)).is_err());
        assert!(validate(&input(&[(f64::INFINITY, 1.0)], 0)).is_err());
        assert!(validate(&input(&[(utc(2010, 1, 1), 1.0), (utc(2024, 1, 1), 1.0)], 0)).is_err());
        let mut labels = input(&[(utc(2024, 1, 1), 1.0)], 0);
        labels.day_labels = vec!["Su".into(); 6];
        assert!(validate(&labels).is_err());
        labels.day_labels = vec!["Su".into(); 7];
        assert!(validate(&labels).is_ok());
        labels.low_color = Some("not a color".into());
        assert!(validate(&labels).is_err());
    }
}
