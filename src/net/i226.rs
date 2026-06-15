use core::ptr::{NonNull, read_volatile};
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::net::core::VendorAdapter;
use crate::net::device::LinkState;
use crate::net::ring::NetRing;
use crate::pci;

const INTEL_VENDOR_ID: u16 = 0x8086;
const I226V_DEVICE_ID: u16 = 0x125C;

const REG_CTRL: u32 = 0x0000;
const REG_STATUS: u32 = 0x0008;
const REG_EECD: u32 = 0x0010;
const REG_ICR: u32 = 0x00C0;
const REG_IMS: u32 = 0x00D0;
const REG_RCTL: u32 = 0x0100;
const REG_TCTL: u32 = 0x0400;
const REG_RAL0: u32 = 0x5400;
const REG_RAH0: u32 = 0x5404;

const STATUS_LU: u32 = 1 << 1;
const STATUS_FD: u32 = 1 << 0;
const STATUS_SPEED_MASK: u32 = (1 << 6) | (1 << 7);
const STATUS_SPEED_100: u32 = 1 << 6;
const STATUS_SPEED_1000: u32 = 1 << 7;
const STATUS_SPEED_2500: u32 = (1 << 6) | (1 << 7);

const PCI_STATUS_CAP_LIST: u16 = 1 << 4;
const PCI_CAP_PTR: u16 = 0x34;
const PCI_CAP_PM: u8 = 0x01;
const PCI_CAP_MSI: u8 = 0x05;
const PCI_CAP_PCIE: u8 = 0x10;
const PCI_CAP_MSIX: u8 = 0x11;

const ECAP_AER: u16 = 0x0001;
const ECAP_DSN: u16 = 0x0003;
const ECAP_LTR: u16 = 0x0018;
const ECAP_L1PM: u16 = 0x001E;
const ECAP_PTM: u16 = 0x001F;

const CAP_PM: u32 = 1 << 0;
const CAP_MSI: u32 = 1 << 1;
const CAP_PCIE: u32 = 1 << 2;
const CAP_MSIX: u32 = 1 << 3;
const ECAP_AER_BIT: u32 = 1 << 8;
const ECAP_DSN_BIT: u32 = 1 << 9;
const ECAP_LTR_BIT: u32 = 1 << 10;
const ECAP_L1PM_BIT: u32 = 1 << 11;
const ECAP_PTM_BIT: u32 = 1 << 12;

static PRIMARY_SNAPSHOT: Mutex<Option<I226Snapshot>> = Mutex::new(None);
static DIAG_SCREEN_DRAWN: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct I226Snapshot {
    pub(crate) bus: u8,
    pub(crate) slot: u8,
    pub(crate) function: u8,
    pub(crate) vendor: u16,
    pub(crate) device: u16,
    pub(crate) revision: u8,
    pub(crate) class: u8,
    pub(crate) subclass: u8,
    pub(crate) prog_if: u8,
    pub(crate) pci_command_before: u16,
    pub(crate) pci_command_after: u16,
    pub(crate) pci_status: u16,
    pub(crate) bar_index: u8,
    pub(crate) bar_phys: u64,
    pub(crate) bar_size: u64,
    pub(crate) map_size: usize,
    pub(crate) mac: [u8; 6],
    pub(crate) ctrl: u32,
    pub(crate) status: u32,
    pub(crate) eecd: u32,
    pub(crate) icr: u32,
    pub(crate) ims: u32,
    pub(crate) rctl: u32,
    pub(crate) tctl: u32,
    pub(crate) cap_mask: u32,
    pub(crate) msix_vectors: u16,
    pub(crate) passive: bool,
}

impl I226Snapshot {
    pub(crate) fn raw_link_up(self) -> bool {
        (self.status & STATUS_LU) != 0
    }

    pub(crate) fn raw_full_duplex(self) -> bool {
        (self.status & STATUS_FD) != 0
    }

    pub(crate) fn raw_speed_mbps(self) -> u32 {
        match self.status & STATUS_SPEED_MASK {
            0 => 10,
            STATUS_SPEED_100 => 100,
            STATUS_SPEED_1000 => 1000,
            STATUS_SPEED_2500 => 2500,
            _ => 0,
        }
    }

    pub(crate) fn caps_text(self) -> &'static str {
        match self.cap_mask
            & (CAP_PM
                | CAP_MSI
                | CAP_PCIE
                | CAP_MSIX
                | ECAP_AER_BIT
                | ECAP_DSN_BIT
                | ECAP_LTR_BIT
                | ECAP_L1PM_BIT
                | ECAP_PTM_BIT)
        {
            0 => "none",
            _ => "pm msi msix pcie aer dsn ltr l1ss ptm",
        }
    }
}

struct Mmio {
    base: NonNull<u8>,
}

unsafe impl Send for Mmio {}

impl Mmio {
    #[inline]
    unsafe fn read_u32(&self, off: u32) -> u32 {
        read_volatile(self.base.as_ptr().add(off as usize) as *const u32)
    }
}

pub(crate) struct I226Adapter {
    mmio: Mmio,
    pci: pci::PciDevice,
    mac: [u8; 6],
    snapshot: I226Snapshot,
    poll_ticks: u64,
}

unsafe impl Send for I226Adapter {}

impl I226Adapter {
    pub(crate) fn init_all() -> alloc::vec::Vec<Self> {
        let mut out = alloc::vec::Vec::new();
        for dev in find_i226_devices() {
            match Self::init_from_device(dev) {
                Ok(adapter) => out.push(adapter),
                Err(()) => {
                    crate::log_warn!(
                        target: "net";
                        "net/i226: passive claim failed for {:02x}:{:02x}.{}\n",
                        dev.bus,
                        dev.slot,
                        dev.function
                    );
                }
            }
        }
        out
    }

    fn init_from_device(dev: pci::PciDevice) -> Result<Self, ()> {
        let pci_command_before = pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x04);
        pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);
        let pci_command_after = pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x04);
        let pci_status = pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x06);
        let revision = pci::config_read_u8(dev.bus, dev.slot, dev.function, 0x08);
        let (bar_index, bar_phys) = find_mmio_bar_phys(&dev)?;
        let bar_size = pci::bar_size_bytes(dev.bus, dev.slot, dev.function, bar_index).unwrap_or(0);
        let map_size = usize::try_from(bar_size)
            .ok()
            .filter(|size| *size != 0)
            .unwrap_or(0x10_0000);
        let mapped = match pci::mmio::map_mmio_region_exact(bar_phys, map_size) {
            Ok(mapped) => mapped,
            Err(err) => {
                crate::log_warn!(
                    target: "net";
                    "net/i226: bar{} mmio map failed phys=0x{:x} size=0x{:x} err={:?}\n",
                    bar_index,
                    bar_phys,
                    map_size,
                    err
                );
                return Err(());
            }
        };
        let mmio = Mmio { base: mapped };
        let cap_info = read_cap_info(&dev);
        let (ctrl, status, eecd, icr, ims, rctl, tctl, mac) = unsafe {
            let ral = mmio.read_u32(REG_RAL0);
            let rah = mmio.read_u32(REG_RAH0);
            (
                mmio.read_u32(REG_CTRL),
                mmio.read_u32(REG_STATUS),
                mmio.read_u32(REG_EECD),
                mmio.read_u32(REG_ICR),
                mmio.read_u32(REG_IMS),
                mmio.read_u32(REG_RCTL),
                mmio.read_u32(REG_TCTL),
                [
                    (ral & 0xFF) as u8,
                    ((ral >> 8) & 0xFF) as u8,
                    ((ral >> 16) & 0xFF) as u8,
                    ((ral >> 24) & 0xFF) as u8,
                    (rah & 0xFF) as u8,
                    ((rah >> 8) & 0xFF) as u8,
                ],
            )
        };
        let snapshot = I226Snapshot {
            bus: dev.bus,
            slot: dev.slot,
            function: dev.function,
            vendor: dev.vendor,
            device: dev.device,
            revision,
            class: dev.class,
            subclass: dev.subclass,
            prog_if: dev.prog_if,
            pci_command_before,
            pci_command_after,
            pci_status,
            bar_index,
            bar_phys,
            bar_size,
            map_size,
            mac,
            ctrl,
            status,
            eecd,
            icr,
            ims,
            rctl,
            tctl,
            cap_mask: cap_info.cap_mask,
            msix_vectors: cap_info.msix_vectors,
            passive: true,
        };
        publish_snapshot(snapshot);
        crate::log_info!(
            target: "net";
            "net/i226: passive claim bdf={:02x}:{:02x}.{} vid={:04x} did={:04x} rev={:02x} bar{}=0x{:x} bar_size=0x{:x} map=0x{:x} cmd=0x{:04x}->0x{:04x} status=0x{:08x} link_raw={} speed_raw={} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} msix_vectors={} mode=diagnostic-only\n",
            snapshot.bus,
            snapshot.slot,
            snapshot.function,
            snapshot.vendor,
            snapshot.device,
            snapshot.revision,
            snapshot.bar_index,
            snapshot.bar_phys,
            snapshot.bar_size,
            snapshot.map_size,
            snapshot.pci_command_before,
            snapshot.pci_command_after,
            snapshot.status,
            snapshot.raw_link_up() as u8,
            snapshot.raw_speed_mbps(),
            mac[0],
            mac[1],
            mac[2],
            mac[3],
            mac[4],
            mac[5],
            snapshot.msix_vectors
        );
        Ok(Self {
            mmio,
            pci: dev,
            mac,
            snapshot,
            poll_ticks: 0,
        })
    }

    fn refresh_snapshot(&mut self) {
        self.poll_ticks = self.poll_ticks.saturating_add(1);
        let status = unsafe { self.mmio.read_u32(REG_STATUS) };
        if status != self.snapshot.status {
            let old = self.snapshot.status;
            self.snapshot.status = status;
            publish_snapshot(self.snapshot);
            crate::log_info!(
                target: "net";
                "net/i226: status change bdf={:02x}:{:02x}.{} 0x{:08x}->0x{:08x} link_raw={} speed_raw={} passive=1\n",
                self.snapshot.bus,
                self.snapshot.slot,
                self.snapshot.function,
                old,
                status,
                self.snapshot.raw_link_up() as u8,
                self.snapshot.raw_speed_mbps()
            );
        } else if self.poll_ticks == 1 || self.poll_ticks.is_multiple_of(10_000) {
            publish_snapshot(self.snapshot);
        }
    }
}

impl VendorAdapter for I226Adapter {
    fn mac(&self) -> [u8; 6] {
        self.mac
    }

    fn poll_rx(&mut self) {
        self.refresh_snapshot();
    }

    fn pop_rx(&mut self) -> Option<alloc::vec::Vec<u8>> {
        None
    }

    fn transmit(&mut self, _frame: &[u8]) -> Result<(), ()> {
        Err(())
    }

    fn link_state(&self) -> LinkState {
        // Stack-facing link is intentionally down until RX/TX rings are implemented.
        LinkState::down()
    }

    fn pci_device(&self) -> Option<pci::PciDevice> {
        Some(self.pci)
    }

    fn bind_ring(&mut self, _ring: *mut NetRing) {}
}

#[embassy_executor::task]
pub(crate) async fn i226_diagnostic_display_task() {
    if !has_primary_snapshot() {
        return;
    }
    crate::intel::wait_hw_logo_sequence_done().await;
    Timer::after(EmbassyDuration::from_secs(10)).await;
    let Some(snapshot) = primary_snapshot() else {
        return;
    };
    let ok = crate::intel::present_i226_diagnostic_screen(snapshot, "i226-diagnostic-screen");
    DIAG_SCREEN_DRAWN.store(ok, Ordering::Release);
    crate::log_info!(
        target: "net";
        "net/i226: diagnostic display submitted ok={} bdf={:02x}:{:02x}.{} passive=1\n",
        ok as u8,
        snapshot.bus,
        snapshot.slot,
        snapshot.function
    );
}

pub(crate) fn has_primary_snapshot() -> bool {
    PRIMARY_SNAPSHOT.lock().is_some()
}

pub(crate) fn primary_snapshot() -> Option<I226Snapshot> {
    *PRIMARY_SNAPSHOT.lock()
}

fn publish_snapshot(snapshot: I226Snapshot) {
    let mut guard = PRIMARY_SNAPSHOT.lock();
    if guard.is_none()
        || guard.map(|s| (s.bus, s.slot, s.function))
            == Some((snapshot.bus, snapshot.slot, snapshot.function))
    {
        *guard = Some(snapshot);
    }
}

fn find_i226_devices() -> alloc::vec::Vec<pci::PciDevice> {
    let mut out = alloc::vec::Vec::new();
    pci::with_devices(|list| {
        for dev in list {
            if dev.vendor == INTEL_VENDOR_ID && dev.device == I226V_DEVICE_ID && dev.class == 0x02 {
                out.push(*dev);
            }
        }
    });
    out
}

fn find_mmio_bar_phys(dev: &pci::PciDevice) -> Result<(u8, u64), ()> {
    let mut i = 0u8;
    while i < 6 {
        let (bar_lo, bar_hi) = pci::read_bar_raw(dev.bus, dev.slot, dev.function, i);
        if bar_lo == 0 {
            i += 1;
            continue;
        }
        if (bar_lo & 0x1) != 0 {
            crate::log_info!(target: "net"; "net/i226: bar{} is IO raw=0x{:08x}\n", i, bar_lo);
            i += 1;
            continue;
        }
        let lo = (bar_lo as u64) & !0xFu64;
        let hi = bar_hi.unwrap_or(0) as u64;
        let phys = lo | (hi << 32);
        if phys != 0 {
            return Ok((i, phys));
        }
        i += 1;
    }
    Err(())
}

#[derive(Copy, Clone, Debug, Default)]
struct CapInfo {
    cap_mask: u32,
    msix_vectors: u16,
}

fn read_cap_info(dev: &pci::PciDevice) -> CapInfo {
    let mut out = CapInfo::default();
    let pci_status = pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x06);
    if (pci_status & PCI_STATUS_CAP_LIST) != 0 {
        let mut ptr =
            (pci::config_read_u8(dev.bus, dev.slot, dev.function, PCI_CAP_PTR) & !0x3) as u16;
        let mut guard = 0usize;
        while ptr >= 0x40 && ptr < 0x100 && guard < 48 {
            let id = pci::config_read_u8(dev.bus, dev.slot, dev.function, ptr);
            match id {
                PCI_CAP_PM => out.cap_mask |= CAP_PM,
                PCI_CAP_MSI => out.cap_mask |= CAP_MSI,
                PCI_CAP_PCIE => out.cap_mask |= CAP_PCIE,
                PCI_CAP_MSIX => {
                    out.cap_mask |= CAP_MSIX;
                    let ctl = pci::config_read_u16(dev.bus, dev.slot, dev.function, ptr + 2);
                    out.msix_vectors = (ctl & 0x07FF).saturating_add(1);
                }
                _ => {}
            }
            ptr = (pci::config_read_u8(dev.bus, dev.slot, dev.function, ptr + 1) & !0x3) as u16;
            guard += 1;
        }
    }

    let mut off = 0x100u16;
    let mut guard = 0usize;
    while off >= 0x100 && off < 0x1000 && guard < 64 {
        let hdr = pci::config_read_u32(dev.bus, dev.slot, dev.function, off);
        if hdr == 0 || hdr == 0xFFFF_FFFF {
            break;
        }
        match (hdr & 0xFFFF) as u16 {
            ECAP_AER => out.cap_mask |= ECAP_AER_BIT,
            ECAP_DSN => out.cap_mask |= ECAP_DSN_BIT,
            ECAP_LTR => out.cap_mask |= ECAP_LTR_BIT,
            ECAP_L1PM => out.cap_mask |= ECAP_L1PM_BIT,
            ECAP_PTM => out.cap_mask |= ECAP_PTM_BIT,
            _ => {}
        }
        let next = ((hdr >> 20) & 0xFFF) as u16;
        if next == 0 || next == off {
            break;
        }
        off = next;
        guard += 1;
    }
    out
}
