use core::ptr::{NonNull, read_volatile, write_bytes, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering, fence};
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;

use crate::pci::PciDevice;

const TGA_VENDOR_ID: u16 = 0x22c2; // DEC vendor:
const TGA_DEVICE_ID: u16 = 0x1100; // TGA adapter
const TGA_EXPECTED_BAR0_SIZE: u64 = 1024; // 1 KiB

// Minimal "unified" contract (we control both ends):
// - BAR0 is MMIO
// - BAR0 + 0x00 is a 32-bit LED bitfield
//   - bit0..bit5: usr_led0..usr_led5
//   - other bits ignored
// - BAR0 + 0x20 is reserved for a future 32-bit read-only protocol magic ("TGAT")
// - BAR0 + 0x28 starts the tiny ADD unit after ARG0/ARG1 are written
//   - RESULT becomes ARG0 + ARG1, STATUS.DONE is set, RETIRE_COUNT increments
const TGA_LED_SET_OFF: usize = 0x00;
const TGA_MAGIC_OFF: usize = 0x20;
const TGA_ADD_STATUS_OFF: usize = 0x24;
const TGA_ADD_CMD_OFF: usize = 0x28;
const TGA_ADD_ARG0_OFF: usize = 0x2C;
const TGA_ADD_ARG1_OFF: usize = 0x30;
const TGA_ADD_RESULT_OFF: usize = 0x34;
const TGA_ADD_RETIRE_COUNT_OFF: usize = 0x38;
const TGA_ADD_ERROR_OFF: usize = 0x3C;
const TGA_HOST_MB_ADDR_LO_OFF: usize = 0x40;
const TGA_HOST_MB_ADDR_HI_OFF: usize = 0x44;
const TGA_HOST_MB_BYTES_OFF: usize = 0x48;
const TGA_HOST_MB_DOORBELL_OFF: usize = 0x4C;

const TGA_CMD_ADD_U32: u32 = 1;
const TGA_STATUS_BUSY: u32 = 1 << 0;
const TGA_STATUS_DONE: u32 = 1 << 1;
const TGA_ADD_POLL_LIMIT: usize = 10_000;
const TGA_HOST_MB_BYTES: usize = 4096;
const TGA_HOST_MB_ALIGN: usize = 4096;
const TGA_HOST_MB_DOORBELL_MAGIC: u32 = 0x484D_4231; // "HMB1"
const TGA_HOST_MB_MAGIC_OFF: usize = 0x00;
const TGA_HOST_MB_SEQ_OFF: usize = 0x04;
const TGA_HOST_MB_VALUE_OFF: usize = 0x08;
const TGA_HOST_MB_STATUS_OFF: usize = 0x0C;
const TGA_BOOT_MMIO_TOUCH_ENABLED: bool = false;
const TGA_HEARTBEAT_MMIO_ENABLED: bool = true;
const TGA_READBACK_DOORBELL_ENABLED: bool = true;
const TGA_HOST_MAILBOX_ENABLED: bool = true;
const TGA_ADD_PROOF_ON_CONNECT_ENABLED: bool = false;
const TGA_READBACK_DOORBELL_AFTER_WRITES: u32 = 20;
const TGA_READBACK_DOORBELL_LED_VALUE: u32 = 0x1F;
const TGA_MAGIC_EXPECTED: u32 = 0x5453_4154;

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
    magic_reg: usize,
    add_status_reg: usize,
    add_cmd_reg: usize,
    add_arg0_reg: usize,
    add_arg1_reg: usize,
    add_result_reg: usize,
    add_retire_count_reg: usize,
    add_error_reg: usize,
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

#[derive(Copy, Clone)]
struct TgaHostMailbox {
    phys: u64,
    virt: usize,
    len: usize,
}

// Safety: `Tga` contains an MMIO pointer and is always accessed behind the `TGA` mutex.
unsafe impl Send for Tga {}

#[derive(Copy, Clone)]
pub struct TgaAddProof {
    pub a: u32,
    pub b: u32,
    pub expected: u32,
    pub result: u32,
    pub status: u32,
    pub retire_count: u32,
    pub error: u32,
    pub polls: usize,
    pub done: bool,
    pub ok: bool,
}

impl Tga {
    #[inline(always)]
    fn write_led(&self, value: u32) {
        unsafe { write_volatile(self.led_reg as *mut u32, value) };
    }

    #[inline(always)]
    fn read_reg(reg: usize) -> u32 {
        unsafe { read_volatile(reg as *const u32) }
    }

    #[inline(always)]
    fn write_reg(reg: usize, value: u32) {
        unsafe { write_volatile(reg as *mut u32, value) };
    }

    fn add_u32(&self, a: u32, b: u32) -> TgaAddProof {
        let expected = a.wrapping_add(b);

        Self::write_reg(self.add_status_reg, 0);
        Self::write_reg(self.add_arg0_reg, a);
        Self::write_reg(self.add_arg1_reg, b);
        Self::write_reg(self.add_cmd_reg, TGA_CMD_ADD_U32);

        let mut status = 0;
        let mut polls = 0usize;
        while polls < TGA_ADD_POLL_LIMIT {
            status = Self::read_reg(self.add_status_reg);
            if (status & TGA_STATUS_DONE) != 0 && (status & TGA_STATUS_BUSY) == 0 {
                break;
            }
            polls += 1;
        }

        let result = Self::read_reg(self.add_result_reg);
        let retire_count = Self::read_reg(self.add_retire_count_reg);
        let error = Self::read_reg(self.add_error_reg);
        let done = (status & TGA_STATUS_DONE) != 0 && (status & TGA_STATUS_BUSY) == 0;
        let ok = done && result == expected && error == 0;

        TgaAddProof {
            a,
            b,
            expected,
            result,
            status,
            retire_count,
            error,
            polls,
            done,
            ok,
        }
    }
}

static TGA: Mutex<Option<Tga>> = Mutex::new(None);
static TGA_LAST_MAP: Mutex<Option<(u64, usize)>> = Mutex::new(None);
static TGA_LAST_DISCONNECT: Mutex<Option<TgaHotplugSnapshot>> = Mutex::new(None);
static TGA_HOST_MAILBOX: Mutex<Option<TgaHostMailbox>> = Mutex::new(None);

// Heartbeat policy: write a visible changing pattern as a "driver alive" indicator.
// We send 0..31 (wrap) so the FPGA can display the low 5 bits.
static TGA_HEARTBEAT_COUNTER: AtomicU32 = AtomicU32::new(0);
static TGA_READBACK_DOORBELL_DONE: AtomicBool = AtomicBool::new(false);
static TGA_HOST_MAILBOX_LAST_SEQ: AtomicU32 = AtomicU32::new(0);
static TGA_HOST_MAILBOX_LAST_MAGIC: AtomicU32 = AtomicU32::new(0);

const TGA_HEARTBEAT_PERIOD_MS: u64 = 100;
const TGA_HEARTBEAT_LOG_EVERY_WRITES: u32 = 50;
const TGA_PRESENCE_PROBE_PERIOD_MS: u64 = 1000;
const TGA_OFFLINE_RETRY_MS: u64 = 250;
const TGA_PRESENCE_MISS_THRESHOLD: u8 = 10;

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

fn write_heartbeat_led(value: u32, count: u32) {
    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return;
    };
    tga.write_led(value);
    if count % TGA_HEARTBEAT_LOG_EVERY_WRITES == 0 {
        let bus = tga.bus;
        let slot = tga.slot;
        let function = tga.function;
        let bar_phys = tga.bar_phys;
        let led_reg = tga.led_reg;
        drop(guard);
        crate::log!(
            "tga: heartbeat mmio write count={} led=0x{:02X} bdf={:02X}:{:02X}.{} bar0=0x{:016X} virt=0x{:016X}\n",
            count,
            value,
            bus,
            slot,
            function,
            bar_phys,
            led_reg
        );
    }
}

fn ensure_host_mailbox() -> Option<TgaHostMailbox> {
    if !TGA_HOST_MAILBOX_ENABLED {
        return None;
    }

    {
        let guard = TGA_HOST_MAILBOX.lock();
        if let Some(mb) = *guard {
            return Some(mb);
        }
    }

    let Some((phys, virt)) = crate::dma::alloc(TGA_HOST_MB_BYTES, TGA_HOST_MB_ALIGN) else {
        crate::log!(
            "tga: host-mailbox alloc failed bytes={} align={}\n",
            TGA_HOST_MB_BYTES,
            TGA_HOST_MB_ALIGN
        );
        return None;
    };

    unsafe { write_bytes(virt, 0, TGA_HOST_MB_BYTES) };
    fence(Ordering::Release);

    let mb = TgaHostMailbox {
        phys,
        virt: virt as usize,
        len: TGA_HOST_MB_BYTES,
    };

    *TGA_HOST_MAILBOX.lock() = Some(mb);
    crate::log!(
        "tga: host-mailbox allocated phys=0x{:016X} virt=0x{:016X} bytes=0x{:X} layout magic+0 seq+4 value+8 status+12\n",
        mb.phys,
        mb.virt,
        mb.len
    );
    Some(mb)
}

fn publish_host_mailbox() {
    if !TGA_HOST_MAILBOX_ENABLED {
        return;
    }
    let Some(mb) = ensure_host_mailbox() else {
        return;
    };

    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        return;
    };
    let bus = tga.bus;
    let slot = tga.slot;
    let function = tga.function;
    let bar_phys = tga.bar_phys;
    let base = tga.mmio_base;

    Tga::write_reg(base + TGA_HOST_MB_ADDR_LO_OFF, mb.phys as u32);
    Tga::write_reg(base + TGA_HOST_MB_ADDR_HI_OFF, (mb.phys >> 32) as u32);
    Tga::write_reg(base + TGA_HOST_MB_BYTES_OFF, mb.len as u32);
    fence(Ordering::Release);
    Tga::write_reg(base + TGA_HOST_MB_DOORBELL_OFF, TGA_HOST_MB_DOORBELL_MAGIC);
    drop(guard);

    crate::log!(
        "tga: host-mailbox published bdf={:02X}:{:02X}.{} bar0=0x{:016X} phys=0x{:016X} bytes=0x{:X} doorbell=0x{:08X}\n",
        bus,
        slot,
        function,
        bar_phys,
        mb.phys,
        mb.len,
        TGA_HOST_MB_DOORBELL_MAGIC
    );
}

fn poll_host_mailbox() {
    if !TGA_HOST_MAILBOX_ENABLED {
        return;
    }
    let Some(mb) = *TGA_HOST_MAILBOX.lock() else {
        return;
    };

    fence(Ordering::Acquire);
    let base = mb.virt;
    let magic = unsafe { read_volatile((base + TGA_HOST_MB_MAGIC_OFF) as *const u32) };
    let seq = unsafe { read_volatile((base + TGA_HOST_MB_SEQ_OFF) as *const u32) };
    let value = unsafe { read_volatile((base + TGA_HOST_MB_VALUE_OFF) as *const u32) };
    let status = unsafe { read_volatile((base + TGA_HOST_MB_STATUS_OFF) as *const u32) };

    let last_seq = TGA_HOST_MAILBOX_LAST_SEQ.load(Ordering::Relaxed);
    let last_magic = TGA_HOST_MAILBOX_LAST_MAGIC.load(Ordering::Relaxed);
    if magic == 0 && seq == 0 && value == 0 && status == 0 {
        return;
    }
    if magic == last_magic && seq == last_seq {
        return;
    }

    TGA_HOST_MAILBOX_LAST_MAGIC.store(magic, Ordering::Relaxed);
    TGA_HOST_MAILBOX_LAST_SEQ.store(seq, Ordering::Relaxed);
    crate::log!(
        "tga: host-mailbox rx magic=0x{:08X} expected=0x{:08X} ok={} seq={} value=0x{:08X} status=0x{:08X} phys=0x{:016X}\n",
        magic,
        TGA_MAGIC_EXPECTED,
        (magic == TGA_MAGIC_EXPECTED) as u8,
        seq,
        value,
        status,
        mb.phys
    );
}

fn try_readback_doorbell(count: u32) {
    if !TGA_READBACK_DOORBELL_ENABLED || count < TGA_READBACK_DOORBELL_AFTER_WRITES {
        return;
    }
    if TGA_READBACK_DOORBELL_DONE.swap(true, Ordering::Relaxed) {
        return;
    }

    let guard = TGA.lock();
    let Some(tga) = guard.as_ref() else {
        TGA_READBACK_DOORBELL_DONE.store(false, Ordering::Relaxed);
        return;
    };
    let bus = tga.bus;
    let slot = tga.slot;
    let function = tga.function;
    let bar_phys = tga.bar_phys;
    let led_reg = tga.led_reg;
    let magic_reg = tga.magic_reg;
    tga.write_led(TGA_READBACK_DOORBELL_LED_VALUE);
    drop(guard);

    crate::log!(
        "tga: readback-doorbell posted led=0x{:02X} future_magic_expected=0x{:08X} bdf={:02X}:{:02X}.{} bar0=0x{:016X} led_virt=0x{:016X} magic_virt=0x{:016X}\n",
        TGA_READBACK_DOORBELL_LED_VALUE,
        TGA_MAGIC_EXPECTED,
        bus,
        slot,
        function,
        bar_phys,
        led_reg,
        magic_reg
    );
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
        if prev.bar_assigned_by_os {
            "assigned"
        } else {
            "fw"
        },
        if now.bar_assigned_by_os {
            "assigned"
        } else {
            "fw"
        },
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
        if tga.bar_assigned_by_os {
            "assigned"
        } else {
            "fw"
        },
        tga.mmio_base
    );
}

pub fn tga_led_write(value: u32) {
    write_led_raw(value);
}

pub fn tga_led_set(on: bool) {
    tga_led_write(if on { 1 } else { 0 });
}

pub fn tga_add_u32(a: u32, b: u32) -> Option<TgaAddProof> {
    let guard = TGA.lock();
    let proof = guard.as_ref()?.add_u32(a, b);
    crate::log!(
        "tga: add-proof a=0x{:08X} b=0x{:08X} result=0x{:08X} expected=0x{:08X} ok={} done={} polls={} status=0x{:08X} retire={} error=0x{:08X}\n",
        proof.a,
        proof.b,
        proof.result,
        proof.expected,
        proof.ok as u8,
        proof.done as u8,
        proof.polls,
        proof.status,
        proof.retire_count,
        proof.error
    );
    Some(proof)
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
        crate::pci::enumerate_impl();
    }

    let mut found: Option<PciDevice> = None;
    crate::pci::with_devices(|devices| {
        found = devices.iter().copied().find(is_tga);
    });
    let Some(dev) = found else {
        if crate::logflag::BOOT_INFO_LOGS {
            crate::logflag::TGA_MISSING_LOG_ONCE.call_once(|| {
                crate::log!(
                    "tga: device not found (vid=0x{:04X} did=0x{:04X}, scanned {} devices)\n",
                    TGA_VENDOR_ID,
                    TGA_DEVICE_ID,
                    device_count
                );
            });
        }
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
    TGA_READBACK_DOORBELL_DONE.store(false, Ordering::Relaxed);
    TGA_HOST_MAILBOX_LAST_MAGIC.store(0, Ordering::Relaxed);
    TGA_HOST_MAILBOX_LAST_SEQ.store(0, Ordering::Relaxed);
    publish_host_mailbox();
    if TGA_BOOT_MMIO_TOUCH_ENABLED {
        // Keep contract explicit when MMIO touch is enabled: default to LED off.
        tga_led_set(false);
    }
    if TGA_ADD_PROOF_ON_CONNECT_ENABLED {
        let _ = tga_add_u32(0x1234_5678, 0x1111_2222);
    }
    true
}

pub fn init_once() {
    crate::log!("tga: init_once deferred; task owns hotplug/probe after network readiness\n");
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

    // Sanity check: if BAR is uninitialized (0) or at a suspiciously high address
    // (e.g. > 256 GiB), we force reassignment to our known-good 32-bit MMIO window.
    //
    // Context: we've observed the device reporting 0x3800_0000_0000 (approx 61 TiB).
    // This exceeds the physical address width of many hosts (e.g. 39-bit = 512 GiB)
    // and causes QEMU VFIO DMA map failures (error -22) when the guest enables
    // the BAR.
    if bar_phys == 0 || bar_phys >= 0x40_0000_0000 {
        bar_assigned_by_os = true;
        // Hotplug path: firmware may not have assigned BARs for devices appearing later.
        // Allocate a fixed 1 KiB window and program BAR0/BAR1.
        let size = TGA_EXPECTED_BAR0_SIZE;
        // Keep BAR base at least 4KiB aligned.
        // The current FPGA-side write decode matches BAR0 + 0x00 via address low bits,
        // so non-page-aligned hotplug bases (e.g. ...FC00) can miss that match.
        let align = TGA_EXPECTED_BAR0_SIZE.max(0x1000);

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
    } else {
        // If the BAR was already valid, ensure the device is enabled now.
        crate::pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);
    }

    // We only need the first few BAR0 registers, so mapping 1 page keeps it minimal.
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
    let magic_reg = base + TGA_MAGIC_OFF;
    let add_status_reg = base + TGA_ADD_STATUS_OFF;
    let add_cmd_reg = base + TGA_ADD_CMD_OFF;
    let add_arg0_reg = base + TGA_ADD_ARG0_OFF;
    let add_arg1_reg = base + TGA_ADD_ARG1_OFF;
    let add_result_reg = base + TGA_ADD_RESULT_OFF;
    let add_retire_count_reg = base + TGA_ADD_RETIRE_COUNT_OFF;
    let add_error_reg = base + TGA_ADD_ERROR_OFF;

    let cmd_after = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x04);

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
        magic_reg,
        add_status_reg,
        add_cmd_reg,
        add_arg0_reg,
        add_arg1_reg,
        add_result_reg,
        add_retire_count_reg,
        add_error_reg,
    };
    if TGA_BOOT_MMIO_TOUCH_ENABLED {
        tga.write_led(0);
    }
    Some(tga)
}

#[embassy_executor::task]
pub(crate) async fn tga_task() {
    crate::logflag::TGA_TASK_STARTED_LOG_ONCE.call_once(|| {
        crate::log!("tga: task started\n");
    });
    let mut presence_miss_streak: u8 = 0;
    let period = EmbassyDuration::from_millis(TGA_HEARTBEAT_PERIOD_MS);
    let presence_probe_period = EmbassyDuration::from_millis(TGA_PRESENCE_PROBE_PERIOD_MS);
    let mut next_tick = Instant::now() + period;
    let mut next_presence_probe = Instant::now() + presence_probe_period;
    loop {
        if !is_online() {
            crate::pci::enumerate_impl();
            let _ = try_init();
            presence_miss_streak = 0;
            next_tick = Instant::now() + period;
            next_presence_probe = Instant::now() + presence_probe_period;
            Timer::after(EmbassyDuration::from_millis(TGA_OFFLINE_RETRY_MS)).await;
            continue;
        }

        let now = Instant::now();
        if now >= next_presence_probe {
            // Probe less frequently than heartbeat writes to keep LED cadence stable.
            let present = {
                let guard = TGA.lock();
                guard.as_ref().map(is_present).unwrap_or(false)
            };
            if !present {
                presence_miss_streak = presence_miss_streak.saturating_add(1);
                if presence_miss_streak >= TGA_PRESENCE_MISS_THRESHOLD {
                    {
                        let mut guard = TGA.lock();
                        if let Some(old) = guard.take() {
                            *TGA_LAST_DISCONNECT.lock() = Some(snapshot_from_tga(&old));
                            log_tga_state("disconnected", &old);
                        }
                    }
                    presence_miss_streak = 0;
                    next_tick = Instant::now() + period;
                    next_presence_probe = Instant::now() + presence_probe_period;
                    Timer::after(EmbassyDuration::from_millis(TGA_OFFLINE_RETRY_MS)).await;
                    continue;
                }
            } else {
                presence_miss_streak = 0;
            }
            next_presence_probe = now + presence_probe_period;
        }

        let t = TGA_HEARTBEAT_COUNTER.fetch_add(1, Ordering::Relaxed);
        if TGA_HEARTBEAT_MMIO_ENABLED {
            // Send 0..31 then wrap.
            write_heartbeat_led(t & 0x1F, t);
        }
        try_readback_doorbell(t);
        poll_host_mailbox();

        let now = Instant::now();
        if next_tick <= now {
            next_tick = now + period;
        }
        Timer::at(next_tick).await;
        next_tick += period;
    }
}
