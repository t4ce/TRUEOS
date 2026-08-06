pub use crate::graphics::primitives::*;

use core::fmt;

#[derive(Clone, Copy)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
