//! General-pane tooltip feedback: the index-axis cursor line under the marks and the tooltip
//! popover above every chart layer. Both are ordered-frame primitives, so every executor
//! paints them alike.
//!
//! Pointer hover wins over keyboard focus, like Recharts `combineTooltipInteractionState`:
//! a focused datum shows its tooltip only while the pointer activates nothing.

use super::*;
use crate::chart_input::FocusTooltipKey;
use crate::general_axes::general_kind_uses_axis_hover;
use crate::{AxisDimension, GeneralLayoutHit, GeneralSeriesKind, GeneralTooltipSnapshot};

/// Recharts `Tooltip` default `offset`: the gap between the anchor and the box. The box flips
/// to the other side of the anchor when it would cross the plot end.
const ANCHOR_OFFSET: f64 = 10.0;
const MIN_WIDTH: f64 = 156.0;
const PADDING_X: f64 = 10.0;
const PADDING_Y: f64 = 7.0;
const HEADER_GAP: f64 = 4.0;
const ROW_GAP: f64 = 2.0;
const COLUMN_GAP: f64 = 12.0;
const MARKER_WIDTH: f64 = SERIES_MARKER_WIDTH;
const MARKER_HEIGHT: f64 = 10.0;
const MARKER_GAP: f64 = 6.0;
const HEADER_WEIGHT: u16 = 590;

struct TooltipRow {
    color: Option<Color>,
    name: String,
    value: String,
}

struct TooltipContent {
    header: String,
    rows: Vec<TooltipRow>,
}

/// One tooltip to paint: its pane, its anchor (pane-local X, chart Y), and its rows.
struct TooltipSource {
    pane_index: usize,
    anchor: (f64, f64),
    content: TooltipContent,
}

fn opaque(color: Color) -> Color {
    Color::rgb(color.r(), color.g(), color.b())
}

fn format_value(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    value.to_string()
}

/// The value text of one tooltip row; the browser `create_general_tooltip` default formats the
/// same fields in the same order.
fn tooltip_detail(item: &GeneralTooltipSnapshot) -> String {
    let optional = |value: Option<f64>| value.map_or_else(|| "—".to_owned(), format_value);
    let mut parts = Vec::new();
    if let Some(value) = item.value {
        parts.push(format_value(value));
    }
    if item.low.is_some() || item.high.is_some() {
        parts.push(format!("{}–{}", optional(item.low), optional(item.high)));
    }
    if item.q1.is_some() || item.q3.is_some() {
        parts.push(format!(
            "Q1 {} · Q3 {}",
            optional(item.q1),
            optional(item.q3)
        ));
    }
    if let Some(size) = item.size {
        parts.push(format!("size {}", format_value(size)));
    }
    if let Some(label) = &item.y_label {
        parts.push(label.clone());
    }
    if parts.is_empty() {
        return item.label.clone().unwrap_or_else(|| "No value".to_owned());
    }
    parts.join(" · ")
}

/// Recharts `getTooltipTranslateByAxis` without `allowEscapeViewBox`: place the box after the
/// anchor, flip it before the anchor when it would cross the plot end, and never start before
/// the plot start. The box may then cover axis strips, which paint below it. Unlike Recharts'
/// HTML portal, a canvas cannot paint past the chart, so the box finally stays on the surface.
fn translate(anchor: f64, size: f64, start: f64, end: f64, surface: f64) -> f64 {
    let positive = anchor + ANCHOR_OFFSET;
    let placed = if positive + size > end {
        (anchor - ANCHOR_OFFSET - size).max(start)
    } else {
        positive.max(start)
    };
    placed.min(surface - size).max(0.0)
}

impl ChartEngine {
    /// The index-axis cursor of an axis-mode general hover or keyboard focus: one line across
    /// the plot at the active datum (band center for bars), styled by `crosshair.vertLine`.
    pub(super) fn build_general_axis_cursor_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        if self.crosshair_mode == CrosshairMode::Hidden
            || self.crosshair_suppressed_by_interaction()
        {
            return;
        }
        let line = &self.options.get().crosshair.vert_line;
        if !line.visible {
            return;
        }
        let (Some((dimension, coordinate)), Some(plot)) = (
            self.general_axis_hover_cursor(pane_index)
                .or_else(|| self.general_focus_cursor(pane_index)),
            self.general_plot_rect(pane_index),
        ) else {
            return;
        };
        let color = css_color(&line.color, CROSSHAIR_COLOR);
        let style = crate::line_style_from_u8(line.style);
        match dimension {
            AxisDimension::Y => out.push(Prim::HLine {
                y: (coordinate * vpr).round() as i32,
                x0: 0,
                x1: (plot.width * hpr).round() as i32,
                width: 1f64.max((line.width * vpr).floor()) as i32,
                style,
                color,
            }),
            _ => out.push(Prim::VLine {
                x: (coordinate * hpr).round() as i32,
                y0: (plot.y * vpr).round() as i32,
                y1: ((plot.y + plot.height) * vpr).round() as i32,
                width: 1f64.max((line.width * hpr).floor()) as i32,
                style,
                color,
            }),
        }
    }

    /// Whether the pointer activates a general datum or layout item; it then owns the tooltip.
    fn general_pointer_hover_active(&self) -> bool {
        self.general_hovered_key().is_some() || self.general_layout_hovered_hit().is_some()
    }

    /// The keyboard-focused datum's undismissed tooltip key, while the pointer activates nothing.
    fn general_focus_key(&self) -> Option<FocusTooltipKey> {
        if self.general_pointer_hover_active() {
            return None;
        }
        self.focus_tooltip_key()
            .filter(|key| !self.focus_tooltip_dismissed(*key))
    }

    /// The cursor of a keyboard-focused datum in an axis-mode pane, like the Recharts keyboard
    /// cursor on the active tick.
    fn general_focus_cursor(&self, pane_index: usize) -> Option<(AxisDimension, f64)> {
        let FocusTooltipKey::General(series_id, row) = self.general_focus_key()? else {
            return None;
        };
        self.general_axis_hover_dimension(pane_index)?;
        if !general_kind_uses_axis_hover(self.general_series(series_id)?.kind()) {
            return None;
        }
        self.general_row_index_coordinate(pane_index, series_id, row)
    }

    fn general_tooltip_header(&self, series: crate::GeneralSeriesId, row: usize) -> Option<String> {
        let dataset = self.general_dataset(self.general_series(series)?.dataset())?;
        let seconds = dataset.temporal_x_epoch_ms()?.get(row)?.div_euclid(1_000);
        let date = format_date_pattern_with_time_zone(
            seconds,
            "dd MMM yyyy",
            &self.month_names,
            self.time_zone,
        );
        let time = format_tick_label_with_time_zone(
            seconds,
            TickMarkType::Time,
            &self.month_names,
            self.time_zone,
        );
        Some(if time == "00:00" {
            date
        } else {
            format!("{date}   {time}")
        })
    }

    /// One general datum's rows: every series sharing its index datum when `axis`, else the
    /// datum alone.
    fn general_series_tooltip_content(
        &self,
        series_id: crate::GeneralSeriesId,
        row: usize,
        axis: bool,
    ) -> Option<TooltipContent> {
        let items = if axis {
            self.general_shared_tooltip_snapshot(series_id, row)?.items
        } else {
            vec![self.general_tooltip_snapshot(series_id, row)?]
        };
        let first = items.first()?;
        let header = self
            .general_tooltip_header(series_id, row)
            .or_else(|| (!axis).then(|| first.label.clone()).flatten())
            .unwrap_or_else(|| first.x_label.clone());
        let rows = items
            .iter()
            .map(|item| TooltipRow {
                color: Some(item.color),
                name: item.title.clone(),
                value: tooltip_detail(item),
            })
            .collect();
        Some(TooltipContent { header, rows })
    }

    fn general_layout_tooltip_content(
        &self,
        pane_index: usize,
        hit: GeneralLayoutHit,
    ) -> Option<TooltipContent> {
        let (label, title, value, color) = self.general_layout_item_tooltip(pane_index, hit)?;
        Some(TooltipContent {
            header: label,
            rows: vec![TooltipRow {
                color: Some(opaque(color)),
                name: title,
                value: value.map_or_else(|| "No value".to_owned(), format_value),
            }],
        })
    }

    fn general_pointer_tooltip(&self) -> Option<TooltipSource> {
        let (x, y) = self.clamped_crosshair()?;
        let pane_index = self.pane_at_y(y)?;
        if self.pane_uses_financial_time(pane_index) {
            return None;
        }
        let content = if let Some((series_id, row)) = self.general_hovered_key()
            && let Some(series) = self.general_series(series_id)
            && self.pane_index_for_id(series.pane_id()) == Some(pane_index)
        {
            self.general_series_tooltip_content(series_id, row, self.general_hover_is_axis())?
        } else {
            self.general_layout_tooltip_content(pane_index, self.general_layout_hovered_hit()?)?
        };
        Some(TooltipSource {
            pane_index,
            anchor: (x, y),
            content,
        })
    }

    /// The keyboard tooltip. Axis-mode panes anchor on the active tick across the middle of the
    /// plot (Recharts `combineCoordinateForDefaultIndex`); item panes anchor on the mark.
    fn general_focus_tooltip(&self) -> Option<TooltipSource> {
        match self.general_focus_key()? {
            FocusTooltipKey::General(series_id, row) => {
                let series = self.general_series(series_id)?;
                if !series.visible() {
                    return None;
                }
                let pane_index = self.pane_index_for_id(series.pane_id())?;
                if self.pane_uses_financial_time(pane_index) {
                    return None;
                }
                let plot = self.general_plot_rect(pane_index)?;
                let middle = (plot.width * 0.5, plot.y + plot.height * 0.5);
                let axis = self.general_axis_hover_dimension(pane_index).is_some()
                    && general_kind_uses_axis_hover(series.kind());
                let anchor = if axis {
                    match self.general_row_index_coordinate(pane_index, series_id, row) {
                        Some((AxisDimension::Y, y)) => Some((middle.0, y)),
                        Some((_, x)) => Some((x, middle.1)),
                        None => None,
                    }
                } else {
                    self.general_mark_anchor(series, row)
                };
                Some(TooltipSource {
                    pane_index,
                    anchor: anchor.unwrap_or(middle),
                    content: self.general_series_tooltip_content(series_id, row, axis)?,
                })
            }
            FocusTooltipKey::Layout(hit) => {
                let (pane_index, anchor) = (0..self.panes.len()).find_map(|pane_index| {
                    Some((pane_index, self.general_layout_hit_anchor(pane_index, hit)?))
                })?;
                Some(TooltipSource {
                    pane_index,
                    anchor,
                    content: self.general_layout_tooltip_content(pane_index, hit)?,
                })
            }
        }
    }

    /// The painted center of one row's mark (pane-local X, chart Y), where an item tooltip
    /// anchors like the Recharts per-graphic `tooltipPosition`.
    fn general_mark_anchor(&self, series: &crate::GeneralSeries, row: usize) -> Option<(f64, f64)> {
        let mut found = None;
        let rect = |left: f64, right: f64, top: f64, bottom: f64| {
            ((left + right) * 0.5, (top + bottom) * 0.5)
        };
        let sector = |sector: aeris_charts_core::scale::general_scale::PolarSector| {
            sector.plot().point(sector.mid_angle(), sector.mid_radius())
        };
        match series.kind() {
            GeneralSeriesKind::Column => self.visit_general_columns(series, |bar| {
                if bar.row == row {
                    found.get_or_insert(rect(bar.left, bar.right, bar.top, bar.bottom));
                }
            }),
            GeneralSeriesKind::Histogram => self.visit_general_histogram_bins(series, |bar| {
                if bar.row == row {
                    found.get_or_insert(rect(bar.left, bar.right, bar.top, bar.bottom));
                }
            }),
            GeneralSeriesKind::HorizontalBar => {
                self.visit_general_horizontal_bars(series, |bar| {
                    if bar.row == row {
                        found.get_or_insert(rect(bar.left, bar.right, bar.top, bar.bottom));
                    }
                });
            }
            GeneralSeriesKind::RangeBar => self.visit_general_range_bars(series, |bar| {
                if bar.row == row {
                    found.get_or_insert(rect(bar.left, bar.right, bar.top, bar.bottom));
                }
            }),
            GeneralSeriesKind::XyLine | GeneralSeriesKind::XyArea => {
                self.visit_general_stacked_area_points(series, |point| {
                    if point.row == row {
                        found.get_or_insert((point.x, point.high_y));
                    }
                });
                if found.is_none() {
                    self.visit_general_path_points(series, |point| {
                        if point.row == row {
                            found.get_or_insert((point.x, point.y));
                        }
                    });
                }
            }
            GeneralSeriesKind::RangeArea => self.visit_general_range_points(series, |point| {
                if point.row == row {
                    found.get_or_insert((point.x, point.high_y));
                }
            }),
            GeneralSeriesKind::ErrorBar => self.visit_general_error_bars(series, |bar| {
                if bar.row == row {
                    found.get_or_insert((bar.x, bar.y));
                }
            }),
            GeneralSeriesKind::BoxPlot => self.visit_general_box_plots(series, |plot| {
                if plot.row == row {
                    found.get_or_insert((plot.center_x, plot.median_y));
                }
            }),
            GeneralSeriesKind::HeatmapGrid => self.visit_general_heatmap_cells(series, |cell| {
                if cell.row == row {
                    found.get_or_insert(rect(cell.left, cell.right, cell.top, cell.bottom));
                }
            }),
            GeneralSeriesKind::Scatter | GeneralSeriesKind::Bubble => {
                self.visit_general_scatter_points(series, |point| {
                    if point.row == row {
                        found.get_or_insert((point.x, point.y));
                    }
                });
            }
            GeneralSeriesKind::Pie => self.visit_general_pie_slices(series, |slice| {
                if slice.row == row && found.is_none() {
                    found = sector(slice.sector);
                }
            }),
            GeneralSeriesKind::PolarArea | GeneralSeriesKind::RadialBar => {
                self.visit_general_polar_value_sectors(series, |slice| {
                    if slice.row == row && found.is_none() {
                        found = sector(slice.sector);
                    }
                });
            }
            GeneralSeriesKind::Radar => {
                self.with_general_radar_polygon(series, |polygon, rows, _| {
                    found = rows
                        .iter()
                        .position(|vertex_row| *vertex_row == Some(row))
                        .and_then(|vertex| polygon.vertex(vertex));
                });
            }
        }
        found
    }

    /// The general tooltip: one single-tone popover box (bold heading, muted names,
    /// right-aligned values) with one color bar per row. It paints last in the unscissored
    /// axis layer, in chart device pixels, so axis labels and outside legends stay below it.
    pub(crate) fn build_general_tooltip_axis(&self, dpr: f64, out: &mut Vec<Prim>) {
        if !self.options.get().general_tooltip.visible || self.crosshair_suppressed_by_interaction()
        {
            return;
        }
        let Some(TooltipSource {
            pane_index,
            anchor,
            mut content,
        }) = self
            .general_pointer_tooltip()
            .or_else(|| self.general_focus_tooltip())
        else {
            return;
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return;
        };
        let (hpr, vpr) = (dpr, dpr);
        let layout = &self.options.get().layout;
        let family = layout.font_family.as_str();
        let size = layout.font_size.max(1.0);
        let line = size + 4.0;
        let chrome = 2.0 + PADDING_Y * 2.0 + line + HEADER_GAP;
        // Keep the box on the chart: rows that do not fit collapse into a final count row.
        let fit = ((self.css_height - chrome + ROW_GAP) / (line + ROW_GAP))
            .floor()
            .max(1.0) as usize;
        if content.rows.len() > fit {
            let hidden = content.rows.len() + 1 - fit;
            content.rows.truncate(fit - 1);
            content.rows.push(TooltipRow {
                color: None,
                name: format!("+{hidden} more"),
                value: String::new(),
            });
        }
        let measure = |text: &str, weight: u16| {
            if text.is_empty() {
                0.0
            } else {
                self.measure_text_run(text, size, family, weight, false)
            }
        };
        let rows_width = content
            .rows
            .iter()
            .map(|row| {
                let name = measure(&row.name, 400);
                let value = measure(&row.value, 400);
                MARKER_WIDTH
                    + MARKER_GAP
                    + name
                    + if value > 0.0 { COLUMN_GAP + value } else { 0.0 }
            })
            .fold(0.0_f64, f64::max);
        let width =
            (measure(&content.header, HEADER_WEIGHT).max(rows_width) + PADDING_X * 2.0 + 2.0)
                .max(MIN_WIDTH)
                .ceil();
        let rows = content.rows.len() as f64;
        let height = (chrome + rows * line + (rows - 1.0).max(0.0) * ROW_GAP).ceil();

        // Plot X is pane-local; the axis layer is chart space.
        let left = translate(
            self.pane_left + anchor.0,
            width,
            self.pane_left,
            self.pane_left + plot.width,
            self.css_width,
        )
        .round();
        let top = translate(
            anchor.1,
            height,
            plot.y,
            plot.y + plot.height,
            self.css_height,
        )
        .round();

        let popover = self.popover_tokens();
        let device = DeviceBox::snap(left, top, width, height, hpr, vpr);
        let ratio = hpr.min(vpr);
        let radius = (aeris_charts_core::style::RADIUS_DEFAULT * ratio).round() as f32;
        let border = aeris_charts_core::style::border_width_device_px(ratio) as f32;
        out.push(Prim::RoundRect {
            x: device.x,
            y: device.y,
            w: device.w,
            h: device.h,
            radii: [radius; 4],
            fill: popover.surface,
            border_width: border,
            border_color: popover.border,
        });
        let text_left = left + 1.0 + PADDING_X;
        let text_right = left + width - 1.0 - PADDING_X;
        let text =
            |out: &mut Vec<Prim>, text: &str, x: f64, center_y: f64, color, align, weight| {
                out.push(Prim::Text {
                    x: (x * hpr) as f32,
                    y: (center_y * vpr) as f32,
                    text: text.to_owned(),
                    color,
                    size: (size * vpr) as f32,
                    family: family.into(),
                    align,
                    weight,
                    italic: false,
                });
            };
        text(
            out,
            &content.header,
            text_left,
            top + 1.0 + PADDING_Y + line * 0.5,
            popover.foreground,
            TextAlign::Left,
            HEADER_WEIGHT,
        );
        let mut row_top = top + 1.0 + PADDING_Y + line + HEADER_GAP;
        for row in &content.rows {
            let center_y = row_top + line * 0.5;
            if let Some(color) = row.color {
                out.push(series_marker_prim(
                    text_left,
                    center_y,
                    MARKER_HEIGHT,
                    color,
                    hpr,
                    vpr,
                ));
            }
            text(
                out,
                &row.name,
                text_left + MARKER_WIDTH + MARKER_GAP,
                center_y,
                popover.muted,
                TextAlign::Left,
                400,
            );
            if !row.value.is_empty() {
                text(
                    out,
                    &row.value,
                    text_right,
                    center_y,
                    popover.foreground,
                    TextAlign::Right,
                    400,
                );
            }
            row_top += line + ROW_GAP;
        }
        out.push(popover.highlight_prim((device.x, device.y, device.w, device.h), radius, border));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CategoryScaleType, ChartSyncEventKind, GeneralAxisDomain, GeneralAxisOptions,
        GeneralScaleType, GeneralSeriesOptions, GeneralXyInput, HorizontalDomain, PointerInput,
    };

    const REVENUE: Color = Color::rgb(0x33, 0x66, 0x99);
    const COST: Color = Color::rgb(0xcc, 0x66, 0x33);

    fn at(x: f64, y: f64) -> PointerInput {
        PointerInput {
            x,
            y,
            ..PointerInput::default()
        }
    }

    fn category_chart(line: bool) -> ChartEngine {
        category_chart_sized(line, 400.0)
    }

    fn category_chart_sized(line: bool, height: f64) -> ChartEngine {
        let scale = if line {
            CategoryScaleType::Point
        } else {
            CategoryScaleType::Band
        };
        let mut chart = ChartEngine::new_with_initial_domain(
            640.0,
            height,
            1.0,
            HorizontalDomain::Category { scale },
        )
        .unwrap();
        let x_scale = if line {
            GeneralScaleType::Point
        } else {
            GeneralScaleType::Band
        };
        chart
            .add_general_axis(GeneralAxisOptions::new("x", 0, AxisDimension::X, x_scale))
            .unwrap();
        let mut y = GeneralAxisOptions::new("y", 0, AxisDimension::Y, GeneralScaleType::Linear);
        y.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        chart.add_general_axis(y).unwrap();
        for (title, color, values) in [
            ("Revenue", "#336699", [1.0, 2.0, 3.0, 4.0]),
            ("Cost", "#cc6633", [4.0, 5.0, 6.0, 7.0]),
        ] {
            let dataset = chart
                .create_general_xy_dataset(GeneralXyInput::Category {
                    ids: None,
                    categories: ["A", "B", "C", "D"].map(str::to_owned).to_vec(),
                    category_indices: vec![0, 1, 2, 3],
                    y: values.to_vec(),
                    y_valid: None,
                })
                .unwrap();
            let mut options = if line {
                GeneralSeriesOptions::xy_line(0, dataset, "x", "y")
            } else {
                GeneralSeriesOptions::column(0, dataset, "x", "y")
            };
            options.title = title.into();
            options.color = Some(color.into());
            chart.add_general_series(options).unwrap();
        }
        chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
        chart
    }

    /// The live axis layer, whose tail is the general tooltip.
    fn axis_layer(chart: &mut ChartEngine) -> Vec<Prim> {
        chart.build_frame();
        let axis = chart.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
        let mut prims = Vec::new();
        chart.build_axis_primitives_into(&axis, &mut prims);
        prims
    }

    fn tooltip_start(prims: &[Prim]) -> Option<usize> {
        prims.iter().position(|prim| {
            matches!(prim, Prim::RoundRect { w, radii, .. }
                if radii[3] > 0.0 && f64::from(*w) >= MIN_WIDTH)
        })
    }

    /// The tooltip primitives, or nothing when no tooltip paints.
    fn tooltip_prims(chart: &mut ChartEngine) -> Vec<Prim> {
        let prims = axis_layer(chart);
        tooltip_start(&prims).map_or_else(Vec::new, |start| prims[start..].to_vec())
    }

    fn texts(prims: &[Prim]) -> Vec<String> {
        prims
            .iter()
            .filter_map(|prim| match prim {
                Prim::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn tooltip_box(prims: &[Prim]) -> (f32, f32, f32, f32) {
        prims
            .iter()
            .find_map(|prim| match prim {
                Prim::RoundRect {
                    x, y, w, h, radii, ..
                } if radii[3] > 0.0 => Some((*x, *y, *w, *h)),
                _ => None,
            })
            .expect("a tooltip box")
    }

    #[test]
    fn column_hover_follows_the_band_with_cursor_and_shared_tooltip() {
        let mut chart = category_chart(false);
        let plot = chart.general_plot_rect(0).unwrap();
        // Empty plot space above the B bars still hovers B, like a Recharts axis tooltip.
        let pointer = (plot.width * 0.375, plot.y + 4.0);
        chart.input_pointer_move(at(pointer.0, pointer.1), false);
        let hit = chart.general_hovered_hit().unwrap();
        assert_eq!(hit.row, 1);
        let (dimension, cursor) = chart.general_axis_hover_cursor(0).unwrap();
        assert_eq!(dimension, AxisDimension::X);
        assert!((cursor - pointer.0).abs() < plot.width / 8.0);

        let frame = chart.build_frame();
        let pane = &frame.panes[0];
        let left = (chart.pane_left).round() as i32;
        let [start, _] = pane.plot_range;
        assert!(
            matches!(pane.main[start], Prim::VLine { x, y0, y1, .. }
                if x == cursor.round() as i32 + left
                    && y0 == plot.y.round() as i32
                    && y1 == (plot.y + plot.height).round() as i32),
            "the cursor is one vertical line under the marks at the band center"
        );
        assert!(
            !pane
                .main
                .iter()
                .any(|prim| matches!(prim, Prim::HLine { .. } | Prim::RectFrame { .. })),
            "axis hover draws no horizontal line and no hover ring"
        );

        let tooltip = tooltip_prims(&mut chart);
        let labels = texts(&tooltip);
        for expected in ["B", "Revenue", "2", "Cost", "5"] {
            assert!(
                labels.iter().any(|text| text == expected),
                "{expected}: {labels:?}"
            );
        }
        for color in [REVENUE, COST] {
            assert!(
                tooltip.iter().any(
                    |prim| matches!(prim, Prim::RoundRect { w, h, radii, fill, .. }
                    if *fill == color && *w == 4.0 && *h == 10.0 && radii == &[2.0; 4])
                ),
                "each row carries a vertical rounded color pill"
            );
        }
        assert!(
            !tooltip
                .iter()
                .any(|prim| matches!(prim, Prim::RoundRect { radii, .. }
                if radii[0] > 0.0 && radii[3] == 0.0)),
            "a single-tone box: no tinted header band"
        );
        let (x, y, w, h) = tooltip_box(&tooltip);
        assert!(
            f64::from(x) > pointer.0 + chart.pane_left,
            "the box sits right of the pointer"
        );
        assert!(f64::from(y) > pointer.1 && f64::from(h) > 0.0);
        assert!(f64::from(w) >= MIN_WIDTH);

        // At the right edge the box flips left of the pointer and stays inside the pane.
        let edge = plot.width - 2.0;
        chart.input_pointer_move(at(edge, plot.y + plot.height - 4.0), false);
        assert_eq!(chart.general_hovered_hit().unwrap().row, 3);
        let (x, y, w, h) = tooltip_box(&tooltip_prims(&mut chart));
        assert!(f64::from(x + w) <= edge + chart.pane_left);
        assert!(f64::from(y + h) <= plot.y + plot.height);

        chart
            .apply_options(r#"{"generalTooltip":{"visible":false}}"#)
            .unwrap();
        assert!(tooltip_prims(&mut chart).is_empty());
        let frame = chart.build_frame();
        assert!(
            matches!(
                frame.panes[0].main[frame.panes[0].plot_range[0]],
                Prim::VLine { .. }
            ),
            "turning the tooltip off keeps the cursor"
        );

        chart.input_pointer_leave();
        assert!(chart.general_hovered_hit().is_none());
        assert!(
            !chart.build_frame().panes[0]
                .main
                .iter()
                .any(|prim| matches!(prim, Prim::VLine { .. }))
        );
    }

    #[test]
    fn tooltip_flips_inside_the_plot_when_it_fits() {
        let mut chart = category_chart(true);
        let plot = chart.general_plot_rect(0).unwrap();
        let mut pointer_y = plot.y + 2.0;
        while pointer_y < plot.y + plot.height - 1.0 {
            chart.input_pointer_move(at(plot.width * 0.375, pointer_y), false);
            let (_, y, _, h) = tooltip_box(&tooltip_prims(&mut chart));
            assert!(
                f64::from(y) >= plot.y - 0.5 && f64::from(y + h) <= plot.y + plot.height + 0.5,
                "pointer y {pointer_y}: box {y}..{} leaves plot {}..{}",
                y + h,
                plot.y,
                plot.y + plot.height
            );
            pointer_y += 3.0;
        }
    }

    #[test]
    fn tooltip_paints_above_axis_labels_when_it_covers_the_axis_strip() {
        // A plot shorter than the box: Recharts keeps the box at the plot top and lets it run
        // over the X axis below, so the axis must paint under it.
        let mut chart = category_chart_sized(true, 90.0);
        let plot = chart.general_plot_rect(0).unwrap();
        chart.input_pointer_move(at(plot.width * 0.375, plot.y + plot.height - 2.0), false);
        let frame = chart.build_frame();
        let segments = chart.frame_pane_segments(0).unwrap();
        assert!(
            tooltip_start(&frame.panes[0].main[segments.trading_end..segments.overlay_end])
                .is_none(),
            "no pane layer paints the tooltip"
        );
        let layer = axis_layer(&mut chart);
        let start = tooltip_start(&layer).expect("the tooltip paints in the axis layer");
        let last_tick = layer
            .iter()
            .rposition(|prim| matches!(prim, Prim::Text { text, .. } if text == "D"))
            .expect("the X-axis tick label D");
        assert!(
            last_tick < start,
            "axis tick labels paint before the tooltip"
        );
        let (_, y, _, h) = tooltip_box(&layer[start..]);
        assert!(
            (f64::from(y) - plot.y).abs() <= 0.5,
            "the box starts at the plot top"
        );
        assert!(
            f64::from(y + h) > plot.y + plot.height,
            "the box {y}+{h} reaches into the axis strip below plot {plot:?}"
        );
        assert!(f64::from(y + h) <= chart.css_height + 0.5);
    }

    #[test]
    fn crosshair_free_exports_omit_the_tooltip() {
        let mut chart = category_chart(false);
        let plot = chart.general_plot_rect(0).unwrap();
        chart.input_pointer_move(at(plot.width * 0.375, plot.y + 4.0), false);
        for include_crosshair in [false, true] {
            let export = chart.capture_export_frame(
                crate::ExportFrameRequest {
                    width: 640.0,
                    height: 400.0,
                    dpr: 2.0,
                    include_crosshair,
                },
                |text, _| text.len() as f64 * 7.0,
                |_, _| 0.0,
            );
            assert_eq!(
                tooltip_start(&export.axis_primitives).is_some(),
                include_crosshair
            );
        }
    }

    #[test]
    fn keyboard_focus_shows_the_tooltip_until_pointer_hover_escape_or_blur() {
        let mut chart = category_chart(false);
        let plot = chart.general_plot_rect(0).unwrap();
        let revenue = chart.general_series_iter().next().unwrap().id();
        assert!(tooltip_prims(&mut chart).is_empty());
        assert!(chart.set_general_accessibility_focus(revenue, 1));
        let focused = tooltip_prims(&mut chart);
        let labels = texts(&focused);
        for expected in ["B", "Revenue", "2", "Cost", "5"] {
            assert!(
                labels.iter().any(|text| text == expected),
                "{expected}: {labels:?}"
            );
        }
        // Recharts anchors a keyboard tooltip on the active tick, centered across the plot.
        let (_, cursor) = chart.general_row_index_coordinate(0, revenue, 1).unwrap();
        let (x, y, _, _) = tooltip_box(&focused);
        assert!((f64::from(x) - (chart.pane_left + cursor + ANCHOR_OFFSET)).abs() <= 1.0);
        assert!((f64::from(y) - (plot.y + plot.height * 0.5 + ANCHOR_OFFSET)).abs() <= 1.0);
        let left = chart.pane_left.round() as i32;
        let frame = chart.build_frame();
        assert!(
            matches!(frame.panes[0].main[frame.panes[0].plot_range[0]],
                Prim::VLine { x, .. } if x == cursor.round() as i32 + left),
            "the focused band shows the axis cursor"
        );

        // Pointer hover wins while it activates a datum.
        chart.input_pointer_move(at(plot.width * 0.875, plot.y + 4.0), false);
        let hovered = texts(&tooltip_prims(&mut chart));
        assert!(hovered.iter().any(|text| text == "D"));
        assert!(!hovered.iter().any(|text| text == "B"));
        chart.input_pointer_leave();
        assert!(
            texts(&tooltip_prims(&mut chart))
                .iter()
                .any(|text| text == "B")
        );

        // Escape dismisses the tooltip and cursor but keeps keyboard focus.
        chart.input_key_down(
            crate::ChartKey::Escape,
            crate::InputModifiers::default(),
            false,
            0.0,
        );
        assert!(tooltip_prims(&mut chart).is_empty());
        assert!(chart.general_accessibility_focused_hit().is_some());
        assert!(
            !chart.build_frame().panes[0]
                .main
                .iter()
                .any(|prim| matches!(prim, Prim::VLine { .. }))
        );
        assert!(chart.set_general_accessibility_focus(revenue, 2));
        assert!(
            texts(&tooltip_prims(&mut chart))
                .iter()
                .any(|text| text == "C")
        );

        // Blur clears focus and its tooltip; focusing again shows it even after an Escape.
        chart.input_key_down(
            crate::ChartKey::Escape,
            crate::InputModifiers::default(),
            false,
            0.0,
        );
        chart.clear_general_accessibility_focus();
        assert!(tooltip_prims(&mut chart).is_empty());
        assert!(chart.set_general_accessibility_focus(revenue, 2));
        assert!(
            texts(&tooltip_prims(&mut chart))
                .iter()
                .any(|text| text == "C")
        );
    }

    #[test]
    fn keyboard_focus_tooltip_anchors_on_item_marks_and_layout_items() {
        let mut chart = ChartEngine::new_with_initial_domain(
            640.0,
            400.0,
            1.0,
            HorizontalDomain::Continuous {
                scale: crate::ContinuousScaleType::Linear,
            },
        )
        .unwrap();
        for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
            let mut axis = GeneralAxisOptions::new(id, 0, dimension, GeneralScaleType::Linear);
            axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
            chart.add_general_axis(axis).unwrap();
        }
        let dataset = chart
            .create_general_xy_dataset(GeneralXyInput::Numeric {
                ids: None,
                x: vec![2.0, 6.0],
                y: vec![3.0, 7.0],
                y_valid: None,
            })
            .unwrap();
        let mut scatter = GeneralSeriesOptions::scatter(0, dataset, "x", "y");
        scatter.title = "Points".into();
        let series = chart.add_general_series(scatter).unwrap();
        chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
        let mut mark = None;
        chart.visit_general_scatter_points(chart.general_series(series).unwrap(), |point| {
            if point.row == 1 {
                mark = Some((point.x, point.y));
            }
        });
        let (mark_x, mark_y) = mark.unwrap();
        assert!(chart.set_general_accessibility_focus(series, 1));
        let prims = tooltip_prims(&mut chart);
        assert!(texts(&prims).iter().any(|text| text == "Points"));
        let (x, y, _, _) = tooltip_box(&prims);
        assert!((f64::from(x) - (chart.pane_left + mark_x + ANCHOR_OFFSET)).abs() <= 1.0);
        assert!((f64::from(y) - (mark_y + ANCHOR_OFFSET)).abs() <= 1.0);

        let mut layout =
            ChartEngine::new_with_initial_domain(400.0, 300.0, 1.0, HorizontalDomain::Layout)
                .unwrap();
        let dataset = layout
            .create_general_layout_dataset(crate::GeneralLayoutInput::Funnel(
                [("top", "Top", 10.0), ("bottom", "Bottom", 5.0)]
                    .map(|(id, label, value)| crate::FunnelStage {
                        id: id.into(),
                        label: label.into(),
                        value,
                    })
                    .to_vec(),
            ))
            .unwrap();
        let funnel = layout
            .add_general_layout_series(crate::GeneralLayoutSeriesOptions::new(
                0,
                dataset,
                crate::GeneralLayoutSeriesKind::Funnel,
            ))
            .unwrap();
        layout.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        let hit = GeneralLayoutHit {
            series: funnel,
            element: crate::GeneralLayoutElement::Stage,
            row: 1,
        };
        assert!(layout.set_general_layout_accessibility_focus(hit, "#2962ff", 14.0));
        let prims = tooltip_prims(&mut layout);
        assert!(texts(&prims).iter().any(|text| text == "Bottom"));
        let (anchor_x, anchor_y) = layout.general_layout_hit_anchor(0, hit).unwrap();
        let (x, y, _, _) = tooltip_box(&prims);
        assert!((f64::from(x) - (layout.pane_left + anchor_x + ANCHOR_OFFSET)).abs() <= 1.0);
        assert!((f64::from(y) - (anchor_y + ANCHOR_OFFSET)).abs() <= 1.0);
        layout.clear_general_layout_accessibility_focus();
        assert!(tooltip_prims(&mut layout).is_empty());
    }
    #[test]
    fn axis_hover_click_selects_only_the_mark_under_the_pointer() {
        let mut chart = category_chart(false);
        let plot = chart.general_plot_rect(0).unwrap();
        let x = plot.width * 0.375;
        chart.input_pointer_down(at(x, plot.y + 4.0), 1);
        chart.input_pointer_up(at(x, plot.y + 4.0));
        assert!(chart.general_hovered_hit().is_some());
        assert!(chart.general_selected_hit().is_none());
        chart.input_pointer_down(at(x, plot.y + plot.height - 2.0), 1);
        chart.input_pointer_up(at(x, plot.y + plot.height - 2.0));
        assert_eq!(chart.general_selected_hit().unwrap().row, 1);
    }

    #[test]
    fn line_hover_draws_active_dots_on_every_series_at_the_active_x() {
        let mut chart = category_chart(true);
        let plot = chart.general_plot_rect(0).unwrap();
        chart.input_pointer_move(at(plot.width * 0.7, plot.y + 4.0), false);
        assert_eq!(chart.general_hovered_hit().unwrap().row, 2);
        let main = chart.build_frame().panes[0].main.clone();
        for color in [REVENUE, COST] {
            assert_eq!(
                main.iter()
                    .filter(|prim| matches!(prim, Prim::Circle { fill, radius, .. }
                        if *fill == color && *radius == 4.0))
                    .count(),
                1,
                "one active dot per line; the tooltip marks rows with bars"
            );
        }
        assert_eq!(
            main.iter()
                .filter(|prim| matches!(prim, Prim::Circle { radius, .. } if *radius == 6.0))
                .count(),
            2,
            "each active dot sits in a background ring"
        );
    }

    #[test]
    fn linked_charts_share_the_axis_hover_and_clear_it_together() {
        let mut source = category_chart(true);
        let mut target = category_chart(true);
        let plot = source.general_plot_rect(0).unwrap();
        source.input_pointer_move(at(plot.width * 0.7, plot.y + 4.0), false);
        let events = source.take_sync_events();
        assert!(matches!(
            events.last().map(|event| &event.kind),
            Some(ChartSyncEventKind::GeneralCrosshair { .. })
        ));
        for event in &events {
            target.apply_external_sync_envelope(event);
        }
        assert_eq!(target.general_hovered_hit().unwrap().row, 2);
        assert!(target.general_axis_hover_cursor(0).is_some());
        assert!(
            texts(&tooltip_prims(&mut target))
                .iter()
                .any(|text| text == "C")
        );
        assert!(
            target.take_sync_events().is_empty(),
            "a linked hover does not echo"
        );

        source.input_pointer_leave();
        for event in &source.take_sync_events() {
            target.apply_external_sync_envelope(event);
        }
        assert!(target.general_hovered_hit().is_none());
    }
}
