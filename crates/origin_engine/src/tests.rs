//! Headless-engine unit tests (extracted from `lib.rs`; `super` is the crate root).

use super::*;
use origin_render::canvas2d::{execute, Canvas2d, Viewport};
use origin_render::color::Color;

#[derive(Default)]
struct CountingCanvas {
    calls: usize,
}

impl Canvas2d for CountingCanvas {
    fn set_fill_solid(&mut self, _color: Color) {
        self.calls += 1;
    }
    fn set_fill_vgradient(&mut self, _y_top: f32, _y_bottom: f32, _top: Color, _bottom: Color) {
        self.calls += 1;
    }
    fn set_stroke(&mut self, _color: Color) {
        self.calls += 1;
    }
    fn set_line_width(&mut self, _width: f32) {
        self.calls += 1;
    }
    fn set_line_dash(&mut self, _pattern: &[f32]) {
        self.calls += 1;
    }
    fn fill_rect(&mut self, _x: f32, _y: f32, _w: f32, _h: f32) {
        self.calls += 1;
    }
    fn begin_path(&mut self) {
        self.calls += 1;
    }
    fn move_to(&mut self, _x: f32, _y: f32) {
        self.calls += 1;
    }
    fn line_to(&mut self, _x: f32, _y: f32) {
        self.calls += 1;
    }
    fn close_path(&mut self) {
        self.calls += 1;
    }
    fn arc(&mut self, _cx: f32, _cy: f32, _r: f32, _start: f32, _end: f32) {
        self.calls += 1;
    }
    fn stroke(&mut self) {
        self.calls += 1;
    }
    fn fill(&mut self) {
        self.calls += 1;
    }
}

#[test]
fn constructs_without_a_browser_or_gpu() {
    let chart = ChartEngine::new(800.0, 500.0, 2.0);
    assert_eq!(chart.series.len(), 1);
    assert_eq!(chart.panes.len(), 1);
    assert_eq!(chart.css_width, 800.0);
    assert_eq!(chart.dpr, 2.0);
}

#[test]
fn pane_layout_is_host_independent() {
    let mut pane = Pane::new();
    pane.top = 100.0;
    pane.height = 200.0;
    pane.layout(500.0);
    pane.price_scale.apply_autoscale_range(
        Some(origin_core::model::price_range::PriceRange::new(0.0, 2.0)),
        0.01,
    );
    let y = pane.price_scale.price_to_coordinate(1.0, 1.0);
    assert!(y.is_finite() && (100.0..=300.0).contains(&y));
}

#[test]
fn ingests_data_without_a_host_runtime() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let report = chart
        .set_series_data(
            0,
            &[3.0, 1.0, 2.0],
            &[12.0, 10.0, 11.0],
            &[13.0, 11.0, 12.0],
            &[9.0, 8.0, 10.0],
            &[11.0, 10.0, 11.5],
        )
        .unwrap();
    assert!(report.reordered);
    assert_eq!(chart.data.merged_times(), &[1, 2, 3]);
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    assert!(chart.time_scale.visible_logical_range().is_some());
    let frame = chart.build_frame();
    assert_eq!(frame.panes.len(), 1);
    assert!(!frame.panes[0].main.is_empty());
}

#[test]
fn series_primitive_autoscale_contribution_expands_the_owning_scale() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0],
            &[5.0, 6.0],
            &[10.0, 9.0],
            &[0.0, 1.0],
            &[7.0, 8.0],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.autoscale_visible();
    let base = chart.panes[0].price_scale.price_range().unwrap();
    let base = (base.min_value(), base.max_value());
    assert_eq!(base, (0.0, 10.0));

    // A primitive on the series reaches past the data on both ends; the merged range unions in.
    chart.add_autoscale_contribution(PrimitiveAutoscaleContribution {
        series: 0,
        pane: 0,
        target: PriceScaleTarget::Right,
        min: -50.0,
        max: 60.0,
    });
    chart.autoscale_visible();
    let merged = chart.panes[0].price_scale.price_range().unwrap();
    assert_eq!((merged.min_value(), merged.max_value()), (-50.0, 60.0));

    // Contributions are per-frame: clearing them returns the scale to the data range.
    chart.clear_autoscale_contributions();
    chart.autoscale_visible();
    let restored = chart.panes[0].price_scale.price_range().unwrap();
    assert_eq!((restored.min_value(), restored.max_value()), base);
}

#[test]
fn series_primitive_autoscale_is_gated_on_owning_series_visibility() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0],
            &[5.0, 6.0],
            &[10.0, 9.0],
            &[0.0, 1.0],
            &[7.0, 8.0],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();

    // reference price-scale.ts `_recalculatePriceRangeImpl` skips invisible sources, and series.ts
    // merges primitive ranges into the series' own autoscale info — a hidden owning series
    // therefore silences its primitives' contributions.
    chart.set_series_visible(0, false);
    chart.add_autoscale_contribution(PrimitiveAutoscaleContribution {
        series: 0,
        pane: 0,
        target: PriceScaleTarget::Right,
        min: -50.0,
        max: 60.0,
    });
    chart.autoscale_visible();
    let range = chart.panes[0].price_scale.price_range();
    assert!(
        range.is_none_or(|r| r.max_value() <= 10.0),
        "hidden series must not contribute: {range:?}"
    );

    chart.set_series_visible(0, true);
    chart.add_autoscale_contribution(PrimitiveAutoscaleContribution {
        series: 0,
        pane: 0,
        target: PriceScaleTarget::Right,
        min: -50.0,
        max: 60.0,
    });
    chart.autoscale_visible();
    let merged = chart.panes[0].price_scale.price_range().unwrap();
    assert_eq!((merged.min_value(), merged.max_value()), (-50.0, 60.0));
}

#[test]
fn series_primitive_autoscale_routes_to_the_owning_scale() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0],
            &[5.0, 6.0],
            &[10.0, 9.0],
            &[0.0, 1.0],
            &[7.0, 8.0],
        )
        .unwrap();
    let left = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(
            left,
            &[1.0, 2.0],
            &[100.0, 100.0],
            &[100.0, 100.0],
            &[100.0, 100.0],
            &[100.0, 100.0],
        )
        .unwrap();
    chart.set_series_price_scale(left, PriceScaleTarget::Left);
    chart.time_scale.set_width(800.0);
    chart.fit_content();

    // The left-bound series' primitive grows only the left scale; the right scale is untouched.
    chart.add_autoscale_contribution(PrimitiveAutoscaleContribution {
        series: left,
        pane: 0,
        target: PriceScaleTarget::Left,
        min: 0.0,
        max: 500.0,
    });
    chart.autoscale_visible();
    assert_eq!(
        chart
            .price_scale_visible_range_for(0, PriceScaleTarget::Left)
            .unwrap(),
        (0.0, 500.0)
    );
    assert_eq!(
        chart
            .price_scale_visible_range_for(0, PriceScaleTarget::Right)
            .unwrap(),
        (0.0, 10.0)
    );

    // A contribution recorded against a pane the series no longer occupies is stale and skipped.
    chart.clear_autoscale_contributions();
    chart.add_autoscale_contribution(PrimitiveAutoscaleContribution {
        series: left,
        pane: 3,
        target: PriceScaleTarget::Left,
        min: -999.0,
        max: 999.0,
    });
    chart.autoscale_visible();
    // (Flat 100 data yields the scale's degenerate ±0.05 range, not the stale contribution.)
    assert_eq!(
        chart
            .price_scale_visible_range_for(0, PriceScaleTarget::Left)
            .unwrap(),
        (99.95, 100.05)
    );
}

#[test]
fn series_primitive_autoscale_rejects_non_finite_bounds() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0],
            &[5.0, 6.0],
            &[10.0, 9.0],
            &[0.0, 1.0],
            &[7.0, 8.0],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    for (min, max) in [(f64::NAN, 10.0), (0.0, f64::INFINITY), (f64::NAN, f64::NAN)] {
        chart.add_autoscale_contribution(PrimitiveAutoscaleContribution {
            series: 0,
            pane: 0,
            target: PriceScaleTarget::Right,
            min,
            max,
        });
    }
    chart.autoscale_visible();
    assert_eq!(
        chart
            .price_scale_visible_range_for(0, PriceScaleTarget::Right)
            .unwrap(),
        (0.0, 10.0)
    );
}

#[test]
fn hidden_series_do_not_expand_autoscale() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0],
            &[5.0, 6.0],
            &[10.0, 9.0],
            &[0.0, 1.0],
            &[7.0, 8.0],
        )
        .unwrap();
    let hidden = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(
            hidden,
            &[1.0, 2.0],
            &[1000.0, 1001.0],
            &[1000.0, 1001.0],
            &[1000.0, 1001.0],
            &[1000.0, 1001.0],
        )
        .unwrap();
    chart.set_series_visible(hidden, false);
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.autoscale_visible();
    assert_eq!(
        chart.panes[0]
            .price_scale
            .price_range()
            .unwrap()
            .max_value(),
        10.0
    );

    chart.set_series_visible(hidden, true);
    chart.autoscale_visible();
    assert_eq!(
        chart.panes[0]
            .price_scale
            .price_range()
            .unwrap()
            .max_value(),
        1001.0
    );
}

#[test]
fn marker_autoscale_margins_are_headless_and_can_be_disabled() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0],
            &[100.0, 101.0],
            &[102.0, 103.0],
            &[99.0, 100.0],
            &[101.0, 102.0],
        )
        .unwrap();
    chart.set_series_markers(
        0,
        vec![Marker {
            time: 2,
            position: marker_pos::ABOVE,
            shape: marker_shape::CIRCLE,
            color: Color::rgb(0x21, 0x96, 0xf3),
            text: String::new(),
        }],
    );
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.build_frame();
    // Two fitted bars clamp marker geometry to the reference's maximum spacing bucket.
    assert_eq!(chart.panes[0].marker_margin_above, 48.0);
    assert_eq!(chart.panes[0].marker_margin_below, 0.0);

    chart.set_series_markers_auto_scale(0, false);
    chart.build_frame();
    assert_eq!(chart.panes[0].marker_margin_above, 0.0);
    assert_eq!(chart.panes[0].marker_margin_below, 0.0);
}

#[test]
fn public_time_scale_options_are_validated_and_headless() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.time_scale.set_width(800.0);
    chart.set_bar_spacing(50.0);
    chart.set_right_offset(3.5);
    assert_eq!(chart.bar_spacing(), 50.0);
    assert_eq!(chart.right_offset(), 3.5);
    chart.set_bar_spacing(f64::NAN);
    chart.set_right_offset(f64::INFINITY);
    assert_eq!(chart.bar_spacing(), 50.0);
    assert_eq!(chart.right_offset(), 3.5);
}

#[test]
fn richer_time_scale_queries_and_mutations_are_headless() {
    let mut chart = ChartEngine::new(300.0, 200.0, 1.0);
    chart
        .set_series_data(
            0,
            &[10.0, 20.0, 30.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
        )
        .unwrap();
    chart.time_scale.set_width(300.0);
    chart.fit_content();

    assert_eq!(chart.time_to_index(20.0, false), Some(1));
    assert_eq!(chart.time_to_index(15.0, false), None);
    assert_eq!(chart.time_to_index(15.0, true), Some(1));
    assert_eq!(chart.time_to_index(35.0, true), Some(2));
    let x = chart.logical_to_coordinate(1.0).unwrap();
    assert_eq!(chart.coordinate_to_logical(x), Some(1.0));
    assert_eq!(chart.logical_to_coordinate(1.25), Some(0.0));
    assert_eq!(
        chart.time_to_coordinate(20.0),
        chart.logical_to_coordinate(1.0)
    );
    assert_eq!(
        chart.coordinate_to_time(chart.logical_to_coordinate(2.0).unwrap()),
        Some(30.0)
    );

    chart.scroll_to_position(4.0);
    // The core clamps excessive future whitespace for a three-point data set.
    assert_eq!(chart.scroll_position(), 1.0);
    chart.scroll_to_real_time();
    assert_eq!(chart.scroll_position(), 0.0);
    chart.set_bar_spacing(20.0);
    chart.set_right_offset(2.0);
    chart.reset_time_scale();
    assert_eq!(chart.bar_spacing(), 6.0);
    assert_eq!(chart.right_offset(), 0.0);

    chart.set_visible_time_range(10.0, 20.0);
    assert_eq!(chart.visible_time_range(), Some((10.0, 20.0)));
}

#[test]
fn public_price_scale_state_is_headless_and_manual_ranges_survive_rendering() {
    let mut chart = ChartEngine::new(300.0, 200.0, 1.0);
    chart
        .set_series_data(
            0,
            &[10.0, 20.0, 30.0],
            &[100.0, 101.0, 102.0],
            &[101.0, 102.0, 103.0],
            &[99.0, 100.0, 101.0],
            &[100.5, 101.5, 102.5],
        )
        .unwrap();
    chart.time_scale.set_width(300.0);
    chart.layout_panes(172.0);
    chart.fit_content();
    chart.build_frame();
    assert_eq!(chart.price_scale_margins(0, false), Some((0.2, 0.1)));

    chart.set_price_scale_visible_range(0, false, 90.0, 110.0);
    assert_eq!(chart.price_scale_auto_scale(0, false), Some(false));
    chart.build_frame();
    assert_eq!(
        chart.price_scale_visible_range(0, false),
        Some((90.0, 110.0))
    );

    chart.set_price_scale_inverted(0, false, true);
    chart.set_price_scale_margins(0, false, 0.25, 0.15);
    assert_eq!(chart.price_scale_inverted(0, false), Some(true));
    assert_eq!(chart.price_scale_margins(0, false), Some((0.25, 0.15)));

    chart.set_price_scale_auto_scale(0, false, true);
    chart.build_frame();
    assert_eq!(chart.price_scale_auto_scale(0, false), Some(true));
    assert_ne!(
        chart.price_scale_visible_range(0, false),
        Some((90.0, 110.0))
    );
    assert_eq!(
        chart.series_price_scale(0),
        Some((0, PriceScaleTarget::Right))
    );

    chart.set_price_scale_mode(0, false, PriceScaleMode::Percentage);
    chart.build_frame();
    assert_eq!(
        chart.price_scale_mode(0, false),
        Some(PriceScaleMode::Percentage)
    );
    assert_eq!(chart.price_scale_auto_scale(0, false), Some(true));
    let coordinate = chart.series_price_to_coordinate(0, 101.5).unwrap();
    assert!((chart.series_coordinate_to_price(0, coordinate).unwrap() - 101.5).abs() < 1e-9);
    let axis = chart.build_axis_frame(80.0, |text| text.len() as f64 * 7.0);
    assert!(axis.labels.iter().any(|label| label.text.ends_with('%')));

    chart.set_price_scale_mode(0, false, PriceScaleMode::Logarithmic);
    chart.build_frame();
    let coordinate = chart.series_price_to_coordinate(0, 102.5).unwrap();
    assert!((chart.series_coordinate_to_price(0, coordinate).unwrap() - 102.5).abs() < 1e-8);
}

#[test]
fn reset_view_restores_time_defaults_and_reenables_autoscale() {
    let mut chart = ChartEngine::new(300.0, 200.0, 1.0);
    chart
        .set_series_data(
            0,
            &[10.0, 20.0, 30.0],
            &[100.0, 101.0, 102.0],
            &[101.0, 102.0, 103.0],
            &[99.0, 100.0, 101.0],
            &[100.5, 101.5, 102.5],
        )
        .unwrap();
    chart.time_scale.set_width(300.0);
    chart.layout_panes(172.0);
    chart.fit_content();
    chart.build_frame();

    // Simulate a user zoom/pan: custom spacing+offset and a manual (contracted) price range.
    chart.set_bar_spacing(20.0);
    chart.set_right_offset(2.0);
    chart.set_price_scale_visible_range(0, false, 101.0, 101.2);
    assert_eq!(chart.price_scale_auto_scale(0, false), Some(false));
    chart.build_frame();
    assert_eq!(
        chart.price_scale_visible_range(0, false),
        Some((101.0, 101.2))
    );

    chart.reset_view();
    // Time scale back to the configured defaults (LWC resetTimeScale)…
    assert_eq!(chart.bar_spacing(), 6.0);
    assert_eq!(chart.right_offset(), 0.0);
    // …and every pane's price scales autoscale again (LWC pane resetPriceScale); the next
    // frame recalculates the range, so the contracted data fits the pane once more.
    assert_eq!(chart.price_scale_auto_scale(0, false), Some(true));
    chart.build_frame();
    let (min, max) = chart.price_scale_visible_range(0, false).unwrap();
    assert!(
        min <= 99.0 && max >= 103.0,
        "range must fit the data, got ({min}, {max})"
    );
}

#[test]
fn left_price_scale_owns_range_axis_labels_and_pane_origin() {
    let mut chart = ChartEngine::new(300.0, 200.0, 1.0);
    chart
        .set_series_data(
            0,
            &[10.0, 20.0, 30.0],
            &[100.0, 101.0, 102.0],
            &[101.0, 102.0, 103.0],
            &[99.0, 100.0, 101.0],
            &[100.5, 101.5, 102.5],
        )
        .unwrap();
    chart.set_series_price_scale(0, PriceScaleTarget::Left);
    chart
        .options
        .apply_str(r#"{"leftPriceScale":{"visible":true},"rightPriceScale":{"visible":false}}"#)
        .unwrap();
    chart.pane_left = 58.0;
    chart.left_axis_w = 58.0;
    chart.pane_w = 242.0;
    chart.time_scale.set_width(242.0);
    chart.layout_panes(172.0);
    chart.fit_content();

    let frame = chart.build_frame();
    assert_eq!(
        chart.series_price_scale(0),
        Some((0, PriceScaleTarget::Left))
    );
    assert!(chart
        .price_scale_visible_range_for(0, PriceScaleTarget::Left)
        .is_some());
    assert!(chart
        .price_scale_visible_range_for(0, PriceScaleTarget::Right)
        .is_none());
    assert_eq!(frame.width, 300.0);
    assert_eq!(frame.panes[0].scissor[0], 58);
    assert!(frame.panes[0].main.iter().any(|prim| matches!(
        prim,
        origin_render::draw_list::Prim::Rect { rect, .. } if rect.x >= 58
    )));

    let axis = chart.build_axis_frame(80.0, |text| text.len() as f64 * 7.0);
    assert!(axis
        .labels
        .iter()
        .any(|label| label.align == AxisTextAlign::Right));
    assert!(!axis
        .labels
        .iter()
        .any(|label| label.align == AxisTextAlign::Left));
    let coordinate = chart.series_price_to_coordinate(0, 101.5).unwrap();
    assert!((chart.series_coordinate_to_price(0, coordinate).unwrap() - 101.5).abs() < 1e-9);
}

#[test]
fn series_data_and_logical_range_queries_match_reference_gap_semantics() {
    let mut chart = ChartEngine::new(300.0, 200.0, 1.0);
    let times = (0..=10).map(|time| time as f64 * 10.0).collect::<Vec<_>>();
    let values = (0..=10).map(|value| value as f64).collect::<Vec<_>>();
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    let sparse = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(
            sparse,
            &[0.0, 100.0],
            &[5.0, 15.0],
            &[5.0, 15.0],
            &[5.0, 15.0],
            &[5.0, 15.0],
        )
        .unwrap();

    assert_eq!(chart.series_kind(sparse), Some(SeriesKind::Line));
    assert_eq!(chart.series_data(sparse).len(), 2);
    assert_eq!(
        chart.series_data_by_index(sparse, 5, MismatchDirection::NearestLeft),
        Some(SeriesDataPoint {
            time: 0,
            open: 5.0,
            high: 5.0,
            low: 5.0,
            close: 5.0,
        })
    );
    assert_eq!(
        chart
            .series_data_by_index(sparse, 5, MismatchDirection::NearestRight)
            .map(|point| point.time),
        Some(100)
    );
    assert_eq!(
        chart.series_bars_in_logical_range(sparse, 3.0, 7.0),
        Some(BarsInLogicalRange {
            bars_before: 3.0,
            bars_after: 3.0,
            from: None,
            to: None,
        })
    );
    assert_eq!(
        chart.series_bars_in_logical_range(sparse, -1.5, 5.25),
        Some(BarsInLogicalRange {
            bars_before: -1.5,
            bars_after: 10.0,
            from: Some(0),
            to: Some(0),
        })
    );
}

#[test]
fn crosshair_geometry_is_host_independent() {
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
    chart.crosshair = Some((200.0, 120.0));
    let frame = chart.build_frame();
    assert!(frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, origin_render::draw_list::Prim::VLine { .. })));
    assert!(frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, origin_render::draw_list::Prim::HLine { .. })));
    // The hover marker is removed: the crosshair paints lines only, no circle marks.
    assert!(!frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, origin_render::draw_list::Prim::Circle { .. })));

    let mut canvas = CountingCanvas::default();
    for pane in &frame.panes {
        execute(
            &pane.under,
            &pane.points,
            &mut canvas,
            Viewport {
                width: 800.0,
                height: 500.0,
            },
        );
        execute(
            &pane.main,
            &pane.points,
            &mut canvas,
            Viewport {
                width: 800.0,
                height: 500.0,
            },
        );
    }
    assert!(
        canvas.calls > 0,
        "the shared frame must be executable by a Canvas2D backend"
    );
}

#[test]
fn indicators_are_engine_owned_series() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0, 4.0],
            &[1.0, 2.0, 3.0, 4.0],
            &[1.0, 2.0, 3.0, 4.0],
            &[1.0, 2.0, 3.0, 4.0],
            &[1.0, 2.0, 3.0, 4.0],
        )
        .unwrap();
    let sma = chart.add_sma(0, 2).expect("valid indicator");
    let rows = chart.data.series_data(sma).unwrap();
    assert_eq!(rows.0, &[2, 3, 4]);
    assert_eq!(rows.1[3], &[1.5, 2.5, 3.5]);

    chart.update_series_bar(0, 4.0, [4.0, 5.0, 3.0, 5.0]);
    let rows = chart.data.series_data(sma).unwrap();
    assert_eq!(rows.1[3], &[1.5, 2.5, 4.0]);

    let ema = chart.add_ema(0, 2).expect("valid indicator");
    let initial_ema = chart.data.series_data(ema).unwrap().1[3];
    assert_eq!(initial_ema.len(), 3);
    assert!((initial_ema[2] - 4.166666666666667).abs() < 1e-12);
    chart.update_series_bar(0, 5.0, [5.0, 6.0, 4.0, 6.0]);
    let ema_rows = chart.data.series_data(ema).unwrap();
    assert!((ema_rows.1[3].last().copied().unwrap() - 5.388888888888889).abs() < 1e-12);
}

#[test]
fn host_formatters_override_builtin_labels() {
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
    chart.crosshair = Some((200.0, 120.0));

    // priceFormatter prefixes every non-percentage price label; tickMarkFormatter tags the tick
    // type; timeFormatter replaces the crosshair time label.
    chart.set_price_formatter(Some(Box::new(|price| Some(format!("${price:.0}")))));
    chart.set_tick_mark_formatter(Some(Box::new(|_ts, kind| Some(format!("T{kind}")))));
    chart.set_time_formatter(Some(Box::new(|_ts| Some("XHAIR".to_string()))));

    let axis = chart.build_axis_frame(80.0, |t| t.len() as f64 * 7.0);
    assert!(axis.labels.iter().any(|l| l.text.starts_with('$')));
    let time_ticks: Vec<_> = axis
        .labels
        .iter()
        .filter(|l| l.align == AxisTextAlign::Center && l.midpoint == AxisTextMidpoint::None)
        .collect();
    assert!(
        !time_ticks.is_empty(),
        "expected at least one time tick label"
    );
    assert!(time_ticks.iter().all(|l| l.text.starts_with('T')));
    assert!(axis
        .labels
        .iter()
        .any(|l| l.midpoint == AxisTextMidpoint::StableTime && l.text == "XHAIR"));

    // Clearing a formatter restores the built-in output.
    chart.set_price_formatter(None);
    let axis = chart.build_axis_frame(80.0, |t| t.len() as f64 * 7.0);
    assert!(!axis.labels.iter().any(|l| l.text.starts_with('$')));
}

#[test]
fn time_scale_option_setters_are_headless_and_clamp() {
    let mut chart = ChartEngine::new(300.0, 200.0, 1.0);
    chart
        .set_series_data(
            0,
            &[10.0, 20.0, 30.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
        )
        .unwrap();
    chart.time_scale.set_width(300.0);
    chart.fit_content();

    // minBarSpacing floors how far the scale can zoom out.
    chart.set_min_bar_spacing(20.0);
    chart.set_bar_spacing(1.0);
    assert!(chart.bar_spacing() >= 20.0);

    // fixRightEdge pins the right offset to zero (no future whitespace).
    chart.set_fix_right_edge(true);
    chart.set_right_offset(5.0);
    assert_eq!(chart.right_offset(), 0.0);

    // timeVisible/secondsVisible are recorded on the engine and drive label formatting.
    chart.set_time_visible(true);
    chart.set_seconds_visible(true);
    assert!(chart.time_visible);
    assert!(chart.seconds_visible);
}

#[test]
fn interaction_disabled_flag_reaches_the_time_scale() {
    let mut chart = ChartEngine::new(300.0, 200.0, 1.0);
    chart
        .set_series_data(
            0,
            &[10.0, 20.0, 30.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
        )
        .unwrap();
    chart.time_scale.set_width(300.0);
    chart.fit_content();
    chart.set_bar_spacing(10.0);

    // reference `_isAllScalingAndScrollingDisabled` (time-scale.ts:975-986): the aggregate only
    // feeds tick-label alignment — it must NOT force fix-edge semantics on the scale math, so
    // offsets and spacing stay put (a non-interactive chart never reacts to resizes).
    chart.set_interaction_disabled(true);
    assert!(chart.time_scale.interaction_disabled());
    chart.set_right_offset(5.0);
    assert_eq!(chart.right_offset(), 5.0);
    chart.set_interaction_disabled(false);
    assert!(!chart.time_scale.interaction_disabled());
    chart.set_bar_spacing(10.0);
    chart.set_right_offset(5.0);
    assert_eq!(chart.right_offset(), 5.0);
}

#[test]
fn remove_series_tombstones_slot_and_drops_derived_indicators() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0],
            &[10.0, 11.0],
            &[10.0, 11.0],
            &[10.0, 11.0],
            &[10.0, 11.0],
        )
        .unwrap();
    // A second series with a far larger range, plus an indicator derived from it.
    let extra = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(
            extra,
            &[1.0, 2.0],
            &[1000.0, 1001.0],
            &[1000.0, 1001.0],
            &[1000.0, 1001.0],
            &[1000.0, 1001.0],
        )
        .unwrap();
    let sma = chart.add_sma(extra, 2).expect("valid indicator");
    assert_eq!(chart.series_kind(sma), Some(SeriesKind::Line));

    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.autoscale_visible();
    // The extra series drives autoscale up to 1001 before removal.
    assert_eq!(
        chart.panes[0]
            .price_scale
            .price_range()
            .unwrap()
            .max_value(),
        1001.0
    );

    // Removing it succeeds, reports absent, and cascades to its derived indicator.
    assert!(chart.remove_series(extra));
    assert_eq!(chart.series_kind(extra), None);
    assert_eq!(chart.series_kind(sma), None);
    assert!(chart.series_data(extra).is_empty());
    // Idempotent â€” removing the same series twice reports false.
    assert!(!chart.remove_series(extra));

    // The tombstoned slot is inert: autoscale now reflects only the primary series.
    chart.autoscale_visible();
    assert_eq!(
        chart.panes[0]
            .price_scale
            .price_range()
            .unwrap()
            .max_value(),
        11.0
    );

    // Data mutations on a removed slot are ignored â€” it can never be silently revived.
    assert!(!chart.update_series_bar(extra, 3.0, [5.0, 5.0, 5.0, 5.0]));
    let report = chart
        .set_series_data(extra, &[3.0], &[5.0], &[5.0], &[5.0], &[5.0])
        .unwrap();
    assert!(report.is_clean());
    assert!(chart.series_data(extra).is_empty());

    // reference `removeSeries` accepts any series: even the primary (id 0) tombstones now.
    assert!(chart.remove_series(0));
    assert!(!chart.remove_series(0));
}

#[test]
fn indicator_outputs_drop_the_countdown_show_the_name_chip_and_default_to_1px() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let values = [1.0, 2.0, 3.0, 4.0, 5.0];
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0, 4.0, 5.0],
            &values,
            &values,
            &values,
            &values,
        )
        .unwrap();
    let rsi = chart.add_rsi(0, 2).expect("valid rsi");
    let entry = chart.series.iter().find(|s| s.id == rsi).unwrap();
    // No candle countdown on a line value, the auto name chip shows, 1px line.
    assert!(!entry.countdown_visible);
    assert!(entry.title_visible);
    assert_eq!(entry.title, "RSI 2");
    assert_eq!(entry.line_width, Some(1.0));

    // The whole native set auto-names itself for the platform's chips.
    let sma = chart.add_sma(0, 2).unwrap();
    let ema = chart.add_ema(0, 2).unwrap();
    let bands = chart.add_bollinger(0, 3, 2.0);
    let bands_frac = chart.add_bollinger(0, 3, 2.5);
    let macd = chart.add_macd(0, 2, 3, 2);
    let stoch = chart.add_stochastic(0, 2, 3);
    let atr = chart.add_atr(0, 2).unwrap();
    let vwap = chart.add_vwap(0, None).unwrap();
    let wma = chart.add_wma(0, 3).unwrap();
    let title_of = |id: SeriesId| {
        chart
            .series
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .title
            .clone()
    };
    assert_eq!(title_of(sma), "SMA 2");
    assert_eq!(title_of(ema), "EMA 2");
    assert_eq!(title_of(bands[0]), "Bollinger 3 2");
    assert_eq!(title_of(bands_frac[0]), "Bollinger 3 2.5");
    assert_eq!(title_of(macd[0]), "MACD 2 3 2");
    assert_eq!(title_of(stoch[0]), "Stochastic 2 3");
    assert_eq!(title_of(atr), "ATR 2");
    assert_eq!(title_of(vwap), "VWAP");
    assert_eq!(title_of(wma), "WMA 3");
}

#[test]
fn a_dragged_short_pane_contracts_its_scale_instead_of_flipping_it() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let values = [1.0, 2.0, 3.0, 4.0, 5.0, 4.0, 3.0, 2.0];
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0],
            &values,
            &values,
            &values,
            &values,
        )
        .unwrap();
    chart.add_rsi(0, 2).expect("valid rsi");
    chart.time_scale.set_width(800.0);
    chart.fit_content();

    let upright = |chart: &ChartEngine, pi: usize| {
        let scale = &chart.panes[pi].price_scale;
        let range = *scale.price_range().expect("a price range");
        scale.internal_height() > 0.0
            && scale.price_to_coordinate(range.max_value(), 0.0)
                < scale.price_to_coordinate(range.min_value(), 0.0)
    };

    // Default oscillator stretch: the small pane keeps an upright, positive-height mapping.
    chart.layout_panes(400.0);
    chart.autoscale_visible();
    assert!(upright(&chart, 0), "main pane upright");
    assert!(upright(&chart, 1), "indicator pane upright");

    // Squeeze the main pane to a strip and grow the indicator pane (the separator-drag
    // extremes): both scales contract but never flip.
    chart.panes[0].stretch_factor = 0.2;
    chart.panes[1].stretch_factor = 2.0;
    chart.layout_panes(400.0);
    chart.autoscale_visible();
    assert!(upright(&chart, 0), "squeezed main pane stays upright");
    assert!(upright(&chart, 1), "grown indicator pane stays upright");
    // The fractional margins (0.2 + 0.1) resolve against the pane's own slot: the internal
    // height is 70% of the pane height regardless of the total content height.
    let slot = chart.panes[1].height;
    assert!((chart.panes[1].price_scale.internal_height() - 0.7 * slot).abs() < 1e-9);
}

#[test]
fn oscillator_indicators_get_their_own_pane_and_band_levels() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0, 4.0, 5.0],
            &[1.0, 2.0, 3.0, 4.0, 5.0],
            &[1.0, 2.0, 3.0, 4.0, 5.0],
            &[1.0, 2.0, 3.0, 4.0, 5.0],
            &[1.0, 2.0, 3.0, 4.0, 5.0],
        )
        .unwrap();
    let rsi = chart.add_rsi(0, 2).expect("valid rsi");
    // A fresh oscillator pane holds the output at the reduced stretch.
    assert_eq!(chart.panes.len(), 2);
    assert!((chart.panes[1].stretch_factor - 0.3).abs() < 1e-9);
    let entry = chart.series.iter().find(|s| s.id == rsi).unwrap();
    assert_eq!(entry.pane_index, 1);
    // Dotted muted 30/70 band lines without axis labels.
    let mut prices: Vec<f64> = entry.price_lines.iter().map(|l| l.price).collect();
    prices.sort_by(|a, b| a.total_cmp(b));
    assert_eq!(prices, vec![30.0, 70.0]);
    assert!(entry
        .price_lines
        .iter()
        .all(|l| !l.axis_label_visible && l.style == LineStyle::Dotted));
    assert_eq!(chart.indicator_info(rsi).unwrap().kind, "rsi");
}

#[test]
fn macd_outputs_are_line_line_histogram_with_four_state_colors() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let values = [1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0, 2.0, 3.0, 4.0];
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
            &values,
            &values,
            &values,
            &values,
        )
        .unwrap();
    let ids = chart.add_macd(0, 2, 3, 2);
    assert_eq!(ids.len(), 3);
    assert_eq!(chart.series_kind(ids[0]), Some(SeriesKind::Line));
    assert_eq!(chart.series_kind(ids[1]), Some(SeriesKind::Line));
    assert_eq!(chart.series_kind(ids[2]), Some(SeriesKind::Histogram));
    // All three live in the same new oscillator pane.
    assert!(ids
        .iter()
        .all(|&id| chart.series.iter().find(|s| s.id == id).unwrap().pane_index == 1));
    // Output slots and the packed signal period.
    assert_eq!(chart.indicator_info(ids[0]).unwrap().output_index, 0);
    assert_eq!(chart.indicator_info(ids[2]).unwrap().output_index, 2);
    let info = chart.indicator_info(ids[0]).unwrap();
    assert_eq!(
        (info.kind, info.period, info.deviation),
        ("macd", 3, Some(2.0))
    );
    // Every installed histogram row carries one of the four palette colors.
    let rows = chart.data.series_data(ids[2]).unwrap().1[3].len();
    assert!(rows > 0);
    const PALETTE: [u32; 4] = [0x26a69aff, 0x26a69a80, 0xef5350ff, 0xef535080];
    for r in 0..rows {
        let color = chart
            .data
            .point_color(
                ids[2],
                origin_core::model::data_layer::PointColorChannel::Body,
                r,
            )
            .expect("every histogram row is colored");
        assert!(
            PALETTE.contains(&color),
            "color {color:#x} is in the palette"
        );
    }
}

#[test]
fn vwap_stays_on_the_source_pane_and_weights_by_the_volume_series() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[0.0, 3_600.0, 86_400.0],
            &[10.0, 20.0, 30.0],
            &[10.0, 20.0, 30.0],
            &[10.0, 20.0, 30.0],
            &[10.0, 20.0, 30.0],
        )
        .unwrap();
    let volume = chart.add_series(SeriesKind::Histogram);
    chart
        .set_series_data(
            volume,
            &[0.0, 3_600.0, 86_400.0],
            &[1.0, 3.0, 5.0],
            &[1.0, 3.0, 5.0],
            &[1.0, 3.0, 5.0],
            &[1.0, 3.0, 5.0],
        )
        .unwrap();
    let vwap = chart.add_vwap(0, Some(volume)).expect("valid vwap");
    assert_eq!(
        chart
            .series
            .iter()
            .find(|s| s.id == vwap)
            .unwrap()
            .pane_index,
        0
    );
    assert_eq!(chart.indicator_info(vwap).unwrap().kind, "vwap");
    let values = &chart.data.series_data(vwap).unwrap().1[3];
    // (10*1 + 20*3) / 4 = 17.5 on day 0; the new UTC day restarts at 30.
    assert!((values[1] - 17.5).abs() < 1e-9);
    assert!((values[2] - 30.0).abs() < 1e-9);
}

#[test]
fn wma_atr_and_stochastic_place_and_report() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let values = [1.0, 2.0, 3.0, 4.0, 5.0];
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0, 4.0, 5.0],
            &values,
            &values,
            &values,
            &values,
        )
        .unwrap();
    let wma = chart.add_wma(0, 3).expect("valid wma");
    assert_eq!(
        chart
            .series
            .iter()
            .find(|s| s.id == wma)
            .unwrap()
            .pane_index,
        0
    );
    assert_eq!(chart.indicator_info(wma).unwrap().kind, "wma");
    // (1*1 + 2*2 + 3*3) / 6 at index 2.
    let wma_values = &chart.data.series_data(wma).unwrap().1[3];
    assert!((wma_values[0] - 14.0 / 6.0).abs() < 1e-9);

    let atr = chart.add_atr(0, 2).expect("valid atr");
    assert_ne!(
        chart
            .series
            .iter()
            .find(|s| s.id == atr)
            .unwrap()
            .pane_index,
        0
    );
    assert_eq!(chart.indicator_info(atr).unwrap().kind, "atr");

    let stoch = chart.add_stochastic(0, 2, 2);
    assert_eq!(stoch.len(), 2);
    let info = chart.indicator_info(stoch[0]).unwrap();
    assert_eq!(
        (info.kind, info.period, info.deviation),
        ("stochastic", 2, Some(2.0))
    );
    assert_eq!(chart.indicator_info(stoch[1]).unwrap().output_index, 1);
    // 20/80 band levels on the %K output.
    let mut prices: Vec<f64> = chart
        .series
        .iter()
        .find(|s| s.id == stoch[0])
        .unwrap()
        .price_lines
        .iter()
        .map(|l| l.price)
        .collect();
    prices.sort_by(|a, b| a.total_cmp(b));
    assert_eq!(prices, vec![20.0, 80.0]);
}

#[test]
fn bollinger_creates_three_output_series() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
        )
        .unwrap();
    let ids = chart.add_bollinger(0, 3, 2.0);
    assert_eq!(ids.len(), 3);
    assert!(chart.data.series_data(ids[0]).unwrap().1[3][0] > 3.0);
    assert_eq!(chart.data.series_data(ids[1]).unwrap().1[3], &[2.0]);
}

#[test]
fn indicator_info_reports_lineage_and_output_slots() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
        )
        .unwrap();
    let sma = chart.add_sma(0, 2).expect("valid indicator");
    let info = chart.indicator_info(sma).expect("output carries lineage");
    assert_eq!(info.kind, "sma");
    assert_eq!(info.period, 2);
    assert_eq!(info.deviation, None);
    assert_eq!(info.source, 0);
    assert_eq!(info.output_index, 0);

    let ids = chart.add_bollinger(0, 3, 2.5);
    let upper = chart.indicator_info(ids[0]).unwrap();
    assert_eq!(upper.kind, "bollinger");
    assert_eq!(upper.deviation, Some(2.5));
    assert_eq!(
        (0..3)
            .map(|i| chart.indicator_info(ids[i]).unwrap().output_index)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );

    // The source series itself and unknown ids are not indicator outputs.
    assert_eq!(chart.indicator_info(0), None);
    assert_eq!(chart.indicator_info(999), None);
}

#[test]
fn remove_series_tracked_reports_the_series_and_its_derived_outputs() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
        )
        .unwrap();
    let sma = chart.add_sma(0, 2).expect("valid indicator");
    let bands = chart.add_bollinger(0, 3, 2.0);

    let mut dropped = chart.remove_series_tracked(0);
    dropped.sort_unstable();
    let mut expected = vec![0, sma];
    expected.extend(bands);
    expected.sort_unstable();
    assert_eq!(dropped, expected);
    // Everything tombstoned: a second attempt (or an unknown id) reports nothing.
    assert!(chart.remove_series_tracked(0).is_empty());
    assert!(chart.remove_series_tracked(42).is_empty());
}

#[test]
fn retained_frame_reuses_pane_buffers() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[2.0, 3.0, 4.0],
            &[0.0, 1.0, 2.0],
            &[1.5, 2.5, 3.5],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let mut frame = ChartFrame::default();
    chart.build_frame_into(&mut frame);
    let first_capacity = frame.panes[0].main.capacity();
    chart.crosshair = Some((300.0, 100.0));
    chart.build_frame_into(&mut frame);
    assert!(frame.panes[0].main.capacity() >= first_capacity);
}

#[test]
fn frame_pane_top_layer_is_retained_per_frame() {
    // The `top` layer is host-appended (pane primitives, reference zOrder "top") after frame
    // construction; the engine owns clearing it between retained-frame rebuilds exactly like
    // `under`/`main`, so a stale top prim can never survive into the next frame.
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[2.0, 3.0, 4.0],
            &[0.0, 1.0, 2.0],
            &[1.5, 2.5, 3.5],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let mut frame = ChartFrame::default();
    chart.build_frame_into(&mut frame);
    assert!(frame.panes[0].top_prims.is_empty());
    // Simulate a host appending a top-layer prim: it survives in the frame output until the
    // next rebuild, which must reset the layer.
    frame.panes[0]
        .top_prims
        .push(origin_render::draw_list::Prim::Rect {
            rect: origin_render::draw_list::IRect {
                x: 0,
                y: 0,
                w: 4,
                h: 4,
            },
            color: origin_render::color::Color::rgb(0xff, 0x00, 0x00),
        });
    assert_eq!(frame.panes[0].top_prims.len(), 1);
    chart.build_frame_into(&mut frame);
    assert!(frame.panes[0].top_prims.is_empty());
    assert!(!frame.panes[0].main.is_empty());
}

#[test]
fn axis_frame_owns_label_content_and_positions() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0],
            &[10.0, 11.0],
            &[11.0, 12.0],
            &[9.0, 10.0],
            &[10.0, 11.0],
        )
        .unwrap();
    chart.time_scale.set_width(760.0);
    chart.fit_content();
    let axes = chart.build_axis_frame(80.0, |text| text.len() as f64);
    assert!(!axes.labels.is_empty());
    assert!(axes.labels.iter().any(|label| label.text.contains("11")));
}

#[test]
fn grid_line_style_and_color_flow_from_options() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0],
            &[10.0, 11.0, 12.0],
            &[9.0, 10.0, 11.0],
            &[8.0, 9.0, 10.0],
            &[9.5, 10.5, 11.5],
        )
        .unwrap();
    chart.time_scale.set_width(760.0);
    chart.fit_content();

    // Default: grid hidden (Origin ships grid-free); no grid lines in the under-paint bucket.
    use origin_render::draw_list::Prim;
    let mut frame = ChartFrame::default();
    chart.build_frame_into(&mut frame);
    let grid_lines: Vec<_> = frame.panes[0]
        .under
        .iter()
        .filter(|p| matches!(p, Prim::VLine { .. } | Prim::HLine { .. }))
        .collect();
    assert!(grid_lines.is_empty());

    // Turning the grid on yields solid lines in both families.
    chart
        .options
        .apply_str(
            r##"{"grid": { "vertLines": { "visible": true }, "horzLines": { "visible": true } }}"##,
        )
        .unwrap();
    let mut frame = ChartFrame::default();
    chart.build_frame_into(&mut frame);
    let grid_lines: Vec<_> = frame.panes[0]
        .under
        .iter()
        .filter(|p| matches!(p, Prim::VLine { .. } | Prim::HLine { .. }))
        .collect();
    assert!(!grid_lines.is_empty());
    assert!(grid_lines.iter().all(|p| matches!(
        p,
        Prim::VLine {
            style: LineStyle::Solid,
            ..
        } | Prim::HLine {
            style: LineStyle::Solid,
            ..
        }
    )));

    // reference numeric styles (2 dashed, 3 large-dashed) + per-family colors reach the frame.
    chart
        .options
        .apply_str(
            r##"{"grid": {
                "vertLines": { "style": 2, "color": "#112233" },
                "horzLines": { "style": 3, "color": "#445566" }
            }}"##,
        )
        .unwrap();
    chart.build_frame_into(&mut frame);
    let under = &frame.panes[0].under;
    assert!(under.iter().any(|p| matches!(
        p,
        Prim::VLine { style: LineStyle::Dashed, color, .. } if *color == Color::rgb(0x11, 0x22, 0x33)
    )));
    assert!(under.iter().any(|p| matches!(
        p,
        Prim::HLine { style: LineStyle::Dashed, color, .. } if *color == Color::rgb(0x44, 0x55, 0x66)
    )));
}

#[test]
fn crosshair_line_style_and_width_flow_from_options() {
    use origin_render::draw_list::Prim;
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
    chart.crosshair = Some((200.0, 120.0));

    // Default: Origin's dotted crosshair at the crisp 1px width.
    let mut frame = ChartFrame::default();
    chart.build_frame_into(&mut frame);
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::VLine {
            style: LineStyle::Dotted,
            width: 1,
            ..
        }
    )));
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::HLine {
            style: LineStyle::Dotted,
            width: 1,
            ..
        }
    )));

    // Per-line reference numeric style (1 dotted / 2 dashed) and lineWidth reach the frame.
    chart
        .options
        .apply_str(
            r##"{"crosshair": {
                "vertLine": { "style": 1, "width": 3 },
                "horzLine": { "style": 2, "width": 2 }
            }}"##,
        )
        .unwrap();
    chart.build_frame_into(&mut frame);
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::VLine {
            style: LineStyle::Dotted,
            width: 3,
            ..
        }
    )));
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::HLine {
            style: LineStyle::Dashed,
            width: 2,
            ..
        }
    )));
}

#[test]
fn crosshair_label_visibility_and_background_flow_from_options() {
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
    chart.crosshair = Some((200.0, 120.0));

    // Distinctive per-line label backgrounds prove each honors `labelBackgroundColor`. The price
    // label follows the horizontal line; the time label is the unique `StableTime` midpoint label.
    chart
        .options
        .apply_str(
            r##"{"crosshair": {
                "horzLine": { "labelBackgroundColor": "#010203" },
                "vertLine": { "labelBackgroundColor": "#040506" }
            }}"##,
        )
        .unwrap();
    let axis = chart.build_axis_frame(80.0, |text| text.len() as f64 * 7.0);
    let price_bg = Color::rgb(0x01, 0x02, 0x03);
    let time_bg = Color::rgb(0x04, 0x05, 0x06);
    assert!(axis
        .labels
        .iter()
        .any(|l| matches!(l.background, Some((.., c)) if c == price_bg)));
    assert!(axis
        .labels
        .iter()
        .any(|l| l.midpoint == AxisTextMidpoint::StableTime
            && matches!(l.background, Some((.., c)) if c == time_bg)));

    // `labelVisible: false` suppresses each label independently.
    chart
        .options
        .apply_str(
            r##"{"crosshair": {
                "horzLine": { "labelVisible": false },
                "vertLine": { "labelVisible": false }
            }}"##,
        )
        .unwrap();
    let axis = chart.build_axis_frame(80.0, |text| text.len() as f64 * 7.0);
    assert!(!axis
        .labels
        .iter()
        .any(|l| matches!(l.background, Some((.., c)) if c == price_bg)));
    assert!(!axis
        .labels
        .iter()
        .any(|l| l.midpoint == AxisTextMidpoint::StableTime));
}

#[test]
fn layout_font_size_scales_axis_label_box_heights() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0],
            &[10.0, 11.0],
            &[11.0, 12.0],
            &[9.0, 10.0],
            &[10.0, 11.0],
        )
        .unwrap();
    chart.time_scale.set_width(760.0);
    chart.fit_content();

    // The last-value badge is the only boxed label here; its box height is fontSize + padding.
    let tallest_box = |chart: &AxisFrame| {
        chart
            .labels
            .iter()
            .filter_map(|l| l.background.map(|b| b.3))
            .fold(0.0_f64, f64::max)
    };
    let axis = chart.build_axis_frame(80.0, |t| t.len() as f64);
    assert_eq!(tallest_box(&axis), 12.0 + 2.5 * 2.0);

    chart
        .options
        .apply_str(r#"{"layout": {"fontSize": 20}}"#)
        .unwrap();
    let axis = chart.build_axis_frame(80.0, |t| t.len() as f64);
    assert_eq!(tallest_box(&axis), 20.0 + 2.5 * 2.0);
}

#[test]
fn series_color_alpha_survives_into_line_and_histogram_strokes() {
    use origin_render::draw_list::Prim;
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Line;
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0],
            &[10.0, 11.0, 12.0],
            &[10.0, 11.0, 12.0],
            &[10.0, 11.0, 12.0],
            &[10.0, 11.0, 12.0],
        )
        .unwrap();
    let histogram = chart.add_series(SeriesKind::Histogram);
    chart
        .set_series_data(
            histogram,
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
        )
        .unwrap();
    // The TS boundary passes the full CSS color; the engine keeps its alpha channel.
    let translucent = Color::parse_css("rgba(10, 20, 30, 0.5)").unwrap();
    assert_eq!(translucent.a(), 128);
    chart.series[0].line_color = Some(translucent.to_css());
    chart.series[histogram].line_color = Some(translucent.to_css());
    chart.time_scale.set_width(800.0);
    chart.fit_content();

    let frame = chart.build_frame();
    assert!(frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, Prim::Polyline { color, .. } if *color == translucent)));
    assert!(frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, Prim::Rect { color, .. } if *color == translucent)));

    // And the options getter round-trips the alpha channel back through CSS.
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(
        Color::parse_css(options["color"].as_str().unwrap()),
        Some(translucent)
    );
}

#[test]
fn price_line_extras_drive_line_and_axis_label_rendering() {
    use origin_render::draw_list::Prim;
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0],
            &[10.0, 11.0],
            &[11.0, 12.0],
            &[9.0, 10.0],
            &[10.0, 11.0],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let line_color = Color::rgb(0x12, 0x34, 0x56);
    let id = chart.create_price_line(0, 10.5, line_color, 2, LineStyle::Solid, "target");
    let has_line = |chart: &mut ChartEngine| {
        chart.build_frame().panes[0]
            .main
            .iter()
            .any(|p| matches!(p, Prim::HLine { color, .. } if *color == line_color))
    };
    let find_label = |chart: &mut ChartEngine| {
        chart
            .build_axis_frame(80.0, |t| t.len() as f64 * 7.0)
            .labels
            .into_iter()
            .find(|l| l.text == "target")
    };

    // Defaults (reference price-line-options.ts): line drawn, boxed label in the line color with
    // contrast text.
    assert!(has_line(&mut chart));
    let label = find_label(&mut chart).expect("price-line label");
    assert!(matches!(label.background, Some((.., c)) if c == line_color));
    assert_eq!(label.color, line_color.contrast_text());

    // `lineVisible: false` skips only the HLine; the axis label stays.
    assert!(chart.price_line_apply_options(id, r#"{"line_visible":false}"#));
    assert!(!has_line(&mut chart));
    assert!(find_label(&mut chart).is_some());

    // `axisLabelVisible: false` drops only the label; the line comes back on its own.
    assert!(
        chart.price_line_apply_options(id, r#"{"line_visible":true,"axis_label_visible":false}"#)
    );
    assert!(has_line(&mut chart));
    assert!(find_label(&mut chart).is_none());

    // Custom label colors paint the box and the text independently of the line color.
    assert!(chart.price_line_apply_options(
        id,
        r##"{"axis_label_visible":true,"axis_label_color":"#010203","axis_label_text_color":"#aabbcc"}"##
    ));
    let label = find_label(&mut chart).expect("price-line label");
    assert!(matches!(label.background, Some((.., c)) if c == Color::rgb(0x01, 0x02, 0x03)));
    assert_eq!(label.color, Color::rgb(0xaa, 0xbb, 0xcc));
}

#[test]
fn price_line_options_merge_and_serialize_round_trip() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let id = chart.create_price_line(
        0,
        42.0,
        Color::rgb(0x21, 0x96, 0xf3),
        1,
        LineStyle::Solid,
        "",
    );

    // A partial patch merges: untouched keys keep their values (reference merge semantics).
    assert!(chart.price_line_apply_options(
        id,
        r#"{"price":43.5,"line_style":"large_dashed","line_width":3,"title":"T","line_visible":false}"#
    ));
    let options: serde_json::Value =
        serde_json::from_str(&chart.price_line_options_json(id).unwrap()).unwrap();
    assert_eq!(options["price"], 43.5);
    // The retired `large_dashed` name folds into its renamed equivalent: it serializes `dashed`.
    assert_eq!(options["line_style"], "dashed");
    assert_eq!(options["line_width"], 3);
    assert_eq!(options["title"], "T");
    assert_eq!(options["line_visible"], false);
    // Untouched defaults survive the merge.
    assert_eq!(options["axis_label_visible"], true);
    assert_eq!(options["color"], "#2196f3");
    assert_eq!(options["axis_label_color"], "");
    assert_eq!(options["axis_label_text_color"], "");

    // CamelCase aliases are accepted, and `""` clears a pinned label color back to
    // following the line color.
    assert!(chart.price_line_apply_options(id, r##"{"axisLabelColor":"#ff0000"}"##));
    let options: serde_json::Value =
        serde_json::from_str(&chart.price_line_options_json(id).unwrap()).unwrap();
    assert_eq!(options["axis_label_color"], "#ff0000");
    assert!(chart.price_line_apply_options(id, r#"{"axis_label_color":""}"#));
    let options: serde_json::Value =
        serde_json::from_str(&chart.price_line_options_json(id).unwrap()).unwrap();
    assert_eq!(options["axis_label_color"], "");

    // Unknown ids and malformed JSON are rejected without touching state.
    assert!(!chart.price_line_apply_options(999, "{}"));
    assert!(!chart.price_line_apply_options(id, "{ nope"));
    assert!(chart.price_line_options_json(999).is_none());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&chart.price_line_options_json(id).unwrap())
            .unwrap()["price"],
        43.5
    );
}

#[test]
fn chart_json_routes_timescale_behavioral_options_patch_driven_only() {
    let mut chart = ChartEngine::new(300.0, 200.0, 1.0);
    chart
        .set_series_data(
            0,
            &[10.0, 20.0, 30.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
        )
        .unwrap();
    chart.time_scale.set_width(300.0);
    chart.fit_content();

    chart
        .apply_options(
            r#"{"timeScale":{"barSpacing":12,"fixLeftEdge":true,"timeVisible":false,"secondsVisible":true}}"#,
        )
        .unwrap();
    assert_eq!(chart.bar_spacing(), 12.0);
    assert!(chart.time_scale.options().fix_left_edge);
    assert!(!chart.time_visible);
    assert!(chart.seconds_visible);
    // The store still deep-merges the patch for round-tripping.
    assert_eq!(
        chart.options.value()["timeScale"]["barSpacing"],
        serde_json::json!(12)
    );

    // An unrelated patch must NOT re-apply the merged store over live scale state.
    chart.set_bar_spacing(20.0);
    chart
        .apply_options(r##"{"grid":{"vertLines":{"color":"#000000"}}}"##)
        .unwrap();
    assert_eq!(chart.bar_spacing(), 20.0);

    // A patch carrying only border cosmetics leaves behavior alone too.
    chart
        .apply_options(r##"{"timeScale":{"borderColor":"#123456"}}"##)
        .unwrap();
    assert_eq!(chart.bar_spacing(), 20.0);

    // Malformed patches error out without touching state.
    assert!(chart.apply_options("{ nope").is_err());
    assert_eq!(chart.bar_spacing(), 20.0);
}

#[test]
fn max_bar_spacing_and_right_offset_pixels_setters_follow_reference() {
    let mut chart = ChartEngine::new(300.0, 200.0, 1.0);
    chart
        .set_series_data(
            0,
            &[10.0, 20.0, 30.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0, 3.0],
        )
        .unwrap();
    chart.time_scale.set_width(300.0);
    chart.fit_content();

    // maxBarSpacing caps zoom-in; 0 restores the default half-width cap; invalid is ignored.
    chart.set_max_bar_spacing(10.0);
    chart.set_bar_spacing(50.0);
    assert_eq!(chart.bar_spacing(), 10.0);
    chart.set_max_bar_spacing(0.0);
    chart.set_bar_spacing(10_000.0);
    assert_eq!(chart.bar_spacing(), 150.0);
    chart.set_max_bar_spacing(f64::NAN);
    chart.set_bar_spacing(10_000.0);
    assert_eq!(chart.bar_spacing(), 150.0);

    // rightOffsetPixels converts to bars through the current spacing; invalid is ignored.
    chart.set_bar_spacing(6.0);
    chart.set_right_offset_pixels(60.0);
    assert_eq!(chart.right_offset(), 10.0);
    chart.set_right_offset_pixels(f64::INFINITY);
    assert_eq!(chart.right_offset(), 10.0);
}

#[test]
fn series_options_json_covers_the_ts_field_set() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);

    // Defaults on the primary candle series.
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    for key in [
        "color",
        "up_color",
        "down_color",
        "wick_up_color",
        "wick_down_color",
        "border_up_color",
        "border_down_color",
        "wick_visible",
        "border_visible",
        "line_width",
        "line_type",
        "area_top_color",
        "area_bottom_color",
        "histogram_updown",
        "baseline_value",
        "point_markers",
        "last_price_animation",
        "visible",
        "price_scale_id",
        "pane",
        "title",
        "title_visible",
        "countdown_visible",
    ] {
        assert!(options.get(key).is_some(), "missing key {key}");
    }
    assert_eq!(options["color"], "#2196f3");
    // Unset optional colors are the follow-body/default state: "".
    assert_eq!(options["up_color"], "");
    assert_eq!(options["down_color"], "");
    assert_eq!(options["wick_up_color"], "");
    assert_eq!(options["border_down_color"], "");
    assert_eq!(options["area_top_color"], "");
    assert_eq!(options["area_bottom_color"], "");
    assert_eq!(options["wick_visible"], true);
    assert_eq!(options["border_visible"], true);
    assert_eq!(options["line_width"], 3.0);
    assert_eq!(options["line_type"], "simple");
    assert_eq!(options["histogram_updown"], false);
    assert_eq!(options["baseline_value"], serde_json::Value::Null);
    assert_eq!(options["point_markers"], false);
    assert_eq!(options["last_price_animation"], false);
    assert_eq!(options["visible"], true);
    assert_eq!(options["price_scale_id"], "right");
    assert_eq!(options["pane"], 0);
    // TradingView-style last-value cluster options: LWC-parity defaults.
    assert_eq!(options["title"], "");
    assert_eq!(options["title_visible"], true);
    assert_eq!(options["countdown_visible"], true);

    // Set state round-trips with colors and flags intact.
    chart.series[0].up_color = Some("#26a69a".to_string());
    chart.series[0].wick_up_color = Some(Color::rgba(1, 2, 3, 0x80).to_css());
    chart.series[0].border_visible = Some(false);
    chart.series[0].line_width = Some(5.0);
    chart.series[0].line_type = LineType::WithSteps;
    chart.series[0].baseline = Some(9.5);
    chart.series[0].point_markers = true;
    chart.set_series_visible(0, false);
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(options["up_color"], "#26a69a");
    assert_eq!(
        Color::parse_css(options["wick_up_color"].as_str().unwrap()),
        Some(Color::rgba(1, 2, 3, 0x80))
    );
    assert_eq!(options["border_visible"], false);
    assert_eq!(options["line_width"], 5.0);
    assert_eq!(options["line_type"], "stepped");
    assert_eq!(options["baseline_value"], 9.5);
    assert_eq!(options["point_markers"], true);
    assert_eq!(options["visible"], false);

    // Scale targeting maps to the reference priceScaleId values; removed series report nothing.
    let overlay = chart.add_series(SeriesKind::Histogram);
    chart.series[overlay].overlay = true;
    let left = chart.add_series(SeriesKind::Line);
    chart.series[left].left_scale = true;
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(overlay).unwrap()).unwrap();
    assert_eq!(options["price_scale_id"], "");
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(left).unwrap()).unwrap();
    assert_eq!(options["price_scale_id"], "left");
    assert!(chart.remove_series(left));
    assert!(chart.series_options_json(left).is_none());
}

#[test]
fn time_scale_options_json_covers_all_fields() {
    let mut chart = ChartEngine::new(300.0, 200.0, 1.0);
    chart
        .apply_options(
            r#"{"timeScale":{"minBarSpacing":2,"fixRightEdge":true,"lockVisibleTimeRangeOnResize":true,"rightBarStaysOnScroll":true,"timeVisible":true,"secondsVisible":true,"rightOffsetPixels":30}}"#,
        )
        .unwrap();
    chart.set_max_bar_spacing(50.0);

    let options: serde_json::Value =
        serde_json::from_str(&chart.time_scale_options_json()).unwrap();
    for key in [
        "bar_spacing",
        "right_offset",
        "min_bar_spacing",
        "max_bar_spacing",
        "right_offset_pixels",
        "time_visible",
        "seconds_visible",
        "fix_left_edge",
        "fix_right_edge",
        "lock_visible_time_range_on_resize",
        "right_bar_stays_on_scroll",
    ] {
        assert!(options.get(key).is_some(), "missing key {key}");
    }
    assert_eq!(options["bar_spacing"], 6.0);
    assert_eq!(options["right_offset"], 0.0);
    assert_eq!(options["min_bar_spacing"], 2.0);
    assert_eq!(options["max_bar_spacing"], 50.0);
    assert_eq!(options["right_offset_pixels"], 30.0);
    assert_eq!(options["time_visible"], true);
    assert_eq!(options["seconds_visible"], true);
    assert_eq!(options["fix_left_edge"], false);
    assert_eq!(options["fix_right_edge"], true);
    assert_eq!(options["lock_visible_time_range_on_resize"], true);
    assert_eq!(options["right_bar_stays_on_scroll"], true);
}

#[test]
fn series_style_options_default_to_reference() {
    // reference defaults: api/options/series-options-defaults.ts (common) and the per-kind style
    // defaults in model/series/{line,area,baseline,bar,histogram}-series.ts.
    let chart = ChartEngine::new(800.0, 500.0, 1.0);
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    for key in [
        "last_value_visible",
        "price_line_visible",
        "price_line_source",
        "price_line_width",
        "price_line_color",
        "price_line_style",
        "line_style",
        "line_visible",
        "point_markers_radius",
        "top_fill_color1",
        "top_fill_color2",
        "top_line_color",
        "top_line_width",
        "top_line_style",
        "bottom_fill_color1",
        "bottom_fill_color2",
        "bottom_line_color",
        "bottom_line_width",
        "bottom_line_style",
        "base",
        "invert_filled_area",
        "open_visible",
        "thin_bars",
    ] {
        assert!(options.get(key).is_some(), "missing key {key}");
    }
    assert_eq!(options["last_value_visible"], true);
    assert_eq!(options["price_line_visible"], true);
    assert_eq!(options["price_line_source"], 0); // PriceLineSource.LastBar
    assert_eq!(options["price_line_width"], 1.0);
    assert_eq!(options["price_line_color"], "");
    assert_eq!(options["price_line_style"], 1); // LineStyle.Dotted
    assert_eq!(options["line_style"], 0); // LineStyle.Solid
    assert_eq!(options["line_visible"], true);
    assert_eq!(options["point_markers_radius"], serde_json::Value::Null);
    assert_eq!(options["top_fill_color1"], "");
    assert_eq!(options["top_fill_color2"], "");
    assert_eq!(options["top_line_color"], "");
    assert_eq!(options["top_line_width"], serde_json::Value::Null);
    assert_eq!(options["top_line_style"], 0);
    assert_eq!(options["bottom_fill_color1"], "");
    assert_eq!(options["bottom_fill_color2"], "");
    assert_eq!(options["bottom_line_color"], "");
    assert_eq!(options["bottom_line_width"], serde_json::Value::Null);
    assert_eq!(options["bottom_line_style"], 0);
    assert_eq!(options["base"], 0.0);
    assert_eq!(options["invert_filled_area"], false);
    assert_eq!(options["open_visible"], true);
    assert_eq!(options["thin_bars"], true);
}

#[test]
fn series_apply_options_json_round_trips_all_new_fields() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let patch = r##"{
        "last_value_visible": false,
        "price_line_visible": false,
        "price_line_source": 1,
        "price_line_width": 2,
        "price_line_color": "#112233",
        "price_line_style": 0,
        "line_style": 2,
        "line_visible": false,
        "point_markers_radius": 6.5,
        "top_fill_color1": "rgba(1,2,3,0.5)",
        "top_fill_color2": "#040506",
        "top_line_color": "#070809",
        "top_line_width": 5,
        "top_line_style": 1,
        "bottom_fill_color1": "#0a0b0c",
        "bottom_fill_color2": "#0d0e0f",
        "bottom_line_color": "#101112",
        "bottom_line_width": 6,
        "bottom_line_style": 3,
        "base": 42.5,
        "invert_filled_area": true,
        "open_visible": false,
        "thin_bars": false,
        "title": "NDQ",
        "title_visible": false,
        "countdown_visible": true
    }"##;
    assert!(chart.series_apply_options_json(0, patch));
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(options["last_value_visible"], false);
    assert_eq!(options["price_line_visible"], false);
    assert_eq!(options["price_line_source"], 1);
    assert_eq!(options["price_line_width"], 2.0);
    assert_eq!(options["price_line_color"], "#112233");
    assert_eq!(options["price_line_style"], 0);
    assert_eq!(options["line_style"], 2);
    assert_eq!(options["line_visible"], false);
    assert_eq!(options["point_markers_radius"], 6.5);
    assert_eq!(options["top_fill_color1"], "rgba(1,2,3,0.5)");
    assert_eq!(options["top_fill_color2"], "#040506");
    assert_eq!(options["top_line_color"], "#070809");
    assert_eq!(options["top_line_width"], 5.0);
    assert_eq!(options["top_line_style"], 1);
    assert_eq!(options["bottom_fill_color1"], "#0a0b0c");
    assert_eq!(options["bottom_fill_color2"], "#0d0e0f");
    assert_eq!(options["bottom_line_color"], "#101112");
    assert_eq!(options["bottom_line_width"], 6.0);
    assert_eq!(options["bottom_line_style"], 3);
    assert_eq!(options["base"], 42.5);
    assert_eq!(options["invert_filled_area"], true);
    assert_eq!(options["open_visible"], false);
    assert_eq!(options["thin_bars"], false);
    assert_eq!(options["title"], "NDQ");
    assert_eq!(options["title_visible"], false);
    assert_eq!(options["countdown_visible"], true);

    // Round-trip parity: re-applying the serialized options is a fixed point.
    let serialized = chart.series_options_json(0).unwrap();
    assert!(chart.series_apply_options_json(0, &serialized));
    assert_eq!(chart.series_options_json(0).unwrap(), serialized);

    // "" clears a pinned color, null restores an auto/follow numeric slot.
    assert!(chart.series_apply_options_json(
        0,
        r#"{"price_line_color": "", "point_markers_radius": null, "top_line_width": null}"#
    ));
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(options["price_line_color"], "");
    assert_eq!(options["point_markers_radius"], serde_json::Value::Null);
    assert_eq!(options["top_line_width"], serde_json::Value::Null);
}

#[test]
fn series_apply_options_json_round_trips_color_strings_verbatim() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    // reference stores the user's color string verbatim: hex case, rgba() spacing, and named colors
    // all come back from options() exactly as applied (named colors the renderer cannot parse
    // fall back to the default at render time, but still round-trip).
    assert!(chart.series_apply_options_json(
        0,
        r##"{"price_line_color": "#FF0000",
             "top_line_color": "#FF0000",
             "top_fill_color1": "rgba(1, 2, 3, 0.5)",
             "top_fill_color2": "red",
             "bottom_line_color": "#FF0000",
             "bottom_fill_color1": "rgba(1, 2, 3, 0.5)",
             "bottom_fill_color2": "red"}"##
    ));
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(options["price_line_color"], "#FF0000");
    assert_eq!(options["top_line_color"], "#FF0000");
    assert_eq!(options["top_fill_color1"], "rgba(1, 2, 3, 0.5)");
    assert_eq!(options["top_fill_color2"], "red");
    assert_eq!(options["bottom_line_color"], "#FF0000");
    assert_eq!(options["bottom_fill_color1"], "rgba(1, 2, 3, 0.5)");
    assert_eq!(options["bottom_fill_color2"], "red");

    // The serialized options remain a fixed point under re-apply.
    let serialized = chart.series_options_json(0).unwrap();
    assert!(chart.series_apply_options_json(0, &serialized));
    assert_eq!(chart.series_options_json(0).unwrap(), serialized);
}

#[test]
fn series_apply_options_json_ignores_unknown_keys_and_bad_input() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    // Unknown keys, wrong types, and out-of-range enum values leave state untouched.
    assert!(chart.series_apply_options_json(
        0,
        r#"{"unknown_key": 1, "line_style": 9, "price_line_source": 4, "line_visible": "yes",
            "price_line_width": -2, "price_line_color": 7}"#
    ));
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(options["line_style"], 0);
    assert_eq!(options["price_line_source"], 0);
    assert_eq!(options["line_visible"], true);
    assert_eq!(options["price_line_width"], 1.0);
    assert_eq!(options["price_line_color"], "");

    // A partial patch merges: untouched keys keep their values (reference merge semantics).
    assert!(chart.series_apply_options_json(0, r#"{"line_style": 3}"#));
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(options["line_style"], 3);
    assert_eq!(options["last_value_visible"], true);

    // Malformed JSON and unknown/removed ids report failure without touching state.
    assert!(!chart.series_apply_options_json(0, "{ nope"));
    assert!(!chart.series_apply_options_json(999, "{}"));
    assert!(!chart.series_apply_options_json(0, "[]"));
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(options["line_style"], 3);
}

// --- per-data-point colors (reference data-item colors) ---

/// Line chart with four bars and a red body override on bar 1.
fn point_colored_chart() -> ChartEngine {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Line;
    let times = [1.0, 2.0, 3.0, 4.0];
    let values = [10.0, 11.0, 12.0, 13.0];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    assert!(chart.set_series_point_colors(0, Some(vec![0, 0xFF0000FF, 0, 0]), None, None));
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart
}

#[test]
fn point_colors_validate_lengths_and_reset_on_set_data() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times = [1.0, 2.0, 3.0];
    let values = [10.0, 11.0, 12.0];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();

    // A channel that does not match the row count rejects the whole call (no partial state).
    assert!(!chart.set_series_point_colors(0, Some(vec![1, 2]), None, None));
    assert!(!chart.data.has_point_colors(0));
    // Unknown series ids reject.
    assert!(!chart.set_series_point_colors(99, Some(vec![1, 2, 3]), None, None));

    assert!(chart.set_series_point_colors(0, Some(vec![1, 2, 3]), None, None));
    assert!(chart.data.has_point_colors(0));

    // set_series_data invalidates the point colors (the host re-installs them after).
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    assert!(!chart.data.has_point_colors(0));
}

/// Body-channel override at `row` (reference data-item color), for the point-color tests.
fn body_color_at(chart: &ChartEngine, id: SeriesId, row: usize) -> Option<u32> {
    chart.data.point_color(
        id,
        origin_core::model::data_layer::PointColorChannel::Body,
        row,
    )
}

#[test]
fn point_colors_stay_aligned_through_updates() {
    let mut chart = point_colored_chart();
    assert_eq!(body_color_at(&chart, 0, 1), Some(0xFF0000FF));

    // Append with a styled update: the new bar carries its own channels.
    assert!(chart.update_series_bar_styled(
        0,
        5.0,
        [14.0, 14.0, 14.0, 14.0],
        [Some(0x00FF00FF), None, None],
    ));
    assert_eq!(body_color_at(&chart, 0, 4), Some(0x00FF00FF));
    assert_eq!(body_color_at(&chart, 0, 1), Some(0xFF0000FF));

    // Append with a plain update: no override on the new bar, channels stay aligned.
    assert!(chart.update_series_bar(0, 6.0, [15.0, 15.0, 15.0, 15.0]));
    assert_eq!(body_color_at(&chart, 0, 5), None);
    assert_eq!(body_color_at(&chart, 0, 4), Some(0x00FF00FF));

    // Replace-last with a styled update retargets the bar's channels.
    assert!(chart.update_series_bar_styled(
        0,
        6.0,
        [15.0, 15.0, 15.0, 15.0],
        [Some(0x0000FFFF), None, None],
    ));
    assert_eq!(body_color_at(&chart, 0, 5), Some(0x0000FFFF));

    // Insert ahead of the first bar with a plain update (the rebuild path): existing
    // overrides shift with their rows.
    assert!(chart.update_series_bar(0, 0.0, [8.0, 8.0, 8.0, 8.0]));
    assert_eq!(body_color_at(&chart, 0, 0), None); // the inserted bar
    assert_eq!(body_color_at(&chart, 0, 2), Some(0xFF0000FF)); // old row 1
    assert_eq!(body_color_at(&chart, 0, 6), Some(0x0000FFFF)); // old row 5
}

#[test]
fn point_colors_follow_the_winning_row_under_dedupe() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    // Duplicate time 2: the later row (value 25, color 99) wins, taking its color along.
    let report = chart
        .set_series_data_styled(
            0,
            &[1.0, 2.0, 2.0, 3.0],
            &[10.0, 20.0, 25.0, 30.0],
            &[10.0, 20.0, 25.0, 30.0],
            &[10.0, 20.0, 25.0, 30.0],
            &[10.0, 20.0, 25.0, 30.0],
            [Some(vec![10, 20, 99, 30]), None, None],
        )
        .unwrap();
    assert_eq!(report.dropped_duplicate, 1);
    assert_eq!(
        (0..3)
            .map(|row| body_color_at(&chart, 0, row))
            .collect::<Vec<_>>(),
        vec![Some(10), Some(99), Some(30)]
    );

    // A color channel length mismatch rejects the ingest like a column mismatch.
    assert!(chart
        .set_series_data_styled(
            0,
            &[1.0, 2.0],
            &[1.0, 2.0],
            &[1.0, 2.0],
            &[1.0, 2.0],
            &[1.0, 2.0],
            [Some(vec![1]), None, None],
        )
        .is_err());
}

// --- per-series price_format (reference PriceFormat) ---

#[test]
fn price_format_defaults_and_options_round_trip() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    // reference series-options-defaults.ts: {type:'price', precision:2, minMove:0.01}.
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(
        options["price_format"],
        serde_json::json!({"type": "price", "precision": 2, "min_move": 0.01})
    );

    // Applying each built-in type round-trips through series_options_json; a nested
    // price_format key in a general options patch routes to the same applier.
    assert!(chart.series_apply_price_format_json(0, r#"{"type": "volume", "precision": 1}"#));
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    // the reference's PriceFormatVolume is exactly {type:"volume"}; the apply-time precision superset is
    // kept by the formatter but not serialized.
    assert_eq!(
        options["price_format"],
        serde_json::json!({"type": "volume"})
    );
    assert!(chart.series_apply_options_json(0, &chart.series_options_json(0).unwrap()));

    assert!(chart.series_apply_options_json(
        0,
        r#"{"price_format": {"type": "percent", "precision": 3}}"#
    ));
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(
        options["price_format"],
        serde_json::json!({"type": "percent", "precision": 3})
    );

    assert!(chart.series_apply_price_format_json(
        0,
        r#"{"type": "price", "precision": 4, "min_move": 0.0001}"#
    ));
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(
        options["price_format"],
        serde_json::json!({"type": "price", "precision": 4, "min_move": 0.0001})
    );
    // Fixed-point round-trip.
    let serialized = chart.series_options_json(0).unwrap();
    assert!(chart.series_apply_options_json(0, &serialized));
    assert_eq!(chart.series_options_json(0).unwrap(), serialized);

    // Malformed patches and unknown types/ids report failure.
    assert!(!chart.series_apply_price_format_json(0, "{ nope"));
    assert!(!chart.series_apply_price_format_json(0, r#"{"type": "nope"}"#));
    assert!(!chart.series_apply_price_format_json(0, r#"{"precision": 2}"#));
    assert!(!chart.series_apply_price_format_json(999, r#"{"type": "price"}"#));
}

#[test]
fn price_format_drives_last_value_label_and_ticks() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Line;
    let times = [1.0, 2.0, 3.0];
    let values = [1500.0, 2500.0, 2_500_000.0];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();

    let label_texts = |chart: &mut ChartEngine| {
        chart
            .build_axis_frame(80.0, |t| t.len() as f64 * 7.0)
            .labels
            .into_iter()
            .map(|l| l.text)
            .collect::<Vec<_>>()
    };

    // Default: two-decimal price labels.
    let texts = label_texts(&mut chart);
    assert!(texts.iter().any(|t| t == "2500000.00"), "{texts:?}");

    // Volume format: K/M/B suffixes on the series' last-value label and the axis ticks
    // (the series is the scale's primary source).
    assert!(chart.series_apply_price_format_json(0, r#"{"type": "volume", "precision": 1}"#));
    let texts = label_texts(&mut chart);
    assert!(texts.iter().any(|t| t == "2.5M"), "{texts:?}");
    assert!(!texts.iter().any(|t| t == "2500000.00"), "{texts:?}");

    // Percent format: a % sign (precision as decimal digits; see the reference-quirk note at
    // `format_with_price_format`).
    assert!(chart.series_apply_price_format_json(0, r#"{"type": "percent", "precision": 0}"#));
    let texts = label_texts(&mut chart);
    assert!(texts.iter().any(|t| t.ends_with('%')), "{texts:?}");

    // Price format with four decimals.
    assert!(chart.series_apply_price_format_json(
        0,
        r#"{"type": "price", "precision": 4, "min_move": 0.0001}"#
    ));
    let texts = label_texts(&mut chart);
    assert!(texts.iter().any(|t| t == "2500000.0000"), "{texts:?}");
}

#[test]
fn price_format_custom_fn_invocation_and_clearing() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Line;
    let times = [1.0, 2.0, 3.0];
    let values = [10.0, 11.0, 12.0];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();

    let label_texts = |chart: &mut ChartEngine| {
        chart
            .build_axis_frame(80.0, |t| t.len() as f64 * 7.0)
            .labels
            .into_iter()
            .map(|l| l.text)
            .collect::<Vec<_>>()
    };

    // The custom fn drives the series' labels (reference priceFormat.formatter).
    assert!(chart.set_series_price_formatter(0, Box::new(|price| Some(format!("P{price:.1}"))),));
    let texts = label_texts(&mut chart);
    assert!(texts.iter().any(|t| t == "P12.0"), "{texts:?}");
    // Custom serializes without the fn.
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(
        options["price_format"],
        serde_json::json!({"type": "custom", "min_move": 0.01})
    );

    // `{type:"custom"}` keeps the installed fn (reference merge of a partial priceFormat patch).
    assert!(chart.series_apply_price_format_json(0, r#"{"type": "custom", "min_move": 0.5}"#));
    let texts = label_texts(&mut chart);
    assert!(texts.iter().any(|t| t == "P12.0"), "{texts:?}");

    // A declining fn (None return, e.g. a throw at the JS boundary) falls back to built-in.
    assert!(chart.set_series_price_formatter(0, Box::new(|_| None)));
    let texts = label_texts(&mut chart);
    assert!(texts.iter().any(|t| t == "12.00"), "{texts:?}");

    // Switching to a non-custom type clears the fn: back to custom, no fn remains.
    assert!(chart.series_apply_price_format_json(0, r#"{"type": "price"}"#));
    assert!(chart.series_apply_price_format_json(0, r#"{"type": "custom"}"#));
    let texts = label_texts(&mut chart);
    assert!(texts.iter().any(|t| t == "12.00"), "{texts:?}");
    assert!(!texts.iter().any(|t| t == "P12.0"), "{texts:?}");
}

#[test]
fn price_format_labels_follow_their_owning_series() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Line;
    let times = [1.0, 2.0, 3.0];
    let values = [10.0, 11.0, 12.0];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    let second = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(second, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();

    // The second series formats with four decimals; the primary stays at the default.
    assert!(chart.series_apply_price_format_json(
        second,
        r#"{"type": "price", "precision": 4, "min_move": 0.0001}"#
    ));
    // Its price-line label uses its OWN format.
    let line_id = chart.create_price_line(
        second,
        11.0,
        Color::rgb(0, 0, 0),
        1,
        origin_render::draw_list::LineStyle::Solid,
        "",
    );
    assert!(line_id > 0);
    let axis = chart.build_axis_frame(80.0, |t| t.len() as f64 * 7.0);
    let boxed: Vec<_> = axis
        .labels
        .iter()
        .filter(|l| l.background.is_some())
        .map(|l| l.text.clone())
        .collect();
    assert!(
        boxed.iter().any(|t| t == "12.0000"),
        "second series' own last-value label: {boxed:?}"
    );
    assert!(
        boxed.iter().any(|t| t == "11.0000"),
        "second series' price-line label: {boxed:?}"
    );
    assert!(
        boxed.iter().any(|t| t == "12.00"),
        "primary series' default-format label: {boxed:?}"
    );

    // The crosshair price label uses the label source's format: restore the second series to
    // the default, give the PRIMARY series the four-decimal format, and magnet-snap the
    // crosshair to the 11.0 close â€” "11.0000" is a text no other label produces.
    assert!(chart.series_apply_price_format_json(
        second,
        r#"{"type": "price", "precision": 2, "min_move": 0.01}"#
    ));
    assert!(chart.series_apply_price_format_json(
        0,
        r#"{"type": "price", "precision": 4, "min_move": 0.0001}"#
    ));
    chart.crosshair_mode = CrosshairMode::Magnet;
    let y11 = chart.series_price_to_coordinate(0, 11.0).unwrap();
    let x1 = chart.time_to_coordinate(2.0).unwrap();
    chart.crosshair = Some((x1, y11));
    let axis = chart.build_axis_frame(80.0, |t| t.len() as f64 * 7.0);
    assert!(
        axis.labels
            .iter()
            .any(|l| l.background.is_some() && l.text == "11.0000"),
        "crosshair label in the primary (label source) series' format"
    );
    assert!(
        axis.labels
            .iter()
            .any(|l| l.background.is_some() && l.text == "12.00"),
        "second series back to the default format"
    );
}

// ---- wave: shiftVisibleRangeOnNewBar / whitespace / pop / lastValueData / programmatic ----
// ---- crosshair / locale / series order / verbatim colors (reference ports; see item refs)   ----

/// Install `n` ascending real bars (close = 100 + i) on series 0 and lay out the scale.
fn install_bars(chart: &mut ChartEngine, n: usize) {
    chart.time_scale.set_width(800.0);
    let times: Vec<f64> = (1..=n).map(|i| i as f64).collect();
    let values: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
}

#[test]
fn new_bar_shift_follows_at_the_right_edge() {
    // reference chart-model.ts:968-983: last bar visible + shiftVisibleRangeOnNewBar (default
    // true) -> no right-offset compensation, the view follows the new bar.
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    install_bars(&mut chart, 10);
    assert_eq!(chart.right_offset(), 0.0);
    chart.update_series_bar(0, 11.0, [109.0, 110.0, 108.0, 109.0]);
    assert_eq!(chart.right_offset(), 0.0, "right edge follows new bars");
    assert_eq!(chart.time_scale.base_index(), 10);
}

#[test]
fn new_bar_compensation_keeps_bars_when_scrolled_back() {
    // Scrolled into the past (last bar not visible): the right offset compensates by the
    // number of new bars so the same bars stay in view (no drift).
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    install_bars(&mut chart, 10);
    chart.set_right_offset(-5.0);
    let before = chart.time_scale.visible_strict_range().unwrap();
    chart.update_series_bar(0, 11.0, [109.0, 110.0, 108.0, 109.0]);
    assert_eq!(chart.right_offset(), -6.0);
    let after = chart.time_scale.visible_strict_range().unwrap();
    assert_eq!(
        (before.left(), before.right()),
        (after.left(), after.right()),
        "same bars stay in view after compensation"
    );
}

#[test]
fn new_bar_compensation_when_shift_disabled_at_the_edge() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    install_bars(&mut chart, 10);
    chart.set_shift_visible_range_on_new_bar(false);
    chart.update_series_bar(0, 11.0, [109.0, 110.0, 108.0, 109.0]);
    assert_eq!(chart.right_offset(), -1.0);
    // After the compensation the last bar is outside the visible range, so the next append
    // keeps compensating even with the option back on (reference parity: the view stays put).
    chart.set_shift_visible_range_on_new_bar(true);
    chart.update_series_bar(0, 12.0, [110.0, 111.0, 109.0, 110.0]);
    assert_eq!(chart.right_offset(), -2.0);
}

#[test]
fn whitespace_replacement_shift_is_gated_by_the_option() {
    let nan = f64::NAN;
    // 10 real bars plus an explicit whitespace time point at 11.
    let build = |chart: &mut ChartEngine| {
        chart.time_scale.set_width(800.0);
        let times: Vec<f64> = (1..=11).map(|i| i as f64).collect();
        let values: Vec<f64> = (0..10)
            .map(|i| 100.0 + i as f64)
            .chain(std::iter::once(nan))
            .collect();
        chart
            .set_series_data(0, &times, &values, &values, &values, &values)
            .unwrap();
        assert_eq!(chart.time_scale.base_index(), 9, "base skips trailing ws");
    };

    // Default (allow=false): replacing the whitespace at the right edge compensates.
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    build(&mut chart);
    chart.update_series_bar(0, 11.0, [110.0, 111.0, 109.0, 110.0]);
    assert_eq!(chart.right_offset(), -1.0);

    // allow=true: the view follows the replacement like a real new bar.
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    build(&mut chart);
    chart.set_allow_shift_visible_range_on_whitespace_replacement(true);
    chart.update_series_bar(0, 11.0, [110.0, 111.0, 109.0, 110.0]);
    assert_eq!(chart.right_offset(), 0.0);
}

#[test]
fn time_scale_shift_options_route_via_json_and_round_trip() {
    let mut chart = ChartEngine::new(300.0, 200.0, 1.0);
    let options: serde_json::Value =
        serde_json::from_str(&chart.time_scale_options_json()).unwrap();
    // reference defaults (time-scale-options-defaults.ts:17-18)
    assert_eq!(options["shift_visible_range_on_new_bar"], true);
    assert_eq!(
        options["allow_shift_visible_range_on_whitespace_replacement"],
        false
    );
    chart
        .apply_options(
            r#"{"timeScale":{"shiftVisibleRangeOnNewBar":false,"allowShiftVisibleRangeOnWhitespaceReplacement":true}}"#,
        )
        .unwrap();
    let options: serde_json::Value =
        serde_json::from_str(&chart.time_scale_options_json()).unwrap();
    assert_eq!(options["shift_visible_range_on_new_bar"], false);
    assert_eq!(
        options["allow_shift_visible_range_on_whitespace_replacement"],
        true
    );
}

// ---- whitespace data items (reference {time}-only rows) ----

#[test]
fn whitespace_rows_draw_nothing_but_keep_their_slots() {
    use origin_render::draw_list::Prim;
    let nan = f64::NAN;
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Line;
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0, 4.0, 5.0],
            &[10.0, 11.0, nan, 12.0, 13.0],
            &[10.0, 11.0, nan, 12.0, 13.0],
            &[10.0, 11.0, nan, 12.0, 13.0],
            &[10.0, 11.0, nan, 12.0, 13.0],
        )
        .unwrap();
    // the whitespace time keeps its merged slot (reference keeps the time-scale point)
    assert_eq!(chart.data.merged_times(), &[1, 2, 3, 4, 5]);
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let frame = chart.build_frame();
    let points: u32 = frame.panes[0]
        .main
        .iter()
        .map(|p| match p {
            Prim::Polyline { point_count, .. } => *point_count,
            _ => 0,
        })
        .sum();
    // the line skips the whitespace row and connects the four real bars across the gap
    assert_eq!(points, 4);
    // and the autoscale ignores it
    let range = chart.panes[0].price_scale.price_range().unwrap();
    assert_eq!(range.min_value(), 10.0);
    assert_eq!(range.max_value(), 13.0);
}

#[test]
fn whitespace_update_replaces_the_bar_and_skips_last_value() {
    let nan = f64::NAN;
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    install_bars(&mut chart, 3);
    chart.fit_content();
    // reference `series.update` with a {time}-only item replaces the last bar with whitespace.
    assert!(chart.update_series_bar(0, 3.0, [nan, nan, nan, nan]));
    let data = chart.series_data(0);
    assert_eq!(data.len(), 3);
    assert!(data[2].close.is_nan());
    // last-value label tracks the last real bar (close of bar 2 = 101)
    let axis = chart.build_axis_frame(80.0, |t| t.len() as f64 * 7.0);
    let texts: Vec<String> = axis.labels.iter().map(|l| l.text.clone()).collect();
    assert!(
        texts.iter().any(|t| t == "101.00"),
        "last-value label skips whitespace: {texts:?}"
    );
    assert!(!texts.iter().any(|t| t == "102.00"));
}

#[test]
fn magnet_treats_whitespace_as_no_bar() {
    use origin_render::draw_list::Prim;
    let nan = f64::NAN;
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0],
            &[10.0, nan, 12.0],
            &[10.0, nan, 12.0],
            &[10.0, nan, 12.0],
            &[10.0, nan, 12.0],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.crosshair_mode = CrosshairMode::Magnet;
    // Cursor over the whitespace bar (time 2): no candidate, the horizontal line stays at
    // the raw cursor y instead of snapping to a bar price. (The crosshair's HLine is told
    // apart from the built-in last-price line by the crosshair grey — both are Dashed now.)
    let crosshair_hline_y = |frame: &ChartFrame| {
        frame.panes[0].main.iter().find_map(|p| match p {
            Prim::HLine { y, color, .. } if *color == Color::rgb(0x95, 0x98, 0xa1) => Some(*y),
            _ => None,
        })
    };
    let x_ws = chart.time_to_coordinate(2.0).unwrap();
    chart.crosshair = Some((x_ws, 10.0));
    let frame = chart.build_frame();
    assert_eq!(
        crosshair_hline_y(&frame),
        Some(10),
        "whitespace: no magnet snap"
    );
    // Over the real bar at time 3 the magnet snaps to its close coordinate.
    let x_bar = chart.time_to_coordinate(3.0).unwrap();
    chart.crosshair = Some((x_bar, 10.0));
    let snapped = chart.series_price_to_coordinate(0, 12.0).unwrap();
    let frame = chart.build_frame();
    assert_eq!(crosshair_hline_y(&frame), Some(snapped.round() as i32));
}

// ---- series pop / lastValueData / priceFormatter ----

#[test]
fn series_pop_removes_tail_and_shifts_point_colors() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    install_bars(&mut chart, 5);
    assert!(chart.set_series_point_colors(0, Some(vec![11, 22, 33, 44, 55]), None, None));
    assert_eq!(chart.series_pop(0, 0), Some(5), "count 0 is a no-op");
    assert_eq!(chart.series_pop(0, 2), Some(3));
    assert_eq!(
        chart
            .data
            .point_color(0, origin_core::model::data_layer::PointColorChannel::Body, 0),
        Some(11)
    );
    assert_eq!(
        chart
            .data
            .point_color(0, origin_core::model::data_layer::PointColorChannel::Body, 2),
        Some(33)
    );
    assert_eq!(
        chart
            .data
            .point_color(0, origin_core::model::data_layer::PointColorChannel::Body, 3),
        None
    );
    assert_eq!(chart.data.merged_times(), &[1, 2, 3]);
    // clamp to the data length; unknown/removed ids report None
    assert_eq!(chart.series_pop(0, 99), Some(0));
    assert_eq!(chart.series_pop(42, 1), None);
}

#[test]
fn series_last_value_data_global_visible_and_whitespace() {
    let nan = f64::NAN;
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart
        .set_series_data(
            0,
            &[1.0, 2.0, 3.0, 4.0],
            &[10.0, 20.0, 30.0, nan],
            &[10.0, 20.0, 30.0, nan],
            &[10.0, 20.0, 30.0, nan],
            &[10.0, 20.0, 30.0, nan],
        )
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();

    // global last: skips the trailing whitespace bar
    let global: serde_json::Value =
        serde_json::from_str(&chart.series_last_value_data(0, true).unwrap()).unwrap();
    assert_eq!(global["value"], 30.0);
    assert_eq!(global["formatted"], "30.00");
    assert_eq!(global["time"], 3);
    // visible last with the right edge at index 1: the bar at time 2
    chart.set_right_offset(-1.0);
    let visible: serde_json::Value =
        serde_json::from_str(&chart.series_last_value_data(0, false).unwrap()).unwrap();
    assert_eq!(visible["value"], 20.0);
    assert_eq!(visible["time"], 2);
    // unknown id -> None ("" at the wasm boundary)
    assert!(chart.series_last_value_data(42, true).is_none());
}

#[test]
fn series_format_price_uses_the_resolved_price_format() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    install_bars(&mut chart, 2);
    // built-in default (price, 2 decimals)
    assert_eq!(
        chart.series_format_price(0, 12.345).as_deref(),
        Some("12.35")
    );
    // per-series precision
    assert!(chart
        .series_apply_price_format_json(0, r#"{"type":"price","precision":4,"min_move":0.0001}"#));
    assert_eq!(
        chart.series_format_price(0, 12.345).as_deref(),
        Some("12.3450")
    );
    // volume / percent built-ins
    assert!(chart.series_apply_price_format_json(0, r#"{"type":"volume"}"#));
    assert_eq!(
        chart.series_format_price(0, 1500.0).as_deref(),
        Some("1.5K")
    );
    assert!(chart.series_apply_price_format_json(0, r#"{"type":"percent","precision":1}"#));
    assert_eq!(
        chart.series_format_price(0, 12.345).as_deref(),
        Some("12.3%")
    );
    // custom fn first, declining fn -> chart formatter fallback -> built-in
    chart.set_series_price_formatter(0, Box::new(|v| Some(format!("px:{v}"))));
    assert_eq!(chart.series_format_price(0, 1.5).as_deref(), Some("px:1.5"));
    chart.set_series_price_formatter(0, Box::new(|_| None));
    assert_eq!(chart.series_format_price(0, 1.5).as_deref(), Some("1.50"));
    chart.set_price_formatter(Some(Box::new(|v| Some(format!("chart:{v}")))));
    assert_eq!(
        chart.series_format_price(0, 1.5).as_deref(),
        Some("chart:1.5")
    );
    assert!(chart.series_format_price(42, 1.5).is_none());
}

// ---- programmatic crosshair ----

#[test]
fn crosshair_position_set_reject_and_clear() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    install_bars(&mut chart, 5);
    chart.fit_content();
    // lay the panes/scales out (a frame build does layout + autoscale)
    chart.build_frame();
    // time resolves to a bar: x is the bar coordinate, y the price on the series' scale
    let x = chart.time_to_coordinate(3.0).unwrap();
    let y = chart.series_price_to_coordinate(0, 102.0).unwrap();
    assert!(chart.set_crosshair_position(102.0, 3.0, 0));
    let (cx, cy) = chart.crosshair.unwrap();
    assert_eq!(cx, x);
    assert_eq!(cy, y);
    // a non-bar time is rejected and leaves the previous position untouched
    assert!(!chart.set_crosshair_position(102.0, 3.5, 0));
    assert_eq!(chart.crosshair, Some((x, y)));
    // unknown series / non-finite price rejected
    assert!(!chart.set_crosshair_position(102.0, 3.0, 42));
    assert!(!chart.set_crosshair_position(f64::NAN, 3.0, 0));
    // clear drops the stored position
    chart.clear_crosshair_position();
    assert_eq!(chart.crosshair, None);
    // a following frame draws it: set again and check the crosshair prims exist
    assert!(chart.set_crosshair_position(102.0, 3.0, 0));
    let frame = chart.build_frame();
    assert!(frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, origin_render::draw_list::Prim::VLine { .. })));
}

// ---- locale / dateFormat ----

/// Crosshair time-label text of the bar at `time`, with the crosshair parked on it.
fn crosshair_time_label(chart: &mut ChartEngine, time: f64) -> Option<String> {
    let x = chart.time_to_coordinate(time)?;
    chart.crosshair = Some((x, 10.0));
    chart
        .build_axis_frame(80.0, |t| t.len() as f64 * 7.0)
        .labels
        .into_iter()
        .find(|l| l.background.is_some() && l.midpoint == AxisTextMidpoint::StableTime)
        .map(|l| l.text)
}

#[test]
fn date_format_drives_the_crosshair_time_label() {
    // 2018-06-25T14:30:45Z
    let ts = 1_529_937_045.0;
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.set_time_visible(false); // date-only labels for these assertions
    chart
        .set_series_data(0, &[ts], &[10.0], &[10.0], &[10.0], &[10.0])
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    // reference default `dd MMM 'yy`
    assert_eq!(
        crosshair_time_label(&mut chart, ts).as_deref(),
        Some("25 Jun '18")
    );
    chart.set_date_format("yyyy-MM-dd");
    assert_eq!(
        crosshair_time_label(&mut chart, ts).as_deref(),
        Some("2018-06-25")
    );
    chart.set_date_format("MMMM d, yyyy");
    assert_eq!(
        crosshair_time_label(&mut chart, ts).as_deref(),
        Some("June 25, 2018")
    );
    // options JSON routing (reference `applyOptions({ localization })`)
    chart
        .apply_options(r#"{"localization":{"dateFormat":"d/M/yy"}}"#)
        .unwrap();
    assert_eq!(
        crosshair_time_label(&mut chart, ts).as_deref(),
        Some("25/6/18")
    );
}

#[test]
fn injected_locale_month_names_drive_labels() {
    let ts = 1_529_937_045.0; // 2018-06-25T14:30:45Z
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.set_time_visible(false);
    chart
        .set_series_data(0, &[ts], &[10.0], &[10.0], &[10.0], &[10.0])
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let mut short: [String; 12] = Default::default();
    let mut long: [String; 12] = Default::default();
    for (i, name) in [
        "Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez",
    ]
    .iter()
    .enumerate()
    {
        short[i] = name.to_string();
        long[i] = format!("{name}ius");
    }
    chart.set_month_names(short, long);
    chart.set_date_format("dd MMM yyyy");
    assert_eq!(
        crosshair_time_label(&mut chart, ts).as_deref(),
        Some("25 Jun 2018")
    );
    chart.set_date_format("MMMM yyyy");
    assert_eq!(
        crosshair_time_label(&mut chart, ts).as_deref(),
        Some("Junius 2018")
    );
}

// ---- primary-series removal + series ordering ----

#[test]
fn removing_series_zero_falls_back_to_the_first_live_series() {
    use origin_render::draw_list::Prim;
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    install_bars(&mut chart, 3);
    chart.series[0].last_price_animation = true;
    let second = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(
            second,
            &[1.0, 2.0, 3.0],
            &[5.0, 6.0, 7.0],
            &[5.0, 6.0, 7.0],
            &[5.0, 6.0, 7.0],
            &[5.0, 6.0, 7.0],
        )
        .unwrap();
    chart.series[second].last_price_animation = true;
    chart.time_scale.set_width(800.0);
    chart.fit_content();

    assert!(chart.remove_series(0));
    assert_eq!(chart.primary_series().map(|s| s.id), Some(second));

    // Crosshair labels still resolve against the remaining series...
    let ts = 2.0;
    assert!(crosshair_time_label(&mut chart, ts).is_some());
    // ...the last-price pulse follows the new primary...
    let frame = chart.build_frame();
    assert!(
        frame.panes[0]
            .main
            .iter()
            .any(|p| matches!(p, Prim::Circle { .. })),
        "pulse falls back to the first visible non-removed series"
    );
    // ...and last-value labels come from the remaining series (close 7).
    let axis = chart.build_axis_frame(80.0, |t| t.len() as f64 * 7.0);
    assert!(axis
        .labels
        .iter()
        .any(|l| l.background.is_some() && l.text == "7.00"));
}

#[test]
fn series_order_round_trips_and_rejects_bad_permutations() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let s1 = chart.add_series(SeriesKind::Line);
    let s2 = chart.add_series(SeriesKind::Line);
    assert_eq!(chart.series_order_json(), "[0,1,2]");
    assert!(chart.set_series_order(vec![s2, 0, s1]));
    assert_eq!(chart.series_order_json(), "[2,0,1]");
    // wrong length, duplicate, unknown id: all rejected without state change
    assert!(!chart.set_series_order(vec![0, 1]));
    assert!(!chart.set_series_order(vec![0, 1, 2, 3]));
    assert!(!chart.set_series_order(vec![0, 1, 1]));
    assert!(!chart.set_series_order(vec![0, 1, 42]));
    assert_eq!(chart.series_order_json(), "[2,0,1]");
    // removed series leave the order
    assert!(chart.remove_series(s1));
    assert_eq!(chart.series_order_json(), "[2,0]");
    assert!(chart.set_series_order(vec![0, 2]));
    assert_eq!(chart.series_order_json(), "[0,2]");
}

#[test]
fn series_order_controls_paint_order() {
    use origin_render::draw_list::Prim;
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    install_bars(&mut chart, 3);
    chart.series[0].kind = SeriesKind::Line;
    chart.series[0].line_color = Some("#ff0000".to_string());
    let second = chart.add_series(SeriesKind::Line);
    chart
        .set_series_data(
            second,
            &[1.0, 2.0, 3.0],
            &[5.0, 6.0, 7.0],
            &[5.0, 6.0, 7.0],
            &[5.0, 6.0, 7.0],
            &[5.0, 6.0, 7.0],
        )
        .unwrap();
    chart.series[second].line_color = Some("#0000ff".to_string());
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    let poly_colors = |chart: &mut ChartEngine| {
        chart.build_frame().panes[0]
            .main
            .iter()
            .filter_map(|p| match p {
                Prim::Polyline { color, .. } => Some(color.to_hex()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    // added order: the second series paints last (on top)
    assert_eq!(poly_colors(&mut chart), ["#ff0000", "#0000ff"]);
    assert!(chart.set_series_order(vec![second, 0]));
    assert_eq!(poly_colors(&mut chart), ["#0000ff", "#ff0000"]);
}

// ---- verbatim CSS color storage (item 2.13b) ----

#[test]
fn color_options_round_trip_verbatim() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let fields: Vec<(&str, &str)> = vec![
        ("color", "#AaBbCc"),
        ("up_color", "#FF0000"),
        ("down_color", "rgb(1, 2, 3)"),
        ("wick_up_color", "rgba(4,5,6,0.5)"),
        ("wick_down_color", "#111"),
        ("border_up_color", "#222233"),
        ("border_down_color", "rebeccapurple"),
        ("area_top_color", "#12345678"),
        ("area_bottom_color", "rgba(9, 8, 7, 0.25)"),
    ];
    {
        let s = &mut chart.series[0];
        s.line_color = Some("#AaBbCc".to_string());
        s.up_color = Some("#FF0000".to_string());
        s.down_color = Some("rgb(1, 2, 3)".to_string());
        s.wick_up_color = Some("rgba(4,5,6,0.5)".to_string());
        s.wick_down_color = Some("#111".to_string());
        s.border_up_color = Some("#222233".to_string());
        s.border_down_color = Some("rebeccapurple".to_string());
        s.area_top_color = Some("#12345678".to_string());
        s.area_bottom_color = Some("rgba(9, 8, 7, 0.25)".to_string());
    }
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    for (key, applied) in &fields {
        assert_eq!(&options[key], applied, "verbatim round-trip for {key}");
    }
    // "" clears back to the follow/default state
    chart.series[0].up_color = None;
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(options["up_color"], "");
}

#[test]
fn unparseable_verbatim_colors_fall_back_at_render_time() {
    use origin_render::draw_list::Prim;
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    install_bars(&mut chart, 3);
    chart.series[0].up_color = Some("rebeccapurple".to_string()); // stored, unparseable
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    // the up bars render with the reference default UP color (0x26a69a), not the stored string
    let up = Color::rgb(0x26, 0xa6, 0x9a);
    let frame = chart.build_frame();
    assert!(frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, Prim::Rect { color, .. } if *color == up)));
    // but options() still returns the applied string verbatim
    let options: serde_json::Value =
        serde_json::from_str(&chart.series_options_json(0).unwrap()).unwrap();
    assert_eq!(options["up_color"], "rebeccapurple");
}

// ---- wave: reference v5 panes API + scale/time cosmetics + background gradient + separator ----
// ---- hover (chart-api.ts/pane-api.ts, price/time-scale options, pane-separator.ts)    ----

#[test]
fn panes_add_remove_swap_move_and_series_movement() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    install_bars(&mut chart, 5);
    let second = chart.add_series(SeriesKind::Line);
    let third = chart.add_series(SeriesKind::Histogram);

    // addPane appends and reports the new index (reference chart-api.ts addPane).
    let pane1 = chart.add_pane(false);
    assert_eq!(pane1, 1);
    let pane2 = chart.add_pane(true);
    assert_eq!(pane2, 2);
    assert!(chart.pane_preserve_empty(pane2));
    assert!(!chart.pane_preserve_empty(pane1));

    chart.set_series_pane(second, 1, 1.0);
    chart.set_series_pane(third, 2, 1.0);
    assert_eq!(chart.pane_series_ids(0), vec![0]);
    assert_eq!(chart.pane_series_ids(1), vec![second]);
    assert_eq!(chart.pane_series_ids(2), vec![third]);
    // Render order within a pane: bottom first (reference pane.ts orderedSources).
    let fourth = chart.add_series(SeriesKind::Area);
    chart.set_series_pane(fourth, 1, 1.0);
    assert_eq!(chart.pane_series_ids(1), vec![second, fourth]);

    // swapPanes: the panes trade places with their series assignments and stretch factors.
    chart.panes[1].stretch_factor = 2.0;
    assert!(chart.swap_panes(1, 2));
    assert_eq!(chart.pane_series_ids(1), vec![third]);
    assert_eq!(chart.pane_series_ids(2), vec![second, fourth]);
    assert_eq!(chart.panes[2].stretch_factor, 2.0);
    assert!(chart.pane_preserve_empty(1), "preserve flag rides along");
    assert!(!chart.swap_panes(1, 7), "stale index rejected");

    // movePane (pane-api.ts moveTo): the pane rides to its new index with its series.
    assert!(chart.move_pane(2, 0));
    assert_eq!(chart.pane_series_ids(0), vec![second, fourth]);
    assert_eq!(chart.pane_series_ids(1), vec![0]);
    assert_eq!(chart.pane_series_ids(2), vec![third]);
    assert!(chart.move_pane(0, 0), "same-index move is a no-op success");
    assert!(!chart.move_pane(0, 9), "stale target rejected");

    // removePane orphans the pane's series (reference paneForSource -> null): they keep their
    // data but render/scale nowhere; panes below shift one index up.
    assert!(chart.remove_pane(0));
    assert_eq!(chart.panes.len(), 2);
    assert_eq!(chart.series[second].pane_index, PANELESS);
    assert_eq!(chart.series[fourth].pane_index, PANELESS);
    assert_eq!(chart.pane_series_ids(0), vec![0]);
    assert_eq!(chart.pane_series_ids(1), vec![third]);
    // A pane-less series re-assigned to a live pane renders again (ids in z-order, not
    // assignment order — reference pane.ts orderedSources).
    chart.set_series_pane(second, 1, 1.0);
    assert_eq!(chart.pane_series_ids(1), vec![second, third]);
    assert!(!chart.remove_pane(9), "stale index rejected");
    chart.remove_pane(0);
    assert!(
        !chart.remove_pane(0),
        "the last remaining pane cannot be removed"
    );
    assert_eq!(chart.panes.len(), 1);
}

#[test]
fn preserve_empty_pruning_on_series_removal_and_move_out() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    install_bars(&mut chart, 5);
    let second = chart.add_series(SeriesKind::Line);
    chart.set_series_pane(second, 1, 1.0);
    assert_eq!(chart.panes.len(), 2);

    // Moving the series back collapses the emptied, non-preserved pane (reference
    // chart-model.ts `_cleanupIfPaneIsEmpty` on moveSeriesToPane).
    chart.set_series_pane(second, 0, 1.0);
    assert_eq!(chart.panes.len(), 1, "empty non-preserved pane collapses");

    // A preserved pane survives both the move-out and a series removal.
    chart.set_series_pane(second, 1, 1.0);
    chart.pane_set_preserve_empty(1, true);
    chart.set_series_pane(second, 0, 1.0);
    assert_eq!(chart.panes.len(), 2, "preserved empty pane stays");
    chart.set_series_pane(second, 1, 1.0);
    chart.pane_set_preserve_empty(1, false);
    assert!(chart.remove_series(second));
    assert_eq!(
        chart.panes.len(),
        1,
        "removal prunes the unpreserved empty pane"
    );
}

#[test]
fn price_scale_apply_options_json_round_trip_and_chart_group_routing() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    install_bars(&mut chart, 5);

    // Per-scale JSON patch (snake_case): the five cosmetics plus the scale-math keys.
    assert!(chart.price_scale_apply_options_json(
        0,
        PriceScaleTarget::Right,
        r##"{"mode":1,"auto_scale":false,"invert_scale":true,"scale_margins":{"top":0.3,"bottom":0.2},"align_labels":false,"ticks_visible":true,"entire_text_only":true,"minimum_width":80,"text_color":"#ff0000"}"##,
    ));
    let options: serde_json::Value = serde_json::from_str(
        &chart
            .price_scale_options_json(0, PriceScaleTarget::Right)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(options["mode"], 1);
    assert_eq!(options["auto_scale"], false);
    assert_eq!(options["invert_scale"], true);
    assert_eq!(options["scale_margins"]["top"], 0.3);
    assert_eq!(options["scale_margins"]["bottom"], 0.2);
    assert_eq!(options["align_labels"], false);
    assert_eq!(options["ticks_visible"], true);
    assert_eq!(options["entire_text_only"], true);
    assert_eq!(options["minimum_width"], 80.0);
    assert_eq!(options["text_color"], "#ff0000");

    // reference defaults on an untouched scale; a stale pane answers None/false.
    let defaults: serde_json::Value = serde_json::from_str(
        &chart
            .price_scale_options_json(0, PriceScaleTarget::Left)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(defaults["align_labels"], true);
    assert_eq!(defaults["ticks_visible"], false);
    assert_eq!(defaults["entire_text_only"], false);
    assert_eq!(defaults["minimum_width"], 0.0);
    assert_eq!(defaults["text_color"], serde_json::Value::Null);
    assert!(chart
        .price_scale_options_json(9, PriceScaleTarget::Right)
        .is_none());
    assert!(!chart.price_scale_apply_options_json(
        9,
        PriceScaleTarget::Right,
        r#"{"ticks_visible":true}"#
    ));
    assert!(!chart.price_scale_apply_options_json(0, PriceScaleTarget::Right, "{ nope"));

    // `text_color: null` / `""` clears back to following `layout.textColor`.
    assert!(chart.price_scale_apply_options_json(
        0,
        PriceScaleTarget::Right,
        r#"{"text_color":null}"#
    ));
    let cleared: serde_json::Value = serde_json::from_str(
        &chart
            .price_scale_options_json(0, PriceScaleTarget::Right)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(cleared["text_color"], serde_json::Value::Null);

    // Chart-group routing (reference pane.ts applyScaleOptions): a `rightPriceScale` patch
    // applies the five camelCase keys to every pane's right scale, present keys only.
    let extra = chart.add_series(SeriesKind::Line);
    chart.set_series_pane(extra, 1, 1.0);
    chart
        .apply_options(
            r##"{"rightPriceScale":{"ticksVisible":true,"minimumWidth":64,"textColor":"#00ff00"},"leftPriceScale":{"alignLabels":false}}"##,
        )
        .unwrap();
    for pane in &chart.panes {
        assert!(pane.price_scale.options().ticks_visible);
        assert_eq!(pane.price_scale.options().minimum_width, 64.0);
        assert_eq!(
            pane.price_scale.options().text_color.as_deref(),
            Some("#00ff00")
        );
        assert!(!pane.left_scale.options().align_labels);
    }
    // A pane added afterwards inherits the merged chart-level cosmetics (reference Pane
    // constructor `_createPriceScale` from the chart options).
    let pane_index = chart.add_pane(false);
    assert!(chart.panes[pane_index].price_scale.options().ticks_visible);
    assert!(!chart.panes[pane_index].left_scale.options().align_labels);
}

#[test]
fn time_axis_options_height_floor_visibility_collapse_and_char_length() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    install_bars(&mut chart, 200);

    // reference chart-widget.ts `Math.max(optimalHeight(), minimumHeight)`: the auto 28px
    // strip is floored at `minimumHeight`; `visible:false` collapses it to zero.
    assert_eq!(chart.time_axis_height(), 28.0);
    chart.set_time_axis_minimum_height(40.0);
    assert_eq!(chart.time_axis_height(), 40.0);
    chart.set_time_axis_minimum_height(10.0);
    assert_eq!(
        chart.time_axis_height(),
        28.0,
        "floor never shrinks the auto height"
    );
    chart.set_time_axis_visible(false);
    assert_eq!(
        chart.time_axis_height(),
        0.0,
        "hidden strip reserves nothing"
    );
    chart.set_time_axis_visible(true);
    assert_eq!(chart.time_axis_height(), 28.0);
    // Invalid heights are ignored (NaN / negative keep the current value).
    chart.set_time_axis_minimum_height(f64::NAN);
    chart.set_time_axis_minimum_height(-5.0);
    assert_eq!(chart.time_axis_height(), 28.0);

    // `timeVisible` stays label semantics only: it never reserves the strip.
    chart.set_time_visible(false);
    assert_eq!(chart.time_axis_height(), 28.0);
    chart.set_time_visible(true);

    // tickMarkMaxCharacterLength widens/narrows the mark spacing; 0 restores the
    // default 8 (reference time-scale.ts `|| defaultTickMarkMaxCharacterLength`).
    let marks = |chart: &mut ChartEngine| {
        let width = (12.0 + 4.0) * 5.0 / 8.0 * f64::from(chart.tick_mark_max_character_length);
        chart.time_marks(width).len()
    };
    let default_count = marks(&mut chart);
    chart.set_tick_mark_max_character_length(2);
    let narrow_count = marks(&mut chart);
    assert!(
        narrow_count > default_count,
        "shorter cap packs marks denser ({narrow_count} vs {default_count})"
    );
    chart.set_tick_mark_max_character_length(16);
    assert!(
        marks(&mut chart) < default_count,
        "wider cap thins marks out"
    );
    chart.set_tick_mark_max_character_length(0);
    assert_eq!(chart.tick_mark_max_character_length, 8);
    assert_eq!(marks(&mut chart), default_count);

    // All four route through the chart-options `timeScale` group and round-trip.
    chart
        .apply_options(
            r#"{"timeScale":{"visible":false,"ticksVisible":true,"minimumHeight":32,"tickMarkMaxCharacterLength":5}}"#,
        )
        .unwrap();
    assert!(!chart.time_axis_visible);
    assert!(chart.time_ticks_visible);
    assert_eq!(chart.time_axis_minimum_height, 32.0);
    assert_eq!(chart.tick_mark_max_character_length, 5);
    let options: serde_json::Value =
        serde_json::from_str(&chart.time_scale_options_json()).unwrap();
    assert_eq!(options["visible"], false);
    assert_eq!(options["ticks_visible"], true);
    assert_eq!(options["minimum_height"], 32.0);
    assert_eq!(options["tick_mark_max_character_length"], 5);
    assert_eq!(chart.time_axis_height(), 0.0, "hidden wins over the floor");

    // Tick stubs reach the axis frame only while the strip is visible and ticks on.
    chart.layout_panes(chart.css_height - chart.time_axis_height());
    chart.time_scale.set_width(800.0);
    let frame = chart.build_axis_frame(80.0, |text| text.len() as f64 * 6.0);
    assert!(
        frame.time_ticks.is_empty(),
        "hidden strip paints no tick stubs"
    );
    chart.set_time_axis_visible(true);
    let frame = chart.build_axis_frame(80.0, |text| text.len() as f64 * 6.0);
    assert!(!frame.time_ticks.is_empty(), "ticksVisible paints stubs");
}

#[test]
fn background_vertical_gradient_emits_a_per_pane_prim_solid_emits_none() {
    use origin_render::draw_list::Prim;
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    install_bars(&mut chart, 5);
    chart.time_scale.set_width(800.0);
    chart.fit_content();

    // Solid background (the default): no Background prim — the backends' clear color
    // covers it.
    let frame = chart.build_frame();
    assert!(!frame.panes[0]
        .under
        .iter()
        .any(|p| matches!(p, Prim::Background { .. })));

    // reference VerticalGradient: one prim per pane spanning that pane's full bitmap rect,
    // first in the under layer (behind the grid).
    chart
        .apply_options(
            r##"{"layout":{"background":{"type":"vertical_gradient","topColor":"#ff0000","bottomColor":"#0000ff"}}}"##,
        )
        .unwrap();
    let extra = chart.add_series(SeriesKind::Line);
    chart.set_series_pane(extra, 1, 1.0);
    let frame = chart.build_frame();
    assert_eq!(frame.panes.len(), 2);
    for pane in &frame.panes {
        let Some(Prim::Background { rect, gradient }) = pane.under.first() else {
            panic!("gradient background must lead the under layer");
        };
        assert_eq!(gradient.top, Color::rgb(0xff, 0x00, 0x00));
        assert_eq!(gradient.bottom, Color::rgb(0x00, 0x00, 0xff));
        assert_eq!(
            *rect,
            [
                pane.scissor[0] as f32,
                pane.scissor[1] as f32,
                pane.scissor[2] as f32,
                pane.scissor[3] as f32,
            ]
        );
    }

    // Back to solid: the prim disappears again.
    chart
        .apply_options(r##"{"layout":{"background":{"type":"solid","color":"#ffffff"}}}"##)
        .unwrap();
    let frame = chart.build_frame();
    assert!(!frame.panes.iter().any(|pane| pane
        .under
        .iter()
        .any(|p| matches!(p, Prim::Background { .. }))));
}

#[test]
fn separator_hover_mirrors_into_the_axis_frame() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    install_bars(&mut chart, 5);
    let extra = chart.add_series(SeriesKind::Line);
    chart.set_series_pane(extra, 1, 1.0);
    chart.layout_panes(472.0);
    chart.time_scale.set_width(800.0);

    // No hover by default; the hovered separator indexes into the frame's separators.
    let frame = chart.build_axis_frame(80.0, |text| text.len() as f64 * 6.0);
    assert_eq!(frame.separator_hover, None);
    assert_eq!(frame.separators.len(), 1);
    chart.set_separator_hover(Some(0));
    let frame = chart.build_axis_frame(80.0, |text| text.len() as f64 * 6.0);
    assert_eq!(frame.separator_hover, Some(0));
    chart.set_separator_hover(None);
    let frame = chart.build_axis_frame(80.0, |text| text.len() as f64 * 6.0);
    assert_eq!(frame.separator_hover, None);
}
