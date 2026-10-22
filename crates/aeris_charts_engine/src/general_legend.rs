//! Engine-drawn legend of general panes. Its entries are ordered-frame primitives at a
//! selected plot edge or inset, so every executor paints the same legend. The engine input
//! controller toggles series visibility from it without host wiring.

use aeris_charts_core::options::GeneralLegendPosition;
use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{Prim, TextAlign};

use crate::frame::{SERIES_MARKER_WIDTH, series_marker_prim};
use crate::{ChartEngine, ChartInputEvent, GeneralLayoutSeriesId, GeneralSeriesId, HitProfile};

const MARKER_HEIGHT: f64 = 10.0;
const MARKER_GAP: f64 = 6.0;
const ENTRY_GAP: f64 = 16.0;
const ROW_GAP: f64 = 2.0;
const LANE_PAD_TOP: f64 = 6.0;
const LANE_PAD_BOTTOM: f64 = 4.0;
const SIDE_MARGIN: f64 = 8.0;
const MAX_ROWS: usize = 3;
const MAX_SIDE_ROWS: usize = 16;
const MAX_SIDE_SHARE: f64 = 0.25;
/// The legend never takes more than this share of its pane; rows past it fold into "+N more".
const MAX_PANE_SHARE: f64 = 0.3;
/// Opacity of a hidden series' entry, matching the browser legend's dimmed buttons.
const HIDDEN_OPACITY: f64 = 0.45;
/// Extra pointer slack around an entry so the gap between marker and text is never a dead zone.
const HIT_SLACK: f64 = 4.0;

/// What clicking a legend entry toggles. Pie, polar-area and radial-bar slices are a color key
/// for one series and carry no target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GeneralLegendTarget {
    Series(GeneralSeriesId),
    Layout(GeneralLayoutSeriesId),
}

#[derive(Clone, Debug, PartialEq)]
struct LegendEntry {
    target: Option<GeneralLegendTarget>,
    label: String,
    /// `None` for the trailing "+N more" summary, which has no marker.
    color: Option<Color>,
    visible: bool,
    /// Pane-local left edge of the marker.
    x: f64,
    width: f64,
    row: usize,
}

/// One pane's measured legend. Rows are relative to the selected lane or inset top.
#[derive(Clone, Debug, PartialEq)]
struct PaneLegend {
    height: f64,
    width: f64,
    row_height: f64,
    entries: Vec<LegendEntry>,
}

impl PaneLegend {
    fn row_center(&self, lane_top: f64, row: usize) -> f64 {
        lane_top + LANE_PAD_TOP + row as f64 * (self.row_height + ROW_GAP) + self.row_height * 0.5
    }
}

/// Everything a measured legend depends on besides scene content. Scene changes cover series,
/// data, titles, colors, visibility and options; the rest are viewport and text metrics.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LegendKey {
    scene: u64,
    layout: u64,
    pane_w: u64,
    pane_left: u64,
    css_width: u64,
    css_height: u64,
    font_size: u64,
    measured: bool,
}

/// Measured legends for every pane, rebuilt at most once per scene or layout change so plot
/// geometry and hit tests never re-measure text.
#[derive(Default)]
pub(crate) struct GeneralLegendCache {
    key: Option<LegendKey>,
    panes: Vec<Option<PaneLegend>>,
}

fn faded(color: Color, visible: bool) -> Color {
    if visible {
        color
    } else {
        let alpha = (f64::from(color.a()) * HIDDEN_OPACITY).round() as u8;
        Color::rgba(color.r(), color.g(), color.b(), alpha)
    }
}

fn fit_legend_label(label: &str, max_width: f64, measure: impl Fn(&str) -> f64) -> String {
    if measure(label) <= max_width {
        return label.to_owned();
    }
    let ellipsis = "…";
    if measure(ellipsis) > max_width {
        return String::new();
    }
    let mut clipped = String::new();
    for ch in label.chars().take(128) {
        clipped.push(ch);
        let mut candidate = clipped.clone();
        candidate.push_str(ellipsis);
        if measure(&candidate) > max_width {
            clipped.pop();
            break;
        }
    }
    clipped.push_str(ellipsis);
    clipped
}
impl ChartEngine {
    fn general_legend_key(&self) -> LegendKey {
        LegendKey {
            scene: self.frame_invalidation.scene_generation(),
            layout: self.frame_invalidation.layout_generation(),
            pane_w: self.pane_w.to_bits(),
            pane_left: self.pane_left.to_bits(),
            css_width: self.css_width.to_bits(),
            css_height: self.css_height.to_bits(),
            font_size: self.axis_metrics().axis.to_bits(),
            measured: self.has_text_measure(),
        }
    }

    fn with_measured_general_legend<R>(
        &self,
        pane_index: usize,
        read: impl FnOnce(&PaneLegend) -> R,
    ) -> Option<R> {
        if !self.options.get().general_legend.visible {
            return None;
        }
        let key = self.general_legend_key();
        if self.general_legend_cache.borrow().key != Some(key) {
            let panes = (0..self.panes.len())
                .map(|pane| self.measure_pane_legend(pane))
                .collect();
            *self.general_legend_cache.borrow_mut() = GeneralLegendCache {
                key: Some(key),
                panes,
            };
        }
        let cache = self.general_legend_cache.borrow();
        let legend = cache.panes.get(pane_index)?.as_ref()?;
        Some(read(legend))
    }

    fn with_general_legend<R>(
        &self,
        pane_index: usize,
        read: impl FnOnce(&PaneLegend, f64) -> R,
    ) -> Option<R> {
        self.with_measured_general_legend(pane_index, |legend| {
            let pane = self.panes.get(pane_index)?;
            let position = self.options.get().general_legend.position;
            let offset = self.general_legend_offset(pane.height);
            let lane_top = match position {
                GeneralLegendPosition::Top => pane.top + offset,
                GeneralLegendPosition::Bottom => pane.top + pane.height - legend.height + offset,
                GeneralLegendPosition::Custom { y, .. } => {
                    pane.top + y.resolve(pane.height) - legend.height
                }
                GeneralLegendPosition::Left
                | GeneralLegendPosition::Right
                | GeneralLegendPosition::InsideLeft
                | GeneralLegendPosition::InsideRight => {
                    pane.top + (pane.height - legend.height).max(0.0) * 0.5
                }
                GeneralLegendPosition::Center
                | GeneralLegendPosition::InsideTop
                | GeneralLegendPosition::InsideBottom
                | GeneralLegendPosition::InsideTopLeft
                | GeneralLegendPosition::InsideTopRight
                | GeneralLegendPosition::InsideBottomLeft
                | GeneralLegendPosition::InsideBottomRight => {
                    let plot = self.general_plot_rect(pane_index)?;
                    let content_height = legend.height - offset;
                    match position {
                        GeneralLegendPosition::InsideTop
                        | GeneralLegendPosition::InsideTopLeft
                        | GeneralLegendPosition::InsideTopRight => plot.y + offset,
                        GeneralLegendPosition::InsideBottom
                        | GeneralLegendPosition::InsideBottomLeft
                        | GeneralLegendPosition::InsideBottomRight => {
                            plot.y + plot.height - content_height - offset
                        }
                        GeneralLegendPosition::Center => {
                            plot.y + (plot.height - content_height) * 0.5
                        }
                        GeneralLegendPosition::Top
                        | GeneralLegendPosition::Bottom
                        | GeneralLegendPosition::Left
                        | GeneralLegendPosition::Right
                        | GeneralLegendPosition::InsideLeft
                        | GeneralLegendPosition::InsideRight
                        | GeneralLegendPosition::Custom { .. } => {
                            unreachable!()
                        }
                    }
                }
            };
            Some(read(legend, lane_top))
        })
        .flatten()
    }

    fn general_legend_offset(&self, pane_height: f64) -> f64 {
        let minimum_row = (self.axis_metrics().axis + 6.0).ceil() + LANE_PAD_TOP + LANE_PAD_BOTTOM;
        let available = (pane_height * MAX_PANE_SHARE - minimum_row).max(0.0);
        self.options
            .get()
            .general_legend
            .offset
            .clamp(0.0, available)
    }

    /// Height the legend reserves at the configured edge of a general pane, or zero without one.
    pub(crate) fn general_legend_height(&self, pane_index: usize) -> f64 {
        if !matches!(
            self.options.get().general_legend.position,
            GeneralLegendPosition::Top | GeneralLegendPosition::Bottom
        ) {
            return 0.0;
        }
        self.with_measured_general_legend(pane_index, |legend| legend.height)
            .unwrap_or(0.0)
    }

    pub(crate) fn general_legend_side_width(
        &self,
        pane_index: usize,
        side: crate::PriceScaleSide,
    ) -> f64 {
        let position = self.options.get().general_legend.position;
        if !matches!(
            (position, side),
            (GeneralLegendPosition::Left, crate::PriceScaleSide::Left)
                | (GeneralLegendPosition::Right, crate::PriceScaleSide::Right)
        ) {
            return 0.0;
        }
        self.with_measured_general_legend(pane_index, |legend| legend.width)
            .unwrap_or(0.0)
    }
    fn measure_pane_legend(&self, pane_index: usize) -> Option<PaneLegend> {
        if self.pane_uses_financial_time(pane_index) {
            return None;
        }
        let pane = self.panes.get(pane_index)?;
        let pane_id = pane.stable_id?;
        let mut items: Vec<(Option<GeneralLegendTarget>, String, Color, bool)> = Vec::new();
        for item in self.general_legend_snapshot(Some(pane_index)).items {
            if item.slices.is_empty() {
                let title = if item.title.is_empty() {
                    format!("Series {}", items.len() + 1)
                } else {
                    item.title
                };
                let color = item
                    .color
                    .as_deref()
                    .and_then(Color::parse_css)
                    .unwrap_or(crate::DEFAULT_LINE_COLOR);
                items.push((
                    Some(GeneralLegendTarget::Series(item.series)),
                    title,
                    color,
                    item.visible,
                ));
            } else {
                for slice in item.slices {
                    let color = Color::parse_css(&slice.color).unwrap_or(crate::DEFAULT_LINE_COLOR);
                    items.push((None, slice.label, color, item.visible));
                }
            }
        }
        for (id, title, color, visible) in self.general_layout_legend_entries(pane_id) {
            items.push((Some(GeneralLegendTarget::Layout(id)), title, color, visible));
        }
        if items.is_empty() {
            return None;
        }

        let size = self.axis_metrics().axis;
        let family = self.options.get().layout.font_family.clone();
        let measure = |text: &str| self.measure_text_run(text, size, &family, 400, false);
        let row_height = (size + 6.0).ceil();
        let offset = if matches!(
            self.options.get().general_legend.position,
            GeneralLegendPosition::Custom { .. }
        ) {
            0.0
        } else {
            self.general_legend_offset(pane.height)
        };
        let max_rows = ((pane.height * MAX_PANE_SHARE - offset - LANE_PAD_TOP - LANE_PAD_BOTTOM
            + ROW_GAP)
            / (row_height + ROW_GAP))
            .floor()
            .clamp(0.0, MAX_ROWS as f64) as usize;
        let side_position = matches!(
            self.options.get().general_legend.position,
            GeneralLegendPosition::Left
                | GeneralLegendPosition::Right
                | GeneralLegendPosition::InsideLeft
                | GeneralLegendPosition::InsideRight
        );
        if max_rows == 0 && !side_position {
            return None;
        }
        let available = (self.pane_w - 2.0 * SIDE_MARGIN).max(0.0);
        let entry_width = |label: &str| SERIES_MARKER_WIDTH + MARKER_GAP + measure(label);
        let position = self.options.get().general_legend.position;
        if matches!(
            position,
            GeneralLegendPosition::Left
                | GeneralLegendPosition::Right
                | GeneralLegendPosition::InsideLeft
                | GeneralLegendPosition::InsideRight
        ) {
            let side_limit = (self.css_width * MAX_SIDE_SHARE).max(1.0);
            let side_offset = self
                .options
                .get()
                .general_legend
                .offset
                .clamp(0.0, self.css_width * 0.1);
            let text_limit =
                (side_limit - 2.0 * SIDE_MARGIN - side_offset - SERIES_MARKER_WIDTH - MARKER_GAP)
                    .max(0.0);
            let max_side_rows = ((pane.height - LANE_PAD_TOP - LANE_PAD_BOTTOM + ROW_GAP)
                / (row_height + ROW_GAP))
                .floor()
                .clamp(0.0, MAX_SIDE_ROWS as f64) as usize;
            if max_side_rows == 0 {
                return None;
            }
            let hidden = items.len().saturating_sub(max_side_rows);
            let visible = if hidden > 0 {
                max_side_rows - 1
            } else {
                items.len()
            };
            let mut entries = Vec::with_capacity(max_side_rows.min(items.len()));
            for (row, (target, label, color, visible)) in
                items.into_iter().take(visible).enumerate()
            {
                let label = fit_legend_label(&label, text_limit, measure);
                let width = entry_width(&label);
                entries.push(LegendEntry {
                    target,
                    label,
                    color: Some(color),
                    visible,
                    x: 0.0,
                    width,
                    row,
                });
            }
            if hidden > 0 {
                let label = fit_legend_label(
                    &format!("+{} more", hidden + 1),
                    side_limit - 2.0 * SIDE_MARGIN,
                    measure,
                );
                entries.push(LegendEntry {
                    target: None,
                    width: measure(&label),
                    label,
                    color: None,
                    visible: true,
                    x: 0.0,
                    row: entries.len(),
                });
            }
            let content_width = entries.iter().map(|entry| entry.width).fold(0.0, f64::max);
            let width = (content_width + 2.0 * SIDE_MARGIN + side_offset).min(side_limit);
            let start = match position {
                GeneralLegendPosition::Left => -self.pane_left + SIDE_MARGIN,
                GeneralLegendPosition::Right => {
                    self.css_width - self.pane_left - width + SIDE_MARGIN
                }
                GeneralLegendPosition::InsideLeft => SIDE_MARGIN + side_offset,
                GeneralLegendPosition::InsideRight => {
                    (self.pane_w - SIDE_MARGIN - side_offset - content_width).max(SIDE_MARGIN)
                }
                _ => unreachable!(),
            };
            for entry in &mut entries {
                entry.x = start.round();
            }
            let count = entries.len() as f64;
            return Some(PaneLegend {
                width,
                height: (LANE_PAD_TOP
                    + count * row_height
                    + (count - 1.0).max(0.0) * ROW_GAP
                    + LANE_PAD_BOTTOM)
                    .ceil(),
                row_height,
                entries,
            });
        }

        // Greedy wrapping that stops measuring once the row budget is exhausted.
        let mut rows: Vec<Vec<LegendEntry>> = vec![Vec::new()];
        let mut row_width = 0.0;
        let mut placed = 0;
        for (target, label, color, visible) in &items {
            let width = entry_width(label);
            let current = rows.last().map_or(0, Vec::len);
            if current > 0 && row_width + ENTRY_GAP + width > available {
                if rows.len() == max_rows {
                    break;
                }
                rows.push(Vec::new());
                row_width = 0.0;
            }
            let row = rows.len() - 1;
            if !rows[row].is_empty() {
                row_width += ENTRY_GAP;
            }
            rows[row].push(LegendEntry {
                target: *target,
                label: label.clone(),
                color: Some(*color),
                visible: *visible,
                x: row_width,
                width,
                row,
            });
            row_width += width;
            placed += 1;
        }
        let hidden = items.len() - placed;
        if hidden > 0 {
            let label = format!("+{hidden} more");
            let width = measure(&label);
            let last = rows.len() - 1;
            let used = |row: &[LegendEntry]| row.last().map_or(0.0, |entry| entry.x + entry.width);
            while rows[last].len() > 1 && used(&rows[last]) + ENTRY_GAP + width > available {
                rows[last].pop();
            }
            let x = if rows[last].is_empty() {
                0.0
            } else {
                used(&rows[last]) + ENTRY_GAP
            };
            rows[last].push(LegendEntry {
                target: None,
                label,
                color: None,
                visible: true,
                x,
                width,
                row: last,
            });
        }

        let mut entries = Vec::with_capacity(rows.iter().map(Vec::len).sum());
        for row in rows {
            let width = row.last().map_or(0.0, |entry| entry.x + entry.width);
            let start = match self.options.get().general_legend.position {
                GeneralLegendPosition::InsideTopLeft | GeneralLegendPosition::InsideBottomLeft => {
                    SIDE_MARGIN + offset
                }
                GeneralLegendPosition::InsideTopRight
                | GeneralLegendPosition::InsideBottomRight => {
                    (self.pane_w - SIDE_MARGIN - offset - width).max(SIDE_MARGIN)
                }
                GeneralLegendPosition::Custom { x, .. } => {
                    x.resolve(self.css_width) - self.pane_left - width
                }
                _ => ((self.pane_w - width) * 0.5).max(SIDE_MARGIN),
            }
            .round();
            entries.extend(row.into_iter().map(|entry| LegendEntry {
                x: start + entry.x,
                ..entry
            }));
        }
        let row_count = entries.iter().map(|entry| entry.row + 1).max().unwrap_or(1) as f64;
        Some(PaneLegend {
            width: self.pane_w,
            height: (offset
                + LANE_PAD_TOP
                + row_count * row_height
                + (row_count - 1.0) * ROW_GAP
                + LANE_PAD_BOTTOM)
                .ceil(),
            row_height,
            entries,
        })
    }

    /// Paint an in-pane legend; outside side and custom legends use the axis layer.
    pub(crate) fn build_general_legend_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        if matches!(
            self.options.get().general_legend.position,
            GeneralLegendPosition::Left
                | GeneralLegendPosition::Right
                | GeneralLegendPosition::Custom { .. }
        ) {
            return;
        }
        self.append_general_legend_prims(pane_index, hpr, vpr, 0.0, out);
    }

    /// Outside side and custom legends paint after axis chrome in its unscissored layer.
    pub(crate) fn build_general_legend_axis(
        &self,
        pane_index: usize,
        dpr: f64,
        out: &mut Vec<Prim>,
    ) {
        if !matches!(
            self.options.get().general_legend.position,
            GeneralLegendPosition::Left
                | GeneralLegendPosition::Right
                | GeneralLegendPosition::Custom { .. }
        ) {
            return;
        }
        self.append_general_legend_prims(pane_index, dpr, dpr, self.pane_left, out);
    }

    fn append_general_legend_prims(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        x_origin: f64,
        out: &mut Vec<Prim>,
    ) {
        let size = self.axis_metrics().axis;
        let family = self.options.get().layout.font_family.clone();
        let text_color = self.primary_text_color();
        self.with_general_legend(pane_index, |legend, lane_top| {
            for entry in &legend.entries {
                let center_y = legend.row_center(lane_top, entry.row);
                let text_x = match entry.color {
                    Some(color) => {
                        out.push(series_marker_prim(
                            entry.x + x_origin,
                            center_y,
                            MARKER_HEIGHT,
                            faded(color, entry.visible),
                            hpr,
                            vpr,
                        ));
                        entry.x + SERIES_MARKER_WIDTH + MARKER_GAP
                    }
                    None => entry.x,
                };
                out.push(Prim::Text {
                    x: ((text_x + x_origin) * hpr) as f32,
                    y: (center_y * vpr) as f32,
                    text: entry.label.clone(),
                    color: faded(text_color, entry.visible),
                    size: (size * vpr) as f32,
                    family: family.clone(),
                    align: TextAlign::Left,
                    weight: 400,
                    italic: false,
                });
            }
        });
    }
    /// The toggleable legend entry under a pane-local `x` and chart `y`, if any.
    pub(crate) fn general_legend_target_at(&self, x: f64, y: f64) -> Option<GeneralLegendTarget> {
        self.general_legend_target_at_for_touch(x, y, false)
    }

    pub(crate) fn general_legend_target_at_for_touch(
        &self,
        x: f64,
        y: f64,
        touch: bool,
    ) -> Option<GeneralLegendTarget> {
        if touch {
            // A painted entry wins before expanded touch bounds can overlap it.
            if let Some(target) = self.general_legend_target_at(x, y) {
                return Some(target);
            }
        }
        let hit = |legend: &PaneLegend, lane_top: f64| {
            if !touch {
                return legend.entries.iter().find_map(|entry| {
                    let center_y = legend.row_center(lane_top, entry.row);
                    let inside = x >= entry.x - HIT_SLACK
                        && x <= entry.x + entry.width + HIT_SLACK
                        && (y - center_y).abs() <= (legend.row_height + ROW_GAP) * 0.5;
                    inside.then_some(entry.target).flatten()
                });
            }
            if legend.entries.iter().any(|entry| {
                entry.target.is_none()
                    && x >= entry.x
                    && x <= entry.x + entry.width
                    && (y - legend.row_center(lane_top, entry.row)).abs() <= legend.row_height * 0.5
            }) {
                return None;
            }
            let half = HitProfile::TOUCH.control_half_size;
            legend
                .entries
                .iter()
                .filter_map(|entry| {
                    let target = entry.target?;
                    let center_y = legend.row_center(lane_top, entry.row);
                    let dy = (y - center_y).abs();
                    if x < entry.x - half || x > entry.x + entry.width + half || dy > half {
                        return None;
                    }
                    let dx = (entry.x - x).max(x - entry.x - entry.width).max(0.0);
                    let dy = (dy - legend.row_height * 0.5).max(0.0);
                    Some((target, dx * dx + dy * dy))
                })
                .min_by(|left, right| left.1.total_cmp(&right.1))
                .map(|(target, _)| target)
        };
        if matches!(
            self.options.get().general_legend.position,
            GeneralLegendPosition::Custom { .. }
        ) {
            // Custom coordinates may cross pane bounds. The axis layer paints later panes last.
            return (0..self.panes.len())
                .rev()
                .find_map(|pane| self.with_general_legend(pane, hit).flatten());
        }
        let pane = self.pane_at_y(y)?;
        self.with_general_legend(pane, hit).flatten()
    }

    /// Toggle the series behind a legend entry and report it so hosts refresh legend observers.
    pub(crate) fn toggle_general_legend_target(&mut self, target: GeneralLegendTarget) -> bool {
        let toggled = match target {
            GeneralLegendTarget::Series(id) => self
                .general_series(id)
                .map(|series| series.visible())
                .is_some_and(|visible| self.set_general_series_visible(id, !visible)),
            GeneralLegendTarget::Layout(id) => self.toggle_general_layout_series_visible(id),
        };
        if toggled {
            self.push_input_event(ChartInputEvent::GeneralLegendChanged);
        }
        toggled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AxisDimension, CategoryScaleType, GeneralAxisOptions, GeneralScaleType,
        GeneralSeriesOptions, GeneralXyInput, HorizontalDomain, InputDevice, PointerInput,
    };

    fn chart(series: &[(&str, &str)]) -> ChartEngine {
        let mut chart = ChartEngine::new_with_initial_domain(
            480.0,
            300.0,
            1.0,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
        chart
            .add_general_axis(GeneralAxisOptions::new(
                "x",
                0,
                AxisDimension::X,
                GeneralScaleType::Band,
            ))
            .unwrap();
        chart
            .add_general_axis(GeneralAxisOptions::new(
                "y",
                0,
                AxisDimension::Y,
                GeneralScaleType::Linear,
            ))
            .unwrap();
        for (title, color) in series {
            let dataset = chart
                .create_general_xy_dataset(GeneralXyInput::Category {
                    ids: None,
                    categories: vec!["A".into(), "B".into()],
                    category_indices: vec![0, 1],
                    y: vec![2.0, 5.0],
                    y_valid: None,
                })
                .unwrap();
            let mut options = GeneralSeriesOptions::column(0, dataset, "x", "y");
            options.title = (*title).into();
            options.color = Some((*color).into());
            options.group_id = Some("g".into());
            chart.add_general_series(options).unwrap();
        }
        let _ = chart.build_frame();
        chart
    }

    fn legend_texts(chart: &mut ChartEngine) -> Vec<String> {
        let frame = chart.build_frame();
        frame.panes[0]
            .main
            .iter()
            .filter_map(|prim| match prim {
                Prim::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn hidden_by_default_and_reserves_no_space() {
        let chart = chart(&[("Revenue", "#336699")]);
        assert_eq!(chart.general_legend_height(0), 0.0);
        assert!(chart.general_legend_target_at(240.0, 290.0).is_none());
    }

    #[test]
    fn visible_legend_reserves_a_lane_and_paints_pills_and_labels() {
        let mut chart = chart(&[("Revenue", "#336699"), ("Cost", "#cc6633")]);
        let plot_without = chart.general_plot_rect(0).unwrap();
        chart
            .apply_options(r#"{"generalLegend":{"visible":true}}"#)
            .unwrap();
        let lane = chart.general_legend_height(0);
        assert!(lane > 0.0);
        let plot_with = chart.general_plot_rect(0).unwrap();
        assert_eq!(plot_with.height, plot_without.height - lane);

        let texts = legend_texts(&mut chart);
        for label in ["Revenue", "Cost"] {
            assert!(texts.iter().any(|text| text == label), "{label}: {texts:?}");
        }
        let frame = chart.build_frame();
        let pills = frame.panes[0]
            .main
            .iter()
            .filter(|prim| {
                matches!(prim, Prim::RoundRect { w, h, radii, .. }
                    if *w == 4.0 && *h == 10.0 && radii == &[2.0; 4])
            })
            .count();
        assert_eq!(pills, 2, "one rounded pill per series entry");
    }

    #[test]
    fn top_legend_reserves_space_above_the_plot_and_keeps_hits_with_its_frame() {
        let mut chart = chart(&[("Revenue", "#336699")]);
        let before = chart.general_plot_rect(0).unwrap();
        chart
            .apply_options(r#"{"generalLegend":{"visible":true,"position":"top","offset":5}}"#)
            .unwrap();
        let lane = chart.general_legend_height(0);
        let after = chart.general_plot_rect(0).unwrap();
        assert!(lane > 5.0);
        assert_eq!(after.y, before.y + lane);
        assert_eq!(after.height, before.height - lane);
        let _ = chart.build_frame();
        let (x, y, target) = chart
            .with_general_legend(0, |legend, lane_top| {
                let entry = &legend.entries[0];
                (
                    entry.x + entry.width * 0.5,
                    legend.row_center(lane_top, 0),
                    entry.target,
                )
            })
            .unwrap();
        assert!(y < after.y);
        assert_eq!(chart.general_legend_target_at(x, y), target);
    }
    #[test]
    fn inset_legend_positions_overlay_the_plot_and_move_hits_with_entries() {
        for (position, horizontal) in [
            ("insideTop", "center"),
            ("insideBottom", "center"),
            ("insideTopLeft", "left"),
            ("insideTopRight", "right"),
            ("insideBottomLeft", "left"),
            ("insideBottomRight", "right"),
            ("center", "center"),
        ] {
            let mut chart = chart(&[("Revenue", "#336699")]);
            let before = chart.general_plot_rect(0).unwrap();
            chart
                .apply_options(&format!(
                    r#"{{"generalLegend":{{"visible":true,"position":"{position}","offset":8}}}}"#
                ))
                .unwrap();
            let after = chart.general_plot_rect(0).unwrap();
            assert_eq!(
                after, before,
                "{position} must overlay instead of reserving a lane"
            );
            let _ = chart.build_frame();
            let (x, y, target) = chart
                .with_general_legend(0, |legend, lane_top| {
                    let entry = &legend.entries[0];
                    (
                        entry.x + entry.width * 0.5,
                        legend.row_center(lane_top, 0),
                        entry.target,
                    )
                })
                .unwrap();
            assert!(
                y >= after.y && y <= after.y + after.height,
                "{position}: {y}"
            );
            if position.starts_with("insideTop") {
                assert!(y < after.y + after.height * 0.3, "{position}: {y}");
            } else if position.starts_with("insideBottom") {
                assert!(y > after.y + after.height * 0.7, "{position}: {y}");
            }
            match horizontal {
                "left" => assert!(x < chart.pane_w * 0.3, "{position}: {x}"),
                "right" => assert!(x > chart.pane_w * 0.7, "{position}: {x}"),
                _ => assert!((x - chart.pane_w * 0.5).abs() < chart.pane_w * 0.1),
            }
            assert_eq!(chart.general_legend_target_at(x, y), target);
        }
    }

    #[test]
    fn custom_legend_coordinates_anchor_entries_without_reserving_plot_space() {
        for (position, expected_x, expected_y) in [
            (r#"{"x":120,"y":140}"#, 120.0, 140.0),
            (r#"{"x":"80%","y":"60%"}"#, 480.0 * 0.8, 300.0 * 0.6),
        ] {
            let mut chart = chart(&[("Revenue", "#336699")]);
            let before = chart.general_plot_rect(0).unwrap();
            chart
                .apply_options(&format!(
                    r#"{{"generalLegend":{{"visible":true,"position":{position},"offset":99}}}}"#
                ))
                .unwrap();
            assert_eq!(chart.general_plot_rect(0).unwrap(), before);
            let _ = chart.build_frame();
            let (right, bottom, x, y, target) = chart
                .with_general_legend(0, |legend, lane_top| {
                    let entry = &legend.entries[0];
                    (
                        entry.x + entry.width,
                        lane_top + legend.height,
                        entry.x + entry.width * 0.5,
                        legend.row_center(lane_top, 0),
                        entry.target,
                    )
                })
                .unwrap();
            assert!((right - expected_x).abs() <= 1.0, "{position}: {right}");
            assert!((bottom - expected_y).abs() <= 1.0, "{position}: {bottom}");
            assert_eq!(chart.general_legend_target_at(x, y), target);
        }
    }
    #[test]
    fn custom_legend_uses_chart_coordinates_and_unscissored_axis_layer_with_axes() {
        let mut chart = chart(&[("Revenue", "#336699")]);
        chart
            .apply_options(r#"{"generalLegend":{"visible":true,"position":{"x":120,"y":140}}}"#)
            .unwrap();
        let mut frame = crate::ChartFrame::default();
        let mut axis = Vec::new();
        chart.prepare_financial_frame_with_measure(
            crate::FinancialFrameRequest {
                width: 480.0,
                height: 300.0,
                dpr: 1.0,
                force_layout: true,
                allow_axis_shrink: true,
                force_frame: true,
                force_axis: true,
                layout_only: false,
                fit_content: false,
                frame: &mut frame,
                axis_frame: None,
                axis_primitives: Some(&mut axis),
            },
            |text, _| text.chars().count() as f64 * 7.0,
            |text, _| text.chars().count() as f64 * 7.0,
        );
        assert!(chart.pane_left > 0.0);
        let (right, x, y, target) = chart
            .with_general_legend(0, |legend, lane_top| {
                let entry = &legend.entries[0];
                (
                    chart.pane_left + entry.x + entry.width,
                    entry.x + entry.width * 0.5,
                    legend.row_center(lane_top, 0),
                    entry.target,
                )
            })
            .unwrap();
        assert!(
            (right - 120.0).abs() <= 1.0,
            "chart-relative right edge: {right}"
        );
        assert_eq!(chart.general_legend_target_at(x, y), target);
        assert!(
            axis.iter()
                .any(|prim| matches!(prim, Prim::Text { text, .. } if text == "Revenue"))
        );
        assert!(
            !frame.panes[0]
                .main
                .iter()
                .any(|prim| matches!(prim, Prim::Text { text, .. } if text == "Revenue"))
        );
    }
    #[test]
    fn custom_legend_hit_follows_its_paint_across_a_pane_boundary() {
        let mut chart = chart(&[("Revenue", "#336699")]);
        let second = chart.add_pane(true).unwrap();
        assert_eq!(second, 1);
        chart.layout_panes(300.0);
        let boundary = chart.panes[second].top;
        let y = boundary - chart.panes[0].top + 20.0;
        chart
            .apply_options(&format!(
                r#"{{"generalLegend":{{"visible":true,"position":{{"x":120,"y":{y}}}}}}}"#
            ))
            .unwrap();
        let (x, center_y, target) = chart
            .with_general_legend(0, |legend, lane_top| {
                let entry = &legend.entries[0];
                (
                    entry.x + entry.width * 0.5,
                    legend.row_center(lane_top, 0),
                    entry.target,
                )
            })
            .unwrap();
        assert_eq!(chart.pane_at_y(center_y), Some(second));
        assert_eq!(chart.general_legend_target_at(x, center_y), target);
    }
    #[test]
    fn outside_side_legends_reserve_gutters_paint_in_axis_layer_and_toggle() {
        fn prepare(chart: &mut ChartEngine) -> (crate::ChartFrame, Vec<Prim>) {
            let mut frame = crate::ChartFrame::default();
            let mut axis = Vec::new();
            chart.prepare_financial_frame_with_measure(
                crate::FinancialFrameRequest {
                    width: 480.0,
                    height: 300.0,
                    dpr: 1.0,
                    force_layout: true,
                    allow_axis_shrink: true,
                    force_frame: true,
                    force_axis: true,
                    layout_only: false,
                    fit_content: false,
                    frame: &mut frame,
                    axis_frame: None,
                    axis_primitives: Some(&mut axis),
                },
                |text, _| text.chars().count() as f64 * 7.0,
                |text, _| text.chars().count() as f64 * 7.0,
            );
            (frame, axis)
        }
        let mut baseline = chart(&[("Revenue", "#336699")]);
        let _ = prepare(&mut baseline);
        for position in ["left", "right"] {
            let mut chart = chart(&[("Revenue", "#336699")]);
            chart
                .apply_options(&format!(
                    r#"{{"generalLegend":{{"visible":true,"position":"{position}","offset":6}}}}"#
                ))
                .unwrap();
            let (frame, axis) = prepare(&mut chart);
            assert!(
                chart.pane_w < baseline.pane_w,
                "{position} must reserve width"
            );
            if position == "left" {
                assert!(chart.pane_left > baseline.pane_left);
            }
            assert!(
                axis.iter()
                    .any(|prim| matches!(prim, Prim::Text { text, .. } if text == "Revenue"))
            );
            assert!(
                !frame.panes[0]
                    .main
                    .iter()
                    .any(|prim| matches!(prim, Prim::Text { text, .. } if text == "Revenue"))
            );
            let (x, y, target) = chart
                .with_general_legend(0, |legend, lane_top| {
                    let entry = &legend.entries[0];
                    (
                        entry.x + entry.width * 0.5,
                        legend.row_center(lane_top, 0),
                        entry.target,
                    )
                })
                .unwrap();
            assert!(if position == "left" {
                x < 0.0
            } else {
                x > chart.pane_w
            });
            assert_eq!(chart.general_legend_target_at(x, y), target);
            let at = |timestamp_ms| PointerInput {
                x,
                y,
                timestamp_ms,
                ..PointerInput::default()
            };
            chart.input_pointer_down(at(1_000.0), 1);
            chart.input_pointer_up(at(1_010.0));
            let Some(GeneralLegendTarget::Series(series)) = target else {
                panic!("series target")
            };
            assert!(!chart.general_series(series).unwrap().visible());
        }
    }
    #[test]
    fn side_legends_bound_long_labels_and_overflow_while_inside_sides_keep_plot_width() {
        let titles: Vec<(String, &str)> = (0..40)
            .map(|index| {
                (
                    format!("Very long 漢字 revenue series {index} with a suffix"),
                    "#336699",
                )
            })
            .collect();
        let refs: Vec<(&str, &str)> = titles
            .iter()
            .map(|(title, color)| (title.as_str(), *color))
            .collect();
        let mut baseline = chart(&refs);
        baseline.recompute_layout_with_measure(
            true,
            |text, _| text.chars().count() as f64 * 7.0,
            |text, _| text.chars().count() as f64 * 7.0,
        );
        for position in ["left", "right", "insideLeft", "insideRight"] {
            let mut chart = chart(&refs);
            chart
                .apply_options(&format!(
                    r#"{{"generalLegend":{{"visible":true,"position":"{position}","offset":6}}}}"#
                ))
                .unwrap();
            chart.recompute_layout_with_measure(
                true,
                |text, _| text.chars().count() as f64 * 7.0,
                |text, _| text.chars().count() as f64 * 7.0,
            );
            let (count, last, has_ellipsis, right_edge) = chart
                .with_measured_general_legend(0, |legend| {
                    (
                        legend.entries.len(),
                        legend.entries.last().unwrap().label.clone(),
                        legend
                            .entries
                            .iter()
                            .any(|entry| entry.label.ends_with('…')),
                        legend
                            .entries
                            .iter()
                            .map(|entry| entry.x + entry.width)
                            .fold(0.0, f64::max),
                    )
                })
                .unwrap();
            assert!(count <= MAX_SIDE_ROWS, "{position}: {count}");
            assert!(last.contains("more"), "{position}: {last}");
            assert!(has_ellipsis, "{position} should elide overlong titles");
            assert!(
                right_edge <= chart.pane_w + chart.axis_w + 1.0
                    || position == "insideLeft"
                    || position == "insideRight"
            );
            if position.starts_with("inside") {
                assert_eq!(chart.pane_w, baseline.pane_w, "{position} must overlay");
            } else {
                assert!(chart.pane_w < baseline.pane_w, "{position} must reserve");
            }
        }
    }
    #[test]
    fn excessive_legend_offset_stays_bounded_without_hiding_entries() {
        let mut chart = chart(&[("Revenue", "#336699")]);
        chart
            .apply_options(r#"{"generalLegend":{"visible":true,"position":"top","offset":100000}}"#)
            .unwrap();
        let lane = chart.general_legend_height(0);
        assert!(lane > 0.0);
        assert!(lane <= 300.0 * MAX_PANE_SHARE);
        assert!(legend_texts(&mut chart).contains(&"Revenue".to_owned()));
    }

    #[test]
    fn clicking_an_entry_toggles_the_series_and_reports_it() {
        let mut chart = chart(&[("Revenue", "#336699"), ("Cost", "#cc6633")]);
        chart
            .apply_options(r#"{"generalLegend":{"visible":true}}"#)
            .unwrap();
        let _ = chart.build_frame();
        let (x, y, target) = chart
            .with_general_legend(0, |legend, lane_top| {
                let entry = &legend.entries[0];
                (
                    entry.x + entry.width * 0.5,
                    legend.row_center(lane_top, entry.row),
                    entry.target,
                )
            })
            .unwrap();
        let Some(GeneralLegendTarget::Series(series)) = target else {
            panic!("the first entry toggles its series");
        };
        assert_eq!(
            chart.general_legend_target_at(x, y),
            Some(GeneralLegendTarget::Series(series))
        );
        let at = |timestamp_ms| PointerInput {
            x,
            y,
            timestamp_ms,
            ..PointerInput::default()
        };
        chart.input_pointer_down(at(1_000.0), 1);
        chart.input_pointer_up(at(1_010.0));
        assert!(!chart.general_series(series).unwrap().visible());
        assert!(
            chart
                .take_input_events()
                .contains(&ChartInputEvent::GeneralLegendChanged)
        );
        let _ = chart.build_frame();
        chart.input_pointer_down(at(5_000.0), 1);
        chart.input_pointer_up(at(5_010.0));
        assert!(chart.general_series(series).unwrap().visible());
    }

    #[test]
    fn touch_legend_press_uses_a_44_pixel_target_without_enlarging_mouse_hits() {
        let mut chart = chart(&[("Revenue", "#336699")]);
        chart
            .apply_options(r#"{"generalLegend":{"visible":true}}"#)
            .unwrap();
        let _ = chart.build_frame();
        let (x, y, target) = chart
            .with_general_legend(0, |legend, lane_top| {
                let entry = &legend.entries[0];
                (
                    entry.x - 15.0,
                    legend.row_center(lane_top, entry.row),
                    entry.target,
                )
            })
            .unwrap();
        let Some(GeneralLegendTarget::Series(series)) = target else {
            panic!("the legend entry must be actionable");
        };
        assert_eq!(chart.general_legend_target_at(x, y), None);
        let at = |timestamp_ms| PointerInput {
            id: 7,
            device: InputDevice::Touch,
            x,
            y,
            timestamp_ms,
            ..PointerInput::default()
        };
        chart.input_pointer_down(at(1_000.0), 1);
        chart.input_pointer_up(at(1_010.0));
        assert!(!chart.general_series(series).unwrap().visible());
    }

    #[test]
    fn overlapping_touch_legend_targets_choose_the_nearest_entry() {
        let mut chart = chart(&[("Revenue", "#336699"), ("Cost", "#cc6633")]);
        chart
            .apply_options(r#"{"generalLegend":{"visible":true}}"#)
            .unwrap();
        let _ = chart.build_frame();
        let (x, y, target) = chart
            .with_general_legend(0, |legend, lane_top| {
                let first = &legend.entries[0];
                let second = &legend.entries[1];
                assert_eq!(first.row, second.row);
                let first_right = first.x + first.width;
                let gap = second.x - first_right;
                assert!(gap > HIT_SLACK * 2.0 && gap < 44.0);
                (
                    first_right + gap * 0.625,
                    legend.row_center(lane_top, first.row),
                    second.target,
                )
            })
            .unwrap();
        assert!(chart.general_legend_target_at(x, y).is_none());
        assert_eq!(chart.general_legend_target_at_for_touch(x, y, true), target);
    }

    #[test]
    fn rows_wrap_and_fold_overflow_into_a_summary() {
        let titles: Vec<(String, &str)> = (0..40)
            .map(|index| (format!("Long series title {index}"), "#336699"))
            .collect();
        let refs: Vec<(&str, &str)> = titles
            .iter()
            .map(|(title, color)| (title.as_str(), *color))
            .collect();
        let mut chart = chart(&refs);
        chart
            .apply_options(r#"{"generalLegend":{"visible":true}}"#)
            .unwrap();
        let rows = chart
            .with_general_legend(0, |legend, _| {
                legend.entries.iter().map(|entry| entry.row).max().unwrap() + 1
            })
            .unwrap();
        assert!(rows <= MAX_ROWS);
        assert!(chart.general_legend_height(0) <= 300.0 * MAX_PANE_SHARE);
        let texts = legend_texts(&mut chart);
        assert!(
            texts
                .iter()
                .any(|text| text.starts_with('+') && text.ends_with(" more")),
            "{texts:?}"
        );
        let (x, y) = chart
            .with_general_legend(0, |legend, lane_top| {
                let summary = legend
                    .entries
                    .iter()
                    .find(|entry| entry.label.ends_with(" more"))
                    .unwrap();
                (summary.x + 2.0, legend.row_center(lane_top, summary.row))
            })
            .unwrap();
        assert_eq!(chart.general_legend_target_at_for_touch(x, y, true), None);
    }
}
