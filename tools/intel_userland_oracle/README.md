# intel_userland_oracle

Tiny Linux-host oracle for the TRUEOS Intel EU bring-up endgame.

This is deliberately not an API-porting layer. It runs one minimal Vulkan
compute shader on the host UHD 770 and records the ordered userspace-to-i915
ritual around it: DRM node opens, GEM/context ioctls, mmap offsets, execbuffer
object lists, CPU stack frames for each traced transition, proc/fd snapshots,
and mapped BO snapshots where userspace exposes them.

The runner also attempts a non-interactive `sudo` hardware lens by default. If
available, it maps the Intel MMIO BAR and samples forcewake state, RCS ring
registers, ACTHD/IPEHR, fault registers, INSTDONE, GPGPU thread counters, TDL,
ROW, SAMPLER, SC, and RCS CS GPRs during the same single run.

Run:

```bash
bash tools/intel_userland_oracle/run_oracle.sh
```

The default workload is the original sentinel shader. To run the T5-small
live-input dot probe instead:

```bash
TRUEOS_ORACLE_SHADER_SOURCE=.codex_tmp/t5_small_live4.comp \
TRUEOS_ORACLE_SHADER_NAME=t5_small_live4 \
TRUEOS_ORACLE_WORKLOAD=t5-small-live4 \
TRUEOS_ORACLE_LOG_DIR=.codex_tmp/intel_userland_oracle/t5-small-live4 \
bash tools/intel_userland_oracle/run_oracle.sh
```

The main artifact is:

```text
.codex_tmp/intel_userland_oracle/latest/log.txt
.codex_tmp/intel_userland_oracle/latest/hw_mmio_log.txt
```

Binary snapshots are written under:

```text
.codex_tmp/intel_userland_oracle/latest/dumps/
```

Useful knobs:

```bash
TRUEOS_ORACLE_VK_DEVICE_ID=0xA780
MESA_VK_DEVICE_SELECT=8086:a780
TRUEOS_ORACLE_MAX_DUMP_BYTES=0
TRUEOS_ORACLE_TRACE_STACKS=1
TRUEOS_ORACLE_STACK_DEPTH=64
TRUEOS_ORACLE_TRACE_SNAPSHOTS=1
TRUEOS_ORACLE_LOG_DIR=.codex_tmp/intel_userland_oracle/run-001
TRUEOS_ORACLE_SHADER_SOURCE=tools/intel_userland_oracle/sentinel.comp
TRUEOS_ORACLE_SHADER_NAME=sentinel
TRUEOS_ORACLE_WORKLOAD=sentinel
TRUEOS_ORACLE_HW_INTERVAL_US=200
TRUEOS_ORACLE_REQUIRE_HW=1
```

`TRUEOS_ORACLE_MAX_DUMP_BYTES=0` means full mapped-BO dumps. Set it to a byte
count only when you intentionally want a capped probe. The stack/snapshot knobs
default on because this tool is meant to be a macro lens, not a benchmark.

`TRUEOS_ORACLE_REQUIRE_HW=1` is the default. If `sudo -n` cannot map the Intel
MMIO BAR, the run aborts instead of producing a userspace-only witness. Run
`sudo -v` first, or run the script as root, when the raw register lens is
required.
