//! Cross-backend parity: the GPUI executor against the Canvas2D executor, on the same frame.
//!
//! # What this proves, and what it does not
//!
//! It does **not** compare rasterized pixels because GPUI exposes no GPU readback. It compares the
//! *draw stream*: for every
//! primitive, the exact geometry and color each backend is asked to fill.
//!
//! For the crisp-rect subset (`Rect`, `RectFrame`, `HLine`, `VLine`, `Background`) that is a
//! stronger statement than an image diff at one DPR, because both backends resolve to
//! "fill this axis-aligned rectangle with this color". If the rectangle lists and the color lists
//! are identical, the rasterized output is identical for any solid axis-aligned filler — there is
//! no antialiasing, no shaping, and no tessellation freedom left to differ over. That subset is the
//! large majority of a candlestick chart's primitives.
//!
//! For the tessellated subset the comparison is structural (same path, same closing edges, same
//! gradient extents), because Canvas2D describes curves analytically while GPUI and WebGPU both
//! consume triangles. Those are exactly the prims the existing WebGPU-vs-Canvas2D reference already
//! records a bounded residual for.

use nucleuscharts_engine::{
    AxisDimension, CategoryScaleType, ChartEngine, ContinuousScaleType, GeneralAxisOptions,
    GeneralScaleType, GeneralSeriesOptions, GeneralXyInput, HorizontalDomain, OrderId, OrderKind,
    OrderRole, OrderSide, OrderStatus, PositionId, PositionSide, SeriesKind, TradingPosition,
    TradingPriceScale, WorkingOrder,
};
use nucleuscharts_render::canvas2d::{execute as canvas_execute, Canvas2d, Viewport};
use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::{Gradient, IRect, LineStyle, LineType, Prim, TextAlign};
use nucleuscharts_render_gpui::{
    fixtures, ExecutorOptions, GpuiChartRenderer, GpuiFrameMetrics, Paint, PreparedNucleusFrame,
    SceneOp, ScenePlan,
};

/// A Canvas2D target that records only what the crisp-rect subset does: the current fill style and
/// every `fill_rect`. Path calls are counted so a test can assert a prim went down the path route.
#[derive(Default)]
struct RectRecorder {
    fill: Option<Paint>,
    /// `(x, y, w, h, fill)` in call order.
    rects: Vec<(f32, f32, f32, f32, Paint)>,
    path_fills: usize,
    path_strokes: usize,
    text_runs: Vec<(String, f32, f32, String)>,
}

impl Canvas2d for RectRecorder {
    fn set_fill_solid(&mut self, color: Color) {
        self.fill = Some(Paint::Solid(color));
    }
    fn set_fill_vgradient(&mut self, _y_top: f32, _y_bottom: f32, top: Color, bottom: Color) {
        self.fill = Some(Paint::VGradient { top, bottom });
    }
    fn set_stroke(&mut self, _color: Color) {}
    fn set_line_width(&mut self, _width: f32) {}
    fn set_line_dash(&mut self, _pattern: &[f32]) {}
    fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let fill = self
            .fill
            .expect("a fill style is set before every fill_rect");
        self.rects.push((x, y, w, h, fill));
    }
    fn begin_path(&mut self) {}
    fn move_to(&mut self, _x: f32, _y: f32) {}
    fn line_to(&mut self, _x: f32, _y: f32) {}
    fn close_path(&mut self) {}
    fn arc(&mut self, _cx: f32, _cy: f32, _r: f32, _s: f32, _e: f32) {}
    fn stroke(&mut self) {
        self.path_strokes += 1;
    }
    fn fill(&mut self) {
        self.path_fills += 1;
    }
    fn fill_text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        font: &str,
        _color: Color,
        _align: TextAlign,
    ) {
        self.text_runs.push((text.into(), x, y, font.into()));
    }
    fn fill_rotated_text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        font: &str,
        _color: Color,
        _align: TextAlign,
        angle: f32,
    ) {
        self.text_runs
            .push((format!("{text}@{angle}"), x, y, font.into()));
    }
}

fn canvas_rects(prims: &[Prim], points: &[[f32; 2]]) -> RectRecorder {
    let mut r = RectRecorder::default();
    canvas_execute(
        prims,
        points,
        &mut r,
        Viewport {
            width: 2000.0,
            height: 2000.0,
        },
    );
    r
}

fn gpui_plan(prims: &[Prim], points: &[[f32; 2]]) -> (ScenePlan, GpuiFrameMetrics) {
    let mut plan = ScenePlan::default();
    let mut metrics = GpuiFrameMetrics::default();
    nucleuscharts_render_gpui::executor::execute_layer(
        prims,
        points,
        ExecutorOptions::default(),
        &mut nucleuscharts_render_gpui::geometry::Scratch::default(),
        &mut plan,
        &mut metrics,
    );
    (plan, metrics)
}

/// The GPUI plan's quads as `(x, y, w, h, fill)`, in emission order.
fn gpui_quads(plan: &ScenePlan) -> Vec<(f32, f32, f32, f32, Paint)> {
    plan.ops
        .iter()
        .filter_map(|op| match op {
            SceneOp::Quad { rect, fill, .. } => Some((rect.x, rect.y, rect.w, rect.h, *fill)),
            _ => None,
        })
        .collect()
}

/// Every rect-family prim, with the awkward cases (even/odd widths, dashes, degenerate extents,
/// negative coordinates) included deliberately.
fn rect_family_cases() -> Vec<(&'static str, Prim)> {
    let c = Color::rgba(0x26, 0xa6, 0x9a, 0xc0);
    vec![
        (
            "Rect",
            Prim::Rect {
                rect: IRect {
                    x: 3,
                    y: 4,
                    w: 10,
                    h: 6,
                },
                color: c,
            },
        ),
        (
            "Rect at a negative offset",
            Prim::Rect {
                rect: IRect {
                    x: -12,
                    y: -5,
                    w: 30,
                    h: 9,
                },
                color: c,
            },
        ),
        (
            "Rect with a zero width",
            Prim::Rect {
                rect: IRect {
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 6,
                },
                color: c,
            },
        ),
        (
            "Rect with a negative height",
            Prim::Rect {
                rect: IRect {
                    x: 0,
                    y: 0,
                    w: 6,
                    h: -3,
                },
                color: c,
            },
        ),
        (
            "RectFrame border 1",
            Prim::RectFrame {
                rect: IRect {
                    x: 10,
                    y: 20,
                    w: 8,
                    h: 6,
                },
                border: 1,
                color: c,
            },
        ),
        (
            "RectFrame border 3",
            Prim::RectFrame {
                rect: IRect {
                    x: 10,
                    y: 20,
                    w: 41,
                    h: 27,
                },
                border: 3,
                color: c,
            },
        ),
        (
            "RectFrame thicker than half its width",
            Prim::RectFrame {
                rect: IRect {
                    x: 0,
                    y: 0,
                    w: 4,
                    h: 4,
                },
                border: 3,
                color: c,
            },
        ),
        (
            "HLine width 1",
            Prim::HLine {
                y: 50,
                x0: 10,
                x1: 40,
                width: 1,
                style: LineStyle::Solid,
                color: c,
            },
        ),
        (
            "HLine width 2 (even)",
            Prim::HLine {
                y: 50,
                x0: 10,
                x1: 40,
                width: 2,
                style: LineStyle::Solid,
                color: c,
            },
        ),
        (
            "HLine width 3 (odd)",
            Prim::HLine {
                y: 51,
                x0: -7,
                x1: 40,
                width: 3,
                style: LineStyle::Solid,
                color: c,
            },
        ),
        (
            "HLine dotted",
            Prim::HLine {
                y: 7,
                x0: 0,
                x1: 97,
                width: 1,
                style: LineStyle::Dotted,
                color: c,
            },
        ),
        (
            "HLine dashed",
            Prim::HLine {
                y: 7,
                x0: 0,
                x1: 97,
                width: 2,
                style: LineStyle::Dashed,
                color: c,
            },
        ),
        (
            "VLine width 1",
            Prim::VLine {
                x: 5,
                y0: 0,
                y1: 24,
                width: 1,
                style: LineStyle::Solid,
                color: c,
            },
        ),
        (
            "VLine width 4 (even)",
            Prim::VLine {
                x: 5,
                y0: 0,
                y1: 24,
                width: 4,
                style: LineStyle::Solid,
                color: c,
            },
        ),
        (
            "VLine dashed",
            Prim::VLine {
                x: 5,
                y0: 0,
                y1: 97,
                width: 1,
                style: LineStyle::Dashed,
                color: c,
            },
        ),
        (
            "VLine dotted odd width",
            Prim::VLine {
                x: 15,
                y0: 3,
                y1: 61,
                width: 3,
                style: LineStyle::Dotted,
                color: c,
            },
        ),
        (
            "Background gradient",
            Prim::Background {
                rect: [20.0, 10.0, 160.0, 60.0],
                gradient: Gradient {
                    top: Color::rgb(1, 2, 3),
                    bottom: Color::rgb(4, 5, 6),
                },
            },
        ),
    ]
}

#[test]
fn crisp_rect_subset_is_draw_call_identical_to_canvas2d() {
    for (name, prim) in rect_family_cases() {
        let prims = [prim];
        let canvas = canvas_rects(&prims, &[]);
        let (plan, _) = gpui_plan(&prims, &[]);
        assert_eq!(
            gpui_quads(&plan),
            canvas.rects,
            "{name}: the GPUI quad stream must match Canvas2D's fill_rect stream exactly"
        );
        assert_eq!(
            canvas.path_fills + canvas.path_strokes,
            0,
            "{name} should not have taken a path route on Canvas2D"
        );
    }
}

#[test]
fn crisp_rect_subset_matches_across_the_whole_dpr_matrix() {
    // The engine bakes the DPR into the prim coordinates, so parity has to hold at every DPR the
    // validation matrix names — including the fractional ones.
    for dpr in [1.0f64, 1.25, 1.5, 2.0, 2.5] {
        let scaled: Vec<Prim> = rect_family_cases()
            .into_iter()
            .map(|(_, prim)| scale_prim(prim, dpr))
            .collect();
        let canvas = canvas_rects(&scaled, &[]);
        let (plan, _) = gpui_plan(&scaled, &[]);
        assert_eq!(
            gpui_quads(&plan),
            canvas.rects,
            "DPR {dpr}: the crisp-rect draw streams diverged"
        );
    }
}

/// Apply a DPR the way the engine would, i.e. by scaling the already-integer device coordinates.
fn scale_prim(prim: Prim, dpr: f64) -> Prim {
    let s = |v: i32| (v as f64 * dpr).round() as i32;
    let sf = |v: f32| (v as f64 * dpr) as f32;
    match prim {
        Prim::Rect { rect, color } => Prim::Rect {
            rect: IRect {
                x: s(rect.x),
                y: s(rect.y),
                w: s(rect.w),
                h: s(rect.h),
            },
            color,
        },
        Prim::RectFrame {
            rect,
            border,
            color,
        } => Prim::RectFrame {
            rect: IRect {
                x: s(rect.x),
                y: s(rect.y),
                w: s(rect.w),
                h: s(rect.h),
            },
            border: s(border).max(1),
            color,
        },
        Prim::HLine {
            y,
            x0,
            x1,
            width,
            style,
            color,
        } => Prim::HLine {
            y: s(y),
            x0: s(x0),
            x1: s(x1),
            width: s(width).max(1),
            style,
            color,
        },
        Prim::VLine {
            x,
            y0,
            y1,
            width,
            style,
            color,
        } => Prim::VLine {
            x: s(x),
            y0: s(y0),
            y1: s(y1),
            width: s(width).max(1),
            style,
            color,
        },
        Prim::Background { rect, gradient } => Prim::Background {
            rect: [sf(rect[0]), sf(rect[1]), sf(rect[2]), sf(rect[3])],
            gradient,
        },
        other => other,
    }
}

#[test]
fn tessellated_prims_take_the_path_route_on_both_backends() {
    let points = vec![
        [0.0f32, 10.0],
        [10.0, 4.0],
        [20.0, 12.0],
        [0.0, 30.0],
        [10.0, 28.0],
        [20.0, 33.0],
    ];
    let c = Color::rgba(0x21, 0x96, 0xf3, 0xa0);
    let cases: Vec<(&str, Prim)> = vec![
        (
            "Polyline",
            Prim::Polyline {
                first_point: 0,
                point_count: 3,
                width: 2.0,
                style: LineStyle::Solid,
                line_type: LineType::Simple,
                color: c,
            },
        ),
        (
            "AreaFill",
            Prim::AreaFill {
                first_point: 0,
                point_count: 3,
                base_y: 40.0,
                line_type: LineType::Simple,
                gradient: Gradient {
                    top: c,
                    bottom: Color::rgba(0x21, 0x96, 0xf3, 0),
                },
            },
        ),
        (
            "BandFill",
            Prim::BandFill {
                upper_first: 0,
                lower_first: 3,
                point_count: 3,
                fill: c,
            },
        ),
        (
            "Circle",
            Prim::Circle {
                cx: 10.0,
                cy: 10.0,
                radius: 4.0,
                fill: c,
                stroke_width: 0.0,
                stroke: c,
            },
        ),
        (
            "Triangle",
            Prim::Triangle {
                a: [0.0, 0.0],
                b: [8.0, 0.0],
                c: [8.0, 8.0],
                color: c,
            },
        ),
        (
            "RoundRect",
            Prim::RoundRect {
                x: 1.0,
                y: 1.0,
                w: 20.0,
                h: 10.0,
                radii: [2.0; 4],
                fill: c,
                border_width: 0.0,
                border_color: c,
            },
        ),
    ];
    for (name, prim) in cases {
        let prims = [prim];
        let canvas = canvas_rects(&prims, &points);
        let (plan, metrics) = gpui_plan(&prims, &points);
        assert!(
            canvas.path_fills + canvas.path_strokes > 0,
            "{name}: Canvas2D should have taken a path route"
        );
        assert!(
            metrics.paths > 0,
            "{name}: GPUI should have emitted at least one triangle mesh"
        );
        assert!(
            canvas.rects.is_empty() && metrics.quads == 0,
            "{name}: neither backend should emit an axis-aligned quad"
        );
        // Every mesh must be a whole number of triangles, or GPUI's `push_triangle` loop would
        // silently drop vertices.
        for op in &plan.ops {
            if let SceneOp::Mesh { vertex_count, .. } = op {
                assert_eq!(vertex_count % 3, 0, "{name}: partial triangle in the mesh");
            }
        }
    }
}

#[test]
fn curved_brush_fixture_lowers_sparse_dense_and_scaled_widths_without_drops() {
    for dpr in [1.0f32, 1.25, 1.5, 2.0, 2.5] {
        let fixture = fixtures::curved_brushes(dpr);
        let stroke_count = fixture
            .prims
            .iter()
            .filter(|prim| {
                matches!(
                    prim,
                    Prim::Polyline {
                        line_type: LineType::Curved,
                        ..
                    }
                )
            })
            .count();
        let (plan, metrics) = gpui_plan(&fixture.prims, &fixture.points);
        let meshes: Vec<_> = plan
            .ops
            .iter()
            .filter_map(|op| match op {
                SceneOp::Mesh { vertex_count, .. } => Some(*vertex_count),
                _ => None,
            })
            .collect();

        assert_eq!(stroke_count, 4, "DPR {dpr}: fixture lost curved strokes");
        assert_eq!(metrics.dropped_prims, 0, "DPR {dpr}: a stroke was dropped");
        assert_eq!(meshes.len(), stroke_count, "DPR {dpr}: wrong mesh count");
        assert!(
            meshes.iter().all(|count| *count >= 3 && *count % 3 == 0),
            "DPR {dpr}: every curved brush must produce complete triangles"
        );
    }
}

#[test]
fn area_fill_gradient_extent_matches_the_canvas2d_ramp() {
    // Canvas2D spans the ramp over [min point y, base_y]; GPUI's ramp is bounds-relative, so the
    // mesh bounds must equal that interval or the two shade differently.
    let points = vec![[0.0f32, 10.0], [10.0, 4.0], [20.0, 12.0]];
    let prims = [Prim::AreaFill {
        first_point: 0,
        point_count: 3,
        base_y: 40.0,
        line_type: LineType::Simple,
        gradient: Gradient {
            top: Color::rgb(0, 0, 0xff),
            bottom: Color::rgba(0, 0, 0xff, 0),
        },
    }];
    let (plan, _) = gpui_plan(&prims, &points);
    let SceneOp::Mesh {
        first_vertex,
        vertex_count,
        fill,
    } = plan.ops[0]
    else {
        panic!("expected one mesh, got {:?}", plan.ops);
    };
    let bounds = plan
        .mesh_bounds(first_vertex, vertex_count)
        .expect("the mesh has vertices");
    assert_eq!(
        (bounds.y, bounds.y + bounds.h),
        (4.0, 40.0),
        "the mesh must span exactly the Canvas2D gradient extent"
    );
    assert_eq!(
        fill,
        Paint::VGradient {
            top: Color::rgb(0, 0, 0xff),
            bottom: Color::rgba(0, 0, 0xff, 0),
        },
        "a full-height fill keeps the prim's own stops"
    );
}

#[test]
fn text_runs_reach_both_backends_with_the_same_font_and_anchor() {
    let prims = [Prim::Text {
        x: 100.5,
        y: 30.0,
        text: "42.50".into(),
        color: Color::rgb(0x13, 0x17, 0x22),
        size: 12.0,
        family: "sans-serif".into(),
        align: TextAlign::Right,
        weight: 400,
        italic: false,
    }];
    let canvas = canvas_rects(&prims, &[]);
    let (plan, metrics) = gpui_plan(&prims, &[]);
    assert_eq!(canvas.text_runs.len(), 1);
    assert_eq!(metrics.text_runs, 1);

    let (text, x, y, font) = &canvas.text_runs[0];
    let SceneOp::Text(run) = &plan.ops[0] else {
        panic!("expected a text op");
    };
    assert_eq!(run.text, *text);
    assert_eq!(run.x, *x);
    assert_eq!(run.y, *y);
    // The GPUI adapter derives the same CSS shorthand Canvas2D is given.
    assert_eq!(
        nucleuscharts_render::draw_list::text_font_spec(
            run.size,
            &run.family,
            run.weight,
            run.italic
        ),
        *font
    );
}

#[test]
fn rotated_text_reaches_canvas_and_gpui_with_the_same_transform() {
    let prims = [Prim::RotatedText {
        x: 100.5,
        y: 30.0,
        text: "trend".into(),
        color: Color::rgb(0x13, 0x17, 0x22),
        size: 12.0,
        family: "sans-serif".into(),
        align: TextAlign::Center,
        weight: 500,
        italic: true,
        angle: -0.625,
    }];
    let canvas = canvas_rects(&prims, &[]);
    let (plan, metrics) = gpui_plan(&prims, &[]);
    assert_eq!(canvas.text_runs.len(), 1);
    assert_eq!(metrics.text_runs, 1);
    assert_eq!(canvas.text_runs[0].0, "trend@-0.625");
    let SceneOp::Text(run) = &plan.ops[0] else {
        panic!("expected a text op");
    };
    assert_eq!((run.x, run.y, run.angle), (100.5, 30.0, -0.625));
}

/// A real multi-series, multi-pane engine frame — not a synthetic prim list.
fn real_engine_frame(dpr: f64) -> ChartEngine {
    let mut engine = ChartEngine::new(900.0, 520.0, dpr);
    let n = 180usize;
    let times: Vec<f64> = (0..n).map(|i| 1_600_000_000.0 + i as f64 * 60.0).collect();
    let close: Vec<f64> = (0..n)
        .map(|i| 100.0 + (i as f64 * 0.13).sin() * 8.0 + (i as f64 * 0.02).cos() * 5.0)
        .collect();
    let open: Vec<f64> = close
        .iter()
        .enumerate()
        .map(|(i, c)| if i == 0 { *c } else { close[i - 1] })
        .collect();
    let high: Vec<f64> = open
        .iter()
        .zip(&close)
        .map(|(o, c)| o.max(*c) + 2.0)
        .collect();
    let low: Vec<f64> = open
        .iter()
        .zip(&close)
        .map(|(o, c)| o.min(*c) - 2.0)
        .collect();

    engine
        .set_series_data(0, &times, &open, &high, &low, &close)
        .expect("candles load");
    engine.series[0].kind = SeriesKind::Candlestick;

    // A line, an area and a histogram, so the tessellated and gradient routes are all populated.
    for kind in [SeriesKind::Line, SeriesKind::Area, SeriesKind::Histogram] {
        let id = engine.add_series(kind);
        engine
            .set_series_data(id, &times, &close, &close, &close, &close)
            .expect("line-ish series loads");
    }

    engine.css_width = 900.0;
    engine.css_height = 520.0;
    engine.dpr = dpr;
    let content_h = (520.0 - engine.time_axis_height()).max(1.0);
    engine.layout_panes(content_h);
    engine.time_scale.set_width(900.0);
    engine.fit_content();
    engine.crosshair = Some((450.0, 260.0));
    let position_id = PositionId::new("gpui-position").unwrap();
    engine
        .update_trading_position(TradingPosition {
            id: position_id.clone(),
            pane_index: 0,
            price_scale: TradingPriceScale::Right,
            side: PositionSide::Long,
            average_price: 105.0,
            quantity: 2.0,
            display_pnl: Some(14.0),
            currency: Some("USD".into()),
        })
        .unwrap();
    for (id, role, kind, price) in [
        ("gpui-tp", OrderRole::TakeProfit, OrderKind::Limit, 112.0),
        ("gpui-sl", OrderRole::StopLoss, OrderKind::Stop, 96.0),
    ] {
        engine
            .update_working_order(WorkingOrder {
                id: OrderId::new(id).unwrap(),
                pane_index: 0,
                price_scale: TradingPriceScale::Right,
                side: OrderSide::Sell,
                kind,
                role,
                status: OrderStatus::Working,
                price,
                stop_price: None,
                quantity: 2.0,
                filled_quantity: 0.0,
                position_id: Some(position_id.clone()),
                parent_order_id: None,
                bracket_id: None,
                oco_group_id: None,
                revision: 1,
            })
            .unwrap();
    }
    engine
}

#[test]
fn a_real_engine_frame_has_draw_call_identical_quads_on_both_backends() {
    for dpr in [1.0f64, 1.25, 1.5, 2.0, 2.5] {
        let mut engine = real_engine_frame(dpr);
        let frame = engine.build_frame();
        assert!(!frame.panes.is_empty(), "the frame should have panes");

        let mut total = 0usize;
        for pane in &frame.panes {
            for layer in [&pane.under, &pane.main, &pane.top_prims] {
                if layer.is_empty() {
                    continue;
                }
                let canvas = canvas_rects(layer, &pane.points);
                let (plan, _) = gpui_plan(layer, &pane.points);
                assert_eq!(
                    gpui_quads(&plan),
                    canvas.rects,
                    "DPR {dpr}: a real frame's quad stream diverged from Canvas2D"
                );
                total += canvas.rects.len();
            }
        }
        assert!(
            total > 50,
            "DPR {dpr}: expected a substantial quad stream, got {total}"
        );
    }
}

#[test]
fn category_column_engine_frame_has_identical_canvas_and_gpui_quads() {
    for dpr in [1.0f64, 1.5, 2.0] {
        let mut engine = ChartEngine::new(420.0, 260.0, dpr);
        let pane = engine
            .add_pane_with_domain(
                true,
                HorizontalDomain::Category {
                    scale: CategoryScaleType::Band,
                },
            )
            .unwrap();
        engine
            .add_general_axis(GeneralAxisOptions::new(
                "x",
                pane,
                AxisDimension::X,
                GeneralScaleType::Band,
            ))
            .unwrap();
        engine
            .add_general_axis(GeneralAxisOptions::new(
                "y",
                pane,
                AxisDimension::Y,
                GeneralScaleType::Linear,
            ))
            .unwrap();
        let dataset = engine
            .create_general_xy_dataset(GeneralXyInput::Category {
                ids: None,
                categories: vec!["A".into(), "B".into(), "C".into()],
                category_indices: vec![0, 1, 2],
                y: vec![-4.0, 8.0, 99.0],
                y_valid: Some(vec![1, 1, 0]),
            })
            .unwrap();
        let mut options = GeneralSeriesOptions::column(pane, dataset, "x", "y");
        options.color = Some("#4f6b8a".into());
        engine.add_general_series(options).unwrap();
        engine.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

        let frame = engine.build_frame();
        let pane_frame = &frame.panes[pane];
        let canvas = canvas_rects(&pane_frame.main, &pane_frame.points);
        let (plan, metrics) = gpui_plan(&pane_frame.main, &pane_frame.points);
        assert_eq!(canvas.rects.len(), 2, "DPR {dpr}: two columns expected");
        assert_eq!(metrics.dropped_prims, 0, "DPR {dpr}: no column may drop");
        assert_eq!(
            gpui_quads(&plan),
            canvas.rects,
            "DPR {dpr}: category-column quads diverged"
        );
    }
}

#[test]
fn xy_scatter_engine_frame_reaches_canvas_and_gpui_path_routes() {
    for dpr in [1.0f64, 1.5, 2.0] {
        let mut engine = ChartEngine::new(420.0, 260.0, dpr);
        let pane = engine
            .add_pane_with_domain(
                true,
                HorizontalDomain::Continuous {
                    scale: ContinuousScaleType::Linear,
                },
            )
            .unwrap();
        engine
            .add_general_axis(GeneralAxisOptions::new(
                "x",
                pane,
                AxisDimension::X,
                GeneralScaleType::Linear,
            ))
            .unwrap();
        engine
            .add_general_axis(GeneralAxisOptions::new(
                "y",
                pane,
                AxisDimension::Y,
                GeneralScaleType::Linear,
            ))
            .unwrap();
        let dataset = engine
            .create_general_xy_dataset(GeneralXyInput::Numeric {
                ids: None,
                x: vec![1.0, 2.0, 3.0, 4.0],
                y: vec![-2.0, 1.0, 5.0, 99.0],
                y_valid: Some(vec![1, 1, 1, 0]),
            })
            .unwrap();
        let mut options = GeneralSeriesOptions::scatter(pane, dataset, "x", "y");
        options.color = Some("#725c9f".into());
        engine.add_general_series(options).unwrap();
        engine.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

        let frame = engine.build_frame();
        let pane_frame = &frame.panes[pane];
        let circles = pane_frame
            .main
            .iter()
            .filter(|primitive| matches!(primitive, Prim::Circle { .. }))
            .count();
        assert_eq!(circles, 3, "DPR {dpr}: three scatter circles expected");
        let canvas = canvas_rects(&pane_frame.main, &pane_frame.points);
        let (_plan, metrics) = gpui_plan(&pane_frame.main, &pane_frame.points);
        assert_eq!(
            canvas.path_fills, 3,
            "DPR {dpr}: Canvas must fill each point"
        );
        assert_eq!(
            metrics.dropped_prims, 0,
            "DPR {dpr}: no scatter point may drop"
        );
        assert!(
            metrics.paths >= 3,
            "DPR {dpr}: GPUI must lower scatter circles to paths ({metrics:?})"
        );
    }
}

#[test]
fn xy_line_engine_frame_reaches_canvas_and_gpui_stroke_routes() {
    for dpr in [1.0f64, 1.5, 2.0] {
        let mut engine = ChartEngine::new(420.0, 260.0, dpr);
        let pane = engine
            .add_pane_with_domain(
                true,
                HorizontalDomain::Continuous {
                    scale: ContinuousScaleType::Linear,
                },
            )
            .unwrap();
        engine
            .add_general_axis(GeneralAxisOptions::new(
                "x",
                pane,
                AxisDimension::X,
                GeneralScaleType::Linear,
            ))
            .unwrap();
        engine
            .add_general_axis(GeneralAxisOptions::new(
                "y",
                pane,
                AxisDimension::Y,
                GeneralScaleType::Linear,
            ))
            .unwrap();
        let dataset = engine
            .create_general_xy_dataset(GeneralXyInput::Numeric {
                ids: None,
                x: vec![0.0, 1.0, 2.0, 3.0, 4.0],
                y: vec![0.0, 1.0, 99.0, 3.0, 4.0],
                y_valid: Some(vec![1, 1, 0, 1, 1]),
            })
            .unwrap();
        let mut options = GeneralSeriesOptions::xy_line(pane, dataset, "x", "y");
        options.color = Some("#365f91".into());
        engine.add_general_series(options).unwrap();
        engine.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

        let frame = engine.build_frame();
        let pane_frame = &frame.panes[pane];
        let expected = Color::parse_css("#365f91").unwrap();
        assert_eq!(
            pane_frame
                .main
                .iter()
                .filter(|primitive| matches!(primitive, Prim::Polyline { color, point_count: 2, .. } if *color == expected))
                .count(),
            2,
            "DPR {dpr}: missing Y must split XY line into two stroke runs"
        );
        let canvas = canvas_rects(&pane_frame.main, &pane_frame.points);
        let (_plan, metrics) = gpui_plan(&pane_frame.main, &pane_frame.points);
        assert_eq!(
            canvas.path_strokes, 2,
            "DPR {dpr}: Canvas must stroke both line runs"
        );
        assert_eq!(metrics.dropped_prims, 0, "DPR {dpr}: no XY line may drop");
        assert!(
            metrics.paths >= 2,
            "DPR {dpr}: GPUI must lower both XY line runs to paths ({metrics:?})"
        );
    }
}

#[test]
fn xy_area_engine_frame_reaches_canvas_and_gpui_fill_routes() {
    for dpr in [1.0f64, 1.5, 2.0] {
        let mut engine = ChartEngine::new(420.0, 260.0, dpr);
        let pane = engine
            .add_pane_with_domain(
                true,
                HorizontalDomain::Continuous {
                    scale: ContinuousScaleType::Linear,
                },
            )
            .unwrap();
        engine
            .add_general_axis(GeneralAxisOptions::new(
                "x",
                pane,
                AxisDimension::X,
                GeneralScaleType::Linear,
            ))
            .unwrap();
        engine
            .add_general_axis(GeneralAxisOptions::new(
                "y",
                pane,
                AxisDimension::Y,
                GeneralScaleType::Linear,
            ))
            .unwrap();
        let dataset = engine
            .create_general_xy_dataset(GeneralXyInput::Numeric {
                ids: None,
                x: vec![0.0, 1.0, 2.0, 3.0, 4.0],
                y: vec![1.0, 2.0, 99.0, 3.0, 1.0],
                y_valid: Some(vec![1, 1, 0, 1, 1]),
            })
            .unwrap();
        engine
            .add_general_series(GeneralSeriesOptions::xy_area(pane, dataset, "x", "y"))
            .unwrap();
        engine.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

        let frame = engine.build_frame();
        let pane_frame = &frame.panes[pane];
        assert_eq!(
            pane_frame
                .main
                .iter()
                .filter(|primitive| matches!(primitive, Prim::AreaFill { point_count: 2, .. }))
                .count(),
            2,
            "DPR {dpr}: missing Y must split XY area into two fill runs"
        );
        let canvas = canvas_rects(&pane_frame.main, &pane_frame.points);
        let (_plan, metrics) = gpui_plan(&pane_frame.main, &pane_frame.points);
        assert_eq!(
            canvas.path_fills, 2,
            "DPR {dpr}: Canvas must fill both area runs"
        );
        assert!(
            canvas.path_strokes >= 2,
            "DPR {dpr}: Canvas must stroke both area runs"
        );
        assert_eq!(metrics.dropped_prims, 0, "DPR {dpr}: no XY area may drop");
        assert!(
            metrics.paths >= 4,
            "DPR {dpr}: GPUI must lower area fill/stroke paths ({metrics:?})"
        );
    }
}

#[test]
fn a_real_engine_frame_lowers_every_prim_it_contains() {
    let mut engine = real_engine_frame(1.5);
    let frame = engine.build_frame();
    let prepared = PreparedNucleusFrame::new(&frame);
    let mut renderer = GpuiChartRenderer::new();
    let metrics = renderer.plan_frame(&prepared, 1.5).expect("frame plans");

    assert!(metrics.prims > 0, "the frame should carry prims");
    assert_eq!(
        metrics.dropped_prims, 0,
        "no prim in a real frame should lower to nothing ({metrics:?})"
    );
    assert!(
        metrics.quads > 0 && metrics.paths > 0,
        "a mixed candle/line/area/histogram frame must exercise both routes ({metrics:?})"
    );
}

#[test]
fn planning_is_deterministic_for_an_unchanged_frame() {
    let mut engine = real_engine_frame(2.0);
    let frame = engine.build_frame();
    let prepared = PreparedNucleusFrame::new(&frame);

    let mut a = GpuiChartRenderer::new();
    let ma = a.plan_frame(&prepared, 2.0).unwrap();
    let ops_a = a.plan().ops.clone();
    let verts_a = a.plan().vertices.clone();

    let mut b = GpuiChartRenderer::new();
    let mb = b.plan_frame(&prepared, 2.0).unwrap();

    assert_eq!(ops_a, b.plan().ops, "the op stream must be reproducible");
    assert_eq!(
        verts_a,
        b.plan().vertices,
        "the vertex pool must be reproducible"
    );
    assert_eq!(
        (ma.prims, ma.quads, ma.paths),
        (mb.prims, mb.quads, mb.paths)
    );
}

#[test]
fn odd_even_and_fractional_viewport_geometry_stays_draw_call_identical() {
    // Covers odd/even viewport sizes, fractional pane sizes, and small and large chart
    // bounds". Odd sizes at a fractional DPR are where half-pixel rounding diverges if a backend
    // recomputes geometry instead of consuming the engine's.
    let sizes = [
        (321.0f64, 199.0f64), // small and odd
        (322.0, 200.0),       // small and even
        (1001.0, 601.0),      // odd
        (1000.0, 600.0),      // even
        (1279.5, 719.5),      // fractional
        (2560.0, 1440.0),     // large
    ];
    for (w, h) in sizes {
        for dpr in [1.0f64, 1.25, 1.5, 2.0, 2.5] {
            let mut engine = ChartEngine::new(w, h, dpr);
            let n = 120usize;
            let times: Vec<f64> = (0..n).map(|i| 1_600_000_000.0 + i as f64 * 60.0).collect();
            let close: Vec<f64> = (0..n)
                .map(|i| 100.0 + (i as f64 * 0.17).sin() * 9.0)
                .collect();
            let open: Vec<f64> = close
                .iter()
                .enumerate()
                .map(|(i, c)| if i == 0 { *c } else { close[i - 1] })
                .collect();
            let high: Vec<f64> = open
                .iter()
                .zip(&close)
                .map(|(o, c)| o.max(*c) + 1.0)
                .collect();
            let low: Vec<f64> = open
                .iter()
                .zip(&close)
                .map(|(o, c)| o.min(*c) - 1.0)
                .collect();
            engine
                .set_series_data(0, &times, &open, &high, &low, &close)
                .expect("series loads");
            engine.series[0].kind = SeriesKind::Candlestick;
            engine.css_width = w;
            engine.css_height = h;
            engine.dpr = dpr;
            let content_h = (h - engine.time_axis_height()).max(1.0);
            engine.layout_panes(content_h);
            engine.time_scale.set_width(w);
            engine.fit_content();

            let frame = engine.build_frame();
            for pane in &frame.panes {
                for layer in [&pane.under, &pane.main, &pane.top_prims] {
                    if layer.is_empty() {
                        continue;
                    }
                    let canvas = canvas_rects(layer, &pane.points);
                    let (plan, _) = gpui_plan(layer, &pane.points);
                    assert_eq!(
                        gpui_quads(&plan),
                        canvas.rects,
                        "{w}x{h} @ DPR {dpr}: quad streams diverged"
                    );
                }
            }

            // The frame must also plan cleanly at this geometry, with nothing silently dropped.
            let prepared = PreparedNucleusFrame::new(&frame);
            let mut renderer = GpuiChartRenderer::new();
            let m = renderer
                .plan_frame(&prepared, dpr as f32)
                .unwrap_or_else(|e| panic!("{w}x{h} @ DPR {dpr}: {e}"));
            assert_eq!(m.dropped_prims, 0, "{w}x{h} @ DPR {dpr} dropped a prim");
        }
    }
}
