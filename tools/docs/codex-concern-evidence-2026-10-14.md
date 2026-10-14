# Codex evidence mapped to the user's concerns — 2026-10-14

## Result

Follow-up: [QEMU debugger evidence](codex-qemu-deadlock-2026-10-14.md)
reproduces this progress pattern and identifies a file-registry spinlock held
by a parked filesystem caller while another worker spins on the same AP.
The broker result is already complete and the owner queued. This is proven
for QEMU; the physical instruction pointer remains unverified. The findings
below describe the original physical capture and its limits.

The coordinated hardware probes identify one Tokio native continuation that
stops returning on LastAP while other continuations become ready. This is a
localized application progress failure. The exact instruction and the reason
it does not return remain unknown. Single-AP placement, memory/TLS bookkeeping
and the interrupt contract have not been cleared as causes.

This investigation changes no runtime policy. It preserves the current
single-AP experiment and gathers evidence before fan-out or further fixes.

## Evidence bundle

The immutable snapshot is
`bld/codex-evidence/20261014-140346/capture.log`, copied from
`bld/baremetal-logs/trueos-baremetal.0.log` at 14:03:46 Europe/Berlin.
SHA-256:
`1a63f419edb02f610f832bdb50b447af5d770d94a7d5ee66fe36a3cbb7beb805`.
Its source tree was clean at commit `67816b092` (full hash in `records.json`).
This records the local source version; it is not a byte-for-byte attestation
of the deployed rig kernel/package.

The same directory contains parsed records, startup markers, `summary.json`,
`checks.json` and excerpts retaining the original snapshot line numbers.
The fresh read-only SSH SMP monitor capture is saved as `smp-terminal.bin`,
`smp-terminal.txt` and `smp-excerpts.json`. The observer session was closed
without selecting, restarting, pausing or stopping Codex.

## What the hardware capture establishes

| Event | Time / state | Snapshot evidence |
| --- | --- | --- |
| First frame flushed | 141 ms | line 942 |
| Auth storage finished | 287 ms | line 990 |
| State DB initialization began | 35,224 ms | line 1103 |
| State DB initialization finished | 38,946 ms | line 2055 |
| App server initialization began | 38,955 ms | line 2064 |
| First sampled persistent stall | AP sample 9; worker already in its stage for 3,360 ms | lines 2104–2105 |
| Last sample in the saved snapshot | AP sample 45; worker stage age 183,360 ms | lines 2600–2601 |

The 34,937 ms between auth-storage completion and DB initialization is an
unattributed interval containing multiple startup operations. DB initialization
itself takes 3,722 ms. No app-server completion or session-ready marker occurs
in this snapshot. Rendering a composer, and even typing into that startup
composer, do not establish that session initialization has completed. The
screenshot's 38,955 ms agrees with this run's app-server entry marker; the
canonical log is used to distinguish stage names from overlapping terminal
text.

The stable worker tuple is:

```text
vm=0 gen=2 id=2147483653 carrier=19 name=tokio-rt-worker
stage=guest-enter seq=592 phase=1 wait=0x0 if=0
age_ms=3360 ... 183360
```

Across 37 consecutive AP samples, the outer executor counters stay at
`poll_b=798236 poll_e=798235`, with `current=1`. Ready tasks increase from
two to five. The worker's identity, VM generation, stage and stage sequence
stay unchanged across 180 seconds of independent sampling. Stage age advances
by exactly 5,000 ms per sample. The observer and SSH/Shell3 continue to run.
The subsequent read-only SMP monitor still shows AP19 busy inside
`codex_application_task`, with six spawned tasks and five ready. Its capture
is a later observation, not a timestamp-aligned part of the saved log window.

The `guest-enter` marker is written after CURRENT, realm, allocation context,
WLS and address space have been installed, immediately before the stack swap.
The next marker would be `guest-return`. Therefore the unfinished window
includes stack switching, the continuation and any kernel ABI calls it makes.
It does not yet identify a guest instruction or prove the installed context
was correct.

## Mapping to the concerns

| Concern | Evidence and present conclusion | What remains unresolved |
| --- | --- | --- |
| One application AP should run the whole workload; debunk before fan-out | It dispatches hundreds of thousands of executor passes initially. Later one pass remains unfinished while five tasks are ready. This demonstrates lost progress, rather than establishing insufficient core count. Keep single-AP placement for localization. | Whether a synchronous wait requires parallel/preemptive progress, or whether our continuation machinery is wrong. Fan-out is deferred and is not the diagnosis. |
| Single-AP isolation gives no assurance about memory | Correct. Reaching `guest-enter` proves the installation code reached its last marker. It does not validate allocation ownership, object lifetime, TLS contents, pointer mapping or lock owner identity. | No captured instruction, stack or memory invariant establishes memory correctness or corruption. A corrupted context could cause the executing worker to spin or wait incorrectly. |
| The failure may be our thread bookkeeping | The observed frozen stage is beyond the outer context-installation boundary, rather than at the outer timing/wake/control or restoration markers. This narrows the location. | The installed state could already be wrong, and our pthread/TLS/wait/allocator bookkeeping can also execute inside the continuation through ABI calls. Bookkeeping is still a first-class suspect. |
| Ordinary threads might rely on an action caused by an interrupt that this adaptation lacks | IF was sampled clear at worker entry. Source shows the AP idle path enables interrupts with `sti; hlt`, then disables them again. The remote-work ISR acknowledges the IPI without dispatching another continuation; the timer ISR refreshes time and acknowledges. These paths do not provide native-thread preemption. | We have not established which operation requires that contract, or whether the worker later changes IF. The IF field is a last-boundary sample, not a live read of the frozen CPU's flags. |
| Local symptom fixes could compensate for a missing contract | The first frame is fast, startup has a separate roughly 35-second gap, and then a particular worker stops returning. Source-level sleep tuning would not identify the stopped worker's operation. | The startup gap and permanent freeze may have different causes. There is no controlled A/B evidence here attributing a speed advantage to placement alone. |
| Multiple probes should produce a pattern that is difficult to dismiss as harmless | Unchanged worker sequence and generation, increasing age, an unfinished outer poll, current-task presence, rising ready count and a continuing independent observer agree for three minutes. This is materially stronger than one long-wait log. | The pattern establishes stalled progress but not a specific deadlock, lock address, lost wake, memory fault or missing-preemption dependency. |

## Interrupt observations and their limits

Before the stall, the sampled windows record 143 remote-work IPI sends, 15
deliveries and three timer interrupt deliveries on AP19. This establishes
that the instrumented delivery paths have operated on that AP during the run.
Successful dispatch also occurs with IF sampled clear, so IF=0 by itself is
not sufficient to explain this failure.

At the first stalled sample, the window includes 29 IPI sends and no recorded
delivery. Subsequent saved stalled windows report zero new IPI sends, zero
deliveries, zero idle timer arming and zero timer delivery. Meanwhile the
ready count grows. There is no evidence that the AP repeatedly services an
interrupt and then fails to select a sibling during this frozen window.

These probes count only the remote-work IPI and the local idle TSC-deadline
timer. They do not count every possible hardware interrupt. IPIs can coalesce,
timers can be disarmed, and a continuously ready AP need not arm an idle timer.
Send and receive totals need not match. None of these zero counts independently
proves a lost interrupt or proves enabling interrupts would fix the program.

## Next evidence to gather, with placement unchanged

1. Locate the instruction/call inside worker `2147483653` during the frozen
   stack-switch window. Use an emulator debugger when reproduced there, or
   a separately designed hardware sampler that can observe an interrupt-masked
   AP. The existing regular-IRQ path cannot be assumed to sample it.
2. Add only narrow boundaries around the identified operation's kernel ABI
   entry/exit. For locks/waits, capture logical waiter and owner identity and
   observed/completed wake generations. For context-sensitive operations,
   verify VM generation, TLS slot and allocation realm without dereferencing
   arbitrary guest pointers from the observer.
3. Reproduce that operation with two actual continuations on one carrier,
   covering wake-before-wait and wake-after-wait. Keep instrumentation as an
   independent toggle and use the same workload. Do not change scheduling,
   sleeps, worker counts and memory policy together.

The existing host tests establish particular explicit wait/context-switch
properties with mocked hardware services. They do not substitute for this
baremetal finding or clear the real memory/interrupt contracts. No new
application response test is claimed by this evidence-only investigation.
