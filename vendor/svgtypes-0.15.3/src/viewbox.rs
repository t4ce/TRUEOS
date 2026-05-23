// Copyright 2018 the SVG Types Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use crate::Stream;

/// List of possible [`ViewBox`] parsing errors.
#[derive(Clone, Copy, Debug)]
pub enum ViewBoxError {
    /// One of the numbers is invalid.
    InvalidNumber,

    /// `ViewBox` has a negative or zero size.
    InvalidSize,
}

impl core::fmt::Display for ViewBoxError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            ViewBoxError::InvalidNumber => {
                write!(f, "viewBox contains an invalid number")
            }
            ViewBoxError::InvalidSize => {
                write!(f, "viewBox has a negative or zero size")
            }
        }
    }
}

impl core::error::Error for ViewBoxError {
    fn description(&self) -> &str {
        "a viewBox parsing error"
    }
}

/// Representation of the [`<viewBox>`] type.
///
/// [`<viewBox>`]: https://www.w3.org/TR/SVG2/coords.html#ViewBoxAttribute
#[allow(missing_docs)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ViewBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl ViewBox {
    /// Creates a new `ViewBox`.
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        ViewBox { x, y, w, h }
    }
}

impl core::str::FromStr for ViewBox {
    type Err = ViewBoxError;

    fn from_str(text: &str) -> Result<Self, ViewBoxError> {
        let mut s = Stream::from(text);

        let x = s
            .parse_list_number()
            .map_err(|_| ViewBoxError::InvalidNumber)?;
        let y = s
            .parse_list_number()
            .map_err(|_| ViewBoxError::InvalidNumber)?;
        let w = s
            .parse_list_number()
            .map_err(|_| ViewBoxError::InvalidNumber)?;
        let h = s
            .parse_list_number()
            .map_err(|_| ViewBoxError::InvalidNumber)?;

        if w <= 0.0 || h <= 0.0 {
            return Err(ViewBoxError::InvalidSize);
        }

        Ok(ViewBox::new(x, y, w, h))
    }
}

