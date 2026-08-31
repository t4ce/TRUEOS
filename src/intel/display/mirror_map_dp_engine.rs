//! MirrorMapDPEngine: Pipe A plane-map reflection into headless Pipe C -> WD0.
//!
//! Pipe A remains the wired monitor. Pipe C re-fetches the same GGTT plane and
//! cursor surfaces through DBUF S2; WD0 is its only transcoder/sink and writes
//! the fixed capture allocation. No CPU or GPU compositor participates here.

use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use spin::Mutex;

use super::*;

const SOURCE_PIPE_SLOT: usize = 0;
const MIRROR_PIPE_SLOT: usize = 2;
const CAPTURE_GPU: u64 = 0xE000_0000;
// WD color mode 2 is DRM XYUV8888: X:Y:Cb:Cr in a little-endian u32.  Gen12
// VDEnc's packed YUV444 source has the identical byte/component positions
// (A:Y:U:V); the unused X byte is accepted in the alpha position.
const CAPTURE_BYTES_PER_PIXEL: u32 = 4;
const CAPTURE_ALIGNMENT: usize = 4096;

const DBUF_CTL_S2: usize = 0x44FE8;
const DBUF_POWER_REQUEST: u32 = 1 << 31;
const DBUF_POWER_STATE: u32 = 1 << 30;

const WD0_HTOTAL: usize = 0x6E000;
const WD0_VTOTAL: usize = 0x6E00C;
const WD0_TRANS_CONF: usize = 0x7E008;
const WD0_FUNC_CTL: usize = 0x6E400;
const WD0_TAIL_CFG: usize = 0x6E520;
const WD0_STRIDE: usize = 0x6E510;
const WD0_SURF: usize = 0x6E514;
const WD0_FRAME_STATUS: usize = 0x6E568;

const WD_TRANS_ENABLE: u32 = 1 << 31;
const WD_TRANS_STATE: u32 = 1 << 30;
const WD_FUNC_ENABLE: u32 = 1 << 31;
const WD_TRIGGERED_CAPTURE_ENABLE: u32 = 1 << 30;
const WD_START_TRIGGER_FRAME: u32 = 1 << 29;
const WD_STOP_TRIGGER_FRAME: u32 = 1 << 28;
const WD_COLOR_MODE_XYUV8888: u32 = 2 << 20;
const WD_DISABLE_POINTERS: u32 = 3 << 18;
const WD_INPUT_PIPE_C: u32 = 6 << 12;
const WD_FRAME_NUMBER_MASK: u32 = 0xF;
const WD_FRAME_COMPLETE: u32 = 1 << 31;

// Pipe C's blender produces RGB. WD color mode 2 only defines how those
// components are stored; it does not itself perform RGB -> YCbCr conversion.
// Use the ICL+ pipe output CSC immediately after blending so WD receives real
// opaque, limited-range BT.709 XYUV8888 (memory bytes V, U, Y, X).
const PIPE_MISC_A: usize = 0x70030;
const PIPE_MISC_OUTPUT_COLORSPACE_YUV: u32 = 1 << 11;
const PIPE_MISC_YUV420_ENABLE: u32 = 1 << 27;
const PIPE_MISC_YUV420_MODE: u32 = 1 << 26;
const PIPE_CSC_MODE_A: usize = 0x49028;
const PIPE_CSC_REGISTER_STRIDE: usize = 0x100;
const PIPE_CSC_ENABLE: u32 = 1 << 31;
const PIPE_OUTPUT_CSC_ENABLE: u32 = 1 << 30;
const PIPE_OUTPUT_CSC_COEFF_A: usize = 0x49050;
const PIPE_OUTPUT_CSC_PREOFF_A: usize = 0x49068;
const PIPE_OUTPUT_CSC_POSTOFF_A: usize = 0x49074;

// Intel's fixed-point BT.709 full-range RGB -> limited-range YCbCr matrix.
// Output CSC channel order is Cr/Y/Cb, matching WD XYUV's V/Y/U bits.
const RGB_TO_LIMITED_BT709_COEFF: [u16; 9] = [
    0x1E08, 0x9CC0, 0xB528, 0x2BA8, 0x09D8, 0x37E8, 0xBCE8, 0x9AD8, 0x1E08,
];
const RGB_TO_LIMITED_BT709_POSTOFF: [u32; 3] = [0x0800, 0x0100, 0x0800];

const CURSOR_A_BASE: usize = 0x70080;
const CURSOR_PIPE_STRIDE: usize = 0x1000;
const CURSOR_CTL_OFF: usize = 0x00;
const CURSOR_BASE_OFF: usize = 0x04;
const CURSOR_POS_OFF: usize = 0x08;
const CURSOR_FBC_CTL_OFF: usize = 0x20;
const CURSOR_SURFLIVE_OFF: usize = 0x2C;
const CURSOR_WM_A0: usize = 0x70140;
const CURSOR_WM_A1: usize = 0x70144;
const CURSOR_WM_TRANS_A: usize = 0x70168;
const CURSOR_WM_SAGV_A: usize = 0x70158;
const CURSOR_WM_SAGV_TRANS_A: usize = 0x7015C;
const CURSOR_BUF_CFG_A: usize = 0x7017C;
const SEL_FETCH_CUR_CTL_A: usize = 0x70880;
const CURSOR_WM_ENABLE: u32 = 1 << 31;

const POWER_WELL_FIRST_INDEX: usize = 0;
const POWER_WELL_LAST_INDEX: usize = 3;
const POWER_WAIT_ITERS: usize = 1_000_000;
const PIPE_WAIT_ITERS: usize = 1_000_000;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WdCaptureError {
    DeviceUnavailable,
    UnsupportedDevice,
    SourcePipeUnavailable,
    MirrorPipeBusy,
    DimensionsInvalid,
    AllocationFailed,
    MappingFailed,
    PowerWellTimeout,
    DbufTimeout,
    WdEnableTimeout,
    AlreadyCapturing,
    NotRunning,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct WdXyuv8888Frame {
    pub(crate) sequence: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
    pub(crate) byte_len: usize,
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) virt: *mut u8,
}

unsafe impl Send for WdXyuv8888Frame {}
unsafe impl Sync for WdXyuv8888Frame {}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WdCapturePoll {
    Idle,
    Pending,
    Complete(WdXyuv8888Frame),
    Failed { status: u32 },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct WdCaptureStatus {
    pub(crate) running: bool,
    pub(crate) capture_pending: bool,
    pub(crate) sequence: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
    pub(crate) wd_func_ctl: u32,
    pub(crate) wd_trans_conf: u32,
    pub(crate) wd_frame_status: u32,
    pub(crate) transcoder_c_conf: u32,
    pub(crate) map_mode: MirrorMapMode,
}

/// The complete first-version mapping policy. Slots 0 and 4 are invariant;
/// Spirit's dedicated hardware cursor slot 5 is mirrored separately and is
/// invariant too. Only the three ordinary single-frame slots may permute.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum MirrorMapMode {
    #[default]
    Identity = 0,
    Swap1And3 = 1,
    Swap2And3 = 2,
}

impl MirrorMapMode {
    const fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::Swap1And3,
            2 => Self::Swap2And3,
            _ => Self::Identity,
        }
    }

    /// Return the Pipe A source slot feeding one Pipe C destination slot.
    const fn source_for_destination(self, destination: usize) -> usize {
        match (self, destination) {
            (Self::Swap1And3, 1) => 3,
            (Self::Swap1And3, 3) => 1,
            (Self::Swap2And3, 2) => 3,
            (Self::Swap2And3, 3) => 2,
            _ => destination,
        }
    }
}

#[derive(Copy, Clone)]
struct WdCaptureState {
    running: bool,
    pending: bool,
    width: u32,
    height: u32,
    pitch_bytes: u32,
    byte_len: usize,
    phys: u64,
    virt: *mut u8,
    frame_number: u8,
    sequence: u64,
    saved_power_well_ctl2: u32,
    saved_dbuf_ctl_s2: u32,
    saved_output_color: PipeOutputColorState,
}

#[derive(Copy, Clone)]
struct PipeOutputColorState {
    pipe_misc: u32,
    csc_mode: u32,
    coeff: [u32; 6],
    preoff: [u32; 3],
    postoff: [u32; 3],
}

impl PipeOutputColorState {
    const fn new() -> Self {
        Self {
            pipe_misc: 0,
            csc_mode: 0,
            coeff: [0; 6],
            preoff: [0; 3],
            postoff: [0; 3],
        }
    }
}

unsafe impl Send for WdCaptureState {}

impl WdCaptureState {
    const fn new() -> Self {
        Self {
            running: false,
            pending: false,
            width: 0,
            height: 0,
            pitch_bytes: 0,
            byte_len: 0,
            phys: 0,
            virt: core::ptr::null_mut(),
            frame_number: 1,
            sequence: 0,
            saved_power_well_ctl2: 0,
            saved_dbuf_ctl_s2: 0,
            saved_output_color: PipeOutputColorState::new(),
        }
    }

    fn frame(self) -> WdXyuv8888Frame {
        WdXyuv8888Frame {
            sequence: self.sequence,
            width: self.width,
            height: self.height,
            pitch_bytes: self.pitch_bytes,
            byte_len: self.byte_len,
            phys: self.phys,
            gpu: CAPTURE_GPU,
            virt: self.virt,
        }
    }
}

fn pipe_c_output_color_registers() -> (usize, usize, usize, usize, usize) {
    let csc_offset = MIRROR_PIPE_SLOT * PIPE_CSC_REGISTER_STRIDE;
    (
        PIPE_MISC_A + MIRROR_PIPE_SLOT * PIPE_MMIO_STRIDE,
        PIPE_CSC_MODE_A + csc_offset,
        PIPE_OUTPUT_CSC_COEFF_A + csc_offset,
        PIPE_OUTPUT_CSC_PREOFF_A + csc_offset,
        PIPE_OUTPUT_CSC_POSTOFF_A + csc_offset,
    )
}

fn program_pipe_c_output_bt709_ycbcr(dev: crate::intel::Dev) -> PipeOutputColorState {
    let (pipe_misc, csc_mode, coeff_base, preoff_base, postoff_base) =
        pipe_c_output_color_registers();
    let mut saved = PipeOutputColorState::new();
    saved.pipe_misc = crate::intel::mmio_read(dev, pipe_misc);
    saved.csc_mode = crate::intel::mmio_read(dev, csc_mode);
    for (index, value) in saved.coeff.iter_mut().enumerate() {
        *value = crate::intel::mmio_read(dev, coeff_base + index * 4);
    }
    for (index, value) in saved.preoff.iter_mut().enumerate() {
        *value = crate::intel::mmio_read(dev, preoff_base + index * 4);
    }
    for (index, value) in saved.postoff.iter_mut().enumerate() {
        *value = crate::intel::mmio_read(dev, postoff_base + index * 4);
    }

    let coeff = RGB_TO_LIMITED_BT709_COEFF;
    let packed = [
        (u32::from(coeff[0]) << 16) | u32::from(coeff[1]),
        u32::from(coeff[2]) << 16,
        (u32::from(coeff[3]) << 16) | u32::from(coeff[4]),
        u32::from(coeff[5]) << 16,
        (u32::from(coeff[6]) << 16) | u32::from(coeff[7]),
        u32::from(coeff[8]) << 16,
    ];
    for (index, value) in packed.into_iter().enumerate() {
        crate::intel::mmio_write(dev, coeff_base + index * 4, value);
    }
    for index in 0..3 {
        crate::intel::mmio_write(dev, preoff_base + index * 4, 0);
        crate::intel::mmio_write(
            dev,
            postoff_base + index * 4,
            RGB_TO_LIMITED_BT709_POSTOFF[index],
        );
    }
    crate::intel::mmio_write(
        dev,
        pipe_misc,
        (saved.pipe_misc & !(PIPE_MISC_YUV420_ENABLE | PIPE_MISC_YUV420_MODE))
            | PIPE_MISC_OUTPUT_COLORSPACE_YUV,
    );
    // PIPE_CSC_MODE arms the double-buffered coefficient and offset banks.
    crate::intel::mmio_write(
        dev,
        csc_mode,
        (saved.csc_mode & !(PIPE_CSC_ENABLE | PIPE_OUTPUT_CSC_ENABLE)) | PIPE_OUTPUT_CSC_ENABLE,
    );
    saved
}

fn restore_pipe_c_output_color(dev: crate::intel::Dev, saved: PipeOutputColorState) {
    let (pipe_misc, csc_mode, coeff_base, preoff_base, postoff_base) =
        pipe_c_output_color_registers();
    for (index, value) in saved.coeff.into_iter().enumerate() {
        crate::intel::mmio_write(dev, coeff_base + index * 4, value);
    }
    for (index, value) in saved.preoff.into_iter().enumerate() {
        crate::intel::mmio_write(dev, preoff_base + index * 4, value);
    }
    for (index, value) in saved.postoff.into_iter().enumerate() {
        crate::intel::mmio_write(dev, postoff_base + index * 4, value);
    }
    crate::intel::mmio_write(dev, pipe_misc, saved.pipe_misc);
    crate::intel::mmio_write(dev, csc_mode, saved.csc_mode);
}

static STATE: Mutex<WdCaptureState> = Mutex::new(WdCaptureState::new());
static CAPTURE_TIMEOUTS: AtomicU64 = AtomicU64::new(0);
static MAP_MODE: AtomicU8 = AtomicU8::new(MirrorMapMode::Identity as u8);

pub(crate) fn set_mirror_map_mode(mode: MirrorMapMode) {
    MAP_MODE.store(mode as u8, Ordering::Release);
}

fn mirror_map_mode() -> MirrorMapMode {
    MirrorMapMode::from_raw(MAP_MODE.load(Ordering::Acquire))
}

const fn power_well_request(index: usize) -> u32 {
    2 << (index * 2)
}

const fn power_well_state(index: usize) -> u32 {
    1 << (index * 2)
}

fn wait_for_mask(
    dev: crate::intel::Dev,
    register: usize,
    mask: u32,
    set: bool,
    iters: usize,
) -> bool {
    for _ in 0..iters {
        if (crate::intel::mmio_read(dev, register) & mask != 0) == set {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn request_pipe_c_power(dev: crate::intel::Dev) -> Result<u32, WdCaptureError> {
    let saved = crate::intel::mmio_read(dev, HSW_PWR_WELL_CTL2);
    let mut requested = saved;
    for index in POWER_WELL_FIRST_INDEX..=POWER_WELL_LAST_INDEX {
        requested |= power_well_request(index);
        crate::intel::mmio_write(dev, HSW_PWR_WELL_CTL2, requested);
        if !wait_for_mask(dev, HSW_PWR_WELL_CTL2, power_well_state(index), true, POWER_WAIT_ITERS) {
            crate::intel::mmio_write(dev, HSW_PWR_WELL_CTL2, saved);
            return Err(WdCaptureError::PowerWellTimeout);
        }
    }
    Ok(saved)
}

fn enable_dbuf_s2(dev: crate::intel::Dev) -> Result<u32, WdCaptureError> {
    let saved = crate::intel::mmio_read(dev, DBUF_CTL_S2);
    crate::intel::mmio_write(dev, DBUF_CTL_S2, saved | DBUF_POWER_REQUEST);
    if !wait_for_mask(dev, DBUF_CTL_S2, DBUF_POWER_STATE, true, POWER_WAIT_ITERS) {
        crate::intel::mmio_write(dev, DBUF_CTL_S2, saved);
        return Err(WdCaptureError::DbufTimeout);
    }
    Ok(saved)
}

fn clone_plane_contract(
    dev: crate::intel::Dev,
    source: PipeInfo,
    mirror: PipeInfo,
    source_slot: usize,
    destination_slot: usize,
    dbuf_start: u16,
    dbuf_end: u16,
) {
    let src = source.plane(source_slot).base();
    let dst = mirror.plane(destination_slot).base();
    let ctl = crate::intel::mmio_read(dev, src + UNI_PLANE_CTL_OFF);
    crate::intel::mmio_write(dev, dst + UNI_PLANE_CTL_OFF, ctl & !PLANE_CTL_ENABLE);
    for offset in [
        UNI_PLANE_STRIDE_OFF,
        UNI_PLANE_POS_OFF,
        UNI_PLANE_SIZE_OFF,
        UNI_PLANE_KEYVAL_OFF,
        UNI_PLANE_KEYMSK_OFF,
        UNI_PLANE_KEYMAX_OFF,
        UNI_PLANE_OFFSET_OFF,
        UNI_PLANE_AUX_DIST_OFF,
        UNI_PLANE_AUX_OFFSET_OFF,
        UNI_PLANE_CUS_CTL_OFF,
        UNI_PLANE_COLOR_CTL_OFF,
    ] {
        crate::intel::mmio_write(dev, dst + offset, crate::intel::mmio_read(dev, src + offset));
    }
    // COLOR_CTL can enable the programmable input CSC. Mirroring only its
    // enable/mode bits while leaving Pipe C's coefficient banks at reset does
    // not reproduce Pipe A's pixel contract.
    for offset in (UNI_PLANE_INPUT_CSC_COEFF_OFF..=UNI_PLANE_INPUT_CSC_COEFF_OFF + 20).step_by(4) {
        crate::intel::mmio_write(dev, dst + offset, crate::intel::mmio_read(dev, src + offset));
    }
    for offset in (UNI_PLANE_INPUT_CSC_PREOFF_OFF..=UNI_PLANE_INPUT_CSC_PREOFF_OFF + 8).step_by(4) {
        crate::intel::mmio_write(dev, dst + offset, crate::intel::mmio_read(dev, src + offset));
    }
    for offset in (UNI_PLANE_INPUT_CSC_POSTOFF_OFF..=UNI_PLANE_INPUT_CSC_POSTOFF_OFF + 8).step_by(4)
    {
        crate::intel::mmio_write(dev, dst + offset, crate::intel::mmio_read(dev, src + offset));
    }
    program_plane_watermark_boot_safe(dev, dst, true);
    let _ = program_plane_buf_cfg(dev, dst, dbuf_start, dbuf_end);
    crate::intel::mmio_write(dev, dst + UNI_PLANE_CTL_OFF, ctl);
    // Mirror the allocation actually being scanned out, never Pipe A's queued
    // next SURF. The write becomes Pipe C's first double-buffered commit.
    crate::intel::mmio_write(
        dev,
        dst + UNI_PLANE_SURF_OFF,
        crate::intel::mmio_read(dev, src + UNI_PLANE_SURFLIVE_OFF),
    );
}

fn mirror_cursor(dev: crate::intel::Dev) {
    let src = CURSOR_A_BASE + SOURCE_PIPE_SLOT * CURSOR_PIPE_STRIDE;
    let dst = CURSOR_A_BASE + MIRROR_PIPE_SLOT * CURSOR_PIPE_STRIDE;
    let ctl = crate::intel::mmio_read(dev, src + CURSOR_CTL_OFF);
    crate::intel::mmio_write(dev, dst + CURSOR_CTL_OFF, 0);
    crate::intel::mmio_write(
        dev,
        dst + CURSOR_POS_OFF,
        crate::intel::mmio_read(dev, src + CURSOR_POS_OFF),
    );
    let source_wm0 =
        crate::intel::mmio_read(dev, CURSOR_WM_A0 + SOURCE_PIPE_SLOT * CURSOR_PIPE_STRIDE);
    // The cursor register unit can consume WM1 even when only WM0 is enabled.
    // Keep WM1 disabled but shadow WM0's payload, as required by Intel's
    // permanent cursor-underrun workaround.
    crate::intel::mmio_write(
        dev,
        CURSOR_WM_A1 + MIRROR_PIPE_SLOT * CURSOR_PIPE_STRIDE,
        source_wm0 & !CURSOR_WM_ENABLE,
    );
    crate::intel::mmio_write(dev, CURSOR_WM_A0 + MIRROR_PIPE_SLOT * CURSOR_PIPE_STRIDE, source_wm0);
    for base in [CURSOR_WM_TRANS_A, CURSOR_WM_SAGV_A, CURSOR_WM_SAGV_TRANS_A] {
        crate::intel::mmio_write(
            dev,
            base + MIRROR_PIPE_SLOT * CURSOR_PIPE_STRIDE,
            crate::intel::mmio_read(dev, base + SOURCE_PIPE_SLOT * CURSOR_PIPE_STRIDE),
        );
    }
    // Pipe C owns blocks 2040..2047; do not clone Pipe A's S1 allocation.
    crate::intel::mmio_write(
        dev,
        CURSOR_BUF_CFG_A + MIRROR_PIPE_SLOT * CURSOR_PIPE_STRIDE,
        ((2047u32) << 16) | 2040,
    );
    crate::intel::mmio_write(
        dev,
        SEL_FETCH_CUR_CTL_A + MIRROR_PIPE_SLOT * CURSOR_PIPE_STRIDE,
        crate::intel::mmio_read(dev, SEL_FETCH_CUR_CTL_A + SOURCE_PIPE_SLOT * CURSOR_PIPE_STRIDE),
    );
    // The mirrored cursor always uses its full mode-defined height. Clear any
    // soft-reset cursor-size-reduction state before CUR_BASE arms the update.
    crate::intel::mmio_write(dev, dst + CURSOR_FBC_CTL_OFF, 0);
    crate::intel::mmio_write(dev, dst + CURSOR_CTL_OFF, ctl);
    crate::intel::mmio_write(
        dev,
        dst + CURSOR_BASE_OFF,
        crate::intel::mmio_read(dev, src + CURSOR_SURFLIVE_OFF),
    );
}

fn mirror_live_scene(dev: crate::intel::Dev) {
    let source = PIPES[SOURCE_PIPE_SLOT];
    let mirror = PIPES[MIRROR_PIPE_SLOT];
    // UI4's color picker owns Pipe A's programmable bottom color. It can
    // change while WD remains resident, so copy it for every triggered frame
    // alongside the double-buffered plane state.
    crate::intel::mmio_write(
        dev,
        SKL_BOTTOM_COLOR_A + mirror.slot * SKL_BOTTOM_COLOR_PIPE_STRIDE,
        crate::intel::mmio_read(
            dev,
            SKL_BOTTOM_COLOR_A + source.slot * SKL_BOTTOM_COLOR_PIPE_STRIDE,
        ),
    );
    let ranges = [
        (PLANE_DBUF_S2_SLOT_0_START, PLANE_DBUF_S2_SLOT_0_END),
        (PLANE_DBUF_S2_SLOT_1_START, PLANE_DBUF_S2_SLOT_1_END),
        (PLANE_DBUF_S2_SLOT_2_START, PLANE_DBUF_S2_SLOT_2_END),
        (PLANE_DBUF_S2_SLOT_3_START, PLANE_DBUF_S2_SLOT_3_END),
        (PLANE_DBUF_S2_SLOT_4_START, PLANE_DBUF_S2_SLOT_4_END),
    ];
    let mode = mirror_map_mode();
    for (destination_slot, (start, end)) in ranges.into_iter().enumerate() {
        clone_plane_contract(
            dev,
            source,
            mirror,
            mode.source_for_destination(destination_slot),
            destination_slot,
            start,
            end,
        );
    }
    mirror_cursor(dev);
}

fn aligned_capture_layout(width: u32, height: u32) -> Option<(u32, usize)> {
    let pitch = width
        .checked_mul(CAPTURE_BYTES_PER_PIXEL)?
        .checked_add(63)?
        & !63;
    let raw = usize::try_from(u64::from(pitch).checked_mul(u64::from(height))?).ok()?;
    let byte_len = raw.checked_add(CAPTURE_ALIGNMENT - 1)? & !(CAPTURE_ALIGNMENT - 1);
    Some((pitch, byte_len))
}

pub(crate) fn start_ui4_wd_xyuv8888_capture() -> Result<WdXyuv8888Frame, WdCaptureError> {
    let dev = crate::intel::claimed_device().ok_or(WdCaptureError::DeviceUnavailable)?;
    if dev.device_id != 0x4680 {
        return Err(WdCaptureError::UnsupportedDevice);
    }
    let mut state = STATE.lock();
    if state.running {
        return Ok(state.frame());
    }
    let source = PIPES[SOURCE_PIPE_SLOT];
    let mirror = PIPES[MIRROR_PIPE_SLOT];
    if crate::intel::mmio_read(dev, PIPECONF_A + source.slot * PIPE_MMIO_STRIDE) & PIPECONF_STATE
        == 0
    {
        return Err(WdCaptureError::SourcePipeUnavailable);
    }
    if crate::intel::mmio_read(dev, PIPECONF_A + mirror.slot * PIPE_MMIO_STRIDE) & PIPECONF_STATE
        != 0
        || crate::intel::mmio_read(dev, TRANS_DDI_FUNC_CTL_A + mirror.slot * PIPE_MMIO_STRIDE)
            & (1 << 31)
            != 0
        || crate::intel::mmio_read(dev, WD0_TRANS_CONF) & WD_TRANS_STATE != 0
        || crate::intel::mmio_read(dev, WD0_FUNC_CTL) & WD_FUNC_ENABLE != 0
    {
        return Err(WdCaptureError::MirrorPipeBusy);
    }
    let (width, height) = decode_pipe_src(crate::intel::mmio_read(dev, source.pipe_src_off))
        .ok_or(WdCaptureError::DimensionsInvalid)?;
    let (pitch_bytes, byte_len) =
        aligned_capture_layout(width, height).ok_or(WdCaptureError::DimensionsInvalid)?;
    let (phys, virt) =
        crate::dma::alloc(byte_len, CAPTURE_ALIGNMENT).ok_or(WdCaptureError::AllocationFailed)?;
    unsafe { core::ptr::write_bytes(virt, 0, byte_len) };
    crate::intel::dma_flush(virt, byte_len);
    if !crate::intel::map_display_scanout_ggtt(dev, phys, byte_len, CAPTURE_GPU) {
        crate::dma::dealloc(virt, byte_len);
        return Err(WdCaptureError::MappingFailed);
    }
    crate::intel::ggtt_invalidate(dev);

    let saved_power_well_ctl2 = match request_pipe_c_power(dev) {
        Ok(saved) => saved,
        Err(error) => {
            let _ = crate::intel::unmap_display_scanout_ggtt(dev, byte_len, CAPTURE_GPU);
            crate::dma::dealloc(virt, byte_len);
            return Err(error);
        }
    };
    let saved_dbuf_ctl_s2 = match enable_dbuf_s2(dev) {
        Ok(saved) => saved,
        Err(error) => {
            crate::intel::mmio_write(dev, HSW_PWR_WELL_CTL2, saved_power_well_ctl2);
            let _ = crate::intel::unmap_display_scanout_ggtt(dev, byte_len, CAPTURE_GPU);
            crate::dma::dealloc(virt, byte_len);
            return Err(error);
        }
    };

    crate::intel::mmio_write(
        dev,
        mirror.pipe_src_off,
        crate::intel::mmio_read(dev, source.pipe_src_off),
    );
    crate::intel::mmio_write(
        dev,
        SKL_BOTTOM_COLOR_A + mirror.slot * SKL_BOTTOM_COLOR_PIPE_STRIDE,
        crate::intel::mmio_read(
            dev,
            SKL_BOTTOM_COLOR_A + source.slot * SKL_BOTTOM_COLOR_PIPE_STRIDE,
        ),
    );
    let chicken = crate::intel::mmio_read(dev, PIPE_CHICKEN_A + mirror.slot * PIPE_MMIO_STRIDE)
        | PIPE_CHICKEN_UNDERRUN_RECOVERY_DISABLE_ADLP
        | PIPE_CHICKEN_PIXEL_ROUNDING_TRUNC_FB_PASSTHRU
        | PIPE_CHICKEN_PER_PIXEL_ALPHA_BYPASS;
    crate::intel::mmio_write(dev, PIPE_CHICKEN_A + mirror.slot * PIPE_MMIO_STRIDE, chicken);
    let saved_output_color = program_pipe_c_output_bt709_ycbcr(dev);
    mirror_live_scene(dev);

    // WD timings contain active dimensions only; triggered capture supplies
    // its own sink timing and requires no DDI or physical connector.
    crate::intel::mmio_write(dev, WD0_HTOTAL, width - 1);
    crate::intel::mmio_write(dev, WD0_VTOTAL, height - 1);
    crate::intel::mmio_write(dev, WD0_STRIDE, (pitch_bytes.div_ceil(64) & 0x3FF) << 6);
    crate::intel::mmio_write(dev, WD0_SURF, CAPTURE_GPU as u32);
    crate::intel::mmio_write(dev, WD0_TAIL_CFG, 0);
    crate::intel::mmio_write(dev, WD0_FRAME_STATUS, WD_FRAME_COMPLETE);
    crate::intel::mmio_write(
        dev,
        WD0_FUNC_CTL,
        WD_FUNC_ENABLE
            | WD_TRIGGERED_CAPTURE_ENABLE
            | WD_COLOR_MODE_XYUV8888
            | WD_DISABLE_POINTERS
            | WD_INPUT_PIPE_C,
    );

    // Do not enable conventional Transcoder C (PIPECONF_C). WD0 is Pipe C's
    // sole transcoder/sink; enabling both would violate the one-transcoder-per-
    // pipe topology and turn this headless route into an invalid dual output.
    crate::intel::mmio_write(dev, WD0_TRANS_CONF, WD_TRANS_ENABLE);
    let _ = crate::intel::mmio_read(dev, WD0_TRANS_CONF);
    if !wait_for_mask(dev, WD0_TRANS_CONF, WD_TRANS_STATE, true, PIPE_WAIT_ITERS) {
        crate::intel::mmio_write(dev, WD0_TRANS_CONF, 0);
        crate::intel::mmio_write(dev, WD0_FUNC_CTL, 0);
        restore_pipe_c_output_color(dev, saved_output_color);
        crate::intel::mmio_write(dev, DBUF_CTL_S2, saved_dbuf_ctl_s2);
        crate::intel::mmio_write(dev, HSW_PWR_WELL_CTL2, saved_power_well_ctl2);
        let _ = crate::intel::unmap_display_scanout_ggtt(dev, byte_len, CAPTURE_GPU);
        crate::dma::dealloc(virt, byte_len);
        return Err(WdCaptureError::WdEnableTimeout);
    }

    *state = WdCaptureState {
        running: true,
        pending: false,
        width,
        height,
        pitch_bytes,
        byte_len,
        phys,
        virt,
        frame_number: 1,
        sequence: 0,
        saved_power_well_ctl2,
        saved_dbuf_ctl_s2,
        saved_output_color,
    };
    let (pipe_misc, csc_mode, _, _, _) = pipe_c_output_color_registers();
    crate::log_info!(target: "intel/display";
        "intel/display: wd0 online=1 source=pipe-c mirror_of=pipe-a map={:?} slots=0-4+cursor5 postblend=opaque-rgb output_csc=bt709-limited-ycbcr pipe_misc=0x{:08X} pipe_csc_mode=0x{:08X} sink=ggtt-xyuv8888 size={}x{} pitch={} bytes=0x{:X} gpu=0x{:X} dbuf=s2:1024-2031 cursor=pipe-c:2040-2047 transcoder-c=disabled ddi=none cpu_compositor=0 gpu_compositor=0\n",
        mirror_map_mode(), crate::intel::mmio_read(dev, pipe_misc), crate::intel::mmio_read(dev, csc_mode), width, height, pitch_bytes, byte_len, CAPTURE_GPU,
    );
    Ok(state.frame())
}

pub(crate) fn begin_ui4_wd_xyuv8888_capture() -> Result<u64, WdCaptureError> {
    let dev = crate::intel::claimed_device().ok_or(WdCaptureError::DeviceUnavailable)?;
    let mut state = STATE.lock();
    if !state.running {
        return Err(WdCaptureError::NotRunning);
    }
    if state.pending {
        return Err(WdCaptureError::AlreadyCapturing);
    }
    mirror_live_scene(dev);
    crate::intel::mmio_write(dev, WD0_SURF, CAPTURE_GPU as u32);
    crate::intel::mmio_write(dev, WD0_FRAME_STATUS, WD_FRAME_COMPLETE);
    let old = crate::intel::mmio_read(dev, WD0_FUNC_CTL);
    let trigger = WD_START_TRIGGER_FRAME | u32::from(state.frame_number);
    crate::intel::mmio_write(
        dev,
        WD0_FUNC_CTL,
        (old & !(WD_START_TRIGGER_FRAME | WD_STOP_TRIGGER_FRAME | WD_FRAME_NUMBER_MASK)) | trigger,
    );
    state.pending = true;
    state.sequence = state.sequence.saturating_add(1);
    Ok(state.sequence)
}

pub(crate) fn poll_ui4_wd_xyuv8888_capture() -> WdCapturePoll {
    let Some(dev) = crate::intel::claimed_device() else {
        return WdCapturePoll::Failed { status: u32::MAX };
    };
    let mut state = STATE.lock();
    if !state.running || !state.pending {
        return WdCapturePoll::Idle;
    }
    let status = crate::intel::mmio_read(dev, WD0_FRAME_STATUS);
    if status & WD_FRAME_COMPLETE == 0 {
        return WdCapturePoll::Pending;
    }
    state.pending = false;
    state.frame_number = if state.frame_number >= 7 {
        1
    } else {
        state.frame_number + 1
    };
    crate::intel::mmio_write(dev, WD0_FRAME_STATUS, WD_FRAME_COMPLETE);
    WdCapturePoll::Complete(state.frame())
}

pub(crate) fn stop_ui4_wd_xyuv8888_capture() -> Result<(), WdCaptureError> {
    let dev = crate::intel::claimed_device().ok_or(WdCaptureError::DeviceUnavailable)?;
    let mut state = STATE.lock();
    if !state.running {
        return Ok(());
    }
    if state.pending {
        let ctl = crate::intel::mmio_read(dev, WD0_FUNC_CTL);
        crate::intel::mmio_write(dev, WD0_FUNC_CTL, ctl | WD_STOP_TRIGGER_FRAME);
        CAPTURE_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
    }
    crate::intel::mmio_write(dev, WD0_TRANS_CONF, 0);
    let _ = wait_for_mask(dev, WD0_TRANS_CONF, WD_TRANS_STATE, false, PIPE_WAIT_ITERS);
    crate::intel::mmio_write(dev, WD0_FUNC_CTL, 0);
    for slot in 0..UNIVERSAL_PLANE_SLOTS {
        let plane = PIPES[MIRROR_PIPE_SLOT].plane(slot);
        crate::intel::mmio_write(dev, plane.ctl(), 0);
        crate::intel::mmio_write(dev, plane.surf(), 0);
    }
    let cursor = CURSOR_A_BASE + MIRROR_PIPE_SLOT * CURSOR_PIPE_STRIDE;
    crate::intel::mmio_write(dev, cursor + CURSOR_CTL_OFF, 0);
    crate::intel::mmio_write(dev, cursor + CURSOR_BASE_OFF, 0);
    restore_pipe_c_output_color(dev, state.saved_output_color);
    crate::intel::mmio_write(dev, DBUF_CTL_S2, state.saved_dbuf_ctl_s2);
    crate::intel::mmio_write(dev, HSW_PWR_WELL_CTL2, state.saved_power_well_ctl2);
    let _ = crate::intel::unmap_display_scanout_ggtt(dev, state.byte_len, CAPTURE_GPU);
    crate::dma::dealloc(state.virt, state.byte_len);
    *state = WdCaptureState::new();
    crate::log_info!(target: "intel/display";
        "intel/display: wd0 online=0 pipe-c-fetch=idle transcoder-c=untouched capture_mapping=released\n"
    );
    Ok(())
}

pub(crate) fn ui4_wd_xyuv8888_capture_status() -> WdCaptureStatus {
    let state = *STATE.lock();
    let Some(dev) = crate::intel::claimed_device() else {
        return WdCaptureStatus {
            running: state.running,
            capture_pending: state.pending,
            sequence: state.sequence,
            width: state.width,
            height: state.height,
            pitch_bytes: state.pitch_bytes,
            wd_func_ctl: u32::MAX,
            wd_trans_conf: u32::MAX,
            wd_frame_status: u32::MAX,
            transcoder_c_conf: u32::MAX,
            map_mode: mirror_map_mode(),
        };
    };
    WdCaptureStatus {
        running: state.running,
        capture_pending: state.pending,
        sequence: state.sequence,
        width: state.width,
        height: state.height,
        pitch_bytes: state.pitch_bytes,
        wd_func_ctl: crate::intel::mmio_read(dev, WD0_FUNC_CTL),
        wd_trans_conf: crate::intel::mmio_read(dev, WD0_TRANS_CONF),
        wd_frame_status: crate::intel::mmio_read(dev, WD0_FRAME_STATUS),
        transcoder_c_conf: crate::intel::mmio_read(
            dev,
            PIPECONF_A + MIRROR_PIPE_SLOT * PIPE_MMIO_STRIDE,
        ),
        map_mode: mirror_map_mode(),
    }
}

#[cfg(test)]
mod tests {
    use super::MirrorMapMode;

    #[test]
    fn identity_maps_all_universal_slots_one_to_one() {
        for slot in 0..=4 {
            assert_eq!(MirrorMapMode::Identity.source_for_destination(slot), slot);
        }
    }

    #[test]
    fn optional_modes_only_swap_the_named_single_frame_slots() {
        assert_eq!(MirrorMapMode::Swap1And3.source_for_destination(0), 0);
        assert_eq!(MirrorMapMode::Swap1And3.source_for_destination(1), 3);
        assert_eq!(MirrorMapMode::Swap1And3.source_for_destination(2), 2);
        assert_eq!(MirrorMapMode::Swap1And3.source_for_destination(3), 1);
        assert_eq!(MirrorMapMode::Swap1And3.source_for_destination(4), 4);

        assert_eq!(MirrorMapMode::Swap2And3.source_for_destination(0), 0);
        assert_eq!(MirrorMapMode::Swap2And3.source_for_destination(1), 1);
        assert_eq!(MirrorMapMode::Swap2And3.source_for_destination(2), 3);
        assert_eq!(MirrorMapMode::Swap2And3.source_for_destination(3), 2);
        assert_eq!(MirrorMapMode::Swap2And3.source_for_destination(4), 4);
    }
}
