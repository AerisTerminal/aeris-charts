//! Gauge, relationship (graph/chord/network), hierarchy (tree, dendrogram, circle packing,
//! icicle), venn and word cloud layouts: complete-build gating, validation, frame and
//! hit-test agreement, interaction, transitions, and persistence through the public engine
//! path.

use crate::{
    ChartEngine, ErrorCode, GaugeBand, GaugeInput, GeneralLayoutInput, GeneralLayoutSeriesKind,
    GeneralLayoutSeriesOptions, GraphEdge, GraphNode, HierarchyNode, HorizontalDomain,
};

fn layout_chart() -> ChartEngine {
    ChartEngine::new_with_initial_domain(400.0, 300.0, 1.0, HorizontalDomain::Layout).unwrap()
}

fn gauge_input(value: f64) -> GaugeInput {
    GaugeInput {
        value,
        min: 0.0,
        max: 100.0,
        label: "Load".into(),
        unit: "%".into(),
        decimals: None,
        bands: vec![
            GaugeBand {
                to: 30.0,
                color: "#67e0e3".into(),
                label: "Low".into(),
            },
            GaugeBand {
                to: 70.0,
                color: "#37a2da".into(),
                label: String::new(),
            },
            GaugeBand {
                to: 100.0,
                color: "#fd666d".into(),
                label: "High".into(),
            },
        ],
        progress: false,
    }
}

fn graph_input() -> GeneralLayoutInput {
    let node = |id: &str, category: Option<&str>| GraphNode {
        id: id.into(),
        label: id.to_uppercase(),
        category: category.map(Into::into),
        value: None,
        x: None,
        y: None,
    };
    let edge = |source: &str, target: &str, value: f64| GraphEdge {
        source: source.into(),
        target: target.into(),
        value,
    };
    GeneralLayoutInput::Graph {
        nodes: vec![
            node("a", Some("east")),
            node("b", None),
            node("c", Some("east")),
            node("d", None),
        ],
        edges: vec![
            edge("a", "b", 5.0),
            edge("b", "a", 2.0),
            edge("b", "c", 3.0),
            edge("c", "a", 4.0),
            edge("d", "d", 2.0),
            edge("a", "d", 1.5),
        ],
    }
}

fn sets_input() -> GeneralLayoutInput {
    let region = |sets: &[&str], size: f64| crate::VennSet {
        sets: sets.iter().map(|name| (*name).to_owned()).collect(),
        size,
        label: None,
    };
    GeneralLayoutInput::Sets(vec![
        region(&["alpha"], 8.0),
        region(&["beta"], 6.0),
        region(&["alpha", "beta"], 4.0),
    ])
}

fn words_input() -> GeneralLayoutInput {
    let word = |text: &str, weight: f64| crate::WordCloudWord {
        text: text.into(),
        weight,
        color: None,
    };
    GeneralLayoutInput::Words(vec![
        word("alpha", 10.0),
        word("beta", 6.0),
        word("gamma", 4.0),
        word("delta", 2.0),
        word("epsilon", 1.0),
    ])
}

/// r(a(a1 = 1, a2 = 3), b = 4) in rows r, a, b, a1, a2.
fn hierarchy_input() -> GeneralLayoutInput {
    let node = |id: &str, parent: Option<&str>, value: Option<f64>| HierarchyNode {
        id: id.into(),
        parent: parent.map(Into::into),
        label: id.to_uppercase(),
        value,
        color_value: None,
    };
    GeneralLayoutInput::Hierarchy(vec![
        node("r", None, None),
        node("a", Some("r"), None),
        node("b", Some("r"), Some(4.0)),
        node("a1", Some("a"), Some(1.0)),
        node("a2", Some("a"), Some(3.0)),
    ])
}

#[cfg(not(feature = "complete"))]
#[test]
fn standard_build_rejects_gauge_and_graph_data_kinds_and_state_atomically() {
    let mut chart = layout_chart();
    for input in [
        GeneralLayoutInput::Gauge(gauge_input(40.0)),
        graph_input(),
        sets_input(),
        words_input(),
    ] {
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
        .replace_general_layout_dataset(dataset, GeneralLayoutInput::Gauge(gauge_input(1.0)))
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
    assert_eq!(chart.general_layout_dataset(dataset), Some(&funnel));
    for kind in [
        GeneralLayoutSeriesKind::Gauge,
        GeneralLayoutSeriesKind::Chord,
        GeneralLayoutSeriesKind::Tree,
        GeneralLayoutSeriesKind::Dendrogram,
        GeneralLayoutSeriesKind::CirclePacking,
        GeneralLayoutSeriesKind::Icicle,
        GeneralLayoutSeriesKind::Network,
        GeneralLayoutSeriesKind::Venn,
        GeneralLayoutSeriesKind::WordCloud,
    ] {
        let error = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(0, dataset, kind))
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
    }
    assert!(chart.general_layout_series_ids().is_empty());
    let hierarchy = chart
        .create_general_layout_dataset(hierarchy_input())
        .unwrap();
    for kind in [
        GeneralLayoutSeriesKind::Tree,
        GeneralLayoutSeriesKind::Dendrogram,
        GeneralLayoutSeriesKind::CirclePacking,
        GeneralLayoutSeriesKind::Icicle,
    ] {
        let error = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(0, hierarchy, kind))
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
        assert!(error.message().contains("complete build"));
    }
    assert!(chart.general_layout_series_ids().is_empty());
    chart.remove_general_layout_dataset(hierarchy).unwrap();

    chart
        .add_general_layout_series(GeneralLayoutSeriesOptions::new(
            0,
            dataset,
            GeneralLayoutSeriesKind::Funnel,
        ))
        .unwrap();
    let document = chart.export_state_json().unwrap();
    let state: serde_json::Value = serde_json::from_str(&document).unwrap();
    let mut gauge_data = state.clone();
    gauge_data["layout_datasets"][0]["input"] = serde_json::json!({ "Gauge": { "value": 5.0 } });
    let mut graph_data = state.clone();
    graph_data["layout_datasets"][0]["input"] = serde_json::json!({
        "Graph": { "nodes": [{ "id": "a", "label": "A" }], "edges": [] }
    });
    let mut chord_kind = state.clone();
    chord_kind["layout_series"][0]["kind"] = serde_json::json!("Chord");
    let mut tree_kind = state.clone();
    tree_kind["layout_datasets"][0]["input"] = serde_json::json!({
        "Hierarchy": [{ "id": "r", "parent": null, "label": "R", "value": 1.0 }]
    });
    tree_kind["layout_series"][0]["kind"] = serde_json::json!("Tree");
    let mut sets_data = state.clone();
    sets_data["layout_datasets"][0]["input"] =
        serde_json::json!({ "Sets": [{ "sets": ["a"], "size": 1.0 }] });
    let mut words_data = state.clone();
    words_data["layout_datasets"][0]["input"] =
        serde_json::json!({ "Words": [{ "text": "a", "weight": 1.0 }] });
    let mut venn_kind = state.clone();
    venn_kind["layout_series"][0]["kind"] = serde_json::json!("Venn");
    let mut word_cloud_kind = state;
    word_cloud_kind["layout_series"][0]["kind"] = serde_json::json!("WordCloud");
    for invalid in [
        gauge_data,
        graph_data,
        chord_kind,
        tree_kind,
        sets_data,
        words_data,
        venn_kind,
        word_cloud_kind,
    ] {
        let mut fresh = layout_chart();
        let before = fresh.export_state_json().unwrap();
        let error = fresh.import_state_json(&invalid.to_string()).unwrap_err();
        assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
        assert!(error.message().contains("complete build"));
        assert_eq!(fresh.export_state_json().unwrap(), before);
    }
}

#[cfg(not(feature = "complete"))]
#[test]
fn standard_build_restores_layout_series_with_default_complete_family_options() {
    let mut chart = layout_chart();
    let dataset = chart
        .create_general_layout_dataset(hierarchy_input())
        .unwrap();
    chart
        .add_general_layout_series(GeneralLayoutSeriesOptions::new(
            0,
            dataset,
            GeneralLayoutSeriesKind::Treemap,
        ))
        .unwrap();
    let document = chart.export_state_json().unwrap();
    let mut state: serde_json::Value = serde_json::from_str(&document).unwrap();
    let series = state["layout_series"][0].as_object_mut().unwrap();
    for (key, value) in [
        ("hierarchy_orientation", serde_json::json!("horizontal")),
        ("collapsed", serde_json::json!([])),
        ("network_layout", serde_json::json!("force")),
        ("network_link_distance", serde_json::json!(30.0)),
        ("pinned", serde_json::json!([])),
        ("positions", serde_json::json!([])),
        (
            "network_view",
            serde_json::json!({ "x": 0.0, "y": 0.0, "zoom": 1.0 }),
        ),
        ("word_cloud_min_size", serde_json::json!(10.0)),
        ("word_cloud_max_size", serde_json::json!(48.0)),
        ("word_cloud_scaling", serde_json::json!("sqrt")),
        ("word_cloud_rotate", serde_json::json!(false)),
    ] {
        assert!(series.insert(key.into(), value).is_none());
    }
    let mut fresh = layout_chart();
    fresh.import_state_json(&state.to_string()).unwrap();
    assert_eq!(fresh.export_state_json().unwrap(), document);
}

#[cfg(feature = "complete")]
mod complete {
    use super::*;
    use crate::frame::FramePane;
    use crate::{GeneralLayoutElement, GeneralLayoutHit, HierarchyOrientation};
    use aeris_charts_render::draw_list::Prim;

    fn layout(chart: &mut ChartEngine) {
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    }

    fn add(
        chart: &mut ChartEngine,
        input: GeneralLayoutInput,
        kind: GeneralLayoutSeriesKind,
    ) -> (crate::GeneralLayoutDatasetId, crate::GeneralLayoutSeriesId) {
        let dataset = chart.create_general_layout_dataset(input).unwrap();
        let series = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(0, dataset, kind))
            .unwrap();
        layout(chart);
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

    /// Whether a plot point lies in one of a mesh's frame triangles. Frames shift pane content
    /// right by the pane's left edge.
    fn point_in_mesh(
        chart: &ChartEngine,
        pane: &FramePane,
        first: u32,
        count: u32,
        (x, y): (f64, f64),
    ) -> bool {
        let x = x + chart.pane_left;
        let points = &pane.points[first as usize..(first + count) as usize];
        points.as_chunks::<3>().0.iter().any(|triangle| {
            let [a, b, c] = triangle.map(|[px, py]| (f64::from(px), f64::from(py)));
            let cross =
                |u: (f64, f64), v: (f64, f64)| (v.0 - u.0) * (y - u.1) - (v.1 - u.1) * (x - u.0);
            let (d1, d2, d3) = (cross(a, b), cross(b, c), cross(c, a));
            let eps = 1e-3;
            (d1 >= -eps && d2 >= -eps && d3 >= -eps) || (d1 <= eps && d2 <= eps && d3 <= eps)
        })
    }

    /// Every accessibility item with an anchor hits back to itself at that anchor.
    fn assert_anchors_hit_items(
        chart: &ChartEngine,
        series: crate::GeneralLayoutSeriesId,
    ) -> usize {
        let snapshot = chart
            .general_layout_accessibility_snapshot(series, 0, 512)
            .unwrap();
        let mut checked = 0;
        for item in &snapshot.items {
            let hit = GeneralLayoutHit {
                series,
                element: item.element,
                row: item.row,
            };
            let Some((x, y)) = chart.general_layout_hit_anchor(0, hit) else {
                continue;
            };
            assert_eq!(
                chart.general_layout_hit_test(0, x, y),
                Some(hit),
                "{item:?}"
            );
            let (id, label) = chart.general_layout_item_identity(hit).unwrap();
            assert_eq!(
                (id.as_str(), label.as_str()),
                (item.id.as_str(), item.label.as_str())
            );
            checked += 1;
        }
        checked
    }

    #[test]
    fn gauge_and_graph_validation_rejects_invalid_data_and_mismatched_kinds() {
        let mut chart = layout_chart();
        let mut bad = gauge_input(10.0);
        bad.bands.swap(0, 1);
        let error = chart
            .create_general_layout_dataset(GeneralLayoutInput::Gauge(bad))
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidData);
        let GeneralLayoutInput::Graph { nodes, mut edges } = graph_input() else {
            unreachable!();
        };
        edges.push(edges[0].clone());
        assert!(
            chart
                .create_general_layout_dataset(GeneralLayoutInput::Graph {
                    nodes: nodes.clone(),
                    edges: edges.clone(),
                })
                .is_err()
        );
        edges.pop();
        edges[0].target = "missing".into();
        assert!(
            chart
                .create_general_layout_dataset(GeneralLayoutInput::Graph { nodes, edges })
                .is_err()
        );
        let gauge = chart
            .create_general_layout_dataset(GeneralLayoutInput::Gauge(gauge_input(10.0)))
            .unwrap();
        let graph = chart.create_general_layout_dataset(graph_input()).unwrap();
        for (dataset, kind) in [
            (gauge, GeneralLayoutSeriesKind::Chord),
            (graph, GeneralLayoutSeriesKind::Gauge),
            (graph, GeneralLayoutSeriesKind::Sankey),
        ] {
            assert!(
                chart
                    .add_general_layout_series(GeneralLayoutSeriesOptions::new(0, dataset, kind))
                    .is_err(),
                "{kind:?}"
            );
        }
        assert!(chart.general_layout_series_ids().is_empty());
    }

    #[test]
    fn gauge_frame_hits_tooltips_and_accessibility_agree() {
        let mut chart = layout_chart();
        let (_, series) = add(
            &mut chart,
            GeneralLayoutInput::Gauge(gauge_input(42.5)),
            GeneralLayoutSeriesKind::Gauge,
        );
        let frame = chart.build_frame();
        let pane = &frame.panes[0];
        let text = texts(pane);
        assert!(text.contains(&"42.5%".to_owned()), "{text:?}");
        assert!(text.contains(&"Load".to_owned()));
        for scale in ["0", "50", "100"] {
            assert!(text.contains(&scale.to_owned()), "{scale} in {text:?}");
        }
        let meshes: Vec<(u32, u32)> = pane
            .main
            .iter()
            .filter_map(|prim| match prim {
                Prim::MeshFill {
                    first_point,
                    point_count,
                    ..
                } => Some((*first_point, *point_count)),
                _ => None,
            })
            .collect();
        // Three bands and the pointer; the bands cover the whole track.
        assert_eq!(meshes.len(), 4);
        let snapshot = chart
            .general_layout_accessibility_snapshot(series, 0, 16)
            .unwrap();
        assert_eq!(snapshot.total, 4);
        assert_eq!(snapshot.items[0].element, GeneralLayoutElement::Pointer);
        assert_eq!(snapshot.items[0].value, Some(42.5));
        assert_eq!(snapshot.items[2].label, "30 – 70");
        assert_eq!(assert_anchors_hit_items(&chart, series), 4);
        for (index, &(first, count)) in meshes.iter().enumerate() {
            let hit = if index < 3 {
                GeneralLayoutHit {
                    series,
                    element: GeneralLayoutElement::Band,
                    row: index,
                }
            } else {
                GeneralLayoutHit {
                    series,
                    element: GeneralLayoutElement::Pointer,
                    row: 0,
                }
            };
            let anchor = chart.general_layout_hit_anchor(0, hit).unwrap();
            assert!(point_in_mesh(&chart, pane, first, count, anchor), "{hit:?}");
        }
        let pointer = GeneralLayoutHit {
            series,
            element: GeneralLayoutElement::Pointer,
            row: 0,
        };
        let tooltip = chart.general_layout_item_tooltip(0, pointer).unwrap();
        assert_eq!(tooltip.label, "Load");
        assert_eq!(tooltip.detail.as_deref(), Some("42.5%"));
        assert_eq!(chart.general_layout_hit_value(pointer), Some(42.5));
        let band = GeneralLayoutHit {
            series,
            element: GeneralLayoutElement::Band,
            row: 2,
        };
        assert_eq!(chart.general_layout_hit_value(band), Some(100.0));
        assert_eq!(
            chart.general_layout_item_identity(band),
            Some(("band-2".into(), "High".into()))
        );
    }

    #[test]
    fn chord_frame_hits_tooltips_and_accessibility_agree() {
        let mut chart = layout_chart();
        let (_, series) = add(&mut chart, graph_input(), GeneralLayoutSeriesKind::Chord);
        let frame = chart.build_frame();
        let pane = &frame.panes[0];
        let meshes: Vec<(u32, u32)> = pane
            .main
            .iter()
            .filter_map(|prim| match prim {
                Prim::MeshFill {
                    first_point,
                    point_count,
                    ..
                } => Some((*first_point, *point_count)),
                _ => None,
            })
            .collect();
        // Six ribbons in edge order, then four groups.
        assert_eq!(meshes.len(), 10);
        let text = texts(pane);
        for label in ["A", "B", "C", "D"] {
            assert!(text.contains(&label.to_owned()), "{label} in {text:?}");
        }
        let snapshot = chart
            .general_layout_accessibility_snapshot(series, 0, 64)
            .unwrap();
        assert_eq!(snapshot.total, 10);
        // Node values default to incoming plus outgoing flow; a self-link counts both ways.
        let values: Vec<Option<f64>> = snapshot.items.iter().map(|item| item.value).collect();
        assert_eq!(
            &values[..4],
            &[Some(12.5), Some(10.0), Some(7.0), Some(5.5)]
        );
        assert_eq!(snapshot.items[4].element, GeneralLayoutElement::Link);
        assert_eq!(snapshot.items[4].id, "a→b");
        assert_eq!(snapshot.items[4].label, "A to B");
        assert_eq!(snapshot.items[8].label, "D to D");
        assert_eq!(assert_anchors_hit_items(&chart, series), 10);
        for (index, &(first, count)) in meshes.iter().enumerate() {
            let (element, row) = if index < 6 {
                (GeneralLayoutElement::Link, index)
            } else {
                (GeneralLayoutElement::Node, index - 6)
            };
            let hit = GeneralLayoutHit {
                series,
                element,
                row,
            };
            let anchor = chart.general_layout_hit_anchor(0, hit).unwrap();
            assert!(point_in_mesh(&chart, pane, first, count, anchor), "{hit:?}");
        }
        let link = GeneralLayoutHit {
            series,
            element: GeneralLayoutElement::Link,
            row: 2,
        };
        let tooltip = chart.general_layout_item_tooltip(0, link).unwrap();
        assert_eq!(tooltip.label, "B to C");
        assert_eq!(tooltip.value, Some(3.0));
        // Nodes a and c share the "east" category color.
        let color = |row| {
            chart
                .general_layout_item_tooltip(
                    0,
                    GeneralLayoutHit {
                        series,
                        element: GeneralLayoutElement::Node,
                        row,
                    },
                )
                .unwrap()
                .color
        };
        assert_eq!(color(0), color(2));
        assert_ne!(color(0), color(1));
        assert_eq!(chart.general_layout_hit_test(0, 1.0, 1.0), None);
    }

    #[test]
    fn gauge_and_chord_frames_are_deterministic() {
        for (input, kind) in [
            (
                GeneralLayoutInput::Gauge(gauge_input(63.0)),
                GeneralLayoutSeriesKind::Gauge,
            ),
            (graph_input(), GeneralLayoutSeriesKind::Chord),
        ] {
            let build = || {
                let mut chart = layout_chart();
                add(&mut chart, input.clone(), kind);
                let frame = chart.build_frame();
                format!("{:?}{:?}", frame.panes[0].main, frame.panes[0].points)
            };
            assert_eq!(build(), build(), "{kind:?}");
        }
    }

    #[test]
    fn gauge_transitions_interpolate_the_pointer_and_report_the_target_value() {
        let mut chart = layout_chart();
        let (dataset, series) = add(
            &mut chart,
            GeneralLayoutInput::Gauge(gauge_input(20.0)),
            GeneralLayoutSeriesKind::Gauge,
        );
        let pointer_tip = |chart: &mut ChartEngine| {
            let frame = chart.build_frame();
            let pane = &frame.panes[0];
            let Some(Prim::MeshFill { first_point, .. }) = pane
                .main
                .iter()
                .rev()
                .find(|prim| matches!(prim, Prim::MeshFill { .. }))
            else {
                panic!("pointer mesh");
            };
            // The pointer kite's third point is its tip.
            let [x, y] = pane.points[*first_point as usize + 2];
            ((f64::from(x), f64::from(y)), texts(pane))
        };
        let settled_tip = |value: f64| {
            let mut probe = layout_chart();
            add(
                &mut probe,
                GeneralLayoutInput::Gauge(gauge_input(value)),
                GeneralLayoutSeriesKind::Gauge,
            );
            pointer_tip(&mut probe).0
        };
        let (from, to) = (settled_tip(20.0), settled_tip(80.0));
        chart.set_general_transition_duration_ms(200.0).unwrap();
        assert!(chart.set_general_animation_time_ms(1_000.0));
        chart
            .replace_general_layout_dataset(dataset, GeneralLayoutInput::Gauge(gauge_input(80.0)))
            .unwrap();
        assert!(chart.general_transition_active());
        assert!(chart.set_general_animation_time_ms(1_100.0));
        let (tip, text) = pointer_tip(&mut chart);
        // The value text reports the committed target while the pointer is still moving.
        assert!(text.contains(&"80%".to_owned()), "{text:?}");
        assert!(
            tip.0 > from.0 + 1.0 && tip.0 < to.0 - 1.0,
            "{from:?} {tip:?} {to:?}"
        );
        assert!(chart.set_general_animation_time_ms(1_300.0));
        assert!(!chart.general_transition_active());
        let (tip, _) = pointer_tip(&mut chart);
        assert!((tip.0 - to.0).abs() < 1e-3 && (tip.1 - to.1).abs() < 1e-3);
        let _ = series;

        // A different band count is a new topology: the replacement jumps.
        let mut fewer = gauge_input(10.0);
        fewer.bands.pop();
        chart
            .replace_general_layout_dataset(dataset, GeneralLayoutInput::Gauge(fewer))
            .unwrap();
        assert!(!chart.general_transition_active());
    }

    #[test]
    fn gauge_and_graph_layouts_round_trip_through_state() {
        let mut source = layout_chart();
        let mut gauge = gauge_input(12.25);
        gauge.decimals = Some(1);
        gauge.progress = true;
        let inputs = [
            (
                GeneralLayoutInput::Gauge(gauge),
                GeneralLayoutSeriesKind::Gauge,
            ),
            (graph_input(), GeneralLayoutSeriesKind::Chord),
        ];
        for (input, kind) in &inputs {
            let dataset = source.create_general_layout_dataset(input.clone()).unwrap();
            source
                .add_general_layout_series(GeneralLayoutSeriesOptions::new(0, dataset, *kind))
                .unwrap();
        }
        let document = source.export_state_json().unwrap();
        let mut target = layout_chart();
        target.import_state_json(&document).unwrap();
        let ids = target.general_layout_series_ids();
        assert_eq!(ids.len(), 2);
        for (id, (input, kind)) in ids.iter().zip(&inputs) {
            let options = target.general_layout_series_options(*id).unwrap();
            assert_eq!(options.kind, *kind);
            assert_eq!(target.general_layout_dataset(options.dataset), Some(input));
        }
        assert_eq!(target.export_state_json().unwrap(), document);

        let mut invalid: serde_json::Value = serde_json::from_str(&document).unwrap();
        invalid["layout_datasets"][1]["input"]["Graph"]["edges"][0]["source"] =
            serde_json::json!("missing");
        let mut fresh = layout_chart();
        let before = fresh.export_state_json().unwrap();
        assert!(fresh.import_state_json(&invalid.to_string()).is_err());
        assert_eq!(fresh.export_state_json().unwrap(), before);
    }

    fn node_hit(series: crate::GeneralLayoutSeriesId, row: usize) -> GeneralLayoutHit {
        GeneralLayoutHit {
            series,
            element: GeneralLayoutElement::Node,
            row,
        }
    }

    fn add_with(
        chart: &mut ChartEngine,
        kind: GeneralLayoutSeriesKind,
        orientation: HierarchyOrientation,
        collapsed: &[&str],
    ) -> Result<(crate::GeneralLayoutDatasetId, crate::GeneralLayoutSeriesId), crate::ChartError>
    {
        let dataset = chart.create_general_layout_dataset(hierarchy_input())?;
        let mut options = GeneralLayoutSeriesOptions::new(0, dataset, kind);
        options.hierarchy_orientation = orientation;
        options.collapsed = collapsed.iter().map(|id| (*id).to_owned()).collect();
        let series = chart.add_general_layout_series(options)?;
        layout(chart);
        Ok((dataset, series))
    }

    fn assert_near((x, y): (f64, f64), (ex, ey): (f64, f64), what: &str) {
        assert!(
            (x - ex).abs() < 1e-6 && (y - ey).abs() < 1e-6,
            "{what}: ({x}, {y}) != ({ex}, {ey})"
        );
    }

    fn click(chart: &mut ChartEngine, (x, y): (f64, f64), timestamp_ms: f64) {
        let pointer = crate::PointerInput {
            x,
            y,
            timestamp_ms,
            ..Default::default()
        };
        chart.input_pointer_move(pointer, false);
        chart.input_pointer_down(pointer, 1);
        chart.input_pointer_up(pointer);
    }

    fn count(pane: &FramePane, predicate: impl Fn(&Prim) -> bool) -> usize {
        pane.main.iter().filter(|prim| predicate(prim)).count()
    }

    #[test]
    fn hierarchy_layout_validation_rejects_orientations_and_collapsed_ids() {
        let mut chart = layout_chart();
        for (kind, orientation) in [
            (
                GeneralLayoutSeriesKind::Icicle,
                HierarchyOrientation::Radial,
            ),
            (
                GeneralLayoutSeriesKind::CirclePacking,
                HierarchyOrientation::Vertical,
            ),
            (
                GeneralLayoutSeriesKind::CirclePacking,
                HierarchyOrientation::Radial,
            ),
            (
                GeneralLayoutSeriesKind::Treemap,
                HierarchyOrientation::Vertical,
            ),
        ] {
            let error = add_with(&mut chart, kind, orientation, &[]).unwrap_err();
            assert_eq!(error.code(), ErrorCode::InvalidOptions, "{kind:?}");
        }
        for (kind, collapsed) in [
            (GeneralLayoutSeriesKind::Tree, &["a1"][..]),
            (GeneralLayoutSeriesKind::Tree, &["missing"][..]),
            (GeneralLayoutSeriesKind::Dendrogram, &["a", "a"][..]),
            (GeneralLayoutSeriesKind::Icicle, &["a"][..]),
            (GeneralLayoutSeriesKind::CirclePacking, &["r"][..]),
        ] {
            let error = add_with(
                &mut chart,
                kind,
                HierarchyOrientation::Horizontal,
                collapsed,
            )
            .unwrap_err();
            assert_eq!(
                error.code(),
                ErrorCode::InvalidOptions,
                "{kind:?} {collapsed:?}"
            );
        }
        let graph = chart.create_general_layout_dataset(graph_input()).unwrap();
        for kind in [
            GeneralLayoutSeriesKind::Tree,
            GeneralLayoutSeriesKind::Dendrogram,
            GeneralLayoutSeriesKind::CirclePacking,
            GeneralLayoutSeriesKind::Icicle,
        ] {
            assert!(
                chart
                    .add_general_layout_series(GeneralLayoutSeriesOptions::new(0, graph, kind))
                    .is_err()
            );
        }
        assert!(chart.general_layout_series_ids().is_empty());
        let (_, icicle) = add_with(
            &mut chart,
            GeneralLayoutSeriesKind::Icicle,
            HierarchyOrientation::Vertical,
            &[],
        )
        .unwrap();
        let error = chart
            .set_general_layout_hierarchy_orientation(icicle, HierarchyOrientation::Radial)
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidOptions);
        assert_eq!(
            chart
                .general_layout_series_options(icicle)
                .unwrap()
                .hierarchy_orientation,
            HierarchyOrientation::Vertical
        );
        add_with(
            &mut chart,
            GeneralLayoutSeriesKind::Tree,
            HierarchyOrientation::Radial,
            &["a"],
        )
        .unwrap();
    }

    #[test]
    fn tree_and_dendrogram_positions_match_d3_hierarchy() {
        // d3 tree() of r(a(a1, a2), b): breadth x is (0, -0.5, 0.5, -1, 0) for (r, a, b, a1, a2)
        // before normalization; a1 and b are the extremes, cousins, so s = 1 and kx = 1 / 3.5.
        let tree_breadth = [2.0 / 3.5, 1.5 / 3.5, 2.5 / 3.5, 1.0 / 3.5, 2.0 / 3.5];
        let tree_depth = [0.0, 0.5, 0.5, 1.0, 1.0];
        // d3 cluster(): leaves a1, a2, b at 0, 1, 3 (cousins separate by 2), spread over
        // [-1, 4]; every leaf sits at full depth.
        let cluster_breadth = [0.55, 0.3, 0.8, 0.2, 0.4];
        let cluster_depth = [0.0, 0.5, 1.0, 1.0, 1.0];
        for (kind, breadth, depth) in [
            (GeneralLayoutSeriesKind::Tree, tree_breadth, tree_depth),
            (
                GeneralLayoutSeriesKind::Dendrogram,
                cluster_breadth,
                cluster_depth,
            ),
        ] {
            for orientation in [
                HierarchyOrientation::Horizontal,
                HierarchyOrientation::Vertical,
                HierarchyOrientation::Radial,
            ] {
                let mut chart = layout_chart();
                let (_, series) = add_with(&mut chart, kind, orientation, &[]).unwrap();
                let plot = chart.general_plot_rect(0).unwrap();
                let (ix, iy) = (plot.x + plot.width * 0.12, plot.y + plot.height * 0.12);
                let (iw, ih) = (plot.width * 0.76, plot.height * 0.76);
                for row in 0..5 {
                    let expected = match orientation {
                        HierarchyOrientation::Horizontal => {
                            (ix + depth[row] * iw, iy + breadth[row] * ih)
                        }
                        HierarchyOrientation::Vertical => {
                            (ix + breadth[row] * iw, iy + depth[row] * ih)
                        }
                        HierarchyOrientation::Radial => {
                            let angle = breadth[row] * std::f64::consts::TAU;
                            let radius = depth[row] * iw.min(ih) * 0.5;
                            (
                                ix + iw * 0.5 + radius * angle.sin(),
                                iy + ih * 0.5 - radius * angle.cos(),
                            )
                        }
                    };
                    let hit = node_hit(series, row);
                    let anchor = chart.general_layout_hit_anchor(0, hit).unwrap();
                    assert_near(
                        anchor,
                        expected,
                        &format!("{kind:?} {orientation:?} row {row}"),
                    );
                    assert_eq!(
                        chart.general_layout_hit_test(0, anchor.0, anchor.1),
                        Some(hit)
                    );
                }
                assert_eq!(assert_anchors_hit_items(&chart, series), 5);
                let frame = chart.build_frame();
                let pane = &frame.panes[0];
                assert_eq!(count(pane, |prim| matches!(prim, Prim::Polyline { .. })), 4);
                assert_eq!(count(pane, |prim| matches!(prim, Prim::Circle { .. })), 5);
                let text = texts(pane);
                for label in ["R", "A", "B", "A1", "A2"] {
                    assert!(text.contains(&label.to_owned()), "{label} in {text:?}");
                }
            }
        }
    }

    #[test]
    fn tree_nodes_collapse_and_expand_through_the_input_controller() {
        for kind in [
            GeneralLayoutSeriesKind::Tree,
            GeneralLayoutSeriesKind::Dendrogram,
        ] {
            let mut chart = layout_chart();
            let (dataset, series) =
                add_with(&mut chart, kind, HierarchyOrientation::Horizontal, &[]).unwrap();
            // (Filled node circles, link polylines.)
            let marks = |chart: &mut ChartEngine| {
                let frame = chart.build_frame();
                let pane = &frame.panes[0];
                (
                    count(pane, |prim| {
                        matches!(prim, Prim::Circle { fill, stroke, stroke_width, .. }
                            if fill == stroke && *stroke_width > 0.0)
                    }),
                    count(pane, |prim| matches!(prim, Prim::Polyline { .. })),
                )
            };
            assert_eq!(marks(&mut chart), (0, 4));
            let snapshot = chart
                .general_layout_accessibility_snapshot(series, 0, 16)
                .unwrap();
            let ids: Vec<&str> = snapshot.items.iter().map(|item| item.id.as_str()).collect();
            assert_eq!(ids, ["r", "a", "a1", "a2", "b"]);
            let expanded: Vec<Option<bool>> =
                snapshot.items.iter().map(|item| item.expanded).collect();
            assert_eq!(expanded, [Some(true), Some(true), None, None, None]);

            let a = chart
                .general_layout_hit_anchor(0, node_hit(series, 1))
                .unwrap();
            let a1 = chart
                .general_layout_hit_anchor(0, node_hit(series, 3))
                .unwrap();
            assert!(chart.set_general_layout_accessibility_focus(
                node_hit(series, 3),
                "#2962ff",
                14.0
            ));
            click(&mut chart, a, 0.0);
            assert_eq!(
                chart.general_layout_selected_hit(),
                Some(node_hit(series, 1))
            );
            assert_eq!(
                chart
                    .general_layout_series_options(series)
                    .unwrap()
                    .collapsed,
                ["a"]
            );
            // The focused row a1 is now hidden.
            assert_eq!(chart.general_layout_accessibility_focused_hit(), None);
            assert_eq!(marks(&mut chart), (1, 2));
            assert_eq!(
                chart.general_layout_hit_anchor(0, node_hit(series, 3)),
                None
            );
            assert_ne!(
                chart.general_layout_hit_test(0, a1.0, a1.1),
                Some(node_hit(series, 3))
            );
            let snapshot = chart
                .general_layout_accessibility_snapshot(series, 0, 16)
                .unwrap();
            assert_eq!(snapshot.total, 3);
            let ids: Vec<&str> = snapshot.items.iter().map(|item| item.id.as_str()).collect();
            assert_eq!(ids, ["r", "a", "b"]);
            assert_eq!(snapshot.items[1].expanded, Some(false));
            assert!(snapshot.items[1].selected);
            assert_eq!(assert_anchors_hit_items(&chart, series), 3);
            // The visible tree r(a, b) is laid out again with a as a leaf; both layouts put
            // sibling leaves a and b at a quarter and three quarters of the breadth.
            let plot = chart.general_plot_rect(0).unwrap();
            assert_near(
                chart
                    .general_layout_hit_anchor(0, node_hit(series, 1))
                    .unwrap(),
                (
                    plot.x + plot.width * 0.88,
                    plot.y + plot.height * (0.12 + 0.76 * 0.25),
                ),
                &format!("{kind:?} collapsed a"),
            );

            let target = |row| crate::ChartFocusTarget::LayoutDatum {
                series,
                element: GeneralLayoutElement::Node,
                row,
            };
            // Enter on a leaf selects without folding anything.
            assert!(chart.input_target_key_down(
                target(2),
                crate::ChartKey::Enter,
                crate::InputModifiers::default()
            ));
            assert_eq!(
                chart.general_layout_selected_hit(),
                Some(node_hit(series, 2))
            );
            assert_eq!(
                chart
                    .general_layout_series_options(series)
                    .unwrap()
                    .collapsed,
                ["a"]
            );
            // Enter on the collapsed node expands it.
            assert!(chart.input_target_key_down(
                target(1),
                crate::ChartKey::Enter,
                crate::InputModifiers::default()
            ));
            assert!(
                chart
                    .general_layout_series_options(series)
                    .unwrap()
                    .collapsed
                    .is_empty()
            );
            assert_eq!(marks(&mut chart), (0, 4));
            assert_eq!(
                chart
                    .general_layout_accessibility_snapshot(series, 0, 0)
                    .unwrap()
                    .total,
                5
            );
            // A later click folds it again.
            click(&mut chart, a, 5_000.0);
            assert_eq!(
                chart
                    .general_layout_series_options(series)
                    .unwrap()
                    .collapsed,
                ["a"]
            );
            // Data that leaves a without children drops its collapsed ID.
            let GeneralLayoutInput::Hierarchy(mut nodes) = hierarchy_input() else {
                unreachable!();
            };
            nodes.truncate(3);
            nodes[1].value = Some(2.0);
            chart
                .replace_general_layout_dataset(dataset, GeneralLayoutInput::Hierarchy(nodes))
                .unwrap();
            assert!(
                chart
                    .general_layout_series_options(series)
                    .unwrap()
                    .collapsed
                    .is_empty()
            );
        }
    }

    #[test]
    fn circle_packing_matches_d3_pack_and_hits_the_deepest_circle() {
        let mut chart = layout_chart();
        let (_, series) = add_with(
            &mut chart,
            GeneralLayoutSeriesKind::CirclePacking,
            HierarchyOrientation::Horizontal,
            &[],
        )
        .unwrap();
        let plot = chart.general_plot_rect(0).unwrap();
        let size = plot.width.min(plot.height);
        let sqrt3 = 3.0_f64.sqrt();
        // d3 pack(): leaf radii are sqrt(value). An unpadded pass finds the root radius
        // 3 + sqrt 3 that converts the 3px padding to layout units; two siblings sit side by
        // side with the first left of the parent center.
        let p = 3.0 * (3.0 + sqrt3) / size;
        let k = size / (2.0 * (3.0 + sqrt3 + 6.0 * p));
        let (cx, cy) = (plot.x + plot.width * 0.5, plot.y + plot.height * 0.5);
        let a = cx - k * (2.0 + p);
        for (row, center, deepest) in [
            (1, (a, cy), 4),
            (2, (cx + k * (1.0 + sqrt3 + 4.0 * p), cy), 2),
            (3, (a - k * (sqrt3 + p), cy), 3),
            (4, (a + k * (1.0 + p), cy), 4),
        ] {
            if row != 1 {
                let anchor = chart
                    .general_layout_hit_anchor(0, node_hit(series, row))
                    .unwrap();
                assert_near(anchor, center, &format!("row {row}"));
            }
            // A point inside nested circles hits the deepest one.
            assert_eq!(
                chart.general_layout_hit_test(0, center.0, center.1),
                Some(node_hit(series, deepest))
            );
        }
        assert_eq!(assert_anchors_hit_items(&chart, series), 5);
        let frame = chart.build_frame();
        let pane = &frame.panes[0];
        let radii: Vec<f64> = pane
            .main
            .iter()
            .filter_map(|prim| match prim {
                Prim::Circle { radius, .. } => Some(f64::from(*radius)),
                _ => None,
            })
            .collect();
        // Depth-first pre-order: r, a, a1, a2, b.
        let expected = [
            size * 0.5,
            k * (1.0 + sqrt3 + 3.0 * p),
            k,
            k * sqrt3,
            2.0 * k,
        ];
        assert_eq!(radii.len(), 5);
        for (radius, expected) in radii.iter().zip(expected) {
            assert!((radius - expected).abs() < 1e-3, "{radii:?}");
        }
        let text = texts(pane);
        assert!(text.contains(&"B".to_owned()), "{text:?}");
        assert!(!text.contains(&"R".to_owned()), "{text:?}");
    }

    #[test]
    fn icicle_partitions_bands_in_both_orientations() {
        for orientation in [
            HierarchyOrientation::Horizontal,
            HierarchyOrientation::Vertical,
        ] {
            let mut chart = layout_chart();
            let (_, series) = add_with(
                &mut chart,
                GeneralLayoutSeriesKind::Icicle,
                orientation,
                &[],
            )
            .unwrap();
            let plot = chart.general_plot_rect(0).unwrap();
            // d3 partition(): three equal depth bands; breadth splits by summed leaf value
            // r = 8, a = 4, b = 4, a1 = 1, a2 = 3. (breadth center, depth center) fractions.
            let cells = [
                (0.5, 1.0 / 6.0),
                (0.25, 0.5),
                (0.75, 0.5),
                (1.0 / 16.0, 5.0 / 6.0),
                (5.0 / 16.0, 5.0 / 6.0),
            ];
            for (row, (breadth, depth)) in cells.into_iter().enumerate() {
                let expected = match orientation {
                    HierarchyOrientation::Vertical => {
                        (plot.x + breadth * plot.width, plot.y + depth * plot.height)
                    }
                    _ => (plot.x + depth * plot.width, plot.y + breadth * plot.height),
                };
                let hit = node_hit(series, row);
                let anchor = chart.general_layout_hit_anchor(0, hit).unwrap();
                assert_near(anchor, expected, &format!("{orientation:?} row {row}"));
            }
            assert_eq!(assert_anchors_hit_items(&chart, series), 5);
            let frame = chart.build_frame();
            // Depth-first pre-order: r, a, a1, a2, b.
            let rects: Vec<_> = frame.panes[0]
                .main
                .iter()
                .filter_map(|prim| match prim {
                    Prim::Rect { rect, color } => Some((*rect, *color)),
                    _ => None,
                })
                .collect();
            assert_eq!(rects.len(), 5, "{rects:?}");
            // a and its children share a branch color; b is a different branch.
            assert_eq!(rects[1].1, rects[2].1);
            assert_eq!(rects[2].1, rects[3].1);
            assert_ne!(rects[1].1, rects[4].1);
            assert_ne!(rects[0].1, rects[1].1);
            let tooltip = |row| {
                chart
                    .general_layout_item_tooltip(0, node_hit(series, row))
                    .unwrap()
            };
            assert_eq!(tooltip(4).value, Some(3.0));
            assert_eq!(tooltip(3).color, tooltip(4).color);
            assert_ne!(tooltip(3).color, tooltip(2).color);
        }
    }

    #[test]
    fn icicle_transitions_interpolate_partition_breadths() {
        let mut chart = layout_chart();
        let (dataset, series) = add_with(
            &mut chart,
            GeneralLayoutSeriesKind::Icicle,
            HierarchyOrientation::Horizontal,
            &[],
        )
        .unwrap();
        let plot = chart.general_plot_rect(0).unwrap();
        let a1 = |chart: &ChartEngine| {
            chart
                .general_layout_hit_anchor(0, node_hit(series, 3))
                .unwrap()
                .1
        };
        let from = plot.y + plot.height / 16.0;
        assert!((a1(&chart) - from).abs() < 1e-6);
        let GeneralLayoutInput::Hierarchy(mut nodes) = hierarchy_input() else {
            unreachable!();
        };
        nodes[3].value = Some(3.0);
        nodes[4].value = Some(1.0);
        chart.set_general_transition_duration_ms(200.0).unwrap();
        assert!(chart.set_general_animation_time_ms(1_000.0));
        chart
            .replace_general_layout_dataset(dataset, GeneralLayoutInput::Hierarchy(nodes))
            .unwrap();
        assert!(chart.general_transition_active());
        assert!(chart.set_general_animation_time_ms(1_100.0));
        let to = plot.y + plot.height * 3.0 / 16.0;
        let middle = a1(&chart);
        assert!(
            middle > from + 1.0 && middle < to - 1.0,
            "{from} {middle} {to}"
        );
        assert!(chart.set_general_animation_time_ms(1_300.0));
        assert!(!chart.general_transition_active());
        assert!((a1(&chart) - to).abs() < 1e-6);
    }

    #[test]
    fn hierarchy_layout_frames_are_deterministic() {
        for (kind, orientation) in [
            (GeneralLayoutSeriesKind::Tree, HierarchyOrientation::Radial),
            (
                GeneralLayoutSeriesKind::Dendrogram,
                HierarchyOrientation::Vertical,
            ),
            (
                GeneralLayoutSeriesKind::CirclePacking,
                HierarchyOrientation::Horizontal,
            ),
            (
                GeneralLayoutSeriesKind::Icicle,
                HierarchyOrientation::Vertical,
            ),
        ] {
            let build = || {
                let mut chart = layout_chart();
                add_with(&mut chart, kind, orientation, &[]).unwrap();
                let frame = chart.build_frame();
                format!("{:?}{:?}", frame.panes[0].main, frame.panes[0].points)
            };
            assert_eq!(build(), build(), "{kind:?}");
        }
    }

    #[test]
    fn hierarchy_layouts_round_trip_orientation_and_collapse_state() {
        let mut source = layout_chart();
        let specs = [
            (
                GeneralLayoutSeriesKind::Tree,
                HierarchyOrientation::Vertical,
                &["a"][..],
            ),
            (
                GeneralLayoutSeriesKind::Dendrogram,
                HierarchyOrientation::Radial,
                &[][..],
            ),
            (
                GeneralLayoutSeriesKind::CirclePacking,
                HierarchyOrientation::Horizontal,
                &[][..],
            ),
            (
                GeneralLayoutSeriesKind::Icicle,
                HierarchyOrientation::Vertical,
                &[][..],
            ),
        ];
        for (kind, orientation, collapsed) in specs {
            add_with(&mut source, kind, orientation, collapsed).unwrap();
        }
        // Collapse state set through interaction persists too.
        let dendrogram = source.general_layout_series_ids()[1];
        assert!(source.input_target_key_down(
            crate::ChartFocusTarget::LayoutDatum {
                series: dendrogram,
                element: GeneralLayoutElement::Node,
                row: 0,
            },
            crate::ChartKey::Enter,
            crate::InputModifiers::default()
        ));
        let document = source.export_state_json().unwrap();
        let mut target = layout_chart();
        target.import_state_json(&document).unwrap();
        let ids = target.general_layout_series_ids();
        assert_eq!(ids.len(), 4);
        let persisted: [&[&str]; 4] = [&["a"], &["r"], &[], &[]];
        for ((id, (kind, orientation, _)), expected) in ids.iter().zip(specs).zip(persisted) {
            let options = target.general_layout_series_options(*id).unwrap();
            assert_eq!(
                (options.kind, options.hierarchy_orientation),
                (kind, orientation)
            );
            assert_eq!(options.collapsed, expected, "{kind:?}");
        }
        let total = |id| {
            target
                .general_layout_accessibility_snapshot(id, 0, 0)
                .unwrap()
                .total
        };
        assert_eq!((total(ids[0]), total(ids[1])), (3, 1));
        assert_eq!(target.export_state_json().unwrap(), document);

        let mut fresh = layout_chart();
        let before = fresh.export_state_json().unwrap();
        for (series, field, value) in [
            (0, "collapsed", serde_json::json!(["a1"])),
            (3, "hierarchy_orientation", serde_json::json!("radial")),
        ] {
            let mut invalid: serde_json::Value = serde_json::from_str(&document).unwrap();
            invalid["layout_series"][series][field] = value;
            assert!(fresh.import_state_json(&invalid.to_string()).is_err());
            assert_eq!(fresh.export_state_json().unwrap(), before);
        }
    }

    fn add_network(
        chart: &mut ChartEngine,
        input: GeneralLayoutInput,
        configure: impl FnOnce(&mut GeneralLayoutSeriesOptions),
    ) -> Result<(crate::GeneralLayoutDatasetId, crate::GeneralLayoutSeriesId), crate::ChartError>
    {
        let dataset = chart.create_general_layout_dataset(input)?;
        let mut options =
            GeneralLayoutSeriesOptions::new(0, dataset, GeneralLayoutSeriesKind::Network);
        configure(&mut options);
        let series = chart.add_general_layout_series(options)?;
        layout(chart);
        Ok((dataset, series))
    }

    /// Runs every warm network simulation to rest.
    fn settle(chart: &mut ChartEngine) {
        chart.set_general_transition_reduced_motion(true);
        chart.set_general_transition_reduced_motion(false);
    }

    fn node_anchors(chart: &ChartEngine, series: crate::GeneralLayoutSeriesId) -> Vec<(f64, f64)> {
        (0..4)
            .map(|row| {
                chart
                    .general_layout_hit_anchor(0, node_hit(series, row))
                    .unwrap()
            })
            .collect()
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

    fn fixed_graph() -> GeneralLayoutInput {
        let GeneralLayoutInput::Graph { mut nodes, edges } = graph_input() else {
            unreachable!();
        };
        for (node, (x, y)) in
            nodes
                .iter_mut()
                .zip([(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)])
        {
            (node.x, node.y) = (Some(x), Some(y));
        }
        GeneralLayoutInput::Graph { nodes, edges }
    }

    #[test]
    fn network_validation_rejects_invalid_options_and_fixed_layouts_without_coordinates() {
        let mut chart = layout_chart();
        let code = |result: Result<_, crate::ChartError>| result.unwrap_err().code();
        let fixed = |options: &mut GeneralLayoutSeriesOptions| {
            options.network_layout = crate::NetworkLayout::Fixed;
        };
        assert_eq!(
            code(add_network(&mut chart, graph_input(), fixed)),
            ErrorCode::InvalidData
        );
        for configure in [
            (|options: &mut GeneralLayoutSeriesOptions| options.network_link_distance = 0.0)
                as fn(&mut GeneralLayoutSeriesOptions),
            |options| {
                options.network_view = crate::NetworkView {
                    x: 0.0,
                    y: 0.0,
                    zoom: 20.0,
                }
            },
            |options| {
                options.pinned = vec![crate::NetworkPin {
                    id: "missing".into(),
                    x: 0.0,
                    y: 0.0,
                }]
            },
            |options| {
                let pin = crate::NetworkPin {
                    id: "a".into(),
                    x: 0.0,
                    y: f64::NAN,
                };
                options.pinned = vec![pin];
            },
        ] {
            assert_eq!(
                code(add_network(&mut chart, graph_input(), configure)),
                ErrorCode::InvalidOptions
            );
        }
        // Network options do not apply to other graph families.
        let dataset = chart.create_general_layout_dataset(graph_input()).unwrap();
        let mut chord = GeneralLayoutSeriesOptions::new(0, dataset, GeneralLayoutSeriesKind::Chord);
        chord.network_layout = crate::NetworkLayout::Circular;
        assert_eq!(
            chart.add_general_layout_series(chord).unwrap_err().code(),
            ErrorCode::InvalidOptions
        );
        assert!(chart.general_layout_series_ids().is_empty());
        // A fixed layout keeps rejecting data without coordinates, atomically.
        let (dataset, _) = add_network(&mut chart, fixed_graph(), fixed).unwrap();
        let error = chart
            .replace_general_layout_dataset(dataset, graph_input())
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidData);
        assert_eq!(chart.general_layout_dataset(dataset), Some(&fixed_graph()));
    }

    #[test]
    fn force_network_animates_on_host_clocks_and_settles_like_reduced_motion() {
        let mut animated = layout_chart();
        let (_, series) = add_network(&mut animated, graph_input(), |_| {}).unwrap();
        assert!(animated.general_transition_active());
        assert!(animated.input_animating());
        let start = node_anchors(&animated, series);
        // A browser drives both clocks with one timebase; the simulation follows it once.
        let mut frames = 0u32;
        while animated.input_animating() {
            let now = f64::from(frames) * 1_000.0 / 60.0;
            animated.input_tick(now);
            animated.set_general_animation_time_ms(now);
            frames += 1;
            assert!(frames <= 400, "the simulation must settle");
        }
        assert!((300..=303).contains(&frames), "{frames} frames");
        assert!(!animated.general_transition_active());
        let rest = node_anchors(&animated, series);
        assert_ne!(rest, start);

        let mut reduced = layout_chart();
        reduced.set_general_transition_reduced_motion(true);
        let (_, series) = add_network(&mut reduced, graph_input(), |_| {}).unwrap();
        assert!(!reduced.input_animating());
        assert!(!reduced.general_transition_active());
        assert_eq!(node_anchors(&reduced, series), rest);
        let frame = reduced.build_frame();
        // Five straight edges, one self-loop circle and four node discs.
        assert_eq!(
            count(&frame.panes[0], |prim| matches!(
                prim,
                Prim::Polyline { .. }
            )),
            5
        );
        assert_eq!(
            count(&frame.panes[0], |prim| matches!(prim, Prim::Circle { .. })),
            5
        );
        let text = texts(&frame.panes[0]);
        for label in ["A", "B", "C", "D"] {
            assert!(text.contains(&label.to_owned()), "{label} in {text:?}");
        }
    }

    #[test]
    fn network_nodes_hit_tooltip_and_describe_like_their_frame() {
        let mut chart = layout_chart();
        chart.set_general_transition_reduced_motion(true);
        let (_, series) = add_network(&mut chart, graph_input(), |_| {}).unwrap();
        for (row, anchor) in node_anchors(&chart, series).into_iter().enumerate() {
            assert_eq!(
                chart.general_layout_hit_test(0, anchor.0, anchor.1),
                Some(node_hit(series, row))
            );
        }
        let snapshot = chart
            .general_layout_accessibility_snapshot(series, 0, 64)
            .unwrap();
        assert_eq!(snapshot.total, 10);
        let edge = GeneralLayoutHit {
            series,
            element: GeneralLayoutElement::Link,
            row: 2,
        };
        let (x, y) = chart.general_layout_hit_anchor(0, edge).unwrap();
        assert_eq!(chart.general_layout_hit_test(0, x, y), Some(edge));
        let tooltip = chart.general_layout_item_tooltip(0, edge).unwrap();
        assert_eq!(
            (tooltip.label.as_str(), tooltip.value),
            ("B to C", Some(3.0))
        );
        let color = |row| {
            chart
                .general_layout_item_tooltip(0, node_hit(series, row))
                .unwrap()
                .color
        };
        assert_eq!(color(0), color(2));
        assert_ne!(color(0), color(1));
        // A node tooltip anchors at its center; keyboard focus rings it there.
        assert!(chart.set_general_layout_accessibility_focus(node_hit(series, 1), "#2962ff", 14.0));
        assert_eq!(
            chart.general_layout_accessibility_focused_hit(),
            Some(node_hit(series, 1))
        );
    }

    #[test]
    fn network_node_drag_pins_through_the_input_controller_and_escape_restores() {
        let mut chart = layout_chart();
        chart.set_general_transition_reduced_motion(true);
        let (_, series) = add_network(&mut chart, graph_input(), |_| {}).unwrap();
        let before = node_anchors(&chart, series);

        // A click without movement selects the node and pins nothing.
        click(&mut chart, before[1], 0.0);
        assert_eq!(
            chart.general_layout_selected_hit(),
            Some(node_hit(series, 1))
        );
        assert!(
            chart
                .general_layout_series_options(series)
                .unwrap()
                .pinned
                .is_empty()
        );
        chart.input_key_down(
            crate::ChartKey::Escape,
            crate::InputModifiers::default(),
            false,
            0.0,
        );

        // Hovering a node offers a move cursor; dragging grabs it.
        chart.input_pointer_move(pointer(before[0].0, before[0].1, 5.0), false);
        assert_eq!(chart.input_cursor(), crate::ChartCursor::Move);
        press_and_drag(&mut chart, before[0], (40.0, 20.0));
        assert_eq!(chart.input_cursor(), crate::ChartCursor::Grabbing);
        let dragged = chart
            .general_layout_hit_anchor(0, node_hit(series, 0))
            .unwrap();
        assert_near(
            dragged,
            (before[0].0 + 40.0, before[0].1 + 20.0),
            "dragged node",
        );
        // Escape cancels the drag: the node returns and stays unpinned.
        assert!(chart.input_key_down(
            crate::ChartKey::Escape,
            crate::InputModifiers::default(),
            false,
            20.0
        ));
        chart.input_pointer_up(pointer(before[0].0 + 40.0, before[0].1 + 20.0, 21.0));
        assert_eq!(node_anchors(&chart, series), before);
        assert!(
            chart
                .general_layout_series_options(series)
                .unwrap()
                .pinned
                .is_empty()
        );

        // A completed drag keeps the node pinned where it was released and does not select it.
        press_and_drag(&mut chart, before[0], (40.0, 20.0));
        chart.input_pointer_up(pointer(before[0].0 + 40.0, before[0].1 + 20.0, 30.0));
        assert_eq!(chart.general_layout_selected_hit(), None);
        let pinned = chart
            .general_layout_series_options(series)
            .unwrap()
            .pinned
            .clone();
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].id, "a");
        let released = chart
            .general_layout_hit_anchor(0, node_hit(series, 0))
            .unwrap();
        assert_near(
            released,
            (before[0].0 + 40.0, before[0].1 + 20.0),
            "pinned node",
        );
        // Neighbors answered the move.
        assert_ne!(node_anchors(&chart, series)[1], before[1]);
    }

    #[test]
    fn network_view_pans_and_zooms_without_touching_the_time_scale() {
        let mut chart = layout_chart();
        chart.set_general_transition_reduced_motion(true);
        let (_, series) = add_network(&mut chart, graph_input(), |_| {}).unwrap();
        let scroll = chart.scroll_position();
        let range = chart.visible_logical_range();
        let plot = chart.general_plot_rect(0).unwrap();
        let empty = (plot.x + 8.0, plot.y + 8.0);
        assert_eq!(chart.general_layout_hit_test(0, empty.0, empty.1), None);
        let before = node_anchors(&chart, series);

        press_and_drag(&mut chart, empty, (30.0, -10.0));
        assert_eq!(chart.input_cursor(), crate::ChartCursor::Grabbing);
        chart.input_pointer_up(pointer(empty.0 + 30.0, empty.1 - 10.0, 30.0));
        let view = chart
            .general_layout_series_options(series)
            .unwrap()
            .network_view;
        assert_near((view.x, view.y), (-30.0, 10.0), "pan offset");
        for (moved, start) in node_anchors(&chart, series).into_iter().zip(&before) {
            assert_near(moved, (start.0 + 30.0, start.1 - 10.0), "panned node");
        }
        assert_eq!(chart.general_layout_selected_hit(), None);

        // The wheel zooms around the pointer: the node under it stays put.
        let anchor = node_anchors(&chart, series)[2];
        let wheel = crate::WheelSample {
            x: anchor.0,
            y: anchor.1,
            delta_y: 1.0,
            ..Default::default()
        };
        assert!(chart.input_wheel(wheel));
        let zoom = chart
            .general_layout_series_options(series)
            .unwrap()
            .network_view
            .zoom;
        assert!((zoom - 0.2f64.exp2()).abs() < 1e-12, "{zoom}");
        assert_near(node_anchors(&chart, series)[2], anchor, "zoom anchor");
        for _ in 0..40 {
            chart.input_wheel(wheel);
        }
        let zoom = chart
            .general_layout_series_options(series)
            .unwrap()
            .network_view
            .zoom;
        assert_eq!(zoom, 10.0);
        // A panned press restores its view on cancel.
        let view = chart
            .general_layout_series_options(series)
            .unwrap()
            .network_view;
        press_and_drag(&mut chart, empty, (25.0, 25.0));
        chart.input_cancel();
        assert_eq!(
            chart
                .general_layout_series_options(series)
                .unwrap()
                .network_view,
            view
        );
        assert_eq!(chart.scroll_position(), scroll);
        assert_eq!(chart.visible_logical_range(), range);
    }

    #[test]
    fn circular_and_fixed_networks_place_nodes_without_simulation() {
        let mut chart = layout_chart();
        let configure = |options: &mut GeneralLayoutSeriesOptions| {
            options.network_layout = crate::NetworkLayout::Circular;
        };
        let (_, series) = add_network(&mut chart, graph_input(), configure).unwrap();
        assert!(!chart.input_animating());
        let plot = chart.general_plot_rect(0).unwrap();
        let (cx, cy) = (plot.x + plot.width * 0.5, plot.y + plot.height * 0.5);
        let radius = 0.4 * plot.width.min(plot.height);
        let anchors = node_anchors(&chart, series);
        assert_near(anchors[0], (cx, cy - radius), "12 o'clock");
        assert_near(anchors[1], (cx + radius, cy), "3 o'clock");
        assert_near(anchors[2], (cx, cy + radius), "6 o'clock");

        let mut chart = layout_chart();
        let configure = |options: &mut GeneralLayoutSeriesOptions| {
            options.network_layout = crate::NetworkLayout::Fixed;
        };
        let (_, series) = add_network(&mut chart, fixed_graph(), configure).unwrap();
        assert!(!chart.input_animating());
        let plot = chart.general_plot_rect(0).unwrap();
        let scale = (0.8 * plot.width / 100.0).min(0.8 * plot.height / 50.0);
        let (cx, cy) = (plot.x + plot.width * 0.5, plot.y + plot.height * 0.5);
        let anchors = node_anchors(&chart, series);
        assert_near(
            anchors[0],
            (cx - 50.0 * scale, cy - 25.0 * scale),
            "fixed a",
        );
        assert_near(
            anchors[2],
            (cx + 50.0 * scale, cy + 25.0 * scale),
            "fixed c",
        );
    }

    #[test]
    fn network_replacement_keeps_surviving_positions_and_pins() {
        let mut chart = layout_chart();
        let (dataset, series) = add_network(&mut chart, graph_input(), |_| {}).unwrap();
        settle(&mut chart);
        let d = node_anchors(&chart, series)[3];
        press_and_drag(&mut chart, d, (10.0, 0.0));
        chart.input_pointer_up(pointer(0.0, 0.0, 30.0));
        settle(&mut chart);
        let GeneralLayoutInput::Graph {
            mut nodes,
            mut edges,
        } = graph_input()
        else {
            unreachable!();
        };
        let before = node_anchors(&chart, series);
        // Drop the pinned node d and add e.
        nodes.retain(|node| node.id != "d");
        edges.retain(|edge| edge.source != "d" && edge.target != "d");
        nodes.push(GraphNode {
            id: "e".into(),
            label: "E".into(),
            category: None,
            value: None,
            x: None,
            y: None,
        });
        chart
            .replace_general_layout_dataset(dataset, GeneralLayoutInput::Graph { nodes, edges })
            .unwrap();
        // Survivors start where they were and the simulation reheats.
        assert_eq!(node_anchors(&chart, series)[..3], before[..3]);
        assert!(chart.input_animating());
        assert!(
            chart
                .general_layout_series_options(series)
                .unwrap()
                .pinned
                .is_empty()
        );
    }

    #[test]
    fn network_settled_positions_round_trip_after_a_drag() {
        let mut source = layout_chart();
        source.set_general_transition_reduced_motion(true);
        let (_, series) = add_network(&mut source, graph_input(), |_| {}).unwrap();
        settle(&mut source);
        let d = node_anchors(&source, series)[3];
        press_and_drag(&mut source, d, (10.0, 0.0));
        source.input_pointer_up(pointer(0.0, 0.0, 30.0));
        settle(&mut source);
        let document = source.export_state_json().unwrap();
        assert!(document.contains("\"positions\""), "{document}");
        let mut target = layout_chart();
        target.set_general_transition_reduced_motion(true);
        target.import_state_json(&document).unwrap();
        let restored = target.export_state_json().unwrap();
        assert_eq!(restored, document);
    }

    #[test]
    fn network_state_round_trips_decimal_edge_values_bit_exactly() {
        // 17-significant-digit coordinates double-round by 1 ulp under serde_json's
        // default decimal parsing; the float_roundtrip feature keeps them exact so an
        // export/import/export cycle reproduces the identical document.
        let mut source = layout_chart();
        source.set_general_transition_reduced_motion(true);
        let configure = |options: &mut GeneralLayoutSeriesOptions| {
            options.pinned = vec![crate::NetworkPin {
                id: "b".into(),
                x: "106.84328928841207".parse().unwrap(),
                y: "-108.80025347522809".parse().unwrap(),
            }];
            options.network_view = crate::NetworkView {
                x: "1.9417415505581381".parse().unwrap(),
                y: "4.66017972133953".parse().unwrap(),
                zoom: 1.5,
            };
        };
        add_network(&mut source, graph_input(), configure).unwrap();
        settle(&mut source);
        let document = source.export_state_json().unwrap();
        let mut target = layout_chart();
        target.set_general_transition_reduced_motion(true);
        target.import_state_json(&document).unwrap();
        assert_eq!(target.export_state_json().unwrap(), document);
    }

    #[test]
    fn network_layout_pins_and_view_round_trip_through_state() {
        let mut source = layout_chart();
        source.set_general_transition_reduced_motion(true);
        let configure = |options: &mut GeneralLayoutSeriesOptions| {
            options.network_link_distance = 60.0;
            options.pinned = vec![crate::NetworkPin {
                id: "b".into(),
                x: 12.5,
                y: -4.0,
            }];
            options.network_view = crate::NetworkView {
                x: 5.0,
                y: 2.0,
                zoom: 1.5,
            };
        };
        let (_, series) = add_network(&mut source, graph_input(), configure).unwrap();
        let configure = |options: &mut GeneralLayoutSeriesOptions| {
            options.network_layout = crate::NetworkLayout::Circular;
        };
        add_network(&mut source, graph_input(), configure).unwrap();
        let document = source.export_state_json().unwrap();
        let mut target = layout_chart();
        target.set_general_transition_reduced_motion(true);
        target.import_state_json(&document).unwrap();
        let ids = target.general_layout_series_ids();
        // The export captured the settled force positions and the restore reuses them
        // verbatim; everything else in the options is unchanged.
        let restored = target.general_layout_series_options(ids[0]).unwrap();
        assert!(!restored.positions.is_empty());
        let mut expected = source
            .general_layout_series_options(series)
            .unwrap()
            .clone();
        expected.positions = restored.positions.clone();
        assert_eq!(restored, &expected);
        assert_eq!(
            target
                .general_layout_series_options(ids[1])
                .unwrap()
                .network_layout,
            crate::NetworkLayout::Circular
        );
        layout(&mut target);
        // Settled from the same start, the restored network matches the source.
        assert_eq!(node_anchors(&target, ids[0]), node_anchors(&source, series));
        assert_eq!(target.export_state_json().unwrap(), document);

        let mut fresh = layout_chart();
        let before = fresh.export_state_json().unwrap();
        for (field, value) in [
            (
                "pinned",
                serde_json::json!([{ "id": "zz", "x": 0.0, "y": 0.0 }]),
            ),
            (
                "network_view",
                serde_json::json!({ "x": 0.0, "y": 0.0, "zoom": 0.0 }),
            ),
            (
                "positions",
                serde_json::json!([{ "id": "zz", "x": 0.0, "y": 0.0 }]),
            ),
        ] {
            let mut invalid: serde_json::Value = serde_json::from_str(&document).unwrap();
            invalid["layout_series"][0][field] = value;
            assert!(fresh.import_state_json(&invalid.to_string()).is_err());
            assert_eq!(fresh.export_state_json().unwrap(), before);
        }
    }

    fn word_anchor(
        chart: &ChartEngine,
        series: crate::GeneralLayoutSeriesId,
        row: usize,
    ) -> Option<(f64, f64)> {
        chart.general_layout_hit_anchor(
            0,
            GeneralLayoutHit {
                series,
                element: GeneralLayoutElement::Word,
                row,
            },
        )
    }

    #[test]
    fn venn_validation_rejects_invalid_data_and_mismatched_kinds() {
        let mut chart = layout_chart();
        let region = |sets: &[&str], size: f64| crate::VennSet {
            sets: sets.iter().map(|name| (*name).to_owned()).collect(),
            size,
            label: None,
        };
        for input in [
            GeneralLayoutInput::Sets(vec![]),
            GeneralLayoutInput::Sets(vec![region(&["a"], 1.0), region(&["a"], 2.0)]),
            GeneralLayoutInput::Sets(vec![region(&["a"], 1.0), region(&["a", "b"], 0.5)]),
            GeneralLayoutInput::Sets(vec![region(&["a"], f64::NAN)]),
        ] {
            let error = chart.create_general_layout_dataset(input).unwrap_err();
            assert_eq!(error.code(), ErrorCode::InvalidData);
        }
        let words = chart.create_general_layout_dataset(words_input()).unwrap();
        let error = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                words,
                GeneralLayoutSeriesKind::Venn,
            ))
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidData);
        assert!(chart.general_layout_series_ids().is_empty());
        // Word cloud options on a venn series are rejected atomically.
        let sets = chart.create_general_layout_dataset(sets_input()).unwrap();
        let mut options = GeneralLayoutSeriesOptions::new(0, sets, GeneralLayoutSeriesKind::Venn);
        options.word_cloud_rotate = true;
        let error = chart.add_general_layout_series(options).unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidOptions);
        assert!(chart.general_layout_series_ids().is_empty());
    }

    #[test]
    fn venn_regions_paint_hit_and_describe_through_the_frame() {
        let mut chart = layout_chart();
        let (_, series) = add(&mut chart, sets_input(), GeneralLayoutSeriesKind::Venn);
        let frame = chart.build_frame();
        let pane = &frame.panes[0];
        let circles = pane
            .main
            .iter()
            .filter(|prim| matches!(prim, Prim::Circle { .. }))
            .count();
        assert_eq!(circles, 4, "two translucent fills and two outlines");
        assert_eq!(assert_anchors_hit_items(&chart, series), 3);
        let overlap = GeneralLayoutHit {
            series,
            element: GeneralLayoutElement::Region,
            row: 2,
        };
        let tooltip = chart.general_layout_item_tooltip(0, overlap).unwrap();
        assert_eq!(
            (tooltip.label.as_str(), tooltip.value),
            ("alpha \u{2229} beta", Some(4.0))
        );
        let snapshot = chart
            .general_layout_accessibility_snapshot(series, 0, 64)
            .unwrap();
        assert_eq!(snapshot.total, 3);
        assert_eq!(snapshot.items[2].id, "alpha\u{2229}beta");
        assert_eq!(snapshot.items[2].label, "alpha \u{2229} beta");
    }

    #[test]
    fn word_cloud_places_words_deterministically_sized_by_weight_without_overlap() {
        let mut chart = layout_chart();
        let (_, series) = add(
            &mut chart,
            words_input(),
            GeneralLayoutSeriesKind::WordCloud,
        );
        let frame = chart.build_frame();
        let pane = &frame.panes[0];
        let size_of = |text: &str| {
            pane.main
                .iter()
                .find_map(|prim| match prim {
                    Prim::Text {
                        text: run, size, ..
                    } if run == text => Some(*size),
                    _ => None,
                })
                .unwrap_or(0.0)
        };
        assert!(size_of("alpha") > size_of("beta"), "weight scales size");
        assert!(size_of("beta") > size_of("epsilon"), "weight scales size");
        // Every placed anchor hits back its own word, so no two boxes overlap.
        assert_eq!(assert_anchors_hit_items(&chart, series), 5);
        // A fresh chart reproduces the same placement from the same inputs.
        let first: Vec<_> = (0..5).map(|row| word_anchor(&chart, series, row)).collect();
        let mut again = layout_chart();
        let (_, other) = add(
            &mut again,
            words_input(),
            GeneralLayoutSeriesKind::WordCloud,
        );
        let second: Vec<_> = (0..5).map(|row| word_anchor(&again, other, row)).collect();
        assert_eq!(first, second);
        let snapshot = chart
            .general_layout_accessibility_snapshot(series, 0, 64)
            .unwrap();
        assert_eq!(snapshot.total, 5);
        assert!(snapshot.items.iter().all(|item| item.placed == Some(true)));
        let tooltip = chart
            .general_layout_item_tooltip(
                0,
                GeneralLayoutHit {
                    series,
                    element: GeneralLayoutElement::Word,
                    row: 0,
                },
            )
            .unwrap();
        assert_eq!(
            (tooltip.label.as_str(), tooltip.value),
            ("alpha", Some(10.0))
        );
    }

    #[test]
    fn word_cloud_omits_words_that_do_not_fit_and_reports_them() {
        let mut chart =
            ChartEngine::new_with_initial_domain(120.0, 60.0, 1.0, HorizontalDomain::Layout)
                .unwrap();
        let dataset = chart.create_general_layout_dataset(words_input()).unwrap();
        let mut options =
            GeneralLayoutSeriesOptions::new(0, dataset, GeneralLayoutSeriesKind::WordCloud);
        options.word_cloud_min_size = 16.0;
        options.word_cloud_max_size = 48.0;
        let series = chart.add_general_layout_series(options).unwrap();
        layout(&mut chart);
        let snapshot = chart
            .general_layout_accessibility_snapshot(series, 0, 64)
            .unwrap();
        assert_eq!(snapshot.total, 5);
        let placed = snapshot
            .items
            .iter()
            .filter(|item| item.placed == Some(true))
            .count();
        let omitted = snapshot
            .items
            .iter()
            .filter(|item| item.placed == Some(false))
            .count();
        assert!(
            placed >= 1 && omitted >= 1,
            "placed {placed}, omitted {omitted}"
        );
        assert_eq!(placed + omitted, 5);
    }

    #[test]
    fn word_cloud_rotation_uses_rotated_text_prims() {
        let mut chart = layout_chart();
        let dataset = chart.create_general_layout_dataset(words_input()).unwrap();
        let mut options =
            GeneralLayoutSeriesOptions::new(0, dataset, GeneralLayoutSeriesKind::WordCloud);
        options.word_cloud_rotate = true;
        chart.add_general_layout_series(options).unwrap();
        layout(&mut chart);
        let frame = chart.build_frame();
        let rotated = frame.panes[0]
            .main
            .iter()
            .filter(|prim| matches!(prim, Prim::RotatedText { .. }))
            .count();
        assert!(rotated >= 1, "every second word rotates");
    }

    #[test]
    fn word_cloud_replaces_words_when_text_measurements_change() {
        let mut chart = layout_chart();
        chart.set_text_measure(Some(Box::new(|text, size, _, _, _| {
            text.chars().count() as f64 * size * 0.6
        })));
        let (_, series) = add(
            &mut chart,
            words_input(),
            GeneralLayoutSeriesKind::WordCloud,
        );
        let first: Vec<_> = (0..5).map(|row| word_anchor(&chart, series, row)).collect();
        // A wider measurer (a font finished loading) re-places every word box.
        chart.set_text_measure(Some(Box::new(|text, size, _, _, _| {
            text.chars().count() as f64 * size
        })));
        layout(&mut chart);
        let second: Vec<_> = (0..5).map(|row| word_anchor(&chart, series, row)).collect();
        assert_ne!(first, second);
    }

    #[test]
    fn venn_and_word_cloud_options_round_trip_through_state() {
        let mut source = layout_chart();
        let (_, venn) = add(&mut source, sets_input(), GeneralLayoutSeriesKind::Venn);
        let dataset = source.create_general_layout_dataset(words_input()).unwrap();
        let mut options =
            GeneralLayoutSeriesOptions::new(0, dataset, GeneralLayoutSeriesKind::WordCloud);
        options.word_cloud_min_size = 8.0;
        options.word_cloud_max_size = 30.0;
        options.word_cloud_scaling = crate::WordCloudScaling::Linear;
        options.word_cloud_rotate = true;
        let words = source.add_general_layout_series(options).unwrap();
        layout(&mut source);
        let document = source.export_state_json().unwrap();
        let mut target = layout_chart();
        target.import_state_json(&document).unwrap();
        let ids = target.general_layout_series_ids();
        assert_eq!(
            target.general_layout_series_options(ids[0]),
            source.general_layout_series_options(venn)
        );
        assert_eq!(
            target.general_layout_series_options(ids[1]),
            source.general_layout_series_options(words)
        );
        layout(&mut target);
        assert_eq!(target.export_state_json().unwrap(), document);

        let mut fresh = layout_chart();
        let before = fresh.export_state_json().unwrap();
        let mut invalid: serde_json::Value = serde_json::from_str(&document).unwrap();
        invalid["layout_series"][1]["word_cloud_min_size"] = serde_json::json!(400.0);
        assert!(fresh.import_state_json(&invalid.to_string()).is_err());
        assert_eq!(fresh.export_state_json().unwrap(), before);
    }
}
