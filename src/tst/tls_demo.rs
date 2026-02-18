extern crate alloc;

use alloc::string::ToString;
use alloc::{boxed::Box, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String as HString;

use trueos_v::vnet;

use crate::net::tls::{TlsClientConfig, TlsRoots};
use crate::net::tls_socket::{register_tls_app_queues, TlsCommand, TlsEvent};
use crate::v::net::dns::{self, DnsConfig};
use crate::v::net::Queue;

// Default host for the demo.
// NOTE: We now resolve via the slirp DNS server so the demo is resilient to
// hard-coded IP issues and better reflects real HTTPS usage.
const DEMO_HOST: &str = "example.com";
const DEMO_PORT: u16 = 443;

fn find_http_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn parse_http_status(buf: &[u8]) -> Option<u16> {
    // Expect: HTTP/1.1 200 ...\r\n
    if !buf.starts_with(b"HTTP/") {
        return None;
    }
    let mut i = 0;
    while i < buf.len() && buf[i] != b' ' {
        i += 1;
    }
    if i + 4 >= buf.len() {
        return None;
    }
    if buf[i] != b' ' {
        return None;
    }
    let d1 = buf.get(i + 1)?.wrapping_sub(b'0');
    let d2 = buf.get(i + 2)?.wrapping_sub(b'0');
    let d3 = buf.get(i + 3)?.wrapping_sub(b'0');
    if d1 > 9 || d2 > 9 || d3 > 9 {
        return None;
    }
    Some((d1 as u16) * 100 + (d2 as u16) * 10 + (d3 as u16))
}

fn ascii_lower(b: u8) -> u8 {
    if (b'A'..=b'Z').contains(&b) {
        b + 32
    } else {
        b
    }
}

fn header_get_value<'a>(headers: &'a [u8], header_name: &[u8]) -> Option<&'a [u8]> {
    // Returns the raw value slice (trimmed of leading spaces/tabs and trailing \r).
    // Case-insensitive ASCII match on header name.
    let mut i = 0;
    while i < headers.len() {
        let mut j = i;
        while j < headers.len() && headers[j] != b'\n' {
            j += 1;
        }
        let line = &headers[i..j.min(headers.len())];
        i = (j + 1).min(headers.len());

        let Some(colon) = line.iter().position(|&b| b == b':') else {
            continue;
        };
        let name = &line[..colon];
        if name.len() != header_name.len() {
            continue;
        }
        if !name
            .iter()
            .zip(header_name.iter())
            .all(|(&a, &b)| ascii_lower(a) == ascii_lower(b))
        {
            continue;
        }

        let mut k = colon + 1;
        while k < line.len() && (line[k] == b' ' || line[k] == b'\t') {
            k += 1;
        }
        let mut v = &line[k..];
        if v.ends_with(b"\r") {
            v = &v[..v.len() - 1];
        }
        return Some(v);
    }
    None
}

#[derive(Clone)]
struct RedirectTarget {
    host: &'static str,
    port: u16,
    path: &'static str,
}

fn parse_redirect_target(current: &RedirectTarget, location: &[u8]) -> Option<RedirectTarget> {
    let loc = core::str::from_utf8(location).ok()?.trim();
    if loc.is_empty() {
        return None;
    }

    // Relative redirect: "/path"
    if loc.starts_with('/') {
        return Some(RedirectTarget {
            host: current.host,
            port: current.port,
            path: leak_str(loc.to_string()),
        });
    }

    // Scheme-relative: "//host/path"
    let loc = if let Some(rest) = loc.strip_prefix("//") {
        alloc::format!("https://{}", rest)
    } else {
        loc.to_string()
    };

    // Only follow HTTPS redirects.
    let rest = loc.strip_prefix("https://")?;

    let (authority, path) = match rest.find('/') {
        Some(pos) => (&rest[..pos], &rest[pos..]),
        None => (rest, "/"),
    };

    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() && !p.is_empty() => {
            let port = p.parse::<u16>().ok()?;
            (h, port)
        }
        _ => (authority, 443),
    };

    if host.is_empty() {
        return None;
    }

    Some(RedirectTarget {
        host: leak_str(host.to_string()),
        port,
        path: leak_str(path.to_string()),
    })
}

fn header_contains_token(headers: &[u8], header_name: &[u8], token: &[u8]) -> bool {
    // Very small ASCII-only, case-insensitive check.
    let mut i = 0;
    while i < headers.len() {
        // find end of line
        let mut j = i;
        while j < headers.len() && headers[j] != b'\n' {
            j += 1;
        }
        let line = &headers[i..j.min(headers.len())];
        i = (j + 1).min(headers.len());

        // split "Name: value"
        let Some(colon) = line.iter().position(|&b| b == b':') else {
            continue;
        };
        let name = &line[..colon];
        if name.len() != header_name.len() {
            continue;
        }
        if !name
            .iter()
            .zip(header_name.iter())
            .all(|(&a, &b)| ascii_lower(a) == ascii_lower(b))
        {
            continue;
        }

        // scan value for token
        let mut k = colon + 1;
        while k < line.len() && (line[k] == b' ' || line[k] == b'\t') {
            k += 1;
        }
        let value = &line[k..];

        // case-insensitive substring search
        if token.is_empty() {
            return true;
        }
        'outer: for start in 0..=value.len().saturating_sub(token.len()) {
            for off in 0..token.len() {
                if ascii_lower(value[start + off]) != ascii_lower(token[off]) {
                    continue 'outer;
                }
            }
            return true;
        }
    }
    false
}

fn header_value_contains_token(value: &[u8], token: &[u8]) -> bool {
    // ASCII-only case-insensitive substring search.
    if token.is_empty() {
        return true;
    }
    if value.len() < token.len() {
        return false;
    }

    'outer: for start in 0..=value.len().saturating_sub(token.len()) {
        for off in 0..token.len() {
            if ascii_lower(value[start + off]) != ascii_lower(token[off]) {
                continue 'outer;
            }
        }
        return true;
    }
    false
}

fn decode_gzip(body: &[u8], max_out: usize) -> Option<Vec<u8>> {
    // Minimal gzip wrapper handling (RFC 1952), without CRC validation.
    // Returns the decompressed payload (deflate stream) on success.
    if body.len() < 18 {
        return None;
    }
    if body[0] != 0x1f || body[1] != 0x8b {
        return None;
    }
    // Compression method: deflate (8)
    if body[2] != 8 {
        return None;
    }

    let flags = body[3];
    let mut pos: usize = 10;
    let len = body.len();

    // FEXTRA
    if (flags & 0x04) != 0 {
        if pos + 2 > len {
            return None;
        }
        let xlen = u16::from_le_bytes([body[pos], body[pos + 1]]) as usize;
        pos += 2;
        if pos + xlen > len {
            return None;
        }
        pos += xlen;
    }

    // FNAME
    if (flags & 0x08) != 0 {
        while pos < len && body[pos] != 0 {
            pos += 1;
        }
        pos = pos.saturating_add(1);
        if pos > len {
            return None;
        }
    }

    // FCOMMENT
    if (flags & 0x10) != 0 {
        while pos < len && body[pos] != 0 {
            pos += 1;
        }
        pos = pos.saturating_add(1);
        if pos > len {
            return None;
        }
    }

    // FHCRC
    if (flags & 0x02) != 0 {
        pos = pos.saturating_add(2);
        if pos > len {
            return None;
        }
    }

    // Trailer is 8 bytes: CRC32 + ISIZE
    if pos + 8 > len {
        return None;
    }
    let deflate_end = len.saturating_sub(8);
    if deflate_end < pos {
        return None;
    }
    let deflate_data = &body[pos..deflate_end];

    miniz_oxide::inflate::decompress_to_vec_with_limit(deflate_data, max_out).ok()
}

fn header_parse_content_length(headers: &[u8]) -> Option<usize> {
    // Looks for Content-Length: <digits>
    let mut i = 0;
    while i < headers.len() {
        let mut j = i;
        while j < headers.len() && headers[j] != b'\n' {
            j += 1;
        }
        let line = &headers[i..j.min(headers.len())];
        i = (j + 1).min(headers.len());

        let Some(colon) = line.iter().position(|&b| b == b':') else {
            continue;
        };
        let name = &line[..colon];
        const CL: &[u8] = b"content-length";
        if name.len() != CL.len()
            || !name
                .iter()
                .zip(CL.iter())
                .all(|(&a, &b)| ascii_lower(a) == b)
        {
            continue;
        }
        let mut k = colon + 1;
        while k < line.len() && (line[k] == b' ' || line[k] == b'\t') {
            k += 1;
        }
        let mut n: usize = 0;
        let mut any = false;
        while k < line.len() {
            let b = line[k];
            if !(b'0'..=b'9').contains(&b) {
                break;
            }
            any = true;
            n = n.saturating_mul(10).saturating_add((b - b'0') as usize);
            k += 1;
        }
        return any.then_some(n);
    }
    None
}

fn decode_http_chunked(body: &[u8]) -> Option<Vec<u8>> {
    // RFC 7230 chunked decoding, minimal: <hex>[;<ext>]\r\n<data>\r\n ... 0\r\n...
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    let mut guard = 0u32;

    while i < body.len() {
        guard += 1;
        if guard > 1_000_000 {
            return None;
        }

        // Read chunk-size line.
        let line_end = body[i..].windows(2).position(|w| w == b"\r\n")?;
        let line = &body[i..i + line_end];
        i += line_end + 2;

        // parse hex up to ';' or end
        let mut size: usize = 0;
        let mut any = false;
        for &b in line.iter() {
            if b == b';' {
                break;
            }
            let v = match b {
                b'0'..=b'9' => (b - b'0') as usize,
                b'a'..=b'f' => (b - b'a' + 10) as usize,
                b'A'..=b'F' => (b - b'A' + 10) as usize,
                b' ' | b'\t' => continue,
                _ => return None,
            };
            any = true;
            size = size.saturating_mul(16).saturating_add(v);
        }
        if !any {
            return None;
        }

        if size == 0 {
            // Ignore trailers; we're done.
            return Some(out);
        }

        if i + size > body.len() {
            return None;
        }
        out.extend_from_slice(&body[i..i + size]);
        i += size;

        // Expect CRLF after data.
        if i + 2 > body.len() || &body[i..i + 2] != b"\r\n" {
            return None;
        }
        i += 2;
    }

    Some(out)
}

static TLS_DEMO_JOB_SEQ: AtomicU32 = AtomicU32::new(1);
static TLS_DEMO_PREFERRED_DEV: AtomicU32 = AtomicU32::new(u32::MAX);

fn leak_str(s: alloc::string::String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

#[task]
pub async fn tls_demo_matrix_job(slot_id: u8, host_arg: HString<96>) {
    tls_demo_matrix_job_run(slot_id, host_arg).await;
}

/// Plain async runner for the TLS demo.
///
/// The `#[task]` wrapper exists for spawning via an Embassy `Spawner`, but tasks
/// return a `SpawnToken` and cannot be awaited directly.
pub async fn tls_demo_matrix_job_run(slot_id: u8, host_arg: HString<96>) {
    crate::matrix::push_line(slot_id, "https: rustls demo starting");

    // Permanent FSM gating: do not run until the network is actually usable.
    crate::v::readiness::wait_for(crate::v::readiness::NET_GATEWAY_REACHABLE).await;

    let dev_count = loop {
        let c = crate::net::device_count();
        if c > 0 {
            break c;
        }
        Timer::after(EmbassyDuration::from_millis(50)).await;
    };

    let initial_host: &'static str = if host_arg.is_empty() {
        DEMO_HOST
    } else {
        leak_str(host_arg.as_str().to_string())
    };

    crate::log!(
        "tls_demo: starting host={} port={}\n",
        initial_host,
        DEMO_PORT
    );

    let preferred = TLS_DEMO_PREFERRED_DEV.load(Ordering::Relaxed);
    let preferred =
        (preferred != u32::MAX && (preferred as usize) < dev_count).then_some(preferred as usize);

    if let Some(dev_idx) = preferred {
        if tls_demo_attempt_device(slot_id, initial_host, dev_idx).await {
            return;
        }
    }

    // Try each NIC index. This is important when multiple devices exist but only
    // one is wired to slirp/user-net in QEMU.
    for dev_idx in 0..dev_count {
        if preferred == Some(dev_idx) {
            continue;
        }
        if tls_demo_attempt_device(slot_id, initial_host, dev_idx).await {
            return;
        }
    }

    crate::matrix::push_line(slot_id, "https: timed out");
    crate::log!("tls_demo: timed out (all devices)\n");
    crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
}

async fn tls_demo_attempt_device(slot_id: u8, initial_host: &'static str, dev_idx: usize) -> bool {
    crate::log!("tls_demo: attempting device={}\n", dev_idx);

    let mut target = RedirectTarget {
        host: initial_host,
        port: DEMO_PORT,
        path: "/",
    };
    let mut redirects: usize = 0;

    'redirects: loop {
        crate::matrix::push_line(slot_id, "https: opening tls/tcp");

        let Ok(ip) = dns::resolve_ipv4_for_device(dev_idx, target.host, DnsConfig::default()).await
        else {
            crate::log!("tls_demo: dns failed (device={})\n", dev_idx);
            break 'redirects;
        };

        crate::log!(
            "tls_demo: resolved host={} ip={}.{}.{}.{} (device={})\n",
            target.host,
            ip[0],
            ip[1],
            ip[2],
            ip[3],
            dev_idx
        );

        let seq = TLS_DEMO_JOB_SEQ.fetch_add(1, Ordering::Relaxed);
        // Suffix with "@<idx>" so tls-socket can pin the underlying TCP socket to
        // the chosen NIC.
        let owner = leak_str(alloc::format!(
            "tlsdemo-{}-{}@{}",
            slot_id + 1,
            seq,
            dev_idx
        ));
        let cmds_name = leak_str(alloc::format!("{}-tls-cmd", owner));
        let evts_name = leak_str(alloc::format!("{}-tls-evt", owner));
        let cmds = Queue::new_leaked(cmds_name, 128);
        let events = Queue::new_leaked(evts_name, 128);
        register_tls_app_queues(owner, cmds, events);

        let mut tls_handle: Option<vnet::NetHandle> = None;
        let mut sent_connect = false;
        let mut http_sent = false;

        let mut plaintext: Vec<u8> = Vec::new();
        let mut truncated = false;

        let roots = TlsRoots::mozilla();
        let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);

        let deadline = Instant::now() + EmbassyDuration::from_secs(10);

        // Cap plaintext capture (headers + body).
        // Google pages can exceed 256 KiB even when uncompressed.
        const MAX_PLAINTEXT: usize = 1024 * 1024;

        loop {
            for ev in events.drain(32) {
                match ev {
                    TlsEvent::Opened { handle } => {
                        tls_handle = Some(handle);
                        crate::matrix::push_line(slot_id, "https: tcp opened");
                    }
                    TlsEvent::Connected { handle } => {
                        if tls_handle != Some(handle) {
                            continue;
                        }
                        crate::matrix::push_line(slot_id, "https: tls connected");
                        crate::log!("tls_demo: connected (device={})\n", dev_idx);

                        if !http_sent {
                            // Ask for gzip so we can decode it into readable HTML. (Some servers ignore
                            // `identity` and still send a compressed body.)
                            let req = alloc::format!(
                                "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS rustls demo\r\nAccept: text/html, */*;q=0.8\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n",
                                target.path,
                                target.host
                            );
                            let _ = cmds.push(TlsCommand::Send {
                                handle,
                                data: req.into_bytes(),
                            });
                            http_sent = true;
                            crate::matrix::push_line(slot_id, "https: sent https request");
                            crate::log!("tls_demo: sent request\n");
                        }
                    }
                    TlsEvent::Data { handle, data } => {
                        if tls_handle != Some(handle) {
                            continue;
                        }

                        if !data.is_empty() {
                            if plaintext.len() < MAX_PLAINTEXT {
                                let room = MAX_PLAINTEXT - plaintext.len();
                                let take = data.len().min(room);
                                plaintext.extend_from_slice(&data[..take]);
                                if take < data.len() {
                                    truncated = true;
                                }
                            } else {
                                truncated = true;
                            }
                        }
                    }
                    TlsEvent::Closed { handle } => {
                        if tls_handle == Some(handle) {
                            // Parse/clean the HTTP response so chunked transfer encoding
                            // doesn't show up as chunk-size lines in the output.
                            let final_blob = if let Some(hdr_end) = find_http_header_end(&plaintext)
                            {
                                let headers = &plaintext[..hdr_end];
                                let body = &plaintext[hdr_end..];

                                let status_code = parse_http_status(&plaintext);
                                if let Some(code) = status_code {
                                    let line = alloc::format!("https: http status={}", code);
                                    crate::matrix::push_line(slot_id, line.as_str());
                                }

                                let is_chunked = header_contains_token(
                                    headers,
                                    b"transfer-encoding",
                                    b"chunked",
                                );

                                if let Some(ct) = header_get_value(headers, b"content-type") {
                                    let ct = core::str::from_utf8(ct).unwrap_or("<non-utf8>");
                                    let line = alloc::format!("https: content-type={}", ct);
                                    crate::matrix::push_line(slot_id, line.as_str());
                                }

                                let content_encoding =
                                    header_get_value(headers, b"content-encoding");
                                if let Some(ce) = content_encoding {
                                    let ce = core::str::from_utf8(ce).unwrap_or("<non-utf8>");
                                    let line = alloc::format!("https: content-encoding={}", ce);
                                    crate::matrix::push_line(slot_id, line.as_str());
                                }

                                let mut decoded_body = if is_chunked {
                                    decode_http_chunked(body).unwrap_or_else(|| body.to_vec())
                                } else if let Some(len) = header_parse_content_length(headers) {
                                    body.get(..len).unwrap_or(body).to_vec()
                                } else {
                                    body.to_vec()
                                };

                                // Decompress common encodings so the stored blob is human-readable.
                                // (Google frequently uses gzip.)
                                if let Some(ce) = content_encoding {
                                    if header_value_contains_token(ce, b"gzip") {
                                        if let Some(out) = decode_gzip(&decoded_body, MAX_PLAINTEXT)
                                        {
                                            let msg = alloc::format!(
                                                "https: gunzip {} -> {} bytes",
                                                decoded_body.len(),
                                                out.len()
                                            );
                                            crate::matrix::push_line(slot_id, msg.as_str());
                                            decoded_body = out;
                                        } else {
                                            crate::matrix::push_line(
                                                slot_id,
                                                "https: gunzip failed (maybe truncated)",
                                            );
                                        }
                                    } else if header_value_contains_token(ce, b"br") {
                                        crate::matrix::push_line(
                                            slot_id,
                                            "https: brotli body (not decoded)",
                                        );
                                    }
                                }

                                // Store only the decoded body in the blob so `§N` prints the actual page.
                                let merged = decoded_body;

                                // Redirect handling: follow up to 3 redirects.
                                if redirects < 3 {
                                    if let Some(code) = status_code {
                                        let is_redirect =
                                            matches!(code, 301 | 302 | 303 | 307 | 308);
                                        if is_redirect {
                                            if let Some(loc) =
                                                header_get_value(headers, b"location")
                                            {
                                                if let Some(next) =
                                                    parse_redirect_target(&target, loc)
                                                {
                                                    redirects += 1;
                                                    let msg = alloc::format!(
                                                        "https: redirect {}/3 -> {}{}",
                                                        redirects,
                                                        next.host,
                                                        next.path
                                                    );
                                                    crate::matrix::push_line(slot_id, msg.as_str());
                                                    crate::log!(
                                                        "tls_demo: redirect {}/3 -> host={} port={} path={}\n",
                                                        redirects,
                                                        next.host,
                                                        next.port,
                                                        next.path
                                                    );
                                                    target = next;
                                                    // Start the next request on the same device.
                                                    continue 'redirects;
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    crate::matrix::push_line(
                                        slot_id,
                                        "https: redirect limit reached (3)",
                                    );
                                }

                                merged
                            } else {
                                plaintext
                            };

                            let line = alloc::format!(
                                "https: plaintext bytes={}{}",
                                final_blob.len(),
                                if truncated { " (truncated)" } else { "" }
                            );
                            crate::matrix::push_line(slot_id, line.as_str());
                            crate::log!(
                                "tls_demo: closed device={} plaintext_bytes={}{}\n",
                                dev_idx,
                                final_blob.len(),
                                if truncated { " (truncated)" } else { "" }
                            );

                            let _ = crate::matrix::set_blob_owned_with_preview(slot_id, final_blob);
                            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);
                            TLS_DEMO_PREFERRED_DEV.store(dev_idx as u32, Ordering::Relaxed);
                            return true;
                        }
                    }
                    TlsEvent::Error { msg } => {
                        crate::log!("tls_demo: net error (device={}): {}\n", dev_idx, msg);
                    }
                    TlsEvent::TlsError { err } => {
                        crate::log!("tls_demo: tls error (device={}): {:?}\n", dev_idx, err);
                        if let Some(h) = tls_handle {
                            let _ = cmds.push(TlsCommand::Close { handle: h });
                        }
                        break 'redirects;
                    }
                }
            }

            if !sent_connect {
                let _ = cmds.push(TlsCommand::OpenTcpConnect {
                    remote: vnet::EndpointV4 {
                        addr: ip,
                        port: target.port,
                    },
                    server_name: target.host,
                    cfg: cfg.clone(),
                    roots: roots.clone(),
                    timeouts: crate::net::tls_socket::TlsTimeouts {
                        connect_ms: 20_000,
                        tls_ms: 30_000,
                        idle_ms: 120_000,
                    },
                });
                sent_connect = true;
            }

            if Instant::now() >= deadline {
                crate::log!("tls_demo: timed out (device={})\n", dev_idx);
                if let Some(h) = tls_handle {
                    let _ = cmds.push(TlsCommand::Close { handle: h });
                }
                break;
            }

            Timer::after(EmbassyDuration::from_millis(50)).await;
        }
    }

    false
}
