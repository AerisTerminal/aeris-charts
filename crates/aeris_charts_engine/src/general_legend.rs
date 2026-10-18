//! Engine-drawn legend of general panes. Its entries are ordered-frame primitives below the
//! plot, so every executor paints the same legend, and the engine input controller toggles
//! series visibility from it without host wiring.

use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{Prim, TextAlign};

use crate::frame::{SERIES_MARKER_WIDTH, series_marker_prim};
use crate::{ChartEngine, ChartInputEvent, GeneralLayoutSeriesId, GeneralSeriesId};

const MARKER_HEIGHT: f64 = 10.0;
const MARKER_GAP: f64 = 6.0;
const ENTRY_GAP: f64 = 16.0;
const ROW_GAP: f64 = 2.0;
const LANE_PAD_TOP: f64 = 6.0;
const LANE_PAD_BOTTOM: f64 = 4.0;
const SIDE_MARGIN: f64 = 8.0;
const MAX_ROWS: usize = 3;
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

/// One pane's measured legend. Rows are relative to the lane top, which sits at the pane bottom.
#[derive(Clone, Debug, PartialEq)]
struct PaneLegend {
    height: f64,
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

impl ChartEngine {
    fn general_legend_key(&self) -> LegendKey {
        LegendKey {
            scene: self.frame_invalidation.scene_generation(),
            layout: self.frame_invalidation.layout_generation(),
            pane_w: self.pane_w.to_bits(),
            css_height: self.css_height.to_bits(),
            font_size: self.axis_metrics().axis.to_bits(),
            measured: self.has_text_measure(),
        }
    }

    fn with_general_legend<R>(
        &self,
        pane_index: usize,
        read: impl FnOnce(&PaneLegend, f64) -> R,
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
        let pane = self.panes.get(pane_index)?;
        Some(read(legend, pane.top + pane.height - legend.height))
    }

    /// Height the legend reserves at the bottom of a general pane, or zero without one.
    pub(crate) fn general_legend_height(&self, pane_index: usize) -> f64 {
        self.with_general_legend(pane_index, |legend, _| legend.height)
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
        let max_rows = ((pane.height * MAX_PANE_SHARE - LANE_PAD_TOP - LANE_PAD_BOTTOM + ROW_GAP)
            / (row_height + ROW_GAP))
            .floor()
            .clamp(0.0, MAX_ROWS as f64) as usize;
        if max_rows == 0 {
            return None;
        }
        let available = (self.pane_w - 2.0 * SIDE_MARGIN).max(0.0);
        let entry_width = |label: &str| SERIES_MARKER_WIDTH + MARKER_GAP + measure(label);

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
            let start = ((self.pane_w - width) * 0.5).max(SIDE_MARGIN).round();
            entries.extend(row.into_iter().map(|entry| LegendEntry {
                x: start + entry.x,
                ..entry
            }));
        }
        let row_count = entries.iter().map(|entry| entry.row + 1).max().unwrap_or(1) as f64;
        Some(PaneLegend {
            height: (LANE_PAD_TOP
                + row_count * row_height
                + (row_count - 1.0) * ROW_GAP
                + LANE_PAD_BOTTOM)
                .ceil(),
            row_height,
            entries,
        })
    }

    /// Paint one pane's legend into its ordered frame: a series-color pill and a label per entry,
    /// dimmed while the series is hidden.
    pub(crate) fn build_general_legend_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
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
                            entry.x,
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
                    x: (text_x * hpr) as f32,
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
        let pane = self.pane_at_y(y)?;
        self.with_general_legend(pane, |legend, lane_top| {
            legend.entries.iter().find_map(|entry| {
                let center_y = legend.row_center(lane_top, entry.row);
                let inside = x >= entry.x - HIT_SLACK
                    && x <= entry.x + entry.width + HIT_SLACK
                    && (y - center_y).abs() <= (legend.row_height + ROW_GAP) * 0.5;
                inside.then_some(entry.target).flatten()
            })
        })
        .flatten()
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
        GeneralSeriesOptions, GeneralXyInput, HorizontalDomain, PointerInput,
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
    }
}
