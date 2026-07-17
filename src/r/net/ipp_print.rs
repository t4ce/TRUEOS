//! Minimal asynchronous IPP/2.0 Print-Job client for the BSP spooler.

extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use embassy_time::{Duration, Instant, Timer};

use super::VNet;

const CONNECT_TIMEOUT_MS: u64 = 15_000;
const RESPONSE_TIMEOUT_MS: u64 = 60_000;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IppError {
    InvalidUri,
    NoNetwork,
    Connect,
    ConnectTimeout,
    ResponseTimeout,
    Transport,
    HttpStatus,
    InvalidResponse,
    Rejected,
}

impl IppError {
    pub(crate) const fn submission_retryable(self) -> bool {
        matches!(self, Self::NoNetwork | Self::Connect | Self::ConnectTimeout)
    }

    pub(crate) const fn status_retryable(self) -> bool {
        matches!(
            self,
            Self::NoNetwork
                | Self::Connect
                | Self::ConnectTimeout
                | Self::ResponseTimeout
                | Self::Transport
        )
    }
}

pub(crate) struct RemoteJob {
    pub id: u32,
    pub uri: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemoteJobState {
    Pending,
    Printing,
    Completed,
    Canceled,
    Aborted,
}

struct IppTarget {
    endpoint: v::vnet::EndpointV4,
    authority: String,
    path: String,
    printer_uri: String,
}

pub(crate) struct IppClient {
    vnet: VNet,
    target: IppTarget,
}

impl IppClient {
    pub(crate) fn new(printer_uri: &str) -> Result<Self, IppError> {
        let target = parse_ipp_uri(printer_uri)?;
        let vnet = VNet::open_primary().ok_or(IppError::NoNetwork)?;
        Ok(Self { vnet, target })
    }

    pub(crate) async fn print_job(
        &self,
        local_job_id: u32,
        document_format: &str,
        document: &[u8],
    ) -> Result<RemoteJob, IppError> {
        let ipp = build_print_job_request(
            local_job_id,
            self.target.printer_uri.as_str(),
            document_format,
        );
        let body = self.request(ipp.as_slice(), Some(document)).await?;
        require_ipp_success(body.as_slice())?;
        let id = find_integer_attribute(body.as_slice(), "job-id")
            .filter(|id| *id > 0)
            .ok_or(IppError::InvalidResponse)? as u32;
        let uri = find_text_attribute(body.as_slice(), "job-uri").map(String::from);
        Ok(RemoteJob { id, uri })
    }

    pub(crate) async fn job_state(
        &self,
        local_job_id: u32,
        job: &RemoteJob,
    ) -> Result<RemoteJobState, IppError> {
        let ipp =
            build_get_job_attributes_request(local_job_id, self.target.printer_uri.as_str(), job);
        let body = self.request(ipp.as_slice(), None).await?;
        require_ipp_success(body.as_slice())?;
        match find_integer_attribute(body.as_slice(), "job-state") {
            Some(3 | 4) => Ok(RemoteJobState::Pending),
            Some(5 | 6) => Ok(RemoteJobState::Printing),
            Some(7) => Ok(RemoteJobState::Canceled),
            Some(8) => Ok(RemoteJobState::Aborted),
            Some(9) => Ok(RemoteJobState::Completed),
            _ => Err(IppError::InvalidResponse),
        }
    }

    async fn request(&self, ipp: &[u8], document: Option<&[u8]>) -> Result<Vec<u8>, IppError> {
        self.vnet
            .submit(v::vnet::Command::OpenTcpConnect {
                remote: self.target.endpoint,
            })
            .map_err(|_| IppError::Connect)?;

        let connect_deadline = Instant::now() + Duration::from_millis(CONNECT_TIMEOUT_MS);
        let handle = 'connect: loop {
            while let Some(event) = self.vnet.pop_event() {
                match event {
                    v::vnet::Event::TcpEstablished { handle, .. } => break 'connect handle,
                    v::vnet::Event::Error { .. } => return Err(IppError::Connect),
                    _ => {}
                }
            }
            if Instant::now() >= connect_deadline {
                return Err(IppError::ConnectTimeout);
            }
            Timer::after(Duration::from_millis(5)).await;
        };

        let document_len = document.map_or(0, |bytes| bytes.len());
        let content_len = ipp
            .len()
            .checked_add(document_len)
            .ok_or(IppError::Transport)?;
        let head = format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS/print2d\r\nContent-Type: application/ipp\r\nAccept: application/ipp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.target.path, self.target.authority, content_len
        );
        self.vnet
            .send_tcp_all(handle, head.as_bytes())
            .and_then(|_| self.vnet.send_tcp_all(handle, ipp))
            .map_err(|_| IppError::Transport)?;
        if let Some(document) = document {
            self.vnet
                .send_tcp_all(handle, document)
                .map_err(|_| IppError::Transport)?;
        }

        let deadline = Instant::now() + Duration::from_millis(RESPONSE_TIMEOUT_MS);
        let mut response = Vec::new();
        loop {
            while let Some(event) = self.vnet.pop_event() {
                match event {
                    v::vnet::Event::TcpData {
                        handle: candidate,
                        data,
                    } if candidate == handle => {
                        if response.len().saturating_add(data.len()) > MAX_RESPONSE_BYTES {
                            let _ = self.vnet.submit(v::vnet::Command::Close { handle });
                            return Err(IppError::InvalidResponse);
                        }
                        response.extend_from_slice(data.as_slice());
                        if let Some(body) = complete_http_body(response.as_slice())? {
                            let _ = self.vnet.submit(v::vnet::Command::Close { handle });
                            return Ok(body);
                        }
                    }
                    v::vnet::Event::Closed { handle: candidate } if candidate == handle => {
                        return http_body_after_close(response.as_slice());
                    }
                    v::vnet::Event::Error { .. } => return Err(IppError::Transport),
                    _ => {}
                }
            }
            if Instant::now() >= deadline {
                let _ = self.vnet.submit(v::vnet::Command::Close { handle });
                return Err(IppError::ResponseTimeout);
            }
            Timer::after(Duration::from_millis(5)).await;
        }
    }
}

fn parse_ipp_uri(uri: &str) -> Result<IppTarget, IppError> {
    let rest = uri.strip_prefix("ipp://").ok_or(IppError::InvalidUri)?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, "ipp/print"));
    let (host, port) = authority
        .rsplit_once(':')
        .map(|(host, port)| {
            let port = port.parse::<u16>().map_err(|_| IppError::InvalidUri)?;
            Ok((host, port))
        })
        .unwrap_or(Ok((authority, 631)))?;
    let mut addr = [0u8; 4];
    let mut count = 0usize;
    for (slot, part) in addr.iter_mut().zip(host.split('.')) {
        *slot = part.parse::<u8>().map_err(|_| IppError::InvalidUri)?;
        count += 1;
    }
    if count != 4 || host.split('.').count() != 4 {
        return Err(IppError::InvalidUri);
    }
    Ok(IppTarget {
        endpoint: v::vnet::EndpointV4::new(addr, port),
        authority: authority.into(),
        path: format!("/{path}"),
        printer_uri: uri.into(),
    })
}

fn build_print_job_request(request_id: u32, printer_uri: &str, document_format: &str) -> Vec<u8> {
    let mut out = ipp_header(0x0002, request_id);
    out.push(0x01); // operation-attributes-tag
    push_text_attribute(&mut out, 0x47, "attributes-charset", "utf-8");
    push_text_attribute(&mut out, 0x48, "attributes-natural-language", "en");
    push_text_attribute(&mut out, 0x45, "printer-uri", printer_uri);
    push_text_attribute(&mut out, 0x42, "requesting-user-name", "trueos");
    push_text_attribute(&mut out, 0x42, "job-name", "GridPaper A4");
    push_text_attribute(&mut out, 0x49, "document-format", document_format);
    out.push(0x02); // job-attributes-tag
    push_text_attribute(&mut out, 0x44, "media", "iso_a4_210x297mm");
    push_text_attribute(&mut out, 0x44, "sides", "one-sided");
    push_text_attribute(&mut out, 0x44, "print-color-mode", "color");
    push_integer_attribute(&mut out, 0x23, "print-quality", 4);
    out.push(0x03); // end-of-attributes-tag
    out
}

fn build_get_job_attributes_request(
    request_id: u32,
    printer_uri: &str,
    job: &RemoteJob,
) -> Vec<u8> {
    let mut out = ipp_header(0x0009, request_id);
    out.push(0x01);
    push_text_attribute(&mut out, 0x47, "attributes-charset", "utf-8");
    push_text_attribute(&mut out, 0x48, "attributes-natural-language", "en");
    if let Some(uri) = &job.uri {
        push_text_attribute(&mut out, 0x45, "job-uri", uri);
    } else {
        push_text_attribute(&mut out, 0x45, "printer-uri", printer_uri);
        push_integer_attribute(&mut out, 0x21, "job-id", job.id as i32);
    }
    push_text_attribute(&mut out, 0x42, "requesting-user-name", "trueos");
    out.push(0x03);
    out
}

fn ipp_header(operation: u16, request_id: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    out.extend_from_slice(&[2, 0]);
    out.extend_from_slice(&operation.to_be_bytes());
    out.extend_from_slice(&request_id.to_be_bytes());
    out
}

fn push_text_attribute(out: &mut Vec<u8>, tag: u8, name: &str, value: &str) {
    out.push(tag);
    out.extend_from_slice(&(name.len() as u16).to_be_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&(value.len() as u16).to_be_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn push_integer_attribute(out: &mut Vec<u8>, tag: u8, name: &str, value: i32) {
    out.push(tag);
    out.extend_from_slice(&(name.len() as u16).to_be_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&4u16.to_be_bytes());
    out.extend_from_slice(&value.to_be_bytes());
}

fn require_ipp_success(body: &[u8]) -> Result<(), IppError> {
    if body.len() < 8 || body[0] > 2 {
        return Err(IppError::InvalidResponse);
    }
    let status = u16::from_be_bytes([body[2], body[3]]);
    if status <= 0x00ff {
        Ok(())
    } else {
        Err(IppError::Rejected)
    }
}

fn find_integer_attribute(body: &[u8], wanted: &str) -> Option<i32> {
    find_attribute(body, wanted).and_then(|value| {
        (value.len() == 4).then(|| i32::from_be_bytes([value[0], value[1], value[2], value[3]]))
    })
}

fn find_text_attribute<'a>(body: &'a [u8], wanted: &str) -> Option<&'a str> {
    core::str::from_utf8(find_attribute(body, wanted)?).ok()
}

fn find_attribute<'a>(body: &'a [u8], wanted: &str) -> Option<&'a [u8]> {
    let mut offset = 8usize;
    let mut last_name = "";
    while offset < body.len() {
        let tag = *body.get(offset)?;
        offset += 1;
        if tag == 0x03 {
            return None;
        }
        if tag <= 0x0f {
            continue;
        }
        let name_len = read_u16(body, offset)? as usize;
        offset += 2;
        let name_bytes = body.get(offset..offset.checked_add(name_len)?)?;
        offset += name_len;
        if name_len != 0 {
            last_name = core::str::from_utf8(name_bytes).ok()?;
        }
        let value_len = read_u16(body, offset)? as usize;
        offset += 2;
        let value = body.get(offset..offset.checked_add(value_len)?)?;
        offset += value_len;
        if last_name == wanted {
            return Some(value);
        }
    }
    None
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let value = bytes.get(offset..offset + 2)?;
    Some(u16::from_be_bytes([value[0], value[1]]))
}

fn complete_http_body(response: &[u8]) -> Result<Option<Vec<u8>>, IppError> {
    let Some(header_end) = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|offset| offset + 4)
    else {
        return Ok(None);
    };
    require_http_success(&response[..header_end])?;
    let headers = &response[..header_end];
    let body = &response[header_end..];
    if let Some(length) = http_content_length(headers) {
        return if body.len() >= length {
            Ok(Some(body[..length].to_vec()))
        } else {
            Ok(None)
        };
    }
    if header_has_chunked(headers) {
        return Ok(decode_chunked(body));
    }
    Ok(None)
}

fn http_body_after_close(response: &[u8]) -> Result<Vec<u8>, IppError> {
    if let Some(body) = complete_http_body(response)? {
        return Ok(body);
    }
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|offset| offset + 4)
        .ok_or(IppError::InvalidResponse)?;
    require_http_success(&response[..header_end])?;
    Ok(response[header_end..].to_vec())
}

fn require_http_success(headers: &[u8]) -> Result<(), IppError> {
    let line_end = headers
        .windows(2)
        .position(|window| window == b"\r\n")
        .ok_or(IppError::InvalidResponse)?;
    let line = core::str::from_utf8(&headers[..line_end]).map_err(|_| IppError::InvalidResponse)?;
    let status = line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or(IppError::InvalidResponse)?;
    (200..300)
        .contains(&status)
        .then_some(())
        .ok_or(IppError::HttpStatus)
}

fn http_content_length(headers: &[u8]) -> Option<usize> {
    http_header(headers, "content-length")?
        .parse::<usize>()
        .ok()
}

fn header_has_chunked(headers: &[u8]) -> bool {
    http_header(headers, "transfer-encoding").is_some_and(|value| {
        value
            .split(',')
            .any(|part| part.trim().eq_ignore_ascii_case("chunked"))
    })
}

fn http_header<'a>(headers: &'a [u8], wanted: &str) -> Option<&'a str> {
    for line in headers.split(|byte| *byte == b'\n') {
        let line = core::str::from_utf8(line.strip_suffix(b"\r").unwrap_or(line)).ok()?;
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case(wanted) {
            return Some(value.trim());
        }
    }
    None
}

fn decode_chunked(body: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    let mut offset = 0usize;
    loop {
        let line_end = body
            .get(offset..)?
            .windows(2)
            .position(|window| window == b"\r\n")?;
        let line = core::str::from_utf8(body.get(offset..offset + line_end)?).ok()?;
        let length = usize::from_str_radix(line.split(';').next()?.trim(), 16).ok()?;
        offset = offset.checked_add(line_end + 2)?;
        if length == 0 {
            return Some(output);
        }
        let end = offset.checked_add(length)?;
        output.extend_from_slice(body.get(offset..end)?);
        if body.get(end..end + 2)? != b"\r\n" {
            return None;
        }
        offset = end + 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_ipv4_ipp_uri_parses() {
        let target = parse_ipp_uri("ipp://192.0.2.1:631/ipp/print").unwrap();
        assert_eq!(target.endpoint.addr, [192, 0, 2, 1]);
        assert_eq!(target.path, "/ipp/print");
    }

    #[test]
    fn print_request_is_ipp2_and_a4_pwg() {
        let request = build_print_job_request(7, "ipp://192.0.2.1/ipp/print", "image/pwg-raster");
        assert_eq!(&request[..8], &[2, 0, 0, 2, 0, 0, 0, 7]);
        assert!(
            request
                .windows(19)
                .any(|window| window == b"iso_a4_210x297mm")
        );
        assert_eq!(request.last(), Some(&3));
    }
}
