use std::borrow::Cow;

use super::*;
use crate::footprint::{
    FootprintAggregationOptions, FootprintBar, FootprintCellMode, FootprintLevel,
    footprint_row_merge, footprint_row_price_bounds, horizontal_imbalance_bar,
    merged_footprint_bar, merged_poc_price,
};

/// Bars at least this wide (CSS px) print cell numbers.
const DETAIL_MIN_SPACING: f64 = 48.0;
/// Bars at least this wide draw volume cells; narrower bars draw a range summary.
const CELLS_MIN_SPACING: f64 = 8.0;
/// Vertical room (CSS px) a printed row keeps around its glyphs for row rounding and the row gap.
pub(crate) const DETAIL_ROW_PADDING: f64 = 2.0;
/// A printed number needs this many font sizes of cell height.
const TEXT_HEIGHT: f64 = 1.05;
/// Dense rows shrink cell numbers down to this size (CSS px) before numbers give way to cells.
const MIN_CELL_FONT: f64 = 8.0;
/// Display rows shorter than this (CSS px) merge when only cells draw.
const CELLS_MIN_ROW: f64 = 3.0;
/// Autoscale keeps a stored row at most this many font sizes tall, so a narrow-range bar stays a
/// compact column of numbers instead of stretching across the pane.
pub(crate) const ROW_HEIGHT_CAP: f64 = 1.6;
/// Average advance of a compact volume glyph relative to the font size.
const GLYPH_ADVANCE: f32 = 0.62;
/// Share of the bar slot the cluster occupies; the rest separates neighboring bars.
const CLUSTER_WIDTH: f64 = 0.9;
/// Share of the cell area the per-row volume histogram takes beside the printed numbers.
const HISTOGRAM_SHARE: f64 = 0.26;
/// Default histogram opacity: rows encode volume by length, so one opacity keeps the profile
/// calm behind prints.
pub(crate) const HISTOGRAM_ALPHA: u8 = 170;
/// Default tint of a single imbalance's half cell; stacked runs are opaque.
pub(crate) const SINGLE_IMBALANCE_ALPHA: u8 = 150;
/// Gap (CSS px) between a printed number and the bid × ask divider.
const NUMBER_PAD: f64 = 2.0;
/// Bar-summary text size relative to the font size.
const SUMMARY_FONT: f64 = 0.9;
/// Line advance of the two bar-summary lines relative to their text size.
const SUMMARY_LINE: f64 = 1.2;
/// Gap (CSS px) between a cluster's low row and its summary.
const SUMMARY_GAP: f64 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FootprintLod {
    Detailed,
    Cells,
    Summary,
}

#[derive(Clone, Copy)]
struct FootprintTextStyle<'a> {
    fallback_color: Color,
    explicit_text: bool,
    surface: Color,
    cell_fill: Color,
    size: f32,
    pad: f32,
    family: &'a str,
    weight: u16,
    strong_weight: u16,
}

/// Imbalance flags as drawn: all off when the host hides imbalance highlights, so every cell
/// mode drops the tint and the strong weight together while the stored truth stays unchanged.
#[derive(Clone, Copy)]
struct DrawnImbalance {
    bid: bool,
    ask: bool,
    stacked_bid: bool,
    stacked_ask: bool,
}

impl DrawnImbalance {
    fn of(level: &FootprintLevel, visual: &crate::FootprintVisualOptions) -> Self {
        let shown = visual.show_imbalances;
        Self {
            bid: shown && (level.bid_imbalance || level.stacked_bid_imbalance),
            ask: shown && (level.ask_imbalance || level.stacked_ask_imbalance),
            stacked_bid: shown && level.stacked_bid_imbalance,
            stacked_ask: shown && level.stacked_ask_imbalance,
        }
    }

    const fn any(self) -> bool {
        self.bid || self.ask
    }
}

fn opacity_alpha(opacity: f64) -> u8 {
    (opacity.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Device-px column of one cluster: the range strip on the left edge, the printed numbers, then
/// the per-row volume histogram on the right.
#[derive(Clone, Copy)]
struct ClusterColumn {
    left: i32,
    right: i32,
    strip_width: i32,
    cells_left: i32,
    /// Right edge of the printed numbers; equals `right` when no histogram draws.
    numbers_right: i32,
    /// Left edge of the histogram; equals `right` when no histogram draws.
    histogram_left: i32,
}

impl ClusterColumn {
    fn new(x: f64, spacing: f64, hpr: f64, histogram: bool) -> Self {
        let half = spacing * CLUSTER_WIDTH / 2.0;
        let left = ((x - half) * hpr).round() as i32;
        let right = (((x + half) * hpr).round() as i32).max(left + 1);
        let strip_width = (3.0 * hpr).round().max(2.0) as i32;
        let gap = (2.0 * hpr).round().max(1.0) as i32;
        let cells_left = (left + strip_width + gap).min(right - 1);
        let (numbers_right, histogram_left) = if histogram {
            let width = (f64::from(right - cells_left) * HISTOGRAM_SHARE).round() as i32;
            (right - width - gap, right - width)
        } else {
            (right, right)
        };
        Self {
            left,
            right,
            strip_width,
            cells_left,
            numbers_right,
            histogram_left,
        }
    }

    /// Device-px cell width left of the histogram for a cluster `spacing` CSS px wide.
    fn numbers_width(spacing: f64, hpr: f64, histogram: bool) -> i32 {
        let column = Self::new(0.0, spacing, hpr, histogram);
        column.numbers_right - column.cells_left
    }

    const fn cells_width(self) -> i32 {
        self.right - self.cells_left
    }
}

/// CSS px row height that prints `font` px cell numbers.
pub(crate) fn legible_row_px(font: f64) -> f64 {
    font * TEXT_HEIGHT + DETAIL_ROW_PADDING
}

/// CSS px the two-line bar summary takes under a cluster, or 0 when it is off or the pane is too
/// short to spare it. Autoscale reserves the same height below the lowest bar so its summary stays
/// on screen.
pub(crate) fn footprint_summary_height(
    visual: &crate::FootprintVisualOptions,
    pane_height: f64,
) -> f64 {
    if !visual.show_bar_summary {
        return 0.0;
    }
    let height = (SUMMARY_GAP + 2.0 * visual.font_size * SUMMARY_FONT * SUMMARY_LINE).ceil();
    if pane_height >= height * 4.0 {
        height
    } else {
        0.0
    }
}

/// The `Δ delta` and `V volume` lines printed under a cluster.
fn summary_lines(bar: &FootprintBar) -> [String; 2] {
    [
        format!("Δ {}", compact_volume(bar.delta)),
        format!("V {}", compact_volume(bar.total_volume)),
    ]
}

impl ChartEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_footprint_series_frame(
        &self,
        rs: ResolvedSeries,
        from: i64,
        to: i64,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        scale: &PriceScaleCore,
    ) {
        let Some(state) = self
            .series_entry(rs.id)
            .and_then(|series| series.footprint.as_ref())
        else {
            return;
        };
        let Some(stream) = self.trade_stream(state.trade_stream_id) else {
            return;
        };
        let start = out.len();
        let visual = &state.visual;
        let plot = self.data.plot(rs.id);
        let spacing = self.time_scale.bar_spacing();
        let font_size = visual.font_size;
        let stored = stream.options();
        let bars = plot
            .visible_rows(from, to)
            .filter_map(|row| {
                let bar = stream.bars().get(row)?;
                let logical = plot.index_at(row)?;
                Some((row, self.time_scale.index_to_coordinate(logical), bar))
            })
            .collect::<Vec<_>>();
        // One merge factor per frame keeps rows aligned across every visible bar.
        let stored_row_px = bars.last().map_or(0.0, |(_, _, bar)| {
            let lower = scale.price_to_coordinate(bar.close, rs.base_value);
            let upper = scale.price_to_coordinate(bar.close + stored.row_size(), rs.base_value);
            (lower - upper).abs()
        });
        let wide = spacing >= DETAIL_MIN_SPACING;
        // Merged rows keep the configured font; unmerged dense rows shrink it instead.
        let merge = if visual.adaptive_rows {
            let minimum = if wide {
                legible_row_px(font_size)
            } else {
                CELLS_MIN_ROW
            };
            footprint_row_merge(stored_row_px, minimum)
        } else {
            1
        };
        let aggregation = FootprintAggregationOptions {
            ticks_per_row: stored.ticks_per_row.saturating_mul(merge),
            ..stored
        };
        let row_px = stored_row_px * f64::from(merge);
        let cell_font = font_size.min((row_px - DETAIL_ROW_PADDING) / TEXT_HEIGHT);
        let mut lod = if wide && cell_font >= MIN_CELL_FONT.min(font_size) {
            FootprintLod::Detailed
        } else if spacing >= CELLS_MIN_SPACING && row_px * vpr >= 2.0 {
            FootprintLod::Cells
        } else {
            FootprintLod::Summary
        };

        let layout = &self.options.get().layout;
        let family = visual
            .font_family
            .clone()
            .unwrap_or_else(|| layout.font_family.clone());
        let foreground = Color::parse_css(&layout.text_color).unwrap_or(Color::rgb(215, 219, 228));
        let fallback_text = visual.text_color.unwrap_or(foreground);
        let surface = Color::parse_css(&layout.background.color).unwrap_or(Color::rgb(
            aeris_charts_core::style::DEFAULT_SURFACE_RGB.0,
            aeris_charts_core::style::DEFAULT_SURFACE_RGB.1,
            aeris_charts_core::style::DEFAULT_SURFACE_RGB.2,
        ));
        // Light cells use `--surface-raised`: the secondary surface is too close to white to read
        // as a tile. Dark cells use the opaque `--surface-secondary`, already distinct from the chart.
        let cell_fill = visual.cell_color.unwrap_or_else(|| {
            let (r, g, b) = if surface.luminance() > 160.0 {
                aeris_charts_core::style::LIGHT_RAISED_RGB
            } else {
                aeris_charts_core::style::DARK_MUTED_RGB
            };
            Color::rgb(r, g, b)
        });
        let text_style = FootprintTextStyle {
            fallback_color: fallback_text,
            explicit_text: visual.text_color.is_some(),
            surface,
            cell_fill,
            size: (font_size * vpr) as f32,
            pad: (NUMBER_PAD * hpr).round().max(1.0) as f32,
            family: &family,
            weight: visual.text_weight,
            strong_weight: visual.strong_text_weight,
        };
        let cell_style = FootprintTextStyle {
            size: (cell_font.max(1.0) * vpr) as f32,
            ..text_style
        };
        let poc_color = visual.poc_color.solid();

        if lod == FootprintLod::Summary {
            for (_, x, source) in &bars {
                let poc_price = merged_poc_price(source, stored.ticks_per_row, stored.row_size());
                push_range_summary(
                    out,
                    source,
                    poc_price,
                    ClusterColumn::new(*x, spacing, hpr, false),
                    scale,
                    rs.base_value,
                    vpr,
                    visual,
                    poc_color,
                    &stored,
                );
            }
        } else {
            // Stored levels are per tick; display rows group `ticks_per_row` of them.
            let presented = bars
                .iter()
                .map(|(_, x, source)| {
                    let mut bar = if aggregation.ticks_per_row > 1 {
                        Cow::Owned(merged_footprint_bar(
                            source,
                            aggregation.ticks_per_row,
                            stored.imbalance,
                            aggregation.row_size(),
                        ))
                    } else {
                        Cow::Borrowed(*source)
                    };
                    if visual.cell_mode == FootprintCellMode::HorizontalImbalance {
                        bar = Cow::Owned(horizontal_imbalance_bar(&bar, stored.imbalance));
                    }
                    (*x, bar)
                })
                .collect::<Vec<_>>();
            let numbers_bars = matches!(
                visual.cell_mode,
                FootprintCellMode::BidAsk
                    | FootprintCellMode::HorizontalImbalance
                    | FootprintCellMode::VolumeLadder
            );
            // Zooming out drops the histogram first, then the numbers, so a printed value is
            // never truncated: one decision per frame keeps every cluster on the same layout.
            let mut histogram = false;
            if lod == FootprintLod::Detailed && numbers_bars {
                let widest = presented
                    .iter()
                    .flat_map(|(_, bar)| &bar.levels)
                    .flat_map(|level| [level.bid_volume, level.ask_volume])
                    .filter(|volume| *volume > 0.0)
                    .map(|volume| compact_volume(volume).chars().count())
                    .max()
                    .unwrap_or(0);
                let half_needed =
                    widest as f32 * GLYPH_ADVANCE * cell_style.size + 2.0 * cell_style.pad + 2.0;
                let fits = |histogram: bool| {
                    ClusterColumn::numbers_width(spacing, hpr, histogram) as f32 / 2.0
                        >= half_needed
                };
                if fits(true) {
                    histogram = true;
                } else if !fits(false) {
                    lod = FootprintLod::Cells;
                }
            }
            // The summary prints with the numbers, and on every cluster or none of them.
            let summary_style = FootprintTextStyle {
                size: (font_size * SUMMARY_FONT * vpr) as f32,
                ..text_style
            };
            let summary = lod == FootprintLod::Detailed
                && footprint_summary_height(visual, scale.height()) > 0.0
                && {
                    let column = ClusterColumn::new(0.0, spacing, hpr, false);
                    let widest = presented
                        .iter()
                        .flat_map(|(_, bar)| summary_lines(bar))
                        .map(|line| text_width(&line, &summary_style))
                        .fold(0.0_f32, f32::max);
                    widest + 2.0 * summary_style.pad <= (column.right - column.left) as f32
                };
            for (x, bar) in &presented {
                let column = ClusterColumn::new(*x, spacing, hpr, histogram);
                push_bar_frame(
                    out,
                    bar,
                    column,
                    lod,
                    scale,
                    rs.base_value,
                    hpr,
                    vpr,
                    visual,
                    &aggregation,
                    &cell_style,
                    poc_color,
                );
                if summary {
                    push_bar_summary(
                        out,
                        bar,
                        column,
                        scale,
                        rs.base_value,
                        vpr,
                        visual,
                        &aggregation,
                        &summary_style,
                    );
                }
            }
        }
        // Fills, then outlines, then text: clusters never overlap, so this layering paints the
        // same pixels while letting batching executors draw the series in a few calls instead of
        // one per alternating fill and number. The sort is stable within each layer.
        out[start..].sort_by_key(|primitive| match primitive {
            Prim::Rect { .. } => 0_u8,
            Prim::Text { .. } => 2,
            _ => 1,
        });
    }
}

#[derive(Clone, Copy)]
struct CellRect {
    top: i32,
    height: i32,
}

#[derive(Clone, Copy)]
struct VolumeMaxima {
    bid: f64,
    ask: f64,
    total: f64,
    abs_delta: f64,
}

fn bar_volume_maxima(bar: &FootprintBar) -> VolumeMaxima {
    let mut maxima = VolumeMaxima {
        bid: 0.0,
        ask: 0.0,
        total: 0.0,
        abs_delta: 0.0,
    };
    for level in &bar.levels {
        maxima.bid = maxima.bid.max(level.bid_volume);
        maxima.ask = maxima.ask.max(level.ask_volume);
        maxima.total = maxima.total.max(level.total_volume);
        maxima.abs_delta = maxima.abs_delta.max(level.delta.abs());
    }
    maxima
}

/// Device-px row between two prices and its full height before the row gap. A 1px gap separates
/// stacked rows without a separator primitive per row.
fn row_cell(
    scale: &PriceScaleCore,
    base_value: f64,
    vpr: f64,
    lower_price: f64,
    upper_price: f64,
) -> (CellRect, i32) {
    let upper = scale.price_to_coordinate(upper_price, base_value);
    let lower = scale.price_to_coordinate(lower_price, base_value);
    let top = (upper.min(lower) * vpr).round() as i32;
    let bottom = (upper.max(lower) * vpr).round() as i32;
    let full_height = (bottom - top).max(1);
    let height = if full_height > 2 {
        full_height - 1
    } else {
        full_height
    };
    (CellRect { top, height }, full_height)
}

/// One presented cluster: the range strip, every row in the active cell mode, and the POC.
#[allow(clippy::too_many_arguments)]
fn push_bar_frame(
    out: &mut Vec<Prim>,
    bar: &FootprintBar,
    column: ClusterColumn,
    lod: FootprintLod,
    scale: &PriceScaleCore,
    base_value: f64,
    hpr: f64,
    vpr: f64,
    visual: &crate::FootprintVisualOptions,
    aggregation: &FootprintAggregationOptions,
    style: &FootprintTextStyle<'_>,
    poc_color: Color,
) {
    if visual.show_range_strip {
        push_range_strip(
            out,
            bar,
            column,
            scale,
            base_value,
            vpr,
            visual,
            aggregation,
        );
    }
    let maxima = bar_volume_maxima(bar);
    let detailed = lod == FootprintLod::Detailed;
    let text = detailed.then_some(style);
    let numbers_bars = matches!(
        visual.cell_mode,
        FootprintCellMode::BidAsk
            | FootprintCellMode::HorizontalImbalance
            | FootprintCellMode::VolumeLadder
    );
    for level in &bar.levels {
        let (lower_price, upper_price) =
            footprint_row_price_bounds(aggregation, level.price, level.price);
        let (cell, full_height) = row_cell(scale, base_value, vpr, lower_price, upper_price);
        let mut poc_right = column.right;
        match visual.cell_mode {
            FootprintCellMode::BidAsk
            | FootprintCellMode::HorizontalImbalance
            | FootprintCellMode::VolumeLadder
                if detailed =>
            {
                // The tile backs the printed numbers only; the histogram stays on the chart surface.
                out.push(Prim::Rect {
                    rect: IRect {
                        x: column.cells_left,
                        y: cell.top,
                        w: (column.numbers_right - column.cells_left).max(1),
                        h: cell.height,
                    },
                    color: style.cell_fill,
                });
                let bid_raise = if visual.cell_mode == FootprintCellMode::VolumeLadder {
                    full_height / 2
                } else {
                    0
                };
                push_bid_ask_row(out, level, column, cell, bid_raise, visual, style);
                if column.histogram_left < column.right {
                    push_histogram_row(
                        out,
                        level,
                        column.histogram_left,
                        column.right,
                        cell,
                        maxima.total,
                        visual,
                        false,
                    );
                }
                poc_right = column.numbers_right;
            }
            _ if numbers_bars => push_histogram_row(
                out,
                level,
                column.cells_left,
                column.right,
                cell,
                maxima.total,
                visual,
                true,
            ),
            FootprintCellMode::BidAskHistogram => {
                push_bid_ask_histogram_row(out, level, column, cell, maxima, visual, text);
            }
            FootprintCellMode::ProfileInBar => push_profile_row(
                out,
                level,
                column,
                cell,
                maxima,
                visual,
                style.fallback_color,
                text,
            ),
            _ => push_single_value_row(out, level, column, cell, maxima, visual, text),
        }
        if visual.show_poc && level.level == bar.poc_level {
            let border = (visual.poc_width * hpr).round().max(1.0) as f32;
            push_poc_outline(out, column.cells_left, poc_right, cell, border, poc_color);
        }
    }
}

/// Traded-range strip on the cluster's left edge: one segment per price row from low to high in
/// the bar-direction color, stepping with the cells and keeping their row gap, so the strip reads
/// as a dashed staircase rather than a candle.
#[allow(clippy::too_many_arguments)]
fn push_range_strip(
    out: &mut Vec<Prim>,
    bar: &FootprintBar,
    column: ClusterColumn,
    scale: &PriceScaleCore,
    base_value: f64,
    vpr: f64,
    visual: &crate::FootprintVisualOptions,
    aggregation: &FootprintAggregationOptions,
) {
    let edge_a = scale.coordinate_to_price(scale.pane_offset(), base_value);
    let edge_b = scale.coordinate_to_price(scale.pane_offset() + scale.height(), base_value);
    if !edge_a.is_finite() || !edge_b.is_finite() {
        return;
    }
    // Only on-screen rows draw: a sparse bar can span far more rows than it has levels.
    let first = aggregation
        .row_level(bar.low)
        .max(aggregation.row_level(edge_a.min(edge_b)));
    let last = aggregation
        .row_level(bar.high)
        .min(aggregation.row_level(edge_a.max(edge_b)));
    let color = if bar.close >= bar.open {
        visual.range_up_color.unwrap_or(visual.ask_color)
    } else {
        visual.range_down_color.unwrap_or(visual.bid_color)
    }
    .solid();
    let row_size = aggregation.row_size();
    let half_tick = aggregation.tick_size / 2.0;
    for row in first..=last {
        let lower = row as f64 * row_size - half_tick;
        let (cell, _) = row_cell(scale, base_value, vpr, lower, lower + row_size);
        out.push(Prim::Rect {
            rect: IRect {
                x: column.left,
                y: cell.top,
                w: column.strip_width,
                h: cell.height,
            },
            color,
        });
    }
}

/// Zoomed-out bar: the traded range in the bar-delta color with its POC marked.
#[allow(clippy::too_many_arguments)]
fn push_range_summary(
    out: &mut Vec<Prim>,
    bar: &FootprintBar,
    poc_price: f64,
    column: ClusterColumn,
    scale: &PriceScaleCore,
    base_value: f64,
    vpr: f64,
    visual: &crate::FootprintVisualOptions,
    poc_color: Color,
    aggregation: &FootprintAggregationOptions,
) {
    let (low_price, high_price) = footprint_row_price_bounds(aggregation, bar.low, bar.high);
    let high = scale.price_to_coordinate(high_price, base_value);
    let low = scale.price_to_coordinate(low_price, base_value);
    let top = (high.min(low) * vpr).round() as i32;
    let bottom = (high.max(low) * vpr).round() as i32;
    let body = if bar.delta >= 0.0 {
        visual.positive_delta_color.solid()
    } else {
        visual.negative_delta_color.solid()
    };
    let width = (column.right - column.left).max(1);
    out.push(Prim::Rect {
        rect: IRect {
            x: column.left,
            y: top,
            w: width,
            h: (bottom - top).max(1),
        },
        color: body,
    });
    if !visual.show_poc {
        return;
    }
    let poc_y = (scale.price_to_coordinate(poc_price, base_value) * vpr).round() as i32;
    out.push(Prim::Rect {
        rect: IRect {
            x: column.left,
            y: poc_y - 1,
            w: width,
            h: ((visual.poc_width * vpr).round().max(1.0) as i32 + 1).max(2),
        },
        color: poc_color,
    });
}

/// One `bid × ask` row: bids right-aligned and asks left-aligned against the center divider, on
/// the bare surface unless the side is imbalanced. The bid half is drawn `bid_raise` device px
/// above the row (the staggered volume ladder); zero keeps it level.
fn push_bid_ask_row(
    out: &mut Vec<Prim>,
    level: &FootprintLevel,
    column: ClusterColumn,
    cell: CellRect,
    bid_raise: i32,
    visual: &crate::FootprintVisualOptions,
    style: &FootprintTextStyle<'_>,
) {
    let width = (column.numbers_right - column.cells_left).max(2);
    let split = column.cells_left + width / 2;
    let ask_left = (split + i32::from(width > 3)).min(column.numbers_right);
    let drawn = DrawnImbalance::of(level, visual);
    let halves = [
        (
            column.cells_left,
            split - column.cells_left,
            cell.top - bid_raise,
            level.bid_volume,
            drawn.bid,
            drawn.stacked_bid,
            visual.stacked_bid_color,
            TextAlign::Right,
        ),
        (
            ask_left,
            column.numbers_right - ask_left,
            cell.top,
            level.ask_volume,
            drawn.ask,
            drawn.stacked_ask,
            visual.stacked_ask_color,
            TextAlign::Left,
        ),
    ];
    for (left, half_width, top, volume, imbalanced, stacked, imbalance_color, align) in halves {
        let half_width = half_width.max(1);
        let mut background = Color::rgba(0, 0, 0, 0);
        if imbalanced {
            background = imbalance_cell(imbalance_color, stacked, visual);
            out.push(Prim::Rect {
                rect: IRect {
                    x: left,
                    y: top,
                    w: half_width,
                    h: cell.height,
                },
                color: background,
            });
        }
        if volume <= 0.0 {
            continue;
        }
        let x = match align {
            TextAlign::Right => (left + half_width) as f32 - style.pad,
            _ => left as f32 + style.pad,
        };
        push_cell_text(
            out,
            x,
            top as f32 + cell.height as f32 / 2.0,
            align,
            half_width - style.pad as i32,
            cell.height,
            compact_volume(volume),
            style,
            contrast_on(background, style),
            imbalanced,
        );
    }
}

/// One volume-histogram row: total volume from `left`, in the dominant side's hue. Without
/// printed numbers the row also carries the imbalance signal as an opaque fill.
#[allow(clippy::too_many_arguments)]
fn push_histogram_row(
    out: &mut Vec<Prim>,
    level: &FootprintLevel,
    left: i32,
    right: i32,
    cell: CellRect,
    peak: f64,
    visual: &crate::FootprintVisualOptions,
    mark_imbalance: bool,
) {
    let width = profile_width(right - left, level.total_volume, peak);
    if width == 0 {
        return;
    }
    let buy = level.ask_volume >= level.bid_volume;
    let drawn = DrawnImbalance::of(level, visual);
    let (ask_imbalanced, bid_imbalanced) = (drawn.ask, drawn.bid);
    let color = if mark_imbalance && ask_imbalanced && (buy || !bid_imbalanced) {
        visual.stacked_ask_color.solid()
    } else if mark_imbalance && bid_imbalanced {
        visual.stacked_bid_color.solid()
    } else {
        let side = if buy {
            visual.ask_color
        } else {
            visual.bid_color
        };
        Color::rgba(
            side.r(),
            side.g(),
            side.b(),
            opacity_alpha(visual.histogram_opacity),
        )
    };
    out.push(Prim::Rect {
        rect: IRect {
            x: left,
            y: cell.top,
            w: width,
            h: cell.height,
        },
        color,
    });
}

/// One total or delta row spanning the full cell width.
fn push_single_value_row(
    out: &mut Vec<Prim>,
    level: &FootprintLevel,
    column: ClusterColumn,
    cell: CellRect,
    maxima: VolumeMaxima,
    visual: &crate::FootprintVisualOptions,
    text: Option<&FootprintTextStyle<'_>>,
) {
    let left = column.cells_left;
    let width = column.cells_width().max(1);
    let total_mode = !matches!(visual.cell_mode, FootprintCellMode::Delta);
    let (value, base, peak) = if total_mode {
        let base = if level.ask_volume >= level.bid_volume {
            visual.ask_color
        } else {
            visual.bid_color
        };
        (level.total_volume, base, maxima.total)
    } else {
        let base = if level.delta >= 0.0 {
            visual.positive_delta_color
        } else {
            visual.negative_delta_color
        };
        (level.delta, base, maxima.abs_delta)
    };
    let drawn = DrawnImbalance::of(level, visual);
    let imbalanced = drawn.any();
    let mut background = Color::rgba(0, 0, 0, 0);
    if imbalanced {
        let stacked = drawn.stacked_bid || drawn.stacked_ask;
        let side_color = if level.ask_volume >= level.bid_volume {
            visual.stacked_ask_color
        } else {
            visual.stacked_bid_color
        };
        background = imbalance_cell(side_color, stacked, visual);
        out.push(Prim::Rect {
            rect: IRect {
                x: left,
                y: cell.top,
                w: width,
                h: cell.height,
            },
            color: background,
        });
    } else {
        // Total grows from the left like a volume profile; delta diverges from the center.
        let magnitude = value.abs();
        let center = left + width / 2;
        let (bar_x, bar_width) = if total_mode {
            (left, profile_width(width, magnitude, peak))
        } else if value >= 0.0 {
            (
                center,
                profile_width(width - (center - left), magnitude, peak),
            )
        } else {
            let bar_width = profile_width(center - left, magnitude, peak);
            (center - bar_width, bar_width)
        };
        if bar_width > 0 {
            let bar = profile_bar(base, magnitude, peak);
            out.push(Prim::Rect {
                rect: IRect {
                    x: bar_x,
                    y: cell.top,
                    w: bar_width,
                    h: cell.height,
                },
                color: bar,
            });
            if bar_width * 4 >= width {
                background = bar;
            }
        }
    }
    if let Some(style) = text
        && value != 0.0
    {
        push_cell_text(
            out,
            left as f32 + width as f32 / 2.0,
            cell.top as f32 + cell.height as f32 / 2.0,
            TextAlign::Center,
            width,
            cell.height,
            compact_volume(value),
            style,
            contrast_on(background, style),
            imbalanced,
        );
    }
}

/// Profile-in-bar row: one total-volume bar from the cluster's left edge, scaled to the bar's
/// peak row and split into bid, ask, and unclassified segments in that order. No track wash is
/// drawn, so the empty space reads as the profile's shape. Imbalanced rows print in bold.
#[allow(clippy::too_many_arguments)]
fn push_profile_row(
    out: &mut Vec<Prim>,
    level: &FootprintLevel,
    column: ClusterColumn,
    cell: CellRect,
    maxima: VolumeMaxima,
    visual: &crate::FootprintVisualOptions,
    unknown_color: Color,
    text: Option<&FootprintTextStyle<'_>>,
) {
    let left = column.cells_left;
    let width = column.cells_width().max(1);
    let total = level.total_volume;
    let bar_width = profile_width(width, total, maxima.total);
    let mut background = Color::rgba(0, 0, 0, 0);
    if bar_width > 0 {
        let segment = |volume: f64| -> i32 {
            ((f64::from(bar_width) * (volume / total).clamp(0.0, 1.0)).round() as i32)
                .clamp(0, bar_width)
        };
        let bid_width = segment(level.bid_volume);
        let ask_width = segment(level.ask_volume).min(bar_width - bid_width);
        let unknown_width = bar_width - bid_width - ask_width;
        let mut x = left;
        let mut widest = (0, background);
        for (segment_width, base) in [
            (bid_width, visual.bid_color),
            (ask_width, visual.ask_color),
            (
                unknown_width,
                visual.unclassified_color.unwrap_or(unknown_color),
            ),
        ] {
            if segment_width == 0 {
                continue;
            }
            let color = profile_bar(base, total, maxima.total);
            out.push(Prim::Rect {
                rect: IRect {
                    x,
                    y: cell.top,
                    w: segment_width,
                    h: cell.height,
                },
                color,
            });
            if segment_width > widest.0 {
                widest = (segment_width, color);
            }
            x += segment_width;
        }
        if bar_width * 4 >= width {
            background = widest.1;
        }
    }
    let imbalanced = DrawnImbalance::of(level, visual).any();
    if let Some(style) = text
        && total != 0.0
    {
        push_cell_text(
            out,
            left as f32 + width as f32 / 2.0,
            cell.top as f32 + cell.height as f32 / 2.0,
            TextAlign::Center,
            width,
            cell.height,
            compact_volume(total),
            style,
            contrast_on(background, style),
            imbalanced,
        );
    }
}

/// Bid/ask histogram row: the ask bar fills the upper half of the row and the bid bar the lower
/// half, both from the cluster's left edge on one scale (the larger side's peak) so their lengths
/// compare directly. Imbalanced sides use the imbalance color; numbers print as `bid x ask`.
fn push_bid_ask_histogram_row(
    out: &mut Vec<Prim>,
    level: &FootprintLevel,
    column: ClusterColumn,
    cell: CellRect,
    maxima: VolumeMaxima,
    visual: &crate::FootprintVisualOptions,
    text: Option<&FootprintTextStyle<'_>>,
) {
    let left = column.cells_left;
    let width = column.cells_width().max(1);
    let peak = maxima.bid.max(maxima.ask);
    let ask_height = (cell.height / 2).max(1);
    let ask_cell = CellRect {
        top: cell.top,
        height: ask_height,
    };
    let bid_cell = CellRect {
        top: cell.top + ask_height,
        height: (cell.height - ask_height).max(1),
    };
    let drawn = DrawnImbalance::of(level, visual);
    let sides = [
        (
            ask_cell,
            level.ask_volume,
            visual.ask_color,
            drawn
                .ask
                .then(|| imbalance_cell(visual.stacked_ask_color, drawn.stacked_ask, visual)),
        ),
        (
            bid_cell,
            level.bid_volume,
            visual.bid_color,
            drawn
                .bid
                .then(|| imbalance_cell(visual.stacked_bid_color, drawn.stacked_bid, visual)),
        ),
    ];
    for (side_cell, volume, base, imbalance) in sides {
        let bar_width = profile_width(width, volume, peak);
        if bar_width == 0 {
            continue;
        }
        out.push(Prim::Rect {
            rect: IRect {
                x: left,
                y: side_cell.top,
                w: bar_width,
                h: side_cell.height,
            },
            color: imbalance.unwrap_or_else(|| profile_bar(base, volume, peak)),
        });
    }
    let Some(style) = text else {
        return;
    };
    if level.bid_volume == 0.0 && level.ask_volume == 0.0 {
        return;
    }
    let imbalanced = drawn.any();
    push_cell_text(
        out,
        left as f32 + width as f32 / 2.0,
        cell.top as f32 + cell.height as f32 / 2.0,
        TextAlign::Center,
        width,
        cell.height,
        format!(
            "{} x {}",
            compact_volume(level.bid_volume),
            compact_volume(level.ask_volume)
        ),
        style,
        style.fallback_color,
        imbalanced,
    );
}

/// Horizontal extent of a profile bar, linear in volume. Returns 0 when nothing draws.
fn profile_width(span: i32, volume: f64, peak: f64) -> i32 {
    if span <= 0 || peak <= 0.0 || volume <= 0.0 {
        return 0;
    }
    let fraction = (volume / peak).clamp(0.0, 1.0);
    ((f64::from(span) * fraction).round() as i32).clamp(1, span)
}

/// Profile bar color: the side hue with an alpha ramp so heavy prints stand out. Gamma below 1
/// keeps small prints visible next to the bar peak.
fn profile_bar(base: Color, volume: f64, peak: f64) -> Color {
    let t = if peak > 0.0 && volume > 0.0 {
        (volume / peak).clamp(0.0, 1.0).powf(0.6)
    } else {
        0.0
    };
    let alpha = (110.0 + 145.0 * t).round().clamp(0.0, 255.0) as u8;
    Color::rgba(base.r(), base.g(), base.b(), alpha)
}

/// Single imbalances tint their cell; stacked runs are opaque so they read at every density.
fn imbalance_cell(color: Color, stacked: bool, visual: &crate::FootprintVisualOptions) -> Color {
    if stacked {
        color.solid()
    } else {
        Color::rgba(
            color.r(),
            color.g(),
            color.b(),
            opacity_alpha(visual.imbalance_opacity),
        )
    }
}

/// Outlines the point-of-control row across the printed cells without covering its numbers.
fn push_poc_outline(
    out: &mut Vec<Prim>,
    left: i32,
    right: i32,
    cell: CellRect,
    border: f32,
    poc: Color,
) {
    out.push(Prim::RoundRect {
        x: left as f32,
        y: cell.top as f32,
        w: (right - left).max(1) as f32,
        h: cell.height.max(1) as f32,
        radii: [0.0; 4],
        fill: Color::rgba(0, 0, 0, 0),
        border_width: border,
        border_color: poc,
    });
}

/// Glyph color for a cell. An explicit host text color stays authoritative and bare cells use
/// the layout foreground. Opaque fills pick the higher-contrast glyph; translucent fills do the
/// same on dark surfaces, while light surfaces keep the foreground so a tint never flips
/// numbers to white.
fn contrast_on(cell: Color, style: &FootprintTextStyle<'_>) -> Color {
    if style.explicit_text
        || cell.a() == 0
        || (cell.a() < u8::MAX && style.surface.luminance() > 160.0)
    {
        return style.fallback_color;
    }
    composite_over(cell, style.surface).contrast_srgb()
}

fn composite_over(foreground: Color, background: Color) -> Color {
    let alpha = u32::from(foreground.a());
    let inverse = 255 - alpha;
    let blend = |f: u8, b: u8| ((u32::from(f) * alpha + u32::from(b) * inverse + 127) / 255) as u8;
    Color::rgb(
        blend(foreground.r(), background.r()),
        blend(foreground.g(), background.g()),
        blend(foreground.b(), background.b()),
    )
}

fn text_width(text: &str, style: &FootprintTextStyle<'_>) -> f32 {
    text.chars().count() as f32 * GLYPH_ADVANCE * style.size
}

/// Prints a cell number at the style's size when it fits its cell; a number that would overflow
/// is left out rather than shrunk further or clipped.
#[allow(clippy::too_many_arguments)]
fn push_cell_text(
    out: &mut Vec<Prim>,
    x: f32,
    y: f32,
    align: TextAlign,
    available_width: i32,
    available_height: i32,
    text: String,
    style: &FootprintTextStyle<'_>,
    color: Color,
    bold: bool,
) {
    if text_width(&text, style) + 2.0 > available_width as f32
        || f64::from(available_height) < f64::from(style.size) * TEXT_HEIGHT
    {
        return;
    }
    out.push(Prim::Text {
        x,
        y,
        text,
        color,
        size: style.size,
        family: style.family.to_string(),
        align,
        weight: if bold {
            style.strong_weight
        } else {
            style.weight
        },
        italic: false,
    });
}

/// `Δ delta` in its sign color over `V volume`, centered under the cluster's low row.
#[allow(clippy::too_many_arguments)]
fn push_bar_summary(
    out: &mut Vec<Prim>,
    bar: &FootprintBar,
    column: ClusterColumn,
    scale: &PriceScaleCore,
    base_value: f64,
    vpr: f64,
    visual: &crate::FootprintVisualOptions,
    aggregation: &FootprintAggregationOptions,
    style: &FootprintTextStyle<'_>,
) {
    let (low_price, _) = footprint_row_price_bounds(aggregation, bar.low, bar.high);
    let bottom = (scale.price_to_coordinate(low_price, base_value) * vpr).round() as f32;
    let line = style.size * SUMMARY_LINE as f32;
    let x = (column.left + column.right) as f32 / 2.0;
    let first = bottom + (SUMMARY_GAP * vpr).round() as f32 + line / 2.0;
    let delta_color = if bar.delta >= 0.0 {
        visual.positive_delta_color
    } else {
        visual.negative_delta_color
    };
    let [delta, volume] = summary_lines(bar);
    let volume_color = visual.summary_text_color.unwrap_or(style.fallback_color);
    for (y, text, color, weight) in [
        (first, delta, delta_color.solid(), style.strong_weight),
        (first + line, volume, volume_color, style.weight),
    ] {
        out.push(Prim::Text {
            x,
            y,
            text,
            color,
            size: style.size,
            family: style.family.to_string(),
            align: TextAlign::Center,
            weight,
            italic: false,
        });
    }
}

fn compact_volume(value: f64) -> String {
    let absolute = value.abs();
    // Whole sizes below 100K print exactly: "1195" is no wider than "1.2K".
    if value.fract().abs() < 1e-9 && absolute < 100_000.0 {
        format!("{value:.0}")
    } else if absolute >= 1_000_000.0 {
        format!("{:.1}M", value / 1_000_000.0)
    } else if absolute >= 1_000.0 {
        format!("{:.1}K", value / 1_000.0)
    } else if absolute >= 10.0 {
        format!("{value:.1}")
    } else if absolute >= 0.01 {
        format!("{value:.2}")
    } else {
        // Fractional crypto sizes: two significant digits, so a traded level never reads "0.00".
        let decimals = (1.0 - absolute.log10().floor()).clamp(2.0, 8.0) as usize;
        let text = format!("{value:.decimals$}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_volume_never_rounds_a_traded_size_to_zero() {
        assert_eq!(compact_volume(0.003), "0.003");
        assert_eq!(compact_volume(0.00042), "0.00042");
        assert_eq!(compact_volume(-0.0051), "-0.0051");
        assert_eq!(compact_volume(0.16), "0.16");
        assert_eq!(compact_volume(4.01), "4.01");
        assert_eq!(compact_volume(26.4), "26.4");
        assert_eq!(compact_volume(70.0), "70");
        assert_eq!(compact_volume(1_195.0), "1195");
        assert_eq!(compact_volume(-25_269.0), "-25269");
        assert_eq!(compact_volume(1_500.5), "1.5K");
        assert_eq!(compact_volume(250_000.0), "250.0K");
        assert_eq!(compact_volume(1_500_000.0), "1.5M");
    }

    #[test]
    fn the_bar_summary_reserves_nothing_when_off_or_the_pane_is_too_short() {
        let visual = crate::FootprintVisualOptions::default();
        let height = footprint_summary_height(&visual, 600.0);
        assert_eq!(
            height,
            (SUMMARY_GAP + 2.0 * visual.font_size * SUMMARY_FONT * SUMMARY_LINE).ceil()
        );
        assert_eq!(footprint_summary_height(&visual, height * 3.0), 0.0);
        let off = crate::FootprintVisualOptions {
            show_bar_summary: false,
            ..visual
        };
        assert_eq!(footprint_summary_height(&off, 600.0), 0.0);
    }
}
