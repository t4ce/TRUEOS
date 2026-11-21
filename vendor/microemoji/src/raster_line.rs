//! Flatten glyph contours into oriented line edges for a nonzero-winding fill.

use crate::{Command, Commands, Layer, Metrics, Point};

const MAX_DEPTH: u8 = 4;

type Position = [f64; 2];

/// A directed edge in font units, with positive y pointing up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub from: [f64; 2],
    pub to: [f64; 2],
}

/// The viewport height must be positive in both font units and pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidHeight;

impl core::fmt::Display for InvalidHeight {
    fn fmt(&self, out: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        out.write_str("microemoji viewport height must be positive")
    }
}

impl core::error::Error for InvalidHeight {}

/// Allocation-free line geometry for one filled color layer.
///
/// Edges preserve contour direction and closure, omitting zero-length edges.
/// Fill all edges of a layer together to preserve holes, then composite the
/// layers in the order they occur in the glyph.
pub struct LayerEdges<'a> {
    commands: Commands<'a>,
    color: [u8; 4],
    current: Position,
    start: Position,
    curves: CurveEdges,
}

impl<'a> LayerEdges<'a> {
    /// Set the target pixel height of the ascender-to-descender span.
    ///
    /// Curves subdivide until their control points project within a quarter
    /// pixel of the chord, or the size-based limit is reached. The limit is
    /// the height rounded up to a power of two and clamped to 4–16 edges per
    /// curve. At the limit a curve may exceed the quarter-pixel target.
    pub fn new(layer: Layer<'a>, metrics: Metrics, height_px: u32) -> Result<Self, InvalidHeight> {
        let font_height = i32::from(metrics.ascender) - i32::from(metrics.descender);
        if font_height <= 0 || height_px == 0 {
            return Err(InvalidHeight);
        }

        Ok(Self {
            commands: layer.commands(),
            color: layer.color(),
            current: [0.0; 2],
            start: [0.0; 2],
            curves: CurveEdges::new(f64::from(font_height), height_px),
        })
    }

    /// The layer's straight-alpha RGBA color.
    pub fn color(&self) -> [u8; 4] {
        self.color
    }

    fn line_to(&mut self, to: Position) -> Edge {
        Edge {
            from: core::mem::replace(&mut self.current, to),
            to,
        }
    }
}

impl Iterator for LayerEdges<'_> {
    type Item = Edge;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let edge = if let Some(edge) = self.curves.next() {
                edge
            } else {
                match self.commands.next()? {
                    Command::Move(point) => {
                        self.current = position(point);
                        self.start = self.current;
                        continue;
                    }
                    Command::Line(point) => self.line_to(position(point)),
                    Command::Quad(control, to) => {
                        let from = core::mem::replace(&mut self.current, position(to));
                        self.curves
                            .push(Curve::quadratic(from, position(control), self.current));
                        continue;
                    }
                    Command::Cubic(first, second, to) => {
                        let from = core::mem::replace(&mut self.current, position(to));
                        self.curves.push(Curve::cubic(
                            from,
                            position(first),
                            position(second),
                            self.current,
                        ));
                        continue;
                    }
                    Command::Close => self.line_to(self.start),
                }
            };
            if edge.from != edge.to {
                return Some(edge);
            }
        }
    }
}

fn position(point: Point) -> Position {
    [f64::from(point.x) / 64.0, f64::from(point.y) / 64.0]
}

#[derive(Clone, Copy)]
struct Curve {
    points: [Position; 4],
    depth: u8,
}

impl Curve {
    const EMPTY: Self = Self {
        points: [[0.0; 2]; 4],
        depth: 0,
    };

    fn quadratic(from: Position, control: Position, to: Position) -> Self {
        // Elevate the quadratic to a cubic so both use the same subdivision.
        Self::cubic(
            from,
            toward(from, control, 2.0 / 3.0),
            toward(to, control, 2.0 / 3.0),
            to,
        )
    }

    fn cubic(from: Position, first: Position, second: Position, to: Position) -> Self {
        Self {
            points: [from, first, second, to],
            depth: 0,
        }
    }

    fn flat_enough(self, tolerance_squared: f64) -> bool {
        let [from, first, second, to] = self.points;
        distance_squared(from, to, first) <= tolerance_squared
            && distance_squared(from, to, second) <= tolerance_squared
    }

    fn split(self) -> (Self, Self) {
        let [a, b, c, d] = self.points;
        let ab = toward(a, b, 0.5);
        let bc = toward(b, c, 0.5);
        let cd = toward(c, d, 0.5);
        let abc = toward(ab, bc, 0.5);
        let bcd = toward(bc, cd, 0.5);
        let midpoint = toward(abc, bcd, 0.5);
        let depth = self.depth + 1;
        (
            Self {
                points: [a, ab, abc, midpoint],
                depth,
            },
            Self {
                points: [midpoint, bcd, cd, d],
                depth,
            },
        )
    }
}

struct CurveEdges {
    pending: [Curve; MAX_DEPTH as usize + 1],
    len: usize,
    max_depth: u8,
    tolerance_squared: f64,
}

impl CurveEdges {
    fn new(font_height: f64, height_px: u32) -> Self {
        let tolerance = 0.25 * font_height / f64::from(height_px);
        let max_segments = height_px.clamp(4, 1 << MAX_DEPTH).next_power_of_two();
        Self {
            pending: [Curve::EMPTY; MAX_DEPTH as usize + 1],
            len: 0,
            max_depth: max_segments.ilog2() as u8,
            tolerance_squared: tolerance * tolerance,
        }
    }

    fn push(&mut self, curve: Curve) {
        self.pending[self.len] = curve;
        self.len += 1;
    }

    fn next(&mut self) -> Option<Edge> {
        self.len = self.len.checked_sub(1)?;
        let mut curve = self.pending[self.len];
        while curve.depth < self.max_depth && !curve.flat_enough(self.tolerance_squared) {
            let (left, right) = curve.split();
            self.push(right);
            curve = left;
        }
        Some(Edge {
            from: curve.points[0],
            to: curve.points[3],
        })
    }
}

fn toward(from: Position, to: Position, fraction: f64) -> Position {
    [
        from[0] + (to[0] - from[0]) * fraction,
        from[1] + (to[1] - from[1]) * fraction,
    ]
}

fn distance_squared(from: Position, to: Position, point: Position) -> f64 {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let chord_squared = dx * dx + dy * dy;
    let point_x = point[0] - from[0];
    let point_y = point[1] - from[1];
    // Use the finite chord so collinear curves that reverse direction still
    // subdivide. A coincident start and end reduces to point distance.
    let fraction = if chord_squared == 0.0 {
        0.0
    } else {
        ((point_x * dx + point_y * dy) / chord_squared).clamp(0.0, 1.0)
    };
    let x = point_x - dx * fraction;
    let y = point_y - dy * fraction;
    x * x + y * y
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn edge_count(curve: Curve, height_px: u32) -> usize {
        let mut curves = CurveEdges::new(1000.0, height_px);
        curves.push(curve);
        let mut current = curve.points[0];
        let mut count = 0;
        while let Some(edge) = curves.next() {
            assert_eq!(edge.from, current);
            current = edge.to;
            count += 1;
        }
        assert_eq!(current, curve.points[3]);
        count
    }

    #[test]
    fn larger_render_sizes_receive_more_edges_with_exact_connections() {
        let arch = Curve::quadratic([0.0, 0.0], [500.0, 1000.0], [1000.0, 0.0]);
        assert!(edge_count(arch, 16) < edge_count(arch, 256));
    }

    #[test]
    fn straight_curve_stays_one_edge_even_at_large_sizes() {
        let line = Curve::cubic([0.0, 0.0], [300.0, 300.0], [700.0, 700.0], [1000.0, 1000.0]);
        assert_eq!(edge_count(line, 256), 1);
    }

    #[test]
    fn collinear_curve_with_coincident_ends_still_subdivides() {
        let loop_back = Curve::quadratic([0.0, 0.0], [500.0, 0.0], [0.0, 0.0]);
        assert_eq!(edge_count(loop_back, 16), 2);
    }

    #[test]
    fn limits_hold_even_when_the_error_target_cannot_be_met() {
        let arch = Curve::quadratic([-1e7, 0.0], [0.0, 1e7], [1e7, 0.0]);
        assert_eq!(edge_count(arch, 4), 4);
        assert_eq!(edge_count(arch, 24), 16);
        assert_eq!(edge_count(arch, u32::MAX), 16);
    }

    #[test]
    fn layer_edges_close_contours_without_connecting_them() {
        let mut path = Vec::new();
        for contour in [[[0i32, 0], [4, 0], [0, 4]], [[1, 1], [1, 2], [2, 1]]] {
            for (index, [x, y]) in contour.into_iter().enumerate() {
                path.push(u8::from(index != 0));
                path.extend((x * 64).to_le_bytes());
                path.extend((y * 64).to_le_bytes());
            }
            path.push(4);
        }
        let color = [1, 2, 3, 128];
        let layer = Layer { color, path: &path };
        let metrics = Metrics {
            units_per_em: 4,
            ascender: 4,
            descender: 0,
        };
        let edges = LayerEdges::new(layer, metrics, 16).unwrap();
        assert_eq!(edges.color(), color);
        assert_eq!(
            edges.map(|edge| (edge.from, edge.to)).collect::<Vec<_>>(),
            [
                ([0.0, 0.0], [4.0, 0.0]),
                ([4.0, 0.0], [0.0, 4.0]),
                ([0.0, 4.0], [0.0, 0.0]),
                ([1.0, 1.0], [1.0, 2.0]),
                ([1.0, 2.0], [2.0, 1.0]),
                ([2.0, 1.0], [1.0, 1.0]),
            ]
        );
        assert!(LayerEdges::new(layer, metrics, 0).is_err());
        assert!(
            LayerEdges::new(
                layer,
                Metrics {
                    ascender: -1,
                    ..metrics
                },
                16
            )
            .is_err()
        );
    }
}
