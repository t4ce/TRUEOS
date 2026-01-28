use alloc::vec::Vec;
use alloc::vec;

use crate::block::{BlockDevice, BlockError, Result};

/// Minimal log-structured block store skeleton.
///
/// This will eventually provide:
/// - append allocation into segments
/// - checksummed object writes
/// - checkpoint selection (A/B superblocks)
pub struct Store<D: BlockDevice> {
    dev: D,
}

impl<D: BlockDevice> Store<D> {
    pub fn new(dev: D) -> Self {
        Self { dev }
    }

    pub fn read_exact_at(&mut self, lba: u64, buf: &mut [u8]) -> Result<()> {
        if buf.is_empty() {
            return Ok(());
        }
        let bs = self.dev.block_size_bytes() as usize;
        if bs == 0 || buf.len() % bs != 0 {
            return Err(BlockError::InvalidParam);
        }
        self.dev.read_blocks(lba, buf)
    }

    pub fn read_blocks_vec(&mut self, lba: u64, blocks: usize) -> Result<Vec<u8>> {
        let bs = self.dev.block_size_bytes() as usize;
        if bs == 0 {
            return Err(BlockError::InvalidParam);
        }
        let mut out = vec![0u8; bs.saturating_mul(blocks)];
        self.read_exact_at(lba, &mut out)?;
        Ok(out)
    }

    pub fn flush(&mut self) -> Result<()> {
        self.dev.flush()
    }
}
