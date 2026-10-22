# Reviewed removal candidates — 2026-10-22

Fresh census: 76 app roots, 4205 non-vendor app source files, 13172 additional support source files, 60 packages. Wrapper/type and named VMCALL tokens were checked in addition to raw exports. Errors: 0.

Ranked by attributed linked kernel text, with repository-only cleanup last. Ratatui options and other Winit backends are intentionally retained. No implementation was deleted.

| Rank | Candidate | Linked text KiB | Data/BSS KiB | Source KiB | App / package hits |
| ---: | --- | ---: | ---: | ---: | --- |
| 1 | Native scheduled audio engine | 20.0 | 0.2 | 44.9 | 0 / 0 |
| 2 | Virtual gamepad automation station | 11.8 | 30.7 | 22.4 | 0 / 0 |
| 3 | TUN CABI and its private handle table | 4.1 | 0.0 | 6.2 | 0 / 0 |
| 4 | Legacy Konsole row/cursor/end-frame exports | 1.9 | 0.0 | 2.5 | 0 / 0 |
| 5 | Legacy zkVM-shaped sys_* exports | 1.6 | 0.0 | 3.0 | 0 / 0 |
| 6 | Vendored Rayon demo package | 0.0 | 0.0 | 156.2 | 0 / 0 |

## Evidence and removal boundaries

1. **Native scheduled audio engine**: No app wrapper or direct named VMCALL caller found. Engine callers are its CABI and three VMCALL dispatch arms. Remove those together; preserve PCM playback, HDA and the separate synth.

2. **Virtual gamepad automation station**: No app producer found. Kernel references register/spawn the station and clean up owners; those keep it live without an app. Module explicitly says gamepads are not routed into UI4 yet. Preserve physical HID/gamepad support.

3. **TUN CABI and its private handle table**: Only the SDK Tun wrapper and kernel re-export refer to the CABI. Private TUN helpers feed that interface. Preserve TCP sockets and the shared VNet/OpenTun implementation.

4. **Legacy Konsole row/cursor/end-frame exports**: No app or dependency caller beyond trueos-v wrappers found. begin_frame IS imported and must remain. Frame state/helpers are shared with begin_frame and excluded from the estimate; removing these is only a partial interface cleanup.

5. **Legacy zkVM-shaped sys_* exports**: Four are kept looking used by std files selected only for target_os=zkvm. No kernel caller beyond resolver registration. SHA export has no consumer and only copies state; preserve sys_alloc_aligned, sys_rand, sys_read and sys_write.

6. **Vendored Rayon demo package**: Example package absent from the previous 58 resolved app unit graphs; current source is still a workspace member, not a production dependency. Remove the member entry with the directory; retain rayon/rayon-core. Source saving only; no linked-kernel saving claimed.

## False positive caught by wrapper checks

RETAIN: current boat and TRUEOS-Picasso-Example sources use VVideoMem and allocate_vvideo_mem despite no import in their existing packaged artifacts. This is a concrete stale-package false positive.

App references: TRUEOS-Picasso-Example/src/main.rs:1264, TRUEOS-Picasso-Example/src/main.rs:1280, TRUEOS-Picasso-Example/src/main.rs:1324, TRUEOS-Picasso-Example/src/main.rs:25, boat/src/main.rs:1246, boat/src/main.rs:1262, boat/src/main.rs:1306, boat/src/main.rs:25

## Interpretation and limits

Linked sizes are attributed symbol footprints from the existing debug ELF, not measured savings from a removal/relink. They exclude shared callees and inlined VMCALL dispatch arms; generic symbols containing a module/type name can include common machinery. Data/BSS includes reserved runtime memory, which is not equivalent to on-disk file size. Source sizes count selected kernel function bodies/whole modules (including module tests); the Rayon row counts its shipped directory, excluding build caches. They exclude additional SDK declarations/wrappers and most registration/dispatch edits.

Absence from these current sources and existing packages establishes candidates, not compiler-proven unreachability. Packages can lag sources; no fresh full-app builds, runtime trace or whole-program call graph was performed. Numeric VMCALL IDs and computed symbol names can escape named-token searches. The full export records, selected source hashes, per-symbol byte sizes, app-token checks and package evidence are in the JSON.

The Unix/POSIX surface was included in the fresh census. TCP/Mio/wait bridges remain because imported POSIX functions call them. `__errno` and `fstatat64` were not promoted to removal candidates: portability macros/aliases need target-specific checking. Directory-stream rejection entries are not live exports to remove.

Taffy 0.15 is now a direct Hull path dependency and is excluded. Crossfont and the old Hickory versions were already removed; Hickory 0.26.3 is retained. Rayon demo is the remaining small source-only package candidate from the earlier compilation-unit audit.

## Reproduce

```sh
python3 tools/audit/local_api_usage.py --out /tmp/local-api-candidates-current
python3 tools/audit/removal_candidates.py --census /tmp/local-api-candidates-current.json --out tools/docs/removal-candidates-2026-10-22
```
