# TRUEOS current-source dead-code check — 2026-10-18

Compiler check of the TRUEOS kernel binary using the repository target and default features. `--force-warn dead_code` overrides local `allow`/`expect` suppression for the kernel crate. Dependency crates retain their configured lint levels. No code was removed.

Build success: **True**. Compiler errors: **0**.

| Lint | TRUEOS-owned diagnostics | Including dependency diagnostics |
| --- | ---: | ---: |
| `dead_code` | 3099 | 3104 |
| `unreachable_code` | 1 | 1 |
| `unused_imports` | 18 | 18 |
| `unused_variables` | 26 | 37 |

Diagnostics can contain multiple unused fields/methods. The CSV preserves each primary source location; diagnostic IDs allow accurate deduplication.

## Overview by source area

| Source area | Dead-code diagnostics | Locations | Examples |
| --- | ---: | ---: | --- |
| `src/intel` | 1565 | 1722 | constant `RING_CONTEXT_CONTROL_REF` is never used; constant `CTX_DESC_FORCE_RESTORE` is never used; constant `CTX_DESC_PPGTT_ENABLE` is never used |
| `src/r` | 423 | 456 | function `pop_blocking_job` is never used; function `spawn_vmx_thread_from_raw` is never used; function `spawn_guest_blocking_job_from_raw` is never used |
| `src/aud` | 180 | 180 | struct `AudioVisualizerStatus` is never constructed; function `status` is never used; constant `SAMPLE_RATE_HZ` is never used |
| `src/net` | 136 | 146 | variant `V4` is never constructed; variant `V6` is never constructed; constant `INTERNAL_NETBENCH_MAX_PENDING` is never used |
| `src/spirit` | 108 | 130 | constant `TRANSFORM_CONTROLS` is never used; constant `ALPHA_CUTOFF_CONTROL` is never used; constant `EDGE_FADE_CONTROL` is never used |
| `src/power` | 105 | 105 | constant `HFI_MAX_TABLE_PAGES` is never used; constant `HFI_TIMESTAMP_BYTES` is never used; constant `HFI_STABLE_CAPTURE_ATTEMPTS` is never used |
| `src/hv` | 99 | 111 | field `version` is never read; field `sections` is never read; function `is_joker_import` is never used |
| `src/usb3` | 97 | 104 | method `endpoint_isochronous_in` is never used; method `short_name` is never used; function `endpoint_transfer_type_label` is never used |
| `crates/trueos-graphics` | 92 | 95 | function `run_parallel` is never used; function `job_height` is never used; function `job_placeholder` is never used |
| `src/ui4` | 57 | 63 | method `same_scene` is never used; function `ui4_compositor_presented_revision` is never used; function `release_window_registration` is never used |
| `src/disc` | 40 | 48 | struct `Exclusive` is never constructed; associated function `acquire` is never used; constant `ACQUIRE_WINDOW_MS` is never used |
| `src/ai` | 34 | 34 | constant `HIDDEN_ELEMENTS` is never used; constant `HIDDEN_Q8_BLOCKS` is never used; field `layer` is never read |
| `src/shell3` | 28 | 59 | function `recognizes` is never used; function `release_matrix_target_terminal_handoff` is never used; function `next_executor_for_shell` is never used |
| `src/gpu` | 18 | 18 | struct `PhysicalBufferSlice` is never constructed; variant `Unsupported` is never constructed; variant `InvalidContext` is never constructed |
| `src/turbo` | 17 | 17 | function `bf16_to_f32` is never used; function `f32_to_bf16_bits` is never used; function `bf16_rowmajor_len_bytes` is never used |
| `src/surfer` | 16 | 18 | fields `url` and `html` are never read; variants `Http` and `Https` are never constructed; associated function `new` is never used |
| `crates/trueos-shader` | 15 | 15 | constant `SOURCE_SHA256` is never used; constant `URB` is never used; function `kernel` is never used |
| `src/allocators.rs` | 9 | 9 | method `rebuild_address_fences` is never used; constant `HV_GUEST_HEAP_IMAGE_MAGIC` is never used; constant `HV_GUEST_HEAP_IMAGE_VERSION` is never used |
| `src/live_update.rs` | 9 | 12 | constant `SHELL_NOTICE` is never used; static `WARM_VM_RANGE_CLAIMED` is never used; variant `VmCheckpointStore` is never constructed |
| `src/cpu.rs` | 6 | 6 | constant `AP_HEARTBEAT_TASK_POOL` is never used; associated function `for_lapic_id` is never used; method `core_kind` is never used |
| `src/tga` | 6 | 6 | field `led_reg` is never read; struct `Status` is never constructed; function `tga_led_write` is never used |
| `dependency/generated` | 5 | 5 | constant `ACCESS_FIELD` is never used; constant `CONNECT_FIELD` is never used; constant `EXTENDED_ACCESS_FIELD` is never used |
| `src/limine.rs` | 4 | 4 | method `response` is never used; field `path` is never read; field `cmdline` is never read |
| `src/ram_probe.rs` | 4 | 4 | field `page_bytes` is never read; field `sample_phys` is never read; field `sample_values` is never read |
| `src/tyche.rs` | 4 | 4 | method `reseed` is never used; method `next_u32` is never used; method `bool` is never used |
| `src/allcaps.rs` | 3 | 3 | constant `INTEL_GPGPU_ARTIFACT_BOOT_SMOKETESTS` is never used; constant `SCREENSHOT_CAPTURE_ENABLED` is never used; constant `BOOT_RESIDENT_WARM_ENABLED` is never used |
| `src/lumen` | 3 | 3 | method `acknowledge_hardware_state_reset` is never used; method `backend_mut` is never used; function `open_hybrid_cpu` is never used |
| `src/std_abi_shim.rs` | 3 | 3 | method `load` is never used; constant `TRUEOS_SC_PAGE_SIZE` is never used; function `refresh_reserved_environment` is never used |
| `src/workers.rs` | 3 | 3 | variant `EfficiencyOnly` is never constructed; variant `AnyBackground` is never constructed; function `last_ap_service_worker` is never used |
| `src/chronos` | 2 | 10 | multiple associated items are never used; method `wait` is never used |
| `src/crypt.rs` | 2 | 2 | field `account_tag` is never read; function `ssh_host_seed` is never used |
| `src/pci` | 2 | 2 | function `map_ram_region_at` is never used; function `map_identity_ram_region` is never used |
| `src/wait.rs` | 2 | 3 | methods `register_poll_waker` and `wait_for_event_blocking_parked` are never used; method `poll_take` is never used |
| `src/wls.rs` | 2 | 2 | function `wls_blueprint_thread_slot` is never used; function `with_current_blueprint_thread_id` is never used |
| `src/iso9660.rs` | 1 | 1 | function `find_embedded_iso9660_start` is never used |
| `src/log_os.rs` | 1 | 1 | function `log_with_target_purpose` is never used |
| `src/mio_compat.rs` | 1 | 1 | function `should_log_selector_probe` is never used |
| `src/phys.rs` | 1 | 1 | function `reserve_heap_arena_at` is never used |
| `src/process_environment.rs` | 1 | 1 | method `refresh_reserved` is never used |

## TRUEOS-owned findings by item kind

| Kind | Diagnostics |
| --- | ---: |
| Constants / statics | 630 |
| Fields (check layout before removal) | 266 |
| Functions | 1047 |
| Methods / associated items | 568 |
| Types / variants / other | 588 |

## Interpretation

- **Compiler dead code:** unused in this configured kernel build. Review individual rows before deletion.
- **Unused fields/constants/types:** can represent ABI layout, hardware register maps, debug visibility, or reserved extension points. Check layout and external contracts.
- **Unused private functions/methods:** stronger removal candidates; verify alternate features, tests, and platform configurations.
- **Unused imports/variables and unreachable code:** separate lint categories, not interchangeable with dead implementation code.
- **Externally visible exports and public library APIs:** may escape this lint. Use the separate Blueprint API census for exported surfaces.
- **Excluded modules/features:** not compiled, so absence here is not evidence of use or non-use. This is not an all-features or linker reachability audit.

## Reproduce

```sh
cargo rustc --offline --locked --bin TRUEOS --message-format=json -- --force-warn dead_code > /tmp/trueos-deadcode.jsonl 2> /tmp/trueos-deadcode.stderr
python3 tools/audit/deadcode_report.py /tmp/trueos-deadcode.jsonl --out /tmp/trueos-deadcode
```

Full findings are in the adjacent CSV and JSON files. They include file, line, source text, package, and compiler message.
