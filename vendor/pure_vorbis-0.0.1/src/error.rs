pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Undecodable(&'static str),
    WrongPacketKind(&'static str),
    ExpectedEof(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ErrorKind {
    Undecodable,
    WrongPacketKind,
    ExpectedEof,
}

impl Error {
    pub fn kind(&self) -> ErrorKind {
        match self {
            &Error::Undecodable(_)      => ErrorKind::Undecodable,
            &Error::ExpectedEof(_)      => ErrorKind::ExpectedEof,
            &Error::WrongPacketKind(_)  => ErrorKind::WrongPacketKind,
        }
    }
}

pub trait ExpectEof<T> {
    fn expect_eof(self) -> Result<T>;
}

impl<T> ExpectEof<T> for Result<T> {
    fn expect_eof(self) -> Result<T> {
        match self {
            Err(Error::Undecodable("Unexpected EOF")) => Err(Error::ExpectedEof("Expected EOF")),
            v => v,
        }
    }
}
