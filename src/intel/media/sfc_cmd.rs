//! Gen12 single-pipe MFX-to-SFC commands for coded-size, linear RGBA8 output.
//!
//! Packet layout follows Intel's Gen12 `mhw_sfc_hwcmd_g12_X.h` and command
//! order follows `CodechalAvcSfcStateG12::AddSfcCommands` (media-driver 3043e7c).
//! Scaling and its coefficient tables are outside this CSC probe.

const STATE_DWORDS: usize = 50;
const IEF_DWORDS: usize = 24;
const SFC_CMD: u32 = 0x7500_0000;
const RGBA8_FORMAT: u32 = 1;
const G12_MOCS_UC_INDEX: u32 = 1;
const PAGE_BYTES: usize = 4096;

#[derive(Copy, Clone)]
pub(super) struct Geometry {
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub rgba_bytes: usize,
    pub avs_bytes: usize,
}

impl Geometry {
    pub(super) fn new(width: u32, height: u32) -> Option<Self> {
        // Keep first bring-up inside the HD single-pipe envelope.
        if !(128..=1920).contains(&width)
            || !(128..=1088).contains(&height)
            || width % 16 != 0
            || height % 16 != 0
        {
            return None;
        }
        let (width, height) = (width as usize, height as usize);
        let pitch = (width * 4).next_multiple_of(64);
        Some(Self {
            width,
            height,
            pitch,
            rgba_bytes: (pitch * height).next_multiple_of(PAGE_BYTES),
            // Gen12 AVC 5x5 filter path reserves three cache lines per 8 pixels.
            avs_bytes: (width.div_ceil(8) * 3 * 64).next_multiple_of(PAGE_BYTES),
        })
    }
}

#[derive(Copy, Clone)]
pub(super) struct Csc {
    // Q8 coefficients shared with the reference conversion. SFC uses Q10.
    coefficients: [i32; 9],
    offsets: [i32; 3],
}

impl Csc {
    pub(super) fn new(full_range: bool, matrix: u8) -> Self {
        let [rv, gu, gv, bu] = match (full_range, matrix == 1) {
            (false, false) => [409, -100, -208, 516],
            (false, true) => [459, -55, -136, 541],
            (true, false) => [359, -88, -183, 454],
            (true, true) => [403, -48, -120, 475],
        };
        let y = if full_range { 256 } else { 298 };
        Self {
            coefficients: [y, 0, rv, y, gu, gv, y, bu, 0],
            offsets: [if full_range { 0 } else { -16 }, -128, -128],
        }
    }

    pub(super) fn reference_rgb(self, y: u8, u: u8, v: u8) -> [u8; 3] {
        let y = (i32::from(y) + self.offsets[0]).max(0);
        let u = i32::from(u) + self.offsets[1];
        let v = i32::from(v) + self.offsets[2];
        core::array::from_fn(|i| {
            let [cy, cu, cv] = self.coefficients[i * 3..i * 3 + 3].try_into().unwrap();
            ((cy * y + cu * u + cv * v + 128) >> 8).clamp(0, 255) as u8
        })
    }

    fn ief_state(self) -> [u32; IEF_DWORDS] {
        let mut words = [0; IEF_DWORDS];
        words[0] = SFC_CMD | (3 << 16) | (IEF_DWORDS as u32 - 2);
        let c = self.coefficients.map(|x| (x * 4) as u32 & 0x1fff);
        // CSC on, IEF off. Input offsets are in units of 1/4 input sample.
        words[16] = 1 | (c[0] << 3) | (c[1] << 16);
        words[17] = c[2] | (c[3] << 13);
        words[18] = c[4] | (c[5] << 13);
        words[19] = c[6] | (c[7] << 13);
        words[20] = c[8];
        for (word, offset) in words[21..24].iter_mut().zip(self.offsets) {
            *word = (offset * 4) as u32 & 0x7ff;
        }
        words
    }
}

pub(super) const COMMAND_DWORDS: usize = 2 + STATE_DWORDS + IEF_DWORDS + 2;

pub(super) fn encode(
    geometry: Geometry,
    target: u64,
    avs_line: u64,
    csc: Csc,
) -> Option<[u32; COMMAND_DWORDS]> {
    let target_end = target.checked_add(geometry.rgba_bytes as u64)?;
    let avs_end = avs_line.checked_add(geometry.avs_bytes as u64)?;
    if target == 0
        || avs_line == 0
        || !(target | avs_line).is_multiple_of(PAGE_BYTES as u64)
        || target_end.max(avs_end) > (1 << 48)
        || (target < avs_end && avs_line < target_end)
    {
        return None;
    }

    let mut words = [0; COMMAND_DWORDS];
    // Request the SFC and leave pre-scaled NV12 output enabled for the DPB.
    words[..2].copy_from_slice(&[SFC_CMD, 1 << 1]);
    let state = &mut words[2..2 + STATE_DWORDS];
    state[0] = SFC_CMD | (1 << 16) | (STATE_DWORDS as u32 - 2);
    state[1] = (1 << 4) | (1 << 8); // 4:2:0, AVC post-deblock ordering
    let extent = (geometry.width as u32 - 1) | ((geometry.height as u32 - 1) << 16);
    state[2] = extent;
    state[3] = RGBA8_FORMAT;
    state[4] = (1 << 8) | (1 << 9) | (1 << 19); // bypass adaptive filters, enable CSC
    state[5] = extent;
    state[7] = extent;
    state[8] = extent;
    state[13] = 1023; // opaque alpha
    state[14] = 1 << 24; // 1.0 in U4.19 at bit 5
    state[15] = 1 << 24;
    for (index, address) in [(17, target), (20, avs_line)] {
        state[index] = address as u32;
        state[index + 1] = (address >> 32) as u32;
        state[index + 2] = G12_MOCS_UC_INDEX << 1;
    }
    state[29] = (RGBA8_FORMAT << 28) | ((geometry.pitch as u32 - 1) << 3);
    words[2 + STATE_DWORDS..COMMAND_DWORDS - 2].copy_from_slice(&csc.ief_state());
    words[COMMAND_DWORDS - 2] = SFC_CMD | (4 << 16);
    Some(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen12_coded_size_csc_uses_four_packets_and_distinct_surfaces() {
        let geometry = Geometry::new(1280, 720).unwrap();
        let target = 0x1_0000_0000;
        let avs = target + geometry.rgba_bytes as u64;
        let csc = Csc::new(false, 2);
        assert!(encode(geometry, target, target + 4096, csc).is_none());
        assert!(encode(geometry, target + 1, avs, csc).is_none());
        let words = encode(geometry, target, avs, csc).unwrap();
        assert_eq!(words.len(), 78);
        assert_eq!(&words[..4], &[0x7500_0000, 2, 0x7501_0030, 0x110]);
        assert_eq!(words[2 + 18], 1);
        assert_eq!(words[2 + 20], geometry.rgba_bytes as u32);
        assert_eq!(words[52], 0x7503_0016);
        assert_eq!(words[76], 0x7504_0000);
        assert_eq!(csc.reference_rgb(16, 128, 128), [0, 0, 0]);
        assert_eq!(csc.reference_rgb(235, 128, 128), [255, 255, 255]);
    }
}
