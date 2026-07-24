# TRUEOS AArch64 CPU-kernel bakery

This is a second artifact backend beside the Intel GPU bakery:

```text
freestanding C++ CPU entry
  -> Clang --target=aarch64-none-elf
  -> little-endian ELF64 AArch64 relocatable object
  -> symbol/runtime audit
  -> reproducibility check
  -> JSON provenance manifest
```

It exists for kernel ports that want native ARM implementations of selected
TRUEOS compute algorithms. It does not translate Intel Zebin into ARM code and
does not claim that an ARM CPU implements an OpenCL GPU execution model.

The default profile emits generic ARMv8-A code using the AAPCS64 C ABI. Sources
are freestanding C++20 with exceptions, RTTI, stack protectors, unwind tables,
outlined atomics, libc builtins, PIC, and the C++ standard library disabled.
The resulting object must have:

- ELF class 64, little-endian, `ET_REL`, `EM_AARCH64`;
- every requested entry as one global C function with a non-empty code range;
- no undefined global or weak symbols;
- byte-identical output across two independent output directories before
  publication.

Compile the reference copy kernel:

```sh
make aarch64-kernel-copy
```

Use a particular Clang or publication directory with:

```sh
ARM_CLANG=/path/to/clang \
ARM_KERNEL_PUBLISH_DIR=/path/to/artifacts \
  tools/aarch64-kernel-bakery/bake_copy_rect.sh
```

The compiler executable may itself be x86-64 or AArch64. Its host architecture
does not change the `aarch64-none-elf` output target.

To add another entry, provide an ordinary freestanding `.cpp` source and run:

```sh
python3 -B tools/aarch64-kernel-bakery/bake.py \
  --source path/to/kernel.cpp \
  --artifact-name kernel \
  --expect-entry exported_c_symbol \
  --publish-dir bld/aarch64-kernel-artifacts \
  --repro-check
```

Verification needs only Python:

```sh
python3 -B tools/aarch64-kernel-bakery/verify.py \
  --artifact-dir bld/aarch64-kernel-artifacts
```

The default profile permits ordinary ARMv8-A FP/Advanced SIMD code. A kernel
that calls these entries must enable and preserve that architectural state, or
use a stricter project-specific profile.
