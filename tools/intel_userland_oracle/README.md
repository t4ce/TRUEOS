# intel_userland_oracle

Tiny Linux-host oracle for the TRUEOS Intel EU bring-up endgame.

This is deliberately not an API-porting layer. It runs one minimal Vulkan
compute shader on the host UHD 770 and records the ordered userspace-to-i915
ritual around it: DRM node opens, GEM/context ioctls, mmap offsets, execbuffer
object lists, and mapped BO snapshots where userspace exposes them.

Run:

```bash
tools/intel_userland_oracle/run_oracle.sh
```

The main artifact is:

```text
.codex_tmp/intel_userland_oracle/latest/log.txt
```

Binary snapshots are written under:

```text
.codex_tmp/intel_userland_oracle/latest/dumps/
```

Useful knobs:

```bash
TRUEOS_ORACLE_VK_DEVICE_ID=0xA780
MESA_VK_DEVICE_SELECT=8086:a780
TRUEOS_ORACLE_MAX_DUMP_BYTES=1048576
TRUEOS_ORACLE_LOG_DIR=.codex_tmp/intel_userland_oracle/run-001
```
