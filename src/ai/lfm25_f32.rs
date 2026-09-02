//! Fail-closed TRUEOSFS loader for the LFM2.5 source-F32 sidecar.
//!
//! The fixed native image intentionally retains its original BF16 representation
//! for these tensors. Hybrid decoding requires the original
//! GGUF F32 bits, sealed separately and bound to the GGUF, native image, and
//! generated tensor table.

extern crate alloc;

use alloc::vec::Vec;
use sha2::{Digest, Sha256};
use trueos_lfm25_cpu::{F32_SIDECAR_BYTES, F32Sidecar};

pub const F32_SIDECAR_PATH: &str = "apps/lumen/LFM2.5-350M-Q8_0.cpu-f32.bin";
pub const F32_SIDECAR_SHA256: [u8; 32] = [
    0xa6, 0x0c, 0x0d, 0x28, 0xe5, 0xe0, 0xf4, 0x83, 0x06, 0x99, 0x26, 0x0f, 0xbd, 0x9c, 0x01, 0x15,
    0x37, 0x63, 0x26, 0x1a, 0x7b, 0x13, 0x2a, 0x6b, 0x44, 0x61, 0x0d, 0x64, 0x91, 0x96, 0x09, 0xb1,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    RootUnavailable,
    Missing,
    Open {
        source: crate::disc::block::Error,
    },
    SizeMismatch {
        observed: u64,
        expected: u64,
    },
    Allocation,
    Read {
        source: crate::disc::block::Error,
    },
    ShortRead {
        observed: usize,
        expected: usize,
    },
    HashMismatch {
        observed: [u8; 32],
        expected: [u8; 32],
    },
    Artifact,
}

/// Load and fully verify the sidecar before the caller creates decoder state.
pub async fn load() -> Result<F32Sidecar, Error> {
    let disk = crate::r::fs::trueosfs::primary_root_handle().ok_or(Error::RootUnavailable)?;
    let handle = crate::r::fs::trueosfs::file_read_open_async(disk, F32_SIDECAR_PATH)
        .await
        .map_err(|source| Error::Open { source })?
        .ok_or(Error::Missing)?;
    if handle.data_len() != F32_SIDECAR_BYTES as u64 {
        return Err(Error::SizeMismatch {
            observed: handle.data_len(),
            expected: F32_SIDECAR_BYTES as u64,
        });
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(F32_SIDECAR_BYTES)
        .map_err(|_| Error::Allocation)?;
    bytes.resize(F32_SIDECAR_BYTES, 0);
    let observed = crate::r::fs::trueosfs::file_read_handle_range_async(handle, 0, &mut bytes)
        .await
        .map_err(|source| Error::Read { source })?
        .unwrap_or(0);
    if observed != bytes.len() {
        return Err(Error::ShortRead {
            observed,
            expected: bytes.len(),
        });
    }
    let observed_hash: [u8; 32] = Sha256::digest(&bytes).into();
    if observed_hash != F32_SIDECAR_SHA256 {
        return Err(Error::HashMismatch {
            observed: observed_hash,
            expected: F32_SIDECAR_SHA256,
        });
    }
    F32Sidecar::from_artifact(&bytes).map_err(|_| Error::Artifact)
}
