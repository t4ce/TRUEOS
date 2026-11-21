//! Stream standalone SVGs and embeddable glyph fragments through `core::fmt::Write`.

use core::fmt::{self, Write};

use crate::bounds::Bounds;
use crate::{Command, Glyph, Point};

/// Write a standalone SVG with an intrinsic size in font units. The viewBox
/// includes the glyph's advance and font line height, expanding for overhangs.
pub fn write_glyph<W: Write + ?Sized>(out: &mut W, glyph: Glyph<'_>) -> fmt::Result {
    let bounds = Bounds::for_glyph(glyph);
    let [width, height] = bounds.extent();
    write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"{} {} {width} {height}\">",
        bounds.left, bounds.top
    )?;
    write_fragment(out, glyph)?;
    out.write_str("</svg>\n")
}

/// Write a glyph as a `<g>` fragment. Its origin is the baseline. Positive x
/// points right, positive SVG y points down, and coordinates use font units.
/// Wrap the fragment in an SVG element with a viewBox that includes its bounds.
pub fn write_fragment<W: Write + ?Sized>(out: &mut W, glyph: Glyph<'_>) -> fmt::Result {
    out.write_str(
        "<g transform=\"scale(1 -1)\" fill-rule=\"nonzero\" fill-opacity=\"1\" stroke=\"none\">",
    )?;
    for layer in glyph.layers() {
        write_path(out, layer.color(), layer.commands())?;
    }
    out.write_str("</g>")
}

fn write_path<W: Write + ?Sized>(
    out: &mut W,
    [r, g, b, a]: [u8; 4],
    commands: impl Iterator<Item = Command>,
) -> fmt::Result {
    write!(out, "<path fill=\"#{r:02x}{g:02x}{b:02x}\"")?;
    if a < u8::MAX {
        let millionths = (u32::from(a) * 1_000_000 + 127) / 255;
        write!(out, " fill-opacity=\"0.{millionths:06}\"")?;
    }
    out.write_str(" d=\"")?;
    for command in commands {
        match command {
            Command::Move(point) => write!(out, "M{}", SvgPoint(point))?,
            Command::Line(point) => write!(out, "L{}", SvgPoint(point))?,
            Command::Quad(control, end) => {
                write!(out, "Q{} {}", SvgPoint(control), SvgPoint(end))?;
            }
            Command::Cubic(control1, control2, end) => {
                write!(
                    out,
                    "C{} {} {}",
                    SvgPoint(control1),
                    SvgPoint(control2),
                    SvgPoint(end)
                )?;
            }
            Command::Close => out.write_char('Z')?,
        }
    }
    out.write_str("\"/>")
}

struct SvgPoint(Point);

impl fmt::Display for SvgPoint {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "{} {}", FontUnits(self.0.x), FontUnits(self.0.y))
    }
}

struct FontUnits(i32);

impl fmt::Display for FontUnits {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 < 0 {
            out.write_char('-')?;
        }
        let magnitude = self.0.unsigned_abs();
        write!(out, "{}", magnitude / 64)?;
        let mut fraction = (magnitude % 64) * 15_625;
        if fraction != 0 {
            let mut digits = 6;
            while fraction % 10 == 0 {
                fraction /= 10;
                digits -= 1;
            }
            write!(out, ".{fraction:0digits$}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use self::std::string::String;
    use super::*;

    #[test]
    fn path_keeps_curve_commands_and_exact_fixed_point_coordinates() {
        let mut out = String::new();
        let commands = [
            Command::Move(Point { x: 0, y: -1 }),
            Command::Line(Point { x: 64, y: -32 }),
            Command::Quad(Point { x: 256, y: 8 }, Point { x: 9, y: 96 }),
            Command::Cubic(
                Point { x: 0, y: 64 },
                Point { x: -16, y: 32 },
                Point { x: i32::MIN, y: 0 },
            ),
            Command::Close,
        ];
        write_path(&mut out, [1, 15, 255, 128], commands.into_iter()).unwrap();
        assert_eq!(
            out,
            "<path fill=\"#010fff\" fill-opacity=\"0.501961\" d=\"M0 -0.015625L1 -0.5Q4 0.125 0.140625 1.5C0 1 -0.25 0.5 -33554432 0Z\"/>"
        );
    }
}
