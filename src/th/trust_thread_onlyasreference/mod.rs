//! trust-thread extraction target.
//!
//! This is the cleaned, reusable shape we want to grow out of the raw TrustOS
//! import in `../trustos/`.  The raw import stays untouched for provenance;
//! this module is where TRUEOS-specific dependencies get pushed behind small
//! hooks.
//!
//! Source lineage:
//! - Derived from TrustOS thread/scheduler ideas by nathan237.
//! - Original source imported under Apache-2.0 in `src/th/trustos`.
//! - This extraction should remain small, no_std, and kernel-portable.

pub mod arch;
pub mod platform;
pub mod queue;
pub mod types;

pub use platform::ThreadPlatform;
pub use queue::RunQueue;
pub use types::{
    CarrierDuration, CarrierId, CarrierPurpose, ThreadFlags, ThreadId, ThreadSpec, ThreadState,
    THREAD_ID_INVALID,
};
