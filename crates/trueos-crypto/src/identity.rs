//! Stable key identity and authorization metadata.

use core::ops::{BitOr, BitOrAssign};

pub const KEY_HANDLE_BYTES: usize = 32;
pub const PROVIDER_ID_BYTES: usize = 16;
pub const MACHINE_ID_BYTES: usize = 32;
pub const BOOT_ID_BYTES: usize = 16;

/// Stable algorithm identifier used at the provider boundary.
///
/// This is a newtype rather than an enum so adding an algorithm does not force
/// consumers to change exhaustive matches. Zero is reserved as invalid.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct AlgorithmId(u16);

impl AlgorithmId {
    pub const ED25519: Self = Self(0x0001);
    pub const SECP256K1: Self = Self(0x0002);
    pub const RSA: Self = Self(0x0003);
    pub const ML_DSA_65: Self = Self(0x0101);

    #[inline]
    pub const fn new(code: u16) -> Option<Self> {
        if code == 0 { None } else { Some(Self(code)) }
    }

    #[inline]
    pub const fn code(self) -> u16 {
        self.0
    }

    #[inline]
    pub const fn is_quantum_resistant(self) -> bool {
        matches!(self, Self::ML_DSA_65)
    }
}

/// Opaque key identifier meaningful only inside its owning provider.
///
/// Providers should generate handles from a cryptographically strong random
/// source. The all-zero value is reserved and rejected.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct KeyHandle([u8; KEY_HANDLE_BYTES]);

impl KeyHandle {
    #[inline]
    pub fn new(bytes: [u8; KEY_HANDLE_BYTES]) -> Option<Self> {
        if bytes.iter().all(|byte| *byte == 0) {
            None
        } else {
            Some(Self(bytes))
        }
    }

    #[inline]
    pub const fn as_bytes(&self) -> &[u8; KEY_HANDLE_BYTES] {
        &self.0
    }
}

/// Stable identifier for the provider that owns a key.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct ProviderId([u8; PROVIDER_ID_BYTES]);

impl ProviderId {
    #[inline]
    pub fn new(bytes: [u8; PROVIDER_ID_BYTES]) -> Option<Self> {
        if bytes.iter().all(|byte| *byte == 0) {
            None
        } else {
            Some(Self(bytes))
        }
    }

    #[inline]
    pub const fn as_bytes(&self) -> &[u8; PROVIDER_ID_BYTES] {
        &self.0
    }
}

/// Stable identity of the machine verifying a login challenge.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct MachineId([u8; MACHINE_ID_BYTES]);

impl MachineId {
    #[inline]
    pub fn new(bytes: [u8; MACHINE_ID_BYTES]) -> Option<Self> {
        if bytes.iter().all(|byte| *byte == 0) {
            None
        } else {
            Some(Self(bytes))
        }
    }

    #[inline]
    pub const fn as_bytes(&self) -> &[u8; MACHINE_ID_BYTES] {
        &self.0
    }
}

/// Per-boot identity preventing a proof from being replayed after restart.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct BootId([u8; BOOT_ID_BYTES]);

impl BootId {
    #[inline]
    pub fn new(bytes: [u8; BOOT_ID_BYTES]) -> Option<Self> {
        if bytes.iter().all(|byte| *byte == 0) {
            None
        } else {
            Some(Self(bytes))
        }
    }

    #[inline]
    pub const fn as_bytes(&self) -> &[u8; BOOT_ID_BYTES] {
        &self.0
    }
}

/// Kernel account identifier. Human-readable names are account-layer metadata.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct AccountId(u64);

impl AccountId {
    pub const ROOT: Self = Self(0);

    #[inline]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    #[inline]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Privilege requested by a machine-login ceremony.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum MachineRole {
    User = 0,
    Administrator = 1,
    Root = 2,
}

/// Globally unambiguous reference to a provider-owned key.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KeyRef {
    pub provider: ProviderId,
    pub handle: KeyHandle,
}

impl KeyRef {
    #[inline]
    pub const fn new(provider: ProviderId, handle: KeyHandle) -> Self {
        Self { provider, handle }
    }
}

/// Algorithms that must participate in an operation for this key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyProfile {
    Single(AlgorithmId),
    Hybrid {
        classical: AlgorithmId,
        quantum_resistant: AlgorithmId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyProfileError {
    DuplicateAlgorithm,
    ClassicalComponentNotRecognized,
    QuantumComponentNotRecognized,
}

impl KeyProfile {
    #[inline]
    pub const fn single(algorithm: AlgorithmId) -> Self {
        Self::Single(algorithm)
    }

    /// Construct an all-components-required hybrid profile.
    ///
    /// There is intentionally no fallback flag: a provider must not silently
    /// downgrade a hybrid operation to its classical component.
    pub const fn hybrid(
        classical: AlgorithmId,
        quantum_resistant: AlgorithmId,
    ) -> Result<Self, KeyProfileError> {
        if classical.code() == quantum_resistant.code() {
            return Err(KeyProfileError::DuplicateAlgorithm);
        }
        if classical.is_quantum_resistant() {
            return Err(KeyProfileError::ClassicalComponentNotRecognized);
        }
        if !quantum_resistant.is_quantum_resistant() {
            return Err(KeyProfileError::QuantumComponentNotRecognized);
        }
        Ok(Self::Hybrid {
            classical,
            quantum_resistant,
        })
    }

    #[inline]
    pub const fn is_hybrid(self) -> bool {
        matches!(self, Self::Hybrid { .. })
    }
}

/// One authorization purpose that may be assigned to a key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum KeyPurpose {
    SshAuthentication = 0,
    SshNamespacedSignature = 1,
    EthereumAccount = 2,
    TrueOsArtifact = 3,
    MachineLogin = 4,
}

/// Compact purpose set stored in key descriptors and generation requests.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub struct KeyPurposeSet(u32);

impl KeyPurposeSet {
    pub const NONE: Self = Self(0);
    pub const SSH_AUTHENTICATION: Self = Self::only(KeyPurpose::SshAuthentication);
    pub const SSH_NAMESPACED_SIGNATURE: Self = Self::only(KeyPurpose::SshNamespacedSignature);
    pub const ETHEREUM_ACCOUNT: Self = Self::only(KeyPurpose::EthereumAccount);
    pub const TRUEOS_ARTIFACT: Self = Self::only(KeyPurpose::TrueOsArtifact);
    pub const MACHINE_LOGIN: Self = Self::only(KeyPurpose::MachineLogin);

    #[inline]
    pub const fn only(purpose: KeyPurpose) -> Self {
        Self(1u32 << purpose as u8)
    }

    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[inline]
    pub const fn contains(self, purpose: KeyPurpose) -> bool {
        (self.0 & Self::only(purpose).0) != 0
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub const fn bits(self) -> u32 {
        self.0
    }
}

impl BitOr for KeyPurposeSet {
    type Output = Self;

    #[inline]
    fn bitor(self, rhs: Self) -> Self::Output {
        self.union(rhs)
    }
}

impl BitOrAssign for KeyPurposeSet {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

/// Provider-neutral request for a newly generated key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeySpec {
    pub profile: KeyProfile,
    pub purposes: KeyPurposeSet,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeySpecError {
    NoPurpose,
    AlgorithmPurposeMismatch,
}

impl KeySpec {
    #[inline]
    pub const fn new(profile: KeyProfile, purposes: KeyPurposeSet) -> Self {
        Self { profile, purposes }
    }

    /// Check the invariants the shared layer can determine without consulting
    /// provider-specific policy.
    pub const fn validate(self) -> Result<(), KeySpecError> {
        if self.purposes.is_empty() {
            return Err(KeySpecError::NoPurpose);
        }

        if self.purposes.contains(KeyPurpose::EthereumAccount)
            && !matches!(self.profile, KeyProfile::Single(AlgorithmId::SECP256K1))
        {
            return Err(KeySpecError::AlgorithmPurposeMismatch);
        }

        match self.profile {
            KeyProfile::Single(AlgorithmId::ED25519 | AlgorithmId::RSA) => {}
            KeyProfile::Single(AlgorithmId::SECP256K1) => {
                if self.purposes.contains(KeyPurpose::SshAuthentication)
                    || self.purposes.contains(KeyPurpose::SshNamespacedSignature)
                    || self.purposes.contains(KeyPurpose::MachineLogin)
                {
                    return Err(KeySpecError::AlgorithmPurposeMismatch);
                }
            }
            KeyProfile::Single(AlgorithmId::ML_DSA_65) => {
                if self.purposes.contains(KeyPurpose::SshAuthentication)
                    || self.purposes.contains(KeyPurpose::SshNamespacedSignature)
                {
                    return Err(KeySpecError::AlgorithmPurposeMismatch);
                }
            }
            KeyProfile::Hybrid { .. }
                if self.purposes.contains(KeyPurpose::EthereumAccount)
                    || self.purposes.contains(KeyPurpose::SshAuthentication)
                    || self.purposes.contains(KeyPurpose::SshNamespacedSignature) =>
            {
                return Err(KeySpecError::AlgorithmPurposeMismatch);
            }
            _ => {}
        }

        Ok(())
    }
}

/// Public metadata for a provider-owned key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyDescriptor {
    pub key: KeyRef,
    pub profile: KeyProfile,
    pub purposes: KeyPurposeSet,
}

impl KeyDescriptor {
    #[inline]
    pub const fn permits(&self, purpose: KeyPurpose) -> bool {
        self.purposes.contains(purpose)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_identifiers_are_reserved() {
        assert!(KeyHandle::new([0; KEY_HANDLE_BYTES]).is_none());
        assert!(ProviderId::new([0; PROVIDER_ID_BYTES]).is_none());
        assert!(MachineId::new([0; MACHINE_ID_BYTES]).is_none());
        assert!(BootId::new([0; BOOT_ID_BYTES]).is_none());
        assert!(AlgorithmId::new(0).is_none());
    }

    #[test]
    fn key_references_are_provider_scoped() {
        let handle = KeyHandle::new([7; KEY_HANDLE_BYTES]).unwrap();
        let left = KeyRef::new(ProviderId::new([1; PROVIDER_ID_BYTES]).unwrap(), handle);
        let right = KeyRef::new(ProviderId::new([2; PROVIDER_ID_BYTES]).unwrap(), handle);
        assert_ne!(left, right);
    }

    #[test]
    fn ethereum_and_ssh_do_not_share_a_single_algorithm_profile() {
        let invalid_ssh = KeySpec::new(
            KeyProfile::single(AlgorithmId::SECP256K1),
            KeyPurposeSet::SSH_AUTHENTICATION,
        );
        let invalid_eth =
            KeySpec::new(KeyProfile::single(AlgorithmId::ED25519), KeyPurposeSet::ETHEREUM_ACCOUNT);
        assert_eq!(invalid_ssh.validate(), Err(KeySpecError::AlgorithmPurposeMismatch));
        assert_eq!(invalid_eth.validate(), Err(KeySpecError::AlgorithmPurposeMismatch));
    }

    #[test]
    fn hybrid_profiles_cannot_silently_downgrade() {
        let profile = KeyProfile::hybrid(AlgorithmId::ED25519, AlgorithmId::ML_DSA_65)
            .expect("known PQ companion");
        assert!(profile.is_hybrid());
        assert_eq!(
            KeyProfile::hybrid(AlgorithmId::ED25519, AlgorithmId::ED25519),
            Err(KeyProfileError::DuplicateAlgorithm)
        );
        assert_eq!(
            KeyProfile::hybrid(AlgorithmId::ML_DSA_65, AlgorithmId::ED25519),
            Err(KeyProfileError::ClassicalComponentNotRecognized)
        );
    }

    #[test]
    fn only_secp256k1_is_an_ethereum_account_profile() {
        for profile in [
            KeyProfile::single(AlgorithmId::RSA),
            KeyProfile::single(AlgorithmId::ML_DSA_65),
        ] {
            assert_eq!(
                KeySpec::new(profile, KeyPurposeSet::ETHEREUM_ACCOUNT).validate(),
                Err(KeySpecError::AlgorithmPurposeMismatch)
            );
        }
    }
}
