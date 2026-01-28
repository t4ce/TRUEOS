//! Kernel-level ACPI facade.
//!
//! The implementation lives in `crate::efi::acpi` (UEFI/ACPI table parsing + helpers).
//! This module is a thin re-export layer so the rest of the kernel can refer to
//! `crate::acpi::*` without depending on the `efi` module path.

/// Ensure ACPI tables are initialized and return them if available.
#[inline]
pub fn ensure_tables() -> Option<&'static ::acpi::AcpiTables<crate::efi::acpi::AcpiIdentityHandler>> {
	crate::efi::acpi::ensure_tables()
}

pub mod bgrt {
	pub use crate::efi::acpi::bgrt::*;
}

pub mod dbg {
	pub use crate::efi::acpi::dbg::*;
}

pub mod dmar {
	pub use crate::efi::acpi::dmar::*;
}

pub mod facp {
	pub use crate::efi::acpi::facp::*;
}

pub mod fpdt {
	pub use crate::efi::acpi::fpdt::*;
}

pub mod hpet {
	pub use crate::efi::acpi::hpet::*;
}

pub mod madt {
	pub use crate::efi::acpi::madt::*;
}

pub mod ssdt {
	pub use crate::efi::acpi::ssdt::*;
}

pub mod tpm2 {
	pub use crate::efi::acpi::tpm2::*;
}
