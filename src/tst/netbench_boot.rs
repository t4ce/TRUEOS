use alloc::{format, string::String as AString};
use core::net::{Ipv4Addr, Ipv6Addr};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

// Boot-time netbench runner.
//
// This version does NOT pull TCP payload bytes through vnet events (which copy
// into fixed-size ByteBuf chunks). It only resolves DNS and then submits an
// internal netbench run that counts bytes directly inside `NetService`.

const WAIT_FOR_NET_READY_MS: u64 = 15_000;

// Boot-time netbench target. IPv6-only.
const BOOT_NETBENCH_URL: &str = "http://ipv6.download.thinkbroadband.com/5GB.zip";
const BOOT_NETBENCH_URL_2: &str = "http://ash-speed.hetzner.com/10GB.bin";

enum Target {
    V6Literal([u8; 16]),
    Name(AString),
}

struct ParsedHttpUrl {
    host_header: AString,
    port: u16,
    path: AString,
    target: Target,
}

fn parse_http_url(url: &str) -> Option<ParsedHttpUrl> {
    let mut u = url.trim();
    if let Some(rest) = u.strip_prefix("http://") {
        u = rest;
    } else {
        return None;
    }

    let (hostport, path) = match u.split_once('/') {
        Some((a, b)) => (a, format!("/{}", b)),
        None => (u, AString::from("/")),
    };
    if hostport.is_empty() {
        return None;
    }

    // Support IPv6 literals in RFC 3986 bracket form: http://[2001:db8::1]:80/path
    // For literals, we skip DNS and connect directly.
    if let Some(rest) = hostport.strip_prefix('[') {
        let (inside, after) = rest.split_once(']')?;
        if inside.is_empty() {
            return None;
        }
        let ip6: Ipv6Addr = inside.parse().ok()?;
        let port = if after.is_empty() {
            80
        } else if let Some(p) = after.strip_prefix(':') {
            if !p.is_empty() && p.as_bytes().iter().all(|b| b.is_ascii_digit()) {
                p.parse::<u16>().ok()?
            } else {
                return None;
            }
        } else {
            return None;
        };
        let host_header = if port == 80 {
            format!("[{}]", inside)
        } else {
            format!("[{}]:{}", inside, port)
        };
        return Some(ParsedHttpUrl {
            host_header,
            port,
            path,
            target: Target::V6Literal(ip6.octets()),
        });
    }

    let (host, port) = if let Some((h, p)) = hostport.rsplit_once(':') {
        if !p.is_empty() && p.as_bytes().iter().all(|b| b.is_ascii_digit()) {
            (h, p.parse::<u16>().ok()?)
        } else {
            (hostport, 80)
        }
    } else {
        (hostport, 80)
    };

    if host.is_empty() {
        return None;
    }

    // If the host portion is a literal IPv4 address, treat it as a name but keep
    // Host header stable.
    if host.parse::<Ipv4Addr>().is_ok() {
        let host_header = if port == 80 {
            AString::from(host)
        } else {
            format!("{}:{}", host, port)
        };
        return Some(ParsedHttpUrl {
            host_header,
            port,
            path,
            target: Target::Name(AString::from(host)),
        });
    }

    let host_header = if port == 80 {
        AString::from(host)
    } else {
        format!("{}:{}", host, port)
    };

    Some(ParsedHttpUrl {
        host_header,
        port,
        path,
        target: Target::Name(AString::from(host)),
    })
}

#[embassy_executor::task]
pub async fn boot_netbench_task() {
    async move {
        if crate::net::device_count() == 0 {
            crate::log_trace!("boot-netbench: skipped (no NIC)\n");
            return;
        }

        // Ensure we start after IPv6 is actually usable for Internet egress.
        // Many routers advertise both a ULA (fd.. / fc..) and a global-unicast
        // prefix (2xxx..). For the public benchmark URL we require a
        // global-unicast address.
        let nic_index = crate::net::primary_device_index();
        let deadline = Instant::now() + EmbassyDuration::from_millis(WAIT_FOR_NET_READY_MS);
        loop {
            let ip6 = crate::net::adapter::ipv6_global_at(nic_index);
            if let Some(ip) = ip6
                && (ip[0] & 0xE0) == 0x20
            {
                crate::log_trace!(
                    "boot-netbench: ipv6_ready=1 dev={} ip6={:02x}{:02x}:{:02x}{:02x}:...\n",
                    nic_index,
                    ip[0],
                    ip[1],
                    ip[2],
                    ip[3]
                );
                break;
            }
            if Instant::now() >= deadline {
                crate::log_trace!(
                    "boot-netbench: ipv6_ready=0 (timeout) dev={}\n",
                    nic_index
                );
                return;
            }
            Timer::after(EmbassyDuration::from_millis(50)).await;
        }

        crate::log_trace!(
            "boot-netbench: starting dev={} name={}\n",
            nic_index,
            crate::net::device_name_at(nic_index).unwrap_or("Unknown")
        );

        // Launch two concurrent downloads to reduce the chance of a per-flow cap.
        // Combined throughput will be logged by the internal netbench runner.
        for (idx, url) in [(1u8, BOOT_NETBENCH_URL), (2u8, BOOT_NETBENCH_URL_2)] {
            let Some(parsed) = parse_http_url(url) else {
                crate::log_trace!("boot-netbench: bad url{}={}\n", idx, url);
                continue;
            };

            let (host_header, port, path) = (parsed.host_header, parsed.port, parsed.path);

            // Boot netbench is intentionally IPv6-only.
            let ip6: [u8; 16] = match parsed.target {
                Target::V6Literal(ip) => ip,
                Target::Name(host) => match crate::t::net::dns::resolve_ipv6_for_device(
                    nic_index,
                    host.as_str(),
                    crate::t::net::dns::DnsConfig::for_device(nic_index),
                )
                .await
                {
                    Ok(ip) => ip,
                    Err(e) => {
                        crate::log_trace!(
                            "boot-netbench: dns6 failed url{} host={} err={:?}\n",
                            idx,
                            host,
                            e
                        );
                        continue;
                    }
                },
            };

            let request = format!(
                "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS netbench\r\nAccept: */*\r\nConnection: close\r\n\r\n",
                path.as_str(),
                host_header.as_str()
            );

            let submitted = crate::net::adapter::internal_netbench_submit_v6(
                nic_index,
                ip6,
                port,
                request.as_bytes(),
            );

            if submitted {
                crate::log_trace!("boot-netbench: submit ok url{}={}\n", idx, url);
            } else {
                crate::log_trace!("boot-netbench: submit failed (queue full) url{}={}\n", idx, url);
            }
        }
    }
    .await;
}
