use crate::disc::block::DeviceHandle;
use crate::usb2::pen::{UasRouteProbeConfig, UasRouteProbeResult};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

const WAIT_FOR_DISK_MS: u64 = 30_000;
const WAIT_POLL_MS: u64 = 100;
const READ_1MIB_BYTES: usize = 1024 * 1024;

const PROBE_ENABLED: bool = crate::allcaps::probes::UAS_SKHYNIX_ROUTE_BOOT_PROBE;
const WRITE_ENABLED: bool = crate::allcaps::storage::USB_MASS_UAS_SKHYNIX_ROUTE_PROBE_WRITE_ENABLED;
const WRITE_X2: bool = crate::allcaps::storage::USB_MASS_UAS_SKHYNIX_ROUTE_PROBE_WRITE_X2;
const CHUNK_BYTES: usize = crate::allcaps::storage::USB_MASS_UAS_SKHYNIX_ROUTE_PROBE_CHUNK_BYTES;
const MAX_INFLIGHT: usize = crate::allcaps::storage::USB_MASS_UAS_SKHYNIX_ROUTE_PROBE_MAX_INFLIGHT;
const WRITE_LBA: u64 = crate::allcaps::storage::USB_MASS_UAS_SKHYNIX_ROUTE_PROBE_WRITE_LBA;

#[derive(Clone, Copy)]
enum ProbeOpKind {
    Read,
    Write,
    Skip,
}

#[derive(Clone, Copy)]
struct ProbeOp {
    name: &'static str,
    kind: ProbeOpKind,
    lba: u64,
    bytes: usize,
    result: &'static str,
}

pub(crate) fn enabled() -> bool {
    PROBE_ENABLED
}

fn now_ms_since(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

fn find_uas_skhynix_route_disk() -> Option<DeviceHandle> {
    crate::usb2::pen::find_uas_skhynix_route_disk()
}

fn blocks_for_bytes(block_size: usize, bytes: usize) -> usize {
    if block_size == 0 || bytes == 0 {
        return 0;
    }
    (bytes / block_size).max(1)
}

fn clamp_probe_bytes(disk: DeviceHandle, wanted_bytes: usize) -> usize {
    let info = disk.info();
    let block_size = info.block_size as usize;
    if block_size == 0 || info.block_count == 0 {
        return 0;
    }

    let max_transfer = info.max_transfer_bytes.max(info.block_size as u64) as usize;
    let wanted = wanted_bytes.min(max_transfer).max(block_size);
    let blocks = blocks_for_bytes(block_size, wanted).min(info.block_count as usize);
    blocks.saturating_mul(block_size)
}

fn write_lba_is_safe(disk: DeviceHandle, lba: u64, bytes: usize) -> bool {
    if lba == 0 {
        return false;
    }
    let block_size = disk.block_size() as usize;
    if block_size == 0 || bytes == 0 || !bytes.is_multiple_of(block_size) {
        return false;
    }
    let blocks = (bytes / block_size) as u64;
    lba.checked_add(blocks)
        .map(|end| end <= disk.block_count())
        .unwrap_or(false)
}

async fn reset_after_failure(disk: DeviceHandle, stage: &'static str) {
    match crate::usb2::pen::reset_uas_skhynix_route_transport(disk, stage).await {
        Ok(()) => crate::log!("uas-skhynix-route-probe: reset stage={} result=ok\n", stage),
        Err(err) => {
            crate::log!("uas-skhynix-route-probe: reset stage={} result=err err={:?}\n", stage, err)
        }
    }
}

fn route_op_enabled(op: ProbeOp) -> bool {
    matches!(op.kind, ProbeOpKind::Read | ProbeOpKind::Write)
}

fn log_route_result(op: ProbeOp, result: UasRouteProbeResult) {
    let status = if result.error.is_some() { "err" } else { "ok" };
    crate::log!(
        "uas-skhynix-route-probe: op={} phase={:?} fill_ms={} command_ms={} ready_ms={} data_ms={} status_ms={} reclaim_ms={} finish_ms={} chunk_bytes={} inflight={} lba={} bytes={} result={} err={:?} submitted={} reclaimed={} stalled={} resets={} quarantined={} bytes_submitted={} bytes_reclaimed={} max_observed_inflight={} live_streams={} dead_streams={}\n",
        op.name,
        result.phase,
        result.timing.fill_ms,
        result.timing.command_ms,
        result.timing.ready_ms,
        result.timing.data_ms,
        result.timing.status_ms,
        result.timing.reclaim_ms,
        result.timing.finish_ms,
        result.chunk_bytes,
        result.max_inflight,
        result.lba,
        result.total_bytes,
        status,
        result.error,
        result.counters.submitted,
        result.counters.reclaimed,
        result.counters.stalled,
        result.counters.resets,
        result.counters.quarantined,
        result.counters.bytes_submitted,
        result.counters.bytes_reclaimed,
        result.counters.max_inflight,
        result.counters.live_streams,
        result.counters.dead_streams
    );
}

async fn run_route_op(disk: DeviceHandle, op: ProbeOp, chunk_bytes: usize, inflight: usize) {
    if !route_op_enabled(op) {
        crate::log!(
            "uas-skhynix-route-probe: op={} fill_ms=0 command_ms=0 ready_ms=0 data_ms=0 status_ms=0 reclaim_ms=0 finish_ms=0 chunk_bytes={} inflight={} lba={} bytes={} result={}\n",
            op.name,
            chunk_bytes,
            inflight,
            op.lba,
            op.bytes,
            op.result
        );
        return;
    }

    let config = UasRouteProbeConfig {
        lba: op.lba,
        total_bytes: op.bytes as u64,
        chunk_bytes,
        max_inflight: inflight,
    };

    match crate::usb2::pen::run_uas_skhynix_route_probe(disk, config).await {
        Ok(result) => {
            let failed = result.error.is_some();
            log_route_result(op, result);
            if failed {
                reset_after_failure(disk, op.name).await;
            }
        }
        Err(err) => {
            crate::log!(
                "uas-skhynix-route-probe: op={} fill_ms=0 command_ms=0 ready_ms=0 data_ms=0 status_ms=0 reclaim_ms=0 finish_ms=0 chunk_bytes={} inflight={} lba={} bytes={} result=err err={:?}\n",
                op.name,
                chunk_bytes,
                inflight,
                op.lba,
                op.bytes,
                err
            );
            reset_after_failure(disk, op.name).await;
        }
    }
}

#[embassy_executor::task(pool_size = 4)]
async fn route_probe_op_task(disk: DeviceHandle, op: ProbeOp, chunk_bytes: usize, inflight: usize) {
    run_route_op(disk, op, chunk_bytes, inflight).await;
}

async fn wait_for_uas_skhynix_route_disk() -> Option<DeviceHandle> {
    let start = Instant::now();
    loop {
        if let Some(disk) = find_uas_skhynix_route_disk() {
            return Some(disk);
        }
        if now_ms_since(start) >= WAIT_FOR_DISK_MS {
            return None;
        }
        Timer::after(EmbassyDuration::from_millis(WAIT_POLL_MS)).await;
    }
}

fn build_write_op(disk: DeviceHandle, name: &'static str, bytes: usize) -> ProbeOp {
    if !WRITE_ENABLED {
        return ProbeOp {
            name,
            kind: ProbeOpKind::Skip,
            lba: WRITE_LBA,
            bytes,
            result: "skipped-write-disabled",
        };
    }

    if !write_lba_is_safe(disk, WRITE_LBA, bytes) {
        return ProbeOp {
            name,
            kind: ProbeOpKind::Skip,
            lba: WRITE_LBA,
            bytes,
            result: "skipped-no-safe-write-lba",
        };
    }

    ProbeOp {
        name,
        kind: ProbeOpKind::Write,
        lba: WRITE_LBA,
        bytes,
        result: "pending",
    }
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn boot_uas_skhynix_route_probe_task() {
    Timer::after(EmbassyDuration::from_millis(1)).await;

    let Some(disk) = wait_for_uas_skhynix_route_disk().await else {
        crate::log!(
            "uas-skhynix-route-probe: result=skipped reason=no-uas-skhynix-disk wait_ms={}\n",
            WAIT_FOR_DISK_MS
        );
        return;
    };

    let info = disk.info();
    let chunk_bytes = clamp_probe_bytes(disk, CHUNK_BYTES);
    let inflight = MAX_INFLIGHT.max(1);
    if chunk_bytes == 0 {
        crate::log!(
            "uas-skhynix-route-probe: result=skipped reason=bad-block-info disk={} bs={} blocks={}\n",
            info.id.raw(),
            info.block_size,
            info.block_count
        );
        return;
    }

    if WRITE_ENABLED {
        match crate::usb2::pen::set_uas_skhynix_route_window(disk, chunk_bytes, inflight).await {
            Ok((actual_chunk, actual_inflight)) => crate::log!(
                "uas-skhynix-route-probe: configure-write result=ok chunk_bytes={} inflight={}\n",
                actual_chunk,
                actual_inflight
            ),
            Err(err) => crate::log!(
                "uas-skhynix-route-probe: configure-write result=err chunk_bytes={} inflight={} err={:?}\n",
                chunk_bytes,
                inflight,
                err
            ),
        }
    } else {
        crate::log!(
            "uas-skhynix-route-probe: configure-write result=skipped write_enabled=0 chunk_bytes={} inflight={}\n",
            chunk_bytes,
            inflight
        );
    }

    crate::log!(
        "uas-skhynix-route-probe: start disk={} label={} bs={} blocks={} max_xfer={} write_enabled={} write_x2={} write_lba={} chunk_bytes={} inflight={}\n",
        info.id.raw(),
        info.label.as_deref().unwrap_or("-"),
        info.block_size,
        info.block_count,
        info.max_transfer_bytes,
        WRITE_ENABLED as u8,
        WRITE_X2 as u8,
        WRITE_LBA,
        chunk_bytes,
        inflight
    );

    let small_bytes = info.block_size.max(1) as usize;
    let read_1mib_bytes = clamp_probe_bytes(disk, READ_1MIB_BYTES);
    let write_1mib_bytes = clamp_probe_bytes(disk, READ_1MIB_BYTES);
    let write_x2_bytes = clamp_probe_bytes(disk, READ_1MIB_BYTES.saturating_mul(2));

    let ops = [
        ProbeOp {
            name: "read-small",
            kind: ProbeOpKind::Read,
            lba: 0,
            bytes: small_bytes,
            result: "pending",
        },
        ProbeOp {
            name: "read-1mib",
            kind: ProbeOpKind::Read,
            lba: 0,
            bytes: read_1mib_bytes,
            result: "pending",
        },
        build_write_op(disk, "write-1mib-x1", write_1mib_bytes),
        if WRITE_X2 {
            build_write_op(disk, "write-1mib-x2", write_x2_bytes)
        } else {
            ProbeOp {
                name: "write-1mib-x2",
                kind: ProbeOpKind::Skip,
                lba: WRITE_LBA,
                bytes: write_x2_bytes,
                result: "skipped-x2-disabled",
            }
        },
    ];

    let spawner = unsafe { Spawner::for_current_executor().await };
    for op in ops {
        match route_probe_op_task(disk, op, chunk_bytes, inflight) {
            Ok(token) => spawner.spawn(token),
            Err(err) => crate::log!(
                "uas-skhynix-route-probe: op={} result=spawn-err err={:?}\n",
                op.name,
                err
            ),
        }
    }
}
