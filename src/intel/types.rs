pub use crate::graphics::primitives::*;

use core::fmt;

#[derive(Clone, Copy)]
pub struct MappedRange {
    pub ptr: *mut u8,
    pub len: usize,
}

impl fmt::Debug for MappedRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MappedRange")
            .field("ptr", &self.ptr)
            .field("len", &self.len)
            .finish()
    }
}
