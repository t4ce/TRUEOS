    pub(crate) const SECTION_NAME: &str = "resources/volume3d-rgba16f-v1.bin";
    pub(crate) const COMPILER_METADATA_SECTION_NAME: &str =
        "compiler/volume3d-bindings-v1.json";

    pub(crate) const MAGIC: [u8; 8] = *b"TOSV3D\0\0";
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
    

    impl TextureFormat {
        
    }

    

    impl TextureDimension {
        
    }

    

    impl CachePolicy {
        
    }

    

    impl MappingLifetime {
        
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

    

    impl AddressMode {
        
    }

    

    impl FilterMode {
        
    }

    

    impl CoordinateMode {
        
    }

    

    impl ShaderStage {
        
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

    

    impl SamplerRecord {
        fn is_valid(self) -> bool {
            self.sampler_id != 0 && self.max_anisotropy == 1
        }

    }

    

    impl TextureBindingRecord {
        fn is_valid(self) -> bool {
            self.pipeline_id != 0
                && self.bind_group_id != 0
                && self.view_id != 0
                && self.binding_table_index != u16::MAX
        }
    }

    

    impl SamplerBindingRecord {
        fn is_valid(self) -> bool {
            self.pipeline_id != 0
                && self.bind_group_id != 0
                && self.sampler_id != 0
                && self.sampler_table_index != u16::MAX
        }
    }

    

    
