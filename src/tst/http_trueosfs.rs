extern crate alloc;

use alloc::vec;
use alloc::{format, string::String, string::ToString, vec::Vec};

use embassy_time::{Duration as EmbassyDuration, Timer};

use v::vhttp_srv;
use v::vnet as api;

use crate::disc::block::DeviceHandle;
use crate::r::net::{VNet, ports};

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

fn http_log_target_preview(prefix: &str, target: &str) {
    let path = vhttp_srv::path_only(target);
    crate::log!("http-trueosfs: {} target={}\n", prefix, path);
}

fn http_submit_commands(vnet: &VNet, cmds: Vec<api::Command>) {
    for cmd in cmds {
        let _ = vnet.submit(cmd);
    }
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
    let decoded = vhttp_srv::url_decode(raw, max_len)?;
    http_normalize_rel_path(decoded.as_str(), max_len)
}

fn http_normalize_name(raw: &str, max_len: usize) -> Option<String> {
    let decoded = vhttp_srv::url_decode(raw, max_len)?;
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
    let rest = vhttp_srv::path_only(target).strip_prefix(prefix)?;
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
    let info = match crate::r::fs::trueosfs::file_info_async(disk, path.as_str()).await {
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

    if let Some(value) = vhttp_srv::find_header(req, "If-None-Match")
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
    if let Some(value) = vhttp_srv::find_header(req, "If-Range")
        && !http_etag_matches(value, etag.as_str())
    {
        allow_ranges = false;
    }

    let mut ranges: Option<Vec<(u64, u64)>> = None;
    if allow_ranges && let Some(value) = vhttp_srv::find_header(req, "Range") {
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
function rmHref(root,path){var enc=encodePath(path);return enc?"/rm/"+root+"/"+enc:"#";}
function upHref(root,dir,name){var base="/up/"+root;var enc=encodePath(dir);if(enc){base+="/"+enc;}return base+"?name="+encodeURIComponent(name);}
function mkdirHref(root,dir,name){var base="/mkdir/"+root;var enc=encodePath(dir);if(enc){base+="/"+enc;}return base+"?name="+encodeURIComponent(name);}
function setStatus(target,msg){if(target){target.textContent=msg;}}
function uploadFile(root,dir,file,status){if(!file){return;}var started=Date.now();var xhr=new XMLHttpRequest();xhr.open("POST",upHref(root,dir,file.name));xhr.setRequestHeader("Content-Type","application/octet-stream");xhr.upload.onprogress=function(ev){if(!ev.lengthComputable){setStatus(status,"uploading "+file.name+" ...");return;}var elapsed=Math.max((Date.now()-started)/1000,0.001);var rate=ev.loaded/elapsed;setStatus(status,"uploading "+file.name+" "+ev.loaded+"/"+ev.total+" bytes @ "+Math.round(rate/1024)+" KiB/s");};xhr.onload=function(){if(xhr.status>=200&&xhr.status<300){setStatus(status,"uploaded "+file.name);window.setTimeout(function(){window.location.reload();},250);}else{setStatus(status,"upload failed: HTTP "+xhr.status);}};xhr.onerror=function(){setStatus(status,"upload failed");};xhr.send(file);}
function deleteFile(root,path,label,status){if(!window.confirm("Delete "+label+"?")){return;}setStatus(status,"deleting "+label+" ...");fetch(rmHref(root,path),{method:"POST"}).then(function(resp){if(!resp.ok){throw new Error(String(resp.status));}setStatus(status,"deleted "+label);window.setTimeout(function(){window.location.reload();},250);}).catch(function(err){setStatus(status,"delete failed: "+err.message);});}
function wireFile(root,li,path,label){if(label===".keep"){li.hidden=true;return;}var text=firstTextNode(li);if(text){text.textContent="";}var del=document.createElement("button");del.type="button";del.textContent="x";var a=document.createElement("a");a.href=dlHref(root,path);a.textContent=label;a.setAttribute("download","");var status=document.createElement("small");li.insertBefore(del,li.firstChild);li.insertBefore(document.createTextNode(" "),del.nextSibling);li.insertBefore(a,del.nextSibling.nextSibling);li.appendChild(document.createTextNode(" "));li.appendChild(status);del.addEventListener("click",function(){deleteFile(root,path,label,status);});}
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
                port: ports::HTTP_TRUEOSFS_TCP_PORT,
            })
            .is_err()
        {
            crate::log!("http-trueosfs: listen submit failed\n");
            return;
        }

        crate::log!(
            "http-trueosfs: listening on tcp {} (hostfwd localhost:8080 -> guest:80)\n",
            ports::HTTP_TRUEOSFS_TCP_PORT
        );

        let mut server = vhttp_srv::HttpServer::new(
            ports::HTTP_TRUEOSFS_TCP_PORT,
            HTTP_TRUEOSFS_MAX_REQUEST_BYTES,
        );
        let mut listener_ready = false;

        loop {
            while let Some(ev) = vnet.pop_event() {
                if !listener_ready
                    && let api::Event::Opened {
                        kind: api::SocketKind::Tcp,
                        ..
                    } = ev
                {
                    listener_ready = true;
                    crate::r::readiness::set(crate::r::readiness::HTTP_TRUEOSFS_LISTENING);
                    crate::log!("http-trueosfs: tcp listen opened and ready\n");
                }

                match server.on_event(ev) {
                    vhttp_srv::HttpServerEvent::None => {}
                    vhttp_srv::HttpServerEvent::Submit(cmd) => {
                        let _ = vnet.submit(cmd);
                    }
                    vhttp_srv::HttpServerEvent::Error(msg) => {
                        crate::log!("http-trueosfs: error {}\n", msg);
                    }
                    vhttp_srv::HttpServerEvent::RequestTooLarge { handle } => {
                        let body = b"request too large\n";
                        let mut cmds = Vec::new();
                        let pending = vhttp_srv::queue_response_head(
                            &mut cmds,
                            handle,
                            "HTTP/1.1 413 Payload Too Large\r\n",
                            "text/plain; charset=utf-8",
                            "",
                            body.len() as u64,
                            false,
                        );
                        server.mark_response(handle, pending, false);
                        vhttp_srv::queue_send_bytes(&mut cmds, handle, body);
                        http_submit_commands(&vnet, cmds);
                    }
                    vhttp_srv::HttpServerEvent::BadRequest { handle } => {
                        crate::log!("http-trueosfs: 400 bad request line/header parse\n");
                        let body = b"bad request\n";
                        let mut cmds = Vec::new();
                        let pending = vhttp_srv::queue_response_head(
                            &mut cmds,
                            handle,
                            "HTTP/1.1 400 Bad Request\r\n",
                            "text/plain; charset=utf-8",
                            "",
                            body.len() as u64,
                            false,
                        );
                        server.mark_response(handle, pending, false);
                        vhttp_srv::queue_send_bytes(&mut cmds, handle, body);
                        http_submit_commands(&vnet, cmds);
                    }
                    vhttp_srv::HttpServerEvent::RequestReady { handle, request } => {
                        let method = request.method().to_string();
                        let target = request.target().to_string();
                        let req = request.raw_bytes();
                        let body_bytes = request.body_bytes();
                        let roots = crate::r::fs::trueosfs::list_roots();

                        let response: HttpResponsePlan = if method == "GET"
                            && vhttp_srv::path_only(target.as_str()).starts_with("/dl/")
                        {
                            // Download endpoint: /dl/<root_raw>/<path>
                            // (where <root_raw> is the DiscId raw value as decimal)
                            'resp: {
                                let path_only = vhttp_srv::path_only(target.as_str());
                                let rest = path_only.strip_prefix("/dl/").unwrap_or("");

                                let (root_raw_s, enc_path) = match rest.split_once('/') {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "missing root/path\n",
                                        );
                                    }
                                };

                                let root_raw = match root_raw_s.parse::<u32>() {
                                    Ok(v) => v,
                                    Err(_) => {
                                        break 'resp http_plain_response("HTTP/1.1 400 Bad Request\r\n", "bad root\n");
                                    }
                                };

                                let root = match roots.iter().find(|r| r.disk_id.raw() == root_raw) {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response("HTTP/1.1 404 Not Found\r\n", "unknown root\n");
                                    }
                                };

                                let disk = match crate::disc::block::device_handle(root.disk_id) {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 503 Service Unavailable\r\n",
                                            "root unavailable\n",
                                        );
                                    }
                                };

                                let path = match http_normalize_rel_path_decoded(enc_path, 240) {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response("HTTP/1.1 400 Bad Request\r\n", "bad path\n");
                                    }
                                };

                                if path.is_empty() {
                                    break 'resp http_plain_response("HTTP/1.1 400 Bad Request\r\n", "bad path\n");
                                }

                                http_prepare_file_response(disk, path, &req).await
                            }
                        } else if method == "GET" && target.starts_with("/dl") {
                            // Back-compat download endpoint: /dl?path=<urlencoded path> (uses primary root)
                            let disk = crate::r::fs::trueosfs::primary_root_handle();
                            match disk {
                                None => {
                                    http_plain_response("HTTP/1.1 503 Service Unavailable\r\n", "no TRUEOSFS mounted\n")
                                }
                                Some(disk) => match vhttp_srv::query_param(target.as_str(), "path") {
                                    None => http_plain_response("HTTP/1.1 400 Bad Request\r\n", "missing path\n"),
                                    Some(enc_path) => match http_normalize_rel_path_decoded(enc_path, 240) {
                                        None => http_plain_response("HTTP/1.1 400 Bad Request\r\n", "bad path\n"),
                                        Some(path) => {
                                            if path.is_empty() {
                                                http_plain_response("HTTP/1.1 400 Bad Request\r\n", "bad path\n")
                                            } else {
                                                http_prepare_file_response(disk, path, &req).await
                                            }
                                        }
                                    },
                                },
                            }
                        } else if method == "POST" && vhttp_srv::path_only(target.as_str()).starts_with("/up/") {
                            'resp: {
                                let (root_raw, dir) = match http_parse_root_and_dir(target.as_str(), "/up/") {
                                    Some(v) => v,
                                    None => {
                                        http_log_target_preview("400 bad upload path", target.as_str());
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "bad upload path\n",
                                        );
                                    }
                                };
                                let name = match vhttp_srv::query_param(target.as_str(), "name")
                                    .and_then(|v| http_normalize_name(v, 120))
                                {
                                    Some(v) => v,
                                    None => {
                                        http_log_target_preview("400 missing file name", target.as_str());
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "missing file name\n",
                                        );
                                    }
                                };
                                let root = match roots.iter().find(|r| r.disk_id.raw() == root_raw) {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response("HTTP/1.1 404 Not Found\r\n", "unknown root\n");
                                    }
                                };
                                let disk = match crate::disc::block::device_handle(root.disk_id) {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 503 Service Unavailable\r\n",
                                            "root unavailable\n",
                                        );
                                    }
                                };
                                let full_path = http_join_rel_path(dir.as_str(), name.as_str());
                                match crate::r::fs::trueosfs::file_in_async(disk, full_path.as_str(), body_bytes).await
                                {
                                    Ok(true) => http_plain_response("HTTP/1.1 200 OK\r\n", "upload ok\n"),
                                    Ok(false) => {
                                        http_plain_response("HTTP/1.1 507 Insufficient Storage\r\n", "upload failed\n")
                                    }
                                    Err(_) => {
                                        http_plain_response("HTTP/1.1 500 Internal Server Error\r\n", "upload error\n")
                                    }
                                }
                            }
                        } else if method == "POST" && vhttp_srv::path_only(target.as_str()).starts_with("/mkdir/") {
                            'resp: {
                                let (root_raw, dir) = match http_parse_root_and_dir(target.as_str(), "/mkdir/") {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "bad mkdir path\n",
                                        );
                                    }
                                };
                                let name = match vhttp_srv::query_param(target.as_str(), "name")
                                    .and_then(|v| http_normalize_name(v, 120))
                                {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "missing folder name\n",
                                        );
                                    }
                                };
                                let root = match roots.iter().find(|r| r.disk_id.raw() == root_raw) {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response("HTTP/1.1 404 Not Found\r\n", "unknown root\n");
                                    }
                                };
                                let disk = match crate::disc::block::device_handle(root.disk_id) {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 503 Service Unavailable\r\n",
                                            "root unavailable\n",
                                        );
                                    }
                                };
                                let folder = http_join_rel_path(dir.as_str(), name.as_str());
                                let marker = http_join_rel_path(folder.as_str(), ".keep");
                                match crate::r::fs::trueosfs::file_in_async(disk, marker.as_str(), &[]).await {
                                    Ok(true) => http_plain_response("HTTP/1.1 200 OK\r\n", "mkdir ok\n"),
                                    Ok(false) => {
                                        http_plain_response("HTTP/1.1 507 Insufficient Storage\r\n", "mkdir failed\n")
                                    }
                                    Err(_) => {
                                        http_plain_response("HTTP/1.1 500 Internal Server Error\r\n", "mkdir error\n")
                                    }
                                }
                            }
                        } else if method == "POST" && vhttp_srv::path_only(target.as_str()).starts_with("/rm/") {
                            'resp: {
                                let path_only = vhttp_srv::path_only(target.as_str());
                                let rest = path_only.strip_prefix("/rm/").unwrap_or("");
                                let (root_raw_s, enc_path) = match rest.split_once('/') {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "missing root/path\n",
                                        );
                                    }
                                };
                                let root_raw = match root_raw_s.parse::<u32>() {
                                    Ok(v) => v,
                                    Err(_) => {
                                        break 'resp http_plain_response("HTTP/1.1 400 Bad Request\r\n", "bad root\n");
                                    }
                                };
                                let path = match http_normalize_rel_path_decoded(enc_path, 240) {
                                    Some(v) if !v.is_empty() => v,
                                    _ => break 'resp http_plain_response("HTTP/1.1 400 Bad Request\r\n", "bad path\n"),
                                };
                                let root = match roots.iter().find(|r| r.disk_id.raw() == root_raw) {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response("HTTP/1.1 404 Not Found\r\n", "unknown root\n");
                                    }
                                };
                                let disk = match crate::disc::block::device_handle(root.disk_id) {
                                    Some(v) => v,
                                    None => {
                                        break 'resp http_plain_response(
                                            "HTTP/1.1 503 Service Unavailable\r\n",
                                            "root unavailable\n",
                                        );
                                    }
                                };
                                match crate::r::fs::trueosfs::file_delete_async(disk, path.as_str()).await {
                                    Ok(true) => http_plain_response("HTTP/1.1 200 OK\r\n", "delete ok\n"),
                                    Ok(false) => http_plain_response("HTTP/1.1 404 Not Found\r\n", "not found\n"),
                                    Err(_) => {
                                        http_plain_response("HTTP/1.1 500 Internal Server Error\r\n", "delete error\n")
                                    }
                                }
                            }
                        } else {
                            // Build HTML trees (best-effort), one per mounted TRUEOSFS root.
                            let mut trees_html = String::new();
                            for r in roots.iter() {
                                let raw = r.disk_id.raw();

                                let tree = match crate::disc::block::device_handle(r.disk_id) {
                                    None => None,
                                    Some(disk) => {
                                        match crate::r::fs::trueosfs::html_tree_async(disk, HTTP_TRUEOSFS_MAX_ENTRIES)
                                            .await
                                        {
                                            Ok(v) => v,
                                            Err(_) => None,
                                        }
                                    }
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
                        let mut cmds = Vec::new();
                        let pending = vhttp_srv::queue_response_head(
                            &mut cmds,
                            handle,
                            status,
                            content_type,
                            extra_headers.as_str(),
                            body_len,
                            request.keep_alive(),
                        );
                        server.mark_response(handle, pending, request.keep_alive());
                        http_submit_commands(&vnet, cmds);

                        let mut perf = HttpPerf::default();

                        match body {
                            HttpBodyPlan::Bytes(bytes) => {
                                let mut cmds = Vec::new();
                                vhttp_srv::queue_send_bytes(&mut cmds, handle, bytes.as_slice());
                                http_submit_commands(&vnet, cmds);
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
                                    let read = match crate::r::fs::trueosfs::file_read_range_async(
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
                                    let mut cmds = Vec::new();
                                    vhttp_srv::queue_send_bytes(&mut cmds, handle, &buf[..read]);
                                    http_submit_commands(&vnet, cmds);
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
                                    let mut cmds = Vec::new();
                                    vhttp_srv::queue_send_bytes(&mut cmds, handle, part.header.as_bytes());
                                    http_submit_commands(&vnet, cmds);
                                    let mut remaining = part.end.saturating_sub(part.start).saturating_add(1);
                                    let mut off = part.start;
                                    while remaining > 0 {
                                        let want = core::cmp::min(remaining, buf.len() as u64) as usize;
                                        let t0 = tsc_now();
                                        let read = match crate::r::fs::trueosfs::file_read_range_async(
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
                                        let mut cmds = Vec::new();
                                        vhttp_srv::queue_send_bytes(&mut cmds, handle, &buf[..read]);
                                        http_submit_commands(&vnet, cmds);
                                        let t3 = tsc_now();
                                        perf.record_submit(t3.wrapping_sub(t2), read);
                                        off = off.saturating_add(read as u64);
                                        remaining = remaining.saturating_sub(read as u64);
                                    }
                                    let mut cmds = Vec::new();
                                    vhttp_srv::queue_send_bytes(&mut cmds, handle, b"\r\n");
                                    http_submit_commands(&vnet, cmds);
                                }
                                let closing = format!("--{}--\r\n", boundary);
                                let mut cmds = Vec::new();
                                vhttp_srv::queue_send_bytes(&mut cmds, handle, closing.as_bytes());
                                http_submit_commands(&vnet, cmds);
                            }
                            HttpBodyPlan::None => {}
                        }

                        perf.log();
                    }
                }
            }

            Timer::after(EmbassyDuration::from_millis(10)).await;
        }
    }
    .await;
}
