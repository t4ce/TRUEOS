use alloc::vec::Vec;

use super::hvlogf;
use crate::hv::memory::*;

pub const VM1_SNAPSHOT_MAGIC: u32 = 0x3153_4D56; // "VMS1"
pub const VM1_SNAPSHOT_VERSION: u32 = 1;
pub const VM1_SNAPSHOT_PATH: &str = "vm/vm1.snapshot";
pub const VM1_ID: u8 = 0;
pub const GUEST_SNAPSHOT_PAGE_COUNT: usize = 7 + GUEST_HIGH_IMAGE_PT_COUNT;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Vm1SnapshotHeader {
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

pub fn capture_snapshot_meta(lr: crate::hv::vmx::LaunchResult) {
    let mut meta = VM1_SNAPSHOT_META.lock();
    if let Some(mut m) = *meta {
        m.exit_reason = lr.exit_reason;
        m.exit_qualification = lr.exit_qualification;
        m.exit_guest_rip = lr.guest_rip;
        *meta = Some(m);
    }
}

pub fn snapshot_bytes() -> Result<Vec<u8>, SaveError> {
    let Some(meta) = *VM1_SNAPSHOT_META.lock() else {
        return Err(SaveError::NoSnapshot);
    };

    let header = Vm1SnapshotHeader {
        magic: VM1_SNAPSHOT_MAGIC,
        version: VM1_SNAPSHOT_VERSION,
        guest_cr3: meta.guest_cr3,
        guest_rip: meta.guest_rip,
        guest_rsp: meta.guest_rsp,
        code_base: meta.code_base,
        code_len: meta.code_len,
        exit_reason: meta.exit_reason,
        exit_qualification: meta.exit_qualification,
        exit_guest_rip: meta.exit_guest_rip,
        guest_stack_bytes: GUEST_STACK_BYTES as u64,
        guest_page_bytes: PAGE_SIZE_4K as u64,
    };

    let total = core::mem::size_of::<Vm1SnapshotHeader>()
        + (GUEST_SNAPSHOT_PAGE_COUNT * PAGE_SIZE_4K)
        + GUEST_STACK_BYTES
        + meta.code_len as usize;
    let mut out = Vec::with_capacity(total);
    push_bytes(&mut out, unsafe {
        core::slice::from_raw_parts(
            (&header as *const Vm1SnapshotHeader).cast::<u8>(),
            core::mem::size_of::<Vm1SnapshotHeader>(),
        )
    });
    unsafe {
        push_guest_page(&mut out, core::ptr::addr_of!(GUEST_PML4.0));
        push_guest_page(&mut out, core::ptr::addr_of!(GUEST_LOW_PDPT.0));
        push_guest_page(&mut out, core::ptr::addr_of!(GUEST_LOW_PD.0));
        push_guest_page(&mut out, core::ptr::addr_of!(GUEST_STACK_PT.0));
        push_guest_page(&mut out, core::ptr::addr_of!(GUEST_HIGH_PDPT.0));
        push_guest_page(&mut out, core::ptr::addr_of!(GUEST_HIGH_PD.0));
        for i in 0..GUEST_HIGH_IMAGE_PT_COUNT {
            push_guest_page(&mut out, core::ptr::addr_of!(GUEST_IMAGE_PTS[i].0));
        }
        push_guest_page(&mut out, core::ptr::addr_of!(GUEST_CODE_PT.0));
        push_bytes(
            &mut out,
            core::slice::from_raw_parts(
                core::ptr::addr_of!(VM1_GUEST_STACK.0).cast::<u8>(),
                GUEST_STACK_BYTES,
            ),
        );
        push_bytes(
            &mut out,
            core::slice::from_raw_parts(meta.code_base as *const u8, meta.code_len as usize),
        );
    }
    Ok(out)
}

pub fn restore_snapshot_bytes(bytes: &[u8]) -> Result<(), RestoreError> {
    let header_len = core::mem::size_of::<Vm1SnapshotHeader>();
    if bytes.len() < header_len {
        return Err(RestoreError::BadSnapshot);
    }

    let header = parse_snapshot_header(&bytes[..header_len])?;
    let expected = header_len
        + (GUEST_SNAPSHOT_PAGE_COUNT * PAGE_SIZE_4K)
        + (header.guest_stack_bytes as usize)
        + (header.code_len as usize);
    if bytes.len() < expected
        || header.guest_stack_bytes as usize != GUEST_STACK_BYTES
        || header.guest_page_bytes as usize != PAGE_SIZE_4K
    {
        return Err(RestoreError::BadSnapshot);
    }

    let mut off = header_len;
    unsafe {
        copy_into_guest_page(
            core::ptr::addr_of_mut!(GUEST_PML4.0),
            &bytes[off..off + PAGE_SIZE_4K],
        );
        off += PAGE_SIZE_4K;
        copy_into_guest_page(
            core::ptr::addr_of_mut!(GUEST_LOW_PDPT.0),
            &bytes[off..off + PAGE_SIZE_4K],
        );
        off += PAGE_SIZE_4K;
        copy_into_guest_page(
            core::ptr::addr_of_mut!(GUEST_LOW_PD.0),
            &bytes[off..off + PAGE_SIZE_4K],
        );
        off += PAGE_SIZE_4K;
        copy_into_guest_page(
            core::ptr::addr_of_mut!(GUEST_STACK_PT.0),
            &bytes[off..off + PAGE_SIZE_4K],
        );
        off += PAGE_SIZE_4K;
        copy_into_guest_page(
            core::ptr::addr_of_mut!(GUEST_HIGH_PDPT.0),
            &bytes[off..off + PAGE_SIZE_4K],
        );
        off += PAGE_SIZE_4K;
        copy_into_guest_page(
            core::ptr::addr_of_mut!(GUEST_HIGH_PD.0),
            &bytes[off..off + PAGE_SIZE_4K],
        );
        off += PAGE_SIZE_4K;
        for i in 0..GUEST_HIGH_IMAGE_PT_COUNT {
            copy_into_guest_page(
                core::ptr::addr_of_mut!(GUEST_IMAGE_PTS[i].0),
                &bytes[off..off + PAGE_SIZE_4K],
            );
            off += PAGE_SIZE_4K;
        }
        copy_into_guest_page(
            core::ptr::addr_of_mut!(GUEST_CODE_PT.0),
            &bytes[off..off + PAGE_SIZE_4K],
        );
        off += PAGE_SIZE_4K;

        core::ptr::copy_nonoverlapping(
            bytes[off..off + GUEST_STACK_BYTES].as_ptr(),
            core::ptr::addr_of_mut!(VM1_GUEST_STACK.0).cast::<u8>(),
            GUEST_STACK_BYTES,
        );
        off += GUEST_STACK_BYTES;
    }

    let code_end = off + header.code_len as usize;
    let live_code = unsafe {
        core::slice::from_raw_parts(header.code_base as *const u8, header.code_len as usize)
    };
    if live_code != &bytes[off..code_end] {
        return Err(RestoreError::CodeMismatch);
    }

    let guest_cr3 = current_guest_cr3_pa().map_err(|_| RestoreError::BadSnapshot)?;
    let restored = Vm1SnapshotMeta {
        guest_cr3,
        guest_rip: header.guest_rip,
        guest_rsp: header.guest_rsp,
        code_base: header.code_base,
        code_len: header.code_len,
        exit_reason: header.exit_reason,
        exit_qualification: header.exit_qualification,
        exit_guest_rip: header.exit_guest_rip,
    };
    *VM1_SNAPSHOT_META.lock() = Some(restored);
    *VM1_RESTORE_META.lock() = Some(restored);
    hvlogf(format_args!(
        "hv: vm1 reporting: restore armed guest_cr3=0x{:016X} guest_rip=0x{:016X} guest_rsp=0x{:016X}",
        restored.guest_cr3, restored.guest_rip, restored.guest_rsp
    ));
    Ok(())
}

fn parse_snapshot_header(bytes: &[u8]) -> Result<Vm1SnapshotHeader, RestoreError> {
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
    if magic != VM1_SNAPSHOT_MAGIC || version != VM1_SNAPSHOT_VERSION {
        return Err(RestoreError::BadSnapshot);
    }
    Ok(Vm1SnapshotHeader {
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
