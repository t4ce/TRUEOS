use core::{
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

use spin::Mutex;

use super::IntelDeviceInfo;

const INTEL_IGPU770_DEVICE_ID: u16 = 0x4680;
const WARM_RING_BYTES: usize = 4096;
const WARM_CONTEXT_BYTES: usize = 4096;
const WARM_BATCH_BYTES: usize = 4096;
const WARM_RESULT_BYTES: usize = 4096;
const WARM_ALIGN: usize = 4096;
const SMOKE_RECT_W: usize = 64;
const SMOKE_RECT_H: usize = 64;
const SMOKE_COLOR_XRGB8888: u32 = 0x00FF_4A24;
const GPU_VA_RING_BASE: u64 = 0x0080_0000;
const GPU_VA_CONTEXT_BASE: u64 = 0x0081_0000;
const GPU_VA_BATCH_BASE: u64 = 0x0082_0000;
const GPU_VA_RESULT_BASE: u64 = 0x0083_0000;

#[derive(Copy, Clone, Debug)]
pub struct Igpu770WarmState {
    pub ring_phys: u64,
    pub ring_virt: *mut u8,
    pub ring_len: usize,
    pub context_phys: u64,
    pub context_virt: *mut u8,
    pub context_len: usize,
    pub batch_phys: u64,
    pub batch_virt: *mut u8,
    pub batch_len: usize,
    pub result_phys: u64,
    pub result_virt: *mut u8,
    pub result_len: usize,
    pub mmio_base: usize,
    pub mmio_len: usize,
    pub aperture_bar_phys: u64,
    pub aperture_bar_size: u64,
    pub limine_fb_phys: u64,
    pub limine_fb_virt: usize,
    pub limine_fb_size: usize,
    pub limine_fb_pitch: usize,
    pub limine_fb_width: usize,
    pub limine_fb_height: usize,
    pub limine_fb_bpp: usize,
}

unsafe impl Send for Igpu770WarmState {}
unsafe impl Sync for Igpu770WarmState {}

static WARM_STATE: Mutex<Option<Igpu770WarmState>> = Mutex::new(None);
static GGTT_BLT_SMOKE_RAN: AtomicBool = AtomicBool::new(false);
static GGTT_RECON_RAN: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug)]
struct LimineFramebufferInfo {
    phys: u64,
    virt: usize,
    size: usize,
    pitch: usize,
    width: usize,
    height: usize,
    bpp: usize,
}

fn limine_framebuffer_info() -> Option<LimineFramebufferInfo> {
    use ::limine::framebuffer::MemoryModel;

    let fb = crate::limine::framebuffer_response()?.framebuffers().next()?;
    if fb.memory_model() != MemoryModel::RGB {
        return None;
    }
    let bpp = fb.bpp() as usize;
    if bpp != 32 {
        return None;
    }

    let virt = fb.addr() as usize;
    let phys = crate::phys::virt_to_phys_checked(fb.addr())?;
    let pitch = fb.pitch() as usize;
    let width = fb.width() as usize;
    let height = fb.height() as usize;
    let size = pitch.saturating_mul(height);

    Some(LimineFramebufferInfo {
        phys,
        virt,
        size,
        pitch,
        width,
        height,
        bpp,
    })
}

#[inline]
pub fn warm_state() -> Option<Igpu770WarmState> {
    *WARM_STATE.lock()
}

#[inline]
pub fn warmed() -> bool {
    WARM_STATE.lock().is_some()
}

#[derive(Copy, Clone, Debug)]
struct GgttMapPlan {
    gpu_addr: u64,
    phys: u64,
    size: usize,
    pages: usize,
    aperture_backed: bool,
}

#[derive(Copy, Clone, Debug)]
struct BltFillRectPlan {
    dst_gpu_addr: u64,
    dst_phys: u64,
    rect_w: usize,
    rect_h: usize,
    pitch: usize,
    color: u32,
}

fn ggtt_map_plan_system_ram(phys: u64, size: usize, gpu_addr: u64) -> Option<GgttMapPlan> {
    if phys == 0 || size == 0 {
        return None;
    }
    let pages = size.div_ceil(WARM_ALIGN);
    Some(GgttMapPlan {
        gpu_addr,
        phys,
        size,
        pages,
        aperture_backed: false,
    })
}

fn ggtt_map_plan_aperture_backed(phys: u64, size: usize, aperture_base: u64) -> Option<GgttMapPlan> {
    if phys == 0 || size == 0 || aperture_base == 0 {
        return None;
    }
    if phys < aperture_base {
        return None;
    }
    let offset = phys.checked_sub(aperture_base)?;
    let size_u64 = u64::try_from(size).ok()?;
    let _ = offset.checked_add(size_u64)?;
    let pages = size.div_ceil(WARM_ALIGN);
    Some(GgttMapPlan {
        gpu_addr: offset,
        phys,
        size,
        pages,
        aperture_backed: true,
    })
}

fn log_ggtt_map_plan(label: &str, plan: GgttMapPlan) {
    crate::log!(
        "intel/igpu770: ggtt-plan label={} gpu=0x{:X} phys=0x{:X} size=0x{:X} pages={} aperture_backed={}\n",
        label,
        plan.gpu_addr,
        plan.phys,
        plan.size,
        plan.pages,
        plan.aperture_backed as u8
    );
}

fn blt_fill_rect_plan(warm: Igpu770WarmState) -> Option<BltFillRectPlan> {
    let rect_w = SMOKE_RECT_W.min(warm.limine_fb_width.max(1));
    let rect_h = SMOKE_RECT_H.min(warm.limine_fb_height.max(1));
    let dst = ggtt_map_plan_aperture_backed(
        warm.limine_fb_phys,
        warm.limine_fb_size,
        warm.aperture_bar_phys,
    )?;
    Some(BltFillRectPlan {
        dst_gpu_addr: dst.gpu_addr,
        dst_phys: warm.limine_fb_phys,
        rect_w,
        rect_h,
        pitch: warm.limine_fb_pitch,
        color: SMOKE_COLOR_XRGB8888,
    })
}

pub fn ggtt_recon_once() {
    if GGTT_RECON_RAN.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(warm) = warm_state() else {
        crate::log!("intel/igpu770: ggtt-recon skipped reason=not-warmed\n");
        return;
    };

    let fb = ggtt_map_plan_aperture_backed(
        warm.limine_fb_phys,
        warm.limine_fb_size,
        warm.aperture_bar_phys,
    );
    let ring = ggtt_map_plan_system_ram(warm.ring_phys, warm.ring_len, GPU_VA_RING_BASE);
    let context =
        ggtt_map_plan_system_ram(warm.context_phys, warm.context_len, GPU_VA_CONTEXT_BASE);
    let batch = ggtt_map_plan_system_ram(warm.batch_phys, warm.batch_len, GPU_VA_BATCH_BASE);
    let result = ggtt_map_plan_system_ram(warm.result_phys, warm.result_len, GPU_VA_RESULT_BASE);

    crate::log!(
        "intel/igpu770: ggtt-recon aperture=0x{:X}/0x{:X} limine_fb_phys=0x{:X} limine_fb_size=0x{:X}\n",
        warm.aperture_bar_phys,
        warm.aperture_bar_size,
        warm.limine_fb_phys,
        warm.limine_fb_size
    );

    if let Some(plan) = fb {
        log_ggtt_map_plan("framebuffer", plan);
    } else {
        crate::log!("intel/igpu770: ggtt-recon framebuffer plan unavailable\n");
    }
    if let Some(plan) = ring {
        log_ggtt_map_plan("ring", plan);
    }
    if let Some(plan) = context {
        log_ggtt_map_plan("context", plan);
    }
    if let Some(plan) = batch {
        log_ggtt_map_plan("batch", plan);
    }
    if let Some(plan) = result {
        log_ggtt_map_plan("result", plan);
    }
    crate::log!(
        "intel/igpu770: ggtt-recon note system-ram objects need explicit GGTT PTEs; framebuffer is aperture-backed\n"
    );
}

pub fn ggtt_blt_smoke_test_once() {
    if GGTT_BLT_SMOKE_RAN.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(warm) = warm_state() else {
        crate::log!("intel/igpu770: ggtt-blt-smoke skipped reason=not-warmed\n");
        return;
    };

    let Some(ring) = ggtt_map_plan_system_ram(warm.ring_phys, warm.ring_len, GPU_VA_RING_BASE) else {
        crate::log!("intel/igpu770: ggtt-blt-smoke skipped reason=ring-plan\n");
        return;
    };
    let Some(batch) = ggtt_map_plan_system_ram(warm.batch_phys, warm.batch_len, GPU_VA_BATCH_BASE) else {
        crate::log!("intel/igpu770: ggtt-blt-smoke skipped reason=batch-plan\n");
        return;
    };
    let Some(result) =
        ggtt_map_plan_system_ram(warm.result_phys, warm.result_len, GPU_VA_RESULT_BASE)
    else {
        crate::log!("intel/igpu770: ggtt-blt-smoke skipped reason=result-plan\n");
        return;
    };
    let Some(fill) = blt_fill_rect_plan(warm) else {
        crate::log!("intel/igpu770: ggtt-blt-smoke skipped reason=fb-plan\n");
        return;
    };

    unsafe {
        ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
    }

    crate::log!("intel/igpu770: ggtt-blt-smoke begin\n");
    log_ggtt_map_plan("ring", ring);
    log_ggtt_map_plan("batch", batch);
    log_ggtt_map_plan("result", result);
    crate::log!(
        "intel/igpu770: blt-fill-plan dst_gpu=0x{:X} dst_phys=0x{:X} rect={}x{} pitch=0x{:X} color=0x{:08X}\n",
        fill.dst_gpu_addr,
        fill.dst_phys,
        fill.rect_w,
        fill.rect_h,
        fill.pitch,
        fill.color
    );
    crate::log!(
        "intel/igpu770: ggtt-blt-smoke scaffold-only no-engine-submit-yet mmio=0x{:X} aperture=0x{:X}/0x{:X}\n",
        warm.mmio_base,
        warm.aperture_bar_phys,
        warm.aperture_bar_size
    );
}

pub fn warm_once(info: IntelDeviceInfo) {
    if info.device_id != INTEL_IGPU770_DEVICE_ID {
        return;
    }

    let mut state = WARM_STATE.lock();
    if state.is_some() {
        return;
    }

    let Some((ring_phys, ring_virt)) = crate::dma::alloc(WARM_RING_BYTES, WARM_ALIGN) else {
        crate::log!("intel/igpu770: warm alloc failed part=ring size=0x{:X}\n", WARM_RING_BYTES);
        return;
    };
    let Some((context_phys, context_virt)) = crate::dma::alloc(WARM_CONTEXT_BYTES, WARM_ALIGN)
    else {
        crate::log!(
            "intel/igpu770: warm alloc failed part=context size=0x{:X}\n",
            WARM_CONTEXT_BYTES
        );
        return;
    };
    let Some((batch_phys, batch_virt)) = crate::dma::alloc(WARM_BATCH_BYTES, WARM_ALIGN) else {
        crate::log!("intel/igpu770: warm alloc failed part=batch size=0x{:X}\n", WARM_BATCH_BYTES);
        return;
    };
    let Some((result_phys, result_virt)) = crate::dma::alloc(WARM_RESULT_BYTES, WARM_ALIGN) else {
        crate::log!(
            "intel/igpu770: warm alloc failed part=result size=0x{:X}\n",
            WARM_RESULT_BYTES
        );
        return;
    };

    unsafe {
        ptr::write_bytes(ring_virt, 0, WARM_RING_BYTES);
        ptr::write_bytes(context_virt, 0, WARM_CONTEXT_BYTES);
        ptr::write_bytes(batch_virt, 0, WARM_BATCH_BYTES);
        ptr::write_bytes(result_virt, 0, WARM_RESULT_BYTES);
    }

    let fb = limine_framebuffer_info();

    let warm = Igpu770WarmState {
        ring_phys,
        ring_virt,
        ring_len: WARM_RING_BYTES,
        context_phys,
        context_virt,
        context_len: WARM_CONTEXT_BYTES,
        batch_phys,
        batch_virt,
        batch_len: WARM_BATCH_BYTES,
        result_phys,
        result_virt,
        result_len: WARM_RESULT_BYTES,
        mmio_base: info.mmio_base.as_ptr() as usize,
        mmio_len: info.mmio_len,
        aperture_bar_phys: info.aperture_bar_phys,
        aperture_bar_size: info.aperture_bar_size,
        limine_fb_phys: fb.map(|v| v.phys).unwrap_or(0),
        limine_fb_virt: fb.map(|v| v.virt).unwrap_or(0),
        limine_fb_size: fb.map(|v| v.size).unwrap_or(0),
        limine_fb_pitch: fb.map(|v| v.pitch).unwrap_or(0),
        limine_fb_width: fb.map(|v| v.width).unwrap_or(0),
        limine_fb_height: fb.map(|v| v.height).unwrap_or(0),
        limine_fb_bpp: fb.map(|v| v.bpp).unwrap_or(0),
    };

    crate::log!(
        "intel/igpu770: warm ring_phys=0x{:X} ring_len=0x{:X} context_phys=0x{:X} context_len=0x{:X} batch_phys=0x{:X} batch_len=0x{:X} result_phys=0x{:X} result_len=0x{:X} mmio_len=0x{:X} aperture=0x{:X}/0x{:X} limine_fb=0x{:X}/0x{:X} {}x{} pitch=0x{:X} bpp={}\n",
        warm.ring_phys,
        warm.ring_len,
        warm.context_phys,
        warm.context_len,
        warm.batch_phys,
        warm.batch_len,
        warm.result_phys,
        warm.result_len,
        warm.mmio_len,
        warm.aperture_bar_phys,
        warm.aperture_bar_size,
        warm.limine_fb_phys,
        warm.limine_fb_size,
        warm.limine_fb_width,
        warm.limine_fb_height,
        warm.limine_fb_pitch,
        warm.limine_fb_bpp
    );

    *state = Some(warm);
}
