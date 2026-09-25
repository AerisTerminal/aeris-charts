use super::*;
use crate::trading::{
    ExecutionKind, OrderRole, OrderSide, OrderStatus, PositionSide, TradingGroupVisualState,
};
use crate::Pane;
use aeris_charts_core::style::RADIUS_SMALL;

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
    layout: TradingChipLayout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TradingControlSegmentKind {
    Quantity,
    Pnl,
    OrderType,
    Cancel,
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

/// One order or position marker: the readout cells (quantity, then PnL or order type) inside a
/// single outlined chip, followed after a gap by the detached close chip. A trailing `Cancel`
/// segment is that close chip; every other segment is a readout cell.
struct TradingControlCluster<'a> {
    segments: &'a [TradingControlSegment<'a>],
    left: f64,
    color: Color,
}

struct TradingTriggerLine {
    pane_index: usize,
    price_scale: crate::TradingPriceScale,
    trigger_price: Option<f64>,
    display_price: f64,
    color: Color,
}

impl<'a> TradingControlCluster<'a> {
    fn start(&self) -> f64 {
        self.left
    }

    fn close(&self) -> Option<&TradingControlSegment<'a>> {
        self.segments
            .last()
            .filter(|segment| segment.kind == TradingControlSegmentKind::Cancel)
    }

    fn body(&self) -> &'a [TradingControlSegment<'a>] {
        let body = self.segments.len() - usize::from(self.close().is_some());
        &self.segments[..body]
    }

    /// Width of the readout chip alone, excluding the gap and the detached close chip.
    fn body_width(&self) -> f64 {
        self.body().iter().map(|segment| segment.width).sum()
    }

    /// Width of the whole marker, gap included — what the hit test measures against.
    fn width(&self) -> f64 {
        self.body_width() + self.close().map_or(0.0, |close| CONTROL_GAP + close.width)
    }
}

const QUANTITY_PAD_X: f64 = 8.0;
const MAX_QUANTITY_WIDTH: f64 = 120.0;
const PNL_WIDTH: f64 = 96.0;
const ORDER_TYPE_WIDTH: f64 = 92.0;
/// Separation between the readout chip and the detached close chip.
const CONTROL_GAP: f64 = 5.0;
const ORDER_MARKER_SPAN: f64 = 304.0;

/// Protection semantics take precedence over their broker-side implementation: an SL remains
/// warning yellow and a TP remains profit green. Ordinary sell orders read bearish red. The
/// neutral working-order accent is reserved for resting buy limits.
pub(crate) fn trading_order_color(
    style: &crate::TradingStyle,
    kind: crate::OrderKind,
    side: OrderSide,
    role: OrderRole,
    status: OrderStatus,
) -> Color {
    let by_side = match side {
        OrderSide::Buy => style.buy,
        OrderSide::Sell => style.sell,
    };
    match status {
        OrderStatus::Rejected | OrderStatus::Cancelled | OrderStatus::Expired => style.rejected,
        OrderStatus::PendingSubmit | OrderStatus::PendingModify | OrderStatus::PendingCancel => {
            style.pending
        }
        _ if role == OrderRole::TakeProfit => style.take_profit,
        _ if role == OrderRole::StopLoss => style.stop_loss,
        OrderStatus::Filled => by_side,
        OrderStatus::Working | OrderStatus::PartiallyFilled => {
            if kind == crate::OrderKind::Limit && side == OrderSide::Buy {
                style.working_order
            } else {
                by_side
            }
        }
    }
}

impl ChartEngine {
    fn trading_control_kind(kind: crate::TradingHitKind) -> Option<TradingControlSegmentKind> {
        match kind {
            crate::TradingHitKind::CancelButton => Some(TradingControlSegmentKind::Cancel),
            _ => None,
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
        let fallback = aeris_charts_core::style::DEFAULT_SURFACE_RGB;
        Color::parse_css(&self.options.get().layout.background.color)
            .unwrap_or(Color::rgb(fallback.0, fallback.1, fallback.2))
            .solid()
    }

    /// Hover and press feedback pre-blended against the chip surface. The tint must stay OPAQUE:
    /// a control sits on top of its own marker line, and a translucent fill would let that line
    /// read straight through the button the pointer is on.
    fn trading_tinted_surface(&self, color: Color, alpha: u16) -> Color {
        let surface = self.trading_chip_background();
        let blend = |surface: u8, foreground: u8| {
            ((u16::from(foreground) * alpha + u16::from(surface) * (255 - alpha) + 127) / 255) as u8
        };
        Color::rgb(
            blend(surface.r(), color.r()),
            blend(surface.g(), color.g()),
            blend(surface.b(), color.b()),
        )
    }

    pub(crate) fn trading_control_height(&self) -> f64 {
        self.options.get().layout.font_size + 5.0
    }

    /// The close chip keeps equal width and height, so its glyph sits on the marker's rhythm.
    pub(crate) fn trading_close_width(&self) -> f64 {
        self.trading_control_height()
    }

    pub(crate) fn trading_marker_end(&self) -> f64 {
        self.pane_w.max(6.0)
    }

    pub(crate) fn trading_marker_start(&self) -> f64 {
        (self.trading_marker_end() - ORDER_MARKER_SPAN).max(6.0)
    }

    /// Quantity cells fit their formatted text instead of reserving a fixed-width box. The upper
    /// bound keeps extreme finite magnitudes from expanding a marker without limit.
    fn trading_quantity_width(&self, text: &str) -> f64 {
        let layout = &self.options.get().layout;
        (self.measure_text_run(text, layout.font_size, &layout.font_family, 400, false)
            + QUANTITY_PAD_X * 2.0)
            .ceil()
            .clamp(self.trading_control_height(), MAX_QUANTITY_WIDTH)
    }

    fn trading_order_quantity_text(&self, order: &crate::WorkingOrder) -> String {
        let remaining = (order.quantity - order.filled_quantity).max(0.0);
        if order.filled_quantity > 0.0 {
            format!(
                "{}/{}",
                self.format_trading_quantity(remaining),
                self.format_trading_quantity(order.quantity)
            )
        } else {
            self.format_trading_quantity(remaining)
        }
    }

    fn trading_position_quantity_text(&self, position: &crate::TradingPosition) -> String {
        let signed_quantity = if position.side == PositionSide::Long {
            position.quantity
        } else {
            -position.quantity
        };
        self.format_trading_quantity(signed_quantity)
    }

    /// Width of the second cell: a working order names itself, a protection order reports the PnL
    /// it would realise.
    fn trading_order_detail_width(order: &crate::WorkingOrder) -> f64 {
        if order.role == OrderRole::Working {
            ORDER_TYPE_WIDTH
        } else {
            PNL_WIDTH
        }
    }

    pub(crate) fn trading_order_cluster_width(&self, order: &crate::WorkingOrder) -> f64 {
        self.trading_quantity_width(&self.trading_order_quantity_text(order))
            + Self::trading_order_detail_width(order)
            + CONTROL_GAP
            + self.trading_close_width()
    }

    pub(crate) fn trading_position_cluster_width(&self, position: &crate::TradingPosition) -> f64 {
        self.trading_quantity_width(&self.trading_position_quantity_text(position))
            + PNL_WIDTH
            + CONTROL_GAP
            + self.trading_close_width()
    }

    pub(crate) fn trading_annotation_hit(
        &self,
        annotations: &[crate::TradingAnnotation],
        line_y: f64,
        x: f64,
        y: f64,
    ) -> Option<String> {
        let height = self.trading_control_height();
        let mut cursor = self.trading_marker_start();
        let visible = annotations.len().min(3);
        for annotation in annotations.iter().take(visible) {
            let width = self.trading_annotation_width(&annotation.text);
            let center_y = match annotation.placement {
                crate::TradingAnnotationPlacement::Above => line_y - height - 2.0,
                crate::TradingAnnotationPlacement::Below => line_y + height + 2.0,
                crate::TradingAnnotationPlacement::Inline => line_y,
            };
            if x >= cursor && x <= cursor + width && (y - center_y).abs() <= height / 2.0 {
                return Some(annotation.id.clone());
            }
            cursor += width + CONTROL_GAP;
        }
        None
    }

    fn trading_annotation_width(&self, text: &str) -> f64 {
        (self.measure_text_run(
            text,
            self.options.get().layout.font_size,
            &self.options.get().layout.font_family,
            400,
            false,
        ) + 10.0)
            .ceil()
            .clamp(24.0, 180.0)
    }

    fn trading_annotation_color(&self, tone: crate::TradingAnnotationTone) -> Color {
        match tone {
            crate::TradingAnnotationTone::Neutral => self.trading_state.style.control,
            crate::TradingAnnotationTone::Info => self.trading_state.style.buy,
            crate::TradingAnnotationTone::Warning => self.trading_state.style.pending,
            crate::TradingAnnotationTone::Danger => self.trading_state.style.risk,
        }
    }

    pub(crate) fn push_trading_annotations(
        &self,
        out: &mut Vec<Prim>,
        annotations: &[crate::TradingAnnotation],
        line_y: f64,
        hpr: f64,
        vpr: f64,
    ) {
        let height = self.trading_control_height();
        let mut cursor = self.trading_marker_start();
        let visible = annotations.len().min(3);
        for annotation in annotations.iter().take(visible) {
            let width = self.trading_annotation_width(&annotation.text);
            let center_y = match annotation.placement {
                crate::TradingAnnotationPlacement::Above => line_y - height - 2.0,
                crate::TradingAnnotationPlacement::Below => line_y + height + 2.0,
                crate::TradingAnnotationPlacement::Inline => line_y,
            };
            let color = self.trading_annotation_color(annotation.tone);
            out.push(Prim::RoundRect {
                x: (cursor * hpr) as f32,
                y: ((center_y - height / 2.0) * vpr) as f32,
                w: (width * hpr) as f32,
                h: (height * vpr) as f32,
                radii: [2.0; 4],
                fill: self.trading_chip_background(),
                border_width: vpr.floor().max(1.0) as f32,
                border_color: color,
            });
            out.push(Prim::Text {
                x: ((cursor + width / 2.0) * hpr) as f32,
                y: (center_y * vpr) as f32,
                text: annotation.text.clone(),
                color,
                size: (self.options.get().layout.font_size * vpr) as f32,
                family: self.options.get().layout.font_family.clone(),
                align: TextAlign::Center,
                weight: 400,
                italic: false,
            });
            cursor += width + CONTROL_GAP;
        }
        if annotations.len() > visible {
            let text = format!("+{}", annotations.len() - visible);
            let width = self.trading_annotation_width(&text);
            out.push(Prim::RoundRect {
                x: (cursor * hpr) as f32,
                y: ((line_y - height / 2.0) * vpr) as f32,
                w: (width * hpr) as f32,
                h: (height * vpr) as f32,
                radii: [2.0; 4],
                fill: self.trading_chip_background(),
                border_width: vpr.floor().max(1.0) as f32,
                border_color: self.trading_state.style.control,
            });
            out.push(Prim::Text {
                x: ((cursor + width / 2.0) * hpr) as f32,
                y: (line_y * vpr) as f32,
                text,
                color: self.trading_state.style.control,
                size: (self.options.get().layout.font_size * vpr) as f32,
                family: self.options.get().layout.font_family.clone(),
                align: TextAlign::Center,
                weight: 400,
                italic: false,
            });
        }
    }

    fn push_host_trigger_line(
        &self,
        lines: &mut Vec<Prim>,
        trigger: TradingTriggerLine,
        hpr: f64,
        vpr: f64,
    ) {
        let Some(trigger_price) = trigger
            .trigger_price
            .filter(|price| *price != trigger.display_price)
        else {
            return;
        };
        let Some(y) =
            self.trading_price_coordinate(trigger.pane_index, trigger.price_scale, trigger_price)
        else {
            return;
        };
        lines.push(Prim::HLine {
            y: (y * vpr).round() as i32,
            x0: (self.trading_marker_start() * hpr).round() as i32,
            x1: (self.pane_w * hpr).round() as i32,
            width: vpr.floor().max(1.0) as i32,
            style: LineStyle::Dotted,
            color: trigger.color,
        });
    }

    fn trading_cluster_hit(
        &self,
        left: f64,
        width: f64,
        x: f64,
        line: crate::TradingHitKind,
    ) -> crate::TradingHitKind {
        let close = self.trading_close_width();
        if x >= left + width - close && x <= left + width {
            crate::TradingHitKind::CancelButton
        } else {
            line
        }
    }

    pub(crate) fn trading_order_chip_hit(
        &self,
        order: &crate::WorkingOrder,
        x: f64,
    ) -> crate::TradingHitKind {
        self.trading_cluster_hit(
            self.trading_marker_start(),
            self.trading_order_cluster_width(order),
            x,
            crate::TradingHitKind::OrderLine,
        )
    }

    pub(crate) fn trading_position_chip_hit(
        &self,
        position: &crate::TradingPosition,
        x: f64,
    ) -> crate::TradingHitKind {
        self.trading_cluster_hit(
            self.trading_marker_start(),
            self.trading_position_cluster_width(position),
            x,
            crate::TradingHitKind::PositionLine,
        )
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
        let radius = (RADIUS_SMALL * hpr.min(vpr)) as f32;
        let fill = match (filled, feedback) {
            (true, TradingControlFeedback::Idle) => color.solid(),
            (true, TradingControlFeedback::Hovered) => color.solid().lighten(0.16),
            (true, TradingControlFeedback::Pressed) => color.solid().darken(0.72),
            (false, TradingControlFeedback::Idle) => self.trading_chip_background(),
            (false, TradingControlFeedback::Hovered) => self.trading_tinted_surface(color, 44),
            (false, TradingControlFeedback::Pressed) => self.trading_tinted_surface(color, 78),
        };
        out.push(Prim::RoundRect {
            x: bx,
            y: by,
            w: bw,
            h: bh,
            radii: [radius; 4],
            fill,
            border_width: vpr.max(1.0) as f32,
            border_color: color,
        });
        out.push(Prim::Text {
            x: ((x + width / 2.0) * hpr) as f32,
            y: (y * vpr) as f32,
            text: text.to_string(),
            color: if filled { color.contrast_text() } else { color },
            size: (font_size * vpr) as f32,
            family: self.options.get().layout.font_family.clone(),
            align: TextAlign::Center,
            weight: 400,
            italic: false,
        });
    }

    /// The close affordance is drawn as two crossing bars rather than a `×` glyph: the marker has
    /// to show it in every embedding, and the host's `font_family` is not guaranteed to cover the
    /// multiplication sign (a missing glyph renders as nothing or as tofu).
    fn push_trading_close_glyph(
        &self,
        out: &mut Vec<Prim>,
        center_x: f64,
        center_y: f64,
        color: Color,
        hpr: f64,
        vpr: f64,
    ) {
        let arm = (self.options.get().layout.font_size * 0.33).max(3.5);
        // A 45° bar offset vertically by `offset` is `offset / √2` thick perpendicular, so this
        // holds the mark at hairline weight — matching the container outline, not a bold glyph.
        let offset = 1.3 * std::f64::consts::FRAC_1_SQRT_2;
        let point = |x: f64, y: f64| [(x * hpr) as f32, (y * vpr) as f32];
        for slope in [1.0_f64, -1.0] {
            let (x0, y0) = (center_x - arm, center_y - arm * slope);
            let (x1, y1) = (center_x + arm, center_y + arm * slope);
            let corners = [
                point(x0, y0 - offset),
                point(x1, y1 - offset),
                point(x1, y1 + offset),
                point(x0, y0 + offset),
            ];
            out.push(Prim::Triangle {
                a: corners[0],
                b: corners[1],
                c: corners[2],
                color,
            });
            out.push(Prim::Triangle {
                a: corners[0],
                b: corners[2],
                c: corners[3],
                color,
            });
        }
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
        let font_size = self.options.get().layout.font_size;
        let height = font_size + 5.0;
        let color = cluster.color;
        let left = cluster.start();
        let top = y - height / 2.0;
        // Hairline outline on the chart's own device-pixel convention. Trading brackets are
        // deliberately square so the readout and detached close control align crisply with their
        // horizontal price rule; tooltips and transient drag labels retain the shared radius.
        let border = vpr.floor().max(1.0) as f32;
        let radius = 0.0;
        let body_width = cluster.body_width();
        let cell_feedback = |kind: TradingControlSegmentKind| {
            if pressed == Some(kind) {
                TradingControlFeedback::Pressed
            } else if hovered == Some(kind) {
                TradingControlFeedback::Hovered
            } else {
                TradingControlFeedback::Idle
            }
        };
        // One outlined chip for the readout cells. Their seam is the edge of the solid quantity
        // block itself: no inner border and no divider hairline, so the chip never reads as one
        // box pasted inside another.
        out.push(Prim::RoundRect {
            x: (left * hpr) as f32,
            y: (top * vpr) as f32,
            w: (body_width * hpr) as f32,
            h: (height * vpr) as f32,
            radii: [radius; 4],
            fill: self.trading_chip_background(),
            border_width: border,
            border_color: color,
        });
        let mut cursor = left;
        let body = cluster.body();
        for (index, segment) in body.iter().enumerate() {
            let feedback = cell_feedback(segment.kind);
            let fill = match (segment.filled, feedback) {
                (true, TradingControlFeedback::Idle) => Some(color.solid()),
                (true, TradingControlFeedback::Hovered) => Some(color.solid().lighten(0.16)),
                (true, TradingControlFeedback::Pressed) => Some(color.solid().darken(0.72)),
                (false, TradingControlFeedback::Idle) => None,
                (false, TradingControlFeedback::Hovered) => {
                    Some(self.trading_tinted_surface(color, 44))
                }
                (false, TradingControlFeedback::Pressed) => {
                    Some(self.trading_tinted_surface(color, 78))
                }
            };
            if let Some(fill) = fill {
                // A filled cell paints flush to the chip's outer edge — over the outline on the
                // sides it touches — so it is part of the chip rather than an inset block.
                out.push(Prim::RoundRect {
                    x: (cursor * hpr) as f32,
                    y: (top * vpr) as f32,
                    w: (segment.width * hpr) as f32,
                    h: (height * vpr) as f32,
                    radii: [
                        if index == 0 { radius } else { 0.0 },
                        if index + 1 == body.len() { radius } else { 0.0 },
                        if index + 1 == body.len() { radius } else { 0.0 },
                        if index == 0 { radius } else { 0.0 },
                    ],
                    fill,
                    border_width: 0.0,
                    border_color: fill,
                });
            }
            out.push(Prim::Text {
                x: ((cursor + segment.width / 2.0) * hpr) as f32,
                y: (y * vpr) as f32,
                text: segment.text.to_string(),
                color: if segment.filled {
                    color.contrast_text()
                } else {
                    segment.color
                },
                size: (font_size * vpr) as f32,
                family: self.options.get().layout.font_family.clone(),
                align: TextAlign::Center,
                weight: 400,
                italic: false,
            });
            cursor += segment.width;
        }
        // The close control is its own chip, separated by a gap: destructive actions do not share
        // an edge with the readout they would destroy.
        if let Some(close) = cluster.close() {
            let feedback = cell_feedback(close.kind);
            let close_left = left + body_width + CONTROL_GAP;
            let fill = match feedback {
                TradingControlFeedback::Idle => self.trading_chip_background(),
                TradingControlFeedback::Hovered => self.trading_tinted_surface(color, 44),
                TradingControlFeedback::Pressed => self.trading_tinted_surface(color, 78),
            };
            out.push(Prim::RoundRect {
                x: (close_left * hpr) as f32,
                y: (top * vpr) as f32,
                w: (close.width * hpr) as f32,
                h: (height * vpr) as f32,
                radii: [radius; 4],
                fill,
                border_width: border,
                border_color: color,
            });
            self.push_trading_close_glyph(
                out,
                close_left + close.width / 2.0,
                y,
                close.color,
                hpr,
                vpr,
            );
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

    /// The chart's own border token, used for chrome that belongs to the surface rather than to a
    /// traded object.
    fn trading_chrome_border(&self) -> Color {
        let options = self.options.get();
        let fallback = aeris_charts_core::style::DEFAULT_BORDER_RGB;
        Color::parse_css(&options.right_price_scale.border_color)
            .or_else(|| Color::parse_css(&options.left_price_scale.border_color))
            .unwrap_or(Color::rgb(fallback.0, fallback.1, fallback.2))
            .solid()
    }

    /// An action tooltip is chart chrome, not part of the object it describes: it follows the
    /// active theme's surface, border, and text tokens rather than the order's buy/sell color, so
    /// it reads the same on every line and in both themes.
    fn push_trading_tooltip(
        &self,
        out: &mut Vec<Prim>,
        text: &str,
        pane: &Pane,
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
        let radius = (RADIUS_SMALL * hpr.min(vpr)) as f32;
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
            radii: [radius; 4],
            fill: self.trading_chip_background(),
            border_width: vpr.floor().max(1.0) as f32,
            border_color: self.trading_chrome_border(),
        });
        out.push(Prim::Text {
            x: ((x + width / 2.0) * hpr) as f32,
            y: ((y + height / 2.0) * vpr) as f32,
            text: text.to_string(),
            color: self.primary_text_color(),
            size: (font_size * vpr) as f32,
            family: self.options.get().layout.font_family.clone(),
            align: TextAlign::Center,
            weight: 400,
            italic: false,
        });
    }

    #[cfg(test)]
    pub(crate) fn build_trading_frame_for_test(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        regions: &mut Vec<Prim>,
        lines: &mut Vec<Prim>,
    ) {
        self.build_trading_frame(pane_index, hpr, vpr, regions, lines);
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

        // Host context is a separate, non-persisted layer. Windows are lowered first and event
        // markers use deterministic LOD collapse when releases share the same pixel column.
        let times = self.data.merged_times();
        if !times.is_empty() {
            let logical = |time: i64| match times.binary_search(&time) {
                Ok(index) => index,
                Err(index) if index < times.len() => index,
                Err(_) => times.len() - 1,
            } as i64;
            for window in &self.trading_state.host_overlay.windows {
                let x0 = self
                    .time_scale
                    .index_to_coordinate(logical(window.start_time));
                let x1 = self
                    .time_scale
                    .index_to_coordinate(logical(window.end_time));
                regions.push(Prim::Rect {
                    rect: IRect {
                        x: (x0.min(x1) * hpr).round() as i32,
                        y: (pane.top * vpr).round() as i32,
                        w: ((x1 - x0).abs() * hpr).round().max(1.0) as i32,
                        h: (pane.height * vpr).round().max(1.0) as i32,
                    },
                    color: Color::rgba(
                        self.trading_state.style.pending.r(),
                        self.trading_state.style.pending.g(),
                        self.trading_state.style.pending.b(),
                        24,
                    ),
                });
            }
            let mut last_event_x = f64::NEG_INFINITY;
            for event in &self.trading_state.host_overlay.events {
                let x = self.time_scale.index_to_coordinate(logical(event.time));
                if x - last_event_x < 8.0 {
                    continue;
                }
                last_event_x = x;
                let color = if event.importance >= 2 {
                    self.trading_state.style.risk
                } else {
                    self.trading_state.style.control
                };
                lines.push(Prim::VLine {
                    x: (x * hpr).round() as i32,
                    y0: (pane.top * vpr).round() as i32,
                    y1: ((pane.top + pane.height) * vpr).round() as i32,
                    width: min_line_width,
                    style: LineStyle::Dotted,
                    color,
                });
                if !event.label.is_empty() {
                    lines.push(Prim::Text {
                        x: (x * hpr) as f32,
                        y: ((pane.top + 12.0) * vpr) as f32,
                        text: event.label.clone(),
                        color,
                        size: (self.options.get().layout.font_size * vpr) as f32,
                        family: self.options.get().layout.font_family.clone(),
                        align: TextAlign::Center,
                        weight: if event.importance >= 2 { 700 } else { 400 },
                        italic: false,
                    });
                }
            }
        }

        for position in &self.trading_state.positions {
            if !self
                .trading_state
                .account_visible(position.account_id.as_ref())
            {
                continue;
            }
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
            let position_color = self.trading_position_color(position.side);
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
            let quantity = self.trading_position_quantity_text(position);
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
                    width: self.trading_quantity_width(&quantity),
                    color: position_color,
                    filled: true,
                },
                // The PnL text keeps its profit/loss tint — the container around it is what
                // carries the position's direction color.
                TradingControlSegment {
                    kind: TradingControlSegmentKind::Pnl,
                    text: pnl.as_str(),
                    width: PNL_WIDTH,
                    color: pnl_color,
                    filled: false,
                },
                TradingControlSegment {
                    kind: TradingControlSegmentKind::Cancel,
                    text: "",
                    width: self.trading_close_width(),
                    color: position_color,
                    filled: false,
                },
            ];
            let cluster = TradingControlCluster {
                segments: &segments,
                left: self.trading_marker_start(),
                color: position_color,
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
            self.push_trading_annotations(lines, &position.annotations, y, hpr, vpr);
            if self.trading_state.tooltip_armed
                && hovered.is_some_and(|hit| hit.kind == crate::TradingHitKind::CancelButton)
            {
                tooltip = Some(TradingTooltip {
                    text: "Close Position",
                    layout: TradingChipLayout {
                        x: cluster.start() + cluster.width() - self.trading_close_width() / 2.0,
                        y,
                        hpr,
                        vpr,
                    },
                });
            }
        }

        for order in &self.trading_state.orders {
            if !self
                .trading_state
                .account_visible(order.account_id.as_ref())
            {
                continue;
            }
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
            let creating_protection =
                self.trading_state
                    .interaction
                    .preview()
                    .is_some_and(|preview| {
                        matches!(
                            &preview.source,
                            crate::TradingPreviewSource::OrderStopLoss { order_id }
                                | crate::TradingPreviewSource::OrderTakeProfit { order_id }
                                if order_id == &order.id
                        )
                    });
            let base_color = trading_order_color(
                &self.trading_state.style,
                order.kind,
                order.side,
                order.role,
                order.status,
            );
            // Only a live drag dims the line; a released change is already applied, so nothing
            // lingers in a pending tint.
            let color = if preview.is_some() {
                Color::rgba(base_color.r(), base_color.g(), base_color.b(), 176)
            } else {
                base_color
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
                // Hover is communicated by the dash pattern, not a thickness jump. Dash metrics
                // scale with stroke width in every executor, so keeping the hairline also keeps
                // the hover dashes compact and consistent across DPRs.
                width: min_line_width,
                style: if creating_protection
                    || (order.role == OrderRole::Working
                        && hovered.is_some_and(|hit| hit.kind == crate::TradingHitKind::OrderLine))
                {
                    LineStyle::Dashed
                } else if preview.is_some()
                    || matches!(
                        order.status,
                        OrderStatus::PendingSubmit
                            | OrderStatus::PendingModify
                            | OrderStatus::PendingCancel
                    )
                {
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
            let quantity = self.trading_order_quantity_text(order);
            let kind = match order.kind {
                crate::OrderKind::Market => "Market",
                crate::OrderKind::Limit => "Limit",
                crate::OrderKind::Stop => "Stop",
                crate::OrderKind::StopLimit => "Stop Limit",
            };
            let descriptor = if preview.is_some() {
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
            let main_x = self.trading_marker_start();
            // A drag names its side ahead of the readout chip so the pointer never hides which way
            // the order goes. Release commits the modification directly — a host that wants a
            // confirmation step runs it around the emitted intent, not inside the chart.
            if preview.is_some() {
                self.push_trading_segment(
                    lines,
                    TradingControlSegment {
                        kind: TradingControlSegmentKind::Quantity,
                        text: if order.side == OrderSide::Buy {
                            "Buy"
                        } else {
                            "Sell"
                        },
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
            let hovered_segment = hovered.and_then(|hit| Self::trading_control_kind(hit.kind));
            let pressed_segment = pressed.and_then(|hit| Self::trading_control_kind(hit.kind));
            let (detail_kind, detail_text, detail_color) = if order.role == OrderRole::Working {
                (
                    TradingControlSegmentKind::OrderType,
                    descriptor.clone(),
                    color,
                )
            } else {
                // The PnL text keeps its profit/loss tint; the container carries the order's
                // buy/sell color.
                let (pnl, pnl_color) = self
                    .trading_protection_pnl(order, display_price)
                    .unwrap_or_else(|| ("—".to_string(), color));
                (TradingControlSegmentKind::Pnl, pnl, pnl_color)
            };
            let segments = [
                TradingControlSegment {
                    kind: TradingControlSegmentKind::Quantity,
                    text: quantity.as_str(),
                    width: self.trading_quantity_width(&quantity),
                    color,
                    filled: true,
                },
                TradingControlSegment {
                    kind: detail_kind,
                    text: detail_text.as_str(),
                    width: Self::trading_order_detail_width(order),
                    color: detail_color,
                    filled: false,
                },
                TradingControlSegment {
                    kind: TradingControlSegmentKind::Cancel,
                    text: "",
                    width: self.trading_close_width(),
                    color,
                    filled: false,
                },
            ];
            let cluster = TradingControlCluster {
                segments: &segments,
                left: main_x,
                color,
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
            self.push_trading_annotations(lines, &order.annotations, y, hpr, vpr);
            if self.trading_state.tooltip_armed
                && hovered.is_some_and(|hit| hit.kind == crate::TradingHitKind::CancelButton)
            {
                tooltip = Some(TradingTooltip {
                    text: "Cancel order",
                    layout: TradingChipLayout {
                        x: cluster.start() + cluster.width() - self.trading_close_width() / 2.0,
                        y,
                        hpr,
                        vpr,
                    },
                });
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
            self.push_host_trigger_line(
                lines,
                TradingTriggerLine {
                    pane_index,
                    price_scale: order.price_scale,
                    trigger_price: order.trailing_trigger_price,
                    display_price,
                    color: self.trading_state.style.pending,
                },
                hpr,
                vpr,
            );
            self.push_host_trigger_line(
                lines,
                TradingTriggerLine {
                    pane_index,
                    price_scale: order.price_scale,
                    trigger_price: order.break_even_trigger_price,
                    display_price,
                    color: self.trading_state.style.take_profit,
                },
                hpr,
                vpr,
            );
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
                let creating_protection =
                    !matches!(preview.source, crate::TradingPreviewSource::Order { .. });
                if creating_protection {
                    let semantic = if preview.role == OrderRole::TakeProfit {
                        self.trading_state.style.take_profit
                    } else {
                        self.trading_state.style.stop_loss
                    };
                    lines.push(Prim::HLine {
                        y: (preview_y * vpr).round() as i32,
                        x0: (self.trading_marker_start() * hpr).round() as i32,
                        x1: (self.trading_marker_end() * hpr).round() as i32,
                        width: min_line_width,
                        style: LineStyle::Dotted,
                        color: semantic,
                    });
                    let quantity = self.format_trading_quantity(preview.quantity);
                    let role = if preview.role == OrderRole::TakeProfit {
                        "TP"
                    } else {
                        "SL"
                    };
                    let segments = [
                        TradingControlSegment {
                            kind: TradingControlSegmentKind::Quantity,
                            text: quantity.as_str(),
                            width: self.trading_quantity_width(&quantity),
                            color: semantic,
                            filled: true,
                        },
                        TradingControlSegment {
                            kind: TradingControlSegmentKind::OrderType,
                            text: role,
                            width: ORDER_TYPE_WIDTH,
                            color: semantic,
                            filled: false,
                        },
                    ];
                    let cluster = TradingControlCluster {
                        segments: &segments,
                        left: self.trading_marker_start(),
                        color: semantic,
                    };
                    self.push_trading_cluster(
                        lines,
                        &cluster,
                        None,
                        None,
                        TradingChipLayout {
                            x: cluster.start(),
                            y: preview_y,
                            hpr,
                            vpr,
                        },
                    );
                }
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
                if !self
                    .trading_state
                    .account_visible(position.account_id.as_ref())
                {
                    continue;
                }
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
                if !self
                    .trading_state
                    .account_visible(order.account_id.as_ref())
                {
                    continue;
                }
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
                    if !self
                        .trading_state
                        .account_visible(position.account_id.as_ref())
                    {
                        continue;
                    }
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
                    if !self
                        .trading_state
                        .account_visible(order.account_id.as_ref())
                    {
                        continue;
                    }
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

        for round_trip in self
            .trading_state
            .round_trips
            .iter()
            .take(crate::MAX_TRADING_ROUND_TRIPS)
        {
            let Some(entry) = self
                .trading_state
                .executions
                .iter()
                .find(|execution| execution.id == round_trip.entry_execution_id)
            else {
                continue;
            };
            let Some(exit) = self
                .trading_state
                .executions
                .iter()
                .find(|execution| execution.id == round_trip.exit_execution_id)
            else {
                continue;
            };
            if !self
                .trading_state
                .account_visible(entry.account_id.as_ref())
                || !self.trading_state.account_visible(exit.account_id.as_ref())
                || entry.pane_index != pane_index
                || exit.pane_index != pane_index
                || times.is_empty()
            {
                continue;
            }
            let logical = |time: i64| match times.binary_search(&time) {
                Ok(index) => index,
                Err(index) if index < times.len() => index,
                Err(_) => times.len() - 1,
            } as i64;
            let entry_x = self.time_scale.index_to_coordinate(logical(entry.time));
            let exit_x = self.time_scale.index_to_coordinate(logical(exit.time));
            let Some(entry_y) =
                self.trading_price_coordinate(pane_index, entry.price_scale, entry.price)
            else {
                continue;
            };
            let Some(exit_y) =
                self.trading_price_coordinate(pane_index, exit.price_scale, exit.price)
            else {
                continue;
            };
            let color = match round_trip.outcome {
                crate::TradingRoundTripOutcome::Profit => self.trading_state.style.profit,
                crate::TradingRoundTripOutcome::Loss => self.trading_state.style.risk,
                crate::TradingRoundTripOutcome::Flat => self.trading_state.style.control,
            };
            let x0 = (entry_x * hpr).round() as i32;
            let x1 = (exit_x * hpr).round() as i32;
            lines.push(Prim::VLine {
                x: x0,
                y0: (entry_y.min(exit_y) * vpr).round() as i32,
                y1: (entry_y.max(exit_y) * vpr).round() as i32,
                width: min_line_width,
                style: LineStyle::Dotted,
                color,
            });
            lines.push(Prim::HLine {
                y: (exit_y * vpr).round() as i32,
                x0: x0.min(x1),
                x1: x0.max(x1),
                width: min_line_width,
                style: LineStyle::Dotted,
                color,
            });
            lines.push(Prim::Text {
                x: (((entry_x + exit_x) / 2.0) * hpr) as f32,
                y: (exit_y * vpr) as f32,
                text: round_trip.result_label.clone(),
                color,
                size: (self.options.get().layout.font_size * vpr) as f32,
                family: self.options.get().layout.font_family.clone(),
                align: TextAlign::Center,
                weight: 400,
                italic: false,
            });
        }
        for execution in &self.trading_state.executions {
            if !self
                .trading_state
                .account_visible(execution.account_id.as_ref())
            {
                continue;
            }
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
            let radius = (if hovered { 10.0 } else { 8.0 })
                * if execution.size_by_quantity {
                    (execution.quantity.abs().sqrt() / 2.0).clamp(0.75, 2.0)
                } else {
                    1.0
                }
                * vpr;
            match execution.marker_shape {
                crate::ExecutionMarkerShape::Circle => lines.push(Prim::Circle {
                    cx: (x * hpr) as f32,
                    cy: (y * vpr) as f32,
                    radius: radius as f32,
                    fill: color,
                    stroke_width: (1.0 * vpr) as f32,
                    stroke: color.contrast_text(),
                }),
                crate::ExecutionMarkerShape::Triangle | crate::ExecutionMarkerShape::Arrow => {
                    let direction = if execution.side == OrderSide::Buy {
                        -1.0
                    } else {
                        1.0
                    };
                    let tip = [(x * hpr) as f32, ((y * vpr) + direction * radius) as f32];
                    let left = [
                        ((x * hpr) - radius) as f32,
                        ((y * vpr) - direction * radius) as f32,
                    ];
                    let right = [
                        ((x * hpr) + radius) as f32,
                        ((y * vpr) - direction * radius) as f32,
                    ];
                    lines.push(Prim::Triangle {
                        a: tip,
                        b: left,
                        c: right,
                        color,
                    });
                }
            }
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
            self.push_trading_tooltip(lines, tooltip.text, pane, tooltip.layout);
        }
    }
}
