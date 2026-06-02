# TRUEOS notes from BA2019 Luetke Dreimann

Source PDF: `/home/t4ce/Downloads/BA2019_Luetke_Dreimann.pdf`

Title: `Ein Treiber fuer die native Codeausfuehrung auf Intel GPUs fuer den MxKernel`

These notes keep the thesis facts that matter for TRUEOS GPU bring-up.

## Core Thesis

The driver is deliberately small because it only targets native GPGPU execution.
It does not attempt to be a display, 3D, video, or general Linux-style graphics
driver.

The required ownership is:

- MMIO access to the GPU registers.
- A translation table mapping CPU memory to GPU graphics addresses.
- A command streamer path through ringbuffer and optional batchbuffer.
- GPGPU state setup.
- Per-task memory/state preparation.
- Interrupt or polling completion.

## Memory Model

The thesis chooses the global GTT only for the implementation. The stated reason
is that the global table is sufficient for the required commands, while
per-process translation tables add complexity without visible benefit for this
minimal driver.

Important memory areas for GPGPU:

- Ring buffer and optional batch buffer.
- General State Memory.
- Dynamic State Memory.
- Instruction Memory.
- Surface Memory.
- Indirect Object Memory.

The General State Memory is not used for the thesis GPGPU tasks, but it is still
allocated because the Intel docs do not clearly guarantee it can be omitted.
This is a useful TRUEOS hint: boring base regions may be part of the implicit
hardware contract even when a given task does not consume them.

Dynamic State Memory contains Interface Descriptor entries.
Instruction Memory contains compiled GPU code.
Surface Memory contains Render Surface State entries plus the binding table.
Indirect Object Memory contains cross-thread data and thread payload data.

## Ringbuffer Contract

The ringbuffer is the main command streamer tool.

Key details:

- `MI_MODE` stops/starts the command streamer.
- `RING_BUFFER_START` points at a GGTT-mapped ring buffer address.
- `RING_BUFFER_CTL` programs size and valid/enable state.
- `RING_BUFFER_TAIL` and `RING_BUFFER_HEAD` are offsets, not raw pointers.
- Tail must be QWORD aligned.
- Head equals tail only proves commands were read from the ring, not that the GPU
  operation completed correctly.
- ESR bit 0 indicates instruction error; ACTHD can identify the command address.

Batchbuffers are used to prepare command sequences without mutating the ring.
The ring starts them with `MI_BATCH_BUFFER_START`; the batch ends with
`MI_BATCH_BUFFER_END`.

## Initialization Sequence

The thesis splits GPU init into:

1. Configure interrupts.
2. Initialize GTT.
3. Force-wake the GPU.
4. Initialize ringbuffer.
5. Select/update GPGPU state.

Forcewake doctrine:

- GPU starts in RC6-like low-power state.
- Some MMIO registers are unavailable until forcewake is active.
- Clear forcewake bits first.
- Set thread 0 active.
- Poll forcewake status until active.

GPGPU state init sequence:

1. `PIPELINE_SELECT`
2. `PIPE_CONTROL`
3. `MEDIA_VFE_STATE`
4. `PIPE_CONTROL`

The thesis explicitly says this state init must complete before the GPU can be
used.

## Per-Task Preparation

Task preparation consists of:

1. Calculate execution parameters.
2. Allocate/map memory areas in GTT.
3. Create Render Surface State entries and binding table.
4. Create Interface Descriptor in Dynamic Memory.
5. Fill Indirect Data Memory.
6. Fill batchbuffer with launch commands.

Indirect Data Memory has two parts:

- Cross-thread data. The thesis says this was not documented by Intel and was
  discovered by debugging Intel NEO.
- Per-thread payload IDs for the first workgroup, laid out in SIMD-width blocks:
  X block, Y block, Z block.

The hardware derives later workgroups from first-workgroup payload and group
size.

## Per-Task Batch Shape

The thesis gives the GPGPU launch batch as:

1. `MEDIA_STATE_FLUSH`
2. `MEDIA_INTERFACE_DESCRIPTOR_LOAD`
3. `GPGPU_WALKER`
4. `MEDIA_STATE_FLUSH`
5. `PIPE_CONTROL`

Meaning:

- `MEDIA_STATE_FLUSH` makes state memory entries current.
- `MEDIA_INTERFACE_DESCRIPTOR_LOAD` loads the dynamic-state interface descriptor.
- `GPGPU_WALKER` starts execution and blocks the command streamer while the
  program runs.
- The post-walker flush makes GPU results visible to CPU.
- Final `PIPE_CONTROL` can trigger interrupt after memory visibility is
  established.

The program start outside the batch is:

1. `STATE_BASE_ADDRESS`, with `PIPE_CONTROL` before and after.
2. `MI_BATCH_BUFFER_START`.
3. `MI_NOOP` for ring QWORD alignment.
4. Submit ring without waiting when using interrupt-driven completion.

## TRUEOS Implications

For TRUEOS compute/GPGPU, this thesis is a minimal-driver oracle:

- Prefer a tight launch window around `MEDIA_STATE_FLUSH`,
  `MEDIA_INTERFACE_DESCRIPTOR_LOAD`, and `GPGPU_WALKER`.
- Treat base memory areas as contractual unless proven otherwise.
- Do not confuse ring read completion with GPU/EU task completion.
- Completion proof should be either interrupt/post-sync visibility or a strong
  store/counter proof.
- If GPGPU launch fails, inspect in this order:
  - forcewake / MMIO availability,
  - graphics address mapping,
  - state base addresses and sizes,
  - interface descriptor,
  - surface state and binding table,
  - indirect/cross-thread data,
  - walker dimensions and masks,
  - post-walker flush/visibility.

For TRUEOS 3D raster, the thesis is not a direct packet recipe. It still gives a
discipline:

- isolate the command window,
- remove nonessential markers between state load and launch,
- make base/state memory contracts boring and complete,
- add error-state capture early because wrong GPU config can hang without useful
  feedback.

