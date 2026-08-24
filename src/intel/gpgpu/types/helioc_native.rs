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

    pub(crate) const PACKAGE_SECTION: &str = "compiler/helioc-native-volume-raymarch-v3.bin";
    pub(crate) const RELOC_STATE_SECTION: &str = "compiler/helioc-relocatable-state-v2.bin";
    pub(crate) const COMPUTE_SOURCE_SECTION: &str = "authored/cloud-engine/simulate.wgsl";
    pub(crate) const GRAPHICS_SOURCE_SECTION: &str = "authored/cloud-engine/render.wgsl";
    pub(crate) const VOLUME_RESOURCES_SECTION: &str = "resources/volume3d-rgba16f-v1.bin";
    pub(crate) const COMPUTE_ISA_SECTION: &str = "intel-xe-lp/helioc-volume-update.bin";
    pub(crate) const VERTEX_ISA_SECTION: &str = "intel-xe-lp/helioc-volume-raymarch.vs.bin";
    pub(crate) const FRAGMENT_ISA_SECTION: &str = "intel-xe-lp/helioc-volume-raymarch.fs.bin";

    const MAGIC: [u8; 8] = *b"HELIOC\0\0";
    const VERSION: u16 = 3;
    const BYTE_LEN: usize = 384;
    const RELOC_STATE_BYTES_OFFSET: usize = 320;
    const RELOC_STATE_HASH_OFFSET: usize = 324;
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
    const LOGICAL_UNIFORM_READ: u8 = 4;
    const LOGICAL_SAMPLED: u8 = 1;
    const LOGICAL_SAMPLER: u8 = 2;
    const LOGICAL_STORAGE: u8 = 3;
    const REQUIRED_BINDINGS: [[u8; 4]; 7] = [
        // stage, group, binding, logical access
        [1, 0, 0, LOGICAL_UNIFORM_READ], // compute: SimParams
        [1, 0, 1, LOGICAL_SAMPLED],      // compute: sampled source volume
        [1, 0, 2, LOGICAL_SAMPLER],      // compute: normalized cloud sampler
        [1, 0, 3, LOGICAL_STORAGE],      // compute: storage destination volume
        [2, 0, 0, LOGICAL_UNIFORM_READ], // graphics: RenderParams
        [2, 0, 1, LOGICAL_SAMPLED],      // graphics: sampled volume
        [2, 0, 2, LOGICAL_SAMPLER],      // graphics: normalized cloud sampler
    ];
    const FLAG_VERTEX_INDEX: u32 = 1 << 0;
    const FLAG_NO_VERTEX_BUFFER: u32 = 1 << 1;
    const FLAG_NO_DEPTH: u32 = 1 << 2;
    const FLAG_SINGLE_SAMPLE: u32 = 1 << 3;
    const FLAG_PREMULTIPLIED_UI4: u32 = 1 << 4;
    const FLAG_FULLSCREEN_TRIANGLE: u32 = 1 << 5;
    const REQUIRED_GRAPHICS_FLAGS: u32 = FLAG_VERTEX_INDEX
        | FLAG_NO_VERTEX_BUFFER
        | FLAG_NO_DEPTH
        | FLAG_SINGLE_SAMPLE
        | FLAG_PREMULTIPLIED_UI4
        | FLAG_FULLSCREEN_TRIANGLE;
    const RELOC_MAGIC: [u8; 8] = *b"HELIOCRS";
    const RELOC_VERSION: u16 = 2;
    const RELOC_HEADER_BYTES: usize = 128;
    const RELOC_OBJECT_BYTES: usize = 64;
    const RELOC_ENTRY_BYTES: usize = 32;
    const RELOC_MAX_OBJECTS: usize = 64;
    const RELOC_MAX_ENTRIES: usize = 512;
    const RELOC_FLAGS: u32 = 0x0F;
    const RELOC_MAX_BYTES: usize = 0x70_000;
    const RELOC_WINDOW_BATCH: u8 = 1;
    const RELOC_WINDOW_SURFACE: u8 = 2;
    const RELOC_WINDOW_DYNAMIC: u8 = 3;
    const RELOC_WINDOW_INDIRECT: u8 = 4;
    const RELOC_KIND_BATCH: u8 = 1;
    const RELOC_KIND_SURFACE: u8 = 2;
    const RELOC_KIND_SAMPLER: u8 = 3;
    const RELOC_KIND_BINDING: u8 = 4;
    const RELOC_KIND_PROGRAM: u8 = 5;
    const RELOC_KIND_INDIRECT: u8 = 6;
    const RELOC_VALUE_OBJECT_OFFSET: u8 = 1;
    const RELOC_VALUE_OBJECT_GPU: u8 = 2;
    const RELOC_VALUE_FIXED_GPU: u8 = 3;
    const RELOC_VALUE_RUNTIME_GPU: u8 = 4;
    const RELOC_VALUE_RUNTIME_U32: u8 = 5;
    const SYMBOL_CS: u16 = 1;
    const SYMBOL_VS: u16 = 2;
    const SYMBOL_FS: u16 = 3;
    const SYMBOL_SURFACE: u16 = 4;
    const SYMBOL_DYNAMIC: u16 = 5;
    const SYMBOL_INDIRECT: u16 = 6;
    const SYMBOL_BATCH: u16 = 7;
    const SYMBOL_RESULT: u16 = 8;
    const SYMBOL_VOLUME_A: u16 = 9;
    const SYMBOL_VOLUME_B: u16 = 10;
    const SYMBOL_SIM_PARAMS: u16 = 11;
    const SYMBOL_RENDER_PARAMS: u16 = 12;
    const SYMBOL_UI4_PRODUCER_GPU: u16 = 13;
    const SYMBOL_WIDTH: u16 = 14;
    const SYMBOL_HEIGHT: u16 = 15;
    const SYMBOL_PITCH: u16 = 16;
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
        InvalidRelocState,
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) struct RelocatableState<'a> {
        bytes: &'a [u8],
        object_count: u16,
        reloc_count: u16,
        object_offset: usize,
        reloc_offset: usize,
        data_offset: usize,
        data_bytes: usize,
    }

    /// The four fixed GPU windows owned by the trusted HelioC encoder.
    ///
    /// These are caller-owned storage; the package never supplies a pointer
    /// or a GPU address.  The exact window sizes are checked before anything
    /// is copied or patched.
    pub(crate) struct HelioCloudMaterializationWindows<'a> {
        pub(crate) batch: &'a mut [u8],
        pub(crate) surface: &'a mut [u8],
        pub(crate) dynamic: &'a mut [u8],
        pub(crate) indirect: &'a mut [u8],
        pub(crate) batch_gpu: u64,
        pub(crate) surface_gpu: u64,
        pub(crate) dynamic_gpu: u64,
        pub(crate) indirect_gpu: u64,
    }

    /// Sealed fixed symbols plus broker-owned dynamic values.  The native
    /// package can name these symbols but cannot provide their addresses.
    #[derive(Copy, Clone)]
    pub(crate) struct HelioCloudMaterializationSymbols {
        pub(crate) cs: u64,
        pub(crate) vs: u64,
        pub(crate) fs: u64,
        pub(crate) surface: u64,
        pub(crate) dynamic: u64,
        pub(crate) indirect: u64,
        pub(crate) batch: u64,
        pub(crate) result: u64,
        pub(crate) volume_a: u64,
        pub(crate) volume_b: u64,
        pub(crate) sim_params: u64,
        pub(crate) render_params: u64,
        pub(crate) ui4_producer_gpu: u64,
        pub(crate) width: u32,
        pub(crate) height: u32,
        pub(crate) pitch: u32,
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) struct HelioCloudMaterializedBatch {
        pub(crate) offset: u32,
        pub(crate) length: u32,
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum MaterializationError {
        InvalidState,
        WindowSize,
        Overflow,
        AddressRange,
        ValueDoesNotFit,
    }

    impl<'a> RelocatableState<'a> {
        pub(crate) const fn object_count(self) -> u16 {
            self.object_count
        }

        pub(crate) const fn reloc_count(self) -> u16 {
            self.reloc_count
        }

        pub(crate) fn object_record(self, index: usize) -> Option<&'a [u8]> {
            if index >= usize::from(self.object_count) {
                return None;
            }
            let start = self
                .object_offset
                .checked_add(index.checked_mul(RELOC_OBJECT_BYTES)?)?;
            self.bytes.get(start..start + RELOC_OBJECT_BYTES)
        }

        pub(crate) fn reloc_record(self, index: usize) -> Option<&'a [u8]> {
            if index >= usize::from(self.reloc_count) {
                return None;
            }
            let start = self
                .reloc_offset
                .checked_add(index.checked_mul(RELOC_ENTRY_BYTES)?)?;
            self.bytes.get(start..start + RELOC_ENTRY_BYTES)
        }

        pub(crate) fn data(self) -> &'a [u8] {
            &self.bytes[self.data_offset..self.data_offset + self.data_bytes]
        }
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) struct NativePackage<'a> {
        descriptor: &'a [u8],
        compute_isa: Option<&'a [u8]>,
        vertex_isa: Option<&'a [u8]>,
        fragment_isa: Option<&'a [u8]>,
        reloc_state: Option<RelocatableState<'a>>,
        compute_simd_width: u16,
        vertex_simd_width: u16,
        fragment_simd_width: u16,
        sim_params_bytes: u16,
        render_params_bytes: u16,
        draw_vertex_count: u16,
    }

    impl<'a> NativePackage<'a> {
        pub(crate) fn parse_artifact(artifact: Artifact<'a>) -> Result<Self, Error> {
            let descriptor = required(artifact, PACKAGE_SECTION, SectionKind::CompilerMetadata)?;
            let mut package = Self::parse(descriptor)?;

            let compute_source =
                required(artifact, COMPUTE_SOURCE_SECTION, SectionKind::ShaderSource)?;
            let graphics_source =
                required(artifact, GRAPHICS_SOURCE_SECTION, SectionKind::ShaderSource)?;
            let resources =
                required(artifact, VOLUME_RESOURCES_SECTION, SectionKind::NormalizedRenderIr)?;
            let compute_isa = required(artifact, COMPUTE_ISA_SECTION, SectionKind::IntelXeLpIsa)?;
            let vertex_isa = required(artifact, VERTEX_ISA_SECTION, SectionKind::IntelXeLpIsa)?;
            let fragment_isa = required(artifact, FRAGMENT_ISA_SECTION, SectionKind::IntelXeLpIsa)?;
            let reloc_bytes =
                required(artifact, RELOC_STATE_SECTION, SectionKind::CompilerMetadata)?;

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
            verify_reference(
                reloc_bytes,
                package.reloc_state_bytes(),
                package.reloc_state_hash(),
                Error::InvalidRelocState,
            )?;
            let reloc_state = parse_relocatable_state(reloc_bytes)?;
            let volume_program = super::helio_volume_resources::Program::parse_artifact(artifact)
                .map_err(|_| Error::InvalidResourceContract)?;
            if !volume_program.is_helio_cloud_profile() {
                return Err(Error::InvalidResourceContract);
            }
            // Only the artifact parser can populate executable views: it has
            // authenticated both HELIOA section identity/kind and the exact
            // descriptor-bound lengths and hashes above. Descriptor-only
            // parsing deliberately leaves these unavailable.
            package.compute_isa = Some(compute_isa);
            package.vertex_isa = Some(vertex_isa);
            package.fragment_isa = Some(fragment_isa);
            package.reloc_state = Some(reloc_state);
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
            if read_u16(descriptor, 50) != Some(8)
                || read_u16(descriptor, 52) != Some(16)
                || read_u16(descriptor, 54) != Some(REQUIRED_BINDINGS.len() as u16)
                || read_u16(descriptor, 88) != Some(112)
                || read_u16(descriptor, 90) != Some(272)
                || read_u16(descriptor, 92) != Some(3)
                || read_u16(descriptor, 94) != Some(REQUIRED_GRAPHICS_FLAGS as u16)
            {
                return Err(Error::InvalidBindings);
            }
            for (index, expected) in REQUIRED_BINDINGS.into_iter().enumerate() {
                let offset = 60 + index * 4;
                if descriptor[offset..offset + 4] != expected {
                    return Err(Error::InvalidBindings);
                }
            }
            if descriptor[56..60].iter().any(|byte| *byte != 0)
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
            let reloc_bytes = read_u32(descriptor, RELOC_STATE_BYTES_OFFSET).unwrap_or(0) as usize;
            if descriptor[356..384].iter().any(|byte| *byte != 0)
                || !(RELOC_HEADER_BYTES..=RELOC_MAX_BYTES).contains(&reloc_bytes)
                || descriptor[RELOC_STATE_HASH_OFFSET..RELOC_STATE_HASH_OFFSET + 32]
                    .iter()
                    .all(|byte| *byte == 0)
            {
                return Err(Error::InvalidDescriptor);
            }
            Ok(Self {
                descriptor,
                compute_isa: None,
                vertex_isa: None,
                fragment_isa: None,
                reloc_state: None,
                compute_simd_width,
                vertex_simd_width: 8,
                fragment_simd_width: 16,
                sim_params_bytes: 112,
                render_params_bytes: 272,
                draw_vertex_count: 3,
            })
        }

        pub(crate) const fn compute_simd_width(self) -> u16 {
            self.compute_simd_width
        }

        pub(crate) const fn compute_hardware_threads(self) -> u16 {
            LOCAL_INVOCATIONS / self.compute_simd_width
        }

        pub(crate) const fn vertex_simd_width(self) -> u16 {
            self.vertex_simd_width
        }

        pub(crate) const fn fragment_simd_width(self) -> u16 {
            self.fragment_simd_width
        }

        pub(crate) const fn sim_params_bytes(self) -> u16 {
            self.sim_params_bytes
        }

        pub(crate) const fn render_params_bytes(self) -> u16 {
            self.render_params_bytes
        }

        pub(crate) const fn draw_vertex_count(self) -> u16 {
            self.draw_vertex_count
        }

        pub(crate) const fn descriptor(self) -> &'a [u8] {
            self.descriptor
        }

        pub(crate) const fn compute_isa(self) -> Option<&'a [u8]> {
            self.compute_isa
        }

        pub(crate) const fn vertex_isa(self) -> Option<&'a [u8]> {
            self.vertex_isa
        }

        pub(crate) const fn fragment_isa(self) -> Option<&'a [u8]> {
            self.fragment_isa
        }

        pub(crate) const fn reloc_state(self) -> Option<RelocatableState<'a>> {
            self.reloc_state
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
        fn reloc_state_bytes(self) -> usize {
            read_u32(self.descriptor, RELOC_STATE_BYTES_OFFSET).unwrap_or(0) as usize
        }
        fn reloc_state_hash(self) -> [u8; 32] {
            self.hash_at(RELOC_STATE_HASH_OFFSET)
        }
        fn hash_at(self, offset: usize) -> [u8; 32] {
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&self.descriptor[offset..offset + 32]);
            hash
        }
    }

    fn parse_relocatable_state(bytes: &[u8]) -> Result<RelocatableState<'_>, Error> {
        if bytes.len() > RELOC_MAX_BYTES
            || bytes.len() < RELOC_HEADER_BYTES
            || bytes[..8] != RELOC_MAGIC
            || read_u16(bytes, 8) != Some(RELOC_VERSION)
            || read_u16(bytes, 10) != Some(RELOC_HEADER_BYTES as u16)
            || read_u16(bytes, 16) != Some(120)
            || read_u16(bytes, 18) != Some(0x4680)
            || bytes[20] != 0x0c
            || bytes[21] != 1
            || bytes[22] != 64
            || bytes[23] != 0
            || read_u16(bytes, 24) != Some(RELOC_OBJECT_BYTES as u16)
            || read_u16(bytes, 26) != Some(RELOC_ENTRY_BYTES as u16)
            || read_u32(bytes, 48) != Some(RELOC_FLAGS)
            || bytes[52] != 6
            || bytes[53] != 2
            || bytes[54..56].iter().any(|byte| *byte != 0)
            || bytes[64..128].iter().any(|byte| *byte != 0)
        {
            return Err(Error::InvalidRelocState);
        }
        let total = read_u32(bytes, 12).ok_or(Error::InvalidRelocState)? as usize;
        let object_count = read_u16(bytes, 28).ok_or(Error::InvalidRelocState)?;
        let reloc_count = read_u16(bytes, 30).ok_or(Error::InvalidRelocState)?;
        if total != bytes.len()
            || object_count == 0
            || usize::from(object_count) > RELOC_MAX_OBJECTS
            || usize::from(reloc_count) > RELOC_MAX_ENTRIES
        {
            return Err(Error::InvalidRelocState);
        }
        let object_offset = read_u32(bytes, 32).ok_or(Error::InvalidRelocState)? as usize;
        let reloc_offset = read_u32(bytes, 36).ok_or(Error::InvalidRelocState)? as usize;
        let data_offset = read_u32(bytes, 40).ok_or(Error::InvalidRelocState)? as usize;
        let data_bytes = read_u32(bytes, 44).ok_or(Error::InvalidRelocState)? as usize;
        let object_table_bytes = read_u32(bytes, 56).ok_or(Error::InvalidRelocState)? as usize;
        let reloc_table_bytes = read_u32(bytes, 60).ok_or(Error::InvalidRelocState)? as usize;
        let object_end = object_offset
            .checked_add(usize::from(object_count) * RELOC_OBJECT_BYTES)
            .ok_or(Error::InvalidRelocState)?;
        let reloc_end = reloc_offset
            .checked_add(usize::from(reloc_count) * RELOC_ENTRY_BYTES)
            .ok_or(Error::InvalidRelocState)?;
        if object_table_bytes != usize::from(object_count) * RELOC_OBJECT_BYTES
            || reloc_table_bytes != usize::from(reloc_count) * RELOC_ENTRY_BYTES
            || object_offset != RELOC_HEADER_BYTES
            || reloc_offset != object_end
            || data_offset < reloc_end
            || data_offset != (reloc_end + 63) & !63
            || data_offset % 64 != 0
            || data_offset.checked_add(data_bytes) != Some(total)
            || bytes[reloc_end..data_offset].iter().any(|byte| *byte != 0)
        {
            return Err(Error::InvalidRelocState);
        }

        for index in 0..usize::from(object_count) {
            let offset = object_offset + index * RELOC_OBJECT_BYTES;
            let id = read_u16(bytes, offset).ok_or(Error::InvalidRelocState)?;
            let window = *bytes.get(offset + 2).ok_or(Error::InvalidRelocState)?;
            let kind = *bytes.get(offset + 3).ok_or(Error::InvalidRelocState)?;
            let semantic = read_u16(bytes, offset + 4).ok_or(Error::InvalidRelocState)?;
            let variant = *bytes.get(offset + 6).ok_or(Error::InvalidRelocState)?;
            let flags = *bytes.get(offset + 7).ok_or(Error::InvalidRelocState)?;
            let dst_off = read_u32(bytes, offset + 8).ok_or(Error::InvalidRelocState)? as usize;
            let data_rel = read_u32(bytes, offset + 12).ok_or(Error::InvalidRelocState)? as usize;
            let object_bytes =
                read_u32(bytes, offset + 16).ok_or(Error::InvalidRelocState)? as usize;
            let alignment = read_u16(bytes, offset + 20).ok_or(Error::InvalidRelocState)? as usize;
            let reloc_first =
                read_u16(bytes, offset + 22).ok_or(Error::InvalidRelocState)? as usize;
            let reloc_len = read_u16(bytes, offset + 24).ok_or(Error::InvalidRelocState)? as usize;
            if id == 0
                || semantic == 0
                || !matches!(
                    window,
                    RELOC_WINDOW_BATCH
                        | RELOC_WINDOW_SURFACE
                        | RELOC_WINDOW_DYNAMIC
                        | RELOC_WINDOW_INDIRECT
                )
                || !matches!(
                    kind,
                    RELOC_KIND_BATCH
                        | RELOC_KIND_SURFACE
                        | RELOC_KIND_SAMPLER
                        | RELOC_KIND_BINDING
                        | RELOC_KIND_PROGRAM
                        | RELOC_KIND_INDIRECT
                )
                || flags != 0
                || object_bytes == 0
                || alignment == 0
                || alignment > 4096
                || !alignment.is_power_of_two()
                || ((kind == RELOC_KIND_BATCH
                    || kind == RELOC_KIND_BINDING
                    || kind == RELOC_KIND_INDIRECT)
                    && alignment < 4)
                || bytes[offset + 60..offset + 64]
                    .iter()
                    .any(|byte| *byte != 0)
                || (variant != 0xff && window != RELOC_WINDOW_BATCH)
                || (window == RELOC_WINDOW_BATCH && variant > 5)
                || (kind == RELOC_KIND_BATCH && window != RELOC_WINDOW_BATCH)
                || (kind == RELOC_KIND_SURFACE && !matches!(window, RELOC_WINDOW_SURFACE))
                || (kind == RELOC_KIND_BINDING && window != RELOC_WINDOW_SURFACE)
                || (kind == RELOC_KIND_SAMPLER && window != RELOC_WINDOW_DYNAMIC)
                || (kind == RELOC_KIND_PROGRAM && window != RELOC_WINDOW_DYNAMIC)
                || (kind == RELOC_KIND_INDIRECT && window != RELOC_WINDOW_INDIRECT)
                || data_rel % alignment != 0
                || data_rel
                    .checked_add(object_bytes)
                    .is_none_or(|end| end > data_bytes)
            {
                return Err(Error::InvalidRelocState);
            }
            let window_bytes = if window == RELOC_WINDOW_BATCH {
                256 * 1024
            } else {
                64 * 1024
            };
            if dst_off % alignment != 0
                || dst_off
                    .checked_add(object_bytes)
                    .is_none_or(|end| end > window_bytes)
            {
                return Err(Error::InvalidRelocState);
            }
            if bytes[offset + 28..offset + 60]
                .iter()
                .all(|byte| *byte == 0)
            {
                return Err(Error::InvalidRelocState);
            }
            let data = &bytes[data_offset + data_rel..data_offset + data_rel + object_bytes];
            if Sha256::digest(data)[..] != bytes[offset + 28..offset + 60] {
                return Err(Error::InvalidRelocState);
            }
            for prior in 0..index {
                let prior_offset = object_offset + prior * RELOC_OBJECT_BYTES;
                let prior_data = read_u32(bytes, prior_offset + 12).unwrap_or(u32::MAX) as usize;
                let prior_bytes = read_u32(bytes, prior_offset + 16).unwrap_or(0) as usize;
                if data_rel < prior_data.saturating_add(prior_bytes)
                    && prior_data < data_rel.saturating_add(object_bytes)
                {
                    return Err(Error::InvalidRelocState);
                }
            }
            for prior in 0..index {
                let prior_offset = object_offset + prior * RELOC_OBJECT_BYTES;
                if read_u16(bytes, prior_offset) == Some(id) {
                    return Err(Error::InvalidRelocState);
                }
                let prior_window = bytes[prior_offset + 2];
                let prior_dst = read_u32(bytes, prior_offset + 8).unwrap_or(u32::MAX) as usize;
                let prior_bytes = read_u32(bytes, prior_offset + 16).unwrap_or(0) as usize;
                if prior_window == window
                    && dst_off < prior_dst.saturating_add(prior_bytes)
                    && prior_dst < dst_off.saturating_add(object_bytes)
                {
                    return Err(Error::InvalidRelocState);
                }
            }
            if reloc_first
                .checked_add(reloc_len)
                .is_none_or(|end| end > usize::from(reloc_count))
            {
                return Err(Error::InvalidRelocState);
            }
        }
        let mut batch_variants = [false; 6];
        for index in 0..usize::from(object_count) {
            let offset = object_offset + index * RELOC_OBJECT_BYTES;
            let window = bytes[offset + 2];
            let kind = bytes[offset + 3];
            let variant = bytes[offset + 6];
            let semantic = read_u16(bytes, offset + 4).unwrap_or(0);
            if kind == RELOC_KIND_BATCH {
                if variant > 5 || batch_variants[variant as usize] {
                    return Err(Error::InvalidRelocState);
                }
                batch_variants[variant as usize] = true;
            }
            for prior in 0..index {
                let prior_offset = object_offset + prior * RELOC_OBJECT_BYTES;
                if read_u16(bytes, prior_offset + 4) == Some(semantic)
                    && bytes[prior_offset + 2] == window
                {
                    return Err(Error::InvalidRelocState);
                }
            }
        }
        if batch_variants.iter().any(|present| !present) {
            return Err(Error::InvalidRelocState);
        }
        let mut grouped_relocations = 0usize;
        for left in 0..usize::from(object_count) {
            let left_offset = object_offset + left * RELOC_OBJECT_BYTES;
            let left_first = read_u16(bytes, left_offset + 22).unwrap_or(u16::MAX) as usize;
            let left_count = read_u16(bytes, left_offset + 24).unwrap_or(u16::MAX) as usize;
            grouped_relocations = grouped_relocations.saturating_add(left_count);
            for right in 0..left {
                let right_offset = object_offset + right * RELOC_OBJECT_BYTES;
                let right_first = read_u16(bytes, right_offset + 22).unwrap_or(u16::MAX) as usize;
                let right_count = read_u16(bytes, right_offset + 24).unwrap_or(u16::MAX) as usize;
                if left_first < right_first.saturating_add(right_count)
                    && right_first < left_first.saturating_add(left_count)
                {
                    return Err(Error::InvalidRelocState);
                }
            }
        }
        if grouped_relocations != usize::from(reloc_count) {
            return Err(Error::InvalidRelocState);
        }
        let mut previous: Option<(u16, u32, u64)> = None;
        for index in 0..usize::from(reloc_count) {
            let offset = reloc_offset + index * RELOC_ENTRY_BYTES;
            let target = read_u16(bytes, offset).ok_or(Error::InvalidRelocState)?;
            let source = read_u16(bytes, offset + 2).ok_or(Error::InvalidRelocState)?;
            let target_off = read_u32(bytes, offset + 4).ok_or(Error::InvalidRelocState)?;
            let source_off = read_u32(bytes, offset + 8).ok_or(Error::InvalidRelocState)?;
            let width = bytes[offset + 12];
            let value_kind = bytes[offset + 13];
            let shift = bytes[offset + 14];
            let flags = bytes[offset + 15];
            let mask = u64::from_le_bytes(bytes[offset + 16..offset + 24].try_into().unwrap());
            let addend = i64::from_le_bytes(bytes[offset + 24..offset + 32].try_into().unwrap());
            if !matches!(width, 4 | 8)
                || shift >= 64
                || flags != 0
                || mask == 0
                || !relocation_mask_is_contiguous(mask)
                || (width == 4 && mask >> 32 != 0)
                || !target_off.is_multiple_of(4)
                || !matches!(value_kind, 1..=5)
            {
                return Err(Error::InvalidRelocState);
            }
            let target_index = (0..usize::from(object_count))
                .find(|object_index| {
                    read_u16(bytes, object_offset + object_index * RELOC_OBJECT_BYTES)
                        == Some(target)
                })
                .ok_or(Error::InvalidRelocState)?;
            let target_base = object_offset + target_index * RELOC_OBJECT_BYTES;
            let target_bytes = read_u32(bytes, target_base + 16).ok_or(Error::InvalidRelocState)?;
            let first = read_u16(bytes, target_base + 22).ok_or(Error::InvalidRelocState)?;
            let count = read_u16(bytes, target_base + 24).ok_or(Error::InvalidRelocState)?;
            if index < usize::from(first)
                || index >= usize::from(first) + usize::from(count)
                || u64::from(target_off) + u64::from(width) > u64::from(target_bytes)
            {
                return Err(Error::InvalidRelocState);
            }
            if matches!(value_kind, RELOC_VALUE_OBJECT_OFFSET | RELOC_VALUE_OBJECT_GPU) {
                let source_base = (0..usize::from(object_count))
                    .find(|object_index| {
                        read_u16(bytes, object_offset + object_index * RELOC_OBJECT_BYTES)
                            == Some(source)
                    })
                    .ok_or(Error::InvalidRelocState)?;
                let source_bytes =
                    read_u32(bytes, object_offset + source_base * RELOC_OBJECT_BYTES + 16)
                        .unwrap_or(0);
                if u64::from(source_off) >= u64::from(source_bytes) {
                    return Err(Error::InvalidRelocState);
                }
            } else if value_kind == RELOC_VALUE_FIXED_GPU {
                if !matches!(source, SYMBOL_CS..=SYMBOL_RENDER_PARAMS) || source_off != 0 {
                    return Err(Error::InvalidRelocState);
                }
            } else if value_kind == RELOC_VALUE_RUNTIME_GPU {
                if source != SYMBOL_UI4_PRODUCER_GPU || source_off != 0 || addend != 0 {
                    return Err(Error::InvalidRelocState);
                }
            } else if value_kind == RELOC_VALUE_RUNTIME_U32 {
                if !matches!(source, SYMBOL_WIDTH..=SYMBOL_PITCH) || source_off != 0 {
                    return Err(Error::InvalidRelocState);
                }
                if addend != 0 && addend != -1 {
                    return Err(Error::InvalidRelocState);
                }
            } else {
                return Err(Error::InvalidRelocState);
            }
            if addend < i64::from(i32::MIN) || addend > i64::from(i32::MAX) {
                return Err(Error::InvalidRelocState);
            }
            for prior_index in 0..index {
                let prior_offset = reloc_offset + prior_index * RELOC_ENTRY_BYTES;
                if read_u16(bytes, prior_offset) != Some(target) {
                    continue;
                }
                let prior_target = read_u32(bytes, prior_offset + 4).unwrap_or(u32::MAX);
                let prior_width = bytes[prior_offset + 12];
                let prior_mask = u64::from_le_bytes(
                    bytes[prior_offset + 16..prior_offset + 24]
                        .try_into()
                        .unwrap(),
                );
                let byte_overlap = target_off < prior_target.saturating_add(u32::from(prior_width))
                    && prior_target < target_off.saturating_add(u32::from(width));
                if byte_overlap
                    && (target_off != prior_target
                        || width != prior_width
                        || prior_mask & mask != 0)
                {
                    return Err(Error::InvalidRelocState);
                }
            }
            let key = (target, target_off, mask);
            if previous.is_some_and(|old| old > key) {
                return Err(Error::InvalidRelocState);
            }
            if previous
                .is_some_and(|old| old.0 == target && old.1 == target_off && old.2 & mask != 0)
            {
                return Err(Error::InvalidRelocState);
            }
            previous = Some(key);
        }
        for index in 0..usize::from(object_count) {
            let offset = object_offset + index * RELOC_OBJECT_BYTES;
            let first = read_u16(bytes, offset + 22).unwrap_or(u16::MAX) as usize;
            let count = read_u16(bytes, offset + 24).unwrap_or(u16::MAX) as usize;
            if first > usize::from(reloc_count) || first + count > usize::from(reloc_count) {
                return Err(Error::InvalidRelocState);
            }
            if bytes[offset + 2] == RELOC_WINDOW_BATCH && bytes[offset + 6] == 0xff {
                return Err(Error::InvalidRelocState);
            }
        }
        Ok(RelocatableState {
            bytes,
            object_count,
            reloc_count,
            object_offset,
            reloc_offset,
            data_offset,
            data_bytes,
        })
    }

    #[derive(Copy, Clone)]
    struct ObjectInfo {
        id: u16,
        window: u8,
        variant: u8,
        dst: u32,
        bytes: u32,
    }

    fn object_info(state: RelocatableState<'_>, index: usize) -> Option<ObjectInfo> {
        let record = state.object_record(index)?;
        Some(ObjectInfo {
            id: u16::from_le_bytes(record[0..2].try_into().ok()?),
            window: record[2],
            variant: record[6],
            dst: u32::from_le_bytes(record[8..12].try_into().ok()?),
            bytes: u32::from_le_bytes(record[16..20].try_into().ok()?),
        })
    }

    fn find_object(state: RelocatableState<'_>, id: u16) -> Option<ObjectInfo> {
        (0..usize::from(state.object_count()))
            .filter_map(|index| object_info(state, index))
            .find(|object| object.id == id)
    }

    fn window_len(window: u8) -> usize {
        match window {
            RELOC_WINDOW_BATCH => 256 * 1024,
            RELOC_WINDOW_SURFACE | RELOC_WINDOW_DYNAMIC | RELOC_WINDOW_INDIRECT => 64 * 1024,
            _ => 0,
        }
    }

    const fn relocation_mask_is_contiguous(mask: u64) -> bool {
        mask != 0 && {
            let normalized = mask >> mask.trailing_zeros();
            normalized & normalized.wrapping_add(1) == 0
        }
    }

    fn window_gpu(windows: &HelioCloudMaterializationWindows<'_>, window: u8) -> Option<u64> {
        Some(match window {
            RELOC_WINDOW_BATCH => windows.batch_gpu,
            RELOC_WINDOW_SURFACE => windows.surface_gpu,
            RELOC_WINDOW_DYNAMIC => windows.dynamic_gpu,
            RELOC_WINDOW_INDIRECT => windows.indirect_gpu,
            _ => return None,
        })
    }

    fn object_value(
        object: ObjectInfo,
        source_off: u32,
        value_kind: u8,
        windows: &HelioCloudMaterializationWindows<'_>,
    ) -> Result<u64, MaterializationError> {
        let offset = u64::from(object.dst)
            .checked_add(u64::from(source_off))
            .ok_or(MaterializationError::Overflow)?;
        match value_kind {
            RELOC_VALUE_OBJECT_OFFSET => Ok(offset),
            RELOC_VALUE_OBJECT_GPU => window_gpu(windows, object.window)
                .and_then(|base| base.checked_add(offset))
                .ok_or(MaterializationError::AddressRange),
            _ => Err(MaterializationError::InvalidState),
        }
    }

    fn fixed_symbol(
        source: u16,
        value_kind: u8,
        symbols: HelioCloudMaterializationSymbols,
    ) -> Result<u64, MaterializationError> {
        if value_kind == RELOC_VALUE_RUNTIME_GPU {
            return (source == SYMBOL_UI4_PRODUCER_GPU)
                .then_some(symbols.ui4_producer_gpu)
                .ok_or(MaterializationError::InvalidState);
        }
        if value_kind == RELOC_VALUE_RUNTIME_U32 {
            return match source {
                SYMBOL_WIDTH => Ok(u64::from(symbols.width)),
                SYMBOL_HEIGHT => Ok(u64::from(symbols.height)),
                SYMBOL_PITCH => Ok(u64::from(symbols.pitch)),
                _ => Err(MaterializationError::InvalidState),
            };
        }
        if value_kind != RELOC_VALUE_FIXED_GPU {
            return Err(MaterializationError::InvalidState);
        }
        match source {
            SYMBOL_CS => Ok(symbols.cs),
            SYMBOL_VS => Ok(symbols.vs),
            SYMBOL_FS => Ok(symbols.fs),
            SYMBOL_SURFACE => Ok(symbols.surface),
            SYMBOL_DYNAMIC => Ok(symbols.dynamic),
            SYMBOL_INDIRECT => Ok(symbols.indirect),
            SYMBOL_BATCH => Ok(symbols.batch),
            SYMBOL_RESULT => Ok(symbols.result),
            SYMBOL_VOLUME_A => Ok(symbols.volume_a),
            SYMBOL_VOLUME_B => Ok(symbols.volume_b),
            SYMBOL_SIM_PARAMS => Ok(symbols.sim_params),
            SYMBOL_RENDER_PARAMS => Ok(symbols.render_params),
            _ => Err(MaterializationError::InvalidState),
        }
    }

    fn target_window<'a>(
        windows: &'a mut HelioCloudMaterializationWindows<'_>,
        window: u8,
    ) -> Option<&'a mut [u8]> {
        match window {
            RELOC_WINDOW_BATCH => Some(windows.batch),
            RELOC_WINDOW_SURFACE => Some(windows.surface),
            RELOC_WINDOW_DYNAMIC => Some(windows.dynamic),
            RELOC_WINDOW_INDIRECT => Some(windows.indirect),
            _ => None,
        }
    }

    /// Materialize authenticated templates into trusted fixed windows and
    /// apply the typed relocations. This performs no allocation and accepts
    /// no package-provided GPU address.
    pub(crate) fn materialize_relocatable_state(
        state: RelocatableState<'_>,
        windows: &mut HelioCloudMaterializationWindows<'_>,
        symbols: HelioCloudMaterializationSymbols,
        variant: u8,
    ) -> Result<HelioCloudMaterializedBatch, MaterializationError> {
        if variant > 5
            || windows.batch.len() != window_len(RELOC_WINDOW_BATCH)
            || windows.surface.len() != window_len(RELOC_WINDOW_SURFACE)
            || windows.dynamic.len() != window_len(RELOC_WINDOW_DYNAMIC)
            || windows.indirect.len() != window_len(RELOC_WINDOW_INDIRECT)
        {
            return Err(MaterializationError::WindowSize);
        }
        if symbols.batch != windows.batch_gpu
            || symbols.surface != windows.surface_gpu
            || symbols.dynamic != windows.dynamic_gpu
            || symbols.indirect != windows.indirect_gpu
        {
            return Err(MaterializationError::AddressRange);
        }

        // Copy source data before taking mutable window borrows for patches.
        for index in 0..usize::from(state.object_count()) {
            let object = object_info(state, index).ok_or(MaterializationError::InvalidState)?;
            let record = state
                .object_record(index)
                .ok_or(MaterializationError::InvalidState)?;
            let data_rel = u32::from_le_bytes(
                record[12..16]
                    .try_into()
                    .map_err(|_| MaterializationError::InvalidState)?,
            ) as usize;
            let start = data_rel
                .checked_add(0)
                .and_then(|value| state.data().get(value..))
                .and_then(|data| data.get(..usize::try_from(object.bytes).ok()?))
                .ok_or(MaterializationError::InvalidState)?;
            let end = usize::try_from(object.dst)
                .ok()
                .and_then(|dst| dst.checked_add(start.len()))
                .ok_or(MaterializationError::Overflow)?;
            if end > window_len(object.window) {
                return Err(MaterializationError::AddressRange);
            }
            let target =
                target_window(windows, object.window).ok_or(MaterializationError::InvalidState)?;
            target[usize::try_from(object.dst).map_err(|_| MaterializationError::Overflow)?..end]
                .copy_from_slice(start);
        }

        for index in 0..usize::from(state.reloc_count()) {
            let record = state
                .reloc_record(index)
                .ok_or(MaterializationError::InvalidState)?;
            let target_id = u16::from_le_bytes(record[0..2].try_into().unwrap());
            let source_id = u16::from_le_bytes(record[2..4].try_into().unwrap());
            let target_off = u32::from_le_bytes(record[4..8].try_into().unwrap());
            let source_off = u32::from_le_bytes(record[8..12].try_into().unwrap());
            let width = record[12];
            let value_kind = record[13];
            let shift = record[14];
            let mask = u64::from_le_bytes(record[16..24].try_into().unwrap());
            let addend = i64::from_le_bytes(record[24..32].try_into().unwrap());
            let target_object =
                find_object(state, target_id).ok_or(MaterializationError::InvalidState)?;
            let target_end = u64::from(target_off)
                .checked_add(u64::from(width))
                .ok_or(MaterializationError::Overflow)?;
            if !matches!(width, 4 | 8)
                || shift >= 64
                || mask == 0
                || !relocation_mask_is_contiguous(mask)
                || (width == 4 && mask >> 32 != 0)
                || !target_off.is_multiple_of(4)
                || target_end > u64::from(target_object.bytes)
            {
                return Err(MaterializationError::InvalidState);
            }
            let raw = if matches!(value_kind, RELOC_VALUE_OBJECT_OFFSET | RELOC_VALUE_OBJECT_GPU) {
                let source =
                    find_object(state, source_id).ok_or(MaterializationError::InvalidState)?;
                if source_off >= source.bytes {
                    return Err(MaterializationError::AddressRange);
                }
                object_value(source, source_off, value_kind, windows)?
            } else {
                fixed_symbol(source_id, value_kind, symbols)?
            };
            let adjusted = if addend >= 0 {
                raw.checked_add(addend as u64)
            } else {
                raw.checked_sub(addend.unsigned_abs())
            }
            .ok_or(MaterializationError::Overflow)?;
            // A relocation's right shift first converts the resolved semantic
            // value to the hardware field's unit. The contiguous mask then
            // supplies the destination bit position. This represents both
            // address-unit fields (for example 4 KiB SBA units) and nonzero-
            // based scalar fields such as width-minus-one without accepting a
            // package-provided, pre-shifted runtime value.
            let unpositioned = adjusted
                .checked_shr(u32::from(shift))
                .ok_or(MaterializationError::Overflow)?;
            let field_shift = mask.trailing_zeros();
            let field_capacity = mask >> field_shift;
            if unpositioned & !field_capacity != 0 {
                return Err(MaterializationError::ValueDoesNotFit);
            }
            let positioned = unpositioned
                .checked_shl(field_shift)
                .ok_or(MaterializationError::Overflow)?;
            let begin = target_object
                .dst
                .checked_add(target_off)
                .and_then(|offset| usize::try_from(offset).ok())
                .ok_or(MaterializationError::Overflow)?;
            let target = target_window(windows, target_object.window)
                .ok_or(MaterializationError::InvalidState)?;
            let end = begin
                .checked_add(usize::from(width))
                .ok_or(MaterializationError::Overflow)?;
            let old = if width == 4 {
                u64::from(u32::from_le_bytes(target[begin..end].try_into().unwrap()))
            } else {
                u64::from_le_bytes(target[begin..end].try_into().unwrap())
            };
            let value = (old & !mask) | (positioned & mask);
            if width == 4 {
                target[begin..end].copy_from_slice(&(value as u32).to_le_bytes());
            } else {
                target[begin..end].copy_from_slice(&value.to_le_bytes());
            }
        }

        let selected = (0..usize::from(state.object_count()))
            .filter_map(|index| object_info(state, index))
            .find(|object| object.window == RELOC_WINDOW_BATCH && object.variant == variant)
            .ok_or(MaterializationError::InvalidState)?;
        Ok(HelioCloudMaterializedBatch {
            offset: selected.dst,
            length: selected.bytes,
        })
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
                assert_eq!(package.vertex_simd_width(), 8);
                assert_eq!(package.fragment_simd_width(), 16);
                assert_eq!(package.sim_params_bytes(), 112);
                assert_eq!(package.render_params_bytes(), 272);
                assert_eq!(package.draw_vertex_count(), 3);
                assert_eq!(package.descriptor().len(), BYTE_LEN);
                assert_eq!(package.compute_isa(), Some(COMPUTE_ISA));
                assert_eq!(package.vertex_isa(), Some(VERTEX_ISA));
                assert_eq!(package.fragment_isa(), Some(FRAGMENT_ISA));
            }
        }

        #[test]
        fn descriptor_only_parse_never_exposes_unverified_executable_bytes() {
            let descriptor = descriptor_fixture(16, RESOURCES);
            let package = NativePackage::parse(&descriptor).expect("valid descriptor metadata");
            assert_eq!(package.compute_isa(), None);
            assert_eq!(package.vertex_isa(), None);
            assert_eq!(package.fragment_isa(), None);
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

        #[test]
        fn reloc_state_fixture_seals_six_batch_variants_and_rejects_tampering() {
            let valid = reloc_fixture();
            let state = parse_relocatable_state(&valid).expect("valid HELIOCRS v2 state");
            assert_eq!(state.object_count(), 6);
            assert_eq!(state.reloc_count(), 1);
            for index in 0..6 {
                assert_eq!(state.object_record(index).unwrap()[6], index as u8);
            }

            let mut bad_header = valid.clone();
            bad_header[48] = 0;
            assert_eq!(parse_relocatable_state(&bad_header), Err(Error::InvalidRelocState));

            let mut bad_hash = valid.clone();
            bad_hash[128 + 28] ^= 1;
            assert_eq!(parse_relocatable_state(&bad_hash), Err(Error::InvalidRelocState));

            let mut bad_overlap = valid;
            put_u32(&mut bad_overlap, 128 + 64 + 8, 0);
            assert_eq!(parse_relocatable_state(&bad_overlap), Err(Error::InvalidRelocState));
        }

        fn materialization_symbols(batch_gpu: u64) -> HelioCloudMaterializationSymbols {
            HelioCloudMaterializationSymbols {
                cs: 0,
                vs: 0,
                fs: 0,
                surface: 0,
                dynamic: 0,
                indirect: 0,
                batch: batch_gpu,
                result: 0,
                volume_a: 0,
                volume_b: 0,
                sim_params: 0,
                render_params: 0,
                ui4_producer_gpu: 0,
                width: 640,
                height: 480,
                pitch: 2560,
            }
        }

        fn materialize(
            state: RelocatableState<'_>,
            batch_gpu: u64,
            symbols: HelioCloudMaterializationSymbols,
        ) -> (Result<HelioCloudMaterializedBatch, MaterializationError>, Vec<u8>) {
            let mut batch = vec![0u8; 256 * 1024];
            let mut surface = vec![0u8; 64 * 1024];
            let mut dynamic = vec![0u8; 64 * 1024];
            let mut indirect = vec![0u8; 64 * 1024];
            let result = {
                let mut windows = HelioCloudMaterializationWindows {
                    batch: &mut batch,
                    surface: &mut surface,
                    dynamic: &mut dynamic,
                    indirect: &mut indirect,
                    batch_gpu,
                    surface_gpu: 0,
                    dynamic_gpu: 0,
                    indirect_gpu: 0,
                };
                materialize_relocatable_state(state, &mut windows, symbols, 0)
            };
            (result, batch)
        }

        #[test]
        fn materializes_minimal_state_and_returns_selected_batch() {
            let bytes = reloc_fixture();
            let state = parse_relocatable_state(&bytes).expect("valid state");
            let (result, batch) = materialize(state, 0x1000, materialization_symbols(0x1000));
            assert_eq!(
                result,
                Ok(HelioCloudMaterializedBatch {
                    offset: 0,
                    length: 4
                })
            );
            assert_eq!(&batch[..4], &[0, 0, 0, 0]);
        }

        #[test]
        fn materialization_preserves_unmasked_bytes_and_resolves_object_addresses() {
            let mut bytes = reloc_fixture();
            let data_offset = (128 + 6 * 64 + 32 + 63) & !63;
            bytes[data_offset..data_offset + 4].copy_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);
            bytes[128 + 28..128 + 60]
                .copy_from_slice(&Sha256::digest(&bytes[data_offset..data_offset + 4]));
            let reloc_offset = 128 + 6 * 64;
            put_u16(&mut bytes, reloc_offset + 2, 2);
            put_u8(&mut bytes, reloc_offset + 13, RELOC_VALUE_OBJECT_OFFSET);
            put_u64(&mut bytes, reloc_offset + 16, 0xffff_ffff);
            let state = parse_relocatable_state(&bytes).expect("valid object relocation");
            let (result, batch) = materialize(state, 0x1000, materialization_symbols(0x1000));
            assert!(result.is_ok());
            assert_eq!(&batch[..4], &[4, 0xbb, 0xcc, 0xdd]);

            put_u8(&mut bytes, reloc_offset + 13, RELOC_VALUE_OBJECT_GPU);
            let state = parse_relocatable_state(&bytes).expect("valid GPU relocation");
            let (result, batch) = materialize(state, 0x1000, materialization_symbols(0x1000));
            assert!(result.is_ok());
            assert_eq!(&batch[..4], &[4, 0x10, 0, 0]);
        }

        #[test]
        fn materialization_applies_runtime_u32_negative_one() {
            let mut bytes = reloc_fixture();
            let reloc_offset = 128 + 6 * 64;
            put_u16(&mut bytes, reloc_offset + 2, SYMBOL_WIDTH);
            put_u8(&mut bytes, reloc_offset + 13, RELOC_VALUE_RUNTIME_U32);
            put_u64(&mut bytes, reloc_offset + 16, 0xffff_ffff);
            put_u64(&mut bytes, reloc_offset + 24, u64::MAX);
            let state = parse_relocatable_state(&bytes).expect("valid runtime relocation");
            let (result, batch) = materialize(state, 0, materialization_symbols(0));
            assert!(result.is_ok());
            assert_eq!(&batch[..4], &[0x7f, 0x02, 0, 0]);
        }

        #[test]
        fn materialization_positions_masked_fields_at_the_object_destination() {
            let mut bytes = reloc_fixture();
            let data_offset = (128 + 6 * 64 + 32 + 63) & !63;
            bytes[data_offset..data_offset + 4].copy_from_slice(&0xc000_55aau32.to_le_bytes());
            bytes[128 + 28..128 + 60]
                .copy_from_slice(&Sha256::digest(&bytes[data_offset..data_offset + 4]));
            put_u32(&mut bytes, 128 + 8, 24);
            let reloc_offset = 128 + 6 * 64;
            put_u16(&mut bytes, reloc_offset + 2, SYMBOL_WIDTH);
            put_u8(&mut bytes, reloc_offset + 13, RELOC_VALUE_RUNTIME_U32);
            put_u64(&mut bytes, reloc_offset + 16, 0x03ff_0000);
            put_u64(&mut bytes, reloc_offset + 24, u64::MAX);

            let state = parse_relocatable_state(&bytes).expect("valid positioned relocation");
            let (result, batch) = materialize(state, 0, materialization_symbols(0));
            assert_eq!(
                result,
                Ok(HelioCloudMaterializedBatch {
                    offset: 24,
                    length: 4,
                })
            );
            assert_eq!(u32::from_le_bytes(batch[24..28].try_into().unwrap()), 0xc27f_55aa);
        }

        #[test]
        fn materialization_rejects_checked_overflow() {
            let mut bytes = reloc_fixture();
            let reloc_offset = 128 + 6 * 64;
            put_u64(&mut bytes, reloc_offset + 24, 1);
            let state = parse_relocatable_state(&bytes).expect("valid state");
            let mut symbols = materialization_symbols(u64::MAX);
            symbols.batch = u64::MAX;
            let (result, _) = materialize(state, u64::MAX, symbols);
            assert_eq!(result, Err(MaterializationError::Overflow));
        }

        #[test]
        fn rejects_missing_uniform_binding_or_invalid_graphics_contract() {
            let mut missing_uniform = descriptor_fixture(16, RESOURCES);
            missing_uniform[63] = LOGICAL_SAMPLED;
            assert_eq!(NativePackage::parse(&missing_uniform), Err(Error::InvalidBindings));

            let mut fake_vertex_simd = descriptor_fixture(16, RESOURCES);
            put_u16(&mut fake_vertex_simd, 50, 16);
            assert_eq!(NativePackage::parse(&fake_vertex_simd), Err(Error::InvalidBindings));

            let mut wrong_params = descriptor_fixture(16, RESOURCES);
            put_u16(&mut wrong_params, 90, 112);
            assert_eq!(NativePackage::parse(&wrong_params), Err(Error::InvalidBindings));

            let mut wrong_flags = descriptor_fixture(16, RESOURCES);
            put_u16(&mut wrong_flags, 94, FLAG_FULLSCREEN_TRIANGLE as u16);
            assert_eq!(NativePackage::parse(&wrong_flags), Err(Error::InvalidBindings));
        }

        fn descriptor_fixture(compute_simd: u16, resources: &[u8]) -> Vec<u8> {
            let mut descriptor = vec![0u8; BYTE_LEN];
            let reloc = reloc_fixture();
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
            put_u16(&mut descriptor, 50, 8);
            put_u16(&mut descriptor, 52, 16);
            put_u16(&mut descriptor, 54, REQUIRED_BINDINGS.len() as u16);
            for (index, binding) in REQUIRED_BINDINGS.into_iter().enumerate() {
                descriptor[60 + index * 4..64 + index * 4].copy_from_slice(&binding);
            }
            put_u16(&mut descriptor, 88, 112);
            put_u16(&mut descriptor, 90, 272);
            put_u16(&mut descriptor, 92, 3);
            put_u16(&mut descriptor, 94, REQUIRED_GRAPHICS_FLAGS as u16);
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
            put_u32(&mut descriptor, RELOC_STATE_BYTES_OFFSET, reloc.len() as u32);
            descriptor[RELOC_STATE_HASH_OFFSET..RELOC_STATE_HASH_OFFSET + 32]
                .copy_from_slice(&Sha256::digest(&reloc));
            descriptor
        }

        fn reloc_fixture() -> Vec<u8> {
            let object_count = 6usize;
            let object_offset = RELOC_HEADER_BYTES;
            let reloc_offset = object_offset + object_count * RELOC_OBJECT_BYTES;
            let data_offset = (reloc_offset + RELOC_ENTRY_BYTES + 63) & !63;
            let mut bytes = vec![0u8; data_offset + object_count * 4];
            bytes[..8].copy_from_slice(&RELOC_MAGIC);
            put_u16(&mut bytes, 8, RELOC_VERSION);
            put_u16(&mut bytes, 10, RELOC_HEADER_BYTES as u16);
            let total_bytes = bytes.len() as u32;
            put_u32(&mut bytes, 12, total_bytes);
            put_u16(&mut bytes, 16, 120);
            put_u16(&mut bytes, 18, 0x4680);
            bytes[20] = 0x0c;
            bytes[21] = 1;
            bytes[22] = 64;
            put_u16(&mut bytes, 24, RELOC_OBJECT_BYTES as u16);
            put_u16(&mut bytes, 26, RELOC_ENTRY_BYTES as u16);
            put_u16(&mut bytes, 28, object_count as u16);
            put_u16(&mut bytes, 30, 1);
            put_u32(&mut bytes, 32, object_offset as u32);
            put_u32(&mut bytes, 36, reloc_offset as u32);
            put_u32(&mut bytes, 40, data_offset as u32);
            put_u32(&mut bytes, 44, (object_count * 4) as u32);
            put_u32(&mut bytes, 48, RELOC_FLAGS);
            bytes[52] = 6;
            bytes[53] = 2;
            put_u32(&mut bytes, 56, (object_count * RELOC_OBJECT_BYTES) as u32);
            put_u32(&mut bytes, 60, RELOC_ENTRY_BYTES as u32);
            for index in 0..object_count {
                let offset = object_offset + index * RELOC_OBJECT_BYTES;
                put_u16(&mut bytes, offset, index as u16 + 1);
                bytes[offset + 2] = RELOC_WINDOW_BATCH;
                bytes[offset + 3] = RELOC_KIND_BATCH;
                put_u16(&mut bytes, offset + 4, index as u16 + 1);
                bytes[offset + 6] = index as u8;
                put_u32(&mut bytes, offset + 8, (index * 4) as u32);
                put_u32(&mut bytes, offset + 12, (index * 4) as u32);
                put_u32(&mut bytes, offset + 16, 4);
                put_u16(&mut bytes, offset + 20, 4);
                if index == 0 {
                    put_u16(&mut bytes, offset + 22, 0);
                    put_u16(&mut bytes, offset + 24, 1);
                }
                bytes[data_offset + index * 4..data_offset + index * 4 + 4].copy_from_slice(&[
                    index as u8 + 1,
                    0,
                    0,
                    0,
                ]);
                bytes[offset + 28..offset + 60].copy_from_slice(&Sha256::digest(
                    &bytes[data_offset + index * 4..data_offset + index * 4 + 4],
                ));
            }
            let offset = reloc_offset;
            put_u16(&mut bytes, offset, 1);
            put_u16(&mut bytes, offset + 2, SYMBOL_BATCH);
            put_u32(&mut bytes, offset + 4, 0);
            put_u32(&mut bytes, offset + 8, 0);
            bytes[offset + 12] = 4;
            bytes[offset + 13] = RELOC_VALUE_FIXED_GPU;
            bytes[offset + 16..offset + 24].copy_from_slice(&0xFFFF_FFFFu64.to_le_bytes());
            bytes
        }

        fn artifact_fixture(
            compute_simd: u16,
            compute_source: &[u8],
            include_compute_isa: bool,
        ) -> Vec<u8> {
            let (resources, volume_metadata) =
                super::super::helio_volume_resources::helioc_test_cloud_resource();
            let descriptor = descriptor_fixture(compute_simd, &resources);
            let reloc = reloc_fixture();
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
                (RELOC_STATE_SECTION, SectionKind::CompilerMetadata.raw(), reloc.as_slice()),
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
        fn put_u8(bytes: &mut [u8], offset: usize, value: u8) {
            bytes[offset] = value;
        }
        fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
}
