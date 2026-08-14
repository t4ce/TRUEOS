//! Raw Intel display-cursor backend owned exclusively by TrueOS-Spirit.
//!
//! This module has no input-device policy, polling task, or convenience API.
//! Spirit is the only caller allowed to turn a command stream into CUR_* MMIO.

use spin::Mutex;

use crate::intel::SPIRIT_CURSOR_DBUF_S1_START;

const SPIRIT_CHANNEL_COUNT: usize = 4;
const SPIRIT_SURFACES_PER_CHANNEL: usize = 2;
pub(super) const SPIRIT_CURSOR_DIM: u32 = 256;
const SPIRIT_CURSOR_BYTES_PER_PIXEL: u32 = 4;
const SPIRIT_CURSOR_RADIUS: i32 = 10;
const SPIRIT_CURSOR_HOTSPOT: i32 = (SPIRIT_CURSOR_DIM / 2) as i32;
const SPIRIT_CURSOR_GPU_STRIDE: u64 = 0x0004_0000;
const SPIRIT_CURSOR_SCANLINE_BYTES: u32 = SPIRIT_CURSOR_DIM * SPIRIT_CURSOR_BYTES_PER_PIXEL;
const SPIRIT_CURSOR_SURFACE_BYTES: u64 =
    SPIRIT_CURSOR_SCANLINE_BYTES as u64 * SPIRIT_CURSOR_DIM as u64;
const _: () = assert!(SPIRIT_CURSOR_GPU_STRIDE >= SPIRIT_CURSOR_SURFACE_BYTES);

const PIPES: [PipeInfo; SPIRIT_CHANNEL_COUNT] = [
    PipeInfo { name: "pipe-a" },
    PipeInfo { name: "pipe-b" },
    PipeInfo { name: "pipe-c" },
    PipeInfo { name: "pipe-d" },
];

const CURSOR_A_BASE: usize = 0x70080;
const CURSOR_CTL_OFF: usize = 0x00;
const CURSOR_BASE_OFF: usize = 0x04;
const CURSOR_POS_OFF: usize = 0x08;
const CURSOR_SURF_LIVE_OFF: usize = 0x2C;
const CURSOR_WM_A0: usize = 0x70140;
const CURSOR_WM_TRANS_A: usize = 0x70168;
const CURSOR_WM_SAGV_A: usize = 0x70158;
const CURSOR_WM_SAGV_TRANS_A: usize = 0x7015C;
const CURSOR_BUF_CFG_A: usize = 0x7017C;
const SEL_FETCH_CUR_CTL_A: usize = 0x70880;
const CURSOR_PIPE_STRIDE: usize = 0x1000;

const CURSOR_ARB_SLOTS_1: u32 = 1 << 28;
const CURSOR_MODE_MASK: u32 = 0x27;
const CURSOR_MODE_256_ARGB_AX: u32 = 0x23;
const CURSOR_POS_Y_SIGN: u32 = 1 << 31;
const CURSOR_POS_X_SIGN: u32 = 1 << 15;
const CURSOR_POS_Y_MASK: u32 = 0x7FFF << 16;
const CURSOR_POS_X_MASK: u32 = 0x7FFF;

const CUR_WM_ENABLE: u32 = 1 << 31;
const CUR_WM_LINES_2: u32 = 2 << 14;
const CUR_WM_BLOCKS_4: u32 = 4;
const CUR_WM_LEVEL0_SPIRIT: u32 = CUR_WM_ENABLE | CUR_WM_LINES_2 | CUR_WM_BLOCKS_4;
const GEN12_DDB_BLOCK_BYTES: u32 = 512;
// Gen10+ linear scanout adds one fetch block beyond the rounded scanline.
// A 256-wide ARGB line therefore consumes three 512-byte DDB blocks, still
// inside the existing four-block WM0 and eight-block per-pipe allocation.
const SPIRIT_CURSOR_FETCH_BLOCKS_PER_LINE: u32 =
    SPIRIT_CURSOR_SCANLINE_BYTES.div_ceil(GEN12_DDB_BLOCK_BYTES) + 1;
const _: () = assert!(SPIRIT_CURSOR_FETCH_BLOCKS_PER_LINE <= CUR_WM_BLOCKS_4);

const DBUF_CTL_S1: usize = 0x45008;
const DBUF_CTL_S2: usize = 0x44FE8;
const DBUF_POWER_REQUEST: u32 = 1 << 31;
const DBUF_POWER_STATE: u32 = 1 << 30;

const SPIRIT_CURSOR_DDB_BLOCKS: u16 = 8;
const SPIRIT_CURSOR_DDB: [(u16, u16); SPIRIT_CHANNEL_COUNT] =
    [(1008, 1016), (1016, 1024), (2040, 2048), (2032, 2040)];
const _: () = assert!(SPIRIT_CURSOR_DBUF_S1_START == SPIRIT_CURSOR_DDB[0].0);
const _: () = assert!(SPIRIT_CURSOR_DDB[0].1 == SPIRIT_CURSOR_DDB[1].0);
const _: () = assert!(SPIRIT_CURSOR_DDB[2].1 == 2048);
const _: () = assert!(SPIRIT_CURSOR_DDB[3].1 == SPIRIT_CURSOR_DDB[2].0);
const _: () = assert!(SPIRIT_CURSOR_DDB[0].1 - SPIRIT_CURSOR_DDB[0].0 == SPIRIT_CURSOR_DDB_BLOCKS);
const _: () = assert!(SPIRIT_CURSOR_DDB[1].1 - SPIRIT_CURSOR_DDB[1].0 == SPIRIT_CURSOR_DDB_BLOCKS);
const _: () = assert!(SPIRIT_CURSOR_DDB[2].1 - SPIRIT_CURSOR_DDB[2].0 == SPIRIT_CURSOR_DDB_BLOCKS);
const _: () = assert!(SPIRIT_CURSOR_DDB[3].1 - SPIRIT_CURSOR_DDB[3].0 == SPIRIT_CURSOR_DDB_BLOCKS);
const _: () = assert!((CUR_WM_LEVEL0_SPIRIT & 0x0FFF) < SPIRIT_CURSOR_DDB_BLOCKS as u32);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum SpiritCursorError {
    InvalidChannel,
    HardwareNotReady,
    PipeInactive,
    DbufNotReady,
    AllocationFailed,
    MappingFailed,
    FlipPending,
    InvalidFrame,
}

#[derive(Copy, Clone)]
pub(super) struct SpiritCursorFrame {
    pub(super) x_normalized: f64,
    pub(super) y_normalized: f64,
}

/// Convert one global normalized point into the exact local coordinates of a
/// Spirit cursor surface, if that point currently overlaps the surface.
///
/// Both positions use the selected pipe's dimensions and the same rounding as
/// CUR_POS programming. Keeping this beside the cursor-plane geometry avoids
/// treating a global desktop coordinate as an always-valid 256x256 shader
/// coordinate.
#[allow(dead_code)]
pub(super) fn spirit_cursor_local_point(
    channel: u8,
    frame: SpiritCursorFrame,
    point_x_normalized: f64,
    point_y_normalized: f64,
) -> Result<Option<(u16, u16)>, SpiritCursorError> {
    if !frame.x_normalized.is_finite()
        || !frame.y_normalized.is_finite()
        || !point_x_normalized.is_finite()
        || !point_y_normalized.is_finite()
    {
        return Err(SpiritCursorError::InvalidFrame);
    }

    let channel_index = channel_index(channel)?;
    let (scanout_width, scanout_height) = pipe_dimensions(channel_index)?;
    let spirit_left = normalized_cursor_to_px(frame.x_normalized, scanout_width);
    let spirit_top = normalized_cursor_to_px(frame.y_normalized, scanout_height);
    let point_x = normalized_screen_point_to_px(point_x_normalized, scanout_width);
    let point_y = normalized_screen_point_to_px(point_y_normalized, scanout_height);
    let local_x = point_x - spirit_left;
    let local_y = point_y - spirit_top;

    if local_x < 0
        || local_x >= SPIRIT_CURSOR_DIM as i32
        || local_y < 0
        || local_y >= SPIRIT_CURSOR_DIM as i32
    {
        return Ok(None);
    }

    Ok(Some((local_x as u16, local_y as u16)))
}

/// One exact Spirit allocation selected for producer access and a later flip.
///
/// `cursor_gpu` is the display GGTT alias. The physical allocation is the
/// identity shared with CPU producers and Spirit's GPGPU execution context;
/// its PPGTT maps these pages without creating or copying a second buffer.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) struct SpiritCursorSurfaceAccess {
    pub(super) channel: u8,
    pub(super) surface: u8,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) pitch_bytes: u32,
    pub(super) byte_len: usize,
    pub(super) phys: u64,
    pub(super) cursor_gpu: u64,
    pub(super) virt: *mut u8,
}

unsafe impl Send for SpiritCursorSurfaceAccess {}
unsafe impl Sync for SpiritCursorSurfaceAccess {}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) struct SpiritCursorFlip {
    channel: u8,
    surface: u8,
    gpu: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum SpiritCursorFlipState {
    Waiting { ctl: u32, base: u32, live: u32 },
    Visible { ctl: u32, base: u32, live: u32 },
    Interrupted { ctl: u32, base: u32, live: u32 },
}

#[derive(Copy, Clone)]
struct CursorRegs {
    ctl: usize,
    base: usize,
    pos: usize,
    surf_live: usize,
    wm0: usize,
    wm_trans: usize,
    wm_sagv: usize,
    wm_sagv_trans: usize,
    buf_cfg: usize,
    sel_fetch_ctl: usize,
}

#[derive(Copy, Clone)]
struct PipeInfo {
    name: &'static str,
}

#[derive(Copy, Clone)]
struct SpiritCursorSurface {
    width: u32,
    height: u32,
    pitch_bytes: u32,
    byte_len: usize,
    phys: u64,
    gpu: u64,
    virt: *mut u8,
}

unsafe impl Send for SpiritCursorSurface {}
unsafe impl Sync for SpiritCursorSurface {}

struct SpiritCursorChannel {
    surfaces: [Option<SpiritCursorSurface>; SPIRIT_SURFACES_PER_CHANNEL],
    front: Option<u8>,
    pending: Option<u8>,
    init_failed: bool,
    visible: bool,
}

impl SpiritCursorChannel {
    const fn new() -> Self {
        Self {
            surfaces: [None, None],
            front: None,
            pending: None,
            init_failed: false,
            visible: false,
        }
    }
}

struct SpiritCursorState {
    channels: [SpiritCursorChannel; SPIRIT_CHANNEL_COUNT],
}

impl SpiritCursorState {
    const fn new() -> Self {
        Self {
            channels: [
                SpiritCursorChannel::new(),
                SpiritCursorChannel::new(),
                SpiritCursorChannel::new(),
                SpiritCursorChannel::new(),
            ],
        }
    }
}

static SPIRIT_CURSOR_STATE: Mutex<SpiritCursorState> = Mutex::new(SpiritCursorState::new());

pub(super) fn spirit_cursor_prepare(channel: u8) -> Result<(), SpiritCursorError> {
    let channel_index = channel_index(channel)?;
    let dev = crate::intel::claimed_device().ok_or(SpiritCursorError::HardwareNotReady)?;
    if !crate::intel::gen12_integrated_pat_ready() {
        return Err(SpiritCursorError::HardwareNotReady);
    }

    let mut state = SPIRIT_CURSOR_STATE.lock();
    let channel_state = &mut state.channels[channel_index];
    if channel_state.surfaces.iter().all(Option::is_some) {
        return Ok(());
    }
    if channel_state.init_failed {
        return Err(SpiritCursorError::AllocationFailed);
    }

    let first = allocate_surface(dev, channel_index, 0);
    let second = allocate_surface(dev, channel_index, 1);
    let (first, second) = match (first, second) {
        (Ok(first), Ok(second)) => (first, second),
        (Err(error), _) | (_, Err(error)) => {
            channel_state.init_failed = true;
            return Err(error);
        }
    };

    channel_state.surfaces = [Some(first), Some(second)];
    crate::intel::ggtt_invalidate(dev);
    crate::log_info!(
        target: "gfx";
        "trueos-spirit: channel={} double-buffer ready size={}x{} pitch={} gpu=[0x{:X},0x{:X}] phys=[0x{:X},0x{:X}] bytes=0x{:X}\n",
        channel,
        first.width,
        first.height,
        first.pitch_bytes,
        first.gpu,
        second.gpu,
        first.phys,
        second.phys,
        first.byte_len,
    );
    Ok(())
}

pub(super) fn spirit_cursor_back_surface(
    channel: u8,
) -> Result<SpiritCursorSurfaceAccess, SpiritCursorError> {
    spirit_cursor_prepare(channel)?;
    let channel_index = channel_index(channel)?;
    let state = SPIRIT_CURSOR_STATE.lock();
    let channel_state = &state.channels[channel_index];
    if channel_state.pending.is_some() {
        return Err(SpiritCursorError::FlipPending);
    }
    let surface_index = channel_state.front.map_or(0, |front| 1 - front);
    let surface = channel_state.surfaces[surface_index as usize]
        .ok_or(SpiritCursorError::HardwareNotReady)?;
    Ok(surface_access(channel, surface_index, surface))
}

pub(super) fn spirit_cursor_draw_solid_circle(
    access: SpiritCursorSurfaceAccess,
    color_bgra_premultiplied: u32,
) -> Result<(), SpiritCursorError> {
    let surface = registered_surface(access)?;
    draw_solid_circle(surface, color_bgra_premultiplied);
    Ok(())
}

pub(super) fn spirit_cursor_flush_cpu(
    access: SpiritCursorSurfaceAccess,
) -> Result<(), SpiritCursorError> {
    let surface = registered_surface(access)?;
    crate::intel::dma_flush(surface.virt, surface.byte_len);
    Ok(())
}

/// Program only CUR_POS for Spirit's selected pipe. The dedicated movement
/// task is the sole caller; frame production and CUR_BASE flips never write
/// this register after ownership has been split.
pub(super) fn spirit_cursor_move(
    channel: u8,
    frame: SpiritCursorFrame,
) -> Result<(i32, i32), SpiritCursorError> {
    if !frame.x_normalized.is_finite() || !frame.y_normalized.is_finite() {
        return Err(SpiritCursorError::InvalidFrame);
    }
    let channel_index = channel_index(channel)?;
    let dev = crate::intel::claimed_device().ok_or(SpiritCursorError::HardwareNotReady)?;
    let (scanout_width, scanout_height) = pipe_dimensions(channel_index)?;
    let x = normalized_cursor_to_px(frame.x_normalized, scanout_width);
    let y = normalized_cursor_to_px(frame.y_normalized, scanout_height);
    write_if_changed(dev, cursor_regs(channel_index).pos, cursor_pos_reg_value(x, y));
    Ok((x, y))
}

pub(super) fn spirit_cursor_arm(
    access: SpiritCursorSurfaceAccess,
) -> Result<SpiritCursorFlip, SpiritCursorError> {
    spirit_cursor_prepare(access.channel)?;

    let channel_index = channel_index(access.channel)?;
    let dev = crate::intel::claimed_device().ok_or(SpiritCursorError::HardwareNotReady)?;
    let _ = pipe_dimensions(channel_index)?;
    if !ensure_dbuf_power(dev, channel_index) {
        return Err(SpiritCursorError::DbufNotReady);
    }

    let regs = cursor_regs(channel_index);
    let mut state = SPIRIT_CURSOR_STATE.lock();
    let channel_state = &mut state.channels[channel_index];
    if channel_state.pending.is_some() {
        return Err(SpiritCursorError::FlipPending);
    }
    if access.surface as usize >= SPIRIT_SURFACES_PER_CHANNEL {
        return Err(SpiritCursorError::InvalidFrame);
    }

    let surface = channel_state.surfaces[access.surface as usize]
        .ok_or(SpiritCursorError::HardwareNotReady)?;
    if surface_access(access.channel, access.surface, surface) != access {
        return Err(SpiritCursorError::InvalidFrame);
    }

    let ctl = cursor_ctl(dev.device_id);
    let buf_cfg = cursor_ddb_cfg(channel_index);
    let base = u32::try_from(surface.gpu).map_err(|_| SpiritCursorError::MappingFailed)?;

    // Frame production owns CUR_BASE but never CUR_POS. CUR_BASE is
    // deliberately last: it arms CTL and the new surface for the next vblank
    // without coupling motion to the frame/flip cadence.
    write_if_changed(dev, regs.buf_cfg, buf_cfg);
    write_if_changed(dev, regs.wm0, CUR_WM_LEVEL0_SPIRIT);
    write_if_changed(dev, regs.wm_trans, 0);
    write_if_changed(dev, regs.wm_sagv, 0);
    write_if_changed(dev, regs.wm_sagv_trans, 0);
    write_if_changed(dev, regs.sel_fetch_ctl, 0);
    write_if_changed(dev, regs.ctl, ctl);
    crate::intel::mmio_write(dev, regs.base, base);

    channel_state.pending = Some(access.surface);
    let was_visible = channel_state.visible;
    crate::log_trace!(
        target: "gfx";
        "trueos-spirit: arm fence={} pipe={} buffer={} pos_owner=spirit-cursor-task ctl=0x{:08X} base=0x{:08X} ddb=0x{:08X} first={}\n",
        access.channel,
        PIPES[channel_index].name,
        access.surface,
        ctl,
        base,
        buf_cfg,
        (!was_visible) as u8,
    );

    Ok(SpiritCursorFlip {
        channel: access.channel,
        surface: access.surface,
        gpu: base,
    })
}

pub(super) fn spirit_cursor_poll(
    flip: SpiritCursorFlip,
) -> Result<SpiritCursorFlipState, SpiritCursorError> {
    let channel_index = channel_index(flip.channel)?;
    let dev = crate::intel::claimed_device().ok_or(SpiritCursorError::HardwareNotReady)?;
    let regs = cursor_regs(channel_index);
    let pipe_inactive =
        matches!(pipe_dimensions(channel_index), Err(SpiritCursorError::PipeInactive));
    let base = crate::intel::mmio_read(dev, regs.base);
    let live = crate::intel::mmio_read(dev, regs.surf_live);
    let ctl = crate::intel::mmio_read(dev, regs.ctl);
    if pipe_inactive || base != flip.gpu {
        let mut state = SPIRIT_CURSOR_STATE.lock();
        let channel_state = &mut state.channels[channel_index];
        if channel_state.pending == Some(flip.surface) {
            channel_state.pending = None;
        }
        channel_state.visible = false;
        return Ok(SpiritCursorFlipState::Interrupted { ctl, base, live });
    }
    if live != flip.gpu || ctl & CURSOR_MODE_MASK == 0 {
        return Ok(SpiritCursorFlipState::Waiting { ctl, base, live });
    }

    let mut state = SPIRIT_CURSOR_STATE.lock();
    let channel_state = &mut state.channels[channel_index];
    if channel_state.pending == Some(flip.surface) {
        channel_state.front = Some(flip.surface);
        channel_state.pending = None;
        channel_state.visible = true;
    }
    Ok(SpiritCursorFlipState::Visible { ctl, base, live })
}

/// Re-issue one cursor flip after its first bounded SURFLIVE wait expires.
///
/// The pending surface identity must still match the original arm. Rewriting
/// the complete cursor contract followed by CUR_BASE gives hardware one
/// explicit second latch opportunity without allocating, rendering, or
/// advancing the public Spirit fence again.
pub(super) fn spirit_cursor_retry_arm(flip: SpiritCursorFlip) -> Result<(), SpiritCursorError> {
    let channel_index = channel_index(flip.channel)?;
    let dev = crate::intel::claimed_device().ok_or(SpiritCursorError::HardwareNotReady)?;
    let _ = pipe_dimensions(channel_index)?;
    if !ensure_dbuf_power(dev, channel_index) {
        return Err(SpiritCursorError::DbufNotReady);
    }

    let regs = cursor_regs(channel_index);
    let state = SPIRIT_CURSOR_STATE.lock();
    let channel_state = &state.channels[channel_index];
    if flip.surface as usize >= SPIRIT_SURFACES_PER_CHANNEL
        || channel_state.pending != Some(flip.surface)
    {
        return Err(SpiritCursorError::InvalidFrame);
    }
    let surface =
        channel_state.surfaces[flip.surface as usize].ok_or(SpiritCursorError::HardwareNotReady)?;
    let base = u32::try_from(surface.gpu).map_err(|_| SpiritCursorError::MappingFailed)?;
    if base != flip.gpu {
        return Err(SpiritCursorError::InvalidFrame);
    }

    write_if_changed(dev, regs.buf_cfg, cursor_ddb_cfg(channel_index));
    write_if_changed(dev, regs.wm0, CUR_WM_LEVEL0_SPIRIT);
    write_if_changed(dev, regs.wm_trans, 0);
    write_if_changed(dev, regs.wm_sagv, 0);
    write_if_changed(dev, regs.wm_sagv_trans, 0);
    write_if_changed(dev, regs.sel_fetch_ctl, 0);
    write_if_changed(dev, regs.ctl, cursor_ctl(dev.device_id));
    crate::intel::mmio_write(dev, regs.base, base);
    Ok(())
}

/// Drop only the software ownership of a failed pending flip. The frame task
/// stops immediately afterward, performs no further cursor-register writes,
/// and never reuses the failed back buffer.
pub(super) fn spirit_cursor_abandon(flip: SpiritCursorFlip) {
    let Ok(channel_index) = channel_index(flip.channel) else {
        return;
    };
    let mut state = SPIRIT_CURSOR_STATE.lock();
    let channel_state = &mut state.channels[channel_index];
    if channel_state.pending == Some(flip.surface) {
        channel_state.pending = None;
    }
}

pub(super) fn spirit_cursor_rearm_needed(channel: u8) -> Result<bool, SpiritCursorError> {
    let channel_index = channel_index(channel)?;
    let dev = crate::intel::claimed_device().ok_or(SpiritCursorError::HardwareNotReady)?;
    match pipe_dimensions(channel_index) {
        Ok(_) => {}
        Err(SpiritCursorError::PipeInactive) => return Ok(false),
        Err(error) => return Err(error),
    }

    let expected_gpu = {
        let state = SPIRIT_CURSOR_STATE.lock();
        let channel_state = &state.channels[channel_index];
        channel_state
            .front
            .and_then(|front| channel_state.surfaces[front as usize])
            .and_then(|surface| u32::try_from(surface.gpu).ok())
    };
    let Some(expected_gpu) = expected_gpu else {
        return Ok(true);
    };

    let regs = cursor_regs(channel_index);
    let contract_lost =
        crate::intel::mmio_read(dev, dbuf_control(channel_index)) & DBUF_POWER_STATE == 0
            || crate::intel::mmio_read(dev, regs.ctl) != cursor_ctl(dev.device_id)
            || crate::intel::mmio_read(dev, regs.surf_live) != expected_gpu
            || crate::intel::mmio_read(dev, regs.buf_cfg) != cursor_ddb_cfg(channel_index)
            || crate::intel::mmio_read(dev, regs.wm0) != CUR_WM_LEVEL0_SPIRIT;
    if contract_lost {
        SPIRIT_CURSOR_STATE.lock().channels[channel_index].visible = false;
    }
    Ok(contract_lost)
}

pub(super) fn with_realtime_encode_overlay_pipe_a_surflive<R>(
    read: impl FnOnce(super::SpiritRealtimeEncodeOverlay<'_>) -> R,
) -> Option<R> {
    // The fixed test-rig real-time encode source is D01, currently driven by
    // display pipe A. Cursor ownership and registers are per-pipe here.
    const REALTIME_ENCODE_PIPE: usize = 0;

    let dev = crate::intel::claimed_device()?;
    let state = SPIRIT_CURSOR_STATE.lock();
    let pipe_state = &state.channels[REALTIME_ENCODE_PIPE];
    let regs = cursor_regs(REALTIME_ENCODE_PIPE);
    if crate::intel::mmio_read(dev, regs.ctl) & CURSOR_MODE_MASK == 0 {
        return None;
    }
    let live = crate::intel::mmio_read(dev, regs.surf_live);
    let surface = pipe_state
        .surfaces
        .iter()
        .flatten()
        .copied()
        .find(|surface| {
            u32::try_from(surface.gpu)
                .ok()
                .is_some_and(|gpu| gpu == live)
        })?;

    let (left, top) = cursor_pos_from_reg(crate::intel::mmio_read(dev, regs.pos));
    crate::intel::dma_flush(surface.virt, surface.byte_len);
    let bgra_premultiplied =
        unsafe { core::slice::from_raw_parts(surface.virt.cast_const(), surface.byte_len) };
    Some(read(super::SpiritRealtimeEncodeOverlay {
        left,
        top,
        width: surface.width,
        height: surface.height,
        pitch_bytes: surface.pitch_bytes,
        bgra_premultiplied,
    }))
}

fn channel_index(channel: u8) -> Result<usize, SpiritCursorError> {
    let index = channel as usize;
    if index < SPIRIT_CHANNEL_COUNT {
        Ok(index)
    } else {
        Err(SpiritCursorError::InvalidChannel)
    }
}

fn surface_access(
    channel: u8,
    surface_index: u8,
    surface: SpiritCursorSurface,
) -> SpiritCursorSurfaceAccess {
    SpiritCursorSurfaceAccess {
        channel,
        surface: surface_index,
        width: surface.width,
        height: surface.height,
        pitch_bytes: surface.pitch_bytes,
        byte_len: surface.byte_len,
        phys: surface.phys,
        cursor_gpu: surface.gpu,
        virt: surface.virt,
    }
}

fn registered_surface(
    access: SpiritCursorSurfaceAccess,
) -> Result<SpiritCursorSurface, SpiritCursorError> {
    let channel_index = channel_index(access.channel)?;
    if access.surface as usize >= SPIRIT_SURFACES_PER_CHANNEL {
        return Err(SpiritCursorError::InvalidFrame);
    }
    let state = SPIRIT_CURSOR_STATE.lock();
    let surface = state.channels[channel_index].surfaces[access.surface as usize]
        .ok_or(SpiritCursorError::HardwareNotReady)?;
    if surface_access(access.channel, access.surface, surface) != access {
        return Err(SpiritCursorError::InvalidFrame);
    }
    Ok(surface)
}

fn allocate_surface(
    dev: crate::intel::Dev,
    channel: usize,
    surface: usize,
) -> Result<SpiritCursorSurface, SpiritCursorError> {
    let pitch_bytes = aligned_pitch_bytes(SPIRIT_CURSOR_DIM, SPIRIT_CURSOR_BYTES_PER_PIXEL)
        .ok_or(SpiritCursorError::AllocationFailed)?;
    let byte_len = usize::try_from(u64::from(pitch_bytes) * u64::from(SPIRIT_CURSOR_DIM))
        .map_err(|_| SpiritCursorError::AllocationFailed)?;
    let (phys, virt) = crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN)
        .ok_or(SpiritCursorError::AllocationFailed)?;
    let surface_slot = channel * SPIRIT_SURFACES_PER_CHANNEL + surface;
    let gpu = crate::intel::GPU_VA_DISPLAY_CURSOR_BASE
        .checked_add(surface_slot as u64 * SPIRIT_CURSOR_GPU_STRIDE)
        .ok_or(SpiritCursorError::MappingFailed)?;

    fill_surface_color(virt, pitch_bytes as usize, SPIRIT_CURSOR_DIM, SPIRIT_CURSOR_DIM, 0);
    crate::intel::dma_flush(virt, byte_len);
    if !crate::intel::map_display_scanout_ggtt(dev, phys, byte_len, gpu) {
        return Err(SpiritCursorError::MappingFailed);
    }

    Ok(SpiritCursorSurface {
        width: SPIRIT_CURSOR_DIM,
        height: SPIRIT_CURSOR_DIM,
        pitch_bytes,
        byte_len,
        phys,
        gpu,
        virt,
    })
}

fn pipe_dimensions(channel: usize) -> Result<(u32, u32), SpiritCursorError> {
    crate::intel::complete_scanout_pipeline_dimensions(channel)
        .ok_or(SpiritCursorError::PipeInactive)
}

fn ensure_dbuf_power(dev: crate::intel::Dev, channel: usize) -> bool {
    let ctl = dbuf_control(channel);
    let before = crate::intel::mmio_read(dev, ctl);
    if before & DBUF_POWER_STATE != 0 {
        return true;
    }
    crate::intel::mmio_write(dev, ctl, before | DBUF_POWER_REQUEST);
    crate::intel::mmio_read(dev, ctl) & DBUF_POWER_STATE != 0
}

fn dbuf_control(channel: usize) -> usize {
    if channel < 2 {
        DBUF_CTL_S1
    } else {
        DBUF_CTL_S2
    }
}

fn cursor_regs(channel: usize) -> CursorRegs {
    let cursor_base = CURSOR_A_BASE + channel * CURSOR_PIPE_STRIDE;
    CursorRegs {
        ctl: cursor_base + CURSOR_CTL_OFF,
        base: cursor_base + CURSOR_BASE_OFF,
        pos: cursor_base + CURSOR_POS_OFF,
        surf_live: cursor_base + CURSOR_SURF_LIVE_OFF,
        wm0: CURSOR_WM_A0 + channel * CURSOR_PIPE_STRIDE,
        wm_trans: CURSOR_WM_TRANS_A + channel * CURSOR_PIPE_STRIDE,
        wm_sagv: CURSOR_WM_SAGV_A + channel * CURSOR_PIPE_STRIDE,
        wm_sagv_trans: CURSOR_WM_SAGV_TRANS_A + channel * CURSOR_PIPE_STRIDE,
        buf_cfg: CURSOR_BUF_CFG_A + channel * CURSOR_PIPE_STRIDE,
        sel_fetch_ctl: SEL_FETCH_CUR_CTL_A + channel * CURSOR_PIPE_STRIDE,
    }
}

fn cursor_ctl(device_id: u16) -> u32 {
    let mut ctl = CURSOR_MODE_256_ARGB_AX;
    if cursor_needs_one_arb_slot(device_id) {
        ctl |= CURSOR_ARB_SLOTS_1;
    }
    ctl
}

fn cursor_needs_one_arb_slot(device_id: u16) -> bool {
    // Wa_22012358565 applies to display version 13. Keep both ADL-S (Xe_D)
    // and ADL-P/N (Xe_LPD) PCI IDs here; omitting ADL-S starves the cursor
    // fetch even though CUR_CTL and CUR_BASE accept their programmed values.
    matches!(
        device_id,
        0x4680
            | 0x4682
            | 0x4688
            | 0x468A
            | 0x468B
            | 0x4690
            | 0x4692
            | 0x4693
            | 0x46A0
            | 0x46A1
            | 0x46A2
            | 0x46A3
            | 0x46A6
            | 0x46A8
            | 0x46AA
            | 0x462A
            | 0x4626
            | 0x4628
            | 0x46B0
            | 0x46B1
            | 0x46B2
            | 0x46B3
    )
}

fn cursor_ddb_cfg(channel: usize) -> u32 {
    let (start, end) = SPIRIT_CURSOR_DDB[channel];
    (u32::from(end - 1) << 16) | u32::from(start)
}

const fn normalized_cursor_to_px(normalized: f64, extent: u32) -> i32 {
    // CUR_POS is the surface's top-left corner, not its hotspot. Map the
    // public normalized position across only the origins for which the whole
    // fixed-size cursor surface remains inside the scanout.
    let last_visible_origin = extent.saturating_sub(SPIRIT_CURSOR_DIM) as f64;
    (normalized.clamp(0.0, 1.0) * last_visible_origin + 0.5) as i32
}

const _: () = {
    assert!(normalized_cursor_to_px(0.0, 1920) == 0);
    assert!(normalized_cursor_to_px(0.5, 1920) == 832);
    assert!(normalized_cursor_to_px(1.0, 1920) == 1664);
    assert!(normalized_cursor_to_px(1.0, 1080) == 824);
    assert!(normalized_cursor_to_px(1.0, SPIRIT_CURSOR_DIM) == 0);
    assert!(normalized_cursor_to_px(1.0, 128) == 0);
};

fn normalized_screen_point_to_px(normalized: f64, extent: u32) -> i32 {
    let last_pixel = extent.saturating_sub(1) as f64;
    (normalized.clamp(0.0, 1.0) * last_pixel + 0.5) as i32
}

fn cursor_pos_reg_value(x: i32, y: i32) -> u32 {
    let mut value = 0u32;
    let x_magnitude = x.unsigned_abs().min(0x7FFF);
    let y_magnitude = y.unsigned_abs().min(0x7FFF);
    if x < 0 {
        value |= CURSOR_POS_X_SIGN;
    }
    if y < 0 {
        value |= CURSOR_POS_Y_SIGN;
    }
    value | (x_magnitude & CURSOR_POS_X_MASK) | ((y_magnitude << 16) & CURSOR_POS_Y_MASK)
}

fn cursor_pos_from_reg(value: u32) -> (i32, i32) {
    let x_magnitude = (value & CURSOR_POS_X_MASK) as i32;
    let y_magnitude = ((value & CURSOR_POS_Y_MASK) >> 16) as i32;
    let x = if value & CURSOR_POS_X_SIGN != 0 {
        -x_magnitude
    } else {
        x_magnitude
    };
    let y = if value & CURSOR_POS_Y_SIGN != 0 {
        -y_magnitude
    } else {
        y_magnitude
    };
    (x, y)
}

fn draw_solid_circle(surface: SpiritCursorSurface, color: u32) {
    fill_surface_color(
        surface.virt,
        surface.pitch_bytes as usize,
        surface.width,
        surface.height,
        0,
    );
    let center = SPIRIT_CURSOR_HOTSPOT;
    let radius_squared = SPIRIT_CURSOR_RADIUS * SPIRIT_CURSOR_RADIUS;
    let pitch_pixels = surface.pitch_bytes as usize / core::mem::size_of::<u32>();
    for y in (center - SPIRIT_CURSOR_RADIUS)..=(center + SPIRIT_CURSOR_RADIUS) {
        let row = unsafe { (surface.virt as *mut u32).add(y as usize * pitch_pixels) };
        let dy = y - center;
        for x in (center - SPIRIT_CURSOR_RADIUS)..=(center + SPIRIT_CURSOR_RADIUS) {
            let dx = x - center;
            if dx * dx + dy * dy <= radius_squared {
                unsafe {
                    core::ptr::write_volatile(row.add(x as usize), color);
                }
            }
        }
    }
}

fn write_if_changed(dev: crate::intel::Dev, register: usize, value: u32) {
    if crate::intel::mmio_read(dev, register) != value {
        crate::intel::mmio_write(dev, register, value);
    }
}

fn aligned_pitch_bytes(width: u32, bytes_per_pixel: u32) -> Option<u32> {
    let bytes = width.checked_mul(bytes_per_pixel)?;
    u32::try_from(crate::intel::align_up(bytes as usize, 64)?).ok()
}

fn fill_surface_color(ptr: *mut u8, pitch_bytes: usize, width: u32, height: u32, color: u32) {
    unsafe {
        for y in 0..height as usize {
            let row = ptr.add(y.saturating_mul(pitch_bytes)) as *mut u32;
            for x in 0..width as usize {
                core::ptr::write_volatile(row.add(x), color);
            }
        }
    }
}
