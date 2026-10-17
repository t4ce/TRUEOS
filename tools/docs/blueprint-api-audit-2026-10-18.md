# TRUEOS / Blueprints API audit — 2026-10-18

The clearest cleanup opportunity is stale SDK surface and contradictory API policy. Missing app imports alone are insufficient evidence to delete an implementation.

This audit added a read-only tool, `tools/audit/blueprint_api.py`, and a machine-readable snapshot, `blueprint-api-audit-2026-10-18.json`. No kernel or SDK behavior was changed. The snapshot includes existing uncommitted work in both repositories.

## Measured surface

| Measurement | Result |
| --- | ---: |
| Packaged apps scanned (`TRUEOS-Blueprints/dist/*.bp`) | 62 |
| Package/decompression/ELF/nm errors | 0 |
| Distinct undefined ELF import names | 728 |
| Kernel `bp_abi.rs` CABI declarations | 390 |
| Blueprint SDK `bp_abi.rs` CABI declarations | 397 |
| Generated resolver candidates (excluding WC3 x86) | 372 |
| Candidates imported by at least one package | 255 |
| Candidates imported by no package | 117 |
| SDK declarations absent from kernel declarations | 15 |
| Kernel declarations absent from SDK declarations | 8 |
| Textual signature differences among shared declarations | 0 |

A generated candidate has both a declaration and a source definition under the build script's discovery rules. It is not necessarily usable: feature gates, prebind restrictions, and runtime behavior still apply. Signature comparison removes whitespace; it does not compare transitive struct layouts.

## Findings, in cleanup order

### 1. SDK advertises 15 bindings with no current kernel definition

All 15 have source references in SDK wrappers, but none appear in the 62 packaged apps:

- Eleven `trueos_cabi_ui3_frame_*` bindings. Public wrappers remain behind `api/Cargo.toml`'s `ui3` feature; calls and local extern declarations are in `api/src/lib.rs`.
- `trueos_cabi_uart1_shell_write`, called by `crates/trueos-v/src/vshell.rs:183`'s `uart1_shell_write`.
- `trueos_cabi_vgpu_cloud_work_graph_create`, `trueos_cabi_vgpu_cloud_work_graph_destroy`, and `trueos_cabi_vgpu_cloud_frame_submit`. Wrappers and associated types remain in `crates/trueos-v/src/vgpu.rs`.

These are concrete stale public paths. First cleanup batch: retire these SDK wrappers/features and their exclusive types, after checking application source in external repositories and confirming whether any feature is intentionally awaiting reimplementation. None should be represented as a working current-kernel capability.

### 2. Five Shell2 exports are explicitly retired stubs

`src/r/io/fs_cabi.rs:1855` states that the signatures remain locked and there is no Shell2 backend. `trueos_cabi_shell2_raw_write` returns zero; `trueos_cabi_shell2_frontend_{attach,read,submit_input,detach}_v1` return `-38`.

They remain declared, discoverable by the generated resolver, and unused by packaged apps. Preserve the distinction between an ABI tombstone and a usable service. A useful follow-up is explicit retired-import rejection with a migration message. Removing locked declarations requires a deliberate ABI-policy change; do not casually rewrite the lock.

### 3. Synchronous filesystem policy contradicts resolver generation

`src/hv/blueprint/prebind.rs:61` rejects every `trueos_cabi_fs_*` and `trueos_cabi_trueosfs_*` import, directing apps to `trueos::async_fs`.

Nevertheless, nine synchronous filesystem names remain in kernel `bp_abi.rs` and are generated resolver candidates. None is imported by the scanned packages. Three typed filesystem declarations are also absent from the Blueprint SDK.

Several underlying implementations use `trueos_kernel_sync_*` export names and are consumed by the kernel-only `crates/trueos-v/src/legacy_fs_abi.rs` facade. Their source functions retain historical `trueos_cabi_*` Rust identifiers. This is intentional internal compatibility, not proof of dead implementation code.

Cleanup should remove these names from the *supported Blueprint capability inventory* and prevent contradictory resolver registration, while preserving kernel-internal callers and following the ABI-lock policy. Investigate direct loader paths too: the generic resolver's explicit unsupported check covers Unix imports, whereas the synchronous-filesystem rejection is in the normal prebind path.

### 4. The zero-import list contains live internal infrastructure

All 14 `trueos_cabi_socket_tcp_*` bindings have no direct packaged-app imports and no references in the scanned Blueprint source corpus. They are nonetheless called from the kernel's `src/std_abi_shim.rs` socket implementation, including open/connect/send/recv/close and readiness/error handling. Applications reach them indirectly through Unix socket imports.

Do not delete these implementations based on the import census. Whether the direct CABI exports should remain public is a separate API-surface decision.

Of the 117 zero-import candidates, 34 also have no non-declaration references in the scanned Blueprint source corpus. That narrower list still includes these sockets, retired Shell2 functions, and internal filesystem compatibility. It is a review queue, not a deletion list.

### 5. Unknown imports can become return-zero stubs

`src/hv/blueprint/blueprint.rs:430` installs a fallback stub when a name has no known resolver binding, subject to the 64-slot table. Invocation logs once and returns zero (`:322`). This path is broader than the `is_joker_import` classifier, which recognizes Rust allocator/runtime names.

That can let a stale API survive symbol binding without useful behavior; zero can even resemble success. Review launch/prebind policy and weak/optional imports before tightening this. The report's `outside_literal_resolver_inventory` field is only a triage hint: generated Ring bindings, WC3 feature bindings, and other dynamic cases must be accounted for before declaring an import missing.

### 6. Catalog and package inventory differ

`apps.json` contains 64 distinct app names. `rustc-min` and `tinyfighter` have no same-name dist package; `codex-cli` corresponds to a differently named `codex.bp`. This audit includes every dist package, so it covers packaged artifacts beyond a one-to-one catalog match. Missing packages do not prove failed or abandoned apps.

## Reproduction and limits

```sh
python3 tools/audit/blueprint_api.py \
  --blueprints ../TRUEOS-Blueprints \
  --out /tmp/blueprint-api-audit.json
```

Requires `python3`, `lz4`, and `nm`. The tool validates TRBP version and lengths, decompresses tar payloads without extracting paths to disk, checks the embedded ELF and raw length, and reads undefined symbols. It records package SHA-256 hashes and mtimes; errors are reported and produce a nonzero exit. An empty artifact directory is an error.

The source reference scan covers Blueprint `api`, `crates`, `apps`, `buildins`, and `probes`, excluding dependency/build directories. It is lexical: declarations/comments are filtered heuristically, cfg and macros are not evaluated, and a wrapper reference does not prove application use. External app repositories such as solara, codex, voxy, and Velory are represented by their dist artifacts, not a complete source call graph. Dynamically selected names, external consumers, alternate build features, stale packages, and transitive ABI layouts need separate checks.

Validation: the scanner completed against all 62 real packages without errors. No rig runs or rebuilds were performed, and this report makes no claim that packaged apps launch successfully. Existing source changes were preserved.
