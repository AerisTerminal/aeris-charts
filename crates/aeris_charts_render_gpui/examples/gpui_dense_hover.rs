//! Real-window measurement of a crosshair sweep over a zoomed-out dense chart.
//!
//! A host chart that shows thousands of bars with several indicator lines must keep crosshair
//! movement inside one display frame. `plan_bench` bounds the GPUI-free lowering of the same
//! fixture; this example adds what only a real window shows: GPUI scene submission, path
//! rasterization, and present. Every frame moves the crosshair, rebuilds the engine frame, and
//! paints it, exactly as a GPUI host does for pointer motion.
//!
//! ```text
//! cargo run --release -p aeris_charts_render_gpui --features gpui-backend --example gpui_dense_hover
//! ```
//!
//! Environment knobs:
//! - `AERIS_CHARTS_DENSE_BARS` — loaded bars (default 4000).
//! - `AERIS_CHARTS_DENSE_FRAMES` — measured frames after warm-up (default 240).
//! - `AERIS_CHARTS_DENSE_VISIBLE` — visible bars at the right edge (default: all).
//!
//! The frame interval includes vsync, so it cannot drop below the display refresh period; a value
//! above that period means the chart missed frames.

use std::time::Instant;

use aeris_charts_engine::{ChartEngine, ChartFrame, FinancialFrameRequest, PointerInput};
use aeris_charts_render::draw_list::Prim;
use aeris_charts_render_gpui::{
    AerisViewport, GpuiChartRenderer, PreparedAerisFrame, backend::measure_text,
    fixtures::dense_indicator_chart, input::install_text_metrics,
};
use gpui::{
    App, Bounds, Context, Render, Window, WindowBounds, WindowOptions, canvas, div, prelude::*, px,
    size,
};
use gpui_platform::application;

const WARM_UP_FRAMES: usize = 30;
const CSS_W: f32 = 1600.0;
const CSS_H: f32 = 900.0;

#[derive(Clone, Copy, Default)]
struct Sample {
    engine_ns: u64,
    plan_ns: u64,
    paint_ns: u64,
    interval_ns: u64,
}

struct DenseHover {
    engine: ChartEngine,
    renderer: GpuiChartRenderer,
    frame: ChartFrame,
    axis: Vec<Prim>,
    bars: usize,
    visible: usize,
    budget: usize,
    built_for: (f32, f32, f32),
    tick: u32,
    engine_ns: u64,
    last_paint: Option<Instant>,
    samples: Vec<Sample>,
    vertices: u32,
    ops: u32,
}

fn env_usize(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
}

impl DenseHover {
    fn new() -> Self {
        let bars = env_usize("AERIS_CHARTS_DENSE_BARS").unwrap_or(4_000).max(2);
        let visible = env_usize("AERIS_CHARTS_DENSE_VISIBLE")
            .unwrap_or(bars)
            .clamp(2, bars);
        let budget = env_usize("AERIS_CHARTS_DENSE_FRAMES").unwrap_or(240).max(1);
        Self {
            engine: dense_indicator_chart(bars, f64::from(CSS_W), f64::from(CSS_H), 1.0),
            renderer: GpuiChartRenderer::new(),
            frame: ChartFrame::default(),
            axis: Vec::new(),
            bars,
            visible,
            budget,
            built_for: (0.0, 0.0, 0.0),
            tick: 0,
            engine_ns: 0,
            last_paint: None,
            samples: Vec::with_capacity(budget),
            vertices: 0,
            ops: 0,
        }
    }

    fn prepare(&mut self, bounds: Bounds<gpui::Pixels>, window: &Window) {
        let width: f32 = bounds.size.width.into();
        let height: f32 = bounds.size.height.into();
        let scale_factor = window.scale_factor();
        let resized = self.built_for != (width, height, scale_factor);
        install_text_metrics(&mut self.engine, window);
        let started = Instant::now();
        if !resized {
            let x = 40.0 + f64::from(self.tick % 200) / 200.0 * (self.engine.pane_w - 80.0);
            let y = self.engine.pane_h * (0.3 + 0.1 * f64::from(self.tick % 5));
            self.engine.input_pointer_move(
                PointerInput {
                    x,
                    y,
                    ..PointerInput::default()
                },
                false,
            );
            self.engine.take_input_events();
            self.tick += 1;
        }
        let family = self.engine.options.get().layout.font_family.clone();
        let axis_size = self.engine.axis_font_size() as f32;
        let countdown_size = self.engine.countdown_font_size() as f32;
        let family = family.as_str();
        let measure = |size: f32| {
            move |text: &str, bold: bool| {
                f64::from(
                    measure_text(
                        window,
                        text,
                        family,
                        size,
                        if bold { 700 } else { 400 },
                        false,
                    )
                    .width,
                )
            }
        };
        self.engine.prepare_financial_frame_with_measure(
            FinancialFrameRequest {
                width: f64::from(width),
                height: f64::from(height),
                dpr: f64::from(scale_factor),
                force_layout: resized,
                allow_axis_shrink: resized,
                force_frame: false,
                force_axis: false,
                layout_only: false,
                fit_content: false,
                frame: &mut self.frame,
                axis_frame: None,
                axis_primitives: Some(&mut self.axis),
            },
            measure(axis_size),
            measure(countdown_size),
        );
        if resized {
            self.built_for = (width, height, scale_factor);
            let right = (self.bars - 1) as f64;
            self.engine
                .set_visible_logical_range(right - (self.visible - 1) as f64, right);
        }
        self.engine_ns = started.elapsed().as_nanos() as u64;
    }

    fn paint(&mut self, bounds: Bounds<gpui::Pixels>, window: &mut Window, cx: &mut App) {
        let viewport = AerisViewport::from_bounds(
            bounds.origin.x.into(),
            bounds.origin.y.into(),
            bounds.size.width.into(),
            bounds.size.height.into(),
        );
        let prepared =
            PreparedAerisFrame::from_engine(&self.frame, &self.engine).with_axis(&self.axis, &[]);
        let metrics =
            match self
                .renderer
                .paint_frame(&prepared, viewport, window.scale_factor(), window, cx)
            {
                Ok(metrics) => metrics,
                Err(error) => {
                    eprintln!("frame skipped: {error}");
                    return;
                }
            };
        let now = Instant::now();
        let interval_ns = self
            .last_paint
            .map_or(0, |last| now.duration_since(last).as_nanos() as u64);
        self.last_paint = Some(now);
        self.vertices = metrics.mesh_vertices;
        self.ops = metrics.ops;
        if self.tick as usize > WARM_UP_FRAMES {
            self.samples.push(Sample {
                engine_ns: self.engine_ns,
                plan_ns: metrics.plan_nanos,
                paint_ns: metrics.paint_nanos,
                interval_ns,
            });
        }
    }

    fn report(&self) {
        let column = |pick: fn(&Sample) -> u64| {
            let mut values: Vec<u64> = self.samples.iter().map(pick).collect();
            values.sort_unstable();
            let at = |p: f64| {
                values
                    .get(((values.len().saturating_sub(1)) as f64 * p).round() as usize)
                    .map_or(0.0, |v| *v as f64 / 1e6)
            };
            format!(
                "p50 {:>6.2} ms  p95 {:>6.2} ms  p99 {:>6.2} ms",
                at(0.5),
                at(0.95),
                at(0.99)
            )
        };
        println!(
            "dense hover: {} bars, {} visible, {} frames, {} ops, {} mesh vertices",
            self.bars,
            self.visible,
            self.samples.len(),
            self.ops,
            self.vertices
        );
        println!("  engine frame   {}", column(|s| s.engine_ns));
        println!("  gpui lowering  {}", column(|s| s.plan_ns));
        println!("  gpui paint     {}", column(|s| s.paint_ns));
        println!("  frame interval {}", column(|s| s.interval_ns));
    }
}

impl Render for DenseHover {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.samples.len() >= self.budget {
            self.report();
            cx.quit();
        } else {
            window.request_animation_frame();
        }
        let prepaint = cx.entity();
        let paint = cx.entity();
        div().size_full().child(
            canvas(
                move |bounds, window, cx| {
                    prepaint.update(cx, |bench: &mut DenseHover, _| {
                        bench.prepare(bounds, window)
                    });
                    bounds
                },
                move |_, bounds, window, cx| {
                    paint.update(cx, |bench: &mut DenseHover, cx| {
                        bench.paint(bounds, window, cx)
                    });
                },
            )
            .size_full(),
        )
    }
}

fn main() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(CSS_W), px(CSS_H)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("Aeris Charts — dense hover measurement");
                cx.new(|_| DenseHover::new())
            },
        )
        .expect("the measurement window opens");
        cx.activate(true);
    });
}
