    pub(crate) const SECTION_NAME: &str = "resources/volume3d-rgba16f-v1.bin";
    pub(crate) const COMPILER_METADATA_SECTION_NAME: &str =
        "compiler/cloud-volume-bindings-v1.json";
    pub(crate) const MAGIC: [u8; 8] = *b"HELV3D\0\0";
    pub(crate) const VERSION: u16 = 1;
    pub(crate) const HEADER_LEN: usize = 96;

    const VOLUME_RECORD_LEN: usize = 40;
    const VIEW_RECORD_LEN: usize = 24;
    const SAMPLER_RECORD_LEN: usize = 24;
    const TEXTURE_BINDING_RECORD_LEN: usize = 24;
    const SAMPLER_BINDING_RECORD_LEN: usize = 24;

    const MAX_VOLUMES: usize = 16;
    const MAX_VIEWS: usize = MAX_VOLUMES * 2;
    const MAX_SAMPLERS: usize = 16;
    const MAX_TEXTURE_BINDINGS: usize = 128;
    const MAX_SAMPLER_BINDINGS: usize = 128;

    pub(crate) const VOLUME_USAGE_SAMPLED: u32 = 1 << 0;
    pub(crate) const VOLUME_USAGE_STORAGE: u32 = 1 << 1;
    const KNOWN_VOLUME_USAGE: u32 = VOLUME_USAGE_SAMPLED | VOLUME_USAGE_STORAGE;

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u16)]
    pub(crate) enum TextureFormat {
        Rgba16Float = 1,
    }

    impl TextureFormat {
        const fn from_raw(raw: u16) -> Option<Self> {
            match raw {
                1 => Some(Self::Rgba16Float),
                _ => None,
            }
        }
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u16)]
    pub(crate) enum TextureDimension {
        D3 = 3,
    }

    impl TextureDimension {
        const fn from_raw(raw: u16) -> Option<Self> {
            match raw {
                3 => Some(Self::D3),
                _ => None,
            }
        }
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u16)]
    pub(crate) enum CachePolicy {
        WriteBack = 1,
        Uncached = 2,
    }

    impl CachePolicy {
        const fn from_raw(raw: u16) -> Option<Self> {
            match raw {
                1 => Some(Self::WriteBack),
                2 => Some(Self::Uncached),
                _ => None,
            }
        }
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u16)]
    pub(crate) enum MappingLifetime {
        Dispatch = 1,
        Frame = 2,
        Artifact = 3,
    }

    impl MappingLifetime {
        const fn from_raw(raw: u16) -> Option<Self> {
            match raw {
                1 => Some(Self::Dispatch),
                2 => Some(Self::Frame),
                3 => Some(Self::Artifact),
                _ => None,
            }
        }
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u16)]
    pub(crate) enum ViewAccess {
        Sampled = 1,
        StorageReadOnly = 2,
        StorageWriteOnly = 3,
        StorageReadWrite = 4,
    }

    impl ViewAccess {
        const fn from_raw(raw: u16) -> Option<Self> {
            match raw {
                1 => Some(Self::Sampled),
                2 => Some(Self::StorageReadOnly),
                3 => Some(Self::StorageWriteOnly),
                4 => Some(Self::StorageReadWrite),
                _ => None,
            }
        }

        pub(crate) const fn is_storage(self) -> bool {
            !matches!(self, Self::Sampled)
        }
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u16)]
    pub(crate) enum AddressMode {
        ClampToEdge = 1,
        Repeat = 2,
    }

    impl AddressMode {
        const fn from_raw(raw: u16) -> Option<Self> {
            match raw {
                1 => Some(Self::ClampToEdge),
                2 => Some(Self::Repeat),
                _ => None,
            }
        }
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u16)]
    pub(crate) enum FilterMode {
        Nearest = 1,
        Linear = 2,
    }

    impl FilterMode {
        const fn from_raw(raw: u16) -> Option<Self> {
            match raw {
                1 => Some(Self::Nearest),
                2 => Some(Self::Linear),
                _ => None,
            }
        }
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u16)]
    pub(crate) enum CoordinateMode {
        Normalized = 1,
        Unnormalized = 2,
    }

    impl CoordinateMode {
        const fn from_raw(raw: u16) -> Option<Self> {
            match raw {
                1 => Some(Self::Normalized),
                2 => Some(Self::Unnormalized),
                _ => None,
            }
        }
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u16)]
    pub(crate) enum ShaderStage {
        Compute = 1,
        Vertex = 2,
        Fragment = 3,
    }

    impl ShaderStage {
        const fn from_raw(raw: u16) -> Option<Self> {
            match raw {
                1 => Some(Self::Compute),
                2 => Some(Self::Vertex),
                3 => Some(Self::Fragment),
                _ => None,
            }
        }
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) struct VolumeRecord {
        pub(crate) resource_id: u32,
        pub(crate) width: u32,
        pub(crate) height: u32,
        pub(crate) depth: u32,
        pub(crate) row_pitch_bytes: u32,
        pub(crate) slice_pitch_bytes: u32,
        pub(crate) format: TextureFormat,
        pub(crate) dimension: TextureDimension,
        pub(crate) cache_policy: CachePolicy,
        pub(crate) mapping_lifetime: MappingLifetime,
        pub(crate) usage_flags: u32,
    }

    impl VolumeRecord {
        pub(crate) fn required_bytes(self) -> Option<usize> {
            if self.width == 0 || self.height == 0 || self.depth == 0 {
                return None;
            }
            let row_bytes = (self.width as usize).checked_mul(GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL)?;
            let last_slice = (self.depth as usize)
                .checked_sub(1)?
                .checked_mul(self.slice_pitch_bytes as usize)?;
            let last_row = (self.height as usize)
                .checked_sub(1)?
                .checked_mul(self.row_pitch_bytes as usize)?;
            last_slice.checked_add(last_row)?.checked_add(row_bytes)
        }

        fn is_valid(self) -> bool {
            if self.resource_id == 0
                || self.width == 0
                || self.height == 0
                || self.depth == 0
                || self.usage_flags & !KNOWN_VOLUME_USAGE != 0
                || self.usage_flags & KNOWN_VOLUME_USAGE != KNOWN_VOLUME_USAGE
            {
                return false;
            }
            let Some(row_bytes) = self
                .width
                .checked_mul(GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL as u32)
            else {
                return false;
            };
            if self.row_pitch_bytes < row_bytes
                || !self
                    .row_pitch_bytes
                    .is_multiple_of(GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL as u32)
            {
                return false;
            }
            let Some(min_slice_pitch) = self.row_pitch_bytes.checked_mul(self.height) else {
                return false;
            };
            self.slice_pitch_bytes >= min_slice_pitch
                && self.slice_pitch_bytes.is_multiple_of(self.row_pitch_bytes)
                && self.required_bytes().is_some()
        }
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) struct ViewRecord {
        pub(crate) view_id: u32,
        pub(crate) resource_id: u32,
        pub(crate) access: ViewAccess,
        pub(crate) format: TextureFormat,
        pub(crate) dimension: TextureDimension,
        pub(crate) base_mip_level: u16,
        pub(crate) mip_level_count: u16,
        pub(crate) base_array_layer: u16,
        pub(crate) array_layer_count: u16,
    }

    impl ViewRecord {
        fn is_valid(self) -> bool {
            self.view_id != 0
                && self.resource_id != 0
                && self.base_mip_level == 0
                && self.mip_level_count == 1
                && self.base_array_layer == 0
                && self.array_layer_count == 1
        }
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) struct SamplerRecord {
        pub(crate) sampler_id: u32,
        pub(crate) address_u: AddressMode,
        pub(crate) address_v: AddressMode,
        pub(crate) address_w: AddressMode,
        pub(crate) min_filter: FilterMode,
        pub(crate) mag_filter: FilterMode,
        pub(crate) mip_filter: FilterMode,
        pub(crate) coordinate_mode: CoordinateMode,
        pub(crate) max_anisotropy: u16,
    }

    impl SamplerRecord {
        fn is_valid(self) -> bool {
            self.sampler_id != 0 && self.max_anisotropy == 1
        }

        pub(crate) const fn is_helio_cloud_sampler(self) -> bool {
            matches!(self.address_u, AddressMode::Repeat)
                && matches!(self.address_v, AddressMode::ClampToEdge)
                && matches!(self.address_w, AddressMode::Repeat)
                && matches!(self.min_filter, FilterMode::Linear)
                && matches!(self.mag_filter, FilterMode::Linear)
                && matches!(self.mip_filter, FilterMode::Nearest)
                && matches!(self.coordinate_mode, CoordinateMode::Normalized)
                && self.max_anisotropy == 1
        }
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) struct TextureBindingRecord {
        pub(crate) pipeline_id: u32,
        pub(crate) bind_group_id: u32,
        pub(crate) view_id: u32,
        pub(crate) stage: ShaderStage,
        pub(crate) group: u16,
        pub(crate) binding: u16,
        pub(crate) binding_table_index: u16,
        pub(crate) access: ViewAccess,
    }

    impl TextureBindingRecord {
        fn is_valid(self) -> bool {
            self.pipeline_id != 0
                && self.bind_group_id != 0
                && self.view_id != 0
                && self.binding_table_index != u16::MAX
        }
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) struct SamplerBindingRecord {
        pub(crate) pipeline_id: u32,
        pub(crate) bind_group_id: u32,
        pub(crate) sampler_id: u32,
        pub(crate) stage: ShaderStage,
        pub(crate) group: u16,
        pub(crate) binding: u16,
        pub(crate) sampler_table_index: u16,
    }

    impl SamplerBindingRecord {
        fn is_valid(self) -> bool {
            self.pipeline_id != 0
                && self.bind_group_id != 0
                && self.sampler_id != 0
                && self.sampler_table_index != u16::MAX
        }
    }

    /// Runtime-resolved form of one relocatable volume record.
    ///
    /// Both views remain logical aliases of the single page-backed allocation;
    /// native state encoders consume their access modes and the separate
    /// compiler binding records rather than allocating duplicate storage.
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) struct ResolvedRgba16FloatVolume3d {
        pub(crate) descriptor: VolumeRecord,
        pub(crate) allocation: GpgpuRgba16FloatVolume3d,
        pub(crate) sampled_view: ViewRecord,
        pub(crate) storage_view: ViewRecord,
    }

    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Program<'a> {
        bytes: &'a [u8],
        volume_count: usize,
        view_count: usize,
        sampler_count: usize,
        texture_binding_count: usize,
        sampler_binding_count: usize,
        volume_offset: usize,
        view_offset: usize,
        sampler_offset: usize,
        texture_binding_offset: usize,
        sampler_binding_offset: usize,
        compiler_metadata_sha256: [u8; 32],
    }
