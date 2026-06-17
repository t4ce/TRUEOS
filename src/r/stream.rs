#![allow(dead_code)]

extern crate alloc;

use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt;

use embedded_io_async::{ErrorKind, ErrorType, Read, Seek, SeekFrom, Write};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HvObjectDesc<'a> {
    pub key: &'a str,
    pub total_len_hint: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HvObjectCommit {
    pub key_len: usize,
    pub bytes_written: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HvObjectOpen {
    pub total_len: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HvPull {
    Chunk { len: usize },
    End,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HvStreamError {
    InvalidState,
    NotOpen,
    AlreadyOpen,
    EmptyKey,
    NoSpace,
    LengthOverflow,
    Truncated,
    Backend(i32),
}

impl HvStreamError {
    const fn backend_code(err: crate::disc::block::Error) -> i32 {
        match err {
            crate::disc::block::Error::NotSupported => 1,
            crate::disc::block::Error::NotReady => 2,
            crate::disc::block::Error::InvalidParam => 3,
            crate::disc::block::Error::OutOfBounds => 4,
            crate::disc::block::Error::DmaUnavailable => 5,
            crate::disc::block::Error::MmioMapFailed => 6,
            crate::disc::block::Error::Timeout => 7,
            crate::disc::block::Error::Io => 8,
            crate::disc::block::Error::Corrupted => 9,
        }
    }
}

impl From<crate::disc::block::Error> for HvStreamError {
    fn from(value: crate::disc::block::Error) -> Self {
        Self::Backend(Self::backend_code(value))
    }
}

impl fmt::Display for HvStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HvStreamError::InvalidState => write!(f, "invalid stream state"),
            HvStreamError::NotOpen => write!(f, "stream not open"),
            HvStreamError::AlreadyOpen => write!(f, "stream already open"),
            HvStreamError::EmptyKey => write!(f, "empty object key"),
            HvStreamError::NoSpace => write!(f, "no space for object"),
            HvStreamError::LengthOverflow => write!(f, "object length overflow"),
            HvStreamError::Truncated => write!(f, "stream truncated"),
            HvStreamError::Backend(code) => write!(f, "backend error code {}", code),
        }
    }
}

impl core::error::Error for HvStreamError {}

impl embedded_io_async::Error for HvStreamError {
    fn kind(&self) -> ErrorKind {
        match self {
            HvStreamError::InvalidState
            | HvStreamError::EmptyKey
            | HvStreamError::NoSpace
            | HvStreamError::LengthOverflow => ErrorKind::InvalidInput,
            HvStreamError::NotOpen | HvStreamError::AlreadyOpen => ErrorKind::Other,
            HvStreamError::Truncated => ErrorKind::InvalidData,
            HvStreamError::Backend(_) => ErrorKind::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HvWriteState<'a> {
    Idle,
    Writing {
        desc: HvObjectDesc<'a>,
        bytes_written: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HvReadState {
    Idle,
    Reading {
        total_len: Option<u64>,
        bytes_read: u64,
    },
}

pub type Error = HvStreamError;
pub type ObjectDesc<'a> = HvObjectDesc<'a>;
pub type ObjectCommit = HvObjectCommit;

#[derive(Debug)]
pub struct ObjectOpen<R> {
    pub reader: R,
    pub total_len: Option<u64>,
}

impl<R> ObjectOpen<R> {
    pub const fn new(reader: R, total_len: Option<u64>) -> Self {
        Self { reader, total_len }
    }

    pub fn into_parts(self) -> (R, Option<u64>) {
        (self.reader, self.total_len)
    }

    pub const fn desc_for_key<'a>(&self, key: &'a str) -> ObjectDesc<'a> {
        ObjectDesc {
            key,
            total_len_hint: self.total_len,
        }
    }
}

pub trait ObjectReader: Read {}

impl<T> ObjectReader for T where T: Read + ?Sized {}

pub trait SeekableObjectReader: ObjectReader + Seek {}

impl<T> SeekableObjectReader for T where T: ObjectReader + Seek + ?Sized {}

pub trait ObjectWriter: Write {}

impl<T> ObjectWriter for T where T: Write + ?Sized {}

pub trait HvObjectSink {
    fn write_state(&self) -> HvWriteState<'_>;

    fn begin_object(&mut self, desc: HvObjectDesc<'_>) -> Result<(), HvStreamError>;

    fn push_chunk(&mut self, bytes: &[u8]) -> Result<(), HvStreamError>;

    fn finish_object(&mut self) -> Result<HvObjectCommit, HvStreamError>;

    fn abort_object(&mut self) -> Result<(), HvStreamError>;
}

pub trait HvObjectSource {
    fn read_state(&self) -> HvReadState;

    fn open_object(&mut self, key: &str) -> Result<HvObjectOpen, HvStreamError>;

    fn pull_chunk(&mut self, out: &mut [u8]) -> Result<HvPull, HvStreamError>;

    fn close_object(&mut self) -> Result<(), HvStreamError>;
}

#[allow(async_fn_in_trait)]
pub trait ObjectSource {
    type Reader: ObjectReader;

    async fn open(&mut self, key: &str) -> Result<ObjectOpen<Self::Reader>, Error>;
}

pub trait SeekableObjectSource: ObjectSource
where
    Self::Reader: SeekableObjectReader,
{
}

impl<T> SeekableObjectSource for T
where
    T: ObjectSource,
    T::Reader: SeekableObjectReader,
{
}

#[allow(async_fn_in_trait)]
pub trait ObjectSink {
    type Writer: ObjectWriter;

    async fn begin(&mut self, desc: ObjectDesc<'_>) -> Result<Self::Writer, Error>;

    async fn commit(&mut self) -> Result<(), Error>;

    async fn abort(&mut self) -> Result<(), Error>;
}

struct TrueosFsWriteSession {
    handle: u32,
    bytes_written: u64,
}

pub struct TrueosFsObjectReader {
    disk: crate::disc::block::DeviceHandle,
    key: String,
    total_len: u64,
    offset: u64,
    closed: bool,
}

impl TrueosFsObjectReader {
    pub async fn open(
        disk: crate::disc::block::DeviceHandle,
        key: &str,
    ) -> Result<Option<Self>, crate::disc::block::Error> {
        if key.is_empty() {
            return Err(crate::disc::block::Error::InvalidParam);
        }
        let Some(info) = crate::r::fs::trueosfs::file_info_async(disk, key).await? else {
            return Ok(None);
        };
        Ok(Some(Self {
            disk,
            key: key.to_string(),
            total_len: info.data_len,
            offset: 0,
            closed: false,
        }))
    }

    #[inline]
    pub const fn total_len(&self) -> u64 {
        self.total_len
    }

    #[inline]
    pub const fn position(&self) -> u64 {
        self.offset
    }

    pub async fn read_exact_at(
        &mut self,
        offset: u64,
        dst: &mut [u8],
    ) -> Result<bool, crate::disc::block::Error> {
        if self.closed {
            return Err(crate::disc::block::Error::NotReady);
        }
        if dst.is_empty() {
            self.offset = offset;
            return Ok(true);
        }
        let end = offset
            .checked_add(dst.len() as u64)
            .ok_or(crate::disc::block::Error::OutOfBounds)?;
        if end > self.total_len {
            return Err(crate::disc::block::Error::OutOfBounds);
        }

        let Some(read) = crate::r::fs::trueosfs::file_read_range_async(
            self.disk,
            self.key.as_str(),
            offset,
            dst,
        )
        .await?
        else {
            return Ok(false);
        };
        if read != dst.len() {
            return Err(crate::disc::block::Error::OutOfBounds);
        }
        self.offset = end;
        Ok(true)
    }

    pub fn close(&mut self) -> Result<(), HvStreamError> {
        if self.closed {
            return Err(HvStreamError::NotOpen);
        }
        self.closed = true;
        Ok(())
    }

    fn seek_position(&self, pos: SeekFrom) -> Result<u64, HvStreamError> {
        let next = match pos {
            SeekFrom::Start(offset) => offset as i128,
            SeekFrom::End(delta) => self.total_len as i128 + delta as i128,
            SeekFrom::Current(delta) => self.offset as i128 + delta as i128,
        };
        if next < 0 || next > u64::MAX as i128 {
            return Err(HvStreamError::InvalidState);
        }
        Ok(next as u64)
    }
}

impl ErrorType for TrueosFsObjectReader {
    type Error = HvStreamError;
}

impl Read for TrueosFsObjectReader {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if self.closed {
            return Err(HvStreamError::NotOpen);
        }
        if buf.is_empty() {
            return Ok(0);
        }
        if self.offset >= self.total_len {
            return Ok(0);
        }

        let remaining = self.total_len.saturating_sub(self.offset);
        let want = core::cmp::min(buf.len(), remaining as usize);
        let Some(read) = crate::r::fs::trueosfs::file_read_range_async(
            self.disk,
            self.key.as_str(),
            self.offset,
            &mut buf[..want],
        )
        .await
        .map_err(HvStreamError::from)?
        else {
            return Err(HvStreamError::Backend(HvStreamError::backend_code(
                crate::disc::block::Error::NotReady,
            )));
        };
        self.offset = self.offset.saturating_add(read as u64);
        Ok(read)
    }
}

impl Seek for TrueosFsObjectReader {
    async fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        if self.closed {
            return Err(HvStreamError::NotOpen);
        }
        let next = self.seek_position(pos)?;
        self.offset = next;
        Ok(next)
    }
}

pub struct TrueosFsObjectSource {
    disk: crate::disc::block::DeviceHandle,
}

impl TrueosFsObjectSource {
    pub const fn new(disk: crate::disc::block::DeviceHandle) -> Self {
        Self { disk }
    }
}

impl ObjectSource for TrueosFsObjectSource {
    type Reader = TrueosFsObjectReader;

    async fn open(&mut self, key: &str) -> Result<ObjectOpen<Self::Reader>, Error> {
        let Some(reader) = TrueosFsObjectReader::open(self.disk, key)
            .await
            .map_err(HvStreamError::from)?
        else {
            return Err(HvStreamError::NotOpen);
        };
        let total_len = Some(reader.total_len());
        Ok(ObjectOpen::new(reader, total_len))
    }
}

pub struct TrueosFsObjectWriter {
    session: Rc<RefCell<TrueosFsWriteSession>>,
}

impl ErrorType for TrueosFsObjectWriter {
    type Error = HvStreamError;
}

impl Write for TrueosFsObjectWriter {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }

        let handle = self.session.borrow().handle;
        crate::r::fs::trueosfs::file_write_chunk_async(handle, buf)
            .await
            .map_err(HvStreamError::from)?;

        let mut session = self.session.borrow_mut();
        session.bytes_written = session.bytes_written.saturating_add(buf.len() as u64);
        Ok(buf.len())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

pub struct TrueosFsObjectSink {
    disk: crate::disc::block::DeviceHandle,
    active: Option<Rc<RefCell<TrueosFsWriteSession>>>,
}

impl TrueosFsObjectSink {
    pub const fn new(disk: crate::disc::block::DeviceHandle) -> Self {
        Self { disk, active: None }
    }
}

impl ObjectSink for TrueosFsObjectSink {
    type Writer = TrueosFsObjectWriter;

    async fn begin(&mut self, desc: ObjectDesc<'_>) -> Result<Self::Writer, Error> {
        if self.active.is_some() {
            return Err(HvStreamError::AlreadyOpen);
        }
        if desc.key.is_empty() {
            return Err(HvStreamError::EmptyKey);
        }

        let total_len = desc.total_len_hint.ok_or(HvStreamError::InvalidState)?;
        let Some(handle) =
            crate::r::fs::trueosfs::file_write_begin_async(self.disk, desc.key, total_len)
                .await
                .map_err(HvStreamError::from)?
        else {
            return Err(HvStreamError::NoSpace);
        };

        let session = Rc::new(RefCell::new(TrueosFsWriteSession {
            handle,
            bytes_written: 0,
        }));
        self.active = Some(session.clone());
        Ok(TrueosFsObjectWriter { session })
    }

    async fn commit(&mut self) -> Result<(), Error> {
        let Some(session) = self.active.take() else {
            return Err(HvStreamError::NotOpen);
        };
        let handle = session.borrow().handle;
        crate::r::fs::trueosfs::file_write_finish_async(handle)
            .await
            .map_err(HvStreamError::from)
    }

    async fn abort(&mut self) -> Result<(), Error> {
        let Some(session) = self.active.take() else {
            return Err(HvStreamError::NotOpen);
        };
        let handle = session.borrow().handle;
        crate::r::fs::trueosfs::file_write_abort_async(handle)
            .await
            .map_err(HvStreamError::from)
    }
}

pub async fn read_trueosfs_file_range_into_async(
    disk: crate::disc::block::DeviceHandle,
    key: &str,
    offset: u64,
    dst: &mut [u8],
) -> Result<bool, crate::disc::block::Error> {
    if dst.is_empty() {
        return Ok(true);
    }

    let Some(read) = crate::r::fs::trueosfs::file_read_range_async(disk, key, offset, dst).await?
    else {
        return Ok(false);
    };
    if read != dst.len() {
        return Err(crate::disc::block::Error::OutOfBounds);
    }
    Ok(true)
}

pub async fn read_trueosfs_file_range_into_logged_async(
    disk: crate::disc::block::DeviceHandle,
    key: &str,
    offset: u64,
    dst: &mut [u8],
    log_label: &str,
) -> Result<bool, crate::disc::block::Error> {
    if dst.is_empty() {
        return Ok(true);
    }

    crate::log!("{} read start path={} offset={} bytes={}\n", log_label, key, offset, dst.len());
    let start_ms = logged_read_now_ms();
    let mut read = 0usize;
    let mut last_log_ms = start_ms;
    let mut last_log_bytes = 0usize;
    while read < dst.len() {
        let chunk_len = core::cmp::min(256 * 1024, dst.len().saturating_sub(read));
        let chunk_offset = offset
            .checked_add(read as u64)
            .ok_or(crate::disc::block::Error::OutOfBounds)?;
        if crate::logflag::STORAGE_TRACE_LOGS {
            crate::log!(
                "{} read chunk start path={} offset={} chunk={} total_done={} total_need={}\n",
                log_label,
                key,
                chunk_offset,
                chunk_len,
                read,
                dst.len()
            );
        }
        let Some(got) = crate::r::fs::trueosfs::file_read_range_async(
            disk,
            key,
            chunk_offset,
            &mut dst[read..read + chunk_len],
        )
        .await?
        else {
            return Ok(false);
        };
        if got != chunk_len {
            crate::log!(
                "{} read short path={} offset={} got={} need={} total_done={} total_need={}\n",
                log_label,
                key,
                chunk_offset,
                got,
                chunk_len,
                read.saturating_add(got),
                dst.len()
            );
            return Err(crate::disc::block::Error::OutOfBounds);
        }
        read = read.saturating_add(got);

        if crate::logflag::STORAGE_TRACE_LOGS {
            crate::log!(
                "{} read chunk done path={} offset={} got={} total_done={} total_need={}\n",
                log_label,
                key,
                chunk_offset,
                got,
                read,
                dst.len()
            );
            let now_ms = logged_read_now_ms();
            let advanced = read.saturating_sub(last_log_bytes);
            if read == dst.len()
                || advanced >= 512 * 1024
                || now_ms.saturating_sub(last_log_ms) >= 1000
            {
                let pct_x10 = read.saturating_mul(1000) / dst.len();
                crate::log!(
                    "{} read progress path={} offset={} done={} total={} pct={}.{} elapsed_ms={}\n",
                    log_label,
                    key,
                    offset,
                    read,
                    dst.len(),
                    pct_x10 / 10,
                    pct_x10 % 10,
                    now_ms.saturating_sub(start_ms)
                );
                last_log_ms = now_ms;
                last_log_bytes = read;
            }
        }
    }
    crate::log!(
        "{} read done path={} offset={} got={} need={}\n",
        log_label,
        key,
        offset,
        read,
        dst.len()
    );
    if read != dst.len() {
        return Err(crate::disc::block::Error::OutOfBounds);
    }
    Ok(true)
}

fn logged_read_now_ms() -> u64 {
    let ticks = embassy_time_driver::now();
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        ticks.saturating_mul(1000) / hz
    }
}

pub async fn read_trueosfs_file_range_via_pipe_async(
    disk: crate::disc::block::DeviceHandle,
    key: &str,
    offset: u64,
    len: usize,
) -> Result<Option<Vec<u8>>, crate::disc::block::Error> {
    if len == 0 {
        return Ok(Some(Vec::new()));
    }

    let mut out = vec![0u8; len];
    if !read_trueosfs_file_range_into_async(disk, key, offset, out.as_mut_slice()).await? {
        return Ok(None);
    }
    Ok(Some(out))
}

pub async fn load_trueosfs_file_via_pipe_async(
    disk: crate::disc::block::DeviceHandle,
    key: &str,
) -> Result<Option<Vec<u8>>, crate::disc::block::Error> {
    let Some(info) = crate::r::fs::trueosfs::file_info_async(disk, key).await? else {
        return Ok(None);
    };

    let total_len =
        usize::try_from(info.data_len).map_err(|_| crate::disc::block::Error::OutOfBounds)?;
    read_trueosfs_file_range_via_pipe_async(disk, key, 0, total_len).await
}
