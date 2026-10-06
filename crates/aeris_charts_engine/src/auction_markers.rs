//! Auction-marker rules over canonical footprint levels.

use aeris_charts_core::model::data_layer::SeriesId;
use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{IRect, LineStyle, Prim, TextAlign};

use crate::footprint::{FootprintBar, FootprintBarAggregation, FootprintError};
use crate::frame::{pane_scale, series_scale_target};
use crate::{ChartEngine, NativePrimitiveId, SeriesKind};

pub const MAX_AUCTION_MARKERS: usize = 16;
const MICROS_PER_SECOND: i64 = 1_000_000;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuctionMarkerOptions {
    pub min_side_volume: f64,
    pub exhaustion_max_volume: f64,
    pub exhaustion_levels: usize,
    pub absorption_min_volume: f64,
    pub absorption_ratio: f64,
    pub extreme_levels: usize,
    pub min_rejection_rows: usize,
    pub extend_until_revisited: bool,
    pub include_forming_bar: bool,
    pub visible: bool,
}

impl Default for AuctionMarkerOptions {
    fn default() -> Self {
        Self {
            min_side_volume: 0.0,
            exhaustion_max_volume: 10.0,
            exhaustion_levels: 3,
            absorption_min_volume: 100.0,
            absorption_ratio: 3.0,
            extreme_levels: 2,
            min_rejection_rows: 1,
            extend_until_revisited: false,
            include_forming_bar: false,
            visible: true,
        }
    }
}

impl AuctionMarkerOptions {
    pub fn valid(&self) -> bool {
        [
            self.min_side_volume,
            self.exhaustion_max_volume,
            self.absorption_min_volume,
            self.absorption_ratio,
        ]
        .iter()
        .all(|value| value.is_finite() && *value >= 0.0)
            && (2..=8).contains(&self.exhaustion_levels)
            && self.extreme_levels > 0
            && self.extreme_levels <= 8
            && self.min_rejection_rows <= 8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuctionMarkKind {
    UnfinishedAuction,
    Exhaustion,
    Absorption,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuctionSide {
    High,
    Low,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct AuctionMark {
    pub bar_time: i64,
    pub kind: AuctionMarkKind,
    pub side: AuctionSide,
    pub price: f64,
    pub volume: f64,
}

fn detect(bar: &FootprintBar, options: &AuctionMarkerOptions) -> Vec<AuctionMark> {
    let levels = &bar.levels;
    let mut marks = Vec::with_capacity(6);
    if levels.is_empty() {
        return marks;
    }
    for side in [AuctionSide::Low, AuctionSide::High] {
        let edge = if side == AuctionSide::Low {
            &levels[0]
        } else {
            levels.last().unwrap()
        };
        let (volume, other) = if side == AuctionSide::Low {
            (edge.bid_volume, edge.ask_volume)
        } else {
            (edge.ask_volume, edge.bid_volume)
        };
        // A strictly positive side is required even at the default zero threshold.
        if volume > 0.0
            && other > 0.0
            && volume >= options.min_side_volume
            && other >= options.min_side_volume
        {
            marks.push(AuctionMark {
                bar_time: 0,
                kind: AuctionMarkKind::UnfinishedAuction,
                side,
                price: edge.price,
                volume: volume + other,
            });
        }
        if levels.len() >= options.exhaustion_levels
            && volume > 0.0
            && volume <= options.exhaustion_max_volume
            && (1..options.exhaustion_levels).all(|offset| {
                let index = if side == AuctionSide::Low {
                    offset
                } else {
                    levels.len() - 1 - offset
                };
                let previous = if side == AuctionSide::Low {
                    levels[index - 1].bid_volume
                } else {
                    levels[index + 1].ask_volume
                };
                let next = if side == AuctionSide::Low {
                    levels[index].bid_volume
                } else {
                    levels[index].ask_volume
                };
                next > previous
            })
        {
            marks.push(AuctionMark {
                bar_time: 0,
                kind: AuctionMarkKind::Exhaustion,
                side,
                price: edge.price,
                volume,
            });
        }
        let candidates = (0..options.extreme_levels.min(levels.len()))
            .filter_map(|distance| {
                let index = if side == AuctionSide::Low {
                    distance
                } else {
                    levels.len() - 1 - distance
                };
                let level = &levels[index];
                let (aggressor, opposite) = if side == AuctionSide::Low {
                    (level.bid_volume, level.ask_volume)
                } else {
                    (level.ask_volume, level.bid_volume)
                };
                let rejected = if side == AuctionSide::Low {
                    levels
                        .iter()
                        .filter(|row| row.price > level.price && row.price <= bar.close)
                        .count()
                        >= options.min_rejection_rows
                } else {
                    levels
                        .iter()
                        .filter(|row| row.price < level.price && row.price >= bar.close)
                        .count()
                        >= options.min_rejection_rows
                };
                (aggressor > 0.0
                    && aggressor >= options.absorption_min_volume
                    && aggressor >= options.absorption_ratio * opposite
                    && rejected)
                    .then_some((distance, level.price, aggressor))
            })
            .max_by(|a, b| a.2.total_cmp(&b.2).then_with(|| b.0.cmp(&a.0)));
        if let Some((_, price, volume)) = candidates {
            marks.push(AuctionMark {
                bar_time: 0,
                kind: AuctionMarkKind::Absorption,
                side,
                price,
                volume,
            });
        }
    }
    marks
}

#[derive(Clone, Debug)]
struct BarMarks {
    start_micros: i64,
    time: i64,
    marks: Vec<AuctionMark>,
}

#[derive(Clone, Debug)]
pub(crate) struct AuctionMarkerIndicator {
    id: NativePrimitiveId,
    pub(crate) series_id: SeriesId,
    options: AuctionMarkerOptions,
    bars: Vec<BarMarks>,
    /// Number of bars evaluated in the most recent refresh, for bounded-work tests.
    #[cfg(test)]
    refreshed_bars: usize,
}

fn bar_time(bar: &FootprintBar, sequence: bool) -> i64 {
    if sequence {
        bar.logical_index as i64
    } else {
        bar.start_timestamp_micros.div_euclid(MICROS_PER_SECOND)
    }
}

impl AuctionMarkerIndicator {
    fn refresh(&mut self, bars: &[FootprintBar], sequence: bool, from: Option<usize>) {
        // A newly closed former tip needs evaluation. Historical corrections invalidate only
        // their suffix; retention is a separate full projection with no surviving stale marks.
        let start = from
            .map_or(0, |index| {
                if bars.len() > self.bars.len() {
                    index.min(self.bars.len().saturating_sub(1))
                } else {
                    index
                }
            })
            .min(self.bars.len())
            .min(bars.len());
        self.bars.truncate(start);
        for (index, bar) in bars.iter().enumerate().skip(start) {
            let mut marks = if index + 1 == bars.len() && !self.options.include_forming_bar {
                Vec::new()
            } else {
                detect(bar, &self.options)
            };
            for mark in &mut marks {
                mark.bar_time = bar_time(bar, sequence);
            }
            self.bars.push(BarMarks {
                start_micros: bar.start_timestamp_micros,
                time: bar_time(bar, sequence),
                marks,
            });
        }
        #[cfg(test)]
        {
            self.refreshed_bars = bars.len() - start;
        }
    }
}

impl ChartEngine {
    pub fn add_auction_markers(
        &mut self,
        stream_id: u64,
        series_id: SeriesId,
        options: AuctionMarkerOptions,
    ) -> Result<NativePrimitiveId, FootprintError> {
        let stream = self
            .trade_stream(stream_id)
            .ok_or(FootprintError::UnknownTradeStream(stream_id))?;
        let series = self
            .series_entry(series_id)
            .ok_or(FootprintError::UnknownSeries(series_id))?;
        if !matches!(
            series.kind,
            SeriesKind::Candlestick
                | SeriesKind::Bar
                | SeriesKind::Line
                | SeriesKind::Area
                | SeriesKind::Baseline
                | SeriesKind::Footprint
        ) {
            return Err(FootprintError::UnsupportedBigTradesSeries(series_id));
        }
        if !options.valid() {
            return Err(FootprintError::InvalidAuctionMarkerOptions);
        }
        if self.auction_markers.values().map(Vec::len).sum::<usize>() >= MAX_AUCTION_MARKERS {
            return Err(FootprintError::AuctionMarkerCapacity);
        }
        let mut indicator = AuctionMarkerIndicator {
            id: self.next_native_primitive_id,
            series_id,
            options,
            bars: Vec::new(),
            #[cfg(test)]
            refreshed_bars: 0,
        };
        let sequence = !matches!(stream.options().bars, FootprintBarAggregation::Time { .. });
        indicator.refresh(stream.bars(), sequence, None);
        self.next_native_primitive_id = self
            .next_native_primitive_id
            .checked_add(1)
            .ok_or(FootprintError::AuctionMarkerCapacity)?;
        let id = indicator.id;
        self.auction_markers
            .entry(stream_id)
            .or_default()
            .push(indicator);
        self.invalidate_frame_series(series_id);
        Ok(id)
    }

    pub fn set_auction_marker_options(
        &mut self,
        id: NativePrimitiveId,
        options: AuctionMarkerOptions,
    ) -> Result<(), FootprintError> {
        if !options.valid() {
            return Err(FootprintError::InvalidAuctionMarkerOptions);
        }
        let (stream_id, indicator) = self
            .auction_markers
            .iter_mut()
            .find_map(|(stream, entries)| {
                entries
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .map(|entry| (*stream, entry))
            })
            .ok_or(FootprintError::UnknownAuctionMarkers(id))?;
        if indicator.options != options {
            let stream = self
                .trade_streams
                .get(&stream_id)
                .ok_or(FootprintError::UnknownTradeStream(stream_id))?;
            let sequence = !matches!(stream.options().bars, FootprintBarAggregation::Time { .. });
            indicator.options = options;
            indicator.refresh(stream.bars(), sequence, None);
        }
        let series_id = indicator.series_id;
        self.invalidate_frame_series(series_id);
        Ok(())
    }

    pub fn auction_marker_options(&self, id: NativePrimitiveId) -> Option<&AuctionMarkerOptions> {
        self.auction_markers
            .values()
            .flatten()
            .find(|entry| entry.id == id)
            .map(|entry| &entry.options)
    }

    pub fn auction_markers_snapshot(
        &self,
        id: NativePrimitiveId,
    ) -> Result<Vec<AuctionMark>, FootprintError> {
        let indicator = self
            .auction_markers
            .values()
            .flatten()
            .find(|entry| entry.id == id)
            .ok_or(FootprintError::UnknownAuctionMarkers(id))?;
        Ok(indicator
            .bars
            .iter()
            .flat_map(|bar| bar.marks.iter().copied())
            .collect())
    }

    pub fn remove_auction_markers(&mut self, id: NativePrimitiveId) -> Result<(), FootprintError> {
        let (stream_id, series_id) = self
            .auction_markers
            .iter_mut()
            .find_map(|(stream, entries)| {
                let index = entries.iter().position(|entry| entry.id == id)?;
                Some((*stream, entries.remove(index).series_id))
            })
            .ok_or(FootprintError::UnknownAuctionMarkers(id))?;
        self.auction_markers
            .retain(|_, entries| !entries.is_empty());
        self.invalidate_frame_series(series_id);
        self.prune_trade_stream_if_unused(stream_id);
        Ok(())
    }

    pub(crate) fn auction_markers_count(&self, stream_id: u64) -> usize {
        self.auction_markers.get(&stream_id).map_or(0, Vec::len)
    }

    pub(crate) fn refresh_auction_markers(&mut self, stream_id: u64, from: Option<usize>) {
        let (Some(stream), Some(entries)) = (
            self.trade_streams.get(&stream_id),
            self.auction_markers.get_mut(&stream_id),
        ) else {
            return;
        };
        let sequence = !matches!(stream.options().bars, FootprintBarAggregation::Time { .. });
        let ids = entries
            .iter_mut()
            .map(|entry| {
                entry.refresh(stream.bars(), sequence, from);
                entry.series_id
            })
            .collect::<Vec<_>>();
        for id in ids {
            self.invalidate_frame_series(id);
        }
    }

    pub(crate) fn build_auction_markers_frame(
        &self,
        series_id: SeriesId,
        from: i64,
        to: i64,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let Some(series) = self.series_entry(series_id).filter(|series| series.visible) else {
            return;
        };
        let Some(pane) = self.panes.get(series.pane_index) else {
            return;
        };
        let scale = pane_scale(pane, series_scale_target(series));
        if scale.is_empty() {
            return;
        }
        let Some(base) = self.series_base_value(series_id, from) else {
            return;
        };
        let font = &self.options.get().layout;
        let color = Color::rgb(230, 170, 60);
        for (stream_id, entries) in &self.auction_markers {
            let Some(stream) = self.trade_stream(*stream_id) else {
                continue;
            };
            let sequence = !matches!(stream.options().bars, FootprintBarAggregation::Time { .. });
            for entry in entries
                .iter()
                .filter(|entry| entry.series_id == series_id && entry.options.visible)
            {
                let to_index = |bar: &BarMarks| {
                    if sequence {
                        Some(bar.time)
                    } else {
                        self.time_to_index(bar.time as f64, true)
                    }
                };
                let first = entry
                    .bars
                    .partition_point(|bar| to_index(bar).is_none_or(|index| index < from));
                let last = entry
                    .bars
                    .partition_point(|bar| to_index(bar).is_some_and(|index| index <= to));
                for bar in &entry.bars[first..last.max(first)] {
                    for mark in &bar.marks {
                        let index = to_index(bar);
                        let Some(index) = index.filter(|index| (from..=to).contains(index)) else {
                            continue;
                        };
                        let x = (self.time_scale.index_to_coordinate(index) + 6.0) * hpr;
                        let y = scale.price_to_coordinate(mark.price, base) * vpr;
                        let side = if mark.side == AuctionSide::High {
                            -1.0
                        } else {
                            1.0
                        };
                        match mark.kind {
                            AuctionMarkKind::UnfinishedAuction => {
                                out.push(Prim::Triangle {
                                    a: [x as f32, (y + side * 5.0 * vpr) as f32],
                                    b: [(x - 4.0 * hpr) as f32, (y - side * 3.0 * vpr) as f32],
                                    c: [(x + 4.0 * hpr) as f32, (y - side * 3.0 * vpr) as f32],
                                    color,
                                });
                                if entry.options.extend_until_revisited {
                                    let after = stream.bars().partition_point(|candidate| {
                                        candidate.start_timestamp_micros <= bar.start_micros
                                    });
                                    // A ray beyond the viewport ends at the pane edge. Do not
                                    // scan invisible future history to construct this frame.
                                    let visible_end = stream.bars().partition_point(|candidate| {
                                        let time = bar_time(candidate, sequence);
                                        let index = if sequence {
                                            Some(time)
                                        } else {
                                            self.time_to_index(time as f64, true)
                                        };
                                        index.is_some_and(|index| index <= to)
                                    });
                                    let end = stream.bars()[after..visible_end.max(after)]
                                        .iter()
                                        .find(|candidate| {
                                            candidate.low <= mark.price
                                                && mark.price <= candidate.high
                                        });
                                    let end_x = end
                                        .map(|candidate| bar_time(candidate, sequence))
                                        .and_then(|time| {
                                            if sequence {
                                                Some(time)
                                            } else {
                                                self.time_to_index(time as f64, true)
                                            }
                                        })
                                        .map(|index| {
                                            if index > to {
                                                self.pane_w * hpr
                                            } else {
                                                self.time_scale.index_to_coordinate(index) * hpr
                                            }
                                        })
                                        .unwrap_or(self.pane_w * hpr);
                                    out.push(Prim::HLine {
                                        y: y.round() as i32,
                                        x0: x.round() as i32,
                                        x1: end_x.round().max(x.round() + 1.0) as i32,
                                        width: hpr.round().max(1.0) as i32,
                                        style: LineStyle::Dashed,
                                        color,
                                    });
                                }
                            }
                            AuctionMarkKind::Exhaustion => out.push(Prim::Circle {
                                cx: x as f32,
                                cy: y as f32,
                                radius: (3.0 * hpr) as f32,
                                fill: color,
                                stroke_width: 0.0,
                                stroke: color,
                            }),
                            AuctionMarkKind::Absorption => {
                                out.push(Prim::RectFrame {
                                    rect: IRect {
                                        x: (x - 4.0 * hpr).round() as i32,
                                        y: (y - 4.0 * vpr).round() as i32,
                                        w: (8.0 * hpr).round().max(1.0) as i32,
                                        h: (8.0 * vpr).round().max(1.0) as i32,
                                    },
                                    border: hpr.round().max(1.0) as i32,
                                    color,
                                });
                                if self.time_scale.bar_spacing() >= 6.0 {
                                    out.push(Prim::Text {
                                        x: (x + 6.0 * hpr) as f32,
                                        y: y as f32,
                                        text: "ABS".into(),
                                        color,
                                        size: (font.font_size * vpr) as f32,
                                        family: font.font_family.clone(),
                                        align: TextAlign::Left,
                                        weight: 600,
                                        italic: false,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::footprint::{
        AggressorSide, FootprintImbalanceOptions, FootprintLevel, FootprintTrade,
        merged_footprint_bar,
    };
    use crate::{
        BigTradesFilter, BigTradesOptions, FootprintAggregationOptions, FootprintSeriesOptions,
        FootprintVisualOptions, OrderFlowPresentationOptions,
    };

    fn level(price: f64, bid: f64, ask: f64) -> FootprintLevel {
        FootprintLevel {
            level: price as i64,
            price,
            bid_volume: bid,
            ask_volume: ask,
            ..FootprintLevel::default()
        }
    }

    fn mark(kind: AuctionMarkKind, side: AuctionSide, price: f64, volume: f64) -> AuctionMark {
        AuctionMark {
            bar_time: 0,
            kind,
            side,
            price,
            volume,
        }
    }

    #[test]
    fn unfinished_auction_requires_both_sides_at_the_canonical_extreme() {
        let mut bar = FootprintBar {
            close: 101.0,
            levels: vec![level(100.0, 0.0, 5.0), level(101.0, 5.0, 0.0)],
            ..FootprintBar::default()
        };
        assert_eq!(detect(&bar, &AuctionMarkerOptions::default()), vec![]);
        bar.levels = vec![level(100.0, 2.0, 3.0), level(101.0, 3.0, 2.0)];
        assert_eq!(
            detect(&bar, &AuctionMarkerOptions::default()),
            vec![
                mark(
                    AuctionMarkKind::UnfinishedAuction,
                    AuctionSide::Low,
                    100.0,
                    5.0
                ),
                mark(
                    AuctionMarkKind::UnfinishedAuction,
                    AuctionSide::High,
                    101.0,
                    5.0
                ),
            ]
        );
        assert_eq!(
            detect(
                &bar,
                &AuctionMarkerOptions {
                    min_side_volume: 4.0,
                    ..AuctionMarkerOptions::default()
                }
            ),
            vec![]
        );
    }

    #[test]
    fn exhaustion_is_strict_on_both_sides_and_plateaus_do_not_qualify() {
        let mut high = FootprintBar {
            close: 103.0,
            levels: vec![
                level(100.0, 0.0, 0.0),
                level(101.0, 0.0, 8.0),
                level(102.0, 0.0, 4.0),
                level(103.0, 0.0, 2.0),
            ],
            ..FootprintBar::default()
        };
        assert_eq!(
            detect(&high, &AuctionMarkerOptions::default()),
            vec![mark(
                AuctionMarkKind::Exhaustion,
                AuctionSide::High,
                103.0,
                2.0
            )]
        );
        high.levels[2].ask_volume = 2.0;
        assert_eq!(detect(&high, &AuctionMarkerOptions::default()), vec![]);

        let low = FootprintBar {
            close: 103.0,
            levels: vec![
                level(100.0, 2.0, 0.0),
                level(101.0, 4.0, 0.0),
                level(102.0, 8.0, 0.0),
                level(103.0, 0.0, 0.0),
            ],
            ..FootprintBar::default()
        };
        assert_eq!(
            detect(&low, &AuctionMarkerOptions::default()),
            vec![mark(
                AuctionMarkKind::Exhaustion,
                AuctionSide::Low,
                100.0,
                2.0
            )]
        );
    }

    #[test]
    fn absorption_chooses_largest_then_nearest_and_requires_rejection_rows() {
        let low = FootprintBar {
            close: 103.0,
            levels: vec![
                level(100.0, 120.0, 20.0),
                level(101.0, 120.0, 20.0),
                level(102.0, 0.0, 0.0),
                level(103.0, 0.0, 0.0),
            ],
            ..FootprintBar::default()
        };
        assert_eq!(
            detect(&low, &AuctionMarkerOptions::default()),
            vec![
                mark(
                    AuctionMarkKind::UnfinishedAuction,
                    AuctionSide::Low,
                    100.0,
                    140.0
                ),
                mark(AuctionMarkKind::Absorption, AuctionSide::Low, 100.0, 120.0),
            ]
        );
        let mut larger_low = low.clone();
        larger_low.levels[1].bid_volume = 150.0;
        assert_eq!(
            detect(&larger_low, &AuctionMarkerOptions::default()),
            vec![
                mark(
                    AuctionMarkKind::UnfinishedAuction,
                    AuctionSide::Low,
                    100.0,
                    140.0
                ),
                mark(AuctionMarkKind::Absorption, AuctionSide::Low, 101.0, 150.0),
            ]
        );
        let high = FootprintBar {
            close: 100.0,
            levels: vec![
                level(100.0, 0.0, 0.0),
                level(101.0, 0.0, 0.0),
                level(102.0, 20.0, 120.0),
                level(103.0, 20.0, 120.0),
            ],
            ..FootprintBar::default()
        };
        assert_eq!(
            detect(&high, &AuctionMarkerOptions::default()),
            vec![
                mark(
                    AuctionMarkKind::UnfinishedAuction,
                    AuctionSide::High,
                    103.0,
                    140.0
                ),
                mark(AuctionMarkKind::Absorption, AuctionSide::High, 103.0, 120.0),
            ]
        );
        let mut larger_high = high.clone();
        larger_high.levels[2].ask_volume = 150.0;
        assert_eq!(
            detect(&larger_high, &AuctionMarkerOptions::default()),
            vec![
                mark(
                    AuctionMarkKind::UnfinishedAuction,
                    AuctionSide::High,
                    103.0,
                    140.0
                ),
                mark(AuctionMarkKind::Absorption, AuctionSide::High, 102.0, 150.0),
            ]
        );
        let mut not_rejected = high.clone();
        not_rejected.close = 102.0;
        assert_eq!(
            detect(
                &not_rejected,
                &AuctionMarkerOptions {
                    min_rejection_rows: 2,
                    ..AuctionMarkerOptions::default()
                }
            ),
            vec![mark(
                AuctionMarkKind::UnfinishedAuction,
                AuctionSide::High,
                103.0,
                140.0
            )]
        );
    }

    #[test]
    fn auction_detection_ignores_display_row_merging() {
        let (mut chart, stream) = setup();
        chart
            .set_trade_stream_trades(
                stream,
                vec![
                    trade(1_000_000, 100.0, 5.0, AggressorSide::Buy),
                    trade(1_000_001, 101.0, 5.0, AggressorSide::Sell),
                    trade(61_000_000, 102.0, 1.0, AggressorSide::Buy),
                ],
            )
            .unwrap();
        let canonical = &chart.trade_stream(stream).unwrap().bars()[0];
        assert_eq!(canonical.levels.len(), 2);
        assert_eq!(detect(canonical, &AuctionMarkerOptions::default()), vec![]);
        let merged = merged_footprint_bar(canonical, 2, FootprintImbalanceOptions::default(), 2.0);
        assert_eq!(
            detect(&merged, &AuctionMarkerOptions::default()),
            vec![
                mark(
                    AuctionMarkKind::UnfinishedAuction,
                    AuctionSide::Low,
                    100.0,
                    10.0
                ),
                mark(
                    AuctionMarkKind::UnfinishedAuction,
                    AuctionSide::High,
                    100.0,
                    10.0
                ),
            ]
        );
        let id = chart
            .add_auction_markers(stream, 0, AuctionMarkerOptions::default())
            .unwrap();
        assert_eq!(chart.auction_markers_snapshot(id).unwrap(), vec![]);
    }

    fn trade(time: i64, price: f64, volume: f64, side: AggressorSide) -> FootprintTrade {
        FootprintTrade {
            timestamp_micros: time,
            price,
            volume,
            aggressor: side,
            bid: None,
            ask: None,
            sequence: None,
            trade_id: None,
            conditions: 0,
            session_id: Some(1),
        }
    }

    fn setup() -> (ChartEngine, u64) {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let stream = chart
            .add_trade_stream(
                "auction",
                FootprintAggregationOptions {
                    tick_size: 1.0,
                    ..FootprintAggregationOptions::default()
                },
            )
            .unwrap();
        chart
            .configure_footprint_series(0, FootprintSeriesOptions::default())
            .unwrap();
        chart.bind_footprint_series_to_stream(0, stream).unwrap();
        (chart, stream)
    }

    fn presentation_options(rows: u32, footprint: bool) -> OrderFlowPresentationOptions {
        OrderFlowPresentationOptions {
            aggregation: FootprintAggregationOptions {
                tick_size: 1.0,
                ticks_per_row: rows,
                ..FootprintAggregationOptions::default()
            },
            visual: FootprintVisualOptions::default(),
            show_footprint: footprint,
            show_cumulative_delta: false,
            show_delta_histogram: false,
            big_trades: None,
        }
    }

    #[test]
    fn reconfiguring_order_flow_keeps_canonical_auction_marks_and_history() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let mut presentation = chart
            .add_order_flow_presentation("auction", 0, presentation_options(1, true))
            .unwrap();
        let stream = presentation.trade_stream();
        let tape = (0..12)
            .flat_map(|row| {
                let time = row * 60_000_000 + 1;
                [
                    trade(time, 100.0, 3.0, AggressorSide::Buy),
                    trade(time + 1, 100.0, 4.0, AggressorSide::Sell),
                ]
            })
            .collect::<Vec<_>>();
        chart
            .update_order_flow_presentation(presentation, tape.clone(), false)
            .unwrap();
        let id = chart
            .add_auction_markers(stream, 0, AuctionMarkerOptions::default())
            .unwrap();
        let initial = chart.auction_markers_snapshot(id).unwrap();
        assert_eq!(initial.len(), 22);
        chart
            .reconfigure_order_flow_presentation(&mut presentation, presentation_options(4, false))
            .unwrap();
        assert_eq!(chart.auction_markers_snapshot(id).unwrap(), initial);
        chart
            .reconfigure_order_flow_presentation(&mut presentation, presentation_options(2, true))
            .unwrap();
        assert_eq!(
            chart
                .trade_stream(stream)
                .unwrap()
                .trades()
                .cloned()
                .collect::<Vec<_>>(),
            tape
        );
        assert_eq!(chart.auction_markers_snapshot(id).unwrap(), initial);

        let mut fresh = ChartEngine::new(800.0, 500.0, 1.0);
        let fresh_presentation = fresh
            .add_order_flow_presentation("auction", 0, presentation_options(2, true))
            .unwrap();
        fresh
            .update_order_flow_presentation(fresh_presentation, tape, false)
            .unwrap();
        let fresh_id = fresh
            .add_auction_markers(
                fresh_presentation.trade_stream(),
                0,
                AuctionMarkerOptions::default(),
            )
            .unwrap();
        assert_eq!(
            chart.auction_markers_snapshot(id),
            fresh.auction_markers_snapshot(fresh_id)
        );
    }

    #[test]
    fn rewritten_window_repairs_only_auction_suffix_even_when_it_shrinks() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let presentation = chart
            .add_order_flow_presentation("auction", 0, presentation_options(1, true))
            .unwrap();
        let stream = presentation.trade_stream();
        let mut tape = (0..80)
            .flat_map(|row| {
                let time = row * 60_000_000 + 1;
                [
                    trade(time, 100.0, 3.0, AggressorSide::Buy),
                    trade(time + 1, 100.0, 4.0, AggressorSide::Sell),
                ]
            })
            .collect::<Vec<_>>();
        chart
            .update_order_flow_presentation(presentation, tape.clone(), false)
            .unwrap();
        let id = chart
            .add_auction_markers(stream, 0, AuctionMarkerOptions::default())
            .unwrap();
        let prefix = chart.auction_markers_snapshot(id).unwrap();
        let replacement = vec![
            trade(73 * 60_000_000 + 1, 100.0, 5.0, AggressorSide::Sell),
            trade(74 * 60_000_000 + 1, 101.0, 8.0, AggressorSide::Buy),
            trade(74 * 60_000_000 + 2, 101.0, 3.0, AggressorSide::Sell),
            trade(75 * 60_000_000 + 1, 102.0, 2.0, AggressorSide::Buy),
        ];
        chart
            .replace_order_flow_window(presentation, replacement.clone())
            .unwrap();
        tape.retain(|print| print.timestamp_micros < replacement[0].timestamp_micros);
        tape.extend(replacement);
        assert!(
            chart.auction_markers[&stream][0].refreshed_bars <= 7,
            "repair must not visit the preserved 73-bar prefix"
        );
        assert_eq!(
            &chart.auction_markers_snapshot(id).unwrap()[..146],
            &prefix[..146]
        );
        let mut fresh = ChartEngine::new(800.0, 500.0, 1.0);
        let fresh_presentation = fresh
            .add_order_flow_presentation("auction", 0, presentation_options(1, true))
            .unwrap();
        fresh
            .update_order_flow_presentation(fresh_presentation, tape, false)
            .unwrap();
        let fresh_id = fresh
            .add_auction_markers(
                fresh_presentation.trade_stream(),
                0,
                AuctionMarkerOptions::default(),
            )
            .unwrap();
        assert_eq!(
            chart.auction_markers_snapshot(id),
            fresh.auction_markers_snapshot(fresh_id)
        );
        assert_eq!(
            chart.footprint_bars(presentation.footprint_series().unwrap()),
            fresh.footprint_bars(fresh_presentation.footprint_series().unwrap())
        );
    }

    #[test]
    fn lifecycle_capacity_validation_and_runtime_only_export() {
        let (mut chart, stream) = setup();
        assert_eq!(
            chart.add_auction_markers(stream + 1, 0, AuctionMarkerOptions::default()),
            Err(FootprintError::UnknownTradeStream(stream + 1))
        );
        let invalid = AuctionMarkerOptions {
            exhaustion_levels: 1,
            ..AuctionMarkerOptions::default()
        };
        assert_eq!(
            chart.add_auction_markers(stream, 0, invalid.clone()),
            Err(FootprintError::InvalidAuctionMarkerOptions)
        );
        let ids = (0..MAX_AUCTION_MARKERS)
            .map(|_| {
                chart
                    .add_auction_markers(stream, 0, AuctionMarkerOptions::default())
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            chart.add_auction_markers(stream, 0, AuctionMarkerOptions::default()),
            Err(FootprintError::AuctionMarkerCapacity)
        );
        assert_eq!(
            chart.set_auction_marker_options(ids[0], invalid),
            Err(FootprintError::InvalidAuctionMarkerOptions)
        );
        assert_eq!(
            chart.auction_marker_options(ids[0]),
            Some(&AuctionMarkerOptions::default())
        );
        let export = chart.export_state_json().unwrap();
        assert!(!export.contains("auction_marker"));
        chart.remove_auction_markers(ids[0]).unwrap();
        assert_eq!(
            chart.auction_markers_snapshot(ids[0]),
            Err(FootprintError::UnknownAuctionMarkers(ids[0]))
        );
        chart
            .add_auction_markers(stream, 0, AuctionMarkerOptions::default())
            .unwrap();
    }

    #[test]
    fn invalid_options_leave_prior_options_and_removed_ids_report_errors() {
        let (mut chart, stream) = setup();
        let original = AuctionMarkerOptions {
            min_side_volume: 2.0,
            include_forming_bar: true,
            ..AuctionMarkerOptions::default()
        };
        let id = chart
            .add_auction_markers(stream, 0, original.clone())
            .unwrap();
        let invalid = [
            AuctionMarkerOptions {
                exhaustion_levels: 9,
                ..original.clone()
            },
            AuctionMarkerOptions {
                extreme_levels: 0,
                ..original.clone()
            },
            AuctionMarkerOptions {
                min_side_volume: -1.0,
                ..original.clone()
            },
            AuctionMarkerOptions {
                min_side_volume: f64::INFINITY,
                ..original.clone()
            },
            AuctionMarkerOptions {
                min_side_volume: f64::NAN,
                ..original.clone()
            },
            AuctionMarkerOptions {
                exhaustion_max_volume: -1.0,
                ..original.clone()
            },
            AuctionMarkerOptions {
                exhaustion_max_volume: f64::NAN,
                ..original.clone()
            },
            AuctionMarkerOptions {
                exhaustion_max_volume: f64::INFINITY,
                ..original.clone()
            },
            AuctionMarkerOptions {
                absorption_min_volume: -1.0,
                ..original.clone()
            },
            AuctionMarkerOptions {
                absorption_min_volume: f64::NEG_INFINITY,
                ..original.clone()
            },
            AuctionMarkerOptions {
                absorption_min_volume: f64::NAN,
                ..original.clone()
            },
            AuctionMarkerOptions {
                absorption_ratio: -1.0,
                ..original.clone()
            },
            AuctionMarkerOptions {
                absorption_ratio: f64::NAN,
                ..original.clone()
            },
            AuctionMarkerOptions {
                absorption_ratio: f64::INFINITY,
                ..original.clone()
            },
        ];
        for candidate in invalid {
            assert_eq!(
                chart.set_auction_marker_options(id, candidate.clone()),
                Err(FootprintError::InvalidAuctionMarkerOptions),
                "{candidate:?}"
            );
            assert_eq!(chart.auction_marker_options(id), Some(&original));
            assert_eq!(
                chart.add_auction_markers(stream, 0, candidate.clone()),
                Err(FootprintError::InvalidAuctionMarkerOptions),
                "{candidate:?}"
            );
            assert_eq!(chart.auction_marker_options(id), Some(&original));
        }
        chart.remove_auction_markers(id).unwrap();
        assert_eq!(chart.auction_marker_options(id), None);
        assert_eq!(
            chart.set_auction_marker_options(id, original),
            Err(FootprintError::UnknownAuctionMarkers(id))
        );
        assert_eq!(
            chart.auction_markers_snapshot(id),
            Err(FootprintError::UnknownAuctionMarkers(id))
        );
        assert_eq!(
            chart.remove_auction_markers(id),
            Err(FootprintError::UnknownAuctionMarkers(id))
        );
    }

    #[test]
    fn forming_late_print_replay_and_shared_dependents() {
        let (mut chart, stream) = setup();
        let tape = vec![
            trade(1_000_000, 100.0, 2.0, AggressorSide::Buy),
            trade(1_000_001, 100.0, 2.0, AggressorSide::Sell),
            trade(61_000_000, 101.0, 2.0, AggressorSide::Buy),
            trade(61_000_001, 101.0, 2.0, AggressorSide::Sell),
            trade(121_000_000, 102.0, 2.0, AggressorSide::Buy),
        ];
        chart.set_trade_stream_trades(stream, tape.clone()).unwrap();
        let markers = chart
            .add_auction_markers(stream, 0, AuctionMarkerOptions::default())
            .unwrap();
        let big = chart
            .add_big_trades(
                stream,
                0,
                BigTradesOptions {
                    filter: BigTradesFilter::Fixed {
                        minimum_volume: 1.0,
                    },
                    ..BigTradesOptions::default()
                },
            )
            .unwrap();
        assert_eq!(
            chart
                .auction_markers_snapshot(markers)
                .unwrap()
                .iter()
                .filter(|mark| mark.kind == AuctionMarkKind::UnfinishedAuction)
                .count(),
            4
        );
        assert!(!chart.big_trades_snapshot(big).unwrap().bubbles.is_empty());
        chart
            .update_trade_stream_trade(stream, trade(61_000_002, 101.0, 5.0, AggressorSide::Buy))
            .unwrap();
        let entry = &chart.auction_markers[&stream][0];
        assert!(
            entry.refreshed_bars <= 2,
            "late print refreshed {} bars",
            entry.refreshed_bars
        );
        let mut fresh = setup().0;
        let fresh_stream = fresh.trade_stream_id("auction").unwrap();
        let mut final_tape = tape;
        final_tape.push(trade(61_000_002, 101.0, 5.0, AggressorSide::Buy));
        fresh
            .set_trade_stream_trades(fresh_stream, final_tape.clone())
            .unwrap();
        let fresh_id = fresh
            .add_auction_markers(fresh_stream, 0, AuctionMarkerOptions::default())
            .unwrap();
        let fresh_big = fresh
            .add_big_trades(
                fresh_stream,
                0,
                BigTradesOptions {
                    filter: BigTradesFilter::Fixed {
                        minimum_volume: 1.0,
                    },
                    ..BigTradesOptions::default()
                },
            )
            .unwrap();
        assert_eq!(chart.footprint_bars(0), fresh.footprint_bars(0));
        assert_eq!(
            chart.big_trades_snapshot(big),
            fresh.big_trades_snapshot(fresh_big)
        );
        assert_eq!(
            chart.auction_markers_snapshot(markers),
            fresh.auction_markers_snapshot(fresh_id)
        );
        chart
            .set_trade_stream_replay_clock_micros(stream, Some(61_000_002))
            .unwrap();
        assert!(
            chart
                .auction_markers_snapshot(markers)
                .unwrap()
                .iter()
                .all(|mark| mark.bar_time <= 61)
        );
        let (mut prefix, prefix_stream) = setup();
        prefix
            .set_trade_stream_trades(
                prefix_stream,
                final_tape
                    .into_iter()
                    .filter(|trade| trade.timestamp_micros <= 61_000_002)
                    .collect(),
            )
            .unwrap();
        let prefix_id = prefix
            .add_auction_markers(prefix_stream, 0, AuctionMarkerOptions::default())
            .unwrap();
        assert_eq!(
            chart.auction_markers_snapshot(markers),
            prefix.auction_markers_snapshot(prefix_id)
        );
        chart
            .set_trade_stream_replay_clock_micros(stream, None)
            .unwrap();
        assert_eq!(
            chart.auction_markers_snapshot(markers),
            fresh.auction_markers_snapshot(fresh_id)
        );
    }

    #[test]
    fn frame_marks_use_shared_chrome_and_visible_bars_only() {
        let (mut chart, stream) = setup();
        chart
            .set_trade_stream_trades(
                stream,
                vec![
                    trade(1_000_000, 100.0, 120.0, AggressorSide::Sell),
                    trade(1_000_001, 100.0, 1.0, AggressorSide::Buy),
                    trade(1_000_002, 101.0, 20.0, AggressorSide::Buy),
                    trade(1_000_003, 102.0, 5.0, AggressorSide::Buy),
                    trade(1_000_004, 103.0, 2.0, AggressorSide::Buy),
                    trade(61_000_000, 101.0, 1.0, AggressorSide::Buy),
                    trade(121_000_000, 102.0, 1.0, AggressorSide::Buy),
                ],
            )
            .unwrap();
        let id = chart
            .add_auction_markers(
                stream,
                0,
                AuctionMarkerOptions {
                    extend_until_revisited: true,
                    ..AuctionMarkerOptions::default()
                },
            )
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        chart.build_frame();
        let mut prims = Vec::new();
        chart.build_auction_markers_frame(0, 0, 0, 1.0, 1.0, &mut prims);
        assert!(
            prims
                .iter()
                .any(|prim| matches!(prim, Prim::Triangle { .. }))
        );
        assert!(prims.iter().any(|prim| matches!(prim, Prim::Circle { .. })));
        assert!(
            prims
                .iter()
                .any(|prim| matches!(prim, Prim::RectFrame { .. }))
        );
        assert!(
            prims
                .iter()
                .any(|prim| matches!(prim, Prim::Text { text, .. } if text == "ABS"))
        );
        assert!(prims.iter().any(|prim| matches!(
            prim,
            Prim::HLine {
                style: LineStyle::Dashed,
                ..
            }
        )));
        prims.clear();
        chart.build_auction_markers_frame(0, 3, 3, 1.0, 1.0, &mut prims);
        assert!(prims.is_empty(), "off-screen bars must emit no chrome");
        chart
            .set_auction_marker_options(
                id,
                AuctionMarkerOptions {
                    visible: false,
                    ..AuctionMarkerOptions::default()
                },
            )
            .unwrap();
        prims.clear();
        chart.build_auction_markers_frame(0, 0, 0, 1.0, 1.0, &mut prims);
        assert!(prims.is_empty());
    }

    #[test]
    fn unfinished_rays_end_on_first_revisiting_bar_and_disabled_option_emits_none() {
        let (mut chart, stream) = setup();
        chart
            .set_trade_stream_trades(
                stream,
                vec![
                    trade(1_000_000, 100.0, 2.0, AggressorSide::Buy),
                    trade(1_000_001, 100.0, 3.0, AggressorSide::Sell),
                    trade(61_000_000, 101.0, 1.0, AggressorSide::Buy),
                    trade(121_000_000, 100.0, 1.0, AggressorSide::Buy),
                    trade(181_000_000, 100.0, 1.0, AggressorSide::Buy),
                ],
            )
            .unwrap();
        let id = chart
            .add_auction_markers(
                stream,
                0,
                AuctionMarkerOptions {
                    extend_until_revisited: true,
                    ..AuctionMarkerOptions::default()
                },
            )
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        chart.build_frame();
        let mut prims = Vec::new();
        chart.build_auction_markers_frame(0, 0, 3, 1.0, 1.0, &mut prims);
        let start_x = (chart.time_scale.index_to_coordinate(0) + 6.0).round() as i32;
        let revisit_x = chart.time_scale.index_to_coordinate(2).round() as i32;
        let rays = prims
            .iter()
            .filter_map(|prim| match prim {
                Prim::HLine {
                    x0,
                    x1,
                    style: LineStyle::Dashed,
                    ..
                } => Some((*x0, *x1)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(rays, vec![(start_x, revisit_x); 2]);
        assert_eq!(
            prims
                .iter()
                .filter(|prim| matches!(prim, Prim::Triangle { .. }))
                .count(),
            2
        );
        chart
            .set_auction_marker_options(id, AuctionMarkerOptions::default())
            .unwrap();
        prims.clear();
        chart.build_auction_markers_frame(0, 0, 3, 1.0, 1.0, &mut prims);
        assert_eq!(
            prims
                .iter()
                .filter(|prim| matches!(prim, Prim::Triangle { .. }))
                .count(),
            2
        );
        assert_eq!(
            prims
                .iter()
                .filter(|prim| matches!(prim, Prim::HLine { .. }))
                .count(),
            0
        );
    }

    #[test]
    fn absorption_frame_suppresses_text_below_six_pixel_bar_spacing() {
        let (mut chart, stream) = setup();
        chart
            .set_trade_stream_trades(
                stream,
                vec![
                    trade(1_000_000, 100.0, 120.0, AggressorSide::Sell),
                    trade(1_000_001, 100.0, 20.0, AggressorSide::Buy),
                    trade(1_000_002, 101.0, 1.0, AggressorSide::Buy),
                    trade(61_000_000, 102.0, 1.0, AggressorSide::Buy),
                ],
            )
            .unwrap();
        let id = chart
            .add_auction_markers(stream, 0, AuctionMarkerOptions::default())
            .unwrap();
        assert_eq!(
            chart.auction_markers_snapshot(id).unwrap(),
            vec![
                mark(
                    AuctionMarkKind::UnfinishedAuction,
                    AuctionSide::Low,
                    100.0,
                    140.0
                ),
                mark(AuctionMarkKind::Absorption, AuctionSide::Low, 100.0, 120.0),
            ]
        );
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        for (spacing, expected_text) in [(5.0, 0), (6.0, 1)] {
            chart.set_bar_spacing(spacing);
            chart.build_frame();
            let mut prims = Vec::new();
            chart.build_auction_markers_frame(0, 0, 0, 1.0, 1.0, &mut prims);
            assert_eq!(
                prims
                    .iter()
                    .filter(|prim| matches!(prim, Prim::RectFrame { .. }))
                    .count(),
                1
            );
            assert_eq!(
                prims
                    .iter()
                    .filter(|prim| matches!(prim, Prim::Text { text, .. } if text == "ABS"))
                    .count(),
                expected_text,
                "bar spacing {spacing}"
            );
        }
    }

    #[test]
    fn forming_option_and_front_retention_keep_mark_times_aligned() {
        let (mut chart, stream) = setup();
        let tape = (0..6)
            .flat_map(|index| {
                let time = index * 60_000_000 + 1;
                [
                    trade(time, 100.0, 3.0, AggressorSide::Buy),
                    trade(time + 1, 100.0, 4.0, AggressorSide::Sell),
                ]
            })
            .collect();
        chart.set_trade_stream_trades(stream, tape).unwrap();
        let id = chart
            .add_auction_markers(stream, 0, AuctionMarkerOptions::default())
            .unwrap();
        assert_eq!(chart.auction_markers_snapshot(id).unwrap().len(), 10);
        chart
            .set_auction_marker_options(
                id,
                AuctionMarkerOptions {
                    include_forming_bar: true,
                    ..AuctionMarkerOptions::default()
                },
            )
            .unwrap();
        assert_eq!(chart.auction_markers_snapshot(id).unwrap().len(), 12);
        let big = chart
            .add_big_trades(
                stream,
                0,
                BigTradesOptions {
                    filter: BigTradesFilter::Fixed {
                        minimum_volume: 1.0,
                    },
                    ..BigTradesOptions::default()
                },
            )
            .unwrap();
        assert!(chart.set_series_max_points(0, Some(3)));
        assert_eq!(chart.footprint_bars(0).unwrap().len(), 3);
        assert!(
            chart
                .big_trades_snapshot(big)
                .unwrap()
                .bubbles
                .iter()
                .all(|order| order.bar_time >= 180)
        );
        let snapshot = chart.auction_markers_snapshot(id).unwrap();
        assert_eq!(snapshot.len(), 6);
        assert_eq!(
            snapshot
                .iter()
                .map(|mark| mark.bar_time)
                .collect::<Vec<_>>(),
            vec![180, 180, 240, 240, 300, 300]
        );
        assert_eq!(chart.auction_markers[&stream][0].bars.len(), 3);
    }

    #[test]
    fn late_tail_repair_does_not_visit_retained_prefix() {
        let (mut chart, stream) = setup();
        let tape = (0..256)
            .flat_map(|row| {
                let time = row * 60_000_000 + 1;
                [
                    trade(time, 100.0, 3.0, AggressorSide::Buy),
                    trade(time + 1, 100.0, 4.0, AggressorSide::Sell),
                ]
            })
            .collect();
        chart.set_trade_stream_trades(stream, tape).unwrap();
        let id = chart
            .add_auction_markers(
                stream,
                0,
                AuctionMarkerOptions {
                    include_forming_bar: true,
                    ..AuctionMarkerOptions::default()
                },
            )
            .unwrap();
        let before = chart.auction_markers_snapshot(id).unwrap();
        chart
            .update_trade_stream_trade(
                stream,
                trade(250 * 60_000_000 + 3, 100.0, 4.0, AggressorSide::Sell),
            )
            .unwrap();
        assert_eq!(
            &chart.auction_markers_snapshot(id).unwrap()[..500],
            &before[..500],
            "unchanged prefix marks must stay identical"
        );
        assert!(
            chart.auction_markers[&stream][0].refreshed_bars <= 6,
            "late print should repair at most its six-bar suffix"
        );
    }
}
