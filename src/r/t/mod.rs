//! TRUEOS Tokio integration.
//!
//! This module owns the boundary between Tokio-facing crates and TRUEOS runtime
//! services: time, blocking workers, filesystem shims, and VNet/Mio/socket2.

pub mod net;

use core::future::Future;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunError {
    Build,
}

pub fn block_on_io<F>(future: F) -> Result<F::Output, RunError>
where
    F: Future,
{
    let mut builder = tokio::runtime::Builder::new_current_thread();
    builder.enable_io();
    builder.enable_time();
    let runtime = builder.build().map_err(|_| RunError::Build)?;
    Ok(runtime.block_on(future))
}
