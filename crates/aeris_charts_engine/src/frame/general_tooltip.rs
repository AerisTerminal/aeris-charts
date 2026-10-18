//! General-pane hover feedback: the index-axis cursor line under the marks and the hover
//! tooltip above them. Both are ordered-frame primitives, so every executor paints them alike.

use super::*;
use crate::{AxisDimension, GeneralTooltipSnapshot};

/// Gap between the pointer and the tooltip box; the box flips to the other side at an edge.
const POINTER_OFFSET: f64 = 12.0;
const MIN_WIDTH: f64 = 156.0;
const PADDING_X: f64 = 10.0;
const HEADER_PADDING_TOP: f64 = 7.0;
const HEADER_PADDING_BOTTOM: f64 = 6.0;
const BODY_PADDING_TOP: f64 = 6.0;
const BODY_PADDING_BOTTOM: f64 = 7.0;
const ROW_GAP: f64 = 2.0;
const COLUMN_GAP: f64 = 12.0;
const DOT_DIAMETER: f64 = 8.0;
const DOT_GAP: f64 = 6.0;
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

fn mix(base: Color, over: Color, amount: f64) -> Color {
    let channel = |from: u8, to: u8| {
        (f64::from(from) + (f64::from(to) - f64::from(from)) * amount).round() as u8
    };
    Color::rgb(
        channel(base.r(), over.r()),
        channel(base.g(), over.g()),
        channel(base.b(), over.b()),
    )
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

impl ChartEngine {
    /// The index-axis cursor of an axis-mode general hover: one line across the plot at the
    /// active datum (band center for bars), styled by \`crosshair.vertLine\`.
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
            self.general_axis_hover_cursor(pane_index),
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

    fn general_tooltip_content(&self, pane_index: usize) -> Option<TooltipContent> {
        if let Some((series_id, row)) = self.general_hovered_key()
            && let Some(series) = self.general_series(series_id)
            && self.pane_index_for_id(series.pane_id()) == Some(pane_index)
        {
            let axis = self.general_hover_is_axis();
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
            return Some(TooltipContent { header, rows });
        }
        let (label, title, value, color) = self.general_layout_hover_tooltip(pane_index)?;
        Some(TooltipContent {
            header: label,
            rows: vec![TooltipRow {
                color: Some(opaque(color)),
                name: title,
                value: value.map_or_else(|| "No value".to_owned(), format_value),
            }],
        })
    }

    /// The general hover tooltip: the financial tooltip's popover box (tinted header with a
    /// divider, muted names, right-aligned values) with one color dot per row, placed beside
    /// the pointer and flipped to stay inside the pane.
    pub(super) fn build_general_tooltip_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        if !self.options.get().general_tooltip.visible
            || self.pane_uses_financial_time(pane_index)
            || self.crosshair_suppressed_by_interaction()
        {
            return;
        }
        let Some((pointer_x, pointer_y)) = self.clamped_crosshair() else {
            return;
        };
        if self.pane_at_y(pointer_y) != Some(pane_index) {
            return;
        }
        let Some(mut content) = self.general_tooltip_content(pane_index) else {
            return;
        };
        let pane = &self.panes[pane_index];
        let layout = &self.options.get().layout;
        let family = layout.font_family.as_str();
        let size = layout.font_size.max(1.0);
        let line = size + 4.0;
        let header_height = HEADER_PADDING_TOP + line + HEADER_PADDING_BOTTOM;
        let body_padding = BODY_PADDING_TOP + BODY_PADDING_BOTTOM;
        // Keep the box inside the pane: rows that do not fit collapse into a final count row.
        let fit = ((pane.height - 2.0 - header_height - 1.0 - body_padding + ROW_GAP)
            / (line + ROW_GAP))
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
                DOT_DIAMETER + DOT_GAP + name + if value > 0.0 { COLUMN_GAP + value } else { 0.0 }
            })
            .fold(0.0_f64, f64::max);
        let width =
            (measure(&content.header, HEADER_WEIGHT).max(rows_width) + PADDING_X * 2.0 + 2.0)
                .max(MIN_WIDTH)
                .ceil();
        let rows = content.rows.len() as f64;
        let body_height = body_padding + rows * line + (rows - 1.0).max(0.0) * ROW_GAP;
        let height = (2.0 + header_height + 1.0 + body_height).ceil();

        let pane_bottom = pane.top + pane.height;
        let mut left = pointer_x + POINTER_OFFSET;
        if left + width > self.pane_w {
            left = pointer_x - POINTER_OFFSET - width;
        }
        let left = left.clamp(0.0, (self.pane_w - width).max(0.0)).round();
        let mut top = pointer_y + POINTER_OFFSET;
        if top + height > pane_bottom {
            top = pointer_y - POINTER_OFFSET - height;
        }
        let top = top
            .clamp(pane.top, (pane_bottom - height).max(pane.top))
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
        let header_bottom = ((top + 1.0 + header_height) * vpr).round() as f32;
        let inner_radius = (radius - border).max(0.0);
        out.push(Prim::RoundRect {
            x: device.x + border,
            y: device.y + border,
            w: (device.w - border * 2.0).max(0.0),
            h: (header_bottom - device.y - border).max(0.0),
            radii: [inner_radius, inner_radius, 0.0, 0.0],
            fill: mix(popover.surface, popover.foreground, 0.04),
            border_width: 0.0,
            border_color: popover.surface,
        });
        out.push(Prim::Rect {
            rect: IRect {
                x: (device.x + border) as i32,
                y: header_bottom as i32,
                w: (device.w - border * 2.0).max(0.0) as i32,
                h: border as i32,
            },
            color: mix(popover.surface, popover.foreground, 0.10),
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
            top + 1.0 + HEADER_PADDING_TOP + line * 0.5,
            popover.foreground,
            TextAlign::Left,
            HEADER_WEIGHT,
        );
        let mut row_top = top + 1.0 + header_height + 1.0 + BODY_PADDING_TOP;
        for row in &content.rows {
            let center_y = row_top + line * 0.5;
            if let Some(color) = row.color {
                out.push(Prim::Circle {
                    cx: ((text_left + DOT_DIAMETER * 0.5) * hpr) as f32,
                    cy: (center_y * vpr) as f32,
                    radius: (DOT_DIAMETER * 0.5 * ratio) as f32,
                    fill: color,
                    stroke_width: 0.0,
                    stroke: color,
                });
            }
            text(
                out,
                &row.name,
                text_left + DOT_DIAMETER + DOT_GAP,
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
        let scale = if line {
            CategoryScaleType::Point
        } else {
            CategoryScaleType::Band
        };
        let mut chart = ChartEngine::new_with_initial_domain(
            640.0,
            400.0,
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

    fn overlay(chart: &mut ChartEngine) -> Vec<Prim> {
        let frame = chart.build_frame();
        let segments = chart.frame_pane_segments(0).unwrap();
        frame.panes[0].main[segments.trading_end..segments.overlay_end].to_vec()
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

        let tooltip = overlay(&mut chart);
        let labels = texts(&tooltip);
        for expected in ["B", "Revenue", "2", "Cost", "5"] {
            assert!(
                labels.iter().any(|text| text == expected),
                "{expected}: {labels:?}"
            );
        }
        for color in [REVENUE, COST] {
            assert!(tooltip.iter().any(
                |prim| matches!(prim, Prim::Circle { fill, radius, .. } if *fill == color && *radius == 4.0)
            ));
        }
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
        let (x, y, w, h) = tooltip_box(&overlay(&mut chart));
        assert!(f64::from(x + w) <= edge + chart.pane_left);
        assert!(f64::from(y + h) <= plot.y + plot.height);

        chart
            .apply_options(r#"{"generalTooltip":{"visible":false}}"#)
            .unwrap();
        let frame = chart.build_frame();
        let segments = chart.frame_pane_segments(0).unwrap();
        assert!(texts(&frame.panes[0].main[segments.trading_end..segments.overlay_end]).is_empty());
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
                2,
                "one active dot per line plus one tooltip dot"
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
        assert!(texts(&overlay(&mut target)).iter().any(|text| text == "C"));
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
