#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    NotSupported,
    NotReady,
    InvalidParam,
    OutOfBounds,
    DmaUnavailable,
    MmioMapFailed,
    Timeout,
    Io,
}

pub type Result<T> = core::result::Result<T, Error>;

/// Minimal block device interface for kernel drivers.
///
/// Contract:
/// - `buf.len()` must be a multiple of `block_size_bytes()`.
/// - `lba` is in units of `block_size_bytes()`.
pub trait BlockDevice {
    fn block_size_bytes(&self) -> u32;
    fn block_count(&self) -> u64;

    fn read_blocks(&mut self, lba: u64, buf: &mut [u8]) -> Result<()>;

    fn write_blocks(&mut self, _lba: u64, _buf: &[u8]) -> Result<()> {
        Err(Error::NotSupported)
    }
}
