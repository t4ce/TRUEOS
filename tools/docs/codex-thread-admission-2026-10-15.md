# Codex scoped-thread admission failure, 2026-10-15

The native regression reproduces the reported `std/thread/scoped.rs:206`
panic: after 20 simultaneous threads, the 21st `Scope::spawn` returns errno 11
(`EAGAIN`). WLS had split its reserved thread slots into fixed 20-slot Hull and
20-slot carrier partitions per VM. A partition could reject valid fanout while
most reserved slots remained unused. This is independent of the existing
64-entry per-process pthread registry and 256-thread global scheduler limit.

The repaired allocator leases from the same 2,560 reserved WLS thread slots,
keyed by VM and the complete tagged thread identity. The public 4,096-slot ABI
and admission limits are unchanged. A scheduler-owned lease supplies constant
time lookup while running and remains live until the thread stack and address
space retire. Simultaneous threads cannot alias a slot; join/detach registry
removal does not prematurely release it.

The current TRUEOS std and Tokio ports use OS-keyed TLS. Native testing checks
both actual Rust thread-local values and distinct/stable WLS slot numbers. The
unused legacy `no_threads` TLS backend was not changed.

## Original observation

The read-only physical capture resolved to `trueos-baremetal.0.log`. The first
snapshot read was 1,582,492 bytes, SHA-256
`2b9de0ea4a1e7fd571680257aa9d3069b3edb3df294e0dfaa998b0a08f376710`;
the collector continued growing. Its expected runtime digest matched the local
`bld/artifacts/debug-f9bace6397c8/TRUEOS.elf` exactly:
`9969094269f3e5a61b9e1c1f5be4e1112323ecaf46187fd96dea56926b3255ce`.

Code Mode emitted `codex.code_mode.nested_tool_dispatched` for `exec_command`
at capture line 9721, before the panic at line 10001. This supports the user's
shell-execution trigger. It does not prove whether Shell3, command preparation,
or a concurrent parallel file walker made the failing scoped spawn. Direct
`Scope::spawn` is the API identified by the panic location; the current trace
does not identify its higher-level caller. No raw prompts, credentials, account
identifiers or tool arguments are copied into this report.

The old pthread trace stopped after 16 successful creates, which hid later
rejections. Failed creates now always record fixed reasons, stack size and join
occupancy. Scheduler rejection records distinguish missing carrier, closed
guest, scheduler capacity, WLS identity, stack allocation, address space and
executor pool. No closure pointers or arguments are logged.

## Validation

The immutable probe package is
`8c9c149e519607a1d7a7da96fc0dd00d67888d1e6b268a5af95fb58329e42a2f`.
It holds eight background threads while creating 24 scoped threads, releases
and joins all 32, then repeats three times.

| Native run | Result |
| --- | --- |
| Previous release kernel, runtime `99cf34f8` | 20 admitted; 21st scoped spawn panics with errno 11 |
| Repaired release kernel, runtime `411ace49` | All three 32-thread rounds pass: 135/79/62 ms; 280 ms total |

The successful native run checks 96 total creations and joins, live WLS
uniqueness/stability, Rust TLS isolation and reuse after retirement. Safe logs,
full package/kernel digests and measured results are in
`tools/probes/thread_fanout/validation/native-{before,after}.{json,log}`.

Host harnesses pass 62 scheduler/WLS tests, four actual pthread-spawn diagnostics
tests, and 15 pthread semantics tests. Coverage includes 64 simultaneously live
scheduler continuations, VM/realm isolation, duplicate admission, full unchanged
WLS-pool exhaustion, retirement/reuse, join-table exhaustion and failures after
the success trace sample has ended. The standalone host fanout also passes.

## Artifacts and scope

The fixed kernel boot bundle is `bld/thread-admission-fixed/TrueOS.lz4`, and the
ISO is `bld/thread-admission-fixed/trueos.iso`:

| Artifact | SHA-256 |
| --- | --- |
| ISO | `80006c03250f93e1be5d17e07f50f18b665a0c356afd53520a0040f7fba787a7` |
| Runtime ELF | `411ace4902d495ff65f388266519285d04b238e542923f3e3a266847c377cafd` |
| Full symbol ELF | `23e0260f4148bd1852559b1efb5b3aa0ab696c70cbfcdea635c8b145af1d865c` |

No Codex/model/prompt change is required for this kernel repair. The probe is
registered only in the SDK probe catalog and published to the authorized Apps
directory. Physical hardware was not reset or deployed; a complete authenticated
Codex `gri` turn still needs checking on the updated physical kernel. The
controlled reproduction establishes the admission defect and its repair,
without attributing every remaining responsiveness issue to it.
