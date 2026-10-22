# Native scheduled audio and legacy Konsole removal

Removed the native scheduled audio engine, its five CABI entry points, the three native render VMCALL handlers, and its SDK types/wrappers. VMCALL IDs 0x152–0x154 remain reserved and now receive the normal unknown-operation response. PCM playback, HDA, endpoint capabilities and the separate synth remain.

Removed `trueos_cabi_konsole_write_row`, `trueos_cabi_konsole_set_cursor`, and `trueos_cabi_konsole_end_frame`, their wrappers, and the exclusive buffered frame state/packet helpers. The still-used `begin_frame` and size interfaces remain; begin-frame still claims terminal handoff before resizing and validates host/guest responses, without allocating an unconsumed frame buffer.

Updated declarations and wrappers in both TRUEOS and TRUEOS-Blueprints. Old imports explicitly fail prebind/direct loader resolution before the unresolved-import fallback. The exact production ABI parser/digest verified the old lock, precisely eight declaration removals, and 348 surviving contracts with unchanged signatures.

The five legacy zkVM-shaped exports were left unchanged: `sys_alloc_words`, `sys_getenv`, `sys_argc`, `sys_argv`, and `sys_sha_compress`. The first allocates storage in 32-bit words, the next three read the environment and arguments, and the last is a placeholder SHA-compression interface that currently copies the supplied state rather than compressing blocks.

Validation:

- `cargo build --offline --locked --bin TRUEOS` passed with final linking.
- Blueprint API `cargo check --offline --locked --manifest-path /home/t4ce/Repos/TRUEOS-Blueprints/api/Cargo.toml --target x86_64-unknown-linux-gnu` passed.
- Three production prebind/loader tests passed, covering all 33 retired names and preserving compiler runtime alias handling.
- Three production Konsole begin-frame tests passed, covering invalid dimensions, absent console, host/guest handoff ordering, wire flags and host failure.
- Linked symbols contain neither the eight retired interfaces nor native-engine code. Live Konsole begin-frame, PCM interfaces and all five legacy sys exports remain defined.
- Fresh census of all 60 available packages found no retired imports and no scan errors.

The prior shortlist attributed about 21.9 KiB of linked kernel text to the two candidates. Whole-ELF section measurements are recorded in the adjacent JSON, but the previous ELF was not rebuilt as a controlled baseline, so their difference is not a causal size-saving measurement. No runtime boot/deployment or full app rebuild was performed.

The original shortlist is a historical pre-removal snapshot. Full removed names, ABI hashes and verification results are in `audio-konsole-removal-2026-10-22.json`.
