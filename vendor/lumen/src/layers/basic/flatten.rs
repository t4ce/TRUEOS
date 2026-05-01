use crate::autograd::Tensor;
use crate::module::Module;
use crate::ops::shape::reshape;
pub struct Flatten;
impl Flatten {
    pub fn new() -> Self {
        Flatten
    }
}
impl Module for Flatten {
    fn forward(&self, input: Tensor) -> Tensor {
        let shape = input.shape_vec();
        assert!(!shape.is_empty(), "Flatten expects at least 1D input");
        let b = shape[0];
        reshape(&input, vec![b as i32, -1])
    }
    fn parameters(&self) -> Vec<Tensor> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autograd::no_grad;
    use crate::precision::DType;
    use ndarray::{Array, IxDyn};

    #[test]
    fn flatten_no_grad_preserves_bf16_dtype() {
        let input = Tensor::from_array_no_grad(
            Array::from_shape_vec(IxDyn(&[2, 3, 4]), (0..24).map(|v| v as f32).collect())
                .expect("test tensor shape mismatch")
                .into_dyn(),
        );
        input.cast_inplace(DType::BF16);

        let out = no_grad(|| Flatten::new().forward(input));
        assert_eq!(out.shape_vec(), vec![2, 12]);
        assert_eq!(out.dtype(), DType::BF16);
    }
}
