#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

//! Cooperative bidirectional LSTM execution for the sealed Kokoro graph.
//!
//! The prepared graph contains exactly six ONNX LSTM nodes. All use batch one,
//! two directions, 256 hidden channels, IOFC gate order, the default
//! Sigmoid/Tanh/Tanh activations, `input_forget=0`, and explicit zero initial
//! state. The text encoder consumes 512 channels; the other five nodes consume
//! 640. This crate deliberately implements only that contract.
//!
//! [`CooperativeLstm::advance`] evaluates at most one direction/time-step pair.
//! Model data, output, recurrent state, and gate scratch are all caller-owned.
//! A dense implementation is supplied through [`DenseKernel`], allowing the
//! scalar reference to be replaced by an AVX2 or GPU-backed implementation
//! without duplicating recurrent-state logic.
//!
//! [`DispatchedDense`] probes AVX2, FMA, OSXSAVE, and XCR0 at runtime. Its
//! direct path gathers eight row-major weights at a time; optional
//! [`PrepackedMatrix`] views replace gathers with contiguous loads while all
//! storage remains caller-owned. Neither path uses AVX-512 or changes a row's
//! ascending-K fused accumulation order.
//!
//! Default Sigmoid/Tanh activations use the bounded rational approximations
//! from ONNX Runtime's MLAS FMA3 kernels. The scalar port keeps the exact
//! single-precision Horner/FMA order, so activation results do not depend on a
//! hosted math library and remain identical on the bare-metal target.
//!
//! On the development i9-13900K, the release `dense_bench` example measured a
//! `[1024, 512]` accumulation at 1.434 ms scalar, 0.053 ms gathered AVX2
//! (27.31x), and 0.040 ms prepacked AVX2 (35.70x). `[1024, 640]` measured
//! 1.784 ms, 0.071 ms (25.17x), and 0.052 ms (34.31x), respectively. One-time
//! packing cost 0.410 ms for K=512 and 0.548 ms for K=640.

use core::convert::Infallible;

/// LSTM nodes admitted from the pinned Kokoro graph.
pub const PINNED_NODE_COVERAGE: usize = 6;
/// Stable node numbers in the prepared-model audit.
pub const PINNED_NODE_IDS: [u16; PINNED_NODE_COVERAGE] = [740, 1686, 1700, 1714, 1728, 1776];
/// Hidden channels in every pinned LSTM node.
pub const HIDDEN_SIZE: usize = 256;
/// Forward and reverse directions in every pinned LSTM node.
pub const DIRECTIONS: usize = 2;
/// Input, output, forget, and cell gates, in that order.
pub const GATE_COUNT: usize = 4;
/// Elements in one direction's concatenated IOFC gate vector.
pub const GATE_ELEMENTS: usize = GATE_COUNT * HIDDEN_SIZE;
/// Input and recurrent bias elements for one direction.
pub const BIAS_ELEMENTS_PER_DIRECTION: usize = 2 * GATE_ELEMENTS;
/// Maximum sequence accepted by the sealed Kokoro graph.
pub const MAX_SEQUENCE_LENGTH: usize = 512;
/// Elements required for caller-owned hidden or cell state.
pub const STATE_ELEMENTS: usize = DIRECTIONS * HIDDEN_SIZE;
/// Elements required for caller-owned gate scratch.
pub const GATE_SCRATCH_ELEMENTS: usize = GATE_ELEMENTS;
/// Elements in the fixed `[2, 1024, 256]` recurrent-weight tensor.
pub const RECURRENT_WEIGHT_ELEMENTS: usize = DIRECTIONS * GATE_ELEMENTS * HIDDEN_SIZE;
/// Elements in the fixed `[2, 2048]` bias tensor.
pub const BIAS_ELEMENTS: usize = DIRECTIONS * BIAS_ELEMENTS_PER_DIRECTION;

const INPUT_GATE_OFFSET: usize = 0;
const OUTPUT_GATE_OFFSET: usize = HIDDEN_SIZE;
const FORGET_GATE_OFFSET: usize = 2 * HIDDEN_SIZE;
const CELL_GATE_OFFSET: usize = 3 * HIDDEN_SIZE;

/// The two input widths present in the six pinned graph nodes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputWidth {
    /// `/encoder/text_encoder/lstm/LSTM_quant__standard` (node 740).
    Text512,
    /// The five duration/prosody LSTMs (nodes 1686, 1700, 1714, 1728, 1776).
    Prosody640,
}

impl InputWidth {
    pub const fn channels(self) -> usize {
        match self {
            Self::Text512 => 512,
            Self::Prosody640 => 640,
        }
    }

    /// Elements in this family's `[2, 1024, K]` input-weight tensor.
    pub const fn weight_elements(self) -> usize {
        DIRECTIONS * GATE_ELEMENTS * self.channels()
    }
}

/// A rejected contract. Constructors validate exact lengths rather than
/// accepting prefixes, preventing tensors from adjacent graph nodes from being
/// confused with one another.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractError {
    EmptySequence,
    SequenceTooLong,
    InputLengthMismatch,
    WeightLengthMismatch,
    RecurrentWeightLengthMismatch,
    BiasLengthMismatch,
    OutputLengthMismatch,
    HiddenLengthMismatch,
    CellLengthMismatch,
    GateScratchLengthMismatch,
    InitialHiddenLengthMismatch,
    InitialCellLengthMismatch,
}

/// Immutable tensors for one pinned bidirectional LSTM invocation.
///
/// Layouts are the ONNX layout-0 forms:
///
/// - `input`: `[sequence, 1, K]`
/// - `weights`: `[2, 1024, K]`
/// - `recurrent_weights`: `[2, 1024, 256]`
/// - `bias`: `[2, 2048]`, with Wb(IOFC) followed by Rb(IOFC)
#[derive(Clone, Copy, Debug)]
pub struct Problem<'a> {
    sequence_length: usize,
    input_width: InputWidth,
    input: &'a [f32],
    weights: &'a [f32],
    recurrent_weights: &'a [f32],
    bias: &'a [f32],
}

impl<'a> Problem<'a> {
    /// Validate and seal one model invocation.
    pub fn new(
        sequence_length: usize,
        input_width: InputWidth,
        input: &'a [f32],
        weights: &'a [f32],
        recurrent_weights: &'a [f32],
        bias: &'a [f32],
    ) -> Result<Self, ContractError> {
        if sequence_length == 0 {
            return Err(ContractError::EmptySequence);
        }
        if sequence_length > MAX_SEQUENCE_LENGTH {
            return Err(ContractError::SequenceTooLong);
        }

        let channels = input_width.channels();
        if input.len() != sequence_length * channels {
            return Err(ContractError::InputLengthMismatch);
        }
        if weights.len() != input_width.weight_elements() {
            return Err(ContractError::WeightLengthMismatch);
        }
        if recurrent_weights.len() != RECURRENT_WEIGHT_ELEMENTS {
            return Err(ContractError::RecurrentWeightLengthMismatch);
        }
        if bias.len() != BIAS_ELEMENTS {
            return Err(ContractError::BiasLengthMismatch);
        }

        Ok(Self {
            sequence_length,
            input_width,
            input,
            weights,
            recurrent_weights,
            bias,
        })
    }

    pub const fn sequence_length(&self) -> usize {
        self.sequence_length
    }

    pub const fn input_width(&self) -> InputWidth {
        self.input_width
    }

    pub const fn output_elements(&self) -> usize {
        self.sequence_length * DIRECTIONS * HIDDEN_SIZE
    }
}

/// Caller-owned output and recurrent workspace.
///
/// `output` uses ONNX layout `[sequence, 2, 1, 256]`. `hidden` and `cell`
/// each use `[2, 1, 256]` and become the `Y_h` and `Y_c` results. The scratch
/// vector holds one `[1, 1024]` IOFC gate row.
pub struct Buffers<'a> {
    output: &'a mut [f32],
    hidden: &'a mut [f32],
    cell: &'a mut [f32],
    gates: &'a mut [f32],
}

impl<'a> Buffers<'a> {
    pub fn new(
        output: &'a mut [f32],
        hidden: &'a mut [f32],
        cell: &'a mut [f32],
        gates: &'a mut [f32],
    ) -> Self {
        Self {
            output,
            hidden,
            cell,
            gates,
        }
    }

    pub fn output(&self) -> &[f32] {
        self.output
    }

    pub fn hidden(&self) -> &[f32] {
        self.hidden
    }

    pub fn cell(&self) -> &[f32] {
        self.cell
    }

    pub fn gates(&self) -> &[f32] {
        self.gates
    }
}

/// Pluggable row-major dense matrix-vector accumulation.
///
/// Implement `accumulator += matrix * vector`, where `matrix` is
/// `[rows, columns]`, `vector` is `[columns]`, and `accumulator` is `[rows]`.
/// Every slice supplied by this crate has exactly the documented length. An
/// implementation may change `accumulator` before returning an error; the LSTM
/// executor treats the gate vector as disposable scratch and will reinitialize
/// it on retry. It must not retain any supplied reference after returning.
pub trait DenseKernel {
    type Error;

    fn accumulate(
        &mut self,
        rows: usize,
        columns: usize,
        matrix: &[f32],
        vector: &[f32],
        accumulator: &mut [f32],
    ) -> Result<(), Self::Error>;
}

/// Deterministic ascending-K scalar reference dense kernel.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScalarDense;

impl DenseKernel for ScalarDense {
    type Error = Infallible;

    fn accumulate(
        &mut self,
        rows: usize,
        columns: usize,
        matrix: &[f32],
        vector: &[f32],
        accumulator: &mut [f32],
    ) -> Result<(), Self::Error> {
        debug_assert_eq!(matrix.len(), rows * columns);
        debug_assert_eq!(vector.len(), columns);
        debug_assert_eq!(accumulator.len(), rows);

        for (row, output) in matrix.chunks_exact(columns).zip(accumulator.iter_mut()) {
            let mut sum = *output;
            for (&weight, &value) in row.iter().zip(vector) {
                sum = libm::fmaf(weight, value, sum);
            }
            *output = sum;
        }
        Ok(())
    }
}

/// CPU implementation selected for one dense accumulation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DenseLane {
    Scalar,
    Avx2Fma,
}

impl DenseLane {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scalar => "scalar-fma",
            Self::Avx2Fma => "avx2-fma-256",
        }
    }
}

/// Concrete path used by the most recent dispatched accumulation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DensePath {
    Scalar,
    Avx2Gather,
    Avx2Prepacked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DenseError {
    UnsupportedShape,
    ShapeOverflow,
    MatrixLengthMismatch,
    VectorLengthMismatch,
    AccumulatorLengthMismatch,
    PackedLengthMismatch,
    Aliasing,
    UnsupportedLane,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CpuCapabilities {
    ymm_state: bool,
    avx2: bool,
    fma: bool,
}

impl CpuCapabilities {
    pub const fn ymm_state(self) -> bool {
        self.ymm_state
    }

    pub const fn avx2(self) -> bool {
        self.avx2
    }

    pub const fn fma(self) -> bool {
        self.fma
    }

    pub const fn supports(self, lane: DenseLane) -> bool {
        match lane {
            DenseLane::Scalar => true,
            DenseLane::Avx2Fma => self.ymm_state && self.avx2 && self.fma,
        }
    }

    pub const fn best_lane(self) -> DenseLane {
        if self.supports(DenseLane::Avx2Fma) {
            DenseLane::Avx2Fma
        } else {
            DenseLane::Scalar
        }
    }
}

/// Caller-owned rows-of-eight weight packing for one pinned dense matrix.
///
/// Source is row-major `[1024, K]`. Packed storage contains
/// `[row_block, K, lane]`, where each row block has eight adjacent source rows.
/// This keeps each lane's ascending-K FMA order unchanged while replacing the
/// AVX2 gather used for source weights with one contiguous 256-bit load.
#[derive(Clone, Copy, Debug)]
pub struct PrepackedMatrix<'a> {
    source: &'a [f32],
    packed: &'a [f32],
    columns: usize,
}

impl<'a> PrepackedMatrix<'a> {
    /// Reorder one matrix into caller-provided storage and return a checked view.
    pub fn pack(
        source: &'a [f32],
        rows: usize,
        columns: usize,
        packed: &'a mut [f32],
    ) -> Result<Self, DenseError> {
        let elements = validate_dense_shape(rows, columns)?;
        if source.len() != elements {
            return Err(DenseError::MatrixLengthMismatch);
        }
        if packed.len() != elements {
            return Err(DenseError::PackedLengthMismatch);
        }
        if memory_ranges_overlap(source.as_ptr(), source.len(), packed.as_ptr(), packed.len()) {
            return Err(DenseError::Aliasing);
        }
        for row_block in 0..rows / 8 {
            for column in 0..columns {
                let packed_start = (row_block * columns + column) * 8;
                for lane in 0..8 {
                    packed[packed_start + lane] = source[(row_block * 8 + lane) * columns + column];
                }
            }
        }
        Ok(Self {
            source,
            packed,
            columns,
        })
    }

    pub const fn rows(self) -> usize {
        GATE_ELEMENTS
    }

    pub const fn columns(self) -> usize {
        self.columns
    }

    pub const fn source(self) -> &'a [f32] {
        self.source
    }

    pub const fn packed(self) -> &'a [f32] {
        self.packed
    }

    fn matches(self, rows: usize, columns: usize, matrix: &[f32]) -> bool {
        rows == GATE_ELEMENTS
            && columns == self.columns
            && matrix.len() == self.source.len()
            && core::ptr::eq(matrix.as_ptr(), self.source.as_ptr())
    }
}

/// Allocation-free runtime-dispatched dense adapter for the pinned LSTMs.
///
/// [`Self::detect`] uses the original row-major matrices and an AVX2 gather.
/// [`Self::detect_with_prepacked`] additionally accepts caller-owned packed
/// views and uses contiguous loads whenever a supplied source slice matches.
/// Unsupported CPUs automatically retain [`ScalarDense`] semantics.
#[derive(Debug)]
pub struct DispatchedDense<'a> {
    capabilities: CpuCapabilities,
    prepacked: &'a [PrepackedMatrix<'a>],
    last_path: Option<DensePath>,
}

impl DispatchedDense<'static> {
    pub fn detect() -> Self {
        Self {
            capabilities: detect_cpu_capabilities(),
            prepacked: &[],
            last_path: None,
        }
    }
}

impl<'a> DispatchedDense<'a> {
    pub fn detect_with_prepacked(prepacked: &'a [PrepackedMatrix<'a>]) -> Self {
        Self {
            capabilities: detect_cpu_capabilities(),
            prepacked,
            last_path: None,
        }
    }

    pub const fn capabilities(&self) -> CpuCapabilities {
        self.capabilities
    }

    pub const fn best_lane(&self) -> DenseLane {
        self.capabilities.best_lane()
    }

    pub const fn supports(&self, lane: DenseLane) -> bool {
        self.capabilities.supports(lane)
    }

    pub const fn last_path(&self) -> Option<DensePath> {
        self.last_path
    }

    pub fn accumulate_with_lane(
        &mut self,
        rows: usize,
        columns: usize,
        matrix: &[f32],
        vector: &[f32],
        accumulator: &mut [f32],
        lane: DenseLane,
    ) -> Result<(), DenseError> {
        validate_dense_call(rows, columns, matrix, vector, accumulator)?;
        if !self.supports(lane) {
            return Err(DenseError::UnsupportedLane);
        }
        match lane {
            DenseLane::Scalar => {
                let result = ScalarDense.accumulate(rows, columns, matrix, vector, accumulator);
                match result {
                    Ok(()) => {}
                    Err(never) => match never {},
                }
                self.last_path = Some(DensePath::Scalar);
            }
            DenseLane::Avx2Fma => {
                #[cfg(target_arch = "x86_64")]
                unsafe {
                    if let Some(packed) = self
                        .prepacked
                        .iter()
                        .copied()
                        .find(|packed| packed.matches(rows, columns, matrix))
                    {
                        accumulate_avx2_prepacked(columns, packed.packed, vector, accumulator);
                        self.last_path = Some(DensePath::Avx2Prepacked);
                    } else {
                        accumulate_avx2_gather(rows, columns, matrix, vector, accumulator);
                        self.last_path = Some(DensePath::Avx2Gather);
                    }
                }
                #[cfg(not(target_arch = "x86_64"))]
                return Err(DenseError::UnsupportedLane);
            }
        }
        Ok(())
    }
}

impl DenseKernel for DispatchedDense<'_> {
    type Error = DenseError;

    fn accumulate(
        &mut self,
        rows: usize,
        columns: usize,
        matrix: &[f32],
        vector: &[f32],
        accumulator: &mut [f32],
    ) -> Result<(), Self::Error> {
        self.accumulate_with_lane(rows, columns, matrix, vector, accumulator, self.best_lane())
    }
}

fn validate_dense_shape(rows: usize, columns: usize) -> Result<usize, DenseError> {
    if rows != GATE_ELEMENTS || !matches!(columns, HIDDEN_SIZE | 512 | 640) {
        return Err(DenseError::UnsupportedShape);
    }
    rows.checked_mul(columns).ok_or(DenseError::ShapeOverflow)
}

fn validate_dense_call(
    rows: usize,
    columns: usize,
    matrix: &[f32],
    vector: &[f32],
    accumulator: &[f32],
) -> Result<(), DenseError> {
    let elements = validate_dense_shape(rows, columns)?;
    if matrix.len() != elements {
        return Err(DenseError::MatrixLengthMismatch);
    }
    if vector.len() != columns {
        return Err(DenseError::VectorLengthMismatch);
    }
    if accumulator.len() != rows {
        return Err(DenseError::AccumulatorLengthMismatch);
    }
    Ok(())
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn accumulate_avx2_gather(
    rows: usize,
    columns: usize,
    matrix: &[f32],
    vector: &[f32],
    accumulator: &mut [f32],
) {
    use core::arch::x86_64::{
        _mm256_fmadd_ps, _mm256_i32gather_ps, _mm256_loadu_ps, _mm256_set_epi32, _mm256_set1_ps,
        _mm256_storeu_ps, _mm256_zeroupper,
    };

    let stride = columns as i32;
    let row_offsets = _mm256_set_epi32(
        7 * stride,
        6 * stride,
        5 * stride,
        4 * stride,
        3 * stride,
        2 * stride,
        stride,
        0,
    );
    for row in (0..rows).step_by(8) {
        let mut sum = unsafe { _mm256_loadu_ps(accumulator.as_ptr().add(row)) };
        let matrix_block = unsafe { matrix.as_ptr().add(row * columns) };
        for (column, &input) in vector.iter().enumerate() {
            let weights =
                unsafe { _mm256_i32gather_ps::<4>(matrix_block.add(column), row_offsets) };
            let value = _mm256_set1_ps(input);
            sum = _mm256_fmadd_ps(weights, value, sum);
        }
        unsafe {
            _mm256_storeu_ps(accumulator.as_mut_ptr().add(row), sum);
        }
    }
    _mm256_zeroupper();
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn accumulate_avx2_prepacked(
    columns: usize,
    packed: &[f32],
    vector: &[f32],
    accumulator: &mut [f32],
) {
    use core::arch::x86_64::{
        _mm256_fmadd_ps, _mm256_loadu_ps, _mm256_set1_ps, _mm256_storeu_ps, _mm256_zeroupper,
    };

    for row_block in 0..GATE_ELEMENTS / 8 {
        let row = row_block * 8;
        let mut sum = unsafe { _mm256_loadu_ps(accumulator.as_ptr().add(row)) };
        let packed_block = row_block * columns * 8;
        for (column, &input) in vector.iter().enumerate() {
            let weights =
                unsafe { _mm256_loadu_ps(packed.as_ptr().add(packed_block + column * 8)) };
            let value = _mm256_set1_ps(input);
            sum = _mm256_fmadd_ps(weights, value, sum);
        }
        unsafe {
            _mm256_storeu_ps(accumulator.as_mut_ptr().add(row), sum);
        }
    }
    _mm256_zeroupper();
}

#[cfg(target_arch = "x86_64")]
fn detect_cpu_capabilities() -> CpuCapabilities {
    use core::arch::x86_64::{__cpuid, __cpuid_count};

    const CPUID_1_ECX_FMA: u32 = 1 << 12;
    const CPUID_1_ECX_OSXSAVE: u32 = 1 << 27;
    const CPUID_1_ECX_AVX: u32 = 1 << 28;
    const CPUID_7_EBX_AVX2: u32 = 1 << 5;
    const XCR0_XMM_YMM: u64 = (1 << 1) | (1 << 2);

    let maximum_leaf = __cpuid(0).eax;
    if maximum_leaf < 1 {
        return CpuCapabilities::default();
    }
    let leaf_1 = __cpuid(1);
    let avx_state_contract = CPUID_1_ECX_OSXSAVE | CPUID_1_ECX_AVX;
    if leaf_1.ecx & avx_state_contract != avx_state_contract {
        return CpuCapabilities::default();
    }
    let xcr0 = unsafe { read_xcr0() };
    if xcr0 & XCR0_XMM_YMM != XCR0_XMM_YMM || maximum_leaf < 7 {
        return CpuCapabilities::default();
    }
    let leaf_7 = __cpuid_count(7, 0);
    CpuCapabilities {
        ymm_state: true,
        avx2: leaf_7.ebx & CPUID_7_EBX_AVX2 != 0,
        fma: leaf_1.ecx & CPUID_1_ECX_FMA != 0,
    }
}

#[cfg(not(target_arch = "x86_64"))]
const fn detect_cpu_capabilities() -> CpuCapabilities {
    CpuCapabilities {
        ymm_state: false,
        avx2: false,
        fma: false,
    }
}

#[cfg(target_arch = "x86_64")]
#[inline]
unsafe fn read_xcr0() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        core::arch::asm!(
            "xgetbv",
            in("ecx") 0u32,
            out("eax") low,
            out("edx") high,
            options(nostack, preserves_flags),
        );
    }
    (u64::from(high) << 32) | u64::from(low)
}

fn memory_ranges_overlap(lhs: *const f32, lhs_len: usize, rhs: *const f32, rhs_len: usize) -> bool {
    if lhs_len == 0 || rhs_len == 0 {
        return false;
    }
    let element_bytes = core::mem::size_of::<f32>();
    let lhs_start = lhs as usize;
    let rhs_start = rhs as usize;
    let lhs_end = lhs_start.saturating_add(lhs_len.saturating_mul(element_bytes));
    let rhs_end = rhs_start.saturating_add(rhs_len.saturating_mul(element_bytes));
    lhs_start < rhs_end && rhs_start < lhs_end
}

/// A direction/time-step pair completed by one cooperative advance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletedStep {
    /// ONNX direction index: zero is forward and one is reverse.
    pub direction: usize,
    /// Original input/output sequence index. Reverse traversal therefore
    /// reports decreasing indices.
    pub sequence_index: usize,
    /// Number of direction/time-step pairs complete after this advance.
    pub completed_steps: usize,
    /// Total pairs in this invocation (`2 * sequence_length`).
    pub total_steps: usize,
}

/// Result of a cooperative advance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Advance {
    /// One direction/time-step pair was evaluated.
    Advanced(CompletedStep),
    /// The invocation was already complete; no buffer or kernel was touched.
    Complete,
}

/// A validated, allocation-free LSTM invocation.
///
/// The run owns no tensor memory; it holds borrows of the model and
/// caller-provided workspace. On a dense-kernel error, only gate scratch may
/// have changed. Output, H, C, and the cooperative cursor remain unchanged.
pub struct CooperativeLstm<'model, 'workspace> {
    problem: Problem<'model>,
    buffers: Buffers<'workspace>,
    direction: usize,
    traversal_step: usize,
    completed_steps: usize,
}

impl<'model, 'workspace> CooperativeLstm<'model, 'workspace> {
    /// Start with the pinned graph's all-zero H and C state.
    ///
    /// All buffer lengths are checked before H or C is cleared. A rejected
    /// call leaves every caller-owned buffer unchanged.
    pub fn start_zeroed(
        problem: Problem<'model>,
        buffers: Buffers<'workspace>,
    ) -> Result<Self, ContractError> {
        validate_buffers(&problem, &buffers)?;
        buffers.hidden.fill(0.0);
        buffers.cell.fill(0.0);
        Ok(Self::started(problem, buffers))
    }

    /// Start from checked explicit `[2, 1, 256]` H and C tensors.
    ///
    /// This is useful for semantic testing and future stateful callers. The
    /// production graph uses [`Self::start_zeroed`]. All lengths are validated
    /// before either destination is modified.
    pub fn start_with_state(
        problem: Problem<'model>,
        buffers: Buffers<'workspace>,
        initial_hidden: &[f32],
        initial_cell: &[f32],
    ) -> Result<Self, ContractError> {
        validate_buffers(&problem, &buffers)?;
        if initial_hidden.len() != STATE_ELEMENTS {
            return Err(ContractError::InitialHiddenLengthMismatch);
        }
        if initial_cell.len() != STATE_ELEMENTS {
            return Err(ContractError::InitialCellLengthMismatch);
        }
        buffers.hidden.copy_from_slice(initial_hidden);
        buffers.cell.copy_from_slice(initial_cell);
        Ok(Self::started(problem, buffers))
    }

    fn started(problem: Problem<'model>, buffers: Buffers<'workspace>) -> Self {
        Self {
            problem,
            buffers,
            direction: 0,
            traversal_step: 0,
            completed_steps: 0,
        }
    }

    pub const fn sequence_length(&self) -> usize {
        self.problem.sequence_length
    }

    pub const fn input_width(&self) -> InputWidth {
        self.problem.input_width
    }

    pub const fn completed_steps(&self) -> usize {
        self.completed_steps
    }

    pub const fn total_steps(&self) -> usize {
        DIRECTIONS * self.problem.sequence_length
    }

    pub const fn is_complete(&self) -> bool {
        self.direction == DIRECTIONS
    }

    /// Inspect the next bounded work item without changing state.
    pub fn next_step(&self) -> Option<(usize, usize)> {
        if self.is_complete() {
            None
        } else {
            Some((self.direction, self.sequence_index()))
        }
    }

    pub fn output(&self) -> &[f32] {
        self.buffers.output
    }

    pub fn hidden(&self) -> &[f32] {
        self.buffers.hidden
    }

    pub fn cell(&self) -> &[f32] {
        self.buffers.cell
    }

    /// Return the caller-owned buffers after dropping execution state.
    pub fn into_buffers(self) -> Buffers<'workspace> {
        self.buffers
    }

    /// Evaluate at most one direction/time-step pair.
    ///
    /// Direction zero traverses `0..sequence`; direction one traverses the
    /// sequence in reverse. Both write to the original sequence index in the
    /// ONNX `[sequence, direction, batch, hidden]` output layout.
    pub fn advance<D: DenseKernel>(&mut self, dense: &mut D) -> Result<Advance, D::Error> {
        if self.is_complete() {
            return Ok(Advance::Complete);
        }

        let direction = self.direction;
        let sequence_index = self.sequence_index();
        let channels = self.problem.input_width.channels();

        // Match ONNX Runtime's CPU LSTM accumulation order: X*W, H*R, then
        // one pre-combined Wb+Rb addition. The combined bias is rounded before
        // it is added to the gate accumulator, as in UniDirectionalLstm::LoadBias.
        // Gate scratch is reinitialized on every attempt, so a failed dense
        // call is retry-safe without touching recurrent or output state.
        self.buffers.gates.fill(0.0);

        let input_start = sequence_index * channels;
        let input = &self.problem.input[input_start..input_start + channels];
        let weight_start = direction * GATE_ELEMENTS * channels;
        let weights = &self.problem.weights[weight_start..weight_start + GATE_ELEMENTS * channels];
        dense.accumulate(GATE_ELEMENTS, channels, weights, input, self.buffers.gates)?;

        let state_start = direction * HIDDEN_SIZE;
        let hidden = &self.buffers.hidden[state_start..state_start + HIDDEN_SIZE];
        let recurrent_start = direction * GATE_ELEMENTS * HIDDEN_SIZE;
        let recurrent = &self.problem.recurrent_weights
            [recurrent_start..recurrent_start + GATE_ELEMENTS * HIDDEN_SIZE];
        dense.accumulate(GATE_ELEMENTS, HIDDEN_SIZE, recurrent, hidden, self.buffers.gates)?;

        let bias_start = direction * BIAS_ELEMENTS_PER_DIRECTION;
        let input_bias = &self.problem.bias[bias_start..bias_start + GATE_ELEMENTS];
        let recurrent_bias = &self.problem.bias
            [bias_start + GATE_ELEMENTS..bias_start + BIAS_ELEMENTS_PER_DIRECTION];
        add_fused_bias(self.buffers.gates, input_bias, recurrent_bias);

        // No operation below this point can fail. Commit C, H, and Y together,
        // then advance the cooperative cursor.
        let output_start = (sequence_index * DIRECTIONS + direction) * HIDDEN_SIZE;
        for hidden_index in 0..HIDDEN_SIZE {
            let input_gate = mlas_logistic(self.buffers.gates[INPUT_GATE_OFFSET + hidden_index]);
            let output_gate = mlas_logistic(self.buffers.gates[OUTPUT_GATE_OFFSET + hidden_index]);
            let forget_gate = mlas_logistic(self.buffers.gates[FORGET_GATE_OFFSET + hidden_index]);
            let cell_gate = mlas_tanh(self.buffers.gates[CELL_GATE_OFFSET + hidden_index]);

            let state_index = state_start + hidden_index;
            let next_cell = forget_gate * self.buffers.cell[state_index] + input_gate * cell_gate;
            let next_hidden = output_gate * mlas_tanh(next_cell);
            self.buffers.cell[state_index] = next_cell;
            self.buffers.hidden[state_index] = next_hidden;
            self.buffers.output[output_start + hidden_index] = next_hidden;
        }

        self.completed_steps += 1;
        self.traversal_step += 1;
        if self.traversal_step == self.problem.sequence_length {
            self.direction += 1;
            self.traversal_step = 0;
        }

        Ok(Advance::Advanced(CompletedStep {
            direction,
            sequence_index,
            completed_steps: self.completed_steps,
            total_steps: self.total_steps(),
        }))
    }

    fn sequence_index(&self) -> usize {
        if self.direction == 0 {
            self.traversal_step
        } else {
            self.problem.sequence_length - 1 - self.traversal_step
        }
    }
}

fn validate_buffers(problem: &Problem<'_>, buffers: &Buffers<'_>) -> Result<(), ContractError> {
    if buffers.output.len() != problem.output_elements() {
        return Err(ContractError::OutputLengthMismatch);
    }
    if buffers.hidden.len() != STATE_ELEMENTS {
        return Err(ContractError::HiddenLengthMismatch);
    }
    if buffers.cell.len() != STATE_ELEMENTS {
        return Err(ContractError::CellLengthMismatch);
    }
    if buffers.gates.len() != GATE_SCRATCH_ELEMENTS {
        return Err(ContractError::GateScratchLengthMismatch);
    }
    Ok(())
}

fn add_fused_bias(output: &mut [f32], input_bias: &[f32], recurrent_bias: &[f32]) {
    for ((output, &input_bias), &recurrent_bias) in
        output.iter_mut().zip(input_bias).zip(recurrent_bias)
    {
        *output += input_bias + recurrent_bias;
    }
}

// ONNX Runtime 1.28 commit 45de2a8b06, MLAS LogisticKernelFma3. The constants
// and operation order are intentionally written out instead of generalized:
// changing one Horner operand can change the recurrent result by an ULP.
fn mlas_logistic(value: f32) -> f32 {
    const ALPHA_9: f32 = 4.37031012579801e-11_f32;
    const ALPHA_7: f32 = 1.15627324459942e-07_f32;
    const ALPHA_5: f32 = 6.08574864600143e-05_f32;
    const ALPHA_3: f32 = 8.51377133304701e-03_f32;
    const ALPHA_1: f32 = 2.48287947061529e-01_f32;
    const BETA_10: f32 = 6.10247389755681e-13_f32;
    const BETA_8: f32 = 5.76102136993427e-09_f32;
    const BETA_6: f32 = 6.29106785017040e-06_f32;
    const BETA_4: f32 = 1.70198817374094e-03_f32;
    const BETA_2: f32 = 1.16817656904453e-01_f32;
    const BETA_0: f32 = 9.93151921023180e-01_f32;

    let value = clamp_preserving_nan(value, -18.0, 18.0);
    let squared = value * value;

    let mut numerator = libm::fmaf(squared, ALPHA_9, ALPHA_7);
    numerator = libm::fmaf(numerator, squared, ALPHA_5);
    numerator = libm::fmaf(numerator, squared, ALPHA_3);
    numerator = libm::fmaf(numerator, squared, ALPHA_1);
    numerator *= value;

    let mut denominator = libm::fmaf(squared, BETA_10, BETA_8);
    denominator = libm::fmaf(denominator, squared, BETA_6);
    denominator = libm::fmaf(denominator, squared, BETA_4);
    denominator = libm::fmaf(denominator, squared, BETA_2);
    denominator = libm::fmaf(denominator, squared, BETA_0);

    let result = numerator / denominator + 0.5;
    if result < 0.0 { 0.0 } else { result }
}

// ONNX Runtime 1.28 commit 45de2a8b06, MLAS TanhKernelFma3.
fn mlas_tanh(value: f32) -> f32 {
    const ALPHA_13: f32 = -2.76076847742355e-16_f32;
    const ALPHA_11: f32 = 2.00018790482477e-13_f32;
    const ALPHA_9: f32 = -8.60467152213735e-11_f32;
    const ALPHA_7: f32 = 5.12229709037114e-08_f32;
    const ALPHA_5: f32 = 1.48572235717979e-05_f32;
    const ALPHA_3: f32 = 6.37261928875436e-04_f32;
    const ALPHA_1: f32 = 4.89352455891786e-03_f32;
    const BETA_6: f32 = 1.19825839466702e-06_f32;
    const BETA_4: f32 = 1.18534705686654e-04_f32;
    const BETA_2: f32 = 2.26843463243900e-03_f32;
    const BETA_0: f32 = 4.89352518554385e-03_f32;

    let value = clamp_preserving_nan(value, -9.0, 9.0);
    let squared = value * value;

    let mut numerator = libm::fmaf(squared, ALPHA_13, ALPHA_11);
    numerator = libm::fmaf(numerator, squared, ALPHA_9);
    numerator = libm::fmaf(numerator, squared, ALPHA_7);
    numerator = libm::fmaf(numerator, squared, ALPHA_5);
    numerator = libm::fmaf(numerator, squared, ALPHA_3);
    numerator = libm::fmaf(numerator, squared, ALPHA_1);
    numerator *= value;

    let mut denominator = libm::fmaf(squared, BETA_6, BETA_4);
    denominator = libm::fmaf(denominator, squared, BETA_2);
    denominator = libm::fmaf(denominator, squared, BETA_0);
    numerator / denominator
}

fn clamp_preserving_nan(value: f32, lower: f32, upper: f32) -> f32 {
    let value = if value < lower { lower } else { value };
    if value > upper { upper } else { value }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
