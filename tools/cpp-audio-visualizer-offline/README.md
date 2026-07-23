# C++ audio visualizer offline replay

This host-only lane loads the production
`cpp_audio_visualizer_rgba8.spv` with `clCreateProgramWithIL`, sends one
synthetic snapshot through the exact two-binding ABI, renders the real
horizontal-pair walker at 2560x1440, and writes an RGBA PNG. It also reports
five profiled kernel dispatches after one warm-up launch.

When the host has no OpenCL GPU (for example, a container without `/dev/dri`),
the tool writes an explicitly labeled scalar CPU reference of the same visual
equations. That fallback is useful for composition review but is never
reported as hardware replay or performance evidence.

From the repository root:

```sh
make -C tools/cpp-audio-visualizer-offline render
```

The program prefers an Intel GPU OpenCL device. It carries its small OpenCL
declaration subset, so it needs only the OpenCL ICD loader and libpng at build
time.
