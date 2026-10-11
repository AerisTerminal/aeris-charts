//! Engine-owned financial legend grouping and value projection.

use crate::{ChartEngine, SeriesId, SeriesValueSnapshot};
use std::collections::HashSet;

const MAX_HOST_LEGEND_SERIES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FinancialLegendIdentity {
    Primary,
    Host(u64),
    Indicator(SeriesId),
    ExternalStudy(u64),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FinancialLegendTone {
    #[default]
    Neutral,
    Bullish,
    Bearish,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinancialLegendValue {
    pub text: String,
    pub color: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FinancialLegendRow {
    pub identity: FinancialLegendIdentity,
    pub first_series_id: SeriesId,
    pub pane: usize,
    pub title: String,
    pub values: Vec<FinancialLegendValue>,
    pub tone: FinancialLegendTone,
    pub visible: bool,
    pub settings_available: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct HostLegendSeries<'a> {
    pub identity: u64,
    pub series_id: SeriesId,
    pub title: &'a str,
    pub settings_available: bool,
    /// Use the primary OHLC direction for product series whose value follows the active bar.
    pub tone_from_primary: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct FinancialLegendRequest<'a> {
    pub logical_index: Option<i64>,
    pub primary_title: &'a str,
    pub show_primary_ohlc: bool,
    /// Series whose bar values the primary row reads out, such as a footprint drawn over a
    /// whitespace primary. `None` reads the primary itself.
    pub primary_values_series: Option<SeriesId>,
    /// Product series placed before native indicator groups, such as volume.
    pub leading_series: &'a [HostLegendSeries<'a>],
    /// Product series placed after native indicator groups, such as order-flow studies.
    pub trailing_series: &'a [HostLegendSeries<'a>],
}

impl ChartEngine {
    /// Produce one ordered financial legend model from canonical series, indicator bindings,
    /// external-study groups, and a bounded list of explicitly product-owned series roles.
    #[must_use]
    pub fn financial_legend(&self, request: FinancialLegendRequest<'_>) -> Vec<FinancialLegendRow> {
        let snapshots = self.value_snapshot(request.logical_index);
        let mut rows = Vec::new();
        let values_series = request.primary_values_series.unwrap_or(0);
        if let Some(primary) = self.series_entry(0) {
            let snapshot = snapshot_for(&snapshots, values_series);
            rows.push(FinancialLegendRow {
                identity: FinancialLegendIdentity::Primary,
                first_series_id: 0,
                pane: primary.pane_index,
                title: if request.primary_title.is_empty() {
                    if primary.title.is_empty() {
                        "Asset".to_string()
                    } else {
                        primary.title.clone()
                    }
                } else {
                    request.primary_title.to_string()
                },
                values: if primary.visible && request.show_primary_ohlc {
                    snapshot.map_or_else(Vec::new, ohlc_values)
                } else {
                    Vec::new()
                },
                tone: snapshot.map_or(FinancialLegendTone::Neutral, snapshot_tone),
                visible: primary.visible,
                settings_available: false,
            });
        }
        append_host_rows(
            self,
            &snapshots,
            values_series,
            request.leading_series,
            &mut rows,
        );

        let mut emitted_bindings = HashSet::new();
        for &series_id in self.series_order() {
            let Some(info) = self.indicator_info(series_id) else {
                continue;
            };
            if !emitted_bindings.insert(info.binding_id) {
                continue;
            }
            let outputs = self
                .series_entries()
                .iter()
                .filter(|series| {
                    !series.removed
                        && self
                            .indicator_info(series.id)
                            .is_some_and(|output| output.binding_id == info.binding_id)
                })
                .collect::<Vec<_>>();
            let Some(first) = outputs.first() else {
                continue;
            };
            let visible = outputs.iter().any(|series| series.visible);
            let values =
                if visible {
                    outputs
                        .iter()
                        .filter(|series| series.visible)
                        .filter_map(|series| {
                            let output = self.indicator_info(series.id)?;
                            let value = snapshot_value(&snapshots, series.id)?;
                            // Ribbon lines and volume columns read by their color alone.
                            let unlabeled = matches!(output.kind.as_ref(), "ema_ribbon" | "volume");
                            Some(FinancialLegendValue {
                                text: if output.output_count > 1 && !unlabeled {
                                    format!("{} {value}", output.output_name)
                                } else {
                                    value
                                },
                                color: Some(
                                    histogram_value_color(self, &snapshots, series.id)
                                        .unwrap_or_else(|| {
                                            series.line_color.clone().unwrap_or_else(|| {
                                                crate::DEFAULT_LINE_COLOR.to_css()
                                            })
                                        }),
                                ),
                            })
                        })
                        .collect()
                } else {
                    Vec::new()
                };
            rows.push(FinancialLegendRow {
                identity: FinancialLegendIdentity::Indicator(info.binding_id),
                first_series_id: first.id,
                pane: first.pane_index,
                title: if info.kind == "ema_ribbon" {
                    "EMA Ribbon".to_string()
                } else {
                    first.title.clone()
                },
                values,
                tone: FinancialLegendTone::Neutral,
                visible,
                settings_available: false,
            });
        }

        append_host_rows(
            self,
            &snapshots,
            values_series,
            request.trailing_series,
            &mut rows,
        );

        let outputs = self.external_study_outputs();
        let mut emitted_studies = HashSet::new();
        for output in &outputs {
            if !emitted_studies.insert(output.study_id) {
                continue;
            }
            let group = outputs
                .iter()
                .filter(|candidate| candidate.study_id == output.study_id)
                .filter_map(|candidate| {
                    self.series_entry(candidate.series_id)
                        .map(|series| (candidate, series))
                })
                .collect::<Vec<_>>();
            let Some((first_output, first_series)) = group.first().copied() else {
                continue;
            };
            let visible = group.iter().any(|(_, series)| series.visible);
            let values = if visible {
                group
                    .iter()
                    .filter(|(_, series)| series.visible)
                    .filter_map(|(candidate, series)| {
                        let value = snapshot_value(&snapshots, candidate.series_id)?;
                        Some(FinancialLegendValue {
                            text: candidate
                                .legend_label
                                .as_ref()
                                .map_or(value.clone(), |label| format!("{label} {value}")),
                            color: Some(
                                series
                                    .line_color
                                    .clone()
                                    .unwrap_or_else(|| crate::DEFAULT_LINE_COLOR.to_css()),
                            ),
                        })
                    })
                    .collect()
            } else {
                Vec::new()
            };
            rows.push(FinancialLegendRow {
                identity: FinancialLegendIdentity::ExternalStudy(output.study_id),
                first_series_id: first_output.series_id,
                pane: first_series.pane_index,
                title: first_series.title.clone(),
                values,
                tone: FinancialLegendTone::Neutral,
                visible,
                settings_available: group
                    .iter()
                    .any(|(candidate, _)| candidate.settings_available),
            });
        }
        rows
    }
}

fn append_host_rows(
    chart: &ChartEngine,
    snapshots: &[SeriesValueSnapshot],
    primary_values_series: SeriesId,
    descriptors: &[HostLegendSeries<'_>],
    rows: &mut Vec<FinancialLegendRow>,
) {
    for descriptor in descriptors.iter().take(MAX_HOST_LEGEND_SERIES) {
        let Some(series) = chart.series_entry(descriptor.series_id) else {
            continue;
        };
        let color = histogram_value_color(chart, snapshots, series.id);
        rows.push(FinancialLegendRow {
            identity: FinancialLegendIdentity::Host(descriptor.identity),
            first_series_id: series.id,
            pane: series.pane_index,
            title: descriptor.title.to_string(),
            values: if series.visible {
                snapshot_value(snapshots, series.id)
                    .map_or_else(Vec::new, |text| vec![FinancialLegendValue { text, color }])
            } else {
                Vec::new()
            },
            tone: if descriptor.tone_from_primary {
                snapshot_for(snapshots, primary_values_series)
                    .map_or(FinancialLegendTone::Neutral, snapshot_tone)
            } else {
                FinancialLegendTone::Neutral
            },
            visible: series.visible,
            settings_available: descriptor.settings_available,
        });
    }
}

/// A histogram value reads in its column's hue, so an up/down volume legend follows the volume
/// columns rather than the price bars. The column's fill alpha is a drawing strength, not part of
/// the hue, and would leave the text faint. `None` for any other series kind.
fn histogram_value_color(
    chart: &ChartEngine,
    snapshots: &[SeriesValueSnapshot],
    series_id: SeriesId,
) -> Option<String> {
    snapshot_for(snapshots, series_id)
        .and_then(|snapshot| snapshot.logical_index)
        .and_then(|index| chart.histogram_column_color_at(series_id, index))
        .map(|color| color.to_hex())
}

fn snapshot_for(
    snapshots: &[SeriesValueSnapshot],
    series_id: SeriesId,
) -> Option<&SeriesValueSnapshot> {
    snapshots
        .iter()
        .find(|snapshot| snapshot.series_id == series_id)
}

fn snapshot_value(snapshots: &[SeriesValueSnapshot], series_id: SeriesId) -> Option<String> {
    let snapshot = snapshot_for(snapshots, series_id)?;
    snapshot
        .formatted_value
        .as_ref()
        .or(snapshot.formatted_close.as_ref())
        .filter(|value| !value.is_empty())
        .cloned()
}

fn ohlc_values(snapshot: &SeriesValueSnapshot) -> Vec<FinancialLegendValue> {
    [
        ("O", &snapshot.formatted_open),
        ("H", &snapshot.formatted_high),
        ("L", &snapshot.formatted_low),
        ("C", &snapshot.formatted_close),
    ]
    .into_iter()
    .map(|(label, value)| FinancialLegendValue {
        text: format!("{label} {}", value.as_deref().unwrap_or("--")),
        color: None,
    })
    .collect()
}

fn snapshot_tone(snapshot: &SeriesValueSnapshot) -> FinancialLegendTone {
    match (snapshot.open, snapshot.close) {
        (Some(open), Some(close)) if close >= open => FinancialLegendTone::Bullish,
        (Some(_), Some(_)) => FinancialLegendTone::Bearish,
        _ => FinancialLegendTone::Neutral,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SeriesKind;

    #[test]
    fn financial_legend_groups_primary_host_series_and_native_outputs() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart
            .set_series_data(
                0,
                &[1.0, 2.0, 3.0],
                &[10.0, 11.0, 12.0],
                &[12.0, 13.0, 14.0],
                &[9.0, 10.0, 11.0],
                &[11.0, 12.0, 13.0],
            )
            .unwrap();
        let volume = chart.add_series(SeriesKind::Histogram);
        chart
            .set_series_data(
                volume,
                &[1.0, 2.0, 3.0],
                &[100.0, 120.0, 140.0],
                &[100.0, 120.0, 140.0],
                &[100.0, 120.0, 140.0],
                &[100.0, 120.0, 140.0],
            )
            .unwrap();
        let sma = chart.add_sma(0, 2).unwrap();
        let request = FinancialLegendRequest {
            logical_index: None,
            primary_title: "BTCUSD",
            show_primary_ohlc: true,
            primary_values_series: None,
            leading_series: &[HostLegendSeries {
                identity: 7,
                series_id: volume,
                title: "Volume",
                settings_available: false,
                tone_from_primary: true,
            }],
            trailing_series: &[],
        };

        let rows = chart.financial_legend(request);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].identity, FinancialLegendIdentity::Primary);
        assert_eq!(rows[0].title, "BTCUSD");
        assert_eq!(rows[0].values.len(), 4);
        assert_eq!(rows[0].tone, FinancialLegendTone::Bullish);
        assert_eq!(rows[1].identity, FinancialLegendIdentity::Host(7));
        assert_eq!(rows[1].first_series_id, volume);
        assert_eq!(rows[2].identity, FinancialLegendIdentity::Indicator(sma));
    }

    #[test]
    fn a_volume_value_reads_in_its_column_hue_not_the_price_bar_colors() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart
            .set_series_data(
                0,
                &[1.0, 2.0],
                &[10.0, 12.0],
                &[12.0, 13.0],
                &[9.0, 10.0],
                &[11.0, 11.0],
            )
            .unwrap();
        chart
            .series_entry_mut(0)
            .unwrap()
            .down_color
            .replace("#ffffff".to_string());
        let volume = chart.add_series(SeriesKind::Histogram);
        chart
            .set_series_data(
                volume,
                &[1.0, 2.0],
                &[100.0, 120.0],
                &[100.0, 120.0],
                &[100.0, 120.0],
                &[100.0, 120.0],
            )
            .unwrap();
        chart.series_entry_mut(volume).unwrap().histogram_updown = true;
        let volume_row = |chart: &ChartEngine, logical_index| {
            chart
                .financial_legend(FinancialLegendRequest {
                    logical_index,
                    primary_title: "BTC",
                    show_primary_ohlc: true,
                    primary_values_series: None,
                    leading_series: &[HostLegendSeries {
                        identity: 7,
                        series_id: volume,
                        title: "Volume",
                        settings_available: false,
                        tone_from_primary: false,
                    }],
                    trailing_series: &[],
                })
                .remove(1)
        };
        let hue = |rgb: (u8, u8, u8)| Some(crate::Color::rgb(rgb.0, rgb.1, rgb.2).to_hex());

        let falling = volume_row(&chart, None);
        assert_eq!(falling.tone, FinancialLegendTone::Neutral);
        assert_eq!(
            falling.values[0].color,
            hue(aeris_charts_core::style::MARKET_DOWN_RGB),
            "a falling bar's volume reads in the volume down hue, not the custom candle color"
        );
        let rising = volume_row(&chart, Some(0));
        assert_eq!(
            rising.values[0].color,
            hue(aeris_charts_core::style::MARKET_UP_RGB)
        );

        chart.series_entry_mut(volume).unwrap().histogram_updown = false;
        chart
            .series_entry_mut(volume)
            .unwrap()
            .line_color
            .replace("#123456".to_string());
        assert_eq!(
            volume_row(&chart, None).values[0].color.as_deref(),
            Some("#123456"),
            "a single-color histogram reads in its own color"
        );
    }

    #[test]
    fn a_volume_indicator_reads_its_columns_and_average_in_their_own_colors() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart
            .set_series_data(
                0,
                &[1.0, 2.0, 3.0],
                &[10.0, 12.0, 11.0],
                &[12.0, 13.0, 12.0],
                &[9.0, 10.0, 9.0],
                &[11.0, 11.0, 12.0],
            )
            .unwrap();
        let weights = chart.add_series(SeriesKind::Histogram);
        chart
            .set_series_data(
                weights,
                &[1.0, 2.0, 3.0],
                &[100.0, 200.0, 300.0],
                &[100.0, 200.0, 300.0],
                &[100.0, 200.0, 300.0],
                &[100.0, 200.0, 300.0],
            )
            .unwrap();
        let outputs = chart.add_indicator_kind(
            0,
            crate::IndicatorKind::Volume {
                period: 2,
                exponential: true,
            },
            Some(weights),
        );
        let [columns, average] = outputs[..] else {
            panic!("volume has a column and an average output");
        };
        let mut style = chart.indicator_info(columns).unwrap().style;
        style.up_color = Some("#00ff00".into());
        style.down_color = Some("#ff00ff".into());
        assert!(chart.set_indicator_output_style(columns, style));
        let mut average_style = chart.indicator_info(average).unwrap().style;
        average_style.line_color = Some("#123456".into());
        assert!(chart.set_indicator_output_style(average, average_style));
        let volume_row = |chart: &ChartEngine, logical_index| {
            chart
                .financial_legend(FinancialLegendRequest {
                    logical_index,
                    primary_title: "BTC",
                    show_primary_ohlc: true,
                    primary_values_series: None,
                    leading_series: &[],
                    trailing_series: &[],
                })
                .into_iter()
                .find(|row| row.identity == FinancialLegendIdentity::Indicator(columns))
                .expect("volume legend row")
        };

        let falling = volume_row(&chart, Some(1));
        assert_eq!(falling.title, "Volume");
        assert_eq!(falling.values.len(), 2, "columns and average, unlabeled");
        assert_eq!(falling.values[0].color.as_deref(), Some("#ff00ff"));
        assert_eq!(falling.values[1].color.as_deref(), Some("#123456"));
        assert!(
            !falling.values[1].text.starts_with("MA"),
            "volume values read by color alone"
        );
        let rising = volume_row(&chart, Some(2));
        assert_eq!(rising.values[0].color.as_deref(), Some("#00ff00"));
    }

    #[test]
    fn host_legend_descriptors_are_bounded() {
        let chart = ChartEngine::new(800.0, 500.0, 1.0);
        let descriptors = (0..32)
            .map(|identity| HostLegendSeries {
                identity,
                series_id: 0,
                title: "Host",
                settings_available: false,
                tone_from_primary: false,
            })
            .collect::<Vec<_>>();
        let rows = chart.financial_legend(FinancialLegendRequest {
            logical_index: None,
            primary_title: "",
            show_primary_ohlc: false,
            primary_values_series: None,
            leading_series: &descriptors,
            trailing_series: &[],
        });
        assert_eq!(rows.len(), 1 + MAX_HOST_LEGEND_SERIES);
    }

    #[test]
    fn a_whitespace_primary_reads_out_its_values_series() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let nan = [f64::NAN; 2];
        chart
            .set_series_data(0, &[60.0, 120.0], &nan, &nan, &nan, &nan)
            .unwrap();
        let footprint = chart
            .add_footprint_series(crate::FootprintSeriesOptions {
                aggregation: crate::FootprintAggregationOptions {
                    tick_size: 1.0,
                    ticks_per_row: 1,
                    ..crate::FootprintAggregationOptions::default()
                },
                visual: crate::FootprintVisualOptions::default(),
            })
            .unwrap();
        chart
            .set_footprint_trades(
                footprint,
                vec![crate::FootprintTrade {
                    timestamp_micros: 120_000_000,
                    price: 100.0,
                    volume: 2.0,
                    aggressor: crate::AggressorSide::Buy,
                    bid: None,
                    ask: None,
                    sequence: None,
                    trade_id: None,
                    conditions: 0,
                    session_id: None,
                }],
            )
            .unwrap();
        let primary_row = |values_series| {
            chart
                .financial_legend(FinancialLegendRequest {
                    logical_index: None,
                    primary_title: "BTC",
                    show_primary_ohlc: true,
                    primary_values_series: values_series,
                    leading_series: &[],
                    trailing_series: &[],
                })
                .remove(0)
        };
        let blank = primary_row(None);
        assert!(blank.values.iter().all(|value| value.text.ends_with("--")));
        let read_out = primary_row(Some(footprint));
        assert_eq!(read_out.identity, FinancialLegendIdentity::Primary);
        assert_eq!(read_out.values[3].text, "C 100");
        assert_eq!(read_out.tone, FinancialLegendTone::Bullish);
    }
}
