use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_time::{Duration, Instant, Timer};
use spin::Mutex;

use super::VNet;

const PEER_STALE_MS: u64 = 120_000;
const PEER_ADVERTISE_INTERVAL_MS: u64 = 1000;
const PEER_QUERY_TIMEOUT_MS: u64 = 3000;
const PEER_FETCH_TIMEOUT_MS: u64 = 10_000;
const PEER_FETCH_MAX_BYTES: usize = 256 * 1024 * 1024;
const QUIET_LIST_WINDOW_MS: u64 = 120;

static PEERS: Mutex<Vec<PeerSnapshot>> = Mutex::new(Vec::new());
static LOCAL_NODE_ID: AtomicU64 = AtomicU64::new(0);
static LAST_ADVERTISE_MS: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub(crate) struct PeerSnapshot {
    pub id: usize,
    pub addr: [u8; 4],
    pub port: u16,
    pub node_id: u64,
    pub caps: u32,
    pub last_seen_ms: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct PeerVmOffer {
    pub vm_id: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PeerStoreError {
    NoNetwork,
    SubmitFailed,
    Timeout,
    Closed,
    MissingVm,
    TooLarge,
    Protocol,
}

fn monotonic_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

fn local_node_id() -> u64 {
    let cached = LOCAL_NODE_ID.load(Ordering::Acquire);
    if cached != 0 {
        return cached;
    }

    let mac = crate::net::mac_address_at(crate::net::primary_device_index())
        .unwrap_or([0x54, 0x52, 0x55, 0x45, 0x4f, 0x53]);
    let id = 0x5400_0000_0000_0000u64
        | ((mac[0] as u64) << 40)
        | ((mac[1] as u64) << 32)
        | ((mac[2] as u64) << 24)
        | ((mac[3] as u64) << 16)
        | ((mac[4] as u64) << 8)
        | mac[5] as u64;

    match LOCAL_NODE_ID.compare_exchange(0, id, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => id,
        Err(existing) => existing,
    }
}

fn is_local_advertisement(advertisement: &trueos_esp::gate::TrueOsHostAdvertisement) -> bool {
    if advertisement.node_id != 0 && advertisement.node_id == local_node_id() {
        return true;
    }

    let dev_idx = crate::net::primary_device_index();
    crate::net::adapter::ipv4_at(dev_idx)
        .map(|addr| addr == advertisement.from.addr)
        .unwrap_or(false)
}

pub(crate) fn publish_host_advertisement(advertisement: trueos_esp::gate::TrueOsHostAdvertisement) {
    if is_local_advertisement(&advertisement) {
        return;
    }

    let now = monotonic_ms();
    let mut peers = PEERS.lock();
    if let Some(peer) = peers
        .iter_mut()
        .find(|peer| peer.addr == advertisement.from.addr && peer.node_id == advertisement.node_id)
    {
        peer.port = advertisement.peer_tcp_port;
        peer.caps = advertisement.caps;
        peer.last_seen_ms = now;
        return;
    }

    let id = peers.len();
    crate::log!(
        "trueos-peer: discovered id={} addr={}.{}.{}.{} port={} node=0x{:016X} caps=0x{:08X}\n",
        id,
        advertisement.from.addr[0],
        advertisement.from.addr[1],
        advertisement.from.addr[2],
        advertisement.from.addr[3],
        advertisement.peer_tcp_port,
        advertisement.node_id,
        advertisement.caps
    );
    peers.push(PeerSnapshot {
        id,
        addr: advertisement.from.addr,
        port: advertisement.peer_tcp_port,
        node_id: advertisement.node_id,
        caps: advertisement.caps,
        last_seen_ms: now,
    });
}

pub(crate) fn take_peer_advertisement() -> Option<Vec<u8>> {
    let now = monotonic_ms();
    let last = LAST_ADVERTISE_MS.load(Ordering::Acquire);
    if now.saturating_sub(last) < PEER_ADVERTISE_INTERVAL_MS {
        return None;
    }
    if LAST_ADVERTISE_MS
        .compare_exchange(last, now, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return None;
    }

    Some(
        format!(
            "{} v=1 node=0x{:016X} tcp={} caps=registry,status,fs",
            trueos_esp::gate::TRUEOS_SWARM_MAGIC_TEXT,
            local_node_id(),
            crate::r::net::ports::VM_STORE_REPL_PORT
        )
        .into_bytes(),
    )
}

pub(crate) fn peer_snapshots() -> Vec<PeerSnapshot> {
    let now = monotonic_ms();
    let mut peers = PEERS.lock();
    peers.retain(|peer| now.saturating_sub(peer.last_seen_ms) <= PEER_STALE_MS);
    for (idx, peer) in peers.iter_mut().enumerate() {
        peer.id = idx;
    }
    peers.clone()
}

async fn connect_peer(
    peer: &PeerSnapshot,
    timeout_ms: u64,
) -> Result<(VNet, v::vnet::NetHandle), PeerStoreError> {
    let Some(vnet) = VNet::open_primary() else {
        return Err(PeerStoreError::NoNetwork);
    };
    vnet.submit(v::vnet::Command::OpenTcpConnect {
        remote: v::vnet::EndpointV4::new(peer.addr, peer.port),
    })
    .map_err(|_| PeerStoreError::SubmitFailed)?;

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        while let Some(ev) = vnet.pop_event() {
            match ev {
                v::vnet::Event::TcpEstablished { handle, .. } => return Ok((vnet, handle)),
                v::vnet::Event::Closed { .. } => return Err(PeerStoreError::Closed),
                v::vnet::Event::Error { .. } => return Err(PeerStoreError::SubmitFailed),
                v::vnet::Event::Opened { .. }
                | v::vnet::Event::UdpPacket { .. }
                | v::vnet::Event::UdpPacketV6 { .. }
                | v::vnet::Event::TcpData { .. }
                | v::vnet::Event::TcpSent { .. }
                | v::vnet::Event::IcmpReply { .. }
                | v::vnet::Event::IcmpReplyV6 { .. } => {}
            }
        }

        if Instant::now() >= deadline {
            return Err(PeerStoreError::Timeout);
        }
        Timer::after(Duration::from_millis(10)).await;
    }
}

fn parse_vm_list(bytes: &[u8]) -> Result<Vec<PeerVmOffer>, PeerStoreError> {
    let text = core::str::from_utf8(bytes).map_err(|_| PeerStoreError::Protocol)?;
    let mut saw_listing = false;
    let mut offers = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_ascii_whitespace();
        if parts.next() != Some("VMS") {
            continue;
        }
        saw_listing = true;
        if let Some(id) = parts.next().and_then(|token| token.parse::<u8>().ok()) {
            offers.push(PeerVmOffer { vm_id: id });
        }
    }

    if saw_listing {
        Ok(offers)
    } else {
        Err(PeerStoreError::Protocol)
    }
}

pub(crate) async fn list_peer_vms(peer: &PeerSnapshot) -> Result<Vec<PeerVmOffer>, PeerStoreError> {
    let (vnet, handle) = connect_peer(peer, PEER_QUERY_TIMEOUT_MS).await?;
    vnet.send_tcp_all(handle, b"VM\n")
        .map_err(|_| PeerStoreError::SubmitFailed)?;

    let deadline = Instant::now() + Duration::from_millis(PEER_QUERY_TIMEOUT_MS);
    let mut quiet_deadline: Option<Instant> = None;
    let mut rx = Vec::new();
    loop {
        while let Some(ev) = vnet.pop_event() {
            match ev {
                v::vnet::Event::TcpData { handle: h, data } if h == handle => {
                    rx.extend_from_slice(data.as_slice());
                    quiet_deadline =
                        Some(Instant::now() + Duration::from_millis(QUIET_LIST_WINDOW_MS));
                }
                v::vnet::Event::Closed { handle: h } if h == handle => {
                    if rx.is_empty() {
                        return Err(PeerStoreError::Closed);
                    }
                    return parse_vm_list(rx.as_slice());
                }
                v::vnet::Event::Error { .. } => return Err(PeerStoreError::SubmitFailed),
                _ => {}
            }
        }

        if let Some(quiet) = quiet_deadline
            && Instant::now() >= quiet
        {
            let _ = vnet.submit(v::vnet::Command::Close { handle });
            return parse_vm_list(rx.as_slice());
        }
        if Instant::now() >= deadline {
            let _ = vnet.submit(v::vnet::Command::Close { handle });
            return Err(PeerStoreError::Timeout);
        }
        Timer::after(Duration::from_millis(10)).await;
    }
}

fn parse_vm_header(line: &[u8], requested_vm_id: u8) -> Result<usize, PeerStoreError> {
    let text = core::str::from_utf8(line)
        .map_err(|_| PeerStoreError::Protocol)?
        .trim();
    if text == "NO" {
        return Err(PeerStoreError::MissingVm);
    }

    let mut parts = text.split_ascii_whitespace();
    if parts.next() != Some("VM") {
        return Err(PeerStoreError::Protocol);
    }
    let Some(vm_id) = parts.next().and_then(|token| token.parse::<u8>().ok()) else {
        return Err(PeerStoreError::Protocol);
    };
    if vm_id != requested_vm_id {
        return Err(PeerStoreError::Protocol);
    }
    let _seq = parts
        .next()
        .and_then(|token| token.parse::<u64>().ok())
        .ok_or(PeerStoreError::Protocol)?;
    let len = parts
        .next()
        .and_then(|token| token.parse::<usize>().ok())
        .ok_or(PeerStoreError::Protocol)?;
    if len > PEER_FETCH_MAX_BYTES {
        return Err(PeerStoreError::TooLarge);
    }
    Ok(len)
}

pub(crate) async fn fetch_peer_vm(
    peer: &PeerSnapshot,
    vm_id: u8,
) -> Result<Vec<u8>, PeerStoreError> {
    let (vnet, handle) = connect_peer(peer, PEER_FETCH_TIMEOUT_MS).await?;
    let request = format!("VM {}\n", vm_id);
    vnet.send_tcp_all(handle, request.as_bytes())
        .map_err(|_| PeerStoreError::SubmitFailed)?;

    let deadline = Instant::now() + Duration::from_millis(PEER_FETCH_TIMEOUT_MS);
    let mut rx = Vec::new();
    let mut body_start = None;
    let mut body_len = 0usize;

    loop {
        while let Some(ev) = vnet.pop_event() {
            match ev {
                v::vnet::Event::TcpData { handle: h, data } if h == handle => {
                    rx.extend_from_slice(data.as_slice());
                    if body_start.is_none()
                        && let Some(pos) = rx.iter().position(|&b| b == b'\n')
                    {
                        body_len = parse_vm_header(&rx[..pos], vm_id)?;
                        body_start = Some(pos + 1);
                    }
                    if let Some(start) = body_start {
                        let end = start.saturating_add(body_len);
                        if rx.len() >= end {
                            let bytes = rx[start..end].to_vec();
                            let ack = format!("VM {} OK\n", vm_id);
                            let _ = vnet.send_tcp_all(handle, ack.as_bytes());
                            let _ = vnet.submit(v::vnet::Command::Close { handle });
                            return Ok(bytes);
                        }
                    }
                }
                v::vnet::Event::Closed { handle: h } if h == handle => {
                    return Err(PeerStoreError::Closed);
                }
                v::vnet::Event::Error { .. } => return Err(PeerStoreError::SubmitFailed),
                _ => {}
            }
        }

        if Instant::now() >= deadline {
            let _ = vnet.submit(v::vnet::Command::Close { handle });
            return Err(PeerStoreError::Timeout);
        }
        Timer::after(Duration::from_millis(10)).await;
    }
}

pub(crate) fn peer_addr_text(peer: &PeerSnapshot) -> String {
    format!("{}.{}.{}.{}", peer.addr[0], peer.addr[1], peer.addr[2], peer.addr[3])
}
