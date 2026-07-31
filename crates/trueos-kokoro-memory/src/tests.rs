use std::vec;
use std::vec::Vec;

use trueos_kokoro_aot::{
    ARTIFACT_SHA256_OFFSET, BINDING_RECORD_BYTES, HEADER_BYTES, LITTLE_ENDIAN_TAG, MAGIC,
    MODEL_SHA256_OFFSET, OP_FLAG_IN_PLACE, OP_RECORD_BYTES, PHASE_COUNT, PHASE_FLAG_RUNTIME_SIZED,
    PHASE_RECORD_BYTES, SECTION_COUNT, SECTION_DIRECTORY_OFFSET, SECTION_ENTRY_BYTES,
    SLOT_RECORD_BYTES, STATIC_DIM, TENSOR_RECORD_BYTES, TensorFlags, VERSION, VOICES_SHA256_OFFSET,
    WorkBudget, WorkSlice, artifact_sha256,
};
use trueos_kokoro_exec::{
    DispatchResult, Dispatcher, Executor, RuntimeShape, SliceEvent, TensorShapeTable,
};

use super::*;

const MAX_ARTIFACT_BYTES: usize = 4_096;

#[repr(align(64))]
struct AlignedArtifact([u8; MAX_ARTIFACT_BYTES]);

#[repr(align(64))]
struct AlignedArena([u8; 512]);

#[repr(align(16))]
struct AlignedF32<const N: usize>([f32; N]);

#[repr(align(16))]
struct AlignedI64<const N: usize>([i64; N]);

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
        1 | 2 => 4_u64,
        3 => 8,
        4..=6 => 1,
        _ => 0,
    };
    let mut max_dims = [1_u32; 4];
    max_dims[..dims.len()].copy_from_slice(dims);
    let mut contiguous = [0_u64; 4];
    let mut stride = element_bytes;
    for index in (0..dims.len()).rev() {
        contiguous[index] = stride;
        stride = stride.saturating_mul(u64::from(dims[index]));
    }
    let byte_capacity = dims
        .iter()
        .fold(element_bytes, |bytes, dim| bytes.saturating_mul(u64::from(*dim)));

    let mut record = [0_u8; TENSOR_RECORD_BYTES];
    record[0] = dtype;
    record[1] = dims.len() as u8;
    record[2] = storage;
    record[3] = phase;
    put_u32(&mut record, 4, flags);
    put_u32(&mut record, 8, slot_id);
    put_u32(&mut record, 12, view_of);
    put_u64(&mut record, 16, storage_offset);
    put_u64(&mut record, 24, byte_capacity);
    let strides = strides.unwrap_or(contiguous);
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

#[allow(clippy::too_many_arguments)]
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
    let mut record = [0_u8; SLOT_RECORD_BYTES];
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
) -> [u8; OP_RECORD_BYTES] {
    let mut record = [0_u8; OP_RECORD_BYTES];
    put_u16(&mut record, 0, opcode);
    put_u16(&mut record, 2, flags);
    record[4] = phase;
    put_u32(&mut record, 8, binding_start);
    put_u16(&mut record, 12, input_count);
    put_u16(&mut record, 14, output_count);
    put_u32(&mut record, 28, 1);
    record
}

#[allow(clippy::too_many_arguments)]
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
    let mut record = [0_u8; PHASE_RECORD_BYTES];
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
    let mut data = Vec::new();
    for value in [1.0_f32, -2.0, 3.5, 4.0] {
        data.extend_from_slice(&value.to_le_bytes());
    }
    for value in [17_i32, -9] {
        data.extend_from_slice(&value.to_le_bytes());
    }
    data.extend_from_slice(&[-7_i8 as u8, 0, 11]);
    data.push(2); // Deliberately invalid Rust bool, rejected by the bridge.
    assert_eq!(data.len(), 28);

    let input = TensorFlags::INPUT.bits();
    let output = TensorFlags::OUTPUT.bits();
    let read_only = TensorFlags::READ_ONLY.bits();
    Parts {
        tensors: vec![
            // 0: phase-zero external input.
            tensor_record(1, &[4], 4, 0, input, u32::MAX, u32::MAX, 0, None, STATIC_DIM, 0, 0, 16),
            // 1: shared DATA f32 constant.
            tensor_record(
                1,
                &[4],
                3,
                2,
                read_only,
                u32::MAX,
                u32::MAX,
                0,
                None,
                STATIC_DIM,
                0,
                0,
                16,
            ),
            // 2: fixed phase-zero slot.
            tensor_record(1, &[4], 1, 0, 0, 0, u32::MAX, 0, None, STATIC_DIM, 0, 0, 64),
            // 3: contiguous writable view into tensor 2.
            tensor_record(1, &[2], 2, 0, 0, u32::MAX, 2, 4, None, STATIC_DIM, 0, 0, 4),
            // 4, 5: external f32 and bool outputs.
            tensor_record(1, &[4], 4, 0, output, u32::MAX, u32::MAX, 0, None, STATIC_DIM, 0, 0, 16),
            tensor_record(6, &[4], 4, 0, output, u32::MAX, u32::MAX, 0, None, STATIC_DIM, 0, 0, 1),
            // 6: zero-sized slot tensor at the end of tensor 2.
            tensor_record(4, &[0], 1, 0, 0, 0, u32::MAX, 16, None, STATIC_DIM, 0, 0, 1),
            // 7: F-sized dynamic phase-one slot.
            tensor_record(1, &[8], 1, 1, 0, 1, u32::MAX, 0, None, 0, 1, 0, 64),
            // 8: shared fixed scalar.
            tensor_record(3, &[], 1, 2, 0, 2, u32::MAX, 0, None, STATIC_DIM, 0, 0, 64),
            // 9: phase-one external output.
            tensor_record(1, &[8], 4, 1, output, u32::MAX, u32::MAX, 0, None, STATIC_DIM, 0, 0, 16),
            // 10: resolver external output.
            tensor_record(3, &[], 4, 0, output, u32::MAX, u32::MAX, 0, None, STATIC_DIM, 0, 0, 8),
            // 11: second external f32 output.
            tensor_record(1, &[4], 4, 0, output, u32::MAX, u32::MAX, 0, None, STATIC_DIM, 0, 0, 16),
            // 12: exact physical view of tensor 2, but a different binding ID.
            tensor_record(1, &[4], 2, 0, 0, u32::MAX, 2, 0, None, STATIC_DIM, 0, 0, 64),
            // 13, 14, 15: i32, i8, and invalid-bool DATA constants.
            tensor_record(
                2,
                &[2],
                3,
                2,
                read_only,
                u32::MAX,
                u32::MAX,
                16,
                None,
                STATIC_DIM,
                0,
                0,
                4,
            ),
            tensor_record(
                5,
                &[3],
                3,
                2,
                read_only,
                u32::MAX,
                u32::MAX,
                24,
                None,
                STATIC_DIM,
                0,
                0,
                1,
            ),
            tensor_record(
                6,
                &[1],
                3,
                2,
                read_only,
                u32::MAX,
                u32::MAX,
                27,
                None,
                STATIC_DIM,
                0,
                0,
                1,
            ),
        ],
        slots: vec![
            slot_record(1, 0, 64, 0, 0, 64, 0, 6),
            slot_record(2, 1, 64, 0, 4, 0, 6, 9),
            slot_record(1, 2, 64, 64, 0, 64, 0, 9),
        ],
        ops: vec![
            op_record(0x0100, 0, 0, 0, 2, 3),
            op_record(0x0100, OP_FLAG_IN_PLACE, 0, 5, 1, 1),
            op_record(0x0100, OP_FLAG_IN_PLACE, 0, 7, 1, 1),
            op_record(0x0100, 0, 0, 9, 1, 1),
            op_record(0x0100, 0, 0, 11, 2, 2),
            op_record(0x0001, 0, 0, 15, 1, 1),
            op_record(0x0100, 0, 1, 17, 1, 1),
            op_record(0x0100, 0, 1, 19, 1, 1),
        ],
        bindings: vec![
            0, 1, 2, 6, 8, // op 0
            2, 2, // op 1: exact same-ID in-place
            2, 12, // op 2: physical alias, different ID
            1, 3, // op 3: initialize the shorter view
            0, 1, 4, 11, // op 4
            8, 10, // op 5: resolver
            1, 7, // op 6
            7, 9, // op 7
        ],
        phases: [
            phase_record(0, 0, 0, 6, 128, 128, 0, 0),
            phase_record(1, PHASE_FLAG_RUNTIME_SIZED, 6, 8, 128, 256, 1, 8),
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
    let alignments = [16_u32, 16, 8, 8, 8, 16];
    let strides = [128_u32, 64, 40, 4, 48, 1];

    let mut artifact = vec![0_u8; HEADER_BYTES];
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
    artifact[MODEL_SHA256_OFFSET..MODEL_SHA256_OFFSET + 32].fill(0x23);
    artifact[VOICES_SHA256_OFFSET..VOICES_SHA256_OFFSET + 32].fill(0x42);

    for index in 0..SECTION_COUNT {
        let offset = align_up_usize(artifact.len(), alignments[index] as usize);
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
    let len = artifact.len() as u64;
    put_u64(&mut artifact, 16, len);
    reseal(&mut artifact);
    artifact
}

fn aligned_fixture() -> (AlignedArtifact, usize) {
    let artifact = emit(&fixture_parts());
    assert!(artifact.len() <= MAX_ARTIFACT_BYTES);
    let mut aligned = AlignedArtifact([0; MAX_ARTIFACT_BYTES]);
    aligned.0[..artifact.len()].copy_from_slice(&artifact);
    (aligned, artifact.len())
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

fn align_up_usize(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
}

fn reseal(artifact: &mut [u8]) {
    artifact[ARTIFACT_SHA256_OFFSET..ARTIFACT_SHA256_OFFSET + 32].fill(0);
    let hash = artifact_sha256(artifact).unwrap();
    artifact[ARTIFACT_SHA256_OFFSET..ARTIFACT_SHA256_OFFSET + 32].copy_from_slice(&hash);
}

fn section_offset(artifact: &[u8], section: usize) -> usize {
    let entry = SECTION_DIRECTORY_OFFSET + section * SECTION_ENTRY_BYTES;
    u64::from_le_bytes(artifact[entry + 8..entry + 16].try_into().unwrap()) as usize
}

fn initialize_shapes<const N: usize>(program: &Program<'_>) -> TensorShapeTable<N> {
    let mut shapes = TensorShapeTable::new();
    shapes.initialize(program).unwrap();
    for (tensor, dimensions) in [
        (0, &[4_u32][..]),
        (4, &[4][..]),
        (5, &[4][..]),
        (9, &[4][..]),
        (10, &[][..]),
        (11, &[4][..]),
    ] {
        shapes
            .bind_external(program, tensor, RuntimeShape::new(dimensions).unwrap())
            .unwrap();
    }
    shapes
        .declare_op_outputs(
            program,
            0,
            &[
                RuntimeShape::new(&[4]).unwrap(),
                RuntimeShape::new(&[0]).unwrap(),
                RuntimeShape::scalar(),
            ],
        )
        .unwrap();
    shapes
        .declare_op_outputs(program, 3, &[RuntimeShape::new(&[2]).unwrap()])
        .unwrap();
    shapes
        .declare_op_outputs(program, 2, &[RuntimeShape::new(&[4]).unwrap()])
        .unwrap();
    shapes
}

#[test]
fn maps_data_fixed_views_externals_zero_size_and_every_dtype() {
    let (artifact, len) = aligned_fixture();
    let program = Program::parse(&artifact.0[..len]).unwrap();
    let shapes = initialize_shapes::<20>(&program);

    let input = AlignedF32([10.0, 20.0, 30.0, 40.0]);
    let mut output = AlignedF32([0.0; 4]);
    let mut bool_output = [false; 4];
    let mut phase_one_output = AlignedF32([0.0; 8]);
    let mut resolver_output = AlignedI64([0]);
    let mut second_output = AlignedF32([0.0; 4]);
    let mut externals: ExternalBindings<'_, 8> = ExternalBindings::new();
    externals
        .bind_input(&program, &shapes, 0, &input.0)
        .unwrap();
    externals
        .bind_output(&program, &shapes, 4, &mut output.0)
        .unwrap();
    externals
        .bind_output(&program, &shapes, 5, &mut bool_output)
        .unwrap();
    externals
        .bind_output(&program, &shapes, 9, &mut phase_one_output.0)
        .unwrap();
    externals
        .bind_output(&program, &shapes, 10, &mut resolver_output.0)
        .unwrap();
    externals
        .bind_output(&program, &shapes, 11, &mut second_output.0)
        .unwrap();

    let mut arena = AlignedArena([0; 512]);
    {
        let mut memory: TensorMemory<'_, '_, '_, 20, 8, 8> =
            TensorMemory::phase_zero(&program, &shapes, &mut arena.0, &mut externals).unwrap();

        memory
            .with_read::<f32, _, _>(1, |values, shape| {
                assert_eq!(shape.dims(), &[4]);
                assert_eq!(values, &[1.0, -2.0, 3.5, 4.0]);
            })
            .unwrap();
        memory
            .with_read::<i32, _, _>(13, |values, _| assert_eq!(values, &[17, -9]))
            .unwrap();
        memory
            .with_read::<i8, _, _>(14, |values, _| assert_eq!(values, &[-7, 0, 11]))
            .unwrap();
        memory
            .with_read::<f32, _, _>(0, |values, _| assert_eq!(values, &input.0))
            .unwrap();

        memory
            .with_write::<f32, _, _>(2, |values, _| {
                values.copy_from_slice(&[5.0, 6.0, 7.0, 8.0]);
            })
            .unwrap();
        memory
            .with_read::<f32, _, _>(3, |values, shape| {
                assert_eq!(shape.dims(), &[2]);
                assert_eq!(values, &[6.0, 7.0]);
            })
            .unwrap();
        memory
            .with_write::<i64, _, _>(8, |values, shape| {
                assert_eq!(shape, RuntimeShape::scalar());
                values[0] = 1234;
            })
            .unwrap();
        memory
            .with_read::<i64, _, _>(8, |values, _| assert_eq!(values, &[1234]))
            .unwrap();
        memory
            .with_write::<u8, _, _>(6, |values, shape| {
                assert!(values.is_empty());
                assert_eq!(shape.dims(), &[0]);
            })
            .unwrap();
        memory
            .with_write::<bool, _, _>(5, |values, _| {
                values.copy_from_slice(&[true, false, true, true]);
            })
            .unwrap();
        memory
            .with_write::<f32, _, _>(4, |values, _| values.fill(9.0))
            .unwrap();

        assert_eq!(
            memory.with_read::<bool, _, _>(15, |_, _| ()),
            Err(MemoryError::InvalidBoolValue)
        );
        assert_eq!(memory.with_write::<f32, _, _>(1, |_, _| ()), Err(MemoryError::ReadOnlyWrite));
        assert_eq!(memory.with_read::<u8, _, _>(1, |_, _| ()), Err(MemoryError::DTypeMismatch));
    }
    assert_eq!(output.0, [9.0; 4]);
    assert_eq!(bool_output, [true, false, true, true]);
}

#[test]
fn operation_leases_enforce_aliases_and_exact_in_place_identity() {
    let (artifact, len) = aligned_fixture();
    let program = Program::parse(&artifact.0[..len]).unwrap();
    let shapes = initialize_shapes::<20>(&program);
    let input = AlignedF32([1.0, 2.0, 3.0, 4.0]);
    let mut output = AlignedF32([0.0; 4]);
    let mut second = AlignedF32([0.0; 4]);
    let mut externals: ExternalBindings<'_, 4> = ExternalBindings::new();
    externals
        .bind_input(&program, &shapes, 0, &input.0)
        .unwrap();
    externals
        .bind_output(&program, &shapes, 4, &mut output.0)
        .unwrap();
    externals
        .bind_output(&program, &shapes, 11, &mut second.0)
        .unwrap();
    let mut arena = AlignedArena([0; 512]);
    let mut memory: TensorMemory<'_, '_, '_, 20, 4, 8> =
        TensorMemory::phase_zero(&program, &shapes, &mut arena.0, &mut externals).unwrap();

    memory
        .with_op(0, |op| {
            assert_eq!((op.input_count(), op.output_count()), (2, 3));
            let input = op.input::<f32>(0).unwrap();
            let constant = op.input::<f32>(1).unwrap();
            let mut result = op.output::<f32>(0).unwrap();
            let zero = op.output::<u8>(1).unwrap();
            let mut scalar = op.output::<i64>(2).unwrap();
            for index in 0..result.len() {
                result[index] = input[index] + constant[index];
            }
            assert!(zero.is_empty());
            scalar[0] = 4;
        })
        .unwrap();

    memory
        .with_op(1, |op| {
            let read = op.input::<f32>(0).unwrap();
            assert_eq!(op.output::<f32>(0).unwrap_err(), MemoryError::BorrowConflict);
            drop(read);
            let mut values = op.in_place::<f32>(0, 0).unwrap();
            values[0] *= 2.0;
            assert_eq!(op.input::<f32>(0).unwrap_err(), MemoryError::BorrowConflict);
        })
        .unwrap();

    let mut callback_called = false;
    assert_eq!(memory.with_op(2, |_| callback_called = true), Err(MemoryError::InputOutputOverlap));
    assert!(!callback_called);

    memory
        .with_op(4, |op| {
            let first = op.input::<f32>(0).unwrap();
            let second_input = op.input::<f32>(1).unwrap();
            let mut first_output = op.output::<f32>(0).unwrap();
            let mut second_output = op.output::<f32>(1).unwrap();
            first_output.copy_from_slice(&first);
            second_output.copy_from_slice(&second_input);
        })
        .unwrap();
}

#[test]
fn external_binding_errors_are_transactional() {
    let (artifact, len) = aligned_fixture();
    let program = Program::parse(&artifact.0[..len]).unwrap();
    let shapes = initialize_shapes::<20>(&program);

    let misaligned = AlignedF32([0.0; 5]);
    let mut bindings: ExternalBindings<'_, 2> = ExternalBindings::new();
    assert_eq!(
        bindings.bind_input(&program, &shapes, 0, &misaligned.0[1..]),
        Err(MemoryError::ExternalMisaligned)
    );
    assert_eq!(bindings.len(), 0);

    let mut too_small = AlignedF32([0.0; 3]);
    assert_eq!(
        bindings.bind_output(&program, &shapes, 4, &mut too_small.0),
        Err(MemoryError::ExternalBufferTooSmall)
    );
    assert_eq!(bindings.len(), 0);

    let input = AlignedF32([0.0; 4]);
    bindings.bind_input(&program, &shapes, 0, &input.0).unwrap();
    assert_eq!(bindings.len(), 1);
    let duplicate = AlignedF32([0.0; 4]);
    assert_eq!(
        bindings.bind_input(&program, &shapes, 0, &duplicate.0),
        Err(MemoryError::DuplicateExternal)
    );
    assert_eq!(bindings.len(), 1);

    let mut one: ExternalBindings<'_, 1> = ExternalBindings::new();
    one.bind_input(&program, &shapes, 0, &input.0).unwrap();
    let mut output = AlignedF32([0.0; 4]);
    assert_eq!(
        one.bind_output(&program, &shapes, 4, &mut output.0),
        Err(MemoryError::ExternalTableFull)
    );
    assert_eq!(one.len(), 1);
}

#[test]
fn constructors_reject_alignment_foreign_shapes_and_noncontiguous_views() {
    let original = emit(&fixture_parts());
    let mut aligned = AlignedArtifact([0; MAX_ARTIFACT_BYTES]);
    aligned.0[..original.len()].copy_from_slice(&original);
    let program = Program::parse(&aligned.0[..original.len()]).unwrap();
    let shapes = initialize_shapes::<20>(&program);
    let mut externals: ExternalBindings<'_, 1> = ExternalBindings::new();
    let mut arena = AlignedArena([0; 512]);

    assert!(matches!(
        TensorMemory::<'_, '_, '_, 20, 1, 8>::phase_zero(
            &program,
            &shapes,
            &mut arena.0[1..],
            &mut externals,
        ),
        Err(MemoryError::ArenaMisaligned)
    ));
    assert!(matches!(
        TensorMemory::<'_, '_, '_, 20, 1, 8>::phase_zero(
            &program,
            &shapes,
            &mut arena.0[..64],
            &mut externals,
        ),
        Err(MemoryError::ArenaTooSmall)
    ));

    let mut shifted = AlignedArtifact([0; MAX_ARTIFACT_BYTES]);
    shifted.0[1..1 + original.len()].copy_from_slice(&original);
    let shifted_program = Program::parse(&shifted.0[1..1 + original.len()]).unwrap();
    let shifted_shapes = initialize_shapes::<20>(&shifted_program);
    assert!(matches!(
        TensorMemory::<'_, '_, '_, 20, 1, 8>::phase_zero(
            &shifted_program,
            &shifted_shapes,
            &mut arena.0,
            &mut externals,
        ),
        Err(MemoryError::DataMisaligned)
    ));

    let mut foreign_bytes = original.clone();
    let data = section_offset(&foreign_bytes, 5);
    foreign_bytes[data] ^= 1;
    reseal(&mut foreign_bytes);
    let mut foreign_aligned = AlignedArtifact([0; MAX_ARTIFACT_BYTES]);
    foreign_aligned.0[..foreign_bytes.len()].copy_from_slice(&foreign_bytes);
    let foreign_program = Program::parse(&foreign_aligned.0[..foreign_bytes.len()]).unwrap();
    let foreign_shapes = initialize_shapes::<20>(&foreign_program);
    assert!(matches!(
        TensorMemory::<'_, '_, '_, 20, 1, 8>::phase_zero(
            &program,
            &foreign_shapes,
            &mut arena.0,
            &mut externals,
        ),
        Err(MemoryError::ForeignShapeTable)
    ));

    let mut strided = program.tensor(3).unwrap();
    strided.max_byte_strides[0] = 8;
    assert!(!view_is_contiguous(strided, RuntimeShape::new(&[2]).unwrap()).unwrap());
}

struct AdmitDispatcher;

impl Dispatcher for AdmitDispatcher {
    type Error = ();

    fn dispatch(
        &mut self,
        _program: &Program<'_>,
        work: WorkSlice,
    ) -> Result<DispatchResult, Self::Error> {
        if work.op().opcode == trueos_kokoro_aot::OpCode::ResolveDecoderShape && work.completes_op()
        {
            Ok(DispatchResult::FrameCount(4))
        } else {
            Ok(DispatchResult::Completed)
        }
    }
}

#[test]
fn phase_one_uses_executor_admission_and_revalidates_slot_bases() {
    let (artifact, len) = aligned_fixture();
    let program = Program::parse(&artifact.0[..len]).unwrap();
    let mut executor: Executor<4> = Executor::new();
    let mut budget = WorkBudget::new(16).unwrap();
    let report = executor.run_slice(&program, &mut AdmitDispatcher, &mut budget);
    let admission = match report.event {
        SliceEvent::PhaseAdmitted(admission) => admission,
        other => panic!("unexpected executor event: {other:?}"),
    };
    assert_eq!(executor.slot_bases(), &[UNRESOLVED_SLOT_BASE, 0, 64]);

    let mut shapes = initialize_shapes::<20>(&program);
    shapes
        .declare_op_outputs(&program, 6, &[RuntimeShape::new(&[4]).unwrap()])
        .unwrap();
    shapes
        .declare_op_outputs(&program, 7, &[RuntimeShape::new(&[4]).unwrap()])
        .unwrap();
    let input = AlignedF32([0.0; 4]);
    let mut output = AlignedF32([0.0; 8]);
    let mut externals: ExternalBindings<'_, 2> = ExternalBindings::new();
    externals
        .bind_input(&program, &shapes, 0, &input.0)
        .unwrap();
    externals
        .bind_output(&program, &shapes, 9, &mut output.0)
        .unwrap();
    let mut arena = AlignedArena([0; 512]);

    assert!(matches!(
        TensorMemory::<'_, '_, '_, 20, 2, 8>::phase_one(
            &program,
            &shapes,
            &mut arena.0,
            admission,
            &executor.slot_bases()[..2],
            &mut externals,
        ),
        Err(MemoryError::SlotBasesTooSmall)
    ));
    let unresolved = [UNRESOLVED_SLOT_BASE; 3];
    assert!(matches!(
        TensorMemory::<'_, '_, '_, 20, 2, 8>::phase_one(
            &program,
            &shapes,
            &mut arena.0,
            admission,
            &unresolved,
            &mut externals,
        ),
        Err(MemoryError::UnresolvedSlot)
    ));
    let misaligned = [UNRESOLVED_SLOT_BASE, 1, 64];
    assert!(matches!(
        TensorMemory::<'_, '_, '_, 20, 2, 8>::phase_one(
            &program,
            &shapes,
            &mut arena.0,
            admission,
            &misaligned,
            &mut externals,
        ),
        Err(MemoryError::SlotBaseMisaligned)
    ));
    let overlapping = [UNRESOLVED_SLOT_BASE, 64, 64];
    assert!(matches!(
        TensorMemory::<'_, '_, '_, 20, 2, 8>::phase_one(
            &program,
            &shapes,
            &mut arena.0,
            admission,
            &overlapping,
            &mut externals,
        ),
        Err(MemoryError::OverlappingLiveSlots)
    ));

    {
        let mut memory: TensorMemory<'_, '_, '_, 20, 2, 8> = TensorMemory::phase_one(
            &program,
            &shapes,
            &mut arena.0,
            admission,
            executor.slot_bases(),
            &mut externals,
        )
        .unwrap();
        memory
            .with_write::<f32, _, _>(7, |values, shape| {
                assert_eq!(shape.dims(), &[4]);
                values.copy_from_slice(&[2.0, 4.0, 6.0, 8.0]);
            })
            .unwrap();
        memory
            .with_op(7, |op| {
                let input = op.input::<f32>(0).unwrap();
                let mut output = op.output::<f32>(0).unwrap();
                output.copy_from_slice(&input);
            })
            .unwrap();
    }
    assert_eq!(&output.0[..4], &[2.0, 4.0, 6.0, 8.0]);
}

#[test]
fn physical_alias_validator_rejects_outputs_before_dispatch() {
    let mut backing = [0_u8; 64];
    let pointer = NonNull::new(backing.as_mut_ptr()).unwrap();
    let shape = RuntimeShape::new(&[4]).unwrap();
    let region = Region {
        pointer,
        bytes: 16,
        elements: 4,
        dtype: DType::F32,
        shape,
        writable: true,
    };
    let op = OpDesc {
        opcode: trueos_kokoro_aot::OpCode::Add,
        flags: 0,
        phase: Phase::Phase0,
        binding_start: 0,
        input_count: 0,
        output_count: 2,
        attribute_offset: 0,
        attribute_len: 0,
        work_units: 1,
    };
    assert_eq!(
        validate_op_aliases(op, &[region, region], &[1, 2]),
        Err(MemoryError::OutputOverlap)
    );
}
