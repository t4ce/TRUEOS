extern crate alloc;

include!("../cabi_codes.rs");

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

const ASYNC_FS_MAX_OPERATIONS: usize = 64;
const ASYNC_FS_MAX_RESULT_BYTES: u64 = 16 * 1024 * 1024;
const ASYNC_FS_IDLE_MS: u64 = 1;

#[derive(Debug)]
enum RequestKind {
    Read { path: String },
    Remove { path: String },
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
    start(owner, RequestKind::Read { path })
}

pub(crate) fn start_remove(owner: u32, path: String) -> i32 {
    start(owner, RequestKind::Remove { path })
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
        OperationState::Pending => 0,
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
        OperationState::Pending => FS_ERR_NOT_FOUND as isize,
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
        OperationState::Pending => FS_ERR_NOT_FOUND as isize,
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

async fn process(request: &Request) -> OperationState {
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        return OperationState::Failed(FS_ERR_NOT_FOUND);
    };
    match &request.kind {
        RequestKind::Read { path } => {
            match crate::r::fs::trueosfs::file_info_async(disk, path.as_str()).await {
                Ok(Some(info)) if info.data_len > ASYNC_FS_MAX_RESULT_BYTES => {
                    OperationState::Failed(FS_ERR_TOO_LARGE)
                }
                Ok(Some(_)) => {
                    match crate::r::fs::trueosfs::file_out_async(disk, path.as_str()).await {
                        Ok(Some(bytes)) => OperationState::Read(bytes),
                        Ok(None) => OperationState::Failed(FS_ERR_NOT_FOUND),
                        Err(error) => OperationState::Failed(map_block_error(error)),
                    }
                }
                Ok(None) => OperationState::Failed(FS_ERR_NOT_FOUND),
                Err(error) => OperationState::Failed(map_block_error(error)),
            }
        }
        RequestKind::Remove { path } => {
            match crate::r::fs::trueosfs::file_delete_async(disk, path.as_str()).await {
                Ok(true) => OperationState::Unit,
                Ok(false) => OperationState::Failed(FS_ERR_NOT_FOUND),
                Err(error) => OperationState::Failed(map_block_error(error)),
            }
        }
    }
}

#[embassy_executor::task]
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_async_fs_read_start(
    path_ptr: *const u8,
    path_len: usize,
) -> i32 {
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
