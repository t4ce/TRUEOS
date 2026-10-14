# Codex filesystem wait boundaries — 2026-10-14

## Change

Two debugger-observed cooperative scheduling deadlocks are corrected in
`src/std_abi_shim.rs`. No interrupt handlers, AP placement, executor quantum,
polling delay or thread-context bookkeeping changed.

- `open_regular_file_world` releases the process file-registry spinlock before
  reading file bytes or metadata. After those operations, it rechecks the
  registry and uses a world published by a peer, preserving its changes rather
  than overwriting it with a stale candidate. A raced lookup error also rechecks
  for a successfully published world. Truncation of a reused world occurs after
  releasing the registry lock.
- `fsync`, and consequently its `fdatasync` wrapper, clones the open description
  under the descriptor-table lock and releases the lock before writeback.
  The retained description owns the path, shared world and advisory-lock handle
  throughout the operation, even if another caller closes the descriptor.

The existing continuation bridge supplies the handoff: a filesystem call parks
its stack, the executor runs peers, and a completion queues that continuation
to resume. These changes remove the spinlocks spanning those parked waits.

## Evidence that shaped the second change

The original [debugger report](codex-qemu-deadlock-2026-10-14.md) proved the
regular-file registry deadlock. The first patched run kept Codex on AP19,
completed app-server initialization at 1912 ms and reached its authenticated
session prompt at 6496 ms. Its evidence is in
`bld/codex-debugger/registry-fix-20261014-160458/`.

After submitting a small prompt, the debugger identified another spinlock
cycle. `post-submit-debugger.txt` locates the active worker inside `open`,
waiting for `ProcessOpenFiles::lock`, at RIP `0xffffffff804f7bdd`.
`parked-after-submit.txt` identifies the owner on the same AP:

```text
worker id=2147483650, AP19
threads::suspend -> threads::wait
request_broker::submit<write_file_begin>
fs_write_begin_host -> trueos_kernel_sync_fs_write_begin
write_file_to_cabi -> writeback_regular_file_world -> fsync
```

That `fsync` implementation retained its borrowed descriptor-table guard across
writeback. The contender, worker `2147483665`, could not return to let the owner
resume. This is the basis for the additional descriptor snapshot change.

## Verification

`tools/testpy/test_posix_file_world.py` compiles the production registry,
opening, materialization, cleanup, writeback and fsync functions. The filesystem
stub runs a competing operation at controlled read, metadata and write wait
points on the same host thread, without preemption. Held mutexes fail immediately
instead of hanging the test process. This exercises the boundary contract;
it does not emulate hardware interrupts or recursively poll an executor.

All 12 cases pass: independent opens, same-path publication races, preservation
of peer writes, raced errors, writable/read-only truncate behavior, creation,
capacity and cleanup, fsync with a peer open/close, description lifetime,
writeback revision checks and fsync errors. The harness rejects the original
registry-lock implementation; substituting the original fsync also fails at
the descriptor-lock boundary. Logs are saved in `bld/codex-debugger/`.

The existing rename and flock harnesses pass 9 and 8 cases respectively.
The kernel build passes, with existing warning output retained in
`bld/codex-debugger/file-wait-build.log`. `git diff --check` passes.

## Emulator outcome and remaining failure

The run with both corrections is
`bld/codex-debugger/file-wait-fix-20261014-161801/`. Its ISO uses the original
debug image's boot assets with only the freshly built kernel replaced.
`run.json` records the exact source diff, image/kernel hashes and launch
arguments. Codex's package remains the catalog-verified
`9a44ad1580ef62e1c08f90594b750a68137bf2cdebb207730aecff373f2a4d31`.

The VM uses 20 CPUs, 8000 MiB RAM and a temporary 512 MiB RAM filesystem.
Cached auth was verified in that filesystem before the launch. Codex remains
on AP19. The authenticated session prompt became ready at **8372 ms**.
Typing and submission of `Reply exactly TRUEOS_OK. No tools.` reached the
session; the UI later displayed working status and reconnection attempts.

**A complete model reply is not verified.** The response WebSocket reports
`IO error: invalid data`, and the subsequent UI reports a stream disconnected
before completion with `invalid data`. This is an explicit transport error,
not a diagnosis of the network, TLS, WebSocket framing or its underlying ABI.
No transport toggle or speculative transport fix was added.

The AP has further long execution intervals, but later poll/stage counters
advance. The next debugger samples capture
`FreeList::allocation_block_is_live` during `free`, then the allocation
registry during `malloc`, rather than either repaired spinlock. A read-only
walk at the second stop reaches the end of an ordered, non-overlapping free
list with 2007 nodes, 386670144 free bytes and a largest free block of
296059888 bytes. This gives a concrete memory observation; it does not prove
absence of memory bugs or establish allocator work as the entire remaining
delay. Allocator and transport investigations remain separate follow-ups.

`validation.log` is a fixed serial snapshot, `validation-summary.json` records
startup and progress samples, and `evidence-manifest.json` hashes the artifacts.
Debugger captures and the request's terminal output are beside them. No physical
rig deployment or physical-regression result is claimed.

The final VM is paused for inspection, PID 2419689, GDB
`127.0.0.1:12397`, using the pinned `TRUEOS.full.elf`. See
`preserved-state.json`. The intermediate registry-only VM was stopped after
its evidence was saved. The final VM's temporary auth-file cleanup was not
verified; its RAM filesystem and process memory remain local to the paused VM.

## Subsequent physical observation: working timer

The user's working-timer screenshot prompted a read-only capture of
`LatestOfThree.logs`, resolving to `trueos-baremetal.1.log`. The fixed snapshot
is `bld/codex-evidence/20261014-170227/capture.log`; its source digest, startup
markers, progress samples and selected timing/transport events are in
`summary.json`. No rig control or runtime change accompanied this observation.

AP19 advances 87 outer executor passes over the final sampled 50 seconds
(samples 53–63), with four ready tasks at the end. Native stage sequences also
advance. This establishes continued dispatch, unlike the earlier invariant
poll counters during the registry deadlock. It is slow progress: recent
ready-to-poll summaries have means of 326–526 ms and maxima around 2.7 seconds;
individual run-slice maxima reach 2.5–2.7 seconds.

The five-character user turn is recorded as accepted at 16:59:47 Europe/Berlin.
A response-WebSocket connection logged `invalid data` at 16:58:31, before that
submission. The snapshot contains no completed response marker. The earlier
connection error is an active lead, not proof of this particular turn's current
transport state.

Physical startup markers are app-server completion at 37213 ms, thread-start
begin at 40110 ms, thread-start completion at 78468 ms, and session-prompt-ready
at 156419 ms. These intervals remain unattributed; the emulator's 8.4-second
prompt result must not be presented as physical startup performance. A working
timer demonstrates UI refresh and active-turn state, not model-token delivery.
