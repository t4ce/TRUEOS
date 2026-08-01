//! Engine-neutral DMA storage for persistent GPU resources.
//!
//! This layer deliberately does not assign a GPU virtual address or map a
//! PPGTT. The consuming context owns both decisions, so a Spirit allocation
//! cannot accidentally become dependent on Render's address space.

pub(crate) struct ResidentDmaBuffer {
    phys: u64,
    virt: *mut u8,
    bytes: usize,
}

unsafe impl Send for ResidentDmaBuffer {}
unsafe impl Sync for ResidentDmaBuffer {}

impl ResidentDmaBuffer {
    pub(crate) fn allocate_zeroed(bytes: usize, alignment: usize) -> Option<Self> {
        if bytes == 0 || !alignment.is_power_of_two() {
            return None;
        }
        let storage_bytes = bytes.checked_add(alignment - 1)? & !(alignment - 1);
        let (phys, virt) = crate::dma::alloc(storage_bytes, alignment)?;
        unsafe {
            core::ptr::write_bytes(virt, 0, storage_bytes);
        }
        crate::intel::dma_flush(virt, storage_bytes);
        Some(Self {
            phys,
            virt,
            bytes: storage_bytes,
        })
    }

    pub(crate) const fn phys(&self) -> u64 {
        self.phys
    }

    pub(crate) const fn bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) fn write(&self, offset: usize, source: &[u8]) -> bool {
        let Some(end) = offset.checked_add(source.len()) else {
            return false;
        };
        if end > self.bytes {
            return false;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(source.as_ptr(), self.virt.add(offset), source.len());
        }
        true
    }

    pub(crate) fn flush(&self) {
        crate::intel::dma_flush(self.virt, self.bytes);
    }
}

impl Drop for ResidentDmaBuffer {
    fn drop(&mut self) {
        crate::dma::dealloc(self.virt, self.bytes);
    }
}
