//! Development SSH is available without account storage, ESP, or TPM readiness.
//! The build reuses a private local host seed and embeds it in the kernel.
//!
//! IMPORTANT: SSH account authentication WORKS. Repeated authenticator approval
//! for temporary development boots is impractical, so the flag below deliberately
//! skips account proof. Anyone who can reach SSH can use the loaded account by
//! its exact username. This is development access, NOT evidence of authentication.
//! Set false to restore enrolled-key / authenticator proof for saved accounts.
//! Revisit when the TRUEOS/Codex integration is hardened; do not carry this
//! concession into deployment policy.
pub(super) const DEV_SKIP_SSH_ACCOUNT_PROOF: bool = true;

use zeroize::Zeroizing;

const HOST_SEED: &[u8; 32] = include_bytes!(concat!(env!("OUT_DIR"), "/ssh-dev-host-seed.bin"));

pub(super) fn allow_none() -> bool {
    true
}

pub(super) fn account_without_proof(username: &str) -> Option<trueos_crypto::AccountId> {
    if DEV_SKIP_SSH_ACCOUNT_PROOF {
        crate::crypt::ssh_development_account(username)
    } else {
        None
    }
}

pub(super) fn host_seed() -> Result<Zeroizing<[u8; 32]>, crate::crypt::CryError> {
    Ok(Zeroizing::new(*HOST_SEED))
}
