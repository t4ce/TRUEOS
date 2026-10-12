//! Bounded, ordered staging of Blueprint-owned shader packages.
use alloc::vec::Vec;
use sha2::{Digest, Sha256};

/// Bound untrusted provenance independently of a particular build's size.
pub(crate) const MAX_PACKAGE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Copy, Clone, Debug)]
pub(crate) struct ShaderToyPackageContract {
    pub(crate) shader_id: u32,
    // Build-time provenance size; runtime uploads may differ in metadata size.
    #[allow(dead_code)]
    pub(crate) bytes: usize,
    pub(crate) bin_bytes: usize,
    pub(crate) spv_bytes: usize,
    // Complete-package provenance hash used by offline build verification only.
    #[allow(dead_code)]
    pub(crate) sha256: [u8; 32],
}

include!("shadertoy_packages.rs");
include!("shadertoy_mandelbox_package.rs");

pub(crate) const SKYBOX_SAMPLE_RGB565_PACKAGE: ShaderToyPackageContract =
    ShaderToyPackageContract {
        shader_id: 17,
        bytes: 99_748,
        bin_bytes: 37_624,
        spv_bytes: 21_292,
        sha256: [
            0x17, 0x0f, 0x02, 0x40, 0xc0, 0xb8, 0x43, 0x6f, 0x60, 0x3e, 0xf1, 0x6e, 0xd3, 0x3d,
            0xff, 0x70, 0xa6, 0x7d, 0xbc, 0x2f, 0x95, 0xdc, 0xf7, 0x0b, 0x72, 0x8e, 0x35, 0x56,
            0x8c, 0x5f, 0xf4, 0x04,
        ],
    };

/// Several gallery selectors use one program with different mode uniforms.
pub(crate) fn program_id(shader_id: u32) -> Option<u32> {
    match shader_id {
        1..=7 | 15 | 16 => Some(shader_id),
        8..=14 => Some(8),
        _ => None,
    }
}

pub(crate) fn contract(shader_id: u32) -> Option<ShaderToyPackageContract> {
    Some(match shader_id {
        17 => SKYBOX_SAMPLE_RGB565_PACKAGE,
        1 => SHADERTOY_MANDELBROT_PACKAGE,
        2 => SHADERTOY_CUBE_FIELD_PACKAGE,
        3 => SHADERTOY_NGUYEN_PACKAGE,
        16 => SHADERTOY_MANDELBOX_PACKAGE,
        4 => SHADERTOY_PALETTE_GRID_PACKAGE,
        5 => SHADERTOY_COSMIC_STRANDS_PACKAGE,
        6 => SHADERTOY_PROTEAN_CLOUDS_PACKAGE,
        7 => SHADERTOY_AUDIO_VISUALIZER_PACKAGE,
        8 => SHADERTOY_CPP_GALLERY_PACKAGE,
        15 => SHADERTOY_PARTICLE_CRAFT_PACKAGE,
        _ => return None,
    })
}

impl ShaderToyPackageContract {
    pub(crate) fn report_mismatch(&self, received_bytes: usize, reason: &str) {
        crate::log_error!(target: "gpgpu";
            "Safety-Guard with SHA256: Shader Package mismatch shader={} reason={} received_bytes={} action=reject\n",
            self.shader_id, reason, received_bytes);
    }

    /// Decode a bounded package and authenticate executable payloads against
    /// the kernel-owned ABI contract. Embedded source/provenance is never used
    /// for execution; its complete-package hash is verified at build time.
    pub(crate) fn payloads<'a>(&self, bytes: &'a [u8]) -> Option<(&'a [u8], &'a [u8])> {
        if bytes.len() > MAX_PACKAGE_BYTES || bytes.get(..8)? != b"STPKG01\0" {
            return None;
        }
        let mut lengths = [0usize; 6];
        for (i, length) in lengths.iter_mut().enumerate() {
            *length = u32::from_le_bytes(bytes.get(8 + i * 4..12 + i * 4)?.try_into().ok()?) as usize;
        }
        let total = lengths.iter().try_fold(32usize, |sum, length| sum.checked_add(*length))?;
        if total != bytes.len() || lengths[0] != self.bin_bytes || lengths[1] != self.spv_bytes {
            return None;
        }
        let bin_end = 32usize.checked_add(self.bin_bytes)?;
        let spv_end = bin_end.checked_add(self.spv_bytes)?;
        let bin = bytes.get(32..bin_end)?;
        let spv = bytes.get(bin_end..spv_end)?;
        let (bin_hash, spv_hash) = execution_hashes(self.shader_id)?;
        if Sha256::digest(bin)[..] != bin_hash || Sha256::digest(spv)[..] != spv_hash {
            return None;
        }
        Some((bin, spv))
    }
}

fn execution_hashes(shader_id: u32) -> Option<([u8; 32], [u8; 32])> {
    let abi = match shader_id {
        17 => &super::SKYBOX_SAMPLE_RGB565_ADLS_CPP_ABI_CONTRACT,
        15 => &super::PARTICLE_CRAFT_STEP_ADLS_CPP_ABI_CONTRACT,
        _ => super::shadertoy_artifact_slot(shader_id)?.0.abi_contract?,
    };
    Some((abi.zebin_sha256, abi.spv_sha256))
}

pub(crate) struct ShaderToyPackageUpload {
    pub(crate) contract: ShaderToyPackageContract,
    pub(crate) bytes: Vec<u8>,
    package_len: usize,
}

impl ShaderToyPackageUpload {
    pub(crate) fn new(shader_id: u32, package_len: usize) -> Option<Self> {
        if !(32..=MAX_PACKAGE_BYTES).contains(&package_len) {
            return None;
        }
        let contract = contract(shader_id)?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(package_len).ok()?;
        Some(Self { contract, bytes, package_len })
    }

    pub(crate) fn append(&mut self, shader_id: u32, offset: usize, bytes: &[u8]) -> bool {
        if shader_id != self.contract.shader_id
            || offset != self.bytes.len()
            || bytes.is_empty()
            || bytes.len() > self.package_len.saturating_sub(offset)
        {
            return false;
        }
        self.bytes.extend_from_slice(bytes);
        true
    }

    pub(crate) fn complete(&self) -> bool {
        self.bytes.len() == self.package_len
    }

    pub(crate) fn package_len(&self) -> usize {
        self.package_len
    }
}
