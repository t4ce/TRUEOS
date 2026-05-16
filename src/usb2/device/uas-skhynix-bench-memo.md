# SK hynix UAS Historical Bench Memo

Current target: SK hynix PSSD X31 `152E:7001` on the UAS-specific path. TRUEOSFS root now mounts normally; the old shell UAS benchmark path has been removed and is no longer part of the runtime workflow.

## Known State

- Read path is stable at `1 MiB` chunks with low inflight. Recent runs are about `0.2-0.4 GB/s`.
- Write path became valid after fixing xHCI chained TD metadata. The first useful stable point was `1 MiB x1`, about `80-90 MB/s`.
- Current conservative statement: raw SK hynix UAS reads work at about `200-400 MB/s`; raw writes use the known fastest validated default, `1 MiB x1`, which has shown about `80-90 MB/s`.
- Parallel write is not proven faster yet. `1 MiB` with higher write inflight did not give a reliable 2x win and can disturb the next write attempt.
- Larger single WRITE(10) transfers above `1 MiB` are not proven safe. `2 MiB` produced `Io`/timeouts and can poison later writes.
- `64 KiB` is no longer the real hardware limit. It was a symptom of our lower-layer transfer handling.

## Known Bad Turns

- Do not defer TRUEOSFS root readiness for SK hynix UAS anymore. The bench phase is over; the disk should be public through the normal root mount.
- Do not reintroduce ramdisk/preflight writes to the SK hynix path. Keep write experiments out of the normal mount path.
- Do not assume bigger single WRITE(10) is the next speed lever. `2 MiB` already failed.
- Do not prearm final status immediately after `WRITE_READY` in the windowed writer unless a new lower-layer change justifies retesting. On this bridge it timed out before `WRITE_READY` on a previously-good `1 MiB x1` probe.

## If / Then Map

- If `1 MiB x1` fails near `7-8 MiB`, restore the last known-good windowed write choreography before testing speed.
- If `1 MiB x1` still fails after the known-good choreography is restored, undo the bounce-copy optimization and retest. That isolates DMA/bounce behavior from UAS protocol behavior. This branch was hit: the failing log showed `ready=false`, `data_pending=false`, and `status_pending=true`, so the device had not issued `WRITE_READY`; the working tree restores map-time OUT bounce contents for the next run.
- If `1 MiB x1` is stable but `1 MiB x2/x4/x8` is not faster, suspect per-transfer overhead: bounce/copy/cache flush, TRUEOSFS finish/sync, or status latency.
- If write timing says `fill_ms` is large, prefill/reuse the bench buffer before judging disk speed.
- If write timing says `finish_ms` is large, split data-write speed from TRUEOSFS commit/sync speed.
- If write timing says `data_ms` dominates, stay in UAS/xHCI/DMA. The next candidates are bounce-buffer policy, cache maintenance, and stream completion handling.
- If a probe times out or returns `Io`, reset the SK hynix UAS transport before any final write or next probe.
- If read speed regresses together with write speed, look below SCSI first: xHCI events, DMA mapping, cache flushes, or unrelated system load.

## Next Steps

1. Boot normally and confirm TRUEOSFS root mounts on `transport=uas-skhynix`.
2. If `1 MiB x1` still times out before `WRITE_READY`, inspect the UAS write command/data/status setup below SCSI.
3. If read and write both regress in the same run, do not chase WRITE(10) first; check xHCI event flow, DMA mapping, and global logging/load.
