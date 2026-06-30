use core::sync::atomic::{AtomicU32, Ordering};

use spin::Mutex;

use super::*;

const PRIMARY_REARM_RGB_PLANE_PROBE_ENABLED: bool = false;
const PRIMARY_REARM_RGB_PLANE_PROBE_SLOT_MASK: u8 = 1 << 2;
const RGB_PLANE_PROBE_SLOT_COUNT: usize = 3;
const RGB_PLANE_PROBE_GPU_BASE: u64 = crate::intel::GPU_VA_DISPLAY_OVERLAY_BASE;
const RGB_PLANE_PROBE_GPU_STRIDE: u64 = 0x0010_0000;
const DIRECT_NV12_PLANE_PROBE_ENABLED: bool = true;
const DIRECT_NV12_PLANE_PROBE_CYCLE_CANDIDATES: bool = true;
const DIRECT_NV12_PLANE_PROBE_SLOT: usize = OVERLAY_PLANE_SLOT;

static DIRECT_NV12_PLANE_PROBE_SEQ: AtomicU32 = AtomicU32::new(0);
static RGB_PLANE_PROBE_SURFACES: Mutex<[Option<RgbPlaneProbeSurface>; RGB_PLANE_PROBE_SLOT_COUNT]> =
    Mutex::new([None; RGB_PLANE_PROBE_SLOT_COUNT]);

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
        return DirectNv12PlaneTiling::Y;
    }
    match seq % 3 {
        1 => DirectNv12PlaneTiling::Y,
        2 => DirectNv12PlaneTiling::Yf,
        _ => DirectNv12PlaneTiling::Linear,
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
            | PLANE_CTL_ROTATE_MASK))
        | PLANE_CTL_ENABLE
        | PLANE_CTL_ARB_SLOTS_4BPP
        | PLANE_CTL_FORMAT_NV12
        | tiling.ctl_bits()
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
    if gpu_addr == 0 || coded_width == 0 || coded_height == 0 || pitch_bytes == 0 {
        crate::log!(
            "intel/display: nv12-plane-probe skipped reason={} cause=bad-surface gpu=0x{:X} coded={}x{} visible={}x{} pitch=0x{:X} uv=0x{:X} bytes=0x{:X}\n",
            reason,
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
    let tiling = direct_nv12_probe_tiling_for_seq(seq);
    let uv_rows = if pitch_bytes == 0 {
        0
    } else {
        uv_offset / pitch_bytes
    };

    program_two_plane_stack_resources(dev, pipe, DIRECT_NV12_PLANE_PROBE_SLOT, reason);

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
    let color_ctl_enabled =
        (color_ctl_before & !PLANE_COLOR_ALPHA_MASK) | PLANE_COLOR_PLANE_GAMMA_DISABLE;

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
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_AUX_OFFSET_OFF, 0);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_CUS_CTL_OFF, 0);
    crate::intel::mmio_write(dev, color_ctl_off, color_ctl_enabled);
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
        "intel/display: nv12-plane-probe seq={} reason={} pipe={} slot={} candidate={} ok={} ctl=0x{:08X}->0x{:08X}/0x{:08X} format={} tiled={} stride=0x{:08X}->0x{:08X}/0x{:08X} pos=0x{:08X}->0x{:08X}({}x{}) size=0x{:08X}->0x{:08X}({}x{}) offset=0x{:08X} surf=0x{:08X}->0x{:08X} live=0x{:08X}->0x{:08X} color=0x{:08X}->0x{:08X} alpha={} cus=0x{:08X}->0x{:08X} aux=0x{:08X}/0x{:08X}->0x{:08X}/0x{:08X} coded={}x{} visible={}x{} pitch=0x{:X} uv=0x{:X} uv_rows={} bytes=0x{:X} gpu=0x{:X} phys=0x{:X} virt=0x{:X} scanout={}x{} frame={}=>{} frame_wait={} live_iters={}\n",
        seq,
        reason,
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
