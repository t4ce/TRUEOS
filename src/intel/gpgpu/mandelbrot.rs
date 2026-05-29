#![allow(dead_code)]

use super::*;

const MANDELBROT_ORACLE_LATEST_HANDLE9_BATCH_BYTES: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/intel_userland_oracle/latest/dumps/000534_pre_exec_handle_9_off_0x2000_len_0x2000.bin",
);
const MANDELBROT_ORACLE_LATEST_HANDLE9_COMPLETION_MARKER: u32 = 0xC0DE_7732;
static MANDELBROT_ORACLE_LATEST_HANDLE9_BATCH_LOGGED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
enum MandelbrotCommandStreamSource {
    DynamicEncoded,
    OracleLatestHandle9Batch,
}

const MANDELBROT_COMMAND_STREAM_SOURCE: MandelbrotCommandStreamSource =
    MandelbrotCommandStreamSource::DynamicEncoded;
// This toggles the compute-walker SIMD mask only. The EU artifact below remains
// the existing scalar groupid row writer; this is not a SIMD16 Mandelbrot body.
const MANDELBROT_GROUPID_LINE1280_SIMD16_MASK_PROBE: bool = true;
const MANDELBROT_GROUPID_LINE1280_SIMD16_PROGRAM_NAME: &str = "gfx12-primary-scanout-groupid-line1280-rows-simd16-walker-mask-existing-scalar-row-writer-hdc1-stateless-unrolled-store-then-ts-eot";
const MANDELBROT_GROUPID_LINE1280_ARTIFACT_BODY: &str = "row-writer-scalar-bw-v1";
const MANDELBROT_GROUPID_LINE1280_PAYLOAD_CONTRACT: &str = "row-color-burst-v1";
const MANDELBROT_GROUPID_LINE1280_SIMD16_DISPATCH_CONTRACT: &str = "simd16-mask-walker-v1";
const MANDELBROT_GROUPID_LINE1280_SIMD8_DISPATCH_CONTRACT: &str = "simd8-mask-walker-v1";
const MANDELBROT_GROUPID_LINE1280_SIMD16_PROVES: &str = "simd16-walker-dispatch-over-row-writer";
const MANDELBROT_GROUPID_LINE1280_SIMD8_PROVES: &str = "simd8-walker-dispatch-over-row-writer";
const MANDELBROT_GROUPID_LINE1280_DOES_NOT_PROVE: &str = "simd16-mandelbrot-eu-body";
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
include!("mandelbrot/line_and_row_pilots.rs");
include!("mandelbrot/simd16_submits.rs");
include!("mandelbrot/preview.rs");
