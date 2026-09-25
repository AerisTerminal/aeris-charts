//! Performance gate for the repository's named production targets (roadmap). Headless: measures the
//! `aeris_charts_engine` CPU cost — frame construction and data ingestion — which is what governs whether
//! the browser can hit 60fps; GPU present time is a separate, backend-specific concern.
//!
//!   Target A — 60fps @ 10 series x 50k bars:  `build_frame` under 16.67 ms/frame
//!   Target B — 1M-bar load under 300 ms:      `set_series_data` of 1,000,000 bars
//!   Target C — canonical pointer sample:      fixed-capacity resolver under 0.01 ms/sample
//!   Target D — footprint history/live/correction ingestion plus shared-frame construction
//!   Target E — 100k visible-bar volume profile refresh and cached shared frame
//!   Target F — 100k-point general XY line frame + nearest-hit interaction
//!   Target G — mixed 100k-row general dashboard frame, hit interaction, and retained memory
//!   Target H — combined 50k-bar financial + 50k-point general frame and retained memory
//!   Target I — 100k-row numeric error bars frame, hit interaction, and retained memory
//!
//! Report-only by default (prints numbers + PASS/FAIL). Set `AERIS_CHARTS_PERF_STRICT=1` to exit non-zero
//! on any failure so CI can treat it as a hard gate; thresholds are machine-dependent, so the
//! strict mode is opt-in rather than the default.
//!
//! Run: `cargo run -p aeris_charts_native --example perf_gate --release`

use std::time::Instant;

use aeris_charts_engine::{
    AggressorSide, AxisDimension, ChartEngine, ChartFrame, ContinuousScaleType,
    FootprintAggregationOptions, FootprintBarAggregation, FootprintSeriesOptions, FootprintTrade,
    FootprintVisualOptions, GeneralAxisOptions, GeneralHitMode, GeneralScaleType,
    GeneralSeriesOptions, GeneralXyInput, GestureResolver, HorizontalDomain, InputDevice,
    InputTarget, PointerSample, SeriesKind, TradeBubbleOptions, TradeStudyOptions,
};
use aeris_charts_render::draw_list::Prim;
use aeris_charts_render_wgpu::{prims_to_group, DrawGroup, TexQuadInstance};

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

fn report_bytes(label: &str, measured_bytes: usize, budget_bytes: usize) -> bool {
    let pass = measured_bytes <= budget_bytes;
    println!(
        "  [{}] {label}: {:.2} MiB (budget {:.2} MiB)",
        if pass { "PASS" } else { "FAIL" },
        measured_bytes as f64 / (1024.0 * 1024.0),
        budget_bytes as f64 / (1024.0 * 1024.0),
    );
    pass
}

fn gen_footprint_trades(
    start_bar: usize,
    bars: usize,
    trades_per_bar: usize,
) -> Vec<FootprintTrade> {
    let mut trades = Vec::with_capacity(bars * trades_per_bar);
    for bar in start_bar..start_bar + bars {
        let bar_time = bar as i64 * 60_000_000;
        for tick in 0..trades_per_bar {
            let level = ((bar + tick * 7) % 21) as i64 - 10;
            trades.push(FootprintTrade {
                timestamp_micros: bar_time + tick as i64 * 1_000,
                price: 100.0 + level as f64 * 0.25,
                volume: (tick % 17 + 1) as f64,
                aggressor: if (bar + tick) % 2 == 0 {
                    AggressorSide::Buy
                } else {
                    AggressorSide::Sell
                },
                bid: None,
                ask: None,
                sequence: Some(tick as u64),
                trade_id: Some((bar * trades_per_bar + tick) as u64),
                conditions: 0,
                session_id: Some((bar / 1_440) as u64),
            });
        }
    }
    trades
}

/// Measure the WebGPU adapter's CPU-side primitive scheduling for a dense numbers bar. The real
/// atlas rasterizer and GPU present are host/device concerns; this gate covers the bounded frame
/// encoding work that runs before those submissions and keeps text runs in prim order.
fn dense_footprint_wgpu_scene_ms() -> (usize, usize, f64) {
    const BARS: usize = 20;
    const TRADES_PER_BAR: usize = 22;
    const RUNS: usize = 30;
    let mut chart = ChartEngine::new(1600.0, 800.0, 1.0);
    let footprint = chart
        .add_footprint_series(FootprintSeriesOptions::default())
        .expect("default footprint options are valid");
    chart.set_series_visible(0, false);
    chart
        .set_footprint_trades(footprint, gen_footprint_trades(0, BARS, TRADES_PER_BAR))
        .expect("dense footprint trades are valid");
    chart.time_scale.set_width(1600.0);
    chart.fit_content();
    chart.set_bar_spacing(72.0);
    let frame = chart.build_frame();
    let pane = &frame.panes[0];
    let text_count = pane
        .main
        .iter()
        .filter(|prim| matches!(prim, Prim::Text { .. } | Prim::RotatedText { .. }))
        .count();
    let mut group = DrawGroup::default();
    let mut resolve_text = |prim: &Prim| -> Option<TexQuadInstance> {
        let (x, y) = match prim {
            Prim::Text { x, y, .. } | Prim::RotatedText { x, y, .. } => (*x, *y),
            _ => return None,
        };
        Some(TexQuadInstance {
            rect: [x, y, 2.0, 2.0],
            uv: [0.0, 0.0, 0.5, 0.5],
            color: [0.0, 0.0, 0.0, 1.0],
        })
    };
    let mut resolve_image = |_prim: &Prim| None;
    for _ in 0..5 {
        group.clear();
        prims_to_group(
            &pane.main,
            &pane.points,
            &mut group,
            &mut resolve_text,
            &mut resolve_image,
        );
    }
    let mut samples = Vec::with_capacity(RUNS);
    let mut text_instances = 0;
    for _ in 0..RUNS {
        group.clear();
        let started = Instant::now();
        prims_to_group(
            &pane.main,
            &pane.points,
            &mut group,
            &mut resolve_text,
            &mut resolve_image,
        );
        samples.push(started.elapsed().as_nanos() as u64);
        text_instances = group.tex_quads.len();
    }
    samples.sort_unstable();
    let p99_index = ((samples.len() as f64 - 1.0) * 0.99).round() as usize;
    (
        text_count,
        text_instances,
        samples[p99_index] as f64 / 1_000_000.0,
    )
}

fn main() {
    const SERIES: usize = 10;
    const FRAME_BARS: usize = 50_000;
    const FRAME_BUDGET_MS: f64 = 1000.0 / 60.0;
    const FRAMES: usize = 60;
    const LOAD_BARS: usize = 1_000_000;
    const LOAD_BUDGET_MS: f64 = 300.0;
    const INPUT_SAMPLES: usize = 1_000_000;
    const INPUT_SAMPLE_BUDGET_MS: f64 = 0.01;
    const FOOTPRINT_HISTORY_BARS: usize = 2_500;
    const FOOTPRINT_TRADES_PER_BAR: usize = 100;
    const FOOTPRINT_LOAD_BUDGET_MS: f64 = 300.0;
    const FOOTPRINT_LIVE_BARS: usize = 100;
    const FOOTPRINT_LIVE_BUDGET_MS: f64 = 50.0;
    const FOOTPRINT_CORRECTION_BARS: usize = 10;
    const FOOTPRINT_CORRECTION_BUDGET_MS: f64 = 300.0;
    const GENERAL_LINE_POINTS: usize = 100_000;
    const GENERAL_LINE_HIT_SAMPLES: usize = 100;
    const GENERAL_LINE_HIT_BUDGET_MS: f64 = 8.0;
    const GENERAL_MIX_POINTS_PER_SERIES: usize = 20_000;
    const GENERAL_MIX_MEMORY_BUDGET_BYTES: usize = 12 * 1024 * 1024;
    const COMBINED_FINANCIAL_BARS: usize = 50_000;
    const COMBINED_GENERAL_POINTS: usize = 50_000;
    const COMBINED_MEMORY_BUDGET_BYTES: usize = 16 * 1024 * 1024;
    const ERROR_BAR_POINTS: usize = 100_000;
    const ERROR_BAR_MEMORY_BUDGET_BYTES: usize = 16 * 1024 * 1024;

    println!("aeris_charts perf gate (release build recommended)\n");

    // ---- Target A: 60fps @ 10 series x 50k bars ---------------------------------------------
    let mut chart = ChartEngine::new(1600.0, 800.0, 1.0);
    // series[0] exists at construction; add the remaining nine on the shared time axis.
    let mut ids = vec![0u32];
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

    // ---- Target C: allocation-free canonical pointer resolver -------------------------------
    // GestureResolver contains only a two-slot inline pointer array and scalar state: the move
    // loop has no heap owner or capacity growth path. Measure the release-mode sample latency.
    let mut input = GestureResolver::default();
    let mut sample = PointerSample {
        id: 1,
        device: InputDevice::Touch,
        target: InputTarget::Pane,
        modifiers: Default::default(),
        x: 100.0,
        y: 100.0,
        timestamp_ms: 0.0,
        pressure: 0.5,
        tilt_x: 0.0,
        tilt_y: 0.0,
    };
    input.pointer_down(sample);
    let start = Instant::now();
    for index in 0..INPUT_SAMPLES {
        sample.x = 100.0 + (index & 63) as f64;
        sample.timestamp_ms = index as f64;
        std::hint::black_box(input.pointer_move(sample));
    }
    let per_sample_ms = start.elapsed().as_secs_f64() * 1000.0 / INPUT_SAMPLES as f64;
    println!(
        "Target C — {INPUT_SAMPLES} canonical pointer samples ({}-byte fixed resolver):",
        std::mem::size_of::<GestureResolver>()
    );
    let c_pass = report("pointer_move", per_sample_ms, INPUT_SAMPLE_BUDGET_MS);

    // ---- Target D: tick-truth footprint history + one synchronized live batch ----------------
    let mut footprint = ChartEngine::new(1600.0, 800.0, 1.0);
    footprint
        .configure_footprint_series(
            0,
            FootprintSeriesOptions {
                aggregation: FootprintAggregationOptions {
                    tick_size: 0.25,
                    bars: FootprintBarAggregation::Time {
                        interval_micros: 60_000_000,
                        anchor_micros: 0,
                    },
                    ..FootprintAggregationOptions::default()
                },
                visual: FootprintVisualOptions::default(),
            },
        )
        .expect("valid footprint options");
    let footprint_stream = footprint
        .add_trade_stream(
            "PERF:ES",
            FootprintAggregationOptions {
                tick_size: 0.25,
                bars: FootprintBarAggregation::Time {
                    interval_micros: 60_000_000,
                    anchor_micros: 0,
                },
                ..FootprintAggregationOptions::default()
            },
        )
        .expect("valid shared footprint stream");
    footprint
        .bind_footprint_series_to_stream(0, footprint_stream)
        .expect("bind footprint stream");
    let _cvd = footprint
        .add_cvd_series(footprint_stream, 1, TradeStudyOptions::default())
        .expect("add CVD dependent");
    let _delta = footprint
        .add_delta_series(footprint_stream, 1)
        .expect("add delta dependent");
    footprint
        .add_trade_bubbles(
            footprint_stream,
            0,
            TradeBubbleOptions {
                minimum_volume: 10.0,
                max_markers: 2_048,
                aggregation_window_micros: 0,
            },
        )
        .expect("add bubble dependent");
    let history = gen_footprint_trades(0, FOOTPRINT_HISTORY_BARS, FOOTPRINT_TRADES_PER_BAR);
    let start = Instant::now();
    footprint
        .set_footprint_trades(0, history)
        .expect("valid footprint history");
    let footprint_load_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert!(footprint.set_series_max_points(0, Some(FOOTPRINT_HISTORY_BARS)));
    let footprint_memory_before = footprint.memory_usage().footprint_capacity_bytes;
    footprint.time_scale.set_width(1600.0);
    footprint.fit_content();
    let mut footprint_frame = ChartFrame::default();
    let start = Instant::now();
    footprint.build_frame_into(&mut footprint_frame);
    let footprint_frame_ms = start.elapsed().as_secs_f64() * 1000.0;
    let live = gen_footprint_trades(
        FOOTPRINT_HISTORY_BARS,
        FOOTPRINT_LIVE_BARS,
        FOOTPRINT_TRADES_PER_BAR,
    );
    let start = Instant::now();
    footprint
        .update_footprint_trades(0, live)
        .expect("valid footprint live batch");
    let footprint_live_ms = start.elapsed().as_secs_f64() * 1000.0;
    let footprint_stats = footprint
        .footprint_work_stats(0)
        .expect("footprint work stats");
    let mut corrections = gen_footprint_trades(
        FOOTPRINT_HISTORY_BARS + FOOTPRINT_LIVE_BARS - FOOTPRINT_CORRECTION_BARS,
        FOOTPRINT_CORRECTION_BARS,
        FOOTPRINT_TRADES_PER_BAR,
    );
    for trade in &mut corrections {
        trade.volume += 1.0;
    }
    let before_correction = footprint_stats;
    let start = Instant::now();
    footprint
        .update_footprint_trades(0, corrections)
        .expect("valid footprint correction batch");
    let footprint_correction_ms = start.elapsed().as_secs_f64() * 1000.0;
    let after_correction = footprint
        .footprint_work_stats(0)
        .expect("footprint work stats after correction");
    let correction_rebuilds =
        after_correction.historical_rebuilds - before_correction.historical_rebuilds;
    let correction_rebuilt_ticks = after_correction.rebuilt_ticks - before_correction.rebuilt_ticks;
    assert_eq!(
        correction_rebuilds, 1,
        "one correction batch must reconstruct once"
    );
    let footprint_retained_bars = footprint.footprint_bars(0).expect("footprint bars").len();
    let footprint_memory_after = footprint.memory_usage().footprint_capacity_bytes;
    let footprint_stream_stats = footprint
        .trade_stream_stats(footprint_stream)
        .expect("shared footprint stream stats");
    assert_eq!(footprint_stream_stats.dependent_count, 3);
    println!(
        "Target D — {} footprint trades / {} bars + {}-trade live batch ({} incremental ticks, {} historical rebuilds, {} dependent incremental updates, {} retained bars, {:.2}/{:.2} MiB footprint capacity):",
        FOOTPRINT_HISTORY_BARS * FOOTPRINT_TRADES_PER_BAR,
        FOOTPRINT_HISTORY_BARS,
        FOOTPRINT_LIVE_BARS * FOOTPRINT_TRADES_PER_BAR,
        footprint_stats.incremental_ticks,
        footprint_stats.historical_rebuilds,
        footprint_stream_stats.dependent_incremental_updates,
        footprint_retained_bars,
        footprint_memory_before as f64 / (1024.0 * 1024.0),
        footprint_memory_after as f64 / (1024.0 * 1024.0),
    );
    let d_load_pass = report(
        "set_footprint_trades",
        footprint_load_ms,
        FOOTPRINT_LOAD_BUDGET_MS,
    );
    let d_live_pass = report(
        "update_footprint_trades",
        footprint_live_ms,
        FOOTPRINT_LIVE_BUDGET_MS,
    );
    let d_correction_pass = report(
        &format!(
            "correct_footprint_trades ({} trades, {correction_rebuilds} rebuild, {correction_rebuilt_ticks} rebuilt ticks)",
            FOOTPRINT_CORRECTION_BARS * FOOTPRINT_TRADES_PER_BAR,
        ),
        footprint_correction_ms,
        FOOTPRINT_CORRECTION_BUDGET_MS,
    );
    let d_frame_pass = report("footprint build_frame", footprint_frame_ms, FRAME_BUDGET_MS);
    let d_retention_pass = footprint_retained_bars <= FOOTPRINT_HISTORY_BARS;
    println!(
        "  {:<24} {:>8}",
        "retention ceiling",
        if d_retention_pass { "PASS" } else { "FAIL" }
    );
    let d_pass =
        d_load_pass && d_live_pass && d_correction_pass && d_frame_pass && d_retention_pass;

    let (dense_text_prims, dense_text_instances, dense_wgpu_p99_ms) =
        dense_footprint_wgpu_scene_ms();
    println!(
        "Target J — dense footprint WebGPU frame encoding ({} text prims, {} atlas instances):",
        dense_text_prims, dense_text_instances
    );
    let j_text_count = dense_text_prims == dense_text_instances;
    println!(
        "  [{}] text-run scheduling preserves all resolved runs",
        if j_text_count { "PASS" } else { "FAIL" }
    );
    let j_scene = report(
        "WebGPU dense text frame encoding p99",
        dense_wgpu_p99_ms,
        2.0,
    );

    // Profile work is measured through the real frame path, including timestamp matching.
    let volume = load_chart.add_series(SeriesKind::Histogram);
    let values = vec![1000.0; LOAD_BARS];
    load_chart
        .set_series_data(volume, &t, &values, &values, &values, &values)
        .unwrap();
    load_chart
        .series
        .iter_mut()
        .find(|series| series.id == volume)
        .unwrap()
        .visible = false;
    load_chart.time_scale.set_width(1600.0);
    load_chart.set_min_bar_spacing(0.001);
    load_chart.build_frame();
    load_chart.set_visible_logical_range(900_000.0, 999_999.0);
    let profile = load_chart
        .add_volume_profile_indicator(0, volume, Default::default())
        .unwrap();
    let start = Instant::now();
    load_chart.build_frame();
    let profile_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        load_chart
            .volume_profile_indicator_snapshot(profile)
            .unwrap()
            .profile
            .bar_count,
        100_000
    );
    let revision = load_chart
        .volume_profile_indicator_snapshot(profile)
        .unwrap()
        .calculation_revision;
    let start = Instant::now();
    for _ in 0..FRAMES {
        load_chart.build_frame();
    }
    let cached_ms = start.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
    assert_eq!(
        load_chart
            .volume_profile_indicator_snapshot(profile)
            .unwrap()
            .calculation_revision,
        revision
    );
    println!("Target E — 100k visible-bar volume profile (48 rows):");
    let e_refresh = report("profile refresh + frame", profile_ms, FRAME_BUDGET_MS);
    let e_cached = report("cached profile frame", cached_ms, FRAME_BUDGET_MS);

    // ---- Target F: first Phase 2 general-only density gate -----------------------------------
    let mut general = ChartEngine::new(1600.0, 800.0, 1.0);
    let pane = general
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .expect("valid general pane");
    general
        .add_general_axis(GeneralAxisOptions::new(
            "general-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .expect("valid general X axis");
    general
        .add_general_axis(GeneralAxisOptions::new(
            "general-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .expect("valid general Y axis");
    let mut x = Vec::with_capacity(GENERAL_LINE_POINTS);
    let mut y = Vec::with_capacity(GENERAL_LINE_POINTS);
    for index in 0..GENERAL_LINE_POINTS {
        x.push(index as f64);
        y.push(100.0 + (index as f64 * 0.013).sin() * 20.0);
    }
    let dataset = general
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x,
            y,
            y_valid: None,
        })
        .expect("valid general line dataset");
    general
        .add_general_series(GeneralSeriesOptions::xy_line(
            pane,
            dataset,
            "general-x",
            "general-y",
        ))
        .expect("valid general XY line");
    general.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut general_frame = ChartFrame::default();
    general.build_frame_into(&mut general_frame);
    let start = Instant::now();
    for _ in 0..FRAMES {
        general.build_frame_into(&mut general_frame);
    }
    let general_frame_ms = start.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
    let start = Instant::now();
    for sample in 0..GENERAL_LINE_HIT_SAMPLES {
        let x = 1600.0 * (sample as f64 + 0.5) / GENERAL_LINE_HIT_SAMPLES as f64;
        std::hint::black_box(general.general_hit_test(
            pane,
            x,
            400.0,
            GeneralHitMode::Nearest { max_distance: 32.0 },
        ));
    }
    let general_hit_ms = start.elapsed().as_secs_f64() * 1000.0 / GENERAL_LINE_HIT_SAMPLES as f64;
    println!("Target F — {GENERAL_LINE_POINTS} point general XY line:");
    let f_frame = report("xy_line build_frame", general_frame_ms, FRAME_BUDGET_MS);
    let f_hit = report(
        "xy_line nearest hit",
        general_hit_ms,
        GENERAL_LINE_HIT_BUDGET_MS,
    );

    // ---- Target G: mixed general-only dashboard ---------------------------------------------
    // Keep every current high-density geometry family in one coordinate region so this gate
    // catches accidental repeated walks, unbounded retained caches, and interaction regressions.
    let mut mixed = ChartEngine::new(1600.0, 800.0, 1.0);
    let mixed_pane = mixed
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .expect("valid mixed-general pane");
    mixed
        .add_general_axis(GeneralAxisOptions::new(
            "mixed-x",
            mixed_pane,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .expect("valid mixed-general X axis");
    mixed
        .add_general_axis(GeneralAxisOptions::new(
            "mixed-y",
            mixed_pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .expect("valid mixed-general Y axis");
    let mixed_x: Vec<f64> = (0..GENERAL_MIX_POINTS_PER_SERIES)
        .map(|index| index as f64)
        .collect();
    let mixed_y: Vec<f64> = mixed_x
        .iter()
        .map(|x| 100.0 + (x * 0.013).sin() * 20.0)
        .collect();
    let line = mixed
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: mixed_x.clone(),
            y: mixed_y.clone(),
            y_valid: None,
        })
        .expect("valid mixed line dataset");
    mixed
        .add_general_series(GeneralSeriesOptions::xy_line(
            mixed_pane, line, "mixed-x", "mixed-y",
        ))
        .expect("valid mixed line");
    let area = mixed
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: mixed_x.clone(),
            y: mixed_y.iter().map(|value| value - 15.0).collect(),
            y_valid: None,
        })
        .expect("valid mixed area dataset");
    mixed
        .add_general_series(GeneralSeriesOptions::xy_area(
            mixed_pane, area, "mixed-x", "mixed-y",
        ))
        .expect("valid mixed area");
    let range = mixed
        .create_general_xy_dataset(GeneralXyInput::RangeNumeric {
            ids: None,
            x: mixed_x.clone(),
            low: mixed_y.iter().map(|value| value - 8.0).collect(),
            low_valid: None,
            high: mixed_y.iter().map(|value| value + 8.0).collect(),
            high_valid: None,
        })
        .expect("valid mixed range dataset");
    mixed
        .add_general_series(GeneralSeriesOptions::range_area(
            mixed_pane, range, "mixed-x", "mixed-y",
        ))
        .expect("valid mixed range area");
    let scatter = mixed
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: mixed_x.clone(),
            y: mixed_y.iter().map(|value| value + 25.0).collect(),
            y_valid: None,
        })
        .expect("valid mixed scatter dataset");
    mixed
        .add_general_series(GeneralSeriesOptions::scatter(
            mixed_pane, scatter, "mixed-x", "mixed-y",
        ))
        .expect("valid mixed scatter");
    let bubble = mixed
        .create_general_xy_dataset(GeneralXyInput::Bubble {
            ids: None,
            x: mixed_x,
            y: mixed_y.iter().map(|value| value - 30.0).collect(),
            y_valid: None,
            size: (0..GENERAL_MIX_POINTS_PER_SERIES)
                .map(|index| (index % 16 + 1) as f64)
                .collect(),
            size_valid: None,
        })
        .expect("valid mixed bubble dataset");
    mixed
        .add_general_series(GeneralSeriesOptions::bubble(
            mixed_pane, bubble, "mixed-x", "mixed-y",
        ))
        .expect("valid mixed bubble");
    mixed.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut mixed_frame = ChartFrame::default();
    mixed.build_frame_into(&mut mixed_frame);
    let start = Instant::now();
    for _ in 0..FRAMES {
        mixed.build_frame_into(&mut mixed_frame);
    }
    let mixed_frame_ms = start.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
    let start = Instant::now();
    for sample in 0..GENERAL_LINE_HIT_SAMPLES {
        let x = 1600.0 * (sample as f64 + 0.5) / GENERAL_LINE_HIT_SAMPLES as f64;
        std::hint::black_box(mixed.general_hit_test(
            mixed_pane,
            x,
            400.0,
            GeneralHitMode::Nearest { max_distance: 32.0 },
        ));
    }
    let mixed_hit_ms = start.elapsed().as_secs_f64() * 1000.0 / GENERAL_LINE_HIT_SAMPLES as f64;
    let mixed_memory = mixed.memory_usage().estimated_live_bytes();
    println!(
        "Target G — 5-series mixed general dashboard ({} total rows):",
        GENERAL_MIX_POINTS_PER_SERIES * 5
    );
    let g_frame = report("mixed general build_frame", mixed_frame_ms, FRAME_BUDGET_MS);
    let g_hit = report(
        "mixed general nearest hit",
        mixed_hit_ms,
        GENERAL_LINE_HIT_BUDGET_MS,
    );
    let g_memory = report_bytes(
        "mixed general retained memory",
        mixed_memory,
        GENERAL_MIX_MEMORY_BUDGET_BYTES,
    );

    // ---- Target H: one engine with financial and general panes ------------------------------
    let mut combined = ChartEngine::new(1600.0, 800.0, 1.0);
    let (times, open, high, low, close) = gen_series(COMBINED_FINANCIAL_BARS, 0.0);
    combined
        .set_series_data(0, &times, &open, &high, &low, &close)
        .expect("valid combined financial fixture");
    combined.time_scale.set_width(1600.0);
    combined.fit_content();
    let combined_pane = combined
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .expect("valid combined general pane");
    combined
        .add_general_axis(GeneralAxisOptions::new(
            "combined-x",
            combined_pane,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .expect("valid combined X axis");
    combined
        .add_general_axis(GeneralAxisOptions::new(
            "combined-y",
            combined_pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .expect("valid combined Y axis");
    let combined_x: Vec<f64> = (0..COMBINED_GENERAL_POINTS)
        .map(|index| index as f64)
        .collect();
    let combined_y: Vec<f64> = combined_x
        .iter()
        .map(|x| 50.0 + (x * 0.017).sin() * 12.0)
        .collect();
    let combined_dataset = combined
        .create_general_xy_dataset(GeneralXyInput::RangeNumeric {
            ids: None,
            x: combined_x,
            low: combined_y.iter().map(|value| value - 4.0).collect(),
            low_valid: None,
            high: combined_y.iter().map(|value| value + 4.0).collect(),
            high_valid: None,
        })
        .expect("valid combined range dataset");
    combined
        .add_general_series(GeneralSeriesOptions::range_area(
            combined_pane,
            combined_dataset,
            "combined-x",
            "combined-y",
        ))
        .expect("valid combined range area");
    combined.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut combined_frame = ChartFrame::default();
    combined.build_frame_into(&mut combined_frame);
    let start = Instant::now();
    for _ in 0..FRAMES {
        combined.build_frame_into(&mut combined_frame);
    }
    let combined_frame_ms = start.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
    let combined_memory = combined.memory_usage().estimated_live_bytes();
    println!(
        "Target H — {COMBINED_FINANCIAL_BARS} financial bars + {COMBINED_GENERAL_POINTS} general points:"
    );
    let h_frame = report("combined build_frame", combined_frame_ms, FRAME_BUDGET_MS);
    let h_memory = report_bytes(
        "combined retained memory",
        combined_memory,
        COMBINED_MEMORY_BUDGET_BYTES,
    );

    // ---- Target I: dense numeric error bars -------------------------------------------------
    let mut errors = ChartEngine::new(1600.0, 800.0, 1.0);
    let error_pane = errors
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .expect("valid error-bar pane");
    errors
        .add_general_axis(GeneralAxisOptions::new(
            "error-x",
            error_pane,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .expect("valid error-bar X axis");
    errors
        .add_general_axis(GeneralAxisOptions::new(
            "error-y",
            error_pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .expect("valid error-bar Y axis");
    let error_x: Vec<f64> = (0..ERROR_BAR_POINTS).map(|index| index as f64).collect();
    let error_y: Vec<f64> = error_x
        .iter()
        .map(|x| 100.0 + (x * 0.013).sin() * 20.0)
        .collect();
    let error_dataset = errors
        .create_general_xy_dataset(GeneralXyInput::ErrorNumeric {
            ids: None,
            x: error_x.clone(),
            y: error_y.clone(),
            y_valid: None,
            x_low: error_x.iter().map(|x| x - 0.25).collect(),
            x_low_valid: None,
            x_high: error_x.iter().map(|x| x + 0.25).collect(),
            x_high_valid: None,
            y_low: error_y.iter().map(|y| y - 3.0).collect(),
            y_low_valid: None,
            y_high: error_y.iter().map(|y| y + 3.0).collect(),
            y_high_valid: None,
        })
        .expect("valid dense error-bar dataset");
    errors
        .add_general_series(GeneralSeriesOptions::error_bar(
            error_pane,
            error_dataset,
            "error-x",
            "error-y",
        ))
        .expect("valid error-bar series");
    errors.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut error_frame = ChartFrame::default();
    errors.build_frame_into(&mut error_frame);
    let start = Instant::now();
    for _ in 0..FRAMES {
        errors.build_frame_into(&mut error_frame);
    }
    let error_frame_ms = start.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
    let start = Instant::now();
    for sample in 0..GENERAL_LINE_HIT_SAMPLES {
        let x = 1600.0 * (sample as f64 + 0.5) / GENERAL_LINE_HIT_SAMPLES as f64;
        std::hint::black_box(errors.general_hit_test(
            error_pane,
            x,
            400.0,
            GeneralHitMode::Nearest { max_distance: 32.0 },
        ));
    }
    let error_hit_ms = start.elapsed().as_secs_f64() * 1000.0 / GENERAL_LINE_HIT_SAMPLES as f64;
    let error_memory = errors.memory_usage().estimated_live_bytes();
    println!("Target I — {ERROR_BAR_POINTS} numeric error bars:");
    let i_frame = report("error_bar build_frame", error_frame_ms, FRAME_BUDGET_MS);
    let i_hit = report(
        "error_bar nearest hit",
        error_hit_ms,
        GENERAL_LINE_HIT_BUDGET_MS,
    );
    let i_memory = report_bytes(
        "error_bar retained memory",
        error_memory,
        ERROR_BAR_MEMORY_BUDGET_BYTES,
    );

    let all_pass = a_pass
        && b_pass
        && c_pass
        && d_pass
        && j_text_count
        && j_scene
        && e_refresh
        && e_cached
        && f_frame
        && f_hit
        && g_frame
        && g_hit
        && g_memory
        && h_frame
        && h_memory
        && i_frame
        && i_hit
        && i_memory;
    println!(
        "\n{}",
        if all_pass {
            "ALL TARGETS PASS"
        } else {
            "SOME TARGETS FAILED"
        }
    );
    if !all_pass && std::env::var("AERIS_CHARTS_PERF_STRICT").is_ok() {
        std::process::exit(1);
    }
}
