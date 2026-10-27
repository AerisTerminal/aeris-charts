//! Pure, confirmation-time swing-point study over borrowed OHLC columns.

use crate::{
    IndicatorInput,
    study_annotations::{StudyAnnotations, StudyMarker, StudyMarkerKind},
};

const CHECKPOINT_ROWS: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StructureStudyKind {
    SwingPoints { left: usize, right: usize },
}

impl StructureStudyKind {
    pub fn swing_points(left: usize, right: usize) -> Self {
        Self::SwingPoints { left, right }
    }

    /// Validate before installing a study. Invalid parameters never silently change the rule.
    pub fn is_valid(self) -> bool {
        let Self::SwingPoints { left, right } = self;
        (1..=50).contains(&left) && (1..=50).contains(&right)
    }

    pub fn output_count(self) -> usize {
        2
    }
}

#[derive(Clone, Copy, Debug)]
struct Pivot {
    price: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct ScanState {
    high: Option<Pivot>,
    low: Option<Pivot>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructureStudyResult {
    /// Swing highs and lows, stepped from confirmation and aligned to source rows.
    pub outputs: Vec<Vec<Option<f64>>>,
    pub annotations: StudyAnnotations,
}

/// Stateful append and historical repair. Sources are borrowed only for a call, never retained.
#[derive(Clone, Debug)]
pub struct StructureStudy {
    kind: StructureStudyKind,
    result: StructureStudyResult,
    state: ScanState,
    /// Scanner state only, never annotation history.
    checkpoints: Vec<ScanState>,
    len: usize,
    #[cfg(test)]
    processed_rows: usize,
}

impl StructureStudy {
    pub fn new(kind: StructureStudyKind) -> Self {
        assert!(kind.is_valid(), "invalid structure study parameters");
        Self {
            kind,
            result: StructureStudyResult {
                outputs: vec![Vec::new(); kind.output_count()],
                annotations: StudyAnnotations::default(),
            },
            state: ScanState::default(),
            checkpoints: Vec::new(),
            len: 0,
            #[cfg(test)]
            processed_rows: 0,
        }
    }

    pub fn annotations(&self) -> &StudyAnnotations {
        &self.result.annotations
    }

    pub fn outputs(&self) -> &[Vec<Option<f64>>] {
        &self.result.outputs
    }

    pub fn result(&self) -> &StructureStudyResult {
        &self.result
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn capacity_bytes(&self) -> usize {
        self.result.annotations.capacity_bytes()
            + self.checkpoints.capacity() * std::mem::size_of::<ScanState>()
            + self
                .result
                .outputs
                .iter()
                .map(|v| v.capacity() * std::mem::size_of::<Option<f64>>())
                .sum::<usize>()
    }

    /// `from` is the earliest changed row, or the old length for an append.
    pub fn update(&mut self, input: IndicatorInput<'_>, from: usize) {
        let n = input
            .open
            .len()
            .min(input.high.len())
            .min(input.low.len())
            .min(input.close.len());
        assert!(
            from <= self.len && from <= n,
            "invalid structure repair row"
        );
        if from < self.len {
            let restart = from.min(n.saturating_sub(1)) / CHECKPOINT_ROWS * CHECKPOINT_ROWS;
            self.state = if restart == 0 {
                ScanState::default()
            } else {
                self.checkpoints[restart / CHECKPOINT_ROWS - 1]
            };
            self.checkpoints.truncate(restart / CHECKPOINT_ROWS);
            self.result.annotations.rebuild_from(restart);
            for column in &mut self.result.outputs {
                column.truncate(restart);
            }
            self.process_rows(input, restart, n);
        } else {
            self.process_rows(input, from, n);
        }
        self.len = n;
    }

    fn process_rows(&mut self, input: IndicatorInput<'_>, start: usize, end: usize) {
        for row in start..end {
            self.advance(input, row);
        }
    }

    fn advance(&mut self, input: IndicatorInput<'_>, row: usize) {
        #[cfg(test)]
        {
            self.processed_rows += 1;
        }
        for column in &mut self.result.outputs {
            column.push(None);
        }
        let StructureStudyKind::SwingPoints { left, right } = self.kind;
        self.confirm_pivots(input, row, left, right);
        if valid(input, row) {
            self.result.outputs[0][row] = self.state.high.map(|p| p.price);
            self.result.outputs[1][row] = self.state.low.map(|p| p.price);
        }
        if (row + 1).is_multiple_of(CHECKPOINT_ROWS) {
            self.checkpoints.push(self.state);
        }
    }

    fn confirm_pivots(&mut self, input: IndicatorInput<'_>, row: usize, left: usize, right: usize) {
        if row < left + right {
            return;
        }
        let center = row - right;
        if !(center - left..=row).all(|i| valid(input, i)) {
            return;
        }
        let high = (center - left..center).all(|i| input.high[center] > input.high[i])
            && (center + 1..=row).all(|i| input.high[center] >= input.high[i]);
        let low = (center - left..center).all(|i| input.low[center] < input.low[i])
            && (center + 1..=row).all(|i| input.low[center] <= input.low[i]);
        if high {
            self.state.high = Some(Pivot {
                price: input.high[center],
            });
            self.result.annotations.push_marker(StudyMarker {
                row: center,
                confirm_row: row,
                price: input.high[center],
                kind: StudyMarkerKind::SwingHigh,
            });
        }
        if low {
            self.state.low = Some(Pivot {
                price: input.low[center],
            });
            self.result.annotations.push_marker(StudyMarker {
                row: center,
                confirm_row: row,
                price: input.low[center],
                kind: StudyMarkerKind::SwingLow,
            });
        }
    }
}

fn valid(input: IndicatorInput<'_>, row: usize) -> bool {
    let (o, h, l, c) = (
        input.open[row],
        input.high[row],
        input.low[row],
        input.close[row],
    );
    o.is_finite()
        && h.is_finite()
        && l.is_finite()
        && c.is_finite()
        && h >= o.max(c)
        && l <= o.min(c)
}

pub fn structure_study(
    input: IndicatorInput<'_>,
    kind: StructureStudyKind,
) -> StructureStudyResult {
    let mut study = StructureStudy::new(kind);
    study.update(input, 0);
    study.result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(o: &'a [f64], h: &'a [f64], l: &'a [f64], c: &'a [f64]) -> IndicatorInput<'a> {
        IndicatorInput {
            times: &[],
            open: o,
            high: h,
            low: l,
            close: c,
            volume: &[],
        }
    }

    #[test]
    fn leftmost_plateau_and_unequal_left_right_confirmation() {
        let c = [4., 5., 8., 7., 7., 6., 6.];
        let h = [5., 6., 9., 9., 9., 7., 7.];
        let l = [3., 4., 5., 5., 5., 4., 4.];
        let out = structure_study(
            input(&c, &h, &l, &c),
            StructureStudyKind::swing_points(2, 2),
        );
        let highs: Vec<_> = out
            .annotations
            .markers()
            .iter()
            .filter(|m| m.kind == StudyMarkerKind::SwingHigh)
            .map(|m| (m.row, m.confirm_row))
            .collect();
        assert_eq!(highs, [(2, 4)]);
        assert_eq!(out.outputs[0][3], None);
        assert_eq!(out.outputs[0][4], Some(9.));
        assert_eq!(out.outputs[0][6], Some(9.));
        assert!(!StructureStudyKind::swing_points(0, 5).is_valid());
        assert!(!StructureStudyKind::swing_points(5, 51).is_valid());
        assert_eq!(StructureStudyKind::swing_points(5, 5).output_count(), 2);
    }

    #[test]
    fn repeated_tip_replacement_preserves_all_markers() {
        // The tip is also the last row of a checkpoint block.
        let n = 8_192;
        let o = vec![100.; n];
        let mut h: Vec<_> = (0..n)
            .map(|i| if i.is_multiple_of(2) { 103. } else { 101. })
            .collect();
        let l: Vec<_> = (0..n)
            .map(|i| if i.is_multiple_of(2) { 99. } else { 97. })
            .collect();
        let mut study = StructureStudy::new(StructureStudyKind::swing_points(1, 1));
        study.update(input(&o, &h, &l, &o), 0);
        assert!(study.annotations().markers().len() > 4_096);
        for i in 0_usize..12 {
            h[n - 1] = if i.is_multiple_of(2) { 104. } else { 101. };
            let before = study.processed_rows;
            study.update(input(&o, &h, &l, &o), n - 1);
            assert!(study.processed_rows - before <= CHECKPOINT_ROWS);
            assert_eq!(
                *study.result(),
                structure_study(
                    input(&o, &h, &l, &o),
                    StructureStudyKind::swing_points(1, 1)
                )
            );
        }
    }

    #[test]
    fn long_history_repairs_from_one_checkpoint_not_from_zero() {
        let n = 20_000;
        let mut o: Vec<_> = (0..n).map(|i| (i / 4 * 4) as f64 + 10.).collect();
        let mut h: Vec<_> = o.iter().map(|v| v + 2.).collect();
        let l: Vec<_> = o.iter().map(|v| v - 2.).collect();
        let mut c = o.clone();
        for i in 0..n {
            if i % 4 == 1 {
                h[i] += 3.;
                c[i] += 1.;
            }
            if i % 4 == 2 {
                o[i] += 1.;
                c[i] -= 1.;
            }
        }
        let kind = StructureStudyKind::swing_points(1, 1);
        let mut study = StructureStudy::new(kind);
        study.update(input(&o, &h, &l, &c), 0);
        let rows: Vec<_> = study
            .annotations()
            .markers()
            .iter()
            .map(|m| m.confirm_row)
            .collect();
        assert!(
            rows.windows(3).all(|rows| rows[0] < rows[2]),
            "more than two annotations at one confirmation"
        );
        for from in [17_123, 1_023, 1_024, 0] {
            c[from] += 0.25;
            let before = study.processed_rows;
            study.update(input(&o, &h, &l, &c), from);
            assert!(
                study.processed_rows - before <= n - from + CHECKPOINT_ROWS,
                "replayed {} rows",
                study.processed_rows - before
            );
            assert_eq!(
                *study.result(),
                structure_study(input(&o, &h, &l, &c), kind)
            );
        }
    }

    #[test]
    fn prefix_append_tip_repair_and_checkpoint_match_batch() {
        let mut o = vec![5.; 620];
        let mut h = vec![6.; 620];
        let mut l = vec![4.; 620];
        let mut c = vec![5.; 620];
        for i in 0..620 {
            o[i] = 10. + (i % 9) as f64;
            c[i] = o[i] + if i % 4 == 0 { -0.5 } else { 0.5 };
            h[i] = o[i].max(c[i]) + 1.;
            l[i] = o[i].min(c[i]) - 1.;
        }
        let kind = StructureStudyKind::swing_points(3, 2);
        let mut study = StructureStudy::new(kind);
        for n in 1..=620 {
            study.update(input(&o[..n], &h[..n], &l[..n], &c[..n]), n - 1);
        }
        assert_eq!(
            *study.result(),
            structure_study(input(&o, &h, &l, &c), kind)
        );
        for index in [0, 255, 256, 440, 619] {
            c[index] = o[index];
            study.update(input(&o, &h, &l, &c), index);
            assert_eq!(
                *study.result(),
                structure_study(input(&o, &h, &l, &c), kind)
            );
            c[index] = o[index] + if index % 4 == 0 { -0.5 } else { 0.5 };
            study.update(input(&o, &h, &l, &c), index);
        }
    }
}
