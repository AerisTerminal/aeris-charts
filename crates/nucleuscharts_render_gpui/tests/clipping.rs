//! Clip (content-mask) behavior for the GPUI executor.
//!
//! Nucleus gives each stacked pane a scissor rect in device px and paints the axis/top layer
//! unscissored. GPUI expresses clipping with `Window::with_content_mask`, which is a *scoped* call,
//! so the plan encodes clips as balanced `PushClip`/`PopClip` pairs and the backend recurses into
//! each masked range.
//!
//! Two things must hold and are easy to break:
//! - the pairs are always balanced, including when a pane's scissor is degenerate (otherwise the
//!   backend's recursion desynchronizes and later panes paint under the wrong mask);
//! - clipping never *removes* geometry from the plan. GPUI intersects each primitive's bounds with
//!   the active mask itself, so out-of-bounds prims must still be emitted; dropping them here would
//!   silently diverge from Canvas2D, which also emits them and lets the canvas clip.

use nucleuscharts_engine::{ChartFrame, FramePane};
use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::{IRect, Prim};
use nucleuscharts_render_gpui::{
    DeviceRect, GpuiChartRenderer, NucleusViewport, PreparedNucleusFrame, SceneOp, ScenePlan,
};

const C: Color = Color::rgb(0x40, 0x50, 0x60);

fn pane(scissor: [u32; 4], main: Vec<Prim>) -> FramePane {
    FramePane {
        top: 0.0,
        height: 100.0,
        scissor,
        under: Vec::new(),
        main,
        top_prims: Vec::new(),
        series_paint_marks: Vec::new(),
        points: Vec::new(),
    }
}

fn rect_at(x: i32, y: i32) -> Prim {
    Prim::Rect {
        rect: IRect { x, y, w: 4, h: 4 },
        color: C,
    }
}

fn frame(panes: Vec<FramePane>) -> ChartFrame {
    ChartFrame {
        width: 400.0,
        height: 300.0,
        pixel_ratio: 1.0,
        panes,
    }
}

fn plan_frame(frame: &ChartFrame, axis: &[Prim]) -> ScenePlan {
    let prepared = PreparedNucleusFrame::new(frame).with_axis(axis, &[]);
    let mut renderer = GpuiChartRenderer::new();
    renderer
        .plan_frame(&prepared, frame.pixel_ratio as f32)
        .expect("frame plans");
    renderer.plan().clone()
}

/// Depth of the clip stack at each op index, or `None` if the stack ever goes negative.
fn clip_depths(plan: &ScenePlan) -> Option<Vec<i32>> {
    let mut depth = 0i32;
    let mut out = Vec::with_capacity(plan.ops.len());
    for op in &plan.ops {
        match op {
            SceneOp::PushClip(_) => depth += 1,
            SceneOp::PopClip => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
        out.push(depth);
    }
    (depth == 0).then_some(out)
}

#[test]
fn each_pane_gets_a_clip_matching_its_scissor_in_device_px() {
    let f = frame(vec![
        pane([0, 0, 400, 180], vec![rect_at(1, 1)]),
        pane([12, 180, 388, 120], vec![rect_at(2, 200)]),
    ]);
    let plan = plan_frame(&f, &[]);

    let clips: Vec<DeviceRect> = plan
        .ops
        .iter()
        .filter_map(|op| match op {
            SceneOp::PushClip(r) => Some(*r),
            _ => None,
        })
        .collect();
    assert_eq!(
        clips,
        vec![
            DeviceRect::new(0.0, 0.0, 400.0, 180.0),
            DeviceRect::new(12.0, 180.0, 388.0, 120.0),
        ],
        "clip rects must be the pane scissors verbatim, in device px"
    );
}

#[test]
fn clip_pairs_are_balanced_and_never_nest_across_panes() {
    let f = frame(vec![
        pane([0, 0, 400, 100], vec![rect_at(0, 0)]),
        pane([0, 100, 400, 100], vec![rect_at(0, 100)]),
        pane([0, 200, 400, 100], vec![rect_at(0, 200)]),
    ]);
    let plan = plan_frame(&f, &[]);
    let depths = clip_depths(&plan).expect("the clip stack must balance");
    assert_eq!(
        depths.iter().copied().max(),
        Some(1),
        "pane clips are siblings, never nested: {depths:?}"
    );
}

#[test]
fn a_degenerate_scissor_pushes_no_clip_but_still_paints_its_prims() {
    // A zero-area scissor is a real state during layout transitions. Pushing an empty content mask
    // would make GPUI drop the whole pane; skipping the push (and its pop) keeps the stack balanced
    // and leaves the prims to be masked by whatever outer mask applies.
    let f = frame(vec![
        pane([0, 0, 0, 0], vec![rect_at(7, 7)]),
        pane([0, 0, 400, 100], vec![rect_at(8, 8)]),
    ]);
    let plan = plan_frame(&f, &[]);
    assert!(clip_depths(&plan).is_some(), "the clip stack must balance");

    let pushes = plan
        .ops
        .iter()
        .filter(|op| matches!(op, SceneOp::PushClip(_)))
        .count();
    assert_eq!(pushes, 1, "only the non-degenerate pane pushes a clip");

    let xs: Vec<f32> = plan
        .ops
        .iter()
        .filter_map(|op| match op {
            SceneOp::Quad { rect, .. } => Some(rect.x),
            _ => None,
        })
        .collect();
    assert_eq!(
        xs,
        vec![7.0, 8.0],
        "both panes' prims must still be emitted"
    );
}

#[test]
fn prims_outside_their_pane_clip_are_still_emitted() {
    // GPUI applies the content mask itself; the executor must not pre-cull, or it would diverge
    // from Canvas2D (which also emits them and lets the canvas clip).
    let f = frame(vec![pane(
        [0, 0, 50, 50],
        vec![rect_at(10, 10), rect_at(500, 500), rect_at(-40, -40)],
    )]);
    let plan = plan_frame(&f, &[]);
    let quads = plan
        .ops
        .iter()
        .filter(|op| matches!(op, SceneOp::Quad { .. }))
        .count();
    assert_eq!(
        quads, 3,
        "all three prims must reach GPUI regardless of the clip"
    );
}

#[test]
fn the_axis_layer_is_outside_every_clip() {
    let f = frame(vec![pane([0, 0, 400, 200], vec![rect_at(1, 1)])]);
    let axis = vec![rect_at(99, 99)];
    let plan = plan_frame(&f, &axis);
    let depths = clip_depths(&plan).expect("balanced");

    let axis_index = plan
        .ops
        .iter()
        .position(|op| matches!(op, SceneOp::Quad { rect, .. } if rect.x == 99.0))
        .expect("the axis quad is present");
    assert_eq!(
        depths[axis_index], 0,
        "the axis layer must paint at clip depth 0"
    );
}

#[test]
fn a_pane_with_no_prims_still_balances_its_clip() {
    let f = frame(vec![
        pane([0, 0, 400, 100], Vec::new()),
        pane([0, 100, 400, 100], vec![rect_at(1, 100)]),
    ]);
    let plan = plan_frame(&f, &[]);
    assert!(clip_depths(&plan).is_some());
    let pushes = plan
        .ops
        .iter()
        .filter(|op| matches!(op, SceneOp::PushClip(_)))
        .count();
    let pops = plan
        .ops
        .iter()
        .filter(|op| matches!(op, SceneOp::PopClip))
        .count();
    assert_eq!((pushes, pops), (2, 2));
}

#[test]
fn clip_rects_convert_to_logical_px_for_every_dpr_in_the_matrix() {
    // The scissor is device px; GPUI's content mask is logical px and gets re-scaled by the window.
    // The round trip must land back on the device rect well inside a pixel.
    for dpr in [1.0f32, 1.25, 1.5, 2.0, 2.5] {
        let device = [0u32, 0, (400.0 * dpr) as u32, (180.0 * dpr) as u32];
        let f = ChartFrame {
            width: 400.0,
            height: 300.0,
            pixel_ratio: dpr as f64,
            panes: vec![pane(device, vec![rect_at(1, 1)])],
        };
        let plan = plan_frame(&f, &[]);
        let SceneOp::PushClip(clip) = plan.ops[0] else {
            panic!("DPR {dpr}: expected a clip first, got {:?}", plan.ops[0]);
        };
        let viewport = NucleusViewport::new(0.0, 0.0, 400.0, 300.0);

        for (device_v, logical) in [
            (
                clip.x,
                nucleuscharts_render_gpui::to_logical_x(clip.x, viewport, dpr),
            ),
            (
                clip.y,
                nucleuscharts_render_gpui::to_logical_y(clip.y, viewport, dpr),
            ),
        ] {
            let back = logical * dpr;
            assert!(
                (back - device_v).abs() < 0.01,
                "DPR {dpr}: {device_v} -> {logical} -> {back}"
            );
        }
        let w_back = nucleuscharts_render_gpui::to_logical_len(clip.w, dpr) * dpr;
        assert!(
            (w_back - clip.w).abs() < 0.01,
            "DPR {dpr}: clip width round trip {} vs {}",
            w_back,
            clip.w
        );
    }
}

#[test]
fn a_clip_offset_by_the_viewport_offset_lands_where_the_element_sits() {
    // AxiusFlow will place the chart element at an offset inside its window; the clip has to move
    // with it, not stay at the window offset.
    let viewport = NucleusViewport::new(64.0, 40.0, 400.0, 300.0);
    let clip = DeviceRect::new(0.0, 0.0, 800.0, 360.0);
    assert_eq!(
        nucleuscharts_render_gpui::to_logical_x(clip.x, viewport, 2.0),
        64.0,
        "the clip's left edge follows the element offset"
    );
    assert_eq!(
        nucleuscharts_render_gpui::to_logical_y(clip.y, viewport, 2.0),
        40.0
    );
    assert_eq!(
        nucleuscharts_render_gpui::to_logical_len(clip.w, 2.0),
        400.0
    );
}

#[test]
fn intersecting_a_clip_with_content_reports_the_visible_region() {
    // `DeviceRect::intersect` is what a host would use to decide whether a prim is visible at all;
    // it must agree with GPUI's own mask intersection semantics (empty = nothing painted).
    let clip = DeviceRect::new(0.0, 0.0, 100.0, 100.0);
    assert_eq!(
        clip.intersect(&DeviceRect::new(50.0, 50.0, 100.0, 100.0)),
        Some(DeviceRect::new(50.0, 50.0, 50.0, 50.0))
    );
    assert_eq!(
        clip.intersect(&DeviceRect::new(200.0, 0.0, 10.0, 10.0)),
        None
    );
    assert_eq!(
        clip.intersect(&DeviceRect::new(-50.0, -50.0, 60.0, 60.0)),
        Some(DeviceRect::new(0.0, 0.0, 10.0, 10.0))
    );
}
