use crate::{Command, Glyph, Point};

/// A glyph viewport in whole font units, with an SVG-style downward Y axis.
pub(crate) struct Bounds {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

impl Bounds {
    pub(crate) fn for_glyph(glyph: Glyph<'_>) -> Self {
        let metrics = glyph.metrics();
        let advance = glyph.advance();
        let mut bounds = Self {
            left: 0,
            top: -i32::from(metrics.ascender),
            right: i32::from(if advance == 0 {
                metrics.units_per_em
            } else {
                advance
            }),
            bottom: -i32::from(metrics.descender),
        };
        for layer in glyph.layers() {
            for command in layer.commands() {
                match command {
                    Command::Move(point) | Command::Line(point) => bounds.include(point),
                    Command::Quad(control, end) => {
                        bounds.include(control);
                        bounds.include(end);
                    }
                    Command::Cubic(control1, control2, end) => {
                        bounds.include(control1);
                        bounds.include(control2);
                        bounds.include(end);
                    }
                    Command::Close => {}
                }
            }
        }
        bounds
    }

    pub(crate) fn extent(&self) -> [i32; 2] {
        [self.right - self.left, self.bottom - self.top]
    }

    fn include(&mut self, point: Point) {
        let x_floor = point.x.div_euclid(64);
        let y_floor = point.y.div_euclid(64);
        let x_ceil = x_floor + i32::from(point.x.rem_euclid(64) != 0);
        let y_ceil = y_floor + i32::from(point.y.rem_euclid(64) != 0);
        self.left = self.left.min(x_floor);
        self.top = self.top.min(-y_ceil);
        self.right = self.right.max(x_ceil);
        self.bottom = self.bottom.max(-y_floor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_box_padding_includes_fractional_overhangs() {
        let mut bounds = Bounds {
            left: 0,
            top: -4,
            right: 4,
            bottom: 0,
        };
        bounds.include(Point { x: -1, y: 257 });
        bounds.include(Point { x: 257, y: -1 });
        assert_eq!(
            (bounds.left, bounds.top, bounds.right, bounds.bottom),
            (-1, -5, 5, 1)
        );
    }
}
