#![no_std]
#![allow(async_fn_in_trait)]
#![allow(unsafe_op_in_unsafe_fn)]
#![doc = include_str!("../README.md")]
#![allow(clippy::new_without_default)]
#![warn(missing_docs)]
#![deny(missing_debug_implementations)]

// This mod MUST go first, so that the others see its macros.
pub(crate) mod fmt;

mod delay;
mod duration;
mod instant;
mod timer;

pub use delay::{Delay, block_for};
pub use duration::Duration;
pub use embassy_time_driver::TICK_HZ;
pub use instant::Instant;
pub use timer::{Ticker, TimeoutError, Timer, WithTimeout, with_deadline, with_timeout};

const fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

pub(crate) const GCD_1K: u64 = gcd(TICK_HZ, 1_000);
pub(crate) const GCD_1M: u64 = gcd(TICK_HZ, 1_000_000);
pub(crate) const GCD_1G: u64 = gcd(TICK_HZ, 1_000_000_000);
