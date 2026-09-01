use super::*;
use crate::trading::{
    ExecutionKind, OrderRole, OrderSide, OrderStatus, PositionSide, TradingGroupVisualState,
};
use crate::Pane;

#[derive(Clone, Copy)]
struct TradingChipLayout {
    x: f64,
    y: f64,
    hpr: f64,
    vpr: f64,
}

#[derive(Clone, Copy)]
struct TradingTooltip<'a> {
    text: &'a str,
    color: Color,
    layout: TradingChipLayout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TradingControlSegmentKind {
    Quantity,
    Pnl,
    OrderType,
    Cancel,
    Confirm,
    Discard,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TradingControlFeedback {
    #[default]
    Idle,
    Hovered,
    Pressed,
}

#[derive(Clone, Copy)]
struct TradingControlSegment<'a> {
    kind: TradingControlSegmentKind,
    text: &'a str,
    width: f64,
    color: Color,
    filled: bool,
}

struct TradingControlCluster<'a> {
    segments: &'a [TradingControlSegment<'a>],
    left: f64,
}

impl TradingControlCluster<'_> {
    fn start(&self) -> f64 {
        self.left
    }
}

const QUANTITY_WIDTH: f64 = 44.0;
const PNL_WIDTH: f64 = 96.0;
const ORDER_TYPE_WIDTH: f64 = 92.0;
const CONTROL_GAP: f64 = 3.0;
const ORDER_MARKER_SPAN: f64 = 280.0;

pub(crate) fn trading_order_color(
    style: &crate::TradingStyle,
    role: OrderRole,
    side: OrderSide,
    status: OrderStatus,
) -> Color {
    match status {
        OrderStatus::Rejected
        | OrderStatus::Cancelled
        | OrderStatus::Expired
        | OrderStatus::Filled => style.rejected,
        OrderStatus::PendingSubmit | OrderStatus::PendingModify | OrderStatus::PendingCancel => {
            style.pending
        }
        _ => match role {
            OrderRole::StopLoss => style.stop_loss,
            OrderRole::TakeProfit => style.take_profit,
            OrderRole::Working => match side {
                OrderSide::Buy => style.buy,
                OrderSide::Sell => style.sell,
            },
        },
    }
}

impl ChartEngine {
    fn trading_control_kind(kind: crate::TradingHitKind) -> Option<TradingControlSegmentKind> {
        match kind {
            crate::TradingHitKind::CancelButton => Some(TradingControlSegmentKind::Cancel),
            crate::TradingHitKind::ConfirmButton => Some(TradingControlSegmentKind::Confirm),
            crate::TradingHitKind::DiscardButton => Some(TradingControlSegmentKind::Discard),
            _ => None,
        }
    }

    fn trading_control_feedback(&self, kind: TradingControlSegmentKind) -> TradingControlFeedback {
        if self
            .trading_state
            .feedback_pressed
            .as_ref()
            .and_then(|hit| Self::trading_control_kind(hit.kind))
            == Some(kind)
        {
            TradingControlFeedback::Pressed
        } else if self
            .trading_state
            .feedback_hover
            .as_ref()
            .and_then(|hit| Self::trading_control_kind(hit.kind))
            == Some(kind)
        {
            TradingControlFeedback::Hovered
        } else {
            TradingControlFeedback::Idle
        }
    }

    pub(crate) fn runtime_scale_base(&self, pane_index: usize, target: PriceScaleTarget) -> f64 {
        self.visible_range()
            .and_then(|(from, _)| {
                let series = self.series.iter().find(|series| {
                    series.visible
                        && !series.removed
                        && series.pane_index == pane_index
                        && series_scale_target(series) == target
                })?;
                self.series_base_value(series.id, from)
            })
            .unwrap_or(0.0)
    }

    pub(crate) fn runtime_price_coordinate(
        &self,
        pane_index: usize,
        target: PriceScaleTarget,
        price: f64,
    ) -> Option<f64> {
        let pane = self.panes.get(pane_index)?;
        let scale = pane_scale(pane, target);
        if scale.is_empty() {
            return None;
        }
        Some(scale.price_to_coordinate(price, self.runtime_scale_base(pane_index, target)))
    }

    pub(crate) fn trading_price_coordinate(
        &self,
        pane_index: usize,
        target: crate::TradingPriceScale,
        price: f64,
    ) -> Option<f64> {
        let target = PriceScaleTarget::from(target);
        self.runtime_price_coordinate(pane_index, target, price)
    }

    pub(crate) fn trading_coordinate_to_price(
        &self,
        pane_index: usize,
        target: crate::TradingPriceScale,
        y: f64,
    ) -> Option<f64> {
        let pane = self.panes.get(pane_index)?;
        let target = PriceScaleTarget::from(target);
        let scale = pane_scale(pane, target);
        if scale.is_empty() {
            return None;
        }
        Some(scale.coordinate_to_price(y, self.runtime_scale_base(pane_index, target)))
    }

    pub(crate) fn format_trading_price(&self, value: f64) -> String {
        match self.trading_state.instrument.price_precision {
            Some(precision) => format!("{value:.precision$}", precision = precision as usize),
            None => self.price_formatter.format(value),
        }
    }

    pub(crate) fn format_trading_quantity(&self, value: f64) -> String {
        match self.trading_state.instrument.quantity_precision {
            Some(precision) => format!("{value:.precision$}", precision = precision as usize),
            None if value.fract().abs() < f64::EPSILON => format!("{value:.0}"),
            None => format!("{value:.4}")
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string(),
        }
    }

    fn trading_order_preview(&self, order: &crate::WorkingOrder) -> Option<&crate::TradingPreview> {
        self.trading_state.interaction.preview().filter(|preview| {
            matches!(
                &preview.source,
                crate::TradingPreviewSource::Order { order_id } if order_id == &order.id
            )
        })
    }

    pub(crate) fn trading_effective_order_price(&self, order: &crate::WorkingOrder) -> f64 {
        self.trading_order_preview(order)
            .map_or(order.price, |preview| preview.price)
    }

    fn trading_pnl_text(&self, value: f64, currency: Option<&str>) -> String {
        let currency = currency
            .or(self.trading_state.instrument.currency.as_deref())
            .unwrap_or("");
        format!(
            "{}{value:.2}{}{}",
            if value >= 0.0 { "+" } else { "" },
            if currency.is_empty() { "" } else { " " },
            currency
        )
    }

    fn trading_chip_background(&self) -> Color {
        let fallback = nucleuscharts_core::style::DEFAULT_SURFACE_RGB;
        Color::parse_css(&self.options.get().layout.background.color)
            .unwrap_or(Color::rgb(fallback.0, fallback.1, fallback.2))
            .solid()
    }

    pub(crate) fn trading_axis_control_size(&self) -> f64 {
        self.options.get().layout.font_size + 5.0
    }

    pub(crate) fn trading_marker_end(&self) -> f64 {
        (self.pane_w - self.trading_axis_control_size()).max(6.0)
    }

    pub(crate) fn trading_marker_start(&self) -> f64 {
        (self.trading_marker_end() - ORDER_MARKER_SPAN).max(6.0)
    }

    pub(crate) fn trading_order_chip_start(&self, _order: &crate::WorkingOrder) -> f64 {
        self.trading_marker_start()
    }

    pub(crate) fn trading_preview_chip_start(&self, _preview: &crate::TradingPreview) -> f64 {
        self.trading_marker_start()
    }

    pub(crate) fn trading_order_chip_hit(
        &self,
        _order: &crate::WorkingOrder,
        _x: f64,
    ) -> crate::TradingHitKind {
        crate::TradingHitKind::OrderLine
    }

    pub(crate) fn trading_position_chip_hit(&self, _x: f64) -> crate::TradingHitKind {
        crate::TradingHitKind::PositionLine
    }

    pub(crate) fn trading_confirmation_hit(
        &self,
        preview: &crate::TradingPreview,
        x: f64,
    ) -> Option<crate::TradingHitKind> {
        if preview.phase != crate::TradingPreviewPhase::AwaitingConfirmation {
            return None;
        }
        let main_x = self.trading_preview_chip_start(preview);
        let discard_x = main_x - 126.0;
        if x >= discard_x && x <= discard_x + 58.0 {
            Some(crate::TradingHitKind::DiscardButton)
        } else if x >= discard_x + 60.0 && x <= main_x - 6.0 {
            Some(crate::TradingHitKind::ConfirmButton)
        } else {
            None
        }
    }

    fn push_trading_segment(
        &self,
        out: &mut Vec<Prim>,
        segment: TradingControlSegment<'_>,
        feedback: TradingControlFeedback,
        layout: TradingChipLayout,
    ) {
        let TradingControlSegment {
            text,
            width,
            color,
            filled,
            ..
        } = segment;
        let TradingChipLayout { x, y, hpr, vpr } = layout;
        let font_size = self.options.get().layout.font_size;
        let height = font_size + 5.0;
        let bx = (x * hpr) as f32;
        let by = ((y - height / 2.0) * vpr) as f32;
        let bw = (width * hpr) as f32;
        let bh = (height * vpr) as f32;
        let fill = match (filled, feedback) {
            (true, TradingControlFeedback::Idle) => color.solid(),
            (true, TradingControlFeedback::Hovered) => color.solid().lighten(0.16),
            (true, TradingControlFeedback::Pressed) => color.solid().darken(0.72),
            (false, TradingControlFeedback::Idle) => self.trading_chip_background(),
            (false, TradingControlFeedback::Hovered) => {
                Color::rgba(color.r(), color.g(), color.b(), 44)
            }
            (false, TradingControlFeedback::Pressed) => {
                Color::rgba(color.r(), color.g(), color.b(), 78)
            }
        };
        out.push(Prim::RoundRect {
            x: bx,
            y: by,
            w: bw,
            h: bh,
            radii: [0.0; 4],
            fill,
            border_width: vpr.max(1.0) as f32,
            border_color: color,
        });
        let is_close_icon = text == "×";
        out.push(Prim::Text {
            x: ((x + width / 2.0) * hpr) as f32,
            y: (y * vpr) as f32,
            text: text.to_string(),
            color: if filled { color.contrast_text() } else { color },
            size: (font_size * if is_close_icon { 1.45 } else { 1.0 } * vpr) as f32,
            family: self.options.get().layout.font_family.clone(),
            align: TextAlign::Center,
            weight: if is_close_icon { 700 } else { 400 },
            italic: false,
        });
    }

    fn push_trading_cluster(
        &self,
        out: &mut Vec<Prim>,
        cluster: &TradingControlCluster<'_>,
        hovered: Option<TradingControlSegmentKind>,
        pressed: Option<TradingControlSegmentKind>,
        layout: TradingChipLayout,
    ) {
        let TradingChipLayout { y, hpr, vpr, .. } = layout;
        let mut cursor = cluster.start();
        for (index, segment) in cluster.segments.iter().enumerate() {
            if index > 0 {
                cursor += CONTROL_GAP;
            }
            let feedback = if pressed == Some(segment.kind) {
                TradingControlFeedback::Pressed
            } else if hovered == Some(segment.kind) {
                TradingControlFeedback::Hovered
            } else {
                TradingControlFeedback::Idle
            };
            self.push_trading_segment(
                out,
                *segment,
                feedback,
                TradingChipLayout {
                    x: cursor,
                    y,
                    hpr,
                    vpr,
                },
            );
            cursor += segment.width;
        }
    }

    pub(crate) fn trading_position_color(&self, side: PositionSide) -> Color {
        match side {
            PositionSide::Long => self.trading_state.style.buy,
            PositionSide::Short => self.trading_state.style.sell,
        }
    }

    fn trading_protection_pnl(
        &self,
        order: &crate::WorkingOrder,
        price: f64,
    ) -> Option<(String, Color)> {
        let position = self
            .trading_state
            .positions
            .iter()
            .find(|position| order.position_id.as_ref() == Some(&position.id))?;
        let direction = if position.side == PositionSide::Long {
            1.0
        } else {
            -1.0
        };
        let quantity = (order.quantity - order.filled_quantity).max(0.0);
        let value = (price - position.average_price)
            * direction
            * quantity
            * self.trading_state.instrument.point_value.unwrap_or(1.0);
        let color = if value >= 0.0 {
            self.trading_state.style.profit
        } else {
            self.trading_state.style.risk
        };
        Some((
            self.trading_pnl_text(value, position.currency.as_deref()),
            color,
        ))
    }

    fn push_trading_endpoint(&self, out: &mut Vec<Prim>, y: f64, color: Color, hpr: f64, vpr: f64) {
        out.push(Prim::Circle {
            cx: ((self.pane_w - 8.0) * hpr) as f32,
            cy: (y * vpr) as f32,
            radius: (3.0 * vpr) as f32,
            fill: self.trading_chip_background(),
            stroke_width: (1.0 * vpr) as f32,
            stroke: color,
        });
    }

    fn push_trading_tooltip(
        &self,
        out: &mut Vec<Prim>,
        text: &str,
        pane: &Pane,
        color: Color,
        layout: TradingChipLayout,
    ) {
        let TradingChipLayout {
            x: center_x,
            y: line_y,
            hpr,
            vpr,
        } = layout;
        let font_size = self.options.get().layout.font_size;
        let height = font_size + 7.0;
        let width = self.measure_text_run(
            text,
            font_size,
            &self.options.get().layout.font_family,
            400,
            false,
        ) + 12.0;
        let x = (center_x - width / 2.0).clamp(4.0, (self.pane_w - width - 4.0).max(4.0));
        let above = line_y - (font_size + 5.0) / 2.0 - height - 5.0;
        let y = if above >= pane.top + 2.0 {
            above
        } else {
            line_y + (font_size + 5.0) / 2.0 + 5.0
        };
        out.push(Prim::RoundRect {
            x: (x * hpr) as f32,
            y: (y * vpr) as f32,
            w: (width * hpr) as f32,
            h: (height * vpr) as f32,
            radii: [3.0 * vpr as f32; 4],
            fill: self.trading_chip_background(),
            border_width: vpr.max(1.0) as f32,
            border_color: color,
        });
        out.push(Prim::Text {
            x: ((x + width / 2.0) * hpr) as f32,
            y: ((y + height / 2.0) * vpr) as f32,
            text: text.to_string(),
            color: self.trading_state.style.label,
            size: (font_size * vpr) as f32,
            family: self.options.get().layout.font_family.clone(),
            align: TextAlign::Center,
            weight: 400,
            italic: false,
        });
    }

    pub(super) fn build_trading_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        regions: &mut Vec<Prim>,
        lines: &mut Vec<Prim>,
    ) {
        let Some(pane) = self.panes.get(pane_index) else {
            return;
        };
        let width = (self.pane_w * hpr).round() as i32;
        let min_line_width = vpr.floor().max(1.0) as i32;
        let mut tooltip = None;

        for position in &self.trading_state.positions {
            if position.pane_index != pane_index {
                continue;
            }
            let Some(y) = self.trading_price_coordinate(
                pane_index,
                position.price_scale,
                position.average_price,
            ) else {
                continue;
            };
            let hovered = self.trading_state.feedback_hover.as_ref().filter(|hit| {
                matches!(&hit.object, crate::TradingObjectId::Position(id) if id == &position.id)
            });
            let pressed = self.trading_state.feedback_pressed.as_ref().filter(|hit| {
                matches!(&hit.object, crate::TradingObjectId::Position(id) if id == &position.id)
            });
            let pending =
                self.trading_state.interaction.pending_position_id() == Some(&position.id);
            let position_color = if pending {
                self.trading_state.style.pending
            } else {
                self.trading_position_color(position.side)
            };
            lines.push(Prim::HLine {
                y: (y * vpr).round() as i32,
                x0: (self.trading_marker_start() * hpr).round() as i32,
                x1: (self.trading_marker_end() * hpr).round() as i32,
                width: if hovered.is_some_and(|hit| hit.kind == crate::TradingHitKind::PositionLine)
                {
                    min_line_width.max(2)
                } else {
                    min_line_width
                },
                style: LineStyle::Solid,
                color: position_color,
            });
            if hovered.is_some() {
                self.push_trading_endpoint(lines, y, position_color, hpr, vpr);
            }
            let signed_quantity = if position.side == PositionSide::Long {
                position.quantity
            } else {
                -position.quantity
            };
            let quantity = self.format_trading_quantity(signed_quantity);
            let pnl = position.display_pnl.map_or_else(
                || "—".to_string(),
                |value| self.trading_pnl_text(value, position.currency.as_deref()),
            );
            let pnl_color = position.display_pnl.map_or(position_color, |value| {
                if value >= 0.0 {
                    self.trading_state.style.profit
                } else {
                    self.trading_state.style.risk
                }
            });
            let segments = [
                TradingControlSegment {
                    kind: TradingControlSegmentKind::Quantity,
                    text: quantity.as_str(),
                    width: QUANTITY_WIDTH,
                    color: position_color,
                    filled: true,
                },
                TradingControlSegment {
                    kind: TradingControlSegmentKind::Pnl,
                    text: pnl.as_str(),
                    width: PNL_WIDTH,
                    color: pnl_color,
                    filled: false,
                },
            ];
            let cluster = TradingControlCluster {
                segments: &segments,
                left: self.trading_marker_start(),
            };
            let hovered_segment = hovered.and_then(|hit| Self::trading_control_kind(hit.kind));
            let pressed_segment = pressed.and_then(|hit| Self::trading_control_kind(hit.kind));
            self.push_trading_cluster(
                lines,
                &cluster,
                hovered_segment,
                pressed_segment,
                TradingChipLayout {
                    x: cluster.start(),
                    y,
                    hpr,
                    vpr,
                },
            );
            if hovered.is_some_and(|hit| hit.kind == crate::TradingHitKind::CancelButton) {
                tooltip = Some(TradingTooltip {
                    text: "Close Position",
                    color: position_color,
                    layout: TradingChipLayout {
                        x: self.trading_marker_end() + self.trading_axis_control_size() / 2.0,
                        y,
                        hpr,
                        vpr,
                    },
                });
            }
        }

        for order in &self.trading_state.orders {
            if order.pane_index != pane_index {
                continue;
            }
            let display_price = self.trading_effective_order_price(order);
            let Some(y) =
                self.trading_price_coordinate(pane_index, order.price_scale, display_price)
            else {
                continue;
            };
            let preview = self.trading_order_preview(order);
            let base_color = trading_order_color(
                &self.trading_state.style,
                order.role,
                order.side,
                order.status,
            );
            let color = match preview.map(|preview| preview.phase) {
                Some(crate::TradingPreviewPhase::Pending) => self.trading_state.style.pending,
                Some(
                    crate::TradingPreviewPhase::Dragging
                    | crate::TradingPreviewPhase::AwaitingConfirmation,
                ) => Color::rgba(base_color.r(), base_color.g(), base_color.b(), 176),
                None => base_color,
            };
            let hovered = self.trading_state.feedback_hover.as_ref().filter(
                |hit| matches!(&hit.object, crate::TradingObjectId::Order(id) if id == &order.id),
            );
            let pressed = self.trading_state.feedback_pressed.as_ref().filter(
                |hit| matches!(&hit.object, crate::TradingObjectId::Order(id) if id == &order.id),
            );
            lines.push(Prim::HLine {
                y: (y * vpr).round() as i32,
                x0: (self.trading_marker_start() * hpr).round() as i32,
                x1: (self.trading_marker_end() * hpr).round() as i32,
                width: if hovered.is_some_and(|hit| hit.kind == crate::TradingHitKind::OrderLine) {
                    min_line_width.max(2)
                } else {
                    min_line_width
                },
                style: if preview.is_some()
                    || matches!(
                        order.status,
                        OrderStatus::PendingSubmit
                            | OrderStatus::PendingModify
                            | OrderStatus::PendingCancel
                    ) {
                    LineStyle::Dotted
                } else {
                    LineStyle::Solid
                },
                color,
            });
            if hovered.is_some() {
                self.push_trading_endpoint(lines, y, color, hpr, vpr);
            }
            let remaining = (order.quantity - order.filled_quantity).max(0.0);
            let quantity = if order.filled_quantity > 0.0 {
                format!(
                    "{}/{}",
                    self.format_trading_quantity(remaining),
                    self.format_trading_quantity(order.quantity)
                )
            } else {
                self.format_trading_quantity(remaining)
            };
            let kind = match order.kind {
                crate::OrderKind::Limit => "Limit",
                crate::OrderKind::Stop => "Stop",
                crate::OrderKind::StopLimit => "Stop Limit",
            };
            let descriptor = if preview.is_some_and(|preview| {
                matches!(
                    preview.phase,
                    crate::TradingPreviewPhase::Dragging
                        | crate::TradingPreviewPhase::AwaitingConfirmation
                )
            }) {
                kind.to_string()
            } else {
                format!(
                    "{} {kind}",
                    if order.side == OrderSide::Buy {
                        "Buy"
                    } else {
                        "Sell"
                    }
                )
            };
            let main_x = preview.map_or_else(
                || self.trading_order_chip_start(order),
                |preview| self.trading_preview_chip_start(preview),
            );
            if let Some(preview) = preview {
                match preview.phase {
                    crate::TradingPreviewPhase::Dragging => {
                        let side = if preview.side == OrderSide::Buy {
                            "Buy"
                        } else {
                            "Sell"
                        };
                        self.push_trading_segment(
                            lines,
                            TradingControlSegment {
                                kind: TradingControlSegmentKind::Quantity,
                                text: side,
                                width: 46.0,
                                color: base_color,
                                filled: true,
                            },
                            TradingControlFeedback::Idle,
                            TradingChipLayout {
                                x: main_x - 52.0,
                                y,
                                hpr,
                                vpr,
                            },
                        );
                    }
                    crate::TradingPreviewPhase::AwaitingConfirmation => {
                        self.push_trading_segment(
                            lines,
                            TradingControlSegment {
                                kind: TradingControlSegmentKind::Discard,
                                text: "Discard",
                                width: 58.0,
                                color: self.trading_state.style.rejected,
                                filled: false,
                            },
                            self.trading_control_feedback(TradingControlSegmentKind::Discard),
                            TradingChipLayout {
                                x: main_x - 126.0,
                                y,
                                hpr,
                                vpr,
                            },
                        );
                        self.push_trading_segment(
                            lines,
                            TradingControlSegment {
                                kind: TradingControlSegmentKind::Confirm,
                                text: "Confirm",
                                width: 60.0,
                                color: self.trading_state.style.position,
                                filled: true,
                            },
                            self.trading_control_feedback(TradingControlSegmentKind::Confirm),
                            TradingChipLayout {
                                x: main_x - 66.0,
                                y,
                                hpr,
                                vpr,
                            },
                        );
                    }
                    crate::TradingPreviewPhase::Pending => {}
                }
            }
            let hovered_segment = hovered.and_then(|hit| Self::trading_control_kind(hit.kind));
            let pressed_segment = pressed.and_then(|hit| Self::trading_control_kind(hit.kind));
            if order.role == OrderRole::Working {
                let segments = [
                    TradingControlSegment {
                        kind: TradingControlSegmentKind::Quantity,
                        text: quantity.as_str(),
                        width: QUANTITY_WIDTH,
                        color,
                        filled: true,
                    },
                    TradingControlSegment {
                        kind: TradingControlSegmentKind::OrderType,
                        text: descriptor.as_str(),
                        width: ORDER_TYPE_WIDTH,
                        color,
                        filled: false,
                    },
                ];
                let cluster = TradingControlCluster {
                    segments: &segments,
                    left: self.trading_marker_start(),
                };
                self.push_trading_cluster(
                    lines,
                    &cluster,
                    hovered_segment,
                    pressed_segment,
                    TradingChipLayout {
                        x: main_x,
                        y,
                        hpr,
                        vpr,
                    },
                );
                if hovered.is_some_and(|hit| hit.kind == crate::TradingHitKind::CancelButton) {
                    tooltip = Some(TradingTooltip {
                        text: "Cancel order",
                        color,
                        layout: TradingChipLayout {
                            x: self.trading_marker_end() + self.trading_axis_control_size() / 2.0,
                            y,
                            hpr,
                            vpr,
                        },
                    });
                }
            } else {
                let (pnl, pnl_color) = self
                    .trading_protection_pnl(order, display_price)
                    .unwrap_or_else(|| ("—".to_string(), color));
                let segments = [
                    TradingControlSegment {
                        kind: TradingControlSegmentKind::Quantity,
                        text: quantity.as_str(),
                        width: QUANTITY_WIDTH,
                        color,
                        filled: true,
                    },
                    TradingControlSegment {
                        kind: TradingControlSegmentKind::Pnl,
                        text: pnl.as_str(),
                        width: PNL_WIDTH,
                        color: pnl_color,
                        filled: false,
                    },
                ];
                let cluster = TradingControlCluster {
                    segments: &segments,
                    left: self.trading_marker_start(),
                };
                self.push_trading_cluster(
                    lines,
                    &cluster,
                    hovered_segment,
                    pressed_segment,
                    TradingChipLayout {
                        x: main_x,
                        y,
                        hpr,
                        vpr,
                    },
                );
                if hovered.is_some_and(|hit| hit.kind == crate::TradingHitKind::CancelButton) {
                    tooltip = Some(TradingTooltip {
                        text: "Cancel order",
                        color,
                        layout: TradingChipLayout {
                            x: self.trading_marker_end() + self.trading_axis_control_size() / 2.0,
                            y,
                            hpr,
                            vpr,
                        },
                    });
                }
            }
            if order.kind == crate::OrderKind::StopLimit {
                if let Some(stop_price) = order.stop_price.filter(|price| *price != display_price) {
                    if let Some(stop_y) =
                        self.trading_price_coordinate(pane_index, order.price_scale, stop_price)
                    {
                        lines.push(Prim::HLine {
                            y: (stop_y * vpr).round() as i32,
                            x0: (self.trading_marker_start() * hpr).round() as i32,
                            x1: (self.pane_w * hpr).round() as i32,
                            width: min_line_width,
                            style: LineStyle::Dotted,
                            color,
                        });
                        let trigger =
                            format!("Trigger {}", self.format_trading_quantity(remaining));
                        self.push_trading_segment(
                            lines,
                            TradingControlSegment {
                                kind: TradingControlSegmentKind::OrderType,
                                text: &trigger,
                                width: 92.0,
                                color,
                                filled: false,
                            },
                            TradingControlFeedback::Idle,
                            TradingChipLayout {
                                x: self.trading_marker_start(),
                                y: stop_y,
                                hpr,
                                vpr,
                            },
                        );
                    }
                }
            }
        }

        if let Some(preview) = self
            .trading_state
            .interaction
            .preview()
            .filter(|preview| preview.pane_index == pane_index)
        {
            if let Some(preview_y) =
                self.trading_price_coordinate(pane_index, preview.price_scale, preview.price)
            {
                if let Some((anchor_price, long)) = self.trading_preview_relation(preview) {
                    if let Some(anchor_y) =
                        self.trading_price_coordinate(pane_index, preview.price_scale, anchor_price)
                    {
                        let valid = match (long, preview.role) {
                            (true, OrderRole::TakeProfit) => preview.price > anchor_price,
                            (true, OrderRole::StopLoss) => preview.price < anchor_price,
                            (false, OrderRole::TakeProfit) => preview.price < anchor_price,
                            (false, OrderRole::StopLoss) => preview.price > anchor_price,
                            (_, OrderRole::Working) => false,
                        };
                        if valid {
                            let top = anchor_y.min(preview_y).max(pane.top);
                            let bottom = anchor_y.max(preview_y).min(pane.top + pane.height);
                            if bottom > top {
                                let fill = if preview.role == OrderRole::TakeProfit {
                                    self.trading_state.style.profit
                                } else {
                                    self.trading_state.style.risk
                                };
                                regions.push(Prim::Rect {
                                    rect: IRect {
                                        x: 0,
                                        y: (top * vpr).round() as i32,
                                        w: width,
                                        h: ((bottom - top) * vpr).round().max(1.0) as i32,
                                    },
                                    color: Color::rgba(fill.r(), fill.g(), fill.b(), 32),
                                });
                            }
                        }
                        if (anchor_y - preview_y).abs() > 0.5 {
                            let connector = self.trading_state.style.position;
                            let x = ((self.pane_w - 8.0) * hpr).round() as i32;
                            lines.push(Prim::VLine {
                                x,
                                y0: (anchor_y.min(preview_y) * vpr).round() as i32,
                                y1: (anchor_y.max(preview_y) * vpr).round() as i32,
                                width: min_line_width,
                                style: LineStyle::Solid,
                                color: connector,
                            });
                            for cy in [anchor_y, preview_y] {
                                lines.push(Prim::Circle {
                                    cx: x as f32,
                                    cy: (cy * vpr) as f32,
                                    radius: (3.0 * vpr) as f32,
                                    fill: self.trading_chip_background(),
                                    stroke_width: (1.0 * vpr) as f32,
                                    stroke: connector,
                                });
                            }
                        }
                    }
                }
            }
        }

        if let TradingGroupVisualState::Active(group) = &self.trading_state.group_visual {
            let mut top = f64::INFINITY;
            let mut bottom = f64::NEG_INFINITY;
            let mut member_count = 0usize;
            let mut include = |y: f64| {
                top = top.min(y);
                bottom = bottom.max(y);
                member_count += 1;
            };
            for position in &self.trading_state.positions {
                if position.pane_index == pane_index
                    && self.trading_group_contains_position(group, position)
                {
                    if let Some(y) = self.trading_price_coordinate(
                        pane_index,
                        position.price_scale,
                        position.average_price,
                    ) {
                        include(y);
                    }
                }
            }
            for order in &self.trading_state.orders {
                if order.pane_index == pane_index && self.trading_group_contains_order(group, order)
                {
                    if let Some(y) = self.trading_price_coordinate(
                        pane_index,
                        order.price_scale,
                        self.trading_effective_order_price(order),
                    ) {
                        include(y);
                    }
                }
            }
            if member_count >= 2 && bottom - top > 0.5 {
                let connector_x = ((self.pane_w - 8.0) * hpr).round() as i32;
                let connector_color = self.trading_state.style.position;
                lines.push(Prim::VLine {
                    x: connector_x,
                    y0: (top * vpr).round() as i32,
                    y1: (bottom * vpr).round() as i32,
                    width: min_line_width,
                    style: LineStyle::Solid,
                    color: connector_color,
                });
                for position in &self.trading_state.positions {
                    if position.pane_index == pane_index
                        && self.trading_group_contains_position(group, position)
                    {
                        if let Some(y) = self.trading_price_coordinate(
                            pane_index,
                            position.price_scale,
                            position.average_price,
                        ) {
                            self.push_trading_endpoint(lines, y, connector_color, hpr, vpr);
                        }
                    }
                }
                for order in &self.trading_state.orders {
                    if order.pane_index == pane_index
                        && self.trading_group_contains_order(group, order)
                    {
                        if let Some(y) = self.trading_price_coordinate(
                            pane_index,
                            order.price_scale,
                            self.trading_effective_order_price(order),
                        ) {
                            self.push_trading_endpoint(lines, y, connector_color, hpr, vpr);
                        }
                    }
                }
            }
        }

        let times = self.data.merged_times();
        for execution in &self.trading_state.executions {
            if execution.pane_index != pane_index || times.is_empty() {
                continue;
            }
            let logical = match times.binary_search(&execution.time) {
                Ok(index) => index,
                Err(index) if index < times.len() => index,
                Err(_) => times.len() - 1,
            } as i64;
            let x = self.time_scale.index_to_coordinate(logical);
            let Some(y) =
                self.trading_price_coordinate(pane_index, execution.price_scale, execution.price)
            else {
                continue;
            };
            let color = match execution.side {
                OrderSide::Buy => self.trading_state.style.buy,
                OrderSide::Sell => self.trading_state.style.sell,
            };
            let hovered = self.trading_state.feedback_hover.as_ref().is_some_and(|hit| {
                matches!(&hit.object, crate::TradingObjectId::Execution(id) if id == &execution.id)
            });
            lines.push(Prim::Circle {
                cx: (x * hpr) as f32,
                cy: (y * vpr) as f32,
                radius: ((if hovered { 10.0 } else { 8.0 }) * vpr) as f32,
                fill: color,
                stroke_width: (1.0 * vpr) as f32,
                stroke: color.contrast_text(),
            });
            lines.push(Prim::Text {
                x: (x * hpr) as f32,
                y: (y * vpr) as f32,
                text: match (execution.side, execution.kind) {
                    (OrderSide::Buy, _) => "B",
                    (OrderSide::Sell, _) => "S",
                }
                .to_string(),
                color: color.contrast_text(),
                size: (10.0 * vpr) as f32,
                family: self.options.get().layout.font_family.clone(),
                align: TextAlign::Center,
                weight: if execution.kind == ExecutionKind::PartialFill {
                    400
                } else {
                    700
                },
                italic: false,
            });
        }
        if let Some(tooltip) = tooltip {
            self.push_trading_tooltip(lines, tooltip.text, pane, tooltip.color, tooltip.layout);
        }
    }
}
