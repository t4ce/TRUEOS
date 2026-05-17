//! Kernel-side virtual network helpers used by narrow C/POSIX ABI shims.

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
    let dev_idx = profile.resolve_device_index().unwrap_or(0);
    crate::t::block_on_io(super::dns::resolve_ipv4_for_device_sync_abi(
        dev_idx,
        host,
        super::dns::DnsConfig::for_profile(profile),
    ))
    .map_err(|_| DnsResolveError::Runtime)?
    .map_err(DnsResolveError::from)
}

fn resolve_ipv4_for_sync_abi_guest_vmcall(host: &str) -> Result<[u8; 4], DnsResolveError> {
    if host.is_empty() || host.as_bytes().len() > trueos_vm::vmcall::PAYLOAD_CAP {
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
