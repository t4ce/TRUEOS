use core::{ptr, ptr::NonNull};

use crate::{disc::block, phys};

const PMM_ALIGN_BYTES: usize = 4096;

pub struct BigMem {
    phys_start: u64,
    ptr: NonNull<u8>,
    len: usize,
}

unsafe impl Send for BigMem {}
unsafe impl Sync for BigMem {}

impl BigMem {
    pub fn new_zeroed(len: usize) -> Result<Self, block::Error> {
        if len == 0 {
            return Err(block::Error::InvalidParam);
        }
        let phys =
            phys::alloc_phys_range(len, PMM_ALIGN_BYTES, 0, None).ok_or(block::Error::NotReady)?;
        let virt = phys::phys_to_virt(phys as usize) as *mut u8;
        let ptr = NonNull::new(virt).ok_or(block::Error::NotReady)?;
        unsafe { ptr::write_bytes(ptr.as_ptr(), 0, len) };
        Ok(Self {
            phys_start: phys,
            ptr,
            len,
        })
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr.as_ptr()
    }
}

impl Drop for BigMem {
    fn drop(&mut self) {
        let _ = phys::free_phys_range(self.phys_start, self.len);
    }
}
