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

## Current non-goals

- no CLI commands or shell registration;
- no runtime service, executor task, or global state;
- no filesystem or `/crypt` persistence implementation;
- no cryptographic algorithm implementation;
- no mailbox/wire encoding;
- no hot-path integration.

Those layers can be added independently after the contracts settle.
