//! Sealed CPU tokenizer artifact for the pinned LFM2.5 GGUF.

extern crate alloc;

use alloc::vec::Vec;
use embassy_time::{Duration, Timer};
use sha2::{Digest, Sha256};

pub const TOKENIZER_PATH: &str = "models/lfm2.5/LFM2.5-350M-Q8_0.tokenizer.bin";
pub const TOKENIZER_BYTES: usize = 1_497_463;
pub const TOKENIZER_SHA256: [u8; 32] = [
    0xdd, 0x64, 0x54, 0xb2, 0x2f, 0x29, 0x5c, 0x43, 0x35, 0x8b, 0x06, 0xcd, 0xd0, 0xef, 0x51, 0x1e,
    0x35, 0xac, 0x73, 0x9b, 0xf0, 0x6a, 0xa7, 0x0e, 0xcc, 0xf9, 0xa7, 0x58, 0x58, 0x0d, 0x7a, 0x35,
];
const READ_CHUNK: usize = 64 * 1024;

#[derive(Debug)]
pub enum Error {
    RootUnavailable,
    Missing,
    Open(crate::disc::block::Error),
    Size {
        observed: u64,
        expected: u64,
    },
    Allocation,
    Read {
        offset: u64,
        source: crate::disc::block::Error,
    },
    ShortRead {
        offset: u64,
        observed: usize,
        expected: usize,
    },
    Hash {
        observed: [u8; 32],
        expected: [u8; 32],
    },
    Artifact(trueos_lfm25_cpu::Error),
}

pub async fn load() -> Result<trueos_lfm25_cpu::Lfm25Tokenizer, Error> {
    let disk = crate::r::fs::trueosfs::primary_root_handle().ok_or(Error::RootUnavailable)?;
    let handle = crate::r::fs::trueosfs::file_read_open_async(disk, TOKENIZER_PATH)
        .await
        .map_err(Error::Open)?
        .ok_or(Error::Missing)?;
    if handle.data_len() != TOKENIZER_BYTES as u64 {
        return Err(Error::Size {
            observed: handle.data_len(),
            expected: TOKENIZER_BYTES as u64,
        });
    }

    let mut artifact = Vec::new();
    artifact
        .try_reserve_exact(TOKENIZER_BYTES)
        .map_err(|_| Error::Allocation)?;
    artifact.resize(TOKENIZER_BYTES, 0);
    let mut hasher = Sha256::new();
    let mut offset = 0usize;
    while offset < artifact.len() {
        let end = core::cmp::min(offset + READ_CHUNK, artifact.len());
        let observed = crate::r::fs::trueosfs::file_read_handle_range_async(
            handle,
            offset as u64,
            &mut artifact[offset..end],
        )
        .await
        .map_err(|source| Error::Read {
            offset: offset as u64,
            source,
        })?
        .unwrap_or(0);
        if observed != end - offset {
            return Err(Error::ShortRead {
                offset: offset as u64,
                observed,
                expected: end - offset,
            });
        }
        hasher.update(&artifact[offset..end]);
        offset = end;
        Timer::after(Duration::from_millis(1)).await;
    }
    let observed: [u8; 32] = hasher.finalize().into();
    if observed != TOKENIZER_SHA256 {
        return Err(Error::Hash {
            observed,
            expected: TOKENIZER_SHA256,
        });
    }
    trueos_lfm25_cpu::Lfm25Tokenizer::from_artifact(&artifact).map_err(Error::Artifact)
}
