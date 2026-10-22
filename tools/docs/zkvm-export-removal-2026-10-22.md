# Legacy zkVM-shaped export removal

Removed `sys_alloc_words`, `sys_getenv`, `sys_argc`, `sys_argv`, and `sys_sha_compress` from the kernel implementations and Blueprint loader. Removed the two exclusive word-buffer text-copy helpers. These interfaces had no app imports or kernel callers beyond registration; apparent dependency references came from Rust std's inactive zkVM argument/environment backend.

Retired names explicitly fail prebind and direct loader resolution before the unresolved-import fallback. The standard allocation, argument/environment and real SHA paths remain. `sys_alloc_aligned`, `sys_rand`, `sys_read`, and `sys_write` remain. No native CABI declarations or contract lock changes are required for these `sys_*` removals. The archived Rust toolchain's other-target std sources remain intact.

Validation results are recorded in the adjacent JSON. The regression harness covers all 38 retired imports, normal import behavior and hash-independent compiler runtime aliases. No runtime deployment is included.
