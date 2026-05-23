//! Machine-operation artifacts.
//!
//! This is the narrow place where Silk admits that the universe bottoms out in
//! instructions. On Intel/x86_64, `add reg, reg` is a real machine operation
//! with a stable register-level meaning. Wrapping it as an artifact keeps the
//! raw operation available while forcing the layer above to see a named shape:
//! inputs, outputs, status, and a validation path instead of loose inline asm.

use crate::SilkStatus;

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MachineOpKind {
    AddU64 = 1,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct MachineOpArtifact {
    pub name: &'static str,
    pub kind: MachineOpKind,
    pub input_count: u8,
    pub output_count: u8,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct MachineOpResult {
    pub status: SilkStatus,
    pub value: u64,
}

impl MachineOpArtifact {
    pub const fn add_u64(name: &'static str) -> Self {
        Self {
            name,
            kind: MachineOpKind::AddU64,
            input_count: 2,
            output_count: 1,
        }
    }

    pub fn run_add_u64(self, lhs: u64, rhs: u64) -> MachineOpResult {
        if self.kind != MachineOpKind::AddU64 || self.input_count != 2 || self.output_count != 1 {
            return MachineOpResult {
                status: SilkStatus::Corrupt,
                value: 0,
            };
        }

        MachineOpResult {
            status: SilkStatus::Ok,
            value: machine_add_u64(lhs, rhs),
        }
    }
}

#[inline(always)]
fn machine_add_u64(lhs: u64, rhs: u64) -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        let mut value = lhs;
        unsafe {
            core::arch::asm!(
                "add {0}, {1}",
                inout(reg) value,
                in(reg) rhs,
                options(nomem, nostack)
            );
        }
        value
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        lhs.wrapping_add(rhs)
    }
}
