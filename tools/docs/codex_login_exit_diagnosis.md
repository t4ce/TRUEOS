# Codex login and exit on TRUEOS, 2026-10-13

Two faults are supported by this run's logs and the deployed Codex entrypoint.
The earlier pending-reentry explanation does not establish the cause of this
run's Ctrl+C exit failure.

Evidence: `bld/baremetal-logs/LatestOfThree.logs`, resolving to
`trueos-baremetal.0.log`, SHA-256
`3919a0f95d5baf3fb2ae290d500c87c1d6b42ac378046165bfdfba5ac00b6b2c`.
Line references below refer to that capture, before the rotating link changes.

## Login listener failure

The callback server binds `127.0.0.1:1455`. At lines 2839 and 2878,
`tiny_http` reports `Error accepting new client: trueos errno 11 (os error 11)`.
Both browser-login attempts then show `Login server error: Login was not
completed`.

The production call chain is:

1. TRUEOS POSIX `accept4` called the nonblocking Mio accept operation once.
   An empty accept queue returned status -2, translated to EAGAIN, even for a
   blocking listener. The PendingListener-to-MioListener transition discarded
   the descriptor's nonblocking flag.
2. `tiny_http` 0.12.0, `src/lib.rs:340`, logs any accept error, queues it to
   its request receiver, and terminates the accept thread.
3. Codex `codex-rs/login/src/server.rs:212` ends its forwarding thread when
   `server.recv()` fails, dropping the async-channel sender.
4. That channel closing produces `Login was not completed` at line 239.
   The app-server adds `Login server error:`.

This failure precedes the OAuth callback and token exchange. Line 2811 also
records an HTTPS 401 response from the ChatGPT plugin endpoint, establishing
that one HTTPS path worked. It does not verify OAuth token exchange or every
TLS path. Lines 2833 and 2873 report that no local browser was found.

Source inspection found a subsequent compatibility gap: `tiny_http` clones
each accepted TCP stream, while TRUEOS socket fcntl rejected duplication.
That path was not reached in the captured failed accepts.

The shim now retains nonblocking flags, waits through the existing descriptor
poll mechanism for blocking accept/read/write, honors nonblocking operations,
and supports native TCP stream clones. Clones share mode changes and retain
the backend until the final descriptor closes. Codex login logic is unchanged.

## Main returned, but the process did not exit

Lines 2917-2927 record terminal mode restoration. Lines 2928-2933 record
`main returned: Ok(())`. Read-only SSH inspection subsequently showed
`1 codex.bp running`, alongside sqlite and share.

The Blueprint packer injected `#![no_main]` and this entrypoint:

```rust
pub extern "C" fn _start() -> ! {
    main();
    loop { core::hint::spin_loop(); }
}
```

The pre-rebuild object placed `_start` at section 12064, offset zero, matching
the actual runtime entry hint at log line 2204. Its final instructions were
`pause` followed by a jump back to `pause`. Thus returning from main never
reached process exit or the guest runner's post-invocation terminal cleanup.

The generated entrypoint now calls the existing Hull `exit(0)` ABI after
`let _ = main()`. This works for std and no_std entrypoint shims and retains
their existing policy of discarding main's return value. The exit ABI releases
the attached terminal before shutting down the VM. It also avoids returning
into image-release code while background pthread carriers may still run.

## Validation and remaining runtime check

- `test_posix_socket_listener.py`: 16 tests passed, covering blocking waits,
  spurious wakes, nonblocking/DONTWAIT behavior, accepted stream modes, clone
  lifetime and shared flags, plus the existing address/listener checks.
- `test_mio_connect_close.py`: 4 tests passed.
- `test_socket_address_families.py`: 4 tests passed.
- Packer `entrypoint_rewrite` tests: 3 passed, including subprocess execution
  proving that unit- and Result-returning main functions actually terminate.
- `cargo check -p TRUEOS`: passed with existing warnings.
- `TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp codex-cli`: packed the updated
  `TRUEOS-Blueprints/dist/codex.bp`; publication was disabled.

The live rig still runs the original kernel and app. Hardware terminal-return
and authenticated OAuth success remain to be tested with both rebuilt pieces.
For a browser on another machine, the localhost callback also needs a route
to the rig; device-code login avoids that callback routing requirement.
Official guidance: <https://learn.chatgpt.com/docs/auth>.

## Follow-up: authenticated startup and apparently missing input

The subsequent capture is `trueos-baremetal.1.log`. After the browser reported
successful sign-in, the UI remained on its provisional loading/composer frame.
The capture records `thread_start_begin` at line 1637, followed by
`thread_start_failed` at line 1668. The fatal diagnostic at line 2929 identifies
the cause: loading `/etc/codex/rules` failed with `FS_ERR_BAD_PARAM`.

Keyboard input was delivered: lines 2918-2920 contain the user's unsent draft.
The missing usable composer was associated with failed session bootstrap.

Two filesystem incompatibilities were found:

- An absent directory in the mounted TRUEOSFS index returned `InvalidParam`.
  The async CABI translated that to -4, while Codex accepts absent optional
  rules directories only when the error is NotFound. The kernel now returns
  `Ok(None)` for the missing directory, which the async CABI translates to its
  existing -8 NotFound status. A file at that path remains an error.
- TRUEOS Rust std's directory reader split async directory results on newline.
  The async API actually returns binary TDL1 records with a 12-byte header,
  typed entries and length-prefixed names. Its header's NUL bytes therefore
  caused even valid empty listings to fail. The packer now installs a strict
  TDL1 decoder into the pinned rust-src before std-based app builds. It checks
  headers, counts, record boundaries, types and names, rejecting truncated
  results while preserving filename bytes. Existing Tokio consumers already
  decoded TDL1; the wire format is unchanged.

The decoder's executable host tests passed for empty/typed directories,
newline/non-UTF8 names, and malformed/truncated records. The kernel check also
passed. The updated Codex package is rebuilt locally without publication.

This new run also validates the previous process-exit repair: lines 2933-2940
record `std-abi: exit`, terminal return and shutdown acknowledgement. Lines
2962-2965 record clean process stop, terminal cleanup and zero native jobs.

## Reboot with saved authentication: fresh thread and stale std cache

The next capture, `trueos-baremetal.2.log`, skips onboarding after account/read.
It still enters the same `SessionSelection::StartFresh` branch and spawns the
same `thread/start` request as the earlier onboarding run. Clearing chat
history does not choose this branch; history persistence is a separate setting.

`thread_start_begin` appears at line 1491. The initial frame is scheduled at
line 1520, but neither thread-start completion nor failure is recorded during
several subsequent minutes. Rules-directory requests now correctly return -8
NotFound. Read-only SSH status confirms Codex remains running. These records
do not establish the exact pending await inside thread creation.

The old directory error still appears at line 774. Inspection established a
build-cache omission in the preceding repair: the packer fingerprints patched
thread/TLS/time/random rust-src, but did not fingerprint `std/src/sys/fs/unix.rs`.
Cargo reused a libstd compiled before the decoder change despite the source
patch being installed. Therefore the prior package rebuild did not actually
include the corrected directory reader.

The std cache fingerprint now includes that filesystem source. A regression
test verifies that changing it changes the cache revision. A new cache shard
was built; inspecting its libstd archive confirms `trueos_directory_names` is
present and the old NUL-error string is absent.

The diagnostic Codex build also logs TRUEOS-only `codex_startup` phase markers
around client request preparation/RPC, server config/metadata/thread creation,
and core rules migration/model loading. It also brackets the persistence/MCP
initialization join, environment readiness, AGENTS/plugin/skill discovery,
required MCP runtime initialization and scheduling of socket prewarm.
Request IDs and existing request spans
correlate the records. These markers change logging only; authentication,
history and startup behavior remain unchanged. A new rig run is needed to
identify the unresolved thread-start await.

The diagnostic package build passed. The module extracted directly from its
compressed payload matches `module.stripped.o` by SHA-256. That packed module
contains the client/server/core phase markers and no old NUL-error string.
Package SHA-256:
`55d6ead5e973c97cec4bad05f2a02c735f77ca094dd9105fc2a8ffd851124f31`.
The current rig already demonstrates the kernel's NotFound correction; the
next diagnostic pass needs this newly compiled Codex package on that kernel.

## Diagnostic run: queued inputs and rules migration

The next run (`trueos-baremetal.0.log`) reaches `thread_start_prepare` and
`thread_start_rpc_begin` at lines 1519–1520, then server config/metadata/create
and `core_rule_migration_begin` at line 1581. No migration completion or
thread-start response appears. TUI redraws record submitted messages under
“Queued follow-up inputs”: keyboard delivery works, but startup is pending.
No additional key is required to start a thread.

The migration uses `NamedTempFile::persist_noclobber`, implemented upstream
with a hard link. TRUEOS std rejects hard links immediately. Dropping the
resulting error also drops the tempfile, flushing its buffered `v1\n` through
the synchronous POSIX close path. At lines 1622–1627 a three-byte temporary
file write begins successfully while plugin staging-directory work overlaps.
Chunk/finish success is not logged, so these logs alone cannot locate the
exact blocked broker request.

Source inspection exposes a definite deadlock in that path: the BSP filesystem
broker awaited requests one at a time, but a streamed write retains its root
write lease between begin and finish/abort. A directory mutation queued between
those calls waits for the lease, preventing the broker from processing the
chunk or finish that releases it. The broker now polls independent request
futures concurrently on its existing BSP task; filesystem futures remain
local and no executor is polled recursively. Arrival notification is registered
before draining the queue.

`tools/testpy/test_fs_request_broker.py` compiles the production service/pump
and verifies that reads, chunks, finish and abort pass a writer waiting for an
open stream's lease, and that releasing the lease completes that writer. Both
tests pass, and `cargo check --bin TRUEOS` succeeds with existing warnings.
This kernel correction still requires hardware validation. The
unsupported migration hard link will continue to produce an upstream warning
after cleanup completes; it is separate from this broker deadlock.

## Next run: conversation writer lock is unsupported

The subsequent `trueos-baremetal.2.log` run completes rules migration at
line 1753 and model loading at line 1900. It reaches and returns from the
persistence/MCP join (1915–1955), then fails at line 1956:
`failed to acquire thread writer coordination lock: lock() not supported`.
The TUI reports `thread_start_failed` at line 1995 after 33.473 seconds.
This is creation of the Codex conversation, not creation of an OS pthread.

Codex's rollout writer coordinator requires std File::lock, try_lock and
close-based release. The pinned std Unix backend excludes TRUEOS from its
flock platform list. The Blueprint overlay now enables all five lock methods
(including shared locks and explicit unlock), and the kernel implements the
flock compatibility import with an actual host-owned advisory-lock registry.
Hull calls cross VMX to that same registry, rather than consulting a private
copy. Independent opens have separate owners; dup/try_clone aliases share an
Arc handle. Its final drop releases ownership, including after a writeback
error, and VM teardown clears that realm's locks. Blocking contention sleeps
five milliseconds between attempts; nonblocking contention returns EAGAIN.
Registry allocations are explicitly host-owned so guest shutdown cannot free
live global state.

Lock identity uses the canonical primary-root/path namespace, including
unpublished buffered creations. This conservatively retains exclusion if a
path is deleted/recreated while locked, rather than promising Unix inode lock
semantics across TRUEOSFS's copy/delete rename compatibility operation.

Eight compiled-production-function lock tests pass, covering cross-realm
contention, shared/exclusive conversion, blocking sleep/retry, Hull dispatch,
duplicate descriptor lifetime, explicit release and realm teardown. Two
Blueprint patch tests and the kernel check pass. Hardware validation still
requires both the newly built kernel and Codex package.

The run also performs extensive plugin bundle extraction/copying. It is a
plausible contributor to responsiveness loss, but these logs do not measure
input-to-render latency or establish a scheduler failure. Logged pthread
creations succeeded; the conversation-store error is independently definite.

The release kernel and Codex package both build successfully. The rebuilt std
archive has `flock` and no `lock() not supported` fallback string. The final
linked Codex module imports `flock`; the module extracted from the compressed
package matches it by SHA-256 (`7ce5d7e3eb82315abf6a4e8e85560f77c6f2572ccb3f5acbee9a016e9fa5b66b`).
The CABI pack guard passes with the same 36 imports and unchanged CABI contract.
Local package: `TRUEOS-Blueprints/dist/codex.bp`, 81,797,305 bytes, SHA-256
`a212c7aa4f10086024211d45ebcaf09182b447824d0e7fd8578eef45acf3e78e`.
Local kernel: `tgt/x86_64-unknown-trueos/release/TRUEOS`.
Kernel SHA-256:
`cb811ca2f198d8f3475a178cce394eb440bc1267167e76ea06d7f4344aa6241f`.
Neither artifact was deployed to the rig during this repair.

## ANSI reaching the Matrix transcript after moving the UI to a carrier

The October 14 capture (`trueos-baremetal.1.log`) starts Codex on a native std
thread and reaches terminal setup at 94 ms. Its mode sequences and full first
frame match the raw ANSI visible in the supplied Matrix transcript. The
stdout route itself is consistent: `blueprint_console_write` forwards to
`shell3::tui::write_stdout`, which appends text lines to the Matrix transcript
until the exact VM/run owns its terminal. Owned output instead reaches the
terminal parser or the byte-preserving SSH output queue.

The implicit claim at `tcsetattr` was restricted to Hull execution, while
stdout already supported native carriers retaining their VM context. The
terminal lease CABI bridge had the same Hull-only restriction. Thus native
terminal setup could succeed without ever claiming the output surface. Its
process-global `TERMINAL_IO_CLAIMED` flag also could not track parking or VM
reuse. Separately, upstream Codex emits EnableBracketedPaste before raw mode,
so that first escape preceded even a successful implicit raw-mode claim.

The kernel lease CABI now dispatches current/release/reentry directly through
the owning VM for native carriers, and retains VMX transport for Hull calls.
The implicit claim rechecks authoritative ownership rather than caching a
global success. Codex's TRUEOS setup explicitly verifies an active lease before
its first ANSI write and returns an error on rejection. Successful initial
claims now log `terminal-lifecycle ... phase=lease-claim state=active epoch=...`.

Five compiled-production CABI tests cover carrier and Hull claims, release,
pending/ready reentry, exact error codes, missing contexts and stale-cache
regressions. All 29 existing Shell3 terminal tests and the kernel check pass.
The baremetal log's raw ANSI mirror in `blueprint_console_text_lines` is a
separate diagnostic copy and remains intentional. Application-level mode
restoration and redraw after parking are not supplied by these startup fixes.

Both release builds pass. The package's CABI check now verifies 37 imports,
including the existing terminal lease-current API, with unchanged kernel
contract. The packed module imports that API and its extracted payload matches
the linked module (`17159eeb4bf185807df9e88c8d7033837a893b1d458311ade9389827dcd32fae`).
Updated package: 67,305,147 bytes, SHA-256
`67d39c2e94e049699000adec5562a6e9a427dfa17685ce89ea1993adef9da620`.
Updated kernel SHA-256:
`80e11d883c7dad0788d912a6b2726aab5648945920a6c02dae5eab8509e34154`.
These local artifacts were not deployed; startup routing still requires a rig
run using both artifacts and checking the new lease-claim marker.

## Early startup delay: stale arg0 cleanup now executes

The next `trueos-baremetal.0.log` run reaches `arg0_cleanup_begin` at 20 ms,
before the main std thread and terminal setup. It keeps advancing through old
`apps/codex/.codex/tmp/arg0/codex-arg0...` directories (list-dir IDs 25, 32,
39 and later), while the cleanup completion marker remains absent. Filesystem
index diagnostics record 81.098- and 77.456-second waits and repeat comparable
waits for subsequent directories. The working `try_lock` implementation now
lets Codex's janitor remove stale directories that the unsupported-lock path
previously skipped.

The expensive removal code resolved every node with `lookup_node_record`,
which reconstructs the live namespace by scanning the entire historical log.
Directory deletion then called `delete_tree`, which repeated that scan. Even
deleting a tiny `.lock` file took this path. After deletion, unconditional index
invalidation forced additional builds and races with subsequent removals.

Mounted removal now warms the index before acquiring the root write lease,
rechecks the indexed record after waiting, reads that physical record directly,
and appends the appropriate node/tree tombstone. It updates the live index in
place, removes descendants using an exact slash-delimited prefix, invalidates
file-cache entries and notifies watchers. A new engine `delete_tree_at_record`
validates the directory reference before writing one durable tombstone; its
contract requires the mounted caller to serialize writes and use its current
logical path, including after a RenameTree.

All 33 engine tests pass, including deletion with a constant read budget after
unrelated log growth and renamed-tree replay. Five compiled-production mounted
removal tests cover retained indexes, exact siblings, failure/corruption,
changed records after lease waits, and rebuilding invalidated indexes outside
the write lease. The kernel check passes. This repair changes the kernel and
filesystem engine only; the existing Codex package does not need rebuilding.

The release kernel build also passes. The new local ELF is
`tgt/x86_64-unknown-trueos/release/TRUEOS`; hardware validation is pending.

The same audit found further history scans in native file and directory moves.
Those mounted paths now use indexed engine helpers and preserve the live index.
Both file and directory moves append a namespace record without copying payloads
or deleting the old physical record. Generic file-only
deletion also rechecks its indexed source after acquiring the lease. See
[TRUEOSFS indexed mutations](trueosfs_indexed_mutations.md) for the platform
contracts, tests and remaining compatibility rename limitations.

For the requested all-state experiment, Codex's private files are now seeded
from their existing disk paths into a private RAM filesystem before app entry.
Login, sessions, caches, temporary files and databases all use that RAM copy;
Shell3 tool/workspace storage keeps its persistent root. See
[Codex volatile files](codex_volatile_files.md) for launch, failure, reboot and
capacity behavior. The disk seed is retained and there is no writeback.

## First model request: WebSocket TLS setup fails before response events

The next physical run (`trueos-baremetal.1.log`) confirms the RAM experiment is
active: line 6052 reports `source_disc=2 ram_disc=3 files=1041 bytes=344742516`.
The prompt reaches `stream_responses_websocket`. Lines 12077/12099 and
13009/13034 record connection attempts to
`wss://chatgpt.com/backend-api/codex/responses`, followed by
`failed to connect to websocket: IO error: invalid data`. The corresponding
telemetry reports `success=false` and roughly four seconds per connection.
The same turn retries five times; no successful WebSocket upgrade or model
response stream is established. The long Working/Reconnecting display does
not demonstrate that the greeting reached model execution.

The port's tokio-rustls adapter discarded the `rustls::Error` returned by
`process_new_packets()` and emitted bare `InvalidData`. Its std/Tokio error
conversions also discarded messages, typed sources and OS codes. The TRUEOS
tokio-tungstenite adapter discarded errors again as `Other: tokio io error`,
including WouldBlock. These adapters now preserve the original I/O error and
TLS source; a failed alert write cannot replace the primary TLS error.

A concrete trust-store mismatch also exists. Codex's explicit WebSocket config
started with an empty store and added native certificates from Unix disk paths.
With no installed CA bundle, this admits no public roots. Reqwest's normal
rustls configuration instead includes bundled WebPKI/Mozilla roots. TRUEOS
WebSocket config now adds that same bundled baseline, then native and custom
roots. Certificate chain and hostname verification stay enabled; other OS
behavior is unchanged. Numeric native/bundled root counts are logged without
certificate contents or authentication data.

The original adapter erased the failure variant, so UnknownIssuer is a likely
cause rather than a captured runtime verdict. HTTP inspection reset its
connection, and a separate read-only SSH session did not complete its help
request; that session was closed. The next package run must confirm the root
counts and either `successfully connected to websocket` or the now-specific TLS
error. No credentials were changed and no model prompt was submitted by the
investigation.

Eight focused Codex custom-CA tests pass, including bundled/custom coexistence,
untrusted-certificate rejection and hostname validation. Seven compiled
production adapter tests pass, covering TLS source preservation, raw OS errors,
WouldBlock/Pending, valid reads and primary-error precedence. The repair changes
the Codex package and vendored transport adapters; it needs no additional
kernel changes. The release package rebuild passes, including the CABI guard,
with publication disabled. The new `TRUEOS-Blueprints/dist/codex.bp` is
66,613,873 bytes, SHA-256
`f23753197a86a9bca799f0be47cca47c629721f40b9517485df17136505d7886`.
Hardware confirmation requires relaunching with that package; no reboot or
additional kernel rebuild is needed for these transport changes.
