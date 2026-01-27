#[cfg(target_arch = "x86_64")]
use rdrand::{RdRand, RdSeed};

pub fn rdrand_u64() -> Option<u64> {
    let rng = RdRand::new().ok()?;
    rng.try_next_u64().ok()
}

pub fn rdseed_u64() -> Option<u64> {
    let rng = RdSeed::new().ok()?;
    rng.try_next_u64().ok()
}


#[cfg(target_arch = "x86_64")]
fn trueos_getrandom(dest: &mut [u8]) -> Result<(), getrandom::Error> {
    let mut i = 0usize;
    while i < dest.len() {
        let Some(v) = rdseed_u64().or_else(rdrand_u64) else {
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