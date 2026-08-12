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
            "replace_current_candle_us": replace_samples_us
        }))
        .expect("JSON serializes")
    );
}
