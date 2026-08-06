# Warning-cleanup archive

This directory records the warning baseline captured by `make iso` before the
warning-cleanup campaign. It is outside the Cargo workspace and the kernel
module tree: archived Rust files stored here are historical source, not build
inputs.

The baseline source commit is recorded in `BASE_COMMIT`. `WARNINGS.md` is the
human-readable inventory, grouped by warning cause and reduced to one row per
cause/source-file pair. `manifest.tsv` is the machine-readable, per-source
archive plan. The captured diagnostic inventory contained 1,916 warning
records across 222 source files plus one build-script diagnostic group. Cargo's
eleven `generated ... warning(s)` summary lines are not counted.

## Scope labels

- `kernel`: the root package and files below `src/`.
- `workspace`: first-party path crates below `crates/`.
- `vendor`: checked-in third-party sources below `vendor/`.
- `build-script`: diagnostics without a Rust source span, currently the kernel
  C-ABI export/declaration mismatch emitted by `build.rs`.

Vendor warnings are kept separate because changing vendored code is a distinct
maintenance decision. A vendor row is not permission to delete upstream code.
Likewise, a C-ABI mismatch is contract work rather than ordinary dead code:
`build.rs` scans live `src/**/*.rs`, compares exports with
`crates/trueos-v/src/bp_abi.rs`, and verifies `abi/portal-cabi-v2.sha256`.

## Archiving policy

The initial per-file snapshot plan remains in `manifest.tsv`; its `pending`
rows are deliberately not presented as completed snapshots. Reviewed removals
are stored as reversible patches instead. `DEAD_CODE_EXPECTATIONS.patch`
records the item-local lint expectations used for dormant-but-retained code,
and `dead_code_expectations.json` records every planned, skipped, and unresolved
compiler span. New warnings remain visible outside those exact items.

For a future whole-file removal, archive the file directly. For a partial
cleanup, prefer a reversible per-file patch with its source commit recorded.
New dead-code analysis must exclude `tools/warnings_last/` (and the older
`tools/intel_leftovers/`) so archived source does not pollute live-code results.

Cross-repository C-ABI cleanup is archived as reversible, per-file patches
under `TRUEOS/` and `TRUEOS-Blueprints/`. Those prefixes identify the owning
repository and avoid conflating the two SDK copies. See `CABI_REMOVALS.md` for
the reviewed symbol set and both source commits.

`src/intel/sound/hda.rs` and `src/intel/gpgpu/` are explicit retained-subsystem
exceptions requested during cleanup. HDA carries a file-scoped dead-code
allow; GPGPU carries a dead-code allow on its module declaration. No source
from either subsystem was extracted.

## Capture provenance

- Command: `make iso`
- Capture time: 2026-08-06 02:13:28 +02:00
- Rust compiler: `rustc 1.99.0-nightly (af3d95584 2026-07-09)`
- Parsed inventory SHA-256:
  `fe443008d7c48f1b5e1cc10ac23ef26529460ca146de0c64367b54ba154dfc96`
- Raw build log SHA-256:
  `44252f7e0922513759b5f98fa2bccdf1c4abec333d835359ebe63d0b6a483427`

The raw capture remains a temporary working artifact under
`.codex_tmp/warnings_inventory/`; the tracked table and manifest are the stable
archive index.
