#![no_std]
#![deny(unsafe_code)]

//! Allocation-free control-plane execution for sealed Kokoro AOT programs.
//!
//! This crate deliberately implements no tensor math. A [`Dispatcher`]
//! executes each cooperative work slice and explicitly reports the decoder
//! frame count produced by the model's resolver operation. [`Executor`] owns
//! the transactional cursor, validates the phase transition, resolves the
//! phase-one arena, and never commits failed work.

use trueos_kokoro_aot::{
    ArenaPlanError, CursorError, CursorPoll, DType, OpCode, OpCursor, Phase, Program, StorageKind,
    UNRESOLVED_SLOT_BASE, WorkBudget, WorkSlice,
};

/// Exact logical dimensions of one tensor during an invocation.
///
/// The sealed descriptor carries maximum-capacity dimensions. Kokoro also has
/// runtime `N` (token count), `F` (decoder frame count), and operator-derived
/// dimensions. Keeping those logical dimensions separately lets phase-zero
/// `N`, `N x N`, mixed `N/F`, and dynamic contiguous views share fixed maximum
/// storage without weakening the AOT capacity proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeShape {
    rank: u8,
    dims: [u32; 4],
}

impl RuntimeShape {
    pub const fn scalar() -> Self {
        Self {
            rank: 0,
            dims: [1; 4],
        }
    }

    pub fn new(dims: &[u32]) -> Result<Self, ShapeError> {
        if dims.len() > 4 {
            return Err(ShapeError::RankTooLarge);
        }
        let mut stored = [1; 4];
        stored[..dims.len()].copy_from_slice(dims);
        Ok(Self {
            rank: dims.len() as u8,
            dims: stored,
        })
    }

    pub const fn rank(self) -> u8 {
        self.rank
    }

    pub fn dims(&self) -> &[u32] {
        &self.dims[..usize::from(self.rank)]
    }

    pub fn element_count(self) -> Result<u64, ShapeError> {
        let mut elements = 1_u64;
        for &dimension in self.dims() {
            elements = elements
                .checked_mul(u64::from(dimension))
                .ok_or(ShapeError::ByteLengthOverflow)?;
        }
        Ok(elements)
    }

    pub fn logical_bytes(self, dtype: DType) -> Result<u64, ShapeError> {
        self.element_count()?
            .checked_mul(dtype.element_bytes())
            .ok_or(ShapeError::ByteLengthOverflow)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShapeError {
    RankTooLarge,
    TableCapacityTooSmall,
    ForeignProgram,
    TensorOutOfBounds,
    TensorUninitialized,
    NotExternal,
    RankMismatch,
    DimensionExceedsCapacity,
    ByteLengthOverflow,
    ByteCapacityExceeded,
    ReadOnlyOutput,
    WrongPhase,
    OutputCountMismatch,
    DuplicateOutput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuntimeTensorState {
    shape: RuntimeShape,
    initialized: bool,
}

impl RuntimeTensorState {
    const EMPTY: Self = Self {
        shape: RuntimeShape::scalar(),
        initialized: false,
    };
}

/// Caller-owned, allocation-free logical-shape state for a sealed program.
///
/// Constants are initialized from their exact descriptors. External tensors
/// are explicitly bound by the backend. A concrete dispatcher declares an
/// operation's output shapes after validating all of them, so a rejected
/// declaration cannot partially mutate the table.
#[derive(Debug)]
pub struct TensorShapeTable<const CAPACITY: usize> {
    states: [RuntimeTensorState; CAPACITY],
    tensor_count: u32,
    bound_artifact_sha256: Option<[u8; 32]>,
}

impl<const CAPACITY: usize> TensorShapeTable<CAPACITY> {
    pub const fn new() -> Self {
        Self {
            states: [RuntimeTensorState::EMPTY; CAPACITY],
            tensor_count: 0,
            bound_artifact_sha256: None,
        }
    }

    /// Bind the table to one artifact and initialize its constant tensors.
    pub fn initialize(&mut self, program: &Program<'_>) -> Result<(), ShapeError> {
        let tensor_count = program.tensor_count();
        if tensor_count as usize > CAPACITY {
            return Err(ShapeError::TableCapacityTooSmall);
        }
        self.states.fill(RuntimeTensorState::EMPTY);
        for tensor_id in 0..tensor_count {
            let descriptor = program
                .tensor(tensor_id)
                .ok_or(ShapeError::TensorOutOfBounds)?;
            if descriptor.storage == StorageKind::Constant {
                let shape = descriptor_max_shape(descriptor.rank, &descriptor.max_dims)?;
                validate_shape(descriptor, shape)?;
                self.states[tensor_id as usize] = RuntimeTensorState {
                    shape,
                    initialized: true,
                };
            }
        }
        self.tensor_count = tensor_count;
        self.bound_artifact_sha256 = Some(*program.artifact_sha256());
        Ok(())
    }

    pub const fn tensor_count(&self) -> u32 {
        self.tensor_count
    }

    pub fn initialized_count(&self) -> usize {
        self.states[..self.tensor_count as usize]
            .iter()
            .filter(|state| state.initialized)
            .count()
    }

    /// Bind one exact logical shape for an external input or output buffer.
    pub fn bind_external(
        &mut self,
        program: &Program<'_>,
        tensor_id: u32,
        shape: RuntimeShape,
    ) -> Result<(), ShapeError> {
        self.check_program(program)?;
        let descriptor = self.descriptor(program, tensor_id)?;
        if descriptor.storage != StorageKind::External {
            return Err(ShapeError::NotExternal);
        }
        validate_shape(descriptor, shape)?;
        self.states[tensor_id as usize] = RuntimeTensorState {
            shape,
            initialized: true,
        };
        Ok(())
    }

    pub fn shape(&self, program: &Program<'_>, tensor_id: u32) -> Result<RuntimeShape, ShapeError> {
        self.check_program(program)?;
        let state = *self
            .states
            .get(tensor_id as usize)
            .filter(|_| tensor_id < self.tensor_count)
            .ok_or(ShapeError::TensorOutOfBounds)?;
        if state.initialized {
            Ok(state.shape)
        } else {
            Err(ShapeError::TensorUninitialized)
        }
    }

    /// Verify that every input binding of an operation has a logical shape.
    pub fn validate_inputs(&self, program: &Program<'_>, op_index: u32) -> Result<(), ShapeError> {
        self.check_program(program)?;
        let op = program.op(op_index).ok_or(ShapeError::TensorOutOfBounds)?;
        for input in 0..op.input_count {
            let tensor_id = program
                .op_input(op, input)
                .ok_or(ShapeError::TensorOutOfBounds)?;
            let descriptor = self.descriptor(program, tensor_id)?;
            validate_phase(descriptor.phase, op.phase)?;
            self.shape(program, tensor_id)?;
        }
        Ok(())
    }

    /// Atomically declare the exact shapes of one sealed operation's outputs.
    ///
    /// `shapes` is in binding order and must match the sealed output count.
    /// may include external graph outputs and contiguous views, but never an
    /// arbitrary tensor ID or a read-only constant.
    pub fn declare_op_outputs(
        &mut self,
        program: &Program<'_>,
        op_index: u32,
        shapes: &[RuntimeShape],
    ) -> Result<(), ShapeError> {
        self.check_program(program)?;
        let op = program.op(op_index).ok_or(ShapeError::TensorOutOfBounds)?;
        if shapes.len() != usize::from(op.output_count) {
            return Err(ShapeError::OutputCountMismatch);
        }
        for (index, &shape) in shapes.iter().enumerate() {
            let tensor_id = program
                .op_output(op, index as u16)
                .ok_or(ShapeError::TensorOutOfBounds)?;
            for previous in 0..index {
                if program.op_output(op, previous as u16) == Some(tensor_id) {
                    return Err(ShapeError::DuplicateOutput);
                }
            }
            let descriptor = self.descriptor(program, tensor_id)?;
            if descriptor.is_read_only() {
                return Err(ShapeError::ReadOnlyOutput);
            }
            validate_phase(descriptor.phase, op.phase)?;
            validate_shape(descriptor, shape)?;
        }
        for (index, &shape) in shapes.iter().enumerate() {
            let tensor_id = program
                .op_output(op, index as u16)
                .ok_or(ShapeError::TensorOutOfBounds)?;
            self.states[tensor_id as usize] = RuntimeTensorState {
                shape,
                initialized: true,
            };
        }
        Ok(())
    }

    /// Invalidate all phase-local state after a phase is no longer live.
    pub fn clear_phase(&mut self, program: &Program<'_>, phase: Phase) -> Result<(), ShapeError> {
        self.check_program(program)?;
        for tensor_id in 0..self.tensor_count {
            let descriptor = self.descriptor(program, tensor_id)?;
            if descriptor.phase == phase && descriptor.storage != StorageKind::Constant {
                self.states[tensor_id as usize] = RuntimeTensorState::EMPTY;
            }
        }
        Ok(())
    }

    fn descriptor(
        &self,
        program: &Program<'_>,
        tensor_id: u32,
    ) -> Result<trueos_kokoro_aot::TensorDesc, ShapeError> {
        if tensor_id >= self.tensor_count {
            return Err(ShapeError::TensorOutOfBounds);
        }
        program
            .tensor(tensor_id)
            .ok_or(ShapeError::TensorOutOfBounds)
    }

    fn check_program(&self, program: &Program<'_>) -> Result<(), ShapeError> {
        if self.bound_artifact_sha256 == Some(*program.artifact_sha256()) {
            Ok(())
        } else {
            Err(ShapeError::ForeignProgram)
        }
    }
}

impl<const CAPACITY: usize> Default for TensorShapeTable<CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

fn descriptor_max_shape(rank: u8, max_dims: &[u32; 4]) -> Result<RuntimeShape, ShapeError> {
    if rank > 4 {
        return Err(ShapeError::RankTooLarge);
    }
    RuntimeShape::new(&max_dims[..usize::from(rank)])
}

fn validate_shape(
    descriptor: trueos_kokoro_aot::TensorDesc,
    shape: RuntimeShape,
) -> Result<(), ShapeError> {
    if shape.rank != descriptor.rank {
        return Err(ShapeError::RankMismatch);
    }
    for axis in 0..usize::from(shape.rank) {
        if shape.dims[axis] > descriptor.max_dims[axis] {
            return Err(ShapeError::DimensionExceedsCapacity);
        }
    }
    if shape.logical_bytes(descriptor.dtype)? > descriptor.byte_capacity {
        return Err(ShapeError::ByteCapacityExceeded);
    }
    Ok(())
}

fn validate_phase(tensor_phase: Phase, operation_phase: Phase) -> Result<(), ShapeError> {
    if tensor_phase == Phase::Shared || tensor_phase == operation_phase {
        Ok(())
    } else {
        Err(ShapeError::WrongPhase)
    }
}

/// Successful result of dispatching one cooperative work slice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchResult {
    /// The requested units completed without producing control-plane data.
    Completed,
    /// The requested units completed and reached the invocation's live work
    /// boundary, so the remaining sealed maximum-capacity suffix is empty.
    CompletedOperation { runtime_work_units: u32 },
    /// The final resolver slice completed and produced phase one's frame count.
    FrameCount(u32),
}

const fn supports_runtime_completion(opcode: OpCode) -> bool {
    matches!(opcode, OpCode::Resize | OpCode::FloatConv1d | OpCode::FloatConvTranspose1d)
}

/// Backend boundary for CPU, GPU, or mixed operation implementations.
///
/// The executor passes the sealed program so a dispatcher can resolve bindings
/// and attributes without duplicating control-plane state. Returning an error
/// leaves the [`WorkSlice`] uncommitted and therefore retryable.
pub trait Dispatcher {
    type Error;

    fn dispatch(
        &mut self,
        program: &Program<'_>,
        work: WorkSlice,
    ) -> Result<DispatchResult, Self::Error>;
}

/// Non-backend failures that make the current execution terminal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutorFault {
    /// A different sealed artifact was supplied after execution began.
    ForeignProgram,
    /// Phase zero ended without an explicit resolver result.
    MissingFrameCount,
    /// A dispatcher reported a frame count from the wrong or a partial op.
    UnexpectedFrameCount,
    /// A dispatcher tried to shorten an operation without a runtime-prefix
    /// work contract.
    UnexpectedOperationCompletion,
    /// More than one frame-count result was reported.
    DuplicateFrameCount,
    /// Runtime arena resolution rejected the frame count or capacity plan.
    Arena(ArenaPlanError),
    /// The underlying transactional cursor rejected an internal transition.
    Cursor(CursorError),
}

/// Durable state of an executor instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutorState {
    Phase0,
    Phase1,
    Complete,
    Cancelled,
    Faulted(ExecutorFault),
}

impl ExecutorState {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Complete | Self::Cancelled | Self::Faulted(_))
    }
}

/// Validated runtime facts retained after phase-one admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedPhase {
    frame_count: u32,
    arena_bytes: u64,
    slot_count: u32,
}

impl ResolvedPhase {
    pub const fn frame_count(self) -> u32 {
        self.frame_count
    }

    pub const fn arena_bytes(self) -> u64 {
        self.arena_bytes
    }

    pub const fn slot_count(self) -> u32 {
        self.slot_count
    }
}

/// Why a call to [`Executor::run_slice`] returned to its scheduler.
#[derive(Debug, Eq, PartialEq)]
pub enum SliceEvent<E> {
    BudgetExhausted,
    PhaseAdmitted(ResolvedPhase),
    Complete,
    DispatchFailed(E),
    Cancelled,
    Faulted(ExecutorFault),
}

/// One scheduler-visible cooperative execution report.
#[derive(Debug, Eq, PartialEq)]
pub struct SliceReport<E> {
    pub event: SliceEvent<E>,
    /// Work units claimed during this invocation, including a failed attempt.
    pub consumed: u32,
    pub remaining: u32,
}

/// Allocation-free executor with capacity for `SLOT_CAPACITY` arena slots.
///
/// Slot bases live inline so the runtime can use this type before an allocator
/// exists. An undersized capacity fails closed during phase-one resolution.
#[derive(Debug)]
pub struct Executor<const SLOT_CAPACITY: usize> {
    cursor: OpCursor,
    state: ExecutorState,
    bound_artifact_sha256: Option<[u8; 32]>,
    pending_frame_count: Option<u32>,
    resolved_phase: Option<ResolvedPhase>,
    slot_bases: [u64; SLOT_CAPACITY],
}

impl<const SLOT_CAPACITY: usize> Executor<SLOT_CAPACITY> {
    pub const fn new() -> Self {
        Self {
            cursor: OpCursor::new(),
            state: ExecutorState::Phase0,
            bound_artifact_sha256: None,
            pending_frame_count: None,
            resolved_phase: None,
            slot_bases: [UNRESOLVED_SLOT_BASE; SLOT_CAPACITY],
        }
    }

    pub const fn state(&self) -> ExecutorState {
        self.state
    }

    pub const fn cursor(&self) -> OpCursor {
        self.cursor
    }

    pub const fn resolved_phase(&self) -> Option<ResolvedPhase> {
        self.resolved_phase
    }

    pub fn slot_bases(&self) -> &[u64] {
        let count = self
            .resolved_phase
            .map_or(0, |phase| phase.slot_count as usize);
        &self.slot_bases[..count]
    }

    pub fn slot_base(&self, slot: u32) -> Option<u64> {
        self.slot_bases()
            .get(slot as usize)
            .copied()
            .filter(|base| *base != UNRESOLVED_SLOT_BASE)
    }

    pub const fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }

    /// Enter the cancelled terminal state if work is still running.
    pub fn cancel(&mut self) -> bool {
        if matches!(self.state, ExecutorState::Phase0 | ExecutorState::Phase1) {
            self.state = ExecutorState::Cancelled;
            true
        } else {
            false
        }
    }

    /// Clear terminal state, program binding, arena facts, and cursor progress.
    pub fn reset(&mut self) {
        self.cursor.reset();
        self.state = ExecutorState::Phase0;
        self.bound_artifact_sha256 = None;
        self.pending_frame_count = None;
        self.resolved_phase = None;
        self.slot_bases.fill(UNRESOLVED_SLOT_BASE);
    }

    /// Execute until the budget is exhausted, dispatch fails, a phase is
    /// admitted, or a terminal state is reached.
    pub fn run_slice<D: Dispatcher>(
        &mut self,
        program: &Program<'_>,
        dispatcher: &mut D,
        budget: &mut WorkBudget,
    ) -> SliceReport<D::Error> {
        let initial_remaining = budget.remaining();

        match self.state {
            ExecutorState::Complete => {
                return report(SliceEvent::Complete, initial_remaining, budget);
            }
            ExecutorState::Cancelled => {
                return report(SliceEvent::Cancelled, initial_remaining, budget);
            }
            ExecutorState::Faulted(fault) => {
                return report(SliceEvent::Faulted(fault), initial_remaining, budget);
            }
            ExecutorState::Phase0 | ExecutorState::Phase1 => {}
        }

        let artifact_sha256 = *program.artifact_sha256();
        match self.bound_artifact_sha256 {
            Some(bound) if bound != artifact_sha256 => {
                return self.fail(ExecutorFault::ForeignProgram, initial_remaining, budget);
            }
            Some(_) => {}
            None => self.bound_artifact_sha256 = Some(artifact_sha256),
        }

        loop {
            match self.cursor.poll(program, budget) {
                CursorPoll::BudgetExhausted => {
                    return report(SliceEvent::BudgetExhausted, initial_remaining, budget);
                }
                CursorPoll::Complete => {
                    self.state = ExecutorState::Complete;
                    return report(SliceEvent::Complete, initial_remaining, budget);
                }
                CursorPoll::PhaseBoundary(_) => {
                    let Some(frame_count) = self.pending_frame_count else {
                        return self.fail(
                            ExecutorFault::MissingFrameCount,
                            initial_remaining,
                            budget,
                        );
                    };
                    let plan = match program.resolve_phase_two(frame_count, &mut self.slot_bases) {
                        Ok(plan) => plan,
                        Err(error) => {
                            return self.fail(
                                ExecutorFault::Arena(error),
                                initial_remaining,
                                budget,
                            );
                        }
                    };
                    let resolved = ResolvedPhase {
                        frame_count: plan.frame_count(),
                        arena_bytes: plan.arena_bytes(),
                        slot_count: program.slot_count(),
                    };
                    if let Err(error) = self.cursor.admit_phase_two(program, &plan) {
                        return self.fail(ExecutorFault::Cursor(error), initial_remaining, budget);
                    }
                    self.pending_frame_count = None;
                    self.resolved_phase = Some(resolved);
                    self.state = ExecutorState::Phase1;
                    return report(SliceEvent::PhaseAdmitted(resolved), initial_remaining, budget);
                }
                CursorPoll::Ready(work) => {
                    let dispatch_result = match dispatcher.dispatch(program, work) {
                        Ok(result) => result,
                        Err(error) => {
                            return report(
                                SliceEvent::DispatchFailed(error),
                                initial_remaining,
                                budget,
                            );
                        }
                    };
                    let (frame_count, runtime_completion) = match dispatch_result {
                        DispatchResult::Completed => (None, None),
                        DispatchResult::CompletedOperation { runtime_work_units } => {
                            if !supports_runtime_completion(work.op().opcode) {
                                return self.fail(
                                    ExecutorFault::UnexpectedOperationCompletion,
                                    initial_remaining,
                                    budget,
                                );
                            }
                            (None, Some(runtime_work_units))
                        }
                        DispatchResult::FrameCount(frame_count) => {
                            if self.state != ExecutorState::Phase0
                                || work.op().opcode != OpCode::ResolveDecoderShape
                                || !work.completes_op()
                            {
                                return self.fail(
                                    ExecutorFault::UnexpectedFrameCount,
                                    initial_remaining,
                                    budget,
                                );
                            }
                            if self.pending_frame_count.is_some() {
                                return self.fail(
                                    ExecutorFault::DuplicateFrameCount,
                                    initial_remaining,
                                    budget,
                                );
                            }
                            (Some(frame_count), None)
                        }
                    };
                    let commit = match runtime_completion {
                        Some(runtime_work_units) => {
                            self.cursor
                                .commit_runtime_complete(program, work, runtime_work_units)
                        }
                        None => self.cursor.commit(program, work),
                    };
                    if let Err(error) = commit {
                        return self.fail(ExecutorFault::Cursor(error), initial_remaining, budget);
                    }
                    if let Some(frame_count) = frame_count {
                        self.pending_frame_count = Some(frame_count);
                    }
                }
            }
        }
    }

    fn fail<E>(
        &mut self,
        fault: ExecutorFault,
        initial_remaining: u32,
        budget: &WorkBudget,
    ) -> SliceReport<E> {
        self.state = ExecutorState::Faulted(fault);
        report(SliceEvent::Faulted(fault), initial_remaining, budget)
    }
}

impl<const SLOT_CAPACITY: usize> Default for Executor<SLOT_CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

fn report<E>(event: SliceEvent<E>, initial_remaining: u32, budget: &WorkBudget) -> SliceReport<E> {
    SliceReport {
        event,
        consumed: initial_remaining - budget.remaining(),
        remaining: budget.remaining(),
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
