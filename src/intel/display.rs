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

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

macro_rules! intel_display_verbose_log {
    ($($arg:tt)*) => {
        if crate::logflag::INTEL_DISPLAY_NGIN_LOGS && !crate::logflag::INTEL_STAGE1_LOGS {
            crate::log!($($arg)*);
        }
    };
}

const PIPE_A_SRC: usize = 0x7001C;
const PIPE_B_SRC: usize = 0x7101C;
const PIPE_C_SRC: usize = 0x7201C;
const PIPE_D_SRC: usize = 0x7301C;
const PIPECONF_A: usize = 0x70008;
const TRANS_HTOTAL_A: usize = 0x60000;
const TRANS_HSYNC_A: usize = 0x60008;
const TRANS_VTOTAL_A: usize = 0x6000C;
const TRANS_VSYNC_A: usize = 0x60014;
const TRANS_DDI_FUNC_CTL_A: usize = 0x60400;
const TRANS_PSR_CTL_A: usize = 0x60800;
const TRANS_PSR_STATUS_A: usize = 0x60840;
const TRANS_PSR2_CTL_A: usize = 0x60900;
const TRANS_PSR2_STATUS_A: usize = 0x60940;
const CUR_SURFLIVE_A: usize = 0x700AC;
const PIPE_FRMCOUNT_A: usize = 0x70040;
const PIPE_MMIO_STRIDE: usize = 0x1000;
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
const PRIMARY_FORMAT_PROBE_XRGB: u32 = 0;
const PRIMARY_FORMAT_PROBE_XBGR: u32 = 1;
const PRIMARY_FORMAT_PROBE_MODE: u32 = PRIMARY_FORMAT_PROBE_XRGB;
const PRIMARY_PRESENT_DISABLE_PSR_PROBE: bool = true;
const PRIMARY_BYTES_PER_PIXEL: u32 = 4;
const PRIMARY_BASELINE_COLOR: u32 = 0x00FF_37FF;
const PRIMARY_BOOT_LOGO_JPEG: &[u8] = include_bytes!("../../logo.jpg");
const PRIMARY_BOOT_LOGO_ENABLED: bool = true;
const CPU_SCANOUT_PROOF_ENABLED: bool = true;
const CPU_SCANOUT_PROOF_COLOR: u32 = 0x00FF_00FF;
const CPU_SCANOUT_PROOF_SIZE: u32 = 8;
const OVERLAY_PLANE_SLOT: usize = 1;
const OVERLAY_MARGIN_X: u32 = 0;
const OVERLAY_MARGIN_Y: u32 = 0;

static PRIMARY_BOOT_SURFACE_INIT: AtomicBool = AtomicBool::new(false);
static PRIMARY_PRESENT_SEQ: AtomicU32 = AtomicU32::new(0);
static PRIMARY_SURFACE: Mutex<Option<PrimarySurface>> = Mutex::new(None);
static OVERLAY_PRESENT_SEQ: AtomicU32 = AtomicU32::new(0);
static OVERLAY_SURFACE: Mutex<Option<OverlaySurface>> = Mutex::new(None);
static HW_LOGO_PENDING_ID: AtomicU32 = AtomicU32::new(0);
static HW_LOGO_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();

#[derive(Copy, Clone)]
pub(super) struct PipeInfo {
    pub(super) name: &'static str,
    pub(super) slot: usize,
    pub(super) pipe_src_off: usize,
    pub(super) plane_ctl_off: usize,
    pub(super) plane_stride_off: usize,
    pub(super) plane_surf_off: usize,
    pub(super) plane_surf_live_off: usize,
}

#[derive(Copy, Clone)]
struct PrimarySurface {
    width: u32,
    height: u32,
    pitch_bytes: u32,
    phys: u64,
    virt: *mut u8,
    pipe: PipeInfo,
}

unsafe impl Send for PrimarySurface {}
unsafe impl Sync for PrimarySurface {}

#[derive(Copy, Clone)]
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
    phys: u64,
    virt: *mut u8,
    pipe: PipeInfo,
    plane_slot: usize,
}

unsafe impl Send for OverlaySurface {}
unsafe impl Sync for OverlaySurface {}

pub(super) const PIPES: [PipeInfo; 4] = [
    PipeInfo {
        name: "pipe-a",
        slot: 0,
        pipe_src_off: PIPE_A_SRC,
        plane_ctl_off: UNI_PLANE_BASE
            + 0 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_CTL_OFF,
        plane_stride_off: UNI_PLANE_BASE
            + 0 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_STRIDE_OFF,
        plane_surf_off: UNI_PLANE_BASE
            + 0 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_SURF_OFF,
        plane_surf_live_off: UNI_PLANE_BASE
            + 0 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_SURFLIVE_OFF,
    },
    PipeInfo {
        name: "pipe-b",
        slot: 1,
        pipe_src_off: PIPE_B_SRC,
        plane_ctl_off: UNI_PLANE_BASE
            + 1 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_CTL_OFF,
        plane_stride_off: UNI_PLANE_BASE
            + 1 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_STRIDE_OFF,
        plane_surf_off: UNI_PLANE_BASE
            + 1 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_SURF_OFF,
        plane_surf_live_off: UNI_PLANE_BASE
            + 1 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_SURFLIVE_OFF,
    },
    PipeInfo {
        name: "pipe-c",
        slot: 2,
        pipe_src_off: PIPE_C_SRC,
        plane_ctl_off: UNI_PLANE_BASE
            + 2 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_CTL_OFF,
        plane_stride_off: UNI_PLANE_BASE
            + 2 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_STRIDE_OFF,
        plane_surf_off: UNI_PLANE_BASE
            + 2 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_SURF_OFF,
        plane_surf_live_off: UNI_PLANE_BASE
            + 2 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_SURFLIVE_OFF,
    },
    PipeInfo {
        name: "pipe-d",
        slot: 3,
        pipe_src_off: PIPE_D_SRC,
        plane_ctl_off: UNI_PLANE_BASE
            + 3 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_CTL_OFF,
        plane_stride_off: UNI_PLANE_BASE
            + 3 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_STRIDE_OFF,
        plane_surf_off: UNI_PLANE_BASE
            + 3 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_SURF_OFF,
        plane_surf_live_off: UNI_PLANE_BASE
            + 3 * UNI_PLANE_PIPE_STRIDE
            + 0 * UNI_PLANE_SLOT_STRIDE
            + UNI_PLANE_SURFLIVE_OFF,
    },
];

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
    program_pipe_bottom_color(dev, pipe, PIPE_BOTTOM_COLOR_RGB);

    let Some(pitch_bytes) = aligned_pitch_bytes(width, PRIMARY_BYTES_PER_PIXEL) else {
        crate::log!("intel/display: primary-boot-surface skipped bad pitch width={}\n", width);
        return;
    };
    let Some(byte_len) = usize::try_from(u64::from(pitch_bytes) * u64::from(height)).ok() else {
        crate::log!("intel/display: primary-boot-surface skipped surface too large\n");
        return;
    };
    let Some((phys, virt)) = crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN) else {
        crate::log!("intel/display: primary-boot-surface alloc failed bytes=0x{:X}\n", byte_len);
        return;
    };

    fill_surface_color(virt, pitch_bytes as usize, width, height, PRIMARY_BASELINE_COLOR);
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
    let ctl_before = crate::intel::mmio_read(dev, pipe.plane_ctl_off);
    let surf_before = crate::intel::mmio_read(dev, pipe.plane_surf_off);
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
    let surf_armed = crate::intel::mmio_read(dev, pipe.plane_surf_off);
    let ctl_after = crate::intel::mmio_read(dev, pipe.plane_ctl_off);
    let ok = surf_live == surface_reg || surf_armed == surface_reg;

    let primary_surface = PrimarySurface {
        width,
        height,
        pitch_bytes,
        phys,
        virt,
        pipe,
    };
    *PRIMARY_SURFACE.lock() = Some(primary_surface);
    log_primary_scanout_pte_window(dev, "after-primary-init", byte_len);

    let logo_ok = if PRIMARY_BOOT_LOGO_ENABLED {
        present_sw_logo_decode()
    } else {
        false
    };

    if CPU_SCANOUT_PROOF_ENABLED {
        log_cpu_scanout_proof(dev, primary_surface);
    }

    crate::log!(
        "intel/display: primary-boot-surface pipe={} size={}x{} pitch=0x{:X} gpu=0x{:X} phys=0x{:X} plane_enabled={} ctl_before=0x{:08X} ctl_after=0x{:08X} surf_before=0x{:08X} surf=0x{:08X} surf_live=0x{:08X} ok={} logo={}\n",
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
        ok as u8,
        logo_ok as u8
    );
}

fn present_sw_logo_decode() -> bool {
    match crate::gfx::jpeg_codec::decode_jpeg_rgba(PRIMARY_BOOT_LOGO_JPEG) {
        Ok(decoded) => present_rgba_surface_center(
            decoded.rgba.as_slice(),
            decoded.width,
            decoded.height,
            (decoded.width as usize).saturating_mul(4),
        ),
        Err(err) => {
            crate::log!(
                "intel/display: primary-logo decode failed code={} bytes=0x{:X}\n",
                err.code(),
                PRIMARY_BOOT_LOGO_JPEG.len()
            );
            false
        }
    }
}

fn probe_hw_logo_decode() -> bool {
    match crate::intel::hw_pic_submit_jpeg(PRIMARY_BOOT_LOGO_JPEG) {
        Ok(id) => {
            HW_LOGO_PENDING_ID.store(id, Ordering::Release);
            HW_LOGO_WAIT.notify_all();
            let snap = crate::intel::hw_pic_snapshot();
            crate::log!(
                "intel/display: hw-logo submit ok id={} bytes=0x{:X} pending={} outputs={} service={}\n",
                id,
                PRIMARY_BOOT_LOGO_JPEG.len(),
                snap.pending,
                snap.outputs,
                snap.service_started as u8
            );
            true
        }
        Err(code) => {
            let snap = crate::intel::hw_pic_snapshot();
            crate::log!(
                "intel/display: hw-logo submit failed code={} bytes=0x{:X} pending={} outputs={} service={}\n",
                code,
                PRIMARY_BOOT_LOGO_JPEG.len(),
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
        let pending_id = HW_LOGO_PENDING_ID.load(Ordering::Acquire);
        if pending_id == 0 {
            HW_LOGO_WAIT.wait_for_event().await;
            continue;
        }

        let Some(output) = crate::intel::hw_pic_wait_output_for_id(pending_id, 500).await else {
            let snap = crate::intel::hw_pic_snapshot();
            crate::log!(
                "intel/display: hw-logo wait id={} pending={} outputs={} service={} timeout_ms=500\n",
                pending_id,
                snap.pending,
                snap.outputs,
                snap.service_started as u8,
            );
            continue;
        };

        HW_LOGO_PENDING_ID.compare_exchange(
            pending_id,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        ).ok();

        let presented = if output.status == crate::intel::hw_pic::HwPicStatus::Ready
            && output.format == crate::intel::hw_pic::HwPicPixelFormat::Nv12
            && output.width != 0
            && output.height != 0
            && output.pitch_bytes != 0
            && output.byte_len != 0
            && output.virt_addr != 0
        {
            let src = unsafe {
                core::slice::from_raw_parts(output.virt_addr as *const u8, output.byte_len)
            };
            present_nv12_surface_center(
                src,
                output.width,
                output.height,
                0,
                0,
                output.width,
                output.height,
                output.pitch_bytes,
            )
        } else {
            false
        };

        crate::log!(
            "intel/display: hw-logo output id={} status={:?} fmt={:?} size={}x{} pitch=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X} presented={} err={}\n",
            output.id,
            output.status,
            output.format,
            output.width,
            output.height,
            output.pitch_bytes,
            output.byte_len,
            output.gpu_addr,
            output.phys_addr,
            presented as u8,
            output.error_code,
        );
    }
}

fn log_cpu_scanout_proof(dev: crate::intel::Dev, surface: PrimarySurface) {
    if surface.width == 0
        || surface.height == 0
        || surface.pitch_bytes < PRIMARY_BYTES_PER_PIXEL
        || surface.virt.is_null()
    {
        crate::log!("intel/display: cpu-scanout-proof accepted=0 reason=bad-surface\n");
        return;
    }

    let marker_w = CPU_SCANOUT_PROOF_SIZE.min(surface.width);
    let marker_h = CPU_SCANOUT_PROOF_SIZE.min(surface.height);
    let x = surface.width.saturating_sub(marker_w).saturating_sub(16);
    let y = surface.height.saturating_sub(marker_h).saturating_sub(16);
    let pitch_bytes = surface.pitch_bytes as usize;
    let pixel_offset = (y as usize)
        .saturating_mul(pitch_bytes)
        .saturating_add((x as usize).saturating_mul(PRIMARY_BYTES_PER_PIXEL as usize));
    let sample_ptr = unsafe { surface.virt.add(pixel_offset) };
    let before = unsafe { core::ptr::read_volatile(sample_ptr as *const u32) };

    for row in 0..marker_h as usize {
        let row_ptr = unsafe {
            surface
                .virt
                .add((y as usize + row).saturating_mul(pitch_bytes))
                .add((x as usize).saturating_mul(PRIMARY_BYTES_PER_PIXEL as usize))
                as *mut u32
        };
        for col in 0..marker_w as usize {
            unsafe {
                core::ptr::write_volatile(row_ptr.add(col), CPU_SCANOUT_PROOF_COLOR);
            }
        }
    }
    let byte_len = pitch_bytes.saturating_mul(surface.height as usize);
    crate::intel::dma_flush(surface.virt, byte_len);

    let after = unsafe { core::ptr::read_volatile(sample_ptr as *const u32) };
    let surf = crate::intel::mmio_read(dev, surface.pipe.plane_surf_off);
    let surf_live = crate::intel::mmio_read(dev, surface.pipe.plane_surf_live_off);
    let stride = crate::intel::mmio_read(dev, surface.pipe.plane_stride_off);
    let want_surf = u32::try_from(crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE).unwrap_or(0);
    let accepted =
        after == CPU_SCANOUT_PROOF_COLOR && (surf == want_surf || surf_live == want_surf);

    crate::log!(
        "intel/display: cpu-scanout-proof accepted={} pipe={} gpu=0x{:X} phys=0x{:X} xy={}x{} size={}x{} pitch=0x{:X} stride_reg=0x{:08X} before=0x{:08X} after=0x{:08X} color=0x{:08X} flush=1 surf=0x{:08X} surf_live=0x{:08X} does_not_prove=render_backend_write\n",
        accepted as u8,
        surface.pipe.name,
        crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE,
        surface.phys,
        x,
        y,
        marker_w,
        marker_h,
        surface.pitch_bytes,
        stride,
        before,
        after,
        CPU_SCANOUT_PROOF_COLOR,
        surf,
        surf_live
    );
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

fn program_pipe_bottom_color(dev: crate::intel::Dev, pipe: PipeInfo, rgb: u32) {
    let reg = SKL_BOTTOM_COLOR_A + pipe.slot * SKL_BOTTOM_COLOR_PIPE_STRIDE;
    crate::intel::mmio_write(dev, reg, rgb);
    let readback = crate::intel::mmio_read(dev, reg);
    intel_display_verbose_log!(
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
    let byte_len = (surface.pitch_bytes as usize).saturating_mul(surface.height as usize);
    if marker_offset.saturating_add(core::mem::size_of::<u32>()) > byte_len {
        return None;
    }

    Some(PrimarySurfaceGpgpuTarget {
        width: surface.width,
        height: surface.height,
        pitch_bytes: surface.pitch_bytes,
        gpu: crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE,
        phys: surface.phys,
        virt: surface.virt,
        byte_len,
        marker_gpu: crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE + marker_offset as u64,
        marker_virt: unsafe { surface.virt.add(marker_offset) },
        marker_offset,
        marker_x,
        marker_y,
    })
}

pub(super) fn notify_primary_surface_external_write(
    reason: &str,
    flush_offset: usize,
    flush_bytes: usize,
) -> bool {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    let byte_len = (surface.pitch_bytes as usize).saturating_mul(surface.height as usize);
    if !surface.virt.is_null() && flush_offset < byte_len {
        let flush_bytes = core::cmp::min(flush_bytes, byte_len.saturating_sub(flush_offset));
        crate::intel::dma_flush(unsafe { surface.virt.add(flush_offset) }, flush_bytes);
    }
    notify_primary_surface_present(surface, reason, byte_len)
}

pub(crate) fn present_rgba_surface_center(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
) -> bool {
    let Some(surface) = *PRIMARY_SURFACE.lock() else {
        return false;
    };
    if surface.virt.is_null() || src_width == 0 || src_height == 0 {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    let src_width = src_width as usize;
    let src_height = src_height as usize;
    if src_pitch_bytes < src_width.saturating_mul(4) || dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    fill_surface_color(
        surface.virt,
        dst_pitch,
        surface.width,
        surface.height,
        PRIMARY_BASELINE_COLOR,
    );

    let copy_w = core::cmp::min(dst_width, src_width);
    let copy_h = core::cmp::min(dst_height, src_height);
    let dst_x = dst_width.saturating_sub(copy_w) / 2;
    let dst_y = dst_height.saturating_sub(copy_h) / 2;
    let src_x = src_width.saturating_sub(copy_w) / 2;
    let src_y = src_height.saturating_sub(copy_h) / 2;

    for row_idx in 0..copy_h {
        let src_row_off = (src_y + row_idx)
            .saturating_mul(src_pitch_bytes)
            .saturating_add(src_x.saturating_mul(4));
        let Some(src_row) = src.get(src_row_off..src_row_off + copy_w.saturating_mul(4)) else {
            return false;
        };
        let dst_row_off = (dst_y + row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_off = col_idx.saturating_mul(4);
            let pixel = u32::from_le_bytes([
                src_row[src_off],
                src_row[src_off + 1],
                src_row[src_off + 2],
                src_row[src_off + 3],
            ]) & 0x00FF_FFFF;
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
    }

    crate::intel::dma_flush(surface.virt, dst_pitch.saturating_mul(dst_height));
    true
}

pub(crate) fn present_rgba_overlay_top_right(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
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

    if !copy_rgba_into_overlay(surface, src, src_width, src_height, src_pitch_bytes) {
        return false;
    }

    let byte_len = (surface.pitch_bytes as usize).saturating_mul(surface.height as usize);
    crate::intel::dma_flush(surface.virt, byte_len);

    if overlay_plane_needs_rearm(dev, surface) {
        if !arm_overlay_plane(dev, surface, "camera-overlay") {
            return false;
        }
    }

    let seq = OVERLAY_PRESENT_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if seq <= 8 || seq.is_multiple_of(60) {
        let plane_base = overlay_plane_base(surface.pipe, surface.plane_slot);
        crate::log!(
            "intel/display: overlay-present seq={} pipe={} slot={} size={}x{} pitch=0x{:X} gpu=0x{:X} phys=0x{:X} surf=0x{:08X} surf_live=0x{:08X}\n",
            seq,
            surface.pipe.name,
            surface.plane_slot,
            surface.width,
            surface.height,
            surface.pitch_bytes,
            crate::intel::GPU_VA_DISPLAY_OVERLAY_BASE,
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

    // Y-tile: 128 bytes wide × 32 rows tall = 4096 bytes per tile.
    // In NV12 tiled, both Y and UV planes share the same tiled surface.
    // UV plane starts at row chroma_y_offset = align(src_height, 32) for
    // tile boundary alignment (must match MFX_SURFACE_STATE programming).
    const YTILE_W: usize = 128;
    const YTILE_H: usize = 32;
    let tiles_per_row = src_pitch_bytes / YTILE_W;
    let chroma_y_offset = (coded_height + YTILE_H - 1) & !(YTILE_H - 1);
    let total_height = chroma_y_offset + (coded_height + 1) / 2;
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

    // Repaint the whole destination every frame. The NV12 copy does not always
    // touch every pixel of the scanout surface, so reusing prior contents leaves
    // deterministic stale bands at the bottom/edges when the copied region is
    // even slightly smaller than the destination.
    fill_surface_color(
        surface.virt,
        dst_pitch,
        surface.width,
        surface.height,
        PRIMARY_BASELINE_COLOR,
    );

    // Fit the visible video into the destination while preserving aspect ratio.
    // The previous code only downscaled and never upscaled, so a 1080p video on
    // a taller panel would leave whole green tile rows unused at the bottom.
    let (copy_w, copy_h) =
        if dst_width.saturating_mul(visible_height) <= dst_height.saturating_mul(visible_width) {
            let copy_w = dst_width.max(1);
            let copy_h = visible_height
                .saturating_mul(copy_w)
                .checked_div(visible_width.max(1))
                .unwrap_or(1)
                .max(1)
                .min(dst_height);
            (copy_w, copy_h)
        } else {
            let copy_h = dst_height.max(1);
            let copy_w = visible_width
                .saturating_mul(copy_h)
                .checked_div(visible_height.max(1))
                .unwrap_or(1)
                .max(1)
                .min(dst_width);
            (copy_w, copy_h)
        };
    let dst_x = dst_width.saturating_sub(copy_w) / 2;
    let dst_y = dst_height.saturating_sub(copy_h) / 2;

    // Detile helper: given (byte_x, row_y) return byte offset in the tiled surface.
    // Y-tile internal layout is OWord-column-major: 8 columns of 16 bytes,
    // each column stored for all 32 rows before the next column.
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
        let uv_row = chroma_y_offset + src_y / 2;
        for col_idx in 0..copy_w {
            let src_x = visible_x.saturating_add(
                col_idx
                    .saturating_mul(visible_width)
                    .checked_div(copy_w.max(1))
                    .unwrap_or(0)
                    .min(visible_width.saturating_sub(1)),
            );
            let y_off = ytile_offset(src_x, src_y, tiles_per_row);
            let y = unsafe { i32::from(*src.get_unchecked(y_off)) };
            let c = (y - 16).max(0);
            let (r, g, b) = if crate::logflag::INTEL_MEDIA_PRESENT_LUMA_ONLY {
                let luma = clamp_u8_i32((298 * c + 128) >> 8);
                (luma, luma, luma)
            } else {
                // UV plane is in the same tiled surface, starting at row chroma_y_offset.
                // Intel MFX NV12 chroma: byte 0 = Cr (V), byte 1 = Cb (U).
                let uv_x = src_x & !1;
                let uv_off = ytile_offset(uv_x, uv_row, tiles_per_row);
                let v = unsafe { i32::from(*src.get_unchecked(uv_off)) } - 128;
                let u = unsafe { i32::from(*src.get_unchecked(uv_off + 1)) } - 128;
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
    let Some(surface_reg) = u32::try_from(crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE).ok() else {
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

    // Skip PSR probe after the first two frames;
    // PSR is always 0x00000000 on this display pipeline.
    if seq <= 2 {
        crate::intel::ggtt_invalidate(dev);

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

    // Fast path: if the plane is already active (seq > 1) and PLANE_SURF
    // already points to our surface, skip MMIO writes and vblank waits.
    // The display scanner is already reading from this address; new pixel
    // data is visible as soon as the CPU cache flush completes.
    if seq > 1 {
        let surf_current = crate::intel::mmio_read(dev, surface.pipe.plane_surf_off);
        if surf_current == surface_reg {
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
        // Different surface address: write and wait one vblank.
        crate::intel::mmio_write(dev, surface.pipe.plane_surf_off, surface_reg);
        let (frame_before, frame_after, frame_iters) = wait_for_pipe_next_frame(dev, surface.pipe);
        if should_log_primary_present(seq) {
            intel_display_verbose_log!(
                "intel/display: primary-flip seq={} reason={} pipe={} surf=0x{:08X}=>0x{:08X} frame={}=>{} frame_wait={}\n",
                seq,
                reason,
                surface.pipe.name,
                surf_current,
                surface_reg,
                frame_before,
                frame_after,
                frame_iters,
            );
        }
        return true;
    }

    let surf_before = crate::intel::mmio_read(dev, surface.pipe.plane_surf_off);
    let surf_live_before = crate::intel::mmio_read(dev, surface.pipe.plane_surf_live_off);
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
    let surf_after = crate::intel::mmio_read(dev, surface.pipe.plane_surf_off);

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
    let mut live = crate::intel::mmio_read(dev, pipe.plane_surf_live_off);
    let mut iter = 0usize;
    while iter < max_iters && live != want_live {
        core::hint::spin_loop();
        live = crate::intel::mmio_read(dev, pipe.plane_surf_live_off);
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
    let order_bits = match PRIMARY_FORMAT_PROBE_MODE {
        PRIMARY_FORMAT_PROBE_XRGB => 0,
        PRIMARY_FORMAT_PROBE_XBGR => PLANE_CTL_ORDER_RGBX,
        _ => 0,
    };
    (ctl_before
        & !(PLANE_CTL_ENABLE
            | PLANE_CTL_FORMAT_MASK_SKL
            | PLANE_CTL_TILED_MASK
            | PLANE_CTL_ORDER_RGBX))
        | PLANE_CTL_ENABLE
        | PLANE_CTL_FORMAT_XRGB_8888
        | PLANE_CTL_TILED_LINEAR
        | order_bits
}

fn overlay_plane_ctl_enabled(ctl_before: u32) -> u32 {
    primary_plane_ctl_enabled(ctl_before)
}

fn primary_format_probe_name() -> &'static str {
    match PRIMARY_FORMAT_PROBE_MODE {
        PRIMARY_FORMAT_PROBE_XRGB => "xrgb8888",
        PRIMARY_FORMAT_PROBE_XBGR => "xbgr8888-order-rgbx",
        _ => "unknown",
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
    let ctl_before = crate::intel::mmio_read(dev, pipe.plane_ctl_off);
    let ctl_disabled = ctl_before & !PLANE_CTL_ENABLE;
    let ctl_enabled = primary_plane_ctl_enabled(ctl_before);

    crate::intel::mmio_write(dev, pipe.plane_ctl_off, ctl_disabled);
    crate::intel::mmio_write(dev, pipe.plane_surf_off, 0);
    let (disable_frame_before, disable_frame_after, disable_frame_iters) =
        wait_for_pipe_next_frame(dev, pipe);
    let (live_cleared, clear_iters) = wait_for_primary_plane_live(dev, pipe, 0, 20_000);

    crate::intel::mmio_write(dev, pipe.plane_stride_off, stride_reg);
    crate::intel::mmio_write(
        dev,
        pipe.plane_ctl_off + UNI_PLANE_POS_OFF,
        plane_pos_reg_value(0, 0),
    );
    crate::intel::mmio_write(
        dev,
        pipe.plane_ctl_off + UNI_PLANE_SIZE_OFF,
        plane_size_reg_value(width, height),
    );
    crate::intel::mmio_write(
        dev,
        pipe.plane_ctl_off + UNI_PLANE_OFFSET_OFF,
        plane_pos_reg_value(0, 0),
    );
    crate::intel::mmio_write(dev, pipe.plane_ctl_off, ctl_enabled);
    crate::intel::mmio_write(dev, pipe.plane_surf_off, surface_reg);

    let (arm_frame_before, arm_frame_after, arm_frame_iters) = wait_for_pipe_next_frame(dev, pipe);
    let (surf_live_after, live_iters) = wait_for_primary_plane_live(dev, pipe, surface_reg, 20_000);

    intel_display_verbose_log!(
        "intel/display: primary-rearm reason={} pipe={} format_probe={} ctl_before=0x{:08X} ctl_disabled=0x{:08X} ctl_enabled=0x{:08X} disable_frame={}=>{} disable_wait={} clear_live=0x{:08X} clear_iters={} arm_frame={}=>{} arm_wait={} surf=0x{:08X} surf_live=0x{:08X} live_iters={}\n",
        reason,
        pipe.name,
        primary_format_probe_name(),
        ctl_before,
        ctl_disabled,
        ctl_enabled,
        disable_frame_before,
        disable_frame_after,
        disable_frame_iters,
        live_cleared,
        clear_iters,
        arm_frame_before,
        arm_frame_after,
        arm_frame_iters,
        crate::intel::mmio_read(dev, pipe.plane_surf_off),
        surf_live_after,
        live_iters
    );

    (ctl_before, ctl_enabled, surf_live_after, live_iters)
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
    pipe.plane_ctl_off + plane_slot.saturating_mul(UNI_PLANE_SLOT_STRIDE)
}

fn overlay_plane_position(surface: OverlaySurface) -> (u32, u32) {
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

fn ensure_overlay_surface(
    dev: crate::intel::Dev,
    width: u32,
    height: u32,
) -> Option<OverlaySurface> {
    let active_pipe = active_pipe(dev)?;

    {
        let state = OVERLAY_SURFACE.lock();
        if let Some(surface) = *state
            && surface.width == width
            && surface.height == height
            && surface.pipe.slot == active_pipe.slot
            && surface.plane_slot == OVERLAY_PLANE_SLOT
        {
            return Some(surface);
        }
    }

    let pitch_bytes = aligned_pitch_bytes(width, PRIMARY_BYTES_PER_PIXEL)?;
    let byte_len = usize::try_from(u64::from(pitch_bytes) * u64::from(height)).ok()?;
    let (phys, virt) = crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN)?;
    fill_surface_color(virt, pitch_bytes as usize, width, height, 0);
    crate::intel::dma_flush(virt, byte_len);

    if !crate::intel::map_display_scanout_ggtt(
        dev,
        phys,
        byte_len,
        crate::intel::GPU_VA_DISPLAY_OVERLAY_BASE,
    ) {
        crate::log!(
            "intel/display: overlay-surface ggtt map failed pipe={} slot={} size={}x{} bytes=0x{:X} gpu=0x{:X}\n",
            active_pipe.name,
            OVERLAY_PLANE_SLOT,
            width,
            height,
            byte_len,
            crate::intel::GPU_VA_DISPLAY_OVERLAY_BASE
        );
        return None;
    }
    crate::intel::ggtt_invalidate(dev);

    let surface = OverlaySurface {
        width,
        height,
        pitch_bytes,
        phys,
        virt,
        pipe: active_pipe,
        plane_slot: OVERLAY_PLANE_SLOT,
    };
    *OVERLAY_SURFACE.lock() = Some(surface);
    crate::log!(
        "intel/display: overlay-surface pipe={} slot={} size={}x{} pitch=0x{:X} gpu=0x{:X} phys=0x{:X}\n",
        active_pipe.name,
        OVERLAY_PLANE_SLOT,
        width,
        height,
        pitch_bytes,
        crate::intel::GPU_VA_DISPLAY_OVERLAY_BASE,
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
            let pixel = u32::from_le_bytes([
                src_row[src_off],
                src_row[src_off + 1],
                src_row[src_off + 2],
                src_row[src_off + 3],
            ]) & 0x00FF_FFFF;
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
    }

    true
}

fn overlay_plane_needs_rearm(dev: crate::intel::Dev, surface: OverlaySurface) -> bool {
    let plane_base = overlay_plane_base(surface.pipe, surface.plane_slot);
    let (pos_x, pos_y) = overlay_plane_position(surface);
    let want_pos = plane_pos_reg_value(pos_x, pos_y);
    let want_size = plane_size_reg_value(surface.width, surface.height);
    let want_surf = u32::try_from(crate::intel::GPU_VA_DISPLAY_OVERLAY_BASE).unwrap_or(0);
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

fn arm_overlay_plane(dev: crate::intel::Dev, surface: OverlaySurface, reason: &str) -> bool {
    let plane_base = overlay_plane_base(surface.pipe, surface.plane_slot);
    let Some(surface_reg) = u32::try_from(crate::intel::GPU_VA_DISPLAY_OVERLAY_BASE).ok() else {
        return false;
    };
    let Some(stride_reg) = plane_stride_reg_value(surface.pitch_bytes) else {
        return false;
    };
    let (pos_x, pos_y) = overlay_plane_position(surface);
    let ctl_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CTL_OFF);
    let ctl_disabled = ctl_before & !PLANE_CTL_ENABLE;
    let ctl_enabled = overlay_plane_ctl_enabled(ctl_before);
    let color_ctl_before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_COLOR_CTL_OFF);
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
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_OFFSET_OFF, plane_pos_reg_value(0, 0));
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_CTL_OFF, ctl_enabled);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_SURF_OFF, surface_reg);

    let (live_after, live_iters) = wait_for_plane_live(dev, plane_base, surface_reg, 20_000);
    crate::log!(
        "intel/display: overlay-arm reason={} pipe={} slot={} ctl_before=0x{:08X} ctl_enabled=0x{:08X} color_ctl=0x{:08X} pos={}x{} size={}x{} stride=0x{:08X} surf_before=0x{:08X} surf_after=0x{:08X} surf_live_before=0x{:08X} surf_live_after=0x{:08X} live_iters={}\n",
        reason,
        surface.pipe.name,
        surface.plane_slot,
        ctl_before,
        ctl_enabled,
        color_ctl_before,
        pos_x,
        pos_y,
        surface.width,
        surface.height,
        stride_reg,
        surf_before,
        crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF),
        live_before,
        live_after,
        live_iters
    );

    live_after == surface_reg
}

pub(super) fn active_pipe(dev: crate::intel::Dev) -> Option<PipeInfo> {
    let mut enabled_plane = None;
    let mut observed = None;
    for pipe in PIPES {
        let pipe_src = crate::intel::mmio_read(dev, pipe.pipe_src_off);
        if decode_pipe_src(pipe_src).is_some() {
            return Some(pipe);
        }
        let plane_ctl = crate::intel::mmio_read(dev, pipe.plane_ctl_off);
        let plane_surf = crate::intel::mmio_read(dev, pipe.plane_surf_off);
        let plane_surf_live = crate::intel::mmio_read(dev, pipe.plane_surf_live_off);
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

fn log_pipe_scanout_probe(dev: crate::intel::Dev, label: &str) {
    for pipe in PIPES {
        let pipe_src_raw = crate::intel::mmio_read(dev, pipe.pipe_src_off);
        let pipe_src_dims = decode_pipe_src(pipe_src_raw);
        let plane_ctl = crate::intel::mmio_read(dev, pipe.plane_ctl_off);
        let plane_surf = crate::intel::mmio_read(dev, pipe.plane_surf_off);
        let plane_surf_live = crate::intel::mmio_read(dev, pipe.plane_surf_live_off);
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

pub(super) fn plane_buf_cfg_for_pipe_slot(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    plane_slot: usize,
) -> u32 {
    crate::intel::mmio_read(
        dev,
        pipe.plane_ctl_off
            + plane_slot.saturating_mul(UNI_PLANE_SLOT_STRIDE)
            + UNI_PLANE_BUF_CFG_OFF,
    )
}

#[inline]
fn decode_xy_x(v: u32) -> u32 {
    v & 0xFFFF
}

#[inline]
fn decode_xy_y(v: u32) -> u32 {
    (v >> 16) & 0xFFFF
}

pub(super) fn decode_pipe_src(value: u32) -> Option<(u32, u32)> {
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

pub(super) fn framebuffer_hint() -> Option<(u32, u32)> {
    let fb = crate::limine::framebuffer_response()?
        .framebuffers()
        .first()
        .copied()?;
    Some((fb.width as u32, fb.height as u32))
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

pub(super) fn aligned_pitch_bytes(width: u32, bytes_per_pixel: u32) -> Option<u32> {
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

pub(super) fn fill_surface_color(
    ptr: *mut u8,
    pitch_bytes: usize,
    width: u32,
    height: u32,
    color: u32,
) {
    let width = width as usize;
    let height = height as usize;
    unsafe {
        for y in 0..height {
            let row = ptr.add(y.saturating_mul(pitch_bytes)) as *mut u32;
            for x in 0..width {
                core::ptr::write_volatile(row.add(x), color);
            }
        }
    }
}
