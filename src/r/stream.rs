extern crate alloc;

use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt;

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::pipe::{Pipe, TryReadError, TryWriteError};
use embedded_io_async::{ErrorKind, ErrorType, Read, Seek, Write};

const TRUEOSFS_FORWARD_PIPE_BYTES: usize = 64 * 1024;
const TRUEOSFS_FORWARD_READ_FALLBACK_BYTES: usize = 256 * 1024;

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
            return Err(HvStreamError::Backend(HvStreamError::backend_code(
                crate::disc::block::Error::Io,
            )));
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

fn trueosfs_forward_read_chunk_bytes(info: &crate::disc::block::DeviceInfo) -> usize {
    let block_size = usize::max(info.block_size as usize, 1);
    let raw = if info.max_transfer_bytes > 0 {
        info.max_transfer_bytes as usize
    } else {
        TRUEOSFS_FORWARD_READ_FALLBACK_BYTES
    };
    let aligned = raw - (raw % block_size);
    usize::max(aligned, block_size)
}

fn drain_pipe_to_vec<const N: usize>(
    pipe: &Pipe<NoopRawMutex, N>,
    out: &mut Vec<u8>,
    mut buf: &mut [u8],
) -> usize {
    let mut drained = 0usize;
    loop {
        match pipe.try_read(&mut *buf) {
            Ok(n) => {
                if n == 0 {
                    break;
                }
                out.extend_from_slice(&buf[..n]);
                drained = drained.saturating_add(n);
            }
            Err(TryReadError::Empty) => break,
        }
    }
    drained
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

    let mut out = Vec::with_capacity(len);
    let mut src = vec![0u8; trueosfs_forward_read_chunk_bytes(&disk.info())];
    let mut drain = vec![0u8; TRUEOSFS_FORWARD_PIPE_BYTES];
    let pipe = Pipe::<NoopRawMutex, TRUEOSFS_FORWARD_PIPE_BYTES>::new();

    let mut cursor = offset;
    let mut remaining = len;
    while remaining != 0 {
        let read_cap = usize::min(src.len(), remaining);
        let Some(read) = crate::r::fs::trueosfs::file_read_range_async(
            disk,
            key,
            cursor,
            &mut src[..read_cap],
        )
        .await?
        else {
            return Ok(None);
        };
        if read == 0 {
            break;
        }

        let mut written = 0usize;
        while written < read {
            match pipe.try_write(&src[written..read]) {
                Ok(n) => {
                    written = written.saturating_add(n);
                }
                Err(TryWriteError::Full) => {
                    let drained = drain_pipe_to_vec(&pipe, &mut out, drain.as_mut_slice());
                    if drained == 0 {
                        return Err(crate::disc::block::Error::NotReady);
                    }
                }
            }
        }

        drain_pipe_to_vec(&pipe, &mut out, drain.as_mut_slice());
        cursor = cursor.saturating_add(read as u64);
        remaining = remaining.saturating_sub(read);
    }

    drain_pipe_to_vec(&pipe, &mut out, drain.as_mut_slice());
    if out.len() != len {
        return Err(crate::disc::block::Error::OutOfBounds);
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
