//! Engine-owned implementations of the official financial primitive examples.
//!
//! Browser hosts translate declarative values at the WASM boundary. Primitive state, autoscale
//! participation, pixel geometry, and lifecycle remain in the shared engine so every retained-
//! frame executor observes the same result.

use std::sync::Arc;

use crate::{ChartEngine, PaneId, SeriesId};
use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::{LineStyle, RasterImage};

pub type NativePrimitiveId = u32;

pub const MAX_RASTER_IMAGE_DIMENSION: u32 = 1024;
const MAX_NATIVE_PANE_PRIMITIVES: usize = 16;
const MAX_NATIVE_TEXT_BYTES: usize = 4 * 1024;
const MAX_NATIVE_FONT_FAMILY_BYTES: usize = 256;
pub const MAX_EXPIRING_PRICE_ALERTS: usize = 1_000;
pub const MAX_EXPIRING_ALERT_TIME_POINTS: usize = 16_384;
pub const MAX_USER_PRICE_ALERTS: usize = 1_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AccessibilityFocusOptions {
    pub color: Color,
    pub size: f64,
    pub high_contrast: bool,
}

impl Default for AccessibilityFocusOptions {
    fn default() -> Self {
        Self {
            color: Color::rgb(41, 98, 255),
            size: 14.0,
            high_contrast: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AccessibilityFocusState {
    pub(crate) options: AccessibilityFocusOptions,
    pub(crate) time: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TooltipOptions {
    pub line_color: Color,
    pub top_margin: f64,
}

impl Default for TooltipOptions {
    fn default() -> Self {
        Self {
            line_color: Color::rgba(0, 0, 0, 51),
            top_margin: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TooltipSnapshot {
    pub x: f64,
    pub index: i64,
    pub price: f64,
    pub time: i64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeltaTooltipOptions {
    pub line_color: Color,
    pub show_time: bool,
    pub top_offset: f64,
}

impl Default for DeltaTooltipOptions {
    fn default() -> Self {
        Self {
            line_color: Color::rgba(0, 0, 0, 51),
            show_time: false,
            top_offset: 20.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeltaTooltipPoint {
    pub x: f64,
    pub index: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeltaTooltipActiveRange {
    pub from: i64,
    pub to: i64,
    pub positive: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct DeltaTooltipState {
    pub options: DeltaTooltipOptions,
    pub committed_points: Vec<DeltaTooltipPoint>,
    pub preview_points: Vec<DeltaTooltipPoint>,
    pub mouse_start: Option<DeltaTooltipPoint>,
    pub mouse_drawing: bool,
}

impl DeltaTooltipState {
    pub(crate) fn visible_points(&self) -> &[DeltaTooltipPoint] {
        if self.mouse_drawing && self.preview_points.len() == 2 {
            &self.preview_points
        } else if self.committed_points.len() == 2 {
            &self.committed_points
        } else {
            &self.preview_points
        }
    }

    fn range_points(&self) -> &[DeltaTooltipPoint] {
        if self.mouse_drawing && self.preview_points.len() == 2 {
            &self.preview_points
        } else {
            &self.committed_points
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchoredTextHorizontalAlign {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchoredTextVerticalAlign {
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnchoredTextOptions {
    pub horizontal_align: AnchoredTextHorizontalAlign,
    pub vertical_align: AnchoredTextVerticalAlign,
    pub text: String,
    pub line_height: f64,
    pub font_size: f64,
    pub font_family: String,
    pub font_weight: u16,
    pub italic: bool,
    pub color: Color,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandsIndicatorOptions {
    pub line_color: Color,
    pub fill_color: Color,
    pub line_width: f64,
}

impl Default for BandsIndicatorOptions {
    fn default() -> Self {
        Self {
            line_color: Color::rgb(25, 200, 100),
            fill_color: Color::rgba(25, 200, 100, 64),
            line_width: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayPriceScaleSide {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayPriceScaleOptions {
    pub text_color: Color,
    pub side: OverlayPriceScaleSide,
}

impl Default for OverlayPriceScaleOptions {
    fn default() -> Self {
        Self {
            text_color: Color::rgb(0, 0, 0),
            side: OverlayPriceScaleSide::Left,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextWatermarkLine {
    pub text: String,
    pub color: Color,
    pub font_size: f64,
    pub font_family: String,
    pub font_weight: u16,
    pub italic: bool,
    pub line_height: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextWatermarkOptions {
    pub visible: bool,
    pub horizontal_align: AnchoredTextHorizontalAlign,
    pub vertical_align: AnchoredTextVerticalAlign,
    pub lines: Vec<TextWatermarkLine>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageWatermarkOptions {
    pub max_width: Option<f64>,
    pub max_height: Option<f64>,
    pub padding: f64,
    pub alpha: f64,
}

impl Default for ImageWatermarkOptions {
    fn default() -> Self {
        Self {
            max_width: None,
            max_height: None,
            padding: 0.0,
            alpha: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SessionHighlightingOptions {
    /// Optional UTC-hour gate. Without it, every source bar receives the weekday/weekend color,
    /// matching the official example. With it, bars outside the half-open session are clear.
    pub start_hour_utc: Option<u8>,
    pub end_hour_utc: Option<u8>,
    pub weekday_color: Color,
    pub weekend_color: Color,
}

/// Host-evaluated result of the official session-highlighter callback. The callback itself is a
/// browser-host concern; the shared engine retains these aligned records and owns all geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SessionHighlightingData {
    pub time: i64,
    pub color: Color,
}

impl Default for SessionHighlightingOptions {
    fn default() -> Self {
        Self {
            start_hour_utc: None,
            end_hour_utc: None,
            weekday_color: Color::rgba(41, 98, 255, 20),
            weekend_color: Color::rgba(255, 152, 1, 20),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SessionHighlightingState {
    pub(crate) options: SessionHighlightingOptions,
    pub(crate) highlights: Option<Vec<SessionHighlightingData>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeProfilePoint {
    pub price: f64,
    pub volume: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VolumeProfileData {
    pub time: i64,
    pub profile: Vec<VolumeProfilePoint>,
    /// Width in time-scale bar slots, matching the official example.
    pub width: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeProfileOptions {
    pub background_color: Color,
    pub row_color: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VerticalLineOptions {
    pub color: Color,
    pub label_text: String,
    pub width: f64,
    pub label_background_color: Color,
    pub label_text_color: Color,
    pub show_label: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UserPriceLinesButtonOptions {
    pub color: Color,
    pub hover_color: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UserPriceAlertsOptions {
    pub symbol_name: String,
    pub color: Color,
    pub hover_color: Color,
}

impl Default for UserPriceAlertsOptions {
    fn default() -> Self {
        Self {
            symbol_name: String::new(),
            color: Color::rgb(0x13, 0x17, 0x22),
            hover_color: Color::rgb(0x50, 0x53, 0x5e),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UserPriceAlert {
    pub id: u32,
    pub price: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserPriceAlertsHit {
    Add,
    Remove(u32),
}

#[derive(Clone, Debug)]
pub(crate) struct UserPriceAlertsState {
    pub options: UserPriceAlertsOptions,
    pub alerts: Vec<UserPriceAlert>,
    pub next_alert_id: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrendLineOptions {
    pub line_color: Color,
    pub width: f64,
    pub show_labels: bool,
    pub label_background_color: Color,
    pub label_text_color: Color,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertCrossingDirection {
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExpiringPriceAlertsOptions {
    pub interval: i64,
    pub clear_timeout_ms: f64,
}

impl Default for ExpiringPriceAlertsOptions {
    fn default() -> Self {
        Self {
            interval: 86_400,
            clear_timeout_ms: 3_000.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExpiringPriceAlert {
    pub id: u32,
    pub price: f64,
    pub start: i64,
    pub end: i64,
    pub title: String,
    pub crossing_direction: AlertCrossingDirection,
    pub crossed: bool,
    pub expired: bool,
    pub remove_at_ms: Option<f64>,
}

#[derive(Clone, Debug)]
pub(crate) struct ExpiringPriceAlertsState {
    pub options: ExpiringPriceAlertsOptions,
    pub alerts: Vec<ExpiringPriceAlert>,
    pub next_alert_id: u32,
    pub last_value: Option<f64>,
}

impl Default for TrendLineOptions {
    fn default() -> Self {
        Self {
            line_color: Color::rgb(0, 0, 0),
            width: 6.0,
            show_labels: true,
            label_background_color: Color::rgba(255, 255, 255, 217),
            label_text_color: Color::rgb(0, 0, 0),
        }
    }
}

impl Default for UserPriceLinesButtonOptions {
    fn default() -> Self {
        Self {
            color: Color::rgb(0, 0, 0),
            hover_color: Color::rgb(119, 119, 119),
        }
    }
}

impl Default for VerticalLineOptions {
    fn default() -> Self {
        Self {
            color: Color::rgb(0, 128, 0),
            label_text: String::new(),
            width: 3.0,
            label_background_color: Color::rgb(0, 128, 0),
            label_text_color: Color::rgb(255, 255, 255),
            show_label: false,
        }
    }
}

impl Default for VolumeProfileOptions {
    fn default() -> Self {
        Self {
            background_color: Color::rgba(0, 0, 255, 51),
            row_color: Color::rgba(80, 80, 255, 204),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum NativeSeriesPrimitiveKind {
    AnchoredText(AnchoredTextOptions),
    ImageWatermark {
        image: RasterImage,
        options: ImageWatermarkOptions,
    },
    BandsIndicator(BandsIndicatorOptions),
    OverlayPriceScale(OverlayPriceScaleOptions),
    PartialPriceLine,
    AccessibilityFocus(AccessibilityFocusState),
    SessionHighlighting(SessionHighlightingState),
    HighlightBarCrosshair {
        color: Color,
    },
    VerticalLine {
        time: i64,
        options: VerticalLineOptions,
    },
    UserPriceLinesButton(UserPriceLinesButtonOptions),
    UserPriceAlerts(UserPriceAlertsState),
    Tooltip(TooltipOptions),
    DeltaTooltip(DeltaTooltipState),
    TrendLine {
        first_time: i64,
        first_price: f64,
        second_time: i64,
        second_price: f64,
        options: TrendLineOptions,
    },
    ExpiringPriceAlerts(ExpiringPriceAlertsState),
    VolumeProfile {
        data: VolumeProfileData,
        options: VolumeProfileOptions,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct NativeSeriesPrimitive {
    pub id: NativePrimitiveId,
    pub kind: NativeSeriesPrimitiveKind,
}

#[derive(Clone, Debug)]
pub(crate) enum NativePanePrimitiveKind {
    TextWatermark(TextWatermarkOptions),
}

#[derive(Clone, Debug)]
pub(crate) struct NativePanePrimitive {
    pub id: NativePrimitiveId,
    pub pane_id: PaneId,
    pub kind: NativePanePrimitiveKind,
}

impl NativeSeriesPrimitive {
    fn capacity_bytes(&self) -> usize {
        match &self.kind {
            NativeSeriesPrimitiveKind::AnchoredText(options) => {
                options.text.capacity() + options.font_family.capacity()
            }
            NativeSeriesPrimitiveKind::ImageWatermark { image, .. } => image.pixels.len(),
            NativeSeriesPrimitiveKind::SessionHighlighting(state) => {
                state.highlights.as_ref().map_or(0, |items| {
                    items.capacity() * core::mem::size_of::<SessionHighlightingData>()
                })
            }
            NativeSeriesPrimitiveKind::VolumeProfile { data, .. } => {
                data.profile.capacity() * core::mem::size_of::<VolumeProfilePoint>()
            }
            NativeSeriesPrimitiveKind::VerticalLine { options, .. } => {
                options.label_text.capacity()
            }
            NativeSeriesPrimitiveKind::ExpiringPriceAlerts(state) => {
                state.alerts.capacity() * core::mem::size_of::<ExpiringPriceAlert>()
                    + state
                        .alerts
                        .iter()
                        .map(|alert| alert.title.capacity())
                        .sum::<usize>()
            }
            NativeSeriesPrimitiveKind::UserPriceAlerts(state) => {
                state.alerts.capacity() * core::mem::size_of::<UserPriceAlert>()
                    + state.options.symbol_name.capacity()
            }
            NativeSeriesPrimitiveKind::DeltaTooltip(state) => {
                (state.committed_points.capacity() + state.preview_points.capacity())
                    * core::mem::size_of::<DeltaTooltipPoint>()
            }
            _ => 0,
        }
    }
}

fn valid_volume_profile(data: &VolumeProfileData) -> bool {
    data.width.is_finite()
        && data.width > 0.0
        && data.width <= 10_000.0
        && (2..=512).contains(&data.profile.len())
        && data
            .profile
            .iter()
            .all(|point| point.price.is_finite() && point.volume.is_finite() && point.volume >= 0.0)
        && data.profile.iter().any(|point| point.volume > 0.0)
}

fn valid_bands_indicator_options(options: BandsIndicatorOptions) -> bool {
    options.line_width.is_finite() && options.line_width > 0.0 && options.line_width <= 64.0
}

fn valid_accessibility_focus_options(options: AccessibilityFocusOptions) -> bool {
    options.size.is_finite() && (4.0..=128.0).contains(&options.size)
}

fn valid_vertical_line(time: i64, options: &VerticalLineOptions) -> bool {
    time != i64::MIN
        && options.width.is_finite()
        && options.width > 0.0
        && options.width <= 32.0
        && options.label_text.len() <= MAX_NATIVE_TEXT_BYTES
}

fn valid_trend_line(
    first_time: i64,
    first_price: f64,
    second_time: i64,
    second_price: f64,
    options: &TrendLineOptions,
) -> bool {
    first_time != i64::MIN
        && second_time != i64::MIN
        && first_price.is_finite()
        && second_price.is_finite()
        && options.width.is_finite()
        && options.width > 0.0
        && options.width <= 64.0
}

fn valid_expiring_alert_options(options: ExpiringPriceAlertsOptions) -> bool {
    options.interval > 0
        && options.interval <= 366 * 86_400
        && options.clear_timeout_ms.is_finite()
        && (0.0..=3_600_000.0).contains(&options.clear_timeout_ms)
}

fn valid_image_options(options: ImageWatermarkOptions) -> bool {
    options.padding.is_finite()
        && options.padding >= 0.0
        && options.alpha.is_finite()
        && (0.0..=1.0).contains(&options.alpha)
        && options
            .max_width
            .is_none_or(|value| value.is_finite() && value > 0.0)
        && options
            .max_height
            .is_none_or(|value| value.is_finite() && value > 0.0)
}

fn valid_anchored_text_options(options: &AnchoredTextOptions) -> bool {
    !options.text.is_empty()
        && options.text.len() <= MAX_NATIVE_TEXT_BYTES
        && !options.font_family.is_empty()
        && options.font_family.len() <= MAX_NATIVE_FONT_FAMILY_BYTES
        && options.line_height.is_finite()
        && options.line_height > 0.0
        && options.font_size.is_finite()
        && options.font_size > 0.0
        && options.font_size <= 512.0
        && (100..=900).contains(&options.font_weight)
}

fn valid_text_watermark_options(options: &TextWatermarkOptions) -> bool {
    options.lines.len() <= 32
        && options.lines.iter().all(|line| {
            line.text.len() <= MAX_NATIVE_TEXT_BYTES
                && !line.font_family.is_empty()
                && line.font_family.len() <= MAX_NATIVE_FONT_FAMILY_BYTES
                && line.font_size.is_finite()
                && line.font_size > 0.0
                && line.font_size <= 512.0
                && line.line_height.is_finite()
                && line.line_height > 0.0
                && (100..=900).contains(&line.font_weight)
        })
}

impl ChartEngine {
    fn insert_native_pane_primitive(
        &mut self,
        pane_index: usize,
        kind: NativePanePrimitiveKind,
    ) -> Option<NativePrimitiveId> {
        let pane_id = self.panes.get(pane_index)?.stable_id()?;
        if self.native_pane_primitives.len() >= MAX_NATIVE_PANE_PRIMITIVES {
            return None;
        }
        let id = self.next_native_primitive_id;
        self.next_native_primitive_id = id.checked_add(1)?;
        self.native_pane_primitives
            .push(NativePanePrimitive { id, pane_id, kind });
        self.invalidate_frame_scene();
        Some(id)
    }

    fn insert_native_primitive(
        &mut self,
        series_id: SeriesId,
        kind: NativeSeriesPrimitiveKind,
    ) -> Option<NativePrimitiveId> {
        self.validate_series_id(series_id).ok()?;
        let id = self.next_native_primitive_id;
        self.next_native_primitive_id = id.checked_add(1)?;
        self.series_entry_mut(series_id)?
            .native_primitives
            .push(NativeSeriesPrimitive { id, kind });
        self.invalidate_frame_series(series_id);
        Some(id)
    }

    pub fn add_partial_price_line(&mut self, series_id: SeriesId) -> Option<NativePrimitiveId> {
        self.insert_native_primitive(series_id, NativeSeriesPrimitiveKind::PartialPriceLine)
    }

    pub fn add_bands_indicator(
        &mut self,
        series_id: SeriesId,
        options: BandsIndicatorOptions,
    ) -> Option<NativePrimitiveId> {
        valid_bands_indicator_options(options).then_some(())?;
        self.insert_native_primitive(
            series_id,
            NativeSeriesPrimitiveKind::BandsIndicator(options),
        )
    }

    pub fn set_bands_indicator_options(
        &mut self,
        primitive_id: NativePrimitiveId,
        options: BandsIndicatorOptions,
    ) -> bool {
        if !valid_bands_indicator_options(options) {
            return false;
        }
        let Some((series_id, current)) = self.series.iter_mut().find_map(|series| {
            series.native_primitives.iter_mut().find_map(|primitive| {
                (primitive.id == primitive_id).then_some(()).and_then(|()| {
                    let NativeSeriesPrimitiveKind::BandsIndicator(current) = &mut primitive.kind
                    else {
                        return None;
                    };
                    Some((series.id, current))
                })
            })
        }) else {
            return false;
        };
        *current = options;
        self.invalidate_frame_series(series_id);
        true
    }

    pub fn add_overlay_price_scale(
        &mut self,
        series_id: SeriesId,
        options: OverlayPriceScaleOptions,
    ) -> Option<NativePrimitiveId> {
        self.insert_native_primitive(
            series_id,
            NativeSeriesPrimitiveKind::OverlayPriceScale(options),
        )
    }

    pub fn set_overlay_price_scale_options(
        &mut self,
        primitive_id: NativePrimitiveId,
        options: OverlayPriceScaleOptions,
    ) -> bool {
        let Some((series_id, current)) = self.series.iter_mut().find_map(|series| {
            series.native_primitives.iter_mut().find_map(|primitive| {
                (primitive.id == primitive_id).then_some(()).and_then(|()| {
                    let NativeSeriesPrimitiveKind::OverlayPriceScale(current) = &mut primitive.kind
                    else {
                        return None;
                    };
                    Some((series.id, current))
                })
            })
        }) else {
            return false;
        };
        *current = options;
        self.invalidate_frame_series(series_id);
        true
    }

    pub fn add_accessibility_focus(
        &mut self,
        series_id: SeriesId,
        options: AccessibilityFocusOptions,
    ) -> Option<NativePrimitiveId> {
        valid_accessibility_focus_options(options).then_some(())?;
        let id = self.insert_native_primitive(
            series_id,
            NativeSeriesPrimitiveKind::AccessibilityFocus(AccessibilityFocusState {
                options,
                time: None,
            }),
        )?;
        self.invalidate_frame_overlay();
        Some(id)
    }

    pub fn set_accessibility_focus(
        &mut self,
        primitive_id: NativePrimitiveId,
        time: Option<i64>,
        options: AccessibilityFocusOptions,
    ) -> bool {
        if !valid_accessibility_focus_options(options) {
            return false;
        }
        let Some(state) = self.series.iter_mut().find_map(|series| {
            series.native_primitives.iter_mut().find_map(|primitive| {
                (primitive.id == primitive_id)
                    .then_some(&mut primitive.kind)
                    .and_then(|kind| match kind {
                        NativeSeriesPrimitiveKind::AccessibilityFocus(state) => Some(state),
                        _ => None,
                    })
            })
        }) else {
            return false;
        };
        state.time = time;
        state.options = options;
        self.invalidate_frame_overlay();
        true
    }

    pub fn add_session_highlighting(
        &mut self,
        series_id: SeriesId,
        options: SessionHighlightingOptions,
    ) -> Option<NativePrimitiveId> {
        if options.start_hour_utc.is_some_and(|hour| hour > 23)
            || options.end_hour_utc.is_some_and(|hour| hour > 24)
            || options.start_hour_utc.is_some() != options.end_hour_utc.is_some()
        {
            return None;
        }
        self.insert_native_primitive(
            series_id,
            NativeSeriesPrimitiveKind::SessionHighlighting(SessionHighlightingState {
                options,
                highlights: None,
            }),
        )
    }

    /// Replace the callback-derived colors for an official session-highlighting primitive.
    /// Records must exactly align with the primitive's source series so stale host state can never
    /// color a different bar after an update or retention trim.
    pub fn set_session_highlighting_data(
        &mut self,
        primitive_id: NativePrimitiveId,
        highlights: Vec<SessionHighlightingData>,
    ) -> bool {
        let Some(series_id) = self.series.iter().find_map(|series| {
            series
                .native_primitives
                .iter()
                .any(|primitive| primitive.id == primitive_id)
                .then_some(series.id)
        }) else {
            return false;
        };
        let Some((times, _)) = self.data.series_data(series_id) else {
            return false;
        };
        if highlights.len() != times.len()
            || highlights
                .iter()
                .zip(times)
                .any(|(highlight, time)| highlight.time != *time)
        {
            return false;
        }
        let Some(state) = self.series_entry_mut(series_id).and_then(|series| {
            series.native_primitives.iter_mut().find_map(|primitive| {
                (primitive.id == primitive_id)
                    .then_some(&mut primitive.kind)
                    .and_then(|kind| match kind {
                        NativeSeriesPrimitiveKind::SessionHighlighting(state) => Some(state),
                        _ => None,
                    })
            })
        }) else {
            return false;
        };
        state.highlights = Some(highlights);
        self.invalidate_frame_series(series_id);
        true
    }

    pub fn add_highlight_bar_crosshair(
        &mut self,
        series_id: SeriesId,
        color: Color,
    ) -> Option<NativePrimitiveId> {
        self.insert_native_primitive(
            series_id,
            NativeSeriesPrimitiveKind::HighlightBarCrosshair { color },
        )
    }

    pub fn add_vertical_line(
        &mut self,
        series_id: SeriesId,
        time: i64,
        options: VerticalLineOptions,
    ) -> Option<NativePrimitiveId> {
        valid_vertical_line(time, &options).then_some(())?;
        self.insert_native_primitive(
            series_id,
            NativeSeriesPrimitiveKind::VerticalLine { time, options },
        )
    }

    pub fn add_user_price_lines_button(
        &mut self,
        series_id: SeriesId,
        options: UserPriceLinesButtonOptions,
    ) -> Option<NativePrimitiveId> {
        self.insert_native_primitive(
            series_id,
            NativeSeriesPrimitiveKind::UserPriceLinesButton(options),
        )
    }

    pub fn add_user_price_alerts(
        &mut self,
        series_id: SeriesId,
        options: UserPriceAlertsOptions,
    ) -> Option<NativePrimitiveId> {
        if options.symbol_name.len() > MAX_NATIVE_TEXT_BYTES {
            return None;
        }
        let id = self.insert_native_primitive(
            series_id,
            NativeSeriesPrimitiveKind::UserPriceAlerts(UserPriceAlertsState {
                options,
                alerts: Vec::new(),
                next_alert_id: 1,
            }),
        )?;
        self.invalidate_frame_overlay();
        self.invalidate_axis_frame();
        Some(id)
    }

    pub fn add_delta_tooltip(
        &mut self,
        series_id: SeriesId,
        options: DeltaTooltipOptions,
    ) -> Option<NativePrimitiveId> {
        if !options.top_offset.is_finite() || !(0.0..=1_000.0).contains(&options.top_offset) {
            return None;
        }
        let id = self.insert_native_primitive(
            series_id,
            NativeSeriesPrimitiveKind::DeltaTooltip(DeltaTooltipState {
                options,
                committed_points: Vec::with_capacity(2),
                preview_points: Vec::with_capacity(2),
                mouse_start: None,
                mouse_drawing: false,
            }),
        )?;
        self.invalidate_frame_overlay();
        Some(id)
    }

    pub fn add_tooltip(
        &mut self,
        series_id: SeriesId,
        options: TooltipOptions,
    ) -> Option<NativePrimitiveId> {
        if !options.top_margin.is_finite() || !(0.0..=1_000.0).contains(&options.top_margin) {
            return None;
        }
        let id =
            self.insert_native_primitive(series_id, NativeSeriesPrimitiveKind::Tooltip(options))?;
        self.invalidate_frame_overlay();
        Some(id)
    }

    pub fn set_tooltip_options(
        &mut self,
        primitive_id: NativePrimitiveId,
        options: TooltipOptions,
    ) -> bool {
        if !options.top_margin.is_finite() || !(0.0..=1_000.0).contains(&options.top_margin) {
            return false;
        }
        let Some(current) = self.series.iter_mut().find_map(|series| {
            series.native_primitives.iter_mut().find_map(|primitive| {
                (primitive.id == primitive_id)
                    .then_some(&mut primitive.kind)
                    .and_then(|kind| {
                        let NativeSeriesPrimitiveKind::Tooltip(current) = kind else {
                            return None;
                        };
                        Some(current)
                    })
            })
        }) else {
            return false;
        };
        if *current != options {
            *current = options;
            self.invalidate_frame_overlay();
        }
        true
    }

    pub fn tooltip_snapshot(&self, primitive_id: NativePrimitiveId) -> Option<TooltipSnapshot> {
        let (series_id, _) = self.series.iter().find_map(|series| {
            series.native_primitives.iter().find_map(|primitive| {
                (primitive.id == primitive_id
                    && matches!(primitive.kind, NativeSeriesPrimitiveKind::Tooltip(_)))
                .then_some((series.id, primitive))
            })
        })?;
        let (pointer_x, pointer_y) = self.clamped_crosshair()?;
        let pane_index = self.series_entry(series_id)?.pane_index;
        if self.pane_at_y(pointer_y) != Some(pane_index) {
            return None;
        }
        let index = self.snapped_crosshair_index(pointer_x);
        let plot = self.data.plot(series_id);
        let row = plot.search(
            index,
            nucleuscharts_core::model::plot_list::MismatchDirection::None,
        )?;
        if plot.is_whitespace_row(row) {
            return None;
        }
        let (times, _) = self.data.series_data(series_id)?;
        Some(TooltipSnapshot {
            x: self.time_scale.index_to_coordinate(index),
            index,
            price: plot.value_at(
                row,
                nucleuscharts_core::model::plot_list::PlotValueIndex::Close,
            ),
            time: *times.get(row)?,
        })
    }

    pub fn set_delta_tooltip_points(
        &mut self,
        primitive_id: NativePrimitiveId,
        xs: &[f64],
    ) -> bool {
        if xs.len() > 2 || xs.iter().any(|x| !x.is_finite()) {
            return false;
        }
        let Some(series_id) = self.series.iter().find_map(|series| {
            series
                .native_primitives
                .iter()
                .any(|primitive| {
                    primitive.id == primitive_id
                        && matches!(primitive.kind, NativeSeriesPrimitiveKind::DeltaTooltip(_))
                })
                .then_some(series.id)
        }) else {
            return false;
        };
        let plot = self.data.plot(series_id);
        let points: Vec<_> = xs
            .iter()
            .filter(|x| **x >= 0.0 && **x <= self.pane_w)
            .filter_map(|x| {
                let index = self.time_scale.coordinate_to_index(*x);
                plot.search(
                    index,
                    nucleuscharts_core::model::plot_list::MismatchDirection::None,
                )
                .filter(|row| !plot.is_whitespace_row(*row))
                .map(|_| DeltaTooltipPoint { x: *x, index })
            })
            .collect();
        let Some(state) = self.series.iter_mut().find_map(|series| {
            series.native_primitives.iter_mut().find_map(|primitive| {
                if primitive.id != primitive_id {
                    return None;
                }
                let NativeSeriesPrimitiveKind::DeltaTooltip(state) = &mut primitive.kind else {
                    return None;
                };
                Some(state)
            })
        }) else {
            return false;
        };
        let (committed, preview) = if points.len() == 2 {
            (points, Vec::new())
        } else {
            (Vec::new(), points)
        };
        if state.committed_points == committed && state.preview_points == preview {
            return true;
        }
        state.committed_points = committed;
        state.preview_points = preview;
        state.mouse_start = None;
        state.mouse_drawing = false;
        self.invalidate_frame_overlay();
        true
    }

    pub fn delta_tooltip_active_range(
        &self,
        primitive_id: NativePrimitiveId,
    ) -> Option<DeltaTooltipActiveRange> {
        let (series_id, state) = self.series.iter().find_map(|series| {
            series.native_primitives.iter().find_map(|primitive| {
                if primitive.id != primitive_id {
                    return None;
                }
                let NativeSeriesPrimitiveKind::DeltaTooltip(state) = &primitive.kind else {
                    return None;
                };
                Some((series.id, state))
            })
        })?;
        let range_points = state.range_points();
        if range_points.len() != 2 {
            return None;
        }
        let mut points = range_points.to_vec();
        points.sort_by_key(|point| point.index);
        let plot = self.data.plot(series_id);
        let price = |point: DeltaTooltipPoint| {
            plot.search(
                point.index,
                nucleuscharts_core::model::plot_list::MismatchDirection::None,
            )
            .map(|row| {
                plot.value_at(
                    row,
                    nucleuscharts_core::model::plot_list::PlotValueIndex::Close,
                )
            })
        };
        let first = price(points[0])?;
        let second = price(points[1])?;
        Some(DeltaTooltipActiveRange {
            from: points[0].index.saturating_add(1),
            to: points[1].index.saturating_add(1),
            positive: second - first >= 0.0,
        })
    }

    /// Capture the first comparison point for the official mouse-drag interaction. The host
    /// forwards only the pane x-coordinate; logical lookup and all plugin state stay here.
    pub fn delta_tooltip_mouse_down(&mut self, x: f64) -> bool {
        if !x.is_finite() {
            return false;
        }
        let targets: Vec<_> = self
            .series
            .iter()
            .flat_map(|series| {
                series
                    .native_primitives
                    .iter()
                    .filter_map(move |primitive| {
                        matches!(primitive.kind, NativeSeriesPrimitiveKind::DeltaTooltip(_))
                            .then_some((series.id, primitive.id))
                    })
            })
            .collect();
        let starts: Vec<_> = targets
            .iter()
            .map(|(series_id, primitive_id)| {
                (*primitive_id, self.delta_tooltip_point(*series_id, x))
            })
            .collect();
        let mut handled = false;
        let mut changed = false;
        for series in &mut self.series {
            for primitive in &mut series.native_primitives {
                let NativeSeriesPrimitiveKind::DeltaTooltip(state) = &mut primitive.kind else {
                    continue;
                };
                handled = true;
                if !state.preview_points.is_empty() {
                    state.preview_points.clear();
                    changed = true;
                }
                state.mouse_start = starts
                    .iter()
                    .find_map(|(id, point)| (*id == primitive.id).then_some(*point))
                    .flatten();
                state.mouse_drawing = state.mouse_start.is_some();
            }
        }
        if changed {
            self.invalidate_frame_overlay();
        }
        handled
    }

    /// Update every attached delta tooltip from the live mouse position. A held primary button
    /// yields the captured point plus the live point; ordinary hover yields one point.
    pub fn delta_tooltip_mouse_move(&mut self, x: f64) -> bool {
        if !x.is_finite() {
            return self.clear_delta_tooltip_previews(true);
        }
        let targets: Vec<_> = self
            .series
            .iter()
            .flat_map(|series| {
                series
                    .native_primitives
                    .iter()
                    .filter_map(move |primitive| {
                        matches!(primitive.kind, NativeSeriesPrimitiveKind::DeltaTooltip(_))
                            .then_some((series.id, primitive.id))
                    })
            })
            .collect();
        let live: Vec<_> = targets
            .iter()
            .map(|(series_id, primitive_id)| {
                (*primitive_id, self.delta_tooltip_point(*series_id, x))
            })
            .collect();
        let mut changed = false;
        for series in &mut self.series {
            for primitive in &mut series.native_primitives {
                let NativeSeriesPrimitiveKind::DeltaTooltip(state) = &mut primitive.kind else {
                    continue;
                };
                let current = live
                    .iter()
                    .find_map(|(id, point)| (*id == primitive.id).then_some(*point))
                    .flatten();
                let mut points = Vec::with_capacity(2);
                if let Some(current) = current {
                    if state.mouse_drawing {
                        if let Some(start) = state.mouse_start {
                            points.push(start);
                        }
                    }
                    points.push(current);
                }
                if state.preview_points != points {
                    state.preview_points = points;
                    changed = true;
                }
            }
        }
        if changed {
            self.invalidate_frame_overlay();
        }
        changed
    }

    /// Commit a completed mouse comparison. An incomplete replacement gesture leaves the prior
    /// committed range intact.
    pub fn delta_tooltip_mouse_up(&mut self) -> bool {
        let mut changed = false;
        for series in &mut self.series {
            for primitive in &mut series.native_primitives {
                let NativeSeriesPrimitiveKind::DeltaTooltip(state) = &mut primitive.kind else {
                    continue;
                };
                if state.mouse_drawing
                    && state.preview_points.len() == 2
                    && state.committed_points != state.preview_points
                {
                    state.committed_points.clone_from(&state.preview_points);
                    changed = true;
                }
                if !state.preview_points.is_empty() {
                    state.preview_points.clear();
                    changed = true;
                }
                state.mouse_drawing = false;
                state.mouse_start = None;
            }
        }
        if changed {
            self.invalidate_frame_overlay();
        }
        changed
    }

    /// Forward a touch-move sample. The first two touches become comparison points; the chart
    /// host remains responsible only for normalizing browser coordinates.
    pub fn delta_tooltip_touch_move(&mut self, xs: &[f64]) -> bool {
        if xs.len() > 2 || xs.iter().any(|x| !x.is_finite()) {
            return false;
        }
        let targets: Vec<_> = self
            .series
            .iter()
            .flat_map(|series| {
                series
                    .native_primitives
                    .iter()
                    .filter_map(move |primitive| {
                        matches!(primitive.kind, NativeSeriesPrimitiveKind::DeltaTooltip(_))
                            .then_some((series.id, primitive.id))
                    })
            })
            .collect();
        let resolved: Vec<_> = targets
            .iter()
            .map(|(series_id, primitive_id)| {
                let points = xs
                    .iter()
                    .filter_map(|x| self.delta_tooltip_point(*series_id, *x))
                    .collect::<Vec<_>>();
                (*primitive_id, points)
            })
            .collect();
        let mut changed = false;
        for series in &mut self.series {
            for primitive in &mut series.native_primitives {
                let NativeSeriesPrimitiveKind::DeltaTooltip(state) = &mut primitive.kind else {
                    continue;
                };
                let points = resolved
                    .iter()
                    .find_map(|(id, points)| (*id == primitive.id).then_some(points))
                    .cloned()
                    .unwrap_or_default();
                let state_changed = if points.len() == 2 {
                    let changed =
                        state.committed_points != points || !state.preview_points.is_empty();
                    state.committed_points = points;
                    state.preview_points.clear();
                    changed
                } else if state.preview_points != points {
                    state.preview_points = points;
                    true
                } else {
                    false
                };
                if state_changed {
                    changed = true;
                }
            }
        }
        if changed {
            self.invalidate_frame_overlay();
        }
        changed
    }

    /// Clear transient hover/gesture state on leave while preserving a committed comparison.
    pub fn delta_tooltip_leave(&mut self) -> bool {
        self.clear_delta_tooltip_previews(true)
    }

    /// Explicitly clear one committed delta-tooltip selection and any transient gesture state.
    pub fn clear_delta_tooltip(&mut self, primitive_id: NativePrimitiveId) -> bool {
        let Some(state) = self.series.iter_mut().find_map(|series| {
            series.native_primitives.iter_mut().find_map(|primitive| {
                if primitive.id != primitive_id {
                    return None;
                }
                let NativeSeriesPrimitiveKind::DeltaTooltip(state) = &mut primitive.kind else {
                    return None;
                };
                Some(state)
            })
        }) else {
            return false;
        };
        let changed = !state.committed_points.is_empty() || !state.preview_points.is_empty();
        state.committed_points.clear();
        state.preview_points.clear();
        state.mouse_start = None;
        state.mouse_drawing = false;
        if changed {
            self.invalidate_frame_overlay();
        }
        true
    }

    fn delta_tooltip_point(&self, series_id: SeriesId, x: f64) -> Option<DeltaTooltipPoint> {
        if !(0.0..=self.pane_w).contains(&x) {
            return None;
        }
        let index = self.time_scale.coordinate_to_index(x);
        let plot = self.data.plot(series_id);
        plot.search(
            index,
            nucleuscharts_core::model::plot_list::MismatchDirection::None,
        )
        .filter(|row| !plot.is_whitespace_row(*row))
        .map(|_| DeltaTooltipPoint { x, index })
    }

    fn clear_delta_tooltip_previews(&mut self, end_mouse: bool) -> bool {
        let mut changed = false;
        for series in &mut self.series {
            for primitive in &mut series.native_primitives {
                let NativeSeriesPrimitiveKind::DeltaTooltip(state) = &mut primitive.kind else {
                    continue;
                };
                if !state.preview_points.is_empty() {
                    state.preview_points.clear();
                    changed = true;
                }
                if end_mouse {
                    state.mouse_drawing = false;
                    state.mouse_start = None;
                }
            }
        }
        if changed {
            self.invalidate_frame_overlay();
        }
        changed
    }

    pub fn add_user_price_alert(
        &mut self,
        primitive_id: NativePrimitiveId,
        price: f64,
    ) -> Option<u32> {
        if !price.is_finite() {
            return None;
        }
        let state = self.series.iter_mut().find_map(|series| {
            series.native_primitives.iter_mut().find_map(|primitive| {
                (primitive.id == primitive_id)
                    .then_some(&mut primitive.kind)
                    .and_then(|kind| {
                        let NativeSeriesPrimitiveKind::UserPriceAlerts(state) = kind else {
                            return None;
                        };
                        Some(state)
                    })
            })
        })?;
        if state.alerts.len() >= MAX_USER_PRICE_ALERTS {
            return None;
        }
        let id = state.next_alert_id;
        state.next_alert_id = id.checked_add(1)?;
        state.alerts.push(UserPriceAlert { id, price });
        state.alerts.sort_by(|a, b| b.price.total_cmp(&a.price));
        self.invalidate_frame_overlay();
        self.invalidate_axis_frame();
        Some(id)
    }

    pub fn remove_user_price_alert(
        &mut self,
        primitive_id: NativePrimitiveId,
        alert_id: u32,
    ) -> bool {
        let Some(state) = self.series.iter_mut().find_map(|series| {
            series.native_primitives.iter_mut().find_map(|primitive| {
                (primitive.id == primitive_id)
                    .then_some(&mut primitive.kind)
                    .and_then(|kind| {
                        let NativeSeriesPrimitiveKind::UserPriceAlerts(state) = kind else {
                            return None;
                        };
                        Some(state)
                    })
            })
        }) else {
            return false;
        };
        let Some(index) = state.alerts.iter().position(|alert| alert.id == alert_id) else {
            return false;
        };
        state.alerts.remove(index);
        self.invalidate_frame_overlay();
        self.invalidate_axis_frame();
        true
    }

    pub fn user_price_alerts(&self, primitive_id: NativePrimitiveId) -> Option<&[UserPriceAlert]> {
        self.series.iter().find_map(|series| {
            series.native_primitives.iter().find_map(|primitive| {
                if primitive.id != primitive_id {
                    return None;
                }
                let NativeSeriesPrimitiveKind::UserPriceAlerts(state) = &primitive.kind else {
                    return None;
                };
                Some(state.alerts.as_slice())
            })
        })
    }

    pub fn user_price_alerts_hit_test(
        &self,
        primitive_id: NativePrimitiveId,
        x: f64,
        y: f64,
    ) -> Option<UserPriceAlertsHit> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let (series_id, pane_index, state) = self.series.iter().find_map(|series| {
            series.native_primitives.iter().find_map(|primitive| {
                if primitive.id != primitive_id {
                    return None;
                }
                let NativeSeriesPrimitiveKind::UserPriceAlerts(state) = &primitive.kind else {
                    return None;
                };
                Some((series.id, series.pane_index, state))
            })
        })?;
        if self.pane_at_y(y) != Some(pane_index) {
            return None;
        }
        let distance_to_scale = self.pane_w - x;
        if (1.0..21.0).contains(&distance_to_scale) {
            return Some(UserPriceAlertsHit::Add);
        }
        let closest = state
            .alerts
            .iter()
            .filter_map(|alert| {
                self.series_price_to_coordinate(series_id, alert.price)
                    .map(|alert_y| (alert, alert_y, (y - alert_y).abs()))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))?;
        if closest.2 >= 50.0 {
            return None;
        }
        let price = self.series_format_price(series_id, closest.0.price)?;
        let text_length = state.options.symbol_name.chars().count()
            + " crossing ".chars().count()
            + price.chars().count();
        let label_width = 9.0 * 2.0 + 26.0 + text_length as f64 * 5.81;
        let remove_left = (self.pane_w - label_width) * 0.5 + label_width - 26.0;
        (x >= remove_left && x <= remove_left + 26.0 && (y - closest.1).abs() <= 10.0)
            .then_some(UserPriceAlertsHit::Remove(closest.0.id))
    }

    pub fn hit_test_user_price_alerts(
        &self,
        x: f64,
        y: f64,
    ) -> Option<(SeriesId, NativePrimitiveId, UserPriceAlertsHit)> {
        self.series_order.iter().rev().find_map(|series_id| {
            let series = self.series_entry(*series_id)?;
            series.native_primitives.iter().rev().find_map(|primitive| {
                matches!(
                    primitive.kind,
                    NativeSeriesPrimitiveKind::UserPriceAlerts(_)
                )
                .then(|| {
                    self.user_price_alerts_hit_test(primitive.id, x, y)
                        .map(|hit| (*series_id, primitive.id, hit))
                })
                .flatten()
            })
        })
    }

    pub fn click_user_price_alerts(
        &mut self,
        primitive_id: NativePrimitiveId,
        x: f64,
        y: f64,
    ) -> bool {
        match self.user_price_alerts_hit_test(primitive_id, x, y) {
            Some(UserPriceAlertsHit::Add) => self
                .series
                .iter()
                .find_map(|series| {
                    series
                        .native_primitives
                        .iter()
                        .any(|primitive| primitive.id == primitive_id)
                        .then_some(series.id)
                })
                .and_then(|series_id| self.series_coordinate_to_price(series_id, y))
                .and_then(|price| self.add_user_price_alert(primitive_id, price))
                .is_some(),
            Some(UserPriceAlertsHit::Remove(alert_id)) => {
                self.remove_user_price_alert(primitive_id, alert_id)
            }
            None => false,
        }
    }

    /// Route a normal chart click through every attached native interactive primitive. Browser
    /// hosts forward one normalized sample; price conversion and resulting chart state stay in
    /// the shared engine.
    pub fn click_native_primitives_at(&mut self, x: f64, y: f64) -> bool {
        if !x.is_finite() || !y.is_finite() {
            return false;
        }
        let alert_ids: Vec<_> = self
            .series
            .iter()
            .flat_map(|series| {
                series.native_primitives.iter().filter_map(|primitive| {
                    matches!(
                        primitive.kind,
                        NativeSeriesPrimitiveKind::UserPriceAlerts(_)
                    )
                    .then_some(primitive.id)
                })
            })
            .collect();
        let mut changed = false;
        for primitive_id in alert_ids {
            changed |= self.click_user_price_alerts(primitive_id, x, y);
        }

        let pane = self.pane_at_y(y);
        let pane_w = self.pane_w;
        let add_lines: Vec<_> = self
            .series
            .iter()
            .filter(|series| Some(series.pane_index) == pane)
            .flat_map(|series| {
                series
                    .native_primitives
                    .iter()
                    .filter_map(move |primitive| {
                        let NativeSeriesPrimitiveKind::UserPriceLinesButton(options) =
                            primitive.kind
                        else {
                            return None;
                        };
                        (x >= 0.0 && pane_w - x >= 0.0 && pane_w - x <= 21.0)
                            .then_some((series.id, options.color))
                    })
            })
            .collect();
        for (series_id, color) in add_lines {
            if let Some(price) = self.series_coordinate_to_price(series_id, y) {
                self.create_price_line(series_id, price, color, 1, LineStyle::Dashed, "");
                changed = true;
            }
        }
        changed
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_trend_line(
        &mut self,
        series_id: SeriesId,
        first_time: i64,
        first_price: f64,
        second_time: i64,
        second_price: f64,
        options: TrendLineOptions,
    ) -> Option<NativePrimitiveId> {
        valid_trend_line(first_time, first_price, second_time, second_price, &options)
            .then_some(())?;
        self.insert_native_primitive(
            series_id,
            NativeSeriesPrimitiveKind::TrendLine {
                first_time,
                first_price,
                second_time,
                second_price,
                options,
            },
        )
    }

    pub fn add_expiring_price_alerts(
        &mut self,
        series_id: SeriesId,
        options: ExpiringPriceAlertsOptions,
    ) -> Option<NativePrimitiveId> {
        valid_expiring_alert_options(options).then_some(())?;
        let last_value = {
            let plot = self.data.plot(series_id);
            plot.last_non_whitespace_row(i64::MAX)
                .map(|row| {
                    plot.value_at(
                        row,
                        nucleuscharts_core::model::plot_list::PlotValueIndex::Close,
                    )
                })
                .filter(|value| value.is_finite())
        };
        self.insert_native_primitive(
            series_id,
            NativeSeriesPrimitiveKind::ExpiringPriceAlerts(ExpiringPriceAlertsState {
                options,
                alerts: Vec::new(),
                next_alert_id: 1,
                last_value,
            }),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_expiring_price_alert(
        &mut self,
        primitive_id: NativePrimitiveId,
        price: f64,
        start: i64,
        end: i64,
        title: String,
        crossing_direction: AlertCrossingDirection,
    ) -> Option<u32> {
        if !price.is_finite() || start > end || title.len() > MAX_NATIVE_TEXT_BYTES {
            return None;
        }
        let series_id = self.series.iter().find_map(|series| {
            series
                .native_primitives
                .iter()
                .any(|primitive| primitive.id == primitive_id)
                .then_some(series.id)
        })?;
        let last_plot_time = self
            .data
            .series_data(series_id)
            .and_then(|(times, _)| times.last().copied());
        let state = self.series.iter_mut().find_map(|series| {
            series.native_primitives.iter_mut().find_map(|primitive| {
                (primitive.id == primitive_id)
                    .then_some(&mut primitive.kind)
                    .and_then(|kind| {
                        let NativeSeriesPrimitiveKind::ExpiringPriceAlerts(state) = kind else {
                            return None;
                        };
                        Some(state)
                    })
            })
        })?;
        let timeline_start = state
            .alerts
            .iter()
            .map(|alert| alert.start)
            .chain([start])
            .chain(last_plot_time)
            .min()?;
        let timeline_end = state
            .alerts
            .iter()
            .map(|alert| alert.end)
            .chain([end])
            .max()?;
        let span = timeline_end.checked_sub(timeline_start)?;
        let points = usize::try_from(span / state.options.interval)
            .ok()?
            .checked_add(1)?;
        if points > MAX_EXPIRING_ALERT_TIME_POINTS
            || state.alerts.len() >= MAX_EXPIRING_PRICE_ALERTS
        {
            return None;
        }
        let id = state.next_alert_id;
        state.next_alert_id = id.checked_add(1)?;
        state.alerts.push(ExpiringPriceAlert {
            id,
            price,
            start,
            end,
            title,
            crossing_direction,
            crossed: false,
            expired: false,
            remove_at_ms: None,
        });
        self.invalidate_frame_series(series_id);
        self.sync_native_time_points();
        Some(id)
    }

    pub fn remove_expiring_price_alert(
        &mut self,
        primitive_id: NativePrimitiveId,
        alert_id: u32,
    ) -> bool {
        let Some((series_id, state)) = self.series.iter_mut().find_map(|series| {
            series.native_primitives.iter_mut().find_map(|primitive| {
                (primitive.id == primitive_id)
                    .then_some(&mut primitive.kind)
                    .and_then(|kind| {
                        let NativeSeriesPrimitiveKind::ExpiringPriceAlerts(state) = kind else {
                            return None;
                        };
                        Some((series.id, state))
                    })
            })
        }) else {
            return false;
        };
        let Some(index) = state.alerts.iter().position(|alert| alert.id == alert_id) else {
            return false;
        };
        state.alerts.remove(index);
        self.invalidate_frame_series(series_id);
        self.sync_native_time_points();
        true
    }

    /// Advance alert crossing/expiry state from the owning series' latest data point. Returns the
    /// delay until the next faded alert should be removed, if any.
    pub fn refresh_expiring_price_alerts(
        &mut self,
        primitive_id: NativePrimitiveId,
        time: i64,
        value: f64,
        now_ms: f64,
    ) -> Option<f64> {
        if !value.is_finite() || !now_ms.is_finite() {
            return None;
        }
        let (series_id, state) = self.series.iter_mut().find_map(|series| {
            series.native_primitives.iter_mut().find_map(|primitive| {
                (primitive.id == primitive_id)
                    .then_some(&mut primitive.kind)
                    .and_then(|kind| {
                        let NativeSeriesPrimitiveKind::ExpiringPriceAlerts(state) = kind else {
                            return None;
                        };
                        Some((series.id, state))
                    })
            })
        })?;
        let mut changed = false;
        if let Some(previous) = state.last_value {
            for alert in &mut state.alerts {
                if alert.crossed {
                    continue;
                }
                let crossed = match alert.crossing_direction {
                    AlertCrossingDirection::Up => previous <= alert.price && value > alert.price,
                    AlertCrossingDirection::Down => previous >= alert.price && value < alert.price,
                };
                if crossed {
                    alert.crossed = true;
                    alert.remove_at_ms = Some(now_ms + state.options.clear_timeout_ms);
                    changed = true;
                }
            }
        }
        state.last_value = Some(value);
        for alert in &mut state.alerts {
            if alert.end <= time && !alert.expired {
                alert.expired = true;
                alert
                    .remove_at_ms
                    .get_or_insert(now_ms + state.options.clear_timeout_ms);
                changed = true;
            }
        }
        let before = state.alerts.len();
        state
            .alerts
            .retain(|alert| alert.remove_at_ms.is_none_or(|deadline| deadline > now_ms));
        let removed = state.alerts.len() != before;
        let next = state
            .alerts
            .iter()
            .filter_map(|alert| alert.remove_at_ms)
            .map(|deadline| (deadline - now_ms).max(0.0))
            .min_by(f64::total_cmp);
        if changed || removed {
            self.invalidate_frame_series(series_id);
        }
        if removed {
            self.sync_native_time_points();
        }
        next
    }

    pub fn expiring_price_alerts(
        &self,
        primitive_id: NativePrimitiveId,
    ) -> Option<&[ExpiringPriceAlert]> {
        self.series.iter().find_map(|series| {
            series.native_primitives.iter().find_map(|primitive| {
                if primitive.id != primitive_id {
                    return None;
                }
                let NativeSeriesPrimitiveKind::ExpiringPriceAlerts(state) = &primitive.kind else {
                    return None;
                };
                Some(state.alerts.as_slice())
            })
        })
    }

    pub(crate) fn sync_native_time_points(&mut self) {
        let mut times = Vec::new();
        for series in &self.series {
            let last_plot_time = self
                .data
                .series_data(series.id)
                .and_then(|(series_times, _)| series_times.last().copied());
            for state in series.native_primitives.iter().filter_map(|primitive| {
                let NativeSeriesPrimitiveKind::ExpiringPriceAlerts(state) = &primitive.kind else {
                    return None;
                };
                Some(state)
            }) {
                let Some(mut time) = state
                    .alerts
                    .iter()
                    .map(|alert| alert.start)
                    .chain(last_plot_time)
                    .min()
                else {
                    continue;
                };
                let Some(end) = state.alerts.iter().map(|alert| alert.end).max() else {
                    continue;
                };
                while time <= end {
                    times.push(time);
                    let Some(next) = time.checked_add(state.options.interval) else {
                        break;
                    };
                    time = next;
                }
            }
        }
        if self.data.set_auxiliary_times(times) {
            self.sync_time_points();
        }
    }

    pub fn add_volume_profile(
        &mut self,
        series_id: SeriesId,
        data: VolumeProfileData,
        options: VolumeProfileOptions,
    ) -> Option<NativePrimitiveId> {
        valid_volume_profile(&data).then_some(())?;
        self.insert_native_primitive(
            series_id,
            NativeSeriesPrimitiveKind::VolumeProfile { data, options },
        )
    }

    pub fn add_image_watermark(
        &mut self,
        series_id: SeriesId,
        width: u32,
        height: u32,
        pixels: Arc<[u8]>,
        options: ImageWatermarkOptions,
    ) -> Option<NativePrimitiveId> {
        let expected = usize::try_from(width)
            .ok()?
            .checked_mul(usize::try_from(height).ok()?)?
            .checked_mul(4)?;
        if width == 0
            || height == 0
            || width > MAX_RASTER_IMAGE_DIMENSION
            || height > MAX_RASTER_IMAGE_DIMENSION
            || pixels.len() != expected
            || !valid_image_options(options)
        {
            return None;
        }
        self.insert_native_primitive(
            series_id,
            NativeSeriesPrimitiveKind::ImageWatermark {
                image: RasterImage {
                    key: u64::from(self.next_native_primitive_id),
                    width,
                    height,
                    pixels,
                },
                options,
            },
        )
    }

    pub fn add_anchored_text(
        &mut self,
        series_id: SeriesId,
        options: AnchoredTextOptions,
    ) -> Option<NativePrimitiveId> {
        valid_anchored_text_options(&options).then_some(())?;
        self.insert_native_primitive(series_id, NativeSeriesPrimitiveKind::AnchoredText(options))
    }

    pub fn add_text_watermark(
        &mut self,
        pane_index: usize,
        options: TextWatermarkOptions,
    ) -> Option<NativePrimitiveId> {
        valid_text_watermark_options(&options).then_some(())?;
        self.insert_native_pane_primitive(
            pane_index,
            NativePanePrimitiveKind::TextWatermark(options),
        )
    }

    pub fn set_text_watermark_options(
        &mut self,
        id: NativePrimitiveId,
        options: TextWatermarkOptions,
    ) -> bool {
        if !valid_text_watermark_options(&options) {
            return false;
        }
        let Some(primitive) = self
            .native_pane_primitives
            .iter_mut()
            .find(|primitive| primitive.id == id)
        else {
            return false;
        };
        let NativePanePrimitiveKind::TextWatermark(current) = &mut primitive.kind;
        *current = options;
        self.invalidate_frame_scene();
        true
    }

    pub fn set_anchored_text_options(
        &mut self,
        id: NativePrimitiveId,
        options: AnchoredTextOptions,
    ) -> bool {
        if !valid_anchored_text_options(&options) {
            return false;
        }
        let Some((series_id, current)) = self.series.iter_mut().find_map(|series| {
            series.native_primitives.iter_mut().find_map(|primitive| {
                (primitive.id == id).then_some(()).and_then(|()| {
                    let NativeSeriesPrimitiveKind::AnchoredText(current) = &mut primitive.kind
                    else {
                        return None;
                    };
                    Some((series.id, current))
                })
            })
        }) else {
            return false;
        };
        *current = options;
        self.invalidate_frame_series(series_id);
        true
    }

    pub fn set_volume_profile_data(
        &mut self,
        id: NativePrimitiveId,
        data: VolumeProfileData,
    ) -> bool {
        if !valid_volume_profile(&data) {
            return false;
        }
        let Some((series_id, primitive)) = self.series.iter_mut().find_map(|series| {
            series
                .native_primitives
                .iter_mut()
                .find(|primitive| primitive.id == id)
                .map(|primitive| (series.id, primitive))
        }) else {
            return false;
        };
        let NativeSeriesPrimitiveKind::VolumeProfile {
            data: current_data, ..
        } = &mut primitive.kind
        else {
            return false;
        };
        *current_data = data;
        self.invalidate_frame_series(series_id);
        true
    }

    pub fn remove_native_primitive(&mut self, id: NativePrimitiveId) -> bool {
        if let Some((series_id, index)) = self.series.iter().find_map(|series| {
            series
                .native_primitives
                .iter()
                .position(|primitive| primitive.id == id)
                .map(|index| (series.id, index))
        }) {
            let Some(series) = self.series_entry_mut(series_id) else {
                return false;
            };
            let removed_alerts = matches!(
                series.native_primitives[index].kind,
                NativeSeriesPrimitiveKind::ExpiringPriceAlerts(_)
            );
            let removed_user_alerts = matches!(
                series.native_primitives[index].kind,
                NativeSeriesPrimitiveKind::UserPriceAlerts(_)
            );
            let removed_delta_tooltip = matches!(
                series.native_primitives[index].kind,
                NativeSeriesPrimitiveKind::DeltaTooltip(_)
            );
            let removed_tooltip = matches!(
                series.native_primitives[index].kind,
                NativeSeriesPrimitiveKind::Tooltip(_)
            );
            series.native_primitives.remove(index);
            self.invalidate_frame_series(series_id);
            if removed_user_alerts || removed_delta_tooltip || removed_tooltip {
                self.invalidate_frame_overlay();
                if removed_user_alerts || removed_delta_tooltip {
                    self.invalidate_axis_frame();
                }
            }
            if removed_alerts {
                self.sync_native_time_points();
            }
            return true;
        }
        let Some(index) = self
            .native_pane_primitives
            .iter()
            .position(|primitive| primitive.id == id)
        else {
            return false;
        };
        self.native_pane_primitives.remove(index);
        self.invalidate_frame_scene();
        true
    }

    pub(crate) fn native_primitive_capacity_bytes(&self) -> usize {
        self.series
            .iter()
            .map(|series| {
                series.native_primitives.capacity() * core::mem::size_of::<NativeSeriesPrimitive>()
                    + series
                        .native_primitives
                        .iter()
                        .map(NativeSeriesPrimitive::capacity_bytes)
                        .sum::<usize>()
            })
            .sum::<usize>()
            + self.native_pane_primitives.capacity() * core::mem::size_of::<NativePanePrimitive>()
            + self
                .native_pane_primitives
                .iter()
                .map(|primitive| match &primitive.kind {
                    NativePanePrimitiveKind::TextWatermark(options) => {
                        options.lines.capacity() * core::mem::size_of::<TextWatermarkLine>()
                            + options
                                .lines
                                .iter()
                                .map(|line| line.text.capacity() + line.font_family.capacity())
                                .sum::<usize>()
                    }
                })
                .sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nucleuscharts_render::draw_list::Prim;
    use std::sync::Arc;

    fn chart() -> ChartEngine {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let times: Vec<f64> = (0..10).map(|day| day as f64 * 86_400.0).collect();
        let close: Vec<f64> = (0..10).map(|day| 100.0 + day as f64).collect();
        let open: Vec<f64> = close.iter().map(|value| value - 1.0).collect();
        let high: Vec<f64> = close.iter().map(|value| value + 2.0).collect();
        let low: Vec<f64> = close.iter().map(|value| value - 2.0).collect();
        chart
            .set_series_data(0, &times, &open, &high, &low, &close)
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        chart.series[0].price_line_visible = false;
        chart
    }

    fn profile() -> VolumeProfileData {
        VolumeProfileData {
            time: 2 * 86_400,
            profile: vec![
                VolumeProfilePoint {
                    price: 95.0,
                    volume: 4.0,
                },
                VolumeProfilePoint {
                    price: 100.0,
                    volume: 10.0,
                },
                VolumeProfilePoint {
                    price: 105.0,
                    volume: 6.0,
                },
            ],
            width: 3.0,
        }
    }

    #[test]
    fn official_primitives_emit_shared_underlay_and_series_geometry() {
        let mut chart = chart();
        chart.add_partial_price_line(0).unwrap();
        chart
            .add_session_highlighting(0, SessionHighlightingOptions::default())
            .unwrap();
        let highlight = Color::rgba(10, 20, 30, 51);
        chart.add_highlight_bar_crosshair(0, highlight).unwrap();
        let profile_options = VolumeProfileOptions::default();
        chart
            .add_volume_profile(0, profile(), profile_options)
            .unwrap();
        let x = chart.time_scale.index_to_coordinate(4);
        chart.set_crosshair_at(x, 250.0);

        let frame = chart.build_frame();
        let pane = &frame.panes[0];
        assert!(pane.under.iter().any(|primitive| matches!(
            primitive,
            Prim::Rect { color, .. } if *color == SessionHighlightingOptions::default().weekday_color
        )));
        assert!(pane.under.iter().any(|primitive| matches!(
            primitive,
            Prim::Rect { color, .. } if *color == SessionHighlightingOptions::default().weekend_color
        )));
        assert!(pane
            .under
            .iter()
            .any(|primitive| matches!(primitive, Prim::Rect { color, .. } if *color == highlight)));
        assert!(pane.main.iter().any(|primitive| matches!(
            primitive,
            Prim::Rect { color, .. } if *color == profile_options.background_color
        )));
        assert!(pane.main.iter().any(|primitive| matches!(
            primitive,
            Prim::Rect { color, .. } if *color == profile_options.row_color
        )));
        // The official partial line's 4:2 dash is emitted as exact solid runs rather than the
        // generic line-style approximation.
        assert!(
            pane.main
                .iter()
                .filter(|primitive| matches!(
                    primitive,
                    Prim::HLine {
                        style: nucleuscharts_render::draw_list::LineStyle::Solid,
                        ..
                    }
                ))
                .count()
                > 2
        );
    }

    #[test]
    fn bands_indicator_uses_official_ten_percent_data_background_and_visible_autoscale() {
        let mut chart = chart();
        let options = BandsIndicatorOptions::default();
        let id = chart.add_bands_indicator(0, options).unwrap();
        let frame = chart.build_frame();
        let pane = &frame.panes[0];
        assert!(matches!(
            pane.main.as_slice(),
            [
                Prim::Polyline { color: upper, width: upper_width, .. },
                Prim::Polyline { color: lower, width: lower_width, .. },
                Prim::BandFill { fill, point_count: 10, .. },
                ..
            ] if *upper == options.line_color
                && *lower == options.line_color
                && *fill == options.fill_color
                && *upper_width == 1.0
                && *lower_width == 1.0
        ));
        let Prim::BandFill {
            upper_first,
            lower_first,
            point_count,
            ..
        } = &pane.main[2]
        else {
            unreachable!();
        };
        assert_eq!(*point_count, 10);
        let upper = pane.points[*upper_first as usize];
        let lower = pane.points[*lower_first as usize];
        assert!((upper[0] - chart.time_scale.index_to_coordinate(0) as f32).abs() < 1e-4);
        assert!(
            (upper[1] - chart.series_price_to_coordinate(0, 110.0).unwrap() as f32).abs() < 1e-4
        );
        assert!(
            (lower[1] - chart.series_price_to_coordinate(0, 90.0).unwrap() as f32).abs() < 1e-4
        );
        let range = chart.panes[0].price_scale.price_range().unwrap();
        assert!(range.min_value() <= 90.0);
        assert!(range.max_value() >= 109.0 * 1.1);

        let updated = BandsIndicatorOptions {
            line_color: Color::rgb(1, 2, 3),
            fill_color: Color::rgba(4, 5, 6, 70),
            line_width: 3.0,
        };
        assert!(chart.set_bands_indicator_options(id, updated));
        assert!(!chart.set_bands_indicator_options(
            id,
            BandsIndicatorOptions {
                line_width: f64::NAN,
                ..updated
            }
        ));
        let frame = chart.build_frame();
        assert!(matches!(
            frame.panes[0].main.as_slice(),
            [Prim::Polyline { color, width, .. }, ..]
                if *color == updated.line_color && *width == 3.0
        ));
    }

    #[test]
    fn overlay_price_scale_emits_unboxed_in_pane_text_and_updates_side() {
        let mut chart = chart();
        chart.set_series_price_scale(0, crate::PriceScaleTarget::Overlay);
        let defaults = OverlayPriceScaleOptions::default();
        let id = chart.add_overlay_price_scale(0, defaults).unwrap();
        let frame = chart.build_frame();
        assert!(!frame.panes[0]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::RoundRect { .. })));
        let labels = frame.panes[0]
            .main
            .iter()
            .filter(|primitive| {
                matches!(primitive, Prim::Text { color, size, .. }
                    if *color == defaults.text_color && *size == 12.0)
            })
            .count();
        assert_eq!(labels, 13);

        let right = OverlayPriceScaleOptions {
            text_color: Color::rgb(1, 2, 3),
            side: OverlayPriceScaleSide::Right,
        };
        assert!(chart.set_overlay_price_scale_options(id, right));
        let frame = chart.build_frame();
        assert!(frame.panes[0].main.iter().any(|primitive| {
            matches!(primitive, Prim::Text { x, color, .. }
                if *color == right.text_color && *x > 700.0)
        }));
    }

    #[test]
    fn session_highlighting_retains_exact_callback_colors_aligned_to_source_times() {
        let mut chart = chart();
        let primitive = chart
            .add_session_highlighting(0, SessionHighlightingOptions::default())
            .unwrap();
        let first = Color::rgba(1, 2, 3, 40);
        let second = Color::rgba(4, 5, 6, 50);
        let highlights = (0..10)
            .map(|day| SessionHighlightingData {
                time: day * 86_400,
                color: if day % 2 == 0 { first } else { second },
            })
            .collect();
        assert!(chart.set_session_highlighting_data(primitive, highlights));
        let frame = chart.build_frame();
        assert!(frame.panes[0]
            .under
            .iter()
            .any(|primitive| matches!(primitive, Prim::Rect { color, .. } if *color == first)));
        assert!(frame.panes[0]
            .under
            .iter()
            .any(|primitive| matches!(primitive, Prim::Rect { color, .. } if *color == second)));
        assert!(!chart.set_session_highlighting_data(
            primitive,
            vec![SessionHighlightingData {
                time: 86_400,
                color: first,
            }],
        ));
    }

    #[test]
    fn volume_profile_validation_is_transactional_and_memory_is_attributed() {
        let mut chart = chart();
        let id = chart
            .add_volume_profile(0, profile(), VolumeProfileOptions::default())
            .unwrap();
        let before = chart.build_frame();
        let malformed = VolumeProfileData {
            time: 0,
            profile: vec![VolumeProfilePoint {
                price: f64::NAN,
                volume: 1.0,
            }],
            width: 1.0,
        };
        assert!(!chart.set_volume_profile_data(id, malformed));
        assert_eq!(before, chart.build_frame());
        assert!(chart.memory_usage().native_primitive_capacity_bytes > 0);
        assert!(chart.remove_native_primitive(id));
        assert!(!chart.remove_native_primitive(id));
    }

    #[test]
    fn expiring_alerts_own_timeline_state_autoscale_and_geometry() {
        let mut chart = chart();
        let primitive = chart
            .add_expiring_price_alerts(
                0,
                ExpiringPriceAlertsOptions {
                    interval: 86_400,
                    clear_timeout_ms: 3_000.0,
                },
            )
            .unwrap();
        let alert = chart
            .add_expiring_price_alert(
                primitive,
                120.0,
                9 * 86_400,
                12 * 86_400,
                "$120".into(),
                AlertCrossingDirection::Up,
            )
            .unwrap();
        assert_eq!(alert, 1);
        assert_eq!(chart.data.merged_times().len(), 13);
        assert_eq!(chart.data.base_index(), Some(9));

        chart.build_frame();
        assert!(chart.panes[0]
            .price_scale
            .price_range()
            .is_some_and(|range| range.max_value() >= 120.0));
        let frame = chart.build_frame();
        assert!(frame.panes[0].main.iter().any(|primitive| matches!(
            primitive,
            Prim::Text { text, .. } if text == "$120"
        )));
        assert!(frame.panes[0].main.iter().any(|primitive| matches!(
            primitive,
            Prim::Circle { fill, .. } if *fill == Color::rgb(0x64, 0xc7, 0x50)
        )));

        assert_eq!(
            chart.refresh_expiring_price_alerts(primitive, 10 * 86_400, 121.0, 1_000.0),
            Some(3_000.0)
        );
        assert!(chart.expiring_price_alerts(primitive).unwrap()[0].crossed);
        let crossed = chart.build_frame();
        assert!(crossed.panes[0].main.iter().any(|primitive| matches!(
            primitive,
            Prim::Circle { fill, .. } if *fill == Color::rgb(0x38, 0x6d, 0x2e)
        )));

        assert_eq!(
            chart.refresh_expiring_price_alerts(primitive, 10 * 86_400, 121.0, 4_000.0),
            None
        );
        assert!(chart.expiring_price_alerts(primitive).unwrap().is_empty());
        assert_eq!(chart.data.merged_times().len(), 10);
    }

    #[test]
    fn expiring_alert_timeline_steps_from_last_source_bar_without_forcing_the_end() {
        let mut chart = chart();
        let primitive = chart
            .add_expiring_price_alerts(
                0,
                ExpiringPriceAlertsOptions {
                    interval: 100,
                    clear_timeout_ms: 3_000.0,
                },
            )
            .unwrap();
        let last = 9 * 86_400;
        let start = last + 100;
        let end = last + 250;
        chart
            .add_expiring_price_alert(
                primitive,
                105.0,
                start,
                end,
                "stepped".into(),
                AlertCrossingDirection::Up,
            )
            .unwrap();
        assert!(chart.data.merged_times().contains(&last));
        assert!(chart.data.merged_times().contains(&(last + 100)));
        assert!(chart.data.merged_times().contains(&(last + 200)));
        assert!(!chart.data.merged_times().contains(&end));
        // The official primitive returns a normal `paneViews()` entry (no `zOrder()` override),
        // so its label belongs in the series/main layer rather than the underlay.
        assert!(chart.build_frame().panes[0]
            .main
            .iter()
            .any(|primitive| matches!(
                primitive,
                Prim::Text { text, .. } if text == "stepped"
            )));
    }

    #[test]
    fn delta_tooltip_owns_pointer_state_sorted_range_and_official_frame_content() {
        let mut chart = chart();
        let options = DeltaTooltipOptions::default();
        let primitive = chart.add_delta_tooltip(0, options).unwrap();
        let x2 = chart.time_scale.index_to_coordinate(2);
        let x7 = chart.time_scale.index_to_coordinate(7);

        assert!(chart.delta_tooltip_mouse_down(x7));
        assert!(chart.delta_tooltip_mouse_move(x2));
        assert_eq!(
            chart.delta_tooltip_active_range(primitive),
            Some(DeltaTooltipActiveRange {
                from: 3,
                to: 8,
                positive: true,
            })
        );
        let frame = chart.build_frame();
        assert_eq!(
            frame.panes[0]
                .main
                .iter()
                .filter(|primitive| matches!(
                    primitive,
                    Prim::VLine { color, .. } if *color == options.line_color
                ))
                .count(),
            2
        );
        assert!(frame.panes[0].main.iter().any(|primitive| matches!(
            primitive,
            Prim::Circle { radius, .. } if (*radius - 6.0).abs() < f32::EPSILON
        )));
        let tooltip_boxes: Vec<_> = frame.panes[0]
            .main
            .iter()
            .filter_map(|primitive| {
                let Prim::RoundRect {
                    fill,
                    border_width,
                    border_color,
                    radii,
                    ..
                } = primitive
                else {
                    return None;
                };
                Some((*fill, *border_width, *border_color, *radii))
            })
            .collect();
        assert_eq!(tooltip_boxes.len(), 1, "tooltip must not emit a shadow box");
        assert_eq!(tooltip_boxes[0].0, Color::rgb(0x07, 0x0a, 0x0f));
        assert!(tooltip_boxes[0].1 > 0.0);
        assert_eq!(tooltip_boxes[0].2, Color::rgb(0x16, 0x19, 0x1f));
        assert!(tooltip_boxes[0].3.iter().all(|radius| *radius == 6.0));
        assert!(frame.panes[0].main.iter().any(|primitive| matches!(
            primitive,
            Prim::Rect { color, .. } if *color == Color::rgba(4, 153, 129, 51)
        )));
        for expected in ["102.00", "107.00", "+5.00", "+4.90%"] {
            assert!(frame.panes[0].main.iter().any(|primitive| matches!(
                primitive,
                Prim::Text { text, .. } if text == expected
            )));
        }

        // Mouse-up commits the comparison; later hover and leave must not clear it.
        assert!(chart.delta_tooltip_mouse_up());
        assert!(chart.delta_tooltip_active_range(primitive).is_some());
        chart.delta_tooltip_mouse_move(x2);
        assert!(chart.delta_tooltip_active_range(primitive).is_some());
        chart.delta_tooltip_leave();
        assert!(chart.delta_tooltip_active_range(primitive).is_some());

        // Touch order is normalized by logical index, matching the official active-range contract.
        chart.delta_tooltip_touch_move(&[x7, x2]);
        assert_eq!(
            chart.delta_tooltip_active_range(primitive),
            Some(DeltaTooltipActiveRange {
                from: 3,
                to: 8,
                positive: true,
            })
        );
        assert!(!chart.delta_tooltip_touch_move(&[x2, x7, x2]));
        assert!(chart.delta_tooltip_active_range(primitive).is_some());
        chart.delta_tooltip_leave();
        assert!(chart.delta_tooltip_active_range(primitive).is_some());
        assert!(chart.build_frame().panes[0].main.iter().any(|primitive| {
            matches!(primitive, Prim::VLine { color, .. } if *color == options.line_color)
        }));

        assert!(chart.clear_delta_tooltip(primitive));
        assert!(chart.delta_tooltip_active_range(primitive).is_none());
        assert!(chart.build_frame().panes[0].main.iter().all(|primitive| {
            !matches!(primitive, Prim::VLine { color, .. } if *color == options.line_color)
        }));
    }

    #[test]
    fn delta_tooltip_reads_runtime_chart_theme_and_font_options() {
        let mut chart = chart();
        let primitive = chart
            .add_delta_tooltip(0, DeltaTooltipOptions::default())
            .unwrap();
        let x2 = chart.time_scale.index_to_coordinate(2);
        let x7 = chart.time_scale.index_to_coordinate(7);
        assert!(chart.delta_tooltip_mouse_down(x2));
        assert!(chart.delta_tooltip_mouse_move(x7));
        assert!(chart.delta_tooltip_mouse_up());
        chart
            .apply_options(
                r##"{
                    "layout": {
                        "background": { "color": "#112233" },
                        "textColor": "#ddeeff",
                        "mutedTextColor": "#778899",
                        "fontSize": 15,
                        "fontFamily": "Theme Test"
                    },
                    "rightPriceScale": { "borderColor": "#445566" }
                }"##,
            )
            .unwrap();

        let frame = chart.build_frame();
        assert!(frame.panes[0].main.iter().any(|primitive| matches!(
            primitive,
            Prim::RoundRect { fill, border_color, radii, .. }
                if *fill == Color::rgb(0x11, 0x22, 0x33)
                    && *border_color == Color::rgb(0x44, 0x55, 0x66)
                    && radii.iter().all(|radius| *radius == 6.0)
        )));
        assert!(frame.panes[0].main.iter().any(|primitive| matches!(
            primitive,
            Prim::Text { color, family, size, .. }
                if *color == Color::rgb(0xdd, 0xee, 0xff)
                    && family == "Theme Test"
                    && (*size - 17.0).abs() < f32::EPSILON
        )));
        assert!(frame.panes[0].main.iter().any(|primitive| matches!(
            primitive,
            Prim::Text { color, family, size, .. }
                if *color == Color::rgb(0x77, 0x88, 0x99)
                    && family == "Theme Test"
                    && (*size - 15.0).abs() < f32::EPSILON
        )));
        assert!(chart.clear_delta_tooltip(primitive));
    }

    #[test]
    fn delta_tooltip_replaces_only_with_complete_mouse_or_touch_ranges() {
        let mut chart = chart();
        let primitive = chart
            .add_delta_tooltip(0, DeltaTooltipOptions::default())
            .unwrap();
        let x1 = chart.time_scale.index_to_coordinate(1);
        let x2 = chart.time_scale.index_to_coordinate(2);
        let x4 = chart.time_scale.index_to_coordinate(4);
        let x5 = chart.time_scale.index_to_coordinate(5);
        let x7 = chart.time_scale.index_to_coordinate(7);
        let x8 = chart.time_scale.index_to_coordinate(8);

        assert!(chart.delta_tooltip_mouse_down(x2));
        assert!(chart.delta_tooltip_mouse_move(x7));
        assert!(chart.delta_tooltip_mouse_up());
        let first = chart.delta_tooltip_active_range(primitive).unwrap();

        // A click or cancelled replacement does not erase the committed range.
        assert!(chart.delta_tooltip_mouse_down(x4));
        chart.delta_tooltip_mouse_up();
        assert_eq!(chart.delta_tooltip_active_range(primitive), Some(first));

        assert!(chart.delta_tooltip_mouse_down(x1));
        assert!(chart.delta_tooltip_mouse_move(x5));
        assert!(chart.delta_tooltip_mouse_up());
        let replacement = chart.delta_tooltip_active_range(primitive).unwrap();
        assert_ne!(replacement, first);

        // Two touches commit immediately. Movement by the sole survivor is only a preview.
        assert!(chart.delta_tooltip_touch_move(&[x7, x8]));
        let touch_range = chart.delta_tooltip_active_range(primitive).unwrap();
        assert_ne!(touch_range, replacement);
        assert!(chart.delta_tooltip_touch_move(&[x4]));
        assert_eq!(
            chart.delta_tooltip_active_range(primitive),
            Some(touch_range)
        );
        chart.delta_tooltip_leave();
        assert_eq!(
            chart.delta_tooltip_active_range(primitive),
            Some(touch_range)
        );

        assert!(chart.remove_native_primitive(primitive));
        assert_eq!(chart.delta_tooltip_active_range(primitive), None);
    }

    #[test]
    fn delta_tooltip_direction_uses_chronological_prices_not_drag_order() {
        let mut chart = chart();
        let times: Vec<f64> = (0..10).map(|day| day as f64 * 86_400.0).collect();
        let close: Vec<f64> = (0..10).map(|day| 110.0 - day as f64).collect();
        let open: Vec<f64> = close.iter().map(|value| value + 1.0).collect();
        let high: Vec<f64> = close.iter().map(|value| value + 2.0).collect();
        let low: Vec<f64> = close.iter().map(|value| value - 2.0).collect();
        chart
            .set_series_data(0, &times, &open, &high, &low, &close)
            .unwrap();
        chart.fit_content();
        let primitive = chart
            .add_delta_tooltip(0, DeltaTooltipOptions::default())
            .unwrap();
        let x2 = chart.time_scale.index_to_coordinate(2);
        let x7 = chart.time_scale.index_to_coordinate(7);

        for (start, end) in [(x2, x7), (x7, x2)] {
            assert!(chart.delta_tooltip_mouse_down(start));
            assert!(chart.delta_tooltip_mouse_move(end));
            assert_eq!(
                chart.delta_tooltip_active_range(primitive),
                Some(DeltaTooltipActiveRange {
                    from: 3,
                    to: 8,
                    positive: false,
                })
            );
            assert!(chart.delta_tooltip_mouse_up());
        }

        chart.delta_tooltip_touch_move(&[x7, x2]);
        assert_eq!(
            chart.delta_tooltip_active_range(primitive),
            Some(DeltaTooltipActiveRange {
                from: 3,
                to: 8,
                positive: false,
            })
        );
    }

    #[test]
    fn tooltip_snapshot_and_bottom_guide_are_owned_by_the_engine() {
        let mut chart = chart();
        let color = Color::rgb(12, 34, 56);
        let primitive = chart
            .add_tooltip(
                0,
                TooltipOptions {
                    line_color: color,
                    top_margin: 30.0,
                },
            )
            .unwrap();
        let _ = chart.build_frame();
        let x = chart.time_scale.index_to_coordinate(4);
        chart.set_crosshair_at(x, 200.0);
        assert_eq!(
            chart.tooltip_snapshot(primitive),
            Some(TooltipSnapshot {
                x,
                index: 4,
                price: 104.0,
                time: 4 * 86_400,
            })
        );
        assert!(chart.build_frame().panes[0]
            .under
            .iter()
            .any(|primitive| matches!(
                primitive,
                Prim::Rect { rect, color: actual }
                    if *actual == color && rect.y == 30 && rect.h == 470
            )));
        assert!(chart.set_tooltip_options(primitive, TooltipOptions::default()));
        chart.clear_crosshair_at();
        assert!(chart.tooltip_snapshot(primitive).is_none());
        assert!(chart.build_frame().panes[0].under.iter().all(
            |primitive| !matches!(primitive, Prim::Rect { color: actual, .. } if *actual == color)
        ));
    }

    #[test]
    fn accessibility_focus_ring_uses_exact_engine_data_and_shared_overlay_geometry() {
        let mut chart = chart();
        let options = AccessibilityFocusOptions {
            color: Color::rgb(12, 34, 56),
            size: 14.0,
            high_contrast: false,
        };
        let primitive = chart.add_accessibility_focus(0, options).unwrap();
        let _ = chart.build_frame();
        assert!(chart.set_accessibility_focus(primitive, Some(4 * 86_400), options));
        let frame = chart.build_frame();
        assert!(frame.panes[0].main.iter().any(|primitive| matches!(
            primitive,
            Prim::Circle { radius, stroke, stroke_width, .. }
                if *stroke == options.color
                    && (*radius - 7.0).abs() < f32::EPSILON
                    && (*stroke_width - 2.0).abs() < f32::EPSILON
        )));
        assert!(chart.set_accessibility_focus(primitive, None, options));
        assert!(chart.build_frame().panes[0]
            .main
            .iter()
            .all(|primitive| !matches!(primitive, Prim::Circle { stroke, .. } if *stroke == options.color)));
    }

    #[test]
    fn image_watermark_is_centered_aspect_preserving_bounded_and_series_owned() {
        let mut chart = chart();
        let pixels = Arc::<[u8]>::from(vec![255; 4 * 2 * 4]);
        let id = chart
            .add_image_watermark(
                0,
                4,
                2,
                Arc::clone(&pixels),
                ImageWatermarkOptions {
                    max_width: Some(200.0),
                    max_height: Some(100.0),
                    padding: 8.0,
                    alpha: 0.4,
                },
            )
            .unwrap();
        let frame = chart.build_frame();
        let image = frame.panes[0]
            .under
            .iter()
            .find_map(|primitive| match primitive {
                Prim::Image {
                    image,
                    rect,
                    opacity,
                } => Some((image, rect, opacity)),
                _ => None,
            })
            .expect("watermark must be emitted through the shared underlay");
        assert_eq!(image.0.width, 4);
        assert_eq!(image.0.height, 2);
        assert_eq!(image.1[2], 200.0);
        assert_eq!(image.1[3], 100.0);
        assert_eq!(*image.2, 0.4);
        assert!((image.1[0] - 300.0).abs() < f32::EPSILON);
        assert!(chart.memory_usage().native_primitive_capacity_bytes >= pixels.len());

        chart.pane_set_preserve_empty(0, true);
        let second_pane = chart.add_pane(true).unwrap();
        chart.set_series_pane(0, second_pane, 1.0);
        let moved = chart.build_frame();
        assert!(moved.panes[0]
            .under
            .iter()
            .all(|primitive| !matches!(primitive, Prim::Image { .. })));
        assert!(moved.panes[second_pane]
            .under
            .iter()
            .any(|primitive| matches!(primitive, Prim::Image { .. })));
        assert!(chart.remove_native_primitive(id));

        assert!(chart
            .add_image_watermark(
                0,
                MAX_RASTER_IMAGE_DIMENSION + 1,
                1,
                Arc::<[u8]>::from(vec![0; 4]),
                ImageWatermarkOptions::default(),
            )
            .is_none());
        assert!(chart.build_frame().panes[0]
            .under
            .iter()
            .all(|primitive| !matches!(primitive, Prim::Image { .. })));
    }

    #[test]
    fn anchored_text_matches_viewport_alignment_updates_and_follows_its_series() {
        let mut chart = chart();
        let options = AnchoredTextOptions {
            horizontal_align: AnchoredTextHorizontalAlign::Middle,
            vertical_align: AnchoredTextVerticalAlign::Middle,
            text: "Anchored Text".into(),
            line_height: 54.0,
            font_size: 54.0,
            font_family: "Arial".into(),
            font_weight: 700,
            italic: true,
            color: Color::rgb(255, 0, 0),
        };
        let id = chart.add_anchored_text(0, options.clone()).unwrap();
        let frame = chart.build_frame();
        let text = frame.panes[0]
            .main
            .iter()
            .find_map(|primitive| match primitive {
                Prim::Text {
                    x,
                    y,
                    text,
                    align,
                    weight,
                    italic,
                    ..
                } if text == "Anchored Text" => Some((*x, *y, *align, *weight, *italic)),
                _ => None,
            })
            .expect("anchored text must be emitted into the shared pane frame");
        assert_eq!(text.0, 400.0);
        assert_eq!(text.1, 250.0);
        assert_eq!(text.2, nucleuscharts_render::draw_list::TextAlign::Center);
        assert_eq!(text.3, 700);
        assert!(text.4);

        chart.pane_set_preserve_empty(0, true);
        let second_pane = chart.add_pane(true).unwrap();
        chart.set_series_pane(0, second_pane, 1.0);
        let moved = chart.build_frame();
        assert!(moved.panes[0].main.iter().all(
            |primitive| !matches!(primitive, Prim::Text { text, .. } if text == "Anchored Text")
        ));
        assert!(moved.panes[second_pane].main.iter().any(
            |primitive| matches!(primitive, Prim::Text { text, .. } if text == "Anchored Text")
        ));
        chart.set_series_pane(0, 0, 1.0);
        assert!(chart.remove_pane(second_pane));

        let before = chart.build_frame();
        let mut malformed = options;
        malformed.font_size = f64::NAN;
        assert!(!chart.set_anchored_text_options(id, malformed));
        assert_eq!(before, chart.build_frame());

        assert!(chart.set_anchored_text_options(
            id,
            AnchoredTextOptions {
                horizontal_align: AnchoredTextHorizontalAlign::Right,
                vertical_align: AnchoredTextVerticalAlign::Bottom,
                text: "Updated".into(),
                line_height: 20.0,
                font_size: 18.0,
                font_family: "Arial".into(),
                font_weight: 400,
                italic: false,
                color: Color::rgb(0, 0, 0),
            },
        ));
        assert!(chart.build_frame().panes[0].main.iter().any(|primitive| {
            matches!(primitive, Prim::Text { x, y, text, align, .. }
                if text == "Updated" && *x == 780.0 && *y == 480.0
                    && *align == nucleuscharts_render::draw_list::TextAlign::Right)
        }));
    }

    #[test]
    fn vertical_line_emits_exact_pane_slot_and_time_axis_label() {
        let mut chart = chart();
        let color = Color::rgb(10, 20, 30);
        let label_background = Color::rgb(40, 50, 60);
        let label_text = Color::rgb(250, 251, 252);
        chart
            .add_vertical_line(
                0,
                4 * 86_400,
                VerticalLineOptions {
                    color,
                    label_text: "Event".into(),
                    width: 3.0,
                    label_background_color: label_background,
                    label_text_color: label_text,
                    show_label: true,
                },
            )
            .unwrap();
        let frame = chart.build_frame();
        assert!(frame.panes[0].main.iter().any(|primitive| {
            matches!(primitive, Prim::Rect { rect, color: actual }
                if *actual == color && rect.h == 500 && rect.w == 3)
        }));
        let axis = chart.build_axis_frame(80.0, |text| text.len() as f64 * 7.0);
        let label = axis
            .labels
            .iter()
            .find(|label| label.text == "Event")
            .expect("vertical line must contribute its time-axis view");
        assert_eq!(label.color, label_text);
        assert_eq!(label.background.unwrap().4, label_background);
        assert_eq!(label.background_corners, crate::AxisLabelCorners::BOTTOM);
    }

    #[test]
    fn user_price_lines_button_only_appears_near_scale_and_tracks_hover() {
        let mut chart = chart();
        let hover = Color::rgb(1, 2, 3);
        let line = Color::rgb(4, 5, 6);
        chart
            .add_user_price_lines_button(
                0,
                UserPriceLinesButtonOptions {
                    color: line,
                    hover_color: hover,
                },
            )
            .unwrap();
        chart.set_crosshair_at(700.0, 100.0);
        assert!(chart.build_frame().panes[0]
            .main
            .iter()
            .all(|primitive| !matches!(primitive, Prim::RoundRect { fill, .. } if fill == &hover)));

        chart.set_crosshair_at(790.0, 100.0);
        let frame = chart.build_frame();
        assert!(frame.panes[0]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::RoundRect { fill, .. } if fill == &hover)));
        assert!(frame.panes[0]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::Circle { .. })));

        assert!(chart.click_native_primitives_at(790.0, 100.0));
        assert_eq!(chart.series[0].price_lines.len(), 1);
        assert_eq!(chart.series[0].price_lines[0].color, line);
        assert_eq!(chart.series[0].price_lines[0].style, LineStyle::Dashed);
    }

    #[test]
    fn user_price_alerts_own_sorted_state_top_geometry_axis_label_and_clicks() {
        let mut chart = chart();
        let options = UserPriceAlertsOptions {
            symbol_name: "AAPL".into(),
            ..UserPriceAlertsOptions::default()
        };
        let primitive = chart.add_user_price_alerts(0, options.clone()).unwrap();
        chart.add_user_price_alert(primitive, 106.0).unwrap();
        chart.add_user_price_alert(primitive, 103.0).unwrap();
        assert_eq!(
            chart
                .user_price_alerts(primitive)
                .unwrap()
                .iter()
                .map(|alert| alert.price)
                .collect::<Vec<_>>(),
            vec![106.0, 103.0]
        );

        let _ = chart.build_frame();
        let y = chart.series_price_to_coordinate(0, 104.0).unwrap();
        chart.set_crosshair_at(790.0, y);
        assert_eq!(
            chart.user_price_alerts_hit_test(primitive, 790.0, y),
            Some(UserPriceAlertsHit::Add)
        );
        assert!(chart.click_user_price_alerts(primitive, 790.0, y));
        assert_eq!(chart.user_price_alerts(primitive).unwrap().len(), 3);

        let frame = chart.build_frame();
        assert!(frame.panes[0].main.iter().any(|primitive| matches!(
            primitive,
            Prim::HLine { color, .. } if *color == options.color
        )));
        assert!(frame.panes[0].main.iter().any(|primitive| matches!(
            primitive,
            Prim::RoundRect { w, fill, .. }
                if (*w - 21.0).abs() < f32::EPSILON && *fill == options.hover_color
        )));
        let axis = chart.build_axis_frame(80.0, |text| text.len() as f64 * 7.0);
        assert!(axis.labels.iter().any(|label| {
            label
                .background
                .is_some_and(|(_, _, _, height, color)| height == 21.0 && color == options.color)
        }));

        let alert = chart.user_price_alerts(primitive).unwrap()[0];
        let alert_y = chart.series_price_to_coordinate(0, alert.price).unwrap();
        let text = format!(
            "{} crossing {}",
            options.symbol_name,
            chart.series_format_price(0, alert.price).unwrap()
        );
        let label_width = 18.0 + 26.0 + text.chars().count() as f64 * 5.81;
        let remove_x = (chart.pane_w - label_width) * 0.5 + label_width - 13.0;
        chart.set_crosshair_at(remove_x, alert_y);
        assert_eq!(
            chart.user_price_alerts_hit_test(primitive, remove_x, alert_y),
            Some(UserPriceAlertsHit::Remove(alert.id))
        );
        assert!(chart.click_user_price_alerts(primitive, remove_x, alert_y));
        assert_eq!(chart.user_price_alerts(primitive).unwrap().len(), 2);
    }

    #[test]
    fn trend_line_emits_endpoint_labels_and_contributes_visible_autoscale() {
        let mut chart = chart();
        chart
            .add_trend_line(
                0,
                2 * 86_400,
                50.0,
                6 * 86_400,
                200.0,
                TrendLineOptions::default(),
            )
            .unwrap();
        let frame = chart.build_frame();
        assert!(frame.panes[0].main.iter().any(|primitive| {
            matches!(primitive, Prim::Polyline { width, point_count: 2, .. } if *width == 6.0)
        }));
        assert!(frame.panes[0]
            .main
            .iter()
            .any(|primitive| { matches!(primitive, Prim::Text { text, .. } if text == "50.0") }));
        assert!(frame.panes[0]
            .main
            .iter()
            .any(|primitive| { matches!(primitive, Prim::Text { text, .. } if text == "200.0") }));
        let range = chart.panes[0].price_scale.price_range().unwrap();
        assert!(range.min_value() <= 50.0);
        assert!(range.max_value() >= 200.0);
    }

    #[test]
    fn text_watermark_zoom_alignment_and_visibility_are_engine_owned() {
        let mut chart = chart();
        chart.set_text_measure(Some(Box::new(
            |text, _, _, _, _| {
                if text == "Wide" {
                    1_600.0
                } else {
                    0.0
                }
            },
        )));
        let options = TextWatermarkOptions {
            visible: true,
            horizontal_align: AnchoredTextHorizontalAlign::Middle,
            vertical_align: AnchoredTextVerticalAlign::Middle,
            lines: vec![TextWatermarkLine {
                text: "Wide".into(),
                color: Color::rgba(10, 20, 30, 128),
                font_size: 48.0,
                font_family: "Arial".into(),
                font_weight: 700,
                italic: false,
                line_height: 57.6,
            }],
        };
        let id = chart.add_text_watermark(0, options.clone()).unwrap();
        let frame = chart.build_frame();
        let text = frame.panes[0]
            .main
            .iter()
            .find_map(|primitive| match primitive {
                Prim::Text {
                    x,
                    y,
                    text,
                    size,
                    align,
                    ..
                } if text == "Wide" => Some((*x, *y, *size, *align)),
                _ => None,
            })
            .unwrap();
        assert_eq!(text.0, 400.0);
        assert!((text.1 - 247.6).abs() < 0.001);
        assert_eq!(text.2, 24.0);
        assert_eq!(text.3, nucleuscharts_render::draw_list::TextAlign::Center);

        assert!(chart.set_text_watermark_options(
            id,
            TextWatermarkOptions {
                visible: false,
                ..options
            },
        ));
        assert!(chart.build_frame().panes[0]
            .main
            .iter()
            .all(|primitive| !matches!(primitive, Prim::Text { text, .. } if text == "Wide")));
    }
}
