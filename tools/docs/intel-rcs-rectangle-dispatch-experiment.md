# Intel RCS rectangle dispatch experiment

Test whether merging adjacent solid rectangles reduces the work dispatched on
RCS0 and saves time for an identical RGBA8 result. Add a manual Shell3
`gpu test compute-dispatch` command using the offscreen solid-rectangle path
behind `gpu test compute`. Start with one workload and the existing kernel
artifact. [Shell3 compute test](../../src/shell3/system/gpu.rs#L427),
[solid-rectangle path](../../src/intel/gpgpu/operations/solid_rects.rs#L29)

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

1. Record the physical PCI device ID and revision, effective driver profile,
   and kernel artifact hash. Admit only a device and stepping with verified
   access to the RCS counter and a working solid-rectangle kernel. Treat an
   unadmitted target as unsupported before encoding register access. The
   Tiger Lake reference defines `0x2290` and `0x2294` as the two halves of a
   64-bit count of enabled lanes at dispatch. Writes to the low half reset
   the counter. [Intel register reference, p. 272](https://cdrdv2-public.intel.com/703046/intel-gfx-prm-osrc-tgl-vol-02-c-command-reference-registers-part-1.pdf#page=298)
2. Take ordered, read-only register snapshots before and after the walker
   in the same batch. Use the Picasso register-to-memory snapshot pattern
   and an owned result mapping. Keep the full counter width and dedicate
   aligned result slots that cannot overlap timestamps or completion
   markers. Hold the scratch and descriptor ownership through readback.
   Require the existing completion marker and saved-HEAD retirement proof
   before reading results. [Snapshot pattern](../../src/intel/render/primary.rs#L2435),
   [existing worklist submission](../../src/intel/gpgpu/operations/worklists.rs#L1)
3. Calibrate on a quiet engine. A batch that skips the walker should add
   zero lanes, and a single full-mask group should add sixteen. Verify
   the A/B counts, then repeat with a known compute workload on another
   GuC context. Confirm the other context ran between the snapshots before
   using that trial as evidence of attribution across a context switch.
   A timeout or incorrect delta fails calibration. Preserve the existing
   quarantine behavior for an accepted submission with ambiguous retirement.
4. Measure timing in separate timestamp-only trials. Bracket the walker
   with the existing GPU timestamp packet, and measure host elapsed time
   from before descriptor preparation through retirement as well. Warm up
   both variants, alternate AB and BA, and report median and p95 GPU ticks
   and host times over repeated pairs. Increase the band height equally
   for both variants if the small case falls below timestamp resolution.
   [GPU timestamp writer](../../src/intel/gpgpu/rcs/commands.rs#L58)

## Runtime decision

The lane delta measures dispatch work. The timestamps establish whether the
chip finishes the same pixel work sooner. A production merge is worth pursuing
if real rectangle lists repeat the time saving after CPU merge costs are
included. Include lists with few merge opportunities to measure the cost of
looking for merges. Start with consecutive, opaque, same-color solid
rectangles whose union exactly covers a rectangle. Require pixel parity and
no regression in p95 host time for the low-merge workload.

The rebuild and deployment checkpoint is after the opt-in command passes
host-side packet, slot layout, and pixel-reference checks. The physical run
then establishes counter attribution, output parity, and timing on the rig.
