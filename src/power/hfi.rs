use core::arch::x86_64::__cpuid;

const CPUID_LEAF_FEATURES: u32 = 0x01;
const CPUID_LEAF_THERMAL_POWER: u32 = 0x06;
const CPUID_FEATURE_MSR: u32 = 1 << 5;
const CPUID_HFI: u32 = 1 << 19;
const CPUID_THREAD_DIRECTOR: u32 = 1 << 23;
const CPUID_TD_CLASS_SHIFT: u32 = 8;
const CPUID_HFI_PERFORMANCE: u32 = 1 << 0;
const CPUID_HFI_ENERGY_EFFICIENCY: u32 = 1 << 1;
const CPUID_HFI_PAGE_SHIFT: u32 = 8;
const CPUID_HFI_INDEX_SHIFT: u32 = 16;
const HFI_PAGE_BYTES: usize = 4096;

const PROFILE_FLAG_LEAF_AVAILABLE: u8 = 1 << 0;
const PROFILE_FLAG_MSR_INSTRUCTION: u8 = 1 << 1;

const INTEL_VENDOR_EBX: u32 = 0x756E_6547;
const INTEL_VENDOR_EDX: u32 = 0x4965_6E69;
const INTEL_VENDOR_ECX: u32 = 0x6C65_746E;

/// CPUID-only Intel Hardware Feedback Interface / Thread Director metadata for
/// one logical processor. This does not mean TRUEOS has configured an HFI table
/// or touched any `IA32_HW_FEEDBACK_*` MSR.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct IntelHfiCpuid {
    pub(crate) leaf_available: bool,
    pub(crate) msr_instruction: bool,
    pub(crate) eax: u32,
    pub(crate) ebx: u32,
    pub(crate) ecx: u32,
    pub(crate) edx: u32,
}

impl IntelHfiCpuid {
    pub(crate) const fn unavailable() -> Self {
        Self {
            leaf_available: false,
            msr_instruction: false,
            eax: 0,
            ebx: 0,
            ecx: 0,
            edx: 0,
        }
    }

    pub(crate) const fn from_profile_storage(
        flags: u8,
        eax: u32,
        ebx: u32,
        ecx: u32,
        edx: u32,
    ) -> Self {
        Self {
            leaf_available: (flags & PROFILE_FLAG_LEAF_AVAILABLE) != 0,
            msr_instruction: (flags & PROFILE_FLAG_MSR_INSTRUCTION) != 0,
            eax,
            ebx,
            ecx,
            edx,
        }
    }

    pub(crate) fn profile_storage_flags(self) -> u8 {
        let mut flags = 0;
        if self.leaf_available {
            flags |= PROFILE_FLAG_LEAF_AVAILABLE;
        }
        if self.msr_instruction {
            flags |= PROFILE_FLAG_MSR_INSTRUCTION;
        }
        flags
    }

    pub(crate) fn detect_current() -> Self {
        let leaf0 = __cpuid(0);
        let vendor_intel = leaf0.ebx == INTEL_VENDOR_EBX
            && leaf0.edx == INTEL_VENDOR_EDX
            && leaf0.ecx == INTEL_VENDOR_ECX;
        if !vendor_intel || leaf0.eax < CPUID_LEAF_THERMAL_POWER {
            return Self::unavailable();
        }

        let features = __cpuid(CPUID_LEAF_FEATURES);
        let leaf6 = __cpuid(CPUID_LEAF_THERMAL_POWER);
        Self {
            leaf_available: true,
            msr_instruction: (features.edx & CPUID_FEATURE_MSR) != 0,
            eax: leaf6.eax,
            ebx: leaf6.ebx,
            ecx: leaf6.ecx,
            edx: leaf6.edx,
        }
    }

    pub(crate) const fn hfi_supported(self) -> bool {
        self.leaf_available && (self.eax & CPUID_HFI) != 0
    }

    pub(crate) const fn thread_director_supported(self) -> bool {
        self.leaf_available && (self.eax & CPUID_THREAD_DIRECTOR) != 0
    }

    pub(crate) const fn thread_director_classes(self) -> u8 {
        ((self.ecx >> CPUID_TD_CLASS_SHIFT) & 0xFF) as u8
    }

    pub(crate) const fn capability_mask(self) -> u8 {
        (self.edx & 0xFF) as u8
    }

    pub(crate) const fn performance_reporting(self) -> bool {
        self.hfi_supported() && (self.edx & CPUID_HFI_PERFORMANCE) != 0
    }

    pub(crate) const fn energy_efficiency_reporting(self) -> bool {
        self.hfi_supported() && (self.edx & CPUID_HFI_ENERGY_EFFICIENCY) != 0
    }

    pub(crate) const fn table_pages(self) -> Option<u8> {
        if self.hfi_supported() {
            Some((((self.edx >> CPUID_HFI_PAGE_SHIFT) & 0x0F) + 1) as u8)
        } else {
            None
        }
    }

    pub(crate) const fn table_bytes(self) -> Option<usize> {
        match self.table_pages() {
            Some(pages) => Some((pages as usize) * HFI_PAGE_BYTES),
            None => None,
        }
    }

    pub(crate) const fn row_index(self) -> Option<i16> {
        if self.hfi_supported() {
            Some(((self.edx >> CPUID_HFI_INDEX_SHIFT) as u16) as i16)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_hfi_and_thread_director_metadata() {
        let raw = IntelHfiCpuid {
            leaf_available: true,
            msr_instruction: true,
            eax: CPUID_HFI | CPUID_THREAD_DIRECTOR,
            ebx: 0,
            ecx: 4 << CPUID_TD_CLASS_SHIFT,
            edx: CPUID_HFI_PERFORMANCE
                | CPUID_HFI_ENERGY_EFFICIENCY
                | (2 << CPUID_HFI_PAGE_SHIFT)
                | (7 << CPUID_HFI_INDEX_SHIFT),
        };

        assert!(raw.hfi_supported());
        assert!(raw.thread_director_supported());
        assert_eq!(raw.thread_director_classes(), 4);
        assert_eq!(raw.capability_mask(), 0x03);
        assert!(raw.performance_reporting());
        assert!(raw.energy_efficiency_reporting());
        assert_eq!(raw.table_pages(), Some(3));
        assert_eq!(raw.table_bytes(), Some(3 * HFI_PAGE_BYTES));
        assert_eq!(raw.row_index(), Some(7));
        assert_eq!(
            IntelHfiCpuid::from_profile_storage(
                raw.profile_storage_flags(),
                raw.eax,
                raw.ebx,
                raw.ecx,
                raw.edx,
            ),
            raw
        );
    }

    #[test]
    fn preserves_signed_hfi_row_index() {
        let raw = IntelHfiCpuid {
            leaf_available: true,
            msr_instruction: true,
            eax: CPUID_HFI,
            ebx: 0,
            ecx: 0,
            edx: CPUID_HFI_PERFORMANCE | (0xFFFF << CPUID_HFI_INDEX_SHIFT),
        };

        assert_eq!(raw.row_index(), Some(-1));
    }
}
