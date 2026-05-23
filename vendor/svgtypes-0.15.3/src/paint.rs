// Copyright 2018 the SVG Types Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use core::str::FromStr;

use crate::{Color, Error, Stream};

/// Representation of the fallback part of the [`<paint>`] type.
///
/// Used by the [`Paint`] type.
///
/// [`<paint>`]: https://www.w3.org/TR/SVG2/painting.html#SpecifyingPaint
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaintFallback {
    /// The `none` value.
    None,
    /// The `currentColor` value.
    CurrentColor,
    /// [`<color>`] value.
    ///
    /// [`<color>`]: https://www.w3.org/TR/css-color-3/
    Color(Color),
}

/// Representation of the [`<paint>`] type.
///
/// Doesn't own the data. Use only for parsing.
///
/// `<icccolor>` isn't supported.
///
/// [`<paint>`]: https://www.w3.org/TR/SVG2/painting.html#SpecifyingPaint
///
/// # Examples
///
/// ```
/// use svgtypes::{Paint, PaintFallback, Color};
///
/// let paint = Paint::from_str("url(#gradient) red").unwrap();
/// assert_eq!(paint, Paint::FuncIRI("gradient",
///                                  Some(PaintFallback::Color(Color::red()))));
///
/// let paint = Paint::from_str("inherit").unwrap();
/// assert_eq!(paint, Paint::Inherit);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Paint<'a> {
    /// The `none` value.
    None,
    /// The `inherit` value.
    Inherit,
    /// The `currentColor` value.
    CurrentColor,
    /// [`<color>`] value.
    ///
    /// [`<color>`]: https://www.w3.org/TR/css-color-3/
    Color(Color),
    /// [`<FuncIRI>`] value with an optional fallback.
    ///
    /// [`<FuncIRI>`]: https://www.w3.org/TR/SVG11/types.html#DataTypeFuncIRI
    FuncIRI(&'a str, Option<PaintFallback>),
    /// The `context-fill` value.
    ContextFill,
    /// The `context-stroke` value.
    ContextStroke,
}

impl<'a> Paint<'a> {
    /// Parses a `Paint` from a string.
    ///
    /// We can't use the `FromStr` trait because it requires
    /// an owned value as a return type.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &'a str) -> Result<Self, Error> {
        let text = text.trim();
        match text {
            "none" => Ok(Paint::None),
            "inherit" => Ok(Paint::Inherit),
            "currentColor" => Ok(Paint::CurrentColor),
            "context-fill" => Ok(Paint::ContextFill),
            "context-stroke" => Ok(Paint::ContextStroke),
            _ => {
                let mut s = Stream::from(text);
                if s.starts_with(b"url(") {
                    match s.parse_func_iri() {
                        Ok(link) => {
                            s.skip_spaces();

                            // get fallback
                            if !s.at_end() {
                                let fallback = s.slice_tail();
                                match fallback {
                                    "none" => Ok(Paint::FuncIRI(link, Some(PaintFallback::None))),
                                    "currentColor" => {
                                        Ok(Paint::FuncIRI(link, Some(PaintFallback::CurrentColor)))
                                    }
                                    _ => {
                                        let color = Color::from_str(fallback)?;
                                        Ok(Paint::FuncIRI(link, Some(PaintFallback::Color(color))))
                                    }
                                }
                            } else {
                                Ok(Paint::FuncIRI(link, None))
                            }
                        }
                        Err(_) => Err(Error::InvalidValue),
                    }
                } else {
                    match Color::from_str(text) {
                        Ok(c) => Ok(Paint::Color(c)),
                        Err(_) => Err(Error::InvalidValue),
                    }
                }
            }
        }
    }
}

