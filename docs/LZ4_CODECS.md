# LZ4 block service and tar/LZ4 archives

The reusable operation is standard LZ4 block compression/decompression. It has
no filenames, filesystem access, archive metadata, or application state.
`src/r/lz4.rs::blocks` accepts owned input blocks, explicit output capacities,
and encode/decode selection. Archive creation, asset loading and compressed
caches can all call this same kernel primitive. Frame handling is a separate
layer; tar is a separate container above it.

## Archive behavior

The existing Blueprint `trueos::archive::{pack, pack_many, unpack}` API remains
asynchronous. A pack destination ending in `.lz4` selects a standard LZ4 frame
containing a POSIX tar archive; use `.tar.lz4`. Other destination names retain
7z. Unpack detects the input magic, regardless of its filename. Termdir's
archive action now creates `.tar.lz4` and recognizes both `.tar.lz4` and `.7z`
for extraction. Termdir polls the operation future once per UI frame and reports
completion in its log; archive work no longer enters a UI-blocking `block_on`.
One operation per explorer is admitted at a time; navigation remains available.

Files are sorted for deterministic output. PAX `path` carries long UTF-8 names;
optional `TRUEOS.content_type` preserves the native content identity. Standard
tar readers can ignore that field. Files from external tar archives without it
use the existing legacy Blob admission rules, including the prohibition on
downgrading an existing typed destination. All member names and content types
are checked before any restore writes. Links, devices, sparse files, global
PAX records and external LZ4 dictionaries are rejected. Directory entries are
accepted but empty directories are not restored, matching the existing
file-oriented archive service. Concatenated/skippable LZ4 frames are rejected.

LZ4 frames support header, optional block and optional content checksums,
content sizes, and block maxima from 64 KiB to 4 MiB. Independent blocks can use
the GPU. Linked blocks use the bounded CPU pool. New frames use independent
4 KiB blocks inside the standard 64 KiB maximum-block frame profile, include
content length and checksum, and store incompressible blocks verbatim.

## Execution and ownership

The native Intel artifact is the pinned ADL-S 0x4680/revision 0x0c image, SIMD16,
zero scratch and zero SLM. One SIMD lane owns an independent block; compression
uses an interleaved caller-owned hash table. New archives expose enough small
blocks for parallelism across the GT. The shared block API admits at most 256
blocks and 5 MiB each of input and output capacities per batch. Encode accepts
blocks up to 64 KiB; decode accepts blocks up to 4 MiB.

The codec has a distinct GuC client, HWLRCA, ring, PPGTT, timeline and persistent
DMA arena. It runs at normal kernel GPU priority. One async coordinator owns
that arena at a time. No spin mutex is held across an await, and CPU workers
are not occupied polling the GPU. The backend polls retirement at 1 ms
intervals and requires both the producer marker and saved ring head at the
published tail. It then retires the exact vGPU submission. A dropped in-flight
waiter, ambiguous submission or timeout quarantines this private context and
retains its backing; memory is never recycled under a late GPU.

Unsupported GPUs use the shared CPU compute pool, capped at four. A failure of
an admitted GPU operation is reported, rather than silently accepting its
output. Future requests can use the CPU once the failed context is quarantined.
The existing archive C ABI exposes the result as the usual operation handle
and report. Other kernel services can call the block/frame API directly.

Storage logs separate encode, decode and filesystem restore elapsed time.
The earlier filesystem fix removes whole-log namespace replay from every
indexed file write; codec acceleration alone cannot fix that storage overhead.

## Validation

- `python3 tools/test_lz4_gpu.py`: runs the production shader as native C++
  against liblz4 under ASan/UBSan, including 20,000 malformed inputs and lane
  isolation. Requires g++ and liblz4.so.1, no development header.
- `python3 tools/test_lz4_archive.py`: production Rust frame/tar code versus
  liblz4 and Python tarfile, including linked frames, long PAX paths, corruption,
  resource bounds and 25 files totaling 2.5 MB.
- `python3 tools/test_lz4_opencl.py`: production SPIR-V on a local Intel OpenCL
  GPU. Select a local ICD with `OCL_ICD_VENDORS` when necessary. This is separate
  from the TRUEOS hardware rig and does not boot or mutate that rig.
- `python3 tools/test_termdir_archive.py`: runs the actual UI polling and duplicate
  admission test with abort-on-use stubs for unavailable kernel imports.
- `tools/intel-gpu-bakery/bake_adls_cpp_lz4.sh`: reproducible pinned bake.
- `python3 tools/intel-gpu-bakery/verify.py --bin crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/lz4_blocks.bin`:
  compiler-free source/hash/ABI verification.

On the local Raptor Lake-S UHD 770 (0xa780/revision 04), the production SPIR-V
compiled by the Intel OpenCL driver passed GPU/reference cross-checks. For
256 x 4 KiB blocks, observed GPU execution was 1.5–1.8 ms for decoding 1 MiB of
output and 2.0–6.8 ms for encoding 1 MiB of input (repeated bytes, repeated text,
and random input). These are single-run smoke measurements, not throughput
claims for the native TRUEOS backend. The local test does not validate the
pinned ADL-S native binary, GuC command encoding, cancellation, or filesystem
latency on the TRUEOS rig. That end-to-end hardware validation remains required.
