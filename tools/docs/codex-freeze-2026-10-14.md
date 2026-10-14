# Codex LastAP freeze observation — 2026-10-14

This is a read-only investigation. No scheduler, polling, runtime or deployment
changes were made. The log selected by `LatestOfThree.logs` resolved to
`bld/baremetal-logs/trueos-baremetal.0.log`. A copy taken before the live SMP
inspection is at `bld/codex-freeze-observation/capture.log`; the observer's
terminal bytes and stripped text are alongside it.

## Observations

* The experimental placement is active: vm0 thread 1 and its child admission
  records select carrier 19 and `codex_application_task` (capture lines 746,
  781). The current policy routes all native Codex continuations there.
* The first frame flushes at 142 ms (line 879). This is output completion,
  not session readiness or physical display visibility.
* Auth storage finishes at 307 ms (line 930); state DB begins at 35,194 ms
  (line 1028). The intervening 34,887 ms is not yet attributed to an individual
  startup operation. Several operations lie between these markers; do not
  call the entire gap authentication or database time.
* State DB finishes at 37,973 ms (line 1789), taking 2,779 ms. App server
  initialization begins at 37,983 ms (line 1798). No `app_server_done` or
  session-readiness marker appears in this copied capture.
* Seven subsequent-to-initial run-slice summaries show counts of 182,347,
  113,853, 101,652, 102,678, 102,340, 102,472 and 102,517. The site's nominal
  interval is five seconds. This is roughly 20,000–36,000 returned native
  execution slices per second, not a count of model tokens or useful tasks.
  Most later windows have maximum returned slice duration of 1 ms and
  wake-to-poll maximum of 1 ms. Many resumptions are happening before the
  freeze; these numbers alone do not identify polling, contention or progress.
* The last Codex ready/run summaries occur at lines 1025–1026. SSH routing
  and Shell3 service summaries continue for hundreds of seconds afterward.
  The host therefore continues executing these services. This is not evidence
  that every network path or external request is healthy.
* One stdin queue sample has maximum age 32,853 ms (line 1034). Its bytes
  are not correlated to a specific user keystroke; it could include terminal
  replies or startup input. Output queue samples before the freeze have
  maximum age 10 ms. These samples do not explain the later freeze.
* A new SSH session opened the existing SMP monitor without selecting or
  stopping Codex. During the four-second monitor capture, AP19 shows
  `current:codex_application_task s=6 r=5` and a busy activity history. AP18,
  for comparison, shows an idle service lane. The monitor was exited and
  the observer SSH session closed; the Codex process was left intact.

## Interpretation and limits

The combined observations strongly suggest a native continuation remains
inside its poll while ready sibling tasks wait on the same cooperative AP.
The exact instruction, lock owner, or blocking operation has not been
captured. A spinlock, nonyielding synchronous call or long computation remains
a hypothesis; none is proven as the culprit. Existing log semantic-gap text
is not enough to establish which staged Tokio/std implementation was linked.

The run-slice timing is recorded only after context switching back to the
executor. It cannot report the duration of a slice that never returns.
Ready-to-poll is also reported by the observed task. Silence from those sites
cannot by itself distinguish stuck execution from parking or a missing wake.
The independent SMP observation supplies evidence that the shared carrier is
occupied while other tasks are ready.

The startup delay and the freeze are two separate outcomes. This capture
stalls during app-server startup; it does not contain a completed model
response or establish the cause of the user's later post-login freezes.

## Execution model

There are two scheduler layers: TRUEOS schedules native stackful continuation
tasks on AP executors, while Tokio schedules application futures on its worker
threads. With all native continuations on LastAP, multiple logical workers
share one physical execution carrier. An async function returns control only
when its current poll returns. A synchronous spin loop inside that poll does
not become preemptible because its caller is async.

The TRUEOS std-thread implementation is itself a cooperative stackful task.
Ordinary placement can give a lock holder and waiter different APs, allowing
the holder to make progress even while a waiter spins. LastAP colocation can
expose a dependence on parallel progress. An OS with preemptive scheduling can
also run the holder after interrupting the waiter. Neither comparison implies
that sync is inherently faster or that async lacks CPU capacity.

Work stealing distributes ready application futures. It does not interrupt
an already executing poll or make a blocking lock implementation cooperative.
Additional APs can help useful parallelism; CPU waste depends on polling and
contention behavior, not on work stealing alone.

## Coordinated progress probes

The next diagnostic implementation keeps single-AP application placement. It
adds passive observations, without changing interrupt flags, sending extra
IPIs, adding preemption or changing polling intervals. It has not been booted
on the rig or incorporated into the running emulator image yet.

`CODEX_PROGRESS_PROBES_ENABLED` independently gates the instrumentation.
The central service registry starts `codex-progress-observer` locally on the
BSP when the experimental LastAP spawner is available. Every five seconds of
active LastAP task ownership it emits one AP summary and at most four native
job records, using the existing Blueprint/Info policy. ISR instrumentation
only increments atomic counters; there is no allocation, lock or logging in
those added ISR hooks.

The AP summary reports executor poll entries/exits, current-task presence and
ready count, plus per-window remote-work IPI sends, send failures, deliveries,
idle timer arming and timer interrupt delivery. Counts are observations, not
an interrupt-to-thread scheduling mechanism. APIC IPIs can coalesce; timers
can be disarmed before firing. Equality of send/delivery counts is not a
correctness condition. `dt_ms` records the observer's actual sampling interval.

The job records bracket poll entry, timing/wake bookkeeping, control checking,
wait polling, context installation, guest entry/return, context restoration
and poll return. They include VM generation, native thread identity, last
stage, stage sequence, elapsed stage age and the sampled interrupt-enable
flag (IF). The atomic stage tuple is sampled with an odd/even sequence; racing
updates report `snapshot=changing`. Job phase and queue information remain
approximate observations. A stage tells us the last reached boundary, not
the current instruction pointer.

The independent observer reads host-owned diagnostic records, never raw
cross-AP Thread/guest-stack pointers. Roster and identity locks use try_lock;
busy locks are reported explicitly instead of blocking the observer. Running
records are prioritized, but four rows cannot describe all possible workers
or VMs. No conclusion that *all* workers are parked follows from that cap.

| Combined pattern over multiple samples | What it directs us to investigate |
| --- | --- |
| Stable job sequence at `poll-begin` or `control-check`, rising age, ready siblings | Timing/wake or lifecycle bookkeeping before guest execution |
| Stable `context-install` or `context-restore`, rising age | Realm/TLS/allocation/address-space bookkeeping |
| Stable `guest-enter`, current task present, ready siblings | Guest execution not returning; operation or wrapper entry needs localization |
| IPI sends increase, delivery stays flat, IF sampled clear, current poll remains occupied | Masking/delivery and the expected wake contract; not proof of a lost interrupt |
| Interrupt delivery increases, but job sequence/poll completion stays frozen | Handler execution is not yielding the carrier or completing the required operation |
| Outer executor poll exit stays flat while native job sequences advance | Executor drain/dispatch remains active; distinguish this from one stuck continuation |
| Job sequences advance rapidly but the application operation never completes | Busy retrying, contention, ineffective wakes or another unmet dependency |
| Wait stages persist and no relevant wake is observed | Completion source and waiter registration; legitimate idle waiting is also possible |

Read all columns together. The AP loop currently uses interrupt-disabled
execution after an idle return; `sti; hlt` is its interrupt-enabled idle path.
The remote-work handler acknowledges the IPI and does not itself dispatch a
logical thread. A clear IF sample is therefore not automatically a regression.
Whether an operation *requires* interrupt-driven scheduling remains a separate
contract question.

## Next experiments

1. Capture these probes during the unchanged single-AP Codex freeze. Debunk
   wrapper bookkeeping, waiter registration and interrupt/progress assumptions
   before changing placement or adding fan-out.
2. Once localized, exercise that operation with two production continuations
   on one emulator AP, including wake-before-wait and wake-after-wait. Add
   narrow call boundaries around the startup gap separately.
3. Fan-out and split main/child placement are deferred. They can later compare
   scheduling contracts, but should not hide the current failure before it is
   understood. Arbitrary native preemption is also deferred.

Validation: `cargo check --offline` and the production scheduler/wait host
harness pass. Its 42 tests include bounded/nonblocking independent snapshots,
stage transitions, event delivery separated from poll completion, and an
external notification while a native continuation is running. The latter
uses a host thread as a notification source and a bounded release; it is not
a hardware IRQ test and does not establish the cause of the Codex freeze.

The host scheduler tests cover explicit yielding, parked waits, wake forwarding
and lifecycle transitions. They do not establish that every synchronous
library used by Codex is safe when all its threads share one cooperative AP.
