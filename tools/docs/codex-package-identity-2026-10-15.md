# Codex package identity in the header

On TRUEOS the title now shows the first eight lowercase hexadecimal digits of
the loaded `.bp` file's SHA-256, replacing the workspace version `v0.0.0`.
The published package for this change displays:

```text
OpenAI Codex (df598646)
```

The generic Blueprint environment builder exports `TRUEOS_APP_SHA256` from the
complete, immutable packed bytes retained for that launch. Both the host process
context and the guest invocation use those same bytes. It does not look up a
package name in the mutable catalog or hash the extracted executable. No new
syscall or exported ABI symbol is required. Existing child-launch environment
filtering keeps `TRUEOS_` metadata kernel-owned.

Codex caches this identity on its first title render. The shared title and raw
session header use it; normal CLI/API version fields retain the Cargo version.
An absent or malformed digest displays `package unknown`. Consequently a new
app launched on an older kernel is distinguishable from an identified package.
Other platforms retain the existing version display.

Verification passed: two Codex TUI unit tests; three production environment and
SHA-256 tests against Python's independent digest; seven existing application
environment tests; three timezone export tests. A host harness also executed
the actual TRUEOS display branch with a supplied and missing digest. The actual
TRUEOS Codex cross-build passed and published the package below. Remote SMB bytes
were read back and matched the local package hash.

```text
codex.bp SHA256:
df5986465616a0c971c9d65fa21fdf063d489e776e76d6d7807c136410179444

published:
smb://t4ce@pdjb/home-share/TRUEOS_SITE/apps/codex.bp§§df5986465616a0c971c9d65fa21fdf063d489e776e76d6d7807c136410179444

kernel image:
bld/codex-package-identity/trueos.iso

build logs:
/tmp/codex-package-header-build.log
/tmp/trueos-codex-package-identity-build.log
```

The kernel image also contains the timezone repair. Physical deployment/reset
is disabled for this isolated image build; the live rig was not rebooted.
The release build and ISO member verification passed. Exact artifact hashes are
recorded in `bld/codex-package-identity/manifest.json`:

```text
runtime ELF: 7373c782daba3bf9922e5b492d82c27431534f7f7e3bb57bca22ebd543883a6c
ISO:         2b773805a0d5c36530f6504de1a718c3a62c5bb91149c70f19731684e94d34ca
```
