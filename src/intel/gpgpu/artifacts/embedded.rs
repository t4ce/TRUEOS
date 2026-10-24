//! Borrow SPIR-V from its existing Zebin section instead of embedding it twice.

const fn u16_at(bytes: &[u8], offset: usize) -> usize {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) as usize
}

const fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

const fn usize_at(bytes: &[u8], offset: usize) -> usize {
    let value = u64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ]);
    assert!(value <= usize::MAX as u64);
    value as usize
}

/// Validate the ELF section table at compile time and return the sole SPIR-V
/// section. The returned slice shares the named binary's static allocation;
/// binary and SPIR-V hashes and runtime admission remain unchanged.
pub(crate) const fn spirv(binary: &[u8]) -> &[u8] {
    assert!(binary.len() >= 64);
    assert!(u32_at(binary, 0) == 0x464c457f);
    assert!(binary[4] == 2 && binary[5] == 1); // ELF64, little endian
    assert!(u16_at(binary, 18) == 205); // Intel GPU
    let table = usize_at(binary, 40);
    let stride = u16_at(binary, 58);
    let count = u16_at(binary, 60);
    assert!(stride == 64 && count > 0);
    assert!(table <= binary.len() && count <= (binary.len() - table) / stride);
    let mut found = false;
    let mut offset = 0;
    let mut size = 0;
    let mut index = 0;
    while index < count {
        let header = table + index * stride;
        if u32_at(binary, header + 4) == 0xff000009 {
            // SHT_ZEBIN_SPIRV
            assert!(!found, "duplicate SPIR-V section in embedded Zebin");
            offset = usize_at(binary, header + 24);
            size = usize_at(binary, header + 32);
            assert!(offset <= binary.len() && size <= binary.len() - offset);
            assert!(size >= 20 && size % 4 == 0);
            assert!(u32_at(binary, offset) == 0x07230203);
            found = true;
        }
        index += 1;
    }
    assert!(found, "missing SPIR-V section in embedded Zebin");
    binary.split_at(offset).1.split_at(size).0
}

#[cfg(all(test, not(target_os = "trueos"), not(target_os = "zkvm")))]
mod tests {
    use super::spirv;

    #[test]
    fn every_committed_compute_spirv_is_borrowed_from_its_zebin() {
        let root = std::path::Path::new(file!())
            .parent()
            .unwrap()
            .join("../../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp");
        let mut count = 0;
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) != Some("bin") {
                continue;
            }
            let binary = std::fs::read(&path).unwrap();
            let expected = std::fs::read(path.with_extension("spv")).unwrap();
            let borrowed = spirv(&binary);
            assert_eq!(borrowed, expected, "{}", path.display());
            let start = binary.as_ptr() as usize;
            assert!((start..start + binary.len()).contains(&(borrowed.as_ptr() as usize)));
            count += 1;
        }
        assert!(count >= 17);
    }

    #[test]
    #[should_panic]
    fn rejects_out_of_bounds_section_table() {
        let mut binary = [0u8; 64];
        binary[..6].copy_from_slice(b"\x7fELF\x02\x01");
        binary[18] = 205;
        binary[40] = 64;
        binary[58] = 64;
        binary[60] = 1;
        spirv(&binary);
    }
}
