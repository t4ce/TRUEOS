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
    // According to xHCI port-speed defaults, Full-speed devices bootstrap EP0
    // at 64 here and setup_max_packet() retunes later from bMaxPacketSize0.
    match speed {
        Speed::Full => 64,             // Full Speed → 64 bytes
        Speed::Low => 8,               // Low Speed → 8 bytes
        Speed::High => 64,             // High Speed → 64 bytes
        Speed::SuperSpeed => 512,      // SuperSpeed → 512 bytes
        Speed::SuperSpeedPlus => 1024, // SuperSpeedPlus → 1024 bytes
        Speed::Wireless => unimplemented!("Wireless"),
    }
}
