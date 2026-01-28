use crate::disc::block;

struct AlignedBuf {
    ptr: *mut u8,
    len: usize,
    layout: alloc::alloc::Layout,
}

impl AlignedBuf {
    fn new(len: usize, align: usize) -> Option<Self> {
        let layout = alloc::alloc::Layout::from_size_align(len, align).ok()?;
        let ptr = unsafe { alloc::alloc::alloc(layout) };
        if ptr.is_null() {
            return None;
        }
        Some(Self { ptr, len, layout })
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { alloc::alloc::dealloc(self.ptr, self.layout) };
        }
    }
}

pub fn format_blank(handle: block::DeviceHandle) -> Result<(), block::Error> {
    if handle.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    if !handle.supports_write() {
        return Err(block::Error::NotSupported);
    }

    let info = handle.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }

    let max_blocks = if info.max_transfer_bytes > 0 {
        (info.max_transfer_bytes as usize / bs).max(1)
    } else {
        1
    };
    let blocks = core::cmp::min(8usize, max_blocks);
    let bytes = bs.saturating_mul(blocks);

    let align = info.dma_alignment.max(1) as usize;
    let mut tmp = AlignedBuf::new(bytes, align).ok_or(block::Error::DmaUnavailable)?;
    let buf = tmp.as_mut_slice();
    buf.fill(0);

    trueos_fs::write_blank_superblock(&mut buf[..bs]);

    handle.write_blocks(0, buf)?;
    handle.flush()?;
    Ok(())
}
