# Tiger Lake 0x9A49 as ADL-S compatibility probe

This branch intentionally aliases the physical Intel Tiger Lake-LP GT2 GPU
`8086:9A49` to TRUEOS's currently admitted Alder Lake-S GT1 identity
`8086:4680`, including revision `0x0c` at the exported PCI boundary.

The purpose is to answer one hardware question before introducing a second
shader/artifact matrix: can the existing Xe-LP display, GuC, render, Spirit,
and ADL-S IGC/Zebin path execute successfully on the i7-1185G7 test laptop?

This is an experiment, not a Tiger Lake support claim. The physical PCI dword
identity remains readable through the raw `config_read_u32(..., 0x00)` path for
diagnostics. If the experiment fails, the first failing hardware checkpoint
should define the real platform split rather than a pre-emptive software seal.
