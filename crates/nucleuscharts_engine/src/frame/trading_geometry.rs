use super::*;
use crate::trading::{ExecutionKind, OrderRole, OrderSide, OrderStatus};

#[derive(Clone, Copy)]
struct TradingChipLayout {
    x: f64,
    y: f64,
    hpr: f64,
    vpr: f64,
}

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

    fn trading_chip_start_at(&self, total_width: f64, anchor: f64) -> f64 {
        (self.pane_w * anchor - total_width / 2.0)
            .clamp(6.0, (self.pane_w - total_width - 16.0).max(6.0))
    }

    fn trading_chip_start(&self, total_width: f64) -> f64 {
        self.trading_chip_start_at(total_width, 0.5)
    }

    fn trading_segment_total(widths: &[f64]) -> f64 {
        widths.iter().sum::<f64>() + if widths.len() == 5 { 4.0 } else { 0.0 }
    }

    fn trading_segment_gap(width_count: usize, index: usize) -> f64 {
        if width_count == 5 && index < 2 {
            2.0
        } else {
            0.0
        }
    }

    fn trading_order_segment_widths(role: OrderRole) -> &'static [f64] {
        if role == OrderRole::Working {
            &[30.0, 30.0, 44.0, 92.0, 28.0]
        } else {
            &[32.0, 44.0, 28.0]
        }
    }

    fn trading_position_segment_widths() -> &'static [f64] {
        &[30.0, 30.0, 44.0, 84.0, 28.0]
    }

    fn trading_preview_prefix_width(preview: &crate::TradingPreview) -> f64 {
        match preview.phase {
            crate::TradingPreviewPhase::Dragging => 46.0 + 6.0,
            crate::TradingPreviewPhase::AwaitingConfirmation => 58.0 + 2.0 + 60.0 + 6.0,
            crate::TradingPreviewPhase::Pending => 0.0,
        }
    }

    pub(crate) fn trading_order_chip_start(&self, order: &crate::WorkingOrder) -> f64 {
        self.trading_chip_start_at(
            Self::trading_segment_total(Self::trading_order_segment_widths(order.role)),
            if order.role == OrderRole::Working {
                0.44
            } else {
                0.30
            },
        )
    }

    pub(crate) fn trading_position_chip_start(&self) -> f64 {
        self.trading_chip_start_at(
            Self::trading_segment_total(Self::trading_position_segment_widths()),
            0.75,
        )
    }

    pub(crate) fn trading_preview_chip_start(&self, preview: &crate::TradingPreview) -> f64 {
        let widths = Self::trading_order_segment_widths(preview.role);
        let main = Self::trading_segment_total(widths);
        self.trading_chip_start(main + Self::trading_preview_prefix_width(preview))
            + Self::trading_preview_prefix_width(preview)
    }

    pub(crate) fn trading_order_chip_hit(
        &self,
        order: &crate::WorkingOrder,
        x: f64,
    ) -> crate::TradingHitKind {
        let widths = Self::trading_order_segment_widths(order.role);
        let start = self.trading_order_chip_start(order);
        let mut cursor = start;
        for (index, width) in widths.iter().copied().enumerate() {
            if x >= cursor && x <= cursor + width {
                return match (order.role, index) {
                    (OrderRole::Working, 0) => crate::TradingHitKind::CreateTargetButton,
                    (OrderRole::Working, 1) => crate::TradingHitKind::CreateStopButton,
                    (_, index) if index + 1 == widths.len() => crate::TradingHitKind::CancelButton,
                    _ => crate::TradingHitKind::OrderLine,
                };
            }
            cursor += width + Self::trading_segment_gap(widths.len(), index);
        }
        crate::TradingHitKind::OrderLine
    }

    pub(crate) fn trading_position_chip_hit(&self, x: f64) -> crate::TradingHitKind {
        let widths = Self::trading_position_segment_widths();
        let mut cursor = self.trading_position_chip_start();
        for (index, width) in widths.iter().copied().enumerate() {
            if x >= cursor && x <= cursor + width {
                return match index {
                    0 => crate::TradingHitKind::CreateTargetButton,
                    1 => crate::TradingHitKind::CreateStopButton,
                    2 | 3 => crate::TradingHitKind::QuantityLabel,
                    _ => crate::TradingHitKind::CancelButton,
                };
            }
            cursor += width + Self::trading_segment_gap(widths.len(), index);
        }
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
        text: &str,
        width: f64,
        color: Color,
        filled: bool,
        layout: TradingChipLayout,
    ) {
        let TradingChipLayout { x, y, hpr, vpr } = layout;
        let font_size = self.options.get().layout.font_size;
        let height = font_size + 5.0;
        let bx = (x * hpr) as f32;
        let by = ((y - height / 2.0) * vpr) as f32;
        let bw = (width * hpr) as f32;
        let bh = (height * vpr) as f32;
        let radius = 3.0 * vpr as f32;
        out.push(Prim::RoundRect {
            x: bx,
            y: by,
            w: bw,
            h: bh,
            radii: [radius; 4],
            fill: color.solid(),
            border_width: 0.0,
            border_color: color,
        });
        if !filled {
            let inset_x = hpr.max(1.0) as f32;
            let inset_y = vpr.max(1.0) as f32;
            out.push(Prim::RoundRect {
                x: bx + inset_x,
                y: by + inset_y,
                w: (bw - inset_x * 2.0).max(0.0),
                h: (bh - inset_y * 2.0).max(0.0),
                radii: [(radius - inset_y).max(0.0); 4],
                fill: self.trading_chip_background(),
                border_width: 0.0,
                border_color: color,
            });
        }
        out.push(Prim::Text {
            x: ((x + width / 2.0) * hpr) as f32,
            y: (y * vpr) as f32,
            text: text.to_string(),
            color: if filled { color.contrast_text() } else { color },
            size: (font_size * vpr) as f32,
            family: self.options.get().layout.font_family.clone(),
            align: TextAlign::Center,
            weight: 500,
            italic: false,
        });
    }

    fn push_trading_segments(
        &self,
        out: &mut Vec<Prim>,
        labels: &[(&str, Color)],
        widths: &[f64],
        layout: TradingChipLayout,
    ) {
        let joined_from = if labels.len() == 5 { 2 } else { 0 };
        let mut x = layout.x;
        for (index, ((text, color), width)) in labels
            .iter()
            .zip(widths.iter().copied())
            .take(joined_from)
            .enumerate()
        {
            self.push_trading_segment(
                out,
                text,
                width,
                *color,
                false,
                TradingChipLayout { x, ..layout },
            );
            x += width + Self::trading_segment_gap(widths.len(), index);
        }
        self.push_joined_trading_segments(
            out,
            &labels[joined_from..],
            &widths[joined_from..],
            joined_from > 0,
            TradingChipLayout { x, ..layout },
        );
    }

    fn push_joined_trading_segments(
        &self,
        out: &mut Vec<Prim>,
        labels: &[(&str, Color)],
        widths: &[f64],
        fill_first: bool,
        layout: TradingChipLayout,
    ) {
        let TradingChipLayout { x, y, hpr, vpr } = layout;
        let font_size = self.options.get().layout.font_size;
        let height = font_size + 5.0;
        let total_width = widths.iter().sum::<f64>();
        let transparent = Color::rgba(0, 0, 0, 0);
        let border_color = labels.first().map_or(transparent, |label| label.1);
        let inset_x = hpr.max(1.0) as f32;
        let inset_y = vpr.max(1.0) as f32;
        let radius = 3.0 * vpr as f32;
        let rect = |fill, border_width| Prim::RoundRect {
            x: (x * hpr) as f32,
            y: ((y - height / 2.0) * vpr) as f32,
            w: (total_width * hpr) as f32,
            h: (height * vpr) as f32,
            radii: [radius; 4],
            fill,
            border_width,
            border_color,
        };
        out.push(rect(border_color.solid(), 0.0));
        out.push(Prim::RoundRect {
            x: (x * hpr) as f32 + inset_x,
            y: ((y - height / 2.0) * vpr) as f32 + inset_y,
            w: ((total_width * hpr) as f32 - inset_x * 2.0).max(0.0),
            h: ((height * vpr) as f32 - inset_y * 2.0).max(0.0),
            radii: [(radius - inset_y).max(0.0); 4],
            fill: self.trading_chip_background(),
            border_width: 0.0,
            border_color,
        });
        if fill_first {
            out.push(Prim::RoundRect {
                x: (x * hpr) as f32,
                y: ((y - height / 2.0) * vpr) as f32,
                w: (widths[0] * hpr) as f32,
                h: (height * vpr) as f32,
                radii: [3.0 * vpr as f32, 0.0, 0.0, 3.0 * vpr as f32],
                fill: labels[0].1.solid(),
                border_width: 0.0,
                border_color,
            });
        }
        let mut cursor = x;
        for (index, ((text, color), width)) in labels.iter().zip(widths.iter().copied()).enumerate()
        {
            if index > 0 {
                out.push(Prim::VLine {
                    x: (cursor * hpr).round() as i32,
                    y0: ((y - height / 2.0) * vpr).round() as i32,
                    y1: ((y + height / 2.0) * vpr).round() as i32,
                    width: vpr.max(1.0) as i32,
                    style: LineStyle::Solid,
                    color: border_color,
                });
            }
            out.push(Prim::Text {
                x: ((cursor + width / 2.0) * hpr) as f32,
                y: (y * vpr) as f32,
                text: (*text).to_string(),
                color: if fill_first && index == 0 {
                    color.contrast_text()
                } else {
                    *color
                },
                size: (font_size * vpr) as f32,
                family: self.options.get().layout.font_family.clone(),
                align: TextAlign::Center,
                weight: 500,
                italic: false,
            });
            cursor += width;
        }
        out.push(rect(transparent, 0.0));
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
            let hovered = self.trading_state.interaction.hover().is_some_and(|hit| {
                matches!(&hit.object, crate::TradingObjectId::Position(id) if id == &position.id)
            });
            let pending =
                self.trading_state.interaction.pending_position_id() == Some(&position.id);
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
            let quantity = self.format_trading_quantity(position.quantity);
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
            let labels = [
                ("TP", self.trading_state.style.take_profit),
                ("SL", self.trading_state.style.stop_loss),
                (quantity.as_str(), position_color),
                (pnl.as_str(), pnl_color),
                ("×", position_color),
            ];
            self.push_trading_segments(
                lines,
                &labels,
                Self::trading_position_segment_widths(),
                TradingChipLayout {
                    x: self.trading_position_chip_start(),
                    y,
                    hpr,
                    vpr,
                },
            );
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
            let hovered = self.trading_state.interaction.hover().is_some_and(
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
                    LineStyle::Dotted
                } else {
                    LineStyle::Solid
                },
                color,
            });
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
            if order.role == OrderRole::Working || hovered || preview.is_some() {
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
                let role = match order.role {
                    OrderRole::StopLoss => "SL",
                    OrderRole::TakeProfit => "TP",
                    OrderRole::Working => "",
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
                                side,
                                46.0,
                                base_color,
                                true,
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
                                "Discard",
                                58.0,
                                self.trading_state.style.rejected,
                                false,
                                TradingChipLayout {
                                    x: main_x - 126.0,
                                    y,
                                    hpr,
                                    vpr,
                                },
                            );
                            self.push_trading_segment(
                                lines,
                                "Confirm",
                                60.0,
                                self.trading_state.style.position,
                                true,
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
                if order.role == OrderRole::Working {
                    let labels = [
                        ("TP", self.trading_state.style.take_profit),
                        ("SL", self.trading_state.style.stop_loss),
                        (quantity.as_str(), color),
                        (descriptor.as_str(), color),
                        ("×", color),
                    ];
                    self.push_trading_segments(
                        lines,
                        &labels,
                        Self::trading_order_segment_widths(order.role),
                        TradingChipLayout {
                            x: main_x,
                            y,
                            hpr,
                            vpr,
                        },
                    );
                } else {
                    let labels = [(role, color), (quantity.as_str(), color), ("×", color)];
                    self.push_trading_segments(
                        lines,
                        &labels,
                        Self::trading_order_segment_widths(order.role),
                        TradingChipLayout {
                            x: main_x,
                            y,
                            hpr,
                            vpr,
                        },
                    );
                }
            }
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
                            style: LineStyle::Dotted,
                            color,
                        });
                        let trigger =
                            format!("Trigger {}", self.format_trading_quantity(remaining));
                        self.push_trading_segment(
                            lines,
                            &trigger,
                            92.0,
                            color,
                            false,
                            TradingChipLayout {
                                x: self.trading_chip_start(92.0),
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
                let semantic = if preview.role == OrderRole::TakeProfit {
                    self.trading_state.style.take_profit
                } else if preview.role == OrderRole::StopLoss {
                    self.trading_state.style.stop_loss
                } else {
                    match preview.side {
                        OrderSide::Buy => self.trading_state.style.buy,
                        OrderSide::Sell => self.trading_state.style.sell,
                    }
                };
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
                            let x = ((self.pane_w - 18.0) * hpr).round() as i32;
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

                if !matches!(preview.source, crate::TradingPreviewSource::Order { .. }) {
                    let color = match preview.phase {
                        crate::TradingPreviewPhase::Pending => self.trading_state.style.pending,
                        crate::TradingPreviewPhase::Dragging
                        | crate::TradingPreviewPhase::AwaitingConfirmation => {
                            Color::rgba(semantic.r(), semantic.g(), semantic.b(), 176)
                        }
                    };
                    lines.push(Prim::HLine {
                        y: (preview_y * vpr).round() as i32,
                        x0: 0,
                        x1: width,
                        width: min_line_width,
                        style: LineStyle::Dotted,
                        color,
                    });
                    let main_x = self.trading_preview_chip_start(preview);
                    match preview.phase {
                        crate::TradingPreviewPhase::Dragging => {
                            self.push_trading_segment(
                                lines,
                                if preview.side == OrderSide::Buy {
                                    "Buy"
                                } else {
                                    "Sell"
                                },
                                46.0,
                                semantic,
                                true,
                                TradingChipLayout {
                                    x: main_x - 52.0,
                                    y: preview_y,
                                    hpr,
                                    vpr,
                                },
                            );
                        }
                        crate::TradingPreviewPhase::AwaitingConfirmation => {
                            self.push_trading_segment(
                                lines,
                                "Discard",
                                58.0,
                                self.trading_state.style.rejected,
                                false,
                                TradingChipLayout {
                                    x: main_x - 126.0,
                                    y: preview_y,
                                    hpr,
                                    vpr,
                                },
                            );
                            self.push_trading_segment(
                                lines,
                                "Confirm",
                                60.0,
                                self.trading_state.style.position,
                                true,
                                TradingChipLayout {
                                    x: main_x - 66.0,
                                    y: preview_y,
                                    hpr,
                                    vpr,
                                },
                            );
                        }
                        crate::TradingPreviewPhase::Pending => {}
                    }
                    let quantity = self.format_trading_quantity(preview.quantity);
                    let role = if preview.role == OrderRole::TakeProfit {
                        "TP"
                    } else {
                        "SL"
                    };
                    let labels = [(role, color), (quantity.as_str(), color), ("×", color)];
                    self.push_trading_segments(
                        lines,
                        &labels,
                        Self::trading_order_segment_widths(preview.role),
                        TradingChipLayout {
                            x: main_x,
                            y: preview_y,
                            hpr,
                            vpr,
                        },
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
            let hovered = self.trading_state.interaction.hover().is_some_and(|hit| {
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
