// Copyright 2024 the Kurbo Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Triangle shape
use crate::{Circle, PathEl, Point, Rect, Shape, Vec2};

use core::cmp::*;
use core::convert::{From, Into};
use core::f64::consts::FRAC_PI_4;
use core::iter::Iterator;
use core::ops::{Add, Sub};
use core::option::Option::{self, None, Some};

#[cfg(not(feature = "std"))]
use crate::common::FloatFuncs;

/// Triangle
//     A
//     *
//    / \
//   /   \
//  *-----*
//  B     C
#[derive(Clone, Copy, PartialEq, Debug)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Triangle {
    /// vertex a.
    pub a: Point,
    /// vertex b.
    pub b: Point,
    /// vertex c.
    pub c: Point,
}

impl Triangle {
    /// The empty [`Triangle`] at the origin.
    pub const ZERO: Self = Self::from_coords((0., 0.), (0., 0.), (0., 0.));

    /// Equilateral [`Triangle`] with the x-axis unit vector as its base.
    pub const EQUILATERAL: Self = Self::from_coords(
        (
            1.0 / 2.0,
            1.732050807568877293527446341505872367_f64 / 2.0, /* (sqrt 3)/2 */
        ),
        (0.0, 0.0),
        (1.0, 0.0),
    );

    /// A new [`Triangle`] from three vertices ([`Point`]s).
    #[inline(always)]
    pub fn new(a: impl Into<Point>, b: impl Into<Point>, c: impl Into<Point>) -> Self {
        Self {
            a: a.into(),
            b: b.into(),
            c: c.into(),
        }
    }

    /// A new [`Triangle`] from three float vertex coordinates.
    ///
    /// Works as a constant [`Triangle::new`].
    #[inline(always)]
    pub const fn from_coords(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> Self {
        Self {
            a: Point::new(a.0, a.1),
            b: Point::new(b.0, b.1),
            c: Point::new(c.0, c.1),
        }
    }

    /// The centroid of the [`Triangle`].
    #[inline]
    pub fn centroid(&self) -> Point {
        (1.0 / 3.0 * (self.a.to_vec2() + self.b.to_vec2() + self.c.to_vec2())).to_point()
    }

    /// The offset of each vertex from the centroid.
    #[inline]
    pub fn offsets(&self) -> [Vec2; 3] {
        let centroid = self.centroid().to_vec2();

        [
            (self.a.to_vec2() - centroid),
            (self.b.to_vec2() - centroid),
            (self.c.to_vec2() - centroid),
        ]
    }

    /// The area of the [`Triangle`].
    #[inline]
    pub fn area(&self) -> f64 {
        0.5 * (self.b - self.a).cross(self.c - self.a)
    }

    /// Whether this [`Triangle`] has zero area.
    #[doc(alias = "is_empty")]
    #[inline]
    pub fn is_zero_area(&self) -> bool {
        self.area() == 0.0
    }

    /// The inscribed circle of [`Triangle`].
    ///
    /// This is defined as the greatest [`Circle`] that lies within the [`Triangle`].
    #[doc(alias = "incircle")]
    #[inline]
    pub fn inscribed_circle(&self) -> Circle {
        let ab = self.a.distance(self.b);
        let bc = self.b.distance(self.c);
        let ac = self.a.distance(self.c);

        // [`Vec2::div`] also multiplies by the reciprocal of the argument, but as this reciprocal
        // is reused below to calculate the radius, there's explicitly only one division needed.
        let perimeter_recip = 1. / (ab + bc + ac);
        let incenter = (self.a.to_vec2() * bc + self.b.to_vec2() * ac + self.c.to_vec2() * ab)
            * perimeter_recip;

        Circle::new(incenter.to_point(), 2.0 * self.area() * perimeter_recip)
    }

    /// The circumscribed circle of [`Triangle`].
    ///
    /// This is defined as the smallest [`Circle`] which intercepts each vertex of the [`Triangle`].
    #[doc(alias = "circumcircle")]
    #[inline]
    pub fn circumscribed_circle(&self) -> Circle {
        let b = self.b - self.a;
        let c = self.c - self.a;
        let b_len2 = b.hypot2();
        let c_len2 = c.hypot2();
        let d_recip = 0.5 / b.cross(c);

        let x = (c.y * b_len2 - b.y * c_len2) * d_recip;
        let y = (b.x * c_len2 - c.x * b_len2) * d_recip;
        let r = (b_len2 * c_len2).sqrt() * (c - b).hypot() * d_recip;

        Circle::new(self.a + Vec2::new(x, y), r)
    }

    /// Expand the triangle by a constant amount (`scalar`) in all directions.
    #[doc(alias = "offset")]
    pub fn inflate(&self, scalar: f64) -> Self {
        let centroid = self.centroid();

        Self::new(
            centroid + (0.0, scalar),
            centroid + scalar * Vec2::from_angle(5.0 * FRAC_PI_4),
            centroid + scalar * Vec2::from_angle(7.0 * FRAC_PI_4),
        )
    }

    /// Is this [`Triangle`] [finite]?
    ///
    /// [finite]: f64::is_finite
    #[inline]
    pub fn is_finite(&self) -> bool {
        self.a.is_finite() && self.b.is_finite() && self.c.is_finite()
    }

    /// Is this [`Triangle`] [NaN]?
    ///
    /// [NaN]: f64::is_nan
    #[inline]
    pub fn is_nan(&self) -> bool {
        self.a.is_nan() || self.b.is_nan() || self.c.is_nan()
    }
}

impl From<(Point, Point, Point)> for Triangle {
    fn from(points: (Point, Point, Point)) -> Triangle {
        Triangle::new(points.0, points.1, points.2)
    }
}

impl Add<Vec2> for Triangle {
    type Output = Triangle;

    #[inline]
    fn add(self, v: Vec2) -> Triangle {
        Triangle::new(self.a + v, self.b + v, self.c + v)
    }
}

impl Sub<Vec2> for Triangle {
    type Output = Triangle;

    #[inline]
    fn sub(self, v: Vec2) -> Triangle {
        Triangle::new(self.a - v, self.b - v, self.c - v)
    }
}

#[doc(hidden)]
pub struct TrianglePathIter {
    triangle: Triangle,
    ix: usize,
}

impl Shape for Triangle {
    type PathElementsIter<'iter> = TrianglePathIter;

    fn path_elements(&self, _tolerance: f64) -> TrianglePathIter {
        TrianglePathIter {
            triangle: *self,
            ix: 0,
        }
    }

    #[inline]
    fn area(&self) -> f64 {
        Triangle::area(self)
    }

    #[inline]
    fn perimeter(&self, _accuracy: f64) -> f64 {
        self.a.distance(self.b) + self.b.distance(self.c) + self.c.distance(self.a)
    }

    #[inline]
    fn winding(&self, pt: Point) -> i32 {
        let s0 = (self.b - self.a).cross(pt - self.a).signum();
        let s1 = (self.c - self.b).cross(pt - self.b).signum();
        let s2 = (self.a - self.c).cross(pt - self.c).signum();

        if s0 == s1 && s1 == s2 {
            s0 as i32
        } else {
            0
        }
    }

    #[inline]
    fn bounding_box(&self) -> Rect {
        Rect::new(
            self.a.x.min(self.b.x.min(self.c.x)),
            self.a.y.min(self.b.y.min(self.c.y)),
            self.a.x.max(self.b.x.max(self.c.x)),
            self.a.y.max(self.b.y.max(self.c.y)),
        )
    }
}

// Note: vertices a, b and c are not guaranteed to be in order as described in the struct comments
//       (i.e. as "vertex a is topmost, vertex b is leftmost, and vertex c is rightmost")
impl Iterator for TrianglePathIter {
    type Item = PathEl;

    fn next(&mut self) -> Option<PathEl> {
        self.ix += 1;
        match self.ix {
            1 => Some(PathEl::MoveTo(self.triangle.a)),
            2 => Some(PathEl::LineTo(self.triangle.b)),
            3 => Some(PathEl::LineTo(self.triangle.c)),
            4 => Some(PathEl::ClosePath),
            _ => None,
        }
    }
}

// TODO: better and more tests
