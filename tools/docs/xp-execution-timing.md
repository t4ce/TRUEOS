# XP execution timing markers

These temporary markers use the existing `log_os` path at Important level.
They remain active in quiet/nolog xpapp builds. No UART path or scheduling
policy change is introduced. Install both rebuilt kernel and xpapp to obtain
all three records. The markers do not recover a stalled network or print
while a measured call never returns.

## Records

- `XPAPP EXEC TIME`: per logical thread, emitted on completion when its window
  reaches two seconds. All durations are **totals**, in microseconds.
  `prepare_us` covers coordinator register/state preparation;
  `request_us` covers sending the request until the worker accepts it;
  `native_us` covers the kernel context run/resume call;
  `reply_us` covers sending the result until the coordinator receives it.
  `context_gap_us` covers time from the prior recorded completion to the next
  preparation on this thread: providers, scheduling, other threads, and waits.
  `max_roundtrip_us` is the largest request+native+reply duration.
- `XPAPP PROVIDER TIME`: aggregate VMCALL dispatch wall time across logical
  threads, emitted on return after two seconds. Includes async waits, special
  traps, and dispatcher work; `max_id`, PID/TID and EIP identify the slowest
  dispatch. The ID is EAX at the trap; special traps may not use provider IDs.
  A scope guard records early returns/continues as well as normal completion.
- `XPAPP VMX TIME`: every 2,048 successful transient runs per physical CPU.
  `setup_cycles` measures entry preparation including VMCS construction;
  `entry_cycles` brackets the assembly wrapper (guest execution **plus**
  xstate switching and hardware VM-entry/exit);
  `finish_cycles` measures capture/cleanup through VMCLEAR.
  Convert TSC cycles to seconds using the reported `tsc_hz`.
  The marker logs after the measured interval, so its output cost is excluded
  from these three totals but remains inside the outer native wall time.

## Interpretation

Request/reply dominating implicates handoff/wakeup/coordinator availability.
Setup/finish dominating implicates the kernel transition path. Entry dominating
requires inspecting guest progress and exit reasons; it alone does not prove
x87 is slow. Provider wall time and context gaps identify work between entries.
The API uses the platform clock across carriers; kernel intervals use fenced
TSC reads on the same CPU. Clock reads and aggregation themselves have overhead.

These records have different windows. Provider time overlaps context gaps;
VMX intervals are nested in native time. Do not sum the three records.
Context gaps also overlap other threads' work. No synthetic 'guest-only' time
is obtained by subtracting unrelated windows. Kernel records aggregate by CPU,
so the owner field is the last owner, not a guarantee of a single-owner window.

Validation: `python3 tools/test_xp_execution_timing.py` exercises the real SDK
carrier implementation with queued work, delayed native execution, errors,
and abandoned requests. Hardware timing still requires a new run.
