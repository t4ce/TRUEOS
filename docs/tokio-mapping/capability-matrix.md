# Tokio And TRUEOS Capability Matrix

This is the concrete split for the new blueprint/runtime story.

The goal is not to make `trueos` disappear. The goal is to make it stop acting
like a second general runtime. Tokio should own most runtime behavior, while
TRUEOS keeps the explicit platform capabilities that matter to blueprints.

## Rule Of Thumb

- if it is runtime structure, async composition, or coordination, prefer Tokio
- if it is a product/platform capability, keep it in `trueos::clean`
- if it is a raw kernel-facing or legacy convenience surface, move it out of
  the default app story and keep it only as compatibility

## Capability Split

| Concern | Default owner | Current status on zkvm | Notes |
| --- | --- | --- | --- |
| Task scheduling | Tokio | proven | `rt`, spawn, join, `LocalSet`, `JoinSet` are already green in the probe |
| Runtime entry / app orchestration | Tokio-facing app layer above `trueos::blueprint!` | partial | `trueos::blueprint!` is the current entry boundary; later higher-level Tokio app wrappers should sit above it |
| Timers / sleeps / intervals | Tokio | proven | `tokio::time` is green on zkvm with the current vendor bridge |
| Coordination primitives | Tokio | proven | `oneshot`, `mpsc`, `watch`, `broadcast`, `notify`, `mutex`, `rwlock`, `semaphore`, `barrier` are green |
| Async composition / cancellation | Tokio | mostly implied | `select`, `timeout`, structured async flows should live here as the runtime grows |
| Async I/O traits and adapters | Tokio | proven | `io-util` and `io-std` probe successfully |
| Filesystem API shape | Tokio | compile/surface only | `tokio::fs` compiles, but runtime ops remain blocked on `spawn_blocking` carrier work |
| General network runtime shape | Tokio | blocked upstream | `tokio/net` is still blocked by `mio` and `socket2` target support |
| Blocking offload | Tokio API on TRUEOS lane substrate | not yet landed | Tokio should keep the API; TRUEOS must provide the carrier lane underneath |
| Windowing / UI shell affordances | TRUEOS clean | keep | `ui2` is exactly the kind of explicit product capability that should stay platform-owned |
| Input devices / cursor / keyboard | TRUEOS clean | keep | `vinput` remains a direct platform capability |
| App environment / launch args | TRUEOS clean | keep | `runtime` and `venv` belong here |
| Clock / wall time bridge | TRUEOS clean + Tokio bridge | keep | `vclock` remains platform-facing, Tokio uses its own bridged runtime time model |
| Fetch-style product networking | TRUEOS clean | keep, but narrow | `vfetch` can remain as a product capability without making raw networking the default app story |
| Graphics/product visuals | TRUEOS clean | keep | `vgfx` and related explicit platform affordances stay here |
| Logging / poll hooks / system glue | TRUEOS clean | keep | `vsys` is part of the small substrate, not a general runtime replacement |
| Direct filesystem calls | compatibility only | phase out from default | `vfs` should not be the normal first API for new blueprints |
| Direct vlayer networking | compatibility only | phase out from default | `vnet` should not be the normal first API for new blueprints |
| Shell-oriented plumbing | compatibility only | phase out from default | `vshell` is useful for samples and tooling, not as the main new-world app surface |
| Raw C ABI (`trueos-sys::vcabi`) | internal plumbing only | keep internal | this must remain broad, but it is not the app-facing API |

## What Stays In `trueos::clean`

This is the currently intended curated surface:

- `runtime`
- `vsys`
- `vclock`
- `venv`
- `vfetch`
- `vfetch_job`
- `vgfx`
- `vinput`
- `ui2`

These are the things that still make sense as explicit TRUEOS affordances even
when Tokio becomes the main runtime language of applications.

## What Moves Out Of The Default App Story

These do not need to be deleted immediately, but they should stop being the
recommended blueprint surface for new applications:

- `vfs`
- `vnet`
- `vshell`
- direct use of `trueos-sys::vcabi`

That is why the wrapper should evolve toward two visible layers:

- `trueos::clean` for the curated default
- an explicitly named compatibility layer for wider legacy access

## Immediate Design Consequence

If a new blueprint asks “where should this live?”, the answer should usually be:

- runtime behavior: Tokio
- platform capability: `trueos::clean`
- legacy raw access: compatibility only, not default

That keeps the runtime story simple and prevents the wrapper crate from growing
back into a second broad runtime facade.