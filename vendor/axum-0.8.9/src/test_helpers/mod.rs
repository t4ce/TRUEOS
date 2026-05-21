#![allow(clippy::disallowed_names)]

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::prelude::rust_2021::*;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use alloc::borrow::ToOwned;
use crate::{extract::Request, response::Response, serve};

mod test_client;
pub use self::test_client::*;

#[cfg(test)]
pub(crate) mod tracing_helpers;

#[cfg(test)]
pub(crate) mod counting_cloneable_state;

#[cfg(test)]
pub(crate) fn assert_send<T: Send>() {}
#[cfg(test)]
pub(crate) fn assert_sync<T: Sync>() {}

#[allow(dead_code)]
pub(crate) struct NotSendSync(*const ());
