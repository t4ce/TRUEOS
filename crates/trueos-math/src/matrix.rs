/// Fixed-size matrix type backed by a stack array.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix<const ROWS: usize, const COLS: usize> {
    pub data: [[f64; COLS]; ROWS],
}

impl<const ROWS: usize, const COLS: usize> Matrix<ROWS, COLS> {
    #[inline]
    pub const fn new(data: [[f64; COLS]; ROWS]) -> Self {
        Self { data }
    }

    #[inline]
    pub const fn as_array(&self) -> &[[f64; COLS]; ROWS] {
        &self.data
    }

    #[inline]
    pub const fn into_array(self) -> [[f64; COLS]; ROWS] {
        self.data
    }

    /// Multiplies `self` (`ROWS x COLS`) by `rhs` (`COLS x OUT_COLS`).
    pub fn mul<const OUT_COLS: usize>(
        &self,
        rhs: &Matrix<COLS, OUT_COLS>,
    ) -> Matrix<ROWS, OUT_COLS> {
        let mut out = [[0.0; OUT_COLS]; ROWS];

        let mut r = 0;
        while r < ROWS {
            let mut c = 0;
            while c < OUT_COLS {
                let mut k = 0;
                let mut sum = 0.0;
                while k < COLS {
                    sum += self.data[r][k] * rhs.data[k][c];
                    k += 1;
                }
                out[r][c] = sum;
                c += 1;
            }
            r += 1;
        }

        Matrix::new(out)
    }
}

/// Row-vector (`1 x N`) wrapper for matrix math ergonomics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vector<const N: usize> {
    pub data: [f64; N],
}

impl<const N: usize> Vector<N> {
    #[inline]
    pub const fn new(data: [f64; N]) -> Self {
        Self { data }
    }

    #[inline]
    pub const fn as_array(&self) -> &[f64; N] {
        &self.data
    }

    #[inline]
    pub const fn into_array(self) -> [f64; N] {
        self.data
    }

    #[inline]
    pub const fn as_row_matrix(&self) -> Matrix<1, N> {
        Matrix::new([self.data])
    }

    #[inline]
    pub const fn from_row_matrix(matrix: Matrix<1, N>) -> Self {
        Self {
            data: matrix.data[0],
        }
    }

    /// Multiplies this row-vector (`1 x N`) by a matrix (`N x OUT_COLS`).
    #[inline]
    pub fn mul_matrix<const OUT_COLS: usize>(&self, rhs: &Matrix<N, OUT_COLS>) -> Vector<OUT_COLS> {
        let row = self.as_row_matrix();
        Vector::from_row_matrix(row.mul(rhs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_mul_smoke() {
        let a = Matrix::<2, 3>::new([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]);
        let b = Matrix::<3, 2>::new([[7.0, 8.0], [9.0, 10.0], [11.0, 12.0]]);

        let out = a.mul(&b);

        assert_eq!(out, Matrix::<2, 2>::new([[58.0, 64.0], [139.0, 154.0]]));
    }

    #[test]
    fn vector_as_row_matrix_mul() {
        let v = Vector::<3>::new([1.0, 2.0, 3.0]);
        let m = Matrix::<3, 2>::new([[2.0, 0.0], [1.0, 2.0], [0.0, 1.0]]);

        let out = v.mul_matrix(&m);

        assert_eq!(out, Vector::<2>::new([4.0, 7.0]));
    }
}
