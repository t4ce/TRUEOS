#![no_std]
#![deny(unsafe_code)]

//! The six rank-three ONNX `Resize` operations in the pinned Kokoro graph.
//!
//! Batch and channel dimensions are unchanged. The only admitted time-axis
//! transforms are nearest/asymmetric upsampling by 2 or 300 and
//! linear/half-pixel downsampling or upsampling by 300. Encoding the scales as
//! an enum avoids rebuilding dynamic float shape policy inside the kernel.
//! ORT nevertheless routes rank-three linear Resize through its trilinear
//! implementation, so the identity batch/channel axes remain explicit in the
//! arithmetic below.

use core::mem::size_of;

pub const PINNED_NODE_COVERAGE: usize = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResizeMode {
    NearestAsymmetric,
    LinearHalfPixel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResizeScale {
    Up2,
    Up300,
    Down300,
}

impl ResizeScale {
    const fn factor(self) -> usize {
        match self {
            Self::Up2 => 2,
            Self::Up300 | Self::Down300 => 300,
        }
    }

    const fn as_f32(self) -> f32 {
        match self {
            Self::Up2 => 2.0,
            Self::Up300 => 300.0,
            Self::Down300 => 1.0 / 300.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    ZeroDimension,
    ShapeOverflow,
    UnsupportedContract,
    InputLengthNotDivisible,
    BufferLengthMismatch,
    InvalidWorkRange,
    Aliasing,
    NonFiniteInput,
    NonFiniteOutput,
}

/// Fully validated static shape/attribute portion of one resize call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResizePlan {
    batch: usize,
    channels: usize,
    input_len: usize,
    output_len: usize,
    mode: ResizeMode,
    scale: ResizeScale,
    input_elements: usize,
    output_elements: usize,
}

impl ResizePlan {
    pub fn new(
        batch: usize,
        channels: usize,
        input_len: usize,
        mode: ResizeMode,
        scale: ResizeScale,
    ) -> Result<Self, Error> {
        if batch == 0 || channels == 0 || input_len == 0 {
            return Err(Error::ZeroDimension);
        }
        if !matches!(
            (mode, scale),
            (ResizeMode::NearestAsymmetric, ResizeScale::Up2 | ResizeScale::Up300)
                | (ResizeMode::LinearHalfPixel, ResizeScale::Up300 | ResizeScale::Down300)
        ) {
            return Err(Error::UnsupportedContract);
        }
        let output_len = match scale {
            ResizeScale::Up2 | ResizeScale::Up300 => input_len
                .checked_mul(scale.factor())
                .ok_or(Error::ShapeOverflow)?,
            ResizeScale::Down300 => {
                if !input_len.is_multiple_of(scale.factor()) {
                    return Err(Error::InputLengthNotDivisible);
                }
                input_len / scale.factor()
            }
        };
        if output_len == 0 {
            return Err(Error::ZeroDimension);
        }
        let planes = batch.checked_mul(channels).ok_or(Error::ShapeOverflow)?;
        let input_elements = planes.checked_mul(input_len).ok_or(Error::ShapeOverflow)?;
        let output_elements = planes.checked_mul(output_len).ok_or(Error::ShapeOverflow)?;
        Ok(Self {
            batch,
            channels,
            input_len,
            output_len,
            mode,
            scale,
            input_elements,
            output_elements,
        })
    }

    pub const fn batch(self) -> usize {
        self.batch
    }

    pub const fn channels(self) -> usize {
        self.channels
    }

    pub const fn input_len(self) -> usize {
        self.input_len
    }

    pub const fn output_len(self) -> usize {
        self.output_len
    }

    pub const fn input_elements(self) -> usize {
        self.input_elements
    }

    pub const fn output_elements(self) -> usize {
        self.output_elements
    }

    /// Execute the complete resize transactionally.
    pub fn run(self, input: &[f32], output: &mut [f32]) -> Result<(), Error> {
        self.run_range(input, output, 0, self.output_elements)
    }

    /// Execute a scheduler-provided contiguous output-element range.
    ///
    /// Validation and all arithmetic checks for the range happen before its
    /// first destination element is written. Other ranges in the same output
    /// remain untouched.
    pub fn run_range(
        self,
        input: &[f32],
        output: &mut [f32],
        unit_start: usize,
        unit_count: usize,
    ) -> Result<(), Error> {
        if input.len() != self.input_elements || output.len() != self.output_elements {
            return Err(Error::BufferLengthMismatch);
        }
        let unit_end = unit_start
            .checked_add(unit_count)
            .ok_or(Error::InvalidWorkRange)?;
        if unit_count == 0 || unit_end > self.output_elements {
            return Err(Error::InvalidWorkRange);
        }
        if memory_ranges_overlap(output, input) {
            return Err(Error::Aliasing);
        }

        for index in unit_start..unit_end {
            self.sample(input, index)?;
        }
        for (index, destination) in output
            .iter_mut()
            .enumerate()
            .take(unit_end)
            .skip(unit_start)
        {
            *destination = self.sample(input, index)?;
        }
        Ok(())
    }

    fn sample(self, input: &[f32], output_index: usize) -> Result<f32, Error> {
        let plane = output_index / self.output_len;
        let position = output_index % self.output_len;
        let input_base = plane * self.input_len;
        let value = match self.mode {
            ResizeMode::NearestAsymmetric => {
                let source = position / self.scale.factor();
                finite_input(input[input_base + source])?
            }
            ResizeMode::LinearHalfPixel => self.linear_trilinear_sample(input, plane, position)?,
        };
        if value.is_finite() {
            Ok(value)
        } else {
            Err(Error::NonFiniteOutput)
        }
    }

    fn linear_trilinear_sample(
        self,
        input: &[f32],
        plane: usize,
        position: usize,
    ) -> Result<f32, Error> {
        // ORT treats a rank-three [B,C,L] input as [D,H,W] and executes the
        // literal trilinear expression even though the B and C scales are 1.
        // At the final coordinate of either identity axis its two indices are
        // equal and ORT assigns both distances 0.5, duplicating terms. Preserve
        // the exact eight-term source and evaluation order: cancellation can
        // otherwise change the result by one ULP before a later quantizer.
        let batch = plane / self.channels;
        let channel = plane % self.channels;
        let original = (position as f32 + 0.5) / self.scale.as_f32() - 0.5;
        let z = linear_axis(batch as f32, self.batch);
        let y = linear_axis(channel as f32, self.channels);
        let x = linear_axis(original, self.input_len);

        let offset = |batch: usize, channel: usize, position: usize| {
            (batch * self.channels + channel) * self.input_len + position
        };
        let x111 = finite_input(input[offset(z.lower, y.lower, x.lower)])?;
        let x211 = finite_input(input[offset(z.lower, y.lower, x.upper)])?;
        let x121 = finite_input(input[offset(z.lower, y.upper, x.lower)])?;
        let x221 = finite_input(input[offset(z.lower, y.upper, x.upper)])?;
        let x112 = finite_input(input[offset(z.upper, y.lower, x.lower)])?;
        let x212 = finite_input(input[offset(z.upper, y.lower, x.upper)])?;
        let x122 = finite_input(input[offset(z.upper, y.upper, x.lower)])?;
        let x222 = finite_input(input[offset(z.upper, y.upper, x.upper)])?;

        let term111 = x.to_upper * y.to_upper * z.to_upper * x111;
        let term211 = x.to_lower * y.to_upper * z.to_upper * x211;
        let term121 = x.to_upper * y.to_lower * z.to_upper * x121;
        let term221 = x.to_lower * y.to_lower * z.to_upper * x221;
        let term112 = x.to_upper * y.to_upper * z.to_lower * x112;
        let term212 = x.to_lower * y.to_upper * z.to_lower * x212;
        let term122 = x.to_upper * y.to_lower * z.to_lower * x122;
        let term222 = x.to_lower * y.to_lower * z.to_lower * x222;
        Ok(term111 + term211 + term121 + term221 + term112 + term212 + term122 + term222)
    }
}

#[derive(Clone, Copy)]
struct LinearAxis {
    lower: usize,
    upper: usize,
    // ORT names these distances d*1 and d*2. The distance to the upper
    // coordinate weights the lower sample and vice versa.
    to_lower: f32,
    to_upper: f32,
}

fn linear_axis(original: f32, length: usize) -> LinearAxis {
    let last = length - 1;
    let clamped = original.min(last as f32).max(0.0);
    let lower = (clamped as usize).min(last);
    let upper = if lower == last { last } else { lower + 1 };
    let mut to_lower = (clamped - lower as f32).abs();
    let mut to_upper = (clamped - upper as f32).abs();
    if lower == upper {
        to_lower = 0.5;
        to_upper = 0.5;
    }
    LinearAxis {
        lower,
        upper,
        to_lower,
        to_upper,
    }
}

fn finite_input(value: f32) -> Result<f32, Error> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Error::NonFiniteInput)
    }
}

fn memory_ranges_overlap(lhs: &[f32], rhs: &[f32]) -> bool {
    let lhs_start = lhs.as_ptr() as usize;
    let rhs_start = rhs.as_ptr() as usize;
    let lhs_end = lhs_start.saturating_add(lhs.len().saturating_mul(size_of::<f32>()));
    let rhs_end = rhs_start.saturating_add(rhs.len().saturating_mul(size_of::<f32>()));
    lhs_start < rhs_end && rhs_start < lhs_end
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_ort_1_27_nearest_fixtures_match() {
        let input = [1.0_f32, -2.0, 3.5, 9.0];
        let plan =
            ResizePlan::new(1, 1, input.len(), ResizeMode::NearestAsymmetric, ResizeScale::Up2)
                .unwrap();
        let mut output = [0.0_f32; 8];
        plan.run(&input, &mut output).unwrap();
        assert_eq!(output, [1.0, 1.0, -2.0, -2.0, 3.5, 3.5, 9.0, 9.0]);

        let plan =
            ResizePlan::new(1, 1, 2, ResizeMode::NearestAsymmetric, ResizeScale::Up300).unwrap();
        let mut output = [0.0_f32; 600];
        plan.run(&input[..2], &mut output).unwrap();
        assert!(output[..300].iter().all(|&value| value == 1.0));
        assert!(output[300..].iter().all(|&value| value == -2.0));
    }

    #[test]
    fn stable_ort_1_27_linear_fixtures_match_bitwise() {
        let mut down_input = [0.0_f32; 600];
        for (index, value) in down_input.iter_mut().enumerate() {
            *value = (((index * 17) % 101) as i32 - 50) as f32 / 13.0;
        }
        let down =
            ResizePlan::new(1, 1, 600, ResizeMode::LinearHalfPixel, ResizeScale::Down300).unwrap();
        let mut down_output = [0.0_f32; 2];
        down.run(&down_input, &mut down_output).unwrap();
        assert_eq!(down_output.map(f32::to_bits), [0xc024_ec4e, 0x3fa2_7627]);

        let up = ResizePlan::new(1, 1, 2, ResizeMode::LinearHalfPixel, ResizeScale::Up300).unwrap();
        let mut up_output = [0.0_f32; 600];
        up.run(&[1.0, -2.0], &mut up_output).unwrap();
        assert_eq!(up_output[0].to_bits(), 0x3f80_0000);
        assert_eq!(up_output[300].to_bits(), 0xbf01_47ae);
        assert_eq!(up_output[599].to_bits(), 0xc000_0000);
    }

    #[test]
    fn linear_rank_three_preserves_ort_trilinear_boundary_order() {
        let plan =
            ResizePlan::new(1, 2, 2, ResizeMode::LinearHalfPixel, ResizeScale::Up300).unwrap();
        let input = [1.0_f32, -2.0, 1.0, -2.0];
        let mut output = [0.0_f32; 1_200];
        plan.run(&input, &mut output).unwrap();

        // Channel 0 has distinct y1/y2 indices and the zero y2 weights select
        // its own samples. At the final channel ORT clamps y2 to y1, changes
        // both y weights to 0.5, and evaluates all eight terms literally.
        // Collapsing either case to a one-dimensional lerp changes these bits.
        assert_eq!(output[150].to_bits(), 0x3F7E_B852);
        assert_eq!(output[600 + 150].to_bits(), 0x3F7E_B851);
        assert_eq!(output[151].to_bits(), 0x3F7C_28F6);
        assert_eq!(output[600 + 151].to_bits(), 0x3F7C_28F7);
    }

    #[test]
    fn work_ranges_are_cooperative_and_transactional() {
        let plan =
            ResizePlan::new(1, 1, 2, ResizeMode::NearestAsymmetric, ResizeScale::Up2).unwrap();
        let input = [2.0_f32, 4.0];
        let mut output = [99.0_f32; 4];
        plan.run_range(&input, &mut output, 1, 2).unwrap();
        assert_eq!(output, [99.0, 2.0, 4.0, 99.0]);

        let bad = [2.0_f32, f32::NAN];
        let snapshot = output;
        assert_eq!(plan.run_range(&bad, &mut output, 2, 2), Err(Error::NonFiniteInput));
        assert_eq!(output, snapshot);
    }

    #[test]
    fn changed_graph_contracts_fail_closed() {
        assert_eq!(
            ResizePlan::new(1, 1, 7, ResizeMode::NearestAsymmetric, ResizeScale::Down300,),
            Err(Error::UnsupportedContract)
        );
        assert_eq!(
            ResizePlan::new(1, 1, 301, ResizeMode::LinearHalfPixel, ResizeScale::Down300,),
            Err(Error::InputLengthNotDivisible)
        );
    }
}
