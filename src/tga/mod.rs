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
const TGA_LED_OFF: u32 = 0;
const TGA_LED_ON: u32 = 1;

const TGA_LIVENESS_PROBE_PERIOD_TOGGLES: u32 = 1; // probe each blink cycle

struct Tga {
    bus: u8,
    slot: u8,
    function: u8,
    bar_phys: u64,
    mmio_base: NonNull<u8>,
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
static TGA_LIVENESS_TOGGLES: Mutex<u32> = Mutex::new(0);

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

    // Re-validate the device is still present at this BDF.
    // A return of 0xFFFF typically means config space read failed / no device.
    let vid_live = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x00);
    let did_live = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x02);
    if vid_live == 0xFFFF {
        crate::log!(
            "tga: config read failed at {:02X}:{:02X}.{} (vid=0xFFFF)\n",
            dev.bus,
            dev.slot,
            dev.function
        );
        return None;
    }
    if vid_live != TGA_VENDOR_ID || did_live != TGA_DEVICE_ID {
        crate::log!(
            "tga: device changed at {:02X}:{:02X}.{} live vid={:04X} did={:04X}\n",
            dev.bus,
            dev.slot,
            dev.function,
            vid_live,
            did_live
        );
        return None;
    }

    let cmd_before = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x04);
    crate::pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);
    let cmd_after = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x04);
    crate::log!(
        "tga: pci cmd before=0x{:04X} after=0x{:04X}\n",
        cmd_before,
        cmd_after
    );

    if cmd_before == 0xFFFF || cmd_after == 0xFFFF {
        crate::log!(
            "tga: pci cmd readback failed at {:02X}:{:02X}.{}\n",
            dev.bus,
            dev.slot,
            dev.function
        );
        return None;
    }

    let (mut bar_lo, mut bar_hi) = crate::pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
    if bar_lo == 0xFFFF_FFFF {
        crate::log!("tga: BAR0 read returned all-ones; device not responding\n");
        return None;
    }
    if (bar_lo & 0x1) != 0 {
        crate::log!("tga: BAR0 is IO; unsupported\n");
        return None;
    }

    let mut bar_is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
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

    let mut bar_phys = {
        let lo = (bar_lo as u64) & !0xFu64;
        let hi = bar_hi.unwrap_or(0) as u64;
        lo | (hi << 32)
    };

    if bar_phys == 0 {
        let size = crate::pci::bar0_size_bytes(dev.bus, dev.slot, dev.function).unwrap_or(0);
        let base = crate::pci::bar_alloc::alloc_tga_bar0_base(size)?;
        crate::log!(
            "tga: BAR0 unassigned (phys=0); sizing says {} bytes; allocating base=0x{:08X}\n",
            size,
            base
        );

        // Preserve the low BAR attribute bits (IO/type/prefetch) if they were present.
        let new_lo = (base & !0xFu32) | (bar_lo & 0xFu32);
        crate::pci::config_write_u32(dev.bus, dev.slot, dev.function, 0x10, new_lo);
        if bar_is_64 {
            crate::pci::config_write_u32(dev.bus, dev.slot, dev.function, 0x14, 0);
        }

        // Re-read and re-validate.
        (bar_lo, bar_hi) = crate::pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
        if bar_lo == 0xFFFF_FFFF {
            crate::log!("tga: BAR0 reread returned all-ones after programming; abort\n");
            return None;
        }
        if (bar_lo & 0x1) != 0 {
            crate::log!("tga: BAR0 became IO after programming; abort\n");
            return None;
        }

        bar_is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
        bar_phys = {
            let lo = (bar_lo as u64) & !0xFu64;
            let hi = bar_hi.unwrap_or(0) as u64;
            lo | (hi << 32)
        };

        crate::pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);
        crate::log!("tga: BAR0 programmed; new bar0_phys=0x{:X}\n", bar_phys);
        if bar_phys == 0 {
            crate::log!("tga: BAR0 still unassigned after programming; abort\n");
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

    crate::log!(
        "tga online: bar0_phys=0x{:X} (page_off=0x{:X}) mmio_virt=0x{:X} led_reg=0x{:X}\n",
        bar_phys,
        (bar_phys as usize) & 0xFFF,
        base,
        led_reg
    );

    let tga = Tga {
        bus: dev.bus,
        slot: dev.slot,
        function: dev.function,
        bar_phys,
        mmio_base: mapped,
        led_reg,
    };
    tga.write_led(TGA_LED_OFF);
    Some(tga)
}

fn is_present(tga: &Tga) -> bool {
    crate::pci::config_read_u16(tga.bus, tga.slot, tga.function, 0x00) != 0xFFFF
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

            // Liveness probe: device can vanish due to power; if it does, go offline.
            {
                let mut cnt = TGA_LIVENESS_TOGGLES.lock();
                *cnt = cnt.wrapping_add(1);
                if (*cnt % TGA_LIVENESS_PROBE_PERIOD_TOGGLES) == 0 {
                    let ok = with_tga(|tga| is_present(tga)).unwrap_or(false);
                    if !ok {
                        crate::log!("tga: device disappeared; going offline\n");
                        *TGA.lock() = None;
                        *cnt = 0;
                        offline_ticks = 0;
                        Timer::after(EmbassyDuration::from_millis(500)).await;
                        continue;
                    }
                }
            }

            tga_led_on();
            Timer::after(EmbassyDuration::from_millis(500)).await;
            tga_led_off();
            Timer::after(EmbassyDuration::from_millis(500)).await;
        }
    })
    .await;
}
