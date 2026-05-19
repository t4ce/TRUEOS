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
mod sync;
mod transfer;

pub(crate) use def::*;
pub use device::Device;
pub use host::Xhci;
use usb_if::host::hub::Speed;

fn parse_default_max_packet_size_from_port_speed(speed: Speed) -> u16 {
    // Bootstrap EP0 conservatively for LS/FS, then retune from bMaxPacketSize0
    // after reading the first 8 bytes of the device descriptor.
    match speed {
        Speed::Full => 8,              // Full Speed → start at 8, then evaluate-context if needed
        Speed::Low => 8,               // Low Speed → 8 bytes
        Speed::High => 64,             // High Speed → 64 bytes
        Speed::SuperSpeed => 512,      // SuperSpeed → 512 bytes
        Speed::SuperSpeedPlus => 1024, // SuperSpeedPlus → 1024 bytes
        Speed::Wireless => unimplemented!("Wireless"),
    }
}
