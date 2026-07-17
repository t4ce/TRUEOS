#![no_std]
#![forbid(unsafe_code)]
#![allow(async_fn_in_trait)]

//! Provider-neutral contracts for keys owned by an isolated signer.
//!
//! This crate deliberately contains no cryptographic implementation, runtime
//! service, filesystem integration, or protocol adapter. Its public surface is
//! the narrow seam shared by future SSH and Ethereum layers:
//!
//! 1. [`identity`] names a key without exposing private material.
//! 2. [`intent`] describes the operation the caller wants authorized.
//! 3. [`provider`] owns generation, persistence, public keys, and signing.
//!
//! Private key bytes are intentionally absent from the API. A provider may be
//! software-backed, an isolated execution realm, or external hardware without
//! changing the consumers above it.

pub mod identity;
pub mod intent;
pub mod provider;

pub use identity::{
    AccountId, AlgorithmId, BootId, KeyDescriptor, KeyHandle, KeyProfile, KeyProfileError,
    KeyPurpose, KeyPurposeSet, KeyRef, KeySpec, KeySpecError, MachineId, MachineRole, ProviderId,
};
pub use intent::{
    IntentFamily, MACHINE_LOGIN_CHALLENGE_BYTES, MachineLoginChallenge, MachineLoginChallengeError,
    SignIntent, SignRequest,
};
pub use provider::{
    IsolationClass, KeyProvider, PersistenceClass, ProviderError, ProviderInfo, PublicKeyComponent,
    PublicKeyOutput, SignatureEncoding, SignatureOutput,
};
