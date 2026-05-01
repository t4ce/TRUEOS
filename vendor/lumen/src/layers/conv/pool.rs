use crate::autograd::Tensor;
use crate::module::Module;
use crate::ops::convolution::max_pool2d;

pub struct MaxPool2D {
    pub kernel_size: usize,
    pub stride: usize,
}
impl MaxPool2D {
    pub fn new(kernel_size: usize, stride: usize) -> Self {
        MaxPool2D {
            kernel_size,
            stride,
        }
    }
}
impl Module for MaxPool2D {
    fn forward(&self, input: Tensor) -> Tensor {
        max_pool2d(
            &input,
            (self.kernel_size, self.kernel_size),
            (self.stride, self.stride),
        )
    }
    fn parameters(&self) -> Vec<Tensor> {
        vec![]
    }
}
