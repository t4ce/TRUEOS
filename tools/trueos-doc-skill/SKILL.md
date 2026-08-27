---
name: trueos-docs
description: Query authoritative, compact TRUEOS Shell2 knowledge when working with Shell2 modes, the § headjack/Matrix operator, Blueprint lifecycle commands, terminal UI handoff, launch scripts, or the physical/QEMU test-rig endpoints. Do not use for unrelated TRUEOS implementation details.
---

# TRUEOS docs

Use `trueos-doc` before reasoning from memory about Shell2 interaction or rig access.

- Start with `trueos-doc` or `trueos-doc context` when several Shell2 concepts are relevant.
- Use `trueos-doc topic <name>` for `headjack`, `shell2`, `tui`, `runscripts`, `rig`, or `apps`.
- Use `trueos-doc command <name>` before composing a command-mode invocation; its parameters come from the live Rust registry.
- Use `trueos-doc search <terms>` when the topic or command name is unclear.
- Treat every response as JSON. A successful response has `ok: true`; an error exits 2 with `ok: false` on stderr.

The documentation command is read-only. Its examples describe syntax; they do not authorize connecting to the rig, launching apps, resetting hardware, or performing any other external mutation.
