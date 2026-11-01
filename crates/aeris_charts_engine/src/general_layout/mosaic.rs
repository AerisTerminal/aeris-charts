//! Marimekko (mosaic) layout: variable-width 100% stacked columns. Column widths are
//! proportional to column totals and segment heights to each segment's share of its column.

use std::collections::{HashMap, HashSet};

use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{Prim, TextAlign};

use super::{
    GeneralLayoutAccessibilityItem, GeneralLayoutElement, GeneralLayoutHit, LayoutSeriesEntry,
    add_text_bytes, invalid_data, layout_irect, validate_rows, validate_value,
};
use crate::general_axes::GeneralPlotRect;
use crate::{ChartEngine, ChartError, GeneralLayoutSeriesId};

const LEGEND_HEIGHT: f64 = 24.0;
const LABEL_HEIGHT: f64 = 20.0;
const SWATCH: f64 = 12.0;
const SWATCH_GAP: f64 = 4.0;
const LEGEND_ITEM_GAP: f64 = 12.0;
const TEXT_SIZE: f64 = 12.0;
/// ECharts 6 default categorical palette (`tokens.color.theme`). Segment 0 takes the series
/// color; later segments continue through this palette.
const PALETTE: [Color; 9] = [
    Color(0x5070_DDFF),
    Color(0xB6D6_34FF),
    Color(0x5053_72FF),
    Color(0xFF99_4DFF),
    Color(0x0CA8_DFFF),
    Color(0xFFD1_0AFF),
    Color(0xFB62_8BFF),
    Color(0x785D_B0FF),
    Color(0x3FBE_95FF),
];

/// One value of a two-category table: its column, its stacked segment, and its weight.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MosaicCell {
    pub column: String,
    pub segment: String,
    pub value: f64,
}

pub(super) fn validate(cells: &[MosaicCell]) -> Result<(), ChartError> {
    validate_rows(cells.len())?;
    let mut pairs = HashSet::with_capacity(cells.len());
    let mut bytes = 0usize;
    for cell in cells {
        if cell.column.is_empty() || cell.segment.is_empty() {
            return Err(invalid_data(
                "marimekko column and segment names must not be empty",
            ));
        }
        add_text_bytes(&mut bytes, &cell.column)?;
        add_text_bytes(&mut bytes, &cell.segment)?;
        validate_value(cell.value)?;
        if !pairs.insert((cell.column.as_str(), cell.segment.as_str())) {
            return Err(invalid_data("duplicate marimekko column and segment"));
        }
    }
    Ok(())
}

pub(super) fn heap_bytes(cells: &[MosaicCell]) -> usize {
    cells
        .iter()
        .map(|cell| cell.column.capacity() + cell.segment.capacity())
        .sum()
}

pub(super) fn segment_color(series_color: Color, segment: usize) -> Color {
    if segment == 0 {
        series_color
    } else {
        PALETTE[segment % PALETTE.len()]
    }
}

struct MosaicColumn {
    first_row: usize,
    total: f64,
    x: f64,
    width: f64,
}

/// Category order, column totals, and per-cell rectangles. Columns and segments keep their
/// first-appearance order; segments stack upward from the column base in that order.
pub(super) struct MosaicLayout {
    columns: Vec<MosaicColumn>,
    /// First input row of each segment, in segment order.
    segments: Vec<usize>,
    segment_of: Vec<usize>,
    shares: Vec<f64>,
    rects: Vec<Option<GeneralPlotRect>>,
}

impl MosaicLayout {
    pub(super) fn new(cells: &[MosaicCell], plot: GeneralPlotRect) -> Self {
        let mut column_index = HashMap::new();
        let mut segment_index = HashMap::new();
        let mut columns = Vec::new();
        let mut segments = Vec::new();
        let mut column_of = Vec::with_capacity(cells.len());
        let mut segment_of = Vec::with_capacity(cells.len());
        for (row, cell) in cells.iter().enumerate() {
            let column = *column_index.entry(cell.column.as_str()).or_insert_with(|| {
                columns.push(MosaicColumn {
                    first_row: row,
                    total: 0.0,
                    x: 0.0,
                    width: 0.0,
                });
                columns.len() - 1
            });
            columns[column].total = (columns[column].total + cell.value).min(f64::MAX);
            column_of.push(column);
            segment_of.push(
                *segment_index
                    .entry(cell.segment.as_str())
                    .or_insert_with(|| {
                        segments.push(row);
                        segments.len() - 1
                    }),
            );
        }
        let grand = columns
            .iter()
            .fold(0.0_f64, |sum, column| (sum + column.total).min(f64::MAX));
        let shares = cells
            .iter()
            .zip(&column_of)
            .map(|(cell, &column)| {
                let total = columns[column].total;
                if total > 0.0 { cell.value / total } else { 0.0 }
            })
            .collect::<Vec<_>>();
        let mut rects = vec![None; cells.len()];
        let body = GeneralPlotRect {
            x: plot.x,
            y: plot.y + LEGEND_HEIGHT,
            width: plot.width,
            height: plot.height - LEGEND_HEIGHT - LABEL_HEIGHT,
        };
        if grand > 0.0 && body.width > 0.0 && body.height > 0.0 {
            let mut x = body.x;
            for column in &mut columns {
                column.x = x;
                column.width = body.width * column.total / grand;
                x += column.width;
            }
            let mut order: Vec<usize> = (0..cells.len()).collect();
            order.sort_by_key(|&row| (column_of[row], segment_of[row]));
            let mut bases = vec![body.y + body.height; columns.len()];
            for row in order {
                let column = &columns[column_of[row]];
                if column.total <= 0.0 {
                    continue;
                }
                let height = body.height * shares[row];
                let base = &mut bases[column_of[row]];
                *base -= height;
                rects[row] = Some(GeneralPlotRect {
                    x: column.x,
                    y: *base,
                    width: column.width,
                    height,
                });
            }
        }
        Self {
            columns,
            segments,
            segment_of,
            shares,
            rects,
        }
    }

    pub(super) fn hit(&self, x: f64, y: f64) -> Option<usize> {
        self.rects.iter().position(|rect| {
            rect.is_some_and(|rect| {
                rect.width > 0.0
                    && rect.height > 0.0
                    && x >= rect.x
                    && x < rect.x + rect.width
                    && y >= rect.y
                    && y < rect.y + rect.height
            })
        })
    }

    pub(super) fn anchor(&self, row: usize) -> Option<(f64, f64)> {
        self.rects
            .get(row)
            .copied()
            .flatten()
            .filter(|rect| rect.width > 0.0 && rect.height > 0.0)
            .map(|rect| (rect.x + rect.width * 0.5, rect.y + rect.height * 0.5))
    }

    pub(super) fn share(&self, row: usize) -> f64 {
        self.shares.get(row).copied().unwrap_or(0.0)
    }

    pub(super) fn color(&self, series_color: Color, row: usize) -> Color {
        segment_color(series_color, self.segment_of.get(row).copied().unwrap_or(0))
    }
}

/// Bounds used when only category order and shares are needed.
pub(super) const UNIT_PLOT: GeneralPlotRect = GeneralPlotRect {
    x: 0.0,
    y: 0.0,
    width: 1.0,
    height: 1.0,
};

impl ChartEngine {
    pub(super) fn mosaic_accessibility_items(
        &self,
        series: GeneralLayoutSeriesId,
        cells: &[MosaicCell],
        selected: Option<GeneralLayoutHit>,
        offset: usize,
        limit: usize,
    ) -> Vec<GeneralLayoutAccessibilityItem> {
        let layout = MosaicLayout::new(cells, UNIT_PLOT);
        cells
            .iter()
            .enumerate()
            .skip(offset)
            .take(limit)
            .map(|(row, cell)| GeneralLayoutAccessibilityItem {
                element: GeneralLayoutElement::Cell,
                row,
                id: format!("{}/{}", cell.column, cell.segment),
                label: format!(
                    "{}, {}, {:.1}% of column",
                    cell.column,
                    cell.segment,
                    layout.share(row) * 100.0
                ),
                value: Some(cell.value),
                selected: selected
                    == Some(GeneralLayoutHit {
                        series,
                        element: GeneralLayoutElement::Cell,
                        row,
                    }),
            })
            .collect()
    }

    pub(super) fn build_mosaic_frame(
        &self,
        series: &LayoutSeriesEntry,
        cells: &[MosaicCell],
        plot: GeneralPlotRect,
        (hpr, vpr): (f64, f64),
        out: &mut Vec<Prim>,
    ) {
        let layout = MosaicLayout::new(cells, plot);
        let options = self.options.get();
        let family = &options.layout.font_family;
        let surface = self.chart_surface_color();
        let text_color = self.primary_text_color();
        let stroke = hpr.min(vpr).round().max(1.0) as i32;
        let text = |text: String, x: f64, y: f64, align: TextAlign| Prim::Text {
            x: (x * hpr) as f32,
            y: (y * vpr) as f32,
            text,
            color: text_color,
            size: (TEXT_SIZE * vpr) as f32,
            family: family.clone(),
            align,
            weight: 400,
            italic: false,
        };
        for (row, rect) in layout.rects.iter().enumerate() {
            let Some(bounds) = rect else {
                continue;
            };
            let Some(rect) = layout_irect(*bounds, hpr, vpr) else {
                continue;
            };
            let fill = layout.color(series.color, row);
            out.push(Prim::Rect { rect, color: fill });
            out.push(Prim::RectFrame {
                rect,
                border: stroke,
                color: surface,
            });
            if series.options.data_labels {
                self.push_general_layout_label(
                    out,
                    &format!("{:.0}%", layout.share(row) * 100.0),
                    *bounds,
                    fill,
                    hpr,
                    vpr,
                );
            }
        }
        for column in &layout.columns {
            if column.width <= 0.0 || column.total <= 0.0 {
                continue;
            }
            let label = &cells[column.first_row].column;
            if self.measure_text_run(label, TEXT_SIZE, family, 400, false) + 4.0 <= column.width {
                out.push(text(
                    label.clone(),
                    column.x + column.width * 0.5,
                    plot.y + plot.height - LABEL_HEIGHT * 0.5,
                    TextAlign::Center,
                ));
            }
        }
        let mut x = plot.x;
        for (segment, &row) in layout.segments.iter().enumerate() {
            let name = &cells[row].segment;
            let width = self.measure_text_run(name, TEXT_SIZE, family, 400, false);
            if x + SWATCH + SWATCH_GAP + width > plot.x + plot.width {
                break;
            }
            if let Some(rect) = layout_irect(
                GeneralPlotRect {
                    x,
                    y: plot.y + (LEGEND_HEIGHT - SWATCH) * 0.5,
                    width: SWATCH,
                    height: SWATCH,
                },
                hpr,
                vpr,
            ) {
                out.push(Prim::Rect {
                    rect,
                    color: segment_color(series.color, segment),
                });
            }
            out.push(text(
                name.clone(),
                x + SWATCH + SWATCH_GAP,
                plot.y + LEGEND_HEIGHT * 0.5,
                TextAlign::Left,
            ));
            x += SWATCH + SWATCH_GAP + width + LEGEND_ITEM_GAP;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(column: &str, segment: &str, value: f64) -> MosaicCell {
        MosaicCell {
            column: column.into(),
            segment: segment.into(),
            value,
        }
    }

    const PLOT: GeneralPlotRect = GeneralPlotRect {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 244.0,
    };

    #[test]
    fn widths_follow_column_totals_and_heights_follow_shares() {
        let cells = [
            cell("A", "x", 30.0),
            cell("A", "y", 10.0),
            cell("B", "y", 45.0),
            cell("B", "x", 15.0),
        ];
        let layout = MosaicLayout::new(&cells, PLOT);
        let rect = |row: usize| layout.rects[row].unwrap();
        assert!((rect(0).width - 160.0).abs() < 1e-9);
        assert!((rect(2).width - 240.0).abs() < 1e-9);
        assert!((rect(2).x - 160.0).abs() < 1e-9);
        let body = PLOT.height - LEGEND_HEIGHT - LABEL_HEIGHT;
        assert!((rect(0).height - body * 0.75).abs() < 1e-9);
        assert!((rect(1).height - body * 0.25).abs() < 1e-9);
        // Segment "x" stacks first (at the base) in both columns despite B's input order.
        assert!((rect(0).y + rect(0).height - (LEGEND_HEIGHT + body)).abs() < 1e-9);
        assert!((rect(3).y + rect(3).height - (LEGEND_HEIGHT + body)).abs() < 1e-9);
        assert!((layout.share(3) - 0.25).abs() < 1e-12);
        assert_eq!(layout.hit(10.0, rect(1).y + 1.0), Some(1));
        assert_eq!(layout.hit(200.0, rect(3).y + 1.0), Some(3));
        assert_eq!(layout.color(PALETTE[0], 2), PALETTE[1]);
    }

    #[test]
    fn zero_totals_get_no_area_and_no_hits() {
        let cells = [
            cell("A", "x", 0.0),
            cell("B", "x", 5.0),
            cell("B", "y", 5.0),
        ];
        let layout = MosaicLayout::new(&cells, PLOT);
        assert!(layout.rects[0].is_none());
        assert_eq!(layout.share(0), 0.0);
        assert!((layout.rects[1].unwrap().width - 400.0).abs() < 1e-9);
        let empty = MosaicLayout::new(&[cell("A", "x", 0.0)], PLOT);
        assert!(empty.rects.iter().all(Option::is_none));
        assert_eq!(empty.hit(10.0, 100.0), None);
        assert_eq!(empty.anchor(0), None);
    }

    #[test]
    fn mosaic_validation_rejects_duplicates_negatives_and_blank_names() {
        assert!(validate(&[cell("A", "x", 1.0), cell("A", "y", 2.0)]).is_ok());
        assert!(validate(&[cell("A", "x", 1.0), cell("A", "x", 2.0)]).is_err());
        assert!(validate(&[cell("A", "x", -1.0)]).is_err());
        assert!(validate(&[cell("A", "x", f64::NAN)]).is_err());
        assert!(validate(&[cell("", "x", 1.0)]).is_err());
    }
}
