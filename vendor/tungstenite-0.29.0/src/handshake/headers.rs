//! HTTP Request and response header handling.

use http::header::{HeaderMap, HeaderName, HeaderValue};
use httparse::Status;

use super::machine::TryParse;
use crate::error::Result;

/// Limit for the number of header lines.
pub const MAX_HEADERS: usize = 124;

/// Trait to convert raw objects into HTTP parseables.
pub(crate) trait FromHttparse<T>: Sized {
    /// Convert raw object into parsed HTTP headers.
    fn from_httparse(raw: T) -> Result<Self>;
}

impl<'b: 'h, 'h> FromHttparse<&'b [httparse::Header<'h>]> for HeaderMap {
    fn from_httparse(raw: &'b [httparse::Header<'h>]) -> Result<Self> {
        let mut headers = HeaderMap::new();
        for h in raw {
            headers.append(
                HeaderName::from_bytes(h.name.as_bytes())?,
                HeaderValue::from_bytes(h.value)?,
            );
        }

        Ok(headers)
    }
}
impl TryParse for HeaderMap {
    fn try_parse(buf: &[u8]) -> Result<Option<(usize, Self)>> {
        let mut hbuffer = [httparse::EMPTY_HEADER; MAX_HEADERS];
        Ok(match httparse::parse_headers(buf, &mut hbuffer)? {
            Status::Partial => None,
            Status::Complete((size, hdr)) => Some((size, HeaderMap::from_httparse(hdr)?)),
        })
    }
}
