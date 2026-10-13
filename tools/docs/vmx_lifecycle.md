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
native continuations. The BSP aborts abandoned streamed writes before resources
and the slot are released. An executing host-native closure must return or yield
before its storage can be reclaimed; unsafe host stack preemption is not part of
this contract. Hardware storage quarantine also continues to prevent unsafe reuse.

The old snapshot/store/replication experiment is excluded from the default
kernel behind `legacy_vm_checkpoint`. Its ABI shim keeps older Blueprints
loadable, returns unavailable for checkpoint operations, and routes guest pause
to live RAM parking. Kernel replacement requires explicit discard of live VMs;
an aborted replacement resumes the original parked execution.
