//! The one line layout every drawing label shares. `\n` separates lines; each line box is
//! [`LINE_HEIGHT_EM`] tall, the block is as wide as its widest measured line, and every line is
//! aligned at the label's anchor by the drawing's horizontal alignment. The anchor is where a
//! one-line label sits; further lines grow away from it: above for `Top`, below for `Bottom`,
//! evenly for `Middle`. Lines stack along the label's normal, so a rotated line label stays
//! readable. Placement, painting, hit testing, the caret, and the host editor all read this
//! layout, so every line's glyphs, grab area, and caret coincide.

use super::{DrawingTextHAlign, DrawingTextVAlign};

/// Line box height in ems.
pub(crate) const LINE_HEIGHT_EM: f64 = 1.2;

/// Lines laid out per label: every label the editor accepts fits (each byte a line break), and
/// longer text set through the API or persistence stays bounded frame work.
pub(crate) const MAX_DRAWING_TEXT_LINES: usize =
    crate::drawing_text_edit::MAX_DRAWING_TEXT_BYTES + 1;

/// Drawing text keeps its line breaks: `\r\n` and lone `\r` become `\n`, and every other
/// control character becomes a space. Every entry path (drawing options, templates, persistence
/// import, the typing session, and paste) applies exactly this, so a host that shows or edits
/// drawing text outside the chart calls it to see the text the chart will hold.
pub fn normalize_drawing_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                chars.next_if_eq(&'\n');
                normalized.push('\n');
            }
            '\n' => normalized.push('\n'),
            c if c.is_control() => normalized.push(' '),
            c => normalized.push(c),
        }
    }
    normalized
}

/// The start of a run `advance` wide, along the run, from an anchor aligned by `align`.
pub(crate) fn aligned_start(align: DrawingTextHAlign, advance: f64) -> f64 {
    match align {
        DrawingTextHAlign::Left => 0.0,
        DrawingTextHAlign::Center => -advance / 2.0,
        DrawingTextHAlign::Right => -advance,
    }
}

/// Where one label is laid out, in the caller's px basis: glyph `size`, the anchor its lines
/// align at, the horizontal alignment, the clockwise rotation in radians, and the vertical
/// alignment that decides which way extra lines grow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextRun {
    pub(crate) size: f64,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) align: DrawingTextHAlign,
    pub(crate) angle: f64,
    pub(crate) v_align: DrawingTextVAlign,
}

impl TextRun {
    /// A screen point in the run's own axes (x along the run, y along its normal).
    pub(crate) fn local(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let (sin, cos) = self.angle.sin_cos();
        let (dx, dy) = (x - self.x, y - self.y);
        (dx * cos + dy * sin, -dx * sin + dy * cos)
    }

    /// A point in the run's own axes back on screen.
    pub(crate) fn screen(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let (sin, cos) = self.angle.sin_cos();
        (self.x + x * cos - y * sin, self.y + x * sin + y * cos)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextLine<'a> {
    pub(crate) text: &'a str,
    /// Offset of the line's first `char` in the whole label.
    pub(crate) start: usize,
    /// Measured advance; 0 for an empty line or an unmeasured block.
    pub(crate) advance: f64,
}

/// One label's lines in the label's own axes: x along the run from the anchor, y along its
/// normal (downwards on screen while unrotated) from the anchor.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextBlock<'a> {
    pub(crate) lines: Vec<TextLine<'a>>,
    /// The widest line's advance.
    pub(crate) width: f64,
    pub(crate) line_height: f64,
    /// The first line's vertical center relative to the anchor.
    first_center: f64,
}

impl<'a> TextBlock<'a> {
    /// The line stack alone, for emitting glyphs at their anchors.
    pub(crate) fn unmeasured(text: &'a str, size: f64, v_align: DrawingTextVAlign) -> Self {
        Self::measured(text, size, v_align, |_| 0.0)
    }

    /// The line stack with each non-empty line measured by `measure`.
    pub(crate) fn measured(
        text: &'a str,
        size: f64,
        v_align: DrawingTextVAlign,
        mut measure: impl FnMut(&str) -> f64,
    ) -> Self {
        let mut lines = Vec::with_capacity(1);
        let mut start = 0;
        let mut width = 0.0_f64;
        for line in text.split('\n').take(MAX_DRAWING_TEXT_LINES) {
            let advance = if line.is_empty() { 0.0 } else { measure(line) };
            width = width.max(advance);
            lines.push(TextLine {
                text: line,
                start,
                advance,
            });
            start += line.chars().count() + 1;
        }
        let line_height = size * LINE_HEIGHT_EM;
        let extra = lines.len().saturating_sub(1) as f64 * line_height;
        let first_center = match v_align {
            DrawingTextVAlign::Top => -extra,
            DrawingTextVAlign::Middle => -extra / 2.0,
            DrawingTextVAlign::Bottom => 0.0,
        };
        Self {
            lines,
            width,
            line_height,
            first_center,
        }
    }

    /// A label with no glyphs keeps a one-em slot, so its chrome, hit target, and caret stay a
    /// caret-sized box instead of collapsing.
    pub(crate) fn keep_empty_slot(&mut self, size: f64) {
        if self.width <= 0.0 {
            self.width = size;
            for line in &mut self.lines {
                line.advance = size;
            }
        }
    }

    pub(crate) fn height(&self) -> f64 {
        self.lines.len() as f64 * self.line_height
    }

    /// The block's top edge relative to the anchor.
    pub(crate) fn top(&self) -> f64 {
        self.first_center - self.line_height / 2.0
    }

    /// Line `index`'s vertical center relative to the anchor.
    pub(crate) fn line_center(&self, index: usize) -> f64 {
        self.first_center + index as f64 * self.line_height
    }

    /// The block's left edge along the run for alignment `align`.
    pub(crate) fn left(&self, align: DrawingTextHAlign) -> f64 {
        aligned_start(align, self.width)
    }

    /// Line `index`'s left edge along the run: every line aligns at the anchor on its own.
    pub(crate) fn line_left(&self, index: usize, align: DrawingTextHAlign) -> f64 {
        aligned_start(
            align,
            self.lines.get(index).map_or(0.0, |line| line.advance),
        )
    }

    /// Whether a point in the run's axes falls on the block padded by `pad`.
    pub(crate) fn contains(&self, (x, y): (f64, f64), align: DrawingTextHAlign, pad: f64) -> bool {
        let (left, top) = (self.left(align), self.top());
        x >= left - pad
            && x <= left + self.width + pad
            && y >= top - pad
            && y <= top + self.height() + pad
    }

    /// The line holding `char` offset `caret` and the text before the caret on that line.
    pub(crate) fn caret_prefix(&self, caret: usize) -> (usize, &'a str) {
        let index = self.line_at(caret);
        let line = &self.lines[index];
        let chars = caret.saturating_sub(line.start);
        let end = line
            .text
            .char_indices()
            .nth(chars)
            .map_or(line.text.len(), |(byte, _)| byte);
        (index, &line.text[..end])
    }

    /// The `char` offset on line `index` whose boundary is nearest `local_x` along the run.
    pub(crate) fn caret_near(
        &self,
        index: usize,
        local_x: f64,
        align: DrawingTextHAlign,
        mut measure: impl FnMut(&str) -> f64,
    ) -> usize {
        let Some(line) = self.lines.get(index) else {
            return 0;
        };
        let x = local_x - self.line_left(index, align);
        let mut best = (0, x.abs());
        for (count, (byte, c)) in line.text.char_indices().enumerate() {
            let distance = (measure(&line.text[..byte + c.len_utf8()]) - x).abs();
            if distance < best.1 {
                best = (count + 1, distance);
            }
        }
        line.start + best.0
    }

    /// The line holding `char` offset `caret`; a caret right after a break starts the next line.
    pub(crate) fn line_at(&self, caret: usize) -> usize {
        self.lines
            .iter()
            .rposition(|line| line.start <= caret)
            .unwrap_or(0)
    }

    /// The line whose box is nearest to a local y, clamped to the first and last lines.
    pub(crate) fn line_near(&self, local_y: f64) -> usize {
        let index = ((local_y - self.first_center) / self.line_height).round();
        (index.max(0.0) as usize).min(self.lines.len().saturating_sub(1))
    }

    /// Each non-empty line's paint anchor (the aligned edge on the line's vertical center) for
    /// a label anchored at `(x, y)` and rotated by `angle` radians.
    pub(crate) fn line_anchors(
        &self,
        x: f64,
        y: f64,
        angle: f64,
    ) -> impl Iterator<Item = (f64, f64, &'a str)> + '_ {
        let (sin, cos) = angle.sin_cos();
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, line)| !line.text.is_empty())
            .map(move |(index, line)| {
                let offset = self.line_center(index);
                (x - sin * offset, y + cos * offset, line.text)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_keeps_line_breaks_and_flattens_other_controls() {
        assert_eq!(
            normalize_drawing_text("a\r\nb\rc\nd\te\u{7}"),
            "a\nb\nc\nd e "
        );
        assert_eq!(normalize_drawing_text("plain"), "plain");
        assert_eq!(normalize_drawing_text("\r\r\n"), "\n\n");
    }

    #[test]
    fn the_block_is_the_widest_line_and_grows_away_from_its_anchor() {
        let measure = |line: &str| line.chars().count() as f64 * 10.0;
        let block = TextBlock::measured("ab\n\nabcd", 10.0, DrawingTextVAlign::Bottom, measure);
        assert_eq!(block.lines.len(), 3);
        assert_eq!(block.width, 40.0);
        assert_eq!(block.height(), 36.0);
        assert_eq!(
            block
                .lines
                .iter()
                .map(|line| line.start)
                .collect::<Vec<_>>(),
            [0, 3, 4]
        );
        // Bottom keeps the first line where a one-line label sits and stacks below it.
        assert_eq!((block.line_center(0), block.top()), (0.0, -6.0));
        let top = TextBlock::measured("ab\n\nabcd", 10.0, DrawingTextVAlign::Top, measure);
        assert_eq!((top.line_center(2), top.top()), (0.0, -30.0));
        let middle = TextBlock::measured("ab\n\nabcd", 10.0, DrawingTextVAlign::Middle, measure);
        assert_eq!((middle.line_center(1), middle.top()), (0.0, -18.0));
        // One line keeps today's single-line box exactly.
        let single = TextBlock::measured("abc", 10.0, DrawingTextVAlign::Top, measure);
        assert_eq!(
            (single.width, single.height(), single.top()),
            (30.0, 12.0, -6.0)
        );
    }

    #[test]
    fn carets_and_points_resolve_to_lines() {
        let block = TextBlock::unmeasured("ab\ncd", 10.0, DrawingTextVAlign::Bottom);
        assert_eq!([0, 2, 3, 5].map(|caret| block.line_at(caret)), [0, 0, 1, 1]);
        assert_eq!(block.line_near(-50.0), 0);
        assert_eq!(block.line_near(7.0), 1);
        assert_eq!(block.line_near(500.0), 1);
        // Lines stack along the normal of a rotated label; empty lines paint nothing.
        let quarter = std::f64::consts::FRAC_PI_2;
        let anchors: Vec<_> = block.line_anchors(100.0, 50.0, quarter).collect();
        assert_eq!(anchors.len(), 2);
        assert!((anchors[1].0 - (100.0 - 12.0)).abs() < 1e-9);
        assert!((anchors[1].1 - 50.0).abs() < 1e-9);
        let gap = TextBlock::unmeasured("a\n\nb", 10.0, DrawingTextVAlign::Bottom);
        assert_eq!(gap.line_anchors(0.0, 0.0, 0.0).count(), 2);
    }
}
