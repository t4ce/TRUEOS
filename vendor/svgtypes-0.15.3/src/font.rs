// Copyright 2024 the SVG Types Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::stream::{ByteExt, Stream};
use crate::Error;
use core::fmt::Display;

/// Parses a list of font families and generic families from a string.
pub fn parse_font_families(text: &str) -> Result<Vec<FontFamily>, Error> {
    let mut s = Stream::from(text);
    let font_families = s.parse_font_families()?;

    s.skip_spaces();
    if !s.at_end() {
        return Err(Error::UnexpectedData(s.calc_char_pos()));
    }

    Ok(font_families)
}

/// A type of font family.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub enum FontFamily {
    /// A serif font.
    Serif,
    /// A sans-serif font.
    SansSerif,
    /// A cursive font.
    Cursive,
    /// A fantasy font.
    Fantasy,
    /// A monospace font.
    Monospace,
    /// A custom named font.
    Named(String),
}

impl Display for FontFamily {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let str = match self {
            FontFamily::Monospace => "monospace".to_string(),
            FontFamily::Serif => "serif".to_string(),
            FontFamily::SansSerif => "sans-serif".to_string(),
            FontFamily::Cursive => "cursive".to_string(),
            FontFamily::Fantasy => "fantasy".to_string(),
            FontFamily::Named(s) => format!("\"{}\"", s),
        };
        write!(f, "{}", str)
    }
}

impl Stream<'_> {
    pub fn parse_font_families(&mut self) -> Result<Vec<FontFamily>, Error> {
        let mut families = vec![];

        while !self.at_end() {
            self.skip_spaces();

            let family = {
                let ch = self.curr_byte()?;
                if ch == b'\'' || ch == b'\"' {
                    let res = self.parse_quoted_string()?;
                    FontFamily::Named(res.to_string())
                } else {
                    let mut idents = vec![];

                    while let Some(c) = self.chars().next() {
                        if c != ',' {
                            idents.push(self.parse_ident()?.to_string());
                            self.skip_spaces();
                        } else {
                            break;
                        }
                    }

                    let joined = idents.join(" ");

                    // TODO: No CSS keyword must be matched as a family name...
                    match joined.as_str() {
                        "serif" => FontFamily::Serif,
                        "sans-serif" => FontFamily::SansSerif,
                        "cursive" => FontFamily::Cursive,
                        "fantasy" => FontFamily::Fantasy,
                        "monospace" => FontFamily::Monospace,
                        _ => FontFamily::Named(joined),
                    }
                }
            };

            families.push(family);

            if let Ok(b) = self.curr_byte() {
                if b == b',' {
                    self.advance(1);
                } else {
                    break;
                }
            }
        }

        let families = families
            .into_iter()
            .filter(|f| match f {
                FontFamily::Named(s) => !s.is_empty(),
                _ => true,
            })
            .collect();

        Ok(families)
    }
}

/// The values of a [`font` shorthand](https://www.w3.org/TR/css-fonts-3/#font-prop).
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct FontShorthand<'a> {
    /// The font style.
    pub font_style: Option<&'a str>,
    /// The font variant.
    pub font_variant: Option<&'a str>,
    /// The font weight.
    pub font_weight: Option<&'a str>,
    /// The font stretch.
    pub font_stretch: Option<&'a str>,
    /// The font size.
    pub font_size: &'a str,
    /// The font family.
    pub font_family: &'a str,
}

impl<'a> FontShorthand<'a> {
    /// Parses the `font` shorthand from a string.
    ///
    /// We can't use the `FromStr` trait because it requires
    /// an owned value as a return type.
    ///
    /// [font]: https://www.w3.org/TR/css-fonts-3/#font-prop
    #[allow(clippy::should_implement_trait)] // We aren't changing public API yet.
    pub fn from_str(text: &'a str) -> Result<Self, Error> {
        let mut stream = Stream::from(text);
        stream.skip_spaces();

        let mut prev_pos = stream.pos();

        let mut font_style = None;
        let mut font_variant = None;
        let mut font_weight = None;
        let mut font_stretch = None;

        for _ in 0..4 {
            let ident = stream.consume_ascii_ident();

            match ident {
                // TODO: Reuse actual parsers to prevent duplication.
                // We ignore normal because it's ambiguous to which it belongs and all
                // other attributes need to be reset anyway.
                "normal" => {}
                "small-caps" => font_variant = Some(ident),
                "italic" | "oblique" => font_style = Some(ident),
                "bold" | "bolder" | "lighter" | "100" | "200" | "300" | "400" | "500" | "600"
                | "700" | "800" | "900" => font_weight = Some(ident),
                "ultra-condensed" | "extra-condensed" | "condensed" | "semi-condensed"
                | "semi-expanded" | "expanded" | "extra-expanded" | "ultra-expanded" => {
                    font_stretch = Some(ident)
                }
                _ => {
                    // Not one of the 4 properties, so we backtrack and then start
                    // passing font size and family.
                    stream = Stream::from(text);
                    stream.advance(prev_pos);
                    break;
                }
            }

            stream.skip_spaces();
            prev_pos = stream.pos();
        }

        prev_pos = stream.pos();
        if stream.curr_byte()?.is_digit() {
            // A font size such as '15pt'.
            let _ = stream.parse_length()?;
        } else {
            // A font size like 'xx-large'.
            let size = stream.consume_ascii_ident();

            if !matches!(
                size,
                "xx-small"
                    | "x-small"
                    | "small"
                    | "medium"
                    | "large"
                    | "x-large"
                    | "xx-large"
                    | "larger"
                    | "smaller"
            ) {
                return Err(Error::UnexpectedData(prev_pos));
            }
        }

        let font_size = stream.slice_back(prev_pos);
        stream.skip_spaces();

        if stream.curr_byte()? == b'/' {
            // We should ignore line height since it has no effect in SVG.
            stream.advance(1);
            stream.skip_spaces();
            let _ = stream.parse_length()?;
            stream.skip_spaces();
        }

        if stream.at_end() {
            return Err(Error::UnexpectedEndOfStream);
        }

        let font_family = stream.slice_tail();

        Ok(Self {
            font_style,
            font_variant,
            font_weight,
            font_stretch,
            font_size,
            font_family,
        })
    }
}

