//! Performance gate for the two named production targets (roadmap). Headless: measures the
//! `origin_engine` CPU cost — frame construction and data ingestion — which is what governs whether
//! the browser can hit 60fps; GPU present time is a separate, backend-specific concern.
//!
//!   Target A — 60fps @ 10 series x 50k bars:  `build_frame` under 16.67 ms/frame
//!   Target B — 1M-bar load under 300 ms:      `set_series_data` of 1,000,000 bars
//!
//! Report-only by default (prints numbers + PASS/FAIL). Set `ORIGIN_PERF_STRICT=1` to exit non-zero
//! on any failure so CI can treat it as a hard gate; thresholds are machine-dependent, so the
//! strict mode is opt-in rather than the default.
//!
//! Run: `cargo run -p origin_native --example perf_gate --release`

use std::time::Instant;

use origin_engine::{ChartEngine, ChartFrame, SeriesKind};

/// Parallel `(times, open, high, low, close)` columns.
type OhlcColumns = (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>);

/// Deterministic OHLC columns for `bars` points, offset by `phase` so stacked series differ.
fn gen_series(bars: usize, phase: f64) -> OhlcColumns {
    let mut times = Vec::with_capacity(bars);
    let mut open = Vec::with_capacity(bars);
    let mut high = Vec::with_capacity(bars);
    let mut low = Vec::with_capacity(bars);
    let mut close = Vec::with_capacity(bars);
    let mut price = 100.0 + phase;
    for i in 0..bars {
        let next = price + ((i as f64) * 0.017 + phase).sin() * 0.8;
        times.push(i as f64);
        open.push(price);
        high.push(price.max(next) + 0.4);
        low.push(price.min(next) - 0.4);
        close.push(next);
        price = next;
    }
    (times, open, high, low, close)
}

fn report(label: &str, measured_ms: f64, budget_ms: f64) -> bool {
    let pass = measured_ms <= budget_ms;
    println!(
        "  [{}] {label}: {measured_ms:.2} ms (budget {budget_ms:.2} ms)",
        if pass { "PASS" } else { "FAIL" }
    );
    pass
}

fn main() {
    const SERIES: usize = 10;
    const FRAME_BARS: usize = 50_000;
    const FRAME_BUDGET_MS: f64 = 1000.0 / 60.0;
    const FRAMES: usize = 60;
    const LOAD_BARS: usize = 1_000_000;
    const LOAD_BUDGET_MS: f64 = 300.0;

    println!("origin perf gate (release build recommended)\n");

    // ---- Target A: 60fps @ 10 series x 50k bars ---------------------------------------------
    let mut chart = ChartEngine::new(1600.0, 800.0, 1.0);
    // series[0] exists at construction; add the remaining nine on the shared time axis.
    let mut ids = vec![0usize];
    for _ in 1..SERIES {
        ids.push(chart.add_series(SeriesKind::Candlestick));
    }
    for (n, &id) in ids.iter().enumerate() {
        let (t, o, h, l, c) = gen_series(FRAME_BARS, n as f64 * 3.0);
        chart
            .set_series_data(id, &t, &o, &h, &l, &c)
            .expect("valid series fixture");
    }
    chart.time_scale.set_width(1600.0);
    chart.fit_content();

    let mut frame = ChartFrame::default();
    chart.build_frame_into(&mut frame); // warm up buffers + caches
    let start = Instant::now();
    for _ in 0..FRAMES {
        chart.build_frame_into(&mut frame);
    }
    let per_frame_ms = start.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
    println!("Target A — 60fps @ {SERIES} series x {FRAME_BARS} bars:");
    let a_pass = report("build_frame", per_frame_ms, FRAME_BUDGET_MS);

    // ---- Target B: 1M-bar load under 300 ms -------------------------------------------------
    let (t, o, h, l, c) = gen_series(LOAD_BARS, 0.0);
    let mut load_chart = ChartEngine::new(1600.0, 800.0, 1.0);
    let start = Instant::now();
    load_chart
        .set_series_data(0, &t, &o, &h, &l, &c)
        .expect("valid load fixture");
    let load_ms = start.elapsed().as_secs_f64() * 1000.0;
    println!("Target B — {LOAD_BARS} bar load:");
    let b_pass = report("set_series_data", load_ms, LOAD_BUDGET_MS);

    let all_pass = a_pass && b_pass;
    println!(
        "\n{}",
        if all_pass {
            "ALL TARGETS PASS"
        } else {
            "SOME TARGETS FAILED"
        }
    );
    if !all_pass && std::env::var("ORIGIN_PERF_STRICT").is_ok() {
        std::process::exit(1);
    }
}
