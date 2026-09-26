//! Engine-owned Heikin Ashi projection for candlestick presentation.
//!
//! The source series remains the canonical raw OHLC owner. This cache is only the derived
//! presentation projection used by frame/scaling paths; trading and crosshair queries continue to
//! read the source columns.

#[derive(Default)]
pub(crate) struct HeikinAshiCache {
    generation: Option<u64>,
    rows: Vec<[f64; 4]>,
}

impl HeikinAshiCache {
    pub(crate) fn row(
        &mut self,
        generation: u64,
        columns: [&[f64]; 4],
        row: usize,
    ) -> Option<[f64; 4]> {
        if self.generation != Some(generation) {
            self.rebuild(generation, columns);
        }
        self.rows.get(row).copied()
    }

    fn rebuild(&mut self, generation: u64, columns: [&[f64]; 4]) {
        let rows = columns.iter().map(|column| column.len()).min().unwrap_or(0);
        self.rows.clear();
        self.rows.reserve(rows);
        let mut previous_open = None;
        let mut previous_close = None;
        #[allow(clippy::needless_range_loop)]
        for row in 0..rows {
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
        self.generation = Some(generation);
    }

    #[cfg(test)]
    pub(crate) fn rows(&self) -> &[[f64; 4]] {
        &self.rows
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
}
