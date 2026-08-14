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

fn memory_density_rows() -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    for points in [10_000, 100_000, 1_000_000] {
        let columns = generate(points, SEED);
        for indicator in ["none", "rsi", "macd", "all"] {
            let mut chart = ChartEngine::new(1280.0, 720.0, 1.0);
            let started = Instant::now();
            install(&mut chart, &columns);
            let source_install_ms = started.elapsed().as_secs_f64() * 1_000.0;
            let source = chart
                .data_layer()
                .series_memory_usage(0)
                .expect("main source exists");
            let started = Instant::now();
            add_indicator_set(&mut chart, indicator);
            let indicator_install_ms = started.elapsed().as_secs_f64() * 1_000.0;
            let mut frame = ChartFrame::default();
            chart.build_frame_into(&mut frame);
            let memory = chart.memory_usage();
            let time = *columns.times.last().unwrap();
            let close = *columns.close.last().unwrap();
            let mut current_update_us = Vec::with_capacity(10);
            for run in 0..10 {
                let value = close + run as f64 * 0.000_01;
                let started = Instant::now();
                chart.update_series_bar(0, time, [value, value + 0.2, value - 0.2, value]);
                current_update_us.push(started.elapsed().as_secs_f64() * 1_000_000.0);
            }
            rows.push(json!({
                "rows": points,
                "series_type": "ohlc",
                "indicator_config": indicator,
                "source_time_bytes": source.owned_time_bytes,
                "canonical_source_value_bytes": source.canonical_value_bytes,
                "canonical_value_bytes": memory.data.canonical_value_bytes,
                "lod_bytes": memory.data.lod_bytes,
                "derived_output_bytes": memory.data.canonical_value_bytes - source.canonical_value_bytes,
                "index_bytes": memory.data.plot_index_bytes,
                "merged_time_bytes": memory.data.merged_time_bytes,
                "time_bytes": memory.data.owned_time_bytes + memory.data.merged_time_bytes,
                "tick_payload_bytes": memory.tick_payload_bytes,
                "derived_runtime_bytes": memory.indicator_runtime_bytes,
                "scratch_capacity_bytes": memory.data.scratch_capacity_bytes + memory.indicator_transfer_capacity_bytes,
                "frame_retained_capacity_bytes": memory.retained_frame_capacity_bytes,
                "engine_live_payload_bytes": memory.estimated_live_bytes(),
                "engine_allocated_capacity_bytes": memory.data.allocated_capacity_bytes
                    + memory.tick_capacity_bytes
                    + memory.indicator_runtime_bytes
                    + memory.indicator_transfer_capacity_bytes
                    + memory.retained_frame_capacity_bytes,
                "aligned_series": memory.data.aligned_series,
                "dense_index_series": memory.data.dense_index_series,
                "source_install_ms": source_install_ms,
                "indicator_install_ms": indicator_install_ms,
                "current_update_us": current_update_us,
            }));
        }
    }
    rows
}

fn historical_correction_rows() -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    for points in [100_000, 1_000_000] {
        let columns = generate(points, SEED);
        for indicator in ["rsi", "all"] {
            let mut chart = ChartEngine::new(1280.0, 720.0, 1.0);
            install(&mut chart, &columns);
            add_indicator_set(&mut chart, indicator);
            for (position, row) in [
                ("last", points - 1),
                ("last_10", points - 10),
                ("middle", points / 2),
                ("quarter", points / 4),
                ("three_quarters", points * 3 / 4),
                ("warmup", 2),
            ] {
                let close = columns.close[row] + 0.001;
                let started = Instant::now();
                chart.update_series_bar(
                    0,
                    columns.times[row],
                    [close, close + 0.2, close - 0.2, close],
                );
                rows.push(json!({
                    "rows": points,
                    "indicator_config": indicator,
                    "position": position,
                    "source_row": row,
                    "work_rows": chart.last_indicator_work_rows(),
                    "elapsed_ms": started.elapsed().as_secs_f64() * 1_000.0,
                }));
            }
        }
    }
    rows
}

fn multi_series_memory_rows() -> Vec<serde_json::Value> {
    let points = 10_000;
    let columns = generate(points, SEED);
    let mut rows = Vec::new();
    for aligned in [true, false] {
        for count in [1, 2, 4, 8, 16] {
            let mut chart = ChartEngine::new(1280.0, 720.0, 1.0);
            let mut ids = vec![0];
            ids.extend(
                (1..count).map(|_| chart.add_series(nucleuscharts_engine::SeriesKind::Candlestick)),
            );
            for (index, id) in ids.into_iter().enumerate() {
                if aligned || index == 0 {
                    install_series(&mut chart, id, &columns, 0.0);
                } else {
                    install_series(&mut chart, id, &columns, index as f64);
                }
            }
            let memory = chart.memory_usage();
            rows.push(json!({
                "series_count": count,
                "alignment": if aligned { "aligned" } else { "independent" },
                "rows_per_series": points,
                "canonical_value_bytes": memory.data.canonical_value_bytes,
                "lod_bytes": memory.data.lod_bytes,
                "owned_time_bytes": memory.data.owned_time_bytes,
                "merged_time_bytes": memory.data.merged_time_bytes,
                "index_bytes": memory.data.plot_index_bytes,
                "engine_live_payload_bytes": memory.estimated_live_bytes(),
                "dense_index_series": memory.data.dense_index_series,
            }));
        }
    }
    rows
}

fn install_series(chart: &mut ChartEngine, id: u32, columns: &Columns, time_offset: f64) {
    let times = if time_offset == 0.0 {
        columns.times.clone()
    } else {
        columns
            .times
            .iter()
            .map(|time| time + time_offset)
            .collect()
    };
    chart
        .set_series_data(
            id,
            &times,
            &columns.open,
            &columns.high,
            &columns.low,
            &columns.close,
        )
        .expect("deterministic fixture is valid");
}

fn multi_chart_memory_rows() -> Vec<serde_json::Value> {
    let columns = generate(10_000, SEED);
    [1, 2, 4, 8, 16]
        .into_iter()
        .map(|count| {
            let charts = (0..count)
                .map(|_| {
                    let mut chart = ChartEngine::new(1280.0, 720.0, 1.0);
                    install(&mut chart, &columns);
                    chart
                })
                .collect::<Vec<_>>();
            json!({
                "chart_count": count,
                "rows_per_chart": columns.times.len(),
                "engine_live_payload_bytes": charts.iter().map(|chart| chart.memory_usage().estimated_live_bytes()).sum::<usize>(),
                "allocated_capacity_bytes": charts.iter().map(|chart| {
                    let memory = chart.memory_usage();
                    memory.data.allocated_capacity_bytes + memory.tick_capacity_bytes + memory.retained_frame_capacity_bytes
                }).sum::<usize>(),
            })
        })
        .collect()
}

fn retention_and_lifecycle() -> serde_json::Value {
    let columns = generate(100_000, SEED);
    let mut chart = ChartEngine::new(1280.0, 720.0, 1.0);
    install(&mut chart, &columns);
    chart.add_rsi(0, 14);
    let before = chart.memory_usage();
    chart.set_series_max_points(0, Some(10_000));
    let retained = chart.memory_usage();
    let removed = chart.remove_series(0);
    let after_remove = chart.memory_usage();

    let cycle_columns = generate(10_000, SEED);
    let started = Instant::now();
    for _ in 0..20 {
        let mut cycle = ChartEngine::new(1280.0, 720.0, 1.0);
        install(&mut cycle, &cycle_columns);
        cycle.add_rsi(0, 14);
    }
    json!({
        "loaded_rows": 100_000,
        "retained_rows": retained.data.rows,
        "before_live_bytes": before.estimated_live_bytes(),
        "retained_live_bytes": retained.estimated_live_bytes(),
        "series_removed": removed,
        "after_remove_live_bytes": after_remove.estimated_live_bytes(),
        "create_destroy_cycles": 20,
        "create_destroy_elapsed_ms": started.elapsed().as_secs_f64() * 1_000.0,
    })
}

fn dense_upload_breakdown(full_history: bool) -> serde_json::Value {
    let columns = generate(1_000_000, SEED);
    let mut chart = ChartEngine::new(1280.0, 720.0, 1.0);
    install(&mut chart, &columns);
    if full_history {
        chart.set_min_bar_spacing(1280.0 / columns.times.len() as f64 / 2.0);
        chart.set_visible_logical_range(0.0, columns.times.len() as f64 - 1.0);
    }
    let mut frame = ChartFrame::default();
    chart.build_frame_into(&mut frame);
    let lod_work = chart.lod_work_stats();
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
        "selected_lod_level": lod_work.selected_level,
        "summary_operations": lod_work.summary_nodes,
        "raw_rows_inspected": lod_work.raw_rows,
    })
}

fn percentiles(mut samples: Vec<f64>) -> serde_json::Value {
    samples.sort_by(f64::total_cmp);
    let at = |quantile: f64| {
        let index = ((samples.len() as f64 * quantile).ceil() as usize)
            .saturating_sub(1)
            .min(samples.len() - 1);
        samples[index]
    };
    json!({ "p50": at(0.50), "p95": at(0.95), "p99": at(0.99) })
}

fn dense_complexity_rows() -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    for points in [10_000, 100_000, 500_000, 1_000_000] {
        let columns = generate(points, SEED);
        for series_count in [1, 4, 8] {
            for (zoom, visible_points) in [
                ("normal", points.min(500)),
                ("moderate", points.min(50_000)),
                ("full", points),
            ] {
                if series_count > 1 && zoom != "full" {
                    continue;
                }
                let mut chart = ChartEngine::new(1280.0, 720.0, 1.0);
                chart.set_min_bar_spacing(1280.0 / points as f64 / 2.0);
                let mut ids = vec![0];
                ids.extend(
                    (1..series_count)
                        .map(|_| chart.add_series(nucleuscharts_engine::SeriesKind::Candlestick)),
                );
                for &id in &ids {
                    install_series(&mut chart, id, &columns, 0.0);
                }
                chart.time_scale.set_width(1280.0);
                chart.fit_content();
                let start = (points - visible_points) / 2;
                let end = start + visible_points - 1;
                chart.set_visible_logical_range(start as f64, end as f64);
                let mut frame = ChartFrame::default();
                chart.build_frame_into(&mut frame);
                let lod_work = chart.lod_work_stats();

                let (from, to) = chart.visible_range().expect("installed range is visible");
                let visible_source_rows = ids
                    .iter()
                    .map(|&id| chart.data_layer().plot(id).visible_rows(from, to).len())
                    .sum::<usize>();
                let primitive_count = chart
                    .frame_series_segments(0)
                    .iter()
                    .map(|segment| segment.end - segment.start)
                    .sum::<usize>();

                let mut frame_ms = Vec::with_capacity(MEASURED_RUNS);
                let mut pan_ms = Vec::with_capacity(MEASURED_RUNS);
                let mut zoom_ms = Vec::with_capacity(MEASURED_RUNS);
                let mut crosshair_us = Vec::with_capacity(MEASURED_RUNS);
                let mut hit_test_us = Vec::with_capacity(MEASURED_RUNS);
                for run in 0..MEASURED_RUNS {
                    let fractional = if run % 2 == 0 { 0.0 } else { 0.25 };
                    let started = Instant::now();
                    chart.set_visible_logical_range(
                        start as f64 + fractional,
                        end as f64 + fractional,
                    );
                    chart.build_frame_into(&mut frame);
                    frame_ms.push(started.elapsed().as_secs_f64() * 1_000.0);

                    let shift = if run % 2 == 0 { -3.0 } else { 3.0 };
                    let started = Instant::now();
                    chart.set_visible_logical_range(start as f64 + shift, end as f64 + shift);
                    chart.build_frame_into(&mut frame);
                    pan_ms.push(started.elapsed().as_secs_f64() * 1_000.0);

                    let zoom_delta = visible_points as f64 * 0.01;
                    let started = Instant::now();
                    chart.set_visible_logical_range(
                        start as f64 - zoom_delta,
                        end as f64 + zoom_delta,
                    );
                    chart.build_frame_into(&mut frame);
                    zoom_ms.push(started.elapsed().as_secs_f64() * 1_000.0);

                    let started = Instant::now();
                    chart.set_crosshair_at(640.0 + f64::from(run as u32 % 7), 360.0);
                    chart.build_frame_into(&mut frame);
                    crosshair_us.push(started.elapsed().as_secs_f64() * 1_000_000.0);

                    let started = Instant::now();
                    let _ = chart.hit_test_one_series(0, 640.0, 360.0);
                    hit_test_us.push(started.elapsed().as_secs_f64() * 1_000_000.0);
                }
                chart.set_visible_logical_range(start as f64, end as f64);
                chart.build_frame_into(&mut frame);
                let last_time = *columns.times.last().expect("fixture is non-empty");
                let last_close = *columns.close.last().expect("fixture is non-empty");
                let started = Instant::now();
                assert!(chart.update_series_bar(
                    0,
                    last_time,
                    [last_close, last_close + 0.25, last_close - 0.25, last_close],
                ));
                let current_update_us = started.elapsed().as_secs_f64() * 1_000_000.0;
                let lod_nodes_updated = chart
                    .data_layer()
                    .last_lod_update_nodes(0)
                    .expect("source exists");
                let started = Instant::now();
                chart.build_frame_into(&mut frame);
                let current_frame_ms = started.elapsed().as_secs_f64() * 1_000.0;
                let current_lod_work = chart.lod_work_stats();
                rows.push(json!({
                    "rows": points,
                    "visible_rows": visible_points,
                    "series_count": series_count,
                    "viewport_width": 1280,
                    "zoom": zoom,
                    "visible_source_rows": visible_source_rows,
                    "source_rows_inspected": lod_work.raw_rows,
                    "selected_lod_level": lod_work.selected_level,
                    "summary_operations": lod_work.summary_nodes,
                    "lod_candidates": lod_work.candidates,
                    "primitive_count": primitive_count,
                    "frame_ms": percentiles(frame_ms),
                    "pan_ms": percentiles(pan_ms),
                    "zoom_ms": percentiles(zoom_ms),
                    "crosshair_us": percentiles(crosshair_us),
                    "hit_test_us": percentiles(hit_test_us),
                    "current_update_us": current_update_us,
                    "current_lod_nodes_updated": lod_nodes_updated,
                    "current_frame_ms": current_frame_ms,
                    "current_summary_operations": current_lod_work.summary_nodes,
                    "current_raw_rows_inspected": current_lod_work.raw_rows,
                }));
            }
        }
    }
    rows
}

fn dense_width_rows() -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    for points in [1_000, 10_000, 50_000, 100_000, 500_000, 1_000_000] {
        let columns = generate(points, SEED);
        for viewport_width in [800, 1_280, 1_920, 3_840] {
            let width = f64::from(viewport_width);
            let mut chart = ChartEngine::new(width, 720.0, 1.0);
            install(&mut chart, &columns);
            chart.time_scale.set_width(width);
            chart.set_min_bar_spacing(width / points as f64 / 2.0);
            chart.set_visible_logical_range(0.0, points as f64 - 1.0);
            let mut frame = ChartFrame::default();
            chart.build_frame_into(&mut frame);
            let work = chart.lod_work_stats();
            let primitive_count = chart
                .frame_series_segments(0)
                .iter()
                .map(|segment| segment.end - segment.start)
                .sum::<usize>();
            let mut frame_ms = Vec::with_capacity(5);
            for run in 0..5 {
                let offset = if run % 2 == 0 { 0.0 } else { 0.25 };
                let started = Instant::now();
                chart.set_visible_logical_range(offset, points as f64 - 1.0 + offset);
                chart.build_frame_into(&mut frame);
                frame_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
            }
            rows.push(json!({
                "rows": points,
                "viewport_width": viewport_width,
                "selected_lod_level": work.selected_level,
                "summary_operations": work.summary_nodes,
                "raw_rows_inspected": work.raw_rows,
                "lod_candidates": work.candidates,
                "primitive_count": primitive_count,
                "frame_ms": percentiles(frame_ms),
            }));
        }
    }
    rows
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
            "memory_density": memory_density_rows(),
            "historical_corrections": historical_correction_rows(),
            "multi_series_memory": multi_series_memory_rows(),
            "multi_chart_memory": multi_chart_memory_rows(),
            "retention_and_lifecycle": retention_and_lifecycle(),
            "dense_upload": dense_upload_breakdown(false),
            "full_history_dense_upload": dense_upload_breakdown(true),
            "dense_complexity": dense_complexity_rows(),
            "dense_width_matrix": dense_width_rows(),
        }))
        .expect("JSON serializes")
    );
}
