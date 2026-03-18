extern crate alloc;

use alloc::vec;
use alloc::{format, string::String, string::ToString, vec::Vec};

use embassy_time::{Duration as EmbassyDuration, Timer};

use trueos_v::vnet as api;

use crate::disc::block::DeviceHandle;
use crate::v::net::VNet;

#[inline]
fn tsc_now() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        // Best-effort cycle counter for perf measurements.
        unsafe { core::arch::x86_64::_rdtsc() }
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
const HTTP_TRUEOSFS_MAX_REQUEST_BYTES: usize = 1024 * 1024;
const HTTP_OCTET_STREAM: &str = "application/octet-stream";
const HTTP_MULTIPART_BOUNDARY: &str = "trueosfs-boundary";
const HTTP_MULTIPART_CONTENT_TYPE: &str = "multipart/byteranges; boundary=trueosfs-boundary";

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

struct HttpRequestLine<'a> {
    method: &'a str,
    target: &'a str,
}

fn http_parse_request_line(req: &[u8]) -> Option<HttpRequestLine<'_>> {
    let s = core::str::from_utf8(req).ok()?;
    let line_end = s.find("\r\n").or_else(|| s.find('\n')).unwrap_or(s.len());
    let line = s.get(..line_end)?;
    let mut it = line.split_whitespace();
    let method = it.next()?;
    let target = it.next()?;
    Some(HttpRequestLine { method, target })
}

fn http_query_param<'a>(target: &'a str, key: &str) -> Option<&'a str> {
    let (_, q) = target.split_once('?')?;
    for part in q.split('&') {
        if let Some((k, v)) = part.split_once('=')
            && k == key
        {
            return Some(v);
        }
    }
    None
}

fn http_path_only(target: &str) -> &str {
    target.split_once('?').map(|(p, _)| p).unwrap_or(target)
}

fn http_header_end(req: &[u8]) -> Option<usize> {
    let mut i = 0usize;
    while i + 3 < req.len() {
        if &req[i..i + 4] == b"\r\n\r\n" {
            return Some(i + 4);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j + 1 < req.len() {
        if &req[j..j + 2] == b"\n\n" {
            return Some(j + 2);
        }
        j += 1;
    }
    None
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
        if let Some((k, v)) = line.split_once(':')
            && k.trim().eq_ignore_ascii_case(key)
        {
            return Some(v.trim());
        }
    }
    None
}

fn http_content_length(req: &[u8]) -> Option<usize> {
    let value = http_find_header(req, "Content-Length")?;
    value.parse::<usize>().ok()
}

fn http_normalize_rel_path(raw: &str, max_len: usize) -> Option<String> {
    let mut out = String::new();
    for seg in raw.split('/') {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        if seg == "." || seg == ".." {
            return None;
        }
        if seg.contains('\\') {
            return None;
        }
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(seg);
        if out.len() > max_len {
            return None;
        }
    }
    Some(out)
}

fn http_normalize_rel_path_decoded(raw: &str, max_len: usize) -> Option<String> {
    let decoded = http_url_decode(raw, max_len)?;
    http_normalize_rel_path(decoded.as_str(), max_len)
}

fn http_normalize_name(raw: &str, max_len: usize) -> Option<String> {
    let decoded = http_url_decode(raw, max_len)?;
    if decoded.is_empty() || decoded.contains('/') || decoded.contains('\\') {
        return None;
    }
    if decoded == "." || decoded == ".." {
        return None;
    }
    Some(decoded)
}

fn http_join_rel_path(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", dir, name)
    }
}

fn http_parse_root_and_dir(target: &str, prefix: &str) -> Option<(u32, String)> {
    let rest = http_path_only(target).strip_prefix(prefix)?;
    if rest.is_empty() {
        return None;
    }
    let (root_raw_s, enc_dir) = match rest.split_once('/') {
        Some((root, dir)) => (root, dir),
        None => (rest, ""),
    };
    let root_raw = root_raw_s.parse::<u32>().ok()?;
    let dir = http_normalize_rel_path_decoded(enc_dir, 240)?;
    Some((root_raw, dir))
}

fn http_etag_from_len(len: u64) -> String {
    // Weak ETag for baseline mode: stable per file length.
    format!("W/\"len-{}\"", len)
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
    if out.is_empty() { None } else { Some(out) }
}

async fn http_prepare_file_response(
    disk: DeviceHandle,
    path: String,
    req: &[u8],
) -> HttpResponsePlan {
    let info = match crate::v::fs::trueosfs::file_info_async(disk, path.as_str()).await {
        Ok(Some(v)) => v,
        Ok(None) => return http_plain_response("HTTP/1.1 404 Not Found\r\n", "not found\n"),
        Err(_) => {
            return http_plain_response("HTTP/1.1 500 Internal Server Error\r\n", "read error\n");
        }
    };

    let total_len = info.data_len;
    let etag = http_etag_from_len(info.data_len);
    let mut extra_headers = String::new();
    extra_headers.push_str("Accept-Ranges: bytes\r\n");
    extra_headers.push_str(format!("ETag: {}\r\n", etag).as_str());

    if let Some(value) = http_find_header(req, "If-None-Match")
        && http_etag_matches(value, etag.as_str())
    {
        return HttpResponsePlan {
            status: "HTTP/1.1 304 Not Modified\r\n",
            content_type: "text/plain; charset=utf-8",
            extra_headers,
            body_len: 0,
            body: HttpBodyPlan::None,
        };
    }

    let mut allow_ranges = true;
    if let Some(value) = http_find_header(req, "If-Range")
        && !http_etag_matches(value, etag.as_str())
    {
        allow_ranges = false;
    }

    let mut ranges: Option<Vec<(u64, u64)>> = None;
    if allow_ranges && let Some(value) = http_find_header(req, "Range") {
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
                HTTP_MULTIPART_BOUNDARY, HTTP_OCTET_STREAM, start, end, total_len
            );
            let len = end.saturating_sub(start).saturating_add(1);
            total_len_out = total_len_out.saturating_add(header.len() as u64);
            total_len_out = total_len_out.saturating_add(len);
            total_len_out = total_len_out.saturating_add(2); // trailing CRLF
            parts.push(MultipartPart { start, end, header });
        }
        let closing = format!("--{}--\r\n", HTTP_MULTIPART_BOUNDARY);
        total_len_out = total_len_out.saturating_add(closing.len() as u64);

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

fn http_root_tree_script() -> &'static str {
    r##"<script>(function(){
function firstTextNode(el){for(var i=0;i<el.childNodes.length;i++){var n=el.childNodes[i];if(n.nodeType===3){return n;}}return null;}
function rawLabel(li){var n=firstTextNode(li);return n?n.textContent||"":"";}
function cleanLabel(li){return rawLabel(li).trim();}
function childList(li){for(var i=0;i<li.children.length;i++){if(li.children[i].tagName==="UL"){return li.children[i];}}return null;}
function isFolder(li){return !!childList(li);}
function trimmedPathPart(text){if(text==="/"){return "";}if(text.endsWith("/")){return text.slice(0,-1);}return text;}
function pathFor(li){var parts=[];var cur=li;while(cur){var label=trimmedPathPart(cleanLabel(cur));if(label){parts.push(label);}var parent=cur.parentElement;cur=parent&&parent.closest("li");}parts.reverse();return parts.join("/");}
function encodePath(path){if(!path){return "";}return path.split("/").filter(Boolean).map(function(seg){return encodeURIComponent(seg);}).join("/");}
function dlHref(root,path){var enc=encodePath(path);return enc?"/dl/"+root+"/"+enc:"#";}
function upHref(root,dir,name){var base="/up/"+root;var enc=encodePath(dir);if(enc){base+="/"+enc;}return base+"?name="+encodeURIComponent(name);}
function mkdirHref(root,dir,name){var base="/mkdir/"+root;var enc=encodePath(dir);if(enc){base+="/"+enc;}return base+"?name="+encodeURIComponent(name);}
function setStatus(target,msg){if(target){target.textContent=msg;}}
function uploadFile(root,dir,file,status){if(!file){return;}var started=Date.now();var xhr=new XMLHttpRequest();xhr.open("POST",upHref(root,dir,file.name));xhr.setRequestHeader("Content-Type","application/octet-stream");xhr.upload.onprogress=function(ev){if(!ev.lengthComputable){setStatus(status,"uploading "+file.name+" ...");return;}var elapsed=Math.max((Date.now()-started)/1000,0.001);var rate=ev.loaded/elapsed;setStatus(status,"uploading "+file.name+" "+ev.loaded+"/"+ev.total+" bytes @ "+Math.round(rate/1024)+" KiB/s");};xhr.onload=function(){if(xhr.status>=200&&xhr.status<300){setStatus(status,"uploaded "+file.name);window.setTimeout(function(){window.location.reload();},250);}else{setStatus(status,"upload failed: HTTP "+xhr.status);}};xhr.onerror=function(){setStatus(status,"upload failed");};xhr.send(file);}
function wireFile(root,li,path,label){if(label===".keep"){li.hidden=true;return;}var text=firstTextNode(li);if(text){text.textContent="";}var a=document.createElement("a");a.href=dlHref(root,path);a.textContent=label;a.setAttribute("download","");li.insertBefore(a,li.firstChild);}
function wireFolder(root,li,path){if(li.getAttribute("data-trueosfs-folder")==="1"){return;}li.setAttribute("data-trueosfs-folder","1");var text=firstTextNode(li);if(text){text.textContent=trimmedPathPart(text.textContent||"");}var host=document.createElement("span");var uploadBtn=document.createElement("button");uploadBtn.type="button";uploadBtn.textContent="upload";var createBtn=document.createElement("button");createBtn.type="button";createBtn.textContent="+";var picker=document.createElement("input");picker.type="file";picker.hidden=true;var status=document.createElement("small");host.appendChild(document.createTextNode(" "));host.appendChild(uploadBtn);host.appendChild(document.createTextNode(" "));host.appendChild(createBtn);host.appendChild(document.createTextNode(" "));host.appendChild(status);li.insertBefore(host,childList(li));li.appendChild(picker);uploadBtn.addEventListener("click",function(){picker.click();});picker.addEventListener("change",function(){if(picker.files&&picker.files[0]){uploadFile(root,path,picker.files[0],status);}picker.value="";});createBtn.addEventListener("click",function(){var name=window.prompt("Folder name");if(!name){return;}setStatus(status,"creating "+name+" ...");fetch(mkdirHref(root,path,name),{method:"POST"}).then(function(resp){if(!resp.ok){throw new Error(String(resp.status));}setStatus(status,"created "+name);window.setTimeout(function(){window.location.reload();},250);}).catch(function(err){setStatus(status,"create failed: "+err.message);});});}
function wireTree(root,tree){var nodes=tree.querySelectorAll("li");for(var i=0;i<nodes.length;i++){var li=nodes[i];var label=cleanLabel(li);if(!label){continue;}var path=pathFor(li);if(isFolder(li)){wireFolder(root,li,path);}else if(path){wireFile(root,li,path,label);}}}
function bootClock(){var status=document.getElementById("ws-time-status");var value=document.getElementById("ws-time-value");var meta=document.getElementById("ws-time-meta");if(!status||!value||!meta||!window.WebSocket){return;}var host=window.location.hostname||"localhost";var url="ws://"+host+":56765/time";var sock=null;var retry=0;function set(text){status.textContent=text;}function later(){if(retry){return;}retry=window.setTimeout(function(){retry=0;open();},3000);}function open(){set("connecting to "+url);try{sock=new WebSocket(url);}catch(_err){set("websocket unavailable");later();return;}sock.onopen=function(){set("live");};sock.onmessage=function(ev){try{var msg=JSON.parse(ev.data);if(msg&&msg.unix){value.textContent=new Date(msg.unix*1000).toLocaleString();meta.textContent=(msg.utc||"")+" | source: "+(msg.source||"unknown")+" | unix: "+msg.unix;}}catch(_err){set("bad payload");}};sock.onclose=function(){set("disconnected, retrying");later();};sock.onerror=function(){set("socket error");};}open();window.addEventListener("beforeunload",function(){if(sock){sock.close();}});}
function boot(){var trees=document.querySelectorAll(".tree[data-root]");for(var i=0;i<trees.length;i++){wireTree(trees[i].getAttribute("data-root")||"",trees[i]);}bootClock();}
if(document.readyState==="loading"){document.addEventListener("DOMContentLoaded",boot);}else{boot();}
})();</script>"##
}

fn http_clock_html() -> &'static str {
    "<section><p><strong id=\"ws-time-value\">waiting for live time...</strong></p><p id=\"ws-time-status\">starting websocket clock</p><p id=\"ws-time-meta\">guest ws endpoint: :56765/time</p></section>"
}

fn http_mount_page(roots_html: &str, has_roots: bool) -> HttpResponsePlan {
    let body = if has_roots {
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>TRUEOSFS</title>{}</head><body><h1>TRUEOSFS</h1>{}{}</body></html>",
            http_root_tree_script(),
            http_clock_html(),
            roots_html
        )
    } else {
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>TRUEOSFS</title>{}</head><body><h1>TRUEOSFS</h1>{}<p>(no TRUEOSFS mounted)</p></body></html>",
            http_root_tree_script(),
            http_clock_html()
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
        let mut active_req: Vec<u8> = Vec::new();

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
                        active_req.clear();
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
                        active_req.extend_from_slice(data.as_slice());
                        if active_req.len() > HTTP_TRUEOSFS_MAX_REQUEST_BYTES {
                            sent_for_active = true;
                            active_close = true;
                            active_sent = 0;
                            let response = http_plain_response(
                                "HTTP/1.1 413 Payload Too Large\r\n",
                                "request too large\n",
                            );
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
                            active_pending = header.len().saturating_add(body_len_usize);
                            for chunk in header.as_bytes().chunks(api::MAX_MSG) {
                                let _ = vnet.submit(api::Command::SendTcp {
                                    handle,
                                    data: api::ByteBuf::from_slice_trunc(chunk),
                                });
                            }
                            if let HttpBodyPlan::Bytes(bytes) = body {
                                for chunk in bytes.as_slice().chunks(api::MAX_MSG) {
                                    let _ = vnet.submit(api::Command::SendTcp {
                                        handle,
                                        data: api::ByteBuf::from_slice_trunc(chunk),
                                    });
                                }
                            }
                            continue;
                        }

                        let Some(header_end) = http_header_end(active_req.as_slice()) else {
                            continue;
                        };
                        let req_line = match http_parse_request_line(active_req.as_slice()) {
                            Some(v) => v,
                            None => {
                                sent_for_active = true;
                                active_close = true;
                                active_sent = 0;
                                let response = http_plain_response(
                                    "HTTP/1.1 400 Bad Request\r\n",
                                    "bad request\n",
                                );
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
                                active_pending = header.len().saturating_add(body_len_usize);
                                for chunk in header.as_bytes().chunks(api::MAX_MSG) {
                                    let _ = vnet.submit(api::Command::SendTcp {
                                        handle,
                                        data: api::ByteBuf::from_slice_trunc(chunk),
                                    });
                                }
                                if let HttpBodyPlan::Bytes(bytes) = body {
                                    for chunk in bytes.as_slice().chunks(api::MAX_MSG) {
                                        let _ = vnet.submit(api::Command::SendTcp {
                                            handle,
                                            data: api::ByteBuf::from_slice_trunc(chunk),
                                        });
                                    }
                                }
                                continue;
                            }
                        };
                        let content_len = http_content_length(active_req.as_slice()).unwrap_or(0);
                        let total_needed = header_end.saturating_add(content_len);
                        if total_needed > HTTP_TRUEOSFS_MAX_REQUEST_BYTES {
                            continue;
                        }
                        if active_req.len() < total_needed {
                            continue;
                        }
                        sent_for_active = true;

                        let req = &active_req[..total_needed];
                        let target = req_line.target;
                        let method = req_line.method;
                        let body_bytes = &req[header_end..total_needed];

                        let roots = crate::v::fs::trueosfs::list_roots();

                        let response: HttpResponsePlan = if method == "GET"
                            && http_path_only(target).starts_with("/dl/")
                        {
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

                                let path = match http_normalize_rel_path_decoded(enc_path, 240) {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "bad path\n",
                                        )
                                    }
                                };

                                if path.is_empty() {
                                    break 'resp http_plain_response(
                                        "HTTP/1.1 400 Bad Request\r\n",
                                        "bad path\n",
                                    );
                                }

                                http_prepare_file_response(disk, path, req).await
                            }
                        } else if method == "GET" && target.starts_with("/dl") {
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
                                    Some(enc_path) => match http_normalize_rel_path_decoded(enc_path, 240)
                                    {
                                        None => http_plain_response(
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "bad path\n",
                                        ),
                                        Some(path) => {
                                            if path.is_empty() {
                                                http_plain_response(
                                                    "HTTP/1.1 400 Bad Request\r\n",
                                                    "bad path\n",
                                                )
                                            } else {
                                                http_prepare_file_response(disk, path, req).await
                                            }
                                        }
                                    },
                                },
                            }
                        } else if method == "POST" && http_path_only(target).starts_with("/up/") {
                            'resp: {
                                let (root_raw, dir) = match http_parse_root_and_dir(target, "/up/") {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "bad upload path\n",
                                        )
                                    }
                                };
                                let name = match http_query_param(target, "name")
                                    .and_then(|v| http_normalize_name(v, 120))
                                {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "missing file name\n",
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
                                let full_path = http_join_rel_path(dir.as_str(), name.as_str());
                                match crate::v::fs::trueosfs::file_in_async(
                                    disk,
                                    full_path.as_str(),
                                    body_bytes,
                                )
                                .await
                                {
                                    Ok(true) => http_plain_response(
                                        "HTTP/1.1 200 OK\r\n",
                                        "upload ok\n",
                                    ),
                                    Ok(false) => http_plain_response(
                                        "HTTP/1.1 507 Insufficient Storage\r\n",
                                        "upload failed\n",
                                    ),
                                    Err(_) => http_plain_response(
                                        "HTTP/1.1 500 Internal Server Error\r\n",
                                        "upload error\n",
                                    ),
                                }
                            }
                        } else if method == "POST" && http_path_only(target).starts_with("/mkdir/") {
                            'resp: {
                                let (root_raw, dir) = match http_parse_root_and_dir(target, "/mkdir/") {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "bad mkdir path\n",
                                        )
                                    }
                                };
                                let name = match http_query_param(target, "name")
                                    .and_then(|v| http_normalize_name(v, 120))
                                {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "missing folder name\n",
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
                                let folder = http_join_rel_path(dir.as_str(), name.as_str());
                                let marker = http_join_rel_path(folder.as_str(), ".keep");
                                match crate::v::fs::trueosfs::file_in_async(disk, marker.as_str(), &[])
                                    .await
                                {
                                    Ok(true) => http_plain_response(
                                        "HTTP/1.1 200 OK\r\n",
                                        "mkdir ok\n",
                                    ),
                                    Ok(false) => http_plain_response(
                                        "HTTP/1.1 507 Insufficient Storage\r\n",
                                        "mkdir failed\n",
                                    ),
                                    Err(_) => http_plain_response(
                                        "HTTP/1.1 500 Internal Server Error\r\n",
                                        "mkdir error\n",
                                    ),
                                }
                            }
                        } else {
                            // Build HTML trees (best-effort), one per mounted TRUEOSFS root.
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
                                        "<details><summary>{}</summary><div class=\"tree\" data-root=\"{}\">",
                                        r.disk_id, raw
                                    )
                                        .as_str(),
                                );
                                if let Some(t) = tree {
                                    trees_html.push_str(t.as_str());
                                } else {
                                    trees_html.push_str("<p>(tree unavailable)</p>");
                                }
                                trees_html.push_str("</div></details>");
                            }
                            http_mount_page(trees_html.as_str(), !roots.is_empty())
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
                    active_pending = header.len().saturating_add(body_len_usize);
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
                        active_req.clear();
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
                api::Event::UdpPacketV6 { .. } => {},
                api::Event::IcmpReply { .. } => {},
                api::Event::IcmpReplyV6 { .. } => {},
            }
        }

        Timer::after(EmbassyDuration::from_millis(10)).await;
    }
    }.await;
}
