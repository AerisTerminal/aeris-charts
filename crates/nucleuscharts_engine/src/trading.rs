//! First-class, broker-neutral trading objects.
//!
//! The host owns authoritative broker state. This module owns only chart-local semantic state,
//! validation, geometry inputs, and dedicated hit identities. Live trading state is deliberately
//! absent from drawing persistence.

use std::collections::{HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::{ChartEngine, ChartError, ErrorCode, PriceScaleTarget, PANELESS};
use nucleuscharts_core::style::{DEFAULT_PRIMARY_RGB, MARKET_DOWN_RGB, MARKET_UP_RGB};
use nucleuscharts_render::color::Color;

pub const MAX_TRADING_OBJECTS: usize = 4_096;
const MAX_TRADING_ID_BYTES: usize = 128;

macro_rules! trading_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ChartError> {
                let value = value.into();
                validate_id(stringify!($name), &value)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            fn heap_bytes(&self) -> usize {
                self.0.capacity()
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

trading_id!(PositionId);
trading_id!(OrderId);
trading_id!(ExecutionId);
trading_id!(TradingGroupId);

fn validate_id(kind: &str, value: &str) -> Result<(), ChartError> {
    if value.is_empty() || value.len() > MAX_TRADING_ID_BYTES {
        return Err(ChartError::new(
            ErrorCode::InvalidData,
            format!("{kind} must contain 1..={MAX_TRADING_ID_BYTES} UTF-8 bytes"),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionSide {
    Long,
    Short,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderKind {
    Limit,
    Stop,
    StopLimit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderRole {
    #[default]
    Working,
    StopLoss,
    TakeProfit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    PendingSubmit,
    Working,
    PendingModify,
    PartiallyFilled,
    Filled,
    PendingCancel,
    Cancelled,
    Rejected,
    Expired,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradingPriceScale {
    #[default]
    Right,
    Left,
    Overlay,
}

impl From<TradingPriceScale> for PriceScaleTarget {
    fn from(value: TradingPriceScale) -> Self {
        match value {
            TradingPriceScale::Right => Self::Right,
            TradingPriceScale::Left => Self::Left,
            TradingPriceScale::Overlay => Self::Overlay,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InstrumentMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tick_size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_precision: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity_precision: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_quantity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TradingPosition {
    pub id: PositionId,
    #[serde(default)]
    pub pane_index: usize,
    #[serde(default)]
    pub price_scale: TradingPriceScale,
    pub side: PositionSide,
    pub average_price: f64,
    pub quantity: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_pnl: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkingOrder {
    pub id: OrderId,
    #[serde(default)]
    pub pane_index: usize,
    #[serde(default)]
    pub price_scale: TradingPriceScale,
    pub side: OrderSide,
    pub kind: OrderKind,
    #[serde(default)]
    pub role: OrderRole,
    pub status: OrderStatus,
    pub price: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<f64>,
    pub quantity: f64,
    #[serde(default)]
    pub filled_quantity: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_id: Option<PositionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_order_id: Option<OrderId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bracket_id: Option<TradingGroupId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oco_group_id: Option<TradingGroupId>,
    #[serde(default)]
    pub revision: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionKind {
    Entry,
    PartialFill,
    Exit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TradingExecution {
    pub id: ExecutionId,
    #[serde(default)]
    pub pane_index: usize,
    #[serde(default)]
    pub price_scale: TradingPriceScale,
    pub side: OrderSide,
    pub kind: ExecutionKind,
    pub time: i64,
    pub price: f64,
    pub quantity: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order_id: Option<OrderId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_id: Option<PositionId>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TradingSnapshot {
    pub instrument: InstrumentMetadata,
    pub positions: Vec<TradingPosition>,
    pub orders: Vec<WorkingOrder>,
    pub executions: Vec<TradingExecution>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradingStyle {
    pub position: Color,
    pub working_order: Color,
    pub buy: Color,
    pub sell: Color,
    pub profit: Color,
    pub risk: Color,
    pub take_profit: Color,
    pub stop_loss: Color,
    pub pending: Color,
    pub rejected: Color,
    pub control: Color,
    pub label: Color,
}

impl Default for TradingStyle {
    fn default() -> Self {
        let primary = Color::rgb(
            DEFAULT_PRIMARY_RGB.0,
            DEFAULT_PRIMARY_RGB.1,
            DEFAULT_PRIMARY_RGB.2,
        );
        let buy = Color::rgb(MARKET_UP_RGB.0, MARKET_UP_RGB.1, MARKET_UP_RGB.2);
        let sell = Color::rgb(MARKET_DOWN_RGB.0, MARKET_DOWN_RGB.1, MARKET_DOWN_RGB.2);
        Self {
            position: primary,
            working_order: primary,
            buy,
            sell,
            profit: buy,
            risk: sell,
            take_profit: buy,
            stop_loss: sell,
            pending: Color::rgb(0xf5, 0xa6, 0x23),
            rejected: Color::rgb(0x78, 0x7b, 0x86),
            control: primary,
            label: Color::rgb(0xff, 0xff, 0xff),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TradingStyleOptions {
    pub position: Option<String>,
    pub working_order: Option<String>,
    pub buy: Option<String>,
    pub sell: Option<String>,
    pub profit: Option<String>,
    pub risk: Option<String>,
    pub take_profit: Option<String>,
    pub stop_loss: Option<String>,
    pub pending: Option<String>,
    pub rejected: Option<String>,
    pub control: Option<String>,
    pub label: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradingIntentAction {
    ModifyOrder,
    CancelOrder,
    CreateStopLoss,
    CreateTakeProfit,
    ClosePosition,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TradingIntent {
    pub sequence: u32,
    pub action: TradingIntentAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<OrderId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_id: Option<PositionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<OrderSide>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<OrderKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<OrderRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bracket_id: Option<TradingGroupId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oco_group_id: Option<TradingGroupId>,
    pub base_revision: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradingPreviewPhase {
    Dragging,
    Pending,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum TradingPreviewSource {
    Order { order_id: OrderId },
    StopLoss { position_id: PositionId },
    TakeProfit { position_id: PositionId },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TradingPreview {
    #[serde(flatten)]
    pub source: TradingPreviewSource,
    pub phase: TradingPreviewPhase,
    pub pane_index: usize,
    pub price_scale: TradingPriceScale,
    pub price: f64,
    pub quantity: f64,
    pub side: OrderSide,
    pub role: OrderRole,
    pub base_revision: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent_sequence: Option<u32>,
}

const MAX_PENDING_TRADING_INTENTS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingPositionAction {
    pub position_id: PositionId,
    pub sequence: u32,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TradingState {
    pub instrument: InstrumentMetadata,
    pub positions: Vec<TradingPosition>,
    pub orders: Vec<WorkingOrder>,
    pub executions: Vec<TradingExecution>,
    pub style: TradingStyle,
    pub preview: Option<TradingPreview>,
    pub hover: Option<TradingHit>,
    pub pending_position_action: Option<PendingPositionAction>,
    intents: VecDeque<TradingIntent>,
    next_intent_sequence: u32,
}

impl TradingState {
    pub(crate) fn snapshot(&self) -> TradingSnapshot {
        TradingSnapshot {
            instrument: self.instrument.clone(),
            positions: self.positions.clone(),
            orders: self.orders.clone(),
            executions: self.executions.clone(),
        }
    }

    fn next_sequence(&mut self) -> u32 {
        self.next_intent_sequence = self.next_intent_sequence.wrapping_add(1).max(1);
        self.next_intent_sequence
    }

    fn push_intent(&mut self, intent: TradingIntent) {
        if self.intents.len() == MAX_PENDING_TRADING_INTENTS {
            self.intents.pop_front();
        }
        self.intents.push_back(intent);
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        let retained_strings = self
            .positions
            .iter()
            .map(|value| {
                value.id.heap_bytes() + value.currency.as_ref().map_or(0, |value| value.capacity())
            })
            .sum::<usize>()
            + self
                .orders
                .iter()
                .map(|value| {
                    value.id.heap_bytes()
                        + value.position_id.as_ref().map_or(0, PositionId::heap_bytes)
                        + value
                            .parent_order_id
                            .as_ref()
                            .map_or(0, OrderId::heap_bytes)
                        + value
                            .bracket_id
                            .as_ref()
                            .map_or(0, TradingGroupId::heap_bytes)
                        + value
                            .oco_group_id
                            .as_ref()
                            .map_or(0, TradingGroupId::heap_bytes)
                })
                .sum::<usize>()
            + self
                .executions
                .iter()
                .map(|value| {
                    value.id.heap_bytes()
                        + value.order_id.as_ref().map_or(0, OrderId::heap_bytes)
                        + value.position_id.as_ref().map_or(0, PositionId::heap_bytes)
                })
                .sum::<usize>()
            + self
                .instrument
                .currency
                .as_ref()
                .map_or(0, |value| value.capacity());
        self.positions.capacity() * std::mem::size_of::<TradingPosition>()
            + self.orders.capacity() * std::mem::size_of::<WorkingOrder>()
            + self.executions.capacity() * std::mem::size_of::<TradingExecution>()
            + self.intents.capacity() * std::mem::size_of::<TradingIntent>()
            + retained_strings
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TradingObjectId {
    Position(PositionId),
    Order(OrderId),
    Execution(ExecutionId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradingHitKind {
    PositionLine,
    OrderLine,
    QuantityLabel,
    CancelButton,
    CreateStopButton,
    CreateTargetButton,
    ExecutionMarker,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TradingHit {
    pub object: TradingObjectId,
    pub kind: TradingHitKind,
    pub distance: f64,
}

fn invalid(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::InvalidData, message)
}

fn validate_instrument(instrument: &InstrumentMetadata) -> Result<(), ChartError> {
    for (name, value) in [
        ("tick_size", instrument.tick_size),
        ("minimum_quantity", instrument.minimum_quantity),
        ("point_value", instrument.point_value),
    ] {
        if value.is_some_and(|value| !value.is_finite() || value <= 0.0) {
            return Err(invalid(format!(
                "instrument {name} must be finite and positive"
            )));
        }
    }
    if instrument.price_precision.is_some_and(|value| value > 15)
        || instrument
            .quantity_precision
            .is_some_and(|value| value > 15)
    {
        return Err(invalid("instrument precision must be in 0..=15"));
    }
    if instrument
        .currency
        .as_ref()
        .is_some_and(|value| value.len() > 16)
    {
        return Err(invalid("instrument currency is too long"));
    }
    Ok(())
}

fn parse_style_color(name: &str, value: Option<&str>) -> Result<Option<Color>, ChartError> {
    value
        .map(|value| {
            Color::parse_css(value).ok_or_else(|| {
                invalid(format!("trading style {name} is not a supported CSS color"))
            })
        })
        .transpose()
}

fn validate_position(position: &TradingPosition) -> Result<(), ChartError> {
    validate_id("PositionId", position.id.as_str())?;
    if !position.average_price.is_finite()
        || !position.quantity.is_finite()
        || position.quantity <= 0.0
    {
        return Err(invalid(
            "position price must be finite and quantity positive",
        ));
    }
    if position.display_pnl.is_some_and(|value| !value.is_finite()) {
        return Err(invalid("position display_pnl must be finite"));
    }
    Ok(())
}

fn validate_order(order: &WorkingOrder) -> Result<(), ChartError> {
    validate_id("OrderId", order.id.as_str())?;
    if !order.price.is_finite()
        || order.stop_price.is_some_and(|value| !value.is_finite())
        || !order.quantity.is_finite()
        || order.quantity <= 0.0
        || !order.filled_quantity.is_finite()
        || order.filled_quantity < 0.0
        || order.filled_quantity > order.quantity
    {
        return Err(invalid("order prices and quantities are invalid"));
    }
    Ok(())
}

fn validate_execution(execution: &TradingExecution) -> Result<(), ChartError> {
    validate_id("ExecutionId", execution.id.as_str())?;
    if !execution.price.is_finite() || !execution.quantity.is_finite() || execution.quantity <= 0.0
    {
        return Err(invalid(
            "execution price must be finite and quantity positive",
        ));
    }
    Ok(())
}

fn validate_unique<'a>(
    values: impl Iterator<Item = &'a str>,
    kind: &str,
) -> Result<(), ChartError> {
    let mut seen = HashSet::new();
    for value in values {
        if !seen.insert(value) {
            return Err(invalid(format!("duplicate {kind} id '{value}'")));
        }
    }
    Ok(())
}

impl ChartEngine {
    pub fn trading_hit_at(&self, x_css: f64, y_css: f64) -> Option<TradingHit> {
        if !x_css.is_finite() || !y_css.is_finite() || x_css < 0.0 || x_css > self.pane_w {
            return None;
        }
        let pane_index = self.pane_at_y(y_css)?;
        const LINE_TOLERANCE: f64 = 6.0;
        const CONTROL_WIDTH: f64 = 28.0;

        for order in self.trading_state.orders.iter().rev() {
            if order.pane_index != pane_index {
                continue;
            }
            let Some(y) = self.trading_price_coordinate(pane_index, order.price_scale, order.price)
            else {
                continue;
            };
            let distance = (y_css - y).abs();
            if distance > LINE_TOLERANCE {
                continue;
            }
            return Some(TradingHit {
                object: TradingObjectId::Order(order.id.clone()),
                kind: if x_css >= self.pane_w - CONTROL_WIDTH {
                    TradingHitKind::CancelButton
                } else {
                    TradingHitKind::OrderLine
                },
                distance,
            });
        }

        for position in self.trading_state.positions.iter().rev() {
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
            let distance = (y_css - y).abs();
            if distance > LINE_TOLERANCE {
                continue;
            }
            let from_right = self.pane_w - x_css;
            let kind = if from_right <= CONTROL_WIDTH {
                TradingHitKind::CancelButton
            } else if from_right <= CONTROL_WIDTH * 2.0 {
                TradingHitKind::CreateTargetButton
            } else if from_right <= CONTROL_WIDTH * 3.0 {
                TradingHitKind::CreateStopButton
            } else if from_right <= 180.0 {
                TradingHitKind::QuantityLabel
            } else {
                TradingHitKind::PositionLine
            };
            return Some(TradingHit {
                object: TradingObjectId::Position(position.id.clone()),
                kind,
                distance,
            });
        }

        let times = self.data.merged_times();
        for execution in self.trading_state.executions.iter().rev() {
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
            let distance = (x_css - x).hypot(y_css - y);
            if distance <= 10.0 {
                return Some(TradingHit {
                    object: TradingObjectId::Execution(execution.id.clone()),
                    kind: TradingHitKind::ExecutionMarker,
                    distance,
                });
            }
        }
        None
    }

    pub fn set_trading_hover(&mut self, x_css: f64, y_css: f64) -> bool {
        if self.trading_state.preview.is_some() {
            return false;
        }
        let next = self.trading_hit_at(x_css, y_css);
        if self.trading_state.hover == next {
            return false;
        }
        self.trading_state.hover = next;
        self.invalidate_frame_trading();
        true
    }

    pub fn clear_trading_hover(&mut self) -> bool {
        if self.trading_state.hover.take().is_none() {
            return false;
        }
        self.invalidate_frame_trading();
        true
    }

    fn snap_trading_price(&self, price: f64) -> f64 {
        let Some(tick) = self.trading_state.instrument.tick_size else {
            return price;
        };
        (price / tick).round() * tick
    }

    pub fn trading_drag_start_at(&mut self, x_css: f64, y_css: f64) -> bool {
        if self.trading_state.preview.is_some()
            || self.trading_state.pending_position_action.is_some()
        {
            return true;
        }
        let Some(hit) = self.trading_hit_at(x_css, y_css) else {
            return false;
        };
        let preview = match (&hit.object, hit.kind) {
            (TradingObjectId::Order(id), TradingHitKind::OrderLine) => {
                let Some(order) = self
                    .trading_state
                    .orders
                    .iter()
                    .find(|order| &order.id == id)
                else {
                    return false;
                };
                if !matches!(
                    order.status,
                    OrderStatus::Working | OrderStatus::PartiallyFilled
                ) {
                    return false;
                }
                TradingPreview {
                    source: TradingPreviewSource::Order {
                        order_id: id.clone(),
                    },
                    phase: TradingPreviewPhase::Dragging,
                    pane_index: order.pane_index,
                    price_scale: order.price_scale,
                    price: order.price,
                    quantity: (order.quantity - order.filled_quantity).max(0.0),
                    side: order.side,
                    role: order.role,
                    base_revision: order.revision,
                    intent_sequence: None,
                }
            }
            (TradingObjectId::Position(id), TradingHitKind::CreateStopButton)
            | (TradingObjectId::Position(id), TradingHitKind::CreateTargetButton) => {
                let Some(position) = self
                    .trading_state
                    .positions
                    .iter()
                    .find(|position| &position.id == id)
                else {
                    return false;
                };
                let role = if hit.kind == TradingHitKind::CreateStopButton {
                    OrderRole::StopLoss
                } else {
                    OrderRole::TakeProfit
                };
                if self
                    .trading_state
                    .orders
                    .iter()
                    .any(|order| order.position_id.as_ref() == Some(id) && order.role == role)
                {
                    return false;
                }
                TradingPreview {
                    source: if role == OrderRole::StopLoss {
                        TradingPreviewSource::StopLoss {
                            position_id: id.clone(),
                        }
                    } else {
                        TradingPreviewSource::TakeProfit {
                            position_id: id.clone(),
                        }
                    },
                    phase: TradingPreviewPhase::Dragging,
                    pane_index: position.pane_index,
                    price_scale: position.price_scale,
                    price: position.average_price,
                    quantity: position.quantity,
                    side: match position.side {
                        PositionSide::Long => OrderSide::Sell,
                        PositionSide::Short => OrderSide::Buy,
                    },
                    role,
                    base_revision: 0,
                    intent_sequence: None,
                }
            }
            _ => return false,
        };
        self.trading_state.hover = Some(hit);
        self.trading_state.preview = Some(preview);
        self.invalidate_frame_trading();
        true
    }

    pub fn trading_drag_to(&mut self, y_css: f64) -> bool {
        let Some(preview) = self.trading_state.preview.as_ref() else {
            return false;
        };
        if preview.phase != TradingPreviewPhase::Dragging || !y_css.is_finite() {
            return false;
        }
        let Some(price) =
            self.trading_coordinate_to_price(preview.pane_index, preview.price_scale, y_css)
        else {
            return false;
        };
        let price = self.snap_trading_price(price);
        if !price.is_finite() || price <= 0.0 || preview.price == price {
            return false;
        }
        self.trading_state
            .preview
            .as_mut()
            .expect("preview exists")
            .price = price;
        self.invalidate_frame_trading();
        true
    }

    pub fn trading_drag_end(&mut self) -> Option<TradingIntent> {
        let preview = self.trading_state.preview.as_ref()?;
        if preview.phase != TradingPreviewPhase::Dragging {
            return None;
        }
        let start_price = match &preview.source {
            TradingPreviewSource::Order { order_id } => self
                .trading_state
                .orders
                .iter()
                .find(|order| &order.id == order_id)
                .map(|order| order.price),
            TradingPreviewSource::StopLoss { position_id }
            | TradingPreviewSource::TakeProfit { position_id } => self
                .trading_state
                .positions
                .iter()
                .find(|position| &position.id == position_id)
                .map(|position| position.average_price),
        };
        let tolerance = self
            .trading_state
            .instrument
            .tick_size
            .unwrap_or(f64::EPSILON)
            * 0.5;
        if start_price.is_none_or(|price| (price - preview.price).abs() <= tolerance) {
            self.trading_state.preview = None;
            self.invalidate_frame_trading();
            return None;
        }
        let valid_side = match &preview.source {
            TradingPreviewSource::Order { .. } => true,
            TradingPreviewSource::StopLoss { position_id }
            | TradingPreviewSource::TakeProfit { position_id } => self
                .trading_state
                .positions
                .iter()
                .find(|position| &position.id == position_id)
                .is_some_and(|position| match (position.side, preview.role) {
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
                }),
        };
        if !valid_side {
            self.trading_state.preview = None;
            self.invalidate_frame_trading();
            return None;
        }
        let sequence = self.trading_state.next_sequence();
        let preview = self.trading_state.preview.as_mut().expect("preview exists");
        preview.phase = TradingPreviewPhase::Pending;
        preview.intent_sequence = Some(sequence);
        let (action, order_id, position_id, kind) = match &preview.source {
            TradingPreviewSource::Order { order_id } => (
                TradingIntentAction::ModifyOrder,
                Some(order_id.clone()),
                None,
                None,
            ),
            TradingPreviewSource::StopLoss { position_id } => (
                TradingIntentAction::CreateStopLoss,
                None,
                Some(position_id.clone()),
                Some(OrderKind::Stop),
            ),
            TradingPreviewSource::TakeProfit { position_id } => (
                TradingIntentAction::CreateTakeProfit,
                None,
                Some(position_id.clone()),
                Some(OrderKind::Limit),
            ),
        };
        let intent = TradingIntent {
            sequence,
            action,
            order_id,
            position_id,
            side: Some(preview.side),
            kind,
            role: Some(preview.role),
            price: Some(preview.price),
            stop_price: None,
            quantity: Some(preview.quantity),
            bracket_id: None,
            oco_group_id: None,
            base_revision: preview.base_revision,
        };
        self.trading_state.push_intent(intent.clone());
        self.invalidate_frame_trading();
        Some(intent)
    }

    pub fn cancel_trading_drag(&mut self) -> bool {
        if self
            .trading_state
            .preview
            .as_ref()
            .is_none_or(|preview| preview.phase != TradingPreviewPhase::Dragging)
        {
            return false;
        }
        self.trading_state.preview = None;
        self.invalidate_frame_trading();
        true
    }

    pub fn trading_activate_at(&mut self, x_css: f64, y_css: f64) -> Option<TradingIntent> {
        if self.trading_state.preview.is_some()
            || self.trading_state.pending_position_action.is_some()
        {
            return None;
        }
        let hit = self.trading_hit_at(x_css, y_css)?;
        if hit.kind != TradingHitKind::CancelButton {
            return None;
        }
        let sequence = self.trading_state.next_sequence();
        let (intent, pending_preview) = match hit.object {
            TradingObjectId::Order(order_id) => {
                let order = self
                    .trading_state
                    .orders
                    .iter()
                    .find(|order| order.id == order_id)?;
                if !matches!(
                    order.status,
                    OrderStatus::Working | OrderStatus::PartiallyFilled
                ) {
                    return None;
                }
                let remaining = (order.quantity - order.filled_quantity).max(0.0);
                (
                    TradingIntent {
                        sequence,
                        action: TradingIntentAction::CancelOrder,
                        order_id: Some(order_id.clone()),
                        position_id: order.position_id.clone(),
                        side: Some(order.side),
                        kind: Some(order.kind),
                        role: Some(order.role),
                        price: Some(order.price),
                        stop_price: order.stop_price,
                        quantity: Some(remaining),
                        bracket_id: order.bracket_id.clone(),
                        oco_group_id: order.oco_group_id.clone(),
                        base_revision: order.revision,
                    },
                    Some(TradingPreview {
                        source: TradingPreviewSource::Order { order_id },
                        phase: TradingPreviewPhase::Pending,
                        pane_index: order.pane_index,
                        price_scale: order.price_scale,
                        price: order.price,
                        quantity: remaining,
                        side: order.side,
                        role: order.role,
                        base_revision: order.revision,
                        intent_sequence: Some(sequence),
                    }),
                )
            }
            TradingObjectId::Position(position_id) => (
                TradingIntent {
                    sequence,
                    action: TradingIntentAction::ClosePosition,
                    order_id: None,
                    position_id: Some(position_id),
                    side: None,
                    kind: None,
                    role: None,
                    price: None,
                    stop_price: None,
                    quantity: None,
                    bracket_id: None,
                    oco_group_id: None,
                    base_revision: 0,
                },
                None,
            ),
            TradingObjectId::Execution(_) => return None,
        };
        if intent.action == TradingIntentAction::ClosePosition {
            self.trading_state.pending_position_action =
                intent
                    .position_id
                    .clone()
                    .map(|position_id| PendingPositionAction {
                        position_id,
                        sequence,
                    });
        }
        self.trading_state.preview = pending_preview;
        self.trading_state.push_intent(intent.clone());
        self.invalidate_frame_trading();
        Some(intent)
    }

    pub fn resolve_trading_intent(&mut self, sequence: u32, accepted: bool) -> bool {
        let preview_matches = self
            .trading_state
            .preview
            .as_ref()
            .is_some_and(|preview| preview.intent_sequence == Some(sequence));
        let position_matches = self
            .trading_state
            .pending_position_action
            .as_ref()
            .is_some_and(|pending| pending.sequence == sequence);
        if (!preview_matches && !position_matches) || accepted {
            return preview_matches || position_matches;
        }
        if preview_matches {
            self.trading_state.preview = None;
        }
        if position_matches {
            self.trading_state.pending_position_action = None;
        }
        self.invalidate_frame_trading();
        true
    }

    pub fn trading_preview(&self) -> Option<&TradingPreview> {
        self.trading_state.preview.as_ref()
    }

    pub fn take_trading_intents(&mut self) -> Vec<TradingIntent> {
        self.trading_state.intents.drain(..).collect()
    }

    pub fn trading_snapshot(&self) -> TradingSnapshot {
        self.trading_state.snapshot()
    }

    pub fn set_trading_snapshot(&mut self, snapshot: TradingSnapshot) -> Result<(), ChartError> {
        let total = snapshot.positions.len() + snapshot.orders.len() + snapshot.executions.len();
        if total > MAX_TRADING_OBJECTS {
            return Err(ChartError::new(
                ErrorCode::ResourceLimit,
                format!("trading snapshot exceeds {MAX_TRADING_OBJECTS} objects"),
            ));
        }
        validate_instrument(&snapshot.instrument)?;
        for position in &snapshot.positions {
            validate_position(position)?;
        }
        for order in &snapshot.orders {
            validate_order(order)?;
        }
        for execution in &snapshot.executions {
            validate_execution(execution)?;
        }
        validate_unique(
            snapshot.positions.iter().map(|value| value.id.as_str()),
            "position",
        )?;
        validate_unique(
            snapshot.orders.iter().map(|value| value.id.as_str()),
            "order",
        )?;
        validate_unique(
            snapshot.executions.iter().map(|value| value.id.as_str()),
            "execution",
        )?;
        let prior = std::mem::take(&mut self.trading_state);
        self.trading_state = TradingState {
            instrument: snapshot.instrument,
            positions: snapshot.positions,
            orders: snapshot.orders,
            executions: snapshot.executions,
            style: prior.style,
            preview: prior.preview,
            hover: prior.hover,
            pending_position_action: prior.pending_position_action,
            intents: prior.intents,
            next_intent_sequence: prior.next_intent_sequence,
        };
        self.reconcile_trading_preview();
        self.reconcile_pending_position_action();
        self.invalidate_frame_trading();
        Ok(())
    }

    pub fn update_trading_position(&mut self, position: TradingPosition) -> Result<(), ChartError> {
        validate_position(&position)?;
        if let Some(index) = self
            .trading_state
            .positions
            .iter()
            .position(|value| value.id == position.id)
        {
            self.trading_state.positions[index] = position;
        } else {
            self.ensure_trading_capacity(1)?;
            self.trading_state.positions.push(position);
        }
        self.reconcile_trading_preview();
        self.invalidate_frame_trading();
        Ok(())
    }

    pub fn remove_trading_position(&mut self, id: &PositionId) -> bool {
        let before = self.trading_state.positions.len();
        self.trading_state.positions.retain(|value| &value.id != id);
        let changed = before != self.trading_state.positions.len();
        if changed {
            if self.trading_state.preview.as_ref().is_some_and(|preview| {
                matches!(
                    &preview.source,
                    TradingPreviewSource::StopLoss { position_id }
                        | TradingPreviewSource::TakeProfit { position_id }
                        if position_id == id
                )
            }) {
                self.trading_state.preview = None;
            }
            if self
                .trading_state
                .pending_position_action
                .as_ref()
                .is_some_and(|pending| &pending.position_id == id)
            {
                self.trading_state.pending_position_action = None;
            }
            self.invalidate_frame_trading();
        }
        changed
    }

    pub fn update_working_order(&mut self, order: WorkingOrder) -> Result<(), ChartError> {
        validate_order(&order)?;
        if let Some(index) = self
            .trading_state
            .orders
            .iter()
            .position(|value| value.id == order.id)
        {
            self.trading_state.orders[index] = order;
        } else {
            self.ensure_trading_capacity(1)?;
            self.trading_state.orders.push(order);
        }
        self.reconcile_trading_preview();
        self.invalidate_frame_trading();
        Ok(())
    }

    pub fn remove_working_order(&mut self, id: &OrderId) -> bool {
        let before = self.trading_state.orders.len();
        self.trading_state.orders.retain(|value| &value.id != id);
        let changed = before != self.trading_state.orders.len();
        if changed {
            if self.trading_state.preview.as_ref().is_some_and(|preview| {
                matches!(
                    &preview.source,
                    TradingPreviewSource::Order { order_id } if order_id == id
                )
            }) {
                self.trading_state.preview = None;
            }
            self.invalidate_frame_trading();
        }
        changed
    }

    pub fn apply_trading_execution(
        &mut self,
        execution: TradingExecution,
    ) -> Result<(), ChartError> {
        validate_execution(&execution)?;
        if let Some(index) = self
            .trading_state
            .executions
            .iter()
            .position(|value| value.id == execution.id)
        {
            self.trading_state.executions[index] = execution;
        } else {
            self.ensure_trading_capacity(1)?;
            self.trading_state.executions.push(execution);
        }
        self.invalidate_frame_trading();
        Ok(())
    }

    pub fn remove_trading_execution(&mut self, id: &ExecutionId) -> bool {
        let before = self.trading_state.executions.len();
        self.trading_state
            .executions
            .retain(|value| &value.id != id);
        let changed = before != self.trading_state.executions.len();
        if changed {
            self.invalidate_frame_trading();
        }
        changed
    }

    pub fn set_instrument_metadata(
        &mut self,
        instrument: InstrumentMetadata,
    ) -> Result<(), ChartError> {
        validate_instrument(&instrument)?;
        self.trading_state.instrument = instrument;
        self.invalidate_frame_trading();
        Ok(())
    }

    pub fn trading_style(&self) -> TradingStyle {
        self.trading_state.style
    }

    pub fn apply_trading_style(&mut self, options: TradingStyleOptions) -> Result<(), ChartError> {
        let mut style = self.trading_state.style;
        macro_rules! apply {
            ($field:ident) => {
                if let Some(color) =
                    parse_style_color(stringify!($field), options.$field.as_deref())?
                {
                    style.$field = color;
                }
            };
        }
        apply!(position);
        apply!(working_order);
        apply!(buy);
        apply!(sell);
        apply!(profit);
        apply!(risk);
        apply!(take_profit);
        apply!(stop_loss);
        apply!(pending);
        apply!(rejected);
        apply!(control);
        apply!(label);
        self.trading_state.style = style;
        self.invalidate_frame_trading();
        Ok(())
    }

    pub(crate) fn remove_trading_pane(&mut self, index: usize) {
        for pane in self
            .trading_state
            .positions
            .iter_mut()
            .map(|value| &mut value.pane_index)
            .chain(
                self.trading_state
                    .orders
                    .iter_mut()
                    .map(|value| &mut value.pane_index),
            )
            .chain(
                self.trading_state
                    .executions
                    .iter_mut()
                    .map(|value| &mut value.pane_index),
            )
        {
            if *pane == index {
                *pane = PANELESS;
            } else if *pane != PANELESS && *pane > index {
                *pane -= 1;
            }
        }
    }

    fn ensure_trading_capacity(&self, additional: usize) -> Result<(), ChartError> {
        let total = self.trading_state.positions.len()
            + self.trading_state.orders.len()
            + self.trading_state.executions.len();
        if total.saturating_add(additional) > MAX_TRADING_OBJECTS {
            return Err(ChartError::new(
                ErrorCode::ResourceLimit,
                format!("trading state exceeds {MAX_TRADING_OBJECTS} objects"),
            ));
        }
        Ok(())
    }

    fn reconcile_trading_preview(&mut self) {
        let Some(preview) = self.trading_state.preview.as_ref() else {
            return;
        };
        if preview.phase == TradingPreviewPhase::Dragging {
            return;
        }
        let tolerance = self
            .trading_state
            .instrument
            .tick_size
            .unwrap_or(f64::EPSILON)
            * 0.5;
        let reconciled = match &preview.source {
            TradingPreviewSource::Order { order_id } => self
                .trading_state
                .orders
                .iter()
                .find(|order| &order.id == order_id)
                .is_none_or(|order| {
                    order.revision > preview.base_revision
                        || (order.price - preview.price).abs() <= tolerance
                }),
            TradingPreviewSource::StopLoss { position_id } => {
                !self
                    .trading_state
                    .positions
                    .iter()
                    .any(|position| &position.id == position_id)
                    || self.trading_state.orders.iter().any(|order| {
                        order.position_id.as_ref() == Some(position_id)
                            && order.role == OrderRole::StopLoss
                            && (order.price - preview.price).abs() <= tolerance
                    })
            }
            TradingPreviewSource::TakeProfit { position_id } => {
                !self
                    .trading_state
                    .positions
                    .iter()
                    .any(|position| &position.id == position_id)
                    || self.trading_state.orders.iter().any(|order| {
                        order.position_id.as_ref() == Some(position_id)
                            && order.role == OrderRole::TakeProfit
                            && (order.price - preview.price).abs() <= tolerance
                    })
            }
        };
        if reconciled {
            self.trading_state.preview = None;
        }
    }

    fn reconcile_pending_position_action(&mut self) {
        let removed = self
            .trading_state
            .pending_position_action
            .as_ref()
            .is_some_and(|pending| {
                !self
                    .trading_state
                    .positions
                    .iter()
                    .any(|position| position.id == pending.position_id)
            });
        if removed {
            self.trading_state.pending_position_action = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChartEngine, TradingPriceScale};
    use nucleuscharts_render::draw_list::Prim;

    fn id<T>(value: &str, constructor: impl FnOnce(String) -> Result<T, ChartError>) -> T {
        constructor(value.to_string()).unwrap()
    }

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
        chart
    }

    fn position(side: PositionSide) -> TradingPosition {
        TradingPosition {
            id: id("position-1", PositionId::new),
            pane_index: 0,
            price_scale: TradingPriceScale::Right,
            side,
            average_price: 101.0,
            quantity: 12.0,
            display_pnl: None,
            currency: None,
        }
    }

    fn order(id_value: &str, role: OrderRole, price: f64) -> WorkingOrder {
        WorkingOrder {
            id: id(id_value, OrderId::new),
            pane_index: 0,
            price_scale: TradingPriceScale::Right,
            side: OrderSide::Sell,
            kind: OrderKind::Limit,
            role,
            status: OrderStatus::Working,
            price,
            stop_price: None,
            quantity: 12.0,
            filled_quantity: 0.0,
            position_id: Some(id("position-1", PositionId::new)),
            parent_order_id: None,
            bracket_id: Some(id("bracket-1", TradingGroupId::new)),
            oco_group_id: Some(id("oco-1", TradingGroupId::new)),
            revision: 1,
        }
    }

    #[test]
    fn snapshot_is_transactional_bounded_and_incremental() {
        let mut chart = chart_with_market();
        chart
            .set_trading_snapshot(TradingSnapshot {
                positions: vec![position(PositionSide::Long)],
                orders: vec![order("tp-1", OrderRole::TakeProfit, 103.0)],
                ..TradingSnapshot::default()
            })
            .unwrap();
        let before = chart.trading_snapshot();
        let mut invalid = before.clone();
        invalid.orders[0].filled_quantity = 20.0;
        assert!(chart.set_trading_snapshot(invalid).is_err());
        assert_eq!(chart.trading_snapshot(), before);

        let mut moved = position(PositionSide::Long);
        moved.average_price = 102.0;
        chart.update_trading_position(moved).unwrap();
        assert_eq!(chart.trading_snapshot().positions.len(), 1);
        assert_eq!(chart.trading_snapshot().positions[0].average_price, 102.0);
        assert!(chart.remove_trading_position(&id("position-1", PositionId::new)));
        assert!(chart.trading_snapshot().positions.is_empty());
    }

    #[test]
    fn trading_frame_owns_regions_lines_labels_executions_and_axis_tags() {
        let mut chart = chart_with_market();
        chart
            .set_trading_snapshot(TradingSnapshot {
                instrument: InstrumentMetadata {
                    point_value: Some(1.0),
                    currency: Some("USD".to_string()),
                    ..InstrumentMetadata::default()
                },
                positions: vec![position(PositionSide::Long)],
                orders: vec![
                    order("tp-1", OrderRole::TakeProfit, 103.0),
                    order("sl-1", OrderRole::StopLoss, 99.0),
                ],
                executions: vec![TradingExecution {
                    id: id("fill-1", ExecutionId::new),
                    pane_index: 0,
                    price_scale: TradingPriceScale::Right,
                    side: OrderSide::Buy,
                    kind: ExecutionKind::Entry,
                    time: 20,
                    price: 101.0,
                    quantity: 5.0,
                    order_id: None,
                    position_id: Some(id("position-1", PositionId::new)),
                }],
            })
            .unwrap();
        let frame = chart.build_frame();
        let segments = chart.frame_pane_segments(0).unwrap();
        assert!(segments.trading_regions_end > segments.series_end);
        assert!(segments.trading_end > segments.drawings_end);
        assert!(
            frame.panes[0].main[segments.series_end..segments.trading_regions_end]
                .iter()
                .any(|primitive| matches!(primitive, Prim::Rect { .. }))
        );
        let trading = &frame.panes[0].main[segments.drawings_end..segments.trading_end];
        assert!(
            trading
                .iter()
                .filter(|primitive| matches!(primitive, Prim::HLine { .. }))
                .count()
                >= 3
        );
        assert!(trading
            .iter()
            .any(|primitive| matches!(primitive, Prim::Circle { .. })));
        assert!(trading
            .iter()
            .any(|primitive| matches!(primitive, Prim::Text { text, .. } if text == "B")));

        let axis = chart.build_axis_frame(100.0, |text| text.len() as f64 * 7.0);
        for price in ["99.00", "101.00", "103.00"] {
            assert!(axis.labels.iter().any(|label| label.text == price));
        }
    }

    #[test]
    fn long_and_short_regions_follow_side_semantics_and_hits_are_trading_local() {
        for (side, take_profit, stop_loss) in [
            (PositionSide::Long, 103.0, 99.0),
            (PositionSide::Short, 99.0, 103.0),
        ] {
            let mut chart = chart_with_market();
            chart
                .set_trading_snapshot(TradingSnapshot {
                    positions: vec![position(side)],
                    orders: vec![
                        order("tp-1", OrderRole::TakeProfit, take_profit),
                        order("sl-1", OrderRole::StopLoss, stop_loss),
                    ],
                    ..TradingSnapshot::default()
                })
                .unwrap();
            chart.build_frame();
            let segments = chart.frame_pane_segments(0).unwrap();
            let frame = chart.build_frame();
            assert_eq!(
                frame.panes[0].main[segments.series_end..segments.trading_regions_end]
                    .iter()
                    .filter(|primitive| matches!(primitive, Prim::Rect { .. }))
                    .count(),
                2
            );
            let y = chart
                .trading_price_coordinate(0, TradingPriceScale::Right, take_profit)
                .unwrap();
            let hit = chart.trading_hit_at(20.0, y).unwrap();
            assert_eq!(hit.kind, TradingHitKind::OrderLine);
            assert!(matches!(hit.object, TradingObjectId::Order(_)));
        }
    }

    #[test]
    fn trading_state_is_chart_local_and_absent_from_drawing_persistence() {
        let mut first = chart_with_market();
        let second = chart_with_market();
        first
            .update_trading_position(position(PositionSide::Long))
            .unwrap();
        assert_eq!(first.trading_snapshot().positions.len(), 1);
        assert!(second.trading_snapshot().positions.is_empty());
        assert!(!first.export_state_json().unwrap().contains("position-1"));
    }

    #[test]
    fn order_drag_snaps_previews_and_emits_once_without_mutating_confirmed_state() {
        let mut chart = chart_with_market();
        chart
            .set_trading_snapshot(TradingSnapshot {
                instrument: InstrumentMetadata {
                    tick_size: Some(0.25),
                    ..InstrumentMetadata::default()
                },
                positions: vec![position(PositionSide::Long)],
                orders: vec![order("tp-1", OrderRole::TakeProfit, 103.0)],
                ..TradingSnapshot::default()
            })
            .unwrap();
        chart.build_frame();
        let start_y = chart
            .trading_price_coordinate(0, TradingPriceScale::Right, 103.0)
            .unwrap();
        let target_y = chart
            .trading_price_coordinate(0, TradingPriceScale::Right, 102.37)
            .unwrap();
        assert!(chart.trading_drag_start_at(100.0, start_y));
        assert!(chart.trading_drag_to(target_y));
        assert_eq!(chart.trading_snapshot().orders[0].price, 103.0);
        assert_eq!(chart.trading_preview().unwrap().price, 102.25);

        let intent = chart.trading_drag_end().unwrap();
        assert_eq!(intent.action, TradingIntentAction::ModifyOrder);
        assert_eq!(intent.order_id.as_ref().unwrap().as_str(), "tp-1");
        assert_eq!(intent.price, Some(102.25));
        assert_eq!(intent.base_revision, 1);
        assert_eq!(chart.trading_snapshot().orders[0].price, 103.0);
        assert_eq!(chart.take_trading_intents(), vec![intent.clone()]);
        assert!(chart.take_trading_intents().is_empty());
        assert!(chart.resolve_trading_intent(intent.sequence, false));
        assert!(chart.trading_preview().is_none());
        assert_eq!(chart.trading_snapshot().orders[0].price, 103.0);
    }

    #[test]
    fn accepted_preview_waits_for_authoritative_update_and_then_reconciles() {
        let mut chart = chart_with_market();
        chart
            .set_trading_snapshot(TradingSnapshot {
                instrument: InstrumentMetadata {
                    tick_size: Some(0.5),
                    ..InstrumentMetadata::default()
                },
                positions: vec![position(PositionSide::Long)],
                orders: vec![order("tp-1", OrderRole::TakeProfit, 103.0)],
                ..TradingSnapshot::default()
            })
            .unwrap();
        chart.build_frame();
        let start_y = chart
            .trading_price_coordinate(0, TradingPriceScale::Right, 103.0)
            .unwrap();
        let target_y = chart
            .trading_price_coordinate(0, TradingPriceScale::Right, 104.0)
            .unwrap();
        assert!(chart.trading_drag_start_at(100.0, start_y));
        assert!(chart.trading_drag_to(target_y));
        let intent = chart.trading_drag_end().unwrap();
        assert!(chart.resolve_trading_intent(intent.sequence, true));
        assert_eq!(
            chart.trading_preview().unwrap().phase,
            TradingPreviewPhase::Pending
        );

        let mut confirmed = order("tp-1", OrderRole::TakeProfit, 104.0);
        confirmed.revision = 2;
        chart.update_working_order(confirmed).unwrap();
        assert!(chart.trading_preview().is_none());
        assert_eq!(chart.trading_snapshot().orders[0].price, 104.0);
    }

    #[test]
    fn position_controls_create_typed_target_preview_and_intent() {
        let mut chart = chart_with_market();
        chart
            .set_trading_snapshot(TradingSnapshot {
                positions: vec![position(PositionSide::Long)],
                ..TradingSnapshot::default()
            })
            .unwrap();
        chart.build_frame();
        let entry_y = chart
            .trading_price_coordinate(0, TradingPriceScale::Right, 101.0)
            .unwrap();
        let target_y = chart
            .trading_price_coordinate(0, TradingPriceScale::Right, 104.0)
            .unwrap();
        assert!(chart.trading_drag_start_at(350.0, entry_y));
        assert!(matches!(
            chart.trading_preview().unwrap().source,
            TradingPreviewSource::TakeProfit { .. }
        ));
        assert!(chart.trading_drag_to(target_y));
        let intent = chart.trading_drag_end().unwrap();
        assert_eq!(intent.action, TradingIntentAction::CreateTakeProfit);
        assert_eq!(intent.position_id.as_ref().unwrap().as_str(), "position-1");
        assert_eq!(intent.kind, Some(OrderKind::Limit));
        assert_eq!(intent.role, Some(OrderRole::TakeProfit));
        assert!(chart.trading_snapshot().orders.is_empty());
    }

    #[test]
    fn semantic_trading_style_updates_transactionally() {
        let mut chart = chart_with_market();
        let before = chart.trading_style();
        assert!(chart
            .apply_trading_style(TradingStyleOptions {
                position: Some("not-a-color".to_string()),
                ..TradingStyleOptions::default()
            })
            .is_err());
        assert_eq!(chart.trading_style(), before);
        chart
            .apply_trading_style(TradingStyleOptions {
                position: Some("#123456".to_string()),
                risk: Some("rgba(200, 10, 20, 0.5)".to_string()),
                ..TradingStyleOptions::default()
            })
            .unwrap();
        assert_eq!(chart.trading_style().position, Color::rgb(0x12, 0x34, 0x56));
        assert_eq!(chart.trading_style().risk, Color::rgba(200, 10, 20, 128));
    }

    #[test]
    fn retained_trading_frame_stays_bounded_at_acceptance_object_counts() {
        for count in [10, 50, 100, 500] {
            let mut chart = chart_with_market();
            let orders = (0..count)
                .map(|index| {
                    order(
                        &format!("stress-{index}"),
                        if index % 2 == 0 {
                            OrderRole::TakeProfit
                        } else {
                            OrderRole::StopLoss
                        },
                        98.0 + index as f64 * 0.01,
                    )
                })
                .collect();
            chart
                .set_trading_snapshot(TradingSnapshot {
                    positions: vec![position(PositionSide::Long)],
                    orders,
                    ..TradingSnapshot::default()
                })
                .unwrap();
            chart.build_frame();
            assert_eq!(chart.frame_build_stats().trading_rebuilds, 1);
            assert_eq!(chart.trading_snapshot().orders.len(), count);
            chart.build_frame();
            assert_eq!(chart.frame_build_stats().trading_rebuilds, 0);
            assert!(chart.memory_usage().trading_capacity_bytes > 0);
        }
    }

    #[test]
    fn cancel_intent_is_pending_until_rejected_or_authoritatively_updated() {
        let mut chart = chart_with_market();
        chart
            .set_trading_snapshot(TradingSnapshot {
                orders: vec![order("order-1", OrderRole::Working, 102.0)],
                ..TradingSnapshot::default()
            })
            .unwrap();
        chart.build_frame();
        let y = chart
            .trading_price_coordinate(0, TradingPriceScale::Right, 102.0)
            .unwrap();
        let rejected = chart.trading_activate_at(390.0, y).unwrap();
        assert_eq!(rejected.action, TradingIntentAction::CancelOrder);
        assert_eq!(
            chart.trading_preview().unwrap().phase,
            TradingPreviewPhase::Pending
        );
        assert_eq!(
            chart.trading_snapshot().orders[0].status,
            OrderStatus::Working
        );
        assert!(chart.resolve_trading_intent(rejected.sequence, false));
        assert!(chart.trading_preview().is_none());

        let accepted = chart.trading_activate_at(390.0, y).unwrap();
        assert!(chart.resolve_trading_intent(accepted.sequence, true));
        assert!(chart.trading_preview().is_some());
        let mut authoritative = order("order-1", OrderRole::Working, 102.0);
        authoritative.status = OrderStatus::PendingCancel;
        authoritative.revision = 2;
        chart.update_working_order(authoritative).unwrap();
        assert!(chart.trading_preview().is_none());
        assert_eq!(
            chart.trading_snapshot().orders[0].status,
            OrderStatus::PendingCancel
        );
    }

    #[test]
    fn close_position_intent_is_pending_without_mutating_the_position() {
        let mut chart = chart_with_market();
        chart
            .set_trading_snapshot(TradingSnapshot {
                positions: vec![position(PositionSide::Long)],
                ..TradingSnapshot::default()
            })
            .unwrap();
        chart.build_frame();
        let y = chart
            .trading_price_coordinate(0, TradingPriceScale::Right, 101.0)
            .unwrap();
        let intent = chart.trading_activate_at(390.0, y).unwrap();
        assert_eq!(intent.action, TradingIntentAction::ClosePosition);
        assert_eq!(chart.trading_snapshot().positions.len(), 1);
        assert_eq!(
            chart
                .trading_state
                .pending_position_action
                .as_ref()
                .unwrap()
                .sequence,
            intent.sequence
        );
        assert!(chart.resolve_trading_intent(intent.sequence, false));
        assert!(chart.trading_state.pending_position_action.is_none());
        assert_eq!(chart.trading_snapshot().positions.len(), 1);
    }
}
