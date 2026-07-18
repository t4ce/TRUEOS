# FluidX3D AOT GPU Path

TRUEOS should not grow a general OpenCL stack just to run FluidX3D on a known
Intel iGPU. The narrower target is a fixed-purpose launch backend:

- Linux/OpenCL remains the factory for compiling and validating kernels.
- TRUEOS consumes packaged Intel kernel records and submits them on known
  hardware.
- The Rust solver owns the FluidX3D host logic and calls a small kernel launcher
  boundary.

The runtime artifact is more than a binary blob. It needs the launch contract
that NEO would normally recover at runtime: target GPU, SIMD width, GRF/scratch
/ SLM requirements, binding and surface counts, cross-thread data, local size,
and argument layout. The in-tree shape for that record is
`GpuKernelBlob` in `src/intel/opencl/artifact.rs`.

Recommended backend split:

```text
fluid solver
  -> KernelLauncher
      -> Linux/OpenCL backend for compile/test/dump/compare
      -> TRUEOS/Intel-AOT backend for bare-metal launch
```

The TRUEOS backend work stays bounded to:

- Allocate and map simulation buffers into GPU-visible memory.
- Create surface-state descriptors and binding tables.
- Patch cross-thread payloads and kernel arguments.
- Emit interface descriptor, VFE/media state, and GPGPU walker commands.
- Submit the batch, wait on the fence, and flush/invalidate caches.

Freeze kernels by target and version, for example:

```text
TRUEOS-fluidx3d-rpl-uHD770-v1/
  collide_stream.simd16.bin
  boundary_bounce.simd16.bin
  voxelize.simd16.bin
  render_velocity.simd16.bin
  kernels.manifest
```

Fragile pieces to keep versioned:

- Kernel metadata must exactly match the TRUEOS launch encoder.
- The GPU generation must match the compiled binary.
- Buffer and surface bindings must be patched at the expected slots.
- Cache and fence behavior must be validated with readback proofs.
- Kernel build options must be frozen with the manifest.
