//! Engine-owned typing session for text-capable drawings (standalone text and trend-line labels).
//!
//! The session owns the product rules every host shares: live text updates, caret position,
//! Enter/Escape semantics, and the empty lifecycle (an empty standalone text drawing is removed;
//! an empty trend label simply has no label). Hosts only forward committed text input and editing
//! keys. A browser host may keep its native editable surface for IME, clipboard, and accessibility
//! and mirror it through [`ChartEngine::set_drawing_text_edit`]; native hosts ask the engine to
//! paint the caret in the canonical frame.

use crate::drawings::{DrawingId, DrawingKind};
use crate::ChartEngine;

/// Largest label the drawing contract accepts, in UTF-8 bytes.
const MAX_DRAWING_TEXT_BYTES: usize = 256;

/// Editing keys a host forwards while a drawing text session is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawingTextEditKey {
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DrawingTextEditSession {
    pub(crate) id: DrawingId,
    original: String,
    pub(crate) text: String,
    /// Caret position in `char`s from the start of `text`.
    pub(crate) caret: usize,
    /// Native hosts have no editable surface of their own, so the frame paints the caret.
    pub(crate) paint_caret: bool,
}

/// Labels are single-line: line breaks and other control characters become spaces.
fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
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
            paint_caret,
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

    /// Insert committed text input at the caret. Input that would exceed the label limit is
    /// rejected whole so a paste never lands half-applied.
    pub fn drawing_text_edit_insert(&mut self, input: &str) -> bool {
        let Some(session) = self.drawing_text_edit.as_ref() else {
            return false;
        };
        let input = sanitize(input);
        if input.is_empty() || session.text.len() + input.len() > MAX_DRAWING_TEXT_BYTES {
            return false;
        }
        let mut text = session.text.clone();
        text.insert_str(byte_index(&text, session.caret), &input);
        let caret = session.caret + input.chars().count();
        self.apply_drawing_text_edit(text, caret)
    }

    /// Apply one editing key. Returns whether the text or caret changed.
    pub fn drawing_text_edit_key(&mut self, key: DrawingTextEditKey) -> bool {
        let Some(session) = self.drawing_text_edit.as_ref() else {
            return false;
        };
        let len = session.text.chars().count();
        let caret = session.caret.min(len);
        let mut text = session.text.clone();
        let next_caret = match key {
            DrawingTextEditKey::Backspace if caret > 0 => {
                text.remove(byte_index(&text, caret - 1));
                caret - 1
            }
            DrawingTextEditKey::Delete if caret < len => {
                text.remove(byte_index(&text, caret));
                caret
            }
            DrawingTextEditKey::Left => caret.saturating_sub(1),
            DrawingTextEditKey::Right => (caret + 1).min(len),
            DrawingTextEditKey::Home => 0,
            DrawingTextEditKey::End => len,
            DrawingTextEditKey::Backspace | DrawingTextEditKey::Delete => caret,
        };
        if text == session.text && next_caret == session.caret {
            return false;
        }
        self.apply_drawing_text_edit(text, next_caret)
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
        let Some(session) = self.drawing_text_edit.as_mut() else {
            return false;
        };
        let id = session.id;
        let text_changed = session.text != text;
        session.text = text;
        session.caret = caret;
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
        let standalone = self
            .drawing(id)
            .is_some_and(|drawing| drawing.kind == DrawingKind::Text);
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

    fn chart_with(kind: DrawingKind, text: &str) -> (ChartEngine, DrawingId) {
        let mut chart = ChartEngine::new(800.0, 400.0, 1.0);
        let times = (0..20)
            .map(|i| 1_700_000_000.0 + f64::from(i) * 60.0)
            .collect::<Vec<_>>();
        let values = vec![100.0; 20];
        chart
            .set_series_data(0, &times, &values, &values, &values, &values)
            .expect("valid bars");
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
    fn typing_edits_the_trend_label_live_at_the_caret() {
        let (mut chart, id) = chart_with(DrawingKind::TrendLine, "");
        assert!(chart.begin_drawing_text_edit(id, true));
        assert_eq!(chart.editing_drawing(), Some(id));
        assert!(chart.drawing_text_edit_insert("Brkoutx"));
        assert_eq!(text(&chart, id).as_deref(), Some("Brkoutx"));
        assert!(chart.drawing_text_edit_key(DrawingTextEditKey::Backspace));
        for _ in 0..4 {
            chart.drawing_text_edit_key(DrawingTextEditKey::Left);
        }
        assert!(chart.drawing_text_edit_insert("ea"));
        assert_eq!(chart.drawing_text_edit(), Some((id, "Breakout", 4)));
        chart.drawing_text_edit_key(DrawingTextEditKey::End);
        assert!(chart.drawing_text_edit_insert("\nnow"));
        assert!(chart.commit_drawing_text_edit());
        assert_eq!(text(&chart, id).as_deref(), Some("Breakout now"));
        assert_eq!(chart.editing_drawing(), None);
        assert!(chart.drawing_text_edit().is_none());
    }

    #[test]
    fn cancel_restores_and_empty_lifecycle_follows_the_drawing_kind() {
        let (mut chart, id) = chart_with(DrawingKind::TrendLine, "keep");
        assert!(chart.begin_drawing_text_edit(id, true));
        chart.drawing_text_edit_key(DrawingTextEditKey::Backspace);
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
    fn oversized_input_is_rejected_whole_and_multibyte_caret_is_char_based() {
        let (mut chart, id) = chart_with(DrawingKind::TrendLine, "");
        assert!(chart.begin_drawing_text_edit(id, true));
        assert!(!chart.drawing_text_edit_insert(&"x".repeat(MAX_DRAWING_TEXT_BYTES + 1)));
        assert_eq!(chart.drawing_text_edit(), Some((id, "", 0)));
        assert!(chart.drawing_text_edit_insert("€€"));
        assert!(chart.drawing_text_edit_key(DrawingTextEditKey::Backspace));
        assert_eq!(chart.drawing_text_edit(), Some((id, "€", 1)));
    }
}
