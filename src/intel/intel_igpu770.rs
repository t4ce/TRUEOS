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
}

unsafe impl Send for Igpu770WarmState {}
unsafe impl Sync for Igpu770WarmState {}

static WARM_STATE: Mutex<Option<Igpu770WarmState>> = Mutex::new(None);

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
    };

    crate::log!(
        "intel/igpu770: warm ring_phys=0x{:X} ring_len=0x{:X} context_phys=0x{:X} context_len=0x{:X} mmio_len=0x{:X} aperture=0x{:X}/0x{:X}\n",
        warm.ring_phys,
        warm.ring_len,
        warm.context_phys,
        warm.context_len,
        warm.mmio_len,
        warm.aperture_bar_phys,
        warm.aperture_bar_size
    );

    *state = Some(warm);
}
