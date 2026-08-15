//! Gen12 AVC/VDEnc command, coded-output, and surface-upload executor.
//!
//! Boot proves the fixed-CQP IDR graph with one procedural NV12 frame. The
//! resident UI4 service then uses a 40-picture IDR-P GOP with ping-pong recon
//! and 4x reference surfaces for live 2560x1440 NV12 frames. Every submission
//! stores authoritative MFC result registers and validates its Annex-B access
//! unit before it can be handed to the network transport.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, Ordering};

use embassy_time::{Duration, Timer};
use spin::Mutex;

use super::engine as media;

// Keep this fixed diagnostic window above render/UI producer allocations
// (which end at 0x4000_0000) and below display's direct-scanout aliases.
const RING_GPU: u64 = 0x4100_0000;
const CONTEXT_GPU: u64 = 0x4101_0000;
const ARENA_GPU: u64 = 0x4110_0000;
const RING_BYTES: usize = 16 * 1024;
const CONTEXT_BYTES: usize = 22 * 4096;
const ARENA_BYTES: usize = 32 * 1024 * 1024;

const BATCH_OFFSET: usize = 0x0000_0000;
const BATCH_BYTES: usize = 64 * 1024;
const PRIMARY_BATCH_BYTES: usize = 4096;
const CODEC_BATCH_OFFSET: usize = PRIMARY_BATCH_BYTES;
const CODEC_BATCH_BYTES: usize = BATCH_BYTES - CODEC_BATCH_OFFSET;
const RESULT_OFFSET: usize = 0x0001_0000;
const RESULT_BYTES: usize = 4096;
const SOURCE_OFFSET: usize = 0x0002_0000;
pub(crate) const FRAME_WIDTH: usize = 2560;
pub(crate) const FRAME_HEIGHT: usize = 1440;
const FRAME_WIDTH_MBS: usize = FRAME_WIDTH / 16;
const FRAME_HEIGHT_MBS: usize = FRAME_HEIGHT / 16;
const FRAME_MACROBLOCKS: usize = FRAME_WIDTH_MBS * FRAME_HEIGHT_MBS;
const SOURCE_BYTES: usize = FRAME_WIDTH * FRAME_HEIGHT * 3 / 2;
const RECON_0_OFFSET: usize = 0x0058_0000;
const RECON_1_OFFSET: usize = 0x00af_0000;
const RECON_BYTES: usize = SOURCE_BYTES;
const DS_WIDTH: usize = FRAME_WIDTH / 4;
const DS_LOGICAL_HEIGHT: usize = FRAME_HEIGHT / 4;
// Intel's AVC VDEnc path allocates the 4x surface as field-safe Tile-Y
// geometry even for a progressive frame: ceil(ceil(360 / 16) / 2) * 16,
// aligned to 32 rows, then doubled.
const DS_HEIGHT: usize = 384;
const DS_PITCH: usize = 640;
const DS_0_OFFSET: usize = 0x0104_0000;
const DS_1_OFFSET: usize = 0x010a_0000;
const DS_BYTES: usize = DS_PITCH * DS_HEIGHT * 3 / 2;
const BITSTREAM_OFFSET: usize = 0x0110_0000;
const BITSTREAM_BYTES: usize = 4 * 1024 * 1024;
const MFX_STATS_OFFSET: usize = 0x0150_0000;
const VDENC_STATS_OFFSET: usize = 0x0151_0000;
const SLICE_SIZE_OFFSET: usize = 0x0152_0000;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const INTRA_ROWSTORE_OFFSET: usize = 0x0153_0000;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const DEBLOCK_ROWSTORE_OFFSET: usize = 0x0154_0000;
const BSP_ROWSTORE_OFFSET: usize = 0x0155_0000;
const MV_OBJECT_OFFSET: usize = 0x0156_0000;
// Gen12 AVC allocates two page-aligned field MV regions at 128 bytes per
// macroblock. The progressive frame still uses the field-safe allocation.
const FIELD_MACROBLOCKS: usize = FRAME_WIDTH_MBS * FRAME_HEIGHT_MBS.div_ceil(2);
const MV_OBJECT_FIELD_BYTES: usize = (FIELD_MACROBLOCKS * 128).next_multiple_of(4096);
const MV_OBJECT_BYTES: usize = MV_OBJECT_FIELD_BYTES * 2;
const DMV_0_OFFSET: usize = 0x0173_0000;
const DMV_1_OFFSET: usize = 0x0182_0000;
const MPR_ROWSTORE_OFFSET: usize = 0x0191_0000;
// AVC direct-MV storage is 64 bytes per macroblock, with an even MB height.
const DMV_BYTES: usize = FRAME_WIDTH_MBS * FRAME_HEIGHT_MBS.next_multiple_of(2) * 64;
const SCRATCH_BYTES: usize = 64 * 1024;
const ARENA_WORK_RANGES: [(usize, usize); 12] = [
    (BATCH_OFFSET, BATCH_BYTES),
    (RESULT_OFFSET, RESULT_BYTES),
    (RECON_0_OFFSET, RECON_BYTES),
    (RECON_1_OFFSET, RECON_BYTES),
    (DS_0_OFFSET, DS_BYTES),
    (DS_1_OFFSET, DS_BYTES),
    (BITSTREAM_OFFSET, BITSTREAM_BYTES),
    (MFX_STATS_OFFSET, BSP_ROWSTORE_OFFSET + SCRATCH_BYTES - MFX_STATS_OFFSET),
    (MV_OBJECT_OFFSET, MV_OBJECT_BYTES),
    (DMV_0_OFFSET, DMV_BYTES),
    (DMV_1_OFFSET, DMV_BYTES),
    (MPR_ROWSTORE_OFFSET, SCRATCH_BYTES),
];

const BATCH_GPU: u64 = ARENA_GPU + BATCH_OFFSET as u64;
const CODEC_BATCH_GPU: u64 = BATCH_GPU + CODEC_BATCH_OFFSET as u64;
const RESULT_GPU: u64 = ARENA_GPU + RESULT_OFFSET as u64;
const SOURCE_GPU: u64 = ARENA_GPU + SOURCE_OFFSET as u64;
const RECON_0_GPU: u64 = ARENA_GPU + RECON_0_OFFSET as u64;
const RECON_1_GPU: u64 = ARENA_GPU + RECON_1_OFFSET as u64;
const DS_0_GPU: u64 = ARENA_GPU + DS_0_OFFSET as u64;
const DS_1_GPU: u64 = ARENA_GPU + DS_1_OFFSET as u64;
const BITSTREAM_GPU: u64 = ARENA_GPU + BITSTREAM_OFFSET as u64;
const MFX_STATS_GPU: u64 = ARENA_GPU + MFX_STATS_OFFSET as u64;
const VDENC_STATS_GPU: u64 = ARENA_GPU + VDENC_STATS_OFFSET as u64;
const SLICE_SIZE_GPU: u64 = ARENA_GPU + SLICE_SIZE_OFFSET as u64;
const MV_OBJECT_GPU: u64 = ARENA_GPU + MV_OBJECT_OFFSET as u64;
const DMV_0_GPU: u64 = ARENA_GPU + DMV_0_OFFSET as u64;
const DMV_1_GPU: u64 = ARENA_GPU + DMV_1_OFFSET as u64;
const MPR_ROWSTORE_GPU: u64 = ARENA_GPU + MPR_ROWSTORE_OFFSET as u64;
const INTRA_ROWSTORE_GPU: u64 = ARENA_GPU + INTRA_ROWSTORE_OFFSET as u64;
const DEBLOCK_ROWSTORE_GPU: u64 = ARENA_GPU + DEBLOCK_ROWSTORE_OFFSET as u64;
const BSP_ROWSTORE_GPU: u64 = ARENA_GPU + BSP_ROWSTORE_OFFSET as u64;

const TIMEOUT_NS: u64 = 100_000_000;
const POLL_LIMIT: u32 = 2_000_000;
const EXPECTED_IDR_CODEC_PACKETS: usize = 32;
const EXPECTED_IDR_BATCH_BYTES: usize = 2_628;
const EXPECTED_P_CODEC_PACKETS: usize = 31;
const EXPECTED_P_BATCH_BYTES: usize = 2_612;
const EXPECTED_PRIMARY_BATCH_BYTES: usize = 40;
pub(crate) const GOP_PICTURES: u32 = 40;

const _: () = {
    assert!(RING_GPU % crate::intel::WARM_ALIGN as u64 == 0);
    assert!(CONTEXT_GPU % crate::intel::WARM_ALIGN as u64 == 0);
    assert!(ARENA_GPU % crate::intel::WARM_ALIGN as u64 == 0);
    assert!(CODEC_BATCH_OFFSET + CODEC_BATCH_BYTES == BATCH_BYTES);
    assert!(FRAME_WIDTH % 16 == 0);
    assert!(FRAME_HEIGHT % 16 == 0);
    assert!(FRAME_WIDTH_MBS == 160);
    assert!(FRAME_HEIGHT_MBS == 90);
    assert!(FRAME_MACROBLOCKS == 14_400);
    assert!(GOP_PICTURES > 1 && GOP_PICTURES <= 128);
    assert!(DS_WIDTH % 16 == 0);
    assert!(DS_LOGICAL_HEIGHT == 360);
    assert!(DS_HEIGHT >= DS_LOGICAL_HEIGHT);
    assert!(DS_HEIGHT % 32 == 0);
    assert!(DS_PITCH >= DS_WIDTH);
    assert!(DS_PITCH % 64 == 0);
    assert!(EXPECTED_PRIMARY_BATCH_BYTES <= PRIMARY_BATCH_BYTES);
    assert!(BATCH_OFFSET + BATCH_BYTES <= RESULT_OFFSET);
    assert!(RESULT_OFFSET + RESULT_BYTES <= SOURCE_OFFSET);
    assert!(SOURCE_OFFSET + SOURCE_BYTES <= RECON_0_OFFSET);
    assert!(RECON_0_OFFSET + RECON_BYTES <= RECON_1_OFFSET);
    assert!(RECON_1_OFFSET + RECON_BYTES <= DS_0_OFFSET);
    assert!(DS_0_OFFSET + DS_BYTES <= DS_1_OFFSET);
    assert!(DS_1_OFFSET + DS_BYTES <= BITSTREAM_OFFSET);
    assert!(BITSTREAM_OFFSET + BITSTREAM_BYTES <= MFX_STATS_OFFSET);
    assert!(BSP_ROWSTORE_OFFSET + SCRATCH_BYTES <= MV_OBJECT_OFFSET);
    assert!(MV_OBJECT_OFFSET + MV_OBJECT_BYTES <= DMV_0_OFFSET);
    assert!(DMV_0_OFFSET + DMV_BYTES <= DMV_1_OFFSET);
    assert!(DMV_1_OFFSET + DMV_BYTES <= MPR_ROWSTORE_OFFSET);
    assert!(MPR_ROWSTORE_OFFSET + SCRATCH_BYTES <= ARENA_BYTES);
    assert!(ARENA_GPU + ARENA_BYTES as u64 <= 0x5000_0000);
};

const KICKOFF_MARKER: u32 = 0x4156_4301;
const CODEC_BEGIN_MARKER: u32 = 0x4156_4302;
const CODEC_END_MARKER: u32 = 0x4156_4303;
const COMPLETE_MARKER: u32 = 0x4156_4304;

// Keep codec-owned status away from the generic media result slots. Each
// register store writes one dword; eight-byte spacing keeps dumps readable and
// leaves room for widening individual fields later.
const RESULT_MFC_FRAME_BYTES_SLOT: u64 = 0x100;
const RESULT_MFX_ERROR_SLOT: u64 = 0x108;
const RESULT_MFC_IMAGE_STATUS_SLOT: u64 = 0x110;
const RESULT_MFC_SLICE_BYTES_SLOT: u64 = 0x118;
const RESULT_MFC_NUM_SLICES_SLOT: u64 = 0x120;

// Xe_LPM+ VDBOX0 completion/status registers. Keep these as a read-only,
// timeout-only diagnostic surface; command-stream status stores remain the
// authority once the hardware encode path is promoted beyond this probe.
const MFX_ERROR_FLAG: usize = 0x1C_0800;
const MFX_FRAME_CRC: usize = 0x1C_0850;
const MFX_MB_COUNT: usize = 0x1C_0868;
const MFC_BITSTREAM_BYTECOUNT_FRAME: usize = 0x1C_08A0;
const MFC_BITSTREAM_SE_BITCOUNT_FRAME: usize = 0x1C_08A4;
const MFC_IMAGE_STATUS_MASK: usize = 0x1C_08B4;
const MFC_IMAGE_STATUS_CONTROL: usize = 0x1C_08B8;
const MFC_QP_STATUS_COUNT: usize = 0x1C_08BC;
const MFC_BITSTREAM_BYTECOUNT_SLICE: usize = 0x1C_08D0;
const MFC_AVC_NUM_SLICES: usize = 0x1C_0954;
const MI_STORE_REGISTER_MEM_GEN8_PPGTT: u32 = (0x24 << 23) | 2;
const GEN8_RING_FAULT_REG: usize = 0x0000_4094;
const GEN8_FAULT_TLB_DATA0: usize = 0x0000_4B10;
const GEN8_FAULT_TLB_DATA1: usize = 0x0000_4B14;
const GEN12_FAULT_TLB_DATA0: usize = 0x0000_CEB8;
const GEN12_FAULT_TLB_DATA1: usize = 0x0000_CEBC;

const MI_FORCE_WAKEUP_MFX: [u32; 2] = [0x0e80_0000, 0x0300_0200];
const MFX_PIPE_MODE_SELECT: [u32; 5] = [0x7000_0003, 0x0002_22d2, 0, 0, 0];
const MFX_SURFACE_RECON: [u32; 6] = mfx_surface_state(0, FRAME_WIDTH, FRAME_HEIGHT, FRAME_WIDTH);
const MFX_SURFACE_SOURCE: [u32; 6] = mfx_surface_state(4, FRAME_WIDTH, FRAME_HEIGHT, FRAME_WIDTH);
const MFX_SURFACE_DS: [u32; 6] = mfx_surface_state(5, DS_WIDTH, DS_HEIGHT, DS_PITCH);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct AvcPicture {
    is_idr: bool,
    frame_num: u8,
}

impl AvcPicture {
    const BOOT_IDR: Self = Self {
        is_idr: true,
        frame_num: 0,
    };

    const fn for_sequence(sequence: u32) -> Self {
        let gop_position = sequence % GOP_PICTURES;
        Self {
            is_idr: gop_position == 0,
            frame_num: gop_position as u8,
        }
    }

    const fn current_recon_gpu(self) -> u64 {
        if self.frame_num & 1 == 0 {
            RECON_0_GPU
        } else {
            RECON_1_GPU
        }
    }

    const fn reference_recon_gpu(self) -> Option<u64> {
        if self.is_idr {
            None
        } else if self.frame_num & 1 == 0 {
            Some(RECON_1_GPU)
        } else {
            Some(RECON_0_GPU)
        }
    }

    const fn current_ds_gpu(self) -> u64 {
        if self.frame_num & 1 == 0 {
            DS_0_GPU
        } else {
            DS_1_GPU
        }
    }

    const fn reference_ds_gpu(self) -> Option<u64> {
        if self.is_idr {
            None
        } else if self.frame_num & 1 == 0 {
            Some(DS_1_GPU)
        } else {
            Some(DS_0_GPU)
        }
    }

    const fn current_dmv_gpu(self) -> u64 {
        if self.frame_num & 1 == 0 {
            DMV_0_GPU
        } else {
            DMV_1_GPU
        }
    }

    const fn reference_dmv_gpu(self) -> Option<u64> {
        if self.is_idr {
            None
        } else if self.frame_num & 1 == 0 {
            Some(DMV_1_GPU)
        } else {
            Some(DMV_0_GPU)
        }
    }
}

const fn mfx_surface_state(surface_id: u32, width: usize, height: usize, pitch: usize) -> [u32; 6] {
    [
        0x7001_0004,
        surface_id,
        (((height - 1) as u32) << 18) | (((width - 1) as u32) << 4),
        0x4800_0000 | (((pitch - 1) as u32) << 3),
        height as u32,
        height as u32,
    ]
}

const MFX_AVC_IMG_STATE_BASE: [u32; 21] = {
    let mut words = [
        0x7100_0013,
        0,
        0,
        0x0000_2000,
        0x0000_1514,
        0x0800_008f,
        0x0fff_0a8c,
        0,
        0,
        0,
        0xffff_c000,
        0x8000_0000,
        0,
        0,
        0,
        0,
        0,
        0x0000_0100,
        0,
        0,
        0,
    ];
    words[1] = FRAME_MACROBLOCKS as u32;
    words[2] = (((FRAME_HEIGHT_MBS - 1) as u32) << 16) | (FRAME_WIDTH_MBS - 1) as u32;
    words
};
const MFX_AVC_SLICE_STATE_BASE: [u32; 11] = {
    let mut words = [
        0x7103_0009,
        0x0000_0002,
        0,
        0x001a_0000,
        0,
        0,
        0x000b_3000,
        0,
        0,
        0x2d00_0000,
        0,
    ];
    words[5] = (FRAME_HEIGHT_MBS as u32) << 16;
    words
};
const MFX_AVC_REF_IDX_STATE_L0: [u32; 10] = [
    0x7104_0008,
    0,
    0x8080_8000,
    0x8080_8080,
    0x8080_8080,
    0x8080_8080,
    0x8080_8080,
    0x8080_8080,
    0x8080_8080,
    0x8080_8080,
];

fn mfx_avc_img_state(picture: AvcPicture) -> [u32; 21] {
    let mut words = MFX_AVC_IMG_STATE_BASE;
    // Match the SPS: one reference frame, pic_order_cnt_type=2 and an
    // eight-bit frame_num (log2_max_frame_num_minus4=4).
    words[13] = 1 << 24;
    words[14] = (4 << 16) | (2 << 2);
    words[15] = u32::from(picture.frame_num) << 16;
    if !picture.is_idr {
        words[13] |= 1 << 8;
    }
    words
}

fn mfx_avc_slice_state(picture: AvcPicture) -> [u32; 11] {
    let mut words = MFX_AVC_SLICE_STATE_BASE;
    if !picture.is_idr {
        words[1] = 0;
        words[2] = 1 << 16;
    }
    words
}

const VDENC_PIPE_MODE_SELECT: [u32; 6] = [0x7080_0004, 0x0122_00a2, 0x002b_030a, 0x0700_0303, 0, 0];
const VDENC_SRC_SURFACE_STATE: [u32; 6] =
    vdenc_surface_state(0x7081_0004, FRAME_WIDTH, FRAME_HEIGHT, FRAME_WIDTH, 0x2070_0000, 0x8);
const VDENC_REF_SURFACE_STATE: [u32; 6] =
    vdenc_surface_state(0x7082_0004, FRAME_WIDTH, FRAME_HEIGHT, FRAME_WIDTH, 0x2000_0000, 0);
const VDENC_DS_REF_SURFACE_STATE: [u32; 10] = {
    let surface = vdenc_surface_state(0x7083_0008, DS_WIDTH, DS_HEIGHT, DS_PITCH, 0x2000_0000, 0);
    [
        surface[0], surface[1], surface[2], surface[3], surface[4], surface[5], 0, 3, 0, 0,
    ]
};

const fn vdenc_surface_state(
    command: u32,
    width: usize,
    height: usize,
    pitch: usize,
    pitch_flags: u32,
    dimension_flags: u32,
) -> [u32; 6] {
    [
        command,
        0,
        (((height - 1) as u32) << 18) | (((width - 1) as u32) << 4) | dimension_flags,
        pitch_flags | (((pitch - 1) as u32) << 3),
        height as u32,
        height as u32,
    ]
}
const VDENC_CMD3: [u32; 61] = {
    let mut words = [0u32; 61];
    words[0] = 0x7086_003b;
    words[1] = 0x0101_0101;
    words[2] = 0x0201_0101;
    words[3] = 0x0302_0202;
    words[4] = 0x0404_0303;
    words[5] = 0x0706_0505;
    words[6] = 0x0a09_0807;
    words[7] = 0x110f_0d0c;
    words[8] = 0x1a17_1513;
    words[9] = 0x2a25_211e;
    words[10] = 0x423b_352f;
    words[11] = 0x0000_534a;
    words
};
const VDENC_IMG_STATE_BASE: [u32; 35] = {
    let mut words = [
        0x7085_0021,
        0x0000_0040,
        0,
        0,
        0x708a_0000,
        0,
        0,
        0,
        2,
        0x2e01_000c,
        0,
        0,
        0,
        0,
        0x0000_001a,
        0,
        0,
        0,
        0,
        0,
        0x0004_0c24,
        0,
        0xffff_0000,
        0,
        0,
        0,
        0,
        0x0400_2000,
        0,
        0,
        0,
        0,
        0,
        0x0f00_0a33,
        0,
    ];
    words[3] = (FRAME_WIDTH_MBS as u32) << 16;
    words[5] = 0x0001_0000 | (FRAME_HEIGHT_MBS - 1) as u32;
    words[6] = (FRAME_HEIGHT_MBS - 1) as u32;
    words
};

fn vdenc_img_state(picture: AvcPicture) -> [u32; 35] {
    let mut words = VDENC_IMG_STATE_BASE;
    if !picture.is_idr {
        words[4] |= 0x0024_3000;
        // This stream binds exactly one L0 reference and leaves Ref1 absent.
        // Gen12 requires HME Ref1 to be disabled in that non-perf P mode.
        words[5] |= (1 << 29) | (1 << 17);
        words[8] |= (1 << 5) | (1 << 6);
    }
    words
}
const VDENC_WEIGHTS_OFFSETS_STATE: [u32; 3] = [0x7088_0001, 0x0001_0001, 0x0000_0001];
const VDENC_WALKER_STATE: [u32; 27] = {
    let mut words = [
        0x7087_0019,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0x3f40_0000,
        0,
        0x003f_3f3f,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    words[2] = FRAME_HEIGHT_MBS as u32;
    words[5] = (FRAME_WIDTH - 1) as u32;
    words
};
const VD_PIPELINE_FLUSH: [u32; 2] = [0x7780_0000, 0x0002_001a];
// Intel's Gen12 AVC path follows VD_PIPELINE_FLUSH with two non-postsync
// MI_FLUSH_DW commands before sampling MFC status registers. The first also
// invalidates the video-pipeline cache; the trailing zero in each packet is
// the same alignment NOOP emitted by the production driver.
const MI_FLUSH_DW_VIDEO_CACHE_INVALIDATE: [u32; 5] = [0x1300_0082, 0, 0, 0, media::MI_NOOP];
const MI_FLUSH_DW_NO_POSTSYNC: [u32; 5] = [0x1300_0002, 0, 0, 0, media::MI_NOOP];

// VUI timing matches the 40 fps live-stream cadence soft cap:
// time_scale / (2 * num_units_in_tick) = 80 / 2.
const SPS_VUI_FRAME_RATE_HZ: usize = 40;
// 2560x1440 at 40 fps is 576,000 macroblocks/s. Level 5.0 admits both the
// 14,400-macroblock frame and that rate; Level 4.1 does not. Since 1440 is
// already macroblock-aligned, the SPS carries no frame crop. One short-term
// reference frame supports the IDR-P GOP without changing the decoded raster.
const SPS: [u8; 30] = [
    0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x40, 0x32, 0x95, 0xa0, 0x0a, 0x00, 0x2d, 0x6c, 0x05, 0xa2,
    0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x00, 0x03, 0x00, 0xa1, 0x1e, 0x10, 0x08, 0x54,
];
const _: () = assert!(SPS_VUI_FRAME_RATE_HZ == crate::allcaps::media_encode::REALTIME_HZ);
const PPS: [u8; 8] = [0x00, 0x00, 0x00, 0x01, 0x68, 0xce, 0x38, 0x80];
const IDR_SLICE_HEADER: [u8; 8] = [0x00, 0x00, 0x01, 0x65, 0x88, 0x80, 0x48, 0x00];
const IDR_SLICE_HEADER_BITS: usize = 53;
const P_SLICE_HEADER_BITS: usize = 51;
// SPS/PPS use HeaderLengthExcludeFrmSize, matching Intel's production AVC
// path. MFC_BITSTREAM_BYTECOUNT_FRAME therefore excludes these two inserts,
// while the emulated slice-header insert remains included.
const EXCLUDED_HEADER_BYTES: usize = SPS.len() + PPS.len();

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum AvcEncodeProbeState {
    NotRun = 0,
    Deferred = 1,
    Preparing = 2,
    Submitted = 3,
    Passed = 4,
    Failed = 5,
    Quarantined = 6,
}

impl AvcEncodeProbeState {
    const fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::Deferred,
            2 => Self::Preparing,
            3 => Self::Submitted,
            4 => Self::Passed,
            5 => Self::Failed,
            6 => Self::Quarantined,
            _ => Self::NotRun,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AvcEncodeProbeFailure {
    None,
    DeviceUnavailable,
    Vcs0Unavailable,
    GucTransportUnavailable,
    TransportProbeUnavailable,
    LaneBusy,
    LaneQuarantined,
    ForcewakeUnavailable,
    BackingAllocation,
    SurfaceConversion,
    BatchBuild,
    ContextBuild,
    RegisterRejected,
    SubmitRejected,
    CompletionTimeout,
    MarkerMismatch,
    CodedOutputInvalid,
    ContextTeardown,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcEncodeTimeoutDiagnostics {
    pub(crate) valid: bool,
    pub(crate) ring_start: u32,
    pub(crate) ring_ctl: u32,
    pub(crate) ring_head: u32,
    pub(crate) ring_tail: u32,
    pub(crate) ring_acthd_lo: u32,
    pub(crate) ring_acthd_hi: u32,
    pub(crate) acthd_region: &'static str,
    pub(crate) acthd_offset_bytes: u32,
    pub(crate) acthd_dword: u32,
    pub(crate) bbaddr_lo: u32,
    pub(crate) bbaddr_hi: u32,
    pub(crate) dma_fadd_lo: u32,
    pub(crate) dma_fadd_hi: u32,
    pub(crate) bbstate: u32,
    pub(crate) esr: u32,
    pub(crate) instdone: u32,
    pub(crate) instps: u32,
    pub(crate) psmi_ctl: u32,
    pub(crate) nopid: u32,
    pub(crate) ipeir: u32,
    pub(crate) ipehr: u32,
    pub(crate) fault_gen8: u32,
    pub(crate) fault_gen12: u32,
    pub(crate) fault_tlb_data0_gen8: u32,
    pub(crate) fault_tlb_data1_gen8: u32,
    pub(crate) fault_tlb_data0_gen12: u32,
    pub(crate) fault_tlb_data1_gen12: u32,
    pub(crate) mfx_error: u32,
    pub(crate) mfx_frame_crc: u32,
    pub(crate) mfx_mb_count: u32,
    pub(crate) mfc_bitstream_bytecount_frame: u32,
    pub(crate) mfc_bitstream_se_bitcount_frame: u32,
    pub(crate) mfc_bitstream_bytecount_slice: u32,
    pub(crate) mfc_image_status_mask: u32,
    pub(crate) mfc_image_status_control: u32,
    pub(crate) mfc_qp_status_count: u32,
    pub(crate) mfc_avc_num_slices: u32,
    pub(crate) bitstream_head: [u32; 8],
    pub(crate) mfx_stats_head: [u32; 4],
    pub(crate) vdenc_stats_head: [u32; 4],
    pub(crate) slice_size_head: [u32; 4],
    pub(crate) current_recon_sample: u32,
    pub(crate) reference_recon_sample: u32,
    pub(crate) current_ds_sample: u32,
    pub(crate) reference_ds_sample: u32,
}

impl AvcEncodeTimeoutDiagnostics {
    const EMPTY: Self = Self {
        valid: false,
        ring_start: 0,
        ring_ctl: 0,
        ring_head: 0,
        ring_tail: 0,
        ring_acthd_lo: 0,
        ring_acthd_hi: 0,
        acthd_region: "none",
        acthd_offset_bytes: 0,
        acthd_dword: 0,
        bbaddr_lo: 0,
        bbaddr_hi: 0,
        dma_fadd_lo: 0,
        dma_fadd_hi: 0,
        bbstate: 0,
        esr: 0,
        instdone: 0,
        instps: 0,
        psmi_ctl: 0,
        nopid: 0,
        ipeir: 0,
        ipehr: 0,
        fault_gen8: 0,
        fault_gen12: 0,
        fault_tlb_data0_gen8: 0,
        fault_tlb_data1_gen8: 0,
        fault_tlb_data0_gen12: 0,
        fault_tlb_data1_gen12: 0,
        mfx_error: 0,
        mfx_frame_crc: 0,
        mfx_mb_count: 0,
        mfc_bitstream_bytecount_frame: 0,
        mfc_bitstream_se_bitcount_frame: 0,
        mfc_bitstream_bytecount_slice: 0,
        mfc_image_status_mask: 0,
        mfc_image_status_control: 0,
        mfc_qp_status_count: 0,
        mfc_avc_num_slices: 0,
        bitstream_head: [0; 8],
        mfx_stats_head: [0; 4],
        vdenc_stats_head: [0; 4],
        slice_size_head: [0; 4],
        current_recon_sample: 0,
        reference_recon_sample: 0,
        current_ds_sample: 0,
        reference_ds_sample: 0,
    };
}

impl AvcEncodeProbeFailure {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::DeviceUnavailable => "device-unavailable",
            Self::Vcs0Unavailable => "vcs0-unavailable",
            Self::GucTransportUnavailable => "guc-transport-unavailable",
            Self::TransportProbeUnavailable => "guc-vcs0-transport-probe-unavailable",
            Self::LaneBusy => "vcs0-lane-busy",
            Self::LaneQuarantined => "vcs0-lane-quarantined",
            Self::ForcewakeUnavailable => "vcs0-forcewake-unavailable",
            Self::BackingAllocation => "encode-probe-backing-allocation",
            Self::SurfaceConversion => "nv12-surface-preparation",
            Self::BatchBuild => "avc-picture-batch-build",
            Self::ContextBuild => "vcs0-context-build",
            Self::RegisterRejected => "guc-register-rejected",
            Self::SubmitRejected => "guc-submit-rejected",
            Self::CompletionTimeout => "completion-timeout",
            Self::MarkerMismatch => "ordered-marker-mismatch",
            Self::CodedOutputInvalid => "coded-output-invalid",
            Self::ContextTeardown => "guc-context-teardown",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcEncodeProbeReport {
    pub(crate) state: AvcEncodeProbeState,
    pub(crate) failure: AvcEncodeProbeFailure,
    pub(crate) forcewake: bool,
    pub(crate) backing_ready: bool,
    pub(crate) surface_uploaded: bool,
    pub(crate) batch_ready: bool,
    pub(crate) context_ready: bool,
    pub(crate) registered: bool,
    pub(crate) submitted: bool,
    pub(crate) retired: bool,
    pub(crate) context_destroyed: bool,
    pub(crate) bitstream_buffer_bound: bool,
    pub(crate) coded_output_validated: bool,
    pub(crate) coded_bytes: usize,
    pub(crate) coded_fnv1a32: u32,
    pub(crate) coded_nal_flags: u8,
    pub(crate) idr_picture: bool,
    pub(crate) frame_num: u8,
    pub(crate) excluded_header_bytes: usize,
    pub(crate) mfc_bitstream_bytecount_frame: u32,
    pub(crate) mfx_error: u32,
    pub(crate) mfc_image_status_control: u32,
    pub(crate) mfc_bitstream_bytecount_slice: u32,
    pub(crate) mfc_avc_num_slices: u32,
    pub(crate) bitstream_head: [u32; 20],
    pub(crate) source_nv12_bytes: usize,
    pub(crate) source_nv12_fnv1a32: u32,
    pub(crate) batch_bytes: usize,
    pub(crate) primary_batch_bytes: usize,
    pub(crate) ring_bytes: usize,
    pub(crate) codec_packets: usize,
    pub(crate) serial: u64,
    pub(crate) hwlrca_lo: u32,
    pub(crate) hwlrca_hi: u32,
    pub(crate) kickoff: u32,
    pub(crate) codec_begin: u32,
    pub(crate) codec_end: u32,
    pub(crate) complete: u32,
    pub(crate) poll_iters: u32,
    pub(crate) elapsed_us: u64,
    pub(crate) timeout_diagnostics: AvcEncodeTimeoutDiagnostics,
}

impl AvcEncodeProbeReport {
    const EMPTY: Self = Self {
        state: AvcEncodeProbeState::NotRun,
        failure: AvcEncodeProbeFailure::None,
        forcewake: false,
        backing_ready: false,
        surface_uploaded: false,
        batch_ready: false,
        context_ready: false,
        registered: false,
        submitted: false,
        retired: false,
        context_destroyed: false,
        bitstream_buffer_bound: false,
        coded_output_validated: false,
        coded_bytes: 0,
        coded_fnv1a32: 0,
        coded_nal_flags: 0,
        idr_picture: false,
        frame_num: 0,
        excluded_header_bytes: EXCLUDED_HEADER_BYTES,
        mfc_bitstream_bytecount_frame: 0,
        mfx_error: 0,
        mfc_image_status_control: 0,
        mfc_bitstream_bytecount_slice: 0,
        mfc_avc_num_slices: 0,
        bitstream_head: [0; 20],
        source_nv12_bytes: 0,
        source_nv12_fnv1a32: 0,
        batch_bytes: 0,
        primary_batch_bytes: 0,
        ring_bytes: 0,
        codec_packets: 0,
        serial: 0,
        hwlrca_lo: 0,
        hwlrca_hi: 0,
        kickoff: 0,
        codec_begin: 0,
        codec_end: 0,
        complete: 0,
        poll_iters: 0,
        elapsed_us: 0,
        timeout_diagnostics: AvcEncodeTimeoutDiagnostics::EMPTY,
    };
}

struct ProbeBacking {
    ring_virt: *mut u8,
    context_virt: *mut u8,
    arena_phys: u64,
    arena_virt: *mut u8,
    ppgtt: crate::intel::ppgtt::SparsePpgtt,
}

unsafe impl Send for ProbeBacking {}

#[derive(Copy, Clone)]
pub(crate) struct AvcNv12DmaSurface {
    phys: u64,
    bytes: usize,
}

impl AvcNv12DmaSurface {
    pub(crate) fn new(phys: u64, bytes: usize) -> Option<Self> {
        (phys != 0 && phys.is_multiple_of(crate::intel::WARM_ALIGN as u64) && bytes == SOURCE_BYTES)
            .then_some(Self { phys, bytes })
    }
}

#[derive(Copy, Clone)]
enum AvcFrameSource {
    BootProof,
    Dma(AvcNv12DmaSurface),
}

static STATE: AtomicU8 = AtomicU8::new(AvcEncodeProbeState::NotRun as u8);
static REPORT: Mutex<AvcEncodeProbeReport> = Mutex::new(AvcEncodeProbeReport::EMPTY);
static BACKING: Mutex<Option<ProbeBacking>> = Mutex::new(None);
static CODED_ACCESS_UNIT: Mutex<Option<Vec<u8>>> = Mutex::new(None);

pub(crate) const fn commands_wired() -> bool {
    true
}

pub(crate) fn passed() -> bool {
    STATE.load(Ordering::Acquire) == AvcEncodeProbeState::Passed as u8
}

pub(crate) fn coded_output_validated() -> bool {
    passed() && REPORT.lock().coded_output_validated
}

pub(crate) fn take_coded_access_unit() -> Option<Vec<u8>> {
    CODED_ACCESS_UNIT.lock().take()
}

pub(crate) fn snapshot() -> AvcEncodeProbeReport {
    *REPORT.lock()
}

/// Submit a GPU-produced linear NV12 surface directly as the VDBOX source.
/// The caller retains the DMA allocation until the submission retires, but
/// fence completion is awaited cooperatively. No CPU-side full-frame copy is
/// performed.
pub(crate) async fn run_nv12_dma_frame(
    surface: AvcNv12DmaSurface,
    sequence: u32,
) -> AvcEncodeProbeReport {
    run_live_frame(AvcFrameSource::Dma(surface), AvcPicture::for_sequence(sequence)).await
}

async fn run_live_frame(source: AvcFrameSource, picture: AvcPicture) -> AvcEncodeProbeReport {
    let state = AvcEncodeProbeState::from_raw(STATE.load(Ordering::Acquire));
    if state == AvcEncodeProbeState::Passed {
        STATE.store(AvcEncodeProbeState::NotRun as u8, Ordering::Release);
    } else if state != AvcEncodeProbeState::NotRun {
        return snapshot();
    }

    *CODED_ACCESS_UNIT.lock() = None;
    run_with_source(source, picture).await
}

pub(crate) async fn run_once() -> AvcEncodeProbeReport {
    run_with_source(AvcFrameSource::BootProof, AvcPicture::BOOT_IDR).await
}

async fn run_with_source(source_kind: AvcFrameSource, picture: AvcPicture) -> AvcEncodeProbeReport {
    let current = AvcEncodeProbeState::from_raw(STATE.load(Ordering::Acquire));
    if current != AvcEncodeProbeState::NotRun {
        return snapshot();
    }

    let Some(dev) = crate::intel::claimed_device() else {
        return deferred(AvcEncodeProbeFailure::DeviceUnavailable);
    };
    let (engine, _) = media::default_encode_engine_and_window();
    if engine.id.instance != 0 || !engine.capabilities.decode {
        return deferred(AvcEncodeProbeFailure::Vcs0Unavailable);
    }
    if !crate::intel::guc_submission::INTEL_GUC_SCHEDULER.ready() {
        return deferred(AvcEncodeProbeFailure::GucTransportUnavailable);
    }
    if !super::guc_probe::passed() {
        return deferred(AvcEncodeProbeFailure::TransportProbeUnavailable);
    }

    let live_frame = !matches!(source_kind, AvcFrameSource::BootProof);
    let lane_result = if live_frame {
        media::acquire_media_lane_bounded(
            engine,
            media::MediaJobMode::AVC_ENCODE_GUC,
            None,
            media::MEDIA_INTERLEAVE_WAIT_NS,
        )
    } else {
        media::try_acquire_media_lane(engine, media::MediaJobMode::AVC_ENCODE_GUC, None)
    };
    let mut lane = match lane_result {
        Ok(lane) => lane,
        Err(media::MediaLaneAcquireError::Busy) => {
            return deferred(AvcEncodeProbeFailure::LaneBusy);
        }
        Err(media::MediaLaneAcquireError::Quarantined) => {
            return deferred(AvcEncodeProbeFailure::LaneQuarantined);
        }
    };
    if STATE
        .compare_exchange(
            AvcEncodeProbeState::NotRun as u8,
            AvcEncodeProbeState::Preparing as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return snapshot();
    }

    let started_ns = crate::chronos::monotonic_nanos();
    let mut report = AvcEncodeProbeReport {
        state: AvcEncodeProbeState::Preparing,
        idr_picture: picture.is_idr,
        frame_num: picture.frame_num,
        excluded_header_bytes: if picture.is_idr {
            EXCLUDED_HEADER_BYTES
        } else {
            0
        },
        ..AvcEncodeProbeReport::EMPTY
    };
    publish(report);

    report.forcewake = media::wake_media_engine_for_guc(dev, engine);
    if !report.forcewake {
        return fail(report, AvcEncodeProbeFailure::ForcewakeUnavailable, started_ns);
    }

    let mut backing_slot = BACKING.lock();
    if backing_slot.is_none() {
        *backing_slot = build_backing(dev);
    }
    let Some(backing) = backing_slot.as_mut() else {
        return fail(report, AvcEncodeProbeFailure::BackingAllocation, started_ns);
    };
    report.backing_ready = true;
    if lane.requires_reactivation() {
        media::reset_media_engine(dev, engine, backing.context_virt);
    }

    unsafe {
        core::ptr::write_bytes(backing.ring_virt, 0, RING_BYTES);
        core::ptr::write_bytes(backing.context_virt, 0, CONTEXT_BYTES);
    }
    clear_frame_result(backing.arena_virt);

    let arena_source_phys = backing.arena_phys.saturating_add(SOURCE_OFFSET as u64);
    let source_virt = unsafe { backing.arena_virt.add(SOURCE_OFFSET) };
    let source = unsafe { core::slice::from_raw_parts_mut(source_virt, SOURCE_BYTES) };
    let (source_phys, source_hash) = match source_kind {
        AvcFrameSource::BootProof => {
            if !fill_boot_proof_nv12(source) {
                return fail(report, AvcEncodeProbeFailure::SurfaceConversion, started_ns);
            }
            (arena_source_phys, fnv1a32(source))
        }
        AvcFrameSource::Dma(surface) => {
            if surface.bytes != SOURCE_BYTES {
                return fail(report, AvcEncodeProbeFailure::SurfaceConversion, started_ns);
            }
            // The live surface is produced by RCS, whose terminal PIPE_CONTROL
            // drains HDC/L3 before its completion marker is published. VDBOX
            // consumes the same DMA allocation directly after that marker.
            // Do not walk all 5.5 MiB through CPU CLFLUSH or sample it between
            // those two GPU owners; live source-change telemetry is collected
            // from the CPU-authored RGBA composition before RCS submission.
            (surface.phys, 0)
        }
    };
    if backing
        .ppgtt
        .map_range(crate::intel::ppgtt::PpgttRange {
            gpu: SOURCE_GPU,
            phys: source_phys,
            bytes: SOURCE_BYTES,
        })
        .is_none()
    {
        return fail(report, AvcEncodeProbeFailure::SurfaceConversion, started_ns);
    }
    report.surface_uploaded = true;
    report.source_nv12_bytes = SOURCE_BYTES;
    report.source_nv12_fnv1a32 = source_hash;

    let batch_virt = unsafe { backing.arena_virt.add(BATCH_OFFSET + CODEC_BATCH_OFFSET) };
    let Some((batch_bytes, codec_packets)) = build_picture_batch(batch_virt, picture) else {
        return fail(report, AvcEncodeProbeFailure::BatchBuild, started_ns);
    };
    report.batch_ready = true;
    report.bitstream_buffer_bound = true;
    report.batch_bytes = batch_bytes;
    report.codec_packets = codec_packets;

    let primary_batch_virt = unsafe { backing.arena_virt.add(BATCH_OFFSET) };
    let Some(primary_batch_bytes) = media::build_primary_second_level_return_words(
        primary_batch_virt,
        PRIMARY_BATCH_BYTES,
        RESULT_GPU,
        CODEC_BATCH_GPU,
        COMPLETE_MARKER,
        lane.mode(),
    ) else {
        return fail(report, AvcEncodeProbeFailure::BatchBuild, started_ns);
    };
    if primary_batch_bytes != EXPECTED_PRIMARY_BATCH_BYTES {
        return fail(report, AvcEncodeProbeFailure::BatchBuild, started_ns);
    }
    report.primary_batch_bytes = primary_batch_bytes;

    let Some(ring_tail_bytes) = media::build_ring_batch_start_words(
        backing.ring_virt,
        RING_BYTES,
        0,
        RESULT_GPU,
        KICKOFF_MARKER,
        BATCH_GPU,
        lane.mode(),
    ) else {
        return fail(report, AvcEncodeProbeFailure::ContextBuild, started_ns);
    };
    report.ring_bytes = ring_tail_bytes;
    let Some(ring_ctl) = media::ring_ctl_value_for_size(RING_BYTES) else {
        return fail(report, AvcEncodeProbeFailure::ContextBuild, started_ns);
    };
    if !media::init_gen12_video_context_image(
        backing.context_virt,
        CONTEXT_BYTES,
        engine.ring_base,
        0,
        RING_GPU as u32,
        ring_tail_bytes as u32,
        ring_ctl,
        CONTEXT_GPU as u32,
        backing.ppgtt.pml4_phys(),
        false,
    ) {
        return fail(report, AvcEncodeProbeFailure::ContextBuild, started_ns);
    }
    report.context_ready = true;

    flush_cpu_authored_frame_ranges(backing.arena_virt, primary_batch_bytes, batch_bytes);
    if !matches!(source_kind, AvcFrameSource::Dma(_)) {
        crate::intel::dma_flush(source_virt, SOURCE_BYTES);
    }
    crate::intel::dma_flush(backing.ring_virt, ring_tail_bytes);
    crate::intel::dma_flush(backing.context_virt, CONTEXT_BYTES);
    crate::intel::ggtt_invalidate(dev);
    core::sync::atomic::fence(Ordering::SeqCst);

    let (hwlrca_lo, hwlrca_hi) = media::build_media_guc_context_descriptor(CONTEXT_GPU);
    report.hwlrca_lo = hwlrca_lo;
    report.hwlrca_hi = hwlrca_hi;
    let token = match crate::intel::guc_submission::INTEL_GUC_SCHEDULER.register(
        dev,
        crate::gpu::physical::PhysicalEngineId::VCS0,
        hwlrca_lo,
        hwlrca_hi,
        crate::gpu::physical::PhysicalContextPriority::KernelNormal,
    ) {
        Ok(token) => token,
        Err(_) => return fail(report, AvcEncodeProbeFailure::RegisterRejected, started_ns),
    };
    report.registered = true;

    let submission = match crate::intel::guc_submission::INTEL_GUC_SCHEDULER.submit(dev, token) {
        Ok(submission) => submission,
        Err(_) => {
            report.context_destroyed = crate::intel::guc_submission::INTEL_GUC_SCHEDULER
                .destroy(dev, token)
                .is_ok();
            if !report.context_destroyed {
                return quarantine(
                    lane,
                    report,
                    AvcEncodeProbeFailure::ContextTeardown,
                    started_ns,
                );
            }
            return fail(report, AvcEncodeProbeFailure::SubmitRejected, started_ns);
        }
    };
    report.state = AvcEncodeProbeState::Submitted;
    report.submitted = true;
    report.serial = submission.serial;
    publish(report);

    let result_virt = unsafe { backing.arena_virt.add(RESULT_OFFSET) };
    let deadline = crate::chronos::monotonic_nanos().saturating_add(TIMEOUT_NS);
    while report.poll_iters < POLL_LIMIT {
        crate::intel::dma_flush(result_virt, RESULT_BYTES);
        report.complete = media::read_result_dword(result_virt, media::MEDIA_RESULT_COMPLETE_SLOT);
        if report.complete == COMPLETE_MARKER {
            report.retired = true;
            break;
        }
        report.poll_iters = report.poll_iters.saturating_add(1);
        if crate::chronos::monotonic_nanos() >= deadline {
            break;
        }
        // VDBOX owns the expensive part of this interval. Yield the private
        // LastAP executor so scanout preparation and UDP egress can advance
        // while the media fence is pending instead of serializing all three
        // cooperative tasks behind a CPU spin loop.
        Timer::after(Duration::from_micros(0)).await;
    }

    crate::intel::dma_flush(result_virt, RESULT_BYTES);
    report.kickoff = media::read_result_dword(result_virt, media::MEDIA_RESULT_KICKOFF_SLOT);
    report.codec_begin = media::read_result_dword(result_virt, media::MEDIA_RESULT_PRESUBMIT_SLOT);
    report.codec_end = media::read_result_dword(result_virt, media::MEDIA_RESULT_POSTSUBMIT_SLOT);
    report.complete = media::read_result_dword(result_virt, media::MEDIA_RESULT_COMPLETE_SLOT);
    if !report.retired {
        report.timeout_diagnostics = capture_timeout_diagnostics(dev, engine, backing, picture);
        return quarantine(lane, report, AvcEncodeProbeFailure::CompletionTimeout, started_ns);
    }

    report.context_destroyed = crate::intel::guc_submission::INTEL_GUC_SCHEDULER
        .destroy(dev, token)
        .is_ok();
    if !report.context_destroyed {
        return quarantine(lane, report, AvcEncodeProbeFailure::ContextTeardown, started_ns);
    }

    if report.kickoff != KICKOFF_MARKER
        || report.codec_begin != CODEC_BEGIN_MARKER
        || report.codec_end != CODEC_END_MARKER
        || report.complete != COMPLETE_MARKER
    {
        return fail(report, AvcEncodeProbeFailure::MarkerMismatch, started_ns);
    }

    report.mfx_error = media::read_result_dword(result_virt, RESULT_MFX_ERROR_SLOT);
    report.mfc_image_status_control =
        media::read_result_dword(result_virt, RESULT_MFC_IMAGE_STATUS_SLOT);
    report.mfc_bitstream_bytecount_slice =
        media::read_result_dword(result_virt, RESULT_MFC_SLICE_BYTES_SLOT);
    report.mfc_avc_num_slices = media::read_result_dword(result_virt, RESULT_MFC_NUM_SLICES_SLOT);
    report.mfc_bitstream_bytecount_frame =
        media::read_result_dword(result_virt, RESULT_MFC_FRAME_BYTES_SLOT);
    let coded_bytes = (report.mfc_bitstream_bytecount_frame as usize)
        .checked_add(report.excluded_header_bytes)
        .unwrap_or(BITSTREAM_BYTES.saturating_add(1));
    let bitstream_virt = unsafe { backing.arena_virt.add(BITSTREAM_OFFSET) };
    crate::intel::dma_flush(
        bitstream_virt,
        coded_bytes.clamp(8 * core::mem::size_of::<u32>(), BITSTREAM_BYTES),
    );
    report.bitstream_head = read_dword_head::<20>(bitstream_virt);
    if coded_bytes == 0 || coded_bytes > BITSTREAM_BYTES {
        return fail(report, AvcEncodeProbeFailure::CodedOutputInvalid, started_ns);
    }
    let coded = unsafe { core::slice::from_raw_parts(bitstream_virt, coded_bytes) };
    report.coded_bytes = coded_bytes;
    report.coded_fnv1a32 = fnv1a32(coded);
    report.coded_nal_flags = annex_b_nal_flags(coded);
    let parameter_sets_present = report.coded_nal_flags & 0b0011 == 0b0011;
    let idr_present = report.coded_nal_flags & 0b0100 != 0;
    let p_present = report.coded_nal_flags & 0b1000 != 0;
    let expected_nal_present = if picture.is_idr {
        parameter_sets_present && idr_present && !p_present
    } else {
        !parameter_sets_present && !idr_present && p_present
    };
    // An unchanged P picture can encode every macroblock as skipped and yield
    // a very small but complete non-IDR slice. Requiring more than 64 payload
    // bytes incorrectly rejects that legal access unit and tears down an idle
    // RDP session immediately after its IDR. Keep the stronger size sanity
    // check for the self-contained IDR proof; for P pictures, the Annex-B
    // parser finding the expected type-1 slice is the structural payload proof.
    let picture_payload_present = if picture.is_idr {
        coded_bytes > report.excluded_header_bytes.saturating_add(64)
    } else {
        p_present
    };
    if report.mfx_error != 0 || !expected_nal_present || !picture_payload_present {
        return fail(report, AvcEncodeProbeFailure::CodedOutputInvalid, started_ns);
    }
    *CODED_ACCESS_UNIT.lock() = Some(coded.to_vec());
    report.coded_output_validated = true;

    lane.complete();
    report.state = AvcEncodeProbeState::Passed;
    report.failure = AvcEncodeProbeFailure::None;
    report.elapsed_us = elapsed_us(started_ns);
    publish(report);
    report
}

fn build_backing(dev: crate::intel::Dev) -> Option<ProbeBacking> {
    let (ring_phys, ring_virt) = crate::dma::alloc(RING_BYTES, crate::intel::WARM_ALIGN)?;
    let (context_phys, context_virt) = crate::dma::alloc(CONTEXT_BYTES, crate::intel::WARM_ALIGN)?;
    let (arena_phys, arena_virt) = crate::dma::alloc(ARENA_BYTES, crate::intel::WARM_ALIGN)?;

    // Establish deterministic first-use contents once. Recon, downscale,
    // bitstream, statistics, and row-store ranges are subsequently owned by
    // VDEnc/MFX. Re-clearing and flushing those GPU outputs for every frame
    // adds no visibility to the terminal media fence and burns a 7.7 MiB CPU
    // zero plus a 7.7 MiB cache flush for every 40 Hz encode.
    initialize_arena_work_ranges(arena_virt);

    if !crate::intel::map_ggtt(dev, ring_phys, RING_BYTES, RING_GPU)
        || !crate::intel::map_ggtt(dev, context_phys, CONTEXT_BYTES, CONTEXT_GPU)
    {
        return None;
    }
    crate::intel::ggtt_invalidate(dev);
    let ppgtt =
        crate::intel::ppgtt::build_sparse_ppgtt_for_ranges(&[crate::intel::ppgtt::PpgttRange {
            gpu: ARENA_GPU,
            phys: arena_phys,
            bytes: ARENA_BYTES,
        }])?;
    Some(ProbeBacking {
        ring_virt,
        context_virt,
        arena_phys,
        arena_virt,
        ppgtt,
    })
}

fn initialize_arena_work_ranges(arena_virt: *mut u8) {
    for (offset, bytes) in ARENA_WORK_RANGES {
        unsafe {
            core::ptr::write_bytes(arena_virt.add(offset), 0, bytes);
        }
        crate::intel::dma_flush(unsafe { arena_virt.add(offset) }, bytes);
    }
}

fn clear_frame_result(arena_virt: *mut u8) {
    unsafe {
        core::ptr::write_bytes(arena_virt.add(RESULT_OFFSET), 0, RESULT_BYTES);
    }
}

fn flush_cpu_authored_frame_ranges(
    arena_virt: *mut u8,
    primary_batch_bytes: usize,
    codec_batch_bytes: usize,
) {
    crate::intel::dma_flush(unsafe { arena_virt.add(BATCH_OFFSET) }, primary_batch_bytes);
    crate::intel::dma_flush(
        unsafe { arena_virt.add(BATCH_OFFSET + CODEC_BATCH_OFFSET) },
        codec_batch_bytes,
    );
    crate::intel::dma_flush(unsafe { arena_virt.add(RESULT_OFFSET) }, RESULT_BYTES);
}

/// Build a legal-range moving-pattern seed without linking a diagnostic file
/// or a software encoder into the kernel. The boot submission exists only to
/// validate the same hardware graph used for subscribed UI4 frames.
fn fill_boot_proof_nv12(nv12: &mut [u8]) -> bool {
    const LUMA_BYTES: usize = FRAME_WIDTH * FRAME_HEIGHT;
    if nv12.len() != SOURCE_BYTES {
        return false;
    }

    for y in 0..FRAME_HEIGHT {
        let row = &mut nv12[y * FRAME_WIDTH..(y + 1) * FRAME_WIDTH];
        for (x, luma) in row.iter_mut().enumerate() {
            let ramp = x * 219 / (FRAME_WIDTH - 1);
            let checker = if ((x / 32) ^ (y / 32)) & 1 == 0 {
                0
            } else {
                12
            };
            *luma = (16 + ramp + checker).min(235) as u8;
        }
    }
    for y in 0..FRAME_HEIGHT / 2 {
        let row = &mut nv12[LUMA_BYTES + y * FRAME_WIDTH..LUMA_BYTES + (y + 1) * FRAME_WIDTH];
        for x in (0..FRAME_WIDTH).step_by(2) {
            let bar = x / 64;
            row[x] = (96 + bar % 9 * 8).min(160) as u8;
            row[x + 1] = (160usize.saturating_sub(bar % 9 * 8)).max(96) as u8;
        }
    }
    true
}

fn build_picture_batch(batch_virt: *mut u8, picture: AvcPicture) -> Option<(usize, usize)> {
    let batch = unsafe {
        core::slice::from_raw_parts_mut(
            batch_virt.cast::<u32>(),
            CODEC_BATCH_BYTES / core::mem::size_of::<u32>(),
        )
    };
    let mut idx = 0usize;
    let mut packet_count = 0usize;

    push_words(batch, &mut idx, &MI_FORCE_WAKEUP_MFX)?;
    if !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        RESULT_GPU + media::MEDIA_RESULT_PRESUBMIT_SLOT,
        CODEC_BEGIN_MARKER,
    ) {
        return None;
    }

    if !media::emit_mfx_wait(batch, &mut idx) {
        return None;
    }
    packet_count += 1;
    push_packet(batch, &mut idx, &MFX_PIPE_MODE_SELECT, &mut packet_count)?;
    if !media::emit_mfx_wait(batch, &mut idx) {
        return None;
    }
    packet_count += 1;
    push_packet(batch, &mut idx, &MFX_SURFACE_RECON, &mut packet_count)?;
    push_packet(batch, &mut idx, &MFX_SURFACE_SOURCE, &mut packet_count)?;
    push_packet(batch, &mut idx, &MFX_SURFACE_DS, &mut packet_count)?;

    let mfx_pipe_buf = mfx_pipe_buf_addr_state(picture);
    push_packet(batch, &mut idx, &mfx_pipe_buf, &mut packet_count)?;
    let mfx_ind_obj = mfx_ind_obj_base_addr_state();
    push_packet(batch, &mut idx, &mfx_ind_obj, &mut packet_count)?;
    let mfx_bsp = mfx_bsp_buf_base_addr_state(picture);
    push_packet(batch, &mut idx, &mfx_bsp, &mut packet_count)?;

    push_packet(batch, &mut idx, &VDENC_PIPE_MODE_SELECT, &mut packet_count)?;
    push_packet(batch, &mut idx, &VDENC_SRC_SURFACE_STATE, &mut packet_count)?;
    push_packet(batch, &mut idx, &VDENC_REF_SURFACE_STATE, &mut packet_count)?;
    push_packet(batch, &mut idx, &VDENC_DS_REF_SURFACE_STATE, &mut packet_count)?;
    let vdenc_pipe_buf = vdenc_pipe_buf_addr_state(picture);
    push_packet(batch, &mut idx, &vdenc_pipe_buf, &mut packet_count)?;

    push_packet(batch, &mut idx, &VDENC_CMD3, &mut packet_count)?;
    let mfx_img = mfx_avc_img_state(picture);
    push_packet(batch, &mut idx, &mfx_img, &mut packet_count)?;
    let vdenc_img = vdenc_img_state(picture);
    push_packet(batch, &mut idx, &vdenc_img, &mut packet_count)?;
    for matrix_type in 0..4u32 {
        let qm = mfx_qm_state(matrix_type);
        push_packet(batch, &mut idx, &qm, &mut packet_count)?;
    }
    for matrix_type in 0..4u32 {
        let fqm = mfx_fqm_state(matrix_type);
        push_packet(batch, &mut idx, &fqm, &mut packet_count)?;
    }

    if !picture.is_idr {
        // Gen12's AVC VDEnc path emits DIRECTMODE_STATE for B pictures only.
        // A P picture has an L0 reference list, but no colocated direct-mode
        // prediction state. Do not carry the decode-shaped DMV table into the
        // first inter-picture command stream.
        push_packet(batch, &mut idx, &MFX_AVC_REF_IDX_STATE_L0, &mut packet_count)?;
    }
    let mfx_slice = mfx_avc_slice_state(picture);
    push_packet(batch, &mut idx, &mfx_slice, &mut packet_count)?;
    if picture.is_idr {
        push_pak_insert(batch, &mut idx, &SPS, SPS.len() * 8, false, false, 5, false)?;
        packet_count += 1;
        push_pak_insert(batch, &mut idx, &PPS, PPS.len() * 8, false, false, 0, false)?;
        packet_count += 1;
        push_pak_insert(
            batch,
            &mut idx,
            &IDR_SLICE_HEADER,
            IDR_SLICE_HEADER_BITS,
            true,
            true,
            8,
            true,
        )?;
    } else {
        let p_slice_header = p_slice_header(picture.frame_num);
        push_pak_insert(
            batch,
            &mut idx,
            &p_slice_header,
            P_SLICE_HEADER_BITS,
            true,
            true,
            8,
            true,
        )?;
    }
    packet_count += 1;

    push_packet(batch, &mut idx, &VDENC_WEIGHTS_OFFSETS_STATE, &mut packet_count)?;
    push_packet(batch, &mut idx, &VDENC_WALKER_STATE, &mut packet_count)?;
    push_packet(batch, &mut idx, &VD_PIPELINE_FLUSH, &mut packet_count)?;
    push_words(batch, &mut idx, &MI_FLUSH_DW_VIDEO_CACHE_INVALIDATE)?;
    push_words(batch, &mut idx, &MI_FLUSH_DW_NO_POSTSYNC)?;

    emit_store_register_mem_ppgtt(
        batch,
        &mut idx,
        MFC_BITSTREAM_BYTECOUNT_FRAME as u32,
        RESULT_GPU + RESULT_MFC_FRAME_BYTES_SLOT,
    )?;
    emit_store_register_mem_ppgtt(
        batch,
        &mut idx,
        MFX_ERROR_FLAG as u32,
        RESULT_GPU + RESULT_MFX_ERROR_SLOT,
    )?;
    emit_store_register_mem_ppgtt(
        batch,
        &mut idx,
        MFC_IMAGE_STATUS_CONTROL as u32,
        RESULT_GPU + RESULT_MFC_IMAGE_STATUS_SLOT,
    )?;
    emit_store_register_mem_ppgtt(
        batch,
        &mut idx,
        MFC_BITSTREAM_BYTECOUNT_SLICE as u32,
        RESULT_GPU + RESULT_MFC_SLICE_BYTES_SLOT,
    )?;
    emit_store_register_mem_ppgtt(
        batch,
        &mut idx,
        MFC_AVC_NUM_SLICES as u32,
        RESULT_GPU + RESULT_MFC_NUM_SLICES_SLOT,
    )?;

    if !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        RESULT_GPU + media::MEDIA_RESULT_POSTSUBMIT_SLOT,
        CODEC_END_MARKER,
    ) {
        return None;
    }
    // This is a second-level codec batch. Its MI_BATCH_BUFFER_END returns to
    // the primary ring, which writes COMPLETE_MARKER only after the return.
    // Keep the terminal return free of a post-sync MI_FLUSH_DW. The two
    // non-postsync completion barriers above are sufficient and match Intel's
    // Gen12 AVC stream.
    if idx.saturating_add(3) > batch.len() {
        return None;
    }
    batch[idx] = media::MI_ARB_CHECK;
    batch[idx + 1] = media::MI_BATCH_BUFFER_END;
    batch[idx + 2] = media::MI_NOOP;
    idx += 3;
    let batch_bytes = idx * core::mem::size_of::<u32>();
    let expected = if picture.is_idr {
        (EXPECTED_IDR_CODEC_PACKETS, EXPECTED_IDR_BATCH_BYTES)
    } else {
        (EXPECTED_P_CODEC_PACKETS, EXPECTED_P_BATCH_BYTES)
    };
    if (packet_count, batch_bytes) != expected {
        return None;
    }
    Some((batch_bytes, packet_count))
}

fn emit_store_register_mem_ppgtt(
    batch: &mut [u32],
    idx: &mut usize,
    register: u32,
    destination: u64,
) -> Option<()> {
    let end = idx.checked_add(4)?;
    let words = batch.get_mut(*idx..end)?;
    words[0] = MI_STORE_REGISTER_MEM_GEN8_PPGTT;
    words[1] = register;
    words[2] = destination as u32;
    words[3] = (destination >> 32) as u32;
    *idx = end;
    Some(())
}

fn push_packet(
    batch: &mut [u32],
    idx: &mut usize,
    words: &[u32],
    packet_count: &mut usize,
) -> Option<()> {
    push_words(batch, idx, words)?;
    *packet_count = packet_count.saturating_add(1);
    Some(())
}

fn push_words(batch: &mut [u32], idx: &mut usize, words: &[u32]) -> Option<()> {
    let end = idx.checked_add(words.len())?;
    batch.get_mut(*idx..end)?.copy_from_slice(words);
    *idx = end;
    Some(())
}

fn set_addr(words: &mut [u32], dword: usize, gpu: u64) {
    words[dword] = gpu as u32;
    words[dword + 1] = (gpu >> 32) as u32;
}

fn mfx_pipe_buf_addr_state(picture: AvcPicture) -> [u32; 68] {
    let mut words = [0u32; 68];
    words[0] = 0x7002_0042;
    for (dword, gpu, attr_dword, attr) in [
        (1, picture.current_recon_gpu(), 3, 6),
        (4, picture.current_recon_gpu(), 6, 6),
        (7, SOURCE_GPU, 9, 6),
        (10, MFX_STATS_GPU, 12, 6),
        (52, MFX_STATS_GPU, 54, 6),
        (62, picture.current_ds_gpu(), 64, 4),
        (65, SLICE_SIZE_GPU, 67, 12),
    ] {
        set_addr(&mut words, dword, gpu);
        words[attr_dword] = attr;
    }
    if picture.is_idr {
        words[13] = 0x0000_8000;
        words[15] = 0x0000_1000;
        words[16] = 0x0000_c000;
        words[18] = 0x0000_1000;
    } else {
        // ACTHD advances to the end of PIPE_BUF_ADDR_STATE before the first
        // P-picture low-address fault. Keep both MFX row stores in mapped
        // PPGTT storage instead of the internal-cache address window.
        set_addr(&mut words, 13, INTRA_ROWSTORE_GPU);
        words[15] = 6;
        set_addr(&mut words, 16, DEBLOCK_ROWSTORE_GPU);
        words[18] = 6;
    }
    if let Some(reference) = picture.reference_recon_gpu() {
        // MFX exposes sixteen frame-store reference addresses at DW19..DW50.
        // Intel's AVC encode path aliases every unused entry to the first
        // valid reconstruction for error concealment; hardware may touch an
        // entry beyond the one selected by REF_IDX_STATE.  Leaving those
        // entries zero faults the first P picture at a low GPU address.
        for dword in (19..=49).step_by(2) {
            set_addr(&mut words, dword, reference);
        }
        words[51] = 6;
    }
    words
}

fn mfx_ind_obj_base_addr_state() -> [u32; 26] {
    let mut words = [0u32; 26];
    words[0] = 0x7003_0018;
    // Gen12 AVC VDEnc supplies motion-estimation data through VDEnc's own
    // picture resources. MFX_IND_OBJ_BASE_ADDR_STATE binds only the PAK coded
    // output; its decode/IT MV-object input range remains disabled for P.
    set_addr(&mut words, 21, BITSTREAM_GPU);
    words[23] = 10;
    set_addr(&mut words, 24, BITSTREAM_GPU.saturating_add(BITSTREAM_BYTES as u64));
    words
}

fn mfx_bsp_buf_base_addr_state(picture: AvcPicture) -> [u32; 10] {
    let mut words = [0u32; 10];
    words[0] = 0x7004_0008;
    if picture.is_idr {
        words[3] = 0x0000_1000;
        words[4] = 0x0000_4000;
        words[6] = 0x0000_1000;
    } else {
        // The first P-picture trace stops with BBADDR on this command's DW1
        // and a low PPGTT fault. Bind the encoder's BSD/MPC row store
        // explicitly instead of relying on the internal-media cache path.
        set_addr(&mut words, 1, BSP_ROWSTORE_GPU);
        words[3] = 6;
        // Gen12 AVC encode binds only the BSD/MPC row store. MPR is a decoder
        // resource and remains disabled in this mode.
    }
    words
}

fn vdenc_pipe_buf_addr_state(picture: AvcPicture) -> [u32; 71] {
    let mut words = [0u32; 71];
    words[0] = 0x7084_0045;
    for (dword, gpu, attr_dword) in [(10, SOURCE_GPU, 12), (34, VDENC_STATS_GPU, 36)] {
        set_addr(&mut words, dword, gpu);
        words[attr_dword] = 6;
    }
    words[16] = 0x0001_4000;
    words[18] = 0x0000_1000;
    words[61] = 0xc0;
    if let Some(reference) = picture.reference_recon_gpu() {
        set_addr(&mut words, 22, reference);
        words[24] = 6;
    }
    if let Some(reference) = picture.reference_ds_gpu() {
        set_addr(&mut words, 1, reference);
        words[3] = 6;
    }
    words
}

fn p_slice_header(frame_num: u8) -> [u8; 7] {
    debug_assert!(frame_num > 0 && u32::from(frame_num) < GOP_PICTURES);
    [0x00, 0x00, 0x01, 0x41, 0x9a, frame_num << 1, 0x20]
}

fn mfx_qm_state(matrix_type: u32) -> [u32; 18] {
    let mut words = [0u32; 18];
    words[0] = 0x7007_0010;
    words[1] = matrix_type & 3;
    let scaling_words = if matrix_type < 2 { 12 } else { 16 };
    words[2..2 + scaling_words].fill(0x1010_1010);
    words
}

fn mfx_fqm_state(matrix_type: u32) -> [u32; 34] {
    let mut words = [0u32; 34];
    words[0] = 0x7008_0020;
    words[1] = matrix_type & 3;
    let scaling_words = if matrix_type < 2 { 24 } else { 32 };
    words[2..2 + scaling_words].fill(0x1000_1000);
    words
}

fn push_pak_insert(
    batch: &mut [u32],
    idx: &mut usize,
    bytes: &[u8],
    bit_count: usize,
    last_header: bool,
    emulate: bool,
    skip_emulation_bytes: u8,
    slice_header: bool,
) -> Option<()> {
    if bit_count == 0 || bit_count > bytes.len().checked_mul(8)? {
        return None;
    }
    let payload_dwords = bytes.len().div_ceil(4);
    let total_dwords = 2usize.checked_add(payload_dwords)?;
    let start =
        media::begin_batch_packet(batch, idx, total_dwords, 0x7048_0000 | payload_dwords as u32)?;
    let bits_in_last_dword = match bit_count % 32 {
        0 => 32,
        bits => bits,
    };
    let mut control = (bits_in_last_dword as u32) << 8;
    control |= (last_header as u32) << 2;
    control |= (emulate as u32) << 3;
    control |= ((skip_emulation_bytes as u32) & 0x0f) << 4;
    control |= (slice_header as u32) << 14;
    if !emulate {
        control |= 1 << 15;
    }
    batch[start + 1] = control;
    for (byte_index, byte) in bytes.iter().copied().enumerate() {
        batch[start + 2 + byte_index / 4] |= (byte as u32) << ((byte_index % 4) * 8);
    }
    Some(())
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for byte in bytes {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// A bounded change detector for GPU-produced frame telemetry. Sampling keeps
/// validation from turning the eliminated full-frame CPU copy into a disguised
/// full-frame CPU readback.
fn fnv1a32_sampled(bytes: &[u8]) -> u32 {
    const MAX_SAMPLES: usize = 4096;
    let stride = bytes.len().div_ceil(MAX_SAMPLES).max(1);
    let mut hash = 0x811c_9dc5u32 ^ bytes.len() as u32;
    for byte in bytes.iter().step_by(stride) {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn annex_b_nal_flags(bytes: &[u8]) -> u8 {
    let mut flags = 0u8;
    let mut cursor = 0usize;
    while cursor + 4 <= bytes.len() {
        let header = if bytes[cursor..].starts_with(&[0, 0, 0, 1]) {
            cursor.checked_add(4)
        } else if bytes[cursor..].starts_with(&[0, 0, 1]) {
            cursor.checked_add(3)
        } else {
            None
        };
        let Some(header) = header else {
            cursor += 1;
            continue;
        };
        let Some(nal_header) = bytes.get(header) else {
            break;
        };
        flags |= match nal_header & 0x1f {
            7 => 1 << 0,
            8 => 1 << 1,
            5 => 1 << 2,
            1 => 1 << 3,
            _ => 0,
        };
        cursor = header + 1;
    }
    flags
}

fn capture_timeout_diagnostics(
    dev: crate::intel::Dev,
    engine: media::MediaEngineDescriptor,
    backing: &ProbeBacking,
    picture: AvcPicture,
) -> AvcEncodeTimeoutDiagnostics {
    let bitstream_virt = unsafe { backing.arena_virt.add(BITSTREAM_OFFSET) };
    let mfx_stats_virt = unsafe { backing.arena_virt.add(MFX_STATS_OFFSET) };
    let vdenc_stats_virt = unsafe { backing.arena_virt.add(VDENC_STATS_OFFSET) };
    let slice_size_virt = unsafe { backing.arena_virt.add(SLICE_SIZE_OFFSET) };
    crate::intel::dma_flush(bitstream_virt, 8 * core::mem::size_of::<u32>());
    crate::intel::dma_flush(mfx_stats_virt, 4 * core::mem::size_of::<u32>());
    crate::intel::dma_flush(vdenc_stats_virt, 4 * core::mem::size_of::<u32>());
    crate::intel::dma_flush(slice_size_virt, 4 * core::mem::size_of::<u32>());

    let base = engine.ring_base;
    let ring_acthd_lo = crate::intel::mmio_read(dev, base + media::RING_ACTHD);
    let ring_acthd_hi = crate::intel::mmio_read(dev, base + media::RING_ACTHD_UDW);
    let acthd = ((ring_acthd_hi as u64) << 32) | ring_acthd_lo as u64;
    let (acthd_region, acthd_offset_bytes, acthd_dword) = classify_acthd(acthd, backing);

    AvcEncodeTimeoutDiagnostics {
        valid: true,
        ring_start: crate::intel::mmio_read(dev, base + media::RING_START),
        ring_ctl: crate::intel::mmio_read(dev, base + media::RING_CTL),
        ring_head: crate::intel::mmio_read(dev, base + media::RING_HEAD),
        ring_tail: crate::intel::mmio_read(dev, base + media::RING_TAIL),
        ring_acthd_lo,
        ring_acthd_hi,
        acthd_region,
        acthd_offset_bytes,
        acthd_dword,
        bbaddr_lo: crate::intel::mmio_read(dev, base + media::RING_BBADDR),
        bbaddr_hi: crate::intel::mmio_read(dev, base + media::RING_BBADDR_UDW),
        dma_fadd_lo: crate::intel::mmio_read(dev, base + media::RING_DMA_FADD),
        dma_fadd_hi: crate::intel::mmio_read(dev, base + media::RING_DMA_FADD_UDW),
        bbstate: crate::intel::mmio_read(dev, base + media::RING_BBSTATE),
        esr: crate::intel::mmio_read(dev, base + media::RING_ESR),
        instdone: crate::intel::mmio_read(dev, base + media::RING_INSTDONE),
        instps: crate::intel::mmio_read(dev, base + media::RING_INSTPS),
        psmi_ctl: crate::intel::mmio_read(dev, base + media::RING_PSMI_CTL),
        nopid: crate::intel::mmio_read(dev, base + media::RING_NOPID),
        ipeir: crate::intel::mmio_read(dev, base + media::RING_IPEIR),
        ipehr: crate::intel::mmio_read(dev, base + media::RING_IPEHR),
        fault_gen8: crate::intel::mmio_read(dev, GEN8_RING_FAULT_REG),
        fault_gen12: crate::intel::mmio_read(dev, media::GEN12_RING_FAULT_REG),
        fault_tlb_data0_gen8: crate::intel::mmio_read(dev, GEN8_FAULT_TLB_DATA0),
        fault_tlb_data1_gen8: crate::intel::mmio_read(dev, GEN8_FAULT_TLB_DATA1),
        fault_tlb_data0_gen12: crate::intel::mmio_read(dev, GEN12_FAULT_TLB_DATA0),
        fault_tlb_data1_gen12: crate::intel::mmio_read(dev, GEN12_FAULT_TLB_DATA1),
        mfx_error: crate::intel::mmio_read(dev, MFX_ERROR_FLAG),
        mfx_frame_crc: crate::intel::mmio_read(dev, MFX_FRAME_CRC),
        mfx_mb_count: crate::intel::mmio_read(dev, MFX_MB_COUNT),
        mfc_bitstream_bytecount_frame: crate::intel::mmio_read(dev, MFC_BITSTREAM_BYTECOUNT_FRAME),
        mfc_bitstream_se_bitcount_frame: crate::intel::mmio_read(
            dev,
            MFC_BITSTREAM_SE_BITCOUNT_FRAME,
        ),
        mfc_bitstream_bytecount_slice: crate::intel::mmio_read(dev, MFC_BITSTREAM_BYTECOUNT_SLICE),
        mfc_image_status_mask: crate::intel::mmio_read(dev, MFC_IMAGE_STATUS_MASK),
        mfc_image_status_control: crate::intel::mmio_read(dev, MFC_IMAGE_STATUS_CONTROL),
        mfc_qp_status_count: crate::intel::mmio_read(dev, MFC_QP_STATUS_COUNT),
        mfc_avc_num_slices: crate::intel::mmio_read(dev, MFC_AVC_NUM_SLICES),
        bitstream_head: read_dword_head::<8>(bitstream_virt),
        mfx_stats_head: read_dword_head::<4>(mfx_stats_virt),
        vdenc_stats_head: read_dword_head::<4>(vdenc_stats_virt),
        slice_size_head: read_dword_head::<4>(slice_size_virt),
        current_recon_sample: sampled_gpu_surface(
            backing,
            picture.current_recon_gpu(),
            RECON_BYTES,
        ),
        reference_recon_sample: picture
            .reference_recon_gpu()
            .map_or(0, |gpu| sampled_gpu_surface(backing, gpu, RECON_BYTES)),
        current_ds_sample: sampled_gpu_surface(backing, picture.current_ds_gpu(), DS_BYTES),
        reference_ds_sample: picture
            .reference_ds_gpu()
            .map_or(0, |gpu| sampled_gpu_surface(backing, gpu, DS_BYTES)),
    }
}

fn sampled_gpu_surface(backing: &ProbeBacking, gpu: u64, bytes: usize) -> u32 {
    let Some(offset) = gpu.checked_sub(ARENA_GPU).map(|offset| offset as usize) else {
        return 0;
    };
    let Some(end) = offset.checked_add(bytes) else {
        return 0;
    };
    if end > ARENA_BYTES {
        return 0;
    }
    let ptr = unsafe { backing.arena_virt.add(offset) };
    crate::intel::dma_flush(ptr, bytes);
    fnv1a32_sampled(unsafe { core::slice::from_raw_parts(ptr, bytes) })
}

fn classify_acthd(acthd: u64, backing: &ProbeBacking) -> (&'static str, u32, u32) {
    if let Some(offset) = acthd.checked_sub(BATCH_GPU) {
        if offset < BATCH_BYTES as u64 {
            let offset = offset as usize;
            let dword = unsafe {
                core::ptr::read_volatile(backing.arena_virt.add(BATCH_OFFSET + offset).cast())
            };
            return ("batch", offset as u32, dword);
        }
    }
    if let Some(offset) = acthd.checked_sub(RING_GPU) {
        if offset < RING_BYTES as u64 {
            let offset = offset as usize;
            let dword = unsafe { core::ptr::read_volatile(backing.ring_virt.add(offset).cast()) };
            return ("ring", offset as u32, dword);
        }
    }
    ("other", 0, 0)
}

fn read_dword_head<const N: usize>(ptr: *mut u8) -> [u32; N] {
    let mut words = [0u32; N];
    for (index, word) in words.iter_mut().enumerate() {
        *word = unsafe { core::ptr::read_volatile(ptr.add(index * 4).cast::<u32>()) };
    }
    words
}

fn deferred(failure: AvcEncodeProbeFailure) -> AvcEncodeProbeReport {
    let report = AvcEncodeProbeReport {
        state: AvcEncodeProbeState::Deferred,
        failure,
        ..AvcEncodeProbeReport::EMPTY
    };
    *REPORT.lock() = report;
    report
}

fn fail(
    mut report: AvcEncodeProbeReport,
    failure: AvcEncodeProbeFailure,
    started_ns: u64,
) -> AvcEncodeProbeReport {
    report.state = AvcEncodeProbeState::Failed;
    report.failure = failure;
    report.elapsed_us = elapsed_us(started_ns);
    publish(report);
    report
}

fn quarantine(
    lane: media::MediaLaneGuard,
    mut report: AvcEncodeProbeReport,
    failure: AvcEncodeProbeFailure,
    started_ns: u64,
) -> AvcEncodeProbeReport {
    lane.quarantine();
    report.state = AvcEncodeProbeState::Quarantined;
    report.failure = failure;
    report.elapsed_us = elapsed_us(started_ns);
    publish(report);
    report
}

fn publish(report: AvcEncodeProbeReport) {
    *REPORT.lock() = report;
    STATE.store(report.state as u8, Ordering::Release);
}

fn elapsed_us(started_ns: u64) -> u64 {
    crate::chronos::monotonic_nanos().saturating_sub(started_ns) / 1_000
}
