use core::ptr;

use crate::{OpDesc, PhasePlan, Program, ResolvedArenaPlan};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetError {
    ZeroLimit,
}

/// Per-`run_slice` work-unit allowance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkBudget {
    limit: u32,
    remaining: u32,
}

impl WorkBudget {
    pub const fn new(limit: u32) -> Result<Self, BudgetError> {
        if limit == 0 {
            Err(BudgetError::ZeroLimit)
        } else {
            Ok(Self {
                limit,
                remaining: limit,
            })
        }
    }

    pub const fn limit(self) -> u32 {
        self.limit
    }

    pub const fn remaining(self) -> u32 {
        self.remaining
    }

    pub const fn spent(self) -> u32 {
        self.limit - self.remaining
    }

    fn claim_up_to(&mut self, requested: u32) -> u32 {
        let claimed = requested.min(self.remaining);
        self.remaining -= claimed;
        claimed
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkSlice {
    op_index: u32,
    op: OpDesc,
    unit_start: u32,
    unit_count: u32,
    artifact_sha256: [u8; 32],
}

impl WorkSlice {
    pub const fn op_index(self) -> u32 {
        self.op_index
    }

    pub const fn op(self) -> OpDesc {
        self.op
    }

    pub const fn unit_start(self) -> u32 {
        self.unit_start
    }

    pub const fn unit_count(self) -> u32 {
        self.unit_count
    }

    pub const fn unit_end(self) -> u32 {
        self.unit_start + self.unit_count
    }

    pub const fn completes_op(self) -> bool {
        self.unit_end() == self.op.work_units
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorPoll {
    Ready(WorkSlice),
    BudgetExhausted,
    PhaseBoundary(PhasePlan),
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorError {
    InvalidCheckpoint,
    ProgramMismatch,
    NotAtPhaseBoundary,
    PhaseTwoNotResolved,
    StaleWorkSlice,
    InvalidWorkSlice,
    InvalidRuntimeCompletion,
}

/// Transactional cursor over a sealed program.
///
/// [`Self::poll`] reserves budget but does not advance the cursor. The caller
/// executes the returned unit range and invokes [`Self::commit`] only after
/// successful completion, so a failed kernel cannot silently skip work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpCursor {
    op_index: u32,
    unit_offset: u32,
    phase_two_admitted: bool,
}

impl OpCursor {
    pub const fn new() -> Self {
        Self {
            op_index: 0,
            unit_offset: 0,
            phase_two_admitted: false,
        }
    }

    pub fn from_checkpoint(
        program: &Program<'_>,
        op_index: u32,
        unit_offset: u32,
        phase_two_admitted: bool,
    ) -> Result<Self, CursorError> {
        if op_index > program.op_count() {
            return Err(CursorError::InvalidCheckpoint);
        }
        if op_index == program.op_count() {
            if unit_offset != 0 || !phase_two_admitted {
                return Err(CursorError::InvalidCheckpoint);
            }
        } else {
            let op = program.op(op_index).ok_or(CursorError::InvalidCheckpoint)?;
            if unit_offset >= op.work_units {
                return Err(CursorError::InvalidCheckpoint);
            }
            let phase_two_start = program.phases()[1].op_start;
            if op_index < phase_two_start && phase_two_admitted {
                return Err(CursorError::InvalidCheckpoint);
            }
            if (op_index > phase_two_start || (op_index == phase_two_start && unit_offset != 0))
                && !phase_two_admitted
            {
                return Err(CursorError::InvalidCheckpoint);
            }
        }
        Ok(Self {
            op_index,
            unit_offset,
            phase_two_admitted,
        })
    }

    pub const fn op_index(self) -> u32 {
        self.op_index
    }

    pub const fn unit_offset(self) -> u32 {
        self.unit_offset
    }

    pub const fn phase_two_admitted(self) -> bool {
        self.phase_two_admitted
    }

    pub fn poll(&self, program: &Program<'_>, budget: &mut WorkBudget) -> CursorPoll {
        if self.op_index == program.op_count() {
            return CursorPoll::Complete;
        }
        let phase_two = program.phases()[1];
        if self.op_index == phase_two.op_start && !self.phase_two_admitted {
            return CursorPoll::PhaseBoundary(phase_two);
        }
        if budget.remaining == 0 {
            return CursorPoll::BudgetExhausted;
        }
        let Some(op) = program.op(self.op_index) else {
            // A Program cannot reach this state after parsing. Treat it as a
            // completed stream rather than manufacturing executable work.
            return CursorPoll::Complete;
        };
        let remaining_units = op.work_units - self.unit_offset;
        let unit_count = budget.claim_up_to(remaining_units);
        CursorPoll::Ready(WorkSlice {
            op_index: self.op_index,
            op,
            unit_start: self.unit_offset,
            unit_count,
            artifact_sha256: *program.artifact_sha256(),
        })
    }

    /// Commit work only against the sealed program that produced the slice.
    pub fn commit(&mut self, program: &Program<'_>, work: WorkSlice) -> Result<(), CursorError> {
        self.validate_commit(program, work)?;
        self.unit_offset = work.unit_end();
        if self.unit_offset == work.op.work_units {
            self.advance_operation();
        }
        Ok(())
    }

    /// Commit the executed slice and a dispatcher-proven empty runtime suffix.
    ///
    /// `runtime_work_units` is the live prefix length for this invocation. It
    /// must fit both the sealed operation and the slice just executed. The
    /// cursor never derives this boundary itself; only a dispatcher that has
    /// validated the runtime output shape can attest it.
    pub fn commit_runtime_complete(
        &mut self,
        program: &Program<'_>,
        work: WorkSlice,
        runtime_work_units: u32,
    ) -> Result<(), CursorError> {
        self.validate_commit(program, work)?;
        if runtime_work_units > work.op.work_units || runtime_work_units > work.unit_end() {
            return Err(CursorError::InvalidRuntimeCompletion);
        }
        self.advance_operation();
        Ok(())
    }

    fn validate_commit(&self, program: &Program<'_>, work: WorkSlice) -> Result<(), CursorError> {
        if work.op_index != self.op_index || work.unit_start != self.unit_offset {
            return Err(CursorError::StaleWorkSlice);
        }
        if program.artifact_sha256() != &work.artifact_sha256
            || program.op(work.op_index) != Some(work.op)
        {
            return Err(CursorError::ProgramMismatch);
        }
        if work.unit_count == 0
            || work.unit_end() > work.op.work_units
            || work.unit_end() < work.unit_start
        {
            return Err(CursorError::InvalidWorkSlice);
        }
        Ok(())
    }

    fn advance_operation(&mut self) {
        self.op_index = self.op_index.saturating_add(1);
        self.unit_offset = 0;
    }

    pub fn admit_phase_two(
        &mut self,
        program: &Program<'_>,
        resolved: &ResolvedArenaPlan<'_, '_, '_>,
    ) -> Result<(), CursorError> {
        if self.op_index != program.phases()[1].op_start || self.unit_offset != 0 {
            return Err(CursorError::NotAtPhaseBoundary);
        }
        if self.phase_two_admitted {
            return Err(CursorError::NotAtPhaseBoundary);
        }
        if !ptr::eq(program, resolved.program()) {
            return Err(CursorError::ProgramMismatch);
        }
        if resolved.arena_bytes() < program.phases()[1].arena_min_bytes
            || resolved.arena_bytes() > program.phases()[1].arena_max_bytes
        {
            return Err(CursorError::PhaseTwoNotResolved);
        }
        self.phase_two_admitted = true;
        Ok(())
    }

    pub const fn reset(&mut self) {
        *self = Self::new();
    }
}

impl Default for OpCursor {
    fn default() -> Self {
        Self::new()
    }
}
