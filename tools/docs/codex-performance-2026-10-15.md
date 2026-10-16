# Codex runtime performance after the first Gridpaper success

The operator's next physical run completed the unchanged Luna/low → Code Mode
→ native Shell3 → `gri` → Gridpaper scenario in **1m33s**, versus **9m13s**
before the previous repairs. The screenshot identifies app `d113417d`.
This is an observed whole-scenario improvement; it does not isolate the
individual contributions of the earlier allocator, history and hook changes.

## Remaining cost map

The safe baseline is `bld/codex-performance-20261015/baseline.json`, generated
from the first 1,907,186 bytes of `trueos-baremetal.0.log`, SHA-256
`d2735a94d6f221cd1a3dd017695331554a0f0bef4cb4e7bb452c3abdc509f58f`.
The physical kernel runtime is
`f17cc146aef4c7514666aed24a51d5f0c95beaa9ac39eb8bae6a96dfa646e19e`,
debug/opt1, built from `fd87202c5e97` plus the bounded-registry changes.

| Observed boundary | Time | Implication |
| --- | ---: | --- |
| Prompt to first real request | about 78 s | Most of the remaining delay precedes inference. |
| Turn context recording | 43.026 s | Contains rendering and persistence; internal subphases were previously missing. |
| MCP readiness in the main turn capture | 19.771 s | Connector startup/body progress still matters; this is not router construction. |
| Display-root work | 7.007 s | Overlaps context recording, not an additional seven seconds on the critical path. |
| Prompt hooks and input recording | 0.790 s | The previous seven-minute input stage is gone. Hook inspection itself is 0 ms here. |
| First router construction / subsequent construction | 74 / 55 ms | No evidence for a router cache rewrite. |
| First / second real response TTFT | 977 / 1,528 ms | Response-stream latency remains much shorter than whole-turn latency. |
| One SQL Execute reply-header wait | 34.557 s | A long established reply wait, not a slow database open. |

The asynchronous filesystem measurements distinguish producer work from
consumer delay. Sampled metadata/listing operations have 0 ms service time and
1 ms queue time, but their consumers resume as much as 6,786 / 10,606 ms later.
The single sampled slow write spends 335 ms in service. These sampled maxima
are neither all operation counts nor additive critical-path measurements.

The same period contains Tokio worker continuation intervals of
13.182 / 10.763 / 10.602 s, with hundreds of thousands of completed allocation
ABI calls. The free list reaches 81,493 nodes. This establishes allocation churn
and delayed consumers; exit call stacks ending at `yield_now` do not identify
where the preceding CPU time was spent.

MCP initializes and receives HTTP 200 quickly, then tools/list bodies stall
after partial delivery (122,880 bytes in one attempt, 90,112 in another).
The first tools/list operation expires after 33.588 s against its existing
30-second deadline. Controlled native TLS/HTTP2 probes previously transferred
330,447 bytes in about 238 ms. That control does not reproduce physical Codex
contention or the production WAN endpoint. Startup timeouts were not raised.

## Improvements implemented in this pass

### Ordered allocator destruction

The address-band index still restarted each free at the band's beginning.
Consecutive frees could repeatedly scan a growing prefix of free nodes. An
optional validated insertion hint now follows the surviving coalesced node.
Allocations, heap installation and restore-index rebuild invalidate it.
Existing fence validation remains in force. Tags, provenance, first-fit
selection, image format and the shared 4 KiB metadata allocation are preserved.

Fourteen production allocator tests pass. For 35,000 ascending frees, measured
node reads fall from **47,280,424 to 102,256**. Reverse order stays at 35,000;
random order has little benefit. See
`bld/codex-free-finger-20261015/host-acceptance.json`.

The identical native allocation probe `35ac4304` independently passes on both
isolated release kernels:

| Probe phase | Bounded registry only | Registry plus insertion hint |
| --- | ---: | ---: |
| Fill 70,000 allocations | 28 ms | 26 ms |
| Reallocate at capacity | 1 ms | 1 ms |
| Free alternate 35,000 blocks | 249 ms | 4 ms |
| Refill | 12 ms | 11 ms |
| Reallocate after churn | 1 ms | 1 ms |
| Verify/free all remaining blocks | 10 ms | 8 ms |
| Complete probe | **307 ms** | **57 ms** |

These are single-run, millisecond-resolution release-VM measurements, not a
prediction for the physical debug build or full model turn. Content, ownership
and realloc preservation checks pass. Evidence is under
`tools/probes/alloc_registry/validation/native-free-finger.json`.
The new kernel is `bld/codex-performance-fixed/TrueOS.lz4`, runtime SHA-256
`e5d372bf55057ca13250f83b2933a7cb5f6c3b79f25197df5f21d25bce58353e`.
The isolated VM was stopped after capture; the physical rig was not reset.

### Database snapshots and framing

Previously, depth-zero Prepare and Execute serialized a complete before image
and after image even for plain reads. A 64 MiB test database copies
134,373,376 bytes per command. The service now asks SQLite's compiler whether a
snapshot can be avoided. Until protected, its temporary authorizer permits only
Read/Select/Recursive actions. Functions, all PRAGMAs, writes and unknown
actions fall back to the conservative snapshot path. This is not SQL text
classification. Existing connection authorization is restored for trusted
snapshot operations and normal execution.

The guard matters because PRAGMAs can act during preparation. Tests cover
encoding, schema/application/user versions, page size, auto vacuum, journal
mode and writable schema. Another regression covers no-op
`DROP TABLE IF EXISTS`: it can report writable without invoking the authorizer,
so its fallback must disarm the temporary guard before snapshot serialization.
Mixed scripts retain statement order, rollback and publication-before-success
semantics; explicit transactions keep their existing snapshot boundaries.

With coalesced framing held constant, the controlled host benchmark changes a
64 MiB Prepare from **48.112 ms to 0.035 ms**, and a plain read from
**48.749 ms to 0.041 ms**. Snapshot bytes for those proven-safe operations fall
from about 128 MiB to zero. These are service/host measurements, not an
attribution of the physical 34-second wait. Functions and PRAGMAs deliberately
retain their conservative copy cost.

Both RPC directions also serialize the existing four-byte length prefix and
JSON into one frame, avoiding separate small header/body writes. Old and new
peers remain compatible; optional numeric correlation metadata is ignored by
old peers. The portable encoder uses the public alloc-only `to_vec` API and
moves the payload once in that Vec to insert the prefix; it does not require
Serde's std-only writer API. A Linux-only Ping floor fell from about 41 ms to
well below a millisecond. Native
TCP already disables Nagle, so that Linux floor is not claimed as a physical
TRUEOS gain. Fewer frame submissions/copies remain useful on both platforms.

### HTTP chunk ownership

The local HTTP body API copied owned `Bytes` into a `Vec`; MCP then copied that
buffer again into its JSON aggregate or converted it back to `Bytes` for SSE.
The new `recv_bytes()` preserves ownership through those boundaries. The
existing `recv()` API retains its Vec behavior, sequence checks, EOF, errors,
backpressure and cancellation semantics. A pointer-identity regression verifies
16 × 16 KiB of payloads retain their storage. MCP JSON still performs its
necessary aggregate copy; SSE forwards owned chunks directly.

All 41 HTTP/MCP checks pass, including local ownership/cancellation, eleven
remote-stream regressions, 22 adapter checks and six timing checks. No whole-
turn physical speedup is assigned to this smaller copy reduction.

## Measurement coverage for the next run

- Five context phases split world-state generation, rendering, history
  recording, snapshot persistence and settings checkpointing. Five existing
  context regressions pass; persistence work and order are preserved.
- MCP body progress records cumulative bytes at geometric thresholds from
  64 KiB. There are at most 48 observations across the u64 thresholds, and a
  large chunk does not emit a burst for skipped thresholds. A stalled-body test
  verifies progress appears while collection is still pending and JSON decode
  has not begun.
- SQLite service samples report lock wait, blocking queue, snapshots, execution,
  reclamation, publication, restoration and reply serialization/write durations
  and byte counts. The first 16 samples are retained; later eligible samples
  share a 250 ms rate cap and report skipped counts. Explicit timing-all mode
  is confined to the controlled benchmark. Optional connection/request numbers
  aid correlation without SQL, paths, parameters or account identifiers.
- The offline reporter retains filesystem timing/pending observations and
  service samples, validates static enums/numeric fields, and preserves absent
  measurements and older reports. Correlation numbers are process-local hints;
  a service VM number is not the client's identity. Overlapping stages are not
  added to whole-turn elapsed time.

## Remaining candidates kept in view

| Candidate | Evidence / decision |
| --- | --- |
| Allocation first-fit scans through unsuitable holes | A synthetic 81,493-hole fixture needs 8.15 million node checks for 100 medium requests, while small requests need only 100. Band maxima or a negative-prefix cache could help; actual physical search lengths still need measurement. No speculative allocator-policy rewrite landed. |
| Size summaries | A 1 KiB summary fits the existing state page. It must never underestimate after split/merge/restore; alignment-only misses also limit it. Candidate details are in `bld/codex-first-fit-audit-20261015/candidate-audit.json`. |
| Removing duplicate C allocation metadata | Deferred: authoritative tags omit original size/alignment. Safe removal needs versioned metadata and heap-image compatibility, not just deleting the bounded ledger. |
| Remaining ordinary hook transcript barriers | Observed 248 + 72 ms in this run. Worth revisiting if repeated often; they do not explain the 43-second wait. |
| Whole-file write amplification | Still a foundation to watch, but this capture's slow metadata/listing samples finish promptly and wait on consumers. Do not blame filesystem execution for consumer delay. |
| Router/schema caching | Already 55–74 ms here. Avoid a new invalidation/cache contract without a measured cost. |
| TCP options / delayed acknowledgements | Native Nagle is already disabled. Coalesced framing is implemented; unrelated socket-option work is not presented as a fix for the measured long wait. |
| MCP readiness policy | Tools/body startup still times out. Preserve feature readiness and deadlines while measuring partial delivery and shared scheduling; do not make it appear fast by disabling connectors. |
| Synchronous DNS waiting on a shared TLS executor | An isolated second Appstore fetch failed before its destination TCP connection after 30.007 s. The synchronous DNS wait can block a sibling TLS service if both share an AP. Actual TLS placement was filtered from this capture, so starvation remains a hypothesis. The exact evidence and bounded next measurements are in `bld/codex-performance-20261015/appstore-dns-audit.json`. |

## Built artifacts and validation limits

Published packages were read back from the operator's Apps share and checked
against their local SHA-256 digests:

| Package | Packed identity | Purpose |
| --- | --- | --- |
| Codex | `33a8e6a3fd4075e8230a3006caa12acc845e2a85128822a9e7b0fe7482308c45` | Context measurements, HTTP ownership/progress, coalesced SQLite client frames. |
| SQLite | `08a5b1efc5d7238b77aab315c08b8df817133d768b535649abe8d61ae59e0a87` | Conservative snapshot avoidance, coalesced replies, bounded service timing. |
| SQLx probe | `6ef7681abb1ec29f02e7283cab45afd2a43af610450a30d24b17c900142190b3` | Ordinary unprivileged contract probe, without an embedded service. |

The final native builds and C ABI guards pass. Allocator behavior was exercised
on the new native kernel. Database transaction/migration, rollback, publication
and old/new-client contracts pass against the real service on the host. The
isolated native SQLx attempt did not reach SQLite: service discovery encountered
the separate Appstore/DNS failure above. An embedded-service fixture also
stopped at the package-import scope check; global filesystem access was not
granted to the probe. Neither setup failure is counted as a database test pass
or database query failure.

The report/parser suite has 22 passing checks, alongside the 41 HTTP/MCP checks,
five context regressions and 14 allocator checks. Package receipts are in
`bld/codex-performance-20261015/published-apps.json`; the native kernel acceptance
record is `bld/codex-performance-fixed/acceptance.json`.

The next physical run should be correlated with the new app, service and
kernel identities. The 1m33s physical result predates this pass; full-scenario
timing with these updates remains to be observed.
