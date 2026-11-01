use super::*;

fn source(values: &[f64]) -> Vec<SyntheticSourceBar> {
    values
        .iter()
        .enumerate()
        .map(|(index, &close)| SyntheticSourceBar {
            timestamp_micros: (index as i64 + 1) * 1_000_000,
            open: close,
            high: close,
            low: close,
            close,
        })
        .collect()
}

fn row(index: i64, open: f64, high: f64, low: f64, close: f64) -> SyntheticSourceBar {
    SyntheticSourceBar {
        timestamp_micros: (index + 1) * 1_000_000,
        open,
        high,
        low,
        close,
    }
}

fn built(options: SyntheticBarOptions, input: Vec<SyntheticSourceBar>) -> Vec<SyntheticBar> {
    let mut bars = SyntheticBarAggregator::new(options).unwrap();
    bars.set_source(input).unwrap();
    bars.bars().to_vec()
}

fn open_close(bars: &[SyntheticBar]) -> Vec<(f64, f64)> {
    bars.iter().map(|bar| (bar.open, bar.close)).collect()
}

fn ohlc(bars: &[SyntheticBar]) -> Vec<(f64, f64, f64, f64)> {
    bars.iter()
        .map(|bar| (bar.open, bar.high, bar.low, bar.close))
        .collect()
}

/// Deterministic random-walk OHLC rows with intrabar ranges.
fn walk(len: usize, seed: u64) -> Vec<SyntheticSourceBar> {
    let mut state = seed;
    let mut next = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 33) % 1_000) as f64 / 1_000.0
    };
    let mut close = 100.0;
    (0..len)
        .map(|index| {
            let open = close;
            close = (open + (next() - 0.5) * 6.0).max(1.0);
            let high = open.max(close) + next() * 2.0;
            let low = (open.min(close) - next() * 2.0).max(0.5);
            row(index as i64, open, high, low, close)
        })
        .collect()
}

fn every_transform() -> Vec<SyntheticBarOptions> {
    let mut options = Vec::new();
    for source in [SyntheticPriceSource::Close, SyntheticPriceSource::HighLow] {
        for wicks in [false, true] {
            options.push(SyntheticBarOptions::RenkoFixed {
                box_size: 1.0,
                source,
                wicks,
            });
            options.push(SyntheticBarOptions::RenkoAtr {
                period: 3,
                source,
                wicks,
            });
        }
        options.push(SyntheticBarOptions::PointAndFigure {
            box_size: 1.0,
            reversal_boxes: 3,
            source,
        });
        options.push(SyntheticBarOptions::PointAndFigure {
            box_size: 0.5,
            reversal_boxes: 1,
            source,
        });
    }
    for lines in [1, 2, 3, 5] {
        options.push(SyntheticBarOptions::LineBreak { lines });
    }
    for reversal_mode in [KagiReversalMode::Absolute, KagiReversalMode::Percent] {
        options.push(SyntheticBarOptions::Kagi {
            reversal_size: 2.0,
            reversal_mode,
        });
    }
    options
}

#[test]
fn fixed_renko_requires_two_boxes_to_reverse_and_emits_every_crossed_box() {
    let bars = built(
        SyntheticBarOptions::RenkoFixed {
            box_size: 1.0,
            source: SyntheticPriceSource::Close,
            wicks: false,
        },
        source(&[100.0, 103.2, 102.1, 100.8]),
    );
    assert_eq!(
        open_close(&bars),
        vec![
            (100.0, 101.0),
            (101.0, 102.0),
            (102.0, 103.0),
            (102.0, 101.0)
        ]
    );
    // Three bricks formed by one source row share that row's time bounds.
    assert!(
        bars[..3]
            .iter()
            .all(|bar| bar.close_timestamp_micros == 2_000_000)
    );
    assert_eq!(bars[1].open_timestamp_micros, 2_000_000);
    assert_eq!(bars[0].open_timestamp_micros, 1_000_000);
    assert_eq!(
        bars.iter().map(|bar| bar.logical_index).collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
}

#[test]
fn atr_renko_waits_for_wilder_warmup() {
    let mut input = source(&[100.0, 101.0, 102.0, 105.0]);
    for bar in &mut input {
        bar.high += 0.5;
        bar.low -= 0.5;
    }
    // True ranges 1, 1.5, 1.5 seed ATR 4/3; the fourth row's 3.5 smooths it to 37/18.
    let bars = built(
        SyntheticBarOptions::RenkoAtr {
            period: 3,
            source: SyntheticPriceSource::Close,
            wicks: false,
        },
        input,
    );
    assert_eq!(bars.len(), 1);
    assert_eq!(bars[0].open_timestamp_micros, 3_000_000);
    assert_eq!(bars[0].open, 102.0);
    assert!((bars[0].close - (102.0 + 37.0 / 18.0)).abs() < 1e-12);
}

#[test]
fn renko_wicks_extend_bricks_to_the_extremes_seen_since_the_prior_brick() {
    let input = vec![
        row(0, 100.0, 100.0, 100.0, 100.0),
        row(1, 100.0, 101.5, 99.4, 101.2),
        row(2, 101.2, 102.8, 100.9, 102.4),
        row(3, 102.4, 102.6, 99.7, 99.9),
    ];
    let options = |wicks| SyntheticBarOptions::RenkoFixed {
        box_size: 1.0,
        source: SyntheticPriceSource::Close,
        wicks,
    };
    assert_eq!(
        ohlc(&built(options(true), input.clone())),
        vec![
            (100.0, 101.0, 99.4, 101.0),
            (101.0, 102.0, 100.9, 102.0),
            (101.0, 102.6, 100.0, 100.0),
        ]
    );
    assert_eq!(
        ohlc(&built(options(false), input)),
        vec![
            (100.0, 101.0, 100.0, 101.0),
            (101.0, 102.0, 101.0, 102.0),
            (101.0, 101.0, 100.0, 100.0),
        ]
    );
}

#[test]
fn high_low_renko_tests_the_current_direction_first() {
    let input = vec![
        row(0, 100.0, 100.0, 100.0, 100.0),
        row(1, 100.0, 102.3, 99.5, 100.5),
        row(2, 100.5, 101.0, 99.8, 100.0),
    ];
    let options = |source| SyntheticBarOptions::RenkoFixed {
        box_size: 1.0,
        source,
        wicks: false,
    };
    assert_eq!(
        open_close(&built(
            options(SyntheticPriceSource::HighLow),
            input.clone()
        )),
        vec![(100.0, 101.0), (101.0, 102.0), (101.0, 100.0)]
    );
    assert!(built(options(SyntheticPriceSource::Close), input).is_empty());
}

#[test]
fn line_break_compares_close_against_last_n_line_extremes() {
    let bars = built(
        SyntheticBarOptions::LineBreak { lines: 3 },
        source(&[100.0, 101.0, 102.0, 101.5, 99.0]),
    );
    // The reversal line starts at the open of the line it reverses.
    assert_eq!(
        open_close(&bars),
        vec![
            (100.0, 100.0),
            (100.0, 101.0),
            (101.0, 102.0),
            (101.0, 99.0)
        ]
    );
    let closes = |lines| {
        built(
            SyntheticBarOptions::LineBreak { lines },
            source(&[100.0, 101.0, 102.0, 100.5]),
        )
        .iter()
        .map(|bar| bar.close)
        .collect::<Vec<_>>()
    };
    assert_eq!(closes(1), vec![100.0, 101.0, 102.0, 100.5]);
    assert_eq!(closes(2), vec![100.0, 101.0, 102.0]);
}

#[test]
fn kagi_extends_direction_and_reverses_only_by_configured_amount() {
    let bars = built(
        SyntheticBarOptions::Kagi {
            reversal_size: 2.0,
            reversal_mode: KagiReversalMode::Absolute,
        },
        source(&[100.0, 103.0, 104.0, 103.0, 101.5, 99.0]),
    );
    assert_eq!(open_close(&bars), vec![(100.0, 104.0), (104.0, 99.0)]);
}

#[test]
fn percent_kagi_scales_the_reversal_with_the_line_extreme() {
    let input = source(&[100.0, 103.0, 104.0, 101.95]);
    let options = |reversal_mode| SyntheticBarOptions::Kagi {
        reversal_size: 2.0,
        reversal_mode,
    };
    // 2% of the 104 extreme is 2.08, so 101.95 is not a reversal; an absolute 2.0 is.
    assert_eq!(
        open_close(&built(options(KagiReversalMode::Percent), input.clone())),
        vec![(100.0, 104.0)]
    );
    assert_eq!(
        open_close(&built(options(KagiReversalMode::Absolute), input)),
        vec![(100.0, 104.0), (104.0, 101.95)]
    );
    for size in [0.0, 100.0, f64::NAN] {
        assert_eq!(
            SyntheticBarAggregator::new(options(KagiReversalMode::Percent)).map(|_| ()),
            Ok(())
        );
        assert_eq!(
            SyntheticBarAggregator::new(SyntheticBarOptions::Kagi {
                reversal_size: size,
                reversal_mode: KagiReversalMode::Percent,
            })
            .map(|_| ()),
            Err(SyntheticBarError::InvalidOptions)
        );
    }
}

#[test]
fn kagi_switches_yang_and_yin_at_prior_shoulders_and_waists() {
    let bars = built(
        SyntheticBarOptions::Kagi {
            reversal_size: 2.0,
            reversal_mode: KagiReversalMode::Absolute,
        },
        source(&[100.0, 104.0, 101.0, 99.0, 102.0, 105.0]),
    );
    assert_eq!(
        open_close(&bars),
        vec![(100.0, 104.0), (104.0, 99.0), (99.0, 105.0)]
    );
    let lines = bars.iter().map(|bar| bar.kagi.unwrap()).collect::<Vec<_>>();
    assert_eq!(
        lines,
        vec![
            KagiLine {
                yang_at_open: true,
                reference: None,
            },
            // The falling line starts yang and turns yin through the 100 waist.
            KagiLine {
                yang_at_open: true,
                reference: Some(100.0),
            },
            // The rising line starts yin and turns yang through the 104 shoulder.
            KagiLine {
                yang_at_open: false,
                reference: Some(104.0),
            },
        ]
    );
    assert_eq!(
        bars.iter()
            .map(|bar| bar.kagi.unwrap().switch_price(bar.open, bar.close))
            .collect::<Vec<_>>(),
        vec![None, Some(100.0), Some(104.0)]
    );
    assert_eq!(
        bars.iter()
            .map(|bar| bar.kagi.unwrap().yang_at_close(bar.open, bar.close))
            .collect::<Vec<_>>(),
        vec![true, false, true]
    );
    // Before the extension through the shoulder, the rising line is still yin at its close.
    let partial = built(
        SyntheticBarOptions::Kagi {
            reversal_size: 2.0,
            reversal_mode: KagiReversalMode::Absolute,
        },
        source(&[100.0, 104.0, 101.0, 99.0, 102.0]),
    );
    let last = partial[2];
    assert!(!last.kagi.unwrap().yang_at_close(last.open, last.close));
    let renko = built(
        SyntheticBarOptions::RenkoFixed {
            box_size: 1.0,
            source: SyntheticPriceSource::Close,
            wicks: false,
        },
        source(&[100.0, 104.0]),
    );
    assert!(renko.iter().all(|bar| bar.kagi.is_none()));
}

#[test]
fn point_and_figure_builds_columns_and_honors_reversal_boxes() {
    let options = |reversal_boxes| SyntheticBarOptions::PointAndFigure {
        box_size: 1.0,
        reversal_boxes,
        source: SyntheticPriceSource::Close,
    };
    assert_eq!(
        open_close(&built(
            options(3),
            source(&[100.0, 104.2, 102.0, 100.5, 103.8])
        )),
        vec![(100.0, 104.0), (103.0, 101.0)]
    );
    assert_eq!(
        open_close(&built(options(2), source(&[100.0, 104.2, 102.0]))),
        vec![(100.0, 104.0), (103.0, 102.0)]
    );
    assert_eq!(
        open_close(&built(options(3), source(&[100.0, 104.2, 102.0]))),
        vec![(100.0, 104.0)]
    );
}

#[test]
fn high_low_point_and_figure_uses_intrabar_extremes() {
    let input = vec![
        row(0, 100.0, 100.0, 100.0, 100.0),
        row(1, 100.0, 103.4, 99.6, 102.0),
        row(2, 102.0, 102.5, 99.9, 100.2),
    ];
    let options = |source| SyntheticBarOptions::PointAndFigure {
        box_size: 1.0,
        reversal_boxes: 3,
        source,
    };
    assert_eq!(
        open_close(&built(
            options(SyntheticPriceSource::HighLow),
            input.clone()
        )),
        vec![(100.0, 103.0), (102.0, 100.0)]
    );
    assert_eq!(
        open_close(&built(options(SyntheticPriceSource::Close), input)),
        vec![(100.0, 102.0)]
    );
}

#[test]
fn options_validate_and_deserialize_with_backward_compatible_defaults() {
    let parse = |json: &str| serde_json::from_str::<SyntheticBarOptions>(json).unwrap();
    assert_eq!(
        parse(r#"{"kind":"line_break"}"#),
        SyntheticBarOptions::LineBreak {
            lines: DEFAULT_LINE_BREAK_LINES
        }
    );
    assert_eq!(
        parse(r#"{"kind":"renko_fixed","box_size":1}"#),
        SyntheticBarOptions::RenkoFixed {
            box_size: 1.0,
            source: SyntheticPriceSource::Close,
            wicks: false,
        }
    );
    assert_eq!(
        parse(r#"{"kind":"renko_atr","period":14,"source":"high_low","wicks":true}"#),
        SyntheticBarOptions::RenkoAtr {
            period: 14,
            source: SyntheticPriceSource::HighLow,
            wicks: true,
        }
    );
    assert_eq!(
        parse(r#"{"kind":"kagi","reversal_size":2}"#),
        SyntheticBarOptions::Kagi {
            reversal_size: 2.0,
            reversal_mode: KagiReversalMode::Absolute,
        }
    );
    assert_eq!(
        parse(r#"{"kind":"point_and_figure","box_size":0.5}"#),
        SyntheticBarOptions::PointAndFigure {
            box_size: 0.5,
            reversal_boxes: DEFAULT_POINT_AND_FIGURE_REVERSAL_BOXES,
            source: SyntheticPriceSource::Close,
        }
    );
    let round_trip = SyntheticBarOptions::Kagi {
        reversal_size: 1.5,
        reversal_mode: KagiReversalMode::Percent,
    };
    assert_eq!(
        parse(&serde_json::to_string(&round_trip).unwrap()),
        round_trip
    );
    for invalid in [
        SyntheticBarOptions::LineBreak { lines: 0 },
        SyntheticBarOptions::LineBreak {
            lines: MAX_LINE_BREAK_LINES + 1,
        },
        SyntheticBarOptions::RenkoAtr {
            period: 0,
            source: SyntheticPriceSource::Close,
            wicks: false,
        },
        SyntheticBarOptions::RenkoFixed {
            box_size: 0.0,
            source: SyntheticPriceSource::Close,
            wicks: false,
        },
        SyntheticBarOptions::PointAndFigure {
            box_size: 1.0,
            reversal_boxes: 0,
            source: SyntheticPriceSource::Close,
        },
    ] {
        assert_eq!(
            SyntheticBarAggregator::new(invalid).map(|_| ()),
            Err(SyntheticBarError::InvalidOptions),
            "{invalid:?}"
        );
    }
    assert!(
        SyntheticBarAggregator::new(SyntheticBarOptions::LineBreak {
            lines: MAX_LINE_BREAK_LINES
        })
        .is_ok()
    );
}

#[test]
fn source_replacement_is_atomic_and_ordered() {
    let mut bars =
        SyntheticBarAggregator::new(SyntheticBarOptions::LineBreak { lines: 3 }).unwrap();
    bars.set_source(source(&[100.0, 101.0])).unwrap();
    let before = bars.bars().to_vec();
    let mut invalid = source(&[102.0])[0];
    invalid.high = 100.0;
    assert!(bars.update_source(invalid).is_err());
    assert_eq!(bars.bars(), before);
    assert_eq!(
        bars.update_source(source(&[99.0])[0]),
        Err(SyntheticBarError::OutOfOrderSource { index: 2 })
    );
    assert_eq!(bars.bars(), before);
}

#[test]
fn ordered_tip_updates_match_full_rebuild_for_every_transform() {
    let input = walk(400, 7);
    for options in every_transform() {
        let mut streamed = SyntheticBarAggregator::new(options).unwrap();
        for bar in &input {
            streamed.update_source(*bar).unwrap();
        }
        let mut rebuilt = SyntheticBarAggregator::new(options).unwrap();
        rebuilt.set_source(input.clone()).unwrap();
        assert_eq!(streamed.bars(), rebuilt.bars(), "{options:?}");
        assert!(!rebuilt.bars().is_empty(), "{options:?}");
    }
}

#[test]
fn tip_replacement_matches_rebuild_and_touches_only_bars_the_tip_formed() {
    let input = walk(240, 11);
    let variants = walk(240, 29);
    for options in every_transform() {
        let mut streamed = SyntheticBarAggregator::new(options).unwrap();
        let mut accepted = Vec::new();
        for (index, bar) in input.iter().enumerate() {
            let before_tip_row = streamed.bars().len();
            streamed.update_source(*bar).unwrap();
            accepted.push(*bar);
            // Replace the live row with a different move, then with its final value.
            let mut moved = variants[index];
            moved.timestamp_micros = bar.timestamp_micros;
            for replacement in [moved, *bar] {
                let previous = streamed.bars().to_vec();
                let changed = streamed.update_source(replacement).unwrap();
                *accepted.last_mut().unwrap() = replacement;
                let mut rebuilt = SyntheticBarAggregator::new(options).unwrap();
                rebuilt.set_source(accepted.clone()).unwrap();
                assert_eq!(streamed.bars(), rebuilt.bars(), "{options:?} row {index}");
                match changed {
                    None => assert_eq!(streamed.bars(), previous, "{options:?}"),
                    Some(from) => {
                        assert!(from + 1 >= before_tip_row, "{options:?} row {index}");
                        assert_eq!(streamed.bars()[..from], previous[..from], "{options:?}");
                    }
                }
            }
        }
    }
}

fn chart(options: SyntheticBarOptions) -> crate::ChartEngine {
    let mut chart = crate::ChartEngine::new(600.0, 400.0, 1.0);
    chart.configure_synthetic_bar_series(0, options).unwrap();
    chart
}

fn series_columns(chart: &crate::ChartEngine, id: crate::SeriesId) -> (Vec<i64>, Vec<Vec<f64>>) {
    let (times, columns) = chart.data_layer().series_data(id).unwrap();
    (
        times.to_vec(),
        columns.iter().map(|column| column.to_vec()).collect(),
    )
}

#[test]
fn live_tail_replacement_matches_a_full_install() {
    let options = SyntheticBarOptions::RenkoFixed {
        box_size: 0.5,
        source: SyntheticPriceSource::HighLow,
        wicks: true,
    };
    let input = walk(160, 3);
    let variants = walk(160, 41);
    let mut streamed = chart(options);
    streamed
        .set_synthetic_bar_source(0, input[..1].to_vec())
        .unwrap();
    let streamed_sma =
        streamed.add_indicator_kind(0, crate::IndicatorKind::Sma { period: 4 }, None);
    assert_eq!(streamed_sma.len(), 1);
    let mut accepted = input[..1].to_vec();
    for (index, bar) in input.iter().enumerate().skip(1) {
        let mut moved = variants[index];
        moved.timestamp_micros = bar.timestamp_micros;
        for update in [*bar, moved, *bar] {
            streamed.update_synthetic_bar_source(0, update).unwrap();
        }
        accepted.push(*bar);
    }
    let mut installed = chart(options);
    installed.set_synthetic_bar_source(0, accepted).unwrap();
    let installed_sma =
        installed.add_indicator_kind(0, crate::IndicatorKind::Sma { period: 4 }, None);

    assert_eq!(streamed.synthetic_bars(0), installed.synthetic_bars(0));
    assert_eq!(streamed.sequence_points(), installed.sequence_points());
    assert_eq!(series_columns(&streamed, 0), series_columns(&installed, 0));
    // Indicators consume derived closes, updated from the first replaced row.
    assert_eq!(
        series_columns(&streamed, streamed_sma[0]),
        series_columns(&installed, installed_sma[0])
    );
    for spacing in [2.0, 6.0, 20.0] {
        assert_eq!(
            streamed.tick_marks.build(spacing, 60.0).to_vec(),
            installed.tick_marks.build(spacing, 60.0).to_vec()
        );
    }
    let bars = streamed.synthetic_bars(0).unwrap().to_vec();
    assert!(bars.len() > 40);
    for (index, bar) in bars.iter().enumerate() {
        assert_eq!(streamed.synthetic_bar_at(0, index), Some(*bar));
    }
    assert_eq!(streamed.synthetic_bar_at(0, bars.len()), None);
}

#[test]
fn derived_axis_reports_source_time_and_shared_times_resolve_to_the_first_brick() {
    let mut chart = chart(SyntheticBarOptions::RenkoFixed {
        box_size: 1.0,
        source: SyntheticPriceSource::Close,
        wicks: false,
    });
    chart
        .set_synthetic_bar_source(0, source(&[100.0, 103.0, 103.5, 99.0]))
        .unwrap();
    // Bricks 100-101, 101-102, 102-103 form at t=2s and 102-101, 101-100, 100-99 at t=4s;
    // each opens at the time its predecessor closed.
    assert_eq!(
        chart
            .sequence_points()
            .unwrap()
            .iter()
            .map(|point| point.open_timestamp_micros)
            .collect::<Vec<_>>(),
        vec![
            1_000_000, 2_000_000, 2_000_000, 2_000_000, 4_000_000, 4_000_000
        ]
    );
    chart.time_scale.set_width(600.0);
    chart.fit_content();
    for index in 0..6 {
        let x = chart.time_scale.index_to_coordinate(index);
        let expected = chart.sequence_points().unwrap()[index as usize].open_timestamp_micros;
        assert_eq!(chart.coordinate_to_time(x), Some(expected as f64 / 1e6));
    }
    assert_eq!(chart.time_to_index(2.0, false), Some(1));
    assert_eq!(chart.time_to_index(4.0, false), Some(4));
    assert_eq!(chart.time_to_index(3.0, false), None);
}

#[test]
fn chart_projection_owns_sequence_identity_replay_and_domain_exclusivity() {
    let mut chart = chart(SyntheticBarOptions::RenkoFixed {
        box_size: 1.0,
        source: SyntheticPriceSource::Close,
        wicks: false,
    });
    chart
        .set_synthetic_bar_source(0, source(&[100.0, 103.0, 99.0]))
        .unwrap();
    assert_eq!(chart.data_layer().series_data(0).unwrap().0.len(), 6);
    assert_eq!(chart.sequence_points().unwrap().len(), 6);

    chart.set_replay_clock_micros(Some(2_000_000)).unwrap();
    assert_eq!(chart.data_layer().series_data(0).unwrap().0.len(), 3);
    assert_eq!(chart.sequence_points().unwrap().len(), 3);
    assert_eq!(chart.synthetic_bar_at(0, 3), None);
    chart.set_replay_clock_micros(None).unwrap();
    assert_eq!(chart.data_layer().series_data(0).unwrap().0.len(), 6);

    let before = chart.synthetic_bars(0).unwrap().to_vec();
    chart
        .update_synthetic_bar_source(0, row(3, 99.0, 105.0, 99.0, 105.0))
        .unwrap();
    assert_eq!(&chart.synthetic_bars(0).unwrap()[..before.len()], before);

    // Reconfiguring keeps the canonical source and rebuilds atomically.
    chart
        .configure_synthetic_bar_series(0, SyntheticBarOptions::LineBreak { lines: 3 })
        .unwrap();
    // Lines 100-100, 100-103, reversal 100-99, reversal 100-105.
    assert_eq!(
        open_close(chart.synthetic_bars(0).unwrap()),
        vec![
            (100.0, 100.0),
            (100.0, 103.0),
            (100.0, 99.0),
            (100.0, 105.0)
        ]
    );
    assert_eq!(
        chart.configure_synthetic_bar_series(0, SyntheticBarOptions::LineBreak { lines: 0 }),
        Err(SyntheticBarError::InvalidOptions)
    );
    assert_eq!(
        chart.synthetic_bar_options(0),
        Some(SyntheticBarOptions::LineBreak { lines: 3 })
    );

    // The source owner is the only write path for synthetic projections: accepting generic
    // OHLC writes, pop, or an independent retention cap would desynchronize replay and the
    // logical sequence from the aggregator's canonical state.
    assert!(!chart.update_series_bar(0, 5.0, [1.0, 1.0, 1.0, 1.0]));
    assert!(
        chart
            .set_series_data(0, &[5.0], &[1.0], &[1.0], &[1.0], &[1.0])
            .is_err()
    );
    assert_eq!(chart.series_pop(0, 1), None);
    assert!(!chart.set_series_max_points(0, Some(2)));

    let second = chart.add_series(crate::SeriesKind::Candlestick);
    assert_eq!(
        chart.configure_synthetic_bar_series(second, SyntheticBarOptions::LineBreak { lines: 3 }),
        Err(SyntheticBarError::SequenceDomainInUse)
    );
    assert_eq!(
        chart.add_trade_stream(
            "second-sequence",
            crate::FootprintAggregationOptions {
                bars: crate::FootprintBarAggregation::Trades { trades_per_bar: 10 },
                ..crate::FootprintAggregationOptions::default()
            },
        ),
        Err(crate::FootprintError::SequenceDomainInUse)
    );
}

#[test]
fn kagi_and_point_and_figure_emit_distinct_shared_frame_geometry() {
    use aeris_charts_render::draw_list::Prim;

    let mut kagi = chart(SyntheticBarOptions::Kagi {
        reversal_size: 2.0,
        reversal_mode: KagiReversalMode::Absolute,
    });
    kagi.set_synthetic_bar_source(0, source(&[100.0, 104.0, 101.0, 99.0, 102.0, 105.0]))
        .unwrap();
    kagi.time_scale.set_width(600.0);
    kagi.fit_content();
    kagi.set_visible_logical_range(-1.0, 3.0);
    kagi.build_frame();
    let frame = kagi.build_frame();
    let verticals = frame.panes[0]
        .main
        .iter()
        .filter_map(|primitive| match primitive {
            Prim::VLine { width, color, .. } => Some((*width, *color)),
            _ => None,
        })
        .collect::<Vec<_>>();
    // Three lines, two of which switch thickness at the prior waist and shoulder.
    assert_eq!(verticals.len(), 5);
    let thin = verticals.iter().map(|(width, _)| *width).min().unwrap();
    let thick = verticals.iter().map(|(width, _)| *width).max().unwrap();
    assert_eq!(thick, thin * 2);
    assert_eq!(
        verticals
            .iter()
            .map(|(width, _)| *width == thick)
            .collect::<Vec<_>>(),
        vec![true, true, false, false, true]
    );
    let yang_color = verticals[0].1;
    assert!(
        verticals
            .iter()
            .all(|(width, color)| (*width == thick) == (*color == yang_color))
    );
    let horizontals = frame.panes[0]
        .main
        .iter()
        .filter(|primitive| matches!(primitive, Prim::HLine { .. }))
        .count();
    assert!(horizontals >= 2);

    let mut point_and_figure = chart(SyntheticBarOptions::PointAndFigure {
        box_size: 1.0,
        reversal_boxes: 3,
        source: SyntheticPriceSource::Close,
    });
    point_and_figure
        .set_synthetic_bar_source(0, source(&[100.0, 104.2, 100.5]))
        .unwrap();
    point_and_figure.time_scale.set_width(600.0);
    point_and_figure.fit_content();
    point_and_figure.set_visible_logical_range(-1.0, 2.0);
    point_and_figure.set_bar_spacing(20.0);
    point_and_figure.build_frame();
    let frame = point_and_figure.build_frame();
    let glyphs = frame.panes[0]
        .main
        .iter()
        .filter_map(|primitive| match primitive {
            Prim::Text { text, .. } if text == "X" || text == "O" => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    // X column 100..104 is five boxes; O column 103..101 is three.
    assert_eq!(glyphs.iter().filter(|glyph| **glyph == "X").count(), 5);
    assert_eq!(glyphs.iter().filter(|glyph| **glyph == "O").count(), 3);
}
