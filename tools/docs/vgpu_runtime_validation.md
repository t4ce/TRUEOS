# TRUEOS vGPU migration runtime validation

The vGPU layer is the stable boundary below future WebGPU/OpenCL runtimes. The
current Intel path is:

```text
Intel PCI device
  -> GuC firmware / ADS / CTB transport
  -> IntelGucScheduler (physical context tokens)
  -> mediated VirtualGpuDevice broker
  -> kernel render/font and GPGPU privileged devices
  -> trueos-v::vgpu opaque control ABI (host C ABI or Hull vmcall)
```

The external ABI intentionally has no shader, pipeline, bind-group, raw command
stream, MMIO, physical-address, PPGTT-entry, or GuC-context-ID surface yet.
Buffer bulk transfers use bounded handle-relative reads/writes. Hull routes
those chunks through its shared vmcall payload page.

## Boot prerequisite

Boot on the validated UHD 770 system and confirm this line contains all ones:

```text
intel/guc: admission accepted=1 firmware_ready=1 ctb_ready=1 physical_gpu_registered=1
```

There is still no legacy ELSP fallback. A failed GuC admission leaves display
bring-up alive but the vGPU tests correctly report no ready physical device.

## Immediate shell sequence

Run:

```text
vgpu status
vgpu test broker
vgpu test abi
vgpu test guc
vgpu test compute
vgpu test font
vgpu status
```

Or run the same checks, including the visible font stamp, in one command:

```text
vgpu test all
```

Each test ends in `pass=1` on success.

### `vgpu test broker`

Expected fields are all `1`:

```text
opened=1 separate_gpuvms=1 buffer=1 isolation=1 quota=1 timeline=1 device_loss=1 stale=1 cleanup=1
```

This creates two tenant devices, proves distinct PML4 roots, maps and transfers
a buffer, rejects cross-principal handles, rejects an over-quota allocation,
checks monotonic virtual timeline points, simulates device loss, rejects stale
handles, and tears both devices down.

### `vgpu test abi`

Expected:

```text
vgpu abi: lifecycle=1 cleanup=1
```

This uses `trueos-v::vgpu`, not the internal broker API. It opens a projected
adapter, allocates a map-readable/map-writable buffer, round-trips
`vgpu-shared-bulk` at a nonzero offset, creates a compute queue, submits two
control-path timeline operations, waits for value 2, and destroys every handle.

### `vgpu test guc`

Expected: `ready=1`, `guc=1`, capacity at least 2, and `failures=0`. Before a
real engine consumer runs, registered/enabled may be zero. Afterwards the
scheduler should show the render/font and/or GPGPU physical contexts.

### `vgpu test compute`

This executes the existing fill-rectangle worklist kernel through the GPGPU
privileged vGPU. Expected:

```text
dispatch=1 timeline=1 submitted=<n>-><n+1> completed=<n+1>
```

The physical GuC admission serial is also printed. This proves the virtual
timeline is retired by the existing GPU result marker rather than CTB
acceptance alone.

### `vgpu test font`

This deliberately executes the important regression anchor using the same
configuration as the original command: default font, 100 percent, legacy blue,
and the literal text `hello`. Expected:

```text
rendered=1 timeline=1 submitted=<n>-><n+1> completed=<n+1> error=none
```

For a direct behavior check of the production buffer path, run:

```text
cpp font stamp "hello"
```

Its asynchronous result should contain `cpp font stamp complete`, `ok=1`, and
an owned RGBA8 `handle`. Release it after inspection with
`cpp font release <handle>`.

## What `vgpu status` proves

The first line reports physical adapter readiness and GuC scheduler totals.
Following lines report only broker-owned data: opaque handles, principal,
filtered capabilities, epoch/loss state, quota use, and resource counts. The
two privileged timelines are printed separately as `render/font` and `gpgpu`.

After the compute and font tests, both timelines should have
`submitted == completed`, both should have a nonzero physical serial, and the
scheduler should have two registered contexts. A timeout or marker mismatch
increments the applicable virtual timeline failure count.

## Hull transport smoke test

A Hull consumer can perform the same ABI lifecycle using only the public
facade:

```rust
let device = v::vgpu::Device::open(v::vgpu::Capabilities::DEFAULT)?;
let buffer = device.create_buffer(
    4096,
    v::vgpu::BUFFER_USAGE_MAP_READ | v::vgpu::BUFFER_USAGE_MAP_WRITE,
)?;
device.write_buffer(buffer, 0, b"hello")?;
let queue = device.create_queue(v::vgpu::QueueClass::Compute)?;
let point = device.submit_control_nop(queue)?;
device.wait(queue, point.value)?;
device.destroy_queue(queue)?;
device.destroy_buffer(buffer)?;
device.close()?;
```

In a Hull execution context, the C ABI automatically selects vmcall transport;
the application never selects or observes the transport itself.
