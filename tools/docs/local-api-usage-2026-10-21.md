# Local Blueprint ABI / std / kernel export usage

Scanned 77 local source roots, 18146 source files, and 23 packages. Errors: 0.

| Export surface | Symbols | App/package reference | SDK/dependency only | No consumer reference | Also no kernel reference |
| --- | ---: | ---: | ---: | ---: | ---: |
| Kernel sys_* | 15 | 2 | 6 | 7 | 5 |
| Native CABI | 366 | 199 | 145 | 22 | 10 |
| Platform / std bridge | 47 | 6 | 13 | 28 | 9 |
| Ring runtime | 121 | 57 | 64 | 0 | 0 |
| Rust runtime | 21 | 9 | 0 | 12 | 12 |
| std / libc / math | 222 | 208 | 11 | 3 | 3 |

SDK/dependency only is unresolved application usage, not an unused finding. A no-consumer row can still have kernel-internal callers (shown below).

| ABI declaration diff | Count | Symbols |
| --- | ---: | --- |
| sdk_only | 45 | `trueos_cabi_browser_asset_ref_push`, `trueos_cabi_browser_asset_refs_begin`, `trueos_cabi_gfx_capture_screenshot_data_url`, `trueos_cabi_uart1_shell_write`, `trueos_cabi_ui3_frame_begin`; full list in JSON |
| kernel_only | 8 | `trueos_cabi_fs_typed_list_dir`, `trueos_cabi_fs_typed_stat`, `trueos_cabi_fs_typed_write_begin`, `trueos_cabi_shell2_frontend_attach_v1`, `trueos_cabi_shell2_frontend_detach_v1`; full list in JSON |
| signature_drift | 0 |  |
| declared_without_definition | 0 |  |
| rust_worker_sdk_only | 2 | `trueos_guest_compute_capacity`, `trueos_guest_compute_submit_job` |
| rust_worker_kernel_only | 0 |  |
| rust_worker_signature_drift | 0 |  |

## Exports with no consumer reference

| Surface | Symbol | Kernel-internal reference | Availability |
| --- | --- | --- | --- |
| Kernel sys_* | `sys_cycle_count` | none found | registered candidate |
| Kernel sys_* | `sys_halt` | TRUEOS/src/blueprint_shims.rs:19 | registered candidate |
| Kernel sys_* | `sys_log` | none found | registered candidate |
| Kernel sys_* | `sys_output` | none found | registered candidate |
| Kernel sys_* | `sys_panic` | none found | registered candidate |
| Kernel sys_* | `sys_sha_buffer` | none found | registered candidate |
| Kernel sys_* | `sys_sha_compress` | TRUEOS/src/std_abi_shim.rs:3046 | registered candidate |
| Native CABI | `trueos_cabi_async_fs_metadata_start_v1` | none found | registered candidate |
| Native CABI | `trueos_cabi_shell2_frontend_attach_v1` | none found | registered candidate |
| Native CABI | `trueos_cabi_shell2_frontend_detach_v1` | none found | registered candidate |
| Native CABI | `trueos_cabi_shell2_frontend_read_v1` | none found | registered candidate |
| Native CABI | `trueos_cabi_shell2_frontend_submit_input_v1` | none found | registered candidate |
| Native CABI | `trueos_cabi_shell2_raw_write` | none found | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_bind_v4` | none found | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_bind_v6` | none found | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_close` | TRUEOS/src/std_abi_shim.rs:4032 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_connect_v4` | TRUEOS/src/std_abi_shim.rs:4507 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_connect_v6` | TRUEOS/src/std_abi_shim.rs:4513 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_open` | TRUEOS/src/std_abi_shim.rs:4010 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_peer_v4` | TRUEOS/src/std_abi_shim.rs:5954 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_peer_v6` | TRUEOS/src/std_abi_shim.rs:5966 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_poll_connect` | TRUEOS/src/std_abi_shim.rs:3959 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_recv` | TRUEOS/src/std_abi_shim.rs:4895 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_send` | TRUEOS/src/std_abi_shim.rs:4781 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_set_nonblocking` | TRUEOS/src/std_abi_shim.rs:4122 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_shutdown` | TRUEOS/src/std_abi_shim.rs:4405 | registered candidate |
| Native CABI | `trueos_cabi_socket_tcp_take_error` | TRUEOS/src/std_abi_shim.rs:4638 | registered candidate |
| Native CABI | `trueos_cabi_vgpu_call_f32_v1` | none found | registered candidate |
| Native CABI | `trueos_cabi_write_cstr` | none found | registered candidate |
| Platform / std bridge | `_embassy_time_now` | TRUEOS/src/r/time.rs:308 | registered candidate |
| Platform / std bridge | `trueos_internal_log_write` | none found | registered candidate |
| Platform / std bridge | `trueos_mio_socket_close` | TRUEOS/src/std_abi_shim.rs:4163 | registered candidate |
| Platform / std bridge | `trueos_mio_socket_local_addr` | TRUEOS/src/std_abi_shim.rs:4272 | registered candidate |
| Platform / std bridge | `trueos_mio_socket_peer_addr` | TRUEOS/src/std_abi_shim.rs:4709 | registered candidate |
| Platform / std bridge | `trueos_mio_socket_take_error` | TRUEOS/src/std_abi_shim.rs:4641 | registered candidate |
| Platform / std bridge | `trueos_mio_tcp_listener_accept` | TRUEOS/src/std_abi_shim.rs:4333 | registered candidate |
| Platform / std bridge | `trueos_mio_tcp_listener_bind` | TRUEOS/src/std_abi_shim.rs:4252 | registered candidate |
| Platform / std bridge | `trueos_mio_tcp_stream_connect` | TRUEOS/src/std_abi_shim.rs:4468 | registered candidate |
| Platform / std bridge | `trueos_mio_tcp_stream_read` | TRUEOS/src/std_abi_shim.rs:4884 | registered candidate |
| Platform / std bridge | `trueos_mio_tcp_stream_write` | TRUEOS/src/std_abi_shim.rs:4773 | registered candidate |
| Platform / std bridge | `trueos_mio_udp_socket_bind` | TRUEOS/src/std_abi_shim.rs:4153 | registered candidate |
| Platform / std bridge | `trueos_mio_udp_socket_connect` | TRUEOS/src/std_abi_shim.rs:4456 | registered candidate |
| Platform / std bridge | `trueos_mio_udp_socket_recv_from` | TRUEOS/src/std_abi_shim.rs:4821 | registered candidate |
| Platform / std bridge | `trueos_mio_udp_socket_send_to` | TRUEOS/src/std_abi_shim.rs:4714 | registered candidate |
| Platform / std bridge | `trueos_platform_cpu_count` | TRUEOS/src/std_abi_shim.rs:7857 | registered candidate |
| Platform / std bridge | `trueos_tokio_platform_log` | none found | registered candidate |
| Platform / std bridge | `trueos_tokio_platform_log_semantic_gap` | none found | registered candidate |
| Platform / std bridge | `trueos_tokio_platform_monotonic_nanos` | none found | registered candidate |
| Platform / std bridge | `trueos_tokio_platform_poll_once` | none found | registered candidate |
| Platform / std bridge | `trueos_tokio_platform_sleep_ms` | none found | registered candidate |
| Platform / std bridge | `trueos_tokio_platform_wait` | none found | registered candidate |
| Platform / std bridge | `trueos_tokio_platform_wait_after` | TRUEOS/src/std_abi_shim.rs:1357 | registered candidate |
| Platform / std bridge | `trueos_tokio_platform_wait_observe` | TRUEOS/src/std_abi_shim.rs:1353 | registered candidate |
| Platform / std bridge | `trueos_tokio_platform_wake_all` | TRUEOS/src/std_abi_shim.rs:1584 | registered candidate |
| Platform / std bridge | `trueos_tokio_platform_wake_one` | TRUEOS/src/std_abi_shim.rs:1586 | registered candidate |
| Platform / std bridge | `trueos_tokio_spawn_blocking_job` | none found | registered candidate |
| Platform / std bridge | `trueos_tokio_worker_carriers_enabled` | none found | registered candidate |
| Rust runtime | `_RNvCs2csqI13tepL_7___rustc12___rust_alloc` | none found | registered candidate |
| Rust runtime | `_RNvCs2csqI13tepL_7___rustc14___rust_dealloc` | none found | registered candidate |
| Rust runtime | `_RNvCs2csqI13tepL_7___rustc14___rust_realloc` | none found | registered candidate |
| Rust runtime | `_RNvCs2csqI13tepL_7___rustc19___rust_alloc_zeroed` | none found | registered candidate |
| Rust runtime | `_RNvCs2csqI13tepL_7___rustc26___rust_alloc_error_handler` | none found | registered candidate |
| Rust runtime | `_RNvCs2csqI13tepL_7___rustc35___rust_no_alloc_shim_is_unstable_v2` | none found | registered candidate |
| Rust runtime | `_RNvCs75cmLyI1ip2_7___rustc14___rust_dealloc` | none found | registered candidate |
| Rust runtime | `_RNvCs75cmLyI1ip2_7___rustc14___rust_realloc` | none found | registered candidate |
| Rust runtime | `_RNvCs75cmLyI1ip2_7___rustc19___rust_alloc_zeroed` | none found | registered candidate |
| Rust runtime | `_RNvCs75cmLyI1ip2_7___rustc26___rust_alloc_error_handler` | none found | registered candidate |
| Rust runtime | `_RNvCs75cmLyI1ip2_7___rustc35___rust_no_alloc_shim_is_unstable_v2` | none found | registered candidate |
| Rust runtime | `_Unwind_RaiseException` | none found | registered candidate |
| std / libc / math | `__memmove_chk` | none found | registered candidate |
| std / libc / math | `__memset_chk` | none found | registered candidate |
| std / libc / math | `fcntl64` | none found | registered candidate |

## Scope

Lexical, conservative current-source census; Rust declaration/definition names and comments excluded. SDK/dependency references count even for dormant wrappers and unrelated features; short libc names can collide. App/package evidence is not proof of execution. No-consumer means no reference in this corpus, not compiler-proven dead code. Tests/probes/build directories are excluded; local Cargo path dependencies and std sources are included, with std PAL restricted to unix. cfg is not evaluated. Resolver candidates include gated/rejected names, marked individually; wildcard Rust runtime families are not finitely enumerated. Reference locations capped at 12 per symbol per source class. No rebuild or runtime test. This is not a complete unused-by-app call graph: dormant wrappers remain unresolved. Compiler-generated imports, token-pasted names, generated-only code, and dependency sources outside Repos (including the Cargo registry) can escape the source census; packages supply positive evidence for their particular builds.

Source roots: `CamFrame`, `Cubes`, `PotatoStamps`, `QuadTexture`, `Startup`, `TRUEOS-Blueprints/apps/Frog`, `TRUEOS-Blueprints/apps/Player`, `TRUEOS-Blueprints/apps/PlotTwist`, `TRUEOS-Blueprints/apps/RbGB`, `TRUEOS-Blueprints/apps/Reticulum-rs`, `TRUEOS-Blueprints/apps/TRUEOS-Blueprints-bios-bundle`, `TRUEOS-Blueprints/apps/badc`, `TRUEOS-Blueprints/apps/bios`, `TRUEOS-Blueprints/apps/calculator`, `TRUEOS-Blueprints/apps/cubesrv`, `TRUEOS-Blueprints/apps/cursive`, `TRUEOS-Blueprints/apps/dobby`, `TRUEOS-Blueprints/apps/esp32_stt`, `TRUEOS-Blueprints/apps/flight_controller_sim`, `TRUEOS-Blueprints/apps/games`, `TRUEOS-Blueprints/apps/gboi`, `TRUEOS-Blueprints/apps/gridpape`, `TRUEOS-Blueprints/apps/hello_world`, `TRUEOS-Blueprints/apps/hello_world_replicatable`, `TRUEOS-Blueprints/apps/image-viewer`, `TRUEOS-Blueprints/apps/ipp-printer`, `TRUEOS-Blueprints/apps/japan`, `TRUEOS-Blueprints/apps/lumen`, `TRUEOS-Blueprints/apps/maxpix`, `TRUEOS-Blueprints/apps/monaco`, `TRUEOS-Blueprints/apps/ocshell`, `TRUEOS-Blueprints/apps/osm`, `TRUEOS-Blueprints/apps/pi_snake`, `TRUEOS-Blueprints/apps/potatomin`, `TRUEOS-Blueprints/apps/prismq`, `TRUEOS-Blueprints/apps/qjs`, `TRUEOS-Blueprints/apps/ratatest`, `TRUEOS-Blueprints/apps/sand`, `TRUEOS-Blueprints/apps/shadertoy`, `TRUEOS-Blueprints/apps/share`, `TRUEOS-Blueprints/apps/skybox`, `TRUEOS-Blueprints/apps/solara`, `TRUEOS-Blueprints/apps/sqlite`, `TRUEOS-Blueprints/apps/superseedr`, `TRUEOS-Blueprints/apps/swarm`, `TRUEOS-Blueprints/apps/tantivy`, `TRUEOS-Blueprints/apps/tboard-0.1.0`, `TRUEOS-Blueprints/apps/tpixel-0.1.0`, `TRUEOS-Blueprints/apps/tredb`, `TRUEOS-Blueprints/apps/velosrv`, `TRUEOS-Blueprints/apps/webmail`, `TRUEOS-Blueprints/apps/websys`, `TRUEOS-Blueprints/apps/wgpu-hello-compute`, `TRUEOS-Blueprints/buildins/appstore`, `TRUEOS-Blueprints/buildins/backup`, `TRUEOS-Blueprints/buildins/commander`, `TRUEOS-Blueprints/buildins/edit`, `TRUEOS-Blueprints/buildins/hull`, `TRUEOS-Blueprints/buildins/hyper`, `TRUEOS-Blueprints/buildins/img`, `TRUEOS-Blueprints/buildins/os`, `TRUEOS-Blueprints/buildins/ssh`, `TRUEOS-Blueprints/buildins/termdir`, `TRUEOS-Picasso`, `TRUEOS-Picasso-Example`, `TRUEOS-RDP`, `TRUEOS-Rust-Toolchain-nightly-2026-07-10/blueprints/rustc-min`, `Velory`, `Voice`, `WindowDecorations`, `boat`, `codex/codex-rs/cli`, `glutin`, `rebels-in-the-sky`, `ripgrep`, `voxy`, `voxy-asset-sync-check.cwKamX`.

Support roots: 505 local SDK, std and dependency paths (full inventory in JSON).

Available packages: `TRUEOS-Blueprints/dist/Frog.bp`, `TRUEOS-Blueprints/dist/Player.bp`, `TRUEOS-Blueprints/dist/PlotTwist.bp`, `TRUEOS-Blueprints/dist/QuadTexture.bp`, `TRUEOS-Blueprints/dist/backup.bp`, `TRUEOS-Blueprints/dist/badc.bp`, `TRUEOS-Blueprints/dist/bios.bp`, `TRUEOS-Blueprints/dist/boat.bp`, `TRUEOS-Blueprints/dist/calculator.bp`, `TRUEOS-Blueprints/dist/codex.bp`, `TRUEOS-Blueprints/dist/commander.bp`, `TRUEOS-Blueprints/dist/cubes.bp`, `TRUEOS-Blueprints/dist/cubesrv.bp`, `TRUEOS-Blueprints/dist/dobby.bp`, `TRUEOS-Blueprints/dist/esp32_stt.bp`, `TRUEOS-Blueprints/dist/flight_controller_sim.bp`, `TRUEOS-Blueprints/dist/hull.bp`, `TRUEOS-Blueprints/dist/hyper.bp`, `TRUEOS-Blueprints/dist/img.bp`, `TRUEOS-Blueprints/dist/os.bp`, `TRUEOS-Blueprints/dist/ssh.bp`, `Velory/dist/velory.bp`, `voxy/target/asset-sync/voxy.bp`.

Missing catalog paths: `tinyfighter` (optional=True).

CABI declaration comparison includes SDK `bp_abi.rs` and `qjs_abi.rs`; native Rust `worker_abi.rs` is compared separately. Whitespace and trailing argument commas are ignored. Transitive struct layouts are not compared.

Snapshot: 2026-10-21T12:28:41.187430+00:00 to 2026-10-21T12:28:54.971509+00:00. Package files may be rebuilt independently; their hashes are recorded in JSON. Historical 7z packages count as usage evidence even though the current loader accepts raw ELF and tar/LZ4 only.

Reproduce: `python3 tools/audit/local_api_usage.py --out tools/docs/local-api-usage-2026-10-21`
