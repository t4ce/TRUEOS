use core::ptr;

use spin::Mutex;

use super::IntelDeviceInfo;

const INTEL_IGPU770_DEVICE_ID: u16 = 0x4680;
const WARM_RING_BYTES: usize = 4096;
const WARM_CONTEXT_BYTES: usize = 4096;
const WARM_ALIGN: usize = 4096;

#[derive(Copy, Clone, Debug)]
pub struct Igpu770WarmState {
    pub ring_phys: u64,
    pub ring_virt: *mut u8,
    pub ring_len: usize,
    pub context_phys: u64,
    pub context_virt: *mut u8,
    pub context_len: usize,
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

    unsafe {
        ptr::write_bytes(ring_virt, 0, WARM_RING_BYTES);
        ptr::write_bytes(context_virt, 0, WARM_CONTEXT_BYTES);
    }

    let fb = limine_framebuffer_info();

    let warm = Igpu770WarmState {
        ring_phys,
        ring_virt,
        ring_len: WARM_RING_BYTES,
        context_phys,
        context_virt,
        context_len: WARM_CONTEXT_BYTES,
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
        "intel/igpu770: warm ring_phys=0x{:X} ring_len=0x{:X} context_phys=0x{:X} context_len=0x{:X} mmio_len=0x{:X} aperture=0x{:X}/0x{:X} limine_fb=0x{:X}/0x{:X} {}x{} pitch=0x{:X} bpp={}\n",
        warm.ring_phys,
        warm.ring_len,
        warm.context_phys,
        warm.context_len,
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
