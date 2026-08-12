use std::time::Instant;

use nucleuscharts_core::scale::time_scale_core::{TimeScaleCore, TimeScaleOptions};
use nucleuscharts_engine::{ChartEngine, ChartFrame, SeriesKind};

type FixtureColumns = (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>);

fn fixture(bars: usize, phase: f64) -> FixtureColumns {
    let mut times = Vec::with_capacity(bars);
    let mut open = Vec::with_capacity(bars);
    let mut high = Vec::with_capacity(bars);
    let mut low = Vec::with_capacity(bars);
    let mut close = Vec::with_capacity(bars);
    let mut price = 100.0 + phase;
    for i in 0..bars {
        let wobble = ((i as f64 + phase) * 0.017).sin() * 0.8;
        let next = price + wobble;
        times.push(i as f64);
        open.push(price);
        high.push(price.max(next) + 0.4);
        low.push(price.min(next) - 0.4);
        close.push(next);
        price = next;
    }
    (times, open, high, low, close)
}

fn install_chart(bars: usize, width: f64, height: f64) -> ChartEngine {
    let (times, open, high, low, close) = fixture(bars, 0.0);
    let mut chart = ChartEngine::new(width, height, 1.0);
    chart
        .set_series_data(0, &times, &open, &high, &low, &close)
        .expect("valid fixture");
    chart.time_scale.set_width(width);
    chart.fit_content();
    chart
}

fn interaction_benchmark<F>(chart: &mut ChartEngine, label: &str, iterations: usize, mut update: F)
where
    F: FnMut(&mut ChartEngine, usize),
{
    let mut frame = ChartFrame::default();
    chart.build_frame_into(&mut frame);
    let mut samples = Vec::with_capacity(iterations);
    for i in 0..iterations {
        update(chart, i);
        let start = Instant::now();
        chart.build_frame_into(&mut frame);
        samples.push(start.elapsed().as_secs_f64() * 1_000_000.0);
    }
    samples.sort_by(f64::total_cmp);
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    let p95 = samples[((samples.len() - 1) * 95) / 100];
    let max = *samples.last().unwrap();
    println!("{label}: {iterations} frames, mean {mean:.3} us, p95 {p95:.3} us, max {max:.3} us");
}

fn main() {
    let bars: usize = std::env::var("NUCLEUSCHARTS_BARS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1_000_000);
    let iterations: usize = std::env::var("NUCLEUSCHARTS_INTERACTIONS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(240);

    let mut chart = install_chart(bars, 1600.0, 800.0);
    println!(
        "single series: {bars} installed bars, {:.3} CSS px/bar",
        chart.time_scale.bar_spacing()
    );
    interaction_benchmark(&mut chart, "1M pan", iterations, |chart, i| {
        chart.time_scale.start_scroll(800.0);
        chart
            .time_scale
            .scroll_to(if i % 2 == 0 { 799.0 } else { 801.0 });
        chart.time_scale.end_scroll();
    });
    interaction_benchmark(&mut chart, "1M zoom", iterations, |chart, i| {
        chart
            .time_scale
            .zoom(800.0, if i % 2 == 0 { 0.01 } else { -0.00990099 });
    });
    interaction_benchmark(&mut chart, "1M crosshair", iterations, |chart, i| {
        chart.crosshair = Some((40.0 + (i % 1520) as f64, 300.0 + (i % 120) as f64));
    });

    // Stress the roadmap's 10-series × 50k-visible-bars target explicitly. The normal product
    // minimum spacing is 0.5 CSS px; this fixture lowers it to 0.08 so all 50k bars are visible,
    // exercising the conflator rather than silently benchmarking only the viewport's 3,200 bars.
    let visible_bars = 50_000usize;
    let (times, open, high, low, close) = fixture(visible_bars, 0.0);
    let mut multi = ChartEngine::new(4000.0, 900.0, 1.0);
    multi.time_scale = TimeScaleCore::new(TimeScaleOptions {
        bar_spacing: 0.08,
        min_bar_spacing: 0.08,
        ..TimeScaleOptions::default()
    });
    multi.series[0].kind = SeriesKind::Line;
    multi
        .set_series_data(0, &times, &open, &high, &low, &close)
        .expect("valid multi fixture");
    for series in 1..10 {
        let id = multi.add_series(SeriesKind::Line);
        let (t, o, h, l, c) = fixture(visible_bars, series as f64);
        multi
            .set_series_data(id, &t, &o, &h, &l, &c)
            .expect("valid multi fixture");
    }
    multi.time_scale.set_width(4000.0);
    multi.fit_content();
    println!(
        "10-series stress: {} visible logical bars, {:.3} CSS px/bar",
        visible_bars,
        multi.time_scale.bar_spacing()
    );
    interaction_benchmark(&mut multi, "10x50k pan", iterations, |chart, i| {
        chart.time_scale.start_scroll(2000.0);
        chart
            .time_scale
            .scroll_to(if i % 2 == 0 { 1999.0 } else { 2001.0 });
        chart.time_scale.end_scroll();
    });
    interaction_benchmark(&mut multi, "10x50k zoom", iterations, |chart, i| {
        chart
            .time_scale
            .zoom(2000.0, if i % 2 == 0 { 0.01 } else { -0.00990099 });
    });
    interaction_benchmark(&mut multi, "10x50k crosshair", iterations, |chart, i| {
        chart.crosshair = Some((100.0 + (i % 3800) as f64, 400.0));
    });
}
