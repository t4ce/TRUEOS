use alloc::{boxed::Box, format, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Deque;
use spin::Mutex;

use crate::disc::block;
use crate::net::adapter::{
    NetCommand, NetEvent, NetHandle, NetQueue, SocketKind, register_app_queues,
};
use crate::wait::WaitQueue;

const VM_STORE_PROBE_PATH: &str = "vm/.probe";
const VM_STORE_MANIFEST_PATH: &str = "vm/committed";
const VM_STORE_PENDING_PREFIX: &str = "vm/pending-";
const VM_STORE_OBJECT_PREFIX: &str = "vm/object-";
const VM_STORE_REPL_PORT: u16 = 32123;
const VM_STORE_REPL_CHUNK: usize = 1200;
const VM_STORE_VM_ID: u8 = 0;
const VM_STORE_BLOCK_SIZE: u32 = 512;
const VM_STORE_RAMDISK_BYTES: u64 = 64 * 1024 * 1024;
const VM_STORE_QUEUE_CAP: usize = 8;
const VM_STORE_PROBE_BYTES: &[u8] = b"trueos-hv-store-probe";

#[inline]
fn boot_probe_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

static VM_STORE_ONLINE: AtomicBool = AtomicBool::new(false);
static VM_STORE_DISK: Mutex<Option<block::DeviceHandle>> = Mutex::new(None);
static VM_STORE_QUEUE: Mutex<Deque<Request, VM_STORE_QUEUE_CAP>> = Mutex::new(Deque::new());
static VM_STORE_QUEUE_WAIT: WaitQueue = WaitQueue::new();
static VM_STORE_REQ_SEQ: AtomicU64 = AtomicU64::new(1);
static VM_STORE_OBJECT_SEQ: AtomicU64 = AtomicU64::new(1);
static VM_STORE_COMMITTED_SEQ: AtomicU64 = AtomicU64::new(0);
static VM_STORE_COMMIT_WAIT: WaitQueue = WaitQueue::new();

#[derive(Clone, Debug)]
pub enum VmStoreError {
    ServiceOffline,
    QueueFull,
    Create(block::Error),
    Format(block::Error),
    BeginWrite(block::Error),
    MissingSnapshot,
    Read(block::Error),
    Write(block::Error),
}

#[derive(Clone, Debug)]
pub enum VmStoreResponse {
    Saved(usize),
    Loaded(Vec<u8>),
}

enum RequestKind {
    Save(Vec<u8>),
    Load,
}

struct Request {
    id: u64,
    kind: RequestKind,
    completion: Arc<Completion>,
}

struct Completion {
    wait: WaitQueue,
    result: Mutex<Option<Result<VmStoreResponse, VmStoreError>>>,
}

impl Completion {
    fn new() -> Self {
        Self {
            wait: WaitQueue::new(),
            result: Mutex::new(None),
        }
    }

    fn complete(&self, result: Result<VmStoreResponse, VmStoreError>) {
        *self.result.lock() = Some(result);
        self.wait.notify_all();
    }

    fn wait_blocking(&self) -> Result<VmStoreResponse, VmStoreError> {
        loop {
            if let Some(result) = self.result.lock().take() {
                return result;
            }
            self.wait.wait_for_event_blocking_parked(10);
        }
    }
}

struct HvStoreBlockIo {
    handle: block::DeviceHandle,
}

impl HvStoreBlockIo {
    #[inline]
    const fn new(handle: block::DeviceHandle) -> Self {
        Self { handle }
    }
}

impl trueos_fs::BlockIo for HvStoreBlockIo {
    type Error = block::Error;

    #[inline]
    fn block_size(&self) -> usize {
        self.handle.info().block_size as usize
    }

    #[inline]
    fn block_count(&self) -> u64 {
        self.handle.info().block_count
    }

    #[inline]
    fn max_transfer_bytes(&self) -> usize {
        let v = self.handle.info().max_transfer_bytes as usize;
        if v == 0 { 256 * 1024 } else { v }
    }

    async fn read_blocks(&self, lba: u64, blocks: usize) -> Result<Vec<u8>, block::Error> {
        if blocks == 0 {
            return Ok(Vec::new());
        }

        let info = self.handle.info();
        let bs = info.block_size as usize;
        if bs == 0 {
            return Err(block::Error::InvalidParam);
        }

        let max_blocks = if info.max_transfer_bytes > 0 {
            (info.max_transfer_bytes as usize / bs).max(1)
        } else {
            1
        };

        let mut out = Vec::with_capacity(bs.saturating_mul(blocks));
        let mut cur_lba = lba;
        let mut remaining = blocks;
        while remaining > 0 {
            let blocks_here = core::cmp::min(remaining, max_blocks);
            let tmp = self.handle.read_blocks(cur_lba, blocks_here).await?;
            out.extend_from_slice(&tmp);
            cur_lba = cur_lba.saturating_add(blocks_here as u64);
            remaining = remaining.saturating_sub(blocks_here);
        }

        Ok(out)
    }

    async fn write_blocks(&self, lba: u64, buf: &[u8]) -> Result<(), block::Error> {
        if buf.is_empty() {
            return Ok(());
        }
        let info = self.handle.info();
        let bs = info.block_size as usize;
        if bs == 0 || !buf.len().is_multiple_of(bs) {
            return Err(block::Error::InvalidParam);
        }

        let max_blocks = if info.max_transfer_bytes > 0 {
            (info.max_transfer_bytes as usize / bs).max(1)
        } else {
            1
        };

        let mut cur_lba = lba;
        let mut off = 0usize;
        while off < buf.len() {
            let remaining = buf.len() - off;
            let blocks_here = core::cmp::min(max_blocks, remaining / bs);
            let bytes_here = blocks_here * bs;
            self.handle
                .write_blocks(cur_lba, &buf[off..off + bytes_here])
                .await?;
            cur_lba = cur_lba.saturating_add(blocks_here as u64);
            off = off.saturating_add(bytes_here);
        }

        Ok(())
    }

    #[inline]
    async fn flush(&self) -> Result<(), block::Error> {
        self.handle.flush().await
    }
}

#[inline]
fn map_engine_err(e: trueos_fs::FsError<block::Error>) -> block::Error {
    match e {
        trueos_fs::FsError::Device(e) => e,
        trueos_fs::FsError::InvalidParam => block::Error::InvalidParam,
        trueos_fs::FsError::Corrupted => block::Error::Corrupted,
    }
}

async fn read_private_file(
    disk: block::DeviceHandle,
    path: &str,
) -> Result<Option<Vec<u8>>, block::Error> {
    let Some(placement) = crate::v::fs::trueosfs::locate_async(disk).await? else {
        return Ok(None);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = HvStoreBlockIo::new(disk);
    trueos_fs::read_file(&io, &params, path)
        .await
        .map_err(map_engine_err)
}

fn object_path(prefix: &str, seq: u64) -> String {
    format!("{}{:020}", prefix, seq)
}

fn parse_manifest_seq(bytes: &[u8]) -> Option<u64> {
    let s = core::str::from_utf8(bytes).ok()?.trim();
    s.parse::<u64>().ok()
}

async fn read_committed_bytes(disk: block::DeviceHandle) -> Result<Option<Vec<u8>>, block::Error> {
    let Some(manifest) = read_private_file(disk, VM_STORE_MANIFEST_PATH).await? else {
        return Ok(None);
    };
    let Some(seq) = parse_manifest_seq(manifest.as_slice()) else {
        return Ok(None);
    };
    let path = object_path(VM_STORE_OBJECT_PREFIX, seq);
    read_private_file(disk, path.as_str()).await
}

async fn write_committed_manifest(disk: block::DeviceHandle, seq: u64) -> Result<(), block::Error> {
    let manifest = format!("{}\n", seq);
    let ok =
        crate::v::fs::trueosfs::file_in_async(disk, VM_STORE_MANIFEST_PATH, manifest.as_bytes())
            .await?;
    if ok { Ok(()) } else { Err(block::Error::Io) }
}

#[inline]
pub fn online() -> bool {
    VM_STORE_ONLINE.load(Ordering::Acquire)
}

pub fn save_bytes(bytes: Vec<u8>) -> Result<usize, VmStoreError> {
    match enqueue(RequestKind::Save(bytes))?.wait_blocking()? {
        VmStoreResponse::Saved(len) => Ok(len),
        VmStoreResponse::Loaded(_) => Err(VmStoreError::Write(block::Error::Io)),
    }
}

pub fn load_bytes() -> Result<Vec<u8>, VmStoreError> {
    match enqueue(RequestKind::Load)?.wait_blocking()? {
        VmStoreResponse::Loaded(bytes) => Ok(bytes),
        VmStoreResponse::Saved(_) => Err(VmStoreError::Read(block::Error::Io)),
    }
}

fn enqueue(kind: RequestKind) -> Result<Arc<Completion>, VmStoreError> {
    if !wait_until_online(2000) {
        return Err(VmStoreError::ServiceOffline);
    }

    let completion = Arc::new(Completion::new());
    let req = Request {
        id: VM_STORE_REQ_SEQ.fetch_add(1, Ordering::Relaxed).max(1),
        kind,
        completion: completion.clone(),
    };

    let pushed = {
        let mut q = VM_STORE_QUEUE.lock();
        q.push_back(req).is_ok()
    };
    if !pushed {
        return Err(VmStoreError::QueueFull);
    }
    VM_STORE_QUEUE_WAIT.notify_one();
    Ok(completion)
}

fn wait_until_online(timeout_ms: u64) -> bool {
    if online() {
        return true;
    }
    crate::wait::spin_until_timeout(timeout_ms, online)
}

fn current_committed_seq() -> u64 {
    VM_STORE_COMMITTED_SEQ.load(Ordering::Acquire)
}

fn push_line(out: &mut Vec<u8>, line: &str) {
    out.extend_from_slice(line.as_bytes());
    out.push(b'\n');
}

fn queue_vm_listing(out: &mut Vec<u8>) {
    if current_committed_seq() != 0 {
        push_line(out, "VMS 0");
    } else {
        push_line(out, "VMS");
    }
}

fn parse_vm_id(token: &str) -> Option<u8> {
    token.trim().parse::<u8>().ok()
}

enum VmStoreNetCmd {
    List,
    Pull(u8),
    Ack(u8, bool),
}

fn parse_vm_store_cmd(line: &[u8]) -> Option<VmStoreNetCmd> {
    let text = core::str::from_utf8(line).ok()?.trim();
    if text.is_empty() {
        return None;
    }
    let mut parts = text.split_ascii_whitespace();
    let head = parts.next()?;
    if head != "VM" {
        return None;
    }
    let Some(id_or_ack) = parts.next() else {
        return Some(VmStoreNetCmd::List);
    };
    let id = parse_vm_id(id_or_ack)?;
    match parts.next() {
        None => Some(VmStoreNetCmd::Pull(id)),
        Some("OK") => Some(VmStoreNetCmd::Ack(id, true)),
        Some("RIP") => Some(VmStoreNetCmd::Ack(id, false)),
        Some(_) => None,
    }
}

#[task(pool_size = 1)]
pub async fn vm_store_task() {
    crate::log!("boot-probe: hv-store task start ms={}\n", boot_probe_ms());
    if let Some(profile) = crate::cpu::CpuProfile::current() {
        crate::log!(
            "hv-store: start slot={} lapic={} kind={}\n",
            profile.slot(),
            profile.lapic_id(),
            profile.core_kind_name()
        );
    } else {
        crate::log!("hv-store: start slot=unknown\n");
    }

    match ensure_store_ready().await {
        Ok(disk) => {
            *VM_STORE_DISK.lock() = Some(disk);
            VM_STORE_ONLINE.store(true, Ordering::Release);
            crate::log!(
                "hv-store: ramdisk ready disk={} bytes={}\n",
                disk.id().raw(),
                VM_STORE_RAMDISK_BYTES
            );
        }
        Err(e) => {
            crate::log!("hv-store: init failed: {:?}\n", e);
            return;
        }
    }

    loop {
        let req = {
            let mut q = VM_STORE_QUEUE.lock();
            q.pop_front()
        };

        match req {
            Some(req) => {
                let result = handle_request(req.id, req.kind).await;
                req.completion.complete(result);
            }
            None => {
                VM_STORE_QUEUE_WAIT.wait_for_event().await;
            }
        }
    }
}

#[task(pool_size = 1)]
pub async fn vm_store_replication_task() {
    if !wait_until_online(5000) {
        crate::log!("hv-store-net: store offline; replication unavailable\n");
        return;
    }

    let mut dev_idx = crate::net::primary_device_index();
    let primary_up = crate::net::link_state_at(dev_idx)
        .map(|ls| ls.up)
        .unwrap_or(false);
    if !primary_up {
        for idx in 0..crate::net::device_count() {
            if crate::net::link_state_at(idx)
                .map(|ls| ls.up)
                .unwrap_or(false)
            {
                dev_idx = idx;
                break;
            }
        }
    }
    if crate::net::device_count() == 0 {
        crate::log!("hv-store-net: no network device; replication unavailable\n");
        return;
    }

    let selector = if let Some((bus, slot, func)) = crate::net::bdf_at(dev_idx) {
        format!("{:02x}:{:02x}.{}", bus, slot, func)
    } else if let Some((vid, pid)) = crate::net::pci_id_at(dev_idx) {
        format!("{:04x}:{:04x}", vid, pid)
    } else {
        format!("{}", dev_idx)
    };
    let owner: &'static str = {
        let s = format!("hv-store-net@{}", selector);
        Box::leak(s.into_boxed_str())
    };
    let cmds = NetQueue::new_leaked("hv-store-net-cmd", 128);
    let events = NetQueue::new_leaked("hv-store-net-evt", 128);
    register_app_queues(owner, cmds, events);
    let _ = cmds.push(NetCommand::OpenTcpListen {
        port: VM_STORE_REPL_PORT,
    });
    crate::log!(
        "hv-store-net: listening on tcp {} owner={}\n",
        VM_STORE_REPL_PORT,
        owner
    );

    let mut tcp_handle: Option<NetHandle> = None;
    let mut rx_buf = Vec::new();
    let mut tx_buf = Vec::new();
    let mut tx_offset: usize = 0;
    let mut inflight = false;
    let mut pending_len: usize = 0;

    loop {
        for ev in events.drain(32) {
            match ev {
                NetEvent::Opened { handle, kind } => {
                    if kind == SocketKind::Tcp {
                        tcp_handle = Some(handle);
                    }
                }
                NetEvent::TcpEstablished { handle } => {
                    tcp_handle = Some(handle);
                    inflight = false;
                    rx_buf.clear();
                    tx_buf.clear();
                    tx_offset = 0;
                    pending_len = 0;
                    crate::log!("hv-store-net: tcp established handle={}\n", handle.0);
                }
                NetEvent::TcpSent { handle, len } => {
                    if tcp_handle == Some(handle) && inflight {
                        tx_offset = tx_offset.saturating_add(len);
                        inflight = false;
                        pending_len = 0;
                    }
                }
                NetEvent::TcpData { handle, data } => {
                    if tcp_handle != Some(handle) {
                        continue;
                    }
                    rx_buf.extend_from_slice(&data);
                    while let Some(pos) = rx_buf.iter().position(|&b| b == b'\n') {
                        let line = rx_buf[..pos].to_vec();
                        rx_buf.drain(..=pos);
                        match parse_vm_store_cmd(line.as_slice()) {
                            Some(VmStoreNetCmd::List) => {
                                queue_vm_listing(&mut tx_buf);
                            }
                            Some(VmStoreNetCmd::Pull(id)) => {
                                if id != VM_STORE_VM_ID {
                                    push_line(&mut tx_buf, "NO");
                                    continue;
                                }
                                let Some(disk) = *VM_STORE_DISK.lock() else {
                                    push_line(&mut tx_buf, "NO");
                                    continue;
                                };
                                match read_committed_bytes(disk).await {
                                    Ok(Some(bytes)) => {
                                        let seq = current_committed_seq();
                                        push_line(
                                            &mut tx_buf,
                                            format!(
                                                "VM {} {} {}",
                                                VM_STORE_VM_ID,
                                                seq,
                                                bytes.len()
                                            )
                                            .as_str(),
                                        );
                                        tx_buf.extend_from_slice(bytes.as_slice());
                                        crate::log!(
                                            "hv-store-net: queued vm id={} seq={} bytes={} handle={}\n",
                                            VM_STORE_VM_ID,
                                            seq,
                                            bytes.len(),
                                            handle.0
                                        );
                                    }
                                    Ok(None) => push_line(&mut tx_buf, "NO"),
                                    Err(e) => {
                                        crate::log!(
                                            "hv-store-net: read current failed err={:?}\n",
                                            e
                                        );
                                        push_line(&mut tx_buf, "NO");
                                    }
                                }
                            }
                            Some(VmStoreNetCmd::Ack(id, ok)) => {
                                crate::log!(
                                    "hv-store-net: vm id={} ack={}\n",
                                    id,
                                    if ok { "OK" } else { "RIP" }
                                );
                            }
                            None => {
                                push_line(&mut tx_buf, "NO");
                            }
                        }
                    }
                }
                NetEvent::Closed { handle } => {
                    if tcp_handle == Some(handle) {
                        tcp_handle = None;
                        inflight = false;
                        rx_buf.clear();
                        tx_buf.clear();
                        tx_offset = 0;
                        pending_len = 0;
                        let _ = cmds.push(NetCommand::OpenTcpListen {
                            port: VM_STORE_REPL_PORT,
                        });
                        crate::log!("hv-store-net: tcp closed handle={} (relisten)\n", handle.0);
                    }
                }
                NetEvent::Error { msg } => {
                    crate::log!("hv-store-net: error {}\n", msg);
                }
                NetEvent::UdpPacket { .. }
                | NetEvent::UdpPacketV6 { .. }
                | NetEvent::IcmpReply { .. }
                | NetEvent::IcmpReplyV6 { .. } => {}
            }
        }

        if let Some(handle) = tcp_handle
            && !inflight
        {
            if tx_offset < tx_buf.len() {
                let end = core::cmp::min(tx_offset + VM_STORE_REPL_CHUNK, tx_buf.len());
                let chunk = tx_buf[tx_offset..end].to_vec();
                pending_len = chunk.len();
                if cmds
                    .push(NetCommand::SendTcp {
                        handle,
                        data: chunk,
                    })
                    .is_ok()
                {
                    inflight = true;
                } else {
                    pending_len = 0;
                }
            } else if !tx_buf.is_empty() {
                tx_buf.clear();
                tx_offset = 0;
                pending_len = 0;
            }
        }

        if inflight && pending_len == 0 {
            inflight = false;
        }
        Timer::after(EmbassyDuration::from_millis(10)).await;
    }
}

async fn ensure_store_ready() -> Result<block::DeviceHandle, VmStoreError> {
    let t0 = boot_probe_ms();
    crate::log!("boot-probe: hv-store ensure begin ms={}\n", t0);
    let disk = crate::v::disc::ramdisk::create(VM_STORE_RAMDISK_BYTES, VM_STORE_BLOCK_SIZE)
        .map_err(VmStoreError::Create)?;
    crate::log!(
        "boot-probe: hv-store ramdisk create done ms={} dt={}\n",
        boot_probe_ms(),
        boot_probe_ms().saturating_sub(t0)
    );
    crate::v::fs::trueosfs::format_blank_force_async(disk)
        .await
        .map_err(VmStoreError::Format)?;
    let t1 = boot_probe_ms();
    crate::log!(
        "boot-probe: hv-store format done ms={} dt={}\n",
        t1,
        t1.saturating_sub(t0)
    );
    crate::v::fs::trueosfs::validate_private_medium_async(disk, 0)
        .await
        .map_err(VmStoreError::Format)?;
    let t2 = boot_probe_ms();
    crate::log!(
        "boot-probe: hv-store validate done ms={} dt={} step={}\n",
        t2,
        t2.saturating_sub(t0),
        t2.saturating_sub(t1)
    );
    let wrote =
        crate::v::fs::trueosfs::file_in_async(disk, VM_STORE_PROBE_PATH, VM_STORE_PROBE_BYTES)
            .await
            .map_err(VmStoreError::Format)?;
    if !wrote {
        return Err(VmStoreError::Format(block::Error::Io));
    }
    let t3 = boot_probe_ms();
    crate::log!(
        "boot-probe: hv-store probe write done ms={} dt={} step={}\n",
        t3,
        t3.saturating_sub(t0),
        t3.saturating_sub(t2)
    );
    let Some(probe) = read_private_file(disk, VM_STORE_PROBE_PATH)
        .await
        .map_err(VmStoreError::Format)?
    else {
        return Err(VmStoreError::Format(block::Error::Corrupted));
    };
    if probe.as_slice() != VM_STORE_PROBE_BYTES {
        return Err(VmStoreError::Format(block::Error::Corrupted));
    }
    let t4 = boot_probe_ms();
    crate::log!(
        "boot-probe: hv-store probe read done ms={} dt={} step={}\n",
        t4,
        t4.saturating_sub(t0),
        t4.saturating_sub(t3)
    );
    let t5 = boot_probe_ms();
    crate::log!(
        "boot-probe: hv-store ensure done ms={} dt={} tail={}\n",
        t5,
        t5.saturating_sub(t0),
        t5.saturating_sub(t4)
    );
    Ok(disk)
}

async fn handle_request(id: u64, kind: RequestKind) -> Result<VmStoreResponse, VmStoreError> {
    let Some(disk) = *VM_STORE_DISK.lock() else {
        return Err(VmStoreError::ServiceOffline);
    };

    match kind {
        RequestKind::Save(bytes) => {
            let seq = VM_STORE_OBJECT_SEQ.fetch_add(1, Ordering::Relaxed).max(1);
            let pending_path = object_path(VM_STORE_PENDING_PREFIX, seq);
            let committed_path = object_path(VM_STORE_OBJECT_PREFIX, seq);
            crate::log!(
                "hv-store: save queued id={} bytes={} pending={} committed={}\n",
                id,
                bytes.len(),
                pending_path.as_str(),
                committed_path.as_str()
            );
            let Some(handle) = crate::v::fs::trueosfs::file_write_begin_async(
                disk,
                pending_path.as_str(),
                bytes.len() as u64,
            )
            .await
            .map_err(VmStoreError::BeginWrite)?
            else {
                return Err(VmStoreError::BeginWrite(block::Error::Io));
            };

            if let Err(e) =
                crate::v::fs::trueosfs::file_write_chunk_async(handle, bytes.as_slice()).await
            {
                let _ = crate::v::fs::trueosfs::file_write_abort_async(handle).await;
                return Err(VmStoreError::Write(e));
            }
            crate::v::fs::trueosfs::file_write_finish_async(handle)
                .await
                .map_err(VmStoreError::Write)?;

            let committed = crate::v::fs::trueosfs::file_rename_async(
                disk,
                pending_path.as_str(),
                committed_path.as_str(),
            )
            .await
            .map_err(VmStoreError::Write)?;
            if !committed {
                return Err(VmStoreError::Write(block::Error::Io));
            }

            write_committed_manifest(disk, seq)
                .await
                .map_err(VmStoreError::Write)?;
            VM_STORE_COMMITTED_SEQ.store(seq, Ordering::Release);
            VM_STORE_COMMIT_WAIT.notify_all();
            crate::log!(
                "hv-store: save complete id={} seq={} bytes={} committed={}\n",
                id,
                seq,
                bytes.len(),
                committed_path.as_str()
            );
            Ok(VmStoreResponse::Saved(bytes.len()))
        }
        RequestKind::Load => {
            crate::log!(
                "hv-store: load queued id={} manifest={}\n",
                id,
                VM_STORE_MANIFEST_PATH
            );
            let Some(bytes) = read_committed_bytes(disk)
                .await
                .map_err(VmStoreError::Read)?
            else {
                return Err(VmStoreError::MissingSnapshot);
            };
            crate::log!("hv-store: load complete id={} bytes={}\n", id, bytes.len());
            Ok(VmStoreResponse::Loaded(bytes))
        }
    }
}
