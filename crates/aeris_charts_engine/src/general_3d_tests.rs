//! 3D charts through the public engine path: complete-build gating, validation, the shared
//! depth-sorted scene frame, front-most picking, tooltips, accessibility, orbit input, camera
//! persistence, and the scene bounds.

use crate::{
    ChartEngine, ErrorCode, GeneralLayoutInput, GeneralLayoutSeriesKind,
    GeneralLayoutSeriesOptions, HorizontalDomain, Point3d, Points3dInput, SurfaceInput,
};

fn layout_chart() -> ChartEngine {
    ChartEngine::new_with_initial_domain(400.0, 300.0, 1.0, HorizontalDomain::Layout).unwrap()
}

fn point(x: f64, y: f64, z: f64) -> Point3d {
    Point3d {
        x,
        y,
        z,
        label: None,
        color: None,
    }
}

fn points(rows: &[[f64; 3]]) -> GeneralLayoutInput {
    GeneralLayoutInput::Points3d(Points3dInput {
        points: rows.iter().map(|&[x, y, z]| point(x, y, z)).collect(),
        ..Points3dInput::default()
    })
}

fn surface() -> GeneralLayoutInput {
    GeneralLayoutInput::Surface(SurfaceInput::sample(
        [-1.0, 1.0, 0.5],
        [-1.0, 1.0, 0.5],
        |x, y| x * x - y * y,
    ))
}

#[test]
fn surface_sampling_follows_the_echarts_gl_equation_grid() {
    let grid = SurfaceInput::sample([-1.0, 1.0, 0.5], [0.0, 2.0, 1.0], |x, y| {
        if x == 0.0 { f64::NAN } else { x + y }
    });
    assert_eq!(grid.x, [-1.0, -0.5, 0.0, 0.5, 1.0]);
    assert_eq!(grid.y, [0.0, 1.0, 2.0]);
    assert_eq!(grid.z.len(), 15);
    assert_eq!(grid.z[5 + 1], Some(0.5), "row 1, column 1: -0.5 + 1");
    assert_eq!(grid.z[2], None, "a non-finite height is a hole");
    assert!(grid.wireframe);
    for empty in [[1.0, -1.0, 0.5], [0.0, 1.0, 0.0], [0.0, f64::INFINITY, 1.0]] {
        assert!(
            SurfaceInput::sample(empty, [0.0, 1.0, 1.0], |_, _| 0.0)
                .x
                .is_empty()
        );
    }
}

#[cfg(not(feature = "complete"))]
#[test]
fn standard_build_rejects_3d_datasets_series_and_state_atomically() {
    let mut chart = layout_chart();
    for input in [points(&[[0.0, 0.0, 1.0]]), surface()] {
        let error = chart.create_general_layout_dataset(input).unwrap_err();
        assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
        assert!(error.message().contains("complete build"));
    }
    let funnel = GeneralLayoutInput::Funnel(vec![crate::FunnelStage {
        id: "one".into(),
        label: "One".into(),
        value: 3.0,
    }]);
    let dataset = chart.create_general_layout_dataset(funnel.clone()).unwrap();
    let error = chart
        .replace_general_layout_dataset(dataset, surface())
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
    assert_eq!(chart.general_layout_dataset(dataset), Some(&funnel));
    for kind in [
        GeneralLayoutSeriesKind::Bar3d,
        GeneralLayoutSeriesKind::Scatter3d,
        GeneralLayoutSeriesKind::Line3d,
        GeneralLayoutSeriesKind::Surface3d,
    ] {
        assert!(kind.requires_complete_build());
        let error = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(0, dataset, kind))
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
    }
    chart
        .add_general_layout_series(GeneralLayoutSeriesOptions::new(
            0,
            dataset,
            GeneralLayoutSeriesKind::Funnel,
        ))
        .unwrap();
    let state: serde_json::Value =
        serde_json::from_str(&chart.export_state_json().unwrap()).unwrap();
    let mut point_data = state.clone();
    point_data["layout_datasets"][0]["input"] =
        serde_json::json!({ "Points3d": { "points": [{ "x": 0, "y": 0, "z": 1 }] } });
    let mut surface_data = state.clone();
    surface_data["layout_datasets"][0]["input"] =
        serde_json::json!({ "Surface": { "x": [0, 1], "y": [0, 1], "z": [0, 1, 2, 3] } });
    let mut invalid = vec![point_data, surface_data];
    for kind in ["Bar3d", "Scatter3d", "Line3d", "Surface3d"] {
        let mut series = state.clone();
        series["layout_series"][0]["kind"] = serde_json::json!(kind);
        invalid.push(series);
    }
    for invalid in invalid {
        let mut fresh = layout_chart();
        let before = fresh.export_state_json().unwrap();
        let error = fresh.import_state_json(&invalid.to_string()).unwrap_err();
        assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
        assert!(error.message().contains("complete build"));
        assert_eq!(fresh.export_state_json().unwrap(), before);
    }
}

#[cfg(feature = "complete")]
mod complete {
    use std::sync::Arc;

    use super::*;
    use crate::frame::FramePane;
    use crate::{
        Camera3d, ChartKey, GeneralLayoutElement, GeneralLayoutHit, GeneralLayoutSeriesId,
        InputModifiers, Projection3d,
    };
    use aeris_charts_render::color::Color;
    use aeris_charts_render::draw_list::Prim;

    fn add(
        chart: &mut ChartEngine,
        input: GeneralLayoutInput,
        kind: GeneralLayoutSeriesKind,
        camera: Camera3d,
    ) -> GeneralLayoutSeriesId {
        let dataset = chart.create_general_layout_dataset(input).unwrap();
        let mut options = GeneralLayoutSeriesOptions::new(0, dataset, kind);
        options.camera_3d = camera;
        let series = chart.add_general_layout_series(options).unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        series
    }

    /// The one mesh of a pane: its triangles' points and colors.
    fn mesh(pane: &FramePane) -> (Vec<[f32; 2]>, Arc<[Color]>) {
        let meshes: Vec<_> = pane
            .main
            .iter()
            .filter_map(|prim| match prim {
                Prim::ColorMesh {
                    first_point,
                    point_count,
                    colors,
                } => Some((
                    pane.points[*first_point as usize..(*first_point + *point_count) as usize]
                        .to_vec(),
                    colors.clone(),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(meshes.len(), 1, "one scene mesh per pane");
        meshes.into_iter().next().unwrap()
    }

    fn texts(pane: &FramePane) -> Vec<String> {
        pane.main
            .iter()
            .filter_map(|prim| match prim {
                Prim::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn camera_of(chart: &ChartEngine, series: GeneralLayoutSeriesId) -> Camera3d {
        chart
            .general_layout_series_options(series)
            .unwrap()
            .camera_3d
    }

    fn pointer(x: f64, y: f64, timestamp_ms: f64) -> crate::PointerInput {
        crate::PointerInput {
            x,
            y,
            timestamp_ms,
            ..Default::default()
        }
    }

    /// Press at `from`, move in four steps by `(dx, dy)`, and leave the button held.
    fn press_and_drag(chart: &mut ChartEngine, from: (f64, f64), (dx, dy): (f64, f64)) {
        chart.input_pointer_move(pointer(from.0, from.1, 10.0), false);
        chart.input_pointer_down(pointer(from.0, from.1, 10.0), 1);
        for step in 1..=4 {
            let t = f64::from(step) * 0.25;
            chart.input_pointer_move(
                pointer(from.0 + dx * t, from.1 + dy * t, 10.0 + f64::from(step)),
                true,
            );
        }
    }

    fn plot_center(chart: &ChartEngine) -> (f64, f64) {
        let plot = chart.general_plot_rect(0).unwrap();
        (plot.x + plot.width / 2.0, plot.y + plot.height / 2.0)
    }

    #[test]
    fn every_family_paints_one_bounded_deterministic_mesh_inside_its_axis_box() {
        let families = [
            (
                GeneralLayoutSeriesKind::Bar3d,
                points(&[
                    [0.0, 0.0, 3.0],
                    [1.0, 0.0, 5.0],
                    [0.0, 1.0, 2.0],
                    [1.0, 1.0, 4.0],
                ]),
            ),
            (
                GeneralLayoutSeriesKind::Scatter3d,
                points(&[[0.0, 0.0, 3.0], [1.0, 2.0, 5.0], [2.0, 1.0, 2.0]]),
            ),
            (
                GeneralLayoutSeriesKind::Line3d,
                points(&[[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [2.0, 0.0, 2.0]]),
            ),
            (GeneralLayoutSeriesKind::Surface3d, surface()),
        ];
        for (kind, input) in families {
            let mut chart = layout_chart();
            add(&mut chart, input, kind, Camera3d::default());
            let frame = chart.build_frame();
            let pane = &frame.panes[0];
            let (corners, colors) = mesh(pane);
            assert!(!colors.is_empty(), "{kind:?} paints triangles");
            assert_eq!(corners.len(), colors.len() * 3, "{kind:?}");
            assert!(colors.iter().all(|color| color.a() == 255), "opaque");
            let plot = chart.general_plot_rect(0).unwrap();
            for [x, y] in &corners {
                let (x, y) = (f64::from(*x), f64::from(*y));
                assert!(
                    x >= plot.x - 1e-3
                        && x <= plot.x + plot.width + 1e-3
                        && y >= plot.y - 1e-3
                        && y <= plot.y + plot.height + 1e-3,
                    "{kind:?}: ({x}, {y}) outside the plot"
                );
            }
            let labels = texts(pane);
            for name in ["x", "y", "z"] {
                assert!(
                    labels.iter().any(|label| label == name),
                    "{kind:?}: {labels:?}"
                );
            }
            let polylines = pane
                .main
                .iter()
                .filter(|prim| matches!(prim, Prim::Polyline { .. }))
                .count();
            assert!(polylines > 9, "{kind:?}: grid, axis lines and ticks");
            assert_eq!(
                chart.build_frame(),
                frame,
                "{kind:?}: frames are deterministic"
            );
        }
    }

    #[test]
    fn scene_items_hit_tooltip_identity_and_accessibility_agree() {
        let mut chart = layout_chart();
        let mut input = Points3dInput {
            points: vec![
                point(0.0, 0.0, 3.0),
                point(1.0, 2.0, 5.0),
                point(2.0, 1.0, 2.0),
            ],
            ..Points3dInput::default()
        };
        input.points[1].label = Some("Peak".into());
        input.points[1].color = Some("#ff0000".into());
        input.x_axis.categories = ["a", "b", "c"].map(String::from).to_vec();
        let series = add(
            &mut chart,
            GeneralLayoutInput::Points3d(input),
            GeneralLayoutSeriesKind::Scatter3d,
            Camera3d::default(),
        );
        let snapshot = chart
            .general_layout_accessibility_snapshot(series, 0, 64)
            .unwrap();
        let rows: Vec<(&str, &str, Option<f64>)> = snapshot
            .items
            .iter()
            .map(|item| (item.id.as_str(), item.label.as_str(), item.value))
            .collect();
        assert_eq!(
            rows,
            [
                ("0", "a, 0", Some(3.0)),
                ("1", "Peak", Some(5.0)),
                ("2", "c, 1", Some(2.0))
            ]
        );
        for item in &snapshot.items {
            assert_eq!(item.element, GeneralLayoutElement::Point);
            let target = GeneralLayoutHit {
                series,
                element: GeneralLayoutElement::Point,
                row: item.row,
            };
            let (x, y) = chart.general_layout_hit_anchor(0, target).unwrap();
            assert_eq!(
                chart.general_layout_hit_test(0, x, y),
                Some(target),
                "{item:?}"
            );
            let tooltip = chart.general_layout_item_tooltip(0, target).unwrap();
            assert_eq!(
                (tooltip.label.as_str(), tooltip.value),
                (item.label.as_str(), item.value)
            );
            assert_eq!(
                chart.general_layout_item_identity(target),
                Some((item.id.clone(), item.label.clone()))
            );
        }
        let red = GeneralLayoutHit {
            series,
            element: GeneralLayoutElement::Point,
            row: 1,
        };
        assert_eq!(
            chart.general_layout_item_tooltip(0, red).unwrap().color,
            Color::rgb(255, 0, 0)
        );
        assert!(chart.set_general_layout_accessibility_focus(red, "#ff8800", 12.0));
        assert!(!chart.select_general_layout_item(series, GeneralLayoutElement::Point, 3));
        assert!(chart.select_general_layout_item(series, GeneralLayoutElement::Point, 2));

        // Surface samples: every drawn sample hits itself; a hole stays addressable without a
        // value or an anchor.
        let mut chart = layout_chart();
        let GeneralLayoutInput::Surface(mut grid) = surface() else {
            unreachable!()
        };
        grid.z[12] = None;
        let series = add(
            &mut chart,
            GeneralLayoutInput::Surface(grid),
            GeneralLayoutSeriesKind::Surface3d,
            Camera3d {
                alpha: 60.0,
                ..Camera3d::default()
            },
        );
        let snapshot = chart
            .general_layout_accessibility_snapshot(series, 0, 64)
            .unwrap();
        assert_eq!(snapshot.total, 25);
        assert_eq!(snapshot.items[12].value, None);
        assert_eq!(snapshot.items[7].id, "2,1");
        assert_eq!(snapshot.items[7].label, "0, -0.5");
        let sample = |row| GeneralLayoutHit {
            series,
            element: GeneralLayoutElement::Sample,
            row,
        };
        assert_eq!(chart.general_layout_hit_anchor(0, sample(12)), None);
        assert_eq!(chart.general_layout_hit_value(sample(12)), None);
        // Probes step 1.5 px from each sample diagonally toward another, so grid corners fall
        // inside a drawn cell rather than on its outline.
        for (row, toward) in [(0, 6), (6, 0), (7, 3), (18, 24), (24, 18)] {
            let inner = chart.general_layout_hit_anchor(0, sample(toward)).unwrap();
            let (x, y) = chart.general_layout_hit_anchor(0, sample(row)).unwrap();
            let (dx, dy) = (inner.0 - x, inner.1 - y);
            let length = dx.hypot(dy).max(1e-9);
            let step = 1.5f64.min(length);
            let hit =
                chart.general_layout_hit_test(0, x + dx / length * step, y + dy / length * step);
            assert_eq!(hit, Some(sample(row)), "sample {row}");
            assert_eq!(
                chart
                    .general_layout_item_tooltip(0, sample(row))
                    .unwrap()
                    .value,
                snapshot.items[row].value
            );
        }
    }

    #[test]
    fn picking_names_the_front_most_item_across_series() {
        let front = Camera3d {
            alpha: 0.0,
            beta: 0.0,
            projection: Projection3d::Orthographic,
            ..Camera3d::default()
        };
        // Data y runs from near to far, so in a straight front view (0, 0, 0) covers
        // (0, 1, 0) exactly, whichever series paints first.
        for near_first in [true, false] {
            let mut chart = layout_chart();
            let [first, second] = if near_first {
                [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
            } else {
                [[0.0, 1.0, 0.0], [0.0, 0.0, 0.0]]
            };
            let a = add(
                &mut chart,
                points(&[first]),
                GeneralLayoutSeriesKind::Scatter3d,
                front,
            );
            let b = add(
                &mut chart,
                points(&[second]),
                GeneralLayoutSeriesKind::Scatter3d,
                front,
            );
            let hit = |series| GeneralLayoutHit {
                series,
                element: GeneralLayoutElement::Point,
                row: 0,
            };
            let at_a = chart.general_layout_hit_anchor(0, hit(a)).unwrap();
            let at_b = chart.general_layout_hit_anchor(0, hit(b)).unwrap();
            assert!((at_a.0 - at_b.0).abs() < 1e-9 && (at_a.1 - at_b.1).abs() < 1e-9);
            let expected = if near_first { a } else { b };
            assert_eq!(
                chart.general_layout_hit_test(0, at_a.0, at_a.1),
                Some(hit(expected))
            );
            // The front symbol paints last.
            let (_, colors) = mesh(&chart.build_frame().panes[0]);
            assert_eq!(colors.len(), 32);
        }

        // A bar in front of another hides it: its top center hits the near bar.
        let mut chart = layout_chart();
        let bars = add(
            &mut chart,
            points(&[[0.0, 1.0, 4.0], [0.0, 0.0, 4.0]]),
            GeneralLayoutSeriesKind::Bar3d,
            Camera3d {
                alpha: 0.0,
                beta: 0.0,
                ..Camera3d::default()
            },
        );
        let near = GeneralLayoutHit {
            series: bars,
            element: GeneralLayoutElement::Point,
            row: 1,
        };
        let (x, y) = chart.general_layout_hit_anchor(0, near).unwrap();
        assert_eq!(chart.general_layout_hit_test(0, x, y + 4.0), Some(near));
    }

    #[test]
    fn orbit_drag_wheel_keys_and_cancel_move_the_pane_camera() {
        let mut chart = layout_chart();
        let bars = add(
            &mut chart,
            points(&[[0.0, 0.0, 3.0], [1.0, 1.0, 5.0]]),
            GeneralLayoutSeriesKind::Bar3d,
            Camera3d::default(),
        );
        let scatter = add(
            &mut chart,
            points(&[[0.5, 0.5, 1.0]]),
            GeneralLayoutSeriesKind::Scatter3d,
            Camera3d::default(),
        );
        let scroll = chart.scroll_position();
        let range = chart.visible_logical_range();
        let plot = chart.general_plot_rect(0).unwrap();
        let start = plot_center(&chart);
        let before = chart.build_frame();

        // A drag right turns the box right; a drag down tilts its top toward the viewer.
        press_and_drag(&mut chart, start, (40.0, 30.0));
        assert_eq!(chart.input_cursor(), crate::ChartCursor::Grabbing);
        chart.input_pointer_up(pointer(start.0 + 40.0, start.1 + 30.0, 20.0));
        let camera = camera_of(&chart, bars);
        let beta = 40.0 - (40.0 / plot.height * 2.0).to_degrees();
        let alpha = 20.0 + (30.0 / plot.width * 2.0).to_degrees();
        assert!((camera.beta - beta).abs() < 1e-9, "{camera:?}");
        assert!((camera.alpha - alpha).abs() < 1e-9, "{camera:?}");
        assert_eq!(camera_of(&chart, scatter), camera, "one camera per pane");
        assert_eq!(
            chart.general_layout_selected_hit(),
            None,
            "a drag is not a click"
        );
        assert_ne!(chart.build_frame(), before, "the frame follows the camera");

        // The wheel moves the camera one notch closer.
        let wheel = crate::WheelSample {
            x: start.0,
            y: start.1,
            delta_y: 1.0,
            ..Default::default()
        };
        assert!(chart.input_wheel(wheel));
        let distance = camera_of(&chart, bars).distance;
        assert!(
            (distance - 200.0 / 0.2f64.exp2()).abs() < 1e-9,
            "{distance}"
        );
        assert_eq!(camera_of(&chart, scatter).distance, distance);

        // Keys orbit the pane under the pointer; Home restores the default view.
        let key = |chart: &mut ChartEngine, key, shift| {
            chart.input_key_down(
                key,
                InputModifiers {
                    shift,
                    ..InputModifiers::default()
                },
                false,
                50.0,
            )
        };
        let turned = camera_of(&chart, bars);
        assert!(key(&mut chart, ChartKey::ArrowLeft, false));
        assert!((camera_of(&chart, bars).beta - (turned.beta + 5.0)).abs() < 1e-9);
        assert!(key(&mut chart, ChartKey::ArrowDown, true));
        assert!((camera_of(&chart, bars).alpha - (turned.alpha + 15.0)).abs() < 1e-9);
        assert!(key(&mut chart, ChartKey::ZoomOut, false));
        assert!((camera_of(&chart, bars).distance - 200.0).abs() < 1e-9);
        assert!(key(&mut chart, ChartKey::Home, false));
        assert_eq!(camera_of(&chart, bars), Camera3d::default());
        assert_eq!(camera_of(&chart, scatter), Camera3d::default());

        // Escape and cancel restore the camera from before the drag.
        press_and_drag(&mut chart, start, (20.0, -20.0));
        assert_ne!(camera_of(&chart, bars), Camera3d::default());
        assert!(key(&mut chart, ChartKey::Escape, false));
        chart.input_pointer_up(pointer(start.0 + 20.0, start.1 - 20.0, 41.0));
        assert_eq!(camera_of(&chart, bars), Camera3d::default());
        press_and_drag(&mut chart, start, (-20.0, 10.0));
        chart.input_cancel();
        assert_eq!(camera_of(&chart, scatter), Camera3d::default());

        // An unmoved press on an item is a click that selects it.
        let item = GeneralLayoutHit {
            series: scatter,
            element: GeneralLayoutElement::Point,
            row: 0,
        };
        let (x, y) = chart.general_layout_hit_anchor(0, item).unwrap();
        chart.input_pointer_move(pointer(x, y, 60.0), false);
        chart.input_pointer_down(pointer(x, y, 60.0), 1);
        chart.input_pointer_up(pointer(x, y, 61.0));
        assert_eq!(chart.general_layout_selected_hit(), Some(item));
        assert_eq!(chart.scroll_position(), scroll);
        assert_eq!(chart.visible_logical_range(), range);
    }

    #[test]
    fn cameras_round_trip_through_state_bit_exactly_and_validate() {
        let mut source = layout_chart();
        add(
            &mut source,
            points(&[[0.0, 0.0, 1.0]]),
            GeneralLayoutSeriesKind::Line3d,
            Camera3d::default(),
        );
        add(
            &mut source,
            surface(),
            GeneralLayoutSeriesKind::Surface3d,
            Camera3d::default(),
        );
        let mut state: serde_json::Value =
            serde_json::from_str(&source.export_state_json().unwrap()).unwrap();
        assert!(
            state["layout_series"][0].get("camera_3d").is_none(),
            "default omitted"
        );
        let camera = serde_json::json!({
            "alpha": 12.345678901234567,
            "beta": -170.1,
            "distance": 123.456,
            "projection": "orthographic",
            "orthographic_size": 77.7
        });
        for series in state["layout_series"].as_array_mut().unwrap() {
            series["camera_3d"] = camera.clone();
        }
        let mut chart = layout_chart();
        chart.import_state_json(&state.to_string()).unwrap();
        let ids = chart.general_layout_series_ids();
        assert_eq!(ids.len(), 2);
        for id in &ids {
            let restored = camera_of(&chart, *id);
            assert_eq!(
                restored.alpha.to_bits(),
                12.345_678_901_234_567f64.to_bits()
            );
            assert_eq!(restored.beta.to_bits(), (-170.1f64).to_bits());
            assert_eq!(restored.projection, Projection3d::Orthographic);
            assert_eq!(restored.orthographic_size.to_bits(), 77.7f64.to_bits());
        }
        let exported = chart.export_state_json().unwrap();
        let mut again = layout_chart();
        again.import_state_json(&exported).unwrap();
        assert_eq!(again.export_state_json().unwrap(), exported);
        assert_eq!(again.build_frame(), chart.build_frame());

        let mut steep = state.clone();
        steep["layout_series"][0]["camera_3d"]["alpha"] = serde_json::json!(95.0);
        let mut close = state.clone();
        close["layout_series"][1]["camera_3d"]["distance"] = serde_json::json!(10.0);
        let mut wrapped = state.clone();
        wrapped["layout_series"][1]["camera_3d"]["beta"] = serde_json::json!(-180.0);
        for invalid in [steep, close, wrapped] {
            let mut fresh = layout_chart();
            let before = fresh.export_state_json().unwrap();
            assert_eq!(
                fresh
                    .import_state_json(&invalid.to_string())
                    .unwrap_err()
                    .code(),
                ErrorCode::InvalidOptions
            );
            assert_eq!(fresh.export_state_json().unwrap(), before);
        }
        let funnel = chart
            .create_general_layout_dataset(GeneralLayoutInput::Funnel(vec![crate::FunnelStage {
                id: "one".into(),
                label: "One".into(),
                value: 3.0,
            }]))
            .unwrap();
        let mut options =
            GeneralLayoutSeriesOptions::new(0, funnel, GeneralLayoutSeriesKind::Funnel);
        options.camera_3d.alpha = 30.0;
        assert_eq!(
            chart.add_general_layout_series(options).unwrap_err().code(),
            ErrorCode::InvalidOptions
        );

        // A later 3D series joins the pane camera instead of keeping its own.
        let mut shared = layout_chart();
        let tilted = Camera3d {
            alpha: 35.0,
            beta: -60.0,
            ..Camera3d::default()
        };
        let first = add(
            &mut shared,
            points(&[[0.0, 0.0, 1.0]]),
            GeneralLayoutSeriesKind::Bar3d,
            tilted,
        );
        let second = add(
            &mut shared,
            surface(),
            GeneralLayoutSeriesKind::Surface3d,
            Camera3d::default(),
        );
        assert_eq!(camera_of(&shared, first), tilted);
        assert_eq!(camera_of(&shared, second), tilted);
    }

    #[test]
    fn validation_is_atomic_and_kinds_match_datasets() {
        let mut chart = layout_chart();
        let valid = points(&[[0.0, 0.0, 1.0]]);
        let dataset = chart.create_general_layout_dataset(valid.clone()).unwrap();
        let with = |edit: fn(&mut Points3dInput)| {
            let GeneralLayoutInput::Points3d(mut input) = points(&[[0.0, 0.0, 1.0]]) else {
                unreachable!()
            };
            edit(&mut input);
            GeneralLayoutInput::Points3d(input)
        };
        let surface_with = |edit: fn(&mut SurfaceInput)| {
            let GeneralLayoutInput::Surface(mut input) = surface() else {
                unreachable!()
            };
            edit(&mut input);
            GeneralLayoutInput::Surface(input)
        };
        let invalid = [
            with(|input| input.points[0].z = f64::NAN),
            with(|input| input.points[0].x = 1e16),
            with(|input| input.size = Some(0.0)),
            with(|input| input.size = Some(101.0)),
            with(|input| input.points[0].color = Some("nope".into())),
            with(|input| input.x_axis.categories = vec!["a".into(), "a".into()]),
            with(|input| input.z_axis.categories = vec!["a".into()]),
            with(|input| {
                input.x_axis.categories = vec!["a".into()];
                input.points[0].x = 1.0;
            }),
            with(|input| {
                input.y_axis.categories = vec!["a".into(), "b".into()];
                input.points[0].y = 0.5;
            }),
            surface_with(|input| input.x = vec![0.0]),
            surface_with(|input| input.x.swap(0, 1)),
            surface_with(|input| input.z.pop().map(drop).unwrap_or(())),
            surface_with(|input| input.z[0] = Some(f64::INFINITY)),
            surface_with(|input| input.colors = vec!["#000".into(); 10]),
            surface_with(|input| input.x_axis.categories = vec!["a".into()]),
        ];
        for input in invalid {
            let error = chart
                .create_general_layout_dataset(input.clone())
                .unwrap_err();
            assert_eq!(error.code(), ErrorCode::InvalidData, "{input:?}: {error:?}");
            assert!(
                chart
                    .replace_general_layout_dataset(dataset, input)
                    .is_err()
            );
            assert_eq!(chart.general_layout_dataset(dataset), Some(&valid));
        }
        let wide = SurfaceInput {
            x: (0..129).map(f64::from).collect(),
            y: vec![0.0, 1.0],
            z: vec![Some(0.0); 258],
            ..SurfaceInput::default()
        };
        assert_eq!(
            chart
                .create_general_layout_dataset(GeneralLayoutInput::Surface(wide))
                .unwrap_err()
                .code(),
            ErrorCode::ResourceLimit
        );
        let grid = chart.create_general_layout_dataset(surface()).unwrap();
        for (dataset, kind) in [
            (grid, GeneralLayoutSeriesKind::Bar3d),
            (grid, GeneralLayoutSeriesKind::Scatter3d),
            (dataset, GeneralLayoutSeriesKind::Surface3d),
            (dataset, GeneralLayoutSeriesKind::Funnel),
        ] {
            let error = chart
                .add_general_layout_series(GeneralLayoutSeriesOptions::new(0, dataset, kind))
                .unwrap_err();
            assert_eq!(error.code(), ErrorCode::InvalidData, "{kind:?}");
        }
    }

    #[test]
    fn replacing_or_hiding_a_member_rebuilds_the_shared_scene() {
        let mut chart = layout_chart();
        let base = add(
            &mut chart,
            points(&[[0.0, 0.0, 1.0]]),
            GeneralLayoutSeriesKind::Bar3d,
            Camera3d::default(),
        );
        let red = |rows: &[[f64; 3]]| {
            let GeneralLayoutInput::Points3d(mut input) = points(rows) else {
                unreachable!()
            };
            for point in &mut input.points {
                point.color = Some("#ff0000".into());
            }
            GeneralLayoutInput::Points3d(input)
        };
        let symbols = |chart: &mut ChartEngine| {
            let (_, colors) = mesh(&chart.build_frame().panes[0]);
            colors
                .iter()
                .filter(|&&color| color == Color::rgb(255, 0, 0))
                .count()
        };
        let dataset = chart
            .create_general_layout_dataset(red(&[[1.0, 1.0, 1.0]]))
            .unwrap();
        let other = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                dataset,
                GeneralLayoutSeriesKind::Scatter3d,
            ))
            .unwrap();
        assert_eq!(symbols(&mut chart), 16);
        // The second series' dataset change reaches the scene its pane's first series holds.
        chart
            .replace_general_layout_dataset(dataset, red(&[[1.0, 1.0, 1.0], [0.0, 1.0, 2.0]]))
            .unwrap();
        assert_eq!(symbols(&mut chart), 32, "one more symbol");
        let show = |chart: &mut ChartEngine, series, visible| {
            chart
                .update_general_layout_series_presentation(
                    series,
                    String::new(),
                    "#4682b4".into(),
                    false,
                    visible,
                )
                .unwrap();
        };
        show(&mut chart, other, false);
        assert_eq!(symbols(&mut chart), 0);
        // Hiding the first series moves the scene to the next visible one.
        show(&mut chart, other, true);
        show(&mut chart, base, false);
        let (_, only) = mesh(&chart.build_frame().panes[0]);
        assert_eq!(only.len(), 32, "only the two symbols");
        chart.remove_general_layout_series(base).unwrap();
        let (_, kept) = mesh(&chart.build_frame().panes[0]);
        assert_eq!(kept, only);
    }

    #[test]
    fn long_surfaces_stride_and_a_pane_scene_stays_bounded() {
        let mut chart = layout_chart();
        add(
            &mut chart,
            GeneralLayoutInput::Surface(SurfaceInput::sample(
                [0.0, 127.0, 1.0],
                [0.0, 127.0, 1.0],
                |x, y| (x * 0.1).sin() * (y * 0.1).cos(),
            )),
            GeneralLayoutSeriesKind::Surface3d,
            Camera3d::default(),
        );
        let (_, colors) = mesh(&chart.build_frame().panes[0]);
        // 64 by 64 drawn cells, each two triangles and four wireframe quads, before clipping.
        assert!(colors.len() <= 64 * 64 * 10 * 2, "{}", colors.len());
        assert!(colors.len() >= 64 * 64, "{}", colors.len());

        let mut chart = layout_chart();
        let rows: Vec<[f64; 3]> = (0..4_096)
            .map(|row| {
                let row = f64::from(row);
                [row % 64.0, (row / 64.0).floor(), (row * 0.37).sin()]
            })
            .collect();
        for _ in 0..9 {
            add(
                &mut chart,
                points(&rows),
                GeneralLayoutSeriesKind::Scatter3d,
                Camera3d::default(),
            );
        }
        let (corners, colors) = mesh(&chart.build_frame().panes[0]);
        assert!(colors.len() <= 131_072, "{}", colors.len());
        assert!(colors.len() > 100_000, "{}", colors.len());
        assert_eq!(corners.len(), colors.len() * 3);
    }

    /// Release timings of rotating panes at the scene bound: a mixed pane (a 128 by 128
    /// surface, 4,096 bars and 4,096 symbols) and a pane of nine 4,096-symbol series that the
    /// 131,072-triangle cap truncates. Each orbit drag step changes the camera, so every timed
    /// frame re-projects, re-shades and re-sorts the whole scene. Run with
    /// `cargo test --release -p aeris_charts_engine --features complete rotation_frame -- --ignored --nocapture`.
    #[test]
    #[ignore = "release timing"]
    fn rotation_frame_and_hit_timings_at_the_scene_bound() {
        use std::time::Instant;
        let grid: Vec<[f64; 3]> = (0..4_096)
            .map(|row| {
                let row = f64::from(row);
                [
                    row % 64.0,
                    (row / 64.0).floor(),
                    1.0 + (row * 0.37).sin().abs() * 9.0,
                ]
            })
            .collect();
        let mixed: &[(GeneralLayoutInput, GeneralLayoutSeriesKind)] = &[
            (
                GeneralLayoutInput::Surface(SurfaceInput::sample(
                    [0.0, 63.0, 63.0 / 127.0],
                    [0.0, 63.0, 63.0 / 127.0],
                    |x, y| 5.0 + 4.0 * (x * 0.2).sin() * (y * 0.2).cos(),
                )),
                GeneralLayoutSeriesKind::Surface3d,
            ),
            (points(&grid), GeneralLayoutSeriesKind::Bar3d),
            (points(&grid), GeneralLayoutSeriesKind::Scatter3d),
        ];
        let capped: Vec<_> = (0..9)
            .map(|_| (points(&grid), GeneralLayoutSeriesKind::Scatter3d))
            .collect();
        for (name, members) in [("mixed", mixed), ("capped", capped.as_slice())] {
            let mut chart =
                ChartEngine::new_with_initial_domain(800.0, 600.0, 1.0, HorizontalDomain::Layout)
                    .unwrap();
            for (input, kind) in members {
                add(&mut chart, input.clone(), *kind, Camera3d::default());
            }
            let started = Instant::now();
            let frame = chart.build_frame();
            let first_ms = started.elapsed().as_secs_f64() * 1e3;
            let triangles = mesh(&frame.panes[0]).1.len();
            let started = Instant::now();
            chart.build_frame();
            let warm_ms = started.elapsed().as_secs_f64() * 1e3;
            let start = plot_center(&chart);
            chart.input_pointer_move(pointer(start.0, start.1, 10.0), false);
            chart.input_pointer_down(pointer(start.0, start.1, 10.0), 1);
            let mut frames = Vec::new();
            for step in 1..=40 {
                let offset = f64::from(step) * 4.0;
                chart.input_pointer_move(pointer(start.0 + offset, start.1, 10.0 + offset), true);
                let started = Instant::now();
                chart.build_frame();
                frames.push(started.elapsed().as_secs_f64() * 1e3);
            }
            chart.input_pointer_up(pointer(start.0 + 160.0, start.1, 200.0));
            frames.sort_by(f64::total_cmp);
            let plot = chart.general_plot_rect(0).unwrap();
            let samples = 20_000u32;
            let started = Instant::now();
            let mut hits = 0usize;
            for sample in 0..samples {
                let t = f64::from(sample) / f64::from(samples);
                let (x, y) = (
                    plot.x + (t * 977.0).fract() * plot.width,
                    plot.y + t * plot.height,
                );
                hits += usize::from(chart.general_layout_hit_test(0, x, y).is_some());
            }
            let hit_us = started.elapsed().as_secs_f64() * 1e6 / f64::from(samples);
            let memory = chart.memory_usage().general_data_capacity_bytes;
            println!(
                "{name}: {triangles} triangles, first frame {first_ms:.2} ms, warm {warm_ms:.3} ms, \
                 rotation frame median {:.2} ms / max {:.2} ms, hit {hit_us:.2} us ({hits} hits), \
                 general data {memory} bytes",
                frames[frames.len() / 2],
                frames[frames.len() - 1],
            );
        }
    }
}
