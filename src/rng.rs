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
        (true, true) => crate::log!("RNG: RDRAND and RDSEED available.\n"),
        (true, false) => crate::log!("RNG: RDRAND available, RDSEED unavailable.\n"),
        (false, true) => crate::log!("RNG: RDSEED available, RDRAND unavailable.\n"),
        (false, false) => {
            crate::log!("RNG: no hardware entropy source (RDRAND/RDSEED unavailable).\n")
        }
    }

    if rdseed {
        match rdseed_u64() {
            Some(val) => crate::log!("RNG: RDSEED sample = 0x{:016X}\n", val),
            None => crate::log!("RNG: RDSEED sample failed (busy?).\n"),
        }
    }

    if rdrand {
        match rdrand_u64() {
            Some(val) => crate::log!("RNG: RDRAND sample = 0x{:016X}\n", val),
            None => crate::log!("RNG: RDRAND sample failed.\n"),
        }
    }
}

// Provide entropy for crates that rely on `getrandom` (e.g. rustls crypto providers).
// This uses x86_64 RDRAND; if unavailable, callers will see an UNSUPPORTED error.
#[cfg(target_arch = "x86_64")]
fn trueos_getrandom(dest: &mut [u8]) -> Result<(), getrandom::Error> {
    let mut i = 0usize;
    while i < dest.len() {
        let Some(v) = rdrand_u64() else {
            return Err(getrandom::Error::UNSUPPORTED);
        };
        let bytes = v.to_le_bytes();
        let n = (dest.len() - i).min(bytes.len());
        dest[i..i + n].copy_from_slice(&bytes[..n]);
        i += n;
    }
    Ok(())
}

#[cfg(target_arch = "x86_64")]
getrandom::register_custom_getrandom!(trueos_getrandom);
