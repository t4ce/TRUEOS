//! Native address resolution. Call hostname resolution from a background worker;
//! numeric IPv4 and IPv6 parsing needs no kernel call or blocking lane.
use crate::io;
use alloc::{vec, vec::Vec};
use core::net::{IpAddr, Ipv4Addr, SocketAddr};
unsafe extern "Rust" {
    fn trueos_platform_dns_resolve_ipv4(host: &str) -> Result<[u8; 4], u32>;
}
pub fn resolve_host(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let ip = unsafe { trueos_platform_dns_resolve_ipv4(host) }.map_err(|code| {
        let (kind, detail) = match code {
            1 => (
                io::ErrorKind::Other,
                "DNS requires a native background worker",
            ),
            2 => (io::ErrorKind::InvalidInput, "invalid DNS hostname"),
            3 => (
                io::ErrorKind::NotConnected,
                "DNS has no available network interface",
            ),
            4 => (io::ErrorKind::TimedOut, "DNS lookup timed out"),
            5 => (io::ErrorKind::NotFound, "DNS returned no IPv4 answer"),
            _ => (io::ErrorKind::Other, "native DNS failure"),
        };
        io::Error::new(kind, detail)
    })?;
    Ok(vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::from(ip)), port)])
}
pub fn resolve_socket_addr(value: &str) -> io::Result<Vec<SocketAddr>> {
    if let Ok(addr) = value.parse::<SocketAddr>() {
        return Ok(vec![addr]);
    }
    let (host, port) = value
        .rsplit_once(':')
        .ok_or_else(|| io::invalid_input("address has no port"))?;
    let port = port
        .parse::<u16>()
        .map_err(|_| io::invalid_input("address has an invalid port"))?;
    if host.is_empty() || host.contains(':') {
        return Err(io::invalid_input("address has an invalid hostname"));
    }
    resolve_host(host, port)
}
