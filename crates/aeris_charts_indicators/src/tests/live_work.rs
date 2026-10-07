//! A live append or tip replacement must read a number of source rows set by the indicator's
//! parameters, never by the history length.
use super::*;

/// These kinds still recompute their whole history on every update.
const WHOLE_HISTORY_KINDS: [TestKind; 2] = [TestKind::PivotPoints, TestKind::ZigZag];

/// Largest trailing lookback among the test parameters (Ichimoku's 52-row leading span).
const MAX_LOOKBACK_ROWS: usize = 52;

struct Columns {
    times: Vec<i64>,
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    volume: Vec<f64>,
}

impl Columns {
    fn new(rows: usize) -> Self {
        let close = (0..rows)
            .map(|row| 100.0 + (row as f64 * 0.37).sin() * 5.0 + (row % 11) as f64 * 0.125)
            .collect::<Vec<_>>();
        Self {
            times: (0..rows as i64).map(|row| row * 3_600).collect(),
            high: close
                .iter()
                .enumerate()
                .map(|(row, close)| close + 0.25 * (1 + row % 4) as f64)
                .collect(),
            low: close
                .iter()
                .enumerate()
                .map(|(row, close)| close - 0.25 * (1 + row % 3) as f64)
                .collect(),
            volume: (0..rows).map(|row| (1 + row % 17) as f64 * 10.0).collect(),
            close,
        }
    }

    fn input(&self, rows: usize) -> IndicatorInput<'_> {
        IndicatorInput {
            times: &self.times[..rows],
            open: &self.close[..rows],
            high: &self.high[..rows],
            low: &self.low[..rows],
            close: &self.close[..rows],
            volume: &self.volume[..rows],
        }
    }
}

/// Source rows read by an append of the last row and by a replacement of that tip.
fn live_work(kind: TestKind, mut state: IncrementalState, rows: usize) -> (usize, usize) {
    let mut columns = Columns::new(rows);
    state.rebuild_from(columns.input(rows - 1), 0);
    state.rebuild_from(columns.input(rows), rows - 1);
    let append = state.last_work_rows();
    let tip = rows - 1;
    columns.close[tip] += 1.5;
    columns.high[tip] += 2.0;
    columns.low[tip] -= 0.5;
    columns.volume[tip] += 3.0;
    state.rebuild_from(columns.input(rows), tip);
    let replace = state.last_work_rows();
    assert!(
        state.output_from(0) <= tip,
        "{kind:?} produced no tip output at {rows} rows"
    );
    (append, replace)
}

#[test]
fn live_append_and_tip_replace_work_does_not_grow_with_history() {
    for (kind, state) in all_test_states() {
        if WHOLE_HISTORY_KINDS.contains(&kind) {
            continue;
        }
        let short = live_work(kind, state.clone(), 2_048);
        let long = live_work(kind, state, 65_536);
        assert_eq!(
            short, long,
            "{kind:?} live (append, replace) source rows grow with history"
        );
        assert!(
            long.0 <= MAX_LOOKBACK_ROWS && long.1 <= MAX_LOOKBACK_ROWS,
            "{kind:?} live (append, replace) read {long:?} source rows"
        );
    }
}
