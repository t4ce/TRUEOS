use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

const PIPE_A_SRC: usize = 0x7001C;
const PIPE_B_SRC: usize = 0x7101C;
const PIPE_C_SRC: usize = 0x7201C;
const PIPE_D_SRC: usize = 0x7301C;
const SKL_BOTTOM_COLOR_A: usize = 0x70034;
const SKL_BOTTOM_COLOR_PIPE_STRIDE: usize = 0x1000;
const UNI_PLANE_BASE: usize = 0x70180;
const UNI_PLANE_PIPE_STRIDE: usize = 0x1000;
const UNI_PLANE_SLOT_STRIDE: usize = 0x100;
const UNI_PLANE_CTL_OFF: usize = 0x00;
const UNI_PLANE_STRIDE_OFF: usize = 0x08;
const UNI_PLANE_POS_OFF: usize = 0x0C;
const UNI_PLANE_SIZE_OFF: usize = 0x10;
const UNI_PLANE_KEYVAL_OFF: usize = 0x14;
const UNI_PLANE_KEYMSK_OFF: usize = 0x18;
const UNI_PLANE_SURF_OFF: usize = 0x1C;
const UNI_PLANE_KEYMAX_OFF: usize = 0x20;
const UNI_PLANE_OFFSET_OFF: usize = 0x24;
const UNI_PLANE_SURFLIVE_OFF: usize = 0x2C;
const UNI_PLANE_AUX_DIST_OFF: usize = 0x40;
const UNI_PLANE_AUX_OFFSET_OFF: usize = 0x44;
const UNI_PLANE_COLOR_CTL_OFF: usize = 0x4C;
const UNI_PLANE_BUF_CFG_OFF: usize = 0xFC;
const PLANE_CTL_ENABLE: u32 = 1 << 31;
const PLANE_CTL_FORMAT_MASK_SKL: u32 = 0x0F << 24;
const PLANE_CTL_ORDER_RGBX: u32 = 1 << 20;
const PLANE_CTL_TILED_MASK: u32 = 0x07 << 10;
const PLANE_CTL_ALPHA_MASK: u32 = 0x03 << 4;
const PLANE_CTL_ROTATE_MASK: u32 = 0x03;
const PLANE_CTL_FORMAT_XRGB_8888: u32 = 4 << 24;
const PLANE_CTL_TILED_LINEAR: u32 = 0 << 10;
const PLANE_COLOR_ALPHA_MASK: u32 = 0x03 << 4;
const PLANE_COLOR_ALPHA_SW_PREMULTIPLY: u32 = 2 << 4;
const PLANE_COLOR_ALPHA_HW_PREMULTIPLY: u32 = 3 << 4;
const PLANE_COLOR_PLANE_GAMMA_DISABLE: u32 = 1 << 13;
const CURSOR_GLYPH_DIM_PX: u32 = 32;
const CURSOR_GLYPH_RADIUS_PX: i32 = 14;
const GPU_VA_DISPLAY_CURSOR_BASE: u64 = 0x0240_0000;
const CURSOR_OVERLAY_PLANE_INDEX: usize = 3;
const PIPE_BOTTOM_COLOR_RGB: u32 = 0x00FF_37FF;

static PRIMARY_GRADIENT_INIT: AtomicBool = AtomicBool::new(false);
static PRIMARY_SURFACE: Mutex<Option<PrimarySurface>> = Mutex::new(None);
static CURSOR_OVERLAY: Mutex<Option<CursorOverlaySurface>> = Mutex::new(None);
static CURSOR_OVERLAY_PROBE_STATE: Mutex<Option<CursorOverlayProbeState>> = Mutex::new(None);

#[derive(Copy, Clone)]
struct PipeInfo {
    name: &'static str,
    slot: usize,
    pipe_src_off: usize,
    plane_ctl_off: usize,
    plane_stride_off: usize,
    plane_surf_off: usize,
    plane_surf_live_off: usize,
}

#[derive(Copy, Clone)]
struct PrimarySurface {
    width: u32,
    height: u32,
    pipe: PipeInfo,
}

unsafe impl Send for PrimarySurface {}
unsafe impl Sync for PrimarySurface {}

#[derive(Copy, Clone)]
struct CursorOverlaySurface {
    phys: u64,
    virt: *mut u8,
    len: usize,
    pitch_bytes: u32,
    gpu_addr: u64,
    width: u32,
    height: u32,
}

unsafe impl Send for CursorOverlaySurface {}
unsafe impl Sync for CursorOverlaySurface {}

#[derive(Copy, Clone, Eq, PartialEq)]
struct CursorOverlayProbeState {
    stride_reg: u32,
    width: u32,
    height: u32,
    bbox_min_x: u32,
    bbox_min_y: u32,
    bbox_max_x: u32,
    bbox_max_y: u32,
    nonzero_pixels: u32,
    alpha_pixels: u32,
    bg_sample: u32,
    center_sample: u32,
    right_edge_sample: u32,
}

const PIPES: [PipeInfo; 4] = [
    PipeInfo {
        name: "pipe-a",
        slot: 0,
        pipe_src_off: PIPE_A_SRC,
        plane_ctl_off: UNI_PLANE_BASE + 0 * UNI_PLANE_PIPE_STRIDE + 0 * UNI_PLANE_SLOT_STRIDE + UNI_PLANE_CTL_OFF,
        plane_stride_off: UNI_PLANE_BASE
            + 0 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_STRIDE_OFF,
        plane_surf_off: UNI_PLANE_BASE + 0 * UNI_PLANE_PIPE_STRIDE + 0 * UNI_PLANE_SLOT_STRIDE + UNI_PLANE_SURF_OFF,
        plane_surf_live_off: UNI_PLANE_BASE
            + 0 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_SURFLIVE_OFF,
    },
    PipeInfo {
        name: "pipe-b",
        slot: 1,
        pipe_src_off: PIPE_B_SRC,
        plane_ctl_off: UNI_PLANE_BASE + 1 * UNI_PLANE_PIPE_STRIDE + 0 * UNI_PLANE_SLOT_STRIDE + UNI_PLANE_CTL_OFF,
        plane_stride_off: UNI_PLANE_BASE
            + 1 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_STRIDE_OFF,
        plane_surf_off: UNI_PLANE_BASE + 1 * UNI_PLANE_PIPE_STRIDE + 0 * UNI_PLANE_SLOT_STRIDE + UNI_PLANE_SURF_OFF,
        plane_surf_live_off: UNI_PLANE_BASE
            + 1 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_SURFLIVE_OFF,
    },
    PipeInfo {
        name: "pipe-c",
        slot: 2,
        pipe_src_off: PIPE_C_SRC,
        plane_ctl_off: UNI_PLANE_BASE + 2 * UNI_PLANE_PIPE_STRIDE + 0 * UNI_PLANE_SLOT_STRIDE + UNI_PLANE_CTL_OFF,
        plane_stride_off: UNI_PLANE_BASE
            + 2 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_STRIDE_OFF,
        plane_surf_off: UNI_PLANE_BASE + 2 * UNI_PLANE_PIPE_STRIDE + 0 * UNI_PLANE_SLOT_STRIDE + UNI_PLANE_SURF_OFF,
        plane_surf_live_off: UNI_PLANE_BASE
            + 2 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_SURFLIVE_OFF,
    },
    PipeInfo {
        name: "pipe-d",
        slot: 3,
        pipe_src_off: PIPE_D_SRC,
        plane_ctl_off: UNI_PLANE_BASE + 3 * UNI_PLANE_PIPE_STRIDE + 0 * UNI_PLANE_SLOT_STRIDE + UNI_PLANE_CTL_OFF,
        plane_stride_off: UNI_PLANE_BASE
            + 3 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_STRIDE_OFF,
        plane_surf_off: UNI_PLANE_BASE + 3 * UNI_PLANE_PIPE_STRIDE + 0 * UNI_PLANE_SLOT_STRIDE + UNI_PLANE_SURF_OFF,
        plane_surf_live_off: UNI_PLANE_BASE
            + 3 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_SURFLIVE_OFF,
    },
];

pub(crate) fn init_primary_gradient(dev: crate::intel::Dev) {
    if PRIMARY_GRADIENT_INIT.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(pipe) = active_pipe(dev) else {
        crate::log!("intel/display: primary-gradient skipped no active pipe discovered\n");
        return;
    };
    let pipe_src_raw = crate::intel::mmio_read(dev, pipe.pipe_src_off);
    let pipe_src_dims = decode_pipe_src(pipe_src_raw);
    let fb_dims = framebuffer_hint();
    let chosen = pipe_src_dims
        .map(|(width, height)| (width, height, "pipe-src"))
        .or_else(|| fb_dims.map(|(width, height)| (width, height, "fb-hint")));
    let Some((width, height, chosen_from)) = chosen else {
        crate::log!(
            "intel/display: primary-gradient skipped no dimensions pipe={}\n",
            pipe.name
        );
        return;
    };
    log_primary_dimensions_probe(pipe.name, pipe_src_raw, pipe_src_dims, fb_dims, chosen_from);
    program_pipe_bottom_color(dev, pipe, PIPE_BOTTOM_COLOR_RGB);

    let Some(pitch_bytes) = aligned_pitch_bytes(width) else {
        crate::log!(
            "intel/display: primary-gradient skipped bad pitch width={}\n",
            width
        );
        return;
    };
    let Some(byte_len) = usize::try_from(u64::from(pitch_bytes) * u64::from(height)).ok() else {
        crate::log!("intel/display: primary-gradient skipped surface too large\n");
        return;
    };
    let Some((phys, virt)) = crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN) else {
        crate::log!(
            "intel/display: primary-gradient alloc failed bytes=0x{:X}\n",
            byte_len
        );
        return;
    };

    fill_vertical_gradient(virt, pitch_bytes as usize, width, height);
    crate::intel::dma_flush(virt, byte_len);

    if !crate::intel::map_ggtt(
        dev,
        phys,
        byte_len,
        crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE,
    ) {
        crate::log!(
            "intel/display: primary-gradient ggtt map failed bytes=0x{:X} gpu=0x{:X}\n",
            byte_len,
            crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE
        );
        return;
    }
    crate::intel::ggtt_invalidate(dev);

    let Some(stride_reg) = plane_stride_reg_value(pitch_bytes) else {
        crate::log!(
            "intel/display: primary-gradient stride encode failed pitch=0x{:X}\n",
            pitch_bytes
        );
        return;
    };
    let Some(surface_reg) = u32::try_from(crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE).ok() else {
        crate::log!("intel/display: primary-gradient gpu addr out of range\n");
        return;
    };

    let ctl_before = crate::intel::mmio_read(dev, pipe.plane_ctl_off);
    let surf_before = crate::intel::mmio_read(dev, pipe.plane_surf_off);
    crate::intel::mmio_write(dev, pipe.plane_stride_off, stride_reg);
    crate::intel::mmio_write(dev, pipe.plane_surf_off, surface_reg);

    let mut surf_live = crate::intel::mmio_read(dev, pipe.plane_surf_live_off);
    let mut iter = 0usize;
    while iter < 4096 && surf_live != surface_reg {
        core::hint::spin_loop();
        surf_live = crate::intel::mmio_read(dev, pipe.plane_surf_live_off);
        iter += 1;
    }
    let surf_armed = crate::intel::mmio_read(dev, pipe.plane_surf_off);
    let ctl_after = crate::intel::mmio_read(dev, pipe.plane_ctl_off);
    let ok = surf_live == surface_reg || surf_armed == surface_reg;

    *PRIMARY_SURFACE.lock() = Some(PrimarySurface {
        width,
        height,
        pipe,
    });
    log_primary_plane_probe(dev, pipe, "primary-live");
    log_overlay_plane_probe(dev, pipe, CURSOR_OVERLAY_PLANE_INDEX, "boot-baseline");

    crate::log!(
        "intel/display: primary-gradient pipe={} size={}x{} pitch=0x{:X} gpu=0x{:X} phys=0x{:X} plane_enabled={} ctl_before=0x{:08X} ctl_after=0x{:08X} surf_before=0x{:08X} surf=0x{:08X} surf_live=0x{:08X} ok={}\n",
        pipe.name,
        width,
        height,
        pitch_bytes,
        crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE,
        phys,
        ((ctl_after & PLANE_CTL_ENABLE) != 0) as u8,
        ctl_before,
        ctl_after,
        surf_before,
        surf_armed,
        surf_live,
        ok as u8
    );
}

fn program_pipe_bottom_color(dev: crate::intel::Dev, pipe: PipeInfo, rgb: u32) {
    let reg = SKL_BOTTOM_COLOR_A + pipe.slot * SKL_BOTTOM_COLOR_PIPE_STRIDE;
    crate::intel::mmio_write(dev, reg, rgb);
    let readback = crate::intel::mmio_read(dev, reg);
    crate::log!(
        "intel/display: bottom-color pipe={} reg=0x{:05X} rgb=0x{:06X} readback=0x{:08X}\n",
        pipe.name,
        reg,
        rgb & 0x00FF_FFFF,
        readback
    );
}

#[allow(dead_code)]
pub(crate) fn active_scanout_dimensions() -> Option<(u32, u32)> {
    let dev = crate::intel::claimed_device()?;
    let pipe = active_pipe(dev)?;
    decode_pipe_src(crate::intel::mmio_read(dev, pipe.pipe_src_off)).or_else(framebuffer_hint)
}

#[allow(dead_code)]
pub(crate) fn primary_surface_gpu_addr() -> Option<u64> {
    PRIMARY_SURFACE
        .lock()
        .as_ref()
        .map(|_| crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE)
}

pub(crate) fn update_cursor_overlay(entries: &[(u32, u32, u32, u32)]) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(primary) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    let Some(surface) = ensure_cursor_overlay_surface(dev, primary) else {
        return false;
    };
    fill_cursor_overlay_surface(surface, entries);
    crate::intel::dma_flush(surface.virt, surface.len);

    let plane = overlay_plane_registers(primary.pipe, CURSOR_OVERLAY_PLANE_INDEX);
    let stride_reg = match plane_stride_reg_value(surface.pitch_bytes) {
        Some(v) => v,
        None => return false,
    };
    let pos_reg = plane_pos_reg_value(0, 0);
    let size_reg = plane_size_reg_value(surface.width, surface.height);
    let ctl_reg = PLANE_CTL_ENABLE | PLANE_CTL_FORMAT_XRGB_8888 | PLANE_CTL_TILED_LINEAR;
    let color_ctl_reg = PLANE_COLOR_PLANE_GAMMA_DISABLE | PLANE_COLOR_ALPHA_SW_PREMULTIPLY;
    let Some(surf_reg) = u32::try_from(surface.gpu_addr).ok() else {
        return false;
    };
    log_cursor_overlay_probe(surface, stride_reg, entries);

    crate::log!(
        "intel/display: overlay-arm pipe={} plane={} hw_plane={} ctl=0x{:08X} format={} tiled={} color_ctl=0x{:08X} color_alpha={}\n",
        primary.pipe.name,
        CURSOR_OVERLAY_PLANE_INDEX,
        overlay_hw_plane_slot(CURSOR_OVERLAY_PLANE_INDEX),
        ctl_reg,
        decode_plane_format(ctl_reg),
        decode_plane_tiling(ctl_reg),
        color_ctl_reg,
        decode_plane_color_alpha(color_ctl_reg)
    );
    crate::intel::mmio_write(dev, plane.keyval_off, 0);
    crate::intel::mmio_write(dev, plane.keymsk_off, 0);
    crate::intel::mmio_write(dev, plane.keymax_off, 0);
    crate::intel::mmio_write(dev, plane.offset_off, 0);
    crate::intel::mmio_write(dev, plane.pos_off, pos_reg);
    crate::intel::mmio_write(dev, plane.size_off, size_reg);
    crate::intel::mmio_write(dev, plane.stride_off, stride_reg);
    crate::intel::mmio_write(dev, plane.color_ctl_off, color_ctl_reg);
    crate::intel::mmio_write(dev, plane.ctl_off, ctl_reg);
    crate::intel::mmio_write(dev, plane.surf_off, surf_reg);
    log_overlay_plane_probe(
        dev,
        primary.pipe,
        CURSOR_OVERLAY_PLANE_INDEX,
        "cursor-overlay-armed",
    );
    true
}

pub(crate) fn disable_cursor_overlay() -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(primary) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    let plane = overlay_plane_registers(primary.pipe, CURSOR_OVERLAY_PLANE_INDEX);
    crate::intel::mmio_write(dev, plane.color_ctl_off, 0);
    crate::intel::mmio_write(dev, plane.ctl_off, 0);
    crate::intel::mmio_write(dev, plane.surf_off, 0);
    log_overlay_plane_probe(
        dev,
        primary.pipe,
        CURSOR_OVERLAY_PLANE_INDEX,
        "cursor-overlay-disabled",
    );
    true
}

fn active_pipe(dev: crate::intel::Dev) -> Option<PipeInfo> {
    let mut observed = None;
    for pipe in PIPES {
        let pipe_src = crate::intel::mmio_read(dev, pipe.pipe_src_off);
        if decode_pipe_src(pipe_src).is_some() {
            return Some(pipe);
        }
        let plane_ctl = crate::intel::mmio_read(dev, pipe.plane_ctl_off);
        let plane_surf = crate::intel::mmio_read(dev, pipe.plane_surf_off);
        let plane_surf_live = crate::intel::mmio_read(dev, pipe.plane_surf_live_off);
        if observed.is_none() && (plane_ctl != 0 || plane_surf != 0 || plane_surf_live != 0) {
            observed = Some(pipe);
        }
    }
    observed
}

#[derive(Copy, Clone)]
struct OverlayPlaneRegs {
    ctl_off: usize,
    stride_off: usize,
    pos_off: usize,
    size_off: usize,
    keyval_off: usize,
    keymsk_off: usize,
    surf_off: usize,
    keymax_off: usize,
    offset_off: usize,
    aux_dist_off: usize,
    aux_offset_off: usize,
    color_ctl_off: usize,
    buf_cfg_off: usize,
}

fn overlay_plane_registers(pipe: PipeInfo, overlay_index: usize) -> OverlayPlaneRegs {
    let hw_plane_slot = overlay_hw_plane_slot(overlay_index);
    let base =
        UNI_PLANE_BASE + pipe.slot * UNI_PLANE_PIPE_STRIDE + hw_plane_slot * UNI_PLANE_SLOT_STRIDE;
    OverlayPlaneRegs {
        ctl_off: base + UNI_PLANE_CTL_OFF,
        stride_off: base + UNI_PLANE_STRIDE_OFF,
        pos_off: base + UNI_PLANE_POS_OFF,
        size_off: base + UNI_PLANE_SIZE_OFF,
        keyval_off: base + UNI_PLANE_KEYVAL_OFF,
        keymsk_off: base + UNI_PLANE_KEYMSK_OFF,
        surf_off: base + UNI_PLANE_SURF_OFF,
        keymax_off: base + UNI_PLANE_KEYMAX_OFF,
        offset_off: base + UNI_PLANE_OFFSET_OFF,
        aux_dist_off: base + UNI_PLANE_AUX_DIST_OFF,
        aux_offset_off: base + UNI_PLANE_AUX_OFFSET_OFF,
        color_ctl_off: base + UNI_PLANE_COLOR_CTL_OFF,
        buf_cfg_off: base + UNI_PLANE_BUF_CFG_OFF,
    }
}

#[inline]
fn overlay_hw_plane_slot(overlay_index: usize) -> usize {
    overlay_index + 1
}

fn log_primary_plane_probe(dev: crate::intel::Dev, pipe: PipeInfo, label: &str) {
    let ctl = crate::intel::mmio_read(dev, pipe.plane_ctl_off);
    let stride = crate::intel::mmio_read(dev, pipe.plane_stride_off);
    let surf = crate::intel::mmio_read(dev, pipe.plane_surf_off);
    let surf_live = crate::intel::mmio_read(dev, pipe.plane_surf_live_off);
    let color_ctl = crate::intel::mmio_read(dev, pipe.plane_ctl_off + UNI_PLANE_COLOR_CTL_OFF);
    let buf_cfg = crate::intel::mmio_read(dev, pipe.plane_ctl_off + UNI_PLANE_BUF_CFG_OFF);

    crate::log!(
        "intel/display: primary-probe label={} pipe={} ctl=0x{:08X} enabled={} format={} tiled={} rot={} rgbx={} stride=0x{:08X} surf=0x{:08X} surf_live=0x{:08X} color_ctl=0x{:08X} color_alpha={} buf_cfg=0x{:08X}\n",
        label,
        pipe.name,
        ctl,
        ((ctl & PLANE_CTL_ENABLE) != 0) as u8,
        decode_plane_format(ctl),
        decode_plane_tiling(ctl),
        decode_plane_rotation(ctl),
        ((ctl & PLANE_CTL_ORDER_RGBX) != 0) as u8,
        stride,
        surf,
        surf_live,
        color_ctl,
        decode_plane_color_alpha(color_ctl),
        buf_cfg
    );
}

fn log_overlay_plane_probe(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    overlay_index: usize,
    label: &str,
) {
    let plane = overlay_plane_registers(pipe, overlay_index);
    let ctl = crate::intel::mmio_read(dev, plane.ctl_off);
    let stride = crate::intel::mmio_read(dev, plane.stride_off);
    let pos = crate::intel::mmio_read(dev, plane.pos_off);
    let size = crate::intel::mmio_read(dev, plane.size_off);
    let keyval = crate::intel::mmio_read(dev, plane.keyval_off);
    let keymsk = crate::intel::mmio_read(dev, plane.keymsk_off);
    let offset = crate::intel::mmio_read(dev, plane.offset_off);
    let surf = crate::intel::mmio_read(dev, plane.surf_off);
    let surf_live = crate::intel::mmio_read(dev, plane.surf_off + (UNI_PLANE_SURFLIVE_OFF - UNI_PLANE_SURF_OFF));
    let keymax = crate::intel::mmio_read(dev, plane.keymax_off);
    let aux_dist = crate::intel::mmio_read(dev, plane.aux_dist_off);
    let aux_offset = crate::intel::mmio_read(dev, plane.aux_offset_off);
    let color_ctl = crate::intel::mmio_read(dev, plane.color_ctl_off);
    let buf_cfg = crate::intel::mmio_read(dev, plane.buf_cfg_off);

    crate::log!(
        "intel/display: overlay-probe label={} pipe={} plane={} hw_plane={} ctl=0x{:08X} enabled={} format={} tiled={} rot={} rgbx={} stride=0x{:08X} pos={}x{} size={}x{} offset={}x{} surf=0x{:08X} surf_live=0x{:08X} color_ctl=0x{:08X} color_alpha={} key=0x{:08X}/0x{:08X}/0x{:08X} aux=0x{:08X}/0x{:08X} buf_cfg=0x{:08X}\n",
        label,
        pipe.name,
        overlay_index,
        overlay_hw_plane_slot(overlay_index),
        ctl,
        ((ctl & PLANE_CTL_ENABLE) != 0) as u8,
        decode_plane_format(ctl),
        decode_plane_tiling(ctl),
        decode_plane_rotation(ctl),
        ((ctl & PLANE_CTL_ORDER_RGBX) != 0) as u8,
        stride,
        decode_xy_x(pos),
        decode_xy_y(pos),
        decode_xy_x(size).saturating_add(1),
        decode_xy_y(size).saturating_add(1),
        decode_xy_x(offset),
        decode_xy_y(offset),
        surf,
        surf_live,
        color_ctl,
        decode_plane_color_alpha(color_ctl),
        keyval,
        keymsk,
        keymax,
        aux_dist,
        aux_offset,
        buf_cfg
    );
}

fn decode_plane_format(ctl: u32) -> &'static str {
    match ctl & PLANE_CTL_FORMAT_MASK_SKL {
        0x0000_0000 => "YUV422",
        0x0100_0000 => "NV12",
        0x0200_0000 => "XRGB2101010",
        0x0300_0000 => "P010",
        0x0400_0000 => "XRGB8888/ARGB8888",
        0x0500_0000 => "P012",
        0x0600_0000 => "XRGB16161616F",
        0x0700_0000 => "P016",
        0x0800_0000 => "XYUV",
        0x0C00_0000 => "INDEXED",
        0x0E00_0000 => "RGB565",
        _ => "unknown",
    }
}

fn decode_plane_alpha(ctl: u32) -> &'static str {
    match ctl & PLANE_CTL_ALPHA_MASK {
        0x00 => "disable",
        0x20 => "sw-premul",
        0x30 => "hw-premul",
        _ => "unknown",
    }
}

fn decode_plane_color_alpha(color_ctl: u32) -> &'static str {
    match color_ctl & PLANE_COLOR_ALPHA_MASK {
        0x00 => "disable",
        0x20 => "sw-premul",
        0x30 => "hw-premul",
        _ => "unknown",
    }
}

fn decode_plane_tiling(ctl: u32) -> &'static str {
    match ctl & PLANE_CTL_TILED_MASK {
        0x0000 => "linear",
        0x0400 => "x",
        0x1000 => "y",
        0x1400 => "yf/4",
        _ => "unknown",
    }
}

fn decode_plane_rotation(ctl: u32) -> &'static str {
    match ctl & PLANE_CTL_ROTATE_MASK {
        0 => "0",
        1 => "90",
        2 => "180",
        3 => "270",
        _ => "unknown",
    }
}

#[inline]
fn decode_xy_x(v: u32) -> u32 {
    v & 0xFFFF
}

#[inline]
fn decode_xy_y(v: u32) -> u32 {
    (v >> 16) & 0xFFFF
}

fn ensure_cursor_overlay_surface(
    dev: crate::intel::Dev,
    primary: PrimarySurface,
) -> Option<CursorOverlaySurface> {
    let mut entry = CURSOR_OVERLAY.lock();
    if let Some(surface) = *entry {
        if surface.width == primary.width && surface.height == primary.height {
            return Some(surface);
        }
    }

    let pitch_bytes = aligned_pitch_bytes(primary.width)?;
    let byte_len = usize::try_from(u64::from(pitch_bytes) * u64::from(primary.height)).ok()?;
    let (phys, virt) = crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN)?;
    let gpu_addr = GPU_VA_DISPLAY_CURSOR_BASE;
    unsafe {
        core::ptr::write_bytes(virt, 0, byte_len);
    }

    if !crate::intel::map_ggtt(dev, phys, byte_len, gpu_addr) {
        return None;
    }
    crate::intel::ggtt_invalidate(dev);

    let surface = CursorOverlaySurface {
        phys,
        virt,
        len: byte_len,
        pitch_bytes,
        gpu_addr,
        width: primary.width,
        height: primary.height,
    };
    crate::log!(
        "intel/display: cursor-overlay size={}x{} pitch=0x{:X} gpu=0x{:X} phys=0x{:X}\n",
        primary.width,
        primary.height,
        pitch_bytes,
        gpu_addr,
        phys
    );
    *entry = Some(surface);
    Some(surface)
}

fn decode_pipe_src(value: u32) -> Option<(u32, u32)> {
    if value == 0 || value == u32::MAX {
        return None;
    }
    let width = (value & 0xFFFF).saturating_add(1);
    let height = ((value >> 16) & 0xFFFF).saturating_add(1);
    if !(320..=8192).contains(&width) || !(200..=4320).contains(&height) {
        return None;
    }
    Some((width, height))
}

fn framebuffer_hint() -> Option<(u32, u32)> {
    let fb = crate::limine::framebuffer_response()?.framebuffers().next()?;
    Some((fb.width() as u32, fb.height() as u32))
}

fn log_primary_dimensions_probe(
    pipe_name: &str,
    pipe_src_raw: u32,
    pipe_src_dims: Option<(u32, u32)>,
    fb_dims: Option<(u32, u32)>,
    chosen_from: &str,
) {
    let (pipe_w, pipe_h) = pipe_src_dims.unwrap_or((0, 0));
    let (fb_w, fb_h) = fb_dims.unwrap_or((0, 0));
    crate::log!(
        "intel/display: primary-dims pipe={} pipe_src=0x{:08X} pipe_decoded={}x{} fb_hint={}x{} chosen={}\n",
        pipe_name,
        pipe_src_raw,
        pipe_w,
        pipe_h,
        fb_w,
        fb_h,
        chosen_from
    );
}

fn aligned_pitch_bytes(width: u32) -> Option<u32> {
    let bytes = width.checked_mul(4)?;
    let aligned = crate::intel::align_up(bytes as usize, 64)?;
    u32::try_from(aligned).ok()
}

fn plane_pos_reg_value(x: u32, y: u32) -> u32 {
    ((y & 0xFFFF) << 16) | (x & 0xFFFF)
}

fn plane_size_reg_value(width: u32, height: u32) -> u32 {
    let enc_w = width.saturating_sub(1) & 0xFFFF;
    let enc_h = height.saturating_sub(1) & 0xFFFF;
    (enc_h << 16) | enc_w
}

fn plane_stride_reg_value(pitch_bytes: u32) -> Option<u32> {
    if pitch_bytes == 0 || !pitch_bytes.is_multiple_of(64) {
        None
    } else {
        Some(pitch_bytes / 64)
    }
}

fn fill_vertical_gradient(ptr: *mut u8, pitch_bytes: usize, width: u32, height: u32) {
    unsafe {
        core::ptr::write_bytes(ptr, 0, pitch_bytes.saturating_mul(height as usize));
    }
    for y in 0..height as usize {
        let shade = if height <= 1 {
            255u8
        } else {
            let top = (height as usize - 1).saturating_sub(y);
            ((top.saturating_mul(255)) / (height as usize - 1)) as u8
        };
        for x in 0..width as usize {
            let off = y
                .saturating_mul(pitch_bytes)
                .saturating_add(x.saturating_mul(4));
            unsafe {
                let dst = ptr.add(off) as *mut u32;
                core::ptr::write_volatile(
                    dst,
                    0xFF00_0000u32 | ((shade as u32) << 16) | ((shade as u32) << 8) | shade as u32,
                );
            }
        }
    }
}

fn fill_cursor_surface(
    _ptr: *mut u8,
    _pitch_bytes: usize,
    _width: u32,
    _height: u32,
    _color: (u8, u8, u8, u8),
    _buttons_down: u32,
) {
}

fn fill_cursor_overlay_surface(surface: CursorOverlaySurface, entries: &[(u32, u32, u32, u32)]) {
    unsafe {
        core::ptr::write_bytes(surface.virt, 0, surface.len);
    }

    for &(slot_id, x_px, y_px, buttons_down) in entries {
        paint_cursor_glyph(
            surface.virt,
            surface.pitch_bytes as usize,
            surface.width,
            surface.height,
            x_px as i32,
            y_px as i32,
            crate::r::ui2::cursor_color(slot_id),
            buttons_down,
        );
    }
}

fn log_cursor_overlay_probe(
    surface: CursorOverlaySurface,
    stride_reg: u32,
    entries: &[(u32, u32, u32, u32)],
) {
    let summary = summarize_cursor_overlay(surface, stride_reg, entries);
    let mut guard = CURSOR_OVERLAY_PROBE_STATE.lock();
    if guard.as_ref().copied() == Some(summary) {
        return;
    }
    *guard = Some(summary);

    let bytes_per_row_from_reg = stride_reg.saturating_mul(64);
    let last_row_end = (surface.height.saturating_sub(1))
        .saturating_mul(surface.pitch_bytes)
        .saturating_add(surface.width.saturating_mul(4));
    crate::log!(
        "intel/display: overlay-geom width={} height={} pitch_bytes=0x{:X} stride_reg=0x{:X} stride_bytes=0x{:X} last_row_end=0x{:X} surface_len=0x{:X} fits={}\n",
        surface.width,
        surface.height,
        surface.pitch_bytes,
        stride_reg,
        bytes_per_row_from_reg,
        last_row_end,
        surface.len,
        (usize::try_from(last_row_end).ok().is_some_and(|end| end <= surface.len)) as u8
    );
    crate::log!(
        "intel/display: overlay-coverage entries={} nonzero={} alpha_pixels={} bbox={}x{}..{}x{}\n",
        entries.len(),
        summary.nonzero_pixels,
        summary.alpha_pixels,
        summary.bbox_min_x,
        summary.bbox_min_y,
        summary.bbox_max_x,
        summary.bbox_max_y
    );
    crate::log!(
        "intel/display: overlay-pixels bg=0x{:08X} center=0x{:08X} right=0x{:08X}\n",
        summary.bg_sample,
        summary.center_sample,
        summary.right_edge_sample
    );
}

fn summarize_cursor_overlay(
    surface: CursorOverlaySurface,
    stride_reg: u32,
    entries: &[(u32, u32, u32, u32)],
) -> CursorOverlayProbeState {
    let mut nonzero_pixels = 0u32;
    let mut alpha_pixels = 0u32;
    let mut bbox_min_x = surface.width;
    let mut bbox_min_y = surface.height;
    let mut bbox_max_x = 0u32;
    let mut bbox_max_y = 0u32;

    for y in 0..surface.height {
        for x in 0..surface.width {
            let px = read_overlay_pixel(surface, x, y);
            if px == 0 {
                continue;
            }
            nonzero_pixels = nonzero_pixels.saturating_add(1);
            if (px >> 24) != 0 {
                alpha_pixels = alpha_pixels.saturating_add(1);
            }
            bbox_min_x = bbox_min_x.min(x);
            bbox_min_y = bbox_min_y.min(y);
            bbox_max_x = bbox_max_x.max(x);
            bbox_max_y = bbox_max_y.max(y);
        }
    }

    if nonzero_pixels == 0 {
        bbox_min_x = 0;
        bbox_min_y = 0;
    }

    let (_, sample_x, sample_y, _) = entries.first().copied().unwrap_or((0, 0, 0, 0));
    let center_sample = read_overlay_pixel(
        surface,
        sample_x.min(surface.width.saturating_sub(1)),
        sample_y.min(surface.height.saturating_sub(1)),
    );
    let right_edge_sample = read_overlay_pixel(
        surface,
        surface.width.saturating_sub(1),
        sample_y.min(surface.height.saturating_sub(1)),
    );

    CursorOverlayProbeState {
        stride_reg,
        width: surface.width,
        height: surface.height,
        bbox_min_x,
        bbox_min_y,
        bbox_max_x,
        bbox_max_y,
        nonzero_pixels,
        alpha_pixels,
        bg_sample: read_overlay_pixel(surface, 0, 0),
        center_sample,
        right_edge_sample,
    }
}

#[inline]
fn read_overlay_pixel(surface: CursorOverlaySurface, x: u32, y: u32) -> u32 {
    if x >= surface.width || y >= surface.height {
        return 0;
    }
    let off = (y as usize)
        .saturating_mul(surface.pitch_bytes as usize)
        .saturating_add((x as usize).saturating_mul(4));
    if off + 4 > surface.len {
        return 0;
    }
    unsafe { core::ptr::read_volatile(surface.virt.add(off) as *const u32) }
}

fn paint_cursor_glyph(
    ptr: *mut u8,
    pitch_bytes: usize,
    surf_w: u32,
    surf_h: u32,
    x_px: i32,
    y_px: i32,
    color: (u8, u8, u8, u8),
    buttons_down: u32,
) {
    let cx = x_px;
    let cy = y_px;
    let outer_r = CURSOR_GLYPH_RADIUS_PX;
    let inner_r = outer_r - 18;
    let core_r = 18 + ((buttons_down.count_ones() as i32).min(6) * 4);
    let (r, g, b, _) = color;

    let min_y = (cy - outer_r - 4).max(0);
    let max_y = (cy + outer_r + 4).min(surf_h as i32 - 1);
    let min_x = (cx - outer_r - 4).max(0);
    let max_x = (cx + outer_r + 4).min(surf_w as i32 - 1);

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x - cx;
            let dy = y - cy;
            let d2 = dx * dx + dy * dy;
            let outer2 = outer_r * outer_r;
            let inner2 = inner_r * inner_r;
            let core2 = core_r * core_r;
            let cross = dx.abs() <= 3 || dy.abs() <= 3;

            let rgba = if d2 <= core2 {
                argb(0xE8, r, g, b)
            } else if d2 <= outer2 && d2 >= inner2 {
                argb(0xFF, r, g, b)
            } else if cross && d2 <= outer2 + 160 {
                argb(0xCC, 0xFF, 0xFF, 0xFF)
            } else {
                0
            };

            if rgba != 0 {
                let off = (y as usize)
                    .saturating_mul(pitch_bytes)
                    .saturating_add((x as usize).saturating_mul(4));
                unsafe {
                    core::ptr::write_volatile(ptr.add(off) as *mut u32, rgba);
                }
            }
        }
    }
}

#[inline]
fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}
