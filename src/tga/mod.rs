use core::ptr::write_volatile;

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

static TGA: Mutex<Option<Tga>> = Mutex::new(None);

pub fn try_init() -> bool {
    if is_online() {
        return true;
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
    true
}

pub fn init_once() {
    let _ = try_init();
}

pub fn is_online() -> bool {
    TGA.lock().is_some()
}

fn is_tga(dev: &PciDevice) -> bool {
    dev.vendor == TGA_VENDOR_ID && dev.device == TGA_DEVICE_ID
}

fn bring_online(dev: &PciDevice) -> Option<Tga> {
    crate::log!(
        "tga: claiming dev {:02X}:{:02X}.{} vid={:04X} did={:04X}\n",
        dev.bus,
        dev.slot,
        dev.function,
        dev.vendor,
        dev.device
    );

    let cmd_before = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x04);
    crate::pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);
    let cmd_after = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x04);
    crate::log!(
        "tga: pci cmd before=0x{:04X} after=0x{:04X}\n",
        cmd_before,
        cmd_after
    );

    let (bar_lo, bar_hi) = crate::pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
    if (bar_lo & 0x1) != 0 {
        crate::log!("tga: BAR0 is IO; unsupported\n");
        return None;
    }

    let bar_is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
    let bar_prefetch = ((bar_lo >> 3) & 0x1) != 0;
    if let Some(hi) = bar_hi {
        crate::log!(
            "tga: bar0 raw lo=0x{:08X} hi=0x{:08X} (is64={} prefetch={})\n",
            bar_lo,
            hi,
            bar_is_64,
            bar_prefetch
        );
    } else {
        crate::log!(
            "tga: bar0 raw lo=0x{:08X} hi=<none> (is64={} prefetch={})\n",
            bar_lo,
            bar_is_64,
            bar_prefetch
        );
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

    crate::log!(
        "tga online: bar0_phys=0x{:X} (page_off=0x{:X}) mmio_virt=0x{:X} led_reg=0x{:X}\n",
        bar_phys,
        (bar_phys as usize) & 0xFFF,
        base,
        led_reg
    );

    let tga = Tga { led_reg };
    tga.write_led(TGA_LED_OFF);
    Some(tga)
}

#[inline]
fn with_tga<R>(f: impl FnOnce(&Tga) -> R) -> Option<R> {
    let guard = TGA.lock();
    let tga = guard.as_ref()?;
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
    crate::v::taskmon::run("tga-blink", async move {
        let mut offline_ticks: u32 = 0;
        loop {
            if !is_online() {
                // Retry strategy: periodically rescan PCI and attempt bring-up.
                // This supports the (rare) case where PCI devices appear after boot.
                offline_ticks = offline_ticks.wrapping_add(1);
                if (offline_ticks % 10) == 0 {
                    crate::pci::enumerate_silent();
                    let _ = try_init();
                }
                Timer::after(EmbassyDuration::from_millis(500)).await;
                continue;
            }

            tga_led_on();
            Timer::after(EmbassyDuration::from_millis(500)).await;
            tga_led_off();
            Timer::after(EmbassyDuration::from_millis(500)).await;
            crate::log!("tga heartbeat on/off once.\n");
        }
    })
    .await;
}
