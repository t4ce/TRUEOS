use super::Status;

#[derive(Debug)]
pub enum SocksV4Error {
    IpV6,
    Command(Status),
}

impl From<Status> for SocksV4Error {
    fn from(err: Status) -> Self {
        Self::Command(err)
    }
}

impl core::fmt::Display for SocksV4Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::IpV6 => f.write_str("IPV6 is not supported"),
            Self::Command(status) => status.fmt(f),
        }
    }
}
