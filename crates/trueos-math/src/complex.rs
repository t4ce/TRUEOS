/// Minimal complex number utilities tailored for kernel-side math use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Complex {
    pub re: f64,
    pub im: f64,
}

impl Complex {
    /// Creates a new complex number from real and imaginary parts.
    #[inline]
    pub const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    /// Squares the complex number.
    #[inline]
    pub fn square(self) -> Self {
        // (a + bi)^2 = (a^2 - b^2) + 2abi
        Self {
            re: (self.re * self.re) - (self.im * self.im),
            im: 2.0 * self.re * self.im,
        }
    }

    /// Adds another complex number.
    #[inline]
    pub fn add(self, other: Self) -> Self {
        Self {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }

    /// Returns the squared magnitude, which avoids the costly square-root.
    #[inline]
    pub fn magnitude_squared(self) -> f64 {
        (self.re * self.re) + (self.im * self.im)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complex_ops_smoke() {
        let a = Complex::new(3.0, 4.0);
        let b = Complex::new(1.0, 2.0);

        let sum = a.add(b);
        assert_eq!(sum, Complex::new(4.0, 6.0));

        let sq = a.square();
        assert_eq!(sq, Complex::new(-7.0, 24.0));

        let mag2 = a.magnitude_squared();
        assert_eq!(mag2, 25.0);
    }
}
