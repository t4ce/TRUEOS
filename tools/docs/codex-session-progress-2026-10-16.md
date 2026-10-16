# Native Shell3 input and startup progress

The physical screenshot identifies Codex `3d9e41ad` completing the unchanged
Luna/low → native Shell3 → `gri` scenario in 1m21s, after the earlier 1m33s run.
This is a whole-scenario observation, not an attribution to one repair. The
debug kernel runtime ELF is
`4005b76a3b46aa8d9742403c2ecc2f5fe100ee61c4dd6e84260227afab6e129d`;
its matching full ELF is retained under `bld/artifacts/debug-2c5ef4ca1470`.

The privacy-filtered frozen baseline is
`bld/codex-performance-20261016/baseline.json`, from a 2,541,950-byte prefix of
`trueos-baremetal.0.log`, SHA-256
`95eb3234b2011c56fabdd33770c2ab0ba7f43a6c4328a07a4757f533e2f7a66f`.
Exact artifact identities are in the adjacent `baseline-identity.json`.

## What the new measurements establish

| Boundary | Previous physical capture | Current physical capture |
| --- | ---: | ---: |
| Initial prompt to first real request | about 78 s | about 38 s |
| Context recording | 43.026 s | 2.255 s |
| MCP readiness in the primary capture | 19.771 s | 20.844 s |
| First response stream TTFT | 977 ms | 1,304 ms |

Current context subphases are world-state generation 1,127 ms, rendering 4 ms,
history recording 231 ms, snapshot persistence 255 ms and settings checkpoint
635 ms. Display roots take 671 ms concurrently. Input recording takes 1,768 ms.
The later follow-up reaches turn TTFT in 5,414 ms.

Database service samples now reach the capture. Their largest admitted service
sample is a 38 ms commit, with 33 ms in publication. This sampling does not cover
every request: client-side Ping reply waits still reach 11,177 and 11,174 ms,
and one Execute waits 8,537 ms after 5,791 ms queued. Sampled producer work and
client-observed delay must remain distinct.

## Retained input versus terminal geometry

The model launched the original command without `tty`. The TRUEOS branch
intentionally creates a Shell3 command session with input open, even without
explicit terminal geometry. The generic `write_stdin` check nevertheless used
the `tty` bit to declare that same input closed. Thus the rejection occurred
before transport; the native session had not lost its input channel after
yielding.

The repair records actual stdin capability separately from terminal geometry.
It must preserve host/remote pipe semantics and the existing explicit EOF and
termination paths. It applies to native Shell3 sessions generally, without a
command-name exception or a change to tool defaults.

`exit` is a real Shell3 terminal built-in, not a Unix emulation added for this
test. It closes the terminal and disposes its original owned slot lease. A
selected application/TUI can receive input directly, so merely accepting a
write does not prove that every selected application slot was closed.

## Startup failure and later MCP recovery

Two tools/list attempts fail during startup:

- HTTP sequence 8 receives 98,304 bytes, then cancels after 34,285 ms in body
  collection; the operation deadline remains 30 seconds.
- HTTP sequence 13 receives 267,264 bytes in its first millisecond after
  headers, then cancels after 18,561 ms in collection when the operation's
  30-second deadline expires.

During the later user follow-up, sequence 17 receives all **608,041 bytes in
109 ms**, decodes JSON in 1,278 ms, and completes tools/list in **1,654 ms**.
This proves that the process can fetch the full inventory later. Discovery can
create a separate threadless MCP runtime, and these phase records lack runtime
identity, so this does not prove the original thread's MCP manager became Ready.
The visible warning intentionally retains startup history. Per-thread status
would be needed to establish recovery; hiding the warning or raising deadlines
is not justified by this capture.

The failure happens before JSON decoding starts. The successful body uses local
reqwest-owned chunks; a remote exec-server delta queue is not established as
the cause of the earlier partial byte counts. Existing native TCP/Mio paths
show no concrete rearm violation in this source audit.

Long cooperative continuation intervals still accompany allocation ABI churn.
Exit stacks identify their final wait/yield boundary, not preceding CPU work.
The next measurement must distinguish raw socket delivery and queued input
from delayed consumers, and count actual allocator scanning. Network bytes
include TLS/HTTP2 framing and must not be equated to plaintext body bytes.

The live rig and its existing sessions were not changed during this work.

## Changes and validation

The stdin fix keeps tool defaults and host/remote launch policy intact. The
process records its actual launch input capability; writes and Ctrl-C handling
use that capability while terminal metadata remains available to approval
review. The targeted suite passes 81 unified-execution tests, five native
adapter tests and the production SDK/OS command-session contract. An unchanged
broader terminal harness has 37 passes and two help-related assertions failing;
those results are not represented as a green full terminal suite.

New kernel diagnostics add at most four raw TCP progress rows and four allocator
work rows per five-second observer sample. Network snapshots use try-lock,
prioritize owners with live TCP sockets and release the lock before formatting.
Counters distinguish peek from consumption, survive socket closure and reject
old-generation activity after VM reuse. Queue bytes can be discarded on close;
received minus read bytes is not a conservation invariant.

Allocator diagnostics count first-fit calls, candidate visits, the largest scan
and failures under the existing heap lock. Snapshots never walk or initialize
the heap. Counters reset on heap installation or restore-index rebuild, so raw
observations must not be joined into invented rates across resets. Selection,
provenance, tags, heap-image format and the shared 4 KiB metadata bound remain
unchanged.

Focused native-source harnesses pass 14 allocator regressions, 13 counter tests,
four watermark tests, seven network-progress tests, 13 receive/peek tests,
11 pause/backpressure tests, four connect/close tests and seven shutdown tests.
The privacy-filtered reporter/gate suite passes 27 checks, including busy and
missing snapshots, counter resets and generation separation.

## Native integration and a confirmed HTTP transfer defect

The isolated release kernel runs the unchanged allocation probe successfully.
Its first control completes in 56 ms; a later replay alongside additional native
console/SQLx activity completes in 90 ms. These are single millisecond-resolution
runs, not a statistical claim about counter overhead or physical turn speed.

The ordinary, unprivileged SQLx probe also passes natively: typed values, nested
and disconnect rollback, 16 concurrent updates and all six Codex migration sets.
Readback binds its package to `6ef7681a` and the SQLite service to `08a5b1ef`.
The full hashes, the synthetic PASS report and capture identity are in
`bld/codex-performance-20261016/native-before-http-fix.json` and
`native-sqlx-files.json`. This resolves the previous pass's native SQLx setup
validation limit. It does not eliminate the separately observed DNS failures.

Reading those packages exposed a controlled transport difference: a GET of the
same 2,110,911-byte file receives only 1,236,408 bytes with `Connection: close`,
but returns the complete expected SHA-256 with keep-alive. The server's
`TcpSent` event acknowledges admission into the send buffer, not peer delivery.
The generic HTTP response-completion path incorrectly issued immediate `Close`,
discarding that buffered tail. It now issues the existing `FinishTcp` command
to drain data and FIN. Abort/error Close and keep-alive behavior are preserved.
Ten listener/state tests and 15 combined server/queue/flush/finish regressions
pass. This is a confirmed outbound HTTP defect, not evidence for the separate
MCP inbound-body stall.

The final native kernel repeats the same large-file comparison successfully:
three Connection-close transfers and one keep-alive control each return all
2,110,911 bytes with SHA-256 `08a5b1ef...`. The measured transfer times are
182/181/186/188 ms. Exact results and capture/kernel identities are in
`bld/codex-performance-20261016/native-http-finish.json`.

## Packaging reproducibility

The native app build exposed an independent staging bug: copying only a member
crate omitted its owning workspace's dependency lock. Fresh resolution then
rejected an already-locked dependency that had been yanked. The SDK now seeds a
private copy of the original owning lock before any staged Cargo resolution.
It replaces staged symlinks safely and allows adaptation without updating the
upstream lock. Ten regressions cover real offline locked metadata, restaging,
stale member locks and symlink isolation. No dependency upgrade was needed.

The corrected Codex package is published as `12a8514e` and read back from the
authorized Apps share. Its full digest and unchanged upstream lock digest are
in `bld/codex-performance-20261016/published-apps.json`.

The final kernel bundle is `bld/codex-session-progress-final/TrueOS.lz4`:
runtime ELF SHA-256
`e1d060e96c9e90a30892280407d24b805f9881ed30edb2198d81d40583260800`,
bundle SHA-256
`d401dd9775900bf8fab609c19fa831a782e11c3857a147987af08c93248b609e`.
Both isolated VMs were stopped after capture. The existing physical session
was not changed, and the full model scenario has not been retimed with this
new app/kernel pair. The original MCP startup stall remains unresolved; the
new counters are intended to distinguish the competing causes on that workload.
