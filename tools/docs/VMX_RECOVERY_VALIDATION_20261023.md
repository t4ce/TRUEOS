# VMX recovery validation — 2026-10-23

The final recovery kernel completed eight victim incarnations on one physical
boot: two sequences of parked kill, explicit native Rust panic, native
allocation failure, and another parked kill. The victim reused VM ID 1 across
generations 1–8 with distinct instance GUIDs. Each generation reached offline
with zero native jobs and a released carrier, while an independent peer
remained live within each sequence.

The primary evidence is [run4's machine-readable summary](../../bld/vmx-recovery-20261023/run4/summary.json)
and [captured kernel records](../../bld/vmx-recovery-20261023/run4/recovery.log).
Run4 directly requested every abandoned partial file and required HTTP 404
with the exact body `not found\n`. The earlier
[run3 summary](../../bld/vmx-recovery-20261023/run3/summary.json) records the
preceding four lifecycle cases; its filesystem absence check used omission
from a soft-capped HTML listing. Run4 repeated the suite with the stronger
direct check, without changing or rebooting the kernel.

## Artifact and boot identity

The [validated artifact manifest](../../bld/vmx-recovery-20261023/validated-artifacts.json)
records the runtime ELF, ISO, probe package hashes, and individual changed-source
hashes. Its Git base is `e86049f9d2c0b5035ce3390f6b35f5f552cc046f`; the source
hashes identify the tested working-tree changes beyond that base.
All 13 recorded production-source hashes still matched that manifest after
the final validation sequence; the runner-only HTTP check required no kernel
rebuild.

| Artifact | SHA-256 |
| --- | --- |
| Runtime `TRUEOS.elf` | `370ffc8c28c11c3b72bc157b05541b0dc98ef32cc24770d7c249ef1e28380edd` |
| Boot ISO | `7d46435e624594c7df9093c50ee63814b1c7b4e45c136e92ee60cf2201ec787a` |
| Victim `vm_recovery.bp` | `aa3c85c92da3121c93febd263f2f8bf734ce0f6bed8a73d8408bbf99d2c78e64` |
| Peer `vm_recovery_peer.bp` | `dbffcf23d082be341543983ed1086da5d29f1a84b26445acbdf7b9416c0a574d` |

[Boot4 reset evidence](../../bld/vmx-recovery-20261023/boot4/reset.json)
has status `verified`. The physical reset watcher observed reads of the staged
bootloader and runtime ELF and verified their file hashes, including the
runtime hash above. The source capture is
`bld/vmx-recovery-20261023/boot4/trueos-baremetal.0.log`; run4 began at
byte offset 275171 and saved its appended records separately. This identifies
the staged files and observed boot, rather than providing guest hardware
attestation.

The probes were embedded under local AppDB aliases `vm_recovery` and `vmpeer`.
The runner used Shell3 SSH sessions with real PTYs. There was no hardware reset
between the eight victim generations and no external application-catalog
publication in this validation route.

## Workload and physical results

Each victim instance had two Tokio runtimes with two workers each, a fifth
native thread parked on a channel, a TCP listener on port 32379, a published
UI4 frame, a 64 KiB vGPU allocation, and an intentionally incomplete streamed
filesystem write. A separate peer on VM ID 0 used port 32380, its own UI4/vGPU
resources and two progressing Tokio runtimes, without holding a disk write
lease. In run4 its instance stayed
`26999b67-989b-45c8-8c3d-32b710a3d45f` throughout the four cases.

| Victim generation | Trigger | Instance GUID | Native terminal carrier | Peer ticks0/ticks1, before → after |
| --- | --- | --- | --- | --- |
| 5 | Parked kill | `1b6d9b3d-188e-41cb-85f2-9175c0e7e4a8` | — | `24/24 → 55/55` |
| 6 | Explicit Rust panic on native thread | `18153ecf-afb6-4aed-86a8-da663558f671` | 7 | `92/92 → 134/134` |
| 7 | Infallible 4 GiB native allocation | `992cfc74-58be-44d3-bd2d-50570e78392e` | 12 | `176/176 → 218/218` |
| 8 | Parked kill | `7043801b-107d-45c1-93c4-ee61644288c2` | — | `251/256 → 280/285` |

The allocation case requested 4294967296 bytes from the victim's 536870912-byte
heap. The log records allocator failure and the standard allocation-error
message before `native-terminal`. The probe does not fill or touch the failed
allocation. This exercised a real infallible allocation failure, not an
explicit abort substituted for allocation.

For each victim generation, the generation-specific records showed:

- `filesystem-drained` with `fs_pending=0`, `fs_streams=0` and
  `async_fs_pending=0`, following an observed open-stream rollback request.
- `ui4-released` with one released surface and zero remaining surfaces and
  input routes/events/context menus.
- vGPU cleanup with one released device and no quarantined device.
- `network-released` with one closed Mio socket; the next instance rebound
  the same listener port.
- `offline native_jobs=0 carrier=released wait_queues_retired=5` in run4
  (six queues in each run3 victim case).

Both native failure cases also required a matching `native-terminal` event.
Every victim instance's committed witness was fetched and matched its GUID.
Run4 also directly requested
`/dl/2/apps/common/vm-recovery/abandoned-<instance>.bin` for each new GUID;
all four returned HTTP 404 with the exact expected missing-file body. A
listing omission, transport error or unrelated HTTP response cannot satisfy
that check.

The peer's two counters advanced across each recovery without its GUID
changing. After the run4 victim cases, peer VM 0 generation 2 was explicitly retired and also
reached zero native jobs, released UI4/vGPU/socket resources, and a released
carrier, with five platform wait queues retired.

The runner saved [victim SSH](../../bld/vmx-recovery-20261023/run4/shell3-victim.ansi),
[peer SSH](../../bld/vmx-recovery-20261023/run4/shell3-peer.ansi),
[completed cases](../../bld/vmx-recovery-20261023/run4/completed-cases.json),
the appended kernel log and `summary.json`. The summary reports `passed` for
all run4 checks. Run3's preceding generations 1–4 also completed all lifecycle
checks, with peer VM 0 generation 1 independently retired afterward.

A fresh SSH Hypervisor status check after both runs reported
`VMs: running=0 paused=0 starting=0 limit=64`. Its
[status text](../../bld/vmx-recovery-20261023/final-status/status.txt) and
[SSH transcript](../../bld/vmx-recovery-20261023/final-status/shell3-status.ansi)
record the final idle VM state.

## Native SDK shutdown route repaired

The first native panic test exposed a second terminal route. A std Blueprint
can define its own SDK `abort` symbol, so Rust's `panic_abort` runtime does not
necessarily call the kernel's imported libc `abort`/`sys_halt`. The SDK's
`panic_abort` instead calls `trueos_cabi_blueprint_shutdown` and then retains a
spin fallback if that call returns.

Previously, the shutdown CABI handled Hull execution through VMCALL but only
logged and returned success on a native carrier. The observed failed run printed
`blueprint abort` and `blueprint-shutdown`, then stayed in that fallback loop.
The linked object corroborated a defined SDK `abort`, an imported shutdown
CABI, and the following `PAUSE`/jump loop. The disassembly is retained in
[probe-panic-disassembly.txt](../../bld/vmx-recovery-20261023/host/probe-panic-disassembly.txt).

The [native shutdown branch](../../src/r/io/fs_cabi.rs) now transfers through
`terminate_current_guest` after logging and message access finish. The
existing `sys_halt` path uses the same boundary. A native terminal call resumes
its retained parent carrier, restores CR3/current-thread/WLS/allocation/domain
state, and only then requests forced teardown for the reserved VM generation.
No probe-specific panic replacement or kernel-wide panic escape was used.
Both runs' panic and allocation cases reached that native terminal marker and
completed teardown.

## Host checks

The saved host harnesses all passed. These counts overlap because some
harnesses include the same production lifetime tests; they are not a count
of distinct tests across the repository.

| Harness evidence | Passing tests | Relevant coverage |
| --- | ---: | --- |
| [scheduler.log](../../bld/vmx-recovery-20261023/host/scheduler.log) | 61 | Terminal transfer after restoration; live peer stack borrow; cross-stack wait cleanup before retirement; Hull/generation gating; no circular peer drain |
| [native-recovery.log](../../bld/vmx-recovery-20261023/host/native-recovery.log) | 17 | Queue purge without worker progress; queued/retained publication races; exact generation; collective-barrier wake edges |
| [native-lifetime.log](../../bld/vmx-recovery-20261023/host/native-lifetime.log) | 13 | Admission, close/pause/resume and retained reservations |
| [native-shutdown.log](../../bld/vmx-recovery-20261023/host/native-shutdown.log) | 3 | Actual shutdown CABI, completed logging before terminal transfer, Hull routing |
| [lifecycle.log](../../bld/vmx-recovery-20261023/host/lifecycle.log) | 17 | Forced native termination scoped to incarnation; stop/pause/control behavior |
| [fs-broker.log](../../bld/vmx-recovery-20261023/host/fs-broker.log) | 5 | Filesystem request ownership/lifecycle |
| [async-fs.log](../../bld/vmx-recovery-20261023/host/async-fs.log) | 8 | Async filesystem lifecycle |

## Bounded recovery contract

This validates cooperative parked work and explicit native guest terminal
boundaries. Forced discard purges proven nonrunning queued/retained service
jobs for the exact VM generation. Cancelled native stacks remain allocated
until the Hull is out of guest execution and every remaining native peer has
completed or acknowledged retirement. Host wait cleanup precedes that
acknowledgment. Each stack is subsequently released on its original carrier.

An arbitrary native spinner that never yields, returns or reaches a terminal
ABI is still outside this guarantee. Direct service-lane closures execute on
the shared host service stack and must finish cooperatively. Interrupt
delivery alone cannot authorize abandoning host locks, destroying an active
future, or releasing pages referenced by running peers. No asynchronous
interrupt escape or blanket executor-task deletion was added.

AP panic restart is blocked after an executor has been published to shared
scheduling, because replacing it could strand suspended guests and remote
submissions. The per-CPU panic recursion correction does not turn that restart
into a VM recovery mechanism.

Rust's panic strategy selects unwinding or an abort path; it does not reset an
AP. The earlier TRUEOS restart stall involved its global panic-recursion latch
and a deliberate second panic after replacement. Recursion state is now per
CPU and that second panic is removed. The old path replaced software executor
state rather than resetting the processor. Generic AP/kernel panic recovery
was not injected or proven by these VM tests.

See the [current lifecycle contract and embedded-probe recipe](vmx_lifecycle.md)
and the [historical source audit](VMX_SLOT_RECOVERY_AUDIT_20261023.md). The audit
describes the earlier queue/stack gaps; this report records the bounded
implementation and its physical evidence, including direct HTTP absence
validation in run4.
