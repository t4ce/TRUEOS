extern crate alloc;

use alloc::{format, string::String, string::ToString, vec::Vec};
use alloc::vec;

use embassy_time::{Duration as EmbassyDuration, Timer};

use trueos_v::vnet as api;

use crate::v::net::VNet;
use crate::disc::block::DeviceHandle;

#[inline]
fn tsc_now() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        // Best-effort cycle counter for perf measurements.
        unsafe { core::arch::x86_64::_rdtsc() as u64 }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

#[derive(Default)]
struct HttpPerf {
    bytes: u64,
    tsc_read: u64,
    tsc_submit: u64,
}

impl HttpPerf {
    fn record_read(&mut self, delta_tsc: u64) {
        self.tsc_read = self.tsc_read.saturating_add(delta_tsc);
    }

    fn record_submit(&mut self, delta_tsc: u64, bytes: usize) {
        self.tsc_submit = self.tsc_submit.saturating_add(delta_tsc);
        self.bytes = self.bytes.saturating_add(bytes as u64);
    }

    fn log(&self) {
        if self.bytes == 0 {
            return;
        }
        let total = self.tsc_read.saturating_add(self.tsc_submit);
        let cycles_per_byte = total / self.bytes;
        crate::log!(
            "http-trueosfs: perf bytes={} tsc_read={} tsc_submit={} tsc_total={} cycles_per_byte={}\n",
            self.bytes,
            self.tsc_read,
            self.tsc_submit,
            total,
            cycles_per_byte
        );
    }
}

const HTTP_TRUEOSFS_TCP_PORT: u16 = 80;
const HTTP_TRUEOSFS_MAX_ENTRIES: usize = 256;
const HTTP_OCTET_STREAM: &str = "application/octet-stream";
const HTTP_MULTIPART_BOUNDARY: &str = "trueosfs-boundary";
const HTTP_MULTIPART_CONTENT_TYPE: &str =
    "multipart/byteranges; boundary=trueosfs-boundary";

fn http_stream_chunk_bytes(disk: DeviceHandle) -> usize {
    let mut base = disk.max_transfer_bytes() as usize;
    if base == 0 {
        base = 256 * 1024;
    }
    let mut safe = base.saturating_mul(9) / 10;
    const MIN: usize = 16 * 1024;
    const MAX: usize = 512 * 1024;
    if safe < MIN {
        safe = MIN;
    }
    if safe > MAX {
        safe = MAX;
    }
    safe
}

fn http_parse_target(req: &[u8]) -> Option<&str> {
    let s = core::str::from_utf8(req).ok()?;
    let line_end = s.find("\r\n").or_else(|| s.find('\n')).unwrap_or(s.len());
    let line = s.get(..line_end)?;
    let mut it = line.split_whitespace();
    let method = it.next()?;
    let target = it.next()?;
    if method != "GET" {
        return None;
    }
    Some(target)
}

fn http_query_param<'a>(target: &'a str, key: &str) -> Option<&'a str> {
    let (_, q) = target.split_once('?')?;
    for part in q.split('&') {
        if let Some((k, v)) = part.split_once('=') {
            if k == key {
                return Some(v);
            }
        }
    }
    None
}

fn http_path_only(target: &str) -> &str {
    target
        .split_once('?')
        .map(|(p, _)| p)
        .unwrap_or(target)
}

fn http_url_decode(s: &str, max_len: usize) -> Option<String> {
    if s.len() > max_len.saturating_mul(3) {
        return None;
    }

    let mut out = String::new();
    let mut i = 0usize;
    let b = s.as_bytes();
    while i < b.len() {
        if out.len() >= max_len {
            return None;
        }
        match b[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' => {
                if i + 2 >= b.len() {
                    return None;
                }
                let hi = b[i + 1];
                let lo = b[i + 2];
                let hex = |c: u8| -> Option<u8> {
                    match c {
                        b'0'..=b'9' => Some(c - b'0'),
                        b'a'..=b'f' => Some(c - b'a' + 10),
                        b'A'..=b'F' => Some(c - b'A' + 10),
                        _ => None,
                    }
                };
                let v = (hex(hi)? << 4) | hex(lo)?;
                out.push(char::from(v));
                i += 3;
            }
            other => {
                out.push(char::from(other));
                i += 1;
            }
        }
    }
    Some(out)
}

#[derive(Clone, Debug)]
struct MultipartPart {
    start: u64,
    end: u64,
    header: String,
}

enum HttpBodyPlan {
    Bytes(Vec<u8>),
    File {
        disk: DeviceHandle,
        path: String,
        offset: u64,
        len: u64,
    },
    Multipart {
        disk: DeviceHandle,
        path: String,
        parts: Vec<MultipartPart>,
        boundary: String,
    },
    None,
}

struct HttpResponsePlan {
    status: &'static str,
    content_type: &'static str,
    extra_headers: String,
    body_len: u64,
    body: HttpBodyPlan,
}

fn http_plain_response(status: &'static str, msg: &'static str) -> HttpResponsePlan {
    let body = msg.as_bytes().to_vec();
    HttpResponsePlan {
        status,
        content_type: "text/plain; charset=utf-8",
        extra_headers: String::new(),
        body_len: body.len() as u64,
        body: HttpBodyPlan::Bytes(body),
    }
}

fn http_find_header<'a>(req: &'a [u8], key: &str) -> Option<&'a str> {
    let s = core::str::from_utf8(req).ok()?;
    let mut lines = s.split('\n');
    let _ = lines.next()?;
    for line in lines {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(key) {
                return Some(v.trim());
            }
        }
    }
    None
}

fn http_etag_from_sha(sha: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(sha.len() * 2 + 2);
    out.push('"');
    for b in sha.iter().copied() {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out.push('"');
    out
}

fn http_etag_matches(value: &str, etag: &str) -> bool {
    let etag_trim = etag.trim();
    let etag_unquoted = etag_trim.trim_matches('"');
    for raw in value.split(',') {
        let mut tag = raw.trim();
        if tag == "*" {
            return true;
        }
        if let Some(stripped) = tag.strip_prefix("W/") {
            tag = stripped.trim();
        }
        if tag == etag_trim {
            return true;
        }
        let tag_unquoted = tag.trim_matches('"');
        if tag_unquoted == etag_unquoted {
            return true;
        }
    }
    false
}

fn http_parse_range_header(value: &str, total_len: u64) -> Option<Vec<(u64, u64)>> {
    if total_len == 0 {
        return None;
    }
    let value = value.trim();
    let value = value.strip_prefix("bytes=")?;
    let mut out: Vec<(u64, u64)> = Vec::new();
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (start_s, end_s) = part.split_once('-')?;
        if start_s.is_empty() {
            let suffix = end_s.parse::<u64>().ok()?;
            if suffix == 0 {
                return None;
            }
            let start = if suffix >= total_len {
                0
            } else {
                total_len.saturating_sub(suffix)
            };
            let end = total_len.saturating_sub(1);
            out.push((start, end));
        } else {
            let start = start_s.parse::<u64>().ok()?;
            let end = if end_s.is_empty() {
                total_len.saturating_sub(1)
            } else {
                end_s.parse::<u64>().ok()?
            };
            if start > end {
                return None;
            }
            if start >= total_len {
                return None;
            }
            let end = end.min(total_len.saturating_sub(1));
            out.push((start, end));
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

async fn http_prepare_file_response(
    disk: DeviceHandle,
    path: String,
    req: &[u8],
) -> HttpResponsePlan {
    let info = match crate::v::fs::trueosfs::file_info_async(disk, path.as_str()).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            return http_plain_response("HTTP/1.1 404 Not Found\r\n", "not found\n")
        }
        Err(_) => {
            return http_plain_response(
                "HTTP/1.1 500 Internal Server Error\r\n",
                "read error\n",
            )
        }
    };

    let total_len = info.data_len;
    let etag = http_etag_from_sha(&info.sha256);
    let mut extra_headers = String::new();
    extra_headers.push_str("Accept-Ranges: bytes\r\n");
    extra_headers.push_str(format!("ETag: {}\r\n", etag).as_str());

    if let Some(value) = http_find_header(req, "If-None-Match") {
        if http_etag_matches(value, etag.as_str()) {
            return HttpResponsePlan {
                status: "HTTP/1.1 304 Not Modified\r\n",
                content_type: "text/plain; charset=utf-8",
                extra_headers,
                body_len: 0,
                body: HttpBodyPlan::None,
            };
        }
    }

    let mut allow_ranges = true;
    if let Some(value) = http_find_header(req, "If-Range") {
        if !http_etag_matches(value, etag.as_str()) {
            allow_ranges = false;
        }
    }

    let mut ranges: Option<Vec<(u64, u64)>> = None;
    if allow_ranges {
        if let Some(value) = http_find_header(req, "Range") {
            ranges = http_parse_range_header(value, total_len);
            if ranges.is_none() {
                let mut headers = extra_headers.clone();
                headers.push_str(format!("Content-Range: bytes */{}\r\n", total_len).as_str());
                return HttpResponsePlan {
                    status: "HTTP/1.1 416 Range Not Satisfiable\r\n",
                    content_type: "text/plain; charset=utf-8",
                    extra_headers: headers,
                    body_len: 0,
                    body: HttpBodyPlan::None,
                };
            }
        }
    }

    if let Some(ranges) = ranges {
        if ranges.len() == 1 {
            let (start, end) = ranges[0];
            let len = end.saturating_sub(start).saturating_add(1);
            let mut headers = extra_headers;
            headers.push_str(
                format!("Content-Range: bytes {}-{}/{}\r\n", start, end, total_len).as_str(),
            );
            return HttpResponsePlan {
                status: "HTTP/1.1 206 Partial Content\r\n",
                content_type: HTTP_OCTET_STREAM,
                extra_headers: headers,
                body_len: len,
                body: HttpBodyPlan::File {
                    disk,
                    path,
                    offset: start,
                    len,
                },
            };
        }

        let mut parts: Vec<MultipartPart> = Vec::new();
        let mut total_len_out: u64 = 0;
        for (start, end) in ranges.into_iter() {
            let header = format!(
                "--{}\r\nContent-Type: {}\r\nContent-Range: bytes {}-{}/{}\r\n\r\n",
                HTTP_MULTIPART_BOUNDARY,
                HTTP_OCTET_STREAM,
                start,
                end,
                total_len
            );
            let len = end.saturating_sub(start).saturating_add(1);
            total_len_out = total_len_out.saturating_add(header.as_bytes().len() as u64);
            total_len_out = total_len_out.saturating_add(len);
            total_len_out = total_len_out.saturating_add(2); // trailing CRLF
            parts.push(MultipartPart { start, end, header });
        }
        let closing = format!("--{}--\r\n", HTTP_MULTIPART_BOUNDARY);
        total_len_out = total_len_out.saturating_add(closing.as_bytes().len() as u64);

        return HttpResponsePlan {
            status: "HTTP/1.1 206 Partial Content\r\n",
            content_type: HTTP_MULTIPART_CONTENT_TYPE,
            extra_headers,
            body_len: total_len_out,
            body: HttpBodyPlan::Multipart {
                disk,
                path,
                parts,
                boundary: HTTP_MULTIPART_BOUNDARY.to_string(),
            },
        };
    }

    HttpResponsePlan {
        status: "HTTP/1.1 200 OK\r\n",
        content_type: HTTP_OCTET_STREAM,
        extra_headers,
        body_len: total_len,
        body: HttpBodyPlan::File {
            disk,
            path,
            offset: 0,
            len: total_len,
        },
    }
}

#[embassy_executor::task]
pub async fn http_trueosfs_task() {
    async move {
        // Once the network is reachable, `open_primary()` should succeed; keep it strict.
        let vnet = loop {
            if let Some(v) = VNet::open_primary() {
                break v;
            }
            Timer::after(EmbassyDuration::from_millis(50)).await;
        };

        if vnet
            .submit(api::Command::OpenTcpListen {
                port: HTTP_TRUEOSFS_TCP_PORT,
            })
            .is_err()
        {
            crate::log!("http-trueosfs: listen submit failed\n");
            return;
        }

        crate::log!(
            "http-trueosfs: listening on tcp {} (hostfwd localhost:8080 -> guest:80)\n",
            HTTP_TRUEOSFS_TCP_PORT
        );

        let mut listener_handle: Option<api::NetHandle> = None;
        let mut active_handle: Option<api::NetHandle> = None;
        let mut sent_for_active: bool = false;
        let mut active_pending: usize = 0;
        let mut active_sent: usize = 0;
        let mut active_close: bool = false;

        loop {
            while let Some(ev) = vnet.pop_event() {
                match ev {
                    api::Event::Opened { handle, kind } => {
                        if kind == api::SocketKind::Tcp {
                            listener_handle = Some(handle);
                        }
                    }
                    api::Event::TcpEstablished { handle } => {
                        active_handle = Some(handle);
                        sent_for_active = false;
                        active_pending = 0;
                        active_sent = 0;
                        active_close = false;
                    }
                    api::Event::TcpData { handle, data } => {
                        if active_handle.is_none() {
                            active_handle = Some(handle);
                            sent_for_active = false;
                        }
                        if active_handle != Some(handle) {
                            continue;
                        }
                        if sent_for_active {
                            continue;
                        }
                        sent_for_active = true;

                        let target = http_parse_target(data.as_slice()).unwrap_or("/");

                        let roots = crate::v::fs::trueosfs::list_roots();

                        let response: HttpResponsePlan = if http_path_only(target).starts_with("/dl/") {
                            // Download endpoint: /dl/<root_raw>/<path>
                            // (where <root_raw> is the DiscId raw value as decimal)
                            'resp: {
                                let path_only = http_path_only(target);
                                let rest = path_only.strip_prefix("/dl/").unwrap_or("");

                                let (root_raw_s, enc_path) = match rest.split_once('/') {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "missing root/path\n",
                                        )
                                    }
                                };

                                let root_raw = match root_raw_s.parse::<u32>() {
                                    Ok(v) => v,
                                    Err(_) => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "bad root\n",
                                        )
                                    }
                                };

                                let root = match roots.iter().find(|r| r.disk_id.raw() == root_raw) {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 404 Not Found\r\n",
                                            "unknown root\n",
                                        )
                                    }
                                };

                                let disk = match crate::disc::block::device_handle(root.disk_id) {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 503 Service Unavailable\r\n",
                                            "root unavailable\n",
                                        )
                                    }
                                };

                                let mut path = match http_url_decode(enc_path, 240) {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "bad path\n",
                                        )
                                    }
                                };

                                while path.starts_with('/') {
                                    path.remove(0);
                                }
                                if path.is_empty() || path.contains("..") {
                                    break 'resp http_plain_response(
                                        "HTTP/1.1 400 Bad Request\r\n",
                                        "bad path\n",
                                    );
                                }

                                http_prepare_file_response(disk, path, data.as_slice()).await
                            }
                        } else if target.starts_with("/dl") {
                            // Back-compat download endpoint: /dl?path=<urlencoded path> (uses primary root)
                            let disk = crate::v::fs::trueosfs::primary_root_handle();
                            match disk {
                                None => http_plain_response(
                                    "HTTP/1.1 503 Service Unavailable\r\n",
                                    "no TRUEOSFS mounted\n",
                                ),
                                Some(disk) => match http_query_param(target, "path") {
                                    None => http_plain_response(
                                        "HTTP/1.1 400 Bad Request\r\n",
                                        "missing path\n",
                                    ),
                                    Some(enc_path) => match http_url_decode(enc_path, 240) {
                                        None => http_plain_response(
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "bad path\n",
                                        ),
                                        Some(mut path) => {
                                            while path.starts_with('/') {
                                                path.remove(0);
                                            }
                                            if path.is_empty() || path.contains("..") {
                                                http_plain_response(
                                                    "HTTP/1.1 400 Bad Request\r\n",
                                                    "bad path\n",
                                                )
                                            } else {
                                                http_prepare_file_response(disk, path, data.as_slice()).await
                                            }
                                        }
                                    },
                                },
                            }
                        } else {
                            // Build HTML trees (best-effort), one per mounted TRUEOSFS root.
                            let js =    r#"<style>li{cursor:pointer}</style><script>(function(){function labelOf(li){var n=li.firstChild;return n&&n.nodeType===3?n.textContent:"";}function pathFor(li){var parts=[];var cur=li;while(cur){var t=labelOf(cur).trim();if(t==="/"){t="";}if(t.endsWith("/")){t=t.slice(0,-1);}if(t){parts.push(t);}var p=cur.parentElement;cur=p&&p.closest("li");}parts.reverse();return parts.join("/");}document.addEventListener("click",function(e){var li=e.target.closest("li");if(!li){return;}var tree=li.closest(".tree");if(!tree){return;}var root=tree.getAttribute("data-root")||"";var path=pathFor(li);if(!root||!path){return;}var segs=path.split("/").map(function(s){return encodeURIComponent(s);}).join("/");window.location.href="/dl/"+root+"/"+segs;});})();</script>"#;

                            let mut trees_html = String::new();
                            for r in roots.iter() {
                                let raw = r.disk_id.raw();

                                let tree = match crate::disc::block::device_handle(r.disk_id) {
                                    None => None,
                                    Some(disk) => match crate::v::fs::trueosfs::html_tree_async(
                                        disk,
                                        HTTP_TRUEOSFS_MAX_ENTRIES,
                                    )
                                    .await
                                    {
                                        Ok(v) => v,
                                        Err(_) => None,
                                    },
                                };

                                trees_html.push_str(
                                    format!(
                                        "<section><h2>Root {} (raw={} seq={})</h2><div class=\"tree\" data-root=\"{}\">",
                                        r.disk_id, raw, r.seq, raw
                                    )
                                        .as_str(),
                                );
                                if let Some(t) = tree {
                                    trees_html.push_str(t.as_str());
                                } else {
                                    trees_html.push_str("<p class=\"muted\">(tree unavailable)</p>");
                                }
                                trees_html.push_str("</div></section>");
                            }

                            let body = if roots.is_empty() {
                                format!(
                                    "<!doctype html><html><head><meta charset=\"utf-8\"><title>TRUEOSFS</title>{}</head><body><h1>TRUEOSFS</h1><p class=\"muted\">(no TRUEOSFS mounted)</p></body></html>",
                                    js
                                )
                            } else if roots.len() == 1 {
                                format!(
                                    "<!doctype html><html><head><meta charset=\"utf-8\"><title>TRUEOSFS</title>{}</head><body><h1>TRUEOSFS</h1><p>Click a file to download.</p>{}</body></html>",
                                    js, trees_html
                                )
                            } else {
                                format!(
                                    "<!doctype html><html><head><meta charset=\"utf-8\"><title>TRUEOSFS</title>{}</head><body><h1>TRUEOSFS</h1><p>Click a file to download.</p>{}</body></html>",
                                    js,
                                    trees_html
                                )
                            };

                            let body_bytes = body.into_bytes();
                            HttpResponsePlan {
                                status: "HTTP/1.1 200 OK\r\n",
                                content_type: "text/html; charset=utf-8",
                                extra_headers: String::new(),
                                body_len: body_bytes.len() as u64,
                                body: HttpBodyPlan::Bytes(body_bytes),
                            }
                        };

                    let HttpResponsePlan {
                        status,
                        content_type,
                        extra_headers,
                        body_len,
                        body,
                    } = response;

                    let mut header = String::new();
                    header.push_str(status);
                    header.push_str("Content-Type: ");
                    header.push_str(content_type);
                    header.push_str("\r\n");
                    if !extra_headers.is_empty() {
                        header.push_str(extra_headers.as_str());
                    }
                    header.push_str("Content-Length: ");
                    header.push_str(format!("{}", body_len).as_str());
                    header.push_str("\r\nConnection: close\r\n\r\n");

                    let body_len_usize = body_len.min(usize::MAX as u64) as usize;
                    active_pending = header.as_bytes().len().saturating_add(body_len_usize);
                    active_sent = 0;
                    active_close = true;

                    // Send headers + body in MAX_MSG chunks.
                    for chunk in header.as_bytes().chunks(api::MAX_MSG) {
                        let _ = vnet.submit(api::Command::SendTcp {
                            handle,
                            data: api::ByteBuf::from_slice_trunc(chunk),
                        });
                    }

                    let mut perf = HttpPerf::default();

                    match body {
                        HttpBodyPlan::Bytes(bytes) => {
                            for chunk in bytes.as_slice().chunks(api::MAX_MSG) {
                                let _ = vnet.submit(api::Command::SendTcp {
                                    handle,
                                    data: api::ByteBuf::from_slice_trunc(chunk),
                                });
                            }
                        }
                        HttpBodyPlan::File {
                            disk,
                            path,
                            offset,
                            len,
                        } => {
                            let mut buf = vec![0u8; http_stream_chunk_bytes(disk)];
                            let mut remaining = len;
                            let mut off = offset;
                            while remaining > 0 {
                                let want = core::cmp::min(remaining, buf.len() as u64) as usize;
                                let t0 = tsc_now();
                                let read = match crate::v::fs::trueosfs::file_read_range_async(
                                    disk,
                                    path.as_str(),
                                    off,
                                    &mut buf[..want],
                                )
                                .await
                                {
                                    Ok(Some(n)) => n,
                                    _ => 0,
                                };
                                let t1 = tsc_now();
                                perf.record_read(t1.wrapping_sub(t0));
                                if read == 0 {
                                    break;
                                }
                                let t2 = tsc_now();
                                for chunk in buf[..read].chunks(api::MAX_MSG) {
                                    let _ = vnet.submit(api::Command::SendTcp {
                                        handle,
                                        data: api::ByteBuf::from_slice_trunc(chunk),
                                    });
                                }
                                let t3 = tsc_now();
                                perf.record_submit(t3.wrapping_sub(t2), read);
                                off = off.saturating_add(read as u64);
                                remaining = remaining.saturating_sub(read as u64);
                            }
                        }
                        HttpBodyPlan::Multipart {
                            disk,
                            path,
                            parts,
                            boundary,
                        } => {
                            let mut buf = vec![0u8; http_stream_chunk_bytes(disk)];
                            for part in parts {
                                for chunk in part.header.as_bytes().chunks(api::MAX_MSG) {
                                    let _ = vnet.submit(api::Command::SendTcp {
                                        handle,
                                        data: api::ByteBuf::from_slice_trunc(chunk),
                                    });
                                }
                                let mut remaining = part.end.saturating_sub(part.start).saturating_add(1);
                                let mut off = part.start;
                                while remaining > 0 {
                                    let want = core::cmp::min(remaining, buf.len() as u64) as usize;
                                    let t0 = tsc_now();
                                    let read = match crate::v::fs::trueosfs::file_read_range_async(
                                        disk,
                                        path.as_str(),
                                        off,
                                        &mut buf[..want],
                                    )
                                    .await
                                    {
                                        Ok(Some(n)) => n,
                                        _ => 0,
                                    };
                                    let t1 = tsc_now();
                                    perf.record_read(t1.wrapping_sub(t0));
                                    if read == 0 {
                                        break;
                                    }
                                    let t2 = tsc_now();
                                    for chunk in buf[..read].chunks(api::MAX_MSG) {
                                        let _ = vnet.submit(api::Command::SendTcp {
                                            handle,
                                            data: api::ByteBuf::from_slice_trunc(chunk),
                                        });
                                    }
                                    let t3 = tsc_now();
                                    perf.record_submit(t3.wrapping_sub(t2), read);
                                    off = off.saturating_add(read as u64);
                                    remaining = remaining.saturating_sub(read as u64);
                                }
                                for chunk in b"\r\n".chunks(api::MAX_MSG) {
                                    let _ = vnet.submit(api::Command::SendTcp {
                                        handle,
                                        data: api::ByteBuf::from_slice_trunc(chunk),
                                    });
                                }
                            }
                            let closing = format!("--{}--\r\n", boundary);
                            for chunk in closing.as_bytes().chunks(api::MAX_MSG) {
                                let _ = vnet.submit(api::Command::SendTcp {
                                    handle,
                                    data: api::ByteBuf::from_slice_trunc(chunk),
                                });
                            }
                        }
                        HttpBodyPlan::None => {}
                    }

                    perf.log();

                }
                api::Event::Closed { handle } => {
                    if active_handle == Some(handle) {
                        active_handle = None;
                        sent_for_active = false;
                        active_pending = 0;
                        active_sent = 0;
                        active_close = false;
                    }

                    // If the listener handle closes (or smoltcp collapses listen/conn handles), relisten.
                    if listener_handle == Some(handle) {
                        listener_handle = None;
                        let _ = vnet.submit(api::Command::OpenTcpListen {
                            port: HTTP_TRUEOSFS_TCP_PORT,
                        });
                    }
                }
                api::Event::Error { msg } => {
                    if msg != "bad handle" {
                        crate::log!("http-trueosfs: error {}\n", msg);
                    }
                }
                api::Event::TcpSent { handle, len } => {
                    if active_close && active_handle == Some(handle) && active_pending != 0 {
                        active_sent = active_sent.saturating_add(len as usize);
                        if active_sent >= active_pending {
                            active_close = false;
                            active_pending = 0;
                            active_sent = 0;
                            let _ = vnet.submit(api::Command::Close { handle });
                        }
                    }
                }
                api::Event::UdpPacket { .. } => {},
                api::Event::IcmpReply { .. } => {},
            }
        }

        Timer::after(EmbassyDuration::from_millis(10)).await;
    }
    }.await;
}
