# Codex startup timing — Ubuntu, 2026-10-13

Build: `cargo +1.95.0 build --bin codex --no-default-features`. Launch: `codex --no-daemon`.
These are naive wall-clock measurements of the unoptimized Linux development build, with a warm OS cache. Fresh means an empty disposable CODEX_HOME with existing authentication copied into it and a trusted empty workspace; it is not a cold boot. Repeat reuses that disposable profile. No model prompt was submitted; the probe typed text without Enter and verified it appeared.

Time zero is the first marker in CLI main, excluding Cargo/build time. External process launch adds approximately 10–14 ms here. Stages overlap with background startup work; the rows below are milestones, not additive durations.

| Milestone | Fresh profile ms | Repeat ms | Repeat, no terminal replies ms |
|---|---:|---:|---:|
| `main_thread_entered` | 0.304 | 0.508 | 0.322 |
| `runtime_done` | 2.042 | 2.041 | 2.097 |
| `cli_entering_tui` | 5.136 | 5.188 | 5.308 |
| `terminal_modes_done` | 14.787 | 11.637 | 12.735 |
| `ratatui_terminal_created` | 14.914 | 11.769 | 262.170 |
| `first_frame_render_begin` | 17.357 | 13.312 | 264.099 |
| `first_frame_render_done` | 37.840 | 32.539 | 283.340 |
| `first_frame_backend_flushed` | 38.532 | 33.191 | 284.006 |
| `editable_composer_visible` | 38.541 | 33.202 | 284.017 |
| `state_db_done` | 162.031 | 120.311 | 361.582 |
| `app_server_done` | 204.776 | 157.946 | 399.638 |
| `account_status_done` | 510.591 | 452.714 | 628.391 |
| `thread_start_done` | 686.410 | 632.337 | 822.624 |
| `session_prompt_ready` | 737.518 | 693.883 | 881.921 |

| Measured operation | Fresh ms | Repeat ms |
|---|---:|---:|
| PATH aliases | 0.077 | 0.166 |
| Runtime construction | 1.722 | 1.513 |
| Presentation config | 5.082 | 3.305 |
| Terminal queries | 0.083 | 0.089 |
| First render callback | 20.483 | 19.227 |
| Cloud config | 18.831 | 19.136 |
| Full config | 9.752 | 18.950 |
| State database | 57.548 | 28.152 |
| Embedded app server | 31.528 | 30.616 |
| Account status | 305.784 | 294.741 |
| Thread start request | 30.522 | 30.345 |

With no terminal replies, queries take 249.356 ms, matching the configured 250 ms deadline. Without authentication, onboarding is painted at 224.701 ms; that is not a session-ready prompt.

`first_frame_backend_flushed` is emitted only after the render callback, terminal update writes and backend flush succeed. It proves execution through the Ratatui-derived terminal renderer and successful output, not physical display visibility on baremetal. `session_prompt_ready` requires a configured session, completed startup thread start, no modal, and a successfully drawn frame.

The authenticated fresh and repeat runs both reached session readiness and displayed the typed text. A second unauthenticated probe with stderr tracing verified that `main_thread_entered`, `ratatui_terminal_created` and `first_frame_backend_flushed` actually appear in the terminal capture.

An earlier trial measured 38.5 ms to the first composer and 908.4 ms to session readiness; the repeat reached readiness at 652.3 ms. Account status timings varied significantly. These single trials identify stages, not statistical performance bounds or baremetal speed predictions.

## Reproduce

From `/home/t4ce/Repos/codex/codex-rs`:

```sh
CODEX_STARTUP_TIMING_STDERR=1 cargo run --bin codex --no-default-features -- --no-daemon
```

For undisturbed UI output, use `CODEX_STARTUP_TIMING_FILE=/tmp/codex-startup.tsv` instead. TSV columns are PID, elapsed milliseconds, previous-marker delta milliseconds, stage. Each marker is recorded once per process. Both sinks are opt-in.

Automated PTY probe, from the Codex repository root:

```sh
python3 scripts/measure_startup.py \
  --binary codex-rs/target/debug/codex \
  --output /tmp/codex-startup-results \
  --auth-source "$HOME/.codex/auth.json"
```

Omit `--auth-source` for onboarding-only tests; add `--stderr` to verify visible markers. Profiles and copied authentication are removed after the probe. Result files retain timing rows and terminal transcripts, which may contain account labels.

Raw measurements: `bld/codex-startup-timing/results.json` and the corresponding TSV files. Visible-marker verification: `bld/codex-startup-timing-stderr/`. The published baremetal pack predates this instrumentation; rebuilding it is required before collecting these markers on-device.
