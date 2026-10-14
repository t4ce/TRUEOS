# Three source crate folds, one rejected wider variant

> Historical experiment: the source folds are now applied. Original graph inputs are preserved under [implementation/before/raw](../../implementation/before/raw/); the [current report](../../README.md) records the integrated package.

The pure text utility fold works as a source prototype. Path and narrow HTTP folds preserve the dependency DAG in the exact staged unit graph. None reduces the complete graph's longest path. The wider HTTP fold creates a cycle and is rejected.

These experiments change only copied source files and disposable local build directories. Codex sources, the pack stage, `.bp` packages, and shared Cargo caches are not modified.

## Exact scope

Input: [`../../raw/unit-graph.json`](../../raw/unit-graph.json), produced for the staged `cargo bp codex-cli` release build, with the pack's TRUEOS target, overlays and features. This is a compilation-unit graph, not the original Codex workspace metadata graph.

Keep reachable target units with `platform != null`, `mode = build`, and target kind `lib` or `bin`. Exclude host proc macros, custom-build/run-custom-build units, and automatically injected `noprelude` build-std edges. Unit variants remain distinct. The CLI library and CLI binary are therefore two nodes; a package-level view will have a different node count and can remove that additional depth edge.

The resulting baseline is **800 target source units, 3,909 dependency edges, maximum shortest depth 9, maximum longest depth 36**. Depth counts edges from the CLI binary. Each projection preserves all external incoming and outgoing edges and deduplicates merged edges. It runs a cycle check before claiming success.

| Attempt | Source units before → after | Whole graph units after | Edges after | Longest depth after | External consumers |
| --- | ---: | ---: | ---: | ---: | ---: |
| Pure text utilities | 4 → 1 | 797 | 3,908 | 36 | 8 units / 8 packages |
| Path primitives | 4 → 1 | 797 | 3,858 | 36 | 54 units / 53 packages |
| HTTP transport, wide | 4 → 1 | 797 | 3,872 | cycle: rejected | 38 units / 37 packages |
| HTTP transport, narrow | 3 → 1 | 798 | 3,896 | 36 | 36 units / 35 packages |

The projected node counts in the rejected row describe the attempted contraction, not a valid Cargo graph. No row claims `.bp` byte savings. Folding Rust crates retains their code, changes type and macro crate identities, and can reduce compilation parallelism or enlarge invalidation scope.

## 1. Pure text utility bundle: source prototype passed

Merge `codex-utils-elapsed`, `codex-utils-fuzzy-match`, `codex-utils-stream-parser`, and `codex-utils-template` into four modules of one crate. All four have zero external normal dependencies in the actual target units. Their existing sources total 2,182 lines across 11 Rust files.

[`prototype.py`](prototype.py) creates two isolated workspaces: the original four crates and a merged crate. It copies source code and original tests unchanged except for 28 `crate::` references in five stream-parser files, which are rebased to `crate::stream_parser::`. `pretty_assertions` remains a test dependency. Both use Rust 1.95.0, host x86_64 Linux, release `opt-level=1`, and `codegen-units=8`.

After the production source migration removed those four package directories, the reproducer reads their original source trees from the pre-migration Codex Git commit recorded in `../../implementation/before/initial-state.json`. It extracts only those utility sources into the ignored `generated/historical-source` directory and records the resolved commit in new results. Use `--source-revision <commit>` to select another revision that still contains the original package paths. The historical results below remain the original measurement.

| Measured host source prototype | Four crates | Merged crate |
| --- | ---: | ---: |
| Source crate `.rlib` artifacts | 4 | 1 |
| Original tests passed | 54 | 54 |
| Failed / ignored tests | 0 / 0 | 0 / 0 |
| Aggregate source `.rlib` bytes | 507,066 | 480,362 |

The 26,704-byte difference is archive/metadata/code composition in this host prototype, **not an executable or TRUEOS pack reduction**. Build-plus-test elapsed times were 0.924 and 1.068 seconds in a single fresh run; they are not a reliable performance comparison. The shared graph's longest source path is unchanged. A source fold removes three crate boundaries but consumers still need migrated imports/dependency aliases. Adding reexport wrapper crates would retain those boundaries and undo the compile-unit reduction.

The merged crate would make a change in any one utility invalidate the union of its consumers. That reverse dependency closure is 61 target units. This is a modest breadth cleanup, not the cause of the deep pack graph or large relocation payload.

Validation is recorded in [`prototype-results.json`](prototype-results.json), with original build/test events under [`raw/`](raw/). The generated source and build directories are ignored and can be recreated:

```sh
python3 tools/docs/depgraph/codex-cli/experiments/small-folds/prototype.py --fresh
```

## 2. Path primitives: acyclic projection, expensive migration

Merge `codex-utils-absolute-path`, `codex-utils-home-dir`, `codex-utils-path`, and `codex-utils-path-uri` into one path crate. The projection removes three internal dependency edges and coalesces repeated incoming edges, reducing 3,909 whole-graph edges to 3,858. It keeps 10 external dependency units; dependencies such as `serde`, `schemars`, `ts-rs`, `dirs`, and `url` remain outside the bundle.

This fold is graph-valid, but affects 53 external packages and a 96-unit reverse rebuild closure. The selected source totals 5,982 lines across 20 files. Public path types, serde/schema generation, existing macros, and every consumer's Cargo aliases/import paths need a coordinated source migration. This is a graph simulation only; it has not been compiled or packed.

The full longest path remains 36. With only three fewer source crates and so many consumers, this is a poor first optimization for packing depth or size.

## 3. HTTP transport: wide fold rejected; smaller boundary works

The tempting four-crate fold is `codex-api` + `codex-client` + `codex-http-client` + `codex-utils-rustls-provider`. It fails even though the original Cargo graph is a DAG:

```text
transport bundle → codex-protocol → codex-network-proxy → transport bundle
```

`codex-api` depends on `codex-protocol`; protocol depends on `codex-network-proxy`; network-proxy depends on the shared rustls provider. Independently, protocol depends on `codex-http-client`, so putting API and HTTP together crosses that protocol boundary. A valid bigger merge would need to absorb intervening crates or extract protocol types further, materially expanding scope.

The smaller fold leaves `codex-api` separate and merges only `codex-client`, `codex-http-client`, and `codex-utils-rustls-provider`. That contraction stays acyclic. It removes two internal dependency edges, reduces the target unit count to 798, and has 23 external dependency units, 35 external consumer packages, and a 92-unit reverse rebuild closure. The selected source totals 11,634 lines across 47 files.

This is a plausible transport boundary if source consolidation is desired, but it is still a projection. The complete longest path remains 36, and native TLS provider/ownership behavior needs to remain identical in any real implementation. No source migration, compiler check, or new TRUEOS package is claimed here.

## What the folds reveal

One existing longest path is:

```text
codex binary → codex-cli lib → codex-cloud-tasks → codex-tui
→ codex-app-server-client → codex-app-server → codex-goal-extension
→ codex-core → codex-agent-message-board-client
→ codex-agent-message-board-extension → codex-extension-api
→ codex-tools → codex-connectors → codex-plugin → codex-utils-plugins
→ codex-exec-server → codex-sandboxing → codex-mxc-sandbox
→ codex-windows-sandbox → codex-otel → codex-api → codex-protocol
→ codex-network-proxy → rama-http-backend → rama-http-core → rama-http
→ rama-http-headers → rama-net → rama-http-types → rama-core → rama-utils
→ tokio → parking_lot → parking_lot_core → smallvec → serde → serde_core
```

Small leaf folds remove boundaries, but the long chain comes from feature/domain consumers repeatedly crossing between core, extensions, command transports, protocol and networking. The API/protocol cycle rejection demonstrates an actual coupling constraint that an attractive tree display can hide. Shared dependencies stay shared; the projection never counts them as exclusively owned by one subtree.

Reproduce all graph attempts:

```sh
python3 tools/docs/depgraph/codex-cli/experiments/small-folds/simulate.py \
  --unit-graph tools/docs/depgraph/codex-cli/implementation/before/raw/unit-graph.json \
  --output tools/docs/depgraph/codex-cli/experiments/small-folds/results.json
```

[`results.json`](results.json) retains each exact source unit, source sizes, consumer/dependency boundaries, cycle witness and complete projected edges. [`folds.json`](folds.json) provides package-name sets and explicit statuses for the overview's interactive previews.
