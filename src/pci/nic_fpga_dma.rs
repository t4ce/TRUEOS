#![allow(dead_code)]

use core::sync::atomic::{Ordering, fence};

use heapless::Vec;
use spin::Mutex;

const DEFAULT_SHARED_DMA_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_SHARED_DMA_ALIGN: usize = 4096;
const MAX_PENDING: usize = 1024;
const MAX_INFLIGHT: usize = 1024;
const MAX_COMPLETED: usize = 1024;
const MAX_OWNED: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedRegion {
    pub phys_base: u64,
    pub virt_base: usize,
    pub size: usize,
    pub align: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Endpoint {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferDesc {
    pub cookie: u32,
    pub phys: u64,
    pub len: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub submitted: u64,
    pub dispatched: u64,
    pub completed: u64,
    pub dropped: u64,
    pub fpga_doorbells: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    NotInitialized,
    AlreadyInitialized,
    AllocationFailed,
    QueueFull,
    InFlightFull,
    CompletedFull,
    InvalidLength,
    UnknownCookie,
}

struct State {
    initialized: bool,
    region: Option<SharedRegion>,
    region_virt: *mut u8,
    nic: Option<Endpoint>,
    fpga: Option<Endpoint>,
    pending: Vec<TransferDesc, MAX_PENDING>,
    inflight: Vec<TransferDesc, MAX_INFLIGHT>,
    completed: Vec<TransferDesc, MAX_COMPLETED>,
    owned: Vec<OwnedBuffer, MAX_OWNED>,
    next_cookie: u32,
    stats: Stats,
}

// Safety: all interior mutability is behind `STATE`.
unsafe impl Send for State {}

static STATE: Mutex<State> = Mutex::new(State {
    initialized: false,
    region: None,
    region_virt: core::ptr::null_mut(),
    nic: None,
    fpga: None,
    pending: Vec::new(),
    inflight: Vec::new(),
    completed: Vec::new(),
    owned: Vec::new(),
    next_cookie: 1,
    stats: Stats {
        submitted: 0,
        dispatched: 0,
        completed: 0,
        dropped: 0,
        fpga_doorbells: 0,
    },
});

/// Reserve the default shared DMA region once at boot.
pub fn init_default_once() -> Result<SharedRegion, Error> {
    init_with_size(DEFAULT_SHARED_DMA_BYTES, DEFAULT_SHARED_DMA_ALIGN)
}

/// Reserve a shared DMA region used by the NIC->FPGA data path.
pub fn init_with_size(size: usize, align: usize) -> Result<SharedRegion, Error> {
    if size == 0 || align == 0 {
        return Err(Error::InvalidLength);
    }

    let mut st = STATE.lock();
    if st.initialized {
        return st.region.ok_or(Error::AlreadyInitialized);
    }

    let (phys, virt) = crate::dma::alloc(size, align).ok_or(Error::AllocationFailed)?;
    let region = SharedRegion {
        phys_base: phys,
        virt_base: virt as usize,
        size,
        align,
    };

    st.initialized = true;
    st.region = Some(region);
    st.region_virt = virt;
    Ok(region)
}

/// Returns the shared DMA region metadata.
pub fn shared_region() -> Option<SharedRegion> {
    STATE.lock().region
}

/// Binds the active NIC endpoint (control-plane identity only).
pub fn bind_nic(endpoint: Endpoint) -> Result<(), Error> {
    let mut st = STATE.lock();
    if !st.initialized {
        return Err(Error::NotInitialized);
    }
    st.nic = Some(endpoint);
    Ok(())
}

/// Binds the active FPGA endpoint (control-plane identity only).
pub fn bind_fpga(endpoint: Endpoint) -> Result<(), Error> {
    let mut st = STATE.lock();
    if !st.initialized {
        return Err(Error::NotInitialized);
    }
    st.fpga = Some(endpoint);
    Ok(())
}

pub fn endpoints() -> (Option<Endpoint>, Option<Endpoint>) {
    let st = STATE.lock();
    (st.nic, st.fpga)
}

/// Called by NIC-side code when a new DMA buffer is ready for FPGA consumption.
pub fn submit_nic_frame(frame_phys: u64, frame_len: usize) -> Result<u32, Error> {
    if frame_len == 0 || frame_len > (u32::MAX as usize) {
        return Err(Error::InvalidLength);
    }

    let mut st = STATE.lock();
    if !st.initialized {
        return Err(Error::NotInitialized);
    }

    let cookie = st.next_cookie;
    st.next_cookie = st.next_cookie.wrapping_add(1).max(1);
    let desc = TransferDesc {
        cookie,
        phys: frame_phys,
        len: frame_len as u32,
    };

    if st.pending.push(desc).is_err() {
        st.stats.dropped = st.stats.dropped.saturating_add(1);
        return Err(Error::QueueFull);
    }

    st.stats.submitted = st.stats.submitted.saturating_add(1);
    Ok(cookie)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OwnedBuffer {
    cookie: u32,
    virt: usize,
    len: usize,
}

/// Convenience path when software only has virtual packet bytes (no NIC phys addr).
/// Copies payload into kernel DMA memory and submits descriptor ownership to this module.
pub fn submit_nic_frame_copy(frame: &[u8]) -> Result<u32, Error> {
    if frame.is_empty() {
        return Err(Error::InvalidLength);
    }

    let mut st = STATE.lock();
    if !st.initialized {
        return Err(Error::NotInitialized);
    }
    if st.pending.is_full() {
        st.stats.dropped = st.stats.dropped.saturating_add(1);
        return Err(Error::QueueFull);
    }
    if st.owned.is_full() {
        st.stats.dropped = st.stats.dropped.saturating_add(1);
        return Err(Error::InFlightFull);
    }

    let (phys, virt) = crate::dma::alloc(frame.len(), 64).ok_or(Error::AllocationFailed)?;
    unsafe {
        core::ptr::copy_nonoverlapping(frame.as_ptr(), virt, frame.len());
    }

    let cookie = st.next_cookie;
    st.next_cookie = st.next_cookie.wrapping_add(1).max(1);
    let desc = TransferDesc {
        cookie,
        phys,
        len: frame.len() as u32,
    };

    if st.pending.push(desc).is_err() {
        crate::dma::dealloc(virt, frame.len());
        st.stats.dropped = st.stats.dropped.saturating_add(1);
        return Err(Error::QueueFull);
    }
    if st
        .owned
        .push(OwnedBuffer {
            cookie,
            virt: virt as usize,
            len: frame.len(),
        })
        .is_err()
    {
        let _ = st.pending.pop();
        crate::dma::dealloc(virt, frame.len());
        st.stats.dropped = st.stats.dropped.saturating_add(1);
        return Err(Error::InFlightFull);
    }

    st.stats.submitted = st.stats.submitted.saturating_add(1);
    Ok(cookie)
}

/// Called by FPGA-side code to fetch work. Moves one descriptor to in-flight.
pub fn acquire_for_fpga() -> Result<Option<TransferDesc>, Error> {
    let mut st = STATE.lock();
    if !st.initialized {
        return Err(Error::NotInitialized);
    }
    if st.pending.is_empty() {
        return Ok(None);
    }
    if st.inflight.is_full() {
        return Err(Error::InFlightFull);
    }

    let desc = st.pending.remove(0);
    if st.inflight.push(desc).is_err() {
        return Err(Error::InFlightFull);
    }

    st.stats.dispatched = st.stats.dispatched.saturating_add(1);

    // Ensure descriptor writes are globally visible before the FPGA doorbell.
    fence(Ordering::Release);
    Ok(Some(desc))
}

/// Marks a descriptor complete when FPGA DMA processing finishes.
pub fn complete_from_fpga(cookie: u32) -> Result<(), Error> {
    let mut st = STATE.lock();
    if !st.initialized {
        return Err(Error::NotInitialized);
    }

    let Some(idx) = st.inflight.iter().position(|d| d.cookie == cookie) else {
        return Err(Error::UnknownCookie);
    };
    let desc = st.inflight.remove(idx);

    if st.completed.push(desc).is_err() {
        st.stats.dropped = st.stats.dropped.saturating_add(1);
        return Err(Error::CompletedFull);
    }

    st.stats.completed = st.stats.completed.saturating_add(1);

    // Ensure completion status is visible before IRQ/event signaling.
    fence(Ordering::Release);
    Ok(())
}

/// Called by CPU-side completion path to reclaim one completed descriptor.
pub fn take_completed() -> Result<Option<TransferDesc>, Error> {
    let mut st = STATE.lock();
    if !st.initialized {
        return Err(Error::NotInitialized);
    }
    if st.completed.is_empty() {
        return Ok(None);
    }

    // Pair with release in `complete_from_fpga`.
    fence(Ordering::Acquire);
    let desc = st.completed.remove(0);
    if let Some(idx) = st.owned.iter().position(|b| b.cookie == desc.cookie) {
        let owned = st.owned.remove(idx);
        crate::dma::dealloc(owned.virt as *mut u8, owned.len);
    }
    Ok(Some(desc))
}

/// Tracks software doorbells sent to FPGA BAR control registers.
pub fn note_fpga_doorbell() -> Result<(), Error> {
    let mut st = STATE.lock();
    if !st.initialized {
        return Err(Error::NotInitialized);
    }
    st.stats.fpga_doorbells = st.stats.fpga_doorbells.saturating_add(1);
    Ok(())
}

pub fn stats() -> Result<Stats, Error> {
    let st = STATE.lock();
    if !st.initialized {
        return Err(Error::NotInitialized);
    }
    Ok(st.stats)
}

pub fn queue_depths() -> Result<(usize, usize, usize), Error> {
    let st = STATE.lock();
    if !st.initialized {
        return Err(Error::NotInitialized);
    }
    Ok((st.pending.len(), st.inflight.len(), st.completed.len()))
}
