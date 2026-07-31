use std::{vec, vec::Vec};

use trueos_kokoro_aot::*;

use crate::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Call {
    artifact_sha256: [u8; 32],
    op_index: u32,
    unit_start: u32,
    unit_count: u32,
}

struct RecordingDispatcher {
    calls: Vec<Call>,
    frame_count: Option<u32>,
    fail_next: bool,
    force_frame_count: bool,
}

impl RecordingDispatcher {
    fn with_frame_count(frame_count: Option<u32>) -> Self {
        Self {
            calls: Vec::new(),
            frame_count,
            fail_next: false,
            force_frame_count: false,
        }
    }
}

impl Dispatcher for RecordingDispatcher {
    type Error = &'static str;

    fn dispatch(
        &mut self,
        program: &Program<'_>,
        work: WorkSlice,
    ) -> Result<DispatchResult, Self::Error> {
        self.calls.push(Call {
            artifact_sha256: *program.artifact_sha256(),
            op_index: work.op_index(),
            unit_start: work.unit_start(),
            unit_count: work.unit_count(),
        });
        if self.fail_next {
            self.fail_next = false;
            return Err("kernel failed");
        }
        if self.force_frame_count {
            return Ok(DispatchResult::FrameCount(self.frame_count.unwrap_or(1)));
        }
        if work.op().opcode == OpCode::ResolveDecoderShape
            && work.completes_op()
            && let Some(frame_count) = self.frame_count
        {
            return Ok(DispatchResult::FrameCount(frame_count));
        }
        Ok(DispatchResult::Completed)
    }
}

#[test]
fn budget_slices_stop_exactly_at_the_phase_boundary_and_complete() {
    let artifact = fixture(64, 0x11, 1);
    let program = Program::parse(&artifact).unwrap();
    let mut executor = Executor::<1>::new();
    let mut dispatcher = RecordingDispatcher::with_frame_count(Some(4));

    let mut budget = WorkBudget::new(1).unwrap();
    let report = executor.run_slice(&program, &mut dispatcher, &mut budget);
    assert_eq!(report.event, SliceEvent::BudgetExhausted);
    assert_eq!((report.consumed, report.remaining), (1, 0));
    assert_eq!(executor.state(), ExecutorState::Phase0);
    assert_eq!((executor.cursor().op_index(), executor.cursor().unit_offset()), (0, 1));

    let mut budget = WorkBudget::new(1).unwrap();
    let report = executor.run_slice(&program, &mut dispatcher, &mut budget);
    let resolved = match report.event {
        SliceEvent::PhaseAdmitted(resolved) => resolved,
        event => panic!("expected phase admission, got {event:?}"),
    };
    assert_eq!((report.consumed, report.remaining), (1, 0));
    assert_eq!(resolved.frame_count(), 4);
    assert_eq!(resolved.arena_bytes(), 64);
    assert_eq!(resolved.slot_count(), 1);
    assert_eq!(executor.resolved_phase(), Some(resolved));
    assert_eq!(executor.slot_bases(), &[0]);
    assert_eq!(executor.slot_base(0), Some(0));
    assert_eq!(executor.state(), ExecutorState::Phase1);
    assert_eq!(dispatcher.calls.len(), 2);
    assert!(dispatcher.calls.iter().all(|call| call.op_index == 0));

    let mut budget = WorkBudget::new(2).unwrap();
    let report = executor.run_slice(&program, &mut dispatcher, &mut budget);
    assert_eq!(report.event, SliceEvent::BudgetExhausted);
    assert_eq!(executor.state(), ExecutorState::Phase1);
    assert_eq!((executor.cursor().op_index(), executor.cursor().unit_offset()), (1, 2));

    let mut budget = WorkBudget::new(3).unwrap();
    let report = executor.run_slice(&program, &mut dispatcher, &mut budget);
    assert_eq!(report.event, SliceEvent::Complete);
    assert_eq!((report.consumed, report.remaining), (3, 0));
    assert_eq!(executor.state(), ExecutorState::Complete);
    assert!(executor.is_terminal());

    let calls = dispatcher.calls.len();
    let mut budget = WorkBudget::new(7).unwrap();
    let report = executor.run_slice(&program, &mut dispatcher, &mut budget);
    assert_eq!(report.event, SliceEvent::Complete);
    assert_eq!((report.consumed, report.remaining), (0, 7));
    assert_eq!(dispatcher.calls.len(), calls);
}

#[test]
fn dispatcher_failure_does_not_commit_and_the_exact_slice_retries() {
    let artifact = fixture(64, 0x11, 1);
    let program = Program::parse(&artifact).unwrap();
    let mut executor = Executor::<1>::new();
    let mut dispatcher = RecordingDispatcher::with_frame_count(Some(4));
    dispatcher.fail_next = true;

    let mut budget = WorkBudget::new(2).unwrap();
    let report = executor.run_slice(&program, &mut dispatcher, &mut budget);
    assert_eq!(report.event, SliceEvent::DispatchFailed("kernel failed"));
    assert_eq!((report.consumed, report.remaining), (2, 0));
    assert_eq!(executor.state(), ExecutorState::Phase0);
    assert_eq!((executor.cursor().op_index(), executor.cursor().unit_offset()), (0, 0));

    let failed_call = dispatcher.calls[0];
    let mut budget = WorkBudget::new(2).unwrap();
    let report = executor.run_slice(&program, &mut dispatcher, &mut budget);
    assert!(matches!(report.event, SliceEvent::PhaseAdmitted(_)));
    assert_eq!(dispatcher.calls[1], failed_call);
    assert_eq!(executor.state(), ExecutorState::Phase1);
}

#[test]
fn frame_count_protocol_is_explicit_and_one_time() {
    let artifact = fixture(64, 0x11, 1);
    let program = Program::parse(&artifact).unwrap();

    let mut missing = Executor::<1>::new();
    let mut dispatcher = RecordingDispatcher::with_frame_count(None);
    let mut budget = WorkBudget::new(2).unwrap();
    let report = missing.run_slice(&program, &mut dispatcher, &mut budget);
    assert_eq!(report.event, SliceEvent::Faulted(ExecutorFault::MissingFrameCount));
    assert_eq!(missing.state(), ExecutorState::Faulted(ExecutorFault::MissingFrameCount));

    let mut partial = Executor::<1>::new();
    let mut dispatcher = RecordingDispatcher::with_frame_count(Some(4));
    dispatcher.force_frame_count = true;
    let mut budget = WorkBudget::new(1).unwrap();
    let report = partial.run_slice(&program, &mut dispatcher, &mut budget);
    assert_eq!(report.event, SliceEvent::Faulted(ExecutorFault::UnexpectedFrameCount));
    assert_eq!((partial.cursor().op_index(), partial.cursor().unit_offset()), (0, 0));

    let duplicate_artifact = fixture(64, 0x11, 2);
    let duplicate_program = Program::parse(&duplicate_artifact).unwrap();
    let mut duplicate = Executor::<1>::new();
    let mut dispatcher = RecordingDispatcher::with_frame_count(Some(4));
    let mut budget = WorkBudget::new(4).unwrap();
    let report = duplicate.run_slice(&duplicate_program, &mut dispatcher, &mut budget);
    assert_eq!(report.event, SliceEvent::Faulted(ExecutorFault::DuplicateFrameCount));
    assert_eq!((duplicate.cursor().op_index(), duplicate.cursor().unit_offset()), (1, 0));

    let mut phase_one = Executor::<1>::new();
    let mut dispatcher = RecordingDispatcher::with_frame_count(Some(4));
    let mut budget = WorkBudget::new(2).unwrap();
    assert!(matches!(
        phase_one
            .run_slice(&program, &mut dispatcher, &mut budget)
            .event,
        SliceEvent::PhaseAdmitted(_)
    ));
    dispatcher.force_frame_count = true;
    let mut budget = WorkBudget::new(1).unwrap();
    let report = phase_one.run_slice(&program, &mut dispatcher, &mut budget);
    assert_eq!(report.event, SliceEvent::Faulted(ExecutorFault::UnexpectedFrameCount));
    assert_eq!((phase_one.cursor().op_index(), phase_one.cursor().unit_offset()), (1, 0));
}

#[test]
fn frame_bounds_arena_capacity_and_slot_scratch_fail_closed() {
    let artifact = fixture(64, 0x11, 1);
    let program = Program::parse(&artifact).unwrap();

    let mut bounds = Executor::<1>::new();
    let mut dispatcher = RecordingDispatcher::with_frame_count(Some(9));
    let mut budget = WorkBudget::new(2).unwrap();
    let report = bounds.run_slice(&program, &mut dispatcher, &mut budget);
    assert_eq!(
        report.event,
        SliceEvent::Faulted(ExecutorFault::Arena(ArenaPlanError::FrameCountOutOfRange))
    );

    let constrained_artifact = fixture(0, 0x11, 1);
    let constrained_program = Program::parse(&constrained_artifact).unwrap();
    let mut constrained = Executor::<1>::new();
    let mut dispatcher = RecordingDispatcher::with_frame_count(Some(1));
    let mut budget = WorkBudget::new(2).unwrap();
    let report = constrained.run_slice(&constrained_program, &mut dispatcher, &mut budget);
    assert_eq!(
        report.event,
        SliceEvent::Faulted(ExecutorFault::Arena(ArenaPlanError::ArenaLimitExceeded))
    );

    let mut no_scratch = Executor::<0>::new();
    let mut dispatcher = RecordingDispatcher::with_frame_count(Some(1));
    let mut budget = WorkBudget::new(2).unwrap();
    let report = no_scratch.run_slice(&program, &mut dispatcher, &mut budget);
    assert_eq!(
        report.event,
        SliceEvent::Faulted(ExecutorFault::Arena(ArenaPlanError::SlotBasesTooSmall))
    );
}

#[test]
fn cancel_reset_and_foreign_program_are_terminal_and_deterministic() {
    let artifact_a = fixture(64, 0x11, 1);
    let artifact_b = fixture(64, 0x12, 1);
    let program_a = Program::parse(&artifact_a).unwrap();
    let program_b = Program::parse(&artifact_b).unwrap();
    let mut executor = Executor::<1>::new();
    let mut dispatcher = RecordingDispatcher::with_frame_count(Some(4));

    let mut budget = WorkBudget::new(1).unwrap();
    executor.run_slice(&program_a, &mut dispatcher, &mut budget);
    assert!(executor.cancel());
    assert_eq!(executor.state(), ExecutorState::Cancelled);
    assert!(!executor.cancel());
    let calls = dispatcher.calls.len();
    let mut budget = WorkBudget::new(1).unwrap();
    let report = executor.run_slice(&program_a, &mut dispatcher, &mut budget);
    assert_eq!(report.event, SliceEvent::Cancelled);
    assert_eq!(report.consumed, 0);
    assert_eq!(dispatcher.calls.len(), calls);

    executor.reset();
    assert_eq!(executor.state(), ExecutorState::Phase0);
    assert_eq!(executor.cursor(), OpCursor::new());
    assert_eq!(executor.resolved_phase(), None);
    assert!(executor.slot_bases().is_empty());
    assert!(!executor.is_terminal());

    let mut budget = WorkBudget::new(1).unwrap();
    executor.run_slice(&program_a, &mut dispatcher, &mut budget);
    let calls = dispatcher.calls.len();
    let mut budget = WorkBudget::new(1).unwrap();
    let report = executor.run_slice(&program_b, &mut dispatcher, &mut budget);
    assert_eq!(report.event, SliceEvent::Faulted(ExecutorFault::ForeignProgram));
    assert_eq!(report.consumed, 0);
    assert_eq!(dispatcher.calls.len(), calls);

    executor.reset();
    let mut budget = WorkBudget::new(1).unwrap();
    let report = executor.run_slice(&program_b, &mut dispatcher, &mut budget);
    assert_eq!(report.event, SliceEvent::BudgetExhausted);
    assert_eq!(dispatcher.calls.last().unwrap().artifact_sha256, *program_b.artifact_sha256());
}

fn fixture(arena_max: u64, model_tag: u8, resolver_count: u32) -> Vec<u8> {
    assert!((1..=2).contains(&resolver_count));
    let op_count = resolver_count + 1;

    let mut external = [0u8; TENSOR_RECORD_BYTES];
    external[0] = DType::I64 as u8;
    external[1] = 0;
    external[2] = StorageKind::External as u8;
    external[3] = Phase::Shared as u8;
    put_u32(&mut external, 8, NO_SLOT);
    put_u32(&mut external, 12, NO_TENSOR);
    put_u64(&mut external, 24, 8);
    for index in 0..4 {
        put_u32(&mut external, 32 + index * 4, 1);
    }
    external[80] = STATIC_DIM;
    put_u32(&mut external, 96, 8);

    let mut waveform = [0u8; TENSOR_RECORD_BYTES];
    waveform[0] = DType::F32 as u8;
    waveform[1] = 1;
    waveform[2] = StorageKind::Slot as u8;
    waveform[3] = Phase::Phase1 as u8;
    put_u32(&mut waveform, 4, TensorFlags::OUTPUT.bits());
    put_u32(&mut waveform, 8, 0);
    put_u32(&mut waveform, 12, NO_TENSOR);
    put_u64(&mut waveform, 24, 32);
    put_u32(&mut waveform, 32, 8);
    for index in 1..4 {
        put_u32(&mut waveform, 32 + index * 4, 1);
    }
    put_u64(&mut waveform, 48, 4);
    waveform[80] = 0;
    put_u32(&mut waveform, 84, 1);
    put_u32(&mut waveform, 96, 64);

    let mut slot = [0u8; SLOT_RECORD_BYTES];
    slot[0] = SlotKind::Dynamic as u8;
    slot[1] = Phase::Phase1 as u8;
    put_u32(&mut slot, 4, 64);
    put_u64(&mut slot, 16, 4);
    put_u32(&mut slot, 32, resolver_count);
    put_u32(&mut slot, 36, op_count + 1);

    let mut ops = Vec::new();
    let mut bindings = Vec::new();
    for resolver in 0..resolver_count {
        ops.extend_from_slice(&op_record(
            OpCode::ResolveDecoderShape as u16,
            Phase::Phase0 as u8,
            resolver,
            0,
            1,
            2,
        ));
        bindings.extend_from_slice(&0u32.to_le_bytes());
    }
    ops.extend_from_slice(&op_record(
        OpCode::Add as u16,
        Phase::Phase1 as u8,
        resolver_count,
        1,
        1,
        5,
    ));
    bindings.extend_from_slice(&0u32.to_le_bytes());
    bindings.extend_from_slice(&1u32.to_le_bytes());

    let phase0 = phase_record(0, 0, 0, resolver_count, 0, 0, 0, 0);
    let phase1 =
        phase_record(1, PHASE_FLAG_RUNTIME_SIZED, resolver_count, op_count, 0, arena_max, 1, 8);
    let sections = [
        records(&[external, waveform]),
        slot.to_vec(),
        ops,
        bindings,
        records(&[phase0, phase1]),
        Vec::new(),
    ];
    emit(&sections, model_tag)
}

fn op_record(
    opcode: u16,
    phase: u8,
    binding_start: u32,
    input_count: u16,
    output_count: u16,
    work_units: u32,
) -> [u8; OP_RECORD_BYTES] {
    let mut record = [0u8; OP_RECORD_BYTES];
    put_u16(&mut record, 0, opcode);
    record[4] = phase;
    put_u32(&mut record, 8, binding_start);
    put_u16(&mut record, 12, input_count);
    put_u16(&mut record, 14, output_count);
    put_u32(&mut record, 28, work_units);
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

fn emit(sections: &[Vec<u8>; SECTION_COUNT], model_tag: u8) -> Vec<u8> {
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
    artifact[MODEL_SHA256_OFFSET..MODEL_SHA256_OFFSET + 32].fill(model_tag);
    artifact[VOICES_SHA256_OFFSET..VOICES_SHA256_OFFSET + 32].fill(0x22);

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
    let seal = artifact_sha256(&artifact).unwrap();
    artifact[ARTIFACT_SHA256_OFFSET..ARTIFACT_SHA256_OFFSET + 32].copy_from_slice(&seal);
    artifact
}

fn records<const N: usize>(records: &[[u8; N]]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(records.len() * N);
    for record in records {
        bytes.extend_from_slice(record);
    }
    bytes
}

fn align_up(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
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
