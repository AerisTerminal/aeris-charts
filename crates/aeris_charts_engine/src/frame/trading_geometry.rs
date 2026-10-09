use super::*;
use crate::Pane;
use crate::trading::{OrderRole, OrderSide, OrderStatus, PositionSide, TradingGroupVisualState};
use aeris_charts_core::style::{
    DARK_NEGATIVE_SUBTLE_RGB, DARK_POSITIVE_SUBTLE_RGB, LIGHT_NEGATIVE_SUBTLE_RGB,
    LIGHT_POSITIVE_SUBTLE_RGB, RADIUS_SMALL,
};

#[derive(Clone, Copy)]
struct TradingChipLayout {
    x: f64,
    y: f64,
    hpr: f64,
    vpr: f64,
}

/// Design-unit envelope of one execution arrow (`size` CSS px is 70 units).
const EXECUTION_ARROW_UNITS: f64 = 70.0;
/// Design-unit pitch between the chevrons of a multi-fill mark.
const EXECUTION_CHEVRON_PITCH: f64 = 24.0;
/// Most chevrons one mark draws, so a busy bar cannot grow its mark without limit.
const MAX_EXECUTION_CHEVRONS: usize = 5;

/// One execution arrow: every visible fill of one side on one bar.
pub(crate) struct TradingExecutionMark {
    /// Range of this mark's fills in [`TradingExecutionLayout::order`].
    pub fills: std::ops::Range<usize>,
    pub side: OrderSide,
    /// Bar center and arrow center in CSS px.
    pub x: f64,
    pub y: f64,
    /// Arrow width envelope in CSS px; one design unit is `size / 70`.
    pub size: f64,
    /// Vertical extent in CSS px: `size` for one chevron, one pitch taller per extra chevron.
    pub height: f64,
    /// Chevrons drawn: one per fill up to [`MAX_EXECUTION_CHEVRONS`]; 1 for non-arrow shapes.
    pub chevrons: usize,
}

#[derive(Default)]
pub(crate) struct TradingExecutionLayout {
    /// Execution slots sorted by bar, side, and time; marks index contiguous runs of it.
    pub order: Vec<usize>,
    pub marks: Vec<TradingExecutionMark>,
}

#[derive(Clone)]
struct TradingTooltip {
    text: String,
    layout: TradingChipLayout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TradingControlSegmentKind {
    Quantity,
    Pnl,
    OrderType,
    TakeProfit,
    StopLoss,
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

/// One order or position marker: quantity, PnL/order type, and a trailing integrated close cell
/// inside one solid-outlined pill. Every segment before `Cancel` is a readout cell.
struct TradingControlCluster<'a> {
    segments: &'a [TradingControlSegment<'a>],
    left: f64,
    color: Color,
    /// Resting limit body and rails use `fill` while text/icons use the stronger `color`.
    fill: Option<Color>,
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

    /// Width of the readout portion before the integrated close cell.
    fn body_width(&self) -> f64 {
        self.body().iter().map(|segment| segment.width).sum()
    }

    /// Width of the complete integrated marker — what the hit test measures against.
    fn width(&self) -> f64 {
        self.segments.iter().map(|segment| segment.width).sum()
    }
}

const CELL_PAD_X: f64 = 6.0;
const MAX_QUANTITY_WIDTH: f64 = 120.0;
/// The value cell holds PnL or the order type. Its floor keeps a ticking PnL from resizing the
/// marker (and moving the close cell under the pointer) until the value outgrows it.
const VALUE_MIN_WIDTH: f64 = 76.0;
const MAX_VALUE_WIDTH: f64 = 180.0;
const PROTECTION_BUTTON_WIDTH: f64 = 24.0;
const PROTECTION_BUTTON_GAP: f64 = 3.0;
/// Separation between independent annotation chips.
const ANNOTATION_GAP: f64 = 5.0;
const ORDER_MARKER_SPAN: f64 = 304.0;
/// Corner radius of every marker control, in CSS px.
const MARKER_RADIUS: f64 = 2.0;
/// Markers set their text one step below the axis font, so the compact control keeps clear air
/// above and below the glyphs: 11px text in a 16px control at the canonical 12px font.
const MARKER_FONT_STEP: f64 = 1.0;
const CONTROL_PAD_Y: f64 = 5.0;
/// Text weight of solid cells and the TP/SL buttons; value text stays regular.
const MARKER_STRONG_WEIGHT: u16 = 600;

/// Opaque `t` mix of `color` over `surface`, the CSS `color-mix(in srgb, …)` of a tint.
fn mix_over(color: Color, surface: Color, t: f64) -> Color {
    let blend = |a: u8, b: u8| (f64::from(a) * t + f64::from(b) * (1.0 - t)).round() as u8;
    Color::rgb(
        blend(color.r(), surface.r()),
        blend(color.g(), surface.g()),
        blend(color.b(), surface.b()),
    )
}

/// Protection semantics take precedence over their broker-side implementation: an SL keeps the
/// stop-loss token and a TP the take-profit token. This is the strong side color of an ordinary
/// order; resting limits use a subtle variant for their lines and markers, not their axis text.
pub(crate) fn trading_order_color(
    style: &crate::TradingStyle,
    _kind: crate::OrderKind,
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
        OrderStatus::Filled | OrderStatus::Working | OrderStatus::PartiallyFilled => by_side,
    }
}

impl ChartEngine {
    /// The subtle line and marker fill of a resting ordinary limit or stop-limit, from the style or
    /// else the token theme of the painted surface. Protections, stops, pending, terminal, and
    /// filled orders stay solid.
    pub(super) fn trading_order_fill(&self, order: &crate::WorkingOrder) -> Option<Color> {
        if order.role != OrderRole::Working
            || !matches!(
                order.kind,
                crate::OrderKind::Limit | crate::OrderKind::StopLimit
            )
            || !matches!(
                order.status,
                OrderStatus::Working | OrderStatus::PartiallyFilled
            )
        {
            return None;
        }
        let style = &self.trading_state.style;
        let light = self.surface_theme() == crate::ChartTheme::Light;
        let (configured, rgb) = match (order.side, light) {
            (OrderSide::Buy, true) => (style.buy_limit, LIGHT_POSITIVE_SUBTLE_RGB),
            (OrderSide::Buy, false) => (style.buy_limit, DARK_POSITIVE_SUBTLE_RGB),
            (OrderSide::Sell, true) => (style.sell_limit, LIGHT_NEGATIVE_SUBTLE_RGB),
            (OrderSide::Sell, false) => (style.sell_limit, DARK_NEGATIVE_SUBTLE_RGB),
        };
        Some(configured.unwrap_or(Color::rgb(rgb.0, rgb.1, rgb.2)))
    }

    fn trading_control_kind(kind: crate::TradingHitKind) -> Option<TradingControlSegmentKind> {
        match kind {
            crate::TradingHitKind::TakeProfitButton => Some(TradingControlSegmentKind::TakeProfit),
            crate::TradingHitKind::StopLossButton => Some(TradingControlSegmentKind::StopLoss),
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

    /// Vertical extent `(top, bottom)` in CSS px of what the primary series of `target` paints
    /// within `half_width` of bar `index`: the wick for OHLC series (Heikin-Ashi when shown), the
    /// column top for histograms, and for line, area, and baseline series the stroked line itself
    /// across the mark's width (the slope toward each neighbor, or a stepped line's riser) padded
    /// by half the line width. Execution marks sit outside this extent, so they clear the rendered
    /// shape on every series type instead of touching a sloped line. Only the primary series
    /// anchors them: fills belong to the traded instrument's bars, so overlays on the same scale
    /// (moving averages, host studies, compare lines) must not push them away from their bar.
    fn trading_bar_extent(
        &self,
        pane_index: usize,
        target: PriceScaleTarget,
        index: i64,
        from: i64,
        half_width: f64,
    ) -> Option<(f64, f64)> {
        let pane = self.panes.get(pane_index)?;
        let scale = pane_scale(pane, target);
        if scale.is_empty() {
            return None;
        }
        let anchor = self
            .primary_series_on_price_scale(pane_index, target)?
            .series_id;
        let series = self.series.iter().find(|series| series.id == anchor)?;
        let render_end = self.series_render_end(series.id, i64::MAX);
        if !series.visible || self.indicator_binding_id(series.id).is_some() || index > render_end {
            return None;
        }
        let plot = self.data.plot(series.id);
        let row = plot.search(index, MismatchDirection::None)?;
        if plot.is_whitespace_row(row) {
            return None;
        }
        let base_value = self.series_base_value(series.id, from)?;
        let bar_spacing = self.time_scale.bar_spacing();
        let mut extent: Option<(f64, f64)> = None;
        let mut include = |y: f64, pad: f64| {
            if y.is_finite() {
                extent = Some(extent.map_or((y - pad, y + pad), |(top, bottom)| {
                    (top.min(y - pad), bottom.max(y + pad))
                }));
            }
        };
        let y_of = |price: f64| {
            if price.is_finite() {
                scale.price_to_coordinate(price, base_value)
            } else {
                f64::NAN
            }
        };
        match series.kind {
            SeriesKind::Candlestick | SeriesKind::Bar | SeriesKind::Footprint => {
                let [high, low] = self
                    .heikin_ashi_row(series.id, row)
                    .map(|values| [values[1], values[2]])
                    .unwrap_or_else(|| {
                        [
                            plot.value_at(row, PlotValueIndex::High),
                            plot.value_at(row, PlotValueIndex::Low),
                        ]
                    });
                include(y_of(high), 0.0);
                include(y_of(low), 0.0);
            }
            SeriesKind::Histogram => {
                include(y_of(plot.value_at(row, PlotValueIndex::Close)), 0.0);
            }
            SeriesKind::Line | SeriesKind::Area | SeriesKind::Baseline => {
                // Baseline quadrants may override the width; clear the widest stroke.
                let width = [series.top_line_width, series.bottom_line_width]
                    .into_iter()
                    .flatten()
                    .fold(series.line_width.unwrap_or(LINE_WIDTH), f64::max);
                let pad = width / 2.0;
                let y = y_of(plot.value_at(row, PlotValueIndex::Close));
                include(y, pad);
                for step in [-1_isize, 1] {
                    let Some(neighbor) = row.checked_add_signed(step) else {
                        continue;
                    };
                    let Some(neighbor_index) = plot.index_at(neighbor) else {
                        continue;
                    };
                    if neighbor_index > render_end || plot.is_whitespace_row(neighbor) {
                        continue;
                    }
                    let neighbor_y = y_of(plot.value_at(neighbor, PlotValueIndex::Close));
                    let gap = (neighbor_index - index).abs() as f64 * bar_spacing;
                    match series.line_type {
                        // A stepped line runs flat at its own value, then turns at the next
                        // bar: the previous bar's riser stands exactly on this bar's x.
                        LineType::WithSteps => {
                            if step < 0 {
                                include(neighbor_y, pad);
                            }
                        }
                        LineType::Simple | LineType::Curved => {
                            if gap > 0.0 {
                                let t = (half_width / gap).min(1.0);
                                include(y + (neighbor_y - y) * t, pad);
                            }
                        }
                    }
                }
            }
            SeriesKind::Custom | SeriesKind::Feature => {}
        }
        extent
    }

    /// Lay out execution marks for one pane, shared by the frame and hit testing. Each fill
    /// resolves to the bar that contains its time, and every visible fill of one side on one bar
    /// becomes a single arrow: buys below the bar, sells above it. Only visible bars are laid out,
    /// so work stays bounded by the viewport rather than the fill history.
    pub(crate) fn trading_execution_layout(&self, pane_index: usize) -> TradingExecutionLayout {
        let mut layout = TradingExecutionLayout::default();
        let Some((from, to)) = self.visible_range_for_frame() else {
            return layout;
        };
        let first_time = self.axis_time_key_at(0);
        let mut entries = Vec::new();
        for (slot, execution) in self.trading_state.executions.iter().enumerate() {
            if execution.pane_index != pane_index
                || !self
                    .trading_state
                    .account_visible(execution.account_id.as_ref())
                || !self.replay_time_is_visible(execution.time)
                // A fill before the loaded history has no bar to sit on yet.
                || first_time.is_some_and(|first| execution.time < first)
            {
                continue;
            }
            let Some(index) = self.axis_index_for_time(execution.time) else {
                continue;
            };
            let index = index as i64;
            if (from..=to).contains(&index) {
                let side = u8::from(execution.side == OrderSide::Sell);
                entries.push((index, side, execution.time, slot));
            }
        }
        entries.sort_unstable();
        layout.order = entries.iter().map(|&(.., slot)| slot).collect();

        let bar_spacing = self.time_scale.bar_spacing();
        let envelope = marker_envelope_size(bar_spacing);
        let margin = marker_margin(bar_spacing);
        let mut start = 0;
        while start < entries.len() {
            let (index, side, ..) = entries[start];
            let end = start
                + entries[start..]
                    .iter()
                    .take_while(|entry| entry.0 == index && entry.1 == side)
                    .count();
            let fills = &layout.order[start..end];
            let executions = &self.trading_state.executions;
            let quantity: f64 = fills.iter().map(|&slot| executions[slot].quantity).sum();
            let scale = if fills.iter().any(|&slot| executions[slot].size_by_quantity) {
                (quantity.abs().sqrt() / 2.0).clamp(0.75, 2.0)
            } else {
                1.0
            };
            let size = envelope.clamp(13.0, 18.0) * scale;
            let chevrons = if executions[fills[fills.len() - 1]].marker_shape
                == crate::ExecutionMarkerShape::Arrow
            {
                fills.len().min(MAX_EXECUTION_CHEVRONS)
            } else {
                1
            };
            let height = size
                * (EXECUTION_ARROW_UNITS + EXECUTION_CHEVRON_PITCH * (chevrons - 1) as f64)
                / EXECUTION_ARROW_UNITS;
            let target = PriceScaleTarget::from(executions[fills[0]].price_scale);
            let extent = self
                .trading_bar_extent(pane_index, target, index, from, size / 2.0)
                .or_else(|| {
                    // No painted bar here (whitespace or a custom series): bracket the fills.
                    fills
                        .iter()
                        .fold(None, |extent: Option<(f64, f64)>, &slot| {
                            let fill = &executions[slot];
                            let y = self.trading_price_coordinate(
                                pane_index,
                                fill.price_scale,
                                fill.price,
                            )?;
                            Some(extent.map_or((y, y), |(top, bottom)| (top.min(y), bottom.max(y))))
                        })
                });
            if let Some((top, bottom)) = extent {
                let side = if side == 0 {
                    OrderSide::Buy
                } else {
                    OrderSide::Sell
                };
                let y = match side {
                    OrderSide::Buy => bottom + margin + height / 2.0,
                    OrderSide::Sell => top - margin - height / 2.0,
                };
                layout.marks.push(TradingExecutionMark {
                    fills: start..end,
                    side,
                    x: self.time_scale.index_to_coordinate(index),
                    y,
                    size,
                    height,
                    chevrons,
                });
            }
            start = end;
        }
        layout
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

    pub(crate) fn format_trading_price(
        &self,
        pane_index: usize,
        scale: crate::TradingPriceScale,
        value: f64,
    ) -> String {
        self.format_scale_price(pane_index, scale.into(), value)
    }

    pub(crate) fn format_trading_quantity(&self, value: f64) -> String {
        match self.trading_state.instrument.quantity_precision {
            Some(precision) => format!("{value:.precision$}", precision = precision as usize),
            // Without host precision, show the exact size without trailing zeros. Eight places
            // covers fractional crypto sizes that four places would round away.
            None if value.fract().abs() < f64::EPSILON => format!("{value:.0}"),
            None => format!("{value:.8}")
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

    pub(crate) fn trading_chip_background(&self) -> Color {
        let fallback = aeris_charts_core::style::DEFAULT_SURFACE_RGB;
        Color::parse_css(&self.options.get().layout.background.color)
            .unwrap_or(Color::rgb(fallback.0, fallback.1, fallback.2))
            .solid()
    }

    /// Engine border width (1 CSS px) on the device grid with browser border semantics: whole
    /// device pixels, rounded down, never thinner than one device pixel.
    fn trading_border_width(vpr: f64) -> f64 {
        aeris_charts_core::style::border_width_device_px(vpr)
    }

    /// Text size of every marker control: quantity, value, close, TP/SL, and annotation chips.
    pub(crate) fn trading_marker_font_size(&self) -> f64 {
        (self.options.get().layout.font_size - MARKER_FONT_STEP).max(1.0)
    }

    pub(crate) fn trading_control_height(&self) -> f64 {
        self.trading_marker_font_size() + CONTROL_PAD_Y
    }

    /// Device-pixel `(left, top, right, bottom)` of a control box centered on `y`, snapped once,
    /// and the device `y` that seats its text. The marker and the TP/SL buttons share this rect,
    /// so every control on a line has the same whole-pixel height.
    fn trading_control_rect(
        &self,
        left: f64,
        width: f64,
        y: f64,
        hpr: f64,
        vpr: f64,
    ) -> ([f64; 4], f64) {
        let (top, bottom, text_y) = self.trading_text_box(
            y,
            self.trading_control_height(),
            self.trading_marker_font_size(),
            vpr,
        );
        (
            [
                (left * hpr).round(),
                top,
                ((left + width) * hpr).round(),
                bottom,
            ],
            text_y,
        )
    }

    /// Device rows `(top, bottom)` of a text box of `height` CSS px centered on `center_y`, and
    /// the `Prim::Text` anchor that seats `font_size` text in it. Equal padding needs the rows
    /// left over after the cap ink to split evenly, so with host cap metrics the box takes the
    /// ink's parity (at most one device row off the nominal height) and the baseline lands on a
    /// whole device row: the rows above and below the ink are then equal at every pixel ratio.
    /// Without metrics the box keeps its nominal height and the text its geometric center.
    fn trading_text_box(
        &self,
        center_y: f64,
        height: f64,
        font_size: f64,
        vpr: f64,
    ) -> (f64, f64, f64) {
        let nominal = (height * vpr).round().max(1.0);
        let center = center_y * vpr;
        let metrics = self
            .text_cap_metrics(
                font_size * vpr,
                &self.options.get().layout.font_family,
                400,
                false,
            )
            .filter(|metrics| metrics.cap_height < nominal);
        let Some(metrics) = metrics else {
            let top = (center - nominal / 2.0).round();
            return (top, top + nominal, top + nominal / 2.0);
        };
        // A cap height a hair over a whole row is measurement noise, not an extra ink row.
        let ink = (metrics.cap_height - 0.01).ceil().max(1.0);
        let pad = ((nominal - ink) / 2.0).round().max(0.0);
        let rows = ink + 2.0 * pad;
        let top = (center - rows / 2.0).round();
        let baseline = top + pad + ink;
        (
            top,
            top + rows,
            baseline + metrics.center_offset - metrics.cap_height / 2.0,
        )
    }

    /// Device-pixel corner radius of a marker control, never more than half its height.
    fn trading_marker_radius(hpr: f64, vpr: f64, height: f64) -> f64 {
        (MARKER_RADIUS * hpr.min(vpr)).round().min(height / 2.0)
    }

    /// The close cell keeps equal width and height, so its glyph sits on the marker's rhythm.
    pub(crate) fn trading_close_width(&self) -> f64 {
        self.trading_control_height()
    }

    pub(crate) fn trading_marker_end(&self) -> f64 {
        self.pane_w.max(6.0)
    }

    pub(crate) fn trading_marker_start(&self) -> f64 {
        (self.trading_marker_end() - ORDER_MARKER_SPAN).max(6.0)
    }

    fn trading_marker_text_width(&self, text: &str, weight: u16) -> f64 {
        let layout = &self.options.get().layout;
        self.measure_text_run(
            text,
            self.trading_marker_font_size(),
            &layout.font_family,
            weight,
            false,
        )
    }

    /// Quantity cells fit their formatted text instead of reserving a fixed-width box. The upper
    /// bound keeps extreme finite magnitudes from expanding a marker without limit.
    fn trading_quantity_width(&self, text: &str) -> f64 {
        (self.trading_marker_text_width(text, MARKER_STRONG_WEIGHT) + CELL_PAD_X * 2.0)
            .ceil()
            .clamp(self.trading_control_height(), MAX_QUANTITY_WIDTH)
    }

    fn trading_value_width(&self, text: &str) -> f64 {
        (self.trading_marker_text_width(text, 400) + CELL_PAD_X * 2.0)
            .ceil()
            .clamp(VALUE_MIN_WIDTH, MAX_VALUE_WIDTH)
    }

    /// A working order shows the size still resting at its price; a partial fill shrinks it.
    fn trading_order_quantity_text(&self, order: &crate::WorkingOrder) -> String {
        self.format_trading_quantity((order.quantity - order.filled_quantity).max(0.0))
    }

    fn trading_position_quantity_text(&self, position: &crate::TradingPosition) -> String {
        let signed_quantity = if position.side == PositionSide::Long {
            position.quantity
        } else {
            -position.quantity
        };
        self.format_trading_quantity(signed_quantity)
    }

    fn trading_side_label(side: OrderSide) -> &'static str {
        match side {
            OrderSide::Buy => "Buy",
            OrderSide::Sell => "Sell",
        }
    }

    /// The value cell of an order: a working order names itself, a protection order reports the
    /// PnL it would realise in the sign's own token. A drag drops the side from the name because
    /// the drag tag ahead of the marker already states it.
    fn trading_order_detail(
        &self,
        order: &crate::WorkingOrder,
    ) -> (TradingControlSegmentKind, String, Color) {
        if order.role != OrderRole::Working {
            let (text, color) = self
                .trading_protection_pnl(order, self.trading_effective_order_price(order))
                .unwrap_or_else(|| ("—".to_string(), self.primary_text_color()));
            return (TradingControlSegmentKind::Pnl, text, color);
        }
        let kind = match order.kind {
            crate::OrderKind::Market => "Market",
            crate::OrderKind::Limit => "Limit",
            crate::OrderKind::Stop => "Stop",
            crate::OrderKind::StopLimit => "Stop Limit",
        };
        let text = if self.trading_order_preview(order).is_some() {
            kind.to_string()
        } else {
            format!("{} {kind}", Self::trading_side_label(order.side))
        };
        (
            TradingControlSegmentKind::OrderType,
            text,
            self.primary_text_color(),
        )
    }

    fn trading_position_detail(&self, position: &crate::TradingPosition) -> (String, Color) {
        match position.display_pnl {
            Some(value) => (
                self.trading_pnl_text(value, position.currency.as_deref()),
                if value >= 0.0 {
                    self.trading_state.style.profit
                } else {
                    self.trading_state.style.risk
                },
            ),
            None => ("—".to_string(), self.primary_text_color()),
        }
    }

    pub(crate) fn trading_order_cluster_width(&self, order: &crate::WorkingOrder) -> f64 {
        self.trading_quantity_width(&self.trading_order_quantity_text(order))
            + self.trading_value_width(&self.trading_order_detail(order).1)
            + self.trading_close_width()
    }

    pub(crate) fn trading_position_cluster_width(&self, position: &crate::TradingPosition) -> f64 {
        self.trading_quantity_width(&self.trading_position_quantity_text(position))
            + self.trading_value_width(&self.trading_position_detail(position).0)
            + self.trading_close_width()
    }

    /// Left edge of the TP/SL buttons, or the marker itself when neither is offered.
    pub(crate) fn trading_protection_buttons_start(
        &self,
        show_take_profit: bool,
        show_stop_loss: bool,
    ) -> f64 {
        let buttons = usize::from(show_take_profit) + usize::from(show_stop_loss);
        self.trading_marker_start()
            - buttons as f64 * (PROTECTION_BUTTON_WIDTH + PROTECTION_BUTTON_GAP)
    }

    /// Width of the side tag a dragged order shows ahead of its other controls.
    pub(crate) fn trading_drag_tag_width(&self, side: OrderSide) -> f64 {
        self.trading_quantity_width(Self::trading_side_label(side))
    }

    /// Where a marker line begins: the pane's left edge, or with `extend_lines_left` off, the
    /// leftmost control on the line, so the line runs only from its controls to the right edge.
    fn trading_line_start(&self, controls_start: f64) -> f64 {
        if self.trading_state.style.extend_lines_left {
            0.0
        } else {
            controls_start
        }
    }

    pub(crate) fn trading_order_line_start(&self, order: &crate::WorkingOrder) -> f64 {
        let mut start = self.trading_protection_buttons_start(
            self.trading_order_protection_preview(order, OrderRole::TakeProfit)
                .is_some(),
            self.trading_order_protection_preview(order, OrderRole::StopLoss)
                .is_some(),
        );
        if self.trading_order_preview(order).is_some() {
            start -= self.trading_drag_tag_width(order.side) + PROTECTION_BUTTON_GAP;
        }
        self.trading_line_start(start)
    }

    pub(crate) fn trading_position_line_start(&self, position: &crate::TradingPosition) -> f64 {
        self.trading_line_start(
            self.trading_protection_buttons_start(
                self.trading_position_protection_preview(position, OrderRole::TakeProfit)
                    .is_some(),
                self.trading_position_protection_preview(position, OrderRole::StopLoss)
                    .is_some(),
            ),
        )
    }

    fn trading_protection_button_hit(
        &self,
        show_take_profit: bool,
        show_stop_loss: bool,
        x: f64,
    ) -> Option<crate::TradingHitKind> {
        let mut right = self.trading_marker_start() - PROTECTION_BUTTON_GAP;
        for (visible, kind) in [
            (show_stop_loss, crate::TradingHitKind::StopLossButton),
            (show_take_profit, crate::TradingHitKind::TakeProfitButton),
        ] {
            if !visible {
                continue;
            }
            let left = right - PROTECTION_BUTTON_WIDTH;
            if x >= left && x <= right {
                return Some(kind);
            }
            right = left - PROTECTION_BUTTON_GAP;
        }
        None
    }

    pub(crate) fn trading_order_protection_hit(
        &self,
        order: &crate::WorkingOrder,
        x: f64,
    ) -> Option<crate::TradingHitKind> {
        self.trading_protection_button_hit(
            self.trading_order_protection_preview(order, OrderRole::TakeProfit)
                .is_some(),
            self.trading_order_protection_preview(order, OrderRole::StopLoss)
                .is_some(),
            x,
        )
    }

    pub(crate) fn trading_position_protection_hit(
        &self,
        position: &crate::TradingPosition,
        x: f64,
    ) -> Option<crate::TradingHitKind> {
        self.trading_protection_button_hit(
            self.trading_position_protection_preview(position, OrderRole::TakeProfit)
                .is_some(),
            self.trading_position_protection_preview(position, OrderRole::StopLoss)
                .is_some(),
            x,
        )
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
            cursor += width + ANNOTATION_GAP;
        }
        None
    }

    fn trading_annotation_width(&self, text: &str) -> f64 {
        (self.trading_marker_text_width(text, 400) + CELL_PAD_X * 2.0)
            .ceil()
            .clamp(24.0, 180.0)
    }

    fn trading_annotation_color(&self, tone: crate::TradingAnnotationTone) -> Color {
        match tone {
            crate::TradingAnnotationTone::Neutral => self.trading_state.style.control,
            crate::TradingAnnotationTone::Info => self.trading_state.style.working_order,
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
        let font_size = self.trading_marker_font_size();
        let radius = [Self::trading_marker_radius(hpr, vpr, height * vpr) as f32; 4];
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
            let ([left, top, right, bottom], text_y) =
                self.trading_control_rect(cursor, width, center_y, hpr, vpr);
            out.push(Prim::RoundRect {
                x: left as f32,
                y: top as f32,
                w: (right - left) as f32,
                h: (bottom - top) as f32,
                radii: radius,
                fill: self.trading_chip_background(),
                border_width: Self::trading_border_width(vpr) as f32,
                border_color: color,
            });
            out.push(Prim::Text {
                x: ((left + right) / 2.0) as f32,
                y: text_y as f32,
                text: annotation.text.clone(),
                color,
                size: (font_size * vpr) as f32,
                family: self.options.get().layout.font_family.clone(),
                align: TextAlign::Center,
                weight: 400,
                italic: false,
            });
            cursor += width + ANNOTATION_GAP;
        }
        if annotations.len() > visible {
            let text = format!("+{}", annotations.len() - visible);
            let width = self.trading_annotation_width(&text);
            let ([left, top, right, bottom], text_y) =
                self.trading_control_rect(cursor, width, line_y, hpr, vpr);
            out.push(Prim::RoundRect {
                x: left as f32,
                y: top as f32,
                w: (right - left) as f32,
                h: (bottom - top) as f32,
                radii: radius,
                fill: self.trading_chip_background(),
                border_width: Self::trading_border_width(vpr) as f32,
                border_color: self.trading_state.style.control,
            });
            out.push(Prim::Text {
                x: ((left + right) / 2.0) as f32,
                y: text_y as f32,
                text,
                color: self.trading_state.style.control,
                size: (font_size * vpr) as f32,
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
            x0: (self.trading_line_start(self.trading_marker_start()) * hpr).round() as i32,
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

    /// Marker text centered on `x` at the `text_y` anchor of its snapped control
    /// ([`Self::trading_text_box`]); every cell of a control shares that anchor.
    fn push_trading_marker_text(
        &self,
        out: &mut Vec<Prim>,
        text: &str,
        (x, text_y): (f64, f64),
        color: Color,
        weight: u16,
        vpr: f64,
    ) {
        let font_size = self.trading_marker_font_size();
        out.push(Prim::Text {
            x: x as f32,
            y: text_y as f32,
            text: text.to_string(),
            color,
            size: (font_size * vpr) as f32,
            family: self.options.get().layout.font_family.clone(),
            align: TextAlign::Center,
            weight,
            italic: false,
        });
    }

    /// A standalone control: a solid tag in the line color with label text, or a hollow chip on
    /// the chart surface with a one-device-pixel border and text in the line color. Only hollow
    /// chips are buttons, so only they tint toward their color under hover and press.
    fn push_trading_segment(
        &self,
        out: &mut Vec<Prim>,
        segment: TradingControlSegment<'_>,
        feedback: TradingControlFeedback,
        layout: TradingChipLayout,
    ) {
        let TradingControlSegment {
            kind,
            text,
            width,
            color,
            filled,
        } = segment;
        let TradingChipLayout { x, y, hpr, vpr } = layout;
        let ([left, top, right, bottom], text_y) = self.trading_control_rect(x, width, y, hpr, vpr);
        let radius = Self::trading_marker_radius(hpr, vpr, bottom - top) as f32;
        let (fill, border_width, text_color, weight) = if filled {
            (
                color.solid(),
                0.0,
                self.trading_state.style.label,
                MARKER_STRONG_WEIGHT,
            )
        } else {
            let surface = self.trading_chip_background();
            let fill = match feedback {
                TradingControlFeedback::Idle => surface,
                TradingControlFeedback::Hovered => mix_over(color, surface, 0.18),
                TradingControlFeedback::Pressed => mix_over(color, surface, 0.30),
            };
            let weight = if matches!(
                kind,
                TradingControlSegmentKind::TakeProfit | TradingControlSegmentKind::StopLoss
            ) {
                MARKER_STRONG_WEIGHT
            } else {
                400
            };
            (fill, Self::trading_border_width(vpr), color, weight)
        };
        out.push(Prim::RoundRect {
            x: left as f32,
            y: top as f32,
            w: (right - left) as f32,
            h: (bottom - top) as f32,
            radii: [radius; 4],
            fill,
            border_width: border_width as f32,
            border_color: color,
        });
        self.push_trading_marker_text(
            out,
            text,
            ((left + right) / 2.0, text_y),
            text_color,
            weight,
            vpr,
        );
    }

    fn push_trading_protection_buttons(
        &self,
        out: &mut Vec<Prim>,
        show_take_profit: bool,
        show_stop_loss: bool,
        hovered: Option<crate::TradingHitKind>,
        pressed: Option<crate::TradingHitKind>,
        layout: TradingChipLayout,
    ) {
        let TradingChipLayout { y, hpr, vpr, .. } = layout;
        let mut right = self.trading_marker_start() - PROTECTION_BUTTON_GAP;
        for (visible, kind, segment_kind, label, color) in [
            (
                show_stop_loss,
                crate::TradingHitKind::StopLossButton,
                TradingControlSegmentKind::StopLoss,
                "SL",
                self.trading_state.style.stop_loss,
            ),
            (
                show_take_profit,
                crate::TradingHitKind::TakeProfitButton,
                TradingControlSegmentKind::TakeProfit,
                "TP",
                self.trading_state.style.take_profit,
            ),
        ] {
            if !visible {
                continue;
            }
            let left = right - PROTECTION_BUTTON_WIDTH;
            let feedback = if pressed == Some(kind) {
                TradingControlFeedback::Pressed
            } else if hovered == Some(kind) {
                TradingControlFeedback::Hovered
            } else {
                TradingControlFeedback::Idle
            };
            self.push_trading_segment(
                out,
                TradingControlSegment {
                    kind: segment_kind,
                    text: label,
                    width: PROTECTION_BUTTON_WIDTH,
                    color,
                    filled: false,
                },
                feedback,
                TradingChipLayout {
                    x: left,
                    y,
                    hpr,
                    vpr,
                },
            );
            right = left - PROTECTION_BUTTON_GAP;
        }
    }

    /// Draw the close icon as two anti-aliased strokes centered on `center` (device px).
    /// `Polyline` is the one stroke primitive every executor antialiases identically; separate
    /// triangles and cap discs are not, and GPUI feathered the tiny cap discs into blobs around
    /// hard-edged arms.
    fn push_trading_close_icon(
        &self,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
        center: (f64, f64),
        color: Color,
        hpr: f64,
        vpr: f64,
    ) {
        let arm = self.trading_control_height() * 0.1875;
        let width = (1.25 * hpr.min(vpr)).max(1.0) as f32;
        let (center_x, center_y) = center;
        for slope in [1.0_f64, -1.0] {
            let first_point = points.len() as u32;
            points.extend([
                [
                    (center_x - arm * hpr) as f32,
                    (center_y - arm * slope * vpr) as f32,
                ],
                [
                    (center_x + arm * hpr) as f32,
                    (center_y + arm * slope * vpr) as f32,
                ],
            ]);
            out.push(Prim::Polyline {
                first_point,
                point_count: 2,
                width,
                style: LineStyle::Solid,
                line_type: LineType::Simple,
                color,
            });
        }
    }

    /// One marker: a solid body in the line color whose readout cell is cut hollow onto the chart
    /// surface, leaving one-device-pixel rails of the line color above and below it. Solid cells
    /// carry label text; the hollow cell carries its own text color (PnL in the sign's token).
    /// Only the close cell reacts to the pointer; the readout is information, not a button.
    fn push_trading_cluster(
        &self,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
        cluster: &TradingControlCluster<'_>,
        hovered: Option<TradingControlSegmentKind>,
        pressed: Option<TradingControlSegmentKind>,
        layout: TradingChipLayout,
    ) {
        let TradingChipLayout { y, hpr, vpr, .. } = layout;
        let color = cluster.color.solid();
        let body = cluster.fill.map_or(color, |fill| fill.solid());
        let label = if cluster.fill.is_some() {
            color
        } else {
            self.trading_state.style.label
        };
        let left = cluster.start();
        // Snap the marker to whole device pixels once. Every cell edge and the text anchor derive
        // from this rect, so the rails stay one crisp device pixel and every cell's text keeps
        // equal padding above and below at every pixel ratio.
        let ([marker_left, top, marker_right, bottom], text_y) =
            self.trading_control_rect(left, cluster.width(), y, hpr, vpr);
        let radius = Self::trading_marker_radius(hpr, vpr, bottom - top);
        let center_y = (top + bottom) / 2.0;
        out.push(Prim::RoundRect {
            x: marker_left as f32,
            y: top as f32,
            w: (marker_right - marker_left) as f32,
            h: (bottom - top) as f32,
            radii: [radius as f32; 4],
            fill: body,
            border_width: 0.0,
            border_color: body,
        });
        let border = Self::trading_border_width(vpr);
        let last = cluster.segments.len() - 1;
        let mut cursor = left;
        for (index, segment) in cluster.body().iter().enumerate() {
            let cell_left = if index == 0 {
                marker_left
            } else {
                (cursor * hpr).round()
            };
            let cell_right = if index == last {
                marker_right
            } else {
                ((cursor + segment.width) * hpr).round()
            };
            if !segment.filled {
                // A hollow cell at an end of the marker keeps the outer rail on that side too,
                // with corners concentric to the marker's.
                let start = index == 0;
                let end = index == last;
                let hollow_left = cell_left + if start { border } else { 0.0 };
                let hollow_right = cell_right - if end { border } else { 0.0 };
                let inner = (radius - border).max(0.0) as f32;
                let surface = self.trading_chip_background();
                out.push(Prim::RoundRect {
                    x: hollow_left as f32,
                    y: (top + border) as f32,
                    w: (hollow_right - hollow_left) as f32,
                    h: (bottom - top - 2.0 * border) as f32,
                    radii: [
                        if start { inner } else { 0.0 },
                        if end { inner } else { 0.0 },
                        if end { inner } else { 0.0 },
                        if start { inner } else { 0.0 },
                    ],
                    fill: surface,
                    border_width: 0.0,
                    border_color: surface,
                });
            }
            let (text_color, weight) = if segment.filled {
                (label, MARKER_STRONG_WEIGHT)
            } else {
                (segment.color, 400)
            };
            self.push_trading_marker_text(
                out,
                segment.text,
                ((cell_left + cell_right) / 2.0, text_y),
                text_color,
                weight,
                vpr,
            );
            cursor += segment.width;
        }
        if let Some(close) = cluster.close() {
            let close_left = ((left + cluster.body_width()) * hpr).round();
            let feedback = match (pressed == Some(close.kind), hovered == Some(close.kind)) {
                (true, _) if body != color => Some(mix_over(color, body, 0.30)),
                (true, _) => Some(color.darken(0.75)),
                (false, true) if body != color => Some(mix_over(color, body, 0.18)),
                (false, true) => Some(color.lighten(0.2)),
                (false, false) => None,
            };
            if let Some(fill) = feedback {
                let r = radius as f32;
                out.push(Prim::RoundRect {
                    x: close_left as f32,
                    y: top as f32,
                    w: (marker_right - close_left) as f32,
                    h: (bottom - top) as f32,
                    radii: [0.0, r, r, 0.0],
                    fill,
                    border_width: 0.0,
                    border_color: fill,
                });
            }
            self.push_trading_close_icon(
                out,
                points,
                ((close_left + marker_right) / 2.0, center_y),
                label,
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
        let font_size = self.options.get().layout.font_size;
        let above =
            layout.y - (font_size + 5.0) / 2.0 - Self::trading_tooltip_height(font_size) - 5.0;
        let y = if above >= pane.top + 2.0 {
            above
        } else {
            layout.y + (font_size + 5.0) / 2.0 + 5.0
        };
        self.push_trading_tooltip_box(out, text, layout.x, y, layout.hpr, layout.vpr);
    }

    fn trading_tooltip_height(font_size: f64) -> f64 {
        font_size + 7.0
    }

    /// Tooltip chrome centered on `center_x` with its top edge at `y` (CSS px).
    fn push_trading_tooltip_box(
        &self,
        out: &mut Vec<Prim>,
        text: &str,
        center_x: f64,
        y: f64,
        hpr: f64,
        vpr: f64,
    ) {
        let font_size = self.options.get().layout.font_size;
        let height = Self::trading_tooltip_height(font_size);
        let radius = (RADIUS_SMALL * hpr.min(vpr)) as f32;
        let width = self.measure_text_run(
            text,
            font_size,
            &self.options.get().layout.font_family,
            400,
            false,
        ) + 12.0;
        let x = (center_x - width / 2.0).clamp(4.0, (self.pane_w - width - 4.0).max(4.0));
        let (top, bottom, text_y) = self.trading_text_box(y + height / 2.0, height, font_size, vpr);
        let left = (x * hpr).round();
        let right = ((x + width) * hpr).round().max(left + 1.0);
        out.push(Prim::RoundRect {
            x: left as f32,
            y: top as f32,
            w: (right - left) as f32,
            h: (bottom - top) as f32,
            radii: [radius.round(); 4],
            fill: self.trading_chip_background(),
            border_width: Self::trading_border_width(vpr) as f32,
            border_color: self.trading_chrome_border(),
        });
        out.push(Prim::Text {
            x: ((left + right) / 2.0) as f32,
            y: text_y as f32,
            text: text.to_string(),
            color: self.primary_text_color(),
            size: (font_size * vpr) as f32,
            family: self.options.get().layout.font_family.clone(),
            align: TextAlign::Center,
            weight: 400,
            italic: false,
        });
    }

    /// One execution mark centered on `(x_device, mark.y)`, drawn as open strokes with
    /// `Polyline`, the stroke primitive every executor antialiases identically. In design units
    /// (`size / 70`): a single fill is a 60-unit shaft with one chevron (wings 22 out and back
    /// from the tip, stroke 10). Each further fill on one side of one bar stacks one identical,
    /// tailless chevron 24 units nearer the bar (up to [`MAX_EXECUTION_CHEVRONS`]), so the mark
    /// counts the fills while only the outermost chevron carries the shaft.
    fn push_trading_execution_arrow(
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
        mark: &TradingExecutionMark,
        shape: crate::ExecutionMarkerShape,
        color: Color,
        layout: TradingChipLayout,
    ) {
        let TradingChipLayout {
            x: x_device,
            hpr,
            vpr,
            ..
        } = layout;
        let unit = mark.size / 70.0;
        // Up for buys, down for sells.
        let direction = if mark.side == OrderSide::Buy {
            -1.0
        } else {
            1.0
        };
        let point = |dx: f64, dy: f64| {
            [
                (x_device + dx * unit * hpr) as f32,
                ((mark.y + direction * dy * unit) * vpr) as f32,
            ]
        };
        match shape {
            crate::ExecutionMarkerShape::Arrow => {
                let mut stroke = |path: &[[f32; 2]], stroke_units: f64| {
                    let first_point = points.len() as u32;
                    points.extend_from_slice(path);
                    out.push(Prim::Polyline {
                        first_point,
                        point_count: path.len() as u32,
                        width: ((stroke_units * unit).max(1.5) * hpr.min(vpr)) as f32,
                        style: LineStyle::Solid,
                        line_type: LineType::Simple,
                        color,
                    });
                };
                // The outermost chevron (the newest fill) keeps the single arrow's shaft; every
                // earlier fill adds one tailless chevron of the same size, one pitch nearer the bar.
                let outer_tip = 30.0 - EXECUTION_CHEVRON_PITCH * (mark.chevrons - 1) as f64 / 2.0;
                stroke(&[point(0.0, outer_tip - 60.0), point(0.0, outer_tip)], 10.0);
                for k in 0..mark.chevrons {
                    let tip = outer_tip + EXECUTION_CHEVRON_PITCH * k as f64;
                    stroke(
                        &[
                            point(-22.0, tip - 22.0),
                            point(0.0, tip),
                            point(22.0, tip - 22.0),
                        ],
                        10.0,
                    );
                }
            }
            crate::ExecutionMarkerShape::Triangle => out.push(Prim::Triangle {
                a: point(0.0, 30.0),
                b: point(-30.0, -30.0),
                c: point(30.0, -30.0),
                color,
            }),
            crate::ExecutionMarkerShape::Circle => out.push(Prim::Circle {
                cx: x_device as f32,
                cy: (mark.y * vpr) as f32,
                radius: (mark.size * 0.4 * hpr) as f32,
                fill: color,
                stroke_width: 0.0,
                stroke: color,
            }),
        }
    }

    /// Execution arrows, then the exact-fill detail of the hovered or pressed arrow: a tick at
    /// every fill's own price on the bar, a dotted lead from the arrow, and a fill tooltip placed
    /// on the arrow's outer side so it never covers the bar.
    fn push_trading_executions(
        &self,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
        pane: &Pane,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
    ) {
        let min_line_width = vpr.floor().max(1.0) as i32;
        let layout = self.trading_execution_layout(pane_index);
        if layout.marks.is_empty() {
            return;
        }
        let executions = &self.trading_state.executions;
        let focused = |slot: &usize| {
            [
                &self.trading_state.feedback_hover,
                &self.trading_state.feedback_pressed,
            ]
            .into_iter()
            .flatten()
            .any(|hit| {
                matches!(&hit.object, crate::TradingObjectId::Execution(id) if id == &executions[*slot].id)
            })
        };
        // Same device-pixel centering as series markers, so both arrow kinds align on a bar.
        let correction = (hpr.floor() as i64).rem_euclid(2) as f64 * 0.5;
        let mut detail = None;
        for mark in &layout.marks {
            let fills = &layout.order[mark.fills.clone()];
            let color = match mark.side {
                OrderSide::Buy => self.trading_state.style.execution_buy,
                OrderSide::Sell => self.trading_state.style.execution_sell,
            };
            let x = (mark.x * hpr).round() + correction;
            let shape = executions[fills[fills.len() - 1]].marker_shape;
            Self::push_trading_execution_arrow(
                out,
                points,
                mark,
                shape,
                color,
                TradingChipLayout {
                    x,
                    y: mark.y,
                    hpr,
                    vpr,
                },
            );
            if detail.is_none() && fills.iter().any(focused) {
                detail = Some((mark, fills, color));
            }
        }

        let Some((mark, fills, color)) = detail else {
            return;
        };
        let tick_half = (self.time_scale.bar_spacing() * 0.5).clamp(4.0, 12.0);
        let x = (mark.x * hpr).round() as i32;
        let (arrow_top, arrow_bottom) = (mark.y - mark.height / 2.0, mark.y + mark.height / 2.0);
        // Fills may sit on either side of the arrow (a line paints only the close), so the lead
        // and tooltip span whichever side the exact prices fall on.
        let (mut fills_top, mut fills_bottom) = (arrow_top, arrow_bottom);
        let mut quantity = 0.0;
        let mut notional = 0.0;
        for &slot in fills {
            let fill = &executions[slot];
            quantity += fill.quantity;
            notional += fill.price * fill.quantity;
            let Some(fill_y) =
                self.trading_price_coordinate(pane_index, fill.price_scale, fill.price)
            else {
                continue;
            };
            fills_top = fills_top.min(fill_y);
            fills_bottom = fills_bottom.max(fill_y);
            out.push(Prim::HLine {
                y: (fill_y * vpr).round() as i32,
                x0: ((mark.x - tick_half) * hpr).round() as i32,
                x1: ((mark.x + tick_half) * hpr).round() as i32,
                width: min_line_width,
                style: LineStyle::Solid,
                color,
            });
            out.push(Prim::Circle {
                cx: (mark.x * hpr) as f32,
                cy: (fill_y * vpr) as f32,
                radius: (2.5 * vpr) as f32,
                fill: self.trading_chip_background(),
                stroke_width: (1.0 * vpr) as f32,
                stroke: color,
            });
        }
        for (y0, y1) in [(fills_top, arrow_top), (arrow_bottom, fills_bottom)] {
            if y1 > y0 {
                out.push(Prim::VLine {
                    x,
                    y0: (y0 * vpr).round() as i32,
                    y1: (y1 * vpr).round() as i32,
                    width: min_line_width,
                    style: LineStyle::Dotted,
                    color,
                });
            }
        }

        let side = match mark.side {
            OrderSide::Buy => "Buy",
            OrderSide::Sell => "Sell",
        };
        let text = if fills.len() == 1 {
            let fill = &executions[fills[0]];
            format!(
                "{side} {} @ {}",
                self.format_trading_quantity(fill.quantity),
                self.format_trading_price(pane_index, fill.price_scale, fill.price)
            )
        } else {
            format!(
                "{side} {} @ {} avg · {} fills",
                self.format_trading_quantity(quantity),
                self.format_trading_price(
                    pane_index,
                    executions[fills[0]].price_scale,
                    notional / quantity
                ),
                fills.len()
            )
        };
        let height = Self::trading_tooltip_height(self.options.get().layout.font_size);
        let above = fills_top - 4.0 - height;
        let below = fills_bottom + 4.0;
        let fits_above = above >= pane.top + 2.0;
        let fits_below = below + height <= pane.top + pane.height - 2.0;
        let y = match mark.side {
            OrderSide::Buy if fits_below || !fits_above => below,
            OrderSide::Sell if fits_above || !fits_below => above,
            OrderSide::Buy => above,
            OrderSide::Sell => below,
        };
        self.push_trading_tooltip_box(out, &text, mark.x, y, hpr, vpr);
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
        let mut points = Vec::new();
        self.build_trading_frame(pane_index, hpr, vpr, regions, lines, &mut points);
    }

    pub(super) fn build_trading_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        regions: &mut Vec<Prim>,
        lines: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let Some(pane) = self.panes.get(pane_index) else {
            return;
        };
        let width = (self.pane_w * hpr).round() as i32;
        let min_line_width = vpr.floor().max(1.0) as i32;
        let mut tooltip = None;

        if let Some(clock_micros) = self.replay_clock_micros {
            let time = if self.sequence_points().is_some() {
                clock_micros as f64 / 1_000_000.0
            } else {
                clock_micros.div_euclid(1_000_000) as f64
            };
            if let Some(index) = self.time_to_index(time, true) {
                let x = self.time_scale.index_to_coordinate(index);
                let color = Color::parse_css(match self.theme {
                    crate::ChartTheme::Light => aeris_charts_core::style::LIGHT_PRIMARY_CSS,
                    crate::ChartTheme::Dark => aeris_charts_core::style::DARK_PRIMARY_CSS,
                })
                .unwrap_or(self.trading_state.style.control);
                lines.push(Prim::VLine {
                    x: (x * hpr).round() as i32,
                    y0: (pane.top * vpr).round() as i32,
                    y1: ((pane.top + pane.height) * vpr).round() as i32,
                    width: min_line_width,
                    style: LineStyle::Dashed,
                    color,
                });
                lines.push(Prim::Text {
                    x: (x * hpr) as f32,
                    y: ((pane.top + 12.0) * vpr) as f32,
                    text: "Replay".to_string(),
                    color,
                    size: (self.options.get().layout.font_size * vpr) as f32,
                    family: self.options.get().layout.font_family.clone(),
                    align: TextAlign::Center,
                    weight: 600,
                    italic: false,
                });
            }
        }

        // Host context is a separate, non-persisted layer. Windows are lowered first and event
        // markers use deterministic LOD collapse when releases share the same pixel column.
        if !self.data.merged_times().is_empty() {
            let logical = |time: i64| self.axis_index_for_time(time).map(|index| index as i64);
            for window in &self.trading_state.host_overlay.windows {
                if !self.replay_time_is_visible(window.start_time) {
                    continue;
                }
                let end_time = self
                    .replay_cutoff_seconds()
                    .map_or(window.end_time, |cutoff| window.end_time.min(cutoff));
                let Some(start_index) = logical(window.start_time) else {
                    continue;
                };
                let Some(end_index) = logical(end_time) else {
                    continue;
                };
                let x0 = self.time_scale.index_to_coordinate(start_index);
                let x1 = self.time_scale.index_to_coordinate(end_index);
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
                if !self.replay_time_is_visible(event.time) {
                    continue;
                }
                let Some(index) = logical(event.time) else {
                    continue;
                };
                let x = self.time_scale.index_to_coordinate(index);
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

        for item in self.trading_paint_items() {
            match item {
                crate::trading::TradingPaintItem::Position(position) => {
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
                        x0: (self.trading_position_line_start(position) * hpr).round() as i32,
                        x1: (self.trading_marker_end() * hpr).round() as i32,
                        width: min_line_width,
                        style: LineStyle::Solid,
                        color: position_color,
                    });
                    if hovered.is_some() {
                        self.push_trading_endpoint(lines, y, position_color, hpr, vpr);
                    }
                    let quantity = self.trading_position_quantity_text(position);
                    let (pnl, pnl_color) = self.trading_position_detail(position);
                    let segments = [
                        TradingControlSegment {
                            kind: TradingControlSegmentKind::Quantity,
                            text: quantity.as_str(),
                            width: self.trading_quantity_width(&quantity),
                            color: position_color,
                            filled: true,
                        },
                        // The PnL text keeps its profit/loss token; the solid body around it is
                        // what carries the position's direction color.
                        TradingControlSegment {
                            kind: TradingControlSegmentKind::Pnl,
                            text: pnl.as_str(),
                            width: self.trading_value_width(&pnl),
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
                        fill: None,
                    };
                    let hovered_segment =
                        hovered.and_then(|hit| Self::trading_control_kind(hit.kind));
                    let pressed_segment =
                        pressed.and_then(|hit| Self::trading_control_kind(hit.kind));
                    self.push_trading_protection_buttons(
                        lines,
                        self.trading_position_protection_preview(position, OrderRole::TakeProfit)
                            .is_some(),
                        self.trading_position_protection_preview(position, OrderRole::StopLoss)
                            .is_some(),
                        hovered.map(|hit| hit.kind),
                        pressed.map(|hit| hit.kind),
                        TradingChipLayout {
                            x: self.trading_marker_start(),
                            y,
                            hpr,
                            vpr,
                        },
                    );
                    self.push_trading_cluster(
                        lines,
                        points,
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
                        && hovered
                            .is_some_and(|hit| hit.kind == crate::TradingHitKind::CancelButton)
                    {
                        tooltip = Some(TradingTooltip {
                            text: "Close position".to_string(),
                            layout: TradingChipLayout {
                                x: cluster.start() + cluster.width()
                                    - self.trading_close_width() / 2.0,
                                y,
                                hpr,
                                vpr,
                            },
                        });
                    }
                }
                crate::trading::TradingPaintItem::Order(order) => {
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
                    let limit_color = self.trading_order_fill(order);
                    let line_color = limit_color.unwrap_or(base_color);
                    let color = if preview.is_some() {
                        Color::rgba(line_color.r(), line_color.g(), line_color.b(), 176)
                    } else {
                        line_color
                    };
                    let hovered = self.trading_state.feedback_hover.as_ref().filter(
                |hit| matches!(&hit.object, crate::TradingObjectId::Order(id) if id == &order.id),
            );
                    let pressed = self.trading_state.feedback_pressed.as_ref().filter(
                |hit| matches!(&hit.object, crate::TradingObjectId::Order(id) if id == &order.id),
            );
                    lines.push(Prim::HLine {
                        y: (y * vpr).round() as i32,
                        x0: (self.trading_order_line_start(order) * hpr).round() as i32,
                        x1: (self.trading_marker_end() * hpr).round() as i32,
                        // Hover is communicated by the dash pattern, not a thickness jump. Dash metrics
                        // scale with stroke width in every executor, so keeping the hairline also keeps
                        // the hover dashes compact and consistent across DPRs.
                        width: min_line_width,
                        style: if creating_protection {
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
                    let main_x = self.trading_marker_start();
                    let show_take_profit = self
                        .trading_order_protection_preview(order, OrderRole::TakeProfit)
                        .is_some();
                    let show_stop_loss = self
                        .trading_order_protection_preview(order, OrderRole::StopLoss)
                        .is_some();
                    // A drag names its side ahead of every other control on the line so the
                    // pointer never hides which way the order goes. Release commits the
                    // modification directly — a host that wants a confirmation step runs it
                    // around the emitted intent, not inside the chart.
                    if preview.is_some() {
                        let width = self.trading_drag_tag_width(order.side);
                        self.push_trading_segment(
                            lines,
                            TradingControlSegment {
                                kind: TradingControlSegmentKind::Quantity,
                                text: Self::trading_side_label(order.side),
                                width,
                                color: base_color,
                                filled: true,
                            },
                            TradingControlFeedback::Idle,
                            TradingChipLayout {
                                x: self.trading_protection_buttons_start(
                                    show_take_profit,
                                    show_stop_loss,
                                ) - PROTECTION_BUTTON_GAP
                                    - width,
                                y,
                                hpr,
                                vpr,
                            },
                        );
                    }
                    let hovered_segment =
                        hovered.and_then(|hit| Self::trading_control_kind(hit.kind));
                    let pressed_segment =
                        pressed.and_then(|hit| Self::trading_control_kind(hit.kind));
                    let (detail_kind, detail_text, detail_color) = self.trading_order_detail(order);
                    // The hollow value cell stays on the chart surface, but a resting limit's
                    // order-type text uses the same strong side token as its quantity and close.
                    let detail_color = if limit_color.is_some() {
                        base_color
                    } else {
                        detail_color
                    };
                    let segments = [
                        TradingControlSegment {
                            kind: TradingControlSegmentKind::Quantity,
                            text: quantity.as_str(),
                            width: self.trading_quantity_width(&quantity),
                            color: base_color,
                            filled: true,
                        },
                        TradingControlSegment {
                            kind: detail_kind,
                            text: detail_text.as_str(),
                            width: self.trading_value_width(&detail_text),
                            color: detail_color,
                            filled: false,
                        },
                        TradingControlSegment {
                            kind: TradingControlSegmentKind::Cancel,
                            text: "",
                            width: self.trading_close_width(),
                            color: base_color,
                            filled: false,
                        },
                    ];
                    let cluster = TradingControlCluster {
                        segments: &segments,
                        left: main_x,
                        color: base_color,
                        fill: limit_color,
                    };
                    self.push_trading_protection_buttons(
                        lines,
                        show_take_profit,
                        show_stop_loss,
                        hovered.map(|hit| hit.kind),
                        pressed.map(|hit| hit.kind),
                        TradingChipLayout {
                            x: self.trading_marker_start(),
                            y,
                            hpr,
                            vpr,
                        },
                    );
                    self.push_trading_cluster(
                        lines,
                        points,
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
                    // The tooltip names exactly what the click does. An order's close control cancels the
                    // unfilled remainder (any filled part already belongs to the position), and it only
                    // acts on live orders, so pending or terminal orders get no action tooltip.
                    let cancel_label = match (order.role, order.status) {
                        (_, status)
                            if !matches!(
                                status,
                                OrderStatus::Working | OrderStatus::PartiallyFilled
                            ) =>
                        {
                            None
                        }
                        (OrderRole::TakeProfit, _) => Some("Cancel take profit".to_string()),
                        (OrderRole::StopLoss, _) => Some("Cancel stop loss".to_string()),
                        (OrderRole::Working, OrderStatus::PartiallyFilled) => Some(format!(
                            "Cancel remaining {}",
                            self.format_trading_quantity(remaining)
                        )),
                        (OrderRole::Working, _) => Some("Cancel order".to_string()),
                    };
                    if let Some(text) = cancel_label.filter(|_| {
                        self.trading_state.tooltip_armed
                            && hovered
                                .is_some_and(|hit| hit.kind == crate::TradingHitKind::CancelButton)
                    }) {
                        tooltip = Some(TradingTooltip {
                            text,
                            layout: TradingChipLayout {
                                x: cluster.start() + cluster.width()
                                    - self.trading_close_width() / 2.0,
                                y,
                                hpr,
                                vpr,
                            },
                        });
                    }
                    if order.kind == crate::OrderKind::StopLimit
                        && let Some(stop_price) =
                            order.stop_price.filter(|price| *price != display_price)
                        && let Some(stop_y) =
                            self.trading_price_coordinate(pane_index, order.price_scale, stop_price)
                    {
                        lines.push(Prim::HLine {
                            y: (stop_y * vpr).round() as i32,
                            x0: (self.trading_line_start(main_x) * hpr).round() as i32,
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
                                width: self.trading_value_width(&trigger),
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
            }
        }

        if let Some(preview) = self
            .trading_state
            .interaction
            .preview()
            .filter(|preview| preview.pane_index == pane_index)
            && let Some(preview_y) =
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
                    x0: (self.trading_line_start(self.trading_marker_start()) * hpr).round() as i32,
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
                        width: self.trading_value_width(role),
                        color: self.primary_text_color(),
                        filled: false,
                    },
                ];
                let cluster = TradingControlCluster {
                    segments: &segments,
                    left: self.trading_marker_start(),
                    color: semantic,
                    fill: None,
                };
                self.push_trading_cluster(
                    lines,
                    points,
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
            if let Some((anchor_price, long)) = self.trading_preview_relation(preview)
                && let Some(anchor_y) =
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
                    && let Some(y) = self.trading_price_coordinate(
                        pane_index,
                        position.price_scale,
                        position.average_price,
                    )
                {
                    include(y);
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
                    && let Some(y) = self.trading_price_coordinate(
                        pane_index,
                        order.price_scale,
                        self.trading_effective_order_price(order),
                    )
                {
                    include(y);
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
                        && let Some(y) = self.trading_price_coordinate(
                            pane_index,
                            position.price_scale,
                            position.average_price,
                        )
                    {
                        self.push_trading_endpoint(lines, y, connector_color, hpr, vpr);
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
                        && let Some(y) = self.trading_price_coordinate(
                            pane_index,
                            order.price_scale,
                            self.trading_effective_order_price(order),
                        )
                    {
                        self.push_trading_endpoint(lines, y, connector_color, hpr, vpr);
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
                || !self.replay_time_is_visible(entry.time)
                || !self.replay_time_is_visible(exit.time)
                || entry.pane_index != pane_index
                || exit.pane_index != pane_index
                || self.data.merged_times().is_empty()
            {
                continue;
            }
            let Some(entry_logical) = self
                .axis_index_for_time(entry.time)
                .map(|index| index as i64)
            else {
                continue;
            };
            let Some(exit_logical) = self
                .axis_index_for_time(exit.time)
                .map(|index| index as i64)
            else {
                continue;
            };
            let entry_x = self.time_scale.index_to_coordinate(entry_logical);
            let exit_x = self.time_scale.index_to_coordinate(exit_logical);
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
        self.push_trading_executions(lines, points, pane, pane_index, hpr, vpr);
        if let Some(tooltip) = tooltip {
            self.push_trading_tooltip(lines, &tooltip.text, pane, tooltip.layout);
        }
    }
}
