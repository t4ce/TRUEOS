# TRUEOS ShaderToy C++ preview

This is a small native Ubuntu paste/run tool for ShaderToy Image passes. The
left pane is a source editor, the button (or `Ctrl+Enter`) runs the repository's
C++-for-OpenCL bakery, and the right pane presents the generated kernel at
640x360 and 60 Hz.

It is deliberately the same artifact path used by the Spirit previews, not an
OpenGL/WebGL fallback:

```text
pasted ShaderToy GLSL
  -> explicit-uniform C++ for OpenCL session source
  -> Clang spir64 LLVM bitcode
  -> llvm-spirv OpenCL Kernel SPIR-V
  -> ocloc/IGC Intel Zebin + TRUEOS ABI audit
  -> Intel OpenCL loads the sibling SPIR-V for the Ubuntu preview
```

The pasted source and all generated session files stay under ignored `bld/`.
The tool does not download or check in the linked shader.

## Ubuntu setup

The host UI follows the existing Spirit tools and uses X11/Xwayland. Install
the small host build dependency and OpenCL loader once:

```sh
sudo apt install build-essential libx11-dev ocl-icd-libopencl1
```

Install the Intel compiler and OpenCL runtime packages locally under `bld/`
(no root and no host package changes):

```sh
make -C tools/shadertoy-cpp-offline toolchain
```

You may instead provide existing tools through `CLANG`, `LLVM_SPIRV`, and
`OCLOC`. Then launch the editor from the repository root:

```sh
make -C tools/shadertoy-cpp-offline run
```

An optional source path preloads the editor:

```sh
make -C tools/shadertoy-cpp-offline
tools/shadertoy-cpp-offline/run.sh /path/to/image-pass.glsl
```

Controls are `Ctrl+V` paste, `Ctrl+A` replace all, `Ctrl+S` save the session,
`Ctrl+Enter` bake/run, and `Esc` close. Pointer input over the preview supplies
ShaderToy-compatible `iMouse` coordinates.

## Compatibility boundary

The first slice accepts a single, texture-free `mainImage` pass and supplies:

- `iResolution`, `iTime`, `iTimeDelta`, `iFrame`, and `iFrameRate`
- `iMouse`, `iDate`, and `iSampleRate`
- scalar/vector GLSL constructors and common math helpers
- `mat2` construction and vector/matrix multiplication

The adapter rejects `iChannel*`, samplers/texture calls, sound passes, and
screen-space derivatives with a direct diagnostic. Those features require
new, audited TRUEOS resource, multipass, or quad-derivative ABIs; silently
running them through WebGL would no longer test the custom stack. `mat3` and
`mat4` are also not part of this initial adapter. If a source uses a GLSL form
outside the adapter subset, the exact generated `.clcpp` and compiler error
remain in `bld/shadertoy-cpp-offline/session/` for a focused compatibility
addition.

The bakery retains the production ADL-S constraints, including SIMD16 and zero
scratch/SLM. A shader that spills is rejected because TRUEOS cannot currently
submit that artifact with the same ABI.

## Tests

```sh
make -C tools/shadertoy-cpp-offline test
```
