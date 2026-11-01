//! Business families: waterfall, bullet, lollipop/dumbbell marks, and slope/bump options.

use aeris_charts_render::draw_list::Prim;

use crate::general_series::general_waterfall_rows;
use crate::{
    AxisDimension, CategoryScaleType, ChartEngine, ContinuousScaleType, GeneralAxisDomain,
    GeneralAxisOptions, GeneralBarMark, GeneralEndLabels, GeneralHitMode, GeneralRankTies,
    GeneralRowDetail, GeneralRowId, GeneralScaleType, GeneralSeriesId, GeneralSeriesKind,
    GeneralSeriesOptions, GeneralWaterfallMark, GeneralWaterfallStep, GeneralXyInput,
    HorizontalDomain,
};

fn category_chart(horizontal: bool) -> ChartEngine {
    let mut chart = ChartEngine::new_with_initial_domain(
        480.0,
        320.0,
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
        ("x", AxisDimension::X, x_scale),
        ("y", AxisDimension::Y, y_scale),
    ] {
        let mut axis = GeneralAxisOptions::new(id, 0, dimension, scale);
        axis.visible = false;
        chart.add_general_axis(axis).unwrap();
    }
    chart
}

fn layout(chart: &mut ChartEngine) {
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
}

fn ids(names: &[&str]) -> Option<Vec<GeneralRowId>> {
    Some(
        names
            .iter()
            .map(|name| GeneralRowId::Text((*name).into()))
            .collect(),
    )
}

fn waterfall_input() -> GeneralXyInput {
    GeneralXyInput::WaterfallCategory {
        ids: ids(&["start", "q1", "q2", "mid", "q3", "end"]),
        categories: ["Start", "Q1", "Q2", "Mid", "Q3", "End"]
            .map(String::from)
            .to_vec(),
        category_indices: vec![0, 1, 2, 3, 4, 5],
        values: vec![100.0, 20.0, -30.0, 0.0, 40.0, 0.0],
        values_valid: Some(vec![1, 1, 1, 0, 1, 0]),
        steps: vec![
            GeneralWaterfallStep::Relative,
            GeneralWaterfallStep::Relative,
            GeneralWaterfallStep::Relative,
            GeneralWaterfallStep::Subtotal,
            GeneralWaterfallStep::Relative,
            GeneralWaterfallStep::Total,
        ],
    }
}

fn bullet_input() -> GeneralXyInput {
    GeneralXyInput::BulletCategory {
        ids: ids(&["revenue", "profit"]),
        categories: vec!["Revenue".into(), "Profit".into()],
        category_indices: vec![0, 1],
        measure: vec![270.0, 22.0],
        measure_valid: None,
        target: vec![250.0, 26.0],
        target_valid: None,
        comparative: vec![230.0, 0.0],
        comparative_valid: Some(vec![1, 0]),
        bands: [vec![150.0, 20.0], vec![225.0, 25.0], vec![300.0, 30.0]],
        bands_valid: [None, None, None],
    }
}

fn category_input(values: &[f64]) -> GeneralXyInput {
    let names: Vec<String> = (0..values.len()).map(|index| format!("C{index}")).collect();
    GeneralXyInput::Category {
        ids: None,
        categories: names,
        category_indices: (0..values.len() as u32).collect(),
        y: values.to_vec(),
        y_valid: None,
    }
}

fn waterfall_bars(
    chart: &ChartEngine,
    series: GeneralSeriesId,
) -> Vec<crate::general_series::GeneralWaterfallGeometry> {
    let mut bars = Vec::new();
    chart.visit_general_waterfall_bars(chart.general_series(series).unwrap(), |bar| bars.push(bar));
    bars
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn waterfall_running_totals_subtotals_and_totals() {
    let mut chart = category_chart(false);
    let dataset = chart.create_general_xy_dataset(waterfall_input()).unwrap();
    let rows = general_waterfall_rows(chart.general_dataset(dataset).unwrap());
    assert_eq!(
        rows,
        vec![
            Some((0.0, 100.0, GeneralWaterfallMark::Increase)),
            Some((100.0, 120.0, GeneralWaterfallMark::Increase)),
            Some((120.0, 90.0, GeneralWaterfallMark::Decrease)),
            Some((0.0, 90.0, GeneralWaterfallMark::Subtotal)),
            Some((90.0, 130.0, GeneralWaterfallMark::Increase)),
            Some((0.0, 130.0, GeneralWaterfallMark::Total)),
        ]
    );
    chart
        .add_general_series(GeneralSeriesOptions::waterfall(0, dataset, "x", "y"))
        .unwrap();
    layout(&mut chart);
    let Some(GeneralAxisDomain::Numeric([low, high])) =
        chart.effective_general_axis_domain(chart.general_axis("y").unwrap())
    else {
        panic!("numeric waterfall value domain");
    };
    assert!(low <= 0.0 && high >= 130.0, "{low}..{high}");
}

#[test]
fn waterfall_geometry_connectors_and_orientation() {
    for horizontal in [false, true] {
        let mut chart = category_chart(horizontal);
        let dataset = chart.create_general_xy_dataset(waterfall_input()).unwrap();
        let series = chart
            .add_general_series(GeneralSeriesOptions::waterfall(0, dataset, "x", "y"))
            .unwrap();
        layout(&mut chart);
        let bars = waterfall_bars(&chart, series);
        assert_eq!(bars.len(), 6, "horizontal={horizontal}");
        assert!(bars.iter().all(|bar| bar.horizontal == horizontal));
        assert_eq!(bars[0].connector, None);
        for pair in bars.windows(2) {
            let (previous, bar) = (pair[0], pair[1]);
            let (band_from, band_to, level) = bar.connector.expect("connector");
            if horizontal {
                // Band Y: the next bar sits below, and the connector runs at the running total.
                assert!(close(band_from, previous.bar.bottom) && close(band_to, bar.bar.top));
                let previous_end = if previous.bar.end_at_min {
                    previous.bar.left
                } else {
                    previous.bar.right
                };
                assert!(close(level, previous_end));
            } else {
                assert!(close(band_from, previous.bar.right) && close(band_to, bar.bar.left));
                let previous_end = if previous.bar.end_at_min {
                    previous.bar.top
                } else {
                    previous.bar.bottom
                };
                assert!(close(level, previous_end));
            }
        }
        // The decrease spans 120 -> 90 and the subtotal 0 -> 90 share their running-total end.
        let (decrease, subtotal) = (bars[2].bar, bars[3].bar);
        assert_eq!(bars[2].mark, GeneralWaterfallMark::Decrease);
        assert_eq!(bars[5].mark, GeneralWaterfallMark::Total);
        if horizontal {
            assert!(close(decrease.left, subtotal.right));
            assert!(decrease.end_at_min);
            assert!(bars[1].bar.left < bars[1].bar.right);
        } else {
            assert!(close(decrease.bottom, subtotal.top));
            assert!(!decrease.end_at_min);
            assert!(close(bars[5].bar.top, bars[4].bar.top));
        }
        let main = chart.build_frame().panes[0].main.clone();
        let connector_count = |main: &[Prim]| {
            main.iter()
                .filter(|prim| {
                    if horizontal {
                        matches!(prim, Prim::VLine { .. })
                    } else {
                        matches!(prim, Prim::HLine { .. })
                    }
                })
                .count()
        };
        let with_connectors = connector_count(&main);
        let mut options = GeneralSeriesOptions::waterfall(0, dataset, "x", "y");
        options.waterfall.connectors = false;
        chart
            .update_general_series_options(series, options)
            .unwrap();
        let without = connector_count(&chart.build_frame().panes[0].main);
        assert_eq!(with_connectors - without, 5, "horizontal={horizontal}");
    }
}

#[test]
fn waterfall_colors_labels_hits_tooltip_and_accessibility() {
    let mut chart = category_chart(false);
    let dataset = chart.create_general_xy_dataset(waterfall_input()).unwrap();
    let mut options = GeneralSeriesOptions::waterfall(0, dataset, "x", "y");
    options.data_labels = true;
    options.waterfall.increase_color = Some("#00ff00".into());
    options.waterfall.decrease_color = Some("#ff0000".into());
    options.waterfall.total_color = Some("#0000ff".into());
    let series = chart.add_general_series(options).unwrap();
    layout(&mut chart);
    let main = chart.build_frame().panes[0].main.clone();
    let fills: Vec<_> = main
        .iter()
        .filter_map(|prim| match prim {
            Prim::Rect { color, .. } => Some(color.to_css()),
            _ => None,
        })
        .collect();
    let count = |css: &str| {
        let color = aeris_charts_render::color::Color::parse_css(css)
            .unwrap()
            .to_css();
        fills.iter().filter(|fill| **fill == color).count()
    };
    assert_eq!(count("#00ff00"), 3);
    assert_eq!(count("#ff0000"), 1);
    // The subtotal falls back to the total color.
    assert_eq!(count("#0000ff"), 2);
    let texts: Vec<&str> = main
        .iter()
        .filter_map(|prim| match prim {
            Prim::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    for label in ["100", "-30", "90", "130"] {
        assert!(texts.contains(&label), "{label} in {texts:?}");
    }

    let total = waterfall_bars(&chart, series)[5].bar;
    let hit = chart
        .general_hit_test(
            0,
            (total.left + total.right) / 2.0,
            (total.top + total.bottom) / 2.0,
            GeneralHitMode::Exact,
        )
        .unwrap();
    assert_eq!(hit.row, 5);
    let tooltip = chart.general_tooltip_snapshot(series, 5).unwrap();
    assert_eq!(tooltip.value, Some(130.0));
    assert_eq!(tooltip.size, None);
    assert_eq!(
        tooltip.detail,
        Some(GeneralRowDetail::Waterfall {
            mark: GeneralWaterfallMark::Total,
            start: 0.0,
            end: 130.0,
        })
    );
    let accessibility = chart.general_accessibility_snapshot(series, 0, 10).unwrap();
    assert_eq!(accessibility.items.len(), 6);
    assert_eq!(accessibility.items[2].value, Some(-30.0));
    assert_eq!(
        accessibility.items[2].detail,
        Some(GeneralRowDetail::Waterfall {
            mark: GeneralWaterfallMark::Decrease,
            start: 120.0,
            end: 90.0,
        })
    );
    assert_eq!(accessibility.items[3].value, Some(90.0));
    let legend = chart.general_legend_snapshot(None);
    assert_eq!(legend.items[0].kind, GeneralSeriesKind::Waterfall);
}

#[test]
fn waterfall_and_bullet_validation_is_atomic() {
    let mut chart = category_chart(false);
    let GeneralXyInput::WaterfallCategory {
        ids,
        categories,
        category_indices,
        values,
        values_valid,
        mut steps,
    } = waterfall_input()
    else {
        unreachable!()
    };
    steps.pop();
    assert!(
        chart
            .create_general_xy_dataset(GeneralXyInput::WaterfallCategory {
                ids,
                categories,
                category_indices,
                values,
                values_valid,
                steps,
            })
            .is_err()
    );
    let GeneralXyInput::BulletCategory {
        ids,
        categories,
        category_indices,
        measure,
        measure_valid,
        target,
        target_valid,
        comparative,
        comparative_valid,
        bands_valid,
        ..
    } = bullet_input()
    else {
        unreachable!()
    };
    assert!(
        chart
            .create_general_xy_dataset(GeneralXyInput::BulletCategory {
                ids,
                categories,
                category_indices,
                measure,
                measure_valid,
                target,
                target_valid,
                comparative,
                comparative_valid,
                bands: [vec![150.0, 20.0], vec![100.0, 25.0], vec![300.0, 30.0]],
                bands_valid,
            })
            .is_err(),
        "descending bands"
    );

    let plain = chart
        .create_general_xy_dataset(category_input(&[1.0, 2.0]))
        .unwrap();
    let waterfall = chart.create_general_xy_dataset(waterfall_input()).unwrap();
    for options in [
        GeneralSeriesOptions::waterfall(0, plain, "x", "y"),
        GeneralSeriesOptions::bullet(0, plain, "x", "y"),
        GeneralSeriesOptions::bullet(0, waterfall, "x", "y"),
        {
            let mut options = GeneralSeriesOptions::column(0, plain, "x", "y");
            options.waterfall.connectors = false;
            options
        },
        {
            let mut options = GeneralSeriesOptions::waterfall(0, waterfall, "x", "y");
            options.waterfall.increase_color = Some("not a color".into());
            options
        },
        {
            let mut options = GeneralSeriesOptions::waterfall(0, waterfall, "x", "y");
            options.group_id = Some("g".into());
            options
        },
    ] {
        assert!(chart.add_general_series(options).is_err());
        assert_eq!(chart.general_series_count(), 0);
    }
    let series = chart
        .add_general_series(GeneralSeriesOptions::waterfall(0, waterfall, "x", "y"))
        .unwrap();
    assert!(
        chart
            .replace_general_xy_dataset(waterfall, category_input(&[1.0, 2.0]))
            .is_err()
    );
    assert_eq!(chart.general_dataset(waterfall).unwrap().len(), 6);
    let options = GeneralSeriesOptions::bullet(0, waterfall, "x", "y");
    assert!(
        chart
            .update_general_series_options(series, options)
            .is_err()
    );
    assert_eq!(
        chart.general_series(series).unwrap().kind(),
        GeneralSeriesKind::Waterfall
    );
}

#[test]
fn bullet_bands_measure_comparative_and_target_in_both_orientations() {
    for horizontal in [false, true] {
        let mut chart = category_chart(horizontal);
        let dataset = chart.create_general_xy_dataset(bullet_input()).unwrap();
        let series = chart
            .add_general_series(GeneralSeriesOptions::bullet(0, dataset, "x", "y"))
            .unwrap();
        layout(&mut chart);
        let mut bullets = Vec::new();
        chart.visit_general_bullets(chart.general_series(series).unwrap(), |bullet| {
            bullets.push(bullet)
        });
        assert_eq!(bullets.len(), 2, "horizontal={horizontal}");
        let bullet = bullets[0];
        assert_eq!(bullet.horizontal, horizontal);
        let thickness = |bar: crate::general_series::GeneralColumnGeometry| {
            if horizontal {
                bar.bottom - bar.top
            } else {
                bar.right - bar.left
            }
        };
        let start = |bar: crate::general_series::GeneralColumnGeometry| {
            if horizontal { bar.left } else { bar.bottom }
        };
        let end = |bar: crate::general_series::GeneralColumnGeometry| {
            if horizontal { bar.right } else { bar.top }
        };
        let bands = bullet.bands.map(Option::unwrap);
        let slot = thickness(bands[0]);
        assert!(bands.iter().all(|band| close(thickness(*band), slot)));
        // Bands tile zero -> 150 -> 225 -> 300 without gaps.
        assert!(close(end(bands[0]), start(bands[1])));
        assert!(close(end(bands[1]), start(bands[2])));
        let measure = bullet.measure.unwrap();
        let comparative = bullet.comparative.unwrap();
        let target = bullet.target.unwrap();
        assert!(close(thickness(measure), slot / 3.0));
        assert!(close(thickness(comparative), slot / 2.0));
        assert!(close(thickness(target), slot * 2.0 / 3.0));
        assert!(close(start(measure), start(bands[0])));
        // Measure 270 passes the 225 band edge; target 250 sits between them.
        let (measure_end, target_center) = if horizontal {
            (measure.right, (target.left + target.right) / 2.0)
        } else {
            (measure.top, (target.top + target.bottom) / 2.0)
        };
        let band_edge = start(bands[2]);
        if horizontal {
            assert!(band_edge < target_center && target_center < measure_end);
        } else {
            assert!(measure_end < target_center && target_center < band_edge);
        }
        assert_eq!(bullets[1].comparative, None);
        let hit = chart
            .general_hit_test(
                0,
                (measure.left + measure.right) / 2.0,
                (measure.top + measure.bottom) / 2.0,
                GeneralHitMode::Exact,
            )
            .unwrap();
        assert_eq!(hit.row, 0, "horizontal={horizontal}");
        let tooltip = chart.general_tooltip_snapshot(series, 0).unwrap();
        assert_eq!(tooltip.value, Some(270.0));
        assert_eq!(
            (tooltip.low, tooltip.high, tooltip.size),
            (None, None, None)
        );
        assert_eq!(
            tooltip.detail,
            Some(GeneralRowDetail::Bullet {
                target: Some(250.0),
                comparative: Some(230.0),
                bands: [Some(150.0), Some(225.0), Some(300.0)],
            })
        );
        let rects = chart.build_frame().panes[0]
            .main
            .iter()
            .filter(|prim| matches!(prim, Prim::Rect { .. }))
            .count();
        // Row 0: three bands, comparative, measure, target. Row 1 has no comparative.
        assert_eq!(rects, 11, "horizontal={horizontal}");
    }
}

#[test]
fn lollipop_and_dumbbell_marks_paint_stems_and_hit_their_markers() {
    for horizontal in [false, true] {
        let mut chart = category_chart(horizontal);
        let dataset = chart
            .create_general_xy_dataset(category_input(&[4.0, 7.0]))
            .unwrap();
        let mut options = if horizontal {
            GeneralSeriesOptions::horizontal_bar(0, dataset, "x", "y")
        } else {
            GeneralSeriesOptions::column(0, dataset, "x", "y")
        };
        options.bar_mark = GeneralBarMark::Lollipop;
        options.point_radius = 6.0;
        let series = chart.add_general_series(options).unwrap();
        layout(&mut chart);
        let main = chart.build_frame().panes[0].main.clone();
        assert!(!main.iter().any(|prim| matches!(prim, Prim::Rect { .. })));
        assert_eq!(
            main.iter()
                .filter(|prim| matches!(prim, Prim::Polyline { point_count: 2, .. }))
                .count(),
            2
        );
        assert_eq!(
            main.iter()
                .filter(|prim| matches!(prim, Prim::Circle { radius, .. } if *radius == 6.0))
                .count(),
            2
        );
        let mut slots = Vec::new();
        let geometry = chart.general_series(series).unwrap();
        if horizontal {
            chart.visit_general_horizontal_bars(geometry, |bar| slots.push(bar));
        } else {
            chart.visit_general_columns(geometry, |bar| slots.push(bar));
        }
        let [base, end] = crate::general_series::general_bar_stem(slots[1], horizontal);
        for (x, y) in [end, ((base.0 + end.0) / 2.0, (base.1 + end.1) / 2.0)] {
            assert_eq!(
                chart
                    .general_hit_test(0, x, y, GeneralHitMode::Exact)
                    .map(|hit| hit.row),
                Some(1),
                "horizontal={horizontal}"
            );
        }
        // The empty bar slot beside the stem is not ink.
        let (x, y) = if horizontal {
            ((base.0 + end.0) / 2.0, slots[1].top + 1.0)
        } else {
            (slots[1].left + 1.0, (base.1 + end.1) / 2.0)
        };
        assert_eq!(chart.general_hit_test(0, x, y, GeneralHitMode::Exact), None);
    }

    let mut chart = category_chart(false);
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::RangeCategory {
            ids: None,
            categories: vec!["A".into(), "B".into()],
            category_indices: vec![0, 1],
            low: vec![2.0, 5.0],
            low_valid: None,
            high: vec![6.0, 5.0],
            high_valid: None,
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::range_bar(0, dataset, "x", "y");
    options.bar_mark = GeneralBarMark::Dumbbell;
    let series = chart.add_general_series(options).unwrap();
    layout(&mut chart);
    let main = chart.build_frame().panes[0].main.clone();
    // Both rows paint two markers, including the row whose ends coincide.
    assert_eq!(
        main.iter()
            .filter(|prim| matches!(prim, Prim::Circle { .. }))
            .count(),
        4
    );
    let mut bars = Vec::new();
    chart.visit_general_range_bars(chart.general_series(series).unwrap(), |bar| bars.push(bar));
    let [low_end, _] = crate::general_series::general_bar_stem(bars[0], false);
    assert_eq!(
        chart
            .general_hit_test(0, low_end.0, low_end.1, GeneralHitMode::Exact)
            .map(|hit| hit.row),
        Some(0)
    );

    let plain = chart
        .create_general_xy_dataset(category_input(&[1.0, 2.0]))
        .unwrap();
    for options in [
        {
            let mut options = GeneralSeriesOptions::xy_line(0, plain, "x", "y");
            options.bar_mark = GeneralBarMark::Lollipop;
            options
        },
        {
            let mut options = GeneralSeriesOptions::column(0, plain, "x", "y");
            options.bar_mark = GeneralBarMark::Lollipop;
            options.stack_id = Some("s".into());
            options
        },
        {
            let mut options = GeneralSeriesOptions::column(0, plain, "x", "y");
            options.bar_mark = GeneralBarMark::Dumbbell;
            options
        },
        {
            let mut options = GeneralSeriesOptions::column(0, plain, "x", "y");
            options.rank = Some(GeneralRankTies::Dense);
            options
        },
        {
            let mut options = GeneralSeriesOptions::column(0, plain, "x", "y");
            options.end_labels = GeneralEndLabels::End;
            options
        },
    ] {
        assert!(chart.add_general_series(options).is_err());
    }
    assert_eq!(chart.general_series_count(), 1);
}

fn numeric_chart() -> ChartEngine {
    let mut chart = ChartEngine::new_with_initial_domain(
        480.0,
        320.0,
        1.0,
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        },
    )
    .unwrap();
    for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
        let mut axis = GeneralAxisOptions::new(id, 0, dimension, GeneralScaleType::Linear);
        axis.visible = false;
        chart.add_general_axis(axis).unwrap();
    }
    chart
}

fn add_line(
    chart: &mut ChartEngine,
    title: &str,
    y: &[f64],
    configure: impl FnOnce(&mut GeneralSeriesOptions),
) -> GeneralSeriesId {
    let dataset = chart
        .create_general_xy_dataset(GeneralXyInput::Numeric {
            ids: None,
            x: (1..=y.len()).map(|x| x as f64).collect(),
            y: y.to_vec(),
            y_valid: None,
        })
        .unwrap();
    let mut options = GeneralSeriesOptions::xy_line(0, dataset, "x", "y");
    options.title = title.into();
    configure(&mut options);
    chart.add_general_series(options).unwrap()
}

#[test]
fn bump_ranks_follow_the_declared_tie_policy_on_a_reversed_axis() {
    for (ties, expected_a) in [
        (GeneralRankTies::Dense, [Some(2), Some(1)]),
        (GeneralRankTies::Competition, [Some(3), Some(1)]),
    ] {
        let mut chart = numeric_chart();
        let rank = |options: &mut GeneralSeriesOptions| options.rank = Some(ties);
        let a = add_line(&mut chart, "A", &[10.0, 30.0], rank);
        let b = add_line(&mut chart, "B", &[20.0, 10.0], rank);
        let c = add_line(&mut chart, "C", &[20.0, 20.0], rank);
        layout(&mut chart);
        let ranks =
            |series| chart.general_series_ranks(chart.general_series(series).unwrap(), true);
        assert_eq!(ranks(a), expected_a.to_vec(), "{ties:?}");
        assert_eq!(ranks(b)[0], Some(1));
        assert_eq!(ranks(c)[0], Some(1));
        assert_eq!(ranks(b)[1], Some(3));
        assert_eq!(ranks(c)[1], Some(2));
        let y_axis = chart.general_axis("y").unwrap();
        assert_eq!(
            chart.effective_general_axis_domain(y_axis),
            Some(GeneralAxisDomain::Numeric([0.5, 3.5]))
        );
        assert!(chart.general_axis_range_reversed(y_axis));
        let mut points = Vec::new();
        chart.visit_general_path_points(chart.general_series(a).unwrap(), |point| {
            points.push(point.y)
        });
        // Rank 1 paints above every lower rank.
        assert!(points[1] < points[0]);
        let accessibility = chart.general_accessibility_snapshot(a, 0, 2).unwrap();
        assert_eq!(accessibility.items[0].value, Some(10.0));
        assert_eq!(
            accessibility.items[1].detail,
            Some(GeneralRowDetail::Rank(1))
        );
    }
}

#[test]
fn slope_end_labels_name_values_and_never_overlap() {
    let mut chart = numeric_chart();
    let both = |options: &mut GeneralSeriesOptions| options.end_labels = GeneralEndLabels::Both;
    add_line(&mut chart, "Alpha", &[10.0, 20.0], both);
    add_line(&mut chart, "Beta", &[15.0, 20.0], both);
    layout(&mut chart);
    let size = chart.options.get().layout.font_size.max(1.0);
    let texts: Vec<(String, f32)> = chart.build_frame().panes[0]
        .main
        .iter()
        .filter_map(|prim| match prim {
            Prim::Text { text, y, .. } => Some((text.clone(), *y)),
            _ => None,
        })
        .collect();
    let find = |label: &str| {
        texts
            .iter()
            .find(|(text, _)| text == label)
            .unwrap_or_else(|| panic!("{label} in {texts:?}"))
            .1
    };
    find("Alpha 10");
    find("Beta 15");
    let (alpha, beta) = (find("20 Alpha"), find("20 Beta"));
    assert!(
        f64::from((alpha - beta).abs()) >= size * 1.25 - 1e-3,
        "{alpha} vs {beta}"
    );
}

#[test]
fn business_options_and_datasets_round_trip_through_persistence() {
    let mut chart = category_chart(true);
    let waterfall = chart.create_general_xy_dataset(waterfall_input()).unwrap();
    let mut options = GeneralSeriesOptions::waterfall(0, waterfall, "x", "y");
    options.waterfall.subtotal_color = Some("#123456".into());
    options.waterfall.connectors = false;
    chart.add_general_series(options).unwrap();
    let bullet = chart.create_general_xy_dataset(bullet_input()).unwrap();
    chart
        .add_general_series(GeneralSeriesOptions::bullet(0, bullet, "x", "y"))
        .unwrap();
    let lollipop = chart
        .create_general_xy_dataset(category_input(&[3.0, 4.0]))
        .unwrap();
    let mut options = GeneralSeriesOptions::horizontal_bar(0, lollipop, "x", "y");
    options.bar_mark = GeneralBarMark::Lollipop;
    chart.add_general_series(options).unwrap();
    layout(&mut chart);
    let document = chart.export_state_json().unwrap();
    let fresh = |domain| ChartEngine::new_with_initial_domain(480.0, 320.0, 1.0, domain).unwrap();
    let mut restored = fresh(HorizontalDomain::Continuous {
        scale: ContinuousScaleType::Linear,
    });
    restored.import_state_json(&document).unwrap();
    assert_eq!(restored.export_state_json().unwrap(), document);
    let order = restored.general_series_order(Some(0));
    let restored_waterfall = restored.general_series(order[0]).unwrap();
    assert_eq!(restored_waterfall.kind(), GeneralSeriesKind::Waterfall);
    assert_eq!(
        restored_waterfall.waterfall().subtotal_color.as_deref(),
        Some("#123456")
    );
    assert!(!restored_waterfall.waterfall().connectors);
    assert_eq!(
        general_waterfall_rows(
            restored
                .general_dataset(restored_waterfall.dataset())
                .unwrap()
        ),
        general_waterfall_rows(chart.general_dataset(waterfall).unwrap())
    );
    assert_eq!(
        restored
            .general_tooltip_snapshot(order[1], 0)
            .unwrap()
            .detail,
        chart
            .general_tooltip_snapshot(chart.general_series_order(Some(0))[1], 0)
            .unwrap()
            .detail
    );
    assert_eq!(
        restored.general_series(order[2]).unwrap().bar_mark(),
        GeneralBarMark::Lollipop
    );

    let mut chart = numeric_chart();
    add_line(&mut chart, "A", &[1.0, 2.0], |options| {
        options.rank = Some(GeneralRankTies::Competition);
        options.end_labels = GeneralEndLabels::End;
    });
    let document = chart.export_state_json().unwrap();
    let mut restored = fresh(HorizontalDomain::Continuous {
        scale: ContinuousScaleType::Linear,
    });
    restored.import_state_json(&document).unwrap();
    let series = restored
        .general_series(restored.general_series_order(Some(0))[0])
        .unwrap();
    assert_eq!(series.rank(), Some(GeneralRankTies::Competition));
    assert_eq!(series.end_labels(), GeneralEndLabels::End);
}
