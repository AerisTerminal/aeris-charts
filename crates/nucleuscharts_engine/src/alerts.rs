//! Host-authoritative price-alert presentation and creation requests.
//!
//! Nucleus renders alert lines and the crosshair create chip, but it does not evaluate alert
//! conditions or deliver notifications. Those operations require the host's live price stream,
//! persistence, account limits, and background/server lifecycle.

use std::collections::{HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::{ChartEngine, ChartError, ErrorCode, PriceScaleTarget, PANELESS};
use nucleuscharts_render::draw_list::RasterImage;

pub const MAX_ALERT_LINES: usize = 4_096;
const MAX_ALERT_REQUESTS: usize = 256;
const MAX_ALERT_ID_BYTES: usize = 128;
const MAX_ALERT_LABEL_BYTES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AlertId(String);

impl AlertId {
    pub fn new(value: impl Into<String>) -> Result<Self, ChartError> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_ALERT_ID_BYTES {
            return Err(invalid(format!(
                "AlertId must contain 1..={MAX_ALERT_ID_BYTES} UTF-8 bytes"
            )));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn heap_bytes(&self) -> usize {
        self.0.capacity()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertPriceScale {
    #[default]
    Right,
    Left,
    Overlay,
}

impl From<AlertPriceScale> for PriceScaleTarget {
    fn from(value: AlertPriceScale) -> Self {
        match value {
            AlertPriceScale::Right => Self::Right,
            AlertPriceScale::Left => Self::Left,
            AlertPriceScale::Overlay => Self::Overlay,
        }
    }
}

impl TryFrom<PriceScaleTarget> for AlertPriceScale {
    type Error = ();

    fn try_from(value: PriceScaleTarget) -> Result<Self, Self::Error> {
        match value {
            PriceScaleTarget::Right => Ok(Self::Right),
            PriceScaleTarget::Left => Ok(Self::Left),
            PriceScaleTarget::Overlay => Ok(Self::Overlay),
            PriceScaleTarget::Named(_) => Err(()),
        }
    }
}

/// Price comparison selected by the host's alert dialog.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertCondition {
    #[default]
    Crossing,
    CrossingUp,
    CrossingDown,
    GreaterThan,
    LessThan,
}

/// Trigger frequency retained for indicator fidelity. The host owns actual evaluation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertFrequency {
    #[default]
    OnlyOnce,
    EveryTime,
    OncePerBar,
    OncePerBarClose,
    OncePerMinute,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertLineStatus {
    #[default]
    Active,
    Triggered,
    Expired,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlertLine {
    pub id: AlertId,
    #[serde(default)]
    pub pane_index: usize,
    #[serde(default)]
    pub price_scale: AlertPriceScale,
    pub price: f64,
    #[serde(default)]
    pub condition: AlertCondition,
    #[serde(default)]
    pub frequency: AlertFrequency,
    #[serde(default)]
    pub status: AlertLineStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AlertSnapshot {
    pub lines: Vec<AlertLine>,
}

/// A click on the crosshair's plus chip. The host opens its dialog from this exact price/scale.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlertCreateRequest {
    pub sequence: u32,
    pub pane_index: usize,
    pub price_scale: AlertPriceScale,
    pub price: f64,
    pub condition: AlertCondition,
    pub frequency: AlertFrequency,
}

#[derive(Clone, Debug)]
pub(crate) struct AlertState {
    pub lines: Vec<AlertLine>,
    pub create_button_visible: bool,
    requests: VecDeque<AlertCreateRequest>,
    next_request_sequence: u32,
    /// Host-rasterized create-chip icon (straight RGBA8). `None` falls back to
    /// the prim-composed icon so headless/native hosts keep working.
    pub(crate) create_icon: Option<RasterImage>,
    next_icon_key: u64,
}

/// Hard cap for one create-icon side: 96px RGBA is 36 KiB, negligible next to
/// the tape, and far above the ~17px the chip ever draws.
pub const MAX_ALERT_ICON_PX: u32 = 96;

impl Default for AlertState {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            create_button_visible: true,
            requests: VecDeque::new(),
            next_request_sequence: 0,
            create_icon: None,
            next_icon_key: 0,
        }
    }
}

impl AlertState {
    fn next_sequence(&mut self) -> u32 {
        self.next_request_sequence = self.next_request_sequence.wrapping_add(1).max(1);
        self.next_request_sequence
    }

    fn push_request(&mut self, request: AlertCreateRequest) {
        if self.requests.len() == MAX_ALERT_REQUESTS {
            self.requests.pop_front();
        }
        self.requests.push_back(request);
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.lines.capacity() * std::mem::size_of::<AlertLine>()
            + self.requests.capacity() * std::mem::size_of::<AlertCreateRequest>()
            + self
                .lines
                .iter()
                .map(|line| {
                    line.id.heap_bytes() + line.label.as_ref().map_or(0, |label| label.capacity())
                })
                .sum::<usize>()
            + self
                .create_icon
                .as_ref()
                .map_or(0, |icon| icon.pixels.len())
    }
}

fn invalid(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::InvalidData, message)
}

fn validate_line(line: &AlertLine) -> Result<(), ChartError> {
    AlertId::new(line.id.as_str())?;
    if !line.price.is_finite() {
        return Err(invalid("alert price must be finite"));
    }
    if line
        .label
        .as_ref()
        .is_some_and(|label| label.len() > MAX_ALERT_LABEL_BYTES)
    {
        return Err(invalid(format!(
            "alert label exceeds {MAX_ALERT_LABEL_BYTES} UTF-8 bytes"
        )));
    }
    Ok(())
}

impl ChartEngine {
    pub fn alert_snapshot(&self) -> AlertSnapshot {
        AlertSnapshot {
            lines: self.alert_state.lines.clone(),
        }
    }

    pub fn set_alert_snapshot(&mut self, snapshot: AlertSnapshot) -> Result<(), ChartError> {
        if snapshot.lines.len() > MAX_ALERT_LINES {
            return Err(ChartError::new(
                ErrorCode::ResourceLimit,
                format!("alert snapshot exceeds {MAX_ALERT_LINES} lines"),
            ));
        }
        let mut ids = HashSet::with_capacity(snapshot.lines.len());
        for line in &snapshot.lines {
            validate_line(line)?;
            if !ids.insert(line.id.as_str()) {
                return Err(invalid(format!(
                    "duplicate alert id '{}'",
                    line.id.as_str()
                )));
            }
        }
        self.alert_state.lines = snapshot.lines;
        self.invalidate_frame_trading();
        Ok(())
    }

    pub fn update_alert_line(&mut self, line: AlertLine) -> Result<(), ChartError> {
        validate_line(&line)?;
        if let Some(index) = self
            .alert_state
            .lines
            .iter()
            .position(|candidate| candidate.id == line.id)
        {
            self.alert_state.lines[index] = line;
        } else {
            if self.alert_state.lines.len() == MAX_ALERT_LINES {
                return Err(ChartError::new(
                    ErrorCode::ResourceLimit,
                    format!("alert state exceeds {MAX_ALERT_LINES} lines"),
                ));
            }
            self.alert_state.lines.push(line);
        }
        self.invalidate_frame_trading();
        Ok(())
    }

    pub fn remove_alert_line(&mut self, id: &AlertId) -> bool {
        let before = self.alert_state.lines.len();
        self.alert_state.lines.retain(|line| &line.id != id);
        let changed = before != self.alert_state.lines.len();
        if changed {
            self.invalidate_frame_trading();
        }
        changed
    }

    pub fn set_alert_create_button_visible(&mut self, visible: bool) -> bool {
        if self.alert_state.create_button_visible == visible {
            return false;
        }
        self.alert_state.create_button_visible = visible;
        self.invalidate_frame_axis();
        true
    }

    pub fn alert_create_button_visible(&self) -> bool {
        self.alert_state.create_button_visible
    }

    /// Install a host-rasterized create-chip icon (straight-alpha RGBA8 rows).
    /// The host owns rasterization (SVG decode, DPR supersampling, glyph
    /// color); the engine retains these exact pixels like a watermark, so the
    /// icon survives theme switches and device loss without re-decoding.
    /// Rejects out-of-range dimensions or a short buffer without mutation.
    pub fn set_alert_create_icon(&mut self, pixels: Vec<u8>, width: u32, height: u32) -> bool {
        if width == 0
            || height == 0
            || width > MAX_ALERT_ICON_PX
            || height > MAX_ALERT_ICON_PX
            || pixels.len() != width as usize * height as usize * 4
        {
            return false;
        }
        self.alert_state.next_icon_key = self.alert_state.next_icon_key.wrapping_add(1).max(1);
        let key = self.alert_state.next_icon_key;
        self.alert_state.create_icon = Some(RasterImage {
            key,
            width,
            height,
            pixels: pixels.into(),
        });
        self.invalidate_frame_axis();
        true
    }

    pub fn clear_alert_create_icon(&mut self) -> bool {
        if self.alert_state.create_icon.take().is_none() {
            return false;
        }
        self.invalidate_frame_axis();
        true
    }

    /// CSS-px side of the create-chip icon box the host rasterizer should
    /// target. The engine draws the installed image centered at this size, so
    /// both sides agree through this one value.
    pub fn alert_create_icon_css_size(&self) -> f64 {
        use crate::frame::alert_geometry::CREATE_ICON_FRACTION;
        (self.options.get().layout.font_size + 5.0) * CREATE_ICON_FRACTION
    }

    pub fn take_alert_create_requests(&mut self) -> Vec<AlertCreateRequest> {
        self.alert_state.requests.drain(..).collect()
    }

    pub(crate) fn queue_alert_create_request(
        &mut self,
        pane_index: usize,
        price_scale: AlertPriceScale,
        price: f64,
    ) -> AlertCreateRequest {
        let request = AlertCreateRequest {
            sequence: self.alert_state.next_sequence(),
            pane_index,
            price_scale,
            price,
            condition: AlertCondition::Crossing,
            // TradingView regular-price alerts offer Only Once and Every Time. A host may change
            // this default in its dialog, or use the interval-dependent modes represented above.
            frequency: AlertFrequency::OnlyOnce,
        };
        self.alert_state.push_request(request.clone());
        request
    }

    pub(crate) fn remove_alert_pane(&mut self, index: usize) {
        for line in &mut self.alert_state.lines {
            if line.pane_index == index {
                line.pane_index = PANELESS;
            } else if line.pane_index != PANELESS && line.pane_index > index {
                line.pane_index -= 1;
            }
        }
    }

    pub(crate) fn swap_alert_panes(&mut self, first: usize, second: usize) {
        for line in &mut self.alert_state.lines {
            if line.pane_index == first {
                line.pane_index = second;
            } else if line.pane_index == second {
                line.pane_index = first;
            }
        }
    }

    pub(crate) fn move_alert_pane(&mut self, from: usize, to: usize) {
        for line in &mut self.alert_state.lines {
            let pane = line.pane_index;
            if pane == PANELESS {
                continue;
            }
            line.pane_index = if pane == from {
                to
            } else if from < to && pane > from && pane <= to {
                pane - 1
            } else if to < from && pane >= to && pane < from {
                pane + 1
            } else {
                pane
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nucleuscharts_render::{color::Color, draw_list::Prim};

    fn chart_with_market() -> ChartEngine {
        let mut chart = ChartEngine::new(400.0, 240.0, 1.0);
        chart
            .set_series_data(
                0,
                &[10.0, 20.0, 30.0],
                &[99.0, 100.0, 101.0],
                &[102.0, 103.0, 104.0],
                &[98.0, 99.0, 100.0],
                &[101.0, 102.0, 103.0],
            )
            .unwrap();
        chart.time_scale.set_width(400.0);
        chart.fit_content();
        chart.build_frame();
        chart
    }

    fn line(id: &str, status: AlertLineStatus) -> AlertLine {
        AlertLine {
            id: AlertId::new(id).unwrap(),
            pane_index: 0,
            price_scale: AlertPriceScale::Right,
            price: 102.0,
            condition: AlertCondition::Crossing,
            frequency: AlertFrequency::EveryTime,
            status,
            label: None,
        }
    }

    #[test]
    fn snapshot_is_transactional_and_alerts_remain_host_authoritative() {
        let mut chart = chart_with_market();
        chart
            .set_alert_snapshot(AlertSnapshot {
                lines: vec![line("active", AlertLineStatus::Active)],
            })
            .unwrap();
        let before = chart.alert_snapshot();
        let mut invalid = line("bad", AlertLineStatus::Triggered);
        invalid.price = f64::NAN;
        assert!(chart
            .set_alert_snapshot(AlertSnapshot {
                lines: vec![invalid],
            })
            .is_err());
        assert_eq!(chart.alert_snapshot(), before);

        let mut negative = line("negative", AlertLineStatus::Active);
        negative.price = -1.0;
        assert!(chart.update_alert_line(negative).is_ok());
    }

    #[test]
    fn alert_lines_and_axis_indicators_use_the_shared_frame() {
        let mut chart = chart_with_market();
        chart
            .set_alert_snapshot(AlertSnapshot {
                lines: vec![line("active", AlertLineStatus::Active)],
            })
            .unwrap();
        let frame = chart.build_frame();
        let segments = chart.frame_pane_segments(0).unwrap();
        let actionable = &frame.panes[0].main[segments.drawings_end..segments.trading_end];
        assert!(actionable
            .iter()
            .any(|primitive| matches!(primitive, Prim::HLine { .. })));
        // The line is named by an attached bell badge — drawn geometry, not a glyph — so the
        // axis tag itself carries nothing but the price, like every other tag.
        let color = crate::frame::alert_geometry::alert_color(AlertLineStatus::Active);
        assert!(actionable.iter().any(|primitive| matches!(
            primitive,
            Prim::RoundRect { fill, radii, .. }
                if *fill == color && radii[0] > 0.0 && radii[1] == 0.0
        )));
        assert!(actionable
            .iter()
            .all(|primitive| !matches!(primitive, Prim::Text { text, .. } if text == "A")));
        let axis = chart.build_axis_frame(100.0, |text| text.len() as f64 * 7.0);
        assert!(axis.labels.iter().any(|label| label.text == "102.00"));
    }

    #[test]
    fn alert_lines_follow_pane_move_swap_and_removal() {
        let mut chart = chart_with_market();
        assert_eq!(chart.add_pane(false), Some(1));
        assert_eq!(chart.add_pane(false), Some(2));
        let mut alert = line("active", AlertLineStatus::Active);
        alert.pane_index = 2;
        chart
            .set_alert_snapshot(AlertSnapshot { lines: vec![alert] })
            .unwrap();

        assert!(chart.move_pane(2, 0));
        assert_eq!(chart.alert_state.lines[0].pane_index, 0);
        assert!(chart.swap_panes(0, 1));
        assert_eq!(chart.alert_state.lines[0].pane_index, 1);
        assert!(chart.remove_pane(1));
        assert_eq!(chart.alert_state.lines[0].pane_index, PANELESS);
    }

    #[test]
    fn alert_axis_tag_remains_at_its_exact_price_coordinate() {
        let mut chart = chart_with_market();
        chart
            .set_series_data(
                0,
                &[10.0, 20.0, 30.0],
                &[99.0, 100.0, 101.0],
                &[102.0, 103.0, 104.0],
                &[98.0, 99.0, 100.0],
                &[101.0, 102.0, 102.0],
            )
            .unwrap();
        chart.fit_content();
        chart
            .set_alert_snapshot(AlertSnapshot {
                lines: vec![line("active", AlertLineStatus::Active)],
            })
            .unwrap();

        let axis = chart.build_axis_frame(100.0, |text| text.len() as f64 * 7.0);
        let alert = axis
            .labels
            .iter()
            .find(|label| label.text == "102.00")
            .expect("alert axis tag");
        let expected_y = chart
            .runtime_price_coordinate(0, PriceScaleTarget::Right, 102.0)
            .expect("populated right scale");
        assert!((alert.y - expected_y).abs() <= f64::EPSILON);
    }

    #[test]
    fn crosshair_plus_chip_queues_exact_default_create_request() {
        let mut chart = chart_with_market();
        chart
            .options
            .apply_str(r##"{"crosshair":{"horzLine":{"labelBackgroundColor":"#123456"}}}"##)
            .unwrap();
        let y = chart
            .runtime_price_coordinate(0, PriceScaleTarget::Right, 102.0)
            .expect("populated right scale");
        chart.crosshair = Some((200.0, y));
        let chip = chart
            .alert_create_chip()
            .expect("visible crosshair alert chip");
        let axis = chart.build_axis_frame(100.0, |text| text.len() as f64 * 7.0);
        // The container is textless; the "+" label owns the icon-hugging ring so
        // its glyph paints after its own boxes.
        let container = axis
            .labels
            .iter()
            .find(|label| {
                label.text.is_empty()
                    && label.background.is_some_and(|(bg_x, _, w, h, _)| {
                        (w - chip.size).abs() < 1e-9
                            && (h - chip.size).abs() < 1e-9
                            && (bg_x - (chip.x + chart.pane_left)).abs() < 1e-9
                    })
            })
            .expect("round plus container");
        let (plus_x, _, plus_width, _, plus_color) = container.background.unwrap();
        assert_eq!(plus_color, Color::rgb(0x12, 0x34, 0x56));
        assert_eq!(container.background_corners, crate::AxisLabelCorners::LEFT);
        assert_eq!(container.border, None);
        // Icon and bars stay fixed white on every theme and state.
        let glyph = Color::rgb(0xff, 0xff, 0xff);
        // PlusSignSquare icon: compact rounded square plus two bars
        // proportioned to the source 24-grid (arms 8/19 long, 1.5/19 thick).
        let icon_side = chip.size * 0.62;
        let stroke = (icon_side * 1.5 / 19.0).round().max(1.0);
        let arm = icon_side * 8.0 / 19.0;
        let icon = axis
            .labels
            .iter()
            .find(|label| {
                label.text.is_empty()
                    && label.background.is_some_and(|(ix, iy, w, h, color)| {
                        color == plus_color
                            && (w - icon_side).abs() < 1e-9
                            && (h - icon_side).abs() < 1e-9
                            && (ix - (chip.x + chart.pane_left + chip.size * 0.19)).abs() < 1e-9
                            && (iy - (chip.y - chip.size * 0.31)).abs() < 1e-9
                    })
            })
            .expect("plus-square icon outline");
        assert_eq!(icon.background_corners, crate::AxisLabelCorners::ALL);
        assert_eq!(icon.border, Some((1.0, glyph)));
        let center_x = chip.x + chart.pane_left + chip.size / 2.0;
        for (bar_w, bar_h) in [(arm, stroke), (stroke, arm)] {
            assert!(
                axis.labels.iter().any(|label| {
                    label.text.is_empty()
                        && label.border.is_none()
                        && label.background.is_some_and(|(bx, by, w, h, color)| {
                            color == glyph
                                && (w - bar_w).abs() < 1e-9
                                && (h - bar_h).abs() < 1e-9
                                && (bx + w / 2.0 - center_x).abs() < 1e-9
                                && (by + h / 2.0 - chip.y).abs() < 1e-9
                        })
                }),
                "plus arm {bar_w}x{bar_h} centered on the chip"
            );
        }
        // The crosshair sits mid-pane here, off the chip: idle styling above.
        // Parking it on the chip lifts the fill a step with no blue anywhere,
        // and the button keeps its geometry and hit rect.
        chart.crosshair = Some((chip.x + chip.size / 2.0, chip.y));
        let hovered_axis = chart.build_axis_frame(100.0, |text| text.len() as f64 * 7.0);
        let lifted = Color::rgb(0x12, 0x34, 0x56).lighten(0.3);
        // Hover lifts the fills; the icon keeps the theme foreground.
        let lifted_glyph = glyph;
        let hovered_container = hovered_axis
            .labels
            .iter()
            .find(|label| {
                label.text.is_empty()
                    && label.background.is_some_and(|(_, _, w, h, color)| {
                        color == lifted
                            && (w - chip.size).abs() < 1e-9
                            && (h - chip.size).abs() < 1e-9
                    })
            })
            .expect("hovered plus container lifts with no blue");
        assert_eq!(hovered_container.border, None);
        assert_eq!(
            hovered_container.background_corners,
            crate::AxisLabelCorners::LEFT
        );
        assert!(
            hovered_axis.labels.iter().any(|label| {
                label.text.is_empty()
                    && label.border == Some((1.0, lifted_glyph))
                    && label.background.is_some_and(|(_, _, w, h, color)| {
                        color == lifted
                            && (w - icon_side).abs() < 1e-9
                            && (h - icon_side).abs() < 1e-9
                    })
            }),
            "hovered icon outline follows the glyph"
        );
        assert!(
            hovered_axis.labels.iter().any(|label| {
                label.text.is_empty()
                    && label.border.is_none()
                    && label.background.is_some_and(|(_, _, w, h, color)| {
                        color == lifted_glyph
                            && ((w - arm).abs() < 1e-9 && (h - stroke).abs() < 1e-9
                                || (w - stroke).abs() < 1e-9 && (h - arm).abs() < 1e-9)
                    })
            }),
            "hovered plus arms follow the glyph"
        );
        let price_chip = axis
            .labels
            .iter()
            .find(|label| {
                !label.text.is_empty()
                    && label.y == chip.y
                    && label
                        .background
                        .is_some_and(|(_, _, _, _, color)| color == plus_color)
            })
            .expect("primary crosshair price chip");
        assert!(
            (plus_x + plus_width - price_chip.background.unwrap().0).abs() < 1e-9,
            "the plus chip meets the price chip at the same border seam as an attached name chip"
        );
        assert!(chart.alert_create_hit_at(chip.x + chip.size / 2.0, chip.y));
        assert!(chart.activate_alert_create_at(chip.x + chip.size / 2.0, chip.y));
        let requests = chart.take_alert_create_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].pane_index, 0);
        assert_eq!(requests[0].price_scale, AlertPriceScale::Right);
        assert_eq!(requests[0].condition, AlertCondition::Crossing);
        assert_eq!(requests[0].frequency, AlertFrequency::OnlyOnce);
        assert!((requests[0].price - 102.0).abs() < 1e-9);
    }

    #[test]
    fn host_rasterized_create_icon_replaces_the_fallback_and_reaches_prims() {
        let mut chart = chart_with_market();
        let y = chart
            .runtime_price_coordinate(0, PriceScaleTarget::Right, 102.0)
            .expect("populated right scale");
        chart.crosshair = Some((200.0, y));
        // Rejections leave state untouched.
        assert!(!chart.set_alert_create_icon(vec![0u8; 3], 1, 1));
        assert!(!chart.set_alert_create_icon(vec![0u8; 4 * 97 * 97], 97, 97));
        assert!(!chart.set_alert_create_icon(vec![0u8; 4], 0, 1));
        assert!(chart.alert_state.create_icon.is_none());
        // Accept a 2x2 white square and confirm the axis picks it up.
        assert!(chart.set_alert_create_icon(vec![0xffu8; 16], 2, 2));
        assert!(chart.frame_requires_axis());
        let chip = chart
            .alert_create_chip()
            .expect("visible crosshair alert chip");
        let axis = chart.build_axis_frame(100.0, |text| text.len() as f64 * 7.0);
        assert!(!chart.frame_requires_axis());
        assert_eq!(axis.images.len(), 1);
        let icon = &axis.images[0];
        let side = chip.size * 0.8;
        assert!((icon.x - (chip.x + chart.pane_left + chip.size * 0.1)).abs() < 1e-9);
        assert!((icon.y - (chip.y - chip.size * 0.4)).abs() < 1e-9);
        assert!((icon.width - side).abs() < 1e-9);
        assert!((icon.height - side).abs() < 1e-9);
        assert_eq!((icon.image.width, icon.image.height), (2, 2));
        assert_eq!(icon.image.pixels.as_ref(), &[0xffu8; 16]);
        // The container fill remains; the prim-composed fallback is gone.
        assert!(
            axis.labels.iter().any(|label| {
                label.text.is_empty()
                    && label.background.is_some_and(|(_, _, w, h, _)| {
                        (w - chip.size).abs() < 1e-9 && (h - chip.size).abs() < 1e-9
                    })
            }),
            "container fill stays under the raster icon"
        );
        assert!(
            !axis.labels.iter().any(|label| {
                label.border.is_some()
                    && label.background.is_some_and(|(_, _, w, h, _)| {
                        (w - chip.size * 0.62).abs() < 1e-9 && (h - chip.size * 0.62).abs() < 1e-9
                    })
            }),
            "no fallback icon outline while the raster icon is installed"
        );
        // The shared converter lowers it to one image prim for every backend.
        let mut prims = Vec::new();
        chart.build_axis_primitives_into(&axis, &mut prims, |_| 0.0);
        let images = prims
            .iter()
            .filter_map(|prim| match prim {
                Prim::Image {
                    image,
                    rect,
                    opacity,
                } => Some((image, rect, opacity)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(images.len(), 1);
        assert_eq!(*images[0].2, 1.0);
        // Clearing restores the prim fallback.
        assert!(chart.clear_alert_create_icon());
        assert!(!chart.clear_alert_create_icon());
        let axis = chart.build_axis_frame(100.0, |text| text.len() as f64 * 7.0);
        assert!(axis.images.is_empty());
        assert!(
            axis.labels.iter().any(|label| {
                label.text.is_empty()
                    && label.border.is_some()
                    && label.background.is_some_and(|(_, _, w, h, _)| {
                        (w - chip.size * 0.62).abs() < 1e-9 && (h - chip.size * 0.62).abs() < 1e-9
                    })
            }),
            "fallback icon returns after clear"
        );
    }
}
