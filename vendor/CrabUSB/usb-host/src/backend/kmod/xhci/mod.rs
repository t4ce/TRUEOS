pub(crate) mod cmd;
mod context;
mod def;
pub(crate) mod device;
mod endpoint;
mod event;
pub(crate) mod host;
pub(crate) mod hub;
mod reg;
mod ring;
mod root_hub_profile;
mod sync;
mod transfer;

pub(crate) use def::*;

pub use device::Device;
pub use host::Xhci;

use core::sync::atomic::{AtomicUsize, Ordering};
use usb_if::host::hub::Speed;

/// Set during deep controller bring-up when every healthy hot-path event is needed.
const XHCI_HOT_PATH_TRACE_ALL: bool = false;
const XHCI_TRACE_STARTUP_BURST: usize = 8;
const XHCI_TRACE_SAMPLE_EVERY: usize = 128;

/// Samples repetitive healthy xHCI traces while retaining a visible event count.
pub(crate) struct TraceSampler {
    seen: AtomicUsize,
}

impl TraceSampler {
    pub(crate) const fn new() -> Self {
        Self {
            seen: AtomicUsize::new(0),
        }
    }

    pub(crate) fn sample(&self) -> Option<usize> {
        let seen = self.seen.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        (XHCI_HOT_PATH_TRACE_ALL
            || seen <= XHCI_TRACE_STARTUP_BURST
            || seen.is_multiple_of(XHCI_TRACE_SAMPLE_EVERY))
        .then_some(seen)
    }

    pub(crate) fn total(&self) -> usize {
        self.seen.load(Ordering::Relaxed)
    }
}

fn parse_default_max_packet_size_from_port_speed(speed: Speed) -> u16 {
    // 根据 xHCI 规范表 6-30 和 U-Boot 实现：
    // 参考 U-Boot drivers/usb/host/xhci-mem.c:730-751
    match speed {
        Speed::Full => 64,             // Full Speed → 64 bytes
        Speed::Low => 8,               // Low Speed → 8 bytes
        Speed::High => 64,             // High Speed → 64 bytes
        Speed::SuperSpeed => 512,      // SuperSpeed → 512 bytes
        Speed::SuperSpeedPlus => 1024, // SuperSpeedPlus → 1024 bytes
        Speed::Wireless => unimplemented!("Wireless"),
    }
}
