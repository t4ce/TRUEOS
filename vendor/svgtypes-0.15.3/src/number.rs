// Copyright 2018 the SVG Types Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use core::str::FromStr;

use crate::{ByteExt, Error, Stream};

/// An [SVG number](https://www.w3.org/TR/SVG2/types.html#InterfaceSVGNumber).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Number(pub f64);

impl core::str::FromStr for Number {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut s = Stream::from(text);
        let n = s.parse_number()?;
        s.skip_spaces();
        if !s.at_end() {
            return Err(Error::UnexpectedData(s.calc_char_pos()));
        }

        Ok(Self(n))
    }
}

impl Stream<'_> {
    /// Parses number from the stream.
    ///
    /// This method will detect a number length and then
    /// will pass a substring to the `f64::from_str` method.
    ///
    /// <https://www.w3.org/TR/SVG2/types.html#InterfaceSVGNumber>
    ///
    /// # Errors
    ///
    /// Returns only `InvalidNumber`.
    pub fn parse_number(&mut self) -> Result<f64, Error> {
        // Strip off leading whitespaces.
        self.skip_spaces();

        let start = self.pos();

        if self.at_end() {
            return Err(Error::InvalidNumber(self.calc_char_pos_at(start)));
        }

        self.parse_number_impl()
            .map_err(|_| Error::InvalidNumber(self.calc_char_pos_at(start)))
    }

    fn parse_number_impl(&mut self) -> Result<f64, Error> {
        let start = self.pos();

        let mut c = self.curr_byte()?;

        // Consume sign.
        if c.is_sign() {
            self.advance(1);
            c = self.curr_byte()?;
        }

        // Consume integer.
        match c {
            b'0'..=b'9' => self.skip_digits(),
            b'.' => {}
            _ => return Err(Error::InvalidNumber(0)),
        }

        // Consume fraction.
        if let Ok(b'.') = self.curr_byte() {
            self.advance(1);
            self.skip_digits();
        }

        if let Ok(c) = self.curr_byte() {
            if matches!(c, b'e' | b'E') {
                let c2 = self.next_byte()?;
                // Check for `em`/`ex`.
                if c2 != b'm' && c2 != b'x' {
                    self.advance(1);

                    match self.curr_byte()? {
                        b'+' | b'-' => {
                            self.advance(1);
                            self.skip_digits();
                        }
                        b'0'..=b'9' => self.skip_digits(),
                        _ => {
                            return Err(Error::InvalidNumber(0));
                        }
                    }
                }
            }
        }

        let s = self.slice_back(start);

        // Use the default f64 parser now.
        if let Ok(n) = f64::from_str(s) {
            // inf, nan, etc. are an error.
            if n.is_finite() {
                return Ok(n);
            }
        }

        Err(Error::InvalidNumber(0))
    }

    /// Parses number from a list of numbers.
    pub fn parse_list_number(&mut self) -> Result<f64, Error> {
        if self.at_end() {
            return Err(Error::UnexpectedEndOfStream);
        }

        let n = self.parse_number()?;
        self.skip_spaces();
        self.parse_list_separator();
        Ok(n)
    }
}

/// A pull-based [`<list-of-numbers>`] parser.
///
/// # Examples
///
/// ```
/// use svgtypes::NumberListParser;
///
/// let mut p = NumberListParser::from("10, 20 -50");
/// assert_eq!(p.next().unwrap().unwrap(), 10.0);
/// assert_eq!(p.next().unwrap().unwrap(), 20.0);
/// assert_eq!(p.next().unwrap().unwrap(), -50.0);
/// assert_eq!(p.next().is_none(), true);
/// ```
///
/// [`<list-of-numbers>`]: https://www.w3.org/TR/SVG2/types.html#InterfaceSVGNumberList
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NumberListParser<'a>(Stream<'a>);

impl<'a> From<&'a str> for NumberListParser<'a> {
    #[inline]
    fn from(v: &'a str) -> Self {
        NumberListParser(Stream::from(v))
    }
}

impl Iterator for NumberListParser<'_> {
    type Item = Result<f64, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.at_end() {
            None
        } else {
            let v = self.0.parse_list_number();
            if v.is_err() {
                self.0.jump_to_end();
            }

            Some(v)
        }
    }
}

