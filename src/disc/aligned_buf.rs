//! Owned, initialized scratch storage for disk installation and formatting.

#![forbid(unsafe_code)]

use alloc::vec::Vec;

/// A fixed aligned window into an owned byte allocation.
///
/// Reserving up to `align - 1` extra bytes lets ordinary slice bounds carry the
/// allocation's lifetime and extent. This guarantees virtual address alignment;
/// physical contiguity and device synchronization remain the block layer's job.
pub(crate) struct AlignedBuf {
    storage: Vec<u8>,
    start: usize,
    len: usize,
}

impl AlignedBuf {
    pub(crate) fn new(len: usize, align: usize) -> Option<Self> {
        if !align.is_power_of_two() {
            return None;
        }
        let allocation_len = len.checked_add(align - 1)?;
        let mut storage = Vec::new();
        storage.try_reserve_exact(allocation_len).ok()?;
        storage.resize(allocation_len, 0);
        let start = storage.as_ptr().align_offset(align);
        // Keep the window within initialized storage, including empty buffers.
        storage.get(start..start.checked_add(len)?)?;
        Some(Self {
            storage,
            start,
            len,
        })
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.storage[self.start..self.start + self.len]
    }
}

#[cfg(test)]
mod tests {
    use super::AlignedBuf;

    #[test]
    fn windows_are_aligned_initialized_and_keep_their_contents() {
        for align in [1, 2, 8, 64, 512, 4096] {
            for len in [0, 1, 7, 511, 512, 4097] {
                let mut buffer = AlignedBuf::new(len, align).unwrap();
                let bytes = buffer.as_mut_slice();
                assert_eq!(bytes.len(), len);
                assert_eq!(bytes.as_ptr().align_offset(align), 0);
                assert!(bytes.iter().all(|byte| *byte == 0));
                bytes.fill(0xa5);
                // Moving the owner must not change the allocated window.
                let mut moved = buffer;
                assert!(moved.as_mut_slice().iter().all(|byte| *byte == 0xa5));
            }
        }
    }

    #[test]
    fn invalid_layouts_and_overflows_fail_before_allocation() {
        assert!(AlignedBuf::new(512, 0).is_none());
        assert!(AlignedBuf::new(512, 3).is_none());
        assert!(AlignedBuf::new(usize::MAX, 512).is_none());
        assert!(AlignedBuf::new(usize::MAX, 1).is_none());
    }
}
