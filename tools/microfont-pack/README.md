# TRUEOS MicroFont packaging

The portable MicroFont payload is one uncompressed `microfont.bin` file. Its
selected Cargo features determine whether it contains bitmap masks (`hex`),
line/quad raster data (`raster`), or both. Selection happens before this wrapper;
the wrapper preserves the portable bytes exactly.

TRUEOS distributes that payload as the sole `microfont.bin` member of one
`microfont.tar.lz4` file:

```sh
python3 tools/microfont-pack/pack.py path/to/microfont.bin \
  --output bld/microfont.tar.lz4
```

The wrapper uses Python's standard library and the existing host `lz4` command.
It writes deterministic POSIX ustar metadata and an LZ4 frame with independent
blocks and the uncompressed content size. It leaves no intermediate tar file.
For `target_os = "trueos"`, MicroFont embeds only this archive. The BSP calls
`src/r/microfont.rs::init` immediately after heap installation and before
applications, displays, or secondary CPUs are initialized. It uses
`src/r/tar.rs::decode_named_single_lz4` to decode the archive once, retains the decoded
file until reboot, and installs it into MicroFont before the first text draw.
Other targets embed the uncompressed file directly.

The portable MicroFont crate does not need tar or LZ4
dependencies, an allocator, or a filesystem to read its uncompressed bundle.
