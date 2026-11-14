# TRUEOS kernel services tasks and pools

[Overview](runtime-inventory.md) · [Task declarations](runtime-task-index.md)

Updated after the 2026-11-02 runtime cleanup. No boot or runtime trace collected. Paths below are repository-relative. **Boot-wired** means a source path exists from `_start`/`kmain`; **conditional** adds a feature, readiness, hardware, topology, or policy gate; **on-demand** means an API/event must submit work; **disabled** means an explicit startup switch prevents the registry from invoking it. Allocation and execution can still fail. These states do not prove a service is currently running.

## Boot and polling trace

- `_start` ([src/main.rs:131](../../src/main.rs#L131)) calls `kmain` . BSP creates its executor  and registers its worker spawner . Cold boot launches APs with `cpu::ap_start` ; warm boot calls `live_update::release_warm_aps` .
- `cpu::ap_start` ([src/cpu.rs:305](../../src/cpu.rs#L305)) initializes per-CPU state and executor; `enter_ap_runtime`  publishes the spawner, marks online, and enters `runtime::run_ap_forever` ([src/runtime.rs:53](../../src/runtime.rs#L53)). Worker slots >1 set `BACKGROUND_AP_WORKER_READY` ([src/cpu.rs:240](../../src/cpu.rs#L240)).
- BSP `spawn_bsp_services` ([src/main.rs:298](../../src/main.rs#L298)) starts the optional task profiler and the central spawn service. `live_update::spawn_post_boot` is invoked separately . BSP `_loop` polls time, `runtime::poll_local_executor`, and pending interrupts . APs poll time/executor and use a local deadline plus `sti; hlt` when idle ([src/runtime.rs:53](../../src/runtime.rs#L53)). `poll_local_executor` has a per-CPU reentrancy guard .
- Central `spawn_service_task` ([src/r/services/spawn_service.rs:1413](../../src/r/services/spawn_service.rs#L1413)) repeatedly scans `TASKS`, tests `disabled`, extra gate, and readiness mask; atomically sets `started`; calls the wrapper; resets `started` on `Skipped`/`Failed`. Retry delay is 25/150/250 ms depending on results. A normal task return does **not** clear this bit automatically.
- Readiness flags are monotonic ([src/r/readiness.rs:123](../../src/r/readiness.rs#L123)), with up to 64 concurrent watch receivers . A set flag records a milestone, not ongoing health. `APP_VM_READY` is derived from NET + root mounted + index ready .

`mermaid
flowchart TD
    Start["_start → kmain"] --> BSP["BSP per-CPU executor"]
    Start --> AP["boot APs → cpu::ap_start"]
    AP --> Register["register worker spawner → AP polling loop"]
    BSP --> Registry["spawn_service_task → TASKS"]
    Registry --> Gates["feature + disabled + readiness + gate"]
    Gates --> Local["BSP tasks"]
    Gates --> UI["AP1 UI tasks; BSP fallback"]
    Gates --> Workers["background AP workers / pools"]
    Requests["guest ABI / shell / UI requests"] --> Demand["on-demand tasks, threads, compute jobs"]
    Register --> Workers
    Register --> Demand
`

## Placement and pool organization

| Executor/pool | Capacity and owner | Admission and static use |
|---|---|---|
| Per-CPU raw executors | One initialized executor per BSP/AP ([src/percpu.rs:97](../../src/percpu.rs#L97)); worker table maximum 256 slots ([src/workers.rs:18](../../src/workers.rs#L18), [src/allcaps.rs:118](../../src/allcaps.rs#L118)) | BSP and AP loops poll them. `WorkerSpawner::spawn` sends + wakes remote AP ([src/workers.rs:123](../../src/workers.rs#L123)). |
| UI/service carrier | Normally AP1; BSP fallback after all topology spawners registered ([src/workers.rs:284](../../src/workers.rs#L284)) | `spawn_on_ui_core` ([src/r/services/spawn_service.rs:207](../../src/r/services/spawn_service.rs#L207)), UI/input/Spirit services. Cooperative services can run on single CPU. |
| General background carriers | Slots >=2; LastAP reservation excluded ([src/workers.rs:306](../../src/workers.rs#L306)) | Round-robin selection; selectors prefer P-cores where applicable . No fallback to BSP for blocking/compute. `BACKGROUND_AP_WORKER_READY` alone does not prove an eligible general carrier exists. |
| Codex application placement | LastAP reserved while `CODEX_EXECUTOR_EXPERIMENT_ENABLED=true` ([src/allcaps.rs:126](../../src/allcaps.rs#L126), [src/workers.rs:293](../../src/workers.rs#L293)) | `r::threads::submit` recognizes Codex archives and chooses role-aware carriers ([src/r/threads.rs:558](../../src/r/threads.rs#L558), [src/workers/application_policy.rs:16](../../src/workers/application_policy.rs#L16)). On demand, not an independent boot task. |
| Native thread continuations | `thread_task` and `codex_application_task` each have 256 task frames, but share a **global 256 thread admission limit** ([src/r/threads.rs:553](../../src/r/threads.rs#L553)) | `std_abi_shim` pthread submission ([src/std_abi_shim.rs:7603](../../src/std_abi_shim.rs#L7603)) → `r::threads::spawn` → `submit` → appropriate executor task. Captures VM ownership, stack, WLS identity; guest VMCall path also supported . |
| Blocking service lanes | Task storage maximum 256 ([src/r/blocking.rs:31](../../src/r/blocking.rs#L31)), one persistent lane task per eligible registered slot | BSP dispatcher calls `start_service_lanes` every 250 ms. Host/guest submit APIs enqueue bounded requests under lane leases. Shared queue cap 4094 . Work runs synchronously inside AP poll; this pool is distinct from native thread continuations. |
| CPU compute task storage | Shared maximum **32** task frames ([src/allcaps.rs:55](../../src/allcaps.rs#L55), [src/cpu_task_pool.rs:272](../../src/cpu_task_pool.rs#L272)) | `CpuTaskPool::try_dispatch` reserves pool capacity and an exclusive compute lease, then spawns a finite closure task. `run` retries admissible transient failures and awaits completion. Exclusive only among compute clients; normal executor work still shares the carrier ([src/workers.rs:400](../../src/workers.rs#L400)). |
| `CODEC_COMPUTE` | Static cap 4, `PerformanceFirst` ([src/r/codec.rs:13](../../src/r/codec.rs#L13)) | Used for archive pack/unpack and LZ4 encode/decode ([src/r/lz4.rs:251](../../src/r/lz4.rs#L251)). CPU work has explicit callers; it is not dormant. |
| Codec request workers | Task pool 4 ([src/r/codec.rs:12](../../src/r/codec.rs#L12)); request queue 32, operation table 64 | Registry `codec-service` → `spawn_codec_service` ([src/r/services/spawn_service.rs:332](../../src/r/services/spawn_service.rs#L332)), waits for topology registration, starts up to four background workers. Workers dequeue archive/LZ4 requests. |
| VMedia image/video | Image task pool 4 ([src/r/services/media_service.rs:645](../../src/r/services/media_service.rs#L645)); video task pool 3 ([src/r/services/video_service.rs:425](../../src/r/services/video_service.rs#L425)) | Registry `vmedia-service` → wrapper ([src/r/services/spawn_service.rs:378](../../src/r/services/spawn_service.rs#L378)): explicitly starts three video futures even on one available carrier, then up to four image workers. Actual decode/playback waits for queued requests. |
| Hull image workers | Task pool 2 ([src/r/services/hull_image_service.rs:396](../../src/r/services/hull_image_service.rs#L396)) | Lazy `ensure_workers` from `request` ; UI4 Hull image ABI calls it ([src/ui4/blueprint_ui4/hull_image_api.rs:160](../../src/ui4/blueprint_ui4/hull_image_api.rs#L160)). Boot registry does not start this pool. |
| Short-lived launch/playback | Archive launch pool 4 ([src/r/io/fs_cabi.rs:2622](../../src/r/io/fs_cabi.rs#L2622)), image-viewer launch pool 4 ([src/r/services/image_open_service.rs:33](../../src/r/services/image_open_service.rs#L33)), video playback pool 3 ([src/r/services/video_open_service.rs:45](../../src/r/services/video_open_service.rs#L45)) | Guest FS/launch/video requests pick background worker, create task, and return admission result. See task table. |
| Spirit | Four reserved fence channels, runtime worker limit 1 ([src/spirit/mod.rs:105](../../src/spirit/mod.rs#L105)); four distinct task types each with pool size 1 | Registry `trueos-spirit` → frame + cursor pair, then selection + thoughts ([src/r/services/spawn_service.rs:597](../../src/r/services/spawn_service.rs#L597)). Requires usable hardware, complete scanout, and UI carrier. Unactivated fence channels are spare capacity, not four running workers. |
| Readiness subscription pool | 64 watch receiver slots ([src/r/readiness.rs:98](../../src/r/readiness.rs#L98)) | Supporting synchronization storage; not an executor or worker pool. |
| Chronos subscriptions | Dynamic weak-reference subscriber list ([src/chronos/signals.rs:87](../../src/chronos/signals.rs#L87)) | One boot `service_task` publishes each 100 ms; Shell3 minute subscription exists ([src/shell3/shell3.rs:792](../../src/shell3/shell3.rs#L792)). Dropped subscriptions are pruned. |

## Extra gates and hidden startup conditions

Registry gate implementations are in [src/r/services/spawn_service.rs](../../src/r/services/spawn_service.rs) unless specified:

| Gate | Static condition |
|---|---|
| `codex_progress_gate` | `CODEX_PROGRESS_PROBES_ENABLED=true` ([src/allcaps.rs:130](../../src/allcaps.rs#L130)). |
| `shell3_pool_gate` | All topology spawners registered, plus background carrier or UI service carrier. |
| `microfont_warm_gate` | Virtio output dimensions, or GuC + warm Intel renderer. |
| `intel_device_gate` | Intel device claimed. |
| `gpu_fault_containment_gate` | GuC submission ready and physical GPU present. |
| `trueos_spirit_gate` | Spirit hardware ready, complete scanout route, UI carrier. |
| `ui4_compositor_gate` | Intel device, active scanout dimensions, ready RGBA8 plane stack, UI carrier. |
| `ui4_input_gate` | Compositor gate, or Virtio output plus UI carrier. |
| `ui_core_ready_gate` | UI carrier exists. |
| `ui4_slot4_service_gate` | Compositor gate and MicroFont does not own the interaction overlay plane. |
| `intel_media_engine_gate` / H.264 | Media decode engine exists; H.264 task still feature gated. |
| `fallback_logo_ui_gate` | Fallback logo available. |
| `http_trueosfs_gate` | Root mounted + FS index ready. |
| `html_shack_gate` | Cold PXE boot enabled, or mounted root. |
| `ttstt_cpu_service_gate` | Root mounted + index ready; boot warm is false by configuration ([src/allcaps.rs:96](../../src/allcaps.rs#L96)). |
| `user_input_writer_gate` | Mounted root. |
| `net-poll-tasks` / `net-service` wrappers | At least one NIC; per-device task storage is bounded separately. |
| `codec-service` / `vmedia-service` wrappers | Full topology registration and available background spawners. |
| `bp-autostart` ([src/r/restart.rs:125](../../src/r/restart.rs#L125)) | After registry BG + VTHREAD bits, cold path additionally awaits mounted root ; launches only manifest entries with `action=launch`. Warm boot restore feature separately controls behavior. |

## Remaining disabled and dormant startup paths

The retired services are recorded in [the cleanup decisions](runtime-retirement.md). Two registry entries remain explicitly disabled: Intel HDA audio demo and RAPL service. TTSTT is opt-in and its residency warm policy remains false; no command-driven lazy-start caller was found. Legacy VM checkpoint/store remains opt-in. These are source admission states, not liveness measurements.

## Runtime verification surfaces already present

- `latest_system_service_snapshot_text` ([src/r/services/spawn_service.rs:1403](../../src/r/services/spawn_service.rs#L1403)) returns service names, enabled/gate/started state, required and missing readiness masks, kind, and readiness text; refreshed every second. It is exported by [src/r/net/vlayer.rs:662](../../src/r/net/vlayer.rs#L662) and consumed by Shell3 captured output ([src/shell3/captured.rs:258](../../src/shell3/captured.rs#L258)).
- Snapshot `started=1` proves wrapper admission success only. It is not a heartbeat, completion marker, or full-pool count. Partial multi-task wrapper success is possible. `spawn_net_poll_tasks` returns `Spawned` once a NIC exists even if individual task allocations failed ([src/r/services/spawn_service.rs:450](../../src/r/services/spawn_service.rs#L450)).
- Optional BSP profiler ([src/executor_task_profile.rs:414](../../src/executor_task_profile.rs#L414)) is on by default feature + config and reports spawned/ready counts, per-task poll work, slow polls, drops and readiness. The profiler covers **BSP** execution, not every AP/task.
- CPU pool `snapshot` includes submitted/completed/active/rejection counts ([src/cpu_task_pool.rs:168](../../src/cpu_task_pool.rs#L168)). Worker spawners expose spawned/ready counts ([src/workers.rs:134](../../src/workers.rs#L134)), blocking lanes expose activity ([src/r/blocking.rs:520](../../src/r/blocking.rs#L520)), and SMP HLT history is consumed by Shell3 + thermal diagnostics.

## Scope and limits

Audited task annotations, spawning and pool APIs under `src/r/**`, [src/workers.rs](../../src/workers.rs), `src/workers/**`, `src/spirit/**`, `src/chronos/**`, plus [src/main.rs](../../src/main.rs), [src/Chronos.rs](../../src/Chronos.rs), [src/cpu.rs](../../src/cpu.rs), [src/cpu_task_pool.rs](../../src/cpu_task_pool.rs), [src/runtime.rs](../../src/runtime.rs), [src/percpu.rs](../../src/percpu.rs), [src/smp.rs](../../src/smp.rs), [src/executor_cache.rs](../../src/executor_cache.rs), [src/executor_task_profile.rs](../../src/executor_task_profile.rs), [src/allcaps.rs](../../src/allcaps.rs), [Cargo.toml](../../Cargo.toml), [.cargo/config.toml](../../.cargo/config.toml). Cross-checked caller references throughout repository-owned `src`/`crates` (vendor omitted). There is no `src/kvm*`; [.cargo/config.toml:12](../../.cargo/config.toml#L12) mentions a `tools/kvm/run.sh` alias target, but that directory is absent in the checkout. Other task/service domains are inventoried separately. No type-aware whole-program call graph or execution proof; function pointers, exported C/VMCall ABI entrypoints, conditional compilation, hardware availability, and externally loaded Blueprint requests require cautious classification. No task in this kernel annotation set was found with zero source path to its spawn/admission point. Some paths are explicitly disabled or excluded in the default feature set.

## Service authorities with no independent executor task

A module named service need not have a task; these are request/callback authorities used synchronously or as child futures:

| Authority | Static consumers and purpose | Status |
|---|---|---|
| Codex input routing | [src/r/services/codex_input_service.rs:214](../../src/r/services/codex_input_service.rs#L214); UI4 prompt imports it ([src/ui4/codex_prompt.rs:24](../../src/ui4/codex_prompt.rs#L24)), async FS vFile reads dispatch to it ([src/r/io/async_fs_cabi.rs:526](../../src/r/io/async_fs_cabi.rs#L526)) | On-demand control/read API. No detached task. |
| IDE workspace service | `read_for_owner`/`write_for_owner` ([src/r/services/ide_workspace_service.rs:492](../../src/r/services/ide_workspace_service.rs#L492)); async FS vFile route ([src/r/io/async_fs_cabi.rs:497](../../src/r/io/async_fs_cabi.rs#L497)); VM cleanup ([src/hv/mod.rs:1535](../../src/hv/mod.rs#L1535)) | Active API wiring, invoked for matching IDE paths. |
| IDE source service | Async `write_for_owner` ([src/r/services/ide_source_service.rs:58](../../src/r/services/ide_source_service.rs#L58)), `read_for_owner` ; async FS vFile route ([src/r/io/async_fs_cabi.rs:496](../../src/r/io/async_fs_cabi.rs#L496)); VM cleanup ([src/hv/mod.rs:1536](../../src/hv/mod.rs#L1536)) | On-demand child future under async FS task. |
| Hull font service/cache | `prepare_blocking` ([src/r/services/hull_font_service.rs:230](../../src/r/services/hull_font_service.rs#L230)) called by UI4 Hull text ABI ([src/ui4/blueprint_ui4/hull_text_api.rs:230](../../src/ui4/blueprint_ui4/hull_text_api.rs#L230)) | On-demand glyph/font cache work. `std::thread::spawn` at `hull_font_service.rs:923` belongs to `#[cfg(test)]`, not a kernel worker pool. |
| MicroFont mask service | UI text masks used in link/drag/slot4 ([src/ui4/link_service.rs](../../src/ui4/link_service.rs), [src/ui4/drag_drop.rs](../../src/ui4/drag_drop.rs), [src/ui4/slot4_service.rs](../../src/ui4/slot4_service.rs)) | Shared initialization/cache API, no detached task. |
| VCopy service | Queue/poll/fill functions ([src/r/services/vcpy_service.rs:5](../../src/r/services/vcpy_service.rs#L5)); Intel BCS queue calls ([src/intel/display/ui4_bcs_queue.rs:33](../../src/intel/display/ui4_bcs_queue.rs#L33)); UI frame fill ([src/ui4/frame_pool.rs:959](../../src/ui4/frame_pool.rs#L959)); Shell3 diagnostics ([src/shell3/system/vcpy.rs:12](../../src/shell3/system/vcpy.rs#L12)) | GPU submission/completion API driven by callers, no independent executor task. |
| File-watch callbacks | [src/r/fs/file_watch.rs](../../src/r/fs/file_watch.rs); startup watcher subscribes `changed` in [src/r/startup_config.rs:65](../../src/r/startup_config.rs#L65); `changed` marks configuration dirty | Callback authority; independent watcher task and FS mutation notifications provide execution. |
| Hull image policy and MicroFont overlap helpers | [src/r/services/hull_image_policy.rs](../../src/r/services/hull_image_policy.rs), [src/r/services/microfont_line_overlap.rs](../../src/r/services/microfont_line_overlap.rs) | Pure support/policy modules; not additional workers. |
| HTTP/HTTPS fetch and JSON POST operation broker | [src/r/net/https.rs:1889](../../src/r/net/https.rs#L1889) host admission; exported C APIs; spawn wrappers, call `wait::spawn_local_detached`. HTTP uses Surfer's fetch/POST worker pool ; HTTPS lazily registers TLS queues and awaits the existing TLS socket service. Results/read/discard/wait APIs track operation IDs. | On demand, queued child futures under the single BSP job runner; no separate executor task per HTTP operation. Shared direct async clients (`get_bytes_shared `, media GET, protobuf POST ) are child futures driven by their callers. |
| SMTP/mail operation broker | [src/r/net/smtp_cabi.rs:171](../../src/r/net/smtp_cabi.rs#L171) start → `spawn_send_text ` → `spawn_local_detached ` → `send_text_mail ` → `SmtpClient`; result, discard, wait . Runtime mail account config loads/saves through [src/r/net/mail_config.rs:94](../../src/r/net/mail_config.rs#L94). | On-demand child future under BSP job runner; mail client ABI is connected despite no task annotation. |
| Printing queue and IPP client | VMCall submission → [src/r/print2d.rs](../../src/r/print2d.rs); boot `printer_spooler_task` ([src/r/net/printer.rs](../../src/r/net/printer.rs)) dequeues complete jobs and calls `run_print_job` → `IppClient` ([src/r/net/ipp_print.rs](../../src/r/net/ipp_print.rs)). | **Producer wired**: Paper, Edit and img compose DIN A4 pages through the shared print SDK. Bounded owner-scoped uploads enter the queue only after complete PWG validation (300 dpi, A4 sRGB). The existing IPP client reports accepted/printing/completed/failed/uncertain outcomes, waits for TCP credit during large sends, and requests no print scaling. Physical calibration remains a hardware test. |
| DNS and socket control authorities | DNS [src/r/net/dns.rs:129](../../src/r/net/dns.rs#L129)// are async client futures; synchronous guest DNS goes through `dns_request_broker` task. [src/r/net/socket_cabi.rs:479](../../src/r/net/socket_cabi.rs#L479) onwards stores socket/TUN handles and sends commands to VNet/net adapter. | On-demand APIs driven by existing networking/broker tasks and client futures; no extra task per socket/DNS request. |

Host `std::thread` usages in [src/r/threads/tests.rs](../../src/r/threads/tests.rs), [src/r/blocking/thread_metrics.rs](../../src/r/blocking/thread_metrics.rs), and [src/r/io/async_fs_progress.rs](../../src/r/io/async_fs_progress.rs) are host test code, not extra kernel tasks.
## Every concrete executor task declaration in this scope

45 concrete annotations, including imported `#[task]` aliases. Task-frame capacity is per declared task type; actual admitted/running count depends on topology and callers. `spawn_service.rs:N` in this table refers to `src/r/services/spawn_service.rs:N`.

| Declaration | Task-frame pool | Brief source trace / use |
|---|---:|---|
| [src/r/services/hull_download_service.rs:134](../../src/r/services/hull_download_service.rs#L134) `launch_worker` | 16 | On-demand download authority: async filesystem writes to `vFile:hulldownloads/submit` or `/received` → `write_for_owner` → queue admission → background spawner. The launcher starts a trauma worker with generation/owner tracking; it is separate from the boot registry. |
| [src/wait.rs:772](../../src/wait.rs#L772) `job_runner_task` | 1 | Boot `job-runner` → wrapper `spawn_service.rs:273`; drains local futures and a legacy Send queue with no producer found. Shell/admin continuations enter through `spawn_local_detached` ([src/wait.rs:817](../../src/wait.rs#L817)); these queued futures share this runner rather than adding task-storage declarations. |
| [src/chronos/signals.rs:87](../../src/chronos/signals.rs#L87) `service_task` | 1 | Boot `clock-signals` → wrapper `spawn_service.rs:524`; 100 ms publication, Shell3 minute consumer `shell3/shell3.rs:752`. |
| [src/cpu_task_pool.rs:272](../../src/cpu_task_pool.rs#L272) `cpu_task_pool_worker` | 32 | On-demand `CpuTaskPool::try_dispatch` [src/cpu_task_pool.rs:67](../../src/cpu_task_pool.rs#L67) → spawn ; finite admitted compute closure. |
| [src/executor_task_profile.rs:414](../../src/executor_task_profile.rs#L414) `bsp_task_profile_reporter_task` | 1 | Conditional boot `spawn_bsp_services` [src/main.rs:298](../../src/main.rs#L298) → task + spawn ; default feature/config enabled. |
| [src/r/blocking.rs:342](../../src/r/blocking.rs#L342) `service_lane_executor_task` | 256 | Boot/on-demand `start_service_lane_for_slot` [src/r/blocking.rs:414](../../src/r/blocking.rs#L414) → spawn ; per-slot queue loop. |
| [src/r/blocking.rs:400](../../src/r/blocking.rs#L400) `blocking_job_dispatcher_task` | 1 | Boot `blocking-service-lanes` → wrapper `spawn_service.rs:289`; supervise/start service lanes every 250 ms. |
| [src/r/codec.rs:1054](../../src/r/codec.rs#L1054) `codec_worker_task` | 4 | Boot `codec-service` → wrapper `spawn_service.rs:332`; dequeue requests ([src/r/codec.rs:1067](../../src/r/codec.rs#L1067)). |
| [src/r/fs/http_trueosfs.rs:964](../../src/r/fs/http_trueosfs.rs#L964) `http_trueosfs_task` | 1 | Conditional boot `http-trueosfs` → wrapper `spawn_service.rs:566`; NET and root/index gates. |
| [src/r/fs/request_broker.rs:1093](../../src/r/fs/request_broker.rs#L1093) `service_task` | 1 | Boot `trueosfs-request-broker` → wrapper `spawn_service.rs:281`; BSP handles synchronous FS bridge requests. |
| [src/r/fs/trueosfs.rs:278](../../src/r/fs/trueosfs.rs#L278) `mount_service_task` | 1 | Boot `trueosfs-mount-service` → wrapper `spawn_service.rs:434`; request visible-root mounting. |
| [src/r/fs/trueosfs.rs:327](../../src/r/fs/trueosfs.rs#L327) `index_service_task` | 1 | Boot `trueosfs-index-service` → wrapper `spawn_service.rs:438`; persistent indexing loop. |
| [src/r/io/async_fs_cabi.rs:1470](../../src/r/io/async_fs_cabi.rs#L1470) `service_task` | 1 | Boot `blueprint-async-fs-service` → wrapper `spawn_service.rs:277`; polls submitted async guest FS requests. |
| [src/r/io/fs_cabi.rs:2622](../../src/r/io/fs_cabi.rs#L2622) `launch_archive_task` | 4 | On-demand guest launch script API [src/r/io/fs_cabi.rs:2585](../../src/r/io/fs_cabi.rs#L2585) → `launch_script_request` → spawn . |
| [src/r/net/cli/ntp.rs:214](../../src/r/net/cli/ntp.rs#L214) `ntp_sync_task` | 1 | Conditional boot `ntp-sync` → wrapper `spawn_service.rs:528`; NET configured. |
| [src/r/net/cli/ws_time.rs:339](../../src/r/net/cli/ws_time.rs#L339) `ws_time_task` | 1 | Conditional boot `ws-time` → wrapper `spawn_service.rs:570`; NET configured; connection retry loop. |
| [src/r/net/discovery.rs:30](../../src/r/net/discovery.rs#L30) `lan_discovery_task` | 1 | Conditional boot `lan-discovery` → wrapper `spawn_service.rs:574`; NET configured. |
| [src/r/net/dns_request_broker.rs:135](../../src/r/net/dns_request_broker.rs#L135) `service_task` | 1 | Boot `dns-request-broker` → wrapper `spawn_service.rs:285`; BSP polls networking for parked synchronous callers. |
| [src/r/net/midi_udp.rs:381](../../src/r/net/midi_udp.rs#L381) `midi_piano_udp_task` | 1 | Conditional boot `midi-piano-udp` → wrapper `spawn_service.rs:586`; NET configured. |
| [src/r/net/printer.rs:238](../../src/r/net/printer.rs#L238) `printer_discovery_task` | 1 | Conditional boot `printer-discovery` → wrapper `spawn_service.rs:578`; network readiness. |
| [src/r/net/printer.rs:420](../../src/r/net/printer.rs#L420) `printer_spooler_task` | 1 | Conditional boot `printer-spooler` → wrapper `spawn_service.rs:582`; queued print jobs. |
| [src/r/net/srv/sntp.rs:111](../../src/r/net/srv/sntp.rs#L111) `sntp_service_task` | 1 | Conditional boot `sntp-service` → wrapper `spawn_service.rs:532`; IPv4 configured. |
| [src/r/restart.rs:124](../../src/r/restart.rs#L124) `restart_apps_task` | 1 | Conditional boot `bp-autostart` → wrapper `spawn_service.rs:920`; cold manifest launches, warm behavior feature dependent. |
| [src/r/services/codex_progress_service.rs:9](../../src/r/services/codex_progress_service.rs#L9) `codex_progress_task` | 1 | Conditional boot `codex-progress-observer` → wrapper `spawn_service.rs:553`; progress flag true, needs background readiness. |
| [src/r/services/gamepad_control_service.rs:451](../../src/r/services/gamepad_control_service.rs#L451) `gamepad_control_service_task` | 1 | Conditional boot `gamepad-control-service` → UI wrapper `spawn_service.rs:676`; commands drive controller state. |
| [src/r/services/gna_audio_frontend_service.rs:278](../../src/r/services/gna_audio_frontend_service.rs#L278) `gna_audio_frontend_service_task` | 1 | Conditional boot `gna-audio-front-end` → E-core-preferred wrapper `spawn_service.rs:815`; HDA + background readiness. |
| [src/r/services/hda_capture_lane.rs:1314](../../src/r/services/hda_capture_lane.rs#L1314) `hda_capture_task` | 1 | Lazy `ensure_started_on_current_worker` [src/r/services/hda_capture_lane.rs:1286](../../src/r/services/hda_capture_lane.rs#L1286) → spawn ; called by GNA frontend and Shell3 capture [src/shell3/capture.rs:924](../../src/shell3/capture.rs#L924). |
| [src/r/services/hid_udp_service.rs:257](../../src/r/services/hid_udp_service.rs#L257) `hid_udp_srv_task` | 1 | Conditional boot `hid-udp-srv` → wrapper `spawn_service.rs:545`; NET configured. |
| [src/r/services/hull_image_service.rs:396](../../src/r/services/hull_image_service.rs#L396) `worker_task` | 2 | Lazy UI4 request [src/ui4/blueprint_ui4/hull_image_api.rs:160](../../src/ui4/blueprint_ui4/hull_image_api.rs#L160) → `request` → `ensure_workers` → spawn . |
| [src/r/services/image_open_service.rs:33](../../src/r/services/image_open_service.rs#L33) `launch_task` | 4 | On-demand guest FS image-open API ([src/r/io/fs_cabi.rs:1808](../../src/r/io/fs_cabi.rs#L1808)) → `enqueue` → spawn ; launches img.bp. |
| [src/r/services/keyboard_control_service.rs:530](../../src/r/services/keyboard_control_service.rs#L530) `keyboard_control_service_task` | 1 | Conditional boot `keyboard-control-service` → UI wrapper `spawn_service.rs:670`; consumes keyboard-control requests. |
| [src/r/services/media_service.rs:645](../../src/r/services/media_service.rs#L645) `worker_task` | 4 | Boot `vmedia-service` → wrapper `spawn_service.rs:378`, image spawn ; work from `commit` [src/r/services/media_service.rs:260](../../src/r/services/media_service.rs#L260), exposed via media C ABI. |
| [src/r/services/mouse_motion_service.rs:791](../../src/r/services/mouse_motion_service.rs#L791) `mouse_motion_service_task` | 1 | Conditional boot `mouse-motion-service` → UI wrapper `spawn_service.rs:664`; consumes cursor command programs. |
| [src/r/services/shell3_font_service.rs:267](../../src/r/services/shell3_font_service.rs#L267) `microfont_warm_task` | 1 | Conditional boot `microfont-warm` → local wrapper `spawn_service.rs:301`; heavy font jobs submitted to blocking service lanes ([src/r/services/shell3_font_service.rs:91](../../src/r/services/shell3_font_service.rs#L91)). |
| [src/r/services/spawn_service.rs:1412](../../src/r/services/spawn_service.rs#L1412) `spawn_service_task` | 1 | Direct boot [src/main.rs:306](../../src/main.rs#L306) → spawn ; registry supervisor, 25/150/250 ms retry loop. |
| [src/r/services/video_open_service.rs:45](../../src/r/services/video_open_service.rs#L45) `playback_task` | 3 | On-demand guest video-open API [src/r/io/fs_cabi.rs:1848](../../src/r/io/fs_cabi.rs#L1848) / [src/hv/vmcall.rs:3486](../../src/hv/vmcall.rs#L3486) → `enqueue` → spawn ; session-owned playback. |
| [src/r/services/video_service.rs:425](../../src/r/services/video_service.rs#L425) `worker_task` | 3 | Boot `vmedia-service` → wrapper `spawn_service.rs:378` → spawn ; actual requests selected [src/r/services/video_service.rs:429](../../src/r/services/video_service.rs#L429). |
| [src/r/startup_config.rs:70](../../src/r/startup_config.rs#L70) `watcher_task` | 1 | Conditional boot Shell3 `start_pool` ([src/shell3/service.rs:813](../../src/shell3/service.rs#L813)) → `start` [src/r/startup_config.rs:63](../../src/r/startup_config.rs#L63) → spawn ; file-watch reload loop. |
| [src/r/threads.rs:547](../../src/r/threads.rs#L547) `thread_task` | 256 | On-demand pthread/guest call → `spawn` [src/r/threads.rs:676](../../src/r/threads.rs#L676) → `submit` → spawn ; native continuation. |
| [src/r/threads.rs:553](../../src/r/threads.rs#L553) `codex_application_task` | 256 | On-demand Codex archive + experiment flag ([src/allcaps.rs:126](../../src/allcaps.rs#L126)) → `submit` [src/r/threads.rs:558](../../src/r/threads.rs#L558) → spawn . |
| [src/smp.rs:266](../../src/smp.rs#L266) `hlt_history_sampler_task` | 1 | Boot `smp-hlt-history` → wrapper `spawn_service.rs:324`; consumed by Shell3 SMP and thermal status. |
| [src/spirit/mod.rs:1000](../../src/spirit/mod.rs#L1000) `spirit_cursor_task` | 1 | Conditional boot `trueos-spirit` → wrapper `spawn_service.rs:597` → spawn ; active channel limit 1. |
| [src/spirit/mod.rs:1179](../../src/spirit/mod.rs#L1179) `spirit_worker_task` | 1 | Conditional boot `trueos-spirit` → wrapper `spawn_service.rs:597` → spawn ; active frame worker limit 1. |
| [src/spirit/spirit_thoughts.rs:894](../../src/spirit/spirit_thoughts.rs#L894) `spirit_thoughts_task` | 1 | Conditional boot `trueos-spirit` → wrapper `spawn_service.rs:650`; non-test service module; agent snapshots drive marker work. |
| [src/spirit/window_selection.rs:30](../../src/spirit/window_selection.rs#L30) `spirit_window_selection_task` | 1 | Conditional boot `trueos-spirit` → wrapper `spawn_service.rs:640`; waits for cursor selection source then paper/UI interactions. |

The shared job runner awaits each detached future to completion before taking the next. `LOCAL_JOBS` has priority over the legacy `JOBS` queue; both are `Vec` queues without an explicit admission cap ([src/wait.rs:773](../../src/wait.rs#L773)). A slow detached future delays subsequent jobs. The concrete producer is `spawn_local_detached`; no writer of the Send queue was found. Detached means no completion cell, rather than parallel execution.

## Central boot registry

66 possible entries across all feature combinations, 62 with default features. Two entries are explicitly disabled. TTSTT residency is additionally disabled by its warm policy. See [structured records](runtime-registry.json).

| Registry name | Admission | Required bits | Extra gate | Placement | Wrapper |
| --- | --- | --- | --- | --- | --- |
| `codex-progress-observer` | boot conditional | `BACKGROUND_AP_WORKER_READY` | `codex_progress_gate` | BSP | [spawn_codex_progress](../../src/r/services/spawn_service.rs#L553) |
| `job-runner` | boot conditional | `0` | `always` | BSP | [spawn_job_runner](../../src/r/services/spawn_service.rs#L273) |
| `blueprint-async-fs-service` | boot conditional | `0` | `always` | BSP | [spawn_blueprint_async_fs_service](../../src/r/services/spawn_service.rs#L277) |
| `trueosfs-request-broker` | boot conditional | `0` | `always` | BSP | [spawn_trueosfs_request_broker](../../src/r/services/spawn_service.rs#L281) |
| `dns-request-broker` | boot conditional | `0` | `always` | BSP | [spawn_dns_request_broker](../../src/r/services/spawn_service.rs#L285) |
| `blocking-service-lanes` | boot conditional | `BACKGROUND_AP_WORKER_READY` | `always` | BSP supervisor → AP service lanes | [spawn_blocking_service_lanes](../../src/r/services/spawn_service.rs#L289) |
| `microfont-warm` | boot conditional | `UI4_COMPOSITOR_READY + BACKGROUND_AP_WORKER_READY` | `microfont_warm_gate` | BSP | [spawn_microfont_warm](../../src/r/services/spawn_service.rs#L301) |
| `ttstt-cpu-service` | boot conditional; feature=trueos_ttstt | `BACKGROUND_AP_WORKER_READY` | `ttstt_cpu_service_gate` | BSP | [spawn_ttstt_cpu_service](../../src/r/services/spawn_service.rs#L311) |
| `ttstt-capture-writer` | boot conditional; feature=trueos_ttstt | `TRUEOSFS_ROOT_MOUNTED` | `always` | BSP | [spawn_ttstt_capture_writer](../../src/r/services/spawn_service.rs#L319) |
| `smp-hlt-history` | boot conditional | `0` | `always` | BSP | [spawn_smp_hlt_history](../../src/r/services/spawn_service.rs#L324) |
| `ram-usage-history` | boot conditional | `0` | `always` | BSP | [spawn_ram_usage_history](../../src/r/services/spawn_service.rs#L328) |
| `codec-service` | boot conditional | `BACKGROUND_AP_WORKER_READY` | `always` | background AP pool | [spawn_codec_service](../../src/r/services/spawn_service.rs#L332) |
| `vmedia-service` | boot conditional | `BACKGROUND_AP_WORKER_READY` | `always` | background AP pool | [spawn_vmedia_service](../../src/r/services/spawn_service.rs#L378) |
| `factory-ram-probe` | boot conditional | `0` | `always` | BSP | [spawn_factory_ram_probe](../../src/r/services/spawn_service.rs#L430) |
| `trueosfs-mount-service` | boot conditional | `0` | `always` | BSP | [spawn_trueosfs_mount_service](../../src/r/services/spawn_service.rs#L434) |
| `trueosfs-index-service` | boot conditional | `0` | `always` | BSP | [spawn_trueosfs_index_service](../../src/r/services/spawn_service.rs#L438) |
| `hv-vm-store` | boot conditional; feature=legacy_vm_checkpoint | `0` | `always` | BSP | [spawn_hv_vm_store](../../src/r/services/spawn_service.rs#L443) |
| `hv-vm-store-net` | boot conditional; feature=legacy_vm_checkpoint | `NET_ANY_CONFIGURED` | `always` | BSP | [spawn_hv_vm_store_net](../../src/r/services/spawn_service.rs#L448) |
| `wifi-service` | boot conditional | `0` | `wifi_service_gate` | BSP | [spawn_wifi_service](../../src/r/services/spawn_service.rs#L456) |
| `net-poll-tasks` | boot conditional | `0` | `always` | BSP | [spawn_net_poll_tasks](../../src/r/services/spawn_service.rs#L460) |
| `net-service` | boot conditional | `0` | `always` | BSP | [spawn_net_service](../../src/r/services/spawn_service.rs#L483) |
| `tls-socket-service` | boot conditional | `NET_ANY_CONFIGURED + BACKGROUND_AP_WORKER_READY` | `always` | E-core AP preferred | [spawn_tls_socket_service](../../src/r/services/spawn_service.rs#L518) |
| `clock-signals` | boot conditional | `0` | `always` | BSP | [spawn_clock_signals](../../src/r/services/spawn_service.rs#L524) |
| `ntp-sync` | boot conditional | `NET_ANY_CONFIGURED` | `always` | BSP | [spawn_ntp_sync](../../src/r/services/spawn_service.rs#L528) |
| `sntp-service` | boot conditional | `NET_V4_CONFIGURED` | `always` | BSP | [spawn_sntp_service](../../src/r/services/spawn_service.rs#L532) |
| `shell3-tcp` | boot conditional | `NET_ANY_CONFIGURED` | `always` | BSP | [spawn_shell3_tcp](../../src/r/services/spawn_service.rs#L541) |
| `sh3srv` | boot conditional | `0` | `shell3_pool_gate` | shell3 pool policy | [spawn_sh3srv](../../src/r/services/spawn_service.rs#L536) |
| `hid-udp-srv` | boot conditional | `NET_ANY_CONFIGURED` | `always` | BSP | [spawn_hid_udp_srv](../../src/r/services/spawn_service.rs#L545) |
| `logtotcp` | boot conditional | `NET_ANY_CONFIGURED` | `always` | BSP | [spawn_logtotcp](../../src/r/services/spawn_service.rs#L562) |
| `http-trueosfs` | boot conditional | `NET_ANY_CONFIGURED` | `http_trueosfs_gate` | BSP | [spawn_http_trueosfs](../../src/r/services/spawn_service.rs#L566) |
| `bp-autostart` | boot conditional | `BP_AUTOSTART_READY` | `always` | BSP | [spawn_bp_autostart](../../src/r/services/spawn_service.rs#L920) |
| `ws-time` | boot conditional | `NET_ANY_CONFIGURED` | `always` | BSP | [spawn_ws_time](../../src/r/services/spawn_service.rs#L570) |
| `usb-controller-tasks` | boot conditional | `0` | `always` | BSP | [spawn_usb_controller_tasks](../../src/r/services/spawn_service.rs#L908) |
| `lan-discovery` | boot conditional | `NET_ANY_CONFIGURED` | `always` | BSP | [spawn_lan_discovery](../../src/r/services/spawn_service.rs#L574) |
| `midi-piano-udp` | boot conditional | `NET_ANY_CONFIGURED` | `always` | BSP | [spawn_midi_piano_udp](../../src/r/services/spawn_service.rs#L586) |
| `printer-discovery` | boot conditional | `NET_ANY_CONFIGURED` | `always` | BSP | [spawn_printer_discovery](../../src/r/services/spawn_service.rs#L578) |
| `printer-spooler` | boot conditional | `NET_ANY_CONFIGURED` | `always` | BSP | [spawn_printer_spooler](../../src/r/services/spawn_service.rs#L582) |
| `gpu-completion-reaper` | boot conditional | `0` | `intel_device_gate` | BSP | [spawn_gpu_completion_reaper](../../src/r/services/spawn_service.rs#L590) |
| `gpu-fault-containment` | boot conditional | `0` | `gpu_fault_containment_gate` | BSP | [spawn_gpu_fault_containment](../../src/r/services/spawn_service.rs#L594) |
| `trueos-spirit` | boot conditional | `0` | `trueos_spirit_gate` | UI core (AP1; BSP fallback) | [spawn_trueos_spirit_workers](../../src/r/services/spawn_service.rs#L598) |
| `mouse-motion-service` | boot conditional | `0` | `ui_core_ready_gate` | UI core (AP1; BSP fallback) | [spawn_mouse_motion_service_task](../../src/r/services/spawn_service.rs#L664) |
| `keyboard-control-service` | boot conditional | `0` | `ui_core_ready_gate` | UI core (AP1; BSP fallback) | [spawn_keyboard_control_service_task](../../src/r/services/spawn_service.rs#L670) |
| `gamepad-control-service` | boot conditional | `0` | `ui_core_ready_gate` | UI core (AP1; BSP fallback) | [spawn_gamepad_control_service_task](../../src/r/services/spawn_service.rs#L676) |
| `ui4-input-service` | boot conditional | `0` | `ui4_input_gate` | UI core (AP1; BSP fallback) | [spawn_ui4_input_service_task](../../src/r/services/spawn_service.rs#L682) |
| `ui4-slot4-service` | boot conditional | `0` | `ui4_slot4_service_gate` | UI core (AP1; BSP fallback) | [spawn_ui4_slot4_service_task](../../src/r/services/spawn_service.rs#L686) |
| `ui4-screenshot-service` | boot conditional | `BACKGROUND_AP_WORKER_READY` | `always` | background AP | [spawn_ui4_screenshot_service_task](../../src/r/services/spawn_service.rs#L694) |
| `ui4-h264-encode-stream` | boot conditional; feature=trueos_h264_encode_stream | `BACKGROUND_AP_WORKER_READY` | `h264_stream_service_gate` | BSP | [spawn_ui4_h264_encode_stream_task](../../src/r/services/spawn_service.rs#L699) |
| `ui4-compositor-service` | boot conditional | `0` | `ui4_compositor_gate` | UI core (AP1; BSP fallback) | [spawn_ui4_compositor_service_task](../../src/r/services/spawn_service.rs#L723) |
| `ui4-color-picker` | boot conditional | `UI4_COMPOSITOR_READY` | `always` | UI core (AP1; BSP fallback) | [spawn_ui4_color_picker_service_task](../../src/r/services/spawn_service.rs#L727) |
| `ui4-link-service` | boot conditional | `UI4_COMPOSITOR_READY` | `always` | BSP | [spawn_ui4_link_service_task](../../src/r/services/spawn_service.rs#L733) |
| `ui4-codex-prompt-service` | boot conditional | `UI4_COMPOSITOR_READY` | `always` | BSP | [spawn_ui4_codex_prompt_service_task](../../src/r/services/spawn_service.rs#L737) |
| `ui4-window-broker-snapshot` | boot conditional | `0` | `ui4_input_gate` | BSP | [spawn_ui4_window_broker_snapshot_service_task](../../src/r/services/spawn_service.rs#L741) |
| `ui4-video-conversion-service` | boot conditional | `BACKGROUND_AP_WORKER_READY` | `ui4_compositor_gate` | background AP, two cooperative lanes | [spawn_ui4_video_conversion_service_task](../../src/r/services/spawn_service.rs#L745) |
| `gpgpu-ui4-preview-consumer-service` | boot conditional | `BACKGROUND_AP_WORKER_READY` | `ui4_compositor_gate` | background AP | [spawn_gpgpu_ui4_preview_consumer_service_task](../../src/r/services/spawn_service.rs#L775) |
| `gpgpu-ui4-svg-probe-consumer-service` | boot conditional | `BACKGROUND_AP_WORKER_READY` | `ui4_compositor_gate` | background AP | [spawn_gpgpu_ui4_svg_probe_consumer_service_task](../../src/r/services/spawn_service.rs#L781) |
| `hw_pic_service` | boot conditional | `0` | `intel_media_engine_gate` | UI core (AP1; BSP fallback) | [spawn_hw_pic_service](../../src/r/services/spawn_service.rs#L787) |
| `fallback-logo-ui` | boot conditional | `0` | `fallback_logo_ui_gate` | BSP | [spawn_fallback_logo_ui_task](../../src/r/services/spawn_service.rs#L791) |
| `intel-hda-audio-demo` | registry disabled | `0` | `always` | background AP | [spawn_intel_hda_audio_demo_task](../../src/r/services/spawn_service.rs#L795) |
| `raple-service` | registry disabled | `0` | `always` | BSP | [spawn_raple_service](../../src/r/services/spawn_service.rs#L802) |
| `thermal-service` | boot conditional | `0` | `always` | BSP | [spawn_thermal_service](../../src/r/services/spawn_service.rs#L806) |
| `html_fetch_service` | boot conditional | `NET_V4_CONFIGURED + BACKGROUND_AP_WORKER_READY` | `html_shack_gate` | surfer pool policy | [html_fetch_service](../../src/r/services/spawn_service.rs#L810) |
| `gna-audio-front-end` | boot conditional | `INTEL_HDA_READY + BACKGROUND_AP_WORKER_READY` | `always` | E-core AP preferred | [spawn_gna_audio_frontend_service](../../src/r/services/spawn_service.rs#L815) |
| `tinyaudio_service` | boot conditional | `INTEL_HDA_READY + BACKGROUND_AP_WORKER_READY` | `always` | E-core AP preferred | [spawn_tinyaudio_service](../../src/r/services/spawn_service.rs#L821) |
| `tinyaudio-live-http` | boot conditional | `NET_ANY_CONFIGURED + INTEL_HDA_READY` | `always` | BSP | [spawn_tinyaudio_live_http](../../src/r/services/spawn_service.rs#L825) |
| `user-input-record-writer` | boot conditional | `0` | `user_input_writer_gate` | BSP | [spawn_user_input_record_writer](../../src/r/services/spawn_service.rs#L916) |
