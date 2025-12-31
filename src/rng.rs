#[cfg(target_arch = "x86_64")]
use rdrand::{RdRand, RdSeed};

/// CPUID probe for RDRAND (ECX bit 30 of leaf 1).
pub fn has_rdrand() -> bool {
    raw_cpuid::CpuId::new()
        .get_feature_info()
        .map(|info| info.has_rdrand())
        .unwrap_or(false)
}

/// CPUID probe for RDSEED (EBX bit 18 of leaf 7, subleaf 0).
pub fn has_rdseed() -> bool {
    raw_cpuid::CpuId::new()
        .get_extended_feature_info()
        .map(|info| info.has_rdseed())
        .unwrap_or(false)
}
/// Fetch a 64-bit random value using RDRAND.
pub fn rdrand_u64() -> Option<u64> {
    let rng = RdRand::new().ok()?;
    rng.try_next_u64().ok()
}

/// Fetch a 64-bit seed value using RDSEED.
pub fn rdseed_u64() -> Option<u64> {
    let rng = RdSeed::new().ok()?;
    rng.try_next_u64().ok()
}

pub fn log_rng_caps() {
    let rdrand = has_rdrand();
    let rdseed = has_rdseed();
    match (rdrand, rdseed) {
        (true, true) => crate::debugconf!("RNG: RDRAND and RDSEED available.\n"),
        (true, false) => crate::debugconf!("RNG: RDRAND available, RDSEED unavailable.\n"),
        (false, true) => crate::debugconf!("RNG: RDSEED available, RDRAND unavailable.\n"),
        (false, false) => {
            crate::debugconf!("RNG: no hardware entropy source (RDRAND/RDSEED unavailable).\n")
        }
    }

    if rdseed {
        match rdseed_u64() {
            Some(val) => crate::debugconf!("RNG: RDSEED sample = 0x{:016X}\n", val),
            None => crate::debugconf!("RNG: RDSEED sample failed (busy?).\n"),
        }
    }

    if rdrand {
        match rdrand_u64() {
            Some(val) => crate::debugconf!("RNG: RDRAND sample = 0x{:016X}\n", val),
            None => crate::debugconf!("RNG: RDRAND sample failed.\n"),
        }
    }
}
