#![allow(dead_code)]

use core::fmt;

use alloc::string::String;

pub const RESET: &str = "\x1b[0m";

pub const SAVE_CURSOR: &str = "\x1b[s";
pub const RESTORE_CURSOR: &str = "\x1b[u";
pub const HIDE_CURSOR: &str = "\x1b[?25l";
pub const SHOW_CURSOR: &str = "\x1b[?25h";

pub const CURSOR_BLINKING_BLOCK: &str = "\x1b[1 q";
pub const CURSOR_STEADY_BLOCK: &str = "\x1b[2 q";
pub const CURSOR_BLINKING_UNDERLINE: &str = "\x1b[3 q";
pub const CURSOR_STEADY_UNDERLINE: &str = "\x1b[4 q";
pub const CURSOR_BLINKING_BAR: &str = "\x1b[5 q";
pub const CURSOR_STEADY_BAR: &str = "\x1b[6 q";

pub const CLEAR_SCREEN: &str = "\x1b[2J";
pub const CLEAR_LINE: &str = "\x1b[2K";
pub const CLEAR_TO_EOL: &str = "\x1b[K";
pub const CLEAR_TO_BOL: &str = "\x1b[1K";
pub const CLEAR_DOWN: &str = "\x1b[J";
pub const HOME: &str = "\x1b[H";

struct Wrapped<'a> {
    prefix: &'static str,
    text: &'a str,
    suffix: &'static str,
}

impl fmt::Display for Wrapped<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}{}", self.prefix, self.text, self.suffix)
    }
}

fn wrap<'a>(prefix: &'static str, text: &'a str) -> Wrapped<'a> {
    Wrapped {
        prefix,
        text,
        suffix: RESET,
    }
}

struct FgRgb<'a> {
    text: &'a str,
    rgb: (u8, u8, u8),
}

impl fmt::Display for FgRgb<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (r, g, b) = self.rgb;
        write!(f, "\x1b[38;2;{};{};{}m{}{}", r, g, b, self.text, RESET)
    }
}

struct BgRgb<'a> {
    text: &'a str,
    rgb: (u8, u8, u8),
}

impl fmt::Display for BgRgb<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (r, g, b) = self.rgb;
        write!(f, "\x1b[48;2;{};{};{}m{}{}", r, g, b, self.text, RESET)
    }
}

struct FgAnsi<'a> {
    text: &'a str,
    idx: u8,
}

impl fmt::Display for FgAnsi<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\x1b[38;5;{}m{}{}", self.idx, self.text, RESET)
    }
}

struct BgAnsi<'a> {
    text: &'a str,
    idx: u8,
}

impl fmt::Display for BgAnsi<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\x1b[48;5;{}m{}{}", self.idx, self.text, RESET)
    }
}

struct FgRgbChar {
    ch: char,
    rgb: (u8, u8, u8),
}

impl fmt::Display for FgRgbChar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (r, g, b) = self.rgb;
        write!(f, "\x1b[38;2;{};{};{}m{}{}", r, g, b, self.ch, RESET)
    }
}

/// Truecolor foreground (24-bit RGB).
pub fn color(text: &str, rgb: (u8, u8, u8)) -> impl fmt::Display + '_ {
    FgRgb { text, rgb }
}

/// Truecolor foreground for a single `char` (24-bit RGB).
pub fn color_char(ch: char, rgb: (u8, u8, u8)) -> impl fmt::Display {
    FgRgbChar { ch, rgb }
}

/// Truecolor background (24-bit RGB).
pub fn bg_color(text: &str, rgb: (u8, u8, u8)) -> impl fmt::Display + '_ {
    BgRgb { text, rgb }
}

/// 8/16/256-color foreground via ANSI palette index.
pub fn fg_ansi(text: &str, idx: u8) -> impl fmt::Display + '_ {
    FgAnsi { text, idx }
}

/// 8/16/256-color background via ANSI palette index.
pub fn bg_ansi(text: &str, idx: u8) -> impl fmt::Display + '_ {
    BgAnsi { text, idx }
}

pub fn bold(text: &str) -> impl fmt::Display + '_ {
    wrap("\x1b[1m", text)
}

pub fn dim(text: &str) -> impl fmt::Display + '_ {
    wrap("\x1b[2m", text)
}

pub fn underline(text: &str) -> impl fmt::Display + '_ {
    wrap("\x1b[4m", text)
}

pub fn italic(text: &str) -> impl fmt::Display + '_ {
    wrap("\x1b[3m", text)
}

pub fn blink(text: &str) -> impl fmt::Display + '_ {
    wrap("\x1b[5m", text)
}

pub fn strike(text: &str) -> impl fmt::Display + '_ {
    wrap("\x1b[9m", text)
}

pub fn invert(text: &str) -> impl fmt::Display + '_ {
    wrap("\x1b[7m", text)
}

struct Cup {
    row: usize,
    col: usize,
}

impl fmt::Display for Cup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\x1b[{};{}H", self.row.max(1), self.col.max(1))
    }
}

/// Cursor position (CUP) using 1-based row/col.
pub fn pos(row: usize, col: usize) -> impl fmt::Display {
    Cup { row, col }
}

struct Cuu {
    n: usize,
}

impl fmt::Display for Cuu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\x1b[{}A", self.n.max(1))
    }
}

/// Cursor up (CUU).
pub fn up(n: usize) -> impl fmt::Display {
    Cuu { n }
}

struct Cud {
    n: usize,
}

impl fmt::Display for Cud {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\x1b[{}B", self.n.max(1))
    }
}

/// Cursor down (CUD).
pub fn down(n: usize) -> impl fmt::Display {
    Cud { n }
}

struct Cuf {
    n: usize,
}

impl fmt::Display for Cuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\x1b[{}C", self.n.max(1))
    }
}

/// Cursor forward/right (CUF).
pub fn right(n: usize) -> impl fmt::Display {
    Cuf { n }
}

struct Cub {
    n: usize,
}

impl fmt::Display for Cub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\x1b[{}D", self.n.max(1))
    }
}

/// Cursor back/left (CUB).
pub fn left(n: usize) -> impl fmt::Display {
    Cub { n }
}

pub struct CursorGuard<'a> {
    io: &'a dyn super::ShellBackend,
}

impl Drop for CursorGuard<'_> {
    fn drop(&mut self) {
        self.io.write_str(SHOW_CURSOR);
    }
}

pub fn hide_cursor_guard(io: &dyn super::ShellBackend) -> CursorGuard<'_> {
    io.write_str(HIDE_CURSOR);
    CursorGuard { io }
}

#[derive(Copy, Clone)]
pub struct Style<'a> {
    text: &'a str,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    blink: bool,
    strike: bool,
    invert: bool,
    fg_rgb: Option<(u8, u8, u8)>,
    bg_rgb: Option<(u8, u8, u8)>,
    fg_ansi: Option<u8>,
    bg_ansi: Option<u8>,
}

impl fmt::Display for Style<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[inline]
        fn emit_code(f: &mut fmt::Formatter<'_>, first: &mut bool, code: &str) -> fmt::Result {
            if !*first {
                write!(f, ";")?;
            }
            *first = false;
            write!(f, "{}", code)
        }

        write!(f, "\x1b[")?;
        let mut first = true;

        if self.bold {
            emit_code(f, &mut first, "1")?;
        }
        if self.dim {
            emit_code(f, &mut first, "2")?;
        }
        if self.italic {
            emit_code(f, &mut first, "3")?;
        }
        if self.underline {
            emit_code(f, &mut first, "4")?;
        }
        if self.blink {
            emit_code(f, &mut first, "5")?;
        }
        if self.invert {
            emit_code(f, &mut first, "7")?;
        }
        if self.strike {
            emit_code(f, &mut first, "9")?;
        }
        if let Some((r, g, b)) = self.fg_rgb {
            emit_code(f, &mut first, "38")?;
            emit_code(f, &mut first, "2")?;
            emit_code(f, &mut first, &alloc::format!("{}", r))?;
            emit_code(f, &mut first, &alloc::format!("{}", g))?;
            emit_code(f, &mut first, &alloc::format!("{}", b))?;
        } else if let Some(c) = self.fg_ansi {
            emit_code(f, &mut first, "38")?;
            emit_code(f, &mut first, "5")?;
            emit_code(f, &mut first, &alloc::format!("{}", c))?;
        }
        if let Some((r, g, b)) = self.bg_rgb {
            emit_code(f, &mut first, "48")?;
            emit_code(f, &mut first, "2")?;
            emit_code(f, &mut first, &alloc::format!("{}", r))?;
            emit_code(f, &mut first, &alloc::format!("{}", g))?;
            emit_code(f, &mut first, &alloc::format!("{}", b))?;
        } else if let Some(c) = self.bg_ansi {
            emit_code(f, &mut first, "48")?;
            emit_code(f, &mut first, "5")?;
            emit_code(f, &mut first, &alloc::format!("{}", c))?;
        }
        if first {
            emit_code(f, &mut first, "0")?;
        }
        write!(f, "m{}{}", self.text, RESET)
    }
}

impl<'a> Style<'a> {
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
    pub fn dim(mut self) -> Self {
        self.dim = true;
        self
    }
    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }
    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }
    pub fn blink(mut self) -> Self {
        self.blink = true;
        self
    }
    pub fn strike(mut self) -> Self {
        self.strike = true;
        self
    }
    pub fn invert(mut self) -> Self {
        self.invert = true;
        self
    }
    pub fn fg(mut self, rgb: (u8, u8, u8)) -> Self {
        self.fg_rgb = Some(rgb);
        self.fg_ansi = None;
        self
    }
    pub fn bg(mut self, rgb: (u8, u8, u8)) -> Self {
        self.bg_rgb = Some(rgb);
        self.bg_ansi = None;
        self
    }
    pub fn fg8(mut self, idx: u8) -> Self {
        self.fg_ansi = Some(idx);
        self.fg_rgb = None;
        self
    }
    pub fn bg8(mut self, idx: u8) -> Self {
        self.bg_ansi = Some(idx);
        self.bg_rgb = None;
        self
    }
}

pub fn style(text: &str) -> Style<'_> {
    Style {
        text,
        bold: false,
        dim: false,
        italic: false,
        underline: false,
        blink: false,
        strike: false,
        invert: false,
        fg_rgb: None,
        bg_rgb: None,
        fg_ansi: None,
        bg_ansi: None,
    }
}

struct Sanitized<'a> {
    text: &'a str,
}

impl fmt::Display for Sanitized<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.text.as_bytes().iter().copied() {
            match b {
                b'\r' => write!(f, "\\r")?,
                b'\n' => write!(f, "\\n")?,
                b'\t' => write!(f, "\\t")?,
                0x20..=0x7E => write!(f, "{}", b as char)?,
                _ => write!(f, "\\x{:02X}", b)?,
            }
        }
        Ok(())
    }
}

pub fn sanitize(text: &str) -> impl fmt::Display + '_ {
    Sanitized { text }
}

/// Returns the visible terminal column width of `text`.
///
/// This is intended for aligning output that contains ECMA-48/ANSI escape
/// sequences. The width calculation:
/// - ignores `ESC [` (CSI) sequences until the final byte in `@..~`
/// - ignores `ESC ]` (OSC) sequences until BEL (`\x07`) or `ESC \\`
/// - treats control characters as zero-width
/// - counts all other Unicode scalar values as width 1
///
/// Note: This is a pragmatic shell UI helper, not a full terminal emulator.
pub fn visible_width(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut width = 0usize;

    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1B {
            // ESC ...
            if i + 1 >= bytes.len() {
                break;
            }
            let next = bytes[i + 1];
            match next {
                b'[' => {
                    // CSI: ESC [ ... <final>
                    i += 2;
                    while i < bytes.len() {
                        let c = bytes[i];
                        // Final byte for CSI is 0x40..=0x7E.
                        i += 1;
                        if (0x40..=0x7E).contains(&c) {
                            break;
                        }
                    }
                    continue;
                }
                b']' => {
                    // OSC: ESC ] ... (BEL | ESC \\)
                    i += 2;
                    while i < bytes.len() {
                        if bytes[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1B && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                    continue;
                }
                _ => {
                    // Other ESC sequence: skip ESC + next.
                    i += 2;
                    continue;
                }
            }
        }

        // Fast path for ASCII.
        if b < 0x80 {
            i += 1;
            if b >= 0x20 && b != 0x7F {
                width += 1;
            }
            continue;
        }

        // UTF-8 decode the next scalar and count it as width 1.
        // If invalid, consume one byte to avoid infinite loop.
        let s = core::str::from_utf8(&bytes[i..]).ok();
        if let Some(s) = s
            && let Some(ch) = s.chars().next() {
                i += ch.len_utf8();
                if !ch.is_control() {
                    width += 1;
                }
                continue;
            }
        i += 1;
    }

    width
}

/// Pads `text` with spaces on the right until it reaches `width` columns.
///
/// Uses `visible_width()` so ANSI sequences do not affect the padding.
pub fn pad_right(text: &str, width: usize) -> String {
    let mut out = String::from(text);
    let w = visible_width(text);
    if w < width {
        out.extend(core::iter::repeat_n(' ', width - w));
    }
    out
}

/// Pads `text` with spaces on the left so that it ends at `width` columns.
pub fn align_right(text: &str, width: usize) -> String {
    let mut out = String::new();
    let w = visible_width(text);
    if w < width {
        out.extend(core::iter::repeat_n(' ', width - w));
    }
    out.push_str(text);
    out
}

/// Applies lightweight ECMA-48 syntax highlighting to JSON text.
///
/// Input/output are still plain JSON characters; this only adds ANSI color
/// sequences for display in shell output.
pub fn json_upgrade(input: &str) -> String {
    const C_KEY: &str = "\x1b[38;2;255;55;255m";
    const C_STRING: &str = "\x1b[38;2;120;210;255m";
    const C_NUMBER: &str = "\x1b[38;2;255;190;110m";
    const C_BOOL: &str = "\x1b[38;2;255;230;140m";
    const C_NULL: &str = "\x1b[38;2;140;140;140m";
    const C_PUNCT: &str = "\x1b[38;2;170;170;170m";

    let mut out = String::new();
    let bytes = input.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];

        // JSON strings.
        if b == b'"' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                match bytes[i] {
                    b'\\' => {
                        i = (i + 2).min(bytes.len());
                    }
                    b'"' => {
                        i += 1;
                        break;
                    }
                    _ => i += 1,
                }
            }

            let token = &input[start..i.min(bytes.len())];
            let mut j = i;
            while j < bytes.len() && matches!(bytes[j], b' ' | b'\t' | b'\r' | b'\n') {
                j += 1;
            }
            let key = j < bytes.len() && bytes[j] == b':';

            out.push_str(if key { C_KEY } else { C_STRING });
            out.push_str(token);
            out.push_str(RESET);
            continue;
        }

        // Whitespace.
        if matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
            out.push(b as char);
            i += 1;
            continue;
        }

        // Punctuation.
        if matches!(b, b'{' | b'}' | b'[' | b']' | b':' | b',') {
            out.push_str(C_PUNCT);
            out.push(b as char);
            out.push_str(RESET);
            i += 1;
            continue;
        }

        // Keywords.
        if bytes[i..].starts_with(b"true") {
            out.push_str(C_BOOL);
            out.push_str("true");
            out.push_str(RESET);
            i += 4;
            continue;
        }
        if bytes[i..].starts_with(b"false") {
            out.push_str(C_BOOL);
            out.push_str("false");
            out.push_str(RESET);
            i += 5;
            continue;
        }
        if bytes[i..].starts_with(b"null") {
            out.push_str(C_NULL);
            out.push_str("null");
            out.push_str(RESET);
            i += 4;
            continue;
        }

        // Numbers.
        if b == b'-' || (b as char).is_ascii_digit() {
            let start = i;
            i += 1;
            while i < bytes.len() {
                let c = bytes[i];
                if (c as char).is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-') {
                    i += 1;
                } else {
                    break;
                }
            }
            out.push_str(C_NUMBER);
            out.push_str(&input[start..i]);
            out.push_str(RESET);
            continue;
        }

        // Fallback for any unexpected token.
        if b < 0x80 {
            out.push(b as char);
            i += 1;
        } else if let Some(ch) = input[i..].chars().next() {
            out.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }

    out
}

pub(crate) fn handle_ecma48(io: &dyn super::ShellBackend, rest: &str, cols: usize) {
    let arg = rest.trim();
    if arg.eq_ignore_ascii_case("help") {
        io.write_str("ecma48: usage\r\n");
        io.write_str("  ecma48 demo\r\n");
        io.write_str("  ecma48 sanitize <text>\r\n");
        io.write_str("  ecma48 clear\r\n");
        return;
    }

    if let Some(text) = arg.strip_prefix("sanitize ") {
        io.write_fmt(format_args!(
            "{}\r\n",
            align_right(&alloc::format!("sanitize: {}", sanitize(text)), cols)
        ));
        return;
    }

    if arg.eq_ignore_ascii_case("clear") {
        io.write_str(CLEAR_TO_BOL);
        io.write_str(CLEAR_TO_EOL);
        io.write_str(CLEAR_DOWN);
        io.write_str(CLEAR_LINE);
        io.write_fmt(format_args!(
            "{}\r\n",
            align_right("ecma48: clear sequences emitted", cols)
        ));
        return;
    }

    if !arg.is_empty() && !arg.eq_ignore_ascii_case("demo") {
        io.write_str("ecma48: usage ecma48 demo | ecma48 sanitize <text> | ecma48 clear\r\n");
        return;
    }

    io.write_fmt(format_args!(
        "{}\r\n",
        align_right("ecma48: demo (ANSI sequences)", cols)
    ));
    io.write_fmt(format_args!(
        "{}\r\n",
        align_right(&alloc::format!("{}", dim("dim text (SGR 2)")), cols)
    ));
    io.write_fmt(format_args!(
        "{}\r\n",
        align_right(&alloc::format!("{}", italic("italic text (SGR 3)")), cols)
    ));
    io.write_fmt(format_args!(
        "{}\r\n",
        align_right(
            &alloc::format!("{}", underline("underline text (SGR 4)")),
            cols
        )
    ));
    io.write_fmt(format_args!(
        "{}\r\n",
        align_right(&alloc::format!("{}", blink("blink text (SGR 5)")), cols)
    ));
    io.write_fmt(format_args!(
        "{}\r\n",
        align_right(&alloc::format!("{}", invert("invert text (SGR 7)")), cols)
    ));
    io.write_fmt(format_args!(
        "{}\r\n",
        align_right(&alloc::format!("{}", strike("strike text (SGR 9)")), cols)
    ));
    io.write_fmt(format_args!(
        "{}\r\n",
        align_right(
            &alloc::format!(
                "{}",
                bg_color("background RGB (48;2;0;128;255)", (0, 128, 255))
            ),
            cols
        )
    ));
    io.write_fmt(format_args!(
        "{}\r\n",
        align_right(
            &alloc::format!("{}", fg_ansi("foreground ANSI idx 196", 196)),
            cols
        )
    ));
    io.write_fmt(format_args!(
        "{}\r\n",
        align_right(
            &alloc::format!("{}", bg_ansi("background ANSI idx 24", 24)),
            cols
        )
    ));
    io.write_fmt(format_args!(
        "{}\r\n",
        align_right(
            &alloc::format!(
                "{}",
                style("composed style builder")
                    .bold()
                    .underline()
                    .fg((255, 255, 0))
                    .bg8(52)
            ),
            cols
        )
    ));
    io.write_fmt(format_args!(
        "{}\r\n",
        align_right(
            &alloc::format!(
                "{}",
                style("builder dim+italic+fg8").dim().italic().fg8(214)
            ),
            cols
        )
    ));
    io.write_fmt(format_args!(
        "{}\r\n",
        align_right(
            &alloc::format!(
                "{}",
                style("builder invert+blink+strike+bg")
                    .invert()
                    .blink()
                    .strike()
                    .bg((32, 64, 96))
            ),
            cols
        )
    ));
    io.write_fmt(format_args!(
        "{}\r\n",
        align_right(
            &alloc::format!("sanitize: {}", sanitize("\x1b[31mraw\x07")),
            cols
        )
    ));
    io.write_fmt(format_args!(
        "{}\r\n",
        align_right("cursor edits: [ABCDE]", cols)
    ));

    // Demonstrate cursor moves without disrupting where the shell continues printing.
    {
        let _cursor = hide_cursor_guard(io);
        io.write_str(SAVE_CURSOR);
        io.write_fmt(format_args!("{}", up(1)));
        io.write_fmt(format_args!("{}", right(cols.saturating_sub(20)))); // Approximately right aligned?
        io.write_fmt(format_args!("{}", bg_color("X", (255, 0, 0))));
        io.write_fmt(format_args!("{}", left(1)));
        io.write_fmt(format_args!("{}", right(1)));
        io.write_fmt(format_args!("{}", down(1)));
        io.write_str(RESTORE_CURSOR);
    }

    io.write_fmt(format_args!("{}\r\n", align_right("ecma48: done", cols)));
}
