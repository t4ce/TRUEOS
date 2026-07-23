# Single-kernel C++ audiovisual instrument

`cpp_audio_visualizer_rgba8.clcpp` is one composed C++ for OpenCL kernel, not
a collection of interchangeable effects. It renders a live, aspect-correct
instrument from the final stereo stream accepted by the HDA output path:

- two independently colored waveform ribbons;
- a mid/side stereo phase constellation;
- a 64-band logarithmic spectrum architecture;
- a circular waveform/spectrum prism;
- bass bloom, onset rings, and high-frequency particles;
- restrained scan texture, peak light, and audio-driven palette motion.

The default Shell2 launch is continuous at 20 Hz:

```text
cpp audio
cpp status
cpp stop
```

`av` and `visualizer` are accepted aliases. The long form is:

```text
cpp start audio [duration_ms] [cadence_ms] [publish_every]
cpp start audio 0 50 1
```

The UI4 window starts at the ordinary C++ demo extent. All C++ demo windows
now use application interaction, so a UI4 maximize/restore notification
replaces the backing double-buffered frame at the actual requested extent.
On the 2560x1440 TestRig, maximizing therefore dispatches a native 1440p
surface without a hard-coded monitor assumption in the kernel.

## PCM and analysis boundary

The source is the exact interleaved signed-16-bit stereo slice accepted by
`HdaPcmStream::push_interleaved_samples`, after the active software sources
have already been mixed and limited and immediately before the HDA DMA-ring
copy. The tee is non-consuming:

```text
active playback sources
  -> final 48 kHz stereo s16 mix
  -> atomic 4096-frame monitor ring
  -> unchanged HDA DMA copy and speaker playback
```

The HDA producer performs no allocation, lock acquisition, FFT, or wait. When
the visualizer is stopped, one atomic flag check is the complete tap cost.
The UI4 producer snapshots 2048 frames into preallocated storage and performs
ordinary Symphonia radix-2 mid/side FFTs outside the callback. It derives:

- 128 interpolated samples for each waveform;
- 64 attack/release-smoothed logarithmic bands from 35 Hz to 18 kHz;
- left/right RMS, peak, stereo width, low/mid/high energy;
- spectral centroid and positive flux;
- a bounded onset/beat envelope and resetting tempo phase.

PrismQ/QFT is deliberately not used. A 16-qubit amplitude state is a
65,536-value transform, adds about 1.37 seconds of 48 kHz input span before
encoding/measurement concerns, and does not improve this classical streaming
spectrum. The existing fixed-size FFT is deterministic, bounded, allocation
free after initialization, and keeps the audio callback untouched.

## Snapshot ABI

One 4096-byte DMA page is rewritten and flushed before dispatch:

| Words | Contents |
| --- | --- |
| `0..7` | `AVZ1`, version, sequence, flags, 48000 Hz, 64 bands, 128 wave points |
| `8..19` | twelve scalar audio features |
| `32..287` | 128 interleaved `float` waveform pairs |
| `320..383` | 64 normalized `float` spectrum bands |

The kernel has two BTIs: read-only snapshot at BTI 0 and read/write RGBA8
destination at BTI 1. Pitch, width, height, time, frame, and flags are scalar
payload arguments.

## Bounded walker shape

One global x lane shades a horizontal pair of output pixels. The y dimension
remains one lane per row:

```text
walker lanes = ceil(width / 2) * height
2560x1440   = 1,843,200 lanes
full pixel  = 3,686,400 lanes
ratio       = 0.5000
```

This is the requested estimated 50% walker envelope, not a claim that every
lane costs the same as a minimal pixel shader. The default 50 ms cadence adds
temporal headroom and avoids turning the demo into a stress test. The kernel
uses one dispatch, SIMD16, 128 GRFs, and no scratch or SLM.

## Artifact and publication

The exact `8086:4680`, revision `0x0c` artifact is:

```text
kernel:           cpp_audio_visualizer_rgba8
Zebin SHA-256:    951e0cb30b42a755812b00eb0c3871f52c765ee74295dc3cb48b84f8361c1b19
SPIR-V SHA-256:   548a2917924e80c4ce77f4fa6b5b5e754a4ad84976bb707823e603ad6750bc97
Zebin bytes:      77592
SPIR-V bytes:     57800
entry offset:     64
entry bytes:      14520
execution:        SIMD16, 128 GRFs, scratch 0, SLM 0
payload:          96 cross-thread bytes + 96 local-ID bytes
bindings:         arg0/BTI0 read-only, arg1/BTI1 read-write
```

Rebuild and compiler-free verification are separate:

```sh
make intel-gpu-bake-audio-visualizer-cpp
make intel-gpu-verify-cpp-artifacts
```

`make kernel` and `make iso` additionally require the complete visualizer
Zebin in the linked and ISO-extracted runtime ELF. C++, Clang, `llvm-spirv`,
`ocloc`, IGC, OpenCL loaders, and compiler runtimes are absent from TRUEOS.

The offline composition-review lane is:

```sh
make -C tools/cpp-audio-visualizer-offline render
```

With an OpenCL GPU it loads the production SPIR-V through
`clCreateProgramWithIL` and profiles five post-warm-up dispatches. In a
container without `/dev/dri`, it emits an explicitly labeled CPU-reference
PNG and never presents that fallback as GPU or performance evidence.

## TestRig promotion

Boot `bld/trueos.iso` on the i5-14500T TestRig at `00:02.0`,
`8086:4680`, revision `0x0c`, leave the normal audio playback active, then:

```text
cpp audio
cpp status
```

Maximize and restore the UI4 window. Promotion evidence should include:

- continuous unchanged speaker playback;
- `pcm_tap=1`, an advancing PCM sequence, and non-zero feature values;
- `resident=1 verified=1`;
- advancing attempted/submitted/completed/published counts;
- post-marker `0xC0DEA902` and no late/failure growth;
- applied resize logs for both maximize and restore;
- visual response to channel width, bass onset, vocal/mid energy, and hats;
- submit time that remains below the configured 50 ms cadence at 2560x1440.

The linked artifact, ABI, and composition are host-validated. Those physical
audio, display, and timing observations remain the final hardware gate.
