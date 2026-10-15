# Scoped thread fanout contract

Keeps eight background threads live while 24 scoped threads start, then joins
all 32. Three rounds check reuse after retirement. Each worker checks its Rust
thread-local value; native execution also checks distinct, stable WLS slots
inside the existing 4096-slot ABI. Logs contain only counts and timings.

This exercises the `Scope::spawn` API named by the Codex panic and crosses the
former 20-live-thread realm partition without increasing any capacity. The
unchanged 64-entry pthread registry and global 256-thread scheduler remain
separate admission bounds.

Host baseline:

```sh
cargo run --manifest-path tools/probes/thread_fanout/Cargo.toml
```

The sibling SDK's `probes.json` registers this standalone directory. Build and
publish from `TRUEOS-Blueprints` with:

```sh
TRUEOS_BLUEPRINT_PROBES_PUBLISH_URI='smb://t4ce@pdjb/home-share/TRUEOS_SITE/apps' cargo bpp thread_fanout
```

Native launch is `§§thread_fanout`. It is an explicit probe, not part of
application startup or the Apps registry.

The unchanged native package `8c9c149e` reproduced errno 11 on the old kernel:
20 live threads were admitted, then the 21st `Scope::spawn` panicked. With the
shared WLS pool, all three 32-thread rounds passed in 135/79/62 ms (280 ms total),
including native slot uniqueness/stability and Rust TLS isolation. See the
[before](validation/native-before.json) and [after](validation/native-after.json)
evidence. This reproduces the admission defect; it does not repeat a full Codex
model turn or prove which higher-level scoped caller triggered the original
crash.
