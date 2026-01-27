#[cfg(target_arch = "x86_64")]
use rdrand::{RdRand, RdSeed};

#[cfg(target_arch = "x86_64")]
use rand_chacha::ChaCha20Rng;

#[cfg(target_arch = "x86_64")]
use rand_core::{RngCore, SeedableRng};

#[cfg(target_arch = "x86_64")]
use spin::Mutex;

#[cfg(target_arch = "x86_64")]
use zeroize::Zeroize;

#[cfg(target_arch = "x86_64")]
static CSPRNG: Mutex<Option<ChaCha20Rng>> = Mutex::new(None);

#[cfg(target_arch = "x86_64")]
pub fn rdrand_u64() -> Option<u64> {
    let rng = RdRand::new().ok()?;
    rng.try_next_u64().ok()
}

#[cfg(target_arch = "x86_64")]
pub fn rdseed_u64() -> Option<u64> {
    let rng = RdSeed::new().ok()?;
    rng.try_next_u64().ok()
}

#[cfg(target_arch = "x86_64")]
fn hw_seed_32() -> Option<[u8; 32]> {
    let mut seed = [0u8; 32];
    for chunk in seed.chunks_mut(8) {
        let v = rdseed_u64().or_else(rdrand_u64)?;
        chunk.copy_from_slice(&v.to_le_bytes());
    }
    Some(seed)
}

#[cfg(target_arch = "x86_64")]
fn virtio_seed_32() -> Option<[u8; 32]> {
    let mut seed = [0u8; 32];
    match crate::pci::vrng::try_fill_bytes(&mut seed) {
        Ok(()) => Some(seed),
        Err(_) => None,
    }
}

#[cfg(target_arch = "x86_64")]
fn seed_32() -> Option<[u8; 32]> {
    hw_seed_32().or_else(virtio_seed_32)
}

#[cfg(target_arch = "x86_64")]
fn ensure_csprng() -> bool {
    // Fast path: already initialized.
    {
        let guard = CSPRNG.lock();
        if guard.is_some() {
            return true;
        }
    }

    // Slow path: gather seed material without holding the CSPRNG lock.
    let Some(mut seed) = seed_32() else {
        return false;
    };

    // Seed material is high-value; wipe it after initializing the CSPRNG.
    let rng = ChaCha20Rng::from_seed(seed);
    seed.zeroize();

    let mut guard = CSPRNG.lock();
    if guard.is_none() {
        *guard = Some(rng);
    }
    true
}

/// Initialize the kernel CSPRNG state.
///
/// Safe to call multiple times; later calls are no-ops after successful init.
#[cfg(target_arch = "x86_64")]
pub fn init() -> bool {
    ensure_csprng()
}

/// Fill `dest` with cryptographically-strong random bytes.
///
/// Backed by a kernel CSPRNG (ChaCha20) seeded from RDSEED/RDRAND.
#[cfg(target_arch = "x86_64")]
pub fn fill_bytes(dest: &mut [u8]) -> bool {
    if !ensure_csprng() {
        return false;
    }
    let mut guard = CSPRNG.lock();
    let Some(rng) = guard.as_mut() else {
        return false;
    };
    rng.fill_bytes(dest);
    true
}

#[cfg(target_arch = "x86_64")]
fn trueos_getrandom(dest: &mut [u8]) -> Result<(), getrandom::Error> {
    if fill_bytes(dest) {
        Ok(())
    } else {
        Err(getrandom::Error::from(core::num::NonZeroU32::new(1).unwrap()))
    }
}

#[cfg(target_arch = "x86_64")]
getrandom::register_custom_getrandom!(trueos_getrandom);