//! Native, nonblocking single-exchange HTTP(S) requests.
//! Redirect and file-publication policies belong to the caller.
use crate::vcabi;
use alloc::{string::String, vec::Vec};

#[path = "http_request_wire.rs"]
pub mod wire;

pub const NET_PENDING: i32 = -116;
pub const MAX_FRAME_BYTES: usize = 32 * 1024;
pub const MAX_BODY_READ_BYTES: usize = 32 * 1024;
pub const MAX_HEADERS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    Get,
    Head,
}

/// Zero disables a phase timeout. There is no total request deadline.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Timeouts {
    pub connect_ms: u32,
    pub tls_ms: u32,
    pub read_idle_ms: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Header {
    pub name: String,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestOwned {
    pub method: Method,
    /// Canonical HTTP(S) transport URL, without userinfo or fragment.
    pub url: String,
    pub headers: Vec<Header>,
    pub timeouts: Timeouts,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponseHead {
    pub status: u16,
    /// The URL of this exchange; redirects are handled by the caller.
    pub url: String,
    /// Duplicate values retain their order; cross-name ordering is unspecified.
    pub headers: Vec<Header>,
}
impl ResponseHead {
    pub fn values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a [u8]> {
        self.headers
            .iter()
            .filter(move |header| header.name.eq_ignore_ascii_case(name))
            .map(|header| header.value.as_slice())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BodyRead {
    Pending,
    Data(usize),
    Eof,
}

/// Owns a kernel request until explicit discard or Drop, including after EOF.
#[derive(Debug)]
pub struct Request {
    id: u32,
}
impl Request {
    pub fn start(request: &RequestOwned) -> Result<Self, i32> {
        let frame = wire::encode_request(request)?;
        let id =
            unsafe { vcabi::trueos_cabi_net_http_request_start_v1(frame.as_ptr(), frame.len()) };
        if id <= 0 {
            return Err(if id == 0 { -4 } else { id });
        }
        Ok(Self { id: id as u32 })
    }
    pub fn try_head(&mut self) -> Result<Option<ResponseHead>, i32> {
        let len = unsafe {
            vcabi::trueos_cabi_net_http_request_head_v1(self.id, core::ptr::null_mut(), 0)
        };
        if len == NET_PENDING as isize {
            return Ok(None);
        }
        if len < 0 {
            return Err(len as i32);
        }
        let len = usize::try_from(len).map_err(|_| -4)?;
        if len == 0 || len > MAX_FRAME_BYTES {
            return Err(-4);
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(len).map_err(|_| -3)?;
        bytes.resize(len, 0);
        let got = unsafe {
            vcabi::trueos_cabi_net_http_request_head_v1(self.id, bytes.as_mut_ptr(), bytes.len())
        };
        if got < 0 {
            return Err(got as i32);
        }
        if got as usize != len {
            return Err(-2);
        }
        wire::decode_head(&bytes).map(Some)
    }
    pub fn try_read(&mut self, out: &mut [u8]) -> Result<BodyRead, i32> {
        let cap = out.len().min(MAX_BODY_READ_BYTES);
        if cap == 0 {
            return Err(-4);
        }
        let got =
            unsafe { vcabi::trueos_cabi_net_http_request_read_v1(self.id, out.as_mut_ptr(), cap) };
        match got {
            value if value == NET_PENDING as isize => Ok(BodyRead::Pending),
            value if value < 0 => Err(value as i32),
            0 => Ok(BodyRead::Eof),
            value if value as usize <= cap => Ok(BodyRead::Data(value as usize)),
            _ => Err(-2),
        }
    }
    pub fn discard(mut self) -> Result<(), i32> {
        let status = vcabi::trueos_cabi_net_http_request_discard_v1(self.id);
        self.id = 0;
        if status == 0 { Ok(()) } else { Err(status) }
    }
}
impl Drop for Request {
    fn drop(&mut self) {
        if self.id != 0 {
            let _ = vcabi::trueos_cabi_net_http_request_discard_v1(self.id);
            self.id = 0;
        }
    }
}
