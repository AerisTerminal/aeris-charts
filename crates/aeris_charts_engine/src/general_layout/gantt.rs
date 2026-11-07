//! Gantt timeline layout: temporal task bars on rows with progress fill, zero-duration milestones,
//! finish-to-start dependency arrows, contiguous row groups, a host-supplied current-time line,
//! and an engine-owned row viewport so frame and hit work stay bounded by the visible rows.

use std::collections::{HashMap, HashSet, VecDeque};

use aeris_charts_core::scale::time_tick_marks::civil_from_timestamp;
use aeris_charts_core::style::DEFAULT_BORDER_RGB;
use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{LineStyle, LineType, Prim, TextAlign};

use super::{
    GeneralLayoutAccessibilityItem, GeneralLayoutElement, GeneralLayoutHit, LayoutSeriesEntry,
    MAX_FLOW_LINKS, add_text_bytes, invalid_data, layout_irect, resource_limit, validate_id,
    validate_rows,
};
use crate::general_axes::{GeneralPlotRect, automatic_temporal_ticks};
use crate::{ChartEngine, ChartError, GeneralLayoutSeriesId, MAX_GENERAL_TEMPORAL_MILLISECONDS};

pub(super) const ROW_HEIGHT: f64 = 28.0;
const HEADER_HEIGHT: f64 = 24.0;
const BAR_HEIGHT: f64 = 18.0;
const GROUP_INDENT: f64 = 12.0;
const LABEL_PADDING: f64 = 8.0;
const TEXT_SIZE: f64 = 12.0;
const RIGHT_PADDING: f64 = 12.0;
const TICK_SPACING: f64 = 96.0;
const DEPENDENCY_STUB: f64 = 8.0;
const ARROW_LENGTH: f64 = 6.0;
const ARROW_HALF_WIDTH: f64 = 3.5;
const SCROLL_THUMB_WIDTH: f64 = 4.0;
const MAX_TASK_DEPENDENCIES: usize = 32;
const TRACK_ALPHA: f64 = 0.35;
const SINGLE_INSTANT_PADDING_MS: f64 = 43_200_000.0;
/// ECharts' default `markLine` red, used for the current-time rule.
pub(super) const NOW_COLOR: Color = Color(0xEE66_66FF);

/// One Gantt task. Times are UTC epoch milliseconds; `end == start` is a milestone.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GanttTask {
    pub id: String,
    pub label: String,
    pub start: f64,
    pub end: f64,
    /// Completed fraction in `[0, 1]`.
    #[serde(default)]
    pub progress: f64,
    /// Rows sharing a group must be contiguous; each group gets one header row.
    #[serde(default)]
    pub group: Option<String>,
    /// Finish-to-start predecessors, by task ID.
    #[serde(default)]
    pub dependencies: Vec<String>,
}

impl GanttTask {
    fn milestone(&self) -> bool {
        self.end == self.start
    }
}

pub(super) fn validate(tasks: &[GanttTask], now: Option<f64>) -> Result<(), ChartError> {
    validate_rows(tasks.len())?;
    let limit = MAX_GENERAL_TEMPORAL_MILLISECONDS as f64;
    let instant = |value: f64| value.is_finite() && value.abs() <= limit;
    let mut ids = HashSet::with_capacity(tasks.len());
    let mut closed_groups = HashSet::new();
    let mut current_group: Option<&str> = None;
    let mut bytes = 0usize;
    let mut dependencies = 0usize;
    for task in tasks {
        validate_id(&task.id, &mut ids)?;
        add_text_bytes(&mut bytes, &task.id)?;
        add_text_bytes(&mut bytes, &task.label)?;
        if !instant(task.start) || !instant(task.end) || task.end < task.start {
            return Err(invalid_data(
                "gantt task times must be bounded epoch milliseconds with end >= start",
            ));
        }
        if !(0.0..=1.0).contains(&task.progress) {
            return Err(invalid_data("gantt progress must be between 0 and 1"));
        }
        let group = task.group.as_deref();
        if group != current_group {
            if let Some(previous) = current_group {
                closed_groups.insert(previous);
            }
            if let Some(next) = group {
                add_text_bytes(&mut bytes, next)?;
                if closed_groups.contains(next) {
                    return Err(invalid_data("gantt group rows must be contiguous"));
                }
            }
            current_group = group;
        }
        if task.dependencies.len() > MAX_TASK_DEPENDENCIES {
            return Err(resource_limit("too many gantt task dependencies"));
        }
        dependencies += task.dependencies.len();
        for dependency in &task.dependencies {
            add_text_bytes(&mut bytes, dependency)?;
        }
    }
    if dependencies > MAX_FLOW_LINKS {
        return Err(resource_limit("too many gantt dependencies"));
    }
    if now.is_some_and(|now| !instant(now)) {
        return Err(invalid_data(
            "gantt current time must be bounded epoch milliseconds",
        ));
    }
    let index: HashMap<&str, usize> = tasks
        .iter()
        .enumerate()
        .map(|(row, task)| (task.id.as_str(), row))
        .collect();
    let mut successors = vec![Vec::new(); tasks.len()];
    let mut indegree = vec![0usize; tasks.len()];
    for (row, task) in tasks.iter().enumerate() {
        let mut seen = HashSet::with_capacity(task.dependencies.len());
        for dependency in &task.dependencies {
            let Some(&source) = index.get(dependency.as_str()) else {
                return Err(invalid_data("gantt dependency does not exist"));
            };
            if source == row {
                return Err(invalid_data("gantt task cannot depend on itself"));
            }
            if !seen.insert(source) {
                return Err(invalid_data("duplicate gantt dependency"));
            }
            successors[source].push(row);
            indegree[row] += 1;
        }
    }
    let mut ready: VecDeque<usize> = (0..tasks.len()).filter(|&row| indegree[row] == 0).collect();
    let mut visited = 0usize;
    while let Some(row) = ready.pop_front() {
        visited += 1;
        for &next in &successors[row] {
            indegree[next] -= 1;
            if indegree[next] == 0 {
                ready.push_back(next);
            }
        }
    }
    if visited != tasks.len() {
        return Err(invalid_data("gantt dependencies contain a cycle"));
    }
    Ok(())
}

/// Heap bytes owned by the tasks' strings and dependency lists, excluding the task vector.
pub(super) fn heap_bytes(tasks: &[GanttTask]) -> usize {
    tasks
        .iter()
        .map(|task| {
            task.id.capacity()
                + task.label.capacity()
                + task.group.as_ref().map_or(0, String::capacity)
                + task.dependencies.capacity() * std::mem::size_of::<String>()
                + task
                    .dependencies
                    .iter()
                    .map(String::capacity)
                    .sum::<usize>()
        })
        .sum::<usize>()
}

/// Semantic value of each task: its completed percentage.
pub(super) fn semantic_values(tasks: &[GanttTask]) -> Vec<f64> {
    tasks.iter().map(|task| task.progress * 100.0).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GanttRow {
    /// A group header introduced by the task at this index.
    Group(usize),
    Task(usize),
}

fn gantt_rows(tasks: &[GanttTask]) -> Vec<GanttRow> {
    let groups = tasks
        .iter()
        .zip(std::iter::once(None).chain(tasks.iter().map(|task| task.group.as_deref())))
        .filter(|(task, previous)| task.group.is_some() && task.group.as_deref() != *previous)
        .count();
    let mut rows = Vec::with_capacity(tasks.len() + groups);
    let mut group: Option<&str> = None;
    for (index, task) in tasks.iter().enumerate() {
        let next = task.group.as_deref();
        if next != group {
            if next.is_some() {
                rows.push(GanttRow::Group(index));
            }
            group = next;
        }
        rows.push(GanttRow::Task(index));
    }
    rows
}

/// The resolved timeline geometry of one series for one plot rectangle and scroll offset.
struct GanttGeometry {
    rows: Vec<GanttRow>,
    label_right: f64,
    track: (f64, f64),
    domain: (f64, f64),
    body_top: f64,
    first_row: usize,
    visible_rows: usize,
}

impl GanttGeometry {
    fn new(tasks: &[GanttTask], plot: GeneralPlotRect, scroll_row: usize) -> Option<Self> {
        if tasks.is_empty() || plot.width <= 0.0 || plot.height <= HEADER_HEIGHT {
            return None;
        }
        let body_top = plot.y + HEADER_HEIGHT;
        let visible_rows = ((plot.y + plot.height - body_top) / ROW_HEIGHT).floor() as usize;
        if visible_rows == 0 {
            return None;
        }
        let label_right = plot.x + (plot.width * 0.25).clamp(64.0, 220.0).min(plot.width * 0.5);
        let inset = BAR_HEIGHT * 0.5;
        let track = (
            label_right + inset,
            plot.x + plot.width - RIGHT_PADDING - inset,
        );
        if track.1 <= track.0 {
            return None;
        }
        let (mut start, mut end) = tasks
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(start, end), task| {
                (start.min(task.start), end.max(task.end))
            });
        if end <= start {
            start -= SINGLE_INSTANT_PADDING_MS;
            end += SINGLE_INSTANT_PADDING_MS;
        }
        let rows = gantt_rows(tasks);
        let first_row = scroll_row.min(rows.len().saturating_sub(visible_rows));
        Some(Self {
            rows,
            label_right,
            track,
            domain: (start, end),
            body_top,
            first_row,
            visible_rows,
        })
    }

    fn x(&self, time: f64) -> f64 {
        self.track.0
            + (time - self.domain.0) / (self.domain.1 - self.domain.0)
                * (self.track.1 - self.track.0)
    }

    fn visible(&self) -> std::ops::Range<usize> {
        self.first_row..(self.first_row + self.visible_rows).min(self.rows.len())
    }

    fn row_top(&self, row: usize) -> f64 {
        self.body_top + (row - self.first_row) as f64 * ROW_HEIGHT
    }

    fn body_bottom(&self) -> f64 {
        self.body_top + self.visible().len() as f64 * ROW_HEIGHT
    }

    fn task_row(&self, task: usize) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| *row == GanttRow::Task(task))
    }

    fn visible_center(&self, row: usize) -> Option<f64> {
        self.visible()
            .contains(&row)
            .then(|| self.row_top(row) + ROW_HEIGHT * 0.5)
    }

    /// Horizontal extent of a task mark: its bar, or its milestone diamond.
    fn extent(&self, task: &GanttTask) -> (f64, f64) {
        if task.milestone() {
            let center = self.x(task.start);
            (center - BAR_HEIGHT * 0.5, center + BAR_HEIGHT * 0.5)
        } else {
            (self.x(task.start), self.x(task.end))
        }
    }
}

/// The task under `(x, y)`, testing the same bar and diamond geometry the frame paints.
pub(super) fn hit(
    tasks: &[GanttTask],
    plot: GeneralPlotRect,
    scroll_row: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    let geometry = GanttGeometry::new(tasks, plot, scroll_row)?;
    if y < geometry.body_top {
        return None;
    }
    let row = geometry.first_row + ((y - geometry.body_top) / ROW_HEIGHT).floor() as usize;
    let center = geometry.visible_center(row)?;
    let GanttRow::Task(index) = geometry.rows[row] else {
        return None;
    };
    let task = &tasks[index];
    let half = BAR_HEIGHT * 0.5;
    let inside = if task.milestone() {
        (x - geometry.x(task.start)).abs() + (y - center).abs() <= half
    } else {
        let (x0, x1) = geometry.extent(task);
        (y - center).abs() <= half && x >= x0 && x <= x1.max(x0 + 1.0)
    };
    inside.then_some(index)
}

/// Center of a visible task mark; `None` while the task is scrolled out of the viewport.
pub(super) fn anchor(
    tasks: &[GanttTask],
    plot: GeneralPlotRect,
    scroll_row: usize,
    task: usize,
) -> Option<(f64, f64)> {
    let geometry = GanttGeometry::new(tasks, plot, scroll_row)?;
    let center = geometry.visible_center(geometry.task_row(task)?)?;
    let (x0, x1) = geometry.extent(tasks.get(task)?);
    Some(((x0 + x1) * 0.5, center))
}

/// The scroll offset that keeps `task` inside the viewport, moving as little as possible.
pub(super) fn reveal(
    tasks: &[GanttTask],
    plot: GeneralPlotRect,
    scroll_row: usize,
    task: usize,
) -> usize {
    let Some(geometry) = GanttGeometry::new(tasks, plot, scroll_row) else {
        return scroll_row;
    };
    let Some(row) = geometry.task_row(task) else {
        return scroll_row;
    };
    let mut first = geometry.first_row;
    // Keep a group header above its first task visible when revealing that task.
    let top = if row > 0 && matches!(geometry.rows[row - 1], GanttRow::Group(_)) {
        row - 1
    } else {
        row
    };
    if top < first {
        first = top;
    } else if row >= first + geometry.visible_rows {
        first = row + 1 - geometry.visible_rows;
    }
    first
}

/// The scroll offset after moving `delta_rows`, or `None` when the viewport cannot move.
pub(super) fn scrolled(
    tasks: &[GanttTask],
    plot: GeneralPlotRect,
    scroll_row: usize,
    delta_rows: i64,
) -> Option<usize> {
    let geometry = GanttGeometry::new(tasks, plot, scroll_row)?;
    let max_first = geometry.rows.len().saturating_sub(geometry.visible_rows);
    let next = (geometry.first_row as i64)
        .saturating_add(delta_rows)
        .clamp(0, max_first as i64) as usize;
    (next != geometry.first_row || scroll_row != geometry.first_row).then_some(next)
}

/// Whole rows for one wheel sample: one row per row height of wheel travel, at least one row,
/// wheel up (positive) scrolling toward the first row.
pub(super) fn wheel_rows(delta_y: f64) -> i64 {
    if !delta_y.is_finite() || delta_y == 0.0 {
        return 0;
    }
    let rows = (delta_y.abs() * 100.0 / ROW_HEIGHT)
        .round()
        .clamp(1.0, 64.0) as i64;
    if delta_y > 0.0 { -rows } else { rows }
}

/// Orthogonal finish-to-start route from a predecessor's end to a successor's start. A
/// successor that starts after the predecessor's stub gets one elbow; otherwise the route
/// doubles back through the gap between the two rows.
fn dependency_route(from: (f64, f64), to: (f64, f64)) -> ([(f64, f64); 6], usize) {
    let (end_x, end_y) = from;
    let (start_x, start_y) = to;
    let stub = end_x + DEPENDENCY_STUB;
    let tip = start_x - ARROW_LENGTH;
    if tip >= stub {
        (
            [
                (end_x, end_y),
                (stub, end_y),
                (stub, start_y),
                (tip, start_y),
                (tip, start_y),
                (tip, start_y),
            ],
            4,
        )
    } else {
        let gap = if start_y >= end_y {
            end_y + ROW_HEIGHT * 0.5
        } else {
            end_y - ROW_HEIGHT * 0.5
        };
        let back = start_x - DEPENDENCY_STUB;
        (
            [
                (end_x, end_y),
                (stub, end_y),
                (stub, gap),
                (back, gap),
                (back, start_y),
                (tip, start_y),
            ],
            6,
        )
    }
}

impl ChartEngine {
    /// `day month year` with `HH:MM` when the instant is not local midnight, in the chart's
    /// display time zone.
    fn gantt_date(&self, epoch_ms: f64) -> String {
        let seconds = (epoch_ms / 1_000.0).floor() as i64;
        let local = self.time_zone.local_epoch_seconds(seconds);
        let (year, month, day) = civil_from_timestamp(local);
        let month = &self.month_names.short[(month - 1) as usize];
        let seconds_of_day = local.rem_euclid(86_400);
        if seconds_of_day == 0 {
            format!("{day} {month} {year}")
        } else {
            format!(
                "{day} {month} {year} {:02}:{:02}",
                seconds_of_day / 3_600,
                seconds_of_day.rem_euclid(3_600) / 60
            )
        }
    }

    /// Schedule text of one task: its span and completion, or its milestone date.
    pub(super) fn gantt_task_detail(&self, task: &GanttTask) -> String {
        if task.milestone() {
            crate::text::gantt_milestone(self.language, &self.gantt_date(task.start))
        } else {
            crate::text::gantt_task_span(
                self.language,
                &self.gantt_date(task.start),
                &self.gantt_date(task.end),
                (task.progress * 100.0).round(),
            )
        }
    }

    pub(super) fn gantt_accessibility_items(
        &self,
        series: GeneralLayoutSeriesId,
        tasks: &[GanttTask],
        selected: Option<GeneralLayoutHit>,
        offset: usize,
        limit: usize,
    ) -> Vec<GeneralLayoutAccessibilityItem> {
        tasks
            .iter()
            .enumerate()
            .skip(offset)
            .take(limit)
            .map(|(row, task)| GeneralLayoutAccessibilityItem {
                element: GeneralLayoutElement::Task,
                row,
                id: task.id.clone(),
                label: format!("{}, {}", task.label, self.gantt_task_detail(task)),
                value: Some(task.progress * 100.0),
                selected: selected
                    == Some(GeneralLayoutHit {
                        series,
                        element: GeneralLayoutElement::Task,
                        row,
                    }),
                #[cfg(feature = "complete")]
                expanded: None,
                #[cfg(feature = "complete")]
                placed: None,
            })
            .collect()
    }

    /// `label` shortened with an ellipsis until it fits `width`, or `None` when nothing fits.
    fn gantt_fit_label(&self, label: &str, width: f64, weight: u16) -> Option<String> {
        let family = &self.options.get().layout.font_family;
        if self.measure_text_run(label, TEXT_SIZE, family, weight, false) <= width {
            return Some(label.to_owned());
        }
        let mut chars: Vec<char> = label.chars().collect();
        while !chars.is_empty() {
            chars.pop();
            let candidate: String = chars.iter().chain(std::iter::once(&'…')).collect();
            if self.measure_text_run(&candidate, TEXT_SIZE, family, weight, false) <= width {
                return Some(candidate);
            }
        }
        None
    }

    fn gantt_text(
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

    pub(super) fn build_gantt_frame(
        &self,
        series: &LayoutSeriesEntry,
        (tasks, now): (&[GanttTask], Option<f64>),
        plot: GeneralPlotRect,
        ratio: (f64, f64),
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let Some(geometry) = GanttGeometry::new(tasks, plot, series.scroll_row) else {
            return;
        };
        let (hpr, vpr) = ratio;
        let options = self.options.get();
        let grid = &options.grid;
        let fallback = Color::rgb(
            DEFAULT_BORDER_RGB.0,
            DEFAULT_BORDER_RGB.1,
            DEFAULT_BORDER_RGB.2,
        );
        let vertical_color = Color::parse_css(&grid.vert_lines.color).unwrap_or(fallback);
        let horizontal_color = Color::parse_css(&grid.horz_lines.color).unwrap_or(fallback);
        let text_color = self.primary_text_color();
        let line_width = hpr.min(vpr).round().max(1.0) as i32;
        let device_x = |x: f64| (x * hpr).round() as i32;
        let device_y = |y: f64| (y * vpr).round() as i32;
        let right = plot.x + plot.width;
        let bottom = geometry.body_bottom();

        let domain = [
            geometry.domain.0.floor() as i64,
            geometry.domain.1.ceil() as i64,
        ];
        let target =
            (((geometry.track.1 - geometry.track.0) / TICK_SPACING).floor() as usize).max(2);
        for (value, label) in
            automatic_temporal_ticks(domain, target, &self.month_names, self.time_zone)
        {
            let x = geometry.x(value as f64);
            if grid.vert_lines.visible {
                out.push(Prim::VLine {
                    x: device_x(x),
                    y0: device_y(geometry.body_top),
                    y1: device_y(bottom),
                    width: line_width,
                    style: crate::line_style_from_u8(grid.vert_lines.style),
                    color: vertical_color,
                });
            }
            let width =
                self.measure_text_run(&label, TEXT_SIZE, &options.layout.font_family, 400, false);
            if x - width * 0.5 >= geometry.label_right && x + width * 0.5 <= right {
                out.push(self.gantt_text(
                    label,
                    (x, plot.y + HEADER_HEIGHT * 0.5),
                    text_color,
                    TextAlign::Center,
                    400,
                    ratio,
                ));
            }
        }
        out.push(Prim::HLine {
            y: device_y(geometry.body_top),
            x0: device_x(plot.x),
            x1: device_x(right),
            width: line_width,
            style: LineStyle::Solid,
            color: horizontal_color,
        });
        out.push(Prim::VLine {
            x: device_x(geometry.label_right),
            y0: device_y(plot.y),
            y1: device_y(bottom),
            width: line_width,
            style: LineStyle::Solid,
            color: horizontal_color,
        });

        let track_color = Color::rgba(
            series.color.r(),
            series.color.g(),
            series.color.b(),
            (f64::from(series.color.a()) * TRACK_ALPHA).round() as u8,
        );
        let band_color = Color::rgba(text_color.r(), text_color.g(), text_color.b(), 18);
        let label_width = geometry.label_right - plot.x - LABEL_PADDING * 2.0;
        let half = BAR_HEIGHT * 0.5;
        for row in geometry.visible() {
            let top = geometry.row_top(row);
            let center = top + ROW_HEIGHT * 0.5;
            match geometry.rows[row] {
                GanttRow::Group(first) => {
                    if let Some(rect) = layout_irect(
                        GeneralPlotRect {
                            x: plot.x,
                            y: top,
                            width: plot.width,
                            height: ROW_HEIGHT,
                        },
                        hpr,
                        vpr,
                    ) {
                        out.push(Prim::Rect {
                            rect,
                            color: band_color,
                        });
                    }
                    let name = tasks[first].group.as_deref().unwrap_or_default();
                    if let Some(name) = self.gantt_fit_label(name, label_width, 600) {
                        out.push(self.gantt_text(
                            name,
                            (plot.x + LABEL_PADDING, center),
                            text_color,
                            TextAlign::Left,
                            600,
                            ratio,
                        ));
                    }
                }
                GanttRow::Task(index) => {
                    let task = &tasks[index];
                    let indent = if task.group.is_some() {
                        GROUP_INDENT
                    } else {
                        0.0
                    };
                    if let Some(label) =
                        self.gantt_fit_label(&task.label, label_width - indent, 400)
                    {
                        out.push(self.gantt_text(
                            label,
                            (plot.x + LABEL_PADDING + indent, center),
                            text_color,
                            TextAlign::Left,
                            400,
                            ratio,
                        ));
                    }
                    if task.milestone() {
                        let x = geometry.x(task.start);
                        let point = |(px, py): (f64, f64)| [(px * hpr) as f32, (py * vpr) as f32];
                        for side in [half, -half] {
                            out.push(Prim::Triangle {
                                a: point((x, center - half)),
                                b: point((x + side, center)),
                                c: point((x, center + half)),
                                color: series.color,
                            });
                        }
                    } else {
                        let (x0, x1) = geometry.extent(task);
                        let bar = GeneralPlotRect {
                            x: x0,
                            y: center - half,
                            width: (x1 - x0).max(1.0 / hpr),
                            height: BAR_HEIGHT,
                        };
                        if let Some(rect) = layout_irect(bar, hpr, vpr) {
                            out.push(Prim::Rect {
                                rect,
                                color: track_color,
                            });
                        }
                        if task.progress > 0.0
                            && let Some(rect) = layout_irect(
                                GeneralPlotRect {
                                    width: bar.width * task.progress,
                                    ..bar
                                },
                                hpr,
                                vpr,
                            )
                        {
                            out.push(Prim::Rect {
                                rect,
                                color: series.color,
                            });
                        }
                        if series.options.data_labels {
                            self.push_general_layout_label(
                                out,
                                &format!("{}%", (task.progress * 100.0).round()),
                                bar,
                                if task.progress >= 0.5 {
                                    series.color
                                } else {
                                    track_color
                                },
                                hpr,
                                vpr,
                            );
                        }
                    }
                }
            }
            if grid.horz_lines.visible {
                out.push(Prim::HLine {
                    y: device_y(top + ROW_HEIGHT),
                    x0: device_x(plot.x),
                    x1: device_x(right),
                    width: line_width,
                    style: crate::line_style_from_u8(grid.horz_lines.style),
                    color: horizontal_color,
                });
            }
        }

        if tasks.iter().any(|task| !task.dependencies.is_empty()) {
            let index: HashMap<&str, usize> = tasks
                .iter()
                .enumerate()
                .map(|(row, task)| (task.id.as_str(), row))
                .collect();
            let mut task_rows = vec![0usize; tasks.len()];
            for (row, kind) in geometry.rows.iter().enumerate() {
                if let GanttRow::Task(task) = kind {
                    task_rows[*task] = row;
                }
            }
            let color = Color::rgba(text_color.r(), text_color.g(), text_color.b(), 150);
            let width = hpr.min(vpr) as f32;
            for row in geometry.visible() {
                let GanttRow::Task(successor) = geometry.rows[row] else {
                    continue;
                };
                let target = &tasks[successor];
                let target_y = geometry.row_top(row) + ROW_HEIGHT * 0.5;
                for dependency in &target.dependencies {
                    let predecessor = index[dependency.as_str()];
                    // Both ends must be on screen; a route to a scrolled-out row is not drawn.
                    let Some(source_y) = geometry.visible_center(task_rows[predecessor]) else {
                        continue;
                    };
                    let from = (geometry.extent(&tasks[predecessor]).1, source_y);
                    let to = (geometry.extent(target).0, target_y);
                    let (route, count) = dependency_route(from, to);
                    let first_point = points.len() as u32;
                    points.extend(
                        route[..count]
                            .iter()
                            .map(|(x, y)| [(x * hpr) as f32, (y * vpr) as f32]),
                    );
                    out.push(Prim::Polyline {
                        first_point,
                        point_count: count as u32,
                        width,
                        style: LineStyle::Solid,
                        line_type: LineType::Simple,
                        color,
                    });
                    let point = |(px, py): (f64, f64)| [(px * hpr) as f32, (py * vpr) as f32];
                    out.push(Prim::Triangle {
                        a: point(to),
                        b: point((to.0 - ARROW_LENGTH, to.1 - ARROW_HALF_WIDTH)),
                        c: point((to.0 - ARROW_LENGTH, to.1 + ARROW_HALF_WIDTH)),
                        color,
                    });
                }
            }
        }

        if let Some(now) = now
            && now >= geometry.domain.0
            && now <= geometry.domain.1
        {
            out.push(Prim::VLine {
                x: device_x(geometry.x(now)),
                y0: device_y(plot.y),
                y1: device_y(bottom),
                width: line_width,
                style: LineStyle::Dashed,
                color: NOW_COLOR,
            });
        }

        if geometry.rows.len() > geometry.visible_rows {
            let track_height = bottom - geometry.body_top;
            let total = geometry.rows.len() as f64;
            let thumb = GeneralPlotRect {
                x: right - SCROLL_THUMB_WIDTH,
                y: geometry.body_top + track_height * geometry.first_row as f64 / total,
                width: SCROLL_THUMB_WIDTH,
                height: (track_height * geometry.visible_rows as f64 / total).max(8.0),
            };
            if let Some(rect) = layout_irect(thumb, hpr, vpr) {
                out.push(Prim::Rect {
                    rect,
                    color: Color::rgba(text_color.r(), text_color.g(), text_color.b(), 96),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, start: f64, end: f64) -> GanttTask {
        GanttTask {
            id: id.into(),
            label: id.into(),
            start,
            end,
            progress: 0.0,
            group: None,
            dependencies: Vec::new(),
        }
    }

    const DAY: f64 = 86_400_000.0;

    #[test]
    fn gantt_validation_rejects_bad_times_progress_groups_and_dependency_cycles() {
        assert!(validate(&[task("a", 0.0, DAY)], Some(DAY * 0.5)).is_ok());
        assert!(validate(&[task("a", DAY, 0.0)], None).is_err());
        assert!(validate(&[task("a", f64::NAN, DAY)], None).is_err());
        assert!(validate(&[task("a", 0.0, DAY)], Some(f64::INFINITY)).is_err());
        let mut over = task("a", 0.0, DAY);
        over.progress = 1.5;
        assert!(validate(&[over], None).is_err());
        let grouped = |id: &str, group: &str| GanttTask {
            group: Some(group.into()),
            ..task(id, 0.0, DAY)
        };
        assert!(
            validate(
                &[grouped("a", "x"), grouped("b", "x"), grouped("c", "y")],
                None
            )
            .is_ok()
        );
        assert!(
            validate(
                &[grouped("a", "x"), grouped("b", "y"), grouped("c", "x")],
                None
            )
            .is_err()
        );
        let depends = |id: &str, on: &[&str]| GanttTask {
            dependencies: on.iter().map(|value| (*value).to_owned()).collect(),
            ..task(id, 0.0, DAY)
        };
        assert!(validate(&[depends("a", &[]), depends("b", &["a"])], None).is_ok());
        assert!(validate(&[depends("a", &["b"]), depends("b", &["a"])], None).is_err());
        assert!(validate(&[depends("a", &["a"])], None).is_err());
        assert!(validate(&[depends("a", &["missing"])], None).is_err());
        assert!(validate(&[depends("a", &[]), depends("b", &["a", "a"])], None).is_err());
        let many: Vec<String> = (0..=MAX_TASK_DEPENDENCIES)
            .map(|i| format!("t{i}"))
            .collect();
        let mut rows: Vec<GanttTask> = many.iter().map(|id| task(id, 0.0, DAY)).collect();
        rows.push(GanttTask {
            dependencies: many,
            ..task("last", 0.0, DAY)
        });
        assert!(validate(&rows, None).is_err());
    }

    #[test]
    fn dependency_routes_are_orthogonal_and_double_back_when_the_successor_starts_early() {
        let (route, count) = dependency_route((100.0, 50.0), (200.0, 78.0));
        assert_eq!(count, 4);
        for pair in route[..count].windows(2) {
            assert!(pair[0].0 == pair[1].0 || pair[0].1 == pair[1].1);
        }
        assert_eq!(route[count - 1], (200.0 - ARROW_LENGTH, 78.0));
        let (route, count) = dependency_route((100.0, 50.0), (104.0, 106.0));
        assert_eq!(count, 6);
        for pair in route[..count].windows(2) {
            assert!(pair[0].0 == pair[1].0 || pair[0].1 == pair[1].1);
        }
        assert_eq!(route[2].1, 50.0 + ROW_HEIGHT * 0.5);
        assert!(route[3].0 < 104.0);
    }

    #[test]
    fn rows_put_one_header_before_each_contiguous_group() {
        let grouped = |id: &str, group: Option<&str>| GanttTask {
            group: group.map(str::to_owned),
            ..task(id, 0.0, DAY)
        };
        let rows = gantt_rows(&[
            grouped("a", None),
            grouped("b", Some("x")),
            grouped("c", Some("x")),
            grouped("d", Some("y")),
        ]);
        assert_eq!(
            rows,
            vec![
                GanttRow::Task(0),
                GanttRow::Group(1),
                GanttRow::Task(1),
                GanttRow::Task(2),
                GanttRow::Group(3),
                GanttRow::Task(3),
            ]
        );
    }
}
