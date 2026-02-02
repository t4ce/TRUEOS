extern crate alloc;

use alloc::{format, string::String, vec::Vec};

use embassy_time::{Duration as EmbassyDuration, Timer};

use trueos_v::vnet as api;

use crate::v::net::VNet;

const HTTP_TRUEOSFS_TCP_PORT: u16 = 80;
const HTTP_TRUEOSFS_MAX_ENTRIES: usize = 256;

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

fn http_sanitize_filename(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        match ch {
            '"' | '\\' | '\r' | '\n' => out.push('_'),
            _ => out.push(ch),
        }
    }
    if out.is_empty() {
        out.push_str("download.bin");
    }
    out
}

#[embassy_executor::task]
pub async fn http_trueosfs_task() {
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
                    let primary_raw = crate::v::fs::trueosfs::primary_root_id().map(|v| v.raw());

                    let (status, content_type, extra_headers, body_bytes): (
                        &'static str,
                        &'static str,
                        String,
                        Vec<u8>,
                    ) = if http_path_only(target).starts_with("/dl/") {
                        // Download endpoint: /dl/<root_raw>/<path>
                        // (where <root_raw> is the DiscId raw value as decimal)
                        'resp: {
                            let mut extra_headers = String::new();
                            let path_only = http_path_only(target);
                            let rest = path_only.strip_prefix("/dl/").unwrap_or("");

                            let (root_raw_s, enc_path) = match rest.split_once('/') {
                                Some(v) => v,
                                None => {
                                    break 'resp (
                                        "HTTP/1.1 400 Bad Request\r\n",
                                        "text/plain; charset=utf-8",
                                        extra_headers,
                                        b"missing root/path\n".to_vec(),
                                    )
                                }
                            };

                            let root_raw = match root_raw_s.parse::<u32>() {
                                Ok(v) => v,
                                Err(_) => {
                                    break 'resp (
                                        "HTTP/1.1 400 Bad Request\r\n",
                                        "text/plain; charset=utf-8",
                                        extra_headers,
                                        b"bad root\n".to_vec(),
                                    )
                                }
                            };

                            let root = match roots.iter().find(|r| r.disk_id.raw() == root_raw) {
                                Some(v) => v,
                                None => {
                                    break 'resp (
                                        "HTTP/1.1 404 Not Found\r\n",
                                        "text/plain; charset=utf-8",
                                        extra_headers,
                                        b"unknown root\n".to_vec(),
                                    )
                                }
                            };

                            let disk = match crate::disc::block::device_handle(root.disk_id) {
                                Some(v) => v,
                                None => {
                                    break 'resp (
                                        "HTTP/1.1 503 Service Unavailable\r\n",
                                        "text/plain; charset=utf-8",
                                        extra_headers,
                                        b"root unavailable\n".to_vec(),
                                    )
                                }
                            };

                            let mut path = match http_url_decode(enc_path, 240) {
                                Some(v) => v,
                                None => {
                                    break 'resp (
                                        "HTTP/1.1 400 Bad Request\r\n",
                                        "text/plain; charset=utf-8",
                                        extra_headers,
                                        b"bad path\n".to_vec(),
                                    )
                                }
                            };

                            while path.starts_with('/') {
                                path.remove(0);
                            }
                            if path.is_empty() || path.contains("..") {
                                break 'resp (
                                    "HTTP/1.1 400 Bad Request\r\n",
                                    "text/plain; charset=utf-8",
                                    extra_headers,
                                    b"bad path\n".to_vec(),
                                );
                            }

                            match crate::v::fs::trueosfs::file_out_async(disk, path.as_str()).await {
                                Ok(Some(bytes)) => {
                                    let fname_raw =
                                        path.rsplit('/').next().unwrap_or("download.bin");
                                    let fname = http_sanitize_filename(fname_raw);
                                    extra_headers.push_str(
                                        "Content-Disposition: attachment; filename=\"",
                                    );
                                    extra_headers.push_str(fname.as_str());
                                    extra_headers.push_str("\"\r\n");
                                    break 'resp (
                                        "HTTP/1.1 200 OK\r\n",
                                        "application/octet-stream",
                                        extra_headers,
                                        bytes,
                                    );
                                }
                                Ok(None) => {
                                    break 'resp (
                                        "HTTP/1.1 404 Not Found\r\n",
                                        "text/plain; charset=utf-8",
                                        extra_headers,
                                        b"not found\n".to_vec(),
                                    )
                                }
                                Err(_) => {
                                    break 'resp (
                                        "HTTP/1.1 500 Internal Server Error\r\n",
                                        "text/plain; charset=utf-8",
                                        extra_headers,
                                        b"read failed\n".to_vec(),
                                    )
                                }
                            }
                        }
                    } else if target.starts_with("/dl") {
                        // Back-compat download endpoint: /dl?path=<urlencoded path> (uses primary root)
                        let disk = crate::v::fs::trueosfs::primary_root_handle();
                        match disk {
                            None => (
                                "HTTP/1.1 503 Service Unavailable\r\n",
                                "text/plain; charset=utf-8",
                                String::new(),
                                b"no TRUEOSFS mounted\n".to_vec(),
                            ),
                            Some(disk) => {
                                let mut extra_headers = String::new();
                                match http_query_param(target, "path") {
                                    None => (
                                        "HTTP/1.1 400 Bad Request\r\n",
                                        "text/plain; charset=utf-8",
                                        extra_headers,
                                        b"missing path\n".to_vec(),
                                    ),
                                    Some(enc_path) => match http_url_decode(enc_path, 240) {
                                        None => (
                                            "HTTP/1.1 400 Bad Request\r\n",
                                            "text/plain; charset=utf-8",
                                            extra_headers,
                                            b"bad path\n".to_vec(),
                                        ),
                                        Some(mut path) => {
                                            while path.starts_with('/') {
                                                path.remove(0);
                                            }
                                            if path.is_empty() || path.contains("..") {
                                                (
                                                    "HTTP/1.1 400 Bad Request\r\n",
                                                    "text/plain; charset=utf-8",
                                                    extra_headers,
                                                    b"bad path\n".to_vec(),
                                                )
                                            } else {
                                                match crate::v::fs::trueosfs::file_out_async(
                                                    disk,
                                                    path.as_str(),
                                                )
                                                .await
                                                {
                                                    Ok(Some(bytes)) => {
                                                        let fname_raw = path
                                                            .rsplit('/')
                                                            .next()
                                                            .unwrap_or("download.bin");
                                                        let fname =
                                                            http_sanitize_filename(fname_raw);
                                                        extra_headers.push_str(
                                                            "Content-Disposition: attachment; filename=\"",
                                                        );
                                                        extra_headers.push_str(fname.as_str());
                                                        extra_headers.push_str("\"\r\n");
                                                        (
                                                            "HTTP/1.1 200 OK\r\n",
                                                            "application/octet-stream",
                                                            extra_headers,
                                                            bytes,
                                                        )
                                                    }
                                                    Ok(None) => (
                                                        "HTTP/1.1 404 Not Found\r\n",
                                                        "text/plain; charset=utf-8",
                                                        extra_headers,
                                                        b"not found\n".to_vec(),
                                                    ),
                                                    Err(_) => (
                                                        "HTTP/1.1 500 Internal Server Error\r\n",
                                                        "text/plain; charset=utf-8",
                                                        extra_headers,
                                                        b"read failed\n".to_vec(),
                                                    ),
                                                }
                                            }
                                        }
                                    },
                                }
                            }
                        }
                    } else {
                        // Build HTML trees (best-effort), one per mounted TRUEOSFS root.
                        let mut selected_raw = primary_raw;

                        if let Some(enc_root) = http_query_param(target, "root") {
                            if let Some(root_s) = http_url_decode(enc_root, 16) {
                                if let Ok(v) = root_s.parse::<u32>() {
                                    if roots.iter().any(|r| r.disk_id.raw() == v) {
                                        selected_raw = Some(v);
                                    }
                                }
                            }
                        }
                        if selected_raw.is_none() {
                            selected_raw = roots.first().map(|r| r.disk_id.raw());
                        }

                        let js = r#"<style>
body{font-family:system-ui,-apple-system,Segoe UI,sans-serif;max-width:980px;margin:2rem auto;padding:0 1rem}
ul{line-height:1.35}
li{margin:0.15rem 0}
a{color:#0b66ff;text-decoration:none}
a:hover{text-decoration:underline}
.bar{display:flex;gap:0.75rem;align-items:center;flex-wrap:wrap;margin:0.75rem 0 1rem}
.tree{display:none}
.muted{color:#666}
</style>
<script>
(()=>{
  const sel=document.getElementById("rootSel");
  const trees=[...document.querySelectorAll(".tree")];
  function show(root){
    for(const t of trees){ t.style.display = (t.dataset.root===root) ? "block" : "none"; }
    const url=new URL(location.href);
    url.searchParams.set("root", root);
    history.replaceState(null, "", url.toString());
  }
  if(sel){
    const q=new URLSearchParams(location.search);
    const want=q.get("root");
    if(want && [...sel.options].some(o=>o.value===want)) sel.value=want;
    sel.addEventListener("change", ()=>show(sel.value));
    show(sel.value);
  } else if(trees.length){
    trees[0].style.display="block";
  }

  function labelForLi(li){
    for(const n of li.childNodes){
      if(n.nodeType===Node.TEXT_NODE){
        const t=(n.textContent||"").trim();
        if(t) return t;
      }
    }
    return "";
  }
  function parentLi(li){
    let p=li.parentElement;
    while(p && p.tagName==="UL") p=p.parentElement;
    return (p && p.tagName==="LI") ? p : null;
  }
  function buildPath(li){
    const parts=[];
    let cur=li;
    while(cur){
      const label=labelForLi(cur);
      if(label && label!=="/"){
        let seg=label;
        if(seg.endsWith("/")) seg=seg.slice(0,-1);
        if(seg) parts.push(seg);
      }
      cur=parentLi(cur);
    }
    return parts.reverse().join("/");
  }
  for(const tree of trees){
    const root=tree.dataset.root;
    if(!root) continue;
    for(const li of tree.querySelectorAll("ul li")){
      if(li.querySelector("a")) continue;
      const label=labelForLi(li);
      if(!label || label==="/" || label.endsWith("/")) continue;
      const path=buildPath(li);
      if(!path) continue;
      const a=document.createElement("a");
      a.href="/dl/"+root+"/"+encodeURIComponent(path);
      a.textContent=label;
      a.setAttribute("download", label);
      const firstText=[...li.childNodes].find(n=>n.nodeType===Node.TEXT_NODE && (n.textContent||"").trim());
      if(firstText){ li.insertBefore(a, firstText); li.removeChild(firstText); }
      else { li.insertBefore(a, li.firstChild); }
    }
  }
})();
</script>"#;

                        let mut options = String::new();
                        let mut trees_html = String::new();
                        for r in roots.iter() {
                            let raw = r.disk_id.raw();
                            let selected = selected_raw == Some(raw);
                            options.push_str(
                                format!(
                                    "<option value=\"{}\"{}>{} (raw={} seq={})</option>",
                                    raw,
                                    if selected { " selected" } else { "" },
                                    r.disk_id,
                                    raw,
                                    r.seq
                                )
                                .as_str(),
                            );

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
                                format!("<div class=\"tree\" data-root=\"{}\">", raw)
                                    .as_str(),
                            );
                            if let Some(t) = tree {
                                trees_html.push_str(t.as_str());
                            } else {
                                trees_html.push_str("<p class=\"muted\">(tree unavailable)</p>");
                            }
                            trees_html.push_str("</div>");
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
                                "<!doctype html><html><head><meta charset=\"utf-8\"><title>TRUEOSFS</title>{}</head><body><h1>TRUEOSFS</h1><div class=\"bar\"><label for=\"rootSel\">Root:</label><select id=\"rootSel\">{}</select><span class=\"muted\">(showing {})</span></div><p>Click a file to download.</p>{}</body></html>",
                                js,
                                options,
                                selected_raw.unwrap_or(0),
                                trees_html
                            )
                        };

                        (
                            "HTTP/1.1 200 OK\r\n",
                            "text/html; charset=utf-8",
                            String::new(),
                            body.into_bytes(),
                        )
                    };

                    let mut header = String::new();
                    header.push_str(status);
                    header.push_str("Content-Type: ");
                    header.push_str(content_type);
                    header.push_str("\r\n");
                    if !extra_headers.is_empty() {
                        header.push_str(extra_headers.as_str());
                    }
                    header.push_str("Content-Length: ");
                    header.push_str(format!("{}", body_bytes.len()).as_str());
                    header.push_str("\r\nConnection: close\r\n\r\n");

                    // Send headers + body in MAX_MSG chunks.
                    for chunk in header.as_bytes().chunks(api::MAX_MSG) {
                        let _ = vnet.submit(api::Command::SendTcp {
                            handle,
                            data: api::ByteBuf::from_slice_trunc(chunk),
                        });
                        Timer::after(EmbassyDuration::from_millis(1)).await;
                    }
                    for chunk in body_bytes.as_slice().chunks(api::MAX_MSG) {
                        let _ = vnet.submit(api::Command::SendTcp {
                            handle,
                            data: api::ByteBuf::from_slice_trunc(chunk),
                        });
                        Timer::after(EmbassyDuration::from_millis(1)).await;
                    }

                    let _ = vnet.submit(api::Command::Close { handle });
                }
                api::Event::Closed { handle } => {
                    if active_handle == Some(handle) {
                        active_handle = None;
                        sent_for_active = false;
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
                    crate::log!("http-trueosfs: error {}\n", msg);
                }
                api::Event::TcpSent { .. } => {}
                api::Event::UdpPacket { .. } => {}
            }
        }

        Timer::after(EmbassyDuration::from_millis(10)).await;
    }
}
