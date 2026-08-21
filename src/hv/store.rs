use alloc::{boxed::Box, collections::BTreeMap, format, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use heapless::Deque;
use spin::Mutex;
use trueos_executor::task;
use trueos_time::{Duration as EmbassyDuration, Timer};

use crate::disc::block;
use crate::net::adapter::{
    NetCommand, NetEvent, NetHandle, NetQueue, SocketKind, register_app_queues,
};
use crate::r::net::ports;
use crate::wait::{CompletionCell, WaitQueue};

const VM_STORE_REPL_CHUNK: usize = 1200;
const VM_STORE_VM_ID_LIMIT: usize = crate::allcaps::hv::VM_ID_LIMIT;
const VM_STORE_QUEUE_CAP: usize = 8;
const PERSISTENT_VM_MAGIC: &[u8; 8] = b"TVMSTR1\0";
const PERSISTENT_VM_VERSION: u32 = 1;
const PERSISTENT_VM_PREFIX: &str = "vm/snapshots/";
const PERSISTENT_VM_SUFFIX: &str = ".tvm";

#[inline]
fn boot_probe_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

static VM_STORE_ONLINE: AtomicBool = AtomicBool::new(false);
// Warm checkpoints are deliberately independent from the block registry.  A
// slot owns one immutable byte image and `eject` can therefore reclaim it
// without leaving an immortal private ramdisk device behind.
static VM_STORE_IMAGES: [Mutex<Option<Arc<[u8]>>>; VM_STORE_VM_ID_LIMIT] =
    [const { Mutex::new(None) }; VM_STORE_VM_ID_LIMIT];
static VM_STORE_QUEUE: Mutex<Deque<Request, VM_STORE_QUEUE_CAP>> = Mutex::new(Deque::new());
static VM_STORE_QUEUE_WAIT: WaitQueue = WaitQueue::new();
static VM_STORE_REQ_SEQ: AtomicU64 = AtomicU64::new(1);
static VM_STORE_OBJECT_SEQ: AtomicU64 = AtomicU64::new(1);
static VM_STORE_COMMITTED_SEQS: Mutex<BTreeMap<u8, u64>> = Mutex::new(BTreeMap::new());
static VM_STORE_COMMIT_WAIT: WaitQueue = WaitQueue::new();
static VM_STORE_REPLICATION_ONLINE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
pub enum VmStoreError {
    ServiceOffline,
    QueueFull,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Create(block::Error),
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Format(block::Error),
    BeginWrite(block::Error),
    MissingSnapshot,
    Read(block::Error),
    Write(block::Error),
    NoPersistentRoot,
    InvalidName,
    BadEnvelope,
}

#[derive(Clone, Debug)]
pub enum VmStoreResponse {
    Saved(usize),
    Loaded(Vec<u8>),
}

enum RequestKind {
    Save(u8, Vec<u8>),
    Load(u8),
}

struct Request {
    id: u64,
    kind: RequestKind,
    completion: Arc<Completion>,
}

struct Completion {
    result: CompletionCell<Result<VmStoreResponse, VmStoreError>>,
}

impl Completion {
    fn new() -> Self {
        Self {
            result: CompletionCell::new(),
        }
    }

    fn complete(&self, result: Result<VmStoreResponse, VmStoreError>) {
        let _ = self.result.complete(result);
    }

    fn wait_blocking(&self) -> Result<VmStoreResponse, VmStoreError> {
        self.result.join_blocking_parked()
    }

    async fn wait_async(&self) -> Result<VmStoreResponse, VmStoreError> {
        self.result.join().await
    }
}

fn vm_id_supported(vm_id: u8) -> bool {
    (vm_id as usize) < VM_STORE_VM_ID_LIMIT
}

fn vm_store_image(vm_id: u8) -> Option<Arc<[u8]>> {
    VM_STORE_IMAGES
        .get(vm_id as usize)
        .and_then(|slot| slot.lock().clone())
}

#[inline]
pub fn online() -> bool {
    VM_STORE_ONLINE.load(Ordering::Acquire)
}

pub fn save_bytes(vm_id: u8, bytes: Vec<u8>) -> Result<usize, VmStoreError> {
    if !vm_id_supported(vm_id) {
        return Err(VmStoreError::ServiceOffline);
    }
    match enqueue(RequestKind::Save(vm_id, bytes))?.wait_blocking()? {
        VmStoreResponse::Saved(len) => Ok(len),
        VmStoreResponse::Loaded(_) => Err(VmStoreError::Write(block::Error::Io)),
    }
}

pub async fn save_bytes_async(vm_id: u8, bytes: Vec<u8>) -> Result<usize, VmStoreError> {
    if !vm_id_supported(vm_id) {
        return Err(VmStoreError::ServiceOffline);
    }
    match enqueue(RequestKind::Save(vm_id, bytes))?
        .wait_async()
        .await?
    {
        VmStoreResponse::Saved(len) => Ok(len),
        VmStoreResponse::Loaded(_) => Err(VmStoreError::Write(block::Error::Io)),
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn load_bytes(vm_id: u8) -> Result<Vec<u8>, VmStoreError> {
    if !vm_id_supported(vm_id) {
        return Err(VmStoreError::ServiceOffline);
    }
    match enqueue(RequestKind::Load(vm_id))?.wait_blocking()? {
        VmStoreResponse::Loaded(bytes) => Ok(bytes),
        VmStoreResponse::Saved(_) => Err(VmStoreError::Read(block::Error::Io)),
    }
}

pub async fn load_bytes_async(vm_id: u8) -> Result<Vec<u8>, VmStoreError> {
    if !vm_id_supported(vm_id) {
        return Err(VmStoreError::ServiceOffline);
    }
    match enqueue(RequestKind::Load(vm_id))?.wait_async().await? {
        VmStoreResponse::Loaded(bytes) => Ok(bytes),
        VmStoreResponse::Saved(_) => Err(VmStoreError::Read(block::Error::Io)),
    }
}

pub fn committed_vm_count() -> usize {
    VM_STORE_COMMITTED_SEQS.lock().len()
}

pub fn has_committed_vm(vm_id: u8) -> bool {
    VM_STORE_COMMITTED_SEQS.lock().contains_key(&vm_id)
}

/// Drop only the warm checkpoint for `vm_id`. Persistent named images are not
/// touched.
pub fn eject_warm(vm_id: u8) -> bool {
    if !vm_id_supported(vm_id) {
        return false;
    }
    let dropped = VM_STORE_IMAGES
        .get(vm_id as usize)
        .is_some_and(|slot| slot.lock().take().is_some());
    if dropped {
        VM_STORE_COMMITTED_SEQS.lock().remove(&vm_id);
        VM_STORE_COMMIT_WAIT.notify_all();
    }
    dropped
}

pub fn replication_online() -> bool {
    VM_STORE_REPLICATION_ONLINE.load(Ordering::Acquire)
}

pub struct PersistentVmImage {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub source_vm_id: u8,
    pub snapshot: Vec<u8>,
    pub guest_heap: Vec<u8>,
    pub hull_rw: Vec<u8>,
    pub blueprint: Vec<u8>,
}

fn valid_persistent_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && name != "."
        && name != ".."
}

fn persistent_path(name: &str) -> Result<String, VmStoreError> {
    if !valid_persistent_name(name) {
        return Err(VmStoreError::InvalidName);
    }
    Ok(format!("{}{}{}", PERSISTENT_VM_PREFIX, name, PERSISTENT_VM_SUFFIX))
}

fn persistent_root() -> Result<block::DeviceHandle, VmStoreError> {
    crate::r::fs::trueosfs::list_roots()
        .into_iter()
        .filter_map(|root| {
            let handle = block::device_handle(root.disk_id)?;
            let info = handle.info();
            (info.kind != block::DeviceKind::Ramdisk && info.parent.is_none() && info.writable)
                .then_some((root.seq, handle))
        })
        .max_by_key(|(seq, _)| *seq)
        .map(|(_, handle)| handle)
        .ok_or(VmStoreError::NoPersistentRoot)
}

fn persistent_header(vm_id: u8, lengths: [usize; 4]) -> Vec<u8> {
    let mut out = Vec::with_capacity(48);
    out.extend_from_slice(PERSISTENT_VM_MAGIC);
    out.extend_from_slice(&PERSISTENT_VM_VERSION.to_le_bytes());
    out.push(vm_id);
    out.extend_from_slice(&[0; 3]);
    for len in lengths {
        out.extend_from_slice(&(len as u64).to_le_bytes());
    }
    out
}

/// Commit the current warm checkpoint and the retained Blueprint-owned memory
/// to physical TRUEOSFS. The warm checkpoint remains resident.
pub async fn store_persistent_async(vm_id: u8, name: &str) -> Result<usize, VmStoreError> {
    let state = crate::hv::vm_state(vm_id);
    if !state.supported || !state.pause_latched || state.running || state.starting {
        return Err(VmStoreError::BadEnvelope);
    }
    let path = persistent_path(name)?;
    let disk = persistent_root()?;
    let snapshot = vm_store_image(vm_id).ok_or(VmStoreError::MissingSnapshot)?;
    let guest_heap =
        crate::allocators::snapshot_hv_guest_heap(vm_id).map_err(|_| VmStoreError::BadEnvelope)?;
    let hull_rw = crate::hv::memory::snapshot_guest_hull_rw_for_vm(vm_id)
        .map_err(|_| VmStoreError::BadEnvelope)?;
    let blueprint = crate::hv::snapshot_blueprint_portable_state(vm_id)
        .map_err(|_| VmStoreError::BadEnvelope)?;
    let header = persistent_header(
        vm_id,
        [
            snapshot.len(),
            guest_heap.len(),
            hull_rw.len(),
            blueprint.len(),
        ],
    );
    let total = header
        .len()
        .checked_add(snapshot.len())
        .and_then(|value| value.checked_add(guest_heap.len()))
        .and_then(|value| value.checked_add(hull_rw.len()))
        .and_then(|value| value.checked_add(blueprint.len()))
        .ok_or(VmStoreError::BadEnvelope)?;
    let Some(handle) =
        crate::r::fs::trueosfs::file_write_begin_async(disk, path.as_str(), total as u64)
            .await
            .map_err(VmStoreError::BeginWrite)?
    else {
        return Err(VmStoreError::BeginWrite(block::Error::Io));
    };
    for chunk in [
        header.as_slice(),
        snapshot.as_ref(),
        guest_heap.as_slice(),
        hull_rw.as_slice(),
        blueprint.as_slice(),
    ] {
        if let Err(error) = crate::r::fs::trueosfs::file_write_chunk_async(handle, chunk).await {
            let _ = crate::r::fs::trueosfs::file_write_abort_async(handle).await;
            return Err(VmStoreError::Write(error));
        }
    }
    crate::r::fs::trueosfs::file_write_finish_async(handle)
        .await
        .map_err(VmStoreError::Write)?;
    crate::log!(
        "hv-store: persistent commit name={} vm_id={} bytes={} disk={} warm_retained=1\n",
        name,
        vm_id,
        total,
        disk.id().raw()
    );
    Ok(total)
}

fn envelope_take_u32(bytes: &[u8], offset: &mut usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let raw = bytes.get(*offset..end)?.try_into().ok()?;
    *offset = end;
    Some(u32::from_le_bytes(raw))
}

fn envelope_take_u64(bytes: &[u8], offset: &mut usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    let raw = bytes.get(*offset..end)?.try_into().ok()?;
    *offset = end;
    Some(u64::from_le_bytes(raw))
}

fn envelope_take_vec(bytes: &[u8], offset: &mut usize, len: usize) -> Option<Vec<u8>> {
    let end = offset.checked_add(len)?;
    let value = bytes.get(*offset..end)?.to_vec();
    *offset = end;
    Some(value)
}

pub async fn load_persistent_async(name: &str) -> Result<PersistentVmImage, VmStoreError> {
    let path = persistent_path(name)?;
    let disk = persistent_root()?;
    let bytes = crate::r::fs::trueosfs::file_out_async(disk, path.as_str())
        .await
        .map_err(VmStoreError::Read)?
        .ok_or(VmStoreError::MissingSnapshot)?;
    let mut offset = 0usize;
    if bytes.get(..PERSISTENT_VM_MAGIC.len()) != Some(PERSISTENT_VM_MAGIC.as_slice()) {
        return Err(VmStoreError::BadEnvelope);
    }
    offset += PERSISTENT_VM_MAGIC.len();
    if envelope_take_u32(bytes.as_slice(), &mut offset) != Some(PERSISTENT_VM_VERSION) {
        return Err(VmStoreError::BadEnvelope);
    }
    let source_vm_id = *bytes.get(offset).ok_or(VmStoreError::BadEnvelope)?;
    offset = offset.checked_add(4).ok_or(VmStoreError::BadEnvelope)?;
    let mut lengths = [0usize; 4];
    for len in lengths.iter_mut() {
        *len = usize::try_from(
            envelope_take_u64(bytes.as_slice(), &mut offset).ok_or(VmStoreError::BadEnvelope)?,
        )
        .map_err(|_| VmStoreError::BadEnvelope)?;
    }
    let snapshot = envelope_take_vec(bytes.as_slice(), &mut offset, lengths[0])
        .ok_or(VmStoreError::BadEnvelope)?;
    let guest_heap = envelope_take_vec(bytes.as_slice(), &mut offset, lengths[1])
        .ok_or(VmStoreError::BadEnvelope)?;
    let hull_rw = envelope_take_vec(bytes.as_slice(), &mut offset, lengths[2])
        .ok_or(VmStoreError::BadEnvelope)?;
    let blueprint = envelope_take_vec(bytes.as_slice(), &mut offset, lengths[3])
        .ok_or(VmStoreError::BadEnvelope)?;
    if offset != bytes.len() {
        return Err(VmStoreError::BadEnvelope);
    }
    Ok(PersistentVmImage {
        source_vm_id,
        snapshot,
        guest_heap,
        hull_rw,
        blueprint,
    })
}

pub async fn delete_persistent_async(name: &str) -> Result<bool, VmStoreError> {
    let path = persistent_path(name)?;
    let disk = persistent_root()?;
    crate::r::fs::trueosfs::file_delete_async(disk, path.as_str())
        .await
        .map_err(VmStoreError::Write)
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

pub(crate) fn current_committed_seq(vm_id: u8) -> u64 {
    VM_STORE_COMMITTED_SEQS
        .lock()
        .get(&vm_id)
        .copied()
        .unwrap_or(0)
}

fn push_line(out: &mut Vec<u8>, line: &str) {
    out.extend_from_slice(line.as_bytes());
    out.push(b'\n');
}

fn queue_vm_listing(out: &mut Vec<u8>) {
    let seqs = VM_STORE_COMMITTED_SEQS.lock();
    let mut has_any = false;
    for vm_id in 0..VM_STORE_VM_ID_LIMIT {
        let vm_id = vm_id as u8;
        if seqs.contains_key(&vm_id) {
            push_line(out, format!("VMS {}", vm_id).as_str());
            has_any = true;
        }
    }
    if !has_any {
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
    crate::log_info!(target: "hv"; "hv-store: task start ms={}\n", boot_probe_ms());
    if let Some(profile) = crate::cpu::CpuProfile::current() {
        crate::log_info!(
            target: "hv";
            "hv-store: start slot={} lapic={} kind={}\n",
            profile.slot(),
            profile.lapic_id(),
            profile.core_kind_name()
        );
    } else {
        crate::log_info!(target: "hv"; "hv-store: start slot=unknown\n");
    }

    VM_STORE_ONLINE.store(true, Ordering::Release);
    crate::log_info!(target: "hv"; "hv-store: online mode=warm-arc+persistent-trueosfs\n");

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
        crate::log_info!(target: "hv"; "hv-store-net: store offline; replication unavailable\n");
        return;
    }
    crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;

    if crate::net::device_count() == 0 {
        crate::log_info!(target: "hv"; "hv-store-net: no network device; replication unavailable\n");
        return;
    }

    let mut dev_idx = crate::net::primary_device_index();
    for idx in 0..crate::net::device_count() {
        if crate::net::link_state_at(idx)
            .map(|ls| ls.up)
            .unwrap_or(false)
        {
            dev_idx = idx;
            break;
        }
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
    if cmds
        .push(NetCommand::OpenTcpListen {
            port: ports::VM_STORE_REPL_PORT,
        })
        .is_err()
    {
        crate::log_info!(target: "hv"; "hv-store-net: listen submit failed\n");
        return;
    }
    VM_STORE_REPLICATION_ONLINE.store(true, Ordering::Release);
    crate::log_info!(
        target: "hv";
        "hv-store-net: listening on tcp {} owner={}\n",
        ports::VM_STORE_REPL_PORT,
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
                NetEvent::TcpEstablished { handle, .. } => {
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
                                if !vm_id_supported(id) {
                                    push_line(&mut tx_buf, "NO");
                                    continue;
                                }
                                let Some(bytes) = vm_store_image(id) else {
                                    push_line(&mut tx_buf, "NO");
                                    continue;
                                };
                                let seq = current_committed_seq(id);
                                push_line(
                                    &mut tx_buf,
                                    format!("VM {} {} {}", id, seq, bytes.len()).as_str(),
                                );
                                tx_buf.extend_from_slice(bytes.as_ref());
                                crate::log!(
                                    "hv-store-net: queued vm id={} seq={} bytes={} handle={}\n",
                                    id,
                                    seq,
                                    bytes.len(),
                                    handle.0
                                );
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
                            port: ports::VM_STORE_REPL_PORT,
                        });
                        crate::log!("hv-store-net: tcp closed handle={} (relisten)\n", handle.0);
                    }
                }
                NetEvent::Error { msg } => {
                    crate::log!("hv-store-net: error {}\n", msg);
                }
                NetEvent::UdpPacket { .. }
                | NetEvent::UdpPacketV6 { .. }
                | NetEvent::IpPacket { .. }
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

async fn handle_request(id: u64, kind: RequestKind) -> Result<VmStoreResponse, VmStoreError> {
    match kind {
        RequestKind::Save(vm_id, bytes) => {
            let seq = VM_STORE_OBJECT_SEQ.fetch_add(1, Ordering::Relaxed).max(1);
            crate::log!(
                "hv-store: save queued id={} vm_id={} bytes={} seq={}\n",
                id,
                vm_id,
                bytes.len(),
                seq
            );
            let len = bytes.len();
            if let Some(slot) = VM_STORE_IMAGES.get(vm_id as usize) {
                *slot.lock() = Some(Arc::from(bytes.into_boxed_slice()));
            }
            let mut seqs = VM_STORE_COMMITTED_SEQS.lock();
            seqs.insert(vm_id, seq);
            VM_STORE_COMMIT_WAIT.notify_all();
            crate::log!(
                "hv-store: save complete id={} vm_id={} seq={} bytes={} medium=warm-arc\n",
                id,
                vm_id,
                seq,
                len
            );
            Ok(VmStoreResponse::Saved(len))
        }
        RequestKind::Load(vm_id) => {
            let Some(bytes) = vm_store_image(vm_id) else {
                return Err(VmStoreError::MissingSnapshot);
            };
            crate::log!("hv-store: load queued id={} vm_id={} medium=warm-arc\n", id, vm_id);
            crate::log!(
                "hv-store: load complete id={} vm_id={} bytes={}\n",
                id,
                vm_id,
                bytes.len()
            );
            Ok(VmStoreResponse::Loaded(bytes.as_ref().to_vec()))
        }
    }
}
