use std::hint::black_box;
use std::time::Instant;

use aeris_charts_engine::{
    AxisDimension, ChartEngine, GeneralAxisOptions, GeneralHitMode, GeneralRowId, GeneralScaleType,
    GeneralSeriesOptions, GeneralXyInput, HorizontalDomain,
};

const CATEGORIES: usize = 4_096;
const SAMPLES: usize = 256;

pub fn run() -> bool {
    const HIT_BUDGET_MS: f64 = 0.05;
    const CACHE_BUDGET_BYTES: usize = 1024 * 1024;
    let mut elapsed = [0.0; 3];
    let mut passed = true;
    for (kind_index, kind) in ["pie", "radar", "polar_area"].into_iter().enumerate() {
        let mut chart =
            ChartEngine::new_with_initial_domain(400.0, 400.0, 1.0, HorizontalDomain::Polar)
                .expect("polar chart");
        let mut angle = GeneralAxisOptions::new(
            "angle",
            0,
            AxisDimension::Angle,
            GeneralScaleType::AngularCategory,
        );
        angle.visible = false;
        chart.add_general_axis(angle).expect("angular axis");
        let mut radius = GeneralAxisOptions::new(
            "radius",
            0,
            AxisDimension::Radius,
            GeneralScaleType::RadialLinear,
        );
        radius.visible = false;
        chart.add_general_axis(radius).expect("radial axis");
        let dataset = chart
            .create_general_xy_dataset(GeneralXyInput::Category {
                ids: None,
                categories: (0..CATEGORIES).map(|index| format!("C{index}")).collect(),
                category_indices: (0..CATEGORIES as u32).collect(),
                y: (0..CATEGORIES)
                    .map(|index| 1.0 + (index % 8) as f64)
                    .collect(),
                y_valid: None,
            })
            .expect("polar dataset");
        let options = match kind {
            "pie" => GeneralSeriesOptions::pie(0, dataset, "angle", "radius"),
            "radar" => GeneralSeriesOptions::radar(0, dataset, "angle", "radius"),
            _ => GeneralSeriesOptions::polar_area(0, dataset, "angle", "radius"),
        };
        chart.add_general_series(options).expect("polar series");
        chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
        let start = Instant::now();
        let mut hits = 0;
        for sample in 0..SAMPLES {
            let angle = std::f64::consts::TAU * sample as f64 / SAMPLES as f64;
            let x = 200.0 + 60.0 * angle.sin();
            let y = 200.0 - 60.0 * angle.cos();
            let hit = black_box(chart.general_hit_test(
                0,
                black_box(x),
                black_box(y),
                GeneralHitMode::Exact,
            ));
            hits += usize::from(hit.is_some());
        }
        elapsed[kind_index] = start.elapsed().as_secs_f64() * 1_000.0 / SAMPLES as f64;
        let cache_bytes = chart.memory_usage().general_series_capacity_bytes;
        let hit_pass = hits > 0 && elapsed[kind_index] <= HIT_BUDGET_MS;
        let cache_pass = cache_bytes <= CACHE_BUDGET_BYTES;
        println!(
            "  [{}] polar {kind}, {CATEGORIES} categories: {:.4} ms/hit ({hits}/{SAMPLES} hits, budget {HIT_BUDGET_MS:.2} ms)",
            if hit_pass { "PASS" } else { "FAIL" },
            elapsed[kind_index],
        );
        println!(
            "  [{}] polar {kind} series cache: {:.2} MiB (budget 1.00 MiB)",
            if cache_pass { "PASS" } else { "FAIL" },
            cache_bytes as f64 / (1024.0 * 1024.0),
        );
        passed &= hit_pass && cache_pass;
    }
    passed
}

pub fn transition_p95_ms() -> f64 {
    const ROWS: usize = 2_048;
    const SAMPLES: usize = 40;
    let mut chart =
        ChartEngine::new_with_initial_domain(400.0, 400.0, 1.0, HorizontalDomain::Polar)
            .expect("polar chart");
    for (id, dimension, scale) in [
        (
            "angle",
            AxisDimension::Angle,
            GeneralScaleType::AngularCategory,
        ),
        (
            "radius",
            AxisDimension::Radius,
            GeneralScaleType::RadialLinear,
        ),
    ] {
        let mut axis = GeneralAxisOptions::new(id, 0, dimension, scale);
        axis.visible = false;
        chart.add_general_axis(axis).expect("polar axis");
    }
    let input = |offset: usize| GeneralXyInput::Category {
        ids: Some(
            (offset..offset + ROWS)
                .map(|row| GeneralRowId::Number(row as f64))
                .collect(),
        ),
        categories: (offset..offset + ROWS)
            .map(|row| format!("C{row}"))
            .collect(),
        category_indices: (0..ROWS as u32).collect(),
        y: (offset..offset + ROWS)
            .map(|row| 1.0 + (row % 8) as f64)
            .collect(),
        y_valid: None,
    };
    let old = input(0);
    let new = input(512);
    let dataset = chart
        .create_general_xy_dataset(old.clone())
        .expect("polar data");
    chart
        .add_general_series(GeneralSeriesOptions::pie(0, dataset, "angle", "radius"))
        .expect("pie series");
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    chart
        .set_general_transition_duration_ms(200.0)
        .expect("transition duration");
    let mut elapsed = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        let next = if sample % 2 == 0 {
            new.clone()
        } else {
            old.clone()
        };
        let now = 1_000.0 + sample as f64 * 20.0;
        let started = Instant::now();
        chart.set_general_animation_time_ms(now);
        chart
            .replace_general_xy_dataset(dataset, next)
            .expect("polar update");
        chart.set_general_animation_time_ms(now + 10.0);
        black_box(chart.build_frame());
        elapsed.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    elapsed.sort_by(f64::total_cmp);
    elapsed[(SAMPLES * 95 / 100).min(SAMPLES - 1)]
}
