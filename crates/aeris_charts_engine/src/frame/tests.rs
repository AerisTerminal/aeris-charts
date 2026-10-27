//! Frame-production unit tests (extracted from `frame.rs`).

use super::conflation::{
    DensityWork, VisibleHistogramRow, VisibleOhlc, visible_histogram_rows_raw_reference,
    visible_line_rows_raw_reference, visible_ohlc_raw_reference,
};
use super::*;
use crate::{
    AxisDimension, AxisPosition, CategoryScaleType, ContinuousScaleType, GeneralAreaBaseline,
    GeneralAxisDomain, GeneralAxisOptions, GeneralAxisTick, GeneralInterpolation, GeneralLineStyle,
    GeneralPaneProposal, GeneralPointSymbol, GeneralReferenceOptions, GeneralReferenceValue,
    GeneralRowId, GeneralRowIdentity, GeneralScaleType, GeneralSeriesKind, GeneralSeriesOptions,
    GeneralSeriesProposal, GeneralStackMode, GeneralXyInput, HorizontalDomain,
};
use aeris_charts_core::model::data_layer::DataLayer;
use aeris_charts_core::model::plot_list::{PlotList, PlotValues};
use aeris_charts_indicators::study_annotations::{StudyAnnotations, StudyMarker, StudyMarkerKind};

const LIVE_TEXT: Color = Color::rgb(0xff, 0xff, 0xff);
const LIVE_COUNTDOWN: Color = Color::rgba(0xff, 0xff, 0xff, 0xb3);

#[test]
fn custom_marker_plot_uses_exact_visible_prices_without_line_or_whitespace_marks() {
    use crate::custom_studies::{
        CustomStudyDefinition, CustomStudyFault, CustomStudyInput, CustomStudyOutput,
        CustomStudyPane, CustomStudyPlot, CustomStudyRuntime,
    };

    struct Marks;
    impl CustomStudyRuntime for Marks {
        fn compute(
            &mut self,
            input: CustomStudyInput<'_>,
            out: &mut [Vec<f64>],
        ) -> Result<(), CustomStudyFault> {
            for row in input.from..input.times.len() {
                out[0].push(if row == 1 {
                    f64::NAN
                } else {
                    input.close[row] + 5.0
                });
            }
            Ok(())
        }
    }

    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let prices = [100.0, 110.0, 120.0, 130.0];
    chart
        .set_series_data(0, &[1.0, 2.0, 3.0, 4.0], &prices, &prices, &prices, &prices)
        .unwrap();
    chart
        .register_custom_study(
            CustomStudyDefinition {
                type_id: "frame_marks".into(),
                version: 1,
                title: "Marks".into(),
                parameters: vec![],
                outputs: vec![CustomStudyOutput {
                    name: "Events".into(),
                    plot: CustomStudyPlot::Marker,
                    pane: CustomStudyPane::Price,
                    default_style: crate::IndicatorOutputStyle {
                        visible: true,
                        line_color: Some("#123456".into()),
                        point_markers: true,
                        ..Default::default()
                    },
                }],
                uses_volume: false,
            },
            Box::new(|_| Ok(Box::new(Marks))),
        )
        .unwrap();
    let output = chart
        .add_custom_study(
            "frame_marks",
            0,
            crate::IndicatorInputSource::Close,
            None,
            Default::default(),
        )
        .unwrap()[0];
    assert!(chart.custom_marker_plot(output));
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let color = Color::rgb(0x12, 0x34, 0x56);
    let frame = chart.build_frame();
    let marks: Vec<_> = frame.panes[0]
        .main
        .iter()
        .filter_map(|prim| match prim {
            Prim::Circle { cx, cy, fill, .. } if *fill == color => Some((*cx, *cy)),
            _ => None,
        })
        .collect();
    assert_eq!(marks.len(), 3);
    let scale = pane_scale(
        &chart.panes[0],
        series_scale_target(chart.series_entry(output).unwrap()),
    );
    let base = chart.series_base_value(output, 0).unwrap();
    for (mark, row) in marks.iter().zip([0, 2, 3]) {
        let logical = chart.data.plot(output).index_at(row).unwrap();
        assert_eq!(
            mark.0,
            chart.time_scale.index_to_coordinate(logical).round() as f32
        );
        assert_eq!(
            mark.1,
            scale.price_to_coordinate(prices[row] + 5.0, base).round() as f32
        );
    }
    assert!(
        !frame.panes[0]
            .main
            .iter()
            .any(|prim| matches!(prim, Prim::Polyline { color: stroke, .. } if *stroke == color))
    );

    chart.set_visible_logical_range(2.0, 3.0);
    let visible = chart.build_frame();
    assert_eq!(
        visible.panes[0]
            .main
            .iter()
            .filter(|prim| matches!(prim, Prim::Circle { fill, .. } if *fill == color))
            .count(),
        2
    );
}

#[test]
fn combined_structure_custom_marker_and_auction_frame_uses_existing_primitives() {
    use crate::custom_studies::{
        CustomStudyDefinition, CustomStudyFault, CustomStudyInput, CustomStudyOutput,
        CustomStudyPane, CustomStudyPlot, CustomStudyRuntime,
    };
    use crate::{
        AggressorSide, AuctionMarkerOptions, FootprintAggregationOptions, FootprintTrade,
        IndicatorInputSource, IndicatorKind, IndicatorOutputStyle,
    };

    struct Marks;
    impl CustomStudyRuntime for Marks {
        fn compute(
            &mut self,
            input: CustomStudyInput<'_>,
            out: &mut [Vec<f64>],
        ) -> Result<(), CustomStudyFault> {
            out[0].extend(
                (input.from..input.times.len())
                    .map(|row| if row == 3 { input.close[row] } else { f64::NAN }),
            );
            Ok(())
        }
    }
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times = (0..7)
        .map(|row| 1_700_000_000.0 + row as f64 * 60.0)
        .collect::<Vec<_>>();
    let open = [9., 12., 12., 16., 16., 18., 19.];
    let high = [10., 14., 12., 18., 16., 20., 21.];
    let low = [8., 9., 10., 15., 15., 8., 16.];
    let close = [9., 13., 11., 17., 15., 19., 20.];
    chart
        .set_series_data(0, &times, &open, &high, &low, &close)
        .unwrap();
    chart.add_indicator_kind(0, IndicatorKind::SwingPoints { left: 1, right: 1 }, None);
    chart
        .register_custom_study(
            CustomStudyDefinition {
                type_id: "combined_marker".into(),
                version: 1,
                title: "Marker".into(),
                parameters: vec![],
                outputs: vec![CustomStudyOutput {
                    name: "event".into(),
                    plot: CustomStudyPlot::Marker,
                    pane: CustomStudyPane::Price,
                    default_style: IndicatorOutputStyle {
                        visible: true,
                        point_markers: true,
                        line_color: Some("#ff00ff".into()),
                        ..Default::default()
                    },
                }],
                uses_volume: false,
            },
            Box::new(|_| Ok(Box::new(Marks))),
        )
        .unwrap();
    let marker = chart
        .add_custom_study(
            "combined_marker",
            0,
            IndicatorInputSource::Close,
            None,
            Default::default(),
        )
        .unwrap()[0];
    let stream = chart
        .add_trade_stream(
            "combined_frame",
            FootprintAggregationOptions {
                tick_size: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
    let trades = [(AggressorSide::Sell, 100.), (AggressorSide::Buy, 25.)]
        .into_iter()
        .enumerate()
        .map(|(offset, (aggressor, volume))| FootprintTrade {
            timestamp_micros: times[0] as i64 * 1_000_000 + offset as i64,
            price: 10.0,
            volume,
            aggressor,
            bid: None,
            ask: None,
            sequence: None,
            trade_id: None,
            conditions: 0,
            session_id: Some(1),
        })
        .collect();
    chart.set_trade_stream_trades(stream, trades).unwrap();
    chart
        .add_auction_markers(
            stream,
            0,
            AuctionMarkerOptions {
                include_forming_bar: true,
                ..Default::default()
            },
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let frame = chart.build_frame();
    let prims = &frame.panes[0].main;
    assert!(chart.custom_marker_plot(marker));
    assert!(
        prims.iter().any(|p| matches!(p, Prim::RoundRect { .. })),
        "swing marker shaft"
    );
    assert!(
        prims
            .iter()
            .any(|p| matches!(p, Prim::Circle { fill, .. } if *fill == Color::rgb(0xff, 0, 0xff))),
        "custom marker"
    );
    assert!(
        prims.iter().any(|p| matches!(p, Prim::Triangle { .. })),
        "auction mark"
    );
}

#[test]
fn study_binding_paints_inside_its_output_layer_without_host_markers() {
    let mut chart = ohlc_chart(SeriesKind::Candlestick, 20);
    let output = chart.add_sma(0, 2).unwrap();
    let mut study = StudyAnnotations::default();
    study.push_marker(StudyMarker {
        row: 4,
        confirm_row: 5,
        price: 108.0,
        kind: StudyMarkerKind::SwingHigh,
    });
    assert!(chart.inject_study_annotations_for_test(output, study));
    let frame = chart.build_frame();
    assert!(
        frame.panes[0]
            .main
            .iter()
            .any(|prim| matches!(prim, Prim::Triangle { .. }))
    );
    assert!(chart.series.iter().all(|series| series.markers.is_empty()));
    chart
        .series
        .iter_mut()
        .find(|series| series.id == output)
        .unwrap()
        .visible = false;
    chart.invalidate_frame_series(output);
    let hidden = chart.build_frame();
    assert!(
        !hidden.panes[0]
            .main
            .iter()
            .any(|prim| matches!(prim, Prim::Triangle { .. }))
    );
}

#[test]
fn study_annotations_paint_swing_glyphs_inside_the_pane() {
    let mut chart = ohlc_chart(SeriesKind::Candlestick, 20);
    chart.build_frame();
    let anchor = chart.series[0].id;
    let mut study = StudyAnnotations::default();
    study.push_marker(StudyMarker {
        row: 9,
        confirm_row: 10,
        price: 109.0,
        kind: StudyMarkerKind::SwingHigh,
    });
    study.push_marker(StudyMarker {
        row: 12,
        confirm_row: 13,
        price: 110.0,
        kind: StudyMarkerKind::SwingLow,
    });
    let mut prims = Vec::new();
    chart.build_study_annotations_frame(anchor, anchor, &study, 0, 19, 800, 1.0, 1.0, &mut prims);
    assert!(matches!(prims[0], Prim::Triangle { .. }));
    assert!(matches!(prims[1], Prim::RoundRect { .. }));
    assert_eq!(
        prims
            .iter()
            .filter(|p| matches!(p, Prim::Triangle { .. }))
            .count(),
        2
    );
    let pane = &chart.panes[0];
    let top = pane.top as f32;
    let bottom = (pane.top + pane.height) as f32;
    for prim in &prims {
        if let Prim::Triangle { a, b, c, .. } = prim {
            for [x, y] in [a, b, c] {
                assert!((0.0..=800.0).contains(x));
                assert!(*y >= top && *y <= bottom);
            }
        }
    }
}

#[test]
fn study_annotations_cull_offscreen_rows() {
    let mut chart = ohlc_chart(SeriesKind::Candlestick, 200);
    chart.build_frame();
    let anchor = chart.series[0].id;
    let mut study = StudyAnnotations::default();
    for row in [3, 31] {
        study.push_marker(StudyMarker {
            row,
            confirm_row: row,
            price: 100.0 + row as f64,
            kind: StudyMarkerKind::SwingLow,
        });
    }
    chart.time_scale.set_bar_spacing(2.0);
    let mut prims = Vec::new();
    chart.build_study_annotations_frame(anchor, anchor, &study, 20, 40, 800, 1.0, 1.0, &mut prims);
    assert_eq!(
        prims
            .iter()
            .filter(|p| matches!(p, Prim::Triangle { .. }))
            .count(),
        1
    );
    chart.series[0].visible = false;
    prims.clear();
    chart.build_study_annotations_frame(anchor, anchor, &study, 20, 40, 800, 1.0, 1.0, &mut prims);
    assert!(prims.is_empty());
}

#[test]
fn axis_text_uses_the_run_font_for_the_shared_cap_center_metric() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let expected_family = chart.options.get().layout.font_family.clone();
    chart.set_text_cap_center(Some(Box::new(move |size, family, weight, italic| {
        assert_eq!(family, expected_family);
        assert!(!italic);
        crate::TextCapMetrics {
            center_offset: size / 10.0 + if weight == 700 { 2.0 } else { 0.0 },
            cap_height: size * 0.7,
        }
    })));
    let mut frame = AxisFrame::default();
    for (text, scale, bold, midpoint) in [
        ("price", 11.0 / 12.0, false, AxisTextMidpoint::Label),
        ("countdown", 10.0 / 12.0, true, AxisTextMidpoint::Label),
        ("time", 11.0 / 12.0, false, AxisTextMidpoint::StableTime),
    ] {
        frame.labels.push(AxisLabel {
            text: text.into(),
            x: 20.0,
            y: 30.0,
            color: Color::rgb(0, 0, 0),
            align: AxisTextAlign::Center,
            midpoint,
            font_scale: scale,
            bold,
            background: None,
            background_corners: AxisLabelCorners::NONE,
            measure_extra: 0.0,
            attach_group: None,
            border: None,
        });
    }
    let mut primitives = Vec::new();
    chart.build_axis_primitives_into(&frame, &mut primitives);
    let offsets: Vec<_> = primitives
        .iter()
        .filter_map(|primitive| match primitive {
            Prim::Text { text, y, size, .. }
                if matches!(text.as_str(), "price" | "countdown" | "time") =>
            {
                Some((text.as_str(), *y, *size))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        offsets,
        [
            ("price", 31.1, 11.0),
            ("countdown", 33.0, 10.0),
            ("time", 31.1, 11.0)
        ]
    );
}

#[test]
fn explicit_general_axes_reserve_layout_and_emit_shared_axis_frame() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    let mut x = GeneralAxisOptions::new("month", pane, AxisDimension::X, GeneralScaleType::Band);
    x.domain = GeneralAxisDomain::Category(vec!["Jan".into(), "Feb".into(), "Mar".into()]);
    x.title = Some("Month".into());
    chart.add_general_axis(x).unwrap();
    let mut y =
        GeneralAxisOptions::new("revenue", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y.domain = GeneralAxisDomain::Numeric([0.0, 100.0]);
    y.title = Some("Revenue".into());
    chart.add_general_axis(y).unwrap();

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let plot = chart.general_plot_rect(pane).unwrap();
    let pane_state = &chart.panes[pane];
    assert!(plot.y >= pane_state.top);
    assert!(plot.y + plot.height < pane_state.top + pane_state.height);
    // The strip hugs the widest tick ("100" at 7 px per glyph) plus the rotated title lane.
    assert_eq!(
        chart.left_axis_w,
        crate::axis_metrics::AxisMetrics::price_strip_width(21.0, 0.0)
            + chart.axis_metrics().axis
            + 8.0,
        "general Y labels reserve a side strip sized to their measured width"
    );

    let frame = chart.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    for expected in ["Jan", "Feb", "Mar", "Month", "0", "100"] {
        assert!(
            frame.labels.iter().any(|label| label.text == expected),
            "missing general-axis label {expected:?}"
        );
    }
    let revenue_title = frame
        .rotated_labels
        .iter()
        .find(|label| label.text == "Revenue")
        .expect("vertical general-axis title must be a rotated label");
    assert_eq!(revenue_title.align, AxisTextAlign::Center);
    assert_eq!(revenue_title.angle, -std::f64::consts::FRAC_PI_2);
    assert!(
        frame.bands.len() >= 2,
        "both general axes emit shared chrome"
    );
    let mut primitives = Vec::new();
    chart.build_axis_primitives_into(&frame, &mut primitives);
    assert!(
        primitives
            .iter()
            .any(|primitive| matches!(primitive, Prim::Text { text, .. } if text == "Jan"))
    );
    assert!(
        primitives.iter().any(
            |primitive| matches!(primitive, Prim::RotatedText { text, .. } if text == "Revenue")
        )
    );
    assert!(
        primitives
            .iter()
            .any(|primitive| matches!(primitive, Prim::Rect { .. }))
    );
}

#[test]
fn polar_axes_emit_shared_labels_spokes_and_rings() {
    let mut chart = ChartEngine::new(400.0, 400.0, 1.0);
    chart
        .apply_options(r#"{"grid":{"vertLines":{"visible":true},"horzLines":{"visible":true}}}"#)
        .unwrap();
    let pane = chart
        .add_pane_with_domain(true, HorizontalDomain::Polar)
        .unwrap();
    let mut angle = GeneralAxisOptions::new(
        "angle",
        pane,
        AxisDimension::Angle,
        GeneralScaleType::AngularCategory,
    );
    angle.domain = GeneralAxisDomain::Category(vec![
        "North".into(),
        "East".into(),
        "South".into(),
        "West".into(),
    ]);
    chart.add_general_axis(angle).unwrap();
    let mut radius = GeneralAxisOptions::new(
        "radius",
        pane,
        AxisDimension::Radius,
        GeneralScaleType::RadialLinear,
    );
    radius.domain = GeneralAxisDomain::Numeric([0.0, 100.0]);
    radius.tick_count = Some(3);
    chart.add_general_axis(radius).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

    let axis = chart.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    // The outer radius tick shares the top spoke with the first category label outside the
    // ring; the category wins the collision.
    assert!(!axis.labels.iter().any(|label| label.text == "100"));
    for expected in ["North", "East", "South", "West", "0", "50"] {
        assert!(
            axis.labels.iter().any(|label| label.text == expected),
            "missing {expected}; labels: {:?}",
            axis.labels
                .iter()
                .map(|label| label.text.as_str())
                .collect::<Vec<_>>()
        );
    }
    let mut grid = Vec::new();
    let mut points = Vec::new();
    chart.append_general_grid_frame(pane, 1.0, 1.0, &mut grid, &mut points);
    assert!(
        grid.iter()
            .any(|prim| matches!(prim, Prim::Polyline { point_count: 2, .. }))
    );
    assert!(
        grid.iter()
            .any(|prim| matches!(prim, Prim::Polyline { point_count, .. } if *point_count > 16))
    );
    assert!((points[0][0] - points[1][0]).abs() < 1.0e-4);
    assert!(points[1][1] < points[0][1]);
    let frame = chart.build_frame();
    let pane_frame = &frame.panes[pane];
    let polar_paths: Vec<_> = pane_frame
        .under
        .iter()
        .filter_map(|prim| match prim {
            Prim::Polyline {
                first_point,
                point_count,
                ..
            } => Some((*first_point as usize, *point_count as usize)),
            _ => None,
        })
        .collect();
    assert!(polar_paths.iter().any(|(_, count)| *count == 2));
    assert!(polar_paths.iter().any(|(_, count)| *count > 16));
    assert!(
        polar_paths
            .iter()
            .all(|(first, count)| first + count <= pane_frame.points.len())
    );

    let mut angle = GeneralAxisOptions::new(
        "angle",
        pane,
        AxisDimension::Angle,
        GeneralScaleType::AngularCategory,
    );
    angle.domain = GeneralAxisDomain::Category(vec![
        "North".into(),
        "East".into(),
        "South".into(),
        "West".into(),
    ]);
    angle.ticks = Some(vec![crate::GeneralAxisTick::Category {
        value: "North".into(),
        label: Some("Top".into()),
    }]);
    chart.update_general_axis_options(angle).unwrap();
    let mut radius = GeneralAxisOptions::new(
        "radius",
        pane,
        AxisDimension::Radius,
        GeneralScaleType::RadialLinear,
    );
    radius.domain = GeneralAxisDomain::Numeric([0.0, 100.0]);
    radius.reverse = true;
    radius.ticks = Some(vec![crate::GeneralAxisTick::Numeric {
        value: 50.0,
        label: Some("Half".into()),
    }]);
    chart.update_general_axis_options(radius).unwrap();
    let explicit = chart.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    assert!(explicit.labels.iter().any(|label| label.text == "Top"));
    assert!(explicit.labels.iter().any(|label| label.text == "Half"));
    assert!(!explicit.labels.iter().any(|label| label.text == "East"));
}

#[test]
fn radial_zero_ring_is_visible_without_grid_and_deduplicated_across_axes() {
    let mut chart = ChartEngine::new(400.0, 400.0, 1.0);
    chart
        .apply_options(r##"{"grid":{"horzLines":{"visible":false}},"rightPriceScale":{"borderColor":"#ff0000"}}"##)
        .unwrap();
    let pane = chart
        .add_pane_with_domain(true, HorizontalDomain::Polar)
        .unwrap();
    for id in ["radius-a", "radius-b"] {
        let mut radius = GeneralAxisOptions::new(
            id,
            pane,
            AxisDimension::Radius,
            GeneralScaleType::RadialLinear,
        );
        radius.domain = GeneralAxisDomain::Numeric([-1.0, 1.0]);
        radius.zero_line = true;
        chart.add_general_axis(radius).unwrap();
    }
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut prims = Vec::new();
    let mut points = Vec::new();
    chart.append_general_grid_frame(pane, 1.0, 1.0, &mut prims, &mut points);
    let rings: Vec<_> = prims
        .iter()
        .filter_map(|prim| match prim {
            Prim::Polyline {
                point_count, color, ..
            } if *point_count > 16 => Some(*color),
            _ => None,
        })
        .collect();
    assert_eq!(rings, [Color::rgb(255, 0, 0)]);

    chart
        .apply_options(r#"{"grid":{"horzLines":{"visible":true}}}"#)
        .unwrap();
    prims.clear();
    points.clear();
    chart.append_general_grid_frame(pane, 1.0, 1.0, &mut prims, &mut points);
    let plot = chart.general_plot_rect(pane).unwrap();
    let center_y = plot.y + plot.height / 2.0;
    let zero_radius = plot.width.min(plot.height) / 4.0;
    let coincident: Vec<_> = prims
        .iter()
        .filter_map(|prim| match prim {
            Prim::Polyline {
                first_point,
                point_count,
                color,
                ..
            } if *point_count > 16 => {
                let radius = center_y - f64::from(points[*first_point as usize][1]);
                ((radius - zero_radius).abs() < 0.5).then_some(*color)
            }
            _ => None,
        })
        .collect();
    assert_eq!(coincident, [Color::rgb(255, 0, 0)]);
}

#[test]
fn pie_and_donut_share_sector_frame_and_exact_hit_geometry() {
    let mut chart = ChartEngine::new(400.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(true, HorizontalDomain::Polar)
        .unwrap();
    let mut angle = GeneralAxisOptions::new(
        "pie-angle",
        pane,
        AxisDimension::Angle,
        GeneralScaleType::AngularCategory,
    );
    angle.visible = false;
    chart.add_general_axis(angle).unwrap();
    let mut radius = GeneralAxisOptions::new(
        "pie-radius",
        pane,
        AxisDimension::Radius,
        GeneralScaleType::RadialLinear,
    );
    radius.visible = false;
    chart.add_general_axis(radius).unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["A".into(), "B".into(), "C".into(), "D".into()],
            category_indices: vec![0, 1, 2, 3],
            y: vec![3.0, 1.0, 0.0, 9.0],
            y_valid: Some(vec![1, 1, 1, 0]),
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::pie(pane, dataset, "pie-angle", "pie-radius");
    options.polar.inner_radius = 0.4;
    let series = chart.add_general_series(options.clone()).unwrap();
    let mut invalid_options = options.clone();
    invalid_options.polar.outer_radius = 0.3;
    assert!(
        chart
            .update_general_series_options(series, invalid_options)
            .is_err()
    );
    assert_eq!(
        chart.general_series(series).unwrap().polar().inner_radius,
        0.4
    );
    let negative_dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["Bad".into()],
            category_indices: vec![0],
            y: vec![-1.0],
            y_valid: None,
        })
        .unwrap();
    assert!(
        chart
            .add_general_series(GeneralSeriesOptions::pie(
                pane,
                negative_dataset,
                "pie-angle",
                "pie-radius"
            ))
            .is_err()
    );
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut slices = Vec::new();
    chart.visit_general_pie_slices(chart.general_series(series).unwrap(), |slice| {
        slices.push(slice);
    });
    assert_eq!(
        slices.iter().map(|slice| slice.row).collect::<Vec<_>>(),
        [0, 1]
    );
    let frame = chart.build_frame();
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|prim| matches!(prim, Prim::MeshFill { .. }))
    );
    assert_eq!(
        frame.panes[pane]
            .main
            .iter()
            .filter(|prim| matches!(prim, Prim::MeshFill { .. }))
            .count(),
        2
    );
    for slice in &slices {
        let point = slice
            .sector
            .plot()
            .point(slice.sector.mid_angle(), slice.sector.mid_radius())
            .unwrap();
        let hit = chart
            .general_hit_test(pane, point.0, point.1, crate::GeneralHitMode::Exact)
            .unwrap();
        assert_eq!(hit.row, slice.row);
    }
    let (cx, cy) = slices[0].sector.plot().center();
    assert!(
        chart
            .general_hit_test(pane, cx, cy, crate::GeneralHitMode::Exact)
            .is_none()
    );
    let mut solid = options;
    solid.polar.inner_radius = 0.0;
    chart
        .update_general_series_options(series, solid.clone())
        .unwrap();
    let mut solid_slices = Vec::new();
    chart.visit_general_pie_slices(chart.general_series(series).unwrap(), |slice| {
        solid_slices.push(slice);
    });
    assert_eq!(solid_slices.len(), 2);
    assert!(solid_slices[0].sector.contains(cx, cy));

    for (start, end, inner, pad) in [
        (0.0, std::f64::consts::TAU, 0.0, 0.0),
        (0.35, 4.3, 0.35, 0.06),
        (5.3, 8.0, 0.2, 0.02),
    ] {
        solid.polar.start_angle = start;
        solid.polar.end_angle = end;
        solid.polar.inner_radius = inner;
        solid.polar.pad_angle = pad;
        chart
            .update_general_series_options(series, solid.clone())
            .unwrap();
        chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
        let mut slices = Vec::new();
        chart.visit_general_pie_slices(chart.general_series(series).unwrap(), |slice| {
            slices.push(slice)
        });
        let plot = slices[0].sector.plot();
        for angle_index in 0..180 {
            for radius_index in 1..20 {
                let (x, y) = plot
                    .point(
                        std::f64::consts::TAU * angle_index as f64 / 180.0,
                        radius_index as f64 / 20.0,
                    )
                    .unwrap();
                let expected = slices
                    .iter()
                    .find(|slice| slice.sector.contains(x, y))
                    .map(|slice| slice.row);
                let actual = chart
                    .general_hit_test(pane, x, y, crate::GeneralHitMode::Exact)
                    .map(|hit| hit.row);
                assert_eq!(
                    actual, expected,
                    "angle={angle_index} radius={radius_index} start={start}"
                );
            }
        }
    }

    let mut old_first = None;
    chart.visit_general_pie_slices(chart.general_series(series).unwrap(), |slice| {
        if slice.row == 0 {
            old_first = slice
                .sector
                .plot()
                .point(slice.sector.mid_angle(), slice.sector.mid_radius());
        }
    });
    let (x, y) = old_first.unwrap();
    assert_eq!(
        chart
            .general_hit_test(pane, x, y, crate::GeneralHitMode::Exact)
            .unwrap()
            .row,
        0
    );
    chart
        .replace_general_xy_dataset(
            dataset,
            GeneralXyInput::Category {
                ids: None,
                categories: vec!["A".into(), "B".into(), "C".into(), "D".into()],
                category_indices: vec![0, 1, 2, 3],
                y: vec![0.0, 1.0, 0.0, 9.0],
                y_valid: Some(vec![1, 1, 1, 0]),
            },
        )
        .unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    assert_eq!(
        chart
            .general_hit_test(pane, x, y, crate::GeneralHitMode::Exact)
            .unwrap()
            .row,
        1
    );
    let bytes = chart.memory_usage().general_series_capacity_bytes;
    for _ in 0..1_000 {
        let _ = chart.general_hit_test(pane, x, y, crate::GeneralHitMode::Exact);
    }
    assert_eq!(chart.memory_usage().general_series_capacity_bytes, bytes);

    let extremes = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["Max A".into(), "Max B".into()],
            category_indices: vec![0, 1],
            y: vec![f64::MAX, f64::MAX],
            y_valid: None,
        })
        .unwrap();
    let extreme_series = chart
        .add_general_series(GeneralSeriesOptions::pie(
            pane,
            extremes,
            "pie-angle",
            "pie-radius",
        ))
        .unwrap();
    let mut extreme_slices = Vec::new();
    chart.visit_general_pie_slices(chart.general_series(extreme_series).unwrap(), |slice| {
        extreme_slices.push(slice);
    });
    assert_eq!(extreme_slices.len(), 2);
    assert!((extreme_slices[0].sector.mid_angle() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);

    let zeros = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["Zero".into(), "Missing".into()],
            category_indices: vec![0, 1],
            y: vec![0.0, 1.0],
            y_valid: Some(vec![1, 0]),
        })
        .unwrap();
    let zero_series = chart
        .add_general_series(GeneralSeriesOptions::pie(
            pane,
            zeros,
            "pie-angle",
            "pie-radius",
        ))
        .unwrap();
    chart.visit_general_pie_slices(chart.general_series(zero_series).unwrap(), |_| {
        panic!("zero and missing rows have no visible sector")
    });
}

#[test]
fn polar_legend_slices_share_frame_colors_and_bounded_row_labels() {
    let mut chart =
        ChartEngine::new_with_initial_domain(400.0, 400.0, 1.0, HorizontalDomain::Polar).unwrap();
    let mut angle = GeneralAxisOptions::new(
        "angle",
        0,
        AxisDimension::Angle,
        GeneralScaleType::AngularCategory,
    );
    angle.visible = false;
    chart.add_general_axis(angle).unwrap();
    let mut radius = GeneralAxisOptions::new(
        "radius",
        0,
        AxisDimension::Radius,
        GeneralScaleType::RadialLinear,
    );
    radius.visible = false;
    chart.add_general_axis(radius).unwrap();
    let input = GeneralXyInput::Category {
        ids: None,
        categories: (0..35).map(|row| format!("Category {row}")).collect(),
        category_indices: (0..35).collect(),
        y: vec![1.0; 35],
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input.clone()).unwrap();
    let mut labels = vec![None; 35];
    labels[0] = Some("Named first".into());
    let mut colors = vec![None; 35];
    colors[1] = Some("#f00".into());
    chart
        .replace_general_xy_dataset_styled(dataset, input, Some(labels), Some(colors), None)
        .unwrap();
    let mut options = GeneralSeriesOptions::pie(0, dataset, "angle", "radius");
    options.title = "Allocation".into();
    let series = chart.add_general_series(options.clone()).unwrap();
    let snapshot = chart.general_legend_snapshot(Some(0));
    let item = &snapshot.items[0];
    assert_eq!(item.slices.len(), 32);
    assert!(item.slices_truncated);
    assert_eq!(item.slices[0].label, "Named first");
    assert_eq!(item.slices[0].color, "#4d8dd2");
    assert_eq!(item.slices[1].label, "Category 1");
    assert_eq!(item.slices[1].color, "#ff0000");
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let frame = chart.build_frame();
    let first_fill = frame.panes[0]
        .main
        .iter()
        .find_map(|prim| match prim {
            Prim::MeshFill { color, .. } => Some(*color),
            _ => None,
        })
        .unwrap();
    assert_eq!(Color::parse_css(&item.slices[0].color), Some(first_fill));
    chart.set_general_series_visible(series, false);
    assert_eq!(
        chart.general_legend_snapshot(Some(0)).items[0].slices.len(),
        32
    );
    for _ in 0..16 {
        chart.add_general_series(options.clone()).unwrap();
    }
    let all = chart.general_legend_snapshot(Some(0));
    assert_eq!(
        all.items
            .iter()
            .map(|item| item.slices.len())
            .sum::<usize>(),
        512
    );
    assert!(all.items[16].slices.is_empty());
    assert!(all.items[16].slices_truncated);
}

#[test]
fn polar_data_labels_emit_collision_free_shared_frame_leaders() {
    let mut chart =
        ChartEngine::new_with_initial_domain(400.0, 400.0, 1.0, HorizontalDomain::Polar).unwrap();
    let mut angle = GeneralAxisOptions::new(
        "angle",
        0,
        AxisDimension::Angle,
        GeneralScaleType::AngularCategory,
    );
    angle.visible = false;
    chart.add_general_axis(angle).unwrap();
    let mut radius = GeneralAxisOptions::new(
        "radius",
        0,
        AxisDimension::Radius,
        GeneralScaleType::RadialLinear,
    );
    radius.visible = false;
    chart.add_general_axis(radius).unwrap();
    let input = GeneralXyInput::Category {
        ids: None,
        categories: (0..32).map(|row| format!("Sector {row}")).collect(),
        category_indices: (0..32).collect(),
        y: vec![1.0; 32],
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input).unwrap();
    let mut options = GeneralSeriesOptions::pie(0, dataset, "angle", "radius");
    options.polar.outer_radius = 0.65;
    options.data_labels = true;
    chart.add_general_series(options).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let frame = chart.build_frame();
    let pane = &frame.panes[0];
    let labels = pane
        .main
        .iter()
        .filter_map(|prim| match prim {
            Prim::Text {
                x, y, text, size, ..
            } => Some((f64::from(*x), f64::from(*y), text, f64::from(*size))),
            _ => None,
        })
        .collect::<Vec<_>>();
    let leaders = pane
        .main
        .iter()
        .filter_map(|prim| match prim {
            Prim::Polyline {
                first_point,
                point_count: 3,
                ..
            } => Some(*first_point as usize),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!labels.is_empty());
    assert!(
        labels
            .iter()
            .all(|(_, _, text, _)| text.starts_with("Sector "))
    );
    assert!(
        labels.len() < 32,
        "dense sectors should suppress colliding labels"
    );
    assert_eq!(leaders.len(), labels.len());
    let plot = chart.general_plot_rect(0).unwrap();
    let bounds = labels
        .iter()
        .map(|(x, y, text, size)| {
            let width = text.len() as f64 * 7.0;
            [
                x - width / 2.0,
                y - size * 0.55,
                x + width / 2.0,
                y + size * 0.55,
            ]
        })
        .collect::<Vec<_>>();
    for (index, rect) in bounds.iter().enumerate() {
        assert!(rect[0] >= plot.x + 2.0 && rect[2] <= plot.x + plot.width - 2.0);
        assert!(rect[1] >= plot.y + 2.0 && rect[3] <= plot.y + plot.height - 2.0);
        for other in &bounds[..index] {
            assert!(
                rect[0] >= other[2] + 2.0
                    || rect[2] <= other[0] - 2.0
                    || rect[1] >= other[3] + 2.0
                    || rect[3] <= other[1] - 2.0
            );
        }
    }
    for first in leaders {
        let segment = &pane.points[first..first + 3];
        assert!(
            segment
                .iter()
                .all(|point| point.iter().all(|value| value.is_finite()))
        );
        assert_ne!(segment[0], segment[2]);
    }
}

#[test]
fn pie_value_transition_uses_fixed_clock_for_frame_hits_and_interruption() {
    let mut chart =
        ChartEngine::new_with_initial_domain(400.0, 400.0, 1.0, HorizontalDomain::Polar).unwrap();
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
        chart.add_general_axis(axis).unwrap();
    }
    let input = |a, b| GeneralXyInput::Category {
        ids: Some(vec![
            crate::GeneralRowId::Text("a".into()),
            crate::GeneralRowId::Text("b".into()),
        ]),
        categories: vec!["A".into(), "B".into()],
        category_indices: vec![0, 1],
        y: vec![a, b],
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input(1.0, 3.0)).unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::pie(0, dataset, "angle", "radius"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    assert!(chart.set_general_animation_time_ms(1_000.0));
    chart
        .replace_general_xy_dataset(dataset, input(3.0, 1.0))
        .unwrap();
    assert_eq!(chart.general_dataset(dataset).unwrap().y(), &[3.0, 1.0]);
    assert!(chart.general_transition_active());
    let first_sweep = |chart: &ChartEngine| {
        let mut sweep = None;
        chart.visit_general_pie_slices(chart.general_series(series).unwrap(), |item| {
            if item.row == 0 {
                sweep = Some(item.sector.mid_angle() * 2.0);
            }
        });
        sweep.unwrap()
    };
    assert!((first_sweep(&chart) - std::f64::consts::FRAC_PI_2).abs() < 1e-10);
    assert!(chart.set_general_animation_time_ms(1_100.0));
    assert!((first_sweep(&chart) - std::f64::consts::PI).abs() < 1e-10);
    let mut first = None;
    chart.visit_general_pie_slices(chart.general_series(series).unwrap(), |item| {
        if item.row == 0 {
            first = Some(item);
        }
    });
    let first = first.unwrap();
    let (x, y) = first
        .sector
        .plot()
        .point(first.sector.mid_angle(), first.sector.mid_radius())
        .unwrap();
    assert_eq!(
        chart
            .general_hit_test(0, x, y, crate::GeneralHitMode::Exact)
            .unwrap()
            .row,
        0
    );
    let frame = chart.build_frame();
    assert_eq!(
        frame.panes[0]
            .main
            .iter()
            .filter(|prim| matches!(prim, Prim::MeshFill { .. }))
            .count(),
        2
    );
    chart
        .replace_general_xy_dataset(dataset, input(1.0, 3.0))
        .unwrap();
    assert!((first_sweep(&chart) - std::f64::consts::PI).abs() < 1e-10);
    assert!(!chart.set_general_animation_time_ms(1_050.0));
    assert!(chart.set_general_animation_time_ms(1_200.0));
    assert!((first_sweep(&chart) - 0.75 * std::f64::consts::PI).abs() < 1e-10);
    chart.set_general_transition_reduced_motion(true);
    assert!(!chart.general_transition_active());
    assert!((first_sweep(&chart) - std::f64::consts::FRAC_PI_2).abs() < 1e-10);
    chart.set_general_transition_reduced_motion(false);
    chart
        .replace_general_xy_dataset(dataset, input(3.0, 1.0))
        .unwrap();
    assert!(chart.general_transition_active());
    assert!(chart.remove_general_series(series));
    assert!(!chart.general_transition_active());
}

#[test]
fn removed_pie_row_shrinks_in_source_order_without_a_ghost_hit() {
    let mut chart =
        ChartEngine::new_with_initial_domain(400.0, 400.0, 1.0, HorizontalDomain::Polar).unwrap();
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
        chart.add_general_axis(axis).unwrap();
    }
    let input = |keep_b: bool| GeneralXyInput::Category {
        ids: Some(
            (if keep_b {
                vec!["a", "b", "c"]
            } else {
                vec!["a", "c"]
            })
            .into_iter()
            .map(|id| GeneralRowId::Text(id.into()))
            .collect(),
        ),
        categories: (if keep_b {
            vec!["A", "B", "C"]
        } else {
            vec!["A", "C"]
        })
        .into_iter()
        .map(str::to_owned)
        .collect(),
        category_indices: if keep_b { vec![0, 1, 2] } else { vec![0, 1] },
        y: if keep_b { vec![2.0; 3] } else { vec![2.0; 2] },
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input(true)).unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::pie(0, dataset, "angle", "radius"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(false))
        .unwrap();
    assert!(chart.general_transition_active());
    assert_eq!(chart.general_dataset(dataset).unwrap().len(), 2);
    let mut slices = Vec::new();
    chart.visit_general_pie_slices(chart.general_series(series).unwrap(), |slice| {
        slices.push(slice)
    });
    assert_eq!(slices.len(), 3);
    assert_eq!(
        chart.build_frame().panes[0]
            .main
            .iter()
            .filter(|prim| matches!(prim, Prim::MeshFill { .. }))
            .count(),
        3
    );
    let presentation = chart.general_geometry_dataset(dataset).unwrap();
    assert_eq!(
        slices
            .iter()
            .map(|slice| presentation.row_identity(slice.row).unwrap().clone())
            .collect::<Vec<_>>(),
        ["a", "b", "c"]
            .map(|id| GeneralRowIdentity::Explicit(GeneralRowId::Text(id.into())))
            .to_vec()
    );
    chart.set_general_animation_time_ms(1_100.0);
    slices.clear();
    chart.visit_general_pie_slices(chart.general_series(series).unwrap(), |slice| {
        slices.push(slice)
    });
    let ghost = slices.iter().find(|slice| slice.row == 1).unwrap();
    let (x, y) = ghost
        .sector
        .plot()
        .point(ghost.sector.mid_angle(), ghost.sector.mid_radius())
        .unwrap();
    assert!(
        chart
            .general_hit_test(0, x, y, crate::GeneralHitMode::Exact)
            .is_none()
    );
    chart
        .replace_general_xy_dataset(dataset, input(true))
        .unwrap();
    assert!(chart.general_transition_active());
    assert!((chart.general_geometry_dataset(dataset).unwrap().y()[1] - 1.0).abs() < 1e-10);
    slices.clear();
    chart.visit_general_pie_slices(chart.general_series(series).unwrap(), |slice| {
        slices.push(slice)
    });
    let restored = slices.iter().find(|slice| slice.row == 1).unwrap();
    let (x, y) = restored
        .sector
        .plot()
        .point(restored.sector.mid_angle(), restored.sector.mid_radius())
        .unwrap();
    assert_eq!(
        chart
            .general_hit_test(0, x, y, crate::GeneralHitMode::Exact)
            .unwrap()
            .row,
        1
    );
    chart.set_general_animation_time_ms(1_300.0);
    assert!(!chart.general_transition_active());
    chart
        .replace_general_xy_dataset(dataset, input(false))
        .unwrap();
    chart.set_general_animation_time_ms(1_500.0);
    assert!(!chart.general_transition_active());
    slices.clear();
    chart.visit_general_pie_slices(chart.general_series(series).unwrap(), |slice| {
        slices.push(slice)
    });
    assert_eq!(slices.len(), 2);
    chart
        .replace_general_xy_dataset(
            dataset,
            GeneralXyInput::Category {
                ids: Some(Vec::new()),
                categories: Vec::new(),
                category_indices: Vec::new(),
                y: Vec::new(),
                y_valid: None,
            },
        )
        .unwrap();
    assert!(chart.general_transition_active());
    slices.clear();
    chart.visit_general_pie_slices(chart.general_series(series).unwrap(), |slice| {
        slices.push(slice)
    });
    assert_eq!(slices.len(), 2);
    chart.set_general_animation_time_ms(1_700.0);
    assert!(!chart.general_transition_active());
    slices.clear();
    chart.visit_general_pie_slices(chart.general_series(series).unwrap(), |slice| {
        slices.push(slice)
    });
    assert!(slices.is_empty());
    assert!(
        !chart.build_frame().panes[0]
            .main
            .iter()
            .any(|prim| matches!(prim, Prim::MeshFill { .. }))
    );
}

#[test]
fn removed_column_keeps_its_category_slot_until_exit_completes() {
    let mut chart = ChartEngine::new_with_initial_domain(
        400.0,
        300.0,
        1.0,
        HorizontalDomain::Category {
            scale: CategoryScaleType::Band,
        },
    )
    .unwrap();
    let mut x_axis = GeneralAxisOptions::new("x", 0, AxisDimension::X, GeneralScaleType::Band);
    x_axis.visible = false;
    chart.add_general_axis(x_axis).unwrap();
    let mut y_axis = GeneralAxisOptions::new("y", 0, AxisDimension::Y, GeneralScaleType::Linear);
    y_axis.visible = false;
    y_axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
    chart.add_general_axis(y_axis).unwrap();
    let input = |keep_b: bool| GeneralXyInput::Category {
        ids: Some(
            (if keep_b {
                vec!["a", "b", "c"]
            } else {
                vec!["a", "c"]
            })
            .into_iter()
            .map(|id| GeneralRowId::Text(id.into()))
            .collect(),
        ),
        categories: (if keep_b {
            vec!["A", "B", "C"]
        } else {
            vec!["A", "C"]
        })
        .into_iter()
        .map(str::to_owned)
        .collect(),
        category_indices: if keep_b { vec![0, 1, 2] } else { vec![0, 1] },
        y: if keep_b {
            vec![2.0, 4.0, 6.0]
        } else {
            vec![2.0, 6.0]
        },
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input(true)).unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::column(0, dataset, "x", "y"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(false))
        .unwrap();
    assert!(chart.general_transition_active());
    chart.set_general_animation_time_ms(1_100.0);
    assert_eq!(
        chart.general_axis_effective_domain("x"),
        Some(GeneralAxisDomain::Category(vec![
            "A".into(),
            "B".into(),
            "C".into()
        ]))
    );
    let mut columns = Vec::new();
    chart.visit_general_columns(chart.general_series(series).unwrap(), |column| {
        columns.push(column)
    });
    assert_eq!(columns.len(), 3);
    let ghost = columns[1];
    let x = (ghost.left + ghost.right) / 2.0;
    let y = (ghost.top + ghost.bottom) / 2.0;
    assert!(
        chart
            .general_hit_test(0, x, y, crate::GeneralHitMode::Exact)
            .is_none()
    );
    chart.set_general_animation_time_ms(1_200.0);
    assert!(!chart.general_transition_active());
    columns.clear();
    chart.visit_general_columns(chart.general_series(series).unwrap(), |column| {
        columns.push(column)
    });
    assert_eq!(columns.len(), 2);
    assert_eq!(
        chart.general_axis_effective_domain("x"),
        Some(GeneralAxisDomain::Category(vec!["A".into(), "C".into()]))
    );
}

#[test]
fn category_reorder_moves_shared_band_geometry_ticks_and_hits() {
    let mut chart = ChartEngine::new(400.0, 300.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .unwrap();
    let mut y_axis = GeneralAxisOptions::new("y", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y_axis.visible = false;
    y_axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
    chart.add_general_axis(y_axis).unwrap();
    let input = |categories: [&str; 3], indices: Vec<u32>| GeneralXyInput::Category {
        ids: Some(
            ["a", "b", "c"]
                .into_iter()
                .map(|id| GeneralRowId::Text(id.into()))
                .collect(),
        ),
        categories: categories.into_iter().map(str::to_owned).collect(),
        category_indices: indices,
        y: vec![2.0, 4.0, 6.0],
        y_valid: None,
    };
    let dataset = chart
        .create_general_xy_dataset(input(["A", "B", "C"], vec![0, 1, 2]))
        .unwrap();
    let mut line_options = GeneralSeriesOptions::xy_line(pane, dataset, "x", "y");
    line_options.point_markers = true;
    let line = chart.add_general_series(line_options).unwrap();
    let column = chart
        .add_general_series(GeneralSeriesOptions::column(pane, dataset, "x", "y"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    let positions = |chart: &ChartEngine| {
        let mut line_x = None;
        chart.visit_general_path_points(chart.general_series(line).unwrap(), |point| {
            if point.row == 0 {
                line_x = Some(point.x);
            }
        });
        let mut column_x = None;
        chart.visit_general_columns(chart.general_series(column).unwrap(), |bar| {
            if bar.row == 0 {
                column_x = Some((bar.left + bar.right) / 2.0);
            }
        });
        let mut axis = AxisFrame::default();
        chart.append_general_axis_frame(&mut axis, &|_, _| 14.0);
        let tick_x = axis
            .labels
            .iter()
            .find(|label| label.text == "A")
            .map(|label| label.x);
        (line_x.unwrap(), column_x.unwrap(), tick_x.unwrap())
    };
    let source = positions(&chart);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(["C", "A", "B"], vec![1, 2, 0]))
        .unwrap();
    assert!(chart.general_transition_active());
    let start = positions(&chart);
    for (before, initial) in [
        (source.0, start.0),
        (source.1, start.1),
        (source.2, start.2),
    ] {
        assert!((before - initial).abs() < 1e-8);
    }
    chart.set_general_animation_time_ms(1_100.0);
    let middle = positions(&chart);
    let mut bar_y = None;
    chart.visit_general_columns(chart.general_series(column).unwrap(), |bar| {
        if bar.row == 0 {
            bar_y = Some((bar.top + bar.bottom) / 2.0);
        }
    });
    assert_eq!(
        chart
            .general_hit_test(pane, middle.1, bar_y.unwrap(), crate::GeneralHitMode::Exact)
            .unwrap()
            .row,
        0
    );
    chart.set_general_animation_time_ms(1_200.0);
    let end = positions(&chart);
    for (before, half, after) in [
        (source.0, middle.0, end.0),
        (source.1, middle.1, end.1),
        (source.2, middle.2, end.2),
    ] {
        assert!((half - (before + after) / 2.0).abs() < 1e-8);
    }
    assert!(end.0 > source.0);
    assert!(!chart.general_transition_active());
}

#[test]
fn category_point_reorder_interrupts_from_its_sampled_position() {
    let mut chart = ChartEngine::new(400.0, 300.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Point,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Point,
        ))
        .unwrap();
    let mut y_axis = GeneralAxisOptions::new("y", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y_axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
    y_axis.visible = false;
    chart.add_general_axis(y_axis).unwrap();
    let input = |categories: [&str; 3], indices: [u32; 3]| GeneralXyInput::Category {
        ids: Some(
            ["a", "b", "c"]
                .into_iter()
                .map(|id| GeneralRowId::Text(id.into()))
                .collect(),
        ),
        categories: categories.into_iter().map(str::to_owned).collect(),
        category_indices: indices.to_vec(),
        y: vec![2.0, 4.0, 6.0],
        y_valid: None,
    };
    let dataset = chart
        .create_general_xy_dataset(input(["A", "B", "C"], [0, 1, 2]))
        .unwrap();
    let line = chart
        .add_general_series(GeneralSeriesOptions::xy_line(pane, dataset, "x", "y"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    let first_x = |chart: &ChartEngine| {
        let mut x = None;
        chart.visit_general_path_points(chart.general_series(line).unwrap(), |point| {
            if point.row == 0 {
                x = Some(point.x);
            }
        });
        x.unwrap()
    };
    let original = first_x(&chart);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(["C", "A", "B"], [1, 2, 0]))
        .unwrap();
    chart.set_general_animation_time_ms(1_100.0);
    let sampled = first_x(&chart);
    assert!(sampled > original);
    chart
        .replace_general_xy_dataset(dataset, input(["B", "C", "A"], [2, 0, 1]))
        .unwrap();
    assert!((first_x(&chart) - sampled).abs() < 1e-8);
    chart.set_general_animation_time_ms(1_200.0);
    let midway = first_x(&chart);
    chart.set_general_animation_time_ms(1_300.0);
    let target = first_x(&chart);
    assert!((midway - (sampled + target) / 2.0).abs() < 1e-8);
    assert!(!chart.general_transition_active());
}

#[test]
fn single_category_rename_with_fixed_geometry_schedules_no_animation() {
    let mut chart = ChartEngine::new(400.0, 300.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Point,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Point,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let input = |category: &str| GeneralXyInput::Category {
        ids: Some(vec![GeneralRowId::Text("same".into())]),
        categories: vec![category.into()],
        category_indices: vec![0],
        y: vec![5.0],
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input("A")).unwrap();
    chart
        .add_general_series(GeneralSeriesOptions::xy_line(pane, dataset, "x", "y"))
        .unwrap();
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input("B"))
        .unwrap();
    assert!(!chart.general_transition_active());
}

#[test]
fn matched_row_moves_between_categories_on_a_fixed_axis() {
    let mut chart = ChartEngine::new(400.0, 300.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    let mut x_axis = GeneralAxisOptions::new("x", pane, AxisDimension::X, GeneralScaleType::Band);
    x_axis.domain = GeneralAxisDomain::Category(vec!["A".into(), "B".into(), "C".into()]);
    chart.add_general_axis(x_axis).unwrap();
    let mut y_axis = GeneralAxisOptions::new("y", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y_axis.visible = false;
    y_axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
    chart.add_general_axis(y_axis).unwrap();
    let input = |category_index| GeneralXyInput::Category {
        ids: Some(vec![GeneralRowId::Text("moving".into())]),
        categories: vec!["A".into(), "B".into(), "C".into()],
        category_indices: vec![category_index],
        y: vec![5.0],
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input(0)).unwrap();
    let line = chart
        .add_general_series(GeneralSeriesOptions::xy_line(pane, dataset, "x", "y"))
        .unwrap();
    let column = chart
        .add_general_series(GeneralSeriesOptions::column(pane, dataset, "x", "y"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    let positions = |chart: &ChartEngine| {
        let mut line_x = None;
        chart.visit_general_path_points(chart.general_series(line).unwrap(), |point| {
            line_x = Some(point.x)
        });
        let mut column_x = None;
        chart.visit_general_columns(chart.general_series(column).unwrap(), |bar| {
            column_x = Some((bar.left + bar.right) / 2.0)
        });
        (line_x.unwrap(), column_x.unwrap())
    };
    let source = positions(&chart);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart.replace_general_xy_dataset(dataset, input(2)).unwrap();
    assert!(chart.general_transition_active());
    let initial = positions(&chart);
    assert!((initial.0 - source.0).abs() < 1e-8);
    assert!((initial.1 - source.1).abs() < 1e-8);
    chart.set_general_animation_time_ms(1_100.0);
    let middle = positions(&chart);
    chart.set_general_animation_time_ms(1_200.0);
    let target = positions(&chart);
    for (from, half, to) in [
        (source.0, middle.0, target.0),
        (source.1, middle.1, target.1),
    ] {
        assert!((half - (from + to) / 2.0).abs() < 1e-8);
    }
    assert!(!chart.general_transition_active());
    chart.set_general_animation_time_ms(2_000.0);
    chart.replace_general_xy_dataset(dataset, input(0)).unwrap();
    chart.set_general_animation_time_ms(2_100.0);
    let sampled = positions(&chart);
    chart.replace_general_xy_dataset(dataset, input(1)).unwrap();
    let restarted = positions(&chart);
    assert!((restarted.0 - sampled.0).abs() < 1e-8);
    assert!((restarted.1 - sampled.1).abs() < 1e-8);
    chart.set_general_animation_time_ms(2_200.0);
    let interrupted_middle = positions(&chart);
    let mut bar_y = None;
    chart.visit_general_columns(chart.general_series(column).unwrap(), |bar| {
        bar_y = Some((bar.top + bar.bottom) / 2.0)
    });
    assert_eq!(
        chart
            .general_hit_test(
                pane,
                interrupted_middle.1,
                bar_y.unwrap(),
                crate::GeneralHitMode::Exact,
            )
            .unwrap()
            .row,
        0
    );
    chart.set_general_animation_time_ms(2_300.0);
    let interrupted_target = positions(&chart);
    for (from, half, to) in [
        (sampled.0, interrupted_middle.0, interrupted_target.0),
        (sampled.1, interrupted_middle.1, interrupted_target.1),
    ] {
        assert!((half - (from + to) / 2.0).abs() < 1e-8);
    }
    assert!(!chart.general_transition_active());
}

#[test]
fn matched_horizontal_bar_moves_between_y_categories() {
    let mut chart = ChartEngine::new(480.0, 360.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    let mut x_axis = GeneralAxisOptions::new("x", pane, AxisDimension::X, GeneralScaleType::Linear);
    x_axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
    chart.add_general_axis(x_axis).unwrap();
    let mut y_axis = GeneralAxisOptions::new("y", pane, AxisDimension::Y, GeneralScaleType::Band);
    y_axis.domain = GeneralAxisDomain::Category(vec!["A".into(), "B".into(), "C".into()]);
    chart.add_general_axis(y_axis).unwrap();
    let input = |index| GeneralXyInput::Category {
        ids: Some(vec![GeneralRowId::Text("bar".into())]),
        categories: vec!["A".into(), "B".into(), "C".into()],
        category_indices: vec![index],
        y: vec![5.0],
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input(0)).unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::horizontal_bar(
            pane, dataset, "x", "y",
        ))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    let center = |chart: &ChartEngine| {
        let mut point = None;
        chart.visit_general_horizontal_bars(chart.general_series(series).unwrap(), |bar| {
            point = Some(((bar.left + bar.right) / 2.0, (bar.top + bar.bottom) / 2.0));
        });
        point.unwrap()
    };
    let source = center(&chart);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart.replace_general_xy_dataset(dataset, input(2)).unwrap();
    let initial = center(&chart);
    assert!((initial.1 - source.1).abs() < 1e-8);
    chart.set_general_animation_time_ms(1_100.0);
    let middle = center(&chart);
    assert_eq!(
        chart
            .general_hit_test(pane, middle.0, middle.1, crate::GeneralHitMode::Exact)
            .map(|hit| (hit.series, hit.row)),
        Some((series, 0))
    );
    chart.set_general_animation_time_ms(1_200.0);
    let target = center(&chart);
    assert!((middle.1 - (source.1 + target.1) / 2.0).abs() < 1e-8);
    assert!(!chart.general_transition_active());
}

#[test]
fn matched_heatmap_cell_moves_on_both_fixed_category_axes() {
    let mut chart = ChartEngine::new(480.0, 360.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
        let mut axis = GeneralAxisOptions::new(id, pane, dimension, GeneralScaleType::Band);
        axis.domain = GeneralAxisDomain::Category(vec!["A".into(), "B".into(), "C".into()]);
        chart.add_general_axis(axis).unwrap();
    }
    let input = |index| GeneralXyInput::HeatmapCategoryCategory {
        ids: Some(vec![GeneralRowId::Text("cell".into())]),
        x_categories: vec!["A".into(), "B".into(), "C".into()],
        x_category_indices: vec![index],
        y_categories: vec!["A".into(), "B".into(), "C".into()],
        y_category_indices: vec![index],
        value: vec![5.0],
        value_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input(0)).unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::heatmap_grid(pane, dataset, "x", "y"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    let cell_center = |chart: &ChartEngine| {
        let mut center = None;
        chart.visit_general_heatmap_cells(chart.general_series(series).unwrap(), |cell| {
            center = Some((
                (cell.left + cell.right) / 2.0,
                (cell.top + cell.bottom) / 2.0,
            ));
        });
        center.unwrap()
    };
    let start = cell_center(&chart);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart.replace_general_xy_dataset(dataset, input(2)).unwrap();
    assert!(chart.general_transition_active());
    let initial = cell_center(&chart);
    assert!((initial.0 - start.0).abs() < 1e-8);
    assert!((initial.1 - start.1).abs() < 1e-8);
    chart.set_general_animation_time_ms(1_100.0);
    let middle = cell_center(&chart);
    chart.set_general_animation_time_ms(1_200.0);
    let end = cell_center(&chart);
    for (from, half, to) in [(start.0, middle.0, end.0), (start.1, middle.1, end.1)] {
        assert!((half - (from + to) / 2.0).abs() < 1e-8);
    }
    chart.set_general_animation_time_ms(2_000.0);
    chart.replace_general_xy_dataset(dataset, input(0)).unwrap();
    chart.set_general_animation_time_ms(2_100.0);
    let sampled = cell_center(&chart);
    chart.replace_general_xy_dataset(dataset, input(1)).unwrap();
    let restarted = cell_center(&chart);
    assert!((restarted.0 - sampled.0).abs() < 1e-8);
    assert!((restarted.1 - sampled.1).abs() < 1e-8);
    chart.set_general_animation_time_ms(2_200.0);
    let interrupted_middle = cell_center(&chart);
    assert_eq!(
        chart
            .general_hit_test(
                pane,
                interrupted_middle.0,
                interrupted_middle.1,
                crate::GeneralHitMode::Exact,
            )
            .map(|hit| (hit.series, hit.row)),
        Some((series, 0))
    );
}

#[test]
fn automatic_category_axis_retains_the_departing_slot_during_row_motion() {
    let mut chart = ChartEngine::new(400.0, 300.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .unwrap();
    let mut y_axis = GeneralAxisOptions::new("y", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y_axis.visible = false;
    y_axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
    chart.add_general_axis(y_axis).unwrap();
    let input = |categories: [&str; 2], indices: [u32; 2]| GeneralXyInput::Category {
        ids: Some(
            ["a", "b"]
                .into_iter()
                .map(|id| GeneralRowId::Text(id.into()))
                .collect(),
        ),
        categories: categories.into_iter().map(str::to_owned).collect(),
        category_indices: indices.to_vec(),
        y: vec![2.0, 4.0],
        y_valid: None,
    };
    let dataset = chart
        .create_general_xy_dataset(input(["A", "B"], [0, 1]))
        .unwrap();
    let column = chart
        .add_general_series(GeneralSeriesOptions::column(pane, dataset, "x", "y"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    let first_x = |chart: &ChartEngine| {
        let mut x = None;
        chart.visit_general_columns(chart.general_series(column).unwrap(), |bar| {
            if bar.row == 0 {
                x = Some((bar.left + bar.right) / 2.0);
            }
        });
        x.unwrap()
    };
    let start = first_x(&chart);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(["B", "C"], [1, 0]))
        .unwrap();
    assert!(chart.general_transition_active());
    assert_eq!(
        chart.general_axis_effective_domain("x"),
        Some(GeneralAxisDomain::Category(vec![
            "A".into(),
            "B".into(),
            "C".into()
        ]))
    );
    assert!((first_x(&chart) - start).abs() < 1e-8);
    chart.set_general_animation_time_ms(1_200.0);
    assert_eq!(
        chart.general_axis_effective_domain("x"),
        Some(GeneralAxisDomain::Category(vec!["B".into(), "C".into()]))
    );
    assert!(first_x(&chart) > start);
}

#[test]
fn cartesian_value_transition_moves_shared_geometry_without_changing_public_data() {
    let mut chart = ChartEngine::new_with_initial_domain(
        400.0,
        300.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
        let mut axis = GeneralAxisOptions::new(id, 0, dimension, GeneralScaleType::Linear);
        axis.visible = false;
        axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        chart.add_general_axis(axis).unwrap();
    }
    let input = |x, y| GeneralXyInput::Numeric {
        ids: Some(vec![crate::GeneralRowId::Text("one".into())]),
        x: vec![x],
        y: vec![y],
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input(2.0, 2.0)).unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::xy_line(0, dataset, "x", "y"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(500.0);
    chart
        .replace_general_xy_dataset(dataset, input(6.0, 6.0))
        .unwrap();
    assert_eq!(chart.general_dataset(dataset).unwrap().y(), &[6.0]);
    chart.set_general_animation_time_ms(600.0);
    let mut midpoint = None;
    chart.visit_general_path_points(chart.general_series(series).unwrap(), |point| {
        midpoint = Some(point);
    });
    let point = midpoint.unwrap();
    let plot = chart.general_plot_rect(0).unwrap();
    let x = plot.width * 0.4;
    let y = plot.y + plot.height * 0.6;
    assert!((point.x - x).abs() < 1e-8);
    assert!((point.y - y).abs() < 1e-8);
    chart.set_general_animation_time_ms(700.0);
    assert!(!chart.general_transition_active());
}

#[test]
fn cartesian_area_families_replay_their_shared_fill_on_the_fixed_clock() {
    for range in [false, true] {
        let mut chart = ChartEngine::new_with_initial_domain(
            400.0,
            300.0,
            1.0,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
        for (id, dimension, domain) in [
            ("x", AxisDimension::X, [0.0, 4.0]),
            ("y", AxisDimension::Y, [0.0, 10.0]),
        ] {
            let mut axis = GeneralAxisOptions::new(id, 0, dimension, GeneralScaleType::Linear);
            axis.visible = false;
            axis.domain = GeneralAxisDomain::Numeric(domain);
            chart.add_general_axis(axis).unwrap();
        }
        let input = |target| {
            let ids = Some(
                ["a", "b", "c"]
                    .into_iter()
                    .map(|id| GeneralRowId::Text(id.into()))
                    .collect(),
            );
            if range {
                GeneralXyInput::RangeNumeric {
                    ids,
                    x: vec![1.0, 2.0, 3.0],
                    low: if target {
                        vec![3.0, 4.0, 3.0]
                    } else {
                        vec![1.0, 2.0, 1.0]
                    },
                    low_valid: None,
                    high: if target {
                        vec![5.0, 6.0, 5.0]
                    } else {
                        vec![2.0, 3.0, 2.0]
                    },
                    high_valid: None,
                }
            } else {
                GeneralXyInput::Numeric {
                    ids,
                    x: vec![1.0, 2.0, 3.0],
                    y: if target {
                        vec![5.0, 6.0, 5.0]
                    } else {
                        vec![2.0, 3.0, 2.0]
                    },
                    y_valid: None,
                }
            }
        };
        let dataset = chart.create_general_xy_dataset(input(false)).unwrap();
        let options = if range {
            GeneralSeriesOptions::range_area(0, dataset, "x", "y")
        } else {
            GeneralSeriesOptions::xy_area(0, dataset, "x", "y")
        };
        let series = chart.add_general_series(options).unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        let frame_signature = |chart: &mut ChartEngine| {
            let frame = chart.build_frame();
            (frame.panes[0].main.clone(), frame.panes[0].points.clone())
        };
        let source = frame_signature(&mut chart);
        assert!(!source.0.is_empty(), "range={range}");
        chart.set_general_transition_duration_ms(200.0).unwrap();
        chart.set_general_animation_time_ms(1_000.0);
        chart
            .replace_general_xy_dataset(dataset, input(true))
            .unwrap();
        assert!(chart.general_transition_active(), "range={range}");
        assert_eq!(frame_signature(&mut chart), source, "range={range}");
        chart.set_general_animation_time_ms(1_100.0);
        let middle = frame_signature(&mut chart);
        assert_ne!(middle, source, "range={range}");
        let plot = chart.general_plot_rect(0).unwrap();
        let value = if range { 3.75 } else { 2.0 };
        let y = plot.y + plot.height * (1.0 - value / 10.0);
        assert_eq!(
            chart
                .general_hit_test(0, plot.width / 2.0, y, crate::GeneralHitMode::Exact)
                .unwrap()
                .series,
            series,
            "range={range}",
        );
        chart.set_general_animation_time_ms(1_200.0);
        let final_frame = frame_signature(&mut chart);
        assert_ne!(middle, final_frame, "range={range}");
        assert!(!chart.general_transition_active(), "range={range}");
    }
}

#[test]
fn range_bars_and_box_plots_replay_their_shared_shapes_on_the_fixed_clock() {
    for box_plot in [false, true] {
        let mut chart = ChartEngine::new_with_initial_domain(
            400.0,
            300.0,
            1.0,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
        let mut x_axis = GeneralAxisOptions::new("x", 0, AxisDimension::X, GeneralScaleType::Band);
        x_axis.visible = false;
        chart.add_general_axis(x_axis).unwrap();
        let mut y_axis =
            GeneralAxisOptions::new("y", 0, AxisDimension::Y, GeneralScaleType::Linear);
        y_axis.visible = false;
        y_axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        chart.add_general_axis(y_axis).unwrap();
        let input = |target| {
            let ids = Some(
                ["a", "b"]
                    .into_iter()
                    .map(|id| GeneralRowId::Text(id.into()))
                    .collect(),
            );
            let categories = vec!["A".into(), "B".into()];
            let category_indices = vec![0, 1];
            if box_plot {
                GeneralXyInput::BoxCategory {
                    ids,
                    categories,
                    category_indices,
                    min: if target {
                        vec![3.0, 4.0]
                    } else {
                        vec![1.0, 2.0]
                    },
                    min_valid: None,
                    q1: if target {
                        vec![4.0, 5.0]
                    } else {
                        vec![2.0, 3.0]
                    },
                    q1_valid: None,
                    median: if target {
                        vec![5.0, 6.0]
                    } else {
                        vec![3.0, 4.0]
                    },
                    median_valid: None,
                    q3: if target {
                        vec![6.0, 7.0]
                    } else {
                        vec![4.0, 5.0]
                    },
                    q3_valid: None,
                    max: if target {
                        vec![7.0, 8.0]
                    } else {
                        vec![5.0, 6.0]
                    },
                    max_valid: None,
                }
            } else {
                GeneralXyInput::RangeCategory {
                    ids,
                    categories,
                    category_indices,
                    low: if target {
                        vec![3.0, 4.0]
                    } else {
                        vec![1.0, 2.0]
                    },
                    low_valid: None,
                    high: if target {
                        vec![6.0, 8.0]
                    } else {
                        vec![3.0, 5.0]
                    },
                    high_valid: None,
                }
            }
        };
        let dataset = chart.create_general_xy_dataset(input(false)).unwrap();
        let options = if box_plot {
            GeneralSeriesOptions::box_plot(0, dataset, "x", "y")
        } else {
            GeneralSeriesOptions::range_bar(0, dataset, "x", "y")
        };
        chart.add_general_series(options).unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        let frame_signature = |chart: &mut ChartEngine| {
            let frame = chart.build_frame();
            (frame.panes[0].main.clone(), frame.panes[0].points.clone())
        };
        let source = frame_signature(&mut chart);
        assert!(!source.0.is_empty(), "box_plot={box_plot}");
        chart.set_general_transition_duration_ms(200.0).unwrap();
        chart.set_general_animation_time_ms(1_000.0);
        chart
            .replace_general_xy_dataset(dataset, input(true))
            .unwrap();
        assert!(chart.general_transition_active(), "box_plot={box_plot}");
        assert_eq!(frame_signature(&mut chart), source, "box_plot={box_plot}");
        chart.set_general_animation_time_ms(1_100.0);
        let middle = frame_signature(&mut chart);
        assert_ne!(middle, source, "box_plot={box_plot}");
        chart.set_general_animation_time_ms(1_200.0);
        assert_ne!(frame_signature(&mut chart), middle, "box_plot={box_plot}");
        assert!(!chart.general_transition_active(), "box_plot={box_plot}");
    }
}

#[test]
fn heatmap_intensity_replays_the_shared_cells_and_exact_hits() {
    let mut chart = ChartEngine::new_with_initial_domain(
        400.0,
        300.0,
        1.0,
        HorizontalDomain::Category {
            scale: CategoryScaleType::Band,
        },
    )
    .unwrap();
    for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
        let mut axis = GeneralAxisOptions::new(id, 0, dimension, GeneralScaleType::Band);
        axis.visible = false;
        chart.add_general_axis(axis).unwrap();
    }
    let input = |target| GeneralXyInput::HeatmapCategoryCategory {
        ids: Some(
            ["a", "b"]
                .into_iter()
                .map(|id| GeneralRowId::Text(id.into()))
                .collect(),
        ),
        x_categories: vec!["A".into(), "B".into()],
        x_category_indices: vec![0, 1],
        y_categories: vec!["A".into(), "B".into()],
        y_category_indices: vec![0, 1],
        value: if target {
            vec![50.0, 70.0]
        } else {
            vec![20.0, 40.0]
        },
        value_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input(false)).unwrap();
    let mut options = GeneralSeriesOptions::heatmap_grid(0, dataset, "x", "y");
    options.heatmap_value_domain = Some([0.0, 100.0]);
    let series = chart.add_general_series(options).unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    let frame_signature = |chart: &mut ChartEngine| chart.build_frame().panes[0].main.clone();
    let source = frame_signature(&mut chart);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(true))
        .unwrap();
    assert_eq!(frame_signature(&mut chart), source);
    chart.set_general_animation_time_ms(1_100.0);
    let middle = frame_signature(&mut chart);
    assert_ne!(middle, source);
    assert_eq!(
        chart.general_geometry_dataset(dataset).unwrap().y(),
        &[35.0, 55.0]
    );
    let mut center = None;
    chart.visit_general_heatmap_cells(chart.general_series(series).unwrap(), |cell| {
        if cell.row == 0 {
            center = Some((
                (cell.left + cell.right) / 2.0,
                (cell.top + cell.bottom) / 2.0,
            ));
        }
    });
    let (x, y) = center.unwrap();
    assert_eq!(
        chart
            .general_hit_test(0, x, y, crate::GeneralHitMode::Exact)
            .unwrap()
            .row_id,
        GeneralRowIdentity::Explicit(GeneralRowId::Text("a".into())),
    );
    chart.set_general_animation_time_ms(1_200.0);
    assert_ne!(frame_signature(&mut chart), middle);
    assert!(!chart.general_transition_active());
}

#[test]
fn column_and_horizontal_bar_values_replay_their_shared_rectangles() {
    for horizontal in [false, true] {
        let mut chart = ChartEngine::new_with_initial_domain(
            400.0,
            300.0,
            1.0,
            if horizontal {
                HorizontalDomain::Continuous {
                    scale: ContinuousScaleType::Linear,
                }
            } else {
                HorizontalDomain::Category {
                    scale: CategoryScaleType::Band,
                }
            },
        )
        .unwrap();
        let mut x_axis = GeneralAxisOptions::new(
            "x",
            0,
            AxisDimension::X,
            if horizontal {
                GeneralScaleType::Linear
            } else {
                GeneralScaleType::Band
            },
        );
        x_axis.visible = false;
        if horizontal {
            x_axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        }
        chart.add_general_axis(x_axis).unwrap();
        let mut y_axis = GeneralAxisOptions::new(
            "y",
            0,
            AxisDimension::Y,
            if horizontal {
                GeneralScaleType::Band
            } else {
                GeneralScaleType::Linear
            },
        );
        y_axis.visible = false;
        if horizontal {
            y_axis.domain = GeneralAxisDomain::Category(vec!["A".into(), "B".into()]);
        } else {
            y_axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        }
        chart.add_general_axis(y_axis).unwrap();
        let input = |target| GeneralXyInput::Category {
            ids: Some(
                ["a", "b"]
                    .into_iter()
                    .map(|id| GeneralRowId::Text(id.into()))
                    .collect(),
            ),
            categories: vec!["A".into(), "B".into()],
            category_indices: vec![0, 1],
            y: if target {
                vec![5.0, 8.0]
            } else {
                vec![2.0, 4.0]
            },
            y_valid: None,
        };
        let dataset = chart.create_general_xy_dataset(input(false)).unwrap();
        let options = if horizontal {
            GeneralSeriesOptions::horizontal_bar(0, dataset, "x", "y")
        } else {
            GeneralSeriesOptions::column(0, dataset, "x", "y")
        };
        let series = chart.add_general_series(options).unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        let frame_signature = |chart: &mut ChartEngine| chart.build_frame().panes[0].main.clone();
        let source = frame_signature(&mut chart);
        chart.set_general_transition_duration_ms(200.0).unwrap();
        chart.set_general_animation_time_ms(1_000.0);
        chart
            .replace_general_xy_dataset(dataset, input(true))
            .unwrap();
        assert_eq!(
            frame_signature(&mut chart),
            source,
            "horizontal={horizontal}"
        );
        chart.set_general_animation_time_ms(1_100.0);
        let middle = frame_signature(&mut chart);
        assert_ne!(middle, source, "horizontal={horizontal}");
        let mut center = None;
        if horizontal {
            chart.visit_general_horizontal_bars(chart.general_series(series).unwrap(), |bar| {
                if bar.row == 0 {
                    center = Some(((bar.left + bar.right) / 2.0, (bar.top + bar.bottom) / 2.0));
                }
            });
        } else {
            chart.visit_general_columns(chart.general_series(series).unwrap(), |bar| {
                if bar.row == 0 {
                    center = Some(((bar.left + bar.right) / 2.0, (bar.top + bar.bottom) / 2.0));
                }
            });
        }
        let (x, y) = center.unwrap();
        assert_eq!(
            chart
                .general_hit_test(0, x, y, crate::GeneralHitMode::Exact)
                .unwrap()
                .row_id,
            GeneralRowIdentity::Explicit(GeneralRowId::Text("a".into())),
            "horizontal={horizontal}",
        );
        chart.set_general_animation_time_ms(1_200.0);
        assert_ne!(
            frame_signature(&mut chart),
            middle,
            "horizontal={horizontal}"
        );
        assert!(
            !chart.general_transition_active(),
            "horizontal={horizontal}"
        );
    }
}

#[test]
fn missing_y_row_enters_and_exits_through_the_shared_scatter_geometry() {
    let mut chart = ChartEngine::new_with_initial_domain(
        400.0,
        300.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    for (id, dimension, domain) in [
        ("x", AxisDimension::X, [0.0, 10.0]),
        ("y", AxisDimension::Y, [0.0, 20.0]),
    ] {
        let mut axis = GeneralAxisOptions::new(id, 0, dimension, GeneralScaleType::Linear);
        axis.visible = false;
        axis.domain = GeneralAxisDomain::Numeric(domain);
        chart.add_general_axis(axis).unwrap();
    }
    let input = |value, valid| GeneralXyInput::Numeric {
        ids: Some(vec![GeneralRowId::Text("one".into())]),
        x: vec![5.0],
        y: vec![value],
        y_valid: Some(vec![u8::from(valid)]),
    };
    let dataset = chart
        .create_general_xy_dataset(input(100.0, false))
        .unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::scatter(0, dataset, "x", "y"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    let visible_y = |chart: &ChartEngine| {
        let mut y = None;
        chart.visit_general_scatter_points(chart.general_series(series).unwrap(), |point| {
            y = Some(point.y);
        });
        y
    };
    assert_eq!(visible_y(&chart), None);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(12.0, true))
        .unwrap();
    assert!(chart.general_transition_active());
    assert!(
        !chart
            .general_geometry_dataset(dataset)
            .unwrap()
            .y_is_valid(0)
    );
    assert_eq!(visible_y(&chart), None);
    chart.set_general_animation_time_ms(1_100.0);
    assert!(visible_y(&chart).is_some());
    assert_eq!(chart.general_geometry_dataset(dataset).unwrap().y(), &[6.0]);
    chart.set_general_animation_time_ms(1_200.0);
    assert!(visible_y(&chart).is_some());
    chart
        .replace_general_xy_dataset(dataset, input(200.0, false))
        .unwrap();
    assert!(visible_y(&chart).is_some());
    chart.set_general_animation_time_ms(1_300.0);
    assert!(visible_y(&chart).is_some());
    assert_eq!(chart.general_geometry_dataset(dataset).unwrap().y(), &[6.0]);
    chart
        .replace_general_xy_dataset(dataset, input(12.0, true))
        .unwrap();
    assert_eq!(chart.general_geometry_dataset(dataset).unwrap().y(), &[6.0]);
    chart.set_general_animation_time_ms(1_400.0);
    assert_eq!(chart.general_geometry_dataset(dataset).unwrap().y(), &[9.0]);
    chart.set_general_animation_time_ms(1_500.0);
    assert_eq!(
        chart.general_geometry_dataset(dataset).unwrap().y(),
        &[12.0]
    );
    chart
        .replace_general_xy_dataset(dataset, input(200.0, false))
        .unwrap();
    chart.set_general_animation_time_ms(1_700.0);
    assert_eq!(visible_y(&chart), None);
    assert!(!chart.general_transition_active());
    chart
        .replace_general_xy_dataset(dataset, input(12.0, true))
        .unwrap();
    chart.set_general_animation_time_ms(1_800.0);
    assert!(visible_y(&chart).is_some());
    chart.set_general_transition_reduced_motion(true);
    assert!(!chart.general_transition_active());
    assert_eq!(
        chart.general_geometry_dataset(dataset).unwrap().y(),
        &[12.0]
    );
    assert!(visible_y(&chart).is_some());
    chart.set_general_transition_reduced_motion(false);
    chart
        .replace_general_xy_dataset(dataset, input(200.0, false))
        .unwrap();
    chart.set_general_animation_time_ms(1_900.0);
    assert!(visible_y(&chart).is_some());
    chart.set_general_transition_duration_ms(0.0).unwrap();
    assert!(!chart.general_transition_active());
    assert_eq!(visible_y(&chart), None);
}

#[test]
fn missing_bubble_size_enters_from_zero_without_snapping_the_dataset() {
    let mut chart = ChartEngine::new_with_initial_domain(
        400.0,
        300.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
        let mut axis = GeneralAxisOptions::new(id, 0, dimension, GeneralScaleType::Linear);
        axis.visible = false;
        axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        chart.add_general_axis(axis).unwrap();
    }
    let input = |valid| GeneralXyInput::Bubble {
        ids: Some(vec![GeneralRowId::Text("bubble".into())]),
        x: vec![5.0],
        y: vec![5.0],
        y_valid: None,
        size: vec![100.0],
        size_valid: Some(vec![u8::from(valid)]),
    };
    let dataset = chart.create_general_xy_dataset(input(false)).unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::bubble(0, dataset, "x", "y"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(true))
        .unwrap();
    assert!(chart.general_transition_active());
    let radius = |chart: &ChartEngine| {
        let mut radius = None;
        chart.visit_general_scatter_points(chart.general_series(series).unwrap(), |point| {
            radius = Some(point.radius)
        });
        radius
    };
    assert_eq!(radius(&chart), None);
    chart.set_general_animation_time_ms(1_100.0);
    let middle = chart.general_geometry_dataset(dataset).unwrap();
    assert_eq!(middle.size().unwrap(), &[50.0]);
    assert!(middle.size_is_valid(0));
    assert!(radius(&chart).is_some());
    chart.set_general_animation_time_ms(1_200.0);
    assert_eq!(radius(&chart), Some(10.0));
}

#[test]
fn identical_general_values_do_not_schedule_a_transition() {
    let mut chart = ChartEngine::new_with_initial_domain(
        400.0,
        300.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
        chart
            .add_general_axis(GeneralAxisOptions::new(
                id,
                0,
                dimension,
                GeneralScaleType::Linear,
            ))
            .unwrap();
    }
    let input = || GeneralXyInput::Numeric {
        ids: Some(vec![crate::GeneralRowId::Text("one".into())]),
        x: vec![2.0],
        y: vec![3.0],
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input()).unwrap();
    chart
        .add_general_series(GeneralSeriesOptions::xy_line(0, dataset, "x", "y"))
        .unwrap();
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(500.0);
    chart.replace_general_xy_dataset(dataset, input()).unwrap();
    assert!(!chart.general_transition_active());
}

#[test]
fn invalid_payload_changes_do_not_schedule_invisible_error_bar_motion() {
    let mut chart = ChartEngine::new_with_initial_domain(
        400.0,
        300.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
        let mut axis = GeneralAxisOptions::new(id, 0, dimension, GeneralScaleType::Linear);
        axis.visible = false;
        axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        chart.add_general_axis(axis).unwrap();
    }
    let input = |ignored| GeneralXyInput::ErrorNumeric {
        ids: Some(
            ["visible", "missing"]
                .into_iter()
                .map(|id| GeneralRowId::Text(id.into()))
                .collect(),
        ),
        x: vec![2.0, 4.0],
        y: vec![3.0, ignored],
        y_valid: Some(vec![1, 0]),
        x_low: vec![1.0, ignored],
        x_low_valid: Some(vec![1, 0]),
        x_high: vec![3.0, ignored],
        x_high_valid: Some(vec![1, 0]),
        y_low: vec![2.0, ignored],
        y_low_valid: Some(vec![1, 0]),
        y_high: vec![4.0, ignored],
        y_high_valid: Some(vec![1, 0]),
    };
    let dataset = chart.create_general_xy_dataset(input(0.0)).unwrap();
    chart
        .add_general_series(GeneralSeriesOptions::error_bar(0, dataset, "x", "y"))
        .unwrap();
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(9.0))
        .unwrap();
    assert_eq!(chart.general_dataset(dataset).unwrap().y(), &[3.0, 9.0]);
    assert!(!chart.general_transition_active());
}

#[test]
fn entering_numeric_error_bar_starts_with_collapsed_horizontal_bounds() {
    let mut chart = ChartEngine::new_with_initial_domain(
        400.0,
        300.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
        let mut axis = GeneralAxisOptions::new(id, 0, dimension, GeneralScaleType::Linear);
        axis.visible = false;
        axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        chart.add_general_axis(axis).unwrap();
    }
    let input = |new_row: bool| GeneralXyInput::ErrorNumeric {
        ids: Some(
            if new_row { vec!["a", "b"] } else { vec!["a"] }
                .into_iter()
                .map(|id| GeneralRowId::Text(id.into()))
                .collect(),
        ),
        x: if new_row { vec![4.0, 6.0] } else { vec![4.0] },
        y: if new_row { vec![8.0, 5.0] } else { vec![8.0] },
        y_valid: None,
        x_low: if new_row { vec![3.0, 5.0] } else { vec![3.0] },
        x_low_valid: None,
        x_high: if new_row { vec![5.0, 7.0] } else { vec![5.0] },
        x_high_valid: None,
        y_low: if new_row { vec![7.0, 4.0] } else { vec![7.0] },
        y_low_valid: None,
        y_high: if new_row { vec![9.0, 6.0] } else { vec![9.0] },
        y_high_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input(false)).unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::error_bar(0, dataset, "x", "y"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(true))
        .unwrap();
    let mut entering = None;
    chart.visit_general_error_bars(chart.general_series(series).unwrap(), |bar| {
        if bar.row == 1 {
            entering = Some(bar);
        }
    });
    let start = entering.unwrap();
    assert!((start.x_low.unwrap() - start.x).abs() < 1e-10);
    assert!((start.x_high.unwrap() - start.x).abs() < 1e-10);
    chart.set_general_animation_time_ms(1_100.0);
    entering = None;
    chart.visit_general_error_bars(chart.general_series(series).unwrap(), |bar| {
        if bar.row == 1 {
            entering = Some(bar);
        }
    });
    let entering = entering.unwrap();
    assert!(entering.x_low.unwrap() < entering.x);
    assert!(entering.x < entering.x_high.unwrap());
}

#[test]
fn missing_error_bounds_morph_from_their_center_on_the_shared_clock() {
    let mut chart = ChartEngine::new_with_initial_domain(
        400.0,
        300.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
        let mut axis = GeneralAxisOptions::new(id, 0, dimension, GeneralScaleType::Linear);
        axis.visible = false;
        axis.domain = GeneralAxisDomain::Numeric([0.0, 20.0]);
        chart.add_general_axis(axis).unwrap();
    }
    let input = |valid| GeneralXyInput::ErrorNumeric {
        ids: Some(vec![GeneralRowId::Text("one".into())]),
        x: vec![5.0],
        y: vec![10.0],
        y_valid: None,
        x_low: vec![4.0],
        x_low_valid: Some(vec![u8::from(valid)]),
        x_high: vec![6.0],
        x_high_valid: Some(vec![u8::from(valid)]),
        y_low: vec![8.0],
        y_low_valid: Some(vec![u8::from(valid)]),
        y_high: vec![12.0],
        y_high_valid: Some(vec![u8::from(valid)]),
    };
    let dataset = chart.create_general_xy_dataset(input(false)).unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::error_bar(0, dataset, "x", "y"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(true))
        .unwrap();
    assert!(chart.general_transition_active());
    let mut bounds = None;
    chart.visit_general_error_bars(chart.general_series(series).unwrap(), |bar| {
        bounds = Some((bar.x_low, bar.x_high, bar.y_low, bar.y_high));
    });
    assert_eq!(bounds, Some((None, None, None, None)));
    chart.set_general_animation_time_ms(1_100.0);
    let middle = chart.general_geometry_dataset(dataset).unwrap();
    assert_eq!(middle.x_low().unwrap(), &[4.5]);
    assert_eq!(middle.x_high().unwrap(), &[5.5]);
    assert_eq!(middle.low().unwrap(), &[9.0]);
    assert_eq!(middle.high().unwrap(), &[11.0]);
    assert!(middle.x_low_is_valid(0) && middle.x_high_is_valid(0));
    assert!(middle.low_is_valid(0) && middle.high_is_valid(0));
    chart.set_general_animation_time_ms(1_200.0);
    chart
        .replace_general_xy_dataset(dataset, input(false))
        .unwrap();
    chart.set_general_animation_time_ms(1_300.0);
    let middle = chart.general_geometry_dataset(dataset).unwrap();
    assert_eq!(middle.x_low().unwrap(), &[4.5]);
    assert_eq!(middle.x_high().unwrap(), &[5.5]);
    assert_eq!(middle.low().unwrap(), &[9.0]);
    assert_eq!(middle.high().unwrap(), &[11.0]);
    chart.set_general_animation_time_ms(1_400.0);
    assert!(!chart.general_transition_active());
    assert!(
        !chart
            .general_geometry_dataset(dataset)
            .unwrap()
            .low_is_valid(0)
    );
}

#[test]
fn entering_temporal_error_bar_collapses_epoch_bounds_at_its_center() {
    let mut chart = ChartEngine::new(400.0, 300.0, 1.0);
    let pane = chart
        .add_pane_with_domain(true, HorizontalDomain::Temporal)
        .unwrap();
    let mut x_axis =
        GeneralAxisOptions::new("x", pane, AxisDimension::X, GeneralScaleType::Temporal);
    x_axis.visible = false;
    x_axis.domain = GeneralAxisDomain::Temporal([1_700_000_000_000, 1_700_000_100_000]);
    chart.add_general_axis(x_axis).unwrap();
    let mut y_axis = GeneralAxisOptions::new("y", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y_axis.visible = false;
    y_axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
    chart.add_general_axis(y_axis).unwrap();
    let input = |new_row: bool| GeneralXyInput::ErrorTemporal {
        ids: Some(
            if new_row { vec!["a", "b"] } else { vec!["a"] }
                .into_iter()
                .map(|id| GeneralRowId::Text(id.into()))
                .collect(),
        ),
        x_epoch_ms: if new_row {
            vec![1_700_000_020_000, 1_700_000_060_000]
        } else {
            vec![1_700_000_020_000]
        },
        y: if new_row { vec![8.0, 5.0] } else { vec![8.0] },
        y_valid: None,
        x_low_epoch_ms: if new_row {
            vec![1_700_000_010_000.0, 1_700_000_050_000.0]
        } else {
            vec![1_700_000_010_000.0]
        },
        x_low_valid: None,
        x_high_epoch_ms: if new_row {
            vec![1_700_000_030_000.0, 1_700_000_070_000.0]
        } else {
            vec![1_700_000_030_000.0]
        },
        x_high_valid: None,
        y_low: if new_row { vec![7.0, 4.0] } else { vec![7.0] },
        y_low_valid: None,
        y_high: if new_row { vec![9.0, 6.0] } else { vec![9.0] },
        y_high_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input(false)).unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::error_bar(pane, dataset, "x", "y"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(true))
        .unwrap();
    let mut entering = None;
    chart.visit_general_error_bars(chart.general_series(series).unwrap(), |bar| {
        if bar.row == 1 {
            entering = Some(bar);
        }
    });
    let start = entering.unwrap();
    assert!((start.x_low.unwrap() - start.x).abs() < 1e-8);
    assert!((start.x_high.unwrap() - start.x).abs() < 1e-8);
    chart.set_general_animation_time_ms(1_100.0);
    entering = None;
    chart.visit_general_error_bars(chart.general_series(series).unwrap(), |bar| {
        if bar.row == 1 {
            entering = Some(bar);
        }
    });
    let midpoint = entering.unwrap();
    assert!(midpoint.x_low.unwrap() < midpoint.x);
    assert!(midpoint.x < midpoint.x_high.unwrap());
}

#[test]
fn temporal_x_transition_moves_geometry_and_hit_with_fixed_clock() {
    let mut chart = ChartEngine::new(400.0, 300.0, 1.0);
    let pane = chart
        .add_pane_with_domain(true, HorizontalDomain::Temporal)
        .unwrap();
    let epoch = 1_700_000_000_000_i64;
    let mut x_axis =
        GeneralAxisOptions::new("x", pane, AxisDimension::X, GeneralScaleType::Temporal);
    x_axis.visible = false;
    x_axis.domain = GeneralAxisDomain::Temporal([epoch, epoch + 4_000]);
    chart.add_general_axis(x_axis).unwrap();
    let mut y_axis = GeneralAxisOptions::new("y", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y_axis.visible = false;
    y_axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
    chart.add_general_axis(y_axis).unwrap();
    let input = |x| GeneralXyInput::Temporal {
        ids: Some(vec![GeneralRowId::Text("moving".into())]),
        x_epoch_ms: vec![x],
        y: vec![5.0],
        y_valid: None,
    };
    let dataset = chart
        .create_general_xy_dataset(input(epoch + 1_000))
        .unwrap();
    let mut options = GeneralSeriesOptions::xy_line(pane, dataset, "x", "y");
    options.point_markers = true;
    let series = chart.add_general_series(options).unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(epoch + 3_000))
        .unwrap();
    assert!(chart.general_transition_active());
    let point_x = |chart: &ChartEngine| {
        let mut x = None;
        chart.visit_general_path_points(chart.general_series(series).unwrap(), |point| {
            x = Some(point.x);
        });
        x.unwrap()
    };
    let plot = chart.general_plot_rect(pane).unwrap();
    assert!((point_x(&chart) - plot.width * 0.25).abs() < 1e-8);
    chart.set_general_animation_time_ms(1_100.0);
    let midpoint = point_x(&chart);
    assert!((midpoint - plot.width * 0.5).abs() < 1e-8);
    let y = plot.y + plot.height * 0.5;
    assert_eq!(
        chart
            .general_hit_test(
                pane,
                midpoint,
                y,
                crate::GeneralHitMode::Nearest { max_distance: 6.0 },
            )
            .unwrap()
            .row,
        0
    );
    chart.set_general_animation_time_ms(1_200.0);
    assert!((point_x(&chart) - plot.width * 0.75).abs() < 1e-8);
    assert!(!chart.general_transition_active());
}

#[test]
fn line_value_transition_survives_unused_optional_channel_changes() {
    let mut chart = ChartEngine::new_with_initial_domain(
        400.0,
        300.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
        let mut axis = GeneralAxisOptions::new(id, 0, dimension, GeneralScaleType::Linear);
        axis.visible = false;
        axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        chart.add_general_axis(axis).unwrap();
    }
    let numeric = |value| GeneralXyInput::Numeric {
        ids: Some(vec![GeneralRowId::Text("one".into())]),
        x: vec![2.0],
        y: vec![value],
        y_valid: None,
    };
    let range = |high| GeneralXyInput::RangeNumeric {
        ids: Some(vec![GeneralRowId::Text("one".into())]),
        x: vec![2.0],
        low: vec![4.0],
        low_valid: None,
        high: vec![high],
        high_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(numeric(2.0)).unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::xy_line(0, dataset, "x", "y"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, range(6.0))
        .unwrap();
    assert!(chart.general_transition_active());
    assert_eq!(
        chart.general_geometry_dataset(dataset).unwrap().low(),
        Some(&[2.0][..])
    );
    chart.set_general_animation_time_ms(1_100.0);
    assert_eq!(
        chart.general_geometry_dataset(dataset).unwrap().low(),
        Some(&[3.0][..])
    );
    let mut point = None;
    chart.visit_general_path_points(chart.general_series(series).unwrap(), |value| {
        point = Some(value)
    });
    let plot = chart.general_plot_rect(0).unwrap();
    assert!((point.unwrap().y - (plot.y + plot.height * 0.6)).abs() < 1e-8);

    chart.set_general_animation_time_ms(1_200.0);
    chart
        .replace_general_xy_dataset(dataset, numeric(8.0))
        .unwrap();
    assert!(chart.general_transition_active());
    chart.set_general_animation_time_ms(1_300.0);
    point = None;
    chart.visit_general_path_points(chart.general_series(series).unwrap(), |value| {
        point = Some(value)
    });
    assert!((point.unwrap().y - (plot.y + plot.height * 0.3)).abs() < 1e-8);
    chart.set_general_animation_time_ms(1_400.0);
    chart
        .replace_general_xy_dataset(dataset, range(8.0))
        .unwrap();
    assert!(!chart.general_transition_active());
}

#[test]
fn automatic_general_domain_tracks_transition_presentation() {
    let mut chart = ChartEngine::new_with_initial_domain(
        400.0,
        300.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "x",
            0,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "y",
            0,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let input = |a, b| GeneralXyInput::Numeric {
        ids: Some(vec![
            crate::GeneralRowId::Text("a".into()),
            crate::GeneralRowId::Text("b".into()),
        ]),
        x: vec![1.0, 2.0],
        y: vec![a, b],
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input(2.0, 4.0)).unwrap();
    chart
        .add_general_series(GeneralSeriesOptions::xy_line(0, dataset, "x", "y"))
        .unwrap();
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(6.0, 8.0))
        .unwrap();
    let domain = |chart: &ChartEngine| {
        chart
            .effective_general_axis_domain(chart.general_axis("y").unwrap())
            .unwrap()
    };
    // Line axes keep zero on the axis and reserve a 4% margin past the nice upper tick.
    assert_eq!(domain(&chart), GeneralAxisDomain::Numeric([0.0, 4.16]));
    chart.set_general_animation_time_ms(1_100.0);
    assert_eq!(domain(&chart), GeneralAxisDomain::Numeric([0.0, 6.24]));
    chart.set_general_animation_time_ms(1_200.0);
    assert_eq!(domain(&chart), GeneralAxisDomain::Numeric([0.0, 8.32]));
}

#[test]
fn radar_transition_budget_applies_target_atomically_for_many_bound_series() {
    const ROWS: usize = 4_096;
    let mut chart =
        ChartEngine::new_with_initial_domain(400.0, 400.0, 1.0, HorizontalDomain::Polar).unwrap();
    let mut angle = GeneralAxisOptions::new(
        "angle",
        0,
        AxisDimension::Angle,
        GeneralScaleType::AngularCategory,
    );
    angle.visible = false;
    chart.add_general_axis(angle).unwrap();
    let mut radius = GeneralAxisOptions::new(
        "radius",
        0,
        AxisDimension::Radius,
        GeneralScaleType::RadialLinear,
    );
    radius.visible = false;
    radius.domain = GeneralAxisDomain::Numeric([0.0, 2.0]);
    chart.add_general_axis(radius).unwrap();
    let labels = (0..ROWS).map(|row| format!("C{row}")).collect::<Vec<_>>();
    let input = |reversed| GeneralXyInput::Category {
        ids: Some(
            (0..ROWS)
                .map(|row| GeneralRowId::Number(row as f64))
                .collect(),
        ),
        categories: if reversed {
            labels.iter().rev().cloned().collect()
        } else {
            labels.clone()
        },
        category_indices: (0..ROWS)
            .map(|row| if reversed { ROWS - 1 - row } else { row } as u32)
            .collect(),
        y: vec![1.0; ROWS],
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input(false)).unwrap();
    for _ in 0..32 {
        chart
            .add_general_series(GeneralSeriesOptions::radar(0, dataset, "angle", "radius"))
            .unwrap();
    }
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(true))
        .unwrap();
    assert!(!chart.general_transition_active());
    assert_eq!(
        chart
            .general_dataset(dataset)
            .unwrap()
            .categories()
            .unwrap()[0],
        "C4095"
    );
    assert!(chart.memory_usage().general_data_capacity_bytes < 8 * 1024 * 1024);
}

#[test]
fn radar_category_entry_preserves_the_source_fill_at_transition_start() {
    let mut chart =
        ChartEngine::new_with_initial_domain(400.0, 400.0, 1.0, HorizontalDomain::Polar).unwrap();
    let mut angle = GeneralAxisOptions::new(
        "angle",
        0,
        AxisDimension::Angle,
        GeneralScaleType::AngularCategory,
    );
    angle.visible = false;
    chart.add_general_axis(angle).unwrap();
    let mut radius = GeneralAxisOptions::new(
        "radius",
        0,
        AxisDimension::Radius,
        GeneralScaleType::RadialLinear,
    );
    radius.visible = false;
    chart.add_general_axis(radius).unwrap();
    let input = |include_d: bool| GeneralXyInput::Category {
        ids: Some(
            ["a", "b", "c", "d"]
                .into_iter()
                .take(if include_d { 4 } else { 3 })
                .map(|id| GeneralRowId::Text(id.into()))
                .collect(),
        ),
        categories: ["A", "B", "C", "D"]
            .into_iter()
            .take(if include_d { 4 } else { 3 })
            .map(str::to_owned)
            .collect(),
        category_indices: (0..if include_d { 4 } else { 3 }).collect(),
        y: [1.0, 2.0, 4.0, 12.0]
            .into_iter()
            .take(if include_d { 4 } else { 3 })
            .collect(),
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input(false)).unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::radar(0, dataset, "angle", "radius"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    let fill_area = |chart: &ChartEngine| {
        let mut area = 0.0;
        chart.with_general_radar_polygon(chart.general_series(series).unwrap(), |polygon, _, _| {
            polygon.visit_triangles(|triangle| {
                let a = (triangle[1].0 - triangle[0].0, triangle[1].1 - triangle[0].1);
                let b = (triangle[2].0 - triangle[0].0, triangle[2].1 - triangle[0].1);
                area += (a.0 * b.1 - a.1 * b.0).abs() / 2.0;
            });
        });
        area
    };
    let source = fill_area(&chart);
    assert!(source > 100.0);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(true))
        .unwrap();
    assert!(chart.general_transition_active());
    assert!((fill_area(&chart) - source).abs() < 1e-8);
    let mut source_markers = 0;
    chart.with_general_radar_polygon(chart.general_series(series).unwrap(), |_, _, markers| {
        source_markers = markers.iter().flatten().count();
    });
    assert_eq!(source_markers, 3);
    chart.set_general_animation_time_ms(1_100.0);
    let middle = fill_area(&chart);
    assert!(middle > 100.0);
    let mut target_markers = 0;
    chart.with_general_radar_polygon(chart.general_series(series).unwrap(), |_, _, markers| {
        target_markers = markers.iter().flatten().count();
    });
    assert_eq!(target_markers, 4);
    chart
        .replace_general_xy_dataset(dataset, input(false))
        .unwrap();
    assert!((fill_area(&chart) - middle).abs() < 1e-8);
    chart.set_general_animation_time_ms(1_300.0);
    assert!(!chart.general_transition_active());
    assert!((fill_area(&chart) - source).abs() < 1e-8);
    chart.set_general_animation_time_ms(2_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(true))
        .unwrap();
    chart.set_general_animation_time_ms(2_199.999);
    let near_target = fill_area(&chart);
    chart.set_general_animation_time_ms(2_200.0);
    assert!((fill_area(&chart) - near_target).abs() < 0.1);
}

#[test]
fn radar_reorder_morphs_slot_radii_and_interrupts_from_the_sample() {
    let mut chart =
        ChartEngine::new_with_initial_domain(400.0, 400.0, 1.0, HorizontalDomain::Polar).unwrap();
    let mut angle = GeneralAxisOptions::new(
        "angle",
        0,
        AxisDimension::Angle,
        GeneralScaleType::AngularCategory,
    );
    angle.visible = false;
    chart.add_general_axis(angle).unwrap();
    let mut radius = GeneralAxisOptions::new(
        "radius",
        0,
        AxisDimension::Radius,
        GeneralScaleType::RadialLinear,
    );
    radius.visible = false;
    radius.domain = GeneralAxisDomain::Numeric([0.0, 5.0]);
    chart.add_general_axis(radius).unwrap();
    let input = |categories: [&str; 3], indices: [u32; 3]| GeneralXyInput::Category {
        ids: Some(
            ["a", "b", "c"]
                .map(|id| GeneralRowId::Text(id.into()))
                .to_vec(),
        ),
        categories: categories.map(str::to_owned).to_vec(),
        category_indices: indices.to_vec(),
        y: vec![1.0, 2.0, 4.0],
        y_valid: None,
    };
    let dataset = chart
        .create_general_xy_dataset(input(["A", "B", "C"], [0, 1, 2]))
        .unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::radar(0, dataset, "angle", "radius"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    let slot_zero_radius = |chart: &ChartEngine| {
        let mut radius = None;
        chart.with_general_radar_polygon(chart.general_series(series).unwrap(), |polygon, _, _| {
            let point = polygon.vertex(0).unwrap();
            radius = Some(polygon.plot().invert(point.0, point.1).unwrap().1);
        });
        radius.unwrap()
    };
    let source = slot_zero_radius(&chart);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(["C", "B", "A"], [2, 1, 0]))
        .unwrap();
    assert!((slot_zero_radius(&chart) - source).abs() < 1e-8);
    chart.set_general_animation_time_ms(1_100.0);
    let middle = slot_zero_radius(&chart);
    chart.set_general_animation_time_ms(1_200.0);
    let target = slot_zero_radius(&chart);
    assert!((middle - (source + target) / 2.0).abs() < 1e-8);
    chart.set_general_animation_time_ms(2_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(["A", "B", "C"], [0, 1, 2]))
        .unwrap();
    chart.set_general_animation_time_ms(2_100.0);
    let sampled = slot_zero_radius(&chart);
    chart
        .replace_general_xy_dataset(dataset, input(["B", "C", "A"], [2, 0, 1]))
        .unwrap();
    assert!((slot_zero_radius(&chart) - sampled).abs() < 1e-8);
    chart.set_general_animation_time_ms(2_200.0);
    let interrupted_middle = slot_zero_radius(&chart);
    chart.set_general_animation_time_ms(2_300.0);
    let interrupted_target = slot_zero_radius(&chart);
    assert!((interrupted_middle - (sampled + interrupted_target) / 2.0).abs() < 1e-8);
}

#[test]
fn radial_bar_category_reorder_moves_sectors_with_the_fixed_clock() {
    let mut chart =
        ChartEngine::new_with_initial_domain(400.0, 400.0, 1.0, HorizontalDomain::Polar).unwrap();
    let mut angle = GeneralAxisOptions::new(
        "angle",
        0,
        AxisDimension::Angle,
        GeneralScaleType::AngularCategory,
    );
    angle.visible = false;
    chart.add_general_axis(angle).unwrap();
    let mut radius = GeneralAxisOptions::new(
        "radius",
        0,
        AxisDimension::Radius,
        GeneralScaleType::RadialLinear,
    );
    radius.visible = false;
    radius.domain = GeneralAxisDomain::Numeric([0.0, 4.0]);
    chart.add_general_axis(radius).unwrap();
    let input = |categories: [&str; 3], indices: [u32; 3]| GeneralXyInput::Category {
        ids: Some(
            ["a", "b", "c"]
                .map(|id| GeneralRowId::Text(id.into()))
                .to_vec(),
        ),
        categories: categories.map(str::to_owned).to_vec(),
        category_indices: indices.to_vec(),
        y: vec![2.0; 3],
        y_valid: None,
    };
    let dataset = chart
        .create_general_xy_dataset(input(["A", "B", "C"], [0, 1, 2]))
        .unwrap();
    let radar = chart
        .add_general_series(GeneralSeriesOptions::radar(0, dataset, "angle", "radius"))
        .unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::radial_bar(
            0, dataset, "angle", "radius",
        ))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    let center_for_a = |chart: &ChartEngine| {
        let mut center = None;
        chart.visit_general_polar_value_sectors(chart.general_series(series).unwrap(), |item| {
            if item.row == 0 {
                center = item
                    .sector
                    .plot()
                    .point(item.sector.mid_angle(), item.sector.mid_radius());
            }
        });
        center.unwrap()
    };
    let radar_a = |chart: &ChartEngine| {
        let mut point = None;
        chart.with_general_radar_polygon(
            chart.general_series(radar).unwrap(),
            |polygon, rows, _| {
                point = rows
                    .iter()
                    .position(|row| *row == Some(0))
                    .and_then(|index| polygon.vertex(index));
            },
        );
        point.unwrap()
    };
    let source = center_for_a(&chart);
    let radar_source = radar_a(&chart);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(["C", "B", "A"], [2, 1, 0]))
        .unwrap();
    assert!(chart.general_transition_active());
    let initial = center_for_a(&chart);
    let radar_initial = radar_a(&chart);
    assert!((initial.0 - source.0).abs() < 1e-8);
    assert!((initial.1 - source.1).abs() < 1e-8);
    assert!((radar_initial.0 - radar_source.0).abs() < 1e-8);
    assert!((radar_initial.1 - radar_source.1).abs() < 1e-8);
    chart.set_general_animation_time_ms(1_100.0);
    let middle = center_for_a(&chart);
    let mut radar_area = 0.0;
    chart.with_general_radar_polygon(chart.general_series(radar).unwrap(), |polygon, _, _| {
        polygon.visit_triangles(|triangle| {
            let a = (triangle[1].0 - triangle[0].0, triangle[1].1 - triangle[0].1);
            let b = (triangle[2].0 - triangle[0].0, triangle[2].1 - triangle[0].1);
            radar_area += (a.0 * b.1 - a.1 * b.0).abs() / 2.0;
        });
    });
    assert!(
        radar_area > 100.0,
        "positive radar values must stay visible during reorder"
    );
    let mut moving_sectors = Vec::new();
    chart.visit_general_polar_value_sectors(chart.general_series(series).unwrap(), |item| {
        moving_sectors.push(item)
    });
    for item in &moving_sectors {
        let point = item
            .sector
            .plot()
            .point(item.sector.mid_angle(), item.sector.mid_radius())
            .unwrap();
        let expected = moving_sectors
            .iter()
            .rev()
            .find(|candidate| candidate.sector.contains(point.0, point.1))
            .map(|candidate| (series, candidate.row));
        assert_eq!(
            chart
                .general_hit_test(0, point.0, point.1, crate::GeneralHitMode::Exact)
                .map(|hit| (hit.series, hit.row)),
            expected
        );
    }
    chart.set_general_animation_time_ms(1_200.0);
    let target = center_for_a(&chart);
    assert!((middle.0 - source.0).abs() > 1.0 || (middle.1 - source.1).abs() > 1.0);
    assert!((middle.0 - target.0).abs() > 1.0 || (middle.1 - target.1).abs() > 1.0);
    chart.set_general_animation_time_ms(2_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(["A", "B", "C"], [0, 1, 2]))
        .unwrap();
    chart.set_general_animation_time_ms(2_100.0);
    let sampled = center_for_a(&chart);
    chart
        .replace_general_xy_dataset(dataset, input(["B", "A", "C"], [1, 0, 2]))
        .unwrap();
    let restarted = center_for_a(&chart);
    assert!((restarted.0 - sampled.0).abs() < 1e-8);
    assert!((restarted.1 - sampled.1).abs() < 1e-8);
}

#[test]
fn pie_row_reorder_moves_slices_with_the_fixed_clock() {
    let mut chart =
        ChartEngine::new_with_initial_domain(400.0, 400.0, 1.0, HorizontalDomain::Polar).unwrap();
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
        chart.add_general_axis(axis).unwrap();
    }
    let input = |ids: [&str; 3]| GeneralXyInput::Category {
        ids: Some(
            ids.iter()
                .map(|id| GeneralRowId::Text((*id).into()))
                .collect(),
        ),
        categories: vec!["A".into(), "B".into(), "C".into()],
        category_indices: ids
            .iter()
            .map(|id| match *id {
                "a" => 0,
                "b" => 1,
                _ => 2,
            })
            .collect(),
        y: vec![1.0; 3],
        y_valid: None,
    };
    let dataset = chart
        .create_general_xy_dataset(input(["a", "b", "c"]))
        .unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::pie(0, dataset, "angle", "radius"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
    let angle_for_a = |chart: &ChartEngine| {
        let mut angle = None;
        chart.visit_general_pie_slices(chart.general_series(series).unwrap(), |slice| {
            if chart
                .general_geometry_dataset(dataset)
                .unwrap()
                .row_identity(slice.row)
                == Some(&GeneralRowIdentity::Explicit(GeneralRowId::Text(
                    "a".into(),
                )))
            {
                angle = Some(slice.sector.mid_angle());
            }
        });
        angle.unwrap()
    };
    let source = angle_for_a(&chart);
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(1_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(["c", "b", "a"]))
        .unwrap();
    assert!(chart.general_transition_active());
    assert!((angle_for_a(&chart) - source).abs() < 1e-8);
    chart.set_general_animation_time_ms(1_100.0);
    let middle = angle_for_a(&chart);
    chart.set_general_animation_time_ms(1_200.0);
    let target = angle_for_a(&chart);
    assert!((middle - (source + target) / 2.0).abs() < 1e-8);
    assert!(!chart.general_transition_active());
    chart.set_general_animation_time_ms(2_000.0);
    chart
        .replace_general_xy_dataset(dataset, input(["a", "b", "c"]))
        .unwrap();
    chart.set_general_animation_time_ms(2_100.0);
    let sampled = angle_for_a(&chart);
    chart
        .replace_general_xy_dataset(dataset, input(["b", "a", "c"]))
        .unwrap();
    assert!((angle_for_a(&chart) - sampled).abs() < 1e-8);
    chart.set_general_animation_time_ms(2_200.0);
    let mut slices = Vec::new();
    chart.visit_general_pie_slices(chart.general_series(series).unwrap(), |slice| {
        slices.push(slice)
    });
    for slice in &slices {
        let point = slice
            .sector
            .plot()
            .point(slice.sector.mid_angle(), slice.sector.mid_radius())
            .unwrap();
        let expected = slices
            .iter()
            .rev()
            .find(|candidate| candidate.sector.contains(point.0, point.1))
            .map(|candidate| candidate.row);
        assert_eq!(
            chart
                .general_hit_test(0, point.0, point.1, crate::GeneralHitMode::Exact)
                .map(|hit| hit.row),
            expected
        );
    }
}

#[test]
fn polar_transition_matches_reordered_rows_by_identity_and_enters_from_zero() {
    let mut chart =
        ChartEngine::new_with_initial_domain(400.0, 400.0, 1.0, HorizontalDomain::Polar).unwrap();
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
        chart.add_general_axis(axis).unwrap();
    }
    let input = |ids: &[&str], values: &[f64]| GeneralXyInput::Category {
        ids: Some(
            ids.iter()
                .map(|id| crate::GeneralRowId::Text((*id).into()))
                .collect(),
        ),
        categories: vec!["A".into(), "B".into(), "C".into()],
        category_indices: ids
            .iter()
            .map(|id| match *id {
                "a" => 0,
                "b" => 1,
                _ => 2,
            })
            .collect(),
        y: values.to_vec(),
        y_valid: None,
    };
    let dataset = chart
        .create_general_xy_dataset(input(&["a", "b"], &[1.0, 3.0]))
        .unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::pie(0, dataset, "angle", "radius"))
        .unwrap();
    assert!(chart.select_general_row(series, 0));
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart.set_general_animation_time_ms(100.0);
    chart
        .replace_general_xy_dataset(dataset, input(&["b", "a", "c"], &[1.0, 3.0, 2.0]))
        .unwrap();
    assert_eq!(chart.general_selected_hit().unwrap().row, 1);
    assert_eq!(
        chart.general_dataset(dataset).unwrap().y(),
        &[1.0, 3.0, 2.0]
    );
    assert_eq!(
        chart.general_geometry_dataset(dataset).unwrap().y(),
        &[3.0, 1.0, 0.0]
    );
    chart.set_general_animation_time_ms(200.0);
    assert_eq!(
        chart.general_geometry_dataset(dataset).unwrap().y(),
        &[2.0, 2.0, 1.0]
    );
    chart.set_general_animation_time_ms(300.0);
    assert!(!chart.general_transition_active());
    assert_eq!(
        chart.general_geometry_dataset(dataset).unwrap().y(),
        &[1.0, 3.0, 2.0]
    );
}

#[test]
fn general_transition_skips_retention_above_its_row_budget() {
    let mut chart = ChartEngine::new_with_initial_domain(
        400.0,
        300.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
        chart
            .add_general_axis(GeneralAxisOptions::new(
                id,
                0,
                dimension,
                GeneralScaleType::Linear,
            ))
            .unwrap();
    }
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: Some(vec![crate::GeneralRowId::Number(0.0)]),
            x: vec![0.0],
            y: vec![1.0],
            y_valid: None,
        })
        .unwrap();
    chart
        .add_general_series(GeneralSeriesOptions::xy_line(0, dataset, "x", "y"))
        .unwrap();
    chart.set_general_transition_duration_ms(200.0).unwrap();
    let rows = 16_385;
    chart
        .replace_general_xy_dataset(
            dataset,
            GeneralXyInput::Numeric {
                ids: Some(
                    (0..rows)
                        .map(|row| crate::GeneralRowId::Number(row as f64))
                        .collect(),
                ),
                x: (0..rows).map(|row| row as f64).collect(),
                y: vec![1.0; rows],
                y_valid: None,
            },
        )
        .unwrap();
    assert!(!chart.general_transition_active());
    assert_eq!(chart.general_dataset(dataset).unwrap().len(), rows);
}

#[test]
fn exiting_and_entering_rows_share_one_bounded_presentation_budget() {
    let mut chart = ChartEngine::new_with_initial_domain(
        400.0,
        300.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
        chart
            .add_general_axis(GeneralAxisOptions::new(
                id,
                0,
                dimension,
                GeneralScaleType::Linear,
            ))
            .unwrap();
    }
    let rows = 8_193;
    let input = |offset: usize| GeneralXyInput::Numeric {
        ids: Some(
            (offset..offset + rows)
                .map(|row| GeneralRowId::Number(row as f64))
                .collect(),
        ),
        x: (offset..offset + rows).map(|row| row as f64).collect(),
        y: vec![1.0; rows],
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input(0)).unwrap();
    chart
        .add_general_series(GeneralSeriesOptions::xy_line(0, dataset, "x", "y"))
        .unwrap();
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart
        .replace_general_xy_dataset(dataset, input(rows))
        .unwrap();
    assert!(!chart.general_transition_active());
    assert_eq!(chart.general_dataset(dataset).unwrap().len(), rows);
}

#[test]
fn polar_exit_union_respects_the_sector_limit() {
    let mut chart =
        ChartEngine::new_with_initial_domain(400.0, 400.0, 1.0, HorizontalDomain::Polar).unwrap();
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
        chart
            .add_general_axis(GeneralAxisOptions::new(id, 0, dimension, scale))
            .unwrap();
    }
    let rows = 2_049;
    let input = |offset: usize| GeneralXyInput::Category {
        ids: Some(
            (offset..offset + rows)
                .map(|row| GeneralRowId::Number(row as f64))
                .collect(),
        ),
        categories: (offset..offset + rows).map(|row| row.to_string()).collect(),
        category_indices: (0..rows as u32).collect(),
        y: vec![1.0; rows],
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input(0)).unwrap();
    chart
        .add_general_series(GeneralSeriesOptions::pie(0, dataset, "angle", "radius"))
        .unwrap();
    chart.set_general_transition_duration_ms(200.0).unwrap();
    chart
        .replace_general_xy_dataset(dataset, input(rows))
        .unwrap();
    assert!(!chart.general_transition_active());
    assert_eq!(chart.general_dataset(dataset).unwrap().len(), rows);
}

#[test]
fn polar_value_families_share_fixed_clock_geometry_and_exact_hits() {
    for kind in [
        GeneralSeriesKind::Radar,
        GeneralSeriesKind::PolarArea,
        GeneralSeriesKind::RadialBar,
    ] {
        let mut chart =
            ChartEngine::new_with_initial_domain(400.0, 400.0, 1.0, HorizontalDomain::Polar)
                .unwrap();
        let mut angle = GeneralAxisOptions::new(
            "angle",
            0,
            AxisDimension::Angle,
            GeneralScaleType::AngularCategory,
        );
        angle.visible = false;
        chart.add_general_axis(angle).unwrap();
        let mut radius = GeneralAxisOptions::new(
            "radius",
            0,
            AxisDimension::Radius,
            GeneralScaleType::RadialLinear,
        );
        radius.visible = false;
        radius.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        chart.add_general_axis(radius).unwrap();
        let input = |values| GeneralXyInput::Category {
            ids: Some(
                ["a", "b", "c"]
                    .map(|id| GeneralRowId::Text(id.into()))
                    .to_vec(),
            ),
            categories: ["A", "B", "C"].map(str::to_owned).to_vec(),
            category_indices: vec![0, 1, 2],
            y: values,
            y_valid: None,
        };
        let dataset = chart
            .create_general_xy_dataset(input(vec![2.0, 4.0, 6.0]))
            .unwrap();
        let options = match kind {
            GeneralSeriesKind::Radar => GeneralSeriesOptions::radar(0, dataset, "angle", "radius"),
            GeneralSeriesKind::PolarArea => {
                GeneralSeriesOptions::polar_area(0, dataset, "angle", "radius")
            }
            GeneralSeriesKind::RadialBar => {
                GeneralSeriesOptions::radial_bar(0, dataset, "angle", "radius")
            }
            _ => unreachable!(),
        };
        let series = chart.add_general_series(options).unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        chart.set_general_transition_duration_ms(200.0).unwrap();
        chart.set_general_animation_time_ms(1_000.0);
        chart
            .replace_general_xy_dataset(dataset, input(vec![6.0, 8.0, 10.0]))
            .unwrap();
        assert!(chart.general_transition_active(), "{kind:?}");
        chart.set_general_animation_time_ms(1_100.0);
        assert_eq!(
            chart.general_dataset(dataset).unwrap().y(),
            &[6.0, 8.0, 10.0]
        );
        let mut geometry = None;
        if kind == GeneralSeriesKind::Radar {
            chart.with_general_radar_polygon(
                chart.general_series(series).unwrap(),
                |polygon, _, _| {
                    let vertex = polygon.vertex(0).unwrap();
                    let center = polygon.plot().center();
                    let normalized = (vertex.0 - center.0).hypot(vertex.1 - center.1)
                        / polygon.plot().outer_radius();
                    geometry = Some((normalized, polygon.plot().point(0.0, 0.2).unwrap()));
                },
            );
        } else {
            chart.visit_general_polar_value_sectors(
                chart.general_series(series).unwrap(),
                |sector| {
                    if sector.row == 0 {
                        let (_, normalized) = sector.sector.radial_bounds();
                        geometry = Some((
                            normalized,
                            sector
                                .sector
                                .plot()
                                .point(sector.sector.mid_angle(), 0.2)
                                .unwrap(),
                        ));
                    }
                },
            );
        }
        let (normalized, inside) = geometry.expect("midpoint geometry");
        let expected = if kind == GeneralSeriesKind::PolarArea {
            0.4_f64.sqrt()
        } else {
            0.4
        };
        assert!((normalized - expected).abs() < 1e-10, "{kind:?}");
        assert_eq!(
            chart
                .general_hit_test(0, inside.0, inside.1, crate::GeneralHitMode::Exact)
                .unwrap()
                .row_id,
            GeneralRowIdentity::Explicit(GeneralRowId::Text("a".into())),
            "{kind:?}"
        );
        assert!(
            chart.build_frame().panes[0]
                .main
                .iter()
                .any(|prim| matches!(prim, Prim::MeshFill { .. }))
        );
        chart.set_general_transition_reduced_motion(true);
        assert!(!chart.general_transition_active(), "{kind:?}");
    }
}

#[test]
fn missing_polar_row_enters_on_the_same_frame_and_hit_clock_for_every_family() {
    for kind in [
        GeneralSeriesKind::Pie,
        GeneralSeriesKind::Radar,
        GeneralSeriesKind::PolarArea,
        GeneralSeriesKind::RadialBar,
    ] {
        let mut chart =
            ChartEngine::new_with_initial_domain(400.0, 400.0, 1.0, HorizontalDomain::Polar)
                .unwrap();
        let mut angle = GeneralAxisOptions::new(
            "angle",
            0,
            AxisDimension::Angle,
            GeneralScaleType::AngularCategory,
        );
        angle.visible = false;
        chart.add_general_axis(angle).unwrap();
        let mut radius = GeneralAxisOptions::new(
            "radius",
            0,
            AxisDimension::Radius,
            GeneralScaleType::RadialLinear,
        );
        radius.visible = false;
        radius.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        chart.add_general_axis(radius).unwrap();
        let input = |valid| GeneralXyInput::Category {
            ids: Some(
                ["a", "b", "c"]
                    .into_iter()
                    .map(|id| GeneralRowId::Text(id.into()))
                    .collect(),
            ),
            categories: ["A", "B", "C"].into_iter().map(str::to_owned).collect(),
            category_indices: vec![0, 1, 2],
            y: vec![4.0, 2.0, if valid { 6.0 } else { 99.0 }],
            y_valid: Some(vec![1, 1, u8::from(valid)]),
        };
        let dataset = chart.create_general_xy_dataset(input(false)).unwrap();
        let options = match kind {
            GeneralSeriesKind::Pie => GeneralSeriesOptions::pie(0, dataset, "angle", "radius"),
            GeneralSeriesKind::Radar => GeneralSeriesOptions::radar(0, dataset, "angle", "radius"),
            GeneralSeriesKind::PolarArea => {
                GeneralSeriesOptions::polar_area(0, dataset, "angle", "radius")
            }
            GeneralSeriesKind::RadialBar => {
                GeneralSeriesOptions::radial_bar(0, dataset, "angle", "radius")
            }
            _ => unreachable!(),
        };
        let series = chart.add_general_series(options).unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        let mesh_count = |chart: &mut ChartEngine| {
            chart.build_frame().panes[0]
                .main
                .iter()
                .filter(|prim| matches!(prim, Prim::MeshFill { .. }))
                .count()
        };
        let source_count = mesh_count(&mut chart);
        assert_eq!(
            source_count,
            if kind == GeneralSeriesKind::Radar {
                1
            } else {
                2
            }
        );
        chart.set_general_transition_duration_ms(200.0).unwrap();
        chart.set_general_animation_time_ms(1_000.0);
        chart
            .replace_general_xy_dataset(dataset, input(true))
            .unwrap();
        assert!(chart.general_transition_active(), "{kind:?}");
        assert_eq!(mesh_count(&mut chart), source_count, "{kind:?}");
        chart.set_general_animation_time_ms(1_100.0);
        let presentation = chart.general_geometry_dataset(dataset).unwrap();
        assert_eq!(presentation.y()[2], 3.0, "{kind:?}");
        assert!(presentation.y_is_valid(2), "{kind:?}");
        let mut interior = None;
        if kind == GeneralSeriesKind::Radar {
            chart.with_general_radar_polygon(
                chart.general_series(series).unwrap(),
                |polygon, _, _| {
                    interior = polygon.plot().point(polygon.angle(2).unwrap(), 0.1);
                },
            );
        } else {
            let visit = |sector: crate::general_series::GeneralPolarSectorGeometry| {
                if sector.row == 2 {
                    interior = sector
                        .sector
                        .plot()
                        .point(sector.sector.mid_angle(), sector.sector.mid_radius());
                }
            };
            if kind == GeneralSeriesKind::Pie {
                chart.visit_general_pie_slices(chart.general_series(series).unwrap(), visit);
            } else {
                chart.visit_general_polar_value_sectors(
                    chart.general_series(series).unwrap(),
                    visit,
                );
            }
        }
        let (x, y) = interior.expect("entering sector has an interior point");
        assert_eq!(
            chart
                .general_hit_test(0, x, y, crate::GeneralHitMode::Exact)
                .unwrap()
                .row_id,
            GeneralRowIdentity::Explicit(GeneralRowId::Text("c".into())),
            "{kind:?}",
        );
        chart.set_general_animation_time_ms(1_200.0);
        assert!(!chart.general_transition_active(), "{kind:?}");
        assert_eq!(
            mesh_count(&mut chart),
            if kind == GeneralSeriesKind::Radar {
                1
            } else {
                3
            }
        );
    }
}

#[test]
fn radar_polygon_uses_radial_view_for_frame_and_exact_hits() {
    let mut chart = ChartEngine::new(400.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(true, HorizontalDomain::Polar)
        .unwrap();
    let mut angle = GeneralAxisOptions::new(
        "radar-angle",
        pane,
        AxisDimension::Angle,
        GeneralScaleType::AngularCategory,
    );
    angle.visible = false;
    chart.add_general_axis(angle).unwrap();
    let mut radius = GeneralAxisOptions::new(
        "radar-radius",
        pane,
        AxisDimension::Radius,
        GeneralScaleType::RadialLinear,
    );
    radius.visible = false;
    chart.add_general_axis(radius).unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["North".into(), "East".into(), "South".into(), "West".into()],
            category_indices: vec![0, 1, 2, 3],
            y: vec![4.0, 2.0, 3.0, 1.0],
            y_valid: None,
        })
        .unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::radar(
            pane,
            dataset,
            "radar-angle",
            "radar-radius",
        ))
        .unwrap();
    assert_eq!(
        chart.general_axis_effective_domain("radar-angle"),
        Some(GeneralAxisDomain::Category(vec![
            "North".into(),
            "East".into(),
            "South".into(),
            "West".into(),
        ]))
    );
    assert_eq!(
        chart.general_axis_effective_domain("radar-radius"),
        Some(GeneralAxisDomain::Numeric([0.0, 4.0]))
    );
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let frame = chart.build_frame();
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|prim| matches!(prim, Prim::MeshFill { .. }))
    );
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|prim| matches!(prim, Prim::Polyline { .. }))
    );
    let mut inside = None;
    let mut outside = None;
    chart.with_general_radar_polygon(chart.general_series(series).unwrap(), |polygon, _, _| {
        let plot = polygon.plot();
        inside = plot.point(0.0, 0.4);
        outside = plot.point(std::f64::consts::FRAC_PI_4, 0.95);
    });
    let inside = inside.unwrap();
    let outside = outside.unwrap();
    assert_eq!(
        chart
            .general_hit_test(pane, inside.0, inside.1, crate::GeneralHitMode::Exact)
            .unwrap()
            .series,
        series
    );
    assert!(
        chart
            .general_hit_test(pane, outside.0, outside.1, crate::GeneralHitMode::Exact)
            .is_none()
    );
    assert!(
        chart
            .replace_general_xy_dataset(
                dataset,
                GeneralXyInput::Category {
                    ids: None,
                    categories: vec!["North".into(), "East".into(), "South".into()],
                    category_indices: vec![0, 0, 2],
                    y: vec![1.0, 2.0, 3.0],
                    y_valid: None,
                },
            )
            .is_err()
    );
    assert_eq!(
        chart.general_dataset(dataset).unwrap().y(),
        &[4.0, 2.0, 3.0, 1.0]
    );
}

#[test]
fn polar_area_sectors_share_radial_axis_and_exact_hits() {
    let mut chart = ChartEngine::new(400.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(true, HorizontalDomain::Polar)
        .unwrap();
    let mut angle = GeneralAxisOptions::new(
        "area-angle",
        pane,
        AxisDimension::Angle,
        GeneralScaleType::AngularCategory,
    );
    angle.visible = false;
    chart.add_general_axis(angle).unwrap();
    let mut radius = GeneralAxisOptions::new(
        "area-radius",
        pane,
        AxisDimension::Radius,
        GeneralScaleType::RadialLinear,
    );
    radius.visible = false;
    chart.add_general_axis(radius).unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["A".into(), "B".into(), "C".into(), "D".into()],
            category_indices: vec![0, 1, 2, 3],
            y: vec![4.0, 2.0, 0.0, 3.0],
            y_valid: Some(vec![1, 1, 1, 0]),
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::polar_area(pane, dataset, "area-angle", "area-radius");
    options.polar.inner_radius = 0.2;
    options.polar.outer_radius = 0.9;
    options.polar.pad_angle = 0.05;
    let series = chart.add_general_series(options).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut sectors = Vec::new();
    chart.visit_general_polar_value_sectors(chart.general_series(series).unwrap(), |geometry| {
        sectors.push(geometry);
    });
    assert_eq!(
        sectors.iter().map(|sector| sector.row).collect::<Vec<_>>(),
        [0, 1]
    );
    let frame = chart.build_frame();
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|prim| matches!(prim, Prim::MeshFill { .. }))
    );
    for geometry in &sectors {
        let point = geometry
            .sector
            .plot()
            .point(geometry.sector.mid_angle(), geometry.sector.mid_radius())
            .unwrap();
        assert_eq!(
            chart
                .general_hit_test(pane, point.0, point.1, crate::GeneralHitMode::Exact)
                .unwrap()
                .row,
            geometry.row
        );
    }
    let (cx, cy) = sectors[0].sector.plot().center();
    assert!(
        chart
            .general_hit_test(pane, cx, cy, crate::GeneralHitMode::Exact)
            .is_none()
    );
    assert!(
        chart
            .replace_general_xy_dataset(
                dataset,
                GeneralXyInput::Category {
                    ids: None,
                    categories: vec!["A".into(), "B".into()],
                    category_indices: vec![0, 1],
                    y: vec![1.0, -2.0],
                    y_valid: None,
                },
            )
            .is_err()
    );
    assert_eq!(
        chart.general_dataset(dataset).unwrap().y(),
        &[4.0, 2.0, 0.0, 3.0]
    );

    // Half the value fills half the radial length for bars, but half the annular
    // area for polar area. The shared sector is also the exact hit boundary.
    let mut bar_options =
        GeneralSeriesOptions::radial_bar(pane, dataset, "area-angle", "area-radius");
    bar_options.polar.inner_radius = 0.2;
    bar_options.polar.outer_radius = 0.9;
    bar_options.polar.pad_angle = 0.05;
    let bar = chart.add_general_series(bar_options).unwrap();
    let legend = chart.general_legend_snapshot(Some(pane));
    assert_eq!(legend.items.len(), 2);
    assert_eq!(
        legend.items[0]
            .slices
            .iter()
            .map(|slice| slice.row)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(
        legend.items[1]
            .slices
            .iter()
            .map(|slice| slice.row)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    sectors.clear();
    chart.visit_general_polar_value_sectors(chart.general_series(series).unwrap(), |geometry| {
        sectors.push(geometry);
    });
    let mut bars = Vec::new();
    chart.visit_general_polar_value_sectors(chart.general_series(bar).unwrap(), |geometry| {
        bars.push(geometry);
    });
    assert_eq!(
        bars.iter().map(|sector| sector.row).collect::<Vec<_>>(),
        [0, 1]
    );
    let mid_angle = sectors[1].sector.mid_angle();
    let plot = sectors[1].sector.plot();
    let between = plot.point(mid_angle, 0.6).unwrap();
    assert!(sectors[1].sector.contains(between.0, between.1));
    assert!(!bars[1].sector.contains(between.0, between.1));
    assert_eq!(
        chart
            .general_hit_test(pane, between.0, between.1, crate::GeneralHitMode::Exact)
            .unwrap()
            .series,
        series
    );
    let inside_bar = plot.point(mid_angle, 0.4).unwrap();
    assert_eq!(
        chart
            .general_hit_test(
                pane,
                inside_bar.0,
                inside_bar.1,
                crate::GeneralHitMode::Exact
            )
            .unwrap()
            .series,
        bar
    );

    let mut reversed = GeneralAxisOptions::new(
        "area-angle",
        pane,
        AxisDimension::Angle,
        GeneralScaleType::AngularCategory,
    );
    reversed.visible = false;
    reversed.reverse = true;
    chart.update_general_axis_options(reversed).unwrap();
    let mut reversed_sectors = Vec::new();
    chart.visit_general_polar_value_sectors(chart.general_series(series).unwrap(), |geometry| {
        reversed_sectors.push(geometry);
    });
    assert_eq!(reversed_sectors.len(), 2);
    assert!(
        (reversed_sectors[1]
            .sector
            .mid_angle()
            .rem_euclid(std::f64::consts::TAU)
            - 3.0 * std::f64::consts::FRAC_PI_2)
            .abs()
            < 1e-12
    );
}

#[test]
fn polar_value_hit_matches_shared_sectors_after_sweep_and_axis_changes() {
    let mut chart =
        ChartEngine::new_with_initial_domain(400.0, 400.0, 1.0, HorizontalDomain::Polar).unwrap();
    let mut angle = GeneralAxisOptions::new(
        "angle",
        0,
        AxisDimension::Angle,
        GeneralScaleType::AngularCategory,
    );
    angle.visible = false;
    chart.add_general_axis(angle.clone()).unwrap();
    let mut radius = GeneralAxisOptions::new(
        "radius",
        0,
        AxisDimension::Radius,
        GeneralScaleType::RadialLinear,
    );
    radius.visible = false;
    chart.add_general_axis(radius).unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: ["A", "B", "C", "D", "E"].map(str::to_owned).to_vec(),
            category_indices: vec![0, 1, 2, 3, 4],
            y: vec![5.0, 2.0, 4.0, 0.0, 3.0],
            y_valid: Some(vec![1, 1, 1, 1, 0]),
        })
        .unwrap();
    let mut area_options = GeneralSeriesOptions::polar_area(0, dataset, "angle", "radius");
    area_options.polar.inner_radius = 0.2;
    area_options.polar.outer_radius = 0.9;
    area_options.polar.pad_angle = 0.03;
    let area = chart.add_general_series(area_options.clone()).unwrap();
    let mut bar_options = GeneralSeriesOptions::radial_bar(0, dataset, "angle", "radius");
    bar_options.polar = area_options.polar;
    let bar = chart.add_general_series(bar_options.clone()).unwrap();

    for (start, end, reversed) in [
        (0.0, std::f64::consts::TAU, false),
        (0.4, 3.1, false),
        (0.4, 3.1, true),
    ] {
        area_options.polar.start_angle = start;
        area_options.polar.end_angle = end;
        bar_options.polar = area_options.polar;
        chart
            .update_general_series_options(area, area_options.clone())
            .unwrap();
        chart
            .update_general_series_options(bar, bar_options.clone())
            .unwrap();
        angle.reverse = reversed;
        chart.update_general_axis_options(angle.clone()).unwrap();
        chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
        let mut area_sectors = Vec::new();
        let mut bar_sectors = Vec::new();
        chart.visit_general_polar_value_sectors(chart.general_series(area).unwrap(), |item| {
            area_sectors.push(item)
        });
        chart.visit_general_polar_value_sectors(chart.general_series(bar).unwrap(), |item| {
            bar_sectors.push(item)
        });
        let plot = area_sectors[0].sector.plot();
        for angle_index in 0..180 {
            for radius_index in 1..20 {
                let (x, y) = plot
                    .point(
                        std::f64::consts::TAU * angle_index as f64 / 180.0,
                        radius_index as f64 / 20.0,
                    )
                    .unwrap();
                let expected = bar_sectors
                    .iter()
                    .find(|item| item.sector.contains(x, y))
                    .map(|item| (bar, item.row))
                    .or_else(|| {
                        area_sectors
                            .iter()
                            .find(|item| item.sector.contains(x, y))
                            .map(|item| (area, item.row))
                    });
                let actual = chart
                    .general_hit_test(0, x, y, crate::GeneralHitMode::Exact)
                    .map(|hit| (hit.series, hit.row));
                assert_eq!(
                    actual, expected,
                    "angle={angle_index} radius={radius_index} reversed={reversed}"
                );
            }
        }
    }

    let mut old_first = None;
    chart.visit_general_polar_value_sectors(chart.general_series(bar).unwrap(), |item| {
        if item.row == 0 {
            old_first = item
                .sector
                .plot()
                .point(item.sector.mid_angle(), item.sector.mid_radius());
        }
    });
    let (x, y) = old_first.unwrap();
    assert!(
        chart
            .general_hit_test(0, x, y, crate::GeneralHitMode::Exact)
            .is_some()
    );
    chart
        .replace_general_xy_dataset(
            dataset,
            GeneralXyInput::Category {
                ids: None,
                categories: ["A", "B", "C", "D", "E"].map(str::to_owned).to_vec(),
                category_indices: vec![0, 1, 2, 3, 4],
                y: vec![0.0, 2.0, 4.0, 0.0, 3.0],
                y_valid: Some(vec![1, 1, 1, 1, 0]),
            },
        )
        .unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    assert!(
        chart
            .general_hit_test(0, x, y, crate::GeneralHitMode::Exact)
            .is_none()
    );
    let bytes = chart.memory_usage().general_series_capacity_bytes;
    for _ in 0..1_000 {
        let _ = chart.general_hit_test(0, x, y, crate::GeneralHitMode::Exact);
    }
    assert_eq!(chart.memory_usage().general_series_capacity_bytes, bytes);
}

#[test]
fn temporal_general_axis_emits_utc_ticks_and_supports_atomic_runtime_view() {
    const JAN_1_2026: i64 = 1_767_225_600_000;
    const JAN_2_2026: i64 = 1_767_312_000_000;
    const JAN_3_2026: i64 = 1_767_398_400_000;

    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(true, HorizontalDomain::Temporal)
        .unwrap();
    let mut x = GeneralAxisOptions::new("time", pane, AxisDimension::X, GeneralScaleType::Temporal);
    x.domain = GeneralAxisDomain::Temporal([JAN_1_2026, JAN_3_2026]);
    chart.add_general_axis(x).unwrap();
    let mut y = GeneralAxisOptions::new("value", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
    chart.add_general_axis(y).unwrap();
    let mut short = std::array::from_fn(|index| format!("M{}", index + 1));
    short[0] = "Ene".into();
    let long = std::array::from_fn(|index| format!("Month {}", index + 1));
    chart.set_month_names(short, long);

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let frame = chart.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    assert!(
        frame
            .labels
            .iter()
            .any(|label| label.text == "1 Ene" || label.text == "2 Ene")
    );
    assert!(frame.labels.iter().any(|label| label.text.contains(':')));

    chart
        .zoom_general_axis("time", 2.0, JAN_2_2026 as f64)
        .unwrap();
    assert_eq!(
        chart.general_axis_effective_domain("time"),
        Some(GeneralAxisDomain::Temporal([
            1_767_268_800_000,
            1_767_355_200_000
        ]))
    );
    chart.pan_general_axis("time", 0.25).unwrap();
    let panned = GeneralAxisDomain::Temporal([1_767_290_400_000, 1_767_376_800_000]);
    assert_eq!(
        chart.general_axis_effective_domain("time"),
        Some(panned.clone())
    );

    assert!(
        chart
            .zoom_general_axis("time", 2.0, JAN_2_2026 as f64 + 0.5)
            .is_err()
    );
    assert!(
        chart
            .zoom_general_axis("time", 1.0e20, JAN_2_2026 as f64)
            .is_err()
    );
    assert_eq!(chart.general_axis_effective_domain("time"), Some(panned));

    assert!(chart.reset_general_axis_view("time"));
    assert_eq!(
        chart.general_axis_effective_domain("time"),
        Some(GeneralAxisDomain::Temporal([JAN_1_2026, JAN_3_2026]))
    );
}

#[test]
fn complete_finite_numeric_domain_builds_geometry_ticks_and_runtime_views() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    let mut x = GeneralAxisOptions::new(
        "extreme-x",
        pane,
        AxisDimension::X,
        GeneralScaleType::Linear,
    );
    x.domain = GeneralAxisDomain::Numeric([-f64::MAX, f64::MAX]);
    chart.add_general_axis(x).unwrap();
    let mut y = GeneralAxisOptions::new(
        "extreme-y",
        pane,
        AxisDimension::Y,
        GeneralScaleType::Linear,
    );
    y.domain = GeneralAxisDomain::Numeric([-1.0, 1.0]);
    chart.add_general_axis(y).unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: vec![-f64::MAX, 0.0, f64::MAX],
            y: vec![-1.0, 0.0, 1.0],
            y_valid: None,
        })
        .unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::scatter(
            pane,
            dataset,
            "extreme-x",
            "extreme-y",
        ))
        .unwrap();

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let plot = chart.general_plot_rect(pane).unwrap();
    let mut geometry = Vec::new();
    chart.visit_general_scatter_points(chart.general_series(series).unwrap(), |point| {
        geometry.push(point)
    });
    assert_eq!(geometry.len(), 3);
    assert!(
        geometry
            .iter()
            .all(|point| point.x.is_finite() && point.y.is_finite())
    );
    assert!((geometry[0].x - 0.0).abs() < 1e-9);
    assert!((geometry[1].x - plot.width * 0.5).abs() < 1e-9);
    assert!((geometry[2].x - plot.width).abs() < 1e-9);

    let axis = chart.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    assert!(
        axis.labels
            .iter()
            .all(|label| label.x.is_finite() && label.y.is_finite())
    );
    assert!(axis.labels.iter().any(|label| label.text.contains('e')));
    let frame = chart.build_frame();
    assert!(
        frame.panes[pane]
            .under
            .iter()
            .any(|primitive| { matches!(primitive, Prim::VLine { .. }) })
    );

    chart.zoom_general_axis("extreme-x", 2.0, 0.0).unwrap();
    let Some(GeneralAxisDomain::Numeric(zoomed)) = chart.general_axis_effective_domain("extreme-x")
    else {
        panic!("numeric zoom must retain a numeric domain");
    };
    assert!((zoomed[0] / f64::MAX + 0.5).abs() < f64::EPSILON);
    assert!((zoomed[1] / f64::MAX - 0.5).abs() < f64::EPSILON);

    chart.pan_general_axis("extreme-x", 0.5).unwrap();
    let Some(GeneralAxisDomain::Numeric(panned)) = chart.general_axis_effective_domain("extreme-x")
    else {
        panic!("numeric pan must retain a numeric domain");
    };
    assert_eq!(panned[0], 0.0);
    assert!((panned[1] / f64::MAX - 1.0).abs() < f64::EPSILON);
}

#[test]
fn numeric_domain_padding_expands_the_base_domain_and_rejects_incompatible_axes() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    let mut x =
        GeneralAxisOptions::new("padded-x", pane, AxisDimension::X, GeneralScaleType::Linear);
    x.domain = GeneralAxisDomain::Numeric([10.0, 20.0]);
    x.domain_padding = 0.1;
    chart.add_general_axis(x).unwrap();
    assert_eq!(
        chart.general_axis_effective_domain("padded-x"),
        Some(GeneralAxisDomain::Numeric([9.0, 21.0]))
    );

    chart.zoom_general_axis("padded-x", 2.0, 15.0).unwrap();
    assert_eq!(
        chart.general_axis_effective_domain("padded-x"),
        Some(GeneralAxisDomain::Numeric([12.0, 18.0]))
    );
    assert!(chart.reset_general_axis_view("padded-x"));
    assert_eq!(
        chart.general_axis_effective_domain("padded-x"),
        Some(GeneralAxisDomain::Numeric([9.0, 21.0]))
    );
    let temporal_pane = chart
        .add_pane_with_domain(true, HorizontalDomain::Temporal)
        .unwrap();
    let mut temporal = GeneralAxisOptions::new(
        "time",
        temporal_pane,
        AxisDimension::X,
        GeneralScaleType::Temporal,
    );
    temporal.domain = GeneralAxisDomain::Temporal([0, 1_000]);
    temporal.domain_padding = 0.1;
    assert!(chart.add_general_axis(temporal).is_err());
    assert!(chart.general_axis("time").is_none());
}

#[test]
fn multiple_reversed_axes_keep_bound_geometry_hits_titles_and_strip_order() {
    let mut chart = ChartEngine::new(720.0, 420.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    for (id, dimension, position, domain, reverse, title) in [
        (
            "x-bottom",
            AxisDimension::X,
            AxisPosition::Bottom,
            [0.0, 10.0],
            false,
            "Bottom X",
        ),
        (
            "x-top",
            AxisDimension::X,
            AxisPosition::Top,
            [0.0, 100.0],
            true,
            "Top X",
        ),
        (
            "y-left",
            AxisDimension::Y,
            AxisPosition::Left,
            [0.0, 10.0],
            false,
            "Left Y",
        ),
        (
            "y-right",
            AxisDimension::Y,
            AxisPosition::Right,
            [0.0, 100.0],
            true,
            "Right Y",
        ),
    ] {
        let mut axis = GeneralAxisOptions::new(id, pane, dimension, GeneralScaleType::Linear);
        axis.position = Some(position);
        axis.domain = GeneralAxisDomain::Numeric(domain);
        axis.reverse = reverse;
        axis.title = Some(title.into());
        chart.add_general_axis(axis).unwrap();
    }
    let left_data = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: Some(vec![GeneralRowId::Text("left".into())]),
            x: vec![2.0],
            y: vec![2.0],
            y_valid: None,
        })
        .unwrap();
    let right_data = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: Some(vec![GeneralRowId::Text("right".into())]),
            x: vec![20.0],
            y: vec![20.0],
            y_valid: None,
        })
        .unwrap();
    let left = chart
        .add_general_series(GeneralSeriesOptions::scatter(
            pane, left_data, "x-bottom", "y-left",
        ))
        .unwrap();
    let right = chart
        .add_general_series(GeneralSeriesOptions::scatter(
            pane, right_data, "x-top", "y-right",
        ))
        .unwrap();

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let plot = chart.general_plot_rect(pane).unwrap();
    let point = |chart: &mut ChartEngine, series| {
        let mut points = Vec::new();
        chart.visit_general_scatter_points(chart.general_series(series).unwrap(), |point| {
            points.push(point)
        });
        points[0]
    };
    let left_point = point(&mut chart, left);
    let right_point = point(&mut chart, right);
    assert!((left_point.x - plot.width * 0.2).abs() < 1e-9);
    assert!((left_point.y - (plot.y + plot.height * 0.8)).abs() < 1e-9);
    assert!((right_point.x - plot.width * 0.8).abs() < 1e-9);
    assert!((right_point.y - (plot.y + plot.height * 0.2)).abs() < 1e-9);
    assert_eq!(
        chart
            .general_hit_test(
                pane,
                right_point.x,
                right_point.y,
                crate::GeneralHitMode::Exact,
            )
            .unwrap()
            .series,
        right
    );

    let frame = chart.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    for title in ["Bottom X", "Top X"] {
        assert!(frame.labels.iter().any(|label| label.text == title));
    }
    for title in ["Left Y", "Right Y"] {
        assert!(frame.rotated_labels.iter().any(|label| label.text == title));
    }

    let width_with_right = chart.axis_w;
    assert!(chart.set_general_axis_visible("y-right", false));
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    assert!(chart.axis_w < width_with_right);
    assert_eq!(chart.general_series(right).unwrap().x_axis_id(), "x-top");
    assert_eq!(chart.general_series(right).unwrap().y_axis_id(), "y-right");
}

#[test]
fn explicit_numeric_ticks_drive_labels_grids_clipping_and_live_updates() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    chart
        .apply_options(r#"{"grid":{"vertLines":{"visible":true},"horzLines":{"visible":true}}}"#)
        .unwrap();
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    let mut x =
        GeneralAxisOptions::new("ticks-x", pane, AxisDimension::X, GeneralScaleType::Linear);
    x.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
    x.zero_line = false;
    x.ticks = Some(vec![
        GeneralAxisTick::Numeric {
            value: 0.0,
            label: Some("Start".into()),
        },
        GeneralAxisTick::Numeric {
            value: 5.0,
            label: None,
        },
        GeneralAxisTick::Numeric {
            value: 20.0,
            label: Some("Outside".into()),
        },
    ]);
    chart.add_general_axis(x.clone()).unwrap();
    let mut y =
        GeneralAxisOptions::new("ticks-y", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y.domain = GeneralAxisDomain::Numeric([0.0, 1.0]);
    y.grid_visible = false;
    y.zero_line = false;
    chart.add_general_axis(y).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

    let axis = chart.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    assert!(axis.labels.iter().any(|label| label.text == "Start"));
    assert!(axis.labels.iter().any(|label| label.text == "5"));
    assert!(!axis.labels.iter().any(|label| label.text == "Outside"));
    let frame = chart.build_frame();
    // The tick at 0 lies on the left Y axis line, which already marks that edge.
    assert_eq!(
        frame.panes[pane]
            .under
            .iter()
            .filter(|primitive| matches!(primitive, Prim::VLine { .. }))
            .count(),
        1
    );

    let handle = chart.general_axis("ticks-x").unwrap().handle();
    x.ticks = Some(vec![GeneralAxisTick::Numeric {
        value: 10.0,
        label: Some("End".into()),
    }]);
    chart.update_general_axis_options(x).unwrap();
    assert_eq!(chart.general_axis("ticks-x").unwrap().handle(), handle);
    let updated = chart.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    assert!(updated.labels.iter().any(|label| label.text == "End"));
    assert!(!updated.labels.iter().any(|label| label.text == "Start"));
}

#[test]
fn explicit_category_and_temporal_ticks_use_typed_values_and_labels() {
    let mut category = ChartEngine::new(480.0, 280.0, 1.0);
    let pane = category
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Point,
            },
        )
        .unwrap();
    let mut axis = GeneralAxisOptions::new(
        "category-x",
        pane,
        AxisDimension::X,
        GeneralScaleType::Point,
    );
    axis.domain = GeneralAxisDomain::Category(vec!["A".into(), "B".into(), "C".into()]);
    axis.ticks = Some(vec![
        GeneralAxisTick::Category {
            value: "B".into(),
            label: Some("Beta".into()),
        },
        GeneralAxisTick::Category {
            value: "missing".into(),
            label: Some("Missing".into()),
        },
    ]);
    category.add_general_axis(axis).unwrap();
    category.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let frame = category.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    assert!(frame.labels.iter().any(|label| label.text == "Beta"));
    assert!(!frame.labels.iter().any(|label| label.text == "A"));
    assert!(!frame.labels.iter().any(|label| label.text == "Missing"));

    const JAN_1_2026: i64 = 1_767_225_600_000;
    const JAN_2_2026: i64 = 1_767_312_000_000;
    const JAN_3_2026: i64 = 1_767_398_400_000;
    let mut temporal = ChartEngine::new(480.0, 280.0, 1.0);
    let pane = temporal
        .add_pane_with_domain(true, HorizontalDomain::Temporal)
        .unwrap();
    let mut axis = GeneralAxisOptions::new(
        "temporal-x",
        pane,
        AxisDimension::X,
        GeneralScaleType::Temporal,
    );
    axis.domain = GeneralAxisDomain::Temporal([JAN_1_2026, JAN_3_2026]);
    axis.ticks = Some(vec![
        GeneralAxisTick::Temporal {
            value: JAN_1_2026,
            label: Some("Open".into()),
        },
        GeneralAxisTick::Temporal {
            value: JAN_2_2026,
            label: None,
        },
    ]);
    temporal.add_general_axis(axis).unwrap();
    temporal.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let frame = temporal.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    assert!(frame.labels.iter().any(|label| label.text == "Open"));
    assert!(frame.labels.iter().any(|label| label.text == "2 Jan"));
}

#[test]
fn general_axis_lines_are_whole_device_pixels_and_never_doubled_by_edge_rules() {
    for dpr in [1.0, 1.25, 1.5, 2.0] {
        let mut chart = ChartEngine::new_with_initial_domain(
            637.0,
            401.0,
            dpr,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
        chart
            .apply_options(
                r#"{"grid":{"vertLines":{"visible":true},"horzLines":{"visible":true}}}"#,
            )
            .unwrap();
        let pane = 0;
        let mut x = GeneralAxisOptions::new("x", pane, AxisDimension::X, GeneralScaleType::Linear);
        x.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        chart.add_general_axis(x).unwrap();
        let mut y = GeneralAxisOptions::new("y", pane, AxisDimension::Y, GeneralScaleType::Linear);
        y.domain = GeneralAxisDomain::Numeric([0.0, 80.0]);
        chart.add_general_axis(y).unwrap();
        chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

        let plot = chart.general_plot_rect(pane).unwrap();
        let bottom = ((plot.y + plot.height) * dpr).round() as i32;
        let frame = chart.build_frame();
        let under = &frame.panes[pane].under;
        assert!(
            under.iter().any(|p| matches!(p, Prim::VLine { .. })),
            "dpr {dpr}"
        );
        assert!(
            under.iter().any(|p| matches!(p, Prim::HLine { .. })),
            "dpr {dpr}"
        );
        for primitive in under {
            match primitive {
                Prim::VLine { x, .. } => {
                    assert!(x.abs() > 1, "dpr {dpr}: rule beside Y axis at {x}")
                }
                Prim::HLine { y, .. } => {
                    assert!(
                        (y - bottom).abs() > 1,
                        "dpr {dpr}: rule beside X axis at {y}"
                    )
                }
                _ => {}
            }
        }

        let axis = chart.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
        let mut prims = Vec::new();
        chart.build_axis_primitives_into(&axis, &mut prims);
        let border = aeris_charts_core::style::border_width_device_px(dpr) as i32;
        let rules = prims
            .iter()
            .filter_map(|p| match p {
                Prim::Rect { rect, .. } if rect.w <= 2 && rect.h > 20 => Some(rect.w),
                Prim::Rect { rect, .. } if rect.h <= 2 && rect.w > 20 => Some(rect.h),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(rules.len(), 2, "dpr {dpr}: {rules:?}");
        assert!(rules.iter().all(|w| *w == border), "dpr {dpr}: {rules:?}");
    }
}

#[test]
fn general_grid_and_zero_lines_render_below_data_and_follow_axis_policy() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    chart
        .apply_options(
            r##"{
                "grid": {
                    "vertLines": { "visible": true, "color": "#010203", "style": 2 },
                    "horzLines": { "visible": true, "color": "#040506", "style": 1 }
                },
                "rightPriceScale": { "borderColor": "#070809" }
            }"##,
        )
        .unwrap();
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    let mut x = GeneralAxisOptions::new("grid-x", pane, AxisDimension::X, GeneralScaleType::Linear);
    x.domain = GeneralAxisDomain::Numeric([-5.0, 5.0]);
    chart.add_general_axis(x.clone()).unwrap();
    let mut y = GeneralAxisOptions::new("grid-y", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y.domain = GeneralAxisDomain::Numeric([-10.0, 10.0]);
    chart.add_general_axis(y.clone()).unwrap();

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let frame = chart.build_frame();
    let under = &frame.panes[pane].under;
    let vertical = Color::parse_css("#010203").unwrap();
    let horizontal = Color::parse_css("#040506").unwrap();
    let zero = Color::parse_css("#070809").unwrap();
    assert!(under.iter().any(|primitive| {
        matches!(primitive, Prim::VLine { color, style: LineStyle::Dashed, .. } if *color == vertical)
    }));
    assert!(under.iter().any(|primitive| {
        matches!(primitive, Prim::HLine { color, style: LineStyle::Dotted, .. } if *color == horizontal)
    }));
    assert_eq!(
        under
            .iter()
            .filter(|primitive| {
                matches!(primitive, Prim::VLine { color, style: LineStyle::Solid, .. } if *color == zero)
                    || matches!(primitive, Prim::HLine { color, style: LineStyle::Solid, .. } if *color == zero)
            })
            .count(),
        2
    );
    let zero_x = under
        .iter()
        .find_map(|primitive| match primitive {
            Prim::VLine { x, color, .. } if *color == zero => Some(*x),
            _ => None,
        })
        .unwrap();
    let zero_y = under
        .iter()
        .find_map(|primitive| match primitive {
            Prim::HLine { y, color, .. } if *color == zero => Some(*y),
            _ => None,
        })
        .unwrap();
    assert!(under.iter().all(|primitive| {
        !matches!(primitive, Prim::VLine { x, color, .. } if *x == zero_x && *color == vertical)
            && !matches!(primitive, Prim::HLine { y, color, .. } if *y == zero_y && *color == horizontal)
    }));

    chart
        .apply_options(r#"{"grid":{"vertLines":{"visible":false}}}"#)
        .unwrap();
    let chart_vertical_disabled = chart.build_frame();
    assert!(
        chart_vertical_disabled.panes[pane]
            .under
            .iter()
            .all(|primitive| !matches!(primitive, Prim::VLine { color, .. } if *color == vertical))
    );
    assert!(
        chart_vertical_disabled.panes[pane]
            .under
            .iter()
            .any(|primitive| matches!(primitive, Prim::VLine { color, .. } if *color == zero))
    );
    chart
        .apply_options(r#"{"grid":{"vertLines":{"visible":true}}}"#)
        .unwrap();

    y.grid_visible = false;
    y.zero_line = false;
    chart.update_general_axis_options(y).unwrap();
    let y_disabled = chart.build_frame();
    assert!(y_disabled.panes[pane]
        .under
        .iter()
        .all(|primitive| !matches!(primitive, Prim::HLine { color, .. } if *color == horizontal || *color == zero)));
    assert!(y_disabled.panes[pane]
        .under
        .iter()
        .any(|primitive| matches!(primitive, Prim::VLine { color, .. } if *color == vertical || *color == zero)));

    x.grid_visible = false;
    x.zero_line = false;
    chart.update_general_axis_options(x).unwrap();
    let disabled = chart.build_frame();
    assert!(disabled.panes[pane].under.iter().all(|primitive| {
        !matches!(primitive, Prim::VLine { color, .. } | Prim::HLine { color, .. }
            if *color == vertical || *color == horizontal || *color == zero)
    }));
}

#[test]
fn general_legend_snapshot_preserves_series_order_visibility_filtering_and_removal() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane_a = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    let pane_b = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    for (pane, prefix) in [(pane_a, "a"), (pane_b, "b")] {
        chart
            .add_general_axis(GeneralAxisOptions::new(
                format!("{prefix}-x"),
                pane,
                AxisDimension::X,
                GeneralScaleType::Linear,
            ))
            .unwrap();
        chart
            .add_general_axis(GeneralAxisOptions::new(
                format!("{prefix}-y"),
                pane,
                AxisDimension::Y,
                GeneralScaleType::Linear,
            ))
            .unwrap();
    }
    let datasets = [4.0, 3.0, 4.0].map(|value| {
        chart
            .create_general_xy_dataset(GeneralXyInput::Numeric {
                ids: None,
                x: vec![1.0, 2.0],
                y: vec![value, value],
                y_valid: None,
            })
            .unwrap()
    });

    let mut first = GeneralSeriesOptions::xy_line(pane_a, datasets[0], "a-x", "a-y");
    first.title = "Revenue".into();
    first.color = Some("#123456".into());
    let first = chart.add_general_series(first).unwrap();

    let mut hidden = GeneralSeriesOptions::scatter(pane_a, datasets[1], "a-x", "a-y");
    hidden.title = "Hidden samples".into();
    hidden.visible = false;
    let hidden = chart.add_general_series(hidden).unwrap();

    let mut third = GeneralSeriesOptions::xy_area(pane_b, datasets[2], "b-x", "b-y");
    third.title = "Margin".into();
    third.color = Some("#abcdef".into());
    let third = chart.add_general_series(third).unwrap();

    let all = chart.general_legend_snapshot(None);
    assert_eq!(
        all.items
            .iter()
            .map(|item| (
                item.series,
                item.pane,
                item.kind,
                item.title.as_str(),
                item.visible,
            ))
            .collect::<Vec<_>>(),
        vec![
            (first, pane_a, GeneralSeriesKind::XyLine, "Revenue", true),
            (
                hidden,
                pane_a,
                GeneralSeriesKind::Scatter,
                "Hidden samples",
                false,
            ),
            (third, pane_b, GeneralSeriesKind::XyArea, "Margin", true),
        ]
    );
    assert_eq!(all.items[0].color.as_deref(), Some("#123456"));
    assert_eq!(all.items[1].color, None);
    assert_eq!(all.items[2].color.as_deref(), Some("#abcdef"));

    let pane_only = chart.general_legend_snapshot(Some(pane_a));
    assert_eq!(
        pane_only
            .items
            .iter()
            .map(|item| item.series)
            .collect::<Vec<_>>(),
        vec![first, hidden]
    );
    assert!(
        chart
            .general_legend_snapshot(Some(usize::MAX))
            .items
            .is_empty()
    );

    assert_eq!(chart.general_series_order(None), vec![first, hidden, third]);
    assert!(chart.set_general_series_order(Some(pane_a), vec![hidden, first]));
    assert_eq!(chart.general_series_order(None), vec![hidden, first, third]);
    assert!(!chart.set_general_series_order(Some(pane_a), vec![first]));
    assert_eq!(
        chart.general_series_order(Some(pane_a)),
        vec![hidden, first]
    );

    let mut rebound = GeneralSeriesOptions::xy_line(pane_b, datasets[0], "b-x", "b-y");
    rebound.title = "Rebound revenue".into();
    rebound.color = Some("#654321".into());
    rebound.point_markers = true;
    rebound.point_radius = 7.0;
    rebound.line_width = 4.0;
    rebound.line_style = GeneralLineStyle::Dotted;
    chart.update_general_series_options(first, rebound).unwrap();
    let rebound = chart.general_series(first).unwrap();
    assert_eq!(chart.general_series_pane_index(first), Some(pane_b));
    assert_eq!(rebound.x_axis_id(), "b-x");
    assert_eq!(rebound.y_axis_id(), "b-y");
    assert_eq!(rebound.dataset(), datasets[0]);
    assert_eq!(rebound.title(), "Rebound revenue");
    assert!(rebound.point_markers());
    assert_eq!(rebound.point_radius(), 7.0);
    assert_eq!(rebound.line_width(), 4.0);
    assert_eq!(rebound.line_style(), GeneralLineStyle::Dotted);

    chart
        .add_general_axis(GeneralAxisOptions::new(
            "b-log",
            pane_b,
            AxisDimension::Y,
            GeneralScaleType::Logarithmic,
        ))
        .unwrap();
    let rejected = GeneralSeriesOptions::xy_line(pane_b, datasets[0], "b-x", "b-log");
    assert!(
        chart
            .update_general_series_options(first, rejected)
            .is_err()
    );
    assert_eq!(chart.general_series(first).unwrap().y_axis_id(), "b-y");

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut overlap = None;
    chart.visit_general_path_points(chart.general_series(first).unwrap(), |point| {
        overlap.get_or_insert((point.x, point.y));
    });
    let overlap = overlap.unwrap();
    assert_eq!(
        chart
            .general_hit_test(pane_b, overlap.0, overlap.1, crate::GeneralHitMode::Exact)
            .unwrap()
            .series,
        third
    );

    assert!(chart.set_general_series_order(Some(pane_b), vec![third, first]));
    assert_eq!(chart.general_series_order(None), vec![hidden, third, first]);
    assert_eq!(
        chart
            .general_hit_test(pane_b, overlap.0, overlap.1, crate::GeneralHitMode::Exact)
            .unwrap()
            .series,
        first
    );
    assert_eq!(
        chart
            .general_legend_snapshot(Some(pane_b))
            .items
            .iter()
            .map(|item| item.series)
            .collect::<Vec<_>>(),
        vec![third, first]
    );

    assert!(chart.remove_general_series(hidden));
    assert_eq!(
        chart
            .general_legend_snapshot(None)
            .items
            .iter()
            .map(|item| item.series)
            .collect::<Vec<_>>(),
        vec![third, first]
    );
}

#[test]
fn dormant_path_radius_survives_marker_toggle_and_invalid_patch_is_atomic() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: Some(vec![GeneralRowId::Text("first".into())]),
            categories: vec!["First".into()],
            category_indices: vec![0],
            y: vec![5.0],
            y_valid: None,
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::xy_line(pane, dataset, "x", "y");
    options.point_markers = true;
    options.point_radius = 7.0;
    let series = chart.add_general_series(options.clone()).unwrap();
    let before = chart.export_state_json().unwrap();
    options.point_markers = false;
    chart
        .update_general_series_options(series, options.clone())
        .unwrap();
    assert_eq!(chart.general_series(series).unwrap().point_radius(), 7.0);
    assert!(!chart.general_series(series).unwrap().point_markers());
    assert_eq!(chart.general_series(series).unwrap().dataset(), dataset);
    assert_eq!(chart.general_series_order(Some(pane)), vec![series]);
    options.point_markers = true;
    chart
        .update_general_series_options(series, options.clone())
        .unwrap();
    assert_eq!(chart.export_state_json().unwrap(), before);
    options.point_radius = f64::INFINITY;
    assert!(
        chart
            .update_general_series_options(series, options)
            .is_err()
    );
    assert_eq!(chart.export_state_json().unwrap(), before);
}

#[test]
fn stacked_area_retains_dormant_baseline_until_unstacked() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
        chart
            .add_general_axis(GeneralAxisOptions::new(
                id,
                pane,
                dimension,
                GeneralScaleType::Linear,
            ))
            .unwrap();
    }
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: Some(vec![
                GeneralRowId::Text("a".into()),
                GeneralRowId::Text("b".into()),
            ]),
            x: vec![0.0, 1.0],
            y: vec![2.0, 4.0],
            y_valid: None,
        })
        .unwrap();
    for baseline in [Some(1.5), None] {
        let mut options = GeneralSeriesOptions::xy_area(pane, dataset, "x", "y");
        options.baseline_value = baseline;
        if baseline.is_none() {
            options.baseline_policy = GeneralAreaBaseline::DomainMax;
        }
        let series = chart.add_general_series(options.clone()).unwrap();
        chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
        assert!(
            chart
                .general_path_baseline_y(chart.general_series(series).unwrap())
                .is_some()
        );
        let before = chart.export_state_json().unwrap();
        options.stack_id = Some("stack".into());
        chart
            .update_general_series_options(series, options.clone())
            .unwrap();
        assert_eq!(chart.general_series(series).unwrap().dataset(), dataset);
        assert_eq!(
            chart.general_series(series).unwrap().baseline_value(),
            baseline
        );
        assert_eq!(
            chart.general_series(series).unwrap().baseline_policy(),
            options.baseline_policy
        );
        let stacked_frame = chart.build_frame();
        assert!(stacked_frame.panes[pane].main.iter().any(|primitive| {
            matches!(
                primitive,
                Prim::BandFill { .. } | Prim::BandGradientFill { .. }
            )
        }));
        assert!(
            !stacked_frame.panes[pane]
                .main
                .iter()
                .any(|primitive| { matches!(primitive, Prim::AreaFill { .. }) })
        );
        let stacked = chart.export_state_json().unwrap();
        options.baseline_value = Some(f64::INFINITY);
        assert!(
            chart
                .update_general_series_options(series, options.clone())
                .is_err()
        );
        assert_eq!(chart.export_state_json().unwrap(), stacked);
        options.baseline_value = baseline;
        if baseline.is_some() {
            options.baseline_policy = GeneralAreaBaseline::DomainMin;
            assert!(
                chart
                    .update_general_series_options(series, options.clone())
                    .is_err()
            );
            assert_eq!(chart.export_state_json().unwrap(), stacked);
            options.baseline_policy = GeneralAreaBaseline::Zero;
        }
        options.stack_id = None;
        chart
            .update_general_series_options(series, options)
            .unwrap();
        assert!(
            chart.build_frame().panes[pane]
                .main
                .iter()
                .any(|primitive| { matches!(primitive, Prim::AreaFill { .. }) })
        );
        assert_eq!(chart.export_state_json().unwrap(), before);
        assert!(chart.remove_general_series(series));
    }
}

#[test]
fn general_shared_tooltip_groups_visible_rows_by_exact_horizontal_datum() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();

    let first_data = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: Some(vec![GeneralRowId::Number(10.0), GeneralRowId::Number(11.0)]),
            x: vec![1.0, 2.0],
            y: vec![10.0, 20.0],
            y_valid: None,
        })
        .unwrap();
    let second_data = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: Some(vec![
                GeneralRowId::Number(20.0),
                GeneralRowId::Number(21.0),
                GeneralRowId::Number(22.0),
            ]),
            x: vec![1.0, 1.0, 3.0],
            y: vec![30.0, 31.0, 32.0],
            y_valid: None,
        })
        .unwrap();
    let hidden_data = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: Some(vec![GeneralRowId::Number(30.0)]),
            x: vec![1.0],
            y: vec![40.0],
            y_valid: None,
        })
        .unwrap();

    let mut first_options = GeneralSeriesOptions::xy_line(pane, first_data, "x", "y");
    first_options.title = "First".into();
    let first = chart.add_general_series(first_options).unwrap();

    let mut second_options = GeneralSeriesOptions::scatter(pane, second_data, "x", "y");
    second_options.title = "Second".into();
    let second = chart.add_general_series(second_options).unwrap();

    let mut hidden_options = GeneralSeriesOptions::xy_area(pane, hidden_data, "x", "y");
    hidden_options.visible = false;
    hidden_options.title = "Hidden".into();
    let hidden = chart.add_general_series(hidden_options).unwrap();

    let snapshot = chart.general_shared_tooltip_snapshot(first, 0).unwrap();
    assert_eq!(snapshot.pane, pane);
    assert_eq!(snapshot.anchor_series, first);
    assert_eq!(snapshot.anchor_row, 0);
    assert_eq!(
        snapshot
            .items
            .iter()
            .map(|item| (item.series, item.row, item.value, item.title.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (first, 0, Some(10.0), "First"),
            (second, 0, Some(30.0), "Second"),
            (second, 1, Some(31.0), "Second"),
        ]
    );
    assert!(chart.general_shared_tooltip_snapshot(first, 99).is_none());
    assert!(chart.general_shared_tooltip_snapshot(hidden, 0).is_none());
}

#[test]
fn general_shared_tooltip_matches_category_identity_across_dataset_registries() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let first_data = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["A".into(), "B".into()],
            category_indices: vec![0, 1],
            y: vec![1.0, 2.0],
            y_valid: None,
        })
        .unwrap();
    let second_data = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["B".into(), "A".into()],
            category_indices: vec![0, 1, 1],
            y: vec![3.0, 4.0, 5.0],
            y_valid: None,
        })
        .unwrap();
    let first = chart
        .add_general_series(GeneralSeriesOptions::xy_line(pane, first_data, "x", "y"))
        .unwrap();
    let second = chart
        .add_general_series(GeneralSeriesOptions::xy_line(pane, second_data, "x", "y"))
        .unwrap();
    let snapshot = chart.general_shared_tooltip_snapshot(first, 0).unwrap();
    assert_eq!(
        snapshot
            .items
            .iter()
            .map(|item| (item.series, item.row, item.x_label.as_str()))
            .collect::<Vec<_>>(),
        vec![(first, 0, "A"), (second, 1, "A"), (second, 2, "A")]
    );
}

#[test]
fn general_brush_keeps_semantic_range_and_returns_bounded_visible_rows() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    let mut x =
        GeneralAxisOptions::new("brush-x", pane, AxisDimension::X, GeneralScaleType::Linear);
    x.domain = GeneralAxisDomain::Numeric([0.0, 4.0]);
    chart.add_general_axis(x).unwrap();
    let mut y =
        GeneralAxisOptions::new("brush-y", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y.domain = GeneralAxisDomain::Numeric([0.0, 100.0]);
    chart.add_general_axis(y).unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: Some(vec![
                GeneralRowId::Text("zero".into()),
                GeneralRowId::Text("one".into()),
                GeneralRowId::Text("two".into()),
                GeneralRowId::Text("three".into()),
                GeneralRowId::Text("four".into()),
            ]),
            x: vec![0.0, 1.0, 2.0, 3.0, 4.0],
            y: vec![10.0, 20.0, 30.0, 40.0, 50.0],
            y_valid: None,
        })
        .unwrap();
    chart
        .add_general_series(GeneralSeriesOptions::xy_line(
            pane, dataset, "brush-x", "brush-y",
        ))
        .unwrap();
    let hidden_dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: Some(vec![GeneralRowId::Text("hidden".into())]),
            x: vec![2.0],
            y: vec![90.0],
            y_valid: None,
        })
        .unwrap();
    let mut hidden = GeneralSeriesOptions::scatter(pane, hidden_dataset, "brush-x", "brush-y");
    hidden.visible = false;
    chart.add_general_series(hidden).unwrap();

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let plot = chart.general_plot_rect(pane).unwrap();
    let snapshot = chart
        .set_general_brush_from_pixels("brush-x", plot.width * 0.25, plot.width * 0.75)
        .unwrap();
    assert_eq!(snapshot.pane, pane);
    assert_eq!(snapshot.axis_id, "brush-x");
    assert_eq!(snapshot.dimension, AxisDimension::X);
    assert_eq!(
        snapshot.range,
        crate::GeneralBrushRange::Numeric([1.0, 3.0])
    );
    assert_eq!(
        snapshot
            .items
            .iter()
            .map(|hit| hit.row_id.clone())
            .collect::<Vec<_>>(),
        vec![
            GeneralRowIdentity::Explicit(GeneralRowId::Text("one".into())),
            GeneralRowIdentity::Explicit(GeneralRowId::Text("two".into())),
            GeneralRowIdentity::Explicit(GeneralRowId::Text("three".into())),
        ]
    );

    let before_bounds = chart.general_brush_axis_bounds(pane).unwrap();
    chart.zoom_general_axis("brush-x", 2.0, 2.0).unwrap();
    let after = chart.general_brush_snapshot().unwrap();
    assert_eq!(after.range, crate::GeneralBrushRange::Numeric([1.0, 3.0]));
    let after_bounds = chart.general_brush_axis_bounds(pane).unwrap();
    assert_ne!(
        before_bounds, after_bounds,
        "semantic brush reprojects after zoom"
    );

    let nudged = chart
        .nudge_general_brush(crate::GeneralBrushEndpoint::Start, 1)
        .unwrap();
    let crate::GeneralBrushRange::Numeric([start, end]) = nudged.range else {
        panic!("numeric brush must retain its range kind");
    };
    assert!(start > 1.0 && start < end);
    assert_eq!(end, 3.0);
    let full = chart.select_full_general_brush("brush-x").unwrap();
    assert_eq!(full.range, crate::GeneralBrushRange::Numeric([1.0, 3.0]));

    let semantic = chart
        .set_general_brush_range("brush-x", crate::GeneralBrushRange::Numeric([1.5, 2.5]))
        .unwrap();
    assert_eq!(
        semantic.range,
        crate::GeneralBrushRange::Numeric([1.5, 2.5])
    );
    assert_eq!(semantic.items.len(), 1);
    assert!(
        chart
            .set_general_brush_range("brush-x", crate::GeneralBrushRange::Numeric([3.0, 1.0]))
            .is_err()
    );
    assert_eq!(
        chart.general_brush_snapshot().unwrap().range,
        semantic.range,
        "an invalid controlled range must not change the brush"
    );

    chart.clear_general_brush();
    assert!(chart.general_brush_snapshot().is_none());
}

#[test]
fn general_brush_keyboard_steps_follow_log_and_temporal_transforms() {
    let mut log = ChartEngine::new_with_initial_domain(
        640.0,
        400.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Logarithmic,
        },
    )
    .unwrap();
    let mut log_x =
        GeneralAxisOptions::new("x", 0, AxisDimension::X, GeneralScaleType::Logarithmic);
    log_x.domain = GeneralAxisDomain::Numeric([1.0, 1_000.0]);
    log.add_general_axis(log_x).unwrap();
    log.add_general_axis(GeneralAxisOptions::new(
        "y",
        0,
        AxisDimension::Y,
        GeneralScaleType::Linear,
    ))
    .unwrap();
    let log_data = log
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: vec![1.0, 10.0, 100.0, 1_000.0],
            y: vec![1.0, 2.0, 3.0, 4.0],
            y_valid: None,
        })
        .unwrap();
    log.add_general_series(GeneralSeriesOptions::xy_line(0, log_data, "x", "y"))
        .unwrap();
    log.select_full_general_brush("x").unwrap();
    let start = log
        .nudge_general_brush(crate::GeneralBrushEndpoint::Start, 1)
        .unwrap();
    assert!(
        matches!(start.range, crate::GeneralBrushRange::Numeric([from, to]) if from > 1.0 && from < 2.0 && to == 1_000.0)
    );

    let mut temporal =
        ChartEngine::new_with_initial_domain(640.0, 400.0, 1.0, HorizontalDomain::Temporal)
            .unwrap();
    let mut time_x = GeneralAxisOptions::new("x", 0, AxisDimension::X, GeneralScaleType::Temporal);
    time_x.domain = GeneralAxisDomain::Temporal([0, 100_000]);
    temporal.add_general_axis(time_x).unwrap();
    temporal
        .add_general_axis(GeneralAxisOptions::new(
            "y",
            0,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let time_data = temporal
        .create_general_xy_dataset(GeneralXyInput::Temporal {
            ids: None,
            x_epoch_ms: vec![0, 50_000, 100_000],
            y: vec![1.0, 2.0, 3.0],
            y_valid: None,
        })
        .unwrap();
    temporal
        .add_general_series(GeneralSeriesOptions::xy_line(0, time_data, "x", "y"))
        .unwrap();
    temporal.select_full_general_brush("x").unwrap();
    let start = temporal
        .nudge_general_brush(crate::GeneralBrushEndpoint::Start, 1)
        .unwrap();
    assert_eq!(
        start.range,
        crate::GeneralBrushRange::Temporal([1_000, 100_000])
    );
    let end = temporal
        .nudge_general_brush(crate::GeneralBrushEndpoint::End, -1)
        .unwrap();
    assert_eq!(
        end.range,
        crate::GeneralBrushRange::Temporal([1_000, 99_000])
    );
}

fn proposal_state(chart: &ChartEngine, pane: usize) -> (Vec<String>, usize, usize) {
    (
        chart
            .general_axes(Some(pane))
            .into_iter()
            .map(|axis| axis.id().to_owned())
            .collect(),
        chart.general_series_ids_in_pane(pane).len(),
        chart.general_reference_count(),
    )
}

fn numeric_proposal(
    pane: usize,
    x_scale: GeneralScaleType,
    y_scale: GeneralScaleType,
    kind: GeneralSeriesKind,
    data: GeneralXyInput,
) -> GeneralPaneProposal {
    GeneralPaneProposal {
        pane,
        axes: vec![
            GeneralAxisOptions::new("x", pane, AxisDimension::X, x_scale),
            GeneralAxisOptions::new("y", pane, AxisDimension::Y, y_scale),
        ],
        series: vec![GeneralSeriesProposal {
            kind,
            x_axis_id: "x".into(),
            y_axis_id: "y".into(),
            data,
        }],
        references: vec![],
    }
}

#[test]
fn general_pane_proposal_dry_run_accepts_complete_graph_without_mutation() {
    let chart = ChartEngine::new_with_initial_domain(
        640.0,
        400.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    let before = proposal_state(&chart, 0);
    let mut proposal = numeric_proposal(
        0,
        GeneralScaleType::Linear,
        GeneralScaleType::Linear,
        GeneralSeriesKind::XyLine,
        GeneralXyInput::Numeric {
            ids: Some(vec![GeneralRowId::Text("a".into())]),
            x: vec![1.0],
            y: vec![2.0],
            y_valid: None,
        },
    );
    proposal.references.push(GeneralReferenceOptions::Line {
        pane: 0,
        axis_id: "y".into(),
        value: GeneralReferenceValue::Numeric(2.0),
        color: None,
        line_width: 1.0,
        extend_domain: false,
    });
    chart.validate_general_pane_proposal(&proposal).unwrap();
    assert_eq!(proposal_state(&chart, 0), before);
}

#[test]
fn general_pane_proposal_rejects_stale_and_incompatible_axes_without_mutation() {
    let chart = ChartEngine::new_with_initial_domain(
        640.0,
        400.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    let before = proposal_state(&chart, 0);
    let stale = GeneralPaneProposal {
        pane: 99,
        axes: vec![],
        series: vec![],
        references: vec![],
    };
    assert!(
        chart
            .validate_general_pane_proposal(&stale)
            .unwrap_err()
            .to_string()
            .contains("stale pane")
    );
    let incompatible = numeric_proposal(
        0,
        GeneralScaleType::Band,
        GeneralScaleType::Linear,
        GeneralSeriesKind::XyLine,
        GeneralXyInput::Numeric {
            ids: None,
            x: vec![1.0],
            y: vec![2.0],
            y_valid: None,
        },
    );
    assert!(chart.validate_general_pane_proposal(&incompatible).is_err());
    assert_eq!(proposal_state(&chart, 0), before);
}

#[test]
fn general_pane_proposal_rejects_kind_and_row_shape_without_mutation() {
    let chart = ChartEngine::new_with_initial_domain(
        640.0,
        400.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    let before = proposal_state(&chart, 0);
    let kind_mismatch = numeric_proposal(
        0,
        GeneralScaleType::Linear,
        GeneralScaleType::Linear,
        GeneralSeriesKind::Column,
        GeneralXyInput::Numeric {
            ids: None,
            x: vec![1.0],
            y: vec![2.0],
            y_valid: None,
        },
    );
    assert!(
        chart
            .validate_general_pane_proposal(&kind_mismatch)
            .is_err()
    );
    let row_shape = numeric_proposal(
        0,
        GeneralScaleType::Linear,
        GeneralScaleType::Linear,
        GeneralSeriesKind::Scatter,
        GeneralXyInput::Numeric {
            ids: None,
            x: vec![1.0, 2.0],
            y: vec![2.0],
            y_valid: None,
        },
    );
    assert!(chart.validate_general_pane_proposal(&row_shape).is_err());
    assert_eq!(proposal_state(&chart, 0), before);
}

#[test]
fn general_pane_proposal_rejects_all_live_log_data_classes_without_mutation() {
    let continuous = || {
        ChartEngine::new_with_initial_domain(
            640.0,
            400.0,
            1.0,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Logarithmic,
            },
        )
        .unwrap()
    };
    for data in [
        GeneralXyInput::Numeric {
            ids: None,
            x: vec![0.0],
            y: vec![1.0],
            y_valid: None,
        },
        GeneralXyInput::Numeric {
            ids: None,
            x: vec![1.0],
            y: vec![0.0],
            y_valid: None,
        },
    ] {
        let chart = continuous();
        let before = proposal_state(&chart, 0);
        let proposal = numeric_proposal(
            0,
            GeneralScaleType::Logarithmic,
            GeneralScaleType::Logarithmic,
            GeneralSeriesKind::Scatter,
            data,
        );
        assert!(chart.validate_general_pane_proposal(&proposal).is_err());
        assert_eq!(proposal_state(&chart, 0), before);
    }

    for (x, y) in [(vec![0.0], vec![1.0]), (vec![1.0], vec![0.0])] {
        let chart = continuous();
        let before = proposal_state(&chart, 0);
        let proposal = numeric_proposal(
            0,
            GeneralScaleType::Logarithmic,
            GeneralScaleType::Logarithmic,
            GeneralSeriesKind::HeatmapGrid,
            GeneralXyInput::HeatmapNumericNumeric {
                ids: None,
                x,
                y_coordinate: y,
                value: vec![1.0],
                value_valid: None,
            },
        );
        assert!(chart.validate_general_pane_proposal(&proposal).is_err());
        assert_eq!(proposal_state(&chart, 0), before);
    }

    let chart = ChartEngine::new_with_initial_domain(
        640.0,
        400.0,
        1.0,
        HorizontalDomain::Category {
            scale: CategoryScaleType::Band,
        },
    )
    .unwrap();
    let before = proposal_state(&chart, 0);
    let proposal = numeric_proposal(
        0,
        GeneralScaleType::Band,
        GeneralScaleType::Logarithmic,
        GeneralSeriesKind::BoxPlot,
        GeneralXyInput::BoxCategory {
            ids: None,
            categories: vec!["A".into()],
            category_indices: vec![0],
            min: vec![0.0],
            min_valid: None,
            q1: vec![1.0],
            q1_valid: None,
            median: vec![2.0],
            median_valid: None,
            q3: vec![3.0],
            q3_valid: None,
            max: vec![4.0],
            max_valid: None,
        },
    );
    assert!(chart.validate_general_pane_proposal(&proposal).is_err());
    assert_eq!(proposal_state(&chart, 0), before);
}

#[test]
fn general_pane_proposal_validates_references_against_proposed_axes_without_mutation() {
    let chart = ChartEngine::new_with_initial_domain(
        640.0,
        400.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Logarithmic,
        },
    )
    .unwrap();
    let before = proposal_state(&chart, 0);
    let mut proposal = numeric_proposal(
        0,
        GeneralScaleType::Logarithmic,
        GeneralScaleType::Logarithmic,
        GeneralSeriesKind::Scatter,
        GeneralXyInput::Numeric {
            ids: None,
            x: vec![1.0],
            y: vec![2.0],
            y_valid: None,
        },
    );
    proposal.references.push(GeneralReferenceOptions::Line {
        pane: 0,
        axis_id: "y".into(),
        value: GeneralReferenceValue::Numeric(0.0),
        color: None,
        line_width: 1.0,
        extend_domain: false,
    });
    assert!(chart.validate_general_pane_proposal(&proposal).is_err());
    assert_eq!(proposal_state(&chart, 0), before);
}

#[test]
fn reference_regions_use_plot_edges_through_axis_zoom_and_mixed_layout() {
    let mut chart = ChartEngine::new_with_initial_domain(
        420.0,
        280.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    let pane = 0;
    for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
        let mut axis = GeneralAxisOptions::new(id, pane, dimension, GeneralScaleType::Linear);
        axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        chart.add_general_axis(axis).unwrap();
    }
    let options: GeneralReferenceOptions = serde_json::from_str(
        r##"{"kind":"region","pane":0,"x_axis_id":"x","y_axis_id":"y",
             "x_from":{"type":"numeric","value":2.0},
             "x_to":{"type":"numeric","value":8.0},
             "fill_color":"#2468ac"}"##,
    )
    .unwrap();
    let reference = chart.add_general_reference(options).unwrap();
    let color = Color::parse_css("#2468ac").unwrap();
    let check = |chart: &mut ChartEngine, pane: usize| {
        chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
        let frame = chart.build_frame();
        let plot = chart.general_plot_rect(pane).unwrap();
        let rect = frame.panes[pane]
            .main
            .iter()
            .find_map(|prim| match prim {
                Prim::Rect { rect, color: fill } if *fill == color => Some(*rect),
                _ => None,
            })
            .expect("full-height reference region");
        assert_eq!(rect.y, plot.y.round() as i32);
        assert_eq!(rect.h, (plot.y + plot.height).round() as i32 - rect.y);
        assert!(rect.w > 0);
    };
    check(&mut chart, pane);
    chart.zoom_general_axis("y", 2.0, 5.0).unwrap();
    check(&mut chart, pane);
    let invalid: GeneralReferenceOptions = serde_json::from_str(
        r##"{"kind":"region","pane":0,"x_axis_id":"x","y_axis_id":"y",
             "fill_color":"#2468ac"}"##,
    )
    .unwrap();
    assert!(
        chart
            .update_general_reference_options(reference, invalid)
            .is_err()
    );
    assert_eq!(chart.general_reference_ids(Some(pane)), vec![reference]);

    let y_only: GeneralReferenceOptions = serde_json::from_str(
        r##"{"kind":"region","pane":0,"x_axis_id":"x","y_axis_id":"y",
             "y_from":{"type":"numeric","value":3.0},
             "y_to":{"type":"numeric","value":7.0},
             "fill_color":"#7b438f"}"##,
    )
    .unwrap();
    chart.add_general_reference(y_only).unwrap();
    let frame = chart.build_frame();
    let plot = chart.general_plot_rect(pane).unwrap();
    let rect = frame.panes[pane]
        .main
        .iter()
        .find_map(|prim| match prim {
            Prim::Rect { rect, color } if *color == Color::parse_css("#7b438f").unwrap() => {
                Some(*rect)
            }
            _ => None,
        })
        .expect("full-width reference region");
    assert_eq!(rect.x, plot.x.round() as i32);
    assert_eq!(rect.w, plot.width.round() as i32);

    for (bound, value, color) in [("x_to", 4.0, "#af5c36"), ("y_to", 4.0, "#4378a1")] {
        let mut options = serde_json::json!({
            "kind": "region", "pane": pane, "x_axis_id": "x", "y_axis_id": "y",
            "fill_color": color,
        });
        options[bound] = serde_json::json!({"type": "numeric", "value": value});
        chart
            .add_general_reference(serde_json::from_value(options).unwrap())
            .unwrap();
        let frame = chart.build_frame();
        let plot = chart.general_plot_rect(pane).unwrap();
        let rect = frame.panes[pane]
            .main
            .iter()
            .find_map(|prim| match prim {
                Prim::Rect { rect, color: fill } if *fill == Color::parse_css(color).unwrap() => {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("one-sided reference region");
        if bound == "x_to" {
            assert_eq!(rect.x, plot.x.round() as i32);
            assert_eq!(rect.h, (plot.y + plot.height).round() as i32 - rect.y);
        } else {
            assert_eq!(rect.y, plot.y.round() as i32);
            assert_eq!(rect.w, plot.width.round() as i32);
        }
    }

    let mut mixed = ChartEngine::new(420.0, 280.0, 1.0);
    let mixed_pane = mixed
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
        let mut axis = GeneralAxisOptions::new(id, mixed_pane, dimension, GeneralScaleType::Linear);
        axis.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        mixed.add_general_axis(axis).unwrap();
    }
    let mixed_options: GeneralReferenceOptions = serde_json::from_value(serde_json::json!({
        "kind": "region", "pane": mixed_pane, "x_axis_id": "x", "y_axis_id": "y",
        "x_from": {"type": "numeric", "value": 2.0},
        "x_to": {"type": "numeric", "value": 8.0},
        "fill_color": "#2468ac"
    }))
    .unwrap();
    mixed.add_general_reference(mixed_options).unwrap();
    check(&mut mixed, mixed_pane);
    mixed.zoom_general_axis("y", 2.0, 5.0).unwrap();
    check(&mut mixed, mixed_pane);
}
#[test]
fn general_references_render_all_kinds_and_extend_domains_only_when_requested() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "ref-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "ref-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: vec![0.0, 1.0],
            y: vec![0.0, 1.0],
            y_valid: None,
        })
        .unwrap();
    chart
        .add_general_series(GeneralSeriesOptions::xy_line(
            pane, dataset, "ref-x", "ref-y",
        ))
        .unwrap();

    let base_x = chart.general_axis_effective_domain("ref-x").unwrap();
    let ignored = chart
        .add_general_reference(crate::GeneralReferenceOptions::Line {
            pane,
            axis_id: "ref-x".into(),
            value: crate::GeneralReferenceValue::Numeric(100.0),
            color: Some("#445566".into()),
            line_width: 2.0,
            extend_domain: false,
        })
        .unwrap();
    assert_eq!(
        chart.general_axis_effective_domain("ref-x").unwrap(),
        base_x,
        "non-extending references must not change automatic domains"
    );

    let extending = chart
        .add_general_reference(crate::GeneralReferenceOptions::Line {
            pane,
            axis_id: "ref-x".into(),
            value: crate::GeneralReferenceValue::Numeric(10.0),
            color: Some("#112233".into()),
            line_width: 2.0,
            extend_domain: true,
        })
        .unwrap();
    let GeneralAxisDomain::Numeric(x_domain) =
        chart.general_axis_effective_domain("ref-x").unwrap()
    else {
        panic!("reference X axis must remain numeric");
    };
    assert!(x_domain[1] >= 10.0);

    let dot = chart
        .add_general_reference(crate::GeneralReferenceOptions::Dot {
            pane,
            x_axis_id: "ref-x".into(),
            y_axis_id: "ref-y".into(),
            x: crate::GeneralReferenceValue::Numeric(0.5),
            y: crate::GeneralReferenceValue::Numeric(0.5),
            color: Some("#778899".into()),
            radius: 5.0,
            extend_domain: false,
        })
        .unwrap();
    let region = chart
        .add_general_reference(crate::GeneralReferenceOptions::Region {
            pane,
            x_axis_id: "ref-x".into(),
            y_axis_id: "ref-y".into(),
            x_from: Some(crate::GeneralReferenceValue::Numeric(0.2)),
            x_to: Some(crate::GeneralReferenceValue::Numeric(0.8)),
            y_from: Some(crate::GeneralReferenceValue::Numeric(0.2)),
            y_to: Some(crate::GeneralReferenceValue::Numeric(0.8)),
            fill_color: Some("rgba(10,20,30,0.25)".into()),
            extend_domain: false,
        })
        .unwrap();

    assert_eq!(
        chart.general_reference_ids(Some(pane)),
        vec![ignored, extending, dot, region]
    );
    assert_eq!(chart.general_reference_options(dot).unwrap().pane(), pane);
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let frame = chart.build_frame();
    let main = &frame.panes[pane].main;
    assert!(
        main.iter()
            .any(|primitive| matches!(primitive, Prim::Rect { .. }))
    );
    assert!(
        main.iter()
            .any(|primitive| matches!(primitive, Prim::Circle { .. }))
    );
    assert!(
        main.iter()
            .any(|primitive| matches!(primitive, Prim::VLine { .. }))
    );
    let region_position = main
        .iter()
        .position(|primitive| {
            matches!(primitive, Prim::Rect { color, .. }
                if *color == Color::parse_css("rgba(10,20,30,0.25)").unwrap())
        })
        .unwrap();
    let series_position = main
        .iter()
        .position(|primitive| matches!(primitive, Prim::Polyline { .. }))
        .unwrap();
    let dot_position = main
        .iter()
        .position(|primitive| {
            matches!(primitive, Prim::Circle { fill, .. }
                if *fill == Color::parse_css("#778899").unwrap())
        })
        .unwrap();
    assert!(region_position < series_position && series_position < dot_position);

    let before = chart.general_reference_options(ignored).unwrap();
    let invalid = crate::GeneralReferenceOptions::Line {
        pane,
        axis_id: "ref-x".into(),
        value: crate::GeneralReferenceValue::Category("wrong scale".into()),
        color: Some("#445566".into()),
        line_width: 2.0,
        extend_domain: true,
    };
    assert!(
        chart
            .update_general_reference_options(ignored, invalid)
            .is_err()
    );
    assert_eq!(chart.general_reference_options(ignored), Some(before));
    chart
        .update_general_reference_options(
            ignored,
            crate::GeneralReferenceOptions::Line {
                pane,
                axis_id: "ref-x".into(),
                value: crate::GeneralReferenceValue::Numeric(50.0),
                color: Some("#445566".into()),
                line_width: 3.0,
                extend_domain: true,
            },
        )
        .unwrap();
    assert_eq!(chart.general_reference_ids(Some(pane))[0], ignored);
    let GeneralAxisDomain::Numeric(updated_domain) =
        chart.general_axis_effective_domain("ref-x").unwrap()
    else {
        panic!("reference X axis must remain numeric");
    };
    assert!(updated_domain[1] >= 50.0);

    let one_sided = chart
        .add_general_reference(crate::GeneralReferenceOptions::Region {
            pane,
            x_axis_id: "ref-x".into(),
            y_axis_id: "ref-y".into(),
            x_from: None,
            x_to: Some(crate::GeneralReferenceValue::Numeric(75.0)),
            y_from: None,
            y_to: None,
            fill_color: Some("#af5c36".into()),
            extend_domain: true,
        })
        .unwrap();
    let GeneralAxisDomain::Numeric(one_sided_domain) =
        chart.general_axis_effective_domain("ref-x").unwrap()
    else {
        panic!("one-sided region X axis must remain numeric");
    };
    assert!(one_sided_domain[1] >= 75.0);
    assert!(chart.remove_general_reference(one_sided));

    assert!(chart.remove_general_reference(ignored));
    assert_eq!(
        chart.general_reference_ids(Some(pane)),
        vec![extending, dot, region]
    );
}

#[test]
fn dense_category_axis_collision_work_and_output_are_bounded() {
    let mut chart = ChartEngine::new(160.0, 240.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Point,
            },
        )
        .unwrap();
    let mut x = GeneralAxisOptions::new("dense", pane, AxisDimension::X, GeneralScaleType::Point);
    x.domain = GeneralAxisDomain::Category(
        (0..2_000)
            .map(|index| format!("category-{index}"))
            .collect(),
    );
    chart.add_general_axis(x).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

    let frame = chart.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let category_labels = frame
        .labels
        .iter()
        .filter(|label| label.text.starts_with("category-"))
        .count();
    assert!(category_labels > 0);
    assert!(
        category_labels < 20,
        "collision selection must scale with pixels"
    );
    assert!(category_labels <= usize::from(crate::MAX_GENERAL_AXIS_TICKS));
}

#[test]
fn excessive_general_axes_preserve_a_bounded_plot_and_chart_space_chrome() {
    let mut chart = ChartEngine::new(240.0, 180.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: crate::ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    for index in 0..20 {
        let mut x = GeneralAxisOptions::new(
            format!("x-{index}"),
            pane,
            AxisDimension::X,
            GeneralScaleType::Linear,
        );
        x.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        chart.add_general_axis(x).unwrap();
        let mut y = GeneralAxisOptions::new(
            format!("y-{index}"),
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        );
        y.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        chart.add_general_axis(y).unwrap();
    }
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let plot = chart.general_plot_rect(pane).unwrap();
    assert!(plot.width >= 1.0 && plot.height >= 1.0);
    assert!(chart.pane_left + chart.pane_w + chart.axis_w <= chart.css_width + f64::EPSILON);

    let frame = chart.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let pane_state = &chart.panes[pane];
    assert!(frame.bands.iter().all(|band| {
        band.x >= 0.0
            && band.y >= pane_state.top
            && band.x + band.width <= chart.css_width
            && band.y + band.height <= pane_state.top + pane_state.height
    }));
}

#[test]
fn category_column_series_owns_auto_domains_geometry_and_lifecycle() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    let mut x = GeneralAxisOptions::new("month", pane, AxisDimension::X, GeneralScaleType::Band);
    x.band_padding_inner = 0.2;
    chart.add_general_axis(x).unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "revenue",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["Jan".into(), "Feb".into(), "Mar".into()],
            category_indices: vec![0, 1, 2],
            y: vec![-5.0, 10.0, 99.0],
            y_valid: Some(vec![1, 1, 0]),
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::column(pane, dataset, "month", "revenue");
    options.color = Some("#123456".into());
    options.title = "Revenue".into();
    options.data_labels = true;
    let series = chart.add_general_series(options).unwrap();

    assert_eq!(
        chart.general_axis_effective_domain("month"),
        Some(GeneralAxisDomain::Category(vec![
            "Jan".into(),
            "Feb".into(),
            "Mar".into()
        ]))
    );
    assert_eq!(
        chart.general_axis_effective_domain("revenue"),
        Some(GeneralAxisDomain::Numeric([-5.0, 10.0]))
    );
    assert!(chart.memory_usage().general_series_capacity_bytes > 0);

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let axis = chart.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    for category in ["Jan", "Feb", "Mar"] {
        assert!(axis.labels.iter().any(|label| label.text == category));
    }

    let plot = chart.general_plot_rect(pane).unwrap();
    let baseline = aeris_charts_core::scale::general_scale::LinearScale::new(
        -5.0,
        10.0,
        plot.y + plot.height,
        plot.y,
    )
    .unwrap()
    .coordinate(0.0)
    .unwrap()
    .round() as i32;
    let expected_color = Color::parse_css("#123456").unwrap();
    let frame = chart.build_frame();
    let rects: Vec<_> = frame.panes[pane]
        .main
        .iter()
        .filter_map(|primitive| match primitive {
            Prim::Rect { rect, color } if *color == expected_color => Some(*rect),
            _ => None,
        })
        .collect();
    assert_eq!(rects.len(), 2, "missing rows must emit no column geometry");
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::Text { text, .. } if text == "-5"))
    );
    assert!(
        !frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::Text { text, .. } if text == "99"))
    );

    assert!(rects.iter().any(|rect| rect.y == baseline));
    assert!(rects.iter().any(|rect| rect.y + rect.h == baseline));
    assert!(
        chart.frame_series_segments(pane).iter().any(
            |segment| segment.series_id.is_none() && segment.end - segment.start >= rects.len()
        )
    );

    let mut geometry = Vec::new();
    chart.visit_general_columns(chart.general_series(series).unwrap(), |item| {
        geometry.push(item)
    });
    assert_eq!(geometry.len(), 2);
    let first = geometry[0];
    let hit = chart
        .general_hit_test(
            pane,
            (first.left + first.right) / 2.0,
            (first.top + first.bottom) / 2.0,
            crate::GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!(hit.series, series);
    assert_eq!(hit.row, 0);
    assert_eq!(hit.distance, 0.0);
    assert_eq!(
        chart.update_general_hover(
            pane,
            (first.left + first.right) / 2.0,
            (first.top + first.bottom) / 2.0,
        ),
        Some(hit.clone())
    );
    assert_eq!(chart.general_hovered_hit(), Some(hit.clone()));
    assert!(chart.set_general_accessibility_focus(series, 0));
    assert_eq!(chart.general_accessibility_focused_hit(), Some(hit.clone()));
    let focused_frame = chart.build_frame();
    assert!(
        focused_frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::RectFrame { border: 1, .. }))
    );
    assert!(chart.select_general_hovered());
    assert_eq!(chart.general_selected_hit(), Some(hit.clone()));
    let selected_frame = chart.build_frame();
    assert!(selected_frame.panes[pane].main.iter().any(|primitive| {
        matches!(primitive, Prim::RectFrame { color, border: 2, .. } if *color == PRIMARY)
    }));
    chart.clear_general_hover();
    assert_eq!(chart.general_hovered_hit(), None);
    assert_eq!(chart.general_selected_hit(), Some(hit.clone()));

    let nearest = chart
        .general_hit_test(
            pane,
            first.left - 2.0,
            (first.top + first.bottom) / 2.0,
            crate::GeneralHitMode::Nearest { max_distance: 3.0 },
        )
        .unwrap();
    assert_eq!(nearest.row, 0);
    assert!((nearest.distance - 2.0).abs() < 1e-9);

    let tooltip = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert_eq!(tooltip.x_label, "Jan");
    assert_eq!(tooltip.value, Some(-5.0));
    assert_eq!(tooltip.title, "Revenue");
    let accessibility = chart
        .general_accessibility_snapshot(series, 0, usize::MAX)
        .unwrap();
    assert_eq!(accessibility.total_rows, 3);
    assert_eq!(accessibility.items.len(), 3);
    assert_eq!(accessibility.items[2].x_label, "Mar");
    assert_eq!(accessibility.items[2].value, None);

    assert!(chart.set_general_series_visible(series, false));
    assert_eq!(chart.general_axis_effective_domain("month"), None);
    assert_eq!(chart.general_axis_effective_domain("revenue"), None);
    assert!(!chart.build_frame().panes[pane].main.iter().any(
        |primitive| matches!(primitive, Prim::Rect { color, .. } if *color == expected_color)
    ));
    assert!(chart.set_general_series_visible(series, true));

    chart
        .replace_general_xy_dataset(
            dataset,
            GeneralXyInput::Category {
                ids: None,
                categories: vec!["Jan".into(), "Feb".into(), "Mar".into()],
                category_indices: vec![0, 1, 2],
                y: vec![-20.0, 5.0, 99.0],
                y_valid: Some(vec![1, 1, 0]),
            },
        )
        .unwrap();
    assert_eq!(
        chart.general_selected_hit(),
        None,
        "batch-scoped generated identities must not retain selection across replacement"
    );
    assert_eq!(
        chart.general_accessibility_focused_hit(),
        None,
        "batch-scoped generated identities must not retain keyboard focus across replacement"
    );
    assert_eq!(
        chart.general_axis_effective_domain("revenue"),
        Some(GeneralAxisDomain::Numeric([-20.0, 5.0]))
    );
    let replaced_axis = chart.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    assert!(replaced_axis.labels.iter().any(|label| label.text == "-20"));

    chart
        .replace_general_xy_dataset(
            dataset,
            GeneralXyInput::Category {
                ids: Some(vec![
                    GeneralRowId::Text("jan".into()),
                    GeneralRowId::Text("feb".into()),
                    GeneralRowId::Text("mar".into()),
                ]),
                categories: vec!["Jan".into(), "Feb".into(), "Mar".into()],
                category_indices: vec![0, 1, 2],
                y: vec![-20.0, 5.0, 10.0],
                y_valid: None,
            },
        )
        .unwrap();
    geometry.clear();
    chart.visit_general_columns(chart.general_series(series).unwrap(), |item| {
        geometry.push(item)
    });
    let jan = geometry.iter().find(|item| item.row == 0).unwrap();
    chart.update_general_hover(
        pane,
        (jan.left + jan.right) / 2.0,
        (jan.top + jan.bottom) / 2.0,
    );
    assert!(chart.select_general_hovered());
    assert!(chart.set_general_accessibility_focus(series, 0));
    chart
        .replace_general_xy_dataset(
            dataset,
            GeneralXyInput::Category {
                ids: Some(vec![
                    GeneralRowId::Text("feb".into()),
                    GeneralRowId::Text("jan".into()),
                    GeneralRowId::Text("mar".into()),
                ]),
                categories: vec!["Jan".into(), "Feb".into(), "Mar".into()],
                category_indices: vec![1, 0, 2],
                y: vec![5.0, -20.0, 10.0],
                y_valid: None,
            },
        )
        .unwrap();
    assert_eq!(chart.general_selected_hit().unwrap().row, 1);
    assert_eq!(
        chart.general_selected_hit().unwrap().row_id,
        crate::GeneralRowIdentity::Explicit(GeneralRowId::Text("jan".into()))
    );
    assert_eq!(chart.general_accessibility_focused_hit().unwrap().row, 1);
    assert_eq!(
        chart.general_accessibility_focused_hit().unwrap().row_id,
        crate::GeneralRowIdentity::Explicit(GeneralRowId::Text("jan".into()))
    );
    chart.clear_general_accessibility_focus();
    assert_eq!(chart.general_accessibility_focused_hit(), None);

    assert!(!chart.remove_general_axis("month"));
    assert!(!chart.remove_general_dataset(dataset));
    assert!(!chart.remove_pane(pane));
    assert!(chart.remove_general_series(series));
    assert_eq!(chart.memory_usage().general_series_capacity_bytes, 0);
    assert!(chart.remove_general_axis("month"));
    assert!(chart.remove_general_axis("revenue"));
    assert!(chart.remove_general_dataset(dataset));
    assert_eq!(chart.memory_usage().general_data_capacity_bytes, 0);
    assert!(chart.remove_pane(pane));
}

#[test]
fn automatic_category_views_clamp_to_replaced_registry_without_stale_labels() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "category-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "category-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: ["A", "B", "C", "D", "E", "F"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            category_indices: (0..6).collect(),
            y: vec![1.0; 6],
            y_valid: None,
        })
        .unwrap();
    chart
        .add_general_series(GeneralSeriesOptions::column(
            pane,
            dataset,
            "category-x",
            "category-y",
        ))
        .unwrap();
    chart
        .zoom_general_category_axis("category-x", 2.0, "C")
        .unwrap();
    assert_eq!(
        chart.general_axis_effective_domain("category-x"),
        Some(GeneralAxisDomain::Category(vec![
            "B".into(),
            "C".into(),
            "D".into(),
        ]))
    );

    chart
        .replace_general_xy_dataset(
            dataset,
            GeneralXyInput::Category {
                ids: None,
                categories: vec!["D".into(), "E".into()],
                category_indices: vec![0, 1],
                y: vec![2.0, 3.0],
                y_valid: None,
            },
        )
        .unwrap();
    assert_eq!(
        chart.general_axis_effective_domain("category-x"),
        Some(GeneralAxisDomain::Category(vec!["D".into(), "E".into()]))
    );
}

#[test]
fn composed_category_area_line_and_columns_share_axes_and_frame_order() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "compose-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "compose-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: Some(vec![
                GeneralRowId::Text("a".into()),
                GeneralRowId::Text("b".into()),
            ]),
            categories: vec!["A".into(), "B".into()],
            category_indices: vec![0, 1],
            y: vec![4.0, 6.0],
            y_valid: None,
        })
        .unwrap();
    let area = chart
        .add_general_series(GeneralSeriesOptions::xy_area(
            pane,
            dataset,
            "compose-x",
            "compose-y",
        ))
        .unwrap();
    let line = chart
        .add_general_series(GeneralSeriesOptions::xy_line(
            pane,
            dataset,
            "compose-x",
            "compose-y",
        ))
        .unwrap();
    let column = chart
        .add_general_series(GeneralSeriesOptions::column(
            pane,
            dataset,
            "compose-x",
            "compose-y",
        ))
        .unwrap();
    assert_eq!(
        chart.general_axis_effective_domain("compose-y"),
        Some(GeneralAxisDomain::Numeric([0.0, 6.0]))
    );
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let frame = chart.build_frame();
    let main = &frame.panes[pane].main;
    let fill = main
        .iter()
        .position(|prim| matches!(prim, Prim::AreaFill { .. }))
        .unwrap();
    let last_line = main
        .iter()
        .rposition(|prim| matches!(prim, Prim::Polyline { .. }))
        .unwrap();
    let first_bar = main
        .iter()
        .position(|prim| matches!(prim, Prim::Rect { .. }))
        .unwrap();
    assert!(fill < last_line && last_line < first_bar);
    let mut bars = Vec::new();
    chart.visit_general_columns(chart.general_series(column).unwrap(), |bar| bars.push(bar));
    assert_eq!(bars.len(), 2);
    let bar = bars[0];
    let hit = chart
        .general_hit_test(
            pane,
            (bar.left + bar.right) * 0.5,
            (bar.top + bar.bottom) * 0.5,
            crate::GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!((hit.series, hit.row), (column, 0));
    assert_ne!(area, line);
}

#[test]
fn grouped_columns_share_each_band_without_overlap_and_keep_hit_identity() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "group-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .unwrap();
    let mut y =
        GeneralAxisOptions::new("group-y", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y.domain = GeneralAxisDomain::Numeric([0.0, 20.0]);
    chart.add_general_axis(y).unwrap();

    let dataset_a = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: Some(vec![GeneralRowId::Text("a-jan".into())]),
            categories: vec!["Jan".into()],
            category_indices: vec![0],
            y: vec![10.0],
            y_valid: None,
        })
        .unwrap();
    let dataset_b = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: Some(vec![GeneralRowId::Text("b-jan".into())]),
            categories: vec!["Jan".into()],
            category_indices: vec![0],
            y: vec![15.0],
            y_valid: None,
        })
        .unwrap();
    let mut first = GeneralSeriesOptions::column(pane, dataset_a, "group-x", "group-y");
    first.group_id = Some("sales".into());
    let first = chart.add_general_series(first).unwrap();
    let mut second = GeneralSeriesOptions::column(pane, dataset_b, "group-x", "group-y");
    second.group_id = Some("sales".into());
    let second = chart.add_general_series(second).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

    let mut first_geometry = Vec::new();
    chart.visit_general_columns(chart.general_series(first).unwrap(), |geometry| {
        first_geometry.push(geometry)
    });
    let mut second_geometry = Vec::new();
    chart.visit_general_columns(chart.general_series(second).unwrap(), |geometry| {
        second_geometry.push(geometry)
    });
    assert_eq!(first_geometry.len(), 1);
    assert_eq!(second_geometry.len(), 1);
    let first_rect = first_geometry[0];
    let second_rect = second_geometry[0];
    assert!(first_rect.right <= second_rect.left + 1e-9);
    assert!((first_rect.right - second_rect.left).abs() < 1e-9);
    assert!(
        (first_rect.right - first_rect.left - (second_rect.right - second_rect.left)).abs() < 1e-9
    );

    for (series, rect, expected_id) in
        [(first, first_rect, "a-jan"), (second, second_rect, "b-jan")]
    {
        let hit = chart
            .general_hit_test(
                pane,
                (rect.left + rect.right) * 0.5,
                (rect.top + rect.bottom) * 0.5,
                crate::GeneralHitMode::Exact,
            )
            .unwrap();
        assert_eq!(hit.series, series);
        assert_eq!(
            hit.row_id,
            crate::GeneralRowIdentity::Explicit(GeneralRowId::Text(expected_id.into()))
        );
    }

    let mut resized = GeneralSeriesOptions::column(pane, dataset_a, "group-x", "group-y");
    resized.group_id = Some("sales".into());
    resized.bar_gap = 8.0;
    resized.bar_max_width = Some(20.0);
    chart
        .update_general_series_options(first, resized.clone())
        .unwrap();
    resized.dataset = dataset_b;
    chart
        .update_general_series_options(second, resized)
        .unwrap();
    let mut resized_rects = Vec::new();
    chart.visit_general_columns(chart.general_series(first).unwrap(), |rect| {
        resized_rects.push(rect)
    });
    chart.visit_general_columns(chart.general_series(second).unwrap(), |rect| {
        resized_rects.push(rect)
    });
    assert_eq!(resized_rects.len(), 2);
    assert!(
        resized_rects
            .iter()
            .all(|rect| rect.right - rect.left <= 20.0 + 1e-9)
    );
    assert!(resized_rects[1].left - resized_rects[0].right >= 8.0 - 1e-9);
    assert!(
        chart
            .general_hit_test(
                pane,
                (resized_rects[0].right + resized_rects[1].left) * 0.5,
                (resized_rects[0].top + resized_rects[0].bottom) * 0.5,
                crate::GeneralHitMode::Exact
            )
            .is_none()
    );

    let mut invalid = GeneralSeriesOptions::column(pane, dataset_a, "group-x", "group-y");
    invalid.group_id = Some("sales".into());
    invalid.bar_gap = f64::NAN;
    assert!(chart.update_general_series_options(first, invalid).is_err());
    assert_eq!(chart.general_series(first).unwrap().bar_gap(), 8.0);
    let mut out_of_range = GeneralSeriesOptions::column(pane, dataset_a, "group-x", "group-y");
    out_of_range.line_width = 40.0;
    assert!(
        chart
            .update_general_series_options(first, out_of_range)
            .is_err()
    );
    assert_eq!(chart.general_series(first).unwrap().bar_gap(), 8.0);
}

#[test]
fn stacked_columns_sum_auto_domain_and_diverge_from_zero_by_sign() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "stack-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "stack-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();

    let make_dataset = |chart: &mut ChartEngine, prefix: &str, values: Vec<f64>| {
        chart
            .create_general_xy_dataset(GeneralXyInput::Category {
                ids: Some(vec![
                    GeneralRowId::Text(format!("{prefix}-pos")),
                    GeneralRowId::Text(format!("{prefix}-neg")),
                ]),
                categories: vec!["Positive".into(), "Negative".into()],
                category_indices: vec![0, 1],
                y: values,
                y_valid: None,
            })
            .unwrap()
    };
    let first_dataset = make_dataset(&mut chart, "first", vec![10.0, -3.0]);
    let second_dataset = make_dataset(&mut chart, "second", vec![5.0, -2.0]);
    let mut first = GeneralSeriesOptions::column(pane, first_dataset, "stack-x", "stack-y");
    first.stack_id = Some("total".into());
    first.bar_corner_radius = 8.0;
    first.color = Some("#112233".into());
    let first = chart.add_general_series(first).unwrap();
    let mut second = GeneralSeriesOptions::column(pane, second_dataset, "stack-x", "stack-y");
    second.stack_id = Some("total".into());
    second.bar_corner_radius = 8.0;
    second.color = Some("#445566".into());
    let second = chart.add_general_series(second).unwrap();

    assert_eq!(
        chart.general_axis_effective_domain("stack-y"),
        Some(GeneralAxisDomain::Numeric([-5.0, 15.0]))
    );
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut first_geometry = Vec::new();
    chart.visit_general_columns(chart.general_series(first).unwrap(), |geometry| {
        first_geometry.push(geometry)
    });
    let mut second_geometry = Vec::new();
    chart.visit_general_columns(chart.general_series(second).unwrap(), |geometry| {
        second_geometry.push(geometry)
    });
    assert_eq!(first_geometry.len(), 2);
    assert_eq!(second_geometry.len(), 2);
    assert!((second_geometry[0].bottom - first_geometry[0].top).abs() < 1e-9);
    assert!((second_geometry[1].top - first_geometry[1].bottom).abs() < 1e-9);
    assert!(first_geometry.iter().all(|geometry| !geometry.exposed_end));
    assert!(second_geometry.iter().all(|geometry| geometry.exposed_end));
    assert!(second_geometry[0].end_at_min);
    assert!(!second_geometry[1].end_at_min);
    let corners: Vec<[f32; 4]> = chart.build_frame().panes[pane]
        .main
        .iter()
        .filter_map(|primitive| match primitive {
            Prim::RoundRect { radii, fill, .. }
                if *fill == aeris_charts_render::color::Color::parse_css("#445566").unwrap() =>
            {
                Some(*radii)
            }
            _ => None,
        })
        .collect();
    assert_eq!(corners.len(), 2);
    assert!(
        corners
            .iter()
            .any(|r| r[0] > 0.0 && r[1] > 0.0 && r[2] == 0.0 && r[3] == 0.0)
    );
    assert!(
        corners
            .iter()
            .any(|r| r[0] == 0.0 && r[1] == 0.0 && r[2] > 0.0 && r[3] > 0.0)
    );

    // Frame construction can resolve a different final plot from the measured layout.
    chart.build_frame();
    second_geometry.clear();
    chart.visit_general_columns(chart.general_series(second).unwrap(), |geometry| {
        second_geometry.push(geometry)
    });
    let second_positive = second_geometry[0];
    let hit = chart
        .general_hit_test(
            pane,
            (second_positive.left + second_positive.right) * 0.5,
            (second_positive.top + second_positive.bottom) * 0.5,
            crate::GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!(hit.series, second);
    let corner_hit = chart.general_hit_test(
        pane,
        second_geometry[0].right - 0.1,
        second_geometry[0].top + 0.1,
        crate::GeneralHitMode::Exact,
    );
    assert!(
        corner_hit.is_none(),
        "the clipped exposed corner must not claim a hit: {corner_hit:?}, {second_positive:?}"
    );
    assert_eq!(
        chart
            .general_hit_test(
                pane,
                second_geometry[0].right - 4.0,
                second_geometry[0].top + 4.0,
                crate::GeneralHitMode::Exact,
            )
            .unwrap()
            .series,
        second,
    );
    assert_eq!(hit.row, 0);

    assert!(chart.set_general_series_order(Some(pane), vec![second, first]));
    let mut reordered_first = Vec::new();
    let mut reordered_second = Vec::new();
    chart.visit_general_columns(chart.general_series(first).unwrap(), |geometry| {
        reordered_first.push(geometry)
    });
    chart.visit_general_columns(chart.general_series(second).unwrap(), |geometry| {
        reordered_second.push(geometry)
    });
    assert!((reordered_first[0].bottom - reordered_second[0].top).abs() < 1e-9);
    assert!((reordered_first[1].top - reordered_second[1].bottom).abs() < 1e-9);
    assert!(reordered_first.iter().all(|geometry| geometry.exposed_end));
    assert!(
        reordered_second
            .iter()
            .all(|geometry| !geometry.exposed_end)
    );
    assert_eq!(chart.general_series_order(Some(pane)), vec![second, first]);
}

#[test]
fn column_gradient_and_row_colors_preserve_rounded_bar_order() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "item-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "item-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let input = GeneralXyInput::Category {
        ids: None,
        categories: vec!["A".into(), "B".into(), "C".into()],
        category_indices: vec![0, 1, 2],
        y: vec![3.0, 5.0, 4.0],
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input.clone()).unwrap();
    chart
        .replace_general_xy_dataset_styled(
            dataset,
            input,
            None,
            Some(vec![Some("#ff0000".into()), Some("#00ff00".into()), None]),
            None,
        )
        .unwrap();
    let mut options = GeneralSeriesOptions::column(pane, dataset, "item-x", "item-y");
    options.bar_corner_radius = 4.0;
    options.fill_gradient = Some(["#112233".into(), "#445566".into()]);
    let series = chart.add_general_series(options).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let frame = chart.build_frame();
    let fills = frame.panes[pane]
        .main
        .iter()
        .filter_map(|primitive| match primitive {
            Prim::RoundRect { fill, .. } => Some(*fill),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(fills.contains(&Color::rgb(255, 0, 0)));
    assert!(fills.contains(&Color::rgb(0, 255, 0)));
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive,
        Prim::GradientRoundRect { gradient, .. }
        if gradient.top == Color::rgb(0x11, 0x22, 0x33)
            && gradient.bottom == Color::rgb(0x44, 0x55, 0x66)))
    );
    let mut square = GeneralSeriesOptions::column(pane, dataset, "item-x", "item-y");
    square.fill_gradient = Some(["#112233".into(), "#445566".into()]);
    chart.update_general_series_options(series, square).unwrap();
    assert!(
        chart.build_frame().panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::GradientRect { .. }))
    );
}

#[test]
fn horizontal_and_range_bars_emit_gradient_marks() {
    for horizontal in [false, true] {
        let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
        let pane = chart
            .add_pane_with_domain(
                true,
                if horizontal {
                    HorizontalDomain::Continuous {
                        scale: ContinuousScaleType::Linear,
                    }
                } else {
                    HorizontalDomain::Category {
                        scale: CategoryScaleType::Band,
                    }
                },
            )
            .unwrap();
        chart
            .add_general_axis(GeneralAxisOptions::new(
                "gradient-x",
                pane,
                AxisDimension::X,
                if horizontal {
                    GeneralScaleType::Linear
                } else {
                    GeneralScaleType::Band
                },
            ))
            .unwrap();
        chart
            .add_general_axis(GeneralAxisOptions::new(
                "gradient-y",
                pane,
                AxisDimension::Y,
                if horizontal {
                    GeneralScaleType::Band
                } else {
                    GeneralScaleType::Linear
                },
            ))
            .unwrap();
        let dataset = chart
            .create_general_xy_dataset(if horizontal {
                GeneralXyInput::Category {
                    ids: None,
                    categories: vec!["A".into()],
                    category_indices: vec![0],
                    y: vec![5.0],
                    y_valid: None,
                }
            } else {
                GeneralXyInput::RangeCategory {
                    ids: None,
                    categories: vec!["A".into()],
                    category_indices: vec![0],
                    low: vec![2.0],
                    low_valid: None,
                    high: vec![5.0],
                    high_valid: None,
                }
            })
            .unwrap();
        let mut options = if horizontal {
            GeneralSeriesOptions::horizontal_bar(pane, dataset, "gradient-x", "gradient-y")
        } else {
            GeneralSeriesOptions::range_bar(pane, dataset, "gradient-x", "gradient-y")
        };
        options.fill_gradient = Some(["#112233".into(), "#445566".into()]);
        if horizontal {
            options.bar_corner_radius = 4.0;
        }
        chart.add_general_series(options).unwrap();
        chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
        let frame = chart.build_frame();
        assert!(
            frame.panes[pane]
                .main
                .iter()
                .any(|primitive| match primitive {
                    Prim::GradientRect { gradient, .. } if !horizontal =>
                        gradient.top == Color::rgb(0x11, 0x22, 0x33)
                            && gradient.bottom == Color::rgb(0x44, 0x55, 0x66),
                    Prim::GradientRoundRect { gradient, .. } if horizontal =>
                        gradient.top == Color::rgb(0x11, 0x22, 0x33)
                            && gradient.bottom == Color::rgb(0x44, 0x55, 0x66),
                    _ => false,
                })
        );
    }
}

#[test]
fn positive_stacks_suppress_negative_bars_in_both_orientations() {
    for horizontal in [false, true] {
        let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
        let pane = chart
            .add_pane_with_domain(
                true,
                if horizontal {
                    HorizontalDomain::Continuous {
                        scale: ContinuousScaleType::Linear,
                    }
                } else {
                    HorizontalDomain::Category {
                        scale: CategoryScaleType::Band,
                    }
                },
            )
            .unwrap();
        chart
            .add_general_axis(GeneralAxisOptions::new(
                "positive-x",
                pane,
                AxisDimension::X,
                if horizontal {
                    GeneralScaleType::Linear
                } else {
                    GeneralScaleType::Band
                },
            ))
            .unwrap();
        chart
            .add_general_axis(GeneralAxisOptions::new(
                "positive-y",
                pane,
                AxisDimension::Y,
                if horizontal {
                    GeneralScaleType::Band
                } else {
                    GeneralScaleType::Linear
                },
            ))
            .unwrap();
        let make_dataset = |chart: &mut ChartEngine, values| {
            chart
                .create_general_xy_dataset(GeneralXyInput::Category {
                    ids: None,
                    categories: vec!["Up".into(), "Down".into()],
                    category_indices: vec![0, 1],
                    y: values,
                    y_valid: None,
                })
                .unwrap()
        };
        let first_dataset = make_dataset(&mut chart, vec![10.0, -4.0]);
        let second_dataset = make_dataset(&mut chart, vec![5.0, -2.0]);
        let make_options = |dataset| {
            let mut options = if horizontal {
                GeneralSeriesOptions::horizontal_bar(pane, dataset, "positive-x", "positive-y")
            } else {
                GeneralSeriesOptions::column(pane, dataset, "positive-x", "positive-y")
            };
            options.stack_id = Some("positive".into());
            options.stack_mode = GeneralStackMode::Positive;
            options.bar_corner_radius = 5.0;
            options
        };
        let first = chart
            .add_general_series(make_options(first_dataset))
            .unwrap();
        let second = chart
            .add_general_series(make_options(second_dataset))
            .unwrap();
        let value_axis = if horizontal {
            "positive-x"
        } else {
            "positive-y"
        };
        assert_eq!(
            chart.general_axis_effective_domain(value_axis),
            Some(GeneralAxisDomain::Numeric([0.0, 15.0]))
        );
        chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
        let mut first_geometry = Vec::new();
        let mut second_geometry = Vec::new();
        if horizontal {
            chart.visit_general_horizontal_bars(chart.general_series(first).unwrap(), |geometry| {
                first_geometry.push(geometry)
            });
            chart
                .visit_general_horizontal_bars(chart.general_series(second).unwrap(), |geometry| {
                    second_geometry.push(geometry)
                });
        } else {
            chart.visit_general_columns(chart.general_series(first).unwrap(), |geometry| {
                first_geometry.push(geometry)
            });
            chart.visit_general_columns(chart.general_series(second).unwrap(), |geometry| {
                second_geometry.push(geometry)
            });
        }
        assert_eq!(first_geometry.len(), 1);
        assert_eq!(second_geometry.len(), 1);
        assert_eq!(first_geometry[0].row, 0);
        assert_eq!(second_geometry[0].row, 0);
        assert!(!first_geometry[0].exposed_end);
        assert!(second_geometry[0].exposed_end);
        if horizontal {
            assert!((first_geometry[0].right - second_geometry[0].left).abs() < 1e-9);
        } else {
            assert!((first_geometry[0].top - second_geometry[0].bottom).abs() < 1e-9);
        }
        assert_eq!(chart.general_dataset(first_dataset).unwrap().y()[1], -4.0);
    }
}

#[test]
fn wiggle_bars_share_weighted_baselines_with_auto_domain_in_both_orientations() {
    for horizontal in [false, true] {
        let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
        let pane = chart
            .add_pane_with_domain(
                true,
                if horizontal {
                    HorizontalDomain::Continuous {
                        scale: ContinuousScaleType::Linear,
                    }
                } else {
                    HorizontalDomain::Category {
                        scale: CategoryScaleType::Band,
                    }
                },
            )
            .unwrap();
        let (x_axis, y_axis) = if horizontal {
            ("wiggle-value", "wiggle-category")
        } else {
            ("wiggle-category", "wiggle-value")
        };
        chart
            .add_general_axis(GeneralAxisOptions::new(
                x_axis,
                pane,
                AxisDimension::X,
                if horizontal {
                    GeneralScaleType::Linear
                } else {
                    GeneralScaleType::Band
                },
            ))
            .unwrap();
        chart
            .add_general_axis(GeneralAxisOptions::new(
                y_axis,
                pane,
                AxisDimension::Y,
                if horizontal {
                    GeneralScaleType::Band
                } else {
                    GeneralScaleType::Linear
                },
            ))
            .unwrap();
        let mut ids = Vec::new();
        for values in [[2.0, 4.0], [2.0, 2.0]] {
            let dataset = chart
                .create_general_xy_dataset(GeneralXyInput::Category {
                    ids: None,
                    categories: vec!["A".into(), "B".into()],
                    category_indices: vec![0, 1],
                    y: values.to_vec(),
                    y_valid: None,
                })
                .unwrap();
            let mut options = if horizontal {
                GeneralSeriesOptions::horizontal_bar(pane, dataset, x_axis, y_axis)
            } else {
                GeneralSeriesOptions::column(pane, dataset, x_axis, y_axis)
            };
            options.stack_id = Some("same".into());
            options.stack_mode = GeneralStackMode::Wiggle;
            options.bar_corner_radius = 4.0;
            ids.push(chart.add_general_series(options).unwrap());
        }
        let [low, high] = match chart.general_axis_effective_domain("wiggle-value") {
            Some(GeneralAxisDomain::Numeric(domain)) => domain,
            other => panic!("unexpected wiggle domain: {other:?}"),
        };
        // The weighted wiggle extent [-4/3, 14/3] rounds out to clean ticks.
        assert_eq!([low, high], [-2.0, 6.0]);
        chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
        let plot = chart.general_plot_rect(pane).unwrap();
        let mut first = Vec::new();
        let mut second = Vec::new();
        if horizontal {
            chart.visit_general_horizontal_bars(chart.general_series(ids[0]).unwrap(), |mark| {
                first.push(mark)
            });
            chart.visit_general_horizontal_bars(chart.general_series(ids[1]).unwrap(), |mark| {
                second.push(mark)
            });
        } else {
            chart.visit_general_columns(chart.general_series(ids[0]).unwrap(), |mark| {
                first.push(mark)
            });
            chart.visit_general_columns(chart.general_series(ids[1]).unwrap(), |mark| {
                second.push(mark)
            });
        }
        assert_eq!(first.len(), 2);
        assert_eq!(second.len(), 2);
        assert!(first.iter().all(|mark| !mark.exposed_end));
        assert!(second.iter().all(|mark| mark.exposed_end));
        let fraction = (-4.0 / 3.0 - low) / (high - low);
        if horizontal {
            assert!((first[1].left - plot.width * fraction).abs() < 1e-9);
            assert!((first[1].right - second[1].left).abs() < 1e-9);
        } else {
            assert!((first[1].bottom - (plot.y + plot.height * (1.0 - fraction))).abs() < 1e-9);
            assert!((first[1].top - second[1].bottom).abs() < 1e-9);
        }
        let end = second[1];
        let hit = chart
            .general_hit_test(
                pane,
                (end.left + end.right) * 0.5,
                (end.top + end.bottom) * 0.5,
                crate::GeneralHitMode::Exact,
            )
            .unwrap();
        assert_eq!(hit.series, ids[1]);
        assert_eq!(hit.row, 1);
    }
}

#[test]
fn wiggle_domains_include_duplicate_position_overshoots() {
    for area in [false, true] {
        let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
        let pane = chart
            .add_pane_with_domain(
                true,
                if area {
                    HorizontalDomain::Continuous {
                        scale: ContinuousScaleType::Linear,
                    }
                } else {
                    HorizontalDomain::Category {
                        scale: CategoryScaleType::Band,
                    }
                },
            )
            .unwrap();
        chart
            .add_general_axis(GeneralAxisOptions::new(
                "duplicate-x",
                pane,
                AxisDimension::X,
                if area {
                    GeneralScaleType::Linear
                } else {
                    GeneralScaleType::Band
                },
            ))
            .unwrap();
        chart
            .add_general_axis(GeneralAxisOptions::new(
                "duplicate-y",
                pane,
                AxisDimension::Y,
                GeneralScaleType::Linear,
            ))
            .unwrap();
        let dataset = if area {
            chart
                .create_general_xy_dataset(GeneralXyInput::Numeric {
                    ids: None,
                    x: vec![0.0, 1.0, 1.0],
                    y: vec![1.0, 10.0, -9.0],
                    y_valid: None,
                })
                .unwrap()
        } else {
            chart
                .create_general_xy_dataset(GeneralXyInput::Category {
                    ids: None,
                    categories: vec!["A".into(), "B".into()],
                    category_indices: vec![0, 1, 1],
                    y: vec![1.0, 10.0, -9.0],
                    y_valid: None,
                })
                .unwrap()
        };
        let mut options = if area {
            GeneralSeriesOptions::xy_area(pane, dataset, "duplicate-x", "duplicate-y")
        } else {
            GeneralSeriesOptions::column(pane, dataset, "duplicate-x", "duplicate-y")
        };
        options.stack_id = Some("same".into());
        options.stack_mode = GeneralStackMode::Wiggle;
        chart.add_general_series(options).unwrap();
        assert_eq!(
            chart.general_axis_effective_domain("duplicate-y"),
            Some(GeneralAxisDomain::Numeric([0.0, 10.0]))
        );
    }
}

#[test]
fn wiggle_areas_align_boundaries_with_the_auto_domain_and_exact_hit() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "wiggle-area-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "wiggle-area-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let mut ids = Vec::new();
    for values in [[2.0, 4.0], [2.0, 2.0]] {
        let dataset = chart
            .create_general_xy_dataset(GeneralXyInput::Numeric {
                ids: None,
                x: vec![0.0, 1.0],
                y: values.to_vec(),
                y_valid: None,
            })
            .unwrap();
        let mut options =
            GeneralSeriesOptions::xy_area(pane, dataset, "wiggle-area-x", "wiggle-area-y");
        options.stack_id = Some("same".into());
        options.stack_mode = GeneralStackMode::Wiggle;
        ids.push(chart.add_general_series(options).unwrap());
    }
    let [low, high] = match chart.general_axis_effective_domain("wiggle-area-y") {
        Some(GeneralAxisDomain::Numeric(domain)) => domain,
        other => panic!("unexpected wiggle area domain: {other:?}"),
    };
    assert_eq!([low, high], [-2.0, 6.0]);
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut first = Vec::new();
    let mut second = Vec::new();
    chart.visit_general_stacked_area_points(chart.general_series(ids[0]).unwrap(), |mark| {
        first.push(mark)
    });
    chart.visit_general_stacked_area_points(chart.general_series(ids[1]).unwrap(), |mark| {
        second.push(mark)
    });
    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 2);
    assert!((first[1].high_y - second[1].low_y).abs() < 1e-9);
    let plot = chart.general_plot_rect(pane).unwrap();
    let fraction = (-4.0 / 3.0 - low) / (high - low);
    assert!((first[1].low_y - (plot.y + plot.height * (1.0 - fraction))).abs() < 1e-9);
    let hit = chart
        .general_hit_test(
            pane,
            second[1].x - 0.5,
            (second[1].low_y + second[1].high_y) * 0.5,
            crate::GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!(hit.series, ids[1]);
}

#[test]
fn cumulative_and_silhouette_stacks_keep_mixed_sign_prefixes_in_domain_and_geometry() {
    for mode in [GeneralStackMode::Cumulative, GeneralStackMode::Silhouette] {
        let expected_domain = if mode == GeneralStackMode::Silhouette {
            [-5.0, 15.0]
        } else {
            [-5.0, 10.0]
        };
        for horizontal in [false, true] {
            let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
            let pane = chart
                .add_pane_with_domain(
                    true,
                    if horizontal {
                        HorizontalDomain::Continuous {
                            scale: ContinuousScaleType::Linear,
                        }
                    } else {
                        HorizontalDomain::Category {
                            scale: CategoryScaleType::Band,
                        }
                    },
                )
                .unwrap();
            let x_axis = if horizontal {
                "cumulative-value"
            } else {
                "cumulative-category"
            };
            let y_axis = if horizontal {
                "cumulative-category"
            } else {
                "cumulative-value"
            };
            chart
                .add_general_axis(GeneralAxisOptions::new(
                    x_axis,
                    pane,
                    AxisDimension::X,
                    if horizontal {
                        GeneralScaleType::Linear
                    } else {
                        GeneralScaleType::Band
                    },
                ))
                .unwrap();
            chart
                .add_general_axis(GeneralAxisOptions::new(
                    y_axis,
                    pane,
                    AxisDimension::Y,
                    if horizontal {
                        GeneralScaleType::Band
                    } else {
                        GeneralScaleType::Linear
                    },
                ))
                .unwrap();
            let mut ids = Vec::new();
            for values in [[10.0, -3.0], [-15.0, 5.0], [2.0, -4.0]] {
                let dataset = chart
                    .create_general_xy_dataset(GeneralXyInput::Category {
                        ids: None,
                        categories: vec!["A".into(), "B".into()],
                        category_indices: vec![0, 1],
                        y: values.to_vec(),
                        y_valid: None,
                    })
                    .unwrap();
                let mut options = if horizontal {
                    GeneralSeriesOptions::horizontal_bar(pane, dataset, x_axis, y_axis)
                } else {
                    GeneralSeriesOptions::column(pane, dataset, x_axis, y_axis)
                };
                options.stack_id = Some("same".into());
                options.stack_mode = mode;
                options.bar_corner_radius = 4.0;
                ids.push(chart.add_general_series(options).unwrap());
            }
            assert_eq!(
                chart.general_axis_effective_domain("cumulative-value"),
                Some(GeneralAxisDomain::Numeric(expected_domain))
            );
            chart.recompute_layout_with_measure(
                true,
                |text, _| text.len() as f64 * 7.0,
                |_, _| 0.0,
            );
            let mut geometry = Vec::new();
            for id in ids {
                let mut marks = Vec::new();
                if horizontal {
                    chart
                        .visit_general_horizontal_bars(chart.general_series(id).unwrap(), |mark| {
                            marks.push(mark)
                        });
                } else {
                    chart.visit_general_columns(chart.general_series(id).unwrap(), |mark| {
                        marks.push(mark)
                    });
                }
                assert_eq!(marks.len(), 2);
                geometry.push(marks);
            }
            assert!(geometry[0].iter().all(|mark| !mark.exposed_end));
            assert!(geometry[1].iter().all(|mark| !mark.exposed_end));
            assert!(geometry[2].iter().all(|mark| mark.exposed_end));
            let plot = chart.general_plot_rect(pane).unwrap();
            let start = if mode == GeneralStackMode::Silhouette {
                1.5
            } else {
                0.0
            };
            let fraction = (start - expected_domain[0]) / (expected_domain[1] - expected_domain[0]);
            if horizontal {
                assert!((geometry[0][0].left - fraction * plot.width).abs() < 1e-9);
                assert!((geometry[0][0].right - geometry[1][0].right).abs() < 1e-9);
                assert!(geometry[1][0].left < geometry[0][0].left);
            } else {
                assert!(
                    (geometry[0][0].bottom - (plot.y + plot.height * (1.0 - fraction))).abs()
                        < 1e-9
                );
                assert!((geometry[0][0].top - geometry[1][0].top).abs() < 1e-9);
                assert!(geometry[1][0].bottom > geometry[0][0].bottom);
            }
        }

        let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
        let pane = chart
            .add_pane_with_domain(
                true,
                HorizontalDomain::Continuous {
                    scale: ContinuousScaleType::Linear,
                },
            )
            .unwrap();
        chart
            .add_general_axis(GeneralAxisOptions::new(
                "cum-area-x",
                pane,
                AxisDimension::X,
                GeneralScaleType::Linear,
            ))
            .unwrap();
        chart
            .add_general_axis(GeneralAxisOptions::new(
                "cum-area-y",
                pane,
                AxisDimension::Y,
                GeneralScaleType::Linear,
            ))
            .unwrap();
        let mut ids = Vec::new();
        for values in [[10.0, -3.0], [-15.0, 5.0], [2.0, -4.0]] {
            let dataset = chart
                .create_general_xy_dataset(GeneralXyInput::Numeric {
                    ids: None,
                    x: vec![0.0, 1.0],
                    y: values.to_vec(),
                    y_valid: None,
                })
                .unwrap();
            let mut options =
                GeneralSeriesOptions::xy_area(pane, dataset, "cum-area-x", "cum-area-y");
            options.stack_id = Some("same".into());
            options.stack_mode = mode;
            ids.push(chart.add_general_series(options).unwrap());
        }
        assert_eq!(
            chart.general_axis_effective_domain("cum-area-y"),
            Some(GeneralAxisDomain::Numeric(expected_domain))
        );
        chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
        let mut geometry = Vec::new();
        for &id in &ids {
            let mut marks = Vec::new();
            chart.visit_general_stacked_area_points(chart.general_series(id).unwrap(), |mark| {
                marks.push(mark)
            });
            assert_eq!(marks.len(), 2);
            geometry.push(marks);
        }
        for ((first, second), third) in geometry[0].iter().zip(&geometry[1]).zip(&geometry[2]) {
            assert!((second.low_y - first.high_y).abs() < 1e-9);
            assert!((third.low_y - second.high_y).abs() < 1e-9);
        }
        let plot = chart.general_plot_rect(pane).unwrap();
        let start = if mode == GeneralStackMode::Silhouette {
            1.5
        } else {
            0.0
        };
        let fraction = (start - expected_domain[0]) / (expected_domain[1] - expected_domain[0]);
        assert!((geometry[0][0].low_y - (plot.y + plot.height * (1.0 - fraction))).abs() < 1e-9);
        let second = &geometry[1][0];
        let hit = chart
            .general_hit_test(
                pane,
                second.x,
                second.low_y + (second.high_y - second.low_y) * 0.8,
                crate::GeneralHitMode::Exact,
            )
            .unwrap();
        assert_eq!(hit.series, ids[1]);
    }
}

#[test]
fn percent_stacked_columns_normalize_each_category_and_validate_stack_contract() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "percent-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "percent-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();

    let dataset_a = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["Up".into(), "Down".into()],
            category_indices: vec![0, 1],
            y: vec![25.0, -10.0],
            y_valid: None,
        })
        .unwrap();
    let dataset_b = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["Up".into(), "Down".into()],
            category_indices: vec![0, 1],
            y: vec![75.0, -30.0],
            y_valid: None,
        })
        .unwrap();

    let mut invalid = GeneralSeriesOptions::column(pane, dataset_a, "percent-x", "percent-y");
    invalid.stack_mode = GeneralStackMode::Percent;
    assert!(chart.add_general_series(invalid).is_err());

    let mut first = GeneralSeriesOptions::column(pane, dataset_a, "percent-x", "percent-y");
    first.stack_id = Some("share".into());
    first.stack_mode = GeneralStackMode::Percent;
    let first = chart.add_general_series(first).unwrap();
    let mut second = GeneralSeriesOptions::column(pane, dataset_b, "percent-x", "percent-y");
    second.stack_id = Some("share".into());
    second.stack_mode = GeneralStackMode::Percent;
    let second = chart.add_general_series(second).unwrap();
    let mut updated = GeneralSeriesOptions::column(pane, dataset_b, "percent-x", "percent-y");
    updated.stack_id = Some("share".into());
    updated.stack_mode = GeneralStackMode::Percent;
    updated.title = "Updated share".into();
    updated.color = Some("#123456".into());
    chart
        .update_general_series_options(second, updated)
        .unwrap();
    let updated = chart.general_series(second).unwrap();
    assert_eq!(updated.title(), "Updated share");
    assert_eq!(updated.color(), Some("#123456"));

    let mut rejected = GeneralSeriesOptions::column(pane, dataset_b, "percent-x", "percent-y");
    rejected.stack_id = Some("share".into());
    assert!(
        chart
            .update_general_series_options(second, rejected)
            .is_err()
    );
    let unchanged = chart.general_series(second).unwrap();
    assert_eq!(unchanged.title(), "Updated share");
    assert_eq!(unchanged.stack_mode(), GeneralStackMode::Percent);
    let mut incompatible_mode =
        GeneralSeriesOptions::column(pane, dataset_b, "percent-x", "percent-y");
    incompatible_mode.stack_id = Some("share".into());
    assert!(
        chart.add_general_series(incompatible_mode).is_err(),
        "members of one stack must agree on stack mode"
    );
    assert_eq!(
        chart.general_axis_effective_domain("percent-y"),
        Some(GeneralAxisDomain::Numeric([-1.0, 1.0]))
    );

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut first_geometry = Vec::new();
    chart.visit_general_columns(chart.general_series(first).unwrap(), |geometry| {
        first_geometry.push(geometry)
    });
    let mut second_geometry = Vec::new();
    chart.visit_general_columns(chart.general_series(second).unwrap(), |geometry| {
        second_geometry.push(geometry)
    });
    assert!((second_geometry[0].bottom - first_geometry[0].top).abs() < 1e-9);
    assert!((second_geometry[1].top - first_geometry[1].bottom).abs() < 1e-9);
    let plot = chart.general_plot_rect(pane).unwrap();
    assert!((second_geometry[0].top - plot.y).abs() < 1e-9);
    assert!((second_geometry[1].bottom - (plot.y + plot.height)).abs() < 1e-9);

    let mut incompatible = GeneralSeriesOptions::column(pane, dataset_b, "percent-x", "percent-y");
    incompatible.group_id = Some("other".into());
    incompatible.stack_id = Some("share".into());
    incompatible.stack_mode = GeneralStackMode::Normal;
    assert!(
        chart.add_general_series(incompatible).is_ok(),
        "the same stack label in a different group is a distinct stack"
    );
}

#[test]
fn horizontal_bars_use_category_y_geometry_and_stack_on_numeric_x() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "bar-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "bar-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Band,
        ))
        .unwrap();

    let make_dataset = |chart: &mut ChartEngine, values: Vec<f64>| {
        chart
            .create_general_xy_dataset(GeneralXyInput::Category {
                ids: None,
                categories: vec!["A".into(), "B".into()],
                category_indices: vec![0, 1],
                y: values,
                y_valid: None,
            })
            .unwrap()
    };
    let first_dataset = make_dataset(&mut chart, vec![10.0, -4.0]);
    let second_dataset = make_dataset(&mut chart, vec![5.0, -2.0]);
    let mut first = GeneralSeriesOptions::horizontal_bar(pane, first_dataset, "bar-x", "bar-y");
    first.group_id = Some("totals".into());
    first.stack_id = Some("combined".into());
    first.bar_gap = 6.0;
    first.bar_max_width = Some(18.0);
    first.bar_corner_radius = 7.0;
    let first = chart.add_general_series(first).unwrap();
    let mut second = GeneralSeriesOptions::horizontal_bar(pane, second_dataset, "bar-x", "bar-y");
    second.group_id = Some("totals".into());
    second.stack_id = Some("combined".into());
    second.bar_gap = 6.0;
    second.bar_max_width = Some(18.0);
    second.bar_corner_radius = 7.0;
    let second = chart.add_general_series(second).unwrap();
    let mut mismatched =
        GeneralSeriesOptions::horizontal_bar(pane, second_dataset, "bar-x", "bar-y");
    mismatched.group_id = Some("totals".into());
    mismatched.stack_id = Some("combined".into());
    assert!(chart.add_general_series(mismatched).is_err());

    assert_eq!(
        chart.general_axis_effective_domain("bar-y"),
        Some(GeneralAxisDomain::Category(vec!["A".into(), "B".into()]))
    );
    assert_eq!(
        chart.general_axis_effective_domain("bar-x"),
        Some(GeneralAxisDomain::Numeric([-10.0, 15.0]))
    );

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut first_geometry = Vec::new();
    chart.visit_general_horizontal_bars(chart.general_series(first).unwrap(), |geometry| {
        first_geometry.push(geometry)
    });
    let mut second_geometry = Vec::new();
    chart.visit_general_horizontal_bars(chart.general_series(second).unwrap(), |geometry| {
        second_geometry.push(geometry)
    });
    assert_eq!(first_geometry.len(), 2);
    assert_eq!(second_geometry.len(), 2);
    assert!(
        first_geometry[0].top < first_geometry[1].top,
        "vertical category axes list the first category at the top"
    );
    assert!((first_geometry[0].top - second_geometry[0].top).abs() < 1e-9);
    assert!((first_geometry[0].bottom - second_geometry[0].bottom).abs() < 1e-9);
    assert!(
        first_geometry
            .iter()
            .all(|rect| rect.bottom - rect.top <= 18.0 + 1e-9)
    );
    assert!((second_geometry[0].left - first_geometry[0].right).abs() < 1e-9);
    assert!((second_geometry[1].right - first_geometry[1].left).abs() < 1e-9);
    assert!(first_geometry.iter().all(|geometry| !geometry.exposed_end));
    assert!(second_geometry.iter().all(|geometry| geometry.exposed_end));
    assert!(!second_geometry[0].end_at_min);
    assert!(second_geometry[1].end_at_min);

    let hit = chart
        .general_hit_test(
            pane,
            (second_geometry[0].left + second_geometry[0].right) * 0.5,
            (second_geometry[0].top + second_geometry[0].bottom) * 0.5,
            crate::GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!(hit.series, second);
    assert_eq!(
        chart.build_frame().panes[pane]
            .main
            .iter()
            .filter(|primitive| matches!(primitive, Prim::Rect { .. }))
            .count(),
        2
    );
    second_geometry.clear();
    chart.visit_general_horizontal_bars(chart.general_series(second).unwrap(), |geometry| {
        second_geometry.push(geometry)
    });
    assert!(
        chart
            .general_hit_test(
                pane,
                second_geometry[0].right - 0.1,
                second_geometry[0].top + 0.1,
                crate::GeneralHitMode::Exact,
            )
            .is_none(),
        "the rounded horizontal bar corner is outside visible ink"
    );
    let rounded: Vec<[f32; 4]> = chart.build_frame().panes[pane]
        .main
        .iter()
        .filter_map(|primitive| match primitive {
            Prim::RoundRect { radii, .. } => Some(*radii),
            _ => None,
        })
        .collect();
    assert_eq!(rounded.len(), 2);
    assert!(
        rounded
            .iter()
            .any(|r| r[0] == 0.0 && r[1] > 0.0 && r[2] > 0.0 && r[3] == 0.0)
    );
    assert!(
        rounded
            .iter()
            .any(|r| r[0] > 0.0 && r[1] == 0.0 && r[2] == 0.0 && r[3] > 0.0)
    );
}

#[test]
fn horizontal_bar_layout_and_hits_do_not_depend_on_eager_resize_order() {
    let measure = |text: &str, _bold| text.len() as f64 * 7.0;
    let countdown = |_: &str, _bold| 0.0;
    let make_chart = |resize_first: bool| {
        let mut chart = ChartEngine::new_with_initial_domain(
            356.0,
            330.0,
            1.0,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
        if resize_first {
            chart.recompute_layout_with_measure(true, measure, countdown);
        }
        chart
            .add_general_axis(GeneralAxisOptions::new(
                "x",
                0,
                AxisDimension::X,
                GeneralScaleType::Linear,
            ))
            .unwrap();
        chart
            .add_general_axis(GeneralAxisOptions::new(
                "y",
                0,
                AxisDimension::Y,
                GeneralScaleType::Band,
            ))
            .unwrap();
        let dataset = chart
            .create_general_xy_dataset(GeneralXyInput::Category {
                ids: None,
                categories: vec!["Jan".into(), "Feb".into(), "Mar".into()],
                category_indices: vec![0, 1, 2],
                y: vec![6.0, -3.0, 4.0],
                y_valid: None,
            })
            .unwrap();
        let series = chart
            .add_general_series(GeneralSeriesOptions::horizontal_bar(0, dataset, "x", "y"))
            .unwrap();
        chart.recompute_layout_with_measure(!resize_first, measure, countdown);
        chart.build_frame();
        let plot = chart.general_plot_rect(0).unwrap();
        let mut bars = Vec::new();
        chart.visit_general_horizontal_bars(chart.general_series(series).unwrap(), |bar| {
            bars.push(bar);
        });
        let hits = bars
            .iter()
            .map(|bar| {
                chart
                    .general_hit_test(
                        0,
                        (bar.left + bar.right) / 2.0,
                        (bar.top + bar.bottom) / 2.0,
                        crate::GeneralHitMode::Exact,
                    )
                    .unwrap()
                    .row_id
            })
            .collect::<Vec<_>>();
        (plot, bars, hits)
    };
    let after = make_chart(false);
    let before = make_chart(true);
    assert_eq!(
        before.0, after.0,
        "plot geometry must not retain an empty-chart gutter"
    );
    assert_eq!(before.1, after.1, "bar ink must use the resolved plot");
    assert_eq!(before.2, after.2, "exact hits must agree with visible bars");
}

#[test]
fn horizontal_bar_groups_split_band_and_percent_stacks_normalize() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "group-bar-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "group-bar-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Band,
        ))
        .unwrap();
    let make_dataset = |chart: &mut ChartEngine, value: f64| {
        chart
            .create_general_xy_dataset(GeneralXyInput::Category {
                ids: None,
                categories: vec!["Only".into()],
                category_indices: vec![0],
                y: vec![value],
                y_valid: None,
            })
            .unwrap()
    };
    let grouped_a = make_dataset(&mut chart, 2.0);
    let grouped_b = make_dataset(&mut chart, 3.0);
    let mut first =
        GeneralSeriesOptions::horizontal_bar(pane, grouped_a, "group-bar-x", "group-bar-y");
    first.group_id = Some("group".into());
    let first = chart.add_general_series(first).unwrap();
    let mut second =
        GeneralSeriesOptions::horizontal_bar(pane, grouped_b, "group-bar-x", "group-bar-y");
    second.group_id = Some("group".into());
    let second = chart.add_general_series(second).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut first_geometry = Vec::new();
    chart.visit_general_horizontal_bars(chart.general_series(first).unwrap(), |geometry| {
        first_geometry.push(geometry)
    });
    let mut second_geometry = Vec::new();
    chart.visit_general_horizontal_bars(chart.general_series(second).unwrap(), |geometry| {
        second_geometry.push(geometry)
    });
    assert_eq!(first_geometry.len(), 1);
    assert_eq!(second_geometry.len(), 1);
    assert!((first_geometry[0].bottom - second_geometry[0].top).abs() < 1e-9);

    let percent_pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "percent-bar-x",
            percent_pane,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "percent-bar-y",
            percent_pane,
            AxisDimension::Y,
            GeneralScaleType::Band,
        ))
        .unwrap();
    let percent_a = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["Up".into(), "Down".into()],
            category_indices: vec![0, 1],
            y: vec![25.0, -10.0],
            y_valid: None,
        })
        .unwrap();
    let percent_b = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["Up".into(), "Down".into()],
            category_indices: vec![0, 1],
            y: vec![75.0, -30.0],
            y_valid: None,
        })
        .unwrap();
    let mut invalid = GeneralSeriesOptions::horizontal_bar(
        percent_pane,
        percent_a,
        "percent-bar-x",
        "percent-bar-y",
    );
    invalid.stack_mode = GeneralStackMode::Percent;
    assert!(chart.add_general_series(invalid).is_err());
    let mut a = GeneralSeriesOptions::horizontal_bar(
        percent_pane,
        percent_a,
        "percent-bar-x",
        "percent-bar-y",
    );
    a.stack_id = Some("share".into());
    a.stack_mode = GeneralStackMode::Percent;
    let a = chart.add_general_series(a).unwrap();
    let mut b = GeneralSeriesOptions::horizontal_bar(
        percent_pane,
        percent_b,
        "percent-bar-x",
        "percent-bar-y",
    );
    b.stack_id = Some("share".into());
    b.stack_mode = GeneralStackMode::Percent;
    let b = chart.add_general_series(b).unwrap();
    assert_eq!(
        chart.general_axis_effective_domain("percent-bar-x"),
        Some(GeneralAxisDomain::Numeric([-1.0, 1.0]))
    );
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut a_geometry = Vec::new();
    chart.visit_general_horizontal_bars(chart.general_series(a).unwrap(), |geometry| {
        a_geometry.push(geometry)
    });
    let mut b_geometry = Vec::new();
    chart.visit_general_horizontal_bars(chart.general_series(b).unwrap(), |geometry| {
        b_geometry.push(geometry)
    });
    let plot = chart.general_plot_rect(percent_pane).unwrap();
    assert!((b_geometry[0].right - plot.width).abs() < 1e-9);
    assert!(b_geometry[1].left.abs() < 1e-9);
    assert!((b_geometry[0].left - a_geometry[0].right).abs() < 1e-9);
    assert!((b_geometry[1].right - a_geometry[1].left).abs() < 1e-9);
}

#[test]
fn xy_line_preserves_gaps_hits_rows_and_shared_frame_geometry() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    let mut x = GeneralAxisOptions::new("line-x", pane, AxisDimension::X, GeneralScaleType::Linear);
    x.domain = GeneralAxisDomain::Numeric([0.0, 4.0]);
    chart.add_general_axis(x).unwrap();
    let mut y = GeneralAxisOptions::new("line-y", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y.domain = GeneralAxisDomain::Numeric([0.0, 4.0]);
    chart.add_general_axis(y).unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: Some(
                ["a", "b", "gap", "d", "e"]
                    .into_iter()
                    .map(|id| GeneralRowId::Text(id.into()))
                    .collect(),
            ),
            x: vec![0.0, 1.0, 2.0, 3.0, 4.0],
            y: vec![0.0, 1.0, 99.0, 3.0, 4.0],
            y_valid: Some(vec![1, 1, 0, 1, 1]),
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::xy_line(pane, dataset, "line-x", "line-y");
    options.color = Some("#336699".into());
    options.title = "Trend".into();
    options.data_labels = true;
    options.point_markers = true;
    options.point_symbol = GeneralPointSymbol::Diamond;
    options.point_radius = 8.0;
    options.line_width = 4.0;
    options.line_style = GeneralLineStyle::Dashed;
    options.interpolation = GeneralInterpolation::Curved;
    let series = chart.add_general_series(options).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

    let frame = chart.build_frame();
    let mut geometry = Vec::new();
    chart.visit_general_path_points(chart.general_series(series).unwrap(), |point| {
        geometry.push(point)
    });
    assert_eq!(
        geometry.iter().map(|point| point.row).collect::<Vec<_>>(),
        vec![0, 1, 3, 4]
    );
    assert_eq!(
        geometry
            .iter()
            .map(|point| point.starts_new_run)
            .collect::<Vec<_>>(),
        vec![true, false, true, false],
        "missing rows must split the line rather than bridging the gap"
    );

    let line_primitives = frame.panes[pane]
        .main
        .iter()
        .filter(|primitive| matches!(primitive, Prim::Polyline { point_count: 2, .. }))
        .count();
    assert_eq!(line_primitives, 2);
    assert!(frame.panes[pane].main.iter().any(|primitive| {
        matches!(primitive, Prim::Polyline { width, .. } if (*width - 4.0).abs() < f32::EPSILON)
    }));
    assert_eq!(
        frame.panes[pane]
            .main
            .iter()
            .filter(|primitive| matches!(primitive, Prim::Triangle { color, .. } if *color == Color::rgb(0x33, 0x66, 0x99)))
            .count(),
        8,
        "each valid path row must emit one two-triangle diamond above the stroke"
    );
    assert!(frame.panes[pane].main.iter().any(|primitive| {
        matches!(
            primitive,
            Prim::Polyline {
                style: LineStyle::Dashed,
                line_type: LineType::Curved,
                ..
            }
        )
    }));
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::Text { text, .. } if text == "1"))
    );
    assert!(
        !frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::Text { text, .. } if text == "99"))
    );

    let first = geometry[0];
    let second = geometry[1];
    let midpoint = ((first.x + second.x) * 0.5, (first.y + second.y) * 0.5);
    let hit = chart
        .general_hit_test(pane, midpoint.0, midpoint.1, crate::GeneralHitMode::Exact)
        .unwrap();
    assert_eq!(hit.series, series);
    assert_eq!(hit.row, 0);
    assert_eq!(hit.distance, 0.0);

    let marker_only_hit = chart
        .general_hit_test(
            pane,
            first.x + 0.5,
            first.y - 0.5,
            crate::GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!(marker_only_hit.row, 0, "marker geometry must be hittable");

    let gap_midpoint = (
        (geometry[1].x + geometry[2].x) * 0.5,
        (geometry[1].y + geometry[2].y) * 0.5,
    );
    assert_eq!(
        chart.general_hit_test(
            pane,
            gap_midpoint.0,
            gap_midpoint.1,
            crate::GeneralHitMode::Exact,
        ),
        None,
        "a missing row must leave the visual and hit-test gap empty"
    );

    let hovered = chart
        .update_general_hover(pane, midpoint.0, midpoint.1)
        .unwrap();
    assert_eq!(
        (hovered.series, hovered.row),
        (hit.series, hit.row),
        "line hover follows the nearest X datum; ties keep the first row"
    );
    assert!(chart.select_general_hovered());
    let selected = chart.build_frame();
    assert!(selected.panes[pane].main.iter().any(|primitive| {
        matches!(primitive, Prim::Circle { stroke, stroke_width, .. } if *stroke == PRIMARY && *stroke_width >= 2.0)
    }));
    let tooltip = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert_eq!(tooltip.x_label, "0");
    assert_eq!(tooltip.value, Some(0.0));
    assert_eq!(tooltip.title, "Trend");
    let accessibility = chart
        .general_accessibility_snapshot(series, 0, usize::MAX)
        .unwrap();
    assert_eq!(accessibility.items.len(), 5);
    assert_eq!(accessibility.items[2].value, None);

    let mut connected = GeneralSeriesOptions::xy_line(pane, dataset, "line-x", "line-y");
    connected.connect_missing = true;
    chart
        .update_general_series_options(series, connected)
        .unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut connected_geometry = Vec::new();
    chart.visit_general_path_points(chart.general_series(series).unwrap(), |point| {
        connected_geometry.push(point)
    });
    assert_eq!(
        connected_geometry
            .iter()
            .map(|point| point.starts_new_run)
            .collect::<Vec<_>>(),
        vec![true, false, false, false],
        "connect_missing must skip missing rows without losing their queryable identity"
    );
    assert_eq!(
        chart.build_frame().panes[pane]
            .main
            .iter()
            .filter(|primitive| matches!(primitive, Prim::Polyline { point_count: 4, .. }))
            .count(),
        1
    );
    let connected_midpoint = (
        (connected_geometry[1].x + connected_geometry[2].x) * 0.5,
        (connected_geometry[1].y + connected_geometry[2].y) * 0.5,
    );
    assert!(
        chart
            .general_hit_test(
                pane,
                connected_midpoint.0,
                connected_midpoint.1,
                crate::GeneralHitMode::Exact,
            )
            .is_some()
    );

    let mut invalid = GeneralSeriesOptions::scatter(pane, dataset, "line-x", "line-y");
    invalid.connect_missing = true;
    assert!(chart.add_general_series(invalid).is_err());
}

#[test]
fn general_path_interpolation_drives_frame_geometry_and_exact_hits() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    let mut x =
        GeneralAxisOptions::new("curve-x", pane, AxisDimension::X, GeneralScaleType::Linear);
    x.domain = GeneralAxisDomain::Numeric([0.0, 3.0]);
    chart.add_general_axis(x).unwrap();
    let mut y =
        GeneralAxisOptions::new("curve-y", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y.domain = GeneralAxisDomain::Numeric([-3.0, 3.0]);
    chart.add_general_axis(y).unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: vec![0.0, 1.0, 2.0, 3.0],
            y: vec![0.0, 3.0, -3.0, 0.0],
            y_valid: None,
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::xy_line(pane, dataset, "curve-x", "curve-y");
    options.interpolation = GeneralInterpolation::Step;
    let series = chart.add_general_series(options).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

    let mut geometry = Vec::new();
    chart.visit_general_path_points(chart.general_series(series).unwrap(), |point| {
        geometry.push(point)
    });
    let step_probe = ((geometry[0].x + geometry[1].x) * 0.5, geometry[0].y);
    assert_eq!(
        chart
            .general_hit_test(
                pane,
                step_probe.0,
                step_probe.1,
                crate::GeneralHitMode::Exact
            )
            .unwrap()
            .series,
        series
    );
    assert!(
        chart.build_frame().panes[pane]
            .main
            .iter()
            .any(|primitive| {
                matches!(
                    primitive,
                    Prim::Polyline {
                        line_type: LineType::WithSteps,
                        ..
                    }
                )
            })
    );

    for (interpolation, line_type, probe_x) in [
        (
            GeneralInterpolation::StepBefore,
            LineType::StepBefore,
            geometry[0].x,
        ),
        (
            GeneralInterpolation::StepMiddle,
            LineType::StepMiddle,
            (geometry[0].x + geometry[1].x) * 0.5,
        ),
        (
            GeneralInterpolation::StepAfter,
            LineType::WithSteps,
            geometry[1].x,
        ),
    ] {
        let mut updated = GeneralSeriesOptions::xy_line(pane, dataset, "curve-x", "curve-y");
        updated.interpolation = interpolation;
        chart
            .update_general_series_options(series, updated)
            .unwrap();
        assert!(
            chart.build_frame().panes[pane]
                .main
                .iter()
                .any(|primitive| matches!(primitive,
            Prim::Polyline { line_type: actual, .. } if *actual == line_type))
        );
        let probe_y = (geometry[0].y + geometry[1].y) * 0.5;
        assert_eq!(
            chart
                .general_hit_test(pane, probe_x, probe_y, crate::GeneralHitMode::Exact)
                .unwrap()
                .series,
            series
        );
    }

    let mut curved = GeneralSeriesOptions::xy_line(pane, dataset, "curve-x", "curve-y");
    curved.interpolation = GeneralInterpolation::Curved;
    chart.update_general_series_options(series, curved).unwrap();
    assert_eq!(
        chart.general_series(series).unwrap().interpolation(),
        GeneralInterpolation::Curved
    );
    let mut curved_geometry = Vec::new();
    chart.visit_general_path_points(chart.general_series(series).unwrap(), |point| {
        curved_geometry.push(point)
    });
    let source = curved_geometry
        .iter()
        .map(|point| aeris_charts_render::line::LinePoint {
            x: point.x,
            y: point.y,
        })
        .collect::<Vec<_>>();
    let expanded = aeris_charts_render::line::expand_line(&source, LineType::Curved);
    let curve_probe = expanded
        .iter()
        .copied()
        .max_by(|left, right| {
            let distance = |point: aeris_charts_render::line::LinePoint| {
                source
                    .windows(2)
                    .map(|pair| {
                        let dx = pair[1].x - pair[0].x;
                        let dy = pair[1].y - pair[0].y;
                        let length_squared = dx * dx + dy * dy;
                        let position = (((point.x - pair[0].x) * dx + (point.y - pair[0].y) * dy)
                            / length_squared)
                            .clamp(0.0, 1.0);
                        (point.x - (pair[0].x + dx * position))
                            .hypot(point.y - (pair[0].y + dy * position))
                    })
                    .fold(f64::INFINITY, f64::min)
            };
            distance(*left).total_cmp(&distance(*right))
        })
        .unwrap();
    let curve_hit = chart
        .general_hit_test(
            pane,
            curve_probe.x,
            curve_probe.y,
            crate::GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!(curve_hit.distance, 0.0);
    assert_eq!(
        curve_hit.series, series,
        "curved hit testing must follow the rendered spline"
    );
    assert!(
        chart.build_frame().panes[pane]
            .main
            .iter()
            .any(|primitive| {
                matches!(
                    primitive,
                    Prim::Polyline {
                        line_type: LineType::Curved,
                        ..
                    }
                )
            })
    );

    let mut monotone = GeneralSeriesOptions::xy_line(pane, dataset, "curve-x", "curve-y");
    monotone.interpolation = GeneralInterpolation::Monotone;
    chart
        .update_general_series_options(series, monotone)
        .unwrap();
    let monotone_points = aeris_charts_render::line::expand_line(&source, LineType::Monotone);
    let sample = monotone_points
        .iter()
        .find(|point| point.x > source[0].x && point.x < source[1].x)
        .unwrap();
    let hit = chart
        .general_hit_test(pane, sample.x, sample.y, crate::GeneralHitMode::Exact)
        .unwrap();
    assert_eq!(hit.series, series);
    assert_eq!(hit.distance, 0.0);
    assert!(
        chart.build_frame().panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(
                primitive,
                Prim::Polyline {
                    line_type: LineType::Monotone,
                    ..
                }
            ))
    );

    let mut natural = GeneralSeriesOptions::xy_line(pane, dataset, "curve-x", "curve-y");
    natural.interpolation = GeneralInterpolation::Natural;
    chart
        .update_general_series_options(series, natural)
        .unwrap();
    let natural_points = aeris_charts_render::line::expand_line(&source, LineType::Natural);
    let sample = natural_points
        .iter()
        .find(|point| point.x > source[0].x && point.x < source[1].x)
        .unwrap();
    let hit = chart
        .general_hit_test(pane, sample.x, sample.y, crate::GeneralHitMode::Exact)
        .unwrap();
    assert_eq!(hit.series, series);
    assert_eq!(hit.distance, 0.0);
    assert!(
        chart.build_frame().panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(
                primitive,
                Prim::Polyline {
                    line_type: LineType::Natural,
                    ..
                }
            ))
    );

    let mut invalid = GeneralSeriesOptions::scatter(pane, dataset, "curve-x", "curve-y");
    invalid.interpolation = GeneralInterpolation::Step;
    assert!(chart.add_general_series(invalid).is_err());
}

#[test]
fn path_families_expand_their_declared_interpolation_into_hittable_geometry() {
    use aeris_charts_render::line::{LinePoint, expand_band, expand_line};

    // One chart per family/order keeps the hit probe unoccluded by another series.
    for (order, xs) in [
        ("increasing", vec![0.5, 1.5, 2.5, 3.5]),
        ("reversed", vec![3.5, 2.5, 1.5, 0.5]),
        ("duplicate", vec![0.5, 1.5, 1.5, 3.5]),
    ] {
        let mut line_paths: Option<Vec<Vec<[f32; 2]>>> = None;
        for family in ["line", "area", "range"] {
            let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
            let pane = chart
                .add_pane_with_domain(
                    true,
                    HorizontalDomain::Continuous {
                        scale: ContinuousScaleType::Linear,
                    },
                )
                .unwrap();
            let mut x = GeneralAxisOptions::new(
                "evidence-x",
                pane,
                AxisDimension::X,
                GeneralScaleType::Linear,
            );
            x.domain = GeneralAxisDomain::Numeric([0.0, 4.0]);
            chart.add_general_axis(x).unwrap();
            let mut y = GeneralAxisOptions::new(
                "evidence-y",
                pane,
                AxisDimension::Y,
                GeneralScaleType::Linear,
            );
            y.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
            chart.add_general_axis(y).unwrap();
            let values = [3.0, 6.0, 4.0, 7.0];
            let dataset = if family == "range" {
                chart
                    .create_general_xy_dataset(GeneralXyInput::RangeNumeric {
                        ids: None,
                        x: xs.clone(),
                        low: values.iter().map(|value| value - 2.0).collect(),
                        low_valid: None,
                        high: values.to_vec(),
                        high_valid: None,
                    })
                    .unwrap()
            } else {
                chart
                    .create_general_xy_dataset(GeneralXyInput::Numeric {
                        ids: None,
                        x: xs.clone(),
                        y: values.to_vec(),
                        y_valid: None,
                    })
                    .unwrap()
            };
            let make_options = || match family {
                "line" => GeneralSeriesOptions::xy_line(pane, dataset, "evidence-x", "evidence-y"),
                "area" => GeneralSeriesOptions::xy_area(pane, dataset, "evidence-x", "evidence-y"),
                _ => GeneralSeriesOptions::range_area(pane, dataset, "evidence-x", "evidence-y"),
            };
            let series = chart.add_general_series(make_options()).unwrap();
            chart.recompute_layout_with_measure(
                true,
                |text, _| text.len() as f64 * 7.0,
                |_, _| 0.0,
            );

            let mut family_paths = Vec::new();
            for (interpolation, line_type) in [
                (GeneralInterpolation::Linear, LineType::Simple),
                (GeneralInterpolation::Step, LineType::WithSteps),
                (GeneralInterpolation::StepBefore, LineType::StepBefore),
                (GeneralInterpolation::StepMiddle, LineType::StepMiddle),
                (GeneralInterpolation::StepAfter, LineType::WithSteps),
                (GeneralInterpolation::Curved, LineType::Curved),
                (GeneralInterpolation::Monotone, LineType::Monotone),
                (GeneralInterpolation::Natural, LineType::Natural),
            ] {
                let mut options = make_options();
                options.interpolation = interpolation;
                chart
                    .update_general_series_options(series, options)
                    .unwrap();
                let frame = chart.build_frame();
                let pane_frame = &frame.panes[pane];
                let (first, count) = pane_frame
                    .main
                    .iter()
                    .find_map(|primitive| match (family, primitive) {
                        (
                            "line",
                            Prim::Polyline {
                                first_point,
                                point_count,
                                line_type: kind,
                                ..
                            },
                        )
                        | (
                            "area",
                            Prim::Polyline {
                                first_point,
                                point_count,
                                line_type: kind,
                                ..
                            },
                        ) if *kind == line_type => {
                            Some((*first_point as usize, *point_count as usize))
                        }
                        (
                            "range",
                            Prim::BandFill {
                                upper_first: first_point,
                                point_count,
                                line_type: kind,
                                ..
                            },
                        ) if *kind == line_type => {
                            Some((*first_point as usize, *point_count as usize))
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| {
                        panic!("{family}/{order}/{interpolation:?}: missing frame path")
                    });
                assert_eq!(count, values.len());
                family_paths.push(pane_frame.points[first..first + count].to_vec());
                let upper = pane_frame.points[first..first + count]
                    .iter()
                    .map(|point| LinePoint {
                        x: point[0] as f64,
                        y: point[1] as f64,
                    })
                    .collect::<Vec<_>>();
                let expanded = expand_line(&upper, line_type);
                assert!(
                    expanded.len() <= 3 * 16 + 1
                        && expanded.iter().all(|p| p.x.is_finite() && p.y.is_finite()),
                    "{family}/{order}/{interpolation:?}: expansion must stay finite and bounded"
                );
                if family == "area" {
                    assert!(pane_frame.main.iter().any(|prim| matches!(
                        prim, Prim::AreaFill { first_point, point_count, line_type: kind, .. }
                        if *first_point as usize == first && *point_count as usize == count && *kind == line_type
                    )));
                }
                if family == "range" {
                    let lower = pane_frame.points[first + count..first + 2 * count]
                        .iter()
                        .map(|point| LinePoint {
                            x: point[0] as f64,
                            y: point[1] as f64,
                        })
                        .collect::<Vec<_>>();
                    let (upper_band, lower_band) = expand_band(&upper, &lower, line_type);
                    assert!(upper_band.len() <= 3 * 16 + 1);
                    assert!(upper_band.iter().zip(&lower_band).all(|(u, l)| u.x == l.x));
                    // Probe on each side of a join, including an interpolated interior seam.
                    for index in 0..upper_band.len() - 1 {
                        let left = upper_band[index];
                        let right = upper_band[index + 1];
                        if (left.x - right.x).abs() < 0.01 {
                            continue;
                        }
                        let x = (left.x + right.x) * 0.5 - chart.general_plot_rect(pane).unwrap().x;
                        let y = (left.y + right.y + lower_band[index].y + lower_band[index + 1].y)
                            * 0.25;
                        assert_eq!(
                            chart
                                .general_hit_test(pane, x, y, crate::GeneralHitMode::Exact)
                                .map(|hit| hit.series),
                            Some(series),
                            "{family}/{order}/{interpolation:?}: band seam {index}"
                        );
                    }
                } else {
                    // The actual frame point pool is what executors expand; probe a displayed
                    // subsegment, not an independently reconstructed data-space curve.
                    let pair = expanded
                        .windows(2)
                        .find(|pair| {
                            (pair[0].x - pair[1].x).abs() > 0.01 && pair[0].x.min(pair[1].x) > 1.0
                        })
                        .unwrap();
                    let x =
                        (pair[0].x + pair[1].x) * 0.5 - chart.general_plot_rect(pane).unwrap().x;
                    let y = (pair[0].y + pair[1].y) * 0.5;
                    assert_eq!(
                        chart
                            .general_hit_test(pane, x, y, crate::GeneralHitMode::Exact)
                            .map(|hit| hit.series),
                        Some(series),
                        "{family}/{order}/{interpolation:?}: displayed segment ({x}, {y}), source {upper:?}, plot {:?}",
                        chart.general_plot_rect(pane)
                    );
                }
            }
            if let Some(line_paths) = &line_paths {
                assert_eq!(
                    &family_paths, line_paths,
                    "{family}/{order}: upper path must share line geometry across all interpolations"
                );
            } else {
                line_paths = Some(family_paths);
            }
        }
    }
}

#[test]
fn path_missing_rows_bridge_but_log_invalid_rows_remain_hard_gaps() {
    for family in ["line", "area", "range"] {
        for invalid_log_row in [false, true] {
            let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
            let pane = chart
                .add_pane_with_domain(
                    true,
                    HorizontalDomain::Continuous {
                        scale: ContinuousScaleType::Linear,
                    },
                )
                .unwrap();
            let mut x =
                GeneralAxisOptions::new("gap-x", pane, AxisDimension::X, GeneralScaleType::Linear);
            x.domain = GeneralAxisDomain::Numeric([0.0, 5.0]);
            chart.add_general_axis(x).unwrap();
            let mut y = GeneralAxisOptions::new(
                "gap-y",
                pane,
                AxisDimension::Y,
                if invalid_log_row {
                    GeneralScaleType::Logarithmic
                } else {
                    GeneralScaleType::Linear
                },
            );
            y.domain = GeneralAxisDomain::Numeric([1.0, 10.0]);
            chart.add_general_axis(y).unwrap();
            let xs = vec![0.5, 1.5, 2.5, 3.5, 4.5];
            let ids = Some(
                ["first", "left", "gap", "right", "last"]
                    .into_iter()
                    .map(|id| GeneralRowId::Text(id.into()))
                    .collect(),
            );
            let dataset = if family == "range" {
                chart
                    .create_general_xy_dataset(GeneralXyInput::RangeNumeric {
                        ids,
                        x: xs,
                        low: vec![2.0, 3.0, if invalid_log_row { 0.0 } else { 4.0 }, 4.0, 5.0],
                        low_valid: (!invalid_log_row).then_some(vec![1, 1, 0, 1, 1]),
                        high: vec![4.0, 5.0, 6.0, 6.0, 7.0],
                        high_valid: (!invalid_log_row).then_some(vec![1, 1, 0, 1, 1]),
                    })
                    .unwrap()
            } else {
                chart
                    .create_general_xy_dataset(GeneralXyInput::Numeric {
                        ids,
                        x: xs,
                        y: vec![4.0, 5.0, if invalid_log_row { 0.0 } else { 6.0 }, 6.0, 7.0],
                        y_valid: (!invalid_log_row).then_some(vec![1, 1, 0, 1, 1]),
                    })
                    .unwrap()
            };
            let make_options = || match family {
                "line" => GeneralSeriesOptions::xy_line(pane, dataset, "gap-x", "gap-y"),
                "area" => GeneralSeriesOptions::xy_area(pane, dataset, "gap-x", "gap-y"),
                _ => GeneralSeriesOptions::range_area(pane, dataset, "gap-x", "gap-y"),
            };
            let series = chart.add_general_series(make_options()).unwrap();
            chart.recompute_layout_with_measure(
                true,
                |text, _| text.len() as f64 * 7.0,
                |_, _| 0.0,
            );
            for connect in [false, true] {
                let mut options = make_options();
                options.connect_missing = connect;
                chart
                    .update_general_series_options(series, options)
                    .unwrap();
                let mut rows = Vec::new();
                if family == "range" {
                    chart.visit_general_range_points(
                        chart.general_series(series).unwrap(),
                        |point| {
                            rows.push((
                                point.row,
                                point.x,
                                (point.low_y + point.high_y) * 0.5,
                                point.starts_new_run,
                            ));
                        },
                    );
                } else {
                    chart.visit_general_path_points(
                        chart.general_series(series).unwrap(),
                        |point| {
                            rows.push((point.row, point.x, point.y, point.starts_new_run));
                        },
                    );
                }
                assert_eq!(
                    rows.iter().map(|row| row.0).collect::<Vec<_>>(),
                    vec![0, 1, 3, 4]
                );
                let bridges = connect && !invalid_log_row;
                assert_eq!(
                    rows.iter().map(|row| row.3).collect::<Vec<_>>(),
                    vec![true, false, !bridges, false],
                    "{family}: connect={connect}, log-invalid={invalid_log_row}"
                );
                let frame = chart.build_frame();
                let run_sizes = frame.panes[pane]
                    .main
                    .iter()
                    .filter_map(|primitive| match (family, primitive) {
                        ("line", Prim::Polyline { point_count, .. })
                        | ("area", Prim::AreaFill { point_count, .. }) => Some(*point_count),
                        ("range", Prim::BandFill { point_count, .. }) => Some(*point_count),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    run_sizes,
                    if bridges { vec![4] } else { vec![2, 2] },
                    "{family}: emitted runs"
                );
                let midpoint = ((rows[1].1 + rows[2].1) * 0.5, (rows[1].2 + rows[2].2) * 0.5);
                let hit = chart.general_hit_test(
                    pane,
                    midpoint.0,
                    midpoint.1,
                    crate::GeneralHitMode::Exact,
                );
                assert_eq!(
                    hit.map(|hit| hit.series),
                    bridges.then_some(series),
                    "{family}: gap hit"
                );
                let tooltip = chart.general_tooltip_snapshot(series, 2).unwrap();
                assert_eq!(
                    tooltip.row_id,
                    GeneralRowIdentity::Explicit(GeneralRowId::Text("gap".into()))
                );
                assert_eq!(
                    tooltip.value,
                    if invalid_log_row {
                        Some(if family == "range" { 6.0 } else { 0.0 })
                    } else {
                        None
                    }
                );
                if family == "range" {
                    assert_eq!(tooltip.low, if invalid_log_row { Some(0.0) } else { None });
                }
                assert_eq!(
                    chart
                        .general_accessibility_snapshot(series, 0, 10)
                        .unwrap()
                        .total_rows,
                    5
                );
            }
        }
    }
}

#[test]
fn xy_line_maps_temporal_and_category_domains_without_reinterpreting_x() {
    let mut temporal = ChartEngine::new(640.0, 400.0, 1.0);
    let temporal_pane = temporal
        .add_pane_with_domain(true, HorizontalDomain::Temporal)
        .unwrap();
    temporal
        .add_general_axis(GeneralAxisOptions::new(
            "time-x",
            temporal_pane,
            AxisDimension::X,
            GeneralScaleType::Temporal,
        ))
        .unwrap();
    temporal
        .add_general_axis(GeneralAxisOptions::new(
            "time-y",
            temporal_pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let temporal_dataset = temporal
        .create_general_xy_dataset(GeneralXyInput::Temporal {
            ids: None,
            x_epoch_ms: vec![1_000, 2_000, 3_000],
            y: vec![1.0, 2.0, 3.0],
            y_valid: None,
        })
        .unwrap();
    let temporal_series = temporal
        .add_general_series(GeneralSeriesOptions::xy_line(
            temporal_pane,
            temporal_dataset,
            "time-x",
            "time-y",
        ))
        .unwrap();
    temporal.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut temporal_points = Vec::new();
    temporal
        .visit_general_path_points(temporal.general_series(temporal_series).unwrap(), |point| {
            temporal_points.push(point)
        });
    assert_eq!(temporal_points.len(), 3);
    assert_eq!(
        temporal
            .general_tooltip_snapshot(temporal_series, 1)
            .unwrap()
            .x_label,
        "2000"
    );

    let mut category = ChartEngine::new(640.0, 400.0, 1.0);
    let category_pane = category
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Point,
            },
        )
        .unwrap();
    category
        .add_general_axis(GeneralAxisOptions::new(
            "category-x",
            category_pane,
            AxisDimension::X,
            GeneralScaleType::Point,
        ))
        .unwrap();
    category
        .add_general_axis(GeneralAxisOptions::new(
            "category-y",
            category_pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let category_dataset = category
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["A".into(), "B".into(), "C".into()],
            category_indices: vec![0, 1, 2],
            y: vec![3.0, 1.0, 2.0],
            y_valid: None,
        })
        .unwrap();
    let category_series = category
        .add_general_series(GeneralSeriesOptions::xy_line(
            category_pane,
            category_dataset,
            "category-x",
            "category-y",
        ))
        .unwrap();
    category.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut category_points = Vec::new();
    category
        .visit_general_path_points(category.general_series(category_series).unwrap(), |point| {
            category_points.push(point)
        });
    assert_eq!(category_points.len(), 3);
    assert!(category_points.windows(2).all(|pair| pair[0].x < pair[1].x));
    assert_eq!(
        category
            .general_tooltip_snapshot(category_series, 1)
            .unwrap()
            .x_label,
        "B"
    );
    let incompatible = category.replace_general_xy_dataset(
        category_dataset,
        GeneralXyInput::Numeric {
            ids: None,
            x: vec![1.0, 2.0],
            y: vec![10.0, 20.0],
            y_valid: None,
        },
    );
    assert_eq!(
        incompatible.unwrap_err().code(),
        crate::ErrorCode::InvalidOptions
    );
    assert_eq!(
        category
            .general_dataset(category_dataset)
            .unwrap()
            .categories(),
        Some(&["A".to_string(), "B".to_string(), "C".to_string()][..]),
        "a bound XY line must reject an X-kind replacement atomically"
    );
}

#[test]
fn xy_line_log_axes_retain_non_positive_rows_as_geometry_gaps() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Logarithmic,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "log-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Logarithmic,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "log-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Logarithmic,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: Some(
                ["a", "b", "bad-x", "bad-y", "e"]
                    .into_iter()
                    .map(|id| GeneralRowId::Text(id.into()))
                    .collect(),
            ),
            x: vec![1.0, 10.0, 0.0, 100.0, 1_000.0],
            y: vec![1.0, 10.0, 100.0, 0.0, 1_000.0],
            y_valid: None,
        })
        .unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::xy_line(
            pane, dataset, "log-x", "log-y",
        ))
        .unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

    let mut geometry = Vec::new();
    chart.visit_general_path_points(chart.general_series(series).unwrap(), |point| {
        geometry.push(point)
    });
    assert_eq!(
        geometry.iter().map(|point| point.row).collect::<Vec<_>>(),
        vec![0, 1, 4]
    );
    assert_eq!(
        geometry
            .iter()
            .map(|point| point.starts_new_run)
            .collect::<Vec<_>>(),
        vec![true, false, true]
    );
    let snapshot = chart
        .general_accessibility_snapshot(series, 0, usize::MAX)
        .unwrap();
    assert_eq!(snapshot.total_rows, 5);
    assert_eq!(snapshot.items[2].x_label, "0");
    assert_eq!(snapshot.items[3].value, Some(0.0));
}

#[test]
fn polar_angle_labels_sit_outside_the_ring_with_clean_radius_ticks() {
    let mut chart =
        ChartEngine::new_with_initial_domain(520.0, 310.0, 1.0, HorizontalDomain::Polar).unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "angle",
            0,
            AxisDimension::Angle,
            GeneralScaleType::AngularCategory,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "radius",
            0,
            AxisDimension::Radius,
            GeneralScaleType::RadialLinear,
        ))
        .unwrap();
    let categories = ["North", "South", "East", "West", "Online", "Partner"];
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: categories.map(str::to_owned).to_vec(),
            category_indices: (0..6).collect(),
            y: vec![35.0, 56.0, 42.0, 68.0, 51.0, 74.0],
            y_valid: None,
        })
        .unwrap();
    chart
        .add_general_series(GeneralSeriesOptions::radar(0, dataset, "angle", "radius"))
        .unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    assert_eq!(
        chart.general_axis_effective_domain("radius"),
        Some(GeneralAxisDomain::Numeric([0.0, 80.0]))
    );

    let plot = chart.general_plot_rect(0).unwrap();
    let polar = chart.general_polar_plot(0).unwrap();
    let (center_x, center_y) = polar.center();
    let mut axis = AxisFrame::default();
    chart.append_general_axis_frame(&mut axis, &|text, _| text.len() as f64 * 7.0);
    for name in categories {
        let label = axis
            .labels
            .iter()
            .find(|label| label.text == name)
            .unwrap_or_else(|| panic!("{name} angle label must fit"));
        let distance = (label.x - plot.x - center_x).hypot(label.y - center_y);
        assert!(
            distance > polar.outer_radius(),
            "{name} must sit outside the ring, not over the data"
        );
    }
    let radius_ticks: Vec<f64> = axis
        .labels
        .iter()
        .filter_map(|label| label.text.parse().ok())
        .collect();
    assert!(radius_ticks.len() >= 3);
    assert!(radius_ticks.iter().all(|value| value % 20.0 == 0.0));

    let frame = chart.build_frame();
    assert!(frame.panes[0].points.iter().all(|point| {
        (f64::from(point[0]) - plot.x - center_x).hypot(f64::from(point[1]) - center_y)
            <= polar.outer_radius() + 0.5
    }));
}

#[test]
fn general_only_charts_use_recharts_chrome_domains_and_hover() {
    let mut chart = ChartEngine::new_with_initial_domain(
        520.0,
        310.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "x",
            0,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "y",
            0,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: vec![1.0, 2.0, 3.0],
            y: vec![12.0, 47.0, 31.0],
            y_valid: None,
        })
        .unwrap();
    let mut line = GeneralSeriesOptions::xy_line(0, dataset, "x", "y");
    line.point_markers = true;
    chart.add_general_series(line).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

    assert_eq!(
        chart.general_axis_effective_domain("y"),
        Some(GeneralAxisDomain::Numeric([0.0, 52.0])),
        "automatic Y domains start at zero and keep a margin past the clean upper tick"
    );
    assert_eq!(
        chart.general_axis_effective_domain("x"),
        Some(GeneralAxisDomain::Numeric([-0.12, 3.12])),
        "edge points stay off the Y axis line"
    );

    assert_eq!(chart.time_axis_height(), 0.0);
    assert!(!chart.price_scale_visible_for(0, PriceScaleTarget::Right));
    assert!(!chart.price_scale_visible_for(0, PriceScaleTarget::Left));
    let plot = chart.general_plot_rect(0).unwrap();
    let margin = crate::general_axes::general_plot_margin(chart.axis_metrics());
    assert!(
        (520.0 - (plot.x + plot.width) - margin).abs() < 1e-9,
        "no financial price strip is reserved beside a general plot"
    );
    assert!((plot.y - margin).abs() < 1e-9);

    chart.input_pointer_move(
        crate::PointerInput {
            x: plot.x + plot.width / 2.0,
            y: plot.y + plot.height / 2.0,
            ..crate::PointerInput::default()
        },
        false,
    );
    let frame = chart.build_frame();
    let segments = chart.frame_pane_segments(0).unwrap();
    assert!(
        !frame.panes[0].main[segments.trading_end..segments.overlay_end]
            .iter()
            .any(|prim| matches!(prim, Prim::VLine { .. } | Prim::HLine { .. })),
        "general plots paint no crosshair"
    );
    let clip = frame.panes[0].plot_scissor.expect("general plot clip");
    let pane_clip = frame.panes[0].scissor;
    assert!(
        f64::from(clip[1]) < plot.y && f64::from(clip[0]) < plot.x,
        "marks on a domain edge are not cut in half"
    );
    assert!(
        pane_clip[0] <= clip[0] && pane_clip[0] + pane_clip[2] >= clip[0] + clip[2],
        "the pane clip admits the plot overflow it contains"
    );
}

#[test]
fn general_paths_do_not_hit_axis_strips_after_y_view_changes() {
    let mut chart = ChartEngine::new(520.0, 320.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    let mut x = GeneralAxisOptions::new("clip-x", pane, AxisDimension::X, GeneralScaleType::Linear);
    x.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
    chart.add_general_axis(x).unwrap();
    let mut y = GeneralAxisOptions::new("clip-y", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y.domain = GeneralAxisDomain::Numeric([0.0, 20.0]);
    chart.add_general_axis(y).unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: vec![0.0, 5.0, 10.0],
            y: vec![10.0, 10.0, 10.0],
            y_valid: None,
        })
        .unwrap();
    let mut area = GeneralSeriesOptions::xy_area(pane, dataset, "clip-x", "clip-y");
    area.baseline_value = Some(5.0);
    chart.add_general_series(area).unwrap();
    chart
        .add_general_series(GeneralSeriesOptions::xy_line(
            pane, dataset, "clip-x", "clip-y",
        ))
        .unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    chart.zoom_general_axis("clip-y", 3.0, 5.0).unwrap();
    let frame = chart.build_frame();
    let plot = chart.general_plot_rect(pane).unwrap();
    assert!(
        frame.panes[0].plot_scissor.is_none(),
        "financial pane keeps its original scissor"
    );
    let plot_clip = frame.panes[pane].plot_scissor.expect("general plot clip");
    let overflow = crate::general_axes::GENERAL_PLOT_OVERFLOW;
    let pane_top = chart.panes[pane].top;
    assert_eq!(
        plot_clip[1],
        (plot.y - overflow).max(pane_top).round() as u32
    );
    assert_eq!(
        plot_clip[3],
        (plot.y + plot.height + overflow).round() as u32 - plot_clip[1]
    );
    let [start, end] = frame.panes[pane].plot_range;
    assert!(end > start);
    assert!(
        frame.panes[pane].main[start..end]
            .iter()
            .any(|prim| matches!(prim, Prim::Polyline { .. } | Prim::AreaFill { .. }))
    );
    assert!(
        plot_clip[1] + plot_clip[3] < frame.panes[pane].scissor[1] + frame.panes[pane].scissor[3]
    );
    let x = plot.width / 2.0;
    let below = plot.y + plot.height + 8.0;
    assert!(below < chart.panes[pane].top + chart.panes[pane].height);
    assert!(
        chart
            .general_hit_test(
                pane,
                x,
                plot.y + plot.height / 2.0,
                crate::GeneralHitMode::Exact
            )
            .is_some()
    );
    for mode in [
        crate::GeneralHitMode::Exact,
        crate::GeneralHitMode::Nearest { max_distance: 50.0 },
    ] {
        assert_eq!(
            chart.general_hit_test(pane, x, below, mode),
            None,
            "painted geometry and hits must stop at the plot, before axis chrome"
        );
    }
}

#[test]
fn general_resize_after_layout_only_rebuilds_retained_bars_and_axes() {
    let mut chart = ChartEngine::new(640.0, 360.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "resize-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "resize-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["A".into(), "B".into()],
            category_indices: vec![0, 1],
            y: vec![8.0, 6.0],
            y_valid: None,
        })
        .unwrap();
    chart
        .add_general_series(GeneralSeriesOptions::column(
            pane, dataset, "resize-x", "resize-y",
        ))
        .unwrap();
    let mut frame = ChartFrame::default();
    let mut axis = Vec::new();
    let prepare = |chart: &mut ChartEngine,
                   frame: &mut ChartFrame,
                   axis: &mut Vec<Prim>,
                   width,
                   height,
                   layout_only| {
        chart.prepare_financial_frame_with_measure(
            crate::FinancialFrameRequest {
                width,
                height,
                dpr: 1.0,
                force_layout: layout_only,
                allow_axis_shrink: layout_only,
                force_frame: false,
                force_axis: false,
                layout_only,
                fit_content: false,
                frame,
                axis_frame: None,
                axis_primitives: if layout_only { None } else { Some(axis) },
            },
            |text, _| text.len() as f64 * 7.0,
            |text, _| text.len() as f64 * 6.0,
        )
    };
    assert!(prepare(&mut chart, &mut frame, &mut axis, 640.0, 360.0, true).layout_recomputed);
    assert!(prepare(&mut chart, &mut frame, &mut axis, 640.0, 360.0, false).frame_built);
    let original = frame.clone();
    assert!(prepare(&mut chart, &mut frame, &mut axis, 400.0, 240.0, true).layout_recomputed);
    let result = prepare(&mut chart, &mut frame, &mut axis, 400.0, 240.0, false);
    assert!(result.frame_built && result.axis_rebuilt);
    assert_ne!(frame, original);
    assert_eq!(frame.height, chart.pane_h);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
}

#[test]
fn xy_area_emits_fill_and_stroke_runs_and_hits_the_filled_region() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    let mut x = GeneralAxisOptions::new("area-x", pane, AxisDimension::X, GeneralScaleType::Linear);
    x.domain = GeneralAxisDomain::Numeric([0.0, 4.0]);
    chart.add_general_axis(x).unwrap();
    let mut y = GeneralAxisOptions::new("area-y", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y.domain = GeneralAxisDomain::Numeric([0.0, 4.0]);
    chart.add_general_axis(y).unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: Some(
                ["a", "b", "gap", "d", "e"]
                    .into_iter()
                    .map(|id| GeneralRowId::Text(id.into()))
                    .collect(),
            ),
            x: vec![0.0, 1.0, 2.0, 3.0, 4.0],
            y: vec![1.0, 2.0, 99.0, 3.0, 1.0],
            y_valid: Some(vec![1, 1, 0, 1, 1]),
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::xy_area(pane, dataset, "area-x", "area-y");
    options.baseline_value = Some(1.5);
    options.interpolation = GeneralInterpolation::Step;
    let series = chart.add_general_series(options).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

    let frame = chart.build_frame();
    let expected_baseline = chart
        .general_path_baseline_y(chart.general_series(series).unwrap())
        .unwrap();
    assert!(frame.panes[pane].main.iter().any(|primitive| {
        matches!(primitive, Prim::AreaFill { base_y, line_type: LineType::WithSteps, .. } if ((*base_y as f64) - expected_baseline).abs() < f64::EPSILON)
    }));
    assert_eq!(
        frame.panes[pane]
            .main
            .iter()
            .filter(|primitive| matches!(primitive, Prim::AreaFill { point_count: 2, .. }))
            .count(),
        2
    );
    assert_eq!(
        frame.panes[pane]
            .main
            .iter()
            .filter(|primitive| matches!(primitive, Prim::Polyline { point_count: 2, .. }))
            .count(),
        2
    );

    let mut geometry = Vec::new();
    chart.visit_general_path_points(chart.general_series(series).unwrap(), |point| {
        geometry.push(point)
    });
    let baseline = chart
        .general_path_baseline_y(chart.general_series(series).unwrap())
        .unwrap();
    let x = (geometry[0].x + geometry[1].x) * 0.5;
    let top = (geometry[0].y + geometry[1].y) * 0.5;
    let y = (top + baseline) * 0.5;
    let hit = chart
        .general_hit_test(pane, x, y, crate::GeneralHitMode::Exact)
        .unwrap();
    assert_eq!(hit.series, series);
    assert_eq!(hit.distance, 0.0);

    let gap_x = (geometry[1].x + geometry[2].x) * 0.5;
    assert_eq!(
        chart.general_hit_test(pane, gap_x, y, crate::GeneralHitMode::Exact),
        None,
        "missing rows must split area fill and hit geometry"
    );

    let mut domain_baseline = GeneralSeriesOptions::xy_area(pane, dataset, "area-x", "area-y");
    domain_baseline.interpolation = GeneralInterpolation::Step;
    domain_baseline.baseline_policy = GeneralAreaBaseline::DomainMax;
    chart
        .update_general_series_options(series, domain_baseline.clone())
        .unwrap();
    chart.pan_general_axis("area-y", 0.25).unwrap();
    let plot = chart.general_plot_rect(pane).unwrap();
    let top_baseline = chart
        .general_path_baseline_y(chart.general_series(series).unwrap())
        .unwrap();
    assert!((top_baseline - plot.y).abs() < 1e-9);
    assert!(chart.build_frame().panes[pane].main.iter().any(|primitive| {
        matches!(primitive, Prim::AreaFill { base_y, .. } if (*base_y as f64 - plot.y).abs() < 1e-6)
    }));

    chart.reset_general_axis_view("area-y");
    domain_baseline.baseline_policy = GeneralAreaBaseline::DomainMin;
    chart
        .update_general_series_options(series, domain_baseline.clone())
        .unwrap();
    chart.pan_general_axis("area-y", -0.25).unwrap();
    let bottom_baseline = chart
        .general_path_baseline_y(chart.general_series(series).unwrap())
        .unwrap();
    assert!((bottom_baseline - (plot.y + plot.height)).abs() < 1e-9);

    domain_baseline.baseline_value = Some(1.5);
    assert!(
        chart
            .update_general_series_options(series, domain_baseline)
            .is_err()
    );
    assert_eq!(
        chart.general_series(series).unwrap().baseline_policy(),
        GeneralAreaBaseline::DomainMin
    );
}

#[test]
fn xy_area_custom_gradient_reaches_shared_frame_and_validates_atomically() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "gradient-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "gradient-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: vec![0.0, 1.0],
            y: vec![1.0, 2.0],
            y_valid: None,
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::xy_area(pane, dataset, "gradient-x", "gradient-y");
    options.fill_opacity = 0.5;
    options.fill_gradient = Some(["#ff000080".into(), "#0000ff".into()]);
    let series = chart.add_general_series(options.clone()).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    assert!(
        chart.build_frame().panes[pane]
            .main
            .iter()
            .any(|primitive| {
                matches!(primitive, Prim::AreaFill { gradient, .. }
            if gradient.top == Color::rgba(255, 0, 0, 64)
                && gradient.bottom == Color::rgba(0, 0, 255, 128))
            })
    );
    let mut invalid = options.clone();
    invalid.fill_gradient = Some(["bad-color".into(), "#ffffff".into()]);
    assert!(
        chart
            .update_general_series_options(series, invalid)
            .is_err()
    );
    assert_eq!(
        chart.general_series(series).unwrap().fill_gradient(),
        options.fill_gradient.as_ref()
    );
    let mut wrong_kind = options.clone();
    wrong_kind.kind = GeneralSeriesKind::XyLine;
    assert!(
        chart
            .update_general_series_options(series, wrong_kind)
            .is_err()
    );
    let mut stacked = options;
    stacked.stack_id = Some("stack".into());
    chart
        .update_general_series_options(series, stacked)
        .unwrap();
    assert!(
        chart.build_frame().panes[pane]
            .main
            .iter()
            .any(|primitive| {
                matches!(primitive, Prim::BandGradientFill { gradient, .. }
            if gradient.top == Color::rgba(255, 0, 0, 64)
                && gradient.bottom == Color::rgba(0, 0, 255, 128))
            })
    );
    let range_dataset = chart
        .create_general_xy_dataset(GeneralXyInput::RangeNumeric {
            ids: None,
            x: vec![0.0, 1.0],
            low: vec![0.0, 0.5],
            low_valid: None,
            high: vec![1.0, 2.0],
            high_valid: None,
        })
        .unwrap();
    let mut range =
        GeneralSeriesOptions::range_area(pane, range_dataset, "gradient-x", "gradient-y");
    range.fill_opacity = 0.5;
    range.fill_gradient = Some(["#00ff00".into(), "#000000".into()]);
    chart.add_general_series(range).unwrap();
    assert!(
        chart.build_frame().panes[pane]
            .main
            .iter()
            .any(|primitive| {
                matches!(primitive, Prim::BandGradientFill { gradient, .. }
            if gradient.top == Color::rgba(0, 255, 0, 128)
                && gradient.bottom == Color::rgba(0, 0, 0, 128))
            })
    );
}

#[test]
fn xy_area_stacks_by_x_identity_with_normal_and_percent_geometry() {
    fn add_axes(chart: &mut ChartEngine, pane: usize, prefix: &str) {
        chart
            .add_general_axis(GeneralAxisOptions::new(
                format!("{prefix}-x"),
                pane,
                AxisDimension::X,
                GeneralScaleType::Linear,
            ))
            .unwrap();
        chart
            .add_general_axis(GeneralAxisOptions::new(
                format!("{prefix}-y"),
                pane,
                AxisDimension::Y,
                GeneralScaleType::Linear,
            ))
            .unwrap();
    }

    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    add_axes(&mut chart, pane, "stack-area");
    let first_dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: vec![0.0, 1.0, 2.0],
            y: vec![2.0, -2.0, 4.0],
            y_valid: None,
        })
        .unwrap();
    let second_dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: vec![0.0, 1.0, 2.0],
            y: vec![3.0, -1.0, 1.0],
            y_valid: None,
        })
        .unwrap();
    let mut first =
        GeneralSeriesOptions::xy_area(pane, first_dataset, "stack-area-x", "stack-area-y");
    first.stack_id = Some("total".into());
    first.interpolation = GeneralInterpolation::Curved;
    let first = chart.add_general_series(first).unwrap();
    let mut second =
        GeneralSeriesOptions::xy_area(pane, second_dataset, "stack-area-x", "stack-area-y");
    second.stack_id = Some("total".into());
    second.interpolation = GeneralInterpolation::Curved;
    let second = chart.add_general_series(second).unwrap();
    let mut invalid_baseline =
        GeneralSeriesOptions::xy_area(pane, second_dataset, "stack-area-x", "stack-area-y");
    invalid_baseline.stack_id = Some("total".into());
    invalid_baseline.interpolation = GeneralInterpolation::Curved;
    invalid_baseline.baseline_policy = GeneralAreaBaseline::DomainMin;
    let dormant_baseline = chart.add_general_series(invalid_baseline).unwrap();
    assert_eq!(
        chart
            .general_series(dormant_baseline)
            .unwrap()
            .baseline_policy(),
        GeneralAreaBaseline::DomainMin
    );
    chart.remove_general_series(dormant_baseline);
    let mut mismatched =
        GeneralSeriesOptions::xy_area(pane, second_dataset, "stack-area-x", "stack-area-y");
    mismatched.stack_id = Some("total".into());
    mismatched.interpolation = GeneralInterpolation::Step;
    assert!(chart.add_general_series(mismatched).is_err());
    let mut mismatched =
        GeneralSeriesOptions::xy_area(pane, second_dataset, "stack-area-x", "stack-area-y");
    mismatched.stack_id = Some("total".into());
    mismatched.interpolation = GeneralInterpolation::Curved;
    mismatched.connect_missing = true;
    assert!(chart.add_general_series(mismatched).is_err());

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let before = chart.build_frame();
    let before_row = chart.general_tooltip_snapshot(second, 1);
    for (interpolation, connect_missing) in [
        (GeneralInterpolation::Step, false),
        (GeneralInterpolation::Curved, true),
    ] {
        let mut invalid =
            GeneralSeriesOptions::xy_area(pane, second_dataset, "stack-area-x", "stack-area-y");
        invalid.stack_id = Some("total".into());
        invalid.interpolation = interpolation;
        invalid.connect_missing = connect_missing;
        assert!(
            chart
                .update_general_series_options(second, invalid)
                .is_err()
        );
        assert_eq!(
            chart.build_frame(),
            before,
            "rejected stack update changed paint"
        );
        assert_eq!(chart.general_tooltip_snapshot(second, 1), before_row);
        assert_eq!(
            chart.general_series(second).unwrap().interpolation(),
            GeneralInterpolation::Curved
        );
        assert!(!chart.general_series(second).unwrap().connect_missing());
    }

    assert_eq!(
        chart.general_axis_effective_domain("stack-area-y"),
        Some(GeneralAxisDomain::Numeric([-4.0, 6.0]))
    );
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut first_geometry = Vec::new();
    chart.visit_general_stacked_area_points(chart.general_series(first).unwrap(), |point| {
        first_geometry.push(point)
    });
    let mut second_geometry = Vec::new();
    chart.visit_general_stacked_area_points(chart.general_series(second).unwrap(), |point| {
        second_geometry.push(point)
    });
    assert_eq!(first_geometry.len(), 3);
    assert_eq!(second_geometry.len(), 3);
    for row in 0..3 {
        assert!(
            (second_geometry[row].low_y - first_geometry[row].high_y).abs() < 1e-9,
            "the next area must start on the previous area's boundary at row {row}"
        );
    }
    let frame = chart.build_frame();
    assert_eq!(
        frame.panes[pane]
            .main
            .iter()
            .filter(|primitive| matches!(
                primitive,
                Prim::BandFill {
                    point_count: 3,
                    line_type: LineType::Curved,
                    ..
                }
            ))
            .count(),
        2
    );
    assert_eq!(
        frame.panes[pane]
            .main
            .iter()
            .filter(|primitive| matches!(primitive, Prim::AreaFill { .. }))
            .count(),
        0
    );
    let inside_x = second_geometry[0].x;
    let lower = second_geometry[0].low_y;
    let upper = second_geometry[0].high_y;
    let hit = chart
        .general_hit_test(
            pane,
            inside_x,
            (lower + upper) * 0.5,
            crate::GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!(hit.series, second);

    let percent_pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    add_axes(&mut chart, percent_pane, "percent-area");
    let dataset_a = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: vec![0.0, 1.0],
            y: vec![1.0, -3.0],
            y_valid: None,
        })
        .unwrap();
    let dataset_b = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: vec![0.0, 1.0],
            y: vec![3.0, -1.0],
            y_valid: None,
        })
        .unwrap();
    let mut a =
        GeneralSeriesOptions::xy_area(percent_pane, dataset_a, "percent-area-x", "percent-area-y");
    a.stack_id = Some("share".into());
    a.stack_mode = GeneralStackMode::Percent;
    let a = chart.add_general_series(a).unwrap();
    let mut b =
        GeneralSeriesOptions::xy_area(percent_pane, dataset_b, "percent-area-x", "percent-area-y");
    b.stack_id = Some("share".into());
    b.stack_mode = GeneralStackMode::Percent;
    let b = chart.add_general_series(b).unwrap();
    assert_eq!(
        chart.general_axis_effective_domain("percent-area-y"),
        Some(GeneralAxisDomain::Numeric([-1.0, 1.0]))
    );
    let mut incompatible =
        GeneralSeriesOptions::xy_area(percent_pane, dataset_b, "percent-area-x", "percent-area-y");
    incompatible.stack_id = Some("share".into());
    assert!(chart.add_general_series(incompatible).is_err());

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut a_geometry = Vec::new();
    chart.visit_general_stacked_area_points(chart.general_series(a).unwrap(), |point| {
        a_geometry.push(point)
    });
    let mut b_geometry = Vec::new();
    chart.visit_general_stacked_area_points(chart.general_series(b).unwrap(), |point| {
        b_geometry.push(point)
    });
    assert!((b_geometry[0].low_y - a_geometry[0].high_y).abs() < 1e-9);
    assert!((b_geometry[1].low_y - a_geometry[1].high_y).abs() < 1e-9);

    let positive_pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    add_axes(&mut chart, positive_pane, "positive-area");
    let mut positive_a = GeneralSeriesOptions::xy_area(
        positive_pane,
        first_dataset,
        "positive-area-x",
        "positive-area-y",
    );
    positive_a.stack_id = Some("positive".into());
    positive_a.stack_mode = GeneralStackMode::Positive;
    let positive_a = chart.add_general_series(positive_a).unwrap();
    let mut positive_b = GeneralSeriesOptions::xy_area(
        positive_pane,
        second_dataset,
        "positive-area-x",
        "positive-area-y",
    );
    positive_b.stack_id = Some("positive".into());
    positive_b.stack_mode = GeneralStackMode::Positive;
    let positive_b = chart.add_general_series(positive_b).unwrap();
    assert_eq!(
        chart.general_axis_effective_domain("positive-area-y"),
        Some(GeneralAxisDomain::Numeric([0.0, 5.0]))
    );
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut positive_a_geometry = Vec::new();
    let mut positive_b_geometry = Vec::new();
    chart.visit_general_stacked_area_points(chart.general_series(positive_a).unwrap(), |point| {
        positive_a_geometry.push(point)
    });
    chart.visit_general_stacked_area_points(chart.general_series(positive_b).unwrap(), |point| {
        positive_b_geometry.push(point)
    });
    assert_eq!(positive_a_geometry.len(), 3);
    assert_eq!(positive_b_geometry.len(), 3);
    assert!((positive_a_geometry[1].low_y - positive_a_geometry[1].high_y).abs() < 1e-9);
    assert!((positive_b_geometry[1].low_y - positive_b_geometry[1].high_y).abs() < 1e-9);
    assert!((positive_b_geometry[0].low_y - positive_a_geometry[0].high_y).abs() < 1e-9);
}

#[test]
fn xy_scatter_owns_independent_domains_hits_and_runtime_view() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: vec![-10.0, 0.0, 10.0, 20.0],
            y: vec![-5.0, 0.0, 5.0, 99.0],
            y_valid: Some(vec![1, 1, 1, 0]),
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::scatter(pane, dataset, "x", "y");
    options.color = Some("#654321".into());
    options.title = "Samples".into();
    options.point_radius = 4.0;
    options.point_symbol = GeneralPointSymbol::Square;
    options.data_labels = true;
    let series = chart.add_general_series(options).unwrap();

    let close = |chart: &ChartEngine, id: &str, expected: [f64; 2]| {
        let Some(GeneralAxisDomain::Numeric(domain)) = chart.general_axis_effective_domain(id)
        else {
            panic!("{id} has a numeric domain");
        };
        assert!(
            (domain[0] - expected[0]).abs() < 1e-9 && (domain[1] - expected[1]).abs() < 1e-9,
            "{id}: {domain:?}"
        );
    };
    // Scatter extents plus the automatic 4% edge margin.
    close(&chart, "x", [-11.2, 21.2]);
    close(&chart, "y", [-5.4, 5.4]);
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

    let expected_color = Color::parse_css("#654321").unwrap();
    let frame = chart.build_frame();
    assert_eq!(
        frame.panes[pane]
            .main
            .iter()
            .filter(|primitive| matches!(primitive, Prim::RoundRect { fill, .. } if *fill == expected_color))
            .count(),
        3
    );
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::Text { text, .. } if text == "0"))
    );
    assert!(
        !frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::Text { text, .. } if text == "99"))
    );

    chart
        .replace_general_xy_dataset_labeled(
            dataset,
            GeneralXyInput::Numeric {
                ids: None,
                x: vec![-10.0, 0.0, 10.0, 20.0],
                y: vec![-5.0, 0.0, 5.0, 99.0],
                y_valid: Some(vec![1, 1, 1, 0]),
            },
            Some(vec![
                None,
                Some("Midpoint".into()),
                None,
                Some("Hidden".into()),
            ]),
        )
        .unwrap();
    let custom_frame = chart.build_frame();
    assert!(
        custom_frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::Text { text, .. } if text == "Midpoint"))
    );
    assert!(
        !custom_frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::Text { text, .. } if text == "Hidden"))
    );

    let mut geometry = Vec::new();
    chart.visit_general_scatter_points(chart.general_series(series).unwrap(), |point| {
        geometry.push(point)
    });
    assert_eq!(geometry.len(), 3, "missing Y rows emit no scatter mark");
    let first = geometry[0];
    let hit = chart
        .general_hit_test(
            pane,
            first.x + 0.5,
            first.y - 0.5,
            crate::GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!(hit.series, series);
    assert_eq!(hit.row, 0);
    assert_eq!(hit.distance, 0.0);
    let square_corner = chart
        .general_hit_test(
            pane,
            first.x + first.radius * 0.9,
            first.y - first.radius * 0.9,
            crate::GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!(
        square_corner.row, 0,
        "hit geometry must match the square mark"
    );
    let nearest = chart
        .general_hit_test(
            pane,
            first.x + first.radius + 2.0,
            first.y - 0.5,
            crate::GeneralHitMode::Nearest { max_distance: 3.0 },
        )
        .unwrap();
    assert_eq!(nearest.row, 0);
    assert!((nearest.distance - 2.0).abs() < 1e-9);

    let tooltip = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert_eq!(tooltip.x_label, "-10");
    assert_eq!(tooltip.value, Some(-5.0));
    assert_eq!(tooltip.title, "Samples");
    let accessibility = chart
        .general_accessibility_snapshot(series, 0, usize::MAX)
        .unwrap();
    assert_eq!(accessibility.items.len(), 4);
    assert_eq!(accessibility.items[3].x_label, "20");
    assert_eq!(accessibility.items[3].value, None);

    chart.zoom_general_axis("x", 2.0, 5.0).unwrap();
    close(&chart, "x", [-3.1, 13.1]);
    geometry.clear();
    chart.visit_general_scatter_points(chart.general_series(series).unwrap(), |point| {
        geometry.push(point)
    });
    assert_eq!(
        geometry.iter().map(|point| point.row).collect::<Vec<_>>(),
        vec![1, 2]
    );

    chart.pan_general_axis("x", 0.5).unwrap();
    close(&chart, "x", [5.0, 21.2]);
    geometry.clear();
    chart.visit_general_scatter_points(chart.general_series(series).unwrap(), |point| {
        geometry.push(point)
    });
    assert_eq!(
        geometry.iter().map(|point| point.row).collect::<Vec<_>>(),
        vec![2]
    );
    assert!(chart.reset_general_axis_view("x"));
    close(&chart, "x", [-11.2, 21.2]);

    let mut invalid_radius = GeneralSeriesOptions::scatter(pane, dataset, "x", "y");
    invalid_radius.point_radius = 0.5;
    assert_eq!(
        chart.add_general_series(invalid_radius).unwrap_err().code(),
        crate::ErrorCode::InvalidOptions
    );
    assert_eq!(chart.general_series_count(), 1);

    let replacement = chart.replace_general_xy_dataset(
        dataset,
        GeneralXyInput::Category {
            ids: None,
            categories: vec!["bad".into()],
            category_indices: vec![0],
            y: vec![1.0],
            y_valid: None,
        },
    );
    assert_eq!(
        replacement.unwrap_err().code(),
        crate::ErrorCode::InvalidOptions
    );
    assert_eq!(
        chart.general_dataset(dataset).unwrap().numeric_x(),
        Some(&[-10.0, 0.0, 10.0, 20.0][..])
    );

    chart
        .replace_general_xy_dataset(
            dataset,
            GeneralXyInput::Numeric {
                ids: None,
                x: vec![100.0, 200.0],
                y: vec![25.0, 50.0],
                y_valid: None,
            },
        )
        .unwrap();
    geometry.clear();
    chart.visit_general_scatter_points(chart.general_series(series).unwrap(), |point| {
        geometry.push(point)
    });
    assert_eq!(geometry.len(), 2);
    close(&chart, "x", [-8.0, 208.0]);
    assert_eq!(
        chart.general_tooltip_snapshot(series, 0).unwrap().x_label,
        "100"
    );
}

#[test]
fn row_symbols_drive_scatter_frame_and_exact_hits() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    for (id, dimension) in [
        ("symbol-x", AxisDimension::X),
        ("symbol-y", AxisDimension::Y),
    ] {
        chart
            .add_general_axis(GeneralAxisOptions::new(
                id,
                pane,
                dimension,
                GeneralScaleType::Linear,
            ))
            .unwrap();
    }
    let input = GeneralXyInput::Numeric {
        ids: None,
        x: vec![0.0, 5.0, 10.0],
        y: vec![0.0, 5.0, 10.0],
        y_valid: None,
    };
    let dataset = chart.create_general_xy_dataset(input.clone()).unwrap();
    chart
        .replace_general_xy_dataset_styled(
            dataset,
            input,
            None,
            None,
            Some(vec![Some("circle".into()), Some("square".into()), None]),
        )
        .unwrap();
    let mut options = GeneralSeriesOptions::scatter(pane, dataset, "symbol-x", "symbol-y");
    options.point_radius = 10.0;
    let series = chart.add_general_series(options).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let frame = chart.build_frame();
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::Circle { radius, .. } if *radius == 10.0))
    );
    assert!(frame.panes[pane].main.iter().any(
        |primitive| matches!(primitive, Prim::RoundRect { w, h, .. } if *w == 20.0 && *h == 20.0)
    ));
    let mut points = Vec::new();
    chart.visit_general_scatter_points(chart.general_series(series).unwrap(), |point| {
        points.push(point)
    });
    let square = points.iter().find(|point| point.row == 1).unwrap();
    let hit = chart.general_hit_test(
        pane,
        square.x + 9.0,
        square.y + 9.0,
        crate::GeneralHitMode::Exact,
    );
    assert_eq!(hit.map(|hit| hit.row), Some(1));
}

#[test]
fn explicit_row_symbols_and_colors_match_scatter_and_path_marker_hits_after_typed_replace() {
    fn mark_count(
        primitives: &[Prim],
        symbol: GeneralPointSymbol,
        x: f64,
        y: f64,
        color: Color,
    ) -> usize {
        primitives
            .iter()
            .filter(|primitive| match (symbol, primitive) {
                (GeneralPointSymbol::Circle, Prim::Circle { cx, cy, fill, .. }) => {
                    (*cx as f64 - x).abs() < 0.01 && (*cy as f64 - y).abs() < 0.01 && *fill == color
                }
                (
                    GeneralPointSymbol::Square,
                    Prim::RoundRect {
                        x: left,
                        y: top,
                        w,
                        h,
                        fill,
                        ..
                    },
                ) => {
                    (*left as f64 + *w as f64 * 0.5 - x).abs() < 0.01
                        && (*top as f64 + *h as f64 * 0.5 - y).abs() < 0.01
                        && *fill == color
                }
                (
                    GeneralPointSymbol::Diamond | GeneralPointSymbol::Triangle,
                    Prim::Triangle { a, color: fill, .. },
                ) => {
                    (a[0] as f64 - x).abs() < 0.01
                        && (a[1] as f64 - (y - 10.0)).abs() < 0.01
                        && *fill == color
                }
                _ => false,
            })
            .count()
    }
    let symbols = [
        (
            GeneralPointSymbol::Circle,
            "circle",
            (0.0, 9.0),
            (0.0, 12.0),
        ),
        (
            GeneralPointSymbol::Square,
            "square",
            (9.0, 9.0),
            (12.0, 12.0),
        ),
        (
            GeneralPointSymbol::Diamond,
            "diamond",
            (4.0, 4.0),
            (7.0, 7.0),
        ),
        (
            GeneralPointSymbol::Triangle,
            "triangle",
            (7.0, 7.0),
            (10.0, 7.0),
        ),
    ];
    for path in [false, true] {
        for (index, (symbol, _, _, _)) in symbols.iter().enumerate() {
            let override_symbol = symbols[(index + 1) % symbols.len()];
            let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
            let pane = chart
                .add_pane_with_domain(
                    true,
                    HorizontalDomain::Continuous {
                        scale: ContinuousScaleType::Linear,
                    },
                )
                .unwrap();
            for (id, dimension) in [
                ("marker-x", AxisDimension::X),
                ("marker-y", AxisDimension::Y),
            ] {
                let mut axis =
                    GeneralAxisOptions::new(id, pane, dimension, GeneralScaleType::Linear);
                axis.domain = GeneralAxisDomain::Numeric([0.0, 4.0]);
                chart.add_general_axis(axis).unwrap();
            }
            let ids = || {
                ["a", "override", "c"]
                    .into_iter()
                    .map(|id| GeneralRowId::Text(id.into()))
                    .collect()
            };
            let input = GeneralXyInput::Numeric {
                ids: Some(ids()),
                x: vec![0.5, 2.0, 3.5],
                y: vec![2.0; 3],
                y_valid: None,
            };
            let dataset = chart.create_general_xy_dataset(input.clone()).unwrap();
            chart
                .replace_general_xy_dataset_styled(
                    dataset,
                    input,
                    None,
                    Some(vec![None, Some("#f01020".into()), None]),
                    Some(vec![None, Some(override_symbol.1.into()), None]),
                )
                .unwrap();
            let mut options = if path {
                GeneralSeriesOptions::xy_line(pane, dataset, "marker-x", "marker-y")
            } else {
                GeneralSeriesOptions::scatter(pane, dataset, "marker-x", "marker-y")
            };
            options.color = Some("#123456".into());
            options.point_symbol = *symbol;
            options.point_radius = 10.0;
            options.point_markers = path;
            let series = chart.add_general_series(options).unwrap();
            chart.recompute_layout_with_measure(
                true,
                |text, _| text.len() as f64 * 7.0,
                |_, _| 0.0,
            );
            let series_color = Color::rgb(0x12, 0x34, 0x56);
            let override_color = Color::rgb(0xf0, 0x10, 0x20);
            let check = |chart: &mut ChartEngine, expected_rows: &[usize; 3]| {
                let frame = chart.build_frame();
                let prims = &frame.panes[pane].main;
                if path {
                    assert_eq!(
                        prims.iter().filter(|prim| matches!(prim, Prim::Polyline { color, .. } if *color == series_color)).count(),
                        1,
                        "row override must not recolor the path"
                    );
                }
                let mut points = Vec::new();
                if path {
                    chart.visit_general_path_points(
                        chart.general_series(series).unwrap(),
                        |point| {
                            points.push((point.row, point.x, point.y));
                        },
                    );
                } else {
                    chart.visit_general_scatter_points(
                        chart.general_series(series).unwrap(),
                        |point| {
                            points.push((point.row, point.x, point.y));
                        },
                    );
                }
                for (row, expected_symbol, color) in [
                    (expected_rows[0], *symbol, series_color),
                    (expected_rows[1], override_symbol.0, override_color),
                    (expected_rows[2], *symbol, series_color),
                ] {
                    let (_, x, y) = points.iter().find(|point| point.0 == row).unwrap();
                    assert_eq!(
                        mark_count(
                            prims,
                            expected_symbol,
                            *x + chart.general_plot_rect(pane).unwrap().x,
                            *y,
                            color
                        ),
                        if expected_symbol == GeneralPointSymbol::Diamond {
                            2
                        } else {
                            1
                        },
                        "path={path}, base={symbol:?}, row={row}"
                    );
                }
                let (_, x, y) = points
                    .iter()
                    .find(|point| point.0 == expected_rows[1])
                    .unwrap();
                let (inside, outside) = (override_symbol.2, override_symbol.3);
                let hit = chart
                    .general_hit_test(
                        pane,
                        x + inside.0,
                        y + inside.1,
                        crate::GeneralHitMode::Exact,
                    )
                    .unwrap();
                assert_eq!(
                    hit.row_id,
                    GeneralRowIdentity::Explicit(GeneralRowId::Text("override".into()))
                );
                assert_eq!(hit.series, series);
                assert_eq!(
                    chart.general_hit_test(
                        pane,
                        x + outside.0,
                        y + outside.1,
                        crate::GeneralHitMode::Exact
                    ),
                    None,
                    "symbol={:?} must not hit past its boundary",
                    override_symbol.0
                );
            };
            check(&mut chart, &[0, 1, 2]);
            chart
                .replace_general_xy_dataset_styled(
                    dataset,
                    GeneralXyInput::Numeric {
                        ids: Some(vec![
                            GeneralRowId::Text("override".into()),
                            GeneralRowId::Text("a".into()),
                            GeneralRowId::Text("c".into()),
                        ]),
                        x: vec![2.0, 0.5, 3.5],
                        y: vec![2.0; 3],
                        y_valid: None,
                    },
                    None,
                    Some(vec![Some("#f01020".into()), None, None]),
                    Some(vec![Some(override_symbol.1.into()), None, None]),
                )
                .unwrap();
            check(&mut chart, &[1, 0, 2]);
        }
    }
}

#[test]
fn bubble_size_channel_drives_geometry_hits_updates_and_queryable_missing_rows() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    let mut x =
        GeneralAxisOptions::new("bubble-x", pane, AxisDimension::X, GeneralScaleType::Linear);
    x.domain = GeneralAxisDomain::Numeric([0.0, 6.0]);
    chart.add_general_axis(x).unwrap();
    let mut y =
        GeneralAxisOptions::new("bubble-y", pane, AxisDimension::Y, GeneralScaleType::Linear);
    y.domain = GeneralAxisDomain::Numeric([0.0, 6.0]);
    chart.add_general_axis(y).unwrap();

    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Bubble {
            ids: Some(
                ["small", "large", "zero", "missing"]
                    .into_iter()
                    .map(|id| GeneralRowId::Text(id.into()))
                    .collect(),
            ),
            x: vec![1.0, 2.0, 3.0, 4.0],
            y: vec![1.0, 2.0, 3.0, 4.0],
            y_valid: None,
            size: vec![4.0, 100.0, 0.0, 0.0],
            size_valid: Some(vec![1, 1, 1, 0]),
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::bubble(pane, dataset, "bubble-x", "bubble-y");
    options.color = Some("#123456".into());
    options.title = "Bubble samples".into();
    let series = chart.add_general_series(options).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

    let expected_color = Color::parse_css("#123456").unwrap();
    let frame = chart.build_frame();
    let mut geometry = Vec::new();
    chart.visit_general_scatter_points(chart.general_series(series).unwrap(), |point| {
        geometry.push(point)
    });
    assert_eq!(
        geometry.iter().map(|point| point.row).collect::<Vec<_>>(),
        vec![0, 1],
        "zero and missing sizes remain stored but emit no bubble geometry"
    );
    assert_eq!(geometry[0].radius, 2.0);
    assert_eq!(geometry[1].radius, 10.0);

    assert_eq!(
        frame.panes[pane]
            .main
            .iter()
            .filter(|primitive| {
                matches!(primitive, Prim::Circle { fill, .. } if *fill == expected_color)
            })
            .count(),
        2
    );

    let large = geometry[1];
    assert!(
        chart
            .general_scatter_hit_candidate_count(series, large.x, large.y, 0.0)
            .unwrap()
            > 0,
        "the bubble center must be present in the shared screen-space hit index"
    );
    let hit = chart
        .general_hit_test(pane, large.x, large.y, crate::GeneralHitMode::Exact)
        .unwrap();
    assert_eq!(hit.series, series);
    assert_eq!(hit.row, 1);
    assert_eq!(
        hit.row_id,
        crate::GeneralRowIdentity::Explicit(GeneralRowId::Text("large".into()))
    );
    let small = geometry[0];
    assert_eq!(
        chart.general_hit_test(
            pane,
            small.x + small.radius + 1.0,
            small.y,
            crate::GeneralHitMode::Exact,
        ),
        None,
        "exact hits use each bubble's actual radius"
    );

    assert_eq!(
        chart.general_tooltip_snapshot(series, 0).unwrap().size,
        Some(4.0)
    );
    assert_eq!(
        chart.general_tooltip_snapshot(series, 2).unwrap().size,
        Some(0.0)
    );
    assert_eq!(
        chart.general_tooltip_snapshot(series, 3).unwrap().size,
        None
    );
    let accessibility = chart
        .general_accessibility_snapshot(series, 0, usize::MAX)
        .unwrap();
    assert_eq!(accessibility.total_rows, 4);
    assert_eq!(
        accessibility
            .items
            .iter()
            .map(|item| item.size)
            .collect::<Vec<_>>(),
        vec![Some(4.0), Some(100.0), Some(0.0), None]
    );

    let before = chart.general_tooltip_snapshot(series, 1).unwrap();
    assert!(
        chart
            .replace_general_xy_dataset(
                dataset,
                GeneralXyInput::Bubble {
                    ids: Some(
                        ["small", "large", "zero", "missing"]
                            .into_iter()
                            .map(|id| GeneralRowId::Text(id.into()))
                            .collect(),
                    ),
                    x: vec![11.0, 12.0, 13.0, 14.0],
                    y: vec![11.0, 12.0, 13.0, 14.0],
                    y_valid: None,
                    size: vec![4.0, -1.0, 0.0, 0.0],
                    size_valid: Some(vec![1, 1, 1, 0]),
                },
            )
            .is_err(),
        "negative bubble sizes reject atomically"
    );
    assert_eq!(chart.general_tooltip_snapshot(series, 1).unwrap(), before);

    chart
        .upsert_general_xy_dataset(
            dataset,
            GeneralXyInput::Bubble {
                ids: Some(vec![
                    GeneralRowId::Text("large".into()),
                    GeneralRowId::Text("new".into()),
                ]),
                x: vec![2.5, 5.0],
                y: vec![2.5, 5.0],
                y_valid: None,
                size: vec![144.0, 25.0],
                size_valid: None,
            },
            Some(4),
        )
        .unwrap();
    let first = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert_eq!(
        first.row_id,
        crate::GeneralRowIdentity::Explicit(GeneralRowId::Text("large".into()))
    );
    assert_eq!(first.size, Some(144.0));
    let last = chart.general_tooltip_snapshot(series, 3).unwrap();
    assert_eq!(
        last.row_id,
        crate::GeneralRowIdentity::Explicit(GeneralRowId::Text("new".into()))
    );
    assert_eq!(last.size, Some(25.0));

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    geometry.clear();
    chart.visit_general_scatter_points(chart.general_series(series).unwrap(), |point| {
        geometry.push(point)
    });
    assert_eq!(
        geometry.iter().map(|point| point.row).collect::<Vec<_>>(),
        vec![0, 3]
    );
    assert_eq!(geometry[0].radius, 12.0);
    assert_eq!(geometry[1].radius, 5.0);
}

#[test]
fn bubble_log_axes_retain_transform_invalid_rows_as_missing_geometry() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Logarithmic,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "bubble-log-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Logarithmic,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "bubble-log-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Logarithmic,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Bubble {
            ids: None,
            x: vec![1.0, 0.0, 10.0],
            y: vec![1.0, 10.0, 0.0],
            y_valid: None,
            size: vec![9.0, 16.0, 25.0],
            size_valid: None,
        })
        .unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::bubble(
            pane,
            dataset,
            "bubble-log-x",
            "bubble-log-y",
        ))
        .unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

    let mut rows = Vec::new();
    chart.visit_general_scatter_points(chart.general_series(series).unwrap(), |point| {
        rows.push(point.row)
    });
    assert_eq!(rows, vec![0]);
    assert_eq!(
        chart
            .general_accessibility_snapshot(series, 0, usize::MAX)
            .unwrap()
            .total_rows,
        3
    );
}

#[test]
fn range_area_preserves_gaps_fills_band_hits_rows_and_exposes_bounds() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    let mut x =
        GeneralAxisOptions::new("range-x", pane, AxisDimension::X, GeneralScaleType::Linear);
    x.domain = GeneralAxisDomain::Numeric([0.0, 3.0]);
    chart.add_general_axis(x).unwrap();
    let y = GeneralAxisOptions::new("range-y", pane, AxisDimension::Y, GeneralScaleType::Linear);
    chart.add_general_axis(y).unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::RangeNumeric {
            ids: Some(
                ["a", "b", "gap", "d"]
                    .into_iter()
                    .map(|id| GeneralRowId::Text(id.into()))
                    .collect(),
            ),
            x: vec![0.0, 1.0, 2.0, 3.0],
            low: vec![1.0, 2.0, -1_000.0, 4.0],
            low_valid: Some(vec![1, 1, 0, 1]),
            high: vec![3.0, 5.0, 1_000.0, 6.0],
            high_valid: Some(vec![1, 1, 0, 1]),
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::range_area(pane, dataset, "range-x", "range-y");
    options.color = Some("#446688".into());
    options.title = "Interval".into();
    options.interpolation = GeneralInterpolation::Curved;
    options.fill_opacity = 0.5;
    let series = chart.add_general_series(options).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

    let GeneralAxisDomain::Numeric([domain_low, domain_high]) =
        chart.general_axis_effective_domain("range-y").unwrap()
    else {
        panic!("range Y axis must resolve to a numeric domain");
    };
    assert!(domain_low > -100.0 && domain_high < 100.0);

    let frame = chart.build_frame();
    let mut geometry = Vec::new();
    chart.visit_general_range_points(chart.general_series(series).unwrap(), |point| {
        geometry.push(point)
    });
    assert_eq!(
        geometry.iter().map(|point| point.row).collect::<Vec<_>>(),
        vec![0, 1, 3]
    );
    assert_eq!(
        geometry
            .iter()
            .map(|point| point.starts_new_run)
            .collect::<Vec<_>>(),
        vec![true, false, true]
    );
    assert_eq!(
        frame.panes[pane]
            .main
            .iter()
            .filter(|primitive| matches!(
                primitive,
                Prim::BandFill {
                    point_count: 2,
                    line_type: LineType::Curved,
                    fill,
                    ..
                } if fill.a() == 99
            ))
            .count(),
        1
    );

    let from = geometry[0];
    let to = geometry[1];
    // Probe left of the midpoint: an exact midpoint ties between the two rows.
    let hit = chart
        .general_hit_test(
            pane,
            from.x + (to.x - from.x) * 0.4,
            (from.low_y + from.high_y) * 0.3 + (to.low_y + to.high_y) * 0.2,
            crate::GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!(hit.series, series);
    assert_eq!(hit.row, 0);
    assert_eq!(hit.distance, 0.0);

    let gap_x = (geometry[1].x + geometry[2].x) * 0.5;
    let gap_y = (geometry[1].high_y + geometry[2].high_y) * 0.5;
    assert_eq!(
        chart.general_hit_test(pane, gap_x, gap_y, crate::GeneralHitMode::Exact),
        None
    );

    let tooltip = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert_eq!(tooltip.low, Some(1.0));
    assert_eq!(tooltip.high, Some(3.0));
    assert_eq!(tooltip.value, Some(3.0));
    let missing = chart.general_tooltip_snapshot(series, 2).unwrap();
    assert_eq!(missing.low, None);
    assert_eq!(missing.high, None);
    let accessibility = chart
        .general_accessibility_snapshot(series, 0, usize::MAX)
        .unwrap();
    assert_eq!(accessibility.items[1].low, Some(2.0));
    assert_eq!(accessibility.items[1].high, Some(5.0));

    let mut connected = GeneralSeriesOptions::range_area(pane, dataset, "range-x", "range-y");
    connected.connect_missing = true;
    chart
        .update_general_series_options(series, connected)
        .unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut connected_geometry = Vec::new();
    chart.visit_general_range_points(chart.general_series(series).unwrap(), |point| {
        connected_geometry.push(point)
    });
    assert_eq!(
        connected_geometry
            .iter()
            .map(|point| point.starts_new_run)
            .collect::<Vec<_>>(),
        vec![true, false, false]
    );
    assert!(
        chart.build_frame().panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::BandFill { point_count: 3, .. }))
    );

    let before = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert!(
        chart
            .replace_general_xy_dataset(
                dataset,
                GeneralXyInput::RangeNumeric {
                    ids: Some(vec![GeneralRowId::Text("invalid".into())]),
                    x: vec![0.0],
                    low: vec![5.0],
                    low_valid: None,
                    high: vec![4.0],
                    high_valid: None,
                },
            )
            .is_err()
    );
    assert_eq!(chart.general_tooltip_snapshot(series, 0), Some(before));
}

#[test]
fn range_bar_uses_category_band_rectangles_and_exact_hits() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: crate::CategoryScaleType::Band,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "range-bar-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "range-bar-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::RangeCategory {
            ids: None,
            categories: vec!["A".into(), "B".into()],
            category_indices: vec![0, 1],
            low: vec![1.0, 2.0],
            low_valid: None,
            high: vec![3.0, 5.0],
            high_valid: None,
        })
        .unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::range_bar(
            pane,
            dataset,
            "range-bar-x",
            "range-bar-y",
        ))
        .unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    assert_eq!(
        {
            let mut bars = Vec::new();
            chart.visit_general_range_bars(chart.general_series(series).unwrap(), |bar| {
                bars.push(bar)
            });
            bars.len()
        },
        2
    );
    let frame = chart.build_frame();
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::Rect { .. }))
    );
    let mut bars = Vec::new();
    chart.visit_general_range_bars(chart.general_series(series).unwrap(), |bar| bars.push(bar));
    assert_eq!(bars.len(), 2);
    let hit = chart
        .general_hit_test(
            pane,
            (bars[0].left + bars[0].right) * 0.5,
            (bars[0].top + bars[0].bottom) * 0.5,
            crate::GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!(hit.series, series);
    assert_eq!(hit.row, 0);
}

#[test]
fn range_bar_uses_bound_numeric_axis_transform() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "range-log-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .unwrap();
    let mut y = GeneralAxisOptions::new(
        "range-log-y",
        pane,
        AxisDimension::Y,
        GeneralScaleType::Logarithmic,
    );
    y.domain = GeneralAxisDomain::Numeric([1.0, 100.0]);
    chart.add_general_axis(y).unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::RangeCategory {
            ids: None,
            categories: vec!["A".into()],
            category_indices: vec![0],
            low: vec![1.0],
            low_valid: None,
            high: vec![10.0],
            high_valid: None,
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::range_bar(pane, dataset, "range-log-x", "range-log-y");
    options.bar_gap = 4.0;
    options.bar_max_width = Some(16.0);
    let series = chart.add_general_series(options).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let plot = chart.general_plot_rect(pane).unwrap();
    let mut bars = Vec::new();
    chart.visit_general_range_bars(chart.general_series(series).unwrap(), |bar| bars.push(bar));
    assert_eq!(bars.len(), 1);
    assert!(bars[0].right - bars[0].left <= 16.0 + 1e-9);
    assert!((bars[0].top - (plot.y + plot.height * 0.5)).abs() < 1e-9);
    assert!((bars[0].bottom - (plot.y + plot.height)).abs() < 1e-9);
}

fn histogram_chart(
    values: Vec<f64>,
    histogram: crate::GeneralHistogramOptions,
) -> (
    ChartEngine,
    usize,
    crate::GeneralDatasetId,
    crate::GeneralSeriesId,
) {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    for (id, dimension) in [("hist-x", AxisDimension::X), ("hist-y", AxisDimension::Y)] {
        chart
            .add_general_axis(GeneralAxisOptions::new(
                id,
                pane,
                dimension,
                GeneralScaleType::Linear,
            ))
            .unwrap();
    }
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Values {
            ids: None,
            values,
            values_valid: None,
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::histogram(pane, dataset, "hist-x", "hist-y");
    options.histogram = histogram;
    let series = chart.add_general_series(options).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    (chart, pane, dataset, series)
}

#[test]
fn histogram_bins_samples_into_contiguous_bars_with_bin_identity() {
    let (mut chart, pane, _, series) = histogram_chart(
        vec![1.0, 2.0, 2.5, 3.0, 7.0],
        crate::GeneralHistogramOptions {
            bin_count: Some(3),
            ..Default::default()
        },
    );
    let GeneralAxisDomain::Numeric([x_from, x_to]) =
        chart.general_axis_effective_domain("hist-x").unwrap()
    else {
        panic!("histogram X axis must be numeric");
    };
    assert!(x_from <= 1.0 && x_from > 0.0 && (7.0..8.0).contains(&x_to));
    let GeneralAxisDomain::Numeric([y_from, y_to]) =
        chart.general_axis_effective_domain("hist-y").unwrap()
    else {
        panic!("histogram Y axis must be numeric");
    };
    assert!(y_from <= 0.0 && y_to >= 3.0);

    let mut bars = Vec::new();
    chart.visit_general_histogram_bins(chart.general_series(series).unwrap(), |bar| bars.push(bar));
    assert_eq!(bars.len(), 3);
    assert!((bars[0].right - bars[1].left).abs() < 1e-9);
    assert!((bars[1].right - bars[2].left).abs() < 1e-9);
    let height = |bar: &crate::general_series::GeneralColumnGeometry| bar.bottom - bar.top;
    assert!((height(&bars[0]) - 3.0 * height(&bars[1])).abs() < 1e-6);
    assert!((height(&bars[1]) - height(&bars[2])).abs() < 1e-9);
    assert!(
        chart.build_frame().panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::Rect { .. }))
    );

    let hit = chart
        .general_hit_test(
            pane,
            (bars[1].left + bars[1].right) * 0.5,
            (bars[1].top + bars[1].bottom) * 0.5,
            crate::GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!((hit.series, hit.row), (series, 1));
    assert_eq!(hit.row_id, GeneralRowIdentity::Generated(1));

    let first = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert_eq!(first.x_label, "[1, 3)");
    assert_eq!(first.value, Some(3.0));
    assert_eq!((first.x_low, first.x_high), (Some(1.0), Some(3.0)));
    assert_eq!(
        chart.general_tooltip_snapshot(series, 2).unwrap().x_label,
        "[5, 7]"
    );
    assert!(chart.general_tooltip_snapshot(series, 3).is_none());
    let shared = chart.general_shared_tooltip_snapshot(series, 0).unwrap();
    assert_eq!(shared.items.len(), 1);

    let accessibility = chart.general_accessibility_snapshot(series, 0, 10).unwrap();
    assert_eq!(accessibility.total_rows, 3);
    assert_eq!(
        accessibility
            .items
            .iter()
            .map(|item| item.value)
            .collect::<Vec<_>>(),
        vec![Some(3.0), Some(1.0), Some(1.0)]
    );

    let brush = chart
        .set_general_brush_range("hist-x", crate::GeneralBrushRange::Numeric([0.0, 3.5]))
        .unwrap();
    assert_eq!(
        brush.items.iter().map(|item| item.row).collect::<Vec<_>>(),
        vec![0]
    );
}

#[test]
fn histogram_normalization_options_and_bin_bounds_are_validated() {
    let (mut chart, pane, dataset, series) = histogram_chart(
        vec![1.0, 2.0, 2.5, 3.0, 7.0],
        crate::GeneralHistogramOptions {
            bin_count: Some(3),
            normalization: crate::GeneralHistogramNormalization::Density,
            ..Default::default()
        },
    );
    let density = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert!((density.value.unwrap() - 0.3).abs() < 1e-12);

    let mut options = GeneralSeriesOptions::histogram(pane, dataset, "hist-x", "hist-y");
    options.histogram = crate::GeneralHistogramOptions {
        bin_count: Some(3),
        normalization: crate::GeneralHistogramNormalization::Percent,
        cumulative: true,
        ..Default::default()
    };
    chart
        .update_general_series_options(series, options.clone())
        .unwrap();
    let cumulative = (0..3)
        .map(|bin| chart.general_tooltip_snapshot(series, bin).unwrap().value)
        .collect::<Vec<_>>();
    assert_eq!(cumulative, vec![Some(60.0), Some(80.0), Some(100.0)]);

    options.histogram.bin_count = Some(0);
    assert_eq!(
        chart
            .update_general_series_options(series, options.clone())
            .unwrap_err()
            .code(),
        crate::ErrorCode::InvalidOptions
    );
    options.histogram = crate::GeneralHistogramOptions {
        bin_width: Some(0.001),
        ..Default::default()
    };
    assert_eq!(
        chart
            .update_general_series_options(series, options.clone())
            .unwrap_err()
            .code(),
        crate::ErrorCode::InvalidData
    );

    options.histogram = crate::GeneralHistogramOptions {
        bin_width: Some(0.5),
        ..Default::default()
    };
    chart
        .update_general_series_options(series, options)
        .unwrap();
    assert_eq!(
        chart
            .replace_general_xy_dataset(
                dataset,
                GeneralXyInput::Values {
                    ids: None,
                    values: vec![0.0, 1_000.0],
                    values_valid: None,
                },
            )
            .unwrap_err()
            .code(),
        crate::ErrorCode::InvalidData
    );
    assert_eq!(
        chart
            .upsert_general_xy_dataset(
                dataset,
                GeneralXyInput::Values {
                    ids: Some(vec![GeneralRowId::Text("far".into())]),
                    values: vec![1_000.0],
                    values_valid: None,
                },
                None,
            )
            .unwrap_err()
            .code(),
        crate::ErrorCode::InvalidData
    );
    let unchanged = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert_eq!(unchanged.x_label, "[1, 1.5)");
    assert_eq!(unchanged.value, Some(1.0));
}

#[test]
fn histogram_options_are_rejected_on_other_series_kinds() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "column-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "column-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["A".into()],
            category_indices: vec![0],
            y: vec![1.0],
            y_valid: None,
        })
        .unwrap();
    let mut column = GeneralSeriesOptions::column(pane, dataset, "column-x", "column-y");
    column.histogram.cumulative = true;
    assert_eq!(
        chart.add_general_series(column.clone()).unwrap_err().code(),
        crate::ErrorCode::InvalidOptions
    );
    column.histogram = crate::GeneralHistogramOptions::default();
    chart.add_general_series(column).unwrap();
}

#[test]
fn range_area_maps_temporal_and_category_x_and_log_invalid_bounds_as_gaps() {
    let mut temporal = ChartEngine::new(640.0, 400.0, 1.0);
    let temporal_pane = temporal
        .add_pane_with_domain(true, HorizontalDomain::Temporal)
        .unwrap();
    temporal
        .add_general_axis(GeneralAxisOptions::new(
            "range-time-x",
            temporal_pane,
            AxisDimension::X,
            GeneralScaleType::Temporal,
        ))
        .unwrap();
    temporal
        .add_general_axis(GeneralAxisOptions::new(
            "range-time-y",
            temporal_pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let temporal_data = temporal
        .create_general_xy_dataset(GeneralXyInput::RangeTemporal {
            ids: None,
            x_epoch_ms: vec![1_700_000_000_000, 1_700_000_060_000],
            low: vec![8.0, 9.0],
            low_valid: None,
            high: vec![12.0, 14.0],
            high_valid: None,
        })
        .unwrap();
    let temporal_series = temporal
        .add_general_series(GeneralSeriesOptions::range_area(
            temporal_pane,
            temporal_data,
            "range-time-x",
            "range-time-y",
        ))
        .unwrap();
    temporal.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut temporal_geometry = Vec::new();
    temporal
        .visit_general_range_points(temporal.general_series(temporal_series).unwrap(), |point| {
            temporal_geometry.push(point)
        });
    assert_eq!(temporal_geometry.len(), 2);

    let mut category = ChartEngine::new(640.0, 400.0, 1.0);
    let category_pane = category
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Point,
            },
        )
        .unwrap();
    category
        .add_general_axis(GeneralAxisOptions::new(
            "range-category-x",
            category_pane,
            AxisDimension::X,
            GeneralScaleType::Point,
        ))
        .unwrap();
    category
        .add_general_axis(GeneralAxisOptions::new(
            "range-category-y",
            category_pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let category_data = category
        .create_general_xy_dataset(GeneralXyInput::RangeCategory {
            ids: None,
            categories: vec!["A".into(), "B".into()],
            category_indices: vec![0, 1],
            low: vec![1.0, 2.0],
            low_valid: None,
            high: vec![3.0, 4.0],
            high_valid: None,
        })
        .unwrap();
    let category_series = category
        .add_general_series(GeneralSeriesOptions::range_area(
            category_pane,
            category_data,
            "range-category-x",
            "range-category-y",
        ))
        .unwrap();
    category.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut category_geometry = Vec::new();
    category
        .visit_general_range_points(category.general_series(category_series).unwrap(), |point| {
            category_geometry.push(point)
        });
    assert_eq!(category_geometry.len(), 2);

    let mut log_chart = ChartEngine::new(640.0, 400.0, 1.0);
    let log_pane = log_chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Logarithmic,
            },
        )
        .unwrap();
    log_chart
        .add_general_axis(GeneralAxisOptions::new(
            "range-log-x",
            log_pane,
            AxisDimension::X,
            GeneralScaleType::Logarithmic,
        ))
        .unwrap();
    log_chart
        .add_general_axis(GeneralAxisOptions::new(
            "range-log-y",
            log_pane,
            AxisDimension::Y,
            GeneralScaleType::Logarithmic,
        ))
        .unwrap();
    let log_data = log_chart
        .create_general_xy_dataset(GeneralXyInput::RangeNumeric {
            ids: None,
            x: vec![1.0, 10.0, 100.0],
            low: vec![1.0, -1.0, 10.0],
            low_valid: None,
            high: vec![2.0, 5.0, 20.0],
            high_valid: None,
        })
        .unwrap();
    let mut log_options =
        GeneralSeriesOptions::range_area(log_pane, log_data, "range-log-x", "range-log-y");
    log_options.connect_missing = true;
    let log_series = log_chart.add_general_series(log_options).unwrap();
    log_chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut log_geometry = Vec::new();
    log_chart.visit_general_range_points(log_chart.general_series(log_series).unwrap(), |point| {
        log_geometry.push(point)
    });
    assert_eq!(
        log_geometry
            .iter()
            .map(|point| point.row)
            .collect::<Vec<_>>(),
        vec![0, 2]
    );
    assert!(
        log_geometry.iter().all(|point| point.starts_new_run),
        "transform-invalid bounds must remain hard gaps even when missing rows connect"
    );
    assert_eq!(
        log_chart
            .general_tooltip_snapshot(log_series, 1)
            .unwrap()
            .low,
        Some(-1.0)
    );
}

#[test]
fn error_bar_owns_xy_bounds_autoscale_geometry_hits_snapshots_and_atomic_updates() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "error-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "error-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::ErrorNumeric {
            ids: Some(
                ["full", "one-sided", "missing"]
                    .into_iter()
                    .map(|id| GeneralRowId::Text(id.into()))
                    .collect(),
            ),
            x: vec![10.0, 20.0, 30.0],
            y: vec![20.0, 30.0, 1_000.0],
            y_valid: Some(vec![1, 1, 0]),
            x_low: vec![8.0, 0.0, -1_000.0],
            x_low_valid: Some(vec![1, 0, 1]),
            x_high: vec![13.0, 25.0, 1_000.0],
            x_high_valid: None,
            y_low: vec![15.0, 24.0, -1_000.0],
            y_low_valid: None,
            y_high: vec![26.0, 0.0, 2_000.0],
            y_high_valid: Some(vec![1, 0, 1]),
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::error_bar(pane, dataset, "error-x", "error-y");
    options.color = Some("#345678".into());
    options.title = "Confidence".into();
    let series = chart.add_general_series(options).unwrap();

    let GeneralAxisDomain::Numeric([x_low, x_high]) =
        chart.general_axis_effective_domain("error-x").unwrap()
    else {
        panic!("error X axis must be numeric");
    };
    assert!(x_low <= 8.0 && (30.0..100.0).contains(&x_high));
    let GeneralAxisDomain::Numeric([y_low, y_high]) =
        chart.general_axis_effective_domain("error-y").unwrap()
    else {
        panic!("error Y axis must be numeric");
    };
    assert!(y_low <= 15.0 && (30.0..100.0).contains(&y_high));

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut geometry = Vec::new();
    chart.visit_general_error_bars(chart.general_series(series).unwrap(), |item| {
        geometry.push(item)
    });
    assert_eq!(
        geometry.iter().map(|item| item.row).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert!(geometry[0].x_low.is_some());
    assert!(geometry[0].x_high.is_some());
    assert!(geometry[0].y_low.is_some());
    assert!(geometry[0].y_high.is_some());
    assert!(geometry[1].x_low.is_none());
    assert!(geometry[1].x_high.is_some());
    assert!(geometry[1].y_low.is_some());
    assert!(geometry[1].y_high.is_none());

    let expected = Color::parse_css("#345678").unwrap();
    let frame = chart.build_frame();
    geometry.clear();
    chart.visit_general_error_bars(chart.general_series(series).unwrap(), |item| {
        geometry.push(item)
    });
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::HLine { color, .. } if *color == expected))
    );
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::VLine { color, .. } if *color == expected))
    );
    assert_eq!(
        frame.panes[pane]
            .main
            .iter()
            .filter(|primitive| matches!(primitive, Prim::Circle { fill, .. } if *fill == expected))
            .count(),
        2
    );

    let first = geometry[0];
    let hit_x = (first.x + first.x_low.unwrap()) * 0.5;
    let hit = chart
        .general_hit_test(pane, hit_x, first.y, crate::GeneralHitMode::Exact)
        .unwrap();
    assert_eq!(hit.series, series);
    assert_eq!(hit.row, 0);
    let tooltip = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert_eq!(tooltip.value, Some(20.0));
    assert_eq!(tooltip.x_low, Some(8.0));
    assert_eq!(tooltip.x_high, Some(13.0));
    assert_eq!(tooltip.low, Some(15.0));
    assert_eq!(tooltip.high, Some(26.0));
    let accessibility = chart
        .general_accessibility_snapshot(series, 0, usize::MAX)
        .unwrap();
    assert_eq!(accessibility.items[1].x_low, None);
    assert_eq!(accessibility.items[1].x_high, Some(25.0));
    assert_eq!(accessibility.items[1].low, Some(24.0));
    assert_eq!(accessibility.items[1].high, None);

    let before = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert!(
        chart
            .replace_general_xy_dataset(
                dataset,
                GeneralXyInput::ErrorNumeric {
                    ids: Some(vec![GeneralRowId::Text("bad".into())]),
                    x: vec![10.0],
                    y: vec![20.0],
                    y_valid: None,
                    x_low: vec![11.0],
                    x_low_valid: None,
                    x_high: vec![13.0],
                    x_high_valid: None,
                    y_low: vec![15.0],
                    y_low_valid: None,
                    y_high: vec![26.0],
                    y_high_valid: None,
                },
            )
            .is_err()
    );
    assert_eq!(chart.general_tooltip_snapshot(series, 0), Some(before));

    chart
        .upsert_general_xy_dataset(
            dataset,
            GeneralXyInput::ErrorNumeric {
                ids: Some(vec![GeneralRowId::Text("full".into())]),
                x: vec![12.0],
                y: vec![22.0],
                y_valid: None,
                x_low: vec![9.0],
                x_low_valid: None,
                x_high: vec![14.0],
                x_high_valid: None,
                y_low: vec![16.0],
                y_low_valid: None,
                y_high: vec![28.0],
                y_high_valid: None,
            },
            Some(3),
        )
        .unwrap();
    let updated = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert_eq!(updated.value, Some(22.0));
    assert_eq!(updated.x_low, Some(9.0));
    assert_eq!(updated.high, Some(28.0));
}

#[test]
fn category_error_bars_share_band_and_point_axes_without_numeric_x_bounds() {
    for (category_scale, axis_scale) in [
        (CategoryScaleType::Band, GeneralScaleType::Band),
        (CategoryScaleType::Point, GeneralScaleType::Point),
    ] {
        let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
        let pane = chart
            .add_pane_with_domain(
                true,
                HorizontalDomain::Category {
                    scale: category_scale,
                },
            )
            .unwrap();
        chart
            .add_general_axis(GeneralAxisOptions::new(
                "category-x",
                pane,
                AxisDimension::X,
                axis_scale,
            ))
            .unwrap();
        chart
            .add_general_axis(GeneralAxisOptions::new(
                "category-y",
                pane,
                AxisDimension::Y,
                GeneralScaleType::Linear,
            ))
            .unwrap();
        let dataset = chart
            .create_general_xy_dataset(GeneralXyInput::ErrorCategory {
                ids: Some(vec![
                    GeneralRowId::Text("first".into()),
                    GeneralRowId::Text("second".into()),
                    GeneralRowId::Text("missing".into()),
                ]),
                categories: vec!["Q1".into(), "Q2".into(), "Q3".into()],
                category_indices: vec![0, 1, 2],
                y: vec![20.0, 30.0, 1000.0],
                y_valid: Some(vec![1, 1, 0]),
                y_low: vec![15.0, 25.0, -1000.0],
                y_low_valid: None,
                y_high: vec![26.0, 0.0, 2000.0],
                y_high_valid: Some(vec![1, 0, 1]),
            })
            .unwrap();
        let series = chart
            .add_general_series(GeneralSeriesOptions::error_bar(
                pane,
                dataset,
                "category-x",
                "category-y",
            ))
            .unwrap();
        let GeneralAxisDomain::Numeric([low, high]) =
            chart.general_axis_effective_domain("category-y").unwrap()
        else {
            panic!("category error-bar Y domain must be numeric");
        };
        assert!(low <= 15.0 && (30.0..100.0).contains(&high));
        chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
        let frame = chart.build_frame();
        let mut geometry = Vec::new();
        chart.visit_general_error_bars(chart.general_series(series).unwrap(), |bar| {
            geometry.push(bar)
        });
        assert_eq!(geometry.len(), 2);
        assert!(
            geometry
                .iter()
                .all(|bar| bar.x_low.is_none() && bar.x_high.is_none())
        );
        assert!(geometry[0].x < geometry[1].x);
        assert_eq!(geometry[1].y_high, None);
        assert_eq!(
            frame.panes[pane]
                .main
                .iter()
                .filter(|prim| matches!(prim, Prim::Circle { .. }))
                .count(),
            2
        );
        let hit = chart
            .general_hit_test(
                pane,
                geometry[0].x,
                geometry[0].y_low.unwrap() - 0.5,
                crate::GeneralHitMode::Exact,
            )
            .unwrap();
        assert_eq!((hit.series, hit.row), (series, 0));
        let tooltip = chart.general_tooltip_snapshot(series, 0).unwrap();
        assert_eq!(tooltip.x_label, "Q1");
        assert_eq!(
            (tooltip.x_low, tooltip.x_high, tooltip.low, tooltip.high),
            (None, None, Some(15.0), Some(26.0))
        );
        assert!(
            chart
                .replace_general_xy_dataset(
                    dataset,
                    GeneralXyInput::ErrorCategory {
                        ids: None,
                        categories: vec!["Q1".into()],
                        category_indices: vec![0],
                        y: vec![20.0],
                        y_valid: None,
                        y_low: vec![21.0],
                        y_low_valid: None,
                        y_high: vec![26.0],
                        y_high_valid: None,
                    }
                )
                .is_err()
        );
        assert_eq!(chart.general_tooltip_snapshot(series, 0), Some(tooltip));
    }
}

#[test]
fn category_box_plots_validate_autoscale_geometry_hits_and_atomic_updates() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "box-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "box-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::BoxCategory {
            ids: Some(vec![
                GeneralRowId::Text("north".into()),
                GeneralRowId::Text("south".into()),
                GeneralRowId::Text("missing".into()),
            ]),
            categories: vec!["North".into(), "South".into(), "Missing".into()],
            category_indices: vec![0, 1, 2],
            min: vec![5.0, 12.0, -1000.0],
            min_valid: None,
            q1: vec![10.0, 18.0, -500.0],
            q1_valid: None,
            median: vec![15.0, 24.0, 0.0],
            median_valid: Some(vec![1, 1, 0]),
            q3: vec![20.0, 30.0, 500.0],
            q3_valid: None,
            max: vec![28.0, 40.0, 1000.0],
            max_valid: None,
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::box_plot(pane, dataset, "box-x", "box-y");
    options.title = "Distribution".into();
    options.color = Some("#345678".into());
    options.box_fill_color = Some("#112233".into());
    options.box_median_color = Some("#ff00ff".into());
    options.line_width = 3.0;
    let series = chart.add_general_series(options).unwrap();

    let GeneralAxisDomain::Numeric([low, high]) =
        chart.general_axis_effective_domain("box-y").unwrap()
    else {
        panic!("box-plot Y domain must be numeric");
    };
    assert!(low <= 5.0 && high >= 40.0);
    assert!(low > -100.0 && high < 100.0);

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut geometry = Vec::new();
    chart.visit_general_box_plots(chart.general_series(series).unwrap(), |item| {
        geometry.push(item)
    });
    assert_eq!(
        geometry.iter().map(|item| item.row).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert!(geometry[0].left < geometry[0].center_x);
    assert!(geometry[0].center_x < geometry[0].right);
    assert!(geometry[0].max_y < geometry[0].q3_y);
    assert!(geometry[0].q3_y < geometry[0].median_y);
    assert!(geometry[0].median_y < geometry[0].q1_y);
    assert!(geometry[0].q1_y < geometry[0].min_y);

    let frame = chart.build_frame();
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::Rect { .. }))
    );
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive,
        Prim::Rect { color, .. } if *color == Color::rgb(0x11, 0x22, 0x33)))
    );
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive,
        Prim::HLine { color, width, .. } if *color == Color::rgb(0xff, 0, 0xff) && *width == 3))
    );
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::HLine { .. }))
    );
    assert!(
        frame.panes[pane]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::VLine { .. }))
    );

    let first = geometry[0];
    let hit = chart
        .general_hit_test(
            pane,
            first.center_x,
            first.median_y,
            crate::GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!((hit.series, hit.row), (series, 0));

    let tooltip = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert_eq!(tooltip.x_label, "North");
    assert_eq!(
        (
            tooltip.low,
            tooltip.q1,
            tooltip.value,
            tooltip.q3,
            tooltip.high,
            tooltip.x_low,
            tooltip.x_high,
        ),
        (
            Some(5.0),
            Some(10.0),
            Some(15.0),
            Some(20.0),
            Some(28.0),
            None,
            None,
        )
    );
    let missing = chart.general_tooltip_snapshot(series, 2).unwrap();
    assert_eq!(missing.value, None);
    assert_eq!(missing.q1, Some(-500.0));
    assert_eq!(missing.q3, Some(500.0));

    let mut invalid_style = GeneralSeriesOptions::box_plot(pane, dataset, "box-x", "box-y");
    invalid_style.box_fill_color = Some("invalid".into());
    assert!(
        chart
            .update_general_series_options(series, invalid_style)
            .is_err()
    );
    assert_eq!(
        chart.general_series(series).unwrap().box_fill_color(),
        Some("#112233")
    );

    let before = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert!(
        chart
            .replace_general_xy_dataset(
                dataset,
                GeneralXyInput::BoxCategory {
                    ids: Some(vec![GeneralRowId::Text("bad".into())]),
                    categories: vec!["Bad".into()],
                    category_indices: vec![0],
                    min: vec![5.0],
                    min_valid: None,
                    q1: vec![20.0],
                    q1_valid: None,
                    median: vec![15.0],
                    median_valid: None,
                    q3: vec![25.0],
                    q3_valid: None,
                    max: vec![30.0],
                    max_valid: None,
                },
            )
            .is_err()
    );
    assert_eq!(chart.general_tooltip_snapshot(series, 0), Some(before));

    chart
        .upsert_general_xy_dataset(
            dataset,
            GeneralXyInput::BoxCategory {
                ids: Some(vec![
                    GeneralRowId::Text("south".into()),
                    GeneralRowId::Text("east".into()),
                ]),
                categories: vec!["South".into(), "East".into()],
                category_indices: vec![0, 1],
                min: vec![14.0, 7.0],
                min_valid: None,
                q1: vec![19.0, 11.0],
                q1_valid: None,
                median: vec![25.0, 16.0],
                median_valid: None,
                q3: vec![31.0, 22.0],
                q3_valid: None,
                max: vec![42.0, 29.0],
                max_valid: None,
            },
            Some(3),
        )
        .unwrap();
    let retained = chart
        .general_accessibility_snapshot(series, 0, usize::MAX)
        .unwrap();
    assert_eq!(retained.total_rows, 3);
    assert_eq!(
        retained
            .items
            .iter()
            .map(|item| item.row_id.clone())
            .collect::<Vec<_>>(),
        vec![
            GeneralRowIdentity::Explicit(GeneralRowId::Text("south".into())),
            GeneralRowIdentity::Explicit(GeneralRowId::Text("missing".into())),
            GeneralRowIdentity::Explicit(GeneralRowId::Text("east".into())),
        ]
    );
    let updated = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert_eq!(
        (
            updated.low,
            updated.q1,
            updated.value,
            updated.q3,
            updated.high
        ),
        (Some(14.0), Some(19.0), Some(25.0), Some(31.0), Some(42.0))
    );
}

#[test]
fn category_heatmap_grid_owns_two_category_axes_geometry_hits_and_retention() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "heat-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Band,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "heat-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Band,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::HeatmapCategoryCategory {
            ids: Some(vec![
                GeneralRowId::Text("jan-north".into()),
                GeneralRowId::Text("jan-south".into()),
                GeneralRowId::Text("feb-north".into()),
                GeneralRowId::Text("missing".into()),
            ]),
            x_categories: vec!["Jan".into(), "Feb".into()],
            x_category_indices: vec![0, 0, 1, 1],
            y_categories: vec!["North".into(), "South".into()],
            y_category_indices: vec![0, 1, 0, 1],
            value: vec![10.0, 30.0, 50.0, 999.0],
            value_valid: Some(vec![1, 1, 1, 0]),
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::heatmap_grid(pane, dataset, "heat-x", "heat-y");
    options.title = "Regional heat".into();
    options.color = Some("#336699".into());
    let series = chart.add_general_series(options).unwrap();

    assert_eq!(
        chart.general_axis_effective_domain("heat-x"),
        Some(GeneralAxisDomain::Category(vec![
            "Jan".into(),
            "Feb".into()
        ]))
    );
    assert_eq!(
        chart.general_axis_effective_domain("heat-y"),
        Some(GeneralAxisDomain::Category(vec![
            "North".into(),
            "South".into()
        ]))
    );

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut geometry = Vec::new();
    chart.visit_general_heatmap_cells(chart.general_series(series).unwrap(), |cell| {
        geometry.push(cell)
    });
    assert_eq!(geometry.len(), 3);
    assert_eq!(
        geometry.iter().map(|cell| cell.row).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert_eq!(geometry[0].intensity, 0.0);
    assert_eq!(geometry[2].intensity, 1.0);
    assert!(geometry[0].left < geometry[2].left);
    assert!(
        geometry[0].bottom <= geometry[1].top || geometry[1].bottom <= geometry[0].top,
        "distinct Y categories must occupy non-overlapping bands"
    );

    let mut fixed = GeneralSeriesOptions::heatmap_grid(pane, dataset, "heat-x", "heat-y");
    fixed.title = "Regional heat".into();
    fixed.color = Some("#336699".into());
    fixed.heatmap_value_domain = Some([0.0, 100.0]);
    fixed.heatmap_low_color = Some("#0000ff".into());
    fixed.heatmap_high_color = Some("#ff0000".into());
    chart
        .update_general_series_options(series, fixed.clone())
        .unwrap();
    let mut fixed_intensity = Vec::new();
    chart.visit_general_heatmap_cells(chart.general_series(series).unwrap(), |cell| {
        fixed_intensity.push(cell.intensity)
    });
    assert_eq!(fixed_intensity, vec![0.1, 0.3, 0.5]);
    fixed.heatmap_value_domain = Some([100.0, 0.0]);
    assert!(
        chart
            .update_general_series_options(series, fixed.clone())
            .is_err()
    );
    fixed.heatmap_value_domain = Some([0.0, 100.0]);
    fixed.heatmap_low_color = Some("invalid".into());
    assert!(chart.update_general_series_options(series, fixed).is_err());
    assert_eq!(
        chart.general_series(series).unwrap().heatmap_value_domain(),
        Some([0.0, 100.0])
    );

    let frame = chart.build_frame();
    let colors = frame.panes[pane]
        .main
        .iter()
        .filter_map(|primitive| match primitive {
            Prim::Rect { color, .. } => Some(*color),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        colors,
        vec![
            Color::rgb(26, 0, 230),
            Color::rgb(77, 0, 179),
            Color::rgb(128, 0, 128),
        ]
    );
    assert_eq!(
        frame.panes[pane]
            .main
            .iter()
            .filter(|primitive| matches!(primitive, Prim::Rect { .. }))
            .count(),
        3
    );
    geometry.clear();
    chart.visit_general_heatmap_cells(chart.general_series(series).unwrap(), |cell| {
        geometry.push(cell)
    });
    let first = geometry[0];
    let hit = chart
        .general_hit_test(
            pane,
            (first.left + first.right) * 0.5,
            (first.top + first.bottom) * 0.5,
            crate::GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!((hit.series, hit.row), (series, 0));
    let center_x = (first.left + first.right) * 0.5;
    let center_y = (first.top + first.bottom) * 0.5;
    let cell_hit = |chart: &ChartEngine, x, y, mode| {
        chart
            .general_hit_test(pane, x, y, mode)
            .map(|hit| (hit.series, hit.row_id))
    };
    assert_eq!(
        cell_hit(
            &chart,
            first.left + 0.25,
            center_y,
            crate::GeneralHitMode::Exact
        )
        .map(|hit| hit.1),
        Some(GeneralRowIdentity::Explicit(GeneralRowId::Text(
            "jan-north".into()
        ))),
        "the visible cell edge must be an exact hit"
    );
    assert_eq!(
        cell_hit(
            &chart,
            first.right + 0.25,
            center_y,
            crate::GeneralHitMode::Exact
        ),
        None,
        "band padding after the cell must not be claimed as painted"
    );
    let missing_center = (
        (geometry[2].left + geometry[2].right) * 0.5,
        (geometry[1].top + geometry[1].bottom) * 0.5,
    );
    assert_eq!(
        cell_hit(
            &chart,
            missing_center.0,
            missing_center.1,
            crate::GeneralHitMode::Exact
        ),
        None,
        "the queryable missing Feb/South row paints no cell"
    );
    let north = geometry[0];
    let south = geometry[1];
    let (upper, lower) = if north.top < south.top {
        (north, south)
    } else {
        (south, north)
    };
    assert!(
        lower.top > upper.bottom,
        "adjacent category bands must have a gap"
    );
    let expected_id = |row: usize| {
        GeneralRowIdentity::Explicit(GeneralRowId::Text(
            if row == 0 { "jan-north" } else { "jan-south" }.into(),
        ))
    };
    let near_upper = upper.bottom + (lower.top - upper.bottom) * 0.2;
    let near_lower = upper.bottom + (lower.top - upper.bottom) * 0.8;
    for (y, row) in [(near_upper, upper.row), (near_lower, lower.row)] {
        assert_eq!(
            cell_hit(&chart, center_x, y, crate::GeneralHitMode::Exact),
            None,
            "unpainted space between rows has no exact cell: upper={upper:?}, lower={lower:?}, y={y}"
        );
        assert_eq!(
            cell_hit(
                &chart,
                center_x,
                y,
                crate::GeneralHitMode::Nearest { max_distance: 20.0 }
            )
            .map(|hit| hit.1),
            Some(expected_id(row)),
            "nearest must choose the adjacent row's stable ID"
        );
    }
    let other = chart
        .add_general_series(GeneralSeriesOptions::heatmap_grid(
            pane, dataset, "heat-x", "heat-y",
        ))
        .unwrap();
    for (order, top_series) in [(vec![series, other], other), (vec![other, series], series)] {
        assert!(chart.set_general_series_order(Some(pane), order));
        for (y, row) in [(near_upper, upper.row), (near_lower, lower.row)] {
            assert_eq!(
                cell_hit(
                    &chart,
                    center_x,
                    y,
                    crate::GeneralHitMode::Nearest { max_distance: 20.0 }
                ),
                Some((top_series, expected_id(row))),
                "nearest ties use paint order but retain row identity"
            );
        }
    }
    // A changed effective view must invalidate cell geometry and preserve surviving IDs.
    chart
        .zoom_general_category_axis("heat-x", 2.0, "Jan")
        .unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut viewed = Vec::new();
    chart.visit_general_heatmap_cells(chart.general_series(series).unwrap(), |cell| {
        viewed.push(cell)
    });
    assert_eq!(
        viewed.iter().map(|cell| cell.row).collect::<Vec<_>>(),
        vec![0, 1]
    );
    let viewed_first = viewed.iter().find(|cell| cell.row == 0).unwrap();
    assert_ne!(
        cell_hit(
            &chart,
            (geometry[2].left + geometry[2].right) * 0.5,
            (viewed_first.top + viewed_first.bottom) * 0.5,
            crate::GeneralHitMode::Exact
        )
        .map(|hit| hit.1),
        Some(GeneralRowIdentity::Explicit(GeneralRowId::Text(
            "feb-north".into()
        ))),
        "a cell removed from the view must not keep its old exact target"
    );
    assert_eq!(
        cell_hit(
            &chart,
            (viewed_first.left + viewed_first.right) * 0.5,
            (viewed_first.top + viewed_first.bottom) * 0.5,
            crate::GeneralHitMode::Nearest { max_distance: 20.0 }
        ),
        Some((series, expected_id(0))),
    );
    let viewed_south = viewed.iter().find(|cell| cell.row == 1).unwrap();
    let (upper, lower) = if viewed_first.top < viewed_south.top {
        (viewed_first, viewed_south)
    } else {
        (viewed_south, viewed_first)
    };
    for (fraction, row) in [(0.2, upper.row), (0.8, lower.row)] {
        assert_eq!(
            cell_hit(
                &chart,
                (upper.left + upper.right) * 0.5,
                upper.bottom + (lower.top - upper.bottom) * fraction,
                crate::GeneralHitMode::Nearest { max_distance: 20.0 }
            ),
            Some((series, expected_id(row))),
            "adjacent nearest identities must survive the changed view"
        );
    }
    let plot = chart.general_plot_rect(pane).unwrap();
    assert_eq!(
        cell_hit(
            &chart,
            plot.width + 0.25,
            center_y,
            crate::GeneralHitMode::Exact
        ),
        None,
        "a mark clipped by the plot cannot be an exact hit"
    );
    let tooltip = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert_eq!(tooltip.x_label, "Jan");
    assert_eq!(tooltip.y_label.as_deref(), Some("North"));
    assert_eq!(tooltip.value, Some(10.0));
    let missing = chart.general_tooltip_snapshot(series, 3).unwrap();
    assert_eq!(missing.y_label.as_deref(), Some("South"));
    assert_eq!(missing.value, None);

    let before = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert!(
        chart
            .replace_general_xy_dataset(
                dataset,
                GeneralXyInput::HeatmapCategoryCategory {
                    ids: None,
                    x_categories: vec!["Jan".into()],
                    x_category_indices: vec![0],
                    y_categories: vec!["North".into()],
                    y_category_indices: vec![1],
                    value: vec![1.0],
                    value_valid: None,
                },
            )
            .is_err()
    );
    assert_eq!(chart.general_tooltip_snapshot(series, 0), Some(before));

    chart
        .upsert_general_xy_dataset(
            dataset,
            GeneralXyInput::HeatmapCategoryCategory {
                ids: Some(vec![
                    GeneralRowId::Text("jan-south".into()),
                    GeneralRowId::Text("mar-west".into()),
                ]),
                x_categories: vec!["Jan".into(), "Mar".into()],
                x_category_indices: vec![0, 1],
                y_categories: vec!["South".into(), "West".into()],
                y_category_indices: vec![0, 1],
                value: vec![35.0, 70.0],
                value_valid: None,
            },
            Some(4),
        )
        .unwrap();
    let snapshot = chart
        .general_accessibility_snapshot(series, 0, usize::MAX)
        .unwrap();
    assert_eq!(snapshot.total_rows, 4);
    assert_eq!(
        snapshot
            .items
            .iter()
            .map(|item| (item.x_label.as_str(), item.y_label.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            ("Jan", Some("South")),
            ("Feb", Some("North")),
            ("Feb", Some("South")),
            ("Mar", Some("West")),
        ]
    );
    assert_eq!(snapshot.items[0].value, Some(35.0));
    assert_eq!(snapshot.items[3].value, Some(70.0));
}

#[test]
fn numeric_and_temporal_heatmaps_autoscale_geometry_hits_and_snapshots() {
    let mut chart = ChartEngine::new(720.0, 520.0, 1.0);
    let numeric_pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "numeric-heat-x",
            numeric_pane,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "numeric-heat-y",
            numeric_pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let numeric_data = chart
        .create_general_xy_dataset(GeneralXyInput::HeatmapNumericNumeric {
            ids: Some(vec![
                GeneralRowId::Text("n00".into()),
                GeneralRowId::Text("n01".into()),
                GeneralRowId::Text("n10".into()),
                GeneralRowId::Text("n11".into()),
            ]),
            x: vec![0.0, 0.0, 10.0, 10.0],
            y_coordinate: vec![100.0, 200.0, 100.0, 200.0],
            value: vec![1.0, 2.0, 3.0, 999.0],
            value_valid: Some(vec![1, 1, 1, 0]),
        })
        .unwrap();
    let numeric_series = chart
        .add_general_series(GeneralSeriesOptions::heatmap_grid(
            numeric_pane,
            numeric_data,
            "numeric-heat-x",
            "numeric-heat-y",
        ))
        .unwrap();

    let temporal_pane = chart
        .add_pane_with_domain(true, HorizontalDomain::Temporal)
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "temporal-heat-x",
            temporal_pane,
            AxisDimension::X,
            GeneralScaleType::Temporal,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "temporal-heat-y",
            temporal_pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let temporal_data = chart
        .create_general_xy_dataset(GeneralXyInput::HeatmapTemporalNumeric {
            ids: Some(vec![
                GeneralRowId::Text("t00".into()),
                GeneralRowId::Text("t01".into()),
                GeneralRowId::Text("t10".into()),
                GeneralRowId::Text("t11".into()),
            ]),
            x_epoch_ms: vec![
                1_700_000_000_000,
                1_700_000_000_000,
                1_700_000_060_000,
                1_700_000_060_000,
            ],
            y_coordinate: vec![5.0, 15.0, 5.0, 15.0],
            value: vec![10.0, 20.0, 30.0, 40.0],
            value_valid: None,
        })
        .unwrap();
    let temporal_series = chart
        .add_general_series(GeneralSeriesOptions::heatmap_grid(
            temporal_pane,
            temporal_data,
            "temporal-heat-x",
            "temporal-heat-y",
        ))
        .unwrap();

    let GeneralAxisDomain::Numeric(numeric_y_domain) = chart
        .general_axis_effective_domain("numeric-heat-y")
        .unwrap()
    else {
        panic!("numeric heatmap Y axis must autoscale numerically");
    };
    assert!(numeric_y_domain[0] <= 100.0 && numeric_y_domain[1] >= 200.0);
    let GeneralAxisDomain::Temporal(temporal_x_domain) = chart
        .general_axis_effective_domain("temporal-heat-x")
        .unwrap()
    else {
        panic!("temporal heatmap X axis must autoscale temporally");
    };
    assert!(temporal_x_domain[0] <= 1_700_000_000_000 && temporal_x_domain[1] >= 1_700_000_060_000);

    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut numeric_geometry = Vec::new();
    chart.visit_general_heatmap_cells(chart.general_series(numeric_series).unwrap(), |cell| {
        numeric_geometry.push(cell)
    });
    assert_eq!(
        numeric_geometry.len(),
        3,
        "missing value emits no numeric heatmap cell"
    );
    assert_eq!(numeric_geometry[0].intensity, 0.0);
    assert_eq!(numeric_geometry[2].intensity, 1.0);
    let mut fixed_numeric = GeneralSeriesOptions::heatmap_grid(
        numeric_pane,
        numeric_data,
        "numeric-heat-x",
        "numeric-heat-y",
    );
    fixed_numeric.heatmap_value_domain = Some([0.0, 4.0]);
    chart
        .update_general_series_options(numeric_series, fixed_numeric)
        .unwrap();
    let mut fixed_numeric_intensity = Vec::new();
    chart.visit_general_heatmap_cells(chart.general_series(numeric_series).unwrap(), |cell| {
        fixed_numeric_intensity.push(cell.intensity)
    });
    assert_eq!(fixed_numeric_intensity, vec![0.25, 0.5, 0.75]);
    let first = numeric_geometry[0];
    let hit = chart
        .general_hit_test(
            numeric_pane,
            (first.left + first.right) * 0.5,
            (first.top + first.bottom) * 0.5,
            crate::GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!((hit.series, hit.row), (numeric_series, 0));
    let numeric_tooltip = chart.general_tooltip_snapshot(numeric_series, 1).unwrap();
    assert_eq!(numeric_tooltip.x_label, "0");
    assert_eq!(numeric_tooltip.y_label.as_deref(), Some("200"));
    assert_eq!(numeric_tooltip.value, Some(2.0));

    let mut temporal_geometry = Vec::new();
    chart.visit_general_heatmap_cells(chart.general_series(temporal_series).unwrap(), |cell| {
        temporal_geometry.push(cell)
    });
    assert_eq!(temporal_geometry.len(), 4);
    let mut fixed_temporal = GeneralSeriesOptions::heatmap_grid(
        temporal_pane,
        temporal_data,
        "temporal-heat-x",
        "temporal-heat-y",
    );
    fixed_temporal.heatmap_value_domain = Some([0.0, 100.0]);
    chart
        .update_general_series_options(temporal_series, fixed_temporal)
        .unwrap();
    let mut fixed_temporal_intensity = Vec::new();
    chart.visit_general_heatmap_cells(chart.general_series(temporal_series).unwrap(), |cell| {
        fixed_temporal_intensity.push(cell.intensity)
    });
    assert_eq!(fixed_temporal_intensity, vec![0.1, 0.2, 0.3, 0.4]);
    let temporal_tooltip = chart.general_tooltip_snapshot(temporal_series, 3).unwrap();
    assert_eq!(temporal_tooltip.x_label, "1700000060000");
    assert_eq!(temporal_tooltip.y_label.as_deref(), Some("15"));
    assert_eq!(temporal_tooltip.value, Some(40.0));
}

#[test]
fn temporal_error_bars_preserve_epoch_bounds_autoscale_geometry_and_atomic_updates() {
    let mut chart = ChartEngine::new(640.0, 400.0, 1.0);
    let pane = chart
        .add_pane_with_domain(true, HorizontalDomain::Temporal)
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "temporal-error-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Temporal,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "temporal-error-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::ErrorTemporal {
            ids: Some(vec![
                GeneralRowId::Text("first".into()),
                GeneralRowId::Text("second".into()),
            ]),
            x_epoch_ms: vec![1_700_000_000_000, 1_700_000_060_000],
            y: vec![20.0, 30.0],
            y_valid: None,
            x_low_epoch_ms: vec![1_699_999_970_000.0, 0.0],
            x_low_valid: Some(vec![1, 0]),
            x_high_epoch_ms: vec![1_700_000_030_000.0, 1_700_000_090_000.0],
            x_high_valid: None,
            y_low: vec![15.0, 25.0],
            y_low_valid: None,
            y_high: vec![26.0, 0.0],
            y_high_valid: Some(vec![1, 0]),
        })
        .unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::error_bar(
            pane,
            dataset,
            "temporal-error-x",
            "temporal-error-y",
        ))
        .unwrap();

    assert_eq!(
        chart.general_axis_effective_domain("temporal-error-x"),
        Some(GeneralAxisDomain::Temporal([
            1_699_999_965_200,
            1_700_000_094_800,
        ])),
        "the error-bar extent plus a 4% margin at each end"
    );
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let mut geometry = Vec::new();
    chart.visit_general_error_bars(chart.general_series(series).unwrap(), |bar| {
        geometry.push(bar)
    });
    assert_eq!(geometry.len(), 2);
    assert!(geometry[0].x_low.is_some());
    assert!(geometry[0].x_high.is_some());
    assert!(geometry[1].x_low.is_none());
    assert!(geometry[1].x_high.is_some());
    assert!(geometry[0].x < geometry[1].x);
    let tooltip = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert_eq!(tooltip.x_label, "1700000000000");
    assert_eq!(tooltip.x_low, Some(1_699_999_970_000.0));
    assert_eq!(tooltip.x_high, Some(1_700_000_030_000.0));
    assert_eq!((tooltip.low, tooltip.high), (Some(15.0), Some(26.0)));

    assert!(
        chart
            .replace_general_xy_dataset(
                dataset,
                GeneralXyInput::ErrorTemporal {
                    ids: None,
                    x_epoch_ms: vec![1_700_000_000_000],
                    y: vec![20.0],
                    y_valid: None,
                    x_low_epoch_ms: vec![1_700_000_000_001.0],
                    x_low_valid: None,
                    x_high_epoch_ms: vec![1_700_000_030_000.0],
                    x_high_valid: None,
                    y_low: vec![15.0],
                    y_low_valid: None,
                    y_high: vec![26.0],
                    y_high_valid: None,
                },
            )
            .is_err()
    );
    assert_eq!(
        chart.general_tooltip_snapshot(series, 0),
        Some(tooltip.clone())
    );

    assert!(
        chart
            .replace_general_xy_dataset(
                dataset,
                GeneralXyInput::ErrorTemporal {
                    ids: None,
                    x_epoch_ms: vec![1_700_000_000_000],
                    y: vec![20.0],
                    y_valid: None,
                    x_low_epoch_ms: vec![1_699_999_999_999.5],
                    x_low_valid: None,
                    x_high_epoch_ms: vec![1_700_000_030_000.0],
                    x_high_valid: None,
                    y_low: vec![15.0],
                    y_low_valid: None,
                    y_high: vec![26.0],
                    y_high_valid: None,
                },
            )
            .is_err()
    );
    assert_eq!(chart.general_tooltip_snapshot(series, 0), Some(tooltip));

    chart
        .upsert_general_xy_dataset(
            dataset,
            GeneralXyInput::ErrorTemporal {
                ids: Some(vec![GeneralRowId::Text("first".into())]),
                x_epoch_ms: vec![1_700_000_010_000],
                y: vec![22.0],
                y_valid: None,
                x_low_epoch_ms: vec![1_699_999_980_000.0],
                x_low_valid: None,
                x_high_epoch_ms: vec![1_700_000_040_000.0],
                x_high_valid: None,
                y_low: vec![16.0],
                y_low_valid: None,
                y_high: vec![28.0],
                y_high_valid: None,
            },
            Some(2),
        )
        .unwrap();
    let updated = chart.general_tooltip_snapshot(series, 0).unwrap();
    assert_eq!(updated.x_label, "1700000010000");
    assert_eq!(updated.x_low, Some(1_699_999_980_000.0));
    assert_eq!(updated.x_high, Some(1_700_000_040_000.0));
    assert_eq!(
        (updated.value, updated.low, updated.high),
        (Some(22.0), Some(16.0), Some(28.0))
    );
}

#[test]
fn scatter_log_and_symlog_axes_validate_and_emit_transformed_ticks() {
    let mut chart = ChartEngine::new(500.0, 320.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Logarithmic,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "x-log",
            pane,
            AxisDimension::X,
            GeneralScaleType::Logarithmic,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "y-symlog",
            pane,
            AxisDimension::Y,
            GeneralScaleType::SymmetricLog,
        ))
        .unwrap();
    let invalid_dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: vec![0.0, 10.0],
            y: vec![-1.0, 1.0],
            y_valid: None,
        })
        .unwrap();
    assert_eq!(
        chart
            .add_general_series(GeneralSeriesOptions::scatter(
                pane,
                invalid_dataset,
                "x-log",
                "y-symlog",
            ))
            .unwrap_err()
            .code(),
        crate::ErrorCode::InvalidOptions
    );

    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: vec![1.0, 10.0, 100.0],
            y: vec![-10.0, 0.0, 10.0],
            y_valid: None,
        })
        .unwrap();
    let series = chart
        .add_general_series(GeneralSeriesOptions::scatter(
            pane, dataset, "x-log", "y-symlog",
        ))
        .unwrap();
    // The automatic margin is applied in transformed space, so log and symlog ends stay valid.
    let Some(GeneralAxisDomain::Numeric([log_low, log_high])) =
        chart.general_axis_effective_domain("x-log")
    else {
        panic!("log axis has a numeric domain");
    };
    assert!((log_low - 10_f64.powf(-0.08)).abs() < 1e-9);
    assert!((log_high - 10_f64.powf(2.08)).abs() < 1e-9);
    let Some(GeneralAxisDomain::Numeric([symlog_low, symlog_high])) =
        chart.general_axis_effective_domain("y-symlog")
    else {
        panic!("symlog axis has a numeric domain");
    };
    let symlog_end = 11_f64.powf(1.08) - 1.0;
    assert!((symlog_low + symlog_end).abs() < 1e-9);
    assert!((symlog_high - symlog_end).abs() < 1e-9);
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let axis = chart.build_axis_frame(80.0, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    let texts: Vec<_> = axis
        .labels
        .iter()
        .map(|label| label.text.as_str())
        .collect();
    for expected in ["2", "10", "50", "0", "-10"] {
        assert!(texts.contains(&expected), "{expected}: {texts:?}");
    }
    assert!(
        texts.iter().all(|text| !text.contains('.')),
        "symlog ticks are round decades: {texts:?}"
    );

    let before = chart.general_dataset(dataset).unwrap().clone();
    let err = chart
        .replace_general_xy_dataset(
            dataset,
            GeneralXyInput::Numeric {
                ids: None,
                x: vec![-1.0, 10.0],
                y: vec![1.0, 2.0],
                y_valid: None,
            },
        )
        .unwrap_err();
    assert_eq!(err.code(), crate::ErrorCode::InvalidOptions);
    assert_eq!(chart.general_dataset(dataset), Some(&before));
    assert!(chart.general_series(series).is_some());
}

#[test]
fn dense_scatter_hit_testing_uses_bounded_screen_space_candidates() {
    const POINTS: usize = 100_000;
    let mut chart = ChartEngine::new(1_000.0, 600.0, 1.0);
    let pane = chart
        .add_pane_with_domain(
            true,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "dense-x",
            pane,
            AxisDimension::X,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    chart
        .add_general_axis(GeneralAxisOptions::new(
            "dense-y",
            pane,
            AxisDimension::Y,
            GeneralScaleType::Linear,
        ))
        .unwrap();
    let x: Vec<_> = (0..POINTS).map(|index| (index % 1_000) as f64).collect();
    let y: Vec<_> = (0..POINTS).map(|index| (index / 1_000) as f64).collect();
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x,
            y,
            y_valid: None,
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::scatter(pane, dataset, "dense-x", "dense-y");
    options.data_labels = true;
    let series = chart.add_general_series(options).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);

    let plot = chart.general_plot_rect(pane).unwrap();
    let x_css = plot.width * 0.5;
    let y_css = plot.y + plot.height * 0.5;
    let candidates = chart
        .general_scatter_hit_candidate_count(series, x_css, y_css, 3.0)
        .unwrap();
    assert!(candidates > 0);
    assert!(
        candidates < POINTS / 20,
        "dense hit query inspected {candidates} of {POINTS} points"
    );
    assert!(
        chart
            .general_hit_test(
                pane,
                x_css,
                y_css,
                crate::GeneralHitMode::Nearest { max_distance: 8.0 },
            )
            .is_some()
    );
    assert!(chart.memory_usage().general_series_capacity_bytes > 0);
    let label_count = chart.build_frame().panes[pane]
        .main
        .iter()
        .filter(|primitive| matches!(primitive, Prim::Text { .. }))
        .count();
    assert!(label_count > 0);
    assert!(
        label_count <= 512,
        "dense scatter emitted {label_count} labels"
    );
}

#[test]
fn marker_geometry_tracks_reference_spacing_buckets() {
    assert_eq!(marker_envelope_size(0.5), 10.0);
    assert_eq!(marker_envelope_size(6.0), 10.0);
    assert_eq!(marker_envelope_size(20.0), 18.0);
    assert_eq!(marker_envelope_size(50.0), 28.0);
    assert_eq!(marker_shape_size(10.0, 0.8), 9.0);
    assert_eq!(marker_shape_size(10.0, 0.7), 9.0);
    assert_eq!(marker_margin(6.0), 3.0);
}

#[test]
fn marker_autoscale_margins_match_reference_position_rules() {
    let marker = |position| crate::Marker {
        time: 0,
        position,
        shape: crate::marker_shape::CIRCLE,
        color: Color::rgb(0, 0, 0),
        text: String::new(),
        id: String::new(),
        size: 1.0,
        price: None,
    };
    assert_eq!(
        marker_auto_scale_margins(&[marker(crate::marker_pos::ABOVE)], 6.0),
        (21.0, 0.0)
    );
    assert_eq!(
        marker_auto_scale_margins(&[marker(crate::marker_pos::IN_BAR)], 6.0),
        (11.0, 11.0)
    );
    assert_eq!(
        marker_auto_scale_margins(
            &[
                marker(crate::marker_pos::ABOVE),
                marker(crate::marker_pos::IN_BAR),
            ],
            6.0,
        ),
        (21.0, 11.0)
    );
}

fn reference_marker(time: i64, position: u8, color: Color, text: &str) -> crate::Marker {
    crate::Marker {
        time,
        position,
        shape: crate::marker_shape::CIRCLE,
        color,
        text: text.to_string(),
        id: String::new(),
        size: 1.0,
        price: None,
    }
}

#[test]
fn marker_price_position_time_snapping_and_layers_are_engine_owned() {
    let mut chart = ChartEngine::new(300.0, 200.0, 1.0);
    chart
        .set_series_data(
            0,
            &[10.0, 20.0, 30.0],
            &[100.0, 101.0, 102.0],
            &[102.0, 103.0, 104.0],
            &[98.0, 99.0, 100.0],
            &[101.0, 102.0, 103.0],
        )
        .unwrap();
    chart.time_scale.set_width(300.0);
    chart.fit_content();
    let color = Color::rgb(222, 17, 99);
    let mut marker = reference_marker(15, crate::marker_pos::AT_PRICE_MIDDLE, color, "exact");
    marker.price = Some(101.5);
    marker.size = 2.0;
    chart.set_series_markers(0, vec![marker]);

    let normal = chart.build_frame();
    let expected_x = chart.time_scale.index_to_coordinate(1).round() as f32 + 0.5;
    let expected_y = chart.panes[0].price_scale.price_to_coordinate(101.5, 100.0) as f32;
    let circle_index = normal.panes[0]
        .main
        .iter()
        .position(|prim| {
            matches!(prim, Prim::Circle { cx, cy, fill, .. }
            if *fill == color && (*cx - expected_x).abs() < 1e-4 && (*cy - expected_y).abs() < 1e-4)
        })
        .expect("marker time 15 must snap to the series bar at time 20");
    let text_index = normal.panes[0]
        .main
        .iter()
        .position(|prim| {
            matches!(prim, Prim::Text { text, color: text_color, .. }
            if text == "exact" && *text_color == color)
        })
        .expect("marker label must share the retained frame instead of the axis overlay");
    let owner = chart
        .frame_series_segments(0)
        .iter()
        .find(|segment| segment.series_id == Some(0))
        .copied()
        .unwrap();
    assert!((owner.start..owner.end).contains(&circle_index));
    assert!((owner.start..owner.end).contains(&text_index));
    assert!(normal.panes[0].top_prims.is_empty());

    assert!(chart.set_series_markers_z_order(0, crate::marker_z_order::ABOVE_SERIES));
    let above = chart.build_frame();
    let above_index = above.panes[0]
        .main
        .iter()
        .position(|prim| matches!(prim, Prim::Circle { fill, .. } if *fill == color))
        .unwrap();
    let owner_end = chart
        .frame_series_segments(0)
        .iter()
        .find(|segment| segment.series_id == Some(0))
        .unwrap()
        .end;
    assert!(
        above_index >= owner_end,
        "aboveSeries must paint after every series slot"
    );
    assert!(above.panes[0].top_prims.is_empty());

    assert!(chart.set_series_markers_z_order(0, crate::marker_z_order::TOP));
    let top = chart.build_frame();
    assert!(
        !top.panes[0]
            .main
            .iter()
            .any(|prim| matches!(prim, Prim::Circle { fill, .. } if *fill == color))
    );
    assert!(
        top.panes[0]
            .top_prims
            .iter()
            .any(|prim| matches!(prim, Prim::Circle { fill, .. } if *fill == color))
    );
    assert!(
        top.panes[0]
            .top_prims
            .iter()
            .any(|prim| matches!(prim, Prim::Text { text, .. } if text == "exact"))
    );
}

#[test]
fn same_bar_markers_stack_with_reference_offsets() {
    let mut chart = ChartEngine::new(300.0, 200.0, 1.0);
    chart
        .set_series_data(
            0,
            &[10.0, 20.0],
            &[100.0, 100.0],
            &[102.0, 102.0],
            &[98.0, 98.0],
            &[101.0, 101.0],
        )
        .unwrap();
    chart.time_scale.set_width(300.0);
    chart.fit_content();
    let first = Color::rgb(201, 10, 10);
    let second = Color::rgb(10, 10, 201);
    chart.set_series_markers(
        0,
        vec![
            reference_marker(20, crate::marker_pos::ABOVE, first, ""),
            reference_marker(20, crate::marker_pos::ABOVE, second, ""),
        ],
    );
    let frame = chart.build_frame();
    let center = |color| {
        frame.panes[0]
            .main
            .iter()
            .find_map(|prim| match prim {
                Prim::Circle { cy, fill, .. } if *fill == color => Some(*cy),
                _ => None,
            })
            .unwrap()
    };
    let spacing = chart.time_scale.bar_spacing();
    let expected = marker_envelope_size(spacing) + marker_margin(spacing);
    assert!(((center(first) - center(second)) as f64 - expected).abs() < 1e-4);
}

struct TestPlot {
    indices: PlotList,
    values: [Vec<f64>; 4],
}

impl TestPlot {
    fn new(indices: Vec<i64>, values: [Vec<f64>; 4]) -> Self {
        let mut plot = PlotList::new();
        plot.set_indices(indices);
        Self {
            indices: plot,
            values,
        }
    }

    fn view(&self) -> PlotListView<'_> {
        PlotListView::new(
            &self.indices,
            PlotValues::Ohlc([
                &self.values[0],
                &self.values[1],
                &self.values[2],
                &self.values[3],
            ]),
        )
    }
}

fn test_plot(count: usize) -> TestPlot {
    let indices: Vec<i64> = (0..count as i64).collect();
    let close: Vec<f64> = indices
        .iter()
        .map(|i| {
            if i % 10 == 4 {
                100.0 + (*i as f64) * 0.2 + 8.0
            } else if i % 10 == 7 {
                100.0 + (*i as f64) * 0.2 - 8.0
            } else {
                100.0 + (*i as f64) * 0.2
            }
        })
        .collect();
    TestPlot::new(
        indices,
        [close.clone(), close.clone(), close.clone(), close],
    )
}

#[test]
fn conflation_preserves_endpoints_and_pixel_bucket_extrema() {
    let plot = test_plot(100);
    let rows = visible_line_rows(plot.view(), 0, 99, 0.1, 1.0, |index| index as f64 * 0.1);
    assert!(
        rows.len() < 60,
        "sub-pixel data should be reduced: {} rows",
        rows.len()
    );
    assert_eq!(rows.first().copied(), Some(0));
    assert_eq!(rows.last().copied(), Some(99));
    // Bucket 0..3.999 keeps the high at row 4 only after the bucket boundary; bucket 4..7.999
    // must retain its low at row 7 rather than smoothing away the visible envelope.
    assert!(rows.contains(&4));
    assert!(rows.contains(&7));
    assert!(rows.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn normal_spacing_keeps_every_visible_row_plus_one_neighbour_per_edge() {
    let plot = test_plot(32);
    let rows = visible_line_rows(plot.view(), 4, 20, 2.0, 1.0, |index| index as f64 * 2.0);
    // Rows 3 and 21 are off-screen, but the path must keep running to both pane edges.
    assert_eq!(rows, (3..=21).map(|i| i as usize).collect::<Vec<_>>());
    // At the data boundaries there is no neighbour to add.
    let rows = visible_line_rows(plot.view(), 0, 31, 2.0, 1.0, |index| index as f64 * 2.0);
    assert_eq!(rows, (0..=31).map(|i| i as usize).collect::<Vec<_>>());
}

#[test]
fn ohlc_conflation_keeps_first_open_last_close_and_full_envelope() {
    let indices: Vec<i64> = (0..8).collect();
    let open = vec![10.0, 12.0, 11.0, 14.0, 20.0, 19.0, 18.0, 17.0];
    let high = vec![13.0, 15.0, 19.0, 16.0, 22.0, 25.0, 21.0, 20.0];
    let low = vec![9.0, 8.0, 10.0, 11.0, 18.0, 16.0, 15.0, 14.0];
    let close = vec![12.0, 11.0, 14.0, 13.0, 19.0, 18.0, 17.0, 16.0];
    let plot = TestPlot::new(indices, [open, high, low, close]);

    let bars = visible_ohlc(plot.view(), 0, 7, 0.25, 1.0, |index| index as f64 * 0.25);
    assert_eq!(
        bars,
        vec![
            VisibleOhlc {
                x_px: 0.0,
                open: 10.0,
                high: 19.0,
                low: 8.0,
                close: 13.0,
                source_row: 3,
                geometry_time: 0
            },
            VisibleOhlc {
                x_px: 1.0,
                open: 20.0,
                high: 25.0,
                low: 14.0,
                close: 16.0,
                source_row: 7,
                geometry_time: 1
            },
        ]
    );
}

#[test]
fn ohlc_normal_spacing_is_an_identity_transform() {
    let plot = test_plot(8);
    let view = plot.view();
    let bars = visible_ohlc(view, 2, 5, 2.0, 1.5, |index| index as f64 * 3.0);
    assert_eq!(bars.len(), 4);
    assert_eq!(bars[0].x_px, 6.0);
    assert_eq!(bars[0].open, view.value_at(2, PlotValueIndex::Open));
    assert_eq!(bars[3].close, view.value_at(5, PlotValueIndex::Close));
}

#[test]
fn histogram_conflation_preserves_largest_magnitude_and_source_row() {
    let indices: Vec<i64> = (0..8).collect();
    let values = vec![1.0, -8.0, 3.0, 4.0, 2.0, 5.0, -12.0, 7.0];
    let plot = TestPlot::new(
        indices,
        [values.clone(), values.clone(), values.clone(), values],
    );

    let rows = visible_histogram_rows(plot.view(), 0, 7, 0.25, 1.0, |index| index as f64 * 0.25);
    assert_eq!(
        rows,
        vec![
            VisibleHistogramRow {
                x_px: 0.0,
                source_row: 1,
                geometry_time: 0
            },
            VisibleHistogramRow {
                x_px: 1.0,
                source_row: 6,
                geometry_time: 1
            },
        ]
    );
}

#[test]
fn hierarchical_density_matches_forced_raw_reference_across_random_viewports() {
    let count = 20_000usize;
    let mut state = 0x7a31_9d2bu32;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    let times = (0..count as i64).collect::<Vec<_>>();
    let mut open = Vec::with_capacity(count);
    let mut high = Vec::with_capacity(count);
    let mut low = Vec::with_capacity(count);
    let mut close = Vec::with_capacity(count);
    for row in 0..count {
        if row % 211 == 0 {
            open.push(f64::NAN);
            high.push(f64::NAN);
            low.push(f64::NAN);
            close.push(f64::NAN);
            continue;
        }
        let base = 100.0 + f64::from(next() % 10_000) / 100.0;
        let end = base + f64::from(next() % 2_000) / 100.0 - 10.0;
        open.push(base);
        high.push(base.max(end) + f64::from(next() % 500) / 100.0);
        low.push(base.min(end) - f64::from(next() % 500) / 100.0);
        close.push(end);
    }
    high[7_777] = 10_000.0;
    low[13_333] = -10_000.0;
    let mut data = DataLayer::new();
    let id = data.add_series();
    assert!(data.set_data(id, times, open, high, low, close));

    for iteration in 0..200 {
        let left = (next() as usize % (count - 2_000)) as i64;
        let right = (left as usize + 1_000 + next() as usize % 1_000) as i64;
        let bar_spacing = [0.0007, 0.0023, 0.007, 0.02][iteration % 4];
        let hpr = [1.0, 1.25, 2.0][iteration % 3];
        let offset = f64::from(next() % 1_000) / 1_000.0;
        let x_at = |index: i64| index as f64 * bar_spacing * hpr + offset;
        let plot = data.plot(id);

        assert_eq!(
            visible_ohlc(plot, left, right, bar_spacing, hpr, x_at),
            visible_ohlc_raw_reference(plot, left, right, bar_spacing, hpr, x_at),
            "OHLC mismatch at iteration {iteration}"
        );
        assert_eq!(
            visible_line_rows(plot, left, right, bar_spacing, hpr, x_at),
            visible_line_rows_raw_reference(plot, left, right, bar_spacing, hpr, x_at),
            "line mismatch at iteration {iteration}"
        );
        assert_eq!(
            visible_histogram_rows(plot, left, right, bar_spacing, hpr, x_at),
            visible_histogram_rows_raw_reference(plot, left, right, bar_spacing, hpr, x_at),
            "histogram mismatch at iteration {iteration}"
        );
    }

    let plot = data.plot(id);
    let mut work = DensityWork::default();
    let rows = visible_ohlc_with_work(
        plot,
        0,
        count as i64 - 1,
        0.001,
        1.0,
        |index| index as f64 * 0.001,
        &mut work,
    );
    assert!(work.selected_level > 0);
    assert!(work.raw_rows + work.summary_nodes < count / 5);
    assert!(rows.iter().any(|bar| bar.high == 10_000.0));
    assert!(rows.iter().any(|bar| bar.low == -10_000.0));
}

#[test]
fn hierarchical_density_preserves_sparse_series_and_long_whitespace_runs() {
    let source_rows = 18_001usize;
    let sparse_rows = 6_001usize;
    let mut data = DataLayer::new();
    let dense = data.add_series();
    let sparse = data.add_series();
    let dense_times = (0..source_rows as i64).collect::<Vec<_>>();
    let dense_values = vec![1.0; source_rows];
    assert!(data.set_data(
        dense,
        dense_times,
        dense_values.clone(),
        dense_values.clone(),
        dense_values.clone(),
        dense_values,
    ));

    let sparse_times = (0..sparse_rows as i64)
        .map(|row| row * 3)
        .collect::<Vec<_>>();
    let mut sparse_values = (0..sparse_rows)
        .map(|row| 100.0 + (row as f64 * 0.03).sin())
        .collect::<Vec<_>>();
    sparse_values[17] = f64::NAN;
    sparse_values[1_000..2_000].fill(f64::NAN);
    assert!(data.set_data(
        sparse,
        sparse_times,
        sparse_values.clone(),
        sparse_values.clone(),
        sparse_values.clone(),
        sparse_values,
    ));
    assert!(!data.series_memory_usage(sparse).unwrap().dense_index_view);

    let plot = data.plot(sparse);
    let from = 41;
    let to = 17_963;
    let spacing = 0.01;
    let x_at = |index: i64| index as f64 * spacing + 0.37;
    assert_eq!(
        visible_ohlc(plot, from, to, spacing, 1.0, x_at),
        visible_ohlc_raw_reference(plot, from, to, spacing, 1.0, x_at)
    );
    assert_eq!(
        visible_line_rows(plot, from, to, spacing, 1.0, x_at),
        visible_line_rows_raw_reference(plot, from, to, spacing, 1.0, x_at)
    );
    assert_eq!(
        visible_histogram_rows(plot, from, to, spacing, 1.0, x_at),
        visible_histogram_rows_raw_reference(plot, from, to, spacing, 1.0, x_at)
    );
}

#[test]
fn million_bar_full_view_bounds_density_work_for_frame_and_hit_test() {
    let count = 1_000_000usize;
    let times = (0..count).map(|row| row as f64).collect::<Vec<_>>();
    let close = (0..count)
        .map(|row| 100.0 + (row % 101) as f64 * 0.01)
        .collect::<Vec<_>>();
    let high = close.iter().map(|value| value + 0.5).collect::<Vec<_>>();
    let low = close.iter().map(|value| value - 0.5).collect::<Vec<_>>();
    let mut chart = ChartEngine::new(1_280.0, 720.0, 1.0);
    chart
        .set_series_data(0, &times, &close, &high, &low, &close)
        .unwrap();
    chart.time_scale.set_width(1_280.0);
    chart.set_min_bar_spacing(0.000_001);
    chart.set_visible_logical_range(0.0, count as f64 - 1.0);

    let frame = chart.build_frame();
    let work = chart.lod_work_stats();
    assert_eq!(work.selected_level, 2);
    assert!(work.raw_rows < 25_000, "{work:?}");
    assert!(work.summary_nodes < 30_000, "{work:?}");
    assert!(work.candidates <= 1_280 * 6, "{work:?}");
    assert!(!frame.panes[0].main.is_empty());

    let _ = chart.hit_test_one_series(0, 640.0, 360.0);
    let hit_work = chart.lod_work_stats();
    assert_eq!(hit_work.selected_level, 2);
    assert!(hit_work.raw_rows < 25_000, "{hit_work:?}");
    assert!(hit_work.summary_nodes < 30_000, "{hit_work:?}");

    for appended in 0..32 {
        let row = count + appended;
        assert!(chart.update_series_bar(0, row as f64, [101.0, 101.5, 100.5, 101.25],));
        assert!(
            chart
                .data
                .last_lod_update_nodes(0)
                .is_some_and(|nodes| nodes <= 5)
        );
        chart.set_visible_logical_range(0.0, row as f64);
        chart.build_frame();
        let append_work = chart.lod_work_stats();
        assert_eq!(append_work.selected_level, 2);
        assert!(append_work.raw_rows < 25_000, "{append_work:?}");
        assert!(append_work.summary_nodes < 30_000, "{append_work:?}");
        assert!(append_work.candidates <= 1_280 * 6, "{append_work:?}");
    }
}

fn crosshair_chart() -> ChartEngine {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Line;
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0],
            &[10.0, 11.0, 12.0],
            &[11.0, 12.0, 13.0],
            &[9.0, 10.0, 11.0],
            &[10.5, 11.5, 12.5],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart
}

fn visual_crosshair_presence(chart: &mut ChartEngine) -> (bool, bool) {
    let line_color = Color::rgb(0x12, 0x34, 0x56);
    let label_color = Color::rgb(0x65, 0x43, 0x21);
    let frame = chart.build_frame();
    let lines = frame
        .panes
        .iter()
        .flat_map(|pane| &pane.main)
        .any(|primitive| {
            matches!(
                primitive,
                Prim::VLine { color, .. } | Prim::HLine { color, .. } if *color == line_color
            )
        });
    let suppressed_after_frame = chart.crosshair_suppressed_by_interaction();
    let axis = chart.build_axis_frame(
        80.0,
        |text, _| text.len() as f64 * 7.0,
        |text, _| text.len() as f64 * 6.0,
    );
    let labels = axis.labels.iter().any(|label| {
        !label.text.is_empty()
            && matches!(label.background, Some((.., color)) if color == label_color)
    });
    assert_eq!(
        suppressed_after_frame,
        chart.crosshair_suppressed_by_interaction()
    );
    (lines, labels)
}

fn configure_distinct_crosshair(chart: &mut ChartEngine) {
    chart
        .apply_options(
            r##"{
                "crosshair": {
                    "vertLine": {"color":"#123456","labelBackgroundColor":"#654321"},
                    "horzLine": {"color":"#123456","labelBackgroundColor":"#654321"}
                }
            }"##,
        )
        .unwrap();
    chart.set_crosshair_at(chart.time_scale.index_to_coordinate(1), 250.0);
}

#[test]
fn trading_objects_suppress_the_visual_crosshair_without_clearing_its_position() {
    let mut chart = crosshair_chart();
    configure_distinct_crosshair(&mut chart);
    assert_eq!(visual_crosshair_presence(&mut chart), (true, true));

    chart
        .set_trading_snapshot(crate::TradingSnapshot {
            orders: vec![crate::WorkingOrder {
                id: crate::OrderId::new("crosshair-order").unwrap(),
                account_id: None,
                pane_index: 0,
                price_scale: crate::TradingPriceScale::Right,
                side: crate::OrderSide::Sell,
                kind: crate::OrderKind::Limit,
                role: crate::OrderRole::Working,
                status: crate::OrderStatus::Working,
                price: 11.5,
                stop_price: None,
                trailing_trigger_price: None,
                break_even_trigger_price: None,
                quantity: 1.0,
                filled_quantity: 0.0,
                position_id: None,
                parent_order_id: None,
                bracket_id: None,
                oco_group_id: None,
                revision: 1,
                annotations: Vec::new(),
            }],
            ..crate::TradingSnapshot::default()
        })
        .unwrap();
    chart.build_frame();
    let y = chart
        .trading_price_coordinate(0, crate::TradingPriceScale::Right, 11.5)
        .unwrap();
    let stop_x = (0..=(chart.pane_w * 2.0) as usize)
        .map(|step| step as f64 / 2.0)
        .find(|x| {
            chart
                .trading_hit_at(*x, y)
                .is_some_and(|hit| hit.kind == crate::TradingHitKind::StopLossButton)
        })
        .expect("visible stop-loss button");
    assert!(chart.set_trading_hover(stop_x, y));
    assert!(chart.crosshair_suppressed_by_interaction());
    assert_eq!(visual_crosshair_presence(&mut chart), (false, false));
    assert!(
        chart.crosshair.is_some(),
        "callbacks retain the pointer position"
    );

    assert!(chart.trading_drag_start_at(stop_x, y));
    assert!(chart.clear_trading_hover());
    assert_eq!(visual_crosshair_presence(&mut chart), (false, false));
    assert!(chart.cancel_trading_drag());
    assert_eq!(visual_crosshair_presence(&mut chart), (true, true));
}

#[test]
fn drawing_hover_and_drag_suppress_the_visual_crosshair_but_creation_keeps_it() {
    let mut chart = crosshair_chart();
    configure_distinct_crosshair(&mut chart);
    let id = chart
        .add_drawing(
            crate::DrawingKind::TrendLine,
            0,
            vec![
                crate::DrawingPoint {
                    logical: 1.0,
                    price: 10.5,
                },
                crate::DrawingPoint {
                    logical: 2.0,
                    price: 12.0,
                },
            ],
            None,
        )
        .unwrap();
    assert_eq!(visual_crosshair_presence(&mut chart), (true, true));

    chart.set_hovered_drawing(Some(id));
    assert!(chart.crosshair_suppressed_by_interaction());
    assert_eq!(visual_crosshair_presence(&mut chart), (false, false));
    chart.set_hovered_drawing(None);
    assert_eq!(visual_crosshair_presence(&mut chart), (true, true));

    let (x, y) = chart.drawing_point_to_coordinate(id, 0).unwrap();
    assert!(chart.drawing_drag_start_at(x, y));
    assert_eq!(visual_crosshair_presence(&mut chart), (false, false));
    chart.drawing_drag_end();
    assert_eq!(visual_crosshair_presence(&mut chart), (true, true));

    assert!(chart.drawing_create_begin(crate::DrawingKind::Rectangle, None));
    assert!(!chart.crosshair_suppressed_by_interaction());
    assert_eq!(visual_crosshair_presence(&mut chart), (true, true));
    // Placing a new drawing across an existing one keeps the crosshair the anchor snaps to.
    chart.set_hovered_drawing(Some(id));
    assert_eq!(visual_crosshair_presence(&mut chart), (true, true));
    chart.set_hovered_drawing(None);
    chart.drawing_create_cancel();
    assert_eq!(visual_crosshair_presence(&mut chart), (true, true));

    let (x, y) = (chart.time_scale.index_to_coordinate(1), 250.0);
    assert!(chart.brush_create_start(None, x, y));
    chart.brush_create_add(x + 40.0, y + 20.0);
    assert_eq!(visual_crosshair_presence(&mut chart), (true, true));
    chart.brush_create_cancel();
    assert_eq!(visual_crosshair_presence(&mut chart), (true, true));
}

#[test]
fn crosshair_column_contains_the_wick_column_at_any_dpr() {
    // The vertical crosshair must land exactly on the hovered candle's wick at every device
    // pixel ratio — including sub-1 browser zoom (75%/80%), where floor(dpr) = 0 once
    // collapsed the wick one column left of the crosshair.
    for dpr in [0.67, 0.75, 0.8, 0.9, 1.0, 1.25, 1.5, 2.0] {
        let mut chart = ChartEngine::new(800.0, 500.0, dpr);
        let times: Vec<f64> = (0..20).map(|i| i as f64 * 60.0).collect();
        let base: Vec<f64> = (0..20)
            .map(|i| 100.0 + (i as f64 * 0.7).sin() * 3.0)
            .collect();
        let opens: Vec<f64> = base.clone();
        let closes: Vec<f64> = base.iter().map(|v| v + 0.4).collect();
        let highs: Vec<f64> = base.iter().map(|v| v + 1.2).collect();
        let lows: Vec<f64> = base.iter().map(|v| v - 1.2).collect();
        chart
            .set_series_data(0, &times, &opens, &highs, &lows, &closes)
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart
            .apply_options(r#"{"timeScale": {"barSpacing": 8.0, "rightOffset": 2.0}}"#)
            .unwrap();
        let idx = 10i64;
        let x_css = chart.time_scale.index_to_coordinate(idx);
        chart.crosshair = Some((x_css, 250.0));
        let frame = chart.build_frame();
        let (cross_x, cross_w) = frame.panes[0]
            .main
            .iter()
            .find_map(|p| match p {
                Prim::VLine { x, width, .. } => Some((*x, *width)),
                _ => None,
            })
            .expect("crosshair vertical line");
        // The target bar's wick: the narrow rect columns nearest the bar's device center.
        let center_dev = (x_css * dpr).round() as i32;
        let wicks: Vec<aeris_charts_render::draw_list::IRect> = frame.panes[0]
            .main
            .iter()
            .filter_map(|p| match p {
                Prim::Rect { rect, .. } if rect.w <= 2 && (rect.x - center_dev).abs() <= 3 => {
                    Some(*rect)
                }
                _ => None,
            })
            .collect();
        assert!(!wicks.is_empty(), "dpr {dpr}: wick rects not found");
        for wick in wicks {
            assert!(
                wick.x > cross_x - cross_w && wick.x + wick.w - 1 <= cross_x,
                "dpr {dpr}: wick {wick:?} escapes the crosshair column ({cross_x} w{cross_w})"
            );
        }
    }
}

#[test]
fn crosshair_clamps_into_pane_instead_of_vanishing() {
    let mut chart = crosshair_chart();
    // reference pane-widget.ts:714-719: out-of-range positions clamp instead of hiding the crosshair.
    chart.crosshair = Some((10_000.0, 10_000.0));
    assert_eq!(chart.clamped_crosshair(), Some((799.0, 499.0)));
    let frame = chart.build_frame();
    assert!(
        frame.panes[0]
            .main
            .iter()
            .any(|p| matches!(p, Prim::VLine { .. }))
    );
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::HLine {
            style: LineStyle::Dotted,
            ..
        }
    )));

    chart.crosshair = Some((-50.0, -50.0));
    assert_eq!(chart.clamped_crosshair(), Some((0.0, 0.0)));
    let frame = chart.build_frame();
    assert!(
        frame.panes[0]
            .main
            .iter()
            .any(|p| matches!(p, Prim::VLine { .. }))
    );

    // Hidden mode still suppresses the crosshair entirely.
    chart.crosshair_mode = CrosshairMode::Hidden;
    let frame = chart.build_frame();
    assert!(
        !frame.panes[0]
            .main
            .iter()
            .any(|p| matches!(p, Prim::VLine { .. }))
    );
}

#[test]
fn crosshair_draws_without_a_primary_series() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    // The primary series (id 0) stays empty; a secondary line series carries the data.
    let secondary = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(
            secondary,
            &[1.0, 2.0, 3.0],
            &[10.0, 11.0, 12.0],
            &[10.0, 11.0, 12.0],
            &[10.0, 11.0, 12.0],
            &[10.0, 11.0, 12.0],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.crosshair = Some((200.0, 120.0));

    let frame = chart.build_frame();
    assert!(
        frame.panes[0]
            .main
            .iter()
            .any(|p| matches!(p, Prim::VLine { .. }))
    );
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::HLine {
            style: LineStyle::Dotted,
            ..
        }
    )));

    // The time label needs only the time scale; the price label comes off the containing pane's
    // default scale (the secondary series' right scale here).
    let axis = chart.build_axis_frame(
        80.0,
        |t, _bold| t.len() as f64 * 7.0,
        |t, _bold| t.len() as f64 * 6.0,
    );
    assert!(
        axis.labels
            .iter()
            .any(|l| l.midpoint == AxisTextMidpoint::StableTime)
    );
    assert!(axis.labels.iter().any(|l| l.background.is_some()));
}

#[test]
fn magnet_snaps_across_all_visible_series_on_the_pane() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.crosshair_mode = CrosshairMode::Magnet;
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0],
            &[99.0, 100.0, 101.0],
            &[99.0, 100.0, 101.0],
            &[99.0, 100.0, 101.0],
            &[99.0, 100.0, 101.0],
        )
        .unwrap();
    let other = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(
            other,
            &[1.0, 2.0, 3.0],
            &[109.0, 110.0, 111.0],
            &[109.0, 110.0, 111.0],
            &[109.0, 110.0, 111.0],
            &[109.0, 110.0, 111.0],
        )
        .unwrap();
    let left = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(
            left,
            &[1.0, 2.0, 3.0],
            &[490.0, 500.0, 510.0],
            &[490.0, 500.0, 510.0],
            &[490.0, 500.0, 510.0],
            &[490.0, 500.0, 510.0],
        )
        .unwrap();
    chart.set_series_price_scale(left, PriceScaleTarget::Left);
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.build_frame();
    let (from, to) = chart.visible_range_for_frame().unwrap();
    let right = pane_scale(&chart.panes[0], PriceScaleTarget::Right);
    let base = chart.series_base_value(0, from).unwrap();
    let x = chart.time_scale.index_to_coordinate(1);

    // Same-scale pick: the old primary-only magnet could only snap to the primary's 100.
    let y110 = right.price_to_coordinate(110.0, base);
    let (price, snapped_y) = chart.crosshair_snap(0, x, y110, from, to);
    assert_eq!(snapped_y, y110);
    assert!((price - 110.0).abs() < 1e-9);

    // Cross-scale pick: the left-scale series' bar converts on its own scale; the winning
    // coordinate converts back to a price on the pane's default (right) scale.
    let left_scale = pane_scale(&chart.panes[0], PriceScaleTarget::Left);
    let left_base = chart.series_base_value(left, from).unwrap();
    let y500 = left_scale.price_to_coordinate(500.0, left_base);
    let expected_on_default = right.coordinate_to_price(y500, base);
    let (price, snapped_y) = chart.crosshair_snap(0, x, y500, from, to);
    assert_eq!(snapped_y, y500);
    assert!((price - expected_on_default).abs() < 1e-9);
    assert!((price - 500.0).abs() > 1.0); // not the left-scale price
}

#[test]
fn crosshair_magnet_ignores_derived_indicator_lines() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.crosshair_mode = CrosshairMode::Magnet;
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0],
            &[10.0, 20.0, 100.0],
            &[10.0, 20.0, 100.0],
            &[10.0, 20.0, 100.0],
            &[10.0, 20.0, 100.0],
        )
        .unwrap();
    let sma = chart.add_sma(0, 2).expect("SMA output");
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.build_frame();

    let (from, to) = chart.visible_range_for_frame().unwrap();
    let scale = pane_scale(&chart.panes[0], PriceScaleTarget::Right);
    let base = chart.series_base_value(0, from).unwrap();
    let x = chart.time_scale.index_to_coordinate(1);
    let indicator_plot = chart.data.plot(sma);
    let indicator_row = indicator_plot
        .search(1, MismatchDirection::None)
        .expect("SMA row at logical index 1");
    let indicator_value = indicator_plot.value_at(indicator_row, PlotValueIndex::Close);
    assert_eq!(indicator_value, 15.0);
    let indicator_y = scale.price_to_coordinate(indicator_value, base);
    let source_y = scale.price_to_coordinate(20.0, base);

    let (price, snapped_y) = chart.crosshair_snap(0, x, indicator_y, from, to);
    assert_eq!(snapped_y, source_y);
    assert!((price - 20.0).abs() < 1e-9);
}

#[test]
fn magnet_ohlc_picks_nearest_of_open_high_low_close() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.crosshair_mode = CrosshairMode::MagnetOhlc;
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0],
            &[10.0, 20.0, 30.0],
            &[15.0, 28.0, 35.0],
            &[8.0, 12.0, 25.0],
            &[12.0, 24.0, 33.0],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.build_frame();
    let (from, to) = chart.visible_range_for_frame().unwrap();
    let scale = pane_scale(&chart.panes[0], PriceScaleTarget::Right);
    let base = chart.series_base_value(0, from).unwrap();
    let x = chart.time_scale.index_to_coordinate(1);

    // Bar 1: open 20, high 28, low 12, close 24 — cursor nearest the high picks 28.
    let y28 = scale.price_to_coordinate(28.0, base);
    let (price, _) = chart.crosshair_snap(0, x, y28, from, to);
    assert!((price - 28.0).abs() < 1e-9);
    // Cursor nearest the low picks 12.
    let y12 = scale.price_to_coordinate(12.0, base);
    let (price, _) = chart.crosshair_snap(0, x, y12, from, to);
    assert!((price - 12.0).abs() < 1e-9);
}

#[test]
fn magnet_ohlc_ignores_unpainted_columns_for_scalar_series() {
    for kind in [
        SeriesKind::Line,
        SeriesKind::Area,
        SeriesKind::Histogram,
        SeriesKind::Baseline,
    ] {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart.convert_series_kind(0, kind);
        chart.crosshair_mode = CrosshairMode::MagnetOhlc;
        chart
            .set_series_data(
                0,
                &[1.0, 2.0, 3.0],
                &[11.0, 22.0, 31.0],
                &[12.0, 25.0, 32.0],
                &[9.0, 15.0, 29.0],
                &[10.0, 20.0, 30.0],
            )
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        chart.build_frame();

        let (from, to) = chart.visible_range_for_frame().unwrap();
        let scale = pane_scale(&chart.panes[0], PriceScaleTarget::Right);
        let base = chart.series_base_value(0, from).unwrap();
        let x = chart.time_scale.index_to_coordinate(1);
        let hidden_high_y = scale.price_to_coordinate(25.0, base);
        let rendered_value_y = scale.price_to_coordinate(20.0, base);
        let (_, snapped_y) = chart.crosshair_snap(0, x, hidden_high_y, from, to);

        assert_eq!(
            snapped_y, rendered_value_y,
            "{kind:?} must snap to its painted close/value"
        );
    }
}

#[test]
fn normal_mode_keeps_the_raw_cursor_price() {
    let mut chart = crosshair_chart();
    chart.crosshair_mode = CrosshairMode::Normal;
    chart.build_frame();
    let (from, to) = chart.visible_range_for_frame().unwrap();
    let scale = pane_scale(&chart.panes[0], PriceScaleTarget::Right);
    let base = chart.series_base_value(0, from).unwrap();
    let x = chart.time_scale.index_to_coordinate(1);
    let (price, snapped_y) = chart.crosshair_snap(0, x, 120.0, from, to);
    assert_eq!(snapped_y, 120.0);
    assert!((price - scale.coordinate_to_price(120.0, base)).abs() < 1e-9);
}

#[test]
fn crosshair_labels_cover_every_visible_populated_price_scale() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0],
            &[100.0, 105.0, 110.0],
            &[102.0, 107.0, 112.0],
            &[98.0, 103.0, 108.0],
            &[101.0, 106.0, 111.0],
        )
        .unwrap();
    let left = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(
            left,
            &[1.0, 2.0, 3.0],
            &[1_000.0, 1_050.0, 1_100.0],
            &[1_000.0, 1_050.0, 1_100.0],
            &[1_000.0, 1_050.0, 1_100.0],
            &[1_000.0, 1_050.0, 1_100.0],
        )
        .unwrap();
    chart.set_series_price_scale(left, PriceScaleTarget::Left);
    chart
        .apply_options(
            r##"{
                "leftPriceScale":{"visible":true},
                "rightPriceScale":{"visible":true},
                "crosshair":{"horzLine":{"labelBackgroundColor":"#ff00ff"}}
            }"##,
        )
        .unwrap();
    assert!(
        chart.series_apply_price_format_json(0, r#"{"type":"price","precision":0,"min_move":1}"#)
    );
    assert!(
        chart.series_apply_price_format_json(
            left,
            r#"{"type":"price","precision":2,"min_move":0.01}"#
        )
    );
    chart.set_price_scale_mode_for(0, PriceScaleTarget::Left, PriceScaleMode::Logarithmic);
    chart.set_price_scale_inverted_for(0, PriceScaleTarget::Left, true);
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.build_frame();
    chart.crosshair = Some((chart.time_scale.index_to_coordinate(1), 220.0));

    let labels = chart.build_axis_frame(
        80.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    let magenta = Color::rgb(0xff, 0x00, 0xff);
    let mut crosshair: Vec<&AxisLabel> = labels
        .labels
        .iter()
        .filter(|label| {
            !label.text.is_empty()
                && matches!(label.background, Some((.., color)) if color == magenta)
                && label.midpoint == AxisTextMidpoint::Label
        })
        .collect();
    crosshair.sort_by(|a, b| a.x.total_cmp(&b.x));

    assert_eq!(crosshair.len(), 2);
    assert_eq!(crosshair[0].align, AxisTextAlign::Right);
    assert_eq!(crosshair[1].align, AxisTextAlign::Left);
    assert_eq!(crosshair[0].y, crosshair[1].y);
    assert!(crosshair[0].text.contains('.'));
    assert!(!crosshair[1].text.contains('.'));

    chart
        .apply_options(r#"{"leftPriceScale":{"visible":false}}"#)
        .unwrap();
    let labels = chart.build_axis_frame(
        80.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    assert_eq!(
        labels
            .labels
            .iter()
            .filter(|label| {
                !label.text.is_empty()
                    && matches!(label.background, Some((.., color)) if color == magenta)
            })
            .count(),
        1
    );

    chart
        .apply_options(r#"{"leftPriceScale":{"visible":true}}"#)
        .unwrap();
    chart
        .set_series_data(left, &[], &[], &[], &[], &[])
        .unwrap();
    chart.build_frame();
    let labels = chart.build_axis_frame(
        80.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    assert_eq!(
        labels
            .labels
            .iter()
            .filter(|label| {
                !label.text.is_empty()
                    && matches!(label.background, Some((.., color)) if color == magenta)
            })
            .count(),
        1
    );
}

#[test]
fn crosshair_labels_share_y_across_crosshair_and_scale_modes() {
    for crosshair_mode in [
        CrosshairMode::Normal,
        CrosshairMode::Magnet,
        CrosshairMode::MagnetOhlc,
    ] {
        for mode in [
            PriceScaleMode::Normal,
            PriceScaleMode::Logarithmic,
            PriceScaleMode::Percentage,
            PriceScaleMode::IndexedTo100,
        ] {
            let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
            chart.crosshair_mode = crosshair_mode;
            chart
                .set_series_data(
                    0,
                    &[1.0, 2.0, 3.0],
                    &[100.0, 110.0, 120.0],
                    &[105.0, 118.0, 125.0],
                    &[95.0, 104.0, 115.0],
                    &[102.0, 112.0, 122.0],
                )
                .unwrap();
            let left = chart.add_series(SeriesKind::Line);
            chart
                .set_series_data(
                    left,
                    &[1.0, 2.0, 3.0],
                    &[1_000.0, 1_100.0, 1_200.0],
                    &[1_000.0, 1_100.0, 1_200.0],
                    &[1_000.0, 1_100.0, 1_200.0],
                    &[1_000.0, 1_100.0, 1_200.0],
                )
                .unwrap();
            chart.set_series_price_scale(left, PriceScaleTarget::Left);
            chart
                .apply_options(
                    r##"{
                    "leftPriceScale":{"visible":true},
                    "rightPriceScale":{"visible":true},
                    "crosshair":{"horzLine":{"labelBackgroundColor":"#ff00ff"}}
                }"##,
                )
                .unwrap();
            chart.set_price_scale_mode_for(0, PriceScaleTarget::Right, mode);
            chart.set_price_scale_mode_for(0, PriceScaleTarget::Left, mode);
            chart.set_price_scale_inverted_for(0, PriceScaleTarget::Left, true);
            chart.time_scale.set_width(800.0);
            chart.fit_content();
            chart.build_frame();
            let from = chart.visible_range_for_frame().unwrap().0;
            let left_base = chart.series_base_value(left, from).unwrap();
            let snap_y = chart.panes[0]
                .left_scale
                .price_to_coordinate(1_100.0, left_base);
            let x = chart.time_scale.index_to_coordinate(1);
            chart.crosshair = Some((x, snap_y + 1.0));
            let (from, to) = chart.visible_range_for_frame().unwrap();
            let expected_snap_y = chart.crosshair_snap(0, x, snap_y + 1.0, from, to).1;

            let labels = chart.build_axis_frame(
                80.0,
                |text, _bold| text.len() as f64 * 7.0,
                |text, _bold| text.len() as f64 * 6.0,
            );
            let magenta = Color::rgb(0xff, 0x00, 0xff);
            let crosshair: Vec<&AxisLabel> = labels
                .labels
                .iter()
                .filter(|label| {
                    !label.text.is_empty()
                        && matches!(label.background, Some((.., color)) if color == magenta)
                })
                .collect();
            assert_eq!(
                crosshair.len(),
                2,
                "crosshair {crosshair_mode:?}, scale {mode:?}"
            );
            assert!(
                crosshair
                    .iter()
                    .all(|label| (label.y - expected_snap_y).abs() < 1e-9),
                "crosshair {crosshair_mode:?}, scale {mode:?} did not retain the shared coordinate"
            );
        }
    }
}

#[test]
fn named_scale_crosshair_labels_use_exact_strips_ranges_and_formatters() {
    let mut chart = ChartEngine::new(900.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0],
            &[100.0, 105.0, 110.0],
            &[100.0, 105.0, 110.0],
            &[100.0, 105.0, 110.0],
            &[100.0, 105.0, 110.0],
        )
        .unwrap();
    let outer_series = chart.add_series(SeriesKind::Line);
    let left_series = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(
            outer_series,
            &[1.0, 2.0, 3.0],
            &[1_000.0, 1_050.0, 1_100.0],
            &[1_000.0, 1_050.0, 1_100.0],
            &[1_000.0, 1_050.0, 1_100.0],
            &[1_000.0, 1_050.0, 1_100.0],
        )
        .unwrap();
    chart
        .set_series_data(
            left_series,
            &[1.0, 2.0, 3.0],
            &[10.0, 10.5, 11.0],
            &[10.0, 10.5, 11.0],
            &[10.0, 10.5, 11.0],
            &[10.0, 10.5, 11.0],
        )
        .unwrap();
    let outer = chart
        .add_price_scale(0, "outer", PriceScaleSide::Right, None, true)
        .unwrap();
    let left = chart
        .add_price_scale(0, "comparison-left", PriceScaleSide::Left, Some(0), true)
        .unwrap();
    chart.set_series_price_scale(outer_series, outer);
    chart.set_series_price_scale(left_series, left);
    chart.set_price_scale_mode_for(0, outer, PriceScaleMode::Logarithmic);
    chart.set_price_scale_inverted_for(0, outer, true);
    chart.set_price_scale_mode_for(0, left, PriceScaleMode::Percentage);
    assert!(
        chart.series_apply_price_format_json(0, r#"{"type":"price","precision":0,"min_move":1}"#)
    );
    assert!(chart.series_apply_price_format_json(
        outer_series,
        r#"{"type":"price","precision":2,"min_move":0.01}"#
    ));
    assert!(chart.series_apply_price_format_json(
        left_series,
        r#"{"type":"price","precision":3,"min_move":0.001}"#
    ));
    chart
        .apply_options(r##"{"crosshair":{"horzLine":{"labelBackgroundColor":"#00ffff"}}}"##)
        .unwrap();
    chart.fit_content();
    chart.recompute_layout_with_measure(
        true,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    chart.build_frame();
    chart.crosshair = Some((chart.time_scale.index_to_coordinate(1), 210.0));

    let axis = chart.build_axis_frame(
        80.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    let cyan = Color::rgb(0x00, 0xff, 0xff);
    let crosshair: Vec<_> = axis
        .labels
        .iter()
        .filter(|label| {
            label.text != "+"
                && label.midpoint == AxisTextMidpoint::Label
                && matches!(label.background, Some((.., color)) if color == cyan)
        })
        .collect();
    assert_eq!(crosshair.len(), 3);
    assert!(
        crosshair
            .iter()
            .all(|label| (label.y - crosshair[0].y).abs() < 1e-9)
    );
    assert!(crosshair.iter().any(|label| !label.text.contains('.')));
    assert!(crosshair.iter().any(|label| label.text.ends_with('%')));
    assert!(
        crosshair
            .iter()
            .any(|label| label.text.contains('.') && !label.text.ends_with('%'))
    );

    for target in [PriceScaleTarget::Right, outer, left] {
        let (side, strip_x, strip_width) = chart
            .price_scale_axis_geometry(0, target)
            .expect("visible scale geometry");
        assert!(crosshair.iter().any(|label| {
            let Some((background_x, _, width, _, _)) = label.background else {
                return false;
            };
            match side {
                PriceScaleSide::Right => (background_x - strip_x).abs() < 1e-9,
                PriceScaleSide::Left => {
                    (background_x + width - (strip_x + strip_width)).abs() < 1e-9
                }
            }
        }));
    }

    chart.set_price_scale_visible_for(0, left, false);
    let axis = chart.build_axis_frame(
        80.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    assert_eq!(
        axis.labels
            .iter()
            .filter(|label| {
                label.text != "+" && matches!(label.background, Some((.., color)) if color == cyan)
            })
            .count(),
        2
    );
    chart
        .set_series_data(outer_series, &[], &[], &[], &[], &[])
        .unwrap();
    chart.build_frame();
    let axis = chart.build_axis_frame(
        80.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    assert_eq!(
        axis.labels
            .iter()
            .filter(|label| {
                label.text != "+" && matches!(label.background, Some((.., color)) if color == cyan)
            })
            .count(),
        1
    );
}

#[test]
fn grid_uses_the_innermost_populated_scale_and_prefers_right_on_equal_order() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0],
            &[100.0, 105.0, 110.0],
            &[100.0, 105.0, 110.0],
            &[100.0, 105.0, 110.0],
            &[100.0, 105.0, 110.0],
        )
        .unwrap();
    let left_series = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(
            left_series,
            &[1.0, 2.0, 3.0],
            &[1_000.0, 1_500.0, 2_000.0],
            &[1_000.0, 1_500.0, 2_000.0],
            &[1_000.0, 1_500.0, 2_000.0],
            &[1_000.0, 1_500.0, 2_000.0],
        )
        .unwrap();
    let left = chart
        .add_price_scale(0, "left-grid", PriceScaleSide::Left, Some(0), true)
        .unwrap();
    chart.set_series_price_scale(left_series, left);
    chart
        .apply_options(
            r##"{"grid":{"vertLines":{"visible":false},"horzLines":{"visible":true,"color":"#010203"}}}"##,
        )
        .unwrap();
    chart.fit_content();
    chart.build_frame();

    let expected: Vec<i32> = chart.panes[0]
        .price_scale
        .build_tick_marks(chart.scale_tick_base(0, PriceScaleTarget::Right), 0.0)
        .into_iter()
        .map(|mark| mark.coord.round() as i32)
        .collect();
    let grid_color = Color::rgb(0x01, 0x02, 0x03);
    let actual: Vec<i32> = chart.build_frame().panes[0]
        .under
        .iter()
        .filter_map(|prim| match prim {
            Prim::HLine { y, color, .. } if *color == grid_color => Some(*y),
            _ => None,
        })
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn do_not_snap_to_hidden_series_indices_moves_to_a_visible_bar() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    // Primary (visible) bars sit at merged indices 0, 1, 3; a hidden series owns index 2.
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 4.0],
            &[10.0, 11.0, 12.0],
            &[10.0, 11.0, 12.0],
            &[10.0, 11.0, 12.0],
            &[10.0, 11.0, 12.0],
        )
        .unwrap();
    let hidden = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(hidden, &[3.0], &[20.0], &[20.0], &[20.0], &[20.0])
        .unwrap();
    chart.set_series_visible(hidden, false);
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let (from, to) = chart.visible_range_for_frame().unwrap();
    assert_eq!((from, to), (0, 3));
    let x2 = chart.time_scale.index_to_coordinate(2);

    // Default (off): the snapped index stays on the hidden-only bar.
    assert_eq!(chart.snapped_crosshair_index(x2), 2);

    // On: it moves to the nearest visible-series bar; the tie resolves left (reference `indexOf(min)`).
    chart
        .options
        .apply_str(r#"{"crosshair":{"doNotSnapToHiddenSeriesIndices":true}}"#)
        .unwrap();
    assert_eq!(chart.snapped_crosshair_index(x2), 1);

    // The drawn vertical line follows the moved index.
    chart.crosshair = Some((x2, 120.0));
    let frame = chart.build_frame();
    let expected_x = chart.time_scale.index_to_coordinate(1).round() as i32;
    assert!(
        frame.panes[0]
            .main
            .iter()
            .any(|p| matches!(p, Prim::VLine { x, .. } if *x == expected_x))
    );
}

/// Two identical line series on the right scale (same last close => colliding label
/// candidates, the case the reference's overlap resolution exists for).
fn two_identical_line_series() -> ChartEngine {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Line;
    let times = [0.0, 60.0, 120.0, 180.0, 240.0];
    let values = [10.0, 11.0, 12.0, 11.5, 12.5];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    let second = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(second, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart
}

#[test]
fn last_value_labels_cover_every_visible_series_and_resolve_overlap() {
    let mut chart = two_identical_line_series();
    let boxed = |chart: &mut ChartEngine| {
        chart
            .build_axis_frame(
                80.0,
                |t, _bold| t.len() as f64 * 7.0,
                |t, _bold| t.len() as f64 * 6.0,
            )
            .labels
            .into_iter()
            .filter(|l| l.background.is_some())
            .collect::<Vec<_>>()
    };

    // reference SeriesPriceAxisView: every visible series with lastValueVisible gets a label on its
    // scale, in the series' bar color (the line color for a line series — not up/down).
    // Colliding chips are SPACED, never restyled: both stay solid while their values are live.
    let labels = boxed(&mut chart);
    assert_eq!(labels.len(), 2);
    assert!(
        labels.iter().all(|label| label.border.is_none()),
        "overlapping chips stay filled — collision is resolved by spacing, not by hollowing"
    );
    // reference `_fixLabelOverlap`: colliding labels are pushed apart by their box height
    // (shared 15px price row).
    let height = 11.0 + 2.0 * 2.0;
    let gap = (labels[0].y - labels[1].y).abs();
    assert!(
        (gap - height).abs() < 1e-9,
        "overlapping labels must be pushed a full box height apart, got {gap}"
    );

    // Separated values are likewise solid.
    let separated = [20.0, 21.0, 22.0, 21.5, 22.5];
    chart
        .set_series_data(
            1,
            &[1.0, 2.0, 3.0, 4.0, 5.0],
            &separated,
            &separated,
            &separated,
            &separated,
        )
        .unwrap();
    let labels = boxed(&mut chart);
    assert_eq!(labels.len(), 2);
    assert!(labels.iter().all(|label| label.border.is_none()));

    // `lastValueVisible: false` on one series drops only its label.
    chart.series[1].last_value_visible = false;
    assert_eq!(boxed(&mut chart).len(), 1);

    // A hidden series loses its label entirely (reference series-price-axis-view.ts:24).
    chart.series[1].last_value_visible = true;
    chart.set_series_visible(1, false);
    assert_eq!(boxed(&mut chart).len(), 1);
}

#[test]
fn price_line_tag_makes_room_for_live_label_then_returns_to_its_price() {
    let mut chart = two_identical_line_series();
    chart.set_series_visible(1, false);
    let line = chart.create_price_line(0, 12.5, LINE, 1, LineStyle::Solid, "target");
    assert_ne!(line, 0);
    let labels = |chart: &mut ChartEngine| {
        chart
            .build_axis_frame(
                80.0,
                |t, _| t.len() as f64 * 7.0,
                |t, _| t.len() as f64 * 6.0,
            )
            .labels
    };
    let crowded = labels(&mut chart);
    let live = crowded
        .iter()
        .find(|label| label.text == "12.50" && label.background.is_some())
        .unwrap();
    let target = crowded.iter().find(|label| label.text == "target").unwrap();
    let live_box = live.background.unwrap();
    let target_box = target.background.unwrap();
    assert!(
        target_box.1 + target_box.3 <= live_box.1 || live_box.1 + live_box.3 <= target_box.1,
        "the target and live price tags must not cover each other"
    );

    for (close, above) in [(12.49, true), (12.51, false)] {
        let values = [10.0, 11.0, 12.0, 11.5, close];
        chart
            .set_series_data(
                0,
                &[0.0, 60.0, 120.0, 180.0, 240.0],
                &values,
                &values,
                &values,
                &values,
            )
            .unwrap();
        let crossing = labels(&mut chart);
        let target = crossing
            .iter()
            .find(|label| label.text == "target")
            .unwrap();
        let raw = chart
            .runtime_price_coordinate(0, PriceScaleTarget::Right, 12.5)
            .unwrap();
        assert_eq!(
            target.y < raw,
            above,
            "tag should move to the free side as the live price crosses"
        );
    }

    let values = [10.0, 11.0, 12.0, 11.5, 14.0];
    chart
        .set_series_data(
            0,
            &[0.0, 60.0, 120.0, 180.0, 240.0],
            &values,
            &values,
            &values,
            &values,
        )
        .unwrap();
    let clear = labels(&mut chart);
    let target = clear.iter().find(|label| label.text == "target").unwrap();
    let raw = chart
        .runtime_price_coordinate(0, PriceScaleTarget::Right, 12.5)
        .unwrap();
    assert!(
        (target.y - raw).abs() < 1e-9,
        "tag must return to its price when clear"
    );
}

#[test]
fn overfull_price_axis_omits_tags_that_have_no_free_slot() {
    let mut chart = ChartEngine::new(800.0, 120.0, 1.0);
    let values = [10.0, 11.0, 12.0, 11.5, 12.5];
    chart
        .set_series_data(
            0,
            &[0.0, 60.0, 120.0, 180.0, 240.0],
            &values,
            &values,
            &values,
            &values,
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    for i in 0..12 {
        chart.create_price_line(0, 12.5, LINE, 1, LineStyle::Solid, &format!("tag-{i}"));
    }
    let labels = chart
        .build_axis_frame(
            80.0,
            |t, _| t.len() as f64 * 7.0,
            |t, _| t.len() as f64 * 6.0,
        )
        .labels;
    let tags: Vec<_> = labels
        .iter()
        .filter(|label| label.text.starts_with("tag-") && label.background.is_some())
        .collect();
    assert!(!tags.is_empty() && tags.len() < 12);
    for (index, tag) in tags.iter().enumerate() {
        let first = tag.background.unwrap();
        for other in &tags[index + 1..] {
            let second = other.background.unwrap();
            assert!(first.1 + first.3 <= second.1 || second.1 + second.3 <= first.1);
        }
    }
}

/// the public reference marks a last-value chip hollow only when its value is no longer live — i.e. the
/// series' final bar has been scrolled out of view (what a negative right offset produces).
#[test]
fn last_value_chips_hollow_only_once_the_final_bar_leaves_the_view() {
    let mut chart = two_identical_line_series();
    let boxed = |chart: &mut ChartEngine| {
        chart
            .build_axis_frame(
                80.0,
                |t, _bold| t.len() as f64 * 7.0,
                |t, _bold| t.len() as f64 * 6.0,
            )
            .labels
            .into_iter()
            .filter(|l| l.background.is_some())
            .collect::<Vec<_>>()
    };

    // Fitted: the last bar is on screen, so every chip is filled.
    assert!(chart.right_offset() >= 0.0);
    assert!(boxed(&mut chart).iter().all(|label| label.border.is_none()));

    // Whitespace after the last bar (positive right offset) is still live.
    chart.set_right_offset(6.0);
    chart.build_frame();
    assert!(boxed(&mut chart).iter().all(|label| label.border.is_none()));

    // Scrolled back far enough to push the final bar off the right edge: the SECONDARY chip
    // outlines, because it is now showing the last visible bar rather than its latest value.
    // The scale's primary source keeps its filled chip — the symbol the scale belongs to is
    // never rendered as stale.
    chart.set_right_offset(-3.0);
    chart.build_frame();
    let labels = boxed(&mut chart);
    assert_eq!(labels.len(), 2);
    assert_eq!(
        labels
            .iter()
            .filter(|label| label.border == Some((1.0, LINE)))
            .count(),
        1,
        "only the non-primary chip outlines when the value goes stale"
    );
    assert_eq!(
        labels.iter().filter(|label| label.border.is_none()).count(),
        1,
        "the scale's primary chip stays filled"
    );

    // Scrolling back to the end fills them again.
    chart.set_right_offset(0.0);
    chart.build_frame();
    assert!(boxed(&mut chart).iter().all(|label| label.border.is_none()));
}

#[test]
fn last_value_label_tracks_the_last_visible_bar() {
    let mut chart = two_identical_line_series();
    chart.series[1].last_value_visible = false;
    // Scroll one bar past the right edge: the label follows the last *visible* bar (reference
    // series.ts lastValueData(false)), not the series' final bar.
    chart.set_right_offset(-1.0);
    let labels = chart
        .build_axis_frame(
            80.0,
            |t, _bold| t.len() as f64 * 7.0,
            |t, _bold| t.len() as f64 * 6.0,
        )
        .labels;
    let label = labels
        .iter()
        .find(|l| l.background.is_some())
        .expect("last-value label");
    let (_, to) = chart.visible_range_for_frame().unwrap();
    let scale = pane_scale(&chart.panes[0], PriceScaleTarget::Right);
    let base = chart.series_base_value(0, 0).unwrap();
    let expected_y = scale.price_to_coordinate(11.5, base); // close of bar index `to` = 3
    assert_eq!(to, 3);
    assert!((label.y - expected_y).abs() < 1e-9);
}

#[test]
fn price_line_family_renders_per_series_with_reference_defaults() {
    let mut chart = two_identical_line_series();
    let dashed_ylines = |chart: &mut ChartEngine| {
        chart.build_frame().panes[0]
            .main
            .iter()
            .filter_map(|p| match p {
                Prim::HLine {
                    y,
                    style: LineStyle::Dotted,
                    width,
                    color,
                    x0,
                    x1,
                } => Some((*y, *width, *color, *x0, *x1)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    // Every visible series gets the Aeris built-in live-price line by default: partial extent
    // from its tracked data point to the pane edge, dotted, 1px, following the series/bar color.
    let lines = dashed_ylines(&mut chart);
    assert_eq!(lines.len(), 2);
    assert!(
        lines
            .iter()
            .all(|&(_, width, color, x0, x1)| width == 1 && color == LINE && x0 > 0 && x1 > x0)
    );

    // priceLineVisible: false hides only that series' line.
    chart.series[1].price_line_visible = false;
    assert_eq!(dashed_ylines(&mut chart).len(), 1);

    // priceLineSource LastVisible anchors at the last visible bar when the final bar is
    // scrolled off the right edge; LastBar keeps the final bar.
    chart.series[0].price_line_source = 1;
    chart.series[1].price_line_source = 0;
    chart.series[1].price_line_visible = true;
    // A full line remains visible even when its LastBar anchor is beyond the viewport; the
    // default partial line naturally appears only when its tracked anchor is in or left of view.
    chart.series[1].price_line_extent = crate::PriceLineExtent::Full;
    chart.set_right_offset(-1.0);
    let lines = dashed_ylines(&mut chart); // builds the frame, autoscaling the new window first
    let scale = pane_scale(&chart.panes[0], PriceScaleTarget::Right);
    let base = chart.series_base_value(0, 0).unwrap();
    let y_last_visible = (scale.price_to_coordinate(11.5, base) * 1.0).round() as i32;
    let y_last_bar = (scale.price_to_coordinate(12.5, base) * 1.0).round() as i32;
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().any(|&(y, ..)| y == y_last_visible));
    assert!(lines.iter().any(|&(y, ..)| y == y_last_bar));

    // Width/color/style are the same canonical options for partial and full extents.
    chart.series[0].price_line_width = 3.0;
    chart.series[0].price_line_color = Some("#112233".to_string());
    chart.series[0].price_line_style = 0;
    chart.series[0].price_line_extent = crate::PriceLineExtent::Full;
    chart.set_right_offset(0.0);
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::HLine {
            x0: 0,
            width: 3,
            style: LineStyle::Solid,
            color,
            ..
        } if *color == Color::rgb(0x11, 0x22, 0x33)
    )));
}

#[test]
fn a_partial_live_price_line_runs_toward_the_side_its_price_scale_is_on() {
    let mut chart = two_identical_line_series();
    chart.series[1].price_line_visible = false;
    let span = |chart: &mut ChartEngine| {
        let frame = chart.build_frame();
        let width = chart.pane_w.round() as i32;
        frame.panes[0]
            .main
            .iter()
            .find_map(|prim| match prim {
                Prim::HLine { x0, x1, color, .. } if *color == LINE => Some((*x0, *x1)),
                _ => None,
            })
            .map(|span| (span, width))
            .expect("the live price line")
    };
    // On the right scale: from the latest bar to the right edge.
    let ((x0, x1), width) = span(&mut chart);
    assert!(x0 > 0 && x1 == width, "{x0}..{x1} of {width}");
    let bar_x = x0;
    // On the left scale: from the left edge (its label) to the latest bar.
    chart.set_series_price_scale(0, PriceScaleTarget::Left);
    let ((x0, x1), _) = span(&mut chart);
    assert_eq!(x0, 0, "the line reaches the left-side label");
    assert!((x1 - bar_x).abs() <= 1, "{x1} vs bar {bar_x}");
    // A named scale moved to the left side follows its side too.
    let moved = chart
        .add_price_scale(0, "moved", PriceScaleSide::Right, None, true)
        .unwrap();
    chart.set_series_price_scale(0, moved);
    assert_eq!(span(&mut chart).0.1, span(&mut chart).1);
    assert!(chart.move_price_scale(0, moved, PriceScaleSide::Left, 0));
    assert_eq!(span(&mut chart).0.0, 0);
}

#[test]
fn indicator_outputs_inherit_partial_live_price_lines() {
    let mut chart = two_identical_line_series();
    chart.set_indicator_chrome_options(crate::IndicatorChromeOptions {
        price_lines_visible: true,
        ..crate::IndicatorChromeOptions::default()
    });
    let sma = chart.add_sma(0, 2).expect("sma output");
    assert_eq!(
        chart
            .series_entry(sma)
            .expect("indicator series")
            .price_line_extent,
        crate::PriceLineExtent::Partial
    );
    let indicator_color = Color::rgb(0xa1, 0xb2, 0xc3);
    chart
        .series_entry_mut(sma)
        .expect("indicator series")
        .price_line_color = Some("#a1b2c3".into());

    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|prim| matches!(
        prim,
        Prim::HLine { x0, x1, color, .. } if *x0 > 0 && *x1 > *x0 && *color == indicator_color
    )));
}

#[test]
fn bid_ask_lines_and_chips_render_only_when_enabled_with_values() {
    let mut chart = two_identical_line_series();
    let hlines = |chart: &mut ChartEngine| {
        chart.build_frame().panes[0]
            .main
            .iter()
            .filter_map(|p| match p {
                Prim::HLine { y, color, .. } => Some((*y, *color)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    // Default OFF: pushing quotes alone renders nothing. (Quotes are inside the fixture's
    // 10-12.5 scale range; out-of-range quotes are clipped by design.)
    chart.set_bid_ask(0, Some(11.0), Some(12.0));
    let primary = aeris_charts_core::style::DEFAULT_PRIMARY_RGB;
    let blue = Color::rgb(primary.0, primary.1, primary.2);
    let red = Color::rgb(0xf7, 0x52, 0x5f);
    assert!(
        !hlines(&mut chart)
            .iter()
            .any(|&(_, c)| c == blue || c == red)
    );
    // Enable: one line per side, on the quotes' coordinates (zero shift with an exact-range scale).
    chart.series_apply_options_json(0, r##"{"bid_ask_visible": true}"##);
    let lines = hlines(&mut chart);
    let scale = &chart.panes[0].price_scale;
    let bid_y = scale.price_to_coordinate(11.0, 0.0).round() as i32;
    let ask_y = scale.price_to_coordinate(12.0, 0.0).round() as i32;
    assert!(
        lines.iter().any(|&(y, c)| y == bid_y && c == blue),
        "bid line: {lines:?}"
    );
    assert!(
        lines.iter().any(|&(y, c)| y == ask_y && c == red),
        "ask line: {lines:?}"
    );
    // Axis chips: "Bid"/"Ask" title chips + price chips centered on the quotes.
    chart.now_override = Some(250.0);
    let labels = boxed_labels(&mut chart);
    let side_chip = |side: &str| {
        labels
            .iter()
            .find(|l| l.text == side)
            .unwrap_or_else(|| panic!("{side} title chip missing: {labels:?}"))
    };
    let bid_chip = side_chip("Bid");
    let ask_chip = side_chip("Ask");
    let foreground = LIVE_TEXT;
    assert_eq!(bid_chip.color, foreground);
    assert_eq!(ask_chip.color, foreground);
    assert!(
        (bid_chip.y - bid_y as f64).abs() < 1.0,
        "bid chip centers on the quote"
    );
    assert!(
        (ask_chip.y - ask_y as f64).abs() < 1.0,
        "ask chip centers on the quote"
    );
    assert!(labels.iter().any(|l| l.text == "11.00"));
    assert!(labels.iter().any(|l| l.text == "12.00"));
    // Light quote colors use black text throughout the shared title+price cluster.
    assert!(chart.series_apply_options_json(0, r##"{"ask_color": "#f0e68c"}"##));
    let labels = boxed_labels(&mut chart);
    let light = Color::rgb(0xf0, 0xe6, 0x8c);
    let ask_labels: Vec<_> = labels
        .iter()
        .filter(|label| label.text == "Ask" || label.text == "12.00")
        .collect();
    assert_eq!(ask_labels.len(), 2);
    assert!(ask_labels.iter().all(|label| {
        label.color == Color::rgb(0, 0, 0)
            && matches!(label.background, Some((.., color)) if color == light)
    }));
    // One side cleared via the options JSON: only the ask line + chips remain.
    assert!(chart.series_apply_options_json(0, r##"{"bid": null}"##));
    assert!(!hlines(&mut chart).iter().any(|&(_, c)| c == blue));
    assert!(hlines(&mut chart).iter().any(|&(_, c)| c == light));
    // Custom colors reach the frame verbatim-parsed; disabling hides everything.
    assert!(chart.series_apply_options_json(0, r##"{"ask_color": "#112233"}"##));
    assert!(
        hlines(&mut chart)
            .iter()
            .any(|&(_, c)| c == Color::rgb(0x11, 0x22, 0x33))
    );
    chart.series_apply_options_json(0, r##"{"bid_ask_visible": false}"##);
    assert!(
        !hlines(&mut chart)
            .iter()
            .any(|&(_, c)| c == Color::rgb(0x11, 0x22, 0x33))
    );
    // Options surface round-trips the full configuration.
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(options["bid_ask_visible"], false);
    assert_eq!(
        options["bid_color"],
        aeris_charts_core::style::DEFAULT_PRIMARY_CSS
    );
    assert_eq!(options["ask_color"], "#112233");
    assert_eq!(options["bid"], serde_json::Value::Null);
    assert_eq!(options["ask"], 12.0);
}

#[test]
fn price_line_color_css_string_parses_at_render_time() {
    let mut chart = two_identical_line_series();
    // Uppercase hex is stored verbatim (options() returns it as-is) and parsed only when the
    // frame resolves the line color.
    assert!(chart.series_apply_options_json(0, r##"{"price_line_color": "#FF0000"}"##));
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(options["price_line_color"], "#FF0000");
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::HLine { color, .. } if *color == Color::rgb(0xFF, 0x00, 0x00)
    )));

    // An unparseable string (named color) falls back to the follow-the-bar-color default.
    assert!(chart.series_apply_options_json(0, r#"{"price_line_color": "red"}"#));
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::HLine { color, .. } if *color == LINE
    )));
}

#[test]
fn explicit_price_line_color_unifies_the_live_line_and_complete_cluster() {
    let mut chart = countdown_chart();
    chart.now_override = Some(250.0);
    chart.series[0].title = "NDQ".to_string();
    chart.series[0].countdown_visible = true;
    chart.series[0].price_line_color = Some("#f0e68c".to_string());
    let live = Color::rgb(0xf0, 0xe6, 0x8c);

    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|primitive| matches!(
        primitive,
        Prim::HLine { color, style: LineStyle::Dotted, .. } if *color == live
    )));

    let labels = boxed_labels(&mut chart);
    assert_eq!(labels.len(), 3);
    assert!(
        labels
            .iter()
            .all(|label| matches!(label.background, Some((.., color)) if color == live))
    );
    assert_eq!(
        labels
            .iter()
            .find(|label| label.text == "NDQ")
            .unwrap()
            .color,
        Color::rgb(0, 0, 0)
    );
    assert_eq!(
        labels
            .iter()
            .find(|label| label.text == "12.50")
            .unwrap()
            .color,
        Color::rgb(0, 0, 0)
    );
    assert_eq!(
        labels
            .iter()
            .find(|label| label.text == "00:50")
            .unwrap()
            .color,
        Color::rgba(0, 0, 0, 0xb3)
    );
}

#[test]
fn dashed_line_style_splits_the_polyline_into_solid_runs() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Line;
    let times: Vec<f64> = (0..40).map(|i| i as f64).collect();
    let values: Vec<f64> = (0..40).map(|i| 100.0 + (i % 7) as f64).collect();
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.series[0].line_style = 2; // reference LineStyle.Dashed

    let frame = chart.build_frame();
    let runs: Vec<_> = frame.panes[0]
        .main
        .iter()
        .filter_map(|p| match p {
            Prim::Polyline {
                first_point,
                point_count,
                style,
                line_type,
                ..
            } => Some((*first_point, *point_count, *style, *line_type)),
            _ => None,
        })
        .collect();
    // A dashed stroke arrives as several solid sub-segments (gap geometry is frame-built, so
    // WebGPU and Canvas2D rasterize identical dashes).
    assert!(runs.len() > 1, "expected dash sub-segments, got {runs:?}");
    assert!(
        runs.iter()
            .all(|&(.., style, line_type)| style == LineStyle::Solid
                && line_type == LineType::Simple)
    );
    // The runs leave real gaps: their on-length totals less than the full path length.
    let pool = &frame.panes[0].points;
    let on_length: f32 = runs
        .iter()
        .map(|&(first, count, ..)| {
            let w = &pool[first as usize..(first + count) as usize];
            w.windows(2)
                .map(|p| ((p[1][0] - p[0][0]).powi(2) + (p[1][1] - p[0][1]).powi(2)).sqrt())
                .sum::<f32>()
        })
        .sum();
    let full_length: f32 = {
        // solid reference frame: same line with the default solid style
        let mut solid = ChartEngine::new(800.0, 500.0, 1.0);
        solid.series[0].kind = SeriesKind::Line;
        solid
            .set_series_data(0, &times, &values, &values, &values, &values)
            .unwrap();
        solid.time_scale.set_width(800.0);
        solid.fit_content();
        let frame = solid.build_frame();
        let run = frame.panes[0]
            .main
            .iter()
            .find_map(|p| match p {
                Prim::Polyline {
                    first_point,
                    point_count,
                    ..
                } => Some((*first_point, *point_count)),
                _ => None,
            })
            .expect("solid line polyline");
        let w = &frame.panes[0].points[run.0 as usize..(run.0 + run.1) as usize];
        w.windows(2)
            .map(|p| ((p[1][0] - p[0][0]).powi(2) + (p[1][1] - p[0][1]).powi(2)).sqrt())
            .sum::<f32>()
    };
    assert!(
        on_length < full_length * 0.95,
        "dashes must leave gaps: on {on_length} vs full {full_length}"
    );
}

#[test]
fn indicator_price_chip_matches_the_main_chip_width_and_stands_one_row_tall() {
    let mut chart = countdown_chart();
    chart
        .apply_options(r##"{"layout":{"textColor":"#333333","mutedTextColor":"#737373"}}"##)
        .unwrap();
    chart.now_override = Some(250.0);
    chart.series[0].title = "BTC".to_string();
    chart.series[0].countdown_visible = true;
    chart.add_sma(0, 2).expect("valid sma");

    let labels = boxed_labels(&mut chart);
    // Main: title chip + price chip + countdown row; the SMA: its name chip + price box (no
    // countdown) — one row tall like the main chip.
    assert_eq!(labels.len(), 5);
    let main_price = labels
        .iter()
        .find(|l| l.text == "12.50")
        .expect("main price chip");
    let sma_price = labels
        .iter()
        .find(|l| l.text == "12.00")
        .expect("sma price chip");
    let bg = |l: &AxisLabel| l.background.expect("boxed");
    assert_eq!(
        bg(main_price).2,
        bg(sma_price).2,
        "one shared chip width across the scale"
    );
    assert_eq!(
        bg(main_price).3,
        bg(sma_price).3,
        "the indicator's box is a single row tall"
    );
    // The indicator's name chip renders too (auto-titled), outside the strip.
    let sma_name = labels
        .iter()
        .find(|l| l.text == "SMA 2")
        .expect("sma name chip");
    assert_eq!(sma_price.color, LIVE_TEXT);
    assert_eq!(sma_name.color, LIVE_TEXT);
    assert_eq!(
        bg(sma_name).3,
        bg(sma_price).3,
        "name chip matches the row height"
    );
}

#[test]
fn indicator_price_chip_inherits_source_precision_at_creation() {
    let mut chart = countdown_chart();
    assert!(
        chart.series_apply_price_format_json(0, r#"{"type":"price","precision":0,"min_move":1}"#)
    );
    let sma = chart.add_sma(0, 2).expect("valid sma");

    assert_eq!(chart.series_format_price(sma, 12.34).as_deref(), Some("12"));
    let labels = boxed_labels(&mut chart);
    assert!(
        labels
            .iter()
            .any(|label| label.attach_group == Some(sma) && label.text == "12"),
        "indicator chip should use the source's zero-decimal format"
    );

    assert!(chart.series_apply_price_format_json(
        sma,
        r#"{"type":"price","precision":4,"min_move":0.0001}"#
    ));
    assert_eq!(
        chart.series_format_price(sma, 12.34).as_deref(),
        Some("12.3400")
    );
}

#[test]
fn runtime_price_format_rebuilds_scale_ticks_layout_and_autoscale() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0, 4.0],
            &[116_000.25, 116_010.25, 115_995.25, 116_020.25],
            &[116_012.75, 116_018.75, 116_006.75, 116_030.75],
            &[115_990.25, 115_998.25, 115_985.25, 116_010.25],
            &[116_008.25, 116_004.25, 116_001.25, 116_025.25],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.build_frame();
    assert_eq!(chart.scale_tick_base(0, PriceScaleTarget::Right), 100);

    for (precision, min_move, expected_base, decimal) in
        [(0, 1.0, 1, false), (2, 0.01, 100, true), (0, 1.0, 1, false)]
    {
        assert!(chart.series_apply_price_format_json(
            0,
            &format!(r#"{{"type":"price","precision":{precision},"min_move":{min_move}}}"#)
        ));
        let frame = chart.build_frame();
        assert_eq!(chart.frame_build_stats().layout_rebuilds, 1);
        assert_eq!(
            chart.scale_tick_base(0, PriceScaleTarget::Right),
            expected_base
        );
        let range = chart
            .price_scale_visible_range_for(0, PriceScaleTarget::Right)
            .unwrap();
        assert!(range.0 <= 115_985.25 && range.1 >= 116_030.75);
        assert!(
            range.1 - range.0 < 100.0,
            "unexpected autoscale range {range:?}"
        );
        let labels = chart.build_axis_frame(
            80.0,
            |text, _bold| text.len() as f64 * 7.0,
            |text, _bold| text.len() as f64 * 6.0,
        );
        let tick_labels: Vec<&str> = labels
            .labels
            .iter()
            .filter(|label| label.background.is_none() && label.align == AxisTextAlign::Left)
            .map(|label| label.text.as_str())
            .collect();
        assert!(!tick_labels.is_empty());
        assert_eq!(tick_labels.iter().any(|label| label.contains('.')), decimal);
        assert!(
            frame.panes[0].main.iter().any(|primitive| {
                matches!(primitive, Prim::Rect { .. } | Prim::Polyline { .. })
            })
        );
    }

    chart.set_price_scale_visible_range_for(0, PriceScaleTarget::Right, 115_900.0, 116_100.0);
    let manual = chart
        .price_scale_visible_range_for(0, PriceScaleTarget::Right)
        .unwrap();
    assert!(
        chart
            .series_apply_price_format_json(0, r#"{"type":"price","precision":2,"min_move":0.01}"#)
    );
    chart.build_frame();
    assert_eq!(
        chart
            .price_scale_visible_range_for(0, PriceScaleTarget::Right)
            .unwrap(),
        manual
    );
}

#[test]
fn scale_formatter_source_tracks_attached_z_order() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let second = chart.add_series(SeriesKind::Line);
    assert!(
        chart
            .series_apply_price_format_json(0, r#"{"type":"price","precision":2,"min_move":0.03}"#)
    );
    assert!(chart.series_apply_price_format_json(
        second,
        r#"{"type":"price","precision":4,"min_move":0.0001}"#
    ));

    assert_eq!(chart.scale_tick_base(0, PriceScaleTarget::Right), 33);
    assert_eq!(
        chart.scale_autoscale_min_move(0, PriceScaleTarget::Right),
        0.03
    );

    assert!(chart.set_series_order(vec![second, 0]));
    assert_eq!(chart.scale_tick_base(0, PriceScaleTarget::Right), 10_000);
    assert_eq!(
        chart.scale_autoscale_min_move(0, PriceScaleTarget::Right),
        0.0001
    );

    chart.set_series_visible(second, false);
    assert_eq!(chart.scale_tick_base(0, PriceScaleTarget::Right), 10_000);
    assert_eq!(
        chart.scale_autoscale_min_move(0, PriceScaleTarget::Right),
        0.0001
    );

    chart.set_price_scale_mode_for(0, PriceScaleTarget::Right, PriceScaleMode::Percentage);
    assert_eq!(chart.scale_tick_base(0, PriceScaleTarget::Right), 100);
    assert_eq!(
        chart.scale_autoscale_min_move(0, PriceScaleTarget::Right),
        1.0
    );
}

#[test]
fn hiding_sole_indicator_preserves_scale_format_and_layout() {
    let measure = |text: &str, _bold: bool| text.len() as f64 * 7.0;
    let countdown_measure = |text: &str, _bold: bool| text.len() as f64 * 6.0;
    let mut chart = ChartEngine::new(900.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0, 4.0],
            &[65_470.0, 65_480.0, 65_490.0, 65_500.0],
            &[65_480.0, 65_490.0, 65_500.0, 65_510.0],
            &[65_460.0, 65_470.0, 65_480.0, 65_490.0],
            &[65_475.0, 65_485.0, 65_495.0, 65_505.0],
        )
        .unwrap();
    assert!(
        chart.series_apply_price_format_json(0, r#"{"type":"price","precision":0,"min_move":1}"#)
    );
    let sma = chart.add_sma(0, 2).expect("valid sma");
    let indicator_scale = chart
        .add_price_scale(0, "indicator", PriceScaleSide::Right, None, true)
        .unwrap();
    chart.set_series_price_scale(0, PriceScaleTarget::Overlay);
    chart.set_series_price_scale(sma, indicator_scale);
    chart.fit_content();
    chart.recompute_layout_with_measure(true, measure, countdown_measure);
    chart.build_frame();
    let initial_axis = chart.build_axis_frame(80.0, measure, countdown_measure);
    let initial_ticks: Vec<_> = initial_axis
        .labels
        .iter()
        .filter(|label| label.background.is_none() && label.align == AxisTextAlign::Left)
        .map(|label| (label.y.to_bits(), label.text.clone()))
        .collect();
    let initial_range = chart
        .price_scale_visible_range_for(0, indicator_scale)
        .unwrap();
    let initial_width = chart.price_scale_axis_width(0, indicator_scale).unwrap();
    assert_eq!(chart.scale_tick_base(0, indicator_scale), 1);
    assert_eq!(chart.scale_autoscale_min_move(0, indicator_scale), 1.0);
    assert!(!initial_ticks.is_empty());
    assert!(initial_ticks.iter().all(|(_, label)| !label.contains('.')));
    // A tick under the indicator's own tag drops its text while the tag shows, so ticks match
    // by coordinate, and by text wherever both frames show it.
    let same_ticks = |a: &[(u64, String)], b: &[(u64, String)]| {
        a.len() == b.len()
            && a.iter().zip(b).all(|((ya, ta), (yb, tb))| {
                ya == yb && (ta == tb || ta.is_empty() || tb.is_empty())
            })
    };

    chart.set_series_visible(sma, false);
    assert!(chart.frame_requires_layout());
    chart.recompute_layout_with_measure(false, measure, countdown_measure);
    chart.build_frame();
    let hidden_axis = chart.build_axis_frame(80.0, measure, countdown_measure);
    let hidden_ticks: Vec<_> = hidden_axis
        .labels
        .iter()
        .filter(|label| label.background.is_none() && label.align == AxisTextAlign::Left)
        .map(|label| (label.y.to_bits(), label.text.clone()))
        .collect();
    assert_eq!(chart.scale_tick_base(0, indicator_scale), 1);
    assert_eq!(chart.scale_autoscale_min_move(0, indicator_scale), 1.0);
    assert!(same_ticks(&hidden_ticks, &initial_ticks));
    assert_eq!(
        chart.price_scale_visible_range_for(0, indicator_scale),
        Some(initial_range)
    );
    assert_eq!(
        chart.price_scale_axis_width(0, indicator_scale),
        Some(initial_width)
    );

    chart.set_series_visible(sma, true);
    assert!(chart.frame_requires_layout());
    chart.recompute_layout_with_measure(false, measure, countdown_measure);
    chart.build_frame();
    let shown_axis = chart.build_axis_frame(80.0, measure, countdown_measure);
    let shown_ticks: Vec<_> = shown_axis
        .labels
        .iter()
        .filter(|label| label.background.is_none() && label.align == AxisTextAlign::Left)
        .map(|label| (label.y.to_bits(), label.text.clone()))
        .collect();
    assert_eq!(shown_ticks, initial_ticks);
    assert_eq!(
        chart.price_scale_visible_range_for(0, indicator_scale),
        Some(initial_range)
    );
    assert_eq!(
        chart.price_scale_axis_width(0, indicator_scale),
        Some(initial_width)
    );
    assert_retained_frame_matches_clean_rebuild(&mut chart);
}

#[test]
fn indicator_lines_support_dotted_and_dashed_styles() {
    let polylines = |chart: &mut ChartEngine| {
        chart.build_frame().panes[0]
            .main
            .iter()
            .filter(|p| matches!(p, Prim::Polyline { .. }))
            .count()
    };
    let mut chart = countdown_chart();
    let sma = chart.add_sma(0, 2).expect("valid sma");
    let solid = polylines(&mut chart);
    chart.series_entry_mut(sma).unwrap().line_style = 1; // dotted
    let dotted = polylines(&mut chart);
    assert!(dotted > solid, "dotted splits the stroke into dash runs");
    chart.series_entry_mut(sma).unwrap().line_style = 2; // dashed
    let dashed = polylines(&mut chart);
    assert!(dashed > solid, "dashed splits the stroke into dash runs");
}

#[test]
fn last_value_clusters_attach_only_within_their_own_series() {
    let mut chart = countdown_chart();
    chart.now_override = Some(250.0);
    chart.series[0].title = "BTC".to_string();
    chart.series[0].countdown_visible = true;
    // A second series with its own (countdown-only) cluster on the same strip.
    let extra = chart.add_series(SeriesKind::Histogram);
    chart.series_entry_mut(extra).unwrap().countdown_visible = true;
    let times = [0.0, 60.0, 120.0, 180.0, 240.0];
    let values = [1.0, 2.0, 3.0, 2.0, 1.0];
    chart
        .set_series_data(extra, &times, &values, &values, &values, &values)
        .unwrap();

    let labels = chart
        .build_axis_frame(
            80.0,
            |t, _bold| t.len() as f64 * 7.0,
            |t, _bold| t.len() as f64 * 6.0,
        )
        .labels;
    let groups: Vec<u32> = labels.iter().filter_map(|l| l.attach_group).collect();
    // The main cluster's chips share one group (the main series' id 0)…
    assert!(groups.contains(&0));
    // …and the volume cluster's chips share a DIFFERENT group (its own series id): clusters
    // never chain into each other (a shared constant merged them into one giant box).
    assert!(groups.contains(&extra));
    // Chips within one cluster: price + countdown share the same group id.
    let main_chips = groups.iter().filter(|&&g| g == 0).count();
    assert!(
        main_chips >= 2,
        "price + countdown chips share the cluster group"
    );
}

#[test]
fn horizontal_line_drawings_label_the_axis_in_the_line_color() {
    use crate::drawings::{DrawingKind, DrawingPoint};

    let mut chart = countdown_chart();
    let id = chart
        .add_drawing(
            DrawingKind::HorizontalLine,
            0,
            vec![DrawingPoint {
                logical: 1.0,
                price: 12.0,
            }],
            None,
        )
        .unwrap();
    let label_at = |chart: &mut ChartEngine| {
        chart
            .build_axis_frame(
                80.0,
                |t, _bold| t.len() as f64 * 7.0,
                |t, _bold| t.len() as f64 * 6.0,
            )
            .labels
            .into_iter()
            .find(|l| l.text == "12.00" && l.background.is_some())
            .expect("the h-line labels the axis")
    };
    let label = label_at(&mut chart);
    let (_, _, _, _, bg) = label.background.expect("boxed");
    assert_eq!(bg, PRIMARY, "default drawing color");
    assert_eq!(label.color, LIVE_TEXT);

    // The label is part of the line: recoloring the drawing recolors the label.
    assert!(chart.drawing_apply_options(id, r##"{"color":"#ff0000"}"##));
    let label = label_at(&mut chart);
    let (_, _, _, _, bg) = label.background.expect("boxed");
    assert_eq!(bg, Color::rgb(0xff, 0x00, 0x00));
    assert_eq!(label.color, LIVE_TEXT);

    // Omitted text color contrasts with a light line; an explicit override remains authoritative.
    assert!(chart.drawing_apply_options(id, r##"{"color":"#f0e68c"}"##));
    let label = label_at(&mut chart);
    assert_eq!(label.color, Color::rgb(0, 0, 0));
    assert!(chart.drawing_apply_options(id, r##"{"label_text_color":"#123456"}"##));
    let label = label_at(&mut chart);
    assert_eq!(label.color, Color::rgb(0x12, 0x34, 0x56));
}

#[test]
fn position_drawings_paint_information_and_entry_target_stop_axis_prices() {
    use crate::drawings::{DrawingKind, DrawingPoint};

    let mut chart = countdown_chart();
    let id = chart
        .add_drawing(
            DrawingKind::LongPosition,
            0,
            vec![
                DrawingPoint {
                    logical: 1.0,
                    price: 12.0,
                },
                DrawingPoint {
                    logical: 3.0,
                    price: 13.0,
                },
                DrawingPoint {
                    logical: 3.0,
                    price: 11.0,
                },
            ],
            None,
        )
        .unwrap();
    chart.set_selected_drawing(Some(id));

    let frame = chart.build_frame();
    let entry = Color::rgb(0x78, 0x7b, 0x86);
    let reward = Color::rgb(0x08, 0x99, 0x81);
    let risk = Color::rgb(0xf7, 0x52, 0x5f);
    assert!(
        frame.panes[0].main.iter().all(|prim| !matches!(
            prim,
            Prim::RectFrame { color, .. } if *color == reward || *color == risk
        )),
        "position target/stop zones are fill-only, not bordered rectangles"
    );
    assert!(frame.panes[0].main.iter().any(|prim| matches!(
        prim,
        Prim::Polyline { color, width, .. }
            if *color == entry && *width < 1.0
    )));
    let text = frame.panes[0]
        .main
        .iter()
        .filter_map(|prim| match prim {
            Prim::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(text.iter().any(|text| text.starts_with("Target: ")));
    assert!(text.iter().any(|text| text.starts_with("Stop: ")));
    assert!(text.iter().any(|text| text.contains(", Amount: 1250")));
    assert!(text.iter().any(|text| text.contains(", Amount: 750")));
    assert!(text.iter().any(|text| text.contains(", Qty: 250")));
    assert!(text.contains(&"Risk/reward ratio: 1"));

    let labels = chart
        .build_axis_frame(
            80.0,
            |text, _bold| text.len() as f64 * 7.0,
            |text, _bold| text.len() as f64 * 6.0,
        )
        .labels;
    let boxed = labels
        .iter()
        .filter_map(|label| {
            label
                .background
                .map(|(_, _, _, _, color)| (&label.text, color))
        })
        .collect::<Vec<_>>();
    assert!(
        boxed
            .iter()
            .any(|(text, color)| text.as_str() == "12.00" && *color == entry),
        "boxed labels: {boxed:?}"
    );
    assert!(
        boxed
            .iter()
            .any(|(text, color)| text.as_str() == "13.00" && *color == reward)
    );
    assert!(
        boxed
            .iter()
            .any(|(text, color)| text.as_str() == "11.00" && *color == risk)
    );
}

#[test]
fn position_stats_show_only_while_the_position_is_selected() {
    use crate::chart_input::PointerInput;
    use crate::drawings::{DrawingKind, DrawingPoint};

    fn stat_texts(chart: &mut ChartEngine) -> Vec<String> {
        chart.build_frame().panes[0]
            .main
            .iter()
            .filter_map(|prim| match prim {
                Prim::Text { text, .. }
                    if text.starts_with("Target: ")
                        || text.starts_with("Stop: ")
                        || text.contains("P&L: ")
                        || text.starts_with("Risk/reward ratio: ") =>
                {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect()
    }
    fn click(chart: &mut ChartEngine, x: f64, y: f64, timestamp_ms: f64) {
        let input = PointerInput {
            x,
            y,
            timestamp_ms,
            ..PointerInput::default()
        };
        chart.input_pointer_down(input, 1);
        chart.input_pointer_up(input);
    }

    let mut chart = countdown_chart();
    let id = chart
        .add_drawing(
            DrawingKind::LongPosition,
            0,
            vec![
                DrawingPoint {
                    logical: 1.0,
                    price: 12.0,
                },
                DrawingPoint {
                    logical: 3.0,
                    price: 13.0,
                },
                DrawingPoint {
                    logical: 3.0,
                    price: 11.0,
                },
            ],
            None,
        )
        .unwrap();
    assert!(
        stat_texts(&mut chart).is_empty(),
        "an idle position shows its zones without statistic labels"
    );
    assert!(
        boxed_labels(&mut chart)
            .iter()
            .any(|label| label.text == "13.00"),
        "idle positions keep their target axis tag"
    );

    let x = chart.logical_to_coordinate(2.0).unwrap();
    let y = chart.series_price_to_coordinate(0, 12.5).unwrap();
    click(&mut chart, x, y, 1_000.0);
    assert_eq!(chart.selected_drawing(), Some(id));
    let selected = stat_texts(&mut chart);
    assert!(selected.iter().any(|text| text.starts_with("Target: ")));
    assert!(selected.iter().any(|text| text.starts_with("Stop: ")));
    assert!(selected.iter().any(|text| text.starts_with("Open P&L: ")));
    assert!(
        selected
            .iter()
            .any(|text| text.starts_with("Risk/reward ratio: "))
    );
    assert_retained_frame_matches_clean_rebuild(&mut chart);

    let empty_x = chart.logical_to_coordinate(4.0).unwrap();
    let empty_y = chart.series_price_to_coordinate(0, 10.0).unwrap();
    click(&mut chart, empty_x, empty_y, 3_000.0);
    assert_eq!(chart.selected_drawing(), None);
    assert!(stat_texts(&mut chart).is_empty());
    assert_retained_frame_matches_clean_rebuild(&mut chart);

    chart.set_selected_drawing(Some(id));
    assert!(!stat_texts(&mut chart).is_empty());
}

#[test]
fn position_square_controls_have_one_rounded_fill_and_inside_border() {
    use crate::drawings::{DrawingKind, DrawingPoint};

    for dpr in [1.0, 1.5, 2.0] {
        for (background, fill) in [
            ("#ffffff", Color::rgb(255, 255, 255)),
            ("#000000", Color::rgb(0, 0, 0)),
        ] {
            let mut chart = countdown_chart();
            chart.dpr = dpr;
            chart
                .apply_options(&format!(
                    r##"{{"layout":{{"background":{{"color":"{background}"}}}}}}"##
                ))
                .unwrap();
            let id = chart
                .add_drawing(
                    DrawingKind::LongPosition,
                    0,
                    vec![
                        DrawingPoint {
                            logical: 1.17,
                            price: 12.13,
                        },
                        DrawingPoint {
                            logical: 3.17,
                            price: 13.13,
                        },
                        DrawingPoint {
                            logical: 1.17,
                            price: 11.13,
                        },
                    ],
                    None,
                )
                .unwrap();
            chart.set_selected_drawing(Some(id));
            let frame = chart.build_frame();
            let squares = frame.panes[0]
                .main
                .iter()
                .filter_map(|prim| match prim {
                    Prim::RoundRect {
                        x,
                        y,
                        w,
                        h,
                        radii,
                        fill: actual_fill,
                        border_width,
                        border_color,
                    } if *border_width > 0.0 && *border_color == super::PRIMARY => {
                        assert_eq!(*actual_fill, fill);
                        assert_eq!(*border_color, super::PRIMARY);
                        assert_eq!(*border_width, (2.0_f64 * dpr).round().max(1.0) as f32);
                        assert_eq!(*w, *h);
                        assert_eq!(x.fract(), 0.0);
                        assert_eq!(y.fract(), 0.0);
                        assert!(
                            radii
                                .iter()
                                .all(|radius| *radius > 0.0 && *radius == radii[0])
                        );
                        // Squares keep small corners; the entry control is fully round.
                        Some(radii[0] < *w / 2.0)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                squares.iter().filter(|square| **square).count(),
                3,
                "one bordered rounded shape per square position control"
            );
            assert_eq!(squares.len(), 4, "plus the round entry control");
        }
    }
}

#[test]
fn position_progress_darkens_the_run_and_keeps_labels_above_the_gray_trend() {
    use crate::drawings::{DrawingKind, DrawingPoint};

    let mut chart = countdown_chart();
    let id = chart
        .add_drawing(
            DrawingKind::LongPosition,
            0,
            vec![
                DrawingPoint {
                    logical: 1.0,
                    price: 12.0,
                },
                DrawingPoint {
                    logical: 4.0,
                    price: 14.0,
                },
                DrawingPoint {
                    logical: 1.0,
                    price: 9.0,
                },
            ],
            None,
        )
        .unwrap();
    chart.set_selected_drawing(Some(id));

    // countdown_chart's latest close is 12.5: inside the long reward zone.
    let frame = chart.build_frame();
    let reward = Color::rgb(0x08, 0x99, 0x81);
    let risk = Color::rgb(0xf7, 0x52, 0x5f);
    let progress_fill = Color::rgba(reward.r(), reward.g(), reward.b(), 96);
    let risk_progress_fill = Color::rgba(risk.r(), risk.g(), risk.b(), 96);
    let progress_rect = frame.panes[0]
        .main
        .iter()
        .find_map(|prim| match prim {
            Prim::Rect { rect, color } if *color == progress_fill => Some(*rect),
            _ => None,
        })
        .expect("reward travel overlay");
    assert!(
        progress_rect.w < chart.pane_w.round() as i32,
        "progress must darken only the elapsed horizontal travel, not the whole position width"
    );
    assert!(
        !frame.panes[0]
            .main
            .iter()
            .any(|prim| matches!(prim, Prim::Rect { color, .. } if *color == risk_progress_fill)),
        "an open/profit-side long run must not alter the stop-loss zone opacity"
    );
    let progress = frame.panes[0]
        .main
        .iter()
        .filter_map(|prim| match prim {
            Prim::Polyline {
                first_point,
                point_count,
                color,
                width,
                style: LineStyle::Solid,
                line_type: LineType::Simple,
                ..
            } if *color == POSITION_ENTRY && *width >= 1.0 => {
                Some((*first_point as usize, *point_count as usize))
            }
            _ => None,
        })
        .filter(|(_, count)| *count >= 2)
        .collect::<Vec<_>>();
    assert!(
        progress.len() > 1,
        "the dashed gray position path must be split into backend-identical solid runs"
    );
    for (first, count) in progress {
        let run = &frame.panes[0].points[first..first + count];
        assert_ne!(run[0][0], run[1][0], "progress must advance in time");
        assert_ne!(run[0][1], run[1][1], "progress must track price");
    }

    let last_progress_prim = frame.panes[0]
        .main
        .iter()
        .enumerate()
        .filter_map(|(index, prim)| match prim {
            Prim::Polyline { color, width, .. } if *color == POSITION_ENTRY && *width >= 1.0 => {
                Some(index)
            }
            _ => None,
        })
        .max()
        .expect("position progress line");
    let first_position_label = frame.panes[0]
        .main
        .iter()
        .enumerate()
        .filter_map(|(index, prim)| match prim {
            Prim::Text { text, .. }
                if text.starts_with("Target: ") || text.starts_with("Stop: ") =>
            {
                Some(index)
            }
            _ => None,
        })
        .min()
        .expect("position information labels");
    assert!(
        last_progress_prim < first_position_label,
        "position labels must paint over the dashed run line"
    );
    assert!(frame.panes[0].main.iter().any(|prim| matches!(
        prim,
        Prim::Text { text, .. } if text.starts_with("Open P&L: ")
    )));
}

#[test]
fn position_run_starts_at_first_fill_and_tracks_latest_close_while_open() {
    use crate::drawings::{DrawingKind, DrawingPoint};
    use crate::frame::drawings::PositionRunSide;

    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Candlestick;
    let times = [0.0, 60.0, 120.0, 180.0, 240.0];
    let open = [10.0, 12.0, 13.0, 14.0, 13.0];
    let high = [11.0, 13.0, 16.0, 15.0, 14.0];
    let low = [9.0, 11.0, 12.0, 10.0, 11.0];
    let close = [10.5, 12.5, 15.0, 10.5, 12.5];
    chart
        .set_series_data(0, &times, &open, &high, &low, &close)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();

    let long_id = chart
        .add_drawing(
            DrawingKind::LongPosition,
            0,
            vec![
                DrawingPoint {
                    logical: 1.0,
                    price: 12.0,
                },
                DrawingPoint {
                    logical: 3.0,
                    price: 20.0,
                },
                DrawingPoint {
                    logical: 1.0,
                    price: 5.0,
                },
            ],
            None,
        )
        .unwrap();
    let short_id = chart
        .add_drawing(
            DrawingKind::ShortPosition,
            0,
            vec![
                DrawingPoint {
                    logical: 1.0,
                    price: 14.0,
                },
                DrawingPoint {
                    logical: 3.0,
                    price: 5.0,
                },
                DrawingPoint {
                    logical: 1.0,
                    price: 20.0,
                },
            ],
            None,
        )
        .unwrap();

    let long = chart
        .drawings
        .iter()
        .find(|drawing| drawing.id == long_id)
        .unwrap();
    let long_run = chart.position_run_progress(long).unwrap();
    assert_eq!(long_run.start.logical, 1.0);
    assert_eq!(long_run.start.price, 12.0);
    assert_eq!(
        long_run.side,
        PositionRunSide::Risk,
        "an active long follows the current side of entry rather than a prior favorable wick"
    );
    assert_eq!(long_run.point.logical, 3.0);
    assert_eq!(
        long_run.point.price, 10.5,
        "the position width ends at logical 3, so progress must use that last in-box Close"
    );

    let short = chart
        .drawings
        .iter()
        .find(|drawing| drawing.id == short_id)
        .unwrap();
    let short_run = chart.position_run_progress(short).unwrap();
    assert_eq!(short_run.start.logical, 2.0);
    assert_eq!(short_run.start.price, 14.0);
    assert_eq!(short_run.side, PositionRunSide::Reward);
    assert_eq!(short_run.point.logical, 3.0);
    assert_eq!(
        short_run.point.price, 10.5,
        "an open short also uses the latest in-box Close rather than its wick"
    );
}

#[test]
fn position_run_stays_pending_until_entry_is_reached() {
    use crate::drawings::{DrawingKind, DrawingPoint};

    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Candlestick;
    chart
        .set_series_data(
            0,
            &[0.0, 60.0, 120.0, 180.0],
            &[100.0, 101.0, 102.0, 103.0],
            &[102.0, 103.0, 104.0, 105.0],
            &[98.0, 99.0, 100.0, 101.0],
            &[101.0, 102.0, 103.0, 104.0],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let id = chart
        .add_drawing(
            DrawingKind::LongPosition,
            0,
            vec![
                DrawingPoint {
                    logical: 0.0,
                    price: 110.0,
                },
                DrawingPoint {
                    logical: 3.0,
                    price: 120.0,
                },
                DrawingPoint {
                    logical: 0.0,
                    price: 105.0,
                },
            ],
            None,
        )
        .unwrap();
    let drawing = chart
        .drawings
        .iter()
        .find(|drawing| drawing.id == id)
        .unwrap();

    assert_eq!(
        chart.position_run_progress(drawing),
        None,
        "an RR tool placed above untouched price must remain pending"
    );

    let frame = chart.build_frame();
    let reward = Color::rgb(0x08, 0x99, 0x81);
    let risk = Color::rgb(0xf7, 0x52, 0x5f);
    let reward_progress = Color::rgba(reward.r(), reward.g(), reward.b(), 96);
    let risk_progress = Color::rgba(risk.r(), risk.g(), risk.b(), 96);
    assert!(!frame.panes[0].main.iter().any(|prim| {
        matches!(prim, Prim::Rect { color, .. } if *color == reward_progress || *color == risk_progress)
    }));
    assert!(!frame.panes[0].main.iter().any(|prim| {
        matches!(prim, Prim::Polyline { color, width, .. } if *color == POSITION_ENTRY && *width >= 1.0)
    }));
}

#[test]
fn position_progress_overlay_covers_only_the_traveled_price_slice() {
    use crate::drawings::{DrawingKind, DrawingPoint};

    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Candlestick;
    chart
        .set_series_data(
            0,
            &[0.0, 60.0, 120.0, 180.0],
            &[95.0, 98.0, 101.0, 102.0],
            &[97.0, 100.5, 104.0, 104.0],
            &[93.0, 96.0, 99.0, 100.0],
            &[96.0, 100.0, 103.0, 102.0],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart
        .add_drawing(
            DrawingKind::LongPosition,
            0,
            vec![
                DrawingPoint {
                    logical: 0.0,
                    price: 100.0,
                },
                DrawingPoint {
                    logical: 3.0,
                    price: 110.0,
                },
                DrawingPoint {
                    logical: 0.0,
                    price: 90.0,
                },
            ],
            None,
        )
        .unwrap();

    let frame = chart.build_frame();
    let reward = Color::rgb(0x08, 0x99, 0x81);
    let base_fill = Color::rgba(reward.r(), reward.g(), reward.b(), 70);
    let progress_fill = Color::rgba(reward.r(), reward.g(), reward.b(), 96);
    assert!(
        progress_fill.a() > base_fill.a(),
        "the traveled reward slice must be intrinsically more opaque than the untouched zone"
    );
    let base = frame.panes[0]
        .main
        .iter()
        .find_map(|prim| match prim {
            Prim::Rect { rect, color } if *color == base_fill => Some(*rect),
            _ => None,
        })
        .expect("base reward zone");
    let progress = frame.panes[0]
        .main
        .iter()
        .find_map(|prim| match prim {
            Prim::Rect { rect, color } if *color == progress_fill => Some(*rect),
            _ => None,
        })
        .expect("traveled reward slice");
    assert!(
        progress.w < base.w,
        "progress starts on the first fill candle, not the placement edge"
    );
    assert!(
        progress.h < base.h,
        "progress darkens entry-to-current price only, not the full target zone"
    );
}

#[test]
fn position_run_freezes_on_the_first_boundary_at_the_exact_exit_price() {
    use crate::drawings::{DrawingKind, DrawingPoint};
    use crate::frame::drawings::PositionRunSide;

    fn position(
        chart: &mut ChartEngine,
        kind: DrawingKind,
        entry: f64,
        target: f64,
        stop: f64,
    ) -> crate::drawings::DrawingId {
        chart
            .add_drawing(
                kind,
                0,
                vec![
                    DrawingPoint {
                        logical: 0.0,
                        price: entry,
                    },
                    DrawingPoint {
                        logical: 3.0,
                        price: target,
                    },
                    DrawingPoint {
                        logical: 0.0,
                        price: stop,
                    },
                ],
                None,
            )
            .unwrap()
    }

    // Long: stop is touched on candle 1; a later candle reaches target, but the position was
    // already terminated. The connector must stay on candle 1 at the exact stop boundary.
    let mut long_stop = ChartEngine::new(800.0, 500.0, 1.0);
    long_stop.series[0].kind = SeriesKind::Candlestick;
    long_stop
        .set_series_data(
            0,
            &[0.0, 60.0, 120.0],
            &[100.0, 99.0, 102.0],
            &[103.0, 104.0, 112.0],
            &[98.0, 94.0, 99.0],
            &[101.0, 96.0, 111.0],
        )
        .unwrap();
    let id = position(
        &mut long_stop,
        DrawingKind::LongPosition,
        100.0,
        110.0,
        95.0,
    );
    let drawing = long_stop
        .drawings
        .iter()
        .find(|drawing| drawing.id == id)
        .unwrap();
    let run = long_stop.position_run_progress(drawing).unwrap();
    assert_eq!(run.side, PositionRunSide::Risk);
    assert_eq!(run.point.logical, 1.0);
    assert_eq!(
        run.point.price, 95.0,
        "completed stop run ends at the exact stop exit level, not the candle's overshoot low"
    );

    // Long target-first is the mirror: a later stop cannot flip an already completed reward run.
    let mut long_target = ChartEngine::new(800.0, 500.0, 1.0);
    long_target.series[0].kind = SeriesKind::Candlestick;
    long_target
        .set_series_data(
            0,
            &[0.0, 60.0, 120.0],
            &[100.0, 102.0, 98.0],
            &[103.0, 111.0, 104.0],
            &[98.0, 99.0, 94.0],
            &[101.0, 110.0, 95.0],
        )
        .unwrap();
    let id = position(
        &mut long_target,
        DrawingKind::LongPosition,
        100.0,
        110.0,
        95.0,
    );
    let drawing = long_target
        .drawings
        .iter()
        .find(|drawing| drawing.id == id)
        .unwrap();
    let run = long_target.position_run_progress(drawing).unwrap();
    assert_eq!(run.side, PositionRunSide::Reward);
    assert_eq!(run.point.logical, 1.0);
    assert_eq!(
        run.point.price, 110.0,
        "completed target run ends at the exact target exit level, not the candle's overshoot high"
    );

    // Short is the mirror: stop-first freezes at the stop price, target-first at target.
    let mut short_stop = ChartEngine::new(800.0, 500.0, 1.0);
    short_stop.series[0].kind = SeriesKind::Candlestick;
    short_stop
        .set_series_data(
            0,
            &[0.0, 60.0, 120.0],
            &[100.0, 101.0, 98.0],
            &[102.0, 106.0, 101.0],
            &[97.0, 96.0, 89.0],
            &[99.0, 104.0, 90.0],
        )
        .unwrap();
    let id = position(
        &mut short_stop,
        DrawingKind::ShortPosition,
        100.0,
        90.0,
        105.0,
    );
    let drawing = short_stop
        .drawings
        .iter()
        .find(|drawing| drawing.id == id)
        .unwrap();
    let run = short_stop.position_run_progress(drawing).unwrap();
    assert_eq!(run.side, PositionRunSide::Risk);
    assert_eq!(run.point.logical, 1.0);
    assert_eq!(run.point.price, 105.0);

    let mut short_target = ChartEngine::new(800.0, 500.0, 1.0);
    short_target.series[0].kind = SeriesKind::Candlestick;
    short_target
        .set_series_data(
            0,
            &[0.0, 60.0, 120.0],
            &[100.0, 98.0, 102.0],
            &[102.0, 103.0, 106.0],
            &[97.0, 89.0, 96.0],
            &[99.0, 90.0, 104.0],
        )
        .unwrap();
    let id = position(
        &mut short_target,
        DrawingKind::ShortPosition,
        100.0,
        90.0,
        105.0,
    );
    let drawing = short_target
        .drawings
        .iter()
        .find(|drawing| drawing.id == id)
        .unwrap();
    let run = short_target.position_run_progress(drawing).unwrap();
    assert_eq!(run.side, PositionRunSide::Reward);
    assert_eq!(run.point.logical, 1.0);
    assert_eq!(run.point.price, 90.0);
}

#[test]
fn position_run_same_candle_target_and_stop_is_conservatively_stop_first() {
    use crate::drawings::{DrawingKind, DrawingPoint};
    use crate::frame::drawings::PositionRunSide;

    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Candlestick;
    chart
        .set_series_data(0, &[0.0], &[100.0], &[111.0], &[94.0], &[102.0])
        .unwrap();
    let id = chart
        .add_drawing(
            DrawingKind::LongPosition,
            0,
            vec![
                DrawingPoint {
                    logical: 0.0,
                    price: 100.0,
                },
                DrawingPoint {
                    logical: 2.0,
                    price: 110.0,
                },
                DrawingPoint {
                    logical: 0.0,
                    price: 95.0,
                },
            ],
            None,
        )
        .unwrap();
    let drawing = chart
        .drawings
        .iter()
        .find(|drawing| drawing.id == id)
        .unwrap();
    let run = chart.position_run_progress(drawing).unwrap();
    assert_eq!(run.side, PositionRunSide::Risk);
    assert_eq!(run.point.logical, 0.0);
    assert_eq!(run.point.price, 95.0);
}

#[test]
fn stop_first_progress_darkens_only_the_traveled_loss_slice() {
    use crate::drawings::{DrawingKind, DrawingPoint};

    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Candlestick;
    chart
        .set_series_data(
            0,
            &[0.0, 60.0, 120.0],
            &[100.0, 99.0, 102.0],
            &[103.0, 104.0, 112.0],
            &[98.0, 94.0, 99.0],
            &[101.0, 96.0, 111.0],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart
        .add_drawing(
            DrawingKind::LongPosition,
            0,
            vec![
                DrawingPoint {
                    logical: 0.0,
                    price: 100.0,
                },
                DrawingPoint {
                    logical: 2.0,
                    price: 110.0,
                },
                DrawingPoint {
                    logical: 0.0,
                    price: 95.0,
                },
            ],
            None,
        )
        .unwrap();

    let frame = chart.build_frame();
    let reward = Color::rgb(0x08, 0x99, 0x81);
    let risk = Color::rgb(0xf7, 0x52, 0x5f);
    let reward_progress = Color::rgba(reward.r(), reward.g(), reward.b(), 96);
    let risk_base = Color::rgba(risk.r(), risk.g(), risk.b(), 70);
    let risk_progress = Color::rgba(risk.r(), risk.g(), risk.b(), 96);
    assert!(
        risk_progress.a() > risk_base.a(),
        "the traveled stop-loss slice must be intrinsically more opaque than the untouched zone"
    );
    assert!(
        frame.panes[0]
            .main
            .iter()
            .any(|prim| matches!(prim, Prim::Rect { color, .. } if *color == risk_progress))
    );
    assert!(
        !frame.panes[0]
            .main
            .iter()
            .any(|prim| matches!(prim, Prim::Rect { color, .. } if *color == reward_progress))
    );
}

#[test]
fn moving_average_band_fill_paints_between_upper_and_lower() {
    for envelopes in [false, true] {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let times = [1.0, 2.0, 3.0, 4.0];
        let values = [10.0, 11.0, 12.0, 11.0];
        chart
            .set_series_data(0, &times, &values, &values, &values, &values)
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        if envelopes {
            chart.add_envelopes(0, 2, 10.0, true);
        } else {
            chart.add_bollinger(0, 2, 2.0);
        }

        let frame = chart.build_frame();
        let fill = frame.panes[0]
            .main
            .iter()
            .find_map(|p| match p {
                Prim::BandFill {
                    point_count, fill, ..
                } => Some((*point_count, *fill)),
                _ => None,
            })
            .expect("study paints a band fill");
        // Three valid rows (4 bars, warm-up 1) and the fill is the band color at 0.2 alpha.
        assert_eq!(fill.0, 3);
        assert_eq!(fill.1, Color::rgba(LINE.r(), LINE.g(), LINE.b(), 51));
    }
}

#[test]
fn rsi_channel_band_paints_a_translucent_strip_between_30_and_70() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times = [1.0, 2.0, 3.0, 4.0, 5.0];
    let values = [10.0, 11.0, 12.0, 11.0, 12.0];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.add_rsi(0, 2);

    let frame = chart.build_frame();
    assert_eq!(frame.panes.len(), 2);
    let strip = frame.panes[1].main.iter().any(
        |p| matches!(p, Prim::Rect { color, .. } if *color == Color::rgba(0x78, 0x7B, 0x86, 0x33)),
    );
    assert!(strip, "the rsi pane paints its 30/70 channel strip");
}

#[test]
fn line_visible_false_keeps_area_fill_but_drops_the_stroke() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Area;
    let times = [1.0, 2.0, 3.0];
    let values = [10.0, 11.0, 12.0];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.series[0].line_visible = false;

    // reference lineVisible: the fill stays, the stroke goes.
    let frame = chart.build_frame();
    assert!(
        frame.panes[0]
            .main
            .iter()
            .any(|p| matches!(p, Prim::AreaFill { .. }))
    );
    assert!(
        !frame.panes[0]
            .main
            .iter()
            .any(|p| matches!(p, Prim::Polyline { .. }))
    );

    // A line series keeps nothing but its point markers.
    chart.series[0].kind = SeriesKind::Line;
    chart.series[0].point_markers = true;
    let frame = chart.build_frame();
    assert!(
        !frame.panes[0]
            .main
            .iter()
            .any(|p| matches!(p, Prim::Polyline { .. } | Prim::AreaFill { .. }))
    );
    assert!(
        frame.panes[0]
            .main
            .iter()
            .any(|p| matches!(p, Prim::Circle { .. }))
    );
}

#[test]
fn point_markers_radius_option_overrides_the_reference_auto_default() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Line;
    let times = [1.0, 2.0, 3.0];
    let values = [10.0, 11.0, 12.0];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.series[0].point_markers = true;
    let radius = |chart: &mut ChartEngine| {
        chart.build_frame().panes[0]
            .main
            .iter()
            .find_map(|p| match p {
                Prim::Circle { radius, .. } => Some(*radius),
                _ => None,
            })
            .expect("point marker circle")
    };
    // reference auto radius (line-pane-view.ts): lineWidth / 2 + 2 = 3 at the Aeris default width 2.
    assert_eq!(radius(&mut chart), 3.0);
    chart.series[0].point_markers_radius = Some(6.0);
    assert_eq!(radius(&mut chart), 6.0);
}

#[test]
fn crosshair_marks_cover_all_line_series_with_per_series_options() {
    let circles = |chart: &mut ChartEngine| {
        chart.build_frame().panes[0]
            .main
            .iter()
            .filter_map(|p| match p {
                Prim::Circle { radius, fill, .. } => Some((*radius, *fill)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    for kind in [SeriesKind::Line, SeriesKind::Area, SeriesKind::Baseline] {
        let mut chart = two_identical_line_series();
        chart.series[0].kind = kind;
        chart.series[1].kind = kind;
        let x = chart.time_scale.index_to_coordinate(2);
        chart.crosshair = Some((x, 120.0));
        assert!(
            circles(&mut chart).is_empty(),
            "crosshair markers default off for {kind:?} series"
        );
        chart.series[0].crosshair_marker_visible = true;
        chart.series[1].crosshair_marker_visible = true;
        assert_eq!(
            circles(&mut chart).len(),
            4,
            "border and fill for both {kind:?} series"
        );
        chart.series[1].crosshair_marker_visible = false;
        assert_eq!(circles(&mut chart).len(), 2);
        chart.series[1].crosshair_marker_visible = true;
        assert_eq!(circles(&mut chart).len(), 4);
    }

    let mut chart = two_identical_line_series();
    let x = chart.time_scale.index_to_coordinate(2);
    chart.crosshair = Some((x, 120.0));
    chart.series[0].crosshair_marker_visible = true;
    let background = css_color(
        &chart.options.get().layout.background.color,
        Color::rgb(0xff, 0xff, 0xff),
    );
    let marks = circles(&mut chart);
    assert!(
        marks
            .iter()
            .any(|&(radius, color)| radius == 6.0 && color == background)
    );
    assert!(
        marks
            .iter()
            .any(|&(radius, color)| radius == 4.0 && color == LINE)
    );

    chart.series[0].crosshair_marker_radius = 7.0;
    chart.series[0].crosshair_marker_border_width = 3.0;
    chart.series[0].crosshair_marker_border_color = Some("#010203".to_string());
    chart.series[0].crosshair_marker_background_color = Some("#040506".to_string());
    let marks = circles(&mut chart);
    assert!(
        marks
            .iter()
            .any(|&(radius, color)| radius == 10.0 && color == Color::rgb(1, 2, 3))
    );
    assert!(
        marks
            .iter()
            .any(|&(radius, color)| radius == 7.0 && color == Color::rgb(4, 5, 6))
    );
}

#[test]
fn baseline_quadrant_options_flow_into_fills_and_strokes() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Baseline;
    let times = [1.0, 2.0];
    let values = [10.0, 20.0]; // auto baseline = 15, so the segment crosses it
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let frame = chart.build_frame();

    // Each half takes the canonical area gradient of its own line: strong at the extreme, faint
    // at the baseline (the bottom half runs top-to-bottom, so it is faint -> strong).
    let (top_strong, top_faint) = area_fill_gradient(BASELINE_TOP_LINE);
    let (bottom_strong, bottom_faint) = area_fill_gradient(BASELINE_BOTTOM_LINE);
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::AreaFill { gradient, .. }
            if gradient.top == top_strong && gradient.bottom == top_faint
    )));
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::AreaFill { gradient, .. }
            if gradient.top == bottom_faint && gradient.bottom == bottom_strong
    )));
    // One continuous solid stroke per quadrant in the reference line colors.
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::Polyline { color, .. } if *color == BASELINE_TOP_LINE
    )));
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::Polyline { color, .. } if *color == BASELINE_BOTTOM_LINE
    )));

    // Per-quadrant options: custom colors/widths, and a dashed quadrant style splits runs.
    assert!(chart.series_apply_options_json(
        0,
        r##"{"top_line_color": "#010203", "top_fill_color1": "#0a0b0c",
             "top_fill_color2": "#0d0e0f", "top_line_width": 5, "bottom_line_style": 2}"##
    ));
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::Polyline { color, width, .. } if *color == Color::rgb(0x01, 0x02, 0x03) && *width == 5.0
    )));
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::AreaFill { gradient, .. }
            if gradient.top == Color::rgb(0x0a, 0x0b, 0x0c)
                && gradient.bottom == Color::rgb(0x0d, 0x0e, 0x0f)
    )));
    let bottom_runs = frame.panes[0]
        .main
        .iter()
        .filter(|p| {
            matches!(
                p,
                Prim::Polyline { color, .. } if *color == BASELINE_BOTTOM_LINE
            )
        })
        .count();
    assert!(bottom_runs > 1, "dashed quadrant must split into runs");

    // lineVisible: false drops both quadrant strokes but keeps the fills.
    chart.series[0].line_visible = false;
    let frame = chart.build_frame();
    assert!(
        !frame.panes[0]
            .main
            .iter()
            .any(|p| matches!(p, Prim::Polyline { .. }))
    );
    assert!(
        frame.panes[0]
            .main
            .iter()
            .any(|p| matches!(p, Prim::AreaFill { .. }))
    );
}

#[test]
fn baseline_fill_uses_one_continuous_area_per_quadrant_run() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Baseline;
    let times = [1.0, 2.0, 3.0, 4.0];
    let values = [10.0, 13.0, 11.0, 14.0];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.series[0].baseline = Some(0.0);
    chart.time_scale.set_width(800.0);
    chart.fit_content();

    let fills = chart.build_frame().panes[0]
        .main
        .iter()
        .filter_map(|primitive| match primitive {
            Prim::AreaFill { point_count, .. } => Some(*point_count),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        fills,
        [4],
        "a continuous quadrant must not be split into gradient pockets"
    );
}

#[test]
fn histogram_base_offsets_the_column_level() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let histogram = chart.add_series(SeriesKind::Histogram);
    let times = [1.0, 2.0, 3.0];
    let values = [1.0, 2.0, 3.0];
    chart
        .set_series_data(histogram, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.series_entry_mut(histogram).unwrap().base = 4.0; // above every value: columns hang down from the base

    let frame = chart.build_frame();
    chart.autoscale_visible();
    let scale = pane_scale(&chart.panes[0], PriceScaleTarget::Right);
    let base_value = chart.series_base_value(histogram, 0).unwrap();
    let expected_top = (scale.price_to_coordinate(4.0, base_value) * 1.0).round() as i32;
    let rects: Vec<_> = frame.panes[0]
        .main
        .iter()
        .filter_map(|p| match p {
            Prim::Rect { rect, .. } => Some(*rect),
            _ => None,
        })
        .collect();
    assert_eq!(rects.len(), 3);
    assert!(
        rects.iter().all(|r| r.y == expected_top),
        "columns below the base start at the base level: {rects:?} vs {expected_top}"
    );
}

#[test]
fn invert_filled_area_flips_the_area_base_to_the_pane_top() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Area;
    let times = [1.0, 2.0, 3.0];
    let values = [10.0, 11.0, 12.0];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let base_y = |chart: &mut ChartEngine| {
        chart.build_frame().panes[0]
            .main
            .iter()
            .find_map(|p| match p {
                Prim::AreaFill { base_y, .. } => Some(*base_y),
                _ => None,
            })
            .expect("area fill")
    };
    // Default: fill from the line down to the pane bottom (500 at dpr 1).
    assert_eq!(base_y(&mut chart), 500.0);
    // reference invertFilledArea: fill from the pane top down to the line.
    chart.series[0].invert_filled_area = true;
    assert_eq!(base_y(&mut chart), 0.0);
}

#[test]
fn bar_open_visible_and_thin_bars_reach_the_builder() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Bar;
    let times: Vec<f64> = (0..5).map(|i| i as f64).collect();
    let open: Vec<f64> = (0..5).map(|i| 100.0 + i as f64).collect();
    let high: Vec<f64> = open.iter().map(|v| v + 2.0).collect();
    let low: Vec<f64> = open.iter().map(|v| v - 2.0).collect();
    let close: Vec<f64> = open.iter().map(|v| v + 0.5).collect();
    chart
        .set_series_data(0, &times, &open, &high, &low, &close)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.set_bar_spacing(10.0);
    let rects = |chart: &mut ChartEngine| {
        chart.build_frame().panes[0]
            .main
            .iter()
            .filter_map(|p| match p {
                Prim::Rect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    // reference barStyleDefaults (openVisible true, thinBars true): body + open/close ticks per bar,
    // body capped to the 1px crisp width.
    let rs = rects(&mut chart);
    assert_eq!(rs.len(), 3 * 5);
    assert!(!rs.iter().any(|r| r.w == 3), "thin bodies stay 1px wide");

    // thinBars: false follows the zoom, but a stroke that rests at 1px on the default 6px spacing
    // grows only to 2px, not the full optimal width (floor(10 * 0.3) = 3).
    chart.series[0].thin_bars = false;
    let rs = rects(&mut chart);
    assert!(rs.iter().any(|r| r.w == 2), "thick bodies reach the frame");
    assert!(
        !rs.iter().any(|r| r.w == 3),
        "zoom never thickens a bar past one extra pixel"
    );
    chart.set_bar_spacing(60.0);
    let rs = rects(&mut chart);
    assert!(
        rs.iter().all(|r| r.w <= 2 || r.h <= 2),
        "deep zoom keeps the 2px stroke"
    );

    // openVisible: false drops the open tick (body + close only).
    chart.series[0].open_visible = false;
    let rs = rects(&mut chart);
    assert_eq!(rs.len(), 2 * 5);

    // High-low bars suppress both side ticks and retain only the vertical range body.
    chart.series[0].close_visible = false;
    let rs = rects(&mut chart);
    assert_eq!(rs.len(), 5);
}

// --- per-data-point colors (reference data-item colors, series-bar-colorer.ts) ---

const POINT_RED: u32 = 0xFF0000FF;
const POINT_GREEN: u32 = 0x00FF00FF;
const POINT_BLUE: u32 = 0x0000FFFF;

fn ohlc_chart(kind: SeriesKind, bars: usize) -> ChartEngine {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = kind;
    let times: Vec<f64> = (0..bars).map(|i| i as f64).collect();
    let open: Vec<f64> = (0..bars).map(|i| 100.0 + i as f64).collect();
    let high: Vec<f64> = open.iter().map(|v| v + 2.0).collect();
    let low: Vec<f64> = open.iter().map(|v| v - 2.0).collect();
    let close: Vec<f64> = open.iter().map(|v| v + 0.5).collect();
    chart
        .set_series_data(0, &times, &open, &high, &low, &close)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart
}

#[test]
fn canonical_style_reaches_the_backend_neutral_frame() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times = [1.0, 2.0];
    let open = [10.0, 12.0];
    let high = [12.0, 13.0];
    let low = [9.0, 10.0];
    let close = [11.0, 11.0];
    chart
        .set_series_data(0, &times, &open, &high, &low, &close)
        .unwrap();
    let volume = chart.add_series(SeriesKind::Histogram);
    chart
        .set_series_data(
            volume,
            &times,
            &[100.0, 120.0],
            &[100.0, 120.0],
            &[100.0, 120.0],
            &[100.0, 120.0],
        )
        .unwrap();
    chart.series_entry_mut(volume).unwrap().histogram_updown = true;
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.crosshair = Some((chart.time_scale.index_to_coordinate(0), 200.0));

    let frame = chart.build_frame();
    let prims = &frame.panes[0].main;
    let theme_up = Color::parse_css(aeris_charts_core::style::DEFAULT_MARKET_UP_CSS).unwrap();
    let theme_down = Color::parse_css(aeris_charts_core::style::DEFAULT_MARKET_DOWN_CSS).unwrap();
    assert!(prims.iter().any(|prim| matches!(
        prim,
        Prim::Rect { color, .. } | Prim::RectFrame { color, .. } if *color == theme_up
    )));
    assert!(prims.iter().any(|prim| matches!(
        prim,
        Prim::Rect { color, .. } | Prim::RectFrame { color, .. } if *color == theme_down
    )));
    assert!(
        prims
            .iter()
            .any(|prim| matches!(prim, Prim::Rect { color, .. } if *color == VOLUME_UP))
    );
    assert!(
        prims
            .iter()
            .any(|prim| matches!(prim, Prim::Rect { color, .. } if *color == VOLUME_DOWN))
    );
    assert!(prims.iter().any(|prim| matches!(
        prim,
        Prim::HLine { color, .. } | Prim::VLine { color, .. } if *color == CROSSHAIR_COLOR
    )));

    let axis_frame = chart.build_axis_frame(
        40.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    let mut axis_prims = Vec::new();
    chart.build_axis_primitives_into(&axis_frame, &mut axis_prims);
    let border = Color::rgb(
        aeris_charts_core::style::DEFAULT_BORDER_RGB.0,
        aeris_charts_core::style::DEFAULT_BORDER_RGB.1,
        aeris_charts_core::style::DEFAULT_BORDER_RGB.2,
    );
    let axis_text = Color::rgb(
        aeris_charts_core::style::DEFAULT_AXIS_TEXT_RGB.0,
        aeris_charts_core::style::DEFAULT_AXIS_TEXT_RGB.1,
        aeris_charts_core::style::DEFAULT_AXIS_TEXT_RGB.2,
    );
    assert!(
        axis_prims
            .iter()
            .any(|prim| matches!(prim, Prim::Rect { color, .. } if *color == border))
    );
    assert!(
        axis_prims
            .iter()
            .any(|prim| matches!(prim, Prim::Text { color, .. } if *color == axis_text))
    );
}

#[test]
fn malformed_grid_and_axis_css_fall_back_to_canonical_style() {
    let mut chart = ohlc_chart(SeriesKind::Candlestick, 4);
    chart
        .apply_options(
            r#"{"layout":{"textColor":"not-a-color"},"grid":{"vertLines":{"color":"bad","visible":true},"horzLines":{"color":"bad","visible":true}}}"#,
        )
        .unwrap();

    let border = Color::rgb(
        aeris_charts_core::style::DEFAULT_BORDER_RGB.0,
        aeris_charts_core::style::DEFAULT_BORDER_RGB.1,
        aeris_charts_core::style::DEFAULT_BORDER_RGB.2,
    );
    let frame = chart.build_frame();
    assert!(frame.panes[0]
        .under
        .iter()
        .any(|prim| matches!(prim, Prim::HLine { color, .. } | Prim::VLine { color, .. } if *color == border)));

    let axis_text = Color::rgb(
        aeris_charts_core::style::DEFAULT_AXIS_TEXT_RGB.0,
        aeris_charts_core::style::DEFAULT_AXIS_TEXT_RGB.1,
        aeris_charts_core::style::DEFAULT_AXIS_TEXT_RGB.2,
    );
    let axis_frame = chart.build_axis_frame(
        40.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    let mut axis_prims = Vec::new();
    chart.build_axis_primitives_into(&axis_frame, &mut axis_prims);
    assert!(
        axis_prims
            .iter()
            .any(|prim| matches!(prim, Prim::Text { color, .. } if *color == axis_text))
    );
}

#[test]
fn candlestick_per_point_colors_override_each_channel() {
    let mut chart = ohlc_chart(SeriesKind::Candlestick, 4);
    // Bar 1 (rising): custom body/wick/border. Bar 2 keeps the series resolution.
    assert!(chart.set_series_point_colors(
        0,
        Some(vec![0, POINT_RED, 0, 0]),
        Some(vec![0, POINT_GREEN, 0, 0]),
        Some(vec![0, POINT_BLUE, 0, 0]),
    ));
    let frame = chart.build_frame();
    let has = |color: u32| {
        frame.panes[0].main.iter().any(|p| match p {
            Prim::Rect { color: c, .. } => *c == Color(color),
            Prim::RectFrame { color: c, .. } => *c == Color(color),
            _ => false,
        })
    };
    assert!(has(POINT_RED), "custom body color drawn");
    assert!(has(POINT_GREEN), "custom wick color drawn");
    assert!(has(POINT_BLUE), "custom border color drawn");
    // The uncolored bars keep the active theme's up-color resolution.
    let theme_up = Color::parse_css(aeris_charts_core::style::DEFAULT_MARKET_UP_CSS).unwrap();
    assert!(has(theme_up.0), "series up color still drawn");
}

#[test]
fn bar_per_point_color_overrides_updown() {
    let mut chart = ohlc_chart(SeriesKind::Bar, 4);
    assert!(chart.set_series_point_colors(0, Some(vec![0, POINT_RED, 0, 0]), None, None));
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::Rect { color, .. } if *color == Color(POINT_RED)
    )));
}

#[test]
fn histogram_per_bar_color_overrides_the_updown_tint() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Line;
    let times: Vec<f64> = (0..4).map(|i| i as f64).collect();
    let values: Vec<f64> = (0..4).map(|i| 100.0 + i as f64).collect();
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    let volume = chart.add_series(SeriesKind::Histogram);
    chart
        .set_series_data(volume, &times, &values, &values, &values, &values)
        .unwrap();
    chart.series_entry_mut(volume).unwrap().histogram_updown = true;
    chart.time_scale.set_width(800.0);
    chart.fit_content();

    // Default: the up/down tint colors every column.
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::Rect { color, .. } if *color == VOLUME_UP
    )));

    // A per-bar color wins over the tint for that bar only.
    assert!(chart.set_series_point_colors(volume, Some(vec![0, POINT_RED, 0, 0]), None, None));
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::Rect { color, .. } if *color == Color(POINT_RED)
    )));
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::Rect { color, .. } if *color == VOLUME_UP
    )));
}

#[test]
fn updown_volume_retints_when_only_the_primary_series_changes() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times: Vec<f64> = (0..4).map(|i| f64::from(i) * 60.0).collect();
    let open = [100.0, 100.0, 100.0, 100.0];
    let close = [101.0, 101.0, 101.0, 101.0];
    let high = [102.0; 4];
    let low = [99.0; 4];
    chart
        .set_series_data(0, &times, &open, &high, &low, &close)
        .unwrap();
    let volume = chart.add_series(SeriesKind::Histogram);
    let volumes = [10.0, 20.0, 30.0, 40.0];
    chart
        .set_series_data(volume, &times, &volumes, &volumes, &volumes, &volumes)
        .unwrap();
    chart.series_entry_mut(volume).unwrap().histogram_updown = true;
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let count = |chart: &mut ChartEngine| {
        let frame = chart.build_frame();
        let up = frame.panes[0]
            .main
            .iter()
            .filter(|p| matches!(p, Prim::Rect { color, .. } if *color == VOLUME_UP))
            .count();
        let down = frame.panes[0]
            .main
            .iter()
            .filter(|p| matches!(p, Prim::Rect { color, .. } if *color == VOLUME_DOWN))
            .count();
        (up, down)
    };
    assert_eq!(count(&mut chart), (4, 0));
    // Price-only replace: the volume series is untouched, yet every bar now closes down.
    chart
        .set_series_data(0, &times, &close, &high, &low, &open)
        .unwrap();
    assert_eq!(count(&mut chart), (0, 4));
    // A live tick that turns the last bar up again retints only that column.
    chart.update_series_bars(0, [(180.0, [100.0, 102.0, 99.0, 101.0])]);
    assert_eq!(count(&mut chart), (1, 3));
}

#[test]
fn a_volume_indicator_draws_its_columns_in_its_own_up_and_down_colors() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times = [0.0, 60.0, 120.0, 180.0];
    let open = [100.0; 4];
    let close = [101.0, 99.0, 102.0, 98.0];
    chart
        .set_series_data(0, &times, &open, &[103.0; 4], &[97.0; 4], &close)
        .unwrap();
    let weights = chart.add_series(SeriesKind::Histogram);
    let values = [10.0, 20.0, 30.0, 40.0];
    chart
        .set_series_data(weights, &times, &values, &values, &values, &values)
        .unwrap();
    chart.set_series_visible(weights, false);
    let outputs = chart.add_indicator_kind(
        0,
        crate::IndicatorKind::Volume {
            period: 2,
            exponential: false,
        },
        Some(weights),
    );
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let count = |chart: &mut ChartEngine, up: Color, down: Color| {
        let frame = chart.build_frame();
        let pane = chart.series_price_scale(outputs[0]).unwrap().0;
        let prims = &frame.panes[pane].main;
        (
            prims
                .iter()
                .filter(|prim| matches!(prim, Prim::Rect { color, .. } if *color == up))
                .count(),
            prims
                .iter()
                .filter(|prim| matches!(prim, Prim::Rect { color, .. } if *color == down))
                .count(),
        )
    };
    assert_eq!(count(&mut chart, VOLUME_UP, VOLUME_DOWN), (2, 2));

    let mut style = chart.indicator_info(outputs[0]).unwrap().style;
    style.up_color = Some("#00ff00".into());
    style.down_color = Some("#ff00ff".into());
    assert!(chart.set_indicator_output_style(outputs[0], style));
    assert_eq!(
        count(
            &mut chart,
            Color::parse_css("#00ff00").unwrap(),
            Color::parse_css("#ff00ff").unwrap()
        ),
        (2, 2)
    );
}

#[test]
fn volume_indicator_last_value_chips_follow_the_restyled_column_and_average_colors() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times = [0.0, 60.0, 120.0, 180.0];
    let open = [100.0; 4];
    let mut close = [101.0, 99.0, 102.0, 98.0];
    chart
        .set_series_data(0, &times, &open, &[103.0; 4], &[97.0; 4], &close)
        .unwrap();
    let weights = chart.add_series(SeriesKind::Histogram);
    let values = [10.0, 20.0, 30.0, 40.0];
    chart
        .set_series_data(weights, &times, &values, &values, &values, &values)
        .unwrap();
    chart.set_series_visible(weights, false);
    let outputs = chart.add_indicator_kind(
        0,
        crate::IndicatorKind::Volume {
            period: 2,
            exponential: false,
        },
        Some(weights),
    );
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let chip_colors = |chart: &mut ChartEngine| -> Vec<Color> {
        chart
            .build_axis_frame(
                80.0,
                |t, _bold| t.len() as f64 * 7.0,
                |t, _bold| t.len() as f64 * 6.0,
            )
            .labels
            .iter()
            .filter_map(|label| label.background.map(|(.., color)| color))
            .collect()
    };
    assert!(
        chip_colors(&mut chart).contains(&VOLUME_DOWN.solid()),
        "the final bar falls, so its column chip wears the default down tint"
    );

    let up = Color::parse_css("#1e90ff").unwrap();
    let down = Color::parse_css("#ff8c00").unwrap();
    let average = Color::parse_css("#00ff00").unwrap();
    let mut style = chart.indicator_info(outputs[0]).unwrap().style;
    style.up_color = Some("#1e90ff".into());
    style.down_color = Some("#ff8c00".into());
    assert!(chart.set_indicator_output_style(outputs[0], style));
    let mut style = chart.indicator_info(outputs[1]).unwrap().style;
    style.line_color = Some("#00ff00".into());
    assert!(chart.set_indicator_output_style(outputs[1], style));
    let chips = chip_colors(&mut chart);
    assert!(
        chips.contains(&down),
        "column chip follows the restyled down color: {chips:?}"
    );
    assert!(
        chips.contains(&average),
        "average chip follows its restyled line: {chips:?}"
    );
    assert!(!chips.contains(&HISTOGRAM));

    close[3] = 104.0;
    chart
        .set_series_data(0, &times, &open, &[105.0; 4], &[97.0; 4], &close)
        .unwrap();
    let chips = chip_colors(&mut chart);
    assert!(
        chips.contains(&up),
        "a rising final bar flips the chip to the up color: {chips:?}"
    );
    assert!(!chips.contains(&down));
}

#[test]
fn style_reset_preserves_mixed_updown_volume_colors() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times = [0.0, 60.0, 120.0, 180.0];
    let open = [100.0; 4];
    let close = [101.0, 99.0, 102.0, 98.0];
    chart
        .set_series_data(0, &times, &open, &[103.0; 4], &[97.0; 4], &close)
        .unwrap();
    let volume = chart.add_series(SeriesKind::Histogram);
    let values = [10.0, 20.0, 30.0, 40.0];
    chart
        .set_series_data(volume, &times, &values, &values, &values, &values)
        .unwrap();
    chart.series_entry_mut(volume).unwrap().histogram_updown = true;
    chart.set_series_price_scale(volume, crate::PriceScaleTarget::Overlay);
    chart.set_price_scale_margins_for(0, crate::PriceScaleTarget::Overlay, 0.8, 0.0);
    chart.time_scale.set_width(800.0);
    chart.fit_content();

    let colors = |chart: &mut ChartEngine| {
        let frame = chart.build_frame();
        let prims = &frame.panes[0].main;
        (
            prims
                .iter()
                .filter(|prim| matches!(prim, Prim::Rect { color, .. } if *color == VOLUME_UP))
                .count(),
            prims
                .iter()
                .filter(|prim| matches!(prim, Prim::Rect { color, .. } if *color == VOLUME_DOWN))
                .count(),
        )
    };
    assert_eq!(colors(&mut chart), (2, 2));
    chart.reset_style_to_defaults();
    assert_eq!(
        colors(&mut chart),
        (2, 2),
        "appearance reset must not change the meaning of volume bars"
    );
    assert!(chart.series_entry(volume).unwrap().histogram_updown);
}

#[test]
fn updown_volume_takes_its_direction_from_a_footprint_over_a_whitespace_primary() {
    let mut chart = ChartEngine::new(1200.0, 600.0, 1.0);
    let times: Vec<f64> = (0..4).map(|minute| f64::from(minute) * 60.0).collect();
    let nan = vec![f64::NAN; times.len()];
    chart
        .set_series_data(0, &times, &nan, &nan, &nan, &nan)
        .unwrap();
    let volume = chart.add_series(SeriesKind::Histogram);
    let volumes = [10.0, 20.0, 30.0, 40.0];
    chart
        .set_series_data(volume, &times, &volumes, &volumes, &volumes, &volumes)
        .unwrap();
    chart.series_entry_mut(volume).unwrap().histogram_updown = true;
    let presentation = chart
        .add_order_flow_presentation(
            "ES",
            0,
            crate::OrderFlowPresentationOptions {
                aggregation: crate::FootprintAggregationOptions {
                    tick_size: 1.0,
                    ticks_per_row: 1,
                    ..crate::FootprintAggregationOptions::default()
                },
                visual: crate::FootprintVisualOptions::default(),
                show_footprint: true,
                show_cumulative_delta: false,
                show_delta_histogram: false,
                cumulative_delta: crate::TradeStudyOptions::default(),
                delta_histogram: crate::TradeStudyOptions::default(),
                big_trades: None,
            },
        )
        .unwrap();
    let trade = |timestamp_micros: i64, price: f64| crate::FootprintTrade {
        timestamp_micros,
        price,
        volume: 1.0,
        aggressor: crate::AggressorSide::Buy,
        bid: None,
        ask: None,
        sequence: None,
        trade_id: None,
        conditions: 0,
        session_id: Some(1),
    };
    // The tape covers only the last two minutes: the third closes up, the fourth down.
    let tape = vec![
        trade(120_000_000, 100.0),
        trade(130_000_000, 102.0),
        trade(180_000_000, 105.0),
        trade(190_000_000, 101.0),
    ];
    chart
        .update_order_flow_presentation(presentation, tape, false)
        .unwrap();
    chart.time_scale.set_width(1200.0);
    chart.fit_content();

    let frame = chart.build_frame();
    let segment = chart
        .frame_series_segments(0)
        .iter()
        .find(|segment| segment.series_id == Some(volume))
        .unwrap();
    let columns = frame.panes[0].main[segment.start..segment.end]
        .iter()
        .filter_map(|primitive| match primitive {
            Prim::Rect { color, .. } => Some(*color),
            _ => None,
        })
        .collect::<Vec<_>>();
    // Uncovered slots have no presented bar and keep the series' solid color.
    assert_eq!(columns, [HISTOGRAM, HISTOGRAM, VOLUME_UP, VOLUME_DOWN]);
    chart.set_series_visible(presentation.footprint_series().unwrap(), false);
    let frame = chart.build_frame();
    let segment = chart
        .frame_series_segments(0)
        .iter()
        .find(|segment| segment.series_id == Some(volume))
        .unwrap();
    let colors = frame.panes[0].main[segment.start..segment.end]
        .iter()
        .filter_map(|primitive| match primitive {
            Prim::Rect { color, .. } => Some(*color),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        colors, [HISTOGRAM; 4],
        "anchors cannot tint uncovered volume"
    );
}

#[test]
fn line_per_point_colors_split_the_stroke_and_color_markers() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Line;
    chart.series[0].point_markers = true;
    let times: Vec<f64> = (0..4).map(|i| i as f64).collect();
    let values: Vec<f64> = (0..4).map(|i| 100.0 + i as f64).collect();
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    // Points 0,1 default (blue), points 2,3 red. reference walkLine: the segment leaving a point
    // takes the point's color, so the runs are [0..2] blue and [2..3] red, sharing point 2.
    assert!(chart.set_series_point_colors(0, Some(vec![0, 0, POINT_RED, POINT_RED]), None, None));
    let frame = chart.build_frame();
    let runs: Vec<(u32, u32, Color)> = frame.panes[0]
        .main
        .iter()
        .filter_map(|p| match p {
            Prim::Polyline {
                first_point,
                point_count,
                color,
                ..
            } => Some((*first_point, *point_count, *color)),
            _ => None,
        })
        .collect();
    assert_eq!(runs.len(), 2, "one run per color span: {runs:?}");
    assert_eq!(runs[0].2, LINE);
    assert_eq!(runs[0].1, 3, "blue run covers points 0..=2");
    assert_eq!(runs[1].2, Color(POINT_RED));
    assert_eq!(runs[1].1, 2, "red run covers points 2..=3");
    // Adjacent runs share the boundary point, keeping the path continuous.
    let pool = &frame.panes[0].points;
    let end_of_blue = pool[(runs[0].0 + runs[0].1 - 1) as usize];
    let start_of_red = pool[runs[1].0 as usize];
    assert_eq!(end_of_blue, start_of_red);

    // Point markers take their own point's color (reference draw-series-point-markers.ts).
    let marker_fills: Vec<Color> = frame.panes[0]
        .main
        .iter()
        .filter_map(|p| match p {
            Prim::Circle { fill, radius, .. } if *radius > 0.0 => Some(*fill),
            _ => None,
        })
        .collect();
    assert_eq!(marker_fills.len(), 4);
    assert_eq!(marker_fills[0], LINE);
    assert_eq!(marker_fills[1], LINE);
    assert_eq!(marker_fills[2], Color(POINT_RED));
    assert_eq!(marker_fills[3], Color(POINT_RED));
}

#[test]
fn brush_defaults_use_the_same_fill_strength_and_brand_hues() {
    use aeris_charts_core::style::AREA_FILL_STRONG_ALPHA;
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Area;
    let defaults = chart.area_brush_defaults(0).expect("area series");
    assert_eq!(
        (defaults.positive.top_color, defaults.positive.bottom_color),
        area_fill_gradient(UP)
    );
    assert_eq!(
        (defaults.negative.top_color, defaults.negative.bottom_color),
        area_fill_gradient(DOWN)
    );
    assert_eq!(defaults.positive.top_color.a(), AREA_FILL_STRONG_ALPHA);
    assert_eq!(defaults.positive.line_width, LINE_WIDTH);
    // Outside the selection: the series' own stroke, faded, with its fill derived by the same rule.
    assert_eq!(defaults.outside.line_color.solid(), UP.solid());
    assert!(defaults.outside.line_color.a() < UP.a());
    assert_eq!(
        (defaults.outside.top_color, defaults.outside.bottom_color),
        area_fill_gradient(defaults.outside.line_color)
    );
    // The reported color is the one the area actually strokes.
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(options["color"], UP.to_css());
    // Brushes exist only on Area series.
    chart.series[0].kind = SeriesKind::Line;
    assert!(chart.area_brush_defaults(0).is_none());
}

#[test]
fn area_like_fills_share_one_strength_and_follow_their_own_line_color() {
    use aeris_charts_core::style::{AREA_FILL_FAINT_ALPHA, AREA_FILL_STRONG_ALPHA};
    assert_eq!(AREA_LINE, UP);
    let fill_of = |kind: SeriesKind, line_color: Option<&str>| {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart.series[0].kind = kind;
        chart.series[0].line_color = line_color.map(str::to_string);
        chart.series[0].top_line_color = line_color.map(str::to_string);
        let values = [10.0, 20.0, 12.0];
        chart
            .set_series_data(0, &[1.0, 2.0, 3.0], &values, &values, &values, &values)
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        chart.build_frame().panes[0]
            .main
            .iter()
            .find_map(|p| match p {
                Prim::AreaFill { gradient, .. } => Some(gradient.top),
                _ => None,
            })
            .expect("area fill")
    };
    // Area and the baseline's upper half use the same strength of the same default hue.
    let area = fill_of(SeriesKind::Area, None);
    let baseline = fill_of(SeriesKind::Baseline, None);
    assert_eq!(
        area,
        Color::rgba(UP.r(), UP.g(), UP.b(), AREA_FILL_STRONG_ALPHA)
    );
    assert_eq!(baseline, area);
    // A recolored line recolors its fill at the same strength.
    let red = fill_of(SeriesKind::Area, Some("#ff0000"));
    assert_eq!(red, Color::rgba(0xff, 0, 0, AREA_FILL_STRONG_ALPHA));
    assert_eq!(fill_of(SeriesKind::Baseline, Some("#ff0000")), red);
    assert_eq!(area_fill_gradient(UP).1.a(), AREA_FILL_FAINT_ALPHA);
}

#[test]
fn area_per_point_colors_split_only_the_stroke() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Area;
    let times: Vec<f64> = (0..3).map(|i| i as f64).collect();
    let values: Vec<f64> = (0..3).map(|i| 100.0 + i as f64).collect();
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    assert!(chart.set_series_point_colors(0, Some(vec![0, POINT_RED, 0]), None, None));
    let frame = chart.build_frame();
    // The fill keeps the series-level gradient (documented deviation; the reference's `lineColor` data
    // field affects only the stroke).
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::AreaFill { gradient, .. }
            if (gradient.top, gradient.bottom) == area_fill_gradient(AREA_LINE)
    )));
    // The stroke splits: segment 0->1 keeps the series color, 1->2 takes point 1's color.
    let stroke_colors: Vec<Color> = frame.panes[0]
        .main
        .iter()
        .filter_map(|p| match p {
            Prim::Polyline { color, .. } => Some(*color),
            _ => None,
        })
        .collect();
    assert_eq!(stroke_colors, vec![AREA_LINE, Color(POINT_RED)]);
}

#[test]
fn area_brush_is_transient_presentation_on_the_builtin_area_series() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.convert_series_kind(0, SeriesKind::Area);
    let times: Vec<f64> = (0..5).map(|i| i as f64).collect();
    let values: Vec<f64> = (0..5).map(|i| 100.0 + i as f64).collect();
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();

    let faded = crate::BrushStyle {
        line_color: Color::rgb(80, 80, 80),
        top_color: Color::rgba(80, 80, 80, 40),
        bottom_color: Color::rgba(80, 80, 80, 0),
        line_width: 2.0,
    };
    let selected = crate::BrushStyle {
        line_color: Color::rgb(4, 153, 129),
        top_color: Color::rgba(4, 153, 129, 102),
        bottom_color: Color::rgba(4, 153, 129, 0),
        line_width: 3.0,
    };
    assert!(chart.set_area_brush_state(
        0,
        faded,
        vec![crate::BrushRange {
            from: 1.0,
            to: 3.0,
            style: selected,
        }],
    ));

    let entry = chart.series_entry(0).unwrap();
    assert_eq!(entry.kind, SeriesKind::Area);
    assert!(
        entry.feature.is_none(),
        "brushing must not create feature-series state"
    );
    assert_eq!(
        chart.data.plot(0).size(),
        5,
        "canonical Area rows stay authoritative"
    );

    let frame = chart.build_frame();
    let fill_point_counts = frame.panes[0]
        .main
        .iter()
        .filter_map(|primitive| match primitive {
            Prim::AreaFill { point_count, .. } => Some(*point_count),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        fill_point_counts,
        vec![3, 3],
        "equal-style intervals stay in continuous meshes instead of double-blending every edge",
    );
    let stroke_colors: Vec<Color> = frame.panes[0]
        .main
        .iter()
        .filter_map(|primitive| match primitive {
            Prim::Polyline { color, .. } => Some(*color),
            _ => None,
        })
        .collect();
    assert_eq!(stroke_colors, vec![selected.line_color, faded.line_color]);

    assert!(chart.set_area_brush_state(0, selected, Vec::new()));
    let frame = chart.build_frame();
    assert!(
        frame.panes[0]
            .main
            .iter()
            .any(|primitive| matches!(primitive, Prim::AreaFill { point_count: 5, .. }))
    );
    assert_eq!(
        frame.panes[0]
            .main
            .iter()
            .filter(|primitive| matches!(primitive, Prim::AreaFill { .. }))
            .count(),
        1,
        "a uniform brush style must preserve one seam-free Area fill",
    );

    assert!(chart.clear_area_brush_state(0));
    let frame = chart.build_frame();
    assert_eq!(
        frame.panes[0]
            .main
            .iter()
            .filter(|primitive| matches!(primitive, Prim::AreaFill { .. }))
            .count(),
        1,
        "clearing the interaction restores the untouched ordinary Area render path",
    );
}

#[test]
fn area_brush_changes_rebuild_only_the_brushed_series() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.convert_series_kind(0, SeriesKind::Area);
    let times: Vec<f64> = (0..40).map(f64::from).collect();
    let values: Vec<f64> = (0..40).map(|i| 100.0 + f64::from(i % 7)).collect();
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.build_frame();
    chart.build_frame();
    assert_eq!(
        chart.frame_build_stats(),
        FrameBuildStats::default(),
        "settled fixture"
    );
    let defaults = chart.area_brush_defaults(0).unwrap();
    let series_only = FrameBuildStats {
        series_rebuilds: 1,
        ..FrameBuildStats::default()
    };
    for to in [12.0, 20.0, 28.0] {
        assert!(chart.set_area_brush_state(
            0,
            defaults.outside,
            vec![crate::BrushRange {
                from: 4.0,
                to,
                style: defaults.positive,
            }],
        ));
        let incremental = chart.build_frame();
        assert_eq!(
            chart.frame_build_stats(),
            series_only,
            "a brush drag step must not relayout or rebuild unrelated layers"
        );
        chart.invalidate_frame_all();
        let full = chart.build_frame();
        assert_eq!(incremental.panes[0].main, full.panes[0].main);
        assert_eq!(incremental.panes[0].under, full.panes[0].under);
    }
    assert!(chart.clear_area_brush_state(0));
    chart.build_frame();
    assert_eq!(chart.frame_build_stats(), series_only);
}

#[test]
fn last_value_label_background_honors_the_per_point_color() {
    let mut chart = ohlc_chart(SeriesKind::Candlestick, 3);
    // The final bar carries a custom body color: the last-value label (and the built-in
    // last-price line) follow it (reference series-bar-colorer.ts).
    assert!(chart.set_series_point_colors(0, Some(vec![0, 0, POINT_RED]), None, None));
    let axis = chart.build_axis_frame(
        80.0,
        |t, _bold| t.len() as f64 * 7.0,
        |t, _bold| t.len() as f64 * 6.0,
    );
    assert!(
        axis.labels
            .iter()
            .any(|l| matches!(l.background, Some((.., c)) if c == Color(POINT_RED)))
    );
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::HLine { color, style: LineStyle::Dotted, .. } if *color == Color(POINT_RED)
    )));
}

/// A custom series (Phase C-c) with time-only (whitespace-style) rows, installed exactly like
/// the wasm host installs them: the engine owns the time mapping while every value arrives
/// through `set_custom_frame_values` (and, host-side, the plugin's autoscale contributions).
use crate::{CustomSeriesFrameValues, CustomSeriesLastValue, PrimitiveAutoscaleContribution};

fn custom_chart(bars: usize) -> (ChartEngine, SeriesId) {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let custom = chart.add_series(SeriesKind::Custom);
    let times: Vec<i64> = (0..bars as i64).collect();
    let nan = vec![f64::NAN; bars];
    chart.install_series_data(custom, times, nan.clone(), nan.clone(), nan.clone(), nan);
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    (chart, custom)
}

#[test]
fn custom_series_emits_no_prims_but_marks_its_paint_slot() {
    let (mut chart, custom) = custom_chart(4);
    let frame = chart.build_frame();
    // Both the (empty) primary candle series and the custom series record a paint mark, in
    // paint order; the custom series painted nothing, so its mark equals the previous one.
    let marks = &frame.panes[0].series_paint_marks;
    assert_eq!(marks.len(), 2);
    assert_eq!(marks[0].0, 0);
    assert_eq!(marks[1].0, custom);
    assert_eq!(marks[0].1, marks[1].1);
    assert!(frame.panes[0].main.is_empty());
    // ...and its time-only rows still anchor the base index (the reference's custom plot rows carry
    // values, so they count), exactly like a built-in kind with real bars.
    assert_eq!(chart.time_scale.base_index(), 3);
}

#[test]
fn custom_series_last_value_line_and_label_follow_the_frame_values() {
    let (mut chart, custom) = custom_chart(4);
    chart.set_price_scale_visible_range_for(0, PriceScaleTarget::Right, 90.0, 110.0);
    let global = CustomSeriesLastValue {
        value: 105.0,
        color: Color::rgb(0x11, 0x22, 0x33),
        time: 3,
    };
    let visible = CustomSeriesLastValue {
        value: 104.0,
        color: Color::rgb(0x44, 0x55, 0x66),
        time: 2,
    };
    chart.set_custom_frame_values(
        custom,
        CustomSeriesFrameValues {
            first_value: Some(100.0),
            last: Some(global),
            last_visible: Some(visible),
        },
    );
    // The built-in last-price line tracks the GLOBAL last (default priceLineSource LastBar).
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::HLine { color, style: LineStyle::Dotted, .. } if *color == global.color
    )));
    // priceLineSource LastVisible switches the line to the visible record.
    chart.series_entry_mut(custom).unwrap().price_line_source = 1;
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::HLine { color, style: LineStyle::Dotted, .. } if *color == visible.color
    )));
    // The last-value axis label always tracks the visible record (reference lastValueData(false)).
    let axis = chart.build_axis_frame(
        80.0,
        |t, _bold| t.len() as f64 * 7.0,
        |t, _bold| t.len() as f64 * 6.0,
    );
    assert!(axis.labels.iter().any(
        |l| matches!(l.background, Some((.., c)) if c == visible.color) && l.text == "104.00"
    ));

    // A valid explicit live color overrides both custom frame colors without changing the
    // line's LastVisible source or the label's visible-value text.
    let live = Color::rgb(0xf0, 0xe6, 0x8c);
    chart.series_entry_mut(custom).unwrap().price_line_color = Some(live.to_hex());
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::HLine { color, style: LineStyle::Dotted, .. } if *color == live
    )));
    let axis = chart.build_axis_frame(
        80.0,
        |t, _bold| t.len() as f64 * 7.0,
        |t, _bold| t.len() as f64 * 6.0,
    );
    assert!(axis.labels.iter().any(|label| {
        label.text == "104.00"
            && label.color == Color::rgb(0, 0, 0)
            && matches!(label.background, Some((.., color)) if color == live)
    }));
}

#[test]
fn custom_series_last_value_data_and_base_value_come_from_the_frame_values() {
    let (mut chart, custom) = custom_chart(3);
    chart.set_price_scale_visible_range_for(0, PriceScaleTarget::Right, 90.0, 110.0);
    chart.set_custom_frame_values(
        custom,
        CustomSeriesFrameValues {
            first_value: Some(100.0),
            last: Some(CustomSeriesLastValue {
                value: 103.5,
                color: Color::rgb(1, 2, 3),
                time: 2,
            }),
            last_visible: Some(CustomSeriesLastValue {
                value: 102.5,
                color: Color::rgb(1, 2, 3),
                time: 1,
            }),
        },
    );
    // reference `ISeriesApi.lastValueData`: global vs visible, formatted with the series' price format.
    let global: serde_json::Value =
        serde_json::from_str(&chart.series_last_value_data(custom, true).unwrap()).unwrap();
    assert_eq!(global["value"].as_f64().unwrap(), 103.5);
    assert_eq!(global["formatted"].as_str().unwrap(), "103.50");
    assert_eq!(global["time"].as_i64().unwrap(), 2);
    let visible: serde_json::Value =
        serde_json::from_str(&chart.series_last_value_data(custom, false).unwrap()).unwrap();
    assert_eq!(visible["value"].as_f64().unwrap(), 102.5);
    let latest_snapshot = chart
        .value_snapshot(None)
        .into_iter()
        .find(|snapshot| snapshot.series_id == custom)
        .unwrap();
    assert_eq!(latest_snapshot.value, Some(103.5));
    let exact_snapshot = chart
        .value_snapshot(Some(2))
        .into_iter()
        .find(|snapshot| snapshot.series_id == custom)
        .unwrap();
    assert_eq!(exact_snapshot.value, None);
    assert_eq!(exact_snapshot.previous_value, None);
    // The custom first value anchors coordinate conversion (series_base_value's custom branch).
    assert_eq!(chart.series_base_value(custom, 0), Some(100.0));
    // A non-custom kind rejects frame values, and a custom series without them reports nothing.
    assert!(chart.series_last_value_data(0, true).is_none());
    chart.set_custom_frame_values(
        0,
        CustomSeriesFrameValues {
            first_value: Some(1.0),
            last: None,
            last_visible: None,
        },
    );
    assert!(chart.series_base_value(0, 0).is_none());
}

#[test]
fn custom_series_autoscale_contribution_merges_through_the_custom_base_value() {
    let (mut chart, custom) = custom_chart(4);
    // The host-side contract (Phase C-c): the plugin's price values reach the scale through
    // the C-b contribution path, gated on the custom first value like any built-in series.
    chart.set_custom_frame_values(
        custom,
        CustomSeriesFrameValues {
            first_value: Some(100.0),
            last: None,
            last_visible: None,
        },
    );
    chart.add_autoscale_contribution(PrimitiveAutoscaleContribution {
        series: custom,
        pane: 0,
        target: PriceScaleTarget::Right,
        min: 95.0,
        max: 115.0,
    });
    chart.autoscale_visible();
    let (from, to) = chart
        .price_scale_visible_range_for(0, PriceScaleTarget::Right)
        .unwrap();
    assert!(
        from <= 95.0 && to >= 115.0,
        "range [{from}, {to}] must contain the contribution"
    );
    // No first value recorded -> the contribution is dropped (same gate as built-ins without
    // data), and the scale keeps whatever range it had.
    chart.set_custom_frame_values(custom, CustomSeriesFrameValues::default());
    chart.clear_autoscale_contributions();
    chart.add_autoscale_contribution(PrimitiveAutoscaleContribution {
        series: custom,
        pane: 0,
        target: PriceScaleTarget::Right,
        min: 1.0,
        max: 2.0,
    });
    chart.autoscale_visible();
    let (from, to) = chart
        .price_scale_visible_range_for(0, PriceScaleTarget::Right)
        .unwrap();
    assert!(
        from < 1.0 || to > 2.0,
        "stale range must survive a dropped contribution"
    );
}

#[test]
fn crosshair_follows_the_cursor_into_the_empty_area() {
    // Two empty slots on the right (reference `rightOffset` > 0): hovering past the last bar
    // keeps the crosshair following the cursor onto the empty slot instead of sticking to the
    // last bar; the time label hides there (reference `indexToTime` ? null).
    let mut chart = crosshair_chart();
    chart.set_right_offset(2.0);
    chart.build_frame();
    let empty_x = chart.time_scale.index_to_coordinate(3);
    chart.crosshair = Some((empty_x, 120.0));
    let frame = chart.build_frame();
    let empty_slot_x = chart.time_scale.index_to_coordinate(3).round() as i32;
    let last_bar_x = chart.time_scale.index_to_coordinate(2).round() as i32;
    assert!(empty_slot_x != last_bar_x);
    assert!(
        frame.panes[0]
            .main
            .iter()
            .any(|p| matches!(p, Prim::VLine { x, .. } if *x == empty_slot_x)),
        "vertical line must follow the cursor into the empty area"
    );
    let axis = chart.build_axis_frame(
        80.0,
        |t, _bold| t.len() as f64 * 7.0,
        |t, _bold| t.len() as f64 * 6.0,
    );
    assert!(
        !axis
            .labels
            .iter()
            .any(|l| l.background.is_some() && l.midpoint == AxisTextMidpoint::StableTime),
        "no time label in the empty area"
    );

    // Control: hovering a real bar still shows the time label.
    chart.crosshair = Some((chart.time_scale.index_to_coordinate(2), 120.0));
    let axis = chart.build_axis_frame(
        80.0,
        |t, _bold| t.len() as f64 * 7.0,
        |t, _bold| t.len() as f64 * 6.0,
    );
    assert!(
        axis.labels
            .iter()
            .any(|l| l.background.is_some() && l.midpoint == AxisTextMidpoint::StableTime),
        "time label present over a real bar"
    );
}

#[test]
fn bold_round_labels_decile_rule() {
    // Uniform step 2: multiples of 20 are round.
    let marks: Vec<f64> = (88..=126).step_by(2).map(|v| v as f64).collect();
    let bold = ChartEngine::bold_round_decisions(&marks, true);
    for (v, b) in marks.iter().zip(&bold) {
        assert_eq!(*b, matches!(*v as i64, 100 | 120), "value {v}");
    }
    // Uniform step 1000: multiples of 10000 are round (the public reference's screenshot behavior).
    let marks: Vec<f64> = (16_000..=40_000).step_by(1_000).map(|v| v as f64).collect();
    let bold = ChartEngine::bold_round_decisions(&marks, true);
    for (v, b) in marks.iter().zip(&bold) {
        assert_eq!(
            *b,
            *v as i64 == 40_000 || *v as i64 % 10_000 == 0,
            "value {v}"
        );
    }
    // Negatives follow the same rule (uniform step 4: multiples of 40 are round).
    let bold = ChartEngine::bold_round_decisions(
        &[
            -40.0, -36.0, -32.0, -28.0, -24.0, -20.0, -16.0, -12.0, -8.0, -4.0, 0.0,
        ],
        true,
    );
    assert_eq!(
        bold,
        vec![
            true, false, false, false, false, false, false, false, false, false, true
        ]
    );
    // Disabled: nothing bold.
    let bold = ChartEngine::bold_round_decisions(&[100.0, 120.0], false);
    assert_eq!(bold, vec![false, false]);
    // Non-uniform (log-style) ticks: exact powers of ten only.
    let bold = ChartEngine::bold_round_decisions(&[1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0], true);
    assert_eq!(bold, vec![true, false, false, true, false, false, true]);
}

#[test]
fn axis_primitives_keep_normal_and_round_tick_weights_distinct() {
    let mut chart = crosshair_chart();
    let mut axis = chart.build_axis_frame(
        80.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    let mut normal = axis
        .labels
        .iter()
        .find(|label| label.background.is_none())
        .cloned()
        .expect("axis tick label");
    normal.text = "normal".to_string();
    normal.bold = false;
    let mut rounded = normal.clone();
    rounded.text = "rounded".to_string();
    rounded.bold = true;
    axis.labels = vec![normal, rounded];

    let mut primitives = Vec::new();
    chart.build_axis_primitives_into(&axis, &mut primitives);
    assert!(
        primitives
            .iter()
            .any(|prim| matches!(prim, Prim::Text { text, weight: 400, .. } if text == "normal"))
    );
    assert!(
        primitives
            .iter()
            .any(|prim| matches!(prim, Prim::Text { text, weight: 700, .. } if text == "rounded"))
    );
}

#[test]
fn allow_bold_labels_gates_major_time_ticks() {
    let mut chart = crosshair_chart();
    chart.time_scale.set_width(300.0);
    let frame = chart.build_axis_frame(
        80.0,
        |t, _bold| t.len() as f64 * 7.0,
        |t, _bold| t.len() as f64 * 6.0,
    );
    let time_labels: Vec<_> = frame
        .labels
        .iter()
        .filter(|l| l.midpoint == AxisTextMidpoint::None)
        .collect();
    assert!(
        time_labels.iter().any(|l| l.bold),
        "major labels bold by default"
    );
    chart.time_scale.set_allow_bold_labels(false);
    let frame = chart.build_axis_frame(
        80.0,
        |t, _bold| t.len() as f64 * 7.0,
        |t, _bold| t.len() as f64 * 6.0,
    );
    assert!(
        frame
            .labels
            .iter()
            .all(|l| l.midpoint != AxisTextMidpoint::None || !l.bold),
        "no bold time labels when allowBoldLabels is off"
    );
}

#[test]
fn future_time_projection_paints_axis_labels_past_the_last_real_bar() {
    let mut chart = ChartEngine::new(600.0, 300.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1_000.0, 1_060.0, 1_120.0],
            &[100.0, 101.0, 102.0],
            &[101.0, 102.0, 103.0],
            &[99.0, 100.0, 101.0],
            &[100.5, 101.5, 102.5],
        )
        .unwrap();
    chart.set_future_time_projection(Some(60), 64);
    chart.recompute_layout_with_measure(
        true,
        |text, _bold| text.len() as f64 * 6.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    chart.set_right_offset(12.0);
    chart.set_tick_mark_formatter(Some(Box::new(|timestamp, _| Some(timestamp.to_string()))));

    let visible = chart.time_scale.visible_strict_range().unwrap();
    assert!(visible.right() > 2, "future logical space must be visible");
    let marks = chart.time_marks(40.0);
    assert!(
        marks.iter().any(|(index, _)| *index > 2),
        "future projected tick marks must exist before axis rendering"
    );

    let frame = chart.build_axis_frame(
        40.0,
        |text, _bold| text.len() as f64 * 6.0,
        |text, _bold| text.len() as f64 * 6.0,
    );

    assert!(
        frame.labels.iter().any(|label| {
            label.midpoint == AxisTextMidpoint::None
                && label
                    .text
                    .parse::<i64>()
                    .is_ok_and(|timestamp| timestamp > 1_120)
        }),
        "future projected timestamps must survive the frame's visible-range filter"
    );
}

#[test]
fn past_time_projection_paints_axis_labels_before_the_first_real_bar() {
    let mut chart = ChartEngine::new(600.0, 300.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1_000.0, 1_060.0, 1_120.0],
            &[100.0, 101.0, 102.0],
            &[101.0, 102.0, 103.0],
            &[99.0, 100.0, 101.0],
            &[100.5, 101.5, 102.5],
        )
        .unwrap();
    chart.set_past_time_projection(Some(60), 64);
    chart.recompute_layout_with_measure(
        true,
        |text, _bold| text.len() as f64 * 6.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    chart.set_visible_logical_range(-12.0, 2.0);
    chart.set_tick_mark_formatter(Some(Box::new(|timestamp, _| Some(timestamp.to_string()))));

    let visible = chart.time_scale.visible_strict_range().unwrap();
    assert!(visible.left() < 0, "past logical space must be visible");
    let marks = chart.time_marks(40.0);
    assert!(
        marks.iter().any(|(index, _)| *index < 0),
        "past projected tick marks must exist before axis rendering"
    );

    let frame = chart.build_axis_frame(
        40.0,
        |text, _bold| text.len() as f64 * 6.0,
        |text, _bold| text.len() as f64 * 6.0,
    );

    assert!(
        frame.labels.iter().any(|label| {
            label.midpoint == AxisTextMidpoint::None
                && label
                    .text
                    .parse::<i64>()
                    .is_ok_and(|timestamp| timestamp < 1_000)
        }),
        "past projected timestamps must survive the frame's visible-range filter"
    );
}

// ---- industry-standard last-value cluster: title chip + price + candle-close countdown ----

#[test]
fn countdown_interval_is_the_median_of_recent_deltas() {
    use super::axis::median_bar_interval;
    // Fewer than two bars (or no positive delta) hides the countdown.
    assert_eq!(median_bar_interval(&[]), None);
    assert_eq!(median_bar_interval(&[100]), None);
    assert_eq!(median_bar_interval(&[5, 5, 5]), None);
    // Single-delta fallback: the last delta.
    assert_eq!(median_bar_interval(&[100, 460]), Some(360.0));
    assert_eq!(median_bar_interval(&[0, 60, 120, 180]), Some(60.0));
    // A single outlier delta does not move the median.
    let mut times: Vec<i64> = (0..20).map(|i| i * 60).collect();
    times[15] = 14 * 60 + 10;
    assert_eq!(median_bar_interval(&times), Some(60.0));
    // Only the last 11 bars (10 deltas) feed the inference: an old irregular regime is dropped.
    let mut times: Vec<i64> = (0..20).map(|i| i * 60).collect();
    times[1] = 5;
    times[2] = 6;
    assert_eq!(median_bar_interval(&times), Some(60.0));
}

#[test]
fn countdown_format_covers_all_three_ranges() {
    use super::axis::format_countdown_remaining as fmt;
    // < 1h: zero-padded mm:ss (floored to whole seconds, clamped at zero).
    assert_eq!(fmt(50.0), "00:50");
    assert_eq!(fmt(59.9), "00:59");
    assert_eq!(fmt(60.0), "01:00");
    assert_eq!(fmt(3599.0), "59:59");
    assert_eq!(fmt(-3.0), "00:00");
    // < 1d: hh:mm:ss.
    assert_eq!(fmt(3600.0), "01:00:00");
    assert_eq!(fmt(3661.0), "01:01:01");
    assert_eq!(fmt(86399.0), "23:59:59");
    // >= 1d: "Xd Xh".
    assert_eq!(fmt(86400.0), "1d 0h");
    assert_eq!(fmt(2.0 * 86400.0 + 5.0 * 3600.0 + 120.0), "2d 5h");
    assert_eq!(fmt(10_000.0 * 86400.0), "9999d+");
}

/// One line series on 60s bars ending at t=240, close 12.5; the host clock pins the countdown.
fn countdown_chart() -> ChartEngine {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Line;
    let times = [0.0, 60.0, 120.0, 180.0, 240.0];
    let values = [10.0, 11.0, 12.0, 11.5, 12.5];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart
}

fn boxed_labels(chart: &mut ChartEngine) -> Vec<AxisLabel> {
    chart
        .build_axis_frame(
            80.0,
            |t, _bold| t.len() as f64 * 7.0,
            |t, _bold| t.len() as f64 * 6.0,
        )
        .labels
        .into_iter()
        .filter(|l| l.background.is_some())
        .collect()
}

#[test]
fn countdown_text_tracks_the_pinned_host_clock() {
    let mut chart = countdown_chart();
    chart.series[0].countdown_visible = true;
    // Headless determinism: no host clock, no countdown row at all.
    assert_eq!(chart.series_countdown_text(0), None);
    assert_eq!(boxed_labels(&mut chart).len(), 1); // the plain price label only
    // 60s interval, last bar t=240: next close 300.
    chart.now_override = Some(250.0);
    assert_eq!(chart.series_countdown_text(0).as_deref(), Some("00:50"));
    chart.now_override = Some(299.7);
    assert_eq!(chart.series_countdown_text(0).as_deref(), Some("00:00"));
    // A quiet market keeps counting subsequent inferred intervals rather than
    // freezing at zero until the next data point arrives.
    chart.now_override = Some(300.0);
    assert_eq!(chart.series_countdown_text(0).as_deref(), Some("01:00"));
    chart.now_override = Some(302.0);
    assert_eq!(chart.series_countdown_text(0).as_deref(), Some("00:58"));
    chart.now_override = Some(480.0);
    assert_eq!(chart.series_countdown_text(0).as_deref(), Some("01:00"));
}

#[test]
fn closed_market_hides_the_countdown_without_touching_the_preference() {
    let mut chart = countdown_chart();
    chart.series[0].countdown_visible = true;
    chart.set_now_seconds(250.0);
    assert_eq!(boxed_labels(&mut chart).len(), 2, "price + countdown rows");

    chart.set_bar_countdown_active(false);
    assert_eq!(chart.series_countdown_text(0), None);
    assert_eq!(
        boxed_labels(&mut chart).len(),
        1,
        "a closed session paints only the price label"
    );
    chart.set_now_seconds(251.0);
    assert_eq!(
        chart.series_countdown_text(0),
        None,
        "the clock keeps ticking"
    );
    assert!(chart.series[0].countdown_visible, "user preference is kept");

    chart.set_bar_countdown_active(true);
    assert_eq!(chart.series_countdown_text(0).as_deref(), Some("00:49"));
    assert_eq!(boxed_labels(&mut chart).len(), 2);
}

#[test]
fn last_value_cluster_chips_stay_solid_when_the_series_color_is_translucent() {
    let mut chart = countdown_chart();
    chart.now_override = Some(250.0);
    chart.series[0].title = "NDQ".to_string();
    chart.series[0].countdown_visible = true;
    // Half-alpha series color: the chips keep the hue but must paint fully opaque.
    chart.series[0].line_color = Some("rgba(38,166,154,0.5)".to_string());

    let labels = boxed_labels(&mut chart);
    assert_eq!(labels.len(), 3);
    let solid = Color::rgb(0x26, 0xa6, 0x9a);
    for label in &labels {
        let Some((_, _, _, _, bg)) = label.background else {
            panic!("cluster label is boxed")
        };
        assert_eq!(bg, solid, "chip background for {:?}", label.text);
    }

    // The plain single-box price label (title + countdown off) is solid too.
    let mut chart = countdown_chart();
    chart.series[0].line_color = Some("rgba(38,166,154,0.5)".to_string());
    let labels = boxed_labels(&mut chart);
    assert_eq!(labels.len(), 1);
    let Some((_, _, _, _, bg)) = labels[0].background else {
        panic!("price label is boxed")
    };
    assert_eq!(bg, solid);
}

#[test]
fn last_value_cluster_rows_toggle_independently() {
    let mut chart = countdown_chart();
    chart
        .apply_options(r##"{"layout":{"textColor":"#333333","mutedTextColor":"#737373"}}"##)
        .unwrap();
    chart.now_override = Some(250.0);
    chart.series[0].title = "NDQ".to_string();
    chart.series[0].countdown_visible = true;

    // All three parts on: chip + price area + countdown, stacked in one connected box.
    let labels = boxed_labels(&mut chart);
    assert_eq!(labels.len(), 3);
    let chip = labels
        .iter()
        .find(|l| l.text == "NDQ")
        .expect("title chip label");
    let price = labels
        .iter()
        .find(|l| l.text == "12.50")
        .expect("price label");
    let countdown = labels
        .iter()
        .find(|l| l.text == "00:50")
        .expect("countdown label");
    let Some((chip_x, chip_y, chip_w, chip_h, chip_bg)) = chip.background else {
        panic!("chip is boxed")
    };
    let Some((price_x, price_y, price_w, price_h, price_bg)) = price.background else {
        panic!("price area is boxed")
    };
    let Some((cd_x, cd_y, cd_w, cd_h, cd_bg)) = countdown.background else {
        panic!("countdown row is boxed")
    };
    // The chip shares the main label color by default (matching the price/countdown chips).
    assert_eq!(chip_bg, LINE);
    assert_eq!(price_bg, LINE);
    assert_eq!(cd_bg, LINE);
    // The title chip ends at the logical border. The primitive encoder starts the axis-side box
    // after the border's device pixels, so the border alone is the seam: no overlap and no extra
    // chart-surface gap.
    assert_eq!(chip_y, price_y);
    let border_x = chart.pane_left + chart.pane_w;
    assert_eq!(chip_x + chip_w, border_x);
    assert_eq!(price_x, border_x);
    assert!((price_y + price_h - cd_y).abs() < 1e-9, "flush stack");
    assert!((cd_x - price_x).abs() < 1e-9, "same left edge");
    assert!((cd_w - price_w).abs() < 1e-9, "same width");
    assert_eq!(chip_h, price_h);
    // Shared metrics: 11px price text with 2px padding per side; 10px countdown text with
    // 2px padding per side, so the countdown reads as secondary information.
    assert_eq!(price_h, 11.0 + 2.0 * 2.0);
    assert_eq!(cd_h, 10.0 + 2.0 * 2.0);
    assert_eq!(price.font_scale, 11.0 / 12.0);
    assert_eq!(countdown.font_scale, 10.0 / 12.0);
    // This dark live background selects white; countdown uses the same RGB at reduced opacity.
    assert_eq!(chip.color, LIVE_TEXT);
    assert_eq!(price.color, LIVE_TEXT);
    assert_eq!(countdown.color, LIVE_COUNTDOWN);

    // Price off, title + countdown on: only the outside title chip and the inside countdown
    // chip render — no empty price box, and the title chip ATTACHES to the countdown row
    // (same top edge, same height) instead of leaving a blank row in the strip.
    chart.series[0].last_value_visible = false;
    let labels = boxed_labels(&mut chart);
    assert_eq!(labels.len(), 2);
    assert!(labels.iter().any(|l| l.text == "NDQ"));
    assert!(labels.iter().any(|l| l.text == "00:50"));
    assert!(labels.iter().all(|l| l.text != "12.50"));
    assert!(
        labels.iter().all(|l| !l.text.is_empty()),
        "no empty price box when the price text is off"
    );
    let chip = labels.iter().find(|l| l.text == "NDQ").unwrap();
    let countdown = labels.iter().find(|l| l.text == "00:50").unwrap();
    let Some((_, chip_y, _, chip_h, _)) = chip.background else {
        panic!("chip is boxed")
    };
    let Some((_, cd_y, _, cd_h, _)) = countdown.background else {
        panic!("countdown is boxed")
    };
    assert_eq!(
        chip_y, cd_y,
        "title chip shares the countdown row's top edge"
    );
    assert_eq!(
        chip_h, cd_h,
        "title chip matches the countdown row's height"
    );

    // Title off, price + countdown on: no chip; the price area spans the top row's full width.
    chart.series[0].last_value_visible = true;
    chart.series[0].title_visible = false;
    let labels = boxed_labels(&mut chart);
    assert_eq!(labels.len(), 2);
    assert!(labels.iter().all(|l| l.text != "NDQ"));
    assert!(labels.iter().any(|l| l.text == "12.50"));
    assert!(labels.iter().any(|l| l.text == "00:50"));

    // Countdown off, title + price on: no countdown row below.
    chart.series[0].title_visible = true;
    chart.series[0].countdown_visible = false;
    let labels = boxed_labels(&mut chart);
    assert_eq!(labels.len(), 2);
    assert!(labels.iter().any(|l| l.text == "NDQ"));
    assert!(labels.iter().all(|l| l.text != "00:50"));

    // Everything off: no cluster at all.
    chart.series[0].last_value_visible = false;
    chart.series[0].title_visible = false;
    assert!(boxed_labels(&mut chart).is_empty());
}

#[test]
fn last_value_cluster_counts_down_only_with_data_and_interval() {
    let mut chart = countdown_chart();
    chart.now_override = Some(250.0);
    chart.series[0].countdown_visible = true;
    chart.series[0].last_value_visible = false;
    // A countdown-only cluster renders (no top row).
    let labels = boxed_labels(&mut chart);
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].text, "00:50");
    // A series with a single bar has no interval: no countdown row, and with the price label
    // off no cluster at all.
    chart
        .set_series_data(0, &[240.0], &[12.0], &[12.0], &[12.0], &[12.0])
        .unwrap();
    assert!(boxed_labels(&mut chart).is_empty());
}

#[test]
fn boxed_axis_labels_select_the_axis_facing_corners() {
    // Plain last-value label on the right strip: right corners rounded.
    let mut chart = countdown_chart();
    let labels = boxed_labels(&mut chart);
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].background_corners, AxisLabelCorners::RIGHT);
    let (right_x, ..) = labels[0].background.expect("right boxed label");
    assert_eq!(right_x, chart.pane_left + chart.pane_w);

    // The same label on the left strip rounds the left corners.
    chart.set_price_scale_visible_for(0, PriceScaleTarget::Left, true);
    chart.set_series_price_scale(0, PriceScaleTarget::Left);
    let labels = boxed_labels(&mut chart);
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].background_corners, AxisLabelCorners::LEFT);
    let (left_x, _, left_w, _, _) = labels[0].background.expect("left boxed label");
    assert_eq!(left_x + left_w, chart.pane_left);
    chart.set_series_price_scale(0, PriceScaleTarget::Right);

    // The crosshair price label follows its strip; the time label rounds the bottom corners.
    // Both choose black text against a configured light background.
    chart
        .apply_options(
            r##"{"crosshair":{"horzLine":{"labelBackgroundColor":"#f0e68c"},"vertLine":{"labelBackgroundColor":"#f0e68c"}}}"##,
        )
        .unwrap();
    chart.crosshair = Some((400.0, 250.0));
    let labels = boxed_labels(&mut chart);
    let light_crosshair = Color::rgb(0xf0, 0xe6, 0x8c);
    let price = labels
        .iter()
        .find(|label| {
            matches!(label.background, Some((.., color)) if color == light_crosshair)
                && label.midpoint == AxisTextMidpoint::Label
        })
        .expect("crosshair price label");
    assert_eq!(price.background_corners, AxisLabelCorners::RIGHT);
    assert_eq!(price.color, Color::rgb(0, 0, 0));
    let (price_x, _, _, price_h, _) = price.background.expect("boxed crosshair price label");
    assert_eq!(price_x, chart.pane_left + chart.pane_w);
    assert_eq!(price_h, 19.0);
    let time = labels
        .iter()
        .find(|l| l.midpoint == AxisTextMidpoint::StableTime)
        .expect("crosshair time label");
    assert_eq!(time.background_corners, AxisLabelCorners::BOTTOM);
    assert_eq!(time.color, Color::rgb(0, 0, 0));
    let (_, time_y, _, time_h, _) = time.background.expect("boxed time label");
    assert_eq!(time_y, chart.pane_h);
    // Shared time strip: axis 11 + 1 border + 3 tick + 3 + 3 padding = 21, even-snapped 22.
    assert_eq!(time_h, 22.0);
    assert_eq!(time.y, chart.pane_h + 1.0 + 3.0 + 3.0 + 11.0 / 2.0);
    chart.crosshair = None;

    // Cluster: only the outer axis-facing corners round; internal boundaries stay sharp.
    chart.now_override = Some(250.0);
    chart.series[0].title = "NDQ".to_string();
    chart.series[0].countdown_visible = true;
    let labels = boxed_labels(&mut chart);
    let corners_of = |text: &str| {
        labels
            .iter()
            .find(|l| l.text == text)
            .unwrap_or_else(|| panic!("label {text}"))
            .background_corners
    };
    assert_eq!(
        corners_of("NDQ"),
        AxisLabelCorners::LEFT,
        "the outside title chip rounds only its outer (chart-facing) side"
    );
    assert_eq!(
        corners_of("12.50"),
        AxisLabelCorners {
            top_right: true,
            ..AxisLabelCorners::NONE
        }
    );
    assert_eq!(
        corners_of("00:50"),
        AxisLabelCorners {
            bottom_right: true,
            ..AxisLabelCorners::NONE
        }
    );

    // Without the countdown row the top row takes over the bottom axis-facing corner.
    chart.series[0].countdown_visible = false;
    let labels = boxed_labels(&mut chart);
    let corners_of = |text: &str| {
        labels
            .iter()
            .find(|l| l.text == text)
            .unwrap_or_else(|| panic!("label {text}"))
            .background_corners
    };
    assert_eq!(corners_of("12.50"), AxisLabelCorners::RIGHT);

    // On the left strip the chip (leftmost top-row box) carries the axis-facing corners.
    chart.set_price_scale_visible_for(0, PriceScaleTarget::Left, true);
    chart.set_series_price_scale(0, PriceScaleTarget::Left);
    chart.series[0].countdown_visible = true;
    let labels = boxed_labels(&mut chart);
    let corners_of = |text: &str| {
        labels
            .iter()
            .find(|l| l.text == text)
            .unwrap_or_else(|| panic!("label {text}"))
            .background_corners
    };
    assert_eq!(
        corners_of("NDQ"),
        AxisLabelCorners::RIGHT,
        "the outside title chip rounds only its outer (chart-facing) side"
    );
    assert_eq!(
        corners_of("12.50"),
        AxisLabelCorners {
            top_left: true,
            ..AxisLabelCorners::NONE
        }
    );
    assert_eq!(
        corners_of("00:50"),
        AxisLabelCorners {
            bottom_left: true,
            ..AxisLabelCorners::NONE
        }
    );

    // A countdown-only cluster rounds both axis-facing corners on its single row.
    chart.series[0].last_value_visible = false;
    chart.series[0].title_visible = false;
    let labels = boxed_labels(&mut chart);
    assert_eq!(labels.len(), 1);
    assert_eq!(
        labels[0].background_corners,
        AxisLabelCorners {
            top_left: true,
            bottom_left: true,
            ..AxisLabelCorners::NONE
        }
    );
}

#[test]
fn boxed_labels_begin_beyond_the_axis_border_at_every_dpr() {
    for dpr in [1.0, 1.25, 1.5, 2.0, 3.0] {
        let mut chart = countdown_chart();
        chart.dpr = dpr;
        chart.series[0].title = "NDQ".to_string();
        chart.series[0].countdown_visible = false;
        chart.crosshair = Some((400.0, 250.0));
        let axis = chart.build_axis_frame(
            80.0,
            |text, _bold| text.len() as f64 * 7.0,
            |text, _bold| text.len() as f64 * 6.0,
        );
        let mut primitives = Vec::new();
        chart.build_axis_primitives_into(&axis, &mut primitives);

        let border_w = aeris_charts_core::style::border_width_device_px(dpr) as i32;
        let price_border = ((chart.pane_left + chart.pane_w) * dpr).round() as i32;
        let title_box = primitives.iter().find_map(|primitive| match primitive {
            Prim::RoundRect {
                x, w, radii, fill, ..
            } if *x < price_border as f32 && *fill == LINE => Some((*x, *w, *radii)),
            _ => None,
        });
        let (title_x, title_w, title_radii) = title_box.expect("title chip primitive");
        assert_eq!(
            title_x + title_w,
            price_border as f32,
            "title chip must end at the border with no overlap or surface gap at dpr {dpr}"
        );
        assert!(
            title_radii
                .into_iter()
                .filter(|radius| *radius > 0.0)
                .all(|radius| {
                    (radius - crate::axis_metrics::AxisMetrics::TAG_RADIUS as f32 * dpr as f32)
                        .abs()
                        < 1e-4
                })
        );
        assert!(
            primitives.iter().any(|primitive| matches!(
                primitive,
                Prim::RoundRect { x, fill, .. }
                    if *x == (price_border + border_w) as f32 && *fill == LINE
            )),
            "price label must start after the border at dpr {dpr}"
        );

        let time_border = (chart.pane_h * dpr).round() as i32;
        assert!(
            primitives.iter().any(|primitive| matches!(
                primitive,
                Prim::RoundRect { y, fill, .. }
                    if *y == (time_border + border_w) as f32 && *fill == CROSSHAIR_LABEL_BG
            )),
            "time label must start below the border at dpr {dpr}"
        );
    }
}

#[test]
fn axis_width_negotiation_includes_the_secondary_countdown_row() {
    let measure = |t: &str, _bold: bool| t.len() as f64 * 7.0;
    let countdown_measure = |t: &str, _bold: bool| t.len() as f64 * 6.0;
    let mut chart = countdown_chart();
    let plain =
        chart.optimal_price_axis_width_for(PriceScaleTarget::Right, measure, countdown_measure);
    // Tick + price texts are 5 chars here; the reference's worst-case crosshair sample
    // ("9.11"/"12.89") is also 5 chars or less, so the plain strip covers the widest label
    // (shared chrome: 1 border + 3 tick + 4 + 4 padding + 5 chars at 7px = 47, even 48).
    assert_eq!(plain, 48.0);

    // The eight-character countdown is wider than the primary price and must widen the strip.
    chart.series[0].countdown_visible = true;
    chart.now_override = Some(300.0 - 86399.0);
    let with_countdown =
        chart.optimal_price_axis_width_for(PriceScaleTarget::Right, measure, countdown_measure);
    assert_eq!(with_countdown, 60.0);

    // The title chip lives OUTSIDE the strip (pane side), so it never widens the axis.
    chart.now_override = Some(250.0); // "00:50" — same 5 chars as the price
    chart.series[0].title = "NDQ".to_string();
    let with_cluster =
        chart.optimal_price_axis_width_for(PriceScaleTarget::Right, measure, countdown_measure);
    assert_eq!(with_cluster, 48.0, "outside chip must not widen the strip");
}

#[test]
fn exact_axis_width_negotiation_includes_the_countdown_row() {
    let measure = |text: &str, _bold: bool| text.len() as f64 * 7.0;
    let countdown_measure = |text: &str, _bold: bool| text.len() as f64 * 6.0;
    let mut chart = countdown_chart();
    chart.series[0].countdown_visible = true;
    chart.now_override = Some(300.0 - 86399.0);

    // Eight-char countdown at 6px plus shared 12px chrome: 60 even.
    assert_eq!(
        chart.optimal_exact_price_axis_width_for(
            0,
            PriceScaleTarget::Right,
            measure,
            countdown_measure
        ),
        60.0
    );
}

#[test]
fn countdown_clock_requests_layout_without_invalidating_coordinates() {
    let mut chart = countdown_chart();
    chart.series[0].countdown_visible = true;
    chart.recompute_layout_with_measure(
        true,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    chart.build_frame();
    assert!(!chart.frame_requires_layout());
    let coordinate_revision = chart.frame_coordinate_revision();

    chart.set_now_seconds(300.0 - 86399.0);

    assert!(chart.frame_requires_layout());
    assert_eq!(chart.frame_coordinate_revision(), coordinate_revision);

    chart.recompute_layout_with_measure(
        false,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    chart.build_axis_frame(
        80.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    chart.set_now_seconds(300.0 - 86398.0);
    assert!(!chart.frame_requires_layout());
    assert!(chart.frame_requires_axis());
}

#[test]
fn countdown_clock_does_not_invalidate_a_series_without_an_interval() {
    let mut chart = countdown_chart();
    chart
        .set_series_data(0, &[240.0], &[12.0], &[12.0], &[12.0], &[12.0])
        .unwrap();
    chart.series[0].countdown_visible = true;
    chart.recompute_layout_with_measure(
        true,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    chart.build_axis_frame(
        80.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );

    chart.set_now_seconds(250.0);

    assert!(!chart.frame_requires_layout());
    assert!(!chart.frame_requires_axis());
}

#[test]
fn long_title_chip_is_fitted_inside_the_pane() {
    let mut chart = countdown_chart();
    chart.series[0].title = "A".repeat(500);
    chart.recompute_layout_with_measure(
        true,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    let labels = boxed_labels(&mut chart);
    let title = labels
        .iter()
        .find(|label| label.text.ends_with("..."))
        .expect("fitted title chip");
    let (x, _, width, _, _) = title.background.expect("title background");
    assert!(x >= chart.pane_left);
    assert!(x + width <= chart.pane_left + chart.pane_w);
}

#[test]
fn built_in_year_labels_honor_the_character_limit_without_truncation() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times = [0.0, 31_536_000.0, 63_072_000.0];
    let values = [10.0; 3];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();

    let labels = chart.build_axis_frame(
        80.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    assert!(labels.labels.iter().any(|label| label.text == "1970"));

    chart.set_tick_mark_max_character_length(2);
    let labels = chart.build_axis_frame(
        20.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    assert!(!labels.labels.iter().any(|label| label.text == "1970"));

    chart.set_tick_mark_formatter(Some(Box::new(|_, _| None)));
    let labels = chart.build_axis_frame(
        20.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    assert!(!labels.labels.iter().any(|label| label.text == "1970"));

    chart.set_tick_mark_formatter(Some(Box::new(|_, _| Some("custom-year".to_string()))));
    let labels = chart.build_axis_frame(
        20.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    assert!(
        labels
            .labels
            .iter()
            .any(|label| label.text == "custom-year")
    );
}

#[test]
fn axis_width_negotiation_ignores_the_transient_crosshair_label() {
    let measure = |t: &str, _bold: bool| t.len() as f64 * 7.0;
    let countdown_measure = |t: &str, _bold: bool| t.len() as f64 * 6.0;
    let mut chart = countdown_chart();
    let base =
        chart.optimal_price_axis_width_for(PriceScaleTarget::Right, measure, countdown_measure);
    // A crosshair at a wide price (its label is much longer than any tick) must NOT widen the
    // strip: it is transient chrome, and the grow-fast/shrink-lazy policy would pin the
    // inflated width forever, leaving a permanent dead zone beside the last-value chips.
    chart.crosshair = Some((400.0, 250.0));
    let with_crosshair =
        chart.optimal_price_axis_width_for(PriceScaleTarget::Right, measure, countdown_measure);
    assert_eq!(
        with_crosshair, base,
        "crosshair label must not inflate the strip"
    );
    chart.crosshair = None;
}

/// Axis clean-rebuild equality: an incremental axis frame must equal a forced full rebuild.
fn assert_axis_frame_matches_clean_rebuild(chart: &mut ChartEngine) {
    let measure = |t: &str, _bold: bool| t.len() as f64 * 7.0;
    let countdown_measure = |t: &str, _bold: bool| t.len() as f64 * 6.0;
    let incremental = chart.build_axis_frame(80.0, measure, countdown_measure);
    chart.retained_frame = RetainedFrame::default();
    chart.frame_invalidation.all();
    let rebuilt = chart.build_axis_frame(80.0, measure, countdown_measure);
    assert_eq!(incremental, rebuilt);
}

#[test]
fn compact_axis_fixture_strips_and_tags_share_metrics_across_dpr() {
    // One deterministic viewport/data/font fixture covering every compact label family: tick
    // labels, live-price + title + countdown cluster, and crosshair price + time tags.
    let build_labels = |dpr: f64| {
        let mut chart = countdown_chart();
        chart.dpr = dpr;
        chart.series[0].title = "NDQ".to_string();
        chart.series[0].countdown_visible = true;
        chart.now_override = Some(250.0); // "00:50"
        // Mid-pane crosshair so both crosshair tags render alongside live-price titles.
        let mid_x = chart.logical_to_coordinate(2.0).unwrap_or(400.0);
        chart.crosshair = Some((mid_x, 250.0));
        chart.recompute_layout_with_measure(
            true,
            |t, _bold| t.len() as f64 * 7.0,
            |t, _bold| t.len() as f64 * 6.0,
        );
        chart.build_frame();
        assert_eq!(chart.time_axis_height(), 22.0);
        assert_eq!(chart.axis_w % 2.0, 0.0, "price strip stays even-snapped");
        let labels = chart
            .build_axis_frame(
                80.0,
                |t, _bold| t.len() as f64 * 7.0,
                |t, _bold| t.len() as f64 * 6.0,
            )
            .labels;
        assert_axis_frame_matches_clean_rebuild(&mut chart);
        assert_retained_frame_matches_clean_rebuild(&mut chart);
        (chart, labels)
    };
    let (chart_dpr1, labels_dpr1) = build_labels(1.0);
    let (_, labels_dpr2) = build_labels(2.0);
    assert_eq!(
        labels_dpr1, labels_dpr2,
        "axis label geometry is CSS-px: DPR must not move it"
    );

    // Every axis-attached glyph runs at 11/12 of layout.fontSize; countdown at 10/12.
    for label in &labels_dpr1 {
        let expected = if label.text == "00:50" {
            10.0 / 12.0
        } else {
            11.0 / 12.0
        };
        assert_eq!(label.font_scale, expected, "scale for {:?}", label.text);
    }
    // Boxed price-side tags share one horizontal chrome: 12px around the measured advance.
    // Ordinary tags remain 15px high; only the transient crosshair Y tag is 19px.
    for label in labels_dpr1.iter().filter(|l| {
        l.background.is_some()
            && l.midpoint == AxisTextMidpoint::Label
            && !l.text.is_empty()
            && l.align != AxisTextAlign::Center
            && l.text != "00:50"
    }) {
        let (_, _, w, h, background) = label.background.expect("boxed");
        assert_eq!(w, 12.0 + label.text.len() as f64 * 7.0);
        assert_eq!(
            h,
            if background == CROSSHAIR_LABEL_BG {
                19.0
            } else {
                15.0
            }
        );
    }
    // Countdown rows share the price width at 14px height.
    let countdown = labels_dpr1
        .iter()
        .find(|l| l.text == "00:50")
        .expect("countdown tag");
    let (_, _, _, countdown_h, _) = countdown.background.expect("boxed countdown");
    assert_eq!(countdown_h, 14.0);
    // Time-side tags fit the 22px strip with 6px padding per side.
    for label in labels_dpr1.iter().filter(|l| {
        l.background.is_some()
            && (l.midpoint == AxisTextMidpoint::StableTime || l.midpoint == AxisTextMidpoint::None)
    }) {
        let (_, y, w, h, _) = label.background.expect("boxed time tag");
        assert_eq!(y, chart_dpr1.pane_h);
        assert_eq!(h, 22.0);
        assert_eq!(w, label.text.len() as f64 * 7.0 + 12.0);
    }
    // Crosshair price + time tags render together with live-price and countdown.
    assert!(labels_dpr1.iter().any(|l| l.text == "12.50"));
    assert!(labels_dpr1.iter().any(|l| l.text == "00:50"));
    assert!(
        labels_dpr1
            .iter()
            .any(|l| { l.background.is_some() && l.midpoint == AxisTextMidpoint::StableTime }),
        "crosshair time tag must render"
    );
}

#[test]
fn streaming_digit_growth_widens_the_strip_without_shrink_on_repaint() {
    let axis7 = |t: &str, _bold: bool| t.len() as f64 * 7.0;
    let countdown6 = |t: &str, _bold: bool| t.len() as f64 * 6.0;
    let mut chart = countdown_chart();
    chart.recompute_layout_with_measure(true, axis7, countdown6);
    chart.build_frame();
    let narrow = chart.axis_w;

    // A streaming wide bar (digit-width growth) flags layout; repaint negotiation grows.
    assert!(chart.update_series_bar(0, 300.0, [1_234.0, 1_240.0, 1_230.0, 1_235.0]));
    assert!(chart.frame_requires_layout());
    chart.recompute_layout_with_measure(false, axis7, countdown6);
    chart.build_frame();
    assert!(
        chart.axis_w > narrow,
        "wider labels must grow the strip: {narrow} -> {}",
        chart.axis_w
    );
    let grown = chart.axis_w;

    // Narrow data again: repaints never breathe the strip smaller, full layouts do.
    chart
        .set_series_data(
            0,
            &[0.0, 60.0, 120.0, 180.0, 240.0],
            &[10.0, 11.0, 12.0, 11.5, 12.5],
            &[10.0, 11.0, 12.0, 11.5, 12.5],
            &[10.0, 11.0, 12.0, 11.5, 12.5],
            &[10.0, 11.0, 12.0, 11.5, 12.5],
        )
        .unwrap();
    chart.recompute_layout_with_measure(false, axis7, countdown6);
    chart.build_frame();
    assert_eq!(chart.axis_w, grown, "repaints must not shrink the strip");
    chart.recompute_layout_with_measure(true, axis7, countdown6);
    chart.build_frame();
    assert_eq!(chart.axis_w, narrow, "full layout releases the width");
}

#[test]
fn countdown_only_cluster_centers_on_the_value_coordinate() {
    let mut chart = countdown_chart();
    chart.now_override = Some(250.0);
    chart.series[0].last_value_visible = false;
    chart.series[0].title_visible = false;
    chart.series[0].countdown_visible = true;
    let labels = boxed_labels(&mut chart);
    assert_eq!(labels.len(), 1, "countdown-only cluster is one chip");
    let chip = &labels[0];
    assert_eq!(chip.text, "00:50");
    // No top row to hang from: the single countdown chip centers on the value coordinate
    // (previously it hung BELOW the line by half the countdown row).
    let scale = &chart.panes[0].price_scale;
    let expected_y = scale.price_to_coordinate(12.5, 10.0);
    assert!(
        (chip.y - expected_y).abs() < 1e-9,
        "countdown-only chip must center on the value: y={} expected={expected_y}",
        chip.y
    );
}

#[test]
fn last_value_cluster_overlap_resolution_uses_the_total_height() {
    let mut chart = two_identical_line_series();
    chart.now_override = Some(12.0);
    chart.series[0].countdown_visible = true;
    chart.series[1].countdown_visible = true;
    let labels = chart
        .build_axis_frame(
            80.0,
            |t, _bold| t.len() as f64 * 7.0,
            |t, _bold| t.len() as f64 * 6.0,
        )
        .labels;
    // Two colliding two-row clusters: the overlap pass pushes them apart by each cluster's
    // total height (15px price row + 14px countdown row), measured between the price rows.
    let cluster_height = (11.0 + 2.0 * 2.0) + (10.0 + 2.0 * 2.0);
    let mut price_ys: Vec<f64> = labels
        .iter()
        .filter(|l| l.background.is_some() && l.text == "12.50")
        .map(|l| l.y)
        .collect();
    price_ys.sort_by(|a, b| a.total_cmp(b));
    assert_eq!(price_ys.len(), 2);
    let gap = price_ys[1] - price_ys[0];
    assert!(
        (gap - cluster_height).abs() < 1e-9,
        "two-row clusters must be pushed a cluster height apart, got {gap}"
    );
}

#[test]
fn mixed_height_series_clusters_do_not_overlap() {
    let mut chart = two_identical_line_series();
    chart.now_override = Some(12.0);
    chart.series[1].countdown_visible = true;
    let labels = chart
        .build_axis_frame(
            80.0,
            |t, _| t.len() as f64 * 7.0,
            |t, _| t.len() as f64 * 6.0,
        )
        .labels;
    let live: Vec<_> = labels
        .iter()
        .filter(|label| label.text == "12.50" && label.background.is_some())
        .collect();
    assert_eq!(live.len(), 2);
    let countdown = labels.iter().find(|label| label.text == "04:48").unwrap();
    let plain_box = live[0].background.unwrap();
    let countdown_box = countdown.background.unwrap();
    assert!(
        plain_box.1 + plain_box.3 <= countdown_box.1
            || countdown_box.1 + countdown_box.3 <= plain_box.1,
        "a taller indicator cluster must clear the plain live-price label"
    );
}

/// One candle series on 60s bars (light default background) for the selection-anchor tests.
fn anchor_chart() -> ChartEngine {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times = [0.0, 60.0, 120.0, 180.0, 240.0];
    let opens = [10.0, 11.0, 12.0, 11.5, 12.5];
    let highs = [10.5, 11.5, 12.5, 12.0, 13.0];
    let lows = [9.5, 10.5, 11.5, 11.0, 12.0];
    let closes = [11.0, 12.0, 11.5, 12.5, 12.8];
    chart
        .set_series_data(0, &times, &opens, &highs, &lows, &closes)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart
}

#[test]
fn profile_and_anchored_vwap_drawings_lower_into_the_shared_frame() {
    use crate::{DrawingKind, DrawingPoint, ProfileDrawingOptions, ProfileSource, SeriesKind};

    let mut chart = anchor_chart();
    let volume = chart.add_series(SeriesKind::Histogram);
    chart
        .set_series_data(
            volume,
            &[0.0, 60.0, 120.0, 180.0, 240.0],
            &[1.0, 2.0, 3.0, 4.0, 5.0],
            &[1.0, 2.0, 3.0, 4.0, 5.0],
            &[1.0, 2.0, 3.0, 4.0, 5.0],
            &[1.0, 2.0, 3.0, 4.0, 5.0],
        )
        .unwrap();
    chart.set_series_visible(volume, false);
    let options = ProfileDrawingOptions {
        source: ProfileSource::Candles {
            price_series: 0,
            volume_series: volume,
        },
        tick_size: 0.5,
        row_count: 16,
        value_area_percent: 70.0,
        band_multiplier: 1.0,
        width_percent: 30.0,
    };
    let fixed = chart
        .add_drawing(
            DrawingKind::FixedRangeVolumeProfile,
            0,
            vec![
                DrawingPoint {
                    logical: 0.0,
                    price: 10.0,
                },
                DrawingPoint {
                    logical: 4.0,
                    price: 10.0,
                },
            ],
            None,
        )
        .unwrap();
    chart
        .configure_profile_drawing(fixed, options.clone())
        .unwrap();
    let profile_frame = chart.build_frame();
    assert!(profile_frame.panes[0].main.iter().any(|prim| matches!(prim,
        Prim::HLine { color, .. } if *color == crate::profile_drawings::PROFILE_POC_COLOR
    )));

    let anchored = chart
        .add_drawing(
            DrawingKind::AnchoredVwap,
            0,
            vec![DrawingPoint {
                logical: 0.0,
                price: 10.0,
            }],
            None,
        )
        .unwrap();
    chart.configure_profile_drawing(anchored, options).unwrap();
    let vwap_frame = chart.build_frame();
    assert!(vwap_frame.panes[0].main.len() > profile_frame.panes[0].main.len());
    chart.build_frame();
    assert_eq!(chart.frame_build_stats(), FrameBuildStats::default());
    assert!(chart.update_series_bar(volume, 240.0, [50.0; 4]));
    let corrected = chart.build_frame();
    assert_eq!(chart.frame_build_stats().drawing_rebuilds, 1);
    assert_ne!(corrected.panes[0].main, vwap_frame.panes[0].main);

    let unrelated = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(
            unrelated,
            &[0.0, 60.0, 120.0, 180.0, 240.0],
            &[10.0; 5],
            &[10.0; 5],
            &[10.0; 5],
            &[10.0; 5],
        )
        .unwrap();
    chart.build_frame();
    chart.build_frame();
    assert!(chart.update_series_bar(unrelated, 240.0, [11.0; 4]));
    chart.build_frame();
    assert_eq!(chart.frame_build_stats().drawing_rebuilds, 0);
}

#[test]
fn periodic_profile_presentation_uses_series_layer_and_rebuilds_on_volume_correction() {
    use crate::{
        PeriodicProfilePresentationOptions, PeriodicProfilePresentationRequest, ProfileSource,
        ResampleBoundary, SeriesKind,
    };

    let mut chart = anchor_chart();
    let volume = chart.add_series(SeriesKind::Histogram);
    let times = [0.0, 60.0, 120.0, 180.0, 240.0];
    chart
        .set_series_data(volume, &times, &[1.0; 5], &[1.0; 5], &[1.0; 5], &[1.0; 5])
        .unwrap();
    chart.set_series_visible(volume, false);
    let request = PeriodicProfilePresentationRequest {
        source: ProfileSource::Candles {
            price_series: 0,
            volume_series: volume,
        },
        boundaries: vec![ResampleBoundary {
            start_time: 0,
            end_time: 300,
            session_id: 1,
        }],
        tick_size: 0.5,
        row_count: 16,
        value_area_percent: 70.0,
    };
    let id = chart
        .add_periodic_profile_presentation(
            0,
            request,
            PeriodicProfilePresentationOptions::default(),
        )
        .unwrap();
    let first = chart.build_frame();
    assert!(first.panes[0].main.iter().any(|prim| matches!(prim,
        Prim::HLine { color, .. } if *color == Color::rgb(245, 166, 35)
    )));
    chart.build_frame();
    assert_eq!(chart.frame_build_stats(), FrameBuildStats::default());
    assert!(chart.update_series_bar(volume, 240.0, [20.0; 4]));
    let corrected = chart.build_frame();
    assert_eq!(chart.frame_build_stats().drawing_rebuilds, 0);
    assert_ne!(corrected.panes[0].main, first.panes[0].main);
    assert!(chart.remove_native_primitive(id));
    let removed = chart.build_frame();
    assert!(!removed.panes[0].main.iter().any(|prim| matches!(prim,
        Prim::HLine { color, .. } if *color == Color::rgb(245, 166, 35)
    )));
}

#[test]
fn candle_periodic_profile_traces_developing_value_area() {
    use crate::{
        PeriodicProfilePresentationOptions, PeriodicProfilePresentationRequest, ProfileRequest,
        ProfileSource, ResampleBoundary, SeriesKind,
    };

    let mut chart = anchor_chart();
    let volume = chart.add_series(SeriesKind::Histogram);
    let times = [0.0, 60.0, 120.0, 180.0, 240.0];
    chart
        .set_series_data(volume, &times, &[1.0; 5], &[1.0; 5], &[1.0; 5], &[1.0; 5])
        .unwrap();
    chart.set_series_visible(volume, false);
    let source = ProfileSource::Candles {
        price_series: 0,
        volume_series: volume,
    };
    let snapshot = chart
        .volume_profile_snapshot(&ProfileRequest {
            source,
            start_timestamp_micros: 0,
            end_timestamp_micros: 300_000_000,
            tick_size: 0.5,
            row_count: 16,
            value_area_percent: 70.0,
        })
        .unwrap();
    assert_eq!(snapshot.developing.len(), 5);
    assert_eq!(
        snapshot.developing.last().unwrap().poc,
        snapshot.poc.unwrap()
    );
    chart
        .add_periodic_profile_presentation(
            0,
            PeriodicProfilePresentationRequest {
                source,
                boundaries: vec![ResampleBoundary {
                    start_time: 0,
                    end_time: 300,
                    session_id: 1,
                }],
                tick_size: 0.5,
                row_count: 16,
                value_area_percent: 70.0,
            },
            PeriodicProfilePresentationOptions {
                show_developing: true,
                ..Default::default()
            },
        )
        .unwrap();
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|prim| matches!(prim,
        Prim::Polyline { color, point_count, .. }
        if *color == Color::rgb(245, 166, 35) && *point_count == 5
    )));
}

#[test]
fn periodic_profile_presentation_reserves_bounded_rows() {
    use crate::{
        FootprintAggregationOptions, PeriodicProfilePresentationOptions,
        PeriodicProfilePresentationRequest, ProfileError, ProfileSource, ResampleBoundary,
    };

    let mut chart = anchor_chart();
    let request = PeriodicProfilePresentationRequest {
        source: ProfileSource::Candles {
            price_series: 0,
            volume_series: 0,
        },
        boundaries: (0..17)
            .map(|index| ResampleBoundary {
                start_time: index * 60,
                end_time: index * 60 + 60,
                session_id: index as u64,
            })
            .collect(),
        tick_size: 0.5,
        row_count: 2_048,
        value_area_percent: 70.0,
    };
    assert_eq!(
        chart.add_periodic_profile_presentation(
            0,
            request,
            PeriodicProfilePresentationOptions::default()
        ),
        Err(ProfileError::LimitExceeded),
    );
    let stream_id = chart
        .add_trade_stream("bounded-tape", FootprintAggregationOptions::default())
        .unwrap();
    let mut tape_request = PeriodicProfilePresentationRequest {
        source: ProfileSource::Tape { stream_id },
        boundaries: (0..17)
            .map(|index| ResampleBoundary {
                start_time: index * 60,
                end_time: index * 60 + 60,
                session_id: index as u64,
            })
            .collect(),
        tick_size: 1.0,
        row_count: 1,
        value_area_percent: 70.0,
    };
    assert_eq!(
        chart.add_periodic_profile_presentation(
            0,
            tape_request.clone(),
            PeriodicProfilePresentationOptions::default()
        ),
        Err(ProfileError::LimitExceeded),
    );
    for boundary in &mut tape_request.boundaries {
        boundary.session_id = 1;
    }
    assert!(
        chart
            .add_periodic_profile_presentation(
                0,
                tape_request,
                PeriodicProfilePresentationOptions::default(),
            )
            .is_ok()
    );
}

#[test]
fn removing_periodic_candle_dependency_releases_presentation() {
    use crate::{
        PeriodicProfilePresentationOptions, PeriodicProfilePresentationRequest, ProfileSource,
        ResampleBoundary, SeriesKind,
    };

    let mut chart = anchor_chart();
    let volume = chart.add_series(SeriesKind::Histogram);
    chart
        .set_series_data(volume, &[0.0], &[1.0], &[1.0], &[1.0], &[1.0])
        .unwrap();
    let id = chart
        .add_periodic_profile_presentation(
            0,
            PeriodicProfilePresentationRequest {
                source: ProfileSource::Candles {
                    price_series: 0,
                    volume_series: volume,
                },
                boundaries: vec![ResampleBoundary {
                    start_time: 0,
                    end_time: 60,
                    session_id: 1,
                }],
                tick_size: 1.0,
                row_count: 8,
                value_area_percent: 70.0,
            },
            PeriodicProfilePresentationOptions::default(),
        )
        .unwrap();
    assert!(chart.remove_series(volume));
    assert!(!chart.remove_native_primitive(id));
}

#[test]
fn periodic_profile_naked_level_extends_to_first_later_tape_touch() {
    use crate::{
        AggressorSide, FootprintAggregationOptions, FootprintTrade,
        PeriodicProfilePresentationOptions, PeriodicProfilePresentationRequest, ProfileSource,
        ResampleBoundary,
    };

    let mut chart = anchor_chart();
    let stream_id = chart
        .add_trade_stream(
            "profile-extension",
            FootprintAggregationOptions {
                tick_size: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
    let trade = |timestamp_micros| FootprintTrade {
        timestamp_micros,
        price: 10.0,
        volume: 1.0,
        aggressor: AggressorSide::Buy,
        bid: None,
        ask: None,
        sequence: None,
        trade_id: None,
        conditions: 0,
        session_id: Some(1),
    };
    chart
        .set_trade_stream_trades(
            stream_id,
            vec![trade(0), trade(30_000_000), trade(120_000_000)],
        )
        .unwrap();
    let request = PeriodicProfilePresentationRequest {
        source: ProfileSource::Tape { stream_id },
        boundaries: vec![ResampleBoundary {
            start_time: 0,
            end_time: 60,
            session_id: 1,
        }],
        tick_size: 1.0,
        row_count: 8,
        value_area_percent: 70.0,
    };
    chart
        .add_periodic_profile_presentation(
            0,
            request,
            PeriodicProfilePresentationOptions {
                extend_naked_levels: true,
                show_developing: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(chart.remove_trade_stream(stream_id).is_err());
    let with_touch = chart.build_frame();
    assert!(with_touch.panes[0].main.iter().any(|prim| matches!(prim,
        Prim::Polyline { color, point_count, .. }
        if *color == Color::rgb(245, 166, 35) && *point_count == 2
    )));
    let poc_lines = |frame: &crate::ChartFrame| {
        frame.panes[0]
            .main
            .iter()
            .filter_map(|prim| match prim {
                Prim::HLine { x0, x1, color, .. } if *color == Color::rgb(245, 166, 35) => {
                    Some((*x0, *x1))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let lines = poc_lines(&with_touch);
    assert_eq!(lines.len(), 2);
    assert!(lines[1].1 > lines[1].0);
    chart
        .set_trade_stream_trades(stream_id, vec![trade(0)])
        .unwrap();
    let untouched = chart.build_frame();
    let extended = poc_lines(&untouched);
    assert!(extended[1].1 > lines[1].1);
}

#[test]
fn tape_profile_drawing_rebuilds_without_a_footprint_series() {
    use crate::{
        AggressorSide, DrawingKind, DrawingPoint, FootprintAggregationOptions, FootprintError,
        FootprintTrade, ProfileDrawingOptions, ProfileSource,
    };

    let mut chart = anchor_chart();
    let stream = chart
        .add_trade_stream(
            "profile-only",
            FootprintAggregationOptions {
                tick_size: 0.5,
                ..Default::default()
            },
        )
        .unwrap();
    let trade = |timestamp_micros, price, volume| FootprintTrade {
        timestamp_micros,
        price,
        volume,
        aggressor: AggressorSide::Buy,
        bid: None,
        ask: None,
        sequence: None,
        trade_id: None,
        conditions: 0,
        session_id: Some(1),
    };
    chart
        .set_trade_stream_trades(
            stream,
            vec![trade(0, 10.0, 1.0), trade(60_000_000, 11.0, 2.0)],
        )
        .unwrap();
    let drawing = chart
        .add_drawing(
            DrawingKind::FixedRangeVolumeProfile,
            0,
            vec![
                DrawingPoint {
                    logical: 0.0,
                    price: 10.0,
                },
                DrawingPoint {
                    logical: 4.0,
                    price: 10.0,
                },
            ],
            None,
        )
        .unwrap();
    chart
        .configure_profile_drawing(
            drawing,
            ProfileDrawingOptions {
                source: ProfileSource::Tape { stream_id: stream },
                tick_size: 0.5,
                row_count: 16,
                value_area_percent: 70.0,
                band_multiplier: 1.0,
                width_percent: 30.0,
            },
        )
        .unwrap();
    let before = chart.build_frame();
    chart.build_frame();
    assert_eq!(chart.frame_build_stats(), FrameBuildStats::default());
    assert!(matches!(
        chart.remove_trade_stream(stream),
        Err(FootprintError::TradeStreamInUse(_))
    ));
    chart
        .update_trade_stream_trade(stream, trade(120_000_000, 12.0, 8.0))
        .unwrap();
    let after = chart.build_frame();
    assert_eq!(chart.frame_build_stats().drawing_rebuilds, 1);
    assert_ne!(after.panes[0].main, before.panes[0].main);
}

#[test]
fn tpo_letters_and_blocks_follow_the_source_in_the_shared_frame() {
    use crate::{ResampleBoundary, TpoCellMode, TpoPresentationOptions, TpoRequest};

    let mut chart = anchor_chart();
    let request = TpoRequest {
        price_series: 0,
        boundaries: vec![ResampleBoundary {
            start_time: 0,
            end_time: 300,
            session_id: 1,
        }],
        period_seconds: 60,
        tick_size: 0.5,
        value_area_percent: 70.0,
        initial_balance_periods: 2,
    };
    let letters = chart
        .add_tpo_presentation(request.clone(), TpoPresentationOptions::default())
        .unwrap();
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|prim| matches!(prim,
        Prim::Text { text, .. } if text == "A"
    )));
    assert!(frame.panes[0].main.iter().any(|prim| matches!(prim,
        Prim::HLine { color, .. } if *color == Color::rgb(245, 166, 35)
    )));
    chart.build_frame();
    assert_eq!(chart.frame_build_stats(), FrameBuildStats::default());
    assert!(chart.update_series_bar(0, 240.0, [12.5, 14.0, 12.0, 13.5]));
    let corrected = chart.build_frame();
    assert_eq!(chart.frame_build_stats().series_rebuilds, 1);
    assert_ne!(frame.panes[0].main, corrected.panes[0].main);
    assert!(chart.remove_native_primitive(letters));

    let blocks = chart
        .add_tpo_presentation(
            request,
            TpoPresentationOptions {
                mode: TpoCellMode::Blocks,
                ..Default::default()
            },
        )
        .unwrap();
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|prim| matches!(prim,
        Prim::Rect { color, .. } if *color == Color::rgb(51, 92, 255)
    )));
    assert!(chart.remove_native_primitive(blocks));
}

#[test]
fn dense_tpo_collapses_to_viewport_rows_instead_of_emitting_every_cell() {
    use crate::{ResampleBoundary, TpoPresentationOptions, TpoRequest};

    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times = (0..1_024).map(f64::from).collect::<Vec<_>>();
    chart
        .set_series_data(
            0,
            &times,
            &vec![15.0; times.len()],
            &vec![20.0; times.len()],
            &vec![10.0; times.len()],
            &vec![15.0; times.len()],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart
        .add_tpo_presentation(
            TpoRequest {
                price_series: 0,
                boundaries: vec![ResampleBoundary {
                    start_time: 0,
                    end_time: 1_024,
                    session_id: 1,
                }],
                period_seconds: 1,
                tick_size: 1.0,
                value_area_percent: 70.0,
                initial_balance_periods: 2,
            },
            TpoPresentationOptions {
                color: "#ff00ff".into(),
                value_area_color: "#ff00ff".into(),
                single_print_color: "#ff00ff".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let frame = chart.build_frame();
    let compact = frame.panes[0]
        .main
        .iter()
        .filter(|prim| {
            matches!(prim,
                Prim::Rect { color, .. } if *color == Color::rgb(255, 0, 255)
            )
        })
        .count();
    assert!(
        (1..=11).contains(&compact),
        "dense TPO emitted {compact} blocks"
    );
}

#[test]
fn trend_line_middle_label_follows_the_segment_and_opens_a_gap() {
    use crate::{DrawingKind, DrawingPoint};

    let mut chart = anchor_chart();
    let id = chart
        .add_drawing(
            DrawingKind::TrendLine,
            0,
            vec![
                DrawingPoint {
                    logical: 0.0,
                    price: 10.0,
                },
                DrawingPoint {
                    logical: 4.0,
                    price: 12.5,
                },
            ],
            None,
        )
        .unwrap();
    assert!(chart.drawing_apply_options(
        id,
        r##"{"color":"#ff9900","text":"Middle center","text_color":"#ff9900","text_h_align":"center","text_v_align":"middle"}"##,
    ));

    let frame = chart.build_frame();
    let pane = &frame.panes[0];
    let label = pane
        .main
        .iter()
        .find_map(|prim| match prim {
            Prim::RotatedText { x, y, text, .. } if text == "Middle center" => Some((*x, *y)),
            _ => None,
        })
        .expect("trend label");
    let orange_runs = pane
        .main
        .iter()
        .filter_map(|prim| match prim {
            Prim::Polyline {
                first_point,
                point_count,
                color,
                ..
            } if *color == Color::rgb(0xff, 0x99, 0x00) => {
                Some(&pane.points[*first_point as usize..(*first_point + *point_count) as usize])
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(orange_runs.len(), 2, "the label must split the segment");
    assert!(orange_runs.iter().all(|run| run.len() == 2));
    let line_mid = {
        let a = orange_runs[0][0];
        let b = orange_runs[1][1];
        [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
    };
    assert!((label.0 - line_mid[0]).abs() < 0.01);
    assert!((label.1 - line_mid[1]).abs() < 0.01);
    assert!(orange_runs[0][1][0] < label.0);
    assert!(orange_runs[1][0][0] > label.0);
}

#[test]
fn every_line_kind_runs_its_middle_label_along_the_line_and_opens_a_gap() {
    use crate::{DrawingKind, DrawingPoint};

    let ink = Color::rgb(0xff, 0x99, 0x00);
    for kind in [
        DrawingKind::Ray,
        DrawingKind::ExtendedLine,
        DrawingKind::HorizontalLine,
        DrawingKind::HorizontalRay,
        DrawingKind::VerticalLine,
    ] {
        let mut chart = anchor_chart();
        let points = [
            DrawingPoint {
                logical: 1.0,
                price: 10.0,
            },
            DrawingPoint {
                logical: 4.0,
                price: 12.5,
            },
        ];
        let id = chart
            .add_drawing(kind, 0, points[..kind.anchor_count()].to_vec(), None)
            .unwrap();
        // No text color: a line label takes its line's ink.
        assert!(chart.drawing_apply_options(
            id,
            r##"{"color":"#ff9900","text":"Pivot","text_h_align":"center","text_v_align":"middle"}"##,
        ));
        let frame = chart.build_frame();
        let pane = &frame.panes[0];
        let (label_x, label_y, label_color) = pane
            .main
            .iter()
            .find_map(|prim| match prim {
                Prim::RotatedText {
                    x, y, text, color, ..
                } if text == "Pivot" => Some((f64::from(*x), f64::from(*y), *color)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{kind:?}: the label runs along the line"));
        assert_eq!(label_color, ink, "{kind:?}");
        // Each stroke piece as (start, end) along the line's own axis, label on that axis.
        let (pieces, label_at) = match kind {
            DrawingKind::HorizontalLine | DrawingKind::HorizontalRay => (
                pane.main
                    .iter()
                    .filter_map(|prim| match prim {
                        Prim::HLine { x0, x1, color, .. } if *color == ink => {
                            Some((f64::from(*x0), f64::from(*x1)))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                label_x,
            ),
            DrawingKind::VerticalLine => (
                pane.main
                    .iter()
                    .filter_map(|prim| match prim {
                        Prim::VLine { y0, y1, color, .. } if *color == ink => {
                            Some((f64::from(*y0), f64::from(*y1)))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                label_y,
            ),
            _ => (
                pane.main
                    .iter()
                    .filter_map(|prim| match prim {
                        Prim::Polyline {
                            first_point,
                            point_count: 2,
                            color,
                            ..
                        } if *color == ink => {
                            let run =
                                &pane.points[*first_point as usize..*first_point as usize + 2];
                            Some((f64::from(run[0][0]), f64::from(run[1][0])))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                label_x,
            ),
        };
        assert_eq!(pieces.len(), 2, "{kind:?}: the label must split its line");
        let (first, second) = (pieces[0], pieces[1]);
        let gap = (first.0.max(first.1), second.0.min(second.1));
        assert!(
            gap.0 < label_at && label_at < gap.1,
            "{kind:?}: label at {label_at} outside the gap {gap:?}"
        );
    }
}

#[test]
fn fibonacci_labels_centered_on_their_levels_open_the_level_lines() {
    use crate::{DrawingKind, DrawingPoint};

    let mut chart = anchor_chart();
    let id = chart
        .add_drawing(
            DrawingKind::FibonacciRetracement,
            0,
            vec![
                DrawingPoint {
                    logical: 0.0,
                    price: 10.0,
                },
                DrawingPoint {
                    logical: 4.0,
                    price: 12.5,
                },
            ],
            None,
        )
        .unwrap();
    // Levels without their own color take the drawing's.
    let ink = Color::rgb(0xff, 0x99, 0x00);
    assert!(chart.drawing_apply_options(id, r##"{"color":"#ff9900"}"##));
    let level_colors = [ink];
    let level_runs = |chart: &mut ChartEngine| {
        let frame = chart.build_frame();
        let pane = &frame.panes[0];
        let labels = pane
            .main
            .iter()
            .filter_map(|prim| match prim {
                Prim::Text { x, y, color, .. } if level_colors.contains(color) => {
                    Some((f64::from(*x), y.round() as i32, *color))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        labels
            .into_iter()
            .map(|(x, y, label_color)| {
                let runs = pane
                    .main
                    .iter()
                    .filter_map(|prim| match prim {
                        Prim::HLine {
                            y: line_y,
                            x0,
                            x1,
                            color,
                            ..
                        } if *line_y == y && *color == label_color => {
                            Some((f64::from(*x0), f64::from(*x1)))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                (x, runs)
            })
            .collect::<Vec<_>>()
    };

    assert!(chart.drawing_apply_options(
        id,
        r#"{"level_label_align":"center","level_label_v_align":"middle"}"#,
    ));
    let centered = level_runs(&mut chart);
    assert!(centered.len() >= 2, "every visible level paints its label");
    for (label_x, runs) in &centered {
        assert_eq!(runs.len(), 2, "a centered label splits its level: {runs:?}");
        assert!(runs[0].1 < *label_x && *label_x < runs[1].0, "{runs:?}");
    }

    // A label above its level leaves the level whole.
    assert!(chart.drawing_apply_options(id, r#"{"level_label_v_align":"top"}"#));
    let frame = chart.build_frame();
    let whole = frame.panes[0]
        .main
        .iter()
        .filter(|prim| matches!(prim, Prim::HLine { color, .. } if level_colors.contains(color)))
        .count();
    assert_eq!(whole, centered.len(), "one unbroken line per level");
}

#[test]
fn tilted_trend_label_keeps_its_line_box_clear_of_the_stroke() {
    use crate::{DrawingKind, DrawingPoint};

    let mut chart = anchor_chart();
    let id = chart
        .add_drawing(
            DrawingKind::TrendLine,
            0,
            vec![
                DrawingPoint {
                    logical: 0.0,
                    price: 10.0,
                },
                DrawingPoint {
                    logical: 4.0,
                    price: 10.4,
                },
            ],
            None,
        )
        .unwrap();
    chart.build_frame();
    let start = chart.drawing_point_to_coordinate(id, 0).unwrap();
    let end = chart.drawing_point_to_coordinate(id, 1).unwrap();
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let length = dx.hypot(dy);
    let top_normal = (dy / length, -dx / length);

    for (slot, sign) in [("top", 1.0), ("bottom", -1.0)] {
        assert!(chart.drawing_apply_options(
            id,
            &format!(r#"{{"text":"Devraj","text_size":12,"text_v_align":"{slot}"}}"#),
        ));
        let (x, y) = chart.drawing_text_coordinate(id).unwrap();
        let distance = (x - start.0) * top_normal.0 + (y - start.1) * top_normal.1;
        assert!((distance - sign * (4.0 + 12.0 * 0.6)).abs() < 0.001);
    }
}

#[test]
fn trend_label_color_follows_its_line_until_explicitly_overridden() {
    use crate::{DrawingKind, DrawingPoint};

    fn label_color(chart: &mut ChartEngine) -> Color {
        chart.build_frame().panes[0]
            .main
            .iter()
            .find_map(|prim| match prim {
                Prim::RotatedText { text, color, .. } if text == "label" => Some(*color),
                _ => None,
            })
            .expect("trend label")
    }

    let mut chart = anchor_chart();
    let id = chart
        .add_drawing(
            DrawingKind::TrendLine,
            0,
            vec![
                DrawingPoint {
                    logical: 0.0,
                    price: 10.0,
                },
                DrawingPoint {
                    logical: 4.0,
                    price: 12.5,
                },
            ],
            Some(r##"{"color":"#123456","text":"label"}"##),
        )
        .unwrap();

    assert_eq!(label_color(&mut chart), Color::rgb(0x12, 0x34, 0x56));
    assert!(chart.drawing_apply_options(id, r##"{"color":"#abcdef"}"##));
    assert_eq!(label_color(&mut chart), Color::rgb(0xab, 0xcd, 0xef));

    assert!(chart.drawing_apply_options(id, r##"{"text_color":"#d32f2f"}"##));
    assert!(chart.drawing_apply_options(id, r##"{"color":"#00aa88"}"##));
    assert_eq!(
        label_color(&mut chart),
        Color::rgb(0xd3, 0x2f, 0x2f),
        "an explicit label color must remain independent of later line-color changes"
    );
}

#[test]
fn rotated_trend_label_hit_test_uses_label_local_coordinates() {
    use crate::{DrawingKind, DrawingPoint};

    let mut chart = anchor_chart();
    let id = chart
        .add_drawing(
            DrawingKind::TrendLine,
            0,
            vec![
                DrawingPoint {
                    logical: 0.0,
                    price: 10.0,
                },
                DrawingPoint {
                    logical: 4.0,
                    price: 12.5,
                },
            ],
            Some(r#"{"text":"rotated label","text_h_align":"center","text_v_align":"middle"}"#),
        )
        .unwrap();
    chart.build_frame();
    let (x, y, angle) = chart.drawing_text_transform(id).unwrap();
    assert!(angle.abs() > 0.25, "fixture must exercise a rotated run");
    let along = 42.0;
    assert_eq!(
        chart.drawing_text_hit_at(x + angle.cos() * along, y + angle.sin() * along),
        Some(id)
    );
    assert_eq!(
        chart.drawing_text_hit_at(x + along, y),
        None,
        "the obsolete screen-horizontal location must miss"
    );
}

#[test]
fn native_text_session_paints_a_rotated_caret_that_follows_the_typed_run() {
    use crate::{DrawingKind, DrawingPoint, DrawingTextEditKey};

    // The caret is the one polyline in the label's ink that is not a line segment.
    fn caret(chart: &mut ChartEngine) -> Option<[[f32; 2]; 2]> {
        let frame = chart.build_frame();
        let pane = &frame.panes[0];
        pane.main.iter().find_map(|prim| match prim {
            Prim::Polyline {
                first_point,
                point_count: 2,
                color,
                ..
            } if *color == Color::rgb(0x24, 0x68, 0xac) => {
                let first = *first_point as usize;
                Some([pane.points[first], pane.points[first + 1]])
            }
            _ => None,
        })
    }

    let mut chart = anchor_chart();
    let id = chart
        .add_drawing(
            DrawingKind::TrendLine,
            0,
            vec![
                DrawingPoint {
                    logical: 0.0,
                    price: 10.0,
                },
                DrawingPoint {
                    logical: 4.0,
                    price: 12.5,
                },
            ],
            Some(r##"{"color":"#123456","text_color":"#2468ac","text_h_align":"center"}"##),
        )
        .unwrap();

    // A browser host paints its own caret, so the engine draws none.
    assert!(chart.begin_drawing_text_edit(id, false));
    assert!(caret(&mut chart).is_none());
    assert!(chart.cancel_drawing_text_edit());

    assert!(chart.begin_drawing_text_edit(id, true));
    let empty = caret(&mut chart).expect("native session paints a caret on an empty label");
    assert!(chart.drawing_text_edit_insert("breakout"));
    let end = caret(&mut chart).unwrap();
    let (_, _, angle) = chart.drawing_text_transform(id).unwrap();
    // The caret stands perpendicular to the rotated run and advances along it as text grows.
    let stroke = (end[1][0] - end[0][0], end[1][1] - end[0][1]);
    let along = (angle.cos() as f32, angle.sin() as f32);
    assert!((stroke.0 * along.0 + stroke.1 * along.1).abs() < 1e-3);
    let advance = (end[0][0] - empty[0][0]) * along.0 + (end[0][1] - empty[0][1]) * along.1;
    assert!(
        advance > 1.0,
        "typed text must move the caret along the label"
    );

    assert!(chart.drawing_text_edit_key(DrawingTextEditKey::Home, false));
    let home = caret(&mut chart).unwrap();
    let back = (home[0][0] - end[0][0]) * along.0 + (home[0][1] - end[0][1]) * along.1;
    assert!(
        back < -1.0,
        "Home must move the caret to the start of the run"
    );

    assert!(chart.commit_drawing_text_edit());
    assert!(caret(&mut chart).is_none());
    assert_eq!(chart.drawing(id).unwrap().text, "breakout");
}

#[test]
fn middle_trend_label_gap_shrinks_to_the_caret_then_expands_with_text() {
    use crate::{DrawingKind, DrawingPoint};

    fn orange_segments(chart: &mut ChartEngine) -> Vec<Vec<[f32; 2]>> {
        let frame = chart.build_frame();
        let pane = &frame.panes[0];
        pane.main
            .iter()
            .filter_map(|prim| match prim {
                Prim::Polyline {
                    first_point,
                    point_count,
                    color,
                    ..
                } if *color == Color::rgb(0x12, 0x34, 0x56) => Some(
                    pane.points[*first_point as usize..(*first_point + *point_count) as usize]
                        .to_vec(),
                ),
                _ => None,
            })
            .collect()
    }

    fn gap_width(segments: &[Vec<[f32; 2]>]) -> f32 {
        let left = segments[0].last().unwrap();
        let right = segments[1].first().unwrap();
        (right[0] - left[0]).hypot(right[1] - left[1])
    }

    let mut chart = anchor_chart();
    let id = chart
        .add_drawing(
            DrawingKind::TrendLine,
            0,
            vec![
                DrawingPoint {
                    logical: 0.0,
                    price: 10.0,
                },
                DrawingPoint {
                    logical: 4.0,
                    price: 12.5,
                },
            ],
            Some(
                r##"{"color":"#123456","text_color":"#2468ac","text_h_align":"center","text_v_align":"middle"}"##,
            ),
        )
        .unwrap();
    chart.set_hovered_text(Some(id));
    assert_eq!(
        orange_segments(&mut chart).len(),
        1,
        "hover alone shows no prompt and opens no gap"
    );
    chart.set_selected_drawing(Some(id));
    let hovered = orange_segments(&mut chart);
    assert_eq!(hovered.len(), 2);
    let prompt_color = chart.build_frame().panes[0]
        .main
        .iter()
        .find_map(|prim| match prim {
            Prim::RotatedText { text, color, .. } if text == "+ Add text" => Some(*color),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        (prompt_color.r(), prompt_color.g(), prompt_color.b()),
        (0x24, 0x68, 0xac)
    );
    assert!(prompt_color.a() < 0xff);

    chart.set_editing_drawing(Some(id));
    let editing_empty = orange_segments(&mut chart);
    assert_eq!(editing_empty.len(), 2);
    assert!(
        gap_width(&editing_empty) < gap_width(&hovered),
        "clicking the prompt must shrink its opening to the one-character caret slot"
    );
    assert!(chart.drawing_apply_options(id, r#"{"text":"typing"}"#));
    let typing = orange_segments(&mut chart);
    assert_eq!(typing.len(), 2);
    assert!(
        gap_width(&typing) > gap_width(&editing_empty),
        "measured typed text must expand the opening"
    );
    chart.set_editing_drawing(None);
    chart.set_hovered_text(None);
    chart.set_selected_drawing(None);
    assert_eq!(orange_segments(&mut chart), typing);
}

/// The circle prims in the primary pane's main layer as `(cx, radius, fill)`. With the
/// default options (no pulse, no markers, no crosshair) only selection anchors emit discs.
fn frame_discs(chart: &mut ChartEngine) -> Vec<(f32, f32, Color)> {
    chart.build_frame().panes[0]
        .main
        .iter()
        .filter_map(|prim| match prim {
            Prim::Circle {
                cx, radius, fill, ..
            } => Some((*cx, *radius, *fill)),
            _ => None,
        })
        .collect()
}

fn selection_border_positions(chart: &mut ChartEngine) -> Vec<(f32, f32)> {
    chart.build_frame().panes[0]
        .main
        .iter()
        .filter_map(|prim| match prim {
            Prim::Circle {
                cx,
                cy,
                radius,
                fill,
                ..
            } if *fill == PRIMARY
                && (*radius - (3.0 * chart.dpr + (2.0 * chart.dpr).round()) as f32).abs()
                    < 1e-4 =>
            {
                Some((*cx, *cy))
            }
            _ => None,
        })
        .collect()
}

fn dense_anchor_chart(count: usize) -> ChartEngine {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times: Vec<f64> = (0..count).map(|i| (i * 60) as f64).collect();
    let opens: Vec<f64> = (0..count).map(|i| 10.0 + (i as f64 * 0.05).sin()).collect();
    let highs: Vec<f64> = opens.iter().map(|o| o + 0.5).collect();
    let lows: Vec<f64> = opens.iter().map(|o| o - 0.5).collect();
    let closes: Vec<f64> = opens.iter().map(|o| o + 0.2).collect();
    chart
        .set_series_data(0, &times, &opens, &highs, &lows, &closes)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart
}

#[test]
fn selection_anchors_paint_theme_derived_discs_on_the_selected_series() {
    const BLUE: Color = PRIMARY;
    let mut chart = anchor_chart();
    // Nothing selected: no anchor discs.
    assert!(frame_discs(&mut chart).is_empty());
    chart.set_selected_series(Some(0));
    let discs = frame_discs(&mut chart);
    // One border disc (blue, radius 3 + a 2 px ring at dpr 1) + one fill disc (radius 3) per bar.
    let borders: Vec<_> = discs.iter().copied().filter(|d| d.2 == BLUE).collect();
    let fills: Vec<_> = discs.iter().copied().filter(|d| d.2 != BLUE).collect();
    assert_eq!(borders.len(), 5);
    assert_eq!(fills.len(), 5);
    assert!(borders.iter().all(|d| (d.1 - 5.0).abs() < 1e-4));
    assert!(fills.iter().all(|d| (d.1 - 3.0).abs() < 1e-4));
    // Aeris defaults dark: black fills, each paired with a border disc at the same x.
    assert!(fills.iter().all(|d| d.2 == Color::rgb(0, 0, 0)));
    for fill in &fills {
        assert!(borders.iter().any(|b| (b.0 - fill.0).abs() < 1e-4));
    }
    // Light background: the fill tracks the background luminance to white, blue border stays.
    chart
        .apply_options(r##"{"layout":{"background":{"color":"#ffffff"}}}"##)
        .unwrap();
    let light: Vec<_> = frame_discs(&mut chart);
    let light_fills: Vec<_> = light.iter().copied().filter(|d| d.2 != BLUE).collect();
    assert_eq!(light.iter().filter(|d| d.2 == BLUE).count(), 5);
    assert_eq!(light_fills.len(), 5);
    assert!(
        light_fills
            .iter()
            .all(|d| d.2 == Color::rgb(0xff, 0xff, 0xff))
    );
    // Deselecting (an empty-pane click) removes the anchors.
    chart.set_selected_series(None);
    assert!(frame_discs(&mut chart).is_empty());
}

#[test]
fn host_selection_group_paints_anchors_on_every_output() {
    let mut chart = anchor_chart();
    let second = chart.add_series(SeriesKind::Line);
    let times = [0.0, 60.0, 120.0, 180.0, 240.0];
    let values = [11.0, 12.0, 11.5, 12.5, 12.8];
    chart
        .set_series_data(second, &times, &values, &values, &values, &values)
        .unwrap();
    assert!(chart.set_selected_series_group(second, &[0, second]));
    assert_eq!(
        chart.selected_series_members().collect::<Vec<_>>(),
        vec![0, second]
    );
    assert_eq!(chart.selected_series(), Some(second));
    let discs = frame_discs(&mut chart);
    let borders = discs.iter().filter(|disc| disc.2 == PRIMARY).count();
    assert_eq!(
        borders, 10,
        "five anchors must paint on both outputs: {discs:?}"
    );
}

#[test]
fn text_tool_selection_paints_a_focus_border_without_anchor_handles() {
    use crate::drawings::{DrawingKind, DrawingPoint, TEXT_TOOL_DEFAULT_SIZE};
    use aeris_charts_render::draw_list::IRect;

    // The text chrome is the only RectFrame in the primary token's blue (candle bodies and
    // rectangle tools carry their own colors).
    let is_chrome = |color: Color| color.0 & 0xFFFF_FF00 == PRIMARY.0 & 0xFFFF_FF00;
    let border_frames = |chart: &mut ChartEngine| -> Vec<(IRect, i32, Color)> {
        chart.build_frame().panes[0]
            .main
            .iter()
            .filter_map(|prim| match prim {
                Prim::RectFrame {
                    rect,
                    border,
                    color,
                } if is_chrome(*color) => Some((*rect, *border, *color)),
                _ => None,
            })
            .collect()
    };

    let mut chart = anchor_chart();
    let text = chart
        .add_drawing(
            DrawingKind::Text,
            0,
            vec![DrawingPoint {
                logical: 2.0,
                price: 11.0,
            }],
            Some(r#"{"text":"levels"}"#),
        )
        .unwrap();
    // A trend line guards the anchor path: it must keep its handle discs.
    let line = chart
        .add_drawing(
            DrawingKind::TrendLine,
            0,
            vec![
                DrawingPoint {
                    logical: 0.0,
                    price: 10.0,
                },
                DrawingPoint {
                    logical: 4.0,
                    price: 12.0,
                },
            ],
            None,
        )
        .unwrap();

    // Unselected, unhovered: no chrome at all.
    let initial = border_frames(&mut chart);
    assert!(initial.is_empty(), "unexpected chrome: {initial:?}");

    // A selected empty trend line owns a borderless, low-opacity inline affordance at its
    // configured text slot. Its measured box is also the direct-edit hit target. Hover alone
    // shows nothing and is no edit target.
    chart.set_hovered_text(Some(line));
    assert!(border_frames(&mut chart).is_empty());
    assert_eq!(chart.hovered_text(), Some(line));
    let (text_x, text_y) = chart.drawing_text_coordinate(line).unwrap();
    assert_eq!(chart.drawing_text_hit_at(text_x, text_y), None);
    assert!(
        chart.build_frame().panes[0]
            .main
            .iter()
            .all(|prim| !matches!(prim, Prim::RotatedText { text, .. } if text == "+ Add text"))
    );
    chart.set_selected_drawing(Some(line));
    assert_eq!(chart.drawing_text_hit_at(text_x, text_y), Some(line));
    let placeholder = chart.build_frame().panes[0]
        .main
        .iter()
        .find_map(|prim| match prim {
            Prim::RotatedText { text, color, .. } if text == "+ Add text" => Some(*color),
            _ => None,
        })
        .expect("hovered trend placeholder");
    assert_eq!(placeholder.a(), 0x99);
    chart.set_editing_drawing(Some(line));
    assert!(
        chart.build_frame().panes[0]
            .main
            .iter()
            .all(|prim| !matches!(prim, Prim::RotatedText { text, .. } if text == "+ Add text"))
    );
    chart.set_editing_drawing(None);
    chart.set_selected_drawing(None);
    chart.set_hovered_text(None);
    chart.set_hovered_text(Some(text));
    assert_eq!(chart.hovered_text(), Some(text));
    let hover = border_frames(&mut chart);
    assert_eq!(hover.len(), 1);
    assert_eq!(hover[0].2.0 & 0xFF, 0x73, "hover ring at reduced opacity");
    // The chrome box: 1.2×size line height + the 6 css px chrome pad, 2 px frame (dpr 1).
    let expected_h = (TEXT_TOOL_DEFAULT_SIZE * 1.2 + 12.0).round() as i32;
    assert_eq!(hover[0].0.h, expected_h);
    assert_eq!(hover[0].1, 2);

    // Selection upgrades the same box to full strength — and paints NO anchor discs
    // (the public reference: text has no drag-point handles).
    chart.set_selected_drawing(Some(text));
    let selected = border_frames(&mut chart);
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].2, PRIMARY, "selected border at full strength");
    assert_eq!(
        selected[0].0, hover[0].0,
        "selection keeps the hover ring's box"
    );
    assert!(frame_discs(&mut chart).is_empty());

    // The hover ring never double-paints over the selection border.
    let still_one = border_frames(&mut chart);
    assert_eq!(still_one.len(), 1);

    // While the typing-mode editor owns the drawing, the engine still paints the focus
    // border (the host wrap is borderless) — the wrap's border is not a second outline.
    chart.set_editing_drawing(Some(text));
    let editing = border_frames(&mut chart);
    assert_eq!(editing.len(), 1, "focus border stays while editing");
    assert_eq!(
        editing[0].0, selected[0].0,
        "edit does not move the focus border"
    );
    chart.set_editing_drawing(None);

    // The trend line keeps its two anchor handles on selection, each one bordered shape.
    chart.set_selected_drawing(Some(line));
    chart.set_hovered_text(None);
    let handles = chart.build_frame().panes[0]
        .main
        .iter()
        .filter(|prim| {
            matches!(prim, Prim::RoundRect { border_color, border_width, .. }
                if *border_color == PRIMARY && *border_width > 0.0)
        })
        .count();
    assert_eq!(handles, 2, "two anchor handles");

    // Deselect/deshover clears everything.
    chart.set_selected_drawing(None);
    assert!(border_frames(&mut chart).is_empty());
}

#[test]
fn selection_anchors_remain_dense_and_bounded_at_tight_spacing() {
    // 200 bars across an 800 css px pane (4 px/bar): selected plots retain the dense reference-informed
    // visual rhythm — about one anchor per 24 css px, with the first and last always kept.
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let n = 200;
    let times: Vec<f64> = (0..n).map(|i| (i * 60) as f64).collect();
    let opens: Vec<f64> = (0..n).map(|i| 10.0 + (i as f64 * 0.05).sin()).collect();
    let highs: Vec<f64> = opens.iter().map(|o| o + 0.5).collect();
    let lows: Vec<f64> = opens.iter().map(|o| o - 0.5).collect();
    let closes: Vec<f64> = opens.iter().map(|o| o + 0.2).collect();
    chart
        .set_series_data(0, &times, &opens, &highs, &lows, &closes)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.set_selected_series(Some(0));
    assert_eq!(chart.selection_anchor_identities().first(), Some(&0));
    assert_eq!(
        chart.selection_anchor_identities().last(),
        Some(&((n as i64 - 1) * 60))
    );
    let discs = frame_discs(&mut chart);
    let blue = PRIMARY;
    let borders: Vec<_> = discs.iter().copied().filter(|d| d.2 == blue).collect();
    // 800 px at one anchor per roughly 24 px: dozens of handles, still well below one per bar.
    assert!(
        borders.len() <= 36,
        "anchors must stay bounded (got {})",
        borders.len()
    );
    assert!(
        borders.len() >= 30,
        "the pane should read densely selected (got {})",
        borders.len()
    );
    let xs: Vec<f32> = borders.iter().map(|d| d.0).collect();
    assert!(
        xs.windows(2).all(|w| w[1] - w[0] >= 20.0),
        "kept anchors preserve a compact visual gap: {xs:?}"
    );
    chart.set_selected_series(None);
}

#[test]
fn selection_anchor_membership_survives_every_coordinate_change() {
    let mut chart = dense_anchor_chart(200);
    chart.set_selected_series(Some(0));
    let selected = chart.selection_anchor_identities().to_vec();
    assert!((2..=crate::MAX_SELECTION_ANCHORS).contains(&selected.len()));

    chart.time_scale_start_scroll(400.0);
    chart.time_scale_scroll_to(250.0);
    chart.time_scale_end_scroll();
    chart.build_frame();
    assert_eq!(chart.selection_anchor_identities(), selected);

    assert!(chart.update_series_bar(0, selected[0] as f64, [11.0; 4]));
    assert_eq!(chart.selection_anchor_identities(), selected);

    assert!(chart.update_series_bar(0, 30.0, [10.5; 4]));
    assert_eq!(chart.selection_anchor_identities(), selected);

    chart.time_scale_zoom(400.0, 1.0);
    chart.build_frame();
    assert_eq!(chart.selection_anchor_identities(), selected);

    chart.set_price_scale_visible_range_for(0, PriceScaleTarget::Right, 5.0, 20.0);
    chart.build_frame();
    assert_eq!(chart.selection_anchor_identities(), selected);

    chart.css_width = 640.0;
    chart.css_height = 420.0;
    chart.pane_w = 640.0;
    chart.pane_h = 420.0;
    chart.time_scale.set_width(640.0);
    chart.build_frame();
    assert_eq!(chart.selection_anchor_identities(), selected);
}

#[test]
fn deselect_reselect_resamples_full_extent_and_offscreen_anchors_return() {
    let mut chart = dense_anchor_chart(200);
    chart.set_visible_logical_range(0.0, 199.0);
    chart.set_selected_series(Some(0));
    let first_ids = chart.selection_anchor_identities().to_vec();
    let first_positions = selection_border_positions(&mut chart);
    assert!(!first_positions.is_empty());

    let first_gap = first_ids[1] / 60 - first_ids[0] / 60;
    assert!(first_gap > 4);
    chart.set_visible_logical_range(1.0, (first_gap - 1) as f64);
    assert!(selection_border_positions(&mut chart).is_empty());
    assert_eq!(chart.selection_anchor_identities(), first_ids);

    chart.set_visible_logical_range(0.0, 199.0);
    assert_eq!(selection_border_positions(&mut chart), first_positions);
    assert_eq!(chart.selection_anchor_identities(), first_ids);

    chart.set_selected_series(None);
    assert!(chart.selection_anchor_identities().is_empty());
    chart.set_visible_logical_range(100.0, 199.0);
    chart.set_selected_series(Some(0));
    let second_ids = chart.selection_anchor_identities();
    assert_eq!(second_ids.first(), Some(&0));
    assert_eq!(second_ids.last(), Some(&(199 * 60)));
    assert_eq!(first_ids.first(), second_ids.first());
    assert_eq!(first_ids.last(), second_ids.last());
}

#[test]
fn candle_selection_anchors_use_the_body_midpoint() {
    let mut chart = anchor_chart();
    chart.set_price_scale_visible_range_for(0, PriceScaleTarget::Right, 0.0, 20.0);
    chart.set_selected_series(Some(0));
    let positions = selection_border_positions(&mut chart);
    let expected = chart
        .series_price_to_coordinate(0, (10.0 + 11.0) / 2.0)
        .unwrap();
    let close = chart.series_price_to_coordinate(0, 11.0).unwrap();
    assert!((positions[0].1 as f64 - expected).abs() < 1e-4);
    assert!((positions[0].1 as f64 - close).abs() > 1.0);
}

#[test]
fn selected_realtime_identity_is_stable_while_values_reproject_and_new_bars_do_not_join() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(0, &[60.0], &[10.0], &[11.0], &[9.0], &[10.0])
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.set_price_scale_visible_range_for(0, PriceScaleTarget::Right, 0.0, 30.0);
    chart.set_selected_series(Some(0));
    let identities = chart.selection_anchor_identities().to_vec();
    let before_y = selection_border_positions(&mut chart)[0].1;

    assert!(chart.update_series_bar(0, 60.0, [20.0, 21.0, 19.0, 20.0]));
    let after_y = selection_border_positions(&mut chart)[0].1;
    assert_eq!(chart.selection_anchor_identities(), identities);
    assert_ne!(after_y, before_y);

    assert!(chart.update_series_bar(0, 120.0, [21.0, 22.0, 20.0, 21.0]));
    assert_eq!(chart.selection_anchor_identities(), identities);
}

#[test]
fn selection_anchor_membership_is_lod_independent() {
    let mut chart = dense_anchor_chart(20_000);
    chart.set_min_bar_spacing(0.000_001);
    chart.set_visible_logical_range(0.0, 19_999.0);
    chart.set_selected_series(Some(0));
    let identities = chart.selection_anchor_identities().to_vec();
    chart.build_frame();
    assert!(chart.lod_work_stats().selected_level > 0);

    chart.set_visible_logical_range(10_000.0, 10_100.0);
    chart.build_frame();
    assert_eq!(chart.lod_work_stats().selected_level, 0);
    assert_eq!(chart.selection_anchor_identities(), identities);
}

#[test]
fn source_and_indicator_snapshots_prune_retention_without_replacement() {
    let mut chart = dense_anchor_chart(200);
    let sma = chart.add_sma(0, 10).unwrap();
    let rsi = chart.add_rsi(0, 14).unwrap();
    let macd = chart.add_macd(0, 12, 26, 9);

    for selected in [sma, rsi, macd[1]] {
        chart.set_selected_series(None);
        chart.set_selected_series(Some(selected));
        let identities = chart.selection_anchor_identities().to_vec();
        assert!(!identities.is_empty());
        chart.time_scale_zoom(400.0, -1.0);
        chart.time_scale_start_scroll(400.0);
        chart.time_scale_scroll_to(300.0);
        chart.time_scale_end_scroll();
        chart.build_frame();
        assert_eq!(chart.selected_series(), Some(selected));
        assert_eq!(chart.selection_anchor_identities(), identities);
    }

    chart.set_selected_series(None);
    chart.set_visible_logical_range(0.0, 199.0);
    chart.set_selected_series(Some(0));
    let before = chart.selection_anchor_identities().to_vec();
    assert!(chart.set_series_max_points(0, Some(100)));
    let retained_times: std::collections::HashSet<_> = chart
        .series_data(0)
        .into_iter()
        .map(|point| point.time)
        .collect();
    let expected: Vec<_> = before
        .into_iter()
        .filter(|time| retained_times.contains(time))
        .collect();
    assert_eq!(chart.selection_anchor_identities(), expected);
}

#[test]
fn full_dataset_replacement_restarts_snapshot_without_retargeting() {
    let mut chart = dense_anchor_chart(200);
    chart.set_selected_series(Some(0));
    let before = chart.selection_anchor_identities().to_vec();
    let times: Vec<f64> = (1_000..1_200).map(|time| time as f64 * 60.0).collect();
    let values = vec![20.0; times.len()];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    let after = chart.selection_anchor_identities();
    assert!(!after.is_empty());
    assert!(after.iter().all(|time| *time >= 1_000 * 60));
    assert!(after.iter().all(|time| !before.contains(time)));

    chart.set_series_data(0, &[], &[], &[], &[], &[]).unwrap();
    assert!(chart.selection_anchor_identities().is_empty());
    assert!(chart.update_series_bar(0, 2_000.0 * 60.0, [30.0; 4]));
    assert!(chart.selection_anchor_identities().is_empty());
}

fn retained_two_series_chart() -> ChartEngine {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let second = chart.add_series(SeriesKind::Line);
    let times = [1.0, 2.0, 3.0];
    chart
        .set_series_data(
            0,
            &times,
            &[10.0, 20.0, 15.0],
            &[11.0, 21.0, 16.0],
            &[9.0, 19.0, 14.0],
            &[10.5, 20.5, 15.5],
        )
        .unwrap();
    chart
        .set_series_data(
            second,
            &times,
            &[100.0, 101.0, 102.0],
            &[101.0, 102.0, 103.0],
            &[99.0, 100.0, 101.0],
            &[100.5, 101.5, 102.5],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart
}

#[test]
fn crosshair_only_rebuilds_the_overlay_layer() {
    let mut chart = retained_two_series_chart();
    chart.build_frame();
    chart.set_crosshair_at(300.0, 200.0);
    chart.build_frame();
    assert_eq!(
        chart.frame_build_stats(),
        FrameBuildStats {
            overlay_rebuilds: 1,
            ..FrameBuildStats::default()
        }
    );
}

#[test]
fn hover_reorders_retained_series_without_rebuilding_geometry() {
    let mut chart = retained_two_series_chart();
    chart.build_frame();
    chart.set_hovered_series(Some(0));
    let incremental = chart.build_frame();
    assert_eq!(chart.frame_build_stats(), FrameBuildStats::default());

    chart.retained_frame = RetainedFrame::default();
    chart.frame_invalidation.all();
    let full = chart.build_frame();
    assert_eq!(incremental, full);

    chart.set_hovered_series(Some(0));
    chart.build_frame();
    assert_eq!(chart.frame_build_stats(), FrameBuildStats::default());
}

#[test]
fn idle_bollinger_paints_below_candles_and_hover_promotes_the_group() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let n = 30;
    let times: Vec<f64> = (0..n).map(|i| (i * 3600) as f64).collect();
    let values: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64).sin() * 5.0).collect();
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    // Later-added BB must not cover the primary candles by default.
    let bb = chart.add_bollinger(0, 5, 2.0);
    for (i, &id) in bb.iter().enumerate() {
        let color = ["#ff0000", "#00ff00", "#0000ff"][i];
        chart.series_entry_mut(id).unwrap().line_color = Some(color.to_string());
    }
    chart.build_frame();
    let main = &chart.build_frame().panes[0].main;
    let poly_colors: Vec<String> = main
        .iter()
        .filter_map(|prim| match prim {
            Prim::Polyline { color, .. } => Some(format!(
                "#{:02x}{:02x}{:02x}",
                color.r(),
                color.g(),
                color.b()
            )),
            _ => None,
        })
        .collect();
    // Idle BB group (upper, middle, lower in binding order) paints first (below price).
    assert!(
        poly_colors
            .windows(3)
            .any(|w| w == ["#ff0000", "#00ff00", "#0000ff"])
    );
    // Candles are Rect bodies; idle BB polylines all precede the first candle body.
    let first_rect = main
        .iter()
        .position(|prim| matches!(prim, Prim::Rect { .. }))
        .unwrap();
    let last_bb = poly_colors.iter().rposition(|c| c == "#0000ff").unwrap();
    // Map polyline order back to main indices: all three BB strokes precede candle bodies.
    let mut poly_main_idx = Vec::new();
    for (idx, prim) in main.iter().enumerate() {
        if matches!(prim, Prim::Polyline { .. }) {
            poly_main_idx.push(idx);
        }
    }
    assert!(poly_main_idx[last_bb] < first_rect);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
    // Hovering any band promotes the complete indicator above price, internal order kept.
    chart.set_hovered_series(Some(bb[1]));
    let hovered = chart.build_frame();
    let main = &hovered.panes[0].main;
    let poly_colors: Vec<String> = main
        .iter()
        .filter_map(|prim| match prim {
            Prim::Polyline { color, .. } => Some(format!(
                "#{:02x}{:02x}{:02x}",
                color.r(),
                color.g(),
                color.b()
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        poly_colors[poly_colors.len() - 3..],
        ["#ff0000", "#00ff00", "#0000ff"]
    );
    let first_rect = main
        .iter()
        .position(|prim| matches!(prim, Prim::Rect { .. }))
        .unwrap();
    let mut poly_main_idx = Vec::new();
    for (idx, prim) in main.iter().enumerate() {
        if matches!(prim, Prim::Polyline { .. }) {
            poly_main_idx.push(idx);
        }
    }
    assert!(poly_main_idx[0] > first_rect, "active BB above candles");
    // Hover reorder reuses retained geometry (no series rebuild, like the two-series case).
    chart.build_frame();
    chart.set_hovered_series(Some(bb[1]));
    chart.build_frame();
    assert_eq!(chart.frame_build_stats(), FrameBuildStats::default());
    assert_retained_frame_matches_clean_rebuild(&mut chart);
    chart.set_hovered_series(None);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
}

#[test]
fn line_area_sma_and_emas_follow_group_order_across_creation_orders_and_streaming() {
    for source_kind in [
        crate::SeriesKind::Line,
        crate::SeriesKind::Area,
        crate::SeriesKind::Candlestick,
    ] {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let n = 60;
        let times: Vec<f64> = (0..n).map(|i| (i * 3600) as f64).collect();
        let values: Vec<f64> = (0..n)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 4.0)
            .collect();
        chart.series[0].kind = source_kind;
        chart
            .set_series_data(0, &times, &values, &values, &values, &values)
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        // Idle tier is stable within the indicator group (insertion), always below ordinary
        // price.
        let emas = [2, 3, 4, 5, 6].map(|period| chart.add_ema(0, period).unwrap());
        let sma = chart.add_sma(0, 5).unwrap();
        let order = chart.effective_series_order();
        // Idle indicators (five EMAs + SMA) precede ordinary source 0.
        assert_eq!(
            &order[..6],
            &[emas[0], emas[1], emas[2], emas[3], emas[4], sma][..]
        );
        assert_eq!(order[6], 0);
        assert_retained_frame_matches_clean_rebuild(&mut chart);
        // Streaming a new bar preserves the tiering (value-only tail append stays on source).
        assert!(chart.update_series_bar(0, (n as f64) * 3600.0, [101.0, 102.0, 100.0, 101.5]));
        assert_eq!(chart.effective_series_order()[6], 0);
        assert_retained_frame_matches_clean_rebuild(&mut chart);
    }
}

#[test]
fn explicit_series_order_overrides_default_indicator_grouping() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let n = 30;
    let times: Vec<f64> = (0..n).map(|i| (i * 3600) as f64).collect();
    let values: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.1).collect();
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let bb = chart.add_bollinger(0, 5, 2.0);
    // Default: BB below candles.
    assert_eq!(chart.effective_series_order(), vec![bb[0], bb[1], bb[2], 0]);
    // Explicit: BB on top when the host asks (idle paints verbatim).
    assert!(chart.set_series_order(vec![0, bb[0], bb[1], bb[2]]));
    assert_eq!(chart.effective_series_order(), vec![0, bb[0], bb[1], bb[2]]);
    let frame = chart.build_frame();
    let first_rect = frame.panes[0]
        .main
        .iter()
        .position(|prim| matches!(prim, Prim::Rect { .. }))
        .unwrap();
    let last_poly = frame.panes[0]
        .main
        .iter()
        .rposition(|prim| matches!(prim, Prim::Polyline { .. }))
        .unwrap();
    assert!(last_poly > first_rect, "explicit BB covers candles");
    assert_retained_frame_matches_clean_rebuild(&mut chart);
}

#[test]
fn explicit_order_hovering_middle_band_keeps_internal_group_order() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let n = 30;
    let times: Vec<f64> = (0..n).map(|i| (i * 3600) as f64).collect();
    let values: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.1).collect();
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let bb = chart.add_bollinger(0, 5, 2.0);
    for (i, &id) in bb.iter().enumerate() {
        let color = ["#ff0000", "#00ff00", "#0000ff"][i];
        chart.series_entry_mut(id).unwrap().line_color = Some(color.to_string());
    }
    // Manual ordering with the indicator on top: internal ids read [1, 2, 3]-style in order.
    assert!(chart.set_series_order(vec![0, bb[0], bb[1], bb[2]]));
    let stroke_colors = |chart: &mut ChartEngine| {
        chart.build_frame().panes[0]
            .main
            .iter()
            .filter_map(|prim| match prim {
                Prim::Polyline { color, .. } => Some(format!(
                    "#{:02x}{:02x}{:02x}",
                    color.r(),
                    color.g(),
                    color.b()
                )),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(stroke_colors(&mut chart), ["#ff0000", "#00ff00", "#0000ff"]);
    // Hovering the middle output must promote the complete group intact, never [1, 3, 2].
    chart.set_hovered_series(Some(bb[1]));
    assert_eq!(chart.effective_series_order(), vec![0, bb[0], bb[1], bb[2]]);
    assert_eq!(stroke_colors(&mut chart), ["#ff0000", "#00ff00", "#0000ff"]);
    assert_eq!(chart.series_order(), &[0, bb[0], bb[1], bb[2]]);
    chart.set_hovered_series(None);
    assert_eq!(stroke_colors(&mut chart), ["#ff0000", "#00ff00", "#0000ff"]);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
}

#[test]
fn idle_drawings_sit_below_price_and_active_drawings_promote() {
    use crate::{DrawingKind, DrawingPoint};
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let n = 10;
    let times: Vec<f64> = (0..n).map(|i| (i * 3600) as f64).collect();
    let values = [11.0, 12.0, 11.0, 10.0, 11.0, 12.0, 13.0, 12.0, 11.0, 10.0];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let id = chart
        .add_drawing(
            DrawingKind::TrendLine,
            0,
            vec![
                DrawingPoint {
                    logical: 1.0,
                    price: 10.0,
                },
                DrawingPoint {
                    logical: 8.0,
                    price: 13.0,
                },
            ],
            None,
        )
        .unwrap();
    // Idle drawing below candles: its polyline precedes the first candle body.
    let frame = chart.build_frame();
    let main = &frame.panes[0].main;
    let first_poly = main
        .iter()
        .position(|prim| matches!(prim, Prim::Polyline { .. }))
        .unwrap();
    let first_rect = main
        .iter()
        .position(|prim| matches!(prim, Prim::Rect { .. }))
        .unwrap();
    assert!(first_poly < first_rect);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
    // Selection promotes above ordinary price; deselection restores below.
    chart.set_selected_drawing(Some(id));
    let frame = chart.build_frame();
    let main = &frame.panes[0].main;
    let last_poly = main
        .iter()
        .rposition(|prim| matches!(prim, Prim::Polyline { .. }))
        .unwrap();
    let last_rect = main
        .iter()
        .rposition(|prim| matches!(prim, Prim::Rect { .. }))
        .unwrap();
    assert!(last_poly > last_rect);
    // Selection-only change reuses drawing geometry (overlay for handles only, no drawing
    // or series rebuilds — ordering reassembles retained segments).
    chart.build_frame();
    chart.set_selected_drawing(Some(id));
    chart.build_frame();
    assert_eq!(
        chart.frame_build_stats(),
        FrameBuildStats {
            overlay_rebuilds: 1,
            ..FrameBuildStats::default()
        }
    );
    chart.set_selected_drawing(None);
    let frame = chart.build_frame();
    let main = &frame.panes[0].main;
    let first_poly = main
        .iter()
        .position(|prim| matches!(prim, Prim::Polyline { .. }))
        .unwrap();
    let first_rect = main
        .iter()
        .position(|prim| matches!(prim, Prim::Rect { .. }))
        .unwrap();
    assert!(first_poly < first_rect);
    // Hover promotes too; hover leave restores. Overlaps stay selectable via stable hit.
    chart.set_hovered_drawing(Some(id));
    let frame = chart.build_frame();
    assert!(
        frame.panes[0]
            .main
            .iter()
            .rposition(|prim| matches!(prim, Prim::Polyline { .. }))
            .unwrap()
            > frame.panes[0]
                .main
                .iter()
                .rposition(|prim| matches!(prim, Prim::Rect { .. }))
                .unwrap()
    );
    // Probe the trend body exactly (logical 4.0 lies 3/7 along 1.0→8.0, 10.0→13.0).
    let x = chart.logical_to_coordinate(4.0).unwrap();
    let price = 10.0 + 3.0 / 7.0 * 3.0;
    let y = chart.series_price_to_coordinate(0, price).unwrap();
    assert!(chart.hit_test_drawing(x, y).is_some_and(|hit| hit.id == id));
    chart.set_hovered_drawing(None);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
}

#[test]
fn separate_panes_hidden_removed_and_incremental_equality_hold() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let n = 30;
    let times: Vec<f64> = (0..n).map(|i| (i * 3600) as f64).collect();
    let values: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64).sin()).collect();
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let rsi = chart.add_rsi(0, 5).expect("rsi");
    assert_eq!(chart.panes.len(), 2);
    // Separate panes: price pane holds candles only; oscillator holds the indicator.
    let price_frame = chart.build_frame();
    assert!(
        price_frame.panes[0]
            .main
            .iter()
            .any(|prim| matches!(prim, Prim::Rect { .. }))
    );
    assert!(
        price_frame.panes[1]
            .main
            .iter()
            .any(|prim| matches!(prim, Prim::Polyline { .. }))
    );
    // Indicator-only pane retains stable internal ordering (single RSI, trivially stable).
    assert_eq!(
        chart.effective_series_order().last(),
        Some(&0).or(Some(&rsi))
    );
    assert_retained_frame_matches_clean_rebuild(&mut chart);
    // Hidden objects paint nowhere but keep saved order; removal prunes and stays equal.
    chart.set_series_visible(rsi, false);
    let frame = chart.build_frame();
    assert!(
        frame.panes[1].main.is_empty()
            || !frame.panes[1]
                .main
                .iter()
                .any(|prim| matches!(prim, Prim::Polyline { .. }))
    );
    assert_retained_frame_matches_clean_rebuild(&mut chart);
    chart.set_series_visible(rsi, true);
    assert!(chart.remove_series(rsi));
    assert!(!chart.series_order().contains(&rsi));
    assert_retained_frame_matches_clean_rebuild(&mut chart);
    // Second ordinary series in the price pane follows stable ordinary order.
    let other = chart.add_series(crate::SeriesKind::Line);
    chart
        .set_series_data(other, &times, &values, &values, &values, &values)
        .unwrap();
    assert_eq!(chart.series_order(), &[0, other]);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
}

#[test]
fn current_bar_rebuilds_only_its_series_when_the_scale_range_is_unchanged() {
    let mut chart = retained_two_series_chart();
    chart.build_frame();
    assert!(chart.update_series_bar(0, 3.0, [15.0, 16.0, 14.0, 15.75]));
    let incremental = chart.build_frame();
    let stats = chart.frame_build_stats();
    assert_eq!(stats.series_rebuilds, 1);
    assert_eq!(stats.grid_rebuilds, 0);
    assert_eq!(stats.drawing_rebuilds, 0);
    assert_eq!(stats.layout_rebuilds, 0);

    chart.retained_frame = RetainedFrame::default();
    chart.frame_invalidation.all();
    let full = chart.build_frame();
    assert_eq!(incremental, full);
}

#[test]
fn stable_frame_reuses_every_semantic_layer() {
    let mut chart = retained_two_series_chart();
    chart.build_frame();
    chart.build_frame();
    assert_eq!(chart.frame_build_stats(), FrameBuildStats::default());
}

#[test]
fn stable_axis_frame_is_not_rebuilt_until_an_axis_input_changes() {
    let mut chart = retained_two_series_chart();
    chart.build_axis_frame(
        80.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    assert!(!chart.frame_requires_axis());
    chart.set_crosshair_at(300.0, 200.0);
    assert!(chart.frame_requires_axis());
    chart.build_axis_frame(
        80.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    assert!(!chart.frame_requires_axis());
}

#[test]
fn countdown_tick_renegotiates_layout_without_rebuilding_unchanged_pane_layers() {
    let mut chart = retained_two_series_chart();
    chart.recompute_layout_with_measure(
        true,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    chart.build_frame();
    chart.build_axis_frame(
        80.0,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    chart.set_now_seconds(1_700_000_000.0);
    assert!(chart.frame_requires_axis());
    assert!(chart.frame_requires_layout());
    chart.recompute_layout_with_measure(
        false,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    chart.build_frame();
    assert_eq!(chart.frame_build_stats(), FrameBuildStats::default());
}

fn assert_retained_frame_matches_clean_rebuild(chart: &mut ChartEngine) {
    let incremental = chart.build_frame();
    let coordinate_revision = chart.frame_coordinate_revision();
    for pane in 0..incremental.panes.len() {
        assert_eq!(
            chart.frame_pane_segments(pane).unwrap().coordinate_revision,
            coordinate_revision
        );
        assert!(
            chart
                .frame_series_segments(pane)
                .iter()
                .all(|segment| segment.coordinate_revision == coordinate_revision)
        );
    }
    chart.retained_frame = RetainedFrame::default();
    chart.frame_invalidation.all();
    let rebuilt = chart.build_frame();
    assert_eq!(incremental, rebuilt);
}

#[test]
fn direct_host_time_scale_mutation_cannot_leave_retained_coordinates_stale() {
    let count = 500usize;
    let times = (0..count).map(|row| row as f64).collect::<Vec<_>>();
    let values = (0..count)
        .map(|row| 100.0 + (row as f64 * 0.1).sin() * 10.0)
        .collect::<Vec<_>>();
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.set_visible_logical_range(200.0, 300.0);
    chart.set_crosshair_at(400.0, 200.0);
    let before = chart.build_frame();

    chart.time_scale.zoom(400.0, 1.0);
    let zoomed = chart.build_frame();
    assert_ne!(
        before, zoomed,
        "zoom changed state but retained historical pixels stayed stale"
    );
    assert!(chart.frame_build_stats().grid_rebuilds > 0);
    assert!(chart.frame_build_stats().series_rebuilds > 0);
    assert_retained_frame_matches_clean_rebuild(&mut chart);

    let before_pan = chart.build_frame();
    chart.time_scale.start_scroll(400.0);
    chart.time_scale.scroll_to(320.0);
    let panned = chart.build_frame();
    chart.time_scale.end_scroll();
    assert_ne!(
        before_pan, panned,
        "pan changed state but retained historical pixels stayed stale"
    );
    assert!(chart.frame_build_stats().grid_rebuilds > 0);
    assert!(chart.frame_build_stats().series_rebuilds > 0);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
}

#[test]
fn direct_host_series_mutation_advances_canonical_revision_without_hashing_state() {
    let mut chart = retained_two_series_chart();
    let before = chart.build_frame();
    let revision = chart.series.revision();

    chart.series[0].up_color = Some("#ff00ff".to_string());

    assert!(chart.series.revision() > revision);
    let restyled = chart.build_frame();
    assert_ne!(before, restyled);
    assert!(chart.frame_build_stats().series_rebuilds > 0);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
}

#[test]
fn direct_host_series_visibility_mutation_requests_layout() {
    let mut chart = retained_two_series_chart();
    chart.recompute_layout_with_measure(
        true,
        |text, _bold| text.len() as f64 * 7.0,
        |text, _bold| text.len() as f64 * 6.0,
    );
    chart.build_frame();
    assert!(!chart.frame_requires_layout());

    chart.series[0].visible = false;

    assert!(chart.frame_requires_layout());
}

#[test]
fn hit_tests_and_drawing_handles_follow_the_current_coordinate_revision() {
    use crate::drawings::{DrawingKind, DrawingPoint};

    let count = 500usize;
    let times = (0..count).map(|row| row as f64).collect::<Vec<_>>();
    let values = (0..count)
        .map(|row| 100.0 + (row as f64 * 0.1).sin() * 10.0)
        .collect::<Vec<_>>();
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.set_visible_logical_range(200.0, 300.0);
    chart.build_frame();

    let logical = 250.0;
    let price = values[logical as usize];
    let drawing = chart
        .add_drawing(
            DrawingKind::HorizontalLine,
            0,
            vec![DrawingPoint { logical, price }],
            None,
        )
        .unwrap();
    chart.set_selected_drawing(Some(drawing));
    chart.time_scale.zoom(400.0, 1.0);
    chart.time_scale.start_scroll(400.0);
    chart.time_scale.scroll_to(360.0);
    chart.time_scale.end_scroll();
    chart.build_frame();

    let x = chart.logical_to_coordinate(logical).unwrap();
    let y = chart.series_price_to_coordinate(0, price).unwrap();
    assert_eq!(chart.hit_test_series(x, y), Some(0));
    chart.set_selected_series(Some(0));
    assert!(chart.hit_test_drawing(x, y).is_some());
    assert!(chart.drawing_drag_start_at(x, y));
    assert_retained_frame_matches_clean_rebuild(&mut chart);
}

#[test]
fn retained_frame_matches_clean_rebuild_across_mutation_sequence() {
    use crate::drawings::{DrawingKind, DrawingPoint};

    let mut chart = retained_two_series_chart();
    assert_retained_frame_matches_clean_rebuild(&mut chart);

    chart.set_crosshair_at(245.0, 175.0);
    assert_retained_frame_matches_clean_rebuild(&mut chart);

    assert!(chart.update_series_bar(0, 3.0, [15.0, 16.0, 14.0, 15.8]));
    assert_retained_frame_matches_clean_rebuild(&mut chart);

    chart.set_right_offset(3.5);
    assert_retained_frame_matches_clean_rebuild(&mut chart);

    chart.set_bar_spacing(11.0);
    assert_retained_frame_matches_clean_rebuild(&mut chart);

    let drawing = chart
        .add_drawing(
            DrawingKind::TrendLine,
            0,
            vec![
                DrawingPoint {
                    logical: 0.0,
                    price: 10.0,
                },
                DrawingPoint {
                    logical: 2.0,
                    price: 16.0,
                },
            ],
            None,
        )
        .unwrap();
    assert_retained_frame_matches_clean_rebuild(&mut chart);

    assert!(chart.drawing_set_points(
        drawing,
        r#"[{"logical":0.5,"price":11.0},{"logical":2.5,"price":17.0}]"#
    ));
    assert_retained_frame_matches_clean_rebuild(&mut chart);

    chart
        .apply_options(r##"{"layout":{"background":{"color":"#101820"}}}"##)
        .unwrap();
    assert_retained_frame_matches_clean_rebuild(&mut chart);

    chart.css_width = 960.0;
    chart.css_height = 540.0;
    assert_retained_frame_matches_clean_rebuild(&mut chart);

    let removed = chart.pane_series_ids(0)[1];
    assert!(chart.remove_series(removed));
    assert_retained_frame_matches_clean_rebuild(&mut chart);

    let added = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(
            added,
            &[1.0, 2.0, 3.0],
            &[30.0, 31.0, 32.0],
            &[30.0, 31.0, 32.0],
            &[30.0, 31.0, 32.0],
            &[30.0, 31.0, 32.0],
        )
        .unwrap();
    assert_retained_frame_matches_clean_rebuild(&mut chart);

    chart.set_series_pane(added, 1, 1.0);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
}

#[test]
fn retained_frame_matches_clean_rebuild_after_drawing_time_rebase() {
    use crate::drawings::{DrawingKind, DrawingPoint};

    let mut chart = retained_two_series_chart();
    let drawing = chart
        .add_drawing(
            DrawingKind::TrendLine,
            0,
            vec![
                DrawingPoint {
                    logical: 0.5,
                    price: 12.0,
                },
                DrawingPoint {
                    logical: 2.0,
                    price: 16.0,
                },
            ],
            None,
        )
        .unwrap();
    chart.build_frame();

    chart
        .set_series_data(
            0,
            &[0.0, 1.0, 2.0, 3.0],
            &[9.0, 10.0, 20.0, 15.0],
            &[10.0, 11.0, 21.0, 16.0],
            &[8.0, 9.0, 19.0, 14.0],
            &[9.5, 10.5, 20.5, 15.5],
        )
        .unwrap();

    assert_eq!(chart.drawing(drawing).unwrap().points[0].logical, 1.5);
    assert_eq!(chart.drawing(drawing).unwrap().points[1].logical, 3.0);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
}

#[test]
fn dense_retained_frame_matches_clean_rebuild_after_update_pan_and_zoom() {
    let count = 20_000usize;
    let times = (0..count).map(|row| row as f64).collect::<Vec<_>>();
    let values = (0..count)
        .map(|row| 100.0 + (row as f64 * 0.017).sin() * 8.0)
        .collect::<Vec<_>>();
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    let rsi = chart.add_rsi(0, 14).unwrap();
    chart.time_scale.set_width(800.0);
    chart.set_min_bar_spacing(0.000_001);
    chart.set_visible_logical_range(0.0, count as f64 - 1.0);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
    assert!(chart.lod_work_stats().selected_level > 0);

    assert!(chart.update_series_bar(0, count as f64 - 1.0, [103.0; 4]));
    assert_retained_frame_matches_clean_rebuild(&mut chart);
    assert!(
        chart
            .data
            .last_lod_update_nodes(0)
            .is_some_and(|nodes| nodes <= 4)
    );
    assert!(
        chart
            .data
            .last_lod_update_nodes(rsi)
            .is_some_and(|nodes| nodes <= 4)
    );

    chart.set_visible_logical_range(2_000.0, count as f64 - 2_000.0);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
    chart.set_visible_logical_range(7_000.0, 13_000.0);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
    chart.set_visible_logical_range(0.0, count as f64 - 1.0);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
    chart.set_visible_logical_range(19_500.0, count as f64 - 1.0);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
    assert_eq!(chart.lod_work_stats().selected_level, 0);
    chart.set_visible_logical_range(0.0, count as f64 - 1.0);
    assert_retained_frame_matches_clean_rebuild(&mut chart);
    assert!(chart.lod_work_stats().selected_level > 0);
}

#[test]
fn repeated_crosshair_moves_only_rebuild_overlay_and_axis_inputs() {
    let mut chart = retained_two_series_chart();
    chart.build_frame();
    for step in 0..100 {
        chart.set_crosshair_at(100.0 + f64::from(step), 150.0 + f64::from(step % 10));
        chart.build_frame();
        assert_eq!(
            chart.frame_build_stats(),
            FrameBuildStats {
                overlay_rebuilds: 1,
                ..FrameBuildStats::default()
            }
        );
    }
}

#[test]
fn appended_timestamp_matches_a_clean_rebuild() {
    let mut chart = retained_two_series_chart();
    chart.build_frame();
    assert!(chart.update_series_bar(0, 4.0, [15.5, 16.5, 14.5, 16.0]));
    assert_retained_frame_matches_clean_rebuild(&mut chart);
}

#[test]
fn one_current_bar_update_rebuilds_one_of_many_series() {
    for series_count in [2, 4, 8, 16] {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let mut ids = vec![0];
        ids.extend((1..series_count).map(|_| chart.add_series(SeriesKind::Line)));
        let times = [1.0, 2.0, 3.0];
        for (index, id) in ids.iter().copied().enumerate() {
            let base = 100.0 + index as f64 * 10.0;
            let values = [base, base + 1.0, base + 2.0];
            chart
                .set_series_data(id, &times, &values, &values, &values, &values)
                .unwrap();
        }
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        chart.build_frame();

        let target = ids[series_count / 2];
        let base = 100.0 + (series_count / 2) as f64 * 10.0;
        assert!(chart.update_series_bar(target, 3.0, [base + 2.0; 4]));
        chart.build_frame();
        assert_eq!(chart.frame_build_stats().series_rebuilds, 1);
    }
}

#[test]
fn multi_pane_current_bar_keeps_other_pane_series_retained() {
    let mut chart = retained_two_series_chart();
    let second = chart.pane_series_ids(0)[1];
    chart.set_series_pane(second, 1, 1.0);
    chart.build_frame();

    assert!(chart.update_series_bar(0, 3.0, [15.0, 16.0, 14.0, 15.8]));
    chart.build_frame();
    assert_eq!(chart.frame_build_stats().series_rebuilds, 1);
    assert_eq!(chart.frame_build_stats().layout_rebuilds, 0);
}

#[test]
fn indicator_tick_rebuilds_only_source_and_dependent_output_layers() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0, 4.0],
            &[10.0, 11.0, 12.0, 13.0],
            &[20.0, 12.0, 13.0, 14.0],
            &[0.0, 10.0, 11.0, 12.0],
            &[10.0, 11.0, 12.0, 13.0],
        )
        .unwrap();
    let unrelated_source = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(
            unrelated_source,
            &[1.0, 2.0, 3.0, 4.0],
            &[30.0, 31.0, 32.0, 33.0],
            &[30.0, 31.0, 32.0, 33.0],
            &[30.0, 31.0, 32.0, 33.0],
            &[30.0, 31.0, 32.0, 33.0],
        )
        .unwrap();
    let rsi = chart.add_rsi(0, 2).unwrap();
    let unrelated_sma = chart.add_sma(unrelated_source, 2).unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.build_frame();
    let unrelated_generation = chart.data.series_generation(unrelated_sma).unwrap();

    chart.update_series_bar(0, 4.0, [12.5, 13.5, 11.5, 12.5]);
    chart.build_frame();
    let stats = chart.frame_build_stats();
    assert_eq!(stats.series_rebuilds, 2, "source + RSI only: {stats:?}");
    assert_eq!(stats.layout_rebuilds, 0);
    assert_eq!(stats.drawing_rebuilds, 0);
    assert_eq!(
        chart.data.series_generation(unrelated_sma),
        Some(unrelated_generation)
    );
    assert!(chart.data.series_generation(rsi).unwrap() > 0);
}

/// The selected series' chip carries the public reference's active-state accent on its axis-facing edge.
#[test]
fn selecting_a_series_accents_its_last_value_chip() {
    let mut chart = two_identical_line_series();
    let accents = |chart: &mut ChartEngine| {
        chart
            .build_axis_frame(
                80.0,
                |t, _bold| t.len() as f64 * 7.0,
                |t, _bold| t.len() as f64 * 6.0,
            )
            .labels
            .into_iter()
            .filter(|l| l.text.is_empty() && l.background.is_some())
            .collect::<Vec<_>>()
    };

    // Nothing selected: no accent anywhere.
    assert!(accents(&mut chart).is_empty());

    let second = chart.series[1].id;
    chart.set_selected_series(Some(second));
    assert_eq!(
        chart.selected_series(),
        Some(second),
        "selection took effect"
    );
    let marks = accents(&mut chart);
    assert_eq!(marks.len(), 1, "exactly the selected series is accented");
    let (x, y, w, h, color) = marks[0].background.unwrap();
    assert_eq!(w, 3.0);
    // Lighter than the series color it marks, and the same hue family.
    assert_eq!(color, LINE.lighten(0.45));
    assert!(color.r() >= LINE.r() && color.g() >= LINE.g() && color.b() >= LINE.b());
    // Pinned to the axis-facing (right) edge of the right-strip chip, spanning the cluster.
    // Flush with the axis-facing (right) edge of a real price chip, spanning its full height.
    let chip = chart
        .build_axis_frame(
            80.0,
            |t, _bold| t.len() as f64 * 7.0,
            |t, _bold| t.len() as f64 * 6.0,
        )
        .labels
        .into_iter()
        .filter_map(|l| (!l.text.is_empty()).then_some(l.background).flatten())
        .find(|(bx, by, bw, bh, _)| {
            (bx + bw - (x + w)).abs() < 1e-9 && (*by - y).abs() < 1e-9 && (*bh - h).abs() < 1e-9
        });
    assert!(
        chip.is_some(),
        "the accent must sit on the axis edge of the chip it marks"
    );

    // Selecting the other series moves the accent; clearing removes it.
    chart.set_selected_series(Some(chart.series[0].id));
    assert_eq!(accents(&mut chart).len(), 1);
    chart.set_selected_series(None);
    assert!(accents(&mut chart).is_empty());
}

#[test]
fn pulse_stages_match_the_reference_and_stay_continuous() {
    use super::series_geometry::last_price_pulse_stage as stage;
    let start = stage(0.0);
    assert_eq!(
        (start.radius, start.fill_alpha, start.stroke_alpha),
        (4.0, 0.25, 0.4)
    );
    let peak = stage(2600.0 * 0.25);
    assert_eq!(
        (peak.radius, peak.fill_alpha, peak.stroke_alpha),
        (10.0, 0.0, 0.8)
    );
    let faded = stage(2600.0 * 0.525);
    assert!((faded.radius - 14.0).abs() < 1e-9 && faded.stroke_alpha.abs() < 1e-9);
    // No jumps inside a cycle: 1 ms steps never move the ring more than a fraction of a pixel.
    let mut previous = stage(0.0);
    for ms in 1..2600 {
        let next = stage(f64::from(ms));
        assert!(
            (next.radius - previous.radius).abs() < 0.05,
            "radius jump at {ms} ms"
        );
        assert!(
            (next.stroke_alpha - previous.stroke_alpha).abs() < 0.01,
            "alpha jump at {ms} ms"
        );
        previous = next;
    }
}

#[test]
fn render_cutoff_stops_drawing_rows_without_dropping_series_data() {
    let up = Color::parse_css(aeris_charts_core::style::DEFAULT_MARKET_UP_CSS).unwrap();
    let candle_bodies = |chart: &mut ChartEngine| {
        chart.build_frame().panes[0]
            .main
            .iter()
            .filter(|prim| matches!(prim, Prim::Rect { color, .. } if *color == up))
            .count()
    };
    let mut chart = ohlc_chart(SeriesKind::Candlestick, 6);
    let all = candle_bodies(&mut chart);
    assert!(
        all > 0 && all % 6 == 0,
        "every rising candle draws the same primitives"
    );
    let per_candle = all / 6;

    chart.set_series_render_before_time(0, Some(4));
    assert_eq!(
        candle_bodies(&mut chart),
        4 * per_candle,
        "rows at or after the cutoff are not drawn"
    );
    assert_eq!(chart.data_layer().series_data(0).unwrap().0.len(), 6);

    chart.set_series_render_before_time(0, Some(0));
    assert_eq!(candle_bodies(&mut chart), 0);
    chart.set_series_render_before_time(0, None);
    assert_eq!(candle_bodies(&mut chart), all);
}

#[test]
fn position_stats_have_opaque_rounded_borderless_fills() {
    use crate::drawings::{DrawingKind, DrawingPoint};
    for dpr in [1.0, 1.5, 2.0] {
        for surface in ["#089981", "#f7525f", "#0000ff", "#000000", "#ffffff"] {
            let mut chart = countdown_chart();
            chart.dpr = dpr;
            chart
                .apply_options(&format!(
                    r##"{{"layout":{{"background":{{"color":"{surface}"}}}}}}"##
                ))
                .unwrap();
            let id = chart
                .add_drawing(
                    DrawingKind::LongPosition,
                    0,
                    vec![
                        DrawingPoint {
                            logical: 1.0,
                            price: 12.0,
                        },
                        DrawingPoint {
                            logical: 3.0,
                            price: 13.0,
                        },
                        DrawingPoint {
                            logical: 1.0,
                            price: 11.0,
                        },
                    ],
                    None,
                )
                .unwrap();
            chart.set_selected_drawing(Some(id));
            let frame = chart.build_frame();
            let stats = frame.panes[0]
                .main
                .iter()
                .filter_map(|prim| match prim {
                    Prim::RoundRect {
                        fill,
                        border_width,
                        radii,
                        ..
                    } if fill.r() == 8 || fill.r() == 247 => Some((fill, border_width, radii)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(stats.len(), 3, "target, stop and two-line PnL block");
            for (fill, width, radii) in stats {
                assert_eq!(fill.a(), 255);
                // The solid fill under contrast text needs no outline on any background.
                assert_eq!(*width, 0.0);
                assert!(radii.iter().all(|radius| *radius > 0.0));
            }
        }
    }
}

#[test]
fn position_stats_follow_open_and_frozen_exit_prices_for_both_sides() {
    use crate::drawings::{DrawingKind, DrawingPoint};
    for kind in [DrawingKind::LongPosition, DrawingKind::ShortPosition] {
        let direction = if kind == DrawingKind::LongPosition {
            1.0
        } else {
            -1.0
        };
        for (terminal, closed) in [(102.0, false), (110.0, true), (90.0, true)] {
            let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
            chart.series[0].kind = SeriesKind::Candlestick;
            chart
                .set_series_data(
                    0,
                    &[0.0, 60.0, 120.0],
                    &[100.0, terminal, 100.0],
                    &[101.0, terminal + 0.5, 101.0],
                    &[99.0, terminal - 0.5, 99.0],
                    &[100.0, terminal, 100.0],
                )
                .unwrap();
            chart.time_scale.set_width(800.0);
            chart.fit_content();
            chart
                .set_instrument_metadata(crate::InstrumentMetadata {
                    tick_size: Some(0.25),
                    point_value: Some(20.0),
                    ..Default::default()
                })
                .unwrap();
            // Keep an open run's right edge at candle 1. Closed runs must freeze there even
            // though candle 2 returns to entry.
            let id = chart
                .add_drawing(
                    kind,
                    0,
                    vec![
                        DrawingPoint {
                            logical: 0.0,
                            price: 100.0,
                        },
                        DrawingPoint {
                            logical: if closed { 2.0 } else { 1.0 },
                            price: 100.0 + direction * 10.0,
                        },
                        DrawingPoint {
                            logical: 0.0,
                            price: 100.0 - direction * 5.0,
                        },
                    ],
                    None,
                )
                .unwrap();
            chart.set_selected_drawing(Some(id));
            let frame = chart.build_frame();
            let pnl = if !closed {
                (terminal - 100.0) * direction
            } else if (terminal - 100.0) * direction > 0.0 {
                10.0
            } else {
                -5.0
            };
            let expected = format!(
                "{} P&L: {pnl:.2}, Qty: 2.5",
                if closed { "Closed" } else { "Open" }
            )
            .replace('-', "\u{2212}");
            assert!(
                frame.panes[0]
                    .main
                    .iter()
                    .any(|prim| matches!(prim, Prim::Text { text, .. } if *text == expected)),
                "{kind:?} {terminal}: {expected}; texts {:?}",
                frame.panes[0]
                    .main
                    .iter()
                    .filter_map(|prim| if let Prim::Text { text, .. } = prim {
                        Some(text)
                    } else {
                        None
                    })
                    .collect::<Vec<_>>()
            );
            assert!(frame.panes[0].main.iter().any(|prim| matches!(prim, Prim::Text { text, .. } if text == "Target: 10.00 (10.000%) 40, Amount: 1500")));
        }
    }
}

#[test]
fn position_stats_refresh_instrument_metadata_and_handle_future_zero_risk_and_visibility() {
    use crate::drawings::{DrawingKind, DrawingPoint};
    fn texts(chart: &mut ChartEngine) -> Vec<String> {
        chart.build_frame().panes[0]
            .main
            .iter()
            .filter_map(|prim| match prim {
                Prim::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }
    let mut chart = countdown_chart();
    let id = chart
        .add_drawing(
            DrawingKind::LongPosition,
            0,
            vec![
                DrawingPoint {
                    logical: 6.0,
                    price: 12.0,
                },
                DrawingPoint {
                    logical: 8.0,
                    price: 13.0,
                },
                DrawingPoint {
                    logical: 6.0,
                    price: 11.0,
                },
            ],
            None,
        )
        .unwrap();
    chart.set_selected_drawing(Some(id));
    assert!(
        texts(&mut chart)
            .iter()
            .any(|text| text == "Open P&L: 0.50, Qty: 250")
    );
    chart
        .set_trading_snapshot(crate::TradingSnapshot {
            instrument: crate::InstrumentMetadata {
                tick_size: Some(0.25),
                point_value: Some(20.0),
                quantity_precision: Some(1),
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    let refreshed = texts(&mut chart);
    assert!(
        refreshed
            .iter()
            .any(|text| text == "Open P&L: 0.50, Qty: 12.5")
    );
    assert!(
        refreshed
            .iter()
            .any(|text| text == "Target: 1.00 (8.333%) 4, Amount: 1250")
    );
    chart
        .set_instrument_metadata(crate::InstrumentMetadata {
            tick_size: Some(0.5),
            point_value: Some(10.0),
            ..Default::default()
        })
        .unwrap();
    assert!(
        texts(&mut chart)
            .iter()
            .any(|text| text == "Open P&L: 0.50, Qty: 25")
    );
    assert!(chart.drawing_set_points(
        id,
        r#"[{"logical":6,"price":12},{"logical":8,"price":13},{"logical":6,"price":12}]"#
    ));
    let zero = texts(&mut chart);
    assert!(
        zero.iter()
            .any(|text| text == "Risk/reward ratio: \u{2014}")
    );
    assert!(zero.iter().any(|text| text.ends_with("Qty: \u{2014}")));
    assert!(chart.drawing_apply_options(id, r#"{"visible":false}"#));
    assert!(
        !texts(&mut chart)
            .iter()
            .any(|text| text.starts_with("Target:")
                || text.starts_with("Stop:")
                || text.contains("Qty:"))
    );
}

const BAR_FILL: &str = "#123456";

fn single_bar_chart(
    horizontal: bool,
    values: Vec<f64>,
    data_labels: bool,
    line_visible: bool,
) -> (ChartEngine, usize) {
    let pane = 0;
    let mut chart = ChartEngine::new_with_initial_domain(
        640.0,
        400.0,
        1.0,
        if horizontal {
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            }
        } else {
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            }
        },
    )
    .unwrap();
    let (x_scale, y_scale) = if horizontal {
        (GeneralScaleType::Linear, GeneralScaleType::Band)
    } else {
        (GeneralScaleType::Band, GeneralScaleType::Linear)
    };
    for (id, dimension, scale) in [
        ("bar-x", AxisDimension::X, x_scale),
        ("bar-y", AxisDimension::Y, y_scale),
    ] {
        let mut axis = GeneralAxisOptions::new(id, pane, dimension, scale);
        axis.line_visible = line_visible;
        chart.add_general_axis(axis).unwrap();
    }
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: (0..values.len()).map(|index| format!("C{index}")).collect(),
            category_indices: (0..values.len() as u32).collect(),
            y: values,
            y_valid: None,
        })
        .unwrap();
    let mut options = if horizontal {
        GeneralSeriesOptions::horizontal_bar(pane, dataset, "bar-x", "bar-y")
    } else {
        GeneralSeriesOptions::column(pane, dataset, "bar-x", "bar-y")
    };
    options.bar_corner_radius = 6.0;
    options.data_labels = data_labels;
    options.color = Some(BAR_FILL.into());
    chart.add_general_series(options).unwrap();
    chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 7.0, |_, _| 0.0);
    (chart, pane)
}

fn bar_rects(frame: &ChartFrame, pane: usize) -> Vec<(f32, f32, f32, f32, [f32; 4])> {
    let fill = aeris_charts_render::color::Color::parse_css(BAR_FILL).unwrap();
    frame.panes[pane]
        .main
        .iter()
        .filter_map(|primitive| match primitive {
            Prim::RoundRect {
                x,
                y,
                w,
                h,
                radii,
                fill: color,
                ..
            } if *color == fill => Some((*x, *y, *w, *h, *radii)),
            _ => None,
        })
        .collect()
}

#[test]
fn unstacked_bars_round_only_their_value_end() {
    for horizontal in [false, true] {
        let (mut chart, pane) = single_bar_chart(horizontal, vec![5.0, -3.0], false, true);
        let radii: Vec<[f32; 4]> = bar_rects(&chart.build_frame(), pane)
            .into_iter()
            .map(|(.., radii)| radii)
            .collect();
        // Corner order is left-top, right-top, right-bottom, left-bottom.
        let (positive, negative) = if horizontal {
            ([0.0, 6.0, 6.0, 0.0], [6.0, 0.0, 0.0, 6.0])
        } else {
            ([6.0, 6.0, 0.0, 0.0], [0.0, 0.0, 6.0, 6.0])
        };
        assert_eq!(radii, vec![positive, negative], "horizontal: {horizontal}");
    }
}

#[test]
fn bar_data_labels_sit_past_the_value_end_of_their_own_bar() {
    for horizontal in [false, true] {
        let (mut chart, pane) = single_bar_chart(horizontal, vec![4.2, 3.0, 3.6], true, true);
        let frame = chart.build_frame();
        let bars = bar_rects(&frame, pane);
        assert_eq!(bars.len(), 3);
        for ((x, y, w, h, _), value) in bars.into_iter().zip(["4.2", "3", "3.6"]) {
            let (label_x, label_y) = frame.panes[pane]
                .main
                .iter()
                .find_map(|primitive| match primitive {
                    Prim::Text { x, y, text, .. } if text == value => Some((*x, *y)),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("label {value}"));
            if horizontal {
                assert!(
                    label_y > y && label_y < y + h,
                    "{value} stays on its bar row"
                );
                assert!(label_x >= x + w, "{value} sits past the bar end");
            } else {
                assert!(
                    label_x > x && label_x < x + w,
                    "{value} stays over its column"
                );
                assert!(label_y <= y, "{value} sits above the column");
            }
        }
    }
}

#[test]
fn general_axis_lines_can_be_hidden_without_losing_labels() {
    let measure = |text: &str, _| text.len() as f64 * 7.0;
    let axis_frame = |line_visible| {
        single_bar_chart(false, vec![2.0, 3.0], false, line_visible)
            .0
            .build_axis_frame(80.0, measure, |_, _| 0.0)
    };
    let (visible, hidden) = (axis_frame(true), axis_frame(false));
    assert_eq!(visible.bands.len(), 2);
    assert!(hidden.bands.is_empty());
    assert_eq!(
        hidden
            .labels
            .iter()
            .map(|label| &label.text)
            .collect::<Vec<_>>(),
        visible
            .labels
            .iter()
            .map(|label| &label.text)
            .collect::<Vec<_>>()
    );
}

#[test]
fn empty_axis_placeholder_width_does_not_floor_later_label_widths() {
    let measure = |text: &str, _| text.len() as f64 * 7.0;
    let (mut fresh, _) = single_bar_chart(true, vec![6.0, -3.0, 4.0], false, true);
    fresh.recompute_layout_with_measure(true, measure, |_, _| 0.0);

    let mut chart = ChartEngine::new_with_initial_domain(
        640.0,
        400.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    for (id, dimension, scale) in [
        ("bar-x", AxisDimension::X, GeneralScaleType::Linear),
        ("bar-y", AxisDimension::Y, GeneralScaleType::Band),
    ] {
        chart
            .add_general_axis(GeneralAxisOptions::new(id, 0, dimension, scale))
            .unwrap();
    }
    // A layout before any data measures no labels and reserves the placeholder strip.
    chart.recompute_layout_with_measure(false, measure, |_, _| 0.0);
    let placeholder = chart.left_axis_w;
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Category {
            ids: None,
            categories: vec!["C0".into(), "C1".into(), "C2".into()],
            category_indices: vec![0, 1, 2],
            y: vec![6.0, -3.0, 4.0],
            y_valid: None,
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::horizontal_bar(0, dataset, "bar-x", "bar-y");
    options.bar_corner_radius = 6.0;
    options.color = Some(BAR_FILL.into());
    chart.add_general_series(options).unwrap();
    chart.recompute_layout_with_measure(false, measure, |_, _| 0.0);
    assert!(fresh.left_axis_w < placeholder);
    assert_eq!(chart.left_axis_w, fresh.left_axis_w);

    // Measured label widths still only grow until a shrinking layout.
    let measured = chart.left_axis_w;
    chart.recompute_layout_with_measure(false, |text, _| text.len() as f64 * 3.0, |_, _| 0.0);
    assert_eq!(chart.left_axis_w, measured);
}

#[test]
fn point_axes_default_to_half_a_step_of_outer_padding() {
    let outer = |scale| GeneralAxisOptions::new("x", 0, AxisDimension::X, scale).band_padding_outer;
    assert_eq!(outer(GeneralScaleType::Point), 0.5);
    assert_eq!(outer(GeneralScaleType::Band), 0.1);
}

/// Series primitives in paint order, split into the x coordinates a pan may translate and
/// every other field, which a pan must leave unchanged.
fn pan_series_geometry(chart: &mut ChartEngine) -> Vec<(Vec<f64>, String)> {
    let frame = chart.build_frame();
    let pane = &frame.panes[0];
    chart
        .frame_series_segments(0)
        .iter()
        .filter(|segment| segment.series_id.is_some())
        .flat_map(|segment| &pane.main[segment.start..segment.end])
        .map(|prim| match prim {
            Prim::Rect { rect, color } => (
                vec![f64::from(rect.x)],
                format!("rect {} {} {} {color:?}", rect.y, rect.w, rect.h),
            ),
            Prim::RectFrame {
                rect,
                border,
                color,
            } => (
                vec![f64::from(rect.x)],
                format!("frame {} {} {} {border} {color:?}", rect.y, rect.w, rect.h),
            ),
            Prim::Polyline {
                first_point,
                point_count,
                width,
                color,
                ..
            } => {
                let points =
                    &pane.points[*first_point as usize..(*first_point + *point_count) as usize];
                (
                    points.iter().map(|point| f64::from(point[0])).collect(),
                    format!(
                        "polyline {width} {color:?} {:?}",
                        points.iter().map(|point| point[1]).collect::<Vec<_>>()
                    ),
                )
            }
            other => (Vec::new(), format!("{other:?}")),
        })
        .collect()
}

#[test]
fn sub_pixel_pans_translate_series_geometry_by_whole_device_pixels() {
    let count = 90usize;
    let times: Vec<f64> = (0..count).map(|row| row as f64 * 60.0).collect();
    let close: Vec<f64> = (0..count)
        .map(|row| 100.0 + (row as f64 * 0.37).sin() * 6.0)
        .collect();
    let open: Vec<f64> = close
        .iter()
        .enumerate()
        .map(|(row, close)| close + if row % 3 == 0 { 1.5 } else { -1.2 })
        .collect();
    let high: Vec<f64> = open
        .iter()
        .zip(&close)
        .map(|(o, c)| o.max(*c) + 0.8)
        .collect();
    let low: Vec<f64> = open
        .iter()
        .zip(&close)
        .map(|(o, c)| o.min(*c) - 0.8)
        .collect();
    for dpr in [1.0, 1.25] {
        let mut chart = ChartEngine::new(900.0, 500.0, dpr);
        chart
            .set_series_data(0, &times, &open, &high, &low, &close)
            .unwrap();
        let volume = chart.add_series(SeriesKind::Histogram);
        chart
            .set_series_data(volume, &times, &close, &close, &close, &close)
            .unwrap();
        chart.add_sma(0, 5).unwrap();
        chart.time_scale.set_width(900.0);
        chart.set_bar_spacing(6.3);
        chart.set_right_offset(4.0);
        let start = pan_series_geometry(&mut chart);
        assert!(start.iter().any(|(_, rest)| rest.starts_with("rect")));
        assert!(start.iter().any(|(_, rest)| rest.starts_with("polyline")));
        let hpr = chart.frame_pixel_ratios().0;
        let mut shifts = Vec::new();
        for fraction in [0.13, 0.37, 0.61, 0.89, 1.42] {
            chart.set_right_offset(4.0 + fraction / (6.3 * hpr));
            let moved = pan_series_geometry(&mut chart);
            assert_eq!(moved.len(), start.len(), "dpr {dpr}, pan {fraction} px");
            let shift = moved
                .iter()
                .zip(&start)
                .find_map(|((now, _), (before, _))| Some(now.first()? - before.first()?))
                .unwrap();
            assert!(
                (shift - shift.round()).abs() < 1e-3,
                "dpr {dpr}, pan {fraction} px: shift {shift}"
            );
            for ((now_x, now_rest), (before_x, before_rest)) in moved.iter().zip(&start) {
                assert_eq!(now_rest, before_rest, "dpr {dpr}, pan {fraction} px");
                assert_eq!(now_x.len(), before_x.len());
                for (now, before) in now_x.iter().zip(before_x) {
                    assert!(
                        (now - before - shift).abs() < 1e-3,
                        "dpr {dpr}, pan {fraction} px: {now} vs {before} + {shift}"
                    );
                }
            }
            shifts.push(shift.round());
        }
        assert!(shifts.contains(&0.0) && shifts.iter().any(|shift| *shift != 0.0));
    }
}
