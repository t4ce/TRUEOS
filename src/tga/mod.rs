use core::ptr::{write_volatile, NonNull};
use core::sync::atomic::{AtomicU32, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::{Mutex, Once};

use crate::pci::PciDevice;

const TGA_VENDOR_ID: u16 = 0x22c2; // DEC vendor:
const TGA_DEVICE_ID: u16 = 0x1100; // TGA adapter
const TGA_EXPECTED_BAR0_SIZE: u64 = 1024 * 1024; // 1 MiB

// Minimal "unified" contract (we control both ends):
// - BAR0 is MMIO
// - BAR0 + 0x00 is a 32-bit LED bitfield
//   - bit0..bit5: usr_led0..usr_led5
//   - other bits ignored
// - BAR0 + 0x100.. stores mouse snapshots (up to 10 sources)
//   - entry stride: 0x10 bytes
//   - +0x00: src tag    [31:24]=valid(1), [23:16]=controller, [15:8]=slot, [7:0]=port
//   - +0x04: payload    [31:24]=wheel(i8), [23:16]=dy(i8), [15:8]=dx(i8), [7:0]=buttons
//   - +0x08: seq (monotonic per write, wraps naturally)
//   - +0x0C: reserved (currently 0)
const TGA_LED_SET_OFF: usize = 0x00;
const TGA_MOUSE_BASE_OFF: usize = 0x100;
const TGA_MOUSE_ENTRY_STRIDE: usize = 0x10;
const TGA_MOUSE_MAX_SOURCES: usize = 10;

#[derive(Copy, Clone)]
struct MouseRoute {
    valid: bool,
    controller: u8,
    slot: u8,
    port: u8,
}

const EMPTY_MOUSE_ROUTE: MouseRoute = MouseRoute {
    valid: false,
    controller: 0,
    slot: 0,
    port: 0,
};

struct Tga {
    bus: u8,
    slot: u8,
    function: u8,
    bar_phys: u64,
    bar_size: u64,
    bar_is_64: bool,
    bar_assigned_by_os: bool,
    mmio_base: usize,
    led_reg: usize,
}

#[derive(Copy, Clone)]
struct TgaHotplugSnapshot {
    bus: u8,
    slot: u8,
    function: u8,
    bar_phys: u64,
    bar_size: u64,
    bar_is_64: bool,
    bar_assigned_by_os: bool,
    mmio_base: usize,
}

// Safety: `Tga` contains an MMIO pointer and is always accessed behind the `TGA` mutex.
unsafe impl Send for Tga {}

impl Tga {
    #[inline(always)]
    fn write_led(&self, value: u32) {
        unsafe { write_volatile(self.led_reg as *mut u32, value) };
    }

    #[inline(always)]
    fn write_mmio_u32(&self, offset: usize, value: u32) {
        unsafe { write_volatile((self.mmio_base + offset) as *mut u32, value) };
    }
}

static TGA: Mutex<Option<Tga>> = Mutex::new(None);
static TGA_LAST_MAP: Mutex<Option<(u64, usize)>> = Mutex::new(None);
static TGA_LAST_DISCONNECT: Mutex<Option<TgaHotplugSnapshot>> = Mutex::new(None);
static TGA_MOUSE_ROUTES: Mutex<[MouseRoute; TGA_MOUSE_MAX_SOURCES]> =
    Mutex::new([EMPTY_MOUSE_ROUTE; TGA_MOUSE_MAX_SOURCES]);
static TGA_MOUSE_SEQ: AtomicU32 = AtomicU32::new(0);

static TGA_MISSING_LOG_ONCE: Once<()> = Once::new();
static TGA_TASK_STARTED_LOG_ONCE: Once<()> = Once::new();

// Heartbeat policy: write a visible changing pattern as a "driver alive" indicator.
// We send 0..15 (wrap) so the FPGA can display the low bits.
static TGA_HEARTBEAT_COUNTER: AtomicU32 = AtomicU32::new(0);

const PCI_COMMAND_IO_SPACE: u16 = 1 << 0;
const PCI_COMMAND_MEM_SPACE: u16 = 1 << 1;

fn tga_bar0_size_bytes(bus: u8, slot: u8, function: u8) -> Option<u64> {
    // BAR sizing writes can confuse some devices if decode is enabled.
    // Also, some endpoints incorrectly return a 0 upper mask for 64-bit BAR sizing.
    // We harden both issues locally for TGA bring-up.
    let cmd_before = crate::pci::config_read_u16(bus, slot, function, 0x04);
    if cmd_before == 0xFFFF {
        return None;
    }

    let cmd_disabled = cmd_before & !(PCI_COMMAND_IO_SPACE | PCI_COMMAND_MEM_SPACE);
    if cmd_disabled != cmd_before {
        crate::pci::config_write_u16(bus, slot, function, 0x04, cmd_disabled);
    }

    let off = 0x10u16;
    let orig_lo = crate::pci::config_read_u32(bus, slot, function, off);
    if orig_lo == 0xFFFF_FFFF {
        crate::pci::config_write_u16(bus, slot, function, 0x04, cmd_before);
        return None;
    }
    if (orig_lo & 0x1) != 0 {
        crate::pci::config_write_u16(bus, slot, function, 0x04, cmd_before);
        return None;
    }

    let is_64 = ((orig_lo >> 1) & 0x3) == 0x2;
    let orig_hi = if is_64 {
        crate::pci::config_read_u32(bus, slot, function, off + 4)
    } else {
        0
    };

    crate::pci::config_write_u32(bus, slot, function, off, 0xFFFF_FFF0);
    if is_64 {
        crate::pci::config_write_u32(bus, slot, function, off + 4, 0xFFFF_FFFF);
    }

    let mask_lo = crate::pci::config_read_u32(bus, slot, function, off);
    let mask_hi = if is_64 {
        crate::pci::config_read_u32(bus, slot, function, off + 4)
    } else {
        0
    };

    crate::pci::config_write_u32(bus, slot, function, off, orig_lo);
    if is_64 {
        crate::pci::config_write_u32(bus, slot, function, off + 4, orig_hi);
    }

    crate::pci::config_write_u16(bus, slot, function, 0x04, cmd_before);

    if is_64 {
        let size_mask_lo = mask_lo & !0xFu32;
        if size_mask_lo == 0 {
            return None;
        }

        // If the upper mask comes back 0, compute the size from the low dword only.
        // For small (<4GiB) 64-bit BARs, a conforming device typically returns 0xFFFF_FFFF
        // in the upper mask during sizing. We've observed 0 here from the endpoint.
        if mask_hi == 0 {
            return Some((!size_mask_lo).wrapping_add(1) as u64);
        }

        let size_mask = ((mask_hi as u64) << 32) | (size_mask_lo as u64);
        if size_mask == 0 {
            return None;
        }
        let size = (!size_mask).wrapping_add(1);

        // Extra guard: if the computed size looks like the "0xFFFFFFFF...." pattern,
        // fall back to the low-dword-only calculation.
        if (size >> 32) == 0xFFFF_FFFF {
            return Some((!size_mask_lo).wrapping_add(1) as u64);
        }

        Some(size)
    } else {
        let size_mask = mask_lo & !0xFu32;
        if size_mask == 0 {
            return None;
        }
        Some((!size_mask).wrapping_add(1) as u64)
    }
}

fn write_led_raw(value: u32) {
    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return;
    };
    tga.write_led(value);
}

fn snapshot_from_tga(tga: &Tga) -> TgaHotplugSnapshot {
    TgaHotplugSnapshot {
        bus: tga.bus,
        slot: tga.slot,
        function: tga.function,
        bar_phys: tga.bar_phys,
        bar_size: tga.bar_size,
        bar_is_64: tga.bar_is_64,
        bar_assigned_by_os: tga.bar_assigned_by_os,
        mmio_base: tga.mmio_base,
    }
}

fn log_reconnect_delta(prev: TgaHotplugSnapshot, now: &Tga) {
    let bdf_changed = prev.bus != now.bus || prev.slot != now.slot || prev.function != now.function;
    let bar_phys_changed = prev.bar_phys != now.bar_phys;
    let bar_size_changed = prev.bar_size != now.bar_size;
    let bar_mode_changed = prev.bar_is_64 != now.bar_is_64;
    let assign_changed = prev.bar_assigned_by_os != now.bar_assigned_by_os;
    let map_changed = prev.mmio_base != now.mmio_base;

    if !(bdf_changed
        || bar_phys_changed
        || bar_size_changed
        || bar_mode_changed
        || assign_changed
        || map_changed)
    {
        crate::log!(
            "tga: reconnect stable bdf={:02X}:{:02X}.{} bar0=0x{:016X} size=0x{:X} map=0x{:X}\n",
            now.bus,
            now.slot,
            now.function,
            now.bar_phys,
            now.bar_size,
            now.mmio_base
        );
        return;
    }

    crate::log!(
        "tga: reconnect delta bdf {:02X}:{:02X}.{} -> {:02X}:{:02X}.{} bar0 0x{:016X} -> 0x{:016X} size 0x{:X} -> 0x{:X} mode {} -> {} assign {} -> {} map 0x{:X} -> 0x{:X}\n",
        prev.bus,
        prev.slot,
        prev.function,
        now.bus,
        now.slot,
        now.function,
        prev.bar_phys,
        now.bar_phys,
        prev.bar_size,
        now.bar_size,
        if prev.bar_is_64 { "64b" } else { "32b" },
        if now.bar_is_64 { "64b" } else { "32b" },
        if prev.bar_assigned_by_os { "assigned" } else { "fw" },
        if now.bar_assigned_by_os { "assigned" } else { "fw" },
        prev.mmio_base,
        now.mmio_base
    );
}

fn log_tga_state(prefix: &str, tga: &Tga) {
    crate::log!(
        "tga: {} bdf={:02X}:{:02X}.{} bar0=0x{:016X} size=0x{:X} {} {} map=0x{:X}\n",
        prefix,
        tga.bus,
        tga.slot,
        tga.function,
        tga.bar_phys,
        tga.bar_size,
        if tga.bar_is_64 { "64b" } else { "32b" },
        if tga.bar_assigned_by_os { "assigned" } else { "fw" },
        tga.mmio_base
    );
}

pub fn tga_led_write(value: u32) {
    write_led_raw(value);
}

fn narrow_u8(value: u32) -> u8 {
    if value > u8::MAX as u32 {
        u8::MAX
    } else {
        value as u8
    }
}

fn resolve_mouse_route_index(controller_id: usize, slot_id: u32, port: u8) -> Option<usize> {
    let controller = narrow_u8(controller_id as u32);
    let slot = narrow_u8(slot_id);

    let mut routes = TGA_MOUSE_ROUTES.lock();

    for idx in 0..routes.len() {
        let r = routes[idx];
        if r.valid && r.controller == controller && r.slot == slot && r.port == port {
            return Some(idx);
        }
    }

    for idx in 0..routes.len() {
        if !routes[idx].valid {
            routes[idx] = MouseRoute {
                valid: true,
                controller,
                slot,
                port,
            };
            crate::log!(
                "tga: mouse route add idx={} ctrl={} slot={} port={}\n",
                idx,
                controller,
                slot,
                port
            );
            return Some(idx);
        }
    }

    None
}

pub fn tga_mouse_write(
    controller_id: usize,
    slot_id: u32,
    port: u8,
    buttons: u8,
    dx: i8,
    dy: i8,
    wheel: i8,
) {
    let Some(idx) = resolve_mouse_route_index(controller_id, slot_id, port) else {
        return;
    };

    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return;
    };

    let entry_off = TGA_MOUSE_BASE_OFF + (idx * TGA_MOUSE_ENTRY_STRIDE);
    let controller = narrow_u8(controller_id as u32);
    let slot = narrow_u8(slot_id);

    let tag = (1u32 << 24) | ((controller as u32) << 16) | ((slot as u32) << 8) | (port as u32);
    let payload = ((wheel as u8 as u32) << 24)
        | ((dy as u8 as u32) << 16)
        | ((dx as u8 as u32) << 8)
        | (buttons as u32);
    let seq = TGA_MOUSE_SEQ.fetch_add(1, Ordering::Relaxed);

    tga.write_mmio_u32(entry_off + 0x00, tag);
    tga.write_mmio_u32(entry_off + 0x04, payload);
    tga.write_mmio_u32(entry_off + 0x08, seq);
    tga.write_mmio_u32(entry_off + 0x0C, 0);
}

pub fn tga_led_set(on: bool) {
    tga_led_write(if on { 1 } else { 0 });
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
        TGA_MISSING_LOG_ONCE.call_once(|| {
            crate::log!(
                "tga: device not found (vid=0x{:04X} did=0x{:04X}, scanned {} devices)\n",
                TGA_VENDOR_ID,
                TGA_DEVICE_ID,
                device_count
            );
        });
        return false;
    };

    let Some(tga) = bring_online(&dev) else {
        return false;
    };

    if let Some(prev) = TGA_LAST_DISCONNECT.lock().take() {
        log_reconnect_delta(prev, &tga);
    } else {
        log_tga_state("connected", &tga);
    }

    *TGA.lock() = Some(tga);
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
    if !bar_is_64 {
        crate::log!(
            "tga: unsupported BAR mode bdf={:02X}:{:02X}.{} (expected 64-bit BAR0/1)\n",
            dev.bus,
            dev.slot,
            dev.function
        );
        return None;
    }

    let mut bar_size = tga_bar0_size_bytes(dev.bus, dev.slot, dev.function).unwrap_or(0);
    if bar_size == 0 {
        bar_size = TGA_EXPECTED_BAR0_SIZE;
    } else if bar_size != TGA_EXPECTED_BAR0_SIZE {
        crate::log!(
            "tga: BAR0 size mismatch bdf={:02X}:{:02X}.{} probed=0x{:X} expected=0x{:X} (continuing)\n",
            dev.bus,
            dev.slot,
            dev.function,
            bar_size,
            TGA_EXPECTED_BAR0_SIZE
        );
    }

    let bar_hi_u32 = bar_hi?;

    let mut bar_phys = {
        let lo = (bar_lo as u64) & !0xFu64;
        let hi = bar_hi_u32 as u64;
        lo | (hi << 32)
    };

    let mut bar_assigned_by_os = false;

    if bar_phys == 0 {
        bar_assigned_by_os = true;
        // Hotplug path: firmware may not have assigned BARs for devices appearing later.
        // Allocate a fixed 1 MiB window and program BAR0/BAR1.
        let size = TGA_EXPECTED_BAR0_SIZE;
        let align = TGA_EXPECTED_BAR0_SIZE;

        let base = crate::pci::alloc_hotplug_mmio_base(dev.bus, size, align)?;
        crate::log!(
            "tga: hotplug BAR assign bdf={:02X}:{:02X}.{} size=0x{:X} align=0x{:X} base=0x{:016X}\n",
            dev.bus,
            dev.slot,
            dev.function,
            size,
            align,
            base
        );

        // Preserve the low BAR attribute bits (IO/type/prefetch) reported by the device.
        let new_lo = ((base as u32) & !0xFu32) | (bar_lo & 0xFu32);
        crate::pci::config_write_u32(dev.bus, dev.slot, dev.function, 0x10, new_lo);
        crate::pci::config_write_u32(dev.bus, dev.slot, dev.function, 0x14, (base >> 32) as u32);

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
            let hi = bar_hi? as u64;
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
    let led_reg = base + TGA_LED_SET_OFF;

    crate::log!(
        "tga: bring_online bdf={:02X}:{:02X}.{} cmd 0x{:04X}->0x{:04X} raw_bar0=0x{:08X} raw_bar1=0x{:08X} bar0=0x{:016X} size=0x{:X}\n",
        dev.bus,
        dev.slot,
        dev.function,
        cmd_before,
        cmd_after,
        bar_lo,
        bar_hi.unwrap_or(0),
        bar_phys,
        bar_size
    );

    let tga = Tga {
        bus: dev.bus,
        slot: dev.slot,
        function: dev.function,
        bar_phys,
        bar_size,
        bar_is_64,
        bar_assigned_by_os,
        mmio_base: base,
        led_reg,
    };
    tga.write_led(0);
    Some(tga)
}

#[embassy_executor::task]
pub(crate) async fn tga_task() {
    TGA_TASK_STARTED_LOG_ONCE.call_once(|| {
        crate::log!("tga: task started\n");
    });
    loop {
        if !is_online() {
            crate::pci::enumerate_silent();
            let _ = try_init();
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
                if let Some(old) = guard.take() {
                    *TGA_LAST_DISCONNECT.lock() = Some(snapshot_from_tga(&old));
                    log_tga_state("disconnected", &old);
                }
            }
            Timer::after(EmbassyDuration::from_secs(5)).await;
            continue;
        }

        let t = TGA_HEARTBEAT_COUNTER.fetch_add(1, Ordering::Relaxed);
        // Send 0..15 then wrap.
        write_led_raw(t & 0xF);

        Timer::after(EmbassyDuration::from_millis(500)).await;
    }
}
