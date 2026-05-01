extern crate alloc;

use alloc::vec::Vec;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use embedded_io_async::Write;
use v::vnet as api;

use super::http::{
    HttpBodyKind, HttpFetchError, find_http_header_end, is_redirect_status, parse_http_head,
    parse_http_status, parse_http_url, redirect_url_from_location,
};
use crate::r::net::{NetProfile, VNet};
use crate::r::stream::{ObjectDesc, ObjectSink};
use crate::t::net::dns::{self, DnsConfig};

const HTTP_FILE_WRITE_FALLBACK_CHUNK_BYTES: usize = 64 * 1024;
const HTTP_FILE_WRITE_PROGRESS_BYTES: u64 = 512 * 1024 * 1024;
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

fn http_stream_error_to_fetch_error(err: crate::r::stream::HvStreamError) -> HttpFetchError {
    match err {
        crate::r::stream::HvStreamError::NoSpace => HttpFetchError::NoSpace,
        _ => HttpFetchError::TimedOut,
    }
}

struct HttpFileStream {
    sink: crate::r::stream::TrueosFsObjectSink,
    writer: crate::r::stream::TrueosFsObjectWriter,
    total_len: u64,
    written: u64,
    started: Instant,
    next_progress: u64,
    next_yield: u64,
}

async fn begin_http_file_stream(
    disk: crate::disc::block::DeviceHandle,
    path: &str,
    total_len: u64,
) -> Result<HttpFileStream, crate::r::stream::HvStreamError> {
    let info = disk.info();
    let chunk_bytes = http_file_write_chunk_bytes(&info);
    crate::log!(
        "http-stream: file write start path={} bytes={} disk_id={} kind={:?} block={} max_transfer={} chunk={} label={}\n",
        path,
        total_len,
        info.id.raw(),
        info.kind,
        info.block_size,
        info.max_transfer_bytes,
        chunk_bytes,
        info.label.as_deref().unwrap_or("-"),
    );

    let mut sink = crate::r::stream::TrueosFsObjectSink::new(disk);
    let writer = sink
        .begin(ObjectDesc {
            key: path,
            total_len_hint: Some(total_len),
        })
        .await?;

    Ok(HttpFileStream {
        sink,
        writer,
        total_len,
        written: 0,
        started: Instant::now(),
        next_progress: HTTP_FILE_WRITE_PROGRESS_BYTES,
        next_yield: HTTP_FILE_WRITE_YIELD_EVERY_BYTES,
    })
}

async fn write_http_file_stream_chunk(
    stream: &mut HttpFileStream,
    path: &str,
    bytes: &[u8],
) -> Result<(), crate::r::stream::HvStreamError> {
    if bytes.is_empty() {
        return Ok(());
    }

    stream.writer.write_all(bytes).await?;
    stream.written = stream.written.saturating_add(bytes.len() as u64);

    if stream.written >= stream.next_progress || stream.written == stream.total_len {
        crate::log!(
            "http-stream: file write progress path={} written={} total={} bps={} elapsed_ms={}\n",
            path,
            stream.written,
            stream.total_len,
            http_file_write_bps(stream.written, stream.started),
            stream.started.elapsed().as_millis(),
        );
        stream.next_progress = stream
            .next_progress
            .saturating_add(HTTP_FILE_WRITE_PROGRESS_BYTES);
    }

    if stream.written >= stream.next_yield && stream.written != stream.total_len {
        Timer::after(EmbassyDuration::from_millis(1)).await;
        stream.next_yield = stream
            .next_yield
            .saturating_add(HTTP_FILE_WRITE_YIELD_EVERY_BYTES);
    }

    Ok(())
}

async fn finish_http_file_stream(
    mut stream: HttpFileStream,
    path: &str,
) -> Result<(), crate::r::stream::HvStreamError> {
    crate::log!(
        "http-stream: file write flush start path={} written={} total={} elapsed_ms={}\n",
        path,
        stream.written,
        stream.total_len,
        stream.started.elapsed().as_millis(),
    );

    if let Err(err) = stream.writer.flush().await {
        crate::log!(
            "http-stream: file write flush failed path={} written={} total={} err={:?} elapsed_ms={}\n",
            path,
            stream.written,
            stream.total_len,
            err,
            stream.started.elapsed().as_millis(),
        );
        return Err(err);
    }

    crate::log!(
        "http-stream: file write flush ok path={} written={} total={} elapsed_ms={}\n",
        path,
        stream.written,
        stream.total_len,
        stream.started.elapsed().as_millis(),
    );
    crate::log!(
        "http-stream: file write commit start path={} written={} total={} elapsed_ms={}\n",
        path,
        stream.written,
        stream.total_len,
        stream.started.elapsed().as_millis(),
    );

    if let Err(err) = stream.sink.commit().await {
        crate::log!(
            "http-stream: file write commit failed path={} written={} total={} err={:?} elapsed_ms={}\n",
            path,
            stream.written,
            stream.total_len,
            err,
            stream.started.elapsed().as_millis(),
        );
        return Err(err);
    }

    crate::log!(
        "http-stream: file write committed path={} bytes={} bps={} elapsed_ms={}\n",
        path,
        stream.written,
        http_file_write_bps(stream.written, stream.started),
        stream.started.elapsed().as_millis(),
    );
    Ok(())
}

async fn abort_http_file_stream(stream: &mut Option<HttpFileStream>, path: &str) {
    let Some(mut stream) = stream.take() else {
        return;
    };

    let written = stream.written;
    match stream.sink.abort().await {
        Ok(()) => {
            crate::log!("http-stream: file write aborted path={} written={}\n", path, written,);
        }
        Err(err) => {
            crate::log!(
                "http-stream: file write abort failed path={} written={} err={:?}\n",
                path,
                written,
                err,
            );
        }
    }
}

fn extend_buffer_capped(buf: &mut Vec<u8>, data: &[u8], cap: usize) -> bool {
    if data.is_empty() {
        return false;
    }
    if buf.len() >= cap {
        return true;
    }

    let room = cap - buf.len();
    let take = data.len().min(room);
    buf.extend_from_slice(&data[..take]);
    take < data.len()
}

async fn write_http_body_to_file(
    disk: crate::disc::block::DeviceHandle,
    path: &str,
    body: &[u8],
) -> Result<(), crate::disc::block::Error> {
    let chunk_bytes = http_file_write_chunk_bytes(&disk.info());
    let mut stream = begin_http_file_stream(disk, path, body.len() as u64)
        .await
        .map_err(|_| crate::disc::block::Error::Io)?;
    for chunk in body.chunks(chunk_bytes) {
        if let Err(err) = write_http_file_stream_chunk(&mut stream, path, chunk).await {
            let written = stream.written;
            let mut active = Some(stream);
            abort_http_file_stream(&mut active, path).await;
            crate::log!(
                "http-stream: file write chunk failed path={} offset={} chunk={} err={:?}\n",
                path,
                written,
                chunk.len(),
                err,
            );
            return Err(crate::disc::block::Error::Io);
        }
    }

    finish_http_file_stream(stream, path)
        .await
        .map_err(|_| crate::disc::block::Error::Io)
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
        crate::r::readiness::NET_ANY_CONFIGURED,
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
    let mut header_buf: Vec<u8> = Vec::new();
    let mut buffered_headers: Option<Vec<u8>> = None;
    let mut buffered_body: Vec<u8> = Vec::new();
    let mut buffered_kind: Option<HttpBodyKind> = None;
    let mut buffered_truncated = false;
    let mut file_stream: Option<HttpFileStream> = None;
    let mut content_remaining: Option<usize> = None;
    let timeout_window = EmbassyDuration::from_millis(timeout_ms as u64);
    let mut last_progress = Instant::now();

    loop {
        for _ in 0..256 {
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
                            let mut send_ok = true;
                            for chunk in req.chunks(api::MAX_MSG) {
                                let mut chunk_sent = false;
                                for _ in 0..64 {
                                    if net
                                        .submit(api::Command::SendTcp {
                                            handle: h,
                                            data: api::ByteBuf::from_slice_trunc(chunk),
                                        })
                                        .is_ok()
                                    {
                                        chunk_sent = true;
                                        break;
                                    }
                                    Timer::after(EmbassyDuration::from_millis(1)).await;
                                }
                                if !chunk_sent {
                                    send_ok = false;
                                    break;
                                }
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

                    if let Some(stream) = file_stream.as_mut() {
                        let remaining = content_remaining.as_mut().expect("content-length state");
                        let take = data.len().min(*remaining);
                        if take > 0
                            && let Err(err) =
                                write_http_file_stream_chunk(stream, path, &data[..take]).await
                        {
                            crate::log!(
                                "http-stream: content-length stream failed path={} written={} chunk={} err={:?}\n",
                                path,
                                stream.written,
                                take,
                                err,
                            );
                            abort_http_file_stream(&mut file_stream, path).await;
                            if let Some(h) = tcp_handle {
                                let _ = net.submit(api::Command::Close { handle: h });
                            }
                            return Err(HttpFetchError::TimedOut);
                        }

                        *remaining = remaining.saturating_sub(take);
                        if *remaining == 0 {
                            crate::log!(
                                "http-stream: content-length received all bytes path={} written={} total={}\n",
                                path,
                                stream.written,
                                stream.total_len,
                            );
                            if let Some(h) = tcp_handle {
                                let _ = net.submit(api::Command::Close { handle: h });
                            }
                            let Some(stream) = file_stream.take() else {
                                return Err(HttpFetchError::TimedOut);
                            };
                            crate::log!(
                                "http-stream: content-length finish start path={} written={} total={}\n",
                                path,
                                stream.written,
                                stream.total_len,
                            );
                            if let Err(err) = finish_http_file_stream(stream, path).await {
                                crate::log!(
                                    "http-stream: content-length finish failed path={} err={:?}\n",
                                    path,
                                    err,
                                );
                                return Err(HttpFetchError::TimedOut);
                            }
                            crate::log!(
                                "http-stream: content-length finish ok path={}\n",
                                path,
                            );
                            return Ok(());
                        }
                        continue;
                    }

                    if let Some(kind) = buffered_kind {
                        if extend_buffer_capped(&mut buffered_body, data, max_rx) {
                            buffered_truncated = true;
                        }

                        if matches!(kind, HttpBodyKind::Chunked)
                            && let Some(body) = super::http::decode_http_chunked(&buffered_body)
                        {
                            if let Some(h) = tcp_handle {
                                let _ = net.submit(api::Command::Close { handle: h });
                            }
                            if buffered_truncated {
                                return Err(HttpFetchError::ResponseTooLarge);
                            }
                            write_http_body_to_file(disk, path, body.as_slice())
                                .await
                                .map_err(|_| HttpFetchError::TimedOut)?;
                            return Ok(());
                        }
                        continue;
                    }

                    header_buf.extend_from_slice(data);
                    if let Some(hdr_end) = find_http_header_end(&header_buf) {
                        let headers = header_buf[..hdr_end].to_vec();
                        let initial_body = header_buf[hdr_end..].to_vec();
                        header_buf.clear();

                        let status = parse_http_status(headers.as_slice()).unwrap_or(0);
                        if is_redirect_status(status)
                            && let Some(next) =
                                redirect_url_from_location(&parsed, headers.as_slice())
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

                        let Some(head) = parse_http_head(headers.as_slice()) else {
                            return Err(HttpFetchError::TimedOut);
                        };

                        match head.body {
                            HttpBodyKind::ContentLength(len) => {
                                let mut stream =
                                    match begin_http_file_stream(disk, path, len as u64).await {
                                        Ok(stream) => stream,
                                        Err(err) => {
                                            if let Some(h) = tcp_handle {
                                                let _ =
                                                    net.submit(api::Command::Close { handle: h });
                                            }
                                            crate::log!(
                                                "http-stream: content-length begin failed path={} len={} err={:?}\n",
                                                path,
                                                len,
                                                err,
                                            );
                                            return Err(http_stream_error_to_fetch_error(err));
                                        }
                                    };

                                let take = initial_body.len().min(len);
                                if take > 0
                                    && let Err(err) = write_http_file_stream_chunk(
                                        &mut stream,
                                        path,
                                        &initial_body[..take],
                                    )
                                    .await
                                {
                                    crate::log!(
                                        "http-stream: content-length initial write failed path={} chunk={} err={:?}\n",
                                        path,
                                        take,
                                        err,
                                    );
                                    let mut active = Some(stream);
                                    abort_http_file_stream(&mut active, path).await;
                                    if let Some(h) = tcp_handle {
                                        let _ = net.submit(api::Command::Close { handle: h });
                                    }
                                    return Err(HttpFetchError::TimedOut);
                                }

                                let remaining = len.saturating_sub(take);
                                if remaining == 0 {
                                    crate::log!(
                                        "http-stream: content-length received all bytes path={} written={} total={}\n",
                                        path,
                                        stream.written,
                                        stream.total_len,
                                    );
                                    if let Some(h) = tcp_handle {
                                        let _ = net.submit(api::Command::Close { handle: h });
                                    }
                                    crate::log!(
                                        "http-stream: content-length finish start path={} written={} total={}\n",
                                        path,
                                        stream.written,
                                        stream.total_len,
                                    );
                                    if let Err(err) = finish_http_file_stream(stream, path).await {
                                        crate::log!(
                                            "http-stream: content-length immediate finish failed path={} err={:?}\n",
                                            path,
                                            err,
                                        );
                                        return Err(HttpFetchError::TimedOut);
                                    }
                                    crate::log!(
                                        "http-stream: content-length finish ok path={}\n",
                                        path,
                                    );
                                    return Ok(());
                                }

                                file_stream = Some(stream);
                                content_remaining = Some(remaining);
                            }
                            HttpBodyKind::Chunked | HttpBodyKind::UntilClose => {
                                buffered_headers = Some(headers);
                                buffered_kind = Some(head.body);
                                if extend_buffer_capped(&mut buffered_body, &initial_body, max_rx) {
                                    buffered_truncated = true;
                                }

                                if matches!(head.body, HttpBodyKind::Chunked)
                                    && let Some(body) =
                                        super::http::decode_http_chunked(&buffered_body)
                                {
                                    if let Some(h) = tcp_handle {
                                        let _ = net.submit(api::Command::Close { handle: h });
                                    }
                                    if buffered_truncated {
                                        return Err(HttpFetchError::ResponseTooLarge);
                                    }
                                    write_http_body_to_file(disk, path, body.as_slice())
                                        .await
                                        .map_err(|_| HttpFetchError::TimedOut)?;
                                    return Ok(());
                                }
                            }
                        }
                    } else if header_buf.len() > max_rx {
                        if let Some(h) = tcp_handle {
                            let _ = net.submit(api::Command::Close { handle: h });
                        }
                        return Err(HttpFetchError::ResponseTooLarge);
                    }
                }
                api::Event::Closed { handle } => {
                    if tcp_handle == Some(handle) {
                        if file_stream.is_some() {
                            if let Some(stream) = file_stream.as_ref() {
                                let remaining = content_remaining.unwrap_or(0);
                                crate::log!(
                                    "http-stream: content-length closed before complete path={} written={} total={} remaining={}\n",
                                    path,
                                    stream.written,
                                    stream.total_len,
                                    remaining,
                                );
                            }
                            abort_http_file_stream(&mut file_stream, path).await;
                            return Err(HttpFetchError::Truncated);
                        }

                        let Some(kind) = buffered_kind else {
                            return Err(HttpFetchError::TimedOut);
                        };
                        if buffered_truncated {
                            return Err(HttpFetchError::ResponseTooLarge);
                        }

                        let headers = buffered_headers.as_deref().unwrap_or(&[]);
                        let status = parse_http_status(headers).unwrap_or(0);
                        if status >= 400 {
                            return Err(HttpFetchError::HttpStatus(status));
                        }

                        let final_body = match kind {
                            HttpBodyKind::Chunked => {
                                super::http::decode_http_chunked(buffered_body.as_slice())
                                    .unwrap_or_else(|| buffered_body.clone())
                            }
                            HttpBodyKind::UntilClose => buffered_body.clone(),
                            HttpBodyKind::ContentLength(len) => {
                                buffered_body.get(..len).unwrap_or(&buffered_body).to_vec()
                            }
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
            abort_http_file_stream(&mut file_stream, path).await;
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
