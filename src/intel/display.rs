// Display proof contract.
//
// Current evidence from the bring-up transcript:
// - `primary-boot-surface` programs pipe-a at `2560x1440`, pitch `0x2800`.
// - The surface GPU address is `0x02000000`.
// - `surf_live` matches `surf`, and the boot logo path reports `ok=1`.
//
// This proves scanout handoff to known memory.  It does not prove the 3D
// pipeline rendered that memory; render must separately produce `ps-rt-proof
// accepted=1` before a displayed pixel can be attributed to GPU rendering.

use crate::intel::types::{Rgba8, UiRect, UiSurface, UiSurfaceFormat};
use alloc::{collections::VecDeque, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

mod regs;
pub(super) use self::regs::*;

macro_rules! intel_display_focus_log {
    ($($arg:tt)*) => {
        if crate::logflag::INTEL_STAGE1_LOGS || crate::logflag::INTEL_DISPLAY_NGIN_LOGS {
            crate::log!($($arg)*);
        }
    };
}

macro_rules! intel_display_verbose_log {
    ($($arg:tt)*) => {
        if crate::logflag::INTEL_DISPLAY_NGIN_LOGS && !crate::logflag::INTEL_STAGE1_LOGS {
            crate::log!($($arg)*);
        }
    };
}

mod display_probes;
pub(crate) use self::display_probes::{
    arm_decoded_nv12_overlay_plane_probe, decoded_nv12_overlay_plane_probe_replaces_cpu_present,
    log_display_plane_ladder_probe,
};
use self::display_probes::{
    arm_rgb_plane_probe_planes, log_pipe_scanout_probe, log_primary_dimensions_probe,
    log_primary_plane_probe, primary_format_probe_name, probe_boot_logo_decode,
    probe_primary_present_psr,
};

// PIPE_BOTTOM_COLOR is not A/R/G/B bytes. PRM layout is:
// bit31 gamma enable, bit30 CSC enable, bits29:20 R/V, bits19:10 G/Y, bits9:0 B/U.
// The color channels are unsigned U0.10, so white is 0x3FF in each channel.
const PIPE_BOTTOM_COLOR_RAW: u32 = pipe_bottom_color_u0_10(0x3FF, 0x3FF, 0x3FF);
const PRIMARY_FORMAT_PROBE_XRGB: u32 = 0;
const PRIMARY_FORMAT_PROBE_XBGR: u32 = 1;
const PRIMARY_FORMAT_PROBE_MODE: u32 = PRIMARY_FORMAT_PROBE_XRGB;
const UNIVERSAL_PLANE_SLOTS: usize = 4;
const PRIMARY_PRESENT_DISABLE_PSR_PROBE: bool = true;
const PRIMARY_BYTES_PER_PIXEL: u32 = 4;
const PRIMARY_BASELINE_COLOR: u32 = 0x00FF_37FF;
const VIDEO_NV12_BLACK_PROOF_LIFT: bool = false;
const PRIMARY_BOOT_LOGO_JPEG: &[u8] = include_bytes!("../../logo.jpg");
const PRIMARY_BOOT_HORIZON_STAMP_PNG: &[u8] = include_bytes!("../../HorizonServer.png");
const PRIMARY_BOOT_LOGO_ENABLED: bool = true;
const PRIMARY_BOOT_HORIZON_STAMP_ENABLED: bool = true;
const PRIMARY_BOOT_LOGO_DECODE_MODE: PrimaryBootLogoDecodeMode =
    PrimaryBootLogoDecodeMode::ZuneJpeg;
const PRIMARY_BOOT_LOGO_WAIT_TIMEOUT_MS: u64 = 5000;
const PRIMARY_BOOT_LOGO_PRESENT_HOLD_MS: u64 = 3000;
const PRIMARY_BOOT_DISPLAY_WARMUP_ENABLED: bool = true;
const PRIMARY_GPGPU_EDGE_GUARD_PIXELS: u32 = 64;

const fn pipe_bottom_color_u0_10(red: u32, green: u32, blue: u32) -> u32 {
    ((red & 0x3FF) << 20) | ((green & 0x3FF) << 10) | (blue & 0x3FF)
}
const JPG_CENTER_CROP: bool = true;
const PRIMARY_REARM_PRESERVE_NON_PRIMARY_PLANES: bool = true;
// Universal plane role map for pipe-local planes.
const UI_OVERLAY_PLANE_SLOT: usize = 1;
const VIDEO_NV12_PLANE_SLOT: usize = 2;
const VIDEO_NV12_Y_PLANE_SLOT: usize = 3;
const OVERLAY_PLANE_SLOT: usize = UI_OVERLAY_PLANE_SLOT;
const DEFAULT_OVERLAY_MARKER_ENABLED: bool = true;
const DEFAULT_OVERLAY_MARKER_SIZE: u32 = 50;
const DEFAULT_OVERLAY_MARKER_COLOR: u32 = 0x0000_0000;
const OVERLAY_MARGIN_X: u32 = 0;
const OVERLAY_MARGIN_Y: u32 = 0;
const OVERLAY_COMPOSITION_PROOF_MARKER_ENABLED: bool = true;
const OVERLAY_COMPOSITION_PROOF_MARKER_SIZE: u32 = 96;
const OVERLAY_COMPOSITION_PROOF_MARKER_GAP: u32 = 16;
const OVERLAY_COMPOSITION_PROOF_MARKER_X: u32 = 48;
const OVERLAY_COMPOSITION_PROOF_MARKER_Y: u32 = 48;
const OVERLAY_SWAP_BUFFER_COUNT: usize = 2;
const OVERLAY_SWAP_GPU_BASE: u64 = 0x0700_0000;
const OVERLAY_SWAP_GPU_STRIDE: u64 = 0x0200_0000;

static PRIMARY_BOOT_SURFACE_INIT: AtomicBool = AtomicBool::new(false);
static PRIMARY_PRESENT_SEQ: AtomicU32 = AtomicU32::new(0);
static UI_SURFACE_PRIMARY_COPY_SEQ: AtomicU32 = AtomicU32::new(0);
static PRIMARY_SURFACE: Mutex<Option<PrimarySurface>> = Mutex::new(None);
static PRIMARY_PLANE_SOURCE_BINDING: Mutex<Option<PrimaryPlaneSourceBinding>> = Mutex::new(None);
static UI3_BASE_SURFACE: Mutex<Option<DisplayRgba8Surface>> = Mutex::new(None);
static UI3_FRAME_SURFACE: Mutex<Option<DisplayRgba8Surface>> = Mutex::new(None);
static OVERLAY_PRESENT_SEQ: AtomicU32 = AtomicU32::new(0);
static OVERLAY_SURFACE: Mutex<OverlaySurfacePool> = Mutex::new(OverlaySurfacePool::new());
static HW_LOGO_PENDING_IDS: Mutex<VecDeque<u32>> = Mutex::new(VecDeque::new());
static HW_LOGO_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();
static HW_LOGO_NEXT_STAGE: AtomicU32 = AtomicU32::new(0);
static HW_LOGO_SEQUENCE_DONE: AtomicBool = AtomicBool::new(false);
static HW_LOGO_SEQUENCE_DONE_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum PrimaryBootLogoDecodeMode {
    HwPic,
    ZuneJpeg,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct PrimarySurfaceSampleSet {
    pub(crate) tl: u32,
    pub(crate) center: u32,
    pub(crate) br: u32,
    pub(crate) apex: u32,
    pub(crate) centroid: u32,
    pub(crate) left: u32,
    pub(crate) right: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct PrimarySurfaceBgra8Snapshot {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct LiveOverlayRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) color: Rgba8,
}

impl LiveOverlayRect {
    pub(crate) const fn new(x: u32, y: u32, width: u32, height: u32, color: Rgba8) -> Self {
        Self {
            x,
            y,
            width,
            height,
            color,
        }
    }
}

impl PrimarySurfaceSampleSet {
    pub(crate) fn any_changed_since(self, before: Self) -> bool {
        self.tl != before.tl
            || self.center != before.center
            || self.br != before.br
            || self.apex != before.apex
            || self.centroid != before.centroid
            || self.left != before.left
            || self.right != before.right
    }

    pub(crate) fn triangle_points_changed_since(self, before: Self) -> bool {
        self.apex != before.apex
            || self.centroid != before.centroid
            || self.left != before.left
            || self.right != before.right
    }
}

#[derive(Copy, Clone)]
struct PrimarySurface {
    width: u32,
    height: u32,
    backing_width: u32,
    backing_height: u32,
    pitch_bytes: u32,
    byte_len: usize,
    phys: u64,
    virt: *mut u8,
    gpu: u64,
    pipe: PipeInfo,
}

unsafe impl Send for PrimarySurface {}
unsafe impl Sync for PrimarySurface {}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum PrimaryPlaneSourceFormat {
    Xrgb8888,
    Xbgr8888,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct PrimaryPlaneSource {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) byte_len: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
    pub(crate) format: PrimaryPlaneSourceFormat,
    pub(crate) src_x: u32,
    pub(crate) src_y: u32,
    pub(crate) dst_x: u32,
    pub(crate) dst_y: u32,
    pub(crate) dst_w: u32,
    pub(crate) dst_h: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct PrimaryPlaneSourceBinding {
    phys: u64,
    gpu: u64,
    byte_len: usize,
    width: u32,
    height: u32,
    pitch_bytes: u32,
    format: PrimaryPlaneSourceFormat,
}

#[derive(Copy, Clone)]
struct PrimaryBackingCopyRect {
    src_x: usize,
    src_y: usize,
    dst_x: usize,
    dst_y: usize,
    width: usize,
    height: usize,
    src_pitch: usize,
    dst_pitch: usize,
    row_bytes: usize,
    flush_offset: usize,
    flush_bytes: usize,
}

#[derive(Copy, Clone)]
struct DisplayRgba8Surface {
    width: u32,
    height: u32,
    pitch_bytes: u32,
    phys: u64,
    virt: *mut u8,
    gpu: u64,
    byte_len: usize,
}

unsafe impl Send for DisplayRgba8Surface {}
unsafe impl Sync for DisplayRgba8Surface {}

#[derive(Copy, Clone)]
pub(super) struct DisplayRgba8GpgpuSurface {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) pitch_bytes: u32,
    pub(super) phys: u64,
    pub(super) virt: *mut u8,
    pub(super) gpu: u64,
    pub(super) byte_len: usize,
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub(super) struct PrimarySurfaceGpgpuTarget {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) pitch_bytes: u32,
    pub(super) gpu: u64,
    pub(super) phys: u64,
    pub(super) virt: *mut u8,
    pub(super) byte_len: usize,
    pub(super) marker_gpu: u64,
    pub(super) marker_virt: *mut u8,
    pub(super) marker_offset: usize,
    pub(super) marker_x: u32,
    pub(super) marker_y: u32,
}

unsafe impl Send for PrimarySurfaceGpgpuTarget {}
unsafe impl Sync for PrimarySurfaceGpgpuTarget {}

#[derive(Copy, Clone)]
struct OverlaySurface {
    width: u32,
    height: u32,
    pitch_bytes: u32,
    byte_len: usize,
    phys: u64,
    virt: *mut u8,
    gpu: u64,
    pipe: PipeInfo,
    plane_slot: usize,
    buffer_index: usize,
}

unsafe impl Send for OverlaySurface {}
unsafe impl Sync for OverlaySurface {}

#[derive(Copy, Clone)]
struct OverlaySurfacePool {
    width: u32,
    height: u32,
    pipe_slot: usize,
    front_index: Option<usize>,
    surfaces: [Option<OverlaySurface>; OVERLAY_SWAP_BUFFER_COUNT],
}

impl OverlaySurfacePool {
    const fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            pipe_slot: usize::MAX,
            front_index: None,
            surfaces: [None; OVERLAY_SWAP_BUFFER_COUNT],
        }
    }

    fn matches(self, width: u32, height: u32, pipe: PipeInfo) -> bool {
        self.width == width && self.height == height && self.pipe_slot == pipe.slot
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum OverlayAlphaMode {
    Opaque,
    Straight,
}

pub(crate) fn init_primary_boot_surface(dev: crate::intel::Dev) {
    if PRIMARY_BOOT_SURFACE_INIT.swap(true, Ordering::AcqRel) {
        return;
    }

    log_pipe_scanout_probe(dev, "before-primary-init");
    log_transcoder_a_state(dev, "before-primary-init");

    let Some(pipe) = active_pipe(dev) else {
        crate::log!("intel/display: primary-boot-surface skipped no active pipe discovered\n");
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
            "intel/display: primary-boot-surface skipped no dimensions pipe={}\n",
            pipe.name
        );
        return;
    };
    log_primary_dimensions_probe(pipe.name, pipe_src_raw, pipe_src_dims, fb_dims, chosen_from);
    program_pipe_bottom_color(dev, pipe, PIPE_BOTTOM_COLOR_RAW);

    let backing_width = width.saturating_add(PRIMARY_GPGPU_EDGE_GUARD_PIXELS);
    let backing_height = height.saturating_add(PRIMARY_GPGPU_EDGE_GUARD_PIXELS);
    let Some(pitch_bytes) = aligned_pitch_bytes(backing_width, PRIMARY_BYTES_PER_PIXEL) else {
        crate::log!("intel/display: primary-boot-surface skipped bad pitch width={}\n", width);
        return;
    };
    let Some(byte_len) = usize::try_from(u64::from(pitch_bytes) * u64::from(backing_height)).ok()
    else {
        crate::log!("intel/display: primary-boot-surface skipped surface too large\n");
        return;
    };
    let Some((phys, virt)) = crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN) else {
        crate::log!("intel/display: primary-boot-surface alloc failed bytes=0x{:X}\n", byte_len);
        return;
    };

    crate::intel::dma_flush(virt, byte_len);

    if !crate::intel::map_display_scanout_ggtt(
        dev,
        phys,
        byte_len,
        crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE,
    ) {
        crate::log!(
            "intel/display: primary-boot-surface ggtt map failed bytes=0x{:X} gpu=0x{:X}\n",
            byte_len,
            crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE
        );
        return;
    }
    crate::intel::ggtt_invalidate(dev);

    let Some(_stride_reg) = plane_stride_reg_value(pitch_bytes) else {
        crate::log!(
            "intel/display: primary-boot-surface stride encode failed pitch=0x{:X}\n",
            pitch_bytes
        );
        return;
    };
    let Some(surface_reg) = u32::try_from(crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE).ok() else {
        crate::log!("intel/display: primary-boot-surface gpu addr out of range\n");
        return;
    };

    log_primary_plane_probe(dev, pipe, "before-arm");
    let ctl_before = crate::intel::mmio_read(dev, pipe.primary_plane().ctl());
    let surf_before = crate::intel::mmio_read(dev, pipe.primary_plane().surf());
    let (_, _, surf_live, _) = program_primary_plane_and_wait(
        dev,
        pipe,
        width,
        height,
        pitch_bytes,
        surface_reg,
        "init-arm",
    );
    log_primary_plane_probe(dev, pipe, "after-arm");
    log_pipe_scanout_probe(dev, "after-primary-init");
    let surf_armed = crate::intel::mmio_read(dev, pipe.primary_plane().surf());
    let ctl_after = crate::intel::mmio_read(dev, pipe.primary_plane().ctl());
    let ok = surf_live == surface_reg || surf_armed == surface_reg;

    let primary_surface = PrimarySurface {
        width,
        height,
        backing_width,
        backing_height,
        pitch_bytes,
        byte_len,
        phys,
        virt,
        gpu: crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE,
        pipe,
    };
    *PRIMARY_SURFACE.lock() = Some(primary_surface);
    let default_overlay_marker_ok = init_default_overlay_marker(dev, primary_surface);
    let ui3_boot = crate::intel::full_ui3_boot_enabled();
    if ok && ui3_boot {
        crate::r::readiness::set(crate::r::readiness::UI3_INTEL_PRESENT_READY);
    } else if ok {
        crate::log!(
            "intel/display: ui3-ready held device=0x{:04X} name={} reason=logo-only-bringup\n",
            dev.device_id,
            crate::intel::display_device_name(dev.device_id)
        );
    }
    let ui3_base_ok = false;
    let ui3_frame_ok = false;
    log_primary_scanout_pte_window(dev, "after-primary-init", byte_len);

    let logo_ok = if PRIMARY_BOOT_LOGO_ENABLED {
        let warmup_ok = if PRIMARY_BOOT_DISPLAY_WARMUP_ENABLED {
            run_primary_display_warmup(primary_surface, false)
        } else {
            false
        };
        let logo_submitted = probe_boot_logo_decode();
        if !logo_submitted && warmup_ok {
            mark_hw_logo_sequence_done("display-warmup-no-logo");
        }
        logo_submitted || warmup_ok
    } else if PRIMARY_BOOT_DISPLAY_WARMUP_ENABLED {
        run_primary_display_warmup(primary_surface, true)
    } else {
        false
    };
    if !logo_ok {
        mark_hw_logo_sequence_done("not-started");
    }

    crate::log!(
        "intel/display: primary-boot-surface pipe={} size={}x{} backing={}x{} pitch=0x{:X} bytes=0x{:X} guard={} gpu=0x{:X} phys=0x{:X} plane_enabled={} ctl_before=0x{:08X} ctl_after=0x{:08X} surf_before=0x{:08X} surf=0x{:08X} surf_live=0x{:08X} ok={} logo={} ui3_ready={} default_overlay_marker={} ui3_base={} ui3_frame={}\n",
        pipe.name,
        width,
        height,
        backing_width,
        backing_height,
        pitch_bytes,
        byte_len,
        PRIMARY_GPGPU_EDGE_GUARD_PIXELS,
        crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE,
        phys,
        ((ctl_after & PLANE_CTL_ENABLE) != 0) as u8,
        ctl_before,
        ctl_after,
        surf_before,
        surf_armed,
        surf_live,
        ok as u8,
        logo_ok as u8,
        (ok && ui3_boot) as u8,
        default_overlay_marker_ok as u8,
        ui3_base_ok as u8,
        ui3_frame_ok as u8
    );
}

fn init_ui3_base_surface(
    dev: crate::intel::Dev,
    width: u32,
    height: u32,
    pitch_bytes: u32,
    byte_len: usize,
) -> bool {
    let Some((phys, virt)) = crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN) else {
        crate::log!("intel/display: ui3-base-surface alloc failed bytes=0x{:X}\n", byte_len);
        return false;
    };

    fill_surface_color(virt, pitch_bytes as usize, width, height, 0x00FF_FFFF);
    crate::intel::dma_flush(virt, byte_len);

    if !crate::intel::map_ggtt(dev, phys, byte_len, crate::intel::GPU_VA_DISPLAY_UI3_BASE_BASE) {
        crate::log!(
            "intel/display: ui3-base-surface ggtt map failed bytes=0x{:X} gpu=0x{:X}\n",
            byte_len,
            crate::intel::GPU_VA_DISPLAY_UI3_BASE_BASE
        );
        return false;
    }

    *UI3_BASE_SURFACE.lock() = Some(DisplayRgba8Surface {
        width,
        height,
        pitch_bytes,
        phys,
        virt,
        gpu: crate::intel::GPU_VA_DISPLAY_UI3_BASE_BASE,
        byte_len,
    });
    true
}

fn init_ui3_frame_surface(
    dev: crate::intel::Dev,
    width: u32,
    height: u32,
    pitch_bytes: u32,
    byte_len: usize,
) -> bool {
    let Some((phys, virt)) = crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN) else {
        crate::log!("intel/display: ui3-frame-surface alloc failed bytes=0x{:X}\n", byte_len);
        return false;
    };

    fill_surface_color(virt, pitch_bytes as usize, width, height, 0x00FF_FFFF);
    crate::intel::dma_flush(virt, byte_len);

    if !crate::intel::map_ggtt(dev, phys, byte_len, crate::intel::GPU_VA_DISPLAY_UI3_FRAME_BASE) {
        crate::log!(
            "intel/display: ui3-frame-surface ggtt map failed bytes=0x{:X} gpu=0x{:X}\n",
            byte_len,
            crate::intel::GPU_VA_DISPLAY_UI3_FRAME_BASE
        );
        return false;
    }

    *UI3_FRAME_SURFACE.lock() = Some(DisplayRgba8Surface {
        width,
        height,
        pitch_bytes,
        phys,
        virt,
        gpu: crate::intel::GPU_VA_DISPLAY_UI3_FRAME_BASE,
        byte_len,
    });
    true
}

fn stamp_horizon_logo_top_left_screen() -> bool {
    if !PRIMARY_BOOT_HORIZON_STAMP_ENABLED {
        return false;
    }

    let stamp = match crate::ui3::img::png_codec::decode_png_rgba(PRIMARY_BOOT_HORIZON_STAMP_PNG) {
        Ok(stamp) => stamp,
        Err(err) => {
            crate::log!(
                "intel/display: boot-logo horizon stamp decode failed code={} bytes=0x{:X}\n",
                err.code(),
                PRIMARY_BOOT_HORIZON_STAMP_PNG.len()
            );
            return false;
        }
    };

    let stamped = blend_rgba_primary_rect(
        stamp.rgba.as_slice(),
        stamp.width,
        stamp.height,
        stamp.width as usize * 4,
        0,
        0,
        0,
        0,
        stamp.width,
        stamp.height,
        "boot-logo-horizon-stamp-top-left-screen",
    );
    crate::log!(
        "intel/display: boot-logo horizon stamp src={}x{} dst=0,0 screen=top-left stored={}\n",
        stamp.width,
        stamp.height,
        stamped as u8
    );
    stamped
}

fn stamp_bgrt_logo_bottom_right_screen() -> bool {
    let Some((bgrt_width, bgrt_height, bgrt_pixels)) = crate::efi::acpi::bgrt::decoded_logo_rgba()
    else {
        crate::log!("intel/display: boot-logo bgrt stamp skipped reason=no-bgrt-logo\n");
        return false;
    };

    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    if surface.virt.is_null()
        || surface.width == 0
        || surface.height == 0
        || surface.pitch_bytes < surface.width.saturating_mul(4)
        || bgrt_width == 0
        || bgrt_height == 0
    {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    let copy_w = bgrt_width.min(dst_width);
    let copy_h = bgrt_height.min(dst_height);
    if copy_w == 0 || copy_h == 0 || bgrt_pixels.len() < bgrt_width.saturating_mul(bgrt_height) {
        return false;
    }

    let dst_x = dst_width.saturating_sub(copy_w);
    let dst_y = dst_height.saturating_sub(copy_h);
    let src_x = bgrt_width.saturating_sub(copy_w);
    let src_y = bgrt_height.saturating_sub(copy_h);

    for row in 0..copy_h {
        let src_row = src_y.saturating_add(row).saturating_mul(bgrt_width);
        let dst_row_off = dst_y
            .saturating_add(row)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col in 0..copy_w {
            let rgb = bgrt_pixels[src_row.saturating_add(src_x).saturating_add(col)];
            let r = ((rgb >> 16) & 0xFF) as u8;
            let g = ((rgb >> 8) & 0xFF) as u8;
            let b = (rgb & 0xFF) as u8;
            unsafe {
                core::ptr::write_volatile(dst_row.add(col), u32::from_le_bytes([b, g, r, 0]));
            }
        }
    }

    let flush_offset = dst_y
        .saturating_mul(dst_pitch)
        .saturating_add(dst_x.saturating_mul(4));
    let flush_bytes = copy_h
        .saturating_sub(1)
        .saturating_mul(dst_pitch)
        .saturating_add(copy_w.saturating_mul(4));
    let stamped = notify_primary_surface_external_write(
        "boot-logo-bgrt-stamp-bottom-right-screen",
        flush_offset,
        flush_bytes,
    );
    crate::log!(
        "intel/display: boot-logo bgrt stamp src={}x{} dst={},{} {}x{} screen=bottom-right stored={}\n",
        bgrt_width,
        bgrt_height,
        dst_x,
        dst_y,
        copy_w,
        copy_h,
        stamped as u8
    );
    stamped
}

fn run_primary_display_warmup(_surface: PrimarySurface, release_render_after: bool) -> bool {
    if release_render_after {
        mark_hw_logo_sequence_done("display-warmup");
    }
    crate::log!("intel/display: primary-display-warmup skipped reason=no-initial-white-fill\n");
    true
}

pub(crate) async fn wait_hw_logo_sequence_done() {
    if !PRIMARY_BOOT_LOGO_ENABLED {
        return;
    }
    while !HW_LOGO_SEQUENCE_DONE.load(Ordering::Acquire) {
        if !HW_LOGO_SEQUENCE_DONE_WAIT
            .wait_for_event_timeout(PRIMARY_BOOT_LOGO_WAIT_TIMEOUT_MS)
            .await
        {
            mark_hw_logo_sequence_done("logo-wait-timeout");
            return;
        }
    }
}

fn mark_hw_logo_sequence_done(reason: &'static str) {
    if HW_LOGO_SEQUENCE_DONE.swap(true, Ordering::AcqRel) {
        return;
    }
    crate::log!("intel/display: hw-logo sequence done reason={}\n", reason);
    HW_LOGO_SEQUENCE_DONE_WAIT.notify_all();
}

fn submit_next_hw_logo_stage() -> bool {
    let stage_idx = HW_LOGO_NEXT_STAGE.fetch_add(1, Ordering::AcqRel) as usize;
    if stage_idx != 0 {
        return false;
    }
    submit_hw_logo_stage("logo", PRIMARY_BOOT_LOGO_JPEG)
}

fn submit_hw_logo_stage(name: &'static str, jpeg: &'static [u8]) -> bool {
    match crate::intel::hw_pic_submit_jpeg(jpeg) {
        Ok(id) => {
            HW_LOGO_PENDING_IDS.lock().push_back(id);
            HW_LOGO_WAIT.notify_all();
            let snap = crate::intel::hw_pic_snapshot();
            crate::log!(
                "intel/display: hw-logo submit ok stage={} id={} bytes=0x{:X} pending={} outputs={} service={}\n",
                name,
                id,
                jpeg.len(),
                snap.pending,
                snap.outputs,
                snap.service_started as u8
            );
            true
        }
        Err(code) => {
            let snap = crate::intel::hw_pic_snapshot();
            crate::log!(
                "intel/display: hw-logo submit failed stage={} code={} bytes=0x{:X} pending={} outputs={} service={}\n",
                name,
                code,
                jpeg.len(),
                snap.pending,
                snap.outputs,
                snap.service_started as u8
            );
            false
        }
    }
}

#[embassy_executor::task]
pub(crate) async fn hw_logo_present_task() {
    loop {
        let pending_id = HW_LOGO_PENDING_IDS.lock().pop_front();
        let Some(pending_id) = pending_id else {
            HW_LOGO_WAIT.wait_for_event().await;
            continue;
        };

        let Some(output) = crate::intel::hw_pic_wait_output_for_id(pending_id, 0).await else {
            continue;
        };

        let (visible_x, visible_y, visible_width, visible_height, target_width, target_height) =
            if output.width != 0 && output.height != 0 {
                if let Some(surface) = *PRIMARY_SURFACE.lock() {
                    let source_width = output.visible_width.max(1).min(output.width);
                    let source_height = output.visible_height.max(1).min(output.height);
                    if JPG_CENTER_CROP
                        && (source_width > surface.width || source_height > surface.height)
                    {
                        let (crop_w, crop_h) = center_crop_size(
                            source_width as usize,
                            source_height as usize,
                            surface.width as usize,
                            surface.height as usize,
                        );
                        (
                            source_width.saturating_sub(crop_w as u32) / 2,
                            source_height.saturating_sub(crop_h as u32) / 2,
                            crop_w as u32,
                            crop_h as u32,
                            surface.width as usize,
                            surface.height as usize,
                        )
                    } else {
                        let (fit_w, fit_h) = aspect_fit_size(
                            source_width as usize,
                            source_height as usize,
                            surface.width as usize,
                            surface.height as usize,
                        );
                        (0, 0, source_width, source_height, fit_w, fit_h)
                    }
                } else {
                    (0, 0, 0, 0, 0, 0)
                }
            } else {
                (0, 0, 0, 0, 0, 0)
            };

        let stored = if matches!(
            output.status,
            crate::intel::hw_pic::HwPicStatus::Ready | crate::intel::hw_pic::HwPicStatus::Streamed
        ) && matches!(
            output.format,
            crate::intel::hw_pic::HwPicPixelFormat::Imc3
                | crate::intel::hw_pic::HwPicPixelFormat::Nv12
        ) && output.width != 0
            && output.height != 0
            && output.visible_width != 0
            && output.visible_height != 0
            && output.pitch_bytes != 0
            && output.byte_len != 0
            && output.virt_addr != 0
        {
            let src = unsafe {
                core::slice::from_raw_parts(output.virt_addr as *const u8, output.byte_len)
            };
            match output.format {
                crate::intel::hw_pic::HwPicPixelFormat::Imc3 => present_imc3_surface_center(
                    src,
                    output.width,
                    output.height,
                    visible_x,
                    visible_y,
                    output.visible_width.min(visible_width),
                    output.visible_height.min(visible_height),
                    output.pitch_bytes,
                ),
                crate::intel::hw_pic::HwPicPixelFormat::Nv12 => present_nv12_surface_center(
                    src,
                    output.width,
                    output.height,
                    visible_x,
                    visible_y,
                    output.visible_width.min(visible_width),
                    output.visible_height.min(visible_height),
                    output.pitch_bytes,
                ),
                _ => false,
            }
        } else {
            false
        };

        crate::log!(
            "intel/display: hw-logo output id={} status={:?} fmt={:?} decoded={}x{} visible={}x{} target={}x{} pitch=0x{:X} uv=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X} stored={} err={}\n",
            output.id,
            output.status,
            output.format,
            output.width,
            output.height,
            output.visible_width,
            output.visible_height,
            target_width,
            target_height,
            output.pitch_bytes,
            output.uv_offset,
            output.byte_len,
            output.gpu_addr,
            output.phys_addr,
            stored as u8,
            output.error_code,
        );

        if stored {
            Timer::after(EmbassyDuration::from_millis(PRIMARY_BOOT_LOGO_PRESENT_HOLD_MS)).await;
        }
        if !submit_next_hw_logo_stage() {
            mark_hw_logo_sequence_done("stages-drained");
        }
    }
}

fn log_transcoder_a_state(dev: crate::intel::Dev, label: &str) {
    let pipe_src = crate::intel::mmio_read(dev, PIPE_A_SRC);
    let pipeconf = crate::intel::mmio_read(dev, PIPECONF_A);
    let htotal = crate::intel::mmio_read(dev, TRANS_HTOTAL_A);
    let hsync = crate::intel::mmio_read(dev, TRANS_HSYNC_A);
    let vtotal = crate::intel::mmio_read(dev, TRANS_VTOTAL_A);
    let vsync = crate::intel::mmio_read(dev, TRANS_VSYNC_A);
    let ddi_func_ctl = crate::intel::mmio_read(dev, TRANS_DDI_FUNC_CTL_A);
    let ddi_select = (ddi_func_ctl >> 27) & 0x07;
    let mode_select = (ddi_func_ctl >> 24) & 0x07;
    let bits_per_color = (ddi_func_ctl >> 20) & 0x03;
    let sync_polarity = (ddi_func_ctl >> 16) & 0x03;
    let port_width = (ddi_func_ctl >> 1) & 0x07;
    intel_display_verbose_log!(
        "intel/display: transcoder-a label={} pipe_src=0x{:08X} pipeconf=0x{:08X} pipe_enable={} pipe_state={} ddi_func_ctl=0x{:08X} trans_enable={} ddi_select={} ddi={} mode_select={} mode={} bpc={} sync_pol=0x{:X} port_width={} htotal=0x{:08X} hsync=0x{:08X} vtotal=0x{:08X} vsync=0x{:08X}\n",
        label,
        pipe_src,
        pipeconf,
        ((pipeconf >> 31) & 1),
        ((pipeconf >> 30) & 1),
        ddi_func_ctl,
        ((ddi_func_ctl >> 31) & 1),
        ddi_select,
        decode_trans_ddi_select(ddi_select),
        mode_select,
        decode_trans_ddi_mode(mode_select),
        decode_trans_bits_per_color(bits_per_color),
        sync_polarity,
        port_width,
        htotal,
        hsync,
        vtotal,
        vsync
    );
}

fn decode_trans_ddi_select(v: u32) -> &'static str {
    match v {
        0 => "none",
        1 => "ddi-b",
        2 => "ddi-c",
        3 => "ddi-d",
        4 => "ddi-e/tc1",
        5 => "ddi-f/tc2",
        6 => "ddi-g/tc3",
        7 => "ddi-h/tc4",
        _ => "unknown",
    }
}

fn decode_trans_ddi_mode(v: u32) -> &'static str {
    match v {
        0 => "hdmi",
        1 => "dvi",
        2 => "dp-sst",
        3 => "dp-mst",
        4 => "fdi-or-reserved",
        _ => "unknown",
    }
}

fn decode_trans_bits_per_color(v: u32) -> u32 {
    match v {
        0 => 8,
        1 => 10,
        2 => 6,
        3 => 12,
        _ => 0,
    }
}

fn program_pipe_bottom_color(dev: crate::intel::Dev, pipe: PipeInfo, raw: u32) {
    let reg = SKL_BOTTOM_COLOR_A + pipe.slot * SKL_BOTTOM_COLOR_PIPE_STRIDE;
    crate::intel::mmio_write(dev, reg, raw);
    let readback = crate::intel::mmio_read(dev, reg);
    intel_display_verbose_log!(
        "intel/display: bottom-color pipe={} reg=0x{:05X} raw=0x{:08X} readback=0x{:08X}\n",
        pipe.name,
        reg,
        raw,
        readback
    );
}

fn pipe_bottom_color_from_xrgb(color: u32) -> u32 {
    let red = ((color >> 16) & 0xFF) * 0x3FF / 0xFF;
    let green = ((color >> 8) & 0xFF) * 0x3FF / 0xFF;
    let blue = (color & 0xFF) * 0x3FF / 0xFF;
    pipe_bottom_color_u0_10(red, green, blue)
}

#[allow(dead_code)]
pub(crate) fn active_scanout_dimensions() -> Option<(u32, u32)> {
    let dev = crate::intel::claimed_device()?;
    let pipe = active_pipe(dev)?;
    decode_pipe_src(crate::intel::mmio_read(dev, pipe.pipe_src_off)).or_else(framebuffer_hint)
}

#[allow(dead_code)]
pub(crate) fn primary_surface_gpu_addr() -> Option<u64> {
    PRIMARY_SURFACE.lock().as_ref().map(|surface| surface.gpu)
}

pub(crate) fn log_primary_surface_samples(label: &str) {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return;
    };
    log_surface_samples(surface, label);
}

pub(crate) fn capture_primary_surface_samples() -> Option<PrimarySurfaceSampleSet> {
    let surface = (*PRIMARY_SURFACE.lock())?;
    capture_surface_samples(surface)
}

pub(crate) fn capture_primary_surface_bgra8() -> Option<PrimarySurfaceBgra8Snapshot> {
    let surface = (*PRIMARY_SURFACE.lock())?;
    let width = surface.width as usize;
    let height = surface.height as usize;
    let pitch_bytes = surface.pitch_bytes as usize;
    if width == 0 || height == 0 || pitch_bytes < width.checked_mul(4)? || surface.virt.is_null() {
        return None;
    }

    let row_bytes = width.checked_mul(4)?;
    let byte_len = pitch_bytes.checked_mul(height)?;
    let pixel_bytes = row_bytes.checked_mul(height)?;
    let mut pixels = Vec::new();
    if pixels.try_reserve_exact(pixel_bytes).is_err() {
        return None;
    }
    pixels.resize(pixel_bytes, 0);

    crate::intel::dma_flush(surface.virt, byte_len);
    for y in 0..height {
        let src_off = y.checked_mul(pitch_bytes)?;
        let dst_off = y.checked_mul(row_bytes)?;
        unsafe {
            core::ptr::copy_nonoverlapping(
                surface.virt.add(src_off),
                pixels.as_mut_ptr().add(dst_off),
                row_bytes,
            );
        }
    }

    Some(PrimarySurfaceBgra8Snapshot {
        width: surface.width,
        height: surface.height,
        pixels,
    })
}

pub(crate) fn sample_primary_surface_pixel(x: u32, y: u32) -> Option<u32> {
    let surface = (*PRIMARY_SURFACE.lock())?;
    sample_surface_pixel(surface, x as usize, y as usize)
}

pub(crate) fn clear_primary_surface_color(color: u32, reason: &str) -> bool {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        crate::log!(
            "intel/display: primary-clear skipped reason={} cause=no-primary-surface\n",
            reason,
        );
        return false;
    };
    if surface.virt.is_null()
        || surface.width == 0
        || surface.height == 0
        || surface.pitch_bytes == 0
    {
        crate::log!("intel/display: primary-clear skipped reason={} cause=bad-surface\n", reason,);
        return false;
    }

    let byte_len = (surface.pitch_bytes as usize).saturating_mul(surface.height as usize);
    if byte_len == 0 {
        crate::log!("intel/display: primary-clear skipped reason={} cause=empty-surface\n", reason,);
        return false;
    }

    fill_surface_color(
        surface.virt,
        surface.pitch_bytes as usize,
        surface.width,
        surface.height,
        color,
    );
    crate::intel::dma_flush(surface.virt, byte_len);
    let presented = notify_primary_surface_present(surface, reason, byte_len);
    crate::log!(
        "intel/display: primary-clear reason={} color=0x{:08X} size={}x{} pitch=0x{:X} bytes=0x{:X} presented={}\n",
        reason,
        color,
        surface.width,
        surface.height,
        surface.pitch_bytes,
        byte_len,
        presented as u8,
    );
    presented
}

pub(crate) fn present_i226_diagnostic_screen(
    snapshot: crate::net::i226::I226Snapshot,
    reason: &str,
) -> bool {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        crate::log!(
            "intel/display: i226-screen skipped reason={} cause=no-primary-surface\n",
            reason
        );
        return false;
    };
    if surface.virt.is_null()
        || surface.width == 0
        || surface.height == 0
        || surface.pitch_bytes == 0
    {
        crate::log!("intel/display: i226-screen skipped reason={} cause=bad-surface\n", reason);
        return false;
    }

    let byte_len = (surface.pitch_bytes as usize).saturating_mul(surface.height as usize);
    if byte_len == 0 {
        crate::log!("intel/display: i226-screen skipped reason={} cause=empty-surface\n", reason);
        return false;
    }

    fill_surface_color(
        surface.virt,
        surface.pitch_bytes as usize,
        surface.width,
        surface.height,
        0x00FF_FFFF,
    );

    let title_scale = if surface.width >= 1920 { 8 } else { 5 };
    let body_scale = if surface.width >= 1920 { 4 } else { 3 };
    let left = 72u32.min(surface.width.saturating_sub(1));
    let mut y = 72u32.min(surface.height.saturating_sub(1));
    let title = "NETWORK CARD";
    let title_pixels = draw_primary_text_line(surface, left, y, title_scale, title);
    y = y.saturating_add(title_scale.saturating_mul(11));
    let subtitle = "INTEL I226-V CLAIMED - PASSIVE DIAGNOSTIC MODE";
    let subtitle_pixels = draw_primary_text_line(surface, left, y, body_scale, subtitle);
    y = y.saturating_add(body_scale.saturating_mul(12));

    let mut lines: Vec<String> = Vec::new();
    lines.push(alloc::format!(
        "BDF {:02X}:{:02X}.{}  VID:PID {:04X}:{:04X}  REV {:02X}",
        snapshot.bus,
        snapshot.slot,
        snapshot.function,
        snapshot.vendor,
        snapshot.device,
        snapshot.revision
    ));
    lines.push(alloc::format!(
        "CLASS {:02X}:{:02X}.{:02X}  PCI CMD {:04X}->{:04X}  PCI STATUS {:04X}",
        snapshot.class,
        snapshot.subclass,
        snapshot.prog_if,
        snapshot.pci_command_before,
        snapshot.pci_command_after,
        snapshot.pci_status
    ));
    lines.push(alloc::format!(
        "BAR{} PHYS 0X{:X}  BAR SIZE 0X{:X}  MAP SIZE 0X{:X}",
        snapshot.bar_index,
        snapshot.bar_phys,
        snapshot.bar_size,
        snapshot.map_size
    ));
    lines.push(alloc::format!(
        "MAC {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        snapshot.mac[0],
        snapshot.mac[1],
        snapshot.mac[2],
        snapshot.mac[3],
        snapshot.mac[4],
        snapshot.mac[5]
    ));
    lines.push(alloc::format!(
        "STATUS 0X{:08X}  LINK RAW={}  SPEED RAW={}MBIT  FULL DUPLEX RAW={}",
        snapshot.status,
        yes_no(snapshot.raw_link_up()),
        snapshot.raw_speed_mbps(),
        yes_no(snapshot.raw_full_duplex())
    ));
    lines.push(alloc::format!(
        "CTRL 0X{:08X}  EECD 0X{:08X}  ICR 0X{:08X}  IMS 0X{:08X}",
        snapshot.ctrl,
        snapshot.eecd,
        snapshot.icr,
        snapshot.ims
    ));
    lines.push(alloc::format!(
        "RCTL 0X{:08X}  TCTL 0X{:08X}  MSI-X VECTORS {}",
        snapshot.rctl,
        snapshot.tctl,
        snapshot.msix_vectors
    ));
    lines.push(alloc::format!(
        "CAP MASK 0X{:08X}  CAPS {}  PASSIVE {}",
        snapshot.cap_mask,
        snapshot.caps_text(),
        yes_no(snapshot.passive)
    ));
    lines.push(String::from("RX/TX DMA DEFERRED. NO RESET. NO RINGS. NO PACKET HANDOFF YET."));
    lines.push(String::from(
        "THIS SCREEN WAS DRAWN 10S AFTER THE BOOT LOGO USING OWNED PRIMARY SCANOUT.",
    ));

    let mut text_pixels = title_pixels.saturating_add(subtitle_pixels);
    for line in lines.iter() {
        text_pixels = text_pixels.saturating_add(draw_primary_text_line(
            surface,
            left,
            y,
            body_scale,
            line.as_str(),
        ));
        y = y.saturating_add(body_scale.saturating_mul(10));
        if y >= surface.height.saturating_sub(body_scale.saturating_mul(8)) {
            break;
        }
    }

    crate::intel::dma_flush(surface.virt, byte_len);
    let presented = notify_primary_surface_present(surface, reason, byte_len);
    crate::log!(
        "intel/display: i226-screen reason={} bdf={:02x}:{:02x}.{} size={}x{} pitch=0x{:X} bytes=0x{:X} text_pixels={} presented={}\n",
        reason,
        snapshot.bus,
        snapshot.slot,
        snapshot.function,
        surface.width,
        surface.height,
        surface.pitch_bytes,
        byte_len,
        text_pixels,
        presented as u8
    );
    presented
}

fn yes_no(v: bool) -> &'static str {
    if v { "YES" } else { "NO" }
}

fn draw_primary_text_line(
    surface: PrimarySurface,
    x: u32,
    y: u32,
    scale: u32,
    text: &str,
) -> usize {
    if scale == 0 || surface.virt.is_null() {
        return 0;
    }
    let mut pen_x = x;
    let mut pixels = 0usize;
    let advance = scale.saturating_mul(6);
    for ch in text.chars() {
        if pen_x >= surface.width {
            break;
        }
        pixels = pixels.saturating_add(draw_primary_glyph(surface, pen_x, y, scale, ch));
        pen_x = pen_x.saturating_add(advance);
    }
    pixels
}

fn draw_primary_glyph(surface: PrimarySurface, x: u32, y: u32, scale: u32, ch: char) -> usize {
    let glyph = glyph5x7(ch);
    let pitch = surface.pitch_bytes as usize;
    let mut pixels = 0usize;
    for (row_idx, row_bits) in glyph.iter().copied().enumerate() {
        for col in 0..5u32 {
            if (row_bits & (1 << (4 - col))) == 0 {
                continue;
            }
            let px0 = x.saturating_add(col.saturating_mul(scale));
            let py0 = y.saturating_add((row_idx as u32).saturating_mul(scale));
            for sy in 0..scale {
                let py = py0.saturating_add(sy);
                if py >= surface.height {
                    continue;
                }
                for sx in 0..scale {
                    let px = px0.saturating_add(sx);
                    if px >= surface.width {
                        continue;
                    }
                    let off = (py as usize).saturating_mul(pitch).saturating_add(
                        (px as usize).saturating_mul(PRIMARY_BYTES_PER_PIXEL as usize),
                    );
                    if off.saturating_add(core::mem::size_of::<u32>()) > surface.byte_len {
                        continue;
                    }
                    unsafe {
                        core::ptr::write_volatile(surface.virt.add(off) as *mut u32, 0x0000_0000);
                    }
                    pixels = pixels.saturating_add(1);
                }
            }
        }
    }
    pixels
}

fn glyph5x7(ch: char) -> [u8; 7] {
    let upper = if ch.is_ascii_lowercase() {
        ((ch as u8) - b'a' + b'A') as char
    } else {
        ch
    };
    match upper {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0F, 0x10, 0x10, 0x10, 0x10, 0x10, 0x0F],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0F, 0x10, 0x10, 0x13, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1F],
        'J' => [0x01, 0x01, 0x01, 0x01, 0x11, 0x11, 0x0E],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1E, 0x01, 0x01, 0x0E, 0x01, 0x01, 0x1E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x10, 0x1E, 0x01, 0x01, 0x1E],
        '6' => [0x0E, 0x10, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x01, 0x0E],
        ':' => [0x00, 0x04, 0x04, 0x00, 0x04, 0x04, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '>' => [0x10, 0x08, 0x04, 0x02, 0x04, 0x08, 0x10],
        '/' => [0x01, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10],
        '=' => [0x00, 0x1F, 0x00, 0x00, 0x1F, 0x00, 0x00],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        '_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x0C, 0x04, 0x08],
        ' ' => [0; 7],
        _ => [0x1F, 0x11, 0x01, 0x02, 0x04, 0x00, 0x04],
    }
}

fn log_surface_samples(surface: PrimarySurface, label: &str) {
    let Some(samples) = capture_surface_samples(surface) else {
        return;
    };

    intel_display_focus_log!(
        "intel/display: primary-samples label={} gpu=0x{:X} phys=0x{:X} pitch=0x{:X} tl=0x{:08X} center=0x{:08X} br=0x{:08X} apex=0x{:08X} centroid=0x{:08X} left=0x{:08X} right=0x{:08X}\n",
        label,
        surface.gpu,
        surface.phys,
        surface.pitch_bytes,
        samples.tl,
        samples.center,
        samples.br,
        samples.apex,
        samples.centroid,
        samples.left,
        samples.right
    );
}

fn capture_surface_samples(surface: PrimarySurface) -> Option<PrimarySurfaceSampleSet> {
    let width = surface.width as usize;
    let height = surface.height as usize;
    let pitch_bytes = surface.pitch_bytes as usize;
    if width == 0 || height == 0 || pitch_bytes < 4 || surface.virt.is_null() {
        return None;
    }

    let clip_to_screen = |clip_x: f32, clip_y: f32| -> (usize, usize) {
        let sx = ((clip_x + 1.0) * 0.5 * width as f32).clamp(0.0, width.saturating_sub(1) as f32)
            as usize;
        let sy = ((1.0 - (clip_y + 1.0) * 0.5) * height as f32)
            .clamp(0.0, height.saturating_sub(1) as f32) as usize;
        (sx, sy)
    };
    let (apex_x, apex_y) = clip_to_screen(0.0, 0.72);
    let (left_x, left_y) = clip_to_screen(-0.72, -0.58);
    let (right_x, right_y) = clip_to_screen(0.72, -0.58);
    let (centroid_x, centroid_y) = clip_to_screen(0.0, -0.15);

    Some(PrimarySurfaceSampleSet {
        tl: sample_surface_pixel(surface, 0, 0)?,
        center: sample_surface_pixel(surface, width / 2, height / 2)?,
        br: sample_surface_pixel(surface, width.saturating_sub(1), height.saturating_sub(1))?,
        apex: sample_surface_pixel(surface, apex_x, apex_y)?,
        centroid: sample_surface_pixel(surface, centroid_x, centroid_y)?,
        left: sample_surface_pixel(surface, left_x, left_y)?,
        right: sample_surface_pixel(surface, right_x, right_y)?,
    })
}

fn sample_surface_pixel(surface: PrimarySurface, x: usize, y: usize) -> Option<u32> {
    let width = surface.width as usize;
    let height = surface.height as usize;
    let pitch_bytes = surface.pitch_bytes as usize;
    if width == 0 || height == 0 || pitch_bytes < 4 || surface.virt.is_null() {
        return None;
    }

    let clamped_x = x.min(width.saturating_sub(1));
    let clamped_y = y.min(height.saturating_sub(1));
    let byte_offset = clamped_y
        .checked_mul(pitch_bytes)?
        .checked_add(clamped_x.checked_mul(4)?)?;
    let sample_ptr = unsafe { surface.virt.add(byte_offset) };
    crate::intel::dma_flush(sample_ptr, core::mem::size_of::<u32>());
    Some(unsafe { core::ptr::read_volatile(sample_ptr as *const u32) })
}

pub(super) fn primary_surface_gpgpu_marker_target() -> Option<PrimarySurfaceGpgpuTarget> {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return None;
    };
    if surface.virt.is_null()
        || surface.width == 0
        || surface.height == 0
        || surface.pitch_bytes < PRIMARY_BYTES_PER_PIXEL
    {
        return None;
    }

    let marker_x = core::cmp::min(32, surface.width.saturating_sub(1));
    let marker_y = core::cmp::min(32, surface.height.saturating_sub(1));
    let marker_offset = (marker_y as usize)
        .saturating_mul(surface.pitch_bytes as usize)
        .saturating_add((marker_x as usize).saturating_mul(PRIMARY_BYTES_PER_PIXEL as usize));
    let byte_len = surface.byte_len;
    if marker_offset.saturating_add(core::mem::size_of::<u32>()) > byte_len {
        return None;
    }

    Some(PrimarySurfaceGpgpuTarget {
        width: surface.backing_width,
        height: surface.backing_height,
        pitch_bytes: surface.pitch_bytes,
        gpu: surface.gpu,
        phys: surface.phys,
        virt: surface.virt,
        byte_len,
        marker_gpu: surface.gpu + marker_offset as u64,
        marker_virt: unsafe { surface.virt.add(marker_offset) },
        marker_offset,
        marker_x,
        marker_y,
    })
}

pub(super) fn ui3_base_surface_gpgpu() -> Option<DisplayRgba8GpgpuSurface> {
    let surface = (*UI3_BASE_SURFACE.lock())?;
    if surface.virt.is_null() || surface.byte_len == 0 {
        return None;
    }
    Some(DisplayRgba8GpgpuSurface {
        width: surface.width,
        height: surface.height,
        pitch_bytes: surface.pitch_bytes,
        phys: surface.phys,
        virt: surface.virt,
        gpu: surface.gpu,
        byte_len: surface.byte_len,
    })
}

pub(super) fn ui3_frame_surface_gpgpu() -> Option<DisplayRgba8GpgpuSurface> {
    let surface = (*UI3_FRAME_SURFACE.lock())?;
    if surface.virt.is_null() || surface.byte_len == 0 {
        return None;
    }
    Some(DisplayRgba8GpgpuSurface {
        width: surface.width,
        height: surface.height,
        pitch_bytes: surface.pitch_bytes,
        phys: surface.phys,
        virt: surface.virt,
        gpu: surface.gpu,
        byte_len: surface.byte_len,
    })
}

pub(super) fn ui3_canvas_overlay_gpgpu(rect: LiveOverlayRect) -> Option<DisplayRgba8GpgpuSurface> {
    let dev = crate::intel::claimed_device()?;
    let (width, height) = active_scanout_dimensions()
        .or_else(|| {
            PRIMARY_SURFACE
                .lock()
                .as_ref()
                .map(|primary| (primary.width, primary.height))
        })
        .unwrap_or((0, 0));
    if width == 0 || height == 0 || rect.width == 0 || rect.height == 0 {
        return None;
    }

    let surface = ensure_overlay_surface(dev, width, height)?;
    fill_surface_color(
        surface.virt,
        surface.pitch_bytes as usize,
        surface.width,
        surface.height,
        0,
    );
    fill_overlay_rect(surface, rect.x, rect.y, rect.width, rect.height, 0);
    crate::intel::dma_flush(surface.virt, surface.byte_len);

    Some(DisplayRgba8GpgpuSurface {
        width: surface.width,
        height: surface.height,
        pitch_bytes: surface.pitch_bytes,
        phys: surface.phys,
        virt: surface.virt,
        gpu: surface.gpu,
        byte_len: surface.byte_len,
    })
}

pub(super) fn commit_ui3_canvas_overlay_gpgpu(
    target: DisplayRgba8GpgpuSurface,
    reason: &str,
) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(surface) = overlay_surface_for_gpu(target.width, target.height, target.gpu) else {
        return false;
    };
    if surface.virt != target.virt
        || surface.byte_len != target.byte_len
        || surface.pitch_bytes != target.pitch_bytes
    {
        return false;
    }

    crate::intel::dma_flush(target.virt, target.byte_len);
    program_three_plane_stack_resources(dev, surface.pipe, reason);
    arm_overlay_plane(dev, surface, 0, 0, OverlayAlphaMode::Straight, reason)
}

pub(super) fn notify_primary_surface_external_write(
    reason: &str,
    flush_offset: usize,
    flush_bytes: usize,
) -> bool {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    let byte_len = surface.byte_len;
    if !surface.virt.is_null() && flush_offset < byte_len {
        let flush_bytes = core::cmp::min(flush_bytes, byte_len.saturating_sub(flush_offset));
        crate::intel::dma_flush(unsafe { surface.virt.add(flush_offset) }, flush_bytes);
    }
    notify_primary_surface_present(surface, reason, byte_len)
}

pub(crate) fn set_primary_plane_source(source: PrimaryPlaneSource, reason: &str) -> bool {
    set_primary_plane_source_inner(source, reason, false)
}

pub(crate) fn set_primary_plane_source_mapped(source: PrimaryPlaneSource, reason: &str) -> bool {
    set_primary_plane_source_inner(source, reason, true)
}

fn set_primary_plane_source_inner(
    source: PrimaryPlaneSource,
    reason: &str,
    already_mapped: bool,
) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(primary) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    if source.phys == 0
        || source.gpu == 0
        || source.byte_len == 0
        || source.width == 0
        || source.height == 0
        || source.dst_w == 0
        || source.dst_h == 0
    {
        return false;
    }
    let Some(surface_reg) = u32::try_from(source.gpu).ok() else {
        return false;
    };
    let Some(stride_reg) = plane_stride_reg_value(source.pitch_bytes) else {
        return false;
    };

    let src_w = source.width.saturating_sub(source.src_x);
    let src_h = source.height.saturating_sub(source.src_y);
    let dst_w = source
        .dst_w
        .min(src_w)
        .min(primary.width.saturating_sub(source.dst_x));
    let dst_h = source
        .dst_h
        .min(src_h)
        .min(primary.height.saturating_sub(source.dst_y));
    if dst_w == 0 || dst_h == 0 {
        return false;
    }

    let min_pitch = source
        .width
        .saturating_mul(core::mem::size_of::<u32>() as u32);
    let min_bytes = (source.height as usize)
        .saturating_sub(1)
        .saturating_mul(source.pitch_bytes as usize)
        .saturating_add(min_pitch as usize);
    if source.pitch_bytes < min_pitch || source.byte_len < min_bytes {
        return false;
    }

    let binding = PrimaryPlaneSourceBinding {
        phys: source.phys,
        gpu: source.gpu,
        byte_len: source.byte_len,
        width: source.width,
        height: source.height,
        pitch_bytes: source.pitch_bytes,
        format: source.format,
    };
    let mut mapped_now = false;
    if *PRIMARY_PLANE_SOURCE_BINDING.lock() != Some(binding) {
        if !already_mapped
            && !crate::intel::map_display_scanout_ggtt(
                dev,
                source.phys,
                source.byte_len,
                source.gpu,
            )
        {
            crate::log!(
                "intel/display: primary-plane-source failed reason={} cause=ggtt gpu=0x{:X} phys=0x{:X} bytes=0x{:X}\n",
                reason,
                source.gpu,
                source.phys,
                source.byte_len
            );
            return false;
        }
        if !already_mapped {
            crate::intel::ggtt_invalidate(dev);
        }
        *PRIMARY_PLANE_SOURCE_BINDING.lock() = Some(binding);
        mapped_now = !already_mapped;
    }

    let pipe = primary.pipe;
    let ctl_before = crate::intel::mmio_read(dev, pipe.primary_plane().ctl());
    let ctl_enabled = primary_plane_ctl_enabled_for_format(ctl_before, source.format);
    let color_ctl_off = pipe.primary_plane().base() + UNI_PLANE_COLOR_CTL_OFF;
    let color_ctl = crate::intel::mmio_read(dev, color_ctl_off);
    let surf_live_before = crate::intel::mmio_read(dev, pipe.primary_plane().surf_live());
    crate::intel::mmio_write(dev, pipe.primary_plane().stride(), stride_reg);
    crate::intel::mmio_write(
        dev,
        pipe.primary_plane().base() + UNI_PLANE_POS_OFF,
        plane_pos_reg_value(source.dst_x, source.dst_y),
    );
    crate::intel::mmio_write(
        dev,
        pipe.primary_plane().base() + UNI_PLANE_SIZE_OFF,
        plane_size_reg_value(dst_w, dst_h),
    );
    crate::intel::mmio_write(
        dev,
        pipe.primary_plane().base() + UNI_PLANE_OFFSET_OFF,
        plane_pos_reg_value(source.src_x, source.src_y),
    );
    crate::intel::mmio_write(
        dev,
        color_ctl_off,
        plane_color_ctl_alpha(color_ctl, OverlayAlphaMode::Opaque),
    );
    crate::intel::mmio_write(dev, pipe.primary_plane().ctl(), ctl_enabled);
    crate::intel::mmio_write(dev, pipe.primary_plane().surf(), surface_reg);

    let surf_after = crate::intel::mmio_read(dev, pipe.primary_plane().surf());
    let (surf_live_after, surf_live_iter) =
        wait_for_primary_plane_live(dev, pipe, surface_reg, 200_000);
    intel_display_verbose_log!(
        "intel/display: primary-plane-source reason={} pipe={} ok={} live_ok={} mapped={} fmt={:?} src={}x{} dst={}x{} size={}x{} pitch=0x{:X} surf=0x{:08X} after=0x{:08X} live=0x{:08X}=>0x{:08X} live_iter={}\n",
        reason,
        pipe.name,
        (surf_after == surface_reg) as u8,
        (surf_live_after == surface_reg) as u8,
        mapped_now as u8,
        source.format,
        source.src_x,
        source.src_y,
        source.dst_x,
        source.dst_y,
        dst_w,
        dst_h,
        source.pitch_bytes,
        surface_reg,
        surf_after,
        surf_live_before,
        surf_live_after,
        surf_live_iter
    );
    surf_after == surface_reg && surf_live_after == surface_reg
}

pub(crate) fn present_ui_surface_to_primary_plane(
    surface: UiSurface,
    phys: u64,
    byte_len: usize,
    src: UiRect,
    dst: UiRect,
    reason: &str,
) -> bool {
    if src.is_empty() || dst.is_empty() {
        return false;
    }
    let format = match surface.format {
        UiSurfaceFormat::Xrgb8888 => PrimaryPlaneSourceFormat::Xrgb8888,
        UiSurfaceFormat::Xbgr8888 => PrimaryPlaneSourceFormat::Xbgr8888,
        UiSurfaceFormat::Rgba8888 => return false,
    };
    set_primary_plane_source(
        PrimaryPlaneSource {
            phys,
            gpu: surface.gpu,
            byte_len,
            width: surface.width,
            height: surface.height,
            pitch_bytes: surface.pitch,
            format,
            src_x: src.x,
            src_y: src.y,
            dst_x: dst.x,
            dst_y: dst.y,
            dst_w: dst.w,
            dst_h: dst.h,
        },
        reason,
    )
}

pub(crate) fn present_ui_surface_to_primary_backing(
    surface: UiSurface,
    virt: *const u8,
    byte_len: usize,
    src: UiRect,
    dst: UiRect,
    reason: &str,
) -> bool {
    let Some(primary) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    if !matches!(
        surface.format,
        UiSurfaceFormat::Rgba8888 | UiSurfaceFormat::Xrgb8888 | UiSurfaceFormat::Xbgr8888
    ) {
        return false;
    }
    if virt.is_null()
        || primary.virt.is_null()
        || byte_len == 0
        || surface.width == 0
        || surface.height == 0
        || src.is_empty()
        || dst.is_empty()
        || surface.pitch < surface.width.saturating_mul(4)
        || primary.pitch_bytes < primary.width.saturating_mul(PRIMARY_BYTES_PER_PIXEL)
    {
        return false;
    }

    let Some(rect) = primary_backing_copy_rect(surface, primary, src, dst, byte_len) else {
        return false;
    };

    for row in 0..rect.height {
        let src_off = rect
            .src_y
            .saturating_add(row)
            .saturating_mul(rect.src_pitch)
            .saturating_add(rect.src_x.saturating_mul(4));
        let dst_off = rect
            .dst_y
            .saturating_add(row)
            .saturating_mul(rect.dst_pitch)
            .saturating_add(rect.dst_x.saturating_mul(PRIMARY_BYTES_PER_PIXEL as usize));
        if src_off.saturating_add(rect.row_bytes) > byte_len
            || dst_off.saturating_add(rect.row_bytes) > primary.byte_len
        {
            return false;
        }
        match surface.format {
            UiSurfaceFormat::Xrgb8888 => unsafe {
                core::ptr::copy_nonoverlapping(
                    virt.add(src_off),
                    primary.virt.add(dst_off),
                    rect.row_bytes,
                );
            },
            UiSurfaceFormat::Xbgr8888 => {
                let src_row =
                    unsafe { core::slice::from_raw_parts(virt.add(src_off), rect.row_bytes) };
                let dst_row = unsafe { primary.virt.add(dst_off) as *mut u32 };
                for col in 0..rect.width {
                    let off = col.saturating_mul(4);
                    let r = src_row[off];
                    let g = src_row[off + 1];
                    let b = src_row[off + 2];
                    unsafe {
                        core::ptr::write_volatile(
                            dst_row.add(col),
                            u32::from_le_bytes([b, g, r, 0]),
                        );
                    }
                }
            }
            UiSurfaceFormat::Rgba8888 => {
                let src_row =
                    unsafe { core::slice::from_raw_parts(virt.add(src_off), rect.row_bytes) };
                let dst_row = unsafe { primary.virt.add(dst_off) as *mut u32 };
                for col in 0..rect.width {
                    let off = col.saturating_mul(4);
                    let r = src_row[off];
                    let g = src_row[off + 1];
                    let b = src_row[off + 2];
                    unsafe {
                        core::ptr::write_volatile(
                            dst_row.add(col),
                            u32::from_le_bytes([b, g, r, 0]),
                        );
                    }
                }
            }
        }
    }

    let presented =
        notify_primary_surface_external_write(reason, rect.flush_offset, rect.flush_bytes);
    let seq = UI_SURFACE_PRIMARY_COPY_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if seq <= 8 || seq.is_multiple_of(60) {
        crate::log!(
            "intel/display: ui-surface-primary-copy seq={} reason={} fmt={:?} src={},{} {}x{} dst={},{} copied={}x{} presented={}\n",
            seq,
            reason,
            surface.format,
            src.x,
            src.y,
            src.w,
            src.h,
            dst.x,
            dst.y,
            rect.width,
            rect.height,
            presented as u8
        );
    }
    presented
}

fn primary_backing_copy_rect(
    surface: UiSurface,
    primary: PrimarySurface,
    src: UiRect,
    dst: UiRect,
    byte_len: usize,
) -> Option<PrimaryBackingCopyRect> {
    let src_pitch = surface.pitch as usize;
    let dst_pitch = primary.pitch_bytes as usize;
    let src_x = src.x as usize;
    let src_y = src.y as usize;
    let dst_x = dst.x as usize;
    let dst_y = dst.y as usize;
    let src_w = surface.width.saturating_sub(src.x).min(src.w).min(dst.w) as usize;
    let src_h = surface.height.saturating_sub(src.y).min(src.h).min(dst.h) as usize;
    let dst_w = primary.width.saturating_sub(dst.x) as usize;
    let dst_h = primary.height.saturating_sub(dst.y) as usize;
    let width = src_w.min(dst_w);
    let height = src_h.min(dst_h);
    if width == 0 || height == 0 {
        return None;
    }

    let row_bytes = width.checked_mul(PRIMARY_BYTES_PER_PIXEL as usize)?;
    let src_last = src_y
        .checked_add(height.saturating_sub(1))?
        .checked_mul(src_pitch)?
        .checked_add(src_x.checked_mul(4)?)?
        .checked_add(row_bytes)?;
    let dst_last = dst_y
        .checked_add(height.saturating_sub(1))?
        .checked_mul(dst_pitch)?
        .checked_add(dst_x.checked_mul(PRIMARY_BYTES_PER_PIXEL as usize)?)?
        .checked_add(row_bytes)?;
    if src_last > byte_len || dst_last > primary.byte_len {
        return None;
    }

    let flush_offset = dst_y
        .checked_mul(dst_pitch)?
        .checked_add(dst_x.checked_mul(PRIMARY_BYTES_PER_PIXEL as usize)?)?;
    let flush_bytes = height
        .saturating_sub(1)
        .checked_mul(dst_pitch)?
        .checked_add(row_bytes)?;

    Some(PrimaryBackingCopyRect {
        src_x,
        src_y,
        dst_x,
        dst_y,
        width,
        height,
        src_pitch,
        dst_pitch,
        row_bytes,
        flush_offset,
        flush_bytes,
    })
}

pub(crate) fn present_rgba_primary(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || src_pitch_bytes < src_width as usize * 4
    {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    let copy_w = (src_width as usize).min(dst_width);
    let copy_h = (src_height as usize).min(dst_height);
    if copy_w == 0 || copy_h == 0 || dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    for row_idx in 0..copy_h {
        let src_row_off = row_idx.saturating_mul(src_pitch_bytes);
        let Some(src_row) = src.get(src_row_off..src_row_off + copy_w.saturating_mul(4)) else {
            return false;
        };
        let dst_row_off = row_idx.saturating_mul(dst_pitch);
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_off = col_idx.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
        crate::intel::dma_flush(unsafe { surface.virt.add(dst_row_off) }, copy_w.saturating_mul(4));
    }

    let byte_len = dst_pitch.saturating_mul(dst_height);
    notify_primary_surface_present(surface, reason, byte_len)
}

pub(crate) fn present_rgba_primary_center_unscaled(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    present_rgba_primary_center_unscaled_bg(src, src_width, src_height, src_pitch_bytes, 0, reason)
}

pub(crate) fn present_rgba_primary_center_unscaled_bg(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    bg_xrgb: u32,
    reason: &str,
) -> bool {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || src_pitch_bytes < src_width as usize * 4
    {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    let src_width = src_width as usize;
    let src_height = src_height as usize;
    if dst_width == 0 || dst_height == 0 || dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    let copy_w = src_width.min(dst_width);
    let copy_h = src_height.min(dst_height);
    if copy_w == 0 || copy_h == 0 {
        return false;
    }

    let src_x = src_width.saturating_sub(copy_w) / 2;
    let src_y = src_height.saturating_sub(copy_h) / 2;
    let dst_x = dst_width.saturating_sub(copy_w) / 2;
    let dst_y = dst_height.saturating_sub(copy_h) / 2;
    let byte_len = dst_pitch.saturating_mul(dst_height);

    fill_surface_color(surface.virt, dst_pitch, surface.width, surface.height, bg_xrgb);

    for row_idx in 0..copy_h {
        let src_row_off = src_y
            .saturating_add(row_idx)
            .saturating_mul(src_pitch_bytes)
            .saturating_add(src_x.saturating_mul(4));
        let Some(src_row) = src.get(src_row_off..src_row_off.saturating_add(copy_w * 4)) else {
            return false;
        };
        let dst_row_off = dst_y
            .saturating_add(row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_off = col_idx.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
    }

    crate::intel::dma_flush(surface.virt, byte_len);
    notify_primary_surface_present(surface, reason, byte_len)
}

pub(crate) fn present_rgba_primary_center_plane_bg(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    bg_xrgb: u32,
    reason: &str,
) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || src_pitch_bytes < src_width as usize * 4
    {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    let src_width = src_width as usize;
    let src_height = src_height as usize;
    if dst_width == 0 || dst_height == 0 || dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    let copy_w = src_width.min(dst_width);
    let copy_h = src_height.min(dst_height);
    if copy_w == 0 || copy_h == 0 {
        return false;
    }

    let src_x = src_width.saturating_sub(copy_w) / 2;
    let src_y = src_height.saturating_sub(copy_h) / 2;
    let dst_x = dst_width.saturating_sub(copy_w) / 2;
    let dst_y = dst_height.saturating_sub(copy_h) / 2;

    program_pipe_bottom_color(dev, surface.pipe, pipe_bottom_color_from_xrgb(bg_xrgb));

    for row_idx in 0..copy_h {
        let src_row_off = src_y
            .saturating_add(row_idx)
            .saturating_mul(src_pitch_bytes)
            .saturating_add(src_x.saturating_mul(4));
        let Some(src_row) = src.get(src_row_off..src_row_off.saturating_add(copy_w * 4)) else {
            return false;
        };
        let dst_row_off = row_idx.saturating_mul(dst_pitch);
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_off = col_idx.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
        crate::intel::dma_flush(unsafe { surface.virt.add(dst_row_off) }, copy_w.saturating_mul(4));
    }

    set_primary_plane_source_inner(
        PrimaryPlaneSource {
            phys: surface.phys,
            gpu: surface.gpu,
            byte_len: surface.byte_len,
            width: surface.width,
            height: surface.height,
            pitch_bytes: surface.pitch_bytes,
            format: match PRIMARY_FORMAT_PROBE_MODE {
                PRIMARY_FORMAT_PROBE_XBGR => PrimaryPlaneSourceFormat::Xbgr8888,
                _ => PrimaryPlaneSourceFormat::Xrgb8888,
            },
            src_x: 0,
            src_y: 0,
            dst_x: dst_x as u32,
            dst_y: dst_y as u32,
            dst_w: copy_w as u32,
            dst_h: copy_h as u32,
        },
        reason,
        true,
    )
}

fn present_rgba_primary_center(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || src_pitch_bytes < src_width as usize * 4
    {
        return false;
    }

    let src_width = src_width as usize;
    let src_height = src_height as usize;
    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    if dst_width == 0 || dst_height == 0 || dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    let (copy_w, copy_h) = aspect_fit_size(src_width, src_height, dst_width, dst_height);
    if copy_w == 0 || copy_h == 0 {
        return false;
    }
    let dst_x = dst_width.saturating_sub(copy_w) / 2;
    let dst_y = dst_height.saturating_sub(copy_h) / 2;

    for row_idx in 0..copy_h {
        let src_y = row_idx
            .saturating_mul(src_height)
            .checked_div(copy_h.max(1))
            .unwrap_or(0)
            .min(src_height.saturating_sub(1));
        let src_row_off = src_y.saturating_mul(src_pitch_bytes);
        let Some(src_row) = src.get(src_row_off..src_row_off + src_width.saturating_mul(4)) else {
            return false;
        };
        let dst_row_off = (dst_y + row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_x = col_idx
                .saturating_mul(src_width)
                .checked_div(copy_w.max(1))
                .unwrap_or(0)
                .min(src_width.saturating_sub(1));
            let src_off = src_x.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
    }

    let byte_len = dst_pitch.saturating_mul(dst_height);
    crate::intel::dma_flush(surface.virt, byte_len);
    notify_primary_surface_present(surface, reason, byte_len)
}

pub(crate) fn blend_rgba_primary_rect(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    src_x: u32,
    src_y: u32,
    dst_x: i32,
    dst_y: i32,
    width: u32,
    height: u32,
    reason: &str,
) -> bool {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || width == 0
        || height == 0
        || src_pitch_bytes < src_width as usize * 4
    {
        return false;
    }

    let mut sx = src_x as i64;
    let mut sy = src_y as i64;
    let mut dx = dst_x as i64;
    let mut dy = dst_y as i64;
    let mut w = width as i64;
    let mut h = height as i64;

    if dx < 0 {
        sx -= dx;
        w += dx;
        dx = 0;
    }
    if dy < 0 {
        sy -= dy;
        h += dy;
        dy = 0;
    }

    let src_max_w = src_width as i64 - sx;
    let src_max_h = src_height as i64 - sy;
    let dst_max_w = surface.width as i64 - dx;
    let dst_max_h = surface.height as i64 - dy;
    w = w.min(src_max_w).min(dst_max_w);
    h = h.min(src_max_h).min(dst_max_h);
    if sx < 0 || sy < 0 || w <= 0 || h <= 0 {
        return false;
    }

    let dst_pitch = surface.pitch_bytes as usize;
    let copy_w = w as usize;
    let copy_h = h as usize;
    let sx = sx as usize;
    let sy = sy as usize;
    let dx = dx as usize;
    let dy = dy as usize;

    for row in 0..copy_h {
        let src_off = sy
            .saturating_add(row)
            .saturating_mul(src_pitch_bytes)
            .saturating_add(sx.saturating_mul(4));
        let Some(src_row) = src.get(src_off..src_off.saturating_add(copy_w.saturating_mul(4)))
        else {
            return false;
        };
        let dst_row_off = dy
            .saturating_add(row)
            .saturating_mul(dst_pitch)
            .saturating_add(dx.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col in 0..copy_w {
            let src_px = col.saturating_mul(4);
            let sa = src_row[src_px + 3] as u32;
            if sa == 0 {
                continue;
            }
            let sr = src_row[src_px] as u32;
            let sg = src_row[src_px + 1] as u32;
            let sb = src_row[src_px + 2] as u32;
            let pixel = if sa == 0xFF {
                u32::from_le_bytes([sb as u8, sg as u8, sr as u8, 0])
            } else {
                let dst = unsafe { core::ptr::read_volatile(dst_row.add(col)) };
                let db = dst & 0xFF;
                let dg = (dst >> 8) & 0xFF;
                let dr = (dst >> 16) & 0xFF;
                let inv = 255 - sa;
                let out_r = (sr * sa + dr * inv + 127) / 255;
                let out_g = (sg * sa + dg * inv + 127) / 255;
                let out_b = (sb * sa + db * inv + 127) / 255;
                u32::from_le_bytes([out_b as u8, out_g as u8, out_r as u8, 0])
            };
            unsafe {
                core::ptr::write_volatile(dst_row.add(col), pixel);
            }
        }
        crate::intel::dma_flush(unsafe { surface.virt.add(dst_row_off) }, copy_w.saturating_mul(4));
    }

    let flush_offset = dy
        .saturating_mul(dst_pitch)
        .saturating_add(dx.saturating_mul(4));
    let flush_bytes = copy_h
        .saturating_sub(1)
        .saturating_mul(dst_pitch)
        .saturating_add(copy_w.saturating_mul(4));
    notify_primary_surface_external_write(reason, flush_offset, flush_bytes)
}

pub(crate) fn blend_rgba_primary_rect_scaled(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    src_x: u32,
    src_y: u32,
    src_w: u32,
    src_h: u32,
    dst_x: i32,
    dst_y: i32,
    dst_w: u32,
    dst_h: u32,
    reason: &str,
) -> bool {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || src_w == 0
        || src_h == 0
        || dst_w == 0
        || dst_h == 0
        || src_pitch_bytes < src_width as usize * 4
        || src_x.saturating_add(src_w) > src_width
        || src_y.saturating_add(src_h) > src_height
    {
        return false;
    }

    let mut dx = dst_x as i64;
    let mut dy = dst_y as i64;
    let mut clip_x0 = 0i64;
    let mut clip_y0 = 0i64;
    let mut copy_w = dst_w as i64;
    let mut copy_h = dst_h as i64;

    if dx < 0 {
        clip_x0 = -dx;
        copy_w += dx;
        dx = 0;
    }
    if dy < 0 {
        clip_y0 = -dy;
        copy_h += dy;
        dy = 0;
    }

    copy_w = copy_w.min(surface.width as i64 - dx);
    copy_h = copy_h.min(surface.height as i64 - dy);
    if copy_w <= 0 || copy_h <= 0 {
        return false;
    }

    let dst_pitch = surface.pitch_bytes as usize;
    let dx = dx as usize;
    let dy = dy as usize;
    let copy_w = copy_w as usize;
    let copy_h = copy_h as usize;
    let clip_x0 = clip_x0 as usize;
    let clip_y0 = clip_y0 as usize;
    let src_x = src_x as usize;
    let src_y = src_y as usize;
    let src_w = src_w as usize;
    let src_h = src_h as usize;
    let dst_w = dst_w as usize;
    let dst_h = dst_h as usize;

    for row in 0..copy_h {
        let mapped_y = src_y.saturating_add(
            (clip_y0.saturating_add(row))
                .saturating_mul(src_h)
                .checked_div(dst_h)
                .unwrap_or(0)
                .min(src_h.saturating_sub(1)),
        );
        let dst_row_off = dy
            .saturating_add(row)
            .saturating_mul(dst_pitch)
            .saturating_add(dx.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col in 0..copy_w {
            let mapped_x = src_x.saturating_add(
                (clip_x0.saturating_add(col))
                    .saturating_mul(src_w)
                    .checked_div(dst_w)
                    .unwrap_or(0)
                    .min(src_w.saturating_sub(1)),
            );
            let src_off = mapped_y
                .saturating_mul(src_pitch_bytes)
                .saturating_add(mapped_x.saturating_mul(4));
            let Some(src_px) = src.get(src_off..src_off.saturating_add(4)) else {
                return false;
            };
            let sa = src_px[3] as u32;
            if sa == 0 {
                continue;
            }
            let sr = src_px[0] as u32;
            let sg = src_px[1] as u32;
            let sb = src_px[2] as u32;
            let pixel = if sa == 0xFF {
                u32::from_le_bytes([sb as u8, sg as u8, sr as u8, 0])
            } else {
                let dst = unsafe { core::ptr::read_volatile(dst_row.add(col)) };
                let db = dst & 0xFF;
                let dg = (dst >> 8) & 0xFF;
                let dr = (dst >> 16) & 0xFF;
                let inv = 255 - sa;
                let out_r = (sr * sa + dr * inv + 127) / 255;
                let out_g = (sg * sa + dg * inv + 127) / 255;
                let out_b = (sb * sa + db * inv + 127) / 255;
                u32::from_le_bytes([out_b as u8, out_g as u8, out_r as u8, 0])
            };
            unsafe {
                core::ptr::write_volatile(dst_row.add(col), pixel);
            }
        }
        crate::intel::dma_flush(unsafe { surface.virt.add(dst_row_off) }, copy_w.saturating_mul(4));
    }

    let flush_offset = dy
        .saturating_mul(dst_pitch)
        .saturating_add(dx.saturating_mul(4));
    let flush_bytes = copy_h
        .saturating_sub(1)
        .saturating_mul(dst_pitch)
        .saturating_add(copy_w.saturating_mul(4));
    notify_primary_surface_external_write(reason, flush_offset, flush_bytes)
}

pub(crate) fn present_rgba_primary_rot180(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || src_pitch_bytes < src_width as usize * 4
    {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    let copy_w = (src_width as usize).min(dst_width);
    let copy_h = (src_height as usize).min(dst_height);
    if copy_w == 0 || copy_h == 0 || dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    for row_idx in 0..copy_h {
        let src_y = copy_h.saturating_sub(1).saturating_sub(row_idx);
        let src_row_off = src_y.saturating_mul(src_pitch_bytes);
        let Some(src_row) = src.get(src_row_off..src_row_off + copy_w.saturating_mul(4)) else {
            return false;
        };
        let dst_row_off = row_idx.saturating_mul(dst_pitch);
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_x = copy_w.saturating_sub(1).saturating_sub(col_idx);
            let src_off = src_x.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
        crate::intel::dma_flush(unsafe { surface.virt.add(dst_row_off) }, copy_w.saturating_mul(4));
    }

    let byte_len = dst_pitch.saturating_mul(dst_height);
    notify_primary_surface_present(surface, reason, byte_len)
}

pub(crate) fn present_rgba_primary_flip_y(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || src_pitch_bytes < src_width as usize * 4
    {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    let copy_w = (src_width as usize).min(dst_width);
    let copy_h = (src_height as usize).min(dst_height);
    if copy_w == 0 || copy_h == 0 || dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    for row_idx in 0..copy_h {
        let src_y = copy_h.saturating_sub(1).saturating_sub(row_idx);
        let src_row_off = src_y.saturating_mul(src_pitch_bytes);
        let Some(src_row) = src.get(src_row_off..src_row_off + copy_w.saturating_mul(4)) else {
            return false;
        };
        let dst_row_off = row_idx.saturating_mul(dst_pitch);
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_off = col_idx.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
        crate::intel::dma_flush(unsafe { surface.virt.add(dst_row_off) }, copy_w.saturating_mul(4));
    }

    let byte_len = dst_pitch.saturating_mul(dst_height);
    notify_primary_surface_present(surface, reason, byte_len)
}

pub(crate) fn present_rgba_primary_top_right(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
) -> bool {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || src_pitch_bytes < src_width as usize * 4
    {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    let copy_w = (src_width as usize).min(dst_width);
    let copy_h = (src_height as usize).min(dst_height);
    if copy_w == 0 || copy_h == 0 || dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    let dst_x = dst_width.saturating_sub(copy_w);
    let dst_y = 0usize;
    for row_idx in 0..copy_h {
        let src_row_off = row_idx.saturating_mul(src_pitch_bytes);
        let Some(src_row) = src.get(src_row_off..src_row_off + copy_w.saturating_mul(4)) else {
            return false;
        };
        let dst_row_off = (dst_y + row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_off = col_idx.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
        crate::intel::dma_flush(unsafe { surface.virt.add(dst_row_off) }, copy_w.saturating_mul(4));
    }

    let byte_len = dst_pitch.saturating_mul(dst_height);
    notify_primary_surface_present(surface, "rgba-primary-top-right", byte_len)
}

pub(crate) fn present_rgba_overlay_top_right(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
) -> bool {
    present_rgba_overlay(src, src_width, src_height, src_pitch_bytes, None, false, "camera-overlay")
}

pub(crate) fn present_rgba_overlay_at(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    x: u32,
    y: u32,
    preserve_alpha: bool,
    reason: &str,
) -> bool {
    present_rgba_overlay(
        src,
        src_width,
        src_height,
        src_pitch_bytes,
        Some((x, y)),
        preserve_alpha,
        reason,
    )
}

pub(crate) fn present_live_overlay_rects(rects: &[LiveOverlayRect], reason: &str) -> bool {
    present_live_overlay_rects_preserving(rects, None, reason)
}

pub(crate) fn present_live_overlay_rects_preserving(
    rects: &[LiveOverlayRect],
    preserve: Option<LiveOverlayRect>,
    reason: &str,
) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let (width, height) = active_scanout_dimensions()
        .or_else(|| {
            PRIMARY_SURFACE
                .lock()
                .as_ref()
                .map(|primary| (primary.width, primary.height))
        })
        .unwrap_or((0, 0));
    if width == 0 || height == 0 {
        return false;
    }
    let Some(surface) = ensure_overlay_surface(dev, width, height) else {
        return false;
    };

    if let Some(rect) = preserve {
        let _ = copy_overlay_front_into_back(surface);
        clear_overlay_except_rect(surface, rect);
    } else {
        fill_surface_color(
            surface.virt,
            surface.pitch_bytes as usize,
            surface.width,
            surface.height,
            0,
        );
    }
    for rect in rects {
        fill_overlay_rect_rgba(surface, *rect);
    }

    let byte_len = surface.byte_len;
    crate::intel::dma_flush(surface.virt, byte_len);

    if overlay_plane_needs_rearm(dev, surface, 0, 0, OverlayAlphaMode::Straight) {
        program_three_plane_stack_resources(dev, surface.pipe, reason);
        if !arm_overlay_plane(dev, surface, 0, 0, OverlayAlphaMode::Straight, reason) {
            return false;
        }
    }

    let seq = OVERLAY_PRESENT_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if seq <= 8 || seq.is_multiple_of(60) {
        let plane_base = overlay_plane_base(surface.pipe, surface.plane_slot);
        crate::log!(
            "intel/display: live-overlay-present seq={} reason={} pipe={} slot={} rects={} size={}x{} pitch=0x{:X} surf=0x{:08X} surf_live=0x{:08X}\n",
            seq,
            reason,
            surface.pipe.name,
            surface.plane_slot,
            rects.len(),
            surface.width,
            surface.height,
            surface.pitch_bytes,
            crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF),
            crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF),
        );
    }

    true
}

pub(crate) fn present_ui3_canvas_rgba(
    rect: LiveOverlayRect,
    src: *mut u8,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let (width, height) = active_scanout_dimensions()
        .or_else(|| {
            PRIMARY_SURFACE
                .lock()
                .as_ref()
                .map(|primary| (primary.width, primary.height))
        })
        .unwrap_or((0, 0));
    if width == 0
        || height == 0
        || rect.width == 0
        || rect.height == 0
        || src.is_null()
        || src_pitch_bytes < rect.width as usize * 4
    {
        return false;
    }

    let Some(surface) = ensure_overlay_surface(dev, width, height) else {
        return false;
    };
    let x0 = rect.x.min(surface.width);
    let y0 = rect.y.min(surface.height);
    let copy_w = rect.width.min(surface.width.saturating_sub(x0));
    let copy_h = rect.height.min(surface.height.saturating_sub(y0));
    let dst_pitch = surface.pitch_bytes as usize;
    if copy_w == 0 || copy_h == 0 || dst_pitch < surface.width as usize * 4 {
        return false;
    }

    if !live_rect_covers_surface(rect, surface) {
        let _ = copy_overlay_front_into_back(surface);
    }

    crate::intel::dma_flush(src, src_pitch_bytes.saturating_mul(copy_h as usize));
    for row_idx in 0..copy_h as usize {
        let src_row = unsafe { src.add(row_idx.saturating_mul(src_pitch_bytes)) as *const u32 };
        let dst_row = unsafe {
            (surface.virt as *mut u32)
                .add((y0 as usize + row_idx).saturating_mul(dst_pitch / 4) + x0 as usize)
        };
        for col_idx in 0..copy_w as usize {
            unsafe {
                core::ptr::write_volatile(
                    dst_row.add(col_idx),
                    core::ptr::read_volatile(src_row.add(col_idx)),
                );
            }
        }
    }

    let byte_len = surface.byte_len;
    crate::intel::dma_flush(surface.virt, byte_len);

    if overlay_plane_needs_rearm(dev, surface, 0, 0, OverlayAlphaMode::Straight) {
        program_three_plane_stack_resources(dev, surface.pipe, reason);
        if !arm_overlay_plane(dev, surface, 0, 0, OverlayAlphaMode::Straight, reason) {
            return false;
        }
    }

    true
}

fn clear_overlay_except_rect(surface: OverlaySurface, rect: LiveOverlayRect) {
    let x0 = rect.x.min(surface.width);
    let y0 = rect.y.min(surface.height);
    let x1 = x0.saturating_add(rect.width).min(surface.width);
    let y1 = y0.saturating_add(rect.height).min(surface.height);
    if x0 >= x1 || y0 >= y1 {
        fill_surface_color(
            surface.virt,
            surface.pitch_bytes as usize,
            surface.width,
            surface.height,
            0,
        );
        return;
    }

    fill_overlay_rect(surface, 0, 0, surface.width, y0, 0);
    fill_overlay_rect(surface, 0, y1, surface.width, surface.height.saturating_sub(y1), 0);
    fill_overlay_rect(surface, 0, y0, x0, y1.saturating_sub(y0), 0);
    fill_overlay_rect(surface, x1, y0, surface.width.saturating_sub(x1), y1.saturating_sub(y0), 0);
}

fn present_rgba_overlay(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    position: Option<(u32, u32)>,
    preserve_alpha: bool,
    reason: &str,
) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    if src_width == 0 || src_height == 0 || src_pitch_bytes < src_width as usize * 4 {
        return false;
    }

    let Some(surface) = ensure_overlay_surface(dev, src_width, src_height) else {
        return false;
    };
    let alpha = if preserve_alpha {
        OverlayAlphaMode::Straight
    } else {
        OverlayAlphaMode::Opaque
    };

    if !copy_rgba_into_overlay(surface, src, src_width, src_height, src_pitch_bytes, alpha) {
        return false;
    }
    if reason == "gfx-full-scene-alpha-overlay" {
        stamp_overlay_composition_proof_marker(surface, alpha, reason);
    }

    let byte_len = surface.byte_len;
    crate::intel::dma_flush(surface.virt, byte_len);

    let (pos_x, pos_y) = position
        .map(|(x, y)| overlay_plane_clamped_position(surface, x, y))
        .unwrap_or_else(|| overlay_plane_top_right_position(surface));
    if overlay_plane_needs_rearm(dev, surface, pos_x, pos_y, alpha) {
        program_three_plane_stack_resources(dev, surface.pipe, reason);
        if !arm_overlay_plane(dev, surface, pos_x, pos_y, alpha, reason) {
            return false;
        }
    }

    let seq = OVERLAY_PRESENT_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if seq <= 8 || seq.is_multiple_of(60) {
        log_primary_surface_samples("under-overlay-present");
        log_pipe_live_scanout_state("overlay-present");
        log_display_power_well_snapshot("overlay-present");
        let plane_base = overlay_plane_base(surface.pipe, surface.plane_slot);
        crate::log!(
            "intel/display: overlay-present seq={} reason={} pipe={} slot={} alpha={:?} pos={}x{} size={}x{} pitch=0x{:X} gpu=0x{:X} phys=0x{:X} surf=0x{:08X} surf_live=0x{:08X}\n",
            seq,
            reason,
            surface.pipe.name,
            surface.plane_slot,
            alpha,
            pos_x,
            pos_y,
            surface.width,
            surface.height,
            surface.pitch_bytes,
            surface.gpu,
            surface.phys,
            crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF),
            crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF)
        );
    }

    true
}

#[inline]
fn clamp_u8_i32(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

fn aspect_fit_size(
    src_width: usize,
    src_height: usize,
    dst_width: usize,
    dst_height: usize,
) -> (usize, usize) {
    if src_width == 0 || src_height == 0 || dst_width == 0 || dst_height == 0 {
        return (0, 0);
    }
    if src_width <= dst_width && src_height <= dst_height {
        return (src_width, src_height);
    }
    if dst_width.saturating_mul(src_height) <= dst_height.saturating_mul(src_width) {
        let copy_w = dst_width.max(1);
        let copy_h = src_height
            .saturating_mul(copy_w)
            .checked_div(src_width)
            .unwrap_or(1)
            .max(1)
            .min(dst_height);
        (copy_w, copy_h)
    } else {
        let copy_h = dst_height.max(1);
        let copy_w = src_width
            .saturating_mul(copy_h)
            .checked_div(src_height)
            .unwrap_or(1)
            .max(1)
            .min(dst_width);
        (copy_w, copy_h)
    }
}

fn center_crop_size(
    src_width: usize,
    src_height: usize,
    dst_width: usize,
    dst_height: usize,
) -> (usize, usize) {
    if src_width == 0 || src_height == 0 || dst_width == 0 || dst_height == 0 {
        return (0, 0);
    }

    (src_width.min(dst_width), src_height.min(dst_height))
}

#[inline(always)]
fn media_ytile_8bpp_offset(byte_x: usize, row_y: usize, tiles_per_row: usize) -> usize {
    const YTILE_W: usize = 128;
    const YTILE_H: usize = 32;

    let tile_col = byte_x / YTILE_W;
    let tile_row = row_y / YTILE_H;
    let in_x = byte_x % YTILE_W;
    let in_y = row_y % YTILE_H;
    let oword_col = in_x / 16;
    let byte_in_oword = in_x % 16;
    let within_tile = oword_col * 512 + in_y * 16 + byte_in_oword;
    (tile_row * tiles_per_row + tile_col) * 4096 + within_tile
}

#[inline(always)]
fn nv12_pixel_to_bgra(y: i32, u: i32, v: i32) -> u32 {
    let c = (y - 16).max(0);
    let u = u - 128;
    let v = v - 128;
    let r = clamp_u8_i32((298 * c + 409 * v + 128) >> 8);
    let g = clamp_u8_i32((298 * c - 100 * u - 208 * v + 128) >> 8);
    let b = clamp_u8_i32((298 * c + 516 * u + 128) >> 8);
    u32::from_le_bytes([b, g, r, 0])
}

fn present_ytile_nv12_surface_center_1to1(
    surface: PrimarySurface,
    src: &[u8],
    visible_x: usize,
    visible_y: usize,
    visible_width: usize,
    visible_height: usize,
    tiles_per_row: usize,
    chroma_y_offset: usize,
    dst_x: usize,
    dst_y: usize,
) {
    let dst_pitch = surface.pitch_bytes as usize;
    for row_idx in 0..visible_height {
        let src_y = visible_y + row_idx;
        let uv_row = chroma_y_offset + src_y / 2;
        let dst_row_off = (dst_y + row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..visible_width {
            let src_x = visible_x + col_idx;
            let y_off = media_ytile_8bpp_offset(src_x, src_y, tiles_per_row);
            let uv_x = (src_x / 2).saturating_mul(2);
            let u_off = media_ytile_8bpp_offset(uv_x, uv_row, tiles_per_row);
            let v_off = media_ytile_8bpp_offset(uv_x + 1, uv_row, tiles_per_row);
            let pixel = nv12_pixel_to_bgra(
                unsafe { i32::from(*src.get_unchecked(y_off)) },
                unsafe { i32::from(*src.get_unchecked(u_off)) },
                unsafe { i32::from(*src.get_unchecked(v_off)) },
            );
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
    }
}

fn dma_flush_primary_rect(
    surface: PrimarySurface,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
) -> usize {
    let dst_pitch = surface.pitch_bytes as usize;
    let row_bytes = width.saturating_mul(4);
    if row_bytes == 0 || height == 0 {
        return 0;
    }
    for row_idx in 0..height {
        let row_off = y
            .saturating_add(row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(x.saturating_mul(4));
        unsafe {
            crate::intel::dma_flush(surface.virt.add(row_off), row_bytes);
        }
    }
    row_bytes.saturating_mul(height)
}

pub(crate) fn present_imc3_surface_center(
    src: &[u8],
    coded_width: u32,
    coded_height: u32,
    visible_x: u32,
    visible_y: u32,
    visible_width: u32,
    visible_height: u32,
    src_pitch_bytes: usize,
) -> bool {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    if surface.virt.is_null() || coded_width == 0 || coded_height == 0 {
        return false;
    }

    let coded_width = coded_width as usize;
    let coded_height = coded_height as usize;
    let visible_x = visible_x as usize;
    let visible_y = visible_y as usize;
    let visible_width = visible_width as usize;
    let visible_height = visible_height as usize;
    if src_pitch_bytes < coded_width || visible_width == 0 || visible_height == 0 {
        return false;
    }
    if visible_x.saturating_add(visible_width) > coded_width
        || visible_y.saturating_add(visible_height) > coded_height
    {
        return false;
    }

    const YTILE_W: usize = 128;
    const YTILE_H: usize = 32;
    let tiles_per_row = src_pitch_bytes / YTILE_W;
    if tiles_per_row == 0 {
        return false;
    }
    let chroma_y_offset = (coded_height + YTILE_H - 1) & !(YTILE_H - 1);
    let chroma_plane_rows = coded_height.div_ceil(2);
    let chroma_plane_stride_rows = (chroma_plane_rows + YTILE_H - 1) & !(YTILE_H - 1);
    let cr_y_offset = chroma_y_offset + chroma_plane_stride_rows;
    let total_height = cr_y_offset + chroma_plane_rows;
    let total_tile_rows = (total_height + YTILE_H - 1) / YTILE_H;
    let needed = total_tile_rows * tiles_per_row * 4096;
    if src.len() < needed {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    if dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    let (copy_w, copy_h) = aspect_fit_size(visible_width, visible_height, dst_width, dst_height);
    if copy_w == 0 || copy_h == 0 {
        return false;
    }
    let dst_x = dst_width.saturating_sub(copy_w) / 2;
    let dst_y = dst_height.saturating_sub(copy_h) / 2;

    #[inline(always)]
    fn ytile_offset(byte_x: usize, row_y: usize, tiles_per_row: usize) -> usize {
        let tile_col = byte_x / YTILE_W;
        let tile_row = row_y / YTILE_H;
        let in_x = byte_x % YTILE_W;
        let in_y = row_y % YTILE_H;
        let oword_col = in_x / 16;
        let byte_in_oword = in_x % 16;
        let within_tile = oword_col * 512 + in_y * 16 + byte_in_oword;
        (tile_row * tiles_per_row + tile_col) * 4096 + within_tile
    }

    for row_idx in 0..copy_h {
        let src_y = visible_y.saturating_add(
            row_idx
                .saturating_mul(visible_height)
                .checked_div(copy_h.max(1))
                .unwrap_or(0)
                .min(visible_height.saturating_sub(1)),
        );
        let dst_row_off = (dst_y + row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        let cb_row = chroma_y_offset + src_y / 2;
        let cr_row = cr_y_offset + src_y / 2;
        for col_idx in 0..copy_w {
            let src_x = visible_x.saturating_add(
                col_idx
                    .saturating_mul(visible_width)
                    .checked_div(copy_w.max(1))
                    .unwrap_or(0)
                    .min(visible_width.saturating_sub(1)),
            );
            let y_off = ytile_offset(src_x, src_y, tiles_per_row);
            let chroma_x = src_x / 2;
            let cb_off = ytile_offset(chroma_x, cb_row, tiles_per_row);
            let cr_off = ytile_offset(chroma_x, cr_row, tiles_per_row);
            let y = unsafe { i32::from(*src.get_unchecked(y_off)) };
            let c = (y - 16).max(0);
            let u = unsafe { i32::from(*src.get_unchecked(cb_off)) } - 128;
            let v = unsafe { i32::from(*src.get_unchecked(cr_off)) } - 128;
            let r = clamp_u8_i32((298 * c + 409 * v + 128) >> 8);
            let g = clamp_u8_i32((298 * c - 100 * u - 208 * v + 128) >> 8);
            let b = clamp_u8_i32((298 * c + 516 * u + 128) >> 8);
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
    }

    let byte_len = dst_pitch.saturating_mul(dst_height);
    crate::intel::dma_flush(surface.virt, byte_len);
    notify_primary_surface_present(surface, "hw-logo-imc3-center", byte_len);
    true
}

pub(crate) fn present_ytile_nv12_surface_center(
    src: &[u8],
    coded_width: u32,
    coded_height: u32,
    visible_x: u32,
    visible_y: u32,
    visible_width: u32,
    visible_height: u32,
    src_pitch_bytes: usize,
    src_uv_offset: usize,
) -> bool {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    if surface.virt.is_null() || coded_width == 0 || coded_height == 0 {
        return false;
    }

    const YTILE_W: usize = 128;
    const YTILE_H: usize = 32;

    let coded_width = coded_width as usize;
    let coded_height = coded_height as usize;
    let visible_x = visible_x as usize;
    let visible_y = visible_y as usize;
    let visible_width = visible_width as usize;
    let visible_height = visible_height as usize;
    if src_pitch_bytes < coded_width
        || !src_pitch_bytes.is_multiple_of(YTILE_W)
        || visible_width == 0
        || visible_height == 0
    {
        return false;
    }
    if visible_x.saturating_add(visible_width) > coded_width
        || visible_y.saturating_add(visible_height) > coded_height
    {
        return false;
    }

    let tiles_per_row = src_pitch_bytes / YTILE_W;
    if tiles_per_row == 0 {
        return false;
    }
    if src_uv_offset < src_pitch_bytes.saturating_mul(coded_height)
        || src_uv_offset % src_pitch_bytes != 0
    {
        return false;
    }
    let chroma_y_offset = src_uv_offset / src_pitch_bytes;
    let total_height = chroma_y_offset.saturating_add(coded_height.div_ceil(2));
    let needed = total_height
        .div_ceil(YTILE_H)
        .saturating_mul(tiles_per_row)
        .saturating_mul(4096);
    if src.len() < needed {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    if dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    let (copy_w, copy_h) = aspect_fit_size(visible_width, visible_height, dst_width, dst_height);
    if copy_w == 0 || copy_h == 0 {
        return false;
    }
    let dst_x = dst_width.saturating_sub(copy_w) / 2;
    let dst_y = dst_height.saturating_sub(copy_h) / 2;

    if copy_w == visible_width && copy_h == visible_height {
        present_ytile_nv12_surface_center_1to1(
            surface,
            src,
            visible_x,
            visible_y,
            visible_width,
            visible_height,
            tiles_per_row,
            chroma_y_offset,
            dst_x,
            dst_y,
        );
        let byte_len = dma_flush_primary_rect(surface, dst_x, dst_y, copy_w, copy_h);
        notify_primary_surface_present(surface, "ytile-nv12-center-1to1", byte_len);
        return true;
    }

    for row_idx in 0..copy_h {
        let src_y = visible_y.saturating_add(
            row_idx
                .saturating_mul(visible_height)
                .checked_div(copy_h.max(1))
                .unwrap_or(0)
                .min(visible_height.saturating_sub(1)),
        );
        let dst_row_off = (dst_y + row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_x = visible_x.saturating_add(
                col_idx
                    .saturating_mul(visible_width)
                    .checked_div(copy_w.max(1))
                    .unwrap_or(0)
                    .min(visible_width.saturating_sub(1)),
            );
            let y_off = media_ytile_8bpp_offset(src_x, src_y, tiles_per_row);
            let uv_x = (src_x / 2).saturating_mul(2);
            let uv_row = chroma_y_offset.saturating_add(src_y / 2);
            let u_off = media_ytile_8bpp_offset(uv_x, uv_row, tiles_per_row);
            let v_off = media_ytile_8bpp_offset(uv_x + 1, uv_row, tiles_per_row);
            let pixel = nv12_pixel_to_bgra(
                unsafe { i32::from(*src.get_unchecked(y_off)) },
                unsafe { i32::from(*src.get_unchecked(u_off)) },
                unsafe { i32::from(*src.get_unchecked(v_off)) },
            );
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
    }

    let byte_len = dst_pitch.saturating_mul(dst_height);
    crate::intel::dma_flush(surface.virt, byte_len);
    notify_primary_surface_present(surface, "ytile-nv12-center", byte_len);
    true
}

pub(crate) fn present_nv12_surface_center(
    src: &[u8],
    coded_width: u32,
    coded_height: u32,
    visible_x: u32,
    visible_y: u32,
    visible_width: u32,
    visible_height: u32,
    src_pitch_bytes: usize,
) -> bool {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    if surface.virt.is_null() || coded_width == 0 || coded_height == 0 {
        return false;
    }

    let coded_width = coded_width as usize;
    let coded_height = coded_height as usize;
    let visible_x = visible_x as usize;
    let visible_y = visible_y as usize;
    let visible_width = visible_width as usize;
    let visible_height = visible_height as usize;
    if src_pitch_bytes < coded_width || visible_width == 0 || visible_height == 0 {
        return false;
    }
    if visible_x.saturating_add(visible_width) > coded_width
        || visible_y.saturating_add(visible_height) > coded_height
    {
        return false;
    }

    if !src_pitch_bytes.is_multiple_of(super::xelp_media2_ngin::MEDIA_TILE64_W) {
        return false;
    }
    let tiles_per_row = src_pitch_bytes / super::xelp_media2_ngin::MEDIA_TILE64_W;
    if tiles_per_row == 0 {
        return false;
    }
    let Some((chroma_y_offset, needed)) =
        super::xelp_media2_ngin::media_tile64_nv12_surface_layout(coded_height, src_pitch_bytes)
    else {
        return false;
    };
    if src.len() < needed {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    if dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    let (copy_w, copy_h) = aspect_fit_size(visible_width, visible_height, dst_width, dst_height);
    if copy_w == 0 || copy_h == 0 {
        return false;
    }
    let dst_x = dst_width.saturating_sub(copy_w) / 2;
    let dst_y = dst_height.saturating_sub(copy_h) / 2;

    for row_idx in 0..copy_h {
        let src_y = visible_y.saturating_add(
            row_idx
                .saturating_mul(visible_height)
                .checked_div(copy_h.max(1))
                .unwrap_or(0)
                .min(visible_height.saturating_sub(1)),
        );
        let dst_row_off = (dst_y + row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_x = visible_x.saturating_add(
                col_idx
                    .saturating_mul(visible_width)
                    .checked_div(copy_w.max(1))
                    .unwrap_or(0)
                    .min(visible_width.saturating_sub(1)),
            );
            let y_off =
                super::xelp_media2_ngin::media_tile64_8bpp_offset(src_x, src_y, tiles_per_row);
            let uv_x = (src_x / 2).saturating_mul(2);
            let uv_row = chroma_y_offset.saturating_add(src_y / 2);
            let u_off =
                super::xelp_media2_ngin::media_tile64_8bpp_offset(uv_x, uv_row, tiles_per_row);
            let v_off =
                super::xelp_media2_ngin::media_tile64_8bpp_offset(uv_x + 1, uv_row, tiles_per_row);
            let y = unsafe { i32::from(*src.get_unchecked(y_off)) };
            let c = (y - 16).max(0);
            let u = unsafe { i32::from(*src.get_unchecked(u_off)) } - 128;
            let v = unsafe { i32::from(*src.get_unchecked(v_off)) } - 128;
            let (r, g, b) =
                if VIDEO_NV12_BLACK_PROOF_LIFT && y <= 24 && u.abs() <= 4 && v.abs() <= 4 {
                    let checker = ((row_idx >> 5) ^ (col_idx >> 5)) & 1;
                    if checker == 0 {
                        (0x30, 0x58, 0xD0)
                    } else {
                        (0x70, 0x20, 0xA0)
                    }
                } else {
                    (
                        clamp_u8_i32((298 * c + 409 * v + 128) >> 8),
                        clamp_u8_i32((298 * c - 100 * u - 208 * v + 128) >> 8),
                        clamp_u8_i32((298 * c + 516 * u + 128) >> 8),
                    )
                };
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
    }

    let byte_len = dst_pitch.saturating_mul(dst_height);
    crate::intel::dma_flush(surface.virt, byte_len);
    notify_primary_surface_present(surface, "nv12-center", byte_len);
    true
}

fn notify_primary_surface_present(surface: PrimarySurface, reason: &str, byte_len: usize) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(surface_reg) = u32::try_from(surface.gpu).ok() else {
        return false;
    };

    let seq = PRIMARY_PRESENT_SEQ.fetch_add(1, Ordering::AcqRel) + 1;

    let pipeconf_off = PIPECONF_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let trans_ddi_func_ctl_off =
        TRANS_DDI_FUNC_CTL_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let trans_psr_ctl_off = TRANS_PSR_CTL_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let trans_psr_status_off =
        TRANS_PSR_STATUS_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let trans_psr2_ctl_off = TRANS_PSR2_CTL_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let trans_psr2_status_off =
        TRANS_PSR2_STATUS_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let cur_surflive_off = CUR_SURFLIVE_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);

    probe_primary_present_psr(dev, surface, reason, seq);

    // Fast path: if the plane is already active (seq > 1) and PLANE_SURF
    // already points to our surface with matching geometry, skip MMIO writes
    // and vblank waits.
    // The display scanner is already reading from this address; new pixel
    // data is visible as soon as the CPU cache flush completes.
    if seq > 1 {
        let surf_current = crate::intel::mmio_read(dev, surface.pipe.primary_plane().surf());
        let regs_match = surf_current == surface_reg
            && primary_plane_surface_regs_match(dev, surface, surface_reg);
        if regs_match {
            if should_log_primary_present(seq) {
                intel_display_verbose_log!(
                    "intel/display: primary-flip seq={} reason={} pipe={} surf=0x{:08X} fast-skip\n",
                    seq,
                    reason,
                    surface.pipe.name,
                    surface_reg,
                );
            }
            return true;
        }

        let stride_before = crate::intel::mmio_read(dev, surface.pipe.primary_plane().stride());
        let size_before =
            crate::intel::mmio_read(dev, surface.pipe.primary_plane().base() + UNI_PLANE_SIZE_OFF);
        let pos_before =
            crate::intel::mmio_read(dev, surface.pipe.primary_plane().base() + UNI_PLANE_POS_OFF);
        let offset_before = crate::intel::mmio_read(
            dev,
            surface.pipe.primary_plane().base() + UNI_PLANE_OFFSET_OFF,
        );
        let ctl_before = crate::intel::mmio_read(dev, surface.pipe.primary_plane().ctl());
        let (_, _, surf_live_after, iter) = program_primary_plane_and_wait(
            dev,
            surface.pipe,
            surface.width,
            surface.height,
            surface.pitch_bytes,
            surface_reg,
            reason,
        );
        let surf_after = crate::intel::mmio_read(dev, surface.pipe.primary_plane().surf());
        if should_log_primary_present(seq) {
            intel_display_verbose_log!(
                "intel/display: primary-flip seq={} reason={} pipe={} rearm=1 regs_match={} surf=0x{:08X}=>0x{:08X} surf_live=0x{:08X} stride_before=0x{:08X} size_before=0x{:08X} pos_before=0x{:08X} offset_before=0x{:08X} ctl_before=0x{:08X} iter={}\n",
                seq,
                reason,
                surface.pipe.name,
                regs_match as u8,
                surf_current,
                surf_after,
                surf_live_after,
                stride_before,
                size_before,
                pos_before,
                offset_before,
                ctl_before,
                iter,
            );
        }
        return surf_after == surface_reg || surf_live_after == surface_reg;
    }

    let surf_before = crate::intel::mmio_read(dev, surface.pipe.primary_plane().surf());
    let surf_live_before = crate::intel::mmio_read(dev, surface.pipe.primary_plane().surf_live());
    let cur_surflive_before = crate::intel::mmio_read(dev, cur_surflive_off);
    let (frame_before, frame_after, frame_iters) = wait_for_pipe_next_frame(dev, surface.pipe);
    crate::intel::mmio_write(dev, cur_surflive_off, 0);
    let cur_surflive_after = crate::intel::mmio_read(dev, cur_surflive_off);

    let (_, _, surf_live_after, iter) = program_primary_plane_and_wait(
        dev,
        surface.pipe,
        surface.width,
        surface.height,
        surface.pitch_bytes,
        surface_reg,
        reason,
    );
    let surf_after = crate::intel::mmio_read(dev, surface.pipe.primary_plane().surf());

    if should_log_primary_present(seq) {
        intel_display_verbose_log!(
            "intel/display: primary-present seq={} reason={} pipe={} bytes=0x{:X} pipeconf=0x{:08X} ddi_func_ctl=0x{:08X} psr_ctl=0x{:08X} psr_status=0x{:08X} psr2_ctl=0x{:08X} psr2_status=0x{:08X} frame={}=>{} frame_wait={} cur_surflive_before=0x{:08X} cur_surflive_after=0x{:08X} surf_before=0x{:08X} surf_after=0x{:08X} surf_live_before=0x{:08X} surf_live_after=0x{:08X} iter={}\n",
            seq,
            reason,
            surface.pipe.name,
            byte_len,
            crate::intel::mmio_read(dev, pipeconf_off),
            crate::intel::mmio_read(dev, trans_ddi_func_ctl_off),
            crate::intel::mmio_read(dev, trans_psr_ctl_off),
            crate::intel::mmio_read(dev, trans_psr_status_off),
            crate::intel::mmio_read(dev, trans_psr2_ctl_off),
            crate::intel::mmio_read(dev, trans_psr2_status_off),
            frame_before,
            frame_after,
            frame_iters,
            cur_surflive_before,
            cur_surflive_after,
            surf_before,
            surf_after,
            surf_live_before,
            surf_live_after,
            iter
        );
    }

    true
}

fn primary_plane_surface_regs_match(
    dev: crate::intel::Dev,
    surface: PrimarySurface,
    surface_reg: u32,
) -> bool {
    let Some(stride_reg) = plane_stride_reg_value(surface.pitch_bytes) else {
        return false;
    };
    let ctl = crate::intel::mmio_read(dev, surface.pipe.primary_plane().ctl());
    let ctl_expected = primary_plane_ctl_enabled(ctl);
    let ctl_match_mask = PLANE_CTL_ENABLE
        | PLANE_CTL_ARB_SLOTS_MASK
        | PLANE_CTL_FORMAT_MASK_SKL
        | PLANE_CTL_KEY_ENABLE_MASK
        | PLANE_CTL_TILED_MASK
        | PLANE_CTL_ORDER_RGBX;
    crate::intel::mmio_read(dev, surface.pipe.primary_plane().surf()) == surface_reg
        && crate::intel::mmio_read(dev, surface.pipe.primary_plane().stride()) == stride_reg
        && crate::intel::mmio_read(dev, surface.pipe.primary_plane().base() + UNI_PLANE_POS_OFF)
            == plane_pos_reg_value(0, 0)
        && crate::intel::mmio_read(dev, surface.pipe.primary_plane().base() + UNI_PLANE_SIZE_OFF)
            == plane_size_reg_value(surface.width, surface.height)
        && crate::intel::mmio_read(dev, surface.pipe.primary_plane().base() + UNI_PLANE_OFFSET_OFF)
            == plane_pos_reg_value(0, 0)
        && (ctl & ctl_match_mask) == (ctl_expected & ctl_match_mask)
}

#[inline]
fn should_log_primary_present(seq: u32) -> bool {
    if crate::logflag::INTEL_STAGE1_LOGS {
        return false;
    }
    seq <= 8 || seq.is_multiple_of(60)
}

fn wait_for_pipe_next_frame(dev: crate::intel::Dev, pipe: PipeInfo) -> (u32, u32, usize) {
    let frame_off = PIPE_FRMCOUNT_A + pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let before = crate::intel::mmio_read(dev, frame_off);
    let mut after = before;
    let mut iter = 0usize;
    while iter < 200_000 && after == before {
        core::hint::spin_loop();
        after = crate::intel::mmio_read(dev, frame_off);
        iter += 1;
    }
    (before, after, iter)
}

fn wait_for_primary_plane_live(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    want_live: u32,
    max_iters: usize,
) -> (u32, usize) {
    let mut live = crate::intel::mmio_read(dev, pipe.primary_plane().surf_live());
    let mut iter = 0usize;
    while iter < max_iters && live != want_live {
        core::hint::spin_loop();
        live = crate::intel::mmio_read(dev, pipe.primary_plane().surf_live());
        iter += 1;
    }
    (live, iter)
}

fn wait_for_plane_live(
    dev: crate::intel::Dev,
    plane_base: usize,
    want_live: u32,
    max_iters: usize,
) -> (u32, usize) {
    let mut live = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);
    let mut iter = 0usize;
    while iter < max_iters && live != want_live {
        core::hint::spin_loop();
        live = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);
        iter += 1;
    }
    (live, iter)
}

fn primary_plane_ctl_enabled(ctl_before: u32) -> u32 {
    let format = match PRIMARY_FORMAT_PROBE_MODE {
        PRIMARY_FORMAT_PROBE_XBGR => PrimaryPlaneSourceFormat::Xbgr8888,
        _ => PrimaryPlaneSourceFormat::Xrgb8888,
    };
    primary_plane_ctl_enabled_for_format(ctl_before, format)
}

fn primary_plane_ctl_enabled_for_format(ctl_before: u32, format: PrimaryPlaneSourceFormat) -> u32 {
    let order_bits = match format {
        PrimaryPlaneSourceFormat::Xrgb8888 => 0,
        PrimaryPlaneSourceFormat::Xbgr8888 => PLANE_CTL_ORDER_RGBX,
    };
    (ctl_before
        & !(PLANE_CTL_ENABLE
            | PLANE_CTL_ARB_SLOTS_MASK
            | PLANE_CTL_FORMAT_MASK_SKL
            | PLANE_CTL_KEY_ENABLE_MASK
            | PLANE_CTL_TILED_MASK
            | PLANE_CTL_ORDER_RGBX))
        | PLANE_CTL_ENABLE
        | PLANE_CTL_ARB_SLOTS_4BPP
        | PLANE_CTL_FORMAT_XRGB_8888
        | PLANE_CTL_TILED_LINEAR
        | order_bits
}

fn overlay_plane_ctl_enabled(ctl_before: u32) -> u32 {
    primary_plane_ctl_enabled(ctl_before)
}

fn plane_color_ctl_alpha(color_ctl: u32, alpha: OverlayAlphaMode) -> u32 {
    let alpha_bits = match alpha {
        OverlayAlphaMode::Opaque => PLANE_COLOR_ALPHA_DISABLE,
        OverlayAlphaMode::Straight => PLANE_COLOR_ALPHA_SW_PREMULT,
    };
    (color_ctl & !PLANE_COLOR_ALPHA_MASK) | PLANE_COLOR_PLANE_GAMMA_DISABLE | alpha_bits
}

fn plane_buf_cfg_value(start: u16, end_inclusive: u16) -> u32 {
    ((u32::from(end_inclusive) & 0x1FFF) << 16) | (u32::from(start) & 0x1FFF)
}

fn program_plane_watermark_boot_safe(dev: crate::intel::Dev, plane_base: usize, enable: bool) {
    crate::intel::mmio_write(
        dev,
        plane_base + UNI_PLANE_WM_0_OFF,
        if enable { PLANE_WM_LEVEL0_BOOT_SAFE } else { 0 },
    );

    let mut level = 1usize;
    while level < UNI_PLANE_WM_LEVELS {
        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_WM_0_OFF + level * 4, 0);
        level += 1;
    }

    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_WM_TRANS_OFF, 0);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_WM_SAGV_OFF, 0);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_WM_SAGV_TRANS_OFF, 0);
}

fn program_plane_buf_cfg(
    dev: crate::intel::Dev,
    plane_base: usize,
    start: u16,
    end_inclusive: u16,
) -> (u32, u32) {
    let before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_BUF_CFG_OFF);
    let after = plane_buf_cfg_value(start, end_inclusive);
    if before != after {
        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_BUF_CFG_OFF, after);
    }
    (before, crate::intel::mmio_read(dev, plane_base + UNI_PLANE_BUF_CFG_OFF))
}

fn program_three_plane_stack_resources(dev: crate::intel::Dev, pipe: PipeInfo, reason: &str) {
    let primary_base = overlay_plane_base(pipe, 0);
    let ui_base = overlay_plane_base(pipe, UI_OVERLAY_PLANE_SLOT);
    let video_uv_base = overlay_plane_base(pipe, VIDEO_NV12_PLANE_SLOT);
    let video_y_base = overlay_plane_base(pipe, VIDEO_NV12_Y_PLANE_SLOT);

    program_plane_watermark_boot_safe(dev, primary_base, true);
    let (primary_buf_before, primary_buf_after) = program_plane_buf_cfg(
        dev,
        primary_base,
        PLANE_DBUF_PRIMARY_STACK_START,
        PLANE_DBUF_PRIMARY_STACK_END,
    );

    if primary_buf_before != primary_buf_after {
        let _ = wait_for_pipe_next_frame(dev, pipe);
    }

    program_plane_watermark_boot_safe(dev, ui_base, true);
    let (ui_buf_before, ui_buf_after) = program_plane_buf_cfg(
        dev,
        ui_base,
        PLANE_DBUF_UI_OVERLAY_STACK_START,
        PLANE_DBUF_UI_OVERLAY_STACK_END,
    );

    program_plane_watermark_boot_safe(dev, video_uv_base, true);
    let (video_uv_buf_before, video_uv_buf_after) = program_plane_buf_cfg(
        dev,
        video_uv_base,
        PLANE_DBUF_VIDEO_NV12_UV_STACK_START,
        PLANE_DBUF_VIDEO_NV12_UV_STACK_END,
    );

    program_plane_watermark_boot_safe(dev, video_y_base, true);
    let (video_y_buf_before, video_y_buf_after) = program_plane_buf_cfg(
        dev,
        video_y_base,
        PLANE_DBUF_VIDEO_NV12_Y_STACK_START,
        PLANE_DBUF_VIDEO_NV12_Y_STACK_END,
    );

    crate::log!(
        "intel/display: plane-stack-resources reason={} pipe={} primary_slot=0 buf=0x{:08X}=>0x{:08X} wm0=0x{:08X} ui_slot={} buf=0x{:08X}=>0x{:08X} wm0=0x{:08X} video_uv_slot={} buf=0x{:08X}=>0x{:08X} wm0=0x{:08X} video_y_slot={} buf=0x{:08X}=>0x{:08X} wm0=0x{:08X}\n",
        reason,
        pipe.name,
        primary_buf_before,
        primary_buf_after,
        crate::intel::mmio_read(dev, primary_base + UNI_PLANE_WM_0_OFF),
        UI_OVERLAY_PLANE_SLOT,
        ui_buf_before,
        ui_buf_after,
        crate::intel::mmio_read(dev, ui_base + UNI_PLANE_WM_0_OFF),
        VIDEO_NV12_PLANE_SLOT,
        video_uv_buf_before,
        video_uv_buf_after,
        crate::intel::mmio_read(dev, video_uv_base + UNI_PLANE_WM_0_OFF),
        VIDEO_NV12_Y_PLANE_SLOT,
        video_y_buf_before,
        video_y_buf_after,
        crate::intel::mmio_read(dev, video_y_base + UNI_PLANE_WM_0_OFF),
    );
}

fn disable_non_primary_universal_planes(dev: crate::intel::Dev, pipe: PipeInfo, reason: &str) {
    let mut slot = 1usize;
    while slot < UNIVERSAL_PLANE_SLOTS {
        let plane_base = pipe.plane(slot).base();
        let ctl_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CTL_OFF);
        let surf_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF);
        let live_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);
        let color_ctl_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_COLOR_CTL_OFF);
        let ctl_disabled = ctl_before & !PLANE_CTL_ENABLE;
        let color_ctl_disabled = plane_color_ctl_alpha(color_ctl_before, OverlayAlphaMode::Opaque);

        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_CTL_OFF, ctl_disabled);
        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_SURF_OFF, 0);
        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_COLOR_CTL_OFF, color_ctl_disabled);

        if (ctl_before & PLANE_CTL_ENABLE) != 0 || surf_before != 0 || live_before != 0 {
            crate::log!(
                "intel/display: plane-stack-disable reason={} pipe={} slot={} ctl=0x{:08X}=>0x{:08X} surf=0x{:08X} live=0x{:08X} color_ctl=0x{:08X}=>0x{:08X} color_alpha={}=>{}\n",
                reason,
                pipe.name,
                slot,
                ctl_before,
                ctl_disabled,
                surf_before,
                live_before,
                color_ctl_before,
                color_ctl_disabled,
                decode_plane_color_alpha(color_ctl_before),
                decode_plane_color_alpha(color_ctl_disabled),
            );
        }

        slot += 1;
    }
}

fn program_primary_plane_and_wait(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    width: u32,
    height: u32,
    pitch_bytes: u32,
    surface_reg: u32,
    reason: &str,
) -> (u32, u32, u32, usize) {
    let Some(stride_reg) = plane_stride_reg_value(pitch_bytes) else {
        return (0, 0, 0, 0);
    };
    let ctl_before = crate::intel::mmio_read(dev, pipe.primary_plane().ctl());
    let ctl_disabled = ctl_before & !PLANE_CTL_ENABLE;
    let ctl_enabled = primary_plane_ctl_enabled(ctl_before);
    let color_ctl_off = pipe.primary_plane().base() + UNI_PLANE_COLOR_CTL_OFF;
    let color_ctl_before = crate::intel::mmio_read(dev, color_ctl_off);
    let color_ctl_enabled = plane_color_ctl_alpha(color_ctl_before, OverlayAlphaMode::Opaque);

    if PRIMARY_REARM_PRESERVE_NON_PRIMARY_PLANES {
        intel_display_verbose_log!(
            "intel/display: primary-rearm-preserve-non-primary reason={} pipe={}\n",
            reason,
            pipe.name
        );
        arm_rgb_plane_probe_planes(dev, pipe, reason);
    } else {
        disable_non_primary_universal_planes(dev, pipe, reason);
    }
    crate::intel::mmio_write(dev, pipe.primary_plane().ctl(), ctl_disabled);
    crate::intel::mmio_write(dev, pipe.primary_plane().surf(), 0);
    let (disable_frame_before, disable_frame_after, disable_frame_iters) =
        wait_for_pipe_next_frame(dev, pipe);
    let (live_cleared, clear_iters) = wait_for_primary_plane_live(dev, pipe, 0, 20_000);

    crate::intel::mmio_write(dev, pipe.primary_plane().stride(), stride_reg);
    crate::intel::mmio_write(
        dev,
        pipe.primary_plane().base() + UNI_PLANE_POS_OFF,
        plane_pos_reg_value(0, 0),
    );
    crate::intel::mmio_write(
        dev,
        pipe.primary_plane().base() + UNI_PLANE_SIZE_OFF,
        plane_size_reg_value(width, height),
    );
    crate::intel::mmio_write(
        dev,
        pipe.primary_plane().base() + UNI_PLANE_OFFSET_OFF,
        plane_pos_reg_value(0, 0),
    );
    crate::intel::mmio_write(dev, color_ctl_off, color_ctl_enabled);
    crate::intel::mmio_write(dev, pipe.primary_plane().ctl(), ctl_enabled);
    crate::intel::mmio_write(dev, pipe.primary_plane().surf(), surface_reg);

    let (arm_frame_before, arm_frame_after, arm_frame_iters) = wait_for_pipe_next_frame(dev, pipe);
    let (surf_live_after, live_iters) = wait_for_primary_plane_live(dev, pipe, surface_reg, 20_000);

    intel_display_verbose_log!(
        "intel/display: primary-rearm reason={} pipe={} format_probe={} ctl_before=0x{:08X} ctl_disabled=0x{:08X} ctl_enabled=0x{:08X} color_ctl=0x{:08X}=>0x{:08X} color_alpha={}=>{} disable_frame={}=>{} disable_wait={} clear_live=0x{:08X} clear_iters={} arm_frame={}=>{} arm_wait={} surf=0x{:08X} surf_live=0x{:08X} live_iters={}\n",
        reason,
        pipe.name,
        primary_format_probe_name(),
        ctl_before,
        ctl_disabled,
        ctl_enabled,
        color_ctl_before,
        color_ctl_enabled,
        decode_plane_color_alpha(color_ctl_before),
        decode_plane_color_alpha(color_ctl_enabled),
        disable_frame_before,
        disable_frame_after,
        disable_frame_iters,
        live_cleared,
        clear_iters,
        arm_frame_before,
        arm_frame_after,
        arm_frame_iters,
        crate::intel::mmio_read(dev, pipe.primary_plane().surf()),
        surf_live_after,
        live_iters
    );

    (ctl_before, ctl_enabled, surf_live_after, live_iters)
}

pub(crate) fn kick_primary_surface_scanout(label: &str) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };

    let pos_off = surface.pipe.primary_plane().base() + UNI_PLANE_POS_OFF;
    let size_off = surface.pipe.primary_plane().base() + UNI_PLANE_SIZE_OFF;
    let pos_before = crate::intel::mmio_read(dev, pos_off);
    let size_before = crate::intel::mmio_read(dev, size_off);
    let stride_before = crate::intel::mmio_read(dev, surface.pipe.primary_plane().stride());
    let surf_before = crate::intel::mmio_read(dev, surface.pipe.primary_plane().surf());
    let live_before = crate::intel::mmio_read(dev, surface.pipe.primary_plane().surf_live());
    let Some(surface_reg) = u32::try_from(surface.gpu).ok() else {
        return false;
    };

    let (_, _, live_after, iter) = program_primary_plane_and_wait(
        dev,
        surface.pipe,
        surface.width,
        surface.height,
        surface.pitch_bytes,
        surface_reg,
        label,
    );
    let pos_after = crate::intel::mmio_read(dev, pos_off);
    let size_after = crate::intel::mmio_read(dev, size_off);
    let stride_after = crate::intel::mmio_read(dev, surface.pipe.primary_plane().stride());
    let surf_after = crate::intel::mmio_read(dev, surface.pipe.primary_plane().surf());

    intel_display_verbose_log!(
        "intel/display: primary-scanout-kick label={} pipe={} stride_before=0x{:08X} stride_after=0x{:08X} size_before=0x{:08X} size_after=0x{:08X} pos_before=0x{:08X} pos_after=0x{:08X} surf_before=0x{:08X} surf_after=0x{:08X} live_before=0x{:08X} live_after=0x{:08X} iter={}\n",
        label,
        surface.pipe.name,
        stride_before,
        stride_after,
        size_before,
        size_after,
        pos_before,
        pos_after,
        surf_before,
        surf_after,
        live_before,
        live_after,
        iter
    );

    live_after == surface_reg
}

pub(crate) fn log_pipe_live_scanout_state(label: &str) {
    let Some(dev) = crate::intel::claimed_device() else {
        return;
    };
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return;
    };
    let pipe = surface.pipe;
    let pipe_src_raw = crate::intel::mmio_read(dev, pipe.pipe_src_off);
    let (pipe_w, pipe_h) = decode_pipe_src(pipe_src_raw).unwrap_or((0, 0));
    crate::log!(
        "intel/display: live-scanout label={} pipe={} pipe_src=0x{:08X} dims={}x{} primary_surf_gpu=0x{:08X}\n",
        label,
        pipe.name,
        pipe_src_raw,
        pipe_w,
        pipe_h,
        crate::intel::mmio_read(dev, pipe.primary_plane().surf())
    );

    let mut slot = 0usize;
    while slot < UNIVERSAL_PLANE_SLOTS {
        let plane_base = pipe.plane(slot).base();
        let ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CTL_OFF);
        let stride = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_STRIDE_OFF);
        let pos = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_POS_OFF);
        let size = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SIZE_OFF);
        let keyval = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYVAL_OFF);
        let keymsk = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYMSK_OFF);
        let surf = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF);
        let keymax = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYMAX_OFF);
        let offset = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_OFFSET_OFF);
        let surf_live = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);
        let aux_dist = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_AUX_DIST_OFF);
        let aux_offset = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_AUX_OFFSET_OFF);
        let cus_ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CUS_CTL_OFF);
        let color_ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_COLOR_CTL_OFF);
        let wm0 = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_WM_0_OFF);
        let wm_sagv = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_WM_SAGV_OFF);
        let wm_sagv_trans = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_WM_SAGV_TRANS_OFF);
        let wm_trans = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_WM_TRANS_OFF);
        let buf_cfg = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_BUF_CFG_OFF);
        crate::log!(
            "intel/display: live-plane label={} pipe={} slot={} enabled={} format={} tiled={} rot={} rgbx={} stride=0x{:08X} pos={}x{} size={}x{} surf=0x{:08X} surf_live=0x{:08X} cus_ctl=0x{:08X} cus_en={} cus_y={} cus_h={} cus_v={} color_ctl=0x{:08X} color_alpha={} buf_cfg=0x{:08X}\n",
            label,
            pipe.name,
            slot,
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
            surf,
            surf_live,
            cus_ctl,
            ((cus_ctl & PLANE_CUS_ENABLE) != 0) as u8,
            ((cus_ctl & PLANE_CUS_Y_PLANE) != 0) as u8,
            decode_plane_cus_phase(cus_ctl, PLANE_CUS_HPHASE_MASK, PLANE_CUS_HPHASE_SIGN_NEGATIVE),
            decode_plane_cus_phase(cus_ctl, PLANE_CUS_VPHASE_MASK, PLANE_CUS_VPHASE_SIGN_NEGATIVE),
            color_ctl,
            decode_plane_color_alpha(color_ctl),
            buf_cfg
        );
        crate::log!(
            "intel/display: plane-raw label={} pipe={} slot={} base=0x{:05X} ctl=0x{:08X} stride=0x{:08X} pos=0x{:08X} size=0x{:08X} key=0x{:08X}/0x{:08X}/0x{:08X} offset=0x{:08X} surf=0x{:08X} live=0x{:08X} aux=0x{:08X}/0x{:08X} cus=0x{:08X} color=0x{:08X} wm0=0x{:08X} wm_sagv=0x{:08X} wm_sagv_trans=0x{:08X} wm_trans=0x{:08X} buf=0x{:08X}\n",
            label,
            pipe.name,
            slot,
            plane_base,
            ctl,
            stride,
            pos,
            size,
            keyval,
            keymsk,
            keymax,
            offset,
            surf,
            surf_live,
            aux_dist,
            aux_offset,
            cus_ctl,
            color_ctl,
            wm0,
            wm_sagv,
            wm_sagv_trans,
            wm_trans,
            buf_cfg
        );
        slot += 1;
    }
}

fn log_display_power_well_snapshot(label: &str) {
    let Some(dev) = crate::intel::claimed_device() else {
        return;
    };

    let main_bios = crate::intel::mmio_read(dev, HSW_PWR_WELL_CTL1);
    let main_driver = crate::intel::mmio_read(dev, HSW_PWR_WELL_CTL2);
    let main_kvmr = crate::intel::mmio_read(dev, HSW_PWR_WELL_CTL3);
    let main_debug = crate::intel::mmio_read(dev, HSW_PWR_WELL_CTL4);
    let aux_bios = crate::intel::mmio_read(dev, ICL_PWR_WELL_CTL_AUX1);
    let aux_driver = crate::intel::mmio_read(dev, ICL_PWR_WELL_CTL_AUX2);
    let aux_debug = crate::intel::mmio_read(dev, ICL_PWR_WELL_CTL_AUX4);
    let ddi_bios = crate::intel::mmio_read(dev, ICL_PWR_WELL_CTL_DDI1);
    let ddi_driver = crate::intel::mmio_read(dev, ICL_PWR_WELL_CTL_DDI2);
    let ddi_debug = crate::intel::mmio_read(dev, ICL_PWR_WELL_CTL_DDI4);
    let fuse = crate::intel::mmio_read(dev, SKL_FUSE_STATUS);

    crate::log!(
        "intel/display: power-wells label={} main_ctl=[bios=0x{:08X},driver=0x{:08X},kvmr=0x{:08X},debug=0x{:08X}] main_state_pg1..5=[{},{},{},{},{}] main_req_pg1..5=[{},{},{},{},{}] aux_ctl=[bios=0x{:08X},driver=0x{:08X},debug=0x{:08X}] ddi_ctl=[bios=0x{:08X},driver=0x{:08X},debug=0x{:08X}] fuse=0x{:08X} fuse_pg1..5=[{},{},{},{},{}]\n",
        label,
        main_bios,
        main_driver,
        main_kvmr,
        main_debug,
        power_well_state_bit(main_driver, 0),
        power_well_state_bit(main_driver, 1),
        power_well_state_bit(main_driver, 2),
        power_well_state_bit(main_driver, 3),
        power_well_state_bit(main_driver, 4),
        power_well_request_bit(main_driver, 0),
        power_well_request_bit(main_driver, 1),
        power_well_request_bit(main_driver, 2),
        power_well_request_bit(main_driver, 3),
        power_well_request_bit(main_driver, 4),
        aux_bios,
        aux_driver,
        aux_debug,
        ddi_bios,
        ddi_driver,
        ddi_debug,
        fuse,
        fuse_pg_distribution_done(fuse, 1),
        fuse_pg_distribution_done(fuse, 2),
        fuse_pg_distribution_done(fuse, 3),
        fuse_pg_distribution_done(fuse, 4),
        fuse_pg_distribution_done(fuse, 5)
    );
}

#[inline]
fn power_well_state_bit(raw: u32, index: u32) -> u8 {
    ((raw >> index.saturating_mul(2)) & 0x1) as u8
}

#[inline]
fn power_well_request_bit(raw: u32, index: u32) -> u8 {
    ((raw >> index.saturating_mul(2).saturating_add(1)) & 0x1) as u8
}

#[inline]
fn fuse_pg_distribution_done(raw: u32, pg: u32) -> u8 {
    if pg > 27 {
        return 0;
    }
    ((raw >> (27 - pg)) & 0x1) as u8
}

fn log_primary_scanout_pte_window(dev: crate::intel::Dev, label: &str, byte_len: usize) {
    let page_count = byte_len.div_ceil(crate::intel::WARM_ALIGN);
    let mut entries = [0u64; 4];
    let count = page_count.min(entries.len());
    let mut idx = 0usize;
    while idx < count {
        let gpu = crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE
            + (idx as u64) * crate::intel::WARM_ALIGN as u64;
        entries[idx] = crate::intel::read_ggtt_pte(dev, gpu).unwrap_or(0);
        idx += 1;
    }
    intel_display_verbose_log!(
        "intel/display: primary-ggtt label={} gpu=0x{:X} bytes=0x{:X} pages={} pte0=0x{:016X} pte1=0x{:016X} pte2=0x{:016X} pte3=0x{:016X}\n",
        label,
        crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE,
        byte_len,
        page_count,
        entries[0],
        entries[1],
        entries[2],
        entries[3]
    );
}

fn overlay_plane_base(pipe: PipeInfo, plane_slot: usize) -> usize {
    pipe.plane(plane_slot).base()
}

fn overlay_plane_top_right_position(surface: OverlaySurface) -> (u32, u32) {
    let (scanout_w, scanout_h) = active_scanout_dimensions()
        .or_else(|| {
            PRIMARY_SURFACE
                .lock()
                .as_ref()
                .map(|primary| (primary.width, primary.height))
        })
        .unwrap_or((surface.width, surface.height));
    let x = scanout_w
        .saturating_sub(surface.width)
        .saturating_sub(OVERLAY_MARGIN_X);
    let y = OVERLAY_MARGIN_Y.min(scanout_h.saturating_sub(surface.height));
    (x, y)
}

fn overlay_plane_clamped_position(surface: OverlaySurface, x: u32, y: u32) -> (u32, u32) {
    let (scanout_w, scanout_h) = active_scanout_dimensions()
        .or_else(|| {
            PRIMARY_SURFACE
                .lock()
                .as_ref()
                .map(|primary| (primary.width, primary.height))
        })
        .unwrap_or((surface.width, surface.height));
    (
        x.min(scanout_w.saturating_sub(surface.width)),
        y.min(scanout_h.saturating_sub(surface.height)),
    )
}

fn overlay_surface_gpu_for_index(index: usize) -> Option<u64> {
    if index >= OVERLAY_SWAP_BUFFER_COUNT {
        return None;
    }
    OVERLAY_SWAP_GPU_BASE.checked_add((index as u64).checked_mul(OVERLAY_SWAP_GPU_STRIDE)?)
}

fn overlay_back_buffer_index(pool: OverlaySurfacePool) -> usize {
    pool.front_index
        .map(|front| (front + 1) % OVERLAY_SWAP_BUFFER_COUNT)
        .unwrap_or(0)
}

fn mark_overlay_surface_front(surface: OverlaySurface) {
    let mut pool = OVERLAY_SURFACE.lock();
    if pool.matches(surface.width, surface.height, surface.pipe) {
        pool.front_index = Some(surface.buffer_index);
    }
}

fn overlay_surface_for_gpu(width: u32, height: u32, gpu: u64) -> Option<OverlaySurface> {
    let pool = OVERLAY_SURFACE.lock();
    if pool.width != width || pool.height != height {
        return None;
    }
    for surface in pool.surfaces.iter().flatten().copied() {
        if surface.gpu == gpu {
            return Some(surface);
        }
    }
    None
}

fn copy_overlay_front_into_back(back: OverlaySurface) -> bool {
    let front = {
        let pool = OVERLAY_SURFACE.lock();
        if !pool.matches(back.width, back.height, back.pipe) {
            return false;
        }
        let Some(front_index) = pool.front_index else {
            return false;
        };
        if front_index == back.buffer_index {
            return false;
        }
        let Some(front) = pool.surfaces[front_index] else {
            return false;
        };
        front
    };
    if front.virt.is_null()
        || back.virt.is_null()
        || front.byte_len != back.byte_len
        || front.pitch_bytes != back.pitch_bytes
    {
        return false;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(front.virt, back.virt, back.byte_len);
    }
    true
}

fn live_rect_covers_surface(rect: LiveOverlayRect, surface: OverlaySurface) -> bool {
    rect.x == 0 && rect.y == 0 && rect.width >= surface.width && rect.height >= surface.height
}

fn init_default_overlay_marker(dev: crate::intel::Dev, primary: PrimarySurface) -> bool {
    if !DEFAULT_OVERLAY_MARKER_ENABLED {
        return false;
    }

    let Some(surface) =
        ensure_overlay_surface(dev, DEFAULT_OVERLAY_MARKER_SIZE, DEFAULT_OVERLAY_MARKER_SIZE)
    else {
        crate::log!(
            "intel/display: default-overlay-marker skipped pipe={} cause=no-surface\n",
            primary.pipe.name
        );
        return false;
    };
    fill_surface_color(
        surface.virt,
        surface.pitch_bytes as usize,
        surface.width,
        surface.height,
        DEFAULT_OVERLAY_MARKER_COLOR,
    );
    crate::intel::dma_flush(
        surface.virt,
        (surface.pitch_bytes as usize).saturating_mul(surface.height as usize),
    );

    let (scanout_w, scanout_h) =
        active_scanout_dimensions().unwrap_or((primary.width, primary.height));
    let pos_x = scanout_w.saturating_sub(surface.width) / 2;
    let pos_y = scanout_h.saturating_sub(surface.height) / 2;
    let reason = "default-overlay-marker";
    if overlay_plane_needs_rearm(dev, surface, pos_x, pos_y, OverlayAlphaMode::Opaque) {
        program_three_plane_stack_resources(dev, surface.pipe, reason);
        if !arm_overlay_plane(dev, surface, pos_x, pos_y, OverlayAlphaMode::Opaque, reason) {
            return false;
        }
        mark_overlay_surface_front(surface);
    }

    crate::log!(
        "intel/display: default-overlay-marker pipe={} slot={} pos={}x{} size={}x{} color=0x{:08X}\n",
        surface.pipe.name,
        surface.plane_slot,
        pos_x,
        pos_y,
        surface.width,
        surface.height,
        DEFAULT_OVERLAY_MARKER_COLOR
    );
    true
}

fn ensure_overlay_surface(
    dev: crate::intel::Dev,
    width: u32,
    height: u32,
) -> Option<OverlaySurface> {
    let active_pipe = active_pipe(dev)?;
    let buffer_index = {
        let pool = OVERLAY_SURFACE.lock();
        if pool.matches(width, height, active_pipe) {
            let index = overlay_back_buffer_index(*pool);
            if let Some(surface) = pool.surfaces[index] {
                return Some(surface);
            }
            index
        } else {
            0
        }
    };
    let gpu = overlay_surface_gpu_for_index(buffer_index)?;

    let pitch_bytes = aligned_pitch_bytes(width, PRIMARY_BYTES_PER_PIXEL)?;
    let byte_len = usize::try_from(u64::from(pitch_bytes) * u64::from(height)).ok()?;
    let (phys, virt) = crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN)?;
    fill_surface_color(virt, pitch_bytes as usize, width, height, 0);
    crate::intel::dma_flush(virt, byte_len);

    if !crate::intel::map_display_scanout_ggtt(dev, phys, byte_len, gpu) {
        crate::log!(
            "intel/display: overlay-surface ggtt map failed pipe={} slot={} buffer={} size={}x{} bytes=0x{:X} gpu=0x{:X}\n",
            active_pipe.name,
            OVERLAY_PLANE_SLOT,
            buffer_index,
            width,
            height,
            byte_len,
            gpu
        );
        return None;
    }
    crate::intel::ggtt_invalidate(dev);

    let surface = OverlaySurface {
        width,
        height,
        pitch_bytes,
        byte_len,
        phys,
        virt,
        gpu,
        pipe: active_pipe,
        plane_slot: OVERLAY_PLANE_SLOT,
        buffer_index,
    };
    {
        let mut pool = OVERLAY_SURFACE.lock();
        if !pool.matches(width, height, active_pipe) {
            *pool = OverlaySurfacePool {
                width,
                height,
                pipe_slot: active_pipe.slot,
                front_index: None,
                surfaces: [None; OVERLAY_SWAP_BUFFER_COUNT],
            };
        }
        pool.surfaces[buffer_index] = Some(surface);
    }
    crate::log!(
        "intel/display: overlay-surface pipe={} slot={} buffer={} size={}x{} pitch=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X}\n",
        active_pipe.name,
        OVERLAY_PLANE_SLOT,
        buffer_index,
        width,
        height,
        pitch_bytes,
        byte_len,
        gpu,
        phys
    );
    Some(surface)
}

fn copy_rgba_into_overlay(
    surface: OverlaySurface,
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    alpha: OverlayAlphaMode,
) -> bool {
    let dst_pitch = surface.pitch_bytes as usize;
    if src_width != surface.width || src_height != surface.height {
        return false;
    }
    if src_pitch_bytes < src_width as usize * 4 || dst_pitch < src_width as usize * 4 {
        return false;
    }

    for row_idx in 0..(src_height as usize) {
        let src_row_off = row_idx.saturating_mul(src_pitch_bytes);
        let Some(src_row) = src.get(src_row_off..src_row_off + src_width as usize * 4) else {
            return false;
        };
        let dst_row = unsafe { surface.virt.add(row_idx.saturating_mul(dst_pitch)) as *mut u32 };
        for col_idx in 0..(src_width as usize) {
            let src_off = col_idx.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let a = src_row[src_off + 3];
            let pixel = match alpha {
                OverlayAlphaMode::Opaque => u32::from_le_bytes([b, g, r, 0]),
                OverlayAlphaMode::Straight => {
                    u32::from_le_bytes([premul_u8(b, a), premul_u8(g, a), premul_u8(r, a), a])
                }
            };
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
    }

    true
}

#[inline]
fn premul_u8(color: u8, alpha: u8) -> u8 {
    (((color as u16) * (alpha as u16) + 127) / 255) as u8
}

fn stamp_overlay_composition_proof_marker(
    surface: OverlaySurface,
    alpha: OverlayAlphaMode,
    reason: &str,
) -> bool {
    if !OVERLAY_COMPOSITION_PROOF_MARKER_ENABLED || alpha != OverlayAlphaMode::Straight {
        return false;
    }

    let size = OVERLAY_COMPOSITION_PROOF_MARKER_SIZE;
    let gap = OVERLAY_COMPOSITION_PROOF_MARKER_GAP;
    let x0 = OVERLAY_COMPOSITION_PROOF_MARKER_X;
    let y0 = OVERLAY_COMPOSITION_PROOF_MARKER_Y;
    let x1 = x0.saturating_add(size).saturating_add(gap);
    let x2 = x1.saturating_add(size).saturating_add(gap);
    if x2.saturating_add(size) > surface.width || y0.saturating_add(size) > surface.height {
        crate::log!(
            "intel/display: overlay-proof skipped reason={} cause=surface-too-small size={}x{} marker={}x{}@{},{}\n",
            reason,
            surface.width,
            surface.height,
            size.saturating_mul(3).saturating_add(gap.saturating_mul(2)),
            size,
            x0,
            y0
        );
        return false;
    }

    let transparent = overlay_scanout_pixel_bgra_premul(0xFF, 0x00, 0xFF, 0x00);
    let half_red = overlay_scanout_pixel_bgra_premul(0xFF, 0x00, 0x00, 0x80);
    let opaque_green = overlay_scanout_pixel_bgra_premul(0x00, 0xFF, 0x00, 0xFF);
    fill_overlay_rect(surface, x0, y0, size, size, transparent);
    fill_overlay_rect(surface, x1, y0, size, size, half_red);
    fill_overlay_rect(surface, x2, y0, size, size, opaque_green);

    let cy = y0.saturating_add(size / 2);
    let transparent_cx = x0.saturating_add(size / 2);
    let half_red_cx = x1.saturating_add(size / 2);
    let opaque_green_cx = x2.saturating_add(size / 2);
    let overlay_transparent = sample_overlay_surface_pixel(surface, transparent_cx, cy);
    let overlay_half_red = sample_overlay_surface_pixel(surface, half_red_cx, cy);
    let overlay_opaque_green = sample_overlay_surface_pixel(surface, opaque_green_cx, cy);
    let primary_transparent = sample_primary_surface_pixel(transparent_cx, cy).unwrap_or_default();
    let primary_half_red = sample_primary_surface_pixel(half_red_cx, cy).unwrap_or_default();
    let primary_opaque_green =
        sample_primary_surface_pixel(opaque_green_cx, cy).unwrap_or_default();

    crate::log!(
        "intel/display: overlay-proof reason={} pipe={} slot={} badge={}x{}@{},{} cells=transparent,half-red,opaque-green overlay=[0x{:08X},0x{:08X},0x{:08X}] primary_under=[0x{:08X},0x{:08X},0x{:08X}] expectation=alpha-ok:underlay/red-blend/green alpha-ignored:black/dark-red/green\n",
        reason,
        surface.pipe.name,
        surface.plane_slot,
        size.saturating_mul(3).saturating_add(gap.saturating_mul(2)),
        size,
        x0,
        y0,
        overlay_transparent,
        overlay_half_red,
        overlay_opaque_green,
        primary_transparent,
        primary_half_red,
        primary_opaque_green
    );
    true
}

#[inline]
fn overlay_scanout_pixel_bgra_premul(r: u8, g: u8, b: u8, a: u8) -> u32 {
    u32::from_le_bytes([premul_u8(b, a), premul_u8(g, a), premul_u8(r, a), a])
}

fn fill_overlay_rect(surface: OverlaySurface, x: u32, y: u32, width: u32, height: u32, pixel: u32) {
    if surface.virt.is_null() || surface.pitch_bytes < surface.width.saturating_mul(4) {
        return;
    }
    let x0 = x.min(surface.width);
    let y0 = y.min(surface.height);
    let x1 = x0.saturating_add(width).min(surface.width);
    let y1 = y0.saturating_add(height).min(surface.height);
    let pitch_pixels = (surface.pitch_bytes as usize) / 4;
    for row_idx in y0 as usize..y1 as usize {
        let row = unsafe { (surface.virt as *mut u32).add(row_idx.saturating_mul(pitch_pixels)) };
        for col_idx in x0 as usize..x1 as usize {
            unsafe {
                core::ptr::write_volatile(row.add(col_idx), pixel);
            }
        }
    }
}

fn fill_overlay_rect_rgba(surface: OverlaySurface, rect: LiveOverlayRect) {
    if rect.width == 0 || rect.height == 0 || rect.color.a == 0 {
        return;
    }
    fill_overlay_rect(
        surface,
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        overlay_scanout_pixel_bgra_premul(rect.color.r, rect.color.g, rect.color.b, rect.color.a),
    );
}

fn sample_overlay_surface_pixel(surface: OverlaySurface, x: u32, y: u32) -> u32 {
    if surface.virt.is_null()
        || x >= surface.width
        || y >= surface.height
        || surface.pitch_bytes < surface.width.saturating_mul(4)
    {
        return 0;
    }
    let pitch_pixels = (surface.pitch_bytes as usize) / 4;
    unsafe {
        core::ptr::read_volatile(
            (surface.virt as *const u32).add(
                (y as usize)
                    .saturating_mul(pitch_pixels)
                    .saturating_add(x as usize),
            ),
        )
    }
}

fn overlay_plane_needs_rearm(
    dev: crate::intel::Dev,
    surface: OverlaySurface,
    pos_x: u32,
    pos_y: u32,
    alpha: OverlayAlphaMode,
) -> bool {
    let plane_base = overlay_plane_base(surface.pipe, surface.plane_slot);
    let want_pos = plane_pos_reg_value(pos_x, pos_y);
    let want_size = plane_size_reg_value(surface.width, surface.height);
    let want_surf = u32::try_from(surface.gpu).unwrap_or(0);
    let ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CTL_OFF);
    let pos = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_POS_OFF);
    let size = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SIZE_OFF);
    let surf = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF);
    let surf_live = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);
    let color_ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_COLOR_CTL_OFF);
    let want_color_ctl = plane_color_ctl_alpha(color_ctl, alpha);

    (ctl & PLANE_CTL_ENABLE) == 0
        || pos != want_pos
        || size != want_size
        || surf != want_surf
        || surf_live != want_surf
        || (color_ctl & PLANE_COLOR_ALPHA_MASK) != (want_color_ctl & PLANE_COLOR_ALPHA_MASK)
}

fn arm_overlay_plane(
    dev: crate::intel::Dev,
    surface: OverlaySurface,
    pos_x: u32,
    pos_y: u32,
    alpha: OverlayAlphaMode,
    reason: &str,
) -> bool {
    let plane_base = overlay_plane_base(surface.pipe, surface.plane_slot);
    let Some(surface_reg) = u32::try_from(surface.gpu).ok() else {
        return false;
    };
    let Some(stride_reg) = plane_stride_reg_value(surface.pitch_bytes) else {
        return false;
    };
    let ctl_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CTL_OFF);
    let ctl_disabled = ctl_before & !PLANE_CTL_ENABLE;
    let ctl_enabled = overlay_plane_ctl_enabled(ctl_before);
    let color_ctl_off = plane_base + UNI_PLANE_COLOR_CTL_OFF;
    let color_ctl_before = crate::intel::mmio_read(dev, color_ctl_off);
    let color_ctl_enabled = plane_color_ctl_alpha(color_ctl_before, alpha);
    let surf_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF);
    let live_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);

    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_CTL_OFF, ctl_disabled);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_SURF_OFF, 0);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_STRIDE_OFF, stride_reg);
    crate::intel::mmio_write(
        dev,
        plane_base + UNI_PLANE_POS_OFF,
        plane_pos_reg_value(pos_x, pos_y),
    );
    crate::intel::mmio_write(
        dev,
        plane_base + UNI_PLANE_SIZE_OFF,
        plane_size_reg_value(surface.width, surface.height),
    );
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_KEYVAL_OFF, 0);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_KEYMSK_OFF, 0);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_KEYMAX_OFF, 0xFF00_0000);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_OFFSET_OFF, plane_pos_reg_value(0, 0));
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_AUX_DIST_OFF, 0);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_AUX_OFFSET_OFF, 0);
    crate::intel::mmio_write(dev, color_ctl_off, color_ctl_enabled);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_CTL_OFF, ctl_enabled);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_SURF_OFF, surface_reg);

    let (frame_before, frame_after, frame_iters) = wait_for_pipe_next_frame(dev, surface.pipe);
    let (live_after, live_iters) = wait_for_plane_live(dev, plane_base, surface_reg, 20_000);
    let keyval_after = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYVAL_OFF);
    let keymsk_after = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYMSK_OFF);
    let keymax_after = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYMAX_OFF);
    let aux_dist_after = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_AUX_DIST_OFF);
    let aux_offset_after = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_AUX_OFFSET_OFF);
    let ok = live_after == surface_reg;
    if ok {
        mark_overlay_surface_front(surface);
    }

    crate::log!(
        "intel/display: overlay-arm reason={} pipe={} slot={} alpha={:?} ctl_before=0x{:08X} ctl_enabled=0x{:08X} color_ctl=0x{:08X}=>0x{:08X} color_alpha={}=>{} pos={}x{} size={}x{} stride=0x{:08X} key=0x{:08X}/0x{:08X}/0x{:08X} aux=0x{:08X}/0x{:08X} surf_before=0x{:08X} surf_after=0x{:08X} surf_live_before=0x{:08X} surf_live_after=0x{:08X} frame={}=>{} frame_wait={} live_iters={}\n",
        reason,
        surface.pipe.name,
        surface.plane_slot,
        alpha,
        ctl_before,
        ctl_enabled,
        color_ctl_before,
        color_ctl_enabled,
        decode_plane_color_alpha(color_ctl_before),
        decode_plane_color_alpha(color_ctl_enabled),
        pos_x,
        pos_y,
        surface.width,
        surface.height,
        stride_reg,
        keyval_after,
        keymsk_after,
        keymax_after,
        aux_dist_after,
        aux_offset_after,
        surf_before,
        crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF),
        live_before,
        live_after,
        frame_before,
        frame_after,
        frame_iters,
        live_iters
    );

    ok
}

pub(super) fn active_pipe(dev: crate::intel::Dev) -> Option<PipeInfo> {
    let mut enabled_plane = None;
    let mut observed = None;
    for pipe in PIPES {
        let pipe_src = crate::intel::mmio_read(dev, pipe.pipe_src_off);
        if decode_pipe_src(pipe_src).is_some() {
            return Some(pipe);
        }
        let plane_ctl = crate::intel::mmio_read(dev, pipe.primary_plane().ctl());
        let plane_surf = crate::intel::mmio_read(dev, pipe.primary_plane().surf());
        let plane_surf_live = crate::intel::mmio_read(dev, pipe.primary_plane().surf_live());
        if enabled_plane.is_none()
            && (plane_ctl & PLANE_CTL_ENABLE) != 0
            && (plane_surf != 0 || plane_surf_live != 0)
        {
            enabled_plane = Some(pipe);
        }
        if observed.is_none() && (plane_ctl != 0 || plane_surf != 0 || plane_surf_live != 0) {
            observed = Some(pipe);
        }
    }
    enabled_plane.or(observed)
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
        PLANE_COLOR_ALPHA_DISABLE => "disable",
        PLANE_COLOR_ALPHA_SW_PREMULT => "sw-premul",
        PLANE_COLOR_ALPHA_HW_PREMULT => "hw-premul",
        _ => "unknown",
    }
}

fn decode_plane_cus_phase(cus_ctl: u32, mask: u32, sign_bit: u32) -> i32 {
    let shift = mask.trailing_zeros();
    let magnitude = ((cus_ctl & mask) >> shift) as i32;
    if (cus_ctl & sign_bit) != 0 {
        -magnitude
    } else {
        magnitude
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
