# Codex local time on TRUEOS, 2026-10-15

The screenshot compares a Codex completion at 10:05 with Shell3 at 12:05.
Shell3's `TitleTime::current` converts the UTC clock through `locale` before
formatting its title. Codex's completion footer instead uses Chrono 0.4.43
`DateTime::with_timezone(&Local)`.

TRUEOS previously exported `TZ=Europe/Berlin`. Chrono's Unix backend resolves
that name through zoneinfo files, then falls back to UTC when resolution fails.
This is a different conversion path from Shell3, and does not call the libc
`localtime_r` adapter. The latter independently returned UTC fields for all
zones. Changing the realtime clock would break UTC timestamps and cannot repair
these civil-time contracts.

## Repair

Both Blueprint environment construction and the locale environment fallback now
export portable POSIX `TZ` rules for the zones TRUEOS already supports:

| Configured zone | TZ | TRUEOS_TIMEZONE |
| --- | --- | --- |
| UTC | UTC0 | UTC |
| Europe/Berlin | CET-1CEST,M3.5.0/2,M10.5.0/3 | Europe/Berlin |

The IANA identity remains available separately. Berlin's rule matches the
kernel's existing modern EU daylight-saving policy. This does not add historical
timezone data or support for further zones.

`localtime_r` now applies the configured offset and supplies the corresponding
`tm_gmtoff`, `tm_isdst` and null-terminated zone abbreviation. Date fields account
for local midnight/year/leap-day rollover. Unrepresentable `tm_year` values return
EOVERFLOW without writing the result. Private locale date arithmetic uses i64
years so valid C `tm_year` values are not truncated through an i32 civil year.
The existing unsigned locale policy is used at epoch zero for pre-epoch inputs;
historical timezone correctness is not established by this repair.

`time`, `gettimeofday` and monotonic clocks retain their UTC/elapsed semantics.
Codex's prompt, selected model and tool routing are unchanged.

## Verification

`python3 tools/testpy/test_timezone_export.py` passes three production-source
tests: actual Blueprint exports, zone identity/roundtrip, winter/summer, both
2026 DST boundaries, and far-future year arithmetic. A host POSIX consumer also
matches all six representative instants for both exported rules.

`python3 tools/testpy/test_localtime_abi.py` passes six production-source ABI
tests covering the above civil fields, local date rollover, UTC and negative
epoch conversion, null arguments, EOVERFLOW, and unchanged realtime timestamps.
Only the clock source and ABI pointer translation are mocked.

An independent executable using the actual Chrono 0.4.43 registry crate passes
both exported rules, the six instants, the spring missing time and autumn
ambiguous time. File-access traces show no successful timezone-file reads:
Chrono attempts zoneinfo lookup, then parses the POSIX rule itself. These are
Linux backend checks, not native TRUEOS execution. The temporary harness and
traces are at `/tmp/chrono-local-proof-20261015/`.

An independent oracle also compared the production libc conversion with Python
zoneinfo for 288 instants across 2020–2035, including both DST transitions and
date rollovers. All calendar fields, offsets, DST flags and names matched.

## Boot and Code Mode evidence

The baremetal capture starting 12:07:10 +02 is a fresh boot. Its reset receipt
reports verified selected runtime ELF `331ce77b…`; the local published Codex
package matches `37f6eb1a…`. Neither the earlier screenshot's session nor the
new boot logs identify a loaded Codex package hash, so they cannot attest the
package executing in that screenshot.

The message `TRUEOS does not support spawning the standalone code-mode host
process` remains in the current Codex platform capability check. The previous
change made this failure explicit before an unsupported spawn; it did not port
the host. The tested Luna configuration requires Code Mode, so the automatic
`gri` prompt still stops before native Shell3 execution. This message alone is
not evidence of an old boot, and the timezone repair does not implement that
tool runtime.

The timezone image is built separately under `bld/timezone-fix/`, with physical
deployment/reset disabled. A boot using that kernel is required for this change.
The release build and ISO-member verification passed. Build log:
`/tmp/trueos-timezone-fix-build.log`.

```text
runtime ELF: e4edf339c9be70562b258b68a371168395e7488a599415c146e2675c247dc5ae
ISO:         4c338eb87ddd117dcecf6eab281544f42757bfdf4165ce7afbff98151f6f1380
TrueOS.lz4:  dd0e78f50f9aa06a93b1c89539176b0a52c25c5ed40a6ef5858445ce35011521
```
