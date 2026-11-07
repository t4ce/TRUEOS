//! Bounded little-endian wire records shared by the SDK and kernel.
use super::{Header, MAX_FRAME_BYTES, MAX_HEADERS, Method, RequestOwned, ResponseHead, Timeouts};
use alloc::{string::String, vec::Vec};
const BAD: i32 = -4;
const LARGE: i32 = -7;
const ALLOC: i32 = -3;

pub fn validate_url(url: &str) -> Result<(), i32> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or(-10)?;
    let authority = rest.split(['/', '?']).next().ok_or(-10)?;
    if authority.is_empty()
        || authority.contains('@')
        || url.contains('#')
        || url
            .bytes()
            .any(|b| b.is_ascii_control() || b == b' ' || b == 0x7f)
    {
        return Err(-10);
    }
    Ok(())
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}
fn valid_value(value: &[u8]) -> bool {
    value.iter().all(|&b| b == b'\t' || b >= 0x20 && b != 0x7f)
}
fn encoded_len(fixed: usize, url: &str, headers: &[Header]) -> Result<usize, i32> {
    validate_url(url)?;
    if headers.len() > MAX_HEADERS {
        return Err(LARGE);
    }
    let mut len = fixed.checked_add(url.len()).ok_or(LARGE)?;
    for header in headers {
        if !valid_name(&header.name) || !valid_value(&header.value) {
            return Err(BAD);
        }
        if header.name.len() > u16::MAX as usize || header.value.len() > u32::MAX as usize {
            return Err(LARGE);
        }
        len = len
            .checked_add(6)
            .and_then(|v| v.checked_add(header.name.len()))
            .and_then(|v| v.checked_add(header.value.len()))
            .ok_or(LARGE)?;
    }
    if len > MAX_FRAME_BYTES {
        return Err(LARGE);
    }
    Ok(len)
}
fn buffer(len: usize) -> Result<Vec<u8>, i32> {
    let mut out = Vec::new();
    out.try_reserve_exact(len).map_err(|_| ALLOC)?;
    Ok(out)
}
fn append_headers(out: &mut Vec<u8>, headers: &[Header]) {
    for header in headers {
        out.extend_from_slice(&(header.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&(header.value.len() as u32).to_le_bytes());
        out.extend_from_slice(header.name.as_bytes());
        out.extend_from_slice(&header.value);
    }
}
pub fn encode_request(request: &RequestOwned) -> Result<Vec<u8>, i32> {
    let len = encoded_len(24, &request.url, &request.headers)?;
    let mut out = buffer(len)?;
    out.extend_from_slice(b"THQ1");
    out.push(match request.method {
        Method::Get => 1,
        Method::Head => 2,
    });
    out.push(0);
    out.extend_from_slice(&(request.headers.len() as u16).to_le_bytes());
    out.extend_from_slice(&(request.url.len() as u32).to_le_bytes());
    for timeout in [
        request.timeouts.connect_ms,
        request.timeouts.tls_ms,
        request.timeouts.read_idle_ms,
    ] {
        out.extend_from_slice(&timeout.to_le_bytes());
    }
    out.extend_from_slice(request.url.as_bytes());
    append_headers(&mut out, &request.headers);
    Ok(out)
}
pub fn encode_head(head: &ResponseHead) -> Result<Vec<u8>, i32> {
    if !(100..=999).contains(&head.status) {
        return Err(BAD);
    }
    let len = encoded_len(12, &head.url, &head.headers)?;
    let mut out = buffer(len)?;
    out.extend_from_slice(b"THS1");
    out.extend_from_slice(&head.status.to_le_bytes());
    out.extend_from_slice(&(head.headers.len() as u16).to_le_bytes());
    out.extend_from_slice(&(head.url.len() as u32).to_le_bytes());
    out.extend_from_slice(head.url.as_bytes());
    append_headers(&mut out, &head.headers);
    Ok(out)
}
struct Cursor<'a> {
    rest: &'a [u8],
}
impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], i32> {
        let bytes = self.rest.get(..len).ok_or(BAD)?;
        self.rest = &self.rest[len..];
        Ok(bytes)
    }
    fn u16(&mut self) -> Result<u16, i32> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().map_err(|_| BAD)?,
        ))
    }
    fn u32(&mut self) -> Result<u32, i32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().map_err(|_| BAD)?,
        ))
    }
    fn string(&mut self, len: usize) -> Result<String, i32> {
        let bytes = self.take(len)?;
        core::str::from_utf8(bytes).map_err(|_| -1)?;
        let mut out = buffer(bytes.len())?;
        out.extend_from_slice(bytes);
        String::from_utf8(out).map_err(|_| -1)
    }
    fn headers(&mut self, count: usize) -> Result<Vec<Header>, i32> {
        if count > MAX_HEADERS || count > self.rest.len() / 7 {
            return Err(BAD);
        }
        let mut headers = Vec::new();
        headers.try_reserve_exact(count).map_err(|_| ALLOC)?;
        for _ in 0..count {
            let name_len = usize::from(self.u16()?);
            let value_len = usize::try_from(self.u32()?).map_err(|_| BAD)?;
            let name = self.string(name_len)?;
            let value = self.take(value_len)?;
            if !valid_name(&name) || !valid_value(value) {
                return Err(BAD);
            }
            let mut bytes = buffer(value.len())?;
            bytes.extend_from_slice(value);
            headers.push(Header { name, value: bytes });
        }
        if !self.rest.is_empty() {
            return Err(BAD);
        }
        Ok(headers)
    }
}
fn cursor<'a>(bytes: &'a [u8], magic: &[u8; 4], min: usize) -> Result<Cursor<'a>, i32> {
    if bytes.len() < min
        || bytes.len() > MAX_FRAME_BYTES
        || bytes.get(..4) != Some(magic.as_slice())
    {
        return Err(BAD);
    }
    Ok(Cursor { rest: &bytes[4..] })
}
pub fn decode_request(bytes: &[u8]) -> Result<RequestOwned, i32> {
    let mut c = cursor(bytes, b"THQ1", 24)?;
    let method = match c.take(1)?[0] {
        1 => Method::Get,
        2 => Method::Head,
        _ => return Err(BAD),
    };
    if c.take(1)?[0] != 0 {
        return Err(BAD);
    }
    let count = usize::from(c.u16()?);
    let url_len = usize::try_from(c.u32()?).map_err(|_| BAD)?;
    let timeouts = Timeouts {
        connect_ms: c.u32()?,
        tls_ms: c.u32()?,
        read_idle_ms: c.u32()?,
    };
    let url = c.string(url_len)?;
    validate_url(&url)?;
    let headers = c.headers(count)?;
    Ok(RequestOwned {
        method,
        url,
        headers,
        timeouts,
    })
}
pub fn decode_head(bytes: &[u8]) -> Result<ResponseHead, i32> {
    let mut c = cursor(bytes, b"THS1", 12)?;
    let status = c.u16()?;
    if !(100..=999).contains(&status) {
        return Err(BAD);
    }
    let count = usize::from(c.u16()?);
    let url_len = usize::try_from(c.u32()?).map_err(|_| BAD)?;
    let url = c.string(url_len)?;
    validate_url(&url)?;
    let headers = c.headers(count)?;
    Ok(ResponseHead {
        status,
        url,
        headers,
    })
}
