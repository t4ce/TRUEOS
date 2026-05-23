//! Binding artifacts: names attached to placed machine reality.
//!
//! A linker ultimately resolves names into addresses, and a CPU only sees the
//! address. Silk needs the step in between: a compact record that says which
//! intent is allowed to point at which placed artifact, under which capability.
//! That keeps future compiler adapters from improvising their own symbol tables
//! while still staying far below a full classical linker.

use crate::{SilkStatus, Span};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SymbolBindingArtifact {
    pub name: &'static str,
    pub import: &'static str,
    pub export: &'static str,
    pub capability: &'static str,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SymbolBindingRecord {
    pub artifact: SymbolBindingArtifact,
    pub span: Span,
}

impl SymbolBindingArtifact {
    pub const fn new(
        name: &'static str,
        import: &'static str,
        export: &'static str,
        capability: &'static str,
    ) -> Self {
        Self {
            name,
            import,
            export,
            capability,
        }
    }

    pub fn bind(self, span: Span, required_align: u64) -> Result<SymbolBindingRecord, SilkStatus> {
        if self.name.is_empty()
            || self.import.is_empty()
            || self.export.is_empty()
            || self.capability.is_empty()
        {
            return Err(SilkStatus::Corrupt);
        }
        if required_align == 0 || !required_align.is_power_of_two() {
            return Err(SilkStatus::BadAlign);
        }
        if span.len == 0 {
            return Err(SilkStatus::OutOfBounds);
        }
        if span.addr & (required_align - 1) != 0 {
            return Err(SilkStatus::BadAlign);
        }

        Ok(SymbolBindingRecord {
            artifact: self,
            span,
        })
    }
}
