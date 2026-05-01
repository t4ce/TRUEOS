use crate::autograd::Tensor;
use crate::module::Module;
use crate::ops::convolution::conv2d;
use crate::precision::DType;
use ndarray::Array;
use ndarray_rand::RandomExt;
use ndarray_rand::rand_distr::Normal;

pub struct Conv2D {
    pub weight: Tensor,
    pub bias: Option<Tensor>,
    pub stride: usize,
    pub padding: usize,
}

impl Conv2D {
    pub fn new(
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        padding: usize,
    ) -> Self {
        let fan_in = in_channels * kernel_size * kernel_size;
        let std_dev = (2.0f32 / fan_in as f32).sqrt();
        let dist = Normal::new(0.0f32, std_dev).unwrap();
        let w_data =
            Array::random((out_channels, in_channels, kernel_size, kernel_size), dist).into_dyn();
        let b_data = Array::zeros((out_channels,)).into_dyn();
        Conv2D {
            weight: Tensor::parameter(w_data),
            bias: Some(Tensor::parameter(b_data)),
            stride,
            padding,
        }
    }

    pub fn new_with_dtype(
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        padding: usize,
        dtype: DType,
    ) -> Self {
        // Kaiming Init (f32)
        let fan_in = in_channels * kernel_size * kernel_size;
        let std_dev = (2.0f32 / fan_in as f32).sqrt();
        let dist = Normal::new(0.0f32, std_dev).unwrap();
        let w_data =
            Array::random((out_channels, in_channels, kernel_size, kernel_size), dist).into_dyn();
        let b_data = Array::zeros((out_channels,)).into_dyn();
        Conv2D {
            weight: Tensor::parameter_with_dtype(w_data, dtype),
            bias: Some(Tensor::parameter_with_dtype(b_data, dtype)),
            stride,
            padding,
        }
    }
}

impl Module for Conv2D {
    fn forward(&self, input: Tensor) -> Tensor {
        conv2d(
            &input,
            &self.weight,
            self.bias.as_ref(),
            (self.stride, self.stride),
            (self.padding, self.padding),
        )
    }
    fn parameters(&self) -> Vec<Tensor> {
        let mut params = vec![self.weight.clone()];
        if let Some(b) = &self.bias {
            params.push(b.clone());
        }
        params
    }
}
