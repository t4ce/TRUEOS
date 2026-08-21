extern crate alloc;

include!("../cabi_codes.rs");

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;
use trueos_time::{Duration as EmbassyDuration, Timer};

const ASYNC_FS_MAX_OPERATIONS: usize = 64;
const ASYNC_FS_MAX_RESULT_BYTES: u64 = 16 * 1024 * 1024;
const ASYNC_FS_IDLE_MS: u64 = 1;
/// Reserved async-FS identifier for a host-provided, one-shot VMX minishell
/// input stream. `vFile:` is deliberately not a TrueOSFS pathname.
const VMX_LAUNCH_SCRIPT_VFILE: &str = "vFile:launch";
const LEGACY_VMX_LAUNCH_SCRIPT_PATH: &str = "/.trueos/launch";

#[derive(Debug)]
enum RequestKind {
    Read { path: String },
    Write { path: String, bytes: Vec<u8> },
    CreateDirAll { path: String },
    Stat { path: String },
    RecordKey { path: String },
    ListDir { path: String },
    ListMounts,
    Remove { path: String },
    Rename { source: String, destination: String },
}

#[derive(Debug)]
struct Request {
    id: u32,
    owner: u32,
    kind: RequestKind,
}

#[derive(Debug)]
enum OperationState {
    Pending,
    Upload {
        path: String,
        bytes: Vec<u8>,
        total_len: usize,
    },
    Read(Vec<u8>),
    Unit,
    Failed(i32),
}

#[derive(Debug)]
struct Operation {
    owner: u32,
    state: OperationState,
}

static ASYNC_FS_SEQUENCE: AtomicU32 = AtomicU32::new(1);
static ASYNC_FS_REQUESTS: Mutex<VecDeque<Request>> = Mutex::new(VecDeque::new());
static ASYNC_FS_OPERATIONS: Mutex<BTreeMap<u32, Operation>> = Mutex::new(BTreeMap::new());

#[inline]
pub(crate) const fn owner_for_vm(vm_id: u8) -> u32 {
    0x8000_0000 | vm_id as u32
}

#[inline]
fn map_block_error(error: crate::disc::block::Error) -> i32 {
    match error {
        crate::disc::block::Error::InvalidParam | crate::disc::block::Error::OutOfBounds => {
            FS_ERR_BAD_PARAM
        }
        crate::disc::block::Error::NotReady => FS_ERR_NOT_FOUND,
        crate::disc::block::Error::NotSupported
        | crate::disc::block::Error::Timeout
        | crate::disc::block::Error::Io
        | crate::disc::block::Error::Corrupted
        | crate::disc::block::Error::DmaUnavailable
        | crate::disc::block::Error::MmioMapFailed => FS_ERR_IO,
    }
}

fn next_operation_id(operations: &BTreeMap<u32, Operation>) -> Option<u32> {
    for _ in 0..ASYNC_FS_MAX_OPERATIONS.saturating_add(1) {
        // Keep operation handles representable as a positive C ABI `i32`, including
        // after the sequence counter eventually wraps.
        let id = (ASYNC_FS_SEQUENCE.fetch_add(1, Ordering::Relaxed) & i32::MAX as u32).max(1);
        if !operations.contains_key(&id) {
            return Some(id);
        }
    }
    None
}

fn start(owner: u32, kind: RequestKind) -> i32 {
    let mut operations = ASYNC_FS_OPERATIONS.lock();
    if operations.len() >= ASYNC_FS_MAX_OPERATIONS {
        return FS_ERR_NO_SPACE;
    }
    let Some(id) = next_operation_id(&operations) else {
        return FS_ERR_NO_SPACE;
    };
    operations.insert(
        id,
        Operation {
            owner,
            state: OperationState::Pending,
        },
    );
    drop(operations);

    let mut requests = ASYNC_FS_REQUESTS.lock();
    if requests.len() >= ASYNC_FS_MAX_OPERATIONS {
        ASYNC_FS_OPERATIONS.lock().remove(&id);
        return FS_ERR_NO_SPACE;
    }
    requests.push_back(Request { id, owner, kind });
    id as i32
}

pub(crate) fn start_read(owner: u32, path: String) -> i32 {
    if path == VMX_LAUNCH_SCRIPT_VFILE
        || path == LEGACY_VMX_LAUNCH_SCRIPT_PATH
        || path == &LEGACY_VMX_LAUNCH_SCRIPT_PATH[1..]
    {
        let vm_bits = owner & !0x8000_0000;
        if owner & 0x8000_0000 == 0 || vm_bits > u8::MAX as u32 {
            crate::log!("async-fs: vFile launch rejected owner=0x{:08x}\n", owner);
            return FS_ERR_NOT_FOUND;
        }
        let Some(script) = crate::hv::take_blueprint_launch_script(vm_bits as u8) else {
            crate::log!("async-fs: vFile launch absent vm={}\n", vm_bits);
            return FS_ERR_NOT_FOUND;
        };
        crate::log!("async-fs: vFile launch served vm={} bytes={}\n", vm_bits, script.len());
        return start_completed_read(owner, script.into_bytes());
    }
    start(owner, RequestKind::Read { path })
}

fn start_completed_read(owner: u32, bytes: Vec<u8>) -> i32 {
    if bytes.len() as u64 > ASYNC_FS_MAX_RESULT_BYTES {
        return FS_ERR_TOO_LARGE;
    }
    let mut operations = ASYNC_FS_OPERATIONS.lock();
    if operations.len() >= ASYNC_FS_MAX_OPERATIONS {
        return FS_ERR_NO_SPACE;
    }
    let Some(id) = next_operation_id(&operations) else {
        return FS_ERR_NO_SPACE;
    };
    operations.insert(
        id,
        Operation {
            owner,
            state: OperationState::Read(bytes),
        },
    );
    id as i32
}

pub(crate) fn start_write(owner: u32, path: String, total_len: usize) -> i32 {
    if total_len as u64 > ASYNC_FS_MAX_RESULT_BYTES {
        return FS_ERR_TOO_LARGE;
    }

    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(total_len).is_err() {
        return FS_ERR_NO_SPACE;
    }

    let mut operations = ASYNC_FS_OPERATIONS.lock();
    if operations.len() >= ASYNC_FS_MAX_OPERATIONS {
        return FS_ERR_NO_SPACE;
    }
    let Some(id) = next_operation_id(&operations) else {
        return FS_ERR_NO_SPACE;
    };
    operations.insert(
        id,
        Operation {
            owner,
            state: OperationState::Upload {
                path,
                bytes,
                total_len,
            },
        },
    );
    id as i32
}

pub(crate) fn write_chunk(owner: u32, id: u32, offset: usize, data: &[u8]) -> i32 {
    let mut operations = ASYNC_FS_OPERATIONS.lock();
    let Some(operation) = operations
        .get_mut(&id)
        .filter(|operation| operation.owner == owner)
    else {
        return FS_ERR_NOT_FOUND;
    };
    let OperationState::Upload {
        bytes, total_len, ..
    } = &mut operation.state
    else {
        return FS_ERR_BAD_PARAM;
    };
    if offset != bytes.len() || data.len() > total_len.saturating_sub(offset) {
        return FS_ERR_BAD_PARAM;
    }
    bytes.extend_from_slice(data);
    0
}

pub(crate) fn write_commit(owner: u32, id: u32) -> i32 {
    let request = {
        let mut operations = ASYNC_FS_OPERATIONS.lock();
        let Some(operation) = operations
            .get_mut(&id)
            .filter(|operation| operation.owner == owner)
        else {
            return FS_ERR_NOT_FOUND;
        };
        let state = core::mem::replace(&mut operation.state, OperationState::Pending);
        match state {
            OperationState::Upload {
                path,
                bytes,
                total_len,
            } if bytes.len() == total_len => Request {
                id,
                owner,
                kind: RequestKind::Write { path, bytes },
            },
            state => {
                operation.state = state;
                return FS_ERR_BAD_PARAM;
            }
        }
    };

    let mut requests = ASYNC_FS_REQUESTS.lock();
    if requests.len() >= ASYNC_FS_MAX_OPERATIONS {
        drop(requests);
        if let Some(operation) = ASYNC_FS_OPERATIONS.lock().get_mut(&id) {
            operation.state = OperationState::Failed(FS_ERR_NO_SPACE);
        }
        return FS_ERR_NO_SPACE;
    }
    requests.push_back(request);
    0
}

pub(crate) fn start_create_dir_all(owner: u32, path: String) -> i32 {
    start(owner, RequestKind::CreateDirAll { path })
}

pub(crate) fn start_stat(owner: u32, path: String) -> i32 {
    start(owner, RequestKind::Stat { path })
}

pub(crate) fn start_record_key(owner: u32, path: String) -> i32 {
    start(owner, RequestKind::RecordKey { path })
}

pub(crate) fn start_list_dir(owner: u32, path: String) -> i32 {
    let path_for_log = path.clone();
    let id = start(owner, RequestKind::ListDir { path });
    crate::log_info!(target: "filesystem";
        "blueprint-async-fs: submitted id={} op=list-dir owner={} path={}\n",
        id,
        owner,
        path_for_log
    );
    id
}

pub(crate) fn start_list_mounts(owner: u32) -> i32 {
    start(owner, RequestKind::ListMounts)
}

pub(crate) fn start_remove(owner: u32, path: String) -> i32 {
    start(owner, RequestKind::Remove { path })
}

pub(crate) fn start_rename(owner: u32, source: String, destination: String) -> i32 {
    start(
        owner,
        RequestKind::Rename {
            source,
            destination,
        },
    )
}

pub(crate) fn status(owner: u32, id: u32) -> i32 {
    let operations = ASYNC_FS_OPERATIONS.lock();
    let Some(operation) = operations
        .get(&id)
        .filter(|operation| operation.owner == owner)
    else {
        return FS_ERR_NOT_FOUND;
    };
    match operation.state {
        OperationState::Pending | OperationState::Upload { .. } => 0,
        OperationState::Read(_) | OperationState::Unit => 1,
        OperationState::Failed(code) => code,
    }
}

pub(crate) fn result_len(owner: u32, id: u32) -> isize {
    let operations = ASYNC_FS_OPERATIONS.lock();
    let Some(operation) = operations
        .get(&id)
        .filter(|operation| operation.owner == owner)
    else {
        return FS_ERR_NOT_FOUND as isize;
    };
    match &operation.state {
        OperationState::Pending | OperationState::Upload { .. } => FS_ERR_NOT_FOUND as isize,
        OperationState::Read(bytes) => bytes.len() as isize,
        OperationState::Unit => 0,
        OperationState::Failed(code) => *code as isize,
    }
}

pub(crate) fn result_read(owner: u32, id: u32, offset: usize, out: &mut [u8]) -> isize {
    let operations = ASYNC_FS_OPERATIONS.lock();
    let Some(operation) = operations
        .get(&id)
        .filter(|operation| operation.owner == owner)
    else {
        return FS_ERR_NOT_FOUND as isize;
    };
    match &operation.state {
        OperationState::Pending | OperationState::Upload { .. } => FS_ERR_NOT_FOUND as isize,
        OperationState::Read(bytes) => {
            if offset > bytes.len() {
                return FS_ERR_BAD_PARAM as isize;
            }
            let end = core::cmp::min(offset.saturating_add(out.len()), bytes.len());
            let count = end.saturating_sub(offset);
            out[..count].copy_from_slice(&bytes[offset..end]);
            count as isize
        }
        OperationState::Unit => 0,
        OperationState::Failed(code) => *code as isize,
    }
}

pub(crate) fn discard(owner: u32, id: u32) -> i32 {
    let mut operations = ASYNC_FS_OPERATIONS.lock();
    if operations.get(&id).map(|operation| operation.owner) != Some(owner) {
        return FS_ERR_NOT_FOUND;
    }
    operations.remove(&id);
    0
}

fn selected_disk(path: &str) -> Result<(crate::disc::block::DeviceHandle, &str), i32> {
    const PREFIX: &str = "trueosfs:disc";
    let Some(selector) = path.strip_prefix(PREFIX) else {
        return crate::r::fs::trueosfs::primary_root_handle()
            .map(|disk| (disk, path))
            .ok_or(FS_ERR_NOT_FOUND);
    };
    let (raw, relative) = selector.split_once('/').unwrap_or((selector, ""));
    let raw = raw.parse::<u32>().map_err(|_| FS_ERR_BAD_PATH)?;
    let disk_id = crate::disc::block::DiscId::from_raw(raw);
    if !crate::r::fs::trueosfs::list_roots()
        .iter()
        .any(|root| root.disk_id == disk_id)
    {
        return Err(FS_ERR_NOT_FOUND);
    }
    crate::disc::block::device_handle(disk_id)
        .map(|disk| (disk, relative))
        .ok_or(FS_ERR_NOT_FOUND)
}

/// Resolve a TRUEOSFS path as a file (`Some(true)`), directory
/// (`Some(false)`), or missing (`None`). Directories are implicit in the log,
/// with `.keep` representing an otherwise empty directory.
async fn path_kind_async(
    disk: crate::disc::block::DeviceHandle,
    path: &str,
) -> Result<Option<bool>, crate::disc::block::Error> {
    if crate::r::fs::trueosfs::file_info_async(disk, path)
        .await?
        .is_some()
    {
        return Ok(Some(true));
    }
    let marker = alloc::format!("{}/.keep", path.trim_end_matches('/'));
    if crate::r::fs::trueosfs::file_exists_async(disk, marker.as_str()).await?
        || crate::r::fs::trueosfs::dir_has_children_async(disk, path).await?
    {
        return Ok(Some(false));
    }
    Ok(None)
}

fn mounted_roots_text() -> String {
    use core::fmt::Write as _;

    let primary = crate::r::fs::trueosfs::primary_root_id();
    let mut out = String::new();
    for root in crate::r::fs::trueosfs::list_roots() {
        let Some(disk) = crate::disc::block::device_handle(root.disk_id) else {
            continue;
        };
        let info = disk.info();
        let label = info
            .label
            .as_deref()
            .unwrap_or("TRUEOSFS")
            .replace(['\t', '\r', '\n'], " ");
        let _ = writeln!(
            out,
            "trueosfs:disc{}\t{}\t{}\t{}",
            root.disk_id.raw(),
            label,
            u8::from(primary == Some(root.disk_id)),
            u8::from(info.is_read_only()),
        );
    }
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

async fn process(request: &Request) -> OperationState {
    if matches!(request.kind, RequestKind::ListMounts) {
        return OperationState::Read(mounted_roots_text().into_bytes());
    }
    if let RequestKind::Rename {
        source,
        destination,
    } = &request.kind
    {
        let (source_disk, source_path) = match selected_disk(source) {
            Ok(selected) => selected,
            Err(code) => return OperationState::Failed(code),
        };
        let (destination_disk, destination_path) = match selected_disk(destination) {
            Ok(selected) => selected,
            Err(code) => return OperationState::Failed(code),
        };
        if source_disk.id() != destination_disk.id() {
            return OperationState::Failed(FS_ERR_BAD_PATH);
        }
        if source_path.is_empty() || destination_path.is_empty() {
            return OperationState::Failed(FS_ERR_BAD_PATH);
        }
        if source_path == destination_path {
            return OperationState::Unit;
        }
        match path_kind_async(destination_disk, destination_path).await {
            Ok(Some(_)) => return OperationState::Failed(FS_ERR_ALREADY_EXISTS),
            Ok(None) => {}
            Err(error) => return OperationState::Failed(map_block_error(error)),
        }

        let result = match path_kind_async(source_disk, source_path).await {
            Ok(Some(true)) => {
                crate::r::fs::trueosfs::file_rename_async(
                    source_disk,
                    source_path,
                    destination_path,
                )
                .await
            }
            Ok(Some(false)) => {
                crate::r::fs::trueosfs::dir_rename_async(source_disk, source_path, destination_path)
                    .await
            }
            Ok(None) => return OperationState::Failed(FS_ERR_NOT_FOUND),
            Err(error) => return OperationState::Failed(map_block_error(error)),
        };
        return match result {
            Ok(true) => OperationState::Unit,
            Ok(false) => OperationState::Failed(FS_ERR_NOT_FOUND),
            Err(error) => OperationState::Failed(map_block_error(error)),
        };
    }
    let path = match &request.kind {
        RequestKind::Read { path }
        | RequestKind::Write { path, .. }
        | RequestKind::CreateDirAll { path }
        | RequestKind::Stat { path }
        | RequestKind::RecordKey { path }
        | RequestKind::ListDir { path }
        | RequestKind::Remove { path } => path.as_str(),
        RequestKind::ListMounts | RequestKind::Rename { .. } => unreachable!(),
    };
    let (disk, selected_path) = match selected_disk(path) {
        Ok(selected) => selected,
        Err(code) => return OperationState::Failed(code),
    };
    match &request.kind {
        RequestKind::Read { path: _ } => {
            let path = selected_path;
            match crate::r::fs::trueosfs::file_info_async(disk, path).await {
                Ok(Some(info)) if info.data_len > ASYNC_FS_MAX_RESULT_BYTES => {
                    OperationState::Failed(FS_ERR_TOO_LARGE)
                }
                Ok(Some(_)) => match crate::r::fs::trueosfs::file_out_async(disk, path).await {
                    Ok(Some(bytes)) => OperationState::Read(bytes),
                    Ok(None) => OperationState::Failed(FS_ERR_NOT_FOUND),
                    Err(error) => OperationState::Failed(map_block_error(error)),
                },
                Ok(None) => OperationState::Failed(FS_ERR_NOT_FOUND),
                Err(error) => OperationState::Failed(map_block_error(error)),
            }
        }
        RequestKind::Write { path, bytes } => {
            let _ = path;
            match crate::r::fs::trueosfs::file_in_async(disk, selected_path, bytes.as_slice()).await
            {
                Ok(true) => OperationState::Unit,
                Ok(false) => OperationState::Failed(FS_ERR_NO_SPACE),
                Err(error) => OperationState::Failed(map_block_error(error)),
            }
        }
        RequestKind::CreateDirAll { path } => {
            let _ = path;
            match crate::r::fs::trueosfs::dir_create_all_async(disk, selected_path).await {
                Ok(true) => OperationState::Unit,
                Ok(false) => OperationState::Failed(FS_ERR_NO_SPACE),
                Err(error) => OperationState::Failed(map_block_error(error)),
            }
        }
        RequestKind::Stat { path } => {
            let _ = path;
            let path = selected_path;
            let stat = if path.is_empty() {
                Ok((2u32, 0u64))
            } else {
                match crate::r::fs::trueosfs::file_info_async(disk, path).await {
                    Ok(Some(info)) => Ok((1u32, info.data_len)),
                    Ok(None) => {
                        let marker = alloc::format!("{}/.keep", path);
                        match crate::r::fs::trueosfs::file_exists_async(disk, marker.as_str()).await
                        {
                            Ok(true) => Ok((2u32, 0u64)),
                            Ok(false) => {
                                match crate::r::fs::trueosfs::dir_has_children_async(disk, path)
                                    .await
                                {
                                    Ok(true) => Ok((2u32, 0u64)),
                                    Ok(false) => Err(FS_ERR_NOT_FOUND),
                                    Err(error) => Err(map_block_error(error)),
                                }
                            }
                            Err(error) => Err(map_block_error(error)),
                        }
                    }
                    Err(error) => Err(map_block_error(error)),
                }
            };
            match stat {
                Ok((kind, len)) => {
                    let mut bytes = Vec::with_capacity(12);
                    bytes.extend_from_slice(&kind.to_le_bytes());
                    bytes.extend_from_slice(&len.to_le_bytes());
                    OperationState::Read(bytes)
                }
                Err(code) => OperationState::Failed(code),
            }
        }
        RequestKind::RecordKey { path } => {
            let _ = path;
            match crate::r::fs::trueosfs::file_info_async(disk, selected_path).await {
                Ok(Some(info)) => {
                    let mut bytes = Vec::with_capacity(56);
                    match info.record_key {
                        crate::r::fs::trueosfs::RecordKey::Ffa => bytes.extend_from_slice(&[0; 56]),
                        crate::r::fs::trueosfs::RecordKey::Key(key) => {
                            bytes.push(1);
                            bytes.extend_from_slice(&[0; 7]);
                            bytes.extend_from_slice(key.provider.as_bytes());
                            bytes.extend_from_slice(key.handle.as_bytes());
                        }
                    }
                    OperationState::Read(bytes)
                }
                Ok(None) => OperationState::Failed(FS_ERR_NOT_FOUND),
                Err(error) => OperationState::Failed(map_block_error(error)),
            }
        }
        RequestKind::ListDir { path: _ } => {
            crate::log_info!(target: "filesystem";
                "blueprint-async-fs: begin id={} op=list-dir owner={} path={}\n",
                request.id,
                request.owner,
                selected_path
            );
            let state = match crate::r::fs::trueosfs::list_dir_async(disk, selected_path).await {
                Ok(Some(listing)) if listing.len() as u64 > ASYNC_FS_MAX_RESULT_BYTES => {
                    OperationState::Failed(FS_ERR_TOO_LARGE)
                }
                Ok(Some(listing)) => OperationState::Read(listing.into_bytes()),
                Ok(None) => OperationState::Failed(FS_ERR_NOT_FOUND),
                Err(error) => OperationState::Failed(map_block_error(error)),
            };
            match &state {
                OperationState::Read(bytes) => crate::log_info!(target: "filesystem";
                    "blueprint-async-fs: done id={} op=list-dir owner={} status=ok bytes={}\n",
                    request.id,
                    request.owner,
                    bytes.len()
                ),
                OperationState::Failed(code) => crate::log_info!(target: "filesystem";
                    "blueprint-async-fs: done id={} op=list-dir owner={} status=error code={}\n",
                    request.id,
                    request.owner,
                    code
                ),
                _ => {}
            }
            state
        }
        RequestKind::Remove { path } => {
            let _ = path;
            match crate::r::fs::trueosfs::file_delete_async(disk, selected_path).await {
                Ok(true) => OperationState::Unit,
                Ok(false) => OperationState::Failed(FS_ERR_NOT_FOUND),
                Err(error) => OperationState::Failed(map_block_error(error)),
            }
        }
        RequestKind::ListMounts => unreachable!(),
        RequestKind::Rename { .. } => unreachable!(),
    }
}

#[trueos_executor::task]
pub async fn service_task() {
    loop {
        let request = ASYNC_FS_REQUESTS.lock().pop_front();
        let Some(request) = request else {
            Timer::after(EmbassyDuration::from_millis(ASYNC_FS_IDLE_MS)).await;
            continue;
        };

        if status(request.owner, request.id) != 0 {
            continue;
        }
        let state = process(&request).await;
        let mut operations = ASYNC_FS_OPERATIONS.lock();
        if let Some(operation) = operations
            .get_mut(&request.id)
            .filter(|operation| operation.owner == request.owner)
        {
            operation.state = state;
        }
    }
}

fn parse_path(path_ptr: *const u8, path_len: usize, allow_empty: bool) -> Result<String, i32> {
    if (path_ptr.is_null() && path_len != 0) || (!allow_empty && path_len == 0) {
        return Err(FS_ERR_BAD_PARAM);
    }
    if path_len > QJS_ASYNC_FS_MAX_PATH {
        return Err(FS_ERR_TOO_LARGE);
    }
    let bytes = if path_len == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(path_ptr, path_len) }
    };
    let path = core::str::from_utf8(bytes).map_err(|_| FS_ERR_BAD_UTF8)?;
    super::env::resolve_fs_path(path, allow_empty).ok_or(FS_ERR_BAD_PATH)
}

#[inline]
fn direct_owner() -> u32 {
    super::runtime_context_key()
}

fn guest_start(op: u32, path: &str) -> i32 {
    let (status, value) = trueos_vm::vmcall::call_with_payload(op, 0, 0, path.as_bytes(), &mut []);
    if status == trueos_vm::vmcall::STATUS_OK {
        (value as i64) as i32
    } else {
        FS_ERR_BAD_PARAM
    }
}

fn guest_write_begin(path: &str, total_len: usize) -> i32 {
    let (status, value) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_ASYNC_FS_WRITE_BEGIN,
        total_len as u64,
        0,
        path.as_bytes(),
        &mut [],
    );
    if status == trueos_vm::vmcall::STATUS_OK {
        (value as i64) as i32
    } else {
        FS_ERR_BAD_PARAM
    }
}

fn guest_rename_start(source: &str, destination: &str) -> i32 {
    let Some(payload_len) = 4usize
        .checked_add(source.len())
        .and_then(|len| len.checked_add(destination.len()))
    else {
        return FS_ERR_TOO_LARGE;
    };
    if payload_len > trueos_vm::vmcall::PAYLOAD_CAP || source.len() > u32::MAX as usize {
        return FS_ERR_TOO_LARGE;
    }
    let mut payload = Vec::with_capacity(payload_len);
    payload.extend_from_slice(&(source.len() as u32).to_le_bytes());
    payload.extend_from_slice(source.as_bytes());
    payload.extend_from_slice(destination.as_bytes());
    let (status, value) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_ASYNC_FS_RENAME_START,
        0,
        0,
        payload.as_slice(),
        &mut [],
    );
    if status == trueos_vm::vmcall::STATUS_OK {
        (value as i64) as i32
    } else {
        FS_ERR_BAD_PARAM
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_async_fs_read_start(
    path_ptr: *const u8,
    path_len: usize,
) -> i32 {
    // Virtual files are VM-local host streams, not members of the app's
    // TrueOSFS root. Preserve the namespace before normal path resolution.
    if crate::hv::current_hull_guest_context_vm_id().is_some()
        && !path_ptr.is_null()
        && path_len == VMX_LAUNCH_SCRIPT_VFILE.len()
    {
        let raw_path = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
        if raw_path == VMX_LAUNCH_SCRIPT_VFILE.as_bytes() {
            return guest_start(
                trueos_vm::vmcall::OP_BP_ASYNC_FS_READ_START,
                VMX_LAUNCH_SCRIPT_VFILE,
            );
        }
    }
    let path = match parse_path(path_ptr, path_len, false) {
        Ok(path) => path,
        Err(code) => return code,
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_start(trueos_vm::vmcall::OP_BP_ASYNC_FS_READ_START, path.as_str())
    } else {
        start_read(direct_owner(), path)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_async_fs_write_begin(
    path_ptr: *const u8,
    path_len: usize,
    total_len: usize,
) -> i32 {
    let path = match parse_path(path_ptr, path_len, false) {
        Ok(path) => path,
        Err(code) => return code,
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_write_begin(path.as_str(), total_len)
    } else {
        start_write(direct_owner(), path, total_len)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_async_fs_write_chunk(
    id: u32,
    offset: usize,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    if data_ptr.is_null() && data_len != 0 {
        return FS_ERR_BAD_PARAM;
    }
    let data = if data_len == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(data_ptr, data_len) }
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut sent = 0usize;
        while sent < data.len() {
            let end =
                core::cmp::min(sent.saturating_add(trueos_vm::vmcall::PAYLOAD_CAP), data.len());
            let (call_status, value) = trueos_vm::vmcall::call_with_payload(
                trueos_vm::vmcall::OP_BP_ASYNC_FS_WRITE_CHUNK,
                id as u64,
                offset.saturating_add(sent) as u64,
                &data[sent..end],
                &mut [],
            );
            let rc = if call_status == trueos_vm::vmcall::STATUS_OK {
                (value as i64) as i32
            } else {
                FS_ERR_BAD_PARAM
            };
            if rc != 0 {
                return rc;
            }
            sent = end;
        }
        0
    } else {
        write_chunk(direct_owner(), id, offset, data)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_async_fs_write_commit(id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (call_status, value) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_ASYNC_FS_WRITE_COMMIT, id as u64, 0);
        if call_status == trueos_vm::vmcall::STATUS_OK {
            (value as i64) as i32
        } else {
            FS_ERR_BAD_PARAM
        }
    } else {
        write_commit(direct_owner(), id)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_async_fs_create_dir_all_start(
    path_ptr: *const u8,
    path_len: usize,
) -> i32 {
    let path = match parse_path(path_ptr, path_len, true) {
        Ok(path) => path,
        Err(code) => return code,
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_start(trueos_vm::vmcall::OP_BP_ASYNC_FS_CREATE_DIR_ALL_START, path.as_str())
    } else {
        start_create_dir_all(direct_owner(), path)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_async_fs_stat_start(
    path_ptr: *const u8,
    path_len: usize,
) -> i32 {
    let path = match parse_path(path_ptr, path_len, true) {
        Ok(path) => path,
        Err(code) => return code,
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_start(trueos_vm::vmcall::OP_BP_ASYNC_FS_STAT_START, path.as_str())
    } else {
        start_stat(direct_owner(), path)
    }
}

/// Start a metadata-only read of the `RecordKey` stored in a TRUEOSFS file header.
///
/// The result is a fixed 56-byte wire record: kind (0 = FFA, 1 = key), seven
/// reserved bytes, provider id (16 bytes), and key handle (32 bytes).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_async_fs_record_key_start(
    path_ptr: *const u8,
    path_len: usize,
) -> i32 {
    let path = match parse_path(path_ptr, path_len, false) {
        Ok(path) => path,
        Err(code) => return code,
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_start(trueos_vm::vmcall::OP_BP_ASYNC_FS_RECORD_KEY_START, path.as_str())
    } else {
        start_record_key(direct_owner(), path)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_async_fs_list_dir_start(
    path_ptr: *const u8,
    path_len: usize,
) -> i32 {
    let path = match parse_path(path_ptr, path_len, true) {
        Ok(path) => path,
        Err(code) => return code,
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_start(trueos_vm::vmcall::OP_BP_ASYNC_FS_LIST_DIR_START, path.as_str())
    } else {
        start_list_dir(direct_owner(), path)
    }
}

/// Enumerate every mounted TRUEOSFS root visible to a host-granted explorer.
///
/// The result uses the normal async result functions and contains one UTF-8
/// TSV record per mount: selector, label, primary flag, read-only flag.
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_async_fs_list_mounts_start() -> i32 {
    if !super::env::trueosfs_scope_granted() {
        return FS_ERR_BAD_PATH;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (call_status, value) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_ASYNC_FS_LIST_MOUNTS_START, 0, 0);
        if call_status == trueos_vm::vmcall::STATUS_OK {
            (value as i64) as i32
        } else {
            FS_ERR_BAD_PARAM
        }
    } else {
        start_list_mounts(direct_owner())
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_async_fs_remove_start(
    path_ptr: *const u8,
    path_len: usize,
) -> i32 {
    let path = match parse_path(path_ptr, path_len, false) {
        Ok(path) => path,
        Err(code) => return code,
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_start(trueos_vm::vmcall::OP_BP_ASYNC_FS_REMOVE_START, path.as_str())
    } else {
        start_remove(direct_owner(), path)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_async_fs_rename_start(
    source_ptr: *const u8,
    source_len: usize,
    destination_ptr: *const u8,
    destination_len: usize,
) -> i32 {
    let source = match parse_path(source_ptr, source_len, false) {
        Ok(path) => path,
        Err(code) => return code,
    };
    let destination = match parse_path(destination_ptr, destination_len, false) {
        Ok(path) => path,
        Err(code) => return code,
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_rename_start(source.as_str(), destination.as_str())
    } else {
        start_rename(direct_owner(), source, destination)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_async_fs_status(id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (call_status, value) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_ASYNC_FS_STATUS, id as u64, 0);
        return if call_status == trueos_vm::vmcall::STATUS_OK {
            (value as i64) as i32
        } else {
            FS_ERR_BAD_PARAM
        };
    }
    status(direct_owner(), id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_async_fs_result_len(id: u32) -> isize {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (call_status, value) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_ASYNC_FS_RESULT_LEN, id as u64, 0);
        return if call_status == trueos_vm::vmcall::STATUS_OK {
            (value as i64) as isize
        } else {
            FS_ERR_BAD_PARAM as isize
        };
    }
    result_len(direct_owner(), id)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_async_fs_result_read(
    id: u32,
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if out_ptr.is_null() && out_cap != 0 {
        return FS_ERR_BAD_PARAM as isize;
    }
    let out = if out_cap == 0 {
        &mut [][..]
    } else {
        unsafe { core::slice::from_raw_parts_mut(out_ptr, out_cap) }
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let want = core::cmp::min(out.len(), trueos_vm::vmcall::PAYLOAD_CAP);
        let packed = ((offset as u64) << 32) | want as u64;
        let (call_status, value) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_ASYNC_FS_RESULT_READ,
            id as u64,
            packed,
            &[],
            &mut out[..want],
        );
        return if call_status == trueos_vm::vmcall::STATUS_OK {
            (value as i64) as isize
        } else {
            FS_ERR_BAD_PARAM as isize
        };
    }
    result_read(direct_owner(), id, offset, out)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_async_fs_discard(id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (call_status, value) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_ASYNC_FS_DISCARD, id as u64, 0);
        return if call_status == trueos_vm::vmcall::STATUS_OK {
            (value as i64) as i32
        } else {
            FS_ERR_BAD_PARAM
        };
    }
    discard(direct_owner(), id)
}
