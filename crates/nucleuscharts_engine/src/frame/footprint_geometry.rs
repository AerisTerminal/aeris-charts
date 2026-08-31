use super::*;
use crate::footprint::{footprint_cell_price_bounds, FootprintBar, FootprintCellMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FootprintLod {
    Detailed,
    Cells,
    Summary,
}

struct FootprintTextStyle<'a> {
    color: Color,
    size: f32,
    family: &'a str,
    pixel_ratio: f64,
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
        let plot = self.data.plot(rs.id);
        let spacing = self.time_scale.bar_spacing();
        let font_size = state.visual.font_size;
        let layout = &self.options.get().layout;
        let family = layout.font_family.clone();
        let text_color = state
            .visual
            .text_color
            .or_else(|| Color::parse_css(&layout.text_color))
            .unwrap_or(Color::rgb(215, 219, 228));
        let text_style = FootprintTextStyle {
            color: text_color,
            size: (font_size * vpr) as f32,
            family: &family,
            pixel_ratio: vpr,
        };
        for row in plot.visible_rows(from, to) {
            let Some(bar) = state.aggregator.bars().get(row) else {
                continue;
            };
            let Some(logical) = plot.index_at(row) else {
                continue;
            };
            let x = self.time_scale.index_to_coordinate(logical);
            let left = ((x - spacing * 0.48) * hpr).round() as i32;
            let right = ((x + spacing * 0.48) * hpr).round() as i32;
            let width = (right - left).max(1);
            let tick_size = state.aggregator.options().tick_size;
            let representative_row_height = bar
                .levels
                .first()
                .map(|level| {
                    let (lower_price, upper_price) =
                        footprint_cell_price_bounds(level.price, level.price, tick_size);
                    let upper = scale.price_to_coordinate(upper_price, rs.base_value);
                    let lower = scale.price_to_coordinate(lower_price, rs.base_value);
                    (lower - upper).abs()
                })
                .unwrap_or(0.0);
            let lod = if spacing >= 64.0 && representative_row_height >= font_size + 3.0 {
                FootprintLod::Detailed
            } else if spacing >= 10.0 && representative_row_height * vpr >= 2.0 {
                FootprintLod::Cells
            } else {
                FootprintLod::Summary
            };
            match lod {
                FootprintLod::Detailed | FootprintLod::Cells => {
                    for level in &bar.levels {
                        let (lower_price, upper_price) =
                            footprint_cell_price_bounds(level.price, level.price, tick_size);
                        let upper = scale.price_to_coordinate(upper_price, rs.base_value);
                        let lower = scale.price_to_coordinate(lower_price, rs.base_value);
                        let top = (upper.min(lower) * vpr).round() as i32;
                        let bottom = (upper.max(lower) * vpr).round() as i32;
                        let height = (bottom - top).max(1);
                        let center_y = top as f32 + height as f32 / 2.0;
                        match state.visual.cell_mode {
                            FootprintCellMode::BidAsk => {
                                let split = left + width / 2;
                                out.push(Prim::Rect {
                                    rect: IRect {
                                        x: left,
                                        y: top,
                                        w: (split - left).max(1),
                                        h: height,
                                    },
                                    color: state.visual.bid_color,
                                });
                                out.push(Prim::Rect {
                                    rect: IRect {
                                        x: split,
                                        y: top,
                                        w: (right - split).max(1),
                                        h: height,
                                    },
                                    color: state.visual.ask_color,
                                });
                                if lod == FootprintLod::Detailed {
                                    push_cell_text(
                                        out,
                                        left as f32 + (split - left) as f32 / 2.0,
                                        center_y,
                                        compact_volume(level.bid_volume),
                                        &text_style,
                                    );
                                    push_cell_text(
                                        out,
                                        split as f32 + (right - split) as f32 / 2.0,
                                        center_y,
                                        compact_volume(level.ask_volume),
                                        &text_style,
                                    );
                                }
                            }
                            FootprintCellMode::Total | FootprintCellMode::Delta => {
                                let value = if state.visual.cell_mode == FootprintCellMode::Total {
                                    level.total_volume
                                } else {
                                    level.delta
                                };
                                let color = if state.visual.cell_mode == FootprintCellMode::Total {
                                    if level.ask_volume >= level.bid_volume {
                                        state.visual.ask_color
                                    } else {
                                        state.visual.bid_color
                                    }
                                } else if value >= 0.0 {
                                    state.visual.positive_delta_color
                                } else {
                                    state.visual.negative_delta_color
                                };
                                out.push(Prim::Rect {
                                    rect: IRect {
                                        x: left,
                                        y: top,
                                        w: width,
                                        h: height,
                                    },
                                    color,
                                });
                                if lod == FootprintLod::Detailed {
                                    push_cell_text(
                                        out,
                                        left as f32 + width as f32 / 2.0,
                                        center_y,
                                        compact_volume(value),
                                        &text_style,
                                    );
                                }
                            }
                        }
                        if level.level == bar.poc_level {
                            out.push(Prim::RoundRect {
                                x: left as f32,
                                y: top as f32,
                                w: width as f32,
                                h: height as f32,
                                radii: [0.0; 4],
                                fill: Color::rgba(0, 0, 0, 0),
                                border_width: hpr.max(vpr).round().max(1.0) as f32,
                                border_color: state.visual.poc_color,
                            });
                        }
                        let strip = (2.0 * hpr).round().max(1.0) as i32;
                        if level.stacked_bid_imbalance {
                            out.push(Prim::Rect {
                                rect: IRect {
                                    x: left,
                                    y: top,
                                    w: strip.min(width),
                                    h: height,
                                },
                                color: state.visual.stacked_bid_color,
                            });
                        }
                        if level.stacked_ask_imbalance {
                            out.push(Prim::Rect {
                                rect: IRect {
                                    x: right - strip.min(width),
                                    y: top,
                                    w: strip.min(width),
                                    h: height,
                                },
                                color: state.visual.stacked_ask_color,
                            });
                        }
                    }
                    if lod == FootprintLod::Detailed && state.visual.show_bar_summary {
                        push_bar_summary(out, bar, left, width, scale, rs.base_value, &text_style);
                    }
                }
                FootprintLod::Summary => {
                    let (low_price, high_price) =
                        footprint_cell_price_bounds(bar.low, bar.high, tick_size);
                    let high = scale.price_to_coordinate(high_price, rs.base_value);
                    let low = scale.price_to_coordinate(low_price, rs.base_value);
                    let top = (high.min(low) * vpr).round() as i32;
                    let bottom = (high.max(low) * vpr).round() as i32;
                    out.push(Prim::Rect {
                        rect: IRect {
                            x: left,
                            y: top,
                            w: width,
                            h: (bottom - top).max(1),
                        },
                        color: if bar.delta >= 0.0 {
                            state.visual.positive_delta_color
                        } else {
                            state.visual.negative_delta_color
                        },
                    });
                    let poc_y = (scale.price_to_coordinate(bar.poc_price, rs.base_value) * vpr)
                        .round() as i32;
                    out.push(Prim::Rect {
                        rect: IRect {
                            x: left,
                            y: poc_y,
                            w: width,
                            h: vpr.round().max(1.0) as i32,
                        },
                        color: state.visual.poc_color,
                    });
                }
            }
        }
    }
}

fn push_cell_text(
    out: &mut Vec<Prim>,
    x: f32,
    y: f32,
    text: String,
    style: &FootprintTextStyle<'_>,
) {
    out.push(Prim::Text {
        x,
        y,
        text,
        color: style.color,
        size: style.size,
        family: style.family.to_string(),
        align: TextAlign::Center,
        weight: 500,
        italic: false,
    });
}

fn push_bar_summary(
    out: &mut Vec<Prim>,
    bar: &FootprintBar,
    left: i32,
    width: i32,
    scale: &PriceScaleCore,
    base_value: f64,
    style: &FootprintTextStyle<'_>,
) {
    let y = (scale.price_to_coordinate(bar.low, base_value) * style.pixel_ratio) as f32
        + style.size * 0.8;
    out.push(Prim::Text {
        x: left as f32 + width as f32 / 2.0,
        y,
        text: format!(
            "Δ {}  H {}  L {}",
            compact_volume(bar.delta),
            compact_volume(bar.max_delta),
            compact_volume(bar.min_delta)
        ),
        color: style.color,
        size: style.size * 0.9,
        family: style.family.to_string(),
        align: TextAlign::Center,
        weight: 600,
        italic: false,
    });
    out.push(Prim::Text {
        x: left as f32 + width as f32 / 2.0,
        y: y + style.size,
        text: format!(
            "V {}  B {}  A {}",
            compact_volume(bar.total_volume),
            compact_volume(bar.bid_volume),
            compact_volume(bar.ask_volume)
        ),
        color: style.color,
        size: style.size * 0.9,
        family: style.family.to_string(),
        align: TextAlign::Center,
        weight: 500,
        italic: false,
    });
}

fn compact_volume(value: f64) -> String {
    let absolute = value.abs();
    if absolute >= 1_000_000.0 {
        format!("{:.1}M", value / 1_000_000.0)
    } else if absolute >= 1_000.0 {
        format!("{:.1}K", value / 1_000.0)
    } else if value.fract().abs() < 1e-9 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}
