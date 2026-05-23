#![no_std]

extern crate alloc;

pub mod binding;
pub mod control;
pub mod machine;
pub mod memory;
pub mod plan;
pub mod ring;
pub mod status;
pub mod validation;

pub use binding::{SymbolBindingArtifact, SymbolBindingRecord};
pub use control::{SequenceArtifact, SequenceResult};
pub use machine::{MachineOpArtifact, MachineOpKind, MachineOpResult};
pub use memory::{Arena, ArenaResult, BufferArtifact, BufferBinding, Span, SpanEnd, SpanResult};
pub use plan::{
    ArenaBinding, ConstBinding, FsReadStep, LogWriteStep, ParseError, PlacementError,
    PlacementProgram, PlacementStep, Plan, parse_placement_program, parse_plan,
};
pub use ring::{
    RING_ALIGN, RING_HEADER_LEN, RingArtifact, RingArtifactResult, RingBinding, RingLayout,
    RingSnapshot, RingState,
};
pub use status::SilkStatus;
pub use validation::{ValidationArtifact, ValidationKind, ValidationResult};
