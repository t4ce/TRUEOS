//! Fixed TRUEOSFS boundary for the sealed LFM2.5-350M native image.
//!
//! This module deliberately exposes range reads, not a GGUF parser or a whole-file
//! allocation. FPGA model-loading and diagnostic functions can share the exact path,
//! length, hash, and pinned file record.

extern crate alloc;

use alloc::vec::Vec;
use embassy_time::{Duration as EmbassyDuration, Timer};
use sha2::{Digest, Sha256};

pub const NATIVE_IMAGE_PATH: &str = "models/lfm2.5/LFM2.5-350M-Q8_0.native.bin";
pub const NATIVE_IMAGE_BYTES: u64 = trueos_lfm25_model::lfm25::PINNED_NATIVE_IMAGE_BYTES as u64;
pub const NATIVE_IMAGE_SHA256: [u8; 32] = trueos_lfm25_model::lfm25::PINNED_NATIVE_IMAGE_SHA256;
pub const VERIFY_CHUNK_BYTES: usize = 256 * 1024;
const VERIFY_YIELD_MS: u64 = 1;

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
    BufferUnavailable,
    Read {
        offset: u64,
        source: crate::disc::block::Error,
    },
    ShortRead {
        offset: u64,
        observed: usize,
        expected: usize,
    },
    HashMismatch {
        observed: [u8; 32],
        expected: [u8; 32],
    },
}

#[derive(Clone, Copy, Debug)]
pub struct NativeImage {
    handle: crate::r::fs::trueosfs::FileReadHandle,
}

impl NativeImage {
    #[inline]
    pub const fn len(&self) -> u64 {
        self.handle.data_len()
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Read exactly one native-image range from the record pinned by [`open`].
    pub async fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<(), Error> {
        let expected = out.len();
        let observed =
            crate::r::fs::trueosfs::file_read_handle_range_async(self.handle, offset, out)
                .await
                .map_err(|source| Error::Read { offset, source })?
                .unwrap_or(0);
        if observed != expected {
            return Err(Error::ShortRead {
                offset,
                observed,
                expected,
            });
        }
        Ok(())
    }
}

/// Open and size-check the exact native image without parsing or allocating it.
pub async fn open() -> Result<NativeImage, Error> {
    let disk = crate::r::fs::trueosfs::primary_root_handle().ok_or(Error::RootUnavailable)?;
    let handle = crate::r::fs::trueosfs::file_read_open_async(disk, NATIVE_IMAGE_PATH)
        .await
        .map_err(|source| Error::Open { source })?
        .ok_or(Error::Missing)?;
    if handle.data_len() != NATIVE_IMAGE_BYTES {
        return Err(Error::SizeMismatch {
            observed: handle.data_len(),
            expected: NATIVE_IMAGE_BYTES,
        });
    }
    Ok(NativeImage { handle })
}

/// Stream and seal-check the exact native image. `progress` receives byte counts.
pub async fn verify_with_progress(
    image: &NativeImage,
    mut progress: impl FnMut(u64, u64),
) -> Result<[u8; 32], Error> {
    let mut scratch = Vec::new();
    scratch
        .try_reserve_exact(VERIFY_CHUNK_BYTES)
        .map_err(|_| Error::BufferUnavailable)?;
    scratch.resize(VERIFY_CHUNK_BYTES, 0);

    let mut hasher = Sha256::new();
    let mut offset = 0u64;
    while offset < image.len() {
        let want = core::cmp::min(scratch.len() as u64, image.len() - offset) as usize;
        image.read_exact_at(offset, &mut scratch[..want]).await?;
        hasher.update(&scratch[..want]);
        offset += want as u64;
        progress(offset, image.len());
        Timer::after(EmbassyDuration::from_millis(VERIFY_YIELD_MS)).await;
    }

    let observed: [u8; 32] = hasher.finalize().into();
    if observed != NATIVE_IMAGE_SHA256 {
        return Err(Error::HashMismatch {
            observed,
            expected: NATIVE_IMAGE_SHA256,
        });
    }
    Ok(observed)
}
