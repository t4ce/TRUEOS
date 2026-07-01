//! Kernel-side vlayer helpers used by narrow runtime ABI shims.

extern crate alloc;

use alloc::string::String;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DnsResolveError {
    Runtime,
    BadName,
    NoNic,
    Timeout,
    NoAnswer,
}

impl From<super::dns::DnsError> for DnsResolveError {
    fn from(err: super::dns::DnsError) -> Self {
        match err {
            super::dns::DnsError::BadName => Self::BadName,
            super::dns::DnsError::NoNic => Self::NoNic,
            super::dns::DnsError::Timeout => Self::Timeout,
            super::dns::DnsError::NoAnswer => Self::NoAnswer,
            super::dns::DnsError::Runtime => Self::Runtime,
        }
    }
}

pub fn dns_resolve_error_code(err: DnsResolveError) -> u64 {
    match err {
        DnsResolveError::Runtime => 1,
        DnsResolveError::BadName => 2,
        DnsResolveError::NoNic => 3,
        DnsResolveError::Timeout => 4,
        DnsResolveError::NoAnswer => 5,
    }
}

pub fn dns_resolve_error_from_code(code: u64) -> DnsResolveError {
    match code {
        2 => DnsResolveError::BadName,
        3 => DnsResolveError::NoNic,
        4 => DnsResolveError::Timeout,
        5 => DnsResolveError::NoAnswer,
        _ => DnsResolveError::Runtime,
    }
}

pub fn resolve_ipv4_for_sync_abi(host: &str) -> Result<[u8; 4], DnsResolveError> {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return resolve_ipv4_for_sync_abi_guest_vmcall(host);
    }
    resolve_ipv4_for_sync_abi_host(host)
}

pub fn resolve_ipv4_for_sync_abi_host(host: &str) -> Result<[u8; 4], DnsResolveError> {
    let profile = crate::r::net::NetProfile::default();
    let dev_idx = profile
        .resolve_device_index()
        .ok_or(DnsResolveError::NoNic)?;
    let host = String::from(host);
    crate::wait::spawn_and_wait_local(async move {
        super::dns::resolve_ipv4_for_device(
            dev_idx,
            host.as_str(),
            super::dns::DnsConfig::for_profile(profile),
        )
        .await
    })
    .map_err(DnsResolveError::from)
}

fn resolve_ipv4_for_sync_abi_guest_vmcall(host: &str) -> Result<[u8; 4], DnsResolveError> {
    if host.is_empty() || host.len() > trueos_vm::vmcall::PAYLOAD_CAP {
        return Err(DnsResolveError::BadName);
    }
    let mut out = [0u8; 4];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_DNS_RESOLVE_IPV4,
        0,
        0,
        host.as_bytes(),
        &mut out,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return Err(DnsResolveError::Runtime);
    }
    if data != 0 {
        return Err(dns_resolve_error_from_code(data));
    }
    Ok(out)
}

pub fn rapl_snapshot_len_host() -> usize {
    crate::power::rapl::latest_snapshot_text().len()
}

pub fn rapl_snapshot_read_host(offset: usize, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }

    let text = crate::power::rapl::latest_snapshot_text();
    let bytes = text.as_bytes();
    if offset >= bytes.len() {
        return 0;
    }

    let n = core::cmp::min(out.len(), bytes.len() - offset);
    out[..n].copy_from_slice(&bytes[offset..offset + n]);
    n
}

pub fn rapl_history_len_host() -> usize {
    crate::power::rapl::history_len()
}

pub fn rapl_history_read_host(offset: usize, out: &mut [u8]) -> usize {
    crate::power::rapl::copy_history_slice(offset, out)
}

pub unsafe extern "C" fn trueos_vlayer_rapl_snapshot_read(
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    rapl_read_runtime(
        trueos_vm::vmcall::OP_BP_RAPL_SNAPSHOT_READ,
        rapl_snapshot_len_host,
        rapl_snapshot_read_host,
        offset,
        out_ptr,
        out_cap,
    )
}

pub unsafe extern "C" fn trueos_vlayer_rapl_history_read(
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    rapl_read_runtime(
        trueos_vm::vmcall::OP_BP_RAPL_HISTORY_READ,
        rapl_history_len_host,
        rapl_history_read_host,
        offset,
        out_ptr,
        out_cap,
    )
}

fn rapl_read_runtime(
    vmcall_op: u32,
    host_len: fn() -> usize,
    host_read: fn(usize, &mut [u8]) -> usize,
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if out_ptr.is_null() || out_cap == 0 {
        return if crate::hv::current_hull_guest_context_vm_id().is_some() {
            rapl_len_guest_vmcall(vmcall_op)
        } else {
            host_len() as isize
        };
    }

    let out = unsafe { core::slice::from_raw_parts_mut(out_ptr, out_cap) };
    let copied = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        rapl_read_guest_vmcall(vmcall_op, offset, out)
    } else {
        host_read(offset, out) as isize
    };
    copied
}

fn rapl_len_guest_vmcall(vmcall_op: u32) -> isize {
    let (status, len) = trueos_vm::vmcall::call(vmcall_op, 0, 0);
    if status == trueos_vm::vmcall::STATUS_OK {
        len as isize
    } else {
        -1
    }
}

fn rapl_read_guest_vmcall(vmcall_op: u32, offset: usize, out: &mut [u8]) -> isize {
    let mut copied = 0usize;
    while copied < out.len() {
        let chunk_cap = core::cmp::min(out.len() - copied, trueos_vm::vmcall::PAYLOAD_CAP);
        let (status, count) = trueos_vm::vmcall::call_with_payload(
            vmcall_op,
            offset.saturating_add(copied) as u64,
            chunk_cap as u64,
            &[],
            &mut out[copied..copied + chunk_cap],
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return -1;
        }
        let count = core::cmp::min(count as usize, chunk_cap);
        if count == 0 {
            break;
        }
        copied = copied.saturating_add(count);
    }
    copied as isize
}
