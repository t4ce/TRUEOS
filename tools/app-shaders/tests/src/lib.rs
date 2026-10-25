extern crate alloc;
#[path = "../../../../src/gpu/app_shaders.rs"]
pub(crate) mod app_shaders;
mod gpu { pub(crate) use crate::app_shaders; }
#[allow(dead_code, unfulfilled_lint_expectations)]
pub(crate) mod metadata {
    include!(concat!(env!("OUT_DIR"), "/types.rs"));
    pub(crate) mod patch { include!(concat!(env!("OUT_DIR"), "/patch.rs")); }
    pub(crate) mod world { include!(concat!(env!("OUT_DIR"), "/world.rs")); }
    pub(crate) mod figure { include!(concat!(env!("OUT_DIR"), "/figure.rs")); }
    pub(crate) mod cloud { include!(concat!(env!("OUT_DIR"), "/cloud.rs")); }
    pub(crate) mod pbr { include!(concat!(env!("OUT_DIR"), "/pbr.rs")); }
    pub(crate) mod maxpix { include!(concat!(env!("OUT_DIR"), "/maxpix.rs")); }
    pub(crate) mod adjacency { include!(concat!(env!("OUT_DIR"), "/adjacency.rs")); }
    pub(crate) mod triangle { include!(concat!(env!("OUT_DIR"), "/triangle.rs")); }
    pub(crate) mod mask { include!(concat!(env!("OUT_DIR"), "/mask.rs")); }
    pub(crate) use patch as patch_cube;
    pub(crate) fn triangle_pipeline() -> &'static TrianglePipeline { triangle::triangle_pipeline() }
    pub(crate) fn triangle_pipeline_simd16() -> &'static TrianglePipeline { triangle::triangle_pipeline_simd16() }
    pub(crate) fn line_adjacency_geometry_shader() -> &'static AdjacencyGeometryShader { adjacency::line_adjacency_geometry_shader() }
    pub(crate) fn triangle_adjacency_geometry_shader() -> &'static AdjacencyGeometryShader { adjacency::triangle_adjacency_geometry_shader() }
    include!(concat!(env!("OUT_DIR"), "/patch_layout.rs"));
}


mod intel {
    pub(crate) use crate::metadata as shader;
    pub(crate) const WARM_ALIGN: usize = 4096;
    pub(crate) fn align_up(value: usize, alignment: usize) -> Option<usize> {
        value.checked_add(alignment-1).map(|value| value & !(alignment-1))
    }
    pub(crate) fn dma_flush(_: *mut u8, _: usize) {}
}
#[allow(dead_code)]
mod upload {
    struct RenderWarmState { draw_state_virt: *mut u8, draw_state_len: usize, device_id: u16 }
    include!(concat!(env!("OUT_DIR"), "/upload.rs"));
    pub(crate) fn boot_font_without_app_shaders() {
        assert!(!crate::app_shaders::present(7));
        let pipeline = crate::metadata::mask::microfont_mask_pipeline();
        let mut memory=vec![0u32; 65536];
        let warm=RenderWarmState { draw_state_virt:memory.as_mut_ptr().cast(), draw_state_len:memory.len()*4, device_id:0x4680 };
        let layout=upload_triangle_shader_pipeline_at(warm, pipeline, None, 0x1000, false).unwrap();
        assert_eq!(layout.line_adjacency_gs.code_size_bytes, 0);
        assert_eq!(layout.triangle_adjacency_gs.code_size_bytes, 0);
        let offset=layout.vs.code_offset_bytes as usize / 4;
        assert_eq!(&memory[offset..offset+pipeline.vs.code.len()], pipeline.vs.code);
        let offset=layout.ps.code_offset_bytes as usize / 4;
        assert_eq!(&memory[offset..offset+pipeline.ps.code.len()], pipeline.ps.code);
    }
}

#[cfg(test)]
#[allow(dead_code)]
mod retained_transform {
    include!(concat!(env!("OUT_DIR"), "/retained_transform.rs"));

    #[test]
    fn app_owned_transform_passes_encoder_admission_with_empty_embedded_bin() {
        use sha2::{Digest, Sha256};
        crate::app_shaders::register_section(&super::tests::bundle("TRUEOS-Picasso")).unwrap();
        let bin = crate::app_shaders::bytes(9, 0).unwrap();
        assert!(HELIO_RETAINED_TRANSFORM_ADLS_BIN.is_empty());
        assert_eq!(bin.len(), 206768);
        let upload = UploadedKernelArtifact {
            name: HELIO_RETAINED_TRANSFORM_KERNEL_NAME,
            target: "adls",
            source: "app-package",
            gpu: HELIO_RETAINED_TRANSFORM_ADLS_GPU,
            phys: 0x0200_0000,
            bytes: bin.len(),
            mapped_bytes: bin.len().next_multiple_of(4096),
            verified: true,
            bin_sha256: Sha256::digest(bin).into(),
            device_id: 0x4680,
            revision_id: 0x0C,
            abi_schema_version: Some(GPGPU_KERNEL_ABI_SCHEMA_VERSION),
            address_space: GpgpuArtifactAddressSpace::CallerPpgtt,
        };
        let mapping = |upload: UploadedKernelArtifact| GpgpuHelioRetainedTransformArtifactMapping {
            gpu: upload.gpu,
            phys: upload.phys,
            bytes: upload.bytes,
            mapped_bytes: upload.mapped_bytes,
            entry_offset: HELIO_RETAINED_TRANSFORM_ADLS_CPP_ABI_CONTRACT.entry_offset,
            upload,
        };
        assert_eq!(validate_helio_transform_artifact(mapping(upload)), Ok(()));
        for invalid in [
            UploadedKernelArtifact { bytes: 0, ..upload },
            UploadedKernelArtifact { bytes: 4096, ..upload },
            UploadedKernelArtifact { verified: false, ..upload },
            UploadedKernelArtifact { bin_sha256: [0; 32], ..upload },
            UploadedKernelArtifact { mapped_bytes: 4096, ..upload },
            UploadedKernelArtifact { address_space: GpgpuArtifactAddressSpace::GlobalGgtt, ..upload },
        ] {
            assert_eq!(validate_helio_transform_artifact(mapping(invalid)), Err(GpgpuHelioTransformError::Artifact));
        }
        let mut inconsistent = mapping(upload);
        inconsistent.bytes -= 1;
        assert_eq!(validate_helio_transform_artifact(inconsistent), Err(GpgpuHelioTransformError::Artifact));
    }
}

#[cfg(test)]
mod tests {
    use super::app_shaders;
    pub(super) fn bundle(owner: &str) -> Vec<u8> {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../..").join(owner).join("shaders/trueos/app-shaders.bundle");
        std::fs::read(path).unwrap()
    }
    #[test]
    fn downloaded_bundles_populate_aligned_shared_storage_once() {
        crate::upload::boot_font_without_app_shaders();
        for owner in ["Cubes", "TRUEOS-Picasso", "voxy"] {
            let package = bundle(owner);
            app_shaders::register_blueprint_elf(&elf(&package)).unwrap();
            app_shaders::register_blueprint_elf(&elf(&package)).unwrap();
        }
        for id in 1..=10 { assert!(app_shaders::present(id)); }
        let cube = app_shaders::words(4, 0).unwrap();
        assert_eq!(cube.len(), 188);
        assert_eq!(cube.as_ptr() as usize % 4, 0);
        let pointer = cube.as_ptr();
        app_shaders::register_section(&bundle("Cubes")).unwrap();
        assert_eq!(app_shaders::words(4, 0).unwrap().as_ptr(), pointer);
        assert_eq!(app_shaders::words(4, 1).unwrap().len(), 4596);
        assert_eq!(app_shaders::bytes(9, 0).unwrap().len(), 206768);
        assert_eq!(&app_shaders::bytes(9, 0).unwrap()[..4], b"\x7fELF");
        assert_eq!(app_shaders::bytes(8, 0).unwrap().len(), 44742);
        assert!(app_shaders::words(8, 0).is_none());
        assert!(app_shaders::bytes(4, 0).is_none());
        assert!(app_shaders::words(0, 0).is_none());
        assert!(app_shaders::words(11, 0).is_none());
        for pipeline in [crate::metadata::patch::pipeline(), crate::metadata::world::pipeline(),
            crate::metadata::figure::pipeline(), crate::metadata::cloud::pipeline(),
            crate::metadata::pbr::pipeline(), crate::metadata::maxpix::pipeline()] {
            assert_eq!(pipeline.vs.code.len() * 4, pipeline.vs.meta.kernel.code_size_bytes as usize);
            assert_eq!(pipeline.ps.code.len() * 4, pipeline.ps.meta.kernel.code_size_bytes as usize);
            assert!(!pipeline.vs.code.is_empty());
        }
        assert!(crate::metadata::patch::matches(crate::metadata::patch::pipeline()));
        assert_eq!(crate::metadata::patch::tess_control().len(), 4596);
        assert_eq!(crate::metadata::patch::tess_eval().len(), 3064);
        assert_eq!(crate::metadata::adjacency::line_adjacency_geometry_shader().code.len(), 96);
        assert_eq!(crate::metadata::adjacency::triangle_adjacency_geometry_shader().code.len(), 128);

    }
    fn elf(package: &[u8]) -> Vec<u8> {
        // Minimal ELF64 REL with the real app-owned section and a string table.
        let names = b"\0.trueos.shaders\0.shstrtab\0";
        let data_offset = 64 + names.len();
        let table_offset = (data_offset + package.len() + 7) & !7;
        let mut image = vec![0u8; table_offset + 3 * 64];
        image[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        image[16..18].copy_from_slice(&1u16.to_le_bytes());
        image[18..20].copy_from_slice(&62u16.to_le_bytes());
        image[20..24].copy_from_slice(&1u32.to_le_bytes());
        image[40..48].copy_from_slice(&(table_offset as u64).to_le_bytes());
        image[52..54].copy_from_slice(&64u16.to_le_bytes());
        image[58..60].copy_from_slice(&64u16.to_le_bytes());
        image[60..62].copy_from_slice(&3u16.to_le_bytes());
        image[62..64].copy_from_slice(&2u16.to_le_bytes());
        image[64..64+names.len()].copy_from_slice(names);
        image[data_offset..data_offset+package.len()].copy_from_slice(package);
        for (index, name, kind, offset, len) in [
            (1, 1u32, 1u32, data_offset, package.len()),
            (2, 17u32, 3u32, 64, names.len()),
        ] {
            let start=table_offset+index*64;
            image[start..start+4].copy_from_slice(&name.to_le_bytes());
            image[start+4..start+8].copy_from_slice(&kind.to_le_bytes());
            image[start+24..start+32].copy_from_slice(&(offset as u64).to_le_bytes());
            image[start+32..start+40].copy_from_slice(&(len as u64).to_le_bytes());
        }
        image
    }
    #[test]
    fn rejects_corrupt_unknown_and_truncated_downloads() {
        assert_eq!(app_shaders::register_blueprint_elf(b"not ELF"), Err("app-shader-elf"));
        let mut package = bundle("Cubes");
        *package.last_mut().unwrap() ^= 1;
        assert_eq!(app_shaders::register_blueprint_elf(&elf(&package)), Err("app-shader-sha256-mismatch"));
        let package = bundle("Cubes");
        assert_eq!(app_shaders::register_section(&package[..package.len()-1]), Err("app-shader-truncated"));
        let mut package = bundle("Cubes");
        package[8..12].copy_from_slice(&99u32.to_le_bytes());
        assert_eq!(app_shaders::register_section(&package), Err("app-shader-untrusted-id"));
        assert_eq!(app_shaders::register_section(b"not-a-package"), Err("app-shader-magic"));
    }
}
