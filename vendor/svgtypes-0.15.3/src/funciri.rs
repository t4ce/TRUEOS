// Copyright 2021 the SVG Types Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use crate::{Error, Stream};

/// Representation of the [`<IRI>`] type.
///
/// [`<IRI>`]: https://www.w3.org/TR/SVG11/types.html#DataTypeIRI
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IRI<'a>(pub &'a str);

impl<'a> IRI<'a> {
    /// Parsers a `IRI` from a string.
    ///
    /// By the SVG spec, the ID must contain only [Name] characters,
    /// but since no one fallows this it will parse any characters.
    ///
    /// We can't use the `FromStr` trait because it requires
    /// an owned value as a return type.
    ///
    /// [Name]: https://www.w3.org/TR/xml/#NT-Name
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &'a str) -> Result<Self, Error> {
        let mut s = Stream::from(text);
        let link = s.parse_iri()?;
        s.skip_spaces();
        if !s.at_end() {
            return Err(Error::UnexpectedData(s.calc_char_pos()));
        }

        Ok(Self(link))
    }
}

/// Representation of the [`<FuncIRI>`] type.
///
/// [`<FuncIRI>`]: https://www.w3.org/TR/SVG11/types.html#DataTypeFuncIRI
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuncIRI<'a>(pub &'a str);

impl<'a> FuncIRI<'a> {
    /// Parsers a `FuncIRI` from a string.
    ///
    /// By the SVG spec, the ID must contain only [Name] characters,
    /// but since no one fallows this it will parse any characters.
    ///
    /// We can't use the `FromStr` trait because it requires
    /// an owned value as a return type.
    ///
    /// [Name]: https://www.w3.org/TR/xml/#NT-Name
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &'a str) -> Result<Self, Error> {
        let mut s = Stream::from(text);
        let link = s.parse_func_iri()?;
        s.skip_spaces();
        if !s.at_end() {
            return Err(Error::UnexpectedData(s.calc_char_pos()));
        }

        Ok(Self(link))
    }
}

impl<'a> Stream<'a> {
    pub fn parse_iri(&mut self) -> Result<&'a str, Error> {
        self.skip_spaces();
        self.consume_byte(b'#')?;
        let link = self.consume_bytes(|_, c| c != b' ');
        if link.is_empty() {
            return Err(Error::InvalidValue);
        }
        Ok(link)
    }

    pub fn parse_func_iri(&mut self) -> Result<&'a str, Error> {
        self.skip_spaces();
        self.consume_string(b"url(")?;
        self.skip_spaces();

        let quote = match self.curr_byte() {
            Ok(b'\'') | Ok(b'"') => self.curr_byte().ok(),
            _ => None,
        };
        if quote.is_some() {
            self.advance(1);
            self.skip_spaces();
        }
        self.consume_byte(b'#')?;
        let link = if let Some(quote) = quote {
            self.consume_bytes(|_, c| c != quote).trim_end()
        } else {
            self.consume_bytes(|_, c| c != b' ' && c != b')')
        };
        if link.is_empty() {
            return Err(Error::InvalidValue);
        }
        // Non-paired quotes is an error.
        if link.contains('\'') || link.contains('"') {
            return Err(Error::InvalidValue);
        }
        self.skip_spaces();
        if let Some(quote) = quote {
            self.consume_byte(quote)?;
            self.skip_spaces();
        }
        self.consume_byte(b')')?;
        Ok(link)
    }
}

