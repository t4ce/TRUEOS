use core::sync::atomic::{AtomicBool, Ordering};

use spin::Mutex;

use super::display::{
    PIPES, PipeInfo, active_pipe, aligned_pitch_bytes, decode_pipe_src, fill_surface_color,
    framebuffer_hint, plane_buf_cfg_for_pipe_slot,
};

const CURSOR_A_BASE: usize = 0x70080;
const CURSOR_B_BASE: usize = 0x71080;
const CURSOR_C_BASE: usize = 0x72080;
const CURSOR_D_BASE: usize = 0x73080;
const CURSOR_CTL_OFF: usize = 0x00;
const CURSOR_BASE_OFF: usize = 0x04;
const CURSOR_POS_OFF: usize = 0x08;
const CURSOR_SURF_LIVE_OFF: usize = 0x2C;
const CURSOR_WM_A0: usize = 0x70140;
const CURSOR_WM_B0: usize = 0x71140;
const CURSOR_WM_SAGV_A: usize = 0x70158;
const CURSOR_WM_SAGV_B: usize = 0x71158;
const CURSOR_WM_SAGV_TRANS_A: usize = 0x7015C;
const CURSOR_WM_SAGV_TRANS_B: usize = 0x7115C;
const CURSOR_WM_TRANS_A: usize = 0x70168;
const CURSOR_WM_TRANS_B: usize = 0x71168;
const CURSOR_BUF_CFG_A: usize = 0x7017C;
const CURSOR_BUF_CFG_B: usize = 0x7117C;
const SEL_FETCH_CUR_CTL_A: usize = 0x70880;
const SEL_FETCH_CUR_CTL_B: usize = 0x71880;
const CURSOR_ENABLE: u32 = 1 << 31;
const CUR_WM_EN: u32 = 1 << 31;
const CURSOR_ARB_SLOTS_1: u32 = 1 << 28;
const CURSOR_MODE_MASK: u32 = 0x27;
const CURSOR_MODE_DISABLE: u32 = 0x00;
const CURSOR_MODE_64_ARGB_AX: u32 = 0x27;
const CURSOR_POS_Y_SIGN: u32 = 1 << 31;
const CURSOR_POS_X_SIGN: u32 = 1 << 15;
const CURSOR_POS_Y_MASK: u32 = 0x7FFF << 16;
const CURSOR_POS_X_MASK: u32 = 0x7FFF;
const KERNEL_CURSOR_DIM: u32 = 64;
const KERNEL_CURSOR_RECT_MIN_DIM: u32 = 8;
const KERNEL_CURSOR_RECT_MAX_DIM: u32 = 32;
const KERNEL_CURSOR_RECT_MIN_SCANOUT_H: u32 = 720;
const KERNEL_CURSOR_RECT_MAX_SCANOUT_H: u32 = 2160;
const KERNEL_CURSOR_BYTES_PER_PIXEL: u32 = 4;
const KERNEL_CURSOR_GPU_ADDR: u64 = crate::intel::GPU_VA_DISPLAY_CURSOR_BASE;
const KERNEL_CURSOR_HOTSPOT_X: i32 = (KERNEL_CURSOR_DIM / 2) as i32;
const KERNEL_CURSOR_HOTSPOT_Y: i32 = (KERNEL_CURSOR_DIM / 2) as i32;
const KERNEL_CURSOR_SPLIT_ALPHA: u8 = 0x40;
const KERNEL_CURSOR_PROBE_ONLY: bool = true;
const KERNEL_CURSOR_PROBE_KIND: CursorProbeKind =
    CursorProbeKind::CurWmEnableBufCfgCtlEnablePosBaseVisible;
const KERNEL_CURSOR_PROBE_LOG_INTERVAL_SECS: u64 = 60;
const KERNEL_CURSOR_DBUF_BLOCKS_ADL_S: u32 = 2048;
const KERNEL_CURSOR_DBUF_BLOCKS_ONE_PIPE: u32 = 32;
const PLANE_CTL_ENABLE: u32 = 1 << 31;

static KERNEL_CURSOR_DDB_MAP_LOGGED: AtomicBool = AtomicBool::new(false);
static KERNEL_CURSOR_STATE: Mutex<KernelCursorState> = Mutex::new(KernelCursorState::new());

#[derive(Copy, Clone)]
struct CursorRegs {
    ctl_off: usize,
    base_off: usize,
    pos_off: usize,
    surf_live_off: usize,
}

#[derive(Copy, Clone)]
struct CursorAuxRegs {
    wm0_off: usize,
    wm_trans_off: usize,
    wm_sagv_off: usize,
    wm_sagv_trans_off: usize,
    buf_cfg_off: usize,
    sel_fetch_ctl_off: usize,
}

#[derive(Copy, Clone)]
struct KernelCursorSurface {
    width: u32,
    height: u32,
    pitch_bytes: u32,
    phys: u64,
    virt: *mut u8,
}

unsafe impl Send for KernelCursorSurface {}
unsafe impl Sync for KernelCursorSurface {}

#[derive(Copy, Clone, Eq, PartialEq)]
enum CursorProbeKind {
    CurPosOnly,
    CurBaseOnly,
    CurBufCfgOnly,
    CurBufCfgBaseOnly,
    CurBufCfgBasePosOnly,
    CurBufCfgBasePosCtlModeOnly,
    CurBufCfgBasePosCtlEnable,
    CurBufCfgBasePosCtlEnableBase,
    CurBufCfgCtlEnablePosBase,
    CurWmEnableBufCfgCtlEnablePosBase,
    CurWmEnableBufCfgCtlEnablePosBaseVisible,
    CurCtlModeOnly,
    CurCtlObserveOnly,
}

impl CursorProbeKind {
    const fn label(self) -> &'static str {
        match self {
            Self::CurPosOnly => "curpos",
            Self::CurBaseOnly => "curbase",
            Self::CurBufCfgOnly => "curbufcfg",
            Self::CurBufCfgBaseOnly => "curbufcfg-base",
            Self::CurBufCfgBasePosOnly => "curbufcfg-base-pos",
            Self::CurBufCfgBasePosCtlModeOnly => "curbufcfg-base-pos-ctlmode",
            Self::CurBufCfgBasePosCtlEnable => "curbufcfg-base-pos-ctlenable",
            Self::CurBufCfgBasePosCtlEnableBase => "curbufcfg-base-pos-ctlenable-base",
            Self::CurBufCfgCtlEnablePosBase => "curbufcfg-ctlenable-pos-base",
            Self::CurWmEnableBufCfgCtlEnablePosBase => "curwm-bufcfg-ctlenable-pos-base",
            Self::CurWmEnableBufCfgCtlEnablePosBaseVisible => {
                "curwm-bufcfg-ctlenable-pos-base-visible"
            }
            Self::CurCtlModeOnly => "curctl-mode",
            Self::CurCtlObserveOnly => "curctl-observe",
        }
    }

    const fn forces_visible_probe_position(self) -> bool {
        matches!(self, Self::CurWmEnableBufCfgCtlEnablePosBaseVisible)
    }
}

struct KernelCursorState {
    surface: Option<KernelCursorSurface>,
    init_failed: bool,
    armed_pipe_slot: Option<usize>,
    visible: bool,
    probe_logged: bool,
    saw_real_cursor: bool,
    last_slot_id: u32,
    last_buttons_down: u32,
    last_color: u32,
    last_rect_dim: u32,
    last_x: i32,
    last_y: i32,
    last_probe_log_tick: u64,
}

impl KernelCursorState {
    const fn new() -> Self {
        Self {
            surface: None,
            init_failed: false,
            armed_pipe_slot: None,
            visible: false,
            probe_logged: false,
            saw_real_cursor: false,
            last_slot_id: 0,
            last_buttons_down: 0,
            last_color: 0,
            last_rect_dim: 0,
            last_x: i32::MIN,
            last_y: i32::MIN,
            last_probe_log_tick: 0,
        }
    }
}

pub(crate) fn update_kernel_hw_cursor() -> Option<u32> {
    let dev = crate::intel::claimed_device()?;
    let pipe = active_pipe(dev)?;
    let pipe_src_raw = crate::intel::mmio_read(dev, pipe.pipe_src_off);
    let pipe_src_dims = decode_pipe_src(pipe_src_raw);
    let fb_dims = framebuffer_hint();
    let (scanout_w, scanout_h) = pipe_src_dims.or(fb_dims)?;

    let first_cursor = crate::r::cursor::preferred_kernel_hw_cursor_snapshot_with_slot_buttons();
    let allow_center_probe_fallback = if KERNEL_CURSOR_PROBE_ONLY {
        let state = KERNEL_CURSOR_STATE.lock();
        !state.saw_real_cursor
    } else {
        false
    };
    let (slot_id, nx, ny, buttons_down, centered_fallback) = match first_cursor {
        Some((slot_id, nx, ny, buttons_down)) => (slot_id, nx, ny, buttons_down, false),
        None if allow_center_probe_fallback => (0, 0.5, 0.5, 0, true),
        None => {
            disable_kernel_hw_cursor(dev, None);
            return None;
        }
    };

    let rect_dim = kernel_cursor_visible_rect_dim(scanout_h);
    let mut x_px = normalized_cursor_to_px(nx, scanout_w, KERNEL_CURSOR_HOTSPOT_X);
    let mut y_px = normalized_cursor_to_px(ny, scanout_h, KERNEL_CURSOR_HOTSPOT_Y);
    if KERNEL_CURSOR_PROBE_ONLY && KERNEL_CURSOR_PROBE_KIND.forces_visible_probe_position() {
        (x_px, y_px) = clamp_cursor_probe_position(x_px, y_px, scanout_w, scanout_h, rect_dim);
    }
    let pressed = buttons_down != 0;
    let texture_variant = pressed as u32;

    let regs = cursor_regs_for_pipe(pipe.slot)?;
    let mut state = KERNEL_CURSOR_STATE.lock();
    if !centered_fallback {
        state.saw_real_cursor = true;
    }
    let surface = ensure_kernel_cursor_surface(dev, &mut state)?;

    let pipe_changed = state.armed_pipe_slot != Some(pipe.slot);
    if pipe_changed {
        disable_cursor_pipe(dev, state.armed_pipe_slot);
    }

    if pipe_changed
        || state.last_slot_id != slot_id
        || state.last_buttons_down != buttons_down
        || state.last_color != texture_variant
        || state.last_rect_dim != rect_dim
    {
        draw_kernel_cursor_surface(surface, rect_dim, pressed);
        crate::intel::dma_flush(
            surface.virt,
            (surface.pitch_bytes as usize).saturating_mul(surface.height as usize),
        );
    }

    let pos = cursor_pos_reg_value(x_px, y_px);
    let base = u32::try_from(KERNEL_CURSOR_GPU_ADDR).ok()?;
    let ctl_mode = kernel_cursor_ctl_mode_bits(dev);
    let buf_cfg_target = kernel_cursor_buf_cfg_target(dev)?;
    let ctl = CURSOR_ENABLE | ctl_mode;
    let probe_ctl = match KERNEL_CURSOR_PROBE_KIND {
        CursorProbeKind::CurCtlModeOnly | CursorProbeKind::CurCtlObserveOnly => ctl_mode,
        _ => ctl,
    };

    if KERNEL_CURSOR_PROBE_ONLY {
        let probe_changed = !state.probe_logged
            || pipe_changed
            || state.last_slot_id != slot_id
            || state.last_x != x_px
            || state.last_y != y_px;
        let should_log_probe = probe_changed && kernel_cursor_probe_log_due(&state);
        if should_log_probe {
            log_kernel_cursor_probe(
                dev,
                pipe,
                regs,
                pipe_src_raw,
                pipe_src_dims,
                fb_dims,
                slot_id,
                buttons_down,
                x_px,
                y_px,
                probe_ctl,
                base,
                pos,
            );
            if kernel_cursor_probe_logs_enabled() {
                crate::log_trace!(
                    "intel/display: kernel-hw-cursor probe-only pipe={} slot={} kind={} pos={}x{} ctl_target=0x{:08X} base_target=0x{:08X} buf_cfg_target=0x{:08X} reason=cursor-enable-wedges-display\n",
                    pipe.name,
                    slot_id,
                    KERNEL_CURSOR_PROBE_KIND.label(),
                    x_px,
                    y_px,
                    probe_ctl,
                    base,
                    buf_cfg_target
                );
            }
        }
        if probe_changed {
            match KERNEL_CURSOR_PROBE_KIND {
                CursorProbeKind::CurPosOnly => {
                    probe_cursor_pos_write(dev, pipe, regs, slot_id, x_px, y_px, pos)
                }
                CursorProbeKind::CurBaseOnly => {
                    probe_cursor_base_write(dev, pipe, regs, slot_id, base)
                }
                CursorProbeKind::CurBufCfgOnly => {
                    probe_cursor_buf_cfg_write(dev, pipe, slot_id, buf_cfg_target)
                }
                CursorProbeKind::CurBufCfgBaseOnly => {
                    probe_cursor_buf_cfg_base_write(dev, pipe, regs, slot_id, buf_cfg_target, base)
                }
                CursorProbeKind::CurBufCfgBasePosOnly => probe_cursor_buf_cfg_base_pos_write(
                    dev,
                    pipe,
                    regs,
                    slot_id,
                    buf_cfg_target,
                    base,
                    pos,
                ),
                CursorProbeKind::CurBufCfgBasePosCtlModeOnly => {
                    probe_cursor_buf_cfg_base_pos_ctl_mode_write(
                        dev,
                        pipe,
                        regs,
                        slot_id,
                        buf_cfg_target,
                        base,
                        pos,
                        ctl_mode,
                    )
                }
                CursorProbeKind::CurBufCfgBasePosCtlEnable => {
                    probe_cursor_buf_cfg_base_pos_ctl_enable_write(
                        dev,
                        pipe,
                        regs,
                        slot_id,
                        buf_cfg_target,
                        base,
                        pos,
                        ctl,
                    )
                }
                CursorProbeKind::CurBufCfgBasePosCtlEnableBase => {
                    probe_cursor_buf_cfg_base_pos_ctl_enable_base_write(
                        dev,
                        pipe,
                        regs,
                        slot_id,
                        buf_cfg_target,
                        base,
                        pos,
                        ctl,
                    )
                }
                CursorProbeKind::CurBufCfgCtlEnablePosBase => {
                    probe_cursor_buf_cfg_ctl_enable_pos_base_write(
                        dev,
                        pipe,
                        regs,
                        slot_id,
                        buf_cfg_target,
                        base,
                        pos,
                        ctl,
                    )
                }
                CursorProbeKind::CurWmEnableBufCfgCtlEnablePosBase
                | CursorProbeKind::CurWmEnableBufCfgCtlEnablePosBaseVisible => {
                    probe_cursor_wm_enable_buf_cfg_ctl_enable_pos_base_write(
                        dev,
                        pipe,
                        regs,
                        slot_id,
                        buf_cfg_target,
                        base,
                        pos,
                        ctl,
                    )
                }
                CursorProbeKind::CurCtlModeOnly => {
                    probe_cursor_ctl_mode_write(dev, pipe, regs, slot_id, ctl_mode)
                }
                CursorProbeKind::CurCtlObserveOnly => {
                    probe_cursor_ctl_mode_observe(dev, pipe, regs, slot_id, ctl_mode)
                }
            }
            state.probe_logged = true;
            if should_log_probe {
                state.last_probe_log_tick = kernel_cursor_probe_now_ticks();
            }
        }
        state.armed_pipe_slot = Some(pipe.slot);
        state.visible = false;
        state.last_slot_id = slot_id;
        state.last_buttons_down = buttons_down;
        state.last_color = texture_variant;
        state.last_rect_dim = rect_dim;
        state.last_x = x_px;
        state.last_y = y_px;
        return None;
    }

    if pipe_changed
        || !state.visible
        || state.last_color != texture_variant
        || state.last_slot_id != slot_id
        || state.last_buttons_down != buttons_down
    {
        crate::intel::mmio_write(dev, regs.ctl_off, ctl);
        crate::intel::mmio_write(dev, regs.pos_off, pos);
        crate::intel::mmio_write(dev, regs.base_off, base);
    } else if state.last_x != x_px || state.last_y != y_px {
        crate::intel::mmio_write(dev, regs.pos_off, pos);
        crate::intel::mmio_write(dev, regs.base_off, base);
    }

    let surf_live = crate::intel::mmio_read(dev, regs.surf_live_off);
    let ctl_live = crate::intel::mmio_read(dev, regs.ctl_off);
    let visible = (ctl_live & CURSOR_ENABLE) != 0
        && (ctl_live & CURSOR_MODE_MASK) != CURSOR_MODE_DISABLE
        && (surf_live == 0 || surf_live == base);

    if !state.visible && visible {
        crate::log_trace!(
            "intel/display: kernel-hw-cursor enabled pipe={} slot={} pos={}x{} ctl=0x{:08X} base=0x{:08X} live=0x{:08X}\n",
            pipe.name,
            slot_id,
            x_px,
            y_px,
            ctl_live,
            base,
            surf_live
        );
    } else if pipe_changed {
        crate::log_trace!(
            "intel/display: kernel-hw-cursor rebind pipe={} slot={} pos={}x{} ctl=0x{:08X} base=0x{:08X} live=0x{:08X}\n",
            pipe.name,
            slot_id,
            x_px,
            y_px,
            ctl_live,
            base,
            surf_live
        );
    }

    state.armed_pipe_slot = Some(pipe.slot);
    state.visible = visible;
    state.probe_logged = false;
    state.last_slot_id = slot_id;
    state.last_buttons_down = buttons_down;
    state.last_color = texture_variant;
    state.last_rect_dim = rect_dim;
    state.last_x = x_px;
    state.last_y = y_px;

    if visible { Some(slot_id) } else { None }
}

pub(crate) fn kernel_hw_cursor_slot() -> Option<u32> {
    let state = KERNEL_CURSOR_STATE.lock();
    if state.visible {
        Some(state.last_slot_id)
    } else {
        None
    }
}

pub(crate) fn log_cursor_ddb_map_once(dev: crate::intel::Dev) {
    if KERNEL_CURSOR_DDB_MAP_LOGGED.swap(true, Ordering::AcqRel) {
        return;
    }

    let pipe_a_plane0 = plane_buf_cfg_for_pipe_slot(dev, PIPES[0], 0);
    let pipe_a_plane1 = plane_buf_cfg_for_pipe_slot(dev, PIPES[0], 1);
    let pipe_a_plane2 = plane_buf_cfg_for_pipe_slot(dev, PIPES[0], 2);
    let pipe_a_plane3 = plane_buf_cfg_for_pipe_slot(dev, PIPES[0], 3);
    let pipe_b_plane0 = plane_buf_cfg_for_pipe_slot(dev, PIPES[1], 0);
    let pipe_b_plane1 = plane_buf_cfg_for_pipe_slot(dev, PIPES[1], 1);
    let pipe_b_plane2 = plane_buf_cfg_for_pipe_slot(dev, PIPES[1], 2);
    let pipe_b_plane3 = plane_buf_cfg_for_pipe_slot(dev, PIPES[1], 3);
    let pipe_c_plane0 = plane_buf_cfg_for_pipe_slot(dev, PIPES[2], 0);
    let pipe_c_plane1 = plane_buf_cfg_for_pipe_slot(dev, PIPES[2], 1);
    let pipe_c_plane2 = plane_buf_cfg_for_pipe_slot(dev, PIPES[2], 2);
    let pipe_c_plane3 = plane_buf_cfg_for_pipe_slot(dev, PIPES[2], 3);
    let pipe_d_plane0 = plane_buf_cfg_for_pipe_slot(dev, PIPES[3], 0);
    let pipe_d_plane1 = plane_buf_cfg_for_pipe_slot(dev, PIPES[3], 1);
    let pipe_d_plane2 = plane_buf_cfg_for_pipe_slot(dev, PIPES[3], 2);
    let pipe_d_plane3 = plane_buf_cfg_for_pipe_slot(dev, PIPES[3], 3);
    let pipe_a_cursor = cursor_aux_regs_for_pipe(0)
        .map(|aux| crate::intel::mmio_read(dev, aux.buf_cfg_off))
        .unwrap_or(0);
    let pipe_b_cursor = cursor_aux_regs_for_pipe(1)
        .map(|aux| crate::intel::mmio_read(dev, aux.buf_cfg_off))
        .unwrap_or(0);
    let pipe_c_cursor = cursor_aux_regs_for_pipe(2)
        .map(|aux| crate::intel::mmio_read(dev, aux.buf_cfg_off))
        .unwrap_or(0);
    let pipe_d_cursor = cursor_aux_regs_for_pipe(3)
        .map(|aux| crate::intel::mmio_read(dev, aux.buf_cfg_off))
        .unwrap_or(0);
    let (pa0_start, pa0_end) = decode_dbuf_cfg(pipe_a_plane0);
    let (pa1_start, pa1_end) = decode_dbuf_cfg(pipe_a_plane1);
    let (pa2_start, pa2_end) = decode_dbuf_cfg(pipe_a_plane2);
    let (pa3_start, pa3_end) = decode_dbuf_cfg(pipe_a_plane3);
    let (pb0_start, pb0_end) = decode_dbuf_cfg(pipe_b_plane0);
    let (pb1_start, pb1_end) = decode_dbuf_cfg(pipe_b_plane1);
    let (pb2_start, pb2_end) = decode_dbuf_cfg(pipe_b_plane2);
    let (pb3_start, pb3_end) = decode_dbuf_cfg(pipe_b_plane3);
    let (pc0_start, pc0_end) = decode_dbuf_cfg(pipe_c_plane0);
    let (pc1_start, pc1_end) = decode_dbuf_cfg(pipe_c_plane1);
    let (pc2_start, pc2_end) = decode_dbuf_cfg(pipe_c_plane2);
    let (pc3_start, pc3_end) = decode_dbuf_cfg(pipe_c_plane3);
    let (pd0_start, pd0_end) = decode_dbuf_cfg(pipe_d_plane0);
    let (pd1_start, pd1_end) = decode_dbuf_cfg(pipe_d_plane1);
    let (pd2_start, pd2_end) = decode_dbuf_cfg(pipe_d_plane2);
    let (pd3_start, pd3_end) = decode_dbuf_cfg(pipe_d_plane3);
    let (ca_start, ca_end) = decode_dbuf_cfg(pipe_a_cursor);
    let (cb_start, cb_end) = decode_dbuf_cfg(pipe_b_cursor);
    let (cc_start, cc_end) = decode_dbuf_cfg(pipe_c_cursor);
    let (cd_start, cd_end) = decode_dbuf_cfg(pipe_d_cursor);

    crate::log_trace!(
        "intel/display: cursor-ddb-map plane[a0]=0x{:08X} {}..{} plane[a1]=0x{:08X} {}..{} plane[a2]=0x{:08X} {}..{} plane[a3]=0x{:08X} {}..{} plane[b0]=0x{:08X} {}..{} plane[b1]=0x{:08X} {}..{} plane[b2]=0x{:08X} {}..{} plane[b3]=0x{:08X} {}..{} plane[c0]=0x{:08X} {}..{} plane[c1]=0x{:08X} {}..{} plane[c2]=0x{:08X} {}..{} plane[c3]=0x{:08X} {}..{} plane[d0]=0x{:08X} {}..{} plane[d1]=0x{:08X} {}..{} plane[d2]=0x{:08X} {}..{} plane[d3]=0x{:08X} {}..{} cursor[a]=0x{:08X} {}..{} cursor[b]=0x{:08X} {}..{} cursor[c]=0x{:08X} {}..{} cursor[d]=0x{:08X} {}..{}\n",
        pipe_a_plane0,
        pa0_start,
        pa0_end,
        pipe_a_plane1,
        pa1_start,
        pa1_end,
        pipe_a_plane2,
        pa2_start,
        pa2_end,
        pipe_a_plane3,
        pa3_start,
        pa3_end,
        pipe_b_plane0,
        pb0_start,
        pb0_end,
        pipe_b_plane1,
        pb1_start,
        pb1_end,
        pipe_b_plane2,
        pb2_start,
        pb2_end,
        pipe_b_plane3,
        pb3_start,
        pb3_end,
        pipe_c_plane0,
        pc0_start,
        pc0_end,
        pipe_c_plane1,
        pc1_start,
        pc1_end,
        pipe_c_plane2,
        pc2_start,
        pc2_end,
        pipe_c_plane3,
        pc3_start,
        pc3_end,
        pipe_d_plane0,
        pd0_start,
        pd0_end,
        pipe_d_plane1,
        pd1_start,
        pd1_end,
        pipe_d_plane2,
        pd2_start,
        pd2_end,
        pipe_d_plane3,
        pd3_start,
        pd3_end,
        pipe_a_cursor,
        ca_start,
        ca_end,
        pipe_b_cursor,
        cb_start,
        cb_end,
        pipe_c_cursor,
        cc_start,
        cc_end,
        pipe_d_cursor,
        cd_start,
        cd_end,
    );
}

fn cursor_regs_for_pipe(slot: usize) -> Option<CursorRegs> {
    let base = match slot {
        0 => CURSOR_A_BASE,
        1 => CURSOR_B_BASE,
        2 => CURSOR_C_BASE,
        3 => CURSOR_D_BASE,
        _ => return None,
    };
    Some(CursorRegs {
        ctl_off: base + CURSOR_CTL_OFF,
        base_off: base + CURSOR_BASE_OFF,
        pos_off: base + CURSOR_POS_OFF,
        surf_live_off: base + CURSOR_SURF_LIVE_OFF,
    })
}

fn cursor_aux_regs_for_pipe(slot: usize) -> Option<CursorAuxRegs> {
    match slot {
        0 => Some(CursorAuxRegs {
            wm0_off: CURSOR_WM_A0,
            wm_trans_off: CURSOR_WM_TRANS_A,
            wm_sagv_off: CURSOR_WM_SAGV_A,
            wm_sagv_trans_off: CURSOR_WM_SAGV_TRANS_A,
            buf_cfg_off: CURSOR_BUF_CFG_A,
            sel_fetch_ctl_off: SEL_FETCH_CUR_CTL_A,
        }),
        1 => Some(CursorAuxRegs {
            wm0_off: CURSOR_WM_B0,
            wm_trans_off: CURSOR_WM_TRANS_B,
            wm_sagv_off: CURSOR_WM_SAGV_B,
            wm_sagv_trans_off: CURSOR_WM_SAGV_TRANS_B,
            buf_cfg_off: CURSOR_BUF_CFG_B,
            sel_fetch_ctl_off: SEL_FETCH_CUR_CTL_B,
        }),
        2 => Some(CursorAuxRegs {
            wm0_off: CURSOR_WM_B0 + 0x1000,
            wm_trans_off: CURSOR_WM_TRANS_B + 0x1000,
            wm_sagv_off: CURSOR_WM_SAGV_B + 0x1000,
            wm_sagv_trans_off: CURSOR_WM_SAGV_TRANS_B + 0x1000,
            buf_cfg_off: CURSOR_BUF_CFG_B + 0x1000,
            sel_fetch_ctl_off: SEL_FETCH_CUR_CTL_B + 0x1000,
        }),
        3 => Some(CursorAuxRegs {
            wm0_off: CURSOR_WM_B0 + 0x2000,
            wm_trans_off: CURSOR_WM_TRANS_B + 0x2000,
            wm_sagv_off: CURSOR_WM_SAGV_B + 0x2000,
            wm_sagv_trans_off: CURSOR_WM_SAGV_TRANS_B + 0x2000,
            buf_cfg_off: CURSOR_BUF_CFG_B + 0x2000,
            sel_fetch_ctl_off: SEL_FETCH_CUR_CTL_B + 0x2000,
        }),
        _ => None,
    }
}

fn disable_kernel_hw_cursor(dev: crate::intel::Dev, preferred_pipe: Option<PipeInfo>) {
    let mut state = KERNEL_CURSOR_STATE.lock();
    if !state.visible && state.armed_pipe_slot.is_none() {
        return;
    }

    let pipe_slot = preferred_pipe
        .map(|pipe| pipe.slot)
        .or(state.armed_pipe_slot);
    disable_cursor_pipe(dev, pipe_slot);
    if state.visible {
        crate::log_trace!(
            "intel/display: kernel-hw-cursor disabled pipe_slot={}\n",
            pipe_slot.unwrap_or(usize::MAX)
        );
    }
    state.armed_pipe_slot = pipe_slot;
    state.visible = false;
    state.probe_logged = false;
    state.last_x = i32::MIN;
    state.last_y = i32::MIN;
}

#[inline]
fn kernel_cursor_probe_logs_enabled() -> bool {
    crate::logflag::INTEL_CURSOR_PROBE_LOGS
}

fn log_kernel_cursor_probe(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    regs: CursorRegs,
    pipe_src_raw: u32,
    pipe_src_dims: Option<(u32, u32)>,
    fb_dims: Option<(u32, u32)>,
    slot_id: u32,
    buttons_down: u32,
    x_px: i32,
    y_px: i32,
    ctl_target: u32,
    base_target: u32,
    pos_target: u32,
) {
    if !kernel_cursor_probe_logs_enabled() {
        return;
    }

    log_cursor_ddb_map_once(dev);

    let aux = cursor_aux_regs_for_pipe(pipe.slot);
    let plane_ctl = crate::intel::mmio_read(dev, pipe.plane_ctl_off);
    let plane_stride = crate::intel::mmio_read(dev, pipe.plane_stride_off);
    let plane_surf = crate::intel::mmio_read(dev, pipe.plane_surf_off);
    let plane_surf_live = crate::intel::mmio_read(dev, pipe.plane_surf_live_off);
    let plane_buf_cfg = plane_buf_cfg_for_pipe_slot(dev, pipe, 0);
    let cursor_ctl = crate::intel::mmio_read(dev, regs.ctl_off);
    let cursor_base = crate::intel::mmio_read(dev, regs.base_off);
    let cursor_pos = crate::intel::mmio_read(dev, regs.pos_off);
    let cursor_live = crate::intel::mmio_read(dev, regs.surf_live_off);
    let cursor_wm0 = aux
        .map(|aux| crate::intel::mmio_read(dev, aux.wm0_off))
        .unwrap_or(0);
    let cursor_wm_trans = aux
        .map(|aux| crate::intel::mmio_read(dev, aux.wm_trans_off))
        .unwrap_or(0);
    let cursor_wm_sagv = aux
        .map(|aux| crate::intel::mmio_read(dev, aux.wm_sagv_off))
        .unwrap_or(0);
    let cursor_wm_sagv_trans = aux
        .map(|aux| crate::intel::mmio_read(dev, aux.wm_sagv_trans_off))
        .unwrap_or(0);
    let cursor_buf_cfg = aux
        .map(|aux| crate::intel::mmio_read(dev, aux.buf_cfg_off))
        .unwrap_or(0);
    let cursor_sel_fetch_ctl = aux
        .map(|aux| crate::intel::mmio_read(dev, aux.sel_fetch_ctl_off))
        .unwrap_or(0);
    let (pipe_w, pipe_h) = pipe_src_dims.unwrap_or((0, 0));
    let (fb_w, fb_h) = fb_dims.unwrap_or((0, 0));

    crate::log_trace!(
        "intel/display: kernel-hw-cursor probe pipe={} slot={} buttons=0x{:X} pipe_src=0x{:08X} pipe_dims={}x{} fb_dims={}x{} plane_ctl=0x{:08X} plane_enabled={} plane_stride=0x{:08X} plane_surf=0x{:08X} plane_live=0x{:08X} plane_buf_cfg=0x{:08X} cur_ctl=0x{:08X} cur_base=0x{:08X} cur_pos=0x{:08X} cur_live=0x{:08X} cur_wm0=0x{:08X} cur_wm_trans=0x{:08X} cur_wm_sagv=0x{:08X} cur_wm_sagv_trans=0x{:08X} cur_buf_cfg=0x{:08X} cur_sel_fetch=0x{:08X} target_pos={}x{} target_pos_reg=0x{:08X} target_ctl=0x{:08X} target_base=0x{:08X}\n",
        pipe.name,
        slot_id,
        buttons_down,
        pipe_src_raw,
        pipe_w,
        pipe_h,
        fb_w,
        fb_h,
        plane_ctl,
        ((plane_ctl & PLANE_CTL_ENABLE) != 0) as u8,
        plane_stride,
        plane_surf,
        plane_surf_live,
        plane_buf_cfg,
        cursor_ctl,
        cursor_base,
        cursor_pos,
        cursor_live,
        cursor_wm0,
        cursor_wm_trans,
        cursor_wm_sagv,
        cursor_wm_sagv_trans,
        cursor_buf_cfg,
        cursor_sel_fetch_ctl,
        x_px,
        y_px,
        pos_target,
        ctl_target,
        base_target
    );
}

fn decode_dbuf_cfg(value: u32) -> (u32, u32) {
    if value == 0 {
        return (0, 0);
    }

    let start = value & 0x0FFF;
    let end = ((value >> 16) & 0x0FFF).saturating_add(1);
    (start, end)
}

fn encode_dbuf_cfg(start: u32, end: u32) -> u32 {
    if end == 0 || end <= start {
        return 0;
    }

    ((end - 1) << 16) | start
}

fn kernel_cursor_buf_cfg_target(dev: crate::intel::Dev) -> Option<u32> {
    let total_blocks = match dev.device_id {
        0x4680 | 0x4682 | 0x4688 | 0x468A | 0x468B | 0x4690 | 0x4692 | 0x4693 => {
            KERNEL_CURSOR_DBUF_BLOCKS_ADL_S
        }
        _ => return None,
    };

    let start = total_blocks.checked_sub(KERNEL_CURSOR_DBUF_BLOCKS_ONE_PIPE)?;
    Some(encode_dbuf_cfg(start, total_blocks))
}

fn probe_cursor_pos_write(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    regs: CursorRegs,
    slot_id: u32,
    x_px: i32,
    y_px: i32,
    pos_target: u32,
) {
    let pos_before = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_before = crate::intel::mmio_read(dev, regs.ctl_off);
    let base_before = crate::intel::mmio_read(dev, regs.base_off);
    let live_before = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::intel::mmio_write(dev, regs.pos_off, pos_target);

    let pos_after = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_after = crate::intel::mmio_read(dev, regs.ctl_off);
    let base_after = crate::intel::mmio_read(dev, regs.base_off);
    let live_after = crate::intel::mmio_read(dev, regs.surf_live_off);

    if kernel_cursor_probe_logs_enabled() {
        crate::log_trace!(
            "intel/display: kernel-hw-cursor probe-curpos pipe={} slot={} target={}x{} pos_before=0x{:08X} pos_after=0x{:08X} ctl_before=0x{:08X} ctl_after=0x{:08X} base_before=0x{:08X} base_after=0x{:08X} live_before=0x{:08X} live_after=0x{:08X}\n",
            pipe.name,
            slot_id,
            x_px,
            y_px,
            pos_before,
            pos_after,
            ctl_before,
            ctl_after,
            base_before,
            base_after,
            live_before,
            live_after
        );
    }
}

fn probe_cursor_buf_cfg_write(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    slot_id: u32,
    buf_cfg_target: u32,
) {
    let Some(aux) = cursor_aux_regs_for_pipe(pipe.slot) else {
        return;
    };

    let buf_cfg_before = crate::intel::mmio_read(dev, aux.buf_cfg_off);
    let wm0_before = crate::intel::mmio_read(dev, aux.wm0_off);
    let wm_trans_before = crate::intel::mmio_read(dev, aux.wm_trans_off);

    crate::intel::mmio_write(dev, aux.buf_cfg_off, buf_cfg_target);

    let buf_cfg_after = crate::intel::mmio_read(dev, aux.buf_cfg_off);
    let wm0_after = crate::intel::mmio_read(dev, aux.wm0_off);
    let wm_trans_after = crate::intel::mmio_read(dev, aux.wm_trans_off);

    if kernel_cursor_probe_logs_enabled() {
        crate::log_trace!(
            "intel/display: kernel-hw-cursor probe-curbufcfg pipe={} slot={} buf_cfg_before=0x{:08X} buf_cfg_after=0x{:08X} wm0_before=0x{:08X} wm0_after=0x{:08X} wm_trans_before=0x{:08X} wm_trans_after=0x{:08X}\n",
            pipe.name,
            slot_id,
            buf_cfg_before,
            buf_cfg_after,
            wm0_before,
            wm0_after,
            wm_trans_before,
            wm_trans_after
        );
    }
}

fn probe_cursor_buf_cfg_base_write(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    regs: CursorRegs,
    slot_id: u32,
    buf_cfg_target: u32,
    base_target: u32,
) {
    let Some(aux) = cursor_aux_regs_for_pipe(pipe.slot) else {
        return;
    };

    let buf_cfg_before = crate::intel::mmio_read(dev, aux.buf_cfg_off);
    let base_before = crate::intel::mmio_read(dev, regs.base_off);
    let ctl_before = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_before = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::intel::mmio_write(dev, aux.buf_cfg_off, buf_cfg_target);
    crate::intel::mmio_write(dev, regs.base_off, base_target);

    let buf_cfg_after = crate::intel::mmio_read(dev, aux.buf_cfg_off);
    let base_after = crate::intel::mmio_read(dev, regs.base_off);
    let ctl_after = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_after = crate::intel::mmio_read(dev, regs.surf_live_off);

    if kernel_cursor_probe_logs_enabled() {
        crate::log_trace!(
            "intel/display: kernel-hw-cursor probe-curbufcfg-base pipe={} slot={} buf_cfg_before=0x{:08X} buf_cfg_after=0x{:08X} base_before=0x{:08X} base_after=0x{:08X} ctl_before=0x{:08X} ctl_after=0x{:08X} live_before=0x{:08X} live_after=0x{:08X}\n",
            pipe.name,
            slot_id,
            buf_cfg_before,
            buf_cfg_after,
            base_before,
            base_after,
            ctl_before,
            ctl_after,
            live_before,
            live_after
        );
    }
}

fn probe_cursor_buf_cfg_base_pos_write(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    regs: CursorRegs,
    slot_id: u32,
    buf_cfg_target: u32,
    base_target: u32,
    pos_target: u32,
) {
    let Some(aux) = cursor_aux_regs_for_pipe(pipe.slot) else {
        return;
    };

    let buf_cfg_before = crate::intel::mmio_read(dev, aux.buf_cfg_off);
    let base_before = crate::intel::mmio_read(dev, regs.base_off);
    let pos_before = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_before = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_before = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::intel::mmio_write(dev, aux.buf_cfg_off, buf_cfg_target);
    crate::intel::mmio_write(dev, regs.base_off, base_target);
    crate::intel::mmio_write(dev, regs.pos_off, pos_target);

    let buf_cfg_after = crate::intel::mmio_read(dev, aux.buf_cfg_off);
    let base_after = crate::intel::mmio_read(dev, regs.base_off);
    let pos_after = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_after = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_after = crate::intel::mmio_read(dev, regs.surf_live_off);

    if kernel_cursor_probe_logs_enabled() {
        crate::log_trace!(
            "intel/display: kernel-hw-cursor probe-curbufcfg-base-pos pipe={} slot={} buf_cfg_before=0x{:08X} buf_cfg_after=0x{:08X} base_before=0x{:08X} base_after=0x{:08X} pos_before=0x{:08X} pos_after=0x{:08X} ctl_before=0x{:08X} ctl_after=0x{:08X} live_before=0x{:08X} live_after=0x{:08X}\n",
            pipe.name,
            slot_id,
            buf_cfg_before,
            buf_cfg_after,
            base_before,
            base_after,
            pos_before,
            pos_after,
            ctl_before,
            ctl_after,
            live_before,
            live_after
        );
    }
}

fn probe_cursor_buf_cfg_base_pos_ctl_mode_write(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    regs: CursorRegs,
    slot_id: u32,
    buf_cfg_target: u32,
    base_target: u32,
    pos_target: u32,
    ctl_target: u32,
) {
    let Some(aux) = cursor_aux_regs_for_pipe(pipe.slot) else {
        return;
    };

    let buf_cfg_before = crate::intel::mmio_read(dev, aux.buf_cfg_off);
    let base_before = crate::intel::mmio_read(dev, regs.base_off);
    let pos_before = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_before = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_before = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::intel::mmio_write(dev, aux.buf_cfg_off, buf_cfg_target);
    crate::intel::mmio_write(dev, regs.base_off, base_target);
    crate::intel::mmio_write(dev, regs.pos_off, pos_target);
    crate::intel::mmio_write(dev, regs.ctl_off, ctl_target);

    let buf_cfg_after = crate::intel::mmio_read(dev, aux.buf_cfg_off);
    let base_after = crate::intel::mmio_read(dev, regs.base_off);
    let pos_after = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_after = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_after = crate::intel::mmio_read(dev, regs.surf_live_off);

    if kernel_cursor_probe_logs_enabled() {
        crate::log_trace!(
            "intel/display: kernel-hw-cursor probe-curbufcfg-base-pos-ctlmode pipe={} slot={} buf_cfg_before=0x{:08X} buf_cfg_after=0x{:08X} base_before=0x{:08X} base_after=0x{:08X} pos_before=0x{:08X} pos_after=0x{:08X} ctl_before=0x{:08X} ctl_after=0x{:08X} live_before=0x{:08X} live_after=0x{:08X}\n",
            pipe.name,
            slot_id,
            buf_cfg_before,
            buf_cfg_after,
            base_before,
            base_after,
            pos_before,
            pos_after,
            ctl_before,
            ctl_after,
            live_before,
            live_after
        );
    }
}

fn probe_cursor_buf_cfg_base_pos_ctl_enable_write(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    regs: CursorRegs,
    slot_id: u32,
    buf_cfg_target: u32,
    base_target: u32,
    pos_target: u32,
    ctl_target: u32,
) {
    let Some(aux) = cursor_aux_regs_for_pipe(pipe.slot) else {
        return;
    };

    let buf_cfg_before = crate::intel::mmio_read(dev, aux.buf_cfg_off);
    let base_before = crate::intel::mmio_read(dev, regs.base_off);
    let pos_before = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_before = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_before = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::intel::mmio_write(dev, aux.buf_cfg_off, buf_cfg_target);
    crate::intel::mmio_write(dev, regs.base_off, base_target);
    crate::intel::mmio_write(dev, regs.pos_off, pos_target);
    crate::intel::mmio_write(dev, regs.ctl_off, ctl_target);

    let buf_cfg_after = crate::intel::mmio_read(dev, aux.buf_cfg_off);
    let base_after = crate::intel::mmio_read(dev, regs.base_off);
    let pos_after = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_after = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_after = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::log_trace!(
        "intel/display: kernel-hw-cursor probe-curbufcfg-base-pos-ctlenable pipe={} slot={} buf_cfg_before=0x{:08X} buf_cfg_after=0x{:08X} base_before=0x{:08X} base_after=0x{:08X} pos_before=0x{:08X} pos_after=0x{:08X} ctl_before=0x{:08X} ctl_after=0x{:08X} live_before=0x{:08X} live_after=0x{:08X}\n",
        pipe.name,
        slot_id,
        buf_cfg_before,
        buf_cfg_after,
        base_before,
        base_after,
        pos_before,
        pos_after,
        ctl_before,
        ctl_after,
        live_before,
        live_after
    );
}

fn probe_cursor_buf_cfg_base_pos_ctl_enable_base_write(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    regs: CursorRegs,
    slot_id: u32,
    buf_cfg_target: u32,
    base_target: u32,
    pos_target: u32,
    ctl_target: u32,
) {
    let Some(aux) = cursor_aux_regs_for_pipe(pipe.slot) else {
        return;
    };

    let buf_cfg_before = crate::intel::mmio_read(dev, aux.buf_cfg_off);
    let base_before = crate::intel::mmio_read(dev, regs.base_off);
    let pos_before = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_before = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_before = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::intel::mmio_write(dev, aux.buf_cfg_off, buf_cfg_target);
    crate::intel::mmio_write(dev, regs.base_off, base_target);
    crate::intel::mmio_write(dev, regs.pos_off, pos_target);
    crate::intel::mmio_write(dev, regs.ctl_off, ctl_target);

    let ctl_after_ctl = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_after_ctl = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::intel::mmio_write(dev, regs.base_off, base_target);

    let buf_cfg_after = crate::intel::mmio_read(dev, aux.buf_cfg_off);
    let base_after = crate::intel::mmio_read(dev, regs.base_off);
    let pos_after = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_after = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_after = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::log_trace!(
        "intel/display: kernel-hw-cursor probe-curbufcfg-base-pos-ctlenable-base pipe={} slot={} buf_cfg_before=0x{:08X} buf_cfg_after=0x{:08X} base_before=0x{:08X} base_after=0x{:08X} pos_before=0x{:08X} pos_after=0x{:08X} ctl_before=0x{:08X} ctl_after_ctl=0x{:08X} ctl_after=0x{:08X} live_before=0x{:08X} live_after_ctl=0x{:08X} live_after=0x{:08X}\n",
        pipe.name,
        slot_id,
        buf_cfg_before,
        buf_cfg_after,
        base_before,
        base_after,
        pos_before,
        pos_after,
        ctl_before,
        ctl_after_ctl,
        ctl_after,
        live_before,
        live_after_ctl,
        live_after
    );
}

fn probe_cursor_buf_cfg_ctl_enable_pos_base_write(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    regs: CursorRegs,
    slot_id: u32,
    buf_cfg_target: u32,
    base_target: u32,
    pos_target: u32,
    ctl_target: u32,
) {
    let Some(aux) = cursor_aux_regs_for_pipe(pipe.slot) else {
        return;
    };

    let buf_cfg_before = crate::intel::mmio_read(dev, aux.buf_cfg_off);
    let base_before = crate::intel::mmio_read(dev, regs.base_off);
    let pos_before = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_before = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_before = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::intel::mmio_write(dev, aux.buf_cfg_off, buf_cfg_target);
    crate::intel::mmio_write(dev, regs.ctl_off, ctl_target);

    let ctl_after_ctl = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_after_ctl = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::intel::mmio_write(dev, regs.pos_off, pos_target);
    crate::intel::mmio_write(dev, regs.base_off, base_target);

    let buf_cfg_after = crate::intel::mmio_read(dev, aux.buf_cfg_off);
    let base_after = crate::intel::mmio_read(dev, regs.base_off);
    let pos_after = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_after = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_after = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::log_trace!(
        "intel/display: kernel-hw-cursor probe-curbufcfg-ctlenable-pos-base pipe={} slot={} buf_cfg_before=0x{:08X} buf_cfg_after=0x{:08X} base_before=0x{:08X} base_after=0x{:08X} pos_before=0x{:08X} pos_after=0x{:08X} ctl_before=0x{:08X} ctl_after_ctl=0x{:08X} ctl_after=0x{:08X} live_before=0x{:08X} live_after_ctl=0x{:08X} live_after=0x{:08X}\n",
        pipe.name,
        slot_id,
        buf_cfg_before,
        buf_cfg_after,
        base_before,
        base_after,
        pos_before,
        pos_after,
        ctl_before,
        ctl_after_ctl,
        ctl_after,
        live_before,
        live_after_ctl,
        live_after
    );
}

fn probe_cursor_wm_enable_buf_cfg_ctl_enable_pos_base_write(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    regs: CursorRegs,
    slot_id: u32,
    buf_cfg_target: u32,
    base_target: u32,
    pos_target: u32,
    ctl_target: u32,
) {
    let Some(aux) = cursor_aux_regs_for_pipe(pipe.slot) else {
        return;
    };

    let wm0_before = crate::intel::mmio_read(dev, aux.wm0_off);
    let wm_trans_before = crate::intel::mmio_read(dev, aux.wm_trans_off);
    let wm_sagv_before = crate::intel::mmio_read(dev, aux.wm_sagv_off);
    let wm_sagv_trans_before = crate::intel::mmio_read(dev, aux.wm_sagv_trans_off);
    let buf_cfg_before = crate::intel::mmio_read(dev, aux.buf_cfg_off);
    let base_before = crate::intel::mmio_read(dev, regs.base_off);
    let pos_before = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_before = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_before = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::intel::mmio_write(dev, aux.wm0_off, wm0_before | CUR_WM_EN);
    crate::intel::mmio_write(dev, aux.wm_trans_off, wm_trans_before | CUR_WM_EN);
    crate::intel::mmio_write(dev, aux.wm_sagv_off, wm_sagv_before | CUR_WM_EN);
    crate::intel::mmio_write(dev, aux.wm_sagv_trans_off, wm_sagv_trans_before | CUR_WM_EN);
    crate::intel::mmio_write(dev, aux.buf_cfg_off, buf_cfg_target);
    crate::intel::mmio_write(dev, regs.ctl_off, ctl_target);

    let wm0_after_wm = crate::intel::mmio_read(dev, aux.wm0_off);
    let wm_trans_after_wm = crate::intel::mmio_read(dev, aux.wm_trans_off);
    let wm_sagv_after_wm = crate::intel::mmio_read(dev, aux.wm_sagv_off);
    let wm_sagv_trans_after_wm = crate::intel::mmio_read(dev, aux.wm_sagv_trans_off);
    let ctl_after_ctl = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_after_ctl = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::intel::mmio_write(dev, regs.pos_off, pos_target);
    crate::intel::mmio_write(dev, regs.base_off, base_target);

    let buf_cfg_after = crate::intel::mmio_read(dev, aux.buf_cfg_off);
    let base_after = crate::intel::mmio_read(dev, regs.base_off);
    let pos_after = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_after = crate::intel::mmio_read(dev, regs.ctl_off);
    let live_after = crate::intel::mmio_read(dev, regs.surf_live_off);

    if kernel_cursor_probe_logs_enabled() {
        crate::log_trace!(
            "intel/display: kernel-hw-cursor probe-curwm-bufcfg-ctlenable-pos-base pipe={} slot={} wm0_before=0x{:08X} wm0_after_wm=0x{:08X} wm_trans_before=0x{:08X} wm_trans_after_wm=0x{:08X} wm_sagv_before=0x{:08X} wm_sagv_after_wm=0x{:08X} wm_sagv_trans_before=0x{:08X} wm_sagv_trans_after_wm=0x{:08X} buf_cfg_before=0x{:08X} buf_cfg_after=0x{:08X} base_before=0x{:08X} base_after=0x{:08X} pos_before=0x{:08X} pos_after=0x{:08X} ctl_before=0x{:08X} ctl_after_ctl=0x{:08X} ctl_after=0x{:08X} live_before=0x{:08X} live_after_ctl=0x{:08X} live_after=0x{:08X}\n",
            pipe.name,
            slot_id,
            wm0_before,
            wm0_after_wm,
            wm_trans_before,
            wm_trans_after_wm,
            wm_sagv_before,
            wm_sagv_after_wm,
            wm_sagv_trans_before,
            wm_sagv_trans_after_wm,
            buf_cfg_before,
            buf_cfg_after,
            base_before,
            base_after,
            pos_before,
            pos_after,
            ctl_before,
            ctl_after_ctl,
            ctl_after,
            live_before,
            live_after_ctl,
            live_after
        );
    }
}

fn probe_cursor_base_write(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    regs: CursorRegs,
    slot_id: u32,
    base_target: u32,
) {
    let pos_before = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_before = crate::intel::mmio_read(dev, regs.ctl_off);
    let base_before = crate::intel::mmio_read(dev, regs.base_off);
    let live_before = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::intel::mmio_write(dev, regs.base_off, base_target);

    let pos_after = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_after = crate::intel::mmio_read(dev, regs.ctl_off);
    let base_after = crate::intel::mmio_read(dev, regs.base_off);
    let live_after = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::log_trace!(
        "intel/display: kernel-hw-cursor probe-curbase pipe={} slot={} base_before=0x{:08X} base_after=0x{:08X} ctl_before=0x{:08X} ctl_after=0x{:08X} pos_before=0x{:08X} pos_after=0x{:08X} live_before=0x{:08X} live_after=0x{:08X}\n",
        pipe.name,
        slot_id,
        base_before,
        base_after,
        ctl_before,
        ctl_after,
        pos_before,
        pos_after,
        live_before,
        live_after
    );
}

fn probe_cursor_ctl_mode_write(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    regs: CursorRegs,
    slot_id: u32,
    ctl_target: u32,
) {
    let pos_before = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_before = crate::intel::mmio_read(dev, regs.ctl_off);
    let base_before = crate::intel::mmio_read(dev, regs.base_off);
    let live_before = crate::intel::mmio_read(dev, regs.surf_live_off);

    if ctl_before != ctl_target {
        crate::intel::mmio_write(dev, regs.ctl_off, ctl_target);
    }

    let pos_after = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_after = crate::intel::mmio_read(dev, regs.ctl_off);
    let base_after = crate::intel::mmio_read(dev, regs.base_off);
    let live_after = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::log_trace!(
        "intel/display: kernel-hw-cursor probe-curctl-mode pipe={} slot={} ctl_before=0x{:08X} ctl_after=0x{:08X} pos_before=0x{:08X} pos_after=0x{:08X} base_before=0x{:08X} base_after=0x{:08X} live_before=0x{:08X} live_after=0x{:08X}\n",
        pipe.name,
        slot_id,
        ctl_before,
        ctl_after,
        pos_before,
        pos_after,
        base_before,
        base_after,
        live_before,
        live_after
    );
}

fn probe_cursor_ctl_mode_observe(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    regs: CursorRegs,
    slot_id: u32,
    ctl_target: u32,
) {
    let pos_before = crate::intel::mmio_read(dev, regs.pos_off);
    let ctl_before = crate::intel::mmio_read(dev, regs.ctl_off);
    let base_before = crate::intel::mmio_read(dev, regs.base_off);
    let live_before = crate::intel::mmio_read(dev, regs.surf_live_off);

    crate::log_trace!(
        "intel/display: kernel-hw-cursor probe-curctl-observe pipe={} slot={} ctl_before=0x{:08X} ctl_target=0x{:08X} pos_before=0x{:08X} base_before=0x{:08X} live_before=0x{:08X}\n",
        pipe.name,
        slot_id,
        ctl_before,
        ctl_target,
        pos_before,
        base_before,
        live_before
    );
}

fn kernel_cursor_ctl_mode_bits(dev: crate::intel::Dev) -> u32 {
    let mut ctl = CURSOR_MODE_64_ARGB_AX;
    if kernel_cursor_needs_arb_slots(dev.device_id) {
        ctl |= CURSOR_ARB_SLOTS_1;
    }
    ctl
}

fn kernel_cursor_needs_arb_slots(device_id: u16) -> bool {
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

fn kernel_cursor_probe_log_due(state: &KernelCursorState) -> bool {
    if state.last_probe_log_tick == 0 {
        return true;
    }

    let now = kernel_cursor_probe_now_ticks();
    let hz = embassy_time_driver::TICK_HZ.max(1);
    let interval = hz.saturating_mul(KERNEL_CURSOR_PROBE_LOG_INTERVAL_SECS);
    now.saturating_sub(state.last_probe_log_tick) >= interval
}

fn kernel_cursor_probe_now_ticks() -> u64 {
    embassy_time_driver::now()
}

fn disable_cursor_pipe(dev: crate::intel::Dev, pipe_slot: Option<usize>) {
    let Some(pipe_slot) = pipe_slot else {
        return;
    };
    let Some(regs) = cursor_regs_for_pipe(pipe_slot) else {
        return;
    };
    crate::intel::mmio_write(dev, regs.ctl_off, 0);
    crate::intel::mmio_write(dev, regs.base_off, 0);
}

fn ensure_kernel_cursor_surface(
    dev: crate::intel::Dev,
    state: &mut KernelCursorState,
) -> Option<KernelCursorSurface> {
    if let Some(surface) = state.surface {
        return Some(surface);
    }
    if state.init_failed {
        return None;
    }

    let pitch_bytes = aligned_pitch_bytes(KERNEL_CURSOR_DIM, KERNEL_CURSOR_BYTES_PER_PIXEL)?;
    let byte_len = usize::try_from(u64::from(pitch_bytes) * u64::from(KERNEL_CURSOR_DIM)).ok()?;
    let Some((phys, virt)) = crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN) else {
        crate::log_warn!(
            target: "gfx";
            "intel/display: kernel-hw-cursor alloc failed bytes=0x{:X}\n",
            byte_len
        );
        state.init_failed = true;
        return None;
    };

    fill_surface_color(virt, pitch_bytes as usize, KERNEL_CURSOR_DIM, KERNEL_CURSOR_DIM, 0);
    crate::intel::dma_flush(virt, byte_len);

    if !crate::intel::map_ggtt(dev, phys, byte_len, KERNEL_CURSOR_GPU_ADDR) {
        crate::log_warn!(
            target: "gfx";
            "intel/display: kernel-hw-cursor ggtt map failed phys=0x{:X} bytes=0x{:X} gpu=0x{:X}\n",
            phys,
            byte_len,
            KERNEL_CURSOR_GPU_ADDR
        );
        state.init_failed = true;
        return None;
    }
    crate::intel::ggtt_invalidate(dev);

    let surface = KernelCursorSurface {
        width: KERNEL_CURSOR_DIM,
        height: KERNEL_CURSOR_DIM,
        pitch_bytes,
        phys,
        virt,
    };
    state.surface = Some(surface);
    crate::log_info!(
        target: "gfx";
        "intel/display: kernel-hw-cursor surface phys=0x{:X} gpu=0x{:X} size={}x{} pitch=0x{:X}\n",
        phys,
        KERNEL_CURSOR_GPU_ADDR,
        surface.width,
        surface.height,
        surface.pitch_bytes
    );
    Some(surface)
}

fn normalized_cursor_to_px(norm: f64, extent: u32, hotspot: i32) -> i32 {
    let limit = extent.saturating_sub(1) as f64;
    let pixel = norm.clamp(0.0, 1.0) * limit;
    (pixel + 0.5) as i32 - hotspot
}

fn cursor_pos_reg_value(x: i32, y: i32) -> u32 {
    let mut value = 0u32;
    let x_mag = x.unsigned_abs().min(0x7FFF);
    let y_mag = y.unsigned_abs().min(0x7FFF);
    if x < 0 {
        value |= CURSOR_POS_X_SIGN;
    }
    if y < 0 {
        value |= CURSOR_POS_Y_SIGN;
    }
    value |= x_mag & CURSOR_POS_X_MASK;
    value |= (y_mag << 16) & CURSOR_POS_Y_MASK;
    value
}

fn clamp_cursor_probe_position(
    x: i32,
    y: i32,
    scanout_w: u32,
    scanout_h: u32,
    rect_dim: u32,
) -> (i32, i32) {
    let inset = KERNEL_CURSOR_DIM.saturating_sub(rect_dim) / 2;
    let min_x = -(inset as i32);
    let min_y = -(inset as i32);
    let max_x = scanout_w.saturating_sub(inset.saturating_add(rect_dim)) as i32;
    let max_y = scanout_h.saturating_sub(inset.saturating_add(rect_dim)) as i32;
    (x.clamp(min_x, max_x), y.clamp(min_y, max_y))
}

fn kernel_cursor_visible_rect_dim(scanout_h: u32) -> u32 {
    let clamped_h =
        scanout_h.clamp(KERNEL_CURSOR_RECT_MIN_SCANOUT_H, KERNEL_CURSOR_RECT_MAX_SCANOUT_H);
    let range_h = KERNEL_CURSOR_RECT_MAX_SCANOUT_H - KERNEL_CURSOR_RECT_MIN_SCANOUT_H;
    let range_dim = KERNEL_CURSOR_RECT_MAX_DIM - KERNEL_CURSOR_RECT_MIN_DIM;
    let scaled = if range_h == 0 {
        KERNEL_CURSOR_RECT_MAX_DIM
    } else {
        KERNEL_CURSOR_RECT_MIN_DIM
            + ((clamped_h - KERNEL_CURSOR_RECT_MIN_SCANOUT_H) * range_dim + (range_h / 2)) / range_h
    };
    scaled.clamp(KERNEL_CURSOR_RECT_MIN_DIM, KERNEL_CURSOR_RECT_MAX_DIM)
}

fn pack_bgra8888(r: u8, g: u8, b: u8, a: u8) -> u32 {
    let alpha = u16::from(a);
    let premul_r = ((u16::from(r) * alpha) / 0xFF) as u8;
    let premul_g = ((u16::from(g) * alpha) / 0xFF) as u8;
    let premul_b = ((u16::from(b) * alpha) / 0xFF) as u8;
    u32::from_le_bytes([premul_b, premul_g, premul_r, a])
}

fn draw_kernel_cursor_surface(surface: KernelCursorSurface, rect_dim: u32, pressed: bool) {
    fill_surface_color(
        surface.virt,
        surface.pitch_bytes as usize,
        surface.width,
        surface.height,
        0,
    );

    let width = surface.width as usize;
    let height = surface.height as usize;
    let pitch_pixels = (surface.pitch_bytes as usize) / 4;
    let rect_dim = rect_dim.min(surface.width).min(surface.height) as usize;
    let rect_x0 = (width.saturating_sub(rect_dim)) / 2;
    let rect_y0 = (height.saturating_sub(rect_dim)) / 2;
    let rect_x1 = rect_x0.saturating_add(rect_dim);
    let rect_y1 = rect_y0.saturating_add(rect_dim);
    let split_x = rect_x0.saturating_add(rect_dim / 2);
    let left_pixel = if pressed {
        pack_bgra8888(0x00, 0x00, 0x00, KERNEL_CURSOR_SPLIT_ALPHA)
    } else {
        pack_bgra8888(0xFF, 0xFF, 0xFF, KERNEL_CURSOR_SPLIT_ALPHA)
    };
    let right_pixel = if pressed {
        pack_bgra8888(0xFF, 0xFF, 0xFF, KERNEL_CURSOR_SPLIT_ALPHA)
    } else {
        pack_bgra8888(0x00, 0x00, 0x00, KERNEL_CURSOR_SPLIT_ALPHA)
    };

    for y in rect_y0..rect_y1 {
        let row = unsafe { (surface.virt as *mut u32).add(y.saturating_mul(pitch_pixels)) };
        for x in rect_x0..split_x {
            unsafe {
                core::ptr::write_volatile(row.add(x), left_pixel);
            }
        }
        for x in split_x..rect_x1 {
            unsafe {
                core::ptr::write_volatile(row.add(x), right_pixel);
            }
        }
    }
}
