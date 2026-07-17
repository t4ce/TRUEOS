//! Provider ownership boundary.

use crate::{AlgorithmId, KeyDescriptor, KeyProfile, KeyRef, KeySpec, ProviderId, SignRequest};

/// Isolation guarantee offered by a provider implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IsolationClass {
    /// Private material is isolated by software structure in the main system.
    Software,
    /// Operations execute in a separately protected execution realm.
    Realm,
    /// Private material is owned by an external hardware boundary.
    Hardware,
}

/// Where provider-owned key state survives between operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistenceClass {
    Volatile,
    Sealed,
    ProviderManaged,
}

/// Public provider metadata; it contains no key material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderInfo {
    pub id: ProviderId,
    pub isolation: IsolationClass,
    pub persistence: PersistenceClass,
}

/// Select one public component from a single or hybrid key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicKeyComponent {
    Primary,
    Classical,
    QuantumResistant,
}

/// Description of canonical public-key bytes written by a provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublicKeyOutput {
    pub algorithm: AlgorithmId,
    pub len: usize,
}

/// Encoding of signature bytes written by a provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureEncoding {
    AlgorithmNative,
    EthereumRecoverable,
    TrueOsHybrid,
}

/// Description of signature bytes written by a provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignatureOutput {
    pub profile: KeyProfile,
    pub encoding: SignatureEncoding,
    pub len: usize,
}

/// Errors deliberately shared across software, realm, and hardware providers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderError {
    Unsupported,
    InvalidKeySpec,
    KeyNotFound,
    WrongProvider,
    PurposeDenied,
    InvalidIntent,
    OutputTooSmall { required: usize },
    Busy,
    Unavailable,
    Internal,
}

/// Semantic key-provider boundary.
///
/// This trait intentionally does not choose an executor, mailbox, or wire
/// codec. A future service may call a local implementation directly or expose
/// a realm/hardware proxy with the same semantics. No method can export private
/// key bytes.
pub trait KeyProvider {
    fn info(&self) -> ProviderInfo;

    fn supports(&self, spec: &KeySpec) -> bool;

    async fn generate(&mut self, spec: KeySpec) -> Result<KeyDescriptor, ProviderError>;

    async fn describe(&mut self, key: KeyRef) -> Result<KeyDescriptor, ProviderError>;

    async fn public_key(
        &mut self,
        key: KeyRef,
        component: PublicKeyComponent,
        output: &mut [u8],
    ) -> Result<PublicKeyOutput, ProviderError>;

    async fn sign(
        &mut self,
        request: SignRequest<'_>,
        output: &mut [u8],
    ) -> Result<SignatureOutput, ProviderError>;

    #[inline]
    fn owns(&self, key: KeyRef) -> bool {
        self.info().id == key.provider
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        KeyHandle, KeyPurposeSet, ProviderId, identity::KEY_HANDLE_BYTES, intent::SignIntent,
    };
    use core::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };

    fn run_ready<F: Future>(future: F) -> F::Output {
        let mut future = pin!(future);
        let mut context = Context::from_waker(Waker::noop());
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("contract provider unexpectedly returned Pending"),
        }
    }

    struct ContractProvider {
        info: ProviderInfo,
        descriptor: KeyDescriptor,
    }

    impl KeyProvider for ContractProvider {
        fn info(&self) -> ProviderInfo {
            self.info
        }

        fn supports(&self, spec: &KeySpec) -> bool {
            spec.validate().is_ok()
        }

        async fn generate(&mut self, spec: KeySpec) -> Result<KeyDescriptor, ProviderError> {
            if !self.supports(&spec) {
                return Err(ProviderError::InvalidKeySpec);
            }
            self.descriptor.profile = spec.profile;
            self.descriptor.purposes = spec.purposes;
            Ok(self.descriptor)
        }

        async fn describe(&mut self, key: KeyRef) -> Result<KeyDescriptor, ProviderError> {
            if !self.owns(key) {
                return Err(ProviderError::WrongProvider);
            }
            Ok(self.descriptor)
        }

        async fn public_key(
            &mut self,
            key: KeyRef,
            _: PublicKeyComponent,
            output: &mut [u8],
        ) -> Result<PublicKeyOutput, ProviderError> {
            if !self.owns(key) {
                return Err(ProviderError::WrongProvider);
            }
            if output.len() < 32 {
                return Err(ProviderError::OutputTooSmall { required: 32 });
            }
            output[..32].fill(9);
            Ok(PublicKeyOutput {
                algorithm: AlgorithmId::ED25519,
                len: 32,
            })
        }

        async fn sign(
            &mut self,
            request: SignRequest<'_>,
            output: &mut [u8],
        ) -> Result<SignatureOutput, ProviderError> {
            if !self.owns(request.key) {
                return Err(ProviderError::WrongProvider);
            }
            if !self.descriptor.permits(request.intent.required_purpose()) {
                return Err(ProviderError::PurposeDenied);
            }
            if output.len() < 64 {
                return Err(ProviderError::OutputTooSmall { required: 64 });
            }
            output[..64].fill(3);
            Ok(SignatureOutput {
                profile: self.descriptor.profile,
                encoding: SignatureEncoding::AlgorithmNative,
                len: 64,
            })
        }
    }

    fn provider() -> ContractProvider {
        let provider_id = ProviderId::new([1; 16]).unwrap();
        let key = KeyRef::new(provider_id, KeyHandle::new([2; KEY_HANDLE_BYTES]).unwrap());
        ContractProvider {
            info: ProviderInfo {
                id: provider_id,
                isolation: IsolationClass::Realm,
                persistence: PersistenceClass::Sealed,
            },
            descriptor: KeyDescriptor {
                key,
                profile: KeyProfile::single(AlgorithmId::ED25519),
                purposes: KeyPurposeSet::SSH_AUTHENTICATION,
            },
        }
    }

    #[test]
    fn provider_rejects_cross_purpose_signing() {
        let mut provider = provider();
        let request = SignRequest::new(
            provider.descriptor.key,
            SignIntent::EthereumPersonalMessage { message: b"gm" },
        );
        let mut signature = [0u8; 65];
        assert_eq!(
            run_ready(provider.sign(request, &mut signature)),
            Err(ProviderError::PurposeDenied)
        );
    }

    #[test]
    fn provider_contract_never_returns_private_material() {
        let mut provider = provider();
        let mut public = [0u8; 32];
        let result = run_ready(provider.public_key(
            provider.descriptor.key,
            PublicKeyComponent::Primary,
            &mut public,
        ))
        .unwrap();
        assert_eq!(result.len, public.len());
        assert_eq!(public, [9; 32]);
    }
}
