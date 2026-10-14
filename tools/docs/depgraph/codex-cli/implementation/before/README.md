# Codex CLI: actual TRUEOS pack graph and fold attempts

Open [the interactive overview](index.html). It uses captured Cargo data to draw a depth graph on canvas, expand a dependency tree, inspect exact features and shared parents, compare root ownership, and preview the attempted folds. Choose shortest or longest depth, focus any crate, or search and press Enter. The overview is standalone HTML and works directly from disk. Its layout is automatic; there is no SVG to maintain.

The useful packing result is **section coalescing: the existing packer policy cuts the compressed payload by 16.4%**. Folding crate archives alone saved no allocated bytes. Small source folds remove crate boundaries but leave the long dependency chain intact.

The original TRUEOS graph links to this overview from its controls. All investigation and experiment sources live alongside that tooling; Codex sources, the packer, the shared build cache, and public packages were not changed.

## Scope and raw evidence

This captures the existing `cargo bp codex-cli` release pack from **2026-10-13**. The alias runs `TRUEOS-Blueprints/src/main.rs`, selects `../codex/codex-rs/cli` through `apps.json`, materializes a standalone source-overlay workspace, and applies 64 patch overrides. It disables default features and enables `trueos-blueprint` / Shell3, plus the injected TrueOS allocator. The pinned archive is `TRUEOS-Rust-Toolchain-nightly-2026-07-10`. [Exact flow, target, flags and container details](../../pack-flow.md).

| Evidence layer | Observed |
| --- | ---: |
| Resolved Cargo metadata, including build/dev resolution | 1,063 packages |
| Planned build, including host tooling and build scripts | 1,277 units |
| Ordinary reachable target library/binary units | 800 units |
| Same graph after merging binary/library package identities | 799 packages |
| Codex packages in that graph | 135 |
| Package dependency edges | 3,831 |
| Maximum shortest package depth | 9 edges |
| Longest package path / target-unit path | 35 / 36 edges |
| Actual selected native link closure | 783 archives + 9 root objects |

Reachability is computed **before** package identities merge. This prevents mixing application and build-std feature variants and inventing extra edges. The CLI binary and library count as two units but one package, which accounts for the longest-path difference. Host proc macros and custom build units are retained in the raw plan. Automatically injected `noprelude` sysroot edges are separated from the visible graph, so every crate does not sprout the same std/core dependencies.

The native `.rlink` closure is a separate authority. A planned crate is not proof its native archive enters the final link: `tokio-test` is planned and cached but absent from this link. `panic_abort` is selected and `panic_unwind` is absent. All 783 selected archives map to exact package source paths or the root's relative dep-info; names alone would confuse multiple `matchit` and `pem` versions. Input archive bytes are not additive payload costs.

Raw artifacts:

- [Cargo depth tree, matching the original TRUEOS text format](raw/codex-depth-tree.txt)
- [Resolved metadata](raw/metadata.json), [exact target build units](raw/unit-graph.json), [64 patch configurations](raw/pack-config.toml)
- [Selected native archives and root objects](raw/rlink-closure.json)
- [Capture commands, toolchain identity and input/output hashes](raw/capture.json)
- [Derived graph JSON](graph.json), [package TSV](packages.tsv), [terminal depth and ownership overview](depth-overview.txt)

## The cause visible in the graph

The deep chain crosses CLI/cloud tasks, TUI, embedded app-server, core extensions, sandboxing, telemetry, API/protocol, Rama networking, and Tokio. A longest exact unit path is retained in the [small-fold report](../../experiments/small-folds/README.md); the package path is also exported in `graph.json` and `depth-overview.txt`.

Shared consumers make inclusive subtree counts misleading. Cutting the CLI's direct app-server, daemon and exec-server dependencies removes zero units because the TUI and other consumers retain them. Even `--no-daemon` starts an embedded app-server. Cutting all five command-only helpers removes 16 target units in the simulated graph, with a functionality tradeoff that has not been implemented.

The file-size cause is more concrete: the stripped relocatable object contains **711,612 sections**. Its section headers alone consume 45.5 MB, section names consume 66.9 MB, symbol names consume 51.6 MB, and relocation records consume 68.8 MB. The final link already uses `--gc-sections`; merely reducing archive count cannot remove live code or those section records.

## Attempts and results

| Attempt | Concrete work | Result |
| --- | --- | --- |
| Major: exclusive network-proxy ownership fold | Contract 45 exact target units; bundle 44 selected archives / 367 ELF objects and relink twice | 800 → 756 unit overview; **zero allocated-byte saving**, +888 module bytes |
| Small: four pure text utilities | Copy original sources into one crate and compile/test before and after | Four crates → one; **54 original tests pass** in both; longest unit depth remains 36 |
| Small: four path primitives | Contract exact unit graph and check cycles / consumer boundaries | Acyclic, three fewer units; migration affects 53 consumer packages; depth unchanged |
| Small: narrow HTTP/client/provider bundle | Contract graph after excluding the API layer | Acyclic, two fewer units; depth unchanged |
| Rejected wider HTTP fold | Attempt API + HTTP/client/provider contraction | Creates `bundle → protocol → network-proxy → bundle` cycle |
| Additional packing experiment: existing section-coalesce policy | Relink original native closure using the packer's existing linker script, compress and verify isolated containers | **711,612 → 15 sections; compressed payload −16.4%** |

The moonshot preserves shared Tokio, HTTP, TLS and serde boundaries instead of swallowing its full inclusive closure. The native bundle has identical imports and allocated-byte totals, but 13 merged/inline sections differ after archive regrouping. Runtime equivalence is unverified, so the archive change is not recommended for implementation. [Moonshot evidence and reproducer](../../experiments/moonshot/README.md).

The utility source prototype is a host Linux build, not a packed TRUEOS binary. Its aggregate `.rlib` inputs shrink by 26.7 KB; no executable or Blueprint saving is inferred. Path and narrow transport attempts are projections, with no source migration or compile claim. [All small-fold results, original tests and reproductions](../../experiments/small-folds/README.md).

## Section coalescing: the useful next implementation

| Measurement | Existing pack | Coalesced | Coalesced + discard locals |
| --- | ---: | ---: | ---: |
| ELF sections | 711,612 | 15 | 15 |
| Stripped ELF file bytes | 375,038,536 | 249,474,776 | 243,509,048 |
| Compressed payload bytes | 81,959,122 | 68,499,750 | 67,330,298 |
| Compressed reduction | — | 16.4% | 17.8% |

The pack container adds a 24-byte header. The first variant uses the existing script already selected for Voxy/rustc; Codex currently falls outside that policy. The narrow implementation would add Codex to the existing whitelist in `TRUEOS-Blueprints/src/main.rs`, keeping the existing garbage collection and regenerated entry hint. Discarding local names is a separate policy choice and is unnecessary for the 16.4% result.

Static verification found the same 273 undefined imports and all 2,865,260 relocation types/counts, with writes in bounds, a defined executable `_start`, valid updated container flags/entry/lengths, and matching decompressed payload hashes. Loader-aligned image size before GOT/thunks changes only from 121,687,441 to 121,589,552 bytes. The two loader section-index arrays shrink from 11,385,792 to 240 bytes. This improves file representation and section bookkeeping; it does not claim a comparable live-code reduction.

[Exact commands, section tables and validation](../../experiments/coalesce/results.json), [container verification](../../experiments/coalesce/container-verification.json), [isolated reproducer](../../experiments/coalesce/run.py). Experimental `.bp` files remain under `/tmp/codex-bp-coalesce-ts43cca6/`. No rig execution or production packer change was performed.

## Reproduce

From the TRUEOS checkout, using the existing staged workspace and captured native cache:

```sh
python3 tools/docs/depgraph/codex-cli/collect.py
python3 tools/docs/depgraph/codex-cli/generate.py
```

Collection is locked and offline, checks that staged manifest/lock/target/config inputs stay unchanged, and only asks Cargo to resolve/tree/plan. It does not compile, restage, publish, or contact the rig. The collector defaults to this capture's native root; use `--rlink /absolute/path/to/codex-<hash>.rlink` after deliberately selecting a different pack. Patch configs are captured from this pack's log; if the packer/stage changes, capture that new patch set before claiming a fresh overview.

Experiment commands are documented in their reports. The viewer was checked in headless Chromium for loading, search/focus, both depth conventions, tree expansion, ownership rows, all four accepted fold previews, and exclusion of the rejected cycle. The raw source graph and input hashes were independently audited by a second agent.

Three parallel investigators plus an independent audit were used. The requested `terra med` type was unavailable in this session; available inherited agents were used and that substitution was disclosed before the experiments.
