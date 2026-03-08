use core::ptr::NonNull;

use trueos_gfx_core::{Error, Result};

// Legacy ring MMIO layout used on several Intel generations.
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
        batch_phys: u64,
        batch_virt: *mut u8,
        batch_len: usize,
    ) -> Option<Self> {
        if batch_virt.is_null() || batch_phys == 0 || batch_len < 64 {
            return None;
        }
        if mmio.len <= (RCS_RING_BASE + RING_CTL + 4) {
            return None;
        }
        Some(Self {
            mmio,
            batch_phys,
            batch_virt,
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
        let ring_start = (self.batch_phys & !0xFFF) as u32;
        let ring_ctl = (((self.batch_len as u32).saturating_sub(4096)) & !0xFFF) | RING_CTL_VALID;

        self.mmio_write32(Self::ring_off(RING_START), ring_start);
        self.mmio_write32(Self::ring_off(RING_CTL), ring_ctl);

        let _ = self.mmio_read32(Self::ring_off(RING_HEAD));
        self.ring_tail = self.mmio_read32(Self::ring_off(RING_TAIL));
        Ok(())
    }

    pub fn begin_batch(&mut self) {
        self.write_off = 0;
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
        // Build a tiny ring kick batch: BB_START -> current batch -> NOOP.
        let bb_addr = (self.batch_phys & !0x7) as u32;

        let tail = self.ring_tail;
        let _ = tail;

        self.mmio_write32(Self::ring_off(RING_TAIL), 0);
        self.mmio_write32(Self::ring_off(RING_HEAD), 0);

        // Program the batch pointer through MI on ring start scratch.
        // We reuse the mapped command page itself as the ring feeder in this minimal path.
        unsafe {
            let p = self.batch_virt as *mut u32;
            core::ptr::write_volatile(p.add(0), MI_BATCH_BUFFER_START);
            core::ptr::write_volatile(p.add(1), bb_addr);
            core::ptr::write_volatile(p.add(2), MI_NOOP);
            core::ptr::write_volatile(p.add(3), MI_BATCH_BUFFER_END);
        }

        self.mmio_write32(Self::ring_off(RING_TAIL), 16);
        Ok(())
    }
}
