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

/// Compact boot-video workload dimensions. The 60-byte scenario below is
/// embedded in the kernel; raw I420 frames are deterministically expanded one
/// at a time so the image does not grow by roughly 23 MiB.
pub const SEQUENCE_WIDTH: u32 = 512;
pub const SEQUENCE_HEIGHT: u32 = 512;
pub const SEQUENCE_FRAME_COUNT: usize = 60;

const SEQUENCE_SCENARIO: [u8; SEQUENCE_FRAME_COUNT] = [
    0, 11, 22, 33, 44, 55, 6, 17, 28, 39, 50, 1, 12, 23, 34, 45, 56, 7, 18, 29, 40, 51, 2, 13, 24,
    35, 46, 57, 8, 19, 30, 41, 52, 3, 14, 25, 36, 47, 58, 9, 20, 31, 42, 53, 4, 15, 26, 37, 48, 59,
    10, 21, 32, 43, 54, 5, 16, 27, 38, 49,
];

const RGB_SPECTRUM: [[u8; 3]; 16] = [
    [255, 255, 255],
    [255, 255, 0],
    [128, 255, 0],
    [0, 255, 0],
    [0, 255, 128],
    [0, 255, 255],
    [0, 128, 255],
    [0, 0, 255],
    [128, 0, 255],
    [255, 0, 255],
    [255, 0, 128],
    [255, 0, 0],
    [255, 128, 0],
    [128, 128, 128],
    [32, 32, 32],
    [0, 0, 0],
];

const LUMA_BAR_VALUES: [u8; 8] = [16, 47, 78, 109, 141, 172, 203, 235];
const CB_BAR_VALUES: [u8; 8] = [128, 90, 166, 54, 202, 72, 184, 128];
const CR_BAR_VALUES: [u8; 8] = [128, 202, 54, 166, 90, 184, 72, 128];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeError {
    Encode,
    InvalidAnnexB,
    IncompleteSequence,
}

impl ProbeError {
    pub const fn code(self) -> i32 {
        match self {
            Self::Encode => -1,
            Self::InvalidAnnexB => -2,
            Self::IncompleteSequence => -3,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SequenceMetrics {
    pub visible_width: u32,
    pub visible_height: u32,
    pub coded_width: u32,
    pub coded_height: u32,
    pub frames: usize,
    pub macroblocks_per_frame: usize,
    pub source_bytes: usize,
    pub encoded_bytes: usize,
    pub sps_bytes: usize,
    pub pps_bytes: usize,
    pub frame_bytes_min: usize,
    pub frame_bytes_max: usize,
    pub source_fnv1a32: u32,
    pub encoded_fnv1a32: u32,
}

pub struct EncodedSequenceProbe {
    pub annex_b: Vec<u8>,
    pub metrics: SequenceMetrics,
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

struct SequenceFrame {
    y: Vec<u8>,
    cb: Vec<u8>,
    cr: Vec<u8>,
}

impl SequenceFrame {
    fn new(frame_index: usize) -> Self {
        let width = SEQUENCE_WIDTH as usize;
        let height = SEQUENCE_HEIGHT as usize;
        let chroma_width = width / 2;
        let chroma_height = height / 2;
        let mut y = vec![16; width * height];
        let mut cb = vec![128; chroma_width * chroma_height];
        let mut cr = vec![128; chroma_width * chroma_height];
        let scenario = usize::from(SEQUENCE_SCENARIO[frame_index % SEQUENCE_FRAME_COUNT]);
        let phase = (scenario + frame_index) % RGB_SPECTRUM.len();
        let motion_x = (scenario * 7 + frame_index * 5) % (width - 64);
        let motion_y = (scenario * 3 + frame_index * 7) % (height - 64);

        // Each chroma sample owns exactly one 2x2 luma cell. This keeps the
        // source valid I420 while exercising color bars, legal-range ramps,
        // high-frequency chroma tiles, hard edges, and temporal motion.
        for chroma_y in 0..chroma_height {
            let pixel_y = chroma_y * 2;
            for chroma_x in 0..chroma_width {
                let pixel_x = chroma_x * 2;
                let in_motion = pixel_x >= motion_x
                    && pixel_x < motion_x + 64
                    && pixel_y >= motion_y
                    && pixel_y < motion_y + 64;
                let rgb = if in_motion {
                    let checker = ((pixel_x - motion_x) / 4 + (pixel_y - motion_y) / 4) & 1;
                    if checker == 0 {
                        RGB_SPECTRUM[(phase + frame_index / 4) % RGB_SPECTRUM.len()]
                    } else {
                        RGB_SPECTRUM[15]
                    }
                } else if pixel_y < 192 {
                    let bar = pixel_x * RGB_SPECTRUM.len() / width;
                    RGB_SPECTRUM[(bar + phase) % RGB_SPECTRUM.len()]
                } else if pixel_y < 320 {
                    let ramp = ((pixel_x * 255) / (width - 1)) as u8;
                    [ramp, ramp, ramp]
                } else {
                    let tile = (pixel_x / 8) + (pixel_y / 8) + phase;
                    RGB_SPECTRUM[tile % RGB_SPECTRUM.len()]
                };
                let (cell_y, cell_cb, cell_cr) = rgb_to_limited_ycbcr(rgb);
                for row in pixel_y..pixel_y + 2 {
                    let row_start = row * width;
                    y[row_start + pixel_x] = cell_y;
                    y[row_start + pixel_x + 1] = cell_y;
                }
                let chroma_offset = chroma_y * chroma_width + chroma_x;
                cb[chroma_offset] = cell_cb;
                cr[chroma_offset] = cell_cr;
            }
        }

        // White/black registration crosshairs are luma-only on purpose: they
        // catch single-pixel positioning errors without making invalid 4:2:0
        // chroma transitions.
        let cross_x = (width / 2 + frame_index) % width;
        let cross_y = (height / 2 + frame_index * 2) % height;
        for row in 0..height {
            y[row * width + cross_x] = if row & 1 == 0 { 235 } else { 16 };
        }
        for col in 0..width {
            y[cross_y * width + col] = if col & 1 == 0 { 235 } else { 16 };
        }

        Self { y, cb, cr }
    }

    fn image(&self) -> YCbCrImage<'_> {
        let luma_stride = SEQUENCE_WIDTH as usize;
        let chroma_stride = (SEQUENCE_WIDTH / 2) as usize;
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
            width: SEQUENCE_WIDTH,
            height: SEQUENCE_HEIGHT,
        }
    }

    fn append_visible_i420(&self, output: &mut Vec<u8>) {
        output.extend_from_slice(self.y.as_slice());
        output.extend_from_slice(self.cb.as_slice());
        output.extend_from_slice(self.cr.as_slice());
    }

    fn byte_len(&self) -> usize {
        self.y.len() + self.cb.len() + self.cr.len()
    }

    fn extend_hash(&self, hash: &mut u32) {
        for plane in [&self.y, &self.cb, &self.cr] {
            fnv1a32_extend(hash, plane.as_slice());
        }
    }
}

fn rgb_to_limited_ycbcr([red, green, blue]: [u8; 3]) -> (u8, u8, u8) {
    let red = i32::from(red);
    let green = i32::from(green);
    let blue = i32::from(blue);
    let y = 16 + ((66 * red + 129 * green + 25 * blue + 128) >> 8);
    let cb = 128 + ((-38 * red - 74 * green + 112 * blue + 128) >> 8);
    let cr = 128 + ((112 * red - 94 * green - 18 * blue + 128) >> 8);
    (y.clamp(16, 235) as u8, cb.clamp(16, 240) as u8, cr.clamp(16, 240) as u8)
}

pub struct DiagnosticSequenceEncoder {
    encoder: LessEncoder,
    annex_b: Vec<u8>,
    next_frame: usize,
    source_bytes: usize,
    source_fnv1a32: u32,
    sps_bytes: usize,
    pps_bytes: usize,
    frame_bytes_min: usize,
    frame_bytes_max: usize,
}

impl DiagnosticSequenceEncoder {
    pub fn new() -> Result<Self, ProbeError> {
        let frame = SequenceFrame::new(0);
        let mut source_fnv1a32 = 0x811c_9dc5u32;
        frame.extend_hash(&mut source_fnv1a32);
        let source_bytes = frame.byte_len();
        let (initial, encoder) =
            LessEncoder::new(&frame.image()).map_err(|_| ProbeError::Encode)?;
        let sps = initial.sps.to_annex_b_data();
        let pps = initial.pps.to_annex_b_data();
        let first = initial.frame.to_annex_b_data();
        let total = sps
            .len()
            .checked_add(pps.len())
            .and_then(|bytes| bytes.checked_add(first.len()))
            .ok_or(ProbeError::Encode)?;
        let mut annex_b = Vec::with_capacity(total.saturating_mul(SEQUENCE_FRAME_COUNT));
        annex_b.extend_from_slice(sps.as_slice());
        annex_b.extend_from_slice(pps.as_slice());
        annex_b.extend_from_slice(first.as_slice());
        Ok(Self {
            encoder,
            annex_b,
            next_frame: 1,
            source_bytes,
            source_fnv1a32,
            sps_bytes: sps.len(),
            pps_bytes: pps.len(),
            frame_bytes_min: first.len(),
            frame_bytes_max: first.len(),
        })
    }

    pub const fn encoded_frames(&self) -> usize {
        self.next_frame
    }

    /// Encode one additional frame. Returns `false` after all 60 frames have
    /// already been encoded, allowing an async caller to yield between steps.
    pub fn encode_next(&mut self) -> Result<bool, ProbeError> {
        if self.next_frame >= SEQUENCE_FRAME_COUNT {
            return Ok(false);
        }
        let frame = SequenceFrame::new(self.next_frame);
        frame.extend_hash(&mut self.source_fnv1a32);
        self.source_bytes = self.source_bytes.saturating_add(frame.byte_len());
        let nal = self
            .encoder
            .encode(&frame.image())
            .map_err(|_| ProbeError::Encode)?;
        let bytes = nal.to_annex_b_data();
        self.frame_bytes_min = self.frame_bytes_min.min(bytes.len());
        self.frame_bytes_max = self.frame_bytes_max.max(bytes.len());
        self.annex_b.extend_from_slice(bytes.as_slice());
        self.next_frame += 1;
        Ok(true)
    }

    pub fn finish(self) -> Result<EncodedSequenceProbe, ProbeError> {
        if self.next_frame != SEQUENCE_FRAME_COUNT {
            return Err(ProbeError::IncompleteSequence);
        }
        if !annex_b_is_sequence(self.annex_b.as_slice(), SEQUENCE_FRAME_COUNT) {
            return Err(ProbeError::InvalidAnnexB);
        }
        let metrics = SequenceMetrics {
            visible_width: SEQUENCE_WIDTH,
            visible_height: SEQUENCE_HEIGHT,
            coded_width: SEQUENCE_WIDTH,
            coded_height: SEQUENCE_HEIGHT,
            frames: SEQUENCE_FRAME_COUNT,
            macroblocks_per_frame: (SEQUENCE_WIDTH as usize / 16) * (SEQUENCE_HEIGHT as usize / 16),
            source_bytes: self.source_bytes,
            encoded_bytes: self.annex_b.len(),
            sps_bytes: self.sps_bytes,
            pps_bytes: self.pps_bytes,
            frame_bytes_min: self.frame_bytes_min,
            frame_bytes_max: self.frame_bytes_max,
            source_fnv1a32: self.source_fnv1a32,
            encoded_fnv1a32: fnv1a32(self.annex_b.as_slice()),
        };
        Ok(EncodedSequenceProbe {
            annex_b: self.annex_b,
            metrics,
        })
    }
}

pub fn diagnostic_sequence_visible_i420() -> Vec<u8> {
    let frame_bytes = SEQUENCE_WIDTH as usize * SEQUENCE_HEIGHT as usize * 3 / 2;
    let mut visible = Vec::with_capacity(frame_bytes * SEQUENCE_FRAME_COUNT);
    for frame_index in 0..SEQUENCE_FRAME_COUNT {
        SequenceFrame::new(frame_index).append_visible_i420(&mut visible);
    }
    visible
}

pub fn encode_diagnostic_sequence_512x512_60() -> Result<EncodedSequenceProbe, ProbeError> {
    let mut encoder = DiagnosticSequenceEncoder::new()?;
    while encoder.encode_next()? {}
    encoder.finish()
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

fn annex_b_is_sequence(bytes: &[u8], frames: usize) -> bool {
    let mut count = 0usize;
    let mut cursor = 0usize;
    while cursor + 5 <= bytes.len() {
        if bytes[cursor..cursor + 4] == [0, 0, 0, 1] {
            let nal_type = bytes[cursor + 4] & 0x1f;
            let expected = match count {
                0 => 7,
                1 => 8,
                _ => 5,
            };
            if nal_type != expected {
                return false;
            }
            count += 1;
            cursor += 5;
        } else {
            cursor += 1;
        }
    }
    count == frames.saturating_add(2)
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
    fnv1a32_extend(&mut hash, bytes);
    hash
}

fn fnv1a32_extend(hash: &mut u32, bytes: &[u8]) {
    for &byte in bytes {
        *hash ^= u32::from(byte);
        *hash = hash.wrapping_mul(0x0100_0193);
    }
}
