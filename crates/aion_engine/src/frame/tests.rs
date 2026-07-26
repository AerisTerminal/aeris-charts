//! Frame-production unit tests (extracted from `frame.rs`).

use super::conflation::{VisibleHistogramRow, VisibleOhlc};
use super::*;

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

fn test_plot(count: usize) -> PlotList {
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
    let mut plot = PlotList::new();
    plot.set_data(indices, close.clone(), close.clone(), close.clone(), close);
    plot
}

#[test]
fn conflation_preserves_endpoints_and_pixel_bucket_extrema() {
    let plot = test_plot(100);
    let rows = visible_line_rows(&plot, 0, 99, 0.1, 1.0, |index| index as f64 * 0.1);
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
fn normal_spacing_keeps_every_visible_row() {
    let plot = test_plot(32);
    let rows = visible_line_rows(&plot, 4, 20, 2.0, 1.0, |index| index as f64 * 2.0);
    assert_eq!(rows, (4..=20).map(|i| i as usize).collect::<Vec<_>>());
}

#[test]
fn ohlc_conflation_keeps_first_open_last_close_and_full_envelope() {
    let indices: Vec<i64> = (0..8).collect();
    let open = vec![10.0, 12.0, 11.0, 14.0, 20.0, 19.0, 18.0, 17.0];
    let high = vec![13.0, 15.0, 19.0, 16.0, 22.0, 25.0, 21.0, 20.0];
    let low = vec![9.0, 8.0, 10.0, 11.0, 18.0, 16.0, 15.0, 14.0];
    let close = vec![12.0, 11.0, 14.0, 13.0, 19.0, 18.0, 17.0, 16.0];
    let mut plot = PlotList::new();
    plot.set_data(indices, open, high, low, close);

    let bars = visible_ohlc(&plot, 0, 7, 0.25, 1.0, |index| index as f64 * 0.25);
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
    let bars = visible_ohlc(&plot, 2, 5, 2.0, 1.5, |index| index as f64 * 3.0);
    assert_eq!(bars.len(), 4);
    assert_eq!(bars[0].x_px, 6.0);
    assert_eq!(bars[0].open, plot.value_at(2, PlotValueIndex::Open));
    assert_eq!(bars[3].close, plot.value_at(5, PlotValueIndex::Close));
}

#[test]
fn histogram_conflation_preserves_largest_magnitude_and_source_row() {
    let indices: Vec<i64> = (0..8).collect();
    let values = vec![1.0, -8.0, 3.0, 4.0, 2.0, 5.0, -12.0, 7.0];
    let mut plot = PlotList::new();
    plot.set_data(
        indices,
        values.clone(),
        values.clone(),
        values.clone(),
        values,
    );

    let rows = visible_histogram_rows(&plot, 0, 7, 0.25, 1.0, |index| index as f64 * 0.25);
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
        let wicks: Vec<aion_render::draw_list::IRect> = frame.panes[0]
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
    assert!(frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, Prim::VLine { .. })));
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
    assert!(frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, Prim::VLine { .. })));

    // Hidden mode still suppresses the crosshair entirely.
    chart.crosshair_mode = CrosshairMode::Hidden;
    let frame = chart.build_frame();
    assert!(!frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, Prim::VLine { .. })));
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
    assert!(frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, Prim::VLine { .. })));
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::HLine {
            style: LineStyle::Dotted,
            ..
        }
    )));

    // The time label needs only the time scale; the price label comes off the containing pane's
    // default scale (the secondary series' right scale here).
    let axis = chart.build_axis_frame(80.0, |t| t.len() as f64 * 7.0);
    assert!(axis
        .labels
        .iter()
        .any(|l| l.midpoint == AxisTextMidpoint::StableTime));
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
    assert!(frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, Prim::VLine { x, .. } if *x == expected_x)));
}

/// Two identical line series on the right scale (same last close => colliding label
/// candidates, the case the reference's overlap resolution exists for).
fn two_identical_line_series() -> ChartEngine {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    chart.series[0].kind = SeriesKind::Line;
    let times = [1.0, 2.0, 3.0, 4.0, 5.0];
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
            .build_axis_frame(80.0, |t| t.len() as f64 * 7.0)
            .labels
            .into_iter()
            .filter(|l| l.background.is_some())
            .collect::<Vec<_>>()
    };

    // reference SeriesPriceAxisView: every visible series with lastValueVisible gets a label on its
    // scale, in the series' bar color (the line color for a line series — not up/down).
    let labels = boxed(&mut chart);
    assert_eq!(labels.len(), 2);
    assert!(labels
        .iter()
        .all(|l| matches!(l.background, Some((.., c)) if c == LINE)));
    // reference `_fixLabelOverlap`: colliding labels are pushed apart by their box height.
    let height = 12.0 + 2.5 * 2.0;
    let gap = (labels[0].y - labels[1].y).abs();
    assert!(
        (gap - height).abs() < 1e-9,
        "overlapping labels must be pushed a full box height apart, got {gap}"
    );

    // `lastValueVisible: false` on one series drops only its label.
    chart.series[1].last_value_visible = false;
    assert_eq!(boxed(&mut chart).len(), 1);

    // A hidden series loses its label entirely (reference series-price-axis-view.ts:24).
    chart.series[1].last_value_visible = true;
    chart.set_series_visible(1, false);
    assert_eq!(boxed(&mut chart).len(), 1);
}

#[test]
fn last_value_label_tracks_the_last_visible_bar() {
    let mut chart = two_identical_line_series();
    chart.series[1].last_value_visible = false;
    // Scroll one bar past the right edge: the label follows the last *visible* bar (reference
    // series.ts lastValueData(false)), not the series' final bar.
    chart.set_right_offset(-1.0);
    let labels = chart
        .build_axis_frame(80.0, |t| t.len() as f64 * 7.0)
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
                    ..
                } => Some((*y, *width, *color)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    // reference priceLineVisible default true: every visible series gets a built-in last-price
    // line (dotted, 1px, following the bar color — the line color for a line series).
    let lines = dashed_ylines(&mut chart);
    assert_eq!(lines.len(), 2);
    assert!(lines
        .iter()
        .all(|&(_, width, color)| width == 1 && color == LINE));

    // priceLineVisible: false hides only that series' line.
    chart.series[1].price_line_visible = false;
    assert_eq!(dashed_ylines(&mut chart).len(), 1);

    // priceLineSource LastVisible anchors at the last visible bar when the final bar is
    // scrolled off the right edge; LastBar keeps the final bar.
    chart.series[0].price_line_source = 1;
    chart.series[1].price_line_source = 0;
    chart.series[1].price_line_visible = true;
    chart.set_right_offset(-1.0);
    let lines = dashed_ylines(&mut chart); // builds the frame, autoscaling the new window first
    let scale = pane_scale(&chart.panes[0], PriceScaleTarget::Right);
    let base = chart.series_base_value(0, 0).unwrap();
    let y_last_visible = (scale.price_to_coordinate(11.5, base) * 1.0).round() as i32;
    let y_last_bar = (scale.price_to_coordinate(12.5, base) * 1.0).round() as i32;
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().any(|&(y, ..)| y == y_last_visible));
    assert!(lines.iter().any(|&(y, ..)| y == y_last_bar));

    // priceLineWidth / priceLineColor / priceLineStyle all reach the frame.
    chart.series[0].price_line_width = 3.0;
    chart.series[0].price_line_color = Some("#112233".to_string());
    chart.series[0].price_line_style = 0;
    chart.set_right_offset(0.0);
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::HLine {
            width: 3,
            style: LineStyle::Solid,
            color,
            ..
        } if *color == Color::rgb(0x11, 0x22, 0x33)
    )));
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
    assert!(runs
        .iter()
        .all(|&(.., style, line_type)| style == LineStyle::Solid && line_type == LineType::Simple));
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
    assert_eq!(
        bg(sma_name).3,
        bg(sma_price).3,
        "name chip matches the row height"
    );
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
    chart.series[sma].line_style = 1; // dotted
    let dotted = polylines(&mut chart);
    assert!(dotted > solid, "dotted splits the stroke into dash runs");
    chart.series[sma].line_style = 2; // dashed
    let dashed = polylines(&mut chart);
    assert!(dashed > solid, "dashed splits the stroke into dash runs");
}

#[test]
fn bollinger_band_fill_paints_between_upper_and_lower() {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times = [1.0, 2.0, 3.0, 4.0];
    let values = [10.0, 11.0, 12.0, 11.0];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.add_bollinger(0, 2, 2.0);

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
        .expect("bollinger paints a band fill");
    // Three valid rows (4 bars, warm-up 1) and the fill is the band color at 0.2 alpha.
    assert_eq!(fill.0, 3);
    assert_eq!(fill.1, Color::rgba(LINE.r(), LINE.g(), LINE.b(), 51));
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
    assert!(frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, Prim::AreaFill { .. })));
    assert!(!frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, Prim::Polyline { .. })));

    // A line series keeps nothing but its point markers.
    chart.series[0].kind = SeriesKind::Line;
    chart.series[0].point_markers = true;
    let frame = chart.build_frame();
    assert!(!frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, Prim::Polyline { .. } | Prim::AreaFill { .. })));
    assert!(frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, Prim::Circle { .. })));
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
    // reference auto radius (line-pane-view.ts): lineWidth / 2 + 2 = 3.5 at the default width 3.
    assert_eq!(radius(&mut chart), 3.5);
    chart.series[0].point_markers_radius = Some(6.0);
    assert_eq!(radius(&mut chart), 6.0);
}

#[test]
fn crosshair_marks_cover_all_line_series_with_per_series_options() {
    let mut chart = two_identical_line_series();
    let x = chart.time_scale.index_to_coordinate(2);
    chart.crosshair = Some((x, 120.0));
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

    // reference crosshair-marks-pane-view: one mark per visible line-like series at the crosshair
    // index. Defaults: radius 4 + border 2, border = chart background, fill = bar color.
    let marks = circles(&mut chart);
    assert_eq!(marks.len(), 4);
    assert!(marks
        .iter()
        .any(|&(r, c)| r == 6.0 && c == Color::rgb(0xff, 0xff, 0xff)));
    assert!(marks.iter().any(|&(r, c)| r == 4.0 && c == LINE));

    // crosshairMarkerVisible: false drops the series' mark.
    chart.series[1].crosshair_marker_visible = false;
    assert_eq!(circles(&mut chart).len(), 2);

    // Per-series radius / border width / pinned colors reach the frame.
    chart.series[0].crosshair_marker_radius = 7.0;
    chart.series[0].crosshair_marker_border_width = 3.0;
    chart.series[0].crosshair_marker_border_color = Some("#010203".to_string());
    chart.series[0].crosshair_marker_background_color = Some("#040506".to_string());
    let marks = circles(&mut chart);
    assert!(marks
        .iter()
        .any(|&(r, c)| r == 10.0 && c == Color::rgb(0x01, 0x02, 0x03)));
    assert!(marks
        .iter()
        .any(|&(r, c)| r == 7.0 && c == Color::rgb(0x04, 0x05, 0x06)));
}

#[test]
fn baseline_quadrant_options_flow_into_fills_strokes_and_marker() {
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

    // reference baselineStyleDefaults: two-stop gradients per quadrant (alphas 0.28 -> 71, 0.05 -> 13).
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::AreaFill { gradient, .. }
            if gradient.top == Color::rgba(0x26, 0xa6, 0x9a, 71)
                && gradient.bottom == Color::rgba(0x26, 0xa6, 0x9a, 13)
    )));
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::AreaFill { gradient, .. }
            if gradient.top == Color::rgba(0xef, 0x53, 0x50, 13)
                && gradient.bottom == Color::rgba(0xef, 0x53, 0x50, 71)
    )));
    // One continuous solid stroke per quadrant in the reference line colors.
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::Polyline { color, .. } if *color == Color::rgb(0x26, 0xa6, 0x9a)
    )));
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::Polyline { color, .. } if *color == Color::rgb(0xef, 0x53, 0x50)
    )));

    // The crosshair marker background follows the baseline bar colorer: the last bar sits
    // above the baseline, so the marker is the top line color.
    let x = chart.time_scale.index_to_coordinate(1);
    chart.crosshair = Some((x, 120.0));
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::Circle { radius, fill, .. }
            if *radius == 4.0 && *fill == Color::rgb(0x26, 0xa6, 0x9a)
    )));
    chart.crosshair = None;

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
                Prim::Polyline { color, .. } if *color == Color::rgb(0xef, 0x53, 0x50)
            )
        })
        .count();
    assert!(bottom_runs > 1, "dashed quadrant must split into runs");

    // lineVisible: false drops both quadrant strokes but keeps the fills.
    chart.series[0].line_visible = false;
    let frame = chart.build_frame();
    assert!(!frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, Prim::Polyline { .. })));
    assert!(frame.panes[0]
        .main
        .iter()
        .any(|p| matches!(p, Prim::AreaFill { .. })));
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
    chart.series[histogram].base = 4.0; // above every value: columns hang down from the base

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

    // thinBars: false lets the body take the full optimal width (floor(10 * 0.3) = 3).
    chart.series[0].thin_bars = false;
    let rs = rects(&mut chart);
    assert!(rs.iter().any(|r| r.w == 3), "thick bodies reach the frame");

    // openVisible: false drops the open tick (body + close only).
    chart.series[0].open_visible = false;
    let rs = rects(&mut chart);
    assert_eq!(rs.len(), 2 * 5);
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
    // The uncolored bars keep the reference up-color resolution.
    assert!(has(0x26A69AFF), "series up color still drawn");
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
    chart.series[volume].histogram_updown = true;
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
            if gradient.top == AREA_TOP && gradient.bottom == AREA_BOTTOM
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
fn last_value_label_background_honors_the_per_point_color() {
    let mut chart = ohlc_chart(SeriesKind::Candlestick, 3);
    // The final bar carries a custom body color: the last-value label (and the built-in
    // last-price line) follow it (reference series-bar-colorer.ts).
    assert!(chart.set_series_point_colors(0, Some(vec![0, 0, POINT_RED]), None, None));
    let axis = chart.build_axis_frame(80.0, |t| t.len() as f64 * 7.0);
    assert!(axis
        .labels
        .iter()
        .any(|l| matches!(l.background, Some((.., c)) if c == Color(POINT_RED))));
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
    chart.series[custom].price_line_source = 1;
    let frame = chart.build_frame();
    assert!(frame.panes[0].main.iter().any(|p| matches!(
        p,
        Prim::HLine { color, style: LineStyle::Dotted, .. } if *color == visible.color
    )));
    // The last-value axis label always tracks the visible record (reference lastValueData(false)).
    let axis = chart.build_axis_frame(80.0, |t| t.len() as f64 * 7.0);
    assert!(axis.labels.iter().any(
        |l| matches!(l.background, Some((.., c)) if c == visible.color) && l.text == "104.00"
    ));
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
    let axis = chart.build_axis_frame(80.0, |t| t.len() as f64 * 7.0);
    assert!(
        !axis
            .labels
            .iter()
            .any(|l| l.background.is_some() && l.midpoint == AxisTextMidpoint::StableTime),
        "no time label in the empty area"
    );

    // Control: hovering a real bar still shows the time label.
    chart.crosshair = Some((chart.time_scale.index_to_coordinate(2), 120.0));
    let axis = chart.build_axis_frame(80.0, |t| t.len() as f64 * 7.0);
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
    // Uniform step 1000: multiples of 10000 are round (TradingView's screenshot behavior).
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
        vec![true, false, false, false, false, false, false, false, false, false, true]
    );
    // Disabled: nothing bold.
    let bold = ChartEngine::bold_round_decisions(&[100.0, 120.0], false);
    assert_eq!(bold, vec![false, false]);
    // Non-uniform (log-style) ticks: exact powers of ten only.
    let bold = ChartEngine::bold_round_decisions(&[1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0], true);
    assert_eq!(bold, vec![true, false, false, true, false, false, true]);
}

#[test]
fn allow_bold_labels_gates_major_time_ticks() {
    let mut chart = crosshair_chart();
    chart.time_scale.set_width(300.0);
    let frame = chart.build_axis_frame(80.0, |t| t.len() as f64 * 7.0);
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
    let frame = chart.build_axis_frame(80.0, |t| t.len() as f64 * 7.0);
    assert!(
        frame
            .labels
            .iter()
            .all(|l| l.midpoint != AxisTextMidpoint::None || !l.bold),
        "no bold time labels when allowBoldLabels is off"
    );
}

// ---- TradingView-style last-value cluster: title chip + price + candle-close countdown ----

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
    // < 1d: h:mm:ss.
    assert_eq!(fmt(3600.0), "1:00:00");
    assert_eq!(fmt(3661.0), "1:01:01");
    assert_eq!(fmt(86399.0), "23:59:59");
    // >= 1d: "Xd Xh".
    assert_eq!(fmt(86400.0), "1d 0h");
    assert_eq!(fmt(2.0 * 86400.0 + 5.0 * 3600.0 + 120.0), "2d 5h");
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
        .build_axis_frame(80.0, |t| t.len() as f64 * 7.0)
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
    // Past the close the remaining time clamps at zero instead of going negative.
    chart.now_override = Some(480.0);
    assert_eq!(chart.series_countdown_text(0).as_deref(), Some("00:00"));
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
    // TradingView geometry: the title chip sits OUTSIDE the strip (a small gap before the
    // border), the price chip and countdown chip share one width inside and stack flush.
    assert_eq!(chip_y, price_y);
    assert!(
        (price_x - (chip_x + chip_w) - 1.0).abs() < 1e-9,
        "chip-to-border gap"
    );
    assert!((price_y + price_h - cd_y).abs() < 1e-9, "flush stack");
    assert!((cd_x - price_x).abs() < 1e-9, "same left edge");
    assert!((cd_w - price_w).abs() < 1e-9, "same width");
    assert_eq!(chip_h, price_h);
    assert_eq!(price_h, cd_h);
    // Title and price texts share the contrast-pick color; the countdown text is slightly
    // muted against the chip (TradingView-style).
    assert_eq!(chip.color, LINE.contrast_text());
    assert_eq!(price.color, LINE.contrast_text());
    let muted = {
        let base = LINE.contrast_text();
        Color::rgba(
            base.r(),
            base.g(),
            base.b(),
            (base.a() as f64 * 0.65).round() as u8,
        )
    };
    assert_eq!(countdown.color, muted);

    // Price off, title + countdown on: only the outside title chip and the inside countdown
    // chip render — no empty price box.
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

    // The same label on the left strip rounds the left corners.
    chart.series[0].left_scale = true;
    let labels = boxed_labels(&mut chart);
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].background_corners, AxisLabelCorners::LEFT);
    chart.series[0].left_scale = false;

    // The crosshair price label follows its strip; the time label rounds the bottom corners.
    chart.crosshair = Some((400.0, 250.0));
    let labels = boxed_labels(&mut chart);
    let price = labels
        .iter()
        .find(|l| l.midpoint == AxisTextMidpoint::Label)
        .expect("crosshair price label");
    assert_eq!(price.background_corners, AxisLabelCorners::RIGHT);
    let time = labels
        .iter()
        .find(|l| l.midpoint == AxisTextMidpoint::StableTime)
        .expect("crosshair time label");
    assert_eq!(time.background_corners, AxisLabelCorners::BOTTOM);
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
    chart.series[0].left_scale = true;
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
fn axis_width_negotiation_covers_the_widest_cluster_row() {
    let measure = |t: &str| t.len() as f64 * 7.0;
    let mut chart = countdown_chart();
    let plain = chart.optimal_price_axis_width_for(PriceScaleTarget::Right, measure);
    // Tick + price texts are 5 chars here; the plain strip covers the widest label.
    assert_eq!(plain, 56.0);

    // A long countdown row ("23:59:59", 8 chars) widens the strip. next_close = 240 + 60.
    chart.series[0].countdown_visible = true;
    chart.now_override = Some(300.0 - 86399.0);
    let with_countdown = chart.optimal_price_axis_width_for(PriceScaleTarget::Right, measure);
    assert_eq!(with_countdown, 78.0); // 77 snapped up to even
    assert!(with_countdown > plain);

    // The title chip lives OUTSIDE the strip (pane side), so it never widens the axis.
    chart.now_override = Some(250.0); // "00:50" — same 5 chars as the price
    chart.series[0].title = "NDQ".to_string();
    let with_cluster = chart.optimal_price_axis_width_for(PriceScaleTarget::Right, measure);
    assert_eq!(with_cluster, 56.0, "outside chip must not widen the strip");
}

#[test]
fn last_value_cluster_overlap_resolution_uses_the_total_height() {
    let mut chart = two_identical_line_series();
    chart.now_override = Some(12.0);
    chart.series[0].countdown_visible = true;
    chart.series[1].countdown_visible = true;
    let labels = chart
        .build_axis_frame(80.0, |t| t.len() as f64 * 7.0)
        .labels;
    // Two colliding two-row clusters: the overlap pass pushes them two full rows apart
    // (measured between the price rows, which share each cluster's layout).
    let row_height = 12.0 + 2.5 * 2.0;
    let mut price_ys: Vec<f64> = labels
        .iter()
        .filter(|l| l.background.is_some() && l.text == "12.50")
        .map(|l| l.y)
        .collect();
    price_ys.sort_by(|a, b| a.total_cmp(b));
    assert_eq!(price_ys.len(), 2);
    let gap = price_ys[1] - price_ys[0];
    assert!(
        (gap - 2.0 * row_height).abs() < 1e-9,
        "two-row clusters must be pushed two box heights apart, got {gap}"
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

#[test]
fn selection_anchors_paint_theme_derived_discs_on_the_selected_series() {
    const BLUE: Color = Color::rgb(0x29, 0x62, 0xff); // TradingView accent blue
    let mut chart = anchor_chart();
    // Nothing selected: no anchor discs.
    assert!(frame_discs(&mut chart).is_empty());
    chart.set_selected_series(Some(0));
    let discs = frame_discs(&mut chart);
    // One border disc (blue, radius 2.5 + 1.5 at dpr 1) + one fill disc (radius 2.5) per bar.
    let borders: Vec<_> = discs.iter().copied().filter(|d| d.2 == BLUE).collect();
    let fills: Vec<_> = discs.iter().copied().filter(|d| d.2 != BLUE).collect();
    assert_eq!(borders.len(), 5);
    assert_eq!(fills.len(), 5);
    assert!(borders.iter().all(|d| (d.1 - 4.0).abs() < 1e-4));
    assert!(fills.iter().all(|d| (d.1 - 2.5).abs() < 1e-4));
    // Light background (default): white fills, each paired with a border disc at the same x.
    assert!(fills.iter().all(|d| d.2 == Color::rgb(0xff, 0xff, 0xff)));
    for fill in &fills {
        assert!(borders.iter().any(|b| (b.0 - fill.0).abs() < 1e-4));
    }
    // Dark background: the fill tracks the background luminance to black, blue border stays.
    chart
        .apply_options(r##"{"layout":{"background":{"color":"#0d0d0d"}}}"##)
        .unwrap();
    let dark: Vec<_> = frame_discs(&mut chart);
    let dark_fills: Vec<_> = dark.iter().copied().filter(|d| d.2 != BLUE).collect();
    assert_eq!(dark.iter().filter(|d| d.2 == BLUE).count(), 5);
    assert_eq!(dark_fills.len(), 5);
    assert!(dark_fills.iter().all(|d| d.2 == Color::rgb(0, 0, 0)));
    // Deselecting (an empty-pane click) removes the anchors.
    chart.set_selected_series(None);
    assert!(frame_discs(&mut chart).is_empty());
}
