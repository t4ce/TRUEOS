use core::error::Error as StdError;
use ::core::fmt;
use crate::io;
use core::task::{Context, Poll};
use alloc::{vec, vec::Vec};

use bytes::{BufMut, Bytes, BytesMut};
use futures_core::ready;
use http::{HeaderMap, HeaderName, HeaderValue};
use http_body::Frame;

use super::io::MemRead;
use super::role::DEFAULT_MAX_HEADERS;
use super::DecodedLength;

use self::Kind::{Chunked, Eof, Length};

/// Maximum amount of bytes allowed in chunked extensions.
///
/// This limit is currentlty applied for the entire body, not per chunk.
const CHUNKED_EXTENSIONS_LIMIT: u64 = 1024 * 16;

/// Maximum number of bytes allowed for all trailer fields.
///
/// TODO: remove this when we land h1_max_header_size support
const TRAILER_LIMIT: usize = 1024 * 16;

/// Decoders to handle different Transfer-Encodings.
///
/// If a message body does not include a Transfer-Encoding, it *should*
/// include a Content-Length header.
#[derive(Clone, PartialEq)]
pub(crate) struct Decoder {
    kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    /// A Reader used when a Content-Length header is passed with a positive integer.
    Length(u64),
    /// A Reader used when Transfer-Encoding is `chunked`.
    Chunked {
        state: ChunkedState,
        chunk_len: u64,
        extensions_cnt: u64,
        trailers_buf: Option<BytesMut>,
        trailers_cnt: usize,
        h1_max_headers: Option<usize>,
        h1_max_header_size: Option<usize>,
    },
    /// A Reader used for responses that don't indicate a length or chunked.
    ///
    /// The bool tracks when EOF is seen on the transport.
    ///
    /// Note: This should only used for `Response`s. It is illegal for a
    /// `Request` to be made with both `Content-Length` and
    /// `Transfer-Encoding: chunked` missing, as explained from the spec:
    ///
    /// > If a Transfer-Encoding header field is present in a response and
    /// > the chunked transfer coding is not the final encoding, the
    /// > message body length is determined by reading the connection until
    /// > it is closed by the server.  If a Transfer-Encoding header field
    /// > is present in a request and the chunked transfer coding is not
    /// > the final encoding, the message body length cannot be determined
    /// > reliably; the server MUST respond with the 400 (Bad Request)
    /// > status code and then close the connection.
    Eof(bool),
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum ChunkedState {
    Start,
    Size,
    SizeLws,
    Extension,
    SizeLf,
    Body,
    BodyCr,
    BodyLf,
    Trailer,
    TrailerLf,
    EndCr,
    EndLf,
    End,
}

impl Decoder {
    // constructors

    pub(crate) fn length(x: u64) -> Decoder {
        Decoder {
            kind: Kind::Length(x),
        }
    }

    pub(crate) fn chunked(
        h1_max_headers: Option<usize>,
        h1_max_header_size: Option<usize>,
    ) -> Decoder {
        Decoder {
            kind: Kind::Chunked {
                state: ChunkedState::new(),
                chunk_len: 0,
                extensions_cnt: 0,
                trailers_buf: None,
                trailers_cnt: 0,
                h1_max_headers,
                h1_max_header_size,
            },
        }
    }

    pub(crate) fn eof() -> Decoder {
        Decoder {
            kind: Kind::Eof(false),
        }
    }

    pub(super) fn new(
        len: DecodedLength,
        h1_max_headers: Option<usize>,
        h1_max_header_size: Option<usize>,
    ) -> Self {
        match len {
            DecodedLength::CHUNKED => Decoder::chunked(h1_max_headers, h1_max_header_size),
            DecodedLength::CLOSE_DELIMITED => Decoder::eof(),
            length => Decoder::length(length.danger_len()),
        }
    }

    // methods

    pub(crate) fn is_eof(&self) -> bool {
        matches!(
            self.kind,
            Length(0)
                | Chunked {
                    state: ChunkedState::End,
                    ..
                }
                | Eof(true)
        )
    }

    pub(crate) fn decode<R: MemRead>(
        &mut self,
        cx: &mut Context<'_>,
        body: &mut R,
    ) -> Poll<Result<Frame<Bytes>, io::Error>> {
        trace!("decode; state={:?}", self.kind);
        match self.kind {
            Length(ref mut remaining) => {
                if *remaining == 0 {
                    Poll::Ready(Ok(Frame::data(Bytes::new())))
                } else {
                    let to_read = *remaining as usize;
                    let buf = ready!(body.read_mem(cx, to_read))?;
                    let num = buf.as_ref().len() as u64;
                    if num > *remaining {
                        *remaining = 0;
                    } else if num == 0 {
                        return Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "incomplete body",
                        )));
                    } else {
                        *remaining -= num;
                    }
                    Poll::Ready(Ok(Frame::data(buf)))
                }
            }
            Chunked {
                ref mut state,
                ref mut chunk_len,
                ref mut extensions_cnt,
                ref mut trailers_buf,
                ref mut trailers_cnt,
                ref h1_max_headers,
                ref h1_max_header_size,
            } => {
                let h1_max_headers = h1_max_headers.unwrap_or(DEFAULT_MAX_HEADERS);
                let h1_max_header_size = h1_max_header_size.unwrap_or(TRAILER_LIMIT);
                loop {
                    let mut buf = None;
                    // advances the chunked state
                    *state = ready!(state.step(
                        cx,
                        body,
                        StepArgs {
                            chunk_size: chunk_len,
                            extensions_cnt,
                            chunk_buf: &mut buf,
                            trailers_buf,
                            trailers_cnt,
                            max_headers_cnt: h1_max_headers,
                            max_headers_bytes: h1_max_header_size,
                        }
                    ))?;
                    if *state == ChunkedState::End {
                        trace!("end of chunked");

                        if trailers_buf.is_some() {
                            trace!("found possible trailers");

                            // decoder enforces that trailers count will not exceed h1_max_headers
                            if *trailers_cnt >= h1_max_headers {
                                return Poll::Ready(Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "chunk trailers count overflow",
                                )));
                            }
                            match decode_trailers(
                                &mut trailers_buf.take().expect("Trailer is None"),
                                *trailers_cnt,
                            ) {
                                Ok(headers) => {
                                    return Poll::Ready(Ok(Frame::trailers(headers)));
                                }
                                Err(e) => {
                                    return Poll::Ready(Err(e));
                                }
                            }
                        }

                        return Poll::Ready(Ok(Frame::data(Bytes::new())));
                    }
                    if let Some(buf) = buf {
                        return Poll::Ready(Ok(Frame::data(buf)));
                    }
                }
            }
            Eof(ref mut is_eof) => {
                if *is_eof {
                    Poll::Ready(Ok(Frame::data(Bytes::new())))
                } else {
                    // 8192 chosen because its about 2 packets, there probably
                    // won't be that much available, so don't have MemReaders
                    // allocate buffers to big
                    body.read_mem(cx, 8192).map_ok(|slice| {
                        *is_eof = slice.is_empty();
                        Frame::data(slice)
                    })
                }
            }
        }
    }

}

impl fmt::Debug for Decoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.kind, f)
    }
}

macro_rules! byte (
    ($rdr:ident, $cx:expr) => ({
        let buf = ready!($rdr.read_mem($cx, 1))?;
        if !buf.is_empty() {
            buf[0]
        } else {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::UnexpectedEof,
                                      "unexpected EOF during chunk size line")));
        }
    })
);

macro_rules! or_overflow {
    ($e:expr) => (
        match $e {
            Some(val) => val,
            None => return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid chunk size: overflow",
            ))),
        }
    )
}

macro_rules! put_u8 {
    ($trailers_buf:expr, $byte:expr, $limit:expr) => {
        $trailers_buf.put_u8($byte);

        if $trailers_buf.len() >= $limit {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "chunk trailers bytes over limit",
            )));
        }
    };
}

struct StepArgs<'a> {
    chunk_size: &'a mut u64,
    chunk_buf: &'a mut Option<Bytes>,
    extensions_cnt: &'a mut u64,
    trailers_buf: &'a mut Option<BytesMut>,
    trailers_cnt: &'a mut usize,
    max_headers_cnt: usize,
    max_headers_bytes: usize,
}

impl ChunkedState {
    fn new() -> ChunkedState {
        ChunkedState::Start
    }
    fn step<R: MemRead>(
        &self,
        cx: &mut Context<'_>,
        body: &mut R,
        StepArgs {
            chunk_size,
            chunk_buf,
            extensions_cnt,
            trailers_buf,
            trailers_cnt,
            max_headers_cnt,
            max_headers_bytes,
        }: StepArgs<'_>,
    ) -> Poll<Result<ChunkedState, io::Error>> {
        use self::ChunkedState::*;
        match *self {
            Start => ChunkedState::read_start(cx, body, chunk_size),
            Size => ChunkedState::read_size(cx, body, chunk_size),
            SizeLws => ChunkedState::read_size_lws(cx, body),
            Extension => ChunkedState::read_extension(cx, body, extensions_cnt),
            SizeLf => ChunkedState::read_size_lf(cx, body, *chunk_size),
            Body => ChunkedState::read_body(cx, body, chunk_size, chunk_buf),
            BodyCr => ChunkedState::read_body_cr(cx, body),
            BodyLf => ChunkedState::read_body_lf(cx, body),
            Trailer => ChunkedState::read_trailer(cx, body, trailers_buf, max_headers_bytes),
            TrailerLf => ChunkedState::read_trailer_lf(
                cx,
                body,
                trailers_buf,
                trailers_cnt,
                max_headers_cnt,
                max_headers_bytes,
            ),
            EndCr => ChunkedState::read_end_cr(cx, body, trailers_buf, max_headers_bytes),
            EndLf => ChunkedState::read_end_lf(cx, body, trailers_buf, max_headers_bytes),
            End => Poll::Ready(Ok(ChunkedState::End)),
        }
    }

    fn read_start<R: MemRead>(
        cx: &mut Context<'_>,
        rdr: &mut R,
        size: &mut u64,
    ) -> Poll<Result<ChunkedState, io::Error>> {
        trace!("Read chunk start");

        let radix = 16;
        match byte!(rdr, cx) {
            b @ b'0'..=b'9' => {
                *size = or_overflow!(size.checked_mul(radix));
                *size = or_overflow!(size.checked_add((b - b'0') as u64));
            }
            b @ b'a'..=b'f' => {
                *size = or_overflow!(size.checked_mul(radix));
                *size = or_overflow!(size.checked_add((b + 10 - b'a') as u64));
            }
            b @ b'A'..=b'F' => {
                *size = or_overflow!(size.checked_mul(radix));
                *size = or_overflow!(size.checked_add((b + 10 - b'A') as u64));
            }
            _ => {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Invalid chunk size line: missing size digit",
                )));
            }
        }

        Poll::Ready(Ok(ChunkedState::Size))
    }

    fn read_size<R: MemRead>(
        cx: &mut Context<'_>,
        rdr: &mut R,
        size: &mut u64,
    ) -> Poll<Result<ChunkedState, io::Error>> {
        trace!("Read chunk hex size");

        let radix = 16;
        match byte!(rdr, cx) {
            b @ b'0'..=b'9' => {
                *size = or_overflow!(size.checked_mul(radix));
                *size = or_overflow!(size.checked_add((b - b'0') as u64));
            }
            b @ b'a'..=b'f' => {
                *size = or_overflow!(size.checked_mul(radix));
                *size = or_overflow!(size.checked_add((b + 10 - b'a') as u64));
            }
            b @ b'A'..=b'F' => {
                *size = or_overflow!(size.checked_mul(radix));
                *size = or_overflow!(size.checked_add((b + 10 - b'A') as u64));
            }
            b'\t' | b' ' => return Poll::Ready(Ok(ChunkedState::SizeLws)),
            b';' => return Poll::Ready(Ok(ChunkedState::Extension)),
            b'\r' => return Poll::Ready(Ok(ChunkedState::SizeLf)),
            _ => {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Invalid chunk size line: Invalid Size",
                )));
            }
        }
        Poll::Ready(Ok(ChunkedState::Size))
    }
    fn read_size_lws<R: MemRead>(
        cx: &mut Context<'_>,
        rdr: &mut R,
    ) -> Poll<Result<ChunkedState, io::Error>> {
        trace!("read_size_lws");
        match byte!(rdr, cx) {
            // LWS can follow the chunk size, but no more digits can come
            b'\t' | b' ' => Poll::Ready(Ok(ChunkedState::SizeLws)),
            b';' => Poll::Ready(Ok(ChunkedState::Extension)),
            b'\r' => Poll::Ready(Ok(ChunkedState::SizeLf)),
            _ => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid chunk size linear white space",
            ))),
        }
    }
    fn read_extension<R: MemRead>(
        cx: &mut Context<'_>,
        rdr: &mut R,
        extensions_cnt: &mut u64,
    ) -> Poll<Result<ChunkedState, io::Error>> {
        trace!("read_extension");
        // We don't care about extensions really at all. Just ignore them.
        // They "end" at the next CRLF.
        //
        // However, some implementations may not check for the CR, so to save
        // them from themselves, we reject extensions containing plain LF as
        // well.
        match byte!(rdr, cx) {
            b'\r' => Poll::Ready(Ok(ChunkedState::SizeLf)),
            b'\n' => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid chunk extension contains newline",
            ))),
            _ => {
                *extensions_cnt += 1;
                if *extensions_cnt >= CHUNKED_EXTENSIONS_LIMIT {
                    Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "chunk extensions over limit",
                    )))
                } else {
                    Poll::Ready(Ok(ChunkedState::Extension))
                }
            } // no supported extensions
        }
    }
    fn read_size_lf<R: MemRead>(
        cx: &mut Context<'_>,
        rdr: &mut R,
        size: u64,
    ) -> Poll<Result<ChunkedState, io::Error>> {
        trace!("Chunk size is {:?}", size);
        match byte!(rdr, cx) {
            b'\n' => {
                if size == 0 {
                    Poll::Ready(Ok(ChunkedState::EndCr))
                } else {
                    debug!("incoming chunked header: {0:#X} ({0} bytes)", size);
                    Poll::Ready(Ok(ChunkedState::Body))
                }
            }
            _ => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid chunk size LF",
            ))),
        }
    }

    fn read_body<R: MemRead>(
        cx: &mut Context<'_>,
        rdr: &mut R,
        rem: &mut u64,
        buf: &mut Option<Bytes>,
    ) -> Poll<Result<ChunkedState, io::Error>> {
        trace!("Chunked read, remaining={:?}", rem);

        // cap remaining bytes at the max capacity of usize
        let rem_cap = match *rem {
            r if r > usize::MAX as u64 => usize::MAX,
            r => r as usize,
        };

        let to_read = rem_cap;
        let slice = ready!(rdr.read_mem(cx, to_read))?;
        let count = slice.len();

        if count == 0 {
            *rem = 0;
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete body",
            )));
        }
        *buf = Some(slice);
        *rem -= count as u64;

        if *rem > 0 {
            Poll::Ready(Ok(ChunkedState::Body))
        } else {
            Poll::Ready(Ok(ChunkedState::BodyCr))
        }
    }
    fn read_body_cr<R: MemRead>(
        cx: &mut Context<'_>,
        rdr: &mut R,
    ) -> Poll<Result<ChunkedState, io::Error>> {
        match byte!(rdr, cx) {
            b'\r' => Poll::Ready(Ok(ChunkedState::BodyLf)),
            _ => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid chunk body CR",
            ))),
        }
    }
    fn read_body_lf<R: MemRead>(
        cx: &mut Context<'_>,
        rdr: &mut R,
    ) -> Poll<Result<ChunkedState, io::Error>> {
        match byte!(rdr, cx) {
            b'\n' => Poll::Ready(Ok(ChunkedState::Start)),
            _ => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid chunk body LF",
            ))),
        }
    }

    fn read_trailer<R: MemRead>(
        cx: &mut Context<'_>,
        rdr: &mut R,
        trailers_buf: &mut Option<BytesMut>,
        h1_max_header_size: usize,
    ) -> Poll<Result<ChunkedState, io::Error>> {
        trace!("read_trailer");
        let byte = byte!(rdr, cx);

        put_u8!(
            trailers_buf.as_mut().expect("trailers_buf is None"),
            byte,
            h1_max_header_size
        );

        match byte {
            b'\r' => Poll::Ready(Ok(ChunkedState::TrailerLf)),
            _ => Poll::Ready(Ok(ChunkedState::Trailer)),
        }
    }

    fn read_trailer_lf<R: MemRead>(
        cx: &mut Context<'_>,
        rdr: &mut R,
        trailers_buf: &mut Option<BytesMut>,
        trailers_cnt: &mut usize,
        h1_max_headers: usize,
        h1_max_header_size: usize,
    ) -> Poll<Result<ChunkedState, io::Error>> {
        let byte = byte!(rdr, cx);
        match byte {
            b'\n' => {
                if *trailers_cnt >= h1_max_headers {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "chunk trailers count overflow",
                    )));
                }
                *trailers_cnt += 1;

                put_u8!(
                    trailers_buf.as_mut().expect("trailers_buf is None"),
                    byte,
                    h1_max_header_size
                );

                Poll::Ready(Ok(ChunkedState::EndCr))
            }
            _ => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid trailer end LF",
            ))),
        }
    }

    fn read_end_cr<R: MemRead>(
        cx: &mut Context<'_>,
        rdr: &mut R,
        trailers_buf: &mut Option<BytesMut>,
        h1_max_header_size: usize,
    ) -> Poll<Result<ChunkedState, io::Error>> {
        let byte = byte!(rdr, cx);
        match byte {
            b'\r' => {
                if let Some(trailers_buf) = trailers_buf {
                    put_u8!(trailers_buf, byte, h1_max_header_size);
                }
                Poll::Ready(Ok(ChunkedState::EndLf))
            }
            byte => {
                match trailers_buf {
                    None => {
                        // 64 will fit a single Expires header without reallocating
                        let mut buf = BytesMut::with_capacity(64);
                        buf.put_u8(byte);
                        *trailers_buf = Some(buf);
                    }
                    Some(trailers_buf) => {
                        put_u8!(trailers_buf, byte, h1_max_header_size);
                    }
                }

                Poll::Ready(Ok(ChunkedState::Trailer))
            }
        }
    }
    fn read_end_lf<R: MemRead>(
        cx: &mut Context<'_>,
        rdr: &mut R,
        trailers_buf: &mut Option<BytesMut>,
        h1_max_header_size: usize,
    ) -> Poll<Result<ChunkedState, io::Error>> {
        let byte = byte!(rdr, cx);
        match byte {
            b'\n' => {
                if let Some(trailers_buf) = trailers_buf {
                    put_u8!(trailers_buf, byte, h1_max_header_size);
                }
                Poll::Ready(Ok(ChunkedState::End))
            }
            _ => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid chunk end LF",
            ))),
        }
    }
}

// TODO: disallow Transfer-Encoding, Content-Length, Trailer, etc in trailers ??
fn decode_trailers(buf: &mut BytesMut, count: usize) -> Result<HeaderMap, io::Error> {
    let mut trailers = HeaderMap::new();
    let mut headers = vec![httparse::EMPTY_HEADER; count];
    let res = httparse::parse_headers(buf, &mut headers);
    match res {
        Ok(httparse::Status::Complete((_, headers))) => {
            for header in headers.iter() {
                use core::convert::TryFrom;
                let name = match HeaderName::try_from(header.name) {
                    Ok(name) => name,
                    Err(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "invalid header name",
                        ));
                    }
                };

                let value = match HeaderValue::from_bytes(header.value) {
                    Ok(value) => value,
                    Err(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "invalid header value",
                        ));
                    }
                };

                trailers.insert(name, value);
            }

            Ok(trailers)
        }
        Ok(httparse::Status::Partial) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Partial header",
        )),
        Err(_) => Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid header")),
    }
}

#[derive(Debug)]
struct IncompleteBody;

impl fmt::Display for IncompleteBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "end of file before message length reached")
    }
}

impl StdError for IncompleteBody {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rt::{Read, ReadBuf};
    use core::pin::Pin;
    use core::time::Duration;

    impl MemRead for &[u8] {
        fn read_mem(&mut self, _: &mut Context<'_>, len: usize) -> Poll<io::Result<Bytes>> {
            let n = core::cmp::min(len, self.len());
            if n > 0 {
                let (a, b) = self.split_at(n);
                let buf = Bytes::copy_from_slice(a);
                *self = b;
                Poll::Ready(Ok(buf))
            } else {
                Poll::Ready(Ok(Bytes::new()))
            }
        }
    }

    impl MemRead for &mut (dyn Read + Unpin) {
        fn read_mem(&mut self, cx: &mut Context<'_>, len: usize) -> Poll<io::Result<Bytes>> {
            let mut v = vec![0; len];
            let mut buf = ReadBuf::new(&mut v);
            ready!(Pin::new(self).poll_read(cx, buf.unfilled())?);
            Poll::Ready(Ok(Bytes::copy_from_slice(buf.filled())))
        }
    }

    impl MemRead for Bytes {
        fn read_mem(&mut self, _: &mut Context<'_>, len: usize) -> Poll<io::Result<Bytes>> {
            let n = core::cmp::min(len, self.len());
            let ret = self.split_to(n);
            Poll::Ready(Ok(ret))
        }
    }

    /*
    use crate::io;
    use crate::io::Write;
    use super::Decoder;
    use super::ChunkedState;
    use futures::{Async, Poll};
    use bytes::{BytesMut, Bytes};
    use crate::mock::AsyncIo;
    */

    #[cfg(not(miri))]

    #[cfg(not(miri))]

    #[cfg(not(miri))]

    #[cfg(not(miri))]



    #[cfg(not(miri))]

    #[cfg(not(miri))]

    // perform an async read using a custom buffer size and causing a blocking
    // read at the specified byte
    async fn read_async(mut decoder: Decoder, content: &[u8], block_at: usize) -> String {
        let mut outs = Vec::new();

        let mut ins = crate::common::io::Compat::new(if block_at == 0 {
            tokio_test::io::Builder::new()
                .wait(Duration::from_millis(10))
                .read(content)
                .build()
        } else {
            tokio_test::io::Builder::new()
                .read(&content[..block_at])
                .wait(Duration::from_millis(10))
                .read(&content[block_at..])
                .build()
        });

        let mut ins = &mut ins as &mut (dyn Read + Unpin);

        loop {
            let buf = decoder
                .decode_fut(&mut ins)
                .await
                .expect("unexpected decode error")
                .into_data()
                .expect("unexpected frame type");
            if buf.is_empty() {
                break; // eof
            }
            outs.extend(buf.as_ref());
        }

        String::from_utf8(outs).expect("decode String")
    }

    // iterate over the different ways that this async read could go.
    // tests blocking a read at each byte along the content - The shotgun approach
    async fn all_async_cases(content: &str, expected: &str, decoder: Decoder) {
        let content_len = content.len();
        for block_at in 0..content_len {
            let actual = read_async(decoder.clone(), content.as_bytes(), block_at).await;
            assert_eq!(expected, &actual) //, "Failed async. Blocking at {}", block_at);
        }
    }

    #[cfg(not(miri))]

    #[cfg(not(miri))]

    #[cfg(not(miri))]

    #[cfg(all(feature = "nightly", not(miri)))]
    #[bench]
    fn bench_decode_chunked_1kb(b: &mut test::Bencher) {
        let rt = new_runtime();

        const LEN: usize = 1024;
        let mut vec = Vec::new();
        vec.extend(format!("{:x}\r\n", LEN).as_bytes());
        vec.extend(&[0; LEN][..]);
        vec.extend(b"\r\n");
        let content = Bytes::from(vec);

        b.bytes = LEN as u64;

        b.iter(|| {
            let mut decoder = Decoder::chunked(None, None);
            rt.block_on(async {
                let mut raw = content.clone();
                let chunk = decoder
                    .decode_fut(&mut raw)
                    .await
                    .unwrap()
                    .into_data()
                    .unwrap();
                assert_eq!(chunk.len(), LEN);
            });
        });
    }

    #[cfg(all(feature = "nightly", not(miri)))]
    #[bench]
    fn bench_decode_length_1kb(b: &mut test::Bencher) {
        let rt = new_runtime();

        const LEN: usize = 1024;
        let content = Bytes::from(&[0; LEN][..]);
        b.bytes = LEN as u64;

        b.iter(|| {
            let mut decoder = Decoder::length(LEN as u64);
            rt.block_on(async {
                let mut raw = content.clone();
                let chunk = decoder
                    .decode_fut(&mut raw)
                    .await
                    .unwrap()
                    .into_data()
                    .unwrap();
                assert_eq!(chunk.len(), LEN);
            });
        });
    }

    #[cfg(feature = "nightly")]
    fn new_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("rt build")
    }




}
