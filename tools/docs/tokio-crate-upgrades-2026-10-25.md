# Tokio upgrade addendum — 2026-10-25

**Tokio 1.52.3 → 1.52.4 is a small candidate: two upstream source files change, neither overlaps the local port, and the three-way source merge has zero conflicts.** The latest stable release is 1.54.1, which requires more porting. `tokio-macros` and `tokio-util` are strong unvendor candidates because their current package sources and manifests match published upstream releases.

Tokio is absent from both selected kernel graphs in the kernel audit. Its fork lives in `TRUEOS-Blueprints/vendor/tokio-1.52.3`; the kernel uses trueos-executor. The existing 2026-10-22 Blueprint graph inventory selected Tokio in 30 app runtime graphs and tokio-macros in 25 host compilation graphs. Those app counts were not re-resolved for this addendum.

## Releases and local changes

| Package | Local version | Newest stable | Local changed source files | Local added / removed lines | Conflicts to newest stable, files / hunks |
| --- | --- | --- | ---: | ---: | ---: |
| `tokio` | 1.52.3 | [1.54.1](https://docs.rs/crate/tokio/1.54.1) | 36 | +1673 / −1104 | 6 / 8 |
| `tokio-macros` | 2.7.0 | [2.7.2](https://docs.rs/crate/tokio-macros/2.7.2) | 0 | +0 / −0 | 0 / 0 |
| `tokio-util` | 0.7.18 | [0.7.20](https://docs.rs/crate/tokio-util/0.7.20) | 0 | +0 / −0 | 0 / 0 |
| `tokio-rustls` | 0.26.4 | [0.26.6](https://docs.rs/crate/tokio-rustls/0.26.6) | 6 | +106 / −474 | 3 / 6 |
| `tokio-tungstenite` | 0.29.0 | [0.30.0](https://docs.rs/crate/tokio-tungstenite/0.30.0) | 5 | +56 / −71 | 0 / 0 |

Tokio 1.52.4 is also a published release. Its only changed source files relative to 1.52.3 are `src/runtime/scheduler/current_thread/mod.rs` and `src/runtime/task/trace/symbol.rs`. These have no overlap with the measured local fork. Preserve the existing port and change all applicable version pins and patch paths together.

## Upgrade order and unvendoring

1. **Tokio 1.52.4:** smallest source update, zero textual conflicts. Keep the vendor: the local filesystem adapter alone adds 1,204 lines, alongside runtime-builder, worker, platform, and networking adjustments.
2. **tokio-macros 2.7.2:** zero local source or manifest changes. All common files in the current 2.7.0 vendor match the published archive byte for byte; only the archive's Cargo.lock is omitted and a local `.cargo-ok` marker is added. A strong candidate to use from the registry. Tokio 1.54.1 requests `~2.7.0`, which accepts 2.7.2.
3. **tokio-util 0.7.20:** zero local source or manifest changes. All package files in the current 0.7.18 vendor match the published archive byte for byte, apart from the local `.cargo-ok` marker. Another strong unvendor candidate. The new release's `tokio ^1.51.0` requirement accepts the existing 1.52.3 and the 1.52.4 patch candidate.
4. **Tokio 1.54.1:** feasible focused porting work, rather than the smallest update. Six conflicting files / eight conflict units, one of which is a deleted test-file conflict. Five other files account for seven source conflict hunks.
5. **tokio-rustls 0.26.6 and tokio-tungstenite 0.30.0:** retain the local patches. Tokio-rustls has three conflicting files / six conflict units, including one deleted test-file conflict. Tokio-tungstenite merges cleanly textually, but crosses an incompatible 0.x line and retains custom crypto-provider / feature changes; its dependencies and callers still need checking.

Unvendoring requires removing the relevant entries from both the Blueprint workspace `[patch.crates-io]` table and the packer's `BLUEPRINT_VENDOR_PATCHES` list in `TRUEOS-Blueprints/src/main.rs`. A version upgrade must also handle exact pins; for example, tokio-util is pinned to `=0.7.18` in Reticulum and the code-mode probe. Other repositories may have their own patch policies. Neither upgrade nor unvendoring was applied in this audit.

## Tokio 1.54.1 source conflicts

| File | Conflict hunks / units |
| --- | ---: |
| `src/fs/file.rs` | 2 (conflict) |
| `src/fs/file/tests.rs` | 1 (add/delete conflict) |
| `src/fs/open_options.rs` | 1 (conflict) |
| `src/fs/rename.rs` | 2 (conflict) |
| `src/fs/try_exists.rs` | 1 (conflict) |
| `src/runtime/builder.rs` | 1 (conflict) |

The 36-file Tokio source delta is +1,673/−1,104 lines. This includes a 978-line deleted test file and changes to test mocks; it is not a count of production logic alone. Other source counts also include inline tests and test files outside directories named `tests`/`test`. The new TRUEOS filesystem adapter accounts for most added source.

## Pins and integration points

The Blueprint workspace pins Tokio to `=1.52.3`. The same version is embedded in the API manifest, app/probe manifests, and generated dependency strings in the packer. The packer also registers the versioned vendor directory, and pi_snake has a direct path to it. These must be updated coherently so generated apps keep selecting the port instead of an unpatched registry copy or duplicate source.

The newer Tokio 1.54.1 release declares Rust 1.85, within the repository's Rust 1.99 nightly compiler version. This does not establish compatibility with the custom standard library or target. The filesystem and runtime changes need compilation and runtime checks for the actual Blueprint app target.

## Evidence and limits

Versions were checked against the public crates.io sparse indexes. Published archives were verified against registry SHA-256 checksums, then compared using the same source comparison and three-way merge helper as the kernel audit. The adjacent JSON stores source/archive URLs, checksums, file deltas, semantic manifest deltas, overlapping files, and per-file conflict counts for both Tokio targets.

This is a source and version audit. No Blueprint compilation, complete app graph resolution, or runtime test was performed. A clean merge does not establish API compatibility or prove that an upgrade preserves dependency counts. Repository dependency manifests, lockfiles, and vendor sources remain unchanged.
