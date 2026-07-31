use std::{vec, vec::Vec};

use crate::*;

const MODEL_HASH: &str = "239d9f4df112a375bea52146570b97eb5c5af727c007761ee121ed123fd1ab29";
const VOICES_HASH: &str = "bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d";
const FIXTURE_ARTIFACT_HASH: &str =
    "0df8861b0d55f3a1d8587b0993a5588b098800c9c3006d080c19e6b90ad8df44";

struct Parts {
    tensors: Vec<[u8; TENSOR_RECORD_BYTES]>,
    slots: Vec<[u8; SLOT_RECORD_BYTES]>,
    ops: Vec<[u8; OP_RECORD_BYTES]>,
    bindings: Vec<u32>,
    phases: [[u8; PHASE_RECORD_BYTES]; PHASE_COUNT],
    data: Vec<u8>,
}

#[allow(clippy::too_many_arguments)]
fn tensor_record(
    dtype: u8,
    dims: &[u32],
    storage: u8,
    phase: u8,
    flags: u32,
    slot_id: u32,
    view_of: u32,
    storage_offset: u64,
    strides: Option<[u64; 4]>,
    symbolic_dim: u8,
    frame_multiplier: u32,
    frame_addend: i64,
    alignment: u32,
) -> [u8; TENSOR_RECORD_BYTES] {
    let element_bytes = match dtype {
        1 | 2 => 4u64,
        3 => 8,
        4..=6 => 1,
        _ => 0,
    };
    let mut max_dims = [1u32; 4];
    max_dims[..dims.len()].copy_from_slice(dims);
    let mut contiguous = [0u64; 4];
    let mut stride = element_bytes;
    for index in (0..dims.len()).rev() {
        contiguous[index] = stride;
        stride = stride.saturating_mul(u64::from(dims[index]));
    }
    let byte_capacity = dims
        .iter()
        .fold(element_bytes, |bytes, dim| bytes.saturating_mul(u64::from(*dim)));
    let strides = strides.unwrap_or(contiguous);

    let mut record = [0u8; TENSOR_RECORD_BYTES];
    record[0] = dtype;
    record[1] = dims.len() as u8;
    record[2] = storage;
    record[3] = phase;
    put_u32(&mut record, 4, flags);
    put_u32(&mut record, 8, slot_id);
    put_u32(&mut record, 12, view_of);
    put_u64(&mut record, 16, storage_offset);
    put_u64(&mut record, 24, byte_capacity);
    for index in 0..4 {
        put_u32(&mut record, 32 + index * 4, max_dims[index]);
        put_u64(&mut record, 48 + index * 8, strides[index]);
    }
    record[80] = symbolic_dim;
    put_u32(&mut record, 84, frame_multiplier);
    put_i64(&mut record, 88, frame_addend);
    put_u32(&mut record, 96, alignment);
    record
}

fn slot_record(
    kind: u8,
    phase: u8,
    alignment: u32,
    fixed_offset: u64,
    byte_multiplier: u64,
    byte_addend: i64,
    live_start: u32,
    live_end: u32,
) -> [u8; SLOT_RECORD_BYTES] {
    let mut record = [0u8; SLOT_RECORD_BYTES];
    record[0] = kind;
    record[1] = phase;
    put_u32(&mut record, 4, alignment);
    put_u64(&mut record, 8, fixed_offset);
    put_u64(&mut record, 16, byte_multiplier);
    put_i64(&mut record, 24, byte_addend);
    put_u32(&mut record, 32, live_start);
    put_u32(&mut record, 36, live_end);
    record
}

#[allow(clippy::too_many_arguments)]
fn op_record(
    opcode: u16,
    flags: u16,
    phase: u8,
    binding_start: u32,
    input_count: u16,
    output_count: u16,
    attribute_offset: u64,
    attribute_len: u32,
    work_units: u32,
) -> [u8; OP_RECORD_BYTES] {
    let mut record = [0u8; OP_RECORD_BYTES];
    put_u16(&mut record, 0, opcode);
    put_u16(&mut record, 2, flags);
    record[4] = phase;
    put_u32(&mut record, 8, binding_start);
    put_u16(&mut record, 12, input_count);
    put_u16(&mut record, 14, output_count);
    put_u64(&mut record, 16, attribute_offset);
    put_u32(&mut record, 24, attribute_len);
    put_u32(&mut record, 28, work_units);
    record
}

fn phase_record(
    phase: u8,
    flags: u8,
    op_start: u32,
    op_end: u32,
    arena_min: u64,
    arena_max: u64,
    frame_min: u32,
    frame_max: u32,
) -> [u8; PHASE_RECORD_BYTES] {
    let mut record = [0u8; PHASE_RECORD_BYTES];
    record[0] = phase;
    record[1] = flags;
    put_u32(&mut record, 4, op_start);
    put_u32(&mut record, 8, op_end);
    put_u64(&mut record, 16, arena_min);
    put_u64(&mut record, 24, arena_max);
    put_u32(&mut record, 32, ARENA_ALIGNMENT);
    put_u32(&mut record, 36, frame_min);
    put_u32(&mut record, 40, frame_max);
    record
}

fn fixture_parts() -> Parts {
    let mut data = vec![0u8; 19];
    for (index, byte) in data[..12].iter_mut().enumerate() {
        *byte = index as u8;
    }
    data[16..19].copy_from_slice(&[0x7f, 0x80, 0x01]);

    Parts {
        tensors: vec![
            tensor_record(1, &[1, 4], 4, 0, 2, NO_SLOT, NO_TENSOR, 0, None, STATIC_DIM, 0, 0, 4),
            tensor_record(5, &[4, 3], 3, 2, 1, NO_SLOT, NO_TENSOR, 0, None, STATIC_DIM, 0, 0, 16),
            tensor_record(1, &[1, 3], 1, 0, 0, 0, NO_TENSOR, 0, None, STATIC_DIM, 0, 0, 64),
            tensor_record(1, &[3], 2, 0, 0, NO_SLOT, 2, 0, None, STATIC_DIM, 0, 0, 64),
            tensor_record(3, &[], 4, 2, 0, NO_SLOT, NO_TENSOR, 0, None, STATIC_DIM, 0, 0, 8),
            tensor_record(
                5,
                &[1, 1, 3],
                3,
                2,
                1,
                NO_SLOT,
                NO_TENSOR,
                16,
                None,
                STATIC_DIM,
                0,
                0,
                16,
            ),
            tensor_record(1, &[1, 1, 64], 1, 1, 4, 1, NO_TENSOR, 0, None, 2, 2, 0, 64),
        ],
        slots: vec![
            slot_record(1, 0, 64, 0, 0, 12, 0, 2),
            slot_record(2, 1, 64, 0, 8, 0, 2, 4),
        ],
        ops: vec![
            op_record(0x0300, 0, 0, 0, 2, 1, 0, 0, 12),
            op_record(0x0001, 0, 0, 3, 1, 1, 0, 0, 1),
            op_record(0x0301, 0, 1, 5, 2, 1, 0, 0, 64),
        ],
        bindings: vec![0, 1, 2, 3, 4, 4, 5, 6],
        phases: [
            phase_record(0, 0, 0, 2, 64, 64, 0, 0),
            phase_record(1, 1, 2, 3, 64, 256, 1, 32),
        ],
        data,
    }
}

fn emit(parts: &Parts) -> Vec<u8> {
    let tensors = join_records(&parts.tensors);
    let slots = join_records(&parts.slots);
    let ops = join_records(&parts.ops);
    let mut bindings = Vec::with_capacity(parts.bindings.len() * 4);
    for binding in &parts.bindings {
        bindings.extend_from_slice(&binding.to_le_bytes());
    }
    let phases = join_records(&parts.phases);
    let sections = [tensors, slots, ops, bindings, phases, parts.data.clone()];
    let alignments = [16u32, 16, 8, 8, 8, 16];
    let strides = [128u32, 64, 40, 4, 48, 1];

    let mut artifact = vec![0u8; HEADER_BYTES];
    artifact[..8].copy_from_slice(&MAGIC);
    put_u16(&mut artifact, 8, VERSION);
    put_u16(&mut artifact, 10, LITTLE_ENDIAN_TAG);
    put_u32(&mut artifact, 12, HEADER_BYTES as u32);
    put_u16(&mut artifact, 24, SECTION_COUNT as u16);
    put_u16(&mut artifact, 26, PHASE_COUNT as u16);
    put_u32(&mut artifact, 32, ARENA_ALIGNMENT);
    put_u16(&mut artifact, 36, TENSOR_RECORD_BYTES as u16);
    put_u16(&mut artifact, 38, SLOT_RECORD_BYTES as u16);
    put_u16(&mut artifact, 40, OP_RECORD_BYTES as u16);
    put_u16(&mut artifact, 42, PHASE_RECORD_BYTES as u16);
    put_u16(&mut artifact, 44, BINDING_RECORD_BYTES as u16);
    artifact[MODEL_SHA256_OFFSET..MODEL_SHA256_OFFSET + 32].copy_from_slice(&hex32(MODEL_HASH));
    artifact[VOICES_SHA256_OFFSET..VOICES_SHA256_OFFSET + 32].copy_from_slice(&hex32(VOICES_HASH));

    for index in 0..SECTION_COUNT {
        let offset = align_up(artifact.len(), alignments[index] as usize);
        artifact.resize(offset, 0);
        artifact.extend_from_slice(&sections[index]);
        let count = if index == 5 {
            sections[index].len() as u64
        } else {
            (sections[index].len() / strides[index] as usize) as u64
        };
        let entry = SECTION_DIRECTORY_OFFSET + index * SECTION_ENTRY_BYTES;
        put_u16(&mut artifact, entry, (index + 1) as u16);
        put_u32(&mut artifact, entry + 4, alignments[index]);
        put_u64(&mut artifact, entry + 8, offset as u64);
        put_u64(&mut artifact, entry + 16, count);
        put_u32(&mut artifact, entry + 24, strides[index]);
    }
    let artifact_len = artifact.len() as u64;
    put_u64(&mut artifact, 16, artifact_len);
    reseal(&mut artifact);
    artifact
}

fn fixture() -> Vec<u8> {
    emit(&fixture_parts())
}

fn join_records<const N: usize>(records: &[[u8; N]]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(records.len() * N);
    for record in records {
        bytes.extend_from_slice(record);
    }
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

fn put_i64(bytes: &mut [u8], offset: usize, value: i64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn get_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn align_up(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
}

fn hex32(hex: &str) -> [u8; 32] {
    assert_eq!(hex.len(), 64);
    let mut bytes = [0u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).unwrap();
    }
    bytes
}

fn reseal(artifact: &mut [u8]) {
    artifact[ARTIFACT_SHA256_OFFSET..ARTIFACT_SHA256_OFFSET + 32].fill(0);
    let hash = artifact_sha256(artifact).unwrap();
    artifact[ARTIFACT_SHA256_OFFSET..ARTIFACT_SHA256_OFFSET + 32].copy_from_slice(&hash);
}

fn section_offset(artifact: &[u8], section: usize) -> usize {
    get_u64(artifact, SECTION_DIRECTORY_OFFSET + section * SECTION_ENTRY_BYTES + 8) as usize
}

fn tensor_offset(artifact: &[u8], tensor: usize) -> usize {
    section_offset(artifact, 0) + tensor * TENSOR_RECORD_BYTES
}

fn slot_offset(artifact: &[u8], slot: usize) -> usize {
    section_offset(artifact, 1) + slot * SLOT_RECORD_BYTES
}

fn op_offset(artifact: &[u8], op: usize) -> usize {
    section_offset(artifact, 2) + op * OP_RECORD_BYTES
}

fn binding_offset(artifact: &[u8], binding: usize) -> usize {
    section_offset(artifact, 3) + binding * BINDING_RECORD_BYTES
}

fn phase_offset(artifact: &[u8], phase: usize) -> usize {
    section_offset(artifact, 4) + phase * PHASE_RECORD_BYTES
}

#[test]
fn sha256_matches_standard_vectors() {
    assert_eq!(
        sha256(b""),
        hex32("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    );
    assert_eq!(
        sha256(b"abc"),
        hex32("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
    );
    assert_eq!(
        sha256(&[0x5a; 1_000]),
        hex32("8fe15844cfeedd35f5dc30a9fa5ed38afd849dbe4f8dcae5642d934be0afb13d")
    );
}

#[test]
fn python_fixture_wire_contract_parses_and_resolves() {
    let artifact = fixture();
    assert_eq!(artifact.len(), 1_651);
    assert_eq!(
        &artifact[ARTIFACT_SHA256_OFFSET..ARTIFACT_SHA256_OFFSET + 32],
        &hex32(FIXTURE_ARTIFACT_HASH)
    );

    let program = Program::parse(&artifact).unwrap();
    assert_eq!(program.tensor_count(), 7);
    assert_eq!(program.slot_count(), 2);
    assert_eq!(program.op_count(), 3);
    assert_eq!(program.binding_count(), 8);
    assert_eq!(program.data().len(), 19);
    assert_eq!(program.phases()[0].op_start, 0);
    assert_eq!(program.phases()[0].op_end, 2);
    assert_eq!(program.phases()[1].op_start, 2);
    assert_eq!(program.phases()[1].frame_count_max, 32);
    assert_eq!(program.model_sha256(), &hex32(MODEL_HASH));
    assert_eq!(program.voices_sha256(), &hex32(VOICES_HASH));

    let waveform = program.tensor(6).unwrap().resolve(7).unwrap();
    assert_eq!(waveform.dims, [1, 1, 14, 1]);
    assert_eq!(waveform.logical_bytes, 56);
    assert!(waveform.is_contiguous());

    let mut bases = [0u64; 2];
    let resolved = program.resolve_phase_two(7, &mut bases).unwrap();
    assert_eq!(resolved.frame_count(), 7);
    assert_eq!(resolved.arena_bytes(), 64);
    assert_eq!(resolved.slot_base(0), None);
    assert_eq!(resolved.slot_base(1), Some(0));
    assert_eq!(resolved.tensor_arena_offset(6), Some(0));
}

#[test]
fn pinned_hashes_and_resource_limits_fail_closed() {
    let artifact = fixture();
    let artifact_hash = hex32(FIXTURE_ARTIFACT_HASH);
    let model = hex32(MODEL_HASH);
    let voices = hex32(VOICES_HASH);
    let options = ParseOptions {
        expected_artifact_sha256: Some(&artifact_hash),
        expected_model_sha256: Some(&model),
        expected_voices_sha256: Some(&voices),
        max_tensors: 7,
        max_slots: 2,
        max_ops: 3,
        max_bindings: 8,
        max_data_bytes: 19,
        max_arena_bytes: 256,
    };
    Program::parse_with_options(&artifact, options).unwrap();

    let wrong = [0xa5; 32];
    assert_eq!(
        Program::parse_with_options(
            &artifact,
            ParseOptions {
                expected_artifact_sha256: Some(&wrong),
                ..ParseOptions::permissive()
            }
        )
        .unwrap_err(),
        ParseError::ExpectedArtifactHashMismatch
    );
    assert_eq!(
        Program::parse_with_options(
            &artifact,
            ParseOptions {
                expected_model_sha256: Some(&wrong),
                ..ParseOptions::permissive()
            }
        )
        .unwrap_err(),
        ParseError::ExpectedModelHashMismatch
    );
    assert_eq!(
        Program::parse_with_options(
            &artifact,
            ParseOptions {
                max_ops: 2,
                ..ParseOptions::permissive()
            }
        )
        .unwrap_err(),
        ParseError::SectionCountTooLarge
    );
    assert_eq!(
        Program::parse_with_options(
            &artifact,
            ParseOptions {
                max_arena_bytes: 128,
                ..ParseOptions::permissive()
            }
        )
        .unwrap_err(),
        ParseError::BadPhase
    );
}

#[test]
fn header_hash_and_directory_corruption_are_rejected() {
    let original = fixture();

    let mut artifact = original.clone();
    artifact[0] ^= 1;
    assert_eq!(Program::parse(&artifact).unwrap_err(), ParseError::BadMagic);

    let mut artifact = original.clone();
    put_u16(&mut artifact, 8, VERSION + 1);
    assert_eq!(Program::parse(&artifact).unwrap_err(), ParseError::UnsupportedVersion);

    let mut artifact = original.clone();
    artifact[48] = 1;
    assert_eq!(Program::parse(&artifact).unwrap_err(), ParseError::HeaderReservedNonZero);

    let mut artifact = original.clone();
    artifact[ARTIFACT_SHA256_OFFSET..ARTIFACT_SHA256_OFFSET + 32].fill(0);
    assert_eq!(Program::parse(&artifact).unwrap_err(), ParseError::HashMissing);

    let mut artifact = original.clone();
    *artifact.last_mut().unwrap() ^= 1;
    assert_eq!(Program::parse(&artifact).unwrap_err(), ParseError::ArtifactHashMismatch);

    let mut artifact = original.clone();
    artifact[MODEL_SHA256_OFFSET] ^= 1;
    assert_eq!(Program::parse(&artifact).unwrap_err(), ParseError::ArtifactHashMismatch);

    let mut artifact = original.clone();
    artifact[VOICES_SHA256_OFFSET] ^= 1;
    assert_eq!(Program::parse(&artifact).unwrap_err(), ParseError::ArtifactHashMismatch);

    let mut artifact = original.clone();
    artifact[SECTION_DIRECTORY_OFFSET + 16] ^= 1;
    assert_eq!(Program::parse(&artifact).unwrap_err(), ParseError::ArtifactHashMismatch);

    let mut artifact = original.clone();
    let claimed_len = get_u64(&artifact, 16);
    put_u64(&mut artifact, 16, claimed_len - 1);
    assert_eq!(Program::parse(&artifact).unwrap_err(), ParseError::BadArtifactSize);

    let mut artifact = original.clone();
    put_u64(&mut artifact, SECTION_DIRECTORY_OFFSET + 16, u64::MAX);
    reseal(&mut artifact);
    assert_eq!(Program::parse(&artifact).unwrap_err(), ParseError::SectionLengthOverflow);

    let mut artifact = original.clone();
    let tensor_entry = SECTION_DIRECTORY_OFFSET;
    let offset = get_u64(&artifact, tensor_entry + 8);
    put_u64(&mut artifact, tensor_entry + 8, offset + 16);
    reseal(&mut artifact);
    assert_eq!(Program::parse(&artifact).unwrap_err(), ParseError::NonCanonicalSectionOffset);

    let mut artifact = original;
    artifact[1_624] = 1;
    reseal(&mut artifact);
    assert_eq!(Program::parse(&artifact).unwrap_err(), ParseError::NonZeroSectionPadding);
}

#[test]
fn malformed_tensor_and_shape_overflow_are_rejected() {
    let original = fixture();

    let mut artifact = original.clone();
    let offset = tensor_offset(&artifact, 0);
    artifact[offset] = 0xff;
    reseal(&mut artifact);
    assert_eq!(
        Program::parse(&artifact).unwrap_err(),
        ParseError::BadTensor {
            tensor: 0,
            reason: TensorError::UnknownDType,
        }
    );

    let mut artifact = original.clone();
    let offset = tensor_offset(&artifact, 0);
    artifact[offset + 1] = 5;
    reseal(&mut artifact);
    assert_eq!(
        Program::parse(&artifact).unwrap_err(),
        ParseError::BadTensor {
            tensor: 0,
            reason: TensorError::RankTooLarge,
        }
    );

    let mut artifact = original.clone();
    let offset = tensor_offset(&artifact, 0);
    put_u32(&mut artifact, offset + 32, u32::MAX);
    put_u32(&mut artifact, offset + 36, u32::MAX);
    reseal(&mut artifact);
    assert_eq!(
        Program::parse(&artifact).unwrap_err(),
        ParseError::BadTensor {
            tensor: 0,
            reason: TensorError::ByteLengthOverflow,
        }
    );

    let mut artifact = original.clone();
    let offset = tensor_offset(&artifact, 6);
    put_u32(&mut artifact, offset + 84, 1);
    reseal(&mut artifact);
    assert_eq!(
        Program::parse(&artifact).unwrap_err(),
        ParseError::BadTensor {
            tensor: 6,
            reason: TensorError::SymbolicMaximumMismatch,
        }
    );

    let mut artifact = original.clone();
    let offset = tensor_offset(&artifact, 5);
    put_u64(&mut artifact, offset + 16, 18);
    reseal(&mut artifact);
    assert_eq!(
        Program::parse(&artifact).unwrap_err(),
        ParseError::BadTensor {
            tensor: 5,
            reason: TensorError::ConstantOutOfBounds,
        }
    );

    let mut artifact = original;
    let offset = slot_offset(&artifact, 0);
    put_u32(&mut artifact, offset + 4, 32);
    reseal(&mut artifact);
    assert_eq!(
        Program::parse(&artifact).unwrap_err(),
        ParseError::BadTensor {
            tensor: 2,
            reason: TensorError::MisalignedStorage,
        }
    );
}

#[test]
fn views_are_bounds_checked_and_report_materialization() {
    let original = fixture();

    let mut artifact = original.clone();
    let offset = tensor_offset(&artifact, 3);
    put_u64(&mut artifact, offset + 16, 4);
    reseal(&mut artifact);
    assert_eq!(
        Program::parse(&artifact).unwrap_err(),
        ParseError::BadTensor {
            tensor: 3,
            reason: TensorError::ViewOutOfBounds,
        }
    );

    let mut artifact = original.clone();
    let offset = tensor_offset(&artifact, 3);
    put_u64(&mut artifact, offset + 48, 8);
    reseal(&mut artifact);
    assert_eq!(
        Program::parse(&artifact).unwrap_err(),
        ParseError::BadTensor {
            tensor: 3,
            reason: TensorError::WritableStridedView,
        }
    );

    let mut artifact = original;
    let offset = tensor_offset(&artifact, 3);
    put_u32(&mut artifact, offset + 4, TensorFlags::READ_ONLY.bits());
    put_u64(&mut artifact, offset + 24, 8);
    put_u32(&mut artifact, offset + 32, 2);
    put_u64(&mut artifact, offset + 48, 8);
    reseal(&mut artifact);
    let program = Program::parse(&artifact).unwrap();
    let view = program.tensor(3).unwrap().resolve(0).unwrap();
    assert!(!view.is_contiguous());
    assert_eq!(
        view.materialization(LayoutRequirement::StridedRead)
            .unwrap(),
        Materialization::Direct
    );
    assert_eq!(
        view.materialization(LayoutRequirement::ContiguousRead { alignment: 64 })
            .unwrap(),
        Materialization::Required {
            bytes: 8,
            alignment: 64,
        }
    );
    assert_eq!(
        view.materialization(LayoutRequirement::ContiguousWrite { alignment: 4 })
            .unwrap_err(),
        TensorError::ReadOnlyWrite
    );
}

#[test]
fn fixed_and_tensor_aliases_require_explicit_liveness_or_views() {
    let original = fixture();

    let mut artifact = original.clone();
    let offset = slot_offset(&artifact, 1);
    artifact[offset] = 1;
    artifact[offset + 1] = 2;
    put_u64(&mut artifact, offset + 16, 0);
    put_i64(&mut artifact, offset + 24, 64);
    put_u32(&mut artifact, offset + 32, 1);
    reseal(&mut artifact);
    assert_eq!(
        Program::parse(&artifact).unwrap_err(),
        ParseError::FixedSlotAlias {
            first: 0,
            second: 1,
        }
    );

    let mut artifact = original;
    let offset = tensor_offset(&artifact, 3);
    artifact[offset + 2] = 1;
    put_u32(&mut artifact, offset + 8, 0);
    put_u32(&mut artifact, offset + 12, NO_TENSOR);
    reseal(&mut artifact);
    assert_eq!(
        Program::parse(&artifact).unwrap_err(),
        ParseError::TensorStorageAlias {
            first: 2,
            second: 3,
        }
    );
}

#[test]
fn dynamic_interval_packing_reuses_nonoverlapping_slots() {
    let mut parts = fixture_parts();
    parts.slots.push(slot_record(2, 1, 64, 0, 0, 64, 2, 3));
    parts.slots.push(slot_record(2, 1, 64, 0, 0, 64, 3, 4));
    let artifact = emit(&parts);
    let program = Program::parse(&artifact).unwrap();

    let mut too_small = [0u64; 3];
    assert_eq!(
        program.resolve_phase_two(8, &mut too_small).unwrap_err(),
        ArenaPlanError::SlotBasesTooSmall
    );
    let mut bases = [0u64; 4];
    assert_eq!(
        program.resolve_phase_two(0, &mut bases).unwrap_err(),
        ArenaPlanError::FrameCountOutOfRange
    );
    let resolved = program.resolve_phase_two(8, &mut bases).unwrap();
    assert_eq!(resolved.slot_base(1), Some(0));
    assert_eq!(resolved.slot_base(2), Some(64));
    assert_eq!(resolved.slot_base(3), Some(64));
    assert_eq!(resolved.arena_bytes(), 128);
    drop(resolved);
    assert_eq!(
        program.resolve_phase_two(32, &mut bases).unwrap_err(),
        ArenaPlanError::ArenaLimitExceeded
    );
}

#[test]
fn affine_slot_sizes_reject_negative_and_overflowing_results() {
    let base = SlotDesc {
        kind: SlotKind::Dynamic,
        phase: Phase::Phase1,
        alignment: 64,
        fixed_offset: 0,
        byte_multiplier: 0,
        byte_addend: -1,
        live_start: 0,
        live_end: 1,
    };
    assert_eq!(base.bytes_at(0).unwrap_err(), ArenaPlanError::InvalidAffineSize);
    assert_eq!(
        SlotDesc {
            byte_multiplier: u64::MAX,
            byte_addend: i64::MAX,
            ..base
        }
        .bytes_at(u32::MAX)
        .unwrap_err(),
        ArenaPlanError::SizeOverflow
    );
}

#[test]
fn op_bindings_enforce_liveness_mutability_and_alias_flags() {
    let original = fixture();

    let mut artifact = original.clone();
    let offset = binding_offset(&artifact, 2);
    put_u32(&mut artifact, offset, 1);
    reseal(&mut artifact);
    assert_eq!(
        Program::parse(&artifact).unwrap_err(),
        ParseError::BadOp {
            op: 0,
            reason: OpError::ReadOnlyOutput,
        }
    );

    let mut artifact = original.clone();
    let offset = binding_offset(&artifact, 2);
    put_u32(&mut artifact, offset, 0);
    reseal(&mut artifact);
    assert_eq!(
        Program::parse(&artifact).unwrap_err(),
        ParseError::BadOp {
            op: 0,
            reason: OpError::AliasingRequiresInPlace,
        }
    );
    let op = op_offset(&artifact, 0);
    put_u16(&mut artifact, op + 2, OP_FLAG_IN_PLACE);
    reseal(&mut artifact);
    Program::parse(&artifact).unwrap();

    let mut artifact = original;
    let offset = slot_offset(&artifact, 0);
    put_u32(&mut artifact, offset + 32, 1);
    reseal(&mut artifact);
    assert_eq!(
        Program::parse(&artifact).unwrap_err(),
        ParseError::BadOp {
            op: 0,
            reason: OpError::TensorNotLive,
        }
    );
}

#[test]
fn cooperative_cursor_is_transactional_and_phase_gated() {
    let artifact = fixture();
    let program = Program::parse(&artifact).unwrap();
    assert_eq!(WorkBudget::new(0).unwrap_err(), BudgetError::ZeroLimit);

    let mut cursor = OpCursor::new();
    let mut budget = WorkBudget::new(5).unwrap();
    let first = match cursor.poll(&program, &mut budget) {
        CursorPoll::Ready(work) => work,
        other => panic!("expected work, got {other:?}"),
    };
    assert_eq!((first.op_index(), first.unit_start(), first.unit_count()), (0, 0, 5));
    assert_eq!(cursor.op_index(), 0);
    assert_eq!(cursor.unit_offset(), 0);
    cursor.commit(first).unwrap();
    assert_eq!(cursor.unit_offset(), 5);
    assert_eq!(cursor.poll(&program, &mut budget), CursorPoll::BudgetExhausted);
    assert_eq!(cursor.commit(first).unwrap_err(), CursorError::StaleWorkSlice);

    let mut budget = WorkBudget::new(8).unwrap();
    let rest = match cursor.poll(&program, &mut budget) {
        CursorPoll::Ready(work) => work,
        other => panic!("expected work, got {other:?}"),
    };
    assert_eq!((rest.unit_start(), rest.unit_count()), (5, 7));
    cursor.commit(rest).unwrap();
    let resolver = match cursor.poll(&program, &mut budget) {
        CursorPoll::Ready(work) => work,
        other => panic!("expected resolver, got {other:?}"),
    };
    assert_eq!((resolver.op_index(), resolver.unit_count()), (1, 1));
    cursor.commit(resolver).unwrap();
    assert!(matches!(
        cursor.poll(&program, &mut budget),
        CursorPoll::PhaseBoundary(PhasePlan { op_start: 2, .. })
    ));

    let mut bases = [0u64; 2];
    let resolved = program.resolve_phase_two(7, &mut bases).unwrap();
    cursor.admit_phase_two(&program, &resolved).unwrap();
    let mut budget = WorkBudget::new(64).unwrap();
    let waveform = match cursor.poll(&program, &mut budget) {
        CursorPoll::Ready(work) => work,
        other => panic!("expected waveform work, got {other:?}"),
    };
    assert!(waveform.completes_op());
    cursor.commit(waveform).unwrap();
    assert_eq!(cursor.poll(&program, &mut budget), CursorPoll::Complete);

    cursor.reset();
    assert_eq!(cursor, OpCursor::new());
    assert!(OpCursor::from_checkpoint(&program, 0, 5, false).is_ok());
    assert_eq!(
        OpCursor::from_checkpoint(&program, 0, 5, true).unwrap_err(),
        CursorError::InvalidCheckpoint
    );
}

#[test]
fn malformed_phase_and_opcode_are_rejected() {
    let original = fixture();

    let mut artifact = original.clone();
    let offset = phase_offset(&artifact, 1);
    put_u32(&mut artifact, offset + 40, 0);
    reseal(&mut artifact);
    assert_eq!(Program::parse(&artifact).unwrap_err(), ParseError::BadPhase);

    let mut artifact = original;
    let offset = op_offset(&artifact, 2);
    put_u16(&mut artifact, offset, 0xffff);
    reseal(&mut artifact);
    assert_eq!(
        Program::parse(&artifact).unwrap_err(),
        ParseError::BadOp {
            op: 2,
            reason: OpError::UnknownOpcode,
        }
    );
}
