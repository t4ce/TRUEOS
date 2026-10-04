Maxpix: native ADL-S POINTLIST tornado

The Blueprint uploads one 24-byte seed vertex and one u32 index per particle
once. Layout 10 chooses this baked VS/PS pair in the retained renderer. Each
vertex carries phase, height fraction, radial factor, and a Float3 offset.
The VS applies tornado motion from camera.jitter_frame.z (elapsed seconds),
then the GPU-authored identity instance and Picasso view-projection. The PS
writes a constant cyan color. Native point width uses TRUEOS's four-pixel
state default, as in the Potato Stamps native point path. HS/DS/GS are disabled.

This is a dedicated baked shader admission, not a general guest shader compiler.
The captured target is Intel UHD Graphics 770, 8086:4680 (Gen12). Mesa captures,
ISA, metadata and hashes are under picasso/maxpix-tornado. Compilation and host
checks do not establish bare-metal raster output; metadata marks that separately.

Rebake (needs cargo, Vulkan development headers/loader, cc, iga64):
1. Unpack https://archive.mesa3d.org/mesa-26.0.4.tar.xz.
2. Apply mesa-26.0.4-capture.patch with patch -p1 in its source root.
3. Configure/build with Meson/Ninja, LLVM/Clang 21, matching libclc and
   SPIRV-LLVM-Translator, SPIRV-Tools, libdrm, libelf, zstd, Mako and glslang:
   meson setup BUILD SOURCE -Dgallium-drivers= -Dvulkan-drivers=intel \
     -Dplatforms= -Dllvm=enabled -Dglx=disabled -Dopengl=false -Degl=disabled \
     -Dtools=drm-shim -Dbuild-tests=false -Dglvnd=disabled \
     -Dlibunwind=disabled -Dvalgrind=disabled -Dintel-rt=disabled -Dvideo-codecs=
   ninja -C BUILD src/intel/vulkan/libvulkan_intel.so \
     src/intel/tools/libintel_noop_drm_shim.so
4. MAXPIX_MESA_BUILD=/absolute/BUILD python3 tools/maxpix-bake/bake.py
5. Build the OS and apps/maxpix together; the new layout requires this kernel.

Hardware reference: Intel Tiger Lake PRM volume 9, POINTLIST rasterization and
3DSTATE_SF PointWidth/PointWidthSource. Points rasterize a square footprint;
this does not invoke tessellation or generate a guest triangle mesh.
https://cdrdv2-public.intel.com/703063/intel-gfx-prm-osrc-tgl-vol-09-render-engine.pdf
