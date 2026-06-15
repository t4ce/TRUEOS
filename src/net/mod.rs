pub mod adapter;
pub mod cache_service;
pub mod core;
pub mod device;
pub mod dhcpv6;
pub mod i226;
pub mod iwl4965;
pub mod r8125;
pub mod r8139;
pub mod r8169;
pub mod ring;
pub mod tls;
pub mod tls_socket;
pub mod vio;
pub mod wifi;

use ::core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

use crate::net::core::NetCore;
use crate::net::device::NetDevice;
use crate::net::i226::I226Adapter;
use crate::net::r8125::R8125Adapter;
use crate::net::r8169::Rtl8169Driver;
use crate::net::ring::NetRing;
use crate::net::vio::VirtioNetAdapter;
use crate::pci::PciDevice;

const RX_DESC_COUNT: usize = 256;
const RX_BUF_SIZE: usize = 2048;
const POLL_BUDGET: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverStatus {
    Unloaded,
    Loading,
    Running,
}

#[derive(Debug, Clone, Copy)]
pub struct DriverInfo {
    pub name: &'static str,
    pub vendor_ids: &'static [(u16, u16)],
}

pub trait Driver {
    fn info(&self) -> &DriverInfo;
    fn probe(&mut self, pci_dev: &PciDevice) -> Result<(), &'static str>;
    fn start(&mut self) -> Result<(), &'static str>;
    fn status(&self) -> DriverStatus;
}

pub trait NetworkDriver: Driver {
    fn link_up(&self) -> bool;
    fn link_speed(&self) -> u32 {
        0
    }
    fn send(&mut self, data: &[u8]) -> Result<(), &'static str>;
    fn receive(&mut self) -> Option<alloc::vec::Vec<u8>>;
    fn poll(&mut self);
}

// Keep RTL8125 enabled, but keep RTL8168 as the primary NIC (dev0) by init order.
// Enable RTL8125 probing as a secondary NIC. Primary selection is kept stable (dev=0)
// so the two adapters don't interfere.
const ENABLE_R8125: bool = true;

enum ActiveDevice {
    Virtio(NetCore<VirtioNetAdapter>),
    //E1000(NetCore<E1000Adapter>),
    I226(NetCore<I226Adapter>),
    Rtl8169(NetCore<Rtl8169Driver>),
    R8125(NetCore<R8125Adapter>),
}

impl NetDevice for ActiveDevice {
    fn mac(&self) -> [u8; 6] {
        match self {
            ActiveDevice::Virtio(dev) => dev.mac(),
            //ActiveDevice::E1000(dev) => dev.mac(),
            ActiveDevice::I226(dev) => dev.mac(),
            ActiveDevice::Rtl8169(dev) => dev.mac(),
            ActiveDevice::R8125(dev) => dev.mac(),
        }
    }

    fn poll_rx(&mut self) -> bool {
        match self {
            ActiveDevice::Virtio(dev) => dev.poll_rx(),
            //ActiveDevice::E1000(dev) => dev.poll_rx(),
            ActiveDevice::I226(dev) => dev.poll_rx(),
            ActiveDevice::Rtl8169(dev) => dev.poll_rx(),
            ActiveDevice::R8125(dev) => dev.poll_rx(),
        }
    }

    fn pop_rx(&mut self) -> Option<alloc::vec::Vec<u8>> {
        match self {
            ActiveDevice::Virtio(dev) => dev.pop_rx(),
            // ActiveDevice::E1000(dev) => dev.pop_rx(),
            ActiveDevice::I226(dev) => dev.pop_rx(),
            ActiveDevice::Rtl8169(dev) => dev.pop_rx(),
            ActiveDevice::R8125(dev) => dev.pop_rx(),
        }
    }

    fn rx_queue_len(&self) -> usize {
        match self {
            ActiveDevice::Virtio(dev) => dev.rx_queue_len(),
            //   ActiveDevice::E1000(dev) => dev.rx_queue_len(),
            ActiveDevice::I226(dev) => dev.rx_queue_len(),
            ActiveDevice::Rtl8169(dev) => dev.rx_queue_len(),
            ActiveDevice::R8125(dev) => dev.rx_queue_len(),
        }
    }

    fn drain_rx_each(&mut self, limit: usize, f: &mut dyn FnMut(alloc::vec::Vec<u8>)) -> usize {
        match self {
            ActiveDevice::Virtio(dev) => dev.drain_rx_each(limit, f),
            //  ActiveDevice::E1000(dev) => dev.drain_rx_each(limit, f),
            ActiveDevice::I226(dev) => dev.drain_rx_each(limit, f),
            ActiveDevice::Rtl8169(dev) => dev.drain_rx_each(limit, f),
            ActiveDevice::R8125(dev) => dev.drain_rx_each(limit, f),
        }
    }

    fn transmit(&mut self, frame: &[u8]) -> Result<(), ()> {
        match self {
            ActiveDevice::Virtio(dev) => dev.transmit(frame),
            //  ActiveDevice::E1000(dev) => dev.transmit(frame),
            ActiveDevice::I226(dev) => dev.transmit(frame),
            ActiveDevice::Rtl8169(dev) => dev.transmit(frame),
            ActiveDevice::R8125(dev) => dev.transmit(frame),
        }
    }

    fn link_state(&self) -> crate::net::device::LinkState {
        match self {
            ActiveDevice::Virtio(dev) => dev.link_state(),
            //  ActiveDevice::E1000(dev) => dev.link_state(),
            ActiveDevice::I226(dev) => dev.link_state(),
            ActiveDevice::Rtl8169(dev) => dev.link_state(),
            ActiveDevice::R8125(dev) => dev.link_state(),
        }
    }
}

pub fn device_name_at(index: usize) -> Option<&'static str> {
    // Access DEVICES directly to match on enum variants
    let guard = DEVICES.lock();
    let dev = guard.get(index)?;
    Some(match dev {
        ActiveDevice::Virtio(_) => "Virtio Net",
        // ActiveDevice::E1000(_) => "Intel E1000",
        ActiveDevice::I226(_) => "Intel I226-V (diagnostic)",
        ActiveDevice::Rtl8169(_) => "Realtek RTL8169/8168",
        ActiveDevice::R8125(_) => "Realtek RTL8125",
    })
}

pub fn pci_device_at(index: usize) -> Option<PciDevice> {
    let guard = DEVICES.lock();
    let dev = guard.get(index)?;
    match dev {
        ActiveDevice::Virtio(n) => n.pci_device(),
        //  ActiveDevice::E1000(n) => n.pci_device(),
        ActiveDevice::I226(n) => n.pci_device(),
        ActiveDevice::Rtl8169(n) => n.pci_device(),
        ActiveDevice::R8125(n) => n.pci_device(),
    }
}

pub fn pci_id_at(index: usize) -> Option<(u16, u16)> {
    let d = pci_device_at(index)?;
    Some((d.vendor, d.device))
}

pub fn bdf_at(index: usize) -> Option<(u8, u8, u8)> {
    let d = pci_device_at(index)?;
    Some((d.bus, d.slot, d.function))
}

pub fn find_device_by_vidpid(vendor: u16, device: u16) -> Option<usize> {
    let count = device_count();
    for idx in 0..count {
        if pci_id_at(idx) == Some((vendor, device)) {
            return Some(idx);
        }
    }
    None
}

pub fn find_device_by_bdf(bus: u8, slot: u8, function: u8) -> Option<usize> {
    let count = device_count();
    for idx in 0..count {
        if bdf_at(idx) == Some((bus, slot, function)) {
            return Some(idx);
        }
    }
    None
}

fn parse_hex_u16(s: &str) -> Option<u16> {
    let s = s.trim();
    if s.is_empty() || s.len() > 4 {
        return None;
    }
    let mut out: u16 = 0;
    for b in s.as_bytes() {
        let v = match b {
            b'0'..=b'9' => (b - b'0') as u16,
            b'a'..=b'f' => (b - b'a' + 10) as u16,
            b'A'..=b'F' => (b - b'A' + 10) as u16,
            _ => return None,
        };
        out = (out << 4) | v;
    }
    Some(out)
}

fn parse_hex_u8(s: &str) -> Option<u8> {
    let v = parse_hex_u16(s)?;
    if v > u8::MAX as u16 {
        return None;
    }
    Some(v as u8)
}

fn parse_u8_dec_or_hex(s: &str) -> Option<u8> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let has_hex_alpha = s
        .as_bytes()
        .iter()
        .any(|b| matches!(b, b'a'..=b'f' | b'A'..=b'F'));
    if has_hex_alpha {
        parse_hex_u8(s)
    } else {
        s.parse::<u8>().ok()
    }
}

/// Map an app/service owner string to a NIC index.
///
/// Supported suffix formats (after the last '@'):
/// - `@<index>` (legacy)
/// - `@vvvv:pppp` (PCI vendor:device, hex)
/// - `@bb:dd.f` (PCI bus:slot.function; bus/slot hex, function dec/hex)
pub fn device_index_from_owner(owner: &str) -> Option<usize> {
    let (base, suffix) = owner.rsplit_once('@')?;
    if base.is_empty() || suffix.is_empty() {
        return None;
    }
    let suffix = suffix.trim();

    // Legacy numeric index.
    if suffix.as_bytes().iter().all(|b| b.is_ascii_digit()) {
        return suffix.parse::<usize>().ok();
    }

    // BDF: bb:dd.f
    if let Some((bus_s, rest)) = suffix.split_once(':')
        && let Some((slot_s, func_s)) = rest.split_once('.')
        && let (Some(bus), Some(slot), Some(function)) =
            (parse_hex_u8(bus_s), parse_hex_u8(slot_s), parse_u8_dec_or_hex(func_s))
        && let Some(idx) = find_device_by_bdf(bus, slot, function)
    {
        return Some(idx);
    }

    // VID:PID: vvvv:pppp
    if let Some((vid_s, pid_s)) = suffix.split_once(':')
        && let (Some(vid), Some(pid)) = (parse_hex_u16(vid_s), parse_hex_u16(pid_s))
        && let Some(idx) = find_device_by_vidpid(vid, pid)
    {
        return Some(idx);
    }

    None
}

static DEVICES: Mutex<alloc::vec::Vec<ActiveDevice>> = Mutex::new(alloc::vec::Vec::new());
static PRIMARY_DEVICE_INDEX: AtomicUsize = AtomicUsize::new(0);

pub fn init() {
    {
        let mut guard = DEVICES.lock();
        guard.clear();
    }
    PRIMARY_DEVICE_INDEX.store(0, Ordering::Relaxed);

    let mut added: usize = 0;
    let mut dormant_detected: usize = 0;

    // Ordering matters: most of the stack defaults to device 0 as the primary
    // interface (e.g. `mac_address()` and early boot probes). Prefer virtio in
    // virtualized environments so we get the best-performing/most-reliable NIC
    // without requiring any external run flags.

    for adapter in VirtioNetAdapter::init_all() {
        let ring = NetRing::new(RX_DESC_COUNT, RX_BUF_SIZE, POLL_BUDGET);
        let mut guard = DEVICES.lock();
        guard.push(ActiveDevice::Virtio(NetCore::new(adapter, ring)));
        added += 1;
    }
    /*
    for adapter in E1000Adapter::init_all() {
        let ring = NetRing::new(RX_DESC_COUNT, RX_BUF_SIZE, POLL_BUDGET);
        let mut guard = DEVICES.lock();
        guard.push(ActiveDevice::E1000(NetCore::new(adapter, ring)));
        added += 1;
    }
    */
    for adapter in Rtl8169Driver::init_all() {
        let ring = NetRing::new(RX_DESC_COUNT, RX_BUF_SIZE, POLL_BUDGET);
        let mut guard = DEVICES.lock();
        guard.push(ActiveDevice::Rtl8169(NetCore::new(adapter, ring)));
        added += 1;
    }

    for adapter in I226Adapter::init_all() {
        let ring = NetRing::new(RX_DESC_COUNT, RX_BUF_SIZE, POLL_BUDGET);
        let mut guard = DEVICES.lock();
        guard.push(ActiveDevice::I226(NetCore::new(adapter, ring)));
        added += 1;
    }

    if ENABLE_R8125 {
        for adapter in R8125Adapter::init_all() {
            let ring = NetRing::new(RX_DESC_COUNT, RX_BUF_SIZE, POLL_BUDGET);
            let mut guard = DEVICES.lock();
            guard.push(ActiveDevice::R8125(NetCore::new(adapter, ring)));
            added += 1;
        }
    }

    if crate::logflag::BOOT_INFO_LOGS {
        for dev in r8139::detect_all() {
            dormant_detected += 1;
            crate::log_info!(target: "net";
                "net: detected rtl8139 bdf={:02x}:{:02x}.{} vid={:04x} did={:04x} (not wired)\n",
                dev.bus,
                dev.slot,
                dev.function,
                dev.vendor_id,
                dev.device_id
            );
        }
    }

    // Prefer a link-up device as primary so swapping cables between ports
    // doesn't strand the stack on a permanently link-down dev0.
    if added != 0 {
        let mut chosen: usize = 0;
        {
            let guard = DEVICES.lock();
            for (idx, dev) in guard.iter().enumerate() {
                if dev.link_state().up {
                    chosen = idx;
                    break;
                }
            }
        }
        PRIMARY_DEVICE_INDEX.store(chosen, Ordering::Relaxed);
        if crate::logflag::BOOT_INFO_LOGS {
            crate::log_info!(target: "net"; "net: primary={} (link-up preference)\n", chosen);
        }
    }

    if added == 0 {
        if crate::logflag::BOOT_INFO_LOGS {
            if dormant_detected == 0 {
                crate::log_info!(target: "net"; "net: no supported NIC detected.\n");
            } else {
                crate::log_info!(target: "net";
                    "net: no supported NIC detected; {} rtl candidate(s) present but not wired.\n",
                    dormant_detected
                );
            }
        }
    } else {
        if crate::logflag::BOOT_INFO_LOGS {
            crate::log_info!(target: "net"; "net: detected {} NIC(s)\n", added);

            // Device inventory helps interpret logs like "tx-batch dev=0".
            let count = device_count();
            for idx in 0..count {
                let name = device_name_at(idx).unwrap_or("?");
                let link_up = link_state_at(idx).map(|ls| ls.up as u8).unwrap_or(0);
                let bdf = bdf_at(idx);
                if let Some((bus, slot, func)) = bdf {
                    crate::log_info!(target: "net";
                        "net: dev{} {} bdf={:02x}:{:02x}.{} link_up={}\n",
                        idx,
                        name,
                        bus,
                        slot,
                        func,
                        link_up
                    );
                } else {
                    crate::log_info!(target: "net"; "net: dev{} {} bdf=? link_up={}\n", idx, name, link_up);
                }
            }
        }
    }

    if crate::logflag::BOOT_INFO_LOGS {
        crate::log_info!(target: "net";
            "net: hint: prefer virtio-net in QEMU (e.g. -netdev user,id=net0,hostfwd=tcp::4245-:4245 -device virtio-net-pci,netdev=net0)\n"
        );
    }
}

pub fn poll_at(index: usize) -> bool {
    with_device_at(index, |dev| dev.poll_rx()).unwrap_or(false)
}

pub fn drain_rx_packets_each_at(
    index: usize,
    limit: usize,
    f: &mut dyn FnMut(alloc::vec::Vec<u8>),
) -> usize {
    with_device_at(index, |dev| dev.drain_rx_each(limit, f)).unwrap_or(0)
}

pub fn rx_pending_at(index: usize) -> usize {
    with_device_at(index, |dev| dev.rx_queue_len()).unwrap_or(0)
}

pub fn link_state_at(index: usize) -> Option<crate::net::device::LinkState> {
    with_device_at(index, |dev| dev.link_state())
}

pub fn transmit_batch_at(index: usize, packets: impl Iterator<Item = alloc::vec::Vec<u8>>) {
    with_device_at(index, |dev| {
        let link_up = dev.link_state().up;
        if !link_up {
            for pkt in packets {
                crate::net::ring::recycle_packet_buf(pkt);
            }
            return;
        }

        let mut ok_count: u32 = 0;
        let mut err_count: u32 = 0;
        for pkt in packets {
            match dev.transmit(&pkt) {
                Ok(()) => {
                    ok_count = ok_count.saturating_add(1);
                }
                Err(()) => {
                    err_count = err_count.saturating_add(1);
                }
            }
            // TX path copies into device DMA buffers for all current NICs, so we can
            // immediately recycle the Vec backing storage.
            crate::net::ring::recycle_packet_buf(pkt);
        }

        // If this ever triggers, the stack is producing frames but the NIC backend
        // is rejecting them (most commonly: TX ring full / wedged DMA).
        if err_count != 0 {
            crate::log_warn!(target: "net"; "net: tx-batch dev={} ok={} err={}\n", index, ok_count, err_count);
        }
    });
}

pub fn mac_address_at(index: usize) -> Option<[u8; 6]> {
    with_device_at(index, |dev| Some(dev.mac())).flatten()
}

pub fn device_count() -> usize {
    DEVICES.lock().len()
}

pub fn default_device_index() -> usize {
    if device_count() == 0 { 0 } else { 0 }
}

pub fn primary_device_index() -> usize {
    let count = device_count();
    if count == 0 {
        return 0;
    }
    let idx = PRIMARY_DEVICE_INDEX.load(Ordering::Relaxed);
    idx.min(count - 1)
}

fn with_device_at<R>(index: usize, f: impl FnOnce(&mut dyn NetDevice) -> R) -> Option<R> {
    let mut guard = DEVICES.lock();
    let dev = guard.get_mut(index)?;
    Some(f(dev))
}
