//! Stable, allocation-light metadata for a completed local disk image.

extern crate alloc;

use alloc::string::String;

pub const LEN: usize = 128;
const MAGIC: &[u8; 8] = b"TBKPL001";
const LABEL_LEN: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Manifest {
    pub block_size: u32,
    pub source_blocks: u64,
    pub image_bytes: u64,
    pub sha256: [u8; 32],
    pub source_label: Option<String>,
}

pub fn encode(manifest: &Manifest, source_id: u32) -> [u8; LEN] {
    let mut out = [0u8; LEN];
    out[..8].copy_from_slice(MAGIC);
    out[8..12].copy_from_slice(&manifest.block_size.to_le_bytes());
    out[12..16].copy_from_slice(&source_id.to_le_bytes());
    out[16..24].copy_from_slice(&manifest.source_blocks.to_le_bytes());
    out[24..32].copy_from_slice(&manifest.image_bytes.to_le_bytes());
    out[32..64].copy_from_slice(&manifest.sha256);
    if let Some(label) = &manifest.source_label {
        let bytes = label.as_bytes();
        let count = bytes.len().min(LABEL_LEN - 1);
        out[64..64 + count].copy_from_slice(&bytes[..count]);
    }
    out
}

pub fn decode(bytes: &[u8]) -> Result<Manifest, ()> {
    if bytes.len() != LEN || &bytes[..8] != MAGIC {
        return Err(());
    }
    let block_size = u32::from_le_bytes(bytes[8..12].try_into().map_err(|_| ())?);
    let source_blocks = u64::from_le_bytes(bytes[16..24].try_into().map_err(|_| ())?);
    let image_bytes = u64::from_le_bytes(bytes[24..32].try_into().map_err(|_| ())?);
    if block_size == 0
        || source_blocks == 0
        || source_blocks.checked_mul(block_size as u64) != Some(image_bytes)
    {
        return Err(());
    }
    let mut sha256 = [0u8; 32];
    sha256.copy_from_slice(&bytes[32..64]);
    let label_len = bytes[64..]
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(LABEL_LEN);
    let source_label = core::str::from_utf8(&bytes[64..64 + label_len])
        .ok()
        .filter(|label| !label.is_empty())
        .map(String::from);
    Ok(Manifest {
        block_size,
        source_blocks,
        image_bytes,
        sha256,
        source_label,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            block_size: 4096,
            source_blocks: 32,
            image_bytes: 4096 * 32,
            sha256: [0xA5; 32],
            source_label: Some(String::from("backup source")),
        }
    }

    #[test]
    fn round_trip_preserves_restore_identity() {
        let expected = sample();
        assert_eq!(decode(&encode(&expected, 7)), Ok(expected));
    }

    #[test]
    fn rejects_corrupt_geometry_before_any_restore_can_start() {
        let expected = sample();
        let mut bytes = encode(&expected, 7);
        bytes[24..32].copy_from_slice(&(expected.image_bytes - 1).to_le_bytes());
        assert_eq!(decode(&bytes), Err(()));
    }

    #[test]
    fn labels_are_bounded_without_affecting_digest_or_geometry() {
        let mut expected = sample();
        expected.source_label = Some(String::from("x".repeat(200)));
        let decoded = decode(&encode(&expected, 7)).unwrap();
        assert_eq!(decoded.source_label.unwrap().len(), LABEL_LEN - 1);
        assert_eq!(decoded.sha256, expected.sha256);
        assert_eq!(decoded.image_bytes, expected.image_bytes);
    }
}
