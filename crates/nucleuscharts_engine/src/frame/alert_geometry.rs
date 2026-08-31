use super::*;
use crate::{AlertLineStatus, AlertPriceScale, PriceScaleSide};

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

pub(super) fn alert_color(status: AlertLineStatus) -> Color {
    match status {
        AlertLineStatus::Active => ALERT_ACTIVE,
        AlertLineStatus::Triggered => ALERT_TRIGGERED,
        AlertLineStatus::Expired => ALERT_EXPIRED,
    }
}

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
        let size = self.options.get().layout.font_size + 5.0;
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

    pub(super) fn append_alert_create_chip(&self, labels: &mut Vec<AxisLabel>) {
        let Some(chip) = self.alert_create_chip() else {
            return;
        };
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
        labels.push(AxisLabel {
            text: "+".to_string(),
            x: x + chip.size / 2.0,
            y: chip.y,
            color: self.axis_label_text_color(background),
            align: AxisTextAlign::Center,
            midpoint: AxisTextMidpoint::Label,
            font_scale: 1.0,
            bold: true,
            background: Some((
                x,
                chip.y - chip.size / 2.0,
                chip.size,
                chip.size,
                background,
            )),
            background_corners: AxisLabelCorners {
                top_left: true,
                top_right: true,
                bottom_left: true,
                bottom_right: true,
            },
            measure_extra: 0.0,
            attach_group: None,
            border: None,
        });
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
            let radius = 7.0 * vpr as f32;
            let cx = (self.pane_w * hpr - radius as f64 - 3.0).max(radius as f64) as f32;
            out.push(Prim::Circle {
                cx,
                cy: (y * vpr) as f32,
                radius,
                fill: color,
                stroke_width: 0.0,
                stroke: color,
            });
            out.push(Prim::Text {
                x: cx,
                y: (y * vpr) as f32,
                text: "A".to_string(),
                color: Color::rgb(0xff, 0xff, 0xff),
                size: (self.options.get().layout.font_size * 0.75 * vpr) as f32,
                family: self.options.get().layout.font_family.clone(),
                align: TextAlign::Center,
                weight: 700,
                italic: false,
            });
        }
    }
}
