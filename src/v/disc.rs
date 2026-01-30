//! Virtual disk (block-device) layer.
//!
//! This module is the home for block-device virtualization/stacking:
//! partitions, ramdisks, crypto, caching, etc.
//!
//! For now it wires in existing disk partition support.

pub mod partition {
    pub use crate::disc::partition::*;
}
