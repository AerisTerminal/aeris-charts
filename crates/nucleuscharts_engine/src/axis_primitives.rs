//! Backend-neutral composition of the final unscissored chart axis/top layer.
//!
//! Axis label selection and geometry already live in [`crate::AxisFrame`]. This module owns the
//! remaining chart policy that every host must execute identically: watermark placement, axis
//! chrome, tick stubs, pane separators, boxed-label attachment, and text primitive placement.
//! Hosts contribute only the platform font metric used to reproduce Canvas2D's middle-baseline
//! correction.

use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::{IRect, Prim, TextAlign};

use crate::{AxisFrame, AxisLabel, AxisTextAlign, AxisTextMidpoint, ChartEngine, PANE_SEPARATOR};

impl ChartEngine {
    /// Build the final unscissored axis/top primitive layer into `output`, retaining its capacity.
    ///
    /// `midpoint_correction` returns `(ascent - descent) / 2` in logical pixels for the supplied
    /// text in the chart layout font. A host without vertical ink metrics may return `0.0`.
    pub fn build_axis_primitives_into<F>(
        &self,
        axis_frame: &AxisFrame,
        output: &mut Vec<Prim>,
        midpoint_correction: F,
    ) where
        F: Fn(&str) -> f64,
    {
        output.clear();
        let dpr = self.dpr;
        let bitmap_w = (self.css_width * dpr).round().max(1.0);
        let pane_left = self.pane_left;
        let pane_w = self.pane_w;
        let pane_h = self.pane_h;
        let options = self.options.get();
        let layout = options.layout;
        let left_scale = options.left_price_scale;
        let right_scale = options.right_price_scale;
        let time_scale = options.time_scale;
        let watermark = options.watermark;
        let border_w = 1f64.max(dpr.floor()) as i32;
        let parse = |css: &str, fallback: Color| Color::parse_css(css).unwrap_or(fallback);
        let fallback = Color::rgb(
            nucleuscharts_core::style::DEFAULT_BORDER_RGB.0,
            nucleuscharts_core::style::DEFAULT_BORDER_RGB.1,
            nucleuscharts_core::style::DEFAULT_BORDER_RGB.2,
        );
        let left_border = parse(&left_scale.border_color, fallback);
        let right_border = parse(&right_scale.border_color, fallback);
        let time_border = parse(&time_scale.border_color, fallback);

        // Watermark is below chrome and labels, matching the browser host's old overlay slot.
        if watermark.visible && !watermark.text.is_empty() {
            let default_text = nucleuscharts_core::style::DEFAULT_AXIS_TEXT_RGB;
            let (x, align) = match watermark.horz_align.as_str() {
                "left" => (pane_left, TextAlign::Left),
                "right" => (pane_left + pane_w, TextAlign::Right),
                _ => (pane_left + pane_w / 2.0, TextAlign::Center),
            };
            let y = match watermark.vert_align.as_str() {
                "top" => watermark.font_size / 2.0,
                "bottom" => pane_h - watermark.font_size / 2.0,
                _ => pane_h / 2.0,
            };
            output.push(Prim::Text {
                x: (x * dpr) as f32,
                y: (y * dpr) as f32,
                text: watermark.text,
                color: parse(
                    &watermark.color,
                    Color::rgb(default_text.0, default_text.1, default_text.2),
                ),
                size: (watermark.font_size * dpr) as f32,
                family: watermark.font_family,
                align,
                weight: if watermark.font_style.contains("bold") {
                    700
                } else {
                    400
                },
                italic: watermark.font_style.contains("italic"),
            });
        }

        {
            let mut rect = |x: f64, y: f64, w: f64, h: f64, color: Color| {
                let x0 = x.round() as i32;
                let y0 = y.round() as i32;
                let x1 = (x + w).round() as i32;
                let y1 = (y + h).round() as i32;
                if x1 > x0 && y1 > y0 {
                    output.push(Prim::Rect {
                        rect: IRect {
                            x: x0,
                            y: y0,
                            w: x1 - x0,
                            h: y1 - y0,
                        },
                        color,
                    });
                }
            };

            if self.left_axis_w > 0.0 && left_scale.border_visible {
                rect(
                    (pane_left * dpr).round() - f64::from(border_w),
                    0.0,
                    f64::from(border_w),
                    (pane_h * dpr).round(),
                    left_border,
                );
            }
            if self.axis_w > 0.0 && right_scale.border_visible {
                rect(
                    ((pane_left + pane_w) * dpr).round(),
                    0.0,
                    f64::from(border_w),
                    (pane_h * dpr).round(),
                    right_border,
                );
            }
            if time_scale.border_visible && self.time_axis_visible {
                rect(
                    0.0,
                    (pane_h * dpr).round(),
                    bitmap_w,
                    f64::from(border_w),
                    time_border,
                );
            }

            let tick_len = (5.0 * dpr).round();
            let tick_off = (dpr * 0.5).floor();
            for tick in &axis_frame.price_ticks {
                let (enabled, color, x) = if tick.left {
                    (
                        left_scale.border_visible,
                        left_border,
                        ((pane_left - 5.0) * dpr).round(),
                    )
                } else {
                    (
                        right_scale.border_visible,
                        right_border,
                        ((pane_left + pane_w) * dpr).round(),
                    )
                };
                if enabled {
                    rect(
                        x,
                        (tick.y * dpr).round() - tick_off,
                        tick_len,
                        f64::from(border_w),
                        color,
                    );
                }
            }
            if time_scale.border_visible && self.time_ticks_visible && self.time_axis_visible {
                let y0 = (pane_h * dpr).round();
                for x in &axis_frame.time_ticks {
                    rect(
                        (x * dpr).round() - tick_off,
                        y0,
                        f64::from(border_w),
                        tick_len,
                        time_border,
                    );
                }
            }

            let separator_color = parse(&layout.panes.separator_color, right_border);
            for separator in &axis_frame.separators {
                rect(
                    (pane_left * dpr).round(),
                    (separator * dpr).round(),
                    (pane_w * dpr).round(),
                    (PANE_SEPARATOR * dpr).max(f64::from(border_w)),
                    separator_color,
                );
            }
            if let Some(separator) = axis_frame
                .separator_hover
                .and_then(|index| axis_frame.separators.get(index))
            {
                rect(
                    0.0,
                    ((separator - 4.0) * dpr).round(),
                    bitmap_w,
                    (9.0 * dpr).round(),
                    parse(&layout.panes.separator_hover_color, separator_color),
                );
            }
        }

        let append_text = |label: &AxisLabel, output: &mut Vec<Prim>| {
            let metrics_text = match label.midpoint {
                AxisTextMidpoint::None => None,
                AxisTextMidpoint::Label => Some(label.text.as_str()),
                AxisTextMidpoint::StableTime => Some("Apr0"),
            };
            let correction =
                metrics_text.map(&midpoint_correction).unwrap_or(0.0) * label.font_scale;
            output.push(Prim::Text {
                x: (label.x * dpr) as f32,
                y: ((label.y + correction) * dpr) as f32,
                text: label.text.clone(),
                color: label.color,
                size: (layout.font_size * label.font_scale * dpr) as f32,
                family: layout.font_family.clone(),
                align: match label.align {
                    AxisTextAlign::Left => TextAlign::Left,
                    AxisTextAlign::Right => TextAlign::Right,
                    AxisTextAlign::Center => TextAlign::Center,
                },
                weight: if label.bold { 700 } else { 400 },
                italic: false,
            });
        };
        for label in axis_frame
            .labels
            .iter()
            .filter(|label| label.background.is_none())
        {
            append_text(label, output);
        }

        let mut last_attach: Option<(u32, f64)> = None;
        for label in axis_frame
            .labels
            .iter()
            .filter(|label| label.background.is_some())
        {
            if let Some((x, y, w, h, color)) = label.background {
                let bx = (x * dpr).round();
                let by = match (label.attach_group, last_attach) {
                    (Some(group), Some((previous, bottom))) if group == previous => bottom,
                    _ => (y * dpr).round(),
                };
                let bw = ((x + w) * dpr).round() - bx;
                let bh = ((y + h) * dpr).round() - by;
                last_attach = label.attach_group.map(|group| (group, by + bh));
                if label.background_corners.is_empty() {
                    output.push(Prim::Rect {
                        rect: IRect {
                            x: bx as i32,
                            y: by as i32,
                            w: bw as i32,
                            h: bh as i32,
                        },
                        color,
                    });
                } else {
                    let corners = label.background_corners;
                    let radius = (2.0 * dpr) as f32;
                    output.push(Prim::RoundRect {
                        x: bx as f32,
                        y: by as f32,
                        w: bw as f32,
                        h: bh as f32,
                        radii: [
                            if corners.top_left { radius } else { 0.0 },
                            if corners.top_right { radius } else { 0.0 },
                            if corners.bottom_right { radius } else { 0.0 },
                            if corners.bottom_left { radius } else { 0.0 },
                        ],
                        fill: color,
                        border_width: 0.0,
                        border_color: Color::rgba(0, 0, 0, 0),
                    });
                }
            } else {
                last_attach = None;
            }
            append_text(label, output);
        }
    }
}
