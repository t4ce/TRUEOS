//! Control artifacts: tiny sequencing without a language.
//!
//! Real hardware boot flows are ordered: place memory, bind names, execute an
//! operation, check the result. A full control language would be too much this
//! early, but a sequence artifact has a right to exist because it turns that
//! ordering into data. It lets Silk prove that several low-level facts all held
//! without smuggling in branches, loops, or syntax.

use crate::SilkStatus;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SequenceArtifact {
    pub name: &'static str,
    pub step_count: u8,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SequenceResult {
    pub status: SilkStatus,
    pub completed: u8,
    pub failed_index: u8,
}

impl SequenceArtifact {
    pub const fn fixed(name: &'static str, step_count: u8) -> Self {
        Self { name, step_count }
    }

    pub fn run(self, statuses: &[SilkStatus]) -> SequenceResult {
        if statuses.len() < self.step_count as usize {
            return SequenceResult {
                status: SilkStatus::OutOfBounds,
                completed: 0,
                failed_index: 0,
            };
        }

        let mut idx = 0usize;
        while idx < self.step_count as usize {
            if statuses[idx] != SilkStatus::Ok {
                return SequenceResult {
                    status: statuses[idx],
                    completed: idx as u8,
                    failed_index: idx as u8,
                };
            }
            idx += 1;
        }

        SequenceResult {
            status: SilkStatus::Ok,
            completed: self.step_count,
            failed_index: u8::MAX,
        }
    }
}
