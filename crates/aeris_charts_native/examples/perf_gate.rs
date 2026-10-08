//! Performance gate for the repository's named production targets (roadmap). Headless: measures the
//! `aeris_charts_engine` CPU cost — frame construction and data ingestion — which is what governs whether
//! the browser can hit 60fps; GPU present time is a separate, backend-specific concern.
//!
//!   Target A — 60fps @ 10 series x 50k bars:  `build_frame` under 16.67 ms/frame
//!   Target B — 1M-bar load under 300 ms:      `set_series_data` of 1,000,000 bars
//!   Target C — canonical pointer sample:      fixed-capacity resolver under 0.01 ms/sample
//!   Target D — shared non-time candles/footprint history, live, correction, and frame construction
//!   Target E — 100k visible-bar volume profile refresh, periodic developing paths, and cached frame
//!   Target F — 100k-point general XY line frame + nearest-hit interaction
//!   Target G — mixed 100k-row general dashboard frame, hit interaction, and retained memory
//!   Target H — combined 50k-bar financial + 50k-point general frame and retained memory
//!   Target I — 100k-row numeric error bars frame, hit interaction, and retained memory
//!   Target K — 100x replay clock advance, shared projections, frame work, and flat retained memory
//!   Target L — sustained depth updates, bounded heatmap frame work, and live-edge upload size
//!   Target M — sustained live order-flow tape: per-batch update + frame, retention, late print
//!   Target N — 1M-row structure studies: tail update p99 and one bounded historical correction
//!   Target O — sustained live order-flow tape with auction markers enabled
//!   Target P — seven sessions of order flow: batch cost through sealing and session eviction,
//!              stream memory, and the session budget
//!   Target Q — live indicator updates for every `IndicatorKind`: tip append and tip replacement
//!              p99 at 10k and 1M rows per binding and with all attached, history-length scaling,
//!              and fresh-engine equality (`perf_gate/indicator_live.rs`)
//!
//! Report-only by default (prints numbers + PASS/FAIL). Set `AERIS_CHARTS_PERF_STRICT=1` to exit non-zero
//! on any failure so CI can treat it as a hard gate; thresholds are machine-dependent, so the
//! strict mode is opt-in rather than the default.
//!
//! Run: `cargo run -p aeris_charts_native --example perf_gate --release`

use std::time::Instant;

use aeris_charts_engine::{
    AggressorSide, AuctionMarkerOptions, AxisDimension, BigTradesOptions, ChartEngine, ChartFrame,
    ContinuousScaleType, DepthHeatmapOptions, DepthLevel, DepthOptions, DepthSide, DepthSnapshot,
    DepthUpdate, FootprintAggregationOptions, FootprintBarAggregation, FootprintSeriesOptions,
    FootprintTrade, FootprintVisualOptions, GeneralAxisOptions, GeneralHitMode, GeneralScaleType,
    GeneralSeriesOptions, GeneralXyInput, GestureResolver, HorizontalDomain, InputDevice,
    InputTarget, ORDER_FLOW_MAX_RETAINED_SESSIONS, ORDER_FLOW_MAX_RETAINED_TRADES,
    ORDER_FLOW_MAX_STREAM_BYTES, OrderBlockZone, OrderFlowPresentationOptions,
    PeriodicProfilePresentationOptions, PeriodicProfilePresentationRequest, PointerSample,
    PreviousPeriod, ProfileSource, ResampleBoundary, SeriesKind, StructureBreakOn,
    StructureMitigation, StructureMitigationPrice, StudyCalendarPolicy, TradeStudyOptions,
};
use aeris_charts_render::draw_list::Prim;
use aeris_charts_render_wgpu::{DrawGroup, TexQuadInstance, prims_to_group};

#[path = "perf_gate/indicator_live.rs"]
mod indicator_live;

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

/// Sustained live order-flow tape timings, in milliseconds.
struct LiveTapeTimings {
    update_frame_p50_ms: f64,
    update_frame_p99_ms: f64,
    update_frame_max_ms: f64,
    late_trade_ms: f64,
    late_rebuilt_ticks: usize,
    retained_trades_capped: bool,
    auction_marks: usize,
}

/// One host-shaped order-flow presentation (time footprint + CVD + delta panes) fed the way a
/// terminal feeds a live tape: small suffix batches, each followed by a frame. The history sits
/// just under the retention ceiling so the loop crosses it, and one late print lands a bar back.
/// With `with_auction_markers`, one auction-marker set (OF13, default options) is bound to the
/// same stream before the live loop, so every batch and the late print also repair its marks.
fn sustained_order_flow_tape(with_auction_markers: bool) -> LiveTapeTimings {
    const HISTORY_BARS: usize = 2_600;
    const TRADES_PER_BAR: usize = 100;
    const BAR_MICROS: i64 = 60_000_000;
    const TRADE_STEP_MICROS: i64 = BAR_MICROS / TRADES_PER_BAR as i64;
    const UPDATES: usize = 600;
    const TRADES_PER_UPDATE: usize = 4;
    let trade = |ordinal: u64, timestamp_micros: i64| FootprintTrade {
        timestamp_micros,
        price: 100.0 + ((ordinal * 7) % 21) as f64 * 0.25,
        volume: (ordinal % 17 + 1) as f64,
        aggressor: if ordinal.is_multiple_of(2) {
            AggressorSide::Buy
        } else {
            AggressorSide::Sell
        },
        bid: None,
        ask: None,
        sequence: Some(ordinal),
        trade_id: None,
        conditions: 0,
        session_id: Some(1),
    };
    let mut chart = ChartEngine::new(1600.0, 800.0, 1.0);
    let presentation = chart
        .add_order_flow_presentation(
            "PERF:LIVE",
            0,
            OrderFlowPresentationOptions {
                aggregation: FootprintAggregationOptions {
                    tick_size: 0.25,
                    ticks_per_row: 0,
                    bars: FootprintBarAggregation::Time {
                        interval_micros: BAR_MICROS as u64,
                        anchor_micros: 0,
                    },
                    ..FootprintAggregationOptions::default()
                },
                visual: FootprintVisualOptions::default(),
                show_footprint: true,
                show_cumulative_delta: true,
                show_delta_histogram: true,
                big_trades: None,
            },
        )
        .expect("valid order-flow presentation");
    chart.set_series_visible(0, false);
    let mut ordinal = 0_u64;
    let mut timestamp = 0_i64;
    let history = (0..HISTORY_BARS * TRADES_PER_BAR)
        .map(|_| {
            ordinal += 1;
            timestamp += TRADE_STEP_MICROS;
            trade(ordinal, timestamp)
        })
        .collect::<Vec<_>>();
    chart
        .update_order_flow_presentation(presentation, history, false)
        .expect("valid order-flow history");
    let auction_markers = with_auction_markers.then(|| {
        chart
            .add_auction_markers(
                presentation.trade_stream(),
                0,
                AuctionMarkerOptions::default(),
            )
            .expect("valid auction markers")
    });
    chart.time_scale.set_width(1600.0);
    chart.fit_footprint_viewport();
    let mut frame = ChartFrame::default();
    chart.build_frame_into(&mut frame);
    chart.build_frame_into(&mut frame);

    let mut samples = Vec::with_capacity(UPDATES);
    for _ in 0..UPDATES {
        let batch = (0..TRADES_PER_UPDATE)
            .map(|_| {
                ordinal += 1;
                timestamp += TRADE_STEP_MICROS;
                trade(ordinal, timestamp)
            })
            .collect::<Vec<_>>();
        let started = Instant::now();
        chart
            .update_order_flow_presentation(presentation, batch, true)
            .expect("valid live batch");
        chart.build_frame_into(&mut frame);
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let footprint = presentation.footprint_series().expect("footprint drawn");
    // Crossing the ceiling seals the oldest bars: the raw tape is bounded and no bar is lost.
    let history_bars = (ordinal as usize).div_ceil(TRADES_PER_BAR);
    let retained_trades_capped =
        chart
            .trade_stream(presentation.trade_stream())
            .is_some_and(|stream| {
                stream.trades().len() <= ORDER_FLOW_MAX_RETAINED_TRADES
                    && stream.sealed_bar_count() > 0
                    && stream.bars().len() >= history_bars
            });
    let before = chart.footprint_work_stats(footprint).expect("work stats");
    ordinal += 1;
    let late = vec![trade(
        ordinal,
        timestamp - BAR_MICROS - TRADE_STEP_MICROS / 2,
    )];
    let started = Instant::now();
    chart
        .update_order_flow_presentation(presentation, late, true)
        .expect("valid late print");
    chart.build_frame_into(&mut frame);
    let late_trade_ms = started.elapsed().as_secs_f64() * 1000.0;
    let after = chart.footprint_work_stats(footprint).expect("work stats");

    let auction_marks = auction_markers.map_or(0, |id| {
        chart
            .auction_markers_snapshot(id)
            .expect("auction markers snapshot")
            .len()
    });
    samples.sort_unstable_by(f64::total_cmp);
    let percentile =
        |fraction: f64| samples[((samples.len() - 1) as f64 * fraction).round() as usize];
    LiveTapeTimings {
        update_frame_p50_ms: percentile(0.5),
        update_frame_p99_ms: percentile(0.99),
        update_frame_max_ms: samples[samples.len() - 1],
        late_trade_ms,
        late_rebuilt_ticks: after.rebuilt_ticks - before.rebuilt_ticks,
        retained_trades_capped,
        auction_marks,
    }
}

/// Multi-session order-flow history results.
struct SessionHistoryResult {
    batch_p99_ms: f64,
    batch_max_ms: f64,
    stream_bytes: usize,
    sessions: usize,
    bars: usize,
    raw_trades: usize,
    sealed_bars: usize,
}

/// Seven trading sessions of one-minute footprint bars streamed as host-sized suffix batches,
/// with CVD, delta and big trades attached. Sealing runs many times along the way and evicts
/// whole sessions at the end, so the batch timings include every retention step.
fn multi_session_order_flow_history() -> SessionHistoryResult {
    const SESSIONS: u64 = 7;
    const BARS_PER_SESSION: u64 = 1_380;
    const TRADES_PER_BAR: u64 = 150;
    const BAR_MICROS: i64 = 60_000_000;
    const BATCH_TRADES: usize = 1_000;
    let mut chart = ChartEngine::new(1600.0, 800.0, 1.0);
    let presentation = chart
        .add_order_flow_presentation(
            "PERF:SESSIONS",
            0,
            OrderFlowPresentationOptions {
                aggregation: FootprintAggregationOptions {
                    tick_size: 0.25,
                    ticks_per_row: 0,
                    bars: FootprintBarAggregation::Time {
                        interval_micros: BAR_MICROS as u64,
                        anchor_micros: 0,
                    },
                    ..FootprintAggregationOptions::default()
                },
                visual: FootprintVisualOptions::default(),
                show_footprint: true,
                show_cumulative_delta: true,
                show_delta_histogram: true,
                big_trades: Some(BigTradesOptions::default()),
            },
        )
        .expect("valid order-flow presentation");
    chart.set_series_visible(0, false);
    chart.time_scale.set_width(1600.0);
    let mut frame = ChartFrame::default();
    let mut samples = Vec::new();
    let mut batch = Vec::with_capacity(BATCH_TRADES);
    let mut ordinal = 0_u64;
    for session in 0..SESSIONS {
        // Sessions are separated by an hour's break, as on CME futures.
        let session_start = (session * (BARS_PER_SESSION + 60)) as i64 * BAR_MICROS;
        for bar in 0..BARS_PER_SESSION {
            let bar_time = session_start + bar as i64 * BAR_MICROS;
            // A slow drift plus an intrabar swing spreads each bar over a few dozen ticks.
            let center = ((bar as f64 * 0.05).sin() * 120.0) as i64;
            for tick in 0..TRADES_PER_BAR {
                ordinal += 1;
                let swing = ((tick * 13 + bar * 7) % 41) as i64 - 20;
                batch.push(FootprintTrade {
                    timestamp_micros: bar_time + (tick * 400_000) as i64,
                    price: 5_000.0 + (center + swing) as f64 * 0.25,
                    volume: (ordinal % 7 + 1) as f64,
                    aggressor: if ordinal.is_multiple_of(3) {
                        AggressorSide::Sell
                    } else {
                        AggressorSide::Buy
                    },
                    bid: None,
                    ask: None,
                    sequence: Some(ordinal),
                    trade_id: None,
                    conditions: 0,
                    session_id: Some(session),
                });
                if batch.len() == BATCH_TRADES {
                    let trades = std::mem::replace(&mut batch, Vec::with_capacity(BATCH_TRADES));
                    let started = Instant::now();
                    chart
                        .update_order_flow_presentation(presentation, trades, true)
                        .expect("valid session batch");
                    chart.build_frame_into(&mut frame);
                    samples.push(started.elapsed().as_secs_f64() * 1000.0);
                }
            }
        }
    }
    samples.sort_unstable_by(f64::total_cmp);
    let stream = chart
        .trade_stream(presentation.trade_stream())
        .expect("order-flow stream");
    let mut sessions = stream
        .bars()
        .iter()
        .map(|bar| bar.session_id)
        .collect::<Vec<_>>();
    sessions.dedup();
    SessionHistoryResult {
        batch_p99_ms: samples[((samples.len() - 1) as f64 * 0.99).round() as usize],
        batch_max_ms: samples[samples.len() - 1],
        stream_bytes: stream.capacity_bytes(),
        sessions: sessions.len(),
        bars: stream.bars().len(),
        raw_trades: stream.trades().len(),
        sealed_bars: stream.sealed_bar_count(),
    }
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
    const REPLAY_SPEED: usize = 100;
    const REPLAY_SECONDS: usize = 6_000;
    const REPLAY_FRAMES: usize = REPLAY_SECONDS / REPLAY_SPEED;
    const DEPTH_SOAK_UPDATES: usize = 1_200_000;
    const DEPTH_BATCH_UPDATES: usize = 100_000;
    const DEPTH_BATCH_BUDGET_MS: f64 = 150.0;
    const DEPTH_UPLOAD_BUDGET_BYTES: usize = 4_096 * 4;

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
                    ticks_per_row: 1,
                    bars: FootprintBarAggregation::Trades {
                        trades_per_bar: FOOTPRINT_TRADES_PER_BAR as u32,
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
                ticks_per_row: 1,
                bars: FootprintBarAggregation::Trades {
                    trades_per_bar: FOOTPRINT_TRADES_PER_BAR as u32,
                },
                ..FootprintAggregationOptions::default()
            },
        )
        .expect("valid shared footprint stream");
    footprint
        .bind_footprint_series_to_stream(0, footprint_stream)
        .expect("bind footprint stream");
    let trade_candles = footprint.add_series(SeriesKind::Candlestick);
    footprint
        .bind_trade_bar_series_to_stream(trade_candles, footprint_stream)
        .expect("bind trade candle stream");
    let _cvd = footprint
        .add_cvd_series(footprint_stream, 1, TradeStudyOptions::default())
        .expect("add CVD dependent");
    let _delta = footprint
        .add_delta_series(footprint_stream, 1)
        .expect("add delta dependent");
    footprint
        .add_big_trades(footprint_stream, 0, BigTradesOptions::default())
        .expect("add big-trades dependent");
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
    assert_eq!(footprint_stream_stats.dependent_count, 4);
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
    load_chart
        .add_periodic_profile_presentation(
            0,
            PeriodicProfilePresentationRequest {
                source: ProfileSource::Candles {
                    price_series: 0,
                    volume_series: volume,
                },
                boundaries: vec![ResampleBoundary {
                    start_time: 900_000,
                    end_time: 1_000_000,
                    session_id: 1,
                }],
                tick_size: 0.01,
                row_count: 48,
                value_area_percent: 70.0,
            },
            PeriodicProfilePresentationOptions {
                show_developing: true,
                ..Default::default()
            },
        )
        .unwrap();
    let started = Instant::now();
    load_chart.build_frame();
    let periodic_ms = started.elapsed().as_secs_f64() * 1000.0;
    let e_periodic = report(
        "periodic developing profile + frame",
        periodic_ms,
        FRAME_BUDGET_MS,
    );

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

    // ---- Target K: 100x replay through the real chart clock and shared trade projections ------
    let mut replay = ChartEngine::new(1600.0, 800.0, 1.0);
    replay
        .configure_footprint_series(
            0,
            FootprintSeriesOptions {
                aggregation: FootprintAggregationOptions {
                    tick_size: 0.25,
                    ticks_per_row: 1,
                    bars: FootprintBarAggregation::Time {
                        interval_micros: 1_000_000,
                        anchor_micros: 0,
                    },
                    ..FootprintAggregationOptions::default()
                },
                visual: FootprintVisualOptions::default(),
            },
        )
        .expect("valid replay footprint options");
    let replay_stream = replay
        .add_trade_stream(
            "PERF:REPLAY",
            FootprintAggregationOptions {
                tick_size: 0.25,
                ticks_per_row: 1,
                bars: FootprintBarAggregation::Time {
                    interval_micros: 1_000_000,
                    anchor_micros: 0,
                },
                ..FootprintAggregationOptions::default()
            },
        )
        .expect("valid replay stream");
    replay
        .bind_footprint_series_to_stream(0, replay_stream)
        .expect("bind replay footprint");
    let replay_candles = replay.add_series(SeriesKind::Candlestick);
    replay
        .bind_trade_bar_series_to_stream(replay_candles, replay_stream)
        .expect("bind replay candles");
    let replay_trades = (1..=REPLAY_SECONDS)
        .map(|second| FootprintTrade {
            timestamp_micros: second as i64 * 1_000_000,
            price: 100.0 + ((second % 40) as f64 - 20.0) * 0.25,
            volume: 1.0,
            aggressor: if second % 2 == 0 {
                AggressorSide::Buy
            } else {
                AggressorSide::Sell
            },
            bid: None,
            ask: None,
            sequence: Some(second as u64),
            trade_id: Some(second as u64),
            conditions: 0,
            session_id: Some(1),
        })
        .collect();
    replay
        .set_trade_stream_trades(replay_stream, replay_trades)
        .expect("valid replay history");
    replay.time_scale.set_width(1600.0);
    replay.fit_content();
    let mut replay_frame = ChartFrame::default();
    let run_replay = |chart: &mut ChartEngine, frame: &mut ChartFrame| {
        chart
            .set_replay_clock_micros(Some(0))
            .expect("reset replay clock");
        let started = Instant::now();
        for frame_index in 1..=REPLAY_FRAMES {
            chart
                .set_replay_clock_micros(Some((frame_index * REPLAY_SPEED) as i64 * 1_000_000))
                .expect("advance replay clock");
            chart.build_frame_into(frame);
        }
        started.elapsed().as_secs_f64() * 1000.0 / REPLAY_FRAMES as f64
    };
    run_replay(&mut replay, &mut replay_frame);
    let replay_first_ms = run_replay(&mut replay, &mut replay_frame);
    let replay_memory_first = replay.memory_usage().estimated_live_bytes();
    let replay_second_ms = run_replay(&mut replay, &mut replay_frame);
    let replay_memory_second = replay.memory_usage().estimated_live_bytes();
    let replay_visible = replay
        .footprint_bars(0)
        .expect("replay footprint bars")
        .len();
    println!(
        "Target K — {REPLAY_SPEED}x replay over {REPLAY_SECONDS} seconds / {REPLAY_FRAMES} frames:"
    );
    let k_frame = report(
        "clock advance + shared frame",
        replay_first_ms.max(replay_second_ms),
        FRAME_BUDGET_MS,
    );
    let k_flat_memory = replay_memory_second <= replay_memory_first;
    println!(
        "  [{}] flat retained memory: {:.2} MiB -> {:.2} MiB; {replay_visible} visible bars",
        if k_flat_memory { "PASS" } else { "FAIL" },
        replay_memory_first as f64 / (1024.0 * 1024.0),
        replay_memory_second as f64 / (1024.0 * 1024.0),
    );

    // ---- Target L: bounded depth soak and shared image heatmap -------------------------------
    let mut depth = ChartEngine::new(1600.0, 800.0, 1.0);
    let (times, open, high, low, close) = gen_series(2_501, 0.0);
    depth
        .set_series_data(0, &times, &open, &high, &low, &close)
        .expect("valid depth time axis");
    let depth_stream = depth
        .add_depth_stream(
            "PERF:DEPTH",
            DepthOptions {
                tick_size: 0.25,
                max_levels_per_side: 256,
                history_bucket_micros: 100_000,
                max_history_buckets: 512,
                max_history_cells: 65_536,
                max_event_markers: 2_048,
            },
        )
        .expect("valid depth stream");
    let bids = (0..64)
        .map(|level| DepthLevel {
            price: 100.0 - level as f64 * 0.25,
            size: (level + 1) as f64,
            order_count: Some(level + 1),
        })
        .collect();
    let asks = (0..64)
        .map(|level| DepthLevel {
            price: 100.25 + level as f64 * 0.25,
            size: (level + 1) as f64,
            order_count: Some(level + 1),
        })
        .collect();
    depth
        .set_depth_snapshot(
            depth_stream,
            DepthSnapshot {
                timestamp_micros: 0,
                sequence: 1,
                bids,
                asks,
            },
        )
        .expect("valid depth snapshot");
    depth
        .add_depth_heatmap(
            depth_stream,
            DepthHeatmapOptions {
                price_min: 84.25,
                price_max: 116.0,
                maximum_size: 128.0,
                ..DepthHeatmapOptions::default()
            },
        )
        .expect("valid depth heatmap");
    depth.time_scale.set_width(1600.0);
    depth.fit_content();
    let run_depth_soak = |chart: &mut ChartEngine, next_sequence: &mut u64| {
        let mut worst_batch_ms = 0.0_f64;
        for _ in 0..DEPTH_SOAK_UPDATES / DEPTH_BATCH_UPDATES {
            let updates = (0..DEPTH_BATCH_UPDATES)
                .map(|offset| {
                    let sequence = next_sequence.saturating_add(offset as u64);
                    let side = if sequence.is_multiple_of(2) {
                        DepthSide::Bid
                    } else {
                        DepthSide::Ask
                    };
                    let level = (sequence as usize / 2) % 64;
                    DepthUpdate {
                        timestamp_micros: sequence as i64 * 1_000,
                        sequence,
                        previous_sequence: sequence - 1,
                        side,
                        level: DepthLevel {
                            price: match side {
                                DepthSide::Bid => 100.0 - level as f64 * 0.25,
                                DepthSide::Ask => 100.25 + level as f64 * 0.25,
                            },
                            size: (sequence % 127 + 1) as f64,
                            order_count: Some((sequence % 32 + 1) as u32),
                        },
                    }
                })
                .collect::<Vec<_>>();
            let started = Instant::now();
            chart
                .update_depth_batch(depth_stream, &updates)
                .expect("valid depth update batch");
            worst_batch_ms = worst_batch_ms.max(started.elapsed().as_secs_f64() * 1000.0);
            *next_sequence = next_sequence.saturating_add(DEPTH_BATCH_UPDATES as u64);
        }
        worst_batch_ms
    };
    let mut next_depth_sequence = 2_u64;
    let depth_first_ms = run_depth_soak(&mut depth, &mut next_depth_sequence);
    let depth_memory_first = depth.memory_usage().depth_capacity_bytes;
    let mut depth_frame = ChartFrame::default();
    let started = Instant::now();
    depth.build_frame_into(&mut depth_frame);
    let depth_frame_ms = started.elapsed().as_secs_f64() * 1000.0;
    let depth_images = depth_frame.panes[0]
        .under
        .iter()
        .filter_map(|primitive| match primitive {
            Prim::Image { image, .. } => Some(image),
            _ => None,
        })
        .collect::<Vec<_>>();
    let live_upload_bytes = depth_images
        .iter()
        .filter(|image| image.width == 1)
        .map(|image| image.pixels.len())
        .sum::<usize>();
    let depth_second_ms = run_depth_soak(&mut depth, &mut next_depth_sequence);
    let depth_memory_second = depth.memory_usage().depth_capacity_bytes;
    println!(
        "Target L — two {DEPTH_SOAK_UPDATES}-update depth soaks, {} heatmap images:",
        depth_images.len()
    );
    let l_update = report(
        "worst 100k depth batch",
        depth_first_ms.max(depth_second_ms),
        DEPTH_BATCH_BUDGET_MS,
    );
    let l_frame = report("depth heatmap build_frame", depth_frame_ms, FRAME_BUDGET_MS);
    let l_upload = report_bytes(
        "incremental live-edge image",
        live_upload_bytes,
        DEPTH_UPLOAD_BUDGET_BYTES,
    );
    let l_flat_memory = depth_memory_second <= depth_memory_first;
    println!(
        "  [{}] flat retained depth memory: {:.2} MiB -> {:.2} MiB",
        if l_flat_memory { "PASS" } else { "FAIL" },
        depth_memory_first as f64 / (1024.0 * 1024.0),
        depth_memory_second as f64 / (1024.0 * 1024.0),
    );

    let live_tape = sustained_order_flow_tape(false);
    println!(
        "Target M — sustained live order-flow tape (footprint + CVD + delta, 260k-trade history, 600 x 4-trade batches each followed by a frame):"
    );
    println!(
        "  update + frame p50 {:.3} ms, p99 {:.3} ms",
        live_tape.update_frame_p50_ms, live_tape.update_frame_p99_ms
    );
    let m_p99 = report("update + frame p99", live_tape.update_frame_p99_ms, 4.0);
    let m_max = report(
        "worst update + frame (crosses retention ceiling)",
        live_tape.update_frame_max_ms,
        FRAME_BUDGET_MS,
    );
    let m_late = report(
        &format!(
            "late print one bar back + frame ({} rebuilt ticks)",
            live_tape.late_rebuilt_ticks
        ),
        live_tape.late_trade_ms,
        FRAME_BUDGET_MS,
    );
    println!(
        "  [{}] retained tape stays within the order-flow ceiling",
        if live_tape.retained_trades_capped {
            "PASS"
        } else {
            "FAIL"
        }
    );

    // ---- Target N: structure studies over 1M rows -------------------------------------------
    // All seven I3 studies bound to one 1M-row source. Tail updates replace the final row, so
    // each binding repairs only its tip (rebuild_from(n-1)); the historical correction reopens a
    // row STRUCTURE_CORRECTION_ROWS back, so repair is bounded by that suffix plus one checkpoint
    // interval and bounded pivot/order-block lookback, never the full history.
    const STRUCTURE_BARS: usize = 1_000_000;
    const STRUCTURE_TIP_SAMPLES: usize = 200;
    // Measured tip p99 is ~0.16 ms with all seven bindings: the canonical axis reads merged times
    // in place, without copying the 1M-row timeline on either synchronization pass.
    const STRUCTURE_TIP_BUDGET_MS: f64 = 8.0;
    const STRUCTURE_CORRECTION_ROWS: usize = 20_000;
    const STRUCTURE_CORRECTION_BUDGET_MS: f64 = 100.0;
    let (times, open, high, low, close) = {
        let (times, mut open, mut high, mut low, mut close) = gen_series(STRUCTURE_BARS, 11.0);
        // Periodic price jumps make the fixture produce real fair-value gaps and order blocks;
        // a smooth sinusoid never gaps, so the zone studies would measure empty work.
        let mut drift = 0.0;
        for row in 0..STRUCTURE_BARS {
            if row % 500 == 499 {
                drift += if (row / 500) % 2 == 0 { 6.0 } else { -6.0 };
            }
            open[row] += drift;
            high[row] += drift;
            low[row] += drift;
            close[row] += drift;
        }
        (times, open, high, low, close)
    };
    let mut structure = ChartEngine::new(1600.0, 800.0, 1.0);
    structure
        .set_series_data(0, &times, &open, &high, &low, &close)
        .expect("valid structure fixture");
    let started = Instant::now();
    let swing = structure.add_swing_points(0, 5, 5);
    structure.add_market_structure(0, 5, 5, StructureBreakOn::Close);
    let fvg = structure.add_fair_value_gaps(
        0,
        0.0,
        StructureMitigation::Touch,
        StructureMitigationPrice::Wick,
        20,
        true,
    );
    let order_blocks = structure.add_order_blocks(
        0,
        5,
        5,
        StructureBreakOn::Close,
        OrderBlockZone::Wick,
        StructureMitigation::Touch,
        StructureMitigationPrice::Wick,
        20,
        true,
    );
    structure.add_session_levels(0, StudyCalendarPolicy::Utc);
    structure.add_previous_period_levels(0, PreviousPeriod::Day, StudyCalendarPolicy::Utc);
    structure.add_opening_range(0, 3_600, StudyCalendarPolicy::Utc);
    let structure_build_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert!(
        !swing.is_empty() && !fvg.is_empty() && !order_blocks.is_empty(),
        "structure bindings are created"
    );
    let last_row = STRUCTURE_BARS - 1;
    let mut tip_samples = Vec::with_capacity(STRUCTURE_TIP_SAMPLES);
    for sample in 0..STRUCTURE_TIP_SAMPLES {
        let wobble = (sample as f64 * 0.37).sin() * 0.5;
        let tip_close = close[last_row] + wobble;
        let started = Instant::now();
        structure.update_series_bar(
            0,
            times[last_row],
            [tip_close - 0.2, tip_close + 0.4, tip_close - 0.4, tip_close],
        );
        tip_samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    tip_samples.sort_unstable_by(f64::total_cmp);
    let tip_p99_ms = tip_samples[((tip_samples.len() - 1) as f64 * 0.99).round() as usize];
    let correction_row = STRUCTURE_BARS - STRUCTURE_CORRECTION_ROWS;
    let started = Instant::now();
    structure.update_series_bar(
        0,
        times[correction_row],
        [
            open[correction_row] + 0.3,
            high[correction_row] + 0.9,
            low[correction_row] - 0.1,
            close[correction_row] + 0.3,
        ],
    );
    let structure_correction_ms = started.elapsed().as_secs_f64() * 1000.0;
    let fvg_annotations = structure
        .study_annotations(fvg[0])
        .expect("fvg annotations snapshot");
    let order_block_annotations = structure
        .study_annotations(order_blocks[0])
        .expect("order block annotations snapshot");
    println!(
        "Target N — 7 structure studies x {STRUCTURE_BARS} rows (initial build {structure_build_ms:.2} ms; {} FVG zones, {} order-block zones retained):",
        fvg_annotations.zones().len(),
        order_block_annotations.zones().len(),
    );
    let n_tip = report(
        "tail update p99 (tip replacement, all bindings)",
        tip_p99_ms,
        STRUCTURE_TIP_BUDGET_MS,
    );
    let n_correction = report(
        &format!("historical correction {STRUCTURE_CORRECTION_ROWS} rows back"),
        structure_correction_ms,
        STRUCTURE_CORRECTION_BUDGET_MS,
    );

    // ---- Target O: the Target M tape with auction markers bound to the same stream -----------
    let auction_tape = sustained_order_flow_tape(true);
    println!(
        "Target O — sustained live order-flow tape with auction markers ({} retained marks):",
        auction_tape.auction_marks
    );
    println!(
        "  update + frame p50 {:.3} ms, p99 {:.3} ms",
        auction_tape.update_frame_p50_ms, auction_tape.update_frame_p99_ms
    );
    let o_p99 = report("update + frame p99", auction_tape.update_frame_p99_ms, 4.0);
    let o_max = report(
        "worst update + frame (crosses retention ceiling)",
        auction_tape.update_frame_max_ms,
        FRAME_BUDGET_MS,
    );
    let o_late = report(
        &format!(
            "late print one bar back + frame ({} rebuilt ticks)",
            auction_tape.late_rebuilt_ticks
        ),
        auction_tape.late_trade_ms,
        FRAME_BUDGET_MS,
    );
    println!(
        "  [{}] retained tape stays within the order-flow ceiling",
        if auction_tape.retained_trades_capped {
            "PASS"
        } else {
            "FAIL"
        }
    );

    // ---- Target P: multi-session order-flow history -----------------------------------------
    let history = multi_session_order_flow_history();
    println!(
        "Target P — seven sessions of one-minute order flow (1.45M trades, footprint + CVD + delta + big trades, 1k-trade batches each followed by a frame):"
    );
    println!(
        "  retained {} bars ({} sealed) over {} sessions, {} raw trades",
        history.bars, history.sealed_bars, history.sessions, history.raw_trades
    );
    let p_p99 = report("batch + frame p99", history.batch_p99_ms, 8.0);
    let p_max = report(
        "worst batch + frame (sealing and session eviction)",
        history.batch_max_ms,
        FRAME_BUDGET_MS,
    );
    let p_memory = report_bytes(
        "order-flow stream memory",
        history.stream_bytes,
        ORDER_FLOW_MAX_STREAM_BYTES,
    );
    let p_retention = history.sessions == ORDER_FLOW_MAX_RETAINED_SESSIONS
        && history.raw_trades <= ORDER_FLOW_MAX_RETAINED_TRADES
        && history.sealed_bars > 0;
    println!(
        "  [{}] history keeps the newest {ORDER_FLOW_MAX_RETAINED_SESSIONS} sessions over a bounded raw tape",
        if p_retention { "PASS" } else { "FAIL" }
    );

    // ---- Target Q: live indicator updates for every IndicatorKind ---------------------------
    let q_pass = indicator_live::run();

    let all_pass = a_pass
        && b_pass
        && c_pass
        && d_pass
        && j_text_count
        && j_scene
        && e_refresh
        && e_cached
        && e_periodic
        && f_frame
        && f_hit
        && g_frame
        && g_hit
        && g_memory
        && h_frame
        && h_memory
        && i_frame
        && i_hit
        && i_memory
        && k_frame
        && k_flat_memory
        && l_update
        && l_frame
        && l_upload
        && l_flat_memory
        && m_p99
        && m_max
        && m_late
        && live_tape.retained_trades_capped
        && n_tip
        && n_correction
        && o_p99
        && o_max
        && o_late
        && auction_tape.retained_trades_capped
        && p_p99
        && p_max
        && p_memory
        && p_retention
        && q_pass;
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
