#[cfg(target_arch = "x86_64")]
mod imp {
    use core::arch::x86_64::__cpuid;
    use x86_64::registers::model_specific::Msr;

    const IA32_PMC0: u32 = 0xC1;
    const IA32_FIXED_CTR0: u32 = 0x309;
    const IA32_PERF_GLOBAL_CTRL: u32 = 0x38F;
    const IA32_FIXED_CTR_CTRL: u32 = 0x38D;
    const FIXED_CTR_CTRL_ENABLE_OS_USER: u64 = 0b11;
    const FIXED_CTR_GLOBAL_CTRL_BASE_BIT: u32 = 32;

    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Snapshot {
        pub(crate) arch_perfmon: bool,
        pub(crate) version: u8,
        pub(crate) gp_counter_count: u8,
        pub(crate) gp_counter_bits: u8,
        pub(crate) event_mask_len: u8,
        pub(crate) unavailable_events: u32,
        pub(crate) fixed_counter_count: u8,
        pub(crate) fixed_counter_bits: u8,
        pub(crate) perf_global_ctrl: Option<u64>,
        pub(crate) fixed_ctr_ctrl: Option<u64>,
        pub(crate) fixed_ctr: [Option<u64>; 3],
        pub(crate) pmc0: Option<u64>,
    }

    pub(crate) fn snapshot() -> Snapshot {
        let max_leaf = unsafe { __cpuid(0) }.eax;
        if max_leaf < 0xA {
            return Snapshot::unsupported();
        }

        let leaf = unsafe { __cpuid(0xA) };
        let version = (leaf.eax & 0xFF) as u8;
        if version == 0 {
            return Snapshot::unsupported();
        }

        let gp_counter_count = ((leaf.eax >> 8) & 0xFF) as u8;
        let fixed_counter_count = (leaf.edx & 0x1F) as u8;
        let mut fixed_ctr = [None; 3];
        let fixed_to_read = fixed_counter_count.min(3);
        for idx in 0..fixed_to_read {
            fixed_ctr[idx as usize] = Some(unsafe { Msr::new(IA32_FIXED_CTR0 + idx as u32).read() });
        }

        Snapshot {
            arch_perfmon: true,
            version,
            gp_counter_count,
            gp_counter_bits: ((leaf.eax >> 16) & 0xFF) as u8,
            event_mask_len: ((leaf.eax >> 24) & 0xFF) as u8,
            unavailable_events: leaf.ebx,
            fixed_counter_count,
            fixed_counter_bits: ((leaf.edx >> 5) & 0xFF) as u8,
            perf_global_ctrl: Some(unsafe { Msr::new(IA32_PERF_GLOBAL_CTRL).read() }),
            fixed_ctr_ctrl: Some(unsafe { Msr::new(IA32_FIXED_CTR_CTRL).read() }),
            fixed_ctr,
            pmc0: if gp_counter_count != 0 {
                Some(unsafe { Msr::new(IA32_PMC0).read() })
            } else {
                None
            },
        }
    }

    pub(crate) fn ensure_liveness_source() -> bool {
        let snapshot = snapshot();
        if !snapshot.arch_perfmon || snapshot.fixed_counter_count == 0 {
            return false;
        }

        let fixed_to_enable = snapshot.fixed_counter_count.min(3);
        let mut fixed_ctr_ctrl = snapshot.fixed_ctr_ctrl.unwrap_or(0);
        let mut perf_global_ctrl = snapshot.perf_global_ctrl.unwrap_or(0);

        for idx in 0..fixed_to_enable {
            fixed_ctr_ctrl |= FIXED_CTR_CTRL_ENABLE_OS_USER << (u32::from(idx) * 4);
            perf_global_ctrl |= 1u64 << (FIXED_CTR_GLOBAL_CTRL_BASE_BIT + u32::from(idx));
        }

        unsafe {
            Msr::new(IA32_FIXED_CTR_CTRL).write(fixed_ctr_ctrl);
            Msr::new(IA32_PERF_GLOBAL_CTRL).write(perf_global_ctrl);
        }
        true
    }

    impl Snapshot {
        const fn unsupported() -> Self {
            Self {
                arch_perfmon: false,
                version: 0,
                gp_counter_count: 0,
                gp_counter_bits: 0,
                event_mask_len: 0,
                unavailable_events: 0,
                fixed_counter_count: 0,
                fixed_counter_bits: 0,
                perf_global_ctrl: None,
                fixed_ctr_ctrl: None,
                fixed_ctr: [None, None, None],
                pmc0: None,
            }
        }
    }
}

#[cfg(not(target_arch = "x86_64"))]
mod imp {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Snapshot {
        pub(crate) arch_perfmon: bool,
        pub(crate) version: u8,
        pub(crate) gp_counter_count: u8,
        pub(crate) gp_counter_bits: u8,
        pub(crate) event_mask_len: u8,
        pub(crate) unavailable_events: u32,
        pub(crate) fixed_counter_count: u8,
        pub(crate) fixed_counter_bits: u8,
        pub(crate) perf_global_ctrl: Option<u64>,
        pub(crate) fixed_ctr_ctrl: Option<u64>,
        pub(crate) fixed_ctr: [Option<u64>; 3],
        pub(crate) pmc0: Option<u64>,
    }

    pub(crate) fn snapshot() -> Snapshot {
        Snapshot {
            arch_perfmon: false,
            version: 0,
            gp_counter_count: 0,
            gp_counter_bits: 0,
            event_mask_len: 0,
            unavailable_events: 0,
            fixed_counter_count: 0,
            fixed_counter_bits: 0,
            perf_global_ctrl: None,
            fixed_ctr_ctrl: None,
            fixed_ctr: [None, None, None],
            pmc0: None,
        }
    }

    pub(crate) fn ensure_liveness_source() -> bool {
        false
    }
}

pub(crate) use imp::{Snapshot, ensure_liveness_source, snapshot};
