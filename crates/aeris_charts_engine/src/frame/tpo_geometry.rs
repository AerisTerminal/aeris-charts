//! TPO series geometry: one profile per session, packed from the session's left edge and
//! confined to the session's own time span, so neighbouring sessions never overlap.
//!
//! Level of detail is chosen once per profile from its cell size: letters when a cell can hold
//! one, blocks when it cannot, and one bar per row (rows sharing a device pixel merged) when
//! cells are narrower than a few pixels or the visible cell count exceeds its bound.

use std::collections::BTreeMap;

use super::feature_geometry::positions_box;
use super::*;
use aeris_charts_render::draw_list::TextAlign;

/// Widest TPO cell, in CSS px. A session wider than its letters keeps them at this width.
const MAX_CELL_WIDTH: f64 = 14.0;
/// Narrowest cell that still draws one block per period.
const MIN_BLOCK_WIDTH: f64 = 3.0;
/// Smallest cell that holds a legible letter.
const MIN_LETTER_WIDTH: f64 = 7.0;
const MIN_LETTER_HEIGHT: f64 = 9.0;
const MAX_LETTER_SIZE: f64 = 12.0;
/// Above this many visible cells every profile draws one bar per row.
const MAX_VISIBLE_CELLS: usize = 8_192;
const LETTERS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

#[derive(Clone, Copy, PartialEq, Eq)]
enum TpoLod {
    Letters,
    Blocks,
    Rows,
}

impl ChartEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_tpo_series_frame(
        &self,
        rs: ResolvedSeries,
        from: i64,
        to: i64,
        hpr: f64,
        vpr: f64,
        pane_top: f64,
        pane_height: f64,
        out: &mut Vec<Prim>,
        scale: &PriceScaleCore,
    ) {
        if scale.is_empty() {
            return;
        }
        let Some(state) = self
            .series_entry(rs.id)
            .and_then(|series| series.tpo.as_ref())
        else {
            return;
        };
        let Some(sessions) = self.tpo_sessions(rs.id) else {
            return;
        };
        let plot = self.data.plot(rs.id);
        let spacing = self.time_scale.bar_spacing();
        let tick = state.rules.tick_size;
        let market = self.market_colors();
        let colors = state.colors.map(|color| market.follow(color));
        let family = &self.options.get().layout.font_family;
        let (clip_top, clip_bottom) = (pane_top * vpr, (pane_top + pane_height) * vpr);
        let y_of = |price: f64| scale.price_to_coordinate(price, rs.base_value);

        // Visible sessions and their horizontal extent.
        let visible = sessions
            .iter()
            .filter_map(|session| {
                let profile = session.profile.as_ref()?;
                let last_row = session.finish_row.checked_sub(1)?;
                let first = plot.index_at(session.first_row)?;
                let last = plot.index_at(last_row)?;
                if last < from || first > to {
                    return None;
                }
                let left = self.time_scale.index_to_coordinate(first) - spacing / 2.0;
                let right = self.time_scale.index_to_coordinate(last) + spacing / 2.0;
                (right > 0.0 && left < self.pane_w).then_some((profile, left, right))
            })
            .collect::<Vec<_>>();
        let visible_cells = visible
            .iter()
            .flat_map(|(profile, ..)| &profile.rows)
            .map(|row| row.periods.len())
            .sum::<usize>();

        for (profile, left, right) in visible {
            let widest = profile
                .rows
                .iter()
                .map(|row| row.periods.len())
                .max()
                .unwrap_or(0);
            if widest == 0 {
                continue;
            }
            let cell = ((right - left) / widest as f64).min(MAX_CELL_WIDTH);
            let row_height = profile
                .rows
                .first()
                .map_or(0.0, |row| (y_of(row.price) - y_of(row.price + tick)).abs());
            let lod = if visible_cells > MAX_VISIBLE_CELLS
                || cell < MIN_BLOCK_WIDTH
                || row_height * vpr < 2.0
            {
                TpoLod::Rows
            } else if state.mode == crate::TpoCellMode::Letters
                && cell >= MIN_LETTER_WIDTH
                && row_height >= MIN_LETTER_HEIGHT
            {
                TpoLod::Letters
            } else {
                TpoLod::Blocks
            };
            let in_value_area = |price: f64| {
                profile.value_area_low.is_some_and(|low| price >= low)
                    && profile.value_area_high.is_some_and(|high| price <= high)
            };
            let category = |price: f64, single: bool| {
                if single {
                    2
                } else if in_value_area(price) {
                    1
                } else {
                    0
                }
            };
            // Device-pixel row -> (right edge, height, strongest category).
            let mut merged = BTreeMap::<i32, (f64, i32, usize)>::new();
            let letter_size = (row_height * 0.9).min(cell * 1.2).min(MAX_LETTER_SIZE);
            for row in &profile.rows {
                let lower = y_of(row.price);
                let upper = y_of(row.price + tick);
                let (y, height) = positions_box(upper, lower, vpr);
                if f64::from(y + height) < clip_top || f64::from(y) > clip_bottom {
                    continue;
                }
                let kind = category(row.price, row.single_print);
                if lod == TpoLod::Rows {
                    let width = row.periods.len() as f64 * cell;
                    merged
                        .entry(y)
                        .and_modify(|entry| {
                            entry.0 = entry.0.max(width);
                            entry.1 = entry.1.max(height);
                            entry.2 = entry.2.max(kind);
                        })
                        .or_insert((width, height, kind));
                    continue;
                }
                for (slot, &period) in row.periods.iter().enumerate() {
                    let x = left + slot as f64 * cell;
                    if x + cell < 0.0 || x > self.pane_w {
                        continue;
                    }
                    if lod == TpoLod::Letters {
                        out.push(Prim::Text {
                            x: ((x + cell / 2.0) * hpr) as f32,
                            y: ((lower + upper) * 0.5 * vpr) as f32,
                            text: char::from(LETTERS[usize::from(period) % LETTERS.len()])
                                .to_string(),
                            color: colors[kind],
                            size: (letter_size * vpr) as f32,
                            family: family.clone(),
                            align: TextAlign::Center,
                            weight: 500,
                            italic: false,
                        });
                    } else {
                        let (x, width) = positions_box(x, x + cell - 1.0, hpr);
                        out.push(Prim::Rect {
                            rect: IRect {
                                x,
                                y,
                                w: width.max(1),
                                h: (height - 1).max(1),
                            },
                            color: colors[kind],
                        });
                    }
                }
            }
            for (y, (width, height, kind)) in merged {
                let (x, width) = positions_box(left, left + width, hpr);
                out.push(Prim::Rect {
                    rect: IRect {
                        x,
                        y,
                        w: width.max(1),
                        h: height.max(1),
                    },
                    color: colors[kind],
                });
            }
            let profile_right = (left + widest as f64 * cell).min(right);
            if let Some(poc) = profile.poc {
                out.push(Prim::HLine {
                    y: ((y_of(poc) + y_of(poc + tick)) * 0.5 * vpr).round() as i32,
                    x0: (left * hpr).round() as i32,
                    x1: (profile_right * hpr).round() as i32,
                    width: vpr.round().max(1.0) as i32,
                    style: LineStyle::Solid,
                    color: colors[3],
                });
            }
            if let (Some(low), Some(high)) =
                (profile.initial_balance_low, profile.initial_balance_high)
            {
                out.push(Prim::VLine {
                    x: ((left + 1.0) * hpr).round() as i32,
                    y0: (y_of(high) * vpr).round() as i32,
                    y1: (y_of(low) * vpr).round() as i32,
                    width: (2.0 * hpr).round().max(1.0) as i32,
                    style: LineStyle::Solid,
                    color: colors[4],
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ResampleBoundary, TpoSeriesOptions};

    /// Three one-hour sessions of twelve five-minute bars on a 1.0 tick grid.
    fn tpo_chart(mode: crate::TpoCellMode) -> (ChartEngine, SeriesId) {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart.convert_series_kind(0, SeriesKind::Tpo);
        chart
            .configure_tpo_series(
                0,
                TpoSeriesOptions {
                    period_seconds: 900,
                    tick_size: 1.0,
                    mode,
                    ..Default::default()
                },
            )
            .unwrap();
        let mut times = Vec::new();
        let (mut open, mut high, mut low, mut close) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for day in 0..3_i64 {
            for bar in 0..12_i64 {
                let price = 100.0 + (bar % 4) as f64;
                times.push((day * 86_400 + bar * 300) as f64);
                open.push(price);
                high.push(price + 1.0);
                low.push(price - 1.0);
                close.push(price);
            }
        }
        chart
            .set_series_data(0, &times, &open, &high, &low, &close)
            .unwrap();
        chart
            .set_tpo_sessions(
                0,
                (0..3_i64)
                    .map(|day| ResampleBoundary {
                        start_time: day * 86_400,
                        end_time: day * 86_400 + 3_600,
                        session_id: day as u64 + 1,
                    })
                    .collect(),
            )
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        (chart, 0)
    }

    fn texts(prims: &[Prim]) -> usize {
        prims
            .iter()
            .filter(|prim| matches!(prim, Prim::Text { .. }))
            .count()
    }

    #[test]
    fn a_tpo_series_draws_letters_in_place_of_candles() {
        let (mut chart, id) = tpo_chart(crate::TpoCellMode::Letters);
        let cells = chart
            .tpo_series_profiles(id)
            .unwrap()
            .iter()
            .flat_map(|profile| &profile.rows)
            .map(|row| row.periods.len())
            .sum::<usize>();
        let frame = chart.build_frame();
        let main = &frame.panes[0].main;
        assert!(
            main.iter()
                .any(|prim| matches!(prim, Prim::Text { text, .. } if text == "A"))
        );
        assert!(
            main.iter()
                .any(|prim| matches!(prim, Prim::Text { text, .. } if text == "D"))
        );
        // Every cell prints one letter, and no candle body is drawn under the profile.
        assert!(texts(main) >= cells);
        assert!(!main.iter().any(|prim| matches!(prim, Prim::Rect { .. })));
        assert!(main.iter().any(|prim| matches!(prim,
            Prim::HLine { color, .. } if *color == Color::rgb(245, 166, 35)
        )));
        // An unchanged chart rebuilds nothing.
        chart.build_frame();
        assert_eq!(chart.frame_build_stats().series_rebuilds, 0);
    }

    #[test]
    fn sessions_never_draw_past_their_own_time_span() {
        let (mut chart, id) = tpo_chart(crate::TpoCellMode::Blocks);
        let frame = chart.build_frame();
        let spacing = chart.time_scale.bar_spacing();
        let plot = chart.data.plot(id);
        let edges = [0_usize, 12, 24].map(|first| {
            let left = chart
                .time_scale
                .index_to_coordinate(plot.index_at(first).unwrap())
                - spacing / 2.0;
            let right = chart
                .time_scale
                .index_to_coordinate(plot.index_at(first + 11).unwrap())
                + spacing / 2.0;
            (left.floor() as i32, right.ceil() as i32)
        });
        for prim in &frame.panes[0].main {
            if let Prim::Rect { rect, .. } = prim {
                assert!(
                    edges
                        .iter()
                        .any(|&(left, right)| rect.x >= left && rect.x + rect.w <= right + 1),
                    "block {rect:?} leaves every session span {edges:?}"
                );
            }
        }
    }

    #[test]
    fn zooming_out_merges_cells_into_one_bar_per_row() {
        let (mut chart, id) = tpo_chart(crate::TpoCellMode::Letters);
        chart.set_bar_spacing(0.5);
        let rows = chart
            .tpo_series_profiles(id)
            .unwrap()
            .iter()
            .map(|profile| profile.rows.len())
            .sum::<usize>();
        let frame = chart.build_frame();
        let main = &frame.panes[0].main;
        assert_eq!(texts(main), 0);
        let blocks = main
            .iter()
            .filter(|prim| matches!(prim, Prim::Rect { .. }))
            .count();
        assert!(
            (1..=rows).contains(&blocks),
            "{blocks} blocks for {rows} rows"
        );
    }
}
