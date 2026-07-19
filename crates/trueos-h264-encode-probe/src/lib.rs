#![no_std]

extern crate alloc;

use alloc::{vec, vec::Vec};

use less_avc::{
    BitDepth, LessEncoder,
    ycbcr_image::{DataPlane, Planes, YCbCrImage},
};

pub const PROBE_WIDTH: u32 = 1920;
pub const PROBE_HEIGHT: u32 = 1080;
pub const PROBE_CODED_WIDTH: u32 = 1920;
pub const PROBE_CODED_HEIGHT: u32 = 1088;

const LUMA_BAR_VALUES: [u8; 8] = [16, 47, 78, 109, 141, 172, 203, 235];
const CB_BAR_VALUES: [u8; 8] = [128, 90, 166, 54, 202, 72, 184, 128];
const CR_BAR_VALUES: [u8; 8] = [128, 202, 54, 166, 90, 184, 72, 128];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeError {
    Encode,
    InvalidAnnexB,
}

impl ProbeError {
    pub const fn code(self) -> i32 {
        match self {
            Self::Encode => -1,
            Self::InvalidAnnexB => -2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProbeMetrics {
    pub visible_width: u32,
    pub visible_height: u32,
    pub coded_width: u32,
    pub coded_height: u32,
    pub macroblocks: usize,
    pub source_bytes: usize,
    pub encoded_bytes: usize,
    pub sps_bytes: usize,
    pub pps_bytes: usize,
    pub idr_bytes: usize,
    pub source_fnv1a32: u32,
    pub encoded_fnv1a32: u32,
}

pub struct EncodedProbe {
    pub annex_b: Vec<u8>,
    pub metrics: ProbeMetrics,
}

struct DiagnosticFrame {
    y: Vec<u8>,
    cb: Vec<u8>,
    cr: Vec<u8>,
}

impl DiagnosticFrame {
    fn new() -> Self {
        let luma_stride = PROBE_CODED_WIDTH as usize;
        let chroma_stride = (PROBE_CODED_WIDTH / 2) as usize;
        let mut y = vec![16; luma_stride * PROBE_CODED_HEIGHT as usize];
        let mut cb = vec![128; chroma_stride * (PROBE_CODED_HEIGHT / 2) as usize];
        let mut cr = vec![128; chroma_stride * (PROBE_CODED_HEIGHT / 2) as usize];

        for row in 0..PROBE_HEIGHT as usize {
            let row_start = row * luma_stride;
            for col in 0..PROBE_WIDTH as usize {
                let bar = col * LUMA_BAR_VALUES.len() / PROBE_WIDTH as usize;
                let grid = col.is_multiple_of(120) || row.is_multiple_of(68);
                y[row_start + col] = if grid { 235 } else { LUMA_BAR_VALUES[bar] };
            }
        }

        let visible_chroma_width = (PROBE_WIDTH / 2) as usize;
        let visible_chroma_height = (PROBE_HEIGHT / 2) as usize;
        for row in 0..visible_chroma_height {
            let row_start = row * chroma_stride;
            for col in 0..visible_chroma_width {
                let bar = col * CB_BAR_VALUES.len() / visible_chroma_width;
                cb[row_start + col] = CB_BAR_VALUES[bar];
                cr[row_start + col] = CR_BAR_VALUES[bar];
            }
        }

        Self { y, cb, cr }
    }

    fn image(&self) -> YCbCrImage<'_> {
        let luma_stride = PROBE_CODED_WIDTH as usize;
        let chroma_stride = (PROBE_CODED_WIDTH / 2) as usize;
        YCbCrImage {
            planes: Planes::YCbCr((
                DataPlane {
                    data: self.y.as_slice(),
                    stride: luma_stride,
                    bit_depth: BitDepth::Depth8,
                },
                DataPlane {
                    data: self.cb.as_slice(),
                    stride: chroma_stride,
                    bit_depth: BitDepth::Depth8,
                },
                DataPlane {
                    data: self.cr.as_slice(),
                    stride: chroma_stride,
                    bit_depth: BitDepth::Depth8,
                },
            )),
            width: PROBE_WIDTH,
            height: PROBE_HEIGHT,
        }
    }

    fn visible_i420(&self) -> Vec<u8> {
        let width = PROBE_WIDTH as usize;
        let height = PROBE_HEIGHT as usize;
        let coded_width = PROBE_CODED_WIDTH as usize;
        let chroma_width = width / 2;
        let chroma_height = height / 2;
        let coded_chroma_width = coded_width / 2;
        let mut visible = Vec::with_capacity(width * height * 3 / 2);
        for row in 0..height {
            visible.extend_from_slice(&self.y[row * coded_width..row * coded_width + width]);
        }
        for plane in [&self.cb, &self.cr] {
            for row in 0..chroma_height {
                visible.extend_from_slice(
                    &plane[row * coded_chroma_width..row * coded_chroma_width + chroma_width],
                );
            }
        }
        visible
    }
}

pub fn diagnostic_visible_i420() -> Vec<u8> {
    DiagnosticFrame::new().visible_i420()
}

pub fn encode_full_hd_diagnostic_idr() -> Result<EncodedProbe, ProbeError> {
    let frame = DiagnosticFrame::new();
    let source_fnv1a32 = fnv1a32_planes([&frame.y, &frame.cb, &frame.cr]);
    let (initial, _encoder) = LessEncoder::new(&frame.image()).map_err(|_| ProbeError::Encode)?;

    let sps = initial.sps.to_annex_b_data();
    let pps = initial.pps.to_annex_b_data();
    let idr = initial.frame.to_annex_b_data();
    let expected_bytes = sps
        .len()
        .checked_add(pps.len())
        .and_then(|bytes| bytes.checked_add(idr.len()))
        .ok_or(ProbeError::Encode)?;
    let mut annex_b = Vec::with_capacity(expected_bytes);
    annex_b.extend_from_slice(sps.as_slice());
    annex_b.extend_from_slice(pps.as_slice());
    annex_b.extend_from_slice(idr.as_slice());

    if annex_b.len() != expected_bytes || annex_b_nal_types(annex_b.as_slice()) != Some([7, 8, 5]) {
        return Err(ProbeError::InvalidAnnexB);
    }

    let source_bytes = frame.y.len() + frame.cb.len() + frame.cr.len();
    Ok(EncodedProbe {
        metrics: ProbeMetrics {
            visible_width: PROBE_WIDTH,
            visible_height: PROBE_HEIGHT,
            coded_width: PROBE_CODED_WIDTH,
            coded_height: PROBE_CODED_HEIGHT,
            macroblocks: (PROBE_CODED_WIDTH as usize / 16) * (PROBE_CODED_HEIGHT as usize / 16),
            source_bytes,
            encoded_bytes: annex_b.len(),
            sps_bytes: sps.len(),
            pps_bytes: pps.len(),
            idr_bytes: idr.len(),
            source_fnv1a32,
            encoded_fnv1a32: fnv1a32(annex_b.as_slice()),
        },
        annex_b,
    })
}

fn annex_b_nal_types(bytes: &[u8]) -> Option<[u8; 3]> {
    let mut types = [0u8; 3];
    let mut count = 0usize;
    let mut cursor = 0usize;
    while cursor + 5 <= bytes.len() {
        if bytes[cursor..cursor + 4] == [0, 0, 0, 1] {
            if count >= types.len() {
                return None;
            }
            types[count] = bytes[cursor + 4] & 0x1f;
            count += 1;
            cursor += 5;
        } else {
            cursor += 1;
        }
    }
    (count == types.len()).then_some(types)
}

fn fnv1a32_planes(planes: [&Vec<u8>; 3]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for plane in planes {
        for &byte in plane {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(0x0100_0193);
        }
    }
    hash
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for &byte in bytes {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}
