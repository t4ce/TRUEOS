# Vulkan headers (vendored)

Khronos Vulkan API headers, vendored so that `tools/helio-intel-bake/bake.py`
can compile its pipeline dumper from a clean checkout without depending on a
host `libvulkan-dev` package or on a sibling working tree.

This directory exists because the previous header location was outside every
git repository. It was recovered from a backup rather than from source control.
Keep these files tracked.

## Contents

```text
include/vulkan/     22 Khronos headers (~1.5 MB)
include/vk_video/   12 video-codec headers (~120 KB)
```

`crates/trueos-shader/xe_lp_shader_bake/simple_triangle_dump.c` includes only
`<vulkan/vulkan.h>`, but that is not sufficient on its own: `vulkan_core.h`
unconditionally includes `vk_video/vulkan_video_codec_h264std.h` and its
siblings, so `include/vk_video/` must be vendored alongside `include/vulkan/`.
Copying `vulkan/` by itself fails to compile.

The platform-specific headers (`vulkan_win32.h`, `vulkan_xcb.h`, ...) are
reached only behind `VK_USE_PLATFORM_*` guards that the dumper does not define,
so no Windows, X11, XCB, Wayland, or Fuchsia headers are needed to build it.

Verified with:

```sh
cc crates/trueos-shader/xe_lp_shader_bake/simple_triangle_dump.c \
   -o /tmp/dump_headertest \
   -I vendor/vulkan-headers/include -l:libvulkan.so.1
```

## Version and license

- `VK_HEADER_VERSION` 346 (`VK_HEADER_VERSION_COMPLETE` = Vulkan 1.4.346)
- Copyright 2015-2026 The Khronos Group Inc.
- SPDX-License-Identifier: Apache-2.0

## Runtime, not build time

Only the headers are vendored. The loader is resolved at link time from the
host as `-l:libvulkan.so.1`; `bake.py` uses the `-l:` spelling because a
runtime-only install provides `libvulkan.so.1` without the `libvulkan.so`
development symlink.

The bake also needs a working Intel Vulkan ICD at run time (`mesa-vulkan-drivers`
on Debian/Ubuntu). That is a host requirement and is deliberately not vendored.

## Search order

`vulkan_compile_flags()` in `tools/helio-intel-bake/bake.py` searches, in order:

1. `vendor/vulkan-headers/include` (this directory)
2. `/usr/include`
3. `../bak/reference/mesa/include`
4. `../blender-default-cube-toggle/lib/linux_x64/vulkan/include`

This directory is first so a bake is reproducible from a clean checkout. The
remaining entries are retained as fallbacks for machines provisioned before
these headers were vendored.

## Refreshing

Replace `include/vulkan/` wholesale from a Khronos
[Vulkan-Headers](https://github.com/KhronosGroup/Vulkan-Headers) release and
update the version recorded above. Changing the header version can change what
Mesa ANV compiles, so treat a refresh as a bake-affecting change and re-run the
artifact validators afterwards.
