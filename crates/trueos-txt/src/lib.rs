#![no_std]
//! Small page-oriented text editor core for TRUEOS.
//!
//! The crate intentionally owns only document/editor state. Rendering, keyboard
//! devices, and filesystem policy live above it.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cmp::min;

pub const DIN_A4_WIDTH_MM: u16 = 210;
pub const DIN_A4_HEIGHT_MM: u16 = 297;
pub const FORM_FEED: char = '\x0C';

pub const SUPPORTED_HIGHLIGHTERS: &[Language] = &[
    Language::Rust,
    Language::C,
    Language::Cpp,
    Language::JavaScript,
    Language::Qjs,
    Language::C4,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    PlainText,
    Rust,
    C,
    Cpp,
    JavaScript,
    Qjs,
    C4,
}

impl Language {
    pub const fn name(self) -> &'static str {
        match self {
            Self::PlainText => "text",
            Self::Rust => "rust",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::JavaScript => "javascript",
            Self::Qjs => "qjs",
            Self::C4 => "c4",
        }
    }

    pub const fn highlight_by_default(self) -> bool {
        !matches!(self, Self::PlainText)
    }
}

pub fn language_for_path(path: &str) -> Language {
    let lower = ascii_lower(path);
    let Some(ext) = lower.rsplit('.').next() else {
        return Language::PlainText;
    };
    match ext {
        "rs" => Language::Rust,
        "c" | "h" => Language::C,
        "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => Language::Cpp,
        "js" | "mjs" => Language::JavaScript,
        "qjs" => Language::Qjs,
        "c4" => Language::C4,
        _ => Language::PlainText,
    }
}

fn ascii_lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        out.push((if b.is_ascii_uppercase() { b + 32 } else { b }) as char);
    }
    out
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HighlightClass {
    Plain,
    Comment,
    String,
    Number,
    Keyword,
    Type,
    Function,
    Punctuation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HighlightSpan {
    pub start: usize,
    pub end: usize,
    pub class: HighlightClass,
}

pub fn highlight_line(language: Language, line: &str, out: &mut Vec<HighlightSpan>) {
    out.clear();
    if !language.highlight_by_default() {
        return;
    }

    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let b = bytes[i];
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            out.push(HighlightSpan {
                start,
                end: bytes.len(),
                class: HighlightClass::Comment,
            });
            break;
        }
        if b == b'"' || b == b'\'' || b == b'`' {
            let quote = b;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = min(i + 2, bytes.len());
                } else {
                    let done = bytes[i] == quote;
                    i += 1;
                    if done {
                        break;
                    }
                }
            }
            out.push(HighlightSpan {
                start,
                end: i,
                class: HighlightClass::String,
            });
            continue;
        }
        if b.is_ascii_digit() {
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric()
                    || matches!(bytes[i], b'.' | b'_' | b'x' | b'X'))
            {
                i += 1;
            }
            out.push(HighlightSpan {
                start,
                end: i,
                class: HighlightClass::Number,
            });
            continue;
        }
        if is_ident_start(b) {
            i += 1;
            while i < bytes.len() && is_ident_continue(bytes[i]) {
                i += 1;
            }
            let word = &line[start..i];
            if keyword_class(language, word).is_some() {
                out.push(HighlightSpan {
                    start,
                    end: i,
                    class: keyword_class(language, word).unwrap_or(HighlightClass::Keyword),
                });
            } else if next_non_ws(bytes, i) == Some(b'(') {
                out.push(HighlightSpan {
                    start,
                    end: i,
                    class: HighlightClass::Function,
                });
            }
            continue;
        }
        if matches!(
            b,
            b'{' | b'}'
                | b'('
                | b')'
                | b'['
                | b']'
                | b'.'
                | b','
                | b':'
                | b';'
                | b'='
                | b'+'
                | b'-'
                | b'*'
                | b'/'
                | b'<'
                | b'>'
                | b'!'
                | b'&'
                | b'|'
        ) {
            out.push(HighlightSpan {
                start,
                end: start + 1,
                class: HighlightClass::Punctuation,
            });
        }
        i += 1;
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn next_non_ws(bytes: &[u8], mut i: usize) -> Option<u8> {
    while matches!(bytes.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    bytes.get(i).copied()
}

fn keyword_class(language: Language, word: &str) -> Option<HighlightClass> {
    let is_type = match language {
        Language::Rust => matches!(
            word,
            "bool"
                | "char"
                | "str"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "usize"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "isize"
                | "f32"
                | "f64"
                | "Self"
        ),
        Language::C | Language::Cpp | Language::C4 => {
            matches!(word, "void" | "char" | "short" | "int" | "long" | "float" | "double" | "bool")
        }
        Language::JavaScript | Language::Qjs => false,
        Language::PlainText => false,
    };
    if is_type {
        return Some(HighlightClass::Type);
    }

    let is_keyword = match language {
        Language::Rust => matches!(
            word,
            "as" | "async"
                | "await"
                | "break"
                | "const"
                | "continue"
                | "crate"
                | "else"
                | "enum"
                | "extern"
                | "false"
                | "fn"
                | "for"
                | "if"
                | "impl"
                | "in"
                | "let"
                | "loop"
                | "match"
                | "mod"
                | "move"
                | "mut"
                | "pub"
                | "ref"
                | "return"
                | "self"
                | "static"
                | "struct"
                | "trait"
                | "true"
                | "type"
                | "unsafe"
                | "use"
                | "where"
                | "while"
        ),
        Language::C | Language::Cpp | Language::C4 => matches!(
            word,
            "break"
                | "case"
                | "const"
                | "continue"
                | "do"
                | "else"
                | "enum"
                | "false"
                | "for"
                | "if"
                | "return"
                | "sizeof"
                | "static"
                | "struct"
                | "switch"
                | "true"
                | "typedef"
                | "while"
        ),
        Language::JavaScript | Language::Qjs => matches!(
            word,
            "async"
                | "await"
                | "break"
                | "const"
                | "continue"
                | "else"
                | "false"
                | "for"
                | "function"
                | "if"
                | "let"
                | "null"
                | "return"
                | "true"
                | "undefined"
                | "var"
                | "while"
        ),
        Language::PlainText => false,
    };
    is_keyword.then_some(HighlightClass::Keyword)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderFlow {
    Wrap,
    Crop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorMode {
    Insert,
    Delete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageMetrics {
    pub width_mm: u16,
    pub height_mm: u16,
    pub cols: usize,
    pub rows: usize,
}

impl PageMetrics {
    pub const fn din_a4(cols: usize, rows: usize) -> Self {
        Self {
            width_mm: DIN_A4_WIDTH_MM,
            height_mm: DIN_A4_HEIGHT_MM,
            cols,
            rows,
        }
    }
}

impl Default for PageMetrics {
    fn default() -> Self {
        Self::din_a4(80, 56)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cursor {
    pub page: usize,
    pub row: usize,
    pub col: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderLine {
    pub text: String,
    pub source_row: usize,
    pub continuation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderPage {
    pub page: usize,
    pub lines: Vec<RenderLine>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageChange {
    pub old_page: usize,
    pub new_page: usize,
    pub page_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExitRequest {
    Replace,
    Forget,
    Rename { new_name: String },
    Move { new_path: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditorStatus {
    Open,
    Closed(ExitRequest),
}

pub trait TxtCallbacks {
    fn set_page(&mut self, _change: PageChange) {}
    fn exit_replace(&mut self, _text: &str) {}
    fn exit_forget(&mut self) {}
    fn exit_rename(&mut self, _new_name: &str, _text: &str) {}
    fn exit_move(&mut self, _new_path: &str, _text: &str) {}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TxtDocument {
    text: String,
}

impl TxtDocument {
    pub fn new(text: String) -> Self {
        Self { text }
    }

    pub fn empty() -> Self {
        Self {
            text: String::new(),
        }
    }

    pub fn as_str(&self) -> &str {
        self.text.as_str()
    }

    pub fn into_string(self) -> String {
        self.text
    }

    pub fn page_count(&self) -> usize {
        self.text.matches(FORM_FEED).count() + 1
    }

    pub fn ensure_page(&mut self, page: usize) {
        while self.page_count() <= page {
            self.text.push(FORM_FEED);
        }
    }

    pub fn page_text(&self, page: usize) -> &str {
        self.text.split(FORM_FEED).nth(page).unwrap_or("")
    }

    pub fn replace_page(&mut self, page: usize, page_text: &str) {
        self.ensure_page(page);
        let mut pages = self.pages();
        pages[page] = page_text.to_string();
        self.text = join_pages(&pages);
    }

    pub fn move_page(&mut self, from: usize, to: usize) -> bool {
        self.ensure_page(from.max(to));
        if from == to {
            return true;
        }
        let mut pages = self.pages();
        if from >= pages.len() || to >= pages.len() {
            return false;
        }
        let page = pages.remove(from);
        pages.insert(to, page);
        self.text = join_pages(&pages);
        true
    }

    fn pages(&self) -> Vec<String> {
        self.text
            .split(FORM_FEED)
            .map(ToString::to_string)
            .collect()
    }
}

fn join_pages(pages: &[String]) -> String {
    let mut out = String::new();
    for (idx, page) in pages.iter().enumerate() {
        if idx != 0 {
            out.push(FORM_FEED);
        }
        out.push_str(page);
    }
    out
}

#[derive(Clone, Debug)]
pub struct TxtEditor {
    path: String,
    document: TxtDocument,
    metrics: PageMetrics,
    cursor: Cursor,
    render_flow: RenderFlow,
    cursor_mode: CursorMode,
    language: Language,
    highlight_enabled: bool,
    status: EditorStatus,
}

impl TxtEditor {
    pub fn new(path: &str, text: String, metrics: PageMetrics) -> Self {
        let language = language_for_path(path);
        Self {
            path: path.to_string(),
            document: TxtDocument::new(text),
            metrics,
            cursor: Cursor::default(),
            render_flow: RenderFlow::Wrap,
            cursor_mode: CursorMode::Insert,
            language,
            highlight_enabled: language.highlight_by_default(),
            status: EditorStatus::Open,
        }
    }

    pub fn path(&self) -> &str {
        self.path.as_str()
    }

    pub fn status(&self) -> &EditorStatus {
        &self.status
    }

    pub fn is_open(&self) -> bool {
        matches!(self.status, EditorStatus::Open)
    }

    pub fn document(&self) -> &TxtDocument {
        &self.document
    }

    pub fn document_mut(&mut self) -> &mut TxtDocument {
        &mut self.document
    }

    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    pub fn set_cursor(&mut self, cursor: Cursor) {
        self.document.ensure_page(cursor.page);
        self.cursor = self.clamp_cursor(cursor);
    }

    pub fn render_flow(&self) -> RenderFlow {
        self.render_flow
    }

    pub fn set_render_flow(&mut self, flow: RenderFlow) {
        self.render_flow = flow;
    }

    pub fn toggle_render_flow(&mut self) {
        self.render_flow = match self.render_flow {
            RenderFlow::Wrap => RenderFlow::Crop,
            RenderFlow::Crop => RenderFlow::Wrap,
        };
    }

    pub fn cursor_mode(&self) -> CursorMode {
        self.cursor_mode
    }

    pub fn set_cursor_mode(&mut self, mode: CursorMode) {
        self.cursor_mode = mode;
    }

    pub fn toggle_cursor_mode(&mut self) {
        self.cursor_mode = match self.cursor_mode {
            CursorMode::Insert => CursorMode::Delete,
            CursorMode::Delete => CursorMode::Insert,
        };
    }

    pub fn language(&self) -> Language {
        self.language
    }

    pub fn set_language(&mut self, language: Language) {
        self.language = language;
        self.highlight_enabled = language.highlight_by_default();
    }

    pub fn highlight_enabled(&self) -> bool {
        self.highlight_enabled
    }

    pub fn set_highlight_enabled(&mut self, enabled: bool) {
        self.highlight_enabled = enabled;
    }

    pub fn page_count(&self) -> usize {
        self.document.page_count()
    }

    pub fn set_page<C: TxtCallbacks>(&mut self, page: usize, callbacks: &mut C) {
        let old_page = self.cursor.page;
        self.document.ensure_page(page);
        self.cursor.page = page;
        self.cursor.row = min(self.cursor.row, self.metrics.rows.saturating_sub(1));
        self.cursor.col = min(self.cursor.col, self.metrics.cols.saturating_sub(1));
        callbacks.set_page(PageChange {
            old_page,
            new_page: page,
            page_count: self.page_count(),
        });
    }

    pub fn page_forward<C: TxtCallbacks>(&mut self, callbacks: &mut C) {
        self.set_page(self.cursor.page + 1, callbacks);
    }

    pub fn page_backward<C: TxtCallbacks>(&mut self, callbacks: &mut C) {
        self.set_page(self.cursor.page.saturating_sub(1), callbacks);
    }

    pub fn move_current_page<C: TxtCallbacks>(&mut self, to: usize, callbacks: &mut C) -> bool {
        let from = self.cursor.page;
        let moved = self.document.move_page(from, to);
        if moved {
            self.cursor.page = to;
            callbacks.set_page(PageChange {
                old_page: from,
                new_page: to,
                page_count: self.page_count(),
            });
        }
        moved
    }

    pub fn arrow_left(&mut self) {
        self.cursor.col = self.cursor.col.saturating_sub(1);
    }

    pub fn arrow_right(&mut self) {
        self.cursor.col =
            min(self.cursor.col.saturating_add(1), self.metrics.cols.saturating_sub(1));
    }

    pub fn arrow_up(&mut self) {
        self.cursor.row = self.cursor.row.saturating_sub(1);
    }

    pub fn arrow_down(&mut self) {
        self.cursor.row =
            min(self.cursor.row.saturating_add(1), self.metrics.rows.saturating_sub(1));
    }

    pub fn put_char(&mut self, ch: char) {
        self.document.ensure_page(self.cursor.page);
        let mut page = self.document.page_text(self.cursor.page).to_string();
        let idx = materialize_cursor(&mut page, self.cursor.row, self.cursor.col);
        match self.cursor_mode {
            CursorMode::Insert => page.insert(idx, ch),
            CursorMode::Delete => {
                if idx < page.len() && page.is_char_boundary(idx) {
                    let end = page[idx..]
                        .chars()
                        .next()
                        .map(|c| idx + c.len_utf8())
                        .unwrap_or(idx);
                    page.replace_range(idx..end, ch.encode_utf8(&mut [0; 4]));
                } else {
                    page.push(ch);
                }
            }
        }
        self.document.replace_page(self.cursor.page, page.as_str());
        self.arrow_right();
    }

    pub fn backspace(&mut self) {
        if self.cursor.col == 0 && self.cursor.row == 0 {
            return;
        }
        if self.cursor.col == 0 {
            self.cursor.row = self.cursor.row.saturating_sub(1);
            self.cursor.col = self.metrics.cols.saturating_sub(1);
        } else {
            self.cursor.col -= 1;
        }
        self.delete_at_cursor();
    }

    pub fn delete_at_cursor(&mut self) {
        let mut page = self.document.page_text(self.cursor.page).to_string();
        let idx = cursor_byte_index(page.as_str(), self.cursor.row, self.cursor.col);
        if idx < page.len() && page.is_char_boundary(idx) {
            let end = page[idx..]
                .chars()
                .next()
                .map(|c| idx + c.len_utf8())
                .unwrap_or(idx);
            page.replace_range(idx..end, "");
            self.document.replace_page(self.cursor.page, page.as_str());
        }
    }

    pub fn render_page(&self, page: usize) -> RenderPage {
        let mut lines = Vec::new();
        for (source_row, line) in self.document.page_text(page).lines().enumerate() {
            match self.render_flow {
                RenderFlow::Crop => lines.push(RenderLine {
                    text: take_chars(line, self.metrics.cols),
                    source_row,
                    continuation: false,
                }),
                RenderFlow::Wrap => push_wrapped(line, source_row, self.metrics.cols, &mut lines),
            }
            if lines.len() >= self.metrics.rows {
                break;
            }
        }
        while lines.len() < self.metrics.rows {
            let source_row = lines.len();
            lines.push(RenderLine {
                text: String::new(),
                source_row,
                continuation: false,
            });
        }
        RenderPage { page, lines }
    }

    pub fn highlight_render_line(&self, line: &str, out: &mut Vec<HighlightSpan>) {
        if self.highlight_enabled {
            highlight_line(self.language, line, out);
        } else {
            out.clear();
        }
    }

    pub fn exit<C: TxtCallbacks>(&mut self, request: ExitRequest, callbacks: &mut C) {
        match &request {
            ExitRequest::Replace => callbacks.exit_replace(self.document.as_str()),
            ExitRequest::Forget => callbacks.exit_forget(),
            ExitRequest::Rename { new_name } => {
                self.path = new_name.clone();
                self.set_language(language_for_path(new_name.as_str()));
                callbacks.exit_rename(new_name.as_str(), self.document.as_str())
            }
            ExitRequest::Move { new_path } => {
                self.path = new_path.clone();
                self.set_language(language_for_path(new_path.as_str()));
                callbacks.exit_move(new_path.as_str(), self.document.as_str())
            }
        }
        self.status = EditorStatus::Closed(request);
    }

    fn clamp_cursor(&self, cursor: Cursor) -> Cursor {
        Cursor {
            page: cursor.page,
            row: min(cursor.row, self.metrics.rows.saturating_sub(1)),
            col: min(cursor.col, self.metrics.cols.saturating_sub(1)),
        }
    }
}

fn push_wrapped(line: &str, source_row: usize, width: usize, out: &mut Vec<RenderLine>) {
    let width = width.max(1);
    let mut chunk = String::new();
    let mut continuation = false;
    for ch in line.chars() {
        if chunk.chars().count() == width {
            out.push(RenderLine {
                text: chunk,
                source_row,
                continuation,
            });
            chunk = String::new();
            continuation = true;
        }
        chunk.push(ch);
    }
    out.push(RenderLine {
        text: chunk,
        source_row,
        continuation,
    });
}

fn take_chars(s: &str, count: usize) -> String {
    s.chars().take(count).collect()
}

fn cursor_byte_index(page: &str, row: usize, col: usize) -> usize {
    let mut byte = 0;
    for _ in 0..row {
        if let Some(pos) = page[byte..].find('\n') {
            byte += pos + 1;
        } else {
            return page.len();
        }
    }
    let line_end = page[byte..]
        .find('\n')
        .map(|pos| byte + pos)
        .unwrap_or(page.len());
    page[byte..line_end]
        .char_indices()
        .nth(col)
        .map(|(offset, _)| byte + offset)
        .unwrap_or(line_end)
}

fn materialize_cursor(page: &mut String, row: usize, col: usize) -> usize {
    let mut lines: Vec<String> = page.lines().map(ToString::to_string).collect();
    if page.ends_with('\n') {
        lines.push(String::new());
    }
    while lines.len() <= row {
        lines.push(String::new());
    }
    let line = &mut lines[row];
    while line.chars().count() < col {
        line.push(' ');
    }
    *page = lines.join("\n");
    cursor_byte_index(page.as_str(), row, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct CallLog {
        page: Option<PageChange>,
        exit: Option<&'static str>,
    }

    impl TxtCallbacks for CallLog {
        fn set_page(&mut self, change: PageChange) {
            self.page = Some(change);
        }

        fn exit_replace(&mut self, _text: &str) {
            self.exit = Some("replace");
        }
    }

    #[test]
    fn creates_pages_when_moving_forward() {
        let mut editor =
            TxtEditor::new("note.rs", String::from("fn main() {}"), PageMetrics::default());
        let mut calls = CallLog::default();
        editor.set_page(2, &mut calls);
        assert_eq!(editor.page_count(), 3);
        assert_eq!(calls.page.unwrap().new_page, 2);
    }

    #[test]
    fn renders_crop_and_wrap() {
        let mut editor = TxtEditor::new("x.txt", String::from("abcdef"), PageMetrics::din_a4(3, 4));
        editor.set_render_flow(RenderFlow::Crop);
        assert_eq!(editor.render_page(0).lines[0].text, "abc");
        editor.set_render_flow(RenderFlow::Wrap);
        assert_eq!(editor.render_page(0).lines[1].text, "def");
    }

    #[test]
    fn defaults_highlight_by_extension() {
        let editor = TxtEditor::new("boot.qjs", String::new(), PageMetrics::default());
        assert_eq!(editor.language(), Language::Qjs);
        assert!(editor.highlight_enabled());
    }

    #[test]
    fn highlights_keywords() {
        let mut spans = Vec::new();
        highlight_line(Language::Rust, "pub fn main() { 42 }", &mut spans);
        assert!(
            spans
                .iter()
                .any(|span| span.class == HighlightClass::Keyword)
        );
        assert!(
            spans
                .iter()
                .any(|span| span.class == HighlightClass::Number)
        );
    }

    #[test]
    fn exit_replace_callback() {
        let editor = TxtEditor::new("x.txt", String::from("hello"), PageMetrics::default());
        let mut calls = CallLog::default();
        editor.exit(ExitRequest::Replace, &mut calls);
        assert_eq!(calls.exit, Some("replace"));
    }
}
