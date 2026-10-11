//! Work bounds for a zoomed-out chart, where every crosshair move repaints thousands of bars and
//! several indicator lines.
//!
//! GPUI converts, uploads and rasterizes every mesh vertex and inserts every submitted primitive
//! into its bounds tree on each frame, so these counts are what keep pointer motion inside a display
//! frame. They are deterministic, so they run in the normal `cargo test` pass without a window; the
//! timed evidence lives in `examples/plan_bench.rs` and `examples/gpui_dense_hover.rs`.

use aeris_charts_engine::{ChartEngine, ChartFrame};
use aeris_charts_render_gpui::fixtures::{DENSE_EMA_PERIODS, dense_indicator_chart};
use aeris_charts_render_gpui::quad_batch::{QUAD_BATCH_THRESHOLD, QuadBatcher, plain_quad_run};
use aeris_charts_render_gpui::{GpuiChartRenderer, GpuiFrameMetrics, PreparedAerisFrame, SceneOp};

const CSS_W: f64 = 1600.0;
const CSS_H: f64 = 900.0;
const DPR: f64 = 1.5;
const BARS: usize = 4_000;

fn zoomed_out() -> (ChartEngine, ChartFrame) {
    let mut engine = dense_indicator_chart(BARS, CSS_W, CSS_H, DPR);
    engine.css_width = CSS_W;
    engine.css_height = CSS_H;
    engine.dpr = DPR;
    let content_h = (CSS_H - engine.time_axis_height()).max(1.0);
    engine.layout_panes(content_h);
    engine.time_scale.set_width(CSS_W);
    engine.fit_content();
    engine.crosshair = Some((CSS_W / 2.0, CSS_H / 2.0));
    let frame = engine.build_frame();
    (engine, frame)
}

fn plan(frame: &ChartFrame) -> (GpuiChartRenderer, GpuiFrameMetrics) {
    let mut renderer = GpuiChartRenderer::new();
    let metrics = renderer
        .plan_frame(&PreparedAerisFrame::new(frame), DPR as f32)
        .expect("the dense frame plans");
    (renderer, metrics)
}

#[test]
fn dense_indicator_lines_lower_to_a_bounded_mesh() {
    let (_, frame) = zoomed_out();
    let source_points: usize = frame
        .panes
        .iter()
        .flat_map(|pane| &pane.main)
        .filter_map(|prim| match prim {
            aeris_charts_render::draw_list::Prim::Polyline { point_count, .. } => {
                Some(*point_count as usize)
            }
            _ => None,
        })
        .sum();
    assert!(
        source_points > DENSE_EMA_PERIODS.len() * 2_000,
        "the fixture is dense: {source_points} line points"
    );
    let (_, metrics) = plan(&frame);
    // Unsimplified, these lines tessellate to roughly 33 vertices per source point.
    assert!(
        (metrics.mesh_vertices as usize) < source_points * 8,
        "{} mesh vertices for {source_points} line points",
        metrics.mesh_vertices
    );
}

#[test]
fn a_zoomed_out_candle_pane_submits_a_handful_of_paths() {
    let (_, frame) = zoomed_out();
    let (renderer, metrics) = plan(&frame);
    let ops = &renderer.plan().ops;
    let mut batcher = QuadBatcher::default();
    let (mut submissions, mut quads) = (0usize, 0usize);
    let mut index = 0;
    while index < ops.len() {
        let run = plain_quad_run(ops, index, ops.len());
        if run < QUAD_BATCH_THRESHOLD {
            if matches!(ops[index], SceneOp::Quad { .. }) {
                submissions += 1;
                quads += 1;
            }
            index += 1;
            continue;
        }
        batcher.group(&ops[index..index + run]);
        for group in batcher.groups() {
            submissions += if group.count >= QUAD_BATCH_THRESHOLD {
                1
            } else {
                group.count
            };
        }
        quads += run;
        index += run;
    }
    assert_eq!(quads as u32, metrics.quads);
    assert!(quads > 4_000, "the fixture is dense: {quads} quads");
    // Alternating up/down colors split adjacent-run batching into thousands of paths, and every
    // GPUI path batch clears and resolves a full-window MSAA target on DirectX.
    assert!(
        submissions < 64,
        "{quads} quads submit as {submissions} GPUI primitives"
    );
}
