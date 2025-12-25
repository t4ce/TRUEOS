use spin::Once;

use crate::pci::PciDevice;

const TGA_VENDOR_ID: u16 = 0x1011; // DEC vendor
const TGA_DEVICE_ID: u16 = 0x0004; // TGA adapter

static INIT: Once<()> = Once::new();

pub fn init_once() {
    INIT.call_once(|| {
        crate::pci::with_devices(|devices| {
            if let Some(dev) = devices.iter().find(|dev| is_tga(dev)) {
                bring_online(dev);
            }
        });
    });
}

fn is_tga(dev: &PciDevice) -> bool {
    dev.vendor == TGA_VENDOR_ID && dev.device == TGA_DEVICE_ID
}

fn bring_online(dev: &PciDevice) {
    let _ = dev;
    crate::debugconf!("tga online!\n");
}
