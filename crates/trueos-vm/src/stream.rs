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
