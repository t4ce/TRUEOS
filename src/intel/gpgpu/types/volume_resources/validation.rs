    impl<'a> Program<'a> {
        fn validate(self) -> Result<(), Error> {
            let mut volume_index = 0usize;
            while volume_index < self.volume_count {
                let volume = self.volume_at(volume_index)?;
                if !volume.is_valid() {
                    return Err(Error::InvalidVolume);
                }
                let mut prior = 0usize;
                while prior < volume_index {
                    if self.volume_at(prior)?.resource_id == volume.resource_id {
                        return Err(Error::DuplicateId);
                    }
                    prior += 1;
                }
                volume_index += 1;
            }

            let mut view_index = 0usize;
            while view_index < self.view_count {
                let view = self.view_at(view_index)?;
                if !view.is_valid() {
                    return Err(Error::InvalidView);
                }
                let volume = self
                    .volume_by_id(view.resource_id)
                    .ok_or(Error::MissingResource)?;
                if view.format != volume.format || view.dimension != volume.dimension {
                    return Err(Error::InvalidView);
                }
                let mut prior = 0usize;
                while prior < view_index {
                    if self.view_at(prior)?.view_id == view.view_id {
                        return Err(Error::DuplicateId);
                    }
                    prior += 1;
                }
                view_index += 1;
            }

            volume_index = 0;
            while volume_index < self.volume_count {
                let volume = self.volume_at(volume_index)?;
                let mut sampled = 0usize;
                let mut storage = 0usize;
                view_index = 0;
                while view_index < self.view_count {
                    let view = self.view_at(view_index)?;
                    if view.resource_id == volume.resource_id {
                        if matches!(view.access, ViewAccess::Sampled) {
                            sampled += 1;
                        } else {
                            storage += 1;
                        }
                    }
                    view_index += 1;
                }
                if sampled != 1 || storage != 1 {
                    return Err(Error::MissingView);
                }
                volume_index += 1;
            }

            let mut sampler_index = 0usize;
            while sampler_index < self.sampler_count {
                let sampler = self.sampler_at(sampler_index)?;
                if !sampler.is_valid() {
                    return Err(Error::InvalidSampler);
                }
                let mut prior = 0usize;
                while prior < sampler_index {
                    if self.sampler_at(prior)?.sampler_id == sampler.sampler_id {
                        return Err(Error::DuplicateId);
                    }
                    prior += 1;
                }
                sampler_index += 1;
            }

            let mut texture_index = 0usize;
            while texture_index < self.texture_binding_count {
                let binding = self.texture_binding_at(texture_index)?;
                if !binding.is_valid() {
                    return Err(Error::InvalidBinding);
                }
                let view = self
                    .view_by_id(binding.view_id)
                    .ok_or(Error::MissingResource)?;
                if binding.access != view.access {
                    return Err(Error::BindingTargetMismatch);
                }
                let mut prior = 0usize;
                while prior < texture_index {
                    let other = self.texture_binding_at(prior)?;
                    if same_binding_slot(
                        binding.pipeline_id,
                        binding.bind_group_id,
                        binding.stage,
                        binding.group,
                        binding.binding,
                        other.pipeline_id,
                        other.bind_group_id,
                        other.stage,
                        other.group,
                        other.binding,
                    ) || (binding.pipeline_id == other.pipeline_id
                        && binding.bind_group_id == other.bind_group_id
                        && binding.stage == other.stage
                        && binding.binding_table_index == other.binding_table_index)
                    {
                        return Err(Error::InvalidBinding);
                    }
                    prior += 1;
                }
                let mut sampler_binding_index = 0usize;
                while sampler_binding_index < self.sampler_binding_count {
                    let other = self.sampler_binding_at(sampler_binding_index)?;
                    if same_binding_slot(
                        binding.pipeline_id,
                        binding.bind_group_id,
                        binding.stage,
                        binding.group,
                        binding.binding,
                        other.pipeline_id,
                        other.bind_group_id,
                        other.stage,
                        other.group,
                        other.binding,
                    ) {
                        return Err(Error::InvalidBinding);
                    }
                    sampler_binding_index += 1;
                }
                texture_index += 1;
            }

            sampler_index = 0;
            while sampler_index < self.sampler_binding_count {
                let binding = self.sampler_binding_at(sampler_index)?;
                if !binding.is_valid()
                    || self.sampler_by_id(binding.sampler_id).is_none()
                {
                    return Err(Error::InvalidBinding);
                }
                let mut prior = 0usize;
                while prior < sampler_index {
                    let other = self.sampler_binding_at(prior)?;
                    if same_binding_slot(
                        binding.pipeline_id,
                        binding.bind_group_id,
                        binding.stage,
                        binding.group,
                        binding.binding,
                        other.pipeline_id,
                        other.bind_group_id,
                        other.stage,
                        other.group,
                        other.binding,
                    ) || (binding.pipeline_id == other.pipeline_id
                        && binding.bind_group_id == other.bind_group_id
                        && binding.stage == other.stage
                        && binding.sampler_table_index == other.sampler_table_index)
                    {
                        return Err(Error::InvalidBinding);
                    }
                    prior += 1;
                }
                sampler_index += 1;
            }

            view_index = 0;
            while view_index < self.view_count {
                let view = self.view_at(view_index)?;
                let mut bound = false;
                texture_index = 0;
                while texture_index < self.texture_binding_count {
                    if self.texture_binding_at(texture_index)?.view_id == view.view_id {
                        bound = true;
                        break;
                    }
                    texture_index += 1;
                }
                if !bound {
                    return Err(Error::MissingBinding);
                }
                view_index += 1;
            }

            let mut declared_sampler_index = 0usize;
            while declared_sampler_index < self.sampler_count {
                let sampler = self.sampler_at(declared_sampler_index)?;
                let mut bound = false;
                sampler_index = 0;
                while sampler_index < self.sampler_binding_count {
                    if self.sampler_binding_at(sampler_index)?.sampler_id == sampler.sampler_id {
                        bound = true;
                        break;
                    }
                    sampler_index += 1;
                }
                if !bound {
                    return Err(Error::MissingBinding);
                }
                declared_sampler_index += 1;
            }
            Ok(())
        }

        fn volume_at(self, index: usize) -> Result<VolumeRecord, Error> {
            let offset = indexed_offset(
                self.volume_offset,
                index,
                self.volume_count,
                VOLUME_RECORD_LEN,
            )?;
            if read_u32(self.bytes, offset + 36)? != 0 {
                return Err(Error::NonZeroReserved);
            }
            Ok(VolumeRecord {
                resource_id: read_u32(self.bytes, offset)?,
                width: read_u32(self.bytes, offset + 4)?,
                height: read_u32(self.bytes, offset + 8)?,
                depth: read_u32(self.bytes, offset + 12)?,
                row_pitch_bytes: read_u32(self.bytes, offset + 16)?,
                slice_pitch_bytes: read_u32(self.bytes, offset + 20)?,
                format: TextureFormat::from_raw(read_u16(self.bytes, offset + 24)?)
                    .ok_or(Error::InvalidEnum)?,
                dimension: TextureDimension::from_raw(read_u16(self.bytes, offset + 26)?)
                    .ok_or(Error::InvalidEnum)?,
                cache_policy: CachePolicy::from_raw(read_u16(self.bytes, offset + 28)?)
                    .ok_or(Error::InvalidEnum)?,
                mapping_lifetime: MappingLifetime::from_raw(read_u16(self.bytes, offset + 30)?)
                    .ok_or(Error::InvalidEnum)?,
                usage_flags: read_u32(self.bytes, offset + 32)?,
            })
        }

        fn view_at(self, index: usize) -> Result<ViewRecord, Error> {
            let offset = indexed_offset(
                self.view_offset,
                index,
                self.view_count,
                VIEW_RECORD_LEN,
            )?;
            if read_u16(self.bytes, offset + 22)? != 0 {
                return Err(Error::NonZeroReserved);
            }
            Ok(ViewRecord {
                view_id: read_u32(self.bytes, offset)?,
                resource_id: read_u32(self.bytes, offset + 4)?,
                access: ViewAccess::from_raw(read_u16(self.bytes, offset + 8)?)
                    .ok_or(Error::InvalidEnum)?,
                format: TextureFormat::from_raw(read_u16(self.bytes, offset + 10)?)
                    .ok_or(Error::InvalidEnum)?,
                dimension: TextureDimension::from_raw(read_u16(self.bytes, offset + 12)?)
                    .ok_or(Error::InvalidEnum)?,
                base_mip_level: read_u16(self.bytes, offset + 14)?,
                mip_level_count: read_u16(self.bytes, offset + 16)?,
                base_array_layer: read_u16(self.bytes, offset + 18)?,
                array_layer_count: read_u16(self.bytes, offset + 20)?,
            })
        }

        fn sampler_at(self, index: usize) -> Result<SamplerRecord, Error> {
            let offset = indexed_offset(
                self.sampler_offset,
                index,
                self.sampler_count,
                SAMPLER_RECORD_LEN,
            )?;
            if read_u16(self.bytes, offset + 18)? != 0
                || read_u16(self.bytes, offset + 22)? != 0
            {
                return Err(Error::NonZeroReserved);
            }
            Ok(SamplerRecord {
                sampler_id: read_u32(self.bytes, offset)?,
                address_u: AddressMode::from_raw(read_u16(self.bytes, offset + 4)?)
                    .ok_or(Error::InvalidEnum)?,
                address_v: AddressMode::from_raw(read_u16(self.bytes, offset + 6)?)
                    .ok_or(Error::InvalidEnum)?,
                address_w: AddressMode::from_raw(read_u16(self.bytes, offset + 8)?)
                    .ok_or(Error::InvalidEnum)?,
                min_filter: FilterMode::from_raw(read_u16(self.bytes, offset + 10)?)
                    .ok_or(Error::InvalidEnum)?,
                mag_filter: FilterMode::from_raw(read_u16(self.bytes, offset + 12)?)
                    .ok_or(Error::InvalidEnum)?,
                mip_filter: FilterMode::from_raw(read_u16(self.bytes, offset + 14)?)
                    .ok_or(Error::InvalidEnum)?,
                coordinate_mode: CoordinateMode::from_raw(read_u16(self.bytes, offset + 16)?)
                    .ok_or(Error::InvalidEnum)?,
                max_anisotropy: read_u16(self.bytes, offset + 20)?,
            })
        }

        fn texture_binding_at(self, index: usize) -> Result<TextureBindingRecord, Error> {
            let offset = indexed_offset(
                self.texture_binding_offset,
                index,
                self.texture_binding_count,
                TEXTURE_BINDING_RECORD_LEN,
            )?;
            if read_u16(self.bytes, offset + 22)? != 0 {
                return Err(Error::NonZeroReserved);
            }
            Ok(TextureBindingRecord {
                pipeline_id: read_u32(self.bytes, offset)?,
                bind_group_id: read_u32(self.bytes, offset + 4)?,
                view_id: read_u32(self.bytes, offset + 8)?,
                stage: ShaderStage::from_raw(read_u16(self.bytes, offset + 12)?)
                    .ok_or(Error::InvalidEnum)?,
                group: read_u16(self.bytes, offset + 14)?,
                binding: read_u16(self.bytes, offset + 16)?,
                binding_table_index: read_u16(self.bytes, offset + 18)?,
                access: ViewAccess::from_raw(read_u16(self.bytes, offset + 20)?)
                    .ok_or(Error::InvalidEnum)?,
            })
        }

        fn sampler_binding_at(self, index: usize) -> Result<SamplerBindingRecord, Error> {
            let offset = indexed_offset(
                self.sampler_binding_offset,
                index,
                self.sampler_binding_count,
                SAMPLER_BINDING_RECORD_LEN,
            )?;
            if read_u32(self.bytes, offset + 20)? != 0 {
                return Err(Error::NonZeroReserved);
            }
            Ok(SamplerBindingRecord {
                pipeline_id: read_u32(self.bytes, offset)?,
                bind_group_id: read_u32(self.bytes, offset + 4)?,
                sampler_id: read_u32(self.bytes, offset + 8)?,
                stage: ShaderStage::from_raw(read_u16(self.bytes, offset + 12)?)
                    .ok_or(Error::InvalidEnum)?,
                group: read_u16(self.bytes, offset + 14)?,
                binding: read_u16(self.bytes, offset + 16)?,
                sampler_table_index: read_u16(self.bytes, offset + 18)?,
            })
        }
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum Error {
        BadMagic,
        UnsupportedVersion(u16),
        MalformedHeader,
        LengthMismatch,
        OutOfBounds,
        NonZeroReserved,
        TooManyRecords,
        InvalidEnum,
        InvalidVolume,
        InvalidView,
        InvalidSampler,
        InvalidBinding,
        DuplicateId,
        MissingResource,
        MissingView,
        MissingBinding,
        BindingTargetMismatch,
        MissingSection,
        WrongSectionKind,
        MissingCompilerMetadata,
        WrongCompilerMetadataKind,
        MissingCompilerMetadataHash,
        CompilerMetadataHashMismatch,
        InvalidRuntimeAllocation,
    }

    fn same_binding_slot(
        left_pipeline: u32,
        left_group_id: u32,
        left_stage: ShaderStage,
        left_group: u16,
        left_binding: u16,
        right_pipeline: u32,
        right_group_id: u32,
        right_stage: ShaderStage,
        right_group: u16,
        right_binding: u16,
    ) -> bool {
        left_pipeline == right_pipeline
            && left_group_id == right_group_id
            && left_stage == right_stage
            && left_group == right_group
            && left_binding == right_binding
    }

    fn record_end(offset: usize, count: usize, stride: usize) -> Result<usize, Error> {
        offset
            .checked_add(count.checked_mul(stride).ok_or(Error::OutOfBounds)?)
            .ok_or(Error::OutOfBounds)
    }

    fn indexed_offset(
        base: usize,
        index: usize,
        count: usize,
        stride: usize,
    ) -> Result<usize, Error> {
        if index >= count {
            return Err(Error::OutOfBounds);
        }
        base.checked_add(index.checked_mul(stride).ok_or(Error::OutOfBounds)?)
            .ok_or(Error::OutOfBounds)
    }

    fn to_usize(value: u32) -> Result<usize, Error> {
        usize::try_from(value).map_err(|_| Error::OutOfBounds)
    }

    fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, Error> {
        let end = offset.checked_add(2).ok_or(Error::OutOfBounds)?;
        let raw = bytes.get(offset..end).ok_or(Error::OutOfBounds)?;
        Ok(u16::from_le_bytes([raw[0], raw[1]]))
    }

    fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Error> {
        let end = offset.checked_add(4).ok_or(Error::OutOfBounds)?;
        let raw = bytes.get(offset..end).ok_or(Error::OutOfBounds)?;
        Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
    }
