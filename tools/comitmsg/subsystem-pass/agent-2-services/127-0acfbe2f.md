# 127 — `0acfbe2f8c996e070ca94445790f97ecb935829b` — 2026-06-21

Original message: `ok`

Hindsight subject: Remove the superseded API tree before the virtual-service rebuild

Body: The commit deleted the old `api/src/{clock,globalog,hid,lib,logl,platform,std_abi,tyche,ui2,vfs,vgfx,vgfx_hosted,vnet,vshell}.rs` surface and reduced `apps.json`, `apps/hello_world/src/main.rs`, and `src/main.rs` to the post-consolidation path. This is deliberately destructive cleanup following catalog pruning in 126; no kernel counterpart or new Blueprint service is evidenced, but it clears the boundary that 128 reconstructs.

