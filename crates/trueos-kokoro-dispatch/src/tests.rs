use std::{
    fs,
    path::PathBuf,
    process::{Command, id},
    vec,
    vec::Vec,
};

use trueos_kokoro_aot::{
    OP_RECORD_BYTES, OpCode, Program, SECTION_DIRECTORY_OFFSET, SECTION_ENTRY_BYTES,
    TENSOR_RECORD_BYTES, TensorFlags, WorkBudget, artifact_sha256,
};
use trueos_kokoro_exec::{Executor, RuntimeShape, SliceEvent, TensorShapeTable};
use trueos_kokoro_memory::{ExternalBindings, TensorMemory};

use crate::{
    AttributeError, Attributes, CpuDispatcher, CpuWorkspace, DispatchError,
    KOKORO_CPU_WORKSPACE_REQUIREMENTS, WorkspaceError, decode,
    native_dispatch_requires_workspace, native_dispatch_supported, record_bytes,
};

#[repr(align(64))]
struct AlignedArtifact([u8; 32_768]);

#[repr(align(64))]
struct AlignedArena([u8; 64]);

#[repr(align(4))]
struct Aligned4<T>(T);

fn fixture() -> (Vec<u8>, PathBuf) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = PathBuf::from(
        manifest
            .parent()
            .and_then(|path| path.parent())
            .expect("dispatch crate is nested below the repository root"),
    );
    let output = std::env::temp_dir().join(std::format!(
        "trueos-kokoro-attribute-fixture-{}-{}.kkaot",
        id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let status = Command::new("python3")
        .arg(root.join("tools/ttstt/compile_kokoro_aot.py"))
        .arg("attribute-fixture")
        .arg(&output)
        .arg("--force")
        .status()
        .expect("python3 must execute the cross-language fixture compiler");
    assert!(status.success(), "Python fixture compiler failed: {status}");
    let artifact = fs::read(&output).expect("read generated attribute fixture");
    (artifact, output)
}

#[test]
fn python_fixture_decodes_all_56_records() {
    let (artifact, path) = fixture();
    let program = Program::parse(&artifact).expect("Python fixture must satisfy the AOT parser");
    assert_eq!(program.op_count(), 56);

    let mut decoded = 0_u32;
    let mut attribute_bytes = 0_usize;
    for op_index in 0..program.op_count() {
        let op = program.op(op_index).expect("operation descriptor");
        let record = program
            .op_attributes(op)
            .expect("every cross-language fixture op has attributes");
        assert_eq!(record_bytes(op.opcode), Some(record.len()));
        let attributes = decode(record, op.opcode).unwrap_or_else(|error| {
            panic!("record {op_index} ({:?}) rejected: {error:?}", op.opcode)
        });
        assert_eq!(attributes.opcode(), op.opcode);
        decoded += 1;
        attribute_bytes += record.len();
    }

    assert_eq!(decoded, 56);
    assert_eq!(attribute_bytes, 1_308);
    fs::remove_file(path).expect("remove generated test fixture");
}

#[test]
fn every_cross_language_record_rejects_common_header_corruption() {
    let (artifact, path) = fixture();
    let program = Program::parse(&artifact).expect("Python fixture must satisfy the AOT parser");

    for op_index in 0..program.op_count() {
        let op = program.op(op_index).expect("operation descriptor");
        let record = program.op_attributes(op).expect("attribute record");

        let mut wrong_version = record.to_vec();
        wrong_version[0..2].copy_from_slice(&2_u16.to_le_bytes());
        assert_eq!(
            decode(&wrong_version, op.opcode),
            Err(AttributeError::UnsupportedVersion { found: 2 })
        );

        let mut wrong_kind = record.to_vec();
        let foreign = if op.opcode == OpCode::Add {
            OpCode::Mul
        } else {
            OpCode::Add
        };
        wrong_kind[2..4].copy_from_slice(&(foreign as u16).to_le_bytes());
        assert_eq!(
            decode(&wrong_kind, op.opcode),
            Err(AttributeError::KindMismatch {
                expected: op.opcode,
                found: foreign as u16,
            })
        );

        let mut wrong_count = record.to_vec();
        wrong_count[4..8].copy_from_slice(&0_u32.to_le_bytes());
        assert_eq!(
            decode(&wrong_count, op.opcode),
            Err(AttributeError::ByteCountMismatch {
                header: 0,
                actual: record.len(),
            })
        );

        assert_eq!(decode(&record[..7], op.opcode), Err(AttributeError::Truncated));
        let mut unaligned = record.to_vec();
        unaligned.push(0);
        assert_eq!(decode(&unaligned, op.opcode), Err(AttributeError::UnalignedLength));
    }

    fs::remove_file(path).expect("remove generated test fixture");
}

#[test]
fn unsupported_source_opcodes_never_acquire_an_attribute_contract() {
    for opcode in [
        OpCode::Clip,
        OpCode::Conv,
        OpCode::ConvInteger,
        OpCode::ConvTranspose,
        OpCode::DynamicQuantizeLinear,
        OpCode::Lstm,
        OpCode::MatMulInteger,
        OpCode::ReduceSum,
        OpCode::Stft,
        OpCode::AddSoftmax,
        OpCode::AlbertAttention,
        OpCode::ElementwiseFusion,
    ] {
        assert_eq!(record_bytes(opcode), None, "{opcode:?}");
    }
}

#[test]
fn cpu_dispatcher_executes_first_fixture_add_through_typed_memory() {
    let (mut artifact, path) = fixture();
    let tensor_offset = u64::from_le_bytes(artifact[168..176].try_into().unwrap()) as usize;
    artifact[tensor_offset + 4..tensor_offset + 8].copy_from_slice(&2_u32.to_le_bytes());
    artifact[tensor_offset + 128 + 4..tensor_offset + 128 + 8]
        .copy_from_slice(&2_u32.to_le_bytes());
    artifact[tensor_offset + 256 + 4..tensor_offset + 256 + 8]
        .copy_from_slice(&4_u32.to_le_bytes());
    let seal = artifact_sha256(&artifact).unwrap();
    artifact[64..96].copy_from_slice(&seal);
    assert!(artifact.len() <= 32_768);
    let mut aligned = AlignedArtifact([0; 32_768]);
    aligned.0[..artifact.len()].copy_from_slice(&artifact);
    let program = Program::parse(&aligned.0[..artifact.len()]).unwrap();

    let mut shapes: TensorShapeTable<256> = TensorShapeTable::new();
    shapes.initialize(&program).unwrap();
    shapes
        .bind_external(&program, 0, RuntimeShape::new(&[1, 4]).unwrap())
        .unwrap();
    shapes
        .bind_external(&program, 1, RuntimeShape::new(&[4]).unwrap())
        .unwrap();
    shapes
        .bind_external(&program, 2, RuntimeShape::new(&[1, 4]).unwrap())
        .unwrap();

    let lhs = [1.0_f32, 2.0, 3.0, 4.0];
    let rhs = [10.0_f32, 20.0, 30.0, 40.0];
    let mut output = [0.0_f32; 4];
    let mut bindings: ExternalBindings<'_, 3> = ExternalBindings::new();
    bindings.bind_input(&program, &shapes, 0, &lhs).unwrap();
    bindings.bind_input(&program, &shapes, 1, &rhs).unwrap();
    bindings
        .bind_output(&program, &shapes, 2, &mut output)
        .unwrap();
    let mut arena = AlignedArena([0; 64]);

    {
        let mut memory: TensorMemory<'_, '_, '_, 256, 3, 8> =
            TensorMemory::phase_zero(&program, &mut shapes, &mut arena.0, &mut bindings).unwrap();
        let mut dispatcher = CpuDispatcher::new(&mut memory);
        let mut executor: Executor<1> = Executor::new();
        let mut budget = WorkBudget::new(1).unwrap();
        let report = executor.run_slice(&program, &mut dispatcher, &mut budget);
        assert!(matches!(report.event, SliceEvent::BudgetExhausted));
        assert_eq!(report.consumed, 1);
    }

    assert_eq!(output, [11.0, 22.0, 33.0, 44.0]);
    fs::remove_file(path).expect("remove generated test fixture");
}

#[test]
fn cpu_resize_attests_the_runtime_prefix_and_skips_sealed_capacity() {
    let (mut artifact, path, inputs, outputs) = selected_fixture(OpCode::Resize);
    assert_eq!((inputs.len(), outputs.len()), (2, 1));
    let op_offset = fixture_section_offset(&artifact, 2);
    artifact[op_offset + 28..op_offset + 32].copy_from_slice(&128_u32.to_le_bytes());
    let seal = artifact_sha256(&artifact).unwrap();
    artifact[64..96].copy_from_slice(&seal);

    let mut aligned = AlignedArtifact([0; 32_768]);
    aligned.0[..artifact.len()].copy_from_slice(&artifact);
    let program = Program::parse(&aligned.0[..artifact.len()]).unwrap();
    let mut shapes: TensorShapeTable<256> = TensorShapeTable::new();
    shapes.initialize(&program).unwrap();
    shapes
        .bind_external(&program, inputs[0], RuntimeShape::new(&[1, 1, 2]).unwrap())
        .unwrap();
    shapes
        .bind_external(&program, inputs[1], RuntimeShape::new(&[3]).unwrap())
        .unwrap();
    shapes
        .bind_external(&program, outputs[0], RuntimeShape::new(&[1, 1, 4]).unwrap())
        .unwrap();

    let input = [3.0_f32, 7.0];
    let scales = [1.0_f32, 1.0, 2.0];
    let mut output = [0.0_f32; 4];
    let mut bindings: ExternalBindings<'_, 3> = ExternalBindings::new();
    bindings
        .bind_input(&program, &shapes, inputs[0], &input)
        .unwrap();
    bindings
        .bind_input(&program, &shapes, inputs[1], &scales)
        .unwrap();
    bindings
        .bind_output(&program, &shapes, outputs[0], &mut output)
        .unwrap();
    let mut arena = AlignedArena([0; 64]);

    {
        let mut memory: TensorMemory<'_, '_, '_, 256, 3, 8> =
            TensorMemory::phase_zero(&program, &mut shapes, &mut arena.0, &mut bindings).unwrap();
        let mut dispatcher = CpuDispatcher::new(&mut memory);
        let mut executor: Executor<1> = Executor::new();
        let mut budget = WorkBudget::new(8).unwrap();
        let report = executor.run_slice(&program, &mut dispatcher, &mut budget);
        assert!(matches!(report.event, SliceEvent::BudgetExhausted));
        assert_eq!((report.consumed, report.remaining), (8, 0));
        assert_eq!((executor.cursor().op_index(), executor.cursor().unit_offset()), (1, 0));
    }

    assert_eq!(output, [3.0, 3.0, 7.0, 7.0]);
    fs::remove_file(path).expect("remove generated test fixture");
}

#[test]
fn cpu_dispatcher_materializes_fixture_transpose() {
    let (mut artifact, path) = fixture();
    let original = Program::parse(&artifact).unwrap();
    let transpose_index = (0..original.op_count())
        .find(|&index| original.op(index).unwrap().opcode == OpCode::Transpose)
        .unwrap();
    let transpose = original.op(transpose_index).unwrap();
    let input_id = original.op_input(transpose, 0).unwrap();
    let output_id = original.op_output(transpose, 0).unwrap();

    let op_offset = fixture_section_offset(&artifact, 2);
    let selected = op_offset + transpose_index as usize * OP_RECORD_BYTES;
    artifact.copy_within(selected..selected + OP_RECORD_BYTES, op_offset);
    let tensor_offset = fixture_section_offset(&artifact, 0);
    artifact[tensor_offset + input_id as usize * TENSOR_RECORD_BYTES + 4
        ..tensor_offset + input_id as usize * TENSOR_RECORD_BYTES + 8]
        .copy_from_slice(&2_u32.to_le_bytes());
    artifact[tensor_offset + output_id as usize * TENSOR_RECORD_BYTES + 4
        ..tensor_offset + output_id as usize * TENSOR_RECORD_BYTES + 8]
        .copy_from_slice(&4_u32.to_le_bytes());
    let seal = artifact_sha256(&artifact).unwrap();
    artifact[64..96].copy_from_slice(&seal);

    let mut aligned = AlignedArtifact([0; 32_768]);
    aligned.0[..artifact.len()].copy_from_slice(&artifact);
    let program = Program::parse(&aligned.0[..artifact.len()]).unwrap();
    let mut shapes: TensorShapeTable<256> = TensorShapeTable::new();
    shapes.initialize(&program).unwrap();
    shapes
        .bind_external(&program, input_id, RuntimeShape::new(&[1, 2, 3]).unwrap())
        .unwrap();
    shapes
        .bind_external(&program, output_id, RuntimeShape::new(&[1, 3, 2]).unwrap())
        .unwrap();

    let input = [1.0_f32, 2.0, 3.0, 4.0, 5.0, 6.0];
    let mut output = [0.0_f32; 6];
    let mut bindings: ExternalBindings<'_, 2> = ExternalBindings::new();
    bindings
        .bind_input(&program, &shapes, input_id, &input)
        .unwrap();
    bindings
        .bind_output(&program, &shapes, output_id, &mut output)
        .unwrap();
    let mut arena = AlignedArena([0; 64]);
    {
        let mut memory: TensorMemory<'_, '_, '_, 256, 2, 8> =
            TensorMemory::phase_zero(&program, &mut shapes, &mut arena.0, &mut bindings).unwrap();
        let mut dispatcher = CpuDispatcher::new(&mut memory);
        let mut executor: Executor<1> = Executor::new();
        let mut budget = WorkBudget::new(1).unwrap();
        let report = executor.run_slice(&program, &mut dispatcher, &mut budget);
        assert!(matches!(report.event, SliceEvent::BudgetExhausted));
        assert_eq!(report.consumed, 1);
    }
    assert_eq!(output, [1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
    fs::remove_file(path).expect("remove generated test fixture");
}

#[test]
fn cpu_dispatcher_executes_attention_score_matmul_through_typed_memory() {
    let (artifact, path, lhs_id, rhs_id, output_id) = matmul_fixture(1);
    let mut aligned = AlignedArtifact([0; 32_768]);
    aligned.0[..artifact.len()].copy_from_slice(&artifact);
    let program = Program::parse(&aligned.0[..artifact.len()]).unwrap();

    let lhs_shape = RuntimeShape::new(&[1, 12, 2, 64]).unwrap();
    let rhs_shape = RuntimeShape::new(&[1, 12, 64, 2]).unwrap();
    let output_shape = RuntimeShape::new(&[1, 12, 2, 2]).unwrap();
    let mut shapes: TensorShapeTable<256> = TensorShapeTable::new();
    shapes.initialize(&program).unwrap();
    shapes.bind_external(&program, lhs_id, lhs_shape).unwrap();
    shapes.bind_external(&program, rhs_id, rhs_shape).unwrap();
    shapes
        .bind_external(&program, output_id, output_shape)
        .unwrap();

    let lhs: Vec<f32> = (0..12 * 2 * 64)
        .map(|index| (index % (2 * 64) / 64 + 1) as f32)
        .collect();
    let rhs: Vec<f32> = (0..12 * 64 * 2)
        .map(|index| (index % 2 + 1) as f32)
        .collect();
    let mut output = vec![0.0_f32; 12 * 2 * 2];
    let mut bindings: ExternalBindings<'_, 3> = ExternalBindings::new();
    bindings
        .bind_input(&program, &shapes, lhs_id, &lhs)
        .unwrap();
    bindings
        .bind_input(&program, &shapes, rhs_id, &rhs)
        .unwrap();
    bindings
        .bind_output(&program, &shapes, output_id, &mut output)
        .unwrap();
    let mut arena = AlignedArena([0; 64]);
    {
        let mut memory: TensorMemory<'_, '_, '_, 256, 3, 8> =
            TensorMemory::phase_zero(&program, &mut shapes, &mut arena.0, &mut bindings).unwrap();
        let mut dispatcher = CpuDispatcher::new(&mut memory);
        let mut executor: Executor<1> = Executor::new();
        let mut budget = WorkBudget::new(1).unwrap();
        let report = executor.run_slice(&program, &mut dispatcher, &mut budget);
        assert!(matches!(report.event, SliceEvent::BudgetExhausted));
        assert_eq!(report.consumed, 1);
    }

    for head in 0..12 {
        assert_eq!(&output[head * 4..head * 4 + 4], &[64.0, 128.0, 128.0, 256.0]);
    }
    fs::remove_file(path).expect("remove generated test fixture");
}

#[test]
fn cpu_dispatcher_rejects_non_atomic_matmul_work_before_writing_output() {
    let (artifact, path, lhs_id, rhs_id, output_id) = matmul_fixture(2);
    let mut aligned = AlignedArtifact([0; 32_768]);
    aligned.0[..artifact.len()].copy_from_slice(&artifact);
    let program = Program::parse(&aligned.0[..artifact.len()]).unwrap();

    let mut shapes: TensorShapeTable<256> = TensorShapeTable::new();
    shapes.initialize(&program).unwrap();
    shapes
        .bind_external(&program, lhs_id, RuntimeShape::new(&[1, 12, 2, 64]).unwrap())
        .unwrap();
    shapes
        .bind_external(&program, rhs_id, RuntimeShape::new(&[1, 12, 64, 2]).unwrap())
        .unwrap();
    shapes
        .bind_external(&program, output_id, RuntimeShape::new(&[1, 12, 2, 2]).unwrap())
        .unwrap();

    let lhs = vec![1.0_f32; 12 * 2 * 64];
    let rhs = vec![1.0_f32; 12 * 64 * 2];
    let mut output = vec![37.0_f32; 12 * 2 * 2];
    let mut bindings: ExternalBindings<'_, 3> = ExternalBindings::new();
    bindings
        .bind_input(&program, &shapes, lhs_id, &lhs)
        .unwrap();
    bindings
        .bind_input(&program, &shapes, rhs_id, &rhs)
        .unwrap();
    bindings
        .bind_output(&program, &shapes, output_id, &mut output)
        .unwrap();
    let mut arena = AlignedArena([0; 64]);
    {
        let mut memory: TensorMemory<'_, '_, '_, 256, 3, 8> =
            TensorMemory::phase_zero(&program, &mut shapes, &mut arena.0, &mut bindings).unwrap();
        let mut dispatcher = CpuDispatcher::new(&mut memory);
        let mut executor: Executor<1> = Executor::new();
        let mut budget = WorkBudget::new(1).unwrap();
        let report = executor.run_slice(&program, &mut dispatcher, &mut budget);
        assert!(matches!(
            report.event,
            SliceEvent::DispatchFailed(crate::DispatchError::InvalidWorkContract {
                opcode: OpCode::MatMul
            })
        ));
        assert_eq!(report.consumed, 1);
    }
    assert!(output.iter().all(|&value| value == 37.0));
    fs::remove_file(path).expect("remove generated test fixture");
}

struct WorkspaceBuffers {
    quant_u8: Vec<u8>,
    packed_i8: Vec<i8>,
    accum_i32: Vec<i32>,
    row_sums_i32: Vec<i32>,
    bias_i32: Vec<i32>,
    lstm_gates_f32: Vec<f32>,
}

impl WorkspaceBuffers {
    fn pinned() -> Self {
        let required = KOKORO_CPU_WORKSPACE_REQUIREMENTS;
        Self {
            quant_u8: vec![0; required.quant_u8],
            packed_i8: vec![0; required.packed_i8],
            accum_i32: vec![0; required.accum_i32],
            row_sums_i32: vec![0; required.row_sums_i32],
            bias_i32: vec![0; required.bias_i32],
            lstm_gates_f32: vec![0.0; required.lstm_gates_f32],
        }
    }

    fn workspace(&mut self) -> CpuWorkspace<'_> {
        CpuWorkspace::new(
            &mut self.quant_u8,
            &mut self.packed_i8,
            &mut self.accum_i32,
            &mut self.row_sums_i32,
            &mut self.bias_i32,
            &mut self.lstm_gates_f32,
        )
        .unwrap()
    }
}

fn selected_fixture(opcode: OpCode) -> (Vec<u8>, PathBuf, Vec<u32>, Vec<u32>) {
    let (mut artifact, path) = fixture();
    let (op_index, input_ids, output_ids) = {
        let program = Program::parse(&artifact).unwrap();
        let op_index = (0..program.op_count())
            .find(|&index| program.op(index).unwrap().opcode == opcode)
            .unwrap();
        let op = program.op(op_index).unwrap();
        let inputs = (0..op.input_count)
            .map(|input| program.op_input(op, input).unwrap())
            .collect::<Vec<_>>();
        let outputs = (0..op.output_count)
            .map(|output| program.op_output(op, output).unwrap())
            .collect::<Vec<_>>();
        (op_index, inputs, outputs)
    };

    let op_offset = fixture_section_offset(&artifact, 2);
    let selected = op_offset + op_index as usize * OP_RECORD_BYTES;
    artifact.copy_within(selected..selected + OP_RECORD_BYTES, op_offset);
    for &tensor in &input_ids {
        rewrite_external_flags(&mut artifact, tensor, TensorFlags::INPUT);
    }
    for &tensor in &output_ids {
        rewrite_external_flags(&mut artifact, tensor, TensorFlags::OUTPUT);
    }
    let seal = artifact_sha256(&artifact).unwrap();
    artifact[64..96].copy_from_slice(&seal);
    (artifact, path, input_ids, output_ids)
}

fn rewrite_external_flags(artifact: &mut [u8], tensor_id: u32, flags: TensorFlags) {
    let offset = fixture_section_offset(artifact, 0) + tensor_id as usize * TENSOR_RECORD_BYTES;
    assert_eq!(artifact[offset + 2], 4, "fixture tensor must be external");
    artifact[offset + 4..offset + 8].copy_from_slice(&flags.bits().to_le_bytes());
}

fn bind_fixture_shapes<const SHAPES: usize>(
    program: &Program<'_>,
    shapes: &mut TensorShapeTable<SHAPES>,
    tensor_ids: &[u32],
) {
    for &tensor_id in tensor_ids {
        let tensor = program.tensor(tensor_id).unwrap();
        let shape = RuntimeShape::new(&tensor.max_dims[..usize::from(tensor.rank)]).unwrap();
        shapes.bind_external(program, tensor_id, shape).unwrap();
    }
}

#[test]
fn quantized_gemm_dispatch_matches_centered_onnx_arithmetic_and_requires_workspace() {
    let (artifact, path, inputs, outputs) = selected_fixture(OpCode::DynamicQuantizedGemm);
    let mut aligned = AlignedArtifact([0; 32_768]);
    aligned.0[..artifact.len()].copy_from_slice(&artifact);
    let program = Program::parse(&aligned.0[..artifact.len()]).unwrap();
    assert_eq!(inputs.len(), 5);
    assert_eq!(outputs.len(), 1);

    let activation = [-1.0_f32, 0.25, 2.0, -0.5];
    let weights = Aligned4([2_i8, -3, 7, -5, 4, 1, 6, -8, 3, 9, 2, -4]);
    let weight_scales = [0.125_f32, 0.25, 0.5];
    let weight_zero_points = Aligned4([1_i8, -1, 2]);
    let bias = [0.5_f32, -1.0, 2.0];
    let mut shapes: TensorShapeTable<256> = TensorShapeTable::new();
    shapes.initialize(&program).unwrap();
    bind_fixture_shapes(&program, &mut shapes, &inputs);
    bind_fixture_shapes(&program, &mut shapes, &outputs);
    let mut arena = AlignedArena([0; 64]);

    {
        let mut rejected_output = [91.0_f32; 3];
        let mut bindings: ExternalBindings<'_, 6> = ExternalBindings::new();
        bindings.bind_input(&program, &shapes, inputs[0], &activation).unwrap();
        bindings.bind_input(&program, &shapes, inputs[1], &weights.0).unwrap();
        bindings
            .bind_input(&program, &shapes, inputs[2], &weight_scales)
            .unwrap();
        bindings
            .bind_input(&program, &shapes, inputs[3], &weight_zero_points.0)
            .unwrap();
        bindings.bind_input(&program, &shapes, inputs[4], &bias).unwrap();
        bindings
            .bind_output(&program, &shapes, outputs[0], &mut rejected_output)
            .unwrap();
        let mut memory: TensorMemory<'_, '_, '_, 256, 6, 8> =
            TensorMemory::phase_zero(&program, &mut shapes, &mut arena.0, &mut bindings).unwrap();
        let mut dispatcher = CpuDispatcher::new(&mut memory);
        let mut executor: Executor<1> = Executor::new();
        let mut budget = WorkBudget::new(1).unwrap();
        let report = executor.run_slice(&program, &mut dispatcher, &mut budget);
        assert!(matches!(
            report.event,
            SliceEvent::DispatchFailed(DispatchError::WorkspaceRequired {
                opcode: OpCode::DynamicQuantizedGemm
            })
        ));
        drop(dispatcher);
        drop(memory);
        drop(bindings);
        assert_eq!(rejected_output, [91.0; 3]);
    }

    let mut output = [91.0_f32; 3];
    let mut bindings: ExternalBindings<'_, 6> = ExternalBindings::new();
    bindings.bind_input(&program, &shapes, inputs[0], &activation).unwrap();
    bindings.bind_input(&program, &shapes, inputs[1], &weights.0).unwrap();
    bindings
        .bind_input(&program, &shapes, inputs[2], &weight_scales)
        .unwrap();
    bindings
        .bind_input(&program, &shapes, inputs[3], &weight_zero_points.0)
        .unwrap();
    bindings.bind_input(&program, &shapes, inputs[4], &bias).unwrap();
    bindings
        .bind_output(&program, &shapes, outputs[0], &mut output)
        .unwrap();
    let mut workspace_buffers = WorkspaceBuffers::pinned();
    {
        let mut memory: TensorMemory<'_, '_, '_, 256, 6, 8> =
            TensorMemory::phase_zero(&program, &mut shapes, &mut arena.0, &mut bindings).unwrap();
        let mut workspace = workspace_buffers.workspace();
        let mut dispatcher = CpuDispatcher::new_with_workspace(&mut memory, &mut workspace);
        let mut executor: Executor<1> = Executor::new();
        let mut budget = WorkBudget::new(1).unwrap();
        let report = executor.run_slice(&program, &mut dispatcher, &mut budget);
        assert!(matches!(report.event, SliceEvent::BudgetExhausted));
        assert_eq!(report.consumed, 1);
    }

    let mut quantized = [0_u8; 4];
    let quantization =
        trueos_ttstt_cpu::dynamic_quantize_linear_u8(&activation, &mut quantized).unwrap();
    let expected = core::array::from_fn(|column| {
        let mut accumulator = 0_i32;
        for reduction in 0..4 {
            accumulator += (i32::from(quantized[reduction])
                - i32::from(quantization.zero_point))
                * (i32::from(weights.0[reduction * 3 + column])
                    - i32::from(weight_zero_points.0[column]));
        }
        accumulator as f32 * (quantization.scale * weight_scales[column]) + bias[column]
    });
    assert_eq!(output.map(f32::to_bits), expected.map(f32::to_bits));
    fs::remove_file(path).unwrap();
}

fn matmul_fixture(work_units: u32) -> (Vec<u8>, PathBuf, u32, u32, u32) {
    let (mut artifact, path) = fixture();
    let (matmul_index, lhs_id, rhs_id, output_id) = {
        let program = Program::parse(&artifact).unwrap();
        let index = (0..program.op_count())
            .find(|&candidate| program.op(candidate).unwrap().opcode == OpCode::MatMul)
            .unwrap();
        let op = program.op(index).unwrap();
        (
            index,
            program.op_input(op, 0).unwrap(),
            program.op_input(op, 1).unwrap(),
            program.op_output(op, 0).unwrap(),
        )
    };
    let op_offset = fixture_section_offset(&artifact, 2);
    let selected = op_offset + matmul_index as usize * OP_RECORD_BYTES;
    artifact.copy_within(selected..selected + OP_RECORD_BYTES, op_offset);
    artifact[op_offset + 28..op_offset + 32].copy_from_slice(&work_units.to_le_bytes());
    rewrite_external_f32_tensor(&mut artifact, lhs_id, TensorFlags::INPUT, &[1, 12, 4, 64]);
    rewrite_external_f32_tensor(&mut artifact, rhs_id, TensorFlags::INPUT, &[1, 12, 64, 4]);
    rewrite_external_f32_tensor(&mut artifact, output_id, TensorFlags::OUTPUT, &[1, 12, 4, 4]);
    let seal = artifact_sha256(&artifact).unwrap();
    artifact[64..96].copy_from_slice(&seal);
    (artifact, path, lhs_id, rhs_id, output_id)
}

fn rewrite_external_f32_tensor(
    artifact: &mut [u8],
    tensor_id: u32,
    flags: TensorFlags,
    dims: &[u32],
) {
    let offset = fixture_section_offset(artifact, 0) + tensor_id as usize * TENSOR_RECORD_BYTES;
    assert_eq!(artifact[offset], 1);
    assert_eq!(usize::from(artifact[offset + 1]), dims.len());
    artifact[offset + 4..offset + 8].copy_from_slice(&flags.bits().to_le_bytes());
    let mut capacity = 4_u64;
    for &dimension in dims {
        capacity = capacity.checked_mul(u64::from(dimension)).unwrap();
    }
    artifact[offset + 24..offset + 32].copy_from_slice(&capacity.to_le_bytes());
    let mut padded = [1_u32; 4];
    padded[..dims.len()].copy_from_slice(dims);
    for (axis, dimension) in padded.into_iter().enumerate() {
        artifact[offset + 32 + axis * 4..offset + 36 + axis * 4]
            .copy_from_slice(&dimension.to_le_bytes());
    }
    let mut strides = [0_u64; 4];
    let mut stride = 4_u64;
    for axis in (0..dims.len()).rev() {
        strides[axis] = stride;
        stride = stride.checked_mul(u64::from(dims[axis])).unwrap();
    }
    for (axis, stride) in strides.into_iter().enumerate() {
        artifact[offset + 48 + axis * 8..offset + 56 + axis * 8]
            .copy_from_slice(&stride.to_le_bytes());
    }
}

fn fixture_section_offset(artifact: &[u8], section: usize) -> usize {
    let entry = SECTION_DIRECTORY_OFFSET + section * SECTION_ENTRY_BYTES;
    u64::from_le_bytes(artifact[entry + 8..entry + 16].try_into().unwrap()) as usize
}
