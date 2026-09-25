use std::hint::black_box;
use std::time::Instant;

use aeris_charts_engine::{ChartEngine, ChartFrame, DrawingKind, DrawingPoint};
use serde_json::json;

const RUNS: usize = 30;

fn chart() -> ChartEngine {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times = (0..10).map(f64::from).collect::<Vec<_>>();
    let values = (0..10)
        .map(|index| 100.0 + f64::from(index))
        .collect::<Vec<_>>();
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .expect("fixture");
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart
}

fn document() -> String {
    let mut source = chart();
    for index in 0..1_000 {
        let logical = if index < 10 {
            2.0 + index as f64 * 0.4
        } else {
            1_000.0 + index as f64 * 10.0
        };
        source
            .add_drawing(
                DrawingKind::TrendLine,
                0,
                vec![
                    DrawingPoint {
                        logical,
                        price: 102.0,
                    },
                    DrawingPoint {
                        logical: logical + 0.25,
                        price: 103.0,
                    },
                ],
                None,
            )
            .expect("drawing fixture");
    }
    source.export_state_json().expect("export fixture")
}

fn main() {
    let document = document();
    let mut parse_us = Vec::with_capacity(RUNS);
    let mut validation_us = Vec::with_capacity(RUNS);
    let mut semantic_install_us = Vec::with_capacity(RUNS);
    let mut index_rebuild_us = Vec::with_capacity(RUNS);
    let mut first_frame_us = Vec::with_capacity(RUNS);
    let mut evidence = None;

    for _ in 0..RUNS {
        let mut restored = chart();
        let profile = restored
            .import_state_json_profiled(black_box(&document))
            .expect("restore fixture");
        assert_eq!(profile.restore.drawings, 1_000);
        parse_us.push(profile.parse_ns as f64 / 1_000.0);
        validation_us.push(profile.validation_ns as f64 / 1_000.0);
        semantic_install_us.push(profile.semantic_install_ns as f64 / 1_000.0);
        index_rebuild_us.push(profile.index_rebuild_ns as f64 / 1_000.0);

        restored.reset_drawing_work_stats();
        let mut frame = ChartFrame::default();
        let started = Instant::now();
        restored.build_frame_into(&mut frame);
        first_frame_us.push(started.elapsed().as_secs_f64() * 1_000_000.0);
        let work = restored.drawing_work_stats();
        assert_eq!(work.drawings_total, 1_000);
        assert_eq!(work.candidates, 10);
        assert_eq!(work.visible, 10);
        assert_eq!(work.geometry_rebuilds, 10);
        evidence = Some(json!({
            "drawings_total": work.drawings_total,
            "frame_candidates": work.candidates,
            "visible_drawings": work.visible,
            "geometry_rebuilds": work.geometry_rebuilds,
        }));
    }

    println!(
        "{}",
        json!({
            "runs": RUNS,
            "document_bytes": document.len(),
            "parse_us": parse_us,
            "validation_us": validation_us,
            "semantic_install_us": semantic_install_us,
            "index_rebuild_us": index_rebuild_us,
            "first_frame_us": first_frame_us,
            "candidate_evidence": evidence.expect("evidence"),
        })
    );
}
