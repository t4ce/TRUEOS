#![allow(dead_code)]

use super::*;

const MANDELBROT16_T11_MODE_LINEAR_FULL_BAND: u32 = 46;
const MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND: u32 = 47;
const MANDELBROT16_T16_MODE_LINEAR_CONSTANT_STORE: u32 = 48;
const MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE: u32 = 49;
const MANDELBROT16_T18_MODE_IMMEDIATE_GRADIENT_STORE: u32 = 50;
const MANDELBROT16_T19_MODE_IMMEDIATE_RAW_RADIUS_STORE: u32 = 51;
const MANDELBROT16_T30_MODE_FULLSCREEN_LINEAR_CONSTANT_STORE: u32 = 60;
const MANDELBROT16_T32_MODE_IMMEDIATE_CONSTANT_SINGLE_SEND: u32 = 61;
const MANDELBROT16_T33_MODE_IMMEDIATE_CONSTANT_BTI1_UNTYPED: u32 = 62;
const MANDELBROT16_T34_MODE_IMMEDIATE_ADDRESS_DATA_WITNESS: u32 = 63;
const MANDELBROT16_T35_MODE_IMMEDIATE_EXPLICIT_WIDE_PAYLOAD: u32 = 64;
const MANDELBROT16_T36_MODE_IMMEDIATE_UNROLLED_SCALAR16: u32 = 65;
const MANDELBROT16_T37_MODE_GROUPID_X_UNROLLED_SCALAR16: u32 = 66;
const MANDELBROT16_T38_MODE_IMMEDIATE_WIDE_STAMP: u32 = 67;
const MANDELBROT16_T39_MODE_IMMEDIATE_WIDE_STAMP_ADDRESS_COLOR: u32 = 68;
const MANDELBROT16_T38_STAMP_REPEATS: u32 = 5;
const MANDELBROT16_T13_Q12_X_STEP: u32 = 5;
const MANDELBROT16_CONSTANT_BODY_UPLOAD_DEBUG: bool = false;

#[derive(Clone, Copy, Eq, PartialEq)]
enum Mandelbrot16AddressMode {
    ImmediateBase,
    GroupIdRowPitch,
    GroupIdLinear64,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Mandelbrot16ConstantStoreVariant {
    LegacyStateless,
    SingleSend,
    Bti1Untyped,
    AddressDataWitness,
    ExplicitWidePayload,
    UnrolledScalar16,
    WideScalar16x5,
    WideScalar16x5AddressColor,
}

impl Mandelbrot16AddressMode {
    const fn label(self) -> &'static str {
        match self {
            Self::ImmediateBase => "immediate-base",
            Self::GroupIdRowPitch => "groupid-row-pitch",
            Self::GroupIdLinear64 => "groupid-linear64",
        }
    }
}

include!("mandelbrot/programs_and_patches.rs");
include!("mandelbrot/simd16_submits.rs");
