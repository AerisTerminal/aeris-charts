//! Crosshair magnet. Port of `src/model/magnet.ts` (RENDERING_SPEC.md §8).
//!
//! In Magnet mode the crosshair's horizontal line snaps to the close price of the bar under
//! the cursor; in MagnetOHLC it snaps to whichever of open/high/low/close is nearest the
//! cursor in *pixel* space. The comparison is done on coordinates (not prices) so it behaves
//! correctly on log scales.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrosshairMode {
    /// Crosshair moves freely with the cursor.
    Normal,
    /// Horizontal line sticks to the close price of the hovered bar.
    Magnet,
    /// Hidden (rendering suppressed).
    Hidden,
    /// Horizontal line sticks to the nearest of O/H/L/C of the hovered bar.
    MagnetOhlc,
}

/// Returns the candidate whose coordinate is nearest `cursor_coord`. Candidates are
/// `(price, coordinate)` pairs (already converted for the current price-scale mode).
/// Returns `None` when there are no candidates (caller keeps the raw cursor price).
pub fn magnet_snap(cursor_coord: f64, candidates: &[(f64, f64)]) -> Option<f64> {
    candidates
        .iter()
        .min_by(|a, b| {
            (a.1 - cursor_coord)
                .abs()
                .partial_cmp(&(b.1 - cursor_coord).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|&(price, _)| price)
}

/// Port of the candidate pick in `Magnet.align` (model/magnet.ts:80-83): candidates are y
/// coordinates, each converted on its own series' price scale; the one nearest the cursor
/// coordinate wins and is returned as a *coordinate* (the caller converts it back to a price
/// on the pane's default scale). Ties resolve to the earliest candidate, matching the reference's stable
/// sort. Returns `None` when there are no candidates (caller keeps the raw cursor price).
pub fn magnet_snap_coordinate(cursor_coord: f64, candidates: &[f64]) -> Option<f64> {
    let mut best: Option<f64> = None;
    for &candidate in candidates {
        let nearer = match best {
            Some(b) => (candidate - cursor_coord).abs() < (b - cursor_coord).abs(),
            None => true,
        };
        if nearer {
            best = Some(candidate);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_to_nearest_coordinate() {
        // cursor at y=100; candidates: close@105, high@40, low@160, open@98
        let candidates = [(10.0, 105.0), (20.0, 40.0), (5.0, 160.0), (12.0, 98.0)];
        // nearest coord to 100 is 98 (open, price 12)
        assert_eq!(magnet_snap(100.0, &candidates), Some(12.0));
    }

    #[test]
    fn single_candidate_close() {
        // Magnet mode passes only the close: always snaps to it
        assert_eq!(magnet_snap(50.0, &[(123.0, 200.0)]), Some(123.0));
    }

    #[test]
    fn empty_candidates() {
        assert_eq!(magnet_snap(50.0, &[]), None);
    }

    #[test]
    fn coordinate_pick_returns_the_winning_coordinate() {
        // cursor at y=100 across two series' scales: 98 beats 105 and 160
        assert_eq!(
            magnet_snap_coordinate(100.0, &[105.0, 40.0, 160.0, 98.0]),
            Some(98.0)
        );
        assert_eq!(magnet_snap_coordinate(50.0, &[200.0]), Some(200.0));
        assert_eq!(magnet_snap_coordinate(50.0, &[]), None);
    }

    #[test]
    fn coordinate_pick_tie_goes_to_the_earliest_candidate() {
        // equidistant candidates: the reference's stable sort keeps the first inserted
        assert_eq!(magnet_snap_coordinate(100.0, &[90.0, 110.0]), Some(90.0));
    }
}
