// Copyright 2021 the SVG Types Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![allow(missing_docs)]
use crate::{colors, ByteExt, Error, Stream};
use kurbo::common::FloatFuncs;

/// Representation of the [`<color>`] type.
///
/// [`<color>`]: https://www.w3.org/TR/css-color-3/
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(missing_docs)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Color {
    /// Constructs a new `Color` from RGB values.
    #[inline]
    pub fn new_rgb(red: u8, green: u8, blue: u8) -> Color {
        Color {
            red,
            green,
            blue,
            alpha: 255,
        }
    }

    /// Constructs a new `Color` from RGBA values.
    #[inline]
    pub fn new_rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Color {
        Color {
            red,
            green,
            blue,
            alpha,
        }
    }

    /// Constructs a new `Color` set to black.
    #[inline]
    pub fn black() -> Color {
        Color::new_rgb(0, 0, 0)
    }

    /// Constructs a new `Color` set to white.
    #[inline]
    pub fn white() -> Color {
        Color::new_rgb(255, 255, 255)
    }

    /// Constructs a new `Color` set to gray.
    #[inline]
    pub fn gray() -> Color {
        Color::new_rgb(128, 128, 128)
    }

    /// Constructs a new `Color` set to red.
    #[inline]
    pub fn red() -> Color {
        Color::new_rgb(255, 0, 0)
    }

    /// Constructs a new `Color` set to green.
    #[inline]
    pub fn green() -> Color {
        Color::new_rgb(0, 128, 0)
    }

    /// Constructs a new `Color` set to blue.
    #[inline]
    pub fn blue() -> Color {
        Color::new_rgb(0, 0, 255)
    }
}

impl core::str::FromStr for Color {
    type Err = Error;

    /// Parses [CSS3](https://www.w3.org/TR/css-color-3/) `Color` from a string.
    ///
    /// # Errors
    ///
    ///  - Returns error if a color has an invalid format.
    ///  - Returns error if `<color>` is followed by `<icccolor>`. It's not supported.
    ///
    /// # Notes
    ///
    ///  - Any non-`hexdigit` bytes will be treated as `0`.
    ///  - The [SVG 1.1 spec] has an error.
    ///    There should be a `number`, not an `integer` for percent values ([details]).
    ///  - It also supports 4 digits and 8 digits hex notation from the
    ///    [CSS Color Module Level 4][css-color-4-hex].
    ///
    /// [SVG 1.1 spec]: https://www.w3.org/TR/SVG11/types.html#DataTypeColor
    /// [details]: https://lists.w3.org/Archives/Public/www-svg/2014Jan/0109.html
    /// [css-color-4-hex]: https://www.w3.org/TR/css-color-4/#hex-notation
    fn from_str(text: &str) -> Result<Self, Error> {
        let mut s = Stream::from(text);
        let color = s.parse_color()?;

        // Check that we are at the end of the stream. Otherwise color can be followed by icccolor,
        // which is not supported.
        s.skip_spaces();
        if !s.at_end() {
            return Err(Error::UnexpectedData(s.calc_char_pos()));
        }

        Ok(color)
    }
}

impl Stream<'_> {
    /// Tries to parse a color, but doesn't advance on error.
    pub fn try_parse_color(&mut self) -> Option<Color> {
        let mut s = *self;
        if let Ok(color) = s.parse_color() {
            *self = s;
            Some(color)
        } else {
            None
        }
    }

    /// Parses a color.
    pub fn parse_color(&mut self) -> Result<Color, Error> {
        self.skip_spaces();

        let mut color = Color::black();

        if self.curr_byte()? == b'#' {
            // See https://www.w3.org/TR/css-color-4/#hex-notation
            self.advance(1);
            let color_str = self.consume_bytes(|_, c| c.is_hex_digit()).as_bytes();
            // get color data len until first space or stream end
            match color_str.len() {
                6 => {
                    // #rrggbb
                    color.red = hex_pair(color_str[0], color_str[1]);
                    color.green = hex_pair(color_str[2], color_str[3]);
                    color.blue = hex_pair(color_str[4], color_str[5]);
                }
                8 => {
                    // #rrggbbaa
                    color.red = hex_pair(color_str[0], color_str[1]);
                    color.green = hex_pair(color_str[2], color_str[3]);
                    color.blue = hex_pair(color_str[4], color_str[5]);
                    color.alpha = hex_pair(color_str[6], color_str[7]);
                }
                3 => {
                    // #rgb
                    color.red = short_hex(color_str[0]);
                    color.green = short_hex(color_str[1]);
                    color.blue = short_hex(color_str[2]);
                }
                4 => {
                    // #rgba
                    color.red = short_hex(color_str[0]);
                    color.green = short_hex(color_str[1]);
                    color.blue = short_hex(color_str[2]);
                    color.alpha = short_hex(color_str[3]);
                }
                _ => {
                    return Err(Error::InvalidValue);
                }
            }
        } else {
            // TODO: remove allocation
            let name = self.consume_ascii_ident().to_ascii_lowercase();
            if name == "rgb" || name == "rgba" {
                self.consume_byte(b'(')?;

                let mut is_percent = false;
                let value = self.parse_number()?;
                if self.starts_with(b"%") {
                    self.advance(1);
                    is_percent = true;
                }
                self.skip_spaces();
                self.parse_list_separator();

                if is_percent {
                    // The division and multiply are explicitly not collapsed, to ensure the red
                    // component has the same rounding behavior as the green and blue components.
                    color.red = ((value / 100.0) * 255.0).round() as u8;
                    color.green = (self.parse_list_number_or_percent()? * 255.0).round() as u8;
                    color.blue = (self.parse_list_number_or_percent()? * 255.0).round() as u8;
                } else {
                    color.red = value.round() as u8;
                    color.green = self.parse_list_number()?.round() as u8;
                    color.blue = self.parse_list_number()?.round() as u8;
                }

                self.skip_spaces();
                if !self.starts_with(b")") {
                    color.alpha = (self.parse_list_number()? * 255.0).round() as u8;
                }

                self.skip_spaces();
                self.consume_byte(b')')?;
            } else if name == "hsl" || name == "hsla" {
                self.consume_byte(b'(')?;

                let mut hue = self.parse_list_number()?;
                hue = ((hue % 360.0) + 360.0) % 360.0;

                let saturation = f64_bound(0.0, self.parse_list_number_or_percent()?, 1.0);
                let lightness = f64_bound(0.0, self.parse_list_number_or_percent()?, 1.0);

                color = hsl_to_rgb(hue as f32 / 60.0, saturation as f32, lightness as f32);

                self.skip_spaces();
                if !self.starts_with(b")") {
                    color.alpha = (self.parse_list_number()? * 255.0).round() as u8;
                }

                self.skip_spaces();
                self.consume_byte(b')')?;
            } else {
                match colors::from_str(&name) {
                    Some(c) => {
                        color = c;
                    }
                    None => {
                        return Err(Error::InvalidValue);
                    }
                }
            }
        }

        Ok(color)
    }
}

#[inline]
fn from_hex(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => b'0',
    }
}

#[inline]
fn short_hex(c: u8) -> u8 {
    let h = from_hex(c);
    (h << 4) | h
}

#[inline]
fn hex_pair(c1: u8, c2: u8) -> u8 {
    let h1 = from_hex(c1);
    let h2 = from_hex(c2);
    (h1 << 4) | h2
}

// `hue` is in a 0..6 range, while `saturation` and `lightness` are in a 0..=1 range.
// Based on https://www.w3.org/TR/css-color-3/#hsl-color
fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> Color {
    let t2 = if lightness <= 0.5 {
        lightness * (saturation + 1.0)
    } else {
        lightness + saturation - (lightness * saturation)
    };

    let t1 = lightness * 2.0 - t2;
    let red = hue_to_rgb(t1, t2, hue + 2.0);
    let green = hue_to_rgb(t1, t2, hue);
    let blue = hue_to_rgb(t1, t2, hue - 2.0);
    Color::new_rgb(
        (red * 255.0).round() as u8,
        (green * 255.0).round() as u8,
        (blue * 255.0).round() as u8,
    )
}

fn hue_to_rgb(t1: f32, t2: f32, mut hue: f32) -> f32 {
    if hue < 0.0 {
        hue += 6.0;
    }
    if hue >= 6.0 {
        hue -= 6.0;
    }

    if hue < 1.0 {
        (t2 - t1) * hue + t1
    } else if hue < 3.0 {
        t2
    } else if hue < 4.0 {
        (t2 - t1) * (4.0 - hue) + t1
    } else {
        t1
    }
}

#[inline]
fn f64_bound(min: f64, val: f64, max: f64) -> f64 {
    debug_assert!(val.is_finite());
    val.clamp(min, max)
}

