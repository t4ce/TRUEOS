use embassy_executor::task;
use core::sync::atomic::{AtomicU32, Ordering};

fn is_hex(b: u8) -> bool {
    matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F')
}

fn hex4_lower(bytes: &[u8]) -> Option<[u8; 4]> {
    if bytes.len() != 4 || !bytes.iter().all(|&b| is_hex(b)) {
        return None;
    }
    let mut out = [0u8; 4];
    out.copy_from_slice(bytes);
    for b in &mut out {
        *b = b.to_ascii_lowercase();
    }
    Some(out)
}

fn trim_ascii_ws(mut s: &[u8]) -> &[u8] {
    while let Some((&b, rest)) = s.split_first() {
        if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
            s = rest;
        } else {
            break;
        }
    }
    while let Some((&b, rest)) = s.split_last() {
        if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
            s = rest;
        } else {
            break;
        }
    }
    s
}

fn trim_trailing_ascii_ws(mut s: &[u8]) -> &[u8] {
    while let Some((&b, rest)) = s.split_last() {
        if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
            s = rest;
        } else {
            break;
        }
    }
    s
}

fn collapse_ascii_ws_into(out: &mut alloc::vec::Vec<u8>, s: &[u8]) {
    let s = trim_ascii_ws(s);
    let mut prev_space = false;
    for &b in s {
        let is_ws = b == b' ' || b == b'\t' || b == b'\r' || b == b'\n';
        if is_ws {
            prev_space = true;
            continue;
        }
        if prev_space && !out.is_empty() && *out.last().unwrap() != b' ' {
            out.push(b' ');
        }
        prev_space = false;
        out.push(b);
    }
}

fn sanitize_pci_ids(raw: &[u8]) -> alloc::vec::Vec<u8> {
    // Goal: keep only vendor/device/subsystem entries with their indentation.
    // - drop blank lines and comments
    // - normalize indentation to 0/1/2 leading tabs
    // - normalize IDs to lowercase
    // - collapse whitespace in names
    use alloc::vec::Vec;

    let mut out: Vec<u8> = Vec::with_capacity(raw.len().min(4 * 1024 * 1024));

    let mut i: usize = 0;
    while i < raw.len() {
        // Find next line.
        let start = i;
        while i < raw.len() && raw[i] != b'\n' {
            i += 1;
        }
        let mut line = &raw[start..i];
        if i < raw.len() && raw[i] == b'\n' {
            i += 1;
        }
        // Strip a trailing '\r' from CRLF.
        if let Some((&b'\r', rest)) = line.split_last() {
            line = rest;
        }

        let line = trim_trailing_ascii_ws(line);
        if line.is_empty() {
            continue;
        }

        // Comment-only lines (allow leading whitespace).
        let mut k: usize = 0;
        while k < line.len() && (line[k] == b' ' || line[k] == b'\t') {
            k += 1;
        }
        if k >= line.len() {
            continue;
        }
        if line[k] == b'#' {
            continue;
        }

        // Indent is encoded as leading tabs in pci.ids.
        // Clamp to 0/1/2.
        let mut indent: usize = 0;
        let mut p: usize = 0;
        while p < line.len() && line[p] == b'\t' {
            indent += 1;
            p += 1;
        }
        let indent = indent.min(2);
        let rest = trim_ascii_ws(&line[p..]);
        if rest.is_empty() {
            continue;
        }

        // Skip non vendor/device/subsystem sections (e.g. classes starting with 'C').
        if indent == 0 {
            if rest.len() < 6 {
                continue;
            }
            let Some(id) = hex4_lower(&rest[..4]) else { continue };
            // Require whitespace after the vendor ID.
            if rest[4] != b' ' && rest[4] != b'\t' {
                continue;
            }
            let name = trim_ascii_ws(&rest[4..]);
            if name.is_empty() {
                continue;
            }
            out.extend_from_slice(&id);
            out.push(b' ');
            collapse_ascii_ws_into(&mut out, name);
            out.push(b'\n');
        } else if indent == 1 {
            if rest.len() < 6 {
                continue;
            }
            let Some(id) = hex4_lower(&rest[..4]) else { continue };
            if rest[4] != b' ' && rest[4] != b'\t' {
                continue;
            }
            let name = trim_ascii_ws(&rest[4..]);
            if name.is_empty() {
                continue;
            }
            out.push(b'\t');
            out.extend_from_slice(&id);
            out.push(b' ');
            collapse_ascii_ws_into(&mut out, name);
            out.push(b'\n');
        } else {
            // Subsystem lines: <subvendor> <subdevice> <name>
            // Example: "\t\t0ccd 0000  MN-Core 2 16GB"
            // Accept one or more whitespace separators.
            if rest.len() < 11 {
                continue;
            }
            let Some(subvendor) = hex4_lower(&rest[..4]) else { continue };
            if rest[4] != b' ' && rest[4] != b'\t' {
                continue;
            }
            let mut j = 4;
            while j < rest.len() && (rest[j] == b' ' || rest[j] == b'\t') {
                j += 1;
            }
            if j + 4 > rest.len() {
                continue;
            }
            let Some(subdevice) = hex4_lower(&rest[j..j + 4]) else { continue };
            j += 4;
            if j >= rest.len() || (rest[j] != b' ' && rest[j] != b'\t') {
                continue;
            }
            let name = trim_ascii_ws(&rest[j..]);
            if name.is_empty() {
                continue;
            }
            out.push(b'\t');
            out.push(b'\t');
            out.extend_from_slice(&subvendor);
            out.push(b' ');
            out.extend_from_slice(&subdevice);
            out.push(b' ');
            collapse_ascii_ws_into(&mut out, name);
            out.push(b'\n');
        }
    }

    out
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn hex4_to_u16(bytes: &[u8]) -> Option<u16> {
    if bytes.len() != 4 {
        return None;
    }
    let a = hex_nibble(bytes[0])? as u16;
    let b = hex_nibble(bytes[1])? as u16;
    let c = hex_nibble(bytes[2])? as u16;
    let d = hex_nibble(bytes[3])? as u16;
    Some((a << 12) | (b << 8) | (c << 4) | d)
}

/// Lookup a vendor name by vendor ID ($vid$) from a sanitized `pci.ids` blob.
///
/// Returns the vendor name bytes (typically UTF-8).
pub fn lookup_vendor_name_from_db<'a>(db: &'a [u8], vid: u16) -> Option<&'a [u8]> {
    let mut i: usize = 0;
    while i < db.len() {
        let start = i;
        while i < db.len() && db[i] != b'\n' {
            i += 1;
        }
        let mut line = &db[start..i];
        if i < db.len() && db[i] == b'\n' {
            i += 1;
        }
        if let Some((&b'\r', rest)) = line.split_last() {
            line = rest;
        }
        if line.is_empty() {
            continue;
        }
        if line[0] == b'\t' {
            continue;
        }
        if line.len() < 6 {
            continue;
        }
        let Some(id) = hex4_to_u16(&line[..4]) else {
            continue;
        };
        if line[4] != b' ' {
            continue;
        }
        if id == vid {
            let mut s = &line[5..];
            while !s.is_empty() && (s[0] == b' ' || s[0] == b'\t') {
                s = &s[1..];
            }
            return Some(s);
        }
    }
    None
}

/// Lookup a `(vendor_name, device_name)` tuple by vendor+device IDs.
///
/// Works on the sanitized `pci.ids` format produced by `sanitize_pci_ids()`:
/// - vendor lines: `vvvv <name>`
/// - device lines: `\tdddd <name>`
/// - subsystem lines are ignored here.
pub fn lookup_vendor_device_from_db<'a>(
    db: &'a [u8],
    vid: u16,
    did: u16,
) -> Option<(&'a [u8], &'a [u8])> {
    let mut i: usize = 0;
    let mut in_vendor = false;
    let mut seen_vendor = false;
    let mut vendor_name: Option<&'a [u8]> = None;

    while i < db.len() {
        let start = i;
        while i < db.len() && db[i] != b'\n' {
            i += 1;
        }
        let mut line = &db[start..i];
        if i < db.len() && db[i] == b'\n' {
            i += 1;
        }
        if let Some((&b'\r', rest)) = line.split_last() {
            line = rest;
        }
        if line.is_empty() {
            continue;
        }

        // Determine indent (0/1/2 tabs) and the remaining payload.
        let mut p: usize = 0;
        while p < line.len() && line[p] == b'\t' {
            p += 1;
        }
        let indent = core::cmp::min(p, 2);
        let rest = &line[p..];

        if indent == 0 {
            if seen_vendor {
                // We already passed the matching vendor section without finding the device.
                return None;
            }
            if rest.len() < 6 {
                continue;
            }
            let Some(v) = hex4_to_u16(&rest[..4]) else { continue };
            if rest[4] != b' ' {
                continue;
            }
            if v == vid {
                in_vendor = true;
                seen_vendor = true;
                vendor_name = Some(&rest[5..]);
            } else {
                in_vendor = false;
                vendor_name = None;
            }
            continue;
        }

        if indent == 1 {
            if !in_vendor {
                continue;
            }
            let vend = vendor_name?;
            if rest.len() < 6 {
                continue;
            }
            let Some(d) = hex4_to_u16(&rest[..4]) else { continue };
            if rest[4] != b' ' {
                continue;
            }
            if d == did {
                return Some((vend, &rest[5..]));
            }
            continue;
        }
    }
    None
}

/// Convenience: read the cached database and do a vendor+device lookup.
pub fn lookup_vendor_device_cached(
    vid: u16,
    did: u16,
) -> Result<Option<(alloc::string::String, alloc::string::String)>, crate::surface::io::kfs::FsError> {
    use alloc::string::String;

    const PATH: &str = "/trueos/pci/pci.ids";
    let db = crate::surface::io::kfs::read_file(PATH)?;

    let Some((v, d)) = lookup_vendor_device_from_db(&db, vid, did) else {
        return Ok(None);
    };

    // Best-effort UTF-8 conversion for logs/UI.
    let v = String::from_utf8_lossy(v).into_owned();
    let d = String::from_utf8_lossy(d).into_owned();
    Ok(Some((v, d)))
}

fn log_pci_enumeration_with_cached_ids(db: &[u8]) {
    // Re-enumerate here so the list reflects the system state after init.
    // (Enumeration is cheap and uses the same static cache the shell relies on.)
    crate::pci::enumerate_silent();
    crate::pci::log_devices_with_pci_ids(db);
}

static PCIIDS_FETCH_SEQ: AtomicU32 = AtomicU32::new(1);

fn leak_str(s: alloc::string::String) -> &'static str {
    alloc::boxed::Box::leak(s.into_boxed_str())
}

fn find_http_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn parse_http_status(headers: &[u8]) -> Option<u16> {
    let line_end = headers.windows(2).position(|w| w == b"\r\n")?;
    let line = &headers[..line_end];
    let mut parts = line.split(|&b| b == b' ');
    let _http = parts.next()?;
    let code = parts.next()?;
    core::str::from_utf8(code).ok()?.parse::<u16>().ok()
}

fn header_value<'a>(headers: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    for line in headers.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let Some(colon) = line.iter().position(|&b| b == b':') else { continue };
        let (k, rest) = line.split_at(colon);
        let Some(v) = rest.get(1..) else { continue };
        if k.eq_ignore_ascii_case(name) {
            let v = v.strip_prefix(b" ").unwrap_or(v);
            return Some(v);
        }
    }
    None
}

fn is_chunked(headers: &[u8]) -> bool {
    header_value(headers, b"transfer-encoding")
    .map(|v: &[u8]| v.to_ascii_lowercase().windows(7).any(|w| w == b"chunked"))
        .unwrap_or(false)
}

fn content_length(headers: &[u8]) -> Option<usize> {
    let v = header_value(headers, b"content-length")?;
    core::str::from_utf8(v).ok()?.trim().parse::<usize>().ok()
}

fn try_decode_chunked_body(body: &[u8], max_bytes: usize) -> Option<alloc::vec::Vec<u8>> {
    let mut out: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let mut i: usize = 0;
    loop {
        let line_end = body.get(i..)?.windows(2).position(|w| w == b"\r\n")?;
        let line = &body[i..i + line_end];
        let line = line.split(|&b| b == b';').next().unwrap_or(line);
        let size = usize::from_str_radix(core::str::from_utf8(line).ok()?.trim(), 16).ok()?;
        i = i.saturating_add(line_end + 2);
        if size == 0 {
            return Some(out);
        }
        if i + size + 2 > body.len() {
            return None;
        }
        if out.len().saturating_add(size) > max_bytes {
            return None;
        }
        out.extend_from_slice(&body[i..i + size]);
        i += size;
        if body.get(i..i + 2)? != b"\r\n" {
            return None;
        }
        i += 2;
    }
}

fn dns_query(id: u16, host: &str) -> alloc::vec::Vec<u8> {
    let mut out: alloc::vec::Vec<u8> = alloc::vec::Vec::with_capacity(64);
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&0x0100u16.to_be_bytes()); // recursion desired
    out.extend_from_slice(&1u16.to_be_bytes()); // qdcount
    out.extend_from_slice(&0u16.to_be_bytes()); // ancount
    out.extend_from_slice(&0u16.to_be_bytes()); // nscount
    out.extend_from_slice(&0u16.to_be_bytes()); // arcount

    for label in host.split('.') {
        let b = label.as_bytes();
        let n = core::cmp::min(63, b.len());
        out.push(n as u8);
        out.extend_from_slice(&b[..n]);
    }
    out.push(0);
    out.extend_from_slice(&1u16.to_be_bytes()); // A
    out.extend_from_slice(&1u16.to_be_bytes()); // IN
    out
}

fn dns_skip_name(pkt: &[u8], idx: &mut usize) -> bool {
    let mut jumps: u8 = 0;
    loop {
        if *idx >= pkt.len() {
            return false;
        }
        let b = pkt[*idx];
        if b & 0xC0 == 0xC0 {
            if *idx + 1 >= pkt.len() {
                return false;
            }
            *idx += 2;
            return true;
        }
        if b == 0 {
            *idx += 1;
            return true;
        }
        let n = b as usize;
        *idx += 1;
        if *idx + n > pkt.len() {
            return false;
        }
        *idx += n;
        jumps = jumps.saturating_add(1);
        if jumps > 64 {
            return false;
        }
    }
}

fn dns_parse_first_a(pkt: &[u8], id: u16) -> Option<[u8; 4]> {
    if pkt.len() < 12 {
        return None;
    }
    if u16::from_be_bytes([pkt[0], pkt[1]]) != id {
        return None;
    }
    let qd = u16::from_be_bytes([pkt[4], pkt[5]]) as usize;
    let an = u16::from_be_bytes([pkt[6], pkt[7]]) as usize;
    let mut idx: usize = 12;

    for _ in 0..qd {
        if !dns_skip_name(pkt, &mut idx) {
            return None;
        }
        if idx + 4 > pkt.len() {
            return None;
        }
        idx += 4;
    }

    for _ in 0..an {
        if !dns_skip_name(pkt, &mut idx) {
            return None;
        }
        if idx + 10 > pkt.len() {
            return None;
        }
        let typ = u16::from_be_bytes([pkt[idx], pkt[idx + 1]]);
        let class = u16::from_be_bytes([pkt[idx + 2], pkt[idx + 3]]);
        let rdlen = u16::from_be_bytes([pkt[idx + 8], pkt[idx + 9]]) as usize;
        idx += 10;
        if idx + rdlen > pkt.len() {
            return None;
        }
        if class == 1 && typ == 1 && rdlen == 4 {
            return Some([pkt[idx], pkt[idx + 1], pkt[idx + 2], pkt[idx + 3]]);
        }
        idx += rdlen;
    }
    None
}

async fn resolve_ipv4_async(dev_idx: usize, host: &str, timeout_ms: u64) -> Result<[u8; 4], i32> {
    use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
    use crate::surface::io::cabi::{NET_ERR_TIMEOUT_DNS};
    use crate::v::net::VNet;
    use trueos_v::vnet;

    // Fast path: IPv4 literal.
    if let Ok(ip) = host.parse::<core::net::Ipv4Addr>() {
        return Ok(ip.octets());
    }

    const DNS_PORT: u16 = 53;
    const SLIRP_DNS_IP: [u8; 4] = [10, 0, 2, 3];

    let dns_id: u16 = 0xD500u16.wrapping_add(dev_idx as u16);
    let net = VNet::open(dev_idx).ok_or(NET_ERR_TIMEOUT_DNS)?;

    let local_port: u16 = 54000u16
        .wrapping_add((dev_idx as u16).wrapping_mul(23))
        .wrapping_add((dns_id as u16) & 0x3FF);
    let _ = net.submit(vnet::Command::OpenUdp { port: local_port });

    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms.max(250));
    let mut udp: Option<vnet::NetHandle> = None;
    let mut sent = false;

    loop {
        for _ in 0..64 {
            let Some(ev) = net.pop_event() else {
                break;
            };
            match ev {
                vnet::Event::Opened { handle, kind } => {
                    if kind == vnet::SocketKind::Udp {
                        udp = Some(handle);
                    }
                }
                vnet::Event::UdpPacket { handle, from, data } => {
                    if udp != Some(handle) {
                        continue;
                    }
                    if from.port == DNS_PORT {
                        if let Some(ip) = dns_parse_first_a(data.as_slice(), dns_id) {
                            let _ = net.submit(vnet::Command::Close { handle });
                            return Ok(ip);
                        }
                    }
                }
                _ => {}
            }
        }

        if !sent {
            if let Some(handle) = udp {
                let q = dns_query(dns_id, host);
                let _ = net.submit(vnet::Command::SendUdp {
                    handle,
                    remote: vnet::EndpointV4 { addr: SLIRP_DNS_IP, port: DNS_PORT },
                    data: vnet::ByteBuf::from_slice_trunc(&q),
                });
                for &server in &[[1, 1, 1, 1], [8, 8, 8, 8]] {
                    let _ = net.submit(vnet::Command::SendUdp {
                        handle,
                        remote: vnet::EndpointV4 { addr: server, port: DNS_PORT },
                        data: vnet::ByteBuf::from_slice_trunc(&q),
                    });
                }
                sent = true;
            }
        }

        if Instant::now() >= deadline {
            if let Some(handle) = udp {
                let _ = net.submit(vnet::Command::Close { handle });
            }
            return Err(NET_ERR_TIMEOUT_DNS);
        }

        Timer::after(EmbassyDuration::from_millis(25)).await;
    }
}

async fn fetch_https_body_async(url: &str, timeout_ms: u64, max_bytes: usize) -> Result<alloc::vec::Vec<u8>, i32> {
    use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
    use crate::surface::io::cabi::{NET_ERR_BAD_URL, NET_ERR_HTTP, NET_ERR_TIMEOUT, NET_ERR_TLS};
    use crate::net::tls_socket::{register_tls_app_queues, TlsCommand, TlsEvent};
    use crate::net::tls::{TlsClientConfig, TlsRoots};
    use crate::v::net::Queue;
    use trueos_v::vnet;

    let rest = url.strip_prefix("https://").ok_or(NET_ERR_BAD_URL)?;
    let (host, path_rest) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.trim();
    if host.is_empty() {
        return Err(NET_ERR_BAD_URL);
    }
    let host_sni: &'static str = leak_str(alloc::string::String::from(host));
    let path = alloc::format!("/{}", path_rest);

    let dev_idx: usize = 0;
    let ip = resolve_ipv4_async(dev_idx, host, core::cmp::min(timeout_ms, 20_000)).await?;

    let seq = PCIIDS_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let owner = leak_str(alloc::format!("pciids-https-{}@{}", seq, dev_idx));
    let cmds_name = leak_str(alloc::format!("{}-tls-cmd", owner));
    let evts_name = leak_str(alloc::format!("{}-tls-evt", owner));
    let cmds = Queue::new_leaked(cmds_name, 512);
    let events = Queue::new_leaked(evts_name, 512);
    register_tls_app_queues(owner, cmds, events);

    let roots = TlsRoots::mozilla();
    let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);

    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms.max(1));
    let mut tls_handle: Option<vnet::NetHandle> = None;
    let mut sent_connect = false;
    let mut http_sent = false;
    let mut plaintext: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    loop {
        for ev in events.drain(64) {
            match ev {
                TlsEvent::Opened { handle } => {
                    tls_handle = Some(handle);
                }
                TlsEvent::Connected { handle } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if !http_sent {
                        let req = alloc::format!(
                            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS pciids\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
                            path,
                            host
                        );
                        let _ = cmds.push(TlsCommand::Send { handle, data: req.into_bytes() });
                        http_sent = true;
                    }
                }
                TlsEvent::Data { handle, data } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if plaintext.len() < max_bytes {
                        let room = max_bytes - plaintext.len();
                        let take = data.len().min(room);
                        plaintext.extend_from_slice(&data[..take]);
                    }
                }
                TlsEvent::Closed { handle } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    let Some(hdr_end) = find_http_header_end(&plaintext) else {
                        return Err(NET_ERR_HTTP);
                    };
                    let headers = &plaintext[..hdr_end];
                    let status = parse_http_status(headers).unwrap_or(0);
                    if status != 200 {
                        return Err(NET_ERR_HTTP);
                    }
                    let body = &plaintext[hdr_end..];
                    if is_chunked(headers) {
                        if let Some(decoded) = try_decode_chunked_body(body, max_bytes.saturating_sub(hdr_end)) {
                            return Ok(decoded);
                        }
                        return Ok(body.to_vec());
                    }
                    if let Some(len) = content_length(headers) {
                        return Ok(body.get(..len).unwrap_or(body).to_vec());
                    }
                    return Ok(body.to_vec());
                }
                TlsEvent::TlsError { .. } => return Err(NET_ERR_TLS),
                TlsEvent::Error { .. } => {}
            }
        }

        if !sent_connect {
            let _ = cmds.push(TlsCommand::OpenTcpConnect {
                remote: vnet::EndpointV4 { addr: ip, port: 443 },
                server_name: host_sni,
                cfg: cfg.clone(),
                roots: roots.clone(),
            });
            sent_connect = true;
        }

        if Instant::now() >= deadline {
            if let Some(h) = tls_handle {
                let _ = cmds.push(TlsCommand::Close { handle: h });
            }
            return Err(NET_ERR_TIMEOUT);
        }

        Timer::after(EmbassyDuration::from_millis(25)).await;
    }
}

/// Fetch and cache the `pci.ids` database on the USBMS FAT filesystem.
///
/// The download is skipped if the destination file already exists.
#[task]
pub(crate) async fn boot_cache_pci_ids_task() {
    use embassy_time::Timer;

    // Source: pciutils/pciids
    const URL: &str = "https://raw.githubusercontent.com/pciutils/pciids/master/pci.ids";

    // Persistent cache location on USBMS FAT.
    //
    // Requirement: keep this under the `/trueos/pci/` folder.
    const PATH: &str = "/trueos/pci/pci.ids";

    // Previous cache locations we may have used in older builds.
    const OLD_PATHS: [&str; 3] = [
        "/trueos/src/pci/pci.ids",
        "/trueos/pci.ids",
        "/§/pci.ids",
    ];

    // Retry: USBMS/FAT may not be ready when the executor starts.
    for attempt in 1..=60u32 {
        match crate::surface::io::kfs::exists_async(PATH).await {
            Ok(true) => {
                crate::log!("pciids: cache hit path={}\n", PATH);
                if let Ok(db) = crate::surface::io::kfs::read_file_async(PATH).await {
                    log_pci_enumeration_with_cached_ids(&db);
                }
                return;
            }
            Ok(false) => {}
            Err(_) => {}
        }

        // One-time migration from old locations (avoid redownload after upgrades).
        // We also sanitize during migration so the persistent cache stays normalized.
        for old in OLD_PATHS {
            match crate::surface::io::kfs::exists_async(old).await {
                Ok(true) => {
                    if let Some((parent, _name)) = PATH.rsplit_once('/') {
                        if !parent.is_empty() {
                            let _ = crate::surface::io::kfs::create_dir_all(parent);
                        }
                    }

                    if let Ok(raw) = crate::surface::io::kfs::read_file_async(old).await {
                        let cleaned = sanitize_pci_ids(&raw);
                        let tmp = alloc::format!("{}.tmp", PATH);
                        if crate::surface::io::kfs::write_file_async(tmp.as_str(), &cleaned)
                            .await
                            .is_ok()
                            && crate::surface::io::kfs::rename_async(tmp.as_str(), PATH)
                                .await
                                .is_ok()
                        {
                            let _ = crate::surface::io::kfs::remove_async(old).await;
                            crate::log!(
                                "pciids: migrated+sanitized old={} new={} bytes_in={} bytes_out={}\n",
                                old,
                                PATH,
                                raw.len(),
                                cleaned.len(),
                            );
                            if let Ok(db) = crate::surface::io::kfs::read_file_async(PATH).await {
                                log_pci_enumeration_with_cached_ids(&db);
                            }
                            return;
                        }
                        let _ = crate::surface::io::kfs::remove_async(tmp.as_str()).await;
                    }
                }
                Ok(false) => {}
                Err(_) => {}
            }
        }

        // Ensure the cache directory exists before downloading.
        // If USBMS/FAT isn't ready yet, don't waste network bandwidth.
        if let Some((parent, _name)) = PATH.rsplit_once('/') {
            if !parent.is_empty() {
                if let Err(e) = crate::surface::io::kfs::create_dir_all(parent) {
                    crate::log!(
                        "pciids: attempt={} fs_not_ready={:?} url={} path={}\n",
                        attempt,
                        e,
                        URL,
                        PATH
                    );
                    Timer::after_millis(500).await;
                    continue;
                }
            }
        }

        let raw = match crate::surface::io::cabi::net_fetch_https_body_async(URL, 30_000, 4 * 1024 * 1024).await {
            Ok(b) => b,
            Err(rc) => {
                crate::log!(
                    "pciids: attempt={} rc={} ({}) url={} path={}\n",
                    attempt,
                    rc,
                    crate::surface::io::cabi::code_name(rc),
                    URL,
                    PATH
                );
                Timer::after_millis(500).await;
                continue;
            }
        };

        let cleaned = sanitize_pci_ids(&raw);
        let tmp = alloc::format!("{}.tmp", PATH);
        let write_res = crate::surface::io::kfs::write_file_async(tmp.as_str(), &cleaned).await;
        let rename_res = match write_res {
            Ok(()) => crate::surface::io::kfs::rename_async(tmp.as_str(), PATH).await,
            Err(e) => Err(e),
        };
        if rename_res.is_ok() {
            crate::log!(
                "pciids: downloaded+sanitized ok url={} path={} bytes_in={} bytes_out={}\n",
                URL,
                PATH,
                raw.len(),
                cleaned.len(),
            );
            if let Ok(db) = crate::surface::io::kfs::read_file_async(PATH).await {
                log_pci_enumeration_with_cached_ids(&db);
            }
            return;
        }

        let _ = crate::surface::io::kfs::remove_async(tmp.as_str()).await;
        crate::log!(
            "pciids: attempt={} write_failed={:?} rename_failed={:?} url={} path={}\n",
            attempt,
            write_res.err(),
            rename_res.err(),
            URL,
            PATH
        );
        Timer::after_millis(500).await;
    }

    crate::log!("pciids: giving up after retries url={} path={}\n", URL, PATH);
}
