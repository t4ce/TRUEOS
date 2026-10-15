# Codex → Shell3 → Gridpaper, 2026-10-15

The next functional smoke test replaces the greeting with an actual Shell3
operation: execute the `gri` alias and observe a new Gridpaper presentation in
UI4. A greeting already proved authenticated request/response delivery. This
scenario adds native tool execution, alias resolution, application launch and
presentation. It does not require launching another GUI shell.

The live `apps/common/startup.json`, read through the discovered public root on
October 15, maps **`gri` → `gridpaper`**. The checked-in default currently uses
`grid`; it is not authoritative for this rig. Shell3 matches aliases exactly.
A native command session starts in CMD mode and appends Enter to its initial
script. There is no preliminary Tab or Unix shell invocation.

## Functional scenario

Select a model that supports direct tools for this native control. The bundled
catalog's `gpt-5.5` entry has no CodeModeOnly requirement. Its Astra and Luna
entries require CodeModeOnly: the capability repair deliberately does not
override that requirement, so those entries will still need a supported Code
Mode host transport before they can execute `gri`. The deployed, refreshed
catalog and account's selectable models must be checked in the TUI; the bundled
catalog alone does not establish current account availability. No model
selection was changed in the live session.

The actual automatic startup injection is
`codex-rs/tui/src/app/startup.rs::trueos_diagnostic_initial_prompt`.
Its default `hi` has been replaced with the following task, submitted through
the existing initial-message path once the session is configured:

> Run exactly `gri` through native Shell3 to open Gridpaper in UI4. Use the
> execution tool and report its actual result.

This remains limited to fresh, authenticated TRUEOS sessions without an explicit
initial prompt or images. Explicit user prompts, other platforms, resume/other
startup selections and the configured model/effort retain their existing
behavior. It does not require manually entering the replacement task.

The expected direct tool arguments are:

```json
{"cmd":"gri","tty":true,"yield_time_ms":1000}
```

Omit `shell`, `login` and `workdir`: the TRUEOS handler rejects these Unix
options. If the result returns a session ID, a still-running command session is
not itself a failure. Read that same session using `write_stdin` with empty
input to inspect output; do not repeatedly launch the alias.

Record these independent observations:

1. The model requests the native execution tool with the verified alias.
2. The tool opens its dedicated Shell3 session and returns output or a session.
3. Shell3 resolves the alias and admits a Gridpaper launch. If Gridpaper is not
   cached, distinguish App Store download from the native session admission.
4. A **new** Gridpaper instance reaches `presentation attached` and
   `frame published` diagnostics, then is visibly present in UI4.
5. Codex receives the tool result and finishes its turn. Retain the session for
   inspection rather than treating termination as necessary for GUI launch.

Correlate the new launch/instance, not an older already-open Gridpaper window.
The physical Codex session and any user draft were left untouched during this
audit. No live `gri` success is claimed by the source changes or host tests.

## Errno 38 was outside Shell3

The physical capture's first failed tool result is at lines 17384–17386 of
`trueos-baremetal.1.log`:

```text
failed to spawn code-mode host /apps/codex/codex-code-mode-host:
trueos errno 38 (os error 38)
```

`TRUEOS_ENOSYS = 38` means **operation not implemented**. The model called
`functions.exec`; Code Mode tried to spawn its standalone process host before
executing the embedded tool call. The inner Shell3 command was never reached.
The capture does not identify which unsupported process primitive failed first.

The provider's old availability check accepted an existing executable file as
proof of runtime support. The repair reports this process host unavailable on
TRUEOS before that check and also guards direct session connection attempts.
Ordinary CodeMode can then use the existing direct-tool fallback. Explicit
CodeModeOnly and configurations that disable fallback retain their existing
closed behavior. This repairs capability reporting; it does not implement Unix
process spawning or a native Code Mode transport.

The native route is separate and implemented:

```text
exec_command → spawn_shell3_script → command_session::KernelBackend
  → versioned Shell3 session C ABI → dedicated Shell3 terminal
  → gri alias → Gridpaper launch → UI4
```

## Physical timing baseline

The baseline parser snapshot is `bld/codex-gridpaper-audit/baseline.json`.
It identifies the captured prefix by byte count and SHA-256; the live capture
continues growing. The following are measured observations from that boot,
before these Codex changes, rather than predictions for the next test.

| Boundary | Observed time |
| --- | ---: |
| First frame flushed, from process marker | 149 ms |
| State DB initialization | 1,802 ms |
| Embedded app server initialization | 1,257 ms |
| Account status | 1,051 ms |
| Thread start markers | 2,589 ms |
| Session prompt ready, from process marker | 75,653 ms |
| Primary startup step-context capture (`startup_prewarm_build_tools`) | 62,309 ms |
| Primary startup prewarm total | 69,866 ms |
| Greeting, end-to-end turn time to first token | 89,256 ms |
| Greeting, response-stream TTFT | 960 ms |
| First failed Code Mode tool invocation | 4,854 ms |

The screenshot's 99-second completed greeting and 51-second tool turn include
more work than response-stream TTFT or tool invocation duration. Later primary
turns show approximately 23–24 seconds from prompt telemetry to the first real
WebSocket request, with response TTFT around 1–2.3 seconds. These observations
place substantial elapsed time before request transmission; they do not yet
assign that time to CPU execution, storage, locks or service readiness.

`startup_prewarm_build_tools` encloses the entire `capture_step_context`, not
only schema construction. It can await environments, instructions, capability
discovery, MCP readiness, recommendations/plugins, tool construction and the
conversation snapshot. Instructions and tool preparation overlap. WebSocket
preconnection is another concurrent branch; its wait after capture does not
inflate the capture's own timer. A background thread measured 471 ms for this
capture, which is useful evidence of differing initialization/cache state but
does not identify the slow primary dependency.

## Fundamental paths still under investigation

| Path | Established behavior | Attribution still missing |
| --- | --- | --- |
| SQLite persistence | Ordinary depth-zero Execute/Prepare commands take a per-database lock, serialize a complete image before and after the command, compare images, and publish the complete changed image before acknowledgement. Ping bypasses this path; transactions retain their guard and defer publication. | Queue/lock wait, image sizes, serialization, SQL, reclamation and disk publication durations for these turns. |
| SQLite storage placement | Runtime SQLite root is `apps/sqlite`, common root is `apps/common`, instance is `codex`. The default storage path resolves to `apps/common/sqlite-service/codex`; current RAM routing covers the private Codex prefixes. | A direct per-image write event identifying resolved medium and elapsed time. Moving the Codex home alone does not demonstrate RAM-backed SQLite persistence. |
| Ordinary file I/O | Uncached open materializes the whole file. Writeback clones and publishes the whole file, then reads metadata. Prior debugger evidence reaches this compatibility path during a turn. | Logical bytes versus materialized/published bytes and actual latency for this boot. Earlier lock-over-wait repairs do not remove whole-file amplification. |
| Remote SQLx | Per-connection sequential command queue, complete JSON requests/results and a service discovery path. Pool slow-acquire/timeout records occur during this boot. | Queue/service work versus scheduler delay; a pool timeout alone does not identify the cause. |
| Cooperative execution | Repeated five-second samples show unchanged poll counts with ready peers, followed by resumed progress. | The active continuation/stack, resource owner and runnable-to-dispatch delay during each interval. |
| Transport | Authenticated responses and tool round trips complete. Current SSE TTFT is much shorter than the full turn latency. | Correlate connection/send/first-event/completion/local delivery for each request. Historical opaque TLS failures are not evidence of a current failure. |

These are a system map and measurement gaps, not an ordered optimization list.
No rare stub is blamed merely because it exists in the source tree.

## Added measurement coverage

TRUEOS step captures emit `codex.step_context_phase` with a process-local
numeric capture ID, fixed phase name, monotonic start offset, duration and
completion/error/cancellation status. It covers active-turn snapshot,
environment readiness, instructions, capability roots/discovery, MCP readiness,
recommendations, selected contributors, router construction, conversation
snapshot and retained context. Existing joins and cancellation behavior remain
in place. An unfinished dropped timing scope cannot be reported as successful.

Run from the TRUEOS repository:

```sh
python3 tools/testpy/codex_phase_report.py --format markdown
python3 tools/testpy/test_codex_phase_report.py
```

The reporter defaults to `LatestOfThree.logs`, accepts an explicit capture,
ignores `trace_safe` mirrors, separates primary/background sessions and warmup
requests, and retains line numbers. It emits allowlisted numeric/static fields
without prompts, tool payloads, auth contents or account identifiers. Same-
capture offsets preserve overlapping phases; missing measurements remain null.
Cross-event wall-clock differences have roughly one-second precision in this
capture. An event recording WebSocket send duration is not full network latency.

## Validation and publication

- Code Mode provider host tests: 6 passed. Synthetic TRUEOS-cfg tests: 5 passed,
  covering availability and direct session creation; these are branch tests,
  not hardware execution.
- Existing unavailable-host integration tests: 3 passed, covering ordinary
  fallback, CodeModeOnly and explicitly disabled fallback. The initial default
  host test stack overflowed; rerunning with `RUST_MIN_STACK=33554432` passed.
- Native Codex Shell3 adapter: 3 passed.
- Production Shell3 session registry/client fixture: 1 passed.
- Shell3 TUI/lifecycle suite: 30 passed after adding its missing timing stub.
- Offline report: 9 privacy/correlation/missing-measurement tests passed.
- Timing helper: 3 production-helper tests passed in an isolated harness;
  actual-dependency host library checks also passed.
- `cargo bp codex-cli`: real TRUEOS release build and C ABI pack guard passed,
  including the TRUEOS-only telemetry emission branch.

The older `test_shell3_pool_input.py` cannot run because it extracts deleted
Shell2 launch functions. Its launch/receipt fixtures need migration to the
current asynchronous launcher API. It was left unchanged; no pass is claimed.

The subsequent startup-prompt correction passed the existing fresh/authenticated
initial-prompt regression in `codex-tui`. Its real TRUEOS release build also
passed and published **codex.bp**, 66,682,370 bytes, to the requested Apps share:

```text
smb://t4ce@pdjb/home-share/TRUEOS_SITE/apps/
codex.bp§§f87b3098366e94d77cab8007be003fa3e032df347ba93ea009e052956f665df9
```

The local and remotely read package SHA-256 both equal the filename's digest.
Build log: `/tmp/codex-gridpaper-startup-build.log`. The running physical app was not
replaced or restarted. The next launch must fetch this package before its new
capability diagnosis and step-capture measurements can be evaluated. Successful
Gridpaper launch through Codex remains a runtime observation to collect.
