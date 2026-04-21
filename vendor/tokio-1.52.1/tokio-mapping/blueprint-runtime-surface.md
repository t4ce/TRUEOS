# Blueprint Runtime Surface Direction

The current Tokio bring-up changes the right abstraction boundary for blueprints.

Now that the current-thread Tokio runtime is proven alive on `target_os="zkvm"`, the long-term hull and blueprint story should be:

- Tokio provides the runtime and async execution model
- TRUEOS provides a narrow application-facing capability surface
- that surface stays explicit and intentionally small
- low-level filesystem and direct vlayer networking stop being the default blueprint API

## Why Reconsider The Old Hull Story

The older wrapper surface grew around direct low-level capabilities:

- `vfs`
- `vnet`
- direct host/kernel plumbing

That was useful while the runtime story was still unsettled.

Now the runtime story is clearer.

Tokio is becoming the general execution substrate, and TRUEOS can concentrate on the capabilities that are actually product-facing and blueprint-useful.

## Proposed Split

### Tokio-owned side

This should become the default place for:

- task scheduling
- timers
- coordination primitives
- async composition
- runtime structure for higher-level services

### Clean TRUEOS side

This should be the curated blueprint API surface:

- `runtime`
- `vsys`
- `vclock`
- `venv`
- `vfetch`
- `vgfx`
- `vinput`
- `ui2`

These are the pieces that still make sense as explicit platform affordances even if the app runtime itself is mostly Tokio.

## What Should Not Be The Default Story

For new blueprint-facing APIs, avoid making these the normal first-layer surface:

- direct `vfs`
- direct `vnet`
- ad hoc host/kern bridging that bypasses the runtime model

This does not require deleting them immediately. It means they stop being the default narrative and stop being the recommended surface for new blueprint work.

## Immediate Codebase Consequence

The `trueos` blueprint wrapper crate now exposes:

- `trueos::clean`
- `trueos::clean::prelude`
- `trueos::blueprint`
- `trueos::blueprint!`

That curated surface is the intended landing zone for future hull/run API cleanup and for any eventual Tokio-backed blueprint runtime wrapper.

The existing broad `trueos::prelude` remains for compatibility, but it should be treated as a wider transitional surface rather than the final architectural target.

The `hello_world` sample is the first app migrated to this newer entry shape.

For the concrete ownership split, see `docs/tokio-mapping/capability-matrix.md`.