//! Validation artifacts.
//!
//! Machine work is only useful if the next layer can prove what happened.
//! Intel instructions give deterministic results for fixed operands, and memory
//! placement gives deterministic bounds. A validation artifact captures those
//! tiny proofs as reusable objects, so an upper language can require a fact like
//! "observed equals expected" without embedding ad hoc checks everywhere.

use crate::SilkStatus;

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ValidationKind {
    ExactU64 = 1,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ValidationArtifact {
    pub name: &'static str,
    pub kind: ValidationKind,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ValidationResult {
    pub status: SilkStatus,
    pub valid: bool,
}

impl ValidationArtifact {
    pub const fn exact_u64(name: &'static str) -> Self {
        Self {
            name,
            kind: ValidationKind::ExactU64,
        }
    }

    pub fn run_exact_u64(self, observed: u64, expected: u64) -> ValidationResult {
        if self.kind != ValidationKind::ExactU64 {
            return ValidationResult {
                status: SilkStatus::Corrupt,
                valid: false,
            };
        }

        ValidationResult {
            status: SilkStatus::Ok,
            valid: observed == expected,
        }
    }
}
