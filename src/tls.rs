#![allow(dead_code)]

//! TLS glue (planned).
//!
//! This module intentionally provides *stubs* only.
//!
//! Goal: define the platform-facing requirements needed to support TLS in TRUEOS
//! (no_std kernel) without committing to a specific implementation yet.
//!
//! The working proof is implemented separately in `crate::tls_demo` using rustls
//! directly.

extern crate alloc;

use alloc::vec::Vec;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum TlsError {
    Unsupported,
    InvalidConfig,
    HandshakeFailed,
    Io,
}

/// Minimal representation of a trust anchor / root CA set.
///
/// NOTE: This is a placeholder. The real implementation will likely wrap a
/// `rustls::RootCertStore` or a custom verifier.
pub struct TlsRoots {
    _priv: (),
}

/// Client-side TLS configuration.
///
/// NOTE: Placeholder. Real config will include:
/// - ALPN (e.g. h2, http/1.1)
/// - protocol versions
/// - server certificate verification policy
/// - client authentication (optional)
pub struct TlsClientConfig {
    _priv: (),
}

/// A platform TCP transport that TLS can run on top of.
///
/// Requirements for an implementation:
/// - Non-blocking or event-driven I/O is fine, but must provide a way for TLS to:
///   - push encrypted bytes to the network
///   - consume encrypted bytes received from the network
/// - Must support orderly close.
/// - Should tolerate partial sends/receives.
pub trait TlsTcpTransport {
    fn send_encrypted(&mut self, bytes: &[u8]) -> Result<usize, TlsError>;
    fn recv_encrypted(&mut self, out: &mut [u8]) -> Result<usize, TlsError>;
    fn close(&mut self) -> Result<(), TlsError>;
}

/// A platform RNG suitable for TLS key material.
///
/// Requirements:
/// - Cryptographically secure random bytes (CSPRNG).
/// - Must be available early in boot (or TLS must fail cleanly).
/// - Ideally backed by RDSEED/RDRAND + DRBG reseeding policy.
pub trait TlsRng {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), TlsError>;
}

/// A platform time source.
///
/// Requirements:
/// - Used for certificate validation (notBefore/notAfter).
/// - Needs UTC-ish wall clock OR an explicit policy to disable time validation.
pub trait TlsTime {
    fn unix_time_seconds(&self) -> Option<u64>;
}

/// Storage/provider for client keys (mTLS) and/or persisted session resumption.
///
/// Requirements:
/// - Optional: client cert + private key retrieval.
/// - Optional: session ticket / PSK storage.
pub trait TlsKeyStore {
    fn load_client_cert_chain_der(&self) -> Result<Option<Vec<Vec<u8>>>, TlsError>;
    fn load_client_key_der(&self) -> Result<Option<Vec<u8>>, TlsError>;
}

/// A future TLS client connection abstraction.
///
/// This will become the main API used by higher-level code (HTTPS, WS, etc).
///
/// Requirements for the future implementation (as comments for now):
/// - Needs a crypto provider (ring/aws-lc/rustcrypto) that works in `no_std`.
/// - Needs root store / verifier wiring.
/// - Needs an I/O pump integration with TRUEOS' net queues.
/// - Needs memory bounds to avoid unbounded heap growth.
pub struct TlsClient {
    _priv: (),
}

impl TlsClient {
    /// Create a TLS client.
    ///
    /// Requirements:
    /// - `server_name` must be a valid DNS name for SNI + verification.
    /// - `roots` must contain trust anchors for the peer.
    /// - `rng` must be a CSPRNG.
    /// - `time` must be available OR verification must be explicitly configured.
    pub fn new(
        _cfg: &TlsClientConfig,
        _roots: &TlsRoots,
        _server_name: &str,
        _rng: &mut dyn TlsRng,
        _time: &dyn TlsTime,
        _keys: Option<&dyn TlsKeyStore>,
    ) -> Result<Self, TlsError> {
        Err(TlsError::Unsupported)
    }

    /// Drive the TLS state machine by consuming encrypted bytes from the network.
    ///
    /// Requirements:
    /// - Must handle partial TLS records.
    /// - Must return decrypted plaintext for the caller to consume.
    pub fn ingest_encrypted(&mut self, _ciphertext: &[u8]) -> Result<Vec<u8>, TlsError> {
        Err(TlsError::Unsupported)
    }

    /// Provide plaintext to be encrypted and sent.
    ///
    /// Requirements:
    /// - Must buffer as needed and expose produced ciphertext for sending.
    pub fn write_plaintext(&mut self, _plaintext: &[u8]) -> Result<(), TlsError> {
        Err(TlsError::Unsupported)
    }

    /// Collect any pending ciphertext that should be sent over the transport.
    pub fn take_ciphertext_to_send(&mut self) -> Result<Vec<u8>, TlsError> {
        Err(TlsError::Unsupported)
    }

    /// True once the handshake is finished.
    pub fn is_connected(&self) -> bool {
        false
    }

    /// Begin an orderly shutdown.
    pub fn close_notify(&mut self) -> Result<(), TlsError> {
        Err(TlsError::Unsupported)
    }
}
