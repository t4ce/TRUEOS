# 116 — `b10fbfeacbb3afaf042020baacc2a60b452e86eb`

- Date: 2026-05-19
- Original message: `ok`
- Hindsight subject: Move Tetris and fd onto the consolidated UI2 surface

`api/src/ui2.rs` adds the missing window/graphics exports, `apps/crates/trueos-tetris` switches from `v::vled::Rgb8` to `trueos::ui2::Rgb8`, and `src/main.rs` gains staging/package handling for the external fd and Tetris inputs; the `apps/fd` pointer also advances. This is a concrete consumer migration after 115 from the old V path to the consolidated ABI-backed API, with kernel `9cefefeb65377ecab9b8bd066f5caa6ccdcb263b` as the evidenced V userspace ABI provider.

