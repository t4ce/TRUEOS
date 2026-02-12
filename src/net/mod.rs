pub mod adapter;
pub mod core;
pub mod device;
pub mod e1000;
pub mod ring;
pub mod r8125;
pub mod r8168;
pub mod tls_socket;
pub mod vio;
pub mod tls;

use ::core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

use crate::net::core::NetCore;
use crate::net::device::NetDevice;
use crate::net::ring::NetRing;
use crate::net::r8125::R8125Adapter;
use crate::net::r8168::R8168Adapter;
use crate::net::vio::VirtioNetAdapter;
use crate::net::e1000::E1000Adapter;

const RX_DESC_COUNT: usize = 64;
const RX_BUF_SIZE: usize = 2048;
const POLL_BUDGET: usize = 256;
const ENABLE_R8125: bool = false;

enum ActiveDevice {
    Virtio(NetCore<VirtioNetAdapter>),
    E1000(NetCore<E1000Adapter>),
    R8125(NetCore<R8125Adapter>),
    R8168(NetCore<R8168Adapter>),
}

impl NetDevice for ActiveDevice {
    fn mac(&self) -> [u8; 6] {
        match self {
            ActiveDevice::Virtio(dev) => dev.mac(),
            ActiveDevice::E1000(dev) => dev.mac(),
            ActiveDevice::R8125(dev) => dev.mac(),
            ActiveDevice::R8168(dev) => dev.mac(),
        }
    }

    fn poll_rx(&mut self) {
        match self {
            ActiveDevice::Virtio(dev) => dev.poll_rx(),
            ActiveDevice::E1000(dev) => dev.poll_rx(),
            ActiveDevice::R8125(dev) => dev.poll_rx(),
            ActiveDevice::R8168(dev) => dev.poll_rx(),
        }
    }

    fn pop_rx(&mut self) -> Option<alloc::vec::Vec<u8>> {
        match self {
            ActiveDevice::Virtio(dev) => dev.pop_rx(),
            ActiveDevice::E1000(dev) => dev.pop_rx(),
            ActiveDevice::R8125(dev) => dev.pop_rx(),
            ActiveDevice::R8168(dev) => dev.pop_rx(),
        }
    }

    fn transmit(&mut self, frame: &[u8]) -> Result<(), ()> {
        match self {
            ActiveDevice::Virtio(dev) => dev.transmit(frame),
            ActiveDevice::E1000(dev) => dev.transmit(frame),
            ActiveDevice::R8125(dev) => dev.transmit(frame),
            ActiveDevice::R8168(dev) => dev.transmit(frame),
        }
    }
}

static DEVICES: Mutex<alloc::vec::Vec<ActiveDevice>> = Mutex::new(alloc::vec::Vec::new());
static PRIMARY_DEVICE_INDEX: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "dma_nic_fpga")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaFpgaStreamStatus {
    pub active: bool,
    pub filter_enabled: bool,
    pub rx_packets_seen: u64,
    pub rx_packets_matched: u64,
    pub queued_packets: u64,
    pub queue_failures: u64,
}

#[cfg(feature = "dma_nic_fpga")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaFpgaIpProto {
    Tcp,
    Udp,
}

#[cfg(feature = "dma_nic_fpga")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaFpgaFlowFilter {
    pub proto: DmaFpgaIpProto,
    pub src_ip: Option<[u8; 4]>,
    pub dst_ip: Option<[u8; 4]>,
    pub src_port: Option<u16>,
    pub dst_port: Option<u16>,
}

#[cfg(feature = "dma_nic_fpga")]
#[derive(Clone, Copy)]
struct DmaFpgaStreamState {
    active: bool,
    filter: Option<DmaFpgaFlowFilter>,
    rx_packets_seen: u64,
    rx_packets_matched: u64,
    queued_packets: u64,
    queue_failures: u64,
}

#[cfg(feature = "dma_nic_fpga")]
static DMA_FPGA_STREAM_STATE: Mutex<DmaFpgaStreamState> = Mutex::new(DmaFpgaStreamState {
    active: false,
    filter: None,
    rx_packets_seen: 0,
    rx_packets_matched: 0,
    queued_packets: 0,
    queue_failures: 0,
});

pub fn init() {
    {
        let mut guard = DEVICES.lock();
        guard.clear();
    }
    PRIMARY_DEVICE_INDEX.store(0, Ordering::Relaxed);

    let mut added: usize = 0;

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

    for adapter in E1000Adapter::init_all() {
        let ring = NetRing::new(RX_DESC_COUNT, RX_BUF_SIZE, POLL_BUDGET);
        let mut guard = DEVICES.lock();
        guard.push(ActiveDevice::E1000(NetCore::new(adapter, ring)));
        added += 1;
    }

    if ENABLE_R8125 {
        for adapter in R8125Adapter::init_all() {
            let ring = NetRing::new(RX_DESC_COUNT, RX_BUF_SIZE, POLL_BUDGET);
            let mut guard = DEVICES.lock();
            guard.push(ActiveDevice::R8125(NetCore::new(adapter, ring)));
            added += 1;
        }
    } else {
        crate::log!("net: r8125 disabled (temporary)\n");
    }

    for adapter in R8168Adapter::init_all() {
        let ring = NetRing::new(RX_DESC_COUNT, RX_BUF_SIZE, POLL_BUDGET);
        let mut guard = DEVICES.lock();
        guard.push(ActiveDevice::R8168(NetCore::new(adapter, ring)));
        added += 1;
    }

    if added == 0 {
        crate::log!("net: no supported NIC detected.\n");
    } else {
        crate::log!("net: detected {} NIC(s); primary=0 (initial)\n", added);
    }

    crate::log!("net: hint: prefer virtio-net in QEMU (e.g. -netdev user,id=net0,hostfwd=tcp::4245-:4245 -device virtio-net-pci,netdev=net0)\n");
}

pub fn poll_at(index: usize) {
    let _ = with_device_at(index, |dev| dev.poll_rx());
}

pub fn pop_rx_packet_at(index: usize) -> Option<alloc::vec::Vec<u8>> {
    with_device_at(index, |dev| dev.pop_rx()).flatten()
}

pub fn transmit_packet_at(index: usize, data: &[u8]) -> Result<(), ()> {
    with_device_at(index, |dev| dev.transmit(data)).unwrap_or(Err(()))
}

pub fn mac_address() -> Option<[u8; 6]> {
    mac_address_at(primary_device_index())
}

pub fn mac_address_at(index: usize) -> Option<[u8; 6]> {
    with_device_at(index, |dev| Some(dev.mac())).flatten()
}

pub fn device_count() -> usize {
    DEVICES.lock().len()
}

pub fn primary_device_index() -> usize {
    let count = device_count();
    if count == 0 {
        return 0;
    }
    let idx = PRIMARY_DEVICE_INDEX.load(Ordering::Relaxed);
    idx.min(count - 1)
}

pub fn set_primary_device_index(index: usize) {
    let count = device_count();
    if count == 0 {
        return;
    }
    let new_idx = index.min(count - 1);
    let old_idx = PRIMARY_DEVICE_INDEX.swap(new_idx, Ordering::Relaxed);
    if old_idx != new_idx {
        crate::log!("net: primary device switched {} -> {}\n", old_idx, new_idx);
    }
}

fn with_device_at<R>(index: usize, f: impl FnOnce(&mut dyn NetDevice) -> R) -> Option<R> {
    let mut guard = DEVICES.lock();
    let dev = guard.get_mut(index)?;
    Some(f(dev))
}

#[cfg(feature = "dma_nic_fpga")]
pub fn dma_fpga_stream_begin() -> Result<(), &'static str> {
    let mut st = DMA_FPGA_STREAM_STATE.lock();
    if st.active {
        return Err("already active");
    }
    st.active = true;
    st.filter = None;
    st.rx_packets_seen = 0;
    st.rx_packets_matched = 0;
    st.queued_packets = 0;
    st.queue_failures = 0;
    Ok(())
}



#[cfg(feature = "dma_nic_fpga")]
fn rx_packet_matches_filter(packet: &[u8], filter: DmaFpgaFlowFilter) -> bool {
    if packet.len() < 14 {
        return false;
    }

    let mut ether_type = u16::from_be_bytes([packet[12], packet[13]]);
    let mut l2_off = 14usize;
    if ether_type == 0x8100 {
        if packet.len() < 18 {
            return false;
        }
        ether_type = u16::from_be_bytes([packet[16], packet[17]]);
        l2_off = 18;
    }
    if ether_type != 0x0800 || packet.len() < l2_off + 20 {
        return false;
    }

    let ver_ihl = packet[l2_off];
    if (ver_ihl >> 4) != 4 {
        return false;
    }
    let ihl = ((ver_ihl & 0x0F) as usize) * 4;
    if ihl < 20 || packet.len() < l2_off + ihl {
        return false;
    }

    let proto = packet[l2_off + 9];
    let want_proto = match filter.proto {
        DmaFpgaIpProto::Tcp => 6u8,
        DmaFpgaIpProto::Udp => 17u8,
    };
    if proto != want_proto {
        return false;
    }

    let src_ip = [
        packet[l2_off + 12],
        packet[l2_off + 13],
        packet[l2_off + 14],
        packet[l2_off + 15],
    ];
    let dst_ip = [
        packet[l2_off + 16],
        packet[l2_off + 17],
        packet[l2_off + 18],
        packet[l2_off + 19],
    ];

    if let Some(want) = filter.src_ip {
        if src_ip != want {
            return false;
        }
    }
    if let Some(want) = filter.dst_ip {
        if dst_ip != want {
            return false;
        }
    }

    let l4_off = l2_off + ihl;
    if packet.len() < l4_off + 4 {
        return false;
    }
    let src_port = u16::from_be_bytes([packet[l4_off], packet[l4_off + 1]]);
    let dst_port = u16::from_be_bytes([packet[l4_off + 2], packet[l4_off + 3]]);

    if let Some(want) = filter.src_port {
        if src_port != want {
            return false;
        }
    }
    if let Some(want) = filter.dst_port {
        if dst_port != want {
            return false;
        }
    }

    true
}

#[cfg(feature = "dma_nic_fpga")]
pub(crate) fn dma_fpga_stream_on_rx_packet(packet: &[u8]) {
    let mut st = DMA_FPGA_STREAM_STATE.lock();
    if !st.active {
        return;
    }

    st.rx_packets_seen = st.rx_packets_seen.saturating_add(1);
    if let Some(filter) = st.filter {
        if !rx_packet_matches_filter(packet, filter) {
            return;
        }
    }
    st.rx_packets_matched = st.rx_packets_matched.saturating_add(1);
    match crate::pci::nic_fpga_dma::submit_nic_frame_copy(packet) {
        Ok(_) => st.queued_packets = st.queued_packets.saturating_add(1),
        Err(_) => st.queue_failures = st.queue_failures.saturating_add(1),
    }
}



