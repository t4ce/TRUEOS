# VMX execution lifecycle

The normal kernel supports start, stop, pause, resume and kill. Start uses the
existing AppDB/Appstore loader. Shell3's Hypervisor controls select a VM slot;
VME controls address the selected slot. Every control validates that slot's
lease, so a stale selection cannot control a later occupant of its VM ID.

Pause is a RAM execution checkpoint. The same Hull task, VMCS, VPID, EPT, heap,
stack, native continuations and carrier lease remain assigned. Resume continues
that execution without loading an image or changing its identity. A paused VM
remains occupied and cannot be selected for another start.

The filesystem checkpoint is a contract: all owned queued/running filesystem
requests must complete, and every streamed write must finish or abort before
pause can be acknowledged. An open file descriptor alone does not block pause.
A streamed write holds its root write lease between begin and finish/abort.
While this boundary is open, the Hull and its native threads continue executing
so the write's owner can release it. After 15 seconds, an unsuccessful pause is
cancelled and execution stays running. It is never reported as paused.

Important hypervisor records identify the pause request and contract, the first
filesystem blocker (`fs_pending`, `fs_streams`, `async_fs_pending`), the cleared
boundary, and checkpoint timeouts. The pause acknowledgement also records RAM,
carrier and VMCS retention. These records distinguish an incomplete long lived
stream from an execution checkpoint that has already completed.

Sockets remain assigned during pause. TCP receive flow is backpressured before
bytes are consumed, including local loopback; retained stream bytes keep their
order. Incoming Mio UDP datagrams for paused owners are dropped. Resume keeps
socket identities, although a remote peer can still time out a connection.

Stop gives a registered guest two seconds to acknowledge cleanup, then forces
discard. Kill skips guest cleanup and checkpoints. Both wake parked waits and
native continuations. Forced discard also removes matching queued service jobs
without waiting for their AP's service task to run. Jobs retained after failure
to install their allocation realm have explicit host-owned records and are
discarded by the same path. Both paths match VM ID and run generation, skip guest
payload destructors, and release the real admission tokens and lane leases.
An executing service closure remains owned until it returns.

A native std/Tokio worker has a separately owned continuation stack. Exit,
abort, and allocation failure that reach its terminal guest ABI transfer back
to its original carrier rather than spin on the guest stack. The carrier first
restores its address space, current-thread state, WLS, allocation context and
kernel task domain. It then requests forced teardown for that exact VM
generation. This terminal transfer does not unwind guest frames or invoke
their destructors; it is valid only at a terminal ABI without host guards that
would be abandoned.

Forced cancellation retains every abandoned native stack until the Hull has
returned to the host and every remaining native job has either completed or
acknowledged retirement at its host boundary. This collective barrier protects
scoped references into a peer's stack. Host wait futures unregister before their
owner acknowledges retirement, because their queues can also live in peer
stacks. Admission stays counted while stacks wait; when the barrier opens, each
carrier destroys its own stack and releases its reservation. A new generation
cannot open admission until those reservations have retired.

Normal teardown then waits for filesystem requests and BSP rollback of
abandoned streamed writes, releases application resources, and releases the
Hull carrier before publishing the slot offline. Hardware storage quarantine
continues to prevent unsafe reuse. Shutdown diagnostics expose the teardown
stage, native jobs, retiring and retained jobs, and filesystem blockers.

An arbitrary native thread that never returns, yields or reaches the terminal
ABI remains cooperative. Direct service-lane work runs on the shared service
task's stack and cannot use the native continuation escape. Interrupt delivery
does not authorize destroying a running future, abandoning host locks or
reclaiming live backing. There is no asynchronous interrupt escape in this
contract.

Kernel panic restart is also excluded once an AP has published its shared
executor. A currently host-owned task does not prove that suspended guests or
remote submissions are absent. The restart path is restricted to an AP that
has not joined shared scheduling; VM recovery uses the retained carrier and
normal teardown. Panic recursion tracking is per CPU, and the old deliberate
post-restart second panic has been removed.

The [2026-10-23 slot recovery audit](VMX_SLOT_RECOVERY_AUDIT_20261023.md) records
the earlier implementation and captured AP restart stall. Its missing queue
purge and collective-retirement findings are historical; the cooperative limit
for arbitrary running native work still applies.

The [recovery probes](../../../TRUEOS-Blueprints/probes/vm_recovery/README.md)
and [physical-rig runner](../testpy/verify_vmx_recovery_baremetal.py) exercise
parked kill, explicit native panic and allocation failure while an independent
peer remains live. Build the probes locally without publishing:

```sh
cd ../TRUEOS-Blueprints
CARGO_NET_OFFLINE=true TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bpp vm_recovery vm_recovery_peer
```

From TRUEOS, snapshot the normal build-ins and both newly built probes into a
new local staging directory. The helper records package hashes and installs the
peer as `vmpeer.bp`; the short alias avoids Shell3 auto-launching the victim
while the longer name is typed. Its generated Make arguments keep the copied
packages from being rebuilt through the normal application registry:

```sh
python3 tools/testpy/prepare_vmx_recovery_buildins.py \
  --output bld/vmx-recovery-reproduction/blueprints
python3 - <<'PY'
import json
from pathlib import Path
import subprocess
metadata = json.loads(Path('bld/vmx-recovery-reproduction/blueprints/metadata.json').read_text())
subprocess.run(metadata['make_argv'] + [
    'BUILD_MODE=release', 'CARGO_BUILD_FLAGS=--release',
    'PUBLISH_RELEASE_SMB=0', 'START_BAREMETAL_LOG=0',
], check=True)
PY
```

After that image is deployed and booted on the physical rig, select its current
capture file and run:

```sh
python3 tools/testpy/verify_vmx_recovery_baremetal.py --appdb \
  --actions kill panic oom kill \
  --log bld/vmx-recovery-20261023/boot4/LatestOfThree.logs \
  --output bld/vmx-recovery-reproduction/run1
```

This recipe uses the embedded AppDB aliases and requires no external catalog
publication. The runner uses Shell3 over SSH port 22 with real PTYs, checks
generation-specific cleanup and same-slot reuse, and saves transcripts, kernel
records and `summary.json`. Filesystem checks read the committed instance
witness and require a direct HTTP 404 for its abandoned stream. The runner does
not build, deploy or reset the rig. The
[2026-10-23 validation report](VMX_RECOVERY_VALIDATION_20261023.md) records the
successful physical runs and their exact artifact hashes.

The old snapshot/store/replication experiment is excluded from the default
kernel behind `legacy_vm_checkpoint`. Its ABI shim keeps older Blueprints
loadable, returns unavailable for checkpoint operations, and routes guest pause
to live RAM parking. Kernel replacement requires explicit discard of live VMs;
an aborted replacement resumes the original parked execution.
