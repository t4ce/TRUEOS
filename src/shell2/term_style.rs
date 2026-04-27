use core::fmt;

use super::ecma48;

#[derive(Copy, Clone)]
pub(crate) enum Color {
    Ansi(u8),
    TrueColor { r: u8, g: u8, b: u8 },
}

impl From<u8> for Color {
    fn from(value: u8) -> Self {
        Self::Ansi(value)
    }
}

impl From<(u8, u8, u8)> for Color {
    fn from((r, g, b): (u8, u8, u8)) -> Self {
        Self::TrueColor { r, g, b }
    }
}

#[derive(Copy, Clone)]
pub(crate) struct Paint<'a> {
    text: &'a str,
    bold: bool,
    dim: bool,
    underline: bool,
    fg: Option<Color>,
}

impl fmt::Display for Paint<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[inline]
        fn sep(f: &mut fmt::Formatter<'_>, first: &mut bool) -> fmt::Result {
            if !*first {
                write!(f, ";")?;
            }
            *first = false;
            Ok(())
        }

        #[inline]
        fn emit_color(
            f: &mut fmt::Formatter<'_>,
            first: &mut bool,
            prefix: u8,
            color: Color,
        ) -> fmt::Result {
            sep(f, first)?;
            match color {
                Color::Ansi(idx) => write!(f, "{};5;{}", prefix, idx),
                Color::TrueColor { r, g, b } => write!(f, "{};2;{};{};{}", prefix, r, g, b),
            }
        }

        write!(f, "\x1b[")?;
        let mut first = true;

        if self.bold {
            sep(f, &mut first)?;
            write!(f, "1")?;
        }
        if self.dim {
            sep(f, &mut first)?;
            write!(f, "2")?;
        }
        if self.underline {
            sep(f, &mut first)?;
            write!(f, "4")?;
        }
        if let Some(color) = self.fg {
            emit_color(f, &mut first, 38, color)?;
        }
        if first {
            write!(f, "0")?;
        }

        write!(f, "m{}{}", self.text, ecma48::RESET)
    }
}

impl<'a> Paint<'a> {
    pub(crate) fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub(crate) fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    pub(crate) fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    pub(crate) fn color(mut self, color: impl Into<Color>) -> Self {
        self.fg = Some(color.into());
        self
    }
}

pub(crate) fn paint(text: &str) -> Paint<'_> {
    Paint {
        text,
        bold: false,
        dim: false,
        underline: false,
        fg: None,
    }
}
