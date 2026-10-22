use aeris_charts_core::scale::general_scale::{BandScale, LinearScale, PointScale};
use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{Gradient, IRect, LineStyle, LineType, Prim, TextAlign};

use crate::general_axes::NumericAxisScale;
use crate::general_series::{GeneralColumnGeometry, general_bar_radii, general_polar_fill_color};
use crate::{
    AxisDimension, ChartEngine, DEFAULT_LINE_COLOR, GeneralAxisDomain, GeneralPointSymbol,
    GeneralReferenceOptions, GeneralReferenceValue, GeneralScaleType, GeneralSeries,
    GeneralSeriesKind,
};

use super::PRIMARY;

const GENERAL_HOVER: Color = Color(PRIMARY.0 & 0xFFFF_FF00 | 0x73);
const GENERAL_BRUSH: Color = Color(PRIMARY.0 & 0xFFFF_FF00 | 0x28);
const GENERAL_BRUSH_EDGE: Color = Color(PRIMARY.0 & 0xFFFF_FF00 | 0x8F);
const GENERAL_REFERENCE_REGION: Color = Color(PRIMARY.0 & 0xFFFF_FF00 | 0x20);
const GENERAL_REFERENCE_MARK: Color = Color(PRIMARY.0 & 0xFFFF_FF00 | 0xCC);
const MAX_GENERAL_DATA_LABELS_PER_PANE: usize = 512;
const MAX_GENERAL_DATA_LABEL_ATTEMPTS_PER_PANE: usize = 4_096;
fn opacity_alpha(opacity: f64) -> u8 {
    (opacity.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Label rows for a vertical bar: just past its value end, then just inside it when the bar is
/// tall enough. A stacked segment that another segment continues has no free end, so it labels
/// its middle instead of covering the next segment.
fn column_label_rows(geometry: GeneralColumnGeometry, label_size: f64) -> (f64, f64) {
    let half = label_size * 0.65 + 2.0;
    if !geometry.exposed_end {
        let center = (geometry.top + geometry.bottom) * 0.5;
        return (center, center);
    }
    let (end, toward_base) = if geometry.end_at_min {
        (geometry.top, 1.0)
    } else {
        (geometry.bottom, -1.0)
    };
    let outside = end - toward_base * half;
    let inside = if geometry.bottom - geometry.top >= 2.0 * half {
        end + toward_base * half
    } else {
        outside
    };
    (outside, inside)
}

fn general_fill_gradient(series: &GeneralSeries) -> Option<Gradient> {
    series.fill_gradient().map(|[top, bottom]| {
        let opacity = if matches!(
            series.kind(),
            GeneralSeriesKind::XyArea | GeneralSeriesKind::RangeArea
        ) {
            series.fill_opacity()
        } else {
            1.0
        };
        let stop = |css: &str| {
            let color = Color::parse_css(css).expect("validated area gradient color");
            Color::rgba(
                color.r(),
                color.g(),
                color.b(),
                opacity_alpha(opacity * f64::from(color.a()) / 255.0),
            )
        };
        Gradient {
            top: stop(top),
            bottom: stop(bottom),
        }
    })
}

fn interpolate_color(low: Color, high: Color, intensity: f64) -> Color {
    let t = intensity.clamp(0.0, 1.0);
    let channel = |from: u8, to: u8| (from as f64 + (to as f64 - from as f64) * t).round() as u8;
    Color::rgba(
        channel(low.r(), high.r()),
        channel(low.g(), high.g()),
        channel(low.b(), high.b()),
        channel(low.a(), high.a()),
    )
}

fn push_general_bar_fill(
    out: &mut Vec<Prim>,
    rect: IRect,
    radii: Option<[f32; 4]>,
    series_color: Color,
    row_color: Option<Color>,
    gradient: Option<Gradient>,
) {
    let gradient = gradient.filter(|_| row_color.is_none());
    match (radii, gradient) {
        (Some(radii), Some(gradient)) => out.push(Prim::GradientRoundRect {
            x: rect.x as f32,
            y: rect.y as f32,
            w: rect.w as f32,
            h: rect.h as f32,
            radii,
            gradient,
        }),
        (Some(radii), None) => {
            let color = row_color.unwrap_or(series_color);
            out.push(Prim::RoundRect {
                x: rect.x as f32,
                y: rect.y as f32,
                w: rect.w as f32,
                h: rect.h as f32,
                radii,
                fill: color,
                border_width: 0.0,
                border_color: color,
            });
        }
        (None, Some(gradient)) => out.push(Prim::GradientRect { rect, gradient }),
        (None, None) => out.push(Prim::Rect {
            rect,
            color: row_color.unwrap_or(series_color),
        }),
    }
}

/// The active dot of an axis-mode hover, drawn like the financial crosshair marker: a 4 px disc
/// inside a 2 px ring of the chart background. `x`/`y` are device px.
fn push_general_active_dot(
    out: &mut Vec<Prim>,
    x: f64,
    y: f64,
    color: Color,
    background: Color,
    vpr: f64,
) {
    for (radius, fill) in [(6.0, background), (4.0, color)] {
        out.push(Prim::Circle {
            cx: x as f32,
            cy: y as f32,
            radius: (radius * vpr) as f32,
            fill,
            stroke_width: 0.0,
            stroke: fill,
        });
    }
}

fn push_general_point_symbol(
    out: &mut Vec<Prim>,
    symbol: GeneralPointSymbol,
    x: f64,
    y: f64,
    radius: f64,
    color: Color,
) {
    let (x, y, radius) = (x as f32, y as f32, radius as f32);
    match symbol {
        GeneralPointSymbol::Circle => out.push(Prim::Circle {
            cx: x,
            cy: y,
            radius,
            fill: color,
            stroke_width: 0.0,
            stroke: color,
        }),
        GeneralPointSymbol::Square => out.push(Prim::RoundRect {
            x: x - radius,
            y: y - radius,
            w: radius * 2.0,
            h: radius * 2.0,
            radii: [0.0; 4],
            fill: color,
            border_width: 0.0,
            border_color: color,
        }),
        GeneralPointSymbol::Diamond => {
            let top = [x, y - radius];
            let right = [x + radius, y];
            let bottom = [x, y + radius];
            let left = [x - radius, y];
            out.push(Prim::Triangle {
                a: top,
                b: right,
                c: bottom,
                color,
            });
            out.push(Prim::Triangle {
                a: top,
                b: bottom,
                c: left,
                color,
            });
        }
        GeneralPointSymbol::Triangle => out.push(Prim::Triangle {
            a: [x, y - radius],
            b: [x + radius, y + radius],
            c: [x - radius, y + radius],
            color,
        }),
    }
}

impl ChartEngine {
    pub(super) fn build_general_series_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) -> Vec<Prim> {
        let mut interaction = [None, None];
        let Some(pane_id) = self.pane_stable_id(pane_index) else {
            return Vec::new();
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return Vec::new();
        };
        let dot_rows = self.general_axis_hover_dot_rows(pane_index);
        let mut active_dots = Vec::new();
        let dot_background = super::css_color(
            &self.options.get().layout.background.color,
            Color::rgb(0xff, 0xff, 0xff),
        );
        let layout = &self.options.get().layout;
        let label_size = layout.font_size.max(1.0);
        let label_color = self.primary_text_color();
        let mut occupied_labels: Vec<[f64; 4]> = Vec::new();
        let mut leader_primitives = Vec::new();
        let mut label_primitives = Vec::new();
        let mut label_attempts = 0;
        self.push_general_reference_regions(pane_id, plot, hpr, vpr, out);
        if let Some((dimension, from, to)) = self.general_brush_axis_bounds(pane_index) {
            match dimension {
                AxisDimension::X => {
                    let left = (from * hpr).round() as i32;
                    let right = (to * hpr).round() as i32;
                    let top = (plot.y * vpr).round() as i32;
                    let bottom = ((plot.y + plot.height) * vpr).round() as i32;
                    out.push(Prim::Rect {
                        rect: IRect {
                            x: left,
                            y: top,
                            w: (right - left).max(1),
                            h: (bottom - top).max(1),
                        },
                        color: GENERAL_BRUSH,
                    });
                    for x in [from, to] {
                        let x = (x * hpr).round() as i32;
                        out.push(Prim::VLine {
                            x,
                            y0: top,
                            y1: bottom,
                            width: hpr.min(vpr).round().max(1.0) as i32,
                            style: LineStyle::Solid,
                            color: GENERAL_BRUSH_EDGE,
                        });
                    }
                }
                AxisDimension::Y => {
                    let top = (from * vpr).round() as i32;
                    let bottom = (to * vpr).round() as i32;
                    let right = (plot.width * hpr).round() as i32;
                    out.push(Prim::Rect {
                        rect: IRect {
                            x: 0,
                            y: top,
                            w: right.max(1),
                            h: (bottom - top).max(1),
                        },
                        color: GENERAL_BRUSH,
                    });
                    for y in [from, to] {
                        let y = (y * vpr).round() as i32;
                        out.push(Prim::HLine {
                            y,
                            x0: 0,
                            x1: right,
                            width: hpr.min(vpr).round().max(1.0) as i32,
                            style: LineStyle::Solid,
                            color: GENERAL_BRUSH_EDGE,
                        });
                    }
                }
                AxisDimension::Angle | AxisDimension::Radius => {}
            }
        }
        for series in self
            .general_series_iter()
            .filter(|series| series.visible() && series.pane_id() == pane_id)
        {
            let Some(dataset) = self.general_geometry_dataset(series.dataset()) else {
                continue;
            };
            // Histogram rows are engine bins, not dataset rows.
            let histogram = self.general_histogram_bins(series);
            let mut push_label = |row: usize,
                                  x: f64,
                                  above: f64,
                                  below: f64,
                                  width_limit: f64,
                                  outside_side: Option<f64>| {
                if !series.data_labels()
                    || occupied_labels.len() >= MAX_GENERAL_DATA_LABELS_PER_PANE
                    || label_attempts >= MAX_GENERAL_DATA_LABEL_ATTEMPTS_PER_PANE
                {
                    return None;
                }
                label_attempts += 1;
                let text = if let Some(bins) = histogram.as_deref() {
                    bins.height(row)?.to_string()
                } else {
                    dataset
                        .row_label(row)
                        .map(str::to_owned)
                        .or_else(|| {
                            if !matches!(
                                series.kind(),
                                GeneralSeriesKind::Pie
                                    | GeneralSeriesKind::Radar
                                    | GeneralSeriesKind::PolarArea
                                    | GeneralSeriesKind::RadialBar
                            ) {
                                return None;
                            }
                            let category =
                                usize::try_from(*dataset.category_indices()?.get(row)?).ok()?;
                            dataset.categories()?.get(category).cloned()
                        })
                        .unwrap_or_else(|| dataset.y()[row].to_string())
                };
                let width =
                    self.measure_text_run(&text, label_size, &layout.font_family, 400, false);
                if !width.is_finite() || width <= 0.0 || width > width_limit {
                    return None;
                }
                let x = x + outside_side.unwrap_or(0.0) * (width * 0.5 + 6.0);
                let left = x - width * 0.5;
                let right = x + width * 0.5;
                if left < 2.0 || right > plot.width - 2.0 {
                    return None;
                }
                for y in [above, below] {
                    let top = y - label_size * 0.55;
                    let bottom = y + label_size * 0.55;
                    if top < plot.y + 2.0 || bottom > plot.y + plot.height - 2.0 {
                        continue;
                    }
                    let rect = [left, top, right, bottom];
                    if occupied_labels.iter().any(|other| {
                        rect[0] < other[2] + 2.0
                            && rect[2] > other[0] - 2.0
                            && rect[1] < other[3] + 2.0
                            && rect[3] > other[1] - 2.0
                    }) {
                        continue;
                    }
                    occupied_labels.push(rect);
                    label_primitives.push(Prim::Text {
                        x: (x * hpr) as f32,
                        y: (y * vpr) as f32,
                        text,
                        color: label_color,
                        size: (label_size * vpr) as f32,
                        family: layout.font_family.clone(),
                        align: TextAlign::Center,
                        weight: 400,
                        italic: false,
                    });
                    return Some((x, width, y));
                }
                None
            };
            let fill_gradient = general_fill_gradient(series);
            match series.kind() {
                GeneralSeriesKind::Pie
                | GeneralSeriesKind::PolarArea
                | GeneralSeriesKind::RadialBar => {
                    let mut paint =
                        |geometry: crate::general_series::GeneralPolarSectorGeometry| {
                            let color = general_polar_fill_color(series, dataset, geometry.row);
                            let first_point = points.len() as u32;
                            geometry.sector.visit_triangles(|triangle| {
                                points.extend(
                                    triangle.map(|(x, y)| [(x * hpr) as f32, (y * vpr) as f32]),
                                );
                            });
                            let point_count = points.len() as u32 - first_point;
                            if point_count > 0 {
                                out.push(Prim::MeshFill {
                                    first_point,
                                    point_count,
                                    color,
                                });
                            }
                            if let Some((x, y)) = geometry
                                .sector
                                .plot()
                                .point(geometry.sector.mid_angle(), geometry.sector.mid_radius())
                            {
                                if series.data_labels() {
                                    let polar_plot = geometry.sector.plot();
                                    let (_, outer) = geometry.sector.radial_bounds();
                                    let angle = geometry.sector.mid_angle();
                                    let extra = 9.0 / polar_plot.outer_radius();
                                    if let (Some(anchor), Some(elbow)) = (
                                        polar_plot.point(angle, outer),
                                        polar_plot.point(angle, outer + extra),
                                    ) {
                                        let side = if elbow.0 >= polar_plot.center().0 {
                                            1.0
                                        } else {
                                            -1.0
                                        };
                                        if let Some((label_x, width, _)) = push_label(
                                            geometry.row,
                                            elbow.0,
                                            elbow.1,
                                            elbow.1,
                                            plot.width - 4.0,
                                            Some(side),
                                        ) {
                                            let tail_x = label_x - side * (width * 0.5 + 2.0);
                                            let first_point = points.len() as u32;
                                            points.extend([
                                                [(anchor.0 * hpr) as f32, (anchor.1 * vpr) as f32],
                                                [(elbow.0 * hpr) as f32, (elbow.1 * vpr) as f32],
                                                [(tail_x * hpr) as f32, (elbow.1 * vpr) as f32],
                                            ]);
                                            leader_primitives.push(Prim::Polyline {
                                                first_point,
                                                point_count: 3,
                                                width: hpr.min(vpr).max(1.0) as f32,
                                                style: LineStyle::Solid,
                                                line_type: LineType::Simple,
                                                color: Color::rgba(
                                                    label_color.r(),
                                                    label_color.g(),
                                                    label_color.b(),
                                                    (f64::from(label_color.a()) * 0.7).round()
                                                        as u8,
                                                ),
                                            });
                                        }
                                    }
                                }
                                let (hovered, selected) =
                                    self.general_row_interaction(series.id(), geometry.row);
                                if hovered || selected {
                                    interaction[usize::from(selected)] = Some(Prim::Circle {
                                        cx: (x * hpr) as f32,
                                        cy: (y * vpr) as f32,
                                        radius: (if selected { 5.0 } else { 4.0 }) * vpr as f32,
                                        fill: Color::rgba(0, 0, 0, 0),
                                        stroke_width: (if selected { 2.0 } else { 1.0 })
                                            * vpr as f32,
                                        stroke: if selected { PRIMARY } else { GENERAL_HOVER },
                                    });
                                }
                            }
                        };
                    if series.kind() == GeneralSeriesKind::Pie {
                        self.visit_general_pie_slices(series, &mut paint);
                    } else {
                        self.visit_general_polar_value_sectors(series, &mut paint);
                    }
                }
                GeneralSeriesKind::Radar => {
                    let base = series
                        .color()
                        .and_then(Color::parse_css)
                        .unwrap_or(DEFAULT_LINE_COLOR);
                    let fill = Color::rgba(
                        base.r(),
                        base.g(),
                        base.b(),
                        opacity_alpha(f64::from(base.a()) / 255.0 * series.fill_opacity()),
                    );
                    self.with_general_radar_polygon(series, |polygon, _, marker_rows| {
                        let fill_first = points.len() as u32;
                        polygon.visit_triangles(|triangle| {
                            points.extend(
                                triangle.map(|(x, y)| [(x * hpr) as f32, (y * vpr) as f32]),
                            );
                        });
                        let fill_count = points.len() as u32 - fill_first;
                        if fill_count > 0 {
                            out.push(Prim::MeshFill {
                                first_point: fill_first,
                                point_count: fill_count,
                                color: fill,
                            });
                        }
                        let first_point = points.len() as u32;
                        let mut markers = Vec::new();
                        for (index, row) in marker_rows.iter().enumerate() {
                            let Some((x, y)) = polygon.vertex(index) else {
                                continue;
                            };
                            let Some(row) = row else {
                                continue;
                            };
                            if !dataset.y_is_valid(*row) {
                                continue;
                            }
                            push_label(*row, x, y, y, plot.width - 4.0, None);
                            if series.point_markers() {
                                push_general_point_symbol(
                                    &mut markers,
                                    dataset.row_symbol(*row).unwrap_or(series.point_symbol()),
                                    x * hpr,
                                    y * vpr,
                                    series.point_radius() * vpr,
                                    dataset.row_color(*row).unwrap_or(base),
                                );
                            }
                        }
                        for index in 0..polygon.vertex_count() {
                            let Some((x, y)) = polygon.vertex(index) else {
                                continue;
                            };
                            let previous = polygon.vertex(
                                (index + polygon.vertex_count() - 1) % polygon.vertex_count(),
                            );
                            let next = polygon.vertex((index + 1) % polygon.vertex_count());
                            if let (Some((px, py)), Some((nx, ny))) = (previous, next) {
                                let cross = (x - px) * (ny - y) - (y - py) * (nx - x);
                                let span = (nx - px).hypot(ny - py);
                                if span > 0.0 && cross.abs() <= span * 1e-9 {
                                    continue;
                                }
                            }
                            points.push([(x * hpr) as f32, (y * vpr) as f32]);
                        }
                        if points.len() > first_point as usize {
                            let point_count = points.len() as u32 - first_point;
                            points.push(points[first_point as usize]);
                            out.push(Prim::Polyline {
                                first_point,
                                point_count: point_count + 1,
                                width: (series.line_width() * vpr) as f32,
                                style: series.line_style().render_style(),
                                line_type: LineType::Simple,
                                color: base,
                            });
                        }
                        out.append(&mut markers);
                    });
                }
                GeneralSeriesKind::XyLine | GeneralSeriesKind::XyArea => {
                    let color = series
                        .color()
                        .and_then(Color::parse_css)
                        .unwrap_or(DEFAULT_LINE_COLOR);
                    let is_area = series.kind() == GeneralSeriesKind::XyArea;
                    if is_area && series.stack_id().is_some() {
                        let fill = Color::rgba(
                            color.r(),
                            color.g(),
                            color.b(),
                            opacity_alpha(series.fill_opacity() * (7.0 / 9.0)),
                        );
                        let mut upper = Vec::<[f32; 2]>::new();
                        let mut lower = Vec::<[f32; 2]>::new();
                        let mut markers = Vec::new();
                        let mut flush_run =
                            |upper: &mut Vec<[f32; 2]>, lower: &mut Vec<[f32; 2]>| {
                                if upper.len() >= 2 && upper.len() == lower.len() {
                                    let upper_first = points.len() as u32;
                                    let point_count = upper.len() as u32;
                                    points.append(upper);
                                    let lower_first = points.len() as u32;
                                    points.append(lower);
                                    if let Some(gradient) = fill_gradient {
                                        out.push(Prim::BandGradientFill {
                                            upper_first,
                                            lower_first,
                                            point_count,
                                            line_type: series.interpolation().render_type(),
                                            gradient,
                                        });
                                    } else {
                                        out.push(Prim::BandFill {
                                            upper_first,
                                            lower_first,
                                            point_count,
                                            line_type: series.interpolation().render_type(),
                                            fill,
                                        });
                                    }
                                    out.push(Prim::Polyline {
                                        first_point: upper_first,
                                        point_count,
                                        width: (series.line_width() * vpr) as f32,
                                        style: series.line_style().render_style(),
                                        line_type: series.interpolation().render_type(),
                                        color,
                                    });
                                } else {
                                    upper.clear();
                                    lower.clear();
                                }
                            };
                        self.visit_general_stacked_area_points(series, |geometry| {
                            if geometry.starts_new_run {
                                flush_run(&mut upper, &mut lower);
                            }
                            upper.push([(geometry.x * hpr) as f32, (geometry.high_y * vpr) as f32]);
                            lower.push([(geometry.x * hpr) as f32, (geometry.low_y * vpr) as f32]);
                            if series.point_markers() {
                                push_general_point_symbol(
                                    &mut markers,
                                    dataset
                                        .row_symbol(geometry.row)
                                        .unwrap_or(series.point_symbol()),
                                    geometry.x * hpr,
                                    geometry.high_y * vpr,
                                    series.point_radius() * vpr,
                                    dataset.row_color(geometry.row).unwrap_or(color),
                                );
                            }
                            push_label(
                                geometry.row,
                                geometry.x,
                                geometry.low_y.min(geometry.high_y) - label_size * 0.65 - 4.0,
                                geometry.low_y.max(geometry.high_y) + label_size * 0.65 + 4.0,
                                plot.width - 4.0,
                                None,
                            );
                            if dot_rows.contains(&(series.id(), geometry.row)) {
                                push_general_active_dot(
                                    &mut active_dots,
                                    geometry.x * hpr,
                                    geometry.high_y * vpr,
                                    dataset.row_color(geometry.row).unwrap_or(color),
                                    dot_background,
                                    vpr,
                                );
                            }
                            let (hovered, selected) =
                                self.general_row_interaction(series.id(), geometry.row);
                            if hovered || selected {
                                let stroke = if selected { PRIMARY } else { GENERAL_HOVER };
                                interaction[usize::from(selected)] = Some(Prim::Circle {
                                    cx: (geometry.x * hpr) as f32,
                                    cy: (geometry.high_y * vpr) as f32,
                                    radius: ((if selected { 5.0 } else { 4.0 }) * vpr) as f32,
                                    fill: Color::rgba(0, 0, 0, 0),
                                    stroke_width: (if selected { 2.0 } else { 1.0 }) * vpr as f32,
                                    stroke,
                                });
                            }
                        });
                        flush_run(&mut upper, &mut lower);
                        out.append(&mut markers);
                        continue;
                    }
                    let baseline_y = is_area
                        .then(|| self.general_path_baseline_y(series))
                        .flatten();
                    let mut run = Vec::<[f32; 2]>::new();
                    let mut markers = Vec::new();
                    let mut flush_run = |run: &mut Vec<[f32; 2]>| {
                        if run.len() >= 2 {
                            let first_point = points.len() as u32;
                            let point_count = run.len() as u32;
                            points.append(run);
                            if let Some(base_y) = baseline_y {
                                out.push(Prim::AreaFill {
                                    first_point,
                                    point_count,
                                    base_y: (base_y * vpr) as f32,
                                    line_type: series.interpolation().render_type(),
                                    gradient: fill_gradient.unwrap_or(Gradient {
                                        top: Color::rgba(
                                            color.r(),
                                            color.g(),
                                            color.b(),
                                            opacity_alpha(series.fill_opacity()),
                                        ),
                                        bottom: Color::rgba(
                                            color.r(),
                                            color.g(),
                                            color.b(),
                                            opacity_alpha(series.fill_opacity() / 3.0),
                                        ),
                                    }),
                                });
                            }
                            out.push(Prim::Polyline {
                                first_point,
                                point_count,
                                width: (series.line_width() * vpr) as f32,
                                style: series.line_style().render_style(),
                                line_type: series.interpolation().render_type(),
                                color,
                            });
                        } else {
                            run.clear();
                        }
                    };
                    self.visit_general_path_points(series, |geometry| {
                        if geometry.starts_new_run {
                            flush_run(&mut run);
                        }
                        run.push([(geometry.x * hpr) as f32, (geometry.y * vpr) as f32]);
                        if series.point_markers() {
                            push_general_point_symbol(
                                &mut markers,
                                dataset
                                    .row_symbol(geometry.row)
                                    .unwrap_or(series.point_symbol()),
                                geometry.x * hpr,
                                geometry.y * vpr,
                                series.point_radius() * vpr,
                                dataset.row_color(geometry.row).unwrap_or(color),
                            );
                        }
                        push_label(
                            geometry.row,
                            geometry.x,
                            geometry.y - label_size * 0.65 - 4.0,
                            geometry.y + label_size * 0.65 + 4.0,
                            plot.width - 4.0,
                            None,
                        );
                        if dot_rows.contains(&(series.id(), geometry.row)) {
                            push_general_active_dot(
                                &mut active_dots,
                                geometry.x * hpr,
                                geometry.y * vpr,
                                dataset.row_color(geometry.row).unwrap_or(color),
                                dot_background,
                                vpr,
                            );
                        }
                        let (hovered, selected) =
                            self.general_row_interaction(series.id(), geometry.row);
                        if hovered || selected {
                            let stroke = if selected { PRIMARY } else { GENERAL_HOVER };
                            interaction[usize::from(selected)] = Some(Prim::Circle {
                                cx: (geometry.x * hpr) as f32,
                                cy: (geometry.y * vpr) as f32,
                                radius: ((if selected { 5.0 } else { 4.0 }) * vpr) as f32,
                                fill: Color::rgba(0, 0, 0, 0),
                                stroke_width: (if selected { 2.0 } else { 1.0 }) * vpr as f32,
                                stroke,
                            });
                        }
                    });
                    flush_run(&mut run);
                    out.append(&mut markers);
                }
                GeneralSeriesKind::RangeBar => {
                    let color = series
                        .color()
                        .and_then(Color::parse_css)
                        .unwrap_or(DEFAULT_LINE_COLOR);
                    self.visit_general_range_bars(series, |geometry| {
                        let left = (geometry.left * hpr).round() as i32;
                        let right = (geometry.right * hpr).round() as i32;
                        let top = (geometry.top * vpr).round() as i32;
                        let bottom = (geometry.bottom * vpr).round() as i32;
                        if right <= left || bottom <= top {
                            return;
                        }
                        let rect = IRect {
                            x: left,
                            y: top,
                            w: right - left,
                            h: bottom - top,
                        };
                        push_general_bar_fill(
                            out,
                            rect,
                            None,
                            color,
                            dataset.row_color(geometry.row),
                            fill_gradient,
                        );
                        let (hovered, selected) =
                            self.general_row_interaction(series.id(), geometry.row);
                        if hovered || selected {
                            interaction[usize::from(selected)] = Some(Prim::RectFrame {
                                rect: IRect {
                                    x: left,
                                    y: top,
                                    w: right - left,
                                    h: bottom - top,
                                },
                                border: ((if selected { 2.0 } else { 1.0 }) * hpr.min(vpr))
                                    .round()
                                    .max(1.0) as i32,
                                color: if selected { PRIMARY } else { GENERAL_HOVER },
                            });
                        }
                    });
                }
                GeneralSeriesKind::RangeArea => {
                    let color = series
                        .color()
                        .and_then(Color::parse_css)
                        .unwrap_or(DEFAULT_LINE_COLOR);
                    let fill = Color::rgba(
                        color.r(),
                        color.g(),
                        color.b(),
                        opacity_alpha(series.fill_opacity() * (7.0 / 9.0)),
                    );
                    let mut upper = Vec::<[f32; 2]>::new();
                    let mut lower = Vec::<[f32; 2]>::new();
                    let mut markers = Vec::new();
                    let mut flush_run = |upper: &mut Vec<[f32; 2]>, lower: &mut Vec<[f32; 2]>| {
                        if upper.len() >= 2 && upper.len() == lower.len() {
                            let upper_first = points.len() as u32;
                            let point_count = upper.len() as u32;
                            points.append(upper);
                            let lower_first = points.len() as u32;
                            points.append(lower);
                            if let Some(gradient) = fill_gradient {
                                out.push(Prim::BandGradientFill {
                                    upper_first,
                                    lower_first,
                                    point_count,
                                    line_type: series.interpolation().render_type(),
                                    gradient,
                                });
                            } else {
                                out.push(Prim::BandFill {
                                    upper_first,
                                    lower_first,
                                    point_count,
                                    line_type: series.interpolation().render_type(),
                                    fill,
                                });
                            }
                            out.push(Prim::Polyline {
                                first_point: upper_first,
                                point_count,
                                width: (series.line_width() * vpr) as f32,
                                style: series.line_style().render_style(),
                                line_type: series.interpolation().render_type(),
                                color,
                            });
                            out.push(Prim::Polyline {
                                first_point: lower_first,
                                point_count,
                                width: (series.line_width() * vpr) as f32,
                                style: series.line_style().render_style(),
                                line_type: series.interpolation().render_type(),
                                color,
                            });
                        } else {
                            upper.clear();
                            lower.clear();
                        }
                    };
                    self.visit_general_range_points(series, |geometry| {
                        if geometry.starts_new_run {
                            flush_run(&mut upper, &mut lower);
                        }
                        upper.push([(geometry.x * hpr) as f32, (geometry.high_y * vpr) as f32]);
                        lower.push([(geometry.x * hpr) as f32, (geometry.low_y * vpr) as f32]);
                        if series.point_markers() {
                            for y in [geometry.low_y, geometry.high_y] {
                                push_general_point_symbol(
                                    &mut markers,
                                    dataset
                                        .row_symbol(geometry.row)
                                        .unwrap_or(series.point_symbol()),
                                    geometry.x * hpr,
                                    y * vpr,
                                    series.point_radius() * vpr,
                                    dataset.row_color(geometry.row).unwrap_or(color),
                                );
                            }
                        }
                        push_label(
                            geometry.row,
                            geometry.x,
                            geometry.high_y - label_size * 0.65 - 4.0,
                            geometry.low_y + label_size * 0.65 + 4.0,
                            plot.width - 4.0,
                            None,
                        );
                        if dot_rows.contains(&(series.id(), geometry.row)) {
                            for y in [geometry.low_y, geometry.high_y] {
                                push_general_active_dot(
                                    &mut active_dots,
                                    geometry.x * hpr,
                                    y * vpr,
                                    dataset.row_color(geometry.row).unwrap_or(color),
                                    dot_background,
                                    vpr,
                                );
                            }
                        }
                        let (hovered, selected) =
                            self.general_row_interaction(series.id(), geometry.row);
                        if hovered || selected {
                            let stroke = if selected { PRIMARY } else { GENERAL_HOVER };
                            interaction[usize::from(selected)] = Some(Prim::Circle {
                                cx: (geometry.x * hpr) as f32,
                                cy: (((geometry.low_y + geometry.high_y) * 0.5) * vpr) as f32,
                                radius: ((if selected { 5.0 } else { 4.0 }) * vpr) as f32,
                                fill: Color::rgba(0, 0, 0, 0),
                                stroke_width: (if selected { 2.0 } else { 1.0 }) * vpr as f32,
                                stroke,
                            });
                        }
                    });
                    flush_run(&mut upper, &mut lower);
                    out.append(&mut markers);
                }
                GeneralSeriesKind::ErrorBar => {
                    let color = series
                        .color()
                        .and_then(Color::parse_css)
                        .unwrap_or(DEFAULT_LINE_COLOR);
                    let line_width = hpr.min(vpr).round().max(1.0) as i32;
                    self.visit_general_error_bars(series, |geometry| {
                        let color = dataset.row_color(geometry.row).unwrap_or(color);
                        let x = (geometry.x * hpr).round() as i32;
                        let y = (geometry.y * vpr).round() as i32;
                        let cap_x = (geometry.cap_half_size * hpr).round().max(1.0) as i32;
                        let cap_y = (geometry.cap_half_size * vpr).round().max(1.0) as i32;
                        let x_low = geometry.x_low.map(|value| (value * hpr).round() as i32);
                        let x_high = geometry.x_high.map(|value| (value * hpr).round() as i32);
                        let y_low = geometry.y_low.map(|value| (value * vpr).round() as i32);
                        let y_high = geometry.y_high.map(|value| (value * vpr).round() as i32);
                        if x_low.is_some() || x_high.is_some() {
                            let from = x_low.unwrap_or(x);
                            let to = x_high.unwrap_or(x);
                            out.push(Prim::HLine {
                                y,
                                x0: from.min(to),
                                x1: from.max(to),
                                width: line_width,
                                style: LineStyle::Solid,
                                color,
                            });
                            for bound in [x_low, x_high].into_iter().flatten() {
                                out.push(Prim::VLine {
                                    x: bound,
                                    y0: y - cap_y,
                                    y1: y + cap_y,
                                    width: line_width,
                                    style: LineStyle::Solid,
                                    color,
                                });
                            }
                        }
                        if y_low.is_some() || y_high.is_some() {
                            let from = y_low.unwrap_or(y);
                            let to = y_high.unwrap_or(y);
                            out.push(Prim::VLine {
                                x,
                                y0: from.min(to),
                                y1: from.max(to),
                                width: line_width,
                                style: LineStyle::Solid,
                                color,
                            });
                            for bound in [y_low, y_high].into_iter().flatten() {
                                out.push(Prim::HLine {
                                    y: bound,
                                    x0: x - cap_x,
                                    x1: x + cap_x,
                                    width: line_width,
                                    style: LineStyle::Solid,
                                    color,
                                });
                            }
                        }
                        out.push(Prim::Circle {
                            cx: (geometry.x * hpr) as f32,
                            cy: (geometry.y * vpr) as f32,
                            radius: (2.0 * vpr) as f32,
                            fill: color,
                            stroke_width: 0.0,
                            stroke: color,
                        });
                        let top = [Some(geometry.y), geometry.y_low, geometry.y_high]
                            .into_iter()
                            .flatten()
                            .fold(geometry.y, f64::min);
                        let bottom = [Some(geometry.y), geometry.y_low, geometry.y_high]
                            .into_iter()
                            .flatten()
                            .fold(geometry.y, f64::max);
                        push_label(
                            geometry.row,
                            geometry.x,
                            top - label_size * 0.65 - 4.0,
                            bottom + label_size * 0.65 + 4.0,
                            plot.width - 4.0,
                            None,
                        );
                        let (hovered, selected) =
                            self.general_row_interaction(series.id(), geometry.row);
                        if hovered || selected {
                            interaction[usize::from(selected)] = Some(Prim::Circle {
                                cx: (geometry.x * hpr) as f32,
                                cy: (geometry.y * vpr) as f32,
                                radius: ((geometry.cap_half_size
                                    + if selected { 3.0 } else { 2.0 })
                                    * vpr) as f32,
                                fill: Color::rgba(0, 0, 0, 0),
                                stroke_width: (if selected { 2.0 } else { 1.0 }) * vpr as f32,
                                stroke: if selected { PRIMARY } else { GENERAL_HOVER },
                            });
                        }
                    });
                }
                GeneralSeriesKind::Column => {
                    let color = series
                        .color()
                        .and_then(Color::parse_css)
                        .unwrap_or(DEFAULT_LINE_COLOR);
                    self.visit_general_columns(series, |geometry| {
                        let left = (geometry.left * hpr).round() as i32;
                        let right = (geometry.right * hpr).round() as i32;
                        let top = (geometry.top * vpr).round() as i32;
                        let bottom = (geometry.bottom * vpr).round() as i32;
                        let width = (right - left).max(1);
                        let height = bottom - top;
                        if height <= 0 {
                            return;
                        }
                        let radii = general_bar_radii(series, geometry, hpr, vpr, false);
                        push_general_bar_fill(
                            out,
                            IRect {
                                x: left,
                                y: top,
                                w: width,
                                h: height,
                            },
                            radii,
                            color,
                            dataset.row_color(geometry.row),
                            fill_gradient,
                        );
                        let (outside, inside) = column_label_rows(geometry, label_size);
                        push_label(
                            geometry.row,
                            (geometry.left + geometry.right) * 0.5,
                            outside,
                            inside,
                            geometry.right - geometry.left - 4.0,
                            None,
                        );
                        let (hovered, selected) =
                            self.general_row_interaction(series.id(), geometry.row);
                        if hovered || selected {
                            interaction[usize::from(selected)] = Some(Prim::RectFrame {
                                rect: IRect {
                                    x: left,
                                    y: top,
                                    w: width,
                                    h: height,
                                },
                                border: ((if selected { 2.0 } else { 1.0 }) * hpr.min(vpr))
                                    .round()
                                    .max(1.0) as i32,
                                color: if selected { PRIMARY } else { GENERAL_HOVER },
                            });
                        }
                    });
                }
                GeneralSeriesKind::Histogram => {
                    let color = series
                        .color()
                        .and_then(Color::parse_css)
                        .unwrap_or(DEFAULT_LINE_COLOR);
                    self.visit_general_histogram_bins(series, |geometry| {
                        let left = (geometry.left * hpr).round() as i32;
                        let right = (geometry.right * hpr).round() as i32;
                        let top = (geometry.top * vpr).round() as i32;
                        let bottom = (geometry.bottom * vpr).round() as i32;
                        let width = (right - left).max(1);
                        let height = bottom - top;
                        if height <= 0 {
                            return;
                        }
                        let rect = IRect {
                            x: left,
                            y: top,
                            w: width,
                            h: height,
                        };
                        let radii = general_bar_radii(series, geometry, hpr, vpr, false);
                        push_general_bar_fill(out, rect, radii, color, None, fill_gradient);
                        let (outside, inside) = column_label_rows(geometry, label_size);
                        push_label(
                            geometry.row,
                            (geometry.left + geometry.right) * 0.5,
                            outside,
                            inside,
                            geometry.right - geometry.left - 4.0,
                            None,
                        );
                        let (hovered, selected) =
                            self.general_row_interaction(series.id(), geometry.row);
                        if hovered || selected {
                            interaction[usize::from(selected)] = Some(Prim::RectFrame {
                                rect,
                                border: ((if selected { 2.0 } else { 1.0 }) * hpr.min(vpr))
                                    .round()
                                    .max(1.0) as i32,
                                color: if selected { PRIMARY } else { GENERAL_HOVER },
                            });
                        }
                    });
                }
                GeneralSeriesKind::HorizontalBar => {
                    let color = series
                        .color()
                        .and_then(Color::parse_css)
                        .unwrap_or(DEFAULT_LINE_COLOR);
                    self.visit_general_horizontal_bars(series, |geometry| {
                        let left = (geometry.left * hpr).round() as i32;
                        let right = (geometry.right * hpr).round() as i32;
                        let top = (geometry.top * vpr).round() as i32;
                        let bottom = (geometry.bottom * vpr).round() as i32;
                        let width = right - left;
                        let height = (bottom - top).max(1);
                        if width <= 0 {
                            return;
                        }
                        let radii = general_bar_radii(series, geometry, hpr, vpr, true);
                        push_general_bar_fill(
                            out,
                            IRect {
                                x: left,
                                y: top,
                                w: width,
                                h: height,
                            },
                            radii,
                            color,
                            dataset.row_color(geometry.row),
                            fill_gradient,
                        );
                        // A horizontal bar's label sits on its own row: past the value end when
                        // there is room, otherwise just inside it. Stacked segments that another
                        // segment continues label their middle.
                        let center_y = (geometry.top + geometry.bottom) * 0.5;
                        let bar_width = geometry.right - geometry.left;
                        if geometry.exposed_end {
                            let (end_x, side) = if geometry.end_at_min {
                                (geometry.left, -1.0)
                            } else {
                                (geometry.right, 1.0)
                            };
                            let outside = push_label(
                                geometry.row,
                                end_x,
                                center_y,
                                center_y,
                                plot.width - 4.0,
                                Some(side),
                            );
                            if outside.is_none() {
                                push_label(
                                    geometry.row,
                                    end_x,
                                    center_y,
                                    center_y,
                                    bar_width - 12.0,
                                    Some(-side),
                                );
                            }
                        } else {
                            push_label(
                                geometry.row,
                                (geometry.left + geometry.right) * 0.5,
                                center_y,
                                center_y,
                                bar_width - 4.0,
                                None,
                            );
                        }
                        let (hovered, selected) =
                            self.general_row_interaction(series.id(), geometry.row);
                        if hovered || selected {
                            interaction[usize::from(selected)] = Some(Prim::RectFrame {
                                rect: IRect {
                                    x: left,
                                    y: top,
                                    w: width,
                                    h: height,
                                },
                                border: ((if selected { 2.0 } else { 1.0 }) * hpr.min(vpr))
                                    .round()
                                    .max(1.0) as i32,
                                color: if selected { PRIMARY } else { GENERAL_HOVER },
                            });
                        }
                    });
                }
                GeneralSeriesKind::BoxPlot => {
                    let color = series
                        .color()
                        .and_then(Color::parse_css)
                        .unwrap_or(DEFAULT_LINE_COLOR);
                    let fill = series
                        .box_fill_color()
                        .and_then(Color::parse_css)
                        .unwrap_or(Color::rgba(color.r(), color.g(), color.b(), 48));
                    let median_color = series
                        .box_median_color()
                        .and_then(Color::parse_css)
                        .unwrap_or(color);
                    let line_width = (series.line_width() * hpr.min(vpr)).round().max(1.0) as i32;
                    self.visit_general_box_plots(series, |geometry| {
                        let row_color = dataset.row_color(geometry.row);
                        let color = row_color.unwrap_or(color);
                        let fill = row_color
                            .map(|color| Color::rgba(color.r(), color.g(), color.b(), 48))
                            .unwrap_or(fill);
                        let median_color = row_color.unwrap_or(median_color);
                        let left = (geometry.left * hpr).round() as i32;
                        let right = (geometry.right * hpr).round() as i32;
                        let q1 = (geometry.q1_y * vpr).round() as i32;
                        let q3 = (geometry.q3_y * vpr).round() as i32;
                        let top = q1.min(q3);
                        let bottom = q1.max(q3);
                        if right <= left || bottom <= top {
                            return;
                        }
                        out.push(Prim::Rect {
                            rect: IRect {
                                x: left,
                                y: top,
                                w: (right - left).max(1),
                                h: (bottom - top).max(1),
                            },
                            color: fill,
                        });
                        let x0 = (geometry.left * hpr).round() as i32;
                        let x1 = (geometry.right * hpr).round() as i32;
                        let cx = (geometry.center_x * hpr).round() as i32;
                        let min_y = (geometry.min_y * vpr).round() as i32;
                        let q1_y = (geometry.q1_y * vpr).round() as i32;
                        let median_y = (geometry.median_y * vpr).round() as i32;
                        let q3_y = (geometry.q3_y * vpr).round() as i32;
                        let max_y = (geometry.max_y * vpr).round() as i32;
                        for (y, stroke) in [
                            (q1_y, color),
                            (median_y, median_color),
                            (q3_y, color),
                            (min_y, color),
                            (max_y, color),
                        ] {
                            out.push(Prim::HLine {
                                y,
                                x0,
                                x1,
                                width: line_width,
                                style: LineStyle::Solid,
                                color: stroke,
                            });
                        }
                        for x in [x0, x1] {
                            out.push(Prim::VLine {
                                x,
                                y0: q1_y.min(q3_y),
                                y1: q1_y.max(q3_y),
                                width: line_width,
                                style: LineStyle::Solid,
                                color,
                            });
                        }
                        for (from, to) in [(min_y, q1_y), (q3_y, max_y)] {
                            out.push(Prim::VLine {
                                x: cx,
                                y0: from.min(to),
                                y1: from.max(to),
                                width: line_width,
                                style: LineStyle::Solid,
                                color,
                            });
                        }
                        push_label(
                            geometry.row,
                            geometry.center_x,
                            geometry.min_y.min(geometry.max_y) - label_size * 0.65 - 4.0,
                            geometry.min_y.max(geometry.max_y) + label_size * 0.65 + 4.0,
                            geometry.right - geometry.left - 4.0,
                            None,
                        );
                        let (hovered, selected) =
                            self.general_row_interaction(series.id(), geometry.row);
                        if hovered || selected {
                            interaction[usize::from(selected)] = Some(Prim::RectFrame {
                                rect: IRect {
                                    x: left,
                                    y: (geometry.min_y.min(geometry.max_y) * vpr).round() as i32,
                                    w: (right - left).max(1),
                                    h: ((geometry.min_y.max(geometry.max_y)
                                        - geometry.min_y.min(geometry.max_y))
                                        * vpr)
                                        .round()
                                        .max(1.0) as i32,
                                },
                                border: ((if selected { 2.0 } else { 1.0 }) * hpr.min(vpr))
                                    .round()
                                    .max(1.0) as i32,
                                color: if selected { PRIMARY } else { GENERAL_HOVER },
                            });
                        }
                    });
                }
                GeneralSeriesKind::HeatmapGrid => {
                    let base_color = series.color().and_then(Color::parse_css);
                    let low = series
                        .heatmap_low_color()
                        .and_then(Color::parse_css)
                        .unwrap_or_else(|| {
                            base_color.map_or(Color::rgba(0, 100, 0, 51), |base| {
                                Color::rgba(
                                    base.r(),
                                    base.g(),
                                    base.b(),
                                    ((base.a() as f64) * 0.2).round() as u8,
                                )
                            })
                        });
                    let high = series
                        .heatmap_high_color()
                        .and_then(Color::parse_css)
                        .unwrap_or_else(|| base_color.unwrap_or(Color::rgba(0, 255, 100, 255)));
                    self.visit_general_heatmap_cells(series, |geometry| {
                        let left = (geometry.left * hpr).round() as i32;
                        let right = (geometry.right * hpr).round() as i32;
                        let top = (geometry.top * vpr).round() as i32;
                        let bottom = (geometry.bottom * vpr).round() as i32;
                        let width = (right - left).max(1);
                        let height = (bottom - top).max(1);
                        let color = if series.heatmap_low_color().is_none()
                            && series.heatmap_high_color().is_none()
                        {
                            let intensity = geometry.intensity.clamp(0.0, 1.0);
                            base_color.map_or_else(
                                || {
                                    Color::rgba(
                                        0,
                                        (100.0 + 155.0 * intensity).round() as u8,
                                        (100.0 * intensity).round() as u8,
                                        (51.0 + 204.0 * intensity).round() as u8,
                                    )
                                },
                                |base| {
                                    Color::rgba(
                                        base.r(),
                                        base.g(),
                                        base.b(),
                                        ((base.a() as f64) * (0.2 + 0.8 * intensity)).round() as u8,
                                    )
                                },
                            )
                        } else {
                            interpolate_color(low, high, geometry.intensity)
                        };
                        out.push(Prim::Rect {
                            rect: IRect {
                                x: left,
                                y: top,
                                w: width,
                                h: height,
                            },
                            color: dataset.row_color(geometry.row).unwrap_or(color),
                        });
                        push_label(
                            geometry.row,
                            (geometry.left + geometry.right) * 0.5,
                            geometry.top - label_size * 0.65,
                            geometry.bottom + label_size * 0.65,
                            geometry.right - geometry.left - 4.0,
                            None,
                        );
                        let (hovered, selected) =
                            self.general_row_interaction(series.id(), geometry.row);
                        if hovered || selected {
                            interaction[usize::from(selected)] = Some(Prim::RectFrame {
                                rect: IRect {
                                    x: left,
                                    y: top,
                                    w: width,
                                    h: height,
                                },
                                border: ((if selected { 2.0 } else { 1.0 }) * hpr.min(vpr))
                                    .round()
                                    .max(1.0) as i32,
                                color: if selected { PRIMARY } else { GENERAL_HOVER },
                            });
                        }
                    });
                }
                GeneralSeriesKind::Scatter | GeneralSeriesKind::Bubble => {
                    let color = series
                        .color()
                        .and_then(Color::parse_css)
                        .unwrap_or(DEFAULT_LINE_COLOR);
                    self.visit_general_scatter_points(series, |geometry| {
                        push_general_point_symbol(
                            out,
                            if series.kind() == GeneralSeriesKind::Bubble {
                                GeneralPointSymbol::Circle
                            } else {
                                dataset
                                    .row_symbol(geometry.row)
                                    .unwrap_or(series.point_symbol())
                            },
                            geometry.x * hpr,
                            geometry.y * vpr,
                            geometry.radius * vpr,
                            dataset.row_color(geometry.row).unwrap_or(color),
                        );
                        push_label(
                            geometry.row,
                            geometry.x,
                            geometry.y - geometry.radius - label_size * 0.65 - 2.0,
                            geometry.y + geometry.radius + label_size * 0.65 + 2.0,
                            plot.width - 4.0,
                            None,
                        );
                        let (hovered, selected) =
                            self.general_row_interaction(series.id(), geometry.row);
                        if hovered || selected {
                            let stroke = if selected { PRIMARY } else { GENERAL_HOVER };
                            interaction[usize::from(selected)] = Some(Prim::Circle {
                                cx: (geometry.x * hpr) as f32,
                                cy: (geometry.y * vpr) as f32,
                                radius: ((geometry.radius + if selected { 3.0 } else { 2.0 }) * vpr)
                                    as f32,
                                fill: Color::rgba(0, 0, 0, 0),
                                stroke_width: (if selected { 2.0 } else { 1.0 }) * vpr as f32,
                                stroke,
                            });
                        }
                    });
                }
            }
        }
        self.push_general_reference_marks(pane_id, plot, hpr, vpr, out);
        out.extend(leader_primitives);
        out.extend(label_primitives);
        active_dots.extend(interaction.into_iter().flatten());
        active_dots
    }

    fn push_general_reference_regions(
        &self,
        pane_id: crate::PaneId,
        plot: crate::general_axes::GeneralPlotRect,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        for reference in self
            .general_reference_iter()
            .filter(|reference| reference.pane_id() == pane_id)
        {
            let GeneralReferenceOptions::Region {
                x_axis_id,
                y_axis_id,
                x_from,
                x_to,
                y_from,
                y_to,
                fill_color,
                ..
            } = reference.options()
            else {
                continue;
            };
            let x0 = match x_from {
                Some(value) => self.general_reference_coordinate(x_axis_id, value, plot),
                None => Some(0.0),
            };
            let x1 = match x_to {
                Some(value) => self.general_reference_coordinate(x_axis_id, value, plot),
                None => Some(plot.width),
            };
            let (Some(x0), Some(x1)) = (x0, x1) else {
                continue;
            };
            let (y0, y1) = match (y_from, y_to) {
                (Some(from), Some(to)) => {
                    let (Some(y0), Some(y1)) = (
                        self.general_reference_coordinate(y_axis_id, from, plot),
                        self.general_reference_coordinate(y_axis_id, to, plot),
                    ) else {
                        continue;
                    };
                    (y0, y1)
                }
                (Some(from), None) => {
                    let Some(y0) = self.general_reference_coordinate(y_axis_id, from, plot) else {
                        continue;
                    };
                    (y0, plot.y + plot.height)
                }
                (None, Some(to)) => {
                    let Some(y1) = self.general_reference_coordinate(y_axis_id, to, plot) else {
                        continue;
                    };
                    (plot.y, y1)
                }
                (None, None) => (plot.y, plot.y + plot.height),
            };
            let left = (x0.min(x1) * hpr).round() as i32;
            let right = (x0.max(x1) * hpr).round() as i32;
            let top = (y0.min(y1) * vpr).round() as i32;
            let bottom = (y0.max(y1) * vpr).round() as i32;
            if right <= left || bottom <= top {
                continue;
            }
            out.push(Prim::Rect {
                rect: IRect {
                    x: left,
                    y: top,
                    w: right - left,
                    h: bottom - top,
                },
                color: fill_color
                    .as_deref()
                    .and_then(Color::parse_css)
                    .unwrap_or(GENERAL_REFERENCE_REGION),
            });
        }
    }

    fn push_general_reference_marks(
        &self,
        pane_id: crate::PaneId,
        plot: crate::general_axes::GeneralPlotRect,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let plot_top = (plot.y * vpr).round() as i32;
        let plot_bottom = ((plot.y + plot.height) * vpr).round() as i32;
        let plot_right = (plot.width * hpr).round() as i32;
        for reference in self
            .general_reference_iter()
            .filter(|reference| reference.pane_id() == pane_id)
        {
            match reference.options() {
                GeneralReferenceOptions::Line {
                    axis_id,
                    value,
                    color,
                    line_width,
                    ..
                } => {
                    let Some(axis) = self.general_axis(axis_id) else {
                        continue;
                    };
                    let Some(coordinate) = self.general_reference_coordinate(axis_id, value, plot)
                    else {
                        continue;
                    };
                    let color = color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or(GENERAL_REFERENCE_MARK);
                    let width = (*line_width * hpr.min(vpr)).round().max(1.0) as i32;
                    match axis.dimension() {
                        AxisDimension::X => out.push(Prim::VLine {
                            x: (coordinate * hpr).round() as i32,
                            y0: plot_top,
                            y1: plot_bottom,
                            width,
                            style: LineStyle::Solid,
                            color,
                        }),
                        AxisDimension::Y => out.push(Prim::HLine {
                            y: (coordinate * vpr).round() as i32,
                            x0: 0,
                            x1: plot_right,
                            width,
                            style: LineStyle::Solid,
                            color,
                        }),
                        AxisDimension::Angle | AxisDimension::Radius => {}
                    }
                }
                GeneralReferenceOptions::Dot {
                    x_axis_id,
                    y_axis_id,
                    x,
                    y,
                    color,
                    radius,
                    ..
                } => {
                    let (Some(x), Some(y)) = (
                        self.general_reference_coordinate(x_axis_id, x, plot),
                        self.general_reference_coordinate(y_axis_id, y, plot),
                    ) else {
                        continue;
                    };
                    let color = color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or(GENERAL_REFERENCE_MARK);
                    out.push(Prim::Circle {
                        cx: (x * hpr) as f32,
                        cy: (y * vpr) as f32,
                        radius: (*radius * vpr) as f32,
                        fill: color,
                        stroke_width: 0.0,
                        stroke: color,
                    });
                }
                GeneralReferenceOptions::Region { .. } => {}
            }
        }
    }

    fn general_reference_coordinate(
        &self,
        axis_id: &str,
        value: &GeneralReferenceValue,
        plot: crate::general_axes::GeneralPlotRect,
    ) -> Option<f64> {
        let axis = self.general_axis(axis_id)?;
        let domain = self.effective_general_axis_domain(axis)?;
        let (from, to, low, high) = match axis.dimension() {
            AxisDimension::X => {
                let range = if axis.range_reversed() {
                    (plot.width, 0.0)
                } else {
                    (0.0, plot.width)
                };
                (range.0, range.1, 0.0, plot.width)
            }
            AxisDimension::Y => {
                let bottom = plot.y + plot.height;
                let range = if axis.range_reversed() {
                    (plot.y, bottom)
                } else {
                    (bottom, plot.y)
                };
                (range.0, range.1, plot.y, bottom)
            }
            AxisDimension::Angle | AxisDimension::Radius => return None,
        };
        let coordinate = match (value, domain) {
            (GeneralReferenceValue::Numeric(value), GeneralAxisDomain::Numeric(domain)) => {
                NumericAxisScale::new(axis.scale(), domain, from, to)?.coordinate(*value)
            }
            (GeneralReferenceValue::Temporal(value), GeneralAxisDomain::Temporal([start, end])) => {
                LinearScale::new(start as f64, end as f64, from, to)
                    .ok()?
                    .coordinate(*value as f64)
            }
            (GeneralReferenceValue::Category(value), GeneralAxisDomain::Category(categories)) => {
                let index = categories.iter().position(|category| category == value)?;
                self.category_motion_bounds(axis, value, from, to)
                    .map(|(first, second)| (first + second) / 2.0)
                    .or_else(|| match axis.scale() {
                        GeneralScaleType::Band => {
                            let scale = BandScale::new(
                                categories.len(),
                                from,
                                to,
                                axis.band_padding_inner(),
                                axis.band_padding_outer(),
                                0.5,
                            )
                            .ok()?;
                            scale.bounds(index).map(|(from, to)| (from + to) * 0.5)
                        }
                        GeneralScaleType::Point => PointScale::new(
                            categories.len(),
                            from,
                            to,
                            axis.band_padding_outer(),
                            0.5,
                        )
                        .ok()?
                        .coordinate(index),
                        _ => None,
                    })
            }
            _ => None,
        }?;
        coordinate
            .is_finite()
            .then_some(coordinate)
            .filter(|coordinate| *coordinate >= low.min(high) && *coordinate <= high.max(low))
    }
}
