//! Basic path, point, and filled vertex-buffer types for kernel graphics.

use alloc::vec::Vec;

const DEFAULT_CURVE_TOLERANCE: f32 = 0.1;
const MAX_CURVE_DEPTH: u8 = 12;
const EPSILON: f32 = 1.0e-5;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Point {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

pub(crate) const fn point(x: f32, y: f32) -> Point {
    Point { x, y }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FillOptions {
    pub(crate) tolerance: f32,
}

impl FillOptions {
    pub(crate) const DEFAULT: Self = Self {
        tolerance: DEFAULT_CURVE_TOLERANCE,
    };

    pub(crate) const fn with_tolerance(mut self, tolerance: f32) -> Self {
        self.tolerance = tolerance;
        self
    }

    fn tolerance_squared(self) -> f32 {
        let tolerance = if self.tolerance.is_finite() && self.tolerance > 0.0 {
            self.tolerance
        } else {
            DEFAULT_CURVE_TOLERANCE
        };
        tolerance * tolerance
    }
}

impl Default for FillOptions {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Default)]
pub(crate) struct VertexBuffers {
    pub(crate) vertices: Vec<[f32; 2]>,
    pub(crate) indices: Vec<u32>,
}

impl VertexBuffers {
    pub(crate) const fn new() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FillError {
    IndexOverflow,
}

#[derive(Clone, Copy)]
struct Segment {
    from: Point,
    to: Point,
}

pub(crate) struct Path {
    segments: Vec<Segment>,
}

pub(crate) struct PathBuilder {
    segments: Vec<Segment>,
    first: Option<Point>,
    current: Option<Point>,
    curve_tolerance_squared: f32,
}

impl Path {
    pub(crate) fn builder() -> PathBuilder {
        Self::builder_with_options(&FillOptions::DEFAULT)
    }

    pub(crate) fn builder_with_options(options: &FillOptions) -> PathBuilder {
        PathBuilder {
            segments: Vec::new(),
            first: None,
            current: None,
            curve_tolerance_squared: options.tolerance_squared(),
        }
    }

    pub(crate) fn tessellate(&self, _options: &FillOptions) -> Result<VertexBuffers, FillError> {
        tessellate_non_zero(self.segments.as_slice())
    }
}

impl PathBuilder {
    pub(crate) fn begin(&mut self, at: Point) {
        self.finish_contour();
        if finite(at) {
            self.first = Some(at);
            self.current = Some(at);
        }
    }

    pub(crate) fn line_to(&mut self, to: Point) {
        let Some(from) = self.current else {
            self.begin(to);
            return;
        };
        push_segment(&mut self.segments, from, to);
        self.current = finite(to).then_some(to);
    }

    pub(crate) fn quadratic_bezier_to(&mut self, control: Point, to: Point) {
        let Some(from) = self.current else {
            self.begin(to);
            return;
        };
        flatten_quadratic(from, control, to, self.curve_tolerance_squared, 0, &mut self.segments);
        self.current = finite(to).then_some(to);
    }

    pub(crate) fn cubic_bezier_to(&mut self, control0: Point, control1: Point, to: Point) {
        let Some(from) = self.current else {
            self.begin(to);
            return;
        };
        flatten_cubic(
            from,
            control0,
            control1,
            to,
            self.curve_tolerance_squared,
            0,
            &mut self.segments,
        );
        self.current = finite(to).then_some(to);
    }

    pub(crate) fn close(&mut self) {
        self.finish_contour();
    }

    pub(crate) fn end(&mut self, _closed: bool) {
        // Filled subpaths are implicitly closed.
        self.finish_contour();
    }

    pub(crate) fn build(mut self) -> Path {
        self.finish_contour();
        Path {
            segments: self.segments,
        }
    }

    fn finish_contour(&mut self) {
        if let (Some(from), Some(to)) = (self.current, self.first) {
            push_segment(&mut self.segments, from, to);
        }
        self.first = None;
        self.current = None;
    }
}

fn finite(value: Point) -> bool {
    value.x.is_finite() && value.y.is_finite()
}

fn same_point(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() <= EPSILON && (a.y - b.y).abs() <= EPSILON
}

fn push_segment(output: &mut Vec<Segment>, from: Point, to: Point) {
    if finite(from) && finite(to) && !same_point(from, to) {
        output.push(Segment { from, to });
    }
}

fn midpoint(a: Point, b: Point) -> Point {
    point((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
}

fn point_line_distance_squared(value: Point, from: Point, to: Point) -> f32 {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let len_squared = dx * dx + dy * dy;
    if len_squared <= EPSILON * EPSILON {
        let px = value.x - from.x;
        let py = value.y - from.y;
        return px * px + py * py;
    }
    let cross = (value.x - from.x) * dy - (value.y - from.y) * dx;
    cross * cross / len_squared
}

fn flatten_quadratic(
    from: Point,
    control: Point,
    to: Point,
    tolerance_squared: f32,
    depth: u8,
    output: &mut Vec<Segment>,
) {
    if !finite(from) || !finite(control) || !finite(to) {
        return;
    }
    if depth >= MAX_CURVE_DEPTH
        || point_line_distance_squared(control, from, to) <= tolerance_squared
    {
        push_segment(output, from, to);
        return;
    }
    let a = midpoint(from, control);
    let b = midpoint(control, to);
    let middle = midpoint(a, b);
    flatten_quadratic(from, a, middle, tolerance_squared, depth + 1, output);
    flatten_quadratic(middle, b, to, tolerance_squared, depth + 1, output);
}

fn flatten_cubic(
    from: Point,
    control0: Point,
    control1: Point,
    to: Point,
    tolerance_squared: f32,
    depth: u8,
    output: &mut Vec<Segment>,
) {
    if !finite(from) || !finite(control0) || !finite(control1) || !finite(to) {
        return;
    }
    let flatness = point_line_distance_squared(control0, from, to)
        .max(point_line_distance_squared(control1, from, to));
    if depth >= MAX_CURVE_DEPTH || flatness <= tolerance_squared {
        push_segment(output, from, to);
        return;
    }
    let p01 = midpoint(from, control0);
    let p12 = midpoint(control0, control1);
    let p23 = midpoint(control1, to);
    let p012 = midpoint(p01, p12);
    let p123 = midpoint(p12, p23);
    let middle = midpoint(p012, p123);
    flatten_cubic(from, p01, p012, middle, tolerance_squared, depth + 1, output);
    flatten_cubic(middle, p123, p23, to, tolerance_squared, depth + 1, output);
}

#[derive(Clone, Copy)]
struct Crossing {
    segment: Segment,
    x: f32,
    winding: i32,
}

fn tessellate_non_zero(segments: &[Segment]) -> Result<VertexBuffers, FillError> {
    let mut levels = Vec::with_capacity(segments.len().saturating_mul(2));
    for segment in segments {
        levels.push(segment.from.y);
        levels.push(segment.to.y);
    }
    levels.sort_by(f32::total_cmp);
    levels.dedup_by(|a, b| (*a - *b).abs() <= EPSILON);

    let mut output = VertexBuffers::new();
    let mut crossings = Vec::new();
    for band in levels.windows(2) {
        let y0 = band[0];
        let y1 = band[1];
        if y1 - y0 <= EPSILON {
            continue;
        }
        let sample_y = (y0 + y1) * 0.5;
        crossings.clear();
        for segment in segments {
            let min_y = segment.from.y.min(segment.to.y);
            let max_y = segment.from.y.max(segment.to.y);
            if max_y - min_y <= EPSILON || sample_y <= min_y || sample_y >= max_y {
                continue;
            }
            crossings.push(Crossing {
                segment: *segment,
                x: x_at_y(*segment, sample_y),
                winding: if segment.to.y > segment.from.y { 1 } else { -1 },
            });
        }
        crossings.sort_by(|a, b| a.x.total_cmp(&b.x));

        let mut winding = 0i32;
        let mut left = None;
        for crossing in &crossings {
            let was_inside = winding != 0;
            winding = winding.saturating_add(crossing.winding);
            let is_inside = winding != 0;
            if !was_inside && is_inside {
                left = Some(crossing.segment);
            } else if was_inside && !is_inside {
                if let Some(left) = left.take() {
                    emit_trapezoid(&mut output, left, crossing.segment, y0, y1)?;
                }
            }
        }
    }
    Ok(output)
}

fn x_at_y(segment: Segment, y: f32) -> f32 {
    let dy = segment.to.y - segment.from.y;
    if dy.abs() <= EPSILON {
        return segment.from.x;
    }
    let t = ((y - segment.from.y) / dy).clamp(0.0, 1.0);
    segment.from.x + (segment.to.x - segment.from.x) * t
}

fn emit_trapezoid(
    output: &mut VertexBuffers,
    left: Segment,
    right: Segment,
    y0: f32,
    y1: f32,
) -> Result<(), FillError> {
    let middle = (y0 + y1) * 0.5;
    if x_at_y(right, middle) - x_at_y(left, middle) <= EPSILON {
        return Ok(());
    }
    let base = u32::try_from(output.vertices.len()).map_err(|_| FillError::IndexOverflow)?;
    let end = base.checked_add(3).ok_or(FillError::IndexOverflow)?;
    output.vertices.extend_from_slice(&[
        [x_at_y(left, y0), y0],
        [x_at_y(right, y0), y0],
        [x_at_y(right, y1), y1],
        [x_at_y(left, y1), y1],
    ]);
    output
        .indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, end]);
    Ok(())
}
