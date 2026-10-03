# Virgl UI4 smoke test

The emulator backend presents ordinary UI4 CPU frames, immediate text stamps,
solid quads and uploaded sprites through virgl. It reuses the existing frame
broker, font tessellator and input routing. The host GPU paints and composites;
there is no Mesa guest port or software rasterizer.

Build without deploying to the physical rig or publishing a release:

```sh
make iso START_BAREMETAL_LOG=0 PUBLISH_RELEASE_SMB=0 RELEASE_BUMP_CNT=0
```

Boot, open the graphical shell, check its startup message and capture the GL
output:

```sh
python3 tools/qemu/verify-virgl.py \
  --command '§virgl' --command shell --settle 5 \
  --expect-log 'shell: local shell2 session online'
```

The verifier uses the existing runner with EGL headless rendering, a temporary
disk snapshot and private Unix QMP/VNC sockets. It saves `serial.log`,
`qemu.log`, `shell.log` and `scanout.png` under `bld/emulator-logs/virgl-verify`,
rejects uniform scanout images and reported renderer failures, and stops its
own VM. A nonuniform image is only a smoke check; inspect the capture for the
requested content. Python Pillow is required for capture. QEMU needs virgl and
EGL support, KVM and a working host render device. This machine reaches virgl
readiness in about 10 seconds, including firmware boot.

`--output PATH` keeps separate runs. `--keep-running` retains the VM and records
its PID and QMP socket; stop it before reusing the runner's forwarded ports.
`QEMU_UEFI_FIRMWARE` can select a combined OVMF image suitable for `-bios`.

To verify the embedded Tokio platform probe in an already built ISO:

```sh
python3 tools/qemu/verify-tokio-platform.py \
  --iso bld/trueos.iso --output bld/emulator-logs/tokio-platform-run1 --timeout 90
```

The output directory must be new. This verifier uses the existing EGL headless
runner, a disk snapshot and private loopback forwards. Guest outbound network
access is disabled. It submits the embedded `tokio_mrt` name through legacy
Shell2 on guest TCP 4245 and records the separate Shell3 TCP 22 banner when
available; Shell3 command execution is not wired yet. It requires the std,
scoped-thread, Tokio multithread and native-lane coverage records and the final
probe PASS. A probe FAIL, the worker-spawn panic or a deadline expires the run
with an error. There is no GPU screenshot or readiness gate.

`serial.log`, `shell.log`, `shell3.log`, `qemu.log`, `qemu-debug.log`,
`qmp.jsonl`, `run.json` and `result.json` preserve the evidence, arguments,
ports and outcome. The verifier always stops its own VM. `--timeout` limits
boot and probe execution together and cannot exceed 90 seconds. The runner's
optional `QEMU_NETDEV_USER` and `QEMU_DEBUG_LOG` overrides provide this
isolation; normal interactive defaults are unchanged.

For an interactive window, use the normal runner with `QEMU_UEFI_FIRMWARE` set:

```sh
tools/qemu/run.sh iso -snapshot
```

It defaults to SDL with GL and a USB keyboard and boot mouse. `QEMU_DISPLAY`
and `QEMU_GPU` override the display and GPU arguments. PS/2 emulation is disabled
so events reach the supported USB devices. The virtio hardware cursor is explicitly
hidden. UI4 software cursors, selection outlines, dock guides and menus use
the existing slot-4 (fifth-plane) builder and render above all window textures.

The first milestone covers the graphical Shell2 terminal and image viewer,
window positioning, opacity and broker transitions. Retained font scenes,
font backbuffers, Picasso/Cubes shaders and compute are
still outside this backend's supported subset. GPU completion is fenced;
inline vertex uploads within one submission use disjoint ranges because virgl
performs those writes without synchronization.

Check software cursor motion, selection while held and erasure on release:

```sh
python3 tools/qemu/verify-virgl.py --interaction-smoke
```

This check sends USB mouse events to an idle desktop and compares captured
pixels. It saves `cursor.png`, `selection-held.png` and
`selection-released.png` alongside the boot capture.
