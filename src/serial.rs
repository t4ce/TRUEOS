use alloc::boxed::Box;
use core::cmp;
use core::future::Future;
use core::pin::Pin;

/// Maximum length for a device serial number (bytes).
pub const SERIAL_NUMBER_MAX: usize = 64;

/// Simple serial number container for identifying endpoints across buses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SerialNumber {
    len: u8,
    bytes: [u8; SERIAL_NUMBER_MAX],
}

impl SerialNumber {
    pub const fn none() -> Self {
        Self {
            len: 0,
            bytes: [0; SERIAL_NUMBER_MAX],
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut out = Self::none();
        let n = cmp::min(bytes.len(), SERIAL_NUMBER_MAX);
        out.bytes[..n].copy_from_slice(&bytes[..n]);
        out.len = n as u8;
        out
    }

    pub fn from_str(s: &str) -> Self {
        Self::from_bytes(s.as_bytes())
    }

    pub fn is_some(&self) -> bool {
        self.len != 0
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..(self.len as usize)]
    }
}

/// Minimal async serial interface that can be shared across transports.
pub trait SerialPort {
    /// Best-effort immediate write; returns bytes accepted.
    fn write(&self, data: &[u8]) -> usize;

    /// Write the full buffer, waiting for backpressure to clear.
    fn write_all<'a>(&'a self, data: &'a [u8]) -> Pin<Box<dyn Future<Output = usize> + 'a>>;

    /// Optional stable serial number for the endpoint.
    fn serial_number(&self) -> Option<SerialNumber>;
}
