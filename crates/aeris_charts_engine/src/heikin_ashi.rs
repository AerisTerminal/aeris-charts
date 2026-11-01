//! Engine-owned Heikin Ashi projection for candlestick presentation.
//!
//! The source series remains the canonical raw OHLC owner. This cache is only the derived
//! presentation projection used by frame/scaling paths; trading and crosshair queries continue to
//! read the source columns. Rows keep the source row keys, so the projection is time-preserving.
//!
//! Each Heikin Ashi row depends only on its raw row and the previous Heikin Ashi row. The engine
//! reports the first changed source row with every data mutation, so a live append or tip
//! replacement recomputes only the rows from that point; a mutation the cache did not observe is
//! detected by its generation and rebuilds the projection.

#[derive(Default)]
pub(crate) struct HeikinAshiCache {
    generation: Option<u64>,
    /// Rows before this index are current for `generation`.
    valid: usize,
    rows: Vec<[f64; 4]>,
    #[cfg(test)]
    computed_rows: usize,
}

impl HeikinAshiCache {
    pub(crate) fn row(
        &mut self,
        generation: u64,
        columns: [&[f64]; 4],
        row: usize,
    ) -> Option<[f64; 4]> {
        if self.generation != Some(generation) {
            self.generation = Some(generation);
            self.valid = 0;
        }
        let rows = columns.iter().map(|column| column.len()).min().unwrap_or(0);
        if self.valid < rows || self.rows.len() != rows {
            self.extend(columns, rows);
        }
        self.rows.get(row).copied()
    }

    /// Record a source mutation from `previous_generation` to `generation` whose first changed
    /// row is `from`. A cache that was not current for `previous_generation` keeps its stale
    /// generation and rebuilds on the next read.
    pub(crate) fn note_change(&mut self, previous_generation: u64, generation: u64, from: usize) {
        if self.generation == Some(previous_generation) {
            self.generation = Some(generation);
            self.valid = self.valid.min(from);
        }
    }

    fn extend(&mut self, columns: [&[f64]; 4], rows: usize) {
        let start = self.valid.min(rows).min(self.rows.len());
        self.rows.truncate(start);
        self.rows.reserve(rows - start);
        let previous = self.rows.iter().rev().find(|row| !row[0].is_nan()).copied();
        let mut previous_open = previous.map(|row| row[0]);
        let mut previous_close = previous.map(|row| row[3]);
        #[allow(clippy::needless_range_loop)]
        for row in start..rows {
            #[cfg(test)]
            {
                self.computed_rows += 1;
            }
            let raw = [
                columns[0][row],
                columns[1][row],
                columns[2][row],
                columns[3][row],
            ];
            if raw.iter().all(|value| value.is_nan()) || raw.iter().any(|value| !value.is_finite())
            {
                self.rows.push([f64::NAN; 4]);
                continue;
            }
            let close = (raw[0] + raw[1] + raw[2] + raw[3]) / 4.0;
            let open = match (previous_open, previous_close) {
                (Some(previous_open), Some(previous_close)) => {
                    (previous_open + previous_close) / 2.0
                }
                _ => (raw[0] + raw[3]) / 2.0,
            };
            let high = raw[1].max(open).max(close);
            let low = raw[2].min(open).min(close);
            self.rows.push([open, high, low, close]);
            previous_open = Some(open);
            previous_close = Some(close);
        }
        self.valid = rows;
    }

    #[cfg(test)]
    pub(crate) fn rows(&self) -> &[[f64; 4]] {
        &self.rows
    }

    #[cfg(test)]
    pub(crate) fn computed_rows(&self) -> usize {
        self.computed_rows
    }
}

#[cfg(test)]
mod tests {
    use super::HeikinAshiCache;

    #[test]
    fn projection_keeps_raw_rows_out_of_the_derived_owner() {
        let columns = [
            &[10.0, 12.0][..],
            &[14.0, 16.0][..],
            &[8.0, 10.0][..],
            &[12.0, 14.0][..],
        ];
        let mut cache = HeikinAshiCache::default();
        assert_eq!(cache.row(1, columns, 0), Some([11.0, 14.0, 8.0, 11.0]));
        assert_eq!(cache.row(1, columns, 1), Some([11.0, 16.0, 10.0, 13.0]));
        assert_eq!(
            cache.rows(),
            &[[11.0, 14.0, 8.0, 11.0], [11.0, 16.0, 10.0, 13.0]]
        );
    }

    #[test]
    fn whitespace_does_not_reset_the_previous_heikin_ashi_open() {
        let columns = [
            &[10.0, f64::NAN, 14.0][..],
            &[14.0, f64::NAN, 18.0][..],
            &[8.0, f64::NAN, 12.0][..],
            &[12.0, f64::NAN, 16.0][..],
        ];
        let mut cache = HeikinAshiCache::default();
        cache.row(1, columns, 2);
        assert!(cache.rows()[1].iter().all(|value| value.is_nan()));
        assert_eq!(cache.rows()[2], [11.0, 18.0, 11.0, 15.0]);
    }

    #[test]
    fn observed_tip_changes_recompute_only_the_changed_rows() {
        let mut open = (0..1_000).map(|row| 100.0 + row as f64).collect::<Vec<_>>();
        let mut high = open.iter().map(|value| value + 2.0).collect::<Vec<_>>();
        let mut low = open.iter().map(|value| value - 2.0).collect::<Vec<_>>();
        let mut close = open.iter().map(|value| value + 1.0).collect::<Vec<_>>();
        let mut cache = HeikinAshiCache::default();
        cache.row(1, [&open, &high, &low, &close], 0);
        assert_eq!(cache.computed_rows(), 1_000);

        // Replace the tip.
        close[999] = 1_090.0;
        high[999] = 1_101.0;
        cache.note_change(1, 2, 999);
        cache.row(2, [&open, &high, &low, &close], 999);
        assert_eq!(cache.computed_rows(), 1_001);

        // Append a row.
        for (column, value) in [
            (&mut open, 1_090.0),
            (&mut high, 1_095.0),
            (&mut low, 1_080.0),
            (&mut close, 1_085.0),
        ] {
            column.push(value);
        }
        cache.note_change(2, 3, 1_000);
        cache.row(3, [&open, &high, &low, &close], 1_000);
        assert_eq!(cache.computed_rows(), 1_002);

        let mut rebuilt = HeikinAshiCache::default();
        rebuilt.row(9, [&open, &high, &low, &close], 0);
        assert_eq!(cache.rows(), rebuilt.rows());

        // A mutation the cache did not observe rebuilds from its generation.
        open.truncate(10);
        high.truncate(10);
        low.truncate(10);
        close.truncate(10);
        cache.note_change(7, 8, 9);
        cache.row(8, [&open, &high, &low, &close], 0);
        assert_eq!(cache.rows().len(), 10);
        assert_eq!(cache.rows(), &rebuilt.rows()[..10]);
    }
}
