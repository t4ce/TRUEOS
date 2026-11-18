# Intel RCS rectangle dispatch experiment

The opt-in Shell3 diagnostic tests whether merging adjacent solid rectangles
reduces RCS0 dispatch work and saves time for an identical RGBA8 result. In
an idle rig session, open ADM → `gpu`, press `m`, and choose
**test compute-dispatch**. The diagnostic uses the existing offscreen
solid-rectangle kernel. [Shell3 diagnostic](../../src/shell3/system/gpu.rs#L450),
[diagnostic implementation](../../src/intel/gpgpu/operations/rectangle_dispatch.rs)

## Workload and expected output

Initialize two 64×4 RGBA8 surfaces with the same background. Fill columns
8 through 23 on all four rows with one opaque color. The current kernel
launches a SIMD16 workgroup per rectangle descriptor, with up to sixteen
descriptors per walker. Both variants submit one batch and one walker.
[Worklist encoder](../../src/intel/gpgpu/rcs/worklists.rs#L74),
[kernel contract](../../crates/trueos-shader/gpgpu/kernels/alpha_blend_worklist_rgba8.clcpp#L4)

| Variant | Rectangles | Expected workgroups | Expected dispatched lanes | Pixels filled |
| --- | --- | ---: | ---: | ---: |
| A | Sixteen 1×4 strips | 16 | 256 | 64 |
| B | One 16×4 rectangle | 1 | 16 | 64 |

Compare every pixel in both surfaces with a CPU reference after retirement.
Include the background in the comparison so an out-of-bounds store fails
the test. Use the same formats, pitches, artifact, context, and submission
path. Swap the backing buffers assigned to A and B between repetitions.

## Counter and timing checks

1. The command admits physical `8086:4680` revision `0x0C` with a matching
   effective identity and an admitted solid-rectangle kernel. It reports
   both PCI identities and the uploaded artifact hash. The Tiger Lake
   reference describes the two halves at `0x2290` and `0x2294` as a 64-bit
   active-lane count. Mesa uses the same Gen12 counter with engine-relative
   register operands. Calibration checks the count on the rig's Alder Lake
   stepping. [Intel register reference, p. 272](https://cdrdv2-public.intel.com/703046/intel-gfx-prm-osrc-tgl-vol-02-c-command-reference-registers-part-1.pdf#page=298),
   [Mesa register addressing](https://chromium.googlesource.com/external/gitlab.freedesktop.org/mesa/mesa/+/7c634b2375af2c145c185666841364c533bafc2d/src/intel/common/mi_builder.h#133)
2. Take ordered, read-only register snapshots before and after the walker
   in the same batch. Use the Picasso register-to-memory snapshot pattern
   and an owned result mapping. Keep the full counter width and dedicate
   aligned result slots that cannot overlap timestamps or completion
   markers. Hold the scratch and descriptor ownership through readback.
   Require the existing completion marker and saved-HEAD retirement proof
   before reading results. [Snapshot pattern](../../src/intel/render/primary.rs#L2435),
   [existing worklist submission](../../src/intel/gpgpu/operations/worklists.rs#L1)
3. Calibration requires deltas of `0`, `16`, and `256` for the empty,
   merged, and strip workloads. A missing snapshot, wrong delta, pixel
   mismatch, or timeout fails the command. An ambiguous retirement uses
   the normal system-lane quarantine and retains its backing. Attribution
   under a forced GuC context switch is a separate validation step that
   requires evidence of an intervening dispatch between snapshots.
4. Timing trials use only GPU timestamps and a separate CS stall before
   each post-sync timestamp write for Alder Lake. The command warms both
   variants, alternates AB and BA, swaps backing buffers, and reports
   median and p95 over 24 pairs. The band grows to 32 rows if the warm-up
   hits timestamp resolution at four rows. Host timings start before
   descriptor preparation and end when retirement is proven.
   [GPU timestamp writer](../../src/intel/gpgpu/rcs/commands.rs#L58),
   [Mesa post-sync ordering workaround](https://chromium.googlesource.com/external/gitlab.freedesktop.org/mesa/mesa/+/7c634b2375af2c145c185666841364c533bafc2d/src/intel/vulkan/genX_cmd_buffer.c#3089)

## Runtime decision

The lane delta measures dispatch work. The timestamps establish whether the
chip finishes the same pixel work sooner. A production merge is worth pursuing
if real rectangle lists repeat the time saving after CPU merge costs are
included. Include lists with few merge opportunities to measure the cost of
looking for merges. Start with consecutive, opaque, same-color solid
rectangles whose union exactly covers a rectangle. Require pixel parity and
no regression in p95 host time for the low-merge workload.

Run `python3 tools/testpy/test_rectangle_dispatch.py` for the host packet,
snapshot, and pixel-reference checks. Run the physical diagnostic after
deploying a kernel with the command to establish the rig's lane counts,
output parity, and timing.
