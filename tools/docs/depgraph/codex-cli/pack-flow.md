# Codex CLI in the TRUEOS Cargo Blueprint pack flow

Snapshot: 2026-10-13. This describes the effective staged build, not the host Codex workspace graph.

## Entry and build

1. `TRUEOS-Blueprints/.cargo/config.toml` defines `bp = "run --bin trueos-blueprint --"`.
2. `TRUEOS-Blueprints/apps.json` maps `codex-cli` to `../codex/codex-rs/cli`.
3. `TRUEOS-Blueprints/src/main.rs:516` resolves that package's first explicit binary, `codex`, and release profile. `src/cli.rs:79` defaults Blueprint builds to release.
4. The packer stages a standalone workspace at `/home/t4ce/Repos/codex/codex-rs/cli/target/trueos-blueprint/work/codex/source-overlay-app/Cargo.toml`. It materializes inherited package/dependency values, absolute local dependency paths, and the Blueprint `trueos` API dependency, with source shims and a native `_start` entry. Original source stays outside this staging area.
5. Its 64 source patches are supplied as Cargo `--config patch.crates-io.*` overrides, **not** embedded in the staged manifest. See [raw/pack-config.toml](raw/pack-config.toml); the captured source-overlay log line is [raw/pack-config-source.txt](raw/pack-config-source.txt). Alias patches contain both `package` and `path`.
6. `TRUEOS-Blueprints/src/toolchain.rs:12` pins `nightly-2026-07-10`, rustc commit `af3d95584dbddcae597890340995509a7fb47a50`. The sibling archive supplies both Cargo and `RUSTC`: `/home/t4ce/Repos/TRUEOS-Rust-Toolchain-nightly-2026-07-10/bin/`.
7. The actual target is the staged JSON `.../cli/target/trueos-blueprint/x86_64-unknown-trueos.json`: `os=trueos`, Unix family, `env=musl`, `panic-strategy=abort`, PIC, local-exec TLS, inline stack probes, x86-64.
8. Cargo uses `rustc -Z build-std=core,compiler_builtins,alloc,std,panic_abort -Z json-target-spec --target <JSON> --release --bin codex --no-default-features --features trueos-blueprint,trueos/default-global-allocator -- -Zno-link --emit=obj`. The packer also supplies `-C panic=abort`, TRUEOS check-cfg/warning flags, native C build flags, and SQLite sort-worker disabling. The root CLI features resolve to `sh3,trueos-blueprint`; `trueos` resolves to `default,default-global-allocator,lifecycle-net,tokio-net-probe,tokio-runtime`.

The standalone release profile is `opt-level=1`, `incremental=true`, `codegen-units=8`; no LTO. `cli/Cargo.toml:9` explicitly chooses this profile for Blueprint builds. Cargo's JSON unit profile can still say `panic=unwind` because the effective abort setting comes from rustflags and the target; the selected `.rlink` confirms `panic_abort` and no `panic_unwind`.

The target cache is shared, separate from this staging directory:

```text
/home/t4ce/Repos/TRUEOS-Blueprints/target/trueos-blueprint/cargo-cache/
  x86_64-unknown-trueos/
    tokio-platform-v7-stackful-threads-d6de8a97ede1c628/
      x86_64-unknown-trueos/release/deps/
```

## Link and container

`TRUEOS-Blueprints/src/main.rs:831` and `src/artifact.rs:112` select the newest root `.rlink` and extract its root object list and selected archives. This is the authoritative native closure; scanning every cached `.rlib` would include stale or unrelated artifacts.

[raw/rlink-closure.json](raw/rlink-closure.json) records `codex-cc61738729adbda0.rlink`, nine root objects, and 783 archives. Their aggregate archive size is 1,695,132,352 bytes. This includes metadata and discarded sections, so it is **not** retained Blueprint payload cost.

The packer combines root codegen objects with `ld -r`, globalizes `_start`, then links `ld -r --gc-sections --undefined _start app.rooted.o --start-group <selected rlibs> --end-group`. It checks import source ABI contracts, strips debug information, places `module.elf` in a deterministic USTAR archive, and compresses with `lz4 -12 -BI --content-size`. A 24-byte `TRBP` header carries version, flags, entry section/offset, compressed size, and raw ELF size.

Current files:

| Stage | Bytes |
| --- | ---: |
| `work/codex/app.codegen.o` | 13,835,248 |
| `work/codex/app.rooted.o` | 12,526,592 |
| `work/codex/module.o` | 433,698,816 |
| `work/codex/module.stripped.o` | 375,038,536 |
| `work/codex/module.stripped.tar.lz4` | 81,959,122 |
| `TRUEOS-Blueprints/dist/codex.bp` | 81,959,146 |

The current header is version 1, flags 3 (tar/LZ4 payload), entry section 12,392, entry offset 0. Native launch already forces the embedded server for a bare launch at `codex-rs/cli/src/main.rs:1062`. CLI features disable system clipboard, voice, local models, Bedrock, and external reporting/import defaults, while the complete command dispatcher and embedded server remain normal dependencies.

Resident tracing patches preserve upstream Rust APIs but route native collection to the existing `trueos_cabi_log` kernel boundary. That is a concrete source/API fold. It does not authorize removing arbitrary same-version crates merely because TRUEOS also depends on them; `TRUEOS-Blueprints/tools/dependency_economy.py` states this limitation explicitly.

## Read-only graph reproduction

Use the staged manifest, captured overrides, custom target and root features together. Cargo metadata on the original Codex workspace alone misses Blueprint source identities and can unify unrelated workspace features.

```sh
export RUSTC=/home/t4ce/Repos/TRUEOS-Rust-Toolchain-nightly-2026-07-10/bin/rustc
cd /home/t4ce/Repos/codex/codex-rs/cli/target/trueos-blueprint/work/codex/source-overlay-app
/home/t4ce/Repos/TRUEOS-Rust-Toolchain-nightly-2026-07-10/bin/cargo metadata \
  --locked --offline --format-version 1 -Z json-target-spec \
  --filter-platform /home/t4ce/Repos/codex/codex-rs/cli/target/trueos-blueprint/x86_64-unknown-trueos.json \
  --manifest-path Cargo.toml --no-default-features \
  --features trueos-blueprint,trueos/default-global-allocator \
  --config /home/t4ce/Repos/TRUEOS/tools/docs/depgraph/codex-cli/raw/pack-config.toml
```

The corresponding tree uses `cargo tree --locked --offline -Z json-target-spec --target <same JSON> --manifest-path Cargo.toml --no-default-features --features <same features> --config <same config> -p codex-cli -e normal,build --prefix depth`. Metadata has a feature-unification caveat; [raw/unit-graph.json](raw/unit-graph.json) records individual target/host build units, including build-std.

No `cargo bp` command was run for this investigation. Existing staging, public `dist/codex.bp`, shared caches and source manifests were left intact.

## Concrete packaging fold: reuse the existing section-coalescing policy

Codex's stripped ELF contains **711,612 sections**, but only 119,998,118 bytes in allocated file sections. Section headers cost 45,543,168 bytes, the section-name table 66,874,812 bytes, symbol names 51,594,550 bytes, and symbol records 17,641,776 bytes. This is an actual cause of packaging and loader traversal cost, separate from Rust dependency count.

The packer already has `REL_COALESCE_LINKER_SCRIPT` for rustc, Voxy, and Pumpkin/dev at `TRUEOS-Blueprints/src/main.rs:85`, but Codex is absent from the selection at line 882. An isolated relink used the same root object, same 783 archives, same garbage collection and `_start` root, adding only that existing script. A second variant adds the `--discard-all` option already used for rustc packaging.

| Variant | ELF sections | Stripped ELF bytes | Compressed bytes | Compressed saving |
| --- | ---: | ---: | ---: | ---: |
| Current pack | 711,612 | 375,038,536 | 81,959,122 | — |
| Existing coalesce script | 15 | 249,474,776 | 68,499,750 | 13,459,372 (16.4%) |
| Coalesce + discard local symbols | 15 | 243,509,048 | 67,330,298 | 14,628,824 (17.8%) |

Both variants retain all 273 undefined symbols, the same 2,865,260 loader-supported relocations and their type counts, and a defined executable `_start`; all relocation writes remain within their target sections. The entry header is regenerated for the new section/offset. Decompression/container checks confirm the experimental `TRBP` header lengths, `module.elf` tar member and SHA-256 match the relinked ELF; see [container-verification.json](experiments/coalesce/container-verification.json). The ELF loader iterates allocated sections by flags/types and uses the entry hint, so the existing coalesced format follows its layout contract. These artifacts have not been launched on a rig; static checks do not prove runtime correctness. Local-symbol discarding merits separate review if local names are wanted for diagnostics.

Coalescing does not shrink compiled behavior. Allocated file-section bytes become 121,575,393 because output sections contain alignment padding; the loader-aligned image size before synthetic GOT/thunks slightly decreases from 121,687,441 to 121,589,552 bytes. It removes almost all section bookkeeping: the loader's two `usize` arrays proportional to section count (`TRUEOS/src/hv/blueprint/blueprint.rs:917`) shrink from 11,385,792 to 240 bytes.

Reproduce with `python3 tools/docs/depgraph/codex-cli/experiments/coalesce/run.py` from TRUEOS. It writes relinked objects, compressed archives and experimental `.bp` files into a fresh `/tmp/codex-bp-coalesce-*` directory. Checked-in-size evidence and exact argument arrays are in [experiments/coalesce/results.json](experiments/coalesce/results.json); the unchanged native archive inventory is [experiments/coalesce/selected-rlibs.json](experiments/coalesce/selected-rlibs.json).
