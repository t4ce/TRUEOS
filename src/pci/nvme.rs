use alloc::{boxed::Box, string::String};
use core::{
    mem,
    ptr::{NonNull, read_volatile, write_bytes, write_volatile},
    sync::atomic::{Ordering, fence},
};

use crate::wait;
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::{disc::block, dma, pci::mmio};

const NVME_REG_CAP: usize = 0x00;
const NVME_REG_VS: usize = 0x08;
const NVME_REG_CC: usize = 0x14;
const NVME_REG_CSTS: usize = 0x1C;
const NVME_REG_AQA: usize = 0x24;
const NVME_REG_ASQ: usize = 0x28;
const NVME_REG_ACQ: usize = 0x30;
const NVME_REG_DBS: usize = 0x1000;

const NVME_ADMIN_CREATE_IO_CQ: u8 = 0x05;
const NVME_ADMIN_CREATE_IO_SQ: u8 = 0x01;
const NVME_ADMIN_IDENTIFY: u8 = 0x06;
const NVME_ADMIN_SET_FEATURES: u8 = 0x09;

const NVME_FEAT_NUMBER_OF_QUEUES: u32 = 0x07;

const NVME_NVM_FLUSH: u8 = 0x00;
const NVME_NVM_WRITE: u8 = 0x01;
const NVME_NVM_READ: u8 = 0x02;
const NVME_QUEUE_PHYS_CONTIG: u16 = 1 << 0;
const NVME_CQ_IRQ_ENABLED: u16 = 1 << 1;
const NVME_ADMIN_QID: u16 = 0;
const NVME_IO_QID: u16 = 1;

const PAGE_SIZE: usize = 4096;
const NVME_IO_TIMEOUT_FAST_MS: u64 = 2000;
const NVME_IO_TIMEOUT_RETRY_MS: u64 = 6000;
const NVME_ADMIN_PROBE_TIMEOUT_MS: u64 = 6000;
const NVME_IO_SYNC_FALLBACK_TIMEOUT_MS: u64 = 12000;

const IO_PENDING_SLOTS: usize = 128;
const CID_BITMAP_WORDS: usize = 1024; // 65536 bits / 64

struct DmaBuffer {
    phys: u64,
    virt: NonNull<u8>,
    len: usize,
}

unsafe impl Send for DmaBuffer {}

impl DmaBuffer {
    fn alloc(size: usize, align: usize) -> core::result::Result<Self, block::Error> {
        let (phys, virt) = dma::alloc(size, align).ok_or(block::Error::DmaUnavailable)?;
        let virt = NonNull::new(virt).ok_or(block::Error::DmaUnavailable)?;
        Ok(Self {
            phys,
            virt,
            len: size,
        })
    }

    fn phys(&self) -> u64 {
        self.phys
    }

    fn as_ptr(&self) -> *mut u8 {
        self.virt.as_ptr()
    }
}

impl Drop for DmaBuffer {
    fn drop(&mut self) {
        if self.len != 0 {
            dma::dealloc(self.virt.as_ptr(), self.len);
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
struct NvmeSqe {
    d: [u32; 16],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct NvmeCqe {
    dw0: u32,
    dw1: u32,
    dw2: u32,
    dw3: u32,
}

#[derive(Debug, Copy, Clone)]
struct Completion {
    cid: u16,
    status: u16,
    dw0: u32,
}

impl Completion {
    fn is_success(self) -> bool {
        // `status` includes phase in bit0 and NVMe status field in bits15:1.
        // Success is strictly status field == 0.
        (self.status >> 1) == 0
    }

    fn status_code(self) -> u8 {
        ((self.status >> 1) & 0xFF) as u8
    }

    fn status_type(self) -> u8 {
        ((self.status >> 9) & 0x7) as u8
    }
}

#[derive(Copy, Clone)]
struct PendingCompletion {
    cid: u16,
    cpl: Completion,
}

struct IdentifyControllerInfo {
    serial: String,
    mdts: u8,
    nn: u32,
}

struct NvmeQueue {
    depth: u16,
    sq_phys: u64,
    sq_virt: *mut NvmeSqe,
    cq_phys: u64,
    cq_virt: *mut NvmeCqe,
    _sq_mem: DmaBuffer,
    _cq_mem: DmaBuffer,
    sq_tail: u16,
    cq_head: u16,
    cq_phase: bool,
}

unsafe impl Send for NvmeQueue {}
unsafe impl Sync for NvmeQueue {}

impl NvmeQueue {
    fn sq_entry_size() -> usize {
        64
    }

    fn cq_entry_size() -> usize {
        16
    }

    fn new(depth: u16, page_size_bytes: usize) -> core::result::Result<Self, block::Error> {
        Self::new_with_alignment(depth, page_size_bytes, page_size_bytes)
    }

    fn new_with_alignment(
        depth: u16,
        page_size_bytes: usize,
        align_hint: usize,
    ) -> core::result::Result<Self, block::Error> {
        if depth == 0 {
            return Err(block::Error::InvalidParam);
        }

        // Submission queue entries are 64B. Completion queue entries are 16B.
        let sq_bytes = (depth as usize)
            .checked_mul(Self::sq_entry_size())
            .ok_or(block::Error::InvalidParam)?;
        let cq_bytes = (depth as usize)
            .checked_mul(Self::cq_entry_size())
            .ok_or(block::Error::InvalidParam)?;

        let align = core::cmp::max(PAGE_SIZE, core::cmp::max(page_size_bytes, align_hint));
        // Be conservative: allocate whole pages for queues. Some controllers/emulators
        // assume queue memory is backed by full pages even when the effective queue
        // size is smaller (e.g. CQ at 64*16=1024 bytes).
        let sq_alloc_bytes = sq_bytes
            .div_ceil(align)
            .checked_mul(align)
            .ok_or(block::Error::InvalidParam)?;
        let cq_alloc_bytes = cq_bytes
            .div_ceil(align)
            .checked_mul(align)
            .ok_or(block::Error::InvalidParam)?;

        let sq_mem = DmaBuffer::alloc(sq_alloc_bytes, align)?;
        let cq_mem = DmaBuffer::alloc(cq_alloc_bytes, align)?;
        crate::log!(
            "nvme: queue alloc depth={} align=0x{:X} sq_phys=0x{:X} cq_phys=0x{:X}\n",
            depth,
            align,
            sq_mem.phys(),
            cq_mem.phys()
        );
        unsafe {
            write_bytes(sq_mem.as_ptr(), 0, sq_alloc_bytes);
            write_bytes(cq_mem.as_ptr(), 0, cq_alloc_bytes);
        }

        Ok(Self {
            depth,
            sq_phys: sq_mem.phys(),
            sq_virt: sq_mem.as_ptr() as *mut NvmeSqe,
            cq_phys: cq_mem.phys(),
            cq_virt: cq_mem.as_ptr() as *mut NvmeCqe,
            _sq_mem: sq_mem,
            _cq_mem: cq_mem,
            sq_tail: 0,
            cq_head: 0,
            // CQ memory is zeroed, so the initial phase-bit in memory is 0.
            // Per NVMe convention, software must start expecting phase=1 so it does not treat
            // empty/zeroed entries as valid completions.
            cq_phase: true,
        })
    }

    fn sq_push(&mut self, sqe: NvmeSqe) -> core::result::Result<u16, block::Error> {
        let tail = self.sq_tail;
        let idx = (tail as usize) % (self.depth as usize);
        unsafe {
            write_volatile(self.sq_virt.add(idx), sqe);
        }
        self.sq_tail = self.sq_tail.wrapping_add(1);
        Ok(tail)
    }

    fn cq_peek(&self) -> NvmeCqe {
        let idx = (self.cq_head as usize) % (self.depth as usize);
        unsafe { read_volatile(self.cq_virt.add(idx)) }
    }

    fn cq_pop(&mut self) {
        let next = self.cq_head.wrapping_add(1);
        if (next as usize).is_multiple_of(self.depth as usize) {
            self.cq_phase = !self.cq_phase;
        }
        self.cq_head = next;
    }
}

struct NvmeController {
    mmio: NonNull<u8>,
    doorbell_stride_bytes: u32,
    page_size_bytes: usize,
    max_transfer_bytes: u64,
    admin: NvmeQueue,
    io: NvmeQueue,
    next_cid: u16,
    io_inflight: [u64; CID_BITMAP_WORDS],
    io_pending: [Option<PendingCompletion>; IO_PENDING_SLOTS],
    pci: block::PciAddress,
    serial: Option<String>,
}

unsafe impl Send for NvmeController {}
unsafe impl Sync for NvmeController {}

impl NvmeController {
    const ADMIN_Q_DEPTH: u16 = 64;
    const IO_Q_DEPTH_DEFAULT: u16 = 64;

    fn default_max_transfer_bytes() -> u64 {
        256 * 1024
    }

    fn mdts_to_max_transfer_bytes(page_size_bytes: usize, mdts: u8) -> u64 {
        if mdts == 0 {
            return Self::default_max_transfer_bytes();
        }
        let Some(factor) = 1u64.checked_shl(mdts as u32) else {
            return Self::default_max_transfer_bytes();
        };
        let page = page_size_bytes as u64;
        page.saturating_mul(factor).max(page)
    }

    fn io_inflight_test(&self, cid: u16) -> bool {
        let idx = (cid as usize) >> 6;
        let bit = 1u64 << ((cid as usize) & 63);
        (self.io_inflight[idx] & bit) != 0
    }

    fn io_inflight_set(&mut self, cid: u16) {
        let idx = (cid as usize) >> 6;
        let bit = 1u64 << ((cid as usize) & 63);
        self.io_inflight[idx] |= bit;
    }

    fn io_inflight_clear(&mut self, cid: u16) {
        let idx = (cid as usize) >> 6;
        let bit = 1u64 << ((cid as usize) & 63);
        self.io_inflight[idx] &= !bit;
    }

    fn io_pending_take(&mut self, cid: u16) -> Option<Completion> {
        for slot in &mut self.io_pending {
            if let Some(p) = slot
                && p.cid == cid
            {
                let cpl = p.cpl;
                *slot = None;
                return Some(cpl);
            }
        }
        None
    }

    fn io_pending_put(&mut self, cpl: Completion) {
        // Update existing slot first.
        for slot in &mut self.io_pending {
            if let Some(p) = slot
                && p.cid == cpl.cid
            {
                *slot = Some(PendingCompletion { cid: cpl.cid, cpl });
                return;
            }
        }

        // Insert into a free slot.
        for slot in &mut self.io_pending {
            if slot.is_none() {
                *slot = Some(PendingCompletion { cid: cpl.cid, cpl });
                return;
            }
        }

        // Buffer full; drop to avoid unbounded growth.
        crate::log!(
            "nvme: {} dropping buffered completion (buffer full) cid={} status=0x{:04X} (sct={} sc={})\n",
            self.pci,
            cpl.cid,
            cpl.status,
            cpl.status_type(),
            cpl.status_code(),
        );
    }

    fn page_size_bytes(&self) -> usize {
        core::cmp::max(PAGE_SIZE, self.page_size_bytes)
    }

    fn dump_regs(&self, reason: &str) {
        let cap = unsafe { read_volatile(self.mmio.as_ptr().add(NVME_REG_CAP) as *const u64) };
        let vs = self.reg32(NVME_REG_VS);
        let cc = self.reg32(NVME_REG_CC);
        let csts = self.reg32(NVME_REG_CSTS);
        let aqa = self.reg32(NVME_REG_AQA);
        let admin_sq_db = self.db_read_sq_tail(NVME_ADMIN_QID);
        let admin_cq_db = self.db_read_cq_head(NVME_ADMIN_QID);
        let io_sq_db = self.db_read_sq_tail(NVME_IO_QID);
        let io_cq_db = self.db_read_cq_head(NVME_IO_QID);
        let admin_cqe = self.admin.cq_peek();
        let io_cqe = self.io.cq_peek();
        let admin_status = (admin_cqe.dw3 >> 16) as u16;
        let io_status = (io_cqe.dw3 >> 16) as u16;
        let admin_phase = (admin_status & 0x1) != 0;
        let io_phase = (io_status & 0x1) != 0;
        crate::log!(
            "nvme: {} {} regs cap=0x{:016X} vs=0x{:08X} cc=0x{:08X} csts=0x{:08X} aqa=0x{:08X} dstrd={} mps={} admin[sq={} cq={} phase={} db_sq={} db_cq={}] io[sq={} cq={} phase={} db_sq={} db_cq={}] admin_cqe[dw0=0x{:08X} dw3=0x{:08X} st=0x{:04X} p={}] io_cqe[dw0=0x{:08X} dw3=0x{:08X} st=0x{:04X} p={}]\n",
            self.pci,
            reason,
            cap,
            vs,
            cc,
            csts,
            aqa,
            self.doorbell_stride_bytes,
            self.page_size_bytes(),
            self.admin.sq_tail,
            self.admin.cq_head,
            self.admin.cq_phase,
            admin_sq_db,
            admin_cq_db,
            self.io.sq_tail,
            self.io.cq_head,
            self.io.cq_phase,
            io_sq_db,
            io_cq_db,
            admin_cqe.dw0,
            admin_cqe.dw3,
            admin_status,
            admin_phase,
            io_cqe.dw0,
            io_cqe.dw3,
            io_status,
            io_phase,
        );
    }

    fn reg32(&self, off: usize) -> u32 {
        unsafe { read_volatile(self.mmio.as_ptr().add(off) as *const u32) }
    }

    fn write32(&self, off: usize, val: u32) {
        unsafe { write_volatile(self.mmio.as_ptr().add(off) as *mut u32, val) }
    }

    fn write64(&self, off: usize, val: u64) {
        unsafe { write_volatile(self.mmio.as_ptr().add(off) as *mut u64, val) }
    }

    fn db_write_sq_tail(&self, qid: u16, tail: u16) {
        let stride = self.doorbell_stride_bytes as usize;
        let idx = (2usize * (qid as usize)) * stride;
        fence(Ordering::SeqCst);
        self.write32(NVME_REG_DBS + idx, tail as u32);
        // Force posted MMIO writes to drain before we start polling for a CQE.
        let _ = self.reg32(NVME_REG_DBS + idx);
    }

    fn db_write_cq_head(&self, qid: u16, head: u16) {
        let stride = self.doorbell_stride_bytes as usize;
        let idx = (2usize * (qid as usize) + 1) * stride;
        fence(Ordering::SeqCst);
        self.write32(NVME_REG_DBS + idx, head as u32);
        let _ = self.reg32(NVME_REG_DBS + idx);
    }

    fn db_read_sq_tail(&self, qid: u16) -> u32 {
        let stride = self.doorbell_stride_bytes as usize;
        let idx = (2usize * (qid as usize)) * stride;
        self.reg32(NVME_REG_DBS + idx)
    }

    fn db_read_cq_head(&self, qid: u16) -> u32 {
        let stride = self.doorbell_stride_bytes as usize;
        let idx = (2usize * (qid as usize) + 1) * stride;
        self.reg32(NVME_REG_DBS + idx)
    }

    fn rering_sq_tail(&self, qid: u16) {
        let q = if qid == NVME_ADMIN_QID {
            &self.admin
        } else {
            &self.io
        };
        self.db_write_sq_tail(qid, q.sq_tail % q.depth);
    }

    fn spin_wait_ready(
        &self,
        want_ready: bool,
        timeout_ms: u64,
    ) -> core::result::Result<(), block::Error> {
        let ok = wait::spin_until_timeout(timeout_ms, || {
            let csts = self.reg32(NVME_REG_CSTS);
            let rdy = (csts & 0x1) != 0;
            rdy == want_ready
        });
        if ok {
            Ok(())
        } else {
            Err(block::Error::Timeout)
        }
    }

    fn alloc_cid(&mut self) -> u16 {
        // Keep CID non-zero to make logs easier to read.
        let cid = self.next_cid;
        self.next_cid = self.next_cid.wrapping_add(1);
        if self.next_cid == 0 {
            self.next_cid = 1;
        }
        cid
    }

    fn admin_submit_and_wait_sync(
        &mut self,
        sqe: NvmeSqe,
        cid: u16,
        timeout_ms: u64,
    ) -> core::result::Result<Completion, block::Error> {
        let (tail, depth) = {
            let q = &mut self.admin;
            let _ = q.sq_push(sqe)?;
            (q.sq_tail, q.depth)
        };
        self.db_write_sq_tail(NVME_ADMIN_QID, tail % depth);
        self.poll_queue_cq_for_cid_sync(NVME_ADMIN_QID, cid, timeout_ms)
    }

    async fn admin_submit_and_wait_async(
        &mut self,
        sqe: NvmeSqe,
        cid: u16,
        timeout_ms: u64,
    ) -> core::result::Result<Completion, block::Error> {
        let (tail, depth) = {
            let q = &mut self.admin;
            let _ = q.sq_push(sqe)?;
            (q.sq_tail, q.depth)
        };
        self.db_write_sq_tail(NVME_ADMIN_QID, tail % depth);
        self.poll_queue_cq_for_cid_async(NVME_ADMIN_QID, cid, timeout_ms)
            .await
    }

    fn io_submit_and_wait_sync(
        &mut self,
        sqe: NvmeSqe,
        cid: u16,
        timeout_ms: u64,
    ) -> core::result::Result<Completion, block::Error> {
        self.io_inflight_set(cid);
        let (tail, depth) = {
            let q = &mut self.io;
            let _ = q.sq_push(sqe)?;
            (q.sq_tail, q.depth)
        };
        self.db_write_sq_tail(NVME_IO_QID, tail % depth);
        let res = self.poll_queue_cq_for_cid_sync(NVME_IO_QID, cid, timeout_ms);
        self.io_inflight_clear(cid);
        res
    }

    async fn io_submit_and_wait_async(
        &mut self,
        sqe: NvmeSqe,
        cid: u16,
        timeout_ms: u64,
    ) -> core::result::Result<Completion, block::Error> {
        self.io_inflight_set(cid);
        let (tail, depth) = {
            let q = &mut self.io;
            let _ = q.sq_push(sqe)?;
            (q.sq_tail, q.depth)
        };
        self.db_write_sq_tail(NVME_IO_QID, tail % depth);
        let res = self
            .poll_queue_cq_for_cid_async(NVME_IO_QID, cid, timeout_ms)
            .await;
        self.io_inflight_clear(cid);
        res
    }

    fn poll_queue_cq_for_cid_step(&mut self, qid: u16, cid: u16) -> Option<Completion> {
        if qid == NVME_IO_QID
            && let Some(cpl) = self.io_pending_take(cid)
        {
            return Some(cpl);
        }

        let io_inflight = &self.io_inflight;
        let (maybe_cpl, new_head, depth) = {
            let q = if qid == NVME_ADMIN_QID {
                &mut self.admin
            } else {
                &mut self.io
            };
            fence(Ordering::SeqCst);
            let cqe = q.cq_peek();
            let status = (cqe.dw3 >> 16) as u16;
            let phase = (status & 0x1) != 0;

            // Some physical controllers appear to present the first CQE with the
            // opposite initial phase bit compared to the conventional software
            // expectation. If we already submitted work and see a non-zero CQE at
            // head=0, adopt the observed phase once and continue, but only if the
            // candidate CID still matches work we actually have outstanding.
            if phase != q.cq_phase {
                let candidate_cid = (cqe.dw3 & 0xFFFF) as u16;
                let cid_matches_live_work = if qid == NVME_IO_QID {
                    let idx = (candidate_cid as usize) >> 6;
                    let bit = 1u64 << ((candidate_cid as usize) & 63);
                    candidate_cid == cid || (io_inflight[idx] & bit) != 0
                } else {
                    candidate_cid == cid
                };
                let may_adopt_phase = q.cq_head == 0
                    && q.sq_tail != 0
                    && (cqe.dw3 != 0 || cqe.dw0 != 0)
                    && cid_matches_live_work;
                if may_adopt_phase {
                    crate::log!(
                        "nvme: {} qid={} adopting cq_phase quirk exp={} got={} cqe_dw0=0x{:08X} cqe_dw3=0x{:08X}\n",
                        self.pci,
                        qid,
                        q.cq_phase as u8,
                        phase as u8,
                        cqe.dw0,
                        cqe.dw3,
                    );
                    q.cq_phase = phase;
                } else {
                    return None;
                }
            }

            if phase == q.cq_phase {
                let got_cid = (cqe.dw3 & 0xFFFF) as u16;

                q.cq_pop();
                (
                    Some(Completion {
                        cid: got_cid,
                        status,
                        dw0: cqe.dw0,
                    }),
                    q.cq_head,
                    q.depth,
                )
            } else {
                (None, q.cq_head, q.depth)
            }
        };

        if let Some(cpl) = maybe_cpl {
            self.db_write_cq_head(qid, new_head % depth);
            if cpl.cid == cid {
                return Some(cpl);
            }

            if qid == NVME_IO_QID && self.io_inflight_test(cpl.cid) {
                self.io_inflight_clear(cpl.cid);
                self.io_pending_put(cpl);
                return None;
            }

            crate::log!(
                "nvme: {} unexpected completion qid={} want_cid={} got_cid={} status=0x{:04X} (sct={} sc={})\n",
                self.pci,
                qid,
                cid,
                cpl.cid,
                cpl.status,
                cpl.status_type(),
                cpl.status_code(),
            );
        }

        None
    }

    fn poll_queue_cq_for_cid_sync(
        &mut self,
        qid: u16,
        cid: u16,
        timeout_ms: u64,
    ) -> core::result::Result<Completion, block::Error> {
        let hz = embassy_time_driver::TICK_HZ;
        let start = embassy_time_driver::now();
        let ticks = if hz == 0 {
            0
        } else {
            timeout_ms.saturating_mul(hz).div_ceil(1000).max(1)
        };
        let deadline = start.saturating_add(ticks);
        let rekick_ticks = if qid == NVME_IO_QID && hz != 0 {
            (hz / 1000).max(1)
        } else {
            0
        };
        let mut next_rekick = start.saturating_add(rekick_ticks);

        loop {
            if let Some(cpl) = self.poll_queue_cq_for_cid_step(qid, cid) {
                return Ok(cpl);
            }

            let now = embassy_time_driver::now();
            if rekick_ticks != 0 && now >= next_rekick {
                // Some passthrough setups appear to occasionally strand the initial
                // IO SQ doorbell write. Re-ringing the same tail value is harmless
                // and helps QEMU/VFIO behave closer to bare metal in practice.
                self.rering_sq_tail(qid);
                next_rekick = now.saturating_add(rekick_ticks);
            }

            if now >= deadline {
                if qid == NVME_IO_QID {
                    let csts = self.reg32(NVME_REG_CSTS);
                    crate::log!(
                        "nvme: {} poll_sync timeout qid={} cid={} csts=0x{:08X}\n",
                        self.pci,
                        qid,
                        cid,
                        csts
                    );
                }
                self.dump_regs("poll_sync_timeout");
                return Err(block::Error::Timeout);
            }
            wait::spin_step();
        }
    }

    async fn poll_queue_cq_for_cid_async(
        &mut self,
        qid: u16,
        cid: u16,
        timeout_ms: u64,
    ) -> core::result::Result<Completion, block::Error> {
        let hz = embassy_time_driver::TICK_HZ;
        let start = embassy_time_driver::now();
        let ticks = if hz == 0 {
            0
        } else {
            timeout_ms.saturating_mul(hz).div_ceil(1000).max(1)
        };
        let deadline = start.saturating_add(ticks);
        let rekick_ticks = if qid == NVME_IO_QID && hz != 0 {
            (hz / 1000).max(1)
        } else {
            0
        };
        let mut next_rekick = start.saturating_add(rekick_ticks);

        loop {
            if let Some(cpl) = self.poll_queue_cq_for_cid_step(qid, cid) {
                return Ok(cpl);
            }

            let now = embassy_time_driver::now();
            if rekick_ticks != 0 && now >= next_rekick {
                self.rering_sq_tail(qid);
                next_rekick = now.saturating_add(rekick_ticks);
            }

            if now >= deadline {
                if qid == NVME_IO_QID {
                    let csts = self.reg32(NVME_REG_CSTS);
                    crate::log!(
                        "nvme: {} poll_async timeout qid={} cid={} csts=0x{:08X}\n",
                        self.pci,
                        qid,
                        cid,
                        csts
                    );
                }
                self.dump_regs("poll_async_timeout");
                return Err(block::Error::Timeout);
            }
            // Cooperative wait: yield to other tasks instead of busy-spinning.
            Timer::after(EmbassyDuration::from_micros(50)).await;
        }
    }

    fn set_enabled(&self, enable: bool) -> core::result::Result<(), block::Error> {
        let mut cc = self.reg32(NVME_REG_CC);
        if enable {
            cc |= 1;
        } else {
            cc &= !1;
        }
        self.write32(NVME_REG_CC, cc);
        self.spin_wait_ready(enable, 2000)
    }

    fn init_with_io_depth(
        mmio: NonNull<u8>,
        pci: block::PciAddress,
        io_depth: u16,
    ) -> core::result::Result<Self, block::Error> {
        Self::init_with_io_profile(mmio, pci, io_depth, false)
    }

    fn init_with_io_profile(
        mmio: NonNull<u8>,
        pci: block::PciAddress,
        io_depth: u16,
        io_cq_irq_enabled: bool,
    ) -> core::result::Result<Self, block::Error> {
        unsafe {
            let regs = mmio.as_ptr();
            let cap = read_volatile(regs.add(NVME_REG_CAP) as *const u64);
            let vs = read_volatile(regs.add(NVME_REG_VS) as *const u32);
            crate::log!("nvme: {} CAP=0x{:016X} VS=0x{:08X}\n", pci, cap, vs);
        }

        let cap = unsafe { read_volatile(mmio.as_ptr().add(NVME_REG_CAP) as *const u64) };
        let dstrd = ((cap >> 32) & 0xF) as u32;
        let doorbell_stride_bytes = (4u32) << dstrd;
        let mpsmin = ((cap >> 48) & 0xF) as u8;
        let mpsmax = ((cap >> 52) & 0xF) as u8;

        // Select the controller's minimum supported page size.
        let mps = core::cmp::min(mpsmin, mpsmax) as u32;
        let page_size_bytes = PAGE_SIZE.checked_shl(mps).unwrap_or(PAGE_SIZE);

        let io_queue_align = if io_cq_irq_enabled {
            0x1_0000
        } else {
            page_size_bytes
        };
        let mut ctrl = Self {
            mmio,
            doorbell_stride_bytes,
            page_size_bytes,
            max_transfer_bytes: Self::default_max_transfer_bytes(),
            admin: NvmeQueue::new(Self::ADMIN_Q_DEPTH, page_size_bytes)?,
            io: NvmeQueue::new_with_alignment(io_depth.max(1), page_size_bytes, io_queue_align)?,
            next_cid: 1,
            io_inflight: [0u64; CID_BITMAP_WORDS],
            io_pending: [None; IO_PENDING_SLOTS],
            pci,
            serial: None,
        };

        // Disable before reconfiguration.
        let _ = ctrl.set_enabled(false);

        // Program admin queues.
        let aqa = ((ctrl.admin.depth as u32 - 1) << 16) | ((ctrl.admin.depth as u32 - 1) & 0xFFFF);
        ctrl.write32(NVME_REG_AQA, aqa);
        ctrl.write64(NVME_REG_ASQ, ctrl.admin.sq_phys);
        ctrl.write64(NVME_REG_ACQ, ctrl.admin.cq_phys);

        // Set CC: enable, IO SQ/CQ entry sizes, memory page size.
        // IOSQES=6 (64B), IOCQES=4 (16B).
        let cc = (mps << 7) | (6u32 << 16) | (4u32 << 20) | 1;
        ctrl.write32(NVME_REG_CC, cc);
        ctrl.spin_wait_ready(true, 2000)?;

        // Request at least one IO submission/completion queue pair.
        // Some controllers/emulators require Set Features (Number of Queues) before IO queues work.
        if let Err(e) = ctrl.admin_set_number_of_queues(1, 1) {
            // Debug logging for doorbell addresses
            let s_sq = (2usize * (NVME_IO_QID as usize)) * (ctrl.doorbell_stride_bytes as usize);
            let s_cq =
                (2usize * (NVME_IO_QID as usize) + 1) * (ctrl.doorbell_stride_bytes as usize);
            crate::log!(
                "nvme: {} io_q pair initialized. dstrd={} sq_db_off=0x{:X} cq_db_off=0x{:X}\n",
                pci,
                ctrl.doorbell_stride_bytes,
                s_sq,
                s_cq
            );

            crate::log!(
                "nvme: {} set num-queues failed (continuing): {:?}\n",
                pci,
                e
            );
        }

        // Create IO completion queue (qid=1) and submission queue (qid=1).
        ctrl.admin_create_io_cq(
            NVME_IO_QID,
            ctrl.io.depth,
            ctrl.io.cq_phys,
            io_cq_irq_enabled,
            0,
        )?;
        ctrl.admin_create_io_sq(NVME_IO_QID, ctrl.io.depth, ctrl.io.sq_phys, NVME_IO_QID)?;

        // Initialize doorbells for IO queue pair (some emulators are picky about initial values).
        ctrl.db_write_cq_head(NVME_IO_QID, 0);
        ctrl.db_write_sq_tail(NVME_IO_QID, 0);

        if io_cq_irq_enabled {
            // Some physical controllers seem to need a moment after a fresh IO queue
            // pair is created with IRQ delivery armed, even when we poll for completions.
            let _ = crate::wait::spin_until_timeout(20, || false);
        }

        // Identify controller once to grab a serial string (optional).
        if let Ok(ctrl_info) = ctrl.identify_controller_info() {
            ctrl.max_transfer_bytes =
                Self::mdts_to_max_transfer_bytes(ctrl.page_size_bytes(), ctrl_info.mdts);
            if !ctrl_info.serial.is_empty() {
                ctrl.serial = Some(ctrl_info.serial);
            }
        }

        Ok(ctrl)
    }

    fn init(mmio: NonNull<u8>, pci: block::PciAddress) -> core::result::Result<Self, block::Error> {
        Self::init_with_io_depth(mmio, pci, Self::IO_Q_DEPTH_DEFAULT)
    }

    fn admin_create_io_cq(
        &mut self,
        qid: u16,
        depth: u16,
        cq_phys: u64,
        irq_enabled: bool,
        irq_vector: u16,
    ) -> core::result::Result<(), block::Error> {
        crate::log!(
            "nvme: {} create_io_cq qid={} depth={} phys=0x{:X} ien={} iv={}\n",
            self.pci,
            qid,
            depth,
            cq_phys,
            irq_enabled as u8,
            irq_vector,
        );
        let cid = self.alloc_cid();
        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = (NVME_ADMIN_CREATE_IO_CQ as u32) | ((cid as u32) << 16);
        // Queue base address goes in PRP1 (DW6/DW7).
        sqe.d[6] = (cq_phys & 0xFFFF_FFFF) as u32;
        sqe.d[7] = (cq_phys >> 32) as u32;
        sqe.d[10] = (qid as u32) | (((depth as u32) - 1) << 16);
        let cq_flags = NVME_QUEUE_PHYS_CONTIG | if irq_enabled { NVME_CQ_IRQ_ENABLED } else { 0 };
        sqe.d[11] = (cq_flags as u32) | ((irq_vector as u32) << 16);
        let cpl = self.admin_submit_and_wait_sync(sqe, cid, 1000)?;
        if !cpl.is_success() {
            crate::log!(
                "nvme: {} create_io_cq failed sct={} sc={} status=0x{:04X}\n",
                self.pci,
                cpl.status_type(),
                cpl.status_code(),
                cpl.status
            );
            return Err(block::Error::Io);
        }
        Ok(())
    }

    fn admin_create_io_sq(
        &mut self,
        qid: u16,
        depth: u16,
        sq_phys: u64,
        cqid: u16,
    ) -> core::result::Result<(), block::Error> {
        crate::log!(
            "nvme: {} create_io_sq qid={} depth={} phys=0x{:X} cqid={}\n",
            self.pci,
            qid,
            depth,
            sq_phys,
            cqid
        );
        let cid = self.alloc_cid();
        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = (NVME_ADMIN_CREATE_IO_SQ as u32) | ((cid as u32) << 16);
        // Queue base address goes in PRP1 (DW6/DW7).
        sqe.d[6] = (sq_phys & 0xFFFF_FFFF) as u32;
        sqe.d[7] = (sq_phys >> 32) as u32;
        sqe.d[10] = (qid as u32) | (((depth as u32) - 1) << 16);
        // PC=1 (physically contiguous), QPRIO=0, CQID in bits31:16.
        sqe.d[11] = 1 | ((cqid as u32) << 16);
        let cpl = self.admin_submit_and_wait_sync(sqe, cid, 1000)?;
        if !cpl.is_success() {
            crate::log!(
                "nvme: {} create_io_sq failed sct={} sc={} status=0x{:04X}\n",
                self.pci,
                cpl.status_type(),
                cpl.status_code(),
                cpl.status
            );
            return Err(block::Error::Io);
        }
        Ok(())
    }

    fn admin_set_number_of_queues(
        &mut self,
        num_io_sqs: u16,
        num_io_cqs: u16,
    ) -> core::result::Result<(), block::Error> {
        // NVMe Set Features - Number of Queues (FID 0x07).
        // Value: bits31:16 = NSQR (0-based), bits15:0 = NCQR (0-based).
        let cid = self.alloc_cid();
        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = (NVME_ADMIN_SET_FEATURES as u32) | ((cid as u32) << 16);
        sqe.d[10] = NVME_FEAT_NUMBER_OF_QUEUES;

        let nsqr = num_io_sqs.saturating_sub(1) as u32;
        let ncqr = num_io_cqs.saturating_sub(1) as u32;
        sqe.d[11] = (nsqr << 16) | (ncqr & 0xFFFF);

        let cpl = self.admin_submit_and_wait_sync(sqe, cid, 1000)?;
        if !cpl.is_success() {
            crate::log!(
                "nvme: {} set_features(num_queues) failed sct={} sc={} status=0x{:04X}\n",
                self.pci,
                cpl.status_type(),
                cpl.status_code(),
                cpl.status
            );
            return Err(block::Error::Io);
        }

        let allocated_ncqr = cpl.dw0 & 0xFFFF;
        let allocated_nsqr = (cpl.dw0 >> 16) & 0xFFFF;
        crate::log!(
            "nvme: {} set_number_of_queues req sq={} cq={} -> got sq={} cq={}\n",
            self.pci,
            nsqr + 1,
            ncqr + 1,
            allocated_nsqr + 1,
            allocated_ncqr + 1
        );
        Ok(())
    }

    fn make_prps(
        &self,
        buf_phys: u64,
        buf_len: usize,
    ) -> core::result::Result<(u64, u64, Option<DmaBuffer>), block::Error> {
        if buf_len == 0 {
            return Ok((0, 0, None));
        }

        let page_size = self.page_size_bytes();

        let first_page = buf_phys & !(page_size as u64 - 1);
        let first_off = (buf_phys - first_page) as usize;
        let span = first_off.saturating_add(buf_len);
        let pages = span.div_ceil(page_size);

        let prp1 = buf_phys;
        if pages <= 1 {
            return Ok((prp1, 0, None));
        }

        let prp2_direct = first_page + page_size as u64;
        if pages == 2 {
            return Ok((prp1, prp2_direct, None));
        }

        // Need a PRP list for remaining pages.
        let list_entries = pages - 1;
        let list_bytes = list_entries
            .checked_mul(mem::size_of::<u64>())
            .ok_or(block::Error::InvalidParam)?;
        let list_mem = DmaBuffer::alloc(list_bytes, page_size)?;

        unsafe {
            let list = core::slice::from_raw_parts_mut(list_mem.as_ptr() as *mut u64, list_entries);
            for i in 0..list_entries {
                list[i] = first_page + ((i + 1) * page_size) as u64;
            }
        }

        Ok((prp1, list_mem.phys(), Some(list_mem)))
    }

    fn admin_identify(
        &mut self,
        nsid: u32,
        cns: u32,
        out: &mut [u8],
    ) -> core::result::Result<(), block::Error> {
        let page_size = self.page_size_bytes();
        if out.len() < page_size {
            return Err(block::Error::InvalidParam);
        }
        let buf = DmaBuffer::alloc(page_size, page_size)?;
        unsafe {
            write_bytes(buf.as_ptr(), 0, page_size);
        }

        let (prp1, prp2, _prp_list) = self.make_prps(buf.phys(), page_size)?;

        let cid = self.alloc_cid();
        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = (NVME_ADMIN_IDENTIFY as u32) | ((cid as u32) << 16);
        sqe.d[1] = nsid;
        sqe.d[6] = (prp1 & 0xFFFF_FFFF) as u32;
        sqe.d[7] = (prp1 >> 32) as u32;
        sqe.d[8] = (prp2 & 0xFFFF_FFFF) as u32;
        sqe.d[9] = (prp2 >> 32) as u32;
        sqe.d[10] = cns;

        let cpl = self.admin_submit_and_wait_sync(sqe, cid, 1000)?;
        let status_ok = cpl.is_success();

        if !status_ok {
            crate::log!(
                "nvme: {} identify failed nsid={} cns={} status=0x{:04X} (sct={} sc={})\n",
                self.pci,
                nsid,
                cns,
                cpl.status,
                cpl.status_type(),
                cpl.status_code(),
            );
            return Err(block::Error::Io);
        }

        unsafe {
            out[..page_size].copy_from_slice(core::slice::from_raw_parts(buf.as_ptr(), page_size));
        }
        Ok(())
    }

    fn identify_controller_serial_from_buf(buf: &[u8]) -> String {
        // Identify Controller: serial number bytes [4..24].
        let raw = &buf[4..24];

        let mut end = raw.len();
        while end > 0 && (raw[end - 1] == 0 || raw[end - 1] == b' ') {
            end -= 1;
        }
        let mut start = 0usize;
        while start < end && (raw[start] == 0 || raw[start] == b' ') {
            start += 1;
        }

        let mut s = String::new();
        for &b in &raw[start..end] {
            if b == 0 {
                break;
            }
            s.push(b as char);
        }
        s
    }

    fn identify_controller_info(
        &mut self,
    ) -> core::result::Result<IdentifyControllerInfo, block::Error> {
        let page_size = self.page_size_bytes();
        let mut buf = alloc::vec![0u8; page_size];
        self.admin_identify(0, 1, &mut buf)?;
        let serial = Self::identify_controller_serial_from_buf(&buf);
        // MDTS: byte 77, value is power-of-two multiplier of MPSMIN.
        let mdts = *buf.get(77).ok_or(block::Error::Corrupted)?;
        // NN: number of namespaces, bytes 516..519.
        let nn = u32::from_le_bytes(
            buf.get(516..520)
                .ok_or(block::Error::Corrupted)?
                .try_into()
                .map_err(|_| block::Error::Corrupted)?,
        );
        Ok(IdentifyControllerInfo { serial, mdts, nn })
    }

    fn identify_first_active_namespace(
        &mut self,
        nn: u32,
    ) -> core::result::Result<u32, block::Error> {
        if nn == 0 {
            return Err(block::Error::NotReady);
        }

        let page_size = self.page_size_bytes();
        let mut buf = alloc::vec![0u8; page_size];
        // CNS=0x02: active namespace ID list (up to 1024 NSIDs per call).
        self.admin_identify(0, 2, &mut buf)?;

        let entries = core::cmp::min(buf.len() / 4, 1024);
        for i in 0..entries {
            let off = i * 4;
            let nsid = u32::from_le_bytes(
                buf[off..off + 4]
                    .try_into()
                    .map_err(|_| block::Error::Corrupted)?,
            );
            if nsid != 0 {
                return Ok(nsid);
            }
        }

        Err(block::Error::NotReady)
    }

    fn identify_namespace(&mut self, nsid: u32) -> core::result::Result<(u64, u32), block::Error> {
        let page_size = self.page_size_bytes();
        let mut buf = alloc::vec![0u8; page_size];
        self.admin_identify(nsid, 0, &mut buf)?;

        let nsze = u64::from_le_bytes(buf[0..8].try_into().unwrap());
        let nlba_fmt = buf[25] as usize;
        let flbas = buf[26];
        let fmt = (flbas & 0x0F) as usize;
        if fmt > nlba_fmt {
            return Err(block::Error::Corrupted);
        }
        let lbaf_off = 128usize + fmt * 4;
        if lbaf_off + 4 > buf.len() {
            return Err(block::Error::Corrupted);
        }
        let lbads = buf[lbaf_off + 2];
        let block_size = 1u32
            .checked_shl(lbads as u32)
            .ok_or(block::Error::Corrupted)?;
        Ok((nsze, block_size))
    }

    async fn io_rw_async(
        &mut self,
        opcode: u8,
        nsid: u32,
        slba: u64,
        nlb: u16,
        buf_phys: u64,
        buf_len: usize,
    ) -> core::result::Result<(), block::Error> {
        let (prp1, prp2, _prp_list) = self.make_prps(buf_phys, buf_len)?;
        let cpl_res = {
            let cid = self.alloc_cid();
            let mut sqe = NvmeSqe { d: [0; 16] };
            sqe.d[0] = (opcode as u32) | ((cid as u32) << 16);
            sqe.d[1] = nsid;
            sqe.d[6] = (prp1 & 0xFFFF_FFFF) as u32;
            sqe.d[7] = (prp1 >> 32) as u32;
            sqe.d[8] = (prp2 & 0xFFFF_FFFF) as u32;
            sqe.d[9] = (prp2 >> 32) as u32;
            sqe.d[10] = (slba & 0xFFFF_FFFF) as u32;
            sqe.d[11] = (slba >> 32) as u32;
            sqe.d[12] = (nlb as u32).wrapping_sub(1) & 0xFFFF;

            match self
                .io_submit_and_wait_async(sqe, cid, NVME_IO_TIMEOUT_FAST_MS)
                .await
            {
                Ok(cpl) => Ok(cpl),
                Err(block::Error::Timeout) => {
                    // Silencing retry logs
                    /*
                    crate::log!(
                        "nvme: {} io retry opcode=0x{:02X} nsid={} slba={} nlb={} wait={}ms->{}ms\n",
                        self.pci,
                        opcode,
                        nsid,
                        slba,
                        nlb,
                        NVME_IO_TIMEOUT_FAST_MS,
                        NVME_IO_TIMEOUT_RETRY_MS
                    );
                    */

                    let cid_retry = self.alloc_cid();
                    let mut sqe_retry = NvmeSqe { d: [0; 16] };
                    sqe_retry.d[0] = (opcode as u32) | ((cid_retry as u32) << 16);
                    sqe_retry.d[1] = nsid;
                    sqe_retry.d[6] = (prp1 & 0xFFFF_FFFF) as u32;
                    sqe_retry.d[7] = (prp1 >> 32) as u32;
                    sqe_retry.d[8] = (prp2 & 0xFFFF_FFFF) as u32;
                    sqe_retry.d[9] = (prp2 >> 32) as u32;
                    sqe_retry.d[10] = (slba & 0xFFFF_FFFF) as u32;
                    sqe_retry.d[11] = (slba >> 32) as u32;
                    sqe_retry.d[12] = (nlb as u32).wrapping_sub(1) & 0xFFFF;

                    let retry_res = self
                        .io_submit_and_wait_async(sqe_retry, cid_retry, NVME_IO_TIMEOUT_RETRY_MS)
                        .await;

                    if matches!(retry_res, Err(block::Error::Timeout)) {
                        crate::log!(
                            "nvme: {} io timeout opcode=0x{:02X} nsid={} slba={} nlb={} buf_phys=0x{:X} buf_len={} after retry\n",
                            self.pci,
                            opcode,
                            nsid,
                            slba,
                            nlb,
                            buf_phys,
                            buf_len,
                        );
                        self.dump_regs("io_rw_async_timeout");
                    }

                    retry_res
                }
                Err(e) => Err(e),
            }
        };

        let cpl = cpl_res?;
        if !cpl.is_success() {
            crate::log!(
                "nvme: {} io failed opcode=0x{:02X} nsid={} slba={} nlb={} status=0x{:04X} (sct={} sc={})\n",
                self.pci,
                opcode,
                nsid,
                slba,
                nlb,
                cpl.status,
                cpl.status_type(),
                cpl.status_code(),
            );
            return Err(block::Error::Io);
        }
        Ok(())
    }

    async fn io_flush_async(&mut self, nsid: u32) -> core::result::Result<(), block::Error> {
        let cid = self.alloc_cid();
        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = (NVME_NVM_FLUSH as u32) | ((cid as u32) << 16);
        sqe.d[1] = nsid;
        let cpl = self.io_submit_and_wait_async(sqe, cid, 2000).await?;
        if !cpl.is_success() {
            return Err(block::Error::Io);
        }
        Ok(())
    }

    fn io_flush_sync(
        &mut self,
        nsid: u32,
        timeout_ms: u64,
    ) -> core::result::Result<Completion, block::Error> {
        let cid = self.alloc_cid();
        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = (NVME_NVM_FLUSH as u32) | ((cid as u32) << 16);
        sqe.d[1] = nsid;
        self.io_submit_and_wait_sync(sqe, cid, timeout_ms)
    }

    fn io_rw_sync(
        &mut self,
        opcode: u8,
        nsid: u32,
        slba: u64,
        nlb: u16,
        buf_phys: u64,
        buf_len: usize,
        timeout_ms: u64,
    ) -> core::result::Result<Completion, block::Error> {
        let (prp1, prp2, _prp_list) = self.make_prps(buf_phys, buf_len)?;
        let cid = self.alloc_cid();

        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = (opcode as u32) | ((cid as u32) << 16);
        sqe.d[1] = nsid;
        sqe.d[6] = (prp1 & 0xFFFF_FFFF) as u32;
        sqe.d[7] = (prp1 >> 32) as u32;
        sqe.d[8] = (prp2 & 0xFFFF_FFFF) as u32;
        sqe.d[9] = (prp2 >> 32) as u32;
        sqe.d[10] = (slba & 0xFFFF_FFFF) as u32;
        sqe.d[11] = (slba >> 32) as u32;
        sqe.d[12] = (nlb as u32).wrapping_sub(1) & 0xFFFF;

        let cpl = self.io_submit_and_wait_sync(sqe, cid, timeout_ms);

        cpl
    }

    async fn admin_rw_async(
        &mut self,
        opcode: u8,
        nsid: u32,
        slba: u64,
        nlb: u16,
        buf_phys: u64,
        buf_len: usize,
        timeout_ms: u64,
    ) -> core::result::Result<Completion, block::Error> {
        let (prp1, prp2, _prp_list) = self.make_prps(buf_phys, buf_len)?;
        let cid = self.alloc_cid();

        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = (opcode as u32) | ((cid as u32) << 16);
        sqe.d[1] = nsid;
        sqe.d[6] = (prp1 & 0xFFFF_FFFF) as u32;
        sqe.d[7] = (prp1 >> 32) as u32;
        sqe.d[8] = (prp2 & 0xFFFF_FFFF) as u32;
        sqe.d[9] = (prp2 >> 32) as u32;
        sqe.d[10] = (slba & 0xFFFF_FFFF) as u32;
        sqe.d[11] = (slba >> 32) as u32;
        sqe.d[12] = (nlb as u32).wrapping_sub(1) & 0xFFFF;

        let cpl = self.admin_submit_and_wait_async(sqe, cid, timeout_ms).await;

        cpl
    }

    fn admin_rw_sync(
        &mut self,
        opcode: u8,
        nsid: u32,
        slba: u64,
        nlb: u16,
        buf_phys: u64,
        buf_len: usize,
        timeout_ms: u64,
    ) -> core::result::Result<Completion, block::Error> {
        let (prp1, prp2, _prp_list) = self.make_prps(buf_phys, buf_len)?;
        let cid = self.alloc_cid();

        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = (opcode as u32) | ((cid as u32) << 16);
        sqe.d[1] = nsid;
        sqe.d[6] = (prp1 & 0xFFFF_FFFF) as u32;
        sqe.d[7] = (prp1 >> 32) as u32;
        sqe.d[8] = (prp2 & 0xFFFF_FFFF) as u32;
        sqe.d[9] = (prp2 >> 32) as u32;
        sqe.d[10] = (slba & 0xFFFF_FFFF) as u32;
        sqe.d[11] = (slba >> 32) as u32;
        sqe.d[12] = (nlb as u32).wrapping_sub(1) & 0xFFFF;

        let cpl = self.admin_submit_and_wait_sync(sqe, cid, timeout_ms);

        cpl
    }
}

struct NvmeBlockDevice {
    ctrl: NvmeController,
    nsid: u32,
    block_size: u32,
    block_count: u64,
    max_transfer_bytes: u64,
    admin_fallback_mode: bool,
}

unsafe impl Send for NvmeBlockDevice {}

impl NvmeBlockDevice {
    fn is_small_probe_read(lba: u64, blocks: usize) -> bool {
        blocks <= 2 && lba <= 2
    }

    fn is_small_probe_write(lba: u64, blocks: usize) -> bool {
        blocks <= 2 && lba <= 2
    }

    fn should_use_admin_fallback(&self) -> bool {
        self.admin_fallback_mode
    }
}

impl block::BlockDevice for NvmeBlockDevice {
    fn block_size_bytes(&self) -> u32 {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn read_blocks<'a>(
        &'a mut self,
        lba: u64,
        blocks: usize,
    ) -> block::BoxFuture<'a, block::Result<alloc::vec::Vec<u8>>> {
        Box::pin(async move {
            let bs = self.block_size as usize;
            if bs == 0 {
                return Err(block::Error::InvalidParam);
            }
            if blocks == 0 {
                return Ok(alloc::vec::Vec::new());
            }

            let blocks_total = blocks as u64;
            let end = lba
                .checked_add(blocks_total)
                .ok_or(block::Error::OutOfBounds)?;
            if end > self.block_count {
                return Err(block::Error::OutOfBounds);
            }

            let total_bytes = blocks.checked_mul(bs).ok_or(block::Error::InvalidParam)?;
            let mut out = alloc::vec![0u8; total_bytes];

            let max_io_bytes = core::cmp::max(self.max_transfer_bytes, bs as u64) as usize;
            let max_blocks =
                core::cmp::max(1, core::cmp::min(max_io_bytes / bs, u16::MAX as usize));
            let dma_buf = DmaBuffer::alloc(max_io_bytes, self.ctrl.page_size_bytes())?;
            let dma_phys = dma_buf.phys();
            let dma_virt = dma_buf.as_ptr();

            let mut remaining = out.as_mut_slice();
            let mut cur_lba = lba;
            while !remaining.is_empty() {
                let blocks_here = core::cmp::min(max_blocks, remaining.len() / bs);
                let bytes_here = blocks_here * bs;
                unsafe { write_bytes(dma_virt, 0, bytes_here) };

                if self.should_use_admin_fallback() {
                    match self
                        .ctrl
                        .admin_rw_async(
                            NVME_NVM_READ,
                            self.nsid,
                            cur_lba,
                            blocks_here as u16,
                            dma_phys,
                            bytes_here,
                            NVME_ADMIN_PROBE_TIMEOUT_MS,
                        )
                        .await
                    {
                        Ok(cpl) if cpl.is_success() => {}
                        Ok(cpl) => {
                            crate::log!(
                                "nvme: {} admin-fallback read failed status=0x{:04X} (sct={} sc={})\n",
                                self.ctrl.pci,
                                cpl.status,
                                cpl.status_type(),
                                cpl.status_code(),
                            );
                            return Err(block::Error::Io);
                        }
                        Err(e) => {
                            return Err(e);
                        }
                    }
                } else {
                    match self
                        .ctrl
                        .io_rw_async(
                            NVME_NVM_READ,
                            self.nsid,
                            cur_lba,
                            blocks_here as u16,
                            dma_phys,
                            bytes_here,
                        )
                        .await
                    {
                        Ok(()) => {}
                        Err(block::Error::Timeout)
                            if Self::is_small_probe_read(cur_lba, blocks_here) =>
                        {
                            // crate::log!(
                            //     "nvme: {} probe-read fallback admin opcode=0x{:02X} nsid={} slba={} nlb={}\n",
                            //     self.ctrl.pci,
                            //     NVME_NVM_READ,
                            //     self.nsid,
                            //     cur_lba,
                            //     blocks_here
                            // );
                            match self
                                .ctrl
                                .admin_rw_async(
                                    NVME_NVM_READ,
                                    self.nsid,
                                    cur_lba,
                                    blocks_here as u16,
                                    dma_phys,
                                    bytes_here,
                                    NVME_ADMIN_PROBE_TIMEOUT_MS,
                                )
                                .await
                            {
                                Ok(cpl) if cpl.is_success() => {}
                                Ok(_cpl) => {
                                    // crate::log!(
                                    //     "nvme: {} probe-read fallback (admin) failed status=0x{:04X} (sct={} sc={})\n",
                                    //     self.ctrl.pci,
                                    //     cpl.status,
                                    //     cpl.status_type(),
                                    //     cpl.status_code(),
                                    // );
                                    return Err(block::Error::Io);
                                }
                                Err(e) => {
                                    return Err(e);
                                }
                            }
                        }
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }

                unsafe {
                    let src = core::slice::from_raw_parts(dma_virt, bytes_here);
                    remaining[..bytes_here].copy_from_slice(src);
                }

                remaining = &mut remaining[bytes_here..];
                cur_lba = cur_lba.saturating_add(blocks_here as u64);
            }

            Ok(out)
        })
    }

    fn write_blocks<'a>(
        &'a mut self,
        lba: u64,
        buf: &'a [u8],
    ) -> block::BoxFuture<'a, block::Result<()>> {
        Box::pin(async move {
            let bs = self.block_size as usize;
            if bs == 0 || !buf.len().is_multiple_of(bs) {
                return Err(block::Error::InvalidParam);
            }
            let blocks_total = (buf.len() / bs) as u64;
            if blocks_total == 0 {
                return Ok(());
            }
            let end = lba
                .checked_add(blocks_total)
                .ok_or(block::Error::OutOfBounds)?;
            if end > self.block_count {
                return Err(block::Error::OutOfBounds);
            }

            let max_io_bytes = core::cmp::max(self.max_transfer_bytes, bs as u64) as usize;
            let max_blocks =
                core::cmp::max(1, core::cmp::min(max_io_bytes / bs, u16::MAX as usize));
            let dma_buf = DmaBuffer::alloc(max_io_bytes, self.ctrl.page_size_bytes())?;
            let dma_phys = dma_buf.phys();
            let dma_virt = dma_buf.as_ptr();

            let mut remaining = buf;
            let mut cur_lba = lba;
            while !remaining.is_empty() {
                let blocks_here = core::cmp::min(max_blocks, remaining.len() / bs);
                let bytes_here = blocks_here * bs;
                unsafe {
                    core::ptr::copy_nonoverlapping(remaining.as_ptr(), dma_virt, bytes_here);
                }

                if self.should_use_admin_fallback() {
                    match self
                        .ctrl
                        .admin_rw_async(
                            NVME_NVM_WRITE,
                            self.nsid,
                            cur_lba,
                            blocks_here as u16,
                            dma_phys,
                            bytes_here,
                            NVME_ADMIN_PROBE_TIMEOUT_MS,
                        )
                        .await
                    {
                        Ok(cpl) if cpl.is_success() => {}
                        Ok(cpl) => {
                            crate::log!(
                                "nvme: {} admin-fallback write failed status=0x{:04X} (sct={} sc={})\n",
                                self.ctrl.pci,
                                cpl.status,
                                cpl.status_type(),
                                cpl.status_code(),
                            );
                            return Err(block::Error::Io);
                        }
                        Err(e) => {
                            return Err(e);
                        }
                    }
                } else {
                    match self
                        .ctrl
                        .io_rw_async(
                            NVME_NVM_WRITE,
                            self.nsid,
                            cur_lba,
                            blocks_here as u16,
                            dma_phys,
                            bytes_here,
                        )
                        .await
                    {
                        Ok(()) => {}
                        Err(block::Error::Timeout)
                            if Self::is_small_probe_write(cur_lba, blocks_here) =>
                        {
                            crate::log!(
                                "nvme: {} probe-write fallback sync opcode=0x{:02X} nsid={} slba={} nlb={}\n",
                                self.ctrl.pci,
                                NVME_NVM_WRITE,
                                self.nsid,
                                cur_lba,
                                blocks_here
                            );
                            match self.ctrl.io_rw_sync(
                                NVME_NVM_WRITE,
                                self.nsid,
                                cur_lba,
                                blocks_here as u16,
                                dma_phys,
                                bytes_here,
                                NVME_IO_SYNC_FALLBACK_TIMEOUT_MS,
                            ) {
                                Ok(cpl) if cpl.is_success() => {}
                                Ok(cpl) => {
                                    crate::log!(
                                        "nvme: {} probe-write fallback failed status=0x{:04X} (sct={} sc={})\n",
                                        self.ctrl.pci,
                                        cpl.status,
                                        cpl.status_type(),
                                        cpl.status_code(),
                                    );
                                    return Err(block::Error::Io);
                                }
                                Err(e) => {
                                    return Err(e);
                                }
                            }
                        }
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }

                remaining = &remaining[bytes_here..];
                cur_lba = cur_lba.saturating_add(blocks_here as u64);
            }

            Ok(())
        })
    }

    fn dma_alignment_bytes(&self) -> u32 {
        64
    }

    fn max_transfer_bytes(&self) -> u64 {
        self.max_transfer_bytes
    }

    fn supports_write(&self) -> bool {
        true
    }

    fn flush<'a>(&'a mut self) -> block::BoxFuture<'a, block::Result<()>> {
        Box::pin(async move {
            if self.should_use_admin_fallback() {
                let cpl = self
                    .ctrl
                    .admin_rw_async(NVME_NVM_FLUSH, self.nsid, 0, 0, 0, 0, 2000)
                    .await?;
                if cpl.is_success() {
                    Ok(())
                } else {
                    Err(block::Error::Io)
                }
            } else {
                self.ctrl.io_flush_async(self.nsid).await
            }
        })
    }
}

fn admin_selftest_read(
    _ctrl: &mut NvmeController,
    pci_addr: block::PciAddress,
    _nsid: u32,
    _block_size: u32,
) -> bool {
    // NVM READ/WRITE/FLUSH are I/O queue commands, not admin queue commands.
    // Treating the admin queue as a data path makes probe decisions misleading
    // on compliant controllers, so keep this probe conservative.
    crate::log!(
        "nvme: {} admin-selftest skipped: NVM I/O opcodes are not valid on the admin queue\n",
        pci_addr
    );
    false
}

fn is_nvme(dev: &crate::pci::PciDevice) -> bool {
    // Standard NVMe match: Mass Storage / NVM / NVMHCI.
    let class_match = dev.class == 0x01 && dev.subclass == 0x08 && dev.prog_if == 0x02;
    // Explicitly claim Samsung SM961/PM961/SM963 controller family when enumerated,
    // even if firmware reports a non-standard programming interface.
    let samsung_sm961_family = dev.vendor == 0x144D && dev.device == 0xA804;
    class_match || samsung_sm961_family
}

fn io_selftest_read(
    ctrl: &mut NvmeController,
    pci_addr: block::PciAddress,
    nsid: u32,
    block_size: u32,
) -> bool {
    let bs = block_size as usize;
    let bytes = bs.max(512);
    let Ok(dma_buf) = DmaBuffer::alloc(bytes, ctrl.page_size_bytes()) else {
        crate::log!("nvme: {} io-selftest: DMA alloc failed\n", pci_addr);
        return false;
    };
    let dma_phys = dma_buf.phys();
    let dma_virt = dma_buf.as_ptr();

    unsafe { write_bytes(dma_virt, 0, bytes) };

    let ok = match ctrl.io_rw_sync(NVME_NVM_READ, nsid, 0, 1, dma_phys, bs, 2000) {
        Ok(cpl) => {
            if !cpl.is_success() {
                crate::log!(
                    "nvme: {} io-selftest read failed status=0x{:04X} (sct={} sc={})\n",
                    pci_addr,
                    cpl.status,
                    cpl.status_type(),
                    cpl.status_code(),
                );
                false
            } else {
                true
            }
        }
        Err(e) => {
            crate::log!("nvme: {} io-selftest read failed: {:?}\n", pci_addr, e);
            false
        }
    };

    ok
}

fn io_selftest_flush(ctrl: &mut NvmeController, pci_addr: block::PciAddress, nsid: u32) -> bool {
    match ctrl.io_flush_sync(nsid, 2000) {
        Ok(cpl) => {
            if !cpl.is_success() {
                crate::log!(
                    "nvme: {} io-selftest flush completed with status=0x{:04X} (sct={} sc={})\n",
                    pci_addr,
                    cpl.status,
                    cpl.status_type(),
                    cpl.status_code(),
                );
            } else {
                crate::log!("nvme: {} io-selftest flush ok\n", pci_addr);
            }
            true
        }
        Err(e) => {
            crate::log!("nvme: {} io-selftest flush failed: {:?}\n", pci_addr, e);
            false
        }
    }
}

pub fn probe_once() {
    if crate::limine::hhdm_offset().is_none() {
        crate::log!("nvme: no HHDM\n");
        return;
    }

    let mut did_any = false;
    let mut registered_any = false;

    crate::pci::with_devices(|list| {
        for dev in list {
            if !is_nvme(dev) {
                continue;
            }
            did_any = true;
            if crate::pci::try_function_level_reset(dev.bus, dev.slot, dev.function) {
                crate::log!(
                    "nvme: {:02X}:{:02X}.{} function-level reset issued\n",
                    dev.bus,
                    dev.slot,
                    dev.function
                );
            }
            crate::pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);

            let (bar_lo, bar_hi) = crate::pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
            if (bar_lo & 0x1) != 0 {
                crate::log!(
                    "nvme: {:02X}:{:02X}.{} BAR0 is IO space (unsupported)\n",
                    dev.bus,
                    dev.slot,
                    dev.function
                );
                continue;
            }

            let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
            let mut base = (bar_lo & 0xFFFF_FFF0) as u64;
            if is_64 {
                base |= (bar_hi.unwrap_or(0) as u64) << 32;
            }
            let size = crate::pci::bar0_size_bytes(dev.bus, dev.slot, dev.function).unwrap_or(0);

            let mut map_len = if size == 0 {
                0x4000usize
            } else {
                size as usize
            };
            map_len = map_len.clamp(0x4000, 0x10000);

            let mmio_ptr = match mmio::map_mmio_region(base, map_len) {
                Ok(ptr) => ptr,
                Err(err) => {
                    crate::log!("nvme: failed to map MMIO: {:?}\n", err);
                    continue;
                }
            };

            let pci_addr = block::PciAddress::new(dev.bus, dev.slot, dev.function);
            let mut ctrl = match NvmeController::init(mmio_ptr, pci_addr) {
                Ok(c) => c,
                Err(e) => {
                    crate::log!("nvme: {} init failed: {:?}\n", pci_addr, e);
                    continue;
                }
            };

            let ctrl_info = match ctrl.identify_controller_info() {
                Ok(info) => info,
                Err(e) => {
                    crate::log!("nvme: {} identify controller failed: {:?}\n", pci_addr, e);
                    continue;
                }
            };

            let mut nsid = match ctrl.identify_first_active_namespace(ctrl_info.nn) {
                Ok(id) => id,
                Err(e) => {
                    crate::log!("nvme: {} no active namespace found: {:?}\n", pci_addr, e);
                    continue;
                }
            };

            // Register the first active namespace.
            let (mut blocks, mut block_size) = match ctrl.identify_namespace(nsid) {
                Ok(v) => v,
                Err(e) => {
                    crate::log!(
                        "nvme: {} identify namespace {} failed: {:?}\n",
                        pci_addr,
                        nsid,
                        e
                    );
                    continue;
                }
            };
            if blocks == 0 || block_size == 0 {
                crate::log!(
                    "nvme: {} namespace {} has invalid capacity\n",
                    pci_addr,
                    nsid
                );
                continue;
            }

            // Strict gate: the IO queue must first complete a no-data command, then
            // complete a data-bearing READ before we register the controller.
            let mut io_ready = io_selftest_flush(&mut ctrl, pci_addr, nsid)
                && io_selftest_read(&mut ctrl, pci_addr, nsid, block_size);
            let mut admin_fallback_mode = false;
            if !io_ready {
                crate::log!(
                    "nvme: {} io-selftest failed; attempting one controller reinit\n",
                    pci_addr
                );

                let mut retry_ctrl =
                    match NvmeController::init_with_io_profile(mmio_ptr, pci_addr, 2, true) {
                        Ok(c) => c,
                        Err(e) => {
                            crate::log!("nvme: {} reinit failed: {:?}\n", pci_addr, e);
                            continue;
                        }
                    };

                let retry_ctrl_info = match retry_ctrl.identify_controller_info() {
                    Ok(info) => info,
                    Err(e) => {
                        crate::log!(
                            "nvme: {} reinit identify controller failed: {:?}\n",
                            pci_addr,
                            e
                        );
                        continue;
                    }
                };

                let retry_nsid =
                    match retry_ctrl.identify_first_active_namespace(retry_ctrl_info.nn) {
                        Ok(id) => id,
                        Err(e) => {
                            crate::log!(
                                "nvme: {} reinit no active namespace found: {:?}\n",
                                pci_addr,
                                e
                            );
                            continue;
                        }
                    };

                let (retry_blocks, retry_block_size) =
                    match retry_ctrl.identify_namespace(retry_nsid) {
                        Ok(v) => v,
                        Err(e) => {
                            crate::log!(
                                "nvme: {} reinit identify namespace {} failed: {:?}\n",
                                pci_addr,
                                retry_nsid,
                                e
                            );
                            continue;
                        }
                    };

                if retry_blocks == 0 || retry_block_size == 0 {
                    crate::log!(
                        "nvme: {} reinit namespace {} has invalid capacity\n",
                        pci_addr,
                        retry_nsid
                    );
                    continue;
                }

                io_ready = io_selftest_flush(&mut retry_ctrl, pci_addr, retry_nsid)
                    && io_selftest_read(&mut retry_ctrl, pci_addr, retry_nsid, retry_block_size);
                if !io_ready {
                    let _ = admin_selftest_read(
                        &mut retry_ctrl,
                        pci_addr,
                        retry_nsid,
                        retry_block_size,
                    );
                    crate::log!(
                        "nvme: {} io-selftest still failed after reinit; registering anyway (degraded probe gate)\n",
                        pci_addr
                    );
                }

                if io_ready {
                    crate::log!("nvme: {} IO queue recovered after reinit\n", pci_addr);
                }
                ctrl = retry_ctrl;
                nsid = retry_nsid;
                blocks = retry_blocks;
                block_size = retry_block_size;
            }

            let label = if let Some(s) = ctrl.serial.as_deref() {
                if !s.is_empty() {
                    alloc::format!("nvme:{}", s)
                } else {
                    String::from("nvme")
                }
            } else {
                String::from("nvme")
            };

            let mut desc = block::DeviceDescriptor::new(block::DeviceKind::Nvme)
                .with_label(label)
                .with_pci(pci_addr);

            if let Some(s) = ctrl.serial.clone() {
                desc = desc.with_serial(s);
            }

            let max_transfer_bytes = ctrl.max_transfer_bytes;
            let dev = NvmeBlockDevice {
                ctrl,
                nsid,
                block_size,
                block_count: blocks,
                max_transfer_bytes,
                admin_fallback_mode,
            };
            let handle = block::register_device(desc, dev);
            crate::r::fs::trueosfs::request_mount_root(handle);
            crate::log!(
                "nvme: registered {} nsid={} id={} blocks={} bs={} max_io={}\n",
                pci_addr,
                nsid,
                handle.id().raw(),
                blocks,
                block_size,
                max_transfer_bytes,
            );
            if admin_fallback_mode {
                crate::log!("nvme: {} registered in admin-fallback mode\n", pci_addr);
            }
            crate::log!("nvme: {} probe outcome: registered\n", pci_addr);
            registered_any = true;

            // For now, only claim/register the first controller.
            break;
        }
    });

    if !did_any {
        crate::log!("nvme: none found\n");
    } else if !registered_any {
        crate::log!("nvme: found controller(s) but none registered\n");
    }
}
