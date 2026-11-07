//! Gauge dial with the pinned ECharts 6.1.0 `gauge` series defaults: a 75% radius, a 270° track
//! running clockwise from 225° to -45°, a 10 px axis line split into colored value bands, an
//! optional progress arc over it, ten split lines with five ticks each, value labels, a kite
//! pointer 60% of the radius long, the gauge label at 20% and the value text at 40% below the
//! center. Values outside `min..max` clamp to the track ends, like ECharts' clipped mapping.

use std::f64::consts::PI;

use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{LineStyle, LineType, Prim, TextAlign};

use super::{
    GaugeInput, GeneralLayoutAccessibilityItem, GeneralLayoutElement, GeneralLayoutHit,
    LayoutItemTooltip, LayoutSeriesEntry, SunburstShape, add_text_bytes, invalid_data,
    point_in_triangle, resource_limit,
};
use crate::general_axes::GeneralPlotRect;
use crate::{ChartEngine, ChartError, GeneralLayoutSeriesId};

const MAX_BANDS: usize = 32;
const MAX_DECIMALS: u8 = 10;
const MAX_COLOR_BYTES: usize = 64;
const RADIUS: f64 = 0.75;
/// ECharts' 225° start angle as a screen angle (radians clockwise from three o'clock, Y down).
const START_ANGLE: f64 = -1.25 * PI;
/// 225° to -45° clockwise.
const SWEEP: f64 = 1.5 * PI;
const AXIS_LINE_WIDTH: f64 = 10.0;
const SPLIT_NUMBER: usize = 10;
const SPLIT_LENGTH: f64 = 10.0;
const SPLIT_DISTANCE: f64 = 10.0;
const SPLIT_WIDTH: f64 = 3.0;
const TICK_SPLIT_NUMBER: usize = 5;
const TICK_LENGTH: f64 = 6.0;
const TICK_DISTANCE: f64 = 10.0;
const TICK_WIDTH: f64 = 1.0;
const LABEL_DISTANCE: f64 = 15.0;
const LABEL_SIZE: f64 = 12.0;
const POINTER_LENGTH: f64 = 0.6;
const POINTER_WIDTH: f64 = 6.0;
const TITLE_OFFSET: f64 = 0.2;
const TITLE_SIZE: f64 = 16.0;
const DETAIL_OFFSET: f64 = 0.4;
const DETAIL_SIZE: f64 = 30.0;

pub(super) fn validate(gauge: &GaugeInput) -> Result<(), ChartError> {
    if !gauge.value.is_finite() || !gauge.min.is_finite() || !gauge.max.is_finite() {
        return Err(invalid_data("gauge value, min and max must be finite"));
    }
    if gauge.min >= gauge.max || !(gauge.max - gauge.min).is_finite() {
        return Err(invalid_data("gauge min must be below a finite-span max"));
    }
    if gauge.decimals.is_some_and(|digits| digits > MAX_DECIMALS) {
        return Err(invalid_data("gauge decimals exceed 10"));
    }
    if gauge.bands.len() > MAX_BANDS {
        return Err(resource_limit("too many gauge bands"));
    }
    let mut bytes = 0usize;
    add_text_bytes(&mut bytes, &gauge.label)?;
    add_text_bytes(&mut bytes, &gauge.unit)?;
    let mut previous = gauge.min;
    for band in &gauge.bands {
        if !band.to.is_finite() || band.to <= previous || band.to > gauge.max {
            return Err(invalid_data(
                "gauge band bounds must ascend inside min..max",
            ));
        }
        previous = band.to;
        if band.color.len() > MAX_COLOR_BYTES || Color::parse_css(&band.color).is_none() {
            return Err(invalid_data("invalid gauge band color"));
        }
        add_text_bytes(&mut bytes, &band.color)?;
        add_text_bytes(&mut bytes, &band.label)?;
    }
    Ok(())
}

/// Shortest decimal text of a scale value after removing floating-point split noise.
fn scale_text(value: f64) -> String {
    let rounded = if value.abs() < 1e8 {
        (value * 1e10).round() / 1e10
    } else {
        value
    };
    if rounded == 0.0 {
        "0".to_owned()
    } else {
        rounded.to_string()
    }
}

fn value_text(gauge: &GaugeInput) -> String {
    let number = match gauge.decimals {
        Some(digits) => format!("{:.*}", usize::from(digits), gauge.value),
        None if gauge.value == 0.0 => "0".to_owned(),
        None => gauge.value.to_string(),
    };
    format!("{number}{}", gauge.unit)
}

/// Appends one annular sector as a triangle mesh over the same segments its hit test uses.
pub(super) fn push_sector_mesh(
    shape: SunburstShape,
    color: Color,
    (hpr, vpr): (f64, f64),
    out: &mut Vec<Prim>,
    points: &mut Vec<[f32; 2]>,
) {
    if shape.end <= shape.start || shape.outer <= shape.inner {
        return;
    }
    let segments = shape.segments();
    let first_point = points.len() as u32;
    for segment in 0..segments {
        let [a, b, c, d] = shape.segment_vertices(segment);
        points.extend([a, b, c, a, c, d].map(|(x, y)| [(x * hpr) as f32, (y * vpr) as f32]));
    }
    out.push(Prim::MeshFill {
        first_point,
        point_count: (segments * 6) as u32,
        color,
    });
}

pub(super) struct GaugeLayout<'a> {
    gauge: &'a GaugeInput,
    cx: f64,
    cy: f64,
    r: f64,
}

impl<'a> GaugeLayout<'a> {
    pub(super) fn new(gauge: &'a GaugeInput, plot: GeneralPlotRect) -> Self {
        Self {
            gauge,
            cx: plot.x + plot.width * 0.5,
            cy: plot.y + plot.height * 0.5,
            r: RADIUS * plot.width.min(plot.height) * 0.5,
        }
    }

    fn drawable(&self) -> bool {
        self.r > AXIS_LINE_WIDTH && self.gauge.max > self.gauge.min
    }

    fn angle(&self, value: f64) -> f64 {
        let span = self.gauge.max - self.gauge.min;
        START_ANGLE + SWEEP * ((value - self.gauge.min) / span).clamp(0.0, 1.0)
    }

    fn sector(&self, from: f64, to: f64) -> SunburstShape {
        SunburstShape {
            row: 0,
            cx: self.cx,
            cy: self.cy,
            inner: self.r - AXIS_LINE_WIDTH,
            outer: self.r,
            start: self.angle(from),
            end: self.angle(to),
        }
    }

    fn band_sector(&self, row: usize) -> Option<SunburstShape> {
        let band = self.gauge.bands.get(row)?;
        let from = row
            .checked_sub(1)
            .map_or(self.gauge.min, |previous| self.gauge.bands[previous].to);
        Some(self.sector(from, band.to))
    }

    fn progress_sector(&self) -> Option<SunburstShape> {
        (self.gauge.progress && self.gauge.value > self.gauge.min)
            .then(|| self.sector(self.gauge.min, self.gauge.value))
    }

    /// Kite outline: tail, one side, tip, other side (the ECharts `PointerPath` shape).
    fn pointer(&self) -> [(f64, f64); 4] {
        let angle = self.angle(self.gauge.value);
        let length = POINTER_LENGTH * self.r;
        let (ux, uy) = (angle.cos(), angle.sin());
        let (px, py) = (uy, -ux);
        let tail = if POINTER_WIDTH >= length / 3.0 {
            POINTER_WIDTH
        } else {
            POINTER_WIDTH * 2.0
        };
        [
            (self.cx - ux * tail, self.cy - uy * tail),
            (self.cx + px * POINTER_WIDTH, self.cy + py * POINTER_WIDTH),
            (self.cx + ux * length, self.cy + uy * length),
            (self.cx - px * POINTER_WIDTH, self.cy - py * POINTER_WIDTH),
        ]
    }

    fn pointer_triangles(&self) -> [[(f64, f64); 3]; 2] {
        let [tail, side, tip, other] = self.pointer();
        [[tail, side, tip], [tail, tip, other]]
    }

    /// Topmost item under `(x, y)`, in paint order reversed: pointer, progress, then bands.
    pub(super) fn hit(&self, x: f64, y: f64) -> Option<(GeneralLayoutElement, usize)> {
        if !self.drawable() {
            return None;
        }
        if self
            .pointer_triangles()
            .iter()
            .any(|&[a, b, c]| point_in_triangle((x, y), a, b, c))
        {
            return Some((GeneralLayoutElement::Pointer, 0));
        }
        if self
            .progress_sector()
            .is_some_and(|shape| shape.end > shape.start && shape.contains(x, y))
        {
            return Some((GeneralLayoutElement::Progress, 0));
        }
        (0..self.gauge.bands.len())
            .find(|&row| {
                self.band_sector(row)
                    .is_some_and(|shape| shape.end > shape.start && shape.contains(x, y))
            })
            .map(|row| (GeneralLayoutElement::Band, row))
    }

    pub(super) fn anchor(&self, element: GeneralLayoutElement, row: usize) -> Option<(f64, f64)> {
        if !self.drawable() {
            return None;
        }
        let middle = |shape: SunburstShape| {
            let angle = (shape.start + shape.end) * 0.5;
            let radius = (shape.inner + shape.outer) * 0.5;
            (
                self.cx + radius * angle.cos(),
                self.cy + radius * angle.sin(),
            )
        };
        match element {
            GeneralLayoutElement::Pointer if row == 0 => {
                let [_, _, tip, _] = self.pointer();
                Some(((self.cx + tip.0) * 0.5, (self.cy + tip.1) * 0.5))
            }
            GeneralLayoutElement::Progress if row == 0 => self.progress_sector().map(middle),
            GeneralLayoutElement::Band => self.band_sector(row).map(middle),
            _ => None,
        }
    }
}

impl ChartEngine {
    fn gauge_label(&self, gauge: &GaugeInput) -> String {
        if gauge.label.is_empty() {
            crate::text::complete::GAUGE_VALUE
                .text(self.language)
                .to_owned()
        } else {
            gauge.label.clone()
        }
    }

    fn gauge_band_range(&self, gauge: &GaugeInput, row: usize) -> Option<String> {
        let band = gauge.bands.get(row)?;
        let from = row
            .checked_sub(1)
            .map_or(gauge.min, |previous| gauge.bands[previous].to);
        Some(crate::text::complete::VALUE_RANGE.format(
            self.language,
            &[("start", &scale_text(from)), ("end", &scale_text(band.to))],
        ))
    }

    fn gauge_band_label(&self, gauge: &GaugeInput, row: usize) -> Option<String> {
        let band = gauge.bands.get(row)?;
        if band.label.is_empty() {
            self.gauge_band_range(gauge, row)
        } else {
            Some(band.label.clone())
        }
    }

    pub(super) fn gauge_item_identity(
        &self,
        gauge: &GaugeInput,
        element: GeneralLayoutElement,
        row: usize,
    ) -> Option<(String, String)> {
        match element {
            GeneralLayoutElement::Pointer | GeneralLayoutElement::Progress if row == 0 => {
                Some(("value".to_owned(), self.gauge_label(gauge)))
            }
            GeneralLayoutElement::Band => {
                Some((format!("band-{row}"), self.gauge_band_label(gauge, row)?))
            }
            _ => None,
        }
    }

    pub(super) fn gauge_item_tooltip(
        &self,
        series: &LayoutSeriesEntry,
        gauge: &GaugeInput,
        element: GeneralLayoutElement,
        row: usize,
    ) -> Option<LayoutItemTooltip> {
        let title = series.options.title.clone();
        match element {
            GeneralLayoutElement::Pointer | GeneralLayoutElement::Progress if row == 0 => {
                Some(LayoutItemTooltip {
                    label: self.gauge_label(gauge),
                    title,
                    value: None,
                    color: series.color,
                    detail: Some(value_text(gauge)),
                })
            }
            GeneralLayoutElement::Band => {
                let band = gauge.bands.get(row)?;
                Some(LayoutItemTooltip {
                    label: self.gauge_band_label(gauge, row)?,
                    title,
                    value: None,
                    color: Color::parse_css(&band.color).unwrap_or(series.color),
                    detail: self.gauge_band_range(gauge, row),
                })
            }
            _ => None,
        }
    }

    /// The pointer (row 0 of the value) followed by one item per band.
    pub(super) fn gauge_accessibility_items(
        &self,
        series: GeneralLayoutSeriesId,
        gauge: &GaugeInput,
        selected: Option<GeneralLayoutHit>,
        offset: usize,
        limit: usize,
    ) -> Vec<GeneralLayoutAccessibilityItem> {
        let total = 1 + gauge.bands.len();
        (offset..offset.saturating_add(limit).min(total))
            .filter_map(|index| {
                let (element, row, value) = if index == 0 {
                    (GeneralLayoutElement::Pointer, 0, gauge.value)
                } else {
                    let band = &gauge.bands[index - 1];
                    (GeneralLayoutElement::Band, index - 1, band.to)
                };
                let (id, label) = self.gauge_item_identity(gauge, element, row)?;
                Some(GeneralLayoutAccessibilityItem {
                    element,
                    row,
                    id,
                    label,
                    value: Some(value),
                    selected: selected
                        == Some(GeneralLayoutHit {
                            series,
                            element,
                            row,
                        }),
                    expanded: None,
                    placed: None,
                })
            })
            .collect()
    }

    /// Paints `presentation` (the transition frame) while the value text reports `target`, the
    /// committed value, as ECharts does without `valueAnimation`.
    pub(super) fn build_gauge_frame(
        &self,
        series: &LayoutSeriesEntry,
        (presentation, target): (&GaugeInput, &GaugeInput),
        plot: GeneralPlotRect,
        (hpr, vpr): (f64, f64),
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let layout = GaugeLayout::new(presentation, plot);
        if !layout.drawable() {
            return;
        }
        let options = self.options.get();
        let family = &options.layout.font_family;
        let text_color = self.primary_text_color();
        let surface = self.chart_surface_color();
        let mix = |amount: f64| {
            let channel = |from: u8, to: u8| {
                (f64::from(from) + (f64::from(to) - f64::from(from)) * amount).round() as u8
            };
            Color::rgb(
                channel(surface.r(), text_color.r()),
                channel(surface.g(), text_color.g()),
                channel(surface.b(), text_color.b()),
            )
        };
        let track = mix(0.1);
        let split_color = mix(0.75);
        let tick_color = mix(0.6);

        let mut from = presentation.min;
        for (row, band) in presentation.bands.iter().enumerate() {
            let color = Color::parse_css(&band.color).unwrap_or(track);
            if let Some(shape) = layout.band_sector(row) {
                push_sector_mesh(shape, color, (hpr, vpr), out, points);
            }
            from = band.to;
        }
        if from < presentation.max {
            push_sector_mesh(
                layout.sector(from, presentation.max),
                track,
                (hpr, vpr),
                out,
                points,
            );
        }
        if let Some(shape) = layout.progress_sector() {
            push_sector_mesh(shape, series.color, (hpr, vpr), out, points);
        }

        let ratio = hpr.min(vpr);
        let mut radial = |angle: f64, from: f64, to: f64, width: f64, color: Color| {
            let first_point = points.len() as u32;
            let (ux, uy) = (angle.cos(), angle.sin());
            for radius in [from, to] {
                points.push([
                    ((layout.cx + ux * radius) * hpr) as f32,
                    ((layout.cy + uy * radius) * vpr) as f32,
                ]);
            }
            out.push(Prim::Polyline {
                first_point,
                point_count: 2,
                width: (width * ratio) as f32,
                style: LineStyle::Solid,
                line_type: LineType::Simple,
                color,
            });
        };
        let split_start = layout.r - SPLIT_DISTANCE - AXIS_LINE_WIDTH;
        let tick_start = layout.r - TICK_DISTANCE - AXIS_LINE_WIDTH;
        let step = SWEEP / SPLIT_NUMBER as f64;
        for split in 0..=SPLIT_NUMBER {
            let angle = START_ANGLE + step * split as f64;
            if split_start - SPLIT_LENGTH > 0.0 {
                radial(
                    angle,
                    split_start,
                    split_start - SPLIT_LENGTH,
                    SPLIT_WIDTH,
                    split_color,
                );
            }
            if split < SPLIT_NUMBER && tick_start - TICK_LENGTH > 0.0 {
                for tick in 1..TICK_SPLIT_NUMBER {
                    radial(
                        angle + step * tick as f64 / TICK_SPLIT_NUMBER as f64,
                        tick_start,
                        tick_start - TICK_LENGTH,
                        TICK_WIDTH,
                        tick_color,
                    );
                }
            }
        }

        let text =
            |text: String, x: f64, y: f64, size: f64, weight: u16, align: TextAlign| Prim::Text {
                x: (x * hpr) as f32,
                y: (y * vpr) as f32,
                text,
                color: text_color,
                size: (size * vpr) as f32,
                family: family.clone(),
                align,
                weight,
                italic: false,
            };
        let label_radius = layout.r - SPLIT_LENGTH - LABEL_DISTANCE - SPLIT_DISTANCE;
        if label_radius > 0.0 {
            let span = presentation.max - presentation.min;
            for split in 0..=SPLIT_NUMBER {
                let angle = START_ANGLE + step * split as f64;
                let (ux, uy) = (angle.cos(), angle.sin());
                let align = if ux < -0.4 {
                    TextAlign::Left
                } else if ux > 0.4 {
                    TextAlign::Right
                } else {
                    TextAlign::Center
                };
                // Prim::Text anchors the vertical center; ECharts hangs top labels below the
                // anchor and stands bottom labels above it.
                let shift = if uy < -0.8 {
                    LABEL_SIZE * 0.5
                } else if uy > 0.8 {
                    -LABEL_SIZE * 0.5
                } else {
                    0.0
                };
                let value = presentation.min + split as f64 * span / SPLIT_NUMBER as f64;
                out.push(text(
                    scale_text(value),
                    layout.cx + ux * label_radius,
                    layout.cy + uy * label_radius + shift,
                    LABEL_SIZE,
                    400,
                    align,
                ));
            }
        }
        if !target.label.is_empty() {
            out.push(text(
                target.label.clone(),
                layout.cx,
                layout.cy + TITLE_OFFSET * layout.r,
                TITLE_SIZE,
                400,
                TextAlign::Center,
            ));
        }
        out.push(text(
            value_text(target),
            layout.cx,
            layout.cy + DETAIL_OFFSET * layout.r,
            DETAIL_SIZE,
            700,
            TextAlign::Center,
        ));

        let first_point = points.len() as u32;
        for triangle in layout.pointer_triangles() {
            points.extend(triangle.map(|(x, y)| [(x * hpr) as f32, (y * vpr) as f32]));
        }
        out.push(Prim::MeshFill {
            first_point,
            point_count: 6,
            color: series.color,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GaugeBand;

    const PLOT: GeneralPlotRect = GeneralPlotRect {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 300.0,
    };

    fn gauge(value: f64) -> GaugeInput {
        GaugeInput {
            value,
            min: 0.0,
            max: 100.0,
            label: String::new(),
            unit: String::new(),
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
            ],
            progress: true,
        }
    }

    #[test]
    fn angles_follow_the_echarts_225_to_minus_45_clockwise_track() {
        let input = gauge(50.0);
        let layout = GaugeLayout::new(&input, PLOT);
        assert!((layout.r - 112.5).abs() < 1e-12);
        assert!((layout.angle(0.0) - START_ANGLE).abs() < 1e-12);
        assert!((layout.angle(100.0) - (START_ANGLE + SWEEP)).abs() < 1e-12);
        // 50% points straight up: screen angle -π/2.
        assert!((layout.angle(50.0) + PI / 2.0).abs() < 1e-12);
        // Values clamp to the track ends.
        assert_eq!(layout.angle(-10.0), layout.angle(0.0));
        assert_eq!(layout.angle(250.0), layout.angle(100.0));
        let [_, _, tip, _] = layout.pointer();
        assert!((tip.0 - 200.0).abs() < 1e-9);
        assert!((tip.1 - (150.0 - 0.6 * 112.5)).abs() < 1e-9);
    }

    #[test]
    fn hits_prefer_pointer_then_progress_then_bands() {
        let input = gauge(50.0);
        let layout = GaugeLayout::new(&input, PLOT);
        let on_track = |value: f64| {
            let angle = layout.angle(value);
            let radius = layout.r - AXIS_LINE_WIDTH * 0.5;
            (
                layout.cx + radius * angle.cos(),
                layout.cy + radius * angle.sin(),
            )
        };
        let (x, y) = (layout.cx, layout.cy - 40.0);
        assert_eq!(layout.hit(x, y), Some((GeneralLayoutElement::Pointer, 0)));
        let (x, y) = on_track(20.0);
        assert_eq!(layout.hit(x, y), Some((GeneralLayoutElement::Progress, 0)));
        let (x, y) = on_track(60.0);
        assert_eq!(layout.hit(x, y), Some((GeneralLayoutElement::Band, 1)));
        // The unbanded remainder of the track is not an item.
        let (x, y) = on_track(85.0);
        assert_eq!(layout.hit(x, y), None);
        let mut plain = gauge(50.0);
        plain.progress = false;
        let layout = GaugeLayout::new(&plain, PLOT);
        let (x, y) = on_track(20.0);
        assert_eq!(layout.hit(x, y), Some((GeneralLayoutElement::Band, 0)));
        assert_eq!(layout.anchor(GeneralLayoutElement::Progress, 0), None);
    }

    #[test]
    fn validation_rejects_bad_ranges_bands_and_formats() {
        assert!(validate(&gauge(50.0)).is_ok());
        assert!(validate(&gauge(150.0)).is_ok(), "out-of-range values clamp");
        let mut bad = gauge(50.0);
        bad.min = 100.0;
        assert!(validate(&bad).is_err());
        let mut bad = gauge(f64::NAN);
        bad.value = f64::NAN;
        assert!(validate(&bad).is_err());
        let mut bad = gauge(50.0);
        bad.bands[1].to = 20.0;
        assert!(validate(&bad).is_err(), "bands must ascend");
        let mut bad = gauge(50.0);
        bad.bands[1].to = 120.0;
        assert!(validate(&bad).is_err(), "bands stay inside max");
        let mut bad = gauge(50.0);
        bad.bands[0].color = "not-a-color".into();
        assert!(validate(&bad).is_err());
        let mut bad = gauge(50.0);
        bad.decimals = Some(11);
        assert!(validate(&bad).is_err());
        let mut bad = gauge(50.0);
        bad.min = -f64::MAX;
        bad.max = f64::MAX;
        assert!(validate(&bad).is_err(), "the span must be finite");
    }

    #[test]
    fn value_and_scale_text_are_minimal_and_exact() {
        let mut input = gauge(42.5);
        input.unit = "%".into();
        assert_eq!(value_text(&input), "42.5%");
        input.decimals = Some(2);
        assert_eq!(value_text(&input), "42.50%");
        assert_eq!(scale_text(0.1 + 0.2), "0.3");
        assert_eq!(scale_text(-0.0), "0");
    }
}
