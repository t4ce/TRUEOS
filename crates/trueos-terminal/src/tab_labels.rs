//! Edit selection metadata, kept outside terminal text and document layout.
use alloc::vec::Vec;
use crate::tab_label_cell::TabLabelCell;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TabLabel {
    pub row: usize,
    pub column: i32,
    pub columns: u32,
    pub count: u32,
    pub clip_left: usize,
    pub clip_right: usize,
}

impl TabLabel {
    pub fn cell(self, row: usize, column: usize) -> Option<TabLabelCell> {
        if row != self.row || column < self.clip_left || column >= self.clip_right { return None; }
        let offset = column as i64 - self.column as i64;
        if offset < 0 || offset >= self.columns as i64 { return None; }
        TabLabelCell::new(self.count, self.columns, offset as u32)
    }
}

pub(crate) fn update(payload: &[u8], labels: &mut Vec<TabLabel>, cols: usize, rows: usize) -> bool {
    let Some(payload) = payload.strip_prefix(b"777;edit-tabs;1;") else { return false; };
    if payload == b"clear" {
        let changed = !labels.is_empty();
        labels.clear();
        return changed;
    }
    let Ok(text) = core::str::from_utf8(payload) else { return false; };
    let mut parts = text.split(';');
    let parsed = (|| Some(TabLabel {
        row: parts.next()?.parse().ok()?, column: parts.next()?.parse().ok()?,
        columns: parts.next()?.parse().ok()?, count: parts.next()?.parse().ok()?,
        clip_left: parts.next()?.parse().ok()?, clip_right: parts.next()?.parse().ok()?,
    }))();
    let Some(label) = parsed else { return false; };
    if parts.next().is_some() || label.row >= rows
        || label.clip_left >= label.clip_right || label.clip_right > cols
        || TabLabelCell::new(label.count, label.columns, 0).is_none()
        || label.column as i64 + label.columns as i64 <= 0 || label.column as i64 >= cols as i64
        || labels.len() >= cols.saturating_mul(rows)
        || labels.iter().any(|old| old.row == label.row
            && (old.column as i64) < label.column as i64 + label.columns as i64
            && (label.column as i64) < old.column as i64 + old.columns as i64)
    { return false; }
    labels.push(label);
    true
}

#[cfg(test)]
mod tests {
    use crate::Terminal;

    #[test]
    fn selection_labels_commit_with_cells_and_clear_with_terminal_ownership() {
        let report = b"\x1b[?2026hHello\x1b]777;edit-tabs;1;clear\x07\x1b]777;edit-tabs;1;0;5;15;4;5;20\x07";
        let mut terminal = Terminal::new(80, 2);
        for byte in report { terminal.feed(core::slice::from_ref(byte)); }
        assert!(terminal.tab_labels().is_empty());
        assert_eq!(terminal.cells()[0].glyph, ' ');
        terminal.feed(b"\x1b[?2026l");
        assert_eq!(terminal.cells()[0].glyph, 'H');
        assert_eq!(terminal.cells()[5].glyph, ' ');
        assert_eq!(terminal.tab_labels().len(), 1);
        assert_eq!(terminal.tab_labels()[0].cell(0, 12).unwrap().count, 4);
        terminal.feed(b"\x1b]777;edit-tabs;1;clear\x07");
        assert!(terminal.tab_labels().is_empty());
        terminal.feed(b"\x1b]777;edit-tabs;1;0;5;4;1;5;9\x07");
        terminal.feed(b"\x1b[?1049l");
        assert!(terminal.tab_labels().is_empty());
        terminal.feed(b"\x1b]777;edit-tabs;1;0;5;4;1;5;9\x07");
        terminal.resize(60, 3);
        assert!(terminal.tab_labels().is_empty());
        terminal.feed(b"\x1b]777;edit-tabs;1;0;5;4;1;5;9\x07");
        terminal.reset();
        assert!(terminal.tab_labels().is_empty());
    }

    #[test]
    fn malformed_labels_do_not_replace_valid_metadata_and_clipping_keeps_the_center() {
        let mut terminal = Terminal::new(80, 2);
        terminal.feed(b"\x1b]777;edit-tabs;1;0;-5;15;4;0;10\x07");
        let label = terminal.tab_labels()[0];
        assert_eq!(label.cell(0, 0).unwrap().column, 5);
        assert!(label.cell(0, 10).is_none());
        for report in ["0;10;33;4;10;43", "0;10;4;5;10;14", "0;10;4;0;10;14",
            "0;10;4;1;10;81", "0;10;4;1;14;10", "0;1;4;1;1;5", "2;0;4;1;0;4",
            "0;10;4;1;10;14;extra", "0;2147483648;4;1;10;14"] {
            terminal.feed(alloc::format!("\x1b]777;edit-tabs;1;{report}\x07").as_bytes());
            assert_eq!(terminal.tab_labels(), &[label], "{report}");
        }
    }
}
