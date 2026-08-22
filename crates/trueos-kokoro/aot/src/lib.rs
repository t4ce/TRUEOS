#![no_std]
#![deny(unsafe_code)]

//! Sealed, model-specific execution substrate for native Kokoro inference.
//!
//! This crate deliberately contains no neural-network math, filesystem I/O, or
//! scheduler integration. It validates a fixed little-endian program artifact,
//! describes bounded rank-four tensors and two activation-arena phases, and
//! provides a transactional cooperative-operation cursor. A host-side compiler
//! is responsible for translating and fusing the pinned Kokoro graph.

mod cursor;
mod format;
mod program;
mod tensor;

pub use cursor::{BudgetError, CursorError, CursorPoll, OpCursor, WorkBudget, WorkSlice};
pub use format::*;
pub use program::{
    ArenaPlanError, OpDesc, OpError, ParseError, ParseOptions, PhasePlan, Program,
    ResolvedArenaPlan, ResolvedStorage, SectionDesc, SlotDesc, SlotKind, StorageOwner,
};
pub use tensor::{
    DType, LayoutRequirement, Materialization, Phase, ResolvedTensorDesc, StorageKind, TensorDesc,
    TensorError, TensorFlags,
};

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
