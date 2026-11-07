//! Deterministic word cloud (complete build). Words are placed by descending weight on an
//! Archimedean spiral from the plot center, each candidate box colliding against the boxes
//! already placed through a bounded spatial grid; words that find no spot within the step
//! budget are omitted and reported as such in the accessibility snapshot. The named algorithm
//! reference is d3-cloud 1.2 (BSD-3-Clause); no implementation code was copied.
//!
//! Placement depends on the host text measurement through [`ChartEngine::measure_text_run`];
//! when a font finishes loading the engine invalidates text measurements, which drops this
//! cache so the next frame re-places every word.

use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{Prim, TextAlign};

use super::{
    GeneralLayoutElement, GeneralLayoutSeriesOptions, LayoutSeriesEntry, WordCloudScaling,
    WordCloudWord, add_text_bytes, invalid_data, resource_limit, validate_value,
};
use crate::general_axes::GeneralPlotRect;
use crate::{ChartEngine, ChartError, ErrorCode};

/// Words a dataset can hold.
pub(super) const MAX_WORDS: usize = 500;
/// Spiral candidates tried per word.
const SPIRAL_STEPS: usize = 600;
/// Angular advance between candidates; an irrational multiple of a turn keeps the walk from
/// re-tracing its own arms.
const GOLDEN_ANGLE: f64 = 2.399_963_229_728_653;
/// Radius growth per turn, in media px: three full turns to cross the plot.
const SPIRAL_TIGHTNESS: f64 = 3.0;
/// Cells of the collision grid per axis.
const GRID_CELLS: usize = 16;
/// Padding around each measured box, in media px.
const BOX_PADDING: f64 = 2.0;

/// A placed or attempted word's axis-aligned box in plot coordinates.
#[derive(Clone, Copy, Debug)]
struct WordBox {
    cx: f64,
    cy: f64,
    /// Half extents of the box actually occupied: swapped when the word is rotated.
    hw: f64,
    hh: f64,
}

impl WordBox {
    fn overlaps(self, other: WordBox) -> bool {
        (self.cx - other.cx).abs() < self.hw + other.hw
            && (self.cy - other.cy).abs() < self.hh + other.hh
    }

    fn inside(self, plot: GeneralPlotRect) -> bool {
        self.cx - self.hw >= plot.x
            && self.cx + self.hw <= plot.x + plot.width
            && self.cy - self.hh >= plot.y
            && self.cy + self.hh <= plot.y + plot.height
    }
}

/// Placed words of one series for one plot rectangle.
pub(super) struct WordCloudCache {
    plot: GeneralPlotRect,
    /// One slot per input word; `None` when the word found no non-overlapping spot.
    placed: Vec<Option<PlacedWord>>,
    /// Font size per word, parallel to `placed`.
    sizes: Vec<f64>,
    /// Measured text width per word in media px, parallel to `placed`.
    widths: Vec<f64>,
    /// Clockwise rotation per word in degrees, parallel to `placed`.
    angles: Vec<f64>,
}

#[derive(Clone, Copy, Debug)]
struct PlacedWord {
    x: f64,
    y: f64,
}

/// Reserved cache bytes for any word dataset: placement slots are bounded by the word cap.
pub(super) fn cache_reservation() -> usize {
    MAX_WORDS * (std::mem::size_of::<Option<PlacedWord>>() + std::mem::size_of::<f64>() * 2)
}

/// Word option validation, run atomically with the dataset: every stored option combination
/// lays out without a fallback.
pub(super) fn validate_options(options: &GeneralLayoutSeriesOptions) -> Result<(), ChartError> {
    let invalid = |message: &'static str| ChartError::new(ErrorCode::InvalidOptions, message);
    let min = options.word_cloud_min_size;
    let max = options.word_cloud_max_size;
    if !min.is_finite() || !max.is_finite() {
        return Err(invalid("word cloud font sizes must be finite"));
    }
    if !(4.0..=256.0).contains(&min) || !(4.0..=256.0).contains(&max) || min > max {
        return Err(invalid(
            "word cloud font sizes stay between 4 and 256, min no larger than max",
        ));
    }
    Ok(())
}

pub(super) fn validate(words: &[WordCloudWord]) -> Result<(), ChartError> {
    if words.is_empty() {
        return Err(invalid_data("a word cloud needs at least one word"));
    }
    if words.len() > MAX_WORDS {
        return Err(resource_limit("too many words"));
    }
    let mut bytes = 0usize;
    for word in words {
        validate_value(word.weight)?;
        if word.text.trim().is_empty() {
            return Err(invalid_data("a word is empty"));
        }
        add_text_bytes(&mut bytes, &word.text)?;
        if let Some(color) = &word.color {
            add_text_bytes(&mut bytes, color)?;
            if Color::parse_css(color).is_none() {
                return Err(invalid_data("invalid word color"));
            }
        }
    }
    Ok(())
}

impl WordCloudCache {
    fn new(
        engine: &ChartEngine,
        series: &LayoutSeriesEntry,
        words: &[WordCloudWord],
        plot: GeneralPlotRect,
    ) -> Self {
        let options = &series.options;
        let family = engine.options.get().layout.font_family.clone();
        let mut placed = vec![None; words.len()];
        let mut sizes = vec![0.0; words.len()];
        let mut widths = vec![0.0; words.len()];
        let mut angles = vec![0.0; words.len()];
        if plot.width <= 0.0 || plot.height <= 0.0 {
            return Self {
                plot,
                placed,
                sizes,
                widths,
                angles,
            };
        }
        // Stable descending-weight order; the sort keeps input order inside one weight.
        let mut order: Vec<usize> = (0..words.len()).collect();
        order.sort_by(|&a, &b| {
            words[b]
                .weight
                .total_cmp(&words[a].weight)
                .then_with(|| a.cmp(&b))
        });
        let (wmin, wmax) = words
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), word| {
                (lo.min(word.weight), hi.max(word.weight))
            });
        let scale_weight = |weight: f64| match options.word_cloud_scaling {
            WordCloudScaling::Linear => weight,
            WordCloudScaling::Sqrt => weight.max(0.0).sqrt(),
        };
        let (smin, smax) = (scale_weight(wmin), scale_weight(wmax));
        let font_size = |weight: f64| {
            if smax - smin <= f64::EPSILON {
                options.word_cloud_max_size
            } else {
                options.word_cloud_min_size
                    + (scale_weight(weight) - smin) / (smax - smin)
                        * (options.word_cloud_max_size - options.word_cloud_min_size)
            }
        };
        let (cx, cy) = (plot.x + 0.5 * plot.width, plot.y + 0.5 * plot.height);
        let radius_step = plot.width.min(plot.height) / (SPIRAL_TIGHTNESS * 360.0);
        let cell_w = (plot.width / GRID_CELLS as f64).max(1e-9);
        let cell_h = (plot.height / GRID_CELLS as f64).max(1e-9);
        // One bitset per grid row; word boxes stamp the cells they cover.
        let mut grid = vec![0u128; GRID_CELLS];
        let stamp = |grid: &mut [u128], bx: &WordBox| {
            let col0 = (((bx.cx - bx.hw - plot.x) / cell_w) as i32).clamp(0, GRID_CELLS as i32 - 1);
            let col1 = (((bx.cx + bx.hw - plot.x) / cell_w) as i32).clamp(0, GRID_CELLS as i32 - 1);
            let row0 = (((bx.cy - bx.hh - plot.y) / cell_h) as i32).clamp(0, GRID_CELLS as i32 - 1);
            let row1 = (((bx.cy + bx.hh - plot.y) / cell_h) as i32).clamp(0, GRID_CELLS as i32 - 1);
            let mut bits = 0u128;
            for col in col0..=col1 {
                bits |= 1u128 << col;
            }
            for row in row0..=row1 {
                grid[row as usize] |= bits;
            }
        };
        let grid_clear = |grid: &[u128], bx: &WordBox| {
            let col0 = (((bx.cx - bx.hw - plot.x) / cell_w) as i32).clamp(0, GRID_CELLS as i32 - 1);
            let col1 = (((bx.cx + bx.hw - plot.x) / cell_w) as i32).clamp(0, GRID_CELLS as i32 - 1);
            let row0 = (((bx.cy - bx.hh - plot.y) / cell_h) as i32).clamp(0, GRID_CELLS as i32 - 1);
            let row1 = (((bx.cy + bx.hh - plot.y) / cell_h) as i32).clamp(0, GRID_CELLS as i32 - 1);
            let mut bits = 0u128;
            for col in col0..=col1 {
                bits |= 1u128 << col;
            }
            (row0..=row1).all(|row| grid[row as usize] & bits == 0)
        };
        let mut boxes: Vec<WordBox> = Vec::with_capacity(words.len().min(MAX_WORDS));
        for (rank, &index) in order.iter().enumerate() {
            let size = font_size(words[index].weight);
            let width = engine.measure_text_run(&words[index].text, size, &family, 400, false);
            if !width.is_finite() || width <= 0.0 {
                continue;
            }
            let rotate = options.word_cloud_rotate && rank % 2 == 1;
            let (tw, th) = if rotate { (size, width) } else { (width, size) };
            let (hw, hh) = (0.5 * tw + BOX_PADDING, 0.5 * th + BOX_PADDING);
            if 2.0 * hw > plot.width || 2.0 * hh > plot.height {
                continue;
            }
            let mut found = None;
            for step in 0..SPIRAL_STEPS {
                let angle = GOLDEN_ANGLE * step as f64;
                let radius = radius_step * step as f64;
                let candidate = WordBox {
                    cx: cx + radius * angle.cos(),
                    cy: cy + radius * angle.sin(),
                    hw,
                    hh,
                };
                if !candidate.inside(plot) {
                    continue;
                }
                // The grid fast-path: a candidate over no stamped cell cannot overlap a
                // placed box; a stamped cell still needs the exact box test.
                if !grid_clear(&grid, &candidate)
                    && boxes
                        .iter()
                        .any(|placed_box| candidate.overlaps(*placed_box))
                {
                    continue;
                }
                found = Some(candidate);
                break;
            }
            if let Some(bx) = found {
                stamp(&mut grid, &bx);
                boxes.push(bx);
                placed[index] = Some(PlacedWord { x: bx.cx, y: bx.cy });
                sizes[index] = size;
                widths[index] = width;
                angles[index] = if rotate { 90.0 } else { 0.0 };
            }
        }
        Self {
            plot,
            placed,
            sizes,
            widths,
            angles,
        }
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.placed.capacity() * std::mem::size_of::<Option<PlacedWord>>()
            + (self.sizes.capacity() + self.widths.capacity() + self.angles.capacity())
                * std::mem::size_of::<f64>()
    }

    /// Whether the word at `row` was placed, for the accessibility snapshot.
    pub(super) fn is_placed(&self, row: usize) -> bool {
        self.placed.get(row).is_some_and(Option::is_some)
    }

    pub(super) fn anchor(&self, row: usize) -> Option<(f64, f64)> {
        self.placed.get(row).copied().flatten().map(|p| (p.x, p.y))
    }

    /// The placed word whose box contains the point; hits test the exact measured boxes so a
    /// point shared by no box misses, independent of the placement grid.
    pub(super) fn hit(&self, x: f64, y: f64) -> Option<(GeneralLayoutElement, usize)> {
        for (row, placed) in self.placed.iter().enumerate() {
            let Some(p) = placed else { continue };
            let (width, size) = (self.widths[row], self.sizes[row]);
            let (hw, hh) = if self.angles[row] != 0.0 {
                (0.5 * size + BOX_PADDING, 0.5 * width + BOX_PADDING)
            } else {
                (0.5 * width + BOX_PADDING, 0.5 * size + BOX_PADDING)
            };
            if (x - p.x).abs() <= hw && (y - p.y).abs() <= hh {
                return Some((GeneralLayoutElement::Word, row));
            }
        }
        None
    }
}

impl LayoutSeriesEntry {
    pub(super) fn with_word_cloud<R>(
        &self,
        engine: &ChartEngine,
        words: &[WordCloudWord],
        plot: GeneralPlotRect,
        f: impl FnOnce(&WordCloudCache) -> R,
    ) -> R {
        let mut cache = self.word_cloud.borrow_mut();
        if cache.as_ref().is_none_or(|cached| cached.plot != plot) {
            *cache = Some(WordCloudCache::new(engine, self, words, plot));
        }
        f(cache.as_ref().expect("word cloud cache initialized"))
    }
}

impl ChartEngine {
    pub(super) fn build_word_cloud_frame(
        &self,
        series: &LayoutSeriesEntry,
        words: &[WordCloudWord],
        plot: GeneralPlotRect,
        (hpr, vpr): (f64, f64),
        out: &mut Vec<Prim>,
    ) {
        let family = self.options.get().layout.font_family.clone();
        self.with_word_cloud_series(series, words, plot, |cache| {
            for (row, word) in words.iter().enumerate() {
                let Some(p) = cache.placed[row] else { continue };
                let size = cache.sizes[row];
                let color = word
                    .color
                    .as_deref()
                    .and_then(Color::parse_css)
                    .unwrap_or(series.color);
                if cache.angles[row] != 0.0 {
                    out.push(Prim::RotatedText {
                        x: (p.x * hpr) as f32,
                        y: (p.y * vpr) as f32,
                        text: word.text.clone(),
                        color,
                        size: (size * vpr) as f32,
                        family: family.clone(),
                        align: TextAlign::Center,
                        weight: 400,
                        italic: false,
                        angle: cache.angles[row] as f32,
                    });
                } else {
                    out.push(Prim::Text {
                        x: (p.x * hpr) as f32,
                        y: (p.y * vpr) as f32,
                        text: word.text.clone(),
                        color,
                        size: (size * vpr) as f32,
                        family: family.clone(),
                        align: TextAlign::Center,
                        weight: 400,
                        italic: false,
                    });
                }
            }
        });
    }

    fn with_word_cloud_series<R>(
        &self,
        series: &LayoutSeriesEntry,
        words: &[WordCloudWord],
        plot: GeneralPlotRect,
        f: impl FnOnce(&WordCloudCache) -> R,
    ) -> R {
        series.with_word_cloud(self, words, plot, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_rejects_empty_and_overweight_data() {
        assert!(validate(&[]).is_err());
        assert!(
            validate(&[WordCloudWord {
                text: "ok".into(),
                weight: f64::NAN,
                color: None,
            }])
            .is_err()
        );
        assert!(
            validate(&[WordCloudWord {
                text: "  ".into(),
                weight: 1.0,
                color: None,
            }])
            .is_err()
        );
        assert!(
            validate(&[WordCloudWord {
                text: "ok".into(),
                weight: 1.0,
                color: Some("not-a-color".into()),
            }])
            .is_err()
        );
        assert!(
            validate(&[WordCloudWord {
                text: "ok".into(),
                weight: 1.0,
                color: Some("#ff0000".into()),
            }])
            .is_ok()
        );
    }

    #[test]
    fn options_validation_bounds_font_sizes() {
        let mut options = GeneralLayoutSeriesOptions::new(
            0,
            crate::GeneralLayoutDatasetId::from_raw(1).expect("id"),
            crate::GeneralLayoutSeriesKind::WordCloud,
        );
        assert!(validate_options(&options).is_ok());
        options.word_cloud_min_size = 100.0;
        options.word_cloud_max_size = 10.0;
        assert!(validate_options(&options).is_err());
        options.word_cloud_min_size = f64::NAN;
        assert!(validate_options(&options).is_err());
    }
}
