# Runtime retirement decisions

Updated 2 November 2026. TRUEOS has no wired local LLM inference service for general agent work. Spirit remains integrated with Codex. TTSTT/Kokoro remains a separate optional speech path for future voice and text integration.

## Removed paths

| Path | Facts and decision |
| --- | --- |
| Lumen warmup and Blueprint workers | Removed the `trueos_lumen` feature, boot entry, session worker, kernel model/decode/backend modules, row-pool wiring, Lumen-only placement helpers, and inference-scoped GT boost. Root kernel dependencies on LFM CPU/model crates are removed. Those crates and the uncompiled turbo patch/reference bundle remain development material. |
| FPGA TGA capture card | Removed its task, RPC worker, heartbeat, PCI claim switch, interrupt registration, protocol/camera modules, and dependent Shell3 `cam` command and save/menu tasks. The existing screenshot/audio/video capture paths remain. |
| FTP | Removed the unused client/server module, disabled boot entry, and FTP-only port constants. Share remains the file-sharing application; this does not claim that its protocol is FTP-compatible. |
| Network throughput tasks | Removed both benchmark tasks, boot wrapper, timing/buffer settings, and reserved benchmark port. The benchmark was policy-disabled; normal networking and its workers remain. |
| USB LED probe | Removed its disconnected start helpers/task and HID module declaration. No active device-discovery caller was found. |
| Filesystem read/write probe | Removed the disabled 500 KiB create/write/read/compare/delete task and its helpers. Normal filesystem services and their tests remain. |
| Unix FD probe | Removed the disabled one-shot POSIX/Unix compatibility smoke test, its boot wrapper/spec, started flag, and ineffective configuration switch. It tested file I/O, descriptor flags, polling, socket/terminal operations, and persistence by calling the real compatibility layer. Both switch branches disabled it; no other caller was found. The actual Unix FD/socket implementation remains. |
| Panic test | Removed `atomic_bomb_task`, which deliberately panicked after a delay, together with its disabled boot entry. |
| Post-restart logger | Removed `atomic_bomb_after_restart_task`, which did not panic: it waited two seconds and printed post-restart messages. This was a diagnostic attached to AP startup retry. The guarded startup-retry mechanism remains and still refuses replacement of an already-published executor or a VM-owned continuation. |
| Executor migration smoke test | Removed the disabled BSP/AP migration probe and its diagnostic helpers. It exercised migration; it did not provide recovery or instantiate a nested Realm. The general executor migration API remains library infrastructure. |
| Network cache service | Removed its boot task and module. It was a RAM byte store with SHA-256 verification: 64 cache entries, a 32-request queue, and a 32-completion table. No callers of store/load/verify submission or direct storage APIs were found outside the module. The task otherwise woke every millisecond with no producer. It was unrelated to DNS caching, HTTP connection reuse, packet-buffer recycling, or hardware caches. |

Eighteen concrete task declarations were removed from the November working tree, including the two dependent FPGA camera tasks. The [current index](runtime-task-index.md) contains 130 declarations across all cfg variants, including one uncompiled reference overlay. The registry has 62 entries with default features and 66 across optional features.

## Spirit capabilities retained

Independent Spirit presentation moved from the former Lumen service to [spirit_abi.rs](../../src/ai/spirit_abi.rs). Existing silent-text, emotion, movement, and guest VMCall forwarding remain available. Codex agent snapshots, progress/input integration, and Spirit's rendering/sequencer behavior remain wired. Speech activity is named generically and still comes from audio; it does not imply a local reasoning model.

The old Lumen-related import and VMCall numbers remain compatibility endpoints in [lumen_service.rs](../../src/ai/lumen_service.rs). Inference operations return `-5` (unavailable), with no session queue, model residency, worker, or inference-to-Spirit presentation. Close is a harmless no-op. The old Lumen-session-bound response endpoint is unavailable; independent Spirit silent text remains supported. General GPU cache-policy code with historical Lumen names is hardware configuration, not an inference service.

The sibling Blueprint API separates its retired `api/src/lumen.rs` response path from `api/src/spirit.rs` silent text. Codex's Spirit tools and agent-snapshot publishers use the independent Spirit API; no sibling source was changed for this cleanup.

## MicroFont polling removed and panel retained

The name `microfont_log_service` no longer describes a generic log reader. Its laptop diagnostic surface now paints only a Wi-Fi square and the eight-bit IP octet, without a font or atlas. The seven-plane laptop layout centers that compact surface on logical slot 6, below the interaction plane on physical plane 7. Logical slot 4 remains UI4's software-cursor and interaction carrier. The old assignment-verdict, USB-stage and font-ready callbacks remain compatibility no-ops.

The retained allocation and its display ownership remain. The redundant 25 ms polling task and boot entry were removed; Wi-Fi state and address changes publish directly through callbacks. [The module](../../src/r/services/microfont_log_service.rs) has no executor task.

## Validation

Kernel type checks passed for default features and `trueos_ttstt` using the verified 12-application builtin set. A final repeat was blocked after the separate builtin configuration began referencing the missing `TRUEOS-Buildins/hyper/Cargo.toml`; that configuration was not changed for this cleanup. Behavioral tests cover unavailable inference endpoints, unchanged output buffers, preserved Spirit direct/guest presentation, Spirit agent transport and sequencer behavior, and retained Wi-Fi status painting. This cleanup does not claim a physical hardware boot or runtime liveness measurement.

The Unix FD probe follow-up passed the production socket-registry ownership/retirement host test. A kernel check was attempted but stopped in the builtin verification step because the prepared builtins are stale; it did not reach kernel compilation.
