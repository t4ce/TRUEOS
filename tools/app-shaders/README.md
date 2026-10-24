# App-owned shaders

Kernel UI4, font, LZ4, Spirit and NV12 shaders remain embedded. Optional native
rendering payloads travel with their app's downloaded Blueprint:

| Owner | Packages |
|---|---|
| `voxy/shaders/trueos` | Voxy world, figure and cloud stages |
| `TRUEOS-Picasso/shaders/trueos` | Shared forward package, retained-transform compute, PBR, textured VS, adjacency and Maxpix |
| `Cubes/shaders/trueos` | Four-stage cube patch pipeline |

Cubes depends on Picasso, so its Blueprint carries both bundles. Other Picasso
clients receive the same shared stages without depending on Cubes. Each owner
has individual `.apshader` publications, a manifest with hashes, and one
`app-shaders.bundle` included by `src/app_shader_packages.rs`.

The Blueprint packer preserves `.trueos.shaders` as **non-ALLOC** metadata. The
kernel loader validates its packages before relocating or executing the guest,
inside the host allocation domain. Only the SHA-256 whitelist, package sizes and
existing GPU ABI/pipeline metadata remain in the kernel. Missing or changed
payloads have no embedded executable fallback. Their rendering request fails
until the app supplies the matching package. The fixed ten-slot registry keeps
one immutable, aligned copy per package for the boot lifetime, shared across
app instances; concurrent/repeated registration does not grow it.

`APSHDR1\0` packages contain the ID, bounded payload length, stage count and
`(kind, length, data)` records. Kind 0 is opaque data, kind 1 is little-endian
u32 ISA. SHA-256 authenticates the complete package, including stage layout.
These are pinned content hashes, not public-key signatures.

Check local publications and the packaged boundary:

```sh
python3 tools/app-shaders/package.py --kernel-elf bld/TRUEOS.elf \
  --blueprint ../TRUEOS-Blueprints/dist/cubes.bp \
  --blueprint ../TRUEOS-Blueprints/dist/voxy.bp \
  --blueprint ../TRUEOS-Blueprints/dist/trueos-picasso-example.bp
```

The normal linked-kernel check runs the no-app-payload check too. A shader bake
must update its owning app publication and reviewed kernel trust/ABI metadata;
it must not regenerate embedded ISA arrays into kernel pipeline metadata.

The host tests exercise production ELF-section intake, corruption rejection,
bounds, repeated registration, storage alignment, and every migrated pipeline's
stage sizes against its existing metadata:

```sh
cd /tmp
cargo +nightly-2026-07-10 test --offline \
  --manifest-path /home/t4ce/Repos/TRUEOS/tools/app-shaders/tests/Cargo.toml \
  --target x86_64-unknown-linux-gnu --target-dir /tmp/trueos-app-shader-test-target
```

Build app Blueprints with `TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp <app>`
when verifying local changes without publishing packages.
