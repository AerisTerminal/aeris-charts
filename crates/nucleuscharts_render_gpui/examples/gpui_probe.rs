//! Official-GPUI integration demo and finite probe for Nucleus's complete engine frame.
//!
//! With `NUCLEUSCHARTS_PROBE_FRAMES` unset this is a real native application shell: grouped controls,
//! independently seeded `Workspace` split cells, drawing creation, indicators, exact package
//! themes, OHLC/click status, and host-side visual approximations of the web plugin fixtures. Those fixture
//! toggles insert engine `Prim`s or use native engine APIs; they are explicitly not a JavaScript
//! object bridge. With `NUCLEUSCHARTS_PROBE_FRAMES` set the source single-chart metrics/probe path is
//! retained: responsive layout, native text measurement, axis/crosshair chrome, pan/zoom/scale,
//! pane separators, drawing selection, fractional DPR, and live updates.
//!
//! ```text
//! cargo run -p nucleuscharts_render_gpui --features gpui-backend --example gpui_probe
//! ```
//!
//! Environment knobs:
//! - `NUCLEUSCHARTS_PROBE_BARS` — synthetic bars to load (default 500).
//! - `NUCLEUSCHARTS_PROBE_FRAMES` — quit after N painted frames and print a metrics summary. Unset runs
//!   interactively until the window closes.

use std::{
    collections::HashMap,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use gpui::{
    canvas, div, prelude::*, px, relative, rgb, size, AnyElement, App, Bounds, Context,
    CursorStyle, Entity, FocusHandle, Focusable, KeyDownEvent, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent, Render, ScrollDelta,
    ScrollWheelEvent, Subscription, Window, WindowBounds, WindowOptions,
};
use gpui_platform::application;
use nucleuscharts_core::model::data_layer::SeriesId;
use nucleuscharts_engine::{
    crosshair_mode_from_u8, marker_pos, marker_shape, ChartEngine, ChartFrame, DrawingKind,
    DrawingModifiers, DrawingPoint, GestureResolver, InputDevice, InputModifiers, InputTarget,
    Marker, PointerSample, PriceScaleTarget, PrimitiveAutoscaleContribution, SeriesKind,
    SplitDirection, WheelBehavior, WheelDeltaMode, WheelIntent, WheelSample, Workspace,
    WorkspaceLayout,
};
use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::{IRect, LineStyle, Prim, TextAlign};
use nucleuscharts_render_gpui::{
    backend::measure_text, GpuiChartRenderer, GpuiFrameMetrics, NucleusViewport,
    PreparedNucleusFrame,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DemoTheme {
    Light,
    Dark,
}

impl DemoTheme {
    fn surface(self) -> &'static str {
        match self {
            Self::Light => nucleuscharts_core::style::LIGHT_SURFACE_CSS,
            Self::Dark => nucleuscharts_core::style::DARK_SURFACE_CSS,
        }
    }

    fn crosshair(self) -> &'static str {
        match self {
            Self::Light => nucleuscharts_core::style::LIGHT_CROSSHAIR_CSS,
            Self::Dark => nucleuscharts_core::style::DARK_CROSSHAIR_CSS,
        }
    }

    fn patch(self) -> String {
        let surface = self.surface();
        let border = theme_border(self);
        let text = theme_text(self);
        let crosshair = self.crosshair();
        format!(
            r#"{{"layout":{{"background":{{"type":"solid","color":"{surface}"}},"textColor":"{text}","panes":{{"separatorColor":"{border}"}}}},"leftPriceScale":{{"borderColor":"{border}"}},"rightPriceScale":{{"borderColor":"{border}"}},"timeScale":{{"borderColor":"{border}"}},"grid":{{"vertLines":{{"color":"{border}"}},"horzLines":{{"color":"{border}"}}}},"crosshair":{{"vertLine":{{"color":"{crosshair}","labelBackgroundColor":"{crosshair}"}},"horzLine":{{"color":"{crosshair}","labelBackgroundColor":"{crosshair}"}}}}}}"#
        )
    }
}

fn apply_package_theme(engine: &mut ChartEngine, theme: DemoTheme) {
    let patch = theme.patch();
    engine
        .options
        .apply_str(&patch)
        .expect("built-in GPUI package theme is valid JSON");
}

#[derive(Clone, Debug, Default)]
struct StylePins {
    grid_color: Option<String>,
    axis_border_color: Option<String>,
    text_color: Option<String>,
    separator_color: Option<String>,
}

impl StylePins {
    fn apply(&self, engine: &mut ChartEngine) {
        if let Some(color) = &self.grid_color {
            engine
                .options
                .apply_str(&format!(
                    r#"{{"grid":{{"vertLines":{{"color":"{color}"}},"horzLines":{{"color":"{color}"}}}}}}"#
                ))
                .expect("pinned grid color is valid");
        }
        if let Some(color) = &self.axis_border_color {
            engine
                .options
                .apply_str(&format!(
                    r#"{{"leftPriceScale":{{"borderColor":"{color}"}},"rightPriceScale":{{"borderColor":"{color}"}},"timeScale":{{"borderColor":"{color}"}}}}"#
                ))
                .expect("pinned axis color is valid");
        }
        if let Some(color) = &self.text_color {
            engine
                .options
                .apply_str(&format!(r#"{{"layout":{{"textColor":"{color}"}}}}"#))
                .expect("pinned text color is valid");
        }
        if let Some(color) = &self.separator_color {
            engine
                .options
                .apply_str(&format!(
                    r#"{{"layout":{{"panes":{{"separatorColor":"{color}"}}}}}}"#
                ))
                .expect("pinned separator color is valid");
        }
    }
}

fn theme_border(theme: DemoTheme) -> &'static str {
    match theme {
        DemoTheme::Light => nucleuscharts_core::style::LIGHT_BORDER_CSS,
        DemoTheme::Dark => nucleuscharts_core::style::DARK_BORDER_CSS,
    }
}

fn theme_text(theme: DemoTheme) -> &'static str {
    match theme {
        DemoTheme::Light => nucleuscharts_core::style::LIGHT_AXIS_TEXT_CSS,
        DemoTheme::Dark => nucleuscharts_core::style::DARK_AXIS_TEXT_CSS,
    }
}

fn shell_rgb(css: &str, fallback: u32) -> u32 {
    let hex = css.strip_prefix('#').unwrap_or(css);
    if hex.len() != 6 {
        return fallback;
    }
    u32::from_str_radix(hex, 16).unwrap_or(fallback)
}

/// Stable, testable manifest for the native toolbar. Controls are intentionally compact cycle
/// buttons rather than HTML inputs; every item maps to an engine or host-side native action.
const TOOLBAR_FEATURE_MANIFEST: &[&str] = &[
    "series:candlestick,bar,line,area,baseline",
    "style:candle-body,wick-colors,border-colors,wick-visible,border-visible,reset-parts,line-color,line-width,area-fill",
    "overlay:sma20,volume,rsi14",
    "workspace:split-horizontal,split-vertical,shortcuts,maximize,restore,close,cap,usage,active,resize",
    "drawing:trend,h-line,h-ray,v-line,rect,text,brush,clear,color,style,width,label,text-color,size,weight,italic",
    "crosshair:mode,color,width,style,label-background,labels",
    "chart:theme,grid,grid-color,grid-style,font-family,font-size",
    "series-chrome:price-line,style,last-value,title-visible,title-text,countdown,bid-ask",
    "axes:border-visible,border-color,text-color,separator",
    "watermark:visible,text,color,size",
    "interaction:axis-scaling,mouse-kinetic,reset-view",
    "native-visual-approximations:day-bands,position-band,autoscale-band,rounded-candles,markers,plugin-watermark,vertical-line",
];

/// Column-major OHLC, in the shape `ChartEngine::set_series_data` takes.
#[derive(Clone)]
struct Bars {
    times: Vec<f64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
}

/// Deterministic synthetic OHLC: no RNG, no clock, so successive runs are comparable.
fn synthetic_bars(count: usize) -> Bars {
    let mut times = Vec::with_capacity(count);
    let mut open = Vec::with_capacity(count);
    let mut high = Vec::with_capacity(count);
    let mut low = Vec::with_capacity(count);
    let mut close = Vec::with_capacity(count);
    let mut price = 100.0f64;
    for i in 0..count {
        let t = i as f64;
        let c = 100.0 + (t * 0.11).sin() * 6.0 + (t * 0.031).cos() * 14.0;
        let o = price;
        price = c;
        times.push(1_600_000_000.0 + t * 60.0);
        open.push(o);
        high.push(o.max(c) + 1.5 + (t * 0.7).sin().abs());
        low.push(o.min(c) - 1.5 - (t * 0.9).cos().abs());
        close.push(c);
    }
    Bars {
        times,
        open,
        high,
        low,
        close,
    }
}

fn next_bar_timestamp(times: &[f64]) -> f64 {
    let Some(&latest) = times.last() else {
        return 1_600_000_000.0;
    };
    let cadence = times
        .windows(2)
        .rev()
        .map(|pair| pair[1] - pair[0])
        .find(|cadence| cadence.is_finite() && *cadence > 0.0)
        .unwrap_or(60.0);
    latest + cadence
}

/// Match the Web demo's root-cell fixture: 1,000 deterministic hourly bars by default.
fn interactive_root_bars(count: usize) -> Bars {
    let end_time = 1_600_000_000.0 + count.saturating_sub(1) as f64 * 3_600.0;
    web_demo_bars(count, 42, 100.0, 2.4, 1.2, end_time)
}

/// Match the Web demo's split-cell contract: every new cell gets 300 deterministic hourly bars
/// for a distinct synthetic asset, aligned to the root asset's final timestamp.
fn split_asset_bars(sequence: usize, end_time: f64) -> Bars {
    web_demo_bars(
        300,
        42_u32.wrapping_add((sequence as u32).wrapping_mul(977)),
        40.0 + sequence as f64 * 25.0,
        2.2,
        0.9,
        end_time,
    )
}

fn web_demo_bars(
    count: usize,
    mut seed: u32,
    start_price: f64,
    close_span: f64,
    wick_span: f64,
    end_time: f64,
) -> Bars {
    let mut times = Vec::with_capacity(count);
    let mut open = Vec::with_capacity(count);
    let mut high = Vec::with_capacity(count);
    let mut low = Vec::with_capacity(count);
    let mut close = Vec::with_capacity(count);
    let mut random = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f64::from(seed) / f64::from(u32::MAX)
    };
    let mut price = start_price;
    let start = end_time - count.saturating_sub(1) as f64 * 3_600.0;
    for i in 0..count {
        let o = price;
        let c = (o + (random() - 0.5) * close_span).max(1.0);
        times.push(start + i as f64 * 3_600.0);
        open.push(o);
        high.push(o.max(c) + random() * wick_span);
        low.push(o.min(c) - random() * wick_span);
        close.push(c);
        price = c;
    }
    Bars {
        times,
        open,
        high,
        low,
        close,
    }
}

const CLICK_SLOP_MANHATTAN: f64 = 5.0;
const PANE_SEPARATOR_HIT: f64 = 4.0;
const WHEEL_LINE_HEIGHT: f32 = 32.0;

/// The browser host's resolved desktop defaults. Keeping them explicit prevents a demo-only
/// behavior from silently diverging from `packages/charts/src/impl.ts`.
#[derive(Clone, Copy, Debug)]
struct GestureConfig {
    pan: bool,
    wheel_scroll: bool,
    wheel_zoom: bool,
    wheel_behavior: WheelBehavior,
    axis_dblclick_reset_time: bool,
    axis_dblclick_reset_price: bool,
    axis_scale_price: bool,
    axis_scale_time: bool,
    kinetic_mouse: bool,
    panes_resize: bool,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            pan: true,
            wheel_scroll: true,
            wheel_zoom: true,
            wheel_behavior: WheelBehavior::Auto,
            axis_dblclick_reset_time: true,
            axis_dblclick_reset_price: true,
            axis_scale_price: true,
            axis_scale_time: true,
            kinetic_mouse: false,
            panes_resize: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum DragMode {
    Pan {
        price_pan: Option<(usize, PriceScaleTarget)>,
    },
    TimeAxis,
    PriceAxis {
        pane: usize,
        target: PriceScaleTarget,
    },
    PaneSeparator {
        index: usize,
        last_y: f64,
    },
    Drawing,
    BrushCreation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DrawingTemplate {
    color: String,
    style: &'static str,
    width: u8,
    text: String,
    text_color: String,
    text_size: u8,
    text_weight: u16,
    text_italic: bool,
}

impl Default for DrawingTemplate {
    fn default() -> Self {
        Self {
            color: "#2962ff".into(),
            style: "solid",
            width: 2,
            text: "Native".into(),
            text_color: "#0a0a0a".into(),
            text_size: 12,
            text_weight: 400,
            text_italic: false,
        }
    }
}

impl DrawingTemplate {
    fn json(&self) -> String {
        format!(
            r#"{{"color":"{}","style":"{}","width":{},"text":"{}","text_color":"{}","text_size":{},"text_weight":{},"text_italic":{}}}"#,
            self.color,
            self.style,
            self.width,
            self.text,
            self.text_color,
            self.text_size,
            self.text_weight,
            self.text_italic
        )
    }

    fn patch_from(&self, previous: &Self) -> String {
        let mut fields = Vec::new();
        if self.color != previous.color {
            fields.push(format!(r#""color":"{}""#, self.color));
        }
        if self.style != previous.style {
            fields.push(format!(r#""style":"{}""#, self.style));
        }
        if self.width != previous.width {
            fields.push(format!(r#""width":{}"#, self.width));
        }
        if self.text != previous.text {
            fields.push(format!(r#""text":"{}""#, self.text));
        }
        if self.text_color != previous.text_color {
            fields.push(format!(r#""text_color":"{}""#, self.text_color));
        }
        if self.text_size != previous.text_size {
            fields.push(format!(r#""text_size":{}"#, self.text_size));
        }
        if self.text_weight != previous.text_weight {
            fields.push(format!(r#""text_weight":{}"#, self.text_weight));
        }
        if self.text_italic != previous.text_italic {
            fields.push(format!(r#""text_italic":{}"#, self.text_italic));
        }
        format!("{{{}}}", fields.join(","))
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct NativeFixtures {
    day_bands: bool,
    position_band: bool,
    autoscale_band: bool,
    rounded_candles: bool,
    markers: bool,
    plugin_watermark: bool,
    vertical_line: bool,
}

/// The probe's chart state: an engine, the adapter, and the last built frame.
struct Probe {
    engine: ChartEngine,
    renderer: GpuiChartRenderer,
    frame: ChartFrame,
    /// Real engine-produced watermark/axis/crosshair top layer.
    axis: Vec<Prim>,
    focus_handle: Option<FocusHandle>,
    /// Size and DPR the frame was last built for, so resize and scale changes are detected.
    built_for: (f32, f32, f32),
    dirty: bool,
    plan_dirty: bool,
    fitted: bool,
    viewport_offset: (f32, f32),
    gesture_config: GestureConfig,
    input: GestureResolver,
    input_target: InputTarget,
    cursor_style: CursorStyle,
    press_start: Option<(f64, f64)>,
    press_moved: bool,
    drag: Option<DragMode>,
    kinetic_active: bool,
    source_bars: Bars,
    armed_tool: Option<DrawingKind>,
    drawing_template: DrawingTemplate,
    style_pins: StylePins,
    fixtures: NativeFixtures,
    sma_id: Option<SeriesId>,
    volume_id: Option<SeriesId>,
    rsi_id: Option<SeriesId>,
    legend: String,
    click_status: String,
    bars: usize,
    appended: usize,
    last_append_epoch: u64,
    painted: u64,
    frame_budget: Option<u64>,
    plan_nanos: Vec<u64>,
    paint_nanos: Vec<u64>,
    total_nanos: Vec<u64>,
    last: GpuiFrameMetrics,
    /// Distinct scale factors and sizes observed, to prove the propagation actually happened.
    seen_scales: Vec<f32>,
    seen_sizes: Vec<(f32, f32)>,
    started: Instant,
    reported: bool,
}

impl Probe {
    fn new(bars: usize, frame_budget: Option<u64>) -> Self {
        let mut engine = ChartEngine::new(1024.0, 640.0, 1.0);
        apply_package_theme(&mut engine, DemoTheme::Light);
        let b = synthetic_bars(bars);
        engine
            .set_series_data(0, &b.times, &b.open, &b.high, &b.low, &b.close)
            .expect("synthetic series is well formed");
        engine.series[0].kind = SeriesKind::Candlestick;
        engine.create_price_line(
            0,
            108.0,
            Color::rgb(0x29, 0xb6, 0xf6),
            2,
            LineStyle::Dashed,
            "LI",
        );
        let last_logical = bars.saturating_sub(1).max(1) as f64;
        engine
            .add_drawing(
                DrawingKind::TrendLine,
                0,
                vec![
                    DrawingPoint {
                        logical: last_logical * 0.28,
                        price: 96.0,
                    },
                    DrawingPoint {
                        logical: last_logical * 0.72,
                        price: 112.0,
                    },
                ],
                Some(r##"{"color":"#ffb300","line_width":2}"##),
            )
            .expect("the deterministic trend-line fixture is valid");
        Self {
            engine,
            renderer: GpuiChartRenderer::new(),
            frame: ChartFrame::default(),
            axis: Vec::new(),
            focus_handle: None,
            built_for: (0.0, 0.0, 0.0),
            dirty: true,
            plan_dirty: true,
            fitted: false,
            viewport_offset: (0.0, 0.0),
            gesture_config: GestureConfig::default(),
            input: GestureResolver::default(),
            input_target: InputTarget::Pane,
            cursor_style: CursorStyle::Crosshair,
            press_start: None,
            press_moved: false,
            drag: None,
            kinetic_active: false,
            source_bars: b,
            armed_tool: None,
            drawing_template: DrawingTemplate::default(),
            style_pins: StylePins::default(),
            fixtures: NativeFixtures::default(),
            sma_id: None,
            volume_id: None,
            rsi_id: None,
            legend: "O —  H —  L —  C —".to_string(),
            click_status: "ready".to_string(),
            bars,
            appended: 0,
            last_append_epoch: 0,
            painted: 0,
            frame_budget,
            plan_nanos: Vec::new(),
            paint_nanos: Vec::new(),
            total_nanos: Vec::new(),
            last: GpuiFrameMetrics::default(),
            seen_scales: Vec::new(),
            seen_sizes: Vec::new(),
            started: Instant::now(),
            reported: false,
        }
    }

    fn new_interactive(bars: usize) -> Self {
        let mut probe = Self::new(bars, None);
        probe.engine.series[0].price_lines.clear();
        probe.engine.clear_drawings();
        probe.replace_source_bars(interactive_root_bars(bars));
        probe
    }

    fn replace_source_bars(&mut self, bars: Bars) {
        self.engine
            .set_series_data(
                0,
                &bars.times,
                &bars.open,
                &bars.high,
                &bars.low,
                &bars.close,
            )
            .expect("split-cell synthetic series is well formed");
        self.bars = bars.times.len();
        self.source_bars = bars;
        self.appended = 0;
        self.last_append_epoch = 0;
        self.fitted = false;
        self.dirty = true;
    }

    fn apply_theme(&mut self, theme: DemoTheme) {
        apply_package_theme(&mut self.engine, theme);
        self.style_pins.apply(&mut self.engine);
        self.renderer.invalidate_caches();
        self.dirty = true;
    }

    fn arm_drawing(&mut self, kind: DrawingKind) {
        self.engine.drawing_create_cancel();
        self.engine.brush_create_cancel();
        self.armed_tool = (self.armed_tool != Some(kind)).then_some(kind);
        if let Some(tool) = self.armed_tool.filter(|tool| *tool != DrawingKind::Brush) {
            let template = self.drawing_template.json();
            self.engine.drawing_create_begin(tool, Some(&template));
        }
        self.click_status = self.armed_tool.map_or_else(
            || "drawing tool disarmed".to_string(),
            |tool| format!("{} armed", tool.name()),
        );
        self.dirty = true;
    }

    fn update_drawing_template(&mut self, mutate: impl FnOnce(&mut DrawingTemplate)) {
        let previous = self.drawing_template.clone();
        mutate(&mut self.drawing_template);
        let patch = self.drawing_template.patch_from(&previous);
        if patch == "{}" {
            return;
        }
        // Merge only changed fields into both an in-progress creation and a selected drawing.
        // This preserves placed anchors and unrelated selected-drawing options.
        self.engine.drawing_create_apply_options(&patch);
        if let Some(id) = self.engine.selected_drawing() {
            self.engine.drawing_apply_options(id, &patch);
        }
        self.dirty = true;
    }

    fn toggle_grid_color_pin(&mut self, theme: DemoTheme) {
        self.style_pins.grid_color = self
            .style_pins
            .grid_color
            .is_none()
            .then(|| "#2962ff".to_string());
        let color = self
            .style_pins
            .grid_color
            .as_deref()
            .unwrap_or_else(|| theme_border(theme));
        self.engine
            .options
            .apply_str(&format!(
                r#"{{"grid":{{"vertLines":{{"color":"{color}"}},"horzLines":{{"color":"{color}"}}}}}}"#
            ))
            .expect("grid color toggle is valid");
        self.dirty = true;
    }

    fn toggle_axis_border_pin(&mut self, theme: DemoTheme) {
        self.style_pins.axis_border_color = self
            .style_pins
            .axis_border_color
            .is_none()
            .then(|| "#2962ff".to_string());
        let color = self
            .style_pins
            .axis_border_color
            .as_deref()
            .unwrap_or_else(|| theme_border(theme));
        self.engine
            .options
            .apply_str(&format!(
                r#"{{"leftPriceScale":{{"borderColor":"{color}"}},"rightPriceScale":{{"borderColor":"{color}"}},"timeScale":{{"borderColor":"{color}"}}}}"#
            ))
            .expect("axis color toggle is valid");
        self.dirty = true;
    }

    fn toggle_text_color_pin(&mut self, theme: DemoTheme) {
        self.style_pins.text_color = self
            .style_pins
            .text_color
            .is_none()
            .then(|| "#ab47bc".to_string());
        let color = self
            .style_pins
            .text_color
            .as_deref()
            .unwrap_or_else(|| theme_text(theme));
        self.engine
            .options
            .apply_str(&format!(r#"{{"layout":{{"textColor":"{color}"}}}}"#))
            .expect("text color toggle is valid");
        self.dirty = true;
    }

    fn toggle_separator_pin(&mut self, theme: DemoTheme) {
        self.style_pins.separator_color = self
            .style_pins
            .separator_color
            .is_none()
            .then(|| "#ff9800".to_string());
        let color = self
            .style_pins
            .separator_color
            .as_deref()
            .unwrap_or_else(|| theme_border(theme));
        self.engine
            .options
            .apply_str(&format!(
                r#"{{"layout":{{"panes":{{"separatorColor":"{color}"}}}}}}"#
            ))
            .expect("separator color toggle is valid");
        self.dirty = true;
    }

    fn set_series_kind(&mut self, kind: SeriesKind) {
        self.engine.convert_series_kind(0, kind);
        self.engine.series[0].kind = kind;
        self.click_status = format!("series: {kind:?}");
        self.dirty = true;
    }

    fn toggle_sma(&mut self) {
        if let Some(id) = self.sma_id.take() {
            self.engine.remove_series(id);
        } else {
            self.sma_id = self.engine.add_sma(0, 20);
            if let Some(id) = self.sma_id {
                if let Some(series) = self
                    .engine
                    .series
                    .iter_mut()
                    .find(|series| series.id == id && !series.removed)
                {
                    series.line_color = Some("#ff9800".into());
                    series.line_width = Some(2.0);
                }
            }
        }
        self.dirty = true;
    }

    fn toggle_volume(&mut self) {
        if let Some(id) = self.volume_id.take() {
            self.engine.remove_series(id);
        } else {
            let id = self.engine.add_series(SeriesKind::Histogram);
            let volume = self
                .source_bars
                .high
                .iter()
                .zip(&self.source_bars.low)
                .enumerate()
                .map(|(i, (h, l))| (h - l) * 25_000.0 + i as f64 * 31.0)
                .collect::<Vec<_>>();
            self.engine
                .set_series_data(
                    id,
                    &self.source_bars.times,
                    &volume,
                    &volume,
                    &volume,
                    &volume,
                )
                .expect("native volume fixture is aligned");
            self.engine
                .set_series_price_scale(id, PriceScaleTarget::Overlay);
            let s = self
                .engine
                .series
                .iter_mut()
                .find(|series| series.id == id && !series.removed)
                .expect("new volume series is live");
            s.histogram_updown = true;
            s.price_line_visible = false;
            s.title = "Volume".into();
            self.volume_id = Some(id);
        }
        self.dirty = true;
    }

    fn toggle_rsi(&mut self) {
        if let Some(id) = self.rsi_id.take() {
            self.engine.remove_series(id);
        } else {
            self.rsi_id = self.engine.add_rsi(0, 14);
            if let Some(id) = self.rsi_id {
                if let Some(series) = self
                    .engine
                    .series
                    .iter_mut()
                    .find(|series| series.id == id && !series.removed)
                {
                    series.line_color = Some("#ab47bc".into());
                    series.line_width = Some(2.0);
                }
            }
        }
        self.dirty = true;
    }

    fn toggle_markers(&mut self) {
        self.fixtures.markers = !self.fixtures.markers;
        let markers = if self.fixtures.markers && !self.source_bars.times.is_empty() {
            let n = self.source_bars.times.len();
            vec![
                Marker {
                    time: self.source_bars.times[n / 3] as i64,
                    position: marker_pos::ABOVE,
                    shape: marker_shape::ARROW_DOWN,
                    color: Color::rgb(0xef, 0x53, 0x50),
                    text: "Native A".into(),
                    id: "native-a".into(),
                    size: 1.0,
                    price: None,
                },
                Marker {
                    time: self.source_bars.times[n * 2 / 3] as i64,
                    position: marker_pos::BELOW,
                    shape: marker_shape::ARROW_UP,
                    color: Color::rgb(0x26, 0xa6, 0x9a),
                    text: "Native B".into(),
                    id: "native-b".into(),
                    size: 1.0,
                    price: None,
                },
            ]
        } else {
            Vec::new()
        };
        self.engine.set_series_markers(0, markers);
        self.dirty = true;
    }

    fn update_legend(&mut self, pane_x: f64) {
        if self.source_bars.close.is_empty() {
            return;
        }
        let logical = (self.engine.time_scale.coordinate_to_float_index(pane_x) + 0.5).round();
        let i = logical.clamp(0.0, (self.source_bars.close.len() - 1) as f64) as usize;
        self.legend = format!(
            "O {:.2}  H {:.2}  L {:.2}  C {:.2}",
            self.source_bars.open[i],
            self.source_bars.high[i],
            self.source_bars.low[i],
            self.source_bars.close[i]
        );
    }

    fn inject_native_equivalents(&mut self) {
        let Some(scissor) = self.frame.panes.first().map(|pane| pane.scissor) else {
            return;
        };
        let [sx, sy, sw, sh] = scissor;
        let (sx, sy, sw, sh) = (sx as i32, sy as i32, sw as i32, sh as i32);
        let dpr = self.engine.dpr;
        let device_x = |logical: f64| {
            self.engine
                .logical_to_coordinate(logical)
                .map(|x| ((self.engine.pane_left + x) * dpr).round() as i32)
        };
        let device_y = |price: f64| {
            self.engine
                .series_price_to_coordinate(0, price)
                .map(|y| (y * dpr).round() as i32)
        };
        let mut under = Vec::new();
        let mut top = Vec::new();
        let count = self.source_bars.close.len();

        if self.fixtures.day_bands {
            // Native equivalent: deterministic session blocks anchored to logical ranges, so they
            // pan/zoom with the data rather than decorating the viewport.
            for start in (0..count).step_by(50).step_by(2) {
                if let (Some(x0), Some(x1)) = (
                    device_x(start as f64 - 0.5),
                    device_x((start + 50).min(count) as f64 - 0.5),
                ) {
                    under.push(Prim::Rect {
                        rect: IRect {
                            x: x0,
                            y: sy,
                            w: (x1 - x0).max(1),
                            h: sh,
                        },
                        color: Color::rgba(0x29, 0x62, 0xff, 20),
                    });
                }
            }
        }
        if self.fixtures.position_band && count > 0 {
            let entry = self.source_bars.close[count / 2];
            if let (Some(y0), Some(y1)) = (device_y(entry * 1.02), device_y(entry * 0.98)) {
                let (top_y, bottom_y) = (y0.min(y1), y0.max(y1));
                top.push(Prim::Rect {
                    rect: IRect {
                        x: sx,
                        y: top_y,
                        w: sw,
                        h: (bottom_y - top_y).max(1),
                    },
                    color: Color::rgba(0xff, 0x98, 0x00, 38),
                });
                for y in [top_y, bottom_y] {
                    top.push(Prim::HLine {
                        y,
                        x0: sx,
                        x1: sx + sw,
                        width: 2,
                        style: LineStyle::Dashed,
                        color: Color::rgb(0xff, 0x98, 0x00),
                    });
                }
            }
        }
        if self.fixtures.autoscale_band && count > 0 {
            let low = self
                .source_bars
                .low
                .iter()
                .copied()
                .fold(f64::INFINITY, f64::min)
                - 10.0;
            let high = self
                .source_bars
                .high
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max)
                + 10.0;
            for y in [device_y(low), device_y(high)].into_iter().flatten() {
                top.push(Prim::HLine {
                    y,
                    x0: sx,
                    x1: sx + sw,
                    width: 2,
                    style: LineStyle::Dashed,
                    color: Color::rgb(0x9c, 0x27, 0xb0),
                });
            }
        }
        if self.fixtures.rounded_candles && count > 0 {
            let step = (count / 16).max(1);
            let body_w = (self.engine.bar_spacing() * dpr * 0.65).max(3.0) as f32;
            for i in (0..count).step_by(step) {
                let Some(cx) = device_x(i as f64) else {
                    continue;
                };
                let (Some(open), Some(close)) = (
                    device_y(self.source_bars.open[i]),
                    device_y(self.source_bars.close[i]),
                ) else {
                    continue;
                };
                let y = open.min(close);
                top.push(Prim::RoundRect {
                    x: cx as f32 - body_w / 2.0,
                    y: y as f32,
                    w: body_w,
                    h: (open - close).abs().max(2) as f32,
                    radii: [2.0 * dpr as f32; 4],
                    fill: if self.source_bars.close[i] >= self.source_bars.open[i] {
                        Color::rgba(0x26, 0xa6, 0x9a, 210)
                    } else {
                        Color::rgba(0xef, 0x53, 0x50, 210)
                    },
                    border_width: 1.0,
                    border_color: Color::rgba(0xff, 0xff, 0xff, 100),
                });
            }
        }
        if self.fixtures.vertical_line && count > 0 {
            if let Some(x) = device_x((count / 2) as f64) {
                top.push(Prim::VLine {
                    x,
                    y0: sy,
                    y1: sy + sh,
                    width: 3,
                    style: LineStyle::Solid,
                    color: Color::rgb(0xe9, 0x1e, 0x63),
                });
            }
        }
        if self.fixtures.plugin_watermark {
            top.push(Prim::Text {
                x: (sx + sw / 2) as f32,
                y: (sy + sh / 2) as f32,
                text: "NUCLEUS · native plugin equivalent".into(),
                color: Color::rgba(0x29, 0x62, 0xff, 70),
                size: 28.0 * dpr as f32,
                family: "monospace".into(),
                align: TextAlign::Center,
                weight: 700,
                italic: false,
            });
        }
        if let Some(pane) = self.frame.panes.first_mut() {
            pane.under.extend(under);
            pane.top_prims.extend(top);
        }
    }

    /// Rebuild the complete frame for `(width, height)` logical px at `scale_factor` using a
    /// host-native text width callback.
    fn rebuild_with_measure<F>(&mut self, width: f32, height: f32, scale_factor: f32, measure: F)
    where
        F: Fn(&str) -> f64,
    {
        if !self.seen_scales.contains(&scale_factor) {
            self.seen_scales.push(scale_factor);
        }
        if !self.seen_sizes.contains(&(width, height)) {
            self.seen_sizes.push((width, height));
        }
        let key = (width, height, scale_factor);
        let resized = self.built_for != key;
        if !resized && !self.dirty && !self.frame.panes.is_empty() {
            return;
        }
        if self.built_for.2 != 0.0 && self.built_for.2 != scale_factor {
            self.renderer.invalidate_caches();
        }
        self.built_for = key;
        self.dirty = false;
        self.engine.css_width = f64::from(width);
        self.engine.css_height = f64::from(height);
        self.engine.dpr = f64::from(scale_factor);

        self.engine.clear_autoscale_contributions();
        if self.frame_budget.is_none() && self.engine.series[0].countdown_visible {
            if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
                self.engine.set_now_seconds(now.as_secs_f64());
            }
        }
        if self.fixtures.autoscale_band && !self.source_bars.low.is_empty() {
            let min = self
                .source_bars
                .low
                .iter()
                .copied()
                .fold(f64::INFINITY, f64::min)
                - 10.0;
            let max = self
                .source_bars
                .high
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max)
                + 10.0;
            self.engine
                .add_autoscale_contribution(PrimitiveAutoscaleContribution {
                    series: 0,
                    pane: 0,
                    target: PriceScaleTarget::Right,
                    min,
                    max,
                });
        }
        self.engine
            .recompute_layout_with_measure(resized, |text| measure(text));
        if !self.fitted {
            self.engine.fit_content();
            self.fitted = true;
            self.engine
                .recompute_layout_with_measure(true, |text| measure(text));
        }

        let layout = self.engine.options.get().layout.clone();
        let max_label_width = (layout.font_size + 4.0) * 5.0 / 8.0
            * f64::from(self.engine.tick_mark_max_character_length.max(1));
        let axis_frame = self
            .engine
            .build_axis_frame(max_label_width, |text| measure(text));
        self.engine.build_frame_into(&mut self.frame);
        self.inject_native_equivalents();
        // GPUI's Prim text executor already converts a vertical center into a baseline from native
        // ascent/descent. The browser needs a Canvas ink-box correction; GPUI correctly supplies 0.
        self.engine
            .build_axis_primitives_into(&axis_frame, &mut self.axis, |_| 0.0);
        self.plan_dirty = true;

        let content_h = self.engine.pane_h;
        let pane = self
            .frame
            .panes
            .first()
            .expect("the probe engine always has a primary pane");
        let expected_x_px = (self.engine.pane_left * self.engine.dpr).round() as u32;
        let expected_w_px = (self.engine.pane_w * self.engine.dpr).round() as u32;
        let expected_h_px = (content_h * self.engine.dpr).round() as u32;
        assert!(
            (self.engine.pane_left + self.engine.pane_w + self.engine.axis_w - f64::from(width))
                .abs()
                < 0.01
                && (self.frame.width - self.engine.pane_left - self.engine.pane_w).abs() < 0.01
                && (self.frame.height - content_h).abs() < 0.01,
            "probe layout must follow logical canvas bounds: canvas={width}x{height}, pane_left={}, pane={}x{}, axes={}+{}",
            self.engine.pane_left,
            self.engine.pane_w,
            self.engine.pane_h,
            self.engine.left_axis_w,
            self.engine.axis_w
        );
        assert_eq!(
            [pane.scissor[0], pane.scissor[2], pane.scissor[3]],
            [expected_x_px, expected_w_px, expected_h_px],
            "probe pane scissor must follow the physical negotiated pane extent"
        );
        println!(
            "nucleuscharts probe viewport: canvas={width:.1}x{height:.1} logical, pane=({:.1},{:.1}) {:.1}x{:.1}, axes={:.1}/{:.1}, scissor={:?} device, dpr={scale_factor:.3}",
            self.engine.pane_left,
            0.0,
            self.engine.pane_w,
            self.engine.pane_h,
            self.engine.left_axis_w,
            self.engine.axis_w,
            pane.scissor
        );
    }

    /// GPUI prepaint entry: use the exact native shaper that the paint backend uses.
    fn rebuild(&mut self, width: f32, height: f32, scale_factor: f32, window: &Window) {
        if self.built_for == (width, height, scale_factor)
            && !self.dirty
            && !self.frame.panes.is_empty()
        {
            return;
        }
        let layout = self.engine.options.get().layout.clone();
        let mut drawing_widths = HashMap::new();
        for drawing in self.engine.drawings() {
            let text = drawing.display_text();
            let size = drawing.text_size.unwrap_or(layout.font_size);
            let weight = if drawing.kind == DrawingKind::Text && drawing.text.is_empty() {
                700
            } else {
                drawing.text_weight.unwrap_or(400)
            };
            let key = format!(
                "{text}\u{0}{size}\u{0}{}\u{0}{weight}\u{0}{}",
                layout.font_family, drawing.text_italic
            );
            drawing_widths.insert(
                key,
                f64::from(
                    measure_text(
                        window,
                        text,
                        &layout.font_family,
                        size as f32,
                        weight,
                        drawing.text_italic,
                    )
                    .width,
                ),
            );
        }
        self.engine
            .set_text_measure(Some(Box::new(move |text, size, family, weight, italic| {
                let key = format!("{text}\u{0}{size}\u{0}{family}\u{0}{weight}\u{0}{italic}");
                drawing_widths
                    .get(&key)
                    .copied()
                    .unwrap_or_else(|| text.chars().count() as f64 * size * 0.6)
            })));
        self.rebuild_with_measure(width, height, scale_factor, |text| {
            f64::from(
                measure_text(
                    window,
                    text,
                    &layout.font_family,
                    layout.font_size as f32,
                    400,
                    false,
                )
                .width,
            )
        });
    }

    /// Append one live bar, forcing a rebuild on the next prepaint.
    fn append_bar(&mut self) {
        let i = self.bars + self.appended;
        let t = i as f64;
        let c = 100.0 + (t * 0.11).sin() * 6.0 + (t * 0.031).cos() * 14.0;
        let o = self.source_bars.close.last().copied().unwrap_or(c);
        let high = o.max(c) + 2.0;
        let low = o.min(c) - 2.0;
        let time = next_bar_timestamp(&self.source_bars.times);
        self.engine.update_series_bar(0, time, [o, high, low, c]);
        if let Some(id) = self.volume_id {
            let volume = (high - low) * 25_000.0 + i as f64 * 31.0;
            self.engine
                .update_series_bar(id, time, [volume, volume, volume, volume]);
        }
        self.source_bars.times.push(time);
        self.source_bars.open.push(o);
        self.source_bars.high.push(high);
        self.source_bars.low.push(low);
        self.source_bars.close.push(c);
        self.appended += 1;
        self.dirty = true;
    }

    fn maybe_append_live_bar(&mut self) {
        if self
            .frame_budget
            .is_some_and(|budget| self.painted >= budget)
        {
            return;
        }
        let epoch = if self.frame_budget.is_some() {
            self.painted / 60
        } else {
            self.started.elapsed().as_secs()
        };
        if self.painted > 0 && epoch > self.last_append_epoch {
            self.last_append_epoch = epoch;
            self.append_bar();
        }
    }

    fn record_frame_metrics(&mut self, metrics: GpuiFrameMetrics) {
        // Finite probe mode retains full samples for percentile reporting. Interactive charts
        // keep only the latest aggregate so an indefinitely open split workspace is bounded.
        if self.frame_budget.is_some() {
            self.plan_nanos.push(metrics.plan_nanos);
            self.paint_nanos.push(metrics.paint_nanos);
            self.total_nanos.push(metrics.total_nanos());
        }
        self.last = metrics;
        self.painted += 1;
    }

    fn now_ms(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1_000.0
    }

    fn needs_animation_frame(&self) -> bool {
        self.frame_budget.is_some() || self.kinetic_active || self.engine.scroll_animation_active()
    }

    fn local_position(&self, position: gpui::Point<gpui::Pixels>) -> (f64, f64, f64) {
        let window_x: f32 = position.x.into();
        let window_y: f32 = position.y.into();
        let chart_x = f64::from(window_x - self.viewport_offset.0);
        let y = f64::from(window_y - self.viewport_offset.1);
        (chart_x, chart_x - self.engine.pane_left, y)
    }

    fn separator_at(&self, y: f64) -> Option<usize> {
        self.engine
            .panes
            .iter()
            .skip(1)
            .position(|pane| (y - pane.top).abs() <= PANE_SEPARATOR_HIT)
    }

    fn update_cursor(&mut self, chart_x: f64, y: f64) {
        let pane_x = chart_x - self.engine.pane_left;
        let active_separator = matches!(self.drag, Some(DragMode::PaneSeparator { .. }));
        let separator = self
            .gesture_config
            .panes_resize
            .then(|| self.separator_at(y))
            .flatten();
        let separator_hover = (!active_separator).then_some(separator).flatten();
        if self.engine.separator_hover != separator_hover {
            self.engine.set_separator_hover(separator_hover);
            self.dirty = true;
        }
        let drawing_cursor = (self.armed_tool.is_none()
            && pane_x >= 0.0
            && pane_x <= self.engine.pane_w
            && y >= 0.0
            && y <= self.engine.pane_h)
            .then(|| self.engine.hit_test_drawing(pane_x, y))
            .flatten()
            .map(|hit| match hit.cursor {
                "pointer" => CursorStyle::PointingHand,
                "move" => CursorStyle::ClosedHand,
                "ns-resize" => CursorStyle::ResizeUpDown,
                "ew-resize" => CursorStyle::ResizeLeftRight,
                "nwse-resize" => CursorStyle::ResizeUpLeftDownRight,
                "nesw-resize" => CursorStyle::ResizeUpRightDownLeft,
                _ => CursorStyle::Crosshair,
            });
        let cursor = if active_separator || separator.is_some() {
            CursorStyle::ResizeRow
        } else if y > self.engine.pane_h {
            CursorStyle::ResizeLeftRight
        } else if chart_x < self.engine.pane_left
            || chart_x > self.engine.pane_left + self.engine.pane_w
        {
            CursorStyle::ResizeUpDown
        } else if let Some(cursor) = drawing_cursor {
            cursor
        } else if self.engine.hovered_series().is_some() {
            CursorStyle::PointingHand
        } else {
            CursorStyle::Crosshair
        };
        if self.cursor_style != cursor {
            self.cursor_style = cursor;
            self.dirty = true;
        }
    }

    fn cancel_kinetic_scroll(&mut self) {
        self.engine.kinetic_stop();
        if self.kinetic_active {
            self.engine.time_scale_end_scroll();
        }
        self.kinetic_active = false;
    }

    fn begin_mouse_pan(&mut self, pane_x: f64) {
        // Close any stale snapshot first. In particular, clicking during a previous coast must not
        // let the next `scroll_to` reuse that coast's source start point.
        self.engine.time_scale_end_scroll();
        self.engine.time_scale_start_scroll(pane_x);
        self.engine.kinetic_begin_sampling(
            self.gesture_config.kinetic_mouse,
            pane_x,
            self.now_ms(),
        );
    }

    fn end_mouse_pan(&mut self, pane_x: f64) {
        self.kinetic_active =
            self.gesture_config.kinetic_mouse && self.engine.kinetic_release(pane_x, self.now_ms());
        if !self.kinetic_active {
            self.engine.kinetic_stop();
            self.engine.time_scale_end_scroll();
        }
    }

    fn mark_press_moved(&mut self, pane_x: f64, y: f64) {
        if let Some((start_x, start_y)) = self.press_start {
            self.press_moved |=
                (pane_x - start_x).abs() + (y - start_y).abs() >= CLICK_SLOP_MANHATTAN;
        }
    }

    fn mouse_sample(
        &self,
        pane_x: f64,
        y: f64,
        target: InputTarget,
        modifiers: gpui::Modifiers,
    ) -> PointerSample {
        PointerSample {
            id: 1,
            device: InputDevice::Mouse,
            target,
            modifiers: InputModifiers {
                shift: modifiers.shift,
                control: modifiers.control,
                alt: modifiers.alt,
                meta: modifiers.platform,
            },
            x: pane_x,
            y,
            timestamp_ms: self.now_ms(),
            pressure: 0.5,
            tilt_x: 0.0,
            tilt_y: 0.0,
        }
    }

    fn input_target_at(&self, chart_x: f64, pane_x: f64, y: f64) -> InputTarget {
        if self.gesture_config.panes_resize && self.separator_at(y).is_some() {
            InputTarget::Separator
        } else if y > self.engine.pane_h {
            InputTarget::TimeAxis
        } else if chart_x < self.engine.pane_left
            || chart_x > self.engine.pane_left + self.engine.pane_w
        {
            InputTarget::PriceAxis
        } else if self.armed_tool.is_some() || self.engine.hit_test_drawing(pane_x, y).is_some() {
            InputTarget::Drawing
        } else {
            InputTarget::Pane
        }
    }

    fn update_crosshair(&mut self, pane_x: f64, y: f64) {
        if pane_x >= 0.0 && pane_x <= self.engine.pane_w && y >= 0.0 && y <= self.engine.pane_h {
            self.engine.crosshair = Some((pane_x, y));
            let hovered = self.engine.hit_test_series(pane_x, y);
            self.engine.set_hovered_series(hovered);
        } else {
            self.engine.crosshair = None;
            self.engine.set_hovered_series(None);
        }
        self.dirty = true;
    }

    fn update_pointer_feedback(&mut self, chart_x: f64, pane_x: f64, y: f64) {
        let over_separator = self.gesture_config.panes_resize && self.separator_at(y).is_some();
        if matches!(self.drag, Some(DragMode::PaneSeparator { .. })) || over_separator {
            self.engine.crosshair = None;
            self.engine.set_hovered_series(None);
            self.dirty = true;
        } else {
            // Match the browser host: refresh the hit-test first, then derive the cursor from the
            // same move so a candle/series immediately exposes its click affordance.
            self.update_crosshair(pane_x, y);
        }
        self.update_cursor(chart_x, y);
    }

    fn update_crosshair_modifier(&mut self, control: bool, platform: bool) {
        let enabled = (control || platform) && self.armed_tool.is_some();
        if self.engine.crosshair_ohlc_magnet != enabled {
            self.engine.crosshair_ohlc_magnet = enabled;
            self.dirty = true;
        }
    }

    fn clear_pointer_state(&mut self) {
        self.input.cancel();
        match self.drag.take() {
            Some(DragMode::Pan { price_pan }) => {
                if let Some((pane, target)) = price_pan {
                    self.engine.price_axis_end_scroll(pane, target);
                }
                self.engine.time_scale_end_scroll();
            }
            Some(DragMode::TimeAxis) => self.engine.time_axis_end_scale(),
            Some(DragMode::PriceAxis { pane, target }) => {
                self.engine.price_axis_end_scale(pane, target);
            }
            Some(DragMode::Drawing) => self.engine.drawing_drag_end(),
            Some(DragMode::BrushCreation) => self.engine.brush_create_cancel(),
            Some(DragMode::PaneSeparator { .. }) | None => {}
        }
        self.cancel_kinetic_scroll();
        self.press_start = None;
        self.press_moved = false;
        self.engine.crosshair_ohlc_magnet = false;
        self.engine.crosshair = None;
        self.engine.set_hovered_series(None);
        self.engine.set_separator_hover(None);
        self.legend = "O —  H —  L —  C —".to_string();
        self.cursor_style = CursorStyle::Crosshair;
        self.dirty = true;
    }

    fn on_hover(&mut self, hovered: &bool, _window: &mut Window, cx: &mut Context<Self>) {
        if !*hovered {
            self.clear_pointer_state();
            cx.notify();
        }
    }

    fn on_modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_crosshair_modifier(event.control, event.platform);
        cx.notify();
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(focus) = &self.focus_handle {
            window.focus(focus, cx);
        }
        self.cancel_kinetic_scroll();
        // Recover defensively from a stale scroll snapshot left by an interrupted host gesture.
        self.engine.time_scale_end_scroll();
        self.engine.cancel_scroll_animation();
        let (chart_x, pane_x, y) = self.local_position(event.position);
        self.input_target = self.input_target_at(chart_x, pane_x, y);
        let sample = self.mouse_sample(pane_x, y, self.input_target, event.modifiers);
        self.input.pointer_down(sample);
        self.update_crosshair_modifier(event.modifiers.control, event.modifiers.platform);
        let pane = self.engine.pane_index_at_y(y);
        self.update_cursor(chart_x, y);
        self.press_start = Some((pane_x, y));
        self.press_moved = false;

        if self.armed_tool == Some(DrawingKind::Brush) {
            let template = self.drawing_template.json();
            if self.engine.brush_create_start(Some(&template), pane_x, y) {
                self.drag = Some(DragMode::BrushCreation);
                self.update_crosshair(pane_x, y);
                cx.notify();
                return;
            }
        }

        if event.click_count >= 2 {
            if y > self.engine.pane_h && self.gesture_config.axis_dblclick_reset_time {
                self.engine.reset_time_scale();
            } else if self.gesture_config.axis_dblclick_reset_price
                && self.engine.price_axis_target_at(pane, pane_x).is_some()
            {
                self.engine.reset_price_scales();
            }
            self.press_moved = true;
            self.dirty = true;
            cx.notify();
            return;
        }

        self.drag = if let Some(index) = self
            .gesture_config
            .panes_resize
            .then(|| self.separator_at(y))
            .flatten()
        {
            self.engine.set_separator_hover(None);
            Some(DragMode::PaneSeparator { index, last_y: y })
        } else if y > self.engine.pane_h {
            if self.gesture_config.axis_scale_time {
                self.engine.time_axis_start_scale(pane_x);
                Some(DragMode::TimeAxis)
            } else {
                None
            }
        } else if chart_x < self.engine.pane_left {
            if self.gesture_config.axis_scale_price
                && self
                    .engine
                    .price_axis_scalable(pane, PriceScaleTarget::Left)
            {
                self.engine
                    .price_axis_start_scale(pane, PriceScaleTarget::Left, y);
                Some(DragMode::PriceAxis {
                    pane,
                    target: PriceScaleTarget::Left,
                })
            } else {
                None
            }
        } else if chart_x > self.engine.pane_left + self.engine.pane_w {
            if self.gesture_config.axis_scale_price
                && self
                    .engine
                    .price_axis_scalable(pane, PriceScaleTarget::Right)
            {
                self.engine
                    .price_axis_start_scale(pane, PriceScaleTarget::Right, y);
                Some(DragMode::PriceAxis {
                    pane,
                    target: PriceScaleTarget::Right,
                })
            } else {
                None
            }
        } else if self.armed_tool.is_some() {
            None
        } else if self.engine.drawing_drag_start_at(pane_x, y) {
            Some(DragMode::Drawing)
        } else if self.gesture_config.pan {
            self.begin_mouse_pan(pane_x);
            let price_pan = self
                .engine
                .begin_price_pan_at(pane, pane_x, y)
                .map(|target| (pane, target));
            Some(DragMode::Pan { price_pan })
        } else {
            None
        };
        if matches!(self.drag, Some(DragMode::PaneSeparator { .. })) {
            self.engine.crosshair = None;
            self.engine.set_hovered_series(None);
            self.dirty = true;
        } else {
            self.update_crosshair(pane_x, y);
        }
        cx.notify();
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (chart_x, pane_x, y) = self.local_position(event.position);
        let sample = self.mouse_sample(pane_x, y, self.input_target, event.modifiers);
        self.input.pointer_move(sample);
        self.update_crosshair_modifier(event.modifiers.control, event.modifiers.platform);
        if event.dragging() {
            self.mark_press_moved(pane_x, y);
        }
        match self.drag {
            Some(DragMode::Pan { price_pan }) if event.dragging() => {
                self.engine.time_scale_scroll_to(pane_x);
                self.engine.kinetic_add_sample(pane_x, self.now_ms());
                if let Some((pane, target)) = price_pan {
                    self.engine.price_axis_scroll_to(pane, target, y);
                }
            }
            Some(DragMode::TimeAxis) if event.dragging() => {
                self.engine.time_axis_scale_to(pane_x);
            }
            Some(DragMode::PriceAxis { pane, target }) if event.dragging() => {
                self.engine.price_axis_scale_to(pane, target, y);
            }
            Some(DragMode::PaneSeparator { index, last_y }) if event.dragging() => {
                self.engine.drag_pane_separator(index, y - last_y);
                self.drag = Some(DragMode::PaneSeparator { index, last_y: y });
                self.dirty = true;
            }
            Some(DragMode::Drawing) if event.dragging() => {
                self.engine.drawing_drag_to(
                    pane_x,
                    y,
                    DrawingModifiers {
                        magnet: event.modifiers.control || event.modifiers.platform,
                        straighten: event.modifiers.shift,
                    },
                );
            }
            Some(DragMode::BrushCreation) if event.dragging() => {
                // Only a captured point dirties the frame; rejected sub-threshold samples leave
                // the scene untouched so fast drags don't rebuild per raw pointer event.
                if self.engine.brush_create_add(pane_x, y) {
                    self.dirty = true;
                }
            }
            _ => {
                if self
                    .armed_tool
                    .is_some_and(|tool| tool != DrawingKind::Brush)
                {
                    self.engine.drawing_create_move(
                        pane_x,
                        y,
                        DrawingModifiers {
                            magnet: event.modifiers.control || event.modifiers.platform,
                            straighten: event.modifiers.shift,
                        },
                    );
                    self.dirty = true;
                }
            }
        }
        if chart_x >= self.engine.pane_left
            && chart_x <= self.engine.pane_left + self.engine.pane_w
            && (0.0..=self.engine.pane_h).contains(&y)
        {
            self.update_legend(pane_x);
        } else {
            self.legend = "O —  H —  L —  C —".to_string();
        }
        self.update_pointer_feedback(chart_x, pane_x, y);
        cx.notify();
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let (chart_x, pane_x, y) = self.local_position(event.position);
        let sample = self.mouse_sample(pane_x, y, self.input_target, event.modifiers);
        self.input.pointer_up(sample);
        self.update_crosshair_modifier(event.modifiers.control, event.modifiers.platform);
        self.mark_press_moved(pane_x, y);
        let moved = self.press_moved;
        self.press_start = None;
        self.press_moved = false;
        let select_click = match self.drag.take() {
            Some(DragMode::Pan { price_pan }) => {
                if let Some((pane, target)) = price_pan {
                    self.engine.price_axis_end_scroll(pane, target);
                }
                self.end_mouse_pan(pane_x);
                !moved
            }
            Some(DragMode::TimeAxis) => {
                self.engine.time_axis_end_scale();
                false
            }
            Some(DragMode::PriceAxis { pane, target }) => {
                self.engine.price_axis_end_scale(pane, target);
                false
            }
            Some(DragMode::PaneSeparator { .. }) => false,
            Some(DragMode::Drawing) => {
                self.engine.drawing_drag_end();
                false
            }
            Some(DragMode::BrushCreation) => {
                let id = self.engine.brush_create_end();
                if id > 0 {
                    self.click_status = format!("created brush #{id}");
                    self.armed_tool = None;
                }
                false
            }
            None if self
                .armed_tool
                .is_some_and(|tool| tool != DrawingKind::Brush) =>
            {
                if !moved {
                    let result = self.engine.drawing_create_click(
                        pane_x,
                        y,
                        DrawingModifiers {
                            magnet: event.modifiers.control || event.modifiers.platform,
                            straighten: event.modifiers.shift,
                        },
                    );
                    if result > 0 {
                        self.click_status = format!("created drawing #{result}");
                        self.armed_tool = None;
                    }
                }
                false
            }
            None => !moved,
        };
        if select_click {
            let selected = self.engine.hit_test_series(pane_x, y);
            self.engine.set_selected_series(selected);
            self.engine.select_drawing_at(pane_x, y);
            self.update_legend(pane_x);
            self.click_status = format!("click x={pane_x:.1} y={y:.1}");
        }
        self.update_pointer_feedback(chart_x, pane_x, y);
        cx.notify();
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (chart_x, pane_x, y) = self.local_position(event.position);
        self.update_crosshair_modifier(event.modifiers.control, event.modifiers.platform);
        // GPUI's Windows backend reports wheel-up as a positive line delta. The browser turns
        // wheel-up's negative DOM deltaY into the same positive normalized value. Convert lines
        // with the browser's 32px adjustment and do not reverse either axis a second time.
        let delta = event.delta.pixel_delta(px(WHEEL_LINE_HEIGHT));
        let dx: f32 = delta.x.into();
        let dy: f32 = delta.y.into();
        let normalized_x = f64::from(dx) / 100.0;
        let normalized_y = f64::from(dy) / 100.0;
        let delta_mode = if matches!(event.delta, ScrollDelta::Pixels(_)) {
            WheelDeltaMode::Pixel
        } else {
            WheelDeltaMode::Line
        };
        let intent = WheelSample {
            x: pane_x,
            y,
            delta_x: normalized_x,
            delta_y: normalized_y,
            delta_mode,
            modifiers: InputModifiers {
                shift: event.modifiers.shift,
                control: event.modifiers.control,
                alt: event.modifiers.alt,
                meta: event.modifiers.platform,
            },
            timestamp_ms: self.now_ms(),
        }
        .intent(self.gesture_config.wheel_behavior);
        if matches!(intent, WheelIntent::Zoom | WheelIntent::PanAndZoom)
            && normalized_y != 0.0
            && self.gesture_config.wheel_zoom
        {
            let zoom = nucleuscharts_engine::wheel_zoom_scale(normalized_y);
            let pane = self.engine.pane_index_at_y(y);
            if chart_x < self.engine.pane_left {
                self.engine
                    .price_axis_wheel_zoom(pane, PriceScaleTarget::Left, y, zoom);
            } else if chart_x > self.engine.pane_left + self.engine.pane_w {
                self.engine
                    .price_axis_wheel_zoom(pane, PriceScaleTarget::Right, y, zoom);
            } else {
                self.engine.time_scale_zoom(pane_x, zoom);
            }
        }
        let pan_delta = if self.gesture_config.wheel_behavior == WheelBehavior::Auto
            || normalized_x.abs() >= normalized_y.abs()
        {
            normalized_x
        } else {
            normalized_y
        };
        if matches!(intent, WheelIntent::Pan | WheelIntent::PanAndZoom)
            && pan_delta != 0.0
            && self.gesture_config.wheel_scroll
        {
            self.engine.time_scale_start_scroll(0.0);
            self.engine
                .time_scale_scroll_to(nucleuscharts_engine::WHEEL_SCROLL_PX_PER_DELTA * pan_delta);
            self.engine.time_scale_end_scroll();
        }
        self.update_pointer_feedback(chart_x, pane_x, y);
        cx.stop_propagation();
        cx.notify();
    }

    fn on_pinch(&mut self, event: &PinchEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let (chart_x, pane_x, y) = self.local_position(event.position);
        let intent = WheelSample {
            x: pane_x,
            y,
            delta_y: f64::from(event.delta),
            modifiers: InputModifiers {
                control: true,
                ..InputModifiers::default()
            },
            timestamp_ms: self.now_ms(),
            ..WheelSample::default()
        }
        .intent(self.gesture_config.wheel_behavior);
        if intent == WheelIntent::Zoom && self.gesture_config.wheel_zoom {
            self.engine.time_scale_zoom(
                pane_x,
                nucleuscharts_engine::pinch_zoom_scale(f64::from(event.delta)),
            );
            self.update_pointer_feedback(chart_x, pane_x, y);
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let step = if event.keystroke.modifiers.control || event.keystroke.modifiers.shift {
            10.0
        } else {
            1.0
        };
        let center = self.engine.pane_w / 2.0;
        let handled = match event.keystroke.key.as_str() {
            "left" => {
                self.engine.start_scroll_animation(
                    self.engine.scroll_position() - step,
                    160.0,
                    self.now_ms(),
                );
                true
            }
            "right" => {
                self.engine.start_scroll_animation(
                    self.engine.scroll_position() + step,
                    160.0,
                    self.now_ms(),
                );
                true
            }
            "+" | "=" if self.gesture_config.wheel_zoom => {
                self.engine.time_scale_zoom(center, 0.5);
                true
            }
            "-" | "_" if self.gesture_config.wheel_zoom => {
                self.engine.time_scale_zoom(center, -0.5);
                true
            }
            "home" => {
                self.engine.fit_content();
                true
            }
            "delete" | "backspace" => self.engine.remove_selected_drawing(),
            "escape" => {
                self.engine.drawing_create_cancel();
                self.engine.brush_create_cancel();
                self.armed_tool = None;
                self.engine.set_selected_drawing(None);
                self.engine.crosshair = None;
                self.engine.set_hovered_series(None);
                true
            }
            _ => false,
        };
        if handled {
            // Browser keyboard gestures stop any wheel/mouse coast and close its saved scroll
            // snapshot. This intentionally does not cancel the new eased Left/Right animation.
            self.cancel_kinetic_scroll();
            self.dirty = true;
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn tick_animations(&mut self) {
        let now = self.now_ms();
        if self.kinetic_active {
            if self.engine.kinetic_finished(now) {
                self.engine.time_scale_end_scroll();
                self.engine.kinetic_stop();
                self.kinetic_active = false;
            } else if let Some(x) = self.engine.kinetic_position(now) {
                self.engine.time_scale_scroll_to(x);
                self.dirty = true;
            }
        }
        if self.engine.scroll_animation_active() {
            self.engine.scroll_animation_tick(now);
            self.dirty = true;
        }
    }

    fn report(&self) {
        let pct = |data: &[u64], p: f64| -> f64 {
            if data.is_empty() {
                return 0.0;
            }
            let mut v = data.to_vec();
            v.sort_unstable();
            let idx = ((v.len() as f64 - 1.0) * p).round() as usize;
            v[idx] as f64 / 1_000_000.0
        };
        println!("--- nucleuscharts_render_gpui probe ---");
        println!("frames painted  : {}", self.painted);
        println!("bars            : {}", self.bars + self.appended);
        println!("wall clock      : {:.2?}", self.started.elapsed());
        println!("scale factors   : {:?}", self.seen_scales);
        println!("sizes (logical) : {:?}", self.seen_sizes);
        println!(
            "last frame      : prims={} ops={} quads={} paths={} tris={} text_runs={} painted_runs={} dropped={}",
            self.last.prims,
            self.last.ops,
            self.last.quads,
            self.last.paths,
            self.last.triangles,
            self.last.text_runs,
            self.last.glyph_runs_painted,
            self.last.dropped_prims
        );
        println!("mesh vertices   : {}", self.last.mesh_vertices);
        println!(
            "quad batching   : {} quads -> {} paths ({} paint_quad calls saved)",
            self.last.batched_quads,
            self.last.quad_batches,
            self.last
                .batched_quads
                .saturating_sub(self.last.quad_batches)
        );
        println!(
            "adapter total ms: p50={:.3} p99={:.3}  (plan + gpui submission)",
            pct(&self.total_nanos, 0.50),
            pct(&self.total_nanos, 0.99)
        );
        println!(
            "text cache      : {} hits / {} misses",
            self.last.text_cache_hits, self.last.text_cache_misses
        );
        println!(
            "plan build ms   : p50={:.3} p95={:.3} p99={:.3} max={:.3}",
            pct(&self.plan_nanos, 0.50),
            pct(&self.plan_nanos, 0.95),
            pct(&self.plan_nanos, 0.99),
            pct(&self.plan_nanos, 1.0)
        );
        println!(
            "gpui paint ms   : p50={:.3} p95={:.3} p99={:.3} max={:.3}",
            pct(&self.paint_nanos, 0.50),
            pct(&self.paint_nanos, 0.95),
            pct(&self.paint_nanos, 0.99),
            pct(&self.paint_nanos, 1.0)
        );
    }
}

/// Build and paint one frame. Split out so both closures can hold disjoint borrows of `Probe`.
fn paint_probe(probe: &mut Probe, bounds: Bounds<gpui::Pixels>, window: &mut Window, cx: &mut App) {
    if probe
        .frame_budget
        .is_some_and(|budget| probe.painted >= budget)
    {
        return;
    }
    let viewport = NucleusViewport::from_bounds(
        bounds.origin.x.into(),
        bounds.origin.y.into(),
        bounds.size.width.into(),
        bounds.size.height.into(),
    );
    let scale_factor = window.scale_factor();
    let plan_dirty = probe.plan_dirty;
    let mut cached_metrics = probe.last;
    cached_metrics.plan_nanos = 0;

    // Disjoint field borrows: the adapter reads `frame` while mutating `renderer`.
    let Probe {
        engine,
        renderer,
        frame,
        axis,
        ..
    } = probe;
    let prepared = PreparedNucleusFrame::from_engine(frame, engine).with_axis(axis, &[]);
    let result = if plan_dirty {
        renderer.paint_frame(&prepared, viewport, scale_factor, window, cx)
    } else {
        renderer.paint_planned_frame(
            &prepared,
            viewport,
            scale_factor,
            window,
            cx,
            cached_metrics,
        )
    };
    match result {
        Ok(metrics) => {
            probe.plan_dirty = false;
            probe.record_frame_metrics(metrics);
        }
        Err(e) => eprintln!("nucleuscharts probe: frame skipped: {e}"),
    }
}

impl Render for Probe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity: Entity<Probe> = cx.entity();
        let prepaint_entity = entity.clone();

        // Finite probes deliberately sample consecutive frames. Interactive charts request a
        // follow-up only while an engine-owned animation is active; ordinary input/resize/data
        // mutations already notify GPUI and an idle chart must stay idle.
        let done = self
            .frame_budget
            .is_some_and(|budget| self.painted >= budget);
        if done {
            if !self.reported {
                self.report();
                self.reported = true;
            }
            cx.quit();
        } else if self.needs_animation_frame() {
            window.request_animation_frame();
        }

        // A solid Nucleus layout background emits no `Prim`: each host clears its own surface from
        // the layout options (as WebGPU does). The probe is a GPUI host, so it must do the same;
        // otherwise GPUI's default transparent/black client clear shows through.
        let fallback = nucleuscharts_core::style::DEFAULT_SURFACE_RGB;
        let background = Color::parse_css(&self.engine.options.get().layout.background.color)
            .unwrap_or(Color::rgb(fallback.0, fallback.1, fallback.2));
        let background = nucleuscharts_render_gpui::backend::to_hsla(background);

        let focus = self
            .focus_handle
            .as_ref()
            .expect("the window host installs a focus handle")
            .clone();
        div()
            .relative()
            .bg(background)
            .size_full()
            .cursor(self.cursor_style)
            .id("nucleuscharts-chart-root")
            .track_focus(&focus)
            .key_context("NucleusGpuiChart")
            .on_hover(cx.listener(Self::on_hover))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .on_pinch(cx.listener(Self::on_pinch))
            .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
            .on_key_down(cx.listener(Self::on_key_down))
            .child(
                canvas(
                    move |bounds: Bounds<gpui::Pixels>, window, cx| {
                        let w: f32 = bounds.size.width.into();
                        let h: f32 = bounds.size.height.into();
                        let offset_x: f32 = bounds.origin.x.into();
                        let offset_y: f32 = bounds.origin.y.into();
                        let scale_factor = window.scale_factor();
                        prepaint_entity.update(cx, |probe: &mut Probe, _| {
                            if probe
                                .frame_budget
                                .is_some_and(|budget| probe.painted >= budget)
                            {
                                return;
                            }
                            probe.viewport_offset = (offset_x, offset_y);
                            probe.tick_animations();
                            probe.maybe_append_live_bar();
                            probe.rebuild(w, h, scale_factor, window);
                        });
                        bounds
                    },
                    move |_bounds: Bounds<gpui::Pixels>, prepainted, window, cx| {
                        entity.update(cx, |probe: &mut Probe, cx| {
                            paint_probe(probe, prepainted, window, cx);
                        });
                    },
                )
                .size_full(),
            )
    }
}

impl Focusable for Probe {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle
            .as_ref()
            .expect("the window host installs a focus handle")
            .clone()
    }
}

fn layout_first(node: &WorkspaceLayout) -> u64 {
    match node {
        WorkspaceLayout::Cell { id } => *id,
        WorkspaceLayout::Split { a, .. } => layout_first(a),
    }
}

fn layout_last(node: &WorkspaceLayout) -> u64 {
    match node {
        WorkspaceLayout::Cell { id } => *id,
        WorkspaceLayout::Split { b, .. } => layout_last(b),
    }
}

const WORKSPACE_DIVIDER_LAYOUT_PX: f32 = 1.0;
const WORKSPACE_DIVIDER_HIT_PX: f32 = 5.0;

fn layout_extent_in_direction<F>(
    node: &WorkspaceLayout,
    direction: SplitDirection,
    leaf_extent: &F,
) -> f32
where
    F: Fn(u64, SplitDirection) -> f32,
{
    match node {
        WorkspaceLayout::Cell { id } => leaf_extent(*id, direction).max(0.0),
        WorkspaceLayout::Split {
            direction: split_direction,
            a,
            b,
            ..
        } => {
            let a_extent = layout_extent_in_direction(a, direction, leaf_extent);
            let b_extent = layout_extent_in_direction(b, direction, leaf_extent);
            if *split_direction == direction {
                a_extent + WORKSPACE_DIVIDER_LAYOUT_PX + b_extent
            } else {
                a_extent.max(b_extent)
            }
        }
    }
}

fn owning_split_total_extent<F>(
    node: &WorkspaceLayout,
    left: u64,
    right: u64,
    direction: SplitDirection,
    leaf_extent: &F,
) -> Option<f32>
where
    F: Fn(u64, SplitDirection) -> f32,
{
    match node {
        WorkspaceLayout::Cell { .. } => None,
        WorkspaceLayout::Split {
            direction: split_direction,
            a,
            b,
            ..
        } => {
            if *split_direction == direction && layout_last(a) == left && layout_first(b) == right {
                return Some(
                    layout_extent_in_direction(a, direction, leaf_extent)
                        + WORKSPACE_DIVIDER_LAYOUT_PX
                        + layout_extent_in_direction(b, direction, leaf_extent),
                );
            }
            owning_split_total_extent(a, left, right, direction, leaf_extent)
                .or_else(|| owning_split_total_extent(b, left, right, direction, leaf_extent))
        }
    }
}

fn split_flex_ratios(ratio: f64) -> (f32, f32) {
    let first = ratio.clamp(0.0, 1.0) as f32;
    (first, 1.0 - first)
}

#[derive(Clone, Copy, Debug)]
enum DemoAction {
    Series(SeriesKind),
    CandleBodyColor,
    CandleWickColor,
    CandleBorderColor,
    CandleWicksVisible,
    CandleBordersVisible,
    CandlePartsReset,
    LineColor,
    LineWidth,
    AreaColor,
    Sma,
    Volume,
    Rsi,
    Split(SplitDirection),
    Close,
    Cap,
    Drawing(DrawingKind),
    ClearDrawings,
    DrawingColor,
    DrawingStyle,
    DrawingWidth,
    DrawingText,
    DrawingTextColor,
    DrawingTextSize,
    DrawingTextWeight,
    DrawingItalic,
    CrosshairMode,
    CrosshairColor,
    CrosshairWidth,
    CrosshairStyle,
    CrosshairLabelBackground,
    CrosshairLabels,
    Theme,
    Grid,
    GridColor,
    GridStyle,
    Font,
    FontSize,
    PriceLine,
    PriceLineStyle,
    LastValue,
    TitleVisible,
    TitleText,
    Countdown,
    BidAsk,
    AxisBorders,
    AxisBorderColor,
    AxisText,
    Separator,
    Watermark,
    WatermarkText,
    WatermarkColor,
    WatermarkSize,
    AxisScaling,
    Kinetic,
    Reset,
    Fixture(usize),
}

struct DemoCell {
    id: u64,
    chart: Entity<Probe>,
}

#[derive(Clone, Copy)]
struct WorkspaceDrag {
    left: u64,
    right: u64,
    direction: SplitDirection,
    start: f32,
    start_ratio: f64,
    current_ratio: f64,
    extent: f32,
}

struct InteractiveDemo {
    workspace: Workspace,
    cells: Vec<DemoCell>,
    active: u64,
    maximized: Option<u64>,
    theme: DemoTheme,
    max_index: usize,
    max_charts: Option<usize>,
    split_asset_seq: usize,
    focus_initialized: bool,
    _root_observer: Subscription,
    workspace_drag: Option<WorkspaceDrag>,
    status: String,
}

impl InteractiveDemo {
    fn new(bars: usize, cx: &mut Context<Self>) -> Self {
        let chart = cx.new(|cx| {
            let mut probe = Probe::new_interactive(bars);
            probe.engine.series[0].title = "NUCLEUS".into();
            probe.engine.series[0].countdown_visible = false;
            probe.focus_handle = Some(cx.focus_handle());
            probe
        });
        let root_observer = cx.observe(&chart, |_, _, cx| cx.notify());
        Self {
            workspace: Workspace::new(),
            cells: vec![DemoCell { id: 1, chart }],
            active: 1,
            maximized: None,
            theme: DemoTheme::Light,
            max_index: 0,
            max_charts: None,
            split_asset_seq: 0,
            focus_initialized: false,
            _root_observer: root_observer,
            workspace_drag: None,
            status: format!(
                "interactive native GPUI demo · active cell 1 · {} feature groups",
                TOOLBAR_FEATURE_MANIFEST.len()
            ),
        }
    }

    fn active_chart(&self) -> Option<Entity<Probe>> {
        self.cells
            .iter()
            .find(|cell| cell.id == self.active)
            .map(|cell| cell.chart.clone())
    }

    fn root_chart(&self) -> Option<Entity<Probe>> {
        self.cells
            .iter()
            .find(|cell| cell.id == 1)
            .map(|cell| cell.chart.clone())
    }

    fn update_chart(
        chart: Option<Entity<Probe>>,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Probe),
    ) {
        if let Some(chart) = chart {
            chart.update(cx, |probe, child_cx| {
                f(probe);
                probe.dirty = true;
                child_cx.notify();
            });
        }
    }

    fn update_active(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Probe)) {
        Self::update_chart(self.active_chart(), cx, f);
    }

    fn update_root(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Probe)) {
        Self::update_chart(self.root_chart(), cx, f);
    }

    fn activate(&mut self, id: u64, cx: &mut Context<Self>) {
        self.active = id;
        self.status = format!("active cell {id}");
        cx.notify();
    }

    fn split(&mut self, direction: SplitDirection, activate_new: bool, cx: &mut Context<Self>) {
        if self
            .max_charts
            .is_some_and(|max| self.workspace.chart_count() >= max)
        {
            self.status = "split rejected: host chart limit reached".into();
            return;
        }
        let end_time = self
            .root_chart()
            .and_then(|chart| chart.read(cx).source_bars.times.last().copied())
            .unwrap_or(1_600_000_000.0);
        match self.workspace.split(self.active, direction) {
            Ok(id) => {
                self.maximized = None;
                self.split_asset_seq += 1;
                let split_bars = split_asset_bars(self.split_asset_seq, end_time);
                let theme = self.theme;
                let sequence = self.split_asset_seq;
                let chart = cx.new(|cx| {
                    let mut probe = Probe::new_interactive(300);
                    probe.replace_source_bars(split_bars);
                    apply_package_theme(&mut probe.engine, theme);
                    probe.engine.series[0].title = format!("ASSET {sequence}");
                    probe.engine.series[0].countdown_visible = false;
                    probe.focus_handle = Some(cx.focus_handle());
                    probe
                });
                self.cells.push(DemoCell { id, chart });
                if activate_new {
                    self.active = id;
                }
                self.status = format!("created independent ASSET {sequence} in cell {id}");
            }
            Err(error) => self.status = format!("split rejected: {error:?}"),
        }
    }

    fn close_active(&mut self) {
        if self.active == 1 {
            self.status = "primary cell 1 is protected".into();
            return;
        }
        if self.workspace.remove(self.active).is_ok() {
            let removed = self.active;
            self.cells.retain(|cell| cell.id != removed);
            self.maximized = None;
            self.active = self.workspace.cell_ids()[0];
            self.status = format!("closed cell {removed}; active {}", self.active);
        } else {
            self.status = "the final chart cannot be closed".into();
        }
    }

    fn toggle_maximize(&mut self, id: u64) {
        self.active = id;
        self.maximized = (self.maximized != Some(id)).then_some(id);
        self.status = self.maximized.map_or_else(
            || format!("restored split layout; active cell {id}"),
            |_| format!("maximized cell {id}; Ctrl/Cmd+click to restore"),
        );
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if event.keystroke.key == "tab"
            && !modifiers.control
            && !modifiers.platform
            && !modifiers.alt
        {
            if modifiers.shift {
                window.focus_prev(cx);
            } else {
                window.focus_next(cx);
            }
            cx.stop_propagation();
            return;
        }
        if !(modifiers.control || modifiers.platform) || modifiers.shift || modifiers.alt {
            return;
        }
        let direction = match event.keystroke.key.as_str() {
            "h" => Some(SplitDirection::Horizontal),
            "v" => Some(SplitDirection::Vertical),
            _ => None,
        };
        if let Some(direction) = direction {
            self.split(direction, false, cx);
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn drag_workspace_divider(&mut self, position: gpui::Point<gpui::Pixels>) {
        let Some(mut drag) = self.workspace_drag else {
            return;
        };
        let current: f32 = match drag.direction {
            SplitDirection::Horizontal => position.x.into(),
            SplitDirection::Vertical => position.y.into(),
        };
        let ratio = (drag.start_ratio + f64::from((current - drag.start) / drag.extent.max(1.0)))
            .clamp(0.05, 0.95);
        if (ratio - drag.current_ratio).abs() > f64::EPSILON {
            drag.current_ratio = ratio;
            self.workspace_drag = Some(drag);
            self.status = format!("resizing divider {}/{}", drag.left, drag.right);
        }
    }

    fn finish_workspace_drag(&mut self) {
        let Some(drag) = self.workspace_drag.take() else {
            return;
        };
        let delta = drag.current_ratio - drag.start_ratio;
        if delta.abs() > f64::EPSILON
            && self
                .workspace
                .resize_between(drag.left, drag.right, delta)
                .is_ok()
        {
            self.status = format!("resized divider {}/{}", drag.left, drag.right);
        }
    }

    fn apply_action(&mut self, action: DemoAction, cx: &mut Context<Self>) {
        match action {
            DemoAction::Series(kind) => self.update_active(cx, |p| p.set_series_kind(kind)),
            DemoAction::CandleBodyColor => self.update_active(cx, |p| {
                let s = &mut p.engine.series[0];
                let alternate = s.up_color.as_deref() == Some(nucleuscharts_core::style::MARKET_UP_CSS);
                s.up_color = Some(
                    if alternate {
                        "#2962ff"
                    } else {
                        nucleuscharts_core::style::MARKET_UP_CSS
                    }
                    .into(),
                );
                s.down_color = Some(
                    if alternate {
                        "#ff9800"
                    } else {
                        nucleuscharts_core::style::MARKET_DOWN_CSS
                    }
                    .into(),
                );
            }),
            DemoAction::CandleWickColor => self.update_active(cx, |p| {
                let s = &mut p.engine.series[0];
                if s.wick_up_color.is_some() {
                    s.wick_up_color = None;
                    s.wick_down_color = None;
                } else {
                    s.wick_up_color = Some("#2962ff".into());
                    s.wick_down_color = Some("#ff9800".into());
                }
            }),
            DemoAction::CandleBorderColor => self.update_active(cx, |p| {
                let s = &mut p.engine.series[0];
                if s.border_up_color.is_some() {
                    s.border_up_color = None;
                    s.border_down_color = None;
                } else {
                    s.border_up_color = Some("#2962ff".into());
                    s.border_down_color = Some("#ff9800".into());
                }
            }),
            DemoAction::CandleWicksVisible => self.update_active(cx, |p| {
                let s = &mut p.engine.series[0];
                s.wick_visible = Some(!s.wick_visible.unwrap_or(true));
            }),
            DemoAction::CandleBordersVisible => self.update_active(cx, |p| {
                let s = &mut p.engine.series[0];
                s.border_visible = Some(!s.border_visible.unwrap_or(true));
            }),
            DemoAction::CandlePartsReset => self.update_active(cx, |p| {
                let s = &mut p.engine.series[0];
                s.wick_up_color = None;
                s.wick_down_color = None;
                s.border_up_color = None;
                s.border_down_color = None;
            }),
            DemoAction::LineColor => self.update_active(cx, |p| {
                let s = &mut p.engine.series[0];
                s.line_color = Some(
                    if s.line_color.as_deref() == Some("#2196f3") {
                        "#ab47bc"
                    } else {
                        "#2196f3"
                    }
                    .into(),
                );
            }),
            DemoAction::LineWidth => self.update_active(cx, |p| {
                let s = &mut p.engine.series[0];
                s.line_width = Some(if s.line_width.unwrap_or(3.0) >= 8.0 {
                    1.0
                } else {
                    s.line_width.unwrap_or(3.0) + 1.0
                });
            }),
            DemoAction::AreaColor => self.update_active(cx, |p| {
                let s = &mut p.engine.series[0];
                let alternate = s.area_top_color.as_deref()
                    == Some(nucleuscharts_core::style::MARKET_UP_CSS);
                let top = if alternate {
                    "#2962ff"
                } else {
                    nucleuscharts_core::style::MARKET_UP_CSS
                };
                s.area_top_color = Some(top.into());
                s.area_bottom_color = Some(format!("{top}00"));
            }),
            DemoAction::Sma => self.update_root(cx, Probe::toggle_sma),
            DemoAction::Volume => self.update_root(cx, Probe::toggle_volume),
            DemoAction::Rsi => self.update_root(cx, Probe::toggle_rsi),
            DemoAction::Split(direction) => self.split(direction, true, cx),
            DemoAction::Close => self.close_active(),
            DemoAction::Cap => {
                self.max_index = (self.max_index + 1) % 4;
                let cap = [None, Some(2), Some(3), Some(4)][self.max_index];
                self.max_charts = cap;
                self.status = format!("max charts: {}", cap.map_or("∞".into(), |v| v.to_string()));
            }
            DemoAction::Drawing(kind) => self.update_root(cx, |p| p.arm_drawing(kind)),
            DemoAction::ClearDrawings => self.update_root(cx, |p| p.engine.clear_drawings()),
            DemoAction::DrawingColor => self.update_root(cx, |p| {
                p.update_drawing_template(|t| {
                    t.color = if t.color == "#2962ff" { "#ff9800" } else { "#2962ff" }.into();
                });
            }),
            DemoAction::DrawingStyle => self.update_root(cx, |p| {
                p.update_drawing_template(|t| {
                    t.style = match t.style { "solid" => "dotted", "dotted" => "dashed", _ => "solid" };
                });
            }),
            DemoAction::DrawingWidth => self.update_root(cx, |p| {
                p.update_drawing_template(|t| t.width = if t.width >= 4 { 1 } else { t.width + 1 });
            }),
            DemoAction::DrawingText => self.update_root(cx, |p| {
                p.update_drawing_template(|t| t.text = if t.text == "Native" { "Nucleus native" } else { "Native" }.into());
            }),
            DemoAction::DrawingTextColor => self.update_root(cx, |p| {
                p.update_drawing_template(|t| t.text_color = if t.text_color == "#0a0a0a" { "#ab47bc" } else { "#0a0a0a" }.into());
            }),
            DemoAction::DrawingTextSize => self.update_root(cx, |p| {
                p.update_drawing_template(|t| t.text_size = if t.text_size >= 18 { 12 } else { t.text_size + 2 });
            }),
            DemoAction::DrawingTextWeight => self.update_root(cx, |p| {
                p.update_drawing_template(|t| t.text_weight = if t.text_weight >= 700 { 400 } else { t.text_weight + 100 });
            }),
            DemoAction::DrawingItalic => self.update_root(cx, |p| {
                p.update_drawing_template(|t| t.text_italic = !t.text_italic);
            }),
            DemoAction::CrosshairMode => self.update_root(cx, |p| {
                let next = match p.engine.options.get().crosshair.mode {
                    0 => 1,
                    1 => 3,
                    3 => 2,
                    _ => 0,
                };
                p.engine.options.apply_str(&format!(r#"{{"crosshair":{{"mode":{next}}}}}"#)).unwrap();
                p.engine.crosshair_mode = crosshair_mode_from_u8(next);
            }),
            DemoAction::CrosshairColor => self.update_root(cx, |p| {
                let current = &p.engine.options.get().crosshair.vert_line.color;
                let color = if current == nucleuscharts_core::style::DEFAULT_CROSSHAIR_CSS {
                    "#2962ff"
                } else {
                    nucleuscharts_core::style::DEFAULT_CROSSHAIR_CSS
                };
                p.engine.options.apply_str(&format!(r#"{{"crosshair":{{"vertLine":{{"color":"{color}"}},"horzLine":{{"color":"{color}"}}}}}}"#)).unwrap();
            }),
            DemoAction::CrosshairWidth => self.update_root(cx, |p| {
                let current = p.engine.options.get().crosshair.vert_line.width;
                let width = if current >= 4.0 { 1.0 } else { current + 1.0 };
                p.engine.options.apply_str(&format!(r#"{{"crosshair":{{"vertLine":{{"width":{width}}},"horzLine":{{"width":{width}}}}}}}"#)).unwrap();
            }),
            DemoAction::CrosshairStyle => self.update_root(cx, |p| {
                let style = (p.engine.options.get().crosshair.vert_line.style + 1) % 3;
                p.engine.options.apply_str(&format!(r#"{{"crosshair":{{"vertLine":{{"style":{style}}},"horzLine":{{"style":{style}}}}}}}"#)).unwrap();
            }),
            DemoAction::CrosshairLabelBackground => self.update_root(cx, |p| {
                let current = &p.engine.options.get().crosshair.vert_line.label_background_color;
                let color = if current == nucleuscharts_core::style::DEFAULT_CROSSHAIR_CSS {
                    "#2962ff"
                } else {
                    nucleuscharts_core::style::DEFAULT_CROSSHAIR_CSS
                };
                p.engine.options.apply_str(&format!(r#"{{"crosshair":{{"vertLine":{{"labelBackgroundColor":"{color}"}},"horzLine":{{"labelBackgroundColor":"{color}"}}}}}}"#)).unwrap();
            }),
            DemoAction::CrosshairLabels => self.update_root(cx, |p| {
                let visible = !p.engine.options.get().crosshair.vert_line.label_visible;
                p.engine.options.apply_str(&format!(r#"{{"crosshair":{{"vertLine":{{"labelVisible":{visible}}},"horzLine":{{"labelVisible":{visible}}}}}}}"#)).unwrap();
            }),
            DemoAction::Theme => {
                self.theme = if self.theme == DemoTheme::Light { DemoTheme::Dark } else { DemoTheme::Light };
                for cell in &self.cells {
                    let theme = self.theme;
                    cell.chart.update(cx, |p, child| { p.apply_theme(theme); child.notify(); });
                }
            }
            DemoAction::Grid => self.update_root(cx, |p| {
                let visible = !p.engine.options.get().grid.vert_lines.visible;
                p.engine.options.apply_str(&format!(r#"{{"grid":{{"vertLines":{{"visible":{visible}}},"horzLines":{{"visible":{visible}}}}}}}"#)).unwrap();
            }),
            DemoAction::GridColor => {
                let theme = self.theme;
                self.update_root(cx, move |p| p.toggle_grid_color_pin(theme));
            }
            DemoAction::GridStyle => self.update_root(cx, |p| {
                let style = (p.engine.options.get().grid.vert_lines.style + 1) % 3;
                p.engine.options.apply_str(&format!(r#"{{"grid":{{"vertLines":{{"style":{style}}},"horzLines":{{"style":{style}}}}}}}"#)).unwrap();
            }),
            DemoAction::Font => self.update_root(cx, |p| {
                let current = &p.engine.options.get().layout.font_family;
                let family = if current.contains("mono") {
                    "Georgia, serif"
                } else if current.contains("Georgia") {
                    "-apple-system, BlinkMacSystemFont, 'Trebuchet MS', Roboto, Ubuntu, sans-serif"
                } else {
                    "monospace"
                };
                p.engine.options.apply_str(&format!(r#"{{"layout":{{"fontFamily":"{family}"}}}}"#)).unwrap();
                p.renderer.invalidate_caches();
            }),
            DemoAction::FontSize => self.update_root(cx, |p| {
                let current = p.engine.options.get().layout.font_size;
                let size = if current >= 20.0 { 8.0 } else { current + 1.0 };
                p.engine.options.apply_str(&format!(r#"{{"layout":{{"fontSize":{size}}}}}"#)).unwrap();
                p.renderer.invalidate_caches();
            }),
            DemoAction::PriceLine => self.update_active(cx, |p| p.engine.series[0].price_line_visible = !p.engine.series[0].price_line_visible),
            DemoAction::PriceLineStyle => self.update_active(cx, |p| p.engine.series[0].price_line_style = (p.engine.series[0].price_line_style + 1) % 3),
            DemoAction::LastValue => self.update_active(cx, |p| p.engine.series[0].last_value_visible = !p.engine.series[0].last_value_visible),
            DemoAction::TitleVisible => self.update_active(cx, |p| p.engine.series[0].title_visible = !p.engine.series[0].title_visible),
            DemoAction::TitleText => self.update_active(cx, |p| { p.engine.series[0].title = if p.engine.series[0].title == "NUCLEUS" { "ASSET".into() } else { "NUCLEUS".into() }; }),
            DemoAction::Countdown => self.update_active(cx, |p| p.engine.series[0].countdown_visible = !p.engine.series[0].countdown_visible),
            DemoAction::BidAsk => self.update_active(cx, |p| { let on = !p.engine.series[0].bid_ask_visible; p.engine.series[0].bid_ask_visible = on; p.engine.set_bid_ask(0, on.then_some(107.95), on.then_some(108.05)); }),
            DemoAction::AxisBorders => self.update_root(cx, |p| { let on = !p.engine.options.get().time_scale.border_visible; p.engine.options.apply_str(&format!(r#"{{"leftPriceScale":{{"borderVisible":{on}}},"rightPriceScale":{{"borderVisible":{on}}},"timeScale":{{"borderVisible":{on}}}}}"#)).unwrap(); }),
            DemoAction::AxisBorderColor => {
                let theme = self.theme;
                self.update_root(cx, move |p| p.toggle_axis_border_pin(theme));
            }
            DemoAction::AxisText => {
                let theme = self.theme;
                self.update_root(cx, move |p| p.toggle_text_color_pin(theme));
            }
            DemoAction::Separator => {
                let theme = self.theme;
                self.update_root(cx, move |p| p.toggle_separator_pin(theme));
            }
            DemoAction::Watermark => self.update_root(cx, |p| {
                let watermark = &p.engine.options.get().watermark;
                let on = !watermark.visible;
                let text = if watermark.text.is_empty() { "NUCLEUS" } else { &watermark.text };
                let color = if watermark.color == "rgba(0, 0, 0, 0)" { "#b0b8c480" } else { &watermark.color };
                p.engine.options.apply_str(&format!(r#"{{"watermark":{{"visible":{on},"text":"{text}","color":"{color}"}}}}"#)).unwrap();
            }),
            DemoAction::WatermarkText => self.update_root(cx, |p| { let current = &p.engine.options.get().watermark.text; let text = if current == "NUCLEUS" { "@nucleuscharts/financial" } else { "NUCLEUS" }; p.engine.options.apply_str(&format!(r#"{{"watermark":{{"text":"{text}"}}}}"#)).unwrap(); }),
            DemoAction::WatermarkColor => self.update_root(cx, |p| { let current = &p.engine.options.get().watermark.color; let color = if current == "#b0b8c480" { "#2962ff80" } else { "#b0b8c480" }; p.engine.options.apply_str(&format!(r#"{{"watermark":{{"color":"{color}"}}}}"#)).unwrap(); }),
            DemoAction::WatermarkSize => self.update_root(cx, |p| { let current = p.engine.options.get().watermark.font_size; let size = if current >= 160.0 { 16.0 } else { current + 4.0 }; p.engine.options.apply_str(&format!(r#"{{"watermark":{{"fontSize":{size}}}}}"#)).unwrap(); }),
            DemoAction::AxisScaling => self.update_root(cx, |p| { p.gesture_config.axis_scale_price = !p.gesture_config.axis_scale_price; p.gesture_config.axis_scale_time = p.gesture_config.axis_scale_price; }),
            DemoAction::Kinetic => self.update_root(cx, |p| p.gesture_config.kinetic_mouse = !p.gesture_config.kinetic_mouse),
            DemoAction::Reset => self.update_root(cx, |p| { p.engine.reset_time_scale(); p.engine.fit_content(); for pane in 0..p.engine.panes.len() { p.engine.set_price_scale_auto_scale_for(pane, PriceScaleTarget::Left, true); p.engine.set_price_scale_auto_scale_for(pane, PriceScaleTarget::Right, true); } }),
            DemoAction::Fixture(index) => self.update_root(cx, move |p| match index {
                0 => p.fixtures.day_bands = !p.fixtures.day_bands,
                1 => p.fixtures.position_band = !p.fixtures.position_band,
                2 => p.fixtures.autoscale_band = !p.fixtures.autoscale_band,
                3 => p.fixtures.rounded_candles = !p.fixtures.rounded_candles,
                4 => p.toggle_markers(),
                5 => p.fixtures.plugin_watermark = !p.fixtures.plugin_watermark,
                _ => p.fixtures.vertical_line = !p.fixtures.vertical_line,
            }),
        }
        let chart_count = self.workspace.chart_count();
        if !matches!(
            action,
            DemoAction::Split(_) | DemoAction::Close | DemoAction::Cap
        ) {
            self.status = format!(
                "active {} · {} charts · {} splits",
                self.active,
                chart_count,
                chart_count.saturating_sub(1)
            );
        }
        cx.notify();
    }

    fn action_selected(&self, action: DemoAction, cx: &Context<Self>) -> bool {
        let active = self.active_chart();
        let root = self.root_chart();
        match action {
            DemoAction::Series(kind) => active.as_ref().is_some_and(|chart| {
                chart
                    .read(cx)
                    .engine
                    .series
                    .first()
                    .is_some_and(|series| series.kind == kind)
            }),
            DemoAction::CandleWicksVisible => active
                .as_ref()
                .is_some_and(|chart| chart.read(cx).engine.series[0].wick_visible.unwrap_or(true)),
            DemoAction::CandleBordersVisible => active.as_ref().is_some_and(|chart| {
                chart.read(cx).engine.series[0]
                    .border_visible
                    .unwrap_or(true)
            }),
            DemoAction::Sma => root
                .as_ref()
                .is_some_and(|chart| chart.read(cx).sma_id.is_some()),
            DemoAction::Volume => root
                .as_ref()
                .is_some_and(|chart| chart.read(cx).volume_id.is_some()),
            DemoAction::Rsi => root
                .as_ref()
                .is_some_and(|chart| chart.read(cx).rsi_id.is_some()),
            DemoAction::Cap => self.max_index != 0,
            DemoAction::Drawing(kind) => root
                .as_ref()
                .is_some_and(|chart| chart.read(cx).armed_tool == Some(kind)),
            DemoAction::DrawingItalic => root
                .as_ref()
                .is_some_and(|chart| chart.read(cx).drawing_template.text_italic),
            DemoAction::CrosshairLabels => root.as_ref().is_some_and(|chart| {
                chart
                    .read(cx)
                    .engine
                    .options
                    .get()
                    .crosshair
                    .vert_line
                    .label_visible
            }),
            DemoAction::Theme => self.theme == DemoTheme::Dark,
            DemoAction::Grid => root
                .as_ref()
                .is_some_and(|chart| chart.read(cx).engine.options.get().grid.vert_lines.visible),
            DemoAction::GridColor => root
                .as_ref()
                .is_some_and(|chart| chart.read(cx).style_pins.grid_color.is_some()),
            DemoAction::PriceLine => active
                .as_ref()
                .is_some_and(|chart| chart.read(cx).engine.series[0].price_line_visible),
            DemoAction::LastValue => active
                .as_ref()
                .is_some_and(|chart| chart.read(cx).engine.series[0].last_value_visible),
            DemoAction::TitleVisible => active
                .as_ref()
                .is_some_and(|chart| chart.read(cx).engine.series[0].title_visible),
            DemoAction::Countdown => active
                .as_ref()
                .is_some_and(|chart| chart.read(cx).engine.series[0].countdown_visible),
            DemoAction::BidAsk => active
                .as_ref()
                .is_some_and(|chart| chart.read(cx).engine.series[0].bid_ask_visible),
            DemoAction::AxisBorders => root.as_ref().is_some_and(|chart| {
                chart
                    .read(cx)
                    .engine
                    .options
                    .get()
                    .time_scale
                    .border_visible
            }),
            DemoAction::AxisBorderColor => root
                .as_ref()
                .is_some_and(|chart| chart.read(cx).style_pins.axis_border_color.is_some()),
            DemoAction::AxisText => root
                .as_ref()
                .is_some_and(|chart| chart.read(cx).style_pins.text_color.is_some()),
            DemoAction::Separator => root
                .as_ref()
                .is_some_and(|chart| chart.read(cx).style_pins.separator_color.is_some()),
            DemoAction::Watermark => root
                .as_ref()
                .is_some_and(|chart| chart.read(cx).engine.options.get().watermark.visible),
            DemoAction::AxisScaling => root
                .as_ref()
                .is_some_and(|chart| chart.read(cx).gesture_config.axis_scale_price),
            DemoAction::Kinetic => root
                .as_ref()
                .is_some_and(|chart| chart.read(cx).gesture_config.kinetic_mouse),
            DemoAction::Fixture(index) => root.as_ref().is_some_and(|chart| {
                let fixtures = chart.read(cx).fixtures;
                match index {
                    0 => fixtures.day_bands,
                    1 => fixtures.position_band,
                    2 => fixtures.autoscale_band,
                    3 => fixtures.rounded_candles,
                    4 => fixtures.markers,
                    5 => fixtures.plugin_watermark,
                    _ => fixtures.vertical_line,
                }
            }),
            _ => false,
        }
    }

    fn action_enabled(&self, action: DemoAction, cx: &Context<Self>) -> bool {
        let kind = self.active_chart().and_then(|chart| {
            chart
                .read(cx)
                .engine
                .series
                .first()
                .map(|series| series.kind)
        });
        match action {
            DemoAction::CandleBodyColor
            | DemoAction::CandleWickColor
            | DemoAction::CandleBorderColor
            | DemoAction::CandleWicksVisible
            | DemoAction::CandleBordersVisible
            | DemoAction::CandlePartsReset => {
                matches!(kind, Some(SeriesKind::Candlestick | SeriesKind::Bar))
            }
            DemoAction::LineColor | DemoAction::LineWidth => matches!(
                kind,
                Some(SeriesKind::Line | SeriesKind::Area | SeriesKind::Baseline)
            ),
            DemoAction::AreaColor => kind == Some(SeriesKind::Area),
            _ => true,
        }
    }

    fn button(
        &self,
        label: &'static str,
        action: DemoAction,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entity = cx.entity();
        let selected = self.action_selected(action, cx);
        let enabled = self.action_enabled(action, cx);
        let background = if selected {
            0x2962ff
        } else if self.theme == DemoTheme::Dark {
            0x16191f
        } else {
            0xffffff
        };
        let foreground = if selected || self.theme == DemoTheme::Dark {
            0xfafafa
        } else {
            0x191919
        };
        let border = if selected {
            0x2962ff
        } else if self.theme == DemoTheme::Dark {
            0x2b2f38
        } else {
            0xd0d3da
        };
        let control = div()
            .id(label)
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(border))
            .bg(rgb(background))
            .text_color(rgb(foreground))
            .child(label);
        if !enabled {
            return control.opacity(0.45).into_any_element();
        }
        control
            .cursor(CursorStyle::PointingHand)
            .hover(|style| style.bg(rgb(0x2962ff)).text_color(rgb(0xffffff)))
            .active(|style| style.bg(rgb(0x1849b8)).text_color(rgb(0xffffff)))
            .tab_index(0)
            .focus(|style| style.border_color(rgb(0xff9800)))
            .on_click(move |_, _, app| {
                entity.update(app, |demo, cx| demo.apply_action(action, cx));
            })
            .into_any_element()
    }

    fn group(&self, caption: &'static str, controls: Vec<AnyElement>) -> AnyElement {
        div()
            .id(caption)
            .relative()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_2()
            .pt_2()
            .pb_1()
            .mt_1()
            .border_1()
            .rounded_md()
            .border_color(rgb(if self.theme == DemoTheme::Dark {
                0x23262e
            } else {
                0xe0e3eb
            }))
            .child(
                div()
                    .absolute()
                    .top(px(-7.0))
                    .left(px(8.0))
                    .px_1()
                    .bg(rgb(if self.theme == DemoTheme::Dark {
                        0x0a0a0a
                    } else {
                        0xffffff
                    }))
                    .text_xs()
                    .text_color(rgb(if self.theme == DemoTheme::Dark {
                        0x9aa0ac
                    } else {
                        0x787b86
                    }))
                    .child(caption),
            )
            .children(controls)
            .into_any_element()
    }

    fn render_node(
        &self,
        node: &WorkspaceLayout,
        divider_color: u32,
        divider_line_width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            WorkspaceLayout::Cell { id } => {
                let chart = self
                    .cells
                    .iter()
                    .find(|cell| cell.id == *id)
                    .expect("workspace snapshots reference a live GPUI cell")
                    .chart
                    .clone();
                let activation_chart = chart.clone();
                let entity = cx.entity();
                let id = *id;
                div()
                    .relative()
                    .size_full()
                    .on_mouse_down(MouseButton::Left, move |event, _, app| {
                        entity.update(app, |demo, cx| {
                            let drawing_armed = activation_chart.read(cx).armed_tool.is_some();
                            if (event.modifiers.control || event.modifiers.platform)
                                && !drawing_armed
                            {
                                demo.toggle_maximize(id);
                            } else {
                                demo.activate(id, cx);
                            }
                            cx.notify();
                        });
                    })
                    .child(chart)
                    .into_any_element()
            }
            WorkspaceLayout::Split {
                direction,
                ratio,
                a,
                b,
            } => {
                let first = self.render_node(a, divider_color, divider_line_width, cx);
                let second = self.render_node(b, divider_color, divider_line_width, cx);
                let direction = *direction;
                let left = layout_last(a);
                let right = layout_first(b);
                let start_ratio = *ratio;
                let down_entity = cx.entity();
                let drag_handle = div()
                    .absolute()
                    .cursor(match direction {
                        SplitDirection::Horizontal => CursorStyle::ResizeLeftRight,
                        SplitDirection::Vertical => CursorStyle::ResizeRow,
                    })
                    .on_mouse_down(MouseButton::Left, move |event, window, app| {
                        let start: f32 = match direction {
                            SplitDirection::Horizontal => event.position.x.into(),
                            SplitDirection::Vertical => event.position.y.into(),
                        };
                        let viewport = window.viewport_size();
                        let fallback_extent = match direction {
                            SplitDirection::Horizontal => f32::from(viewport.width),
                            SplitDirection::Vertical => f32::from(viewport.height),
                        };
                        down_entity.update(app, |demo, cx| {
                            let layout = demo.workspace.layout();
                            let measured_extent = owning_split_total_extent(
                                &layout,
                                left,
                                right,
                                direction,
                                &|id, axis| {
                                    demo.cells
                                        .iter()
                                        .find(|cell| cell.id == id)
                                        .map(|cell| {
                                            let built_for = cell.chart.read(cx).built_for;
                                            match axis {
                                                SplitDirection::Horizontal => built_for.0,
                                                SplitDirection::Vertical => built_for.1,
                                            }
                                        })
                                        .unwrap_or(0.0)
                                },
                            )
                            .filter(|extent| *extent > 0.0)
                            .unwrap_or_else(|| fallback_extent.max(1.0));
                            demo.workspace_drag = Some(WorkspaceDrag {
                                left,
                                right,
                                direction,
                                start,
                                start_ratio,
                                current_ratio: start_ratio,
                                extent: measured_extent,
                            });
                            cx.notify();
                        });
                    });
                let hit_offset = (WORKSPACE_DIVIDER_LAYOUT_PX - WORKSPACE_DIVIDER_HIT_PX) / 2.0;
                let divider_line = canvas(
                    move |bounds: Bounds<gpui::Pixels>, _, _| bounds,
                    move |_, mut bounds: Bounds<gpui::Pixels>, window, _| {
                        let dpr = window.scale_factor().max(f32::EPSILON);
                        let device_width = (divider_line_width * dpr).round().max(1.0);
                        let logical_width = device_width / dpr;
                        match direction {
                            SplitDirection::Horizontal => {
                                let edge: f32 = bounds.origin.x.into();
                                let extent: f32 = bounds.size.width.into();
                                let aligned_device =
                                    ((edge + extent / 2.0) * dpr - device_width / 2.0 + 0.5)
                                        .floor();
                                bounds.origin.x = px(aligned_device / dpr);
                                bounds.size.width = px(logical_width);
                            }
                            SplitDirection::Vertical => {
                                let edge: f32 = bounds.origin.y.into();
                                let extent: f32 = bounds.size.height.into();
                                let aligned_device =
                                    ((edge + extent / 2.0) * dpr - device_width / 2.0 + 0.5)
                                        .floor();
                                bounds.origin.y = px(aligned_device / dpr);
                                bounds.size.height = px(logical_width);
                            }
                        }
                        window.paint_quad(gpui::fill(bounds, rgb(divider_color)));
                    },
                )
                .absolute()
                .size_full();
                let divider = match direction {
                    SplitDirection::Horizontal => div()
                        .relative()
                        .h_full()
                        .w(px(WORKSPACE_DIVIDER_LAYOUT_PX))
                        .flex_shrink_0()
                        .child(divider_line)
                        .child(
                            drag_handle
                                .left(px(hit_offset))
                                .h_full()
                                .w(px(WORKSPACE_DIVIDER_HIT_PX)),
                        ),
                    SplitDirection::Vertical => div()
                        .relative()
                        .w_full()
                        .h(px(WORKSPACE_DIVIDER_LAYOUT_PX))
                        .flex_shrink_0()
                        .child(divider_line)
                        .child(
                            drag_handle
                                .top(px(hit_offset))
                                .w_full()
                                .h(px(WORKSPACE_DIVIDER_HIT_PX)),
                        ),
                };
                let effective_ratio = self
                    .workspace_drag
                    .filter(|drag| {
                        drag.left == left && drag.right == right && drag.direction == direction
                    })
                    .map_or(*ratio, |drag| drag.current_ratio);
                let (first_ratio, second_ratio) = split_flex_ratios(effective_ratio);
                let base = div().flex().size_full();
                match direction {
                    SplitDirection::Horizontal => base
                        .flex_row()
                        .child(
                            div()
                                .h_full()
                                .flex_basis(relative(first_ratio))
                                .flex_shrink(1.0)
                                .child(first),
                        )
                        .child(divider)
                        .child(
                            div()
                                .h_full()
                                .flex_basis(relative(second_ratio))
                                .flex_shrink(1.0)
                                .child(second),
                        )
                        .into_any_element(),
                    SplitDirection::Vertical => base
                        .flex_col()
                        .child(
                            div()
                                .w_full()
                                .flex_basis(relative(first_ratio))
                                .flex_shrink(1.0)
                                .child(first),
                        )
                        .child(divider)
                        .child(
                            div()
                                .w_full()
                                .flex_basis(relative(second_ratio))
                                .flex_shrink(1.0)
                                .child(second),
                        )
                        .into_any_element(),
                }
            }
        }
    }
}

impl Render for InteractiveDemo {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus_initialized {
            if let Some(focus) = self
                .root_chart()
                .and_then(|chart| chart.read(cx).focus_handle.clone())
            {
                window.focus(&focus, cx);
                self.focus_initialized = true;
            }
        }
        let chart_count = self.workspace.chart_count();
        let mut b = |label, action| self.button(label, action, cx);
        let toolbar = vec![
            self.group(
                "Series",
                vec![
                    b("candles", DemoAction::Series(SeriesKind::Candlestick)),
                    b("bars", DemoAction::Series(SeriesKind::Bar)),
                    b("line", DemoAction::Series(SeriesKind::Line)),
                    b("area", DemoAction::Series(SeriesKind::Area)),
                    b("baseline", DemoAction::Series(SeriesKind::Baseline)),
                ],
            ),
            self.group(
                "Candle style",
                vec![
                    b("body colors", DemoAction::CandleBodyColor),
                    b("wick colors", DemoAction::CandleWickColor),
                    b("border colors", DemoAction::CandleBorderColor),
                    b("wicks", DemoAction::CandleWicksVisible),
                    b("borders", DemoAction::CandleBordersVisible),
                    b("reset parts", DemoAction::CandlePartsReset),
                ],
            ),
            self.group(
                "Line / area style",
                vec![
                    b("line color", DemoAction::LineColor),
                    b("line width", DemoAction::LineWidth),
                    b("area fill", DemoAction::AreaColor),
                ],
            ),
            self.group(
                "Indicators",
                vec![
                    b("SMA(20)", DemoAction::Sma),
                    b("volume overlay", DemoAction::Volume),
                    b("RSI(14) pane", DemoAction::Rsi),
                ],
            ),
            self.group(
                "Multi-chart",
                vec![
                    b("split ↔", DemoAction::Split(SplitDirection::Horizontal)),
                    b("split ↕", DemoAction::Split(SplitDirection::Vertical)),
                    b("close", DemoAction::Close),
                    b("max ∞/2/3/4", DemoAction::Cap),
                    div()
                        .text_xs()
                        .text_color(rgb(0x787b86))
                        .child(format!(
                            "{} chart{} · {} splits",
                            chart_count,
                            if chart_count == 1 { "" } else { "s" },
                            chart_count.saturating_sub(1)
                        ))
                        .into_any_element(),
                ],
            ),
            self.group(
                "Drawing tools",
                vec![
                    b("trend", DemoAction::Drawing(DrawingKind::TrendLine)),
                    b("h-line", DemoAction::Drawing(DrawingKind::HorizontalLine)),
                    b("h-ray", DemoAction::Drawing(DrawingKind::HorizontalRay)),
                    b("v-line", DemoAction::Drawing(DrawingKind::VerticalLine)),
                    b("rect", DemoAction::Drawing(DrawingKind::Rectangle)),
                    b("text", DemoAction::Drawing(DrawingKind::Text)),
                    b("brush", DemoAction::Drawing(DrawingKind::Brush)),
                    b("clear", DemoAction::ClearDrawings),
                ],
            ),
            self.group(
                "Drawing style",
                vec![
                    b("color", DemoAction::DrawingColor),
                    b("style", DemoAction::DrawingStyle),
                    b("width", DemoAction::DrawingWidth),
                    b("label", DemoAction::DrawingText),
                    b("text color", DemoAction::DrawingTextColor),
                    b("size", DemoAction::DrawingTextSize),
                    b("weight", DemoAction::DrawingTextWeight),
                    b("italic", DemoAction::DrawingItalic),
                ],
            ),
            self.group(
                "Crosshair",
                vec![
                    b("mode", DemoAction::CrosshairMode),
                    b("color", DemoAction::CrosshairColor),
                    b("width", DemoAction::CrosshairWidth),
                    b("style", DemoAction::CrosshairStyle),
                    b("label bg", DemoAction::CrosshairLabelBackground),
                    b("labels", DemoAction::CrosshairLabels),
                ],
            ),
            self.group(
                "Chart",
                vec![
                    b("light/dark", DemoAction::Theme),
                    b("grid on/off", DemoAction::Grid),
                    b("grid color follow/custom", DemoAction::GridColor),
                    b("grid style", DemoAction::GridStyle),
                    b("font family", DemoAction::Font),
                    b("font size", DemoAction::FontSize),
                ],
            ),
            self.group(
                "Series chrome",
                vec![
                    b("price line", DemoAction::PriceLine),
                    b("line style", DemoAction::PriceLineStyle),
                    b("last value", DemoAction::LastValue),
                    b("title chip", DemoAction::TitleVisible),
                    b("title text", DemoAction::TitleText),
                    b("countdown", DemoAction::Countdown),
                    b("bid/ask", DemoAction::BidAsk),
                ],
            ),
            self.group(
                "Axes",
                vec![
                    b("borders", DemoAction::AxisBorders),
                    b("border color", DemoAction::AxisBorderColor),
                    b("text color", DemoAction::AxisText),
                    b("separator", DemoAction::Separator),
                ],
            ),
            self.group(
                "Watermark",
                vec![
                    b("show", DemoAction::Watermark),
                    b("text", DemoAction::WatermarkText),
                    b("color", DemoAction::WatermarkColor),
                    b("size", DemoAction::WatermarkSize),
                ],
            ),
            self.group(
                "Interaction",
                vec![
                    b("axis scaling", DemoAction::AxisScaling),
                    b("mouse kinetic", DemoAction::Kinetic),
                    b("reset view", DemoAction::Reset),
                ],
            ),
            self.group(
                "Native visual approximations (no JS bridge)",
                vec![
                    b("day bands", DemoAction::Fixture(0)),
                    b("position band", DemoAction::Fixture(1)),
                    b("autoscale band", DemoAction::Fixture(2)),
                    b("rounded fixture", DemoAction::Fixture(3)),
                    b("markers", DemoAction::Fixture(4)),
                    b("plugin watermark", DemoAction::Fixture(5)),
                    b("vertical line", DemoAction::Fixture(6)),
                ],
            ),
        ];
        let (divider_color, legend) = self.root_chart().map_or_else(
            || {
                (
                    shell_rgb(theme_border(self.theme), 0xe5e5e5),
                    "O —  H —  L —  C —".to_string(),
                )
            },
            |chart| {
                let probe = chart.read(cx);
                // Live frame cost, so an open demo answers "how much FPS" without a finite probe.
                let frame_ms = probe.last.total_nanos() as f64 / 1.0e6;
                let fps = if frame_ms > 0.0 {
                    1.0e3 / frame_ms
                } else {
                    0.0
                };
                (
                    shell_rgb(
                        &probe.engine.options.get().time_scale.border_color,
                        shell_rgb(theme_border(self.theme), 0xe5e5e5),
                    ),
                    format!(
                        "{}  ·  {}  ·  {frame_ms:.1} ms ({fps:.0} fps)",
                        probe.legend, probe.click_status
                    ),
                )
            },
        );
        let dpr = window.scale_factor();
        let divider_line_width = dpr.floor().max(1.0) / dpr;
        let chart = if let Some(id) = self.maximized {
            self.render_node(
                &WorkspaceLayout::Cell { id },
                divider_color,
                divider_line_width,
                cx,
            )
        } else {
            let layout = self.workspace.layout();
            self.render_node(&layout, divider_color, divider_line_width, cx)
        };
        let move_entity = cx.entity();
        let up_entity = move_entity.clone();
        let up_out_entity = move_entity.clone();
        div()
            .id("interactive-demo-root")
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(if self.theme == DemoTheme::Dark {
                0x0a0a0a
            } else {
                0xffffff
            }))
            .text_color(rgb(if self.theme == DemoTheme::Dark {
                0xfafafa
            } else {
                0x191919
            }))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_move(move |event, _, app| {
                if event.dragging() {
                    move_entity.update(app, |demo, cx| {
                        if demo.workspace_drag.is_some() {
                            demo.drag_workspace_divider(event.position);
                            cx.notify();
                        }
                    });
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, app| {
                up_entity.update(app, |demo, cx| {
                    if demo.workspace_drag.is_some() {
                        demo.finish_workspace_drag();
                        cx.notify();
                    }
                });
            })
            .on_mouse_up_out(MouseButton::Left, move |_, _, app| {
                up_out_entity.update(app, |demo, cx| {
                    if demo.workspace_drag.is_some() {
                        demo.finish_workspace_drag();
                        cx.notify();
                    }
                });
            })
            .child(
                div()
                    .id("interactive-toolbar")
                    .tab_group()
                    .tab_stop(false)
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .p_2()
                    .flex_shrink_0()
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(rgb(0x089981))
                            .text_color(rgb(0xffffff))
                            .child("@nucleuscharts/financial"),
                    )
                    .children(toolbar)
                    .child(div().text_xs().text_color(rgb(0x787b86)).child(format!(
                            "{} · active {} · cap {}{}",
                            self.status,
                            self.active,
                            ["∞", "2", "3", "4"][self.max_index],
                            self.maximized
                                .map(|id| format!(" · cell {id} maximized"))
                                .unwrap_or_default(),
                        ))),
            )
            .child(
                div().relative().flex_1().w_full().child(chart).child(
                    div()
                        .absolute()
                        .top_2()
                        .left_2()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(rgb(if self.theme == DemoTheme::Dark {
                            0x0a0a0a
                        } else {
                            0xffffff
                        }))
                        .text_color(rgb(if self.theme == DemoTheme::Dark {
                            0xfafafa
                        } else {
                            0x191919
                        }))
                        .text_sm()
                        .child(legend),
                ),
            )
    }
}

enum AppRoot {
    Interactive(Entity<InteractiveDemo>),
    Finite(Entity<Probe>),
}

impl Render for AppRoot {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        match self {
            Self::Interactive(view) => view.clone().into_any_element(),
            Self::Finite(view) => view.clone().into_any_element(),
        }
    }
}

fn main() {
    let budget = std::env::var("NUCLEUSCHARTS_PROBE_FRAMES")
        .ok()
        .and_then(|v| v.parse::<u64>().ok());
    let bars = std::env::var("NUCLEUSCHARTS_PROBE_BARS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(if budget.is_none() {
            1_000usize
        } else {
            500usize
        });

    application().run(move |cx: &mut App| {
        let interactive = budget.is_none();
        let bounds = Bounds::centered(
            None,
            if interactive {
                size(px(1280.0), px(820.0))
            } else {
                size(px(1024.0), px(640.0))
            },
            cx,
        );
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |_, cx| {
                let root = if interactive {
                    AppRoot::Interactive(cx.new(|cx| InteractiveDemo::new(bars, cx)))
                } else {
                    AppRoot::Finite(cx.new(|cx| {
                        let mut probe = Probe::new(bars, budget);
                        probe.focus_handle = Some(cx.focus_handle());
                        probe
                    }))
                };
                cx.new(|_| root)
            },
        )
        .expect("the probe window opens");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_append_uses_the_latest_source_cadence() {
        let mut probe = Probe::new_interactive(3);
        let previous = *probe.source_bars.times.last().unwrap();
        probe.append_bar();
        assert_eq!(probe.source_bars.times.len(), 4);
        assert_eq!(*probe.source_bars.times.last().unwrap(), previous + 3_600.0);

        let mut hourly = vec![1_600_000_000.0, 1_600_003_600.0, 1_600_007_200.0];
        let first_append = next_bar_timestamp(&hourly);
        assert_eq!(first_append, 1_600_010_800.0);
        hourly.push(first_append);
        assert_eq!(next_bar_timestamp(&hourly), 1_600_014_400.0);

        assert_eq!(
            next_bar_timestamp(&[1_600_000_000.0, 1_600_000_060.0]),
            1_600_000_120.0,
            "the finite probe's minute cadence remains unchanged"
        );
    }

    #[test]
    fn resize_replaces_negotiated_pane_dimensions_at_fractional_dpr() {
        let mut probe = Probe::new(32, Some(1));
        let measure = |text: &str| text.chars().count() as f64 * 7.0;
        probe.rebuild_with_measure(1024.0, 640.0, 1.5, measure);
        let old_scissor = probe.frame.panes[0].scissor;

        probe.rebuild_with_measure(1536.0, 864.0, 1.5, measure);

        let content_h = 864.0 - probe.engine.time_axis_height();
        let pane_w = 1536.0 - probe.engine.left_axis_w - probe.engine.axis_w;
        assert_eq!(probe.engine.pane_w, pane_w);
        assert_eq!(probe.engine.pane_h, content_h);
        assert_eq!(
            probe.frame.width,
            probe.engine.pane_left + probe.engine.pane_w
        );
        assert_eq!(probe.frame.height, content_h);
        assert_eq!(probe.frame.pixel_ratio, 1.5);
        assert_eq!(
            probe.frame.panes[0].scissor,
            [
                (probe.engine.pane_left * 1.5).round() as u32,
                0,
                (pane_w * 1.5).round() as u32,
                (content_h * 1.5).round() as u32,
            ]
        );
        assert_ne!(probe.frame.panes[0].scissor, old_scissor);
        assert!(probe
            .axis
            .iter()
            .any(|prim| matches!(prim, Prim::Text { .. })));
    }

    #[test]
    fn desktop_gesture_defaults_match_browser_host() {
        let config = GestureConfig::default();

        assert!(config.pan);
        assert!(config.wheel_scroll);
        assert!(config.wheel_zoom);
        assert!(config.axis_dblclick_reset_time);
        assert!(config.axis_dblclick_reset_price);
        assert!(config.axis_scale_price);
        assert!(config.axis_scale_time);
        assert!(config.panes_resize);
        assert!(!config.kinetic_mouse);
    }

    #[test]
    fn click_after_pan_preserves_position_and_next_pan_uses_current_snapshot() {
        let mut probe = Probe::new(64, Some(1));
        probe.rebuild_with_measure(1024.0, 640.0, 1.0, |text| text.chars().count() as f64 * 7.0);
        probe.engine.scroll_to_position(0.0);
        let initial = probe.engine.scroll_position();

        probe.begin_mouse_pan(200.0);
        probe.engine.time_scale_scroll_to(160.0);
        probe.end_mouse_pan(160.0);
        let after_first_pan = probe.engine.scroll_position();
        let first_delta = after_first_pan - initial;
        assert!(first_delta > 0.0);

        // A press/release below click slop still enters the pane pan recognizer, but must close its
        // snapshot without restoring the position reached by the previous drag.
        probe.cancel_kinetic_scroll();
        probe.begin_mouse_pan(120.0);
        probe.end_mouse_pan(120.0);
        let after_click = probe.engine.scroll_position();
        assert!((after_click - after_first_pan).abs() < 1e-12);

        // A fresh 20px drag must start at `after_click`, rather than reusing the first drag's saved
        // state. It therefore moves half as far as the first 40px drag at unchanged bar spacing.
        probe.begin_mouse_pan(120.0);
        probe.engine.time_scale_scroll_to(100.0);
        probe.end_mouse_pan(100.0);
        let after_second_pan = probe.engine.scroll_position();
        let expected = after_click + first_delta / 2.0;
        assert!((after_second_pan - expected).abs() < 1e-12);
    }

    fn assert_theme(engine: &ChartEngine, theme: DemoTheme) {
        let options = engine.options.get();
        let background = theme.surface();
        let border = theme_border(theme);
        let text = theme_text(theme);
        assert_eq!(options.layout.background.color, background);
        assert_eq!(options.layout.text_color, text);
        assert_eq!(options.left_price_scale.border_color, border);
        assert_eq!(options.right_price_scale.border_color, border);
        assert_eq!(options.time_scale.border_color, border);
        assert_eq!(options.grid.vert_lines.color, border);
        assert_eq!(options.grid.horz_lines.color, border);
        assert_eq!(options.layout.panes.separator_color, border);
        assert_eq!(options.crosshair.vert_line.color, theme.crosshair());
        assert_eq!(
            options.crosshair.horz_line.label_background_color,
            theme.crosshair()
        );
    }

    #[test]
    fn gpui_package_themes_use_exact_tokens_on_every_axis() {
        let mut probe = Probe::new(32, Some(1));
        assert_theme(&probe.engine, DemoTheme::Light);
        probe.apply_theme(DemoTheme::Dark);
        assert_theme(&probe.engine, DemoTheme::Dark);
    }

    #[test]
    fn toolbar_manifest_covers_the_full_native_demo_surface() {
        let manifest = TOOLBAR_FEATURE_MANIFEST.join("|");
        for required in [
            "candlestick",
            "baseline",
            "sma20",
            "rsi14",
            "split-horizontal",
            "resize",
            "brush",
            "text-color",
            "crosshair",
            "price-line",
            "bid-ask",
            "separator",
            "mouse-kinetic",
            "rounded-candles",
            "plugin-watermark",
            "vertical-line",
        ] {
            assert!(
                manifest.contains(required),
                "missing toolbar feature {required}"
            );
        }
        assert!(!GestureConfig::default().kinetic_mouse);
    }

    #[test]
    fn drawing_template_controls_compose_without_restarting_creation() {
        let mut probe = Probe::new(64, Some(1));
        probe.rebuild_with_measure(1024.0, 640.0, 1.0, |text| text.chars().count() as f64 * 7.0);
        probe.engine.clear_drawings();
        probe.arm_drawing(DrawingKind::TrendLine);
        assert_eq!(
            probe
                .engine
                .drawing_create_click(200.0, 180.0, DrawingModifiers::default()),
            -1
        );
        probe.update_drawing_template(|template| {
            template.color = "#ff9800".into();
            template.width = 4;
            template.text_italic = true;
        });
        assert!(probe.engine.drawing_create_active());
        let id = probe
            .engine
            .drawing_create_click(500.0, 300.0, DrawingModifiers::default());
        assert!(id > 0, "changing style must not discard the first anchor");
        assert_eq!(probe.drawing_template.color, "#ff9800");
        assert_eq!(probe.drawing_template.width, 4);
        assert!(probe.drawing_template.text_italic);
    }

    #[test]
    fn interactive_drawing_creation_commits_and_selects() {
        let mut probe = Probe::new(64, Some(1));
        probe.rebuild_with_measure(1024.0, 640.0, 1.0, |text| text.chars().count() as f64 * 7.0);
        probe.engine.clear_drawings();
        probe.arm_drawing(DrawingKind::TrendLine);
        assert!(probe.engine.drawing_create_active());
        assert_eq!(
            probe
                .engine
                .drawing_create_click(200.0, 180.0, DrawingModifiers::default()),
            -1
        );
        let id = probe
            .engine
            .drawing_create_click(500.0, 300.0, DrawingModifiers::default());
        assert!(id > 0);
        assert_eq!(probe.engine.drawings().len(), 1);
        assert_eq!(probe.engine.selected_drawing(), Some(id as u32));
    }
}

#[cfg(test)]
mod semantic_regressions {
    use super::*;

    #[test]
    fn pending_template_patch_reaches_committed_drawing() {
        let mut probe = Probe::new(64, Some(1));
        probe.rebuild_with_measure(1024.0, 640.0, 1.0, |text| text.chars().count() as f64 * 7.0);
        probe.engine.clear_drawings();
        probe.arm_drawing(DrawingKind::TrendLine);
        assert_eq!(
            probe
                .engine
                .drawing_create_click(200.0, 180.0, DrawingModifiers::default()),
            -1
        );
        probe.update_drawing_template(|template| {
            template.color = "#ff9800".into();
            template.width = 4;
            template.text_italic = true;
        });
        let id = probe
            .engine
            .drawing_create_click(500.0, 300.0, DrawingModifiers::default());
        assert!(id > 0);
        let options = probe.engine.drawing_options_json(id as u32).unwrap();
        assert!(options.contains("\"color\":\"#ff9800\""));
        assert!(options.contains(r#""width":4.0"#));
        assert!(options.contains(r#""text_italic":true"#));
    }

    #[test]
    fn selected_template_patch_preserves_unrelated_options() {
        let mut probe = Probe::new(64, Some(1));
        let id = probe
            .engine
            .add_drawing(
                DrawingKind::TrendLine,
                0,
                vec![
                    DrawingPoint {
                        logical: 4.0,
                        price: 100.0,
                    },
                    DrawingPoint {
                        logical: 8.0,
                        price: 105.0,
                    },
                ],
                Some(r##"{"color":"#123456","width":7,"text":"keep me"}"##),
            )
            .unwrap();
        probe.engine.set_selected_drawing(Some(id));
        probe.update_drawing_template(|template| template.text_italic = true);
        let options = probe.engine.drawing_options_json(id).unwrap();
        assert!(options.contains("\"color\":\"#123456\""));
        assert!(options.contains(r#""width":7.0"#));
        assert!(options.contains(r#""text":"keep me""#));
        assert!(options.contains(r#""text_italic":true"#));
    }

    #[test]
    fn theme_changes_preserve_pinned_styles_and_follow_unpinned_styles() {
        let mut probe = Probe::new(32, Some(1));
        probe.toggle_axis_border_pin(DemoTheme::Light);
        probe.toggle_text_color_pin(DemoTheme::Light);
        probe.apply_theme(DemoTheme::Dark);
        assert_eq!(
            probe.engine.options.get().time_scale.border_color,
            "#2962ff"
        );
        assert_eq!(probe.engine.options.get().layout.text_color, "#ab47bc");

        probe.toggle_axis_border_pin(DemoTheme::Dark);
        probe.toggle_text_color_pin(DemoTheme::Dark);
        assert_eq!(
            probe.engine.options.get().time_scale.border_color,
            nucleuscharts_core::style::DARK_BORDER_CSS
        );
        assert_eq!(
            probe.engine.options.get().layout.text_color,
            nucleuscharts_core::style::DARK_AXIS_TEXT_CSS
        );
        probe.apply_theme(DemoTheme::Light);
        assert_eq!(
            probe.engine.options.get().time_scale.border_color,
            nucleuscharts_core::style::LIGHT_BORDER_CSS
        );
        assert_eq!(
            probe.engine.options.get().layout.text_color,
            nucleuscharts_core::style::LIGHT_AXIS_TEXT_CSS
        );
    }

    #[test]
    fn interactive_metrics_are_bounded_while_finite_metrics_are_retained() {
        let mut interactive = Probe::new(8, None);
        for _ in 0..10_000 {
            interactive.record_frame_metrics(GpuiFrameMetrics::default());
        }
        assert_eq!(interactive.painted, 10_000);
        assert!(interactive.plan_nanos.is_empty());
        assert!(interactive.paint_nanos.is_empty());
        assert!(interactive.total_nanos.is_empty());

        let mut finite = Probe::new(8, Some(2));
        finite.record_frame_metrics(GpuiFrameMetrics::default());
        finite.record_frame_metrics(GpuiFrameMetrics::default());
        assert_eq!(finite.plan_nanos.len(), 2);
        assert_eq!(finite.paint_nanos.len(), 2);
        assert_eq!(finite.total_nanos.len(), 2);
    }

    #[test]
    fn idle_interactive_probe_stops_requesting_frames() {
        let mut interactive = Probe::new(8, None);
        assert!(!interactive.needs_animation_frame());
        interactive.kinetic_active = true;
        assert!(interactive.needs_animation_frame());
        interactive.kinetic_active = false;
        interactive.engine.start_scroll_animation(3.0, 160.0, 0.0);
        assert!(interactive.needs_animation_frame());
        assert!(Probe::new(8, Some(2)).needs_animation_frame());
    }

    #[test]
    fn finite_live_append_occurs_once_per_epoch_before_the_frame_budget() {
        let mut probe = Probe::new(8, Some(120));
        probe.painted = 59;
        probe.maybe_append_live_bar();
        assert_eq!(probe.appended, 0);
        probe.painted = 60;
        probe.maybe_append_live_bar();
        probe.maybe_append_live_bar();
        assert_eq!(probe.appended, 1);
        probe.painted = 120;
        probe.maybe_append_live_bar();
        assert_eq!(probe.appended, 1);
    }

    #[test]
    fn nested_workspace_split_extents_use_the_owning_browser_grid_space() {
        let horizontal = WorkspaceLayout::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            a: Box::new(WorkspaceLayout::Cell { id: 1 }),
            b: Box::new(WorkspaceLayout::Split {
                direction: SplitDirection::Horizontal,
                ratio: 0.5,
                a: Box::new(WorkspaceLayout::Cell { id: 2 }),
                b: Box::new(WorkspaceLayout::Cell { id: 3 }),
            }),
        };
        let width = |id, _| match id {
            1 => 100.0,
            2 => 200.0,
            _ => 300.0,
        };
        assert_eq!(
            owning_split_total_extent(&horizontal, 1, 2, SplitDirection::Horizontal, &width),
            Some(602.0)
        );
        assert_eq!(
            owning_split_total_extent(&horizontal, 2, 3, SplitDirection::Horizontal, &width),
            Some(501.0)
        );

        let vertical = WorkspaceLayout::Split {
            direction: SplitDirection::Vertical,
            ratio: 0.5,
            a: Box::new(WorkspaceLayout::Cell { id: 1 }),
            b: Box::new(WorkspaceLayout::Split {
                direction: SplitDirection::Vertical,
                ratio: 0.5,
                a: Box::new(WorkspaceLayout::Cell { id: 2 }),
                b: Box::new(WorkspaceLayout::Cell { id: 3 }),
            }),
        };
        let height = |id, _| match id {
            1 => 80.0,
            2 => 120.0,
            _ => 160.0,
        };
        assert_eq!(
            owning_split_total_extent(&vertical, 1, 2, SplitDirection::Vertical, &height),
            Some(362.0)
        );
        assert_eq!(
            owning_split_total_extent(&vertical, 2, 3, SplitDirection::Vertical, &height),
            Some(281.0)
        );
    }

    #[test]
    fn split_flex_ratios_preserve_the_model_ratio_after_the_fixed_divider() {
        let (first, second) = split_flex_ratios(0.35);
        assert!((first - 0.35).abs() < f32::EPSILON);
        assert!((second - 0.65).abs() < f32::EPSILON);
        let content_extent = 1_000.0 - WORKSPACE_DIVIDER_LAYOUT_PX;
        assert!(
            (content_extent * first / (content_extent * (first + second)) - 0.35).abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn workspace_snapshot_helpers_follow_authoritative_layout() {
        let mut workspace = Workspace::new();
        let second = workspace.split(1, SplitDirection::Horizontal).unwrap();
        let third = workspace.split(second, SplitDirection::Vertical).unwrap();
        let layout = workspace.layout();
        assert_eq!(layout_first(&layout), 1);
        assert_eq!(layout_last(&layout), third);
        workspace.resize_between(second, third, 0.2).unwrap();
        workspace.remove(second).unwrap();
        let layout = workspace.layout();
        assert_eq!(layout_first(&layout), 1);
        assert_eq!(layout_last(&layout), third);
        assert_eq!(workspace.cell_ids(), [1, third]);
    }
}
