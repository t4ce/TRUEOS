use core::fmt;

/// Errors for block-level IO.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockError {
    NotSupported,
    NotReady,
    InvalidParam,
    OutOfBounds,
    Io,
}

impl fmt::Display for BlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type Result<T> = core::result::Result<T, BlockError>;

/// Classic synchronous block device interface.
///
/// Contract:
/// - `buf.len()` is always a multiple of `block_size_bytes()`.
/// - `buf.as_ptr()` is aligned to `dma_alignment_bytes()`.
pub trait BlockDevice {
    fn block_size_bytes(&self) -> u32;
    fn block_count(&self) -> u64;
    fn dma_alignment_bytes(&self) -> u32 {
        1
    }
    fn max_transfer_bytes(&self) -> u64 {
        0
    }
    fn supports_write(&self) -> bool {
        false
    }

    fn read_blocks(&mut self, lba: u64, buf: &mut [u8]) -> Result<()>;

    fn write_blocks(&mut self, _lba: u64, _buf: &[u8]) -> Result<()> {
        Err(BlockError::NotSupported)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}
