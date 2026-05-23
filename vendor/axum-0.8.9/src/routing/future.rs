//! Future types.

pub use super::{
    into_make_service::IntoMakeServiceFuture,
    route::{InfallibleRouteFuture, RouteFuture},
};
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::prelude::rust_2024::*;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use alloc::borrow::ToOwned;
