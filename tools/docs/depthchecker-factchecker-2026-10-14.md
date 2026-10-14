# Codex Blueprint depthchecker_factchecker — 2026-10-14

The probe ran real Codex crate functions through the TRUEOS `cargo bp` flow,
then captured the first entry into 14 selected dependency functions across
seven scenarios. Both the baseline and corrected kernel runs passed every
assertion (`pass_mask=0x7f`). The experiment found and corrected two concrete
filesystem causes: Hull metadata used private, unmounted filesystem globals,
and a guest-only VMcall buffer inflated the native file-read stack.

## Evidence to open

- [Corrected run overview](../../bld/depthchecker_factchecker/report-stack-fold/overview.md),
  [complete physical stacks](../../bld/depthchecker_factchecker/report-stack-fold/stacks.txt),
  and [machine-readable checks](../../bld/depthchecker_factchecker/report-stack-fold/checks.json).
- [Baseline overview](../../bld/depthchecker_factchecker/report-baseline/overview.md)
  and [baseline stacks](../../bld/depthchecker_factchecker/report-baseline/stacks.txt).
- [Authenticated before/after machine-code comparison](../../bld/depthchecker_factchecker/run-stack-fold/stack-layout-comparison.json)
  and [independent corrected-run audit](../../bld/depthchecker_factchecker/run-stack-fold/peer-audit.json).
- [Raw Cargo depth table](../../bld/depthchecker_factchecker/raw-final/cargo-depth.tsv)
  and [exact native unit graph](../../bld/depthchecker_factchecker/raw-final/native-unit-graph.json).
- [Probe source and fixtures](../../../TRUEOS-Blueprints/probes/depthchecker_factchecker/README.md)
  and [reproduction commands](../depthchecker_factchecker/README.md).

Each capture directory contains `stack.bin`, register evidence, readable ranges,
SHA-256, physical frames, source/inline labels, and its scoped execution identity.
The runner pins the Blueprint, full/packed ELF, kernel, boot ISO, plan, staged
probe source, and capture helpers. The reports hash their evidence files.

## What actually ran

| Scenario | Selected entries captured |
| --- | --- |
| Fragmented text parsing | Text crate's `str::find` instantiation; `StrSearcher::new` |
| Typed path resolution | URL `parse_path`; `push_pending` |
| Protocol permission roundtrip | Serde's emitted `skip_to_escape` clone |
| Prepared transport body | Native `ZSTD_compressStream2`; `ZSTD_decompressStream` |
| TRUEOS clock | `trueos_time_monotonic_nanos` |
| Native worker parsing and file I/O | `StrSearcher::new`; `fsync`; `trueos_thread_context_swap` |
| Hull file I/O | `fsync`; `trueos_kernel_sync_fs_read_file`; metadata-v2 start |

The exact diagnostic probe graph has 650 build units, including 416 ordinary
target units and 1,224 ordinary dependency edges, with maximum dependency depth
17. This is the probe's compiled graph. Runtime physical chains contain 10–19
frames; Cargo depth and physical frame depth count different things.

All 14 corrected snapshots have the expected x86-64 entry alignment
(`RSP % 16 == 8`), no unreadable stack ranges, and an authenticated physical root.
Eleven are L2 Hull guest snapshots; three are direct native continuation stops.
Native stops belong to the worker's own 2 MiB stack. A symbol belonging to the
kernel ELF can execute inside a Hull, so symbol image ownership is kept separate
from execution identity. Assembly roots/context-switch functions have exact
symbols but no DWARF source lines.

## The large-stack cause and correction

The native filesystem wait follows this observed chain, with standard-library
wrapper frames folded here for readability; the raw report retains every frame:

```text
depthchecker_native_worker_run
  depthchecker_filesystem_run
    std::fs::File::open
      open -> read_file_from_cabi -> trueos_kernel_sync_fs_read_file
        fs_read_file_len_host -> request_broker::submit
          threads::wait -> threads::suspend -> trueos_thread_context_swap
```

The baseline dispatcher reserved `0x28028` bytes before choosing its native or
Hull branch. The guest helper's `[u8; PAYLOAD_CAP]` scratch buffer has 163,784
bytes. Inlining placed that guest-only storage in the common frame; saved
registers and call bookkeeping explain the observed `0x28060` frame-pointer gap
exactly. An earlier pre-probe kernel has the identical large prologue.

[Keeping the private guest helper out of line](../../src/r/io/fs_cabi.rs:961)
allocates its scratch frame only on the guest path. The same Blueprint and
14-checkpoint plan were run again against a separately rebuilt kernel:

| Native filesystem wait measurement | Baseline | Corrected |
| --- | ---: | ---: |
| Active stack bytes, captured RSP to owned stack upper boundary | 166,824 | 3,048 |
| Read-file dispatcher to host helper frame-pointer gap | 163,936 | 160 |
| Largest adjacent frame-pointer gap | 163,936 | 480 |
| Physical frames | 19 | 19 |
| Unreadable bytes | 0 | 0 |

The call chain remains intact. These byte counts measure the active stack at
that stop, not lifetime peak use. The guest path retains its existing buffer
and VMcall protocol.

## The metadata cause and correction

An earlier run wrote the file successfully, reached `fsync`, then failed with
`NotFound`. The metadata helper queried the Hull's private mount globals rather
than the host filesystem that had committed the write. The
[metadata helper](../../src/std_abi_shim.rs:2363) now uses the existing metadata-v2
CABI when executing in a Hull. It checks the complete 32-byte response, carries
the file/root identity and revision, preserves errors, and discards the async
operation. Native reads retain their existing host route.

The fixture keeps synchronization and metadata assertions in both the native
worker and Hull. Both 35-byte write/sync/read/reopen/remove roundtrips pass in
the corrected guest.

## Capture mechanics and validation

The packer opt-in retains frame pointers and authenticated debug sidecars in a
separate diagnostic cache. Actual emitted symbols and relocations select the
checkpoints: the text search's `next_match` is inlined, so an unrelated emitted
clone would give misleading coverage.

Nested VMX intercepts software breakpoints. A default-disabled, probe-only
hypervisor handoff exposes the latched guest registers, XSAVE, CR3/EPT and stack
mapping to GDB. It accepts the exact probe archive and whitelisted entries,
restores the original opcode, and resumes at the same RIP. The first caller at
an entry breakpoint is `[RSP]`; inherited RBP belongs to that caller.

Validation includes 97 packer tests, 24 capture tests, 29 focused kernel-source
regressions, portable fixture assertions, actual native packaging, two complete
passing guest runs, and an independent raw-stack/ELF audit. Each run used a fresh
owned snapshot QEMU and RAM filesystem; its VM was stopped after capture.

These are first-hit snapshots of selected real paths. Inlining, tail calls and
separate async continuations affect visible frames. Complete physical stacks
establish the selected caller chains; they do not establish every earlier or
later execution turn, every Codex CLI branch, or release-profile stack usage.
