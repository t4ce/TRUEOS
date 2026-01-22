use core::fmt;

pub const RESET: &str = "\x1b[0m";

pub const SAVE_CURSOR: &str = "\x1b[s";
pub const RESTORE_CURSOR: &str = "\x1b[u";
pub const HIDE_CURSOR: &str = "\x1b[?25l";
pub const SHOW_CURSOR: &str = "\x1b[?25h";
pub const CLEAR_SCREEN: &str = "\x1b[2J";
pub const CLEAR_LINE: &str = "\x1b[2K";
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

/// Truecolor foreground (24-bit RGB).
pub fn color(text: &str, rgb: (u8, u8, u8)) -> impl fmt::Display + '_ {
    FgRgb { text, rgb }
}

/// Truecolor background (24-bit RGB).
pub fn bg_color(text: &str, rgb: (u8, u8, u8)) -> impl fmt::Display + '_ {
    BgRgb { text, rgb }
}

pub fn bold(text: &str) -> impl fmt::Display + '_ {
    Wrapped {
        prefix: "\x1b[1m",
        text,
        suffix: RESET,
    }
}

pub fn dim(text: &str) -> impl fmt::Display + '_ {
    Wrapped {
        prefix: "\x1b[2m",
        text,
        suffix: RESET,
    }
}

pub fn underline(text: &str) -> impl fmt::Display + '_ {
    Wrapped {
        prefix: "\x1b[4m",
        text,
        suffix: RESET,
    }
}

pub fn invert(text: &str) -> impl fmt::Display + '_ {
    Wrapped {
        prefix: "\x1b[7m",
        text,
        suffix: RESET,
    }
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
