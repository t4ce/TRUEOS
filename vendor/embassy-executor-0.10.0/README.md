# embassy-executor

TRUEOS-local vendored copy of Embassy's async task executor.

This copy is intentionally pruned to the executor surface TRUEOS uses on x86:

- `platform-spin`
- `executor-thread`
- the task macro support
- the raw executor core
- the timer-queue hook used by `embassy-time`

Chip-specific platform wrappers from upstream Embassy are not present here.
TRUEOS provides wake routing through `__trueos_embassy_pender` from the spin
platform wrapper.
