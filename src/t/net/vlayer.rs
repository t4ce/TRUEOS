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
    crate::t::block_on_io(super::dns::resolve_ipv4_with_profile(
        host,
        crate::r::net::NetProfile::default(),
        super::dns::DnsConfig::default(),
    ))
    .map_err(|_| DnsResolveError::Runtime)?
    .map_err(DnsResolveError::from)
}
