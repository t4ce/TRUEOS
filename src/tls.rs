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

use alloc::sync::Arc;
use alloc::boxed::Box;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use rustls::time_provider::TimeProvider;
use spin::Once;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum TlsError {
    Unsupported,
    InvalidConfig,
    HandshakeFailed,
    Io,
}

/// Trust anchors / root CA set.
pub struct TlsRoots {
    store: Arc<rustls::RootCertStore>,
}

impl TlsRoots {
    pub fn empty() -> Self {
        Self {
            store: Arc::new(rustls::RootCertStore::empty()),
        }
    }

    /// Mozilla root set via `webpki-roots`.
    pub fn mozilla() -> Self {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Self {
            store: Arc::new(roots),
        }
    }

    /// Build a root store from a list of DER-encoded certs.
    pub fn from_der_certs(certs: &[&[u8]]) -> Result<Self, TlsError> {
        let mut roots = rustls::RootCertStore::empty();
        for der in certs {
            let der = rustls::pki_types::CertificateDer::from((*der).to_vec());
            roots
                .add(der)
                .map_err(|_| TlsError::InvalidConfig)?;
        }
        Ok(Self {
            store: Arc::new(roots),
        })
    }
}

/// Client-side TLS configuration.
pub struct TlsClientConfig {
    /// ALPN protocols (e.g. "h2", "http/1.1").
    pub alpn: Vec<Vec<u8>>,
    /// If true, reject connections when the time source is unavailable.
    pub require_time: bool,
}

impl Default for TlsClientConfig {
    fn default() -> Self {
        Self {
            alpn: Vec::new(),
            require_time: true,
        }
    }
}

impl TlsClientConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_alpn_protocols(mut self, protos: &[&[u8]]) -> Self {
        self.alpn.clear();
        self.alpn
            .extend(protos.iter().map(|p| (*p).to_vec()));
        self
    }
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
    conn: rustls::client::UnbufferedClientConnection,
    incoming_tls: Vec<u8>,
    outgoing_tls: Vec<u8>,
    pending_plaintext: Vec<u8>,
    connected: bool,
    closed: bool,
}

static TLS_PROVIDER_ONCE: Once<()> = Once::new();

#[derive(Debug)]
struct TimeProviderAdapter {
    time: *const dyn TlsTime,
}

// Safety: this is only used during the lifetime of the borrowed `TlsTime` passed
// into `TlsClient::new`, which we currently require to be 'static.
unsafe impl Send for TimeProviderAdapter {}
unsafe impl Sync for TimeProviderAdapter {}

impl rustls::time_provider::TimeProvider for TimeProviderAdapter {
    fn current_time(&self) -> Option<rustls::pki_types::UnixTime> {
        let time = unsafe { &*self.time };
        let secs = time.unix_time_seconds()?;
        Some(rustls::pki_types::UnixTime::since_unix_epoch(
            core::time::Duration::from_secs(secs),
        ))
    }
}

fn ensure_rustls_provider_installed() {
    TLS_PROVIDER_ONCE.call_once(|| {
        let _ = rustls::crypto::CryptoProvider::install_default(rustls_rustcrypto::provider());
    });
}

fn leak_str(s: alloc::string::String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn pump(
    conn: &mut rustls::client::UnbufferedClientConnection,
    incoming_tls: &mut Vec<u8>,
    outgoing_tls: &mut Vec<u8>,
    pending_plaintext: &mut Vec<u8>,
    connected: &mut bool,
    closed: &mut bool,
) -> Result<Vec<u8>, TlsError> {
    use rustls::unbuffered::{ConnectionState, EncodeError, EncryptError};

    let mut plaintext: Vec<u8> = Vec::new();

    loop {
        let status = conn.process_tls_records(incoming_tls.as_mut_slice());
        let mut discard = status.discard;

        let state = status.state.map_err(|_| TlsError::HandshakeFailed)?;
        match state {
            ConnectionState::EncodeTlsData(mut enc) => {
                let mut out = [0u8; 4096];
                let chunk = match enc.encode(&mut out) {
                    Ok(n) => out[..n].to_vec(),
                    Err(EncodeError::InsufficientSize(e)) => {
                        let mut v = vec![0u8; e.required_size];
                        let n = enc.encode(&mut v).map_err(|_| TlsError::HandshakeFailed)?;
                        v.truncate(n);
                        v
                    }
                    Err(_) => return Err(TlsError::HandshakeFailed),
                };
                outgoing_tls.extend_from_slice(&chunk);
            }
            ConnectionState::TransmitTlsData(tx) => {
                // We don't send here; the caller drains `outgoing_tls`.
                tx.done();
            }
            ConnectionState::ReadTraffic(mut rt) => {
                while let Some(rec) = rt.next_record() {
                    let rec = rec.map_err(|_| TlsError::HandshakeFailed)?;
                    discard = discard.saturating_add(rec.discard);
                    plaintext.extend_from_slice(rec.payload);
                }
            }
            ConnectionState::WriteTraffic(mut wt) => {
                *connected = true;
                if !pending_plaintext.is_empty() {
                    let mut out = [0u8; 4096];
                    let chunk = match wt.encrypt(pending_plaintext.as_slice(), &mut out) {
                        Ok(n) => out[..n].to_vec(),
                        Err(EncryptError::InsufficientSize(e)) => {
                            let mut v = vec![0u8; e.required_size];
                            let n = wt
                                .encrypt(pending_plaintext.as_slice(), &mut v)
                                .map_err(|_| TlsError::HandshakeFailed)?;
                            v.truncate(n);
                            v
                        }
                        Err(_) => return Err(TlsError::HandshakeFailed),
                    };
                    outgoing_tls.extend_from_slice(&chunk);
                    pending_plaintext.clear();
                } else {
                    // Ready for app data, but none queued.
                    break;
                }
            }
            ConnectionState::ReadEarlyData(_) | ConnectionState::BlockedHandshake => {
                break;
            }
            ConnectionState::PeerClosed | ConnectionState::Closed => {
                *closed = true;
                break;
            }
            _ => break,
        }

        if discard > 0 {
            let discard = discard.min(incoming_tls.len());
            incoming_tls.drain(0..discard);
        }
    }

    Ok(plaintext)
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
        cfg: &TlsClientConfig,
        roots: &TlsRoots,
        server_name: &str,
        _rng: &mut dyn TlsRng,
        time: &'static dyn TlsTime,
        _keys: Option<&dyn TlsKeyStore>,
    ) -> Result<Self, TlsError> {
        ensure_rustls_provider_installed();

        let server_name_static = leak_str(server_name.to_string());
        let server_name = rustls::pki_types::ServerName::try_from(server_name_static)
            .map_err(|_| TlsError::InvalidConfig)?;

        let time_provider = Arc::new(TimeProviderAdapter {
            time: time as *const dyn TlsTime,
        });

        if cfg.require_time && TimeProvider::current_time(time_provider.as_ref()).is_none() {
            return Err(TlsError::InvalidConfig);
        }

        let provider = Arc::new(rustls_rustcrypto::provider());
        let mut config = rustls::client::ClientConfig::builder_with_details(provider, time_provider)
            .with_safe_default_protocol_versions()
            .map_err(|_| TlsError::InvalidConfig)?
            .with_root_certificates(roots.store.clone())
            .with_no_client_auth();

        if !cfg.alpn.is_empty() {
            config.alpn_protocols = cfg.alpn.clone();
        }

        let config = Arc::new(config);
        let mut conn = rustls::client::UnbufferedClientConnection::new(config, server_name)
            .map_err(|_| TlsError::HandshakeFailed)?;

        // Generate the initial ClientHello immediately so callers can send it.
        let mut incoming_tls = Vec::new();
        let mut outgoing_tls = Vec::new();
        let mut pending_plaintext = Vec::new();
        let mut connected = false;
        let mut closed = false;
        let _ = pump(
            &mut conn,
            &mut incoming_tls,
            &mut outgoing_tls,
            &mut pending_plaintext,
            &mut connected,
            &mut closed,
        )?;

        Ok(Self {
            conn,
            incoming_tls,
            outgoing_tls,
            pending_plaintext,
            connected,
            closed,
        })
    }

    /// Drive the TLS state machine by consuming encrypted bytes from the network.
    ///
    /// Requirements:
    /// - Must handle partial TLS records.
    /// - Must return decrypted plaintext for the caller to consume.
    pub fn ingest_encrypted(&mut self, _ciphertext: &[u8]) -> Result<Vec<u8>, TlsError> {
        if self.closed {
            return Ok(Vec::new());
        }
        if !_ciphertext.is_empty() {
            self.incoming_tls.extend_from_slice(_ciphertext);
        }
        pump(
            &mut self.conn,
            &mut self.incoming_tls,
            &mut self.outgoing_tls,
            &mut self.pending_plaintext,
            &mut self.connected,
            &mut self.closed,
        )
    }

    /// Provide plaintext to be encrypted and sent.
    ///
    /// Requirements:
    /// - Must buffer as needed and expose produced ciphertext for sending.
    pub fn write_plaintext(&mut self, _plaintext: &[u8]) -> Result<(), TlsError> {
        if self.closed {
            return Err(TlsError::Io);
        }
        if _plaintext.is_empty() {
            return Ok(());
        }

        self.pending_plaintext.extend_from_slice(_plaintext);
        // Attempt to encrypt immediately if the connection is ready.
        let _ = pump(
            &mut self.conn,
            &mut self.incoming_tls,
            &mut self.outgoing_tls,
            &mut self.pending_plaintext,
            &mut self.connected,
            &mut self.closed,
        )?;
        Ok(())
    }

    /// Collect any pending ciphertext that should be sent over the transport.
    pub fn take_ciphertext_to_send(&mut self) -> Result<Vec<u8>, TlsError> {
        if self.outgoing_tls.is_empty() {
            return Ok(Vec::new());
        }
        Ok(core::mem::take(&mut self.outgoing_tls))
    }

    /// True once the handshake is finished.
    pub fn is_connected(&self) -> bool {
        self.connected && !self.closed
    }

    /// Begin an orderly shutdown.
    pub fn close_notify(&mut self) -> Result<(), TlsError> {
        // Proper close_notify wiring for unbuffered rustls is TODO.
        // For now, mark closed; callers should close the underlying transport.
        self.closed = true;
        Ok(())
    }
}
