//! Backend-neutral resolved drawing geometry.
//!
//! A tool's semantic anchors are converted once into this small geometry vocabulary.  Frame
//! lowering and precise hit-testing both consume the same result, preventing the rendered shape
//! and the interactive shape from drifting as more drawing kinds are added.

use nucleuscharts_render::draw_list::LineType;

use super::{path_arrow_points, DrawingKind, TextBox};

#[derive(Clone, Copy, Debug)]
pub(crate) enum DrawingBodyGeometry<'a> {
    Empty,
    Segment {
        a: (f64, f64),
        b: (f64, f64),
    },
    Horizontal {
        y: f64,
        x0: f64,
        x1: f64,
    },
    Vertical {
        x: f64,
        y0: f64,
        y1: f64,
    },
    Rectangle {
        left: f64,
        right: f64,
        top: f64,
        bottom: f64,
    },
    Polyline {
        points: &'a [(f64, f64)],
        line_type: LineType,
        terminal: Option<[(f64, f64); 3]>,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResolvedDrawingGeometry<'a> {
    pub(crate) body: DrawingBodyGeometry<'a>,
    pub(crate) text_box: TextBox,
}

fn points_box(px: &[(f64, f64)]) -> Option<TextBox> {
    let &(first_x, first_y) = px.first()?;
    let (mut left, mut right, mut top, mut bottom) = (first_x, first_x, first_y, first_y);
    for &(x, y) in &px[1..] {
        left = left.min(x);
        right = right.max(x);
        top = top.min(y);
        bottom = bottom.max(y);
    }
    Some(TextBox {
        left,
        right,
        top,
        bottom,
    })
}

pub(crate) fn resolve_drawing_geometry<'a>(
    kind: DrawingKind,
    px: &'a [(f64, f64)],
    pane_w: f64,
    pane_top: f64,
    pane_h: f64,
    line_width: f64,
    device_scale: f64,
) -> Option<ResolvedDrawingGeometry<'a>> {
    if px.is_empty() {
        return None;
    }
    let body = match kind {
        DrawingKind::TrendLine => DrawingBodyGeometry::Segment {
            a: *px.first()?,
            b: *px.get(1)?,
        },
        DrawingKind::HorizontalLine => DrawingBodyGeometry::Horizontal {
            y: px[0].1,
            x0: 0.0,
            x1: pane_w,
        },
        DrawingKind::HorizontalRay => DrawingBodyGeometry::Horizontal {
            y: px[0].1,
            x0: px[0].0,
            x1: pane_w,
        },
        DrawingKind::VerticalLine => DrawingBodyGeometry::Vertical {
            x: px[0].0,
            y0: pane_top,
            y1: pane_top + pane_h,
        },
        DrawingKind::Rectangle => {
            let (a, b) = (*px.first()?, *px.get(1)?);
            DrawingBodyGeometry::Rectangle {
                left: a.0.min(b.0),
                right: a.0.max(b.0),
                top: a.1.min(b.1),
                bottom: a.1.max(b.1),
            }
        }
        DrawingKind::Text => DrawingBodyGeometry::Empty,
        DrawingKind::Brush => DrawingBodyGeometry::Polyline {
            points: px,
            line_type: LineType::Curved,
            terminal: None,
        },
        DrawingKind::Path => DrawingBodyGeometry::Polyline {
            points: px,
            line_type: LineType::Simple,
            terminal: path_arrow_points(px, line_width, device_scale),
        },
    };

    let text_box = match body {
        DrawingBodyGeometry::Empty => {
            let (x, y) = *px.first()?;
            TextBox {
                left: x,
                right: x,
                top: y,
                bottom: y,
            }
        }
        DrawingBodyGeometry::Segment { a, b } => TextBox {
            left: a.0.min(b.0),
            right: a.0.max(b.0),
            top: a.1.min(b.1),
            bottom: a.1.max(b.1),
        },
        DrawingBodyGeometry::Horizontal { y, x0, x1 } => TextBox {
            // Preserve the semantic direction of a ray. A right ray anchored beyond the pane may
            // intentionally have `left > right`; text placement historically uses that oriented
            // reference rather than normalizing it into a finite segment.
            left: x0,
            right: x1,
            top: y,
            bottom: y,
        },
        DrawingBodyGeometry::Vertical { x, y0, y1 } => TextBox {
            left: x,
            right: x,
            top: y0.min(y1),
            bottom: y0.max(y1),
        },
        DrawingBodyGeometry::Rectangle {
            left,
            right,
            top,
            bottom,
        } => TextBox {
            left,
            right,
            top,
            bottom,
        },
        DrawingBodyGeometry::Polyline { points, .. } => points_box(points)?,
    };
    Some(ResolvedDrawingGeometry { body, text_box })
}
