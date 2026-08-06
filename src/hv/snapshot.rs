use alloc::{format, string::String, vec::Vec};

use super::hvlogf;
use crate::hv::memory::*;

pub const VM_SNAPSHOT_MAGIC: u32 = 0x3153_4D56; // "VMS1"
pub const VM_SNAPSHOT_VERSION_LEGACY: u32 = 1;
pub const VM_SNAPSHOT_VERSION_SPARSE: u32 = 2;
pub const VM_SNAPSHOT_VERSION_GPRS: u32 = 3;
pub const VM_SNAPSHOT_VERSION: u32 = 4;
const VM_SNAPSHOT_LEGACY_HEADER_BYTES: usize = 8 + 10 * core::mem::size_of::<u64>();
const VM_SNAPSHOT_GPRS_HEADER_BYTES: usize = 216;
pub const GUEST_SNAPSHOT_PAGE_COUNT: usize = 6 + GUEST_LOW_PT_COUNT + GUEST_HIGH_IMAGE_PT_COUNT;
pub const GUEST_SNAPSHOT_PAGE_BITMAP_BYTES: usize = GUEST_SNAPSHOT_PAGE_COUNT.div_ceil(8);
// Versions 2 through 4 store this fixed bitmap immediately after the header,
// followed by only the 4 KiB page-table pages whose bits are set. Version 3
// adds the live guest GPR/RFLAGS continuation state, and version 4 adds the
// VM-owned x87/SSE/YMM state. Version 1 has no bitmap and stores every table
// page.

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
    pub guest_registers: crate::hv::vmx::GuestRegisters,
    pub guest_rflags: u64,
    pub guest_extended_state_mask: u64,
    pub guest_extended_state: [u8; crate::hv::vmx::VMX_EXTENDED_STATE_BYTES],
}

const _: [(); 1056] = [(); core::mem::size_of::<VmSnapshotHeader>()];

#[derive(Copy, Clone, Debug)]
pub enum SaveError {
    UnsupportedVmId,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    NoRoot,
    NoSnapshot,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    BeginWrite,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Io(crate::disc::block::Error),
}

#[derive(Copy, Clone, Debug)]
pub enum RestoreError {
    UnsupportedVmId,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    NoRoot,
    MissingFile,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Read(crate::disc::block::Error),
    BadSnapshot,
    CodeMismatch,
    VgpuQuarantined,
}

pub fn capture_snapshot_meta(vm_id: u8, lr: crate::hv::vmx::LaunchResult) {
    let Some(meta_lock) = vm_snapshot_meta_lock(vm_id) else {
        return;
    };
    let mut meta = meta_lock.lock();
    if let Some(mut m) = *meta {
        m.guest_rip =
            crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RIP).unwrap_or(lr.guest_rip);
        m.guest_rsp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP).unwrap_or(m.guest_rsp);
        m.guest_registers = crate::hv::vmx::guest_registers();
        m.guest_rflags = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RFLAGS)
            .unwrap_or(crate::hv::vmx::RFLAGS_RESERVED_BIT1);
        if let Ok((mask, state)) = crate::hv::vmx::guest_extended_state_snapshot(vm_id) {
            m.guest_extended_state_mask = mask;
            m.guest_extended_state = state;
        }
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
        guest_registers: meta.guest_registers,
        guest_rflags: meta.guest_rflags,
        guest_extended_state_mask: meta.guest_extended_state_mask,
        guest_extended_state: meta.guest_extended_state,
    };
    let guest_stack = guest_stack_slice_for_vm(vm_id).ok_or(SaveError::NoSnapshot)?;

    let total_capacity = core::mem::size_of::<VmSnapshotHeader>()
        + GUEST_SNAPSHOT_PAGE_BITMAP_BYTES
        + (GUEST_SNAPSHOT_PAGE_COUNT * PAGE_SIZE_4K)
        + guest_stack.len()
        + meta.code_len as usize;
    let mut out = Vec::with_capacity(total_capacity);
    push_bytes(&mut out, unsafe {
        core::slice::from_raw_parts(
            (&header as *const VmSnapshotHeader).cast::<u8>(),
            core::mem::size_of::<VmSnapshotHeader>(),
        )
    });
    let bitmap_offset = out.len();
    out.resize(bitmap_offset + GUEST_SNAPSHOT_PAGE_BITMAP_BYTES, 0);
    unsafe {
        let stored_pages = push_guest_pages_sparse_for_vm(vm_id, &mut out, bitmap_offset)
            .map_err(|_| SaveError::NoSnapshot)?;
        hvlogf(format_args!(
            "hv: vm{} reporting: snapshot page tables sparse stored={} zero={} saved_bytes={}",
            vm_id,
            stored_pages,
            GUEST_SNAPSHOT_PAGE_COUNT.saturating_sub(stored_pages),
            GUEST_SNAPSHOT_PAGE_COUNT
                .saturating_sub(stored_pages)
                .saturating_mul(PAGE_SIZE_4K)
                .saturating_sub(GUEST_SNAPSHOT_PAGE_BITMAP_BYTES),
        ));
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
    if !crate::gpu::vgpu::hull_guest_storage_reusable(vm_id) {
        return Err(RestoreError::VgpuQuarantined);
    }
    if bytes.len() < VM_SNAPSHOT_LEGACY_HEADER_BYTES {
        return Err(RestoreError::BadSnapshot);
    }

    let version = u32::from_le_bytes(
        bytes[4..8]
            .try_into()
            .map_err(|_| RestoreError::BadSnapshot)?,
    );
    let header_len = match version {
        VM_SNAPSHOT_VERSION => core::mem::size_of::<VmSnapshotHeader>(),
        VM_SNAPSHOT_VERSION_GPRS => VM_SNAPSHOT_GPRS_HEADER_BYTES,
        _ => VM_SNAPSHOT_LEGACY_HEADER_BYTES,
    };
    let header = parse_snapshot_header(bytes.get(..header_len).ok_or(RestoreError::BadSnapshot)?)?;
    let sparse_pages = header.version >= VM_SNAPSHOT_VERSION_SPARSE;
    let bitmap_end = if sparse_pages {
        header_len
            .checked_add(GUEST_SNAPSHOT_PAGE_BITMAP_BYTES)
            .ok_or(RestoreError::BadSnapshot)?
    } else {
        header_len
    };
    let bitmap = bytes
        .get(header_len..bitmap_end)
        .ok_or(RestoreError::BadSnapshot)?;
    let stored_page_count = if sparse_pages {
        sparse_page_count(bitmap)
    } else {
        GUEST_SNAPSHOT_PAGE_COUNT
    };
    let expected = header_len
        .checked_add(bitmap.len())
        .and_then(|len| len.checked_add(stored_page_count.checked_mul(PAGE_SIZE_4K)?))
        .and_then(|len| len.checked_add(usize::try_from(header.guest_stack_bytes).ok()?))
        .and_then(|len| len.checked_add(usize::try_from(header.code_len).ok()?))
        .ok_or(RestoreError::BadSnapshot)?;
    if bytes.len() < expected || header.guest_page_bytes as usize != PAGE_SIZE_4K {
        return Err(RestoreError::BadSnapshot);
    }
    let header_stack_bytes =
        usize::try_from(header.guest_stack_bytes).map_err(|_| RestoreError::BadSnapshot)?;
    prepare_guest_stack_bytes_for_vm(vm_id, header_stack_bytes)
        .map_err(|_| RestoreError::BadSnapshot)?;

    let mut off = bitmap_end;
    unsafe {
        if sparse_pages {
            restore_guest_pages_sparse_for_vm(vm_id, bytes, &mut off, bitmap)
                .map_err(|_| RestoreError::BadSnapshot)?;
        } else {
            restore_guest_pages_for_vm(vm_id, bytes, &mut off)
                .map_err(|_| RestoreError::BadSnapshot)?;
        }
        let stack_ptr = guest_stack_mut_ptr_for_vm(vm_id).ok_or(RestoreError::BadSnapshot)?;
        core::ptr::copy_nonoverlapping(
            bytes[off..off + header_stack_bytes].as_ptr(),
            stack_ptr,
            header_stack_bytes,
        );
        off += header_stack_bytes;
    }

    let code_len = usize::try_from(header.code_len).map_err(|_| RestoreError::BadSnapshot)?;
    let code_end = off.checked_add(code_len).ok_or(RestoreError::BadSnapshot)?;
    let stored_code = bytes.get(off..code_end).ok_or(RestoreError::BadSnapshot)?;
    if !immutable_code_matches(&header, stored_code) {
        return Err(RestoreError::CodeMismatch);
    }

    let guest_cr3 = guest_cr3_pa_for_vm(vm_id).map_err(|_| RestoreError::BadSnapshot)?;
    let (guest_extended_state_mask, guest_extended_state) = if header.version == VM_SNAPSHOT_VERSION
    {
        crate::hv::vmx::restore_guest_extended_state(
            vm_id,
            header.guest_extended_state_mask,
            &header.guest_extended_state,
        )
        .map_err(|_| RestoreError::BadSnapshot)?;
        (header.guest_extended_state_mask, header.guest_extended_state)
    } else {
        crate::hv::vmx::reset_guest_extended_state(vm_id).map_err(|_| RestoreError::BadSnapshot)?;
        crate::hv::vmx::guest_extended_state_snapshot(vm_id)
            .map_err(|_| RestoreError::BadSnapshot)?
    };

    let restored = VmSnapshotMeta {
        guest_cr3,
        guest_rip: header.guest_rip,
        guest_rsp: header.guest_rsp,
        guest_registers: header.guest_registers,
        guest_rflags: header.guest_rflags,
        guest_extended_state_mask,
        guest_extended_state,
        code_base: header.code_base,
        code_len: header.code_len,
        exit_reason: header.exit_reason,
        exit_qualification: header.exit_qualification,
        exit_guest_rip: header.exit_guest_rip,
    };
    *snapshot_meta_lock.lock() = Some(restored);
    *restore_meta_lock.lock() = Some(restored);
    hvlogf(format_args!(
        "hv: vm{} reporting: restore armed path={} format=v{} table_pages={} guest_cr3=0x{:016X} guest_rip=0x{:016X} guest_rsp=0x{:016X}",
        vm_id,
        snapshot_path(vm_id).as_str(),
        header.version,
        stored_page_count,
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
    let mut guest_registers = crate::hv::vmx::GuestRegisters::default();
    let mut guest_rflags = crate::hv::vmx::RFLAGS_RESERVED_BIT1;
    if version >= VM_SNAPSHOT_VERSION_GPRS {
        guest_registers.rax = take_u64(bytes, &mut off)?;
        guest_registers.rbx = take_u64(bytes, &mut off)?;
        guest_registers.rcx = take_u64(bytes, &mut off)?;
        guest_registers.rdx = take_u64(bytes, &mut off)?;
        guest_registers.rsi = take_u64(bytes, &mut off)?;
        guest_registers.rdi = take_u64(bytes, &mut off)?;
        guest_registers.rbp = take_u64(bytes, &mut off)?;
        guest_registers.r8 = take_u64(bytes, &mut off)?;
        guest_registers.r9 = take_u64(bytes, &mut off)?;
        guest_registers.r10 = take_u64(bytes, &mut off)?;
        guest_registers.r11 = take_u64(bytes, &mut off)?;
        guest_registers.r12 = take_u64(bytes, &mut off)?;
        guest_registers.r13 = take_u64(bytes, &mut off)?;
        guest_registers.r14 = take_u64(bytes, &mut off)?;
        guest_registers.r15 = take_u64(bytes, &mut off)?;
        guest_rflags = take_u64(bytes, &mut off)?;
    }
    let mut guest_extended_state_mask = crate::cpu::vmx_xsave_mask();
    let mut guest_extended_state = [0u8; crate::hv::vmx::VMX_EXTENDED_STATE_BYTES];
    if version == VM_SNAPSHOT_VERSION {
        guest_extended_state_mask = take_u64(bytes, &mut off)?;
        guest_extended_state = take_bytes(bytes, &mut off)?;
    }
    if magic != VM_SNAPSHOT_MAGIC
        || !matches!(
            version,
            VM_SNAPSHOT_VERSION_LEGACY
                | VM_SNAPSHOT_VERSION_SPARSE
                | VM_SNAPSHOT_VERSION_GPRS
                | VM_SNAPSHOT_VERSION
        )
    {
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
        guest_registers,
        guest_rflags,
        guest_extended_state_mask,
        guest_extended_state,
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

fn take_bytes<const N: usize>(bytes: &[u8], off: &mut usize) -> Result<[u8; N], RestoreError> {
    let end = off.checked_add(N).ok_or(RestoreError::BadSnapshot)?;
    let raw = bytes
        .get(*off..end)
        .ok_or(RestoreError::BadSnapshot)?
        .try_into()
        .map_err(|_| RestoreError::BadSnapshot)?;
    *off = end;
    Ok(raw)
}

fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(bytes);
}

fn sparse_page_count(bitmap: &[u8]) -> usize {
    (0..GUEST_SNAPSHOT_PAGE_COUNT)
        .filter(|index| bitmap[index / 8] & (1 << (index % 8)) != 0)
        .count()
}

fn immutable_code_matches(header: &VmSnapshotHeader, stored_code: &[u8]) -> bool {
    let layout = crate::hv::guest::hull_image_layout();
    immutable_span_matches(header.code_base, stored_code, layout.text_start, layout.text_end)
        && immutable_span_matches(
            header.code_base,
            stored_code,
            layout.rodata_start,
            layout.rodata_end,
        )
}

fn immutable_span_matches(
    code_base: u64,
    stored_code: &[u8],
    span_start: u64,
    span_end: u64,
) -> bool {
    if span_end < span_start || span_start < code_base {
        return false;
    }
    let Some(start) = span_start
        .checked_sub(code_base)
        .and_then(|offset| usize::try_from(offset).ok())
    else {
        return false;
    };
    let Some(end) = span_end
        .checked_sub(code_base)
        .and_then(|offset| usize::try_from(offset).ok())
    else {
        return false;
    };
    let Some(stored) = stored_code.get(start..end) else {
        return false;
    };
    let live =
        unsafe { core::slice::from_raw_parts(span_start as *const u8, end.saturating_sub(start)) };
    live == stored
}
