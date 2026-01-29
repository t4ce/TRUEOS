use alloc::string::String;
use core::{
    hint::spin_loop,
    mem,
    ptr::{read_volatile, write_bytes, write_volatile, NonNull},
    sync::atomic::{fence, Ordering},
};

use crate::{
    disc::block,
    pci::{dma, mmio},
};

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

const NVME_NVM_FLUSH: u8 = 0x00;
const NVME_NVM_WRITE: u8 = 0x01;
const NVME_NVM_READ: u8 = 0x02;

const PAGE_SIZE: usize = 4096;

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
    sq_head: u16,
    sq_id: u16,
    status: u16,
}

impl Completion {
    fn phase(self) -> bool {
        (self.status & 0x1) != 0
    }

    fn status_code(self) -> u8 {
        ((self.status >> 1) & 0xFF) as u8
    }

    fn status_type(self) -> u8 {
        ((self.status >> 9) & 0x7) as u8
    }
}

struct NvmeQueue {
    qid: u16,
    depth: u16,
    sq_phys: u64,
    sq_virt: *mut NvmeSqe,
    cq_phys: u64,
    cq_virt: *mut NvmeCqe,
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

    fn new(qid: u16, depth: u16, page_size_bytes: usize) -> core::result::Result<Self, block::Error> {
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

        let align = core::cmp::max(PAGE_SIZE, page_size_bytes);
        let (sq_phys, sq_virt_u8) = dma::alloc(sq_bytes, align).ok_or(block::Error::DmaUnavailable)?;
        let (cq_phys, cq_virt_u8) = dma::alloc(cq_bytes, align).ok_or(block::Error::DmaUnavailable)?;
        unsafe {
            write_bytes(sq_virt_u8, 0, sq_bytes);
            write_bytes(cq_virt_u8, 0, cq_bytes);
        }

        Ok(Self {
            qid,
            depth,
            sq_phys,
            sq_virt: sq_virt_u8 as *mut NvmeSqe,
            cq_phys,
            cq_virt: cq_virt_u8 as *mut NvmeCqe,
            sq_tail: 0,
            cq_head: 0,
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
        if (next as usize) % (self.depth as usize) == 0 {
            self.cq_phase = !self.cq_phase;
        }
        self.cq_head = next;
    }
}

struct NvmeController {
    mmio: NonNull<u8>,
    doorbell_stride_bytes: u32,
    page_size_bytes: usize,
    admin: NvmeQueue,
    io: NvmeQueue,
    next_cid: u16,
    pci: block::PciAddress,
    serial: Option<String>,
}

unsafe impl Send for NvmeController {}
unsafe impl Sync for NvmeController {}

impl NvmeController {
    fn page_size_bytes(&self) -> usize {
        core::cmp::max(PAGE_SIZE, self.page_size_bytes)
    }

    fn reg32(&self, off: usize) -> u32 {
        unsafe { read_volatile(self.mmio.as_ptr().add(off) as *const u32) }
    }

    fn reg64(&self, off: usize) -> u64 {
        unsafe { read_volatile(self.mmio.as_ptr().add(off) as *const u64) }
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
        fence(Ordering::Release);
        self.write32(NVME_REG_DBS + idx, tail as u32);
    }

    fn db_write_cq_head(&self, qid: u16, head: u16) {
        let stride = self.doorbell_stride_bytes as usize;
        let idx = (2usize * (qid as usize) + 1) * stride;
        fence(Ordering::Release);
        self.write32(NVME_REG_DBS + idx, head as u32);
    }

    fn spin_wait_ready(&self, want_ready: bool, timeout_ms: u64) -> core::result::Result<(), block::Error> {
        let hz = embassy_time_driver::TICK_HZ as u64;
        let start = embassy_time_driver::now();
        let ticks = if hz == 0 {
            0
        } else {
            ((timeout_ms.saturating_mul(hz) + 999) / 1000).max(1)
        };
        let deadline = start.saturating_add(ticks);
        loop {
            let csts = self.reg32(NVME_REG_CSTS);
            let rdy = (csts & 0x1) != 0;
            if rdy == want_ready {
                return Ok(());
            }
            if embassy_time_driver::now() >= deadline {
                return Err(block::Error::Timeout);
            }
            crate::time::poll_executor();
            spin_loop();
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

    fn admin_submit_and_wait(
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
        self.db_write_sq_tail(0, tail % depth);
        self.admin_poll_cq_for_cid(cid, timeout_ms)
    }

    fn io_submit_and_wait(
        &mut self,
        sqe: NvmeSqe,
        cid: u16,
        timeout_ms: u64,
    ) -> core::result::Result<Completion, block::Error> {
        let (tail, depth) = {
            let q = &mut self.io;
            let _ = q.sq_push(sqe)?;
            (q.sq_tail, q.depth)
        };
        self.db_write_sq_tail(1, tail % depth);
        self.io_poll_cq_for_cid(cid, timeout_ms)
    }

    fn admin_poll_cq_for_cid(
        &mut self,
        cid: u16,
        timeout_ms: u64,
    ) -> core::result::Result<Completion, block::Error> {
        self.poll_queue_cq_for_cid(0, cid, timeout_ms)
    }

    fn io_poll_cq_for_cid(
        &mut self,
        cid: u16,
        timeout_ms: u64,
    ) -> core::result::Result<Completion, block::Error> {
        self.poll_queue_cq_for_cid(1, cid, timeout_ms)
    }

    fn poll_queue_cq_for_cid(
        &mut self,
        qid: u16,
        cid: u16,
        timeout_ms: u64,
    ) -> core::result::Result<Completion, block::Error> {
        let hz = embassy_time_driver::TICK_HZ as u64;
        let start = embassy_time_driver::now();
        let ticks = if hz == 0 {
            0
        } else {
            ((timeout_ms.saturating_mul(hz) + 999) / 1000).max(1)
        };
        let deadline = start.saturating_add(ticks);

        loop {
            let (maybe_cpl, new_head, depth) = if qid == 0 {
                let q = &mut self.admin;
                let cqe = q.cq_peek();
                let status = (cqe.dw3 >> 16) as u16;
                let phase = (status & 0x1) != 0;
                if phase == q.cq_phase {
                    fence(Ordering::Acquire);
                    let cqe = q.cq_peek();
                    let got_cid = (cqe.dw3 & 0xFFFF) as u16;
                    let sq_head = (cqe.dw2 & 0xFFFF) as u16;
                    let sq_id = (cqe.dw2 >> 16) as u16;
                    q.cq_pop();
                    (
                        Some(Completion {
                            cid: got_cid,
                            sq_head,
                            sq_id,
                            status,
                        }),
                        q.cq_head,
                        q.depth,
                    )
                } else {
                    (None, q.cq_head, q.depth)
                }
            } else {
                let q = &mut self.io;
                let cqe = q.cq_peek();
                let status = (cqe.dw3 >> 16) as u16;
                let phase = (status & 0x1) != 0;
                if phase == q.cq_phase {
                    fence(Ordering::Acquire);
                    let cqe = q.cq_peek();
                    let got_cid = (cqe.dw3 & 0xFFFF) as u16;
                    let sq_head = (cqe.dw2 & 0xFFFF) as u16;
                    let sq_id = (cqe.dw2 >> 16) as u16;
                    q.cq_pop();
                    (
                        Some(Completion {
                            cid: got_cid,
                            sq_head,
                            sq_id,
                            status,
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
                    return Ok(cpl);
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
                // Drain and keep waiting for the desired CID.
            }

            if embassy_time_driver::now() >= deadline {
                return Err(block::Error::Timeout);
            }
            crate::time::poll_executor();
            spin_loop();
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

    fn init(mmio: NonNull<u8>, pci: block::PciAddress) -> core::result::Result<Self, block::Error> {
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
        let page_size_bytes = PAGE_SIZE
            .checked_shl(mps)
            .unwrap_or(PAGE_SIZE);

        let mut ctrl = Self {
            mmio,
            doorbell_stride_bytes,
            page_size_bytes,
            admin: NvmeQueue::new(0, 64, page_size_bytes)?,
            io: NvmeQueue::new(1, 64, page_size_bytes)?,
            next_cid: 1,
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

        // Create IO completion queue (qid=1) and submission queue (qid=1).
        ctrl.admin_create_io_cq(1, ctrl.io.depth, ctrl.io.cq_phys)?;
        ctrl.admin_create_io_sq(1, ctrl.io.depth, ctrl.io.sq_phys, 1)?;

        // Identify controller once to grab a serial string (optional).
        if let Ok(serial) = ctrl.identify_controller_serial() {
            if !serial.is_empty() {
                ctrl.serial = Some(serial);
            }
        }

        Ok(ctrl)
    }

    fn admin_create_io_cq(&mut self, qid: u16, depth: u16, cq_phys: u64) -> core::result::Result<(), block::Error> {
        let cid = self.alloc_cid();
        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = (NVME_ADMIN_CREATE_IO_CQ as u32) | ((cid as u32) << 16);
        sqe.d[4] = (cq_phys & 0xFFFF_FFFF) as u32;
        sqe.d[5] = (cq_phys >> 32) as u32;
        sqe.d[10] = (qid as u32) | (((depth as u32) - 1) << 16);
        // PC=1 (physically contiguous), IEN=0.
        sqe.d[11] = 1;
        let cpl = self.admin_submit_and_wait(sqe, cid, 1000)?;
        if cpl.status_code() != 0 {
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
        let cid = self.alloc_cid();
        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = (NVME_ADMIN_CREATE_IO_SQ as u32) | ((cid as u32) << 16);
        sqe.d[4] = (sq_phys & 0xFFFF_FFFF) as u32;
        sqe.d[5] = (sq_phys >> 32) as u32;
        sqe.d[10] = (qid as u32) | (((depth as u32) - 1) << 16);
        // PC=1 (physically contiguous), QPRIO=0, CQID in bits31:16.
        sqe.d[11] = 1 | ((cqid as u32) << 16);
        let cpl = self.admin_submit_and_wait(sqe, cid, 1000)?;
        if cpl.status_code() != 0 {
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

    fn make_prps(&self, buf_phys: u64, buf_len: usize) -> core::result::Result<(u64, u64, Option<(u64, *mut u8, usize)>), block::Error> {
        if buf_len == 0 {
            return Ok((0, 0, None));
        }

        let page_size = self.page_size_bytes();

        let first_page = buf_phys & !(page_size as u64 - 1);
        let first_off = (buf_phys - first_page) as usize;
        let span = first_off.saturating_add(buf_len);
        let pages = (span + page_size - 1) / page_size;

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
        let (list_phys, list_virt) = dma::alloc(list_bytes, page_size).ok_or(block::Error::DmaUnavailable)?;

        unsafe {
            let list = core::slice::from_raw_parts_mut(list_virt as *mut u64, list_entries);
            for i in 0..list_entries {
                list[i] = first_page + ((i + 1) * page_size) as u64;
            }
        }

        Ok((prp1, list_phys, Some((list_phys, list_virt, list_bytes))))
    }

    fn admin_identify(&mut self, nsid: u32, cns: u32, out: &mut [u8]) -> core::result::Result<(), block::Error> {
        let page_size = self.page_size_bytes();
        if out.len() < page_size {
            return Err(block::Error::InvalidParam);
        }
        let (buf_phys, buf_virt) = dma::alloc(page_size, page_size).ok_or(block::Error::DmaUnavailable)?;
        unsafe {
            write_bytes(buf_virt, 0, page_size);
        }

        let (prp1, prp2, prp_list) = self.make_prps(buf_phys, page_size)?;

        let cid = self.alloc_cid();
        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = (NVME_ADMIN_IDENTIFY as u32) | ((cid as u32) << 16);
        sqe.d[1] = nsid;
        sqe.d[6] = (prp1 & 0xFFFF_FFFF) as u32;
        sqe.d[7] = (prp1 >> 32) as u32;
        sqe.d[8] = (prp2 & 0xFFFF_FFFF) as u32;
        sqe.d[9] = (prp2 >> 32) as u32;
        sqe.d[10] = cns;

        let cpl = self.admin_submit_and_wait(sqe, cid, 1000)?;
        let status_ok = cpl.status_code() == 0;

        if let Some((_lp, lv, lb)) = prp_list {
            dma::dealloc(lv, lb);
        }

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
            dma::dealloc(buf_virt, page_size);
            return Err(block::Error::Io);
        }

        unsafe {
            out[..page_size].copy_from_slice(core::slice::from_raw_parts(buf_virt, page_size));
        }
        dma::dealloc(buf_virt, page_size);
        Ok(())
    }

    fn identify_controller_serial(&mut self) -> core::result::Result<String, block::Error> {
        let mut buf = [0u8; PAGE_SIZE];
        self.admin_identify(0, 1, &mut buf)?;
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
        Ok(s)
    }

    fn identify_namespace(&mut self, nsid: u32) -> core::result::Result<(u64, u32), block::Error> {
        let mut buf = [0u8; PAGE_SIZE];
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
        let block_size = 1u32.checked_shl(lbads as u32).ok_or(block::Error::Corrupted)?;
        Ok((nsze, block_size))
    }

    fn io_rw(
        &mut self,
        opcode: u8,
        nsid: u32,
        slba: u64,
        nlb: u16,
        buf_phys: u64,
        buf_len: usize,
    ) -> core::result::Result<(), block::Error> {
        let (prp1, prp2, prp_list) = self.make_prps(buf_phys, buf_len)?;
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

        let cpl = self.io_submit_and_wait(sqe, cid, 2000)?;

        if let Some((_lp, lv, lb)) = prp_list {
            dma::dealloc(lv, lb);
        }

        if cpl.status_code() != 0 {
            crate::log!(
                "nvme: {} io failed opcode=0x{:02X} nsid={} slba={} nlb={} status=0x{:04X} (sct={} sc={})\n",
                self.pci,
                opcode,
                nsid,
                slba,
                nlb,
                cpl.status_type(),
                cpl.status_code(),
                cpl.status
            );
            return Err(block::Error::Io);
        }
        Ok(())
    }

    fn io_flush(&mut self, nsid: u32) -> core::result::Result<(), block::Error> {
        let cid = self.alloc_cid();
        let mut sqe = NvmeSqe { d: [0; 16] };
        sqe.d[0] = (NVME_NVM_FLUSH as u32) | ((cid as u32) << 16);
        sqe.d[1] = nsid;
        let cpl = self.io_submit_and_wait(sqe, cid, 2000)?;
        if cpl.status_code() != 0 {
            return Err(block::Error::Io);
        }
        Ok(())
    }
}

struct NvmeBlockDevice {
    ctrl: NvmeController,
    nsid: u32,
    block_size: u32,
    block_count: u64,
}

unsafe impl Send for NvmeBlockDevice {}

impl block::BlockDevice for NvmeBlockDevice {
    fn block_size_bytes(&self) -> u32 {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn read_blocks(&mut self, lba: u64, buf: &mut [u8]) -> block::Result<()> {
        let bs = self.block_size as usize;
        if bs == 0 || (buf.len() % bs) != 0 {
            return Err(block::Error::InvalidParam);
        }
        let blocks_total = (buf.len() / bs) as u64;
        if blocks_total == 0 {
            return Ok(());
        }
        let end = lba.checked_add(blocks_total).ok_or(block::Error::OutOfBounds)?;
        if end > self.block_count {
            return Err(block::Error::OutOfBounds);
        }

        const MAX_IO_BYTES: usize = 256 * 1024;
        let max_blocks = core::cmp::max(1, MAX_IO_BYTES / bs);

        let mut remaining = buf;
        let mut cur_lba = lba;
        while !remaining.is_empty() {
            let blocks_here = core::cmp::min(max_blocks, remaining.len() / bs);
            let bytes_here = blocks_here * bs;
            let (dma_phys, dma_virt) = dma::alloc(bytes_here, self.ctrl.page_size_bytes()).ok_or(block::Error::DmaUnavailable)?;
            unsafe { write_bytes(dma_virt, 0, bytes_here) };

            let ok = self
                .ctrl
                .io_rw(NVME_NVM_READ, self.nsid, cur_lba, blocks_here as u16, dma_phys, bytes_here)
                .is_ok();
            if !ok {
                dma::dealloc(dma_virt, bytes_here);
                return Err(block::Error::Io);
            }

            unsafe {
                let src = core::slice::from_raw_parts(dma_virt, bytes_here);
                remaining[..bytes_here].copy_from_slice(src);
            }
            dma::dealloc(dma_virt, bytes_here);

            crate::time::poll_executor();
            remaining = &mut remaining[bytes_here..];
            cur_lba = cur_lba.saturating_add(blocks_here as u64);
        }

        Ok(())
    }

    fn write_blocks(&mut self, lba: u64, buf: &[u8]) -> block::Result<()> {
        let bs = self.block_size as usize;
        if bs == 0 || (buf.len() % bs) != 0 {
            return Err(block::Error::InvalidParam);
        }
        let blocks_total = (buf.len() / bs) as u64;
        if blocks_total == 0 {
            return Ok(());
        }
        let end = lba.checked_add(blocks_total).ok_or(block::Error::OutOfBounds)?;
        if end > self.block_count {
            return Err(block::Error::OutOfBounds);
        }

        const MAX_IO_BYTES: usize = 256 * 1024;
        let max_blocks = core::cmp::max(1, MAX_IO_BYTES / bs);

        let mut remaining = buf;
        let mut cur_lba = lba;
        while !remaining.is_empty() {
            let blocks_here = core::cmp::min(max_blocks, remaining.len() / bs);
            let bytes_here = blocks_here * bs;
            let (dma_phys, dma_virt) = dma::alloc(bytes_here, self.ctrl.page_size_bytes()).ok_or(block::Error::DmaUnavailable)?;
            unsafe {
                core::ptr::copy_nonoverlapping(remaining.as_ptr(), dma_virt, bytes_here);
            }

            let ok = self
                .ctrl
                .io_rw(NVME_NVM_WRITE, self.nsid, cur_lba, blocks_here as u16, dma_phys, bytes_here)
                .is_ok();
            dma::dealloc(dma_virt, bytes_here);
            if !ok {
                return Err(block::Error::Io);
            }

            crate::time::poll_executor();
            remaining = &remaining[bytes_here..];
            cur_lba = cur_lba.saturating_add(blocks_here as u64);
        }

        Ok(())
    }

    fn dma_alignment_bytes(&self) -> u32 {
        64
    }

    fn max_transfer_bytes(&self) -> u64 {
        256 * 1024
    }

    fn supports_write(&self) -> bool {
        true
    }

    fn flush(&mut self) -> block::Result<()> {
        self.ctrl.io_flush(self.nsid)
    }
}

fn is_nvme(dev: &crate::pci::PciDevice) -> bool {
    dev.class == 0x01 && dev.subclass == 0x08 && dev.prog_if == 0x02
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

            let mut map_len = if size == 0 { 0x4000usize } else { size as usize };
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

            // For now: only register the first namespace (nsid=1).
            let (blocks, block_size) = match ctrl.identify_namespace(1) {
                Ok(v) => v,
                Err(e) => {
                    crate::log!("nvme: {} identify namespace failed: {:?}\n", pci_addr, e);
                    continue;
                }
            };
            if blocks == 0 || block_size == 0 {
                crate::log!("nvme: {} namespace has invalid capacity\n", pci_addr);
                continue;
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

            let dev = NvmeBlockDevice {
                ctrl,
                nsid: 1,
                block_size,
                block_count: blocks,
            };
            let handle = block::register_device(desc, dev);
            crate::log!(
                "nvme: registered {} id={} blocks={} bs={}\n",
                pci_addr,
                handle.id().raw(),
                blocks,
                block_size
            );
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
