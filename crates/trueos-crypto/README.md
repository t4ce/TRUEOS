# trueos-crypto

`trueos-crypto` is the provider-neutral contract for keys owned by an isolated
signer. It is deliberately a small, inert `no_std` crate.

## The three core concepts

1. **Identity** — `KeyRef` combines an opaque `KeyHandle` with its owning
   `ProviderId`. Public metadata describes algorithms and permitted purposes;
   private bytes have no representation in the API.
2. **Intent** — `SignIntent` gives a request its SSH, Ethereum, or native
   TRUEOS meaning. There is no raw-digest escape hatch.
3. **Provider** — `KeyProvider` owns generation, persistence, public-key access,
   and signing. `IsolationClass` permits software, realm, and hardware-backed
   implementations behind the same boundary.

## Future ecosystem halves

```text
SSH adapter --------> SignIntent -----> KeyProvider
Ethereum adapter ---> SignIntent -----> KeyProvider
```

The SSH half will own OpenSSH wire encoding, SSH authentication transcripts,
and SSHSIG compatibility. The Ethereum half will own addresses, transaction
encoding, EIP-191, and EIP-712 presentation. Neither half owns or receives a
private key.

Local account authentication is represented separately by
`MachineLoginChallenge` and `SignIntent::MachineLogin`. Account and role
authorization remain outside the cryptographic provider.

The crate also contains the narrow RFC 6238 SHA-1 TOTP compatibility primitive
needed by common authenticator applications. TOTP enrollment state, secret
storage, retry policy, and account authorization remain runtime responsibilities.

## SHA-2 dependency policy

Repository-owned SHA-2 dependencies in TRUEOS, Buildins, and Blueprints stay
pinned to `=0.10.9`. Upgrading only our hashing calls to SHA-2 0.11 leaves the
existing crypto consumers on 0.10 and keeps both digest dependency families;
that isolated upgrade does not meet our dependency-consolidation goal.

Before lifting the pin, coordinate the migration of Sunset and
rustls-rustcrypto, including their ed25519-dalek, P-256/P-384, and RSA consumers,
onto the newer digest ecosystem. SHA-1, HMAC, and HKDF users must use compatible
digest traits too. Removing the remaining lower-level duplicates also requires
movement in AES/AEAD/cipher consumers of crypto-common/cpufeatures and the
DER/SPKI/PKCS consumers of const-oid. Check the kernel's actual target dependency
graph rather than treating a direct SHA-2 version bump as consolidation.

`trueos-lily-pack` uses a vendored `lzma-rust2 0.21` whose SHA-2 requirement is
ported to `=0.10.9`. Its XZ decoder and checksum validation are unchanged. This
keeps Lily compression on the existing crypto stack instead of reintroducing
SHA-2 0.11 through upstream's `xz` feature. Preserve that alignment when updating
the compression dependency; do not disable checksum validation to hide a new
dependency branch.

## Current non-goals

- no CLI commands or shell registration;
- no runtime service, executor task, or global state;
- no filesystem or `/crypt` persistence implementation;
- no signing-key algorithm implementation;
- no mailbox/wire encoding;
- no hot-path integration.

Those layers can be added independently after the contracts settle.
