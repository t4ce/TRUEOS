# Moonshot: fold the network proxy's owned native subtree

> Historical experiment: the source folds are now applied. Original graph inputs are preserved under [implementation/before/raw](../../implementation/before/raw/); the [current report](../../README.md) records the integrated package.

The realistic large fold is `codex-network-proxy` plus its **exclusive**
target dependencies. The full recursive closure has 271 units, but 226 of
those are shared by other Codex subsystems. The owned subtree has 45 target
units, including Rama's transport stack, ASN.1/X.509 helpers, CSV, and host
matching libraries. Its ten incoming edges all enter `codex-network-proxy`;
168 outgoing edges reach 78 shared units. Shared Tokio, HTTP, TLS, serde,
and utility crates remain visible outside this fold.

This fold reduces the explicit target overview from 800 units to 756 and
3,909 edges to 3,717. The 800 units correspond to 799 packages because the
CLI has both a binary and a library. These counts omit implicit `noprelude`
sysroot edges, host proc macros, and build scripts. The captured raw unit
graph retains all those details.

## Concrete native packing experiment

`run.py --link` authenticates the exact native archives against the root
`.rlink`, extracts only native ELF object members, combines them in one
archive, and replaces the original subtree archives in an isolated copy of
the packer's `rust-lld -r --gc-sections` operation. It uses the existing
root entry object, root symbol, remaining archives, and link group.
No Rust source or manifest is changed and no compile or publish occurs.
Hashes of all native input files are checked before and after.

45 planned build units produced only 44 archives in the selected root link:
`tokio-test 0.4.6` exists in the build cache but is absent from `.rlink`.
Duplicate `matchit` archive stems were distinguished using Cargo dep-info
source paths; the network fold selects 0.9.2 and leaves 0.8.4 outside.

| Evidence | Existing module | Folded module |
| --- | ---: | ---: |
| Native archive inputs in owned subtree | 44 | 1 |
| Native object members in subtree | 367 | 367 |
| Relocatable module file bytes | 433,698,816 | 433,699,704 |
| ELF sections | 712,059 | 712,059 |
| Allocated ELF sections | 421,165 | 421,165 |
| Sum of allocated section bytes | 120,008,595 | 120,008,595 |
| Allocated bytes backed by file | 119,998,118 | 119,998,118 |
| Allocated NOBITS bytes | 10,477 | 10,477 |
| Undefined symbol inventory | Identical | Identical |

The subtree's source archives total 119,803,588 bytes, and their ELF object
members total 106,669,664 bytes. These are **input sizes**, not additive
resident-memory contributions. The allocated-byte sum also measures ELF
section content, not the loader's measured memory footprint.

**Result:** a correct ownership fold helps the overview; combining native
archives by itself yielded zero allocated-byte reduction and added 888
file bytes. The current final link already performs section garbage
collection, so reducing archive count does not reduce the final section
inventory. This directs packing work toward retained sections and ELF
representation rather than the number of Cargo crates.

Both an initial unit-order archive bundle and a second bundle preserving
the original `.rlink` archive order were linked. The final rerun against
the refreshed raw capture took 2.38 seconds. Its imports, section counts, and allocated totals match the
baseline, but 13 allocated section contents differ. They are merged
constant/string sections and repeated inline-code sections; archive
regrouping changes their ordering/selection. Byte identity and runtime
equivalence were not proven. This archive change is therefore an experiment,
not an implementation recommendation. The large temporary archive and module
outputs were removed; evidence remains in `result.json` and the initial
`attempt-unit-order.json`.

## Why a native-only helper split was not the large fold

The first candidate was to gate daemon/server/admin helpers for the TRUEOS
build. An exact target-unit cut of the CLI's dependencies to app-server,
app-server-daemon, and exec-server removes **zero** units: other essential
subsystems still use them. In particular, the TUI's `--no-daemon` path starts
the embedded app-server through `app-server-client`. Removing the server
would require a different TUI architecture.

Gating the command-only TCP tunnel, cloud tasks, responses proxy, test
client, and stdio-to-UDS dependencies removes 16 target units in total.
That is a possible future feature split with a concrete functionality
tradeoff, not the major packaging cause. The saved unit count differs from
the Cargo metadata-only result because metadata includes development-feature
unification. See `command-plane-rejection.json`.

The network-proxy crate is itself also needed by ten consumers, including
protocol/config crates which only need policy types. A future policy-model
and transport split could improve boundaries, but runtime consumers still
need the proxy; that split alone does not remove the transport from this
fully functional pack. No substitute TRUEOS policy backend was invented.

## Reproduce

From the TRUEOS checkout, with the captured raw files and existing native
pack cache available:

```sh
python3 tools/docs/depgraph/codex-cli/experiments/moonshot/run.py
python3 tools/docs/depgraph/codex-cli/experiments/moonshot/run.py --link
```

The first command repeats the ownership and helper-cut simulation. The
second repeats the isolated native archive experiment, including input-hash
verification. `folds.json` provides 45 exact Cargo package IDs for the
graph-only ownership preview. Package names are descriptive labels;
membership must use the IDs so other versions of the same crate remain
outside. The isolated archive result is runtime-unverified, and the preview
does not hide the raw dependency edges.
