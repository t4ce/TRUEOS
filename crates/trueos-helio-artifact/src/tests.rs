use super::*;
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
    for raw in [0, 1, 2, 3, 4, 5, 6, 7, 77, u16::MAX] {
        assert_eq!(SectionKind::from_raw(raw).raw(), raw);
    }
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

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
