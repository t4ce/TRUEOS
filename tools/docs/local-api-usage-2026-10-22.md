# Local Blueprint ABI / std / kernel export usage

Scanned 76 local source roots, 18127 source files, and 60 packages. Errors: 0.

| Export surface | Symbols | App/package reference | SDK/dependency only | No consumer reference | Also no kernel reference |
| --- | ---: | ---: | ---: | ---: | ---: |
| Kernel sys_* | 10 | 2 | 6 | 2 | 1 |
| Native CABI | 356 | 253 | 91 | 12 | 0 |
| Platform / std bridge | 38 | 15 | 5 | 18 | 0 |
| Ring runtime | 121 | 57 | 64 | 0 | 0 |
| Rust runtime | 9 | 9 | 0 | 0 | 0 |
| std / libc / math | 222 | 217 | 5 | 0 | 0 |

SDK/dependency only is unresolved application usage, not an unused finding. A no-consumer row can still have kernel-internal callers (shown below).

| ABI declaration diff | Count | Symbols |
| --- | ---: | --- |
| sdk_only | 45 | `trueos_cabi_browser_asset_ref_push`, `trueos_cabi_browser_asset_refs_begin`, `trueos_cabi_gfx_capture_screenshot_data_url`, `trueos_cabi_uart1_shell_write`, `trueos_cabi_ui3_frame_begin`; full list in JSON |
| kernel_only | 3 | `trueos_cabi_fs_typed_list_dir`, `trueos_cabi_fs_typed_stat`, `trueos_cabi_fs_typed_write_begin` |
| signature_drift | 0 |  |
| declared_without_definition | 0 |  |
| rust_worker_sdk_only | 2 | `trueos_guest_compute_capacity`, `trueos_guest_compute_submit_job` |
| rust_worker_kernel_only | 0 |  |
| rust_worker_signature_drift | 0 |  |

## Exports with no consumer reference

| Surface | Symbol | Kernel-internal reference | Availability |
| --- | --- | --- | --- |
| Kernel sys_* | `sys_halt` | TRUEOS/src/blueprint_shims.rs:19 | registered candidate |
| Kernel sys_* | `sys_sha_compress` | none found | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_close` | TRUEOS/src/std_abi_shim.rs:3981 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_connect_v4` | TRUEOS/src/std_abi_shim.rs:4456 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_connect_v6` | TRUEOS/src/std_abi_shim.rs:4462 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_open` | TRUEOS/src/std_abi_shim.rs:3959 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_peer_v4` | TRUEOS/src/std_abi_shim.rs:5903 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_peer_v6` | TRUEOS/src/std_abi_shim.rs:5915 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_poll_connect` | TRUEOS/src/std_abi_shim.rs:3908 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_recv` | TRUEOS/src/std_abi_shim.rs:4844 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_send` | TRUEOS/src/std_abi_shim.rs:4730 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_set_nonblocking` | TRUEOS/src/std_abi_shim.rs:4071 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_shutdown` | TRUEOS/src/std_abi_shim.rs:4354 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_take_error` | TRUEOS/src/std_abi_shim.rs:4587 | registered candidate |
| Platform / std bridge | `trueos_mio_socket_close` | TRUEOS/src/std_abi_shim.rs:4112 | registered candidate |
| Platform / std bridge | `trueos_mio_socket_local_addr` | TRUEOS/src/std_abi_shim.rs:4221 | registered candidate |
| Platform / std bridge | `trueos_mio_socket_peer_addr` | TRUEOS/src/std_abi_shim.rs:4658 | registered candidate |
| Platform / std bridge | `trueos_mio_socket_take_error` | TRUEOS/src/std_abi_shim.rs:4590 | registered candidate |
| Platform / std bridge | `trueos_mio_tcp_listener_accept` | TRUEOS/src/std_abi_shim.rs:4282 | registered candidate |
| Platform / std bridge | `trueos_mio_tcp_listener_bind` | TRUEOS/src/std_abi_shim.rs:4201 | registered candidate |
| Platform / std bridge | `trueos_mio_tcp_stream_connect` | TRUEOS/src/std_abi_shim.rs:4417 | registered candidate |
| Platform / std bridge | `trueos_mio_tcp_stream_read` | TRUEOS/src/std_abi_shim.rs:4833 | registered candidate |
| Platform / std bridge | `trueos_mio_tcp_stream_write` | TRUEOS/src/std_abi_shim.rs:4722 | registered candidate |
| Platform / std bridge | `trueos_mio_udp_socket_bind` | TRUEOS/src/std_abi_shim.rs:4102 | registered candidate |
| Platform / std bridge | `trueos_mio_udp_socket_connect` | TRUEOS/src/std_abi_shim.rs:4405 | registered candidate |
| Platform / std bridge | `trueos_mio_udp_socket_recv_from` | TRUEOS/src/std_abi_shim.rs:4770 | registered candidate |
| Platform / std bridge | `trueos_mio_udp_socket_send_to` | TRUEOS/src/std_abi_shim.rs:4663 | registered candidate |
| Platform / std bridge | `trueos_platform_cpu_count` | TRUEOS/src/std_abi_shim.rs:7806 | registered candidate |
| Platform / std bridge | `trueos_tokio_platform_wait_after` | TRUEOS/src/std_abi_shim.rs:1357 | registered candidate |
| Platform / std bridge | `trueos_tokio_platform_wait_observe` | TRUEOS/src/std_abi_shim.rs:1353 | registered candidate |
| Platform / std bridge | `trueos_tokio_platform_wake_all` | TRUEOS/src/std_abi_shim.rs:1584 | registered candidate |
| Platform / std bridge | `trueos_tokio_platform_wake_one` | TRUEOS/src/std_abi_shim.rs:1586 | registered candidate |

## Scope

Lexical, conservative current-source census; Rust declaration/definition names and comments excluded. SDK/dependency references count even for dormant wrappers and unrelated features; short libc names can collide. App/package evidence is not proof of execution. No-consumer means no reference in this corpus, not compiler-proven dead code. Tests/probes/build directories are excluded; local Cargo path dependencies and std sources are included, with std PAL restricted to unix. cfg is not evaluated. Resolver candidates include gated/rejected names, marked individually; wildcard Rust runtime families are not finitely enumerated. Reference locations capped at 12 per symbol per source class. No rebuild or runtime test. This is not a complete unused-by-app call graph: dormant wrappers remain unresolved. Compiler-generated imports, token-pasted names, generated-only code, and dependency sources outside Repos (including the Cargo registry) can escape the source census; packages supply positive evidence for their particular builds.

Source roots: `CamFrame`, `Cubes`, `PotatoStamps`, `QuadTexture`, `Startup`, `TRUEOS-Blueprints/apps/Frog`, `TRUEOS-Blueprints/apps/Player`, `TRUEOS-Blueprints/apps/PlotTwist`, `TRUEOS-Blueprints/apps/RbGB`, `TRUEOS-Blueprints/apps/Reticulum-rs`, `TRUEOS-Blueprints/apps/TRUEOS-Blueprints-bios-bundle`, `TRUEOS-Blueprints/apps/badc`, `TRUEOS-Blueprints/apps/bios`, `TRUEOS-Blueprints/apps/calculator`, `TRUEOS-Blueprints/apps/cubesrv`, `TRUEOS-Blueprints/apps/cursive`, `TRUEOS-Blueprints/apps/dobby`, `TRUEOS-Blueprints/apps/esp32_stt`, `TRUEOS-Blueprints/apps/flight_controller_sim`, `TRUEOS-Blueprints/apps/games`, `TRUEOS-Blueprints/apps/gboi`, `TRUEOS-Blueprints/apps/gridpape`, `TRUEOS-Blueprints/apps/hello_world`, `TRUEOS-Blueprints/apps/hello_world_replicatable`, `TRUEOS-Blueprints/apps/image-viewer`, `TRUEOS-Blueprints/apps/ipp-printer`, `TRUEOS-Blueprints/apps/japan`, `TRUEOS-Blueprints/apps/lumen`, `TRUEOS-Blueprints/apps/maxpix`, `TRUEOS-Blueprints/apps/monaco`, `TRUEOS-Blueprints/apps/ocshell`, `TRUEOS-Blueprints/apps/osm`, `TRUEOS-Blueprints/apps/pi_snake`, `TRUEOS-Blueprints/apps/potatomin`, `TRUEOS-Blueprints/apps/prismq`, `TRUEOS-Blueprints/apps/qjs`, `TRUEOS-Blueprints/apps/ratatest`, `TRUEOS-Blueprints/apps/sand`, `TRUEOS-Blueprints/apps/shadertoy`, `TRUEOS-Blueprints/apps/share`, `TRUEOS-Blueprints/apps/skybox`, `TRUEOS-Blueprints/apps/solara`, `TRUEOS-Blueprints/apps/sqlite`, `TRUEOS-Blueprints/apps/superseedr`, `TRUEOS-Blueprints/apps/swarm`, `TRUEOS-Blueprints/apps/tantivy`, `TRUEOS-Blueprints/apps/tboard-0.1.0`, `TRUEOS-Blueprints/apps/tpixel-0.1.0`, `TRUEOS-Blueprints/apps/tredb`, `TRUEOS-Blueprints/apps/webmail`, `TRUEOS-Blueprints/apps/websys`, `TRUEOS-Blueprints/apps/wgpu-hello-compute`, `TRUEOS-Blueprints/buildins/appstore`, `TRUEOS-Blueprints/buildins/backup`, `TRUEOS-Blueprints/buildins/commander`, `TRUEOS-Blueprints/buildins/edit`, `TRUEOS-Blueprints/buildins/hull`, `TRUEOS-Blueprints/buildins/hyper`, `TRUEOS-Blueprints/buildins/img`, `TRUEOS-Blueprints/buildins/os`, `TRUEOS-Blueprints/buildins/ssh`, `TRUEOS-Blueprints/buildins/termdir`, `TRUEOS-Picasso`, `TRUEOS-Picasso-Example`, `TRUEOS-RDP`, `TRUEOS-Rust-Toolchain-nightly-2026-07-10/blueprints/rustc-min`, `Velory`, `Voice`, `WindowDecorations`, `boat`, `codex/codex-rs/cli`, `glutin`, `rebels-in-the-sky`, `ripgrep`, `voxy`, `voxy-asset-sync-check.cwKamX`.

Support roots: 502 local SDK, std and dependency paths (full inventory in JSON).

Available packages: `TRUEOS-Blueprints/dist/Frog.bp`, `TRUEOS-Blueprints/dist/Player.bp`, `TRUEOS-Blueprints/dist/PlotTwist.bp`, `TRUEOS-Blueprints/dist/QuadTexture.bp`, `TRUEOS-Blueprints/dist/appstore.bp`, `TRUEOS-Blueprints/dist/backup.bp`, `TRUEOS-Blueprints/dist/badc.bp`, `TRUEOS-Blueprints/dist/bios.bp`, `TRUEOS-Blueprints/dist/boat.bp`, `TRUEOS-Blueprints/dist/calculator.bp`, `TRUEOS-Blueprints/dist/codex.bp`, `TRUEOS-Blueprints/dist/commander.bp`, `TRUEOS-Blueprints/dist/cubes.bp`, `TRUEOS-Blueprints/dist/cubesrv.bp`, `TRUEOS-Blueprints/dist/dobby.bp`, `TRUEOS-Blueprints/dist/edit.bp`, `TRUEOS-Blueprints/dist/esp32_stt.bp`, `TRUEOS-Blueprints/dist/flight_controller_sim.bp`, `TRUEOS-Blueprints/dist/games.bp`, `TRUEOS-Blueprints/dist/gboi.bp`, `TRUEOS-Blueprints/dist/gridpape.bp`, `TRUEOS-Blueprints/dist/hello_world.bp`, `TRUEOS-Blueprints/dist/hello_world_replicatable.bp`, `TRUEOS-Blueprints/dist/hull.bp`, `TRUEOS-Blueprints/dist/hyper.bp`, `TRUEOS-Blueprints/dist/image-viewer.bp`, `TRUEOS-Blueprints/dist/img.bp`, `TRUEOS-Blueprints/dist/ipp-printer.bp`, `TRUEOS-Blueprints/dist/japan.bp`, `TRUEOS-Blueprints/dist/lumen.bp`, `TRUEOS-Blueprints/dist/maxpix.bp`, `TRUEOS-Blueprints/dist/monaco.bp`, `TRUEOS-Blueprints/dist/os.bp`, `TRUEOS-Blueprints/dist/osm.bp`, `TRUEOS-Blueprints/dist/pi_snake.bp`, `TRUEOS-Blueprints/dist/potato-stamps.bp`, `TRUEOS-Blueprints/dist/potatomin.bp`, `TRUEOS-Blueprints/dist/prismq.bp`, `TRUEOS-Blueprints/dist/qjs.bp`, `TRUEOS-Blueprints/dist/ratatest.bp`, `TRUEOS-Blueprints/dist/rbgb.bp`, `TRUEOS-Blueprints/dist/rebsky.bp`, `TRUEOS-Blueprints/dist/shadertoy.bp`, `TRUEOS-Blueprints/dist/share.bp`, `TRUEOS-Blueprints/dist/skybox.bp`, `TRUEOS-Blueprints/dist/sqlite.bp`, `TRUEOS-Blueprints/dist/ssh.bp`, `TRUEOS-Blueprints/dist/superseedr.bp`, `TRUEOS-Blueprints/dist/swarm.bp`, `TRUEOS-Blueprints/dist/termdir.bp`, `TRUEOS-Blueprints/dist/tredb.bp`, `TRUEOS-Blueprints/dist/trueos-picasso-example.bp`, `TRUEOS-Blueprints/dist/velory.bp`, `TRUEOS-Blueprints/dist/voxy.bp`, `TRUEOS-Blueprints/dist/webmail.bp`, `TRUEOS-Blueprints/dist/websys.bp`, `TRUEOS-Blueprints/dist/wgpu-hello-compute.bp`, `TRUEOS-Blueprints/dist/window-decorations.bp`, `Velory/dist/velory.bp`, `voxy/target/asset-sync/voxy.bp`.

Missing catalog paths: `tinyfighter` (optional=True).

CABI declaration comparison includes SDK `bp_abi.rs` and `qjs_abi.rs`; native Rust `worker_abi.rs` is compared separately. Whitespace and trailing argument commas are ignored. Transitive struct layouts are not compared.

Snapshot: 2026-10-22T09:10:21.582862+00:00 to 2026-10-22T09:10:38.957244+00:00. Package files may be rebuilt independently; their hashes are recorded in JSON. Historical 7z packages count as usage evidence even though the current loader accepts raw ELF and tar/LZ4 only.

Reproduce: `python3 tools/audit/local_api_usage.py --out tools/docs/local-api-usage-2026-10-21`
