use std::time::Instant;

use nucleuscharts_engine::{ChartEngine, ChartFrame};
use serde_json::json;

const POINTS: usize = 100_000;
const SEED: u32 = 0x02f6_e2b1;
const WARMUP_RUNS: usize = 5;
const MEASURED_RUNS: usize = 30;

struct Columns {
    times: Vec<f64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
}

fn random(state: &mut u32) -> f64 {
    let mut value = *state;
    value ^= value << 13;
    value ^= value >> 17;
    value ^= value << 5;
    *state = value;
    f64::from(value) / 4_294_967_296.0
}

fn generate(points: usize, seed: u32) -> Columns {
    let mut state = seed.max(1);
    let mut columns = Columns {
        times: Vec::with_capacity(points),
        open: Vec::with_capacity(points),
        high: Vec::with_capacity(points),
        low: Vec::with_capacity(points),
        close: Vec::with_capacity(points),
    };
    let mut price = 100.0_f64;
    for index in 0..points {
        let noise = (random(&mut state) - 0.5) * 0.8;
        let drift = (index as f64 * 0.017).sin() * 0.8;
        let next = (price + drift + noise).max(0.01);
        let wick = 0.1 + random(&mut state) * 0.8;
        columns.times.push(1_577_836_800.0 + index as f64 * 60.0);
        columns.open.push(price);
        columns.high.push(price.max(next) + wick);
        columns.low.push((price.min(next) - wick).max(0.0));
        columns.close.push(next);
        price = next;
        let _volume = 100 + (random(&mut state) * 9_900.0).floor() as u32;
    }
    columns
}

fn install(chart: &mut ChartEngine, columns: &Columns) {
    chart
        .set_series_data(
            0,
            &columns.times,
            &columns.open,
            &columns.high,
            &columns.low,
            &columns.close,
        )
        .expect("deterministic fixture is valid");
    chart.time_scale.set_width(1280.0);
    chart.fit_content();
}

fn add_indicator_set(chart: &mut ChartEngine, name: &str) {
    match name {
        "none" => {}
        "sma" => {
            chart.add_sma(0, 20);
        }
        "rsi" => {
            chart.add_rsi(0, 14);
        }
        "macd" => {
            chart.add_macd(0, 12, 26, 9);
        }
        "atr" => {
            chart.add_atr(0, 14);
        }
        "vwap" => {
            chart.add_vwap(0, None);
        }
        "all" => {
            chart.add_sma(0, 20);
            chart.add_ema(0, 20);
            chart.add_bollinger(0, 20, 2.0);
            chart.add_rsi(0, 14);
            chart.add_macd(0, 12, 26, 9);
            chart.add_stochastic(0, 14, 3);
            chart.add_atr(0, 14);
            chart.add_vwap(0, None);
            chart.add_wma(0, 20);
        }
        _ => unreachable!("known benchmark indicator set"),
    }
}

fn indicator_current_rows() -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    for points in [10_000, 100_000, 1_000_000] {
        let columns = generate(points, SEED);
        for indicator in ["none", "sma", "rsi", "macd", "atr", "vwap", "all"] {
            let mut chart = ChartEngine::new(1280.0, 720.0, 1.0);
            install(&mut chart, &columns);
            add_indicator_set(&mut chart, indicator);
            let time = *columns.times.last().unwrap();
            let close = *columns.close.last().unwrap();
            for warmup in 0..5 {
                let value = close + warmup as f64 * 0.0001;
                chart.update_series_bar(0, time, [value, value + 0.2, value - 0.2, value]);
            }
            let mut samples = Vec::with_capacity(MEASURED_RUNS);
            for run in 0..MEASURED_RUNS {
                let value = close + (run as f64 * 0.17).sin() * 0.02;
                let started = Instant::now();
                chart.update_series_bar(0, time, [value, value + 0.2, value - 0.2, value]);
                samples.push(started.elapsed().as_secs_f64() * 1_000_000.0);
            }
            rows.push(json!({ "points": points, "indicator": indicator, "samples_us": samples }));
        }
    }
    rows
}

fn indicator_batch_rows() -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    for points in [10_000, 100_000, 1_000_000] {
        let columns = generate(points, SEED);
        for indicator in ["none", "sma", "rsi", "macd", "atr", "vwap", "all"] {
            for batch_size in [1, 10, 100, 1_000, 10_000] {
                let mut chart = ChartEngine::new(1280.0, 720.0, 1.0);
                install(&mut chart, &columns);
                add_indicator_set(&mut chart, indicator);
                let mut samples = Vec::with_capacity(MEASURED_RUNS);
                for run in 0..WARMUP_RUNS + MEASURED_RUNS {
                    let start = points + run * batch_size;
                    let times = (0..batch_size)
                        .map(|offset| (1_577_836_800 + (start + offset) * 60) as i64)
                        .collect::<Vec<_>>();
                    let close = (0..batch_size)
                        .map(|offset| 100.0 + ((start + offset) as f64 * 0.017).sin())
                        .collect::<Vec<_>>();
                    let open = close.clone();
                    let high = close.iter().map(|value| value + 0.2).collect();
                    let low = close.iter().map(|value| value - 0.2).collect();
                    let started = Instant::now();
                    chart.update_series_bars_sanitized(0, times, open, high, low, close);
                    let elapsed = started.elapsed().as_secs_f64() * 1_000_000.0;
                    if run >= WARMUP_RUNS {
                        samples.push(elapsed);
                    }
                }
                rows.push(json!({
                    "points": points,
                    "indicator": indicator,
                    "batch_size": batch_size,
                    "samples_us": samples,
                }));
            }
        }
    }
    rows
}

fn indicator_scaling_rows() -> Vec<serde_json::Value> {
    let columns = generate(100_000, SEED);
    let mut rows = Vec::new();
    for count in [1, 4, 8, 16] {
        let mut chart = ChartEngine::new(1280.0, 720.0, 1.0);
        install(&mut chart, &columns);
        for index in 0..count {
            match index % 5 {
                0 => {
                    chart.add_rsi(0, 14);
                }
                1 => {
                    chart.add_macd(0, 12, 26, 9);
                }
                2 => {
                    chart.add_atr(0, 14);
                }
                3 => {
                    chart.add_vwap(0, None);
                }
                _ => {
                    chart.add_wma(0, 20);
                }
            }
        }
        let time = *columns.times.last().unwrap();
        let close = *columns.close.last().unwrap();
        let mut samples = Vec::with_capacity(MEASURED_RUNS);
        for run in 0..MEASURED_RUNS {
            let value = close + run as f64 * 0.0001;
            let started = Instant::now();
            chart.update_series_bar(0, time, [value, value + 0.2, value - 0.2, value]);
            samples.push(started.elapsed().as_secs_f64() * 1_000_000.0);
        }
        rows.push(json!({ "indicator_count": count, "samples_us": samples }));
    }
    rows
}

fn indicator_source_scaling_rows() -> Vec<serde_json::Value> {
    let columns = generate(10_000, SEED);
    let mut rows = Vec::new();
    for source_count in [1, 2, 4, 8, 16] {
        let mut chart = ChartEngine::new(1280.0, 720.0, 1.0);
        let mut sources = vec![0];
        sources.extend(
            (1..source_count).map(|_| chart.add_series(nucleuscharts_engine::SeriesKind::Line)),
        );
        for &source in &sources {
            chart
                .set_series_data(
                    source,
                    &columns.times,
                    &columns.open,
                    &columns.high,
                    &columns.low,
                    &columns.close,
                )
                .unwrap();
            chart.add_rsi(source, 14);
        }
        let time = *columns.times.last().unwrap();
        let close = *columns.close.last().unwrap();
        let mut samples = Vec::with_capacity(MEASURED_RUNS);
        for run in 0..MEASURED_RUNS {
            let value = close + run as f64 * 0.0001;
            let started = Instant::now();
            chart.update_series_bar(0, time, [value, value + 0.2, value - 0.2, value]);
            samples.push(started.elapsed().as_secs_f64() * 1_000_000.0);
        }
        rows.push(json!({
            "source_count": source_count,
            "pane_count": chart.panes.len(),
            "samples_us": samples,
        }));
    }
    rows
}

fn dense_upload_breakdown() -> serde_json::Value {
    let columns = generate(1_000_000, SEED);
    let mut chart = ChartEngine::new(1280.0, 720.0, 1.0);
    install(&mut chart, &columns);
    let mut frame = ChartFrame::default();
    chart.build_frame_into(&mut frame);
    let time = *columns.times.last().unwrap();
    let close = *columns.close.last().unwrap();
    chart.update_series_bar(0, time, [close, close + 0.2, close - 0.2, close]);
    chart.build_frame_into(&mut frame);
    let segment = chart
        .frame_series_segments(0)
        .iter()
        .find(|segment| segment.series_id == Some(0))
        .unwrap();
    let pane = &frame.panes[0];
    let mut group = nucleuscharts_render_wgpu::DrawGroup::default();
    nucleuscharts_render_wgpu::prims_to_group(
        &pane.main[segment.start..segment.end],
        &pane.points,
        &mut group,
        &mut |_| None,
    );
    let triangle_bytes =
        group.tris.len() * std::mem::size_of::<nucleuscharts_render_wgpu::TriVertex>();
    let quad_bytes =
        group.quads.len() * std::mem::size_of::<nucleuscharts_render_wgpu::QuadInstance>();
    let textured_bytes =
        group.tex_quads.len() * std::mem::size_of::<nucleuscharts_render_wgpu::TexQuadInstance>();
    let visible_bars = chart.time_scale.visible_strict_range().map_or(0, |range| {
        (range.right() - range.left() + 1).max(0) as usize
    });
    json!({
        "visible_bars": visible_bars,
        "primitive_count": segment.end - segment.start,
        "triangle_vertices": group.tris.len(),
        "quad_instances": group.quads.len(),
        "textured_quad_instances": group.tex_quads.len(),
        "encoded_vertex_count": group.tris.len() + group.quads.len() * 6 + group.tex_quads.len() * 6,
        "triangle_bytes": triangle_bytes,
        "quad_bytes": quad_bytes,
        "textured_bytes": textured_bytes,
        "total_bytes": triangle_bytes + quad_bytes + textured_bytes,
    })
}

fn main() {
    let columns = generate(POINTS, SEED);
    let mut load_samples_ms = Vec::with_capacity(MEASURED_RUNS);
    for run in 0..WARMUP_RUNS + MEASURED_RUNS {
        let mut chart = ChartEngine::new(1280.0, 720.0, 1.0);
        let started = Instant::now();
        install(&mut chart, &columns);
        if run >= WARMUP_RUNS {
            load_samples_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
        }
    }

    let mut chart = ChartEngine::new(1280.0, 720.0, 1.0);
    install(&mut chart, &columns);
    let mut frame = ChartFrame::default();
    for _ in 0..WARMUP_RUNS {
        chart.build_frame_into(&mut frame);
    }
    let mut frame_samples_ms = Vec::with_capacity(MEASURED_RUNS);
    for _ in 0..MEASURED_RUNS {
        let started = Instant::now();
        chart.build_frame_into(&mut frame);
        frame_samples_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
    }

    let last_time = *columns.times.last().expect("non-empty fixture");
    let last_close = *columns.close.last().expect("non-empty fixture");
    let mut replace_samples_us = Vec::with_capacity(MEASURED_RUNS);
    for run in 0..MEASURED_RUNS {
        let value = last_close + (run as f64 * 0.17).sin() * 0.02;
        let started = Instant::now();
        assert!(chart.update_series_bar(0, last_time, [value, value + 0.2, value - 0.2, value]));
        replace_samples_us.push(started.elapsed().as_secs_f64() * 1_000_000.0);
    }

    println!(
        "{}",
        serde_json::to_string(&json!({
            "generator_version": 1,
            "seed": SEED,
            "points": POINTS,
            "warmup_runs": WARMUP_RUNS,
            "measured_runs": MEASURED_RUNS,
            "set_series_data_ms": load_samples_ms,
            "build_frame_ms": frame_samples_ms,
            "replace_current_candle_us": replace_samples_us,
            "indicator_current": indicator_current_rows(),
            "indicator_batch": indicator_batch_rows(),
            "indicator_scaling": indicator_scaling_rows(),
            "indicator_source_scaling": indicator_source_scaling_rows(),
            "dense_upload": dense_upload_breakdown(),
        }))
        .expect("JSON serializes")
    );
}
