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
