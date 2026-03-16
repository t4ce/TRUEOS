use core::ptr::NonNull;

use trueos_gfx_core::{Error, Result};

// Legacy ring MMIO layout used on older Intel generations only.
// Newer platforms need a different submission path.
const PAGE_SIZE: usize = 4096;
const RCS_RING_BASE: usize = 0x2030;
const RING_TAIL: usize = 0x00;
const RING_HEAD: usize = 0x04;
const RING_START: usize = 0x08;
const RING_CTL: usize = 0x0C;

const RING_CTL_VALID: u32 = 1;

const MI_NOOP: u32 = 0x0000_0000;
const MI_BATCH_BUFFER_END: u32 = 0x0A << 23;
const MI_BATCH_BUFFER_START: u32 = (0x31 << 23) | 1;
const MI_FLUSH_DW: u32 = (0x26 << 23) | 1;

#[derive(Clone, Copy)]
pub struct RingMmio {
    pub base: NonNull<u8>,
    pub len: usize,
}

unsafe impl Send for RingMmio {}
unsafe impl Sync for RingMmio {}

pub struct IntelCmd {
    mmio: RingMmio,
    ring_phys: u64,
    ring_virt: *mut u8,
    ring_len: usize,
    batch_phys: u64,
    batch_virt: *mut u8,
    batch_len: usize,
    write_off: usize,
    ring_tail: u32,
}

unsafe impl Send for IntelCmd {}
unsafe impl Sync for IntelCmd {}

impl IntelCmd {
    pub fn new(
        mmio: RingMmio,
        cmd_phys: u64,
        cmd_virt: *mut u8,
        cmd_len: usize,
    ) -> Option<Self> {
        if cmd_virt.is_null() || cmd_phys == 0 || cmd_len < (PAGE_SIZE * 2) {
            return None;
        }
        if mmio.len <= (RCS_RING_BASE + RING_CTL + 4) {
            return None;
        }
        let ring_len = PAGE_SIZE;
        let batch_len = cmd_len.saturating_sub(PAGE_SIZE);
        Some(Self {
            mmio,
            ring_phys: cmd_phys,
            ring_virt: cmd_virt,
            ring_len,
            batch_phys: cmd_phys + (PAGE_SIZE as u64),
            batch_virt: unsafe { cmd_virt.add(PAGE_SIZE) },
            batch_len,
            write_off: 0,
            ring_tail: 0,
        })
    }

    #[inline]
    fn mmio_read32(&self, off: usize) -> u32 {
        let ptr = unsafe { self.mmio.base.as_ptr().add(off) as *const u32 };
        unsafe { core::ptr::read_volatile(ptr) }
    }

    #[inline]
    fn mmio_write32(&self, off: usize, v: u32) {
        let ptr = unsafe { self.mmio.base.as_ptr().add(off) as *mut u32 };
        unsafe { core::ptr::write_volatile(ptr, v) };
    }

    #[inline]
    fn ring_off(reg: usize) -> usize {
        RCS_RING_BASE + reg
    }

    pub fn init_ring(&mut self) -> Result<()> {
        let ring_start = (self.ring_phys & !0xFFF) as u32;
        let ring_ctl = (((self.ring_len as u32).saturating_sub(PAGE_SIZE as u32)) & !0xFFF)
            | RING_CTL_VALID;

        unsafe {
            core::ptr::write_bytes(self.ring_virt, 0, self.ring_len);
        }

        self.mmio_write32(Self::ring_off(RING_START), ring_start);
        self.mmio_write32(Self::ring_off(RING_CTL), ring_ctl);
        self.mmio_write32(Self::ring_off(RING_TAIL), 0);

        let head = self.mmio_read32(Self::ring_off(RING_HEAD));
        self.ring_tail = self.mmio_read32(Self::ring_off(RING_TAIL));
        let n = crate::logflag::INTEL_RING_INIT_LOGS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if n < 4 {
            crate::log!(
                "gfx-intel: ring init start=0x{:08X} ctl=0x{:08X} head=0x{:08X} tail=0x{:08X} ring_phys=0x{:X} ring_len=0x{:X} batch_phys=0x{:X} batch_len=0x{:X}\n",
                ring_start,
                ring_ctl,
                head,
                self.ring_tail,
                self.ring_phys,
                self.ring_len,
                self.batch_phys,
                self.batch_len,
            );
        }
        Ok(())
    }

    pub fn begin_batch(&mut self) {
        self.write_off = 0;
        unsafe {
            core::ptr::write_bytes(self.batch_virt, 0, self.batch_len);
        }
    }

    #[inline]
    fn push_dw(&mut self, v: u32) -> Result<()> {
        if self.write_off + 4 > self.batch_len {
            return Err(Error::OutOfMemory);
        }
        let p = unsafe { self.batch_virt.add(self.write_off) as *mut u32 };
        unsafe { core::ptr::write_volatile(p, v) };
        self.write_off += 4;
        Ok(())
    }

    pub fn emit_noop(&mut self) -> Result<()> {
        self.push_dw(MI_NOOP)
    }

    pub fn emit_cache_flush(&mut self) -> Result<()> {
        // MI_FLUSH_DW with zero payload.
        self.push_dw(MI_FLUSH_DW)?;
        self.push_dw(0)?;
        self.push_dw(0)?;
        self.push_dw(0)
    }

    pub fn emit_batch_end(&mut self) -> Result<()> {
        self.push_dw(MI_BATCH_BUFFER_END)
    }

    pub fn submit_batch(&mut self) -> Result<()> {
        // Build a tiny ring kick page: BB_START -> current batch -> NOOP.
        let bb_addr = (self.batch_phys & !0x7) as u32;

        self.mmio_write32(Self::ring_off(RING_TAIL), 0);

        unsafe {
            let p = self.ring_virt as *mut u32;
            core::ptr::write_volatile(p.add(0), MI_BATCH_BUFFER_START);
            core::ptr::write_volatile(p.add(1), bb_addr);
            core::ptr::write_volatile(p.add(2), MI_NOOP);
            core::ptr::write_volatile(p.add(3), MI_BATCH_BUFFER_END);
        }

        self.mmio_write32(Self::ring_off(RING_TAIL), 16);
        self.ring_tail = 16;
        let n = crate::logflag::INTEL_SUBMIT_LOGS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if n < 8 || (n % 64) == 0 {
            let head = self.mmio_read32(Self::ring_off(RING_HEAD));
            let tail_after = self.mmio_read32(Self::ring_off(RING_TAIL));
            crate::log!(
                "gfx-intel: ring submit seq={} bb=0x{:08X} write_off={} head=0x{:08X} tail=0x{:08X}\n",
                n + 1,
                bb_addr,
                self.write_off,
                head,
                tail_after
            );
        }
        Ok(())
    }
}
