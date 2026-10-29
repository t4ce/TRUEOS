//! Bounded UTF-8 input for the kernel Codex prompt. Visual wrapping never
//! inserts newlines into the user message.

use alloc::{string::String, vec::Vec};

pub(super) const COLUMNS: usize = 19;
pub(super) const ROWS: usize = 9;
pub(super) const MAX_BYTES: usize = 16 * 1024;

#[derive(Default)]
pub(super) struct Editor {
    text: String,
    cursor: usize,
    top: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) struct VisibleCell {
    pub column: usize,
    pub row: usize,
    pub character: char,
}

impl Editor {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn insert(&mut self, ch: char) -> bool {
        if (ch.is_control() && ch != '\n' && ch != '\t')
            || self.text.len() + ch.len_utf8() > MAX_BYTES
        {
            return false;
        }
        self.text.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.reveal_cursor();
        true
    }

    pub fn backspace(&mut self) {
        if let Some((previous, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.text.drain(previous..self.cursor);
            self.cursor = previous;
        }
        self.reveal_cursor();
    }

    pub fn delete(&mut self) {
        if let Some(ch) = self.text[self.cursor..].chars().next() {
            self.text.drain(self.cursor..self.cursor + ch.len_utf8());
        }
        self.reveal_cursor();
    }

    pub fn left(&mut self) {
        if let Some((previous, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.cursor = previous;
        }
        self.reveal_cursor();
    }

    pub fn right(&mut self) {
        if let Some(ch) = self.text[self.cursor..].chars().next() {
            self.cursor += ch.len_utf8();
        }
        self.reveal_cursor();
    }

    pub fn vertical(&mut self, delta: isize) {
        let (column, row) = self.position();
        self.go_to(column, row.saturating_add_signed(delta));
    }

    pub fn home(&mut self) {
        self.go_to(0, self.position().1);
    }

    pub fn end(&mut self) {
        self.go_to(COLUMNS - 1, self.position().1);
    }

    pub fn click(&mut self, column: usize, row: usize) {
        self.go_to(column.min(COLUMNS - 1), self.top + row.min(ROWS - 1));
    }

    pub fn caret(&self) -> (usize, usize) {
        let (column, row) = self.position();
        (column, row - self.top)
    }

    pub fn visible_cells(&self) -> Vec<VisibleCell> {
        let mut cells = Vec::new();
        let mut column = 0;
        let mut row = 0;
        for character in self.text.chars() {
            if (self.top..self.top + ROWS).contains(&row) && character != '\n' && character != '\t'
            {
                cells.push(VisibleCell {
                    column,
                    row: row - self.top,
                    character,
                });
            }
            advance(character, &mut column, &mut row);
            if row >= self.top + ROWS {
                break;
            }
        }
        cells
    }

    fn position(&self) -> (usize, usize) {
        let mut column = 0;
        let mut row = 0;
        for character in self.text[..self.cursor].chars() {
            advance(character, &mut column, &mut row);
        }
        (column, row)
    }

    fn reveal_cursor(&mut self) {
        let row = self.position().1;
        if row < self.top {
            self.top = row;
        } else if row >= self.top + ROWS {
            self.top = row + 1 - ROWS;
        }
    }

    fn go_to(&mut self, target_column: usize, target_row: usize) {
        let mut column = 0;
        let mut row = 0;
        let mut candidate = self.text.len();
        for (index, character) in self.text.char_indices() {
            if row > target_row || (row == target_row && column > target_column) {
                break;
            }
            candidate = index;
            advance(character, &mut column, &mut row);
            if row < target_row || (row == target_row && column <= target_column) {
                candidate = index + character.len_utf8();
            }
        }
        self.cursor = candidate;
        self.reveal_cursor();
    }
}

fn advance(character: char, column: &mut usize, row: &mut usize) {
    if character == '\n' {
        *column = 0;
        *row += 1;
    } else {
        *column += 1;
        if *column == COLUMNS {
            *column = 0;
            *row += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_text(editor: &mut Editor, text: &str) {
        for character in text.chars() {
            assert!(editor.insert(character));
        }
    }

    #[test]
    fn user_text_keeps_whitespace_and_unicode_when_edited() {
        let mut editor = Editor::default();
        type_text(&mut editor, "  hello §莉\nworld  ");
        editor.left();
        editor.left();
        editor.backspace();
        assert!(editor.insert('d'));
        assert_eq!(editor.text(), "  hello §莉\nworld  ");
        editor.click(9, 0);
        editor.delete();
        assert_eq!(editor.text(), "  hello §\nworld  ");
        editor.backspace();
        assert_eq!(editor.text(), "  hello \nworld  ");
    }

    #[test]
    fn wrapping_and_scroll_never_change_submitted_message() {
        let mut editor = Editor::default();
        let message = "x".repeat(COLUMNS * (ROWS + 3));
        type_text(&mut editor, &message);
        assert_eq!(editor.text(), message);
        assert_eq!(editor.caret(), (0, ROWS - 1));
        assert!(editor.visible_cells().len() <= COLUMNS * ROWS);
        for _ in 0..ROWS + 3 {
            editor.vertical(-1);
        }
        assert_eq!(editor.caret(), (0, 0));
        assert_eq!(editor.text(), message);
    }

    #[test]
    fn pointer_and_arrow_positions_follow_wrapping_and_explicit_lines() {
        let mut editor = Editor::default();
        type_text(&mut editor, "abcdefghijklmnopqrsXY\nshort");
        editor.click(1, 1);
        assert_eq!(editor.caret(), (1, 1));
        editor.vertical(-1);
        assert_eq!(editor.caret(), (1, 0));
        editor.end();
        assert_eq!(editor.caret(), (18, 0));
        editor.right();
        assert_eq!(editor.caret(), (0, 1));
        editor.vertical(1);
        assert_eq!(editor.caret(), (0, 2));
        editor.end();
        assert_eq!(editor.caret(), (5, 2));
        editor.home();
        assert_eq!(editor.caret(), (0, 2));
    }

    #[test]
    fn utf8_budget_refuses_whole_scalar_without_truncating_message() {
        let mut editor = Editor::default();
        type_text(&mut editor, &"x".repeat(MAX_BYTES - 2));
        assert!(!editor.insert('莉'));
        assert!(editor.insert('§'));
        assert_eq!(editor.text().len(), MAX_BYTES);
        assert!(!editor.insert('x'));
        editor.backspace();
        assert_eq!(editor.text().len(), MAX_BYTES - 2);
    }

    #[test]
    fn control_characters_cannot_escape_into_terminal_output() {
        let mut editor = Editor::default();
        assert!(!editor.insert('\u{1b}'));
        assert!(!editor.insert('\u{8}'));
        assert!(editor.insert('\t'));
        assert!(editor.insert('\n'));
        assert_eq!(editor.text(), "\t\n");
    }
}
