//! Headless tests for the series hit-test ports (reference per-kind rules + arbitration).

use aeris_charts_render::draw_list::Prim;

use super::*;
use crate::SeriesKind;

/// One candle series over 10 hourly bars with a settled layout (dpr 1, pane = the full
/// 800×500 content area). Returns the chart with the frame built once so autoscale/scale
/// state matches a rendered chart.
fn settled_candle_chart() -> ChartEngine {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times = (0..10).map(|i| (i * 3600) as f64).collect::<Vec<_>>();
    let open = [10.0, 11.0, 12.0, 11.0, 10.0, 11.0, 12.0, 13.0, 12.0, 11.0];
    let high = [11.0, 12.0, 13.0, 12.0, 11.0, 12.0, 13.0, 14.0, 13.0, 12.0];
    let low = [9.0, 10.0, 11.0, 10.0, 9.0, 10.0, 11.0, 12.0, 11.0, 10.0];
    let close = [11.0, 12.0, 11.0, 10.0, 11.0, 12.0, 13.0, 12.0, 11.0, 10.0];
    chart
        .set_series_data(0, &times, &open, &high, &low, &close)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.build_frame();
    chart
}

fn settled_line_chart(values: &[f64]) -> ChartEngine {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Line;
    let times = (0..values.len())
        .map(|i| (i * 3600) as f64)
        .collect::<Vec<_>>();
    chart
        .set_series_data(0, &times, values, values, values, values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.build_frame();
    chart
}

fn x_at(chart: &ChartEngine, index: i64) -> f64 {
    chart.logical_to_coordinate(index as f64).unwrap()
}

fn y_at(chart: &ChartEngine, id: SeriesId, price: f64) -> f64 {
    chart.series_price_to_coordinate(id, price).unwrap()
}

#[test]
fn candle_range_hit_covers_body_and_wick_span() {
    let chart = settled_candle_chart();
    let x = x_at(&chart, 5);
    let high_y = y_at(&chart, 0, 12.0);
    let low_y = y_at(&chart, 0, 10.0);
    // Body interior.
    assert_eq!(chart.hit_test_series(x, (high_y + low_y) / 2.0), Some(0));
    // Wick-only span (between high and the body top) hits too — reference ranges over high..low.
    assert_eq!(chart.hit_test_series(x, high_y + 1.0), Some(0));
    // Within the 3px tolerance above the high.
    assert_eq!(chart.hit_test_series(x, high_y - 2.5), Some(0));
    // Beyond tolerance: a clear miss.
    assert_eq!(chart.hit_test_series(x, high_y - 6.0), None);
    assert_eq!(chart.hit_test_series(x, low_y + 6.0), None);
}

#[test]
fn candle_hit_respects_horizontal_slot_boundaries() {
    let chart = settled_candle_chart();
    let spacing = chart.bar_spacing();
    let x = x_at(&chart, 5);
    let mid_y = (y_at(&chart, 0, 12.0) + y_at(&chart, 0, 10.0)) / 2.0;
    // Inside the slot (within barSpacing/2 of the center).
    assert_eq!(
        chart.hit_test_series(x + spacing / 2.0 - 1.0, mid_y),
        Some(0)
    );
    // Past the slot edge plus tolerance is the neighbour's territory (also a candle at the
    // same y — either way it must not miss the pair), so test the far gap with a y no bar
    // covers: between the bars there is no geometry at y near the scale extremes.
    let top_y = y_at(&chart, 0, 14.5);
    assert_eq!(chart.hit_test_series(x + spacing / 2.0 + 4.0, top_y), None);
}

#[test]
fn candle_hit_slot_extends_to_the_midpoint_between_bars() {
    let chart = settled_candle_chart();
    let spacing = chart.bar_spacing();
    assert!(spacing > 8.0, "fixture spacing must leave gaps");
    let x = x_at(&chart, 5);
    // Just past bar 5's slot half the NEXT bar's slot already owns the cursor (reference slots
    // meet at the midpoint), and bar 6's high..low covers bar 5's mid-price.
    let mid_y = (y_at(&chart, 0, 12.0) + y_at(&chart, 0, 10.0)) / 2.0;
    assert_eq!(
        chart.hit_test_series(x + spacing / 2.0 + 2.0, mid_y),
        Some(0)
    );
    // But at a price no bar covers, the same gap misses both slots.
    let top_y = y_at(&chart, 0, 14.5);
    assert_eq!(
        chart.hit_test_series(x - spacing / 2.0 - HIT_TEST_TOLERANCE - 2.0, top_y),
        None
    );
}

#[test]
fn hidden_and_removed_series_are_not_hit() {
    let mut chart = settled_candle_chart();
    let x = x_at(&chart, 5);
    let y = (y_at(&chart, 0, 12.0) + y_at(&chart, 0, 10.0)) / 2.0;
    chart.set_series_visible(0, false);
    assert_eq!(chart.hit_test_one_series(0, x, y), None);
    assert_eq!(chart.hit_test_series(x, y), None);

    let mut chart = settled_candle_chart();
    assert!(chart.remove_series(0));
    assert_eq!(chart.hit_test_one_series(0, x, y), None);
    assert_eq!(chart.hit_test_series(x, y), None);
}

#[test]
fn topmost_series_wins_equal_distance_ties() {
    let mut chart = settled_candle_chart();
    // An identical twin added later paints on top (reference appends to the pane's sources).
    let twin = chart.add_series(SeriesKind::Candlestick);
    let times = (0..10).map(|i| (i * 3600) as f64).collect::<Vec<_>>();
    let open = [10.0, 11.0, 12.0, 11.0, 10.0, 11.0, 12.0, 13.0, 12.0, 11.0];
    let high = [11.0, 12.0, 13.0, 12.0, 11.0, 12.0, 13.0, 14.0, 13.0, 12.0];
    let low = [9.0, 10.0, 11.0, 10.0, 9.0, 10.0, 11.0, 12.0, 11.0, 10.0];
    let close = [11.0, 12.0, 11.0, 10.0, 11.0, 12.0, 13.0, 12.0, 11.0, 10.0];
    chart
        .set_series_data(twin, &times, &open, &high, &low, &close)
        .unwrap();
    chart.build_frame();
    let x = x_at(&chart, 5);
    let y = (y_at(&chart, twin, 12.0) + y_at(&chart, twin, 10.0)) / 2.0;
    assert_eq!(chart.hit_test_series(x, y), Some(twin));
}

#[test]
fn closer_series_beats_paint_order_and_ties_go_topmost() {
    // A steep diagonal pins the scale (~20 px per price unit) so the two flat lines can sit
    // a controlled ~6 CSS px apart; it is probed far from the diagonal's own geometry.
    let mut chart = settled_line_chart(&[90.0, 95.0, 100.0, 105.0, 110.0]);
    chart.series[0].line_width = Some(4.0);
    let times = (0..5).map(|i| (i * 3600) as f64).collect::<Vec<_>>();
    let bottom = chart.add_series(SeriesKind::Line);
    chart.series_entry_mut(bottom).unwrap().line_width = Some(4.0);
    let values = [100.0; 5];
    chart
        .set_series_data(bottom, &times, &values, &values, &values, &values)
        .unwrap();
    let top = chart.add_series(SeriesKind::Line);
    chart.series_entry_mut(top).unwrap().line_width = Some(4.0);
    let values = [100.3; 5];
    chart
        .set_series_data(top, &times, &values, &values, &values, &values)
        .unwrap();
    chart.build_frame();
    // Probe at bar 0, where the diagonal anchor is ~200 px away from both flat lines.
    let x = x_at(&chart, 0);
    let y_bottom = y_at(&chart, bottom, 100.0);
    let y_top = y_at(&chart, top, 100.3);
    let separation = y_bottom - y_top;
    assert!((4.0..10.0).contains(&separation), "separation {separation}");
    // Stroke radius is 2 + 3 = 5 for both, so a probe a quarter of the way up from one
    // stroke is inside both radii but closer to that stroke (reference isBetterHit: distance
    // beats paint order).
    assert_eq!(
        chart.hit_test_series(x, y_bottom - separation * 0.25),
        Some(bottom)
    );
    assert_eq!(
        chart.hit_test_series(x, y_bottom - separation * 0.75),
        Some(top)
    );
    // Beyond both strokes.
    assert_eq!(chart.hit_test_series(x, y_top - 20.0), None);
    // Identical geometry ties resolve to the topmost series.
    let values = [100.0; 5];
    chart
        .set_series_data(top, &times, &values, &values, &values, &values)
        .unwrap();
    chart.build_frame();
    assert_eq!(
        chart.hit_test_series(x, y_at(&chart, bottom, 100.0)),
        Some(top)
    );
}

#[test]
fn line_stroke_radius_is_half_width_plus_tolerance() {
    let mut chart = settled_line_chart(&[100.0, 101.0, 99.0, 100.0, 100.0]);
    chart.series[0].line_width = Some(4.0);
    chart.build_frame();
    let x = x_at(&chart, 0);
    let y = y_at(&chart, 0, 100.0);
    // width/2 + 3 = 5: inside at 4.5, outside at 5.5 (perpendicular to the line direction).
    assert_eq!(chart.hit_test_series(x, y + 4.5), Some(0));
    assert_eq!(chart.hit_test_series(x, y + 5.5), None);

    // reference lineVisible=false: the stroke radius falls back to width 1 (0.5 + 3).
    chart.series[0].line_visible = false;
    assert_eq!(chart.hit_test_series(x, y + 3.0), Some(0));
    assert_eq!(chart.hit_test_series(x, y + 4.0), None);
}

#[test]
fn point_markers_report_point_priority() {
    let mut chart = settled_line_chart(&[100.0, 100.0, 100.0, 100.0, 100.0]);
    chart.series[0].line_width = Some(3.0);
    chart.series[0].point_markers = true;
    chart.build_frame();
    let x = x_at(&chart, 2);
    let y = y_at(&chart, 0, 100.0);
    // Stroke radius = 1.5 + 3 = 4.5; marker radius = (1.5 + 2) + 3 = 6.5. At 5.5px only the
    // marker disc reaches — a Point-class hit.
    let hit = chart.hit_test_one_series(0, x, y + 5.5).unwrap();
    assert_eq!(hit.kind, SeriesHitKind::Point);
    // Between bars only the stroke is near (reference returns the Point class when both cover the
    // cursor, so the Line class shows away from the markers).
    let between = (x_at(&chart, 1) + x_at(&chart, 2)) / 2.0;
    let hit = chart.hit_test_one_series(0, between, y + 2.0).unwrap();
    assert_eq!(hit.kind, SeriesHitKind::Line);
}

#[test]
fn single_visible_point_line_hits_along_the_bar_segment() {
    let chart = settled_line_chart(&[100.0]);
    let x = x_at(&chart, 0);
    let y = y_at(&chart, 0, 100.0);
    let spacing = chart.bar_spacing();
    // the reference's single-point segment spans ±max(barSpacing/2, radius) at the point's y.
    assert_eq!(chart.hit_test_series(x + spacing / 2.0 - 1.0, y), Some(0));
    assert_eq!(chart.hit_test_series(x, y + 20.0), None);
}

#[test]
fn area_series_hits_the_stroke_but_not_the_fill() {
    let mut chart = settled_line_chart(&[100.0, 100.0, 100.0, 100.0, 100.0]);
    chart.series[0].kind = SeriesKind::Area;
    chart.build_frame();
    let x = x_at(&chart, 2);
    let y = y_at(&chart, 0, 100.0);
    assert_eq!(chart.hit_test_series(x, y + 2.0), Some(0));
    // The filled region below the line carries no hit test in reference v5.2 (no renderer
    // implements hitTest) — a point deep inside the fill misses.
    assert_eq!(chart.hit_test_series(x, y + 60.0), None);
}

#[test]
fn histogram_column_rect_hits_between_base_and_value() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Histogram;
    let times = (0..10).map(|i| (i * 3600) as f64).collect::<Vec<_>>();
    let values = [10.0, 20.0, 15.0, 25.0, 30.0, 12.0, 18.0, 22.0, 16.0, 14.0];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.build_frame();
    let x = x_at(&chart, 4);
    let value_y = y_at(&chart, 0, 30.0);
    let base_y = y_at(&chart, 0, 0.0);
    // Inside the column (between the value and the base).
    assert_eq!(chart.hit_test_series(x, (value_y + base_y) / 2.0), Some(0));
    // Above the column's top, beyond tolerance.
    assert_eq!(chart.hit_test_series(x, value_y - 6.0), None);
    // Below the base, beyond tolerance.
    assert_eq!(chart.hit_test_series(x, base_y + 6.0), None);
}

#[test]
fn baseline_series_hits_its_stroke() {
    let mut chart = settled_line_chart(&[90.0, 110.0, 95.0, 105.0, 100.0]);
    chart.series[0].kind = SeriesKind::Baseline;
    chart.series[0].baseline = Some(100.0);
    chart.build_frame();
    let x = x_at(&chart, 1);
    let y = y_at(&chart, 0, 110.0);
    assert_eq!(chart.hit_test_series(x, y + 2.0), Some(0));
    assert_eq!(chart.hit_test_series(x, y + 20.0), None);
}

#[test]
fn whitespace_rows_carry_no_hit() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times = (0..10).map(|i| (i * 3600) as f64).collect::<Vec<_>>();
    let mut values = [100.0; 10];
    values[5] = f64::NAN; // whitespace bar in the middle
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.build_frame();
    let spacing = chart.bar_spacing();
    let x = x_at(&chart, 5);
    let y = y_at(&chart, 0, 100.0);
    // The gap's own x misses (neighbours' slots end barSpacing/2 away, well past tolerance).
    assert!(spacing / 2.0 > HIT_TEST_TOLERANCE + 2.0);
    assert_eq!(chart.hit_test_series(x, y), None);
    // A real bar still hits.
    assert_eq!(chart.hit_test_series(x_at(&chart, 4), y), Some(0));
}

#[test]
fn hits_are_restricted_to_the_pane_under_the_cursor() {
    let mut chart = settled_candle_chart();
    let other = chart.add_series(SeriesKind::Line);
    let times = (0..10).map(|i| (i * 3600) as f64).collect::<Vec<_>>();
    let values = [100.0; 10];
    chart
        .set_series_data(other, &times, &values, &values, &values, &values)
        .unwrap();
    chart.set_series_pane(other, 1, 1.0);
    chart.build_frame();

    let x = x_at(&chart, 5);
    // Pane 0 candle geometry: hit while the cursor is in pane 0.
    let candle_y = (y_at(&chart, 0, 12.0) + y_at(&chart, 0, 10.0)) / 2.0;
    assert!(candle_y <= chart.panes[0].top + chart.panes[0].height);
    assert_eq!(chart.hit_test_series(x, candle_y), Some(0));
    // The same x with the cursor down in pane 1 (on the line's own y there).
    let line_y = y_at(&chart, other, 100.0);
    assert!(line_y >= chart.panes[1].top);
    assert_eq!(chart.hit_test_series(x, line_y), Some(other));
    // The candle's y evaluated against pane 1 is out of pane 1's band — no cross-pane hit.
    assert_eq!(chart.pane_at_y(candle_y), Some(0));
    assert_eq!(chart.pane_at_y(line_y), Some(1));
}

#[test]
fn hovered_series_on_top_reorders_the_frame_but_not_the_stable_order() {
    let mut chart = settled_line_chart(&[100.0, 100.0, 100.0, 100.0, 100.0]);
    chart.series[0].line_color = Some("#ff0000".to_string());
    let top = chart.add_series(SeriesKind::Line);
    chart.series_entry_mut(top).unwrap().line_color = Some("#0000ff".to_string());
    let times = (0..5).map(|i| (i * 3600) as f64).collect::<Vec<_>>();
    let values = [100.0; 5];
    chart
        .set_series_data(top, &times, &values, &values, &values, &values)
        .unwrap();

    let stroke_colors = |chart: &mut ChartEngine| -> Vec<String> {
        let frame = chart.build_frame();
        frame.panes[0]
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
            .collect()
    };

    // Stable order: the later series strokes last (topmost).
    assert_eq!(stroke_colors(&mut chart), ["#ff0000", "#0000ff"]);
    // Hovering the bottom series bumps it to the top of the FRAME only.
    chart.set_hovered_series(Some(0));
    assert_eq!(stroke_colors(&mut chart), ["#0000ff", "#ff0000"]);
    assert_eq!(chart.series_order(), &[0, top]);
    // The option gates the bump.
    chart
        .apply_options("{\"hoveredSeriesOnTop\": false}")
        .unwrap();
    assert_eq!(stroke_colors(&mut chart), ["#ff0000", "#0000ff"]);
    // Clearing the hover restores the stable order.
    chart
        .apply_options("{\"hoveredSeriesOnTop\": true}")
        .unwrap();
    chart.set_hovered_series(None);
    assert_eq!(stroke_colors(&mut chart), ["#ff0000", "#0000ff"]);
}

#[test]
fn removing_the_hovered_series_releases_the_z_bump() {
    let mut chart = settled_line_chart(&[100.0, 100.0, 100.0]);
    chart.set_hovered_series(Some(0));
    assert_eq!(chart.hovered_series(), Some(0));
    assert!(chart.remove_series(0));
    assert_eq!(chart.hovered_series(), None);
    // A tombstoned id can never be re-pinned.
    chart.set_hovered_series(Some(0));
    assert_eq!(chart.hovered_series(), None);
}

#[test]
fn removing_the_selected_series_releases_the_selection() {
    let mut chart = settled_line_chart(&[100.0, 100.0, 100.0]);
    assert_eq!(chart.selected_series(), None);
    chart.set_selected_series(Some(0));
    assert_eq!(chart.selected_series(), Some(0));
    assert!(chart.remove_series(0));
    assert_eq!(chart.selected_series(), None);
    // A tombstoned id can never be re-pinned.
    chart.set_selected_series(Some(0));
    assert_eq!(chart.selected_series(), None);
}

#[test]
fn is_better_hit_ports_reference_arbitration() {
    let point = SeriesHit {
        series: 0,
        distance: 5.0,
        kind: SeriesHitKind::Point,
    };
    let close_line = SeriesHit {
        series: 1,
        distance: 1.0,
        kind: SeriesHitKind::Line,
    };
    let far_range = SeriesHit {
        series: 2,
        distance: 5.0,
        kind: SeriesHitKind::Range,
    };
    // Point beats any non-point regardless of distance.
    assert!(point.is_better_than(&close_line));
    assert!(!close_line.is_better_than(&point));
    // Distance decides among non-points.
    assert!(close_line.is_better_than(&far_range));
    // Equal-distance non-point ties lose (the caller's topmost-first order holds).
    assert!(!far_range.is_better_than(&SeriesHit {
        series: 3,
        distance: 5.0,
        kind: SeriesHitKind::Range,
    }));
}

mod edge_panics;
