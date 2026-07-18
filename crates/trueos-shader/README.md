# TRUEOS Intel shader assets

This repository owns TRUEOS's handcrafted Intel GPU shader sources, EU
programs, generated binaries, validation material, and shader-baking tools.
TRUEOS consumes it as the private `crates/trueos-shader` submodule.

The embedded OpenCL GPGPU collection lives under `gpgpu/kernels`, with the
Alder Lake S artifacts under `gpgpu/kernels/artifacts/adls`. From this
repository's root, rebuild selected artifacts with:

```sh
gpgpu/bake_adls_artifacts.sh copy_rect_rgba8 fill_rect_rgba8
```

Set `OCLOC` or `IGC_ROOT` when the Intel compiler is not in the default TRUEOS
tool location. A standalone checkout can use `TRUEOS_ROOT` to point the tool at
a sibling TRUEOS worktree.
