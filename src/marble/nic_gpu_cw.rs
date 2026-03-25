//! Sidepath universe for NIC receive and GPU submit worlds.
//!
//! The calculator CCW stays independent. This module models two separate
//! collapsed runnable worlds in the same universe:
//! - NIC RX world: raw frame ingress -> meaningful UDP payload egress
//! - GPU submit world: meaningful payload ingress -> GPU submission egress

pub(crate) use super::{Marble, MarbleGadget};

#[path = "nic_gpu_cw/gpu_submit_cw.rs"]
mod gpu_submit_cw;
#[path = "nic_gpu_cw/nic_rx_cw.rs"]
mod nic_rx_cw;
#[path = "nic_gpu_cw/shared.rs"]
mod shared;

pub use gpu_submit_cw::*;
pub use nic_rx_cw::*;
pub use shared::*;
