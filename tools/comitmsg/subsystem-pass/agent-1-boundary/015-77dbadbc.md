# 015 — `77dbadbc7ed792b85cf418daac0dd063fa307952` — 2026-04-21

Original message: `tokio runtime unification`

## Establish shared Tokio, system, and UI2 Blueprint layers

The root package adds the `triangle` example and `trueos` dependency; `trueos/src/ui2.rs`, `vgfx.rs`, and `vsys.rs` add `OwnedWindow`, `SurfaceWindow`, `WindowId`, RGB triangle rendering, texture/window wrappers, logging, and polling, with matching declarations in `trueos-sys/src/vcabi.rs`. The triangle is the first concrete consumer of this shared surface, following 014’s package reset and probing kernel `126c7167`’s Tokio integration and VMX/UI lane.

