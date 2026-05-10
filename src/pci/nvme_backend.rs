use alloc::{boxed::Box, string::String, vec::Vec};
use core::{
    mem,
    ptr::{
        NonNull, copy_nonoverlapping, read_unaligned, read_volatile, write_bytes, write_volatile,
    },
};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

use crate::{disc::block, dma, wait};

const NVME_REG_CAP: usize = 0x00;
const NVME_REG_VS: usize = 0x08;
const NVME_REG_INTMS: usize = 0x0C;
const NVME_REG_CC: usize = 0x14;
const NVME_REG_CSTS: usize = 0x1C;
const NVME_REG_AQA: usize = 0x24;
const NVME_REG_ASQ: usize = 0x28;
const NVME_REG_ACQ: usize = 0x30;
const NVME_REG_DBS: usize = 0x1000;

const NVME_CC_EN: u32 = 1 << 0;
const NVME_CC_CSS_NVM: u32 = 0 << 4;
const NVME_CC_MPS_4K: u32 = 0 << 7;
const NVME_CC_AMS_RR: u32 = 0 << 11;
const NVME_CC_IOSQES: u32 = 6 << 16;
const NVME_CC_IOCQES: u32 = 4 << 20;

const NVME_CSTS_RDY: u32 = 1 << 0;
const NVME_CSTS_CFS: u32 = 1 << 1;

const NVME_ADMIN_CREATE_IO_SQ: u8 = 0x01;
const NVME_ADMIN_CREATE_IO_CQ: u8 = 0x05;
const NVME_ADMIN_IDENTIFY: u8 = 0x06;
const NVME_ADMIN_SET_FEATURES: u8 = 0x09;

const NVME_FEAT_NUMBER_OF_QUEUES: u32 = 0x07;

const NVME_NVM_FLUSH: u8 = 0x00;
const NVME_NVM_WRITE: u8 = 0x01;
const NVME_NVM_READ: u8 = 0x02;

const NVME_IDENTIFY_NAMESPACE: u32 = 0x00;
const NVME_IDENTIFY_CONTROLLER: u32 = 0x01;
const NVME_IDENTIFY_ACTIVE_NSID_LIST: u32 = 0x02;

const PAGE_SIZE: usize = 4096;
const ADMIN_TIMEOUT_MS: u64 = crate::allcaps::storage::NVME_ADMIN_TIMEOUT_MS;
const IO_TIMEOUT_MS: u64 = crate::allcaps::storage::NVME_IO_TIMEOUT_MS;
// Keep a conservative floor even when CAP.TO reports a shorter controller timeout.
const READY_TIMEOUT_MS: u64 = crate::allcaps::storage::NVME_READY_TIMEOUT_MS;
const NVME_CAP_TO_GRANULARITY_MS: u64 = crate::allcaps::storage::NVME_CAP_TO_GRANULARITY_MS;
// Bound a tiny hot-poll window before yielding so immediate completions do not
// pay a full timer tick.
const IO_HOT_POLL_LIMIT: usize = crate::allcaps::storage::NVME_IO_HOT_POLL_LIMIT;
const IO_POLL_INTERVAL_MS: u64 = crate::allcaps::storage::NVME_IO_POLL_INTERVAL_MS;
const QUEUE_DEPTH_CAP: u16 = crate::allcaps::storage::NVME_QUEUE_DEPTH_CAP;
const IO_TRANSFER_PAGES_CAP: u64 = crate::allcaps::storage::NVME_IO_TRANSFER_PAGES_CAP;
const IO_QID: u16 = 1;

#[derive(Copy, Clone)]
#[repr(C)]
struct NvmeSqe {
    d: [u32; 16],
}

#[derive(Copy, Clone)]
#[repr(C)]
struct NvmeCqe {
    dw0: u32,
    dw1: u32,
    dw2: u32,
    dw3: u32,
}

impl NvmeCqe {
    fn status_field(self) -> u16 {
        ((self.dw3 >> 16) & 0xFFFF) as u16
    }

    fn phase(self) -> bool {
        (self.status_field() & 0x1) != 0
    }

    fn command_id(self) -> u16 {
        (self.dw3 & 0xFFFF) as u16
    }

    fn status_code(self) -> u8 {
        ((self.status_field() >> 1) & 0xFF) as u8
    }

    fn status_type(self) -> u8 {
        ((self.status_field() >> 9) & 0x7) as u8
    }

    fn do_not_retry(self) -> bool {
        (self.status_field() & (1 << 14)) != 0
    }

    fn is_success(self) -> bool {
        (self.status_field() >> 1) == 0
    }
}

fn nvme_status_name(sct: u8, sc: u8) -> &'static str {
    match (sct, sc) {
        (0x0, 0x00) => "success",
        (0x0, 0x01) => "invalid opcode",
        (0x0, 0x02) => "invalid field",
        (0x0, 0x06) => "internal error",
        (0x0, 0x0B) => "invalid namespace or format",
        (0x0, 0x14) => "command sequence error",
        (0x1, 0x80) => "lba out of range",
        (0x1, 0x81) => "capacity exceeded",
        (0x1, 0x82) => "namespace not ready",
        (0x1, 0x86) => "access denied",
        _ => "unknown",
    }
}

struct DmaBuffer {
    phys: u64,
    virt: NonNull<u8>,
    len: usize,
}

#[inline]
fn nvme_dma_cache_flush(ptr: *const u8, len: usize) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use core::arch::x86_64::{_mm_clflush, _mm_mfence};

        if ptr.is_null() || len == 0 {
            return;
        }

        let line = 64usize;
        let start = (ptr as usize) & !(line - 1);
        let end = (ptr as usize).saturating_add(len);
        let mut cur = start;
        while cur < end {
            _mm_clflush(cur as *const _);
            cur = cur.saturating_add(line);
        }
        _mm_mfence();
    }
}

#[inline]
fn nvme_dma_cache_invalidate(ptr: *const u8, len: usize) {
    nvme_dma_cache_flush(ptr, len);
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

    fn flush_range(&self, offset: usize, len: usize) {
        if offset >= self.len || len == 0 {
            return;
        }
        let span = len.min(self.len.saturating_sub(offset));
        unsafe { nvme_dma_cache_flush(self.virt.as_ptr().add(offset), span) };
    }

    fn invalidate_range(&self, offset: usize, len: usize) {
        if offset >= self.len || len == 0 {
            return;
        }
        let span = len.min(self.len.saturating_sub(offset));
        unsafe { nvme_dma_cache_invalidate(self.virt.as_ptr().add(offset), span) };
    }

    fn zero_all(&self) {
        unsafe {
            write_bytes(self.virt.as_ptr(), 0, self.len);
        }
        self.flush_range(0, self.len);
    }

    fn copy_from_slice(&self, src: &[u8]) {
        let span = src.len().min(self.len);
        unsafe {
            copy_nonoverlapping(src.as_ptr(), self.virt.as_ptr(), span);
        }
        self.flush_range(0, span);
    }
}

impl Drop for DmaBuffer {
    fn drop(&mut self) {
        if self.len != 0 {
            dma::dealloc(self.virt.as_ptr(), self.len);
        }
    }
}

struct QueuePair {
    qid: u16,
    depth: u16,
    sq_phys: u64,
    cq_phys: u64,
    sq_virt: *mut NvmeSqe,
    cq_virt: *mut NvmeCqe,
    sq_mem: DmaBuffer,
    cq_mem: DmaBuffer,
    sq_tail: u16,
    cq_head: u16,
    cq_phase: bool,
    next_cid: u16,
}

unsafe impl Send for QueuePair {}

impl QueuePair {
    fn new(qid: u16, depth: u16) -> core::result::Result<Self, block::Error> {
        if depth == 0 {
            return Err(block::Error::InvalidParam);
        }

        let sq_bytes = (depth as usize)
            .checked_mul(mem::size_of::<NvmeSqe>())
            .ok_or(block::Error::InvalidParam)?;
        let cq_bytes = (depth as usize)
            .checked_mul(mem::size_of::<NvmeCqe>())
            .ok_or(block::Error::InvalidParam)?;

        let sq_alloc = sq_bytes
            .div_ceil(PAGE_SIZE)
            .checked_mul(PAGE_SIZE)
            .ok_or(block::Error::InvalidParam)?;
        let cq_alloc = cq_bytes
            .div_ceil(PAGE_SIZE)
            .checked_mul(PAGE_SIZE)
            .ok_or(block::Error::InvalidParam)?;

        let sq_mem = DmaBuffer::alloc(sq_alloc, PAGE_SIZE)?;
        let cq_mem = DmaBuffer::alloc(cq_alloc, PAGE_SIZE)?;
        sq_mem.zero_all();
        cq_mem.zero_all();

        Ok(Self {
            qid,
            depth,
            sq_phys: sq_mem.phys(),
            cq_phys: cq_mem.phys(),
            sq_virt: sq_mem.as_ptr() as *mut NvmeSqe,
            cq_virt: cq_mem.as_ptr() as *mut NvmeCqe,
            sq_mem,
            cq_mem,
            sq_tail: 0,
            cq_head: 0,
            cq_phase: true,
            next_cid: 0,
        })
    }

    fn submit(&mut self, mut cmd: NvmeSqe) -> u16 {
        let cid = self.next_cid;
        self.next_cid = self.next_cid.wrapping_add(1);
        cmd.d[0] = (cmd.d[0] & 0x0000_FFFF) | ((cid as u32) << 16);

        let idx = self.sq_tail as usize;
        unsafe {
            write_volatile(self.sq_virt.add(idx), cmd);
        }
        self.sq_mem
            .flush_range(idx * mem::size_of::<NvmeSqe>(), mem::size_of::<NvmeSqe>());

        self.sq_tail = (self.sq_tail + 1) % self.depth;
        cid
    }

    fn poll_completion(&mut self) -> Option<NvmeCqe> {
        let idx = self.cq_head as usize;
        self.cq_mem
            .invalidate_range(idx * mem::size_of::<NvmeCqe>(), mem::size_of::<NvmeCqe>());
        let entry = unsafe { read_volatile(self.cq_virt.add(idx)) };
        if entry.phase() != self.cq_phase {
            return None;
        }

        self.cq_head += 1;
        if self.cq_head >= self.depth {
            self.cq_head = 0;
            self.cq_phase = !self.cq_phase;
        }
        Some(entry)
    }
}

struct IdentifyControllerInfo {
    serial: String,
    model: String,
    mdts: u8,
    nn: u32,
}

#[derive(Clone, Copy)]
struct NamespaceInfo {
    nsid: u32,
    block_count: u64,
    block_size: u32,
}

#[derive(Clone)]
struct NvmeIoRuntime {
    pci: block::PciAddress,
    mmio: NonNull<u8>,
    doorbell_stride_bytes: u32,
    max_transfer_bytes: u64,
}

unsafe impl Send for NvmeIoRuntime {}

impl NvmeIoRuntime {
    fn write32(&self, offset: usize, value: u32) {
        unsafe {
            write_volatile(self.mmio.as_ptr().add(offset) as *mut u32, value);
        }
    }

    fn ring_sq_doorbell(&self, qid: u16, new_tail: u16) {
        let offset = NVME_REG_DBS + (2 * qid as usize) * (self.doorbell_stride_bytes as usize);
        self.write32(offset, new_tail as u32);
    }

    fn ring_cq_doorbell(&self, qid: u16, new_head: u16) {
        let offset = NVME_REG_DBS + (2 * qid as usize + 1) * (self.doorbell_stride_bytes as usize);
        self.write32(offset, new_head as u32);
    }

    fn io_cmd_blocking(
        &self,
        queue: &mut QueuePair,
        cmd: NvmeSqe,
    ) -> core::result::Result<NvmeCqe, block::Error> {
        let opcode = (cmd.d[0] & 0xFF) as u8;
        let nsid = cmd.d[1];
        let start_lba = ((cmd.d[11] as u64) << 32) | (cmd.d[10] as u64);
        let blocks = ((cmd.d[12] & 0xFFFF) as u16).wrapping_add(1);
        let cid = queue.submit(cmd);
        self.ring_sq_doorbell(queue.qid, queue.sq_tail);

        let mut found = None;
        let completed = wait::spin_until_timeout_no_exec(IO_TIMEOUT_MS, || {
            if let Some(cqe) = queue.poll_completion() {
                found = Some(cqe);
                true
            } else {
                false
            }
        });
        if !completed {
            crate::log_trace!(
                "nvme: {} io timeout qid={} opcode=0x{:02X} nsid={} cid={} slba={} blocks={}\n",
                self.pci,
                queue.qid,
                opcode,
                nsid,
                cid,
                start_lba,
                blocks
            );
            return Err(block::Error::Timeout);
        }

        let cqe = found.ok_or(block::Error::Timeout)?;
        self.ring_cq_doorbell(queue.qid, queue.cq_head);
        if cqe.command_id() != cid {
            crate::log_trace!(
                "nvme: {} io bad-cid qid={} opcode=0x{:02X} nsid={} want={} got={} slba={} blocks={}\n",
                self.pci,
                queue.qid,
                opcode,
                nsid,
                cid,
                cqe.command_id(),
                start_lba,
                blocks
            );
            return Err(block::Error::Io);
        }
        if !cqe.is_success() {
            crate::log_trace!(
                "nvme: {} io failed qid={} opcode=0x{:02X} nsid={} cid={} slba={} blocks={} sct={} sc={} dnr={} raw=0x{:04X} {}\n",
                self.pci,
                queue.qid,
                opcode,
                nsid,
                cid,
                start_lba,
                blocks,
                cqe.status_type(),
                cqe.status_code(),
                cqe.do_not_retry(),
                cqe.status_field(),
                nvme_status_name(cqe.status_type(), cqe.status_code())
            );
            return Err(block::Error::Io);
        }
        Ok(cqe)
    }

    async fn io_cmd_async(
        &self,
        queue: &mut QueuePair,
        cmd: NvmeSqe,
    ) -> core::result::Result<NvmeCqe, block::Error> {
        let opcode = (cmd.d[0] & 0xFF) as u8;
        let nsid = cmd.d[1];
        let start_lba = ((cmd.d[11] as u64) << 32) | (cmd.d[10] as u64);
        let blocks = ((cmd.d[12] & 0xFFFF) as u16).wrapping_add(1);
        let cid = queue.submit(cmd);
        self.ring_sq_doorbell(queue.qid, queue.sq_tail);
        let deadline = Instant::now() + EmbassyDuration::from_millis(IO_TIMEOUT_MS);
        let mut hot_polls_remaining = IO_HOT_POLL_LIMIT;

        loop {
            if let Some(cqe) = queue.poll_completion() {
                self.ring_cq_doorbell(queue.qid, queue.cq_head);
                if cqe.command_id() != cid {
                    crate::log_trace!(
                        "nvme: {} io bad-cid qid={} opcode=0x{:02X} nsid={} want={} got={} slba={} blocks={}\n",
                        self.pci,
                        queue.qid,
                        opcode,
                        nsid,
                        cid,
                        cqe.command_id(),
                        start_lba,
                        blocks
                    );
                    return Err(block::Error::Io);
                }
                if !cqe.is_success() {
                    crate::log_trace!(
                        "nvme: {} io failed qid={} opcode=0x{:02X} nsid={} cid={} slba={} blocks={} sct={} sc={} dnr={} raw=0x{:04X} {}\n",
                        self.pci,
                        queue.qid,
                        opcode,
                        nsid,
                        cid,
                        start_lba,
                        blocks,
                        cqe.status_type(),
                        cqe.status_code(),
                        cqe.do_not_retry(),
                        cqe.status_field(),
                        nvme_status_name(cqe.status_type(), cqe.status_code())
                    );
                    return Err(block::Error::Io);
                }
                return Ok(cqe);
            }

            if Instant::now() >= deadline {
                crate::log_trace!(
                    "nvme: {} io timeout qid={} opcode=0x{:02X} nsid={} cid={} slba={} blocks={}\n",
                    self.pci,
                    queue.qid,
                    opcode,
                    nsid,
                    cid,
                    start_lba,
                    blocks
                );
                return Err(block::Error::Timeout);
            }

            if hot_polls_remaining > 0 {
                hot_polls_remaining -= 1;
                core::hint::spin_loop();
                continue;
            }

            Timer::after(EmbassyDuration::from_millis(IO_POLL_INTERVAL_MS)).await;
        }
    }

    fn build_prps(
        &self,
        buf: &DmaBuffer,
        len: usize,
    ) -> core::result::Result<(u64, u64, Option<DmaBuffer>), block::Error> {
        if len == 0 {
            return Err(block::Error::InvalidParam);
        }

        let prp1 = buf.phys();
        let page_count = len.div_ceil(PAGE_SIZE);
        if page_count <= 1 {
            return Ok((prp1, 0, None));
        }
        if page_count == 2 {
            return Ok((prp1, buf.phys() + PAGE_SIZE as u64, None));
        }

        let remaining_pages = page_count - 1;
        if remaining_pages > 512 {
            return Err(block::Error::InvalidParam);
        }

        let list = DmaBuffer::alloc(PAGE_SIZE, PAGE_SIZE)?;
        list.zero_all();
        let entries = list.as_ptr() as *mut u64;
        for idx in 0..remaining_pages {
            let page_phys = buf.phys() + ((idx + 1) * PAGE_SIZE) as u64;
            unsafe {
                write_volatile(entries.add(idx), page_phys);
            }
        }
        list.flush_range(0, remaining_pages * mem::size_of::<u64>());
        Ok((prp1, list.phys(), Some(list)))
    }

    fn io_rw_blocking(
        &self,
        queue: &mut QueuePair,
        opcode: u8,
        nsid: u32,
        start_lba: u64,
        blocks: u16,
        buf: &DmaBuffer,
        len: usize,
    ) -> core::result::Result<(), block::Error> {
        let (prp1, prp2, _prp_list) = self.build_prps(buf, len)?;

        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = opcode as u32;
        sqe.d[1] = nsid;
        sqe.d[6] = prp1 as u32;
        sqe.d[7] = (prp1 >> 32) as u32;
        sqe.d[8] = prp2 as u32;
        sqe.d[9] = (prp2 >> 32) as u32;
        sqe.d[10] = start_lba as u32;
        sqe.d[11] = (start_lba >> 32) as u32;
        sqe.d[12] = (blocks as u32).wrapping_sub(1) & 0xFFFF;
        let _ = self.io_cmd_blocking(queue, sqe)?;
        Ok(())
    }

    async fn io_rw_async(
        &self,
        queue: &mut QueuePair,
        opcode: u8,
        nsid: u32,
        start_lba: u64,
        blocks: u16,
        buf: &DmaBuffer,
        len: usize,
    ) -> core::result::Result<(), block::Error> {
        let (prp1, prp2, _prp_list) = self.build_prps(buf, len)?;

        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = opcode as u32;
        sqe.d[1] = nsid;
        sqe.d[6] = prp1 as u32;
        sqe.d[7] = (prp1 >> 32) as u32;
        sqe.d[8] = prp2 as u32;
        sqe.d[9] = (prp2 >> 32) as u32;
        sqe.d[10] = start_lba as u32;
        sqe.d[11] = (start_lba >> 32) as u32;
        sqe.d[12] = (blocks as u32).wrapping_sub(1) & 0xFFFF;
        let _ = self.io_cmd_async(queue, sqe).await?;
        Ok(())
    }

    async fn flush_async(
        &self,
        queue: &mut QueuePair,
        nsid: u32,
    ) -> core::result::Result<(), block::Error> {
        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = NVME_NVM_FLUSH as u32;
        sqe.d[1] = nsid;
        let _ = self.io_cmd_async(queue, sqe).await?;
        Ok(())
    }

    fn smoke_test_read_blocking(
        &self,
        queue: &mut QueuePair,
        nsid: u32,
        block_size: u32,
    ) -> core::result::Result<(), block::Error> {
        let read_len = block_size as usize;
        if read_len == 0 {
            return Err(block::Error::InvalidParam);
        }
        let dma_bytes = read_len
            .div_ceil(PAGE_SIZE)
            .checked_mul(PAGE_SIZE)
            .ok_or(block::Error::InvalidParam)?;
        let dma_buf = DmaBuffer::alloc(dma_bytes, PAGE_SIZE)?;
        dma_buf.zero_all();
        self.io_rw_blocking(queue, NVME_NVM_READ, nsid, 0, 1, &dma_buf, read_len)?;
        dma_buf.invalidate_range(0, read_len);
        crate::log_trace!(
            "nvme: {} smoke-read ok qid={} nsid={} lba=0 bytes={}\n",
            self.pci,
            queue.qid,
            nsid,
            read_len
        );
        Ok(())
    }
}

struct NvmeController {
    pci: block::PciAddress,
    mmio: NonNull<u8>,
    doorbell_stride_bytes: u32,
    ready_timeout_ms: u64,
    admin: QueuePair,
    serial: Option<String>,
    model: Option<String>,
    max_transfer_bytes: u64,
}

unsafe impl Send for NvmeController {}

impl NvmeController {
    fn reg32(&self, offset: usize) -> u32 {
        unsafe { read_volatile(self.mmio.as_ptr().add(offset) as *const u32) }
    }

    fn write32(&self, offset: usize, value: u32) {
        unsafe {
            write_volatile(self.mmio.as_ptr().add(offset) as *mut u32, value);
        }
    }

    fn write64(&self, offset: usize, value: u64) {
        self.write32(offset, value as u32);
        self.write32(offset + 4, (value >> 32) as u32);
    }

    fn runtime(&self) -> NvmeIoRuntime {
        NvmeIoRuntime {
            pci: self.pci,
            mmio: self.mmio,
            doorbell_stride_bytes: self.doorbell_stride_bytes,
            max_transfer_bytes: self.max_transfer_bytes,
        }
    }

    fn wait_ready(&self, expect_ready: bool) -> core::result::Result<(), block::Error> {
        let mut fatal = false;
        let ready = wait::spin_until_timeout_no_exec(self.ready_timeout_ms, || {
            let csts = self.reg32(NVME_REG_CSTS);
            if (csts & NVME_CSTS_CFS) != 0 {
                fatal = true;
                return true;
            }
            ((csts & NVME_CSTS_RDY) != 0) == expect_ready
        });

        if fatal {
            crate::log_trace!(
                "nvme: {} controller fatal while waiting for RDY={} timeout={}ms csts=0x{:08X}\n",
                self.pci,
                expect_ready,
                self.ready_timeout_ms,
                self.reg32(NVME_REG_CSTS)
            );
            return Err(block::Error::Io);
        }
        if !ready {
            crate::log_trace!(
                "nvme: {} ready timeout waiting for RDY={} timeout={}ms csts=0x{:08X}\n",
                self.pci,
                expect_ready,
                self.ready_timeout_ms,
                self.reg32(NVME_REG_CSTS)
            );
            return Err(block::Error::Timeout);
        }
        Ok(())
    }

    fn admin_cmd(&mut self, cmd: NvmeSqe) -> core::result::Result<NvmeCqe, block::Error> {
        let opcode = (cmd.d[0] & 0xFF) as u8;
        let cid = self.admin.submit(cmd);
        self.runtime()
            .ring_sq_doorbell(self.admin.qid, self.admin.sq_tail);

        let mut found = None;
        let completed = wait::spin_until_timeout_no_exec(ADMIN_TIMEOUT_MS, || {
            if let Some(cqe) = self.admin.poll_completion() {
                found = Some(cqe);
                true
            } else {
                false
            }
        });
        if !completed {
            crate::log_trace!(
                "nvme: {} admin timeout opcode=0x{:02X} cid={}\n",
                self.pci,
                opcode,
                cid
            );
            return Err(block::Error::Timeout);
        }

        let cqe = found.ok_or(block::Error::Timeout)?;
        self.runtime()
            .ring_cq_doorbell(self.admin.qid, self.admin.cq_head);
        if cqe.command_id() != cid {
            crate::log_trace!(
                "nvme: {} admin bad-cid opcode=0x{:02X} want={} got={}\n",
                self.pci,
                opcode,
                cid,
                cqe.command_id()
            );
            return Err(block::Error::Io);
        }
        if !cqe.is_success() {
            crate::log_trace!(
                "nvme: {} admin failed opcode=0x{:02X} cid={} sct={} sc={} dnr={} raw=0x{:04X} {}\n",
                self.pci,
                opcode,
                cid,
                cqe.status_type(),
                cqe.status_code(),
                cqe.do_not_retry(),
                cqe.status_field(),
                nvme_status_name(cqe.status_type(), cqe.status_code())
            );
            return Err(block::Error::Io);
        }
        Ok(cqe)
    }

    fn identify_controller(
        &mut self,
    ) -> core::result::Result<IdentifyControllerInfo, block::Error> {
        let buf = DmaBuffer::alloc(PAGE_SIZE, PAGE_SIZE)?;
        buf.zero_all();

        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = NVME_ADMIN_IDENTIFY as u32;
        sqe.d[6] = buf.phys() as u32;
        sqe.d[7] = (buf.phys() >> 32) as u32;
        sqe.d[10] = NVME_IDENTIFY_CONTROLLER;
        self.admin_cmd(sqe)?;

        let ptr = buf.as_ptr();
        buf.invalidate_range(0, PAGE_SIZE);

        let serial = unsafe {
            let bytes = core::slice::from_raw_parts(ptr.add(4), 20);
            String::from(
                core::str::from_utf8(bytes)
                    .unwrap_or("")
                    .trim_matches(char::from(0))
                    .trim(),
            )
        };
        let model = unsafe {
            let bytes = core::slice::from_raw_parts(ptr.add(24), 40);
            String::from(
                core::str::from_utf8(bytes)
                    .unwrap_or("")
                    .trim_matches(char::from(0))
                    .trim(),
            )
        };
        let mdts = unsafe { read_volatile(ptr.add(77) as *const u8) };
        let nn = unsafe { read_unaligned(ptr.add(516) as *const u32) };

        Ok(IdentifyControllerInfo {
            serial,
            model,
            mdts,
            nn,
        })
    }

    fn identify_namespace_by_id(
        &mut self,
        nsid: u32,
    ) -> core::result::Result<NamespaceInfo, block::Error> {
        let buf = DmaBuffer::alloc(PAGE_SIZE, PAGE_SIZE)?;
        buf.zero_all();

        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = NVME_ADMIN_IDENTIFY as u32;
        sqe.d[1] = nsid;
        sqe.d[6] = buf.phys() as u32;
        sqe.d[7] = (buf.phys() >> 32) as u32;
        sqe.d[10] = NVME_IDENTIFY_NAMESPACE;
        self.admin_cmd(sqe)?;

        let ptr = buf.as_ptr();
        buf.invalidate_range(0, PAGE_SIZE);

        let block_count = unsafe { read_unaligned(ptr as *const u64) };
        let flbas = unsafe { read_volatile(ptr.add(26) as *const u8) & 0x0F };
        let lbaf_offset = 128usize
            .checked_add(
                (flbas as usize)
                    .checked_mul(4)
                    .ok_or(block::Error::Corrupted)?,
            )
            .ok_or(block::Error::Corrupted)?;
        let lbaf = unsafe { read_unaligned(ptr.add(lbaf_offset) as *const u32) };
        let lbads = ((lbaf >> 16) & 0xFF) as u32;
        let block_size = 1u32.checked_shl(lbads).ok_or(block::Error::Corrupted)?;

        Ok(NamespaceInfo {
            nsid,
            block_count,
            block_size,
        })
    }

    fn active_namespace_ids(&mut self, nn: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let Ok(buf) = DmaBuffer::alloc(PAGE_SIZE, PAGE_SIZE) else {
            out.push(1);
            return out;
        };
        buf.zero_all();

        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = NVME_ADMIN_IDENTIFY as u32;
        sqe.d[6] = buf.phys() as u32;
        sqe.d[7] = (buf.phys() >> 32) as u32;
        sqe.d[10] = NVME_IDENTIFY_ACTIVE_NSID_LIST;

        if self.admin_cmd(sqe).is_err() {
            out.push(1);
            return out;
        }

        let ptr = buf.as_ptr();
        buf.invalidate_range(0, PAGE_SIZE);
        let limit = core::cmp::min(nn.max(1), 1024) as usize;
        for idx in 0..limit {
            let nsid = unsafe { read_volatile((ptr as *const u32).add(idx)) };
            if nsid == 0 {
                break;
            }
            out.push(nsid);
        }
        if out.is_empty() {
            out.push(1);
        }
        out
    }

    fn set_number_of_queues(
        &mut self,
        requested_queues: u16,
    ) -> core::result::Result<u16, block::Error> {
        let requested = requested_queues.max(1);
        let requested_zero_based = requested.saturating_sub(1) as u32;
        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = NVME_ADMIN_SET_FEATURES as u32;
        sqe.d[10] = NVME_FEAT_NUMBER_OF_QUEUES;
        sqe.d[11] = requested_zero_based | (requested_zero_based << 16);
        let cqe = self.admin_cmd(sqe)?;
        let supported_sq = (cqe.dw0 & 0xFFFF) as u16;
        let supported_cq = ((cqe.dw0 >> 16) & 0xFFFF) as u16;
        Ok(supported_sq
            .saturating_add(1)
            .min(supported_cq.saturating_add(1))
            .max(1))
    }

    fn create_io_cq(
        &mut self,
        qid: u16,
        cq_phys: u64,
        depth: u16,
    ) -> core::result::Result<(), block::Error> {
        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = NVME_ADMIN_CREATE_IO_CQ as u32;
        sqe.d[6] = cq_phys as u32;
        sqe.d[7] = (cq_phys >> 32) as u32;
        sqe.d[10] = (qid as u32) | (((depth - 1) as u32) << 16);
        sqe.d[11] = 1;
        let _ = self.admin_cmd(sqe)?;
        Ok(())
    }

    fn create_io_sq(
        &mut self,
        qid: u16,
        sq_phys: u64,
        cqid: u16,
        depth: u16,
    ) -> core::result::Result<(), block::Error> {
        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = NVME_ADMIN_CREATE_IO_SQ as u32;
        sqe.d[6] = sq_phys as u32;
        sqe.d[7] = (sq_phys >> 32) as u32;
        sqe.d[10] = (qid as u32) | (((depth - 1) as u32) << 16);
        sqe.d[11] = 1 | ((cqid as u32) << 16);
        let _ = self.admin_cmd(sqe)?;
        Ok(())
    }

    fn collect_namespaces(
        &mut self,
        nsids: Vec<u32>,
    ) -> core::result::Result<Vec<NamespaceInfo>, block::Error> {
        let mut out = Vec::new();
        for nsid in nsids {
            match self.identify_namespace_by_id(nsid) {
                Ok(info) if info.block_count > 0 && info.block_size != 0 => out.push(info),
                Ok(_) => {}
                Err(err) => {
                    crate::log_trace!(
                        "nvme: {} namespace identify failed nsid={} err={:?}\n",
                        self.pci,
                        nsid,
                        err
                    );
                }
            }
        }
        if out.is_empty() {
            return Err(block::Error::NotReady);
        }
        Ok(out)
    }

    fn init(
        mmio: NonNull<u8>,
        pci: block::PciAddress,
    ) -> core::result::Result<(Self, Vec<NamespaceInfo>, u16), block::Error> {
        let cap = unsafe { read_volatile(mmio.as_ptr().add(NVME_REG_CAP) as *const u64) };
        let vs = unsafe { read_volatile(mmio.as_ptr().add(NVME_REG_VS) as *const u32) };
        let mqes = ((cap & 0xFFFF) as u16).saturating_add(1).max(2);
        let cap_to_units = ((cap >> 24) & 0xFF) as u32;
        let dstrd = ((cap >> 32) & 0xF) as u32;
        let mpsmin = ((cap >> 48) & 0xF) as u32;
        let version_major = (vs >> 16) & 0xFFFF;
        let version_minor = (vs >> 8) & 0xFF;
        let version_tertiary = vs & 0xFF;
        let reported_ready_timeout_ms =
            (cap_to_units as u64).saturating_mul(NVME_CAP_TO_GRANULARITY_MS);
        let ready_timeout_ms = if reported_ready_timeout_ms == 0 {
            READY_TIMEOUT_MS
        } else {
            READY_TIMEOUT_MS.max(reported_ready_timeout_ms)
        };
        crate::log_trace!(
            "nvme: {} caps ver={}.{}.{} mqes={} dstrd={} mpsmin={} cap.to={}ms ready_timeout={}ms\n",
            pci,
            version_major,
            version_minor,
            version_tertiary,
            mqes,
            dstrd,
            mpsmin,
            reported_ready_timeout_ms,
            ready_timeout_ms
        );
        if mpsmin > 0 {
            crate::log_trace!(
                "nvme: {} unsupported CAP.MPSMIN={} (requires page size > 4KiB)\n",
                pci,
                mpsmin
            );
            return Err(block::Error::NotSupported);
        }

        let queue_depth = mqes.min(QUEUE_DEPTH_CAP).max(2);
        let admin = QueuePair::new(0, queue_depth)?;
        let doorbell_stride_bytes = 4u32 << dstrd;

        let ctrl = Self {
            pci,
            mmio,
            doorbell_stride_bytes,
            ready_timeout_ms,
            admin,
            serial: None,
            model: None,
            max_transfer_bytes: IO_TRANSFER_PAGES_CAP * PAGE_SIZE as u64,
        };

        if (ctrl.reg32(NVME_REG_CC) & NVME_CC_EN) != 0 {
            ctrl.write32(NVME_REG_CC, ctrl.reg32(NVME_REG_CC) & !NVME_CC_EN);
            ctrl.wait_ready(false)?;
        }

        let aqa = ((queue_depth - 1) as u32) | (((queue_depth - 1) as u32) << 16);
        ctrl.write32(NVME_REG_AQA, aqa);
        ctrl.write64(NVME_REG_ASQ, ctrl.admin.sq_phys);
        ctrl.write64(NVME_REG_ACQ, ctrl.admin.cq_phys);
        ctrl.write32(NVME_REG_INTMS, 0xFFFF_FFFF);
        ctrl.write32(
            NVME_REG_CC,
            NVME_CC_EN
                | NVME_CC_CSS_NVM
                | NVME_CC_MPS_4K
                | NVME_CC_AMS_RR
                | NVME_CC_IOSQES
                | NVME_CC_IOCQES,
        );
        ctrl.wait_ready(true)?;

        let mut ctrl = ctrl;
        let ctrl_info = ctrl.identify_controller()?;
        let nsids = ctrl.active_namespace_ids(ctrl_info.nn);
        ctrl.serial = if ctrl_info.serial.is_empty() {
            None
        } else {
            Some(ctrl_info.serial)
        };
        ctrl.model = if ctrl_info.model.is_empty() {
            None
        } else {
            Some(ctrl_info.model)
        };

        let mdts_pages = if ctrl_info.mdts == 0 {
            256
        } else {
            1u64 << (ctrl_info.mdts as u64)
        };
        ctrl.max_transfer_bytes = (mdts_pages * PAGE_SIZE as u64)
            .min(IO_TRANSFER_PAGES_CAP * PAGE_SIZE as u64)
            .max(PAGE_SIZE as u64);

        let namespaces = ctrl.collect_namespaces(nsids)?;
        Ok((ctrl, namespaces, queue_depth))
    }
}

struct NvmeWorkerBackend {
    runtime: NvmeIoRuntime,
    io: QueuePair,
    nsid: u32,
    block_size: u32,
    block_count: u64,
    max_transfer_bytes: u64,
}

unsafe impl Send for NvmeWorkerBackend {}

impl NvmeWorkerBackend {
    async fn do_read_blocks(&mut self, lba: u64, blocks: usize) -> block::Result<Vec<u8>> {
        let bs = self.block_size as usize;
        if bs == 0 {
            return Err(block::Error::InvalidParam);
        }
        if blocks == 0 {
            return Ok(Vec::new());
        }

        let total_bytes = blocks.checked_mul(bs).ok_or(block::Error::InvalidParam)?;
        let mut out = vec![0u8; total_bytes];
        let max_io_bytes = self.max_transfer_bytes.max(bs as u64) as usize;
        let max_blocks = core::cmp::max(1, core::cmp::min(max_io_bytes / bs, u16::MAX as usize));

        let mut cur_lba = lba;
        let mut offset = 0usize;
        let mut remaining = blocks;
        while remaining > 0 {
            let blocks_here = remaining.min(max_blocks);
            let bytes_here = blocks_here * bs;
            let dma_bytes = bytes_here
                .div_ceil(PAGE_SIZE)
                .checked_mul(PAGE_SIZE)
                .ok_or(block::Error::InvalidParam)?;
            let dma_buf = DmaBuffer::alloc(dma_bytes, PAGE_SIZE)?;
            dma_buf.zero_all();
            self.runtime
                .io_rw_async(
                    &mut self.io,
                    NVME_NVM_READ,
                    self.nsid,
                    cur_lba,
                    blocks_here as u16,
                    &dma_buf,
                    bytes_here,
                )
                .await?;
            dma_buf.invalidate_range(0, bytes_here);
            unsafe {
                copy_nonoverlapping(
                    dma_buf.as_ptr(),
                    out[offset..offset + bytes_here].as_mut_ptr(),
                    bytes_here,
                );
            }

            cur_lba = cur_lba.saturating_add(blocks_here as u64);
            offset = offset.saturating_add(bytes_here);
            remaining = remaining.saturating_sub(blocks_here);
            if remaining > 0 {
                Timer::after(EmbassyDuration::from_micros(1)).await;
            }
        }

        Ok(out)
    }

    async fn do_write_blocks(&mut self, lba: u64, buf: &[u8]) -> block::Result<()> {
        let bs = self.block_size as usize;
        if bs == 0 || !buf.len().is_multiple_of(bs) {
            return Err(block::Error::InvalidParam);
        }

        let blocks_total = buf.len() / bs;
        if blocks_total == 0 {
            return Ok(());
        }

        let max_io_bytes = self.max_transfer_bytes.max(bs as u64) as usize;
        let max_blocks = core::cmp::max(1, core::cmp::min(max_io_bytes / bs, u16::MAX as usize));

        let mut cur_lba = lba;
        let mut offset = 0usize;
        let mut remaining_blocks = blocks_total;
        while remaining_blocks > 0 {
            let blocks_here = remaining_blocks.min(max_blocks);
            let bytes_here = blocks_here * bs;
            let dma_bytes = bytes_here
                .div_ceil(PAGE_SIZE)
                .checked_mul(PAGE_SIZE)
                .ok_or(block::Error::InvalidParam)?;
            let dma_buf = DmaBuffer::alloc(dma_bytes, PAGE_SIZE)?;
            dma_buf.zero_all();
            dma_buf.copy_from_slice(&buf[offset..offset + bytes_here]);
            self.runtime
                .io_rw_async(
                    &mut self.io,
                    NVME_NVM_WRITE,
                    self.nsid,
                    cur_lba,
                    blocks_here as u16,
                    &dma_buf,
                    bytes_here,
                )
                .await?;

            cur_lba = cur_lba.saturating_add(blocks_here as u64);
            offset = offset.saturating_add(bytes_here);
            remaining_blocks = remaining_blocks.saturating_sub(blocks_here);
            if remaining_blocks > 0 {
                Timer::after(EmbassyDuration::from_micros(1)).await;
            }
        }

        Ok(())
    }

    async fn do_flush(&mut self) -> block::Result<()> {
        self.runtime.flush_async(&mut self.io, self.nsid).await
    }
}

impl block::BlockDevice for NvmeWorkerBackend {
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
    ) -> block::BoxFuture<'a, block::Result<Vec<u8>>> {
        Box::pin(async move {
            let bs = self.block_size as usize;
            if bs == 0 {
                return Err(block::Error::InvalidParam);
            }
            if blocks == 0 {
                return Ok(Vec::new());
            }

            let blocks_total = blocks as u64;
            let end = lba
                .checked_add(blocks_total)
                .ok_or(block::Error::OutOfBounds)?;
            if end > self.block_count {
                return Err(block::Error::OutOfBounds);
            }

            self.do_read_blocks(lba, blocks).await
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

            self.do_write_blocks(lba, buf).await
        })
    }

    fn dma_alignment_bytes(&self) -> u32 {
        PAGE_SIZE as u32
    }

    fn max_transfer_bytes(&self) -> u64 {
        self.max_transfer_bytes
    }

    fn supports_write(&self) -> bool {
        true
    }

    fn flush<'a>(&'a mut self) -> block::BoxFuture<'a, block::Result<()>> {
        Box::pin(async move { self.do_flush().await })
    }
}

pub(crate) fn register_mapped_controller(mmio: NonNull<u8>, pci: block::PciAddress) -> bool {
    let (mut ctrl, namespaces, queue_depth) = match NvmeController::init(mmio, pci) {
        Ok(v) => v,
        Err(e) => {
            crate::log_trace!("nvme: {} init failed: {:?}\n", pci, e);
            return false;
        }
    };

    let total_namespaces = namespaces.len();
    let requested_queues = total_namespaces.min(u16::MAX as usize) as u16;
    let available_queues = match ctrl.set_number_of_queues(requested_queues.max(1)) {
        Ok(supported) => supported.min(requested_queues.max(1)),
        Err(err) => {
            crate::log_trace!(
                "nvme: {} set-number-of-queues failed err={:?}; falling back to one I/O queue\n",
                pci,
                err
            );
            1
        }
    };
    if total_namespaces > available_queues as usize {
        crate::log_trace!(
            "nvme: {} limiting namespace registration to {} queue-backed namespace(s) out of {}\n",
            pci,
            available_queues,
            total_namespaces
        );
    }

    let runtime = ctrl.runtime();
    let serial = ctrl.serial.clone();
    let max_transfer_bytes = runtime.max_transfer_bytes;
    let mut registered_any = false;

    for (idx, ns) in namespaces
        .into_iter()
        .take(available_queues as usize)
        .enumerate()
    {
        let qid = IO_QID.saturating_add(idx as u16);
        let mut io_queue = match QueuePair::new(qid, queue_depth) {
            Ok(queue) => queue,
            Err(err) => {
                crate::log_trace!(
                    "nvme: {} queue alloc failed qid={} nsid={} err={:?}\n",
                    pci,
                    qid,
                    ns.nsid,
                    err
                );
                continue;
            }
        };

        if let Err(err) = ctrl.create_io_cq(qid, io_queue.cq_phys, queue_depth) {
            crate::log_trace!(
                "nvme: {} create-io-cq failed qid={} nsid={} err={:?}\n",
                pci,
                qid,
                ns.nsid,
                err
            );
            continue;
        }
        if let Err(err) = ctrl.create_io_sq(qid, io_queue.sq_phys, qid, queue_depth) {
            crate::log_trace!(
                "nvme: {} create-io-sq failed qid={} nsid={} err={:?}\n",
                pci,
                qid,
                ns.nsid,
                err
            );
            continue;
        }
        if let Err(err) = runtime.smoke_test_read_blocking(&mut io_queue, ns.nsid, ns.block_size) {
            crate::log_trace!(
                "nvme: {} smoke-read failed qid={} nsid={} err={:?}\n",
                pci,
                qid,
                ns.nsid,
                err
            );
            continue;
        }

        let label = match serial.as_deref() {
            Some(drive_serial) if !drive_serial.is_empty() && total_namespaces > 1 => {
                alloc::format!("nvme:{}:ns{}", drive_serial, ns.nsid)
            }
            Some(drive_serial) if !drive_serial.is_empty() => {
                alloc::format!("nvme:{}", drive_serial)
            }
            _ if total_namespaces > 1 => alloc::format!("nvme:ns{}", ns.nsid),
            _ => String::from("nvme"),
        };

        let mut desc = block::DeviceDescriptor::new(block::DeviceKind::Nvme)
            .with_label(label)
            .with_pci(pci);
        if let Some(drive_serial) = serial.clone() {
            desc = desc.with_serial(drive_serial);
        }

        let dev = NvmeWorkerBackend {
            runtime: runtime.clone(),
            io: io_queue,
            nsid: ns.nsid,
            block_size: ns.block_size,
            block_count: ns.block_count,
            max_transfer_bytes,
        };
        let handle = block::register_device_with_worker(desc, dev);
        crate::log_trace!(
            "nvme: registered {} nsid={} qid={} id={} blocks={} bs={} max_io={}\n",
            pci,
            ns.nsid,
            qid,
            handle.id().raw(),
            ns.block_count,
            ns.block_size,
            max_transfer_bytes
        );
        registered_any = true;
    }

    registered_any
}
