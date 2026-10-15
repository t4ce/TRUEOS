# Codex files in boot-local RAM

Codex and codex-check now prepare a private sparse 512 MiB RAM disk before
application entry. The first launch copies the entire existing `apps/codex`
or `apps/codex-check` tree from the current persistent root, including auth,
configuration, sessions, databases, plugin files, caches and temporary files.
There are no auth or session persistence exceptions. The source remains intact.

Logical paths stay unchanged. For example, `apps/codex/.codex/auth.json` is
seeded at that same path in RAM. A source snapshot holds the filesystem's writer
gate while copying; streamed copies use a 64 KiB buffer and retain content type
and record key. Seeding does not inherit the interactive directory listing cap.
A failed copy aborts its write stream, fails launch and leaves the incomplete
RAM subtree unpublished. It never silently starts Codex against the disk.

Sync kfs/std, async std/Tokio, renames and advisory locks select the same RAM
backing store for the private prefix. Codex HOME stays at its existing app root;
CODEX_HOME, TMPDIR and XDG directories are explicitly placed beneath it. Empty
homes receive those directories before Codex tries to canonicalize CODEX_HOME.
The whole app family is seeded once so named/default instance launch order
cannot overwrite another instance's live RAM state.

Shell3 command sessions retain their normal filesystem scope. Other app trees,
workspace paths and the primary disk do not move. The private RAM medium is
excluded from public root enumeration and can never become the primary root.
Explicit physical disk selectors still reach the unchanged seed for inspection.

Codex relaunches in the same boot reuse RAM changes. Reboot discards them and
the next launch seeds from the disk again, including the older saved login and
sessions. No automatic writeback is performed. RAM chunks are allocated on
demand in 64 KiB units; 512 MiB is the shared logical maximum, not an upfront
allocation. The existing append-only TRUEOSFS format still consumes log space
as files are replaced/deleted, so this experiment is bounded by that capacity.

The ready log is:

```
codex-files: ready path=apps/codex source_disc=... ram_disc=... files=... bytes=... capacity=536870912 persistence=boot-only
```

`tools/testpy/test_volatile_codex_files.py` compiles the production seeder and
both sync/async disk selectors with controlled storage. It verifies complete
copying, unchanged source, type/key preservation, failures and retry, boot and
relaunch behavior, listing beyond 1024 children and workspace isolation. Live
hardware validation requires the updated kernel and Codex package.
`tools/testpy/test_private_fs_roots.py` also compiles the production mount,
enumeration and primary-root selection code, verifying that private mounts and
remounts never become the shell disk, including the uncached fallback path.

Local validation completed: 36 engine, 7 mounted removal, 8 mounted move,
3 POSIX binding, 8 RAM seeding/routing and 3 private-root cases pass (65 total).
Both release builds pass; no artifacts were published or deployed:

- Kernel: `tgt/x86_64-unknown-trueos/release/TRUEOS`, 56,065,416 bytes,
  SHA-256 `ea78878187fae91568fae8ef5256d64c06961a82f04de48a98af9997396d3aad`.
- Codex: `TRUEOS-Blueprints/dist/codex.bp`, 66,567,022 bytes,
  SHA-256 `958a76d20cc2ff96f64622b5f43c4514531ee1a26e960cb7ed97cd1eeab8f956`.

The current log confirms the private state lives under `apps/codex`. A read-only
HTTP inspection reset the connection; the updated RAM behavior remains to be
checked on hardware using both new artifacts.
