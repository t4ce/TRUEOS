#[derive(Copy, Clone)]
pub struct X2ApicTopology {
    pub leaf: u32,
    pub smt_bits: u32,
    pub core_bits: u32,
}

impl X2ApicTopology {
    #[inline]
    pub fn decode(&self, apic_id: u32) -> (u32, u32, u32) {
        let smt_bits = self.smt_bits.min(31);
        let core_bits = self.core_bits.min(31).max(smt_bits);

        let smt_mask = if smt_bits == 0 {
            0
        } else {
            (1u32 << smt_bits) - 1
        };
        let core_mask_bits = core_bits.saturating_sub(smt_bits);
        let core_mask = if core_mask_bits == 0 {
            0
        } else {
            (1u32 << core_mask_bits) - 1
        };

        let smt = apic_id & smt_mask;
        let core = (apic_id >> smt_bits) & core_mask;
        let pkg = apic_id >> core_bits;
        (pkg, core, smt)
    }
}

pub fn detect_x2apic_topology() -> X2ApicTopology {
    #[cfg(target_arch = "x86_64")]
    {
        if let Some(t) = detect_x2apic_topology_leaf(0x1F) {
            return t;
        }
        if let Some(t) = detect_x2apic_topology_leaf(0x0B) {
            return t;
        }
    }
    X2ApicTopology {
        leaf: 0,
        smt_bits: 0,
        core_bits: 0,
    }
}

#[cfg(target_arch = "x86_64")]
fn detect_x2apic_topology_leaf(leaf: u32) -> Option<X2ApicTopology> {
    use core::arch::x86_64::__cpuid_count;

    let mut smt_bits: u32 = 0;
    let mut core_bits: u32 = 0;

    for subleaf in 0..32u32 {
        let r = unsafe { __cpuid_count(leaf, subleaf) };
        if r.ebx == 0 {
            break;
        }
        let level_type = (r.ecx >> 8) & 0xFF;
        let shift = r.eax & 0x1F;

        // level_type: 1 = SMT, 2 = Core
        if level_type == 1 {
            smt_bits = shift;
        } else if level_type == 2 {
            core_bits = shift;
        }
    }

    if smt_bits == 0 && core_bits == 0 {
        None
    } else {
        if core_bits < smt_bits {
            core_bits = smt_bits;
        }
        Some(X2ApicTopology {
            leaf,
            smt_bits,
            core_bits,
        })
    }
}