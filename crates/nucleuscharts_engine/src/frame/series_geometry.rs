//! Per-kind series geometry builders (grid, candles, bars, histogram, line, baseline,
//! price lines, markers, last-value line and pulse) emitting backend-neutral prims.

use super::*;
use nucleuscharts_core::TimePointIndex;

/// Emit a polyline stroke. A solid style emits a single `Polyline` prim (the backends expand
/// `line_type` themselves, as before). Any dashed style is expanded with `line_type` and split
/// into solid dash sub-segments here in the frame builder — reference `setLineDash` semantics on the
/// device-px path (draw-line.ts `getDashPattern`) — because the WebGPU tessellator has no dash
/// concept; generating the gap geometry once keeps both backends pixel-identical by
/// construction.
#[allow(clippy::too_many_arguments)]
fn push_line_stroke(
    out: &mut Vec<Prim>,
    points: &mut Vec<[f32; 2]>,
    window: &[[f32; 2]],
    width: f32,
    style: LineStyle,
    line_type: LineType,
    color: Color,
) {
    let pattern = style.dash_pattern(width);
    if pattern.is_empty() {
        let first = points.len() as u32;
        points.extend_from_slice(window);
        out.push(Prim::Polyline {
            first_point: first,
            point_count: window.len() as u32,
            width,
            style: LineStyle::Solid,
            line_type,
            color,
        });
        return;
    }
    let device: Vec<LinePoint> = window
        .iter()
        .map(|p| LinePoint {
            x: p[0] as f64,
            y: p[1] as f64,
        })
        .collect();
    let expanded = expand_line(&device, line_type);
    let pattern: Vec<f64> = pattern.iter().map(|&len| len as f64).collect();
    for run in dash_split(&expanded, &pattern) {
        let first = points.len() as u32;
        points.extend(run.iter().map(|p| [p.x as f32, p.y as f32]));
        out.push(Prim::Polyline {
            first_point: first,
            point_count: run.len() as u32,
            width,
            style: LineStyle::Solid,
            line_type: LineType::Simple,
            color,
        });
    }
}

/// Per-point-color stroke runs over a resolved per-point color list, porting reference walkLine's
/// style splitting (renderers/walk-line.ts): the segment from point `i` to `i+1` takes
/// `colors[i]` — `changeStyle` strokes the accumulated old-style path up to and including the
/// point where the new style first appears, so a point's color governs the segment leaving it,
/// and the last point's color shows only in its point marker. Yields `(start, end)` as an
/// exclusive point range plus the run color; adjacent runs share their boundary point, keeping
/// the path continuous.
fn color_runs(colors: &[Color]) -> Vec<(usize, usize, Color)> {
    let n = colors.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![(0, 1, colors[0])];
    }
    let mut out = Vec::new();
    let mut run_start = 0usize;
    for i in 1..n {
        if i == n - 1 || colors[i] != colors[run_start] {
            out.push((run_start, i + 1, colors[run_start]));
            run_start = i;
        }
    }
    out
}

impl ChartEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_grid_frame(
        &self,
        out: &mut Vec<Prim>,
        marks: &[(i64, u8)],
        from: i64,
        to: i64,
        width: i32,
        top: i32,
        height: i32,
        hpr: f64,
        vpr: f64,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
        tick_base: i64,
    ) {
        let grid = &self.options.get().grid;
        let vert = css_color(&grid.vert_lines.color, GRID);
        let horz = css_color(&grid.horz_lines.color, GRID);
        // reference lineStyle (0 solid … 4 sparse-dotted); the backends expand dash patterns into
        // segment rects identically.
        let vert_style = crate::line_style_from_u8(grid.vert_lines.style);
        let horz_style = crate::line_style_from_u8(grid.horz_lines.style);
        let lw = 1f64.max(hpr.floor()) as i32;
        if grid.vert_lines.visible {
            for &(idx, _) in marks {
                if idx >= from && idx <= to {
                    out.push(Prim::VLine {
                        x: (self.time_scale.index_to_coordinate(idx) * hpr).round() as i32,
                        y0: top - lw,
                        y1: top + height + lw,
                        width: lw,
                        style: vert_style,
                        color: vert,
                    });
                }
            }
        }
        if grid.horz_lines.visible {
            for mark in scale.build_tick_marks(tick_base, 0.0) {
                out.push(Prim::HLine {
                    y: (mark.coord * vpr).round() as i32,
                    x0: -lw,
                    x1: width + lw,
                    width: lw,
                    style: horz_style,
                    color: horz,
                });
            }
        }
    }

    #[allow(clippy::too_many_arguments)] // mirrors the reference renderer-data signature
    pub(super) fn build_candles_frame(
        &self,
        rs: ResolvedSeries,
        from: i64,
        to: i64,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
    ) {
        let plot = self.data.plot(rs.id);
        let mut work = conflation::DensityWork::default();
        let visible = visible_ohlc_with_work(
            plot,
            from,
            to,
            self.time_scale.bar_spacing(),
            hpr,
            |index| self.time_scale.index_to_coordinate(index) * hpr,
            &mut work,
        );
        self.record_lod_work(
            work.selected_level,
            work.summary_nodes,
            work.raw_rows,
            work.candidates,
        );
        let point_colors = self.data.point_colors(rs.id);
        let items = visible
            .into_iter()
            .map(|bar| {
                let rising = bar.close >= bar.open;
                // reference data-item colors (series-bar-colorer.ts Candlestick arm): a per-point
                // override wins over the series' up/down resolution for its own channel.
                let point = |channel: PointColorChannel| {
                    point_colors
                        .and_then(|colors| colors.color(channel, bar.source_row))
                        .map(Color)
                };
                CandleItem {
                    x: bar.x_px / hpr,
                    open_y: scale.price_to_coordinate(bar.open, rs.base_value),
                    high_y: scale.price_to_coordinate(bar.high, rs.base_value),
                    low_y: scale.price_to_coordinate(bar.low, rs.base_value),
                    close_y: scale.price_to_coordinate(bar.close, rs.base_value),
                    body_color: point(PointColorChannel::Body).unwrap_or(if rising {
                        rs.up
                    } else {
                        rs.down
                    }),
                    border_color: point(PointColorChannel::Border).unwrap_or(if rising {
                        rs.border_up
                    } else {
                        rs.border_down
                    }),
                    wick_color: point(PointColorChannel::Wick).unwrap_or(if rising {
                        rs.wick_up
                    } else {
                        rs.wick_down
                    }),
                }
            })
            .collect::<Vec<_>>();
        build_candles(
            &items,
            &CandlesParams {
                bar_spacing: self.time_scale.bar_spacing(),
                horizontal_pixel_ratio: hpr,
                vertical_pixel_ratio: vpr,
                wick_visible: rs.wick_visible,
                border_visible: rs.border_visible,
            },
            out,
        );
    }

    #[allow(clippy::too_many_arguments)] // mirrors the reference renderer-data signature
    pub(super) fn build_bars_frame(
        &self,
        rs: ResolvedSeries,
        from: i64,
        to: i64,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
    ) {
        let plot = self.data.plot(rs.id);
        let mut work = conflation::DensityWork::default();
        let visible = visible_ohlc_with_work(
            plot,
            from,
            to,
            self.time_scale.bar_spacing(),
            hpr,
            |index| self.time_scale.index_to_coordinate(index) * hpr,
            &mut work,
        );
        self.record_lod_work(
            work.selected_level,
            work.summary_nodes,
            work.raw_rows,
            work.candidates,
        );
        let point_colors = self.data.point_colors(rs.id);
        let items = visible
            .into_iter()
            .map(|bar| BarItem {
                x: bar.x_px / hpr,
                open_y: scale.price_to_coordinate(bar.open, rs.base_value),
                high_y: scale.price_to_coordinate(bar.high, rs.base_value),
                low_y: scale.price_to_coordinate(bar.low, rs.base_value),
                close_y: scale.price_to_coordinate(bar.close, rs.base_value),
                // reference data-item color (series-bar-colorer.ts Bar arm): a per-point `color`
                // overrides the bar's up/down body color.
                color: point_colors
                    .and_then(|colors| colors.color(PointColorChannel::Body, bar.source_row))
                    .map(Color)
                    .unwrap_or(if bar.close >= bar.open {
                        rs.up
                    } else {
                        rs.down
                    }),
            })
            .collect::<Vec<_>>();
        build_bars(
            &items,
            &BarsParams {
                bar_spacing: self.time_scale.bar_spacing(),
                horizontal_pixel_ratio: hpr,
                vertical_pixel_ratio: vpr,
                open_visible: rs.open_visible,
                thin_bars: rs.thin_bars,
            },
            out,
        );
    }

    #[allow(clippy::too_many_arguments)] // mirrors the reference renderer-data signature
    pub(super) fn build_histogram_frame(
        &self,
        rs: ResolvedSeries,
        from: i64,
        to: i64,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
    ) {
        let plot = self.data.plot(rs.id);
        let c = plot.column(PlotValueIndex::Close);
        // reference HistogramStyleOptions.base (histogram-renderer.ts): columns grow from this price
        // level (default 0).
        let base = scale.price_to_coordinate(rs.base, rs.base_value);
        let solid = if rs.color != LINE {
            rs.color
        } else {
            HISTOGRAM
        };
        // TradingView volume tint: the primary series' up/down direction per bar. The primary
        // is the first visible, non-removed series (id 0 may be tombstoned).
        let main = self.primary_series().map(|s| self.data.plot(s.id));
        let point_colors = self.data.point_colors(rs.id);
        let histogram_updown = self
            .series_entry(rs.id)
            .is_some_and(|series| series.histogram_updown);
        let mut work = conflation::DensityWork::default();
        let visible = visible_histogram_rows_with_work(
            plot,
            from,
            to,
            self.time_scale.bar_spacing(),
            hpr,
            |index| self.time_scale.index_to_coordinate(index) * hpr,
            &mut work,
        );
        self.record_lod_work(
            work.selected_level,
            work.summary_nodes,
            work.raw_rows,
            work.candidates,
        );
        let items = visible
            .into_iter()
            .map(|item| {
                let r = item.source_row;
                // reference data-item color (series-bar-colorer.ts Histogram arm): a per-point
                // `color` wins over both the series `color` and the up/down volume tint.
                let color = match point_colors
                    .and_then(|colors| colors.color(PointColorChannel::Body, r))
                {
                    Some(c) => Color(c),
                    None => {
                        if histogram_updown {
                            // A whitespace row (or no row) on the primary series carries no
                            // direction — the column falls back to its solid color.
                            let direction = main.and_then(|m| {
                                let row = m.search(
                                    plot.index_at(r).expect("histogram row index"),
                                    MismatchDirection::None,
                                )?;
                                if m.is_whitespace_row(row) {
                                    return None;
                                }
                                let close = m.value_at(row, PlotValueIndex::Close);
                                let open = m.value_at(row, PlotValueIndex::Open);
                                (close.is_finite() && open.is_finite()).then_some(close >= open)
                            });
                            match direction {
                                Some(true) => VOLUME_UP,
                                Some(false) => VOLUME_DOWN,
                                None => solid,
                            }
                        } else {
                            solid
                        }
                    }
                };
                HistogramItem {
                    x: item.x_px / hpr,
                    y: scale.price_to_coordinate(c[r], rs.base_value),
                    time: item.geometry_time,
                    color,
                }
            })
            .collect::<Vec<_>>();
        build_histogram(
            &items,
            &HistogramParams {
                bar_spacing: self.time_scale.bar_spacing(),
                horizontal_pixel_ratio: hpr,
                vertical_pixel_ratio: vpr,
                histogram_base: base,
            },
            out,
        );
    }

    #[allow(clippy::too_many_arguments)] // mirrors the reference renderer-data signature
    pub(super) fn build_line_frame(
        &self,
        rs: ResolvedSeries,
        from: i64,
        to: i64,
        hpr: f64,
        vpr: f64,
        band_top: f64,
        band_bottom: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
    ) {
        let plot = self.data.plot(rs.id);
        let c = plot.column(PlotValueIndex::Close);
        let mut work = conflation::DensityWork::default();
        let rows = visible_line_rows_with_work(
            plot,
            from,
            to,
            self.time_scale.bar_spacing(),
            hpr,
            |index| self.time_scale.index_to_coordinate(index) * hpr,
            &mut work,
        );
        self.record_lod_work(
            work.selected_level,
            work.summary_nodes,
            work.raw_rows,
            work.candidates,
        );
        let mut row_points: Vec<[f32; 2]> = Vec::with_capacity(rows.len());
        for &r in &rows {
            row_points.push([
                (self
                    .time_scale
                    .index_to_coordinate(plot.index_at(r).expect("line row index"))
                    * hpr) as f32,
                (scale.price_to_coordinate(c[r], rs.base_value) * vpr) as f32,
            ]);
        }
        if row_points.is_empty() {
            return;
        }
        let first = points.len() as u32;
        points.extend_from_slice(&row_points);
        let count = row_points.len() as u32;
        let color = if rs.color != LINE {
            rs.color
        } else if rs.kind == SeriesKind::Area {
            AREA_LINE
        } else {
            LINE
        };
        let point_colors = self.data.point_colors(rs.id);
        // Bollinger background fill: the band between this UPPER output and its LOWER
        // companion, in the band color at TradingView's 0.2 background alpha, painted under
        // the band strokes. Both outputs share bar times, so the rows (and x's) align
        // point-for-point; a count mismatch skips the fill rather than drawing a wrong one.
        if let Some(lower_id) = self.bollinger_fill_companion(rs.id) {
            let lower_plot = self.data.plot(lower_id);
            let lower_close = lower_plot.column(PlotValueIndex::Close);
            let mut work = conflation::DensityWork::default();
            let lower_rows = visible_line_rows_with_work(
                lower_plot,
                from,
                to,
                self.time_scale.bar_spacing(),
                hpr,
                |index| self.time_scale.index_to_coordinate(index) * hpr,
                &mut work,
            );
            self.record_lod_work(
                work.selected_level,
                work.summary_nodes,
                work.raw_rows,
                work.candidates,
            );
            if lower_rows.len() == rows.len() && rows.len() >= 2 {
                let upper_first = points.len() as u32;
                points.extend_from_slice(&row_points);
                let lower_first = points.len() as u32;
                points.extend(lower_rows.iter().map(|&r| {
                    [
                        (self.time_scale.index_to_coordinate(
                            lower_plot.index_at(r).expect("lower-band row index"),
                        ) * hpr) as f32,
                        (scale.price_to_coordinate(lower_close[r], rs.base_value) * vpr) as f32,
                    ]
                }));
                out.push(Prim::BandFill {
                    upper_first,
                    lower_first,
                    point_count: rows.len() as u32,
                    fill: Color::rgba(
                        color.r(),
                        color.g(),
                        color.b(),
                        (color.a() as f64 * 0.2).round() as u8,
                    ),
                });
            }
        }
        // reference data-item colors (series-bar-colorer.ts Line/Area arms — area reads `lineColor`,
        // mapped onto the body channel): a per-point color governs the stroke segment leaving
        // its point and the point marker. Resolved per visible point, falling back to the
        // series stroke color. `None` when no visible point overrides — the single-prim path
        // below then stays byte-identical to a series without data-item colors.
        let resolved: Option<Vec<Color>> = rows
            .iter()
            .any(|&r| {
                point_colors
                    .and_then(|colors| colors.color(PointColorChannel::Body, r))
                    .is_some()
            })
            .then(|| {
                rows.iter()
                    .map(|&r| {
                        point_colors
                            .and_then(|colors| colors.color(PointColorChannel::Body, r))
                            .map(Color)
                            .unwrap_or(color)
                    })
                    .collect::<Vec<_>>()
            });
        if rs.kind == SeriesKind::Area {
            // reference `invertFilledArea` (area-renderer-base.ts): fill from the pane's top edge
            // down to the line instead of from the line down to the pane's bottom edge.
            let base_y = if rs.invert_filled_area {
                band_top
            } else {
                band_bottom
            };
            // Deviation: the area fill keeps the series-level gradient even with per-point
            // colors — the reference's `color`/`lineColor` data-item field affects only the stroke
            // (per-point `topColor`/`bottomColor` fill overrides are not modeled).
            out.push(Prim::AreaFill {
                first_point: first,
                point_count: count,
                base_y: (base_y * vpr) as f32,
                line_type: self
                    .series_entry(rs.id)
                    .map_or(LineType::Simple, |series| series.line_type),
                gradient: Gradient {
                    top: rs.area_top,
                    bottom: rs.area_bottom,
                },
            });
        }
        // reference `lineVisible` (line-renderer-base.ts): the stroke is skipped; an area keeps its
        // fill and a line series keeps only its point markers.
        if rs.line_visible {
            let width = (rs.line_width * vpr) as f32;
            match &resolved {
                Some(colors) => {
                    // Per-point colors: one stroke run per maximal equal-color span (the
                    // walkLine split). With steps/curves each run expands independently, and a
                    // dashed style restarts its pattern per run — reference keeps dash offset and
                    // splits the step corner at the color change; those sub-segment details are
                    // not modeled (documented deviation; Simple lines are exact).
                    for (start, end, run_color) in color_runs(colors) {
                        if rs.line_style == LineStyle::Solid {
                            let run_first = points.len() as u32;
                            points.extend_from_slice(&row_points[start..end]);
                            out.push(Prim::Polyline {
                                first_point: run_first,
                                point_count: (end - start) as u32,
                                width,
                                style: LineStyle::Solid,
                                line_type: rs.line_type,
                                color: run_color,
                            });
                        } else {
                            push_line_stroke(
                                out,
                                points,
                                &row_points[start..end],
                                width,
                                rs.line_style,
                                rs.line_type,
                                run_color,
                            );
                        }
                    }
                }
                None => {
                    if rs.line_style == LineStyle::Solid {
                        out.push(Prim::Polyline {
                            first_point: first,
                            point_count: count,
                            width,
                            style: LineStyle::Solid,
                            line_type: rs.line_type,
                            color,
                        });
                    } else {
                        push_line_stroke(
                            out,
                            points,
                            &row_points,
                            width,
                            rs.line_style,
                            rs.line_type,
                            color,
                        );
                    }
                }
            }
        }
        if rs.point_markers {
            // reference `pointMarkersRadius` default (line-pane-view.ts): `lineWidth / 2 + 2`. reference
            // draws the markers unconditionally once enabled (draw-series-point-markers.ts),
            // each in its point's own resolved color.
            let radius = rs.point_markers_radius.unwrap_or(rs.line_width / 2.0 + 2.0);
            for (i, p) in row_points.iter().enumerate() {
                let marker_color = resolved.as_ref().map_or(color, |colors| colors[i]);
                out.push(Prim::Circle {
                    cx: p[0],
                    cy: p[1],
                    radius: (radius * vpr) as f32,
                    fill: marker_color,
                    stroke_width: 0.0,
                    stroke: marker_color,
                });
            }
        }
    }

    #[allow(clippy::too_many_arguments)] // mirrors the reference renderer-data signature
    pub(super) fn build_baseline_frame(
        &self,
        rs: ResolvedSeries,
        from: i64,
        to: i64,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
        scale: &nucleuscharts_core::scale::price_scale_core::PriceScaleCore,
    ) {
        let plot = self.data.plot(rs.id);
        let close = plot.column(PlotValueIndex::Close);
        let rows = visible_line_rows(
            plot,
            from,
            to,
            self.time_scale.bar_spacing(),
            hpr,
            |index| self.time_scale.index_to_coordinate(index) * hpr,
        );
        if rows.len() < 2 {
            return;
        }
        let Some(baseline_price) = self.resolved_baseline_price(rs.id, from, to) else {
            return;
        };
        let baseline_y = scale.price_to_coordinate(baseline_price, rs.base_value);
        let mut top_runs: Vec<Vec<[f32; 2]>> = Vec::new();
        let mut bottom_runs: Vec<Vec<[f32; 2]>> = Vec::new();
        for pair in rows.windows(2) {
            let a_row = pair[0];
            let b_row = pair[1];
            let a = (
                self.time_scale
                    .index_to_coordinate(plot.index_at(a_row).expect("baseline row index")),
                scale.price_to_coordinate(close[a_row], rs.base_value),
            );
            let b = (
                self.time_scale
                    .index_to_coordinate(plot.index_at(b_row).expect("baseline row index")),
                scale.price_to_coordinate(close[b_row], rs.base_value),
            );
            let mut segments = vec![(a, b)];
            if (a.1 < baseline_y) != (b.1 < baseline_y) && (b.1 - a.1).abs() > 1e-9 {
                let t = (baseline_y - a.1) / (b.1 - a.1);
                let crossing = (a.0 + (b.0 - a.0) * t, baseline_y);
                segments = vec![(a, crossing), (crossing, b)];
            }
            for (s0, s1) in segments {
                let above = (s0.1 + s1.1) * 0.5 < baseline_y;
                let p0 = [(s0.0 * hpr) as f32, (s0.1 * vpr) as f32];
                let p1 = [(s1.0 * hpr) as f32, (s1.1 * vpr) as f32];
                let runs = if above {
                    &mut top_runs
                } else {
                    &mut bottom_runs
                };
                if let Some(run) = runs.last_mut().filter(|run| run.last() == Some(&p0)) {
                    run.push(p1);
                } else {
                    runs.push(vec![p0, p1]);
                }
            }
        }

        // One area primitive per uninterrupted quadrant keeps the gradient continuous instead of
        // restarting it at every source segment and producing visible rectangular pockets.
        for (runs, gradient) in [
            (
                &top_runs,
                Gradient {
                    top: rs.top_fill1,
                    bottom: rs.top_fill2,
                },
            ),
            (
                &bottom_runs,
                Gradient {
                    top: rs.bottom_fill1,
                    bottom: rs.bottom_fill2,
                },
            ),
        ] {
            for run in runs {
                let first = points.len() as u32;
                points.extend_from_slice(run);
                out.push(Prim::AreaFill {
                    first_point: first,
                    point_count: run.len() as u32,
                    base_y: (baseline_y * vpr) as f32,
                    line_type: LineType::Simple,
                    gradient,
                });
            }
        }
        if rs.line_visible {
            for run in &top_runs {
                push_line_stroke(
                    out,
                    points,
                    run,
                    (rs.top_line_width * vpr) as f32,
                    rs.top_line_style,
                    LineType::Simple,
                    rs.top_line,
                );
            }
            for run in &bottom_runs {
                push_line_stroke(
                    out,
                    points,
                    run,
                    (rs.bottom_line_width * vpr) as f32,
                    rs.bottom_line_style,
                    LineType::Simple,
                    rs.bottom_line,
                );
            }
        }
    }

    pub(super) fn build_price_lines_frame(
        &self,
        pane_index: usize,
        out: &mut Vec<Prim>,
        width: i32,
        vpr: f64,
    ) {
        let pane = &self.panes[pane_index];
        let min_width = 1f64.max(vpr.floor()) as i32;
        for series in &self.series {
            if series.pane_index != pane_index {
                continue;
            }
            let scale = pane_scale(pane, series_scale_target(series));
            if scale.is_empty() {
                continue;
            }
            let Some(base_value) = self.visible_series_base_value(series.id) else {
                continue;
            };
            for line in &series.price_lines {
                // reference `lineVisible`: the axis label survives a hidden line.
                if !line.line_visible {
                    continue;
                }
                out.push(Prim::HLine {
                    y: (scale.price_to_coordinate(line.price, base_value) * vpr).round() as i32,
                    x0: 0,
                    x1: width,
                    width: line.width.max(min_width),
                    style: line.style,
                    color: line.color,
                });
            }
        }
    }

    pub(super) fn build_series_markers_frame(
        &self,
        series_id: SeriesId,
        from: i64,
        to: i64,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let Some(series) = self
            .series
            .iter()
            .find(|series| series.id == series_id && series.visible && !series.removed)
        else {
            return;
        };
        let Some(pane) = self.panes.get(series.pane_index) else {
            return;
        };
        let scale = pane_scale(pane, series_scale_target(series));
        if scale.is_empty() {
            return;
        }
        let Some(base_value) = self.series_base_value(series.id, from) else {
            return;
        };
        let plot = self.data.plot(series.id);
        let Some(first_data_index) = plot.first_index() else {
            return;
        };
        let spacing = self.time_scale.bar_spacing();
        let envelope = marker_envelope_size(spacing);
        let shape_margin = marker_margin(spacing);
        let font_size = self.options.get().layout.font_size;
        let font_family = self.options.get().layout.font_family.clone();
        let correction = (hpr.floor() as i64).rem_euclid(2) as f64 * 0.5;
        let mut previous_index = None;
        let mut above_offset = shape_margin;
        let mut below_offset = shape_margin;

        for marker in &series.markers {
            let Some(candidate) = self.time_to_index(marker.time as f64, true) else {
                continue;
            };
            let direction = if candidate < first_data_index {
                MismatchDirection::NearestRight
            } else {
                MismatchDirection::NearestLeft
            };
            let Some(row) = plot.search(candidate, direction) else {
                continue;
            };
            let Some(index) = plot.index_at(row) else {
                continue;
            };
            if index < from || index > to || plot.is_whitespace_row(row) {
                continue;
            }
            if previous_index != Some(index) {
                above_offset = shape_margin;
                below_offset = shape_margin;
                previous_index = Some(index);
            }

            let high = plot.value_at(row, PlotValueIndex::High);
            let low = plot.value_at(row, PlotValueIndex::Low);
            let close = plot.value_at(row, PlotValueIndex::Close);
            let exact_price = matches!(
                marker.position,
                crate::marker_pos::AT_PRICE_TOP
                    | crate::marker_pos::AT_PRICE_BOTTOM
                    | crate::marker_pos::AT_PRICE_MIDDLE
            );
            let price = if exact_price {
                let Some(price) = marker.price else {
                    continue;
                };
                price
            } else {
                match marker.position {
                    crate::marker_pos::ABOVE => {
                        if scale.is_inverted() {
                            low
                        } else {
                            high
                        }
                    }
                    crate::marker_pos::BELOW => {
                        if scale.is_inverted() {
                            high
                        } else {
                            low
                        }
                    }
                    _ => close,
                }
            };
            let size = envelope * marker.size.max(0.0);
            let half_size = size * 0.5;
            let price_y = scale.price_to_coordinate(price, base_value);
            let (y, text_y) = match marker.position {
                crate::marker_pos::ABOVE | crate::marker_pos::AT_PRICE_TOP => {
                    let offset = if exact_price { 0.0 } else { above_offset };
                    let y = price_y - half_size - offset;
                    let text_y = y - half_size - font_size * 0.6;
                    if !marker.text.is_empty() {
                        above_offset += font_size * 1.2;
                    }
                    if !exact_price {
                        above_offset += size + shape_margin;
                    }
                    (y, text_y)
                }
                crate::marker_pos::BELOW | crate::marker_pos::AT_PRICE_BOTTOM => {
                    let offset = if exact_price { 0.0 } else { below_offset };
                    let y = price_y + half_size + offset;
                    let text_y = y + half_size + shape_margin + font_size * 0.6;
                    if !marker.text.is_empty() {
                        below_offset += font_size * 1.2;
                    }
                    if !exact_price {
                        below_offset += size + shape_margin;
                    }
                    (y, text_y)
                }
                _ => {
                    let y = price_y;
                    (y, y + half_size + shape_margin + font_size * 0.6)
                }
            };
            let x = (self.time_scale.index_to_coordinate(index) * hpr).round() + correction;
            let x = x as f32;
            let y = (y * vpr) as f32;
            match marker.shape {
                crate::marker_shape::SQUARE => {
                    let shape_size = marker_shape_size(size, 0.7);
                    let half = ((shape_size - 1.0) * hpr * 0.5) as f32;
                    out.push(Prim::RoundRect {
                        x: x - half,
                        y: y - half,
                        w: (shape_size * hpr) as f32,
                        h: (shape_size * hpr) as f32,
                        radii: [0.0; 4],
                        fill: marker.color,
                        border_width: 0.0,
                        border_color: marker.color,
                    });
                }
                crate::marker_shape::ARROW_UP | crate::marker_shape::ARROW_DOWN => {
                    let arrow_size = marker_shape_size(size, 1.0);
                    let half_arrow = (((arrow_size - 1.0) * 0.5) * hpr) as f32;
                    let base_size = ceiled_odd(size / 2.0);
                    let half_base = (((base_size - 1.0) * 0.5) * hpr) as f32;
                    let up = marker.shape == crate::marker_shape::ARROW_UP;
                    out.push(Prim::Triangle {
                        a: [x, y + if up { -half_arrow } else { half_arrow }],
                        b: [x - half_arrow, y],
                        c: [x + half_arrow, y],
                        color: marker.color,
                    });
                    out.push(Prim::RoundRect {
                        x: x - half_base,
                        y: if up { y } else { y - half_arrow },
                        w: half_base * 2.0,
                        h: half_arrow,
                        radii: [0.0; 4],
                        fill: marker.color,
                        border_width: 0.0,
                        border_color: marker.color,
                    });
                }
                _ => {
                    let radius = (((marker_shape_size(size, 0.8) - 1.0) * 0.5) * hpr) as f32;
                    out.push(Prim::Circle {
                        cx: x,
                        cy: y,
                        radius,
                        fill: marker.color,
                        stroke_width: 0.0,
                        stroke: marker.color,
                    });
                }
            }
            if !marker.text.is_empty() {
                out.push(Prim::Text {
                    x,
                    y: (text_y * vpr) as f32,
                    text: marker.text.clone(),
                    color: marker.color,
                    size: (font_size * vpr) as f32,
                    family: font_family.clone(),
                    align: TextAlign::Center,
                    weight: 400,
                    italic: false,
                });
            }
        }
    }

    #[allow(clippy::too_many_arguments)] // per-pane signature shared with the other builders
    pub(super) fn build_last_value_line_frame(
        &self,
        pane_index: usize,
        from: i64,
        to: i64,
        out: &mut Vec<Prim>,
        width: i32,
        hpr: f64,
        vpr: f64,
    ) {
        let pane = &self.panes[pane_index];
        for series in &self.series {
            // reference SeriesPriceLinePaneView: one built-in last-price line per visible series
            // with `priceLineVisible` (default true), drawn on the series' own price scale.
            if !series.visible || !series.price_line_visible {
                continue;
            }
            if series.pane_index != pane_index {
                continue;
            }
            let scale = pane_scale(pane, series_scale_target(series));
            // A custom series' last value comes from the host-recorded frame values (Phase
            // C-c): the plugin's current value (the LAST `priceValueBuilder` element, the
            // Close slot of the reference's custom plot-row mapping) of the last non-whitespace item —
            // global, or visible per `priceLineSource`.
            if series.kind == SeriesKind::Custom {
                if scale.is_empty() {
                    continue;
                }
                let last = if series.price_line_source == 1 {
                    series.custom_frame.last_visible
                } else {
                    series.custom_frame.last
                };
                let Some(last) = last else {
                    continue;
                };
                let Some(base_value) = self.series_base_value(series.id, from) else {
                    continue;
                };
                let color = self.effective_series_live_color(series, last.color);
                out.push(Prim::HLine {
                    y: (scale.price_to_coordinate(last.value, base_value) * vpr).round() as i32,
                    x0: 0,
                    x1: width,
                    width: 1f64.max((series.price_line_width * hpr).floor()) as i32,
                    style: crate::line_style_from_u8(series.price_line_style),
                    color,
                });
                continue;
            }
            let plot = self.data.plot(series.id);
            if plot.is_empty() || scale.is_empty() {
                continue;
            }
            // reference PriceLineSource (series.ts lastValueData): LastBar follows the series'
            // final bar, LastVisible the last bar at or left of the visible right edge.
            // Whitespace rows are skipped (the reference's plot list never contains them).
            let row = if series.price_line_source == 1 {
                plot.last_non_whitespace_row(to)
            } else {
                plot.last_non_whitespace_row(TimePointIndex::MAX)
            };
            let Some(row) = row else {
                continue;
            };
            let close = plot.value_at(row, PlotValueIndex::Close);
            if !close.is_finite() {
                continue;
            }
            let Some(base_value) = self.series_base_value(series.id, from) else {
                continue;
            };
            let baseline = if series.kind == SeriesKind::Baseline {
                self.resolved_baseline_price(series.id, from, to)
            } else {
                None
            };
            // reference `priceLineColor` default '' (series.ts priceLineColor): follow the bar color.
            // The pinned CSS string parses here; an unparseable string falls back to ''.
            let color = self
                .effective_series_live_color(series, self.series_bar_color(series, row, baseline));
            out.push(Prim::HLine {
                y: (scale.price_to_coordinate(close, base_value) * vpr).round() as i32,
                x0: 0,
                x1: width,
                // reference horizontal-line-renderer.ts:65 scales lineWidth by the HORIZONTAL ratio
                // (kept verbatim, including the ratio choice).
                width: 1f64.max((series.price_line_width * hpr).floor()) as i32,
                style: crate::line_style_from_u8(series.price_line_style),
                color,
            });
        }
    }

    /// TradingView-style bid/ask lines (default OFF, `bid_ask_visible`): one horizontal line
    /// per side with a live value, on the series' own price scale, colored by the side's
    /// pinned CSS color (semantic primary / market-loss defaults).
    pub(super) fn build_bid_ask_lines_frame(
        &self,
        pane_index: usize,
        from: i64,
        out: &mut Vec<Prim>,
        width: i32,
        hpr: f64,
        vpr: f64,
    ) {
        let pane = &self.panes[pane_index];
        for series in &self.series {
            if !series.visible || !series.bid_ask_visible || series.pane_index != pane_index {
                continue;
            }
            let scale = pane_scale(pane, series_scale_target(series));
            if scale.is_empty() {
                continue;
            }
            let Some(base_value) = self.series_base_value(series.id, from) else {
                continue;
            };
            let sides = [
                (series.bid, series.bid_color.as_str(), PRIMARY),
                (series.ask, series.ask_color.as_str(), DOWN),
            ];
            for (value, css, fallback) in sides {
                let Some(price) = value else {
                    continue;
                };
                let color = Color::parse_css(css).unwrap_or(fallback);
                out.push(Prim::HLine {
                    y: (scale.price_to_coordinate(price, base_value) * vpr).round() as i32,
                    x0: 0,
                    x1: width,
                    width: 1f64.max((series.bid_ask_line_width * hpr).floor()) as i32,
                    style: crate::line_style_from_u8(series.bid_ask_line_style),
                    color,
                });
            }
        }
    }

    pub(super) fn build_last_pulse_frame(&self, out: &mut Vec<Prim>, hpr: f64, vpr: f64) {
        // The pulse anchors on the primary series (the reference's single last-price animation source);
        // with id 0 tombstoned it falls back to the first visible, non-removed series.
        let Some(series) = self.primary_series() else {
            return;
        };
        if !series.last_price_animation {
            return;
        }
        let series_id = series.id;
        let series_kind = series.kind;
        let scale = pane_scale(&self.panes[0], series_scale_target(series));
        let plot = self.data.plot(series_id);
        if plot.is_empty() || scale.is_empty() {
            return;
        }
        // Last non-whitespace bar (the reference's whitespace-filtered last row).
        let Some(last) = plot.last_non_whitespace_row(TimePointIndex::MAX) else {
            return;
        };
        let index = plot.index_at(last).expect("last series row index");
        let close = plot.value_at(last, PlotValueIndex::Close);
        let Some(base_value) = self.visible_series_base_value(series_id) else {
            return;
        };
        let cx = (self.time_scale.index_to_coordinate(index) * hpr) as f32;
        let cy = (scale.price_to_coordinate(close, base_value) * vpr) as f32;
        let base = match series_kind {
            SeriesKind::Line => LINE,
            SeriesKind::Area => AREA_LINE,
            SeriesKind::Histogram => HISTOGRAM,
            _ => {
                let open = plot.value_at(last, PlotValueIndex::Open);
                if close >= open {
                    UP
                } else {
                    DOWN
                }
            }
        };
        const PERIOD_MS: f64 = 2600.0;
        let phase = (self.animation_time.rem_euclid(PERIOD_MS) / PERIOD_MS) as f32;
        let ring = Color::rgba(
            base.r(),
            base.g(),
            base.b(),
            ((1.0 - phase) * 0.35 * 255.0) as u8,
        );
        out.push(Prim::Circle {
            cx,
            cy,
            radius: (4.0 + phase * 10.0) * vpr as f32,
            fill: ring,
            stroke_width: 0.0,
            stroke: ring,
        });
        out.push(Prim::Circle {
            cx,
            cy,
            radius: 4.0 * vpr as f32,
            fill: base,
            stroke_width: 0.0,
            stroke: base,
        });
    }

    /// TradingView-style SELECTION ANCHORS: canonical timestamps sampled when the series was
    /// selected are resolved against current values and coordinates, then clipped to the pane.
    /// Each carries a theme-derived fill with the TradingView accent-blue border.
    pub(super) fn build_selection_anchors_frame(
        &self,
        pane_index: usize,
        from: i64,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        const ANCHOR_RADIUS: f64 = 3.5;
        const ANCHOR_BORDER_WIDTH: f64 = 1.5;
        const ANCHOR_BORDER: Color = PRIMARY;
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let selected = selection.series;
        let pane = &self.panes[pane_index];
        for series in &self.series {
            if series.id != selected || !series.visible || series.pane_index != pane_index {
                continue;
            }
            let scale = pane_scale(pane, series_scale_target(series));
            if scale.is_empty() {
                continue;
            }
            let Some(base_value) = self.series_base_value(series.id, from) else {
                continue;
            };
            let plot = self.data.plot(series.id);
            let fallback = nucleuscharts_core::style::DEFAULT_SURFACE_RGB;
            let background = css_color(
                &self.options.get().layout.background.color,
                Color::rgb(fallback.0, fallback.1, fallback.2),
            );
            let fill = if background.luminance() > 160.0 {
                Color::rgb(0xff, 0xff, 0xff)
            } else {
                Color::rgb(0, 0, 0)
            };
            let Some((times, _)) = self.data.series_data(selected) else {
                continue;
            };
            for time in &selection.times {
                let Ok(row) = times.binary_search(time) else {
                    continue;
                };
                if plot.is_whitespace_row(row) {
                    continue;
                }
                let Some(index) = plot.index_at(row) else {
                    continue;
                };
                let cx = (self.time_scale.index_to_coordinate(index) * hpr) as f32;
                let value = if series.kind == SeriesKind::Candlestick {
                    (plot.value_at(row, PlotValueIndex::Open)
                        + plot.value_at(row, PlotValueIndex::Close))
                        / 2.0
                } else {
                    plot.value_at(row, PlotValueIndex::Close)
                };
                let cy = (scale.price_to_coordinate(value, base_value) * vpr) as f32;
                if !cx.is_finite()
                    || !cy.is_finite()
                    || cx < 0.0
                    || cx > (self.pane_w * hpr) as f32
                    || cy < (pane.top * vpr) as f32
                    || cy > ((pane.top + pane.height) * vpr) as f32
                {
                    continue;
                }
                // The crosshair-marks disc idiom: the border is a larger filled disc underneath.
                out.push(Prim::Circle {
                    cx,
                    cy,
                    radius: ((ANCHOR_RADIUS + ANCHOR_BORDER_WIDTH) * vpr) as f32,
                    fill: ANCHOR_BORDER,
                    stroke_width: 0.0,
                    stroke: ANCHOR_BORDER,
                });
                out.push(Prim::Circle {
                    cx,
                    cy,
                    radius: (ANCHOR_RADIUS * vpr) as f32,
                    fill,
                    stroke_width: 0.0,
                    stroke: fill,
                });
            }
        }
    }
}
