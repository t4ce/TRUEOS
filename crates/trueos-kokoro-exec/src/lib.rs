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
    ArenaPlanError, CursorError, CursorPoll, OpCode, OpCursor, Program, UNRESOLVED_SLOT_BASE,
    WorkBudget, WorkSlice,
};

/// Successful result of dispatching one cooperative work slice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchResult {
    /// The requested units completed without producing control-plane data.
    Completed,
    /// The final resolver slice completed and produced phase one's frame count.
    FrameCount(u32),
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
                    let frame_count = match dispatch_result {
                        DispatchResult::Completed => None,
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
                            Some(frame_count)
                        }
                    };
                    if let Err(error) = self.cursor.commit(program, work) {
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
