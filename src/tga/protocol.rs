//! Small, frozen BAR0 contract retained for the dormant TGA board.
//!
//! This is intentionally not a general FPGA ABI. TRUEOS exposes only the two
//! known-working calls that are useful for proving the PCI/MMIO/MSI path:
//! heartbeat and `add_u32`.

use core::mem::{align_of, size_of};

pub const ABI_VERSION: u16 = 1;
pub const INLINE_INPUT_BYTES: usize = 96;
pub const INLINE_OUTPUT_BYTES: usize = 96;
pub const WORK_PACKAGE_MAGIC: u32 = 0x4B50_5754; // "TWPK"
pub const FLAG_INTERRUPT_ON_COMPLETE: u32 = 1;
pub const HEARTBEAT_REPLY: u32 = 0x5453_4154; // "TGAT"

pub const BAR0_LED_OFFSET: usize = 0x000;
pub const BAR0_LIVENESS_MAGIC_OFFSET: usize = 0x020;
pub const BAR0_CALL_DOORBELL_OFFSET: usize = 0x080;
pub const BAR0_CALL_IRQ_ACK_OFFSET: usize = 0x084;
pub const BAR0_WORK_PACKAGE_OFFSET: usize = 0x100;

pub const CALL_DOORBELL_MAGIC: u32 = 0x4C4C_4143; // "CALL"

#[repr(transparent)]
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct FunctionId(u16);

impl FunctionId {
    pub const HEARTBEAT: Self = Self(0);
    pub const ADD_U32: Self = Self(1);

    pub const fn new(raw: u16) -> Option<Self> {
        match raw {
            0 => Some(Self::HEARTBEAT),
            1 => Some(Self::ADD_U32),
            _ => None,
        }
    }

    pub const fn raw(self) -> u16 {
        self.0
    }
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WorkState {
    Idle = 0,
    HostReady = 1,
    FpgaBusy = 2,
    Complete = 3,
    Failed = 4,
}

impl WorkState {
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Idle),
            1 => Some(Self::HostReady),
            2 => Some(Self::FpgaBusy),
            3 => Some(Self::Complete),
            4 => Some(Self::Failed),
            _ => None,
        }
    }
}

#[repr(C, align(64))]
#[derive(Copy, Clone)]
pub struct WorkPackage {
    pub magic: u32,
    pub abi_version: u16,
    pub function: u16,
    pub call_id: u64,
    pub state: u32,
    pub flags: u32,
    pub input_len: u32,
    pub output_capacity: u32,
    pub output_len: u32,
    pub error_code: i32,
    pub reserved_header: [u8; 24],
    pub input: [u8; INLINE_INPUT_BYTES],
    pub output: [u8; INLINE_OUTPUT_BYTES],
}

impl WorkPackage {
    pub const ZEROED: Self = Self {
        magic: WORK_PACKAGE_MAGIC,
        abi_version: ABI_VERSION,
        function: 0,
        call_id: 0,
        state: WorkState::Idle as u32,
        flags: 0,
        input_len: 0,
        output_capacity: 0,
        output_len: 0,
        error_code: 0,
        reserved_header: [0; 24],
        input: [0; INLINE_INPUT_BYTES],
        output: [0; INLINE_OUTPUT_BYTES],
    };
}

pub const WORK_PACKAGE_STATE_OFFSET: usize = core::mem::offset_of!(WorkPackage, state);

pub fn encode_add_u32(a: u32, b: u32) -> [u8; 8] {
    let mut encoded = [0; 8];
    encoded[..4].copy_from_slice(&a.to_le_bytes());
    encoded[4..].copy_from_slice(&b.to_le_bytes());
    encoded
}

pub fn decode_u32(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
}

const _: [(); 256] = [(); size_of::<WorkPackage>()];
const _: [(); 64] = [(); align_of::<WorkPackage>()];
