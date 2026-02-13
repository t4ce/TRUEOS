use alloc::vec::Vec;

use crate::net::device::LinkState;
use spin::Mutex;

static PACKET_POOL: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
const POOL_MAX: usize = 1024;
const RX_BUF_SIZE: usize = 2048;

pub fn alloc_rx_buf() -> Vec<u8> {
    if let Some(buf) = PACKET_POOL.lock().pop() {
        buf
    } else {
        alloc::vec![0u8; RX_BUF_SIZE]
    }
}

pub fn recycle_rx_buf(mut buf: Vec<u8>) {
    if buf.capacity() < RX_BUF_SIZE {
        return;
    }
    // Safety: we are about to put this into the ring for DMA overwrite.
    // The previous contents don't matter, and we don't need to zero-fill (costly).
    unsafe { buf.set_len(RX_BUF_SIZE); }
    
    let mut pool = PACKET_POOL.lock();
    if pool.len() < POOL_MAX {
        pool.push(buf);
    }
}

pub struct DmaRegion {
    phys: u64,
    virt: *mut u8,
    len: usize,
}

// Safety: physical memory backing these pointers is stable for the OS lifetime.
unsafe impl Send for DmaRegion {}

impl DmaRegion {
    pub fn alloc(size: usize, align: usize) -> Option<Self> {
        let (phys, virt) = crate::pci::dma::alloc(size, align)?;
        Some(Self { phys, virt, len: size })
    }

    pub fn phys(&self) -> u64 {
        self.phys
    }

    pub fn virt(&self) -> *mut u8 {
        self.virt
    }
}

impl Drop for DmaRegion {
    fn drop(&mut self) {
        if self.len == 0 || self.virt.is_null() {
            return;
        }
        crate::pci::dma::dealloc(self.virt, self.len);
        self.len = 0;
        self.virt = core::ptr::null_mut();
        self.phys = 0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RxError {
    RingFull,
}

struct RxSlot {
    buf: Vec<u8>,
    len: usize,
    owned_by_hw: bool,
}


pub struct RxRing {
    slots: Vec<RxSlot>,
    head: usize,
    tail: usize,
}

impl RxRing {
    pub fn new(desc_count: usize, buf_size: usize) -> Self {
        let mut slots = Vec::with_capacity(desc_count.max(1));
        for _ in 0..desc_count.max(1) {
            slots.push(RxSlot {
                buf: alloc::vec![0u8; buf_size],
                len: 0,
                owned_by_hw: true,
            });
        }
        Self { slots, head: 0, tail: 0 }
    }

    pub fn mark_complete(&mut self, slot: usize, len: usize) {
        let idx = slot % self.slots.len();
        let slot = &mut self.slots[idx];
        slot.len = len.min(slot.buf.len());
        slot.owned_by_hw = false;
    }

    pub fn poll(&mut self, budget: usize) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut processed = 0;
        while processed < budget {
            let slot = &mut self.slots[self.head];
            if slot.owned_by_hw || slot.len == 0 {
                break;
            }
            
            // Swap buffer from pool to avoid allocation and copy
            let new_buf = alloc_rx_buf();
            let len = slot.len;
            let mut packet = core::mem::replace(&mut slot.buf, new_buf);
            packet.truncate(len);
            
            out.push(packet);
            slot.len = 0;
            slot.owned_by_hw = true;
            self.head = (self.head + 1) % self.slots.len();
            processed += 1;
        }
        out
    }

    pub fn push_hw_owned(&mut self) -> Result<usize, RxError> {
        let next = (self.tail + 1) % self.slots.len();
        if next == self.head {
            return Err(RxError::RingFull);
        }
        let idx = self.tail;
        self.tail = next;
        let slot = &mut self.slots[idx];
        slot.len = 0;
        slot.owned_by_hw = true;
        Ok(idx)
    }

    pub fn push_packet(&mut self, data: &[u8]) -> Result<(), RxError> {
        let next = (self.tail + 1) % self.slots.len();
        if next == self.head {
            return Err(RxError::RingFull);
        }
        let idx = self.tail;
        let slot = &mut self.slots[idx];
        let len = data.len().min(slot.buf.len());
        slot.buf[..len].copy_from_slice(&data[..len]);
        slot.len = len;
        slot.owned_by_hw = false;
        self.tail = next;
        Ok(())
    }

    pub fn buffer_mut(&mut self, slot: usize) -> &mut [u8] {
        let idx = slot % self.slots.len();
        &mut self.slots[idx].buf[..]
    }
}

pub struct NetRing {
    rx: RxRing,
    poll_budget: usize,
    _link: LinkState,
}

impl NetRing {
    pub fn new(
        rx_desc: usize,
        rx_buf_size: usize,
        poll_budget: usize,
    ) -> Self {
        Self {
            rx: RxRing::new(rx_desc, rx_buf_size),
            poll_budget: poll_budget.max(1),
            _link: LinkState::down(),
        }
    }

    pub fn poll_rx(&mut self) -> Vec<Vec<u8>> {
        self.rx.poll(self.poll_budget)
    }

    pub fn push_rx_packet(&mut self, data: &[u8]) -> Result<(), RxError> {
        self.rx.push_packet(data)
    }


    pub fn rx_ring_mut(&mut self) -> &mut RxRing {
        &mut self.rx
    }
}
