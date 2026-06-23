//! Tyche: kernel entropy and small pseudo-random helpers.
//! The kernel CSPRNG is ChaCha20 seeded from hardware entropy and virtio-rng.
//! `SoftRng` is for UI variation, games, randomized retries, and demos. It is
//! not a cryptographic RNG.
use rand_chacha::ChaCha20Rng;
use rand_core::{RngCore, SeedableRng};
use spin::Mutex;
use zeroize::Zeroize;

#[cfg(target_arch = "x86_64")]
use rand_core::rdrand::{RdRand, RdSeed};

const SPLITMIX_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

static CSPRNG: Mutex<Option<ChaCha20Rng>> = Mutex::new(None);
static TYCHE_SEED_SALT: u8 = 0xA7;

#[derive(Clone, Copy, Debug)]
pub struct SoftRng {
    state: u64,
}

impl SoftRng {
    pub fn new() -> Self {
        let local = 0u8;
        let stack_addr = (&local as *const u8 as usize) as u64;
        let salt_addr = (&TYCHE_SEED_SALT as *const u8 as usize) as u64;
        let seed = random_u64().unwrap_or_else(|| {
            mix_seed(
                crate::chronos::monotonic_nanos(),
                stack_addr.rotate_left(17) ^ salt_addr.rotate_right(7),
            )
        });
        Self::from_seed(seed)
    }

    pub const fn from_seed(seed: u64) -> Self {
        Self {
            state: if seed == 0 { SPLITMIX_GAMMA } else { seed },
        }
    }

    pub fn reseed(&mut self, seed: u64) {
        self.state = if seed == 0 { SPLITMIX_GAMMA } else { seed };
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(SPLITMIX_GAMMA);
        mix64(self.state)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    pub fn usize_below(&mut self, upper: usize) -> usize {
        if upper <= 1 {
            return 0;
        }

        let upper64 = upper as u64;
        let zone = u64::MAX - (u64::MAX % upper64);
        loop {
            let value = self.next_u64();
            if value < zone {
                return (value % upper64) as usize;
            }
        }
    }

    pub fn bool(&mut self) -> bool {
        (self.next_u64() & 1) != 0
    }

    pub fn shuffle<T>(&mut self, values: &mut [T]) {
        for idx in (1..values.len()).rev() {
            let swap_with = self.usize_below(idx + 1);
            values.swap(idx, swap_with);
        }
    }
}

impl Default for SoftRng {
    fn default() -> Self {
        Self::new()
    }
}

pub fn soft_rng() -> SoftRng {
    SoftRng::new()
}

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

#[cfg(not(target_arch = "x86_64"))]
pub fn rdrand_u64() -> Option<u64> {
    None
}

#[cfg(not(target_arch = "x86_64"))]
pub fn rdseed_u64() -> Option<u64> {
    None
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

#[cfg(not(target_arch = "x86_64"))]
fn hw_seed_32() -> Option<[u8; 32]> {
    None
}

fn virtio_seed_32() -> Option<[u8; 32]> {
    let mut seed = [0u8; 32];
    match crate::pci::vrng::try_fill_bytes(&mut seed) {
        Ok(()) => Some(seed),
        Err(_) => None,
    }
}

fn seed_32() -> Option<[u8; 32]> {
    hw_seed_32().or_else(virtio_seed_32)
}

fn ensure_csprng() -> bool {
    {
        let guard = CSPRNG.lock();
        if guard.is_some() {
            return true;
        }
    }

    let Some(mut seed) = seed_32() else {
        return false;
    };

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
pub fn init() -> bool {
    ensure_csprng()
}

/// Fill `dest` with cryptographically-strong random bytes.
///
/// Backed by a kernel CSPRNG (ChaCha20) seeded from hardware and/or virtio entropy.
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

pub fn random_u64() -> Option<u64> {
    let mut bytes = [0u8; 8];
    fill_bytes(&mut bytes).then(|| u64::from_le_bytes(bytes))
}

#[cfg(any(target_os = "none", target_os = "trueos", target_os = "zkvm"))]
fn trueos_getrandom_02(dest: &mut [u8]) -> Result<(), getrandom_02::Error> {
    if fill_bytes(dest) {
        Ok(())
    } else {
        let code = core::num::NonZeroU32::new(getrandom_02::Error::CUSTOM_START + 1).unwrap();
        Err(getrandom_02::Error::from(code))
    }
}

#[cfg(any(target_os = "none", target_os = "trueos", target_os = "zkvm"))]
getrandom_02::register_custom_getrandom!(trueos_getrandom_02);

#[inline]
const fn mix_seed(a: u64, b: u64) -> u64 {
    a ^ b ^ 0xD1B5_4A32_D192_ED03
}

#[inline]
fn mix64(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}
