use core::ptr::{read_volatile, write_volatile};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Once;

use crate::pci::PciDevice;

const TGA_VENDOR_ID: u16 = 0x22c2; // DEC vendor:
const TGA_DEVICE_ID: u16 = 0x1100; // TGA adapter

// Minimal "unified" contract (we control both ends):
// - BAR0 is MMIO
// - BAR0 + 0x00 is a 32-bit LED register
// - write 0 => LED off, write 1 => LED on
const TGA_LED_REG_OFF: usize = 0x00;
const TGA_LED_OFF: u32 = 0;
const TGA_LED_ON: u32 = 1;

struct Tga {
    led_reg: usize,
}

impl Tga {
    #[inline(always)]
    fn write_led(&self, value: u32) {
        unsafe { write_volatile(self.led_reg as *mut u32, value) };
    }
}

static TGA: Once<Option<Tga>> = Once::new();

pub fn init_once() {
    TGA.call_once(|| {
        let mut found: Option<PciDevice> = None;
        crate::pci::with_devices(|devices| {
            found = devices.iter().copied().find(is_tga);
        });
        let Some(dev) = found else {
            return None;
        };
        bring_online(&dev)
    });
}

pub fn is_online() -> bool {
    TGA.get().and_then(|x| x.as_ref()).is_some()
}

fn is_tga(dev: &PciDevice) -> bool {
    dev.vendor == TGA_VENDOR_ID && dev.device == TGA_DEVICE_ID
}

fn bring_online(dev: &PciDevice) -> Option<Tga> {
    crate::pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);

    let (bar_lo, bar_hi) = crate::pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
    if (bar_lo & 0x1) != 0 {
        crate::debugconf!("tga: BAR0 is IO; unsupported\n");
        return None;
    }

    let bar_phys = {
        let lo = (bar_lo as u64) & !0xFu64;
        let hi = bar_hi.unwrap_or(0) as u64;
        lo | (hi << 32)
    };

    // We only need BAR0+0, so mapping 1 page keeps it minimal.
    let mapped = crate::pci::mmio::map_mmio_region_exact(bar_phys, 0x1000).ok()?;
    let base = mapped.as_ptr() as usize;
    let led_reg = base + TGA_LED_REG_OFF;

    crate::debugconf!("tga online: bar0=0x{:X}\n", bar_phys);

    let tga = Tga { led_reg };
    tga.write_led(TGA_LED_OFF);
    Some(tga)
}

#[inline]
fn with_tga<R>(f: impl FnOnce(&Tga) -> R) -> Option<R> {
    let tga = TGA.get().and_then(|x| x.as_ref())?;
    Some(f(tga))
}

pub fn tga_led_on() {
    let _ = with_tga(|tga| {
        tga.write_led(TGA_LED_ON);
        //unsafe { let _ = read_volatile(tga.led_reg as *const u32); }
    });
}

pub fn tga_led_off() {
    let _ = with_tga(|tga| {
        tga.write_led(TGA_LED_OFF);
        //unsafe { let _ = read_volatile(tga.led_reg as *const u32); }
    });
}

#[embassy_executor::task]
pub(crate) async fn blink_task() {
    loop {
        tga_led_on();
        Timer::after(EmbassyDuration::from_millis(500)).await;
        tga_led_off();
        Timer::after(EmbassyDuration::from_millis(500)).await;
        crate::debugconf!("tga heartbeat on/off once.");
    }
}
