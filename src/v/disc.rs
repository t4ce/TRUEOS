//! Virtual disk (block-device) layer.
//!
//! This module is the home for block-device virtualization/stacking:
//! partitions, ramdisks, crypto, caching, etc.
//!
//! This is where block-device virtualization/stacking lives.

pub mod detect;
pub mod partition;
pub mod ramdisk;
