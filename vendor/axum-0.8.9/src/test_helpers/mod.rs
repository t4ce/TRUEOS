#![allow(clippy::disallowed_names)]

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::prelude::rust_2021::*;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use alloc::borrow::ToOwned;
use crate::{extract::Request, response::Response, serve};

mod test_client;
pub use self::test_client::*;




#[allow(dead_code)]
pub(crate) struct NotSendSync(*const ());
