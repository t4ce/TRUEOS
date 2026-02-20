use alloc::format;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

use crate::shell::{ShellBackend, ShellIo};

pub(crate) const NETBENCH_URL: &str = "http://ipv6.download.thinkbroadband.com/5GB.zip";

fn format_speed(bps: u64) -> alloc::string::String {
    if bps < 100 {
        return format!("{} B/s", bps);
    }
    let kb = bps as f64 / 1024.0;
    if kb < 100.0 {
        return format!("{:.1} KB/s", kb);
    }
    let mb = kb / 1024.0;
    if mb < 100.0 {
        return format!("{:.1} MB/s", mb);
    }
    let gb = mb / 1024.0;
    if gb < 100.0 {
        return format!("{:.1} GB/s", gb);
    }
    let tb = gb / 1024.0;
    format!("{:.1} TB/s", tb)
}

pub(crate) async fn run_bench_fs(
    io: &dyn ShellBackend,
    disk: crate::disc::block::DeviceHandle,
    cols: usize,
    rows: usize,
    history: &mut alloc::vec::Vec<alloc::string::String>,
) {
    const BENCH_PATH: &str = "bench-lorem-100mb.txt";
    const BENCH_TOTAL_BYTES: u64 = 100 * 1024 * 1024;
    const UPDATE_MS: u64 = 100;
    const PATTERN: &[u8] = b"10101010";

    let rev_io = crate::shell::output::ReverseOutput::new(io, cols, rows, history);

    let slot = match crate::matrix::alloc_slot("fsbench") {
        Some(s) => s,
        None => {
            let _ = rev_io.write_str("bench: matrix full\n");
            return;
        }
    };

    // We cannot move slot into closure easily if we need it, so we replicate cleanup logic at return points.

    let _ = crate::shell::statusbar::set_active_slot(slot);
    let _ = crate::shell::statusbar::set_left(slot, "fsbench");
    let _ = crate::shell::statusbar::set_right(slot, "init");
    for i in 0..crate::shell::statusbar::INDICATOR_COUNT {
        let _ = crate::shell::statusbar::set_indicator(slot, i, 2);
    }
    crate::shell::statusbar::refresh(io, cols, rows);

    let Some(placement) = crate::v::fs::trueosfs::locate_async(disk)
        .await
        .ok()
        .flatten()
    else {
        let _ = rev_io.write_str("bench: selected disk is not TRUEOSFS\n");
        let _ = crate::shell::statusbar::set_active_slot(u8::MAX);
        let _ = crate::matrix::free_slot(slot);
        return;
    };
    if !disk.supports_write() {
        let _ = rev_io.write_str("bench: selected disk is read-only\n");
        let _ = crate::shell::statusbar::set_active_slot(u8::MAX);
        let _ = crate::matrix::free_slot(slot);
        return;
    }

    let _ = rev_io.write_fmt(format_args!(
        "bench: target={} super_lba={} data_lba={} file=/{}\n",
        disk.id(),
        placement.super_lba,
        placement.data_lba,
        BENCH_PATH
    ));
    let _ = rev_io.write_str("bench: writing 100MB fs stream (press any key to abort)\n");

    let info = disk.info();
    let bench_chunk_bytes = if info.max_transfer_bytes > 0 {
        let max_transfer = info.max_transfer_bytes as usize;
        core::cmp::max(4 * 1024, core::cmp::min(max_transfer, 1024 * 1024))
    } else {
        256 * 1024
    };

    let Some(stream_handle) =
        (match crate::v::fs::trueosfs::file_write_begin_async(disk, BENCH_PATH, BENCH_TOTAL_BYTES)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                let _ = rev_io.write_fmt(format_args!("bench: begin failed ({:?})\n", e));
                let _ = crate::shell::statusbar::set_active_slot(u8::MAX);
                let _ = crate::matrix::free_slot(slot);
                return;
            }
        })
    else {
        let _ = rev_io.write_str("bench: begin failed (no space / no placement)\n");
        let _ = crate::shell::statusbar::set_active_slot(u8::MAX);
        let _ = crate::matrix::free_slot(slot);
        return;
    };

    let mut chunk: alloc::vec::Vec<u8> = alloc::vec![0u8; bench_chunk_bytes];
    if !PATTERN.is_empty() {
        let mut off = 0usize;
        while off < chunk.len() {
            let take = core::cmp::min(PATTERN.len(), chunk.len() - off);
            chunk[off..off + take].copy_from_slice(&PATTERN[..take]);
            off = off.saturating_add(take);
        }
    }

    let mut written: u64 = 0;
    let mut aborted = false;
    let mut write_err: Option<crate::disc::block::Error> = None;
    let mut finished_ok = false;

    let start_tick = embassy_time_driver::now();
    let mut next_update = Instant::now() + EmbassyDuration::from_millis(UPDATE_MS);

    while written < BENCH_TOTAL_BYTES {
        if io.read_byte().is_some() {
            aborted = true;
            break;
        }

        let remaining = (BENCH_TOTAL_BYTES - written) as usize;
        let n = core::cmp::min(remaining, chunk.len());

        if let Err(e) =
            crate::v::fs::trueosfs::file_write_chunk_async(stream_handle, &chunk[..n]).await
        {
            write_err = Some(e);
            break;
        }
        written = written.saturating_add(n as u64);

        if Instant::now() >= next_update || written >= BENCH_TOTAL_BYTES {
            let now_tick = embassy_time_driver::now();
            let elapsed_ticks = now_tick.saturating_sub(start_tick);
            let hz = embassy_time_driver::TICK_HZ as u64;
            let elapsed_ms = if hz == 0 {
                0
            } else {
                elapsed_ticks.saturating_mul(1000) / hz
            };
            let bps = if elapsed_ms == 0 {
                0
            } else {
                written.saturating_mul(1000) / elapsed_ms
            };

            let spd = format_speed(bps);
            let _ = crate::shell::statusbar::set_right(slot, spd.as_str());
            crate::shell::statusbar::refresh(io, cols, rows);

            next_update = Instant::now() + EmbassyDuration::from_millis(UPDATE_MS);
        }
    }

    if write_err.is_none() && !aborted {
        match crate::v::fs::trueosfs::file_write_finish_async(stream_handle).await {
            Ok(()) => finished_ok = true,
            Err(e) => {
                let _ = crate::v::fs::trueosfs::file_write_abort_async(stream_handle).await;
                write_err = Some(e);
            }
        }
    } else {
        let _ = crate::v::fs::trueosfs::file_write_abort_async(stream_handle).await;
    }

    let _ = crate::v::fs::trueosfs::file_delete_async(disk, BENCH_PATH).await;

    if aborted {
        let _ = rev_io.write_str("bench: aborted by key press\n");
    } else if let Some(e) = write_err {
        let _ = rev_io.write_fmt(format_args!("bench: write failed ({:?})\n", e));
    } else if finished_ok {
        let _ = rev_io.write_str("bench: write complete\n");
    }

    let _ = crate::shell::statusbar::set_active_slot(u8::MAX);
    let _ = crate::matrix::free_slot(slot);
}

pub(crate) async fn run_netbench(
    io: &dyn ShellBackend,
    nic_index: usize,
    cols: usize,
    rows: usize,
    history: &mut alloc::vec::Vec<alloc::string::String>,
) {
    use alloc::{string::String as AString, vec::Vec};
    use core::net::{Ipv4Addr, Ipv6Addr};
    use trueos_v::vnet as api;

    const OPEN_TIMEOUT_MS: u64 = 4000;
    const OVERALL_TIMEOUT_MS: u64 = 120000;
    const UPDATE_MS: u64 = 100;
    const IDLE_YIELD_US: u64 = 100;

    let rev_io = crate::shell::output::ReverseOutput::new(io, cols, rows, history);

    enum HostTarget {
        Name(AString),
        V4([u8; 4]),
        V6([u8; 16]),
    }

    struct ParsedHttpUrl {
        host_header: AString,
        port: u16,
        path: AString,
        target: HostTarget,
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
                target: HostTarget::V6(ip6.octets()),
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

        // If the host portion is a literal IPv4 address, skip DNS.
        if let Ok(ip4) = host.parse::<Ipv4Addr>() {
            return Some(ParsedHttpUrl {
                host_header: if port == 80 {
                    AString::from(host)
                } else {
                    format!("{}:{}", host, port)
                },
                port,
                path,
                target: HostTarget::V4(ip4.octets()),
            });
        }

        Some(ParsedHttpUrl {
            host_header: if port == 80 {
                AString::from(host)
            } else {
                format!("{}:{}", host, port)
            },
            port,
            path,
            target: HostTarget::Name(AString::from(host)),
        })
    }

    fn find_http_header_end(buf: &[u8]) -> Option<usize> {
        buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
    }

    fn header_get_value<'a>(headers: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
        let mut i = 0usize;
        while i < headers.len() {
            let line_start = i;
            while i < headers.len() && headers[i] != b'\n' {
                i = i.saturating_add(1);
            }
            let mut line = &headers[line_start..i];
            if i < headers.len() && headers[i] == b'\n' {
                i = i.saturating_add(1);
            }
            if let Some((&b'\r', rest)) = line.split_last() {
                line = rest;
            }
            if line.is_empty() {
                continue;
            }
            let Some(colon) = line.iter().position(|b| *b == b':') else {
                continue;
            };
            let (k, mut v) = line.split_at(colon);
            v = v.get(1..).unwrap_or(&[]);
            if k.len() != name.len() {
                continue;
            }
            if !k
                .iter()
                .zip(name.iter())
                .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
            {
                continue;
            }
            while !v.is_empty() && (v[0] == b' ' || v[0] == b'\t') {
                v = &v[1..];
            }
            return Some(v);
        }
        None
    }

    fn parse_content_length(headers: &[u8]) -> Option<usize> {
        let v = header_get_value(headers, b"content-length")?;
        let s = core::str::from_utf8(v).ok()?;
        s.trim().parse::<usize>().ok()
    }

    let slot = match crate::matrix::alloc_slot("netbench") {
        Some(s) => s,
        None => {
            let _ = rev_io.write_str("netbench: matrix full\n");
            return;
        }
    };

    let _ = crate::shell::statusbar::set_active_slot(slot);
    let _ = crate::shell::statusbar::set_left(slot, "netbench");
    let _ = crate::shell::statusbar::set_right(slot, "resolving");
    for i in 0..crate::shell::statusbar::INDICATOR_COUNT {
        let _ = crate::shell::statusbar::set_indicator(slot, i, 2);
    }
    crate::shell::statusbar::refresh(io, cols, rows);

    let Some(parsed) = parse_http_url(NETBENCH_URL) else {
        let _ = rev_io.write_str("netbench: bad url\n");
        let _ = crate::shell::statusbar::set_active_slot(u8::MAX);
        let _ = crate::matrix::free_slot(slot);
        return;
    };

    let (host_header, port, path) = (parsed.host_header, parsed.port, parsed.path);

    let (ip4, ip6) = match parsed.target {
        HostTarget::V4(ip) => (Ok(ip), None),
        HostTarget::V6(ip) => (Err(crate::v::net::dns::DnsError::NoAnswer), Some(ip)),
        HostTarget::Name(host) => {
            let _ = rev_io.write_fmt(format_args!("netbench: resolving {}\n", host));
            let ip4 = crate::v::net::dns::resolve_ipv4_for_device(
                nic_index,
                host.as_str(),
                crate::v::net::dns::DnsConfig::for_device(nic_index),
            )
            .await;

            let ip6 = if ip4.is_err() {
                crate::v::net::dns::resolve_ipv6_for_device(
                    nic_index,
                    host.as_str(),
                    crate::v::net::dns::DnsConfig::for_device(nic_index),
                )
                .await
                .ok()
            } else {
                None
            };

            (ip4, ip6)
        }
    };

    if ip4.is_err() && ip6.is_none() {
        let _ = rev_io.write_str("netbench: resolve failed\n");
        let _ = crate::shell::statusbar::set_active_slot(u8::MAX);
        let _ = crate::matrix::free_slot(slot);
        return;
    }

    if let Ok(ip) = ip4 {
        let _ = rev_io.write_fmt(format_args!(
            "netbench: connecting to {}.{}.{}.{}\n",
            ip[0], ip[1], ip[2], ip[3]
        ));
    } else if let Some(ip) = ip6 {
        let _ = rev_io.write_fmt(format_args!(
            "netbench: connecting to ipv6 {:02x}{:02x}:{:02x}{:02x}:...\n",
            ip[0], ip[1], ip[2], ip[3]
        ));
    }
    let _ = crate::shell::statusbar::set_right(slot, "connecting");
    crate::shell::statusbar::refresh(io, cols, rows);

    let Some(vnet) = crate::v::net::VNet::open_with_event_queue_depth(nic_index, 4096) else {
        let _ = rev_io.write_str("netbench: vnet open failed\n");
        let _ = crate::shell::statusbar::set_active_slot(u8::MAX);
        let _ = crate::matrix::free_slot(slot);
        return;
    };

    let connect_ok = if let Ok(ip) = ip4 {
        vnet.submit(api::Command::OpenTcpConnect {
            remote: api::EndpointV4 { addr: ip, port },
        })
    } else if let Some(ip) = ip6 {
        vnet.submit(api::Command::OpenTcpConnectV6 {
            remote: api::EndpointV6 { addr: ip, port },
        })
    } else {
        Err(())
    };

    if connect_ok.is_err() {
        let _ = rev_io.write_str("netbench: tcp connect submit failed\n");
        let _ = crate::shell::statusbar::set_active_slot(u8::MAX);
        let _ = crate::matrix::free_slot(slot);
        return;
    }

    let mut aborted = false;
    let open_deadline = Instant::now() + EmbassyDuration::from_millis(OPEN_TIMEOUT_MS);
    let tcp_handle = loop {
        if io.read_byte().is_some() {
            aborted = true;
            break None;
        }
        if Instant::now() >= open_deadline {
            let _ = rev_io.write_str("netbench: connect timeout\n");
            break None;
        }
        if let Some(ev) = vnet.pop_event() {
            match ev {
                api::Event::Opened { handle, kind } if kind == api::SocketKind::Tcp => {
                    break Some(handle)
                }
                api::Event::Error { msg } => {
                    let _ = rev_io.write_fmt(format_args!("netbench: connect error: {:?}\n", msg));
                    break None;
                }
                _ => {}
            }
        } else {
            Timer::after(EmbassyDuration::from_millis(1)).await;
        }
    };

    let Some(tcp_handle) = tcp_handle else {
        if aborted {
            let _ = rev_io.write_str("netbench: aborted\n");
        }
        let _ = crate::shell::statusbar::set_active_slot(u8::MAX);
        let _ = crate::matrix::free_slot(slot);
        return;
    };

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS netbench\r\nAccept: */*\r\nConnection: close\r\n\r\n",
        path.as_str(),
        host_header.as_str()
    );
    if vnet
        .submit(api::Command::SendTcp {
            handle: tcp_handle,
            data: api::ByteBuf::from_slice_trunc(request.as_bytes()),
        })
        .is_err()
    {
        let _ = rev_io.write_str("netbench: send failed\n");
        let _ = vnet.submit(api::Command::Close { handle: tcp_handle });
        let _ = crate::shell::statusbar::set_active_slot(u8::MAX);
        let _ = crate::matrix::free_slot(slot);
        return;
    };

    let mut overall_deadline = Instant::now() + EmbassyDuration::from_millis(OVERALL_TIMEOUT_MS);
    let mut header_bytes: Vec<u8> = Vec::new();
    let mut header_done = false;
    let mut expected_len: Option<usize> = None;
    let mut received_bytes: usize = 0;
    let mut closed = false;

    let start_tick = embassy_time_driver::now();
    let mut next_update = Instant::now() + EmbassyDuration::from_millis(UPDATE_MS);

    let _ = rev_io.write_str("netbench: receiving data (press any key to abort)...\n");

    loop {
        if io.read_byte().is_some() {
            aborted = true;
            break;
        }
        if Instant::now() >= overall_deadline {
            let _ = rev_io.write_str("netbench: transfer timeout\n");
            break;
        }

        let mut got_event = false;
        while let Some(ev) = vnet.pop_event() {
            got_event = true;
            match ev {
                api::Event::TcpData { handle, data } if handle == tcp_handle => {
                    let bytes = data.as_slice();
                    if !header_done {
                        if header_bytes.len() + bytes.len() > 16 * 1024 {
                            let _ = rev_io.write_str("netbench: header too large\n");
                            closed = true;
                            break;
                        }
                        header_bytes.extend_from_slice(bytes);

                        if let Some(hend) = find_http_header_end(header_bytes.as_slice()) {
                            header_done = true;
                            expected_len = parse_content_length(&header_bytes[..hend]);
                            received_bytes += header_bytes.len() - hend;
                            if let Some(cl) = expected_len {
                                if received_bytes >= cl {
                                    closed = true;
                                    break;
                                }
                            }
                        }
                    } else {
                        received_bytes += bytes.len();
                        if let Some(cl) = expected_len {
                            if received_bytes >= cl {
                                closed = true;
                                break;
                            }
                        }
                    }
                    overall_deadline =
                        Instant::now() + EmbassyDuration::from_millis(OVERALL_TIMEOUT_MS);
                }
                api::Event::Closed { handle } if handle == tcp_handle => {
                    closed = true;
                    break;
                }
                api::Event::Error { msg } => {
                    let _ = rev_io.write_fmt(format_args!("netbench: socket error: {:?}\n", msg));
                    closed = true;
                    break;
                }
                _ => {}
            }
        }

        if closed {
            break;
        }

        if Instant::now() >= next_update {
            let now_tick = embassy_time_driver::now();
            let elapsed_ticks = now_tick.saturating_sub(start_tick);
            let hz = embassy_time_driver::TICK_HZ as u64;
            let elapsed_ms = if hz == 0 {
                0
            } else {
                elapsed_ticks.saturating_mul(1000) / hz
            };
            let bps = if elapsed_ms == 0 {
                0
            } else {
                (received_bytes as u64).saturating_mul(1000) / elapsed_ms
            };

            let spd = format_speed(bps);
            let _ = crate::shell::statusbar::set_right(slot, spd.as_str());
            crate::shell::statusbar::refresh(io, cols, rows);
            next_update = Instant::now() + EmbassyDuration::from_millis(UPDATE_MS);
        }

        if !got_event {
            Timer::after(EmbassyDuration::from_micros(IDLE_YIELD_US)).await;
        }
    }

    let _ = vnet.submit(api::Command::Close { handle: tcp_handle });

    if aborted {
        let _ = rev_io.write_str("netbench: aborted\n");
    } else {
        let now_tick = embassy_time_driver::now();
        let elapsed_ticks = now_tick.saturating_sub(start_tick);
        let hz = embassy_time_driver::TICK_HZ as u64;
        let elapsed_ms = if hz == 0 {
            0
        } else {
            elapsed_ticks.saturating_mul(1000) / hz
        };
        let bps = if elapsed_ms == 0 {
            0
        } else {
            (received_bytes as u64).saturating_mul(1000) / elapsed_ms
        };
        let _ = rev_io.write_fmt(format_args!(
            "netbench: done. {} bytes received ({}/s)\n",
            received_bytes,
            format_speed(bps)
        ));
    }

    let _ = crate::shell::statusbar::set_active_slot(u8::MAX);
    let _ = crate::matrix::free_slot(slot);
}
