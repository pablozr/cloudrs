//! Single-line text model: UTF-8 byte ranges at the GPUI boundary, UTF-16 at
//! the OS (IME) boundary.
//!
//! Adapted from xemnas `ui/search_edit.rs` (MIT, same maintainer); see `NOTICE`.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

/// Editable value with a selection and an optional IME composition.
#[derive(Debug, Default)]
pub struct SearchEdit {
    pub text: String,
    /// Selected UTF-8 byte range.
    pub selection: Range<usize>,
    /// True when the caret is at the start of the selection.
    pub reversed: bool,
    /// Active IME composition, in UTF-8 bytes.
    pub marked: Option<Range<usize>>,
}

impl SearchEdit {
    /// Replaces the composition, the given range or the selection. Line breaks become spaces.
    pub fn replace(&mut self, range: Option<Range<usize>>, text: &str) {
        let range = range
            .or_else(|| self.marked.clone())
            .unwrap_or(self.selection.clone());
        let text = single_line(text);
        self.text.replace_range(range.clone(), &text);
        let offset = range.start + text.len();
        self.selection = offset..offset;
        self.reversed = false;
        self.marked = None;
    }

    /// UTF-16 code-unit offset (from the OS) to a UTF-8 byte offset.
    pub fn from_utf16(&self, offset: usize) -> usize {
        utf16_to_byte(&self.text, offset)
    }

    /// UTF-8 byte offset to UTF-16 code units.
    pub fn to_utf16(&self, offset: usize) -> usize {
        self.text[..offset].encode_utf16().count()
    }

    /// Deletes one user-perceived character before the caret, or the selection.
    pub fn backspace(&mut self) {
        if self.selection.is_empty() {
            let caret = self.caret();
            self.selection = self.previous_boundary(caret)..caret;
        }
        self.replace(None, "");
    }

    /// Deletes one user-perceived character after the caret, or the selection.
    pub fn delete(&mut self) {
        if self.selection.is_empty() {
            let caret = self.caret();
            self.selection = caret..self.next_boundary(caret);
        }
        self.replace(None, "");
    }

    /// The moving end of the selection.
    pub fn caret(&self) -> usize {
        if self.reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }

    /// Collapses the selection at `offset`.
    pub fn move_to(&mut self, offset: usize) {
        let offset = self.valid_offset(offset);
        self.selection = offset..offset;
        self.reversed = false;
    }

    /// Extends the selection to `offset`.
    pub fn select_to(&mut self, offset: usize) {
        let offset = self.valid_offset(offset);
        let anchor = if self.reversed {
            self.selection.end
        } else {
            self.selection.start
        };
        self.selection = anchor.min(offset)..anchor.max(offset);
        self.reversed = offset < anchor;
    }

    /// The closest grapheme boundary strictly before `offset`.
    pub fn previous_boundary(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    /// The closest grapheme boundary strictly after `offset`.
    pub fn next_boundary(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.text.len())
    }

    /// Applies an IME composition; `selected` is relative to the new text, in UTF-16.
    pub fn replace_and_mark(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
    ) {
        let range = range
            .or_else(|| self.marked.clone())
            .unwrap_or(self.selection.clone());
        let text = single_line(text);
        self.text.replace_range(range.clone(), &text);
        self.marked = (!text.is_empty()).then(|| range.start..range.start + text.len());
        self.selection = selected
            .map(|selection| {
                range.start + utf16_to_byte(&text, selection.start)
                    ..range.start + utf16_to_byte(&text, selection.end)
            })
            .unwrap_or_else(|| range.start + text.len()..range.start + text.len());
        self.reversed = false;
    }

    fn valid_offset(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }
}

fn single_line(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut count = 0;
    for (index, ch) in text.char_indices() {
        if count >= offset || count + ch.len_utf16() > offset {
            return index;
        }
        count += ch.len_utf16();
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::SearchEdit;

    #[test]
    fn the_caret_never_leaves_the_value_or_splits_a_character() {
        let mut edit = SearchEdit::default();
        edit.move_to(12);
        edit.replace(None, "techno");
        assert_eq!(edit.text, "techno");
        edit.move_to(100);
        edit.select_to(999);
        edit.replace(None, "!");
        assert_eq!(edit.text, "techno!");
        edit.text = "é".into();
        edit.move_to(1);
        assert_eq!(edit.caret(), 0);
    }

    #[test]
    fn replaces_the_selection() {
        let mut edit = SearchEdit {
            text: "deep house".into(),
            selection: 0..4,
            ..Default::default()
        };
        edit.replace(None, "tech");
        assert_eq!(edit.text, "tech house");
        assert_eq!(edit.selection, 4..4);
    }

    #[test]
    fn utf16_offsets_survive_emoji() {
        let edit = SearchEdit {
            text: "a🪷b".into(),
            ..Default::default()
        };
        assert_eq!(edit.from_utf16(3), 5);
        assert_eq!(edit.to_utf16(5), 3);
    }

    #[test]
    fn backspace_removes_a_whole_grapheme() {
        let mut edit = SearchEdit {
            text: "e\u{301}x".into(),
            selection: 3..3,
            ..Default::default()
        };
        edit.backspace();
        assert_eq!(edit.text, "x");
    }

    #[test]
    fn pasted_lines_become_one() {
        let mut edit = SearchEdit::default();
        edit.replace(None, "foo\r\nbar\nbaz");
        assert_eq!(edit.text, "foo bar baz");
    }

    #[test]
    fn composition_replaces_the_marked_range() {
        let mut edit = SearchEdit::default();
        edit.replace_and_mark(None, "🪷a", Some(2..2));
        assert_eq!(edit.marked, Some(0..5));
        assert_eq!(edit.selection, 4..4);
        edit.replace(None, "z");
        assert_eq!(edit.text, "z");
        assert_eq!(edit.marked, None);
    }
}
