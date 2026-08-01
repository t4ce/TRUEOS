use super::*;
use sha2::{Digest, Sha256};
use std::{vec, vec::Vec};

struct TestSection<'a> {
    kind: u16,
    name: &'a str,
    data: &'a [u8],
}

fn fixture(sections: &[TestSection<'_>]) -> Vec<u8> {
    let toc_len = sections
        .iter()
        .map(|section| align_8(ENTRY_FIXED_LEN + section.name.len()).unwrap())
        .sum::<usize>();
    let payload_offset = HEADER_LEN + toc_len;
    let payload_len = sections
        .iter()
        .map(|section| section.data.len())
        .sum::<usize>();
    let mut bytes = vec![0u8; payload_offset + payload_len];
    bytes[..8].copy_from_slice(&MAGIC);
    put_u16(&mut bytes, 8, FORMAT_VERSION);
    put_u16(&mut bytes, 10, HEADER_LEN as u16);
    put_u32(&mut bytes, 12, sections.len() as u32);
    put_u64(&mut bytes, 16, toc_len as u64);
    put_u64(&mut bytes, 24, payload_offset as u64);

    let mut toc = HEADER_LEN;
    let mut payload = payload_offset;
    for section in sections {
        put_u16(&mut bytes, toc, section.name.len() as u16);
        put_u16(&mut bytes, toc + 2, section.kind);
        put_u64(&mut bytes, toc + 8, payload as u64);
        put_u64(&mut bytes, toc + 16, section.data.len() as u64);
        put_u32(&mut bytes, toc + 24, crc32fast::hash(section.data));
        bytes[toc + ENTRY_FIXED_LEN..toc + ENTRY_FIXED_LEN + section.name.len()]
            .copy_from_slice(section.name.as_bytes());
        toc = align_8(toc + ENTRY_FIXED_LEN + section.name.len()).unwrap();
        bytes[payload..payload + section.data.len()].copy_from_slice(section.data);
        payload += section.data.len();
    }
    bytes
}

fn base_fixture() -> Vec<u8> {
    fixture(&[
        TestSection {
            kind: 1,
            name: "manifest.json",
            data: br#"{"schema":1}"#,
        },
        TestSection {
            kind: 2,
            name: "wgpu/trace.ron",
            data: b"draw_indexed",
        },
        TestSection {
            kind: 77,
            name: "future/optional.bin",
            data: b"newer extension",
        },
    ])
}

#[test]
fn parses_helioa_v1_without_allocation_api() {
    let bytes = base_fixture();
    let artifact = Artifact::parse(&bytes).unwrap();
    assert_eq!(artifact.section_count(), 3);
    assert_eq!(
        artifact.section("wgpu/trace.ron"),
        Some(Section {
            kind: SectionKind::WgpuTrace,
            name: "wgpu/trace.ron",
            data: b"draw_indexed",
        })
    );
    assert_eq!(artifact.section("future/optional.bin").unwrap().kind, SectionKind::Unknown(77));
    assert_eq!(artifact.sections().len(), 3);
}

#[test]
fn required_sections_check_name_and_kind() {
    let bytes = base_fixture();
    let artifact = Artifact::parse(&bytes).unwrap();
    artifact
        .require_all(&[
            RequiredSection::new("manifest.json", SectionKind::Manifest),
            RequiredSection::new("wgpu/trace.ron", SectionKind::WgpuTrace),
        ])
        .unwrap();
    assert_eq!(
        artifact
            .require(RequiredSection::new("wgpu/trace.ron", SectionKind::ShaderSource,))
            .unwrap_err(),
        Error::WrongSectionKind {
            expected: SectionKind::ShaderSource,
            actual: SectionKind::WgpuTrace,
        }
    );
    assert_eq!(
        artifact
            .require(RequiredSection::new("missing", SectionKind::WgpuTrace))
            .unwrap_err(),
        Error::MissingSection
    );
}

#[test]
fn rejects_corrupt_payload() {
    let mut bytes = base_fixture();
    *bytes.last_mut().unwrap() ^= 0x80;
    assert_eq!(Artifact::parse(&bytes).unwrap_err(), Error::ChecksumMismatch);
}

#[test]
fn rejects_truncated_toc_and_payload() {
    let bytes = base_fixture();
    for len in 0..bytes.len() {
        let result = Artifact::parse(&bytes[..len]);
        assert!(result.is_err(), "unexpected success for truncation at {len}");
    }
    Artifact::parse(&bytes).unwrap();
}

#[test]
fn rejects_duplicate_names() {
    let bytes = fixture(&[
        TestSection {
            kind: 1,
            name: "manifest.json",
            data: b"a",
        },
        TestSection {
            kind: 2,
            name: "manifest.json",
            data: b"b",
        },
    ]);
    assert_eq!(Artifact::parse(&bytes).unwrap_err(), Error::DuplicateName);
}

#[test]
fn rejects_overlapping_payloads_even_with_valid_checksums() {
    let mut bytes = fixture(&[
        TestSection {
            kind: 1,
            name: "manifest.json",
            data: b"same",
        },
        TestSection {
            kind: 2,
            name: "wgpu/trace.ron",
            data: b"same",
        },
    ]);
    let first_offset = read_u64(&bytes, HEADER_LEN + 8).unwrap();
    let second_toc = align_8(HEADER_LEN + ENTRY_FIXED_LEN + "manifest.json".len()).unwrap();
    put_u64(&mut bytes, second_toc + 8, first_offset);
    assert_eq!(Artifact::parse(&bytes).unwrap_err(), Error::OverlappingSections);
}

#[test]
fn rejects_bad_names_and_missing_manifest() {
    for bad_name in ["", "/root", "../escape", "dir\\file"] {
        let bytes = fixture(&[TestSection {
            kind: 1,
            name: bad_name,
            data: b"x",
        }]);
        assert_eq!(Artifact::parse(&bytes).unwrap_err(), Error::InvalidName);
    }

    let bytes = fixture(&[TestSection {
        kind: 2,
        name: "wgpu/trace.ron",
        data: b"x",
    }]);
    assert_eq!(Artifact::parse(&bytes).unwrap_err(), Error::MissingManifest);
}

#[test]
fn rejects_impossible_count_before_iteration() {
    let mut bytes = base_fixture();
    put_u32(&mut bytes, 12, u32::MAX);
    assert_eq!(Artifact::parse(&bytes).unwrap_err(), Error::MalformedHeader);
}

#[test]
fn section_kind_round_trips_raw_values() {
    for raw in [0, 1, 2, 3, 4, 5, 6, 7, 8, 77, u16::MAX] {
        assert_eq!(SectionKind::from_raw(raw).raw(), raw);
    }
}

#[test]
fn opens_authenticated_churn_forward_program() {
    let source = b"Helio Churn forward WGSL";
    let vs = b"vertex00";
    let ps = b"pixel000";
    let descriptor = churn_descriptor(source, vs, ps);
    let bytes = fixture(&[
        TestSection {
            kind: 1,
            name: "manifest.json",
            data: b"{}",
        },
        TestSection {
            kind: 8,
            name: churn_forward::SECTION_NAME,
            data: &descriptor,
        },
        TestSection {
            kind: 3,
            name: churn_forward::SHADER_SOURCE_SECTION,
            data: source,
        },
        TestSection {
            kind: 4,
            name: churn_forward::VERTEX_ISA_SECTION,
            data: vs,
        },
        TestSection {
            kind: 4,
            name: churn_forward::FRAGMENT_ISA_SECTION,
            data: ps,
        },
    ]);
    let program = Artifact::parse(&bytes)
        .unwrap()
        .churn_forward_program()
        .unwrap();
    assert_eq!(program.camera_layout().stride, 368);
    assert_eq!(program.camera_layout().prev_view_proj, 304);
    assert_eq!(program.instance_layout().material_id, 196);
    assert_eq!(program.instance_layout().lightmap_index, 204);
    assert_eq!(program.indirect_layout().stride, 20);
    assert_eq!(program.vertex_fetch().attributes[1].offset, 12);
    assert_eq!(program.vertex_fetch().vf_component_packing_dw0, 0x0000_0a77);
    assert_eq!(program.vertex_fetch().packed_vs_input_count, 8);
    assert_eq!(program.bindings()[2].intel_bti, 3);
    assert_eq!(program.vertex_stage().grf_start_register, 2);
    assert_eq!(program.vertex_stage().urb_entry_output_length, 1);
    assert_eq!(program.fragment_stage().grf_start_register, 4);
    assert_eq!(program.fragment_stage().num_varying_inputs, 2);
    assert!(program.fragment_stage().uses_vmask);
    assert_eq!(program.fragment_stage().flat_inputs, 2);
    assert_eq!(program.sgvs().vf_sgvs_dw1, 0xe002_4002);
    assert_eq!(program.vf_instancing()[0].element_index, 0);
    assert!(!program.vf_instancing()[1].enabled);
    assert_eq!(program.vf_instancing()[2].element_index, 2);
    assert_eq!(program.synthetic_instance_id_element().vertex_buffer_index, 31);
}

#[test]
fn published_churn_forward_artifact_authenticates() {
    let bytes = include_bytes!("../../../assets/helio/churn-forward.trueos.intel.helio");
    let program = Artifact::parse(bytes)
        .unwrap()
        .churn_forward_program()
        .unwrap();
    assert_eq!(program.vertex_stage().code_size_bytes, 912);
    assert_eq!(program.fragment_stage().code_size_bytes, 736);
    assert_eq!(program.shader_source().byte_len, 2_852);
}

#[test]
fn churn_forward_rejects_layout_corruption() {
    let mut descriptor = churn_descriptor(b"source", b"vert", b"frag");
    put_u32(&mut descriptor, 32, 256);
    let bytes = fixture(&[
        TestSection {
            kind: 1,
            name: "manifest.json",
            data: b"{}",
        },
        TestSection {
            kind: 8,
            name: churn_forward::SECTION_NAME,
            data: &descriptor,
        },
    ]);
    assert_eq!(
        Artifact::parse(&bytes)
            .unwrap()
            .churn_forward_program()
            .unwrap_err(),
        Error::InvalidChurnForward(churn_forward::Error::InvalidLayout)
    );
}

#[test]
fn churn_forward_rejects_reference_kind_and_hash() {
    let source = b"source";
    let vs = b"vert";
    let ps = b"frag";
    let descriptor = churn_descriptor(source, vs, ps);
    let wrong_kind = fixture(&[
        TestSection {
            kind: 1,
            name: "manifest.json",
            data: b"{}",
        },
        TestSection {
            kind: 8,
            name: churn_forward::SECTION_NAME,
            data: &descriptor,
        },
        TestSection {
            kind: 4,
            name: churn_forward::SHADER_SOURCE_SECTION,
            data: source,
        },
        TestSection {
            kind: 4,
            name: churn_forward::VERTEX_ISA_SECTION,
            data: vs,
        },
        TestSection {
            kind: 4,
            name: churn_forward::FRAGMENT_ISA_SECTION,
            data: ps,
        },
    ]);
    assert_eq!(
        Artifact::parse(&wrong_kind)
            .unwrap()
            .churn_forward_program()
            .unwrap_err(),
        Error::WrongChurnForwardReferenceKind
    );

    let wrong_hash = fixture(&[
        TestSection {
            kind: 1,
            name: "manifest.json",
            data: b"{}",
        },
        TestSection {
            kind: 8,
            name: churn_forward::SECTION_NAME,
            data: &descriptor,
        },
        TestSection {
            kind: 3,
            name: churn_forward::SHADER_SOURCE_SECTION,
            data: source,
        },
        TestSection {
            kind: 4,
            name: churn_forward::VERTEX_ISA_SECTION,
            data: b"xxxx",
        },
        TestSection {
            kind: 4,
            name: churn_forward::FRAGMENT_ISA_SECTION,
            data: ps,
        },
    ]);
    assert_eq!(
        Artifact::parse(&wrong_hash)
            .unwrap()
            .churn_forward_program()
            .unwrap_err(),
        Error::ChurnForwardReferenceHashMismatch
    );
}

#[test]
fn opens_typed_replay_plan_section() {
    let replay = replay_fixture();
    let bytes = fixture(&[
        TestSection {
            kind: 1,
            name: "manifest.json",
            data: b"{}",
        },
        TestSection {
            kind: 7,
            name: replay::SECTION_NAME,
            data: &replay,
        },
    ]);
    let artifact = Artifact::parse(&bytes).unwrap();
    let plan = artifact.replay_plan().unwrap();
    assert_eq!(plan.command_count(), 1);
    assert_eq!(plan.commands().next().unwrap().index_count, 36);
}

#[test]
fn replay_accessor_checks_kind_and_payload() {
    let replay = replay_fixture();
    let wrong_kind = fixture(&[
        TestSection {
            kind: 1,
            name: "manifest.json",
            data: b"{}",
        },
        TestSection {
            kind: 6,
            name: replay::SECTION_NAME,
            data: &replay,
        },
    ]);
    assert_eq!(
        Artifact::parse(&wrong_kind)
            .unwrap()
            .replay_plan()
            .unwrap_err(),
        Error::WrongSectionKind {
            expected: SectionKind::RenderReplay,
            actual: SectionKind::NormalizedRenderIr,
        }
    );

    let mut invalid = replay;
    invalid[0] ^= 1;
    let bad_payload = fixture(&[
        TestSection {
            kind: 1,
            name: "manifest.json",
            data: b"{}",
        },
        TestSection {
            kind: 7,
            name: replay::SECTION_NAME,
            data: &invalid,
        },
    ]);
    assert_eq!(
        Artifact::parse(&bad_payload)
            .unwrap()
            .replay_plan()
            .unwrap_err(),
        Error::InvalidReplay(replay::Error::BadMagic)
    );
}

fn replay_fixture() -> Vec<u8> {
    let mut bytes = vec![0u8; replay::HEADER_LEN + replay::COMMAND_STRIDE];
    bytes[..8].copy_from_slice(&replay::MAGIC);
    put_u16(&mut bytes, 8, replay::VERSION);
    put_u16(&mut bytes, 10, replay::HEADER_LEN as u16);
    let total_len = bytes.len() as u32;
    put_u32(&mut bytes, 12, total_len);
    put_u32(&mut bytes, 16, 1);
    put_u32(&mut bytes, 20, replay::COMMAND_STRIDE as u32);
    put_u32(&mut bytes, 28, 0x1234_5678);
    put_u32(&mut bytes, 32, 1);
    put_u32(&mut bytes, 36, 2);
    put_u32(&mut bytes, replay::HEADER_LEN, 36);
    put_u32(&mut bytes, replay::HEADER_LEN + 4, 1);
    bytes
}

fn churn_descriptor(source: &[u8], vs: &[u8], ps: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; churn_forward::BYTE_LEN];
    bytes[..8].copy_from_slice(&churn_forward::MAGIC);
    put_u16(&mut bytes, 8, churn_forward::FORMAT_VERSION);
    put_u16(&mut bytes, 10, churn_forward::BYTE_LEN as u16);
    put_u32(&mut bytes, 12, churn_forward::BYTE_LEN as u32);
    put_u32(&mut bytes, 16, 0x3f);
    put_u16(&mut bytes, 20, 2);
    put_u16(&mut bytes, 22, 3);
    put_u16(&mut bytes, 24, 2);
    put_u32s(&mut bytes, 32, &[368, 0, 64, 128, 192, 256, 272, 288, 304, 0]);
    put_u32s(&mut bytes, 72, &[208, 0, 64, 112, 128, 192, 196, 200, 204, 64, 48, 0]);
    put_u32s(&mut bytes, 120, &[4, 20, 0, 4, 8, 12, 16, 36, 0, 0]);
    put_u32(&mut bytes, 160, 24);
    put_u16(&mut bytes, 164, 1);
    put_u16(&mut bytes, 166, 2);
    put_attribute(&mut bytes, 168, 0, 0);
    put_attribute(&mut bytes, 180, 1, 12);
    put_u32(&mut bytes, 196, 24);
    put_binding(&mut bytes, 208, 0, 1, 368);
    put_binding(&mut bytes, 224, 1, 2, 208);
    put_binding(&mut bytes, 240, 2, 3, 4);
    for offset in (256..=270).step_by(2) {
        put_u16(&mut bytes, offset, 1);
    }
    put_u32(&mut bytes, 272, 1);
    put_u32(&mut bytes, 276, 0xf);
    bytes[282] = 1;
    bytes[283] = 1;
    put_u16(&mut bytes, 284, 2);
    put_stage(&mut bytes, 288, 1, vs, b"vs_main", churn_forward::VERTEX_ISA_SECTION.as_bytes());
    put_stage(&mut bytes, 448, 2, ps, b"fs_main", churn_forward::FRAGMENT_ISA_SECTION.as_bytes());
    put_u32(&mut bytes, 608, source.len() as u32);
    put_u16(&mut bytes, 612, churn_forward::SHADER_SOURCE_SECTION.len() as u16);
    bytes[616..648].copy_from_slice(&Sha256::digest(source));
    bytes[648..648 + churn_forward::SHADER_SOURCE_SECTION.len()]
        .copy_from_slice(churn_forward::SHADER_SOURCE_SECTION.as_bytes());
    put_u32(&mut bytes, 704, 0xe002_4002);
    put_u32(&mut bytes, 708, 0xb002_0002);
    put_u32(&mut bytes, 712, 3);
    put_u16(&mut bytes, 716, 3);
    put_u16(&mut bytes, 720, 0);
    put_u16(&mut bytes, 728, 1);
    put_u16(&mut bytes, 736, 2);
    put_u16(&mut bytes, 744, 2);
    bytes[746] = 31;
    put_u16(&mut bytes, 748, 135);
    bytes[750..754].copy_from_slice(&[2; 4]);
    put_u32(&mut bytes, 756, 0x0000_0a77);
    put_u16(&mut bytes, 760, 8);
    put_u16(&mut bytes, 762, 1);
    bytes
}

fn put_stage(bytes: &mut [u8], offset: usize, stage: u16, code: &[u8], entry: &[u8], name: &[u8]) {
    put_u16(bytes, offset, stage);
    put_u16(bytes, offset + 2, 8);
    put_u32(bytes, offset + 4, code.len() as u32);
    put_u32(bytes, offset + 12, 64);
    put_u16(bytes, offset + 20, if stage == 1 { 2 } else { 4 });
    put_u16(bytes, offset + 22, 128);
    put_u16(bytes, offset + 24, 64);
    put_u16(bytes, offset + 26, if stage == 1 { 4 } else { 1 });
    put_u16(bytes, offset + 32, if stage == 1 { 1 } else { 0 });
    put_u16(bytes, offset + 34, if stage == 1 { 0 } else { 2 });
    put_u32(bytes, offset + 36, if stage == 1 { 0 } else { 1 });
    put_u32(bytes, offset + 40, if stage == 1 { 0 } else { 2 });
    bytes[offset + 48..offset + 80].copy_from_slice(&Sha256::digest(code));
    put_u16(bytes, offset + 80, entry.len() as u16);
    put_u16(bytes, offset + 82, name.len() as u16);
    bytes[offset + 88..offset + 88 + entry.len()].copy_from_slice(entry);
    bytes[offset + 104..offset + 104 + name.len()].copy_from_slice(name);
}

fn put_attribute(bytes: &mut [u8], offset: usize, location: u16, byte_offset: u32) {
    put_u16(bytes, offset, location);
    put_u16(bytes, offset + 2, 1);
    put_u32(bytes, offset + 4, byte_offset);
    put_u32(bytes, offset + 8, 0x7);
}

fn put_binding(bytes: &mut [u8], offset: usize, binding: u8, bti: u8, size: u32) {
    bytes[offset + 1] = binding;
    bytes[offset + 2] = bti;
    bytes[offset + 3] = 1;
    bytes[offset + 4] = 1;
    bytes[offset + 5] = 1;
    put_u32(bytes, offset + 8, size);
    put_u32(bytes, offset + 12, size);
}

fn put_u32s(bytes: &mut [u8], offset: usize, values: &[u32]) {
    for (index, value) in values.iter().copied().enumerate() {
        put_u32(bytes, offset + index * 4, value);
    }
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
