// Copyright 2021 the Kurbo Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Quadratic Bézier splines.
use crate::Point;

use crate::QuadBez;
use alloc::vec::Vec;

/// A quadratic Bézier spline in [B-spline](https://en.wikipedia.org/wiki/B-spline) format.
#[derive(Clone, Debug, PartialEq)]
pub struct QuadSpline(Vec<Point>);

impl QuadSpline {
    /// Construct a new `QuadSpline` from an array of [`Point`]s.
    #[inline(always)]
    pub fn new(points: Vec<Point>) -> Self {
        Self(points)
    }

    /// Return the spline's control [`Point`]s.
    #[inline(always)]
    pub fn points(&self) -> &[Point] {
        &self.0
    }

    /// Return an iterator over the implied [`QuadBez`] sequence.
    ///
    /// The returned quads are guaranteed to be G1 continuous.
    #[inline(always)]
    pub fn to_quads(&self) -> impl Iterator<Item = QuadBez> + '_ {
        ToQuadBez {
            idx: 0,
            points: &self.0,
        }
    }
}

struct ToQuadBez<'a> {
    idx: usize,
    points: &'a Vec<Point>,
}

impl Iterator for ToQuadBez<'_> {
    type Item = QuadBez;

    fn next(&mut self) -> Option<Self::Item> {
        let [mut p0, p1, mut p2]: [Point; 3] =
            self.points.get(self.idx..=self.idx + 2)?.try_into().ok()?;

        if self.idx != 0 {
            p0 = p0.midpoint(p1);
        }
        if self.idx + 2 < self.points.len() - 1 {
            p2 = p1.midpoint(p2);
        }

        self.idx += 1;

        Some(QuadBez { p0, p1, p2 })
    }
}
