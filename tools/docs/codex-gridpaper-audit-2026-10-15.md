# Codex → Shell3 → Gridpaper, 2026-10-15

## Physical success and remaining local delay

The operator's later run of **Codex `c970a622`**, GPT-6 Luna / low, completed
the automatic `gri` request. Gridpaper is visible behind Codex in the supplied
screenshot. The corresponding physical capture independently records a new
Gridpaper presentation attaching at line 14816 and a frame publishing at line
14839. Codex then received the command-session result and completed its turn;
the screenshot reports **9m13s**. Its final answer says it sent Ctrl+C after
observing the application. The screenshot still lists a background terminal,
so terminal cleanup is not established by turn completion.

This supersedes the earlier sections' pending end-to-end observation. Code Mode,
native tool dispatch, Shell3 alias resolution and Gridpaper presentation now
work together with the requested model. MCP connector startup is a separate
dependency from local execution through Code Mode.

The measured delay is predominantly **before the first real model request**:

| Boundary | Elapsed time |
| --- | ---: |
| Prompt telemetry to first real WebSocket request | approximately 533 s |
| Record turn context | 47.847 s |
| Session-start hooks and their preparation | 21.397 s |
| Inspect hooks and record input | 429.806 s |
| First real response stream TTFT | 1.124 s |
| Subsequent response stream TTFTs | 0.794 s, 0.699 s |

The hook/input label encloses transcript preparation, input recording and
durability barriers; it does not prove that hook handlers executed or consumed
that time. These spans include waits and synchronous work. They are not CPU
samples and are not added to overlapping startup captures. The cross-event
533-second interval uses wall timestamps with roughly one-second precision.

Safe baseline metadata and allowlisted measurements are retained in
`bld/codex-gridpaper-success-20261015/baseline.json`. The captured prefix of
`trueos-baremetal.2.log` is 2,724,996 bytes, SHA-256
`1b08349c8f531b37b28c197d65ca05359a94f29aa743beefc14505f960c656a7`.
The physical runtime's SHA-256 is
`0059ba562cc963b1a604240020be791cc3947ed7c2ce6b708a80adb24c1a2043`,
matching `bld/artifacts/debug-50fce22f84e2/TRUEOS.elf` (**debug / opt1**).
It already contains the shared WLS pool, allocator address fences and admitted
heap-watermark diagnostics. The separately tested release kernel has a
different identity; its timings must not be substituted for this physical run.

Three concrete local issues are being checked against these boundaries:

- The fixed C allocation registry becomes full in this capture. Its existing
  missing-key operations scan up to 65,536 entries under a shared mutex.
- Thread-history projection checks for a local SQLite file even when the SQLx
  backend keeps the database on the service. This falsely reports no projection
  checkpoint and triggers repeated replay attempts.
- Hook requests prepare a durable transcript before determining whether any
  configured handlers match. The observed hook phase therefore includes work
  even when no handlers execute.

The fixes and subsequent acceptance results must be distinguished from this
baseline; it predates them. The old capability warnings and pending results
below document earlier artifacts rather than this successful run.

### Repairs following the successful run

The C allocation registry now limits each operation to one page's worth of
entries: 128 for its 32-byte metadata records. Tombstones preserve lookup chains
and reuse deleted slots without moving or evicting live records. Exhaustion
retains the existing untracked allocation fallback, using authoritative shared
allocator tags for free/realloc. Neither the 65,536-record capacity nor its
2 MiB storage increased. Ten production registry tests pass, including bounded
collision chains, wrapped deletion, full/tombstone reuse, ownership drops and
randomized differential checks.

An instrumented production-sized host fixture changes 100 saturated
insert/get/remove cycles from 19,660,800 to 38,400 slot visits (**512× fewer**).
Those counters establish the algorithmic bound, not a physical turn speedup.

The unchanged native `alloc_registry` probe keeps 70,000 C allocations live,
checks realloc contents before and after alternating free/refill churn, and
verifies/releases every allocation. Both isolated release-kernel runs pass:

| Native probe boundary | Previous registry | Bounded registry |
| --- | ---: | ---: |
| Fill 70,000 allocations | 222 ms | 28 ms |
| 1,072 reallocations at capacity | 147 ms | 1 ms |
| Free alternate 35,000 allocations | 178 ms | 249 ms |
| Refill 35,000 allocations | 120 ms | 12 ms |
| 620 reallocations after churn | 112 ms | 1 ms |
| Check/free 70,000 allocations | 242 ms | 10 ms |
| Complete probe | 1,030 ms | 307 ms |

These are single-run, millisecond-resolution native measurements. The slower
alternate-free phase remains visible; this is not a claim that every operation
improved. Both release probe runs differ from the physical debug/opt1 Codex
workload, and their total difference does not account for its full delay.
The probe and before/after/package-readback receipts are under
`tools/probes/alloc_registry/validation/`. Package `35ac4304` is published to the
requested Apps share, registered only as a probe. Its bytes are identical in
both runs. The repaired isolated kernel is
`bld/registry-saturation-fixed/TrueOS.lz4`; its runtime SHA-256 is
`4e7ac5fe20910d4cd94efb16ad1a1513b81744c62a4a4ffe9ede7cad46e02c00`.
The physical rig was not reset or replaced during these tests.

`SqliteConfig::database_exists()` now asks the backend that owns the database.
Ordinary hosts retain filesystem existence/error semantics; TRUEOS uses a
read-only, non-creating service connection. Initialized history/memory stores
reuse their pool rather than opening an existence connection per append.
Thread-history projection/deletion and memory reset/deletion share this
contract. No local marker files substitute for remote state. This fixes the
observed false missing-checkpoint/replay failure. Two state-existence tests,
32 existing history materialization tests and eight remote adapter tests pass.
An integration harness uses the production checkpoint/deletion functions and
real remote SQLx adapter/service without a client database file; checkpoint
advancement, reopen, deletion, no-create absence and error fidelity all pass.

The RPC trace now adds only a static operation label, distinguishing
open/execute/prepare/transaction/ping/close work without SQL or payloads.
Established header waits in the baseline, including a 30.417-second wait next
to a 30.551-second pool acquisition, remain service/scheduling attribution
questions. They overlap across connections and are not summed as critical-path
time. A health ping can wait too; the baseline cannot distinguish it from SQL.

Startup and prompt-hook requests now select matching handlers before preparing
a durable transcript, using the same captured Hooks snapshot for selection and
dispatch. No-match requests still dispatch normally but omit the unused
transcript barrier. Matching synchronous/asynchronous hooks retain their actual
materialized transcript. Stop/Interrupt/executor-hook semantics and final input
durability are unchanged. Eight static subphases distinguish hook inspection,
transcript lookup/persistence, input preparation/history recording, UI events
and final persistence. These measurements preserve unfinished/cancelled states;
no input content or transcript paths are logged.

All 25 core transcript regressions pass, including the new no-match case and
matching synchronous/asynchronous handlers reading actual durable SessionMeta.
The production timing-helper tests pass (5); parser/gate checks pass (17),
including two Rust tests compiling the actual TRUEOS-only telemetry method.

The native Codex release build and C ABI guard passed. The updated package is
**`codex.bp§§f3156fab46e1573357f7988454f5983e083d64674c1b9511ca018fb846e80e49`**,
66,675,754 bytes, published to the requested Apps share. A complete remote
readback has the same SHA-256. Its title prefix is `f3156fab`. The cached
QuickJS source was verified at the native backend's pinned revision
`c2eaa5fb7a5f8f04e084412560dd48d02bd00d8d` for the offline build. Package/source
identities are in `bld/codex-gridpaper-success-20261015/updated-app.json`.
Luna/low, Code Mode, MCP settings and the automatic `gri` request are retained.
Publication does not replace the running `c970a622` session. The complete
physical turn has not yet been timed with this app and the registry-repair
kernel together.

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
