//! Typed signing intentions shared by protocol adapters and key providers.

use crate::{AccountId, BootId, KeyPurpose, KeyRef, MachineId, MachineRole};

const MACHINE_LOGIN_DOMAIN: [u8; 16] = *b"TRUEOS-LOGIN-V1\0";
pub const MACHINE_LOGIN_CHALLENGE_BYTES: usize = 136;

/// Fresh, machine-bound proof request for a local account login.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MachineLoginChallenge {
    machine: MachineId,
    boot: BootId,
    account: AccountId,
    requested_role: MachineRole,
    issued_at_ticks: u64,
    expires_at_ticks: u64,
    sequence: u64,
    nonce: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MachineLoginChallengeError {
    EmptyNonce,
    InvalidLifetime,
}

impl MachineLoginChallenge {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        machine: MachineId,
        boot: BootId,
        account: AccountId,
        requested_role: MachineRole,
        issued_at_ticks: u64,
        expires_at_ticks: u64,
        sequence: u64,
        nonce: [u8; 32],
    ) -> Result<Self, MachineLoginChallengeError> {
        if nonce.iter().all(|byte| *byte == 0) {
            return Err(MachineLoginChallengeError::EmptyNonce);
        }
        if expires_at_ticks <= issued_at_ticks {
            return Err(MachineLoginChallengeError::InvalidLifetime);
        }
        Ok(Self {
            machine,
            boot,
            account,
            requested_role,
            issued_at_ticks,
            expires_at_ticks,
            sequence,
            nonce,
        })
    }

    #[inline]
    pub const fn machine(&self) -> MachineId {
        self.machine
    }

    #[inline]
    pub const fn boot(&self) -> BootId {
        self.boot
    }

    #[inline]
    pub const fn account(&self) -> AccountId {
        self.account
    }

    #[inline]
    pub const fn requested_role(&self) -> MachineRole {
        self.requested_role
    }

    #[inline]
    pub const fn issued_at_ticks(&self) -> u64 {
        self.issued_at_ticks
    }

    #[inline]
    pub const fn expires_at_ticks(&self) -> u64 {
        self.expires_at_ticks
    }

    #[inline]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    #[inline]
    pub const fn is_valid_at(&self, now_ticks: u64) -> bool {
        now_ticks >= self.issued_at_ticks && now_ticks <= self.expires_at_ticks
    }

    /// Canonical, domain-separated bytes signed by the credential provider.
    pub fn encode(&self) -> [u8; MACHINE_LOGIN_CHALLENGE_BYTES] {
        let mut out = [0u8; MACHINE_LOGIN_CHALLENGE_BYTES];
        out[0..16].copy_from_slice(&MACHINE_LOGIN_DOMAIN);
        out[16..48].copy_from_slice(self.machine.as_bytes());
        out[48..64].copy_from_slice(self.boot.as_bytes());
        out[64..72].copy_from_slice(&self.account.raw().to_le_bytes());
        out[72] = self.requested_role as u8;
        // [73..80] is reserved and remains zero for compatible extensions.
        out[80..88].copy_from_slice(&self.issued_at_ticks.to_le_bytes());
        out[88..96].copy_from_slice(&self.expires_at_ticks.to_le_bytes());
        out[96..104].copy_from_slice(&self.sequence.to_le_bytes());
        out[104..136].copy_from_slice(&self.nonce);
        out
    }
}

/// The upper ecosystem that gave a signing request its meaning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntentFamily {
    Ssh,
    Ethereum,
    TrueOs,
    Machine,
}

/// A signing operation with protocol meaning attached.
///
/// There is deliberately no `RawDigest` variant. Adding a new protocol means
/// adding an explicit intent with enough context for policy and confirmation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignIntent<'a> {
    /// Canonical SSH authentication data prepared by the SSH transport.
    SshAuthentication { signed_data: &'a [u8] },
    /// A namespaced SSHSIG-style signature over a message.
    SshNamespaced {
        namespace: &'a str,
        message: &'a [u8],
    },
    /// An EIP-191 personal message. The provider, not the caller, applies the
    /// Ethereum prefix and hashing rules.
    EthereumPersonalMessage { message: &'a [u8] },
    /// The two semantic hashes used by an EIP-712 signing operation. The
    /// provider constructs and hashes the final EIP-712 preimage.
    EthereumTypedData {
        domain_separator: [u8; 32],
        struct_hash: [u8; 32],
    },
    /// Canonical unsigned Ethereum transaction payload and its chain domain.
    EthereumTransaction {
        chain_id: u64,
        unsigned_payload: &'a [u8],
    },
    /// Native TRUEOS artifact signing, including future hybrid signatures.
    TrueOsArtifact {
        namespace: &'a str,
        message: &'a [u8],
    },
    /// Proof of possession for a local machine account and requested role.
    MachineLogin {
        challenge: &'a MachineLoginChallenge,
    },
}

impl SignIntent<'_> {
    #[inline]
    pub const fn family(&self) -> IntentFamily {
        match self {
            Self::SshAuthentication { .. } | Self::SshNamespaced { .. } => IntentFamily::Ssh,
            Self::EthereumPersonalMessage { .. }
            | Self::EthereumTypedData { .. }
            | Self::EthereumTransaction { .. } => IntentFamily::Ethereum,
            Self::TrueOsArtifact { .. } => IntentFamily::TrueOs,
            Self::MachineLogin { .. } => IntentFamily::Machine,
        }
    }

    #[inline]
    pub const fn required_purpose(&self) -> KeyPurpose {
        match self {
            Self::SshAuthentication { .. } => KeyPurpose::SshAuthentication,
            Self::SshNamespaced { .. } => KeyPurpose::SshNamespacedSignature,
            Self::EthereumPersonalMessage { .. }
            | Self::EthereumTypedData { .. }
            | Self::EthereumTransaction { .. } => KeyPurpose::EthereumAccount,
            Self::TrueOsArtifact { .. } => KeyPurpose::TrueOsArtifact,
            Self::MachineLogin { .. } => KeyPurpose::MachineLogin,
        }
    }
}

/// Complete request submitted across the isolated provider boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignRequest<'a> {
    pub key: KeyRef,
    pub intent: SignIntent<'a>,
}

impl<'a> SignRequest<'a> {
    #[inline]
    pub const fn new(key: KeyRef, intent: SignIntent<'a>) -> Self {
        Self { key, intent }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intents_carry_their_authorization_domain() {
        let ssh = SignIntent::SshNamespaced {
            namespace: "source-release",
            message: b"artifact",
        };
        let eth = SignIntent::EthereumTransaction {
            chain_id: 1,
            unsigned_payload: b"canonical transaction",
        };

        assert_eq!(ssh.family(), IntentFamily::Ssh);
        assert_eq!(ssh.required_purpose(), KeyPurpose::SshNamespacedSignature);
        assert_eq!(eth.family(), IntentFamily::Ethereum);
        assert_eq!(eth.required_purpose(), KeyPurpose::EthereumAccount);
    }

    #[test]
    fn machine_login_challenge_is_fresh_and_canonically_bound() {
        let challenge = MachineLoginChallenge::new(
            MachineId::new([1; 32]).unwrap(),
            BootId::new([2; 16]).unwrap(),
            AccountId::ROOT,
            MachineRole::Root,
            100,
            200,
            7,
            [3; 32],
        )
        .unwrap();

        let encoded = challenge.encode();
        assert_eq!(&encoded[..16], b"TRUEOS-LOGIN-V1\0");
        assert_eq!(&encoded[64..72], &AccountId::ROOT.raw().to_le_bytes());
        assert_eq!(encoded[72], MachineRole::Root as u8);
        assert_eq!(&encoded[96..104], &7u64.to_le_bytes());
        assert!(challenge.is_valid_at(150));
        assert!(!challenge.is_valid_at(201));

        let intent = SignIntent::MachineLogin {
            challenge: &challenge,
        };
        assert_eq!(intent.family(), IntentFamily::Machine);
        assert_eq!(intent.required_purpose(), KeyPurpose::MachineLogin);
    }
}
