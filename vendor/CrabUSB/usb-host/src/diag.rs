//! Live xHCI diagnostic access.
//!
//! This surface deliberately stays below USB device/class policy.  A kernel can
//! serialize requests through the task that owns [`crate::USBHost`] and use the
//! same mapped register aperture as the running controller.

use alloc::vec::Vec;
use core::fmt::{self, Write};

const ENUMERATION_FAILURE_REASON_CAPACITY: usize = 256;

/// Bounded evidence retained by one controller owner across later probes.
///
/// A successful empty discovery pass must not erase a failure from an earlier
/// batch: the port may already have admitted a quarantined slot and DMA owner.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XhciEnumerationDiagnostics {
    pub failure_count: u64,
    pub first_failure: Option<XhciEnumerationFailure>,
    pub last_failure: Option<XhciEnumerationFailure>,
}

impl XhciEnumerationDiagnostics {
    pub fn record_failure(
        &mut self,
        root_port_id: u8,
        port_id: u8,
        slot_id: Option<u8>,
        stage: &'static str,
        reason: impl fmt::Display,
    ) {
        let mut failure = XhciEnumerationFailure {
            root_port_id,
            port_id,
            slot_id,
            stage,
            reason_truncated: false,
            reason_len: 0,
            reason_bytes: [0; ENUMERATION_FAILURE_REASON_CAPACITY],
        };
        let _ = write!(failure, "{reason}");
        self.failure_count = self.failure_count.saturating_add(1);
        if self.first_failure.is_none() {
            self.first_failure = Some(failure.clone());
        }
        self.last_failure = Some(failure);
    }
}

/// Latest failed enumeration operation; no device or DMA ownership is changed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XhciEnumerationFailure {
    pub root_port_id: u8,
    pub port_id: u8,
    pub slot_id: Option<u8>,
    pub stage: &'static str,
    pub reason_truncated: bool,
    reason_len: usize,
    reason_bytes: [u8; ENUMERATION_FAILURE_REASON_CAPACITY],
}

impl XhciEnumerationFailure {
    pub fn reason(&self) -> &str {
        // write_str only copies complete UTF-8 prefixes into this buffer.
        core::str::from_utf8(&self.reason_bytes[..self.reason_len])
            .expect("enumeration failure reason is UTF-8")
    }
}

impl Write for XhciEnumerationFailure {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if self.reason_truncated {
            return Ok(());
        }
        let available = ENUMERATION_FAILURE_REASON_CAPACITY - self.reason_len;
        let mut length = value.len().min(available);
        while !value.is_char_boundary(length) {
            length -= 1;
        }
        self.reason_bytes[self.reason_len..self.reason_len + length]
            .copy_from_slice(&value.as_bytes()[..length]);
        self.reason_len += length;
        self.reason_truncated = length != value.len();
        Ok(())
    }
}

/// One Supported Protocol extended-capability record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XhciProtocolSnapshot {
    pub major: u8,
    pub minor: u8,
    pub name: u32,
    pub port_offset: u8,
    pub port_count: u8,
    pub slot_type: u8,
    pub psi_count: u8,
}

/// One physical xHCI port-register set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XhciPortSnapshot {
    pub port_id: u8,
    pub portsc: u32,
    pub portpmsc: u32,
    pub portli: u32,
    pub porthlpmc: u32,
}

/// Coherent read-only census of the live xHCI register aperture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XhciControllerSnapshot {
    pub enumeration: XhciEnumerationDiagnostics,
    pub mmio_len: usize,
    pub caplength: u8,
    pub hciversion: u16,
    pub hcsparams1: u32,
    pub hcsparams2: u32,
    pub hcsparams3: u32,
    pub hccparams1: u32,
    pub hccparams2: u32,
    pub dboff: u32,
    pub rtsoff: u32,
    pub usbcmd: u32,
    pub usbsts: u32,
    pub pagesize: u32,
    pub dnctrl: u32,
    pub crcr: u64,
    pub dcbaap: u64,
    pub config: u32,
    pub mfindex: u32,
    pub iman: u32,
    pub imod: u32,
    pub erstsz: u32,
    pub erstba: u64,
    pub erdp: u64,
    pub protocols: Vec<XhciProtocolSnapshot>,
    pub ports: Vec<XhciPortSnapshot>,
}

/// A direct register operation performed by the live controller owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XhciDirectRequest {
    Snapshot,
    Read32 {
        offset: usize,
    },
    Read64 {
        offset: usize,
    },
    Write32 {
        offset: usize,
        value: u32,
    },
    Write64 {
        offset: usize,
        value: u64,
    },
    ReadModifyWrite32 {
        offset: usize,
        clear_mask: u32,
        set_mask: u32,
    },
}

/// Result of one direct register mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XhciWriteResult {
    pub offset: usize,
    pub before: u32,
    pub requested: u32,
    pub after: u32,
}

/// Result of one direct 64-bit register mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XhciWrite64Result {
    pub offset: usize,
    pub before: u64,
    pub requested: u64,
    pub after: u64,
}

/// Result returned by [`XhciDirectRequest`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XhciDirectResponse {
    Snapshot(XhciControllerSnapshot),
    Read32 { offset: usize, value: u32 },
    Read64 { offset: usize, value: u64 },
    Write32(XhciWriteResult),
    Write64(XhciWrite64Result),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumeration_failures_retain_first_and_latest_per_controller() {
        let mut diagnostic = XhciEnumerationDiagnostics::default();
        let other_controller = XhciEnumerationDiagnostics::default();
        diagnostic.record_failure(2, 2, Some(1), "address-device", "UsbTransactionError");
        diagnostic.record_failure(6, 3, Some(4), "configure-hub", "StallError");
        assert_eq!(diagnostic.failure_count, 2);
        let first = diagnostic.first_failure.as_ref().unwrap();
        assert_eq!(
            (first.root_port_id, first.port_id, first.slot_id),
            (2, 2, Some(1))
        );
        assert_eq!(first.stage, "address-device");
        assert_eq!(first.reason(), "UsbTransactionError");
        let latest = diagnostic.last_failure.as_ref().unwrap();
        assert_eq!(
            (latest.root_port_id, latest.port_id, latest.slot_id),
            (6, 3, Some(4))
        );
        assert_eq!(latest.stage, "configure-hub");
        assert_eq!(latest.reason(), "StallError");
        assert_eq!(other_controller.failure_count, 0);
        assert!(other_controller.first_failure.is_none());
        assert!(other_controller.last_failure.is_none());
        assert_eq!(diagnostic.clone(), diagnostic);
    }

    #[test]
    fn enumeration_failure_reason_is_bounded_valid_utf8_and_count_saturates() {
        let mut diagnostic = XhciEnumerationDiagnostics::default();
        diagnostic.failure_count = u64::MAX;
        let prefix = "x".repeat(ENUMERATION_FAILURE_REASON_CAPACITY - 1);
        diagnostic.record_failure(1, 4, None, "enable-slot", format_args!("{prefix}é ignored"));
        let latest = diagnostic.last_failure.as_ref().unwrap();
        assert_eq!(latest.reason(), prefix);
        assert!(latest.reason_truncated);
        assert_eq!(diagnostic.failure_count, u64::MAX);
        diagnostic.record_failure(1, 4, Some(1), "get-device-descriptor", "ok length");
        let latest = diagnostic.last_failure.as_ref().unwrap();
        assert_eq!(latest.reason(), "ok length");
        assert!(!latest.reason_truncated);
        assert!(diagnostic.first_failure.as_ref().unwrap().reason_truncated);
    }
}
