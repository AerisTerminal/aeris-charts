use super::*;
use crate::{
    axis_metrics::{AxisMetrics, AXIS_FONT_SCALE},
    AlertLineStatus, AlertPriceScale, PriceScaleSide,
};

const ALERT_ACTIVE: Color = PRIMARY;
const ALERT_TRIGGERED: Color = Color::rgb(0xf5, 0xa6, 0x23);
const ALERT_EXPIRED: Color = Color::rgb(0x78, 0x7b, 0x86);

#[derive(Clone, Copy)]
pub(crate) struct AlertCreateChip {
    pub(crate) pane_index: usize,
    pub(crate) price_scale: AlertPriceScale,
    pub(crate) price: f64,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) size: f64,
}

pub(crate) fn alert_color(status: AlertLineStatus) -> Color {
    match status {
        AlertLineStatus::Active => ALERT_ACTIVE,
        AlertLineStatus::Triggered => ALERT_TRIGGERED,
        AlertLineStatus::Expired => ALERT_EXPIRED,
    }
}

/// Fraction of the create chip occupied by the host icon's SVG viewport.
/// The asset already carries its own optical padding inside that viewport, so
/// shrinking it again would push its stroke below one CSS pixel at 1x DPR.
/// The TypeScript rasterizer sizes its pixels from
/// `alert_create_icon_css_size`, so both sides agree without duplicating this.
pub(crate) const CREATE_ICON_FRACTION: f64 = 1.0;

impl ChartEngine {
    pub(crate) fn alert_create_chip(&self) -> Option<AlertCreateChip> {
        if !self.alert_state.create_button_visible
            || self.crosshair_mode == CrosshairMode::Hidden
            || !self.options.get().crosshair.horz_line.label_visible
        {
            return None;
        }
        let (x_css, y_css) = self.clamped_crosshair()?;
        let pane_index = self.pane_at_y(y_css)?;
        let (from, to) = self.visible_range_for_frame()?;
        let target = self.pane_default_scale_target(pane_index);
        let price_scale = AlertPriceScale::try_from(target).ok()?;
        let (side, strip_x, strip_width) = self.price_scale_axis_geometry(pane_index, target)?;
        let series = self.scale_formatter_source(pane_index, target)?;
        let scale = pane_scale(&self.panes[pane_index], target);
        if scale.is_empty() {
            return None;
        }
        let base = self.series_base_value(series.id, from)?;
        let snap_y = self.crosshair_snap(pane_index, x_css, y_css, from, to).1;
        let raw_price = scale.coordinate_to_price(snap_y, base);
        let min_move = series.price_format.min_move;
        let price = if min_move.is_finite() && min_move > 0.0 {
            (raw_price / min_move).round() * min_move
        } else {
            raw_price
        };
        let size = self.axis_metrics().crosshair_price_tag_height();
        let full_x = if side == PriceScaleSide::Right {
            strip_x - size
        } else {
            strip_x + strip_width
        };
        Some(AlertCreateChip {
            pane_index,
            price_scale,
            price,
            x: full_x - self.pane_left,
            y: snap_y,
            size,
        })
    }

    pub fn alert_create_hit_at(&self, x_css: f64, y_css: f64) -> bool {
        if !x_css.is_finite() || !y_css.is_finite() {
            return false;
        }
        self.alert_create_chip().is_some_and(|chip| {
            x_css >= chip.x
                && x_css <= chip.x + chip.size
                && y_css >= chip.y - chip.size / 2.0
                && y_css <= chip.y + chip.size / 2.0
        })
    }

    pub fn activate_alert_create_at(&mut self, x_css: f64, y_css: f64) -> bool {
        let Some(chip) = self.alert_create_chip() else {
            return false;
        };
        if x_css < chip.x
            || x_css > chip.x + chip.size
            || y_css < chip.y - chip.size / 2.0
            || y_css > chip.y + chip.size / 2.0
        {
            return false;
        }
        self.queue_alert_create_request(chip.pane_index, chip.price_scale, chip.price);
        true
    }

    pub(super) fn append_alert_create_chip(&self, frame: &mut AxisFrame) {
        let Some(chip) = self.alert_create_chip() else {
            return;
        };
        let labels = &mut frame.labels;
        let x = chip.x + self.pane_left;
        let background = css_color(
            &self
                .options
                .get()
                .crosshair
                .horz_line
                .label_background_color,
            CROSSHAIR_LABEL_BG,
        );
        // Hover restyles only — the chip stays visible whenever the crosshair is.
        // Pointer and hit rect share chart space (`alert_create_hit_at`), and every
        // crosshair move already rebuilds the axis, so no extra invalidation is needed.
        let hovered = self.crosshair.is_some_and(|(pointer_x, pointer_y)| {
            pointer_x >= chip.x
                && pointer_x <= chip.x + chip.size
                && (pointer_y - chip.y).abs() <= chip.size / 2.0
        });
        // Neutral hover: the fill lifts a step; the icon stays fixed white —
        // the chip fill is dark on both themes, so one color reads everywhere.
        // Never a blue fill — the button stays chrome, not a call to action.
        let fill = if hovered {
            background.lighten(0.3)
        } else {
            background
        };
        let glyph = Color::rgb(0xff, 0xff, 0xff);
        // The container is textless; the icon label below owns the "+" glyph so
        // each label's text paints after its own boxes and nothing covers it.
        labels.push(AxisLabel {
            text: String::new(),
            x: x + chip.size / 2.0,
            y: chip.y,
            color: glyph,
            align: AxisTextAlign::Center,
            midpoint: AxisTextMidpoint::Label,
            font_scale: AXIS_FONT_SCALE,
            bold: false,
            background: Some((x, chip.y - chip.size / 2.0, chip.size, chip.size, fill)),
            // Attached like before: rounded on the outer edge, square against
            // the price tag — no radius on the tag side, no outer border.
            background_corners: match chip.price_scale {
                AlertPriceScale::Left => AxisLabelCorners::RIGHT,
                AlertPriceScale::Right | AlertPriceScale::Overlay => AxisLabelCorners::LEFT,
            },
            measure_extra: 0.0,
            attach_group: None,
            border: None,
        });
        // Host icon wins when installed: the exact rasterized asset, drawn by
        // every backend through the shared image primitive. Otherwise the
        // prim-composed fallback below keeps headless/native hosts working.
        if let Some(image) = self.alert_state.create_icon.clone() {
            let side = chip.size * CREATE_ICON_FRACTION;
            frame.images.push(AxisIcon {
                x: x + (chip.size - side) / 2.0,
                y: chip.y - side / 2.0,
                width: side,
                height: side,
                image,
            });
            return;
        }
        // PlusSignSquare icon (HugeIcons free set, MIT — geometry replicated, not
        // the asset file, which is SVG for the DOM and unloadable here): on its
        // 24-grid the square spans 2.5→21.5 with ~1.4 radius and the plus spans
        // 8→16 at 1.5 stroke with round caps. System fonts cannot be trusted
        // with such a glyph (same reason the alert bell is prim-built), so the
        // icon is a small rounded box carrying the plus bars, all painted from
        // the same box machinery every backend already executes for axis
        // labels. Interiors repaint the chip fill opaque: a transparent fill
        // would be a no-op blend and leave a solid block of the stroke color.
        let icon = chip.size * 0.62;
        let icon_x = x + (chip.size - icon) / 2.0;
        let icon_y = chip.y - icon / 2.0;
        labels.push(AxisLabel {
            text: String::new(),
            x: x + chip.size / 2.0,
            y: chip.y,
            color: glyph,
            align: AxisTextAlign::Center,
            midpoint: AxisTextMidpoint::Label,
            font_scale: AXIS_FONT_SCALE,
            bold: false,
            background: Some((icon_x, icon_y, icon, icon, fill)),
            background_corners: AxisLabelCorners::ALL,
            measure_extra: 0.0,
            attach_group: None,
            border: Some((1.0, glyph)),
        });
        // Plus arms: 8 of the icon square's 19 inner units long, 1.5 thick.
        let center_x = x + chip.size / 2.0;
        let arm = icon * 8.0 / 19.0;
        let stroke = (icon * 1.5 / 19.0).round().max(1.0);
        for (bar_x, bar_y, bar_w, bar_h) in [
            (center_x - arm / 2.0, chip.y - stroke / 2.0, arm, stroke),
            (center_x - stroke / 2.0, chip.y - arm / 2.0, stroke, arm),
        ] {
            labels.push(AxisLabel {
                text: String::new(),
                x: center_x,
                y: chip.y,
                color: glyph,
                align: AxisTextAlign::Center,
                midpoint: AxisTextMidpoint::Label,
                font_scale: AXIS_FONT_SCALE,
                bold: false,
                background: Some((bar_x, bar_y, bar_w, bar_h, glyph)),
                background_corners: AxisLabelCorners::NONE,
                measure_extra: 0.0,
                attach_group: None,
                border: None,
            });
        }
    }

    pub(super) fn build_alert_lines_frame(
        &self,
        pane_index: usize,
        pane_w_px: i32,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        for line in &self.alert_state.lines {
            if line.pane_index != pane_index {
                continue;
            }
            let target = PriceScaleTarget::from(line.price_scale);
            let Some(y) = self.runtime_price_coordinate(pane_index, target, line.price) else {
                continue;
            };
            let color = alert_color(line.status);
            out.push(Prim::HLine {
                y: (y * vpr).round() as i32,
                x0: 0,
                x1: pane_w_px,
                width: 1f64.max(vpr.floor()) as i32,
                style: match line.status {
                    AlertLineStatus::Active => LineStyle::Dashed,
                    AlertLineStatus::Triggered => LineStyle::Solid,
                    AlertLineStatus::Expired => LineStyle::Dotted,
                },
                color,
            });
            self.push_alert_badge(out, y, color, hpr, vpr);
        }
    }

    /// The badge that names the line: a square chip attached to the pane-facing edge of the
    /// alert's price tag, carrying a bell. It mirrors the crosshair's create chip — rounded on
    /// its outer edge, square against the price tag — so the pair reads as one attached control
    /// and the price tag itself is free to show nothing but the price, like every other tag.
    fn push_alert_badge(&self, out: &mut Vec<Prim>, y: f64, color: Color, hpr: f64, vpr: f64) {
        // The badge shares the price tag's height so the pair reads as one attached control.
        let size = self.axis_metrics().price_tag_height();
        let left = (self.pane_w - size).max(0.0);
        let radius = AxisMetrics::TAG_RADIUS as f32 * vpr as f32;
        out.push(Prim::RoundRect {
            x: (left * hpr) as f32,
            y: ((y - size / 2.0) * vpr) as f32,
            w: (size * hpr) as f32,
            h: (size * vpr) as f32,
            radii: [radius, 0.0, 0.0, radius],
            fill: color,
            border_width: 0.0,
            border_color: color,
        });
        self.push_alert_bell(
            out,
            left + size / 2.0,
            y,
            size * 0.52,
            color.contrast_text(),
            hpr,
            vpr,
        );
    }

    /// A bell built from prims rather than a glyph: the badge has to read as an alert in every
    /// embedding, and the host's `font_family` is not guaranteed to carry a bell character.
    #[allow(clippy::too_many_arguments)] // center/size/color/scale map 1:1 onto the drawn glyph
    fn push_alert_bell(
        &self,
        out: &mut Vec<Prim>,
        center_x: f64,
        center_y: f64,
        size: f64,
        color: Color,
        hpr: f64,
        vpr: f64,
    ) {
        let width = size * 0.68;
        let body_h = size * 0.72;
        let top = center_y - size / 2.0;
        // Dome-topped body: a rounded rect whose top radii are half its width.
        out.push(Prim::RoundRect {
            x: ((center_x - width / 2.0) * hpr) as f32,
            y: (top * vpr) as f32,
            w: (width * hpr) as f32,
            h: (body_h * vpr) as f32,
            radii: [
                (width / 2.0 * hpr) as f32,
                (width / 2.0 * hpr) as f32,
                0.0,
                0.0,
            ],
            fill: color,
            border_width: 0.0,
            border_color: color,
        });
        // Rim, wider than the body, then the clapper below it.
        let rim_w = size * 0.94;
        let rim_h = (size * 0.15).max(1.0);
        out.push(Prim::Rect {
            rect: IRect {
                x: ((center_x - rim_w / 2.0) * hpr).round() as i32,
                y: ((top + body_h) * vpr).round() as i32,
                w: (rim_w * hpr).round() as i32,
                h: (rim_h * vpr).round().max(1.0) as i32,
            },
            color,
        });
        out.push(Prim::Circle {
            cx: (center_x * hpr) as f32,
            cy: ((top + body_h + rim_h + size * 0.1) * vpr) as f32,
            radius: (size * 0.13).max(1.0) as f32 * vpr as f32,
            fill: color,
            stroke_width: 0.0,
            stroke: color,
        });
    }
}
