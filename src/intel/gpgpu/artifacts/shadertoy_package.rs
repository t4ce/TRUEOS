//! Bounded, ordered staging of Blueprint-owned shader packages.
use alloc::vec::Vec;
use sha2::{Digest, Sha256};

#[derive(Copy, Clone, Debug)]
pub(crate) struct ShaderToyPackageContract {
    pub(crate) shader_id: u32,
    pub(crate) bytes: usize,
    pub(crate) bin_bytes: usize,
    pub(crate) spv_bytes: usize,
    pub(crate) sha256: [u8; 32],
}

include!("shadertoy_packages.rs");

pub(crate) fn contract(shader_id: u32) -> Option<ShaderToyPackageContract> {
    Some(match shader_id {
        1 => SHADERTOY_MANDELBROT_PACKAGE,
        2 => SHADERTOY_CUBE_FIELD_PACKAGE,
        3 => SHADERTOY_NGUYEN_PACKAGE,
        4 => SHADERTOY_PALETTE_GRID_PACKAGE,
        5 => SHADERTOY_COSMIC_STRANDS_PACKAGE,
        6 => SHADERTOY_PROTEAN_CLOUDS_PACKAGE,
        _ => return None,
    })
}

impl ShaderToyPackageContract {
    /// Authenticate the entire immutable kernel copy before interpreting bytes.
    /// Source and provenance are covered as well as the executable and SPIR-V.
    pub(crate) fn payloads<'a>(&self, bytes: &'a [u8]) -> Option<(&'a [u8], &'a [u8])> {
        if bytes.len() != self.bytes || Sha256::digest(bytes)[..] != self.sha256 {
            return None;
        }
        if bytes.get(..8)? != b"STPKG01\0" {
            return None;
        }
        let bin_end = 32usize.checked_add(self.bin_bytes)?;
        let spv_end = bin_end.checked_add(self.spv_bytes)?;
        Some((bytes.get(32..bin_end)?, bytes.get(bin_end..spv_end)?))
    }
}

pub(crate) struct ShaderToyPackageUpload {
    pub(crate) contract: ShaderToyPackageContract,
    pub(crate) bytes: Vec<u8>,
}

impl ShaderToyPackageUpload {
    pub(crate) fn new(shader_id: u32) -> Option<Self> {
        let contract = contract(shader_id)?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(contract.bytes).ok()?;
        Some(Self { contract, bytes })
    }

    pub(crate) fn append(&mut self, shader_id: u32, offset: usize, bytes: &[u8]) -> bool {
        if shader_id != self.contract.shader_id
            || offset != self.bytes.len()
            || bytes.is_empty()
            || bytes.len() > self.contract.bytes.saturating_sub(offset)
        {
            return false;
        }
        self.bytes.extend_from_slice(bytes);
        true
    }

    pub(crate) fn complete(&self) -> bool {
        self.bytes.len() == self.contract.bytes
    }
}
