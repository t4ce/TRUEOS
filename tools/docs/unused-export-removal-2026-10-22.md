# Unused Blueprint export removal — 2026-10-22

Rechecked the 39 names in the 2026-10-21 audit's no-consumer/no-kernel-caller subset against current local source and all 60 available packages. Removed 25 exported interfaces and their implementations, pruned 11 redundant literal Rust runtime aliases, and retained three libc interfaces now imported by `sqlite.bp`.

| Surface | Removed exports | Other disposition |
| --- | ---: | --- |
| Native CABI | 10 | Declarations removed from kernel and Blueprint SDK where present |
| Legacy `sys_*` | 5 | Removed resolver entries and implementations |
| Platform / std bridge | 9 | Removed unused wrappers and their exclusive logging helpers |
| Rust unwind | 1 | Removed `_Unwind_RaiseException` and its unused stub |
| Compiler runtime aliases | 0 | Pruned 11 redundant literal names; hash-independent suffix resolution still supports them |
| libc | 0 | Kept `__memmove_chk`, `__memset_chk`, and `fcntl64`: current SQLite imports all three |

The native CABI removals are `trueos_cabi_async_fs_metadata_start_v1`, `trueos_cabi_write_cstr`, `trueos_cabi_vgpu_call_f32_v1`, `trueos_cabi_socket_tcp_bind_{v4,v6}`, and the five retired `trueos_cabi_shell2_*` stubs. Internal metadata and socket bind implementations remain used by live VMCALL and listener routes. The unused guest sleep helper, formerly called only by the retired Tokio sleep wrapper, was also removed.

The ABI lock was deliberately updated for the ten removals. The exact parser and digest functions from `build.rs` verified the previous lock, confirmed that precisely these ten declarations were removed, and compared all 356 surviving canonical contracts without a signature change.

Retired imports fail normal prebind with a rebuild error. Direct loader paths also reject them before the generic unresolved-import/Joker fallback, so an older package cannot silently bind a retired call to a return-zero stub. This intentionally breaks compatibility for those 25 interfaces.

Validation:

- `cargo build --offline --locked --bin TRUEOS` passed, including final linking.
- Blueprint SDK `cargo check --offline --locked --manifest-path api/Cargo.toml --target x86_64-unknown-linux-gnu` passed.
- 30 tests passed across `test_retired_blueprint_imports.py`, `test_socket_cabi_shutdown.py`, `test_posix_socket_listener.py`, and `test_async_fs_progress.py`.
- The linked kernel and generated CABI resolver contain none of the retired exports. All three retained libc symbols remain defined.
- The post-removal census scanned 60 packages without errors or retired imports. It inventories 756 literal export candidates, down from 792; the difference includes the 11 redundant alias entries.

Full names and retained-import evidence are in `unused-export-removal-2026-10-22.json`. The updated source/package census is `local-api-usage-2026-10-22.{md,json,tsv}`. That census now flags `sys_sha_compress` as having no consumers or kernel callers after removal of `sys_sha_buffer`; it is outside the original removal subset and remains registered.

No rig deployment or runtime boot test was performed.
