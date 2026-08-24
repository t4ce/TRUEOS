    #[cfg(test)]
    mod tests {
        use super::*;
        use alloc::vec;
        use alloc::vec::Vec;

        const PHYS: u64 = 0x0000_0000_1234_5000;
        const GPU: u64 = 0x0000_0000_4000_0000;

        #[test]
        fn opens_the_contract_from_helioa_and_verifies_compiler_metadata() {
            let metadata = br#"{\"source\":\"igc\",\"profile\":\"cloud-volume\"}
"#;
            let mut resource = cloud_fixture();
            let digest = Sha256::digest(metadata);
            resource[48..80].copy_from_slice(&digest);
            let artifact_bytes = helioa_fixture(&resource, metadata);
            let artifact = Artifact::parse(&artifact_bytes).expect("validated HELIOA");
            let program = Program::parse_artifact(artifact).expect("hash-bound volume contract");
            assert!(program.is_helio_cloud_profile());
            assert_eq!(&program.compiler_metadata_sha256()[..], &digest[..]);
        }

        #[test]
        fn rejects_a_helioa_contract_bound_to_different_compiler_metadata() {
            let metadata = br#"{\"source\":\"igc\",\"profile\":\"cloud-volume\"}
"#;
            let resource = cloud_fixture();
            let artifact_bytes = helioa_fixture(&resource, metadata);
            let artifact = Artifact::parse(&artifact_bytes).expect("validated HELIOA");
            assert!(matches!(
                Program::parse_artifact(artifact),
                Err(Error::CompilerMetadataHashMismatch)
            ));
        }

        #[test]
        fn rejects_a_resource_section_without_a_compiler_metadata_digest() {
            let mut bytes = cloud_fixture();
            bytes[48..80].fill(0);
            assert!(matches!(
                Program::parse(&bytes),
                Err(Error::MissingCompilerMetadataHash)
            ));
        }

        #[test]
        fn parses_cloud_pair_and_resolves_one_allocation_per_volume() {
            let bytes = cloud_fixture();
            let program = Program::parse(&bytes).expect("cloud volume resource contract");
            assert!(program.is_helio_cloud_profile());
            assert_eq!(program.volume_count(), 2);
            assert_eq!(program.view_count(), 4);
            assert_eq!(program.sampler_count(), 1);
            assert_eq!(program.texture_binding_count(), 6);
            assert_eq!(program.sampler_binding_count(), 4);

            let resolved = program
                .resolve_volume(1, PHYS, GPU, 3_538_944)
                .expect("resolve cloud volume A");
            assert_eq!(resolved.allocation.width, 96);
            assert_eq!(resolved.allocation.height, 48);
            assert_eq!(resolved.allocation.depth, 96);
            assert_eq!(resolved.allocation.row_pitch_bytes, 768);
            assert_eq!(resolved.allocation.slice_pitch_bytes, 36_864);
            assert_eq!(resolved.sampled_view.resource_id, resolved.descriptor.resource_id);
            assert_eq!(resolved.storage_view.resource_id, resolved.descriptor.resource_id);
            assert_eq!(resolved.sampled_view.access, ViewAccess::Sampled);
            assert_eq!(resolved.storage_view.access, ViewAccess::StorageWriteOnly);
            assert_eq!(resolved.descriptor.cache_policy, CachePolicy::WriteBack);
            assert_eq!(resolved.descriptor.mapping_lifetime, MappingLifetime::Artifact);
        }

        #[test]
        fn clamp_all_axes_remains_a_probe_not_the_final_cloud_abi() {
            let mut bytes = cloud_fixture();
            let sampler_offset = read_u32(&bytes, 36).unwrap() as usize;
            put_u16(&mut bytes, sampler_offset + 4, AddressMode::ClampToEdge as u16);
            put_u16(&mut bytes, sampler_offset + 8, AddressMode::ClampToEdge as u16);
            let program = Program::parse(&bytes).expect("generic clamp sampler remains valid");
            assert!(!program.is_helio_cloud_profile());
        }

        #[test]
        fn rejects_a_volume_without_distinct_sampled_and_storage_views() {
            let mut bytes = cloud_fixture();
            let view_offset = read_u32(&bytes, 32).unwrap() as usize;
            put_u16(&mut bytes, view_offset + VIEW_RECORD_LEN + 8, ViewAccess::Sampled as u16);
            assert!(matches!(
                Program::parse(&bytes),
                Err(Error::MissingView)
            ));
        }

        #[test]
        fn rejects_binding_table_collisions_inside_one_bound_variant() {
            let mut bytes = cloud_fixture();
            let binding_offset = read_u32(&bytes, 40).unwrap() as usize;
            let sampled_bti = read_u16(&bytes, binding_offset + 18).unwrap();
            put_u16(
                &mut bytes,
                binding_offset + TEXTURE_BINDING_RECORD_LEN + 18,
                sampled_bti,
            );
            assert!(matches!(
                Program::parse(&bytes),
                Err(Error::InvalidBinding)
            ));
        }

        #[test]
        fn rejects_runtime_backing_smaller_than_the_artifact_layout() {
            let bytes = cloud_fixture();
            let program = Program::parse(&bytes).unwrap();
            assert_eq!(
                program.resolve_volume(1, PHYS, GPU, 3_538_943),
                Err(Error::InvalidRuntimeAllocation)
            );
        }

        fn cloud_fixture() -> Vec<u8> {
            const VOLUMES: usize = 2;
            const VIEWS: usize = 4;
            const SAMPLERS: usize = 1;
            const TEXTURE_BINDINGS: usize = 6;
            const SAMPLER_BINDINGS: usize = 4;

            let volume_offset = HEADER_LEN;
            let view_offset = volume_offset + VOLUMES * VOLUME_RECORD_LEN;
            let sampler_offset = view_offset + VIEWS * VIEW_RECORD_LEN;
            let texture_binding_offset = sampler_offset + SAMPLERS * SAMPLER_RECORD_LEN;
            let sampler_binding_offset =
                texture_binding_offset + TEXTURE_BINDINGS * TEXTURE_BINDING_RECORD_LEN;
            let total_len =
                sampler_binding_offset + SAMPLER_BINDINGS * SAMPLER_BINDING_RECORD_LEN;
            let mut bytes = vec![0u8; total_len];
            bytes[..8].copy_from_slice(&MAGIC);
            put_u16(&mut bytes, 8, VERSION);
            put_u16(&mut bytes, 10, HEADER_LEN as u16);
            put_u32(&mut bytes, 12, total_len as u32);
            put_u16(&mut bytes, 16, VOLUMES as u16);
            put_u16(&mut bytes, 18, VIEWS as u16);
            put_u16(&mut bytes, 20, SAMPLERS as u16);
            put_u16(&mut bytes, 22, TEXTURE_BINDINGS as u16);
            put_u16(&mut bytes, 24, SAMPLER_BINDINGS as u16);
            put_u32(&mut bytes, 28, volume_offset as u32);
            put_u32(&mut bytes, 32, view_offset as u32);
            put_u32(&mut bytes, 36, sampler_offset as u32);
            put_u32(&mut bytes, 40, texture_binding_offset as u32);
            put_u32(&mut bytes, 44, sampler_binding_offset as u32);
            bytes[48..80].fill(0xA5);

            write_volume(&mut bytes, volume_offset, 1);
            write_volume(&mut bytes, volume_offset + VOLUME_RECORD_LEN, 2);
            write_view(&mut bytes, view_offset, 11, 1, ViewAccess::Sampled);
            write_view(
                &mut bytes,
                view_offset + VIEW_RECORD_LEN,
                12,
                1,
                ViewAccess::StorageWriteOnly,
            );
            write_view(
                &mut bytes,
                view_offset + VIEW_RECORD_LEN * 2,
                21,
                2,
                ViewAccess::Sampled,
            );
            write_view(
                &mut bytes,
                view_offset + VIEW_RECORD_LEN * 3,
                22,
                2,
                ViewAccess::StorageWriteOnly,
            );
            write_sampler(&mut bytes, sampler_offset, 31);

            let texture_records = [
                (100, 1_000, 11, ShaderStage::Compute, 0, 1, 4, ViewAccess::Sampled),
                (100, 1_000, 22, ShaderStage::Compute, 0, 3, 5, ViewAccess::StorageWriteOnly),
                (100, 1_001, 21, ShaderStage::Compute, 0, 1, 4, ViewAccess::Sampled),
                (100, 1_001, 12, ShaderStage::Compute, 0, 3, 5, ViewAccess::StorageWriteOnly),
                (200, 2_000, 11, ShaderStage::Fragment, 0, 1, 6, ViewAccess::Sampled),
                (200, 2_001, 21, ShaderStage::Fragment, 0, 1, 6, ViewAccess::Sampled),
            ];
            for (index, record) in texture_records.into_iter().enumerate() {
                write_texture_binding(
                    &mut bytes,
                    texture_binding_offset + index * TEXTURE_BINDING_RECORD_LEN,
                    record,
                );
            }
            let sampler_records = [
                (100, 1_000, 31, ShaderStage::Compute, 0, 2, 1),
                (100, 1_001, 31, ShaderStage::Compute, 0, 2, 1),
                (200, 2_000, 31, ShaderStage::Fragment, 0, 2, 2),
                (200, 2_001, 31, ShaderStage::Fragment, 0, 2, 2),
            ];
            for (index, record) in sampler_records.into_iter().enumerate() {
                write_sampler_binding(
                    &mut bytes,
                    sampler_binding_offset + index * SAMPLER_BINDING_RECORD_LEN,
                    record,
                );
            }
            bytes
        }

        fn helioa_fixture(resource: &[u8], metadata: &[u8]) -> Vec<u8> {
            let sections: [(&str, u16, &[u8]); 3] = [
                ("manifest.json", SectionKind::Manifest.raw(), b"{}"),
                (
                    COMPILER_METADATA_SECTION_NAME,
                    SectionKind::CompilerMetadata.raw(),
                    metadata,
                ),
                (
                    SECTION_NAME,
                    SectionKind::NormalizedRenderIr.raw(),
                    resource,
                ),
            ];
            let mut payload_offset = 32usize;
            for (name, _, _) in sections {
                payload_offset = align_8(payload_offset + 32 + name.len());
            }
            let total_len = sections.iter().fold(payload_offset, |length, (_, _, data)| {
                length.checked_add(data.len()).expect("HELIOA fixture length")
            });
            let mut bytes = vec![0u8; total_len];
            bytes[..8].copy_from_slice(b"HELIOA\0\0");
            put_u16(&mut bytes, 8, 1);
            put_u16(&mut bytes, 10, 32);
            put_u32(&mut bytes, 12, sections.len() as u32);
            put_u64(&mut bytes, 16, (payload_offset - 32) as u64);
            put_u64(&mut bytes, 24, payload_offset as u64);

            let mut toc_cursor = 32usize;
            let mut data_cursor = payload_offset;
            for (name, kind, data) in sections {
                put_u16(&mut bytes, toc_cursor, name.len() as u16);
                put_u16(&mut bytes, toc_cursor + 2, kind);
                put_u64(&mut bytes, toc_cursor + 8, data_cursor as u64);
                put_u64(&mut bytes, toc_cursor + 16, data.len() as u64);
                put_u32(&mut bytes, toc_cursor + 24, crc32fast::hash(data));
                let name_start = toc_cursor + 32;
                bytes[name_start..name_start + name.len()].copy_from_slice(name.as_bytes());
                toc_cursor = align_8(name_start + name.len());
                bytes[data_cursor..data_cursor + data.len()].copy_from_slice(data);
                data_cursor += data.len();
            }
            assert_eq!(toc_cursor, payload_offset);
            assert_eq!(data_cursor, total_len);
            bytes
        }

        const fn align_8(value: usize) -> usize {
            (value + 7) & !7
        }

        fn write_volume(bytes: &mut [u8], offset: usize, resource_id: u32) {
            put_u32(bytes, offset, resource_id);
            put_u32(bytes, offset + 4, 96);
            put_u32(bytes, offset + 8, 48);
            put_u32(bytes, offset + 12, 96);
            put_u32(bytes, offset + 16, 768);
            put_u32(bytes, offset + 20, 36_864);
            put_u16(bytes, offset + 24, TextureFormat::Rgba16Float as u16);
            put_u16(bytes, offset + 26, TextureDimension::D3 as u16);
            put_u16(bytes, offset + 28, CachePolicy::WriteBack as u16);
            put_u16(bytes, offset + 30, MappingLifetime::Artifact as u16);
            put_u32(bytes, offset + 32, KNOWN_VOLUME_USAGE);
        }

        fn write_view(
            bytes: &mut [u8],
            offset: usize,
            view_id: u32,
            resource_id: u32,
            access: ViewAccess,
        ) {
            put_u32(bytes, offset, view_id);
            put_u32(bytes, offset + 4, resource_id);
            put_u16(bytes, offset + 8, access as u16);
            put_u16(bytes, offset + 10, TextureFormat::Rgba16Float as u16);
            put_u16(bytes, offset + 12, TextureDimension::D3 as u16);
            put_u16(bytes, offset + 14, 0);
            put_u16(bytes, offset + 16, 1);
            put_u16(bytes, offset + 18, 0);
            put_u16(bytes, offset + 20, 1);
        }

        fn write_sampler(bytes: &mut [u8], offset: usize, sampler_id: u32) {
            put_u32(bytes, offset, sampler_id);
            put_u16(bytes, offset + 4, AddressMode::Repeat as u16);
            put_u16(bytes, offset + 6, AddressMode::ClampToEdge as u16);
            put_u16(bytes, offset + 8, AddressMode::Repeat as u16);
            put_u16(bytes, offset + 10, FilterMode::Linear as u16);
            put_u16(bytes, offset + 12, FilterMode::Linear as u16);
            put_u16(bytes, offset + 14, FilterMode::Nearest as u16);
            put_u16(bytes, offset + 16, CoordinateMode::Normalized as u16);
            put_u16(bytes, offset + 20, 1);
        }

        fn write_texture_binding(
            bytes: &mut [u8],
            offset: usize,
            record: (u32, u32, u32, ShaderStage, u16, u16, u16, ViewAccess),
        ) {
            put_u32(bytes, offset, record.0);
            put_u32(bytes, offset + 4, record.1);
            put_u32(bytes, offset + 8, record.2);
            put_u16(bytes, offset + 12, record.3 as u16);
            put_u16(bytes, offset + 14, record.4);
            put_u16(bytes, offset + 16, record.5);
            put_u16(bytes, offset + 18, record.6);
            put_u16(bytes, offset + 20, record.7 as u16);
        }

        fn write_sampler_binding(
            bytes: &mut [u8],
            offset: usize,
            record: (u32, u32, u32, ShaderStage, u16, u16, u16),
        ) {
            put_u32(bytes, offset, record.0);
            put_u32(bytes, offset + 4, record.1);
            put_u32(bytes, offset + 8, record.2);
            put_u16(bytes, offset + 12, record.3 as u16);
            put_u16(bytes, offset + 14, record.4);
            put_u16(bytes, offset + 16, record.5);
            put_u16(bytes, offset + 18, record.6);
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
