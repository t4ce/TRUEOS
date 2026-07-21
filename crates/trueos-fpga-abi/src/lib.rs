#![no_std]

//! Fixed host/FPGA call ABI shared by the ahead-of-time firmware build and TRUEOS.
//!
//! This crate deliberately contains no compiler, RTL, transport driver, allocator, or
//! scheduler.  An FPGA firmware build assigns code to the three function slots and emits
//! a [`FirmwareManifest`] beside its bitstream.  The kernel only copies one
//! [`WorkPackage`] through the fixed TGA BAR window and observes its completion state.

use core::mem::{align_of, size_of};

pub const ABI_VERSION: u16 = 1;
pub const FUNCTION_COUNT: usize = 3;
pub const INLINE_INPUT_BYTES: usize = 96;
pub const INLINE_OUTPUT_BYTES: usize = 96;

pub const WORK_PACKAGE_MAGIC: u32 = 0x4B50_5754; // "TWPK"
pub const FIRMWARE_MANIFEST_MAGIC: u32 = 0x4D46_5754; // "TWFM"

/// Ask the endpoint to raise its completion interrupt after publishing `state`.
///
/// Polling the state remains valid, so early firmware can ignore this flag.
pub const FLAG_INTERRUPT_ON_COMPLETE: u32 = 1 << 0;

/// One of exactly three functions fused into an FPGA firmware artifact.
///
/// Names and Rust signatures belong to the generated Rust interface emitted beside the
/// firmware.  The wire ABI intentionally sees only stable slot numbers.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct FunctionId(u16);

impl FunctionId {
    pub const SLOT_0: Self = Self(0);
    pub const SLOT_1: Self = Self(1);
    pub const SLOT_2: Self = Self(2);

    pub const fn new(raw: u16) -> Option<Self> {
        if (raw as usize) < FUNCTION_COUNT {
            Some(Self(raw))
        } else {
            None
        }
    }

    pub const fn raw(self) -> u16 {
        self.0
    }
}

/// Ownership flag stored in [`WorkPackage::state`].
///
/// The host publishes all request bytes before `HostReady`.  The FPGA publishes all
/// completion bytes before `Complete` or `Failed`.  Both sides must access the shared
/// state with the cache/fence operations required by their PCIe implementation.
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

/// The only call object shared with the FPGA, mapped at BAR0 + [`BAR0_WORK_PACKAGE_OFFSET`].
///
/// Inputs and outputs are inline by design.  The first implementation therefore needs no
/// process address space, DMA requester, scatter/gather list, TLB, or device-side command
/// processor.
/// Larger payload protocols can be introduced later as separate function signatures
/// without changing this minimal call path.
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

/// Build-time description of one compiled function slot.
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FunctionDescriptor {
    pub id: u16,
    pub input_bytes: u16,
    pub output_bytes: u16,
    pub flags: u16,
    /// Stable hash of the generated Rust symbol/signature, defined by the build tool.
    pub symbol_hash: u64,
}

/// Binary manifest emitted beside, or embedded into, the FPGA bitstream.
///
/// TRUEOS may compare this fixed metadata with the generated Rust interface, but it never
/// compiles or interprets the firmware source at runtime.
#[repr(C, align(64))]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FirmwareManifest {
    pub magic: u32,
    pub abi_version: u16,
    pub function_count: u16,
    pub work_package_bytes: u32,
    pub flags: u32,
    /// SHA-256 of the generated RTL consumed by the vendor synthesis tool.
    pub rtl_sha256: [u8; 32],
    pub functions: [FunctionDescriptor; FUNCTION_COUNT],
    pub reserved: [u8; 32],
}

/// TRUEGA's preserved LED/debug plane.
pub const BAR0_LED_OFFSET: usize = 0x000;
pub const BAR0_LIVENESS_MAGIC_OFFSET: usize = 0x020;
/// Write-only handoff after the complete package has been copied into the BAR window.
pub const BAR0_CALL_DOORBELL_OFFSET: usize = 0x080;
/// Optional completion-interrupt acknowledgement. Polling firmware may ignore it.
pub const BAR0_CALL_IRQ_ACK_OFFSET: usize = 0x084;
/// Read-only count of work-package retirements presented to the interrupt bridge.
pub const BAR0_CALL_IRQ_RETIRE_COUNT_OFFSET: usize = 0x088;
/// Read-only count of interrupt request pulses presented to the PCIe controller.
pub const BAR0_CALL_IRQ_REQUEST_COUNT_OFFSET: usize = 0x08C;
/// Read-only count of request acknowledgements returned by the PCIe controller.
pub const BAR0_CALL_IRQ_CONTROLLER_ACK_COUNT_OFFSET: usize = 0x090;
/// Read-only live interrupt bridge state: status, request, controller ACK, enable.
pub const BAR0_CALL_IRQ_STATE_OFFSET: usize = 0x094;
/// First byte of the fixed, dword-addressable call window.
pub const BAR0_WORK_PACKAGE_OFFSET: usize = 0x100;
/// Read-only manifest fused into the same firmware image as the function circuits.
pub const BAR0_FIRMWARE_MANIFEST_OFFSET: usize = 0x200;
pub const BAR0_REQUIRED_BYTES: usize =
    BAR0_FIRMWARE_MANIFEST_OFFSET + size_of::<FirmwareManifest>();
pub const WORK_PACKAGE_STATE_OFFSET: usize = core::mem::offset_of!(WorkPackage, state);

/// Typed host interface emitted beside the RustHDL-generated firmware RTL.
///
/// This checked-in module is ordinary `no_std` Rust metadata and packing code. Regenerating
/// it is an Ubuntu build action; TRUEOS never links the generator or understands HDL.
pub mod generated;
pub use generated as builtins;

/// Fixed LFM2.5 model-image layout.  This is data metadata, not a runtime compiler.
pub mod lfm25;

const _: [(); 256] = [(); size_of::<WorkPackage>()];
const _: [(); 64] = [(); align_of::<WorkPackage>()];
const _: [(); 128] = [(); size_of::<FirmwareManifest>()];
const _: [(); 64] = [(); align_of::<FirmwareManifest>()];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_binary_layout() {
        assert_eq!(size_of::<WorkPackage>(), 256);
        assert_eq!(align_of::<WorkPackage>(), 64);
        assert_eq!(size_of::<FirmwareManifest>(), 128);
        assert_eq!(align_of::<FirmwareManifest>(), 64);
        assert_eq!(BAR0_REQUIRED_BYTES, 0x280);
    }

    #[test]
    fn exactly_three_function_ids_are_valid() {
        assert_eq!(FunctionId::new(0), Some(FunctionId::SLOT_0));
        assert_eq!(FunctionId::new(1), Some(FunctionId::SLOT_1));
        assert_eq!(FunctionId::new(2), Some(FunctionId::SLOT_2));
        assert_eq!(FunctionId::new(3), None);
    }

    #[test]
    fn completion_states_are_stable() {
        assert_eq!(WorkState::from_raw(WorkState::Complete as u32), Some(WorkState::Complete));
        assert_eq!(WorkState::from_raw(99), None);
    }

    #[test]
    fn builtin_binary_interface_is_little_endian() {
        let args = builtins::add_u32::encode(0x1122_3344, 0xAABB_CCDD);
        assert_eq!(args, [0x44, 0x33, 0x22, 0x11, 0xDD, 0xCC, 0xBB, 0xAA]);
        assert_eq!(builtins::add_u32::decode(&args), Some(0x1122_3344));
        assert_eq!(builtins::led_step_heartbeat::encode(), []);

        use builtins::lfm25_q8_row_block as q8;
        let q8_input = q8::encode_single(&q8::GOLDEN_ACTIVATION, &q8::GOLDEN_WEIGHT);
        assert_eq!(q8_input.len(), 72);
        assert_eq!(&q8_input[..4], &[3, 0, 0, 0]);
        assert_eq!(&q8_input[4..38], &q8::GOLDEN_ACTIVATION);
        assert_eq!(&q8_input[38..], &q8::GOLDEN_WEIGHT);
        assert_eq!(&q8_input[4..8], &[0x30, 0x18, 0x0D, 0xA8]);
        assert_eq!(
            q8::decode(&[
                0xCB, 0xC5, 0xFF, 0xFF, 0x80, 0x1C, 0x70, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x80, 0x1C,
                0x70, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            ]),
            Some(q8::GOLDEN_RESULT),
        );
    }

    #[test]
    fn generated_manifest_matches_the_host_contract() {
        let manifest = builtins::FIRMWARE_MANIFEST;
        assert_eq!(manifest.magic, FIRMWARE_MANIFEST_MAGIC);
        assert_eq!(manifest.abi_version, ABI_VERSION);
        assert_eq!(manifest.function_count as usize, FUNCTION_COUNT);
        assert_eq!(manifest.work_package_bytes as usize, size_of::<WorkPackage>());
        assert_eq!(manifest.functions, builtins::FUNCTIONS);
    }
}
