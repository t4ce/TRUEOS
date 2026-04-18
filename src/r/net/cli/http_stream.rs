extern crate alloc;

use alloc::vec::Vec;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use v::vnet as api;

use super::http::{
    HttpBodyKind, HttpFetchError, find_http_header_end, header_parse_content_length,
    is_redirect_status, parse_http_head, parse_http_status, parse_http_url,
    redirect_url_from_location,
};
use crate::r::net::dns::{self, DnsConfig};
use crate::r::net::{NetProfile, VNet};

const HTTP_FILE_WRITE_FALLBACK_CHUNK_BYTES: usize = 64 * 1024;
const HTTP_FILE_WRITE_PROGRESS_BYTES: u64 = 512 * 1024;
const HTTP_FILE_WRITE_YIELD_EVERY_BYTES: u64 = 256 * 1024;

fn http_file_write_chunk_bytes(info: &crate::disc::block::DeviceInfo) -> usize {
    let block_size = usize::max(info.block_size as usize, 1);
    let raw = if info.max_transfer_bytes > 0 {
        info.max_transfer_bytes as usize
    } else {
        HTTP_FILE_WRITE_FALLBACK_CHUNK_BYTES
    };
    let aligned = raw - (raw % block_size);
    usize::max(aligned, block_size)
}

fn http_file_write_bps(written: u64, started: Instant) -> u64 {
    let elapsed_ms = started.elapsed().as_millis();
    if elapsed_ms == 0 {
        0
    } else {
        ((written as u128).saturating_mul(1000) / elapsed_ms as u128) as u64
    }
}

async fn write_http_body_to_file(
    disk: crate::disc::block::DeviceHandle,
    path: &str,
    body: &[u8],
) -> Result<(), crate::disc::block::Error> {
    let info = disk.info();
    let chunk_bytes = http_file_write_chunk_bytes(&info);
    crate::log!(
        "http-stream: file write start path={} bytes={} disk_id={} kind={:?} block={} max_transfer={} chunk={} label={}\n",
        path,
        body.len(),
        info.id.raw(),
        info.kind,
        info.block_size,
        info.max_transfer_bytes,
        chunk_bytes,
        info.label.as_deref().unwrap_or("-"),
    );

    let Some(handle) =
        crate::r::fs::trueosfs::file_write_begin_async(disk, path, body.len() as u64).await?
    else {
        return Err(crate::disc::block::Error::NotReady);
    };

    let started = Instant::now();
    let mut written = 0u64;
    let mut next_progress = HTTP_FILE_WRITE_PROGRESS_BYTES;
    let mut next_yield = HTTP_FILE_WRITE_YIELD_EVERY_BYTES;

    for chunk in body.chunks(chunk_bytes) {
        if let Err(err) = crate::r::fs::trueosfs::file_write_chunk_async(handle, chunk).await {
            let _ = crate::r::fs::trueosfs::file_write_abort_async(handle).await;
            crate::log!(
                "http-stream: file write chunk failed path={} offset={} chunk={} err={:?}\n",
                path,
                written,
                chunk.len(),
                err,
            );
            return Err(err);
        }

        written = written.saturating_add(chunk.len() as u64);
        if written >= next_progress || written == body.len() as u64 {
            crate::log!(
                "http-stream: file write progress path={} written={} total={} bps={} elapsed_ms={}\n",
                path,
                written,
                body.len(),
                http_file_write_bps(written, started),
                started.elapsed().as_millis(),
            );
            next_progress = next_progress.saturating_add(HTTP_FILE_WRITE_PROGRESS_BYTES);
        }

        if written >= next_yield && written != body.len() as u64 {
            Timer::after(EmbassyDuration::from_millis(1)).await;
            next_yield = next_yield.saturating_add(HTTP_FILE_WRITE_YIELD_EVERY_BYTES);
        }
    }

    crate::log!(
        "http-stream: file write flush path={} written={} elapsed_ms={}\n",
        path,
        written,
        started.elapsed().as_millis(),
    );

    match crate::r::fs::trueosfs::file_write_finish_async(handle).await {
        Ok(()) => {
            crate::log!(
                "http-stream: file write committed path={} bytes={} bps={} elapsed_ms={}\n",
                path,
                written,
                http_file_write_bps(written, started),
                started.elapsed().as_millis(),
            );
            Ok(())
        }
        Err(err) => {
            crate::log!(
                "http-stream: file write finish failed path={} written={} err={:?}\n",
                path,
                written,
                err,
            );
            Err(err)
        }
    }
}

async fn request_http_to_file(
    url: &str,
    timeout_ms: u32,
    max_rx: usize,
    disk: crate::disc::block::DeviceHandle,
    path: &str,
) -> Result<(), HttpFetchError> {
    let parsed = parse_http_url(url).map_err(|_| HttpFetchError::BadUrl)?;

    let _ = crate::r::readiness::wait_for_timeout(
        crate::r::readiness::NET_CONFIGURED,
        EmbassyDuration::from_secs(3),
    )
    .await;

    let profile = NetProfile::default();
    let ip = if let Some(ip) = super::http::parse_ipv4_literal(parsed.host.as_str()) {
        ip
    } else {
        let Ok(ip) = dns::resolve_ipv4_with_profile(
            parsed.host.as_str(),
            profile,
            DnsConfig::for_profile(profile),
        )
        .await
        else {
            return Err(HttpFetchError::DnsFailed);
        };
        ip
    };

    let net = loop {
        if let Some(v) = VNet::open_with_profile(profile) {
            break v;
        }
        Timer::after(EmbassyDuration::from_millis(50)).await;
    };

    let mut open_sent = false;
    for _ in 0..64 {
        if net
            .submit(api::Command::OpenTcpConnect {
                remote: api::EndpointV4 {
                    addr: ip,
                    port: parsed.port,
                },
            })
            .is_ok()
        {
            open_sent = true;
            break;
        }
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
    if !open_sent {
        crate::log!("http-stream: open failed host={} port={}\n", parsed.host, parsed.port);
        return Err(HttpFetchError::TimedOut);
    }

    let mut tcp_handle: Option<api::NetHandle> = None;
    let mut sent_request = false;
    let mut rx: Vec<u8> = Vec::new();
    let mut truncated = false;
    let timeout_window = EmbassyDuration::from_millis(timeout_ms as u64);
    let mut last_progress = Instant::now();

    loop {
        for _ in 0..32 {
            let Some(ev) = net.pop_event() else { break };
            match ev {
                api::Event::Opened { handle, kind } => {
                    if matches!(kind, api::SocketKind::Tcp) {
                        tcp_handle = Some(handle);
                        last_progress = Instant::now();
                    }
                }
                api::Event::TcpEstablished { handle } => {
                    if tcp_handle.is_none() {
                        tcp_handle = Some(handle);
                    }
                    if tcp_handle != Some(handle) {
                        continue;
                    }
                    if !sent_request {
                        let mut req: Vec<u8> = Vec::new();
                        req.extend_from_slice(b"GET ");
                        req.extend_from_slice(parsed.path.as_str().as_bytes());
                        req.extend_from_slice(b" HTTP/1.1\r\nHost: ");
                        req.extend_from_slice(parsed.host.as_str().as_bytes());
                        req.extend_from_slice(
                            b"\r\nUser-Agent: TRUEOS\r\nAccept: */*\r\nConnection: close\r\n\r\n",
                        );

                        if let Some(h) = tcp_handle {
                            let mut send_ok = false;
                            for _ in 0..64 {
                                if net
                                    .submit(api::Command::SendTcp {
                                        handle: h,
                                        data: api::ByteBuf::from_slice_trunc(req.as_slice()),
                                    })
                                    .is_ok()
                                {
                                    send_ok = true;
                                    break;
                                }
                                Timer::after(EmbassyDuration::from_millis(1)).await;
                            }
                            if send_ok {
                                sent_request = true;
                                last_progress = Instant::now();
                            }
                        }
                    }
                }
                api::Event::TcpData { handle, data } => {
                    if tcp_handle != Some(handle) {
                        continue;
                    }
                    let data = data.as_slice();
                    if !data.is_empty() {
                        last_progress = Instant::now();
                    }
                    if rx.len() < max_rx {
                        let room = max_rx - rx.len();
                        let take = data.len().min(room);
                        rx.extend_from_slice(&data[..take]);
                        if take < data.len() {
                            truncated = true;
                        }
                    } else {
                        truncated = true;
                    }

                    if let Some(hdr_end) = find_http_header_end(&rx) {
                        let headers = &rx[..hdr_end];
                        let status = parse_http_status(headers).unwrap_or(0);
                        if is_redirect_status(status)
                            && let Some(next) = redirect_url_from_location(&parsed, headers)
                        {
                            if let Some(h) = tcp_handle {
                                let _ = net.submit(api::Command::Close { handle: h });
                            }
                            return Err(HttpFetchError::Redirect(next));
                        }
                        if status >= 400 {
                            if let Some(h) = tcp_handle {
                                let _ = net.submit(api::Command::Close { handle: h });
                            }
                            return Err(HttpFetchError::HttpStatus(status));
                        }

                        if let Some(head) = parse_http_head(headers) {
                            match head.body {
                                HttpBodyKind::ContentLength(len) => {
                                    let body_len = rx.len().saturating_sub(hdr_end);
                                    if body_len >= len {
                                        if let Some(h) = tcp_handle {
                                            let _ = net.submit(api::Command::Close { handle: h });
                                        }
                                        if truncated {
                                            return Err(HttpFetchError::ResponseTooLarge);
                                        }
                                        write_http_body_to_file(
                                            disk,
                                            path,
                                            &rx[hdr_end..hdr_end + len],
                                        )
                                        .await
                                        .map_err(|_| HttpFetchError::TimedOut)?;
                                        return Ok(());
                                    }
                                }
                                HttpBodyKind::Chunked => {
                                    if let Some(body) =
                                        super::http::decode_http_chunked(&rx[hdr_end..])
                                    {
                                        if let Some(h) = tcp_handle {
                                            let _ = net.submit(api::Command::Close { handle: h });
                                        }
                                        if truncated {
                                            return Err(HttpFetchError::ResponseTooLarge);
                                        }
                                        write_http_body_to_file(disk, path, body.as_slice())
                                            .await
                                            .map_err(|_| HttpFetchError::TimedOut)?;
                                        return Ok(());
                                    }
                                }
                            }
                        }
                    }
                }
                api::Event::Closed { handle } => {
                    if tcp_handle == Some(handle) {
                        let hdr_end = find_http_header_end(&rx).unwrap_or(0);
                        let headers = &rx[..hdr_end.min(rx.len())];
                        let status = parse_http_status(headers).unwrap_or(0);
                        if status >= 400 {
                            return Err(HttpFetchError::HttpStatus(status));
                        }
                        if truncated {
                            return Err(HttpFetchError::ResponseTooLarge);
                        }
                        if hdr_end == 0 {
                            return Err(HttpFetchError::TimedOut);
                        }

                        let body = &rx[hdr_end..];
                        let final_body = if super::http::header_contains_token(
                            headers,
                            b"transfer-encoding",
                            b"chunked",
                        ) {
                            super::http::decode_http_chunked(body).unwrap_or_else(|| body.to_vec())
                        } else if let Some(len) = header_parse_content_length(headers) {
                            body.get(..len).unwrap_or(body).to_vec()
                        } else {
                            body.to_vec()
                        };
                        write_http_body_to_file(disk, path, final_body.as_slice())
                            .await
                            .map_err(|_| HttpFetchError::TimedOut)?;
                        return Ok(());
                    }
                }
                _ => {}
            }
        }

        if Instant::now().saturating_duration_since(last_progress) >= timeout_window {
            if let Some(h) = tcp_handle {
                let _ = net.submit(api::Command::Close { handle: h });
            }
            return Err(HttpFetchError::TimedOut);
        }

        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}

pub async fn fetch_http_to_file_async(
    url: &str,
    disk: crate::disc::block::DeviceHandle,
    path: &str,
    timeout_ms: u32,
    max_rx: usize,
) -> Result<(), HttpFetchError> {
    request_http_to_file(url, timeout_ms, max_rx, disk, path).await
}
