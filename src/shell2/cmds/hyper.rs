extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::str::SplitWhitespace;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use v::vnet;

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};
use super::tlb_helper::print_table;
use crate::net::tls::{TlsClientConfig, TlsRoots};
use crate::net::tls_socket::{TlsCommand, TlsEvent, TlsTimeouts, register_tls_app_queues};
use crate::r::net::Queue;
use crate::shell2::shell2_cmd::ParseOutcome;

const HYPER_DOWNLOAD_TIMEOUT_MS: u64 = 90_000;
const HYPER_DOWNLOAD_MAX_BYTES: usize = 128 * 1024 * 1024;
static HYPER_DOWNLOAD_SEQ: AtomicU32 = AtomicU32::new(1);

const HYPER_MENU_HEADERS: [&str; 2] = ["Subcommand", "Description"];
const HYPER_MENU_ROWS: [[&str; 2]; 3] = [
    ["status", "Show the kernel Hyper transport surfaces"],
    ["probe", "Describe the background HTTP/1 probe service"],
    ["<url> [path]", "Download URL into TRUEOSFS"],
];

fn line(io: &'static dyn ShellBackend2, text: &str) {
    print_shell_line(io, text);
}

fn print_status(io: &'static dyn ShellBackend2) {
    line(io, "hyper: client=http1 transport=hyper-vnet");
    line(io, "hyper: http fetch=byte lane");
    line(io, "hyper: https fetch=tls-socket");
    line(io, "hyper: probe=spawn-svc hyper-http1-probe");
}

fn print_probe(io: &'static dyn ShellBackend2) {
    line(io, "hyper probe: boot loopback validates HTTP/1 client");
    line(io, "hyper probe: background net probe waits for socket+gateway readiness");
    line(io, "hyper probe: target example.de:80 GET /");
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_table(io, &HYPER_MENU_HEADERS, &HYPER_MENU_ROWS);
}

fn normalize_url(input: &str) -> Result<String, &'static str> {
    let url = input.trim();
    if url.is_empty() {
        return Err("empty url");
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return Ok(String::from(url));
    }
    Ok(alloc::format!("https://{url}"))
}

fn basename_from_url(url: &str) -> &str {
    let without_query = url.split('?').next().unwrap_or(url);
    let without_fragment = without_query.split('#').next().unwrap_or(without_query);
    let path = without_fragment
        .rsplit('/')
        .next()
        .unwrap_or(without_fragment);
    if path.is_empty() {
        "download.bin"
    } else {
        path
    }
}

fn normalize_path(path: &str) -> Result<String, &'static str> {
    crate::r::path::FsPath::parse(path, false)
        .map(|path| path.to_relative_string())
        .map_err(|_| "bad path")
}

struct DownloadTarget {
    scheme: &'static str,
    host: String,
    port: u16,
    path_and_query: String,
}

fn parse_download_url(url: &str) -> Result<DownloadTarget, &'static str> {
    let trimmed = url.trim();
    let (scheme, rest, default_port) = if let Some(rest) = trimmed.strip_prefix("https://") {
        ("https", rest, 443)
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        ("http", rest, 80)
    } else {
        return Err("unsupported scheme");
    };

    let authority_end = rest
        .find(|c| c == '/' || c == '?' || c == '#')
        .unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return Err("bad url");
    }

    let (host, port) = if let Some((host, port_text)) = authority.rsplit_once(':') {
        let port = port_text.parse::<u16>().map_err(|_| "bad port")?;
        (host, port)
    } else {
        (authority, default_port)
    };
    if host.is_empty() {
        return Err("bad host");
    }

    let mut path_and_query = if authority_end >= rest.len() {
        String::from("/")
    } else {
        let suffix = &rest[authority_end..];
        if suffix.starts_with('?') {
            format!("/{}", suffix)
        } else {
            String::from(suffix)
        }
    };
    if let Some(fragment) = path_and_query.find('#') {
        path_and_query.truncate(fragment);
    }
    if path_and_query.is_empty() {
        path_and_query.push('/');
    }

    Ok(DownloadTarget {
        scheme,
        host: String::from(host),
        port,
        path_and_query,
    })
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn find_http_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|idx| idx + 4)
}

fn parse_http_status(bytes: &[u8]) -> Option<u16> {
    let line_end = bytes.windows(2).position(|w| w == b"\r\n")?;
    let line = core::str::from_utf8(&bytes[..line_end]).ok()?;
    let mut parts = line.split_whitespace();
    let _http = parts.next()?;
    parts.next()?.parse::<u16>().ok()
}

fn header_value<'a>(headers: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    for line in headers.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(colon) = line.iter().position(|b| *b == b':') else {
            continue;
        };
        let key = &line[..colon];
        if key.len() == name.len()
            && key
                .iter()
                .zip(name.iter())
                .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
        {
            let mut value = &line[colon + 1..];
            while value.first() == Some(&b' ') || value.first() == Some(&b'\t') {
                value = &value[1..];
            }
            return Some(value);
        }
    }
    None
}

fn header_value_has_token(value: &[u8], token: &[u8]) -> bool {
    value.split(|b| *b == b',' || *b == b';').any(|part| {
        let part = trim_ascii(part);
        part.len() == token.len()
            && part
                .iter()
                .zip(token.iter())
                .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
    })
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while matches!(bytes.first(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        bytes = &bytes[1..];
    }
    while matches!(bytes.last(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn decode_chunked(body: &[u8], max_bytes: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    loop {
        let line_rel = body[offset..].windows(2).position(|w| w == b"\r\n")?;
        let line = &body[offset..offset + line_rel];
        let size_text = core::str::from_utf8(line.split(|b| *b == b';').next()?).ok()?;
        let size = usize::from_str_radix(size_text.trim(), 16).ok()?;
        offset = offset.checked_add(line_rel + 2)?;
        if size == 0 {
            return Some(out);
        }
        if offset.checked_add(size + 2)? > body.len() {
            return None;
        }
        if out.len().checked_add(size)? > max_bytes {
            return None;
        }
        out.extend_from_slice(&body[offset..offset + size]);
        offset += size + 2;
    }
}

fn http_body_from_response(response: &[u8]) -> Result<Vec<u8>, String> {
    let hdr_end = find_http_header_end(response).ok_or_else(|| String::from("bad response"))?;
    let status = parse_http_status(response).ok_or_else(|| String::from("bad status"))?;
    if !(200..300).contains(&status) {
        return Err(format!("http status {}", status));
    }

    let headers = &response[..hdr_end];
    let body = &response[hdr_end..];
    if let Some(te) = header_value(headers, b"transfer-encoding")
        && header_value_has_token(te, b"chunked")
    {
        return decode_chunked(body, HYPER_DOWNLOAD_MAX_BYTES)
            .ok_or_else(|| String::from("bad chunked body"));
    }

    if let Some(len_text) = header_value(headers, b"content-length")
        && let Ok(len) = core::str::from_utf8(trim_ascii(len_text))
            .unwrap_or("")
            .parse::<usize>()
    {
        return Ok(body.get(..len).unwrap_or(body).to_vec());
    }

    Ok(body.to_vec())
}

fn write_file(path: &str, bytes: &[u8]) -> Result<(), String> {
    let handle = crate::r::io::kfs::write_file_begin(path, bytes.len() as u64)
        .map_err(|err| format!("write begin failed: {:?}", err))?;
    if let Err(err) = crate::r::io::kfs::write_file_chunk(handle, bytes) {
        let _ = crate::r::io::kfs::write_file_abort(handle);
        return Err(format!("write chunk failed: {:?}", err));
    }
    crate::r::io::kfs::write_file_finish(handle)
        .map_err(|err| format!("write finish failed: {:?}", err))
}

async fn fetch_http_bytes(url: String) -> Result<Vec<u8>, String> {
    crate::surfer::html_shack::fetch_bytes_via_pool(
        url,
        HYPER_DOWNLOAD_TIMEOUT_MS,
        HYPER_DOWNLOAD_MAX_BYTES,
    )
    .await
    .map(|fetch| fetch.bytes)
}

async fn fetch_https_bytes(target: &DownloadTarget) -> Result<Vec<u8>, String> {
    crate::r::readiness::wait_for(
        crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::TLS_SOCKET_SERVICE_READY,
    )
    .await;

    let ip = crate::r::net::dns::resolve_ipv4_primary(
        target.host.as_str(),
        crate::r::net::dns::DnsConfig::default(),
    )
    .await
    .map_err(|err| format!("dns {:?}", err))?;

    let seq = HYPER_DOWNLOAD_SEQ.fetch_add(1, Ordering::Relaxed);
    let owner = leak_str(format!("hyper-download-{}", seq));
    let cmds = Queue::new_leaked(leak_str(format!("{}-cmd", owner)), 128);
    let events = Queue::new_leaked(leak_str(format!("{}-evt", owner)), 1024);
    register_tls_app_queues(owner, cmds, events);

    cmds.push(TlsCommand::OpenTcpConnect {
        remote: vnet::EndpointV4 {
            addr: ip,
            port: target.port,
        },
        server_name: leak_str(target.host.clone()),
        cfg: TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]),
        roots: TlsRoots::mozilla(),
        timeouts: TlsTimeouts {
            connect_ms: 20_000,
            tls_ms: 30_000,
            idle_ms: HYPER_DOWNLOAD_TIMEOUT_MS as u32,
        },
    })
    .map_err(|_| String::from("tls queue full"))?;

    let deadline = Instant::now() + EmbassyDuration::from_millis(HYPER_DOWNLOAD_TIMEOUT_MS);
    let mut tls_handle = None;
    let mut sent_request = false;
    let mut response = Vec::new();

    loop {
        for ev in events.drain(64) {
            match ev {
                TlsEvent::Opened { handle } => tls_handle = Some(handle),
                TlsEvent::Connected { handle } => {
                    if tls_handle != Some(handle) || sent_request {
                        continue;
                    }
                    let req = format!(
                        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS hyper\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
                        target.path_and_query, target.host
                    );
                    cmds.push(TlsCommand::Send {
                        handle,
                        data: req.into_bytes(),
                    })
                    .map_err(|_| String::from("tls send queue full"))?;
                    sent_request = true;
                }
                TlsEvent::Data { handle, data } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if response.len().saturating_add(data.len()) > HYPER_DOWNLOAD_MAX_BYTES {
                        let _ = cmds.push(TlsCommand::Close { handle });
                        return Err(String::from("too large"));
                    }
                    response.extend_from_slice(data.as_slice());
                }
                TlsEvent::Closed { handle } => {
                    if tls_handle == Some(handle) {
                        return http_body_from_response(response.as_slice());
                    }
                }
                TlsEvent::Error { msg } => return Err(String::from(msg)),
                TlsEvent::TlsError { err } => return Err(format!("tls {:?}", err)),
            }
        }

        if Instant::now() >= deadline {
            if let Some(handle) = tls_handle {
                let _ = cmds.push(TlsCommand::Close { handle });
            }
            return Err(String::from("timeout"));
        }
        Timer::after(EmbassyDuration::from_millis(10)).await;
    }
}

async fn fetch_download_bytes(url: String) -> Result<Vec<u8>, String> {
    crate::r::net::https::get_bytes_shared(
        url.as_str(),
        HYPER_DOWNLOAD_TIMEOUT_MS as u32,
        HYPER_DOWNLOAD_MAX_BYTES,
    )
    .await
}

fn submit_download(spawner: &Spawner, io: &'static dyn ShellBackend2, url: String, path: String) {
    let target = matrix_target_for_backend(io);
    print_matrix_target_line(
        &target,
        alloc::format!("hyper: download {} -> {}", url, path).as_str(),
    );

    set_matrix_target_active(&target, true);
    match hyper_download_task(target.clone(), url, path) {
        Ok(token) => {
            spawner.spawn(token);
        }
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_shell_line(io, "hyper: spawn failed");
        }
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn hyper_download_task(target: MatrixTarget, url: String, path: String) {
    let log = |line: &str| print_matrix_target_line(&target, line);

    match fetch_download_bytes(url.clone()).await {
        Ok(bytes) => match write_file(path.as_str(), bytes.as_slice()) {
            Ok(()) => log(format!("hyper: saved {} bytes -> {}", bytes.len(), path).as_str()),
            Err(err) => log(format!("hyper: write failed: {}", err).as_str()),
        },
        Err(err) => log(format!("hyper: download failed: {}", err).as_str()),
    }
    set_matrix_target_active(&target, false);
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    match args.next() {
        None | Some("status") => print_status(io),
        Some("probe") => print_probe(io),
        Some("help") | Some("-h") | Some("--help") => print_usage(io),
        Some(url_arg) => {
            let url = match normalize_url(url_arg) {
                Ok(url) => url,
                Err(err) => {
                    line(io, alloc::format!("hyper: {}", err).as_str());
                    return ParseOutcome::Handled;
                }
            };
            let path = match args.next() {
                Some(path_arg) => match normalize_path(path_arg) {
                    Ok(path) => path,
                    Err(err) => {
                        line(io, alloc::format!("hyper: {}", err).as_str());
                        return ParseOutcome::Handled;
                    }
                },
                None => String::from(basename_from_url(url.as_str())),
            };
            if args.next().is_some() {
                line(io, "hyper: usage `hyper <url> [path]`");
                return ParseOutcome::Handled;
            }
            submit_download(spawner, io, url, path);
        }
    }

    ParseOutcome::Handled
}
