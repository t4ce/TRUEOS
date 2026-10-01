# Owned asynchronous WD screenshots on BCS0

An explicit `shot` request copies the completed 2560×1440 XYUV8888 WD target
into an owned DMA allocation using `XY_FAST_COPY_BLT`. Each copy is 14,745,600
bytes. The old static snapshot array and CPU `copy_nonoverlapping` are removed.
XYUV8888-to-RGBA conversion and PNG encoding still run on the CPU.

## Ownership and ordering

1. The WD completion owner claims the requested snapshot. Without a request,
   this function does no allocation, BCS work, or asynchronous waiting.
2. A lease pins the exact completed WD physical allocation and sequence. WD
   cannot rearm or free that target while the lease exists.
3. Allocate the destination and flush/invalidate recycled CPU cache lines
   before DMA writes. Map both source and destination UC in the BCS PPGTT.
   Private aliases `0x30000000` and `0x31000000` fit the existing 1 GiB BCS
   address envelope; they do not change WD's display GGTT mapping.
4. Use the existing vcpy/GuC queue and its pre-copy TLB invalidation. Admission
   retries yield at 1 ms and have a 250 ms limit. Completion uses the existing
   BCS timeout, ordered post-sync marker and saved-context retirement proof.
5. Completion polling yields at 1 ms. After retirement, release the source
   lease, acquire the destination for CPU access, and publish the owned snapshot.
6. The screenshot worker converts the owned image, then frees its raw backing.
   PNG persistence retains its existing RGBA allocation and file workflow.

Standalone shots await the copy before WD teardown. A live H.264 capture awaits
it before proceeding with the borrowed frame. This introduces a bounded pause
only for explicitly requested shots; it is not a new streaming frame copy.

Cancellation or ambiguous submission/completion retains both DMA allocations.
WD cannot rearm or free the source, and further screenshot requests are rejected
until reboot. Rejection before submission frees the destination normally. There
is no CPU-copy fallback. Request reply targets remain queued until an actual
ready or failed result, so asynchronous polling cannot discard the `shot` reply.

## Measurements

`vcpy status` reports completed WD snapshot copies, bytes, failures and the last
sample's timings. The bare-metal log records each completed copy, conversion,
and PNG save. These values are expressed in microseconds, but the configured
monotonic clock has 1 ms resolution (`tick-hz-1_000`):

- `prepare_us`: source lease, destination allocation and pre-DMA cache flush.
- `admission_us`: admission retries and submission CPU work.
- `submit_us`: CPU time inside queue calls, **included in** `admission_us`.
- `retire_us`: host-observed time from successful queue return to retirement;
  includes scheduling and 1 ms polling, not pure GPU execution time.
- `acquire_us`: releasing the source lease and acquiring destination CPU caches.
- `request_to_ready_us`: request to CPU-readable raw snapshot, including WD
  capture and worker scheduling. It excludes conversion, encoding and storage.
- `conversion_us`, `encode_us`, `write_us`: subsequent screenshot stages.

These measurements locate costs. They do not establish a speedup over the old
CPU copy or Gridpaper's compute reference. A performance comparison needs the
same workload and geometry, warm-up, multiple samples, and matched reference
measurements. For Gridpaper, include both copy latency and total frame time;
retirement counters and visible output establish functionality only.

## Host checks

- `cargo check --bin TRUEOS` and `cargo build --bin TRUEOS`.
- `tools/testpy/test_wd_bcs_snapshot.py`: production async snapshot lifecycle,
  busy admission, single consumption, allocation failure, cancellation before
  and after submission, ambiguous failure, and quarantine admission.
- `tools/testpy/test_rdp_pipeline.py`: existing streaming lifecycle plus real
  WD lease and teardown exclusion tests.
- `tools/testpy/test_tgl_bcs_packets.py`: packet and mapping checks, including
  UC device-written sources while retaining WB for ordinary copy sources.
- `tools/testpy/test_vcpy_consumer.py`: existing Gridpaper consumer regressions.

## Physical rig result

Runtime SHA-256:
`4dc5042d917ac425f6f7e552e7c161b51ae193afc2cc00b10c1fdd7fd8ce3f4b`.
`reset-receipt.json` verifies the ELF and a fresh PXE read. The ISO was not rebuilt.

Eight shots completed: 117,964,800 bytes, zero snapshot failures. Five were
standalone captures; two borrowed hardware H.264 recording frames (WD sequences
425 and 495); one ran after recording stopped and WD was released. Recording
was stopped after validation. Gridpaper continued successfully, with one
pre-submission compute fallback while the shared BCS lane was busy.

All eight requests produced stored-file acknowledgements. Two downloaded PNGs
passed decoding checks (2560×1440 RGBA); the first visibly contains the complete
Gridpaper greeting. No byte-for-byte GPU/CPU reference comparison was performed.

Seven samples with full stage logs, including the two streaming shots:

| Host-observed stage | Median | Range |
| --- | ---: | ---: |
| Allocation and pre-copy cache preparation | 13 ms | 13–14 ms |
| Admission, including CPU submission | 7 ms | 7 ms |
| BCS retirement wait | 3 ms | 2–35 ms |
| CPU acquisition | 13 ms | 13–15 ms |
| Request to readable raw snapshot | 69 ms | 45–142 ms |

The 35 ms retirement outlier occurred during the startup workload. These few
samples do not identify its cause or establish tail-latency statistics.
Conversion took 61–69 ms, PNG encoding 127–135 ms and file writing 19–62 ms.
Consequently the GPU copy wait is usually a small part of this screenshot
pipeline. Cache preparation/acquisition are worthwhile next profiling targets;
this experiment does not demonstrate an overall speedup against CPU copying.

Artifacts under `bld/artifacts/wd-bcs-snapshot/`:

- `boot-and-shots.log`, `live-stream-shots.log`, `final-status.txt`,
  `final-shell.txt`, and `reset-receipt.json`.
- `measurements.json`: seven recorded copy samples and stage summaries.
- `screenshot-1.png`: visible Gridpaper greeting copied through BCS.
- `screenshot-2.png`: a later successfully decoded screenshot.

The 24 relevant host tests passed: 8 packet/mapping, 14 stream/lease/teardown,
1 asynchronous snapshot lifecycle, and 1 existing Gridpaper consumer test.
Build and check passed with the existing 275 kernel warnings.
