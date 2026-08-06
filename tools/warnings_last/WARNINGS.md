# Warning baseline

This is the compact inventory captured from `make iso` at the commit in
[`BASE_COMMIT`](BASE_COMMIT). Each row represents one warning cause in one
source file. The count is exact; the message is one representative diagnostic,
kept to one line. Paths are lexically normalized, so compiler spellings with
parent-directory components do not split a source across scopes.

Cargo package-summary diagnostics are excluded. The table contains 1,916
warning records, 235 cause/file groups, and 222 source files; the span-less
C-ABI diagnostics form one additional build-script group.

Post-cleanup verification: `make iso` completed successfully with zero
`warning:` diagnostics. This document intentionally remains the immutable
pre-cleanup inventory; the retained dead-code decisions are recorded in
`DEAD_CODE_EXPECTATIONS.patch` and `dead_code_expectations.json`.

## Scope summary

| Scope | Warnings | Source files | Ownership |
|---|---:|---:|---|
| `kernel` | 1,759 | 186 | Root TRUEOS package under `src/` |
| `workspace` | 65 | 11 | First-party path crates under `crates/` |
| `vendor` | 79 | 25 | Checked-in third-party dependencies under `vendor/` |
| `build-script` | 13 | — | Span-less root build diagnostics |

## Cause summary

| Order | Cause | Warnings | Cause/file groups |
|---:|---|---:|---:|
| 01 | Dead code | 1,851 | 204 |
| 02 | Unexpected cfg | 15 | 7 |
| 03 | Unused imports | 13 | 6 |
| 04 | CABI export mismatch | 13 | 1 |
| 05 | Lifetime syntax | 9 | 4 |
| 06 | Unused variables | 5 | 4 |
| 07 | Unreachable patterns | 3 | 2 |
| 08 | Deprecated API | 2 | 2 |
| 09 | Unused macros | 2 | 2 |
| 10 | Misplaced macro_use | 2 | 2 |
| 11 | Unused mut | 1 | 1 |

## 01. Dead code — 1,851 warnings; 204 cause/file rows

| Scope | Count | Source location | Representative warning |
|---|---:|---|---|
| `kernel` | 115 | `src/intel/gpu_font.rs:21` | constant `MIN_FONT_STAMP_SIZE_PERCENT` is never used |
| `kernel` | 74 | `src/intel/sound/hda.rs:276` | fields `dac_nid` and `device_type` are never read |
| `kernel` | 73 | `src/intel/render/primary.rs:4` | multiple fields are never read |
| `kernel` | 70 | `src/intel/display.rs:59` | constant `PRIMARY_BASELINE_COLOR` is never used |
| `kernel` | 69 | `src/intel/display/display_probes.rs:7` | constant `PRIMARY_REARM_RGB_PLANE_PROBE_ENABLED` is never used |
| `kernel` | 67 | `src/intel/render/constants.rs:1` | constant `FORCEWAKE_RENDER` is never used |
| `kernel` | 59 | `src/aud/m4a_demux.rs:12` | constant `MAX_STSD_ENTRIES` is never used |
| `kernel` | 34 | `src/intel/opencl/types.rs:3` | constant `CL_SUCCESS` is never used |
| `kernel` | 34 | `src/r/stream.rs:13` | struct `HvObjectDesc` is never constructed |
| `kernel` | 28 | `src/aud/m4a.rs:9` | constant `PCM_SAMPLE_RATE_HZ` is never used |
| `kernel` | 28 | `src/intel/display/regs.rs:13` | constant `TRANS_PSR_CTL_A` is never used |
| `kernel` | 28 | `src/intel/gpgpu/rcs/constants.rs:138` | constant `KOKORO_QGEMM_U8_I8_TEXT_OFFSET_BYTES` is never used |
| `kernel` | 28 | `src/net/wifi.rs:12` | variants `WEP`, `WPA3`, and `Unknown` are never constructed |
| `kernel` | 26 | `src/intel/opencl/artifact.rs:8` | multiple variants are never constructed |
| `kernel` | 26 | `src/usb3/hid/leds.rs:11` | constant `LED_VID_JGINYUE` is never used |
| `kernel` | 25 | `src/intel/render/state.rs:71` | fields `gpgpu_arena_phys`, `gpgpu_arena_virt`, and `gpgpu_arena_len` are never read |
| `workspace` | 24 | `crates/trueos-graphics/primitives.rs:11` | variants `Unsupported` and `NotFound` are never constructed |
| `kernel` | 23 | `src/intel/media/h264_cmd.rs:3` | constant `UPSTREAM_INTEL_MEDIA_DRIVER_REPO` is never used |
| `kernel` | 22 | `src/intel/copy/blt.rs:10` | constant `RING_HWS_PGA` is never used |
| `kernel` | 19 | `src/r/rdp.rs:2` | function `client_count` is never used |
| `vendor` | 19 | `vendor/ring-0.17.14/src/arithmetic/limbs/x86_64/mont.rs:34` | function `mul_mont5` is never used |
| `kernel` | 18 | `src/pci/pci.rs:21` | constant `MAX_PCI_CLAIMS` is never used |
| `kernel` | 18 | `src/r/spawn_service.rs:126` | static `STOP_UI_TEXT_INPUT_DEMO` is never used |
| `kernel` | 18 | `src/usb3/dev_gears.rs:7` | constant `USB_CLASS_HID` is never used |
| `kernel` | 17 | `src/aud/mod.rs:98` | function `play_note` is never used |
| `kernel` | 17 | `src/r/keyboard.rs:7` | constant `KEYBOARD_TEXT_BURST_MAX_SCALARS` is never used |
| `kernel` | 16 | `src/intel/gpgpu/kernel_catalog.rs:6` | constant `COPY_RECT_RGBA8_ARTIFACT_FRONTEND` is never used |
| `kernel` | 16 | `src/intel/gpgpu/operations/kokoro_conv1d.rs:53` | enum `KokoroConv1dError` is never used |
| `kernel` | 16 | `src/intel/gpgpu/operations/kokoro_qgemm.rs:9` | struct `KokoroQgemmSpec` is never constructed |
| `kernel` | 16 | `src/intel/mod.rs:51` | constant `GPU_VA_DISPLAY_OVERLAY_BASE` is never used |
| `kernel` | 16 | `src/shell2/mod.rs:64` | constant `SECTION_STATUS_TEXT` is never used |
| `kernel` | 16 | `src/turbo/avx2_fma_sse2_help.rs:2` | function `bf16_to_f32` is never used |
| `kernel` | 15 | `src/intel/media/engine.rs:461` | variant `SubmissionWiring` is never constructed |
| `kernel` | 15 | `src/net/cache_service.rs:13` | constant `MAX_PENDING_REQUESTS` is never used |
| `kernel` | 14 | `src/intel/gpgpu/operations/probes.rs:1` | constant `COPY_RECT_PROBE_CASE_COUNT` is never used |
| `kernel` | 14 | `src/r/resource_monitor.rs:30` | variants `Png` and `Jpeg` are never constructed |
| `workspace` | 13 | `crates/trueos-graphics/font.rs:28` | constant `FONT_TESSEL_SAMPLE_TEXT` is never used |
| `kernel` | 13 | `src/aud/dmg.rs:8` | constant `SAMPLE_RATE_HZ` is never used |
| `kernel` | 13 | `src/intel/gpgpu/operations/lfm25_q8.rs:46` | method `label` is never used |
| `kernel` | 13 | `src/intel/opencl/api.rs:17` | struct `KnownKernelInfo` is never constructed |
| `kernel` | 13 | `src/intel/render/lrc.rs:259` | function `encode_rgb_triangle_store_batch` is never used |
| `kernel` | 13 | `src/net/adapter.rs:42` | variants `V4` and `V6` are never constructed |
| `kernel` | 13 | `src/r/cabi_codes.rs:1` | constant `FS_ERR_BAD_UTF8` is never used |
| `kernel` | 12 | `src/intel/gpgpu/types/surfaces.rs:210` | method `class` is never used |
| `kernel` | 12 | `src/intel/render/resources.rs:460` | function `prepare_triangle_draw_resources` is never used |
| `kernel` | 12 | `src/intel/sound/intel_hda_audio_demo.rs:13` | constant `HDA_WAV_LOOP_RETRY_DELAY_MS` is never used |
| `kernel` | 12 | `src/usb3/hid/mediacontrol.rs:16` | constant `HID_INTERRUPT_TIMEOUT_MS` is never used |
| `kernel` | 11 | `src/r/net/cli/ftp.rs:20` | struct `ParsedFtpUrl` is never constructed |
| `kernel` | 10 | `src/intel/opencl/backend.rs:16` | struct `BackendCaps` is never constructed |
| `kernel` | 10 | `src/r/ttstt_service.rs:224` | field `whisper` is never read |
| `kernel` | 10 | `src/r/ui_surface.rs:34` | method `raw` is never used |
| `kernel` | 10 | `src/shell2/backends/net_tcp.rs:18` | constant `NET_SHELL_RX_CAP` is never used |
| `kernel` | 9 | `src/hv/memory.rs:274` | function `active_guest_hull_rw_backing` is never used |
| `kernel` | 9 | `src/power/mod.rs:12` | constant `IA32_MSR_PLATFORM_INFO` is never used |
| `kernel` | 9 | `src/power/turbo.rs:27` | variant `Disarmed` is never constructed |
| `kernel` | 9 | `src/r/lfm25_hybrid_cpu_backend.rs:82` | field `0` is never read |
| `kernel` | 9 | `src/r/net/cli/pop3.rs:19` | constant `POP3_HOST` is never used |
| `kernel` | 9 | `src/r/net/srv/ssmtp.rs:6` | enum `SSmtpState` is never used |
| `kernel` | 8 | `src/aud/synth.rs:22` | constant `BYTES_PER_SAMPLE` is never used |
| `kernel` | 8 | `src/gpu/vgpu.rs:108` | associated items `render_carrier` and `render_carrier_index` are never used |
| `kernel` | 8 | `src/hv/guest_work.rs:14` | variants `TokioBlocking` and `Worker` are never constructed |
| `kernel` | 8 | `src/hv/mod.rs:493` | associated function `from_peer` is never used |
| `kernel` | 8 | `src/intel/gpgpu/artifacts/uploads.rs:600` | enum `GpgpuArtifactReloadError` is never used |
| `kernel` | 8 | `src/r/codec.rs:63` | variant `SevenZExtractMemory` is never constructed |
| `kernel` | 8 | `src/ui4/screenshot.rs:59` | variant `WindowUnavailable` is never constructed |
| `kernel` | 8 | `src/ui4/window_broker.rs:48` | variant `Kernel` is never constructed |
| `workspace` | 7 | `crates/trueos-shader/generated_triangle.rs:12` | constant `TRIANGLE_PIPELINE_PUSH_COLOR_NOTE` is never used |
| `kernel` | 7 | `src/intel/opencl/queue.rs:8` | enum `CommandKind` is never used |
| `kernel` | 7 | `src/r/cursor.rs:158` | function `mouse_cursor_snapshot` is never used |
| `kernel` | 7 | `src/r/lfm25_decode.rs:16` | constant `HIDDEN_ELEMENTS` is never used |
| `kernel` | 7 | `src/r/lfm25_tokenizer.rs:59` | field `0` is never read |
| `kernel` | 7 | `src/r/net/srv/spop3.rs:6` | enum `SPop3State` is never used |
| `kernel` | 7 | `src/r/net/srv/wss.rs:20` | static `WSS_SEQ` is never used |
| `kernel` | 7 | `src/shell2/matrix.rs:674` | function `begin_slot_running` is never used |
| `kernel` | 7 | `src/surfer/html_shack.rs:124` | field `url` is never read |
| `workspace` | 6 | `crates/trueos-graphics/image.rs:12` | enum `EncodedImageKind` is never used |
| `kernel` | 6 | `src/intel/format.rs:7` | enum `VfConvertedType` is never used |
| `kernel` | 6 | `src/intel/gpgpu/operations/lab256.rs:14` | methods `tag` and `frame` are never used |
| `kernel` | 6 | `src/intel/opencl/registry.rs:22` | fields `artifact`, `upload`, `status`, and `role` are never read |
| `kernel` | 6 | `src/intel/opencl/validation.rs:10` | enum `KnownAotValidationIssueKind` is never used |
| `kernel` | 6 | `src/intel/render/submit.rs:1786` | function `maybe_soft_accept_streamout_submit` is never used |
| `kernel` | 6 | `src/power/thermal.rs:72` | multiple fields are never read |
| `kernel` | 6 | `src/r/net/cli/ws.rs:17` | static `WS_SEQ` is never used |
| `kernel` | 6 | `src/spirit/response_window.rs:111` | struct `ReasoningResponseStream` is never constructed |
| `kernel` | 6 | `src/ui4/mod.rs:275` | methods `application_plane_count` and `supports_application_plane` are never used |
| `kernel` | 5 | `src/aud/pcm_convert.rs:6` | enum `PcmAdapterError` is never used |
| `kernel` | 5 | `src/hv/security.rs:7` | constant `HVSR_0001_VMEXIT_PREDICTOR_ISOLATION` is never used |
| `kernel` | 5 | `src/intel/gpgpu/operations/primitives.rs:40` | function `copy_rect_rgba8_complete` is never used |
| `kernel` | 5 | `src/intel/media/avc_encode_probe.rs:56` | constant `INTRA_ROWSTORE_OFFSET` is never used |
| `kernel` | 5 | `src/power/rapl.rs:149` | method `has_data` is never used |
| `kernel` | 5 | `src/r/font_plan_service.rs:113` | methods `producer` and `candidate_attempts` are never used |
| `kernel` | 5 | `src/r/fs/trueosfs.rs:99` | method `data_lba` is never used |
| `kernel` | 5 | `src/r/gridpaper_service.rs:74` | constant `GRID_WIDTH_MM` is never used |
| `kernel` | 5 | `src/r/lfm25_model.rs:16` | constant `VERIFY_CHUNK_BYTES` is never used |
| `vendor` | 5 | `vendor/ring-0.17.14/src/limb.rs:243` | type alias `Window` is never used |
| `kernel` | 4 | `src/aud/pattern.rs:26` | constant `DEFAULT_BPM` is never used |
| `kernel` | 4 | `src/disc/block.rs:399` | method `as_str` is never used |
| `kernel` | 4 | `src/gpu/vram.rs:70` | method `has_data` is never used |
| `kernel` | 4 | `src/intel/gpgpu/operations/fill_rect_worklist.rs:1` | function `fill_rect_worklist_rgba8_stats` is never used |
| `kernel` | 4 | `src/intel/gpgpu/rcs/kokoro_conv1d.rs:1` | function `kokoro_conv1d_upload_valid` is never used |
| `kernel` | 4 | `src/intel/gpgpu/rcs/kokoro_qgemm.rs:1` | function `kokoro_qgemm_upload_valid` is never used |
| `kernel` | 4 | `src/intel/opencl/memory.rs:5` | struct `BufferObject` is never constructed |
| `kernel` | 4 | `src/intel/render/pipeline.rs:4059` | function `encode_minimal_streamout_proof_batch` is never used |
| `kernel` | 4 | `src/intel/shader.rs:5` | variant `Simd32` is never constructed |
| `kernel` | 4 | `src/r/net/cli/irc.rs:13` | constant `IRC_TLS_PORT` is never used |
| `kernel` | 4 | `src/r/net/https.rs:407` | methods `post_json` and `post_json_bearer` are never used |
| `kernel` | 4 | `src/r/net/socket_cabi.rs:52` | fields `socket_type` and `protocol` are never read |
| `kernel` | 4 | `src/spirit/lilly.rs:134` | field `0` is never read |
| `kernel` | 4 | `src/usb3/descriptor.rs:119` | function `endpoint_transfer_type_label` is never used |
| `kernel` | 4 | `src/usb3/hid/input.rs:116` | fields `modifiers`, `keys`, and `ascii` are never read |
| `vendor` | 4 | `vendor/alsa-0.11.0/src/error.rs:38` | function `from_const` is never used |
| `vendor` | 4 | `vendor/ring-0.17.14/src/arithmetic.rs:36` | constant `MIN_LIMBS` is never used |
| `workspace` | 3 | `crates/trueos-graphics/decoder/png_decode_pool.rs:296` | function `run_parallel` is never used |
| `workspace` | 3 | `crates/trueos-graphics/encoder/png.rs:16` | method `code` is never used |
| `kernel` | 3 | `src/aud/tables.rs:56` | function `note_name_to_midi` is never used |
| `kernel` | 3 | `src/hv/lane.rs:82` | method `as_str` is never used |
| `kernel` | 3 | `src/hv/snapshot.rs:51` | variants `NoRoot`, `BeginWrite`, and `Io` are never constructed |
| `kernel` | 3 | `src/hv/store.rs:48` | variants `Create` and `Format` are never constructed |
| `kernel` | 3 | `src/hv/vmcall.rs:211` | constant `OP_BP_TOKIO_BLOCKING_SPAWN` is never used |
| `kernel` | 3 | `src/intel/gpgpu/artifacts/contract.rs:9` | constant `GPGPU_ADLS_4680_PCI_DEVICE_IDS` is never used |
| `kernel` | 3 | `src/intel/gt_state.rs:30` | constant `GEN12_GT0_PERF_LIMIT_REASONS_MASK` is never used |
| `kernel` | 3 | `src/intel/guc_ctb.rs:33` | constant `GUC_HXG_TYPE_REQUEST` is never used |
| `kernel` | 3 | `src/intel/media/hw_pic.rs:34` | variant `Jpeg` is never constructed |
| `kernel` | 3 | `src/intel/opencl/example.rs:14` | struct `KnownAotQueueProbe` is never constructed |
| `kernel` | 3 | `src/intel/opencl/mod.rs:51` | constant `TRUEOS_OPENCL_PLATFORM_NAME` is never used |
| `kernel` | 3 | `src/intel/render/warmup.rs:414` | function `log_cursor_plane_info` is never used |
| `kernel` | 3 | `src/intel/stats.rs:22` | multiple variants are never constructed |
| `kernel` | 3 | `src/net/r8139.rs:12` | struct `Rtl8139Driver` is never constructed |
| `kernel` | 3 | `src/power/hwp.rs:63` | struct `HwpRequestFields` is never constructed |
| `kernel` | 3 | `src/r/blocking.rs:59` | function `pop_blocking_job` is never used |
| `kernel` | 3 | `src/r/disc/partition.rs:129` | fields `index`, `unique_guid`, and `attributes` are never read |
| `kernel` | 3 | `src/r/font_kernel_service.rs:631` | methods `error` and `plan` are never used |
| `kernel` | 3 | `src/r/ui_cursor.rs:4` | struct `CursorOverlayGlyphSpec` is never constructed |
| `kernel` | 3 | `src/shell2/backends/container.rs:41` | function `container_shell_submit_input` is never used |
| `kernel` | 3 | `src/shell2/cmds/run.rs:1101` | function `enqueue_blueprint_bytes` is never used |
| `kernel` | 3 | `src/std_abi_shim.rs:83` | constant `TRUEOS_ENOMEM` is never used |
| `kernel` | 3 | `src/usb3/hid/midi.rs:53` | struct `PianoNoteSnapshot` is never constructed |
| `vendor` | 3 | `vendor/ring-0.17.14/src/arithmetic/limbs512/storage.rs:30` | struct `AlignedStorage` is never constructed |
| `vendor` | 3 | `vendor/ring-0.17.14/src/arithmetic/montgomery.rs:118` | function `limbs_mul_mont` is never used |
| `workspace` | 2 | `crates/trueos-vm/src/guest.rs:68` | function `hull_text_start` is never used |
| `kernel` | 2 | `src/allcaps.rs:23` | constant `INTEL_GPGPU_ARTIFACT_BOOT_SMOKETESTS` is never used |
| `kernel` | 2 | `src/aud/player.rs:21` | variant `Paused` is never constructed |
| `kernel` | 2 | `src/cpu.rs:12` | constant `AP_HEARTBEAT_TASK_POOL` is never used |
| `kernel` | 2 | `src/gpu/physical.rs:154` | struct `PhysicalBufferSlice` is never constructed |
| `kernel` | 2 | `src/hv/blueprint/blueprint.rs:413` | function `portal_logf` is never used |
| `kernel` | 2 | `src/hv/vmx.rs:159` | constant `VMEXIT_INTERRUPTION_INFO_NMI_UNBLOCKING` is never used |
| `kernel` | 2 | `src/intel/gpgpu/rcs/commands.rs:365` | function `font_rcs_submit_batch` is never used |
| `kernel` | 2 | `src/intel/gpgpu/rcs/context.rs:294` | function `direct_rcs_wait_eq` is never used |
| `kernel` | 2 | `src/intel/gpgpu/rcs/two_d.rs:377` | function `direct_rcs_encode_glyph_mask_2d_batch` is never used |
| `kernel` | 2 | `src/intel/ppgtt.rs:6` | constant `ENTRIES` is never used |
| `kernel` | 2 | `src/intel/uc_fw.rs:82` | function `read_le_u32` is never used |
| `kernel` | 2 | `src/lumen/decode.rs:85` | methods `acknowledge_hardware_state_reset` and `backend_mut` are never used |
| `kernel` | 2 | `src/r/helio_game.rs:512` | field `0` is never read |
| `kernel` | 2 | `src/r/net/cli/ntp.rs:159` | function `ntp_frame_snapshot` is never used |
| `kernel` | 2 | `src/r/net/mail_config.rs:105` | function `save_runtime_config` is never used |
| `kernel` | 2 | `src/ram_probe.rs:13` | fields `page_bytes`, `sample_phys`, and `sample_values` are never read |
| `kernel` | 2 | `src/shell2/interface.rs:17` | associated items `stream`, `for_local_session`, and `local_session_generation` are never used |
| `kernel` | 2 | `src/shell2/shell2_cmd_registry.rs:13` | fields `mode`, `tool_description`, and `tool_parameters_json` are never read |
| `kernel` | 2 | `src/ui4/cursor_frame_inout.rs:311` | method `cursor_icon` is never used |
| `kernel` | 2 | `src/ui4/gpgpu_preview_consumer.rs:98` | variant `Lab256` is never constructed |
| `kernel` | 2 | `src/ui4/winit_input.rs:118` | variant `PixelDelta` is never constructed |
| `vendor` | 2 | `vendor/ring-0.17.14/src/arithmetic/n0.rs:19` | struct `N0` is never constructed |
| `workspace` | 1 | `crates/trueos-graphics/path_mesh.rs:102` | associated function `builder` is never used |
| `workspace` | 1 | `crates/trueos-ttstt-cpu/src/lib.rs:216` | method `validate_dequantized` is never used |
| `kernel` | 1 | `src/intel/gpgpu/operations/helio_retained_transform.rs:193` | method `bytes` is never used |
| `kernel` | 1 | `src/intel/gpgpu/operations/particle_craft.rs:308` | methods `params_phys` and `tile_masks_phys` are never used |
| `kernel` | 1 | `src/intel/gpgpu/operations/spirit_vfx.rs:50` | variant `Failed` is never constructed |
| `kernel` | 1 | `src/intel/gpgpu/operations/submission_2d.rs:543` | function `submit_glyph_mask_2d` is never used |
| `kernel` | 1 | `src/intel/gpgpu/operations/surfaces.rs:195` | function `mandel64_worklist_surface` is never used |
| `kernel` | 1 | `src/intel/gpgpu/rcs/descriptors.rs:19` | function `direct_rcs_write_sprite_quad_worklist_interface_descriptor` is never used |
| `kernel` | 1 | `src/intel/gpgpu/types/kernel.rs:144` | constant `UI4_COMPOSE_FLAG_DEST_XRGB` is never used |
| `kernel` | 1 | `src/intel/guc.rs:68` | constant `GUC_ACTION_HOST2GUC_CONTROL_CTB` is never used |
| `kernel` | 1 | `src/intel/guc_submission.rs:386` | method `contexts` is never used |
| `kernel` | 1 | `src/intel/opencl/value.rs:151` | multiple methods are never used |
| `kernel` | 1 | `src/intel/types.rs:6` | struct `MappedRange` is never constructed |
| `kernel` | 1 | `src/iso9660.rs:158` | function `find_embedded_iso9660_start` is never used |
| `kernel` | 1 | `src/limine.rs:103` | fields `path` and `cmdline` are never read |
| `kernel` | 1 | `src/log_os.rs:459` | function `log_with_target_purpose` is never used |
| `kernel` | 1 | `src/r/disc/detect.rs:63` | function `detect_physical_disk` is never used |
| `kernel` | 1 | `src/r/io/fs_cabi.rs:1395` | function `konsole_write_fmt` is never used |
| `kernel` | 1 | `src/r/mouse_motion_service.rs:50` | variant `KernelApp` is never constructed |
| `kernel` | 1 | `src/r/net/cli/smtp.rs:26` | fields `0` and `1` are never read |
| `kernel` | 1 | `src/r/net/dns.rs:136` | function `resolve_ipv4_for_profile` is never used |
| `kernel` | 1 | `src/r/net/midi_udp.rs:81` | method `handle` is never used |
| `kernel` | 1 | `src/r/net/mod.rs:54` | associated items `new`, `with_nic`, and `nic_index` are never used |
| `kernel` | 1 | `src/r/net/udp.rs:126` | methods `send_v6` and `next_event` are never used |
| `kernel` | 1 | `src/r/print2d.rs:77` | field `owner` is never read |
| `kernel` | 1 | `src/r/spawn_spec.rs:11` | variant `Worker` is never constructed |
| `kernel` | 1 | `src/r/static_map.rs:46` | method `get_or_insert_with` is never used |
| `kernel` | 1 | `src/r/static_slots.rs:26` | method `checked_u8` is never used |
| `kernel` | 1 | `src/shell2/backends/session_pool.rs:15` | constant `TERMINAL_RESET` is never used |
| `kernel` | 1 | `src/shell2/cmds/mod.rs:41` | function `command_registry_json` is never used |
| `kernel` | 1 | `src/shell2/shell2_dl.rs:542` | function `submit_download` is never used |
| `kernel` | 1 | `src/shell2/term_style.rs:89` | method `dim` is never used |
| `kernel` | 1 | `src/tyche.rs:42` | methods `reseed`, `next_u32`, `bool`, and `shuffle` are never used |
| `kernel` | 1 | `src/ui4/blueprint_text.rs:656` | method `same_scene` is never used |
| `kernel` | 1 | `src/ui4/compositor_service.rs:46` | function `ui4_compositor_presented_revision` is never used |
| `kernel` | 1 | `src/ui4/context_menu.rs:234` | function `release_window_registration` is never used |
| `kernel` | 1 | `src/usb3/api.rs:57` | method `endpoint_isochronous_in` is never used |
| `kernel` | 1 | `src/usb3/class.rs:146` | method `short_name` is never used |
| `kernel` | 1 | `src/usb3/hid/mod.rs:527` | function `inject_usb3_mouse_relative_event` is never used |
| `vendor` | 1 | `vendor/ring-0.17.14/src/arithmetic/ffi.rs:55` | function `bn_mul_mont_ffi` is never used |
| `vendor` | 1 | `vendor/ring-0.17.14/src/arithmetic/inout.rs:36` | method `with_non_dangling_non_null_pointers_ra` is never used |
| `vendor` | 1 | `vendor/ring-0.17.14/src/polyfill/slice/as_chunks_mut.rs:44` | methods `as_ptr`, `as_mut`, and `split_at_mut` are never used |

## 02. Unexpected cfg — 15 warnings; 7 cause/file rows

| Scope | Count | Source location | Representative warning |
|---|---:|---|---|
| `vendor` | 5 | `vendor/crossbeam-epoch-0.9.18/src/internal.rs:56` | unexpected `cfg` condition name: `crossbeam_sanitize` |
| `vendor` | 3 | `vendor/crossbeam-epoch-0.9.18/src/atomic.rs:1675` | unexpected `cfg` condition name: `crossbeam_loom` |
| `vendor` | 3 | `vendor/crossbeam-epoch-0.9.18/src/lib.rs:66` | unexpected `cfg` condition name: `crossbeam_loom` |
| `vendor` | 1 | `vendor/crossbeam-epoch-0.9.18/src/collector.rs:112` | unexpected `cfg` condition name: `crossbeam_loom` |
| `vendor` | 1 | `vendor/crossbeam-epoch-0.9.18/src/deferred.rs:90` | unexpected `cfg` condition name: `crossbeam_loom` |
| `vendor` | 1 | `vendor/crossbeam-epoch-0.9.18/src/sync/list.rs:298` | unexpected `cfg` condition name: `crossbeam_loom` |
| `vendor` | 1 | `vendor/crossbeam-epoch-0.9.18/src/sync/queue.rs:217` | unexpected `cfg` condition name: `crossbeam_loom` |

## 03. Unused imports — 13 warnings; 6 cause/file rows

| Scope | Count | Source location | Representative warning |
|---|---:|---|---|
| `kernel` | 5 | `src/ui4/mod.rs:65` | unused import: `ui4_compositor_presented_revision` |
| `workspace` | 3 | `crates/trueos-graphics/image.rs:5` | unused import: `super::decoder` |
| `vendor` | 2 | `vendor/smoltcp-0.13.1/src/iface/interface/mod.rs:32` | unused import: `super::fragmentation::PacketAssemblerSet` |
| `kernel` | 1 | `src/intel/display.rs:43` | unused import: `probe_primary_present_psr` |
| `kernel` | 1 | `src/r/net/ports.rs:2` | unused imports: `GAMESERVER_TACTICS_TCP_PORT` and `MIDI_PIANO_UDP_PORT` |
| `vendor` | 1 | `vendor/ring-0.17.14/src/arithmetic/limbs512/mod.rs:17` | unused imports: `AlignedStorage` and `LIMBS_PER_CHUNK` |

## 04. CABI export mismatch — 13 warnings; 1 cause/file row

| Scope | Count | Source location | Representative warning |
|---|---:|---|---|
| `build-script` | 13 | `(no file)` | TRUEOS@0.0.2: declared CABI symbol trueos_cabi_gfx_texture_dimensions has no kernel export and will stay unresolved |

## 05. Lifetime syntax — 9 warnings; 4 cause/file rows

| Scope | Count | Source location | Representative warning |
|---|---:|---|---|
| `vendor` | 5 | `vendor/ring-0.17.14/src/polyfill/slice/as_chunks_mut.rs:18` | hiding a lifetime that's elided elsewhere is confusing |
| `vendor` | 2 | `vendor/ring-0.17.14/src/pkcs8.rs:56` | hiding a lifetime that's elided elsewhere is confusing |
| `vendor` | 1 | `vendor/ring-0.17.14/src/arithmetic/limbs512/storage.rs:45` | hiding a lifetime that's elided elsewhere is confusing |
| `vendor` | 1 | `vendor/ring-0.17.14/src/polyfill/slice/as_chunks.rs:19` | hiding a lifetime that's elided elsewhere is confusing |

## 06. Unused variables — 5 warnings; 4 cause/file rows

| Scope | Count | Source location | Representative warning |
|---|---:|---|---|
| `kernel` | 2 | `src/shell2/mod.rs:432` | unused variable: `output_mask` |
| `kernel` | 1 | `src/intel/display.rs:5874` | unused variable: `sparse_static_painter` |
| `vendor` | 1 | `vendor/smoltcp-0.13.1/src/iface/interface/ipv4.rs:101` | unused variable: `frag` |
| `vendor` | 1 | `vendor/smoltcp-0.13.1/src/iface/interface/mod.rs:1320` | unused variable: `repr` |

## 07. Unreachable patterns — 3 warnings; 2 cause/file rows

| Scope | Count | Source location | Representative warning |
|---|---:|---|---|
| `vendor` | 2 | `vendor/smoltcp-0.13.1/src/iface/interface/mod.rs:1288` | unreachable pattern |
| `kernel` | 1 | `src/log_os.rs:145` | unreachable pattern |

## 08. Deprecated API — 2 warnings; 2 cause/file rows

| Scope | Count | Source location | Representative warning |
|---|---:|---|---|
| `workspace` | 1 | `crates/trueos-executor/src/raw/mod.rs:501` | use of deprecated method `core::sync::atomic::Atomic::<usize>::fetch_update`: renamed to `try_update` for consistency |
| `vendor` | 1 | `vendor/CrabUSB/usb-host/src/backend/kmod/queue.rs:19` | use of deprecated method `core::sync::atomic::Atomic::<usize>::fetch_update`: renamed to `try_update` for consistency |

## 09. Unused macros — 2 warnings; 2 cause/file rows

| Scope | Count | Source location | Representative warning |
|---|---:|---|---|
| `workspace` | 1 | `crates/trueos-executor/src/lib.rs:15` | unused macro definition: `check_at_most_one` |
| `vendor` | 1 | `vendor/alsa-0.11.0/src/error.rs:30` | unused macro definition: `acheck` |

## 10. Misplaced macro_use — 2 warnings; 2 cause/file rows

| Scope | Count | Source location | Representative warning |
|---|---:|---|---|
| `vendor` | 1 | `vendor/zune-core-0.5.1/src/lib.rs:47` | `#[macro_use]` attribute cannot be used on crates |
| `vendor` | 1 | `vendor/zune-jpeg-0.5.15/src/lib.rs:166` | `#[macro_use]` attribute cannot be used on crates |

## 11. Unused mut — 1 warning; 1 cause/file row

| Scope | Count | Source location | Representative warning |
|---|---:|---|---|
| `vendor` | 1 | `vendor/smoltcp-0.13.1/src/iface/interface/ipv4.rs:103` | variable does not need to be mutable |
