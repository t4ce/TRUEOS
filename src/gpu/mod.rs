//! TRUEOS mediated GPU core.
//!
//! `physical` is the kernel/driver boundary. `vgpu` owns principals, opaque
//! handles, quotas, per-client GPUVMs, queues, and virtual timelines.

pub(crate) mod physical;
pub(crate) mod vgpu;

pub(crate) use physical::register_physical_device;
