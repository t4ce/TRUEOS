# Backup builtin

`backup` is its own shipped Blueprint (`backup.bp`). It does not add anything to
OS administration, and restore is an operation inside Backup, not a separate
Shell2 command.

Choose **Back up a disk**, select a whole disk, then choose:

- **Network**: an encrypted TCP service on port 4246 for one client at a time.
- **Another disk**: a full, uncompressed image in `/backups` on a different mounted
  TRUEOSFS root. This choice appears only when the destination has enough free
  log space, including reserved metadata space. Filesystem allocation is checked
  again when writing starts. Two equal-size disks generally cannot hold a full
  image of one another because the destination also needs filesystem space.

Backup waits up to five seconds for complete filesystem transactions and block
I/O to finish. It leaves a busy disk mounted if it cannot claim it. Once claimed,
ordinary handles, partition handles, writers, and mount attempts cannot use it.
The source is flushed and its mount is suspended for the entire image operation.
Progress reports the phase, percentage, and bytes remaining. `backup stop` in the
owning shell slot or Ctrl-C cancels the operation. A read-only backup restores the
original mount and caches automatically when it ends.

The Blueprint performs selection and confirmation. It returns its terminal to
Shell2 before the kernel performs privileged I/O and reports progress. The shell
slot owns the running operation; losing a network client does not end it.

## Network client

Install PyNaCl in a Python environment, then run the exact command displayed by
Backup, for example:

```sh
python3 -m venv .backup-venv
.backup-venv/bin/pip install PyNaCl
.backup-venv/bin/python tools/backup-client.py HOST disk.img --session SESSION_ID
```

Enter the temporary key at its prompt. The client does not save the key or put it
in command-line arguments. Keep `disk.img` and `disk.img.resume.json` together.
The sidecar binds the saved prefix to this backup session, geometry, offset, and
SHA-256. Every received chunk is authenticated, written, flushed to stable
storage, and checkpointed before the next request acknowledges it. After a
router outage the client reconnects and starts at the saved boundary; a partial
chunk is downloaded again. Restarting the client verifies the complete saved
prefix before continuing. Existing untracked image files are never overwritten.

Resume applies only to the same open builtin session. Stopping Backup restores
normal disk access and retires that snapshot and key. A new session requires a
new image. There is no persistent network listener after the operation ends.
An unauthenticated connection times out after 10 seconds; an authenticated stalled
connection after 60 seconds, without releasing the disk snapshot.

After the client acknowledges the full image, the server releases the disk even
if the optional last acknowledgement is lost. If the last response itself is
lost, the client retries finalization three times, then reports a locally complete
image with unconfirmed service teardown; `backup stop` explicitly ends any
remaining session. The image does not need to be downloaded again.

### Wire protocol v1

This is a small dedicated protocol, not HTTP. It uses the existing
XChaCha20-Poly1305 implementation and a fresh random 256-bit key per held snapshot.

The server sends a 56-byte hello: `TBKP0001`, a random 16-byte connection nonce
prefix, a 16-byte session ID, little-endian image size (u64), sector size (u32),
and chunk limit (u32). The entire hello is AEAD associated data. Each request is
an encrypted u64 byte offset plus its 16-byte authentication tag (24 bytes).
Responses are a little-endian u32 ciphertext length followed by authenticated
image bytes. The nonce is the hello's random prefix followed by a little-endian
sequence counter; response nonces set its high bit. Requests and responses share
one sequence number, incremented per response. A fresh hello prevents replay
across reconnects. Offsets must be sector-aligned and cannot skip ahead of data
already served. Buffers and chunk sizes are bounded at 256 KiB.

An offset equal to the image length acknowledges the complete saved image and
receives an authenticated empty response. An optional u64::MAX request confirms
receipt of that response; the service also completes after a two-second grace.

## Local backup and restore

A local backup creates an image and a versioned manifest with disk geometry and
SHA-256. The manifest is published only after the image is complete and flushed.
The Restore picker lists completed image/manifest pairs in mounted TRUEOSFS roots'
`/backups` directories. Incomplete images are not offered for restore.

Select the image, then a different target disk with **exactly the same sector
size and block count**. The confirmation shows both and defaults to Cancel.
Restore replaces every sector, including partition tables and filesystem data;
a preliminary format would simply be overwritten and is not performed.

The selected manifest must still match what was shown in the picker. Restore
holds the disk containing the image steady and verifies its entire SHA-256 before
claiming and writing the target. A busy target is refused. After any attempted
raw write, the old mount caches are discarded and the target is probed afresh.
Cancellation or I/O failure after writing begins can leave an incomplete disk;
there is no rollback. Backup images themselves remain on the other disk.

## Host validation

```sh
python3 tools/test_backup_access.py
python3 tools/test_backup_local.py
.backup-venv/bin/python tools/test_backup_client.py
rustc --edition=2024 --test ../TRUEOS-Blueprints/buildins/backup/src/model.rs -o /tmp/backup-ui-tests
/tmp/backup-ui-tests
cargo check --bin TRUEOS
```

Build the shipped Blueprint locally from TRUEOS-Blueprints with
`TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp backup`. The local harness tests the real orchestration with in-memory I/O and injected
failures; it does not emulate TRUEOSFS layout or drivers. No hardware restore or
network-rig exercise is implied by these host checks.
