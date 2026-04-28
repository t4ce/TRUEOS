extern crate alloc;
extern crate std;

use alloc::vec::Vec;
use core::convert::Infallible;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::io;

use hyper::body::{Body, Bytes, Frame, SizeHint};
use hyper::rt::{Read, ReadBufCursor, Write};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use v::vnet;

use crate::net::tls::{KernelTlsRng, TlsClient, TlsClientConfig, TlsRoots, TlsTime};

pub const PCI_IDS_URL: &str = "https://raw.githubusercontent.com/pciutils/pciids/master/pci.ids";
pub const PCI_IDS_KEY: &str = "trueos/pci/pci.ids";
const PCI_IDS_HOST: &str = "raw.githubusercontent.com";
const PCI_IDS_PATH: &str = "/pciutils/pciids/master/pci.ids";
const PCI_IDS_PORT: u16 = 443;
const PCI_IDS_TIMEOUT_MS: u32 = 120_000;
const PCI_IDS_MAX_BYTES: usize = 4 * 1024 * 1024;

struct EmptyBody;

impl Body for EmptyBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Poll::Ready(None)
    }

    fn is_end_stream(&self) -> bool {
        true
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(0)
    }
}

struct HyperTokioIo<T> {
    inner: T,
}

impl<T> HyperTokioIo<T> {
    fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T> Read for HyperTokioIo<T>
where
    T: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: ReadBufCursor<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let limit = buf.remaining().min(4096);
        if limit == 0 {
            return Poll::Ready(Ok(()));
        }

        let mut scratch = [0u8; 4096];
        let mut tokio_buf = tokio::io::ReadBuf::new(&mut scratch[..limit]);
        match Pin::new(&mut self.inner).poll_read(cx, &mut tokio_buf) {
            Poll::Ready(Ok(())) => {
                buf.put_slice(tokio_buf.filled());
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T> Write for HyperTokioIo<T>
where
    T: AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

struct KernelTime;

impl TlsTime for KernelTime {
    fn unix_time_seconds(&self) -> Option<u64> {
        crate::time::unix_time_seconds()
    }
}

static KERNEL_TIME: KernelTime = KernelTime;

fn block_error_label(e: crate::disc::block::Error) -> &'static str {
    match e {
        crate::disc::block::Error::NotSupported => "fs not supported",
        crate::disc::block::Error::NotReady => "fs not ready",
        crate::disc::block::Error::Io => "fs io",
        crate::disc::block::Error::OutOfBounds => "fs out of bounds",
        crate::disc::block::Error::InvalidParam => "fs invalid param",
        crate::disc::block::Error::DmaUnavailable => "fs dma unavailable",
        crate::disc::block::Error::MmioMapFailed => "fs mmio map failed",
        crate::disc::block::Error::Timeout => "fs timeout",
        crate::disc::block::Error::Corrupted => "fs corrupted",
    }
}

fn dns_error_label(e: crate::r::net::dns::DnsError) -> &'static str {
    match e {
        crate::r::net::dns::DnsError::NoNic => "dns no nic",
        crate::r::net::dns::DnsError::BadName => "dns bad name",
        crate::r::net::dns::DnsError::Timeout => "dns timeout",
        crate::r::net::dns::DnsError::NoAnswer => "dns no answer",
    }
}

async fn vnet_send_tcp_all(
    net: &crate::r::net::VNet,
    handle: vnet::NetHandle,
    data: &[u8],
) -> Result<(), &'static str> {
    for chunk in data.chunks(vnet::MAX_MSG) {
        let mut sent = false;
        for _ in 0..64 {
            if net
                .submit(vnet::Command::SendTcp {
                    handle,
                    data: vnet::ByteBuf::from_slice_trunc(chunk),
                })
                .is_ok()
            {
                sent = true;
                break;
            }
            tokio::time::sleep(core::time::Duration::from_millis(1)).await;
        }
        if !sent {
            return Err("tcp send");
        }
    }
    Ok(())
}

async fn flush_tls_to_tcp(
    net: &crate::r::net::VNet,
    handle: vnet::NetHandle,
    tls: &mut TlsClient,
) -> Result<(), &'static str> {
    let out = tls.take_ciphertext_to_send().map_err(|_| "tls drain")?;
    if !out.is_empty() {
        vnet_send_tcp_all(net, handle, out.as_slice()).await?;
    }
    Ok(())
}

async fn pciids_tls_bridge(
    net: crate::r::net::VNet,
    handle: vnet::NetHandle,
    mut tls: TlsClient,
    mut io: DuplexStream,
) -> Result<(), &'static str> {
    let mut outbound = [0u8; 4096];
    loop {
        tokio::select! {
            read = io.read(&mut outbound) => {
                let n = read.map_err(|_| "bridge read")?;
                if n == 0 {
                    break;
                }
                tls.write_plaintext(&outbound[..n]).map_err(|_| "tls write")?;
                flush_tls_to_tcp(&net, handle, &mut tls).await?;
            }
            _ = tokio::time::sleep(core::time::Duration::from_millis(1)) => {
                for _ in 0..128 {
                    let Some(ev) = net.pop_event() else {
                        break;
                    };
                    match ev {
                        vnet::Event::TcpData { handle: h, data } if h == handle => {
                            let plaintext = tls
                                .ingest_encrypted(data.as_slice())
                                .map_err(|_| "tls ingest")?;
                            flush_tls_to_tcp(&net, handle, &mut tls).await?;
                            if !plaintext.is_empty() {
                                io.write_all(plaintext.as_slice())
                                    .await
                                    .map_err(|_| "bridge write")?;
                            }
                        }
                        vnet::Event::Closed { handle: h } if h == handle => {
                            let _ = io.shutdown().await;
                            return Ok(());
                        }
                        vnet::Event::Error { .. } => return Err("vnet error"),
                        _ => {}
                    }
                }
            }
        }
    }

    let _ = net.submit(vnet::Command::Close { handle });
    let _ = io.shutdown().await;
    Ok(())
}

async fn connect_pciids_tls_stream(
    dev_idx: usize,
    ip: [u8; 4],
) -> Result<DuplexStream, &'static str> {
    let Some(net) = crate::r::net::VNet::open(dev_idx) else {
        return Err("vnet open");
    };

    let remote = vnet::EndpointV4::new(ip, PCI_IDS_PORT);
    net.submit(vnet::Command::OpenTcpConnect { remote })
        .map_err(|_| "connect submit")?;

    let roots = TlsRoots::mozilla();
    let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
    let mut rng = KernelTlsRng::new();
    let mut tls = TlsClient::new(&cfg, &roots, PCI_IDS_HOST, &mut rng, &KERNEL_TIME)
        .map_err(|_| "tls init")?;

    let deadline =
        tokio::time::Instant::now() + core::time::Duration::from_millis(PCI_IDS_TIMEOUT_MS as u64);
    let mut tcp_handle = None;
    let handle = 'connect: loop {
        while let Some(ev) = net.pop_event() {
            match ev {
                vnet::Event::Opened { handle, kind } if kind == vnet::SocketKind::Tcp => {
                    tcp_handle = Some(handle);
                }
                vnet::Event::TcpEstablished { handle } => {
                    if tcp_handle.is_none() {
                        tcp_handle = Some(handle);
                    }
                    if tcp_handle == Some(handle) {
                        flush_tls_to_tcp(&net, handle, &mut tls).await?;
                    }
                }
                vnet::Event::TcpData { handle, data } if tcp_handle == Some(handle) => {
                    let _ = tls
                        .ingest_encrypted(data.as_slice())
                        .map_err(|_| "tls handshake")?;
                    flush_tls_to_tcp(&net, handle, &mut tls).await?;
                    if tls.is_connected() {
                        break 'connect handle;
                    }
                }
                vnet::Event::Closed { handle } if tcp_handle == Some(handle) => {
                    return Err("closed during handshake");
                }
                vnet::Event::Error { .. } => return Err("vnet error"),
                _ => {}
            }
        }

        if tokio::time::Instant::now() >= deadline {
            return Err("connect timeout");
        }
        tokio::time::sleep(core::time::Duration::from_millis(1)).await;
    };

    let (client_io, bridge_io) = tokio::io::duplex(32 * 1024);
    tokio::spawn(async move {
        if let Err(stage) = pciids_tls_bridge(net, handle, tls, bridge_io).await {
            crate::log!("pciids: hyper tls bridge ended at {}\n", stage);
        }
    });
    Ok(client_io)
}

async fn hyper_fetch_pciids_on_device(
    dev_idx: usize,
    ip: [u8; 4],
) -> Result<Vec<u8>, &'static str> {
    let stream = connect_pciids_tls_stream(dev_idx, ip).await?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake::<_, EmptyBody>(HyperTokioIo::new(stream))
            .await
            .map_err(|_| "hyper handshake")?;
    let connection = tokio::spawn(async move { connection.await });

    sender.ready().await.map_err(|_| "hyper ready")?;
    let request = hyper::Request::builder()
        .method(hyper::Method::GET)
        .uri(PCI_IDS_PATH)
        .header(hyper::header::HOST, PCI_IDS_HOST)
        .header(hyper::header::USER_AGENT, "TRUEOS pciids hyper")
        .header(hyper::header::ACCEPT, "text/plain, */*;q=0.8")
        .body(EmptyBody)
        .map_err(|_| "request build")?;
    let mut response = sender
        .send_request(request)
        .await
        .map_err(|_| "send request")?;

    if response.status() != hyper::StatusCode::OK {
        return Err("bad status");
    }

    let mut body = Vec::new();
    while let Some(frame) = core::future::poll_fn(|cx| Pin::new(response.body_mut()).poll_frame(cx))
        .await
        .transpose()
        .map_err(|_| "body frame")?
    {
        if let Some(data) = frame.data_ref() {
            if body.len().saturating_add(data.len()) > PCI_IDS_MAX_BYTES {
                return Err("body too large");
            }
            body.extend_from_slice(data);
        }
    }

    drop(response);
    drop(sender);
    let _ = tokio::time::timeout(core::time::Duration::from_millis(250), connection).await;
    Ok(body)
}

async fn fetch_pciids_hyper_body() -> Result<Vec<u8>, &'static str> {
    crate::r::readiness::wait_for(crate::r::readiness::NET_V4_CONFIGURED).await;

    let dev_idx = crate::net::primary_device_index();
    let dns_cfg = crate::r::net::dns::DnsConfig::for_device_v4_only(dev_idx);
    let ip = match crate::r::net::dns::resolve_ipv4_for_device(dev_idx, PCI_IDS_HOST, dns_cfg).await
    {
        Ok(ip) => ip,
        Err(e) => {
            let reason = dns_error_label(e);
            crate::log!(
                "pciids: dns failed host={} dev={} reason={}\n",
                PCI_IDS_HOST,
                dev_idx,
                reason
            );
            return Err(reason);
        }
    };

    let mut runtime_builder = tokio::runtime::Builder::new_current_thread();
    runtime_builder.enable_io();
    runtime_builder.enable_time();
    let runtime = runtime_builder.build().map_err(|_| "tokio runtime")?;
    runtime.block_on(hyper_fetch_pciids_on_device(dev_idx, ip))
}

async fn download_pciids_hyper_to_file(
    disk: crate::disc::block::DeviceHandle,
) -> Result<usize, &'static str> {
    let body = fetch_pciids_hyper_body().await?;
    let bytes = body.len();
    let ok = crate::r::fs::trueosfs::file_in_async(disk, PCI_IDS_KEY, body.as_slice())
        .await
        .map_err(block_error_label)?;
    if !ok {
        return Err("fs no space");
    }
    Ok(bytes)
}

pub async fn download_once_async() -> Result<usize, &'static str> {
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        crate::log!(
            "pciids: download skipped; no root disk url={} key={}\n",
            PCI_IDS_URL,
            PCI_IDS_KEY
        );
        return Err("no root disk");
    };

    if let Err(reason) = download_pciids_hyper_to_file(disk).await {
        crate::log!(
            "pciids: hyper download failed reason={} url={} key={}\n",
            reason,
            PCI_IDS_URL,
            PCI_IDS_KEY
        );
        return Err(reason);
    }

    match crate::r::fs::trueosfs::file_out_async(disk, PCI_IDS_KEY).await {
        Ok(Some(raw)) => {
            crate::log!(
                "pciids: downloaded ok url={} key={} bytes={}\n",
                PCI_IDS_URL,
                PCI_IDS_KEY,
                raw.len(),
            );
            Ok(raw.len())
        }
        Ok(None) => {
            crate::log!(
                "pciids: download finished but file missing url={} key={}\n",
                PCI_IDS_URL,
                PCI_IDS_KEY
            );
            Err("download missing")
        }
        Err(e) => {
            crate::log!("pciids: downloaded but size check failed={:?} key={}\n", e, PCI_IDS_KEY);
            Err("download verify failed")
        }
    }
}

pub fn download_once_detached() {
    crate::wait::spawn_local_detached(async {
        match download_once_async().await {
            Ok(bytes) => crate::log!(
                "pciids: detached download finished key={} bytes={}\n",
                PCI_IDS_KEY,
                bytes
            ),
            Err(reason) => crate::log!(
                "pciids: detached download failed reason={} url={}\n",
                reason,
                PCI_IDS_URL
            ),
        }
    });
}

pub fn load_raw_from_root_blocking()
-> Result<Option<alloc::vec::Vec<u8>>, crate::disc::block::Error> {
    let mut last_err: Option<crate::disc::block::Error> = None;

    // Try every mounted TRUEOSFS root (newest first) so a valid pci.ids on an
    // older root still works if the primary root switched later in boot.
    for root in crate::r::fs::trueosfs::list_roots() {
        let Some(disk) = crate::disc::block::device_handle(root.disk_id) else {
            continue;
        };
        match crate::wait::spawn_and_wait_local(async move {
            crate::r::fs::trueosfs::file_out_async(disk, PCI_IDS_KEY).await
        }) {
            Ok(Some(raw)) => return Ok(Some(raw)),
            Ok(None) => {}
            Err(e) => last_err = Some(e),
        }
    }

    if let Some(e) = last_err {
        return Err(e);
    }
    Ok(None)
}

pub fn load_sanitized_from_root_blocking()
-> Result<Option<alloc::vec::Vec<u8>>, crate::disc::block::Error> {
    let Some(raw) = load_raw_from_root_blocking()? else {
        return Ok(None);
    };
    Ok(Some(sanitize_pci_ids(&raw)))
}

fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
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

pub fn sanitize_pci_ids(raw: &[u8]) -> alloc::vec::Vec<u8> {
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
            let Some(id) = hex4_lower(&rest[..4]) else {
                continue;
            };
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
            let Some(id) = hex4_lower(&rest[..4]) else {
                continue;
            };
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
            let Some(subvendor) = hex4_lower(&rest[..4]) else {
                continue;
            };
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
            let Some(subdevice) = hex4_lower(&rest[j..j + 4]) else {
                continue;
            };
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
            let Some(v) = hex4_to_u16(&rest[..4]) else {
                continue;
            };
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
            let Some(d) = hex4_to_u16(&rest[..4]) else {
                continue;
            };
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
