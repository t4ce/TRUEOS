# Freestanding CPU kernel twins

This directory contains explicit CPU entries for algorithms that also exist as
TRUEOS GPU kernels. They are not Intel Zebins and they are not submitted through
RCS/GuC. Each source owns a complete CPU dispatch and exports an ordinary,
unmangled AAPCS64 function that another bare-metal kernel can link and call.

The first reference entry is:

```c
trueos_arm_copy_rect_rgba8(...);
```

It preserves the GPU copy contract: linear RGBA8 buffers, byte pitches,
independent source/destination origins, odd widths, and untouched padding. The
implementation retains the GPU kernel's two-adjacent-pixels work-item shape,
but performs the work-item iteration explicitly on the CPU.

Build and verify the generic ARMv8-A object from the repository root:

```sh
make aarch64-kernel-copy
make aarch64-kernel-verify
```

The output is
`bld/aarch64-kernel-artifacts/copy_rect_rgba8.o` plus a compiler-free audit
manifest. The object is freestanding and intentionally rejected if it contains
an unresolved runtime symbol.

The AArch64 backend does not alter or emulate the OpenCL execution model.
Additional GPU algorithms need a small CPU entry wrapper that owns their
dispatch loops and exports a declaration from `include/trueos_arm_kernels.h`.
