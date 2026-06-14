# Third-Party Notices

This repository contains third-party code, vendored dependencies, imported
reference material, generated artifacts, and tool/build components. Those
components are not licensed under the TRUEOS Source-Available Public View
License unless explicitly stated.

Known third-party/provenance areas include:

- `vendor/`: vendored Rust crates and external projects, with their license
  files preserved inside each vendored package.
- `.limine/` and `vendor/limine/`: Limine bootloader material, governed by the
  Limine licenses and notices included in those directories.
- `src/t/th/trust_thread_onlyasreference/`: TrustOS-derived reference and
  extraction material documented in that directory.
- `crates/trueos-lsd/`: Apache-2.0 licensed material with upstream repository
  provenance declared in its Cargo manifest.
- Files with embedded license headers or bundled license notices, including
  JavaScript modules under `crates/trueos-qjs/src/`.
- Build or release artifacts under `bld/`, where present, may include their own
  third-party license notices such as OVMF/QEMU-related notices.

When distributing any TRUEOS build, ISO, image, archive, or derived artifact,
include the applicable third-party license files and notices for all bundled
components. This file is a human-readable index, not a complete replacement for
the original third-party license texts.
