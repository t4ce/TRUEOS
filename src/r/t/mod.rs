//! TRUEOS Tokio integration.
//!
//! This module owns the boundary between Tokio-facing crates and TRUEOS runtime
//! services: time, blocking workers, filesystem shims, and VNet/Mio/socket2.

pub mod net;
