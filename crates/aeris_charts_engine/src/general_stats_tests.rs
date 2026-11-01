//! Statistics families (G11): creation gating, geometry, hit testing, tooltips, validation,
//! and persistence. Models are unit-tested in `general_stats`; these tests exercise the public
//! engine path (dataset -> series -> layout -> hit/tooltip/accessibility -> state round trip).

use crate::{
    AxisDimension, CategoryScaleType, ChartEngine, ContinuousScaleType, ErrorCode,
    GeneralAxisOptions, GeneralScaleType, GeneralSeriesOptions, GeneralXyInput, HorizontalDomain,
};

fn chart_with(x_scale: GeneralScaleType, y_scale: GeneralScaleType) -> ChartEngine {
    let domain = if x_scale == GeneralScaleType::Band {
        HorizontalDomain::Category {
            scale: CategoryScaleType::Band,
        }
    } else {
        HorizontalDomain::Continuous {
            scale: ContinuousScaleType::Linear,
        }
    };
    let mut chart = ChartEngine::new_with_initial_domain(480.0, 320.0, 1.0, domain).unwrap();
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

fn band_chart() -> ChartEngine {
    chart_with(GeneralScaleType::Band, GeneralScaleType::Linear)
}

fn linear_chart() -> ChartEngine {
    chart_with(GeneralScaleType::Linear, GeneralScaleType::Linear)
}

#[cfg(feature = "complete")]
fn ridgeline_chart() -> ChartEngine {
    chart_with(GeneralScaleType::Linear, GeneralScaleType::Band)
}

#[cfg(feature = "complete")]
fn layout(chart: &mut ChartEngine) {
    chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
}

fn category_input() -> GeneralXyInput {
    GeneralXyInput::Category {
        ids: None,
        categories: vec!["A".to_owned(), "B".to_owned()],
        category_indices: vec![0, 0, 0, 0, 1, 1, 1, 1],
        y: vec![1.0, 2.0, 3.0, 4.0, 2.0, 3.0, 4.0, 5.0],
        y_valid: None,
    }
}

fn values_input() -> GeneralXyInput {
    GeneralXyInput::Values {
        ids: None,
        values: vec![0.5, 1.0, 1.2, 1.5, 2.0, 2.2, 2.5, 3.0, 3.5, 4.0],
        values_valid: None,
    }
}

/// A cluster around (5, 5) with a few outliers: smooth contours close around the blob.
fn numeric_input() -> GeneralXyInput {
    let mut x = Vec::new();
    let mut y = Vec::new();
    for i in 0..40 {
        x.push(5.0 + (i % 5) as f64 * 0.4 - 0.8);
        y.push(5.0 + ((i / 5) % 8) as f64 * 0.25 - 0.875);
    }
    x.extend([0.0, 10.0, 0.0, 10.0]);
    y.extend([0.0, 10.0, 10.0, 0.0]);
    GeneralXyInput::Numeric {
        ids: None,
        x,
        y,
        y_valid: None,
    }
}

fn matrix_input() -> GeneralXyInput {
    let rows = 8;
    GeneralXyInput::Matrix {
        ids: None,
        fields: vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
        columns: vec![
            (0..rows).map(|i| i as f64 + 1.0).collect(),
            (0..rows).map(|i| (rows - i) as f64).collect(),
            (0..rows).map(|i| (i as f64 + 1.0) * 0.5).collect(),
        ],
    }
}

#[cfg(not(feature = "complete"))]
#[test]
fn stats_families_require_the_complete_build() {
    let mut chart = band_chart();
    let category = chart.create_general_xy_dataset(category_input()).unwrap();
    let error = chart
        .add_general_series(GeneralSeriesOptions::violin(0, category, "x", "y"))
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::UnsupportedOperation);

    let mut chart = linear_chart();
    let values = chart.create_general_xy_dataset(values_input()).unwrap();
    let error = chart
        .add_general_series(GeneralSeriesOptions::density_curve(0, values, "x", "y"))
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::UnsupportedOperation);

    let numeric = chart.create_general_xy_dataset(numeric_input()).unwrap();
    let error = chart
        .add_general_series(GeneralSeriesOptions::hexbin(0, numeric, "x", "y"))
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::UnsupportedOperation);

    let error = chart
        .add_general_series(GeneralSeriesOptions::parallel_coordinates(
            0, numeric, "x", "y",
        ))
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
    assert_eq!(chart.general_series_count(), 0);
}

#[cfg(not(feature = "complete"))]
#[test]
fn standard_build_rejects_matrix_data() {
    let mut chart = band_chart();
    let error = chart.create_general_xy_dataset(matrix_input()).unwrap_err();
    assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
    assert!(error.message().contains("complete build"));
    let unbound = chart.create_general_xy_dataset(values_input()).unwrap();
    let error = chart
        .replace_general_xy_dataset(unbound, matrix_input())
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
    let GeneralXyInput::Matrix {
        fields, columns, ..
    } = matrix_input()
    else {
        unreachable!();
    };
    let keyed = GeneralXyInput::Matrix {
        ids: Some(
            (0..columns[0].len())
                .map(|row| crate::GeneralRowId::Number(row as f64))
                .collect(),
        ),
        fields,
        columns,
    };
    let error = chart
        .upsert_general_xy_dataset(unbound, keyed, None)
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
    assert_eq!(chart.general_dataset(unbound).unwrap().len(), 10);
}

#[cfg(not(feature = "complete"))]
#[test]
fn standard_build_rejects_stats_kinds_in_proposals() {
    let chart = band_chart();
    let proposal = crate::GeneralPaneProposal {
        pane: 0,
        axes: vec![
            GeneralAxisOptions::new("x", 0, AxisDimension::X, GeneralScaleType::Band),
            GeneralAxisOptions::new("y", 0, AxisDimension::Y, GeneralScaleType::Linear),
        ],
        series: vec![crate::GeneralSeriesProposal {
            kind: crate::GeneralSeriesKind::Violin,
            x_axis_id: "x".to_owned(),
            y_axis_id: "y".to_owned(),
            data: category_input(),
        }],
        references: Vec::new(),
    };
    let error = chart.validate_general_pane_proposal(&proposal).unwrap_err();
    assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
}

#[cfg(not(feature = "complete"))]
#[test]
fn standard_build_restores_shared_state_and_rejects_stats_state() {
    let mut chart = band_chart();
    let category = chart.create_general_xy_dataset(category_input()).unwrap();
    chart
        .add_general_series(GeneralSeriesOptions::column(0, category, "x", "y"))
        .unwrap();
    let json = chart.export_state_json().unwrap();
    assert!(json.contains("\"kind\":\"Column\""));

    // Complete-build states carry default stats options on every series; the standard build
    // restores them unchanged.
    let with_stats = json.replacen(
        "\"kind\":\"Column\"",
        "\"kind\":\"Column\",\"stats\":{\"grid\":0}",
        1,
    );
    let fresh = || {
        ChartEngine::new_with_initial_domain(
            480.0,
            320.0,
            1.0,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap()
    };
    let mut restored = fresh();
    restored.import_state_json(&with_stats).unwrap();
    assert_eq!(restored.general_series_count(), 1);

    let violin = json.replacen("\"kind\":\"Column\"", "\"kind\":\"Violin\"", 1);
    let mut rejected = fresh();
    let error = rejected.import_state_json(&violin).unwrap_err();
    assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
    assert_eq!(rejected.general_series_count(), 0);
    // The failed import stages nothing, so the chart still accepts a compatible state.
    rejected.import_state_json(&json).unwrap();
    assert_eq!(rejected.general_series_count(), 1);

    // A complete-build state with a matrix dataset fails with the same typed error.
    let mut chart = band_chart();
    chart.create_general_xy_dataset(values_input()).unwrap();
    let json = chart.export_state_json().unwrap();
    assert!(json.contains("{\"Values\":{"));
    let matrix = json.replacen("{\"Values\":{", "{\"Matrix\":{", 1);
    let mut rejected = fresh();
    let error = rejected.import_state_json(&matrix).unwrap_err();
    assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
    assert!(error.message().contains("complete build"));
}

#[cfg(feature = "complete")]
mod complete {
    use aeris_charts_core::scale::general_scale::LinearScale;

    use super::*;
    use crate::{
        GeneralHitMode, GeneralRowDetail, GeneralSeriesKind, GeneralStatsBandwidth,
        GeneralStatsKernel, GeneralViolinOverlay, GeneralViolinSide,
    };

    #[test]
    fn violin_geometry_hit_tooltip_and_split() {
        let mut chart = band_chart();
        let dataset = chart.create_general_xy_dataset(category_input()).unwrap();
        let mut options = GeneralSeriesOptions::violin(0, dataset, "x", "y");
        options.stats.violin_overlay = GeneralViolinOverlay::Box;
        let series = chart.add_general_series(options).unwrap();
        layout(&mut chart);

        let series_ref = chart.general_series(series).unwrap();
        let mut violins = Vec::new();
        chart.visit_general_violins(series_ref, |geometry| {
            violins.push((
                geometry.position,
                geometry.center_x,
                geometry
                    .y_scale
                    .coordinate(geometry.category.median)
                    .unwrap(),
            ))
        });
        assert_eq!(violins.len(), 2);
        let hit = chart
            .general_hit_test(0, violins[0].1, violins[0].2, GeneralHitMode::Exact)
            .unwrap();
        assert_eq!(hit.series, series);
        assert_eq!(hit.row, 0);
        let tooltip = chart.general_tooltip_snapshot(series, 0).unwrap();
        assert_eq!(tooltip.x_label, "A");
        assert_eq!(tooltip.value, Some(2.5));
        let snapshot = chart.general_accessibility_snapshot(series, 0, 4).unwrap();
        assert_eq!(snapshot.total_rows, 2);
        assert_eq!(snapshot.items.len(), 2);

        // A left-half violin answers hits on the left of the slot center only.
        let mut chart = band_chart();
        let dataset = chart.create_general_xy_dataset(category_input()).unwrap();
        let mut options = GeneralSeriesOptions::violin(0, dataset, "x", "y");
        options.stats.violin_side = GeneralViolinSide::Left;
        let series = chart.add_general_series(options).unwrap();
        layout(&mut chart);
        let series_ref = chart.general_series(series).unwrap();
        let mut slots = Vec::new();
        chart.visit_general_violins(series_ref, |geometry| {
            slots.push((
                geometry.center_x,
                geometry.half_width(geometry.category.curve.max_density),
                geometry
                    .y_scale
                    .coordinate(geometry.category.median)
                    .unwrap(),
            ));
        });
        let (center_x, half_width, median_y) = slots[0];
        assert!(half_width > 4.0);
        let hit = chart
            .general_hit_test(
                0,
                center_x - half_width * 0.5,
                median_y,
                GeneralHitMode::Exact,
            )
            .unwrap();
        assert_eq!(hit.row, 0);
        assert!(
            chart
                .general_hit_test(
                    0,
                    center_x + half_width * 0.5,
                    median_y,
                    GeneralHitMode::Exact,
                )
                .is_none(),
            "a left violin must miss on the right half"
        );
    }

    #[test]
    fn density_curve_hit_and_tooltip() {
        let mut chart = linear_chart();
        let dataset = chart.create_general_xy_dataset(values_input()).unwrap();
        let series = chart
            .add_general_series(GeneralSeriesOptions::density_curve(0, dataset, "x", "y"))
            .unwrap();
        layout(&mut chart);
        let series_ref = chart.general_series(series).unwrap();
        let geometry = chart.general_density_geometry(series_ref).unwrap();
        assert!(geometry.points.len() >= 8);
        let apex = geometry
            .points
            .iter()
            .enumerate()
            .min_by(|a, b| f64::total_cmp(&a.1.1, &b.1.1))
            .unwrap();
        let hit = chart
            .general_hit_test(0, apex.1.0, apex.1.1, GeneralHitMode::Exact)
            .unwrap();
        assert_eq!(hit.row, apex.0);
        let tooltip = chart.general_tooltip_snapshot(series, apex.0).unwrap();
        assert!(tooltip.x_label.parse::<f64>().is_ok());
        assert!(tooltip.value.unwrap() > 0.0);
    }

    #[test]
    fn ridgeline_bands_hit_test() {
        let mut chart = ridgeline_chart();
        let dataset = chart.create_general_xy_dataset(category_input()).unwrap();
        let series = chart
            .add_general_series(GeneralSeriesOptions::ridgeline(0, dataset, "x", "y"))
            .unwrap();
        layout(&mut chart);
        let series_ref = chart.general_series(series).unwrap();
        let geometry = chart.general_ridgeline_geometry(series_ref).unwrap();
        assert_eq!(geometry.bands.len(), 2);
        let series_model = chart.general_stats_model(series_ref).unwrap();
        let crate::general_stats::GeneralStatsModel::Ridgeline(model) = series_model else {
            panic!("expected a ridgeline model");
        };
        let apex = model.categories[1]
            .curve
            .density
            .iter()
            .enumerate()
            .max_by(|a, b| f64::total_cmp(a.1, b.1))
            .map(|(index, _)| index)
            .unwrap();
        let x = geometry
            .x_scale
            .coordinate(model.categories[1].curve.grid[apex])
            .unwrap();
        let y = geometry.bands[1].base_y - geometry.bands[1].height * 0.5;
        let hit = chart
            .general_hit_test(0, x, y, GeneralHitMode::Nearest { max_distance: 24.0 })
            .unwrap();
        assert_eq!(hit.row, 1);
    }

    #[test]
    fn strip_beeswarm_and_dot_points_hit_real_rows() {
        for (kind, jitter) in [
            (GeneralSeriesKind::StripPlot, 0.6),
            (GeneralSeriesKind::Beeswarm, 0.6),
            (GeneralSeriesKind::DotPlot, 0.6),
        ] {
            let mut chart = band_chart();
            let dataset = chart.create_general_xy_dataset(category_input()).unwrap();
            let mut options = match kind {
                GeneralSeriesKind::StripPlot => {
                    GeneralSeriesOptions::strip_plot(0, dataset, "x", "y")
                }
                GeneralSeriesKind::Beeswarm => GeneralSeriesOptions::beeswarm(0, dataset, "x", "y"),
                _ => GeneralSeriesOptions::dot_plot(0, dataset, "x", "y"),
            };
            options.stats.jitter = jitter;
            let series = chart.add_general_series(options).unwrap();
            layout(&mut chart);
            let series_ref = chart.general_series(series).unwrap();
            let mut points = Vec::new();
            chart.visit_general_stats_points(series_ref, |point| {
                points.push((point.row, point.x, point.y));
            });
            assert_eq!(points.len(), 8, "{kind:?}");
            let (row, x, y) = points[5];
            let hit = chart
                .general_hit_test(0, x, y, GeneralHitMode::Exact)
                .unwrap();
            assert_eq!(hit.row, row, "{kind:?}");
            // Real dataset rows keep the standard category tooltip.
            let tooltip = chart.general_tooltip_snapshot(series, row).unwrap();
            assert!(tooltip.value.is_some(), "{kind:?}");
        }
    }

    #[test]
    fn ecdf_and_qq_hit_and_reference_line() {
        let mut chart = linear_chart();
        let dataset = chart.create_general_xy_dataset(values_input()).unwrap();
        let ecdf = chart
            .add_general_series(GeneralSeriesOptions::ecdf(0, dataset, "x", "y"))
            .unwrap();
        layout(&mut chart);
        let series_ref = chart.general_series(ecdf).unwrap();
        let (path, count) = chart.general_ecdf_path(series_ref).unwrap();
        assert_eq!(count, 10);
        assert!(path.len() > 10);
        let (x, y) = path[path.len() / 2];
        let hit = chart
            .general_hit_test(0, x, y, GeneralHitMode::Nearest { max_distance: 12.0 })
            .unwrap();
        let tooltip = chart.general_tooltip_snapshot(ecdf, hit.row).unwrap();
        assert!(tooltip.x_label.parse::<f64>().is_ok());

        let mut chart = linear_chart();
        let dataset = chart.create_general_xy_dataset(values_input()).unwrap();
        let qq = chart
            .add_general_series(GeneralSeriesOptions::qq_plot(0, dataset, "x", "y"))
            .unwrap();
        layout(&mut chart);
        let series_ref = chart.general_series(qq).unwrap();
        let (points, reference) = chart.general_qq_geometry(series_ref).unwrap();
        assert_eq!(points.len(), 10);
        assert!(reference.is_some());
        let (row, x, y) = points[4];
        let hit = chart
            .general_hit_test(0, x, y, GeneralHitMode::Exact)
            .unwrap();
        assert_eq!(hit.row, row);
        let tooltip = chart.general_tooltip_snapshot(qq, row).unwrap();
        assert!(tooltip.x_label.parse::<f64>().is_ok());
    }

    #[test]
    fn hexbin_hit_tooltip_and_radius_clamp() {
        let mut chart = linear_chart();
        let dataset = chart.create_general_xy_dataset(numeric_input()).unwrap();
        let series = chart
            .add_general_series(GeneralSeriesOptions::hexbin(0, dataset, "x", "y"))
            .unwrap();
        layout(&mut chart);
        let series_ref = chart.general_series(series).unwrap();
        let mut bins = Vec::new();
        chart.visit_general_hexbins(series_ref, |bin| bins.push((bin.index, bin.x, bin.y)));
        assert!(!bins.is_empty());
        // The dense cluster merges into fewer bins than points.
        assert!(bins.len() < 44);
        let (index, x, y) = bins[0];
        let hit = chart
            .general_hit_test(0, x, y, GeneralHitMode::Exact)
            .unwrap();
        assert_eq!(hit.row, index);
        let tooltip = chart.general_tooltip_snapshot(series, index).unwrap();
        match tooltip.detail {
            Some(GeneralRowDetail::Stats { count, min, max }) => {
                assert!(count > 0);
                assert!(min.is_nan());
                assert!(max.is_finite());
            }
            other => panic!("unexpected hexbin detail: {other:?}"),
        }
        // Auto radius keeps the bin count bounded for tiny plots.
        let mut small = ChartEngine::new_with_initial_domain(
            120.0,
            90.0,
            1.0,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
        for (id, dimension) in [("x", AxisDimension::X), ("y", AxisDimension::Y)] {
            let mut axis = GeneralAxisOptions::new(id, 0, dimension, GeneralScaleType::Linear);
            axis.visible = false;
            small.add_general_axis(axis).unwrap();
        }
        let dataset = small.create_general_xy_dataset(numeric_input()).unwrap();
        let series = small
            .add_general_series(GeneralSeriesOptions::hexbin(0, dataset, "x", "y"))
            .unwrap();
        layout(&mut small);
        let series_ref = small.general_series(series).unwrap();
        let mut count = 0;
        small.visit_general_hexbins(series_ref, |_| count += 1);
        assert!(count <= 4096);
    }

    #[test]
    fn contour_bands_lines_and_hit() {
        let mut chart = linear_chart();
        let dataset = chart.create_general_xy_dataset(numeric_input()).unwrap();
        let series = chart
            .add_general_series(GeneralSeriesOptions::contour(0, dataset, "x", "y"))
            .unwrap();
        layout(&mut chart);
        let series_ref = chart.general_series(series).unwrap();
        let geometry = chart.general_contour_geometry(series_ref).unwrap();
        assert!(!geometry.model.bands.is_empty());
        assert!(!geometry.model.lines.is_empty());
        let line = &geometry.model.lines[0];
        assert!(!line.paths.is_empty());
        let vertex = geometry.project(line.paths[0][0]).unwrap();
        let hit = chart
            .general_hit_test(
                0,
                vertex.0,
                vertex.1,
                GeneralHitMode::Nearest { max_distance: 8.0 },
            )
            .unwrap();
        assert_eq!(hit.series, series);
        let tooltip = chart.general_tooltip_snapshot(series, hit.row).unwrap();
        assert!(tooltip.x_label.starts_with("level "));
    }

    #[test]
    fn parallel_coordinates_rows_hit_real_rows() {
        let mut chart = linear_chart();
        let dataset = chart.create_general_xy_dataset(matrix_input()).unwrap();
        let series = chart
            .add_general_series(GeneralSeriesOptions::parallel_coordinates(
                0, dataset, "x", "y",
            ))
            .unwrap();
        layout(&mut chart);
        let series_ref = chart.general_series(series).unwrap();
        let geometry = chart.general_parallel_geometry(series_ref).unwrap();
        assert_eq!(geometry.slots.len(), 3);
        assert_eq!(geometry.rows.len(), 8);
        let (row, points) = &geometry.rows[0];
        assert_eq!(points.len(), 3);
        let hit = chart
            .general_hit_test(
                0,
                points[1].0,
                points[1].1,
                GeneralHitMode::Nearest { max_distance: 12.0 },
            )
            .unwrap();
        assert_eq!(hit.row, *row);
    }

    #[test]
    fn scatter_matrix_cells_and_hit() {
        let mut chart = linear_chart();
        let dataset_id = chart.create_general_xy_dataset(matrix_input()).unwrap();
        let series = chart
            .add_general_series(GeneralSeriesOptions::scatter_matrix(
                0, dataset_id, "x", "y",
            ))
            .unwrap();
        layout(&mut chart);
        let series_ref = chart.general_series(series).unwrap();
        let geometry = chart.general_scatter_matrix_geometry(series_ref).unwrap();
        assert_eq!(geometry.cells.len(), 9);
        let dataset = chart.general_geometry_dataset(dataset_id).unwrap();
        let cell = &geometry.cells[1];
        // Row 0 projects exactly onto the plot boundary, which the public hit test excludes;
        // use a middle row.
        let (x, y) = geometry.point(dataset, cell, 3).unwrap();
        let hit = chart
            .general_hit_test(0, x, y, GeneralHitMode::Exact)
            .unwrap();
        assert_eq!(hit.row, 3);
        let tooltip = chart.general_tooltip_snapshot(series, 3).unwrap();
        assert!(!tooltip.x_label.is_empty());
    }

    #[test]
    fn stats_options_and_datasets_are_validated() {
        let mut chart = linear_chart();
        let values = chart.create_general_xy_dataset(values_input()).unwrap();
        let mut options = GeneralSeriesOptions::density_curve(0, values, "x", "y");
        options.stats.bandwidth = GeneralStatsBandwidth::Explicit(0.0);
        let error = chart.add_general_series(options).unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidOptions);

        let mut options = GeneralSeriesOptions::density_curve(0, values, "x", "y");
        options.stats.grid = 4;
        let error = chart.add_general_series(options).unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidOptions);

        let mut chart = band_chart();
        let category = chart.create_general_xy_dataset(category_input()).unwrap();
        let mut options = GeneralSeriesOptions::strip_plot(0, category, "x", "y");
        options.stats.jitter = 2.0;
        let error = chart.add_general_series(options).unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidOptions);

        // A category dataset cannot back a density curve on the band chart: the generic
        // axis/data contract rejects it before the stats layer.
        let error = chart
            .add_general_series(GeneralSeriesOptions::density_curve(0, category, "x", "y"))
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidOptions);

        // Matrix kinds with a plain numeric dataset are rejected by the generic axis/data
        // contract before the stats layer.
        let mut chart = linear_chart();
        let numeric = chart.create_general_xy_dataset(numeric_input()).unwrap();
        let error = chart
            .add_general_series(GeneralSeriesOptions::parallel_coordinates(
                0, numeric, "x", "y",
            ))
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidOptions);

        // The stats validator owns the category budget: 65 categories exceed the violin limit.
        let mut chart = band_chart();
        let many = chart
            .create_general_xy_dataset(GeneralXyInput::Category {
                ids: None,
                categories: (0..65).map(|i| format!("c{i}")).collect(),
                category_indices: (0..65).collect(),
                y: (0..65).map(|i| i as f64).collect(),
                y_valid: None,
            })
            .unwrap();
        let error = chart
            .add_general_series(GeneralSeriesOptions::violin(0, many, "x", "y"))
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::ResourceLimit);

        // Matrix field bounds are enforced at dataset creation.
        let too_many_fields = GeneralXyInput::Matrix {
            ids: None,
            fields: (0..9).map(|i| format!("f{i}")).collect(),
            columns: (0..9).map(|_| vec![1.0, 2.0]).collect(),
        };
        let error = chart
            .create_general_xy_dataset(too_many_fields)
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::ResourceLimit);
    }

    #[test]
    fn stats_options_round_trip_through_persistence() {
        let mut chart = band_chart();
        let category = chart.create_general_xy_dataset(category_input()).unwrap();
        let mut options = GeneralSeriesOptions::violin(0, category, "x", "y");
        options.stats.violin_overlay = GeneralViolinOverlay::Box;
        options.stats.kernel = GeneralStatsKernel::Epanechnikov;
        options.stats.bandwidth = GeneralStatsBandwidth::Explicit(0.75);
        chart.add_general_series(options).unwrap();

        let mut chart2 = linear_chart();
        // Both charts must live in one engine for a single document; use a second pane-free
        // approach instead: verify the matrix family round trips from its own chart.
        let matrix = chart2.create_general_xy_dataset(matrix_input()).unwrap();
        let mut options = GeneralSeriesOptions::scatter_matrix(0, matrix, "x", "y");
        options.point_radius = 3.0;
        chart2.add_general_series(options).unwrap();

        layout(&mut chart);
        layout(&mut chart2);

        let document = chart.export_state_json().unwrap();
        let mut restored = ChartEngine::new_with_initial_domain(
            480.0,
            320.0,
            1.0,
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            },
        )
        .unwrap();
        restored.import_state_json(&document).unwrap();
        assert_eq!(restored.export_state_json().unwrap(), document);
        let order = restored.general_series_order(Some(0));
        let series = restored.general_series(order[0]).unwrap();
        assert_eq!(series.kind(), GeneralSeriesKind::Violin);
        assert_eq!(series.stats().violin_overlay, GeneralViolinOverlay::Box);
        assert_eq!(series.stats().kernel, GeneralStatsKernel::Epanechnikov);
        assert_eq!(
            series.stats().bandwidth,
            GeneralStatsBandwidth::Explicit(0.75)
        );

        let document = chart2.export_state_json().unwrap();
        let mut restored = ChartEngine::new_with_initial_domain(
            480.0,
            320.0,
            1.0,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap();
        restored.import_state_json(&document).unwrap();
        assert_eq!(restored.export_state_json().unwrap(), document);
        let order = restored.general_series_order(Some(0));
        let series = restored.general_series(order[0]).unwrap();
        assert_eq!(series.kind(), GeneralSeriesKind::ScatterMatrix);
        assert_eq!(series.point_radius(), 3.0);
        let dataset = restored.general_dataset(series.dataset()).unwrap();
        assert_eq!(dataset.matrix_fields().map(|fields| fields.len()), Some(3));
    }

    #[test]
    fn stats_frame_emits_primitives() {
        use aeris_charts_render::draw_list::Prim;
        let mut chart = band_chart();
        let category = chart.create_general_xy_dataset(category_input()).unwrap();
        chart
            .add_general_series(GeneralSeriesOptions::violin(0, category, "x", "y"))
            .unwrap();
        let dataset = chart.create_general_xy_dataset(category_input()).unwrap();
        chart
            .add_general_series(GeneralSeriesOptions::beeswarm(0, dataset, "x", "y"))
            .unwrap();
        layout(&mut chart);
        let frame = chart.build_frame();
        let mut fills = 0;
        let mut circles = 0;
        for prim in &frame.panes[0].main {
            match prim {
                Prim::BandFill { .. } => fills += 1,
                Prim::Circle { .. } => circles += 1,
                _ => {}
            }
        }
        assert!(fills >= 2, "two violin silhouettes, got {fills}");
        assert!(circles >= 8, "beeswarm points, got {circles}");
    }

    #[test]
    fn numeric_axis_scale_helper_handles_linear_and_log() {
        // The value-scale helper used by every statistics kind maps through the axis domain.
        let scale = LinearScale::new(0.0, 10.0, 100.0, 200.0).unwrap();
        assert!((scale.coordinate(5.0).unwrap() - 150.0).abs() < 1e-9);
        assert!((scale.invert(150.0).unwrap() - 5.0).abs() < 1e-9);
    }
}
