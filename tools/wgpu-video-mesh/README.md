# WGPU Video Mesh

A small, independent Ubuntu desktop demo that loops `x31_head_movie.mp4` through FFmpeg and
uploads the decoded frames to a WGPU texture. The texture is rendered on a large UV sphere by
default and can be switched among ten normalized Blender-style meshes at runtime. egui provides playback, transform, color,
exposure, lighting, and saturation controls, plus a native picker for loading another video.

The copied video asset lives at `assets/x31_head_movie.mp4`. Audio is intentionally ignored: this
demo is focused on video-as-a-GPU-texture playback.

## Ubuntu prerequisites

```sh
sudo apt update
sudo apt install build-essential pkg-config ffmpeg libvulkan1 mesa-vulkan-drivers \
  libx11-dev libxi-dev libxkbcommon-dev libwayland-dev xdg-desktop-portal-gtk
```

Use your normal NVIDIA/AMD Vulkan driver instead of `mesa-vulkan-drivers` when applicable.
The included `rust-toolchain.toml` selects Rust 1.96.1 so this host demo does not inherit TRUEOS's
nightly bare-metal standard-library build.

## Build and run

```sh
cd tools/wgpu-video-mesh
cargo run --release
```

Pass another video as the optional first argument:

```sh
cargo run --release -- /path/to/video.mp4
```

FFmpeg performs decode on a background thread, loops the input indefinitely, scales it to a
960×540 RGBA stream, and keeps only the newest pending frame so a slow or minimized window does
not accumulate latency. WGPU uploads each new frame without recreating the texture.

Controls:

- Select Plane, Cube, Circle, UV Sphere, Icosphere, Cylinder, Cone, Torus, Grid, or Suzanne from
  the **Geometry** dropdown. Every mesh has a maximum vertex radius of `1.0`.
- Use **Choose video…** to replace the looping texture without restarting the demo.
- Drag in the scene to orbit and use the mouse wheel to zoom.
- Press Space or use the button to pause/resume texture updates.
- Pick an object tint and background color.
- Adjust spin, size, camera distance, exposure, 3D lighting, and saturation live.
