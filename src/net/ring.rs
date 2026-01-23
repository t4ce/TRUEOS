#![allow(dead_code)]

use alloc::vec::Vec;

use crate::net::device::{LinkState, OffloadFlags};

pub struct DmaRegion {
    phys: u64,
    virt: *mut u8,
    len: usize,
}

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

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn free(self) {
        crate::pci::dma::dealloc(self.virt, self.len);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxError {
    RingFull,
    FrameTooLarge,
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

struct TxSlot {
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
                buf: vec![0u8; buf_size],
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
            let mut packet = Vec::with_capacity(slot.len);
            packet.extend_from_slice(&slot.buf[..slot.len]);
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

pub struct TxRing {
    slots: Vec<TxSlot>,
    head: usize,
    tail: usize,
}

impl TxRing {
    pub fn new(desc_count: usize, buf_size: usize) -> Self {
        let mut slots = Vec::with_capacity(desc_count.max(1));
        for _ in 0..desc_count.max(1) {
            slots.push(TxSlot {
                buf: vec![0u8; buf_size],
                len: 0,
                owned_by_hw: false,
            });
        }
        Self { slots, head: 0, tail: 0 }
    }

    pub fn submit(&mut self, frame: &[u8]) -> Result<(), TxError> {
        let slot = &mut self.slots[self.tail];
        if slot.owned_by_hw {
            return Err(TxError::RingFull);
        }
        if frame.len() > slot.buf.len() {
            return Err(TxError::FrameTooLarge);
        }
        slot.buf[..frame.len()].copy_from_slice(frame);
        slot.len = frame.len();
        slot.owned_by_hw = true;
        self.tail = (self.tail + 1) % self.slots.len();
        Ok(())
    }

    pub fn mark_complete(&mut self, slot: usize) {
        let idx = slot % self.slots.len();
        let slot = &mut self.slots[idx];
        slot.len = 0;
        slot.owned_by_hw = false;
        if idx == self.head {
            self.head = (self.head + 1) % self.slots.len();
        }
    }

    pub fn buffer(&self, slot: usize) -> &[u8] {
        let idx = slot % self.slots.len();
        let slot = &self.slots[idx];
        &slot.buf[..slot.len]
    }
}

pub struct NetRing {
    rx: RxRing,
    tx: TxRing,
    poll_budget: usize,
    link: LinkState,
    offloads: OffloadFlags,
    irq_pending: bool,
}

impl NetRing {
    pub fn new(
        rx_desc: usize,
        rx_buf_size: usize,
        tx_desc: usize,
        tx_buf_size: usize,
        poll_budget: usize,
    ) -> Self {
        Self {
            rx: RxRing::new(rx_desc, rx_buf_size),
            tx: TxRing::new(tx_desc, tx_buf_size),
            poll_budget: poll_budget.max(1),
            link: LinkState::down(),
            offloads: OffloadFlags::default(),
            irq_pending: false,
        }
    }

    pub fn poll_rx(&mut self) -> Vec<Vec<u8>> {
        self.irq_pending = false;
        self.rx.poll(self.poll_budget)
    }

    pub fn tx_submit(&mut self, frame: &[u8]) -> Result<(), TxError> {
        self.tx.submit(frame)
    }

    pub fn push_rx_packet(&mut self, data: &[u8]) -> Result<(), RxError> {
        self.rx.push_packet(data)
    }

    pub fn set_link(&mut self, link: LinkState) {
        self.link = link;
    }

    pub fn set_offloads(&mut self, offloads: OffloadFlags) {
        self.offloads = offloads;
    }

    pub fn note_irq(&mut self) {
        self.irq_pending = true;
    }

    pub fn rx_ring_mut(&mut self) -> &mut RxRing {
        &mut self.rx
    }

    pub fn tx_ring_mut(&mut self) -> &mut TxRing {
        &mut self.tx
    }

    pub fn link(&self) -> LinkState {
        self.link
    }

    pub fn offloads(&self) -> OffloadFlags {
        self.offloads
    }
}

pub fn smoke_test() -> bool {
    let mut ring = NetRing::new(4, 64, 4, 64, 4);
    let _ = ring.tx_submit(&[1, 2, 3, 4]);
    ring.set_link(LinkState::up(1000, true));
    ring.set_offloads(OffloadFlags::default());
    ring.link().up && !ring.offloads().tso
}
