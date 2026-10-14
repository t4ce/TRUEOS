# SSH input latency investigation — 2026-10-14

The target is steady-state input and redraw responsiveness over SSH. Startup
readiness and model/tool execution latency are separate measurements.

## Initial baseline

The probe opens an SSH session with a 120×40 PTY, disables local echo, waits
three seconds, then enters 30 distinct printable UTF-8 characters one at a
time. It waits for each character in remote output and erases it without
submitting a command. This measures local write to returned output bytes,
including SSH and transport; it does not measure physical display visibility
or isolate network latency. Both sessions were Shell3, not Codex.

| Endpoint | Samples | Median ms | p95 ms | Maximum ms |
| --- | ---: | ---: | ---: | ---: |
| QEMU, localhost:22294 | 30/30 | 31.95 | 44.79 | 44.99 |
| Physical rig, 192.168.178.94:22 | 30/30 | 37.32 | 45.32 | 47.80 |

Evidence is in `bld/emulator-logs/codex-ssh-latency-20261014-123957/`:
`shell-baseline/result.json`, `rig-shell-baseline/result.json`, terminal byte
captures and chunk arrival timestamps. These are single baseline runs, not
statistical performance bounds. Similar distributions are grounds to inspect
shared polling paths, not proof that the network has no contribution.

QEMU uses the existing ISO, 14 virtual CPUs, 8 GB RAM, KVM with nested VMX,
EGL headless display, and a temporary disk snapshot. SSH is forwarded to
localhost:22294; HTTP to localhost:18094. `run.json` records the process ID
and launch arguments. No kernel or Blueprint was deployed to baremetal.

## Execution route and candidate delays

The existing October 14 Codex diagnosis records native std-thread execution.
The route of the next measured Codex instance must be verified again; do not
assume VMX scheduling controls native carriers.

* `src/shell3/net.rs`: the SSH transport routing task unconditionally waits
  10 ms after each pass.
* `src/shell3/service.rs`: with attached terminals/shells, the service waits
  10 ms after each pass. Remote app output calls `notify_work()`, but that
  branch awaits a timer rather than the work notification.
* `src/r/platform.rs`: diagnostic messages explicitly describe Tokio runtime
  parking and scheduler condvar fallbacks implemented through platform
  sleep/poll. This is a source-level candidate, not a measured delay.
* `src/allcaps.rs`: the network idle polling soft cap is 5 ms. Determine
  whether it is active during the measured input before tuning it.
* Codex's frame rate limiter has an 8.33 ms minimum interval. Trace actual
  frame deadlines and runtime wakeups before changing it.

VMX cooperative yield already has no minimum timed delay. Native PAUSE is
enabled when the preemption timer is available; intercepted PAUSE sleeps
1 ms. The lifecycle preemption quantum is 125 ms. Those values belong to a
different execution path unless evidence shows the workload uses it.

## Two investigation tracks

The current experiment changes only Codex continuation placement and adds
timing summaries. It does not change the polling intervals described above.

`src/allcaps.rs` exposes two independent switches:
`CODEX_EXECUTOR_EXPERIMENT_ENABLED` and `INPUT_TIMING_ENABLED`, both enabled
for this experiment. Codex and codex-check native continuations (including
their std/Tokio child threads) use `codex_application_task`, submitted with
TRUEOS's Spawner to LastAP. The existing stackful continuation keeps pthread
identity, errno, realm restoration, wait queues and pause/kill handling.
The ordinary std-thread round-robin selector is bypassed for these archives.

LastAP is selected by topology identity, never registration order. Ordinary
std threads, service lanes, compute placement and Hull lane selection exclude
it. There is no preceding-AP reservation. Laptop MicroFont remains a regular
service with its existing iGPU identity gating. On insufficient topology
(fewer than four slots or LastAP outside the tracked slot limit), experimental
Codex admission fails with EAGAIN instead of silently changing placement.
Disable the placement switch to restore the general selector.
Codex continuation timings remain enabled in that mode, allowing the same
metrics to compare ordinary and reserved placement. Disabling the timing
switch also removes Codex's wrapped timing waker.

Timing sites aggregate per carrier. Each emits the first observation and then
at most one Info summary per five seconds of activity, with sample count,
integer mean/max milliseconds and counts at or above 10/50 ms. All samples
contribute; per-character content is never logged. Pending summaries emit on
the next observation after the period; there is no new logging task. Admission
logs use the existing callsite limiter (first two, then every 64 admissions).
The summaries use the Blueprint logging area, whose Info policy already
admits them without widening Service or Global logging.

Validation: `cargo check --offline` passes. The production scheduler/wait,
remote terminal, Matrix, MicroFont and reservation-policy host harnesses pass
67 tests in total, including wake forwarding/replacement, timed pause/kill,
bounded summary accounting and partial output drain age. This change has not
been rebuilt into the running QEMU ISO or deployed to the physical rig; the
baseline above predates the experiment and is not a measured improvement.

| Site | Meaning |
| --- | --- |
| `codex-ready-to-poll` | Earliest pending wake to executor poll; initial sample is admission to first poll |
| `codex-run-slice` | Synchronous guest execution until yield/wait/return; excludes host summary logging |
| `codex-sleep-overrun` | Actual suspended sleep minus requested duration, clamped to zero |
| `codex-wait-elapsed` | Entire wait-queue suspension; includes legitimate idle waiting |
| `ssh-routing-pass` | Work performed during one SSH routing pass |
| `ssh-routing-sleep-overrun` | Actual 10 ms routing sleep minus 10 ms |
| `shell3-service-pass` | Work performed during one terminal service pass |
| `shell3-service-sleep-overrun` | Actual 10 ms service sleep minus 10 ms |
| `app-stdin-queue-age` | SSH/terminal bytes enqueued to app read; oldest queued burst age for partial reads |
| `ssh-app-output-queue-age` | App bytes enqueued to remote frontend drain; oldest queued burst age for partial drains |

Queue timings exclude earlier SSH transport and later socket flushing. They
are not a complete input-to-visible-frame measurement. The wrapped app waker
retains the earliest outstanding notification and forwards every wake to the
original executor waker. Millisecond timer resolution cannot distinguish
submillisecond work. A deliberate lifecycle pause can extend wait/sleep times;
use native lifecycle diagnostics when interpreting those samples.

The following broader investigation is deferred until this placement/timing
experiment is measured:

1. Attribute latency with correlated timestamps at network event admission,
   SSH routing, Shell3 input dispatch, app stdin read, Codex key handling,
   frame request/draw/flush, remote output enqueue, and SSH output drain.
   Record scheduled deadline versus actual resume time for native carrier
   waits. Use host monotonic time for host segments and local elapsed time
   for app segments; do not subtract unsynchronized clocks.
2. Change one delay at a time and rerun the same baseline: first replace or
   experimentally shorten Shell3's 10 ms waits, then inspect Tokio wake/park
   polling, then test the network idle soft cap if traced as active. Record
   idle CPU use alongside median/p95 latency so responsiveness does not
   silently become a permanent busy loop.

Before attributing Codex-specific overhead, run the same probe against its
verified idle editable composer. The initial QEMU Shell3 app list does not
include Codex, so this baseline does not establish Codex responsiveness.

## Reproduce the Shell3 baseline

```sh
python3 tools/qemu/measure-ssh-input.py t4ce@127.0.0.1 \
  --port 22294 --known-hosts /tmp/trueos-qemu-known-hosts \
  --output /tmp/trueos-qemu-input-baseline --samples 30
```

For baremetal use port 22, host `t4ce@192.168.178.94`, identity
`/home/t4ce/.ssh/id_ed25519_trueos`, and the existing known-hosts file.
The output directory must be new. The helper tests a newly connected Shell3
prompt; it does not automatically attach to or launch Codex.

The installed `trueos-doc` command was absent from PATH. The repository copy
also rejects the current tree because its root check requires the removed
Shell2 registry. Logging guidance was read directly from its `logs` topic:
area/level filtering precedes capture, so absent markers are not evidence of
absent execution unless their logging policy and capture sink are verified.
