use std::hint::black_box;
use std::time::Instant;

use aeris_charts_engine::{
    AxisDimension, CategoryScaleType, ChartEngine, GeneralAxisDomain, GeneralAxisOptions,
    GeneralRowId, GeneralScaleType, GeneralSeriesOptions, GeneralXyInput, HorizontalDomain,
};

pub fn reorder_p95_ms() -> f64 {
    const ROWS: usize = 4_096;
    const SAMPLES: usize = 40;
    let mut chart = ChartEngine::new_with_initial_domain(
        800.0,
        400.0,
        1.0,
        HorizontalDomain::Category {
            scale: CategoryScaleType::Band,
        },
    )
    .expect("category chart");
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "x",
            0,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .expect("category axis");
    let mut y_axis = GeneralAxisOptions::new("y", 0, AxisDimension::Y, GeneralScaleType::Linear);
    y_axis.visible = false;
    y_axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
    chart.add_general_axis(y_axis).expect("value axis");
    let input = |offset: usize| GeneralXyInput::Category {
        ids: Some(
            (0..ROWS)
                .map(|row| GeneralRowId::Number(row as f64))
                .collect(),
        ),
        categories: (0..ROWS)
            .map(|index| format!("C{}", (index + offset) % ROWS))
            .collect(),
        category_indices: (0..ROWS)
            .map(|row| ((row + ROWS - offset) % ROWS) as u32)
            .collect(),
        y: (0..ROWS).map(|row| 1.0 + (row % 8) as f64).collect(),
        y_valid: None,
    };
    let old = input(0);
    let new = input(ROWS / 4);
    let dataset = chart
        .create_general_xy_dataset(old.clone())
        .expect("category data");
    chart
        .add_general_series(GeneralSeriesOptions::column(0, dataset, "x", "y"))
        .expect("column series");
    chart
        .add_general_series(GeneralSeriesOptions::xy_line(0, dataset, "x", "y"))
        .expect("line series");
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
            .expect("category update");
        chart.set_general_animation_time_ms(now + 10.0);
        black_box(chart.build_frame());
        assert!(chart.general_transition_active());
        elapsed.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    elapsed.sort_by(f64::total_cmp);
    elapsed[(SAMPLES * 95 / 100).min(SAMPLES - 1)]
}
