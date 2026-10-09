//! Engine-owned typing session for text-capable drawings (standalone text and trend-line labels).
//!
//! The session owns the product rules every host shares: live text updates, caret position,
//! Enter/Escape semantics, and the empty lifecycle (an empty standalone text drawing is removed;
//! an empty trend label simply has no label). Hosts only forward committed text input and editing
//! keys. A browser host may keep its native editable surface for IME, clipboard, and accessibility
//! and mirror it through [`ChartEngine::set_drawing_text_edit`]; native hosts ask the engine to
//! paint the caret in the canonical frame.

use crate::ChartEngine;
use crate::drawings::{DrawingId, DrawingKind};

/// Largest label the drawing contract accepts, in UTF-8 bytes.
const MAX_DRAWING_TEXT_BYTES: usize = 256;

/// Editing keys a host forwards while a drawing text session is open. Movement keys extend the
/// selection when the host passes `extend_selection` (Shift); word variants follow the host's
/// word modifier (Ctrl on Windows/Linux, Alt/Option on macOS).
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
}

/// The browser's transparent editing surface follows this engine-owned text run and caret.
/// Coordinates are CSS-logical media pixels in the chart overlay's space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawingTextEditLayout<'a> {
    pub anchor_x: f64,
    pub anchor_y: f64,
    pub angle: f64,
    pub font_size: f64,
    pub font_family: &'a str,
    pub font_weight: u16,
    pub font_italic: bool,
    pub left_edge: f64,
    pub advance: f64,
    pub caret_x: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DrawingTextEditSession {
    pub(crate) id: DrawingId,
    original: String,
    pub(crate) text: String,
    /// Caret position in `char`s from the start of `text`.
    pub(crate) caret: usize,
    /// Selection anchor in `char`s; the selection spans anchor..caret. Like the browser editor,
    /// the selection is not painted, but typing, deletion, copy, and cut all honor it.
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

/// Labels are single-line: line breaks and other control characters become spaces.
fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

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

    /// Resolve the open editor from the same placement and text measurements as the frame.
    pub fn drawing_text_edit_layout(&self) -> Option<DrawingTextEditLayout<'_>> {
        let session = self.drawing_text_edit.as_ref()?;
        let drawing = self.drawing(session.id)?;
        let px = self.drawing_px(drawing)?;
        let pane = self.panes.get(drawing.pane_index)?;
        let layout = &self.options.get().layout;
        let font_size = drawing.resolved_text_size(layout.font_size);
        // Box annotations type at their box's text start (the same layout the frame paints).
        let (anchor_x, anchor_y, align, angle) = match self.annotation_layout(drawing, &px, 1.0) {
            Some(annotation) => (
                annotation.text_x,
                annotation.text_y,
                crate::drawings::DrawingTextHAlign::Left,
                0.0,
            ),
            None => Self::drawing_text_placement(
                drawing,
                &px,
                self.pane_w,
                pane.top,
                pane.height,
                font_size,
                crate::drawings::TEXT_PAD,
            ),
        };
        let measure = |text: &str| {
            self.measure_text_run(
                text,
                font_size,
                &layout.font_family,
                drawing.text_weight.unwrap_or(400),
                drawing.text_italic,
            )
        };
        let advance = if session.text.is_empty() {
            font_size
        } else {
            measure(&session.text)
        };
        let start = match align {
            crate::drawings::DrawingTextHAlign::Left => 0.0,
            crate::drawings::DrawingTextHAlign::Center => -advance / 2.0,
            crate::drawings::DrawingTextHAlign::Right => -advance,
        };
        let prefix: String = session.text.chars().take(session.caret).collect();
        Some(DrawingTextEditLayout {
            anchor_x,
            anchor_y,
            angle,
            font_size,
            font_family: &layout.font_family,
            font_weight: drawing.text_weight.unwrap_or(400),
            font_italic: drawing.text_italic,
            left_edge: anchor_x + start,
            advance,
            caret_x: anchor_x + start + measure(&prefix).ceil(),
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

    /// The selected `char` range, if any.
    fn drawing_text_edit_range(session: &DrawingTextEditSession) -> Option<(usize, usize)> {
        session
            .anchor
            .filter(|&anchor| anchor != session.caret)
            .map(|anchor| (anchor.min(session.caret), anchor.max(session.caret)))
    }

    /// The selected text of the open session (for host copy/cut), if any.
    pub fn drawing_text_edit_selection(&self) -> Option<&str> {
        let session = self.drawing_text_edit.as_ref()?;
        let (start, end) = Self::drawing_text_edit_range(session)?;
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

    /// Insert committed text input at the caret, replacing any selection. Input that would exceed
    /// the label limit is rejected whole so a paste never lands half-applied.
    pub fn drawing_text_edit_insert(&mut self, input: &str) -> bool {
        let Some(session) = self.drawing_text_edit.as_ref() else {
            return false;
        };
        let input = sanitize(input);
        let (start, end) =
            Self::drawing_text_edit_range(session).unwrap_or((session.caret, session.caret));
        let mut text = session.text.clone();
        let (from, to) = (byte_index(&text, start), byte_index(&text, end));
        if input.is_empty() || text.len() - (to - from) + input.len() > MAX_DRAWING_TEXT_BYTES {
            return false;
        }
        text.replace_range(from..to, &input);
        let caret = start + input.chars().count();
        self.apply_drawing_text_edit_with_anchor(text, caret, None)
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
        let range = Self::drawing_text_edit_range(session);
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
                DrawingTextEditKey::Home => 0,
                _ => len,
            };
            let anchor = extend_selection.then(|| session.anchor.unwrap_or(caret));
            (session.text.clone(), target, anchor)
        };
        if text == session.text && next_caret == session.caret && anchor == session.anchor {
            return false;
        }
        self.apply_drawing_text_edit_with_anchor(text, next_caret, anchor)
    }

    /// Place the caret at the character boundary nearest a media-px pointer, in the label's
    /// rotated coordinates (the browser editor's click-to-place behavior for native hosts).
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
        let Some(pane) = self.panes.get(drawing.pane_index) else {
            return false;
        };
        let layout = &self.options.get().layout;
        let size = drawing.resolved_text_size(layout.font_size);
        let (tx, ty, align, angle) = Self::drawing_text_placement(
            drawing,
            &px,
            self.pane_w,
            pane.top,
            pane.height,
            size,
            crate::drawings::TEXT_PAD,
        );
        let measure = |text: &str| {
            self.measure_text_run(
                text,
                size,
                &layout.font_family,
                drawing.text_weight.unwrap_or(400),
                drawing.text_italic,
            )
        };
        let text = session.text.as_str();
        let advance = if text.is_empty() { size } else { measure(text) };
        let start = match align {
            crate::drawings::DrawingTextHAlign::Left => 0.0,
            crate::drawings::DrawingTextHAlign::Center => -advance / 2.0,
            crate::drawings::DrawingTextHAlign::Right => -advance,
        };
        let local_x = (x - tx) * angle.cos() + (y - ty) * angle.sin() - start;
        let mut best = (0, local_x.abs());
        let mut prefix = String::with_capacity(text.len());
        for (index, c) in text.chars().enumerate() {
            prefix.push(c);
            let distance = (measure(&prefix) - local_x).abs();
            if distance < best.1 {
                best = (index + 1, distance);
            }
        }
        let caret = best.0;
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
        let text = sanitize(text);
        if text.len() > MAX_DRAWING_TEXT_BYTES {
            return false;
        }
        let caret = caret.min(text.chars().count());
        self.apply_drawing_text_edit(text, caret)
    }

    /// Enter / blur: keep the typed text. An empty standalone text drawing is removed; an empty
    /// trend label leaves the line without a label.
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
        let values = vec![100.0; 20];
        chart
            .set_series_data(0, &times, &values, &values, &values, &values)
            .expect("valid bars");
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        let points = match kind {
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
        assert!(chart.drawing_text_edit_insert("\nnow"));
        assert!(chart.commit_drawing_text_edit());
        assert_eq!(text(&chart, id).as_deref(), Some("Breakout now"));
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
