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
const PLANE_CTL_ROTATE_MASK: u32 = 0x03;
const PLANE_CTL_FORMAT_XRGB_8888: u32 = 4 << 24;
const PLANE_CTL_TILED_LINEAR: u32 = 0 << 10;
const PLANE_COLOR_ALPHA_MASK: u32 = 0x03 << 4;
const PIPE_BOTTOM_COLOR_RGB: u32 = 0x00FF_37FF;
const PRIMARY_BYTES_PER_PIXEL: u32 = 4;

static PRIMARY_GRADIENT_INIT: AtomicBool = AtomicBool::new(false);
static PRIMARY_SURFACE: Mutex<Option<PrimarySurface>> = Mutex::new(None);

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

    let Some(pitch_bytes) = aligned_pitch_bytes(width, PRIMARY_BYTES_PER_PIXEL) else {
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

    clear_surface_black(virt, pitch_bytes as usize, height);
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
    let ctl_programmed = (ctl_before & !(PLANE_CTL_FORMAT_MASK_SKL | PLANE_CTL_TILED_MASK))
        | PLANE_CTL_FORMAT_XRGB_8888
        | PLANE_CTL_TILED_LINEAR;
    let surf_before = crate::intel::mmio_read(dev, pipe.plane_surf_off);
    crate::intel::mmio_write(dev, pipe.plane_stride_off, stride_reg);
    crate::intel::mmio_write(dev, pipe.plane_ctl_off, ctl_programmed);
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
    crate::intel::render::submit_primary_triangle_once();
    log_primary_plane_probe(dev, pipe, "primary-live");

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

fn log_primary_plane_probe(dev: crate::intel::Dev, pipe: PipeInfo, label: &str) {
    let ctl = crate::intel::mmio_read(dev, pipe.plane_ctl_off);
    let stride = crate::intel::mmio_read(dev, pipe.plane_stride_off);
    let pos = crate::intel::mmio_read(dev, pipe.plane_ctl_off + UNI_PLANE_POS_OFF);
    let size = crate::intel::mmio_read(dev, pipe.plane_ctl_off + UNI_PLANE_SIZE_OFF);
    let keyval = crate::intel::mmio_read(dev, pipe.plane_ctl_off + UNI_PLANE_KEYVAL_OFF);
    let keymsk = crate::intel::mmio_read(dev, pipe.plane_ctl_off + UNI_PLANE_KEYMSK_OFF);
    let surf = crate::intel::mmio_read(dev, pipe.plane_surf_off);
    let keymax = crate::intel::mmio_read(dev, pipe.plane_ctl_off + UNI_PLANE_KEYMAX_OFF);
    let offset = crate::intel::mmio_read(dev, pipe.plane_ctl_off + UNI_PLANE_OFFSET_OFF);
    let surf_live = crate::intel::mmio_read(dev, pipe.plane_surf_live_off);
    let aux_dist = crate::intel::mmio_read(dev, pipe.plane_ctl_off + UNI_PLANE_AUX_DIST_OFF);
    let aux_offset = crate::intel::mmio_read(dev, pipe.plane_ctl_off + UNI_PLANE_AUX_OFFSET_OFF);
    let color_ctl = crate::intel::mmio_read(dev, pipe.plane_ctl_off + UNI_PLANE_COLOR_CTL_OFF);
    let buf_cfg = crate::intel::mmio_read(dev, pipe.plane_ctl_off + UNI_PLANE_BUF_CFG_OFF);

    crate::log!(
        "intel/display: primary-probe label={} pipe={} ctl=0x{:08X} enabled={} format={} tiled={} rot={} rgbx={} stride=0x{:08X} pos={}x{} size={}x{} offset={}x{} surf=0x{:08X} surf_live=0x{:08X} color_ctl=0x{:08X} color_alpha={} key=0x{:08X}/0x{:08X}/0x{:08X} aux=0x{:08X}/0x{:08X} buf_cfg=0x{:08X}\n",
        label,
        pipe.name,
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

fn aligned_pitch_bytes(width: u32, bytes_per_pixel: u32) -> Option<u32> {
    let bytes = width.checked_mul(bytes_per_pixel)?;
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

fn clear_surface_black(ptr: *mut u8, pitch_bytes: usize, height: u32) {
    unsafe {
        core::ptr::write_bytes(ptr, 0, pitch_bytes.saturating_mul(height as usize));
    }
}
