//! Scene-construction benchmark for `aeris_charts_render_gpui`.
//!
//! Measures the adapter's lowering pass — `Prim` stream to [`ScenePlan`] — which is the
//! "GPUI scene-construction overhead" the performance gate bounds. It deliberately needs **no**
//! GPUI feature, window, or GPU, so it runs anywhere and on every CI target:
//!
//! ```text
//! cargo run --release -p aeris_charts_render_gpui --example plan_bench
//! ```
//!
//! The GPUI-side cost of issuing the plan (`paint_frame`'s second half) needs a real window and is
//! measured by `examples/gpui_probe.rs` instead.
//!
//! Environment knobs:
//! - `AERIS_CHARTS_BENCH_ITERS` — timed iterations per fixture (default 200).
//! - `AERIS_CHARTS_BENCH_POINTS` — comma-separated source-point counts (default `10000,100000,1000000`).

use std::time::Instant;

use aeris_charts_engine::{
    AggressorSide, ChartEngine, FootprintAggregationOptions, FootprintBarAggregation,
    FootprintSeriesOptions, FootprintTrade, SeriesKind,
};
use aeris_charts_render_gpui::{GpuiChartRenderer, PreparedAerisFrame};

/// Chart size and DPR the gate is quoted at: a dense visible range on a fractional DPR.
const CSS_W: f64 = 1600.0;
const CSS_H: f64 = 900.0;
const DPR: f64 = 1.5;

/// The scene-construction performance target.
const P99_BUDGET_MS: f64 = 2.0;

struct Fixture {
    engine: ChartEngine,
    source_points: usize,
}

fn build_fixture(source_points: usize) -> Fixture {
    let mut engine = ChartEngine::new(CSS_W, CSS_H, DPR);
    let n = source_points;
    let mut times = Vec::with_capacity(n);
    let mut open = Vec::with_capacity(n);
    let mut high = Vec::with_capacity(n);
    let mut low = Vec::with_capacity(n);
    let mut close = Vec::with_capacity(n);
    let mut price = 100.0f64;
    for i in 0..n {
        let t = i as f64;
        let c = 100.0 + (t * 0.013).sin() * 18.0 + (t * 0.0007).cos() * 40.0;
        times.push(1_600_000_000.0 + t * 60.0);
        open.push(price);
        high.push(price.max(c) + 1.0);
        low.push(price.min(c) - 1.0);
        close.push(c);
        price = c;
    }
    engine
        .set_series_data(0, &times, &open, &high, &low, &close)
        .expect("fixture loads");
    engine.series[0].kind = SeriesKind::Candlestick;

    // A line and an overlay histogram, so the tessellated route is exercised too.
    // `AERIS_CHARTS_BENCH_QUADS_ONLY=1` drops the line series, which isolates how much of the
    // scene-construction time is polyline tessellation versus quad emission.
    if std::env::var("AERIS_CHARTS_BENCH_QUADS_ONLY").as_deref() != Ok("1") {
        let line = engine.add_series(SeriesKind::Line);
        engine
            .set_series_data(line, &times, &close, &close, &close, &close)
            .expect("line loads");
    }
    let hist = engine.add_series(SeriesKind::Histogram);
    engine
        .set_series_data(hist, &times, &close, &close, &close, &close)
        .expect("histogram loads");

    engine.css_width = CSS_W;
    engine.css_height = CSS_H;
    engine.dpr = DPR;
    let content_h = (CSS_H - engine.time_axis_height()).max(1.0);
    engine.layout_panes(content_h);
    engine.time_scale.set_width(CSS_W);
    engine.fit_content();
    engine.crosshair = Some((CSS_W / 2.0, CSS_H / 2.0));

    Fixture {
        engine,
        source_points,
    }
}

/// A deliberately dense numbers-bar fixture: 20 visible one-minute bars, eleven price levels,
/// and both bid/ask prints at every level. The spacing and tick size keep the renderer in its
/// detailed LOD so every cell emits the same text primitives that a live order-flow chart does.
fn build_dense_footprint_fixture() -> ChartEngine {
    const BARS: usize = 20;
    const LEVELS: usize = 11;
    const TICK_SIZE: f64 = 0.25;
    const INTERVAL_MICROS: i64 = 60_000_000;
    let mut engine = ChartEngine::new(CSS_W, CSS_H, DPR);
    let footprint = engine
        .add_footprint_series(FootprintSeriesOptions {
            aggregation: FootprintAggregationOptions {
                tick_size: TICK_SIZE,
                bars: FootprintBarAggregation::Time {
                    interval_micros: INTERVAL_MICROS as u64,
                    anchor_micros: 1_600_000_000_000_000,
                },
                ..FootprintAggregationOptions::default()
            },
            visual: Default::default(),
        })
        .expect("dense footprint options are valid");
    engine.set_series_visible(0, false);
    engine.set_bar_spacing(72.0);
    engine.set_right_offset(0.0);

    let first = 1_600_000_000_000_000i64;
    let mut trades = Vec::with_capacity(BARS * LEVELS * 2);
    let mut trade_id = 1u64;
    for bar in 0..BARS {
        let center = 400 + (bar as i64 % 3) - 1;
        for level in 0..LEVELS {
            let price = (center + level as i64 - LEVELS as i64 / 2) as f64 * TICK_SIZE;
            let timestamp = first + bar as i64 * INTERVAL_MICROS + level as i64 * 10_000;
            let volume = 10.0 + (level % 7) as f64 * 3.0;
            for (offset, aggressor) in [(0usize, AggressorSide::Sell), (1, AggressorSide::Buy)] {
                trades.push(FootprintTrade {
                    timestamp_micros: timestamp + offset as i64,
                    price,
                    volume,
                    aggressor,
                    bid: None,
                    ask: None,
                    sequence: Some((level * 2 + offset) as u64),
                    trade_id: Some(trade_id),
                    conditions: 0,
                    session_id: Some(1),
                });
                trade_id += 1;
            }
        }
    }
    engine
        .set_footprint_trades(footprint, trades)
        .expect("dense footprint trades are valid");
    engine.css_width = CSS_W;
    engine.css_height = CSS_H;
    engine.dpr = DPR;
    let content_h = (CSS_H - engine.time_axis_height()).max(1.0);
    engine.layout_panes(content_h);
    engine.time_scale.set_width(CSS_W);
    engine.fit_content();
    // Re-assert the detail spacing after fit_content, which intentionally chooses a compact view.
    engine.set_bar_spacing(72.0);
    engine.set_right_offset(0.0);
    engine.crosshair = Some((CSS_W / 2.0, CSS_H / 2.0));
    engine
}

fn percentile(sorted: &[u64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx] as f64 / 1_000_000.0
}

fn main() {
    let iters: usize = std::env::var("AERIS_CHARTS_BENCH_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let counts: Vec<usize> = std::env::var("AERIS_CHARTS_BENCH_POINTS")
        .ok()
        .map(|v| v.split(',').filter_map(|p| p.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![10_000, 100_000, 1_000_000]);

    println!(
        "aeris_charts_render_gpui scene-construction bench — {CSS_W}x{CSS_H} @ DPR {DPR}, {iters} iters/fixture"
    );
    println!("budget: p99 <= {P99_BUDGET_MS} ms\n");
    println!(
        "{:>10}  {:>7}  {:>6}  {:>6}  {:>7}  {:>7}  {:>7}  {:>7}  {:>8}  {:>6}",
        "src pts",
        "prims",
        "quads",
        "paths",
        "p50 ms",
        "p95 ms",
        "p99 ms",
        "max ms",
        "verts",
        "gate"
    );

    let mut all_pass = true;
    for count in counts {
        let mut fixture = build_fixture(count);
        let frame = fixture.engine.build_frame();
        let prepared = PreparedAerisFrame::new(&frame);
        let mut renderer = GpuiChartRenderer::new();

        // Warm up so the first-frame allocation of the plan and vertex pool is not measured as
        // steady state (the pool is reused across frames by design).
        let first = renderer
            .plan_frame(&prepared, DPR as f32)
            .expect("frame plans");
        for _ in 0..10 {
            renderer.plan_frame(&prepared, DPR as f32).unwrap();
        }

        let mut samples = Vec::with_capacity(iters);
        let mut steady_verts = 0u32;
        for _ in 0..iters {
            let started = Instant::now();
            let m = renderer.plan_frame(&prepared, DPR as f32).unwrap();
            samples.push(started.elapsed().as_nanos() as u64);
            steady_verts = m.mesh_vertices;
            // The plan must not grow frame over frame: it is cleared and refilled.
            assert_eq!(
                m.prims, first.prims,
                "prim count drifted between identical frames"
            );
            assert_eq!(
                m.ops, first.ops,
                "op count drifted between identical frames"
            );
        }
        samples.sort_unstable();
        let p99 = percentile(&samples, 0.99);
        let pass = p99 <= P99_BUDGET_MS;
        all_pass &= pass;
        println!(
            "{:>10}  {:>7}  {:>6}  {:>6}  {:>7.3}  {:>7.3}  {:>7.3}  {:>7.3}  {:>8}  {:>6}",
            count,
            first.prims,
            first.quads,
            first.paths,
            percentile(&samples, 0.50),
            percentile(&samples, 0.95),
            p99,
            percentile(&samples, 1.0),
            steady_verts,
            if pass { "PASS" } else { "FAIL" }
        );
        let _ = fixture.source_points;
    }

    let mut footprint = build_dense_footprint_fixture();
    let frame = footprint.build_frame();
    let prepared = PreparedAerisFrame::new(&frame);
    let mut renderer = GpuiChartRenderer::new();
    let first = renderer
        .plan_frame(&prepared, DPR as f32)
        .expect("dense footprint frame plans");
    for _ in 0..10 {
        renderer.plan_frame(&prepared, DPR as f32).unwrap();
    }
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let started = Instant::now();
        let metrics = renderer.plan_frame(&prepared, DPR as f32).unwrap();
        samples.push(started.elapsed().as_nanos() as u64);
        assert_eq!(metrics.prims, first.prims, "footprint prim count drifted");
        assert_eq!(
            metrics.text_runs, first.text_runs,
            "footprint text count drifted"
        );
    }
    samples.sort_unstable();
    let p99 = percentile(&samples, 0.99);
    let pass = p99 <= P99_BUDGET_MS;
    all_pass &= pass;
    println!(
        "dense footprint  {:>7} prims  {:>6} text  p50 {:>7.3} ms  p95 {:>7.3} ms  p99 {:>7.3} ms  {:>6}",
        first.prims,
        first.text_runs,
        percentile(&samples, 0.50),
        percentile(&samples, 0.95),
        p99,
        if pass { "PASS" } else { "FAIL" }
    );

    println!(
        "\noverall: {}",
        if all_pass {
            "PASS — every fixture's p99 is within budget"
        } else {
            "FAIL — at least one fixture exceeded the p99 budget"
        }
    );
    if !all_pass {
        std::process::exit(1);
    }
}
