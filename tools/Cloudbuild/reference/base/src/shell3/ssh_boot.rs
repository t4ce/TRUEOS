//! Development SSH is available without account storage, ESP, or TPM readiness.
//! The build reuses a private local host seed and embeds it in the kernel.
use zeroize::Zeroizing;

const HOST_SEED: &[u8; 32] = include_bytes!(concat!(env!("OUT_DIR"), "/ssh-dev-host-seed.bin"));

pub(super) fn allow_none() -> bool {
    true
}

pub(super) fn host_seed() -> Result<Zeroizing<[u8; 32]>, crate::crypt::CryError> {
    Ok(Zeroizing::new(*HOST_SEED))
}
