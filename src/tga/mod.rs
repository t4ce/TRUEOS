use core::ptr::{write_volatile, NonNull};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::pci::PciDevice;

const TGA_VENDOR_ID: u16 = 0x22c2; // DEC vendor:
const TGA_DEVICE_ID: u16 = 0x1100; // TGA adapter

// Minimal "unified" contract (we control both ends):
// - BAR0 is MMIO
// - BAR0 + 0x00 is a 32-bit LED register
// - write 0 => LED off, write 1 => LED on
const TGA_LED_REG_OFF: usize = 0x00;

struct Tga {
    bus: u8,
    slot: u8,
    function: u8,
    led_reg: usize,
}

// Safety: `Tga` contains an MMIO pointer and is always accessed behind the `TGA` mutex.
unsafe impl Send for Tga {}

impl Tga {
    #[inline(always)]
    fn write_led(&self, value: u32) {
        unsafe { write_volatile(self.led_reg as *mut u32, value) };
    }
}

static TGA: Mutex<Option<Tga>> = Mutex::new(None);
static TGA_LAST_MAP: Mutex<Option<(u64, usize)>> = Mutex::new(None);

fn write_led_raw(value: u32) {
    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return;
    };
    tga.write_led(value);
}

pub fn tga_led_set(on: bool) {
    write_led_raw(if on { 1 } else { 0 });
}

pub fn try_init() -> bool {
    if is_online() {
        return true;
    }

    // Ensure PCI enumeration happened at least once.
    let mut device_count: usize = 0;
    crate::pci::with_devices(|devices| {
        device_count = devices.len();
    });
    if device_count == 0 {
        crate::pci::enumerate_silent();
    }

    let mut found: Option<PciDevice> = None;
    crate::pci::with_devices(|devices| {
        found = devices.iter().copied().find(is_tga);
    });
    let Some(dev) = found else {
        return false;
    };

    let Some(tga) = bring_online(&dev) else {
        return false;
    };

    *TGA.lock() = Some(tga);
    crate::log!("tga: connected\n");
    // Keep contract explicit: default to LED off.
    tga_led_set(false);
    true
}

pub fn init_once() {
    let _ = try_init();
}

fn is_present(tga: &Tga) -> bool {
    crate::pci::config_read_u16(tga.bus, tga.slot, tga.function, 0x00) != 0xFFFF
}

pub fn is_online() -> bool {
    TGA.lock().is_some()
}

fn is_tga(dev: &PciDevice) -> bool {
    dev.vendor == TGA_VENDOR_ID && dev.device == TGA_DEVICE_ID
}

fn bring_online(dev: &PciDevice) -> Option<Tga> {
    // Re-validate the device is still present at this BDF.
    // A return of 0xFFFF typically means config space read failed / no device.
    let vid_live = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x00);
    let did_live = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x02);
    if vid_live == 0xFFFF {
        return None;
    }
    if vid_live != TGA_VENDOR_ID || did_live != TGA_DEVICE_ID {
        return None;
    }

    let cmd_before = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x04);
    crate::pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);
    let cmd_after = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x04);

    if cmd_before == 0xFFFF || cmd_after == 0xFFFF {
        return None;
    }

    let (mut bar_lo, mut bar_hi) = crate::pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
    if bar_lo == 0xFFFF_FFFF {
        return None;
    }
    if (bar_lo & 0x1) != 0 {
        return None;
    }

    let bar_is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
    let _bar_prefetch = ((bar_lo >> 3) & 0x1) != 0;

    let mut bar_phys = {
        let lo = (bar_lo as u64) & !0xFu64;
        let hi = bar_hi.unwrap_or(0) as u64;
        lo | (hi << 32)
    };

    if bar_phys == 0 {
        let size = crate::pci::bar0_size_bytes(dev.bus, dev.slot, dev.function).unwrap_or(0);
        let base = crate::pci::bar_alloc::alloc_tga_bar0_base(size)?;

        // Preserve the low BAR attribute bits (IO/type/prefetch) if they were present.
        let new_lo = (base & !0xFu32) | (bar_lo & 0xFu32);
        crate::pci::config_write_u32(dev.bus, dev.slot, dev.function, 0x10, new_lo);
        if bar_is_64 {
            crate::pci::config_write_u32(dev.bus, dev.slot, dev.function, 0x14, 0);
        }

        // Re-read and re-validate.
        (bar_lo, bar_hi) = crate::pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
        if bar_lo == 0xFFFF_FFFF {
            return None;
        }
        if (bar_lo & 0x1) != 0 {
            return None;
        }

        bar_phys = {
            let lo = (bar_lo as u64) & !0xFu64;
            let hi = bar_hi.unwrap_or(0) as u64;
            lo | (hi << 32)
        };

        crate::pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);
        if bar_phys == 0 {
            return None;
        }
    }

    // We only need BAR0+0, so mapping 1 page keeps it minimal.
    let mapped = {
        let last = *TGA_LAST_MAP.lock();
        if let Some((last_phys, last_base)) = last {
            if last_phys == bar_phys {
                NonNull::new(last_base as *mut u8)?
            } else {
                let m = crate::pci::mmio::map_mmio_region_exact(bar_phys, 0x1000).ok()?;
                *TGA_LAST_MAP.lock() = Some((bar_phys, m.as_ptr() as usize));
                m
            }
        } else {
            let m = crate::pci::mmio::map_mmio_region_exact(bar_phys, 0x1000).ok()?;
            *TGA_LAST_MAP.lock() = Some((bar_phys, m.as_ptr() as usize));
            m
        }
    };

    let base = mapped.as_ptr() as usize;
    let led_reg = base + TGA_LED_REG_OFF;

    let tga = Tga {
        bus: dev.bus,
        slot: dev.slot,
        function: dev.function,
        led_reg,
    };
    tga.write_led(0);
    Some(tga)
}

#[embassy_executor::task]
pub(crate) async fn tga_task() {
    let mut ctr: u32 = 0;
    loop {
        if !is_online() {
            crate::pci::enumerate_silent();
            let _ = try_init();
            ctr = 0;
            Timer::after(EmbassyDuration::from_secs(5)).await;
            continue;
        }

        // If device disappeared, go offline and stop writes.
        let present = {
            let guard = TGA.lock();
            guard.as_ref().map(is_present).unwrap_or(false)
        };
        if !present {
            {
                let mut guard = TGA.lock();
                if guard.is_some() {
                    *guard = None;
                    crate::log!("tga: disconnected\n");
                }
            }
            Timer::after(EmbassyDuration::from_secs(5)).await;
            continue;
        }

        tga_led_set((ctr & 1) != 0);
        ctr = ctr.wrapping_add(1);
        Timer::after(EmbassyDuration::from_millis(500)).await;
    }
}
