/// Sealed HELIOA-native package contract for HelioC's volume update/raymarch
/// workload.
///
/// This is deliberately an admission contract, not a submit path. It names
/// compiler-produced source and ISA sections, authenticates each exact byte
/// sequence, and records only logical binding requirements. Intel surface,
/// sampler, and graphics-state encodings remain outside this format until
/// hardware proof supplies them.
#[allow(dead_code)]
pub(crate) mod helioc_native_package {
    use crate::gpu::vgpu::{CLOUD_RENDER_WGSL_SHA256, CLOUD_SIMULATION_WGSL_SHA256};
    use sha2::{Digest, Sha256};
    use trueos_helio_artifact::{Artifact, RequiredSection, SectionKind};

    pub(crate) const PACKAGE_SECTION: &str = "compiler/helioc-native-volume-raymarch-v1.bin";
    pub(crate) const COMPUTE_SOURCE_SECTION: &str = "authored/cloud-engine/simulate.wgsl";
    pub(crate) const GRAPHICS_SOURCE_SECTION: &str = "authored/cloud-engine/render.wgsl";
    pub(crate) const VOLUME_RESOURCES_SECTION: &str = "resources/volume3d-rgba16f-v1.bin";
    pub(crate) const COMPUTE_ISA_SECTION: &str = "intel-xe-lp/helioc-volume-update.bin";
    pub(crate) const VERTEX_ISA_SECTION: &str = "intel-xe-lp/helioc-volume-raymarch.vs.bin";
    pub(crate) const FRAGMENT_ISA_SECTION: &str = "intel-xe-lp/helioc-volume-raymarch.fs.bin";

    const MAGIC: [u8; 8] = *b"HELIOC\0\0";
    const VERSION: u16 = 1;
    const BYTE_LEN: usize = 320;
    // ADL-S GT1 / UHD 770 is Mesa gfx12 (verx10=120).  gfx125 names the
    // Xe-HP/DG2 packet family and must never authenticate Xe-LP ISA/state.
    const GFX_VERSION: u16 = 120;
    const ADLS_UHD_770_DEVICE_ID: u16 = 0x4680;
    const ADLS_UHD_770_REVISION: u8 = 0x0C;
    const LOCAL_DIMENSIONS: [u16; 3] = [4, 4, 4];
    const GROUP_DIMENSIONS: [u16; 3] = [24, 12, 24];
    const LOCAL_INVOCATIONS: u16 = 64;
    const CROSS_THREAD_BYTES: u16 = 96;
    const INDIRECT_BYTES: u16 = 480;
    const REQUIRED_BINDINGS: [[u8; 4]; 5] = [
        // stage, group, binding, logical access
        [1, 0, 1, 1], // compute: sampled source volume
        [1, 0, 2, 2], // compute: normalized cloud sampler
        [1, 0, 3, 3], // compute: write-only storage destination volume
        [2, 0, 1, 1], // graphics: sampled volume
        [2, 0, 2, 2], // graphics: normalized cloud sampler
    ];
    const REQUIRED_STATE_FLAGS: u32 = 0x3F;
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum Error {
        MissingSection,
        WrongSectionKind,
        InvalidDescriptor,
        UnsupportedTarget,
        InvalidComputeShape,
        InvalidPayload,
        InvalidBindings,
        InvalidSource,
        InvalidIsa,
        InvalidResourceContract,
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) struct NativePackage<'a> {
        descriptor: &'a [u8],
        compute_simd_width: u16,
    }

    impl<'a> NativePackage<'a> {
        pub(crate) fn parse_artifact(artifact: Artifact<'a>) -> Result<Self, Error> {
            let descriptor = required(artifact, PACKAGE_SECTION, SectionKind::CompilerMetadata)?;
            let package = Self::parse(descriptor)?;

            let compute_source =
                required(artifact, COMPUTE_SOURCE_SECTION, SectionKind::ShaderSource)?;
            let graphics_source =
                required(artifact, GRAPHICS_SOURCE_SECTION, SectionKind::ShaderSource)?;
            let resources =
                required(artifact, VOLUME_RESOURCES_SECTION, SectionKind::NormalizedRenderIr)?;
            let compute_isa = required(artifact, COMPUTE_ISA_SECTION, SectionKind::IntelXeLpIsa)?;
            let vertex_isa = required(artifact, VERTEX_ISA_SECTION, SectionKind::IntelXeLpIsa)?;
            let fragment_isa = required(artifact, FRAGMENT_ISA_SECTION, SectionKind::IntelXeLpIsa)?;

            verify_reference(
                compute_source,
                package.compute_source_bytes(),
                CLOUD_SIMULATION_WGSL_SHA256,
                Error::InvalidSource,
            )?;
            verify_reference(
                graphics_source,
                package.graphics_source_bytes(),
                CLOUD_RENDER_WGSL_SHA256,
                Error::InvalidSource,
            )?;
            verify_reference(
                resources,
                package.resource_bytes(),
                package.hash_at(192),
                Error::InvalidResourceContract,
            )?;
            verify_reference(
                compute_isa,
                package.compute_isa_bytes(),
                package.hash_at(224),
                Error::InvalidIsa,
            )?;
            verify_reference(
                vertex_isa,
                package.vertex_isa_bytes(),
                package.hash_at(256),
                Error::InvalidIsa,
            )?;
            verify_reference(
                fragment_isa,
                package.fragment_isa_bytes(),
                package.hash_at(288),
                Error::InvalidIsa,
            )?;
            let volume_program = super::helio_volume_resources::Program::parse_artifact(artifact)
                .map_err(|_| Error::InvalidResourceContract)?;
            if !volume_program.is_helio_cloud_profile() {
                return Err(Error::InvalidResourceContract);
            }
            Ok(package)
        }

        pub(crate) fn parse(descriptor: &'a [u8]) -> Result<Self, Error> {
            if descriptor.len() != BYTE_LEN
                || descriptor.get(..8) != Some(MAGIC.as_slice())
                || read_u16(descriptor, 8) != Some(VERSION)
                || read_u16(descriptor, 10) != Some(BYTE_LEN as u16)
                || read_u32(descriptor, 12) != Some(BYTE_LEN as u32)
                || read_u32(descriptor, 16) != Some(0x0F)
            {
                return Err(Error::InvalidDescriptor);
            }
            if read_u16(descriptor, 20) != Some(GFX_VERSION)
                || read_u16(descriptor, 22) != Some(ADLS_UHD_770_DEVICE_ID)
                || descriptor[24] != ADLS_UHD_770_REVISION
                || descriptor[25] != ADLS_UHD_770_REVISION
                || descriptor[26] != 1
                || descriptor[27] != 0
            {
                return Err(Error::UnsupportedTarget);
            }

            let compute_simd_width = read_u16(descriptor, 28).ok_or(Error::InvalidDescriptor)?;
            let compute_threads = read_u16(descriptor, 30).ok_or(Error::InvalidDescriptor)?;
            let local = [
                read_u16(descriptor, 32).ok_or(Error::InvalidDescriptor)?,
                read_u16(descriptor, 34).ok_or(Error::InvalidDescriptor)?,
                read_u16(descriptor, 36).ok_or(Error::InvalidDescriptor)?,
            ];
            let groups = [
                read_u16(descriptor, 38).ok_or(Error::InvalidDescriptor)?,
                read_u16(descriptor, 40).ok_or(Error::InvalidDescriptor)?,
                read_u16(descriptor, 42).ok_or(Error::InvalidDescriptor)?,
            ];
            if local != LOCAL_DIMENSIONS
                || groups != GROUP_DIMENSIONS
                || compute_simd_width != 16 && compute_simd_width != 32
            {
                return Err(Error::InvalidComputeShape);
            }
            let expected_threads = LOCAL_INVOCATIONS / compute_simd_width;
            if compute_threads != expected_threads || expected_threads != 4 && expected_threads != 2
            {
                return Err(Error::InvalidComputeShape);
            }
            let expected_per_thread = if compute_simd_width == 16 { 96 } else { 192 };
            if read_u16(descriptor, 44) != Some(CROSS_THREAD_BYTES)
                || read_u16(descriptor, 46) != Some(expected_per_thread)
                || read_u16(descriptor, 48) != Some(INDIRECT_BYTES)
                || usize::from(CROSS_THREAD_BYTES)
                    + usize::from(expected_per_thread) * usize::from(expected_threads)
                    != usize::from(INDIRECT_BYTES)
            {
                return Err(Error::InvalidPayload);
            }
            if read_u16(descriptor, 50) != Some(16)
                || read_u16(descriptor, 52) != Some(REQUIRED_BINDINGS.len() as u16)
                || read_u32(descriptor, 56) != Some(REQUIRED_STATE_FLAGS)
            {
                return Err(Error::InvalidBindings);
            }
            for (index, expected) in REQUIRED_BINDINGS.into_iter().enumerate() {
                let offset = 60 + index * 4;
                if descriptor[offset..offset + 4] != expected {
                    return Err(Error::InvalidBindings);
                }
            }
            if descriptor[54..56].iter().any(|byte| *byte != 0)
                || descriptor[80..96].iter().any(|byte| *byte != 0)
                || descriptor[120..128].iter().any(|byte| *byte != 0)
            {
                return Err(Error::InvalidBindings);
            }
            for offset in [128, 160, 192, 224, 256, 288] {
                if descriptor[offset..offset + 32]
                    .iter()
                    .all(|byte| *byte == 0)
                {
                    return Err(Error::InvalidDescriptor);
                }
            }
            if descriptor[128..160] != CLOUD_SIMULATION_WGSL_SHA256
                || descriptor[160..192] != CLOUD_RENDER_WGSL_SHA256
            {
                return Err(Error::InvalidSource);
            }
            Ok(Self {
                descriptor,
                compute_simd_width,
            })
        }

        pub(crate) const fn compute_simd_width(self) -> u16 {
            self.compute_simd_width
        }

        pub(crate) const fn compute_hardware_threads(self) -> u16 {
            LOCAL_INVOCATIONS / self.compute_simd_width
        }

        pub(crate) const fn descriptor(self) -> &'a [u8] {
            self.descriptor
        }

        fn compute_source_bytes(self) -> usize {
            read_u32(self.descriptor, 96).unwrap_or(0) as usize
        }
        fn graphics_source_bytes(self) -> usize {
            read_u32(self.descriptor, 100).unwrap_or(0) as usize
        }
        fn resource_bytes(self) -> usize {
            read_u32(self.descriptor, 104).unwrap_or(0) as usize
        }
        fn compute_isa_bytes(self) -> usize {
            read_u32(self.descriptor, 108).unwrap_or(0) as usize
        }
        fn vertex_isa_bytes(self) -> usize {
            read_u32(self.descriptor, 112).unwrap_or(0) as usize
        }
        fn fragment_isa_bytes(self) -> usize {
            read_u32(self.descriptor, 116).unwrap_or(0) as usize
        }
        fn hash_at(self, offset: usize) -> [u8; 32] {
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&self.descriptor[offset..offset + 32]);
            hash
        }
    }

    fn required<'a>(
        artifact: Artifact<'a>,
        name: &'static str,
        kind: SectionKind,
    ) -> Result<&'a [u8], Error> {
        let section = artifact
            .require(RequiredSection::new(name, kind))
            .map_err(|error| match error {
                trueos_helio_artifact::Error::MissingSection => Error::MissingSection,
                trueos_helio_artifact::Error::WrongSectionKind { .. } => Error::WrongSectionKind,
                _ => Error::InvalidDescriptor,
            })?;
        Ok(section.data)
    }

    fn verify_reference(
        data: &[u8],
        expected_bytes: usize,
        expected_hash: [u8; 32],
        error: Error,
    ) -> Result<(), Error> {
        if data.is_empty()
            || data.len() != expected_bytes
            || (!data.len().is_multiple_of(4) && matches!(error, Error::InvalidIsa))
        {
            return Err(error);
        }
        let actual = Sha256::digest(data);
        (actual[..] == expected_hash).then_some(()).ok_or(error)
    }

    fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
        Some(u16::from_le_bytes(bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?))
    }

    fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
        Some(u32::from_le_bytes(bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use alloc::{vec, vec::Vec};

        const COMPUTE_SOURCE: &[u8] = include_bytes!(
            "../../../../../Helio-Examples/cloud-engine-webgpu-linux-aligned/shaders/simulate.wgsl"
        );
        const GRAPHICS_SOURCE: &[u8] = include_bytes!(
            "../../../../../Helio-Examples/cloud-engine-webgpu-linux-aligned/shaders/render.wgsl"
        );
        const RESOURCES: &[u8] = b"sealed logical volume resource contract";
        const COMPUTE_ISA: &[u8] = b"CSIM";
        const VERTEX_ISA: &[u8] = b"VSIM";
        const FRAGMENT_ISA: &[u8] = b"FSIM";

        #[test]
        fn sealed_helioc_package_accepts_simd16_and_simd32_compute_metadata() {
            for simd in [16, 32] {
                let bytes = artifact_fixture(simd, COMPUTE_SOURCE, true);
                let artifact = Artifact::parse(&bytes).expect("valid HELIOA fixture");
                let package = NativePackage::parse_artifact(artifact).expect("sealed HELIOC");
                assert_eq!(package.compute_simd_width(), simd);
                assert_eq!(package.compute_hardware_threads(), if simd == 16 { 4 } else { 2 });
                assert_eq!(package.descriptor().len(), BYTE_LEN);
            }
        }

        #[test]
        fn rejects_graphics_only_package_without_the_compute_isa() {
            let bytes = artifact_fixture(16, COMPUTE_SOURCE, false);
            let artifact = Artifact::parse(&bytes).unwrap();
            assert_eq!(NativePackage::parse_artifact(artifact), Err(Error::MissingSection));
        }

        #[test]
        fn rejects_source_hash_or_compute_geometry_tampering() {
            let bytes = artifact_fixture(16, b"tampered compute source", true);
            let artifact = Artifact::parse(&bytes).unwrap();
            assert_eq!(NativePackage::parse_artifact(artifact), Err(Error::InvalidSource));

            let mut descriptor = descriptor_fixture(16, RESOURCES);
            put_u16(&mut descriptor, 38, 23);
            assert_eq!(NativePackage::parse(&descriptor), Err(Error::InvalidComputeShape));

            let mut consistently_rehashed = descriptor_fixture(16, RESOURCES);
            consistently_rehashed[128..160]
                .copy_from_slice(&Sha256::digest(b"alternate simulate.wgsl"));
            assert_eq!(NativePackage::parse(&consistently_rehashed), Err(Error::InvalidSource));
        }

        #[test]
        fn rejects_xehp_gfx125_label_for_the_xelp_uhd770_target() {
            let mut descriptor = descriptor_fixture(16, RESOURCES);
            put_u16(&mut descriptor, 20, 125);
            assert_eq!(NativePackage::parse(&descriptor), Err(Error::UnsupportedTarget));
        }

        #[test]
        fn rejects_mismatched_payload_and_logical_binding_state() {
            let mut payload = descriptor_fixture(32, RESOURCES);
            put_u16(&mut payload, 46, 96);
            assert_eq!(NativePackage::parse(&payload), Err(Error::InvalidPayload));

            let mut bindings = descriptor_fixture(16, RESOURCES);
            bindings[60 + 4 + 2] = 7;
            assert_eq!(NativePackage::parse(&bindings), Err(Error::InvalidBindings));
        }

        fn descriptor_fixture(compute_simd: u16, resources: &[u8]) -> Vec<u8> {
            let mut descriptor = vec![0u8; BYTE_LEN];
            descriptor[..8].copy_from_slice(&MAGIC);
            put_u16(&mut descriptor, 8, VERSION);
            put_u16(&mut descriptor, 10, BYTE_LEN as u16);
            put_u32(&mut descriptor, 12, BYTE_LEN as u32);
            put_u32(&mut descriptor, 16, 0x0F);
            put_u16(&mut descriptor, 20, GFX_VERSION);
            put_u16(&mut descriptor, 22, ADLS_UHD_770_DEVICE_ID);
            descriptor[24] = ADLS_UHD_770_REVISION;
            descriptor[25] = ADLS_UHD_770_REVISION;
            descriptor[26] = 1;
            put_u16(&mut descriptor, 28, compute_simd);
            put_u16(&mut descriptor, 30, LOCAL_INVOCATIONS / compute_simd);
            for (index, value) in LOCAL_DIMENSIONS.into_iter().enumerate() {
                put_u16(&mut descriptor, 32 + index * 2, value);
            }
            for (index, value) in GROUP_DIMENSIONS.into_iter().enumerate() {
                put_u16(&mut descriptor, 38 + index * 2, value);
            }
            put_u16(&mut descriptor, 44, CROSS_THREAD_BYTES);
            put_u16(&mut descriptor, 46, if compute_simd == 16 { 96 } else { 192 });
            put_u16(&mut descriptor, 48, INDIRECT_BYTES);
            put_u16(&mut descriptor, 50, 16);
            put_u16(&mut descriptor, 52, REQUIRED_BINDINGS.len() as u16);
            put_u32(&mut descriptor, 56, REQUIRED_STATE_FLAGS);
            for (index, binding) in REQUIRED_BINDINGS.into_iter().enumerate() {
                descriptor[60 + index * 4..64 + index * 4].copy_from_slice(&binding);
            }
            for (offset, data) in [
                (96, COMPUTE_SOURCE),
                (100, GRAPHICS_SOURCE),
                (104, resources),
                (108, COMPUTE_ISA),
                (112, VERTEX_ISA),
                (116, FRAGMENT_ISA),
            ] {
                put_u32(&mut descriptor, offset, data.len() as u32);
            }
            descriptor[128..160].copy_from_slice(&CLOUD_SIMULATION_WGSL_SHA256);
            descriptor[160..192].copy_from_slice(&CLOUD_RENDER_WGSL_SHA256);
            for (offset, data) in [
                (192, resources),
                (224, COMPUTE_ISA),
                (256, VERTEX_ISA),
                (288, FRAGMENT_ISA),
            ] {
                descriptor[offset..offset + 32].copy_from_slice(&Sha256::digest(data));
            }
            descriptor
        }

        fn artifact_fixture(
            compute_simd: u16,
            compute_source: &[u8],
            include_compute_isa: bool,
        ) -> Vec<u8> {
            let (resources, volume_metadata) =
                super::super::helio_volume_resources::helioc_test_cloud_resource();
            let descriptor = descriptor_fixture(compute_simd, &resources);
            let mut sections = vec![
                ("manifest.json", SectionKind::Manifest.raw(), b"{}".as_slice()),
                (PACKAGE_SECTION, SectionKind::CompilerMetadata.raw(), descriptor.as_slice()),
                (COMPUTE_SOURCE_SECTION, SectionKind::ShaderSource.raw(), compute_source),
                (GRAPHICS_SOURCE_SECTION, SectionKind::ShaderSource.raw(), GRAPHICS_SOURCE),
                (
                    super::super::helio_volume_resources::COMPILER_METADATA_SECTION_NAME,
                    SectionKind::CompilerMetadata.raw(),
                    volume_metadata,
                ),
                (
                    VOLUME_RESOURCES_SECTION,
                    SectionKind::NormalizedRenderIr.raw(),
                    resources.as_slice(),
                ),
                (VERTEX_ISA_SECTION, SectionKind::IntelXeLpIsa.raw(), VERTEX_ISA),
                (FRAGMENT_ISA_SECTION, SectionKind::IntelXeLpIsa.raw(), FRAGMENT_ISA),
            ];
            if include_compute_isa {
                sections.push((COMPUTE_ISA_SECTION, SectionKind::IntelXeLpIsa.raw(), COMPUTE_ISA));
            }
            helioa(&sections)
        }

        fn helioa(sections: &[(&str, u16, &[u8])]) -> Vec<u8> {
            let mut payload_offset = 32usize;
            for (name, _, _) in sections {
                payload_offset = align_8(payload_offset + 32 + name.len());
            }
            let total = sections
                .iter()
                .fold(payload_offset, |length, (_, _, data)| length + data.len());
            let mut bytes = vec![0u8; total];
            bytes[..8].copy_from_slice(b"HELIOA\0\0");
            put_u16(&mut bytes, 8, 1);
            put_u16(&mut bytes, 10, 32);
            put_u32(&mut bytes, 12, sections.len() as u32);
            put_u64(&mut bytes, 16, (payload_offset - 32) as u64);
            put_u64(&mut bytes, 24, payload_offset as u64);

            let mut toc = 32usize;
            let mut data_offset = payload_offset;
            for (name, kind, data) in sections {
                put_u16(&mut bytes, toc, name.len() as u16);
                put_u16(&mut bytes, toc + 2, *kind);
                put_u64(&mut bytes, toc + 8, data_offset as u64);
                put_u64(&mut bytes, toc + 16, data.len() as u64);
                put_u32(&mut bytes, toc + 24, crc32fast::hash(data));
                bytes[toc + 32..toc + 32 + name.len()].copy_from_slice(name.as_bytes());
                toc = align_8(toc + 32 + name.len());
                bytes[data_offset..data_offset + data.len()].copy_from_slice(data);
                data_offset += data.len();
            }
            bytes
        }

        const fn align_8(value: usize) -> usize {
            (value + 7) & !7
        }
        fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
}
