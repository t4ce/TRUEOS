# Codex config replacement on TRUEOS, 2026-10-15

The migration preference save fails because POSIX `rename` refuses an existing
destination. The logical path `/apps/codex/.codex/config.toml` is correct and
selects Codex's private RAM TRUEOSFS after normalization. RAM changes the medium;
it does not remove the filesystem contract needed to replace an existing file.

The caller is `PersistModelMigrationPromptAcknowledged` in Codex's TUI. Its
config editor calls `write_atomically`: create a temporary file in the same
directory, write the new document, then `NamedTempFile::persist(config_path)`.
There is no explicit sync or close before persist. Tempfile's overwrite-enabled
Unix path invokes rustix/libc `rename`. TRUEOS's async rename rejected occupied
destinations with `FS_ERR_ALREADY_EXISTS`, translated to `EEXIST`.

The original UI displayed only the outer anyhow context, obscuring that cause.
The updated migration-preference branch displays/logs the complete error chain.
No config values, authentication contents or account identifiers were added to
diagnostics.

## Repair

POSIX rename now requests file replacement through the existing async-FS owner
queue and Hull transport. Wire flag zero retains the native no-replace contract;
flag one requests file replacement. Unknown flags are rejected before enqueue.
Both names retain the existing mount permissions and private RAM selection.

The mounted implementation checks the live source, destination type and parent
under one root write lease. It publishes one existing namespace-rename record,
updates the index/caches and notifies watchers. Payload blocks remain in place:
there is no file-content copy or destination-unlink gap. Directories cannot be
replaced by this new file-replacement operation. Native no-replace callers keep
their existing behavior. No on-disk format or exported C ABI symbol was added.

The std shim also handles the open-file side of atomic saves:

- Flush dirty source worlds before committing the namespace change. Descriptor,
  registry and world locks are released before filesystem waits.
- After success, move source handle/registry bindings to the destination name.
  Subsequent writes, syncs and closes follow that name instead of resurrecting
  the temporary pathname.
- Detach the old destination's open descriptions from its replaced pathname.
  Those handles retain their old bytes and accept memory writes/sync without
  publishing over the replacement.
- Leave bindings unchanged if flush or rename fails.

Handle bookkeeping is process-local. These checks do not establish complete
cross-process inode identity or arbitrary concurrent namespace semantics.
Boot-local persistence remains boot-local: this repair does not add disk
writeback for Codex's private RAM home.

## Verification

The actual `file_replace_contract` Blueprint ran on the new kernel in an isolated
QEMU, using a disposable 512 MiB RAM TRUEOSFS. It passed **5/5 in 142 ms**:

1. Persist a dirty, open tempfile over an existing config; retained source and
   returned file descriptors continue writing to the destination.
2. Retain an old destination descriptor; its reads/writes/sync/close do not
   change the replacement.
3. Fail replacement over a nonempty directory; preserve both source and target.
4. Save again while the first saved file is still open; old handles cannot
   overwrite the second save.
5. Clean up the probe-owned directory.

The probe stopped its watchdog, returned the terminal, and completed clean
teardown with `native_jobs=0`. Its host baseline also passed 5/5.

Focused host checks passed: filesystem engine 37, indexed move/replacement 14,
indexed removal 7, async/Hull routing 10, POSIX rename 4, renameat 5, file-world
semantics 12, metadata 4. Engine coverage includes namespace replay/remount,
metadata identity, failure preservation and a 64 KiB payload move using only
three namespace blocks. The existing Codex config-migration regression passed.

Runtime evidence is in:

```text
TRUEOS-Blueprints/probes/file_replace_contract/validation/qemu-2026-10-15.json
TRUEOS-Blueprints/probes/file_replace_contract/validation/qemu-2026-10-15.log
TRUEOS/bld/file-replace-contract/qemu2/
```

The first QEMU setup was excluded before any probe ran: its Make invocation
compiled release but selected the default debug artifact for packaging. The
corrected image uses `BUILD_MODE=release`, and the native result above pins its
actual runtime hash. A separate selector setup attempt exceeded Shell3's
16-character fastspawn name limit; the successful attempt verified the exact
catalog digest, then used its catalog ID. Neither setup attempt is a test pass.

## Artifacts

Reproduce the kernel image with both artifact and compiler modes selected:

```sh
make iso BUILD_MODE=release CARGO_BUILD_FLAGS=--release \
  PUBLISH_RELEASE_SMB=0 START_BAREMETAL_LOG=0
```

| Artifact | SHA-256 |
| --- | --- |
| Release kernel | `0913a59eaf5a060e880afc8dffe8d6e3b371333161c30451a9fd77321cf972dc` |
| Stripped runtime `bld/TRUEOS.elf` | `56e0424fe99d868ddf906f086ee5eeb27f99686c4da3854b1c209e3bae1cd1d5` |
| `bld/trueos.iso` | `9f5dab022fb1361bfe919a37ce283137c45fd8efb58f940d3ad624075dc7e6c9` |
| Codex package | `37f6eb1a5573bc2dd63a5b6c5530f9423323f2c62ad333e130ece1070659c6ac` |
| File replacement probe | `22fee863c7746dca0f935f4b5d25797b625d8345e063715d4d44188d9a4e9007` |

The Codex and probe packages are published to
`smb://t4ce@pdjb/home-share/TRUEOS_SITE/apps` with SHA-256 filenames. Their
remotely read bytes match the local hashes. The probe remains registered only
in `probes.json`, with a workspace member; it has no Apps registry copy.
The physical rig was not rebooted or changed by the native validation. The
filesystem repair requires booting the new kernel, not just updating Codex.

## The remaining startup delay and tool frontier

The later physical `.2` capture proves automatic `gri` submission eventually
occurs; the preference-save error is nonfatal. Its measured milestones are:

| Boundary | Duration |
| --- | ---: |
| Process to account status complete | 5.628 s |
| Account completion to thread-start marker, including migration confirmation | 86.286 s |
| Thread-start markers | 136.954 s |
| Server's thread-start total within that interval | 117.744 s |
| Server create-thread subphase | 5.365 s |
| Thread-start completion to session-ready marker | 63.321 s |

The config-loading portion inside server startup spans at least 110 seconds of
five-second progress samples. Its largest observed continuation slice is
95.379 seconds; that maximum is a subinterval, not the entire config stage.
The marker/server difference is 19.210 seconds, and pre-dispatch evidence also
contains a long execution slice. Existing records do not assign the config
delay to a particular file operation.

The new startup capture telemetry resolves 51.255 seconds of tool preparation
into MCP readiness (20.661 seconds) and subsequent router construction
(30.590 seconds), with instructions taking 237 ms concurrently. Other phases
and WebSocket preparation overlap. A later capture is 1.165 seconds. These
timers establish enclosures; they do not prove that every enclosed millisecond
was CPU work or network service time. The preference-write repair is not
claimed to remove these startup delays.

The automatic turn reaches Code Mode and reports that TRUEOS cannot spawn its
standalone host. With the selected CodeModeOnly model, that occurs before any
Shell3 command. A supported Code Mode transport remains the next execution
boundary; this file-replacement change does not implement it.
