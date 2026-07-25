//! Live xHCI diagnostic access.
//!
//! This surface deliberately stays below USB device/class policy.  A kernel can
//! serialize requests through the task that owns [`crate::USBHost`] and use the
//! same mapped register aperture as the running controller.

use alloc::vec::Vec;

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
    Write32 {
        offset: usize,
        value: u32,
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

/// Result returned by [`XhciDirectRequest`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XhciDirectResponse {
    Snapshot(XhciControllerSnapshot),
    Read32 {
        offset: usize,
        value: u32,
    },
    Write32(XhciWriteResult),
}
