#![no_std]
#![deny(unsafe_code)]

//! Typed, fail-closed dispatch boundary for the sealed Kokoro AOT stream.
//!
//! Attribute decoding is kept separate from tensor execution: a record that
//! is not part of the model-specific v1 contract is rejected before any
//! kernel can observe tensor memory.

pub mod attributes;
mod cpu;

pub use attributes::{
    ATTRIBUTE_ABI_VERSION, AttributeError, Attributes, ContractReason, decode, record_bytes,
};
pub use cpu::{
    CpuDispatchPlan, CpuDispatcher, CpuWorkspace, CpuWorkspaceRequirements, DispatchError,
    KOKORO_CPU_WORKSPACE_REQUIREMENTS, WorkspaceError, native_dispatch_requires_workspace,
    native_dispatch_supported,
};

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
