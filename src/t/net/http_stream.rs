extern crate alloc;

use alloc::vec::Vec;
use core::pin::Pin;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use embedded_io_async::Write;
use hyper::body::Body;

use super::http::{HttpFetchError, is_redirect_status, parse_http_url};
use crate::r::stream::{ObjectDesc, ObjectSink};
use crate::t::net::hyper_io::{HyperBytesBody, HyperTokioIo};

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

pub async fn fetch_http_to_file_hyper_async(
    url: &str,
    disk: crate::disc::block::DeviceHandle,
    path: &str,
    timeout_ms: u32,
    max_rx: usize,
) -> Result<(), HttpFetchError> {
    const MAX_REDIRECTS: usize = 3;
    let mut current_url = alloc::string::String::from(url);
    for hop in 0..=MAX_REDIRECTS {
        match request_http_to_file_hyper(current_url.as_str(), disk, path, timeout_ms, max_rx).await
        {
            Ok(()) => return Ok(()),
            Err(HttpFetchError::Redirect(next)) if hop < MAX_REDIRECTS => {
                current_url = next;
            }
            Err(err) => return Err(err),
        }
    }
    Err(HttpFetchError::TimedOut)
}

fn hyper_content_length(headers: &hyper::HeaderMap) -> Option<u64> {
    headers
        .get(hyper::header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
}

async fn write_hyper_buffer_to_file(
    disk: crate::disc::block::DeviceHandle,
    path: &str,
    body: &[u8],
) -> Result<(), HttpFetchError> {
    match crate::r::fs::trueosfs::file_in_async(disk, path, body).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(HttpFetchError::NoSpace),
        Err(crate::disc::block::Error::InvalidParam) => Err(HttpFetchError::BadUrl),
        Err(crate::disc::block::Error::Timeout) => Err(HttpFetchError::TimedOut),
        Err(_) => Err(HttpFetchError::NoSpace),
    }
}

async fn request_http_to_file_hyper(
    url: &str,
    disk: crate::disc::block::DeviceHandle,
    path: &str,
    timeout_ms: u32,
    max_rx: usize,
) -> Result<(), HttpFetchError> {
    let parsed = parse_http_url(url).map_err(|_| HttpFetchError::BadUrl)?;
    let stream = super::http::connect_hyper_tcp_stream(&parsed, timeout_ms).await?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake::<_, HyperBytesBody>(HyperTokioIo::new(stream))
            .await
            .map_err(|_| HttpFetchError::TimedOut)?;
    let connection = tokio::spawn(async move { connection.await });

    sender.ready().await.map_err(|_| HttpFetchError::TimedOut)?;
    let request = hyper::Request::builder()
        .method(hyper::Method::GET)
        .uri(parsed.path.as_str())
        .header(hyper::header::HOST, parsed.host.as_str())
        .header(hyper::header::USER_AGENT, "TRUEOS hyper")
        .header(hyper::header::ACCEPT, "*/*")
        .header(hyper::header::ACCEPT_ENCODING, "identity")
        .header(hyper::header::CONNECTION, "close")
        .body(HyperBytesBody::new(&[]))
        .map_err(|_| HttpFetchError::BadUrl)?;
    let response = tokio::time::timeout(
        core::time::Duration::from_millis(timeout_ms as u64),
        sender.send_request(request),
    )
    .await
    .map_err(|_| HttpFetchError::TimedOut)?
    .map_err(|_| HttpFetchError::TimedOut)?;

    let status = response.status().as_u16();
    if is_redirect_status(status) {
        if let Some(url) =
            super::http::hyper_redirect_url_from_location(&parsed, response.headers())
        {
            return Err(HttpFetchError::Redirect(url));
        }
    }
    if status >= 400 {
        return Err(HttpFetchError::HttpStatus(status));
    }

    let content_len = hyper_content_length(response.headers());
    if let Some(len) = content_len
        && len > max_rx as u64
    {
        return Err(HttpFetchError::ResponseTooLarge);
    }

    let mut body = response.into_body();
    if let Some(total_len) = content_len {
        let mut stream = begin_http_file_stream(disk, path, total_len)
            .await
            .map_err(http_stream_error_to_fetch_error)?;
        loop {
            let next = tokio::time::timeout(
                core::time::Duration::from_millis(timeout_ms as u64),
                core::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)),
            )
            .await
            .map_err(|_| HttpFetchError::TimedOut)?;
            let Some(frame) = next else {
                break;
            };
            let frame = frame.map_err(|_| HttpFetchError::TimedOut)?;
            if let Ok(data) = frame.into_data() {
                if stream.written.saturating_add(data.len() as u64) > total_len {
                    let mut active = Some(stream);
                    abort_http_file_stream(&mut active, path).await;
                    return Err(HttpFetchError::ResponseTooLarge);
                }
                if let Err(err) = write_http_file_stream_chunk(&mut stream, path, &data).await {
                    let mut active = Some(stream);
                    abort_http_file_stream(&mut active, path).await;
                    return Err(http_stream_error_to_fetch_error(err));
                }
            }
        }
        if stream.written != total_len {
            let mut active = Some(stream);
            abort_http_file_stream(&mut active, path).await;
            return Err(HttpFetchError::Truncated);
        }
        finish_http_file_stream(stream, path)
            .await
            .map_err(http_stream_error_to_fetch_error)?;
    } else {
        let mut out = Vec::new();
        loop {
            let next = tokio::time::timeout(
                core::time::Duration::from_millis(timeout_ms as u64),
                core::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)),
            )
            .await
            .map_err(|_| HttpFetchError::TimedOut)?;
            let Some(frame) = next else {
                break;
            };
            let frame = frame.map_err(|_| HttpFetchError::TimedOut)?;
            if let Ok(data) = frame.into_data() {
                if out.len().saturating_add(data.len()) > max_rx {
                    return Err(HttpFetchError::ResponseTooLarge);
                }
                out.extend_from_slice(&data);
            }
        }
        write_hyper_buffer_to_file(disk, path, out.as_slice()).await?;
    }

    drop(sender);
    let _ = tokio::time::timeout(core::time::Duration::from_millis(250), connection).await;
    Ok(())
}
