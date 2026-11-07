Vendored from the crates.io `sunset` 0.5.0 release (0BSD; see LICENSE).

TRUEOS extensions:

- `alloc`/`std` forward allocation support to `ssh-key` only when that optional
  dependency is already enabled; allocation alone does not enable the unused
  OpenSSH key-file parser.

- RFC 4256 server keyboard-interactive authentication with one non-echoed
  authenticator-code prompt and exactly one response. Ordinary password and
  public-key methods remain independently configurable.
- The existing PasswordAuth event exposes the code response. `defer()` retains
  that event until `finish_deferred_auth()` resolves a durable verification.
  The application must not call `progress()` while authentication is deferred.
- Public server authentication-method configuration and graceful session exit.
- PTY dimensions and window-change notifications exposed to the server application.
- Authentication responses redact Debug output; decoding errors omit raw bytes.

- RFC 4256 client authentication preserves challenge names, instructions, echo
  flags, zero or multiple prompts, and repeated rounds. Allocating clients use
  fallible metadata storage bounded by the received packet; allocation-free
  configurations support 32 prompts. Response counts must match exactly.
- Client method selection follows the server's advertised remaining methods.
  Partial success advances authentication factors without reporting success or
  reoffering the completed factor. Only USERAUTH_SUCCESS authenticates.
- PTY requests encode their supplied terminal modes. Client PTY/shell requests
  can be retried independently of the SessionOpened guard. Shared EOF/close
  helpers track sent state and prevent subsequent local channel data.
- The initial-key-exchange query reflects installed receive encryption keys,
  and short output copies preserve the caller's untouched suffix.
- Optional ECDSA covers NIST P-256, P-384 and P-521 using the stable RustCrypto
  0.13 family already used by TRUEOS TLS. Per-curve SSH signatures use SHA-256,
  SHA-384 and SHA-512 respectively. Incoming alloc-only builds do not activate
  ECDSA or the optional key-file codec.
- Optional key import preserves unencrypted OpenSSH, PPK v2/v3, PKCS8 and SEC1
  identities. PPK authentication codes and derived public keys are verified;
  explicit SEC1 domains must match the complete supported NIST parameters.
  Private import buffers are zeroized. Key-file parsing remains separate from
  the protocol's stable cryptographic backends.

`src/native_protocol_tests.rs` exercises the existing incoming Ed25519 and
durably deferred authenticator policy over real encrypted, fragmented traffic.
Client method/round regressions live in `src/cliauth.rs`; signature and key-file
regressions live beside their implementations.
