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

pub fn resolve_ipv4_for_sync_abi(host: &str) -> Result<[u8; 4], DnsResolveError> {
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
