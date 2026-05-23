// Copyright 2021 the SVG Types Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![allow(missing_docs)]
use crate::{Error, Stream};

/// Representation of the [`enable-background`] attribute.
///
/// [`enable-background`]: https://www.w3.org/TR/SVG11/filters.html#EnableBackgroundProperty
#[derive(Clone, Copy, PartialEq, Debug)]
#[allow(missing_docs)]
pub enum EnableBackground {
    Accumulate,
    New,
    NewWithRegion {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
}

impl core::str::FromStr for EnableBackground {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut s = Stream::from(text);
        s.skip_spaces();
        if s.starts_with(b"accumulate") {
            s.advance(10);
            s.skip_spaces();
            if !s.at_end() {
                return Err(Error::UnexpectedData(s.calc_char_pos()));
            }

            Ok(EnableBackground::Accumulate)
        } else if s.starts_with(b"new") {
            s.advance(3);
            s.skip_spaces();
            if s.at_end() {
                return Ok(EnableBackground::New);
            }

            let x = s.parse_list_number()?;
            let y = s.parse_list_number()?;
            let width = s.parse_list_number()?;
            let height = s.parse_list_number()?;

            s.skip_spaces();
            if !s.at_end() {
                return Err(Error::UnexpectedData(s.calc_char_pos()));
            }

            // Region size must be valid;
            if !(width > 0.0 && height > 0.0) {
                return Err(Error::InvalidValue);
            }

            Ok(EnableBackground::NewWithRegion {
                x,
                y,
                width,
                height,
            })
        } else {
            Err(Error::InvalidValue)
        }
    }
}

