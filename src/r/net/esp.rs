use alloc::{collections::VecDeque, format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration, Timer};
use spin::Mutex;

use super::VNet;

const ESP_GATE_REGISTRY_MAX_DEVICES: usize = 64;
const ESP_STATUS_FETCH_TIMEOUT_MS: u32 = 3000;
const ESP_STATUS_FETCH_MAX_RX: usize = 1024;
const ESP_STATUS_POLL_MS: u64 = 1000;
const ESP_CONTROL_TIMEOUT_MS: u32 = 3000;
const ESP_CONTROL_MAX_RX: usize = 1024;
const TRUEOS_PEER_ADVERTISE_MS: u64 = 5000;
const TRUEOS_PEER_HELLO_MAX: usize = 128;
const TRUEOS_LUMEN_WORK_FRAME_MAX: usize = 256;
const TRUEOS_LUMEN_WORK_PROBE_TIMEOUT_MS: u64 = 3000;
const TRUEOS_LUMEN_WORK_PROBE_PREFIX: &str = "C0DEC0DE LUMEN_CAN_TAKE_WORK";
const TRUEOS_LUMEN_WORK_CAP_PREFIX: &str = "C0DEC0DE LUMEN_WORK_CAP";
const TRUEOS_LUMEN_MATVEC_SHADOW_PREFIX: &str = "C0DEC0DE LUMEN_MATVEC_SHADOW";
const TRUEOS_LUMEN_MATVEC_XCHUNK_PREFIX: &str = "C0DEC0DE LUMEN_MATVEC_XCHUNK";
const TRUEOS_LUMEN_MATVEC_RESULT_CHUNK_PREFIX: &str = "C0DEC0DE LUMEN_MATVEC_RESULT_CHUNK";
const TRUEOS_LUMEN_APP_MAGIC: u32 = 0x3153_4F4C; // LOS1
const TRUEOS_LUMEN_APP_HEADER_BYTES: usize = 12;
const TRUEOS_LUMEN_APP_RX_BUF_BYTES: usize = v::vnet::MAX_MSG * 4;
const TRUEOS_LUMEN_APP_VERSION: u8 = 1;
const TRUEOS_LUMEN_APP_OP_TEXT: u8 = 1;
const TRUEOS_SWARM_HOST_CAP: usize = crate::allcaps::net::TRUEOS_SWARM_HOST_CAP;
const TRUEOS_PEER_LINK_CAP: usize = crate::allcaps::net::TRUEOS_SWARM_PEER_LINK_CAP;
const TRUEOS_PEER_RX_BUF_BYTES: usize = crate::allcaps::net::TRUEOS_SWARM_PEER_RX_BUF_BYTES;

static DEVICE_REGISTRY: Mutex<trueos_esp::gate::DeviceRegistry> =
    Mutex::new(trueos_esp::gate::DeviceRegistry::with_trueos_host_limit(
        ESP_GATE_REGISTRY_MAX_DEVICES,
        TRUEOS_SWARM_HOST_CAP,
    ));
static STATUS_EVENTS: Mutex<VecDeque<trueos_esp::swarm::StatusChangeEvent>> =
    Mutex::new(VecDeque::new());
static REGISTRY_CHANGE_SEQ: AtomicU32 = AtomicU32::new(1);
static LUMEN_WORK_PROBE_SEQ: AtomicU32 = AtomicU32::new(1);
static LUMEN_WORK_PROBE_RESULT_SEQ: AtomicU32 = AtomicU32::new(1);
static LUMEN_WORK_PROBE_RESULT_SENT: AtomicU32 = AtomicU32::new(0);
static LUMEN_WORK_PROBE_RESULT_REPLIES: AtomicU32 = AtomicU32::new(0);
static LUMEN_WORK_PROBE_RESULT_BEST: AtomicU32 = AtomicU32::new(0);

fn monotonic_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

#[allow(dead_code)]
pub fn remove_device(handle: v::vnet::NetHandle) -> bool {
    let removed = DEVICE_REGISTRY.lock().remove_device(handle);
    if removed {
        note_registry_change();
    }
    removed
}

#[allow(dead_code)]
pub fn device_snapshot() -> Vec<trueos_esp::gate::DeviceSnapshot> {
    DEVICE_REGISTRY.lock().snapshot()
}

#[allow(dead_code)]
pub fn device_status_snapshot(
    handle: v::vnet::NetHandle,
) -> Option<trueos_esp::swarm::DeviceStatusSnapshot> {
    DEVICE_REGISTRY
        .lock()
        .snapshot_for(handle)
        .and_then(|snapshot| snapshot.status)
}

#[allow(dead_code)]
pub fn drain_status_events(max_events: usize) -> Vec<trueos_esp::swarm::StatusChangeEvent> {
    let mut out = Vec::new();
    let mut queue = STATUS_EVENTS.lock();
    for _ in 0..max_events {
        let Some(event) = queue.pop_front() else {
            break;
        };
        out.push(event);
    }
    out
}

#[allow(dead_code)]
pub fn registry_change_seq() -> u32 {
    REGISTRY_CHANGE_SEQ.load(Ordering::Acquire)
}

pub(crate) fn request_lumen_work_capacity_probe() -> u32 {
    let seq = LUMEN_WORK_PROBE_SEQ
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    crate::lumen::lumen_net::set_remote_bf16_route_available(false);
    crate::log!(
        "esp-gate: lumen work capacity probe requested seq={} timeout_ms={}\n",
        seq,
        TRUEOS_LUMEN_WORK_PROBE_TIMEOUT_MS
    );
    seq
}

pub(crate) fn prepare_lumen_offload_for_prompt() -> bool {
    let seq = request_lumen_work_capacity_probe();
    let start = embassy_time_driver::now();
    let timeout_ticks = TRUEOS_LUMEN_WORK_PROBE_TIMEOUT_MS
        .saturating_mul(embassy_time_driver::TICK_HZ.max(1))
        / 1000;

    loop {
        if LUMEN_WORK_PROBE_RESULT_SEQ.load(Ordering::Acquire) == seq {
            let sent = LUMEN_WORK_PROBE_RESULT_SENT.load(Ordering::Acquire);
            let replies = LUMEN_WORK_PROBE_RESULT_REPLIES.load(Ordering::Acquire);
            let best = LUMEN_WORK_PROBE_RESULT_BEST.load(Ordering::Acquire);
            let enabled = sent != 0 && replies != 0 && best != 0;
            crate::lumen::lumen_net::set_remote_bf16_route_available(enabled);
            crate::log!(
                "esp-gate: lumen prompt offload gate seq={} sent={} replies={} best={} enabled={}\n",
                seq,
                sent,
                replies,
                best,
                if enabled { 1 } else { 0 }
            );
            return enabled;
        }

        crate::time::poll();
        crate::smp::poll();
        if timeout_ticks != 0 && embassy_time_driver::now().saturating_sub(start) > timeout_ticks {
            crate::lumen::lumen_net::set_remote_bf16_route_available(false);
            crate::log!(
                "esp-gate: lumen prompt offload gate seq={} result=timeout enabled=0\n",
                seq
            );
            return false;
        }
        core::hint::spin_loop();
    }
}

fn note_registry_change() {
    REGISTRY_CHANGE_SEQ.fetch_add(1, Ordering::AcqRel);
}

fn snapshot_for_handle(handle: v::vnet::NetHandle) -> Option<trueos_esp::gate::DeviceSnapshot> {
    DEVICE_REGISTRY.lock().snapshot_for(handle)
}

fn trueos_node_id(vnet: &VNet) -> u64 {
    if let Some(v::vnet::MacAddr(mac)) = vnet.mac_address() {
        let mut bytes = [0u8; 8];
        bytes[0] = 0xC0;
        bytes[1] = 0xDE;
        bytes[2..].copy_from_slice(&mac);
        return u64::from_be_bytes(bytes);
    }
    0
}

fn trueos_peer_hello(node_id: u64) -> String {
    format!(
        "{} v=1 node=0x{:016X} tcp={} caps=registry,status,lumen-work\n",
        trueos_esp::gate::TRUEOS_SWARM_MAGIC_TEXT,
        node_id,
        trueos_esp::gate::TRUEOS_PEER_TCP_PORT
    )
}

fn submit_trueos_peer_hello(vnet: &VNet, handle: v::vnet::NetHandle, node_id: u64) {
    let hello = trueos_peer_hello(node_id);
    let bytes = hello.as_bytes();
    let len = bytes.len().min(TRUEOS_PEER_HELLO_MAX);
    let _ = vnet.submit(v::vnet::Command::SendTcp {
        handle,
        data: v::vnet::ByteBuf::from_slice_trunc(&bytes[..len]),
    });
}

fn trueos_lumen_work_probe_received(data: &[u8]) -> bool {
    core::str::from_utf8(data)
        .map(|text| {
            text.lines().any(|line| {
                line.trim_start()
                    .starts_with(TRUEOS_LUMEN_WORK_PROBE_PREFIX)
            })
        })
        .unwrap_or(false)
}

fn trueos_lumen_matvec_shadow_received(data: &[u8]) -> Option<&str> {
    let text = core::str::from_utf8(data).ok()?;
    text.lines().find(|line| {
        line.trim_start()
            .starts_with(TRUEOS_LUMEN_MATVEC_SHADOW_PREFIX)
    })
}

#[derive(Copy, Clone, Debug)]
struct LumenAppFrame<'a> {
    opcode: u8,
    payload: &'a [u8],
}

#[derive(Copy, Clone, Debug)]
enum LumenAppDrain {
    Incomplete,
    BadMagic,
    BadVersion,
    BadLength,
    Frame { opcode: u8, payload_len: usize },
}

fn encode_lumen_app_text_frame(payload: &[u8]) -> Option<Vec<u8>> {
    if payload.len() > u32::MAX as usize {
        return None;
    }
    let mut out = Vec::with_capacity(TRUEOS_LUMEN_APP_HEADER_BYTES.saturating_add(payload.len()));
    out.extend_from_slice(&TRUEOS_LUMEN_APP_MAGIC.to_le_bytes());
    out.push(TRUEOS_LUMEN_APP_VERSION);
    out.push(TRUEOS_LUMEN_APP_OP_TEXT);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    Some(out)
}

fn next_lumen_app_frame(buffer: &[u8]) -> LumenAppDrain {
    if buffer.len() < TRUEOS_LUMEN_APP_HEADER_BYTES {
        return LumenAppDrain::Incomplete;
    }
    let magic = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
    if magic != TRUEOS_LUMEN_APP_MAGIC {
        return LumenAppDrain::BadMagic;
    }
    if buffer[4] != TRUEOS_LUMEN_APP_VERSION {
        return LumenAppDrain::BadVersion;
    }
    let payload_len = u32::from_le_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]) as usize;
    if payload_len > v::vnet::MAX_MSG {
        return LumenAppDrain::BadLength;
    }
    let frame_len = TRUEOS_LUMEN_APP_HEADER_BYTES.saturating_add(payload_len);
    if buffer.len() < frame_len {
        return LumenAppDrain::Incomplete;
    }
    LumenAppDrain::Frame {
        opcode: buffer[5],
        payload_len,
    }
}

fn drain_lumen_app_frames<F>(buffer: &mut Vec<u8>, mut on_frame: F)
where
    F: FnMut(LumenAppFrame<'_>),
{
    loop {
        match next_lumen_app_frame(buffer.as_slice()) {
            LumenAppDrain::Incomplete => break,
            LumenAppDrain::BadMagic => {
                if let Some(offset) = find_lumen_app_magic(buffer.as_slice(), 1) {
                    buffer.drain(..offset);
                } else {
                    let keep = buffer.len().min(TRUEOS_LUMEN_APP_HEADER_BYTES - 1);
                    if keep == 0 {
                        buffer.clear();
                    } else {
                        let start = buffer.len().saturating_sub(keep);
                        buffer.drain(..start);
                    }
                    break;
                }
            }
            LumenAppDrain::BadVersion | LumenAppDrain::BadLength => {
                buffer.drain(..1);
            }
            LumenAppDrain::Frame {
                opcode,
                payload_len,
            } => {
                let start = TRUEOS_LUMEN_APP_HEADER_BYTES;
                let end = start.saturating_add(payload_len);
                on_frame(LumenAppFrame {
                    opcode,
                    payload: &buffer[start..end],
                });
                buffer.drain(..end);
            }
        }
    }
}

fn find_lumen_app_magic(buffer: &[u8], start: usize) -> Option<usize> {
    let needle = TRUEOS_LUMEN_APP_MAGIC.to_le_bytes();
    if buffer.len() < needle.len() || start >= buffer.len() {
        return None;
    }
    buffer[start..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| offset.saturating_add(start))
}

#[derive(Copy, Clone, Debug, Default)]
struct LumenWorkCapacity {
    lanes: u32,
    protocol_version: u16,
    caps: u32,
    workers: u32,
    pending: u32,
    min_rows: u32,
}

fn parse_u32_field(part: &str, name: &str) -> Option<u32> {
    let value = part.strip_prefix(name)?;
    if let Some(hex) = value.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).ok()
    } else {
        value.parse::<u32>().ok()
    }
}

fn parse_u64_field(part: &str, name: &str) -> Option<u64> {
    let value = part.strip_prefix(name)?;
    if let Some(hex) = value.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).ok()
    } else {
        value.parse::<u64>().ok()
    }
}

fn parse_usize_field(part: &str, name: &str) -> Option<usize> {
    parse_u64_field(part, name).and_then(|value| usize::try_from(value).ok())
}

fn parse_trueos_lumen_work_capacity(data: &[u8]) -> Option<LumenWorkCapacity> {
    let prefix = TRUEOS_LUMEN_WORK_CAP_PREFIX.as_bytes();
    let start = data
        .windows(prefix.len())
        .position(|window| window == prefix)?;
    let end = data[start..]
        .iter()
        .position(|byte| *byte == b'\n' || *byte == b'\r')
        .map(|offset| start.saturating_add(offset))
        .unwrap_or(data.len());
    let line = core::str::from_utf8(&data[start..end]).ok()?;
    let mut out = LumenWorkCapacity::default();
    for part in line.split_ascii_whitespace() {
        if let Some(value) = parse_u32_field(part, "n=") {
            out.lanes = value;
        } else if let Some(value) = parse_u32_field(part, "proto=") {
            out.protocol_version = value.min(u16::MAX as u32) as u16;
        } else if let Some(value) = parse_u32_field(part, "caps=") {
            out.caps = value;
        } else if let Some(value) = parse_u32_field(part, "workers=") {
            out.workers = value;
        } else if let Some(value) = parse_u32_field(part, "pending=") {
            out.pending = value;
        } else if let Some(value) = parse_u32_field(part, "min_rows=") {
            out.min_rows = value;
        }
    }
    Some(out)
}

fn publish_lumen_work_probe_result(seq: u32, sent: usize, replies: usize, best: u32) {
    LUMEN_WORK_PROBE_RESULT_SENT.store(sent.min(u32::MAX as usize) as u32, Ordering::Release);
    LUMEN_WORK_PROBE_RESULT_REPLIES.store(replies.min(u32::MAX as usize) as u32, Ordering::Release);
    LUMEN_WORK_PROBE_RESULT_BEST.store(best, Ordering::Release);
    LUMEN_WORK_PROBE_RESULT_SEQ.store(seq, Ordering::Release);
}

#[derive(Copy, Clone, Debug)]
struct LumenMatvecXChunk<'a> {
    job_id: u64,
    chunk_index: usize,
    offset: usize,
    bytes: usize,
    total: usize,
    hex: &'a str,
}

#[derive(Copy, Clone, Debug)]
struct LumenMatvecResultChunk<'a> {
    job_id: u64,
    chunk_index: usize,
    offset: usize,
    bytes: usize,
    total: usize,
    hex: &'a str,
}

#[derive(Copy, Clone, Debug)]
struct LumenMatvecShadowDescriptor {
    job_id: u64,
    matrix_id: u64,
    row_start: usize,
    row_end: usize,
    n_rows: usize,
    k_dim: usize,
}

#[derive(Debug)]
struct ShadowXReassembly {
    job_id: u64,
    total_bytes: usize,
    received_bytes: usize,
    chunks: usize,
    complete: bool,
    data: Vec<u8>,
}

#[derive(Copy, Clone, Debug)]
struct ShadowXUpdate {
    received_bytes: usize,
    total_bytes: usize,
    chunks: usize,
    complete: bool,
    checksum: u64,
}

fn parse_lumen_matvec_xchunk(line: &str) -> Option<LumenMatvecXChunk<'_>> {
    let mut job_id = None;
    let mut chunk_index = None;
    let mut offset = None;
    let mut bytes = None;
    let mut total = None;
    let mut hex = None;
    for part in line.split_ascii_whitespace() {
        if let Some(value) = parse_u64_field(part, "job=") {
            job_id = Some(value);
        } else if let Some(value) = parse_usize_field(part, "chunk=") {
            chunk_index = Some(value);
        } else if let Some(value) = parse_usize_field(part, "offset=") {
            offset = Some(value);
        } else if let Some(value) = parse_usize_field(part, "bytes=") {
            bytes = Some(value);
        } else if let Some(value) = parse_usize_field(part, "total=") {
            total = Some(value);
        } else if let Some(value) = part.strip_prefix("hex=") {
            hex = Some(value);
        }
    }
    Some(LumenMatvecXChunk {
        job_id: job_id?,
        chunk_index: chunk_index?,
        offset: offset?,
        bytes: bytes?,
        total: total?,
        hex: hex?,
    })
}

fn parse_lumen_matvec_result_chunk(line: &str) -> Option<LumenMatvecResultChunk<'_>> {
    let mut job_id = None;
    let mut chunk_index = None;
    let mut offset = None;
    let mut bytes = None;
    let mut total = None;
    let mut hex = None;
    for part in line.split_ascii_whitespace() {
        if let Some(value) = parse_u64_field(part, "job=") {
            job_id = Some(value);
        } else if let Some(value) = parse_usize_field(part, "chunk=") {
            chunk_index = Some(value);
        } else if let Some(value) = parse_usize_field(part, "offset=") {
            offset = Some(value);
        } else if let Some(value) = parse_usize_field(part, "bytes=") {
            bytes = Some(value);
        } else if let Some(value) = parse_usize_field(part, "total=") {
            total = Some(value);
        } else if let Some(value) = part.strip_prefix("hex=") {
            hex = Some(value);
        }
    }
    Some(LumenMatvecResultChunk {
        job_id: job_id?,
        chunk_index: chunk_index?,
        offset: offset?,
        bytes: bytes?,
        total: total?,
        hex: hex?,
    })
}

fn parse_lumen_matvec_shadow_descriptor(line: &str) -> Option<LumenMatvecShadowDescriptor> {
    let mut job_id = None;
    let mut matrix_id = None;
    let mut row_start = None;
    let mut row_end = None;
    let mut n_rows = None;
    let mut k_dim = None;
    for part in line.split_ascii_whitespace() {
        if let Some(value) = parse_u64_field(part, "job=") {
            job_id = Some(value);
        } else if let Some(value) = parse_u64_field(part, "matrix=") {
            matrix_id = Some(value);
        } else if let Some(value) = part.strip_prefix("rows=") {
            let (start, end) = value.split_once("..")?;
            row_start = start.parse::<usize>().ok();
            row_end = end.parse::<usize>().ok();
        } else if let Some(value) = parse_usize_field(part, "n_rows=") {
            n_rows = Some(value);
        } else if let Some(value) = parse_usize_field(part, "k_dim=") {
            k_dim = Some(value);
        }
    }
    Some(LumenMatvecShadowDescriptor {
        job_id: job_id?,
        matrix_id: matrix_id?,
        row_start: row_start?,
        row_end: row_end?,
        n_rows: n_rows?,
        k_dim: k_dim?,
    })
}

fn record_shadow_x_chunk(
    reassemblies: &mut Vec<ShadowXReassembly>,
    chunk: LumenMatvecXChunk<'_>,
) -> Option<ShadowXUpdate> {
    if chunk.bytes == 0
        || chunk.total == 0
        || chunk.offset.saturating_add(chunk.bytes) > chunk.total
        || chunk.hex.len() < chunk.bytes.saturating_mul(2)
    {
        return None;
    }

    let index = if let Some(index) = reassemblies
        .iter()
        .position(|entry| entry.job_id == chunk.job_id)
    {
        index
    } else {
        reassemblies.push(ShadowXReassembly {
            job_id: chunk.job_id,
            total_bytes: chunk.total,
            received_bytes: 0,
            chunks: 0,
            complete: false,
            data: {
                let mut data = Vec::new();
                data.resize(chunk.total, 0);
                data
            },
        });
        reassemblies.len().saturating_sub(1)
    };

    let entry = &mut reassemblies[index];
    if entry.total_bytes != chunk.total || entry.data.len() != chunk.total {
        return None;
    }

    let hex = chunk.hex.as_bytes();
    for i in 0..chunk.bytes {
        let hi = hex_nibble(hex[i.saturating_mul(2)])?;
        let lo = hex_nibble(hex[i.saturating_mul(2).saturating_add(1)])?;
        entry.data[chunk.offset + i] = (hi << 4) | lo;
    }
    entry.received_bytes = entry
        .received_bytes
        .saturating_add(chunk.bytes)
        .min(chunk.total);
    entry.chunks = entry.chunks.saturating_add(1);
    if entry.received_bytes >= entry.total_bytes {
        entry.complete = true;
    }

    Some(ShadowXUpdate {
        received_bytes: entry.received_bytes,
        total_bytes: entry.total_bytes,
        chunks: entry.chunks,
        complete: entry.complete,
        checksum: if entry.complete {
            fnv1a64(entry.data.as_slice())
        } else {
            0
        },
    })
}

fn record_shadow_descriptor(
    descriptors: &mut Vec<LumenMatvecShadowDescriptor>,
    descriptor: LumenMatvecShadowDescriptor,
) {
    if let Some(existing) = descriptors
        .iter_mut()
        .find(|item| item.job_id == descriptor.job_id)
    {
        *existing = descriptor;
        return;
    }
    descriptors.push(descriptor);
}

fn shadow_descriptor_for_job(
    descriptors: &[LumenMatvecShadowDescriptor],
    job_id: u64,
) -> Option<LumenMatvecShadowDescriptor> {
    descriptors
        .iter()
        .copied()
        .find(|item| item.job_id == job_id)
}

fn shadow_x_data_for_job(reassemblies: &[ShadowXReassembly], job_id: u64) -> Option<&[u8]> {
    reassemblies
        .iter()
        .find(|entry| entry.job_id == job_id && entry.complete)
        .map(|entry| entry.data.as_slice())
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn decode_hex_exact(hex: &str, bytes: usize) -> Option<Vec<u8>> {
    if hex.len() < bytes.saturating_mul(2) {
        return None;
    }
    let hex = hex.as_bytes();
    let mut out = Vec::with_capacity(bytes);
    for i in 0..bytes {
        let hi = hex_nibble(hex[i.saturating_mul(2)])?;
        let lo = hex_nibble(hex[i.saturating_mul(2).saturating_add(1)])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn append_hex(out: &mut Vec<u8>, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize]);
        out.push(HEX[(byte & 0x0F) as usize]);
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

fn process_lumen_app_text_payload(
    vnet: &VNet,
    handle: v::vnet::NetHandle,
    payload: &[u8],
    shadow_descriptors: &mut Vec<LumenMatvecShadowDescriptor>,
    shadow_x_reassemblies: &mut Vec<ShadowXReassembly>,
) {
    if let Some(line) = trueos_lumen_matvec_shadow_received(payload) {
        crate::log!(
            "esp-gate: lumen matvec shadow received handle={} bytes={} {}\n",
            handle.0,
            payload.len(),
            line
        );
        if let Some(descriptor) = parse_lumen_matvec_shadow_descriptor(line) {
            record_shadow_descriptor(shadow_descriptors, descriptor);
        }
    }
    let Ok(text) = core::str::from_utf8(payload) else {
        return;
    };
    for line in text.lines().filter(|line| {
        line.trim_start()
            .starts_with(TRUEOS_LUMEN_MATVEC_RESULT_CHUNK_PREFIX)
    }) {
        if let Some(chunk) = parse_lumen_matvec_result_chunk(line)
            && let Some(bytes) = decode_hex_exact(chunk.hex, chunk.bytes)
            && let Some(update) = crate::lumen::lumen_net::record_remote_bf16_matvec_result_chunk(
                chunk.job_id,
                chunk.offset,
                chunk.total,
                bytes.as_slice(),
            )
        {
            crate::log!(
                "esp-gate: lumen matvec result chunk received handle={} job={} chunk={} offset={} bytes={} total={} got={} chunks={} complete={} copied_rows={} checksum=0x{:016X}\n",
                handle.0,
                chunk.job_id,
                chunk.chunk_index,
                chunk.offset,
                chunk.bytes,
                chunk.total,
                update.received_bytes,
                update.chunks,
                if update.complete { 1 } else { 0 },
                update.copied_rows,
                update.checksum
            );
        }
    }
    for line in text.lines().filter(|line| {
        line.trim_start()
            .starts_with(TRUEOS_LUMEN_MATVEC_XCHUNK_PREFIX)
    }) {
        if let Some(chunk) = parse_lumen_matvec_xchunk(line) {
            if let Some(update) = record_shadow_x_chunk(shadow_x_reassemblies, chunk) {
                crate::log!(
                    "esp-gate: lumen matvec xchunk received handle={} job={} chunk={} offset={} bytes={} total={} got={} chunks={} complete={}\n",
                    handle.0,
                    chunk.job_id,
                    chunk.chunk_index,
                    chunk.offset,
                    chunk.bytes,
                    chunk.total,
                    update.received_bytes,
                    update.chunks,
                    if update.complete { 1 } else { 0 }
                );
                if update.complete {
                    if let Some(descriptor) =
                        shadow_descriptor_for_job(shadow_descriptors.as_slice(), chunk.job_id)
                        && let Some(x_bytes) =
                            shadow_x_data_for_job(shadow_x_reassemblies.as_slice(), chunk.job_id)
                        && let Some((proof, result_bytes)) =
                            crate::lumen::lumen_net::compute_shadow_bf16_matvec_result_bytes(
                                descriptor.matrix_id,
                                descriptor.row_start,
                                descriptor.row_end.saturating_sub(descriptor.row_start),
                                descriptor.n_rows,
                                descriptor.k_dim,
                                x_bytes,
                                crate::allcaps::lumen::NET_BF16_MATVEC_SHADOW_COMPUTE_PROOF_ROWS,
                            )
                    {
                        let result_frames = submit_trueos_lumen_result_frames(
                            vnet,
                            handle,
                            chunk.job_id,
                            result_bytes.as_slice(),
                        );
                        crate::log!(
                            "esp-gate: lumen matvec x reassembled job={} bytes={} chunks={} checksum=0x{:016X} proof=remote-bf16-matvec rows={} matrix=0x{:016X} result_checksum=0x{:016X} first={:.6} last={:.6} result_bytes={} result_frames={}\n",
                            chunk.job_id,
                            update.total_bytes,
                            update.chunks,
                            update.checksum,
                            proof.rows,
                            descriptor.matrix_id,
                            proof.checksum,
                            proof.first,
                            proof.last,
                            result_bytes.len(),
                            result_frames
                        );
                    } else {
                        crate::log!(
                            "esp-gate: lumen matvec x reassembled job={} bytes={} chunks={} checksum=0x{:016X} proof=unavailable\n",
                            chunk.job_id,
                            update.total_bytes,
                            update.chunks,
                            update.checksum
                        );
                    }
                }
            } else {
                crate::log!(
                    "esp-gate: lumen matvec xchunk rejected handle={} payload_bytes={}\n",
                    handle.0,
                    payload.len()
                );
            }
        }
    }
}

fn submit_trueos_lumen_work_capacity(vnet: &VNet, handle: v::vnet::NetHandle) {
    let capacity = crate::lumen::lumen_service::remote_work_capacity();
    let telemetry = crate::lumen::lumen_net::backend_telemetry(capacity);
    let reply = format!(
        "{} LUMEN_WORK_CAP v=1 n={} proto={} caps=0x{:08X} workers={} pending={} min_rows={}\n",
        trueos_esp::gate::TRUEOS_SWARM_MAGIC_TEXT,
        telemetry.capacity_lanes,
        telemetry.protocol_version,
        telemetry.caps,
        telemetry.local_workers,
        telemetry.pending_bf16_matvecs,
        telemetry.min_remote_rows
    );
    let bytes = reply.as_bytes();
    let len = bytes.len().min(TRUEOS_LUMEN_WORK_FRAME_MAX);
    let _ = vnet.submit(v::vnet::Command::SendTcp {
        handle,
        data: v::vnet::ByteBuf::from_slice_trunc(&bytes[..len]),
    });
    crate::log!(
        "esp-gate: lumen work capacity reply handle={} n={} proto={} caps=0x{:08X} workers={} pending={} min_rows={} online={} running={}\n",
        handle.0,
        telemetry.capacity_lanes,
        telemetry.protocol_version,
        telemetry.caps,
        telemetry.local_workers,
        telemetry.pending_bf16_matvecs,
        telemetry.min_remote_rows,
        crate::lumen::lumen_service::is_online(),
        crate::lumen::lumen_service::is_prompt_running()
    );
}

fn submit_trueos_lumen_work_probe(vnet: &VNet, handle: v::vnet::NetHandle) {
    let probe = format!(
        "{} LUMEN_CAN_TAKE_WORK v=1 timeout_ms={}\n",
        trueos_esp::gate::TRUEOS_SWARM_MAGIC_TEXT,
        TRUEOS_LUMEN_WORK_PROBE_TIMEOUT_MS
    );
    let bytes = probe.as_bytes();
    let len = bytes.len().min(TRUEOS_LUMEN_WORK_FRAME_MAX);
    let _ = vnet.submit(v::vnet::Command::SendTcp {
        handle,
        data: v::vnet::ByteBuf::from_slice_trunc(&bytes[..len]),
    });
}

fn submit_trueos_lumen_work_probe_to_all(vnet: &VNet, links: &[TrueOsPeerLink]) -> usize {
    let mut sent = 0usize;
    let mut seen_nodes = Vec::new();
    for link in links.iter().filter(|link| link.handle.is_some()) {
        if link.node_id != 0 {
            if seen_nodes.contains(&link.node_id) {
                continue;
            }
            seen_nodes.push(link.node_id);
        }
        if let Some(handle) = link.handle {
            submit_trueos_lumen_work_probe(vnet, handle);
            sent = sent.saturating_add(1);
        }
    }
    sent
}

fn submit_trueos_lumen_shadow_frame_to_first_peer(
    vnet: &VNet,
    links: &[TrueOsPeerLink],
    payload: &[u8],
) -> bool {
    let Some(handle) = links.iter().find_map(|link| link.handle) else {
        return false;
    };
    let Some(frame) = encode_lumen_app_text_frame(payload) else {
        return false;
    };
    if frame.len() > v::vnet::MAX_MSG {
        return false;
    }
    let _ = vnet.submit(v::vnet::Command::SendTcp {
        handle,
        data: v::vnet::ByteBuf::from_slice_trunc(frame.as_slice()),
    });
    crate::log!(
        "esp-gate: lumen app frame sent handle={} opcode={} payload_bytes={} wire_bytes={} pending={}\n",
        handle.0,
        TRUEOS_LUMEN_APP_OP_TEXT,
        payload.len(),
        frame.len(),
        crate::lumen::lumen_net::pending_shadow_bf16_matvecs()
    );
    true
}

fn encode_lumen_matvec_result_chunk_frame(
    job_id: u64,
    chunk_index: usize,
    offset: usize,
    total_bytes: usize,
    chunk: &[u8],
) -> Vec<u8> {
    let mut out = format!(
        "C0DEC0DE LUMEN_MATVEC_RESULT_CHUNK v=1 job={} chunk={} offset={} bytes={} total={} hex=",
        job_id,
        chunk_index,
        offset,
        chunk.len(),
        total_bytes,
    )
    .into_bytes();
    append_hex(&mut out, chunk);
    out.push(b'\n');
    out
}

fn submit_trueos_lumen_result_frames(
    vnet: &VNet,
    handle: v::vnet::NetHandle,
    job_id: u64,
    result_bytes: &[u8],
) -> usize {
    let chunk_bytes = crate::allcaps::lumen::NET_BF16_MATVEC_RESULT_CHUNK_BYTES.max(1);
    let mut sent = 0usize;
    for (chunk_index, offset) in (0..result_bytes.len()).step_by(chunk_bytes).enumerate() {
        let end = offset.saturating_add(chunk_bytes).min(result_bytes.len());
        let payload = encode_lumen_matvec_result_chunk_frame(
            job_id,
            chunk_index,
            offset,
            result_bytes.len(),
            &result_bytes[offset..end],
        );
        let Some(frame) = encode_lumen_app_text_frame(payload.as_slice()) else {
            continue;
        };
        if frame.len() > v::vnet::MAX_MSG {
            crate::log!(
                "esp-gate: lumen matvec result send skipped job={} chunk={} reason=frame-too-large wire_bytes={}\n",
                job_id,
                chunk_index,
                frame.len()
            );
            continue;
        }
        let _ = vnet.submit(v::vnet::Command::SendTcp {
            handle,
            data: v::vnet::ByteBuf::from_slice_trunc(frame.as_slice()),
        });
        sent = sent.saturating_add(1);
    }
    sent
}

fn advertise_trueos_peer(vnet: &VNet, udp_handle: v::vnet::NetHandle, node_id: u64) {
    let hello = trueos_peer_hello(node_id);
    let bytes = hello.as_bytes();
    let len = bytes.len().min(TRUEOS_PEER_HELLO_MAX);
    let _ = vnet.submit(v::vnet::Command::SendUdp {
        handle: udp_handle,
        remote: v::vnet::EndpointV4::new(
            [255, 255, 255, 255],
            trueos_esp::gate::ESP_UDP_BROADCAST_PORT,
        ),
        data: v::vnet::ByteBuf::from_slice_trunc(&bytes[..len]),
    });
}

struct TrueOsPeerLink {
    handle: Option<v::vnet::NetHandle>,
    node_id: u64,
    rx: Vec<u8>,
    lumen_rx: Vec<u8>,
}

fn allocate_trueos_peer_links() -> Vec<TrueOsPeerLink> {
    let mut links = Vec::with_capacity(TRUEOS_PEER_LINK_CAP);
    for _ in 0..TRUEOS_PEER_LINK_CAP {
        links.push(TrueOsPeerLink {
            handle: None,
            node_id: 0,
            rx: Vec::with_capacity(TRUEOS_PEER_RX_BUF_BYTES),
            lumen_rx: Vec::with_capacity(v::vnet::MAX_MSG.saturating_mul(2)),
        });
    }
    links
}

fn trueos_peer_link_count(links: &[TrueOsPeerLink]) -> usize {
    links.iter().filter(|link| link.handle.is_some()).count()
}

fn trueos_peer_link_node_id(links: &[TrueOsPeerLink], handle: v::vnet::NetHandle) -> u64 {
    links
        .iter()
        .find(|link| link.handle == Some(handle))
        .map(|link| link.node_id)
        .unwrap_or(0)
}

fn trueos_peer_link_known(links: &[TrueOsPeerLink], node_id: u64) -> bool {
    node_id != 0
        && links
            .iter()
            .any(|link| link.handle.is_some() && link.node_id == node_id)
}

fn trueos_peer_duplicate_handle(
    links: &[TrueOsPeerLink],
    node_id: u64,
    current: v::vnet::NetHandle,
) -> Option<v::vnet::NetHandle> {
    if node_id == 0 {
        return None;
    }

    links
        .iter()
        .filter(|link| link.node_id == node_id)
        .filter_map(|link| link.handle)
        .find(|handle| *handle != current)
}

fn trueos_peer_link_has_room_or_known(links: &[TrueOsPeerLink], node_id: u64) -> bool {
    trueos_peer_link_known(links, node_id) || trueos_peer_link_count(links) < TRUEOS_PEER_LINK_CAP
}

fn ensure_trueos_peer_link(links: &mut [TrueOsPeerLink], handle: v::vnet::NetHandle) -> bool {
    if links.iter().any(|link| link.handle == Some(handle)) {
        return true;
    }

    let Some(link) = links.iter_mut().find(|link| link.handle.is_none()) else {
        return false;
    };

    link.handle = Some(handle);
    link.node_id = 0;
    link.rx.clear();
    link.lumen_rx.clear();
    true
}

fn remove_trueos_peer_link(links: &mut [TrueOsPeerLink], handle: v::vnet::NetHandle) -> bool {
    let Some(link) = links.iter_mut().find(|link| link.handle == Some(handle)) else {
        return false;
    };

    link.handle = None;
    link.node_id = 0;
    link.rx.clear();
    link.lumen_rx.clear();
    true
}

fn clear_trueos_peer_links(links: &mut [TrueOsPeerLink]) -> usize {
    let mut cleared = 0usize;
    for link in links.iter_mut().filter(|link| link.handle.is_some()) {
        link.handle = None;
        link.node_id = 0;
        link.rx.clear();
        link.lumen_rx.clear();
        cleared = cleared.saturating_add(1);
    }
    cleared
}

fn drain_lumen_app_data_for_handle<F>(
    links: &mut [TrueOsPeerLink],
    handle: v::vnet::NetHandle,
    data: &[u8],
    mut on_frame: F,
) where
    F: FnMut(LumenAppFrame<'_>),
{
    let Some(link) = links.iter_mut().find(|link| link.handle == Some(handle)) else {
        return;
    };
    if link.lumen_rx.len().saturating_add(data.len()) > TRUEOS_LUMEN_APP_RX_BUF_BYTES {
        link.lumen_rx.clear();
        crate::log!(
            "esp-gate: lumen app rx reset handle={} reason=overflow cap={}\n",
            handle.0,
            TRUEOS_LUMEN_APP_RX_BUF_BYTES
        );
    }
    link.lumen_rx.extend_from_slice(data);
    drain_lumen_app_frames(&mut link.lumen_rx, |frame| on_frame(frame));
}

fn record_trueos_peer_data(
    links: &mut [TrueOsPeerLink],
    handle: v::vnet::NetHandle,
    data: &[u8],
) -> Option<trueos_esp::gate::TrueOsHostAdvertisement> {
    let link = links.iter_mut().find(|link| link.handle == Some(handle))?;
    if link.node_id != 0 {
        return None;
    }

    if link.rx.len().saturating_add(data.len()) > TRUEOS_PEER_RX_BUF_BYTES {
        link.rx.clear();
    }
    let room = TRUEOS_PEER_RX_BUF_BYTES.saturating_sub(link.rx.len());
    link.rx.extend_from_slice(&data[..data.len().min(room)]);

    let advertisement = trueos_esp::gate::parse_trueos_host_advertisement(
        v::vnet::EndpointV4::new([0, 0, 0, 0], 0),
        link.rx.as_slice(),
    )?;
    link.node_id = advertisement.node_id;
    Some(advertisement)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EspControlError {
    DeviceMissing,
    DeviceUnreachable,
    UploadFailed,
    RunFailed,
    RestartFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EspRestartResult {
    pub restart_requested: bool,
    pub removed_from_registry: bool,
}

fn manual_endpoint_url(snapshot: &trueos_esp::gate::DeviceSnapshot, path: &str) -> Option<String> {
    if snapshot.class != trueos_esp::gate::DeviceClass::EspUploader {
        return None;
    }

    let path = path.strip_prefix('/').unwrap_or(path);
    match snapshot.ip {
        Some(trueos_esp::gate::DeviceIp::V4(addr)) => Some(format!(
            "http://{}.{}.{}.{}:{}/{}",
            addr[0], addr[1], addr[2], addr[3], snapshot.service_port, path
        )),
        Some(trueos_esp::gate::DeviceIp::V6(_)) | None => None,
    }
}

#[allow(dead_code)]
pub async fn upload_app_to_device(
    handle: v::vnet::NetHandle,
    source_name: &str,
    body: &[u8],
    target_name: &str,
) -> Result<(), EspControlError> {
    let Some(snapshot) = snapshot_for_handle(handle) else {
        return Err(EspControlError::DeviceMissing);
    };
    let iface = trueos_esp::swarm::DeviceInterface::from_snapshot(&snapshot);
    let Some(upload_url) = iface.upload_url() else {
        return Err(EspControlError::DeviceUnreachable);
    };
    crate::log!(
        "esp-gate: manual upload handle={} source={} target={} bytes={}\n",
        handle.0,
        source_name,
        target_name,
        body.len()
    );
    crate::t::net::http::post_http_body_hyper_with_headers(
        upload_url.as_str(),
        "application/octet-stream",
        &[("X-Filename", target_name)],
        body,
        ESP_CONTROL_TIMEOUT_MS,
        ESP_CONTROL_MAX_RX,
    )
    .await
    .map_err(|_| EspControlError::UploadFailed)?;

    let Some(run_url) = iface.run_url() else {
        return Err(EspControlError::DeviceUnreachable);
    };
    crate::t::net::http::post_http_body_hyper(
        run_url.as_str(),
        "",
        &[],
        ESP_CONTROL_TIMEOUT_MS,
        ESP_CONTROL_MAX_RX,
    )
    .await
    .map_err(|_| EspControlError::RunFailed)?;

    Ok(())
}

#[allow(dead_code)]
pub async fn restart_device(
    handle: v::vnet::NetHandle,
) -> Result<EspRestartResult, EspControlError> {
    let Some(snapshot) = snapshot_for_handle(handle) else {
        return Err(EspControlError::DeviceMissing);
    };

    let Some(url) = manual_endpoint_url(&snapshot, trueos_esp::swarm::ESP_RESTART_PATH) else {
        return Err(EspControlError::DeviceUnreachable);
    };
    let restart_requested = crate::t::net::http::post_http_body_hyper(
        url.as_str(),
        "",
        &[],
        ESP_CONTROL_TIMEOUT_MS,
        ESP_CONTROL_MAX_RX,
    )
    .await
    .is_ok();

    let removed_from_registry = remove_device(handle);
    if restart_requested {
        Ok(EspRestartResult {
            restart_requested,
            removed_from_registry,
        })
    } else if removed_from_registry {
        Ok(EspRestartResult {
            restart_requested,
            removed_from_registry,
        })
    } else {
        Err(EspControlError::RestartFailed)
    }
}

async fn poll_device_status(snapshot: &trueos_esp::gate::DeviceSnapshot) {
    let iface = trueos_esp::swarm::DeviceInterface::from_snapshot(snapshot);
    let Some(url) = iface.status_url() else {
        return;
    };

    let url_string = String::from(url.as_str());
    match crate::t::run_on_shared_tokio(move || async move {
        crate::t::net::http::fetch_http_body_hyper(
            url_string.as_str(),
            ESP_STATUS_FETCH_TIMEOUT_MS,
            ESP_STATUS_FETCH_MAX_RX,
        )
        .await
    })
    .await
    {
        Ok(Ok(body)) => {
            if let Some(status) = trueos_esp::swarm::parse_status_snapshot(body.as_slice()) {
                let now_ms = monotonic_ms();
                let event =
                    DEVICE_REGISTRY
                        .lock()
                        .update_status(snapshot.handle, status.clone(), now_ms);
                if let Some(event) = event {
                    crate::log!(
                        "esp-gate: status changed handle={} running={} last_status={} last_error={}\n",
                        event.handle.0,
                        if event.current.running { 1 } else { 0 },
                        event.current.last_status.as_str(),
                        event.current.last_error.as_str()
                    );
                    note_registry_change();
                    STATUS_EVENTS.lock().push_back(event);
                }
            } else {
                crate::log!(
                    "esp-gate: status parse failed handle={} url={} bytes={}\n",
                    snapshot.handle.0,
                    url.as_str(),
                    body.len()
                );
            }
        }
        Ok(Err(err)) => {
            crate::log!(
                "esp-gate: status fetch failed handle={} url={} timeout_ms={} err={:?}\n",
                snapshot.handle.0,
                url.as_str(),
                ESP_STATUS_FETCH_TIMEOUT_MS,
                err
            );
        }
        Err(err) => {
            crate::log!(
                "esp-gate: status fetch shared-tokio failed handle={} url={} err={:?}\n",
                snapshot.handle.0,
                url.as_str(),
                err
            );
        }
    }
}

#[embassy_executor::task]
pub async fn esp_gate_task() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;

    loop {
        let Some(vnet) = VNet::open_primary() else {
            Timer::after(Duration::from_millis(100)).await;
            continue;
        };

        let mut gate = trueos_esp::gate::GateDiscovery::new();
        let local_node_id = trueos_node_id(&vnet);
        let mut udp_handle: Option<v::vnet::NetHandle> = None;
        let mut peer_listener: Option<v::vnet::NetHandle> = None;
        let mut peer_links = allocate_trueos_peer_links();
        let mut next_peer_advertise_ms = 0u64;
        let mut seen_lumen_work_probe_seq = LUMEN_WORK_PROBE_SEQ.load(Ordering::Acquire);
        let mut lumen_work_probe_seq = seen_lumen_work_probe_seq;
        let mut lumen_work_probe_deadline_ms = 0u64;
        let mut lumen_work_probe_sent = 0usize;
        let mut lumen_work_probe_replies = 0usize;
        let mut lumen_work_probe_best = 0u32;
        let mut lumen_work_probe_seen_nodes: Vec<u64> = Vec::new();
        let mut shadow_descriptors: Vec<LumenMatvecShadowDescriptor> = Vec::new();
        let mut shadow_x_reassemblies: Vec<ShadowXReassembly> = Vec::new();
        let _ = vnet.submit(gate.bootstrap_command());
        let _ = vnet.submit(v::vnet::Command::OpenTcpListen {
            port: trueos_esp::gate::TRUEOS_PEER_TCP_PORT,
        });
        crate::log!(
            "esp-gate: starting udp swarm listener on port {} payload=swarm trueos_magic={} peer_tcp={} node=0x{:016X} peer_slots={} rx_buf_bytes={}\n",
            trueos_esp::gate::ESP_UDP_BROADCAST_PORT,
            trueos_esp::gate::TRUEOS_SWARM_MAGIC_TEXT,
            trueos_esp::gate::TRUEOS_PEER_TCP_PORT,
            local_node_id,
            TRUEOS_PEER_LINK_CAP,
            TRUEOS_PEER_RX_BUF_BYTES
        );

        loop {
            if let Some(ev) = vnet.pop_event() {
                if let v::vnet::Event::Error { msg } = ev {
                    crate::log!("esp-gate: error {}\n", msg);
                    if msg == "bad handle" {
                        let cleared = clear_trueos_peer_links(peer_links.as_mut_slice());
                        crate::lumen::lumen_net::set_remote_bf16_route_available(false);
                        crate::log!(
                            "esp-gate: peer links cleared reason=bad-handle count={}\n",
                            cleared
                        );
                    }
                }

                match ev {
                    v::vnet::Event::Opened {
                        handle,
                        kind: v::vnet::SocketKind::Tcp,
                    } if peer_listener.is_none() => {
                        peer_listener = Some(handle);
                        crate::log!(
                            "esp-gate: trueos peer tcp listener bound handle={} port={}\n",
                            handle.0,
                            trueos_esp::gate::TRUEOS_PEER_TCP_PORT
                        );
                    }
                    v::vnet::Event::TcpEstablished { handle, .. } => {
                        if !ensure_trueos_peer_link(peer_links.as_mut_slice(), handle) {
                            crate::log!(
                                "esp-gate: trueos peer tcp rejected handle={} reason=peer-slot-cap cap={}\n",
                                handle.0,
                                TRUEOS_PEER_LINK_CAP
                            );
                            let _ = vnet.submit(v::vnet::Command::Close { handle });
                            continue;
                        }

                        crate::log!(
                            "esp-gate: trueos peer tcp established handle={} active_links={} sending hello\n",
                            handle.0,
                            trueos_peer_link_count(peer_links.as_slice())
                        );
                        submit_trueos_peer_hello(&vnet, handle, local_node_id);
                    }
                    v::vnet::Event::TcpData { handle, data } => {
                        let mut close_duplicate_peer = false;
                        if let Some(advertisement) = record_trueos_peer_data(
                            peer_links.as_mut_slice(),
                            handle,
                            data.as_slice(),
                        ) {
                            crate::log!(
                                "esp-gate: trueos peer hello received handle={} node=0x{:016X} bytes={}\n",
                                handle.0,
                                advertisement.node_id,
                                data.len()
                            );
                            if let Some(retained) = trueos_peer_duplicate_handle(
                                peer_links.as_slice(),
                                advertisement.node_id,
                                handle,
                            ) {
                                crate::log!(
                                    "esp-gate: trueos peer duplicate handle={} retained_handle={} node=0x{:016X} action=close-duplicate\n",
                                    handle.0,
                                    retained.0,
                                    advertisement.node_id
                                );
                                let _ = remove_trueos_peer_link(peer_links.as_mut_slice(), handle);
                                let _ = vnet.submit(v::vnet::Command::Close { handle });
                                close_duplicate_peer = true;
                            }
                        }
                        if !close_duplicate_peer {
                            if trueos_lumen_work_probe_received(data.as_slice()) {
                                submit_trueos_lumen_work_capacity(&vnet, handle);
                            }
                            drain_lumen_app_data_for_handle(
                                peer_links.as_mut_slice(),
                                handle,
                                data.as_slice(),
                                |frame| {
                                    if frame.opcode == TRUEOS_LUMEN_APP_OP_TEXT {
                                        process_lumen_app_text_payload(
                                            &vnet,
                                            handle,
                                            frame.payload,
                                            &mut shadow_descriptors,
                                            &mut shadow_x_reassemblies,
                                        );
                                    } else {
                                        crate::log!(
                                            "esp-gate: lumen app frame ignored handle={} opcode={} bytes={}\n",
                                            handle.0,
                                            frame.opcode,
                                            frame.payload.len()
                                        );
                                    }
                                },
                            );
                            if let Some(capacity) =
                                parse_trueos_lumen_work_capacity(data.as_slice())
                            {
                                let now_ms = monotonic_ms();
                                let node_id =
                                    trueos_peer_link_node_id(peer_links.as_slice(), handle);
                                let mut counted = false;
                                if lumen_work_probe_deadline_ms != 0
                                    && now_ms <= lumen_work_probe_deadline_ms
                                {
                                    if node_id == 0
                                        || !lumen_work_probe_seen_nodes.contains(&node_id)
                                    {
                                        if node_id != 0 {
                                            lumen_work_probe_seen_nodes.push(node_id);
                                        }
                                        lumen_work_probe_replies =
                                            lumen_work_probe_replies.saturating_add(1);
                                        lumen_work_probe_best =
                                            lumen_work_probe_best.max(capacity.lanes);
                                        counted = true;
                                    }
                                    if counted && lumen_work_probe_best != 0 {
                                        publish_lumen_work_probe_result(
                                            lumen_work_probe_seq,
                                            lumen_work_probe_sent,
                                            lumen_work_probe_replies,
                                            lumen_work_probe_best,
                                        );
                                    }
                                }
                                crate::log!(
                                    "esp-gate: lumen work capacity received handle={} node=0x{:016X} n={} proto={} caps=0x{:08X} workers={} pending={} min_rows={} counted={}\n",
                                    handle.0,
                                    node_id,
                                    capacity.lanes,
                                    capacity.protocol_version,
                                    capacity.caps,
                                    capacity.workers,
                                    capacity.pending,
                                    capacity.min_rows,
                                    if counted { 1 } else { 0 }
                                );
                            }
                        }
                    }
                    v::vnet::Event::Closed { handle } if peer_listener == Some(handle) => {
                        peer_listener = None;
                        let retained_peer_node =
                            trueos_peer_link_node_id(peer_links.as_slice(), handle);
                        let _ = remove_trueos_peer_link(peer_links.as_mut_slice(), handle);
                        crate::log!(
                            "esp-gate: trueos peer tcp listener closed, reopening port={} retained_peer_node=0x{:016X} active_links={}\n",
                            trueos_esp::gate::TRUEOS_PEER_TCP_PORT,
                            retained_peer_node,
                            trueos_peer_link_count(peer_links.as_slice())
                        );
                        let _ = vnet.submit(v::vnet::Command::OpenTcpListen {
                            port: trueos_esp::gate::TRUEOS_PEER_TCP_PORT,
                        });
                    }
                    v::vnet::Event::Closed { handle } => {
                        if remove_trueos_peer_link(peer_links.as_mut_slice(), handle) {
                            if trueos_peer_link_count(peer_links.as_slice()) == 0 {
                                crate::lumen::lumen_net::set_remote_bf16_route_available(false);
                            }
                            crate::log!(
                                "esp-gate: trueos peer tcp closed handle={} active_links={}\n",
                                handle.0,
                                trueos_peer_link_count(peer_links.as_slice())
                            );
                        }
                    }
                    _ => {}
                }

                match gate.on_event(ev) {
                    trueos_esp::gate::GateAction::None => {}
                    trueos_esp::gate::GateAction::Signal(signal) => match signal {
                        trueos_esp::gate::GateSignal::UdpBound(handle) => {
                            udp_handle = Some(handle);
                            next_peer_advertise_ms = 0;
                            crate::log!(
                                "esp-gate: udp listener bound handle={} port={}\n",
                                handle.0,
                                trueos_esp::gate::ESP_UDP_BROADCAST_PORT
                            );
                        }
                        trueos_esp::gate::GateSignal::EspDiscovered(from) => {
                            crate::globalog::log_with_level(
                                log::Level::Trace,
                                format_args!(
                                    "esp-gate: heartbeat=swarm from {}.{}.{}.{} upload_port={}\n",
                                    from.addr[0],
                                    from.addr[1],
                                    from.addr[2],
                                    from.addr[3],
                                    trueos_esp::gate::ESP_HTTP_UPLOAD_PORT
                                ),
                            );

                            let now_ms = monotonic_ms();
                            let is_new = {
                                let mut registry = DEVICE_REGISTRY.lock();
                                registry.upsert_heartbeat_v4(
                                    from.addr,
                                    trueos_esp::gate::ESP_HTTP_UPLOAD_PORT,
                                    now_ms,
                                )
                            };
                            if is_new {
                                note_registry_change();
                            }
                        }
                        trueos_esp::gate::GateSignal::TrueOsHostDiscovered(advertisement) => {
                            if advertisement.node_id != 0 && advertisement.node_id == local_node_id
                            {
                                continue;
                            }

                            crate::log!(
                                "esp-gate: heartbeat={} from {}.{}.{}.{} peer_tcp={} node=0x{:016X} caps=0x{:08X}\n",
                                trueos_esp::gate::TRUEOS_SWARM_MAGIC_TEXT,
                                advertisement.from.addr[0],
                                advertisement.from.addr[1],
                                advertisement.from.addr[2],
                                advertisement.from.addr[3],
                                advertisement.peer_tcp_port,
                                advertisement.node_id,
                                advertisement.caps
                            );

                            let now_ms = monotonic_ms();
                            let is_new = {
                                let mut registry = DEVICE_REGISTRY.lock();
                                registry.upsert_trueos_host_v4(
                                    advertisement.from.addr,
                                    advertisement.peer_tcp_port,
                                    advertisement.node_id,
                                    advertisement.caps,
                                    now_ms,
                                )
                            };
                            if is_new {
                                note_registry_change();
                                if trueos_peer_link_has_room_or_known(
                                    peer_links.as_slice(),
                                    advertisement.node_id,
                                ) {
                                    let _ = vnet.submit(v::vnet::Command::OpenTcpConnect {
                                        remote: v::vnet::EndpointV4::new(
                                            advertisement.from.addr,
                                            advertisement.peer_tcp_port,
                                        ),
                                    });
                                } else {
                                    crate::log!(
                                        "esp-gate: trueos peer dial skipped node=0x{:016X} reason=peer-slot-cap cap={}\n",
                                        advertisement.node_id,
                                        TRUEOS_PEER_LINK_CAP
                                    );
                                }
                            }
                        }
                    },
                    trueos_esp::gate::GateAction::Submit(cmd) => {
                        let _ = vnet.submit(cmd);
                    }
                }

                continue;
            }

            let now_ms = monotonic_ms();
            if let Some(handle) = udp_handle
                && now_ms >= next_peer_advertise_ms
            {
                advertise_trueos_peer(&vnet, handle, local_node_id);
                next_peer_advertise_ms = now_ms.saturating_add(TRUEOS_PEER_ADVERTISE_MS);
            }

            let requested_lumen_work_probe_seq = LUMEN_WORK_PROBE_SEQ.load(Ordering::Acquire);
            if requested_lumen_work_probe_seq != seen_lumen_work_probe_seq {
                seen_lumen_work_probe_seq = requested_lumen_work_probe_seq;
                lumen_work_probe_seq = requested_lumen_work_probe_seq;
                lumen_work_probe_replies = 0;
                lumen_work_probe_best = 0;
                lumen_work_probe_seen_nodes.clear();
                lumen_work_probe_sent =
                    submit_trueos_lumen_work_probe_to_all(&vnet, peer_links.as_slice());
                if lumen_work_probe_sent == 0 {
                    lumen_work_probe_deadline_ms = 0;
                    crate::lumen::lumen_net::set_remote_bf16_route_available(false);
                    publish_lumen_work_probe_result(lumen_work_probe_seq, 0, 0, 0);
                    crate::log!(
                        "esp-gate: lumen work capacity probe seq={} skipped reason=no-peers\n",
                        lumen_work_probe_seq
                    );
                } else {
                    lumen_work_probe_deadline_ms =
                        now_ms.saturating_add(TRUEOS_LUMEN_WORK_PROBE_TIMEOUT_MS);
                    crate::log!(
                        "esp-gate: lumen work capacity probe seq={} sent={} timeout_ms={}\n",
                        lumen_work_probe_seq,
                        lumen_work_probe_sent,
                        TRUEOS_LUMEN_WORK_PROBE_TIMEOUT_MS
                    );
                }
            }

            if lumen_work_probe_deadline_ms != 0 && now_ms >= lumen_work_probe_deadline_ms {
                if lumen_work_probe_replies == 0 {
                    crate::lumen::lumen_net::set_remote_bf16_route_available(false);
                }
                publish_lumen_work_probe_result(
                    lumen_work_probe_seq,
                    lumen_work_probe_sent,
                    lumen_work_probe_replies,
                    lumen_work_probe_best,
                );
                crate::log!(
                    "esp-gate: lumen work capacity probe seq={} complete sent={} replies={} best={}\n",
                    lumen_work_probe_seq,
                    lumen_work_probe_sent,
                    lumen_work_probe_replies,
                    lumen_work_probe_best
                );
                lumen_work_probe_deadline_ms = 0;
            }

            if trueos_peer_link_count(peer_links.as_slice()) != 0 {
                if let Some(frame) = crate::lumen::lumen_net::take_shadow_bf16_matvec_frame()
                    && !submit_trueos_lumen_shadow_frame_to_first_peer(
                        &vnet,
                        peer_links.as_slice(),
                        frame.as_slice(),
                    )
                {
                    crate::log!(
                        "esp-gate: lumen matvec shadow send failed bytes={}\n",
                        frame.len()
                    );
                }
            }

            Timer::after(Duration::from_millis(10)).await;
        }
    }
}

#[embassy_executor::task]
pub async fn esp_gate_registry_task() {
    let mut heartbeat_tick = 0u32;
    let mut status_poll_index = 0usize;

    loop {
        let snapshot_to_poll = {
            let snapshots = DEVICE_REGISTRY.lock().snapshot();
            if snapshots.is_empty() {
                None
            } else {
                let idx = status_poll_index % snapshots.len();
                status_poll_index = status_poll_index.wrapping_add(1);
                Some(snapshots[idx].clone())
            }
        };

        if let Some(snapshot) = snapshot_to_poll.as_ref() {
            poll_device_status(snapshot).await;
        }

        heartbeat_tick = heartbeat_tick.wrapping_add(1);
        if heartbeat_tick >= 20 {
            heartbeat_tick = 0;
            let count = DEVICE_REGISTRY.lock().len();
            if count != 0 {
                crate::log!("esp-gate: registry active_devices={}\n", count);
            }
        }

        Timer::after(Duration::from_millis(ESP_STATUS_POLL_MS)).await;
    }
}

#[embassy_executor::task]
pub async fn esp_piano_udp_task() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;

    loop {
        let Some(vnet) = VNet::open_primary() else {
            Timer::after(Duration::from_millis(100)).await;
            continue;
        };

        let mut piano = trueos_esp::piano::PianoUdpReceiver::new();
        let _ = vnet.submit(piano.bootstrap_command());
        crate::log!(
            "esp-piano: starting udp listener port={} keys={}\n",
            trueos_esp::piano::TRUEOS_PIANO_UDP_PORT,
            trueos_esp::piano::PIANO_KEY_COUNT
        );

        loop {
            if let Some(ev) = vnet.pop_event() {
                match ev {
                    v::vnet::Event::Opened { handle, kind } if kind == v::vnet::SocketKind::Udp => {
                        piano.bind(handle);
                        crate::log!(
                            "esp-piano: udp listener bound handle={} port={}\n",
                            handle.0,
                            trueos_esp::piano::TRUEOS_PIANO_UDP_PORT
                        );
                    }
                    v::vnet::Event::Closed { handle } if piano.unbind(handle) => {
                        crate::log!("esp-piano: udp listener closed, reopening\n");
                        let _ = vnet.submit(piano.bootstrap_command());
                    }
                    v::vnet::Event::UdpPacket { handle, from, data } => {
                        let handled = piano.on_packet(handle, data.as_slice(), |event| {
                            let kind = match event.kind {
                                trueos_esp::piano::PianoNoteEventKind::Down => "down",
                                trueos_esp::piano::PianoNoteEventKind::Up => "up",
                            };
                            crate::log!(
                                "esp-piano: note {} key={} note={} velocity={} delta={} from={}.{}.{}.{}\n",
                                kind,
                                event.key_index,
                                event.note,
                                event.velocity,
                                event.delta,
                                from.addr[0],
                                from.addr[1],
                                from.addr[2],
                                from.addr[3]
                            );

                            match event.kind {
                                trueos_esp::piano::PianoNoteEventKind::Down => {
                                    crate::aud::live_piano::note_on(event.note, event.velocity);
                                }
                                trueos_esp::piano::PianoNoteEventKind::Up => {
                                    crate::aud::live_piano::note_off(event.note);
                                }
                            }
                        });
                        if !handled {
                            crate::log!(
                                "esp-piano: ignored udp bytes={} from={}.{}.{}.{}:{}\n",
                                data.len(),
                                from.addr[0],
                                from.addr[1],
                                from.addr[2],
                                from.addr[3],
                                from.port
                            );
                        }
                    }
                    v::vnet::Event::Error { msg } => {
                        crate::log!("esp-piano: error {}\n", msg);
                    }
                    v::vnet::Event::UdpPacketV6 { .. }
                    | v::vnet::Event::TcpEstablished { .. }
                    | v::vnet::Event::TcpData { .. }
                    | v::vnet::Event::TcpSent { .. }
                    | v::vnet::Event::IcmpReply { .. }
                    | v::vnet::Event::IcmpReplyV6 { .. }
                    | v::vnet::Event::Opened { .. }
                    | v::vnet::Event::Closed { .. } => {}
                }

                continue;
            }

            Timer::after(Duration::from_millis(5)).await;
        }
    }
}
