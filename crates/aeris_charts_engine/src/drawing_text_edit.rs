//! Engine-owned typing session for text-capable drawings (standalone text, box annotations,
//! and line labels).
//!
//! The session owns the product rules every host shares: live text updates, caret position,
//! Enter/Shift+Enter/Escape semantics, and the empty lifecycle (an empty standalone text drawing
//! is removed; an empty trend label simply has no label). Labels are multi-line: `\n` separates
//! lines laid out by the shared line rule (drawings/text_block.rs), and caret motion follows that
//! measured layout. Hosts only forward committed text input and editing keys. A browser host may
//! keep its native editable surface for IME, clipboard, and accessibility and mirror it through
//! [`ChartEngine::set_drawing_text_edit`]; native hosts ask the engine to paint the caret and
//! the selection in the canonical frame.

use crate::ChartEngine;
use crate::drawings::{
    DrawingId, DrawingKind, DrawingTextHAlign, drawing_text_weight, normalize_drawing_text,
};

/// Largest label the drawing contract accepts, in UTF-8 bytes.
pub(crate) const MAX_DRAWING_TEXT_BYTES: usize = 256;

/// Editing keys a host forwards while a drawing text session is open. Movement keys extend the
/// selection when the host passes `extend_selection` (Shift); word variants follow the host's
/// word modifier (Ctrl on Windows/Linux, Alt/Option on macOS). `Home`/`End` go to the caret
/// line's start and end; `Up`/`Down` move to the neighboring line at the nearest measured x,
/// or to the label's start or end from its first or last line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawingTextEditKey {
    Backspace,
    Delete,
    DeleteWordBackward,
    DeleteWordForward,
    Left,
    Right,
    WordLeft,
    WordRight,
    Home,
    End,
    Up,
    Down,
}

/// The browser's transparent editing surface follows this engine-owned line block and caret.
/// Coordinates are CSS-logical media pixels in the chart overlay's space; edges and the caret
/// are given before the label's rotation by `angle` around the anchor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawingTextEditLayout<'a> {
    pub anchor_x: f64,
    pub anchor_y: f64,
    pub angle: f64,
    pub font_size: f64,
    pub font_family: &'a str,
    pub font_weight: u16,
    pub font_italic: bool,
    /// The alignment every line takes at the anchor.
    pub align: DrawingTextHAlign,
    /// The line block: its left and top edges, its width (the widest line, one em while empty),
    /// and its height (one line box per line).
    pub left_edge: f64,
    pub top_edge: f64,
    pub advance: f64,
    pub height: f64,
    pub line_height: f64,
    pub line_count: usize,
    /// The caret: its line, its x, and the top of its line box.
    pub caret_line: usize,
    pub caret_x: f64,
    pub caret_top: f64,
    /// The ink the frame paints the label with, for a host-drawn caret.
    pub ink: aeris_charts_render::color::Color,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DrawingTextEditSession {
    pub(crate) id: DrawingId,
    original: String,
    pub(crate) text: String,
    /// Caret position in `char`s from the start of `text`.
    pub(crate) caret: usize,
    /// Selection anchor in `char`s; the selection spans anchor..caret. Typing, deletion, copy,
    /// and cut all honor it, and the frame paints it wherever it paints the caret.
    anchor: Option<usize>,
    /// Native hosts have no editable surface of their own, so the frame paints the caret.
    pub(crate) paint_caret: bool,
    /// The painted caret's blink phase: shown, and the host-clock time of its next toggle.
    /// Every edit or caret move shows it and restarts the cycle (`None` until the next tick
    /// supplies the clock).
    pub(crate) caret_shown: bool,
    pub(crate) caret_toggle_ms: Option<f64>,
}

/// Half of the painted caret's blink cycle (the common platform 1.06 s rate).
pub(crate) const CARET_BLINK_MS: f64 = 530.0;

/// Start of the word before `caret`: skip whitespace leftwards, then the word itself.
fn word_left(chars: &[char], caret: usize) -> usize {
    let mut index = caret.min(chars.len());
    while index > 0 && chars[index - 1].is_whitespace() {
        index -= 1;
    }
    while index > 0 && !chars[index - 1].is_whitespace() {
        index -= 1;
    }
    index
}

/// Start of the next word after `caret`: skip the current word, then whitespace.
fn word_right(chars: &[char], caret: usize) -> usize {
    let mut index = caret.min(chars.len());
    while index < chars.len() && !chars[index].is_whitespace() {
        index += 1;
    }
    while index < chars.len() && chars[index].is_whitespace() {
        index += 1;
    }
    index
}

fn byte_index(text: &str, caret: usize) -> usize {
    text.char_indices()
        .nth(caret)
        .map_or(text.len(), |(index, _)| index)
}

impl DrawingTextEditSession {
    /// The selected `char` range, if any.
    pub(crate) fn selection(&self) -> Option<(usize, usize)> {
        self.anchor
            .filter(|&anchor| anchor != self.caret)
            .map(|anchor| (anchor.min(self.caret), anchor.max(self.caret)))
    }
}

impl ChartEngine {
    /// Open typing mode on a standalone text drawing or a trend-line label. Any open session is
    /// committed first. `paint_caret` asks the frame to draw the caret (native hosts); a browser
    /// host that paints its own caret passes `false`. Returns `false` for a drawing that cannot be
    /// edited (unknown, locked, hidden, or not text-capable).
    pub fn begin_drawing_text_edit(&mut self, id: DrawingId, paint_caret: bool) -> bool {
        self.commit_drawing_text_edit();
        self.set_editing_drawing(Some(id));
        if self.editing_drawing != Some(id) {
            return false;
        }
        let Some(drawing) = self.drawing(id) else {
            self.set_editing_drawing(None);
            return false;
        };
        let text = drawing.text.clone();
        let caret = text.chars().count();
        self.drawing_text_edit = Some(DrawingTextEditSession {
            id,
            original: text.clone(),
            text,
            caret,
            anchor: None,
            paint_caret,
            caret_shown: true,
            caret_toggle_ms: None,
        });
        self.invalidate_frame_drawings();
        true
    }

    /// The open session as `(drawing, text, caret)`, with the caret in `char`s.
    pub fn drawing_text_edit(&self) -> Option<(DrawingId, &str, usize)> {
        self.drawing_text_edit
            .as_ref()
            .map(|session| (session.id, session.text.as_str(), session.caret))
    }

    /// Resolve the open editor from the same run, line layout, and text measurements as the
    /// frame.
    pub fn drawing_text_edit_layout(&self) -> Option<DrawingTextEditLayout<'_>> {
        let session = self.drawing_text_edit.as_ref()?;
        let drawing = self.drawing(session.id)?;
        let px = self.drawing_px(drawing)?;
        let run = self.drawing_text_run(drawing, &px, self.pane_w, 1.0)?;
        let block = self.drawing_label_block(drawing, run.size);
        let measure = self.drawing_text_measurer(drawing, run.size);
        let (line, prefix) = block.caret_prefix(session.caret);
        Some(DrawingTextEditLayout {
            anchor_x: run.x,
            anchor_y: run.y,
            angle: run.angle,
            font_size: run.size,
            font_family: &self.options.get().layout.font_family,
            font_weight: drawing_text_weight(drawing),
            font_italic: drawing.text_italic,
            align: run.align,
            left_edge: run.x + block.left(run.align),
            top_edge: run.y + block.top(),
            advance: block.width,
            height: block.height(),
            line_height: block.line_height,
            line_count: block.lines.len(),
            caret_line: line,
            caret_x: run.x + block.line_left(line, run.align) + measure(prefix).ceil(),
            caret_top: run.y + block.line_center(line) - block.line_height / 2.0,
            ink: self.drawing_text_color(drawing),
        })
    }

    /// Advance the engine-painted caret's blink on the host clock (from [`Self::input_tick`]).
    /// The first tick after an edit starts the cycle; returns whether the caret toggled.
    pub(crate) fn tick_drawing_text_caret(&mut self, now_ms: f64) -> bool {
        let Some(session) = self
            .drawing_text_edit
            .as_mut()
            .filter(|session| session.paint_caret)
        else {
            return false;
        };
        match session.caret_toggle_ms {
            None => {
                session.caret_toggle_ms = Some(now_ms + CARET_BLINK_MS);
                false
            }
            Some(deadline) if now_ms >= deadline => {
                // A stalled host resumes on the cycle instead of replaying missed toggles.
                let missed = ((now_ms - deadline) / CARET_BLINK_MS).floor();
                if missed % 2.0 == 0.0 {
                    session.caret_shown = !session.caret_shown;
                }
                session.caret_toggle_ms = Some(deadline + (missed + 1.0) * CARET_BLINK_MS);
                self.invalidate_frame_drawings();
                true
            }
            Some(_) => false,
        }
    }

    /// When the engine-painted caret next toggles, while one is blinking.
    pub(crate) fn drawing_text_caret_deadline_ms(&self) -> Option<f64> {
        self.drawing_text_edit
            .as_ref()
            .filter(|session| session.paint_caret)
            .and_then(|session| session.caret_toggle_ms)
    }

    /// A host with a native text input surface paints its own caret over the shared label.
    pub fn set_drawing_text_edit_paint_caret(&mut self, paint_caret: bool) {
        if let Some(session) = self.drawing_text_edit.as_mut()
            && session.paint_caret != paint_caret
        {
            session.paint_caret = paint_caret;
            self.invalidate_frame_drawings();
        }
    }

    /// The selected text of the open session (for host copy/cut), if any.
    pub fn drawing_text_edit_selection(&self) -> Option<&str> {
        let session = self.drawing_text_edit.as_ref()?;
        let (start, end) = session.selection()?;
        let text = session.text.as_str();
        Some(&text[byte_index(text, start)..byte_index(text, end)])
    }

    /// Select the whole label (Ctrl/Cmd+A).
    pub fn drawing_text_edit_select_all(&mut self) -> bool {
        let Some(session) = self.drawing_text_edit.as_mut() else {
            return false;
        };
        let len = session.text.chars().count();
        if session.anchor == Some(0) && session.caret == len {
            return false;
        }
        session.anchor = Some(0);
        session.caret = len;
        self.invalidate_frame_drawings();
        true
    }

    /// Insert committed text input at the caret, replacing any selection. Line breaks are kept
    /// (`\r\n` and `\r` become `\n`); other control characters become spaces. Input that
    /// would exceed the label limit is rejected whole so a paste never lands half-applied.
    pub fn drawing_text_edit_insert(&mut self, input: &str) -> bool {
        let Some(session) = self.drawing_text_edit.as_ref() else {
            return false;
        };
        let input = normalize_drawing_text(input);
        let (start, end) = session
            .selection()
            .unwrap_or((session.caret, session.caret));
        let mut text = session.text.clone();
        let (from, to) = (byte_index(&text, start), byte_index(&text, end));
        if input.is_empty() || text.len() - (to - from) + input.len() > MAX_DRAWING_TEXT_BYTES {
            return false;
        }
        text.replace_range(from..to, &input);
        let caret = start + input.chars().count();
        self.apply_drawing_text_edit_with_anchor(text, caret, None)
    }

    /// Enter: Shift+Enter (`line_break`) inserts a line break at the caret; plain Enter commits.
    pub fn drawing_text_edit_enter(&mut self, line_break: bool) -> bool {
        if line_break {
            self.drawing_text_edit_insert("\n")
        } else {
            self.commit_drawing_text_edit()
        }
    }

    /// Apply one editing key; `extend_selection` (Shift) turns movement into selection.
    /// Deletion removes the selection when there is one. Returns whether anything changed.
    pub fn drawing_text_edit_key(
        &mut self,
        key: DrawingTextEditKey,
        extend_selection: bool,
    ) -> bool {
        let Some(session) = self.drawing_text_edit.as_ref() else {
            return false;
        };
        let chars: Vec<char> = session.text.chars().collect();
        let len = chars.len();
        let caret = session.caret.min(len);
        let range = session.selection();
        let deletion = match key {
            DrawingTextEditKey::Backspace => Some((caret.saturating_sub(1), caret)),
            DrawingTextEditKey::Delete => Some((caret, (caret + 1).min(len))),
            DrawingTextEditKey::DeleteWordBackward => Some((word_left(&chars, caret), caret)),
            DrawingTextEditKey::DeleteWordForward => Some((caret, word_right(&chars, caret))),
            _ => None,
        };
        let (text, next_caret, anchor) = if let Some(span) = deletion {
            let (start, end) = range.unwrap_or(span);
            if start == end {
                return false;
            }
            let mut text = session.text.clone();
            text.replace_range(byte_index(&text, start)..byte_index(&text, end), "");
            (text, start, None)
        } else {
            let target = match key {
                DrawingTextEditKey::Left if !extend_selection && range.is_some() => {
                    range.map_or(caret, |(start, _)| start)
                }
                DrawingTextEditKey::Right if !extend_selection && range.is_some() => {
                    range.map_or(caret, |(_, end)| end)
                }
                DrawingTextEditKey::Left => caret.saturating_sub(1),
                DrawingTextEditKey::Right => (caret + 1).min(len),
                DrawingTextEditKey::WordLeft => word_left(&chars, caret),
                DrawingTextEditKey::WordRight => word_right(&chars, caret),
                key => self.drawing_text_edit_line_target(key, caret, len),
            };
            let anchor = extend_selection.then(|| session.anchor.unwrap_or(caret));
            (session.text.clone(), target, anchor)
        };
        if text == session.text && next_caret == session.caret && anchor == session.anchor {
            return false;
        }
        self.apply_drawing_text_edit_with_anchor(text, next_caret, anchor)
    }

    /// Where a line-aware motion key moves the caret, from the label's measured line layout:
    /// `Home`/`End` to the caret line's ends, `Up`/`Down` to the neighboring line's
    /// boundary nearest the caret's x (the label's start or end past the first or last line).
    fn drawing_text_edit_line_target(
        &self,
        key: DrawingTextEditKey,
        caret: usize,
        len: usize,
    ) -> usize {
        let up = matches!(key, DrawingTextEditKey::Home | DrawingTextEditKey::Up);
        let Some(drawing) = self
            .drawing_text_edit
            .as_ref()
            .and_then(|session| self.drawing(session.id))
        else {
            return if up { 0 } else { len };
        };
        let size = drawing.resolved_text_size(self.options.get().layout.font_size);
        let block = self.drawing_label_block(drawing, size);
        let (line, prefix) = block.caret_prefix(caret);
        let current = &block.lines[line];
        let target = match key {
            DrawingTextEditKey::Home => return current.start,
            DrawingTextEditKey::End => return current.start + current.text.chars().count(),
            DrawingTextEditKey::Up => line.checked_sub(1),
            _ => Some(line + 1).filter(|&next| next < block.lines.len()),
        };
        let Some(target) = target else {
            return if up { 0 } else { len };
        };
        let align = drawing.label_align();
        let measure = self.drawing_text_measurer(drawing, size);
        let x = block.line_left(line, align) + measure(prefix);
        block.caret_near(target, x, align, measure).min(len)
    }

    /// Place the caret at the character boundary nearest a media-px pointer, in the label's
    /// rotated coordinates: on the line nearest the pointer, at that line's nearest boundary
    /// (the browser editor's click-to-place behavior for native hosts).
    pub fn drawing_text_edit_caret_at(&mut self, x: f64, y: f64) -> bool {
        let Some(session) = self.drawing_text_edit.as_ref() else {
            return false;
        };
        let Some(drawing) = self.drawing(session.id) else {
            return false;
        };
        let Some(px) = self.drawing_px(drawing) else {
            return false;
        };
        let Some(run) = self.drawing_text_run(drawing, &px, self.pane_w, 1.0) else {
            return false;
        };
        let block = self.drawing_label_block(drawing, run.size);
        let (local_x, local_y) = run.local((x, y));
        let caret = block.caret_near(
            block.line_near(local_y),
            local_x,
            run.align,
            self.drawing_text_measurer(drawing, run.size),
        );
        if caret == session.caret && session.anchor.is_none() {
            return false;
        }
        let text = session.text.clone();
        self.apply_drawing_text_edit_with_anchor(text, caret, None)
    }

    /// Mirror a host-owned editable surface (browser IME/clipboard) into the session: the whole
    /// current value plus its caret in `char`s.
    pub fn set_drawing_text_edit(&mut self, text: &str, caret: usize) -> bool {
        if self.drawing_text_edit.is_none() {
            return false;
        }
        let text = normalize_drawing_text(text);
        if text.len() > MAX_DRAWING_TEXT_BYTES {
            return false;
        }
        let caret = caret.min(text.chars().count());
        self.apply_drawing_text_edit(text, caret)
    }

    /// Enter / blur: keep the typed text, trimmed of surrounding whitespace and blank lines. An
    /// empty standalone text drawing is removed; an empty trend label leaves the line without a
    /// label.
    pub fn commit_drawing_text_edit(&mut self) -> bool {
        let Some(session) = self.drawing_text_edit.take() else {
            return false;
        };
        let text = session.text.trim().to_string();
        self.finish_drawing_text_edit(session.id, text);
        true
    }

    /// Escape: restore the text from before the session. A standalone text drawing that started
    /// empty (a fresh placement) is removed.
    pub fn cancel_drawing_text_edit(&mut self) -> bool {
        let Some(session) = self.drawing_text_edit.take() else {
            return false;
        };
        self.finish_drawing_text_edit(session.id, session.original);
        true
    }

    fn apply_drawing_text_edit(&mut self, text: String, caret: usize) -> bool {
        self.apply_drawing_text_edit_with_anchor(text, caret, None)
    }

    fn apply_drawing_text_edit_with_anchor(
        &mut self,
        text: String,
        caret: usize,
        anchor: Option<usize>,
    ) -> bool {
        let Some(session) = self.drawing_text_edit.as_mut() else {
            return false;
        };
        let id = session.id;
        let text_changed = session.text != text;
        session.text = text;
        session.caret = caret;
        session.anchor = anchor;
        // Typing or moving the caret shows it solid and restarts the blink.
        session.caret_shown = true;
        session.caret_toggle_ms = None;
        if text_changed {
            let patch = serde_json::json!({ "text": session.text }).to_string();
            if !self.drawing_apply_options(id, &patch) {
                // The drawing disappeared underneath the session.
                self.drawing_text_edit = None;
                self.set_editing_drawing(None);
                return false;
            }
        } else {
            self.invalidate_frame_drawings();
        }
        true
    }

    fn finish_drawing_text_edit(&mut self, id: DrawingId, text: String) {
        self.set_editing_drawing(None);
        let standalone = self.drawing(id).is_some_and(|drawing| {
            drawing.kind == DrawingKind::Text || drawing.kind.is_text_annotation()
        });
        if text.trim().is_empty() && standalone {
            self.remove_drawing(id);
        } else {
            let current = self.drawing(id).map(|drawing| drawing.text.clone());
            if current.as_deref() != Some(text.as_str()) {
                let patch = serde_json::json!({ "text": text }).to_string();
                self.drawing_apply_options(id, &patch);
            }
        }
        self.invalidate_frame_drawings();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawings::DrawingPoint;
    use aeris_charts_render::draw_list::Prim;

    fn chart_with(kind: DrawingKind, text: &str) -> (ChartEngine, DrawingId) {
        let mut chart = ChartEngine::new(800.0, 400.0, 1.0);
        let times = (0..20)
            .map(|i| 1_700_000_000.0 + f64::from(i) * 60.0)
            .collect::<Vec<_>>();
        // Bars span 96..104 so the 99..101 anchors land inside the pane.
        let values = (0..20)
            .map(|i| if i % 2 == 0 { 96.0 } else { 104.0 })
            .collect::<Vec<_>>();
        chart
            .set_series_data(0, &times, &values, &values, &values, &values)
            .expect("valid bars");
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        let mut points = match kind {
            DrawingKind::Text => vec![DrawingPoint {
                logical: 5.0,
                price: 100.0,
            }],
            _ => vec![
                DrawingPoint {
                    logical: 2.0,
                    price: 99.0,
                },
                DrawingPoint {
                    logical: 12.0,
                    price: 101.0,
                },
            ],
        };
        points.truncate(kind.anchor_count());
        let id = chart.add_drawing(kind, 0, points, None).expect("drawing");
        if !text.is_empty() {
            let patch = serde_json::json!({ "text": text }).to_string();
            assert!(chart.drawing_apply_options(id, &patch));
        }
        (chart, id)
    }

    fn text(chart: &ChartEngine, id: DrawingId) -> Option<String> {
        chart.drawing(id).map(|drawing| drawing.text.clone())
    }

    #[test]
    fn editor_layout_uses_the_frame_anchor_font_and_caret_metrics() {
        let (mut chart, id) = chart_with(DrawingKind::TrendLine, "Parity");
        chart.build_frame();
        for align in ["left", "center", "right"] {
            assert!(
                chart.drawing_apply_options(
                    id,
                    &serde_json::json!({
                        "text_h_align": align,
                        "text_size": 19,
                        "text_weight": 700,
                        "text_italic": true,
                    })
                    .to_string(),
                )
            );
            assert!(chart.begin_drawing_text_edit(id, false));
            let geometry = chart.drawing_text_edit_layout().expect("live layout");
            let (x, y, angle) = chart.drawing_text_transform(id).expect("text transform");
            assert_eq!(
                (geometry.anchor_x, geometry.anchor_y, geometry.angle),
                (x, y, angle)
            );
            assert_eq!(geometry.font_size, 19.0);
            assert_eq!(geometry.font_weight, 700);
            assert!(geometry.font_italic);
            assert_eq!(geometry.font_family, chart.options.get().layout.font_family);
            let expected_left = match align {
                "left" => x,
                "center" => x - geometry.advance / 2.0,
                _ => x - geometry.advance,
            };
            assert!((geometry.left_edge - expected_left).abs() < 1e-9);
            assert_eq!(
                geometry.caret_x,
                geometry.left_edge + geometry.advance.ceil()
            );
            assert!(chart.cancel_drawing_text_edit());
        }
    }

    #[test]
    fn every_line_kind_types_its_label_in_place_with_the_line_ink() {
        use aeris_charts_render::color::Color;

        for kind in [
            DrawingKind::TrendLine,
            DrawingKind::Ray,
            DrawingKind::ExtendedLine,
            DrawingKind::HorizontalLine,
            DrawingKind::HorizontalRay,
            DrawingKind::VerticalLine,
        ] {
            let (mut chart, id) = chart_with(kind, "Level");
            assert!(chart.drawing_apply_options(id, r##"{"color":"#ff9900"}"##));
            chart.build_frame();
            let (x, y, _) = chart.drawing_text_transform(id).expect("text transform");
            assert_eq!(chart.drawing_text_hit_at(x, y), Some(id), "{kind:?}");
            assert!(chart.begin_drawing_text_edit(id, false), "{kind:?}");
            let layout = chart.drawing_text_edit_layout().expect("live layout");
            assert_eq!(layout.ink, Color::rgb(0xff, 0x99, 0x00), "{kind:?}");
            assert!(chart.cancel_drawing_text_edit());
            assert!(chart.drawing_apply_options(id, r##"{"text_color":"#3366ff"}"##));
            assert!(chart.begin_drawing_text_edit(id, false));
            assert_eq!(
                chart.drawing_text_edit_layout().expect("live layout").ink,
                Color::rgb(0x33, 0x66, 0xff),
                "{kind:?}: an explicit text color wins"
            );
            assert!(chart.cancel_drawing_text_edit());
        }
    }

    #[test]
    fn typing_edits_the_trend_label_live_at_the_caret() {
        let (mut chart, id) = chart_with(DrawingKind::TrendLine, "");
        assert!(chart.begin_drawing_text_edit(id, true));
        assert_eq!(chart.editing_drawing(), Some(id));
        assert!(chart.drawing_text_edit_insert("Brkoutx"));
        assert_eq!(text(&chart, id).as_deref(), Some("Brkoutx"));
        assert!(chart.drawing_text_edit_key(DrawingTextEditKey::Backspace, false));
        for _ in 0..4 {
            chart.drawing_text_edit_key(DrawingTextEditKey::Left, false);
        }
        assert!(chart.drawing_text_edit_insert("ea"));
        assert_eq!(chart.drawing_text_edit(), Some((id, "Breakout", 4)));
        chart.drawing_text_edit_key(DrawingTextEditKey::End, false);
        assert!(chart.drawing_text_edit_insert("\r\nnow"));
        assert!(chart.commit_drawing_text_edit());
        assert_eq!(text(&chart, id).as_deref(), Some("Breakout\nnow"));
        assert_eq!(chart.editing_drawing(), None);
        assert!(chart.drawing_text_edit().is_none());
    }

    #[test]
    fn the_painted_caret_blinks_on_the_host_clock_and_restarts_solid_on_edits() {
        let (mut chart, id) = chart_with(DrawingKind::Text, "Blink");
        assert!(chart.begin_drawing_text_edit(id, true));
        // The caret: one crisp 1 px rule the height of the line box.
        let caret_rects = |chart: &mut ChartEngine| {
            chart.build_frame().panes[0]
                .main
                .iter()
                .filter(
                    |prim| matches!(prim, Prim::Rect { rect, .. } if rect.w == 1 && rect.h > 10),
                )
                .count()
        };
        assert_eq!(caret_rects(&mut chart), 1);
        // The first tick starts the cycle; the caret hides after half a period, then returns.
        assert!(!chart.input_tick(1_000.0));
        assert_eq!(
            chart.input_wake_deadline_ms(),
            Some(1_000.0 + CARET_BLINK_MS)
        );
        assert!(!chart.input_tick(1_000.0 + CARET_BLINK_MS - 1.0));
        assert!(chart.input_tick(1_000.0 + CARET_BLINK_MS));
        assert_eq!(caret_rects(&mut chart), 0, "hidden in the off phase");
        assert!(chart.input_tick(1_000.0 + 2.0 * CARET_BLINK_MS));
        assert_eq!(caret_rects(&mut chart), 1, "shown again");
        // A stalled host resumes in phase: three missed toggles leave it hidden.
        assert!(chart.input_tick(1_000.0 + 5.0 * CARET_BLINK_MS + 10.0));
        assert_eq!(caret_rects(&mut chart), 0);
        assert_eq!(
            chart.input_wake_deadline_ms(),
            Some(1_000.0 + 6.0 * CARET_BLINK_MS)
        );
        // Typing shows the caret solid and restarts the cycle from the next tick.
        assert!(chart.drawing_text_edit_insert("!"));
        assert_eq!(caret_rects(&mut chart), 1);
        assert_eq!(chart.input_wake_deadline_ms(), None);
        assert!(!chart.input_tick(5_000.0));
        assert_eq!(
            chart.input_wake_deadline_ms(),
            Some(5_000.0 + CARET_BLINK_MS)
        );
        // A host that paints its own caret (the browser) needs no blink wakes.
        assert!(chart.commit_drawing_text_edit());
        assert!(chart.begin_drawing_text_edit(id, false));
        assert!(!chart.input_tick(6_000.0));
        assert_eq!(chart.input_wake_deadline_ms(), None);
    }

    #[test]
    fn cancel_restores_and_empty_lifecycle_follows_the_drawing_kind() {
        let (mut chart, id) = chart_with(DrawingKind::TrendLine, "keep");
        assert!(chart.begin_drawing_text_edit(id, true));
        chart.drawing_text_edit_key(DrawingTextEditKey::Backspace, false);
        assert_eq!(text(&chart, id).as_deref(), Some("kee"));
        assert!(chart.cancel_drawing_text_edit());
        assert_eq!(text(&chart, id).as_deref(), Some("keep"));

        // Clearing a trend label keeps the line; clearing standalone text removes the drawing.
        assert!(chart.begin_drawing_text_edit(id, true));
        assert!(chart.set_drawing_text_edit("  ", 2));
        assert!(chart.commit_drawing_text_edit());
        assert_eq!(text(&chart, id).as_deref(), Some(""));

        let (mut chart, note) = chart_with(DrawingKind::Text, "");
        assert!(chart.begin_drawing_text_edit(note, false));
        assert!(chart.cancel_drawing_text_edit());
        assert!(chart.drawing(note).is_none());
    }

    #[test]
    fn selection_extends_replaces_deletes_and_collapses_like_a_text_field() {
        let (mut chart, id) = chart_with(DrawingKind::TrendLine, "buy the dip");
        assert!(chart.begin_drawing_text_edit(id, true));
        // Shift+word-left selects "dip"; typing replaces it.
        assert!(chart.drawing_text_edit_key(DrawingTextEditKey::WordLeft, true));
        assert_eq!(chart.drawing_text_edit_selection(), Some("dip"));
        assert!(chart.drawing_text_edit_insert("rip"));
        assert_eq!(chart.drawing_text_edit(), Some((id, "buy the rip", 11)));
        assert_eq!(chart.drawing_text_edit_selection(), None);

        // Ctrl+Backspace removes a word; select-all then Delete clears everything.
        assert!(chart.drawing_text_edit_key(DrawingTextEditKey::DeleteWordBackward, false));
        assert_eq!(chart.drawing_text_edit(), Some((id, "buy the ", 8)));
        assert!(chart.drawing_text_edit_select_all());
        assert_eq!(chart.drawing_text_edit_selection(), Some("buy the "));
        // A plain arrow collapses the selection to its edge without moving past it.
        assert!(chart.drawing_text_edit_key(DrawingTextEditKey::Left, false));
        assert_eq!(chart.drawing_text_edit(), Some((id, "buy the ", 0)));
        assert!(chart.drawing_text_edit_key(DrawingTextEditKey::End, true));
        assert!(chart.drawing_text_edit_key(DrawingTextEditKey::Delete, false));
        assert_eq!(chart.drawing_text_edit(), Some((id, "", 0)));
    }

    #[test]
    fn a_click_places_the_caret_at_the_nearest_character_boundary() {
        let (mut chart, id) = chart_with(DrawingKind::Text, "");
        chart.set_text_measure(Some(Box::new(|text, _, _, _, _| {
            text.chars().count() as f64 * 10.0
        })));
        assert!(chart.drawing_apply_options(id, r#"{"text":"abcd","text_h_align":"left"}"#));
        chart.build_frame();
        assert!(chart.begin_drawing_text_edit(id, true));
        let (x, y, _) = chart.drawing_text_transform(id).unwrap();
        assert!(chart.drawing_text_edit_caret_at(x + 12.0, y));
        assert_eq!(chart.drawing_text_edit(), Some((id, "abcd", 1)));
        assert!(chart.drawing_text_edit_caret_at(x + 26.0, y));
        assert_eq!(chart.drawing_text_edit(), Some((id, "abcd", 3)));
        assert!(chart.drawing_text_edit_caret_at(x - 30.0, y));
        assert_eq!(chart.drawing_text_edit(), Some((id, "abcd", 0)));
    }

    /// A measurer whose advance is 10 px per char, so line widths are exact.
    fn fixed_width(chart: &mut ChartEngine) {
        chart.set_text_measure(Some(Box::new(|text, _, _, _, _| {
            text.chars().count() as f64 * 10.0
        })));
    }

    #[test]
    fn every_entry_path_keeps_line_breaks_and_flattens_other_controls() {
        let (mut chart, id) = chart_with(DrawingKind::Text, "");
        assert!(chart.begin_drawing_text_edit(id, true));
        assert!(chart.drawing_text_edit_insert("a\r\nb\tc"));
        assert_eq!(chart.drawing_text_edit(), Some((id, "a\nb c", 5)));
        assert!(chart.drawing_text_edit_enter(true));
        assert_eq!(chart.drawing_text_edit(), Some((id, "a\nb c\n", 6)));
        assert!(chart.set_drawing_text_edit("x\ry\u{7}", 3));
        assert_eq!(chart.drawing_text_edit(), Some((id, "x\ny ", 3)));
        // Plain Enter commits, trimming the surrounding blank space but keeping inner breaks.
        assert!(chart.set_drawing_text_edit("\n top\n\nbottom \n", 0));
        assert!(chart.drawing_text_edit_enter(false));
        assert_eq!(text(&chart, id).as_deref(), Some("top\n\nbottom"));
        assert!(chart.drawing_apply_options(id, r#"{"text":"api\r\nset"}"#));
        assert_eq!(text(&chart, id).as_deref(), Some("api\nset"));
    }

    #[test]
    fn the_settings_session_stores_exactly_the_public_normalization() {
        let (mut chart, id) = chart_with(DrawingKind::Text, "");
        let typed = "first\r\nsecond\rthird\tend\u{1b}";
        assert!(chart.begin_drawing_options_edit(id));
        let patch = serde_json::json!({ "text": typed }).to_string();
        assert!(chart.drawing_apply_options(id, &patch));
        assert!(chart.commit_drawing_options_edit());
        let expected = crate::normalize_drawing_text(typed);
        assert_eq!(expected, "first\nsecond\nthird end ");
        assert_eq!(text(&chart, id).as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn the_editor_layout_spans_every_line_and_follows_the_caret_line() {
        let (mut chart, id) = chart_with(DrawingKind::Text, "");
        fixed_width(&mut chart);
        assert!(chart.drawing_apply_options(
            id,
            r#"{"text":"ab\nabcd","text_h_align":"center","text_v_align":"bottom","text_size":10}"#
        ));
        chart.build_frame();
        assert!(chart.begin_drawing_text_edit(id, false));
        let layout = chart.drawing_text_edit_layout().expect("live layout");
        let (x, y) = (layout.anchor_x, layout.anchor_y);
        assert_eq!(layout.align, DrawingTextHAlign::Center);
        assert_eq!((layout.line_count, layout.line_height), (2, 12.0));
        // The block is the widest line wide and two line boxes tall, growing down from the
        // one-line slot for a bottom label.
        assert_eq!((layout.advance, layout.height), (40.0, 24.0));
        assert_eq!((layout.left_edge, layout.top_edge), (x - 20.0, y - 6.0));
        // The caret ends the second line, which is centered on the anchor by itself.
        assert_eq!(layout.caret_line, 1);
        assert_eq!(layout.caret_x, x + 20.0);
        assert_eq!(layout.caret_top, y + 6.0);
        assert!(chart.drawing_text_edit_key(DrawingTextEditKey::Up, false));
        let layout = chart.drawing_text_edit_layout().expect("live layout");
        assert_eq!(chart.drawing_text_edit(), Some((id, "ab\nabcd", 2)));
        assert_eq!((layout.caret_line, layout.caret_x), (0, x + 10.0));
        assert_eq!(layout.caret_top, y - 6.0);
    }

    #[test]
    fn clicks_place_the_caret_on_the_nearest_line_and_backspace_joins_lines() {
        let (mut chart, id) = chart_with(DrawingKind::Text, "");
        fixed_width(&mut chart);
        assert!(chart.drawing_apply_options(
            id,
            r#"{"text":"abc\nde","text_h_align":"left","text_v_align":"bottom","text_size":10}"#
        ));
        chart.build_frame();
        assert!(chart.begin_drawing_text_edit(id, true));
        let (x, y, _) = chart.drawing_text_transform(id).unwrap();
        // The second line's box spans y + 6 ..= y + 18.
        assert!(chart.drawing_text_edit_caret_at(x + 11.0, y + 14.0));
        assert_eq!(chart.drawing_text_edit(), Some((id, "abc\nde", 5)));
        assert!(chart.drawing_text_edit_caret_at(x + 100.0, y + 40.0));
        assert_eq!(chart.drawing_text_edit(), Some((id, "abc\nde", 6)));
        assert!(chart.drawing_text_edit_caret_at(x + 21.0, y - 30.0));
        assert_eq!(chart.drawing_text_edit(), Some((id, "abc\nde", 2)));
        // Backspace at a line start joins it to the line above; Delete at a line end likewise.
        assert!(chart.drawing_text_edit_caret_at(x, y + 12.0));
        assert!(chart.drawing_text_edit_key(DrawingTextEditKey::Backspace, false));
        assert_eq!(chart.drawing_text_edit(), Some((id, "abcde", 3)));
        assert!(chart.drawing_text_edit_enter(true));
        assert!(chart.drawing_text_edit_key(DrawingTextEditKey::Left, false));
        assert!(chart.drawing_text_edit_key(DrawingTextEditKey::Delete, false));
        assert_eq!(chart.drawing_text_edit(), Some((id, "abcde", 3)));
        // Shift+Down on the last line selects to the label's end.
        assert!(chart.drawing_text_edit_key(DrawingTextEditKey::Down, true));
        assert_eq!(chart.drawing_text_edit_selection(), Some("de"));
    }

    #[test]
    fn line_labels_and_box_annotations_grow_with_every_line() {
        let (mut chart, line) = chart_with(DrawingKind::HorizontalLine, "");
        fixed_width(&mut chart);
        assert!(chart.drawing_apply_options(
            line,
            r#"{"text":"one\ntwo","text_v_align":"top","text_h_align":"center","text_size":10}"#
        ));
        chart.build_frame();
        // A top label grows upward: its second line holds the one-line slot, the first sits a
        // line above it, and both are text targets.
        let (x, y, _) = chart.drawing_text_transform(line).unwrap();
        assert_eq!(chart.drawing_text_hit_at(x, y), Some(line));
        assert_eq!(chart.drawing_text_hit_at(x, y - 12.0), Some(line));
        assert_eq!(chart.drawing_text_hit_at(x, y - 30.0), None);

        let (mut chart, note) = chart_with(DrawingKind::Note, "");
        fixed_width(&mut chart);
        chart.build_frame();
        let height = |chart: &ChartEngine| {
            let drawing = chart.drawing(note).unwrap();
            let px = chart.drawing_px(drawing).unwrap();
            chart.annotation_layout(drawing, &px, 1.0).unwrap().rect[3]
        };
        assert!(chart.drawing_apply_options(note, r#"{"text":"one","text_size":10}"#));
        let one = height(&chart);
        assert!(chart.drawing_apply_options(note, r#"{"text":"one\ntwo\nsix","text_size":10}"#));
        assert_eq!(height(&chart), one + 24.0);
        // The editor types on the box's centered block, from its first line's top.
        assert!(chart.begin_drawing_text_edit(note, false));
        let layout = chart.drawing_text_edit_layout().expect("live layout");
        assert_eq!((layout.height, layout.line_count), (36.0, 3));
        assert_eq!(layout.top_edge, layout.anchor_y - 18.0);
        assert_eq!(layout.caret_top, layout.anchor_y + 6.0);
    }

    #[test]
    fn oversized_input_is_rejected_whole_and_multibyte_caret_is_char_based() {
        let (mut chart, id) = chart_with(DrawingKind::TrendLine, "");
        assert!(chart.begin_drawing_text_edit(id, true));
        assert!(!chart.drawing_text_edit_insert(&"x".repeat(MAX_DRAWING_TEXT_BYTES + 1)));
        assert_eq!(chart.drawing_text_edit(), Some((id, "", 0)));
        assert!(chart.drawing_text_edit_insert("€€"));
        assert!(chart.drawing_text_edit_key(DrawingTextEditKey::Backspace, false));
        assert_eq!(chart.drawing_text_edit(), Some((id, "€", 1)));
    }
}
