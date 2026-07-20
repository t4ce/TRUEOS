# TRUEGA source salvage

This directory was migrated into `trueos-fpga-abi` from `t4ce/TRUEGA` at commit
`e6f7160e22f4463b3167b14a418a6d2ff2cc4322` (the same snapshot as
`TRUEGA-main.zip`). The original PCIe SerDes configuration, Gowin project, board
constraints, Analyzer probes, schematics, generator, build scripts, and flash scripts
are preserved here.

The recovered design proved the TRUEOS `22c2:1100` PCI claim, BAR0 posted writes,
BAR0 read completions, and the active-low LED debug path. The function-call redesign
keeps that debug plane and adds one fixed BAR work-package window shared with the
Rust `trueos-fpga-abi` crate. It deliberately does not add an FPGA processor, DMA
requester, TLB, command language, or runtime compiler.

The authoritative ABI layout is in `../src/lib.rs`; the corresponding hardware is in
`src/top.vhd`.

