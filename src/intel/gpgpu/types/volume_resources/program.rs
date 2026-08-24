    impl<'a> Program<'a> {
        /// Opens this resource contract directly from a validated HELIOA container.
        ///
        /// The generic container remains forward-compatible; the Intel backend
        /// opts into this named normalized-IR section only when it is present.
        pub(crate) fn parse_artifact(artifact: Artifact<'a>) -> Result<Self, Error> {
            let section = artifact.section(SECTION_NAME).ok_or(Error::MissingSection)?;
            if section.kind != SectionKind::NormalizedRenderIr {
                return Err(Error::WrongSectionKind);
            }
            let program = Self::parse(section.data)?;
            let metadata = artifact
                .section(COMPILER_METADATA_SECTION_NAME)
                .ok_or(Error::MissingCompilerMetadata)?;
            if metadata.kind != SectionKind::CompilerMetadata {
                return Err(Error::WrongCompilerMetadataKind);
            }
            let actual_metadata_sha256 = Sha256::digest(metadata.data);
            if actual_metadata_sha256[..] != program.compiler_metadata_sha256[..] {
                return Err(Error::CompilerMetadataHashMismatch);
            }
            Ok(program)
        }

        pub(crate) fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
            if bytes.len() < HEADER_LEN || bytes.get(..MAGIC.len()) != Some(MAGIC.as_slice()) {
                return Err(Error::BadMagic);
            }
            let version = read_u16(bytes, 8)?;
            if version != VERSION {
                return Err(Error::UnsupportedVersion(version));
            }
            if usize::from(read_u16(bytes, 10)?) != HEADER_LEN {
                return Err(Error::MalformedHeader);
            }
            if to_usize(read_u32(bytes, 12)?)? != bytes.len() {
                return Err(Error::LengthMismatch);
            }

            let volume_count = usize::from(read_u16(bytes, 16)?);
            let view_count = usize::from(read_u16(bytes, 18)?);
            let sampler_count = usize::from(read_u16(bytes, 20)?);
            let texture_binding_count = usize::from(read_u16(bytes, 22)?);
            let sampler_binding_count = usize::from(read_u16(bytes, 24)?);
            if read_u16(bytes, 26)? != 0 || bytes[80..96].iter().any(|byte| *byte != 0) {
                return Err(Error::NonZeroReserved);
            }
            let mut compiler_metadata_sha256 = [0u8; 32];
            compiler_metadata_sha256.copy_from_slice(&bytes[48..80]);
            if compiler_metadata_sha256.iter().all(|byte| *byte == 0) {
                return Err(Error::MissingCompilerMetadataHash);
            }
            if volume_count == 0
                || volume_count > MAX_VOLUMES
                || view_count != volume_count.checked_mul(2).ok_or(Error::TooManyRecords)?
                || view_count > MAX_VIEWS
                || sampler_count == 0
                || sampler_count > MAX_SAMPLERS
                || texture_binding_count == 0
                || texture_binding_count > MAX_TEXTURE_BINDINGS
                || sampler_binding_count == 0
                || sampler_binding_count > MAX_SAMPLER_BINDINGS
            {
                return Err(Error::TooManyRecords);
            }

            let volume_offset = to_usize(read_u32(bytes, 28)?)?;
            let view_offset = to_usize(read_u32(bytes, 32)?)?;
            let sampler_offset = to_usize(read_u32(bytes, 36)?)?;
            let texture_binding_offset = to_usize(read_u32(bytes, 40)?)?;
            let sampler_binding_offset = to_usize(read_u32(bytes, 44)?)?;

            let expected_volume_offset = HEADER_LEN;
            let expected_view_offset = record_end(
                expected_volume_offset,
                volume_count,
                VOLUME_RECORD_LEN,
            )?;
            let expected_sampler_offset =
                record_end(expected_view_offset, view_count, VIEW_RECORD_LEN)?;
            let expected_texture_binding_offset = record_end(
                expected_sampler_offset,
                sampler_count,
                SAMPLER_RECORD_LEN,
            )?;
            let expected_sampler_binding_offset = record_end(
                expected_texture_binding_offset,
                texture_binding_count,
                TEXTURE_BINDING_RECORD_LEN,
            )?;
            let expected_len = record_end(
                expected_sampler_binding_offset,
                sampler_binding_count,
                SAMPLER_BINDING_RECORD_LEN,
            )?;
            if volume_offset != expected_volume_offset
                || view_offset != expected_view_offset
                || sampler_offset != expected_sampler_offset
                || texture_binding_offset != expected_texture_binding_offset
                || sampler_binding_offset != expected_sampler_binding_offset
                || expected_len != bytes.len()
            {
                return Err(Error::MalformedHeader);
            }

            let program = Self {
                bytes,
                volume_count,
                view_count,
                sampler_count,
                texture_binding_count,
                sampler_binding_count,
                volume_offset,
                view_offset,
                sampler_offset,
                texture_binding_offset,
                sampler_binding_offset,
                compiler_metadata_sha256,
            };
            program.validate()?;
            Ok(program)
        }

        pub(crate) const fn volume_count(self) -> usize {
            self.volume_count
        }

        pub(crate) const fn view_count(self) -> usize {
            self.view_count
        }

        pub(crate) const fn sampler_count(self) -> usize {
            self.sampler_count
        }

        pub(crate) const fn texture_binding_count(self) -> usize {
            self.texture_binding_count
        }

        pub(crate) const fn sampler_binding_count(self) -> usize {
            self.sampler_binding_count
        }

        pub(crate) const fn compiler_metadata_sha256(self) -> [u8; 32] {
            self.compiler_metadata_sha256
        }

        pub(crate) fn volume(self, index: usize) -> Option<VolumeRecord> {
            self.volume_at(index).ok()
        }

        pub(crate) fn view(self, index: usize) -> Option<ViewRecord> {
            self.view_at(index).ok()
        }

        pub(crate) fn sampler(self, index: usize) -> Option<SamplerRecord> {
            self.sampler_at(index).ok()
        }

        pub(crate) fn texture_binding(self, index: usize) -> Option<TextureBindingRecord> {
            self.texture_binding_at(index).ok()
        }

        pub(crate) fn sampler_binding(self, index: usize) -> Option<SamplerBindingRecord> {
            self.sampler_binding_at(index).ok()
        }

        pub(crate) fn volume_by_id(self, resource_id: u32) -> Option<VolumeRecord> {
            let mut index = 0;
            while index < self.volume_count {
                let record = self.volume(index)?;
                if record.resource_id == resource_id {
                    return Some(record);
                }
                index += 1;
            }
            None
        }

        pub(crate) fn view_by_id(self, view_id: u32) -> Option<ViewRecord> {
            let mut index = 0;
            while index < self.view_count {
                let record = self.view(index)?;
                if record.view_id == view_id {
                    return Some(record);
                }
                index += 1;
            }
            None
        }

        pub(crate) fn sampler_by_id(self, sampler_id: u32) -> Option<SamplerRecord> {
            let mut index = 0;
            while index < self.sampler_count {
                let record = self.sampler(index)?;
                if record.sampler_id == sampler_id {
                    return Some(record);
                }
                index += 1;
            }
            None
        }

        pub(crate) fn resolve_volume(
            self,
            resource_id: u32,
            phys: u64,
            gpu: u64,
            bytes: usize,
        ) -> Result<ResolvedRgba16FloatVolume3d, Error> {
            let descriptor = self
                .volume_by_id(resource_id)
                .ok_or(Error::MissingResource)?;
            let allocation = GpgpuRgba16FloatVolume3d::new(
                phys,
                gpu,
                bytes,
                descriptor.width,
                descriptor.height,
                descriptor.depth,
                descriptor.row_pitch_bytes,
                descriptor.slice_pitch_bytes,
            )
            .ok_or(Error::InvalidRuntimeAllocation)?;
            let mut sampled_view = None;
            let mut storage_view = None;
            let mut index = 0;
            while index < self.view_count {
                let view = self.view(index).ok_or(Error::OutOfBounds)?;
                if view.resource_id == resource_id {
                    if matches!(view.access, ViewAccess::Sampled) {
                        sampled_view = Some(view);
                    } else {
                        storage_view = Some(view);
                    }
                }
                index += 1;
            }
            Ok(ResolvedRgba16FloatVolume3d {
                descriptor,
                allocation,
                sampled_view: sampled_view.ok_or(Error::MissingView)?,
                storage_view: storage_view.ok_or(Error::MissingView)?,
            })
        }

        /// Exact resource/view profile requested by `Helio-Examples/cloud_engine.rs`.
        ///
        /// Compiler-selected BTI and sampler indices may vary, but the two
        /// ping-pong bind-group variants must preserve one sampled source, one
        /// write-only storage destination, and the shared repeat/clamp/repeat
        /// normalized linear sampler.
        pub(crate) fn is_helio_cloud_profile(self) -> bool {
            if self.volume_count != 2
                || self.view_count != 4
                || self.sampler_count != 1
                || self.texture_binding_count != 6
                || self.sampler_binding_count != 4
            {
                return false;
            }

            let first = match self.volume(0) {
                Some(volume) => volume,
                None => return false,
            };
            let second = match self.volume(1) {
                Some(volume) => volume,
                None => return false,
            };
            for volume in [first, second] {
                if volume.width != 96
                    || volume.height != 48
                    || volume.depth != 96
                    || volume.row_pitch_bytes != 768
                    || volume.slice_pitch_bytes != 36_864
                    || !matches!(volume.format, TextureFormat::Rgba16Float)
                    || !matches!(volume.dimension, TextureDimension::D3)
                    || !matches!(volume.cache_policy, CachePolicy::WriteBack)
                    || !matches!(volume.mapping_lifetime, MappingLifetime::Artifact)
                    || volume.usage_flags != KNOWN_VOLUME_USAGE
                {
                    return false;
                }
            }
            if first.resource_id == second.resource_id {
                return false;
            }
            let sampler = match self.sampler(0) {
                Some(sampler) if sampler.is_helio_cloud_sampler() => sampler,
                _ => return false,
            };

            let mut compute_pairs = [(0u32, 0u32, 0u32, 0u16, 0u16, 0u16); 2];
            let mut compute_count = 0usize;
            let mut texture_index = 0usize;
            while texture_index < self.texture_binding_count {
                let sampled = match self.texture_binding(texture_index) {
                    Some(binding)
                        if matches!(binding.stage, ShaderStage::Compute)
                            && binding.group == 0
                            && binding.binding == 1
                            && matches!(binding.access, ViewAccess::Sampled) =>
                    {
                        binding
                    }
                    _ => {
                        texture_index += 1;
                        continue;
                    }
                };
                if compute_count >= compute_pairs.len() {
                    return false;
                }
                let source = match self.view_by_id(sampled.view_id) {
                    Some(view) => view.resource_id,
                    None => return false,
                };
                let storage = match self.find_texture_binding(
                    sampled.pipeline_id,
                    sampled.bind_group_id,
                    ShaderStage::Compute,
                    0,
                    3,
                ) {
                    Some(binding) if matches!(binding.access, ViewAccess::StorageWriteOnly) => {
                        binding
                    }
                    _ => return false,
                };
                let destination = match self.view_by_id(storage.view_id) {
                    Some(view) => view.resource_id,
                    None => return false,
                };
                let sampler_binding = match self.find_sampler_binding(
                    sampled.pipeline_id,
                    sampled.bind_group_id,
                    ShaderStage::Compute,
                    0,
                    2,
                ) {
                    Some(binding) if binding.sampler_id == sampler.sampler_id => binding,
                    _ => return false,
                };
                if source == destination {
                    return false;
                }
                compute_pairs[compute_count] = (
                    source,
                    destination,
                    sampled.pipeline_id,
                    sampled.binding_table_index,
                    storage.binding_table_index,
                    sampler_binding.sampler_table_index,
                );
                compute_count += 1;
                texture_index += 1;
            }
            if compute_count != 2
                || compute_pairs[0].0 != compute_pairs[1].1
                || compute_pairs[0].1 != compute_pairs[1].0
                || compute_pairs[0].2 != compute_pairs[1].2
                || compute_pairs[0].3 != compute_pairs[1].3
                || compute_pairs[0].4 != compute_pairs[1].4
                || compute_pairs[0].5 != compute_pairs[1].5
                || compute_pairs[0].3 == compute_pairs[0].4
            {
                return false;
            }
            let resource_ids = [first.resource_id, second.resource_id];
            if !resource_ids.contains(&compute_pairs[0].0)
                || !resource_ids.contains(&compute_pairs[0].1)
            {
                return false;
            }

            let mut fragment_sources = [(0u32, 0u32, 0u16, 0u16); 2];
            let mut fragment_count = 0usize;
            texture_index = 0;
            while texture_index < self.texture_binding_count {
                let sampled = match self.texture_binding(texture_index) {
                    Some(binding)
                        if matches!(binding.stage, ShaderStage::Fragment)
                            && binding.group == 0
                            && binding.binding == 1
                            && matches!(binding.access, ViewAccess::Sampled) =>
                    {
                        binding
                    }
                    _ => {
                        texture_index += 1;
                        continue;
                    }
                };
                if fragment_count >= fragment_sources.len() {
                    return false;
                }
                let source = match self.view_by_id(sampled.view_id) {
                    Some(view) => view.resource_id,
                    None => return false,
                };
                let sampler_binding = match self.find_sampler_binding(
                    sampled.pipeline_id,
                    sampled.bind_group_id,
                    ShaderStage::Fragment,
                    0,
                    2,
                ) {
                    Some(binding) if binding.sampler_id == sampler.sampler_id => binding,
                    _ => return false,
                };
                fragment_sources[fragment_count] = (
                    source,
                    sampled.pipeline_id,
                    sampled.binding_table_index,
                    sampler_binding.sampler_table_index,
                );
                fragment_count += 1;
                texture_index += 1;
            }
            fragment_count == 2
                && fragment_sources[0].0 != fragment_sources[1].0
                && resource_ids.contains(&fragment_sources[0].0)
                && resource_ids.contains(&fragment_sources[1].0)
                && fragment_sources[0].1 == fragment_sources[1].1
                && fragment_sources[0].2 == fragment_sources[1].2
                && fragment_sources[0].3 == fragment_sources[1].3
        }

        fn find_texture_binding(
            self,
            pipeline_id: u32,
            bind_group_id: u32,
            stage: ShaderStage,
            group: u16,
            binding: u16,
        ) -> Option<TextureBindingRecord> {
            let mut found = None;
            let mut index = 0;
            while index < self.texture_binding_count {
                let candidate = self.texture_binding(index)?;
                if candidate.pipeline_id == pipeline_id
                    && candidate.bind_group_id == bind_group_id
                    && candidate.stage == stage
                    && candidate.group == group
                    && candidate.binding == binding
                {
                    if found.is_some() {
                        return None;
                    }
                    found = Some(candidate);
                }
                index += 1;
            }
            found
        }

        fn find_sampler_binding(
            self,
            pipeline_id: u32,
            bind_group_id: u32,
            stage: ShaderStage,
            group: u16,
            binding: u16,
        ) -> Option<SamplerBindingRecord> {
            let mut found = None;
            let mut index = 0;
            while index < self.sampler_binding_count {
                let candidate = self.sampler_binding(index)?;
                if candidate.pipeline_id == pipeline_id
                    && candidate.bind_group_id == bind_group_id
                    && candidate.stage == stage
                    && candidate.group == group
                    && candidate.binding == binding
                {
                    if found.is_some() {
                        return None;
                    }
                    found = Some(candidate);
                }
                index += 1;
            }
            found
        }
    }
