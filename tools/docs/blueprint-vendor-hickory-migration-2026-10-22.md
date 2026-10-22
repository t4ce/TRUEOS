# Shared Hickory 0.26.3 vendors

Blueprints now owns the same three Hickory 0.26.3 sources previously resolved from the Cargo registry by Voxy:

| Package | Shared source directory |
| --- | --- |
| `hickory-proto` | `TRUEOS-Blueprints/vendor/hickory-proto-0.26.3` |
| `hickory-net` | `TRUEOS-Blueprints/vendor/hickory-net-0.26.3` |
| `hickory-resolver` | `TRUEOS-Blueprints/vendor/hickory-resolver-0.26.3` |

`hickory-net` is a separate dependency of the 0.26.3 resolver. All three are registered in the Blueprint workspace patches, the packer's source-overlay table, and Voxy's patches. Voxy's lockfile now records local path packages for this stack. Its existing resolver version requirement and features are preserved.

Removed `vendor/crossfont`, `vendor/hickory-proto-0.25.2`, and `vendor/hickory-resolver-0.25.2`. The replacement sources, licenses, and manifests were copied from Voxy's existing registry sources and verified file-by-file with SHA-256; no Hickory implementation changes were made. Cargo cache markers were omitted.

Validation:

- Blueprint packer `cargo check --offline --locked --bin trueos-blueprint --target x86_64-unknown-linux-gnu` passed.
- An isolated workspace depending on the shared resolver and the Blueprint patches compiled all three Hickory crates successfully.
- A copy of Voxy's staged manifest/lock was resolved for the TRUEOS target with the packer's configuration. A second, locked compilation-unit graph query passed and selected all three shared local source paths.
- Both repositories passed `git diff --check`.

The original Voxy workspace's general metadata/check queries could not finish offline because its lockfile references uncached `clap 4.6.7`. No full Voxy rebuild was performed. The isolated dependency check and copied staged TRUEOS graph avoid that unrelated cache gap.

Refreshing Voxy's lock also synchronized existing manifest changes for local MicroFont, the root Moka dependency, and the selected `twox-hash` Rand dependency. An incidental UUID upgrade was reverted to the previous locked version.

Earlier vendor-usage reports are historical snapshots taken before this migration; their stale Hickory/Crossfont findings describe the old tree.
