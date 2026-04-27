use alloc::{format, string::String, vec::Vec};

use super::hvlogf;
use crate::hv::memory::*;

pub const VM_SNAPSHOT_MAGIC: u32 = 0x3153_4D56; // "VMS1"
pub const VM_SNAPSHOT_VERSION: u32 = 1;
pub const GUEST_SNAPSHOT_PAGE_COUNT: usize = 6 + GUEST_LOW_PT_COUNT + GUEST_HIGH_IMAGE_PT_COUNT;

pub fn snapshot_path(vm_id: u8) -> String {
    format!("vm/vm{}.snapshot", vm_id)
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct VmSnapshotHeader {
    pub magic: u32,
    pub version: u32,
    pub guest_cr3: u64,
    pub guest_rip: u64,
    pub guest_rsp: u64,
    pub code_base: u64,
    pub code_len: u64,
    pub exit_reason: u64,
    pub exit_qualification: u64,
    pub exit_guest_rip: u64,
    pub guest_stack_bytes: u64,
    pub guest_page_bytes: u64,
}

#[derive(Copy, Clone, Debug)]
pub enum SaveError {
    UnsupportedVmId,
    NoRoot,
    NoSnapshot,
    BeginWrite,
    Io(crate::disc::block::Error),
}

#[derive(Copy, Clone, Debug)]
pub enum RestoreError {
    UnsupportedVmId,
    NoRoot,
    MissingFile,
    Read(crate::disc::block::Error),
    BadSnapshot,
    CodeMismatch,
}

pub fn capture_snapshot_meta(vm_id: u8, lr: crate::hv::vmx::LaunchResult) {
    let Some(meta_lock) = vm_snapshot_meta_lock(vm_id) else {
        return;
    };
    let mut meta = meta_lock.lock();
    if let Some(mut m) = *meta {
        m.exit_reason = lr.exit_reason;
        m.exit_qualification = lr.exit_qualification;
        m.exit_guest_rip = lr.guest_rip;
        *meta = Some(m);
    }
}

pub fn snapshot_bytes(vm_id: u8) -> Result<Vec<u8>, SaveError> {
    let Some(meta_lock) = vm_snapshot_meta_lock(vm_id) else {
        return Err(SaveError::UnsupportedVmId);
    };
    let Some(meta) = *meta_lock.lock() else {
        return Err(SaveError::NoSnapshot);
    };

    let header = VmSnapshotHeader {
        magic: VM_SNAPSHOT_MAGIC,
        version: VM_SNAPSHOT_VERSION,
        guest_cr3: meta.guest_cr3,
        guest_rip: meta.guest_rip,
        guest_rsp: meta.guest_rsp,
        code_base: meta.code_base,
        code_len: meta.code_len,
        exit_reason: meta.exit_reason,
        exit_qualification: meta.exit_qualification,
        exit_guest_rip: meta.exit_guest_rip,
        guest_stack_bytes: active_guest_stack_bytes_for_vm(vm_id) as u64,
        guest_page_bytes: PAGE_SIZE_4K as u64,
    };
    let guest_stack = guest_stack_slice_for_vm(vm_id).ok_or(SaveError::NoSnapshot)?;

    let total = core::mem::size_of::<VmSnapshotHeader>()
        + (GUEST_SNAPSHOT_PAGE_COUNT * PAGE_SIZE_4K)
        + guest_stack.len()
        + meta.code_len as usize;
    let mut out = Vec::with_capacity(total);
    push_bytes(&mut out, unsafe {
        core::slice::from_raw_parts(
            (&header as *const VmSnapshotHeader).cast::<u8>(),
            core::mem::size_of::<VmSnapshotHeader>(),
        )
    });
    unsafe {
        push_guest_pages_for_vm(vm_id, &mut out).map_err(|_| SaveError::NoSnapshot)?;
        push_bytes(&mut out, guest_stack);
        push_bytes(
            &mut out,
            core::slice::from_raw_parts(meta.code_base as *const u8, meta.code_len as usize),
        );
    }
    Ok(out)
}

pub fn restore_snapshot_bytes(vm_id: u8, bytes: &[u8]) -> Result<(), RestoreError> {
    let Some(snapshot_meta_lock) = vm_snapshot_meta_lock(vm_id) else {
        return Err(RestoreError::UnsupportedVmId);
    };
    let Some(restore_meta_lock) = vm_restore_meta_lock(vm_id) else {
        return Err(RestoreError::UnsupportedVmId);
    };
    let header_len = core::mem::size_of::<VmSnapshotHeader>();
    if bytes.len() < header_len {
        return Err(RestoreError::BadSnapshot);
    }

    let header = parse_snapshot_header(&bytes[..header_len])?;
    let expected = header_len
        + (GUEST_SNAPSHOT_PAGE_COUNT * PAGE_SIZE_4K)
        + (header.guest_stack_bytes as usize)
        + (header.code_len as usize);
    if bytes.len() < expected || header.guest_page_bytes as usize != PAGE_SIZE_4K {
        return Err(RestoreError::BadSnapshot);
    }
    let header_stack_bytes =
        usize::try_from(header.guest_stack_bytes).map_err(|_| RestoreError::BadSnapshot)?;
    prepare_guest_stack_bytes_for_vm(vm_id, header_stack_bytes)
        .map_err(|_| RestoreError::BadSnapshot)?;

    let mut off = header_len;
    unsafe {
        restore_guest_pages_for_vm(vm_id, bytes, &mut off)
            .map_err(|_| RestoreError::BadSnapshot)?;
        let stack_ptr = guest_stack_mut_ptr_for_vm(vm_id).ok_or(RestoreError::BadSnapshot)?;
        core::ptr::copy_nonoverlapping(
            bytes[off..off + header_stack_bytes].as_ptr(),
            stack_ptr,
            header_stack_bytes,
        );
        off += header_stack_bytes;
    }

    let code_end = off + header.code_len as usize;
    let live_code = unsafe {
        core::slice::from_raw_parts(header.code_base as *const u8, header.code_len as usize)
    };
    if live_code != &bytes[off..code_end] {
        return Err(RestoreError::CodeMismatch);
    }

    let guest_cr3 = guest_cr3_pa_for_vm(vm_id).map_err(|_| RestoreError::BadSnapshot)?;
    let restored = VmSnapshotMeta {
        guest_cr3,
        guest_rip: header.guest_rip,
        guest_rsp: header.guest_rsp,
        code_base: header.code_base,
        code_len: header.code_len,
        exit_reason: header.exit_reason,
        exit_qualification: header.exit_qualification,
        exit_guest_rip: header.exit_guest_rip,
    };
    *snapshot_meta_lock.lock() = Some(restored);
    *restore_meta_lock.lock() = Some(restored);
    hvlogf(format_args!(
        "hv: vm{} reporting: restore armed path={} guest_cr3=0x{:016X} guest_rip=0x{:016X} guest_rsp=0x{:016X}",
        vm_id,
        snapshot_path(vm_id).as_str(),
        restored.guest_cr3,
        restored.guest_rip,
        restored.guest_rsp
    ));
    Ok(())
}

fn parse_snapshot_header(bytes: &[u8]) -> Result<VmSnapshotHeader, RestoreError> {
    let mut off = 0usize;
    let magic = take_u32(bytes, &mut off)?;
    let version = take_u32(bytes, &mut off)?;
    let guest_cr3 = take_u64(bytes, &mut off)?;
    let guest_rip = take_u64(bytes, &mut off)?;
    let guest_rsp = take_u64(bytes, &mut off)?;
    let code_base = take_u64(bytes, &mut off)?;
    let code_len = take_u64(bytes, &mut off)?;
    let exit_reason = take_u64(bytes, &mut off)?;
    let exit_qualification = take_u64(bytes, &mut off)?;
    let exit_guest_rip = take_u64(bytes, &mut off)?;
    let guest_stack_bytes = take_u64(bytes, &mut off)?;
    let guest_page_bytes = take_u64(bytes, &mut off)?;
    if magic != VM_SNAPSHOT_MAGIC || version != VM_SNAPSHOT_VERSION {
        return Err(RestoreError::BadSnapshot);
    }
    Ok(VmSnapshotHeader {
        magic,
        version,
        guest_cr3,
        guest_rip,
        guest_rsp,
        code_base,
        code_len,
        exit_reason,
        exit_qualification,
        exit_guest_rip,
        guest_stack_bytes,
        guest_page_bytes,
    })
}

fn take_u32(bytes: &[u8], off: &mut usize) -> Result<u32, RestoreError> {
    let end = off.checked_add(4).ok_or(RestoreError::BadSnapshot)?;
    let raw: [u8; 4] = bytes
        .get(*off..end)
        .ok_or(RestoreError::BadSnapshot)?
        .try_into()
        .map_err(|_| RestoreError::BadSnapshot)?;
    *off = end;
    Ok(u32::from_le_bytes(raw))
}

fn take_u64(bytes: &[u8], off: &mut usize) -> Result<u64, RestoreError> {
    let end = off.checked_add(8).ok_or(RestoreError::BadSnapshot)?;
    let raw: [u8; 8] = bytes
        .get(*off..end)
        .ok_or(RestoreError::BadSnapshot)?
        .try_into()
        .map_err(|_| RestoreError::BadSnapshot)?;
    *off = end;
    Ok(u64::from_le_bytes(raw))
}

fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(bytes);
}
