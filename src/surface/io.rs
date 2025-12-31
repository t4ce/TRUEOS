use alloc::{string::String, vec, vec::Vec};
use core::{cmp, fmt, str};

/// Convenient alias that mirrors `std::io::Result`.
pub type Result<T> = core::result::Result<T, Error>;

/// Coarse-grained classification for I/O failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    UnexpectedEof,
    WriteZero,
    WouldBlock,
    InvalidInput,
    InvalidData,
    Interrupted,
    Other,
}

/// Lightweight error type carried by `io::Result`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Error {
    kind: ErrorKind,
}

impl Error {
    pub const fn new(kind: ErrorKind) -> Self {
        Self { kind }
    }

    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self.kind {
            ErrorKind::UnexpectedEof => "unexpected end of file",
            ErrorKind::WriteZero => "failed to write whole buffer",
            ErrorKind::WouldBlock => "operation would block",
            ErrorKind::InvalidInput => "invalid input",
            ErrorKind::InvalidData => "invalid data",
            ErrorKind::Interrupted => "operation interrupted",
            ErrorKind::Other => "io error",
        };
        f.write_str(msg)
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Self::new(kind)
    }
}

/// Trait for byte-oriented readers.
pub trait Read {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;

    fn read_exact(&mut self, mut buf: &mut [u8]) -> Result<()> {
        while !buf.is_empty() {
            match self.read(buf) {
                Ok(0) => return Err(Error::new(ErrorKind::UnexpectedEof)),
                Ok(n) => {
                    let tmp = buf;
                    buf = &mut tmp[n..];
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<usize> {
        let start_len = buf.len();
        let mut tmp = vec![0u8; DEFAULT_BUF_SIZE];
        loop {
            match self.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(buf.len() - start_len)
    }

    fn take(self, limit: u64) -> Take<Self>
    where
        Self: Sized,
    {
        Take { inner: self, limit }
    }
}

/// Trait for byte sinks.
pub trait Write {
    fn write(&mut self, buf: &[u8]) -> Result<usize>;

    fn flush(&mut self) -> Result<()>;

    fn write_all(&mut self, mut buf: &[u8]) -> Result<()> {
        while !buf.is_empty() {
            match self.write(buf) {
                Ok(0) => return Err(Error::new(ErrorKind::WriteZero)),
                Ok(n) => buf = &buf[n..],
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

/// Trait for cursor-based movement within a stream.
pub trait Seek {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64>;

    fn rewind(&mut self) -> Result<()> {
        self.seek(SeekFrom::Start(0)).map(|_| ())
    }

    fn stream_position(&mut self) -> Result<u64> {
        self.seek(SeekFrom::Current(0))
    }
}

/// Trait for buffered readers.
pub trait BufRead: Read {
    fn fill_buf(&mut self) -> Result<&[u8]>;

    fn consume(&mut self, amt: usize);

    fn read_until(&mut self, byte: u8, buf: &mut Vec<u8>) -> Result<usize> {
        let mut total = 0;
        loop {
            let available = self.fill_buf()?;
            if available.is_empty() {
                return Ok(total);
            }

            if let Some(idx) = available.iter().position(|&b| b == byte) {
                let end = idx + 1;
                buf.extend_from_slice(&available[..end]);
                self.consume(end);
                total += end;
                return Ok(total);
            } else {
                let len = available.len();
                buf.extend_from_slice(available);
                self.consume(len);
                total += len;
            }
        }
    }

    fn read_line(&mut self, buf: &mut String) -> Result<usize> {
        let start_len = buf.len();
        let mut bytes = Vec::new();
        let read = self.read_until(b'\n', &mut bytes)?;
        if read == 0 {
            return Ok(0);
        }
        let chunk = str::from_utf8(&bytes).map_err(|_| Error::new(ErrorKind::InvalidData))?;
        buf.push_str(chunk);
        Ok(buf.len() - start_len)
    }
}

/// Position selector used by `Seek`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SeekFrom {
    Start(u64),
    End(i64),
    Current(i64),
}

const DEFAULT_BUF_SIZE: usize = 8 * 1024;

pub struct BufReader<R> {
    inner: R,
    buf: Vec<u8>,
    pos: usize,
    cap: usize,
}

impl<R: Read> BufReader<R> {
    pub fn new(inner: R) -> Self {
        Self::with_capacity(DEFAULT_BUF_SIZE, inner)
    }

    pub fn with_capacity(capacity: usize, inner: R) -> Self {
        let capacity = capacity.max(1);
        let mut buf = Vec::with_capacity(capacity);
        buf.resize(capacity, 0);
        Self {
            inner,
            buf,
            pos: 0,
            cap: 0,
        }
    }

    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for BufReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let available = self.fill_buf()?;
        if available.is_empty() {
            return Ok(0);
        }
        let amt = cmp::min(buf.len(), available.len());
        buf[..amt].copy_from_slice(&available[..amt]);
        self.consume(amt);
        Ok(amt)
    }
}

impl<R: Read> BufRead for BufReader<R> {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        if self.pos >= self.cap {
            self.cap = self.inner.read(&mut self.buf)?;
            self.pos = 0;
        }
        Ok(&self.buf[self.pos..self.cap])
    }

    fn consume(&mut self, amt: usize) {
        self.pos = cmp::min(self.pos + amt, self.cap);
    }
}

pub struct BufWriter<W> {
    inner: W,
    buf: Vec<u8>,
    cap: usize,
}

impl<W: Write> BufWriter<W> {
    pub fn new(inner: W) -> Self {
        Self::with_capacity(DEFAULT_BUF_SIZE, inner)
    }

    pub fn with_capacity(capacity: usize, inner: W) -> Self {
        Self {
            inner,
            buf: Vec::with_capacity(capacity.max(1)),
            cap: capacity.max(1),
        }
    }

    fn flush_buf(&mut self) -> Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        self.inner.write_all(&self.buf)?;
        self.buf.clear();
        Ok(())
    }

    pub fn get_ref(&self) -> &W {
        &self.inner
    }

    pub fn get_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    pub fn into_inner(mut self) -> Result<W> {
        self.flush_buf()?;
        Ok(self.inner)
    }
}

impl<W: Write> Write for BufWriter<W> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        if buf.len() >= self.cap {
            self.flush_buf()?;
            return self.inner.write(buf);
        }

        if self.buf.len() + buf.len() > self.cap {
            self.flush_buf()?;
        }

        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<()> {
        self.flush_buf()?;
        self.inner.flush()
    }
}

pub struct LineWriter<W: Write> {
    inner: BufWriter<W>,
}

impl<W: Write> LineWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner: BufWriter::new(inner),
        }
    }

    fn flush_partial(&mut self) -> Result<()> {
        self.inner.flush()
    }

    pub fn into_inner(self) -> Result<W> {
        self.inner.into_inner()
    }
}

impl<W: Write> Write for LineWriter<W> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let mut start = 0;
        for idx in 0..buf.len() {
            if buf[idx] == b'\n' {
                self.inner.write_all(&buf[start..=idx])?;
                self.flush_partial()?;
                start = idx + 1;
            }
        }
        if start < buf.len() {
            self.inner.write_all(&buf[start..])?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

/// Reader that limits how many bytes can be read from the underlying source.
pub struct Take<R> {
    inner: R,
    limit: u64,
}

impl<R> Take<R> {
    pub fn into_inner(self) -> R {
        self.inner
    }

    pub fn limit(&self) -> u64 {
        self.limit
    }
}

impl<R: Read> Read for Take<R> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if self.limit == 0 {
            return Ok(0);
        }
        let max = cmp::min(buf.len() as u64, self.limit) as usize;
        let n = self.inner.read(&mut buf[..max])?;
        self.limit -= n as u64;
        Ok(n)
    }
}

impl<R: BufRead> BufRead for Take<R> {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        if self.limit == 0 {
            return Ok(&[]);
        }
        let buf = self.inner.fill_buf()?;
        let max = cmp::min(buf.len() as u64, self.limit) as usize;
        Ok(&buf[..max])
    }

    fn consume(&mut self, amt: usize) {
        let consumed = cmp::min(amt as u64, self.limit);
        self.limit -= consumed;
        self.inner.consume(consumed as usize);
    }
}

/// Cursor adaptor over in-memory buffers.
pub struct Cursor<T> {
    inner: T,
    pos: u64,
}

impl<T> Cursor<T> {
    pub fn new(inner: T) -> Self {
        Self { inner, pos: 0 }
    }

    pub fn into_inner(self) -> T {
        self.inner
    }

    pub fn position(&self) -> u64 {
        self.pos
    }

    pub fn set_position(&mut self, pos: u64) {
        self.pos = pos;
    }

    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: AsRef<[u8]>> Read for Cursor<T> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let data = self.inner.as_ref();
        let len = data.len();
        let pos = cmp::min(self.pos, len as u64) as usize;
        let n = cmp::min(buf.len(), data.len().saturating_sub(pos));
        if n == 0 {
            return Ok(0);
        }
        buf[..n].copy_from_slice(&data[pos..pos + n]);
        self.pos = self.pos.saturating_add(n as u64);
        Ok(n)
    }
}

impl<T: AsRef<[u8]>> Seek for Cursor<T> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        let len = self.inner.as_ref().len() as u64;
        let new = match pos {
            SeekFrom::Start(off) => off as i128,
            SeekFrom::End(off) => len as i128 + off as i128,
            SeekFrom::Current(off) => self.pos as i128 + off as i128,
        };
        if new < 0 {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let new_u128 = new as u128;
        if new_u128 > u64::MAX as u128 {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        self.pos = new_u128 as u64;
        Ok(self.pos)
    }
}

impl<'a> Write for Cursor<&'a mut [u8]> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let data: &mut [u8] = &mut *self.inner;
        let len = data.len();
        let pos = cmp::min(self.pos, len as u64) as usize;
        let n = cmp::min(buf.len(), data.len().saturating_sub(pos));
        if n == 0 {
            return Ok(0);
        }
        data[pos..pos + n].copy_from_slice(&buf[..n]);
        self.pos = self.pos.saturating_add(n as u64);
        Ok(n)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

impl Write for Cursor<Vec<u8>> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let max_pos = usize::MAX as u64;
        if self.pos > max_pos {
            return Err(Error::new(ErrorKind::InvalidInput));
        }

        let pos = self.pos as usize;
        if pos > self.inner.len() {
            self.inner.resize(pos, 0);
        }

        let end = match pos.checked_add(buf.len()) {
            Some(v) => v,
            None => return Err(Error::new(ErrorKind::InvalidInput)),
        };

        if end > self.inner.len() {
            self.inner.resize(end, 0);
        }

        self.inner[pos..end].copy_from_slice(buf);
        self.pos = end as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Reader that yields EOF immediately.
pub struct Empty;

impl Read for Empty {
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize> {
        Ok(0)
    }
}

impl BufRead for Empty {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        Ok(&[])
    }

    fn consume(&mut self, _amt: usize) {}
}

/// Writer that discards all bytes.
pub struct Sink;

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Reader that yields an infinite stream of the same byte.
pub struct Repeat {
    byte: u8,
}

impl Read for Repeat {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        buf.fill(self.byte);
        Ok(buf.len())
    }
}

pub const fn empty() -> Empty {
    Empty
}

pub const fn sink() -> Sink {
    Sink
}

pub const fn repeat(byte: u8) -> Repeat {
    Repeat { byte }
}

pub fn smoke_test() {
    crate::debugconf!("io: smoke_test begin\n");

    let manual_error = Error::new(ErrorKind::Other);
    crate::debugconf!("io: manual error kind={:?}\n", manual_error.kind());

    let mut cursor = Cursor::new(&b"FalseOS-io\nalpha-beta\nomega"[..]);

    let mut exact = [0u8; 10];
    match cursor.read_exact(&mut exact) {
        Ok(()) => {
            let snippet = match str::from_utf8(&exact) {
                Ok(s) => s,
                Err(_) => "<utf8 err>",
            };
            crate::debugconf!("io: read_exact='{}'\n", snippet);
        }
        Err(e) => crate::debugconf!("io: read_exact err={:?}\n", e.kind()),
    }

    let mut tail = Vec::new();
    match cursor.read_to_end(&mut tail) {
        Ok(n) => crate::debugconf!(
            "io: read_to_end bytes={} tail_last=0x{:02X}\n",
            n,
            tail.last().copied().unwrap_or(0)
        ),
        Err(e) => crate::debugconf!("io: read_to_end err={:?}\n", e.kind()),
    }

    match cursor.rewind() {
        Ok(()) => crate::debugconf!("io: rewind ok\n"),
        Err(e) => crate::debugconf!("io: rewind err={:?}\n", e.kind()),
    }

    match cursor.seek(SeekFrom::Current(4)) {
        Ok(pos) => crate::debugconf!("io: seek current->{}\n", pos),
        Err(e) => crate::debugconf!("io: seek current err={:?}\n", e.kind()),
    }

    match cursor.seek(SeekFrom::End(-5)) {
        Ok(pos) => crate::debugconf!("io: seek end-5->{}\n", pos),
        Err(e) => crate::debugconf!("io: seek end err={:?}\n", e.kind()),
    }

    match cursor.seek(SeekFrom::Start(0)) {
        Ok(pos) => crate::debugconf!("io: seek start->{}\n", pos),
        Err(e) => crate::debugconf!("io: seek start err={:?}\n", e.kind()),
    }

    match cursor.stream_position() {
        Ok(pos) => crate::debugconf!("io: stream_position={}\n", pos),
        Err(e) => crate::debugconf!("io: stream_position err={:?}\n", e.kind()),
    }

    let mut reader =
        BufReader::with_capacity(4, Cursor::new(&b"first line\nsecond-line\nthird"[..]));
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(n) => crate::debugconf!(
            "io: read_line bytes={} content='{}'\n",
            n,
            line.trim_end_matches('\n')
        ),
        Err(e) => crate::debugconf!("io: read_line err={:?}\n", e.kind()),
    }

    let mut until_dash = Vec::new();
    match reader.read_until(b'-', &mut until_dash) {
        Ok(n) => crate::debugconf!("io: read_until bytes={} data={:02X?}\n", n, until_dash),
        Err(e) => crate::debugconf!("io: read_until err={:?}\n", e.kind()),
    }

    let buf_cursor = {
        let mut writer = BufWriter::with_capacity(8, Cursor::new(Vec::new()));
        let _ = writer.write_all(b"buf");
        let _ = writer.write_all(b"-writer");
        let _ = writer.flush();
        match writer.into_inner() {
            Ok(inner) => inner,
            Err(e) => {
                crate::debugconf!("io: buf_writer into_inner err={:?}\n", e.kind());
                Cursor::new(Vec::new())
            }
        }
    };

    let writer_view = buf_cursor.get_ref();
    match str::from_utf8(writer_view) {
        Ok(text) => crate::debugconf!("io: buf_writer captured='{}'\n", text),
        Err(_) => crate::debugconf!(
            "io: buf_writer captured={} bytes (non-utf8)\n",
            writer_view.len()
        ),
    }

    let line_cursor = {
        let mut line_writer = LineWriter::new(Cursor::new(Vec::new()));
        let _ = line_writer.write_all(b"line A\n");
        let _ = line_writer.write_all(b"line B");
        let _ = line_writer.flush();
        match line_writer.into_inner() {
            Ok(inner) => inner,
            Err(e) => {
                crate::debugconf!("io: line_writer into_inner err={:?}\n", e.kind());
                Cursor::new(Vec::new())
            }
        }
    };

    match str::from_utf8(line_cursor.get_ref()) {
        Ok(text) => crate::debugconf!("io: line_writer captured='{}'\n", text),
        Err(_) => crate::debugconf!(
            "io: line_writer captured={} bytes (non-utf8)\n",
            line_cursor.get_ref().len()
        ),
    }

    let mut dropper = sink();
    let _ = dropper.write_all(b"bit bucket\n");
    let _ = dropper.flush();

    let mut repeater = repeat(0xA5);
    let mut repeated = [0u8; 6];
    match repeater.read_exact(&mut repeated) {
        Ok(()) => crate::debugconf!("io: repeat sample={:02X?}\n", repeated),
        Err(e) => crate::debugconf!("io: repeat err={:?}\n", e.kind()),
    }

    let mut void_reader = empty();
    let mut single = [0u8; 1];
    match void_reader.read_exact(&mut single) {
        Ok(()) => crate::debugconf!("io: empty unexpectedly produced data\n"),
        Err(e) => crate::debugconf!("io: empty read_exact err={:?}\n", e.kind()),
    }

    let mut limited = Cursor::new(&b"take-limited"[..]).take(4);
    let mut limited_buf = Vec::new();
    match limited.read_to_end(&mut limited_buf) {
        Ok(n) => crate::debugconf!(
            "io: take read {} bytes (remaining={})\n",
            n,
            limited.limit()
        ),
        Err(e) => crate::debugconf!("io: take read err={:?}\n", e.kind()),
    }
    let _ = limited.into_inner();

    crate::debugconf!("io: smoke_test end\n");
}

pub mod core2 {
    use super::*;
    use ::core2::io as c2;

    const SURFACE_ERROR_MSG: &str = "surface io error";

    #[inline]
    fn to_core2_error_kind(kind: ErrorKind) -> c2::ErrorKind {
        match kind {
            ErrorKind::UnexpectedEof => c2::ErrorKind::UnexpectedEof,
            ErrorKind::WriteZero => c2::ErrorKind::WriteZero,
            ErrorKind::WouldBlock => c2::ErrorKind::WouldBlock,
            ErrorKind::InvalidInput => c2::ErrorKind::InvalidInput,
            ErrorKind::InvalidData => c2::ErrorKind::InvalidData,
            ErrorKind::Interrupted => c2::ErrorKind::Interrupted,
            ErrorKind::Other => c2::ErrorKind::Other,
        }
    }

    #[inline]
    fn from_core2_error_kind(kind: c2::ErrorKind) -> ErrorKind {
        match kind {
            c2::ErrorKind::UnexpectedEof => ErrorKind::UnexpectedEof,
            c2::ErrorKind::WriteZero => ErrorKind::WriteZero,
            c2::ErrorKind::WouldBlock => ErrorKind::WouldBlock,
            c2::ErrorKind::InvalidInput => ErrorKind::InvalidInput,
            c2::ErrorKind::InvalidData => ErrorKind::InvalidData,
            c2::ErrorKind::Interrupted => ErrorKind::Interrupted,
            _ => ErrorKind::Other,
        }
    }

    #[inline]
    fn to_core2_error(err: Error) -> c2::Error {
        c2::Error::new(to_core2_error_kind(err.kind()), SURFACE_ERROR_MSG)
    }

    #[inline]
    fn from_core2_error(err: c2::Error) -> Error {
        Error::new(from_core2_error_kind(err.kind()))
    }

    #[inline]
    fn to_core2_seek_from(from: SeekFrom) -> c2::SeekFrom {
        match from {
            SeekFrom::Start(v) => c2::SeekFrom::Start(v),
            SeekFrom::End(v) => c2::SeekFrom::End(v),
            SeekFrom::Current(v) => c2::SeekFrom::Current(v),
        }
    }

    #[inline]
    fn from_core2_seek_from(from: c2::SeekFrom) -> SeekFrom {
        match from {
            c2::SeekFrom::Start(v) => SeekFrom::Start(v),
            c2::SeekFrom::End(v) => SeekFrom::End(v),
            c2::SeekFrom::Current(v) => SeekFrom::Current(v),
        }
    }

    pub struct ToCore2<T>(pub T);

    impl<T> ToCore2<T> {
        pub fn into_inner(self) -> T {
            self.0
        }
    }

    impl<T: Read> c2::Read for ToCore2<T> {
        fn read(&mut self, buf: &mut [u8]) -> c2::Result<usize> {
            self.0.read(buf).map_err(to_core2_error)
        }
    }

    impl<T: Write> c2::Write for ToCore2<T> {
        fn write(&mut self, buf: &[u8]) -> c2::Result<usize> {
            self.0.write(buf).map_err(to_core2_error)
        }

        fn flush(&mut self) -> c2::Result<()> {
            self.0.flush().map_err(to_core2_error)
        }
    }

    impl<T: Seek> c2::Seek for ToCore2<T> {
        fn seek(&mut self, pos: c2::SeekFrom) -> c2::Result<u64> {
            self.0
                .seek(from_core2_seek_from(pos))
                .map_err(to_core2_error)
        }
    }

    impl<T: BufRead> c2::BufRead for ToCore2<T> {
        fn fill_buf(&mut self) -> c2::Result<&[u8]> {
            self.0.fill_buf().map_err(to_core2_error)
        }

        fn consume(&mut self, amt: usize) {
            self.0.consume(amt);
        }
    }

    pub struct FromCore2<T>(pub T);

    impl<T> FromCore2<T> {
        pub fn into_inner(self) -> T {
            self.0
        }
    }

    impl<T: c2::Read> Read for FromCore2<T> {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
            self.0.read(buf).map_err(from_core2_error)
        }
    }

    impl<T: c2::Write> Write for FromCore2<T> {
        fn write(&mut self, buf: &[u8]) -> Result<usize> {
            self.0.write(buf).map_err(from_core2_error)
        }

        fn flush(&mut self) -> Result<()> {
            self.0.flush().map_err(from_core2_error)
        }
    }

    impl<T: c2::Seek> Seek for FromCore2<T> {
        fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
            self.0
                .seek(to_core2_seek_from(pos))
                .map_err(from_core2_error)
        }
    }

    impl<T: c2::BufRead> BufRead for FromCore2<T> {
        fn fill_buf(&mut self) -> Result<&[u8]> {
            self.0.fill_buf().map_err(from_core2_error)
        }

        fn consume(&mut self, amt: usize) {
            self.0.consume(amt);
        }
    }
}
