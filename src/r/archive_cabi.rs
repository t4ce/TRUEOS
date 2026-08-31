extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::r::io::cabi::{
    BLUEPRINT_ASYNC_FS_MAX_PATH, FS_ERR_BAD_PARAM, FS_ERR_BAD_PATH, FS_ERR_BAD_UTF8, FS_ERR_IO,
    FS_ERR_NO_SPACE, FS_ERR_NOT_FOUND, FS_ERR_TOO_LARGE,
};

/// A bounded path list keeps a guest request inside one comm-page payload and
/// matches the explorer's 256-node visible-directory cap.
pub const PACK_MANY_SOURCE_CAP: usize = 256;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct TrueosArchiveReport {
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub file_count: u32,
    pub reserved: u32,
}

#[inline]
fn direct_owner() -> u32 {
    crate::r::io::runtime_context_key()
}

pub(crate) fn map_error(error: &crate::r::codec::CodecError) -> i32 {
    use crate::r::codec::CodecError;
    match error {
        CodecError::BadPath => FS_ERR_BAD_PATH,
        CodecError::NotFound | CodecError::NoRoot => FS_ERR_NOT_FOUND,
        CodecError::QueueFull => FS_ERR_NO_SPACE,
        CodecError::LimitExceeded => FS_ERR_TOO_LARGE,
        CodecError::NotReady | CodecError::PathConflict => FS_ERR_BAD_PARAM,
        CodecError::ReadFailed
        | CodecError::WriteFailed
        | CodecError::Archive(_)
        | CodecError::Fs(_) => FS_ERR_IO,
    }
}

pub(crate) fn start_pack(owner: u32, source: String, archive: String) -> i32 {
    match crate::r::codec::enqueue_7z_pack(owner, source, archive) {
        Ok(id) => id as i32,
        Err(error) => map_error(&error),
    }
}

pub(crate) fn start_pack_many(owner: u32, sources: Vec<String>, archive: String) -> i32 {
    match crate::r::codec::enqueue_7z_pack_many(owner, sources, archive) {
        Ok(id) => id as i32,
        Err(error) => map_error(&error),
    }
}

pub(crate) fn start_unpack(owner: u32, archive: String, destination: String) -> i32 {
    match crate::r::codec::enqueue_7z_unpack(owner, archive, destination) {
        Ok(id) => id as i32,
        Err(error) => map_error(&error),
    }
}

pub(crate) fn status(owner: u32, id: u32) -> i32 {
    match crate::r::codec::operation_status(owner, id) {
        crate::r::codec::OPERATION_PENDING => 0,
        crate::r::codec::OPERATION_READY => 1,
        crate::r::codec::OPERATION_NOT_FOUND => FS_ERR_NOT_FOUND,
        crate::r::codec::OPERATION_FAILED => crate::r::codec::operation_report(owner, id)
            .err()
            .map_or(FS_ERR_IO, |error| map_error(&error)),
        _ => FS_ERR_IO,
    }
}

pub(crate) fn report(owner: u32, id: u32) -> Result<TrueosArchiveReport, i32> {
    crate::r::codec::operation_report(owner, id)
        .map(|report| TrueosArchiveReport {
            input_bytes: report.input_bytes,
            output_bytes: report.output_bytes,
            file_count: report.file_count,
            reserved: 0,
        })
        .map_err(|error| map_error(&error))
}

pub(crate) fn discard(owner: u32, id: u32) -> i32 {
    match crate::r::codec::discard_operation(owner, id) {
        0 => 0,
        _ => FS_ERR_NOT_FOUND,
    }
}

fn parse_path(path_ptr: *const u8, path_len: usize) -> Result<String, i32> {
    if path_ptr.is_null() || path_len == 0 {
        return Err(FS_ERR_BAD_PARAM);
    }
    if path_len > BLUEPRINT_ASYNC_FS_MAX_PATH {
        return Err(FS_ERR_TOO_LARGE);
    }
    let bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
    let path = core::str::from_utf8(bytes).map_err(|_| FS_ERR_BAD_UTF8)?;
    crate::r::io::env::resolve_fs_path(path, false).ok_or(FS_ERR_BAD_PATH)
}

fn guest_start(op: u32, first: &str, second: &str) -> i32 {
    let total = match first.len().checked_add(second.len()) {
        Some(total) if total <= trueos_vm::vmcall::PAYLOAD_CAP => total,
        _ => return FS_ERR_TOO_LARGE,
    };
    let mut payload = Vec::new();
    if payload.try_reserve_exact(total).is_err() {
        return FS_ERR_NO_SPACE;
    }
    payload.extend_from_slice(first.as_bytes());
    payload.extend_from_slice(second.as_bytes());
    let (call_status, value) = trueos_vm::vmcall::call_with_payload(
        op,
        first.len() as u64,
        0,
        payload.as_slice(),
        &mut [],
    );
    if call_status == trueos_vm::vmcall::STATUS_OK {
        (value as i64) as i32
    } else {
        FS_ERR_BAD_PARAM
    }
}

fn parse_path_list(paths_ptr: *const u8, paths_len: usize) -> Result<Vec<String>, i32> {
    if paths_ptr.is_null() || paths_len == 0 {
        return Err(FS_ERR_BAD_PARAM);
    }
    let bytes = unsafe { core::slice::from_raw_parts(paths_ptr, paths_len) };
    let mut paths = Vec::new();
    for encoded_path in bytes.split(|byte| *byte == 0) {
        if encoded_path.is_empty() || paths.len() >= PACK_MANY_SOURCE_CAP {
            return Err(FS_ERR_BAD_PARAM);
        }
        paths.push(parse_path(encoded_path.as_ptr(), encoded_path.len())?);
    }
    Ok(paths)
}

fn guest_start_many(op: u32, sources: &[String], archive: &str) -> i32 {
    if sources.is_empty() || sources.len() > PACK_MANY_SOURCE_CAP {
        return FS_ERR_BAD_PARAM;
    }
    let source_bytes =
        match sources
            .iter()
            .enumerate()
            .try_fold(0usize, |total, (index, source)| {
                total
                    .checked_add(usize::from(index != 0))
                    .and_then(|total| total.checked_add(source.len()))
                    .ok_or(())
            }) {
            Ok(bytes) => bytes,
            Err(()) => return FS_ERR_TOO_LARGE,
        };
    let total = match source_bytes.checked_add(archive.len()) {
        Some(total) if total <= trueos_vm::vmcall::PAYLOAD_CAP => total,
        _ => return FS_ERR_TOO_LARGE,
    };
    let mut payload = Vec::new();
    if payload.try_reserve_exact(total).is_err() {
        return FS_ERR_NO_SPACE;
    }
    for (index, source) in sources.iter().enumerate() {
        if index != 0 {
            payload.push(0);
        }
        payload.extend_from_slice(source.as_bytes());
    }
    payload.extend_from_slice(archive.as_bytes());
    let (call_status, value) = trueos_vm::vmcall::call_with_payload(
        op,
        source_bytes as u64,
        0,
        payload.as_slice(),
        &mut [],
    );
    if call_status == trueos_vm::vmcall::STATUS_OK {
        (value as i64) as i32
    } else {
        FS_ERR_BAD_PARAM
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_archive_pack_start(
    source_ptr: *const u8,
    source_len: usize,
    archive_ptr: *const u8,
    archive_len: usize,
) -> i32 {
    let source = match parse_path(source_ptr, source_len) {
        Ok(path) => path,
        Err(error) => return error,
    };
    let archive = match parse_path(archive_ptr, archive_len) {
        Ok(path) => path,
        Err(error) => return error,
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_start(trueos_vm::vmcall::OP_BP_ARCHIVE_PACK_START, source.as_str(), archive.as_str())
    } else {
        start_pack(direct_owner(), source, archive)
    }
}

/// Start one archive operation for a NUL-separated non-empty list of regular
/// file paths. The archive itself is written only when every source has been
/// read and encoded successfully.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_archive_pack_many_start(
    sources_ptr: *const u8,
    sources_len: usize,
    archive_ptr: *const u8,
    archive_len: usize,
) -> i32 {
    let sources = match parse_path_list(sources_ptr, sources_len) {
        Ok(paths) => paths,
        Err(error) => return error,
    };
    let archive = match parse_path(archive_ptr, archive_len) {
        Ok(path) => path,
        Err(error) => return error,
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_start_many(
            trueos_vm::vmcall::OP_BP_ARCHIVE_PACK_MANY_START,
            sources.as_slice(),
            archive.as_str(),
        )
    } else {
        start_pack_many(direct_owner(), sources, archive)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_archive_unpack_start(
    archive_ptr: *const u8,
    archive_len: usize,
    destination_ptr: *const u8,
    destination_len: usize,
) -> i32 {
    let archive = match parse_path(archive_ptr, archive_len) {
        Ok(path) => path,
        Err(error) => return error,
    };
    let destination = match parse_path(destination_ptr, destination_len) {
        Ok(path) => path,
        Err(error) => return error,
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_start(
            trueos_vm::vmcall::OP_BP_ARCHIVE_UNPACK_START,
            archive.as_str(),
            destination.as_str(),
        )
    } else {
        start_unpack(direct_owner(), archive, destination)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_archive_status(id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (call_status, value) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_ARCHIVE_STATUS, id as u64, 0);
        return if call_status == trueos_vm::vmcall::STATUS_OK {
            (value as i64) as i32
        } else {
            FS_ERR_BAD_PARAM
        };
    }
    status(direct_owner(), id)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_archive_report(id: u32, out: *mut TrueosArchiveReport) -> i32 {
    if out.is_null() {
        return FS_ERR_BAD_PARAM;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut bytes = [0u8; 24];
        let (call_status, value) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_ARCHIVE_REPORT,
            id as u64,
            0,
            &[],
            &mut bytes,
        );
        let rc = if call_status == trueos_vm::vmcall::STATUS_OK {
            (value as i64) as i32
        } else {
            FS_ERR_BAD_PARAM
        };
        if rc != 0 {
            return rc;
        }
        unsafe {
            *out = TrueosArchiveReport {
                input_bytes: u64::from_le_bytes(bytes[0..8].try_into().unwrap_or_default()),
                output_bytes: u64::from_le_bytes(bytes[8..16].try_into().unwrap_or_default()),
                file_count: u32::from_le_bytes(bytes[16..20].try_into().unwrap_or_default()),
                reserved: 0,
            };
        }
        return 0;
    }
    match report(direct_owner(), id) {
        Ok(report) => {
            unsafe {
                *out = report;
            }
            0
        }
        Err(error) => error,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_archive_discard(id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (call_status, value) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_ARCHIVE_DISCARD, id as u64, 0);
        return if call_status == trueos_vm::vmcall::STATUS_OK {
            (value as i64) as i32
        } else {
            FS_ERR_BAD_PARAM
        };
    }
    discard(direct_owner(), id)
}
