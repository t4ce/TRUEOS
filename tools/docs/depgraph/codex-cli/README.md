# Codex CLI: applied TRUEOS Cargo/Blueprint folds

Open the [current depth overview](index.html) or the [preserved original overview](implementation/before/index.html). Both are generated from raw Cargo data. The current graph reflects the actual rebuilt `cargo bp codex-cli` package, including source migrations and native ownership folding.

The local package is **67,295,588 bytes**, down from **81,959,146 bytes**: **14,663,558 bytes / 17.891% smaller**. Thirteen source crates are now four real crates, and all consumers use the new module paths. The broad transport cycle was resolved by moving shared contracts into an independent lower layer.

## Applied source folds

| Original source packages | New package and public modules | Original tests |
| --- | --- | ---: |
| elapsed, fuzzy-match, stream-parser, template | `codex-utils-text::{elapsed, fuzzy_match, stream_parser, template}` | 54 pass |
| absolute-path, home-dir, path-utils, path-uri | `codex-utils-paths::{absolute_path, home_dir, path, path_uri}` | 124 pass |
| codex-api, codex-client, http-client, websocket-client | `codex-transport::{api, client, http, websocket}` | 315 unit + 30 integration pass |
| rustls-provider + shared HTTP contracts extracted in the second pass | `codex-network-foundation` | 8 pass |

The old thirteen package directories and Cargo dependencies are removed. The migration updates workspace members/dependencies, Cargo.lock, consumer Rust imports, optional feature references, explicit Bazel labels, new BUILD targets, and test/fixture locations. It leaves no wrapper crates that would retain the original compilation boundaries.

The original wide fold crossed both `API → protocol → HTTP` and `protocol → network-proxy → TLS provider` feedback edges. The second pass puts canonical HTTP errors, route/CA errors, retry deadlines, network revocation, and TLS-provider initialization in `codex-network-foundation`. Protocol and network proxy depend on that leaf; the transport implementation depends on protocol and the same leaf. Websocket execution is included in transport because it also used the HTTP implementation.

Error source chains, native TLS fallback classification, CA precedence, cancellation/revocation behavior, Windows dependencies, public serde/schema names, and TS output filenames were audited against the original source. Protocol's **367 original tests** pass. Derived internal Rust schema IDs follow the new crate/module identities.

[Transport migration and exact test command](implementation/transport-migration.json), [utility/path migration](implementation/utility-path-migration.json), [consumer migration](implementation/consumer-migration-results.json), [utility/path integration audit](implementation/utility-path-integration-audit.json).

## Applied packing changes

Codex now selects the packer's existing section-coalescing script. This reduces the ELF section directory from **711,612 sections to 15**. The measured local-symbol cleanup is also enabled for Codex; entry/import symbols and all referenced definitions remain available to the loader. The stripped ELF is **243,513,936 bytes**, compared with the original **375,038,536 bytes**.

The network subtree also has an integrated ownership fold. The packer derives its boundary from the exact current staged unit graph, intersects native inputs with both `.rlink` and the current Cargo artifact stream, and writes ordered response files inside the existing link group. It owns **45 units / 44 selected archives**; shared dependencies stay outside. Expanding the response files yields the same archive sequence and members. The full actual native ELF is **byte-identical** to a direct link of those same inputs.

The earlier physical archive bundle saved zero allocated bytes and changed merged-section selection. The implemented response-file method preserves native input order while making the ownership boundary explicit in the pack flow. Its report includes every input position, source/package identity, owned flag, and response-run boundary.

[Actual package validation](implementation/pack-validation.json), [native ownership manifest](implementation/native-folds.json), [actual planned build units](implementation/native-unit-graph.json), [full build log](implementation/pack-build-final.log).

## Measured before / after

| Measurement | Original | Applied |
| --- | ---: | ---: |
| Reachable target packages | 799 | 790 |
| Codex packages | 135 | 126 |
| Ordinary target units | 800 | 791 |
| Package dependency edges | 3,831 | 3,743 |
| Native link archives | 783 | 774 |
| Total build units including host tooling/scripts | 1,277 | 1,268 |
| Longest package / unit path | 35 / 36 | 35 / 36 |
| ELF sections | 711,612 | 15 |
| Blueprint file bytes | 81,959,146 | 67,295,588 |

The source folds remove nine compilation boundaries and 88 package edges. The longest remaining chain still crosses core/extensions, sandboxing, protocol, Rama, and Tokio. The packing-size gain comes primarily from coalescing ELF representation, as independently established by the original same-input experiments (16.4% with coalescing, 17.8% including local-symbol cleanup). The final baseline comparison also includes a contemporaneous std backend/cache revision change; it is not a claim that source folding alone saved 17.891%.

[Machine-readable measured results](implementation/results.json), [current graph JSON](graph.json), [package table](packages.tsv), [terminal depth/ownership overview](depth-overview.txt).

## Validation

**992 focused tests pass:** 178 utility/path tests, 720 foundation/transport/protocol tests, and all 94 packer tests. The original ignored home-directory subprocess probe also ran successfully as a child. Scoped Rust formatting passes for 151 Codex workspace packages. Bazel source targets, dependency labels, and fixture discovery were audited statically; a Bazel executable was unavailable.

The consolidated transport tests use two test threads for their local socket fixtures. Their runner clears certificate environment variables that Cargo/rustup injects before executing the test binaries, so native TLS fixtures exercise their intended backend. Tests which intentionally set custom CA variables still do so. Production TLS selection and certificate precedence are unchanged.

The real TRUEOS release pack passes its C ABI guard. Static checks verify all 273 undefined imports, supported relocation types and write bounds, executable `_start`, its regenerated header hint, container version/lengths, and the decompressed module hash. Direct/folded full-link output hashes match, native input hashes stay unchanged, and the selected panic runtime is exactly one `panic_abort` with no `panic_unwind`.

The resulting local file is `TRUEOS-Blueprints/dist/codex.bp`. Apps publication was disabled for the build; no rig execution is claimed.

[Utility tests](implementation/utility-path-tests.log), [transport/protocol tests](implementation/transport-tests.log), [packer tests](implementation/packer-tests-final.log), [formatting check](implementation/codex-scoped-rustfmt.json).

## Reproduce the current pack and overview

From TRUEOS-Blueprints:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 CARGO_BUILD_JOBS=2 cargo bp codex-cli
```

From TRUEOS, after selecting the resulting root `.rlink`:

```sh
python3 tools/docs/depgraph/codex-cli/collect.py \
  --pack-log tools/docs/depgraph/codex-cli/implementation/pack-build-final.log \
  --rlink /absolute/path/to/current/codex-<hash>.rlink
python3 tools/docs/depgraph/codex-cli/generate.py
python3 tools/docs/depgraph/codex-cli/implementation/validate_pack.py
python3 tools/docs/depgraph/codex-cli/implementation/report.py
```

Collection recovers the exact patch set from the pack log, derives the active features from the packer's plan, uses locked/offline Cargo queries, and verifies that staged inputs do not change. The captured unit graph matches the actual packer's unit plan. Raw current inputs are under [raw/](raw/); original inputs and workspace manifests are frozen under [implementation/before/](implementation/before/).

The original experiments and recorded results remain under `experiments/`. The text-fold prototype can reconstruct the old source crates from the saved original git revision. Historical graph projections use the frozen original unit graph. Current integrated packing is reproduced and validated by the commands above.
