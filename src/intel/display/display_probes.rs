use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use spin::Mutex;

use super::*;

const PRIMARY_REARM_RGB_PLANE_PROBE_ENABLED: bool = false;
const PRIMARY_REARM_RGB_PLANE_PROBE_SLOT_MASK: u8 = 1 << 2;
const RGB_PLANE_PROBE_SLOT_COUNT: usize = 3;
const RGB_PLANE_PROBE_GPU_BASE: u64 = crate::intel::GPU_VA_DISPLAY_OVERLAY_BASE;
const RGB_PLANE_PROBE_GPU_STRIDE: u64 = 0x0010_0000;
const DIRECT_NV12_PLANE_PROBE_ENABLED: bool = true;
const DIRECT_NV12_LINEAR_PATTERN_PROBE_ONLY: bool = false;
const DIRECT_NV12_LINEAR_PATTERN_GPU: u64 = 0x1200_0000;
const DIRECT_NV12_LINEAR_PATTERN_WIDTH: u32 = 640;
const DIRECT_NV12_LINEAR_PATTERN_HEIGHT: u32 = 360;
const DIRECT_NV12_DECODED_LINEAR_STAGING_ENABLED: bool = true;
const DIRECT_NV12_DECODED_LINEAR_STAGING_SCALE: u32 = 2;
const DIRECT_NV12_DECODED_LINEAR_STAGING_GPU: u64 = 0x1300_0000;
const DIRECT_NV12_DECODED_LINEAR_STAGING_COUNT: usize = 3;
const DIRECT_NV12_DECODED_LINEAR_STAGING_GPU_STRIDE: u64 = 0x0040_0000;
const DIRECT_NV12_INPUT_CSC_PROBE_ENABLED: bool = true;
const DIRECT_NV12_LINKED_PLANES_PROBE_ENABLED: bool = true;
const DIRECT_NV12_LINKED_PLANES_SURF_ONLY_FLIP: bool = true;
const DIRECT_NV12_PLANE_PROBE_CYCLE_CANDIDATES: bool = false;
const DIRECT_NV12_PLANE_PROBE_SLOT: usize = VIDEO_NV12_PLANE_SLOT;
const DIRECT_NV12_Y_PLANE_PROBE_SLOT: usize = VIDEO_NV12_Y_PLANE_SLOT;

static DIRECT_NV12_PLANE_PROBE_SEQ: AtomicU32 = AtomicU32::new(0);
static DIRECT_NV12_LINEAR_PATTERN_ARMED: AtomicBool = AtomicBool::new(false);
static RGB_PLANE_PROBE_SURFACES: Mutex<[Option<RgbPlaneProbeSurface>; RGB_PLANE_PROBE_SLOT_COUNT]> =
    Mutex::new([None; RGB_PLANE_PROBE_SLOT_COUNT]);
static DIRECT_NV12_LINEAR_PATTERN_SURFACE: Mutex<Option<Nv12PlaneProbeSurface>> = Mutex::new(None);
static DIRECT_NV12_DECODED_LINEAR_STAGING_SURFACES: Mutex<
    [Option<Nv12PlaneProbeSurface>; DIRECT_NV12_DECODED_LINEAR_STAGING_COUNT],
> = Mutex::new([None; DIRECT_NV12_DECODED_LINEAR_STAGING_COUNT]);
static DIRECT_NV12_DECODED_LINEAR_STAGING_NEXT: AtomicU32 = AtomicU32::new(0);

fn log_decoded_nv12_plane_alpha_program(
    seq: u32,
    phase: &str,
    reason: &str,
    owner: &str,
    pipe: PipeInfo,
    proof: DecodedNv12PlaneAlphaProgram,
) {
    if proof.alpha == 0xFF && seq > 3 {
        return;
    }
    crate::log!(
        "intel/display: nv12-linked-plane-alpha seq={} phase={} reason={} owner={} pipe={} alpha={} uv_slot={} y_slot={} uv_keymsk=0x{:08X}->0x{:08X} uv_keymax=0x{:08X}->0x{:08X} y_keymsk=0x{:08X}->0x{:08X} y_keymax=0x{:08X}->0x{:08X}\n",
        seq,
        phase,
        reason,
        owner,
        pipe.name,
        proof.alpha,
        DIRECT_NV12_PLANE_PROBE_SLOT,
        DIRECT_NV12_Y_PLANE_PROBE_SLOT,
        proof.uv_keymsk_before,
        proof.uv_keymsk_after,
        proof.uv_keymax_before,
        proof.uv_keymax_after,
        proof.y_keymsk_before,
        proof.y_keymsk_after,
        proof.y_keymax_before,
        proof.y_keymax_after
    );
}

#[derive(Copy, Clone)]
struct RgbPlaneProbeSurface {
    width: u32,
    height: u32,
    pitch_bytes: u32,
    phys: u64,
    virt: *mut u8,
    pipe: PipeInfo,
    plane_slot: usize,
    gpu: u64,
    color: u32,
}

unsafe impl Send for RgbPlaneProbeSurface {}
unsafe impl Sync for RgbPlaneProbeSurface {}

#[derive(Copy, Clone)]
struct Nv12PlaneProbeSurface {
    width: u32,
    height: u32,
    pitch_bytes: u32,
    uv_offset: usize,
    byte_len: usize,
    phys: u64,
    virt: *mut u8,
    pipe: PipeInfo,
    gpu: u64,
}

unsafe impl Send for Nv12PlaneProbeSurface {}
unsafe impl Sync for Nv12PlaneProbeSurface {}

pub(super) fn probe_boot_logo_decode() -> bool {
    match PRIMARY_BOOT_LOGO_DECODE_MODE {
        PrimaryBootLogoDecodeMode::HwPic => probe_hw_logo_decode(),
        PrimaryBootLogoDecodeMode::ZuneJpeg => probe_zune_boot_logo_decode(),
    }
}

fn probe_hw_logo_decode() -> bool {
    let submitted = submit_next_hw_logo_stage();
    crate::log!("intel/display: boot-logo decode mode=hw_pic submitted={}\n", submitted as u8);
    submitted
}

fn probe_zune_boot_logo_decode() -> bool {
    let decoded = match crate::ui3::img::jpeg_codec::decode_jpeg_rgba(PRIMARY_BOOT_LOGO_JPEG) {
        Ok(decoded) => decoded,
        Err(err) => {
            crate::log!(
                "intel/display: boot-logo decode mode=zune_jpeg failed code={} bytes=0x{:X}\n",
                err.code(),
                PRIMARY_BOOT_LOGO_JPEG.len()
            );
            return false;
        }
    };

    let stored = present_rgba_primary_center(
        decoded.rgba.as_slice(),
        decoded.width,
        decoded.height,
        decoded.width as usize * 4,
        "boot-logo-zune-jpeg-horizon-stamp-center",
    );
    let stamped = stored && stamp_horizon_logo_top_left_screen();
    let bgrt_stamped = stored && stamp_bgrt_logo_bottom_right_screen();
    crate::log!(
        "intel/display: boot-logo decode mode=zune_jpeg decoded={}x{} bytes=0x{:X} horizon_stamp={} bgrt_stamp={} stored={}\n",
        decoded.width,
        decoded.height,
        decoded.rgba.len(),
        stamped as u8,
        bgrt_stamped as u8,
        stored as u8
    );
    if stored {
        mark_hw_logo_sequence_done("zune-logo-presented");
    }
    stored
}

pub(crate) fn log_display_plane_ladder_probe(label: &str) {
    crate::log!("intel/display: display-ladder label={} stage=read-only begin\n", label);
    log_primary_surface_samples("display-ladder-primary");
    log_active_pipe_raw_state("display-ladder");
    log_pipe_live_scanout_state("display-ladder");
    log_display_power_well_snapshot("display-ladder");
    crate::log!("intel/display: display-ladder label={} stage=read-only end\n", label);
}

pub(super) fn probe_primary_present_psr(
    dev: crate::intel::Dev,
    surface: PrimarySurface,
    reason: &str,
    seq: u32,
) {
    // Skip PSR probe after the first two frames;
    // PSR is always 0x00000000 on this display pipeline.
    if seq > 2 {
        return;
    }

    crate::intel::ggtt_invalidate(dev);

    let trans_psr_ctl_off = TRANS_PSR_CTL_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let trans_psr_status_off =
        TRANS_PSR_STATUS_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let trans_psr2_ctl_off = TRANS_PSR2_CTL_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let trans_psr2_status_off =
        TRANS_PSR2_STATUS_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let psr_before = crate::intel::mmio_read(dev, trans_psr_ctl_off);
    let psr2_before = crate::intel::mmio_read(dev, trans_psr2_ctl_off);
    if PRIMARY_PRESENT_DISABLE_PSR_PROBE {
        crate::intel::mmio_write(dev, trans_psr_ctl_off, 0);
        crate::intel::mmio_write(dev, trans_psr2_ctl_off, 0);
        intel_display_verbose_log!(
            "intel/display: psr-probe reason={} pipe={} psr_ctl_before=0x{:08X} psr2_ctl_before=0x{:08X} psr_ctl_after=0x{:08X} psr2_ctl_after=0x{:08X} psr_status=0x{:08X} psr2_status=0x{:08X}\n",
            reason,
            surface.pipe.name,
            psr_before,
            psr2_before,
            crate::intel::mmio_read(dev, trans_psr_ctl_off),
            crate::intel::mmio_read(dev, trans_psr2_ctl_off),
            crate::intel::mmio_read(dev, trans_psr_status_off),
            crate::intel::mmio_read(dev, trans_psr2_status_off)
        );
    }
}

fn log_active_pipe_raw_state(label: &str) {
    let Some(dev) = crate::intel::claimed_device() else {
        return;
    };
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return;
    };
    let pipe = surface.pipe;
    let pipeconf_off = PIPECONF_A + pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let htotal_off = TRANS_HTOTAL_A + pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let hsync_off = TRANS_HSYNC_A + pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let vtotal_off = TRANS_VTOTAL_A + pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let vsync_off = TRANS_VSYNC_A + pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let ddi_func_ctl_off = TRANS_DDI_FUNC_CTL_A + pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let psr_ctl_off = TRANS_PSR_CTL_A + pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let psr_status_off = TRANS_PSR_STATUS_A + pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let psr2_ctl_off = TRANS_PSR2_CTL_A + pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let psr2_status_off = TRANS_PSR2_STATUS_A + pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let cur_surflive_off = CUR_SURFLIVE_A + pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let frmcount_off = PIPE_FRMCOUNT_A + pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let bottom_color_off =
        SKL_BOTTOM_COLOR_A + pipe.slot.saturating_mul(SKL_BOTTOM_COLOR_PIPE_STRIDE);

    crate::log!(
        "intel/display: pipe-raw label={} pipe={} pipe_src@0x{:05X}=0x{:08X} pipeconf@0x{:05X}=0x{:08X} frm@0x{:05X}=0x{:08X} cur_live@0x{:05X}=0x{:08X} bottom@0x{:05X}=0x{:08X} ddi_ctl@0x{:05X}=0x{:08X} htotal@0x{:05X}=0x{:08X} hsync@0x{:05X}=0x{:08X} vtotal@0x{:05X}=0x{:08X} vsync@0x{:05X}=0x{:08X} psr=0x{:08X}/0x{:08X} psr2=0x{:08X}/0x{:08X}\n",
        label,
        pipe.name,
        pipe.pipe_src_off,
        crate::intel::mmio_read(dev, pipe.pipe_src_off),
        pipeconf_off,
        crate::intel::mmio_read(dev, pipeconf_off),
        frmcount_off,
        crate::intel::mmio_read(dev, frmcount_off),
        cur_surflive_off,
        crate::intel::mmio_read(dev, cur_surflive_off),
        bottom_color_off,
        crate::intel::mmio_read(dev, bottom_color_off),
        ddi_func_ctl_off,
        crate::intel::mmio_read(dev, ddi_func_ctl_off),
        htotal_off,
        crate::intel::mmio_read(dev, htotal_off),
        hsync_off,
        crate::intel::mmio_read(dev, hsync_off),
        vtotal_off,
        crate::intel::mmio_read(dev, vtotal_off),
        vsync_off,
        crate::intel::mmio_read(dev, vsync_off),
        crate::intel::mmio_read(dev, psr_ctl_off),
        crate::intel::mmio_read(dev, psr_status_off),
        crate::intel::mmio_read(dev, psr2_ctl_off),
        crate::intel::mmio_read(dev, psr2_status_off)
    );
}

#[derive(Copy, Clone, Debug)]
enum DirectNv12PlaneTiling {
    Y,
    Yf,
    Linear,
}

impl DirectNv12PlaneTiling {
    const fn name(self) -> &'static str {
        match self {
            Self::Y => "y",
            Self::Yf => "yf",
            Self::Linear => "linear",
        }
    }

    const fn ctl_bits(self) -> u32 {
        match self {
            Self::Y => PLANE_CTL_TILED_Y,
            Self::Yf => PLANE_CTL_TILED_YF,
            Self::Linear => PLANE_CTL_TILED_LINEAR,
        }
    }
}

fn direct_nv12_probe_tiling_for_seq(seq: u32) -> DirectNv12PlaneTiling {
    if !DIRECT_NV12_PLANE_PROBE_CYCLE_CANDIDATES {
        return DirectNv12PlaneTiling::Linear;
    }
    match seq % 3 {
        1 => DirectNv12PlaneTiling::Y,
        2 => DirectNv12PlaneTiling::Yf,
        _ => DirectNv12PlaneTiling::Linear,
    }
}

fn direct_nv12_decoded_probe_tiling_for_seq(seq: u32) -> DirectNv12PlaneTiling {
    if DIRECT_NV12_PLANE_PROBE_CYCLE_CANDIDATES {
        direct_nv12_probe_tiling_for_seq(seq)
    } else {
        DirectNv12PlaneTiling::Y
    }
}

fn direct_nv12_plane_ctl_enabled(ctl_before: u32, tiling: DirectNv12PlaneTiling) -> u32 {
    (ctl_before
        & !(PLANE_CTL_ENABLE
            | PLANE_CTL_ARB_SLOTS_MASK
            | PLANE_CTL_FORMAT_MASK_SKL
            | PLANE_CTL_KEY_ENABLE_MASK
            | PLANE_CTL_TILED_MASK
            | PLANE_CTL_ORDER_RGBX
            | PLANE_CTL_YUV420_Y_PLANE
            | PLANE_CTL_ROTATE_MASK))
        | PLANE_CTL_ENABLE
        | PLANE_CTL_FORMAT_NV12
        | tiling.ctl_bits()
}

fn direct_nv12_y_plane_ctl_enabled(ctl_before: u32, tiling: DirectNv12PlaneTiling) -> u32 {
    direct_nv12_plane_ctl_enabled(ctl_before, tiling) | PLANE_CTL_YUV420_Y_PLANE
}

fn direct_nv12_plane_color_ctl_enabled(color_ctl_before: u32) -> u32 {
    let color_ctl = (color_ctl_before
        & !(PLANE_COLOR_ALPHA_MASK
            | PLANE_COLOR_YUV_RANGE_CORRECTION_DISABLE
            | PLANE_COLOR_PIPE_CSC_ENABLE
            | PLANE_COLOR_PLANE_CSC_ENABLE
            | PLANE_COLOR_INPUT_CSC_ENABLE
            | PLANE_COLOR_CSC_MODE_MASK))
        | PLANE_COLOR_PLANE_GAMMA_DISABLE
        | PLANE_COLOR_ALPHA_DISABLE;
    if DIRECT_NV12_INPUT_CSC_PROBE_ENABLED {
        color_ctl | PLANE_COLOR_INPUT_CSC_ENABLE
    } else {
        color_ctl | PLANE_COLOR_CSC_MODE_YUV709_TO_RGB709
    }
}

fn direct_nv12_plane_cus_ctl_enabled() -> u32 {
    if DIRECT_NV12_INPUT_CSC_PROBE_ENABLED {
        PLANE_CUS_ENABLE
            | PLANE_CUS_HPHASE_0
            | PLANE_CUS_VPHASE_SIGN_NEGATIVE
            | PLANE_CUS_VPHASE_0_25
    } else {
        0
    }
}

const fn plane_input_csc_coeff_value(index: usize) -> u32 {
    match index {
        0 => (0x7C98 << 16) | 0x7800,
        1 => 0x0000 << 16,
        2 => (0x9EF8 << 16) | 0x7800,
        3 => 0xAC00 << 16,
        4 => 0x0000 | 0x7800,
        5 => 0x7ED8 << 16,
        _ => 0,
    }
}

const fn plane_input_csc_preoff_value(index: usize) -> u32 {
    match index {
        0 => 0x1800,
        1 => 0x0000,
        2 => 0x1800,
        _ => 0,
    }
}

fn program_direct_nv12_input_csc(
    dev: crate::intel::Dev,
    plane_base: usize,
    pipe: PipeInfo,
    reason: &str,
    probe_name: &str,
    owner: &str,
) {
    if !DIRECT_NV12_INPUT_CSC_PROBE_ENABLED {
        return;
    }

    let coeff0_off = plane_base + UNI_PLANE_INPUT_CSC_COEFF_OFF;
    let coeff1_off = coeff0_off + 4;
    let coeff2_off = coeff0_off + 8;
    let coeff3_off = coeff0_off + 12;
    let coeff4_off = coeff0_off + 16;
    let coeff5_off = coeff0_off + 20;
    let pre0_off = plane_base + UNI_PLANE_INPUT_CSC_PREOFF_OFF;
    let pre1_off = pre0_off + 4;
    let pre2_off = pre0_off + 8;
    let post0_off = plane_base + UNI_PLANE_INPUT_CSC_POSTOFF_OFF;
    let post1_off = post0_off + 4;
    let post2_off = post0_off + 8;
    let c0_before = crate::intel::mmio_read(dev, coeff0_off);
    let c1_before = crate::intel::mmio_read(dev, coeff1_off);
    let c2_before = crate::intel::mmio_read(dev, coeff2_off);
    let c3_before = crate::intel::mmio_read(dev, coeff3_off);
    let c4_before = crate::intel::mmio_read(dev, coeff4_off);
    let c5_before = crate::intel::mmio_read(dev, coeff5_off);
    let pre0_before = crate::intel::mmio_read(dev, pre0_off);
    let pre1_before = crate::intel::mmio_read(dev, pre1_off);
    let pre2_before = crate::intel::mmio_read(dev, pre2_off);
    let post0_before = crate::intel::mmio_read(dev, post0_off);
    let post1_before = crate::intel::mmio_read(dev, post1_off);
    let post2_before = crate::intel::mmio_read(dev, post2_off);

    crate::intel::mmio_write(dev, coeff0_off, plane_input_csc_coeff_value(0));
    crate::intel::mmio_write(dev, coeff1_off, plane_input_csc_coeff_value(1));
    crate::intel::mmio_write(dev, coeff2_off, plane_input_csc_coeff_value(2));
    crate::intel::mmio_write(dev, coeff3_off, plane_input_csc_coeff_value(3));
    crate::intel::mmio_write(dev, coeff4_off, plane_input_csc_coeff_value(4));
    crate::intel::mmio_write(dev, coeff5_off, plane_input_csc_coeff_value(5));
    crate::intel::mmio_write(dev, pre0_off, plane_input_csc_preoff_value(0));
    crate::intel::mmio_write(dev, pre1_off, plane_input_csc_preoff_value(1));
    crate::intel::mmio_write(dev, pre2_off, plane_input_csc_preoff_value(2));
    crate::intel::mmio_write(dev, post0_off, 0);
    crate::intel::mmio_write(dev, post1_off, 0);
    crate::intel::mmio_write(dev, post2_off, 0);

    crate::log!(
        "intel/display: nv12-input-csc probe={} reason={} owner={} pipe={} slot={} coeff=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}]->[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pre=[0x{:08X},0x{:08X},0x{:08X}]->[0x{:08X},0x{:08X},0x{:08X}] post=[0x{:08X},0x{:08X},0x{:08X}]->[0x{:08X},0x{:08X},0x{:08X}]\n",
        probe_name,
        reason,
        owner,
        pipe.name,
        DIRECT_NV12_PLANE_PROBE_SLOT,
        c0_before,
        c1_before,
        c2_before,
        c3_before,
        c4_before,
        c5_before,
        crate::intel::mmio_read(dev, coeff0_off),
        crate::intel::mmio_read(dev, coeff1_off),
        crate::intel::mmio_read(dev, coeff2_off),
        crate::intel::mmio_read(dev, coeff3_off),
        crate::intel::mmio_read(dev, coeff4_off),
        crate::intel::mmio_read(dev, coeff5_off),
        pre0_before,
        pre1_before,
        pre2_before,
        crate::intel::mmio_read(dev, pre0_off),
        crate::intel::mmio_read(dev, pre1_off),
        crate::intel::mmio_read(dev, pre2_off),
        post0_before,
        post1_before,
        post2_before,
        crate::intel::mmio_read(dev, post0_off),
        crate::intel::mmio_read(dev, post1_off),
        crate::intel::mmio_read(dev, post2_off)
    );
}

fn fill_linear_nv12_pattern(
    ptr: *mut u8,
    pitch_bytes: usize,
    width: u32,
    height: u32,
    uv_offset: usize,
) {
    let width = width as usize;
    let height = height as usize;
    if width == 0 || height == 0 || pitch_bytes < width {
        return;
    }

    for y in 0..height {
        for x in 0..pitch_bytes {
            let value = if x < width {
                let bar = x.saturating_mul(8) / width.max(1);
                match bar {
                    0 => 32,
                    1 => 64,
                    2 => 96,
                    3 => 128,
                    4 => 160,
                    5 => 192,
                    6 => 224,
                    _ => 235,
                }
            } else {
                16
            };
            unsafe {
                core::ptr::write_volatile(ptr.add(y * pitch_bytes + x), value);
            }
        }
    }

    for y in 0..(height / 2) {
        for x in (0..pitch_bytes).step_by(2) {
            let (u, v) = if x < width {
                let top = y < height / 4;
                let left = x < width / 2;
                match (top, left) {
                    (true, true) => (128, 128),
                    (true, false) => (90, 240),
                    (false, true) => (240, 90),
                    (false, false) => (128, 128),
                }
            } else {
                (128, 128)
            };
            let offset = uv_offset + y * pitch_bytes + x;
            unsafe {
                core::ptr::write_volatile(ptr.add(offset), u);
                if x + 1 < pitch_bytes {
                    core::ptr::write_volatile(ptr.add(offset + 1), v);
                }
            }
        }
    }
}

fn sample_probe_byte(ptr: *const u8, byte_len: usize, offset: usize) -> u8 {
    if offset >= byte_len {
        return 0;
    }
    unsafe { core::ptr::read_volatile(ptr.add(offset)) }
}

fn sample_probe_pair(ptr: *const u8, byte_len: usize, offset: usize) -> (u8, u8) {
    (
        sample_probe_byte(ptr, byte_len, offset),
        sample_probe_byte(ptr, byte_len, offset.saturating_add(1)),
    )
}

fn log_nv12_probe_surface_samples(
    label: &str,
    ptr: *const u8,
    byte_len: usize,
    width: u32,
    height: u32,
    pitch_bytes: usize,
    uv_offset: usize,
) {
    let width = width as usize;
    let height = height as usize;
    if ptr.is_null() || byte_len == 0 || width == 0 || height == 0 || pitch_bytes == 0 {
        crate::log!(
            "intel/display: nv12-probe-samples label={} skipped ptr=0x{:X} bytes=0x{:X} size={}x{} pitch=0x{:X} uv=0x{:X}\n",
            label,
            ptr as usize,
            byte_len,
            width,
            height,
            pitch_bytes,
            uv_offset
        );
        return;
    }

    let sample_x = |x: usize| x.min(width.saturating_sub(1));
    let sample_y = |y: usize| y.min(height.saturating_sub(1));
    let y_off = |row: usize, x: usize| {
        sample_y(row)
            .saturating_mul(pitch_bytes)
            .saturating_add(sample_x(x))
    };
    let even_x = |x: usize| {
        let max_x = width.saturating_sub(2);
        x.min(max_x) & !1
    };
    let uv_off = |row: usize, x: usize| {
        uv_offset
            .saturating_add(row.min(height / 2).saturating_mul(pitch_bytes))
            .saturating_add(even_x(x))
    };

    let y0_0 = sample_probe_byte(ptr, byte_len, y_off(0, 0));
    let y0_1 = sample_probe_byte(ptr, byte_len, y_off(0, width / 4));
    let y0_2 = sample_probe_byte(ptr, byte_len, y_off(0, width / 2));
    let y0_3 = sample_probe_byte(ptr, byte_len, y_off(0, width.saturating_mul(3) / 4));
    let y0_4 = sample_probe_byte(ptr, byte_len, y_off(0, width.saturating_sub(1)));
    let ym_0 = sample_probe_byte(ptr, byte_len, y_off(height / 2, 0));
    let ym_1 = sample_probe_byte(ptr, byte_len, y_off(height / 2, width / 4));
    let ym_2 = sample_probe_byte(ptr, byte_len, y_off(height / 2, width / 2));
    let ym_3 = sample_probe_byte(ptr, byte_len, y_off(height / 2, width.saturating_mul(3) / 4));
    let ym_4 = sample_probe_byte(ptr, byte_len, y_off(height / 2, width.saturating_sub(1)));
    let (uv0_0_u, uv0_0_v) = sample_probe_pair(ptr, byte_len, uv_off(0, 0));
    let (uv0_1_u, uv0_1_v) = sample_probe_pair(ptr, byte_len, uv_off(0, width / 2));
    let (uvm_0_u, uvm_0_v) = sample_probe_pair(ptr, byte_len, uv_off(height / 4, 0));
    let (uvm_1_u, uvm_1_v) = sample_probe_pair(ptr, byte_len, uv_off(height / 4, width / 2));

    crate::log!(
        "intel/display: nv12-probe-samples label={} ptr=0x{:X} bytes=0x{:X} size={}x{} pitch=0x{:X} uv=0x{:X} y0=[0x{:02X},0x{:02X},0x{:02X},0x{:02X},0x{:02X}] ym=[0x{:02X},0x{:02X},0x{:02X},0x{:02X},0x{:02X}] uv0=[0x{:02X}/0x{:02X},0x{:02X}/0x{:02X}] uvm=[0x{:02X}/0x{:02X},0x{:02X}/0x{:02X}]\n",
        label,
        ptr as usize,
        byte_len,
        width,
        height,
        pitch_bytes,
        uv_offset,
        y0_0,
        y0_1,
        y0_2,
        y0_3,
        y0_4,
        ym_0,
        ym_1,
        ym_2,
        ym_3,
        ym_4,
        uv0_0_u,
        uv0_0_v,
        uv0_1_u,
        uv0_1_v,
        uvm_0_u,
        uvm_0_v,
        uvm_1_u,
        uvm_1_v
    );
}

fn ensure_linear_nv12_pattern_surface(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
) -> Option<Nv12PlaneProbeSurface> {
    let width = DIRECT_NV12_LINEAR_PATTERN_WIDTH;
    let height = DIRECT_NV12_LINEAR_PATTERN_HEIGHT;
    let gpu = DIRECT_NV12_LINEAR_PATTERN_GPU;

    {
        let state = DIRECT_NV12_LINEAR_PATTERN_SURFACE.lock();
        if let Some(surface) = *state
            && surface.width == width
            && surface.height == height
            && surface.pipe.slot == pipe.slot
            && surface.gpu == gpu
        {
            return Some(surface);
        }
    }

    let pitch_bytes = aligned_pitch_bytes(width, 1)?;
    let raw_uv_offset = usize::try_from(u64::from(pitch_bytes) * u64::from(height)).ok()?;
    let uv_offset = crate::intel::align_up(raw_uv_offset, crate::intel::WARM_ALIGN)?;
    let uv_bytes = usize::try_from(u64::from(pitch_bytes) * u64::from(height / 2)).ok()?;
    let byte_len = uv_offset.checked_add(uv_bytes)?;
    let (phys, virt) = crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN)?;
    fill_linear_nv12_pattern(virt, pitch_bytes as usize, width, height, uv_offset);
    crate::intel::dma_flush(virt, byte_len);
    log_nv12_probe_surface_samples(
        "nv12-linear-pattern-filled",
        virt,
        byte_len,
        width,
        height,
        pitch_bytes as usize,
        uv_offset,
    );

    if !crate::intel::map_display_scanout_ggtt(dev, phys, byte_len, gpu) {
        crate::log!(
            "intel/display: nv12-linear-pattern-surface ggtt map failed pipe={} size={}x{} pitch=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X}\n",
            pipe.name,
            width,
            height,
            pitch_bytes,
            byte_len,
            gpu,
            phys
        );
        return None;
    }
    crate::intel::ggtt_invalidate(dev);

    let surface = Nv12PlaneProbeSurface {
        width,
        height,
        pitch_bytes,
        uv_offset,
        byte_len,
        phys,
        virt,
        pipe,
        gpu,
    };
    *DIRECT_NV12_LINEAR_PATTERN_SURFACE.lock() = Some(surface);
    crate::log!(
        "intel/display: nv12-linear-pattern-surface pipe={} size={}x{} pitch=0x{:X} uv=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X} virt=0x{:X}\n",
        pipe.name,
        width,
        height,
        pitch_bytes,
        uv_offset,
        byte_len,
        gpu,
        phys,
        virt as usize
    );
    Some(surface)
}

fn arm_linear_nv12_pattern_video_plane_probe(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    reason: &str,
) -> bool {
    if DIRECT_NV12_LINEAR_PATTERN_ARMED.load(Ordering::Acquire) {
        return true;
    }
    let Some(surface) = ensure_linear_nv12_pattern_surface(dev, pipe) else {
        return false;
    };
    crate::intel::dma_flush(surface.virt, surface.byte_len);
    log_nv12_probe_surface_samples(
        "nv12-linear-pattern-arm",
        surface.virt,
        surface.byte_len,
        surface.width,
        surface.height,
        surface.pitch_bytes as usize,
        surface.uv_offset,
    );
    let armed = arm_nv12_video_plane_probe_surface(
        "nv12-linear-pattern",
        "linear-pattern",
        reason,
        surface.gpu,
        surface.phys,
        surface.virt as usize,
        surface.width,
        surface.height,
        surface.width,
        surface.height,
        surface.pitch_bytes as usize,
        surface.uv_offset,
        surface.byte_len,
        DirectNv12PlaneTiling::Linear,
    );
    if armed {
        DIRECT_NV12_LINEAR_PATTERN_ARMED.store(true, Ordering::Release);
    }
    armed
}

fn ensure_decoded_linear_nv12_staging_surface(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    width: u32,
    height: u32,
) -> Option<(Nv12PlaneProbeSurface, usize)> {
    let slot = (DIRECT_NV12_DECODED_LINEAR_STAGING_NEXT.fetch_add(1, Ordering::AcqRel) as usize)
        % DIRECT_NV12_DECODED_LINEAR_STAGING_COUNT;
    let gpu = DIRECT_NV12_DECODED_LINEAR_STAGING_GPU
        .checked_add((slot as u64).checked_mul(DIRECT_NV12_DECODED_LINEAR_STAGING_GPU_STRIDE)?)?;
    {
        let state = DIRECT_NV12_DECODED_LINEAR_STAGING_SURFACES.lock();
        if let Some(surface) = state[slot]
            && surface.width == width
            && surface.height == height
            && surface.pipe.slot == pipe.slot
            && surface.gpu == gpu
        {
            return Some((surface, slot));
        }
    }

    let pitch_bytes = aligned_pitch_bytes(width, 1)?;
    let raw_uv_offset = usize::try_from(u64::from(pitch_bytes) * u64::from(height)).ok()?;
    let uv_offset = crate::intel::align_up(raw_uv_offset, crate::intel::WARM_ALIGN)?;
    let uv_rows = height.div_ceil(2);
    let uv_bytes = usize::try_from(u64::from(pitch_bytes) * u64::from(uv_rows)).ok()?;
    let byte_len = uv_offset.checked_add(uv_bytes)?;
    let (phys, virt) = crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 16, raw_uv_offset);
        core::ptr::write_bytes(virt.add(uv_offset), 128, uv_bytes);
    }
    crate::intel::dma_flush(virt, byte_len);

    if !crate::intel::map_display_scanout_ggtt(dev, phys, byte_len, gpu) {
        crate::log!(
            "intel/display: decoded-nv12-linear-staging ggtt map failed pipe={} stage_slot={} size={}x{} pitch=0x{:X} uv=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X}\n",
            pipe.name,
            slot,
            width,
            height,
            pitch_bytes,
            uv_offset,
            byte_len,
            gpu,
            phys
        );
        return None;
    }
    crate::intel::ggtt_invalidate(dev);

    let surface = Nv12PlaneProbeSurface {
        width,
        height,
        pitch_bytes,
        uv_offset,
        byte_len,
        phys,
        virt,
        pipe,
        gpu,
    };
    DIRECT_NV12_DECODED_LINEAR_STAGING_SURFACES.lock()[slot] = Some(surface);
    crate::log!(
        "intel/display: decoded-nv12-linear-staging allocated pipe={} stage_slot={} size={}x{} pitch=0x{:X} uv=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X} virt=0x{:X}\n",
        pipe.name,
        slot,
        width,
        height,
        pitch_bytes,
        uv_offset,
        byte_len,
        gpu,
        phys,
        virt as usize
    );
    Some((surface, slot))
}

#[inline(always)]
fn decoded_nv12_ytile_8bpp_offset(byte_x: usize, row_y: usize, tiles_per_row: usize) -> usize {
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

fn copy_decoded_ytile_nv12_to_linear_staging(
    src_virt: usize,
    src_byte_len: usize,
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    src_uv_offset: usize,
    dst: Nv12PlaneProbeSurface,
    scale: u32,
) -> bool {
    let scale = scale.max(1) as usize;
    if src_virt == 0
        || src_byte_len == 0
        || src_width == 0
        || src_height == 0
        || src_pitch_bytes < src_width as usize
        || !src_pitch_bytes.is_multiple_of(128)
        || src_uv_offset < src_pitch_bytes.saturating_mul(src_height as usize)
        || !src_uv_offset.is_multiple_of(src_pitch_bytes)
        || dst.virt.is_null()
        || dst.width as usize != (src_width as usize).saturating_mul(scale)
        || dst.height as usize != (src_height as usize).saturating_mul(scale)
    {
        return false;
    }

    let src_uv_row = src_uv_offset / src_pitch_bytes;
    let src_tiles_per_row = src_pitch_bytes / 128;
    let dst_pitch = dst.pitch_bytes as usize;
    if dst_pitch < dst.width as usize {
        return false;
    }

    let src = unsafe { core::slice::from_raw_parts(src_virt as *const u8, src_byte_len) };
    let width = src_width as usize;
    let height = src_height as usize;
    for row in 0..height {
        for col in 0..width {
            let src_off = decoded_nv12_ytile_8bpp_offset(col, row, src_tiles_per_row);
            let Some(&value) = src.get(src_off) else {
                return false;
            };
            let dst_y0 = row.saturating_mul(scale);
            let dst_x0 = col.saturating_mul(scale);
            for dy in 0..scale {
                let dst_row = unsafe {
                    dst.virt
                        .add(dst_y0.saturating_add(dy).saturating_mul(dst_pitch))
                };
                for dx in 0..scale {
                    unsafe {
                        core::ptr::write_volatile(dst_row.add(dst_x0.saturating_add(dx)), value);
                    }
                }
            }
        }
    }

    let uv_rows = height.div_ceil(2);
    for row in 0..uv_rows {
        let src_row = src_uv_row.saturating_add(row);
        for col in (0..width).step_by(2) {
            let u_off = decoded_nv12_ytile_8bpp_offset(col, src_row, src_tiles_per_row);
            let v_off =
                decoded_nv12_ytile_8bpp_offset(col.saturating_add(1), src_row, src_tiles_per_row);
            let (Some(&u), Some(&v)) = (src.get(u_off), src.get(v_off)) else {
                return false;
            };
            let dst_uv_y0 = row.saturating_mul(scale);
            let dst_chroma_x0 = (col / 2).saturating_mul(scale);
            for dy in 0..scale {
                let dst_row = unsafe {
                    dst.virt.add(
                        dst.uv_offset
                            .saturating_add(dst_uv_y0.saturating_add(dy).saturating_mul(dst_pitch)),
                    )
                };
                for dx in 0..scale {
                    let dst_col = dst_chroma_x0.saturating_add(dx).saturating_mul(2);
                    unsafe {
                        core::ptr::write_volatile(dst_row.add(dst_col), u);
                        core::ptr::write_volatile(dst_row.add(dst_col.saturating_add(1)), v);
                    }
                }
            }
        }
    }

    crate::intel::dma_flush(dst.virt, dst.byte_len);
    true
}

fn log_linear_nv12_green_probe(
    reason: &str,
    pipe: PipeInfo,
    stage_slot: usize,
    surface: Nv12PlaneProbeSurface,
    visible_width: u32,
    visible_height: u32,
) {
    if surface.virt.is_null()
        || surface.pitch_bytes == 0
        || surface.uv_offset >= surface.byte_len
        || visible_width < 2
        || visible_height < 2
    {
        return;
    }

    let width = visible_width.min(surface.width) as usize;
    let height = visible_height.min(surface.height) as usize;
    let pitch = surface.pitch_bytes as usize;
    let cols = 8usize;
    let rows = 6usize;
    let mut samples = 0usize;
    let mut greenish = 0usize;
    let mut y_sum = 0usize;
    let mut u_sum = 0usize;
    let mut v_sum = 0usize;
    let mut y_min = u8::MAX;
    let mut y_max = u8::MIN;
    let mut u_min = u8::MAX;
    let mut u_max = u8::MIN;
    let mut v_min = u8::MAX;
    let mut v_max = u8::MIN;

    for row in 0..rows {
        let y = ((row + 1) * height / (rows + 1)).min(height.saturating_sub(1));
        for col in 0..cols {
            let x = ((col + 1) * width / (cols + 1)).min(width.saturating_sub(1));
            let uv_x = x & !1;
            let uv_y = y / 2;
            let y_off = y.saturating_mul(pitch).saturating_add(x);
            let uv_off = surface
                .uv_offset
                .saturating_add(uv_y.saturating_mul(pitch))
                .saturating_add(uv_x);
            if y_off >= surface.byte_len || uv_off.saturating_add(1) >= surface.byte_len {
                continue;
            }
            let y_value = unsafe { core::ptr::read_volatile(surface.virt.add(y_off)) };
            let u_value = unsafe { core::ptr::read_volatile(surface.virt.add(uv_off)) };
            let v_value = unsafe { core::ptr::read_volatile(surface.virt.add(uv_off + 1)) };
            samples += 1;
            y_sum = y_sum.saturating_add(y_value as usize);
            u_sum = u_sum.saturating_add(u_value as usize);
            v_sum = v_sum.saturating_add(v_value as usize);
            y_min = y_min.min(y_value);
            y_max = y_max.max(y_value);
            u_min = u_min.min(u_value);
            u_max = u_max.max(u_value);
            v_min = v_min.min(v_value);
            v_max = v_max.max(v_value);
            if y_value > 48 && u_value < 112 && v_value < 112 {
                greenish += 1;
            }
        }
    }

    if samples != 0 && greenish.saturating_mul(4) >= samples.saturating_mul(3) {
        crate::log!(
            "intel/display: decoded-nv12-green-suspect reason={} pipe={} stage_slot={} samples={} greenish={} y_avg={} u_avg={} v_avg={} y_range={}..{} u_range={}..{} v_range={}..{} gpu=0x{:X} uv_gpu=0x{:X}\n",
            reason,
            pipe.name,
            stage_slot,
            samples,
            greenish,
            y_sum / samples,
            u_sum / samples,
            v_sum / samples,
            y_min,
            y_max,
            u_min,
            u_max,
            v_min,
            v_max,
            surface.gpu,
            surface.gpu.saturating_add(surface.uv_offset as u64)
        );
    }
}

fn arm_decoded_linear_nv12_staging_video_plane_probe(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    reason: &str,
    src_gpu_addr: u64,
    src_phys_addr: u64,
    src_virt_addr: usize,
    coded_width: u32,
    coded_height: u32,
    visible_width: u32,
    visible_height: u32,
    src_pitch_bytes: usize,
    src_uv_offset: usize,
    src_byte_len: usize,
) -> Option<bool> {
    let (scanout_w, scanout_h) = active_scanout_dimensions().unwrap_or((coded_width, coded_height));
    let scale = if DIRECT_NV12_DECODED_LINEAR_STAGING_SCALE > 1
        && coded_width
            .checked_mul(DIRECT_NV12_DECODED_LINEAR_STAGING_SCALE)
            .is_some_and(|width| width <= scanout_w)
        && coded_height
            .checked_mul(DIRECT_NV12_DECODED_LINEAR_STAGING_SCALE)
            .is_some_and(|height| height <= scanout_h)
    {
        DIRECT_NV12_DECODED_LINEAR_STAGING_SCALE
    } else {
        1
    };
    let dst_width = coded_width.checked_mul(scale)?;
    let dst_height = coded_height.checked_mul(scale)?;
    let dst_visible_width = visible_width.checked_mul(scale)?;
    let dst_visible_height = visible_height.checked_mul(scale)?;
    let (staging, staging_slot) =
        ensure_decoded_linear_nv12_staging_surface(dev, pipe, dst_width, dst_height)?;
    let copied = copy_decoded_ytile_nv12_to_linear_staging(
        src_virt_addr,
        src_byte_len,
        coded_width,
        coded_height,
        src_pitch_bytes,
        src_uv_offset,
        staging,
        scale,
    );
    crate::log!(
        "intel/display: decoded-nv12-linear-staging copy reason={} pipe={} stage_slot={} copied={} scale={} src_layout=ytile src_gpu=0x{:X} src_phys=0x{:X} src_virt=0x{:X} src_size={}x{} src_visible={}x{} src_pitch=0x{:X} src_uv=0x{:X} src_bytes=0x{:X} dst_gpu=0x{:X} dst_phys=0x{:X} dst_virt=0x{:X} dst_size={}x{} dst_visible={}x{} dst_pitch=0x{:X} dst_uv=0x{:X} dst_bytes=0x{:X}\n",
        reason,
        pipe.name,
        staging_slot,
        copied as u8,
        scale,
        src_gpu_addr,
        src_phys_addr,
        src_virt_addr,
        coded_width,
        coded_height,
        visible_width,
        visible_height,
        src_pitch_bytes,
        src_uv_offset,
        src_byte_len,
        staging.gpu,
        staging.phys,
        staging.virt as usize,
        staging.width,
        staging.height,
        dst_visible_width,
        dst_visible_height,
        staging.pitch_bytes,
        staging.uv_offset,
        staging.byte_len
    );
    if !copied {
        return Some(false);
    }

    log_linear_nv12_green_probe(
        reason,
        pipe,
        staging_slot,
        staging,
        dst_visible_width,
        dst_visible_height,
    );

    Some(arm_nv12_video_plane_probe_surface(
        "decoded-nv12-linear-staging",
        "video-nv12-staging",
        reason,
        staging.gpu,
        staging.phys,
        staging.virt as usize,
        staging.width,
        staging.height,
        dst_visible_width,
        dst_visible_height,
        staging.pitch_bytes as usize,
        staging.uv_offset,
        staging.byte_len,
        DirectNv12PlaneTiling::Linear,
    ))
}

fn arm_nv12_video_plane_probe_surface(
    probe_name: &str,
    owner: &str,
    reason: &str,
    gpu_addr: u64,
    phys_addr: u64,
    virt_addr: usize,
    coded_width: u32,
    coded_height: u32,
    visible_width: u32,
    visible_height: u32,
    pitch_bytes: usize,
    uv_offset: usize,
    byte_len: usize,
    tiling: DirectNv12PlaneTiling,
) -> bool {
    if DIRECT_NV12_LINKED_PLANES_PROBE_ENABLED {
        return arm_nv12_linked_video_plane_probe_surface(
            probe_name,
            owner,
            reason,
            gpu_addr,
            phys_addr,
            virt_addr,
            coded_width,
            coded_height,
            visible_width,
            visible_height,
            pitch_bytes,
            uv_offset,
            byte_len,
            tiling,
        );
    }

    if !DIRECT_NV12_PLANE_PROBE_ENABLED {
        return false;
    }
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(pipe) = active_pipe(dev) else {
        return false;
    };
    if gpu_addr == 0 || coded_width == 0 || coded_height == 0 || pitch_bytes == 0 {
        crate::log!(
            "intel/display: nv12-plane-probe skipped probe={} reason={} owner={} cause=bad-surface gpu=0x{:X} coded={}x{} visible={}x{} pitch=0x{:X} uv=0x{:X} bytes=0x{:X}\n",
            probe_name,
            reason,
            owner,
            gpu_addr,
            coded_width,
            coded_height,
            visible_width,
            visible_height,
            pitch_bytes,
            uv_offset,
            byte_len
        );
        return false;
    }
    let Some(surface_reg) = u32::try_from(gpu_addr).ok() else {
        return false;
    };
    let Some(stride_reg) = u32::try_from(pitch_bytes)
        .ok()
        .and_then(plane_stride_reg_value)
    else {
        return false;
    };

    let plane_width = coded_width;
    let plane_height = coded_height;
    let (scanout_w, scanout_h) = active_scanout_dimensions().unwrap_or((plane_width, plane_height));
    let pos_x = scanout_w.saturating_sub(plane_width) / 2;
    let pos_y = scanout_h.saturating_sub(plane_height) / 2;
    let plane_base = overlay_plane_base(pipe, DIRECT_NV12_PLANE_PROBE_SLOT);
    let seq = DIRECT_NV12_PLANE_PROBE_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    let uv_rows = if pitch_bytes == 0 {
        0
    } else {
        uv_offset / pitch_bytes
    };
    let Some(uv_y_offset) = u32::try_from(uv_rows).ok() else {
        crate::log!(
            "intel/display: nv12-plane-probe skipped probe={} reason={} owner={} cause=bad-uv-offset pitch=0x{:X} uv=0x{:X} uv_rows={}\n",
            probe_name,
            reason,
            owner,
            pitch_bytes,
            uv_offset,
            uv_rows
        );
        return false;
    };

    program_three_plane_stack_resources(dev, pipe, reason);

    let ctl_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CTL_OFF);
    let stride_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_STRIDE_OFF);
    let pos_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_POS_OFF);
    let size_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SIZE_OFF);
    let surf_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF);
    let live_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);
    let color_ctl_off = plane_base + UNI_PLANE_COLOR_CTL_OFF;
    let color_ctl_before = crate::intel::mmio_read(dev, color_ctl_off);
    let cus_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CUS_CTL_OFF);
    let aux_dist_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_AUX_DIST_OFF);
    let aux_offset_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_AUX_OFFSET_OFF);
    let ctl_disabled = ctl_before & !PLANE_CTL_ENABLE;
    let ctl_enabled = direct_nv12_plane_ctl_enabled(ctl_before, tiling);
    let color_ctl_enabled = direct_nv12_plane_color_ctl_enabled(color_ctl_before);
    let cus_ctl_enabled = direct_nv12_plane_cus_ctl_enabled();

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
        plane_size_reg_value(plane_width, plane_height),
    );
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_KEYVAL_OFF, 0);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_KEYMSK_OFF, 0);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_KEYMAX_OFF, 0);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_OFFSET_OFF, plane_pos_reg_value(0, 0));
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_AUX_DIST_OFF, 0);
    crate::intel::mmio_write(
        dev,
        plane_base + UNI_PLANE_AUX_OFFSET_OFF,
        plane_pos_reg_value(0, uv_y_offset),
    );
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_CUS_CTL_OFF, cus_ctl_enabled);
    crate::intel::mmio_write(dev, color_ctl_off, color_ctl_enabled);
    program_direct_nv12_input_csc(dev, plane_base, pipe, reason, probe_name, owner);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_CTL_OFF, ctl_enabled);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_SURF_OFF, surface_reg);

    let (frame_before, frame_after, frame_iters) = wait_for_pipe_next_frame(dev, pipe);
    let (live_after, live_iters) = wait_for_plane_live(dev, plane_base, surface_reg, 20_000);
    let ctl_after = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CTL_OFF);
    let stride_after = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_STRIDE_OFF);
    let pos_after = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_POS_OFF);
    let size_after = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SIZE_OFF);
    let surf_after = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF);
    let color_ctl_after = crate::intel::mmio_read(dev, color_ctl_off);
    let cus_after = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CUS_CTL_OFF);
    let aux_dist_after = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_AUX_DIST_OFF);
    let aux_offset_after = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_AUX_OFFSET_OFF);
    let ok = live_after == surface_reg;

    crate::log!(
        "intel/display: nv12-plane-probe seq={} probe={} reason={} owner={} pipe={} slot={} candidate={} ok={} ctl=0x{:08X}->0x{:08X}/0x{:08X} format={} tiled={} stride=0x{:08X}->0x{:08X}/0x{:08X} pos=0x{:08X}->0x{:08X}({}x{}) size=0x{:08X}->0x{:08X}({}x{}) offset=0x{:08X} surf=0x{:08X}->0x{:08X} live=0x{:08X}->0x{:08X} color=0x{:08X}->0x{:08X} alpha={} cus=0x{:08X}->0x{:08X} aux=0x{:08X}/0x{:08X}->0x{:08X}/0x{:08X} coded={}x{} visible={}x{} pitch=0x{:X} uv=0x{:X} uv_rows={} bytes=0x{:X} gpu=0x{:X} phys=0x{:X} virt=0x{:X} scanout={}x{} frame={}=>{} frame_wait={} live_iters={}\n",
        seq,
        probe_name,
        reason,
        owner,
        pipe.name,
        DIRECT_NV12_PLANE_PROBE_SLOT,
        tiling.name(),
        ok as u8,
        ctl_before,
        ctl_enabled,
        ctl_after,
        decode_plane_format(ctl_after),
        decode_plane_tiling(ctl_after),
        stride_before,
        stride_reg,
        stride_after,
        pos_before,
        pos_after,
        pos_x,
        pos_y,
        size_before,
        size_after,
        plane_width,
        plane_height,
        crate::intel::mmio_read(dev, plane_base + UNI_PLANE_OFFSET_OFF),
        surf_before,
        surf_after,
        live_before,
        live_after,
        color_ctl_before,
        color_ctl_after,
        decode_plane_color_alpha(color_ctl_after),
        cus_before,
        cus_after,
        aux_dist_before,
        aux_offset_before,
        aux_dist_after,
        aux_offset_after,
        coded_width,
        coded_height,
        visible_width,
        visible_height,
        pitch_bytes,
        uv_offset,
        uv_rows,
        byte_len,
        gpu_addr,
        phys_addr,
        virt_addr,
        scanout_w,
        scanout_h,
        frame_before,
        frame_after,
        frame_iters,
        live_iters
    );

    ok
}

fn arm_nv12_linked_video_plane_probe_surface(
    probe_name: &str,
    owner: &str,
    reason: &str,
    gpu_addr: u64,
    phys_addr: u64,
    virt_addr: usize,
    coded_width: u32,
    coded_height: u32,
    visible_width: u32,
    visible_height: u32,
    pitch_bytes: usize,
    uv_offset: usize,
    byte_len: usize,
    tiling: DirectNv12PlaneTiling,
) -> bool {
    if !DIRECT_NV12_PLANE_PROBE_ENABLED {
        return false;
    }
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(pipe) = active_pipe(dev) else {
        return false;
    };
    if gpu_addr == 0 || coded_width == 0 || coded_height == 0 || pitch_bytes == 0 {
        crate::log!(
            "intel/display: nv12-linked-plane-probe skipped probe={} reason={} owner={} cause=bad-surface gpu=0x{:X} coded={}x{} visible={}x{} pitch=0x{:X} uv=0x{:X} bytes=0x{:X}\n",
            probe_name,
            reason,
            owner,
            gpu_addr,
            coded_width,
            coded_height,
            visible_width,
            visible_height,
            pitch_bytes,
            uv_offset,
            byte_len
        );
        return false;
    }

    let Some(y_surface_reg) = u32::try_from(gpu_addr).ok() else {
        return false;
    };
    let Some(uv_gpu_addr) = gpu_addr.checked_add(uv_offset as u64) else {
        return false;
    };
    let Some(uv_surface_reg) = u32::try_from(uv_gpu_addr).ok() else {
        return false;
    };
    let Some(stride_reg) = u32::try_from(pitch_bytes)
        .ok()
        .and_then(plane_stride_reg_value)
    else {
        return false;
    };

    let plane_width = coded_width;
    let plane_height = coded_height;
    let (scanout_w, scanout_h) = active_scanout_dimensions().unwrap_or((plane_width, plane_height));
    let pos_x = scanout_w.saturating_sub(plane_width) / 2;
    let pos_y = scanout_h.saturating_sub(plane_height) / 2;
    let uv_base = overlay_plane_base(pipe, DIRECT_NV12_PLANE_PROBE_SLOT);
    let y_base = overlay_plane_base(pipe, DIRECT_NV12_Y_PLANE_PROBE_SLOT);
    let seq = DIRECT_NV12_PLANE_PROBE_SEQ.fetch_add(1, Ordering::AcqRel) + 1;

    program_three_plane_stack_resources(dev, pipe, reason);

    let uv_ctl_before = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_CTL_OFF);
    let uv_stride_before = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_STRIDE_OFF);
    let uv_surf_before = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_SURF_OFF);
    let uv_live_before = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_SURFLIVE_OFF);
    let uv_color_ctl_off = uv_base + UNI_PLANE_COLOR_CTL_OFF;
    let uv_color_before = crate::intel::mmio_read(dev, uv_color_ctl_off);
    let uv_cus_before = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_CUS_CTL_OFF);
    let y_ctl_before = crate::intel::mmio_read(dev, y_base + UNI_PLANE_CTL_OFF);
    let y_stride_before = crate::intel::mmio_read(dev, y_base + UNI_PLANE_STRIDE_OFF);
    let y_surf_before = crate::intel::mmio_read(dev, y_base + UNI_PLANE_SURF_OFF);
    let y_live_before = crate::intel::mmio_read(dev, y_base + UNI_PLANE_SURFLIVE_OFF);
    let y_color_ctl_off = y_base + UNI_PLANE_COLOR_CTL_OFF;
    let y_color_before = crate::intel::mmio_read(dev, y_color_ctl_off);
    let y_cus_before = crate::intel::mmio_read(dev, y_base + UNI_PLANE_CUS_CTL_OFF);
    let uv_ctl_enabled = direct_nv12_plane_ctl_enabled(uv_ctl_before, tiling);
    let y_ctl_enabled = direct_nv12_y_plane_ctl_enabled(y_ctl_before, tiling);
    let uv_color_enabled = direct_nv12_plane_color_ctl_enabled(uv_color_before);
    let y_color_enabled = (y_color_before
        & !(PLANE_COLOR_ALPHA_MASK
            | PLANE_COLOR_YUV_RANGE_CORRECTION_DISABLE
            | PLANE_COLOR_PIPE_CSC_ENABLE
            | PLANE_COLOR_PLANE_CSC_ENABLE
            | PLANE_COLOR_INPUT_CSC_ENABLE
            | PLANE_COLOR_CSC_MODE_MASK))
        | PLANE_COLOR_PLANE_GAMMA_DISABLE
        | PLANE_COLOR_ALPHA_DISABLE
        | PLANE_COLOR_CSC_MODE_YUV709_TO_RGB709;
    let uv_cus_enabled = direct_nv12_plane_cus_ctl_enabled();
    let want_pos = plane_pos_reg_value(pos_x, pos_y);
    let want_size = plane_size_reg_value(plane_width, plane_height);

    if DIRECT_NV12_LINKED_PLANES_SURF_ONLY_FLIP
        && (uv_ctl_before & PLANE_CTL_ENABLE) != 0
        && (y_ctl_before & PLANE_CTL_ENABLE) != 0
        && uv_stride_before == stride_reg
        && y_stride_before == stride_reg
        && crate::intel::mmio_read(dev, uv_base + UNI_PLANE_POS_OFF) == want_pos
        && crate::intel::mmio_read(dev, y_base + UNI_PLANE_POS_OFF) == want_pos
        && crate::intel::mmio_read(dev, uv_base + UNI_PLANE_SIZE_OFF) == want_size
        && crate::intel::mmio_read(dev, y_base + UNI_PLANE_SIZE_OFF) == want_size
    {
        let alpha_proof = program_decoded_nv12_overlay_plane_alpha(dev, uv_base, y_base);
        crate::intel::mmio_write(dev, uv_base + UNI_PLANE_SURF_OFF, uv_surface_reg);
        crate::intel::mmio_write(dev, y_base + UNI_PLANE_SURF_OFF, y_surface_reg);

        let (frame_before, frame_after, frame_iters) = wait_for_pipe_next_frame(dev, pipe);
        let (uv_live_after, uv_live_iters) =
            wait_for_plane_live(dev, uv_base, uv_surface_reg, 20_000);
        let (y_live_after, y_live_iters) = wait_for_plane_live(dev, y_base, y_surface_reg, 20_000);
        let ok = uv_live_after == uv_surface_reg && y_live_after == y_surface_reg;
        log_decoded_nv12_plane_alpha_program(seq, "flip", reason, owner, pipe, alpha_proof);

        crate::log!(
            "intel/display: nv12-linked-plane-flip seq={} probe={} reason={} owner={} pipe={} ok={} uv_slot={} y_slot={} candidate={} uv_surf=0x{:08X}->0x{:08X} uv_live=0x{:08X}->0x{:08X} y_surf=0x{:08X}->0x{:08X} y_live=0x{:08X}->0x{:08X} y_gpu=0x{:X} uv_gpu=0x{:X} frame={}=>{} frame_wait={} uv_live_iters={} y_live_iters={}\n",
            seq,
            probe_name,
            reason,
            owner,
            pipe.name,
            ok as u8,
            DIRECT_NV12_PLANE_PROBE_SLOT,
            DIRECT_NV12_Y_PLANE_PROBE_SLOT,
            tiling.name(),
            uv_surf_before,
            crate::intel::mmio_read(dev, uv_base + UNI_PLANE_SURF_OFF),
            uv_live_before,
            uv_live_after,
            y_surf_before,
            crate::intel::mmio_read(dev, y_base + UNI_PLANE_SURF_OFF),
            y_live_before,
            y_live_after,
            gpu_addr,
            uv_gpu_addr,
            frame_before,
            frame_after,
            frame_iters,
            uv_live_iters,
            y_live_iters
        );

        return ok;
    }

    crate::intel::mmio_write(dev, uv_base + UNI_PLANE_CTL_OFF, uv_ctl_before & !PLANE_CTL_ENABLE);
    crate::intel::mmio_write(dev, y_base + UNI_PLANE_CTL_OFF, y_ctl_before & !PLANE_CTL_ENABLE);
    crate::intel::mmio_write(dev, uv_base + UNI_PLANE_SURF_OFF, 0);
    crate::intel::mmio_write(dev, y_base + UNI_PLANE_SURF_OFF, 0);

    for plane_base in [y_base, uv_base] {
        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_STRIDE_OFF, stride_reg);
        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_POS_OFF, want_pos);
        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_SIZE_OFF, want_size);
        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_KEYVAL_OFF, 0);
        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_KEYMSK_OFF, 0);
        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_KEYMAX_OFF, 0);
        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_OFFSET_OFF, plane_pos_reg_value(0, 0));
        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_AUX_DIST_OFF, 0);
        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_AUX_OFFSET_OFF, 0);
    }
    let alpha_proof = program_decoded_nv12_overlay_plane_alpha(dev, uv_base, y_base);

    crate::intel::mmio_write(dev, y_base + UNI_PLANE_CUS_CTL_OFF, 0);
    crate::intel::mmio_write(dev, y_color_ctl_off, y_color_enabled);
    crate::intel::mmio_write(dev, y_base + UNI_PLANE_CTL_OFF, y_ctl_enabled);
    crate::intel::mmio_write(dev, y_base + UNI_PLANE_SURF_OFF, y_surface_reg);

    crate::intel::mmio_write(dev, uv_base + UNI_PLANE_CUS_CTL_OFF, uv_cus_enabled);
    crate::intel::mmio_write(dev, uv_color_ctl_off, uv_color_enabled);
    program_direct_nv12_input_csc(dev, uv_base, pipe, reason, probe_name, owner);
    crate::intel::mmio_write(dev, uv_base + UNI_PLANE_CTL_OFF, uv_ctl_enabled);
    crate::intel::mmio_write(dev, uv_base + UNI_PLANE_SURF_OFF, uv_surface_reg);

    let (frame_before, frame_after, frame_iters) = wait_for_pipe_next_frame(dev, pipe);
    let (uv_live_after, uv_live_iters) = wait_for_plane_live(dev, uv_base, uv_surface_reg, 20_000);
    let (y_live_after, y_live_iters) = wait_for_plane_live(dev, y_base, y_surface_reg, 20_000);
    let uv_ctl_after = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_CTL_OFF);
    let uv_stride_after = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_STRIDE_OFF);
    let uv_surf_after = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_SURF_OFF);
    let uv_color_after = crate::intel::mmio_read(dev, uv_color_ctl_off);
    let uv_cus_after = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_CUS_CTL_OFF);
    let y_ctl_after = crate::intel::mmio_read(dev, y_base + UNI_PLANE_CTL_OFF);
    let y_stride_after = crate::intel::mmio_read(dev, y_base + UNI_PLANE_STRIDE_OFF);
    let y_surf_after = crate::intel::mmio_read(dev, y_base + UNI_PLANE_SURF_OFF);
    let y_color_after = crate::intel::mmio_read(dev, y_color_ctl_off);
    let y_cus_after = crate::intel::mmio_read(dev, y_base + UNI_PLANE_CUS_CTL_OFF);
    let ok = uv_live_after == uv_surface_reg && y_live_after == y_surface_reg;
    log_decoded_nv12_plane_alpha_program(seq, "arm", reason, owner, pipe, alpha_proof);

    crate::log!(
        "intel/display: nv12-linked-plane-probe seq={} probe={} reason={} owner={} pipe={} ok={} uv_slot={} y_slot={} candidate={} uv_ctl=0x{:08X}->0x{:08X}/0x{:08X} uv_format={} uv_tiled={} uv_stride=0x{:08X}->0x{:08X}/0x{:08X} uv_surf=0x{:08X}->0x{:08X} uv_live=0x{:08X}->0x{:08X} uv_color=0x{:08X}->0x{:08X} uv_cus=0x{:08X}->0x{:08X} y_ctl=0x{:08X}->0x{:08X}/0x{:08X} y_format={} y_tiled={} y_is_y={} y_stride=0x{:08X}->0x{:08X}/0x{:08X} y_surf=0x{:08X}->0x{:08X} y_live=0x{:08X}->0x{:08X} y_color=0x{:08X}->0x{:08X} y_cus=0x{:08X}->0x{:08X} coded={}x{} visible={}x{} pitch=0x{:X} uv=0x{:X} bytes=0x{:X} y_gpu=0x{:X} uv_gpu=0x{:X} y_phys=0x{:X} uv_phys=0x{:X} virt=0x{:X} scanout={}x{} pos={}x{} size={}x{} frame={}=>{} frame_wait={} uv_live_iters={} y_live_iters={}\n",
        seq,
        probe_name,
        reason,
        owner,
        pipe.name,
        ok as u8,
        DIRECT_NV12_PLANE_PROBE_SLOT,
        DIRECT_NV12_Y_PLANE_PROBE_SLOT,
        tiling.name(),
        uv_ctl_before,
        uv_ctl_enabled,
        uv_ctl_after,
        decode_plane_format(uv_ctl_after),
        decode_plane_tiling(uv_ctl_after),
        uv_stride_before,
        stride_reg,
        uv_stride_after,
        uv_surf_before,
        uv_surf_after,
        uv_live_before,
        uv_live_after,
        uv_color_before,
        uv_color_after,
        uv_cus_before,
        uv_cus_after,
        y_ctl_before,
        y_ctl_enabled,
        y_ctl_after,
        decode_plane_format(y_ctl_after),
        decode_plane_tiling(y_ctl_after),
        ((y_ctl_after & PLANE_CTL_YUV420_Y_PLANE) != 0) as u8,
        y_stride_before,
        stride_reg,
        y_stride_after,
        y_surf_before,
        y_surf_after,
        y_live_before,
        y_live_after,
        y_color_before,
        y_color_after,
        y_cus_before,
        y_cus_after,
        coded_width,
        coded_height,
        visible_width,
        visible_height,
        pitch_bytes,
        uv_offset,
        byte_len,
        gpu_addr,
        uv_gpu_addr,
        phys_addr,
        phys_addr.saturating_add(uv_offset as u64),
        virt_addr,
        scanout_w,
        scanout_h,
        pos_x,
        pos_y,
        plane_width,
        plane_height,
        frame_before,
        frame_after,
        frame_iters,
        uv_live_iters,
        y_live_iters
    );

    ok
}

pub(crate) fn arm_decoded_nv12_overlay_plane_probe(
    reason: &str,
    gpu_addr: u64,
    phys_addr: u64,
    virt_addr: usize,
    coded_width: u32,
    coded_height: u32,
    visible_width: u32,
    visible_height: u32,
    pitch_bytes: usize,
    uv_offset: usize,
    byte_len: usize,
) -> bool {
    if !DIRECT_NV12_PLANE_PROBE_ENABLED {
        return false;
    }
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(pipe) = active_pipe(dev) else {
        return false;
    };
    if DIRECT_NV12_LINEAR_PATTERN_PROBE_ONLY {
        return arm_linear_nv12_pattern_video_plane_probe(dev, pipe, reason);
    }
    let seq = DIRECT_NV12_PLANE_PROBE_SEQ.load(Ordering::Acquire) + 1;
    let tiling = direct_nv12_decoded_probe_tiling_for_seq(seq);
    crate::log!(
        "intel/display: decoded-nv12-direct-probe reason={} pipe={} owner=video-nv12 staging={} tiling={} gpu=0x{:X} phys=0x{:X} virt=0x{:X} coded={}x{} visible={}x{} pitch=0x{:X} uv=0x{:X} bytes=0x{:X} y_gpu_page_aligned={} uv_gpu=0x{:X} uv_gpu_page_aligned={} uv_offset_page_aligned={} byte_len_covers_uv={}\n",
        reason,
        pipe.name,
        DIRECT_NV12_DECODED_LINEAR_STAGING_ENABLED as u8,
        tiling.name(),
        gpu_addr,
        phys_addr,
        virt_addr,
        coded_width,
        coded_height,
        visible_width,
        visible_height,
        pitch_bytes,
        uv_offset,
        byte_len,
        ((gpu_addr as usize) & (crate::intel::WARM_ALIGN - 1) == 0) as u8,
        gpu_addr.saturating_add(uv_offset as u64),
        (((gpu_addr.saturating_add(uv_offset as u64)) as usize) & (crate::intel::WARM_ALIGN - 1)
            == 0) as u8,
        (uv_offset & (crate::intel::WARM_ALIGN - 1) == 0) as u8,
        (byte_len > uv_offset) as u8
    );
    if DIRECT_NV12_DECODED_LINEAR_STAGING_ENABLED
        && let Some(armed) = arm_decoded_linear_nv12_staging_video_plane_probe(
            dev,
            pipe,
            reason,
            gpu_addr,
            phys_addr,
            virt_addr,
            coded_width,
            coded_height,
            visible_width,
            visible_height,
            pitch_bytes,
            uv_offset,
            byte_len,
        )
    {
        return armed;
    }
    arm_nv12_video_plane_probe_surface(
        "decoded-nv12",
        "video-nv12",
        reason,
        gpu_addr,
        phys_addr,
        virt_addr,
        coded_width,
        coded_height,
        visible_width,
        visible_height,
        pitch_bytes,
        uv_offset,
        byte_len,
        tiling,
    )
}

pub(crate) fn decoded_nv12_overlay_plane_probe_replaces_cpu_present() -> bool {
    DIRECT_NV12_LINKED_PLANES_PROBE_ENABLED
}

fn rgb_plane_probe_spec(index: usize) -> Option<(usize, u32, u32, u32, u32, u32, &'static str)> {
    match index {
        0 => Some((1, 256, 64, 0, 0, 0x00FF_0000, "red")),
        1 => Some((2, 512, 64, 0, 64, 0x0000_FF00, "green")),
        2 => Some((3, 64, 64, 0, 128, 0x0000_00FF, "blue")),
        _ => None,
    }
}

fn rgb_plane_probe_gpu(index: usize) -> Option<u64> {
    let offset = (index as u64).checked_mul(RGB_PLANE_PROBE_GPU_STRIDE)?;
    RGB_PLANE_PROBE_GPU_BASE.checked_add(offset)
}

fn ensure_rgb_plane_probe_surface(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    index: usize,
) -> Option<RgbPlaneProbeSurface> {
    let (plane_slot, width, height, _x, _y, color, name) = rgb_plane_probe_spec(index)?;
    let gpu = rgb_plane_probe_gpu(index)?;

    {
        let state = RGB_PLANE_PROBE_SURFACES.lock();
        if let Some(surface) = state[index]
            && surface.width == width
            && surface.height == height
            && surface.pipe.slot == pipe.slot
            && surface.plane_slot == plane_slot
            && surface.gpu == gpu
            && surface.color == color
        {
            return Some(surface);
        }
    }

    let pitch_bytes = aligned_pitch_bytes(width, PRIMARY_BYTES_PER_PIXEL)?;
    let byte_len = usize::try_from(u64::from(pitch_bytes) * u64::from(height)).ok()?;
    let (phys, virt) = crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN)?;
    fill_surface_color(virt, pitch_bytes as usize, width, height, color);
    crate::intel::dma_flush(virt, byte_len);

    if !crate::intel::map_display_scanout_ggtt(dev, phys, byte_len, gpu) {
        crate::log!(
            "intel/display: rgb-plane-probe ggtt map failed pipe={} slot={} name={} size={}x{} bytes=0x{:X} gpu=0x{:X}\n",
            pipe.name,
            plane_slot,
            name,
            width,
            height,
            byte_len,
            gpu
        );
        return None;
    }
    crate::intel::ggtt_invalidate(dev);

    let surface = RgbPlaneProbeSurface {
        width,
        height,
        pitch_bytes,
        phys,
        virt,
        pipe,
        plane_slot,
        gpu,
        color,
    };
    RGB_PLANE_PROBE_SURFACES.lock()[index] = Some(surface);
    crate::log!(
        "intel/display: rgb-plane-probe-surface pipe={} slot={} name={} size={}x{} pitch=0x{:X} gpu=0x{:X} phys=0x{:X}\n",
        pipe.name,
        plane_slot,
        name,
        width,
        height,
        pitch_bytes,
        gpu,
        phys
    );
    Some(surface)
}

fn rgb_plane_probe_needs_rearm(dev: crate::intel::Dev, surface: RgbPlaneProbeSurface) -> bool {
    let Some((_slot, _width, _height, x, y, _color, _name)) =
        rgb_plane_probe_spec(surface.plane_slot.saturating_sub(1))
    else {
        return false;
    };
    let plane_base = overlay_plane_base(surface.pipe, surface.plane_slot);
    let want_pos = plane_pos_reg_value(x, y);
    let want_size = plane_size_reg_value(surface.width, surface.height);
    let want_surf = u32::try_from(surface.gpu).unwrap_or(0);
    let ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CTL_OFF);
    let pos = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_POS_OFF);
    let size = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SIZE_OFF);
    let surf = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF);
    let surf_live = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);

    (ctl & PLANE_CTL_ENABLE) == 0
        || pos != want_pos
        || size != want_size
        || surf != want_surf
        || surf_live != want_surf
}

fn arm_rgb_plane_probe(
    dev: crate::intel::Dev,
    surface: RgbPlaneProbeSurface,
    reason: &str,
) -> bool {
    let Some((_slot, _width, _height, x, y, _color, name)) =
        rgb_plane_probe_spec(surface.plane_slot.saturating_sub(1))
    else {
        return false;
    };
    let Some(surface_reg) = u32::try_from(surface.gpu).ok() else {
        return false;
    };
    let Some(stride_reg) = plane_stride_reg_value(surface.pitch_bytes) else {
        return false;
    };
    let plane_base = overlay_plane_base(surface.pipe, surface.plane_slot);
    let ctl_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CTL_OFF);
    let ctl_disabled = ctl_before & !PLANE_CTL_ENABLE;
    let ctl_enabled = overlay_plane_ctl_enabled(ctl_before);
    let color_ctl_off = plane_base + UNI_PLANE_COLOR_CTL_OFF;
    let color_ctl_before = crate::intel::mmio_read(dev, color_ctl_off);
    let color_ctl_enabled = plane_color_ctl_alpha(color_ctl_before, OverlayAlphaMode::Opaque);
    let surf_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF);
    let live_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);

    crate::intel::dma_flush(
        surface.virt,
        (surface.pitch_bytes as usize).saturating_mul(surface.height as usize),
    );
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_CTL_OFF, ctl_disabled);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_SURF_OFF, 0);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_STRIDE_OFF, stride_reg);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_POS_OFF, plane_pos_reg_value(x, y));
    crate::intel::mmio_write(
        dev,
        plane_base + UNI_PLANE_SIZE_OFF,
        plane_size_reg_value(surface.width, surface.height),
    );
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_OFFSET_OFF, plane_pos_reg_value(0, 0));
    crate::intel::mmio_write(dev, color_ctl_off, color_ctl_enabled);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_CTL_OFF, ctl_enabled);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_SURF_OFF, surface_reg);

    let (live_after, live_iters) = wait_for_plane_live(dev, plane_base, surface_reg, 20_000);
    crate::log!(
        "intel/display: rgb-plane-probe-arm reason={} pipe={} slot={} name={} pos={}x{} size={}x{} color=0x{:08X} stride=0x{:X} surf_before=0x{:08X} surf_after=0x{:08X} live_before=0x{:08X} live_after=0x{:08X} ctl_before=0x{:08X} ctl_enabled=0x{:08X} color_ctl=0x{:08X}=>0x{:08X} live_iters={}\n",
        reason,
        surface.pipe.name,
        surface.plane_slot,
        name,
        x,
        y,
        surface.width,
        surface.height,
        surface.color,
        stride_reg,
        surf_before,
        crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF),
        live_before,
        live_after,
        ctl_before,
        ctl_enabled,
        color_ctl_before,
        color_ctl_enabled,
        live_iters
    );

    live_after == surface_reg
}

pub(super) fn arm_rgb_plane_probe_planes(dev: crate::intel::Dev, pipe: PipeInfo, reason: &str) {
    if !PRIMARY_REARM_RGB_PLANE_PROBE_ENABLED {
        return;
    }

    for index in 0..RGB_PLANE_PROBE_SLOT_COUNT {
        let Some((plane_slot, _width, _height, _x, _y, _color, _name)) =
            rgb_plane_probe_spec(index)
        else {
            continue;
        };
        if (PRIMARY_REARM_RGB_PLANE_PROBE_SLOT_MASK & (1u8 << plane_slot)) == 0 {
            continue;
        }
        let Some(surface) = ensure_rgb_plane_probe_surface(dev, pipe, index) else {
            crate::log!(
                "intel/display: rgb-plane-probe skipped reason={} pipe={} index={}\n",
                reason,
                pipe.name,
                index
            );
            continue;
        };
        if rgb_plane_probe_needs_rearm(dev, surface) {
            let _ = arm_rgb_plane_probe(dev, surface, reason);
        }
    }
}

pub(super) fn primary_format_probe_name() -> &'static str {
    match PRIMARY_FORMAT_PROBE_MODE {
        PRIMARY_FORMAT_PROBE_XRGB => "xrgb8888",
        PRIMARY_FORMAT_PROBE_XBGR => "xbgr8888-order-rgbx",
        _ => "unknown",
    }
}

pub(super) fn log_pipe_scanout_probe(dev: crate::intel::Dev, label: &str) {
    for pipe in PIPES {
        let pipe_src_raw = crate::intel::mmio_read(dev, pipe.pipe_src_off);
        let pipe_src_dims = decode_pipe_src(pipe_src_raw);
        let plane_ctl = crate::intel::mmio_read(dev, pipe.primary_plane().ctl());
        let plane_surf = crate::intel::mmio_read(dev, pipe.primary_plane().surf());
        let plane_surf_live = crate::intel::mmio_read(dev, pipe.primary_plane().surf_live());
        let (width, height) = pipe_src_dims.unwrap_or((0, 0));
        intel_display_verbose_log!(
            "intel/display: pipe-probe label={} pipe={} pipe_src=0x{:08X} dims={}x{} plane_enabled={} surf=0x{:08X} surf_live=0x{:08X}\n",
            label,
            pipe.name,
            pipe_src_raw,
            width,
            height,
            ((plane_ctl & PLANE_CTL_ENABLE) != 0) as u8,
            plane_surf,
            plane_surf_live
        );
    }
}

pub(super) fn log_primary_plane_probe(dev: crate::intel::Dev, pipe: PipeInfo, label: &str) {
    let ctl = crate::intel::mmio_read(dev, pipe.primary_plane().ctl());
    let stride = crate::intel::mmio_read(dev, pipe.primary_plane().stride());
    let pos = crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_POS_OFF);
    let size = crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_SIZE_OFF);
    let keyval = crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_KEYVAL_OFF);
    let keymsk = crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_KEYMSK_OFF);
    let surf = crate::intel::mmio_read(dev, pipe.primary_plane().surf());
    let keymax = crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_KEYMAX_OFF);
    let offset = crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_OFFSET_OFF);
    let surf_live = crate::intel::mmio_read(dev, pipe.primary_plane().surf_live());
    let aux_dist =
        crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_AUX_DIST_OFF);
    let aux_offset =
        crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_AUX_OFFSET_OFF);
    let color_ctl =
        crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_COLOR_CTL_OFF);
    let buf_cfg = crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_BUF_CFG_OFF);

    intel_display_verbose_log!(
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

pub(super) fn log_primary_dimensions_probe(
    pipe_name: &str,
    pipe_src_raw: u32,
    pipe_src_dims: Option<(u32, u32)>,
    fb_dims: Option<(u32, u32)>,
    chosen_from: &str,
) {
    let (pipe_w, pipe_h) = pipe_src_dims.unwrap_or((0, 0));
    let (fb_w, fb_h) = fb_dims.unwrap_or((0, 0));
    intel_display_verbose_log!(
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
