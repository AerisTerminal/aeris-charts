use super::*;
use crate::trading::{ExecutionKind, OrderRole, OrderSide, OrderStatus, PositionSide};

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
    fn trading_scale_base(&self, pane_index: usize, target: PriceScaleTarget) -> f64 {
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

    pub(crate) fn trading_price_coordinate(
        &self,
        pane_index: usize,
        target: crate::TradingPriceScale,
        price: f64,
    ) -> Option<f64> {
        let pane = self.panes.get(pane_index)?;
        let target = PriceScaleTarget::from(target);
        let scale = pane_scale(pane, target);
        if scale.is_empty() {
            return None;
        }
        let base = self.trading_scale_base(pane_index, target);
        Some(scale.price_to_coordinate(price, base))
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
        Some(scale.coordinate_to_price(y, self.trading_scale_base(pane_index, target)))
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
        self.trading_state.preview.as_ref().filter(|preview| {
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

    pub(crate) fn indicative_order_pnl(
        &self,
        order: &crate::WorkingOrder,
        price: f64,
    ) -> Option<f64> {
        let position = self
            .trading_state
            .positions
            .iter()
            .find(|position| order.position_id.as_ref() == Some(&position.id))?;
        let point_value = self.trading_state.instrument.point_value?;
        let direction = match position.side {
            PositionSide::Long => 1.0,
            PositionSide::Short => -1.0,
        };
        Some((price - position.average_price) * position.quantity * point_value * direction)
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

    fn trading_label_width(&self, text: &str) -> f64 {
        self.text_measure_fn
            .as_ref()
            .map(|measure| {
                measure(
                    text,
                    self.options.get().layout.font_size,
                    &self.options.get().layout.font_family,
                    600,
                    false,
                )
            })
            .unwrap_or(text.chars().count() as f64 * self.options.get().layout.font_size * 0.58)
            + 12.0
    }

    fn push_trading_label(
        &self,
        out: &mut Vec<Prim>,
        text: String,
        y: f64,
        color: Color,
        hpr: f64,
        vpr: f64,
    ) {
        let font_size = self.options.get().layout.font_size;
        let width = self.trading_label_width(&text).min(self.pane_w.max(1.0));
        let height = font_size + 8.0;
        let x = (self.pane_w - width - 6.0).max(0.0);
        out.push(Prim::RoundRect {
            x: (x * hpr) as f32,
            y: ((y - height / 2.0) * vpr) as f32,
            w: (width * hpr) as f32,
            h: (height * vpr) as f32,
            radii: [3.0 * vpr as f32; 4],
            fill: color.solid(),
            border_width: 0.0,
            border_color: color,
        });
        out.push(Prim::Text {
            x: ((x + 6.0) * hpr) as f32,
            y: (y * vpr) as f32,
            text,
            color: color.contrast_text(),
            size: (font_size * vpr) as f32,
            family: self.options.get().layout.font_family.clone(),
            align: TextAlign::Left,
            weight: 600,
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

        for order in &self.trading_state.orders {
            if order.pane_index != pane_index {
                continue;
            }
            let Some(position) = self
                .trading_state
                .positions
                .iter()
                .find(|position| order.position_id.as_ref() == Some(&position.id))
            else {
                continue;
            };
            let Some(entry_y) = self.trading_price_coordinate(
                pane_index,
                position.price_scale,
                position.average_price,
            ) else {
                continue;
            };
            let display_price = self.trading_effective_order_price(order);
            let Some(order_y) =
                self.trading_price_coordinate(pane_index, order.price_scale, display_price)
            else {
                continue;
            };
            let valid_region = match (position.side, order.role) {
                (PositionSide::Long, OrderRole::TakeProfit) => {
                    display_price > position.average_price
                }
                (PositionSide::Long, OrderRole::StopLoss) => display_price < position.average_price,
                (PositionSide::Short, OrderRole::TakeProfit) => {
                    display_price < position.average_price
                }
                (PositionSide::Short, OrderRole::StopLoss) => {
                    display_price > position.average_price
                }
                (_, OrderRole::Working) => false,
            };
            if valid_region {
                let top = entry_y.min(order_y).max(pane.top);
                let bottom = entry_y.max(order_y).min(pane.top + pane.height);
                if bottom > top {
                    let color = match order.role {
                        OrderRole::TakeProfit => {
                            let color = self.trading_state.style.profit;
                            Color::rgba(color.r(), color.g(), color.b(), 32)
                        }
                        OrderRole::StopLoss => {
                            let color = self.trading_state.style.risk;
                            Color::rgba(color.r(), color.g(), color.b(), 32)
                        }
                        OrderRole::Working => continue,
                    };
                    regions.push(Prim::Rect {
                        rect: IRect {
                            x: 0,
                            y: (top * vpr).round() as i32,
                            w: width,
                            h: ((bottom - top) * vpr).round().max(1.0) as i32,
                        },
                        color,
                    });
                }
            }
        }

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
            let hovered = self.trading_state.hover.as_ref().is_some_and(|hit| {
                matches!(&hit.object, crate::TradingObjectId::Position(id) if id == &position.id)
            });
            let pending = self
                .trading_state
                .pending_position_action
                .as_ref()
                .is_some_and(|action| action.position_id == position.id);
            let position_color = if pending {
                self.trading_state.style.pending
            } else {
                self.trading_state.style.position
            };
            lines.push(Prim::HLine {
                y: (y * vpr).round() as i32,
                x0: 0,
                x1: width,
                width: if hovered { 3 } else { min_line_width.max(2) },
                style: LineStyle::Solid,
                color: position_color,
            });
            let mut text = self.format_trading_quantity(position.quantity);
            if let Some(pnl) = position.display_pnl {
                text.push(' ');
                text.push_str(&self.trading_pnl_text(pnl, position.currency.as_deref()));
            }
            text.push_str("  SL  TP  ×");
            self.push_trading_label(lines, text, y, position_color, hpr, vpr);
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
            let color = if preview
                .is_some_and(|preview| preview.phase == crate::TradingPreviewPhase::Pending)
            {
                self.trading_state.style.pending
            } else {
                trading_order_color(
                    &self.trading_state.style,
                    order.role,
                    order.side,
                    order.status,
                )
            };
            let hovered = self.trading_state.hover.as_ref().is_some_and(
                |hit| matches!(&hit.object, crate::TradingObjectId::Order(id) if id == &order.id),
            );
            lines.push(Prim::HLine {
                y: (y * vpr).round() as i32,
                x0: 0,
                x1: width,
                width: if hovered {
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
                    LineStyle::Dashed
                } else {
                    LineStyle::Solid
                },
                color,
            });
            let role = match order.role {
                OrderRole::StopLoss => "SL",
                OrderRole::TakeProfit => "TP",
                OrderRole::Working => match order.kind {
                    crate::OrderKind::Limit => "LIMIT",
                    crate::OrderKind::Stop => "STOP",
                    crate::OrderKind::StopLimit => "STOP LIMIT",
                },
            };
            let remaining = (order.quantity - order.filled_quantity).max(0.0);
            let mut text = format!("{role} {}", self.format_trading_quantity(remaining));
            if order.filled_quantity > 0.0 {
                text.push_str(&format!(
                    " ({} filled)",
                    self.format_trading_quantity(order.filled_quantity)
                ));
            }
            if let Some(pnl) = self.indicative_order_pnl(order, display_price) {
                text.push(' ');
                text.push_str(&self.trading_pnl_text(pnl, None));
            }
            text.push_str("  ×");
            self.push_trading_label(lines, text, y, color, hpr, vpr);
            if order.kind == crate::OrderKind::StopLimit {
                if let Some(stop_price) = order.stop_price.filter(|price| *price != display_price) {
                    if let Some(stop_y) =
                        self.trading_price_coordinate(pane_index, order.price_scale, stop_price)
                    {
                        lines.push(Prim::HLine {
                            y: (stop_y * vpr).round() as i32,
                            x0: 0,
                            x1: width,
                            width: min_line_width,
                            style: LineStyle::Dashed,
                            color,
                        });
                        self.push_trading_label(
                            lines,
                            format!("TRIGGER {}", self.format_trading_quantity(remaining)),
                            stop_y,
                            color,
                            hpr,
                            vpr,
                        );
                    }
                }
            }
        }

        if let Some(preview) = self.trading_state.preview.as_ref().filter(|preview| {
            preview.pane_index == pane_index
                && !matches!(preview.source, crate::TradingPreviewSource::Order { .. })
        }) {
            let position_id = match &preview.source {
                crate::TradingPreviewSource::StopLoss { position_id }
                | crate::TradingPreviewSource::TakeProfit { position_id } => position_id,
                crate::TradingPreviewSource::Order { .. } => unreachable!(),
            };
            if let Some(position) = self
                .trading_state
                .positions
                .iter()
                .find(|position| &position.id == position_id)
            {
                if let (Some(entry_y), Some(preview_y)) = (
                    self.trading_price_coordinate(
                        pane_index,
                        position.price_scale,
                        position.average_price,
                    ),
                    self.trading_price_coordinate(pane_index, preview.price_scale, preview.price),
                ) {
                    let valid = match (position.side, preview.role) {
                        (PositionSide::Long, OrderRole::TakeProfit) => {
                            preview.price > position.average_price
                        }
                        (PositionSide::Long, OrderRole::StopLoss) => {
                            preview.price < position.average_price
                        }
                        (PositionSide::Short, OrderRole::TakeProfit) => {
                            preview.price < position.average_price
                        }
                        (PositionSide::Short, OrderRole::StopLoss) => {
                            preview.price > position.average_price
                        }
                        (_, OrderRole::Working) => false,
                    };
                    let semantic = if preview.role == OrderRole::TakeProfit {
                        self.trading_state.style.profit
                    } else {
                        self.trading_state.style.risk
                    };
                    if valid {
                        let top = entry_y.min(preview_y).max(pane.top);
                        let bottom = entry_y.max(preview_y).min(pane.top + pane.height);
                        if bottom > top {
                            regions.push(Prim::Rect {
                                rect: IRect {
                                    x: 0,
                                    y: (top * vpr).round() as i32,
                                    w: width,
                                    h: ((bottom - top) * vpr).round().max(1.0) as i32,
                                },
                                color: Color::rgba(semantic.r(), semantic.g(), semantic.b(), 32),
                            });
                        }
                    }
                    let color = if preview.phase == crate::TradingPreviewPhase::Pending {
                        self.trading_state.style.pending
                    } else {
                        semantic
                    };
                    lines.push(Prim::HLine {
                        y: (preview_y * vpr).round() as i32,
                        x0: 0,
                        x1: width,
                        width: min_line_width,
                        style: LineStyle::Dashed,
                        color,
                    });
                    let role = if preview.role == OrderRole::TakeProfit {
                        "TP"
                    } else {
                        "SL"
                    };
                    let phase = if preview.phase == crate::TradingPreviewPhase::Pending {
                        "PENDING"
                    } else {
                        "PREVIEW"
                    };
                    self.push_trading_label(
                        lines,
                        format!(
                            "{role} {} {phase}",
                            self.format_trading_quantity(preview.quantity)
                        ),
                        preview_y,
                        color,
                        hpr,
                        vpr,
                    );
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
            let hovered = self.trading_state.hover.as_ref().is_some_and(|hit| {
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
    }
}
