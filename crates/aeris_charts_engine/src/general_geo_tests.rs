//! Map (choropleth) layouts through the public engine path: complete-build gating, validation,
//! frame and hit-test agreement, tooltips, accessibility, focus, and persistence.

use crate::{
    ChartEngine, ErrorCode, GeneralLayoutInput, GeneralLayoutSeriesKind,
    GeneralLayoutSeriesOptions, GeoFeature, GeoInput, GeoShape, HorizontalDomain,
};

fn layout_chart() -> ChartEngine {
    ChartEngine::new_with_initial_domain(400.0, 300.0, 1.0, HorizontalDomain::Layout).unwrap()
}

fn triangle_input() -> GeoInput {
    GeoInput {
        features: vec![GeoFeature {
            id: "a".into(),
            label: "A".into(),
            shape: GeoShape::Polygon,
            points: vec![[0.0, 0.0], [10.0, 0.0], [5.0, 8.0]],
            rings: vec![3],
            polygons: vec![1],
        }],
        ..GeoInput::default()
    }
}

#[cfg(not(feature = "complete"))]
#[test]
fn standard_build_rejects_map_datasets_series_and_state_atomically() {
    let mut chart = layout_chart();
    let error = chart
        .create_general_layout_dataset(GeneralLayoutInput::Geo(triangle_input()))
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
    assert!(error.message().contains("complete build"));
    let funnel = GeneralLayoutInput::Funnel(vec![crate::FunnelStage {
        id: "one".into(),
        label: "One".into(),
        value: 3.0,
    }]);
    let dataset = chart.create_general_layout_dataset(funnel.clone()).unwrap();
    let error = chart
        .replace_general_layout_dataset(dataset, GeneralLayoutInput::Geo(triangle_input()))
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
    assert_eq!(chart.general_layout_dataset(dataset), Some(&funnel));
    for marks in [
        GeneralLayoutInput::GeoPoints(crate::GeoPointsInput::default()),
        GeneralLayoutInput::GeoLinks(crate::GeoLinksInput::default()),
    ] {
        let error = chart.create_general_layout_dataset(marks).unwrap_err();
        assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
    }
    for kind in [
        GeneralLayoutSeriesKind::Choropleth,
        GeneralLayoutSeriesKind::BubbleMap,
        GeneralLayoutSeriesKind::FlowMap,
    ] {
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
    let mut geo_data = state.clone();
    geo_data["layout_datasets"][0]["input"] = serde_json::json!({ "Geo": {
        "geojson": { "type": "Point", "coordinates": [1, 2] },
        "values": { "#0": 1 }
    } });
    let mut points_data = state.clone();
    points_data["layout_datasets"][0]["input"] =
        serde_json::json!({ "GeoPoints": { "points": [{ "id": "p", "at": [1, 2] }] } });
    let mut links_data = state.clone();
    links_data["layout_datasets"][0]["input"] =
        serde_json::json!({ "GeoLinks": { "links": [{ "source": [0, 0], "target": [1, 2] }] } });
    let mut kinds = Vec::new();
    for kind in ["Choropleth", "BubbleMap", "FlowMap"] {
        let mut invalid = state.clone();
        invalid["layout_series"][0]["kind"] = serde_json::json!(kind);
        kinds.push(invalid);
    }
    for invalid in [geo_data, points_data, links_data].into_iter().chain(kinds) {
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
    use super::*;
    use crate::frame::FramePane;
    use crate::{GeneralLayoutElement, GeneralLayoutHit};
    use aeris_charts_render::draw_list::Prim;

    /// A ring with a hole, a two-island multipolygon, and a feature without a value, in
    /// planar units.
    fn map_json() -> serde_json::Value {
        serde_json::json!({
            "geojson": { "type": "FeatureCollection", "features": [
                { "type": "Feature", "id": "ring", "properties": { "name": "Ring" }, "geometry": {
                    "type": "Polygon", "coordinates": [
                        [[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]],
                        [[3, 3], [3, 7], [7, 7], [7, 3], [3, 3]]
                    ] } },
                { "type": "Feature", "id": "isles", "properties": { "name": "Isles" }, "geometry": {
                    "type": "MultiPolygon", "coordinates": [
                        [[[20, 0], [30, 0], [30, 10], [20, 10], [20, 0]]],
                        [[[40, 0], [45, 0], [45, 5], [40, 5], [40, 0]]]
                    ] } },
                { "type": "Feature", "id": "blank", "properties": { "name": "Blank" }, "geometry": {
                    "type": "Polygon", "coordinates": [[[0, 20], [10, 20], [5, 28], [0, 20]]] } }
            ] },
            "values": { "ring": 2.5, "isles": 7.25 },
            "projection": { "type": "identity" }
        })
    }

    fn map_input(value: serde_json::Value) -> GeneralLayoutInput {
        GeneralLayoutInput::Geo(serde_json::from_value(value).unwrap())
    }

    fn add(
        chart: &mut ChartEngine,
        input: GeneralLayoutInput,
    ) -> (crate::GeneralLayoutDatasetId, crate::GeneralLayoutSeriesId) {
        let dataset = chart.create_general_layout_dataset(input).unwrap();
        let series = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                dataset,
                GeneralLayoutSeriesKind::Choropleth,
            ))
            .unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        (dataset, series)
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

    fn hit(series: crate::GeneralLayoutSeriesId, row: usize) -> GeneralLayoutHit {
        GeneralLayoutHit {
            series,
            element: GeneralLayoutElement::Feature,
            row,
        }
    }

    #[test]
    fn choropleth_frame_hits_tooltips_and_accessibility_agree() {
        let mut chart = layout_chart();
        let (_, series) = add(&mut chart, map_input(map_json()));
        let frame = chart.build_frame();
        let pane = &frame.panes[0];
        let meshes: Vec<_> = pane
            .main
            .iter()
            .filter_map(|prim| match prim {
                Prim::MeshFill { color, .. } => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(meshes.len(), 3, "one mesh per polygon feature");
        assert_ne!(meshes[0], meshes[1], "values map to distinct fills");
        let polylines = pane
            .main
            .iter()
            .filter(|prim| matches!(prim, Prim::Polyline { .. }))
            .count();
        assert_eq!(polylines, 5, "one border per ring");
        let labels = texts(pane);
        assert_eq!(labels.len(), 2, "legend minimum and maximum: {labels:?}");
        assert!(
            labels[0].contains("2.5") && labels[1].contains("7.25"),
            "{labels:?}"
        );

        let snapshot = chart
            .general_layout_accessibility_snapshot(series, 0, 64)
            .unwrap();
        assert_eq!(snapshot.total, 3);
        let rows: Vec<(&str, &str, Option<f64>)> = snapshot
            .items
            .iter()
            .map(|item| (item.id.as_str(), item.label.as_str(), item.value))
            .collect();
        assert_eq!(
            rows,
            [
                ("ring", "Ring", Some(2.5)),
                ("isles", "Isles", Some(7.25)),
                ("blank", "Blank", None)
            ]
        );
        for item in &snapshot.items {
            assert_eq!(item.element, GeneralLayoutElement::Feature);
            let target = hit(series, item.row);
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
            let (id, label) = chart.general_layout_item_identity(target).unwrap();
            assert_eq!(
                (id.as_str(), label.as_str()),
                (item.id.as_str(), item.label.as_str())
            );
        }
        assert_eq!(chart.general_layout_hit_value(hit(series, 2)), None);
        assert!(!chart.select_general_layout_item(series, GeneralLayoutElement::Feature, 3));
        assert!(chart.select_general_layout_item(series, GeneralLayoutElement::Feature, 1));
    }

    #[test]
    fn keyboard_focus_rings_each_feature_at_its_anchor() {
        let mut chart = layout_chart();
        let (_, series) = add(&mut chart, map_input(map_json()));
        let plain = chart.build_frame().panes[0].main.len();
        for row in 0..3 {
            assert!(chart.set_general_layout_accessibility_focus(
                hit(series, row),
                "#ff8800",
                12.0
            ));
            assert_eq!(
                chart.general_layout_accessibility_focused_hit(),
                Some(hit(series, row))
            );
            let focused = chart.build_frame().panes[0].main.len();
            assert!(focused > plain, "row {row} draws a focus ring");
        }
        assert!(!chart.set_general_layout_accessibility_focus(hit(series, 3), "#ff8800", 12.0));
    }

    #[test]
    fn map_validation_is_atomic_and_rejects_mismatched_kinds() {
        let mut chart = layout_chart();
        let (dataset, _) = add(&mut chart, map_input(map_json()));
        let before = chart.general_layout_dataset(dataset).cloned();
        let mut duplicate = map_json();
        duplicate["geojson"]["features"][1]["id"] = serde_json::json!("ring");
        let mut unknown = map_json();
        unknown["values"]["nowhere"] = serde_json::json!(1.0);
        let mut classes = map_json();
        classes["scale"] = serde_json::json!({ "type": "quantize", "classes": 12 });
        let mut color = map_json();
        color["missing_color"] = serde_json::json!("not a color");
        let mut parallels = map_json();
        parallels["projection"] =
            serde_json::json!({ "type": "conic_equal_area", "parallels": [10, 95] });
        for invalid in [duplicate, unknown, classes, color, parallels] {
            let input = map_input(invalid);
            let error = chart
                .create_general_layout_dataset(input.clone())
                .unwrap_err();
            assert_eq!(error.code(), ErrorCode::InvalidData, "{error:?}");
            assert!(
                chart
                    .replace_general_layout_dataset(dataset, input)
                    .is_err()
            );
            assert_eq!(chart.general_layout_dataset(dataset).cloned(), before);
        }
        let mut huge = triangle_input();
        huge.features[0] = GeoFeature {
            id: "many".into(),
            label: "Many".into(),
            shape: GeoShape::Point,
            points: vec![[0.0, 0.0]; 262_145],
            ..GeoFeature::default()
        };
        assert!(
            chart
                .create_general_layout_dataset(GeneralLayoutInput::Geo(huge))
                .is_err()
        );
        let mut broken = triangle_input();
        broken.features[0].rings = vec![4];
        assert!(
            chart
                .create_general_layout_dataset(GeneralLayoutInput::Geo(broken))
                .is_err()
        );

        let funnel = chart
            .create_general_layout_dataset(GeneralLayoutInput::Funnel(vec![crate::FunnelStage {
                id: "one".into(),
                label: "One".into(),
                value: 3.0,
            }]))
            .unwrap();
        for (dataset, kind) in [
            (funnel, GeneralLayoutSeriesKind::Choropleth),
            (dataset, GeneralLayoutSeriesKind::Venn),
            (dataset, GeneralLayoutSeriesKind::Treemap),
        ] {
            assert!(
                chart
                    .add_general_layout_series(GeneralLayoutSeriesOptions::new(0, dataset, kind))
                    .is_err()
            );
        }
    }

    #[test]
    fn map_layouts_round_trip_through_state_bit_exactly() {
        let mut source = layout_chart();
        let mut value = map_json();
        value["geojson"]["features"][0]["geometry"]["coordinates"][0][1] =
            serde_json::from_str("[10.84328928841207, -0.80025347522809]").unwrap();
        value["values"]["isles"] = serde_json::from_str("1.9417415505581381").unwrap();
        value["scale"] =
            serde_json::json!({ "type": "quantize", "classes": 3, "method": "quantile" });
        value["projection"] = serde_json::json!({
            "type": "conic_equal_area", "parallels": [29.5, 45.5], "rotate": [96, 0, 0],
            "center": [-0.6, 38.7], "padding": 12
        });
        value["missing_color"] = serde_json::json!("#cccccc");
        let input = map_input(value);
        let (dataset, _) = add(&mut source, input.clone());
        let document = source.export_state_json().unwrap();
        let mut target = layout_chart();
        target.import_state_json(&document).unwrap();
        let ids = target.general_layout_series_ids();
        let options = target.general_layout_series_options(ids[0]).unwrap();
        assert_eq!(options.kind, GeneralLayoutSeriesKind::Choropleth);
        assert_eq!(target.general_layout_dataset(options.dataset), Some(&input));
        assert_eq!(source.general_layout_dataset(dataset), Some(&input));
        assert_eq!(target.export_state_json().unwrap(), document);

        let mut invalid: serde_json::Value = serde_json::from_str(&document).unwrap();
        invalid["layout_datasets"][0]["input"]["Geo"]["values"][0]["id"] =
            serde_json::json!("missing");
        let mut fresh = layout_chart();
        let before = fresh.export_state_json().unwrap();
        assert!(fresh.import_state_json(&invalid.to_string()).is_err());
        assert_eq!(fresh.export_state_json().unwrap(), before);
    }

    #[test]
    fn choropleth_frames_are_deterministic_and_resize_rebuilds_the_projection() {
        let mut chart = layout_chart();
        let (_, series) = add(&mut chart, map_input(map_json()));
        let first = chart.build_frame();
        let again = chart.build_frame();
        assert_eq!(first.panes[0].main, again.panes[0].main);
        assert_eq!(first.panes[0].points, again.panes[0].points);
        let anchor = chart.general_layout_hit_anchor(0, hit(series, 0)).unwrap();
        chart.css_width = 600.0;
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        let moved = chart.general_layout_hit_anchor(0, hit(series, 0)).unwrap();
        assert_ne!(anchor, moved);
        assert_eq!(
            chart.general_layout_hit_test(0, moved.0, moved.1),
            Some(hit(series, 0))
        );
    }

    fn marks_input(kind: &str, value: serde_json::Value) -> GeneralLayoutInput {
        serde_json::from_value(serde_json::json!({ kind: value })).unwrap()
    }

    fn bubbles_json() -> serde_json::Value {
        serde_json::json!({
            "points": [
                { "id": "blank-bubble", "at": "blank", "value": 4 },
                { "id": "isle", "label": "Isle", "at": [25, 5], "value": 16, "color": "#ff0000" },
                { "id": "far", "at": [1000, 1000], "value": 1 }
            ],
            "radius": [4, 12]
        })
    }

    fn flows_json() -> serde_json::Value {
        serde_json::json!({
            "links": [{ "id": "route", "source": "ring", "target": [42.5, 2.5], "value": 3 }],
            "arrows": true
        })
    }

    fn add_kind(
        chart: &mut ChartEngine,
        input: GeneralLayoutInput,
        kind: GeneralLayoutSeriesKind,
    ) -> crate::GeneralLayoutSeriesId {
        let dataset = chart.create_general_layout_dataset(input).unwrap();
        let series = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(0, dataset, kind))
            .unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        series
    }

    /// A choropleth base map with bubbles and flows over it.
    fn map_chart() -> (
        ChartEngine,
        crate::GeneralLayoutSeriesId,
        crate::GeneralLayoutSeriesId,
        crate::GeneralLayoutSeriesId,
    ) {
        let mut chart = layout_chart();
        let (_, base) = add(&mut chart, map_input(map_json()));
        let bubbles = add_kind(
            &mut chart,
            marks_input("GeoPoints", bubbles_json()),
            GeneralLayoutSeriesKind::BubbleMap,
        );
        let flows = add_kind(
            &mut chart,
            marks_input("GeoLinks", flows_json()),
            GeneralLayoutSeriesKind::FlowMap,
        );
        (chart, base, bubbles, flows)
    }

    fn mark(
        series: crate::GeneralLayoutSeriesId,
        element: GeneralLayoutElement,
        row: usize,
    ) -> GeneralLayoutHit {
        GeneralLayoutHit {
            series,
            element,
            row,
        }
    }

    fn assert_near((x, y): (f64, f64), (ex, ey): (f64, f64), what: &str) {
        assert!(
            (x - ex).abs() < 1e-6 && (y - ey).abs() < 1e-6,
            "{what}: ({x}, {y}) != ({ex}, {ey})"
        );
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

    fn view_of(chart: &ChartEngine, series: crate::GeneralLayoutSeriesId) -> crate::MapView {
        chart
            .general_layout_series_options(series)
            .unwrap()
            .map_view
    }

    #[test]
    fn bubbles_and_flows_project_through_the_base_map_and_hit_tooltip_and_announce() {
        let (mut chart, base, bubbles, flows) = map_chart();
        let bubble = |row| mark(bubbles, GeneralLayoutElement::Bubble, row);
        let flow = mark(flows, GeneralLayoutElement::Link, 0);
        // A bubble anchored to a feature sits at its centroid, which the triangle's
        // choropleth anchor also is.
        let feature = chart.general_layout_hit_anchor(0, hit(base, 2)).unwrap();
        let anchored = chart.general_layout_hit_anchor(0, bubble(0)).unwrap();
        assert_near(anchored, feature, "feature centroid");
        assert_eq!(
            chart.general_layout_hit_anchor(0, bubble(2)),
            None,
            "clipped"
        );

        let frame = chart.build_frame();
        let pane = &frame.panes[0];
        let circles: Vec<f32> = pane
            .main
            .iter()
            .filter_map(|prim| match prim {
                Prim::Circle { radius, .. } => Some(*radius),
                _ => None,
            })
            .collect();
        assert_eq!(
            circles,
            [12.0, 4.0 + 8.0 * 0.5],
            "largest first, sqrt radii"
        );
        let arrows = pane
            .main
            .iter()
            .filter(|prim| matches!(prim, Prim::Triangle { .. }))
            .count();
        assert_eq!(arrows, 1);

        for target in [bubble(0), bubble(1), flow] {
            let (x, y) = chart.general_layout_hit_anchor(0, target).unwrap();
            assert_eq!(chart.general_layout_hit_test(0, x, y), Some(target));
        }
        let (x, y) = chart.general_layout_hit_anchor(0, bubble(1)).unwrap();
        assert_eq!(
            chart.general_layout_hit_test(0, x + 11.5, y),
            Some(bubble(1)),
            "inside the 12 px circle"
        );
        assert_ne!(
            chart.general_layout_hit_test(0, x + 12.5, y),
            Some(bubble(1)),
            "outside the circle"
        );

        let tooltip = chart.general_layout_item_tooltip(0, bubble(1)).unwrap();
        assert_eq!(
            (tooltip.label.as_str(), tooltip.value),
            ("Isle", Some(16.0))
        );
        assert_eq!(
            tooltip.color,
            aeris_charts_render::color::Color::rgb(255, 0, 0)
        );
        let tooltip = chart.general_layout_item_tooltip(0, flow).unwrap();
        assert!(
            tooltip.label.contains("Ring") && tooltip.label.contains("42.5, 2.5"),
            "{}",
            tooltip.label
        );
        assert_eq!(tooltip.value, Some(3.0));

        let snapshot = chart
            .general_layout_accessibility_snapshot(bubbles, 0, 64)
            .unwrap();
        let rows: Vec<(&str, &str, Option<f64>)> = snapshot
            .items
            .iter()
            .map(|item| (item.id.as_str(), item.label.as_str(), item.value))
            .collect();
        assert_eq!(
            rows,
            [
                ("blank-bubble", "Blank", Some(4.0)),
                ("isle", "Isle", Some(16.0)),
                ("far", "far", Some(1.0))
            ]
        );
        let snapshot = chart
            .general_layout_accessibility_snapshot(flows, 0, 64)
            .unwrap();
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.items[0].element, GeneralLayoutElement::Link);
        assert_eq!(snapshot.items[0].id, "route");
        assert_eq!(snapshot.items[0].label, tooltip.label);
        assert_eq!(
            chart.general_layout_item_identity(flow),
            Some(("route".into(), tooltip.label.clone()))
        );
    }

    #[test]
    fn map_view_pans_and_zooms_every_map_series_through_the_controller() {
        let (mut chart, base, bubbles, flows) = map_chart();
        let scroll = chart.scroll_position();
        let range = chart.visible_logical_range();
        let bubble = mark(bubbles, GeneralLayoutElement::Bubble, 1);
        let start = chart.general_layout_hit_anchor(0, bubble).unwrap();

        // At zoom 1 the map cannot pan.
        press_and_drag(&mut chart, start, (12.0, -8.0));
        chart.input_pointer_up(pointer(start.0 + 12.0, start.1 - 8.0, 20.0));
        assert!(view_of(&chart, base).is_default());

        // The wheel zooms around the pointer and takes priority over the time scale.
        let wheel = crate::WheelSample {
            x: start.0,
            y: start.1,
            delta_y: 1.0,
            ..Default::default()
        };
        assert!(chart.input_wheel(wheel));
        let zoom = view_of(&chart, bubbles).zoom;
        assert!((zoom - 0.2f64.exp2()).abs() < 1e-12, "{zoom}");
        assert_near(
            chart.general_layout_hit_anchor(0, bubble).unwrap(),
            start,
            "zoom anchor",
        );
        for _ in 0..4 {
            assert!(chart.input_wheel(wheel));
        }
        assert!((view_of(&chart, base).zoom - 2.0).abs() < 1e-12);
        assert_near(
            chart.general_layout_hit_anchor(0, bubble).unwrap(),
            start,
            "zoom anchor",
        );

        // A drag pans every map series of the pane together.
        let feature = chart.general_layout_hit_anchor(0, hit(base, 1)).unwrap();
        let link = chart
            .general_layout_hit_anchor(0, mark(flows, GeneralLayoutElement::Link, 0))
            .unwrap();
        press_and_drag(&mut chart, start, (12.0, -8.0));
        assert_eq!(chart.input_cursor(), crate::ChartCursor::Grabbing);
        chart.input_pointer_up(pointer(start.0 + 12.0, start.1 - 8.0, 20.0));
        assert_eq!(chart.general_layout_selected_hit(), None);
        let moved = (start.0 + 12.0, start.1 - 8.0);
        assert_near(
            chart.general_layout_hit_anchor(0, bubble).unwrap(),
            moved,
            "panned bubble",
        );
        assert_near(
            chart.general_layout_hit_anchor(0, hit(base, 1)).unwrap(),
            (feature.0 + 12.0, feature.1 - 8.0),
            "panned feature",
        );
        assert_near(
            chart
                .general_layout_hit_anchor(0, mark(flows, GeneralLayoutElement::Link, 0))
                .unwrap(),
            (link.0 + 12.0, link.1 - 8.0),
            "panned flow",
        );
        assert_eq!(
            chart.general_layout_hit_test(0, moved.0, moved.1),
            Some(bubble)
        );
        let view = view_of(&chart, base);
        assert_eq!(view_of(&chart, bubbles), view);
        assert_eq!(view_of(&chart, flows), view);

        // An unmoved press on a bubble is a click that selects it.
        chart.input_pointer_move(pointer(moved.0, moved.1, 30.0), false);
        chart.input_pointer_down(pointer(moved.0, moved.1, 30.0), 1);
        chart.input_pointer_up(pointer(moved.0, moved.1, 31.0));
        assert_eq!(chart.general_layout_selected_hit(), Some(bubble));

        // Escape and cancel restore the view from before the drag.
        press_and_drag(&mut chart, moved, (20.0, 20.0));
        assert_ne!(view_of(&chart, base), view);
        assert!(chart.input_key_down(
            crate::ChartKey::Escape,
            crate::InputModifiers::default(),
            false,
            40.0
        ));
        chart.input_pointer_up(pointer(moved.0 + 20.0, moved.1 + 20.0, 41.0));
        assert_eq!(view_of(&chart, base), view);
        assert_eq!(view_of(&chart, flows), view);
        press_and_drag(&mut chart, moved, (-20.0, 10.0));
        chart.input_cancel();
        assert_eq!(view_of(&chart, base), view);
        assert_eq!(chart.scroll_position(), scroll);
        assert_eq!(chart.visible_logical_range(), range);
    }

    #[test]
    fn map_views_and_marks_round_trip_through_state_bit_exactly() {
        let (source, ..) = map_chart();
        let document = source.export_state_json().unwrap();
        let mut state: serde_json::Value = serde_json::from_str(&document).unwrap();
        let view: serde_json::Value = serde_json::from_str(
            r#"{ "x": 0.12345678901234568, "y": -0.2718281828459045, "zoom": 3.3333333333333335 }"#,
        )
        .unwrap();
        for series in state["layout_series"].as_array_mut().unwrap() {
            series["map_view"] = view.clone();
        }
        let mut chart = layout_chart();
        chart.import_state_json(&state.to_string()).unwrap();
        let ids = chart.general_layout_series_ids();
        assert_eq!(ids.len(), 3);
        for id in &ids {
            let restored = view_of(&chart, *id);
            assert_eq!(restored.x.to_bits(), 0.123_456_789_012_345_68f64.to_bits());
            assert_eq!(
                restored.y.to_bits(),
                (-0.271_828_182_845_904_5f64).to_bits()
            );
            assert_eq!(
                restored.zoom.to_bits(),
                3.333_333_333_333_333_5f64.to_bits()
            );
        }
        let exported = chart.export_state_json().unwrap();
        let mut again = layout_chart();
        again.import_state_json(&exported).unwrap();
        assert_eq!(again.export_state_json().unwrap(), exported);
        let options = chart.general_layout_series_options(ids[1]).unwrap();
        assert_eq!(
            chart.general_layout_dataset(options.dataset),
            Some(&marks_input("GeoPoints", bubbles_json()))
        );

        let mut outside = state.clone();
        outside["layout_series"][0]["map_view"] =
            serde_json::json!({ "x": 0.4, "y": 0.0, "zoom": 2.0 });
        let mut deep = state.clone();
        deep["layout_series"][0]["map_view"] =
            serde_json::json!({ "x": 0.0, "y": 0.0, "zoom": 40.0 });
        for invalid in [outside, deep] {
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
        options.map_view.zoom = 2.0;
        assert_eq!(
            chart.add_general_layout_series(options).unwrap_err().code(),
            ErrorCode::InvalidOptions
        );
    }

    #[test]
    fn mark_validation_is_atomic_and_rejects_mismatched_kinds() {
        let mut chart = layout_chart();
        let points = marks_input("GeoPoints", bubbles_json());
        let dataset = chart.create_general_layout_dataset(points.clone()).unwrap();
        let mut duplicate = bubbles_json();
        duplicate["points"][1]["id"] = serde_json::json!("far");
        let mut negative = bubbles_json();
        negative["points"][0]["value"] = serde_json::json!(-1);
        let mut reversed = bubbles_json();
        reversed["radius"] = serde_json::json!([20, 10]);
        let mut huge = bubbles_json();
        huge["radius"] = serde_json::json!([1, 65]);
        let mut empty = bubbles_json();
        empty["points"][0]["at"] = serde_json::json!("");
        let mut color = bubbles_json();
        color["points"][0]["color"] = serde_json::json!("nope");
        let mut invalid: Vec<GeneralLayoutInput> =
            [duplicate, negative, reversed, huge, empty, color]
                .into_iter()
                .map(|value| marks_input("GeoPoints", value))
                .collect();
        let mut thin = flows_json();
        thin["width"] = serde_json::json!([0.1, 2]);
        let mut twin = flows_json();
        let first = twin["links"][0].clone();
        twin["links"].as_array_mut().unwrap().push(first);
        let mut far = flows_json();
        far["links"][0]["target"] = serde_json::json!([2e9, 0]);
        invalid.extend(
            [thin, twin, far]
                .into_iter()
                .map(|value| marks_input("GeoLinks", value)),
        );
        for input in invalid {
            let error = chart
                .create_general_layout_dataset(input.clone())
                .unwrap_err();
            assert_eq!(error.code(), ErrorCode::InvalidData, "{error:?}");
            assert!(
                chart
                    .replace_general_layout_dataset(dataset, input)
                    .is_err()
            );
            assert_eq!(chart.general_layout_dataset(dataset), Some(&points));
        }
        let links = chart
            .create_general_layout_dataset(marks_input("GeoLinks", flows_json()))
            .unwrap();
        let geo = chart
            .create_general_layout_dataset(map_input(map_json()))
            .unwrap();
        for (dataset, kind) in [
            (geo, GeneralLayoutSeriesKind::BubbleMap),
            (links, GeneralLayoutSeriesKind::BubbleMap),
            (dataset, GeneralLayoutSeriesKind::FlowMap),
            (dataset, GeneralLayoutSeriesKind::Choropleth),
        ] {
            assert!(
                chart
                    .add_general_layout_series(GeneralLayoutSeriesOptions::new(0, dataset, kind))
                    .is_err()
            );
        }
    }

    #[test]
    fn marks_rebuild_when_their_base_map_changes() {
        let (mut chart, base, bubbles, _) = map_chart();
        let bubble = mark(bubbles, GeneralLayoutElement::Bubble, 0);
        let before = chart.general_layout_hit_anchor(0, bubble).unwrap();
        let options = chart.general_layout_series_options(base).unwrap().clone();
        let mut moved = map_json();
        moved["geojson"]["features"][2]["geometry"]["coordinates"] =
            serde_json::json!([[[0, 30], [10, 30], [5, 38], [0, 30]]]);
        chart
            .replace_general_layout_dataset(options.dataset, map_input(moved))
            .unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        let after = chart.general_layout_hit_anchor(0, bubble).unwrap();
        assert_ne!(before, after);
        let feature = chart.general_layout_hit_anchor(0, hit(base, 2)).unwrap();
        assert_near(after, feature, "follows the replaced feature");
    }

    /// Release timings of a world choropleth with 500 bubbles and 500 flows through the
    /// engine: the first frame (projection, triangulation, mark caches), a wheel zoom and a
    /// drag pan frame, and hit tests. Needs `npm ci` in `examples/web_demo`; run with
    /// `cargo test --release -p aeris_charts_engine --features complete world_marks -- --ignored --nocapture`.
    #[test]
    #[ignore = "release timing; reads the world-atlas fixture"]
    fn world_marks_frame_pan_zoom_and_hit_timings() {
        use std::time::Instant;
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        for scale in ["110m", "50m"] {
            let path = format!(
                "{}/../../examples/web_demo/node_modules/world-atlas/countries-{scale}.json",
                env!("CARGO_MANIFEST_DIR")
            );
            let text = std::fs::read_to_string(path)
                .expect("run npm ci in examples/web_demo for the world-atlas fixture");
            let wire = format!(
                r#"{{"topojson":{text},"object":"countries","id_property":"name","projection":{{"type":"natural_earth1"}}}}"#
            );
            let geo: crate::GeoInput = serde_json::from_str(&wire).unwrap();
            let names: Vec<String> = geo.features.iter().map(|f| f.id.clone()).collect();
            let mut anchor = |row: usize| {
                if row.is_multiple_of(2) {
                    serde_json::json!(names[(row * 7) % names.len()])
                } else {
                    serde_json::json!([next() * 360.0 - 180.0, next() * 140.0 - 60.0])
                }
            };
            let points: Vec<serde_json::Value> = (0..500)
                .map(|row| serde_json::json!({ "id": format!("p{row}"), "at": anchor(row), "value": row }))
                .collect();
            let links: Vec<serde_json::Value> = (0..500)
                .map(|row| {
                    serde_json::json!({
                        "source": anchor(row), "target": anchor(row + 1), "value": row
                    })
                })
                .collect();
            let mut chart =
                ChartEngine::new_with_initial_domain(800.0, 600.0, 1.0, HorizontalDomain::Layout)
                    .unwrap();
            let mut series = Vec::new();
            for (input, kind) in [
                (
                    GeneralLayoutInput::Geo(geo.clone()),
                    GeneralLayoutSeriesKind::Choropleth,
                ),
                (
                    marks_input("GeoPoints", serde_json::json!({ "points": points })),
                    GeneralLayoutSeriesKind::BubbleMap,
                ),
                (
                    marks_input(
                        "GeoLinks",
                        serde_json::json!({ "links": links, "arrows": true }),
                    ),
                    GeneralLayoutSeriesKind::FlowMap,
                ),
            ] {
                let dataset = chart.create_general_layout_dataset(input).unwrap();
                series.push(
                    chart
                        .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                            0, dataset, kind,
                        ))
                        .unwrap(),
                );
            }
            chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
            let started = Instant::now();
            let frame = chart.build_frame();
            let first_ms = started.elapsed().as_secs_f64() * 1e3;
            let prims = frame.panes[0].main.len();
            let started = Instant::now();
            chart.build_frame();
            let warm_ms = started.elapsed().as_secs_f64() * 1e3;
            let plot = chart.general_plot_rect(0).unwrap();
            let (cx, cy) = (plot.x + plot.width * 0.4, plot.y + plot.height * 0.4);
            let wheel = crate::WheelSample {
                x: cx,
                y: cy,
                delta_y: 5.0,
                ..Default::default()
            };
            let mut zoom_ms = f64::INFINITY;
            for _ in 0..5 {
                assert!(chart.input_wheel(wheel));
                let started = Instant::now();
                chart.build_frame();
                zoom_ms = zoom_ms.min(started.elapsed().as_secs_f64() * 1e3);
            }
            let zoom = view_of(&chart, series[0]).zoom;
            chart.input_pointer_move(pointer(cx, cy, 100.0), false);
            chart.input_pointer_down(pointer(cx, cy, 100.0), 1);
            let mut pan_ms = f64::INFINITY;
            for step in 1..=10 {
                let offset = f64::from(step) * 6.0;
                chart.input_pointer_move(pointer(cx + offset, cy - offset, 100.0 + offset), true);
                let started = Instant::now();
                chart.build_frame();
                pan_ms = pan_ms.min(started.elapsed().as_secs_f64() * 1e3);
            }
            chart.input_pointer_up(pointer(cx + 60.0, cy - 60.0, 200.0));
            assert!(!view_of(&chart, series[2]).is_default());
            let samples = 20_000;
            let started = Instant::now();
            let mut hits = [0usize; 3];
            for _ in 0..samples {
                let (x, y) = (plot.x + next() * plot.width, plot.y + next() * plot.height);
                if let Some(hit) = chart.general_layout_hit_test(0, x, y) {
                    hits[series.iter().position(|id| *id == hit.series).unwrap()] += 1;
                }
            }
            let hit_us = started.elapsed().as_secs_f64() * 1e6 / f64::from(samples);
            let memory = chart.memory_usage().general_data_capacity_bytes;
            println!(
                "{scale}: first frame {first_ms:.2} ms, warm {warm_ms:.2} ms, zoom {zoom:.2} frame {zoom_ms:.2} ms, \
                 pan frame {pan_ms:.2} ms, hit {hit_us:.2} us (features/bubbles/flows {hits:?}), \
                 {prims} prims, general data {memory} bytes"
            );
        }
    }
}
